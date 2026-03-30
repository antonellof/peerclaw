//! Saved workflows for chat and the Workflow builder.
//!
//! Every entry is a flow graph (`FlowSpec`).  Built-in entries include example flows
//! and minimal single-agent wrappers for the `templates/agents/*.toml` presets.
//! Full TOML agent behavior remains `peerclaw serve --agent <path>`.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLibraryEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Always `"flow"`. Legacy `"task"` values are migrated on load.
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub flow_spec: Option<crate::flow::FlowSpec>,
    /// Deprecated — kept for backward compat during migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
}

fn default_kind() -> String {
    "flow".into()
}

impl AgentLibraryEntry {
    pub fn is_builtin(&self) -> bool {
        self.id.starts_with("builtin-")
    }

    /// Ensure legacy `task` entries are migrated to `flow` with a minimal spec.
    pub fn migrate_if_needed(&mut self) {
        if self.kind == "task" && self.flow_spec.is_none() {
            self.flow_spec = Some(crate::flow::FlowSpec::single_agent(
                self.task_type.as_deref().unwrap_or("general"),
            ));
            self.kind = "flow".into();
        } else if self.kind != "flow" {
            self.kind = "flow".into();
        }
    }
}

fn load_flow_template(json: &str) -> Option<crate::flow::FlowSpec> {
    serde_json::from_str(json).ok()
}

fn minimal_example_flow() -> Option<crate::flow::FlowSpec> {
    load_flow_template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/flows/minimal.json")))
}

fn interpreter_example_flow() -> Option<crate::flow::FlowSpec> {
    load_flow_template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/flows/interpreter-linear.json")))
}

fn deep_researcher_flow() -> Option<crate::flow::FlowSpec> {
    load_flow_template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/flows/deep-researcher.json")))
}

fn code_reviewer_flow() -> Option<crate::flow::FlowSpec> {
    load_flow_template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/flows/code-reviewer.json")))
}

fn creative_writer_flow() -> Option<crate::flow::FlowSpec> {
    load_flow_template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/flows/creative-writer.json")))
}

fn data_analyst_flow() -> Option<crate::flow::FlowSpec> {
    load_flow_template(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/flows/data-analyst.json")))
}

fn crew_example_flow() -> Option<crate::flow::FlowSpec> {
    const RAW: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/crews/minimal.json"));
    let crew: crate::crew::CrewSpec = serde_json::from_str(RAW).ok()?;
    Some(crate::flow::FlowSpec::from_crew(crew))
}

/// Static catalog shipped with the binary.
pub fn builtin_entries() -> Vec<AgentLibraryEntry> {
    let mut v = Vec::new();

    // Example flows
    if let Some(spec) = minimal_example_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-flow-minimal".into(),
            name: "Minimal flow".into(),
            description: "Two-step LLM chain (legacy DAG mode).".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/flows/minimal.json".into()),
        });
    }
    if let Some(spec) = interpreter_example_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-flow-interpreter".into(),
            name: "Linear flow".into(),
            description: "Start → LLM → End (interpreter mode).".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/flows/interpreter-linear.json".into()),
        });
    }
    if let Some(spec) = crew_example_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-crew-minimal".into(),
            name: "Crew example".into(),
            description: "Multi-agent crew wrapped as a workflow.".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/crews/minimal.json".into()),
        });
    }

    // Multi-step agents (sophisticated flows with classify, guardrails, multi-agent chains)
    if let Some(spec) = deep_researcher_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-agent-researcher".into(),
            name: "Deep Researcher".into(),
            description: "Classify → guardrail → research → synthesize into a polished report.".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/flows/deep-researcher.json".into()),
        });
    }
    if let Some(spec) = code_reviewer_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-agent-coder".into(),
            name: "Code Reviewer".into(),
            description: "Analyze → refactor suggestions → formatted review with severity levels.".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/flows/code-reviewer.json".into()),
        });
    }
    if let Some(spec) = creative_writer_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-agent-writer".into(),
            name: "Creative Writer".into(),
            description: "Classify → outline → draft → editor pass for polished creative writing.".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/flows/creative-writer.json".into()),
        });
    }
    if let Some(spec) = data_analyst_flow() {
        v.push(AgentLibraryEntry {
            id: "builtin-agent-data-analyst".into(),
            name: "Data Analyst".into(),
            description: "Understand → analyze → insights & recommendations pipeline.".into(),
            kind: "flow".into(),
            flow_spec: Some(spec),
            task_type: None,
            source_path: Some("templates/flows/data-analyst.json".into()),
        });
    }

    // Simple single-agent presets
    let simple_presets: &[(&str, &str, &str)] = &[
        ("builtin-agent-assistant", "Assistant", "General-purpose personal assistant."),
        ("builtin-agent-monitor", "Monitor", "System monitoring agent."),
    ];
    for &(id, name, desc) in simple_presets {
        v.push(AgentLibraryEntry {
            id: id.into(),
            name: name.into(),
            description: desc.into(),
            kind: "flow".into(),
            flow_spec: Some(crate::flow::FlowSpec::single_agent(name)),
            task_type: None,
            source_path: None,
        });
    }
    v
}

pub fn load_user_entries(path: &Path) -> Vec<AgentLibraryEntry> {
    let mut entries: Vec<AgentLibraryEntry> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // Migrate legacy "task" entries to "flow"
    for e in &mut entries {
        e.migrate_if_needed();
    }
    entries
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
