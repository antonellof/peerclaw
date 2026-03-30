//! Agent runtime - the core agentic execution loop.
//!
//! Implements a ReAct-style loop: LLM generates thoughts and tool calls,
//! tools are executed, results fed back, until a final answer is produced.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::executor::task::{ChatMessage, ExecutionTask, InferenceTask, MessageRole, TaskData};
use crate::executor::TaskExecutor;
use crate::mcp::McpManager;
use crate::tools::{NodeToolTx, ToolContext, ToolRegistry};
use crate::vector::VectorStore;

use super::budget::BudgetTracker;
use super::compaction;
use super::session::SessionStore;
use super::spec::AgentSpec;
use super::unified_loop::{run_unified_agentic_loop, AgenticInferenceSink, AgenticProgressSink};

/// Configuration for an agent runtime instance.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    /// Model context window size in tokens. Used to trigger context compaction.
    /// Default: 4096 (conservative for small local GGUF models).
    pub context_window: u32,
}

/// Default context window size in tokens for small local models.
const DEFAULT_CONTEXT_WINDOW: u32 = 4096;

impl AgentConfig {
    /// Create from an AgentSpec.
    pub fn from_spec(spec: &AgentSpec) -> Self {
        Self {
            id: format!(
                "agent_{}",
                &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]
            ),
            name: spec.agent.name.clone(),
            model: spec.model.name.clone(),
            max_tokens: spec.model.max_tokens,
            temperature: spec.model.temperature,
            system_prompt: spec.model.system_prompt.clone(),
            allowed_tools: spec.tools.builtin.clone(),
            context_window: spec.model.context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW),
        }
    }
}

/// Result of an agent task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// Final answer text
    pub answer: String,
    /// Tool calls made during execution
    pub tool_calls: Vec<ToolCallRecord>,
    /// Number of LLM iterations
    pub iterations: u32,
    /// Total tokens consumed
    pub total_tokens: u32,
    /// Budget spent on this task
    pub budget_spent: f64,
    /// Whether the task completed successfully
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Record of a tool call made during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: String,
    pub success: bool,
    pub duration_ms: u64,
}

/// Extra options for dashboard tasks (MCP, skill block, context size) when using the unified loop.
#[derive(Clone, Default)]
pub struct AgentTaskExtras {
    pub use_mcp: bool,
    pub mcp: Option<Arc<McpManager>>,
    pub skill_block: String,
    /// Character budget for prompt (from inference engine ctx × 4); 0 = default 48k.
    pub model_ctx_chars: usize,
}

/// Append agentic progress lines to [`AgentRuntime::task_log`].
struct TaskLogProgressSink {
    inner: Arc<RwLock<Vec<String>>>,
}

#[async_trait]
impl AgenticProgressSink for TaskLogProgressSink {
    async fn set_react_pass(&self, pass: u32) {
        let line = format!(
            "[{}] Pass {}/{}",
            chrono::Utc::now().format("%H:%M:%S"),
            pass,
            super::unified_loop::AGENTIC_MAX_ITERS
        );
        self.inner.write().await.push(line);
    }

    async fn append_log(&self, line: String) {
        self.inner.write().await.push(line);
    }

    async fn set_tokens(&self, _tokens: u32) {}

    async fn record_tool_step(&self, line: String, _tokens: u32) {
        self.inner.write().await.push(line);
    }
}

/// Maximum number of iterations before stopping.
const MAX_ITERATIONS: u32 = 20;

/// Estimated cost per 1k tokens (in PCLAW) for budget tracking.
/// Aligned with EconomyConfig default (0.5 PCLAW / 1K tokens for small models).
const COST_PER_1K_TOKENS: f64 = 0.5;

/// The core agent runtime.
pub struct AgentRuntime {
    pub config: AgentConfig,
    executor: Arc<TaskExecutor>,
    tools: Arc<ToolRegistry>,
    conversation: Vec<ChatMessage>,
    budget: BudgetTracker,
    tool_context: ToolContext,
    /// Task log for the web UI
    pub task_log: Arc<RwLock<Vec<String>>>,
    /// Persistent session store (optional).
    pub session_store: Option<Arc<SessionStore>>,
    /// Vector store for cross-session agent memory (optional).
    pub vector_store: Option<Arc<VectorStore>>,
    /// When set (e.g. `peerclaw serve` with web), tasks use the shared unified tool+MCP loop.
    pub inference_sink: Option<Arc<dyn AgenticInferenceSink>>,
    /// Shared prompt fragments (same as node `Runtime`).
    pub prompts: Arc<crate::prompts::PromptBundle>,
}

impl AgentRuntime {
    /// Create a new agent runtime.
    pub fn new(
        config: AgentConfig,
        executor: Arc<TaskExecutor>,
        tools: Arc<ToolRegistry>,
        budget: BudgetTracker,
        peer_id: String,
        node_tool_tx: Option<NodeToolTx>,
        prompts: Arc<crate::prompts::PromptBundle>,
        inference_sink: Option<Arc<dyn AgenticInferenceSink>>,
    ) -> Self {
        let tool_context = ToolContext {
            session_id: config.id.clone(),
            job_id: None,
            peer_id,
            working_dir: std::env::current_dir().unwrap_or_default(),
            sandboxed: false,
            available_secrets: vec![],
            node_tool_tx,
            egress_policy: None,
            agent_depth: 0,
        };

        Self {
            config,
            executor,
            tools,
            conversation: Vec::new(),
            budget,
            tool_context,
            task_log: Arc::new(RwLock::new(Vec::new())),
            session_store: None,
            vector_store: None,
            inference_sink,
            prompts,
        }
    }

    /// Attach a vector store for cross-session agent memory.
    pub fn with_vector_store(mut self, store: Arc<VectorStore>) -> Self {
        self.vector_store = Some(store);
        self
    }

    /// Attach a persistent session store to this runtime.
    pub fn with_session_store(mut self, store: Arc<SessionStore>) -> Self {
        self.session_store = Some(store);
        self
    }

    /// Create from an AgentSpec.
    pub fn from_spec(
        spec: &AgentSpec,
        executor: Arc<TaskExecutor>,
        tools: Arc<ToolRegistry>,
        peer_id: String,
        node_tool_tx: Option<NodeToolTx>,
        prompts: Arc<crate::prompts::PromptBundle>,
        inference_sink: Option<Arc<dyn AgenticInferenceSink>>,
    ) -> Self {
        let config = AgentConfig::from_spec(spec);
        let budget = BudgetTracker::new(
            spec.budget.per_request,
            spec.budget.per_hour,
            spec.budget.per_day,
            spec.budget.total,
        );
        Self::new(
            config,
            executor,
            tools,
            budget,
            peer_id,
            node_tool_tx,
            prompts,
            inference_sink,
        )
    }

    /// Run a task through the agentic loop.
    ///
    /// When `stop` is set, it is polled at the start of each iteration (after the current LLM or
    /// tool work finishes).
    ///
    /// When `session_id` is provided and a [`SessionStore`] is attached, prior conversation turns
    /// are loaded into context before the task starts, and the user input + final answer are
    /// persisted after the task completes.
    pub async fn run_task(&mut self, user_input: &str, stop: Option<&AtomicBool>) -> AgentResult {
        self.run_task_with_session(user_input, stop, None, AgentTaskExtras::default())
            .await
    }

    /// Like [`run_task`](Self::run_task) but with an explicit session id for conversation continuity.
    pub async fn run_task_with_session(
        &mut self,
        user_input: &str,
        stop: Option<&AtomicBool>,
        session_id: Option<&str>,
        extras: AgentTaskExtras,
    ) -> AgentResult {
        if let Some(sink) = self.inference_sink.clone() {
            return self
                .run_task_via_unified_loop(user_input, stop, session_id, extras, sink.as_ref())
                .await;
        }

        self.budget.new_request();
        let mut tool_calls = Vec::new();
        let mut iterations = 0u32;
        let mut total_tokens = 0u32;

        self.log(&format!("Task received: {}", user_input)).await;

        // Build system prompt with tool descriptions
        let system = self.build_system_prompt(user_input).await;

        // Initialize conversation for this task
        self.conversation.clear();
        self.conversation.push(ChatMessage::system(&system));

        // If a session_id is provided, load recent history from the session store.
        if let (Some(sid), Some(store)) = (session_id, &self.session_store) {
            const MAX_HISTORY_TURNS: usize = 40;
            match store.load_session(sid, MAX_HISTORY_TURNS) {
                Ok(turns) if !turns.is_empty() => {
                    self.log(&format!(
                        "Loaded {} prior turns from session '{}'",
                        turns.len(),
                        sid
                    ))
                    .await;
                    for turn in &turns {
                        match turn.role.as_str() {
                            "user" => self.conversation.push(ChatMessage::user(&turn.content)),
                            "assistant" => self
                                .conversation
                                .push(ChatMessage::assistant(&turn.content)),
                            _ => self.conversation.push(ChatMessage::user(&turn.content)),
                        }
                    }
                }
                Err(e) => {
                    self.log(&format!("Failed to load session '{}': {}", sid, e))
                        .await;
                }
                _ => {}
            }
        }

        self.conversation.push(ChatMessage::user(user_input));

        loop {
            iterations += 1;

            if stop.is_some_and(|s| s.load(Ordering::Acquire)) {
                self.log("Stopped by user").await;
                return AgentResult {
                    answer: self.extract_last_text(),
                    tool_calls,
                    iterations,
                    total_tokens,
                    budget_spent: self.budget.total_spent(),
                    success: false,
                    error: Some("Stopped by user".to_string()),
                };
            }

            if iterations > MAX_ITERATIONS {
                self.log("Max iterations reached, returning partial result")
                    .await;
                return AgentResult {
                    answer: self.extract_last_text(),
                    tool_calls,
                    iterations,
                    total_tokens,
                    budget_spent: self.budget.total_spent(),
                    success: false,
                    error: Some("Max iterations reached".to_string()),
                };
            }

            // Check budget
            let estimated_cost = (self.config.max_tokens as f64 / 1000.0) * COST_PER_1K_TOKENS;
            if !self.budget.can_spend(estimated_cost) {
                self.log("Budget exhausted").await;
                return AgentResult {
                    answer: self.extract_last_text(),
                    tool_calls,
                    iterations,
                    total_tokens,
                    budget_spent: self.budget.total_spent(),
                    success: false,
                    error: Some("Budget exhausted".to_string()),
                };
            }

            // Compact context if approaching the model's context window limit.
            if compaction::needs_compaction(&self.conversation, self.config.context_window) {
                let max_chars = (self.config.context_window as usize) * 4 * 3 / 4; // 75% of window
                let before = self.conversation.len();
                compaction::prune_conversation(&mut self.conversation, max_chars);
                let after = self.conversation.len();
                if before != after {
                    self.log(&format!(
                        "Context compacted: {} → {} messages ({} chars budget)",
                        before, after, max_chars
                    ))
                    .await;
                }
            }

            // Call LLM
            self.log(&format!(
                "LLM iteration {} (model: {})",
                iterations, self.config.model
            ))
            .await;

            let task = InferenceTask {
                model: self.config.model.clone(),
                messages: self.conversation.clone(),
                max_tokens: self.config.max_tokens,
                temperature: self.config.temperature,
                stop_sequences: vec![],
                stream: false,
            };

            let result = self.executor.execute(ExecutionTask::Inference(task)).await;

            let response_text = match result {
                Ok(task_result) => match &task_result.data {
                    TaskData::Inference(r) => {
                        total_tokens += r.tokens_generated;
                        let cost = (r.tokens_generated as f64 / 1000.0) * COST_PER_1K_TOKENS;
                        self.budget.spend(cost);
                        r.text.clone()
                    }
                    TaskData::Error(e) => {
                        return AgentResult {
                            answer: String::new(),
                            tool_calls,
                            iterations,
                            total_tokens,
                            budget_spent: self.budget.total_spent(),
                            success: false,
                            error: Some(format!("Inference error: {}", e)),
                        };
                    }
                    _ => {
                        return AgentResult {
                            answer: String::new(),
                            tool_calls,
                            iterations,
                            total_tokens,
                            budget_spent: self.budget.total_spent(),
                            success: false,
                            error: Some("Unexpected response type".to_string()),
                        };
                    }
                },
                Err(e) => {
                    return AgentResult {
                        answer: String::new(),
                        tool_calls,
                        iterations,
                        total_tokens,
                        budget_spent: self.budget.total_spent(),
                        success: false,
                        error: Some(format!("Executor error: {}", e)),
                    };
                }
            };

            // Add assistant response to conversation
            self.conversation
                .push(ChatMessage::assistant(&response_text));

            // Check for tool calls in the response
            let mut parsed_calls = parse_tool_calls(&response_text);

            if parsed_calls.is_empty() {
                // No tool calls → this is the final answer
                self.log("Final answer produced").await;
                let trimmed_raw = response_text.trim();
                let cleaned = extract_answer(&response_text);
                let answer = if !cleaned.trim().is_empty() {
                    cleaned
                } else if !trimmed_raw.is_empty() {
                    trimmed_raw.to_string()
                } else {
                    "(No text in the model's final reply after stripping tool markup. See logs or retry.)"
                        .to_string()
                };

                // Persist to session store if available.
                if let (Some(sid), Some(store)) = (session_id, &self.session_store) {
                    if let Err(e) = store.save_turn(sid, "user", user_input) {
                        tracing::warn!(session = sid, "Failed to save user turn: {}", e);
                    }
                    if let Err(e) = store.save_turn(sid, "assistant", &answer) {
                        tracing::warn!(session = sid, "Failed to save assistant turn: {}", e);
                    }
                }

                // Auto-save task summary to vector store for cross-session memory
                if self.vector_store.is_some() {
                    self.save_task_memory(user_input, &answer).await;
                }

                return AgentResult {
                    answer,
                    tool_calls,
                    iterations,
                    total_tokens,
                    budget_spent: self.budget.total_spent(),
                    success: true,
                    error: None,
                };
            }

            let model_tool_call_count = parsed_calls.len();
            let mut seen_sig: HashSet<(String, String)> = HashSet::new();
            parsed_calls.retain(|call| {
                let sig = (call.name.clone(), call.args.to_string());
                seen_sig.insert(sig)
            });
            let duplicate_calls_merged = model_tool_call_count.saturating_sub(parsed_calls.len());
            if duplicate_calls_merged > 0 {
                self.log(&format!(
                    "Merged {} duplicate tool call(s) (identical name+args); {} unique this turn.",
                    duplicate_calls_merged,
                    parsed_calls.len()
                ))
                .await;
                self.conversation.push(ChatMessage::user(format!(
                    "(System: merged {duplicate_calls_merged} duplicate tool call(s); each unique call runs once.)"
                )));
            }

            // Execute tool calls
            for call in parsed_calls {
                if stop.is_some_and(|s| s.load(Ordering::Acquire)) {
                    self.log("Stopped by user").await;
                    return AgentResult {
                        answer: self.extract_last_text(),
                        tool_calls,
                        iterations,
                        total_tokens,
                        budget_spent: self.budget.total_spent(),
                        success: false,
                        error: Some("Stopped by user".to_string()),
                    };
                }

                self.log(&format!(
                    "Calling tool: {} with args: {}",
                    call.name, call.args
                ))
                .await;

                // Check if tool is allowed
                if !self.config.allowed_tools.is_empty()
                    && !self.config.allowed_tools.contains(&call.name)
                {
                    let error_msg = format!("Tool '{}' is not in allowed tools list", call.name);
                    self.log(&error_msg).await;
                    self.conversation
                        .push(ChatMessage::user(format!("Tool error: {}", error_msg)));
                    tool_calls.push(ToolCallRecord {
                        tool_name: call.name,
                        args: call.args,
                        result: error_msg,
                        success: false,
                        duration_ms: 0,
                    });
                    continue;
                }

                let start = std::time::Instant::now();
                let tool_result = self
                    .tools
                    .execute_local(&call.name, call.args.clone(), &self.tool_context)
                    .await;

                let duration_ms = start.elapsed().as_millis() as u64;

                match tool_result {
                    Ok(result) => {
                        let result_text = if let Some(msg) = &result.output.message {
                            msg.clone()
                        } else {
                            serde_json::to_string_pretty(&result.output.data)
                                .unwrap_or_else(|_| "{}".to_string())
                        };

                        self.log(&format!(
                            "Tool '{}' succeeded ({} ms)",
                            call.name, duration_ms
                        ))
                        .await;

                        // Feed result back into conversation
                        self.conversation.push(ChatMessage::user(format!(
                            "Tool result for {}:\n{}",
                            call.name, result_text
                        )));

                        tool_calls.push(ToolCallRecord {
                            tool_name: call.name,
                            args: call.args,
                            result: result_text,
                            success: true,
                            duration_ms,
                        });
                    }
                    Err(e) => {
                        let error_msg = format!("Tool error: {}", e);
                        self.log(&format!("Tool '{}' failed: {}", call.name, e))
                            .await;

                        self.conversation.push(ChatMessage::user(format!(
                            "Tool '{}' failed: {}",
                            call.name, e
                        )));

                        tool_calls.push(ToolCallRecord {
                            tool_name: call.name,
                            args: call.args,
                            result: error_msg,
                            success: false,
                            duration_ms,
                        });
                    }
                }
            }

            // Continue the loop - LLM will see tool results and decide next steps
        }
    }

    async fn run_task_via_unified_loop(
        &mut self,
        user_input: &str,
        stop: Option<&AtomicBool>,
        session_id: Option<&str>,
        extras: AgentTaskExtras,
        sink: &dyn AgenticInferenceSink,
    ) -> AgentResult {
        self.budget.new_request();
        self.log(&format!("Task received: {}", user_input)).await;

        let model_ctx = if extras.model_ctx_chars > 0 {
            extras.model_ctx_chars
        } else {
            48_000
        };

        let mut history = String::new();
        if let (Some(sid), Some(store)) = (session_id, &self.session_store) {
            const MAX_HISTORY_TURNS: usize = 40;
            if let Ok(turns) = store.load_session(sid, MAX_HISTORY_TURNS) {
                if !turns.is_empty() {
                    history.push_str(
                        self.prompts
                            .agent_runtime_prior_conversation_header
                            .trim_end_matches('\n'),
                    );
                    history.push('\n');
                    for turn in &turns {
                        let label = match turn.role.as_str() {
                            "user" => "User",
                            "assistant" => "Assistant",
                            _ => "Message",
                        };
                        history.push_str(&format!("{}: {}\n\n", label, turn.content));
                    }
                    history.push('\n');
                    self.log(&format!(
                        "Loaded {} prior turns from session '{}'",
                        turns.len(),
                        sid
                    ))
                    .await;
                }
            }
        }

        let spec_system = self.config.system_prompt.trim();
        let system_block = if spec_system.is_empty() {
            String::new()
        } else {
            self.prompts.agent_runtime_instructions_block(spec_system)
        };

        let body = format!("{}{}{}", system_block, extras.skill_block, history)
            + &self.prompts.agent_runtime_task_body(user_input);

        let mcp = if extras.use_mcp {
            extras.mcp.clone()
        } else {
            None
        };
        let include_mcp = extras.use_mcp && mcp.as_ref().is_some_and(|m| m.tool_count() > 0);

        let allowed = if self.config.allowed_tools.is_empty() {
            None
        } else {
            Some(self.config.allowed_tools.as_slice())
        };

        let progress: Arc<dyn AgenticProgressSink> = Arc::new(TaskLogProgressSink {
            inner: self.task_log.clone(),
        });

        match run_unified_agentic_loop(
            sink,
            self.prompts.as_ref(),
            Some(self.tools.clone()),
            mcp,
            include_mcp,
            allowed,
            body,
            self.config.model.clone(),
            self.config.max_tokens,
            self.config.temperature,
            model_ctx,
            self.tool_context.peer_id.clone(),
            self.tool_context.node_tool_tx.clone(),
            Some(progress),
            stop,
            false,
        )
        .await
        {
            Ok((out, _logs, tool_calls, iterations)) => {
                let cost = (out.tokens_generated as f64 / 1000.0) * COST_PER_1K_TOKENS;
                self.budget.spend(cost);
                let answer = out.text.clone();
                if let (Some(sid), Some(store)) = (session_id, &self.session_store) {
                    if let Err(e) = store.save_turn(sid, "user", user_input) {
                        tracing::warn!(session = sid, "Failed to save user turn: {}", e);
                    }
                    if let Err(e) = store.save_turn(sid, "assistant", &answer) {
                        tracing::warn!(session = sid, "Failed to save assistant turn: {}", e);
                    }
                }
                if self.vector_store.is_some() {
                    self.save_task_memory(user_input, &answer).await;
                }
                AgentResult {
                    answer,
                    tool_calls,
                    iterations,
                    total_tokens: out.tokens_generated,
                    budget_spent: self.budget.total_spent(),
                    success: true,
                    error: None,
                }
            }
            Err(e) => {
                let user_stop = e == "Stopped by user";
                AgentResult {
                    answer: if user_stop {
                        "Stopped by user.".to_string()
                    } else {
                        String::new()
                    },
                    tool_calls: Vec::new(),
                    iterations: 0,
                    total_tokens: 0,
                    budget_spent: self.budget.total_spent(),
                    success: false,
                    error: Some(e),
                }
            }
        }
    }

    /// Build the system prompt with tool descriptions and recalled memories.
    async fn build_system_prompt(&self, user_input: &str) -> String {
        let mut prompt = self.config.system_prompt.clone();

        if prompt.is_empty() {
            prompt = self.prompts.agent_legacy_default_name(&self.config.name);
            if !prompt.ends_with('\n') {
                prompt.push('\n');
            }
        }

        // Inject recalled memories from vector store (cross-session learning)
        if let Some(store) = &self.vector_store {
            let memories = self.recall_memories(store, user_input).await;
            if !memories.is_empty() {
                prompt.push_str(
                    self.prompts
                        .agent_recalled_memories_header
                        .trim_end_matches('\n'),
                );
                prompt.push('\n');
                for (i, memory) in memories.iter().enumerate() {
                    prompt.push_str(&format!("{}. {}\n", i + 1, memory));
                }
            }
        }

        // Add tool descriptions
        let tools = self.tools.list_tools().await;
        let available: Vec<_> = if self.config.allowed_tools.is_empty() {
            tools
        } else {
            tools
                .into_iter()
                .filter(|t| self.config.allowed_tools.contains(&t.name))
                .collect()
        };

        if !available.is_empty() {
            prompt.push_str(&self.prompts.agent_legacy_tool_block);
            if !self.prompts.agent_legacy_tool_block.ends_with('\n') {
                prompt.push('\n');
            }
            for tool in &available {
                let desc: String = tool.description.chars().take(200).collect();
                // Include required parameter names so the model knows the schema
                let params_hint = if tool.required_params.is_empty() {
                    String::new()
                } else {
                    format!(" Params: {}", tool.required_params.join(", "))
                };
                prompt.push_str(&format!("- {}: {}{}\n", tool.name, desc, params_hint));
            }
        }

        prompt
    }

    /// Search the vector store for memories relevant to the current task.
    /// Returns up to 5 text snippets.
    async fn recall_memories(&self, store: &VectorStore, query: &str) -> Vec<String> {
        const MEMORY_COLLECTION: &str = "memories";
        const MAX_MEMORIES: usize = 5;
        const MIN_SCORE: f32 = 0.25;

        // Ensure the memories collection exists
        let collection_exists = store
            .list_collections()
            .iter()
            .any(|c| c.name == MEMORY_COLLECTION);

        if !collection_exists {
            return Vec::new();
        }

        // Generate embedding for the query
        let embedder = crate::vector::get_embedder();
        let embedding = match embedder.embed(query).await {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!(error = %err, "Failed to embed query for memory recall");
                return Vec::new();
            }
        };

        // Search the vector store
        let results = match store.search(MEMORY_COLLECTION, embedding, MAX_MEMORIES * 2) {
            Ok(r) => r,
            Err(err) => {
                tracing::debug!(error = %err, "Memory search failed");
                return Vec::new();
            }
        };

        // Filter by minimum score and take top results
        results
            .into_iter()
            .filter(|r| r.score >= MIN_SCORE)
            .take(MAX_MEMORIES)
            .filter_map(|r| {
                r.text.or_else(|| {
                    r.payload
                        .as_ref()
                        .and_then(|p| p.get("text"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
            })
            .collect()
    }

    /// Save a task summary to the vector store for cross-session memory.
    async fn save_task_memory(&self, user_input: &str, answer: &str) {
        const MEMORY_COLLECTION: &str = "memories";

        let store = match &self.vector_store {
            Some(s) => s,
            None => return,
        };

        // Ensure collection exists
        if let Err(e) = store.get_or_create_collection(MEMORY_COLLECTION) {
            tracing::debug!(error = %e, "Failed to get/create memories collection");
            return;
        }

        // Build a concise summary for embedding
        let summary_answer = if answer.len() > 500 {
            &answer[..500]
        } else {
            answer
        };
        let content = format!("Task: {}\nResult: {}", user_input, summary_answer);

        // Generate embedding
        let embedder = crate::vector::get_embedder();
        let embedding = match embedder.embed(&content).await {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!(error = %err, "Failed to embed task memory");
                return;
            }
        };

        // Generate a unique ID
        let id = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(content.as_bytes());
            hasher.update(chrono::Utc::now().to_rfc3339().as_bytes());
            hasher.finalize().to_hex()[..16].to_string()
        };

        let now = chrono::Utc::now();
        let payload = serde_json::json!({
            "text": content,
            "category": "agent_task",
            "agent_name": self.config.name,
            "source_peer": self.tool_context.peer_id,
            "created_at": now.to_rfc3339(),
            "modified_at": now.to_rfc3339(),
        });

        if let Err(e) = store.upsert(MEMORY_COLLECTION, &id, embedding, Some(payload)) {
            tracing::debug!(error = %e, "Failed to save task memory");
        } else {
            tracing::info!(
                agent = %self.config.name,
                memory_id = %id,
                "Saved task memory for cross-session recall"
            );
        }
    }

    /// Log a message to the task log.
    async fn log(&self, message: &str) {
        let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();
        let entry = format!("[{}] {}", timestamp, message);
        tracing::info!(agent = %self.config.name, "{}", message);
        self.task_log.write().await.push(entry);
    }

    /// Extract the last assistant text (fallback for partial results).
    fn extract_last_text(&self) -> String {
        self.conversation
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant)
            .map(|m| extract_answer(&m.content))
            .unwrap_or_default()
    }

    /// Get the task log.
    pub async fn get_log(&self) -> Vec<String> {
        self.task_log.read().await.clone()
    }

    /// Clear the task log.
    pub async fn clear_log(&self) {
        self.task_log.write().await.clear();
    }
}

/// A parsed tool call from LLM output.
#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub name: String,
    pub args: serde_json::Value,
}

fn flush_tool_call_arg_buffer(acc: &mut Vec<String>, args: &mut serde_json::Value) {
    if acc.is_empty() {
        return;
    }
    let buf = acc.join("\n");
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&buf) {
        *args = parsed;
        acc.clear();
    }
}

fn parse_tool_call_block(block: &str) -> Option<ParsedToolCall> {
    let mut name: Option<String> = None;
    let mut args = serde_json::json!({});
    let mut acc: Vec<String> = Vec::new();
    let mut in_args = false;

    for line in block.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(n) = line.strip_prefix("name:") {
            if in_args {
                flush_tool_call_arg_buffer(&mut acc, &mut args);
            }
            in_args = false;
            acc.clear();
            name = Some(n.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("args:") {
            if in_args {
                flush_tool_call_arg_buffer(&mut acc, &mut args);
            }
            in_args = true;
            acc.clear();
            let first = rest.trim();
            if !first.is_empty() {
                acc.push(first.to_string());
            }
            flush_tool_call_arg_buffer(&mut acc, &mut args);
        } else if in_args {
            acc.push(line.to_string());
            flush_tool_call_arg_buffer(&mut acc, &mut args);
        }
    }
    if in_args {
        flush_tool_call_arg_buffer(&mut acc, &mut args);
    }

    name.map(|n| ParsedToolCall { name: n, args })
}

/// Parse tool calls from LLM output text.
/// Looks for: <tool_call>\nname: X\nargs: {...}\n</tool_call>
///
/// `args` may span multiple lines (JSON object or array) until it parses.
///
/// If none are found, falls back to common **wrong** formats models emit (`<web_fetch>`, `<job_status>`).
pub fn parse_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let standard = parse_standard_tool_calls(text);
    if !standard.is_empty() {
        return standard;
    }
    parse_loose_tool_markup(text)
}

fn parse_standard_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<tool_call>") {
        let after_tag = &remaining[start + 11..];
        let Some(end) = after_tag.find("</tool_call>") else {
            break;
        };

        let block = after_tag[..end].trim();
        remaining = &after_tag[end + 12..];

        if let Some(call) = parse_tool_call_block(block) {
            calls.push(call);
        }
    }

    calls
}

fn re_web_fetch_block() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<web_fetch>\s*(.*?)</web_fetch>").expect("regex"))
}

fn re_url_in_loose_inner() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?i)url\s*:\s*"([^"]+)"|url\s*:\s*'([^']+)'|(https?://[^\s"'<>]+)"#)
            .expect("regex")
    })
}

fn re_job_status_open() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?is)<job_status\s+[^>]*?job_id\s*=\s*"([^"]+)"[^>]*>"#).expect("regex")
    })
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    for i in 0..=h.len().saturating_sub(n.len()) {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) {
            return Some(i);
        }
    }
    None
}

fn re_loose_json_tool_open() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?is)<json\s+([^>]+)>"#).expect("regex"))
}

/// `<json action="parse" input={...}>` / `input="..."` — models often emit this instead of `<tool_call>`.
fn parse_loose_json_tool_tags(text: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();
    let open = re_loose_json_tool_open();
    for cap in open.captures_iter(text) {
        let Some(attrs) = cap.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let Some(args) = json_tag_attrs_to_tool_args(attrs) else {
            continue;
        };
        calls.push(ParsedToolCall {
            name: "json".to_string(),
            args,
        });
    }
    calls
}

fn json_tag_attrs_to_tool_args(attr_src: &str) -> Option<serde_json::Value> {
    let action_re = Regex::new(r#"(?i)action\s*=\s*"([^"]*)""#).ok()?;
    let action = action_re.captures(attr_src)?.get(1)?.as_str().to_string();
    if !matches!(action.as_str(), "parse" | "format" | "query" | "validate") {
        return None;
    }
    let pos = find_case_insensitive(attr_src, "input=")? + "input=".len();
    let b = attr_src.as_bytes();
    let mut i = pos;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= b.len() {
        return None;
    }
    let input_val = if b[i] == b'"' {
        i += 1;
        let start = i;
        while i < b.len() {
            if b[i] == b'\\' {
                i = i.saturating_add(2);
                continue;
            }
            if b[i] == b'"' {
                break;
            }
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        let s = std::str::from_utf8(&b[start..i]).ok()?.to_string();
        serde_json::Value::String(s)
    } else if b[i] == b'{' {
        let start = i;
        let mut depth = 0u32;
        while i < b.len() {
            match b[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            return None;
        }
        let slice = std::str::from_utf8(&b[start..i]).ok()?;
        serde_json::from_str(slice).ok()?
    } else {
        return None;
    };

    let mut out = serde_json::json!({
        "action": action,
        "input": input_val,
    });
    if action == "query" {
        if let Ok(qre) = Regex::new(r#"(?i)query\s*=\s*"([^"]*)""#) {
            if let Some(c) = qre.captures(attr_src) {
                if let Some(q) = c.get(1) {
                    let q = q.as_str();
                    if !q.is_empty() {
                        out["query"] = serde_json::Value::String(q.to_string());
                    }
                }
            }
        }
    }
    Some(out)
}

fn re_inline_tool_call() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Matches: `name: tool_name\nargs: {...}` or `name: tool_name args: {...}` (single line)
        Regex::new(r#"(?m)^(?:\s*name:\s*(\w+)\s*\n\s*args:\s*(\{[^\n]*\}))|(?:\s*name:\s*(\w+)\s+args:\s*(\{[^\n]*\}))"#)
            .expect("regex")
    })
}

/// Matches bare `tool_name args: {json}` without the `name:` prefix.
/// Small models often drop the `name:` prefix entirely and may emit the call
/// inline without a preceding newline (e.g. `…recommendations.memory_search args: {…}`).
fn re_bare_tool_call() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Tool names must contain an underscore or colon (MCP) to reduce false positives.
        // No start-of-line anchor: models often emit tool calls inline after punctuation.
        Regex::new(r#"([\w]+(?:[_:][\w]+)+)\s+args:\s*(\{[^\n]*\})"#).expect("regex")
    })
}

/// Parse inline `name: X args: {...}` lines that small models emit without `<tool_call>` wrappers.
fn parse_inline_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();
    for cap in re_inline_tool_call().captures_iter(text) {
        let name = cap.get(1).or_else(|| cap.get(3));
        let args_str = cap.get(2).or_else(|| cap.get(4));
        if let (Some(n), Some(a)) = (name, args_str) {
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(a.as_str()) {
                calls.push(ParsedToolCall {
                    name: n.as_str().to_string(),
                    args,
                });
            }
        }
    }
    // Fall back to bare `tool_name args: {json}` format (no `name:` prefix).
    if calls.is_empty() {
        for cap in re_bare_tool_call().captures_iter(text) {
            if let (Some(n), Some(a)) = (cap.get(1), cap.get(2)) {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(a.as_str()) {
                    calls.push(ParsedToolCall {
                        name: n.as_str().to_string(),
                        args,
                    });
                }
            }
        }
    }
    calls
}

/// Models often emit pseudo-XML instead of `<tool_call>` blocks; map a few builtins to real tool calls.
fn parse_loose_tool_markup(text: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    // Try inline `name: X args: {...}` format first (common from small local models).
    calls.extend(parse_inline_tool_calls(text));
    if !calls.is_empty() {
        return calls;
    }

    calls.extend(parse_loose_json_tool_tags(text));

    for cap in re_web_fetch_block().captures_iter(text) {
        let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        if let Some(url) = extract_url_from_loose_fetch_inner(inner) {
            calls.push(ParsedToolCall {
                name: "web_fetch".to_string(),
                args: serde_json::json!({ "url": url }),
            });
        }
    }

    for cap in re_job_status_open().captures_iter(text) {
        if let Some(jid) = cap.get(1) {
            calls.push(ParsedToolCall {
                name: "job_status".to_string(),
                args: serde_json::json!({ "job_id": jid.as_str() }),
            });
        }
    }

    calls
}

fn extract_url_from_loose_fetch_inner(inner: &str) -> Option<String> {
    let caps = re_url_in_loose_inner().captures(inner)?;
    if let Some(m) = caps.get(1) {
        return Some(m.as_str().to_string());
    }
    if let Some(m) = caps.get(2) {
        return Some(m.as_str().to_string());
    }
    caps.get(3).map(|m| m.as_str().to_string())
}

fn re_strip_loose_fetch() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?is)<web_fetch>\s*.*?</web_fetch>").expect("regex"))
}

fn re_strip_loose_job_status() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?is)<job_status\s[^>]*(?:/>|>[\s\S]*?</job_status>)"#).expect("regex")
    })
}

fn re_strip_loose_json_open() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?is)<json\s[^>]+>"#).expect("regex"))
}

fn re_strip_loose_pseudo_tools() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // rust-regex has no backrefs; list open + close for each pseudo-tag.
        Regex::new(
            r"(?is)(?:<file_list\b[^>]*(?:/>|>[\s\S]*?</file_list\s*>)|<wallet_balance\b[^>]*(?:/>|>[\s\S]*?</wallet_balance\s*>))",
        )
        .expect("regex")
    })
}

/// Extract the final answer from LLM text (remove any tool_call blocks).
pub fn extract_answer(text: &str) -> String {
    let mut result = text.to_string();

    // Remove all tool_call blocks
    while let Some(start) = result.find("<tool_call>") {
        if let Some(end) = result[start..].find("</tool_call>") {
            result = format!("{}{}", &result[..start], &result[start + end + 12..]);
        } else {
            break;
        }
    }

    result = re_strip_loose_fetch().replace_all(&result, "").to_string();
    result = re_strip_loose_job_status()
        .replace_all(&result, "")
        .to_string();
    result = re_strip_loose_json_open()
        .replace_all(&result, "")
        .to_string();
    result = re_strip_loose_pseudo_tools()
        .replace_all(&result, "")
        .to_string();
    // Strip inline `name: X args: {...}` and bare `tool_name args: {...}` lines.
    result = re_inline_tool_call().replace_all(&result, "").to_string();
    result = re_bare_tool_call().replace_all(&result, "").to_string();

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_calls() {
        let text = r#"I'll search for that information.

<tool_call>
name: web_search
args: {"query": "rust async patterns"}
</tool_call>

Let me also check the docs.

<tool_call>
name: web_fetch
args: {"url": "https://doc.rust-lang.org"}
</tool_call>"#;

        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(calls[1].name, "web_fetch");
    }

    #[test]
    fn test_parse_no_tool_calls() {
        let text = "Here is the answer to your question. No tools needed.";
        let calls = parse_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_multiline_args() {
        let text = r#"<tool_call>
name: json
args: {
  "action": "parse",
  "input": "{\"a\":1}"
}
</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "json");
        assert_eq!(calls[0].args["action"], "parse");
        assert_eq!(calls[0].args["input"], "{\"a\":1}");
    }

    #[test]
    fn test_extract_answer() {
        let text = r#"Here is my answer.

<tool_call>
name: web_search
args: {"query": "test"}
</tool_call>

The final answer is 42."#;

        let answer = extract_answer(text);
        assert!(answer.contains("Here is my answer"));
        assert!(answer.contains("The final answer is 42"));
        assert!(!answer.contains("tool_call"));
    }

    #[test]
    fn test_parse_loose_web_fetch_and_job_status() {
        let text = r#"<web_fetch> url: "https://example.com/path" </web_fetch>

<job_status job_id="job_12345"></job_status>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_fetch");
        assert_eq!(calls[0].args["url"], "https://example.com/path");
        assert_eq!(calls[1].name, "job_status");
        assert_eq!(calls[1].args["job_id"], "job_12345");
    }

    #[test]
    fn test_standard_tool_calls_take_precedence_over_loose() {
        let text = r#"<tool_call>
name: web_fetch
args: {"url": "https://a.example"}
</tool_call>
<web_fetch> url: "https://b.example" </web_fetch>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args["url"], "https://a.example");
    }

    #[test]
    fn test_extract_answer_strips_loose_tags() {
        let text = r#"Summary here.

<web_fetch> url: "https://x.test" </web_fetch>"#;
        let answer = extract_answer(text);
        assert!(answer.contains("Summary"));
        assert!(!answer.contains("web_fetch"));
    }

    #[test]
    fn test_parse_loose_json_tool_tag() {
        let text = r#"Planning.

<json action="parse" input={"travel": "weekend", "budget": 500}>

More text."#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "json");
        assert_eq!(calls[0].args["action"], "parse");
        assert!(calls[0].args.get("input").is_some());
    }

    #[test]
    fn test_extract_answer_strips_loose_json_open_tag() {
        let text = "Answer.\n\n<json action=\"parse\" input={}>\n";
        let answer = extract_answer(text);
        assert!(answer.contains("Answer"));
        assert!(!answer.contains("<json"));
    }

    #[test]
    fn test_extract_answer_strips_wallet_balance_pseudo_tag() {
        let text = "Your balance:\n<wallet_balance>100 PCLAW</wallet_balance>\nDone.";
        let answer = extract_answer(text);
        assert!(answer.contains("Your balance:"));
        assert!(answer.contains("Done."));
        assert!(!answer.contains("wallet_balance"));
    }

    #[test]
    fn test_extract_answer_strips_file_list_self_closing() {
        let text = "Files:\n<file_list path=\"/tmp\" />\nEnd.";
        let answer = extract_answer(text);
        assert!(!answer.contains("file_list"));
        assert!(answer.contains("End."));
    }

    #[test]
    fn test_parse_inline_tool_calls() {
        let text = "I'll search for that.\n\nname: web_fetch\nargs: {\"url\": \"https://example.com\"}\n\nAnd also:\n\nname: json\nargs: {\"action\": \"parse\", \"input\": \"hello\"}";
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_fetch");
        assert_eq!(calls[0].args["url"], "https://example.com");
        assert_eq!(calls[1].name, "json");
    }

    #[test]
    fn test_parse_inline_tool_call_single_line() {
        let text = "Let me check.\nname: web_fetch args: {\"url\": \"https://test.org\"}";
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_fetch");
    }

    #[test]
    fn test_extract_answer_strips_inline_tool_calls() {
        let text =
            "Here is info.\n\nname: web_fetch\nargs: {\"url\": \"https://x.com\"}\n\nFinal answer.";
        let answer = extract_answer(text);
        assert!(answer.contains("Here is info."));
        assert!(answer.contains("Final answer."));
        assert!(!answer.contains("web_fetch"));
    }

    #[test]
    fn test_parse_bare_tool_call() {
        let text = "I'll search for that.\n\nmemory_search args: {\"query\": \"cat food\"}\n";
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_search");
        assert_eq!(calls[0].args["query"], "cat food");
    }

    #[test]
    fn test_parse_bare_tool_call_multiple() {
        let text = "Let me fetch multiple sources.\nweb_fetch args: {\"url\": \"https://a.example\"}\nweb_fetch args: {\"url\": \"https://b.example\"}";
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_fetch");
        assert_eq!(calls[1].args["url"], "https://b.example");
    }

    #[test]
    fn test_parse_bare_tool_call_mcp_colon() {
        let text = "server:tool_name args: {\"key\": \"val\"}";
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "server:tool_name");
    }

    #[test]
    fn test_extract_answer_strips_bare_tool_calls() {
        let text = "Here is info.\n\nmemory_search args: {\"query\": \"test\"}\n\nFinal answer.";
        let answer = extract_answer(text);
        assert!(answer.contains("Here is info."));
        assert!(answer.contains("Final answer."));
        assert!(!answer.contains("memory_search"));
    }

    #[test]
    fn test_bare_tool_call_no_false_positive_single_word() {
        // Single words without underscores or colons should NOT match
        let text = "Hello args: {\"key\": \"value\"}";
        let calls = parse_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_bare_tool_call_inline_no_newline() {
        // Model emits tool call directly after text without newline
        let text = "Let me gather some current information.memory_search args: {\"query\": \"Bari Italy travel\"}";
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_search");
        assert_eq!(calls[0].args["query"], "Bari Italy travel");
    }

    #[test]
    fn test_extract_answer_strips_inline_bare_no_newline() {
        let text = "I'll help you plan.memory_search args: {\"query\": \"test\"}\nMore text.";
        let answer = extract_answer(text);
        assert!(answer.contains("I'll help you plan."));
        assert!(!answer.contains("memory_search"));
    }
}
