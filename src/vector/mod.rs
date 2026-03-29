//! Vector database module using vectX.
//!
//! Provides semantic vector search for memories, documents, and embeddings
//! using vectX's HNSW indexing and SIMD-optimized similarity search.

pub mod embeddings;

use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// Re-export vectX types for convenience
pub use vectx::{
    Collection, CollectionConfig, Distance, Filter, PayloadFilter, Point, PointId, Vector,
};

// Re-export embeddings
pub use embeddings::{get_embedder, init_embedder, Embedder, EmbeddingConfig, EmbeddingProvider};

/// Default embedding dimension (compatible with most sentence transformers)
pub const DEFAULT_EMBEDDING_DIM: usize = 384;

/// Error type for vector operations
#[derive(Debug, Error)]
pub enum VectorError {
    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Collection already exists: {0}")]
    CollectionExists(String),

    #[error("Invalid vector dimension: expected {expected}, got {actual}")]
    InvalidDimension { expected: usize, actual: usize },

    #[error("vectX error: {0}")]
    VectxError(#[from] vectx::Error),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Embedding error: {0}")]
    EmbeddingError(String),
}

pub type Result<T> = std::result::Result<T, VectorError>;

/// Configuration for VectorStore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStoreConfig {
    /// Default embedding dimension
    pub embedding_dim: usize,
    /// Use HNSW index for large collections
    pub use_hnsw: bool,
    /// Enable BM25 text search
    pub enable_bm25: bool,
    /// Distance metric
    pub distance: DistanceMetric,
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            embedding_dim: DEFAULT_EMBEDDING_DIM,
            use_hnsw: true,
            enable_bm25: true,
            distance: DistanceMetric::Cosine,
        }
    }
}

/// Distance metric for similarity search
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum DistanceMetric {
    #[default]
    Cosine,
    Euclidean,
    DotProduct,
}

impl From<DistanceMetric> for Distance {
    fn from(m: DistanceMetric) -> Self {
        match m {
            DistanceMetric::Cosine => Distance::Cosine,
            DistanceMetric::Euclidean => Distance::Euclidean,
            DistanceMetric::DotProduct => Distance::Dot,
        }
    }
}

/// Convert a vectx Distance back to our serializable DistanceMetric.
fn distance_to_metric(d: Distance) -> DistanceMetric {
    match d {
        Distance::Cosine => DistanceMetric::Cosine,
        Distance::Euclidean => DistanceMetric::Euclidean,
        Distance::Dot => DistanceMetric::DotProduct,
    }
}

/// Search result with score and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Point ID
    pub id: String,
    /// Similarity score (0.0 to 1.0 for cosine)
    pub score: f32,
    /// Optional payload/metadata
    pub payload: Option<serde_json::Value>,
    /// Original text (if stored)
    pub text: Option<String>,
}

impl SearchResult {
    /// Create from vectX Point and score
    fn from_point(point: &Point, score: f32) -> Self {
        let text = point
            .payload
            .as_ref()
            .and_then(|p| p.get("text"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            id: point.id.to_string(),
            score,
            payload: point.payload.clone(),
            text,
        }
    }
}

/// Serializable snapshot of a single collection (metadata + all points).
/// Used for JSON persistence since vectx's CollectionConfig / Distance
/// do not derive Serialize/Deserialize.
#[derive(Debug, Serialize, Deserialize)]
struct CollectionSnapshot {
    name: String,
    vector_dim: usize,
    distance: DistanceMetric,
    use_hnsw: bool,
    enable_bm25: bool,
    points: Vec<Point>,
}

/// Vector store managing multiple collections
pub struct VectorStore {
    config: VectorStoreConfig,
    collections: Arc<RwLock<std::collections::HashMap<String, Arc<Collection>>>>,
    storage_path: Option<std::path::PathBuf>,
}

impl VectorStore {
    /// Create a new in-memory vector store
    pub fn new(config: VectorStoreConfig) -> Self {
        Self {
            config,
            collections: Arc::new(RwLock::new(std::collections::HashMap::new())),
            storage_path: None,
        }
    }

    /// Create vector store with persistence
    pub fn with_storage(config: VectorStoreConfig, path: &Path) -> Result<Self> {
        // Create storage directory if it doesn't exist
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| VectorError::StorageError(e.to_string()))?;
        }

        let store = Self {
            config,
            collections: Arc::new(RwLock::new(std::collections::HashMap::new())),
            storage_path: Some(path.to_path_buf()),
        };

        // Load existing collections
        store.load_collections()?;

        Ok(store)
    }

    /// Load collections from storage by reading every `*.json` file in the
    /// storage directory, deserializing the snapshot, and re-creating the
    /// in-memory Collection with all its points.
    fn load_collections(&self) -> Result<()> {
        let storage = match &self.storage_path {
            Some(p) => p,
            None => return Ok(()),
        };

        let entries = std::fs::read_dir(storage)
            .map_err(|e| VectorError::StorageError(format!("read_dir failed: {e}")))?;

        let mut collections = self.collections.write();

        for entry in entries {
            let entry =
                entry.map_err(|e| VectorError::StorageError(format!("dir entry error: {e}")))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let data = std::fs::read(&path)
                .map_err(|e| VectorError::StorageError(format!("read {}: {e}", path.display())))?;

            let snap: CollectionSnapshot = serde_json::from_slice(&data).map_err(|e| {
                VectorError::StorageError(format!("deserialize {}: {e}", path.display()))
            })?;

            let cfg = CollectionConfig {
                name: snap.name.clone(),
                vector_dim: snap.vector_dim,
                distance: snap.distance.into(),
                use_hnsw: snap.use_hnsw,
                enable_bm25: snap.enable_bm25,
            };

            let collection = Collection::new(cfg);
            // Batch-insert all points for efficiency (rebuilds indexes once)
            if !snap.points.is_empty() {
                collection.batch_upsert(snap.points).map_err(|e| {
                    VectorError::StorageError(format!(
                        "batch_upsert for collection '{}': {e}",
                        snap.name
                    ))
                })?;
            }

            tracing::info!(
                collection = %snap.name,
                count = collection.count(),
                "Loaded collection from disk"
            );
            collections.insert(snap.name, Arc::new(collection));
        }

        Ok(())
    }

    /// Persist a single collection to disk as an atomic JSON write
    /// (write to a temp file then rename).
    fn save_collection(&self, name: &str) -> Result<()> {
        let storage = match &self.storage_path {
            Some(p) => p,
            None => return Ok(()),
        };

        let col = {
            let collections = self.collections.read();
            collections
                .get(name)
                .cloned()
                .ok_or_else(|| VectorError::CollectionNotFound(name.to_string()))?
        };

        let snap = CollectionSnapshot {
            name: name.to_string(),
            vector_dim: col.vector_dim(),
            distance: distance_to_metric(col.distance()),
            use_hnsw: col.use_hnsw(),
            enable_bm25: col.enable_bm25(),
            points: col.get_all_points(),
        };

        let json = serde_json::to_vec(&snap)
            .map_err(|e| VectorError::StorageError(format!("serialize '{}': {e}", name)))?;

        let final_path = storage.join(format!("{name}.json"));
        let tmp_path = storage.join(format!(".{name}.json.tmp"));

        std::fs::write(&tmp_path, &json)
            .map_err(|e| VectorError::StorageError(format!("write tmp: {e}")))?;
        std::fs::rename(&tmp_path, &final_path)
            .map_err(|e| VectorError::StorageError(format!("rename: {e}")))?;

        Ok(())
    }

    /// Remove the on-disk file for a collection.
    fn remove_collection_file(&self, name: &str) {
        if let Some(storage) = &self.storage_path {
            let path = storage.join(format!("{name}.json"));
            let _ = std::fs::remove_file(path);
        }
    }

    /// Create a new collection
    pub fn create_collection(&self, name: &str) -> Result<()> {
        self.create_collection_with_dim(name, self.config.embedding_dim)
    }

    /// Create a new collection with specific dimension
    pub fn create_collection_with_dim(&self, name: &str, dim: usize) -> Result<()> {
        let mut collections = self.collections.write();

        if collections.contains_key(name) {
            return Err(VectorError::CollectionExists(name.to_string()));
        }

        let config = CollectionConfig {
            name: name.to_string(),
            vector_dim: dim,
            distance: self.config.distance.into(),
            use_hnsw: self.config.use_hnsw,
            enable_bm25: self.config.enable_bm25,
        };

        let collection = Collection::new(config);
        collections.insert(name.to_string(), Arc::new(collection));
        // Release the write lock before saving to avoid holding it during I/O
        drop(collections);

        self.save_collection(name)?;

        tracing::info!(collection = name, dim = dim, "Created vector collection");
        Ok(())
    }

    /// Get or create a collection
    pub fn get_or_create_collection(&self, name: &str) -> Result<Arc<Collection>> {
        {
            let collections = self.collections.read();
            if let Some(col) = collections.get(name) {
                return Ok(col.clone());
            }
        }

        self.create_collection(name)?;
        let collections = self.collections.read();
        Ok(collections.get(name).unwrap().clone())
    }

    /// Delete a collection (and its on-disk file if persistence is enabled)
    pub fn delete_collection(&self, name: &str) -> Result<bool> {
        let removed = {
            let mut collections = self.collections.write();
            collections.remove(name).is_some()
        };
        if removed {
            self.remove_collection_file(name);
        }
        Ok(removed)
    }

    /// List all collections
    pub fn list_collections(&self) -> Vec<CollectionInfo> {
        let collections = self.collections.read();
        collections
            .iter()
            .map(|(name, col)| CollectionInfo {
                name: name.clone(),
                count: col.count(),
                dimension: col.vector_dim(),
            })
            .collect()
    }

    /// Insert a vector with payload
    pub fn upsert(
        &self,
        collection: &str,
        id: &str,
        vector: Vec<f32>,
        payload: Option<serde_json::Value>,
    ) -> Result<()> {
        let col = self.get_collection(collection)?;

        // Validate dimension
        if vector.len() != col.vector_dim() {
            return Err(VectorError::InvalidDimension {
                expected: col.vector_dim(),
                actual: vector.len(),
            });
        }

        let point = Point::new(
            PointId::String(id.to_string()),
            Vector::new(vector),
            payload,
        );

        col.upsert(point)?;
        self.save_collection(collection)?;
        Ok(())
    }

    /// Insert text with automatic embedding (placeholder - requires embedding model)
    pub fn upsert_text(
        &self,
        collection: &str,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        extra_payload: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut payload = serde_json::json!({
            "text": text,
        });

        if let Some(extra) = extra_payload {
            if let (Some(obj), Some(extra_obj)) = (payload.as_object_mut(), extra.as_object()) {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        self.upsert(collection, id, embedding, Some(payload))
    }

    /// Search for similar vectors
    pub fn search(
        &self,
        collection: &str,
        query: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;
        let query_vec = Vector::new(query);

        let results = col.search(&query_vec, limit, None);

        Ok(results
            .into_iter()
            .map(|(point, score)| SearchResult::from_point(&point, score))
            .collect())
    }

    /// Search with filter
    pub fn search_with_filter(
        &self,
        collection: &str,
        query: Vec<f32>,
        limit: usize,
        filter: &PayloadFilter,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;
        let query_vec = Vector::new(query);

        let results = col.search(&query_vec, limit, Some(filter));

        Ok(results
            .into_iter()
            .map(|(point, score)| SearchResult::from_point(&point, score))
            .collect())
    }

    /// Full-text search (BM25)
    pub fn search_text(
        &self,
        collection: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;
        let results = col.search_text(query, limit);

        // Get full points for results
        Ok(results
            .into_iter()
            .filter_map(|(id, score)| {
                col.get(&id)
                    .map(|point| SearchResult::from_point(&point, score))
            })
            .collect())
    }

    /// Hybrid search combining vector and text
    pub fn hybrid_search(
        &self,
        collection: &str,
        query_vector: Vec<f32>,
        query_text: &str,
        limit: usize,
        vector_weight: f32,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;

        // Vector search
        let query_vec = Vector::new(query_vector);
        let vector_results = col.search(&query_vec, limit * 2, None);

        // Text search
        let text_results = col.search_text(query_text, limit * 2);

        // Combine results with weighted scoring
        let mut combined: std::collections::HashMap<String, f32> = std::collections::HashMap::new();

        let text_weight = 1.0 - vector_weight;

        for (point, score) in &vector_results {
            let id = point.id.to_string();
            *combined.entry(id).or_insert(0.0) += score * vector_weight;
        }

        for (id, score) in &text_results {
            *combined.entry(id.clone()).or_insert(0.0) += score * text_weight;
        }

        // Sort by combined score
        let mut results: Vec<_> = combined.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Get full points
        Ok(results
            .into_iter()
            .take(limit)
            .filter_map(|(id, score)| {
                col.get(&id)
                    .map(|point| SearchResult::from_point(&point, score))
            })
            .collect())
    }

    /// Get a point by ID
    pub fn get(&self, collection: &str, id: &str) -> Result<Option<SearchResult>> {
        let col = self.get_collection(collection)?;
        Ok(col
            .get(id)
            .map(|point| SearchResult::from_point(&point, 1.0)))
    }

    /// Delete a point (and persist the change if storage is enabled)
    pub fn delete(&self, collection: &str, id: &str) -> Result<bool> {
        let col = self.get_collection(collection)?;
        col.delete(id)?;
        self.save_collection(collection)?;
        Ok(true)
    }

    /// Get collection count
    pub fn count(&self, collection: &str) -> Result<usize> {
        let col = self.get_collection(collection)?;
        Ok(col.count())
    }

    /// Get a collection by name
    fn get_collection(&self, name: &str) -> Result<Arc<Collection>> {
        let collections = self.collections.read();
        collections
            .get(name)
            .cloned()
            .ok_or_else(|| VectorError::CollectionNotFound(name.to_string()))
    }
}

/// Information about a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub name: String,
    pub count: usize,
    pub dimension: usize,
}

/// Global vector store instance
static VECTOR_STORE: std::sync::LazyLock<RwLock<Option<Arc<VectorStore>>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// Initialize the global vector store
pub fn init_vector_store(config: VectorStoreConfig) {
    let mut store = VECTOR_STORE.write();
    *store = Some(Arc::new(VectorStore::new(config)));
}

/// Initialize the global vector store with persistence
pub fn init_vector_store_with_storage(config: VectorStoreConfig, path: &Path) -> Result<()> {
    let vector_store = VectorStore::with_storage(config, path)?;
    let mut store = VECTOR_STORE.write();
    *store = Some(Arc::new(vector_store));
    Ok(())
}

/// Get the global vector store
pub fn get_vector_store() -> Option<Arc<VectorStore>> {
    let store = VECTOR_STORE.read();
    store.clone()
}

/// Install a default in-memory vectx [`VectorStore`] in the process-global slot if unset.
///
/// Used by `serve` and by [`resolve_vector_store`] so HTTP and flows (e.g. `file_search`) always
/// have a store without a separate init step.
pub fn get_or_init_vector_store() -> Arc<VectorStore> {
    let mut guard = VECTOR_STORE.write();
    if let Some(s) = guard.as_ref() {
        return s.clone();
    }
    let s = Arc::new(VectorStore::new(VectorStoreConfig::default()));
    *guard = Some(s.clone());
    s
}

/// Prefer an explicit handle (e.g. [`crate::web::WebState::vector_store`]), then the global slot
/// from [`init_vector_store`], then [`get_or_init_vector_store`].
pub fn resolve_vector_store(explicit: Option<Arc<VectorStore>>) -> Arc<VectorStore> {
    match explicit {
        Some(s) => s,
        None => get_vector_store().unwrap_or_else(get_or_init_vector_store),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_init_vector_store_is_idempotent() {
        let a = super::get_or_init_vector_store();
        let b = super::get_or_init_vector_store();
        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn test_create_collection() {
        let store = VectorStore::new(VectorStoreConfig::default());
        store.create_collection("test").unwrap();

        let collections = store.list_collections();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "test");
    }

    #[test]
    fn test_upsert_and_search() {
        let store = VectorStore::new(VectorStoreConfig {
            embedding_dim: 4,
            ..Default::default()
        });
        store.create_collection_with_dim("test", 4).unwrap();

        // Insert vectors
        store
            .upsert(
                "test",
                "vec1",
                vec![1.0, 0.0, 0.0, 0.0],
                Some(serde_json::json!({"text": "hello"})),
            )
            .unwrap();
        store
            .upsert(
                "test",
                "vec2",
                vec![0.0, 1.0, 0.0, 0.0],
                Some(serde_json::json!({"text": "world"})),
            )
            .unwrap();
        store
            .upsert(
                "test",
                "vec3",
                vec![0.9, 0.1, 0.0, 0.0],
                Some(serde_json::json!({"text": "hi"})),
            )
            .unwrap();

        // Search for similar to vec1
        let results = store.search("test", vec![1.0, 0.0, 0.0, 0.0], 2).unwrap();

        assert_eq!(results.len(), 2);
        // vec1 should be most similar to itself
        assert_eq!(results[0].id, "vec1");
        // vec3 should be second (0.9 similarity)
        assert_eq!(results[1].id, "vec3");
    }

    #[test]
    fn test_text_search() {
        let store = VectorStore::new(VectorStoreConfig {
            embedding_dim: 4,
            enable_bm25: true,
            ..Default::default()
        });
        store.create_collection_with_dim("test", 4).unwrap();

        // Insert with text
        store
            .upsert_text(
                "test",
                "doc1",
                "The quick brown fox jumps over the lazy dog",
                vec![1.0, 0.0, 0.0, 0.0],
                None,
            )
            .unwrap();
        store
            .upsert_text(
                "test",
                "doc2",
                "A lazy cat sleeps on the couch",
                vec![0.0, 1.0, 0.0, 0.0],
                None,
            )
            .unwrap();

        // Search for "lazy"
        let results = store.search_text("test", "lazy", 10).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_delete() {
        let store = VectorStore::new(VectorStoreConfig {
            embedding_dim: 4,
            ..Default::default()
        });
        store.create_collection_with_dim("test", 4).unwrap();

        store
            .upsert("test", "vec1", vec![1.0, 0.0, 0.0, 0.0], None)
            .unwrap();

        assert_eq!(store.count("test").unwrap(), 1);

        store.delete("test", "vec1").unwrap();

        assert_eq!(store.count("test").unwrap(), 0);
    }

    #[test]
    fn test_persistence_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // --- Phase 1: create store, insert data, drop store ---
        {
            let store = VectorStore::with_storage(
                VectorStoreConfig {
                    embedding_dim: 4,
                    use_hnsw: false,
                    enable_bm25: true,
                    distance: DistanceMetric::Cosine,
                },
                path,
            )
            .unwrap();

            store.create_collection_with_dim("memories", 4).unwrap();

            store
                .upsert(
                    "memories",
                    "m1",
                    vec![1.0, 0.0, 0.0, 0.0],
                    Some(serde_json::json!({"text": "hello world", "tag": "greeting"})),
                )
                .unwrap();
            store
                .upsert(
                    "memories",
                    "m2",
                    vec![0.0, 1.0, 0.0, 0.0],
                    Some(serde_json::json!({"text": "goodbye", "tag": "farewell"})),
                )
                .unwrap();

            assert_eq!(store.count("memories").unwrap(), 2);
            // store is dropped here
        }

        // --- Phase 2: reload from disk, verify data survived ---
        {
            let store = VectorStore::with_storage(
                VectorStoreConfig {
                    embedding_dim: 4,
                    use_hnsw: false,
                    enable_bm25: true,
                    distance: DistanceMetric::Cosine,
                },
                path,
            )
            .unwrap();

            // Collection should exist
            let collections = store.list_collections();
            assert_eq!(collections.len(), 1);
            assert_eq!(collections[0].name, "memories");
            assert_eq!(collections[0].count, 2);
            assert_eq!(collections[0].dimension, 4);

            // Points should be retrievable
            let m1 = store.get("memories", "m1").unwrap().expect("m1 missing");
            assert_eq!(m1.id, "m1");
            assert_eq!(m1.text.as_deref(), Some("hello world"));
            let payload = m1.payload.unwrap();
            assert_eq!(payload["tag"], "greeting");

            let m2 = store.get("memories", "m2").unwrap().expect("m2 missing");
            assert_eq!(m2.id, "m2");

            // Vector search should work on reloaded data
            let results = store
                .search("memories", vec![1.0, 0.0, 0.0, 0.0], 1)
                .unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, "m1");
        }

        // --- Phase 3: delete a point, reload, verify deletion persisted ---
        {
            let store = VectorStore::with_storage(
                VectorStoreConfig {
                    embedding_dim: 4,
                    use_hnsw: false,
                    enable_bm25: true,
                    distance: DistanceMetric::Cosine,
                },
                path,
            )
            .unwrap();

            store.delete("memories", "m1").unwrap();
            assert_eq!(store.count("memories").unwrap(), 1);
        }

        // Reload again
        {
            let store = VectorStore::with_storage(
                VectorStoreConfig {
                    embedding_dim: 4,
                    use_hnsw: false,
                    enable_bm25: true,
                    distance: DistanceMetric::Cosine,
                },
                path,
            )
            .unwrap();

            assert_eq!(store.count("memories").unwrap(), 1);
            assert!(store.get("memories", "m1").unwrap().is_none());
            assert!(store.get("memories", "m2").unwrap().is_some());
        }

        // --- Phase 4: delete the collection, reload, verify file is gone ---
        {
            let store = VectorStore::with_storage(
                VectorStoreConfig {
                    embedding_dim: 4,
                    use_hnsw: false,
                    enable_bm25: true,
                    distance: DistanceMetric::Cosine,
                },
                path,
            )
            .unwrap();

            assert!(store.delete_collection("memories").unwrap());
            // File should be removed
            assert!(!path.join("memories.json").exists());
        }

        // Reload - should be empty
        {
            let store = VectorStore::with_storage(
                VectorStoreConfig {
                    embedding_dim: 4,
                    use_hnsw: false,
                    enable_bm25: true,
                    distance: DistanceMetric::Cosine,
                },
                path,
            )
            .unwrap();

            assert!(store.list_collections().is_empty());
        }
    }
}
