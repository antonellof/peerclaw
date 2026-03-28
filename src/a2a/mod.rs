//! Agent-to-agent (A2A) interoperability layer.
//!
//! HTTP JSON-RPC 2.0 surface and GossipSub discovery align with the open A2A model
//! (see <https://google.github.io/A2A/specification/>). libp2p request-response
//! carries the same JSON-RPC envelope when peers are connected on the mesh.

pub mod agent_card;
pub mod gossip;
pub mod jsonrpc;
pub mod state;

pub use agent_card::{AgentCard, AgentCardAnnouncement, AgentSkill, AgentTransport};
pub use gossip::A2A_GOSSIP_TOPIC;
pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
pub use state::{A2aState, A2aTaskRecord, A2aTaskStatus};
