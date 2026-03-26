//! Channel state and signed updates.

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier};
use serde::{Deserialize, Serialize};

use super::ChannelId;
use crate::identity::NodeIdentity;

/// A channel state update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelUpdate {
    /// Channel this update applies to
    pub channel_id: ChannelId,
    /// Monotonically increasing nonce
    pub nonce: u64,
    /// Local peer's new balance
    pub local_balance: u64,
    /// Remote peer's new balance
    pub remote_balance: u64,
    /// When this update was created
    pub timestamp: DateTime<Utc>,
}

impl ChannelUpdate {
    /// Create a new channel update.
    pub fn new(channel_id: ChannelId, nonce: u64, local_balance: u64, remote_balance: u64) -> Self {
        Self {
            channel_id,
            nonce,
            local_balance,
            remote_balance,
            timestamp: Utc::now(),
        }
    }

    /// Serialize the update for signing.
    pub fn to_signing_bytes(&self) -> Vec<u8> {
        // Create a canonical representation for signing
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.channel_id.0.as_bytes());
        bytes.extend_from_slice(&self.nonce.to_le_bytes());
        bytes.extend_from_slice(&self.local_balance.to_le_bytes());
        bytes.extend_from_slice(&self.remote_balance.to_le_bytes());
        bytes.extend_from_slice(&self.timestamp.timestamp().to_le_bytes());
        bytes
    }

    /// Sign this update with the given identity.
    pub fn sign(&self, identity: &NodeIdentity) -> SignedUpdate {
        let bytes = self.to_signing_bytes();
        let signature = identity.sign(&bytes);

        SignedUpdate {
            update: self.clone(),
            signature: hex::encode(signature.to_bytes()),
            signer: identity.peer_id().to_string(),
        }
    }
}

/// A signed channel update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedUpdate {
    /// The update being signed
    pub update: ChannelUpdate,
    /// Hex-encoded Ed25519 signature
    pub signature: String,
    /// Peer ID of the signer
    pub signer: String,
}

impl SignedUpdate {
    /// Verify the signature against a node identity (requires private key access).
    pub fn verify(&self, verifier: &NodeIdentity) -> bool {
        self.verify_with_public_key(&verifier.public_key())
    }

    /// Verify the signature using only a public key (for remote peer verification).
    pub fn verify_with_public_key(&self, public_key: &ed25519_dalek::VerifyingKey) -> bool {
        let bytes = self.update.to_signing_bytes();

        // Decode signature
        let sig_bytes = match hex::decode(&self.signature) {
            Ok(b) if b.len() == 64 => b,
            _ => return false,
        };

        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);

        let sig = Signature::from_bytes(&sig_array);
        public_key.verify(&bytes, &sig).is_ok()
    }

    /// Verify the signature using hex-encoded public key bytes.
    pub fn verify_with_hex_key(&self, hex_public_key: &str) -> bool {
        let key_bytes = match hex::decode(hex_public_key) {
            Ok(b) if b.len() == 32 => b,
            _ => return false,
        };

        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);

        match ed25519_dalek::VerifyingKey::from_bytes(&key_array) {
            Ok(public_key) => self.verify_with_public_key(&public_key),
            Err(_) => false,
        }
    }
}

/// Complete state of a channel for settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelState {
    /// Channel ID
    pub channel_id: ChannelId,
    /// Opening transaction reference (for on-chain anchoring)
    pub opening_tx: Option<String>,
    /// Latest update signed by local peer
    pub local_signed: Option<SignedUpdate>,
    /// Latest update signed by remote peer
    pub remote_signed: Option<SignedUpdate>,
    /// Whether the channel has been disputed
    pub disputed: bool,
    /// Closing transaction reference
    pub closing_tx: Option<String>,
}

impl ChannelState {
    /// Create a new channel state.
    pub fn new(channel_id: ChannelId) -> Self {
        Self {
            channel_id,
            opening_tx: None,
            local_signed: None,
            remote_signed: None,
            disputed: false,
            closing_tx: None,
        }
    }

    /// Get the latest agreed-upon state (both parties signed).
    pub fn latest_agreed(&self) -> Option<&ChannelUpdate> {
        match (&self.local_signed, &self.remote_signed) {
            (Some(local), Some(remote)) => {
                // Return the one with higher nonce
                if local.update.nonce >= remote.update.nonce {
                    Some(&local.update)
                } else {
                    Some(&remote.update)
                }
            }
            (Some(local), None) => Some(&local.update),
            (None, Some(remote)) => Some(&remote.update),
            (None, None) => None,
        }
    }

    /// Check if channel can be settled.
    pub fn can_settle(&self) -> bool {
        // Need at least one signed state
        self.local_signed.is_some() || self.remote_signed.is_some()
    }

    /// Update with a new local signature.
    pub fn update_local(&mut self, signed: SignedUpdate) {
        self.local_signed = Some(signed);
    }

    /// Update with a new remote signature.
    pub fn update_remote(&mut self, signed: SignedUpdate) {
        self.remote_signed = Some(signed);
    }

    /// Mark channel as disputed.
    pub fn dispute(&mut self) {
        self.disputed = true;
    }
}

/// A signed request to close a channel (for cooperative close protocol).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCloseRequest {
    /// Channel being closed.
    pub channel_id: ChannelId,
    /// Final local balance proposed by the closer.
    pub final_local_balance: u64,
    /// Final remote balance proposed by the closer.
    pub final_remote_balance: u64,
    /// Nonce at close time.
    pub nonce: u64,
    /// Hex-encoded Ed25519 signature of the close request.
    pub signature: String,
    /// Peer ID of the requester.
    pub requester: String,
    /// Timestamp of the close request.
    pub timestamp: DateTime<Utc>,
}

impl ChannelCloseRequest {
    /// Create and sign a close request from the current channel state.
    pub fn new(
        channel_id: ChannelId,
        local_balance: u64,
        remote_balance: u64,
        nonce: u64,
        identity: &NodeIdentity,
    ) -> Self {
        let timestamp = Utc::now();
        let mut req = Self {
            channel_id,
            final_local_balance: local_balance,
            final_remote_balance: remote_balance,
            nonce,
            signature: String::new(),
            requester: identity.peer_id().to_string(),
            timestamp,
        };
        let bytes = req.signing_bytes();
        req.signature = hex::encode(identity.sign(&bytes).to_bytes());
        req
    }

    /// Canonical bytes for signing.
    fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"close:");
        bytes.extend_from_slice(self.channel_id.0.as_bytes());
        bytes.extend_from_slice(&self.final_local_balance.to_le_bytes());
        bytes.extend_from_slice(&self.final_remote_balance.to_le_bytes());
        bytes.extend_from_slice(&self.nonce.to_le_bytes());
        bytes.extend_from_slice(&self.timestamp.timestamp().to_le_bytes());
        bytes
    }

    /// Verify the close request signature using a hex-encoded public key.
    pub fn verify(&self, hex_public_key: &str) -> bool {
        let key_bytes = match hex::decode(hex_public_key) {
            Ok(b) if b.len() == 32 => b,
            _ => return false,
        };
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);

        let public_key = match ed25519_dalek::VerifyingKey::from_bytes(&key_array) {
            Ok(pk) => pk,
            Err(_) => return false,
        };

        let sig_bytes = match hex::decode(&self.signature) {
            Ok(b) if b.len() == 64 => b,
            _ => return false,
        };
        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);

        let sig = Signature::from_bytes(&sig_array);
        public_key.verify(&self.signing_bytes(), &sig).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_update_signing() {
        let identity = NodeIdentity::generate();

        let update = ChannelUpdate::new(ChannelId::new(), 1, 90_000_000, 10_000_000);

        let signed = update.sign(&identity);

        // Verify with same identity
        assert!(signed.verify(&identity));
    }

    #[test]
    fn test_channel_update_tamper_detection() {
        let identity = NodeIdentity::generate();

        let update = ChannelUpdate::new(ChannelId::new(), 1, 90_000_000, 10_000_000);

        let mut signed = update.sign(&identity);

        // Tamper with the update
        signed.update.local_balance = 100_000_000;

        // Verification should fail
        assert!(!signed.verify(&identity));
    }

    #[test]
    fn test_channel_state() {
        let identity = NodeIdentity::generate();

        let channel_id = ChannelId::new();
        let mut state = ChannelState::new(channel_id.clone());

        assert!(state.latest_agreed().is_none());
        assert!(!state.can_settle());

        // Add a local signed update
        let update = ChannelUpdate::new(channel_id, 1, 90_000_000, 10_000_000);
        let signed = update.sign(&identity);
        state.update_local(signed);

        assert!(state.latest_agreed().is_some());
        assert!(state.can_settle());
    }

    #[test]
    fn test_verify_with_public_key() {
        let identity = NodeIdentity::generate();
        let update = ChannelUpdate::new(ChannelId::new(), 1, 90_000_000, 10_000_000);
        let signed = update.sign(&identity);

        // Verify using public key directly
        assert!(signed.verify_with_public_key(&identity.public_key()));

        // Verify using hex-encoded public key
        let hex_key = hex::encode(identity.public_key_bytes());
        assert!(signed.verify_with_hex_key(&hex_key));

        // Wrong key should fail
        let other = NodeIdentity::generate();
        assert!(!signed.verify_with_public_key(&other.public_key()));
    }

    #[test]
    fn test_close_request_signing() {
        let identity = NodeIdentity::generate();
        let channel_id = ChannelId::new();

        let close_req = ChannelCloseRequest::new(
            channel_id,
            90_000_000,
            10_000_000,
            5,
            &identity,
        );

        let hex_key = hex::encode(identity.public_key_bytes());
        assert!(close_req.verify(&hex_key));

        // Wrong key should fail
        let other = NodeIdentity::generate();
        let wrong_key = hex::encode(other.public_key_bytes());
        assert!(!close_req.verify(&wrong_key));
    }
}
