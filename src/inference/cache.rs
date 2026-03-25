//! LRU model cache with memory-aware eviction.

use super::model::{ModelId, ModelInfo};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// A loaded model ready for inference.
pub struct LoadedModel {
    /// Model information
    pub info: ModelInfo,
    /// When the model was loaded
    pub loaded_at: Instant,
    /// Last time the model was used
    pub last_used: Instant,
    /// Reference count for concurrent usage
    pub ref_count: usize,
    /// Handle to the actual model (opaque, backend-specific)
    pub handle: ModelHandle,
}

/// Opaque handle to backend-specific model state.
/// This will be implemented differently for each backend (llama.cpp, candle, etc.)
pub enum ModelHandle {
    /// Placeholder for now - will be replaced with actual backend handles
    Placeholder,
    #[cfg(feature = "local-inference")]
    LlamaCpp {
        // Will hold llama_cpp model context
        _marker: std::marker::PhantomData<()>,
    },
}

/// LRU cache for loaded models.
pub struct ModelCache {
    /// Loaded models by ID
    models: Arc<RwLock<HashMap<ModelId, Arc<RwLock<LoadedModel>>>>>,
    /// Maximum number of models to keep loaded
    max_models: usize,
    /// Maximum total memory usage in MB
    max_memory_mb: u32,
    /// Current memory usage estimate in MB
    current_memory_mb: Arc<RwLock<u32>>,
}

impl ModelCache {
    /// Create a new model cache.
    pub fn new(max_models: usize, max_memory_mb: u32) -> Self {
        Self {
            models: Arc::new(RwLock::new(HashMap::new())),
            max_models,
            max_memory_mb,
            current_memory_mb: Arc::new(RwLock::new(0)),
        }
    }

    /// Get a loaded model by ID.
    pub async fn get(&self, model_id: &str) -> Option<Arc<RwLock<LoadedModel>>> {
        let models = self.models.read().await;
        if let Some(model) = models.get(model_id) {
            // Update last_used
            let mut model_guard = model.write().await;
            model_guard.last_used = Instant::now();
            model_guard.ref_count += 1;
            drop(model_guard);
            return Some(model.clone());
        }
        None
    }

    /// Release a reference to a model.
    pub async fn release(&self, model_id: &str) {
        let models = self.models.read().await;
        if let Some(model) = models.get(model_id) {
            let mut model_guard = model.write().await;
            model_guard.ref_count = model_guard.ref_count.saturating_sub(1);
        }
    }

    /// Insert a loaded model into the cache.
    pub async fn insert(&self, model: LoadedModel) -> Result<(), CacheError> {
        let model_id = model.info.id.clone();
        let model_memory = model.info.estimate_ram_mb();

        // Check if we need to evict
        self.ensure_capacity(model_memory).await?;

        let model = Arc::new(RwLock::new(model));
        {
            let mut models = self.models.write().await;
            models.insert(model_id, model);
        }

        // Update memory tracking
        {
            let mut current = self.current_memory_mb.write().await;
            *current += model_memory;
        }

        Ok(())
    }

    /// Ensure we have capacity for a model of the given size.
    async fn ensure_capacity(&self, needed_mb: u32) -> Result<(), CacheError> {
        let current = *self.current_memory_mb.read().await;
        let models_count = self.models.read().await.len();

        // Check model count limit
        if models_count >= self.max_models {
            self.evict_lru().await?;
        }

        // Check memory limit
        if current + needed_mb > self.max_memory_mb {
            self.evict_until_fits(needed_mb).await?;
        }

        Ok(())
    }

    /// Evict the least recently used model.
    async fn evict_lru(&self) -> Result<(), CacheError> {
        let mut models = self.models.write().await;

        // Find LRU model that isn't in use
        let lru_id = {
            let mut candidates: Vec<_> = Vec::new();

            for (id, model) in models.iter() {
                let guard = model.read().await;
                if guard.ref_count == 0 {
                    candidates.push((id.clone(), guard.last_used));
                }
            }

            candidates
                .into_iter()
                .min_by_key(|(_, last_used)| *last_used)
                .map(|(id, _)| id)
        };

        if let Some(id) = lru_id {
            if let Some(model) = models.remove(&id) {
                let model_guard = model.read().await;
                let memory = model_guard.info.estimate_ram_mb();
                drop(model_guard);

                let mut current = self.current_memory_mb.write().await;
                *current = current.saturating_sub(memory);

                tracing::info!(model_id = %id, "Evicted model from cache");
            }
            Ok(())
        } else {
            Err(CacheError::AllModelsInUse)
        }
    }

    /// Evict models until we have enough space.
    async fn evict_until_fits(&self, needed_mb: u32) -> Result<(), CacheError> {
        loop {
            let current = *self.current_memory_mb.read().await;
            if current + needed_mb <= self.max_memory_mb {
                return Ok(());
            }

            let models_count = self.models.read().await.len();
            if models_count == 0 {
                return Err(CacheError::InsufficientMemory {
                    needed: needed_mb,
                    available: self.max_memory_mb.saturating_sub(current),
                });
            }

            self.evict_lru().await?;
        }
    }

    /// Check if a model is loaded.
    pub async fn is_loaded(&self, model_id: &str) -> bool {
        self.models.read().await.contains_key(model_id)
    }

    /// Get list of loaded model IDs.
    pub async fn loaded_models(&self) -> Vec<ModelId> {
        self.models.read().await.keys().cloned().collect()
    }

    /// Get current memory usage.
    pub async fn memory_usage_mb(&self) -> u32 {
        *self.current_memory_mb.read().await
    }

    /// Get number of loaded models.
    pub async fn model_count(&self) -> usize {
        self.models.read().await.len()
    }

    /// Clear the cache, unloading all models.
    pub async fn clear(&self) -> Result<(), CacheError> {
        let mut models = self.models.write().await;

        // Check if any models are in use
        for model in models.values() {
            let guard = model.read().await;
            if guard.ref_count > 0 {
                return Err(CacheError::AllModelsInUse);
            }
        }

        models.clear();
        *self.current_memory_mb.write().await = 0;

        Ok(())
    }
}

/// Errors from cache operations.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("All models are currently in use")]
    AllModelsInUse,

    #[error("Insufficient memory: need {needed}MB, only {available}MB available")]
    InsufficientMemory { needed: u32, available: u32 },

    #[error("Model not found: {0}")]
    ModelNotFound(ModelId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_model(id: &str, size_mb: u32) -> LoadedModel {
        LoadedModel {
            info: ModelInfo {
                id: id.to_string(),
                name: id.to_string(),
                path: PathBuf::from(format!("/models/{}.gguf", id)),
                size_bytes: size_mb as u64 * 1_000_000,
                architecture: super::super::model::ModelArchitecture::Llama,
                quantization: super::super::model::Quantization::Q4_K_M,
                parameters_billions: 7.0,
                context_length: 4096,
                hash: None,
            },
            loaded_at: Instant::now(),
            last_used: Instant::now(),
            ref_count: 0,
            handle: ModelHandle::Placeholder,
        }
    }

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = ModelCache::new(10, 100_000);

        let model = make_test_model("test-model", 4000);
        cache.insert(model).await.unwrap();

        assert!(cache.is_loaded("test-model").await);

        let loaded = cache.get("test-model").await;
        assert!(loaded.is_some());
    }

    #[tokio::test]
    async fn test_cache_lru_eviction() {
        let cache = ModelCache::new(2, 100_000); // Max 2 models

        cache
            .insert(make_test_model("model-1", 4000))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache
            .insert(make_test_model("model-2", 4000))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // This should evict model-1 (oldest)
        cache
            .insert(make_test_model("model-3", 4000))
            .await
            .unwrap();

        assert!(!cache.is_loaded("model-1").await);
        assert!(cache.is_loaded("model-2").await);
        assert!(cache.is_loaded("model-3").await);
    }
}
