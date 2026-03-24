//! LLM provider sharing protocol.
//!
//! Peers can share their LLM provider capacity (Ollama, GGUF, OpenAI-compatible)
//! with the network. Other peers pay CLAW tokens to use remote LLMs.

use serde::{Deserialize, Serialize};

/// GossipSub topic for provider advertisements.
pub const PROVIDER_TOPIC: &str = "peerclaw/providers/v1";

/// A provider's advertisement of available LLM models and rate limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderManifest {
    /// The advertising peer's ID
    pub peer_id: String,
    /// Available model offerings
    pub models: Vec<ModelOffering>,
    /// Rate limits for this provider
    pub rate_limits: ProviderRateLimits,
    /// Unix timestamp when this manifest was created
    pub timestamp: u64,
    /// Ed25519 signature of the manifest
    pub signature: Vec<u8>,
}

impl ProviderManifest {
    /// Create a new unsigned provider manifest.
    pub fn new(peer_id: String, models: Vec<ModelOffering>, rate_limits: ProviderRateLimits) -> Self {
        Self {
            peer_id,
            models,
            rate_limits,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: vec![],
        }
    }

    /// Check if this manifest is expired (older than `max_age_secs`).
    pub fn is_expired(&self, max_age_secs: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.timestamp) > max_age_secs
    }

    /// Get the bytes to sign.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.peer_id.as_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes.extend_from_slice(&rmp_serde::to_vec(&self.models).unwrap_or_default());
        bytes.extend_from_slice(&rmp_serde::to_vec(&self.rate_limits).unwrap_or_default());
        bytes
    }

    /// Sign the manifest.
    pub fn sign<F>(&mut self, signer: F)
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let bytes = self.signing_bytes();
        self.signature = signer(&bytes);
    }

    /// Check if this provider offers a given model.
    pub fn has_model(&self, model_name: &str) -> bool {
        let needle = model_name.to_lowercase();
        self.models.iter().any(|m| {
            let name = m.model_name.to_lowercase();
            name == needle || name.contains(&needle) || needle.contains(&name)
        })
    }

    /// Find an offering for a specific model.
    pub fn find_model(&self, model_name: &str) -> Option<&ModelOffering> {
        let needle = model_name.to_lowercase();
        self.models.iter().find(|m| {
            let name = m.model_name.to_lowercase();
            name == needle || name.contains(&needle) || needle.contains(&name)
        })
    }
}

/// A single model offering from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOffering {
    /// Model name (e.g., "llama3.2:3b", "mistral:7b")
    pub model_name: String,
    /// Context window size
    pub context_size: u32,
    /// Price per 1k tokens in μPCLAW
    pub price_per_1k_tokens: u64,
    /// Maximum tokens per request
    pub max_tokens_per_request: u32,
    /// Quantization level (e.g., "Q4_K_M", "Q5_K_M")
    pub quantization: Option<String>,
    /// Backend type
    pub backend: ProviderBackend,
}

/// Backend type for the LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderBackend {
    /// Local Ollama instance
    Ollama,
    /// Local GGUF model via llama.cpp
    Gguf,
    /// OpenAI-compatible API endpoint
    OpenAiCompatible,
}

impl std::fmt::Display for ProviderBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ollama => write!(f, "ollama"),
            Self::Gguf => write!(f, "gguf"),
            Self::OpenAiCompatible => write!(f, "openai-compatible"),
        }
    }
}

/// Rate limits for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRateLimits {
    /// Maximum requests per hour from all peers combined
    pub max_requests_per_hour: u32,
    /// Maximum tokens per day from all peers combined
    pub max_tokens_per_day: u64,
    /// Maximum concurrent inference requests
    pub max_concurrent_requests: u32,
}

impl Default for ProviderRateLimits {
    fn default() -> Self {
        Self {
            max_requests_per_hour: 60,
            max_tokens_per_day: 100_000,
            max_concurrent_requests: 2,
        }
    }
}

/// Re-export config type for convenience.
pub use crate::config::ProviderSharingConfig;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_manifest_creation() {
        let models = vec![ModelOffering {
            model_name: "llama3.2:3b".to_string(),
            context_size: 4096,
            price_per_1k_tokens: 100,
            max_tokens_per_request: 2048,
            quantization: Some("Q4_K_M".to_string()),
            backend: ProviderBackend::Ollama,
        }];

        let manifest = ProviderManifest::new(
            "12D3KooWTest".to_string(),
            models,
            ProviderRateLimits::default(),
        );

        assert!(manifest.has_model("llama3.2:3b"));
        assert!(manifest.has_model("llama3.2"));
        assert!(!manifest.has_model("mistral"));
        assert!(!manifest.is_expired(300));
    }

    #[test]
    fn test_provider_manifest_serialization() {
        let manifest = ProviderManifest::new(
            "test-peer".to_string(),
            vec![],
            ProviderRateLimits::default(),
        );

        let bytes = rmp_serde::to_vec(&manifest).unwrap();
        let decoded: ProviderManifest = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded.peer_id, manifest.peer_id);
    }
}
