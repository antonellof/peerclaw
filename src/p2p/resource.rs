//! Resource manifest for advertising peer capabilities.

use serde::{Deserialize, Serialize};

/// Resource manifest advertised to the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceManifest {
    /// The peer's ID as a string
    pub peer_id: String,

    /// Unix timestamp when the manifest was created
    pub timestamp: u64,

    /// Ed25519 signature of the manifest
    pub signature: Vec<u8>,

    /// Available resources
    pub resources: Resources,

    /// Capabilities this peer supports
    pub capabilities: Vec<Capability>,

    /// Models this peer has loaded
    pub supported_models: Vec<String>,

    /// Total uptime in hours
    pub uptime_hours: u64,
}

impl ResourceManifest {
    /// Create a new resource manifest.
    pub fn new(peer_id: String, resources: Resources, capabilities: Vec<Capability>) -> Self {
        Self {
            peer_id,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: vec![],
            resources,
            capabilities,
            supported_models: vec![],
            uptime_hours: 0,
        }
    }

    /// Get the bytes to sign.
    pub fn signing_bytes(&self) -> Vec<u8> {
        // Create a deterministic representation for signing
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.peer_id.as_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        bytes.extend_from_slice(&rmp_serde::to_vec(&self.resources).unwrap_or_default());
        bytes.extend_from_slice(&rmp_serde::to_vec(&self.capabilities).unwrap_or_default());
        bytes
    }

    /// Sign the manifest with the given signing function.
    pub fn sign<F>(&mut self, signer: F)
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let bytes = self.signing_bytes();
        self.signature = signer(&bytes);
    }

    /// Verify the manifest signature with the given verifier.
    pub fn verify<F>(&self, verifier: F) -> bool
    where
        F: FnOnce(&[u8], &[u8]) -> bool,
    {
        let bytes = self.signing_bytes();
        verifier(&bytes, &self.signature)
    }
}

/// Available resources on a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    /// Number of CPU cores
    pub cpu_cores: u16,

    /// Available CPU frequency in MHz
    pub cpu_available_mhz: u32,

    /// GPU information if available
    pub gpu: Option<GpuInfo>,

    /// Available storage in bytes
    pub storage_available_bytes: u64,

    /// Available bandwidth in Mbps
    pub bandwidth_mbps: u32,

    /// Available RAM in MB
    pub ram_available_mb: u32,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            cpu_cores: num_cpus::get() as u16,
            cpu_available_mhz: 0, // Would need sysinfo to detect
            gpu: None,
            storage_available_bytes: 0,
            bandwidth_mbps: 0,
            ram_available_mb: 0,
        }
    }
}

/// GPU information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    /// GPU vendor
    pub vendor: GpuVendor,

    /// VRAM in MB
    pub vram_mb: u32,

    /// Compute capability (e.g., "8.9" for RTX 4090)
    pub compute_capability: String,

    /// Model name
    pub model_name: String,
}

/// GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    Other,
}

/// Capability flags for a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Can execute WASM tools in sandbox
    WasmSandbox,

    /// Can run AI inference
    Inference,

    /// Can provide distributed storage
    Storage,

    /// Can act as a relay for other peers
    Relay,

    /// Can provide web access (HTTP 402 proxy)
    WebProxy,

    /// Can share LLM inference capacity with network peers
    LlmProvider,
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Capability::WasmSandbox => write!(f, "wasm"),
            Capability::Inference => write!(f, "inference"),
            Capability::Storage => write!(f, "storage"),
            Capability::Relay => write!(f, "relay"),
            Capability::WebProxy => write!(f, "web-proxy"),
            Capability::LlmProvider => write!(f, "llm-provider"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_manifest_creation() {
        let resources = Resources::default();
        let capabilities = vec![Capability::WasmSandbox, Capability::Inference];

        let manifest = ResourceManifest::new(
            "12D3KooWTest...".to_string(),
            resources,
            capabilities.clone(),
        );

        assert!(!manifest.peer_id.is_empty());
        assert!(manifest.timestamp > 0);
        assert_eq!(manifest.capabilities.len(), 2);
    }

    #[test]
    fn test_manifest_signing() {
        let resources = Resources::default();
        let capabilities = vec![Capability::Inference];

        let mut manifest = ResourceManifest::new(
            "test-peer".to_string(),
            resources,
            capabilities,
        );

        // Simple mock signer
        manifest.sign(|data| {
            let hash = blake3::hash(data);
            hash.as_bytes().to_vec()
        });

        assert!(!manifest.signature.is_empty());

        // Verify with matching verifier
        let valid = manifest.verify(|data, sig| {
            let hash = blake3::hash(data);
            hash.as_bytes() == sig
        });

        assert!(valid);
    }
}
