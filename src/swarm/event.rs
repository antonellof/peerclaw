//! Swarm events for SSE broadcasting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::agent::SwarmAgentState;
use super::profile::AgentProfile;

/// Events emitted by the swarm system for real-time visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data")]
pub enum SwarmEvent {
    /// An agent joined the swarm
    AgentJoined {
        agent_id: Uuid,
        name: String,
        peer_id: Option<String>,
        profile: AgentProfile,
        is_local: bool,
        timestamp: DateTime<Utc>,
    },

    /// An agent left the swarm
    AgentLeft {
        agent_id: Uuid,
        name: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// An agent's state changed
    AgentStateChanged {
        agent_id: Uuid,
        name: String,
        old_state: SwarmAgentState,
        new_state: SwarmAgentState,
        timestamp: DateTime<Utc>,
    },

    /// An agent performed an action
    AgentAction(AgentAction),

    /// A connection was established between agents
    AgentConnection {
        from_agent: Uuid,
        to_agent: Uuid,
        connection_type: ConnectionType,
        timestamp: DateTime<Utc>,
    },

    /// Full topology update (sent periodically or on request)
    TopologyUpdate {
        agents: Vec<AgentSummary>,
        connections: Vec<AgentConnectionInfo>,
        timestamp: DateTime<Utc>,
    },

    /// Swarm statistics update
    StatsUpdate {
        total_agents: usize,
        active_agents: usize,
        total_actions: u64,
        total_jobs: u64,
        timestamp: DateTime<Utc>,
    },
}

impl SwarmEvent {
    /// Get the timestamp of this event
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::AgentJoined { timestamp, .. } => *timestamp,
            Self::AgentLeft { timestamp, .. } => *timestamp,
            Self::AgentStateChanged { timestamp, .. } => *timestamp,
            Self::AgentAction(action) => action.timestamp,
            Self::AgentConnection { timestamp, .. } => *timestamp,
            Self::TopologyUpdate { timestamp, .. } => *timestamp,
            Self::StatsUpdate { timestamp, .. } => *timestamp,
        }
    }

    /// Get the event type as a string (for SSE)
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::AgentJoined { .. } => "swarm_agent_joined",
            Self::AgentLeft { .. } => "swarm_agent_left",
            Self::AgentStateChanged { .. } => "swarm_agent_state",
            Self::AgentAction { .. } => "swarm_action",
            Self::AgentConnection { .. } => "swarm_connection",
            Self::TopologyUpdate { .. } => "swarm_topology",
            Self::StatsUpdate { .. } => "swarm_stats",
        }
    }
}

/// An action performed by an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    /// Unique action ID
    pub id: Uuid,

    /// Agent that performed the action
    pub agent_id: Uuid,

    /// Agent name (for display)
    pub agent_name: String,

    /// Type of action
    pub action_type: ActionType,

    /// Human-readable description
    pub description: String,

    /// Additional details (action-specific)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,

    /// Whether the action succeeded
    pub success: bool,

    /// Duration in milliseconds (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    /// When the action occurred
    pub timestamp: DateTime<Utc>,
}

impl AgentAction {
    /// Create a new action
    pub fn new(
        agent_id: Uuid,
        agent_name: String,
        action_type: ActionType,
        description: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            agent_id,
            agent_name,
            action_type,
            description,
            details: None,
            success: true,
            duration_ms: None,
            timestamp: Utc::now(),
        }
    }

    /// Set additional details
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Mark as failed
    pub fn failed(mut self) -> Self {
        self.success = false;
        self
    }

    /// Set duration
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }
}

/// Types of actions agents can perform
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    // Inference actions
    Inference,
    Thinking,
    Response,

    // Tool actions
    ToolCall,
    ToolResult,

    // Job marketplace actions
    JobRequest,
    JobBid,
    JobAccepted,
    JobStarted,
    JobCompleted,
    JobFailed,

    // Network actions
    PeerConnect,
    PeerDisconnect,
    MessageSent,
    MessageReceived,

    // Memory actions
    MemoryStore,
    MemoryRetrieve,

    // System actions
    Startup,
    Shutdown,
    Error,

    // Custom action
    Custom(String),
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inference => write!(f, "Inference"),
            Self::Thinking => write!(f, "Thinking"),
            Self::Response => write!(f, "Response"),
            Self::ToolCall => write!(f, "Tool Call"),
            Self::ToolResult => write!(f, "Tool Result"),
            Self::JobRequest => write!(f, "Job Request"),
            Self::JobBid => write!(f, "Job Bid"),
            Self::JobAccepted => write!(f, "Job Accepted"),
            Self::JobStarted => write!(f, "Job Started"),
            Self::JobCompleted => write!(f, "Job Completed"),
            Self::JobFailed => write!(f, "Job Failed"),
            Self::PeerConnect => write!(f, "Peer Connect"),
            Self::PeerDisconnect => write!(f, "Peer Disconnect"),
            Self::MessageSent => write!(f, "Message Sent"),
            Self::MessageReceived => write!(f, "Message Received"),
            Self::MemoryStore => write!(f, "Memory Store"),
            Self::MemoryRetrieve => write!(f, "Memory Retrieve"),
            Self::Startup => write!(f, "Startup"),
            Self::Shutdown => write!(f, "Shutdown"),
            Self::Error => write!(f, "Error"),
            Self::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Types of connections between agents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    /// P2P network connection
    P2P,
    /// Job delegation
    JobDelegation,
    /// Message exchange
    Messaging,
    /// Resource sharing
    ResourceSharing,
}

/// Summary of an agent for topology updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub id: Uuid,
    pub name: String,
    pub peer_id: Option<String>,
    pub state: String,
    pub is_local: bool,
    pub action_count: u64,
    pub success_rate: f64,
}

/// Connection info for topology updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConnectionInfo {
    pub from: Uuid,
    pub to: Uuid,
    pub connection_type: ConnectionType,
    pub strength: f64, // 0.0 - 1.0, based on interaction frequency
}
