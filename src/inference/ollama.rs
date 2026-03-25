//! Ollama provider - proxies inference to a local Ollama instance.
//!
//! Ollama exposes an OpenAI-compatible API at http://localhost:11434.
//! This module calls it for models not available as local GGUF files.

use serde::{Deserialize, Serialize};

/// Ollama API configuration.
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    /// Base URL (default: http://localhost:11434)
    pub base_url: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            timeout_secs: 120,
        }
    }
}

/// Ollama chat request.
#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

/// Ollama chat response.
#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaResponseMessage,
    #[serde(default)]
    eval_count: u32,
    #[serde(default)]
    eval_duration: u64, // nanoseconds
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    total_duration: u64, // nanoseconds
}

#[derive(Deserialize)]
struct OllamaResponseMessage {
    content: String,
}

/// Ollama model list response.
#[derive(Deserialize)]
pub struct OllamaModelsResponse {
    pub models: Vec<OllamaModelInfo>,
}

#[derive(Deserialize, Clone)]
pub struct OllamaModelInfo {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub details: Option<OllamaModelDetails>,
}

#[derive(Deserialize, Clone)]
pub struct OllamaModelDetails {
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
    pub family: Option<String>,
}

/// Ollama provider client.
pub struct OllamaProvider {
    config: OllamaConfig,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Create a new Ollama provider.
    pub fn new(config: OllamaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_default();
        Self { config, client }
    }

    /// Check if Ollama is reachable.
    pub async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.config.base_url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }

    /// List available models from Ollama.
    pub async fn list_models(&self) -> Result<Vec<OllamaModelInfo>, String> {
        let resp = self
            .client
            .get(format!("{}/api/tags", self.config.base_url))
            .send()
            .await
            .map_err(|e| format!("Ollama not reachable: {e}"))?;

        let body: OllamaModelsResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {e}"))?;

        Ok(body.models)
    }

    /// Check if a model is available in Ollama (fuzzy match).
    pub async fn has_model(&self, model_name: &str) -> bool {
        if let Ok(models) = self.list_models().await {
            let normalized = normalize_model_name(model_name);
            models.iter().any(|m| {
                let n = normalize_model_name(&m.name);
                n == normalized || n.starts_with(&normalized) || normalized.starts_with(&n)
            })
        } else {
            false
        }
    }

    /// Resolve model name to the exact Ollama model name.
    pub async fn resolve_model(&self, model_name: &str) -> Option<String> {
        let models = self.list_models().await.ok()?;
        let normalized = normalize_model_name(model_name);

        // Exact match first
        if let Some(m) = models
            .iter()
            .find(|m| normalize_model_name(&m.name) == normalized)
        {
            return Some(m.name.clone());
        }

        // Prefix match (e.g. "llama3.2" matches "llama3.2:latest")
        if let Some(m) = models
            .iter()
            .find(|m| normalize_model_name(&m.name).starts_with(&normalized))
        {
            return Some(m.name.clone());
        }

        // Reverse prefix (e.g. "llama-3.2-3b" matches "llama3.2")
        if let Some(m) = models
            .iter()
            .find(|m| normalized.starts_with(&normalize_model_name(&m.name)))
        {
            return Some(m.name.clone());
        }

        None
    }

    /// Generate a response using Ollama's chat API.
    pub async fn generate(
        &self,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
        system_prompt: Option<&str>,
    ) -> Result<OllamaGenerateResult, String> {
        // Resolve model name
        let ollama_model = self
            .resolve_model(model)
            .await
            .unwrap_or_else(|| model.to_string());

        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(OllamaMessage {
                role: "system".to_string(),
                content: sys.to_string(),
            });
        }

        messages.push(OllamaMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        let request = OllamaChatRequest {
            model: ollama_model.clone(),
            messages,
            stream: false,
            options: OllamaOptions {
                temperature,
                num_predict: max_tokens,
                top_p: None,
                stop: None,
            },
        };

        let start = std::time::Instant::now();

        let resp = self
            .client
            .post(format!("{}/api/chat", self.config.base_url))
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama returned {status}: {body}"));
        }

        let chat_resp: OllamaChatResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {e}"))?;

        let elapsed = start.elapsed();
        let tokens = chat_resp.eval_count;
        let tps = if chat_resp.eval_duration > 0 {
            tokens as f64 / (chat_resp.eval_duration as f64 / 1_000_000_000.0)
        } else if elapsed.as_secs_f64() > 0.0 {
            tokens as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        Ok(OllamaGenerateResult {
            text: chat_resp.message.content,
            tokens_generated: tokens,
            tokens_per_second: tps,
            total_time_ms: elapsed.as_millis() as u64,
            model_used: ollama_model,
        })
    }
}

/// Result from Ollama generation.
pub struct OllamaGenerateResult {
    pub text: String,
    pub tokens_generated: u32,
    pub tokens_per_second: f64,
    pub total_time_ms: u64,
    pub model_used: String,
}

/// Normalize model names for fuzzy matching.
/// "llama-3.2-3b" -> "llama3.23b", "llama3.2:latest" -> "llama3.2"
fn normalize_model_name(name: &str) -> String {
    let name = name.split(':').next().unwrap_or(name);
    name.to_lowercase().replace(['-', '_', ' '], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_model_name() {
        assert_eq!(normalize_model_name("llama-3.2-3b"), "llama3.23b");
        assert_eq!(normalize_model_name("llama3.2:latest"), "llama3.2");
        assert_eq!(normalize_model_name("Phi-3-Mini"), "phi3mini");
        assert_eq!(normalize_model_name("deepseek-r1:7b"), "deepseekr1");
    }
}
