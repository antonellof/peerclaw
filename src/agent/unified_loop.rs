//! Shared ReAct loop for dashboard chat, background tasks, and the TOML agent runtime.
//!
//! Uses the same inference channel as the web UI (`InferenceTask` via `peerclaw serve` loop) and
//! dispatches `<tool_call>` blocks to local [`ToolRegistry`] tools and optional MCP (`server:tool`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use crate::mcp::McpManager;
use crate::tools::{NodeToolTx, ToolContext, ToolLocation, ToolRegistry};

use super::compaction;
use super::runtime::{extract_answer, parse_tool_calls, ToolCallRecord};

/// Upper bound on LLM↔tool rounds.
pub const AGENTIC_MAX_ITERS: u32 = 20;
/// Cap parallel tool calls per model response.
pub const AGENTIC_MAX_TOOL_CALLS_PER_PASS: usize = 10;

/// One inference turn result (mirrors web `InferenceResponse` fields).
#[derive(Clone, Debug)]
pub struct AgenticTurnOutcome {
    pub text: String,
    pub tokens_generated: u32,
    pub tokens_per_second: f32,
    pub location: String,
    pub provider_peer_id: Option<String>,
}

/// Async inference backend (e.g. web `mpsc` queue to the node run loop).
#[async_trait]
pub trait AgenticInferenceSink: Send + Sync {
    async fn infer(
        &self,
        prompt: String,
        model: String,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<AgenticTurnOutcome, String>;
}

/// Optional live progress (web task logs, etc.).
#[async_trait]
pub trait AgenticProgressSink: Send + Sync {
    async fn set_react_pass(&self, pass: u32);
    async fn append_log(&self, line: String);
    async fn set_tokens(&self, tokens: u32);
    async fn record_tool_step(&self, line: String, tokens: u32);
    /// Structured tool call event (started / completed).
    async fn record_tool_call(
        &self,
        _tool_name: &str,
        _status: &str,
        _args: &str,
        _result: &str,
    ) {
        // default no-op
    }
}

/// No-op progress implementation.
pub struct NoAgenticProgress;

#[async_trait]
impl AgenticProgressSink for NoAgenticProgress {
    async fn set_react_pass(&self, _pass: u32) {}

    async fn append_log(&self, _line: String) {}

    async fn set_tokens(&self, _tokens: u32) {}

    async fn record_tool_step(&self, _line: String, _tokens: u32) {}
}

/// Build the tool + MCP system prefix (OpenClaw-style concise instructions).
pub async fn build_agentic_system_prefix(
    prompts: &crate::prompts::PromptBundle,
    registry: Option<&ToolRegistry>,
    mcp: Option<&McpManager>,
    include_mcp_catalog: bool,
    allowed_local_tools: Option<&[String]>,
) -> String {
    let mut s = prompts.agentic_system_intro.clone();
    if let Some(registry) = registry {
        s.push_str(&prompts.agentic_tools_header);
        if !s.ends_with('\n') {
            s.push('\n');
        }
        let mut infos = registry.list_tools().await;
        infos.retain(|t| matches!(t.location, ToolLocation::Local));
        if let Some(allowed) = allowed_local_tools {
            if !allowed.is_empty() {
                let allow: HashSet<_> = allowed.iter().cloned().collect();
                infos.retain(|t| allow.contains(&t.name));
            }
        }
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        for t in &infos {
            let desc: String = t.description.chars().take(200).collect();
            s.push_str(&format!("- {}: {}\n", t.name, desc));
        }
    } else {
        s.push_str(&prompts.agentic_mcp_only_intro);
        if !s.ends_with('\n') {
            s.push('\n');
        }
    }
    if include_mcp_catalog {
        if let Some(manager) = mcp {
            if manager.tool_count() > 0 {
                s.push_str(&prompts.agentic_mcp_tools_header);
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                let mut entries = manager.list_tools_with_ids();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                for (id, tool) in entries {
                    let desc: String = tool
                        .description
                        .as_deref()
                        .unwrap_or("(no description)")
                        .chars()
                        .take(80)
                        .collect();
                    s.push_str(&format!("- {}: {}\n", id, desc));
                }
            }
        }
    }
    s
}

/// Whether the agent could call at least one tool (local registry and/or MCP).
async fn agentic_action_tools_available(
    registry: Option<&ToolRegistry>,
    mcp: Option<&McpManager>,
    allowed_local_tools: Option<&[String]>,
) -> bool {
    if mcp.is_some_and(|m| m.tool_count() > 0) {
        return true;
    }
    let Some(reg) = registry else {
        return false;
    };
    let mut infos = reg.list_tools().await;
    infos.retain(|t| matches!(t.location, ToolLocation::Local));
    if let Some(allowed) = allowed_local_tools {
        if !allowed.is_empty() {
            let allow: HashSet<_> = allowed.iter().cloned().collect();
            infos.retain(|t| allow.contains(&t.name));
        }
    }
    !infos.is_empty()
}

/// Print tool invocation and raw result to stderr when `peerclaw serve --verbose-agentic` is on.
fn eprint_verbose_tool_io(tool: &str, args: &serde_json::Value, ok: bool, result: &str) {
    const MAX_ARGS: usize = 12_000;
    const MAX_RESULT: usize = 200_000;
    eprintln!("\n======== PEERCLAW TOOL_CALL tool={tool} ok={ok} ========");
    let args_s = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
    let (a_out, a_note) = clip_utf8_prefix(&args_s, MAX_ARGS);
    eprintln!("args:\n{a_out}{a_note}");
    let (r_out, r_note) = clip_utf8_prefix(result, MAX_RESULT);
    eprintln!("result:\n{r_out}{r_note}");
    eprintln!("======== END TOOL_CALL ========\n");
}

fn clip_utf8_prefix(s: &str, max_chars: usize) -> (String, String) {
    let n = s.chars().count();
    if n <= max_chars {
        (s.to_string(), String::new())
    } else {
        let head: String = s.chars().take(max_chars).collect();
        (head, format!("\n… [stderr truncated, {n} chars total]\n"))
    }
}

/// Model promised work or research but did not deliver structured content (no tool calls in this turn).
fn response_looks_like_intent_without_delivery(response: &str) -> bool {
    let t = response.trim();
    let n = t.chars().count();
    if n > 2800 {
        return false;
    }
    if (t.contains("(1)") && t.contains("(2)")) || t.matches("##").count() >= 2 {
        return false;
    }
    let lower = t.to_lowercase();
    let preamble = lower.contains("i'll ")
        || lower.contains("i will ")
        || lower.contains("let me ")
        || lower.contains("i'm going to ")
        || lower.contains("i am going to ")
        || lower.contains("gathering information")
        || lower.contains("authoritative sources")
        || lower.contains("i'll start")
        || lower.contains("i'll begin");
    preamble && n < 1400
}

/// Goal / user body (after the system prefix) looks like it needs a live web fetch/search, but no tools have run yet.
/// Ignores the prefix so instructions that mention `www.` do not false-trigger nudges.
fn goal_suggests_network_fetch(conversation: &str, after_prefix: usize) -> bool {
    let tail = conversation.get(after_prefix..).unwrap_or("");
    let lower = tail.to_lowercase();
    let has_url_token = lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("www.")
        || lower.contains("url:");
    let fetch_phrase = lower.contains("fetch and summarize")
        || lower.contains("fetch this")
        || lower.contains("summarize this url")
        || lower.contains("this page")
        || (lower.contains("summarize") && has_url_token);
    has_url_token || fetch_phrase
}

/// Run the unified ReAct loop. Returns final outcome (accumulated token count), tool log lines, and structured tool records.
#[allow(clippy::too_many_arguments)]
pub async fn run_unified_agentic_loop(
    sink: &dyn AgenticInferenceSink,
    prompts: &crate::prompts::PromptBundle,
    registry: Option<Arc<ToolRegistry>>,
    mcp: Option<Arc<McpManager>>,
    include_mcp_catalog: bool,
    allowed_local_tools: Option<&[String]>,
    conversation_body: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
    model_ctx_chars: usize,
    local_peer_id: String,
    node_tool_tx: Option<NodeToolTx>,
    progress: Option<Arc<dyn AgenticProgressSink>>,
    cancel: Option<&AtomicBool>,
    verbose_tool_io: bool,
) -> Result<(AgenticTurnOutcome, Vec<String>, Vec<ToolCallRecord>, u32), String> {
    let max_tokens = max_tokens.min(16384);
    let prefix = build_agentic_system_prefix(
        prompts,
        registry.as_deref(),
        mcp.as_deref(),
        include_mcp_catalog,
        allowed_local_tools,
    )
    .await;
    let mut conversation = format!("{prefix}\n\n{conversation_body}");
    let prefix_len = prefix.len();
    let mut tool_logs: Vec<String> = Vec::new();
    let mut tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut total_tokens: u32 = 0;
    let tool_session = uuid::Uuid::new_v4().to_string();

    let output_budget_chars = (max_tokens as usize) * 4;
    let prompt_budget = model_ctx_chars.saturating_sub(output_budget_chars + 200);
    let conv_max_chars = prompt_budget.max(2_000);

    const MAX_CONSECUTIVE_FAIL_PASSES: u32 = 2;
    let mut consecutive_all_fail_passes: u32 = 0;
    let mut tool_call_history: Vec<(String, u64)> = Vec::new();
    // Nudge when the model promises tools/work but emits no `<tool_call>` (common with some cloud models).
    const MAX_TOOL_PREAMBLE_NUDGES: u32 = 2;
    let mut tool_preamble_nudges: u32 = 0;
    /// When the goal names a URL or asks to fetch a page but the model answered at length without any tool calls.
    const MAX_URL_FETCH_NUDGES: u32 = 2;
    let mut url_fetch_nudges: u32 = 0;

    for iter in 1..=AGENTIC_MAX_ITERS {
        if conversation.len() > conv_max_chars {
            conversation =
                compaction::prune_string_conversation(&conversation, prefix_len, conv_max_chars);
        }

        if iter == AGENTIC_MAX_ITERS {
            conversation.push_str("\n\n");
            conversation.push_str(prompts.unified_final_turn_suffix.trim());
            conversation.push('\n');
        }
        if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err("Stopped by user".into());
        }
        if let Some(ref p) = progress {
            p.set_react_pass(iter).await;
            p.append_log(format!(
                "[{}] Pass {}/{}: requesting model…",
                chrono::Utc::now().format("%H:%M:%S"),
                iter,
                AGENTIC_MAX_ITERS
            ))
            .await;
        }

        let mut prompt_for_model = conversation.clone();
        if iter == 1 {
            prompt_for_model.push_str(&prompts.unified_assistant_turn_marker);
        }

        let inf = sink
            .infer(prompt_for_model, model.clone(), max_tokens, temperature)
            .await?;
        total_tokens = total_tokens.saturating_add(inf.tokens_generated);

        if let Some(ref p) = progress {
            p.set_tokens(total_tokens).await;
        }

        let text = inf.text;

        if inf.location == "error"
            || text.starts_with("Error: Inference error:")
            || text.starts_with("Error: ")
        {
            if let Some(ref p) = progress {
                p.append_log(format!(
                    "[{}] Inference error on pass {}: {} — compacting and retrying",
                    chrono::Utc::now().format("%H:%M:%S"),
                    iter,
                    text.chars().take(120).collect::<String>()
                ))
                .await;
            }
            let goal_start = conversation
                .find("Goal:")
                .or_else(|| conversation.find("### Agent goal"))
                .or_else(|| conversation.find("### Task"))
                .or_else(|| conversation.find("### User thread"))
                .unwrap_or(prefix_len);
            let goal_end = conversation[goal_start..]
                .find("\n\nAssistant:")
                .map(|i| goal_start + i)
                .unwrap_or(conversation.len().min(goal_start + 2000));
            let goal_section = conversation[goal_start..goal_end].to_string();
            conversation = format!(
                "{}\n\n{}\n\n{}\n",
                &conversation[..prefix_len],
                goal_section,
                prompts.unified_compact_retry_suffix.trim(),
            );
            continue;
        }

        let mut calls = parse_tool_calls(&text);
        if calls.is_empty() {
            if url_fetch_nudges < MAX_URL_FETCH_NUDGES
                && iter < AGENTIC_MAX_ITERS
                && tool_logs.is_empty()
                && tool_records.is_empty()
                && goal_suggests_network_fetch(&conversation, prefix_len)
                && agentic_action_tools_available(
                    registry.as_deref(),
                    mcp.as_deref(),
                    allowed_local_tools,
                )
                .await
            {
                url_fetch_nudges += 1;
                conversation.push_str("\n\nAssistant:\n");
                conversation.push_str(text.trim());
                conversation.push_str("\n\n");
                conversation.push_str(prompts.unified_url_fetch_nudge.trim());
                conversation.push('\n');
                if let Some(ref p) = progress {
                    p.append_log(format!(
                        "[{}] Pass {}: no tools run yet for URL/fetch-style goal; nudging model to call web_fetch/web_search…",
                        chrono::Utc::now().format("%H:%M:%S"),
                        iter
                    ))
                    .await;
                }
                continue;
            }

            if tool_preamble_nudges < MAX_TOOL_PREAMBLE_NUDGES
                && iter < AGENTIC_MAX_ITERS
                && agentic_action_tools_available(
                    registry.as_deref(),
                    mcp.as_deref(),
                    allowed_local_tools,
                )
                .await
                && response_looks_like_intent_without_delivery(&text)
            {
                tool_preamble_nudges += 1;
                conversation.push_str("\n\nAssistant:\n");
                conversation.push_str(text.trim());
                conversation.push_str("\n\n");
                conversation.push_str(prompts.unified_preamble_nudge.trim());
                conversation.push('\n');
                if let Some(ref p) = progress {
                    p.append_log(format!(
                        "[{}] Pass {}: model replied without tools (preamble-style); nudging to continue…",
                        chrono::Utc::now().format("%H:%M:%S"),
                        iter
                    ))
                    .await;
                }
                continue;
            }

            let trimmed_raw = text.trim();
            let cleaned = extract_answer(&text);
            let text_out = if !cleaned.trim().is_empty() {
                cleaned
            } else if !trimmed_raw.is_empty() {
                trimmed_raw.to_string()
            } else if !tool_records.is_empty() {
                // Model returned only tool markup with no prose — synthesize from last tool result.
                let last = &tool_records[tool_records.len() - 1];
                let preview: String = last.result.chars().take(4000).collect();
                if preview.trim().is_empty() {
                    format!("Completed {} tool call(s). No additional commentary from model.", tool_records.len())
                } else {
                    preview
                }
            } else {
                prompts.unified_empty_final_reply.trim().to_string()
            };
            return Ok((
                AgenticTurnOutcome {
                    text: text_out,
                    tokens_generated: total_tokens,
                    tokens_per_second: inf.tokens_per_second,
                    location: inf.location.clone(),
                    provider_peer_id: inf.provider_peer_id.clone(),
                },
                tool_logs,
                tool_records,
                iter,
            ));
        }

        {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut filtered_calls = Vec::new();
            let mut loop_warnings: Vec<String> = Vec::new();
            for call in calls {
                let mut hasher = DefaultHasher::new();
                call.args.to_string().hash(&mut hasher);
                let args_hash = hasher.finish();
                let sig = (call.name.clone(), args_hash);
                let repeat_count = tool_call_history.iter().filter(|s| **s == sig).count();
                if repeat_count >= 4 {
                    if let Some(ref p) = progress {
                        p.append_log(format!(
                            "[{}] Pass {}: BLOCKED repeated call to '{}' ({} prior identical calls)",
                            chrono::Utc::now().format("%H:%M:%S"),
                            iter,
                            call.name,
                            repeat_count + 1,
                        ))
                        .await;
                    }
                    continue;
                } else if repeat_count >= 2 {
                    loop_warnings.push(
                        prompts.unified_repeat_warning(&call.name, (repeat_count + 1) as u32),
                    );
                }
                tool_call_history.push(sig);
                filtered_calls.push(call);
            }
            calls = filtered_calls;
            if !loop_warnings.is_empty() {
                conversation.push('\n');
                for w in &loop_warnings {
                    conversation.push_str(w);
                    conversation.push('\n');
                }
            }
            if calls.is_empty() {
                let trimmed_raw = text.trim();
                let cleaned = extract_answer(&text);
                let text_out = if !cleaned.trim().is_empty() {
                    cleaned
                } else if !trimmed_raw.is_empty() {
                    trimmed_raw.to_string()
                } else {
                    prompts.unified_all_calls_blocked.trim().to_string()
                };
                return Ok((
                    AgenticTurnOutcome {
                        text: text_out,
                        tokens_generated: total_tokens,
                        tokens_per_second: inf.tokens_per_second,
                        location: inf.location.clone(),
                        provider_peer_id: inf.provider_peer_id.clone(),
                    },
                    tool_logs,
                    tool_records,
                    iter,
                ));
            }
        }

        let model_tool_call_count = calls.len();
        let mut seen_sig: HashSet<(String, String)> = HashSet::new();
        calls.retain(|call| {
            let sig = (call.name.clone(), call.args.to_string());
            seen_sig.insert(sig)
        });
        let duplicate_calls_merged = model_tool_call_count.saturating_sub(calls.len());

        if let Some(ref p) = progress {
            let mut msg = format!(
                "[{}] Pass {}: {} tool call(s)",
                chrono::Utc::now().format("%H:%M:%S"),
                iter,
                model_tool_call_count
            );
            if duplicate_calls_merged > 0 {
                msg.push_str(&format!(
                    " → {} unique (merged {} duplicate(s))",
                    calls.len(),
                    duplicate_calls_merged
                ));
            }
            p.append_log(msg).await;
        }

        let dropped_calls = if calls.len() > AGENTIC_MAX_TOOL_CALLS_PER_PASS {
            let n = calls.len() - AGENTIC_MAX_TOOL_CALLS_PER_PASS;
            calls.truncate(AGENTIC_MAX_TOOL_CALLS_PER_PASS);
            if let Some(ref p) = progress {
                p.append_log(format!(
                    "[{}] Pass {}: executing first {} of {} tool call(s) (max {} per turn)",
                    chrono::Utc::now().format("%H:%M:%S"),
                    iter,
                    AGENTIC_MAX_TOOL_CALLS_PER_PASS,
                    AGENTIC_MAX_TOOL_CALLS_PER_PASS + n,
                    AGENTIC_MAX_TOOL_CALLS_PER_PASS
                ))
                .await;
            }
            Some(n)
        } else {
            None
        };

        conversation.push_str("\n\nAssistant:\n");
        conversation.push_str(&text);
        conversation.push_str(&prompts.unified_conversation_user_label);
        if duplicate_calls_merged > 0 {
            conversation.push_str(&prompts.unified_merge_duplicates(duplicate_calls_merged));
            conversation.push('\n');
        }
        if let Some(d) = dropped_calls {
            conversation
                .push_str(&prompts.unified_skipped_calls(d, AGENTIC_MAX_TOOL_CALLS_PER_PASS));
            conversation.push('\n');
        }
        conversation.push_str(&prompts.unified_tool_results_header);
        if !prompts.unified_tool_results_header.ends_with('\n') {
            conversation.push('\n');
        }
        let mut pass_failures = 0u32;
        let call_count = calls.len();

        for call in calls {
            // Emit structured "started" event.
            if let Some(ref p) = progress {
                let args_preview: String = call.args.to_string().chars().take(500).collect();
                p.record_tool_call(&call.name, "started", &args_preview, "")
                    .await;
            }
            let start = std::time::Instant::now();
            let (summary, success) = if call.name.contains(':') {
                match &mcp {
                    Some(m) => {
                        let res = m.call_tool(&call.name, call.args.clone()).await;
                        match res {
                            Ok(r) => (
                                serde_json::to_string(&r)
                                    .unwrap_or_else(|_| "(unserializable result)".into()),
                                true,
                            ),
                            Err(e) => {
                                pass_failures += 1;
                                (format!("ERROR: {e}"), false)
                            }
                        }
                    }
                    None => {
                        pass_failures += 1;
                        (
                            "ERROR: MCP tool requested but MCP is not enabled or has no connected tools"
                                .to_string(),
                            false,
                        )
                    }
                }
            } else {
                let allowed_ok = allowed_local_tools
                    .map_or(true, |a| a.is_empty() || a.iter().any(|n| n == &call.name));
                if !allowed_ok {
                    pass_failures += 1;
                    let msg = format!(
                        "ERROR: Tool '{}' is not in the allowed tools list",
                        call.name
                    );
                    tool_records.push(ToolCallRecord {
                        tool_name: call.name.clone(),
                        args: call.args.clone(),
                        result: msg.clone(),
                        success: false,
                        duration_ms: 0,
                    });
                    let preview = msg.chars().take(220).collect::<String>();
                    let line = format!(
                        "[{}] {} → {}",
                        chrono::Utc::now().format("%H:%M:%S"),
                        call.name,
                        preview
                    );
                    tool_logs.push(line.clone());
                    if let Some(ref p) = progress {
                        p.record_tool_step(line, total_tokens).await;
                    }
                    conversation.push_str(&format!("Tool: {}\nResult: {}\n\n", call.name, msg));
                    if verbose_tool_io {
                        eprint_verbose_tool_io(&call.name, &call.args, false, &msg);
                    }
                    continue;
                }

                match &registry {
                    Some(reg) => {
                        let ctx = ToolContext {
                            session_id: tool_session.clone(),
                            job_id: None,
                            peer_id: local_peer_id.clone(),
                            working_dir: std::env::current_dir().unwrap_or_default(),
                            sandboxed: false,
                            available_secrets: vec![],
                            node_tool_tx: node_tool_tx.clone(),
                            egress_policy: None,
                            agent_depth: 0,
                        };
                        match reg.execute_local(&call.name, call.args.clone(), &ctx).await {
                            Ok(r) => {
                                let s = serde_json::to_string(&r.output)
                                    .unwrap_or_else(|_| "(unserializable)".into());
                                (s, true)
                            }
                            Err(e) => {
                                pass_failures += 1;
                                (
                                    serde_json::json!({ "error": e.to_string() }).to_string(),
                                    false,
                                )
                            }
                        }
                    }
                    None => {
                        pass_failures += 1;
                        (
                            prompts.unified_mcp_only_tool_error.trim().to_string(),
                            false,
                        )
                    }
                }
            };

            if verbose_tool_io {
                eprint_verbose_tool_io(&call.name, &call.args, success, &summary);
            }

            let duration_ms = start.elapsed().as_millis() as u64;
            tool_records.push(ToolCallRecord {
                tool_name: call.name.clone(),
                args: call.args.clone(),
                result: summary.clone(),
                success,
                duration_ms,
            });

            let preview = if summary.chars().count() > 220 {
                let short: String = summary.chars().take(217).collect();
                format!("{short}…")
            } else {
                summary.clone()
            };
            let line = format!(
                "[{}] {} → {}",
                chrono::Utc::now().format("%H:%M:%S"),
                call.name,
                preview
            );
            tool_logs.push(line.clone());
            if let Some(ref p) = progress {
                p.record_tool_step(line, total_tokens).await;
                let result_preview: String = summary.chars().take(500).collect();
                let args_preview: String = call.args.to_string().chars().take(500).collect();
                p.record_tool_call(
                    &call.name,
                    "completed",
                    &args_preview,
                    &result_preview,
                )
                .await;
            }
            let truncated = if summary.len() > 3000 {
                format!("{}… (truncated)", &summary[..3000])
            } else {
                summary
            };
            conversation.push_str(&format!("Tool: {}\nResult: {}\n\n", call.name, truncated));
        }

        if pass_failures < call_count as u32 {
            conversation.push_str(&prompts.unified_tool_results_followup);
            if !prompts.unified_tool_results_followup.ends_with('\n') {
                conversation.push('\n');
            }
        }

        if pass_failures as usize >= call_count {
            consecutive_all_fail_passes += 1;
            if consecutive_all_fail_passes >= MAX_CONSECUTIVE_FAIL_PASSES {
                conversation.push('\n');
                conversation.push_str(prompts.unified_all_tools_failed_suffix.trim());
                conversation.push('\n');
                if let Some(ref p) = progress {
                    p.append_log(format!(
                        "[{}] {} consecutive all-fail passes — forcing answer from knowledge",
                        chrono::Utc::now().format("%H:%M:%S"),
                        MAX_CONSECUTIVE_FAIL_PASSES
                    ))
                    .await;
                }
            }
        } else {
            consecutive_all_fail_passes = 0;
        }
    }

    Err(prompts.unified_max_iters_error.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;

    struct FinalAnswerSink {
        text: String,
    }

    #[async_trait::async_trait]
    impl AgenticInferenceSink for FinalAnswerSink {
        async fn infer(
            &self,
            _prompt: String,
            _model: String,
            _max_tokens: u32,
            _temperature: f32,
        ) -> Result<AgenticTurnOutcome, String> {
            Ok(AgenticTurnOutcome {
                text: self.text.clone(),
                tokens_generated: 3,
                tokens_per_second: 1.0,
                location: "Local".to_string(),
                provider_peer_id: Some("local".into()),
            })
        }
    }

    #[tokio::test]
    async fn unified_loop_ends_on_plain_answer() {
        let prompts = crate::prompts::PromptBundle::load(None);
        let sink = FinalAnswerSink {
            text: "Hello, no tools.".to_string(),
        };
        let reg = Arc::new(ToolRegistry::new("test_peer".into()));
        let (out, logs, records, passes) = run_unified_agentic_loop(
            &sink,
            &prompts,
            Some(reg),
            None,
            false,
            None,
            "### Task\nping\n".to_string(),
            "m".into(),
            64,
            0.0,
            8000,
            "test_peer".into(),
            None,
            None,
            None,
            false,
        )
        .await
        .expect("loop");
        assert_eq!(out.text, "Hello, no tools.");
        assert!(logs.is_empty());
        assert!(records.is_empty());
        assert_eq!(passes, 1);
    }

    #[tokio::test]
    async fn build_prefix_respects_allowed_local_tools() {
        let prompts = crate::prompts::PromptBundle::load(None);
        let reg = ToolRegistry::new("p".into());
        let allowed = vec!["web_search".to_string()];
        let prefix =
            build_agentic_system_prefix(&prompts, Some(&reg), None, false, Some(&allowed)).await;
        let tools_block = prefix.split("Available tools:").nth(1).unwrap_or("");
        assert!(tools_block.contains("web_search"));
        assert!(!tools_block.contains("web_fetch"));
    }

    #[test]
    fn intent_without_delivery_detects_research_preamble() {
        let s = "I'll research AI agents for you by gathering information from authoritative sources and structuring it according to your requirements.";
        assert!(response_looks_like_intent_without_delivery(s));
    }

    #[test]
    fn intent_without_delivery_skips_plain_final() {
        assert!(!response_looks_like_intent_without_delivery(
            "Hello, no tools."
        ));
        assert!(!response_looks_like_intent_without_delivery(
            "(1) Overview\n\nFoo.\n\n(2) Terms\n\nBar."
        ));
    }

    #[test]
    fn goal_fetch_detects_bracketed_www_url() {
        let g = "Goal: Fetch [www.google.com/news] and summarize.";
        assert!(goal_suggests_network_fetch(g, 0));
    }

    #[test]
    fn goal_fetch_detects_https() {
        assert!(goal_suggests_network_fetch(
            "Goal: read https://example.com/page for facts",
            0
        ));
    }
}
