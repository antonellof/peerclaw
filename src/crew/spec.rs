//! Declarative crew specification (agents, tasks, process).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CrewProcess {
    #[default]
    Sequential,
    Hierarchical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewAgentDef {
    pub id: String,
    pub role: String,
    pub goal: String,
    #[serde(default)]
    pub backstory: String,
    #[serde(default = "default_model")]
    pub llm: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub max_iter: u32,
}

fn default_model() -> String {
    "llama3.2:3b".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewTaskDef {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub expected_output: String,
    pub agent_id: String,
    /// Prior task ids whose outputs are injected as context.
    #[serde(default)]
    pub context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewSpec {
    #[serde(default)]
    pub name: String,
    pub agents: Vec<CrewAgentDef>,
    pub tasks: Vec<CrewTaskDef>,
    #[serde(default)]
    pub process: CrewProcess,
    /// For hierarchical process: optional manager agent id (must exist in agents).
    #[serde(default)]
    pub manager_agent_id: Option<String>,
    #[serde(default)]
    pub planning: bool,
}

impl CrewSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.agents.is_empty() {
            return Err("crew needs at least one agent".to_string());
        }
        if self.tasks.is_empty() {
            return Err("crew needs at least one task".to_string());
        }
        let ids: std::collections::HashSet<_> = self.agents.iter().map(|a| a.id.as_str()).collect();
        for t in &self.tasks {
            if !ids.contains(t.agent_id.as_str()) {
                return Err(format!(
                    "task {} references unknown agent {}",
                    t.id, t.agent_id
                ));
            }
            for c in &t.context {
                if !self.tasks.iter().any(|x| x.id == *c) {
                    return Err(format!(
                        "task {} references unknown context task {}",
                        t.id, c
                    ));
                }
            }
        }
        if self.process == CrewProcess::Hierarchical {
            if let Some(ref mid) = self.manager_agent_id {
                if !ids.contains(mid.as_str()) {
                    return Err(format!("manager_agent_id {mid} not found"));
                }
            }
        }
        Ok(())
    }
}
