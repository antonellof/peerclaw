//! Concrete prompt strings; embedded from `prompts/*.txt`, optional disk overlay.

use std::fs;
use std::path::Path;

use super::subst;

macro_rules! embed {
    ($file:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/prompts/", $file))
    };
}

fn read_overlay(dir: &Path, stem: &str, embedded: &str) -> String {
    let p = dir.join(format!("{stem}.txt"));
    match fs::read_to_string(&p) {
        Ok(s) if !s.trim().is_empty() => {
            tracing::debug!(path = %p.display(), "Prompt fragment overridden from disk");
            s
        }
        _ => embedded.to_string(),
    }
}

/// All template fragments used by agentic loops, web tasks, chat, and crew.
#[derive(Clone, Debug)]
pub struct PromptBundle {
    pub agentic_system_intro: String,
    pub agentic_mcp_only_intro: String,
    pub agentic_tools_header: String,
    pub agentic_mcp_tools_header: String,
    pub unified_final_turn_suffix: String,
    pub unified_assistant_turn_marker: String,
    pub unified_compact_retry_suffix: String,
    pub unified_url_fetch_nudge: String,
    pub unified_preamble_nudge: String,
    pub unified_empty_final_reply: String,
    pub unified_all_calls_blocked: String,
    pub unified_repeat_call_warning: String,
    pub unified_merge_duplicates_notice: String,
    pub unified_skipped_calls_notice: String,
    pub unified_tool_results_followup: String,
    pub unified_all_tools_failed_suffix: String,
    pub unified_max_iters_error: String,
    pub unified_conversation_user_label: String,
    pub unified_tool_results_header: String,
    pub unified_mcp_only_tool_error: String,
    pub web_task_unified_body: String,
    pub web_task_mcp_body: String,
    pub web_chat_user_thread_header: String,
    pub web_chat_active_skill_fmt: String,
    pub web_task_skill_section_fmt: String,
    pub agent_runtime_prior_conversation_header: String,
    pub agent_runtime_instructions_fmt: String,
    pub agent_runtime_task_body_suffix: String,
    pub crew_agent_system_fmt: String,
    pub crew_worker_prompt_fmt: String,
    pub agent_legacy_default_name_fmt: String,
    pub agent_legacy_tool_block: String,
    pub agent_recalled_memories_header: String,
}

impl PromptBundle {
    fn embedded_inner() -> Self {
        Self {
            agentic_system_intro: embed!("agentic_system_intro.txt").to_string(),
            agentic_mcp_only_intro: embed!("agentic_mcp_only_intro.txt").to_string(),
            agentic_tools_header: embed!("agentic_tools_header.txt").to_string(),
            agentic_mcp_tools_header: embed!("agentic_mcp_tools_header.txt").to_string(),
            unified_final_turn_suffix: embed!("unified_final_turn_suffix.txt").to_string(),
            unified_assistant_turn_marker: embed!("unified_assistant_turn_marker.txt").to_string(),
            unified_compact_retry_suffix: embed!("unified_compact_retry_suffix.txt").to_string(),
            unified_url_fetch_nudge: embed!("unified_url_fetch_nudge.txt").to_string(),
            unified_preamble_nudge: embed!("unified_preamble_nudge.txt").to_string(),
            unified_empty_final_reply: embed!("unified_empty_final_reply.txt").to_string(),
            unified_all_calls_blocked: embed!("unified_all_calls_blocked.txt").to_string(),
            unified_repeat_call_warning: embed!("unified_repeat_call_warning.txt").to_string(),
            unified_merge_duplicates_notice: embed!("unified_merge_duplicates_notice.txt")
                .to_string(),
            unified_skipped_calls_notice: embed!("unified_skipped_calls_notice.txt").to_string(),
            unified_tool_results_followup: embed!("unified_tool_results_followup.txt").to_string(),
            unified_all_tools_failed_suffix: embed!("unified_all_tools_failed_suffix.txt")
                .to_string(),
            unified_max_iters_error: embed!("unified_max_iters_error.txt").to_string(),
            unified_conversation_user_label: embed!("unified_conversation_user_label.txt")
                .to_string(),
            unified_tool_results_header: embed!("unified_tool_results_header.txt").to_string(),
            unified_mcp_only_tool_error: embed!("unified_mcp_only_tool_error.txt").to_string(),
            web_task_unified_body: embed!("web_task_unified_body.txt").to_string(),
            web_task_mcp_body: embed!("web_task_mcp_body.txt").to_string(),
            web_chat_user_thread_header: embed!("web_chat_user_thread_header.txt").to_string(),
            web_chat_active_skill_fmt: embed!("web_chat_active_skill_fmt.txt").to_string(),
            web_task_skill_section_fmt: embed!("web_task_skill_section_fmt.txt").to_string(),
            agent_runtime_prior_conversation_header: embed!(
                "agent_runtime_prior_conversation_header.txt"
            )
            .to_string(),
            agent_runtime_instructions_fmt: embed!("agent_runtime_instructions_fmt.txt")
                .to_string(),
            agent_runtime_task_body_suffix: embed!("agent_runtime_task_body_suffix.txt")
                .to_string(),
            crew_agent_system_fmt: embed!("crew_agent_system_fmt.txt").to_string(),
            crew_worker_prompt_fmt: embed!("crew_worker_prompt_fmt.txt").to_string(),
            agent_legacy_default_name_fmt: embed!("agent_legacy_default_name_fmt.txt").to_string(),
            agent_legacy_tool_block: embed!("agent_legacy_tool_block.txt").to_string(),
            agent_recalled_memories_header: embed!("agent_recalled_memories_header.txt")
                .to_string(),
        }
    }

    /// Load with optional overlay directory (same-named `stem.txt` overrides each field).
    pub fn load(overlay: Option<&Path>) -> Self {
        let e = Self::embedded_inner();
        let Some(dir) = overlay else {
            return e;
        };
        Self {
            agentic_system_intro: read_overlay(
                dir,
                "agentic_system_intro",
                &e.agentic_system_intro,
            ),
            agentic_mcp_only_intro: read_overlay(
                dir,
                "agentic_mcp_only_intro",
                &e.agentic_mcp_only_intro,
            ),
            agentic_tools_header: read_overlay(
                dir,
                "agentic_tools_header",
                &e.agentic_tools_header,
            ),
            agentic_mcp_tools_header: read_overlay(
                dir,
                "agentic_mcp_tools_header",
                &e.agentic_mcp_tools_header,
            ),
            unified_final_turn_suffix: read_overlay(
                dir,
                "unified_final_turn_suffix",
                &e.unified_final_turn_suffix,
            ),
            unified_assistant_turn_marker: read_overlay(
                dir,
                "unified_assistant_turn_marker",
                &e.unified_assistant_turn_marker,
            ),
            unified_compact_retry_suffix: read_overlay(
                dir,
                "unified_compact_retry_suffix",
                &e.unified_compact_retry_suffix,
            ),
            unified_url_fetch_nudge: read_overlay(
                dir,
                "unified_url_fetch_nudge",
                &e.unified_url_fetch_nudge,
            ),
            unified_preamble_nudge: read_overlay(
                dir,
                "unified_preamble_nudge",
                &e.unified_preamble_nudge,
            ),
            unified_empty_final_reply: read_overlay(
                dir,
                "unified_empty_final_reply",
                &e.unified_empty_final_reply,
            ),
            unified_all_calls_blocked: read_overlay(
                dir,
                "unified_all_calls_blocked",
                &e.unified_all_calls_blocked,
            ),
            unified_repeat_call_warning: read_overlay(
                dir,
                "unified_repeat_call_warning",
                &e.unified_repeat_call_warning,
            ),
            unified_merge_duplicates_notice: read_overlay(
                dir,
                "unified_merge_duplicates_notice",
                &e.unified_merge_duplicates_notice,
            ),
            unified_skipped_calls_notice: read_overlay(
                dir,
                "unified_skipped_calls_notice",
                &e.unified_skipped_calls_notice,
            ),
            unified_tool_results_followup: read_overlay(
                dir,
                "unified_tool_results_followup",
                &e.unified_tool_results_followup,
            ),
            unified_all_tools_failed_suffix: read_overlay(
                dir,
                "unified_all_tools_failed_suffix",
                &e.unified_all_tools_failed_suffix,
            ),
            unified_max_iters_error: read_overlay(
                dir,
                "unified_max_iters_error",
                &e.unified_max_iters_error,
            ),
            unified_conversation_user_label: read_overlay(
                dir,
                "unified_conversation_user_label",
                &e.unified_conversation_user_label,
            ),
            unified_tool_results_header: read_overlay(
                dir,
                "unified_tool_results_header",
                &e.unified_tool_results_header,
            ),
            unified_mcp_only_tool_error: read_overlay(
                dir,
                "unified_mcp_only_tool_error",
                &e.unified_mcp_only_tool_error,
            ),
            web_task_unified_body: read_overlay(
                dir,
                "web_task_unified_body",
                &e.web_task_unified_body,
            ),
            web_task_mcp_body: read_overlay(dir, "web_task_mcp_body", &e.web_task_mcp_body),
            web_chat_user_thread_header: read_overlay(
                dir,
                "web_chat_user_thread_header",
                &e.web_chat_user_thread_header,
            ),
            web_chat_active_skill_fmt: read_overlay(
                dir,
                "web_chat_active_skill_fmt",
                &e.web_chat_active_skill_fmt,
            ),
            web_task_skill_section_fmt: read_overlay(
                dir,
                "web_task_skill_section_fmt",
                &e.web_task_skill_section_fmt,
            ),
            agent_runtime_prior_conversation_header: read_overlay(
                dir,
                "agent_runtime_prior_conversation_header",
                &e.agent_runtime_prior_conversation_header,
            ),
            agent_runtime_instructions_fmt: read_overlay(
                dir,
                "agent_runtime_instructions_fmt",
                &e.agent_runtime_instructions_fmt,
            ),
            agent_runtime_task_body_suffix: read_overlay(
                dir,
                "agent_runtime_task_body_suffix",
                &e.agent_runtime_task_body_suffix,
            ),
            crew_agent_system_fmt: read_overlay(
                dir,
                "crew_agent_system_fmt",
                &e.crew_agent_system_fmt,
            ),
            crew_worker_prompt_fmt: read_overlay(
                dir,
                "crew_worker_prompt_fmt",
                &e.crew_worker_prompt_fmt,
            ),
            agent_legacy_default_name_fmt: read_overlay(
                dir,
                "agent_legacy_default_name_fmt",
                &e.agent_legacy_default_name_fmt,
            ),
            agent_legacy_tool_block: read_overlay(
                dir,
                "agent_legacy_tool_block",
                &e.agent_legacy_tool_block,
            ),
            agent_recalled_memories_header: read_overlay(
                dir,
                "agent_recalled_memories_header",
                &e.agent_recalled_memories_header,
            ),
        }
    }

    pub fn web_task_unified(&self, description: &str, skill_block: &str) -> String {
        subst(
            &self.web_task_unified_body,
            &[("description", description), ("skill_block", skill_block)],
        )
    }

    pub fn web_task_mcp(&self, description: &str, skill_block: &str) -> String {
        subst(
            &self.web_task_mcp_body,
            &[("description", description), ("skill_block", skill_block)],
        )
    }

    pub fn web_chat_active_skill(&self, name: &str, content: &str) -> String {
        subst(
            &self.web_chat_active_skill_fmt,
            &[("name", name), ("content", content)],
        )
    }

    pub fn web_task_skill_section(
        &self,
        skill_name: &str,
        task_type: &str,
        content: &str,
    ) -> String {
        subst(
            &self.web_task_skill_section_fmt,
            &[
                ("skill_name", skill_name),
                ("task_type", task_type),
                ("content", content),
            ],
        )
    }

    pub fn agent_runtime_instructions_block(&self, spec: &str) -> String {
        subst(&self.agent_runtime_instructions_fmt, &[("spec", spec)])
    }

    pub fn agent_runtime_task_body(&self, user_input: &str) -> String {
        subst(
            &self.agent_runtime_task_body_suffix,
            &[("user_input", user_input)],
        )
    }

    pub fn crew_agent_system(&self, role: &str, goal: &str, backstory: &str) -> String {
        subst(
            &self.crew_agent_system_fmt,
            &[("role", role), ("goal", goal), ("backstory", backstory)],
        )
    }

    pub fn crew_worker_prompt(&self, summary: &str) -> String {
        subst(&self.crew_worker_prompt_fmt, &[("summary", summary)])
    }

    pub fn agent_legacy_default_name(&self, name: &str) -> String {
        subst(&self.agent_legacy_default_name_fmt, &[("name", name)])
    }

    pub fn unified_repeat_warning(&self, name: &str, count: u32) -> String {
        subst(
            &self.unified_repeat_call_warning,
            &[("name", name), ("count", &count.to_string())],
        )
    }

    pub fn unified_merge_duplicates(&self, duplicate_calls_merged: usize) -> String {
        subst(
            &self.unified_merge_duplicates_notice,
            &[(
                "duplicate_calls_merged",
                &duplicate_calls_merged.to_string(),
            )],
        )
    }

    pub fn unified_skipped_calls(&self, d: usize, max: usize) -> String {
        subst(
            &self.unified_skipped_calls_notice,
            &[("d", &d.to_string()), ("max", &max.to_string())],
        )
    }
}
