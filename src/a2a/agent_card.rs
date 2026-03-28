//! Agent Card — capability metadata for discovery (A2A-shaped).

use serde::{Deserialize, Serialize};

/// How callers reach this agent's RPC surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTransport {
    /// e.g. `http`, `https`
    #[serde(rename = "type")]
    pub transport_type: String,
    /// Base URL for JSON-RPC POST (e.g. `http://127.0.0.1:8080/a2a`)
    pub url: String,
}

/// Declared skill / capability entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSkill {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Agent Card document served at `/.well-known/agent-card.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    /// Protocol version marker for PeerClaw nodes.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AgentSkill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_transport: Option<AgentTransport>,
    /// libp2p peer id string when announced on the mesh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    /// Models this node can run (best-effort from local inference).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

impl AgentCard {
    pub fn peerclaw_default(
        name: impl Into<String>,
        description: impl Into<String>,
        base_url: String,
        peer_id: String,
        models: Vec<String>,
    ) -> Self {
        Self {
            protocol_version: "0.1".to_string(),
            name: name.into(),
            description: description.into(),
            url: base_url.clone(),
            skills: vec![
                AgentSkill {
                    id: "peerclaw.inference".to_string(),
                    name: "Local inference".to_string(),
                    description: "Execute inference via node executor".to_string(),
                },
                AgentSkill {
                    id: "peerclaw.tasks".to_string(),
                    name: "A2A tasks".to_string(),
                    description: "Create and query A2A task lifecycle".to_string(),
                },
            ],
            preferred_transport: Some(AgentTransport {
                transport_type: "http".to_string(),
                url: format!("{}/a2a", base_url.trim_end_matches('/')),
            }),
            peer_id: Some(peer_id),
            models,
        }
    }
}

/// Gossip payload: announce an agent card to the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCardAnnouncement {
    pub peer_id: String,
    pub epoch_ms: u64,
    pub card: AgentCard,
}
