//! Runtime-mutable inference options (Ollama, local GGUF routing, remote OpenAI-compatible API).

use serde::{Deserialize, Serialize};

use crate::config::InferenceConfig;

/// Live settings read on every `InferenceEngine::generate` (updated from web UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceLiveSettings {
    /// Prefer registered local `.gguf` in the inference engine registry when present.
    #[serde(default = "default_true")]
    pub use_local_gguf: bool,
    /// Allow Ollama fallback when local model is missing or local GGUF is off.
    #[serde(default)]
    pub use_ollama: bool,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    /// Use an OpenAI-compatible HTTP API (Chat Completions).
    #[serde(default)]
    pub remote_api_enabled: bool,
    #[serde(default)]
    pub remote_api_base_url: String,
    /// If empty, the requested model id from chat is sent to the remote API.
    #[serde(default)]
    pub remote_api_model: String,
    #[serde(default)]
    pub remote_api_key: String,
}

fn default_true() -> bool {
    true
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

impl Default for InferenceLiveSettings {
    fn default() -> Self {
        Self {
            use_local_gguf: true,
            use_ollama: std::env::var("USE_OLLAMA").is_ok_and(|v| v == "1" || v == "true")
                || std::env::var("OLLAMA_BASE_URL").is_ok(),
            ollama_url: std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            remote_api_enabled: false,
            remote_api_base_url: String::new(),
            remote_api_model: String::new(),
            remote_api_key: String::new(),
        }
    }
}

impl InferenceLiveSettings {
    pub fn from_config(c: &InferenceConfig) -> Self {
        Self {
            use_local_gguf: c.use_local_gguf,
            use_ollama: c.use_ollama,
            ollama_url: c.ollama_url.clone(),
            remote_api_enabled: c.remote_api_enabled,
            remote_api_base_url: c.remote_api_base_url.clone(),
            remote_api_model: c.remote_api_model.clone(),
            remote_api_key: c.remote_api_key.clone(),
        }
    }

    pub fn apply_to_config(&self, c: &mut InferenceConfig) {
        c.use_local_gguf = self.use_local_gguf;
        c.use_ollama = self.use_ollama;
        c.ollama_url = self.ollama_url.clone();
        c.remote_api_enabled = self.remote_api_enabled;
        c.remote_api_base_url = self.remote_api_base_url.clone();
        c.remote_api_model = self.remote_api_model.clone();
        c.remote_api_key = self.remote_api_key.clone();
    }
}
