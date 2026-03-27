//! LLM task delegation tool.
//!
//! Allows an agent to delegate a sub-task (summarization, translation, analysis, etc.)
//! to the LLM without consuming its own context window.

use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::oneshot;

use crate::tools::node_tool::NodeToolCommand;
use crate::tools::tool::{
    optional_str, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain, ToolError,
    ToolOutput,
};

/// Tool that delegates a sub-task prompt to the LLM via the node's inference engine.
pub struct LlmTaskTool;

#[async_trait]
impl Tool for LlmTaskTool {
    fn name(&self) -> &str {
        "llm_task"
    }

    fn description(&self) -> &str {
        "Delegate a sub-task to the LLM (e.g. summarization, translation, analysis). \
         Runs a separate inference call without consuming the current conversation context."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task description/prompt to send to the LLM"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use (default: current model)"
                },
                "max_tokens": {
                    "type": "integer",
                    "description": "Maximum tokens to generate (default: 1024)"
                },
                "temperature": {
                    "type": "number",
                    "description": "Sampling temperature 0.0-2.0 (default: 0.7)"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "Override the system prompt for this sub-task"
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let task = require_str(&params, "task")?;
        let model = optional_str(&params, "model").map(|s| s.to_string());
        let max_tokens = params
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(1024) as u32;
        let temperature = params
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.7) as f32;
        let system_prompt = optional_str(&params, "system_prompt").map(|s| s.to_string());

        let tx = ctx
            .node_tool_tx
            .as_ref()
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "llm_task requires a running node (use `peerclaw serve --agent`)".to_string(),
                )
            })?;

        let (reply, rx) = oneshot::channel();
        tx.send(NodeToolCommand::InferenceRequest {
            prompt: task.to_string(),
            model,
            max_tokens,
            temperature,
            system_prompt,
            reply,
        })
        .await
        .map_err(|_| ToolError::ExecutionFailed("node tool channel closed".to_string()))?;

        let result = rx
            .await
            .map_err(|_| ToolError::ExecutionFailed("node dropped inference reply".to_string()))?
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        Ok(ToolOutput::success(
            serde_json::json!({
                "text": result,
            }),
            start.elapsed(),
        ))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn rate_limit(&self) -> Option<u32> {
        Some(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_task_tool_metadata() {
        let tool = LlmTaskTool;
        assert_eq!(tool.name(), "llm_task");
        assert_eq!(tool.approval_requirement(), ApprovalRequirement::Never);
        assert_eq!(tool.rate_limit(), Some(10));
    }

    #[tokio::test]
    async fn test_llm_task_requires_node_tx() {
        let tool = LlmTaskTool;
        let ctx = ToolContext::local("test".to_string());
        let result = tool
            .execute(serde_json::json!({"task": "summarize this"}), &ctx)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("running node"));
    }

    #[test]
    fn test_llm_task_schema_has_required() {
        let tool = LlmTaskTool;
        let schema = tool.parameters_schema();
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "task");
    }
}
