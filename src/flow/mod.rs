//! Declarative flow graphs for the web **Agent builder** (OpenAI-style).
//!
//! ## Execution modes
//! - **Legacy DAG** — no `start` node: all nodes run in topological order (backward compatible with
//!   `templates/flows/minimal.json`). Each LLM step sees outputs of all prior steps.
//! - **Interpreter** — exactly one node with `kind: "start"`: execution begins at `start` and follows
//!   outgoing edges. Branching uses edge `label` values (`true`/`false`, `loop`/`exit`, `pass`/`fail`).
//!
//! ## CEL (If / While)
//! Expressions are evaluated with variables: `inputs`, `outputs`, `state` (maps), `input_as_text` (string),
//! and `iteration` (uint, for while loops — completed iterations before the current check).
//!
//! See [OpenAI node reference](https://developers.openai.com/api/docs/guides/node-reference/) for the
//! conceptual catalog (Start, Agent, Note, File search, Guardrails, MCP, If/else, While, …).

mod interpreter;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::agent::AgentTaskExtras;
use crate::agent::AgenticInferenceSink;
use crate::executor::TaskExecutor;
use crate::prompts::PromptBundle;
use crate::safety::SafetyLayer;
use crate::tools::{NodeToolTx, ToolRegistry};
use crate::vector::VectorStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSpec {
    #[serde(default)]
    pub name: String,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlowNode {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    /// Display title in the agent builder UI.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    /// When `kind` is `crew` (or this is set), run a nested [`crate::crew::CrewSpec`].
    #[serde(default)]
    pub crew_spec: Option<crate::crew::CrewSpec>,
    // --- Agent ---
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Persist turns across flow runs (same key) when set with [`Self::include_chat_history`].
    #[serde(default)]
    pub include_chat_history: bool,
    /// `text` (default) or `json` — steers the model to emit JSON only when `json`.
    #[serde(default)]
    pub output_format: String,
    /// Session id for chat history; if empty with `include_chat_history`, defaults per node id.
    #[serde(default)]
    pub agent_session_key: String,
    // --- Classify (dedicated kind) ---
    #[serde(default)]
    pub classify_categories: Vec<String>,
    #[serde(default)]
    pub classify_model: String,
    /// `{{var}}` template against flow context; empty uses `input_as_text`.
    #[serde(default)]
    pub classify_input_template: String,
    /// JSON array of `{ "input": "...", "category": "..." }` few-shot examples.
    #[serde(default)]
    pub classify_examples_json: String,
    // --- If / While ---
    #[serde(default)]
    pub condition_cel: String,
    /// Optional label for documentation / UI (not used by the engine).
    #[serde(default)]
    pub if_case_name: String,
    /// 0 = default cap (100) in the interpreter.
    #[serde(default)]
    pub max_iterations: u32,
    // --- Guardrails ---
    #[serde(default)]
    pub source_node_id: String,
    #[serde(default)]
    pub guardrail_checks: Vec<String>,
    /// Interpolate `{{...}}` from inputs + outputs; when non-empty, overrides raw `source_node_id` text.
    #[serde(default)]
    pub guardrail_input_template: String,
    /// If true, failed checks still follow the **pass** edge (logged).
    #[serde(default)]
    pub guardrail_continue_on_error: bool,
    /// With check `custom`, fail when this substring appears (case-insensitive).
    #[serde(default)]
    pub guardrail_custom_substring: String,
    // --- MCP ---
    #[serde(default)]
    pub mcp_tool_id: String,
    #[serde(default)]
    pub mcp_arguments_json: String,
    // --- File search (local vector) ---
    #[serde(default)]
    pub vector_collection: String,
    #[serde(default)]
    pub vector_query_template: String,
    /// Max hits for `search_text` (0 = default 10).
    #[serde(default)]
    pub vector_top_k: u32,
    // --- Transform / set_state ---
    #[serde(default)]
    pub transform_from_node_id: String,
    /// `copy` (default), `expressions`, or `object`.
    #[serde(default)]
    pub transform_mode: String,
    /// JSON `[{ "key": "k", "cel": "expression" }]`.
    #[serde(default)]
    pub transform_expressions_json: String,
    /// JSON object merged into `state` when `transform_mode` is `object`.
    #[serde(default)]
    pub transform_object_json: String,
    #[serde(default)]
    pub state_key: String,
    #[serde(default)]
    pub state_value_json: String,
    /// When non-empty, evaluated as CEL instead of parsing [`Self::state_value_json`].
    #[serde(default)]
    pub state_value_cel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowEdge {
    pub from: String,
    pub to: String,
    /// Branch label: `true`/`false`, `loop`/`exit`, `pass`/`fail`, `default`, etc.
    #[serde(default)]
    pub label: Option<String>,
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
            r.logs
                .get_or_insert_with(Vec::new)
                .push("[flow] completed successfully".to_string());
        }
    }

    pub fn complete_err(&self, id: &str, err: impl Into<String>) {
        let msg = err.into();
        if let Some(r) = self.runs.write().get_mut(id) {
            r.status = "failed".to_string();
            r.error = Some(msg.clone());
            r.completed_at = Some(chrono::Utc::now().to_rfc3339());
            r.logs
                .get_or_insert_with(Vec::new)
                .push(format!("[flow] failed: {msg}"));
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
    /// Wrap a [`crate::crew::CrewSpec`] as a single-node flow (Start → Crew → End).
    pub fn from_crew(crew: crate::crew::CrewSpec) -> Self {
        let name = if crew.name.is_empty() {
            "crew-workflow".to_string()
        } else {
            crew.name.clone()
        };
        Self {
            name,
            nodes: vec![
                FlowNode { id: "s".into(), kind: "start".into(), name: "Start".into(), ..Default::default() },
                FlowNode {
                    id: "crew".into(),
                    kind: "crew".into(),
                    name: "Crew".into(),
                    crew_spec: Some(crew),
                    ..Default::default()
                },
                FlowNode { id: "e".into(), kind: "end".into(), name: "End".into(), ..Default::default() },
            ],
            edges: vec![
                FlowEdge { from: "s".into(), to: "crew".into(), label: None },
                FlowEdge { from: "crew".into(), to: "e".into(), label: None },
            ],
        }
    }

    /// Minimal single-agent flow: Start → Agent → End.  Used for agent preset library entries.
    pub fn single_agent(agent_name: &str) -> Self {
        Self {
            name: agent_name.to_string(),
            nodes: vec![
                FlowNode { id: "s".into(), kind: "start".into(), name: "Start".into(), ..Default::default() },
                FlowNode {
                    id: "agent".into(),
                    kind: "agent".into(),
                    name: agent_name.to_string(),
                    instructions: format!("You are a helpful {agent_name} agent. Complete the user's request."),
                    ..Default::default()
                },
                FlowNode { id: "e".into(), kind: "end".into(), name: "End".into(), ..Default::default() },
            ],
            edges: vec![
                FlowEdge { from: "s".into(), to: "agent".into(), label: None },
                FlowEdge { from: "agent".into(), to: "e".into(), label: None },
            ],
        }
    }

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

    /// `true` if this spec uses the graph interpreter (requires exactly one `start` node).
    pub fn has_interpreter_start(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| n.kind.eq_ignore_ascii_case("start"))
    }

    /// Validate for kickoff: legacy flows must be acyclic; interpreter flows use [`interpreter::validate_interpreter`].
    pub fn validate_for_run(&self) -> Result<(), String> {
        self.validate()?;
        if self.has_interpreter_start() {
            interpreter::validate_interpreter(self)
        } else {
            self.execution_order().map(|_| ())
        }
    }

    /// Topological order of nodes respecting edges (Kahn). Fails on cycles — used for legacy mode only.
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

/// Used by the web server and `serve` loop; passes optional vector + safety for file_search / guardrails.
#[allow(clippy::too_many_arguments)]
pub async fn run_flow_with_extras(
    spec: &FlowSpec,
    inputs: &serde_json::Value,
    default_model: &str,
    executor: Arc<TaskExecutor>,
    tools: Arc<ToolRegistry>,
    peer_id: String,
    node_tool_tx: Option<NodeToolTx>,
    inference_sink: Option<Arc<dyn AgenticInferenceSink>>,
    prompts: Arc<PromptBundle>,
    extras: AgentTaskExtras,
    vector_store: Option<Arc<VectorStore>>,
    safety: Option<Arc<SafetyLayer>>,
    flow_run_log: Option<(Arc<FlowRunStore>, String)>,
) -> Result<FlowRunOutput, String> {
    interpreter::run_flow(
        spec,
        inputs,
        default_model,
        executor,
        tools,
        peer_id,
        node_tool_tx,
        inference_sink,
        prompts,
        extras,
        vector_store,
        safety,
        flow_run_log,
    )
    .await
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
                    name: String::new(),
                    prompt: String::new(),
                    crew_spec: None,
                    instructions: String::new(),
                    model: String::new(),
                    tools: vec![],
                    temperature: None,
                    max_tokens: None,
                    include_chat_history: false,
                    output_format: String::new(),
                    agent_session_key: String::new(),
                    classify_categories: vec![],
                    classify_model: String::new(),
                    classify_input_template: String::new(),
                    classify_examples_json: String::new(),
                    condition_cel: String::new(),
                    if_case_name: String::new(),
                    max_iterations: 0,
                    source_node_id: String::new(),
                    guardrail_checks: vec![],
                    guardrail_input_template: String::new(),
                    guardrail_continue_on_error: false,
                    guardrail_custom_substring: String::new(),
                    mcp_tool_id: String::new(),
                    mcp_arguments_json: String::new(),
                    vector_collection: String::new(),
                    vector_query_template: String::new(),
                    vector_top_k: 0,
                    transform_from_node_id: String::new(),
                    transform_mode: String::new(),
                    transform_expressions_json: String::new(),
                    transform_object_json: String::new(),
                    state_key: String::new(),
                    state_value_json: String::new(),
                    state_value_cel: String::new(),
                },
                FlowNode {
                    id: "b".into(),
                    kind: String::new(),
                    name: String::new(),
                    prompt: String::new(),
                    crew_spec: None,
                    instructions: String::new(),
                    model: String::new(),
                    tools: vec![],
                    temperature: None,
                    max_tokens: None,
                    include_chat_history: false,
                    output_format: String::new(),
                    agent_session_key: String::new(),
                    classify_categories: vec![],
                    classify_model: String::new(),
                    classify_input_template: String::new(),
                    classify_examples_json: String::new(),
                    condition_cel: String::new(),
                    if_case_name: String::new(),
                    max_iterations: 0,
                    source_node_id: String::new(),
                    guardrail_checks: vec![],
                    guardrail_input_template: String::new(),
                    guardrail_continue_on_error: false,
                    guardrail_custom_substring: String::new(),
                    mcp_tool_id: String::new(),
                    mcp_arguments_json: String::new(),
                    vector_collection: String::new(),
                    vector_query_template: String::new(),
                    vector_top_k: 0,
                    transform_from_node_id: String::new(),
                    transform_mode: String::new(),
                    transform_expressions_json: String::new(),
                    transform_object_json: String::new(),
                    state_key: String::new(),
                    state_value_json: String::new(),
                    state_value_cel: String::new(),
                },
            ],
            edges: vec![FlowEdge {
                from: "a".into(),
                to: "b".into(),
                label: None,
            }],
        };
        assert_eq!(f.execution_order().unwrap(), vec!["a", "b"]);
    }
}
