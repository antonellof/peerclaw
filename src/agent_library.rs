//! Saved agents for chat and the Agent builder: flow graphs and skill-backed tasks.
//!
//! Built-in entries mirror `examples/agents/*.toml` and `examples/flows/` (task entries use the web
//! `general` skill; full TOML behavior remains `peerclaw serve --agent <path>`).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentLibraryKind {
    Flow,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLibraryEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: AgentLibraryKind,
    #[serde(default)]
    pub flow_spec: Option<crate::flow::FlowSpec>,
    #[serde(default)]
    pub task_type: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
}

impl AgentLibraryEntry {
    pub fn is_builtin(&self) -> bool {
        self.id.starts_with("builtin-")
    }
}

fn minimal_example_flow() -> Option<crate::flow::FlowSpec> {
    const RAW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/flows/minimal.json"));
    serde_json::from_str(RAW).ok()
}

/// Static catalog: example flow + example agent TOML shortcuts (web tasks use `general` unless you add skills).
pub fn builtin_entries() -> Vec<AgentLibraryEntry> {
    let mut v = Vec::new();
    if let Some(spec) = minimal_example_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-flow-minimal".into(),
            name: "Minimal flow (example)".into(),
            description: "Two-step LLM chain from examples/flows/minimal.json.".into(),
            kind: AgentLibraryKind::Flow,
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("examples/flows/minimal.json".into()),
        });
    }
    let toml_shortcuts: &[(&str, &str, &str, &str)] = &[
        (
            "builtin-agent-assistant",
            "Assistant",
            "examples/agents/assistant.toml",
            "Personal assistant. Web chat uses skill task `general`; for full TOML load use CLI: peerclaw serve --agent …/assistant.toml",
        ),
        (
            "builtin-agent-coder",
            "Coder",
            "examples/agents/coder.toml",
            "Coding-focused preset. Web: `general` task; CLI: --agent examples/agents/coder.toml",
        ),
        (
            "builtin-agent-researcher",
            "Researcher",
            "examples/agents/researcher.toml",
            "Research preset. Web: `general` task; CLI: --agent examples/agents/researcher.toml",
        ),
        (
            "builtin-agent-data-analyst",
            "Data analyst",
            "examples/agents/data-analyst.toml",
            "Analysis preset. Web: `general` task; CLI: --agent examples/agents/data-analyst.toml",
        ),
        (
            "builtin-agent-monitor",
            "Monitor",
            "examples/agents/monitor.toml",
            "Monitoring preset. Web: `general` task; CLI: --agent examples/agents/monitor.toml",
        ),
        (
            "builtin-agent-telegram-bot",
            "Telegram bot",
            "examples/agents/telegram-bot.toml",
            "Channel bot template. Web: `general` task for prompts only; Telegram needs CLI + channel config.",
        ),
    ];
    for &(id, name, path, desc) in toml_shortcuts {
        v.push(AgentLibraryEntry {
            id: id.into(),
            name: name.into(),
            description: desc.into(),
            kind: AgentLibraryKind::Task,
            flow_spec: None,
            task_type: Some("general".into()),
            source_path: Some(path.into()),
        });
    }
    v
}

pub fn load_user_entries(path: &Path) -> Vec<AgentLibraryEntry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub async fn save_user_entries(path: &Path, entries: &[AgentLibraryEntry]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let j = serde_json::to_string_pretty(entries).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;
    tokio::fs::write(path, j).await
}
