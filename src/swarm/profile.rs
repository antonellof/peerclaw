//! Agent profile with personality traits and capabilities.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent profile containing personality and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Display name for the agent
    pub display_name: Option<String>,

    /// Seed for procedural avatar generation
    pub avatar_seed: String,

    /// Short bio/description
    #[serde(default)]
    pub bio: String,

    /// Personality traits
    #[serde(default)]
    pub personality: PersonalityTraits,

    /// Agent capabilities
    #[serde(default)]
    pub capabilities: Vec<AgentCapability>,

    /// Model being used (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Platform-specific metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,

    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Default for AgentProfile {
    fn default() -> Self {
        Self {
            display_name: None,
            avatar_seed: uuid::Uuid::new_v4().to_string(),
            bio: String::new(),
            personality: PersonalityTraits::default(),
            capabilities: Vec::new(),
            model: None,
            metadata: HashMap::new(),
            tags: Vec::new(),
        }
    }
}

impl AgentProfile {
    /// Create a new profile with a name
    pub fn new(name: &str) -> Self {
        Self {
            display_name: Some(name.to_string()),
            avatar_seed: name.to_string(),
            ..Default::default()
        }
    }

    /// Create profile from peer capabilities
    pub fn from_capabilities(capabilities: Vec<AgentCapability>) -> Self {
        Self {
            capabilities,
            ..Default::default()
        }
    }

    /// Add a capability
    pub fn with_capability(mut self, capability: AgentCapability) -> Self {
        self.capabilities.push(capability);
        self
    }

    /// Set model
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Set bio
    pub fn with_bio(mut self, bio: &str) -> Self {
        self.bio = bio.to_string();
        self
    }

    /// Check if agent has a capability
    pub fn has_capability(&self, capability: &AgentCapability) -> bool {
        self.capabilities.contains(capability)
    }

    /// Get capability score (0.0 - 1.0) based on number of capabilities
    pub fn capability_score(&self) -> f64 {
        let max_capabilities = 10.0;
        (self.capabilities.len() as f64 / max_capabilities).min(1.0)
    }
}

/// Personality traits for an agent (MiroFish-inspired)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalityTraits {
    /// How creative/divergent the agent's thinking is (0.0 - 1.0)
    pub creativity: f32,

    /// How willing the agent is to collaborate (0.0 - 1.0)
    pub collaboration: f32,

    /// How thorough/detailed the agent is (0.0 - 1.0)
    pub thoroughness: f32,

    /// Speed vs quality tradeoff (0.0 = quality, 1.0 = speed)
    pub speed_vs_quality: f32,

    /// Risk tolerance (0.0 = conservative, 1.0 = bold)
    pub risk_tolerance: f32,

    /// Verbosity level (0.0 = terse, 1.0 = verbose)
    pub verbosity: f32,
}

impl Default for PersonalityTraits {
    fn default() -> Self {
        Self {
            creativity: 0.5,
            collaboration: 0.7,
            thoroughness: 0.6,
            speed_vs_quality: 0.5,
            risk_tolerance: 0.3,
            verbosity: 0.5,
        }
    }
}

impl PersonalityTraits {
    /// Create a balanced personality
    pub fn balanced() -> Self {
        Self::default()
    }

    /// Create a creative personality
    pub fn creative() -> Self {
        Self {
            creativity: 0.9,
            collaboration: 0.6,
            thoroughness: 0.4,
            speed_vs_quality: 0.6,
            risk_tolerance: 0.7,
            verbosity: 0.6,
        }
    }

    /// Create an analytical personality
    pub fn analytical() -> Self {
        Self {
            creativity: 0.3,
            collaboration: 0.5,
            thoroughness: 0.9,
            speed_vs_quality: 0.2,
            risk_tolerance: 0.2,
            verbosity: 0.7,
        }
    }

    /// Create a collaborative personality
    pub fn collaborative() -> Self {
        Self {
            creativity: 0.5,
            collaboration: 0.95,
            thoroughness: 0.5,
            speed_vs_quality: 0.5,
            risk_tolerance: 0.4,
            verbosity: 0.6,
        }
    }

    /// Create a fast-moving personality
    pub fn fast() -> Self {
        Self {
            creativity: 0.4,
            collaboration: 0.6,
            thoroughness: 0.3,
            speed_vs_quality: 0.9,
            risk_tolerance: 0.5,
            verbosity: 0.3,
        }
    }
}

/// Capabilities an agent can have
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    /// Can perform LLM inference
    Inference,

    /// Has GPU acceleration
    GpuAcceleration,

    /// Can execute WASM tools
    WasmSandbox,

    /// Can store data
    Storage,

    /// Can relay messages
    Relay,

    /// Can access the web
    WebAccess,

    /// Has vector memory
    VectorMemory,

    /// Can build tools dynamically
    ToolBuilding,

    /// Can run MCP servers
    McpServer,

    /// Has specific model available
    Model(String),

    /// Custom capability
    Custom(String),
}

impl std::fmt::Display for AgentCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inference => write!(f, "Inference"),
            Self::GpuAcceleration => write!(f, "GPU"),
            Self::WasmSandbox => write!(f, "WASM"),
            Self::Storage => write!(f, "Storage"),
            Self::Relay => write!(f, "Relay"),
            Self::WebAccess => write!(f, "Web"),
            Self::VectorMemory => write!(f, "Memory"),
            Self::ToolBuilding => write!(f, "Tools"),
            Self::McpServer => write!(f, "MCP"),
            Self::Model(name) => write!(f, "Model:{}", name),
            Self::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Convert from P2P Capability enum
impl From<crate::p2p::Capability> for AgentCapability {
    fn from(cap: crate::p2p::Capability) -> Self {
        match cap {
            crate::p2p::Capability::WasmSandbox => Self::WasmSandbox,
            crate::p2p::Capability::Inference => Self::Inference,
            crate::p2p::Capability::Storage => Self::Storage,
            crate::p2p::Capability::Relay => Self::Relay,
            crate::p2p::Capability::WebProxy => Self::WebAccess,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_profile() {
        let profile = AgentProfile::default();
        assert!(profile.display_name.is_none());
        assert!(profile.capabilities.is_empty());
    }

    #[test]
    fn test_personality_presets() {
        let creative = PersonalityTraits::creative();
        assert!(creative.creativity > 0.8);

        let analytical = PersonalityTraits::analytical();
        assert!(analytical.thoroughness > 0.8);
    }

    #[test]
    fn test_capability_check() {
        let profile = AgentProfile::default()
            .with_capability(AgentCapability::Inference)
            .with_capability(AgentCapability::GpuAcceleration);

        assert!(profile.has_capability(&AgentCapability::Inference));
        assert!(!profile.has_capability(&AgentCapability::Storage));
    }
}
