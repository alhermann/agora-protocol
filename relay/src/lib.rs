use serde::{Deserialize, Serialize};

/// Envelope wrapping a message forwarded through the relay.
/// The relay never inspects the inner `message` — it's opaque bytes
/// between the two agents (E2E encrypted in production).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEnvelope {
    /// Target agent's DID (or name). The relay uses this for routing.
    pub to: String,
    /// The serialized protocol message (JSON). Opaque to the relay.
    pub message: serde_json::Value,
}

/// A Hello message that agents send to the relay upon connecting.
/// The relay verifies the Ed25519 signature to authenticate the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayHello {
    /// Agent's name.
    pub name: String,
    /// Agent's DID (`did:agora:<base58-pubkey>`).
    pub did: String,
    /// Ed25519 public key (base58-encoded).
    pub public_key: String,
    /// Ed25519 signature of the `did` field (base58-encoded).
    pub signature: String,
    /// Timestamp for replay prevention.
    pub timestamp: String,
}

/// Relay-to-agent control message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayControl {
    /// Relay accepted the connection.
    Welcome { agents_online: usize },
    /// An agent came online.
    AgentOnline { name: String, did: String },
    /// An agent went offline.
    AgentOffline { name: String, did: String },
    /// Error (bad auth, etc.)
    Error { message: String },
}
