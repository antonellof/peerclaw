//! Graph interpreter for [`super::FlowSpec`] (OpenAI Agent Builder–style branching).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use cel::{Context, Program};
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::agent::runtime::{AgentConfig, AgentRuntime, AgentTaskExtras};
use crate::agent::AgenticInferenceSink;
use crate::crew;
use crate::executor::task::{ExecutionTask, InferenceTask, TaskData};
use crate::executor::TaskExecutor;
use crate::mcp::McpManager;
use crate::prompts::PromptBundle;
use crate::safety::ssrf::validate_url_relaxed;
use crate::safety::SafetyLayer;
use crate::tools::{NodeToolTx, ToolRegistry};
use crate::vector::VectorStore;

use super::{interpolate_inputs, FlowNode, FlowRunOutput, FlowRunStore, FlowSpec};

fn flow_step_log(log: &Option<(Arc<FlowRunStore>, String)>, line: impl std::fmt::Display) {
    if let Some((store, id)) = log {
        store.push_log(id, line.to_string());
    }
}

/// Extended `{{key}}` interpolation: `ctx` should be a JSON object (merged inputs + stringified outputs).
pub fn interpolate_context(template: &str, ctx: &Value) -> String {
    let mut out = template.to_string();
    let Some(obj) = ctx.as_object() else {
        return out;
    };
    for (k, v) in obj {
        let pat = format!("{{{k}}}");
        let rep = match v {
            Value::String(x) => x.clone(),
            o => o.to_string(),
        };
        out = out.replace(&pat, &rep);
    }
    out
}

fn build_template_context(inputs: &Value, outputs: &HashMap<String, Value>) -> Value {
    let mut m = Map::new();
    if let Some(obj) = inputs.as_object() {
        for (k, v) in obj {
            m.insert(k.clone(), v.clone());
        }
    }
    for (id, v) in outputs {
        m.insert(id.clone(), v.clone());
    }
    Value::Object(m)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

fn activation_for_cel(
    inputs: &Value,
    outputs: &HashMap<String, Value>,
    state_map: &Map<String, Value>,
    input_as_text: &str,
    iteration: u32,
) -> Value {
    json!({
        "inputs": inputs.clone(),
        "outputs": Value::Object(outputs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        "state": Value::Object(state_map.clone()),
        "input_as_text": input_as_text,
        "iteration": iteration,
    })
}

fn cel_fill_context(ctx: &mut Context, activation: &Value, iteration: u32) -> Result<(), String> {
    ctx.add_variable("inputs", activation.get("inputs").cloned().unwrap_or(Value::Null))
        .map_err(|e| e.to_string())?;
    ctx.add_variable("outputs", activation.get("outputs").cloned().unwrap_or(Value::Null))
        .map_err(|e| e.to_string())?;
    ctx.add_variable("state", activation.get("state").cloned().unwrap_or(Value::Null))
        .map_err(|e| e.to_string())?;
    let it = activation
        .get("input_as_text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    ctx.add_variable("input_as_text", it)
        .map_err(|e| e.to_string())?;
    ctx.add_variable("iteration", iteration as i64)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn cel_bool(expr: &str, activation: &Value, iteration: u32) -> Result<bool, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Ok(false);
    }
    let program = Program::compile(expr).map_err(|e| format!("CEL parse: {e:?}"))?;
    let mut ctx = Context::default();
    cel_fill_context(&mut ctx, activation, iteration)?;
    let v = program.execute(&ctx).map_err(|e| format!("CEL exec: {e}"))?;
    match v {
        cel::Value::Bool(b) => Ok(b),
        cel::Value::Int(i) => Ok(i != 0),
        cel::Value::UInt(u) => Ok(u != 0),
        cel::Value::Null => Ok(false),
        _ => Err(format!("CEL result is not bool: {v:?}")),
    }
}

fn cel_json_value(expr: &str, activation: &Value, iteration: u32) -> Result<Value, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err("empty CEL expression".to_string());
    }
    let program = Program::compile(expr).map_err(|e| format!("CEL parse: {e:?}"))?;
    let mut ctx = Context::default();
    cel_fill_context(&mut ctx, activation, iteration)?;
    let v = program.execute(&ctx).map_err(|e| format!("CEL exec: {e}"))?;
    v.json().map_err(|e| format!("CEL→JSON: {e}"))
}

static URL_IN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s]+").expect("url pattern"));

fn guard_urls_allowed(text: &str) -> bool {
    for m in URL_IN_TEXT.find_iter(text) {
        if validate_url_relaxed(m.as_str()).is_err() {
            return false;
        }
    }
    true
}

fn normalize_label(l: &Option<String>) -> Option<String> {
    l.as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
}

fn outgoing_map(spec: &FlowSpec) -> HashMap<String, Vec<(String, Option<String>)>> {
    let mut m: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
    for e in &spec.edges {
        m.entry(e.from.clone())
            .or_default()
            .push((e.to.clone(), e.label.clone()));
    }
    m
}

fn incoming_map(spec: &FlowSpec) -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for e in &spec.edges {
        m.entry(e.to.clone()).or_default().push(e.from.clone());
    }
    m
}

fn pick_next(
    from: &FlowNode,
    outs: &[(String, Option<String>)],
    branch: Option<&str>,
) -> Result<Option<String>, String> {
    let k = from.kind.to_ascii_lowercase();
    match k.as_str() {
        "if" | "ifelse" => {
            let want = branch.ok_or_else(|| format!("node {}: if/else internal error", from.id))?;
            let want = want.to_ascii_lowercase();
            for (to, lab) in outs {
                if normalize_label(&Some(lab.clone().unwrap_or_default()))
                    .as_deref()
                    == Some(want.as_str())
                {
                    return Ok(Some(to.clone()));
                }
            }
            Err(format!(
                "node {}: missing '{}' labeled outgoing edge",
                from.id, want
            ))
        }
        "while" => {
            let want = branch.ok_or_else(|| format!("node {}: while internal error", from.id))?;
            let want = want.to_ascii_lowercase();
            for (to, lab) in outs {
                if normalize_label(lab).as_deref() == Some(want.as_str()) {
                    return Ok(Some(to.clone()));
                }
            }
            Err(format!(
                "node {}: missing '{}' labeled outgoing edge",
                from.id, want
            ))
        }
        "guardrails" => {
            let want = branch.ok_or_else(|| {
                format!(
                    "node {}: guardrails internal error",
                    from.id
                )
            })?;
            let want = want.to_ascii_lowercase();
            for (to, lab) in outs {
                if normalize_label(lab).as_deref() == Some(want.as_str()) {
                    return Ok(Some(to.clone()));
                }
            }
            Err(format!(
                "node {}: missing '{}' labeled outgoing edge",
                from.id, want
            ))
        }
        "human_approval" | "humanapproval" | "user_approval" | "userapproval" => {
            let want = branch.ok_or_else(|| format!("node {}: user approval internal error", from.id))?;
            let want = want.to_ascii_lowercase();
            for (to, lab) in outs {
                if normalize_label(lab).as_deref() == Some(want.as_str()) {
                    return Ok(Some(to.clone()));
                }
            }
            Err(format!(
                "node {}: missing '{}' labeled outgoing edge",
                from.id, want
            ))
        }
        _ => {
            if outs.is_empty() {
                return Ok(None);
            }
            if outs.len() == 1 {
                return Ok(Some(outs[0].0.clone()));
            }
            let unlabeled: Vec<_> = outs
                .iter()
                .filter(|(_, l)| {
                    l.as_ref()
                        .map(|s| s.trim().is_empty())
                        .unwrap_or(true)
                })
                .collect();
            if unlabeled.len() == 1 {
                return Ok(Some(unlabeled[0].0.clone()));
            }
            let defaults: Vec<_> = outs
                .iter()
                .filter(|(_, l)| normalize_label(l).as_deref() == Some("default"))
                .collect();
            if defaults.len() == 1 {
                return Ok(Some(defaults[0].0.clone()));
            }
            Err(format!(
                "node {}: ambiguous outgoing edges ({}); use one unlabeled edge or one default label",
                from.id,
                outs.len()
            ))
        }
    }
}

fn prior_context_block(
    spec: &FlowSpec,
    node_id: &str,
    outputs: &HashMap<String, Value>,
) -> String {
    let inc = incoming_map(spec);
    let Some(srcs) = inc.get(node_id) else {
        return String::new();
    };
    let mut v: Vec<&String> = srcs.iter().collect();
    v.sort();
    let mut s = String::new();
    for sid in v {
        if let Some(o) = outputs.get(sid.as_str()) {
            s.push_str(&format!("\n\n--- from {sid} ---\n{}", value_to_string(o)));
        }
    }
    s
}

/// Run flow: interpreter mode if a `start` node exists; otherwise legacy topological DAG execution.
#[allow(clippy::too_many_arguments)]
pub async fn run_flow(
    spec: &FlowSpec,
    inputs: &Value,
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
    spec.validate_for_run()?;
    if spec.has_interpreter_start() {
        run_interpreter(
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
    } else {
        run_legacy_topo(
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
            flow_run_log,
        )
        .await
    }
}

async fn run_legacy_topo(
    spec: &FlowSpec,
    inputs: &Value,
    default_model: &str,
    executor: Arc<TaskExecutor>,
    tools: Arc<ToolRegistry>,
    peer_id: String,
    node_tool_tx: Option<NodeToolTx>,
    inference_sink: Option<Arc<dyn AgenticInferenceSink>>,
    prompts: Arc<PromptBundle>,
    extras: AgentTaskExtras,
    flow_run_log: Option<(Arc<FlowRunStore>, String)>,
) -> Result<FlowRunOutput, String> {
    let order = spec.execution_order()?;
    let mut step_outputs: HashMap<String, Value> = HashMap::new();
    let mut ordered_steps = Vec::new();

    for node_id in order {
        let node = spec
            .nodes
            .iter()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("missing node {node_id}"))?;
        let kind = node.kind.to_ascii_lowercase();
        flow_step_log(
            &flow_run_log,
            format!("[step] {node_id} ({kind}) [legacy DAG]"),
        );
        if kind == "note" {
            continue;
        }
        if kind == "crew" || node.crew_spec.is_some() {
            let crew = node.crew_spec.as_ref().ok_or_else(|| {
                format!("flow node {}: crew steps require \"crew_spec\"", node.id)
            })?;
            crew.validate()?;
            let merged_inputs = if node.prompt.is_empty() {
                inputs.clone()
            } else {
                json!({ "flow_prompt": interpolate_inputs(&node.prompt, inputs) })
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
            ordered_steps.push(json!({"id": node.id, "kind": "crew", "output": v}));
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
        let j = json!({ "text": text });
        step_outputs.insert(node.id.clone(), j.clone());
        ordered_steps.push(json!({"id": node.id, "kind": "llm", "output": j}));
    }

    Ok(FlowRunOutput { steps: ordered_steps })
}

#[allow(clippy::too_many_arguments)]
async fn run_interpreter(
    spec: &FlowSpec,
    inputs: &Value,
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
    let vector_store = crate::vector::resolve_vector_store(vector_store);
    let start_ids: Vec<_> = spec
        .nodes
        .iter()
        .filter(|n| n.kind.eq_ignore_ascii_case("start"))
        .map(|n| n.id.clone())
        .collect();
    let start_id = start_ids
        .first()
        .cloned()
        .ok_or_else(|| "interpreter mode requires exactly one start node".to_string())?;

    let out_adj = outgoing_map(spec);
    let mut outputs: HashMap<String, Value> = HashMap::new();
    let mut state_map: Map<String, Value> = Map::new();
    let mut while_iter: HashMap<String, u32> = HashMap::new();
    let mut ordered_steps: Vec<Value> = Vec::new();

    let input_as_text = inputs
        .get("input_as_text")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            inputs
                .get("text")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| inputs.to_string());

    let mut cursor = Some(start_id);
    let mut steps_guard = 0u32;
    const MAX_STEPS: u32 = 10_000;

    while let Some(cur) = cursor.take() {
        steps_guard += 1;
        if steps_guard > MAX_STEPS {
            return Err("flow interpreter: max steps exceeded (infinite loop?)".to_string());
        }

        let node = spec
            .nodes
            .iter()
            .find(|n| n.id == cur)
            .ok_or_else(|| format!("missing node {cur}"))?;
        let kind = node.kind.to_ascii_lowercase();
        flow_step_log(&flow_run_log, format!("[step] {cur} ({kind})"));
        let outs = out_adj.get(&cur).map(|v| v.as_slice()).unwrap_or(&[]);

        match kind.as_str() {
            "note" => {
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "start" => {
                outputs.insert(cur.clone(), inputs.clone());
                ordered_steps.push(json!({"id": cur, "kind": "start", "output": inputs}));
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "end" => {
                ordered_steps.push(json!({"id": cur, "kind": "end", "output": null}));
                break;
            }
            "human_approval" | "humanapproval" | "user_approval" | "userapproval" => {
                // Until pause/resume exists, branch is driven by flow inputs (for builder testing).
                let raw = inputs
                    .get("human_approval")
                    .or_else(|| inputs.get("simulated_human_approval"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("approve");
                let branch = if raw.eq_ignore_ascii_case("reject") {
                    "reject"
                } else {
                    "approve"
                };
                flow_step_log(
                    &flow_run_log,
                    format!(
                        "[user_approval] {cur} → {branch} (set inputs.human_approval to approve|reject)"
                    ),
                );
                let msg = node.prompt.trim();
                let out = json!({
                    "branch": branch,
                    "message": msg,
                    "simulated": true,
                });
                outputs.insert(cur.clone(), out.clone());
                ordered_steps.push(json!({"id": cur, "kind": "user_approval", "branch": branch, "output": out}));
                cursor = pick_next(node, outs, Some(branch))?;
                continue;
            }
            "if" | "ifelse" => {
                let act = json!({
                    "inputs": inputs,
                    "outputs": Value::Object(outputs.iter().map(|(k,v)| (k.clone(), v.clone())).collect()),
                    "state": Value::Object(state_map.clone()),
                    "input_as_text": input_as_text,
                });
                let cond = cel_bool(&node.condition_cel, &act, 0)?;
                let branch = if cond { "true" } else { "false" };
                flow_step_log(
                    &flow_run_log,
                    format!("[branch] if {cur} → {branch}"),
                );
                cursor = pick_next(node, outs, Some(branch))?;
                ordered_steps.push(json!({"id": cur, "kind": kind, "branch": branch}));
                continue;
            }
            "while" => {
                let max_iter = if node.max_iterations == 0 {
                    100
                } else {
                    node.max_iterations
                };
                let done = *while_iter.get(&cur).unwrap_or(&0);
                if done >= max_iter {
                    while_iter.remove(&cur);
                    flow_step_log(
                        &flow_run_log,
                        format!("[branch] while {cur} → exit (max_iterations)"),
                    );
                    cursor = pick_next(node, outs, Some("exit"))?;
                    ordered_steps.push(json!({"id": cur, "kind": "while", "branch": "exit", "reason": "max_iterations"}));
                    continue;
                }
                let act = json!({
                    "inputs": inputs,
                    "outputs": Value::Object(outputs.iter().map(|(k,v)| (k.clone(), v.clone())).collect()),
                    "state": Value::Object(state_map.clone()),
                    "input_as_text": input_as_text,
                    "iteration": done,
                });
                let cond = cel_bool(&node.condition_cel, &act, done)?;
                if !cond {
                    while_iter.remove(&cur);
                    flow_step_log(&flow_run_log, format!("[branch] while {cur} → exit"));
                    cursor = pick_next(node, outs, Some("exit"))?;
                    ordered_steps.push(json!({"id": cur, "kind": "while", "branch": "exit"}));
                    continue;
                }
                *while_iter.entry(cur.clone()).or_insert(0) += 1;
                flow_step_log(
                    &flow_run_log,
                    format!("[branch] while {cur} → loop (iter {done})"),
                );
                cursor = pick_next(node, outs, Some("loop"))?;
                ordered_steps.push(json!({"id": cur, "kind": "while", "branch": "loop", "iteration": done}));
                continue;
            }
            "guardrails" => {
                let tpl_ctx = build_template_context(inputs, &outputs);
                let text = if !node.guardrail_input_template.trim().is_empty() {
                    interpolate_context(&node.guardrail_input_template, &tpl_ctx)
                } else {
                    let sid = node.source_node_id.trim();
                    if sid.is_empty() {
                        return Err(format!(
                            "node {}: guardrails needs source_node_id or guardrail_input_template",
                            node.id
                        ));
                    }
                    outputs
                        .get(sid)
                        .map(value_to_string)
                        .unwrap_or_default()
                };
                let mut pass = if let Some(ref layer) = safety {
                    let mut ok = true;
                    let checks: Vec<String> = if node.guardrail_checks.is_empty() {
                        vec![
                            "leak".into(),
                            "injection".into(),
                            "policy".into(),
                        ]
                    } else {
                        node.guardrail_checks.clone()
                    };
                    for c in &checks {
                        let c = c.to_ascii_lowercase();
                        match c.as_str() {
                            "leak" | "pii" => {
                                if layer.scan_inbound(&text).is_err() {
                                    ok = false;
                                    break;
                                }
                            }
                            "injection" | "jailbreak" => {
                                let san = layer.sanitizer().sanitize(&text);
                                if !san.warnings.is_empty() {
                                    ok = false;
                                    break;
                                }
                            }
                            "policy"
                            | "moderation"
                            | "hallucination"
                            | "nsfw" => {
                                let violations = layer.policy().check(&text);
                                if violations.iter().any(|v| {
                                    matches!(
                                        v.action,
                                        crate::safety::policy::PolicyAction::Block
                                    )
                                }) {
                                    ok = false;
                                    break;
                                }
                            }
                            "url" | "url_filter" => {
                                if !guard_urls_allowed(&text) {
                                    ok = false;
                                    break;
                                }
                            }
                            "custom" => {
                                let needle = node.guardrail_custom_substring.trim();
                                if !needle.is_empty()
                                    && text.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
                                {
                                    ok = false;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    ok
                } else {
                    true
                };
                let mut forced = false;
                if !pass && node.guardrail_continue_on_error {
                    pass = true;
                    forced = true;
                    flow_step_log(
                        &flow_run_log,
                        format!("[guardrails] {cur} continue_on_error → treating as pass"),
                    );
                }
                let branch = if pass { "pass" } else { "fail" };
                flow_step_log(
                    &flow_run_log,
                    format!("[branch] guardrails {cur} → {branch}"),
                );
                outputs.insert(
                    cur.clone(),
                    json!({
                        "pass": pass,
                        "forced_pass": forced,
                        "text_preview": text.chars().take(500).collect::<String>()
                    }),
                );
                ordered_steps.push(json!({"id": cur, "kind": "guardrails", "branch": branch, "output": outputs.get(&cur)}));
                cursor = pick_next(node, outs, Some(branch))?;
                continue;
            }
            "mcp" => {
                let tool_id = node.mcp_tool_id.trim();
                if tool_id.is_empty() {
                    return Err(format!("node {}: mcp needs mcp_tool_id (server:tool)", node.id));
                }
                let tpl_ctx = build_template_context(inputs, &outputs);
                let args_raw = interpolate_context(&node.mcp_arguments_json, &tpl_ctx);
                let args: Value = if args_raw.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&args_raw).map_err(|e| {
                        format!("node {}: mcp_arguments_json must be JSON object: {e}", node.id)
                    })?
                };
                let mcp: Option<Arc<McpManager>> = extras.mcp.clone();
                let Some(m) = mcp else {
                    return Err(format!("node {}: MCP is not connected on this node", node.id));
                };
                let result = m
                    .call_tool(tool_id, args)
                    .await
                    .map_err(|e| format!("MCP call failed: {e}"))?;
                let text: String = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        crate::mcp::McpContent::Text { text } => Some(text.as_str()),
                        crate::mcp::McpContent::Resource { text, .. } => text.as_deref(),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let out = json!({
                    "text": text,
                    "is_error": result.is_error,
                });
                outputs.insert(cur.clone(), out.clone());
                ordered_steps.push(json!({"id": cur, "kind": "mcp", "output": out}));
                flow_step_log(
                    &flow_run_log,
                    format!(
                        "[mcp] {cur} tool={tool_id} error={}",
                        result.is_error
                    ),
                );
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "file_search" | "filesearch" => {
                let coll = node.vector_collection.trim();
                if coll.is_empty() {
                    return Err(format!("node {}: file_search needs vector_collection", node.id));
                }
                let tpl_ctx = build_template_context(inputs, &outputs);
                let q = interpolate_context(&node.vector_query_template, &tpl_ctx);
                if q.trim().is_empty() {
                    return Err(format!("node {}: file_search needs vector_query_template", node.id));
                }
                let top_k = if node.vector_top_k == 0 {
                    10
                } else {
                    (node.vector_top_k as usize).min(100)
                };
                let results = vector_store
                    .search_text(coll, &q, top_k)
                    .map_err(|e| format!("vector search: {e}"))?;
                flow_step_log(
                    &flow_run_log,
                    format!(
                        "[file_search] {cur} collection={coll} hits={}",
                        results.len()
                    ),
                );
                let items: Vec<Value> = results
                    .iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "score": r.score,
                            "text": r.text.clone().unwrap_or_default(),
                            "payload": r.payload,
                        })
                    })
                    .collect();
                let out = json!({ "results": items, "query": q });
                outputs.insert(cur.clone(), out.clone());
                ordered_steps.push(json!({"id": cur, "kind": "file_search", "output": out}));
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "set_state" | "setstate" => {
                let key = node.state_key.trim();
                if key.is_empty() {
                    return Err(format!("node {}: set_state needs state_key", node.id));
                }
                let act = activation_for_cel(inputs, &outputs, &state_map, &input_as_text, 0);
                let val: Value = if !node.state_value_cel.trim().is_empty() {
                    cel_json_value(&node.state_value_cel, &act, 0)?
                } else {
                    let tpl_ctx = build_template_context(inputs, &outputs);
                    let raw = interpolate_context(&node.state_value_json, &tpl_ctx);
                    if raw.trim().is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_str(&raw).unwrap_or(Value::String(raw))
                    }
                };
                state_map.insert(key.to_string(), val.clone());
                outputs.insert(cur.clone(), json!({ "state_key": key, "value": val }));
                ordered_steps.push(json!({"id": cur, "kind": "set_state", "output": outputs.get(&cur)}));
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "transform" => {
                let act = activation_for_cel(inputs, &outputs, &state_map, &input_as_text, 0);
                let mode = node.transform_mode.to_ascii_lowercase();
                let has_obj = !node.transform_object_json.trim().is_empty();
                let has_expr = !node.transform_expressions_json.trim().is_empty();
                let use_object = mode == "object" || (has_obj && mode != "copy" && mode != "expressions");
                let use_expr = mode == "expressions" || (has_expr && mode != "object" && !use_object);

                if use_object {
                    let tpl_ctx = build_template_context(inputs, &outputs);
                    let raw_obj =
                        interpolate_context(&node.transform_object_json.trim(), &tpl_ctx);
                    let map_val: Value = serde_json::from_str(&raw_obj).map_err(|e| {
                        format!("node {}: transform_object_json: {e}", node.id)
                    })?;
                    let obj = map_val.as_object().ok_or_else(|| {
                        format!("node {}: transform_object_json must be a JSON object", node.id)
                    })?;
                    let keys: Vec<String> = obj.keys().cloned().collect();
                    for (k, v) in obj {
                        state_map.insert(k.clone(), v.clone());
                    }
                    outputs.insert(
                        cur.clone(),
                        json!({ "mode": "object", "keys": keys }),
                    );
                } else if use_expr {
                    let rows: Vec<Value> = serde_json::from_str(&node.transform_expressions_json)
                        .map_err(|e| {
                        format!(
                            "node {}: transform_expressions_json must be a JSON array: {e}",
                            node.id
                        )
                    })?;
                    if rows.is_empty() {
                        return Err(format!(
                            "node {}: transform expressions mode needs at least one {{key, cel}} row",
                            node.id
                        ));
                    }
                    let mut applied = Vec::new();
                    for row in rows {
                        let key = row
                            .get("key")
                            .and_then(|x| x.as_str())
                            .ok_or_else(|| {
                                format!("node {}: each transform row needs string key", node.id)
                            })?
                            .to_string();
                        let cel_e = row
                            .get("cel")
                            .and_then(|x| x.as_str())
                            .ok_or_else(|| {
                                format!("node {}: each transform row needs string cel", node.id)
                            })?
                            .to_string();
                        let v = cel_json_value(&cel_e, &act, 0)?;
                        state_map.insert(key.clone(), v.clone());
                        applied.push(json!({ "key": key, "value": v }));
                    }
                    outputs.insert(
                        cur.clone(),
                        json!({ "mode": "expressions", "applied": applied }),
                    );
                } else {
                    let from_id = node.transform_from_node_id.trim();
                    let sk = node.state_key.trim();
                    if from_id.is_empty() || sk.is_empty() {
                        return Err(format!(
                            "node {}: transform (copy) needs transform_from_node_id and state_key, or use expressions/object mode",
                            node.id
                        ));
                    }
                    let v = outputs
                        .get(from_id)
                        .cloned()
                        .unwrap_or(Value::Null);
                    state_map.insert(sk.to_string(), v.clone());
                    outputs.insert(
                        cur.clone(),
                        json!({ "copied_from": from_id, "state_key": sk, "mode": "copy" }),
                    );
                }
                ordered_steps.push(json!({"id": cur, "kind": "transform", "output": outputs.get(&cur)}));
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "crew" => {
                let crew = node.crew_spec.as_ref().ok_or_else(|| {
                    format!("flow node {}: crew requires crew_spec", node.id)
                })?;
                crew.validate()?;
                let merged_inputs = if node.prompt.is_empty() {
                    inputs.clone()
                } else {
                    json!({ "flow_prompt": interpolate_inputs(&node.prompt, inputs) })
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
                outputs.insert(cur.clone(), v.clone());
                ordered_steps.push(json!({"id": cur, "kind": "crew", "output": v}));
                flow_step_log(&flow_run_log, format!("[crew] {cur} done"));
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "agent" => {
                let model = if node.model.trim().is_empty() {
                    default_model.to_string()
                } else {
                    node.model.clone()
                };
                let allowed = if node.tools.is_empty() {
                    tools.builtin_names()
                } else {
                    node.tools.clone()
                };
                let mut system = if node.instructions.trim().is_empty() {
                    "You are a helpful assistant.".to_string()
                } else {
                    node.instructions.clone()
                };
                if node.output_format.eq_ignore_ascii_case("json") {
                    system.push_str("\n\nRespond with valid JSON only. Do not wrap in markdown fences or add commentary outside the JSON.");
                }
                let config = AgentConfig {
                    id: format!("flow_{}", node.id),
                    name: if node.name.trim().is_empty() {
                        node.id.clone()
                    } else {
                        node.name.clone()
                    },
                    model,
                    max_tokens: node.max_tokens.unwrap_or(2048),
                    temperature: node.temperature.unwrap_or(0.5),
                    system_prompt: system,
                    allowed_tools: allowed,
                    context_window: 4096,
                };
                let budget = crate::agent::budget::BudgetTracker::new(50.0, 500.0, 2000.0, 10_000.0);
                let mut rt = AgentRuntime::new(
                    config,
                    executor.clone(),
                    tools.clone(),
                    budget,
                    peer_id.clone(),
                    node_tool_tx.clone(),
                    prompts.clone(),
                    inference_sink.clone(),
                );
                let ctx_block = prior_context_block(spec, &cur, &outputs);
                let user = interpolate_inputs(&node.prompt, inputs);
                let user_block = format!("{ctx_block}\n\n## Task\n{user}\n");
                let session_id_opt: Option<String> = if node.include_chat_history {
                    Some(if node.agent_session_key.trim().is_empty() {
                        format!("flow_agent_{}", node.id)
                    } else {
                        node.agent_session_key.clone()
                    })
                } else {
                    None
                };
                let res = rt
                    .run_task_with_session(
                        &user_block,
                        None,
                        session_id_opt.as_deref(),
                        extras.clone(),
                    )
                    .await;
                let out = json!({
                    "text": res.answer,
                    "iterations": res.iterations,
                    "tokens": res.total_tokens,
                    "success": res.success,
                    "error": res.error,
                });
                outputs.insert(cur.clone(), out.clone());
                ordered_steps.push(json!({"id": cur, "kind": "agent", "output": out}));
                flow_step_log(
                    &flow_run_log,
                    format!(
                        "[agent] {cur} iterations={} tokens={} success={}",
                        res.iterations, res.total_tokens, res.success
                    ),
                );
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "classify" => {
                if node.classify_categories.is_empty() {
                    return Err(format!(
                        "node {}: classify needs at least one category",
                        node.id
                    ));
                }
                let cats = node.classify_categories.join(", ");
                let tpl_ctx = build_template_context(inputs, &outputs);
                let input_block = if node.classify_input_template.trim().is_empty() {
                    input_as_text.clone()
                } else {
                    interpolate_context(&node.classify_input_template, &tpl_ctx)
                };
                let mut body = format!(
                    "You are a classifier. Possible categories (reply with exactly one label from this list): [{}].\n\nText to classify:\n{}\n\nReply with only the category label, no punctuation or explanation.",
                    cats, input_block
                );
                if !node.classify_examples_json.trim().is_empty() {
                    body.push_str("\n\nFew-shot examples (JSON):\n");
                    body.push_str(node.classify_examples_json.trim());
                }
                let model = if node.classify_model.trim().is_empty() {
                    default_model.to_string()
                } else {
                    node.classify_model.clone()
                };
                if node.output_format.eq_ignore_ascii_case("json") {
                    body.push_str("\n\nReturn JSON: {\"category\": \"<label>\"} only.");
                }
                let max_t = node.max_tokens.unwrap_or(128).max(16);
                let temp = node.temperature.unwrap_or(0.2);
                let task = InferenceTask::new(model, body)
                    .with_max_tokens(max_t)
                    .with_temperature(temp);
                let res = executor
                    .execute(ExecutionTask::Inference(task))
                    .await
                    .map_err(|e| e.to_string())?;
                let text = match res.data {
                    TaskData::Inference(r) => r.text,
                    TaskData::Error(e) => return Err(e),
                    _ => return Err("non-inference flow step".into()),
                };
                let label = text.trim().to_string();
                let j = json!({
                    "text": text,
                    "category": label,
                });
                outputs.insert(cur.clone(), j.clone());
                ordered_steps.push(json!({"id": cur, "kind": "classify", "output": j}));
                flow_step_log(
                    &flow_run_log,
                    format!("[classify] {cur} → {}", label.chars().take(80).collect::<String>()),
                );
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            "" | "llm" => {
                let prompt = interpolate_inputs(&node.prompt, inputs);
                let ctx_block = prior_context_block(spec, &cur, &outputs);
                let mut full = format!("{ctx_block}\n\n{prompt}");
                if node.output_format.eq_ignore_ascii_case("json") {
                    full.push_str("\n\nRespond with valid JSON only. No markdown fences.");
                }
                let model = if node.model.trim().is_empty() {
                    default_model.to_string()
                } else {
                    node.model.clone()
                };
                let max_t = node.max_tokens.unwrap_or(512);
                let temp = node.temperature.unwrap_or(0.7);
                let task = InferenceTask::new(model, full)
                    .with_max_tokens(max_t)
                    .with_temperature(temp);
                let res = executor
                    .execute(ExecutionTask::Inference(task))
                    .await
                    .map_err(|e| e.to_string())?;
                let text = match res.data {
                    TaskData::Inference(r) => r.text,
                    TaskData::Error(e) => return Err(e),
                    _ => return Err("non-inference flow step".into()),
                };
                let j = json!({ "text": text });
                outputs.insert(cur.clone(), j.clone());
                ordered_steps.push(json!({"id": cur, "kind": "llm", "output": j}));
                flow_step_log(
                    &flow_run_log,
                    format!("[llm] {cur} chars={}", text.chars().count()),
                );
                cursor = pick_next(node, outs, None)?;
                continue;
            }
            other => {
                return Err(format!(
                    "node {}: unknown kind '{other}' for interpreter mode",
                    node.id
                ));
            }
        }
    }

    Ok(FlowRunOutput {
        steps: ordered_steps,
    })
}

/// Static validation for interpreter mode (branch labels, single start).
pub fn validate_interpreter(spec: &FlowSpec) -> Result<(), String> {
    let starts: Vec<_> = spec
        .nodes
        .iter()
        .filter(|n| n.kind.eq_ignore_ascii_case("start"))
        .collect();
    if starts.len() != 1 {
        return Err(format!(
            "flow must have exactly one start node, found {}",
            starts.len()
        ));
    }
    let ids: HashSet<_> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
    let out = outgoing_map(spec);

    for n in &spec.nodes {
        let k = n.kind.to_ascii_lowercase();
        if matches!(k.as_str(), "note") {
            continue;
        }
        let outs = out.get(&n.id).map(|v| v.as_slice()).unwrap_or(&[]);
        match k.as_str() {
            "if" | "ifelse" => {
                let mut has_t = false;
                let mut has_f = false;
                for (_, lab) in outs {
                    match normalize_label(lab).as_deref() {
                        Some("true") => has_t = true,
                        Some("false") => has_f = true,
                        _ => {}
                    }
                }
                if !has_t || !has_f {
                    return Err(format!(
                        "node {}: if/else needs outgoing edges labeled true and false",
                        n.id
                    ));
                }
            }
            "while" => {
                let mut has_l = false;
                let mut has_e = false;
                for (_, lab) in outs {
                    match normalize_label(lab).as_deref() {
                        Some("loop") => has_l = true,
                        Some("exit") => has_e = true,
                        _ => {}
                    }
                }
                if !has_l || !has_e {
                    return Err(format!(
                        "node {}: while needs outgoing edges labeled loop and exit",
                        n.id
                    ));
                }
            }
            "guardrails" => {
                let mut has_p = false;
                let mut has_f = false;
                for (_, lab) in outs {
                    match normalize_label(lab).as_deref() {
                        Some("pass") => has_p = true,
                        Some("fail") => has_f = true,
                        _ => {}
                    }
                }
                if !has_p || !has_f {
                    return Err(format!(
                        "node {}: guardrails needs outgoing edges labeled pass and fail",
                        n.id
                    ));
                }
            }
            "human_approval" | "humanapproval" | "user_approval" | "userapproval" => {
                let mut has_a = false;
                let mut has_r = false;
                for (_, lab) in outs {
                    match normalize_label(lab).as_deref() {
                        Some("approve") => has_a = true,
                        Some("reject") => has_r = true,
                        _ => {}
                    }
                }
                if !has_a || !has_r {
                    return Err(format!(
                        "node {}: user approval needs outgoing edges labeled approve and reject",
                        n.id
                    ));
                }
            }
            "end" => {
                if !outs.is_empty() {
                    return Err(format!("node {}: end node must have no outgoing edges", n.id));
                }
            }
            _ => {
                // allow 0 (implicit stop) or 1 successor; multiple requires default label
                if outs.len() > 1 {
                    let defaults: Vec<_> = outs
                        .iter()
                        .filter(|(_, l)| {
                            normalize_label(l).as_deref() == Some("default")
                        })
                        .collect();
                    let unlabeled: Vec<_> = outs
                        .iter()
                        .filter(|(_, l)| {
                            l.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true)
                        })
                        .collect();
                    if defaults.len() != 1 && unlabeled.len() != 1 {
                        return Err(format!(
                            "node {}: multiple outgoing edges need exactly one default label or one unlabeled edge",
                            n.id
                        ));
                    }
                }
            }
        }
        if k == "guardrails" {
            if n.guardrail_input_template.trim().is_empty() {
                let sid = n.source_node_id.trim();
                if sid.is_empty() {
                    return Err(format!(
                        "node {}: guardrails needs source_node_id or guardrail_input_template",
                        n.id
                    ));
                }
                if !ids.contains(sid) {
                    return Err(format!(
                        "node {}: guardrails source_node_id '{}' not found",
                        n.id, sid
                    ));
                }
            } else if !n.source_node_id.trim().is_empty()
                && !ids.contains(n.source_node_id.trim())
            {
                return Err(format!(
                    "node {}: guardrails source_node_id '{}' not found",
                    n.id,
                    n.source_node_id.trim()
                ));
            }
        }
        if k == "transform" {
            let mode = n.transform_mode.to_ascii_lowercase();
            let copy_mode = mode == "copy"
                || (mode.is_empty()
                    && n.transform_expressions_json.trim().is_empty()
                    && n.transform_object_json.trim().is_empty());
            if copy_mode {
                let fid = n.transform_from_node_id.trim();
                if !fid.is_empty() && !ids.contains(fid) {
                    return Err(format!(
                        "node {}: transform_from_node_id '{}' not found",
                        n.id, fid
                    ));
                }
            }
        }
    }

    // Reachability from start
    let start_id = starts[0].id.clone();
    let so = out
        .get(&start_id)
        .map(|v| v.len())
        .unwrap_or(0);
    if so != 1 {
        return Err(format!(
            "start node must have exactly one outgoing edge, found {so}"
        ));
    }

    let mut seen = HashSet::new();
    let mut stack = vec![start_id.clone()];
    while let Some(x) = stack.pop() {
        if !seen.insert(x.clone()) {
            continue;
        }
        if let Some(outs) = out.get(&x) {
            for (to, _) in outs {
                stack.push(to.clone());
            }
        }
    }
    for n in &spec.nodes {
        let k = n.kind.to_ascii_lowercase();
        if matches!(k.as_str(), "note") {
            continue;
        }
        if !seen.contains(&n.id) {
            return Err(format!("node {} is not reachable from start", n.id));
        }
    }

    Ok(())
}
