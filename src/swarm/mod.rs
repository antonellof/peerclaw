//! Swarm Agent Visualization System
//!
//! This module provides MiroFish-style swarm agent tracking and visualization:
//! - Agent lifecycle management
//! - Real-time event streaming via SSE
//! - Network topology visualization
//! - Action timeline tracking

mod agent;
mod event;
mod manager;
mod profile;

pub use agent::{SwarmAgent, SwarmAgentState};
pub use event::{SwarmEvent, AgentAction, ConnectionType};
pub use manager::SwarmManager;
pub use profile::{AgentProfile, PersonalityTraits, AgentCapability};
