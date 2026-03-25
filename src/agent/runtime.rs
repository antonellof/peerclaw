//! Agent runtime - the core agentic execution loop.
//!
//! Implements a ReAct-style loop: LLM generates thoughts and tool calls,
//! tools are executed, results fed back, until a final answer is produced.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::executor::task::{ChatMessage, ExecutionTask, InferenceTask, MessageRole, TaskData};
use crate::executor::TaskExecutor;
use crate::tools::{NodeToolTx, ToolContext, ToolRegistry};

use super::budget::BudgetTracker;
use super::spec::AgentSpec;

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
}

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

/// Maximum number of iterations before stopping.
const MAX_ITERATIONS: u32 = 10;

/// Estimated cost per 1k tokens (in PCLAW) for budget tracking.
const COST_PER_1K_TOKENS: f64 = 0.001;

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
    ) -> Self {
        let tool_context = ToolContext {
            session_id: config.id.clone(),
            job_id: None,
            peer_id,
            working_dir: std::env::current_dir().unwrap_or_default(),
            sandboxed: false,
            available_secrets: vec![],
            node_tool_tx,
        };

        Self {
            config,
            executor,
            tools,
            conversation: Vec::new(),
            budget,
            tool_context,
            task_log: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create from an AgentSpec.
    pub fn from_spec(
        spec: &AgentSpec,
        executor: Arc<TaskExecutor>,
        tools: Arc<ToolRegistry>,
        peer_id: String,
        node_tool_tx: Option<NodeToolTx>,
    ) -> Self {
        let config = AgentConfig::from_spec(spec);
        let budget = BudgetTracker::new(
            spec.budget.per_request,
            spec.budget.per_hour,
            spec.budget.per_day,
            spec.budget.total,
        );
        Self::new(config, executor, tools, budget, peer_id, node_tool_tx)
    }

    /// Run a task through the agentic loop.
    ///
    /// When `stop` is set, it is polled at the start of each iteration (after the current LLM or
    /// tool work finishes).
    pub async fn run_task(
        &mut self,
        user_input: &str,
        stop: Option<&AtomicBool>,
    ) -> AgentResult {
        self.budget.new_request();
        let mut tool_calls = Vec::new();
        let mut iterations = 0u32;
        let mut total_tokens = 0u32;

        self.log(&format!("Task received: {}", user_input)).await;

        // Build system prompt with tool descriptions
        let system = self.build_system_prompt().await;

        // Initialize conversation for this task
        self.conversation.clear();
        self.conversation.push(ChatMessage::system(&system));
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
            let parsed_calls = parse_tool_calls(&response_text);

            if parsed_calls.is_empty() {
                // No tool calls → this is the final answer
                self.log("Final answer produced").await;
                return AgentResult {
                    answer: extract_answer(&response_text),
                    tool_calls,
                    iterations,
                    total_tokens,
                    budget_spent: self.budget.total_spent(),
                    success: true,
                    error: None,
                };
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

    /// Build the system prompt with tool descriptions.
    async fn build_system_prompt(&self) -> String {
        let mut prompt = self.config.system_prompt.clone();

        if prompt.is_empty() {
            prompt = format!(
                "You are {}, a helpful AI assistant. You solve tasks step by step using available tools.\n",
                self.config.name
            );
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
            prompt.push_str("\n\nYou have access to the following tools:\n\n");
            for tool in &available {
                prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
            }

            prompt.push_str("\nTo use a tool, include a tool_call block in your response:\n\n");
            prompt.push_str(
                "<tool_call>\nname: tool_name\nargs: {\"param\": \"value\"}\n</tool_call>\n\n",
            );
            prompt.push_str("You can make multiple tool calls in one response. After tool results are returned, continue reasoning and make more tool calls if needed. When you have the final answer, respond without any tool_call blocks.\n");
        }

        prompt
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
pub fn parse_tool_calls(text: &str) -> Vec<ParsedToolCall> {
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
}
