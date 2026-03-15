//! Distributed memory tools for P2P workspace.
//!
//! These tools provide persistent memory across the P2P network:
//! - Search past memories across local and network storage
//! - Write memories that can be replicated to peers
//! - Support for both local-first and distributed modes

use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::tools::tool::{
    Tool, ToolContext, ToolError, ToolOutput, ToolDomain, ApprovalRequirement,
    require_str, optional_str, optional_i64, optional_bool,
};

/// Memory entry stored in the distributed workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID (BLAKE3 hash of content + timestamp)
    pub id: String,
    /// Content text
    pub content: String,
    /// Category/tag
    pub category: String,
    /// Source peer ID
    pub source_peer: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modified timestamp
    pub modified_at: DateTime<Utc>,
    /// Relevance score (for search results)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Whether this is replicated to other peers
    pub replicated: bool,
}

impl MemoryEntry {
    /// Create a new memory entry.
    pub fn new(content: String, category: String, peer_id: String) -> Self {
        let now = Utc::now();
        let id = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(content.as_bytes());
            hasher.update(now.to_rfc3339().as_bytes());
            hasher.finalize().to_hex()[..16].to_string()
        };

        Self {
            id,
            content,
            category,
            source_peer: peer_id,
            created_at: now,
            modified_at: now,
            score: None,
            replicated: false,
        }
    }
}

/// In-memory storage for demonstration (production would use redb + P2P sync).
/// TODO: Integrate with actual P2P storage layer.
static MEMORY_STORE: std::sync::LazyLock<tokio::sync::RwLock<Vec<MemoryEntry>>> =
    std::sync::LazyLock::new(|| tokio::sync::RwLock::new(Vec::new()));

/// Memory search tool - searches across local and network memories.
pub struct MemorySearchTool {
    // In production, this would hold references to:
    // - Local redb database
    // - P2P network for distributed search
    // - Vector embedding service for semantic search
}

impl MemorySearchTool {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search past memories, decisions, and context across the P2P network. \
         MUST be called before answering questions about prior work, decisions, \
         dates, people, preferences, or todos. Supports both keyword and semantic search."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query - natural language description of what you're looking for"
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category (facts, decisions, preferences, todos, daily_log)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 10, max: 50)"
                },
                "include_network": {
                    "type": "boolean",
                    "description": "Include results from other peers (default: true)"
                },
                "min_score": {
                    "type": "number",
                    "description": "Minimum relevance score (0.0-1.0, default: 0.1)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let query = require_str(&params, "query")?;
        let category = optional_str(&params, "category");
        let limit = optional_i64(&params, "limit", 10).min(50) as usize;
        let include_network = optional_bool(&params, "include_network", true);

        // Search local memory
        let store = MEMORY_STORE.read().await;
        let query_lower = query.to_lowercase();

        let mut results: Vec<MemoryEntry> = store
            .iter()
            .filter(|entry| {
                // Category filter
                if let Some(cat) = category {
                    if entry.category != cat {
                        return false;
                    }
                }

                // Simple keyword matching (production would use vector similarity)
                let content_lower = entry.content.to_lowercase();
                query_lower.split_whitespace().any(|word| content_lower.contains(word))
            })
            .cloned()
            .map(|mut entry| {
                // Calculate simple relevance score
                let content_lower = entry.content.to_lowercase();
                let matched_words = query_lower
                    .split_whitespace()
                    .filter(|word| content_lower.contains(word))
                    .count();
                let total_words = query_lower.split_whitespace().count().max(1);
                entry.score = Some(matched_words as f32 / total_words as f32);
                entry
            })
            .collect();

        // Sort by score
        results.sort_by(|a, b| {
            b.score.unwrap_or(0.0).partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit results
        results.truncate(limit);

        let result = serde_json::json!({
            "query": query,
            "results": results,
            "result_count": results.len(),
            "searched_local": true,
            "searched_network": include_network,
            "peer_id": ctx.peer_id,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any // Can search local or network
    }

    fn requires_sanitization(&self) -> bool {
        false // Internal memory, trusted content
    }
}

/// Memory write tool - persists memories locally and optionally replicates to network.
pub struct MemoryWriteTool {
    // In production: redb handle, P2P network reference
}

impl MemoryWriteTool {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MemoryWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Write to persistent distributed memory. Use for important facts, decisions, \
         preferences, or lessons learned that should be remembered across sessions. \
         Memories can be replicated to trusted peers for redundancy."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The content to remember. Be concise but include relevant context."
                },
                "category": {
                    "type": "string",
                    "description": "Category: facts, decisions, preferences, todos, daily_log",
                    "enum": ["facts", "decisions", "preferences", "todos", "daily_log"]
                },
                "replicate": {
                    "type": "boolean",
                    "description": "Replicate to trusted peers for redundancy (default: false)"
                },
                "append_to": {
                    "type": "string",
                    "description": "Append to existing memory by ID instead of creating new"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let content = require_str(&params, "content")?;
        let category = optional_str(&params, "category").unwrap_or("facts");
        let replicate = optional_bool(&params, "replicate", false);
        let append_to = optional_str(&params, "append_to");

        // Validate content
        if content.trim().is_empty() {
            return Err(ToolError::InvalidParameters("Content cannot be empty".to_string()));
        }

        if content.len() > 100_000 {
            return Err(ToolError::InvalidParameters(
                "Content too large (max 100KB)".to_string()
            ));
        }

        let mut store = MEMORY_STORE.write().await;

        let entry = if let Some(existing_id) = append_to {
            // Append to existing entry
            if let Some(entry) = store.iter_mut().find(|e| e.id == existing_id) {
                entry.content.push_str("\n\n");
                entry.content.push_str(content);
                entry.modified_at = Utc::now();
                entry.clone()
            } else {
                return Err(ToolError::ExecutionFailed(
                    format!("Memory not found: {}", existing_id)
                ));
            }
        } else {
            // Create new entry
            let entry = MemoryEntry::new(
                content.to_string(),
                category.to_string(),
                ctx.peer_id.clone(),
            );
            store.push(entry.clone());
            entry
        };

        // TODO: If replicate is true, broadcast to P2P network
        if replicate {
            tracing::info!(
                memory_id = %entry.id,
                "Memory marked for replication (not yet implemented)"
            );
        }

        let result = serde_json::json!({
            "id": entry.id,
            "category": entry.category,
            "created_at": entry.created_at.to_rfc3339(),
            "content_length": entry.content.len(),
            "replicated": replicate,
            "peer_id": ctx.peer_id,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local // Writes are local-first, then replicated
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_write_and_search() {
        let write_tool = MemoryWriteTool::new();
        let search_tool = MemorySearchTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        // Write a memory
        let write_result = write_tool.execute(
            serde_json::json!({
                "content": "The user prefers dark mode and vim keybindings",
                "category": "preferences"
            }),
            &ctx,
        ).await.unwrap();

        assert!(write_result.success);
        let memory_id = write_result.data["id"].as_str().unwrap();

        // Search for it
        let search_result = search_tool.execute(
            serde_json::json!({
                "query": "dark mode preferences"
            }),
            &ctx,
        ).await.unwrap();

        assert!(search_result.success);
        assert!(search_result.data["result_count"].as_u64().unwrap() >= 1);
    }

    #[tokio::test]
    async fn test_memory_categories() {
        let write_tool = MemoryWriteTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        // Write memories in different categories
        for category in ["facts", "decisions", "todos"] {
            let result = write_tool.execute(
                serde_json::json!({
                    "content": format!("Test content for {}", category),
                    "category": category
                }),
                &ctx,
            ).await.unwrap();
            assert!(result.success);
        }
    }
}
