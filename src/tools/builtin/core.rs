//! Core utility tools: echo, time, json.

use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Local, Utc};

use crate::tools::tool::{
    optional_str, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain, ToolError,
    ToolOutput,
};

/// Echo tool - returns the input message.
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo back the input message. Useful for testing and debugging."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to echo back"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();
        let message = require_str(&params, "message")?;
        Ok(ToolOutput::text(message, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Time tool - returns current time in various formats.
pub struct TimeTool;

#[async_trait]
impl Tool for TimeTool {
    fn name(&self) -> &str {
        "time"
    }

    fn description(&self) -> &str {
        "Get the current date and time. Returns ISO-8601, Unix timestamp, and human-readable formats."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timezone": {
                    "type": "string",
                    "description": "Timezone (utc or local, default: local)",
                    "enum": ["utc", "local"]
                },
                "format": {
                    "type": "string",
                    "description": "Output format (iso, unix, human, all). Default: all"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let tz = optional_str(&params, "timezone").unwrap_or("local");
        let format = optional_str(&params, "format").unwrap_or("all");

        let (utc_now, local_now): (DateTime<Utc>, DateTime<Local>) = (Utc::now(), Local::now());

        let result = match format {
            "iso" => {
                if tz == "utc" {
                    serde_json::json!({ "iso": utc_now.to_rfc3339() })
                } else {
                    serde_json::json!({ "iso": local_now.to_rfc3339() })
                }
            }
            "unix" => {
                serde_json::json!({ "unix": utc_now.timestamp() })
            }
            "human" => {
                if tz == "utc" {
                    serde_json::json!({ "human": utc_now.format("%Y-%m-%d %H:%M:%S UTC").to_string() })
                } else {
                    serde_json::json!({ "human": local_now.format("%Y-%m-%d %H:%M:%S %Z").to_string() })
                }
            }
            _ => {
                serde_json::json!({
                    "iso": local_now.to_rfc3339(),
                    "unix": utc_now.timestamp(),
                    "utc": utc_now.to_rfc3339(),
                    "local": local_now.to_rfc3339(),
                    "human": local_now.format("%A, %B %d, %Y at %H:%M:%S").to_string(),
                    "timezone": local_now.format("%Z").to_string(),
                })
            }
        };

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// JSON tool - parse, format, and manipulate JSON.
pub struct JsonTool;

#[async_trait]
impl Tool for JsonTool {
    fn name(&self) -> &str {
        "json"
    }

    fn description(&self) -> &str {
        "Parse, format, query, and manipulate JSON data. Supports JSONPath queries."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: parse, format, query, validate",
                    "enum": ["parse", "format", "query", "validate"]
                },
                "input": {
                    "type": "string",
                    "description": "JSON to process: a JSON string, or an object/array (accepted as structured input). Markdown ```json … ``` fences are stripped automatically."
                },
                "query": {
                    "type": "string",
                    "description": "JSONPath query (for query action)"
                },
                "indent": {
                    "type": "integer",
                    "description": "Indentation spaces for formatting (default: 2)"
                }
            },
            "required": ["action", "input"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        // Accept "action" or default to "format" when missing (small models often omit it)
        let action = match params.get("action").and_then(|v| v.as_str()) {
            Some(a) if !a.is_empty() => a.to_string(),
            _ => "format".to_string(),
        };
        let input_raw = json_tool_input_raw(&params)?;

        match action.as_str() {
            "parse" | "validate" => {
                let prepared = prepare_json_text(&input_raw);
                match serde_json::from_str::<serde_json::Value>(&prepared) {
                    Ok(parsed) => {
                        if action == "validate" {
                            Ok(ToolOutput::success(
                                serde_json::json!({ "valid": true, "type": value_type(&parsed) }),
                                start.elapsed(),
                            ))
                        } else {
                            Ok(ToolOutput::success(parsed, start.elapsed()))
                        }
                    }
                    Err(e) => {
                        if action == "validate" {
                            Ok(ToolOutput::success(
                                serde_json::json!({ "valid": false, "error": e.to_string() }),
                                start.elapsed(),
                            ))
                        } else {
                            Err(invalid_json_tool_error(e, &prepared))
                        }
                    }
                }
            }
            "format" => {
                let prepared = prepare_json_text(&input_raw);
                let parsed: serde_json::Value = serde_json::from_str(&prepared)
                    .map_err(|e| invalid_json_tool_error(e, &prepared))?;

                let indent = params.get("indent").and_then(|v| v.as_u64()).unwrap_or(2) as usize;

                let formatted = if indent == 0 {
                    serde_json::to_string(&parsed)
                } else {
                    serde_json::to_string_pretty(&parsed)
                }
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                Ok(ToolOutput::text(formatted, start.elapsed()))
            }
            "query" => {
                let query = optional_str(&params, "query").ok_or_else(|| {
                    ToolError::InvalidParameters("query required for query action".to_string())
                })?;

                let prepared = prepare_json_text(&input_raw);
                let parsed: serde_json::Value = serde_json::from_str(&prepared)
                    .map_err(|e| invalid_json_tool_error(e, &prepared))?;

                // Simple dot-notation query (e.g., "user.name" or "items[0].id")
                let result = json_query(&parsed, query)?;
                Ok(ToolOutput::success(result, start.elapsed()))
            }
            _ => Err(ToolError::InvalidParameters(format!(
                "Unknown action: {}",
                action
            ))),
        }
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Models often pass `input` as a JSON object instead of a quoted string — accept both.
/// Also accepts `json` as alias for `input` (small models often use this).
fn json_tool_input_raw(params: &serde_json::Value) -> Result<String, ToolError> {
    // Try "input" first, then "json" as alias
    let val = params.get("input").or_else(|| params.get("json"));
    match val {
        Some(serde_json::Value::String(s)) => Ok(s.clone()),
        Some(v) => Ok(v.to_string()),
        None => Err(ToolError::InvalidParameters(
            format!(
                "missing or empty required parameter `action` for `json`. \
                 Required: `action` (one of parse|format|query|validate), \
                 `input` (JSON to process: a JSON string, or an object/array \
                 (accepted as structured input). \
                 Markdown ```json … ``` fences are stripped automatically.)"
            ),
        )),
    }
}

/// Strip BOM, trim, and unwrap common ``` / ```json markdown fences from LLM output.
fn prepare_json_text(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.starts_with('\u{feff}') {
        s.remove(0);
    }
    let t = s.trim();
    if !t.starts_with("```") {
        return t.to_string();
    }
    let after_open = &t[3..];
    let body = if let Some(nl) = after_open.find('\n') {
        &after_open[nl + 1..]
    } else {
        after_open
    };
    let body = if let Some(end) = body.rfind("```") {
        body[..end].trim()
    } else {
        body.trim()
    };
    body.to_string()
}

fn json_input_preview(prepared: &str, max: usize) -> String {
    let collapsed: String = prepared
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t = collapsed.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(max).collect::<String>())
    }
}

fn invalid_json_tool_error(e: serde_json::Error, prepared: &str) -> ToolError {
    let preview = json_input_preview(prepared, 140);
    ToolError::ExecutionFailed(format!(
        "Invalid JSON: {}. Normalized input starts: {:?} (pass raw JSON or a ```json``` block; not HTML or plain prose)",
        e, preview
    ))
}

fn value_type(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn json_query(value: &serde_json::Value, query: &str) -> Result<serde_json::Value, ToolError> {
    let mut current = value.clone();

    for part in query.split('.') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Check for array index: items[0]
        if let Some(bracket_pos) = part.find('[') {
            let key = &part[..bracket_pos];
            let index_str = &part[bracket_pos + 1..part.len() - 1];

            if !key.is_empty() {
                current = current
                    .get(key)
                    .cloned()
                    .ok_or_else(|| ToolError::ExecutionFailed(format!("Key not found: {}", key)))?;
            }

            let index: usize = index_str.parse().map_err(|_| {
                ToolError::ExecutionFailed(format!("Invalid array index: {}", index_str))
            })?;

            current = current.get(index).cloned().ok_or_else(|| {
                ToolError::ExecutionFailed(format!("Index out of bounds: {}", index))
            })?;
        } else {
            current = current
                .get(part)
                .cloned()
                .ok_or_else(|| ToolError::ExecutionFailed(format!("Key not found: {}", part)))?;
        }
    }

    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo() {
        let tool = EchoTool;
        let ctx = ToolContext::local("test".to_string());
        let result = tool
            .execute(serde_json::json!({"message": "hello"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.message, Some("hello".to_string()));
    }

    #[tokio::test]
    async fn test_time() {
        let tool = TimeTool;
        let ctx = ToolContext::local("test".to_string());
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert!(result.success);
        assert!(result.data.get("unix").is_some());
    }

    #[tokio::test]
    async fn test_json_parse() {
        let tool = JsonTool;
        let ctx = ToolContext::local("test".to_string());
        let result = tool
            .execute(
                serde_json::json!({"action": "parse", "input": r#"{"name": "test"}"#}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data["name"], "test");
    }

    #[tokio::test]
    async fn test_json_query() {
        let tool = JsonTool;
        let ctx = ToolContext::local("test".to_string());
        let result = tool
            .execute(
                serde_json::json!({
                    "action": "query",
                    "input": r#"{"user": {"name": "alice", "age": 30}}"#,
                    "query": "user.name"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data, "alice");
    }

    #[tokio::test]
    async fn test_json_parse_markdown_fence() {
        let tool = JsonTool;
        let ctx = ToolContext::local("test".to_string());
        let fenced = "```json\n{\"x\": 1}\n```";
        let result = tool
            .execute(
                serde_json::json!({"action": "parse", "input": fenced}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data["x"], 1);
    }

    #[tokio::test]
    async fn test_json_parse_object_input_not_string() {
        let tool = JsonTool;
        let ctx = ToolContext::local("test".to_string());
        let result = tool
            .execute(
                serde_json::json!({"action": "parse", "input": {"k": "v"}}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data["k"], "v");
    }
}
