//! PeerClaw - Decentralized P2P AI Agent Network
//!
//! A fully decentralized, peer-to-peer network where autonomous AI agents
//! collaborate, share resources, and transact using a native token economy.

pub mod bootstrap;
pub mod channel;
pub mod channels;
pub mod cli;
pub mod config;
pub mod db;
pub mod executor;
pub mod identity;
pub mod inference;
pub mod job;
pub mod mcp;
pub mod messaging;
pub mod node;
pub mod p2p;
pub mod proxy;
pub mod routines;
pub mod runtime;
pub mod safety;
pub mod skills;
pub mod tools;
pub mod vector;
pub mod wallet;
pub mod wasm;
pub mod web;
pub mod workspace;

// Re-export commonly used types
pub use config::Config;
pub use executor::{ExecutorConfig, ResourceMonitor, TaskExecutor};
pub use identity::NodeIdentity;
pub use inference::{InferenceConfig, InferenceEngine};
pub use node::Node;
pub use runtime::Runtime;
pub use skills::{SkillRegistry, LoadedSkill, SkillTrust};
pub use tools::{Tool, ToolRegistry, ToolContext, ToolOutput, ToolError};
pub use safety::{SafetyLayer, SafetyConfig, LeakDetector, Sanitizer};
pub use vector::{VectorStore, VectorStoreConfig, SearchResult};
pub use wallet::{Wallet, WalletConfig};
pub use workspace::{Workspace, WorkspaceConfig};
pub use channels::{Channel, ChannelManager, IncomingMessage, OutgoingResponse};
pub use routines::{RoutineEngine, RoutineConfig, Routine, Heartbeat};
pub use mcp::{McpManager, McpConfig, McpClient};
