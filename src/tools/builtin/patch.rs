//! Apply patch tool: targeted find-and-replace edits to files.

use std::path::PathBuf;
use std::time::Instant;

use async_trait::async_trait;
use tokio::fs;

use crate::tools::tool::{
    optional_bool, require_str, ApprovalRequirement, Tool, ToolContext, ToolDomain, ToolError,
    ToolOutput,
};

/// Protected paths that should never be modified.
const PROTECTED_PATHS: &[&str] = &[
    "/etc/passwd",
    "/etc/shadow",
    "/etc/sudoers",
    ".ssh/id_rsa",
    ".ssh/id_ed25519",
    ".env",
    ".bash_history",
    ".zsh_history",
];

/// Number of context lines to show before/after a change in the diff preview.
const DIFF_CONTEXT_LINES: usize = 3;

/// Apply patch tool for targeted find-and-replace file edits.
pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Make targeted edits to a file by finding exact text and replacing it. \
         Returns a diff preview showing what changed. Can also create new files."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find in the file"
                },
                "new_text": {
                    "type": "string",
                    "description": "Text to replace old_text with"
                },
                "create_if_missing": {
                    "type": "boolean",
                    "description": "Create the file with new_text if it does not exist (default: false)"
                }
            },
            "required": ["file_path", "old_text", "new_text"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let file_path_str = require_str(&params, "file_path")?;
        let old_text = require_str(&params, "old_text")?;
        let new_text = require_str(&params, "new_text")?;
        let create_if_missing = optional_bool(&params, "create_if_missing", false);

        // Security check: protected paths
        if is_protected_path(file_path_str) {
            return Err(ToolError::NotAuthorized(format!(
                "Access to {} is not allowed",
                file_path_str
            )));
        }

        let path = resolve_path(file_path_str, &ctx.working_dir);

        // Check if file exists
        let file_exists = fs::metadata(&path).await.is_ok();

        if !file_exists {
            if create_if_missing {
                // Create the file with new_text as content
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).await.map_err(|e| {
                        ToolError::ExecutionFailed(format!("Cannot create directory: {}", e))
                    })?;
                }

                fs::write(&path, new_text).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!("Cannot write file: {}", e))
                })?;

                let result = serde_json::json!({
                    "action": "created",
                    "path": path.display().to_string(),
                    "bytes_written": new_text.len(),
                    "diff": format!("+ (new file with {} bytes)", new_text.len()),
                });

                return Ok(ToolOutput::success(result, start.elapsed()));
            } else {
                return Err(ToolError::ExecutionFailed(format!(
                    "File not found: {}. Use create_if_missing: true to create it.",
                    path.display()
                )));
            }
        }

        // Read the file
        let content = fs::read_to_string(&path).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Cannot read file: {}", e))
        })?;

        // Find old_text in the content
        let Some(match_pos) = content.find(old_text) else {
            // Provide helpful context about what's near the expected location
            let suggestion = find_nearby_text(&content, old_text);
            return Err(ToolError::ExecutionFailed(format!(
                "old_text not found in {}. {}",
                path.display(),
                suggestion
            )));
        };

        // Replace first occurrence
        let new_content = format!(
            "{}{}{}",
            &content[..match_pos],
            new_text,
            &content[match_pos + old_text.len()..]
        );

        // Write back to file
        fs::write(&path, &new_content).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Cannot write file: {}", e))
        })?;

        // Generate a diff preview with context
        let diff = generate_diff_preview(&content, &new_content, match_pos, old_text, new_text);

        let result = serde_json::json!({
            "action": "replaced",
            "path": path.display().to_string(),
            "match_position": match_pos,
            "old_length": old_text.len(),
            "new_length": new_text.len(),
            "diff": diff,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Always // File modification always needs approval
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Resolve a path relative to the working directory.
fn resolve_path(path: &str, working_dir: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else if path.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            home.join(path.strip_prefix("~").unwrap_or(&path))
        } else {
            path
        }
    } else {
        working_dir.join(path)
    }
}

/// Check if a path matches any protected path pattern.
fn is_protected_path(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    PROTECTED_PATHS.iter().any(|p| path_lower.contains(*p))
}

/// Try to find text near where old_text might be and provide a suggestion.
fn find_nearby_text(content: &str, old_text: &str) -> String {
    // Try to find a partial match using the first line of old_text
    let first_line = old_text.lines().next().unwrap_or(old_text);
    let search_term = if first_line.len() > 40 {
        &first_line[..40]
    } else {
        first_line
    };

    if let Some(pos) = content.find(search_term.trim()) {
        // Show context around the partial match
        let start = content[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let end_search = &content[pos..];
        let end = end_search
            .find('\n')
            .map(|p| {
                // Get a couple more lines
                let after = &end_search[p + 1..];
                let next_nl = after.find('\n').unwrap_or(after.len());
                pos + p + 1 + next_nl
            })
            .unwrap_or(content.len())
            .min(content.len());

        let nearby = &content[start..end];
        format!(
            "Partial match found near byte {}. Nearby text:\n{}",
            pos, nearby
        )
    } else {
        // No partial match; show the file is non-empty at least
        let preview_len = content.len().min(200);
        let preview = &content[..preview_len];
        format!(
            "No partial match found. File has {} bytes. First {} chars:\n{}",
            content.len(),
            preview_len,
            preview
        )
    }
}

/// Generate a unified-diff-like preview of the change with context lines.
fn generate_diff_preview(
    old_content: &str,
    _new_content: &str,
    match_pos: usize,
    old_text: &str,
    new_text: &str,
) -> String {
    let old_lines: Vec<&str> = old_content.lines().collect();

    // Find the line number where the match starts
    let bytes_before = &old_content[..match_pos];
    let start_line = bytes_before.matches('\n').count();

    // Find how many lines old_text spans
    let old_line_count = old_text.matches('\n').count() + 1;

    // Calculate context boundaries
    let context_start = start_line.saturating_sub(DIFF_CONTEXT_LINES);
    let context_end = (start_line + old_line_count + DIFF_CONTEXT_LINES).min(old_lines.len());

    let mut diff = String::new();
    diff.push_str(&format!(
        "@@ -{},{} @@\n",
        context_start + 1,
        context_end - context_start
    ));

    // Lines before the change (context)
    for line in old_lines.iter().take(start_line).skip(context_start) {
        diff.push_str(&format!(" {}\n", line));
    }

    // Removed lines
    for line in old_text.lines() {
        diff.push_str(&format!("-{}\n", line));
    }

    // Added lines
    for line in new_text.lines() {
        diff.push_str(&format!("+{}\n", line));
    }

    // Lines after the change (context)
    for line in old_lines
        .iter()
        .take(context_end)
        .skip(start_line + old_line_count)
    {
        diff.push_str(&format!(" {}\n", line));
    }

    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tool::ToolContext;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_apply_patch_basic_replace() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!\nThis is a test.\nGoodbye!")
            .await
            .unwrap();

        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_text": "World",
                    "new_text": "Rust"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data["action"], "replaced");

        // Verify the file was changed
        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "Hello, Rust!\nThis is a test.\nGoodbye!");
    }

    #[tokio::test]
    async fn test_apply_patch_multiline_replace() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("multi.txt");
        fs::write(
            &file_path,
            "fn main() {\n    println!(\"old\");\n}\n",
        )
        .await
        .unwrap();

        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_text": "    println!(\"old\");",
                    "new_text": "    println!(\"new\");\n    println!(\"extra\");"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.success);

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("println!(\"new\")"));
        assert!(content.contains("println!(\"extra\")"));
    }

    #[tokio::test]
    async fn test_apply_patch_file_creation() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("new_file.txt");

        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let result = tool
            .execute(
                serde_json::json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_text": "",
                    "new_text": "Brand new content!",
                    "create_if_missing": true
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data["action"], "created");

        let content = fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "Brand new content!");
    }

    #[tokio::test]
    async fn test_apply_patch_file_not_found() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("nonexistent.txt");

        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let err = tool
            .execute(
                serde_json::json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_text": "something",
                    "new_text": "other"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("File not found"));
                assert!(msg.contains("create_if_missing"));
            }
            _ => panic!("Expected ExecutionFailed, got {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_apply_patch_old_text_not_found() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!").await.unwrap();

        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let err = tool
            .execute(
                serde_json::json!({
                    "file_path": file_path.to_str().unwrap(),
                    "old_text": "Nonexistent text",
                    "new_text": "replacement"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        match err {
            ToolError::ExecutionFailed(msg) => {
                assert!(msg.contains("old_text not found"));
            }
            _ => panic!("Expected ExecutionFailed, got {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_apply_patch_protected_path() {
        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let err = tool
            .execute(
                serde_json::json!({
                    "file_path": "/home/user/.ssh/id_rsa",
                    "old_text": "key",
                    "new_text": "newkey"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        match err {
            ToolError::NotAuthorized(msg) => {
                assert!(msg.contains("not allowed"));
            }
            _ => panic!("Expected NotAuthorized, got {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_apply_patch_env_protected() {
        let tool = ApplyPatchTool;
        let ctx = ToolContext::local("test".to_string());

        let err = tool
            .execute(
                serde_json::json!({
                    "file_path": "/project/.env",
                    "old_text": "SECRET=old",
                    "new_text": "SECRET=new"
                }),
                &ctx,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::NotAuthorized(_)));
    }

    #[test]
    fn test_generate_diff_preview() {
        let old = "line1\nline2\nline3\nline4\nline5\nline6\nline7\n";
        let new_content = "line1\nline2\nline3\nNEW LINE\nline5\nline6\nline7\n";
        // match_pos is the start of "line4" in the old content
        let match_pos = "line1\nline2\nline3\n".len();

        let diff = generate_diff_preview(old, new_content, match_pos, "line4", "NEW LINE");
        assert!(diff.contains("-line4"));
        assert!(diff.contains("+NEW LINE"));
        // Should have context lines
        assert!(diff.contains(" line3"));
    }

    #[test]
    fn test_find_nearby_text() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        // "println!" is a common prefix — partial match should find it
        let suggestion = find_nearby_text(content, "println!(\"hello\");\n    extra_line();");
        assert!(suggestion.contains("println!"));
        // When no partial match at all, should show file preview
        let no_match = find_nearby_text(content, "totally_different_code()");
        assert!(no_match.contains("fn main"));
    }

    #[test]
    fn test_is_protected_path() {
        assert!(is_protected_path("/etc/shadow"));
        assert!(is_protected_path("/home/user/.ssh/id_rsa"));
        assert!(is_protected_path("/project/.env"));
        assert!(!is_protected_path("/home/user/project/main.rs"));
        assert!(!is_protected_path("/tmp/test.txt"));
    }
}
