//! Gossip messages for distributed crew task market and pods/world campaigns.

use libp2p::identity::PublicKey;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};

use crate::identity::NodeIdentity;

pub const CREW_TASK_TOPIC: &str = "peerclaw/crew/v1";
pub const POD_TOPIC: &str = "peerclaw/pod/v1";

const IDENTITY_MULTIHASH_CODE: u64 = 0;

/// Build campaign world topic for sharded fan-in.
pub fn world_topic(campaign_id: &str) -> String {
    format!("peerclaw/world/{campaign_id}/v1")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewTaskOffer {
    pub run_id: String,
    pub task_id: String,
    #[serde(default)]
    pub pod_id: String,
    pub orchestrator_peer: String,
    #[serde(default)]
    pub model_hint: String,
    pub summary: String,
    pub expires_at_ms: u64,
    /// Ed25519 over [`CrewTaskOffer::signable_bytes`] (orchestrator identity key).
    #[serde(default)]
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewTaskClaim {
    pub offer_run_id: String,
    pub offer_task_id: String,
    pub worker_peer: String,
    #[serde(default)]
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewTaskResult {
    pub run_id: String,
    pub task_id: String,
    pub worker_peer: String,
    pub output_summary: String,
    pub success: bool,
    #[serde(default)]
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodArtifactPublished {
    pub pod_id: String,
    pub campaign_id: String,
    pub artifact_id: String,
    pub summary: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignMilestone {
    pub campaign_id: String,
    pub summary: String,
    pub reporter_peer: String,
}

impl CrewTaskOffer {
    pub fn new(
        run_id: impl Into<String>,
        task_id: impl Into<String>,
        orchestrator: &PeerId,
        summary: impl Into<String>,
        ttl_ms: u64,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            run_id: run_id.into(),
            task_id: task_id.into(),
            pod_id: String::new(),
            orchestrator_peer: orchestrator.to_string(),
            model_hint: String::new(),
            summary: summary.into(),
            expires_at_ms: now.saturating_add(ttl_ms),
            signature: Vec::new(),
        }
    }

    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"peerclaw:crew_offer:v1:");
        b.extend_from_slice(self.run_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.task_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.pod_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.orchestrator_peer.as_bytes());
        b.push(0);
        b.extend_from_slice(self.model_hint.as_bytes());
        b.push(0);
        b.extend_from_slice(self.summary.as_bytes());
        b.push(0);
        b.extend_from_slice(self.expires_at_ms.to_string().as_bytes());
        b
    }

    pub fn sign(&mut self, id: &NodeIdentity) {
        let payload = self.signable_bytes();
        self.signature = id.sign(&payload).to_bytes().to_vec();
    }

    /// Gossip source must match `orchestrator_peer`; optional Ed25519 when `signature` is non-empty.
    pub fn verify_source(&self, source: &PeerId) -> bool {
        if self.orchestrator_peer != source.to_string() {
            return false;
        }
        if self.signature.is_empty() {
            return true;
        }
        let mh = source.as_ref();
        if mh.code() != IDENTITY_MULTIHASH_CODE {
            tracing::debug!("crew offer: hashed PeerId — using transport binding only");
            return true;
        }
        match PublicKey::try_decode_protobuf(mh.digest()) {
            Ok(pk) => pk.verify(&self.signable_bytes(), &self.signature),
            Err(_) => false,
        }
    }
}

impl CrewTaskClaim {
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"peerclaw:crew_claim:v1:");
        b.extend_from_slice(self.offer_run_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.offer_task_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.worker_peer.as_bytes());
        b
    }

    pub fn sign(&mut self, id: &NodeIdentity) {
        let payload = self.signable_bytes();
        self.signature = id.sign(&payload).to_bytes().to_vec();
    }

    pub fn verify_worker(&self, source: &PeerId) -> bool {
        if self.worker_peer != source.to_string() {
            return false;
        }
        if self.signature.is_empty() {
            return true;
        }
        let mh = source.as_ref();
        if mh.code() != IDENTITY_MULTIHASH_CODE {
            return true;
        }
        match PublicKey::try_decode_protobuf(mh.digest()) {
            Ok(pk) => pk.verify(&self.signable_bytes(), &self.signature),
            Err(_) => false,
        }
    }
}

impl CrewTaskResult {
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"peerclaw:crew_result:v1:");
        b.extend_from_slice(self.run_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.task_id.as_bytes());
        b.push(0);
        b.extend_from_slice(self.worker_peer.as_bytes());
        b.push(0);
        b.extend_from_slice(self.output_summary.as_bytes());
        b.push(0);
        b.extend_from_slice(if self.success { b"1" } else { b"0" });
        b
    }

    pub fn sign(&mut self, id: &NodeIdentity) {
        let payload = self.signable_bytes();
        self.signature = id.sign(&payload).to_bytes().to_vec();
    }

    pub fn verify_worker(&self, source: &PeerId) -> bool {
        if self.worker_peer != source.to_string() {
            return false;
        }
        if self.signature.is_empty() {
            return true;
        }
        let mh = source.as_ref();
        if mh.code() != IDENTITY_MULTIHASH_CODE {
            return true;
        }
        match PublicKey::try_decode_protobuf(mh.digest()) {
            Ok(pk) => pk.verify(&self.signable_bytes(), &self.signature),
            Err(_) => false,
        }
    }
}
