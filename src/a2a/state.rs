//! Shared in-memory state for A2A tasks and discovered peer cards.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::agent_card::AgentCard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum A2aTaskStatus {
    Working,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2aTaskRecord {
    pub id: String,
    pub status: A2aTaskStatus,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<serde_json::Value>,
}

/// Process-wide A2A state (HTTP + libp2p RR).
pub struct A2aState {
    tasks: RwLock<HashMap<String, A2aTaskRecord>>,
    /// Remote peer_id -> latest card
    peer_cards: RwLock<HashMap<String, AgentCard>>,
    announce_epoch: AtomicU64,
}

impl A2aState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tasks: RwLock::new(HashMap::new()),
            peer_cards: RwLock::new(HashMap::new()),
            announce_epoch: AtomicU64::new(0),
        })
    }

    pub fn next_epoch(&self) -> u64 {
        self.announce_epoch.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn upsert_peer_card(&self, peer_id: impl Into<String>, card: AgentCard) {
        self.peer_cards.write().insert(peer_id.into(), card);
    }

    pub fn get_peer_card(&self, peer_id: &str) -> Option<AgentCard> {
        self.peer_cards.read().get(peer_id).cloned()
    }

    pub fn list_peer_cards(&self) -> Vec<(String, AgentCard)> {
        self.peer_cards
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn create_task(&self, initial_message: impl Into<String>) -> A2aTaskRecord {
        let id = Uuid::new_v4().to_string();
        let rec = A2aTaskRecord {
            id: id.clone(),
            status: A2aTaskStatus::Working,
            message: initial_message.into(),
            artifact: None,
        };
        self.tasks.write().insert(id.clone(), rec.clone());
        rec
    }

    pub fn get_task(&self, id: &str) -> Option<A2aTaskRecord> {
        self.tasks.read().get(id).cloned()
    }

    pub fn complete_task(&self, id: &str, artifact: Option<serde_json::Value>) -> bool {
        let mut g = self.tasks.write();
        if let Some(t) = g.get_mut(id) {
            t.status = A2aTaskStatus::Completed;
            t.artifact = artifact;
            true
        } else {
            false
        }
    }

    pub fn fail_task(&self, id: &str, err: impl Into<String>) -> bool {
        let mut g = self.tasks.write();
        if let Some(t) = g.get_mut(id) {
            t.status = A2aTaskStatus::Failed;
            t.message = err.into();
            true
        } else {
            false
        }
    }

    pub fn cancel_task(&self, id: &str) -> bool {
        let mut g = self.tasks.write();
        if let Some(t) = g.get_mut(id) {
            t.status = A2aTaskStatus::Canceled;
            true
        } else {
            false
        }
    }
}

impl Default for A2aState {
    fn default() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            peer_cards: RwLock::new(HashMap::new()),
            announce_epoch: AtomicU64::new(0),
        }
    }
}
