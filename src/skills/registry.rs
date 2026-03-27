//! Skill registry for managing and discovering skills.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use serde::Serialize;

use super::parser::{parse_skill, ParseError};
use super::{LoadedSkill, SkillAnnouncement, SkillAnnouncementBatch, SkillSource, SkillTrust};

/// Skill registry manages local and network skills.
pub struct SkillRegistry {
    /// Local peer ID
    local_peer_id: String,
    /// Skills directory
    skills_dir: PathBuf,
    /// Loaded local skills
    local_skills: RwLock<HashMap<String, Arc<LoadedSkill>>>,
    /// Network skills (from other peers)
    network_skills: RwLock<HashMap<String, Vec<SkillAnnouncement>>>,
    /// Disabled skill names (toggled off by the user).
    disabled_skills: RwLock<HashSet<String>>,
}

impl SkillRegistry {
    /// Create a new skill registry.
    pub fn new(skills_dir: PathBuf, local_peer_id: String) -> std::io::Result<Self> {
        std::fs::create_dir_all(&skills_dir)?;

        Ok(Self {
            local_peer_id,
            skills_dir,
            local_skills: RwLock::new(HashMap::new()),
            network_skills: RwLock::new(HashMap::new()),
            disabled_skills: RwLock::new(HashSet::new()),
        })
    }

    /// Toggle a skill on or off. Returns the new enabled state, or `None` if not found.
    pub async fn toggle_skill(&self, name: &str) -> Option<bool> {
        let exists = self.local_skills.read().await.contains_key(name)
            || self.network_skills.read().await.contains_key(name);
        if !exists {
            return None;
        }
        let mut disabled = self.disabled_skills.write().await;
        if disabled.contains(name) {
            disabled.remove(name);
            Some(true)
        } else {
            disabled.insert(name.to_string());
            Some(false)
        }
    }

    /// Check whether a skill is currently enabled.
    pub async fn is_enabled(&self, name: &str) -> bool {
        !self.disabled_skills.read().await.contains(name)
    }

    /// Scan skills directory and load all skills.
    pub async fn scan(&self) -> Result<usize, ScanError> {
        let mut count = 0;
        let mut skills = self.local_skills.write().await;
        skills.clear();

        // Scan skills directory
        let entries =
            std::fs::read_dir(&self.skills_dir).map_err(|e| ScanError::IoError(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();

            // Check for SKILL.md files or directories with SKILL.md
            let skill_file = if path.is_dir() {
                path.join("SKILL.md")
            } else if path.extension().is_some_and(|e| e == "md") {
                path.clone()
            } else {
                continue;
            };

            if !skill_file.exists() {
                continue;
            }

            match parse_skill(&skill_file, SkillTrust::Local) {
                Ok(skill) => {
                    tracing::info!(
                        skill = %skill.name(),
                        version = %skill.manifest.version,
                        "Loaded skill"
                    );
                    skills.insert(skill.name().to_string(), Arc::new(skill));
                    count += 1;
                }
                Err(e) => {
                    tracing::warn!(
                        path = %skill_file.display(),
                        error = %e,
                        "Failed to load skill"
                    );
                }
            }
        }

        Ok(count)
    }

    /// Get a skill by name.
    pub async fn get(&self, name: &str) -> Option<Arc<LoadedSkill>> {
        self.local_skills.read().await.get(name).cloned()
    }

    /// Select the best matching skill for the given user input text.
    pub async fn select_best(&self, input: &str) -> Option<Arc<LoadedSkill>> {
        let skills: Vec<Arc<LoadedSkill>> =
            self.local_skills.read().await.values().cloned().collect();
        let scores = super::selector::select_skills(&skills, input, &[], 1, 0.3);
        scores.into_iter().next().map(|s| s.skill)
    }

    /// List all local skills.
    pub async fn list_local(&self) -> Vec<Arc<LoadedSkill>> {
        self.local_skills.read().await.values().cloned().collect()
    }

    /// List all available skills (local + network).
    pub async fn list_all(&self) -> Vec<SkillInfo> {
        let mut skills = Vec::new();
        let disabled = self.disabled_skills.read().await;

        // Local skills
        for skill in self.local_skills.read().await.values() {
            let name = skill.name().to_string();
            skills.push(SkillInfo {
                enabled: !disabled.contains(&name),
                name,
                version: skill.manifest.version.clone(),
                description: skill.description().to_string(),
                trust: skill.trust,
                available: skill.is_available(),
                provider: self.local_peer_id.clone(),
                price: skill.manifest.sharing.price,
                keywords: skill.manifest.activation.keywords.clone(),
                tags: skill.manifest.activation.tags.clone(),
            });
        }

        // Network skills
        for (name, announcements) in self.network_skills.read().await.iter() {
            if let Some(best) = announcements.first() {
                skills.push(SkillInfo {
                    enabled: !disabled.contains(name),
                    name: name.clone(),
                    version: best.version.clone(),
                    description: best.description.clone(),
                    trust: SkillTrust::Network,
                    available: true,
                    provider: best.provider.clone(),
                    price: best.price,
                    keywords: best.keywords.clone(),
                    tags: best.tags.clone(),
                });
            }
        }

        skills
    }

    /// Register a skill announcement from the network.
    pub async fn register_network_skill(&self, announcement: SkillAnnouncement) {
        let mut network = self.network_skills.write().await;
        let announcements = network
            .entry(announcement.name.clone())
            .or_insert_with(Vec::new);

        // Update or add
        if let Some(existing) = announcements
            .iter_mut()
            .find(|a| a.provider == announcement.provider)
        {
            *existing = announcement;
        } else {
            announcements.push(announcement);
        }

        // Sort by price
        announcements.sort_by_key(|a| a.price);
    }

    /// Get local skill announcements for sharing.
    pub async fn get_announcements(&self) -> Vec<SkillAnnouncement> {
        let skills = self.local_skills.read().await;
        skills
            .values()
            .filter(|s| s.manifest.sharing.enabled)
            .map(|s| SkillAnnouncement {
                name: s.name().to_string(),
                version: s.manifest.version.clone(),
                description: s.description().to_string(),
                hash: s.hash.clone(),
                price: s.manifest.sharing.price,
                provider: self.local_peer_id.clone(),
                keywords: s.manifest.activation.keywords.clone(),
                tags: s.manifest.activation.tags.clone(),
            })
            .collect()
    }

    /// Install a skill from content.
    pub async fn install(
        &self,
        content: &str,
        trust: SkillTrust,
    ) -> Result<Arc<LoadedSkill>, ParseError> {
        let source = SkillSource::Workspace(self.skills_dir.clone());
        let skill = super::parser::parse_skill_content(content, source, trust)?;
        let name = skill.name().to_string();

        // Save to file
        let skill_file = self.skills_dir.join(format!("{}.md", &name));
        std::fs::write(&skill_file, content)?;

        let skill = Arc::new(skill);
        self.local_skills.write().await.insert(name, skill.clone());

        Ok(skill)
    }

    /// Remove a skill.
    pub async fn remove(&self, name: &str) -> bool {
        let mut skills = self.local_skills.write().await;
        if skills.remove(name).is_some() {
            // Try to remove file
            let skill_file = self.skills_dir.join(format!("{}.md", name));
            let _ = std::fs::remove_file(skill_file);
            true
        } else {
            false
        }
    }

    /// Build a signed skill announcement batch for publishing to the network.
    ///
    /// Only includes skills that have `sharing.enabled = true`.
    /// The caller should serialize the result and publish it to the
    /// `peerclaw/skills/v1` GossipSub topic.
    pub async fn build_announcement_batch<F>(&self, signer: F) -> Option<SkillAnnouncementBatch>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let announcements = self.get_announcements().await;
        if announcements.is_empty() {
            return None;
        }

        let mut batch = SkillAnnouncementBatch::new(self.local_peer_id.clone(), announcements);
        batch.sign(signer);
        Some(batch)
    }

    /// Handle an incoming skill announcement batch from the network.
    ///
    /// Validates that the batch is not expired, verifies the Ed25519 signature
    /// using the provided verifier, then registers each announced skill.
    /// Ignores announcements from ourselves and rejects unverified batches.
    ///
    /// The `verifier` closure receives `(peer_id, signing_bytes, signature)` and
    /// should return `true` if the signature is valid for that peer.
    pub async fn handle_announcement_batch<F>(&self, batch: &SkillAnnouncementBatch, verifier: F)
    where
        F: FnOnce(&str, &[u8], &[u8]) -> bool,
    {
        // Ignore our own announcements
        if batch.peer_id == self.local_peer_id {
            return;
        }

        // Reject stale announcements (older than 10 minutes)
        if batch.is_expired(600) {
            tracing::debug!(
                peer = %batch.peer_id,
                "Ignoring expired skill announcement batch"
            );
            return;
        }

        // Verify the batch signature
        let signing_bytes = batch.signing_bytes();
        if !verifier(&batch.peer_id, &signing_bytes, &batch.signature) {
            tracing::warn!(
                peer = %batch.peer_id,
                "Rejected skill announcement batch: invalid signature"
            );
            return;
        }

        tracing::info!(
            peer = %batch.peer_id,
            count = batch.skills.len(),
            "Received verified skill announcements from network"
        );

        for announcement in &batch.skills {
            self.register_network_skill(announcement.clone()).await;
        }
    }
}

/// Skill information for listing.
#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub trust: SkillTrust,
    pub available: bool,
    pub provider: String,
    pub price: u64,
    pub keywords: Vec<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
}

/// Error scanning skills directory.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("IO error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(#[from] ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_registry_creation() {
        let dir = tempdir().unwrap();
        let registry =
            SkillRegistry::new(dir.path().to_path_buf(), "test-peer".to_string()).unwrap();

        let skills = registry.list_local().await;
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn test_install_skill() {
        let dir = tempdir().unwrap();
        let registry =
            SkillRegistry::new(dir.path().to_path_buf(), "test-peer".to_string()).unwrap();

        let content = r#"---
name: test-skill
version: 1.0.0
description: A test skill
---

# Test Skill

This is a test skill.
"#;

        let skill = registry.install(content, SkillTrust::Local).await.unwrap();
        assert_eq!(skill.name(), "test-skill");

        let retrieved = registry.get("test-skill").await;
        assert!(retrieved.is_some());
    }
}
