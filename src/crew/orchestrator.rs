//! Run a crew: sequential or lightweight hierarchical planning pass.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agent::budget::BudgetTracker;
use crate::agent::runtime::{AgentConfig, AgentRuntime, AgentTaskExtras};
use crate::executor::TaskExecutor;
use crate::tools::{NodeToolTx, ToolRegistry};

use super::spec::{CrewProcess, CrewSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewTaskOutput {
    pub task_id: String,
    pub agent_id: String,
    pub answer: String,
    pub iterations: u32,
    pub tokens: u32,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewOutput {
    pub raw: String,
    pub tasks_output: Vec<CrewTaskOutput>,
    pub token_usage: serde_json::Value,
}

fn interpolate_template(s: &str, inputs: &serde_json::Value) -> String {
    let mut out = s.to_string();
    if let Some(obj) = inputs.as_object() {
        for (k, v) in obj {
            let pat = format!("{{{k}}}");
            let rep = match v {
                serde_json::Value::String(x) => x.clone(),
                _ => v.to_string(),
            };
            out = out.replace(&pat, &rep);
        }
    }
    out
}

fn allowed_tools_for_agent(
    def: &super::spec::CrewAgentDef,
    registry: &ToolRegistry,
) -> Vec<String> {
    if def.tools.is_empty() {
        registry.builtin_names()
    } else {
        def.tools.clone()
    }
}

fn make_runtime_for_agent(
    def: &super::spec::CrewAgentDef,
    executor: Arc<TaskExecutor>,
    tools: Arc<ToolRegistry>,
    peer_id: String,
    node_tool_tx: Option<NodeToolTx>,
    prompts: Arc<crate::prompts::PromptBundle>,
    inference_sink: Option<Arc<dyn crate::agent::AgenticInferenceSink>>,
) -> AgentRuntime {
    let system_prompt = prompts.crew_agent_system(&def.role, &def.goal, &def.backstory);
    let allowed = allowed_tools_for_agent(def, tools.as_ref());
    let max_iter = if def.max_iter == 0 { 20 } else { def.max_iter };
    let config = AgentConfig {
        id: format!("crew_{}", def.id),
        name: def.id.clone(),
        model: def.llm.clone(),
        max_tokens: 2048,
        temperature: 0.5,
        system_prompt,
        allowed_tools: allowed,
        context_window: 4096,
    };
    let budget = BudgetTracker::new(50.0, 500.0, 2000.0, 10_000.0);
    let rt = AgentRuntime::new(
        config,
        executor,
        tools,
        budget,
        peer_id,
        node_tool_tx,
        prompts,
        inference_sink,
    );
    // Respect per-agent iteration cap inside unified loop via config if needed — runtime uses AGENTIC_MAX_ITERS global; we clip via early description only for now.
    let _ = max_iter;
    rt
}

/// Execute crew tasks in order; optional hierarchical planning preamble.
pub async fn run_crew(
    spec: &CrewSpec,
    inputs: &serde_json::Value,
    executor: Arc<TaskExecutor>,
    tools: Arc<ToolRegistry>,
    peer_id: String,
    node_tool_tx: Option<NodeToolTx>,
    inference_sink: Option<Arc<dyn crate::agent::AgenticInferenceSink>>,
    prompts: Arc<crate::prompts::PromptBundle>,
    cancel: Option<&AtomicBool>,
    extras: AgentTaskExtras,
) -> Result<CrewOutput, String> {
    spec.validate()?;

    let mut plan_preamble = String::new();
    if spec.process == CrewProcess::Hierarchical {
        let mgr_id = spec
            .manager_agent_id
            .clone()
            .or_else(|| spec.agents.first().map(|a| a.id.clone()))
            .ok_or_else(|| {
                "hierarchical crew needs manager_agent_id or at least one agent".to_string()
            })?;
        let mgr = spec
            .agents
            .iter()
            .find(|a| a.id == mgr_id)
            .ok_or_else(|| "manager agent not found".to_string())?;
        let task_list: Vec<_> = spec.tasks.iter().map(|t| t.id.clone()).collect();
        let mut mgr_rt = make_runtime_for_agent(
            mgr,
            executor.clone(),
            tools.clone(),
            peer_id.clone(),
            node_tool_tx.clone(),
            prompts.clone(),
            inference_sink.clone(),
        );
        let plan_prompt = format!(
            "You coordinate this crew. Tasks (in order): {:?}.\nInputs: {}\nReply with a short bullet plan (max 8 lines) for how the crew should execute.",
            task_list,
            inputs
        );
        let plan_res = mgr_rt
            .run_task_with_session(&plan_prompt, cancel, None, extras.clone())
            .await;
        plan_preamble = format!("## Manager plan\n{}\n\n", plan_res.answer);
    }

    let mut outputs: HashMap<String, String> = HashMap::new();
    let mut tasks_output: Vec<CrewTaskOutput> = Vec::new();
    let mut total_tokens = 0u32;

    for task in &spec.tasks {
        let agent = spec
            .agents
            .iter()
            .find(|a| a.id == task.agent_id)
            .ok_or_else(|| format!("agent {} missing", task.agent_id))?;

        let mut ctx = String::new();
        if !plan_preamble.is_empty() {
            ctx.push_str(&plan_preamble);
        }
        for cid in &task.context {
            if let Some(prev) = outputs.get(cid) {
                ctx.push_str(&format!("## Prior task {cid}\n{prev}\n\n"));
            }
        }

        let desc = interpolate_template(&task.description, inputs);
        let exp = interpolate_template(&task.expected_output, inputs);
        let user_block = format!(
            "{ctx}## Current task\n{desc}\n\nExpected output: {exp}\nProduce the final deliverable clearly."
        );

        let mut rt = make_runtime_for_agent(
            agent,
            executor.clone(),
            tools.clone(),
            peer_id.clone(),
            node_tool_tx.clone(),
            prompts.clone(),
            inference_sink.clone(),
        );
        let res = rt
            .run_task_with_session(&user_block, cancel, None, extras.clone())
            .await;
        total_tokens += res.total_tokens;
        outputs.insert(task.id.clone(), res.answer.clone());
        tasks_output.push(CrewTaskOutput {
            task_id: task.id.clone(),
            agent_id: task.agent_id.clone(),
            answer: res.answer.clone(),
            iterations: res.iterations,
            tokens: res.total_tokens,
            success: res.success,
            error: res.error.clone(),
        });
    }

    let raw = outputs
        .get(spec.tasks.last().map(|t| t.id.as_str()).unwrap_or(""))
        .cloned()
        .unwrap_or_default();

    Ok(CrewOutput {
        raw,
        tasks_output,
        token_usage: json!({ "total_tokens": total_tokens }),
    })
}
