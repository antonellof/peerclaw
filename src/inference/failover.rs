//! Model failover chain for inference backends.
//!
//! Tries backends in order (local GGUF -> P2P -> Ollama -> remote API),
//! with circuit breaker logic per backend to avoid hammering broken endpoints.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::sync::RwLock as AsyncRwLock;

use super::gguf::GgufEngine;
use super::live_settings::InferenceLiveSettings;
use super::ollama::{OllamaConfig, OllamaProvider};
use super::remote_openai;
use super::{FinishReason, GenerateRequest, GenerateResponse, InferenceError, ModelRegistry};

/// Identifies a backend in the failover chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendId {
    LocalGguf,
    PeerToPeer,
    Ollama,
    RemoteApi,
}

impl std::fmt::Display for BackendId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendId::LocalGguf => write!(f, "local-gguf"),
            BackendId::PeerToPeer => write!(f, "p2p"),
            BackendId::Ollama => write!(f, "ollama"),
            BackendId::RemoteApi => write!(f, "remote-api"),
        }
    }
}

/// Whether an error is retriable or fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Transient errors: timeout, overloaded, rate-limited. Worth retrying on next backend.
    Retriable,
    /// Fatal errors: auth failure, model not found. Don't trip circuit breaker.
    Fatal,
}

/// Classify an `InferenceError` as retriable or fatal.
pub fn classify_error(err: &InferenceError) -> ErrorKind {
    match err {
        InferenceError::ModelNotFound(_) => ErrorKind::Fatal,
        InferenceError::LoadFailed(msg) => {
            if msg.contains("auth") || msg.contains("unauthorized") || msg.contains("403") {
                ErrorKind::Fatal
            } else {
                ErrorKind::Retriable
            }
        }
        InferenceError::GenerationFailed(msg) => {
            let lower = msg.to_lowercase();
            if lower.contains("timeout")
                || lower.contains("timed out")
                || lower.contains("rate limit")
                || lower.contains("429")
                || lower.contains("overloaded")
                || lower.contains("503")
                || lower.contains("502")
                || lower.contains("connection refused")
                || lower.contains("connection reset")
            {
                ErrorKind::Retriable
            } else if lower.contains("auth")
                || lower.contains("unauthorized")
                || lower.contains("401")
                || lower.contains("403")
                || lower.contains("not found")
                || lower.contains("404")
            {
                ErrorKind::Fatal
            } else {
                // Default: treat unknown generation errors as retriable
                ErrorKind::Retriable
            }
        }
        InferenceError::CacheError(_) => ErrorKind::Retriable,
        InferenceError::IoError(_) => ErrorKind::Retriable,
    }
}

/// Cooldown escalation tiers: 1 min, 5 min, 30 min (cap).
const COOLDOWN_TIERS: [Duration; 3] = [
    Duration::from_secs(60),
    Duration::from_secs(300),
    Duration::from_secs(1800),
];

/// Per-backend circuit breaker state.
#[derive(Debug)]
struct CircuitBreaker {
    /// Consecutive retriable error count.
    error_count: u32,
    /// When the backend is allowed to be tried again (if in cooldown).
    cooldown_until: Option<Instant>,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            error_count: 0,
            cooldown_until: None,
        }
    }

    /// Returns true if the backend is available (not in cooldown).
    fn is_available(&self) -> bool {
        match self.cooldown_until {
            None => true,
            Some(until) => Instant::now() >= until,
        }
    }

    /// Record a successful call — resets the breaker.
    fn record_success(&mut self) {
        self.error_count = 0;
        self.cooldown_until = None;
    }

    /// Record a retriable failure — escalate cooldown.
    fn record_failure(&mut self) {
        self.error_count += 1;
        let tier = (self.error_count as usize)
            .saturating_sub(1)
            .min(COOLDOWN_TIERS.len() - 1);
        let cooldown = COOLDOWN_TIERS[tier];
        self.cooldown_until = Some(Instant::now() + cooldown);
        tracing::warn!(
            error_count = self.error_count,
            cooldown_secs = cooldown.as_secs(),
            "Circuit breaker tripped"
        );
    }
}

/// A backend entry in the failover chain.
struct BackendEntry {
    id: BackendId,
    breaker: CircuitBreaker,
}

/// Failover chain that tries backends in order.
pub struct FailoverChain {
    backends: RwLock<Vec<BackendEntry>>,
    /// Shared inference settings (remote API config, Ollama config, etc.)
    live: Arc<AsyncRwLock<InferenceLiveSettings>>,
    /// GGUF engine for local inference
    gguf: Arc<GgufEngine>,
    /// Model registry for checking local availability
    registry: Arc<AsyncRwLock<ModelRegistry>>,
}

impl FailoverChain {
    /// Build a new failover chain.
    ///
    /// `p2p_enabled` controls whether P2P is inserted between local and Ollama.
    pub fn new(
        live: Arc<AsyncRwLock<InferenceLiveSettings>>,
        gguf: Arc<GgufEngine>,
        registry: Arc<AsyncRwLock<ModelRegistry>>,
        p2p_enabled: bool,
    ) -> Self {
        let mut backends = vec![BackendEntry {
            id: BackendId::LocalGguf,
            breaker: CircuitBreaker::new(),
        }];

        if p2p_enabled {
            backends.push(BackendEntry {
                id: BackendId::PeerToPeer,
                breaker: CircuitBreaker::new(),
            });
        }

        backends.push(BackendEntry {
            id: BackendId::Ollama,
            breaker: CircuitBreaker::new(),
        });

        backends.push(BackendEntry {
            id: BackendId::RemoteApi,
            breaker: CircuitBreaker::new(),
        });

        Self {
            backends: RwLock::new(backends),
            live,
            gguf,
            registry,
        }
    }

    /// Execute a generate request with failover across all backends.
    pub async fn execute_with_failover(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, InferenceError> {
        // Snapshot which backends are available (not in cooldown)
        let backend_ids: Vec<BackendId> = {
            let backends = self.backends.read();
            backends
                .iter()
                .filter(|b| b.breaker.is_available())
                .map(|b| b.id)
                .collect()
        };

        if backend_ids.is_empty() {
            return Err(InferenceError::GenerationFailed(
                "All backends are in cooldown. Try again later.".to_string(),
            ));
        }

        let mut last_error: Option<InferenceError> = None;

        for backend_id in &backend_ids {
            tracing::debug!(backend = %backend_id, model = %request.model, "Trying backend");

            let result = match backend_id {
                BackendId::LocalGguf => self.try_local_gguf(request).await,
                BackendId::PeerToPeer => self.try_p2p(request).await,
                BackendId::Ollama => self.try_ollama(request).await,
                BackendId::RemoteApi => self.try_remote_api(request).await,
            };

            match result {
                Ok(response) => {
                    tracing::info!(backend = %backend_id, model = %request.model, "Backend succeeded");
                    let mut backends = self.backends.write();
                    if let Some(entry) = backends.iter_mut().find(|b| b.id == *backend_id) {
                        entry.breaker.record_success();
                    }
                    return Ok(response);
                }
                Err(err) => {
                    let kind = classify_error(&err);
                    tracing::warn!(
                        backend = %backend_id,
                        error = %err,
                        kind = ?kind,
                        "Backend failed"
                    );

                    match kind {
                        ErrorKind::Retriable => {
                            let mut backends = self.backends.write();
                            if let Some(entry) = backends.iter_mut().find(|b| b.id == *backend_id) {
                                entry.breaker.record_failure();
                            }
                        }
                        ErrorKind::Fatal => {
                            // Don't trip the circuit breaker for fatal errors
                            // (e.g. model not found locally is expected, try next)
                        }
                    }

                    last_error = Some(err);
                    // Continue to next backend
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            InferenceError::GenerationFailed("All backends exhausted".to_string())
        }))
    }

    /// Try local GGUF backend.
    async fn try_local_gguf(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, InferenceError> {
        let live = self.live.read().await;
        if !live.use_local_gguf {
            return Err(InferenceError::ModelNotFound(
                "Local GGUF disabled".to_string(),
            ));
        }

        let model_path = {
            let reg = self.registry.read().await;
            reg.get(&request.model).map(|info| info.path.clone())
        };

        let path = model_path.ok_or_else(|| {
            InferenceError::ModelNotFound(format!(
                "Model '{}' not found in local registry",
                request.model
            ))
        })?;

        tracing::info!(
            model = %request.model,
            path = %path.display(),
            "Running local GGUF inference (failover chain)"
        );

        self.gguf
            .load(&path)
            .map_err(|e| InferenceError::LoadFailed(format!("GGUF load failed: {e}")))?;

        self.gguf
            .generate(request)
            .map_err(|e| InferenceError::GenerationFailed(format!("GGUF generate failed: {e}")))
    }

    /// Try P2P inference (stub — actual routing goes through the job marketplace).
    async fn try_p2p(
        &self,
        _request: &GenerateRequest,
    ) -> Result<GenerateResponse, InferenceError> {
        // P2P inference is routed through the job marketplace / remote executor.
        // This is a placeholder for the failover chain; actual P2P calls go through
        // TaskExecutor → RemoteExecutor → P2P job protocol.
        // For now, signal "not available" so we fall through to Ollama/remote.
        Err(InferenceError::ModelNotFound(
            "P2P inference routing not yet wired into failover chain".to_string(),
        ))
    }

    /// Try Ollama backend.
    async fn try_ollama(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, InferenceError> {
        let live = self.live.read().await;
        if !live.use_ollama {
            return Err(InferenceError::ModelNotFound("Ollama disabled".to_string()));
        }

        tracing::info!(
            model = %request.model,
            "Routing to Ollama (failover chain)"
        );

        let prov = OllamaProvider::new(OllamaConfig {
            base_url: live.ollama_url.clone(),
            timeout_secs: 120,
        });

        let start = Instant::now();
        let result = prov
            .generate(
                &request.model,
                &request.prompt,
                request.max_tokens,
                request.temperature,
                None,
            )
            .await
            .map_err(InferenceError::GenerationFailed)?;

        let elapsed = start.elapsed();

        Ok(GenerateResponse {
            text: result.text,
            tokens_generated: result.tokens_generated,
            tokens_per_second: result.tokens_per_second,
            time_to_first_token_ms: 0,
            total_time_ms: elapsed.as_millis() as u64,
            finish_reason: FinishReason::Stop,
            model_id: result.model_used,
        })
    }

    /// Try remote OpenAI-compatible API backend.
    async fn try_remote_api(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, InferenceError> {
        let live = self.live.read().await;
        if !live.remote_api_enabled
            || live.remote_api_base_url.trim().is_empty()
            || live.remote_api_key.trim().is_empty()
        {
            return Err(InferenceError::ModelNotFound(
                "Remote API disabled or not configured".to_string(),
            ));
        }

        let remote_model = if live.remote_api_model.trim().is_empty() {
            request.model.as_str()
        } else {
            live.remote_api_model.trim()
        };

        tracing::info!(
            model = %remote_model,
            "Routing to remote API (failover chain)"
        );

        let start = Instant::now();
        let r = remote_openai::chat_completion(
            live.remote_api_base_url.trim(),
            live.remote_api_key.trim(),
            remote_model,
            &request.prompt,
            request.max_tokens,
            request.temperature,
        )
        .await
        .map_err(InferenceError::GenerationFailed)?;

        let elapsed = start.elapsed();

        Ok(GenerateResponse {
            text: r.text,
            tokens_generated: r.tokens_generated,
            tokens_per_second: if elapsed.as_secs_f64() > 0.0 {
                r.tokens_generated as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            },
            time_to_first_token_ms: 0,
            total_time_ms: elapsed.as_millis() as u64,
            finish_reason: FinishReason::Stop,
            model_id: remote_model.to_string(),
        })
    }

    /// Reset the circuit breaker for a specific backend (for testing or manual recovery).
    #[allow(dead_code)]
    pub fn reset_breaker(&self, backend_id: BackendId) {
        let mut backends = self.backends.write();
        if let Some(entry) = backends.iter_mut().find(|b| b.id == backend_id) {
            entry.breaker.record_success();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classification_retriable() {
        let err = InferenceError::GenerationFailed("connection timeout".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Retriable);

        let err = InferenceError::GenerationFailed("429 Too Many Requests".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Retriable);

        let err = InferenceError::GenerationFailed("503 Service Unavailable".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Retriable);

        let err = InferenceError::GenerationFailed("connection refused".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Retriable);

        let err = InferenceError::IoError(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timed out",
        ));
        assert_eq!(classify_error(&err), ErrorKind::Retriable);
    }

    #[test]
    fn test_error_classification_fatal() {
        let err = InferenceError::ModelNotFound("no-such-model".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Fatal);

        let err = InferenceError::GenerationFailed("401 Unauthorized".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Fatal);

        let err = InferenceError::GenerationFailed("403 Forbidden".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Fatal);

        let err = InferenceError::LoadFailed("unauthorized access".to_string());
        assert_eq!(classify_error(&err), ErrorKind::Fatal);
    }

    #[test]
    fn test_circuit_breaker_cooldown_escalation() {
        let mut breaker = CircuitBreaker::new();

        // Initially available
        assert!(breaker.is_available());

        // First failure: 1 min cooldown
        breaker.record_failure();
        assert_eq!(breaker.error_count, 1);
        assert!(!breaker.is_available()); // in cooldown

        // Simulate cooldown expiry by resetting
        breaker.cooldown_until = Some(Instant::now() - Duration::from_secs(1));
        assert!(breaker.is_available());

        // Second failure: 5 min cooldown
        breaker.record_failure();
        assert_eq!(breaker.error_count, 2);
        assert!(!breaker.is_available());

        // Third failure: 30 min cooldown (cap)
        breaker.cooldown_until = Some(Instant::now() - Duration::from_secs(1));
        breaker.record_failure();
        assert_eq!(breaker.error_count, 3);
        assert!(!breaker.is_available());

        // Fourth failure: still 30 min (cap)
        breaker.cooldown_until = Some(Instant::now() - Duration::from_secs(1));
        breaker.record_failure();
        assert_eq!(breaker.error_count, 4);

        // Success resets everything
        breaker.cooldown_until = Some(Instant::now() - Duration::from_secs(1));
        breaker.record_success();
        assert_eq!(breaker.error_count, 0);
        assert!(breaker.is_available());
    }

    #[tokio::test]
    async fn test_failover_all_backends_disabled() {
        let live = Arc::new(AsyncRwLock::new(InferenceLiveSettings {
            use_local_gguf: false,
            use_ollama: false,
            remote_api_enabled: false,
            ..Default::default()
        }));

        let gguf = Arc::new(GgufEngine::new(Default::default()));
        let registry = Arc::new(AsyncRwLock::new(ModelRegistry::new()));

        let chain = FailoverChain::new(live, gguf, registry, false);
        let request = GenerateRequest::new("test-model", "Hello");
        let result = chain.execute_with_failover(&request).await;

        // Should fail since all backends are disabled (fatal, no circuit break)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_failover_chain_order() {
        // Verify backends are in correct order
        let live = Arc::new(AsyncRwLock::new(InferenceLiveSettings::default()));
        let gguf = Arc::new(GgufEngine::new(Default::default()));
        let registry = Arc::new(AsyncRwLock::new(ModelRegistry::new()));

        // Without P2P
        let chain = FailoverChain::new(live.clone(), gguf.clone(), registry.clone(), false);
        let ids: Vec<BackendId> = chain.backends.read().iter().map(|b| b.id).collect();
        assert_eq!(
            ids,
            vec![
                BackendId::LocalGguf,
                BackendId::Ollama,
                BackendId::RemoteApi
            ]
        );

        // With P2P
        let chain = FailoverChain::new(live, gguf, registry, true);
        let ids: Vec<BackendId> = chain.backends.read().iter().map(|b| b.id).collect();
        assert_eq!(
            ids,
            vec![
                BackendId::LocalGguf,
                BackendId::PeerToPeer,
                BackendId::Ollama,
                BackendId::RemoteApi
            ]
        );
    }
}
