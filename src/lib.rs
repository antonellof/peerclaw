//! PeerClaw - Decentralized P2P AI Agent Network
//!
//! A fully decentralized, peer-to-peer network where autonomous AI agents
//! collaborate, share resources, and transact using a native token economy.

pub mod a2a;
pub mod agent;
pub mod bootstrap;
pub mod channel;
pub mod channels;
pub mod cli;
pub mod config;
pub mod crew;
pub mod db;
pub mod executor;
pub mod flow;
pub mod identity;
pub mod inference;
pub mod job;
pub mod mcp;
pub mod messaging;
pub mod models_hf;
pub mod node;
pub mod p2p;
pub mod proxy;
pub mod routines;
pub mod runtime;
pub mod safety;
pub mod skills;
pub mod swarm;
pub mod tools;
pub mod vector;
pub mod wallet;
pub mod wasm;
pub mod web;
pub mod workspace;

// Re-export commonly used types
pub use channels::{Channel, ChannelManager, IncomingMessage, OutgoingResponse};
pub use config::Config;
pub use executor::{ExecutorConfig, ResourceMonitor, TaskExecutor};
pub use identity::NodeIdentity;
pub use inference::{InferenceConfig, InferenceEngine};
pub use mcp::{McpClient, McpConfig, McpManager};
pub use node::Node;
pub use routines::{Heartbeat, Routine, RoutineConfig, RoutineEngine};
pub use runtime::Runtime;
pub use safety::{LeakDetector, SafetyConfig, SafetyLayer, Sanitizer};
pub use skills::{LoadedSkill, SkillRegistry, SkillTrust};
pub use swarm::{AgentProfile, SwarmAgent, SwarmEvent, SwarmManager};
pub use tools::{Tool, ToolContext, ToolError, ToolOutput, ToolRegistry};
pub use vector::{SearchResult, VectorStore, VectorStoreConfig};
pub use wallet::{Wallet, WalletConfig};
pub use workspace::{Workspace, WorkspaceConfig};
