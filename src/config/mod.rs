//! Configuration management with layered resolution.
//!
//! Priority: defaults -> config file -> env vars -> CLI flags

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::bootstrap;

/// Root configuration for PeerClaw.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// P2P networking configuration
    pub p2p: P2pConfig,

    /// Web dashboard configuration
    pub web: WebConfig,

    /// Resource advertisement configuration
    pub resources: ResourcesConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Agent configuration
    pub agent: AgentConfig,

    /// Inference engine configuration
    pub inference: InferenceConfig,

    /// Task executor configuration
    pub executor: ExecutorConfig,

    /// WASM sandbox configuration
    pub wasm: WasmConfig,

    /// Token economy configuration
    pub economy: EconomyConfig,

    /// LLM provider sharing configuration
    #[serde(default)]
    pub provider_sharing: ProviderSharingConfig,

    /// Local skills directory (`SKILL.md` files). Default: `~/.peerclaw/skills`.
    #[serde(default)]
    pub skills: SkillsConfig,

    /// MCP client settings (used by the web UI and for future agent wiring; servers run as sidecars).
    #[serde(default)]
    pub mcp: crate::mcp::McpConfig,

    /// Multi-agent orchestration (crew / flow workers).
    #[serde(default)]
    pub orchestration: OrchestrationConfig,

    /// Editable LLM prompt fragments (`prompts/*.txt` overlays).
    #[serde(default)]
    pub prompts: PromptsConfig,
}

/// Optional directory of `*.txt` files that override built-in prompt fragments at startup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptsConfig {
    /// Same-named `.txt` as embedded stems (e.g. `agentic_system_intro.txt`).
    pub directory: Option<PathBuf>,
}

/// Crew task market worker and related flags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    /// When true, this node may claim `peerclaw/crew/v1` offers and return signed results.
    #[serde(default)]
    pub crew_worker: bool,
}

/// Skills directory and discovery options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillsConfig {
    /// If set, load skills from this directory instead of `~/.peerclaw/skills`.
    #[serde(default)]
    pub directory: Option<PathBuf>,
}

impl Config {
    /// Load configuration with layered resolution.
    pub fn load() -> anyhow::Result<Self> {
        let mut config = Self::default();

        // Try to load from config file
        let config_path = bootstrap::base_dir().join("config.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            config = toml::from_str(&content)?;
        }

        // Apply environment variable overrides
        config.apply_env_overrides();

        Ok(config)
    }

    /// Save configuration to file.
    pub fn save(&self) -> anyhow::Result<()> {
        let config_path = bootstrap::base_dir().join("config.toml");
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    /// Apply environment variable overrides.
    fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("PEERCLAWD_WEB_ENABLED") {
            self.web.enabled = val.parse().unwrap_or(self.web.enabled);
        }

        if let Ok(val) = std::env::var("PEERCLAWD_WEB_ADDR") {
            if let Ok(addr) = val.parse() {
                self.web.listen_addr = addr;
            }
        }

        if let Ok(val) = std::env::var("PEERCLAWD_BOOTSTRAP") {
            self.p2p.bootstrap_peers = val.split(',').map(String::from).collect();
        }

        if let Ok(val) = std::env::var("PEERCLAW_PROMPTS_DIR") {
            let p = PathBuf::from(val.trim());
            if !p.as_os_str().is_empty() {
                self.prompts.directory = Some(p);
            }
        }
    }
}

/// P2P networking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2pConfig {
    /// Listen addresses for P2P connections
    pub listen_addresses: Vec<String>,

    /// Bootstrap peers to connect to
    pub bootstrap_peers: Vec<String>,

    /// Enable mDNS for local discovery
    pub mdns_enabled: bool,

    /// Enable Kademlia DHT
    pub kademlia_enabled: bool,

    /// Resource advertisement interval in seconds
    pub advertise_interval_secs: u64,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            listen_addresses: vec!["/ip4/0.0.0.0/tcp/0".to_string()],
            bootstrap_peers: vec![],
            mdns_enabled: true,
            kademlia_enabled: true,
            advertise_interval_secs: 300, // 5 minutes
        }
    }
}

/// Web dashboard configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Enable web dashboard
    pub enabled: bool,

    /// Listen address for web server
    pub listen_addr: SocketAddr,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_addr: "127.0.0.1:8080".parse().unwrap(),
        }
    }
}

/// Resource advertisement configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourcesConfig {
    /// Advertise GPU resources
    pub advertise_gpu: bool,

    /// CPU cores to advertise (None = auto-detect)
    pub cpu_cores: Option<u16>,

    /// Storage to advertise in bytes (None = auto-detect)
    pub storage_bytes: Option<u64>,

    /// RAM to advertise in MB (None = auto-detect)
    pub ram_mb: Option<u32>,
}

/// Database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to the database file
    pub path: PathBuf,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: bootstrap::database_path(),
        }
    }
}

/// Agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum concurrent agents
    pub max_agents: usize,

    /// Default model for agents
    pub default_model: String,

    /// WASM tool timeout in seconds
    pub tool_timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            default_model: "llama-3.2-3b".to_string(),
            tool_timeout_secs: 60,
        }
    }
}

/// Inference engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Directory for model storage
    pub models_dir: PathBuf,
    /// Maximum models to keep loaded
    pub max_loaded_models: usize,
    /// Maximum memory for models in MB
    pub max_memory_mb: u32,
    /// Number of GPU layers to offload (-1 = auto, 0 = CPU only)
    pub gpu_layers: i32,
    /// Context size for inference
    pub context_size: u32,
    /// Batch size for inference
    pub batch_size: u32,
    /// Enable P2P model download
    pub enable_p2p_download: bool,
    /// Use Ollama as inference provider (set USE_OLLAMA=1 or OLLAMA_BASE_URL)
    #[serde(default)]
    pub use_ollama: bool,
    /// Ollama API base URL
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    /// Prefer local `.gguf` in the inference registry when routing (disable to force cloud/Ollama).
    #[serde(default = "default_true")]
    pub use_local_gguf: bool,
    /// Use an OpenAI-compatible Chat Completions API when enabled (checked before Ollama).
    #[serde(default)]
    pub remote_api_enabled: bool,
    /// Base URL, e.g. `https://api.openai.com/v1` or `https://api.groq.com/openai/v1`
    #[serde(default)]
    pub remote_api_base_url: String,
    /// Optional fixed model id for the remote API; if empty, the chat request model name is used.
    #[serde(default)]
    pub remote_api_model: String,
    /// Bearer token for the remote API (store `config.toml` with restrictive permissions).
    #[serde(default)]
    pub remote_api_key: String,
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            models_dir: bootstrap::base_dir().join("models"),
            max_loaded_models: 3,
            max_memory_mb: 16_000, // 16 GB
            gpu_layers: -1,        // Auto
            context_size: 4096,
            batch_size: 512,
            enable_p2p_download: true,
            use_ollama: std::env::var("USE_OLLAMA").is_ok_and(|v| v == "1" || v == "true")
                || std::env::var("OLLAMA_BASE_URL").is_ok(),
            ollama_url: std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            use_local_gguf: true,
            remote_api_enabled: false,
            remote_api_base_url: String::new(),
            remote_api_model: String::new(),
            remote_api_key: String::new(),
        }
    }
}

/// Task executor configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// CPU utilization threshold for local execution (0.0 - 1.0)
    pub local_utilization_threshold: f64,
    /// Utilization threshold above which to offload
    pub offload_threshold: f64,
    /// Allow offloading tasks to network peers
    pub allow_network_offload: bool,
    /// Maximum concurrent inference tasks
    pub max_concurrent_inference: u32,
    /// Maximum concurrent WASM tasks
    pub max_concurrent_wasm: u32,
    /// Maximum web response size in bytes
    pub max_web_response_size: usize,
    /// Default web timeout in seconds
    pub default_web_timeout_secs: u32,
    /// Batch aggregation: time window in ms to collect requests
    pub batch_window_ms: Option<u64>,
    /// Batch aggregation: maximum requests per batch
    pub max_batch_size: Option<usize>,
    /// Batch aggregation: minimum requests to trigger early processing
    pub min_batch_size: Option<usize>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            local_utilization_threshold: 0.8,
            offload_threshold: 0.9,
            allow_network_offload: true,
            max_concurrent_inference: 2,
            max_concurrent_wasm: 10,
            max_web_response_size: 10 * 1024 * 1024, // 10 MB
            default_web_timeout_secs: 30,
            batch_window_ms: Some(50),
            max_batch_size: Some(8),
            min_batch_size: Some(4),
        }
    }
}

/// WASM sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmConfig {
    /// Directory for WASM tools
    pub tools_dir: PathBuf,
    /// Maximum memory per execution in MB
    pub max_memory_mb: u32,
    /// Default fuel limit
    pub default_fuel_limit: u64,
    /// Default timeout in seconds
    pub timeout_secs: u32,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            tools_dir: bootstrap::base_dir().join("tools"),
            max_memory_mb: 256,
            default_fuel_limit: 100_000_000,
            timeout_secs: 60,
        }
    }
}

/// Token economy configuration.
///
/// Simple accounting for P2P job execution. Tokens track resource usage
/// across the mesh; on-chain settlement is deferred to v1.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyConfig {
    /// Enable token accounting. When false, jobs execute without payment.
    pub enabled: bool,

    /// Default price per 1K inference tokens (μPCLAW). 500_000 = 0.5 PCLAW.
    pub inference_price_per_1k: u64,

    /// Default price per tool invocation (μPCLAW). 20_000 = 0.02 PCLAW.
    pub tool_price_per_call: u64,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            inference_price_per_1k: 500_000, // 0.5 PCLAW per 1K tokens
            tool_price_per_call: 20_000,     // 0.02 PCLAW per tool call
        }
    }
}

/// LLM provider sharing configuration.
///
/// Controls whether this node shares its inference capacity with network peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSharingConfig {
    /// Enable sharing local inference with network peers
    pub enabled: bool,
    /// Advertise available models to network
    pub advertise_models: bool,
    /// Maximum requests per hour from all peers combined
    pub max_requests_per_hour: u32,
    /// Maximum tokens per day from all peers combined
    pub max_tokens_per_day: u64,
    /// Maximum concurrent inference requests from peers
    pub max_concurrent_requests: u32,
    /// Price multiplier on base economy prices (1.0 = base price)
    pub price_multiplier: f64,
}

impl Default for ProviderSharingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            advertise_models: true,
            max_requests_per_hour: 60,
            max_tokens_per_day: 100_000,
            max_concurrent_requests: 2,
            price_multiplier: 1.0,
        }
    }
}

impl EconomyConfig {
    /// Config for private networks (no token accounting).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            inference_price_per_1k: 0,
            tool_price_per_call: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.web.enabled);
        assert!(config.p2p.mdns_enabled);
        assert!(config.p2p.kademlia_enabled);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.p2p.mdns_enabled, parsed.p2p.mdns_enabled);
    }
}
