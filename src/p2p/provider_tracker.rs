//! Tracks discovered LLM providers on the P2P network.
//!
//! Maintains an in-memory map of peer → provider manifest, updated from
//! GossipSub advertisements. Used by the executor to find remote inference providers.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::provider::{ModelOffering, ProviderManifest};

/// Maximum age of a provider manifest before it's considered stale (10 minutes).
const MANIFEST_MAX_AGE_SECS: u64 = 600;

/// Tracks all known LLM providers on the network.
pub struct ProviderTracker {
    /// Known providers: peer_id → manifest
    providers: Arc<RwLock<HashMap<String, ProviderManifest>>>,
    /// Local sharing config
    local_config: Arc<RwLock<crate::config::ProviderSharingConfig>>,
    /// Usage tracking: peer_id → requests made this hour
    usage_counts: Arc<RwLock<HashMap<String, UsageCounter>>>,
}

/// Tracks usage for rate limiting.
#[derive(Debug, Clone, Default)]
struct UsageCounter {
    requests_this_hour: u32,
    tokens_this_day: u64,
    hour_start: u64,
    day_start: u64,
}

impl UsageCounter {
    fn reset_if_needed(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Reset hourly counter
        if now - self.hour_start >= 3600 {
            self.requests_this_hour = 0;
            self.hour_start = now;
        }

        // Reset daily counter
        if now - self.day_start >= 86400 {
            self.tokens_this_day = 0;
            self.day_start = now;
        }
    }
}

impl ProviderTracker {
    /// Create a new provider tracker.
    pub fn new(config: crate::config::ProviderSharingConfig) -> Self {
        Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            local_config: Arc::new(RwLock::new(config)),
            usage_counts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Update or add a provider from a received manifest.
    pub async fn update_provider(&self, manifest: ProviderManifest) {
        // Don't store expired manifests
        if manifest.is_expired(MANIFEST_MAX_AGE_SECS) {
            return;
        }

        let peer_id = manifest.peer_id.clone();
        self.providers.write().await.insert(peer_id, manifest);
    }

    /// Remove stale providers.
    pub async fn prune_stale(&self) {
        self.providers
            .write()
            .await
            .retain(|_, m| !m.is_expired(MANIFEST_MAX_AGE_SECS));
    }

    /// Find providers that offer a specific model.
    pub async fn find_providers(&self, model_name: &str) -> Vec<(String, ModelOffering)> {
        let providers = self.providers.read().await;
        let mut results = Vec::new();

        for manifest in providers.values() {
            if let Some(offering) = manifest.find_model(model_name) {
                results.push((manifest.peer_id.clone(), offering.clone()));
            }
        }

        // Sort by price (cheapest first)
        results.sort_by_key(|(_, o)| o.price_per_1k_tokens);
        results
    }

    /// Get all known providers.
    pub async fn all_providers(&self) -> Vec<ProviderManifest> {
        self.providers.read().await.values().cloned().collect()
    }

    /// Get provider count.
    pub async fn provider_count(&self) -> usize {
        self.providers.read().await.len()
    }

    /// Check if we can make a request to a specific provider (rate limiting).
    pub async fn can_request(&self, peer_id: &str) -> bool {
        let providers = self.providers.read().await;
        let Some(manifest) = providers.get(peer_id) else {
            return false;
        };

        let mut usage = self.usage_counts.write().await;
        let counter = usage.entry(peer_id.to_string()).or_default();
        counter.reset_if_needed();

        counter.requests_this_hour < manifest.rate_limits.max_requests_per_hour
            && counter.tokens_this_day < manifest.rate_limits.max_tokens_per_day
    }

    /// Record a request made to a provider.
    pub async fn record_usage(&self, peer_id: &str, tokens: u32) {
        let mut usage = self.usage_counts.write().await;
        let counter = usage.entry(peer_id.to_string()).or_default();
        counter.reset_if_needed();
        counter.requests_this_hour += 1;
        counter.tokens_this_day += tokens as u64;
    }

    /// Get the current local sharing config.
    pub async fn local_config(&self) -> crate::config::ProviderSharingConfig {
        self.local_config.read().await.clone()
    }

    /// Update the local sharing config.
    pub async fn set_local_config(&self, config: crate::config::ProviderSharingConfig) {
        *self.local_config.write().await = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::provider::{ProviderBackend, ProviderRateLimits};

    fn test_manifest(peer_id: &str, model: &str) -> ProviderManifest {
        ProviderManifest::new(
            peer_id.to_string(),
            vec![ModelOffering {
                model_name: model.to_string(),
                context_size: 4096,
                price_per_1k_tokens: 100,
                max_tokens_per_request: 2048,
                quantization: None,
                backend: ProviderBackend::Ollama,
            }],
            ProviderRateLimits::default(),
        )
    }

    #[tokio::test]
    async fn test_provider_tracking() {
        let tracker = ProviderTracker::new(crate::config::ProviderSharingConfig::default());

        tracker
            .update_provider(test_manifest("peer-1", "llama3.2:3b"))
            .await;
        tracker
            .update_provider(test_manifest("peer-2", "mistral:7b"))
            .await;

        assert_eq!(tracker.provider_count().await, 2);

        let llama_providers = tracker.find_providers("llama3.2").await;
        assert_eq!(llama_providers.len(), 1);
        assert_eq!(llama_providers[0].0, "peer-1");
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let tracker = ProviderTracker::new(crate::config::ProviderSharingConfig::default());
        tracker
            .update_provider(test_manifest("peer-1", "llama3.2:3b"))
            .await;

        assert!(tracker.can_request("peer-1").await);
        tracker.record_usage("peer-1", 100).await;
        assert!(tracker.can_request("peer-1").await);
    }
}
