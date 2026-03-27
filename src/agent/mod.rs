//! Agent runtime for autonomous task execution.
//!
//! Provides the core agentic loop: receive task → plan → execute tools → return result.
//! Agents use a ReAct-style loop where the LLM decides which tools to call.

pub mod budget;
pub mod compaction;
pub mod runtime;
pub mod session;
pub mod spec;
pub mod unified_loop;

pub use budget::BudgetTracker;
pub use runtime::{
    extract_answer, parse_tool_calls, AgentConfig, AgentResult, AgentRuntime, AgentTaskExtras,
    ParsedToolCall, ToolCallRecord,
};
pub use session::{SessionInfo, SessionStore, SessionTurn};
pub use spec::AgentSpec;
pub use unified_loop::{
    build_agentic_system_prefix, run_unified_agentic_loop, AgenticInferenceSink,
    AgenticProgressSink, AgenticTurnOutcome, NoAgenticProgress, AGENTIC_MAX_ITERS,
    AGENTIC_MAX_TOOL_CALLS_PER_PASS,
};
