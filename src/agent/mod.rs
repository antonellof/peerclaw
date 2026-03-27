//! Agent runtime for autonomous task execution.
//!
//! Provides the core agentic loop: receive task → plan → execute tools → return result.
//! Agents use a ReAct-style loop where the LLM decides which tools to call.

pub mod budget;
pub mod compaction;
pub mod runtime;
pub mod session;
pub mod spec;

pub use budget::BudgetTracker;
pub use runtime::{extract_answer, parse_tool_calls, AgentConfig, AgentResult, AgentRuntime, ParsedToolCall};
pub use session::{SessionInfo, SessionStore, SessionTurn};
pub use spec::AgentSpec;
