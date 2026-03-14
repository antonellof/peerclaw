//! Network events for external consumption.

use libp2p::{Multiaddr, PeerId};

/// Events emitted by the P2P network layer.
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A peer was discovered via mDNS or DHT.
    PeerDiscovered {
        peer_id: PeerId,
        addresses: Vec<Multiaddr>,
    },

    /// A connection was established with a peer.
    PeerConnected(PeerId),

    /// A connection was closed with a peer.
    PeerDisconnected(PeerId),

    /// A message was received via GossipSub.
    GossipMessage {
        topic: String,
        data: Vec<u8>,
        source: Option<PeerId>,
    },

    /// A request was received from a peer.
    RequestReceived {
        request_id: String,
        from: PeerId,
        payload: Vec<u8>,
    },

    /// A resource manifest was received from a peer.
    ResourceAdvertised {
        peer_id: PeerId,
        manifest: super::ResourceManifest,
    },
}

impl std::fmt::Display for NetworkEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkEvent::PeerDiscovered { peer_id, addresses } => {
                write!(f, "PeerDiscovered({}, {} addrs)", peer_id, addresses.len())
            }
            NetworkEvent::PeerConnected(peer_id) => {
                write!(f, "PeerConnected({})", peer_id)
            }
            NetworkEvent::PeerDisconnected(peer_id) => {
                write!(f, "PeerDisconnected({})", peer_id)
            }
            NetworkEvent::GossipMessage { topic, data, .. } => {
                write!(f, "GossipMessage({}, {} bytes)", topic, data.len())
            }
            NetworkEvent::RequestReceived { from, payload, .. } => {
                write!(f, "RequestReceived({}, {} bytes)", from, payload.len())
            }
            NetworkEvent::ResourceAdvertised { peer_id, .. } => {
                write!(f, "ResourceAdvertised({})", peer_id)
            }
        }
    }
}
