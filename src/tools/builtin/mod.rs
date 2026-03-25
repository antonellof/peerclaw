//! Builtin tools for the AI agent.
//!
//! These tools provide core capabilities:
//! - Core: echo, time, json
//! - Network: http, web_fetch
//! - Filesystem: file_read, file_write, file_list
//! - System: shell
//! - P2P: memory, job, peer_discovery, wallet

mod core;
mod file;
mod http;
mod memory;
mod p2p;
mod shell;

pub use core::{EchoTool, JsonTool, TimeTool};
pub use file::{FileListTool, FileReadTool, FileWriteTool};
pub use http::{HttpTool, WebFetchTool};
pub use memory::{MemorySearchTool, MemoryWriteTool};
pub use p2p::{JobStatusTool, JobSubmitTool, PeerDiscoveryTool, WalletBalanceTool};
pub use shell::ShellTool;
