//! Sandboxed code execution tool (Code Interpreter).
//!
//! Runs Python, JavaScript (Node), or Bash code in an isolated environment:
//! - macOS: `sandbox-exec` denies network access and restricts filesystem
//! - All platforms: temp directory isolation, timeout, output capture
//! - Resource limits: 30s default timeout, 64KB output cap

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::tools::tool::{
    optional_i64, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain,
    ToolError, ToolOutput,
};

/// Maximum output size before truncation.
const MAX_OUTPUT: usize = 64 * 1024;

/// Default timeout for code execution.
const DEFAULT_TIMEOUT_SECS: i64 = 30;

/// macOS sandbox profile: deny network, deny reads on user home dirs, restrict writes to tmp.
const MACOS_SANDBOX_PROFILE: &str = r#"
(version 1)
(allow default)
;; Block all network access
(deny network*)
;; Block reading user home directories and sensitive paths
(deny file-read* (subpath "/Users") (subpath "/home"))
(deny file-read-data (subpath "/Users") (subpath "/home"))
;; Block writes everywhere except temp dirs
(deny file-write*
    (require-not (subpath "/private/tmp"))
    (require-not (subpath "/tmp"))
    (require-not (subpath "/var/folders"))
    (require-not (subpath "/dev"))
)
"#;

/// Supported languages and their interpreters.
fn interpreter_for(lang: &str) -> Option<(&'static str, &'static str, &'static str)> {
    // Returns (command, flag, file_extension)
    match lang.to_lowercase().as_str() {
        "python" | "python3" | "py" => Some(("python3", "-u", "py")),
        "javascript" | "js" | "node" => Some(("node", "--", "mjs")),
        "bash" | "sh" | "shell" => Some(("bash", "--", "sh")),
        "ruby" | "rb" => Some(("ruby", "--", "rb")),
        _ => None,
    }
}

/// Check if an interpreter is available on the system.
async fn check_interpreter(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if macOS sandbox-exec is available.
fn has_sandbox_exec() -> bool {
    cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/sandbox-exec").exists()
}

pub struct CodeExecTool;

#[async_trait]
impl Tool for CodeExecTool {
    fn name(&self) -> &str {
        "code_exec"
    }

    fn description(&self) -> &str {
        "Execute code in a sandboxed environment. Supports Python, JavaScript (Node), Bash, Ruby. \
         Code runs in an isolated temp directory with no network access (macOS sandbox). \
         Use for calculations, data processing, file generation, or testing logic."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "description": "Programming language: python, javascript, bash, ruby",
                    "enum": ["python", "javascript", "bash", "ruby"]
                },
                "code": {
                    "type": "string",
                    "description": "Source code to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 30, max: 120)"
                }
            },
            "required": ["language", "code"]
        })
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Always
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let lang = require_str(&params, "language")?;
        let code = require_str(&params, "code")?;
        let timeout_secs = optional_i64(&params, "timeout", DEFAULT_TIMEOUT_SECS)
            .min(120)
            .max(1) as u64;

        // Resolve interpreter
        let (interpreter, _flag, ext) = interpreter_for(lang).ok_or_else(|| {
            ToolError::InvalidParameters(format!(
                "Unsupported language '{}'. Use: python, javascript, bash, ruby",
                lang
            ))
        })?;

        // Check interpreter availability
        if !check_interpreter(interpreter).await {
            return Err(ToolError::ExecutionFailed(format!(
                "'{}' not found on this system. Install it to use language '{}'.",
                interpreter, lang
            )));
        }

        // Create isolated temp directory
        let tmp_path = std::env::temp_dir().join(format!(
            "peerclaw_code_{}",
            uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("x")
        ));
        tokio::fs::create_dir_all(&tmp_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create temp dir: {e}")))?;

        let code_file = tmp_path.join(format!("script.{ext}"));
        tokio::fs::write(&code_file, code)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write code: {e}")))?;

        // Build command — use sandbox-exec on macOS if available
        let mut cmd = if has_sandbox_exec() {
            let mut c = Command::new("sandbox-exec");
            c.arg("-p").arg(MACOS_SANDBOX_PROFILE);
            c.arg(interpreter);
            c
        } else {
            Command::new(interpreter)
        };

        cmd.arg(&code_file);
        cmd.current_dir(&tmp_path);

        // Minimal environment
        cmd.env_clear();
        cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin");
        cmd.env("HOME", &tmp_path);
        cmd.env("TMPDIR", &tmp_path);
        cmd.env("LANG", "en_US.UTF-8");
        // Python: unbuffered output, no user site-packages
        if lang.starts_with("python") || lang == "py" {
            cmd.env("PYTHONDONTWRITEBYTECODE", "1");
            cmd.env("PYTHONNOUSERSITE", "1");
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Spawn and wait with timeout
        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn {interpreter}: {e}")))?;

        let result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            child.wait_with_output(),
        )
        .await;

        let elapsed = start.elapsed();
        let cleanup = || { let p = tmp_path.clone(); tokio::spawn(async move { let _ = tokio::fs::remove_dir_all(&p).await; }); };

        match result {
            Ok(Ok(output)) => {
                let exit_code = output.status.code().unwrap_or(-1);
                let stdout_raw = &output.stdout[..output.stdout.len().min(MAX_OUTPUT)];
                let stderr_raw = &output.stderr[..output.stderr.len().min(MAX_OUTPUT)];
                let stdout = String::from_utf8_lossy(stdout_raw);
                let stderr = String::from_utf8_lossy(stderr_raw);

                let success = exit_code == 0;
                let data = serde_json::json!({
                    "exit_code": exit_code,
                    "stdout": stdout.as_ref(),
                    "stderr": stderr.as_ref(),
                    "language": lang,
                    "duration_ms": elapsed.as_millis() as u64,
                    "sandboxed": has_sandbox_exec(),
                });

                cleanup();
                if success {
                    Ok(ToolOutput::success(data, elapsed))
                } else {
                    let mut out = ToolOutput::failure(
                        format!("Exit code {exit_code}: {}", stderr.trim()),
                        elapsed,
                    );
                    out.data = data;
                    Ok(out)
                }
            }
            Ok(Err(e)) => {
                cleanup();
                Err(ToolError::ExecutionFailed(format!("Process error: {e}")))
            }
            Err(_) => {
                cleanup();
                Err(ToolError::ExecutionFailed(format!(
                    "Code execution timed out after {timeout_secs}s"
                )))
            }
        }
    }
}

/// Returns info about available interpreters on this system.
pub async fn available_interpreters() -> Vec<(&'static str, &'static str, bool)> {
    let langs = [
        ("python", "python3"),
        ("javascript", "node"),
        ("bash", "bash"),
        ("ruby", "ruby"),
    ];
    let mut result = Vec::new();
    for (name, cmd) in langs {
        let available = check_interpreter(cmd).await;
        result.push((name, cmd, available));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpreter_lookup() {
        assert!(interpreter_for("python").is_some());
        assert!(interpreter_for("py").is_some());
        assert!(interpreter_for("javascript").is_some());
        assert!(interpreter_for("js").is_some());
        assert!(interpreter_for("bash").is_some());
        assert!(interpreter_for("ruby").is_some());
        assert!(interpreter_for("rust").is_none());
        assert!(interpreter_for("c++").is_none());
    }

    #[test]
    fn test_sandbox_profile() {
        // Just ensure the profile string is valid (non-empty)
        assert!(MACOS_SANDBOX_PROFILE.contains("deny network"));
    }
}
