//! Declarative flow graphs (DAG over named steps).

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::agent::AgentTaskExtras;
use crate::crew;
use crate::executor::task::{ExecutionTask, InferenceTask, TaskData};
use crate::executor::TaskExecutor;
use crate::tools::{NodeToolTx, ToolRegistry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSpec {
    #[serde(default)]
    pub name: String,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowNode {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub prompt: String,
    /// When `kind` is `crew` (or this is set), run a nested [`crew::CrewSpec`].
    #[serde(default)]
    pub crew_spec: Option<crate::crew::CrewSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowRunOutput {
    pub steps: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowRunRecord {
    pub id: String,
    pub status: String,
    pub flow_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<FlowRunOutput>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<Vec<String>>,
}

pub struct FlowRunStore {
    runs: RwLock<HashMap<String, FlowRunRecord>>,
}

impl FlowRunStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            runs: RwLock::new(HashMap::new()),
        })
    }

    pub fn insert_pending(&self, id: impl Into<String>, spec: &FlowSpec) -> String {
        let id = id.into();
        let rec = FlowRunRecord {
            id: id.clone(),
            status: "pending".to_string(),
            flow_name: spec.name.clone(),
            error: None,
            output: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            logs: Some(vec!["[flow] queued".to_string()]),
        };
        self.runs.write().insert(id.clone(), rec);
        id
    }

    pub fn update_status(&self, id: &str, status: &str) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = status.to_string();
        }
    }

    pub fn push_log(&self, id: &str, line: impl Into<String>) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.logs.get_or_insert_with(Vec::new).push(line.into());
        }
    }

    pub fn complete_ok(&self, id: &str, out: FlowRunOutput) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = "completed".to_string();
            r.output = Some(out);
            r.completed_at = Some(chrono::Utc::now().to_rfc3339());
            r.error = None;
        }
    }

    pub fn complete_err(&self, id: &str, err: impl Into<String>) {
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = "failed".to_string();
            r.error = Some(err.into());
            r.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    pub fn get(&self, id: &str) -> Option<FlowRunRecord> {
        self.runs.read().get(id).cloned()
    }

    pub fn list(&self) -> Vec<FlowRunRecord> {
        let mut v: Vec<_> = self.runs.read().values().cloned().collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }
}

/// Replace `${key}` placeholders using string leaves from `inputs`.
pub fn interpolate_inputs(template: &str, inputs: &serde_json::Value) -> String {
    let mut s = template.to_string();
    let Some(obj) = inputs.as_object() else {
        return s;
    };
    for (k, v) in obj {
        let needle = format!("${{{k}}}");
        let rep = match v {
            serde_json::Value::String(x) => x.clone(),
            o => o.to_string(),
        };
        s = s.replace(&needle, &rep);
    }
    s
}

impl FlowSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.nodes.is_empty() {
            return Err("flow needs nodes".to_string());
        }
        let ids: HashSet<_> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        for e in &self.edges {
            if !ids.contains(e.from.as_str()) {
                return Err(format!("edge from unknown node {}", e.from));
            }
            if !ids.contains(e.to.as_str()) {
                return Err(format!("edge to unknown node {}", e.to));
            }
        }
        Ok(())
    }

    /// Topological order of nodes respecting edges (Kahn).
    pub fn execution_order(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        let mut indeg: HashMap<String, usize> = HashMap::new();
        for n in &self.nodes {
            indeg.insert(n.id.clone(), 0);
        }
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for e in &self.edges {
            *indeg.entry(e.to.clone()).or_insert(0) += 1;
            adj.entry(e.from.clone()).or_default().push(e.to.clone());
        }
        let mut q: VecDeque<String> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(k, _)| k.clone())
            .collect();
        let mut out = Vec::new();
        while let Some(u) = q.pop_front() {
            out.push(u.clone());
            if let Some(nbrs) = adj.get(&u) {
                for v in nbrs {
                    let e = indeg.get_mut(v).unwrap();
                    *e -= 1;
                    if *e == 0 {
                        q.push_back(v.clone());
                    }
                }
            }
        }
        if out.len() != self.nodes.len() {
            return Err("flow has a cycle".to_string());
        }
        Ok(out)
    }
}

/// Execute flow steps in topological order (LLM nodes + optional nested crews).
pub async fn run_flow(
    spec: &FlowSpec,
    inputs: &serde_json::Value,
    default_model: &str,
    executor: Arc<TaskExecutor>,
    tools: Arc<ToolRegistry>,
    peer_id: String,
    node_tool_tx: Option<NodeToolTx>,
    inference_sink: Option<Arc<dyn crate::agent::AgenticInferenceSink>>,
    prompts: Arc<crate::prompts::PromptBundle>,
    extras: AgentTaskExtras,
) -> Result<FlowRunOutput, String> {
    spec.validate()?;
    let order = spec.execution_order()?;
    let mut step_outputs: HashMap<String, serde_json::Value> = HashMap::new();
    let mut ordered_steps = Vec::new();

    for node_id in order {
        let node = spec
            .nodes
            .iter()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("missing node {node_id}"))?;
        let kind = node.kind.to_lowercase();
        if kind == "crew" || node.crew_spec.is_some() {
            let crew = node.crew_spec.as_ref().ok_or_else(|| {
                format!("flow node {}: crew steps require \"crew_spec\"", node.id)
            })?;
            crew.validate()?;
            let merged_inputs = if node.prompt.is_empty() {
                inputs.clone()
            } else {
                serde_json::json!({ "flow_prompt": interpolate_inputs(&node.prompt, inputs) })
            };
            let out = crew::run_crew(
                crew,
                &merged_inputs,
                executor.clone(),
                tools.clone(),
                peer_id.clone(),
                node_tool_tx.clone(),
                inference_sink.clone(),
                prompts.clone(),
                None,
                extras.clone(),
            )
            .await?;
            let v = serde_json::to_value(&out).map_err(|e| e.to_string())?;
            step_outputs.insert(node.id.clone(), v.clone());
            ordered_steps.push(serde_json::json!({"id": node.id, "kind": "crew", "output": v}));
            continue;
        }

        let prompt = interpolate_inputs(&node.prompt, inputs);
        let mut ctx = prompt;
        for (k, v) in &step_outputs {
            ctx.push_str(&format!("\n\n--- prior step {k} ---\n{v}"));
        }
        let task = InferenceTask::new(default_model, ctx).with_max_tokens(512);
        let res = executor
            .execute(ExecutionTask::Inference(task))
            .await
            .map_err(|e| e.to_string())?;
        let text = match res.data {
            TaskData::Inference(r) => r.text,
            TaskData::Error(e) => return Err(e),
            _ => return Err("non-inference flow step".into()),
        };
        let j = serde_json::json!({ "text": text });
        step_outputs.insert(node.id.clone(), j.clone());
        ordered_steps.push(serde_json::json!({"id": node.id, "kind": "llm", "output": j}));
    }

    Ok(FlowRunOutput {
        steps: ordered_steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_order_linear() {
        let f = FlowSpec {
            name: "t".into(),
            nodes: vec![
                FlowNode {
                    id: "a".into(),
                    kind: String::new(),
                    prompt: String::new(),
                    crew_spec: None,
                },
                FlowNode {
                    id: "b".into(),
                    kind: String::new(),
                    prompt: String::new(),
                    crew_spec: None,
                },
            ],
            edges: vec![FlowEdge {
                from: "a".into(),
                to: "b".into(),
            }],
        };
        assert_eq!(f.execution_order().unwrap(), vec!["a", "b"]);
    }
}
