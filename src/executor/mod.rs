//! Unified task execution system with automatic routing.
//!
//! The executor module provides a unified interface for running tasks
//! (inference, web access, WASM execution) either locally or distributed
//! across the P2P network.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │TaskExecutor │
//! └──────┬──────┘
//!        │
//!        ▼
//! ┌─────────────┐     ┌─────────────────┐
//! │ TaskRouter  │◄────│ ResourceMonitor │
//! └──────┬──────┘     └─────────────────┘
//!        │
//!   ┌────┴────┐
//!   │         │
//!   ▼         ▼
//! Local    Network
//! ```

pub mod remote;
pub mod resource;
pub mod router;
pub mod task;

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc::UnboundedSender, RwLock};
use uuid::Uuid;

pub use resource::{MonitorConfig, ResourceMonitor, ResourceState, TaskType};
pub use router::{PeerFilter, RouterConfig, RoutingDecision, TaskRouter};
pub use task::*;

use crate::inference::gguf::{GgufConfig, GgufEngine, GgufModelHandle, TokenCallback};
use crate::inference::GenerateRequest;
use crate::job::JobManager;
use crate::p2p::Network;

use self::remote::RemoteExecutor;

/// Unified task executor with automatic local/network routing.
#[allow(dead_code)]
pub struct TaskExecutor {
    router: TaskRouter,
    resource_monitor: Arc<ResourceMonitor>,
    job_manager: Option<Arc<RwLock<JobManager>>>,
    network: Option<Arc<RwLock<Network>>>,
    config: ExecutorConfig,
    /// GGUF inference engine
    gguf_engine: Arc<RwLock<GgufEngine>>,
    /// Currently loaded model handle
    loaded_model: Arc<RwLock<Option<GgufModelHandle>>>,
    /// High-level inference engine (supports GGUF + Ollama)
    inference_engine: Option<Arc<crate::inference::InferenceEngine>>,
    /// Remote executor for P2P task offloading
    remote_executor: Option<Arc<RemoteExecutor>>,
}

impl TaskExecutor {
    /// Create a new task executor.
    pub fn new(
        resource_monitor: Arc<ResourceMonitor>,
        router_config: RouterConfig,
        config: ExecutorConfig,
    ) -> Self {
        let router = TaskRouter::new(resource_monitor.clone(), router_config);

        // Initialize GGUF inference engine
        let gguf_config = GgufConfig::default();
        let gguf_engine = GgufEngine::new(gguf_config);

        Self {
            router,
            resource_monitor,
            job_manager: None,
            network: None,
            config,
            gguf_engine: Arc::new(RwLock::new(gguf_engine)),
            loaded_model: Arc::new(RwLock::new(None)),
            inference_engine: None,
            remote_executor: None,
        }
    }

    /// Set the job manager for remote execution.
    pub fn with_job_manager(mut self, job_manager: Arc<RwLock<JobManager>>) -> Self {
        self.job_manager = Some(job_manager);
        self
    }

    /// Set the network for P2P operations.
    pub fn with_network(mut self, network: Arc<RwLock<Network>>) -> Self {
        self.network = Some(network);
        self
    }

    /// Set the high-level inference engine (supports GGUF + Ollama).
    pub fn with_inference_engine(mut self, engine: Arc<crate::inference::InferenceEngine>) -> Self {
        self.inference_engine = Some(engine);
        self
    }

    /// Set the remote executor for P2P task offloading.
    pub fn with_remote_executor(mut self, remote: Arc<RemoteExecutor>) -> Self {
        self.remote_executor = Some(remote);
        self
    }

    /// Execute a task, routing automatically based on resources.
    pub async fn execute(&self, task: ExecutionTask) -> Result<TaskResult, ExecutorError> {
        let task_id = Uuid::new_v4().to_string();
        let start = Instant::now();

        // Get routing decision
        let decision = self.router.route(&task).await;

        tracing::debug!(
            task_id = %task_id,
            task_type = %task.task_type(),
            decision = ?decision,
            "Routing task"
        );

        match decision {
            RoutingDecision::ExecuteLocally => self.execute_local(task_id, task, start).await,
            RoutingDecision::OffloadToNetwork { requirements } => {
                self.execute_remote(task_id, task, requirements, start)
                    .await
            }
            RoutingDecision::InsufficientResources { reason } => {
                Err(ExecutorError::InsufficientResources(reason))
            }
        }
    }

    /// Force local execution (skip routing).
    pub async fn execute_local(
        &self,
        task_id: TaskId,
        task: ExecutionTask,
        start: Instant,
    ) -> Result<TaskResult, ExecutorError> {
        let task_type = match &task {
            ExecutionTask::Inference(_) => TaskType::Inference,
            ExecutionTask::WasmExecution(_) => TaskType::Wasm,
            ExecutionTask::WebFetch(_) | ExecutionTask::WebSearch(_) => TaskType::Web,
        };

        // Track active task
        self.resource_monitor.task_started(task_type).await;

        let result = match task {
            ExecutionTask::Inference(inference_task) => {
                self.execute_inference_local(inference_task).await
            }
            ExecutionTask::WebFetch(fetch_task) => self.execute_web_fetch_local(fetch_task).await,
            ExecutionTask::WebSearch(search_task) => {
                self.execute_web_search_local(search_task).await
            }
            ExecutionTask::WasmExecution(wasm_task) => self.execute_wasm_local(wasm_task).await,
        };

        // Task completed
        self.resource_monitor.task_completed(task_type).await;

        let total_time_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(data) => Ok(TaskResult {
                task_id,
                location: ExecutionLocation::Local,
                data,
                metrics: TaskMetrics {
                    ttfb_ms: 0, // TODO: Track actual TTFB
                    total_time_ms,
                    queue_time_ms: 0,
                },
                cost: None,
            }),
            Err(e) => Ok(TaskResult {
                task_id,
                location: ExecutionLocation::Local,
                data: TaskData::Error(e.to_string()),
                metrics: TaskMetrics {
                    ttfb_ms: 0,
                    total_time_ms,
                    queue_time_ms: 0,
                },
                cost: None,
            }),
        }
    }

    /// Execute via network (offload to peer).
    async fn execute_remote(
        &self,
        task_id: TaskId,
        task: ExecutionTask,
        peer_filter: PeerFilter,
        start: Instant,
    ) -> Result<TaskResult, ExecutorError> {
        // Use the RemoteExecutor if available
        if let Some(remote) = &self.remote_executor {
            tracing::info!(
                task_id = %task_id,
                task_type = %task.task_type(),
                "Offloading task to P2P network"
            );

            match remote
                .execute(task_id.clone(), task.clone(), peer_filter)
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id,
                        error = %e,
                        "Remote execution failed, falling back to local"
                    );
                    // Fall back to local execution
                    return self.execute_local(task_id, task, start).await;
                }
            }
        }

        // No remote executor available, fall back to local
        tracing::debug!(
            task_id = %task_id,
            "No remote executor configured, executing locally"
        );
        self.execute_local(task_id, task, start).await
    }

    /// Execute inference locally.
    ///
    /// If a high-level `InferenceEngine` is set (supports GGUF + Ollama routing),
    /// it is used. Otherwise falls back to the direct GGUF engine.
    async fn execute_inference_local(
        &self,
        task: InferenceTask,
    ) -> Result<TaskData, ExecutorError> {
        // Convert chat messages to prompt string
        let prompt = task
            .messages
            .iter()
            .map(|msg| match msg.role {
                MessageRole::System => format!("System: {}\n", msg.content),
                MessageRole::User => format!("User: {}\n", msg.content),
                MessageRole::Assistant => format!("Assistant: {}\n", msg.content),
            })
            .collect::<String>()
            + "Assistant:";

        tracing::info!(
            model = %task.model,
            max_tokens = task.max_tokens,
            prompt_len = prompt.len(),
            "Executing inference locally"
        );

        // --- High-level InferenceEngine path (GGUF + Ollama) ---
        if let Some(engine) = &self.inference_engine {
            let request = GenerateRequest {
                model: task.model.clone(),
                prompt,
                max_tokens: task.max_tokens,
                temperature: task.temperature,
                top_p: 0.9,
                stop_sequences: task.stop_sequences.clone(),
            };

            let response = engine
                .generate(&request)
                .await
                .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

            tracing::info!(
                tokens = response.tokens_generated,
                tps = format!("{:.1}", response.tokens_per_second),
                model_id = %response.model_id,
                "Inference complete"
            );

            let finish_reason = match response.finish_reason {
                crate::inference::FinishReason::Stop => FinishReason::Stop,
                crate::inference::FinishReason::Length => FinishReason::Length,
                crate::inference::FinishReason::ContentFilter => FinishReason::ContentFilter,
            };

            return Ok(TaskData::Inference(InferenceResult {
                text: response.text,
                tokens_generated: response.tokens_generated,
                tokens_per_second: response.tokens_per_second,
                finish_reason,
            }));
        }

        // --- Direct GGUF engine fallback ---
        let actual_path = self.find_gguf_model(&task.model)?;

        tracing::info!(
            model_path = %actual_path.display(),
            "Loading model for inference (direct GGUF)"
        );

        let engine = self.gguf_engine.read().await;

        let model_handle = engine
            .load(&actual_path)
            .map_err(|e| ExecutorError::InferenceError(format!("Failed to load model: {}", e)))?;

        let request = GenerateRequest {
            model: task.model.clone(),
            prompt,
            max_tokens: task.max_tokens,
            temperature: task.temperature,
            top_p: 0.9,
            stop_sequences: task.stop_sequences.clone(),
        };

        let response = engine
            .generate(&model_handle, &request)
            .map_err(|e| ExecutorError::InferenceError(format!("Generation failed: {}", e)))?;

        tracing::info!(
            tokens = response.tokens_generated,
            tps = format!("{:.1}", response.tokens_per_second),
            "Inference complete"
        );

        let finish_reason = match response.finish_reason {
            crate::inference::FinishReason::Stop => FinishReason::Stop,
            crate::inference::FinishReason::Length => FinishReason::Length,
            crate::inference::FinishReason::ContentFilter => FinishReason::ContentFilter,
        };

        Ok(TaskData::Inference(InferenceResult {
            text: response.text,
            tokens_generated: response.tokens_generated,
            tokens_per_second: response.tokens_per_second,
            finish_reason,
        }))
    }

    /// Run inference with optional network offload; push UTF-8 chunks to `delta_tx` as they are produced.
    ///
    /// Local direct GGUF uses true token streaming; the high-level `InferenceEngine` and remote jobs
    /// typically emit a single chunk when the full reply is ready.
    pub async fn execute_inference_streaming(
        &self,
        task: InferenceTask,
        delta_tx: UnboundedSender<String>,
    ) -> Result<TaskResult, ExecutorError> {
        let task_id = Uuid::new_v4().to_string();
        let start = Instant::now();
        let exec_task = ExecutionTask::Inference(task.clone());
        let decision = self.router.route(&exec_task).await;

        match decision {
            RoutingDecision::InsufficientResources { reason } => {
                Err(ExecutorError::InsufficientResources(reason))
            }
            RoutingDecision::ExecuteLocally => {
                self.resource_monitor
                    .task_started(TaskType::Inference)
                    .await;
                let data = match self
                    .execute_inference_local_streaming(task, &delta_tx)
                    .await
                {
                    Ok(d) => d,
                    Err(e) => TaskData::Error(e.to_string()),
                };
                self.resource_monitor
                    .task_completed(TaskType::Inference)
                    .await;
                Ok(TaskResult {
                    task_id,
                    location: ExecutionLocation::Local,
                    data,
                    metrics: TaskMetrics {
                        ttfb_ms: 0,
                        total_time_ms: start.elapsed().as_millis() as u64,
                        queue_time_ms: 0,
                    },
                    cost: None,
                })
            }
            RoutingDecision::OffloadToNetwork { requirements } => {
                self.resource_monitor
                    .task_started(TaskType::Inference)
                    .await;
                let out = if let Some(remote) = &self.remote_executor {
                    match remote
                        .execute(task_id.clone(), exec_task.clone(), requirements)
                        .await
                    {
                        Ok(result) => {
                            match &result.data {
                                TaskData::Inference(inf) => {
                                    let _ = delta_tx.send(inf.text.clone());
                                }
                                TaskData::Error(e) => {
                                    let _ = delta_tx.send(e.clone());
                                }
                                _ => {
                                    let _ = delta_tx.send("Unexpected response type".to_string());
                                }
                            }
                            Ok(result)
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = %task_id,
                                error = %e,
                                "Remote execution failed, falling back to local (streaming)"
                            );
                            let data = match self
                                .execute_inference_local_streaming(task, &delta_tx)
                                .await
                            {
                                Ok(d) => d,
                                Err(e2) => TaskData::Error(e2.to_string()),
                            };
                            Ok(TaskResult {
                                task_id,
                                location: ExecutionLocation::Local,
                                data,
                                metrics: TaskMetrics {
                                    ttfb_ms: 0,
                                    total_time_ms: start.elapsed().as_millis() as u64,
                                    queue_time_ms: 0,
                                },
                                cost: None,
                            })
                        }
                    }
                } else {
                    let data = match self
                        .execute_inference_local_streaming(task, &delta_tx)
                        .await
                    {
                        Ok(d) => d,
                        Err(e) => TaskData::Error(e.to_string()),
                    };
                    Ok(TaskResult {
                        task_id,
                        location: ExecutionLocation::Local,
                        data,
                        metrics: TaskMetrics {
                            ttfb_ms: 0,
                            total_time_ms: start.elapsed().as_millis() as u64,
                            queue_time_ms: 0,
                        },
                        cost: None,
                    })
                };
                self.resource_monitor
                    .task_completed(TaskType::Inference)
                    .await;
                out
            }
        }
    }

    async fn execute_inference_local_streaming(
        &self,
        task: InferenceTask,
        delta_tx: &UnboundedSender<String>,
    ) -> Result<TaskData, ExecutorError> {
        let prompt = task
            .messages
            .iter()
            .map(|msg| match msg.role {
                MessageRole::System => format!("System: {}\n", msg.content),
                MessageRole::User => format!("User: {}\n", msg.content),
                MessageRole::Assistant => format!("Assistant: {}\n", msg.content),
            })
            .collect::<String>()
            + "Assistant:";

        tracing::info!(
            model = %task.model,
            max_tokens = task.max_tokens,
            prompt_len = prompt.len(),
            "Executing inference locally (streaming)"
        );

        if let Some(engine) = &self.inference_engine {
            let request = GenerateRequest {
                model: task.model.clone(),
                prompt,
                max_tokens: task.max_tokens,
                temperature: task.temperature,
                top_p: 0.9,
                stop_sequences: task.stop_sequences.clone(),
            };

            let response = engine
                .generate(&request)
                .await
                .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

            let _ = delta_tx.send(response.text.clone());

            tracing::info!(
                tokens = response.tokens_generated,
                tps = format!("{:.1}", response.tokens_per_second),
                model_id = %response.model_id,
                "Inference complete (streaming path, batched output)"
            );

            let finish_reason = match response.finish_reason {
                crate::inference::FinishReason::Stop => FinishReason::Stop,
                crate::inference::FinishReason::Length => FinishReason::Length,
                crate::inference::FinishReason::ContentFilter => FinishReason::ContentFilter,
            };

            return Ok(TaskData::Inference(InferenceResult {
                text: response.text,
                tokens_generated: response.tokens_generated,
                tokens_per_second: response.tokens_per_second,
                finish_reason,
            }));
        }

        let actual_path = self.find_gguf_model(&task.model)?;

        tracing::info!(
            model_path = %actual_path.display(),
            "Loading model for streaming inference (direct GGUF)"
        );

        let engine = self.gguf_engine.read().await;

        let model_handle = engine
            .load(&actual_path)
            .map_err(|e| ExecutorError::InferenceError(format!("Failed to load model: {}", e)))?;

        let request = GenerateRequest {
            model: task.model.clone(),
            prompt,
            max_tokens: task.max_tokens,
            temperature: task.temperature,
            top_p: 0.9,
            stop_sequences: task.stop_sequences.clone(),
        };

        let stream_tx = delta_tx.clone();
        let callback: TokenCallback = Box::new(move |token: &str| {
            let _ = stream_tx.send(token.to_string());
        });

        let response = engine
            .generate_streaming(&model_handle, &request, callback)
            .map_err(|e| {
                ExecutorError::InferenceError(format!("Streaming generation failed: {}", e))
            })?;

        tracing::info!(
            tokens = response.tokens_generated,
            tps = format!("{:.1}", response.tokens_per_second),
            "Streaming inference complete"
        );

        let finish_reason = match response.finish_reason {
            crate::inference::FinishReason::Stop => FinishReason::Stop,
            crate::inference::FinishReason::Length => FinishReason::Length,
            crate::inference::FinishReason::ContentFilter => FinishReason::ContentFilter,
        };

        Ok(TaskData::Inference(InferenceResult {
            text: response.text,
            tokens_generated: response.tokens_generated,
            tokens_per_second: response.tokens_per_second,
            finish_reason,
        }))
    }

    /// Find a GGUF model file by name, with fuzzy matching and fallback.
    fn find_gguf_model(&self, model_name: &str) -> Result<std::path::PathBuf, ExecutorError> {
        let model_filename = format!("{}.gguf", model_name.to_lowercase().replace(" ", "-"));
        let model_path = self.config.models_dir.join(&model_filename);

        if model_path.exists() {
            return Ok(model_path);
        }

        // Look for any gguf file that contains the model name
        let norm = |s: &str| s.to_lowercase().replace('-', "").replace(' ', "");
        let needle = norm(model_name);

        let mut found_path = None;
        if let Ok(entries) = std::fs::read_dir(&self.config.models_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "gguf") {
                    let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    let hay = norm(filename);
                    if hay.contains(&needle) || needle.contains(&hay) {
                        return Ok(path);
                    }
                    if found_path.is_none() {
                        found_path = Some(path);
                    }
                }
            }
        }

        // Fall back to first available gguf
        match found_path {
            Some(p) => Ok(p),
            None => Err(ExecutorError::InferenceError(format!(
                "Model not found: {}. No GGUF models in {}",
                model_name,
                self.config.models_dir.display()
            ))),
        }
    }

    /// Execute web fetch locally.
    async fn execute_web_fetch_local(&self, task: WebFetchTask) -> Result<TaskData, ExecutorError> {
        use reqwest::Client;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(task.timeout_secs as u64))
            .build()
            .map_err(|e| ExecutorError::WebError(e.to_string()))?;

        let mut request = match task.method {
            HttpMethod::Get => client.get(&task.url),
            HttpMethod::Post => client.post(&task.url),
            HttpMethod::Put => client.put(&task.url),
            HttpMethod::Delete => client.delete(&task.url),
            HttpMethod::Patch => client.patch(&task.url),
            HttpMethod::Head => client.head(&task.url),
        };

        // Add headers
        for (key, value) in &task.headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if present
        if let Some(body) = task.body {
            request = request.body(body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ExecutorError::WebError(e.to_string()))?;

        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .bytes()
            .await
            .map_err(|e| ExecutorError::WebError(e.to_string()))?
            .to_vec();

        Ok(TaskData::WebFetch(WebFetchResult {
            status,
            headers,
            body,
        }))
    }

    /// Execute web search locally.
    async fn execute_web_search_local(
        &self,
        task: WebSearchTask,
    ) -> Result<TaskData, ExecutorError> {
        // TODO: Implement actual web search
        // For now, return placeholder
        tracing::info!(
            query = %task.query,
            engine = ?task.engine,
            "Would execute web search"
        );

        Ok(TaskData::WebSearch(WebSearchResult { results: vec![] }))
    }

    /// Execute WASM tool locally.
    async fn execute_wasm_local(&self, task: WasmTask) -> Result<TaskData, ExecutorError> {
        // TODO: Implement via wasm sandbox module
        tracing::info!(
            tool_hash = %task.tool_hash,
            function = %task.function,
            "Would execute WASM tool"
        );

        Ok(TaskData::Wasm(WasmResult {
            value: serde_json::json!({"status": "not_implemented"}),
            fuel_consumed: 0,
        }))
    }

    /// Get current resource state.
    pub async fn resource_state(&self) -> ResourceState {
        self.resource_monitor.current_state().await
    }

    /// Get the resource monitor.
    pub fn resource_monitor(&self) -> Arc<ResourceMonitor> {
        self.resource_monitor.clone()
    }

    /// Check if a model is loaded locally.
    pub async fn has_model(&self, model_id: &str) -> bool {
        let state = self.resource_monitor.current_state().await;
        state.has_model(model_id)
    }

    /// Execute inference locally with streaming - prints tokens directly to stdout.
    /// Returns the final InferenceResult after completion.
    ///
    /// When an `InferenceEngine` is set, delegates to it (non-streaming for now,
    /// since the high-level engine doesn't yet expose a streaming API).
    /// Otherwise uses the direct GGUF streaming path.
    pub async fn execute_inference_streaming_print(
        &self,
        task: InferenceTask,
    ) -> Result<InferenceResult, ExecutorError> {
        use std::io::Write;

        // Build prompt with messages
        let prompt = task
            .messages
            .iter()
            .map(|m| match m.role {
                MessageRole::System => format!("System: {}\n\n", m.content),
                MessageRole::User => format!("User: {}\n", m.content),
                MessageRole::Assistant => format!("Assistant: {}\n\n", m.content),
            })
            .collect::<String>()
            + "Assistant:";

        tracing::info!(
            model = %task.model,
            max_tokens = task.max_tokens,
            "Executing streaming inference"
        );

        // --- High-level InferenceEngine path ---
        if let Some(engine) = &self.inference_engine {
            let request = GenerateRequest {
                model: task.model.clone(),
                prompt,
                max_tokens: task.max_tokens,
                temperature: task.temperature,
                top_p: 0.9,
                stop_sequences: task.stop_sequences.clone(),
            };

            let response = engine
                .generate(&request)
                .await
                .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

            // Print the full response (non-streaming for now)
            print!("{}", response.text);
            let _ = std::io::stdout().flush();

            let finish_reason = match response.finish_reason {
                crate::inference::FinishReason::Stop => FinishReason::Stop,
                crate::inference::FinishReason::Length => FinishReason::Length,
                crate::inference::FinishReason::ContentFilter => FinishReason::ContentFilter,
            };

            return Ok(InferenceResult {
                text: response.text,
                tokens_generated: response.tokens_generated,
                tokens_per_second: response.tokens_per_second,
                finish_reason,
            });
        }

        // --- Direct GGUF streaming fallback ---
        let actual_path = self.find_gguf_model(&task.model)?;

        let engine = self.gguf_engine.read().await;
        let model_handle = engine
            .load(&actual_path)
            .map_err(|e| ExecutorError::InferenceError(format!("Failed to load model: {}", e)))?;

        let request = GenerateRequest {
            model: task.model.clone(),
            prompt,
            max_tokens: task.max_tokens,
            temperature: task.temperature,
            top_p: 0.9,
            stop_sequences: task.stop_sequences.clone(),
        };

        let callback: TokenCallback = Box::new(|token: &str| {
            print!("{}", token);
            let _ = std::io::stdout().flush();
        });

        let response = engine
            .generate_streaming(&model_handle, &request, callback)
            .map_err(|e| {
                ExecutorError::InferenceError(format!("Streaming generation failed: {}", e))
            })?;

        let finish_reason = match response.finish_reason {
            crate::inference::FinishReason::Stop => FinishReason::Stop,
            crate::inference::FinishReason::Length => FinishReason::Length,
            crate::inference::FinishReason::ContentFilter => FinishReason::ContentFilter,
        };

        Ok(InferenceResult {
            text: response.text,
            tokens_generated: response.tokens_generated,
            tokens_per_second: response.tokens_per_second,
            finish_reason,
        })
    }
}

/// Executor configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Models directory
    pub models_dir: std::path::PathBuf,
    /// Maximum response size for web fetch (bytes)
    pub max_web_response_size: usize,
    /// Default timeout for web requests (seconds)
    pub default_web_timeout_secs: u32,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            models_dir: crate::bootstrap::base_dir().join("models"),
            max_web_response_size: 10 * 1024 * 1024, // 10 MB
            default_web_timeout_secs: 30,
        }
    }
}

/// Errors from task execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("Insufficient resources: {0}")]
    InsufficientResources(String),

    #[error("Network unavailable for remote execution")]
    NetworkUnavailable,

    #[error("Web error: {0}")]
    WebError(String),

    #[error("Inference error: {0}")]
    InferenceError(String),

    #[error("WASM error: {0}")]
    WasmError(String),

    #[error("Timeout")]
    Timeout,

    #[error("Task cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_executor() -> TaskExecutor {
        let monitor = Arc::new(ResourceMonitor::with_defaults());
        // Disable network offload so tests run locally
        let mut router_config = RouterConfig::default();
        router_config.allow_network_offload = false;
        TaskExecutor::new(monitor, router_config, ExecutorConfig::default())
    }

    #[tokio::test]
    async fn test_execute_web_fetch_local() {
        let executor = create_executor();

        // Execute locally directly
        let task = WebFetchTask::get("https://httpbin.org/get");
        let result = executor
            .execute_web_fetch_local(task)
            .await
            .expect("Web fetch should succeed");

        match result {
            TaskData::WebFetch(fetch_result) => {
                assert_eq!(fetch_result.status, 200);
            }
            _ => panic!("Expected WebFetch result"),
        }
    }

    #[tokio::test]
    async fn test_execute_inference_placeholder() {
        let executor = create_executor();

        // Execute locally directly
        let task = InferenceTask::new("llama-7b", "Hello, world!");
        let result = executor.execute_inference_local(task).await;

        // Test passes if inference succeeds OR if no models are available
        match result {
            Ok(TaskData::Inference(_)) => {}
            Ok(_) => panic!("Expected Inference result"),
            Err(e) => {
                // Accept "no models" error - this is expected in CI/test environments
                let err_str = e.to_string();
                assert!(
                    err_str.contains("Model not found") || err_str.contains("No GGUF models"),
                    "Unexpected error: {}",
                    err_str
                );
            }
        }
    }

    #[tokio::test]
    async fn test_resource_state() {
        let monitor = Arc::new(ResourceMonitor::with_defaults());
        let state = monitor.current_state().await;

        // Default state should have some defaults
        assert_eq!(state.active_inference_tasks, 0);
    }
}
