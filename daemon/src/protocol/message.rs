use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Generate a new random UUID (used as serde default for Message.id).
fn new_uuid() -> Uuid {
    Uuid::new_v4()
}

/// Protocol message envelope — all communication uses this format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Protocol version
    pub version: String,

    /// Message type
    #[serde(rename = "type")]
    pub msg_type: MessageType,

    /// Sender identifier (agent name for now, DID later)
    pub from: String,

    /// Message body
    pub body: String,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Unique message identifier.
    #[serde(default = "new_uuid")]
    pub id: Uuid,

    /// If this message is a reply, the id of the parent message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Uuid>,

    /// Conversation thread identifier — groups related messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<Uuid>,

    /// Sender's DID (e.g. `did:agora:<base58-pubkey>`). Present in Hello messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did: Option<String>,

    /// Sender's Ed25519 public key (base58-encoded). Present in Hello messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,

    /// Per-process session ID — distinguishes concurrent instances of the same agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,

    /// Ed25519 signature of the message body (base58-encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// Owner DID (e.g. `did:agora:owner:<base58-pubkey>`). Present when agent has an owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_did: Option<String>,

    /// Owner attestation — cryptographic proof binding owner to agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_attestation: Option<crate::identity::OwnerAttestation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Hello,
    Message,
    Heartbeat,
    Close,

    // --- Thread/sub-group types ---
    /// Create a new conversation thread / sub-group
    #[serde(rename = "thread.create")]
    ThreadCreate,
    /// A message within a thread
    #[serde(rename = "thread.message")]
    ThreadMessage,
    /// Update thread metadata or participants
    #[serde(rename = "thread.update")]
    ThreadUpdate,
    /// Close a thread — no more messages or participant changes
    #[serde(rename = "thread.close")]
    ThreadClose,

    // --- Friend request types ---
    /// Send a friend request to a peer
    #[serde(rename = "friend.request")]
    FriendRequest,
    /// Accept a friend request
    #[serde(rename = "friend.accept")]
    FriendAccept,
    /// Reject a friend request
    #[serde(rename = "friend.reject")]
    FriendReject,
    /// Revoke an existing friendship
    #[serde(rename = "friend.revoke")]
    FriendRevoke,

    // --- Project collaboration types ---
    /// Invite a peer to join a project
    #[serde(rename = "project.invite")]
    ProjectInvite,
    /// Accept a project invitation
    #[serde(rename = "project.accept")]
    ProjectAccept,
    /// Decline a project invitation
    #[serde(rename = "project.decline")]
    ProjectDecline,
    /// Leave a project
    #[serde(rename = "project.leave")]
    ProjectLeave,
    /// Update project metadata (status, description, etc.)
    #[serde(rename = "project.update")]
    ProjectUpdate,
    /// Clock in to a project (signal you're actively working)
    #[serde(rename = "project.clock_in")]
    ProjectClockIn,
    /// Clock out of a project
    #[serde(rename = "project.clock_out")]
    ProjectClockOut,

    // --- Task types ---
    /// Assign/create a task in a project
    #[serde(rename = "task.assign")]
    TaskAssign,
    /// Update a task's status, description, etc.
    #[serde(rename = "task.update")]
    TaskUpdate,
    /// Mark a task as complete
    #[serde(rename = "task.complete")]
    TaskComplete,

    // --- Stage types ---
    /// Update a project's lifecycle stage
    #[serde(rename = "project.stage")]
    ProjectStage,

    /// Replicate an audit trail entry to peers
    #[serde(rename = "project.audit")]
    AuditEntry,

    /// Suspend an agent in a project
    #[serde(rename = "project.suspend")]
    ProjectSuspend,

    /// Unsuspend an agent in a project
    #[serde(rename = "project.unsuspend")]
    ProjectUnsuspend,

    /// Delivery acknowledgement for offline message queue
    Ack,

    // --- Marketplace types ---
    /// Advertise agent capabilities
    #[serde(rename = "marketplace.advertise")]
    CapabilityAdvertise,
    /// Search for agents by capability
    #[serde(rename = "marketplace.search")]
    AgentSearch,
    /// Search results returned
    #[serde(rename = "marketplace.search_result")]
    AgentSearchResult,

    // --- Reputation types ---
    /// Broadcast a reputation update
    #[serde(rename = "reputation.update")]
    ReputationUpdate,

    // --- Coordinator types ---
    /// Coordinator project digest
    #[serde(rename = "coordinator.digest")]
    CoordinatorDigest,
    /// Coordinator suggestion
    #[serde(rename = "coordinator.suggestion")]
    CoordinatorSuggestion,

    // --- Gossip / Discovery types ---
    /// Exchange signed capability entries with a peer
    #[serde(rename = "gossip.capabilities")]
    GossipCapabilities,
    /// Friend-of-friend introduction
    #[serde(rename = "gossip.introduction")]
    GossipIntroduction,
    /// Advertise a project's open roles to the network
    #[serde(rename = "gossip.project_ad")]
    GossipProjectAd,
    /// Request discovery sync from a peer
    #[serde(rename = "gossip.sync_request")]
    GossipSyncRequest,
    /// Response to a discovery sync request
    #[serde(rename = "gossip.sync_response")]
    GossipSyncResponse,

    /// Forward compatibility — older peers ignore unknown message types.
    #[serde(other)]
    Unknown,
}

impl MessageType {
    /// Whether this is a thread-related message type.
    pub fn is_thread(&self) -> bool {
        matches!(
            self,
            MessageType::ThreadCreate
                | MessageType::ThreadMessage
                | MessageType::ThreadUpdate
                | MessageType::ThreadClose
        )
    }

    /// Whether this is a project-related message type.
    pub fn is_project(&self) -> bool {
        matches!(
            self,
            MessageType::ProjectInvite
                | MessageType::ProjectAccept
                | MessageType::ProjectDecline
                | MessageType::ProjectLeave
                | MessageType::ProjectUpdate
                | MessageType::ProjectClockIn
                | MessageType::ProjectClockOut
                | MessageType::ProjectStage
                | MessageType::AuditEntry
                | MessageType::ProjectSuspend
                | MessageType::ProjectUnsuspend
        )
    }

    /// Whether this is a task-related message type.
    pub fn is_task(&self) -> bool {
        matches!(
            self,
            MessageType::TaskAssign | MessageType::TaskUpdate | MessageType::TaskComplete
        )
    }

    /// Whether this is a friend-request-related message type.
    pub fn is_friend(&self) -> bool {
        matches!(
            self,
            MessageType::FriendRequest
                | MessageType::FriendAccept
                | MessageType::FriendReject
                | MessageType::FriendRevoke
        )
    }

    /// Whether this is a gossip/discovery message type.
    pub fn is_gossip(&self) -> bool {
        matches!(
            self,
            MessageType::GossipCapabilities
                | MessageType::GossipIntroduction
                | MessageType::GossipProjectAd
                | MessageType::GossipSyncRequest
                | MessageType::GossipSyncResponse
        )
    }
}

// ---------------------------------------------------------------------------
// Thread payload types — serialized as JSON in the Message body field.
// See protocol/threads.md for the wire format spec.
// ---------------------------------------------------------------------------

/// Payload for thread.create messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadCreatePayload {
    pub conversation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participants: Vec<String>,
    #[serde(default)]
    pub min_trust: u8,
    #[serde(default)]
    pub closed: bool,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Payload for thread.update messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadUpdatePayload {
    pub conversation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_participants: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_participants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// Payload for thread.close messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadClosePayload {
    pub conversation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Friend request payload types — serialized as JSON in the Message body field.
// See protocol/friend-requests.md for the wire format spec.
// ---------------------------------------------------------------------------

/// Payload for friend.request messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRequestPayload {
    /// Sender's DID.
    pub did: String,
    /// Sender's Ed25519 public key (base58-encoded).
    pub public_key: String,
    /// Trust level the sender is offering us (what they'll assign us).
    pub trust_level: u8,
    /// Optional human-readable message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Sender's node name.
    pub node_name: String,
    /// Sender's owner DID (if they have one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_did: Option<String>,
}

/// Payload for friend.accept messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendAcceptPayload {
    /// Accepter's DID.
    pub did: String,
    /// Trust level the accepter is assigning to the requester.
    pub trust_level: u8,
    /// Optional human-readable message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Payload for friend.reject messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRejectPayload {
    /// Optional reason for rejection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for friend.revoke messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRevokePayload {
    /// Optional reason for revocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Project payload types — serialized as JSON in the Message body field.
// ---------------------------------------------------------------------------

/// Payload for project.invite messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInvitePayload {
    pub project_id: Uuid,
    pub project_name: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<crate::project::ProjectContext>,
}

/// Payload for project.accept messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAcceptPayload {
    pub project_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Payload for project.decline messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDeclinePayload {
    pub project_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for project.leave messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLeavePayload {
    pub project_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for project.update messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUpdatePayload {
    pub project_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Payload for project.clock_in messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectClockInPayload {
    pub project_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

/// Payload for project.clock_out messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectClockOutPayload {
    pub project_id: Uuid,
}

// ---------------------------------------------------------------------------
// Task payload types — serialized as JSON in the Message body field.
// ---------------------------------------------------------------------------

/// Payload for task.assign messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignPayload {
    pub project_id: Uuid,
    pub task_id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<Uuid>,
}

/// Payload for task.update messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdatePayload {
    pub project_id: Uuid,
    pub task_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
}

/// Payload for task.complete messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletePayload {
    pub project_id: Uuid,
    pub task_id: Uuid,
    /// IDs of tasks that were auto-unblocked by completing this task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unblocked_task_ids: Vec<Uuid>,
}

// ---------------------------------------------------------------------------
// Stage payload types — serialized as JSON in the Message body field.
// ---------------------------------------------------------------------------

/// Payload for project.stage messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStagePayload {
    pub project_id: Uuid,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_stage: Option<String>,
}

// ---------------------------------------------------------------------------
// Audit + Oversight payload types
// ---------------------------------------------------------------------------

/// Payload for project.audit messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntryPayload {
    pub project_id: Uuid,
    pub entry: crate::project::AuditEntry,
}

/// Payload for project.suspend messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSuspendPayload {
    pub project_id: Uuid,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Payload for project.unsuspend messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectUnsuspendPayload {
    pub project_id: Uuid,
    pub target: String,
}

/// Payload for ack messages — confirms delivery of a specific message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckPayload {
    /// The ID of the message being acknowledged.
    pub message_id: Uuid,
}

// ---------------------------------------------------------------------------
// Message constructors
// ---------------------------------------------------------------------------

impl Message {
    /// Hello without identity — for backward compatibility with legacy peers.
    #[allow(dead_code)]
    pub fn hello(from: &str) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::Hello,
            from: from.to_string(),
            body: format!("Hello from {}", from),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Hello message with cryptographic identity attached.
    pub fn hello_with_identity(
        from: &str,
        identity: &crate::identity::AgentIdentity,
        owner_attestation: Option<&crate::identity::OwnerAttestation>,
    ) -> Self {
        let body = format!("Hello from {}", from);
        let sig = identity.sign(body.as_bytes());
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::Hello,
            from: from.to_string(),
            body,
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: Some(identity.did().to_string()),
            public_key: Some(identity.public_key_base58()),
            session_id: Some(identity.session_id()),
            signature: Some(bs58::encode(&sig).into_string()),
            owner_did: owner_attestation.map(|a| a.owner_did.clone()),
            owner_attestation: owner_attestation.cloned(),
        }
    }

    pub fn text(from: &str, body: &str) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::Message,
            from: from.to_string(),
            body: body.to_string(),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    #[allow(dead_code)]
    pub fn heartbeat(from: &str) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::Heartbeat,
            from: from.to_string(),
            body: String::new(),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    pub fn close(from: &str) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::Close,
            from: from.to_string(),
            body: "Goodbye".to_string(),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a thread.create message.
    pub fn thread_create(from: &str, payload: &ThreadCreatePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ThreadCreate,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ThreadCreatePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: Some(payload.conversation_id),
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a thread.update message.
    pub fn thread_update(from: &str, payload: &ThreadUpdatePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ThreadUpdate,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ThreadUpdatePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: Some(payload.conversation_id),
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a thread.close message.
    pub fn thread_close(from: &str, payload: &ThreadClosePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ThreadClose,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ThreadClosePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: Some(payload.conversation_id),
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a friend.request message.
    pub fn friend_request(from: &str, payload: &FriendRequestPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::FriendRequest,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("FriendRequestPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a friend.accept message.
    pub fn friend_accept(from: &str, payload: &FriendAcceptPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::FriendAccept,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("FriendAcceptPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a friend.reject message.
    #[allow(dead_code)]
    pub fn friend_reject(from: &str, payload: &FriendRejectPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::FriendReject,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("FriendRejectPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a friend.revoke message.
    #[allow(dead_code)]
    pub fn friend_revoke(from: &str, payload: &FriendRevokePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::FriendRevoke,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("FriendRevokePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.invite message.
    pub fn project_invite(from: &str, payload: &ProjectInvitePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectInvite,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectInvitePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.accept message.
    pub fn project_accept(from: &str, payload: &ProjectAcceptPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectAccept,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectAcceptPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.decline message.
    pub fn project_decline(from: &str, payload: &ProjectDeclinePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectDecline,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectDeclinePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.leave message.
    pub fn project_leave(from: &str, payload: &ProjectLeavePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectLeave,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectLeavePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.update message.
    pub fn project_update(from: &str, payload: &ProjectUpdatePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectUpdate,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectUpdatePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.clock_in message.
    pub fn project_clock_in(from: &str, payload: &ProjectClockInPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectClockIn,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectClockInPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.clock_out message.
    pub fn project_clock_out(from: &str, payload: &ProjectClockOutPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectClockOut,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectClockOutPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a task.assign message.
    pub fn task_assign(from: &str, payload: &TaskAssignPayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::TaskAssign,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("TaskAssignPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a task.update message.
    pub fn task_update(from: &str, payload: &TaskUpdatePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::TaskUpdate,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("TaskUpdatePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a task.complete message.
    pub fn task_complete(from: &str, payload: &TaskCompletePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::TaskComplete,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("TaskCompletePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create a project.stage message.
    pub fn project_stage(from: &str, payload: &ProjectStagePayload) -> Self {
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::ProjectStage,
            from: from.to_string(),
            body: serde_json::to_string(payload).expect("ProjectStagePayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Create an ack message confirming delivery of a specific message.
    pub fn ack(from: &str, message_id: Uuid) -> Self {
        let payload = AckPayload { message_id };
        Self {
            version: "0.1.0".to_string(),
            msg_type: MessageType::Ack,
            from: from.to_string(),
            body: serde_json::to_string(&payload).expect("AckPayload serialization"),
            timestamp: Utc::now(),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            did: None,
            public_key: None,
            session_id: None,
            signature: None,
            owner_did: None,
            owner_attestation: None,
        }
    }

    /// Parse the body as a typed payload. Returns None if parsing fails.
    pub fn parse_payload<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_str(&self.body).ok()
    }

    /// Verify this message's Ed25519 signature against the embedded public key.
    /// Returns true if signature is present and valid, false otherwise.
    pub fn verify_signature(&self) -> bool {
        let (Some(pk_b58), Some(sig_b58)) = (&self.public_key, &self.signature) else {
            return false;
        };
        crate::identity::AgentIdentity::verify_base58(pk_b58, self.body.as_bytes(), sig_b58)
    }
}
