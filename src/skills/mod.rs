//! Skills system for extending agent behavior via prompt instructions.
//!
//! Skills are SKILL.md files (YAML frontmatter + markdown prompt) that extend the
//! agent's behavior through prompt-level instructions. Skills can be:
//! - Local (trusted, full tool access)
//! - Shared via P2P network (verified, restricted access)
//! - Downloaded from peers (installed, limited tools)
//!
//! # P2P Integration
//!
//! Skills can be shared across the network:
//! - Peers announce their available skills
//! - Skills are content-addressed (BLAKE3 hash)
//! - Trust is based on source: local > verified > network

pub mod parser;
mod registry;
mod selector;

pub use parser::{parse_skill, ParseError};
pub use registry::{SkillInfo, SkillRegistry};
pub use selector::{score_skill, select_skills, SkillScore};

use std::path::PathBuf;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Maximum file size for SKILL.md (64 KiB).
pub const MAX_SKILL_SIZE: u64 = 64 * 1024;

/// GossipSub topic for skill announcements across the P2P network.
pub const SKILLS_TOPIC: &str = "peerclaw/skills/v1";

/// Regex for validating skill names.
static SKILL_NAME_PATTERN: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}$").unwrap());

/// Validate a skill name.
pub fn validate_skill_name(name: &str) -> bool {
    SKILL_NAME_PATTERN.is_match(name)
}

/// Trust level for skills (affects tool access).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrust {
    /// Network skill (from other peers). Read-only tools only.
    Network = 0,
    /// Installed skill (downloaded, verified). Limited tools.
    Installed = 1,
    /// Local skill (user-placed). Full tool access.
    Local = 2,
}

impl std::fmt::Display for SkillTrust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network => write!(f, "network"),
            Self::Installed => write!(f, "installed"),
            Self::Local => write!(f, "local"),
        }
    }
}

/// Where a skill was loaded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Local skills directory (~/.peerclaw/skills/).
    Local(PathBuf),
    /// Workspace skills directory.
    Workspace(PathBuf),
    /// Downloaded from network peer.
    Network { peer_id: String, hash: String },
    /// Bundled with the application.
    Bundled(String),
}

/// Activation criteria from SKILL.md frontmatter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivationCriteria {
    /// Keywords that trigger this skill.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Keywords that exclude this skill.
    #[serde(default)]
    pub exclude_keywords: Vec<String>,
    /// Regex patterns for matching.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Tags for category matching.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Maximum context tokens for this skill's prompt.
    #[serde(default = "default_max_tokens")]
    pub max_context_tokens: usize,
}

fn default_max_tokens() -> usize {
    2000
}

/// Requirements for skill activation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillRequirements {
    /// Required binaries on PATH.
    #[serde(default)]
    pub bins: Vec<String>,
    /// Required environment variables.
    #[serde(default)]
    pub env: Vec<String>,
    /// Required tools.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Required models.
    #[serde(default)]
    pub models: Vec<String>,
}

/// Skill manifest from YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    /// Skill name.
    pub name: String,
    /// Version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Author.
    #[serde(default)]
    pub author: Option<String>,
    /// Activation criteria.
    #[serde(default)]
    pub activation: ActivationCriteria,
    /// Requirements.
    #[serde(default)]
    pub requires: SkillRequirements,
    /// P2P sharing settings.
    #[serde(default)]
    pub sharing: SkillSharing,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// P2P sharing settings for a skill.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillSharing {
    /// Whether this skill can be shared with other peers.
    #[serde(default)]
    pub enabled: bool,
    /// Price to access this skill (in micro-PCLAW).
    #[serde(default)]
    pub price: u64,
    /// Maximum uses per peer per day.
    #[serde(default)]
    pub rate_limit: Option<u32>,
}

/// A loaded skill ready for use.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// Parsed manifest.
    pub manifest: SkillManifest,
    /// Prompt content (markdown body).
    pub prompt_content: String,
    /// Trust level.
    pub trust: SkillTrust,
    /// Source location.
    pub source: SkillSource,
    /// Content hash (BLAKE3).
    pub hash: String,
    /// Whether requirements are met.
    pub requirements_met: bool,
}

impl LoadedSkill {
    /// Get the skill name.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Get the skill description.
    pub fn description(&self) -> &str {
        &self.manifest.description
    }

    /// Check if this skill can be used.
    pub fn is_available(&self) -> bool {
        self.requirements_met
    }

    /// Get the prompt content for injection into LLM context.
    pub fn prompt(&self) -> &str {
        &self.prompt_content
    }

    /// Calculate content hash.
    pub fn calculate_hash(content: &str) -> String {
        let hash = blake3::hash(content.as_bytes());
        hash.to_hex()[..16].to_string()
    }
}

/// Skill announcement for P2P sharing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAnnouncement {
    /// Skill name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Description.
    pub description: String,
    /// Content hash.
    pub hash: String,
    /// Price per use.
    pub price: u64,
    /// Provider peer ID.
    pub provider: String,
    /// Activation keywords (for matching).
    pub keywords: Vec<String>,
    /// Tags.
    pub tags: Vec<String>,
}

/// A batch of skill announcements from a single peer, with signature.
/// Published to the `peerclaw/skills/v1` GossipSub topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAnnouncementBatch {
    /// The announcing peer's ID.
    pub peer_id: String,
    /// All skills this peer is sharing.
    pub skills: Vec<SkillAnnouncement>,
    /// Unix timestamp when this batch was created.
    pub timestamp: u64,
    /// Ed25519 signature over (peer_id || timestamp || skills hash).
    pub signature: Vec<u8>,
}

impl SkillAnnouncementBatch {
    /// Create a new unsigned batch.
    pub fn new(peer_id: String, skills: Vec<SkillAnnouncement>) -> Self {
        Self {
            peer_id,
            skills,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: vec![],
        }
    }

    /// Get the bytes to sign.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.peer_id.as_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        // Hash the skills payload for a compact signing input
        let skills_bytes = rmp_serde::to_vec(&self.skills).unwrap_or_default();
        let skills_hash = blake3::hash(&skills_bytes);
        bytes.extend_from_slice(skills_hash.as_bytes());
        bytes
    }

    /// Sign this batch.
    pub fn sign<F>(&mut self, signer: F)
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let bytes = self.signing_bytes();
        self.signature = signer(&bytes);
    }

    /// Verify the batch signature with the given verifier closure.
    ///
    /// The verifier receives `(signing_bytes, signature)` and should return
    /// `true` if the signature is valid for the announcing peer.
    pub fn verify<F>(&self, verifier: F) -> bool
    where
        F: FnOnce(&[u8], &[u8]) -> bool,
    {
        let bytes = self.signing_bytes();
        verifier(&bytes, &self.signature)
    }

    /// Check if this batch is expired (older than `max_age_secs`).
    pub fn is_expired(&self, max_age_secs: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.timestamp) > max_age_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_skill_name() {
        assert!(validate_skill_name("my-skill"));
        assert!(validate_skill_name("my_skill_v2"));
        assert!(validate_skill_name("MySkill.v1"));
        assert!(!validate_skill_name("-invalid"));
        assert!(!validate_skill_name(""));
        assert!(!validate_skill_name("a".repeat(100).as_str()));
    }

    #[test]
    fn test_skill_trust_ordering() {
        assert!(SkillTrust::Network < SkillTrust::Installed);
        assert!(SkillTrust::Installed < SkillTrust::Local);
    }
}
