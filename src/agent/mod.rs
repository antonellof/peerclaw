//! Agent runtime for autonomous task execution.
//!
//! Provides the core agentic loop: receive task → plan → execute tools → return result.
//! Agents use a ReAct-style loop where the LLM decides which tools to call.

pub mod budget;
pub mod runtime;
pub mod spec;

pub use budget::BudgetTracker;
pub use runtime::{AgentRuntime, AgentResult, AgentConfig};
pub use spec::AgentSpec;
