//! Builtin tools for the AI agent.
//!
//! These tools provide core capabilities:
//! - Core: echo, time, json
//! - Network: http, web_fetch, browser
//! - Filesystem: file_read, file_write, file_list
//! - Documents: pdf_read
//! - System: shell
//! - P2P: memory, job, peer_discovery, wallet

mod browser;
mod code_exec;
mod core;
mod file;
mod http;
mod llm;
mod memory;
mod p2p;
pub mod patch;
mod pdf;
pub mod search;
mod shell;
mod subagent;

pub use browser::BrowserTool;
pub use code_exec::CodeExecTool;
pub use core::{EchoTool, JsonTool, TimeTool};
pub use file::{FileListTool, FileReadTool, FileWriteTool};
pub use http::{HttpTool, WebFetchTool};
pub use llm::LlmTaskTool;
pub use memory::{MemorySearchTool, MemoryWriteTool};
pub use p2p::{JobStatusTool, JobSubmitTool, PeerDiscoveryTool, WalletBalanceTool};
pub use patch::ApplyPatchTool;
pub use pdf::PdfReadTool;
pub use search::WebSearchTool;
pub use shell::ShellTool;
pub use subagent::SubAgentTool;
