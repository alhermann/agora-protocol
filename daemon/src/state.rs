use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, Notify, broadcast};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::identity::{AgentIdentity, OwnerAttestation};
use crate::protocol::message::Message;
use crate::thread::{ThreadError, ThreadManager, ThreadSummary};

// ---------------------------------------------------------------------------
// Conversation history limits
// ---------------------------------------------------------------------------

/// Maximum messages kept per conversation_id (or per the "no-conversation" bucket).
/// When exceeded, the oldest messages for that conversation are evicted first.
const MAX_MESSAGES_PER_CONVERSATION: usize = 500;

/// Global safety cap across all conversations to bound total memory usage.
const MAX_MESSAGES_GLOBAL: usize = 5000;

/// Trim conversation history so that no single conversation exceeds
/// `MAX_MESSAGES_PER_CONVERSATION` messages. If the global count still exceeds
/// `MAX_MESSAGES_GLOBAL` after per-conversation trimming, evict the oldest
/// messages globally (across all conversations) until the cap is met.
fn trim_conversation_history(history: &mut Vec<StoredMessage>) {
    // Phase 1: per-conversation trim
    // Count messages per conversation_id (None = legacy/untagged bucket)
    let mut counts: HashMap<Option<&str>, usize> = HashMap::new();
    for msg in history.iter() {
        let key = msg.conversation_id.as_deref();
        *counts.entry(key).or_insert(0) += 1;
    }

    // Find conversations over the limit
    let over_limit: HashSet<Option<&str>> = counts
        .iter()
        .filter(|(_, count)| **count > MAX_MESSAGES_PER_CONVERSATION)
        .map(|(&key, _)| key)
        .collect();

    if !over_limit.is_empty() {
        // For each over-limit conversation, keep only the newest MAX_MESSAGES_PER_CONVERSATION.
        // We iterate in reverse (newest first) and count per-conversation.
        let mut keep_counts: HashMap<Option<&str>, usize> = HashMap::new();
        let mut keep = vec![true; history.len()];

        for (i, msg) in history.iter().enumerate().rev() {
            let key = msg.conversation_id.as_deref();
            if over_limit.contains(&key) {
                let count = keep_counts.entry(key).or_insert(0);
                *count += 1;
                if *count > MAX_MESSAGES_PER_CONVERSATION {
                    keep[i] = false;
                }
            }
        }

        let mut idx = 0;
        history.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });
    }

    // Phase 2: global cap (safety valve)
    while history.len() > MAX_MESSAGES_GLOBAL {
        history.remove(0);
    }
}

// ---------------------------------------------------------------------------
// Consumer types for fan-out inbox
// ---------------------------------------------------------------------------

/// Unique identifier for an inbox consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ConsumerId(pub u64);

impl std::fmt::Display for ConsumerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Simple per-consumer token-bucket rate limiter.
struct ConsumerRateLimiter {
    tokens: u32,
    max_tokens: u32,
    last_refill: std::time::Instant,
    refill_interval: Duration,
}

impl ConsumerRateLimiter {
    fn new(max_tokens: u32, refill_interval: Duration) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            last_refill: std::time::Instant::now(),
            refill_interval,
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        if elapsed >= self.refill_interval {
            let refills = (elapsed.as_millis() / self.refill_interval.as_millis()) as u32;
            self.tokens = self
                .max_tokens
                .min(self.tokens.saturating_add(refills * self.max_tokens));
            self.last_refill = now;
        }
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

/// A registered consumer slot — each consumer gets its own message buffer.
struct ConsumerSlot {
    buffer: VecDeque<Message>,
    label: String,
    registered_at: chrono::DateTime<chrono::Utc>,
    last_active: chrono::DateTime<chrono::Utc>,
    notify: Arc<Notify>,
    /// Whether this consumer should suppress the wake hook when active.
    /// Only explicitly registered consumers (e.g., MCP monitor) suppress wake.
    /// The lazy "http-default" consumer does NOT suppress wake.
    suppresses_wake: bool,
    /// Per-consumer rate limiter: 100 requests/second.
    rate_limiter: ConsumerRateLimiter,
}

/// Public consumer info returned by the API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConsumerInfo {
    pub id: ConsumerId,
    pub label: String,
    pub registered_at: String,
    pub last_active: String,
    pub buffered_messages: usize,
    pub suppresses_wake: bool,
}

/// Snapshot of the daemon's wake readiness and recent wake activity.
#[derive(Debug, Clone)]
pub struct WakeStatusSnapshot {
    pub enabled: bool,
    pub armed: bool,
    pub active_listener_count: usize,
    pub active_listener_labels: Vec<String>,
    pub last_fired_at: Option<String>,
    pub last_fired_from: Option<String>,
    pub last_message_count: Option<usize>,
}

// ---------------------------------------------------------------------------
// Conversation threading types
// ---------------------------------------------------------------------------

/// A stored message for conversation history tracking.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredMessage {
    pub id: String,
    pub from: String,
    pub body: String,
    pub timestamp: String,
    pub reply_to: Option<String>,
    pub conversation_id: Option<String>,
    pub direction: String, // "inbound" or "outbound"
    /// Project ID if this message is related to a project operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Ed25519 signature of the message body (base58-encoded).
    /// Present for both outbound (signed by this daemon) and inbound
    /// (signed by the remote peer) messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Signer's Ed25519 public key (base58-encoded).
    /// Allows verification without looking up the signer's identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

/// Summary of a conversation thread.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub message_count: usize,
    pub participants: Vec<String>,
    pub first_message_at: String,
    pub last_message_at: String,
    pub preview: String,
}

// ---------------------------------------------------------------------------
// Trust Levels
// ---------------------------------------------------------------------------

/// Trust level for a friend (0–4).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct TrustLevel(pub u8);

impl TrustLevel {
    pub const UNKNOWN: TrustLevel = TrustLevel(0);
    pub const ACQUAINTANCE: TrustLevel = TrustLevel(1);
    pub const FRIEND: TrustLevel = TrustLevel(2);
    pub const TRUSTED: TrustLevel = TrustLevel(3);
    pub const INNER_CIRCLE: TrustLevel = TrustLevel(4);

    pub fn name(&self) -> &'static str {
        match self.0 {
            0 => "Unknown",
            1 => "Acquaintance",
            2 => "Friend",
            3 => "Trusted",
            4 => "Inner Circle",
            _ => "Invalid",
        }
    }

    /// Whether this trust level allows triggering the wake-up hook.
    pub fn can_wake(&self) -> bool {
        self.0 >= 3
    }
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.0, self.name())
    }
}

// ---------------------------------------------------------------------------
// Friend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Friend {
    pub name: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub trust_level: TrustLevel,
    pub added_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub notes: Option<String>,
    /// When true, messages from this friend will not trigger the wake hook.
    #[serde(default)]
    pub muted: bool,
    /// Last known network address (host:port) for auto-connect.
    #[serde(default)]
    pub last_address: Option<String>,
    /// Friend's verified DID (set after successful Hello with identity).
    #[serde(default)]
    pub did: Option<String>,
    /// Friend's owner DID (set when their Hello carries a verified owner attestation).
    #[serde(default)]
    pub owner_did: Option<String>,
    /// What trust level the remote side assigned us (from friend.accept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub their_trust: Option<u8>,
}

/// Partial update for a friend — only the fields that are `Some` get applied.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FriendPatch {
    pub trust_level: Option<u8>,
    pub alias: Option<String>,
    pub notes: Option<String>,
    pub muted: Option<bool>,
}

// ---------------------------------------------------------------------------
// FriendsStore — JSON-backed friend persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FriendsStore {
    path: PathBuf,
    friends: HashMap<String, Friend>,
}

impl FriendsStore {
    /// Default path: `~/.agora/friends.json`
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        crate::config::agora_home().join("friends.json")
    }

    /// Load from disk, or start empty if file doesn't exist.
    pub fn load(path: &Path) -> Self {
        let friends = if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => match serde_json::from_str::<Vec<Friend>>(&data) {
                    Ok(list) => list.into_iter().map(|f| (f.name.clone(), f)).collect(),
                    Err(e) => {
                        warn!(
                            "Failed to parse {}: {} — starting with empty friends list",
                            path.display(),
                            e
                        );
                        HashMap::new()
                    }
                },
                Err(e) => {
                    warn!(
                        "Failed to read {}: {} — starting with empty friends list",
                        path.display(),
                        e
                    );
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };
        Self {
            path: path.to_path_buf(),
            friends,
        }
    }

    /// Save to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let list: Vec<&Friend> = self.friends.values().collect();
        let data = serde_json::to_string_pretty(&list)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }

    /// Look up a friend by exact name, then fall back to alias match.
    /// This lets peers connect with a different node name and still be
    /// recognized if their name matches a friend's alias (or vice versa).
    pub fn get(&self, name: &str) -> Option<&Friend> {
        self.friends.get(name).or_else(|| {
            self.friends.values().find(|f| {
                f.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(name))
            })
        })
    }

    pub fn get_trust_level(&self, name: &str) -> TrustLevel {
        self.get(name)
            .map(|f| f.trust_level)
            .unwrap_or(TrustLevel::UNKNOWN)
    }

    pub fn list(&self) -> Vec<&Friend> {
        let mut friends: Vec<&Friend> = self.friends.values().collect();
        friends.sort_by(|a, b| a.name.cmp(&b.name));
        friends
    }

    /// Add or update a friend. Returns `Some(warning)` if the name collides
    /// with an existing friend's alias.
    pub fn add(&mut self, friend: Friend) -> Option<String> {
        // Check if this name matches another friend's alias
        let warning = self.friends.values().find(|f| {
            f.name != friend.name
                && f.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(&friend.name))
        }).map(|f| {
            format!(
                "Name '{}' matches alias of existing friend '{}'  — these may be the same agent",
                friend.name, f.name
            )
        });
        self.friends.insert(friend.name.clone(), friend);
        warning
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.friends.remove(name).is_some()
    }

    /// Apply a partial update to an existing friend. Returns false if not found.
    pub fn update(&mut self, name: &str, patch: &FriendPatch) -> bool {
        let Some(friend) = self.friends.get_mut(name) else {
            return false;
        };
        if let Some(trust) = patch.trust_level {
            friend.trust_level = TrustLevel(trust.min(4));
        }
        if let Some(ref alias) = patch.alias {
            friend.alias = Some(alias.clone());
        }
        if let Some(ref notes) = patch.notes {
            friend.notes = Some(notes.clone());
        }
        if let Some(muted) = patch.muted {
            friend.muted = muted;
        }
        true
    }

    /// Resolve a name to the canonical friend key (checks aliases).
    fn resolve_key(&self, name: &str) -> Option<String> {
        if self.friends.contains_key(name) {
            return Some(name.to_string());
        }
        self.friends
            .values()
            .find(|f| {
                f.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(name))
            })
            .map(|f| f.name.clone())
    }

    /// Update a friend's last known address. Returns false if not found.
    pub fn update_address(&mut self, name: &str, address: &str) -> bool {
        let key = self.resolve_key(name);
        if let Some(friend) = key.and_then(|k| self.friends.get_mut(&k)) {
            friend.last_address = Some(address.to_string());
            true
        } else {
            false
        }
    }

    /// Get friends that have a stored address (for auto-connect).
    pub fn friends_with_addresses(&self) -> Vec<(&Friend, &str)> {
        self.friends
            .values()
            .filter_map(|f| f.last_address.as_deref().map(|addr| (f, addr)))
            .collect()
    }

    /// Get the highest trust level among all friends sharing a given owner DID.
    pub fn owner_trust_level(&self, owner_did: &str) -> TrustLevel {
        self.friends
            .values()
            .filter(|f| f.owner_did.as_deref() == Some(owner_did))
            .map(|f| f.trust_level)
            .max()
            .unwrap_or(TrustLevel::UNKNOWN)
    }

    /// Find a friend by DID. Returns the friend's name if found.
    pub fn find_by_did(&self, did: &str) -> Option<&Friend> {
        self.friends
            .values()
            .find(|f| f.did.as_deref() == Some(did))
    }

    /// Merge duplicate friends that share the same DID as the connecting peer.
    ///
    /// When a peer connects as "alice-desktop" with DID X, and there's already
    /// a friend "alice" with DID X, this merges them: keeps the higher trust
    /// level, sets the old name as alias, and removes the stale entry.
    ///
    /// Returns a description of what was merged, or None if no duplicates.
    pub fn merge_by_did(&mut self, current_name: &str, did: &str) -> Option<String> {
        // Find all friends with this DID but a different name
        let stale_names: Vec<String> = self
            .friends
            .values()
            .filter(|f| f.did.as_deref() == Some(did) && f.name != current_name)
            .map(|f| f.name.clone())
            .collect();

        if stale_names.is_empty() {
            return None;
        }

        // Collect the highest trust level and any useful metadata from stale entries
        let mut max_trust = self
            .friends
            .get(current_name)
            .map(|f| f.trust_level.0)
            .unwrap_or(0);
        let mut aliases: Vec<String> = Vec::new();
        let mut best_notes: Option<String> = None;

        for stale in &stale_names {
            if let Some(old) = self.friends.get(stale) {
                if old.trust_level.0 > max_trust {
                    max_trust = old.trust_level.0;
                }
                aliases.push(stale.clone());
                if best_notes.is_none() {
                    best_notes = old.notes.clone();
                }
            }
        }

        // Update the current entry with merged data
        if let Some(current) = self.friends.get_mut(current_name) {
            current.trust_level = TrustLevel(max_trust);
            current.did = Some(did.to_string());
            // Set alias to the old name(s) if no alias exists yet
            if current.alias.is_none() && !aliases.is_empty() {
                current.alias = Some(aliases.join(", "));
            }
            if current.notes.is_none() {
                current.notes = best_notes;
            }
        }

        // Remove stale entries
        for stale in &stale_names {
            self.friends.remove(stale);
        }

        let msg = format!(
            "Merged duplicate friend(s) {} into {} (same DID: {}...)",
            stale_names.join(", "),
            current_name,
            &did[..did.len().min(30)]
        );
        Some(msg)
    }
}

// ---------------------------------------------------------------------------
// Friend Request types
// ---------------------------------------------------------------------------

/// Status of a friend request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FriendRequestStatus {
    Pending,
    Accepted,
    Rejected,
}

/// Direction of a friend request relative to us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FriendRequestDirection {
    Inbound,
    Outbound,
}

/// A friend request record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FriendRequest {
    pub id: Uuid,
    pub peer_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_did: Option<String>,
    /// Trust level being offered (inbound: what they'd assign us; outbound: what we'd assign them).
    pub offered_trust: u8,
    pub direction: FriendRequestDirection,
    pub status: FriendRequestStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_did: Option<String>,
}

// ---------------------------------------------------------------------------
// FriendRequestStore — JSON-backed friend request persistence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FriendRequestStore {
    path: PathBuf,
    requests: Vec<FriendRequest>,
}

impl FriendRequestStore {
    /// Default path: `~/.agora/friend_requests.json`
    pub fn default_path() -> PathBuf {
        crate::config::agora_home()
            .join("friend_requests.json")
    }

    /// Load from disk, or start empty if file doesn't exist.
    pub fn load(path: &Path) -> Self {
        let requests = if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => match serde_json::from_str::<Vec<FriendRequest>>(&data) {
                    Ok(list) => list,
                    Err(e) => {
                        warn!(
                            "Failed to parse {}: {} — starting with empty friend requests",
                            path.display(),
                            e
                        );
                        Vec::new()
                    }
                },
                Err(e) => {
                    warn!(
                        "Failed to read {}: {} — starting with empty friend requests",
                        path.display(),
                        e
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        Self {
            path: path.to_path_buf(),
            requests,
        }
    }

    /// Save to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(&self.requests)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }

    /// Add a friend request.
    pub fn add(&mut self, request: FriendRequest) {
        self.requests.push(request);
    }

    /// Get a request by ID.
    pub fn get(&self, id: &Uuid) -> Option<&FriendRequest> {
        self.requests.iter().find(|r| r.id == *id)
    }

    /// Get a mutable request by ID.
    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut FriendRequest> {
        self.requests.iter_mut().find(|r| r.id == *id)
    }

    /// Get all requests.
    pub fn list(&self) -> &[FriendRequest] {
        &self.requests
    }

    /// Get pending inbound requests.
    pub fn pending_inbound(&self) -> Vec<&FriendRequest> {
        self.requests
            .iter()
            .filter(|r| {
                r.direction == FriendRequestDirection::Inbound
                    && r.status == FriendRequestStatus::Pending
            })
            .collect()
    }

    /// Get pending outbound requests.
    pub fn pending_outbound(&self) -> Vec<&FriendRequest> {
        self.requests
            .iter()
            .filter(|r| {
                r.direction == FriendRequestDirection::Outbound
                    && r.status == FriendRequestStatus::Pending
            })
            .collect()
    }

    /// Get pending outbound request to a specific peer name.
    pub fn pending_outbound_to(&self, name: &str) -> Option<&FriendRequest> {
        self.requests.iter().find(|r| {
            r.direction == FriendRequestDirection::Outbound
                && r.status == FriendRequestStatus::Pending
                && r.peer_name == name
        })
    }

    /// Get pending inbound request from a specific peer name.
    pub fn pending_inbound_from(&self, name: &str) -> Option<&FriendRequest> {
        self.requests.iter().find(|r| {
            r.direction == FriendRequestDirection::Inbound
                && r.status == FriendRequestStatus::Pending
                && r.peer_name == name
        })
    }
}

/// Tracks a pending debounced wake-up invocation.
struct WakeDebounce {
    handle: JoinHandle<()>,
    count: usize,
    /// Accumulated messages that triggered the wake (cloned so they survive inbox drain).
    messages: Vec<Message>,
}

#[derive(Debug, Clone)]
struct LastWakeEvent {
    fired_at: chrono::DateTime<chrono::Utc>,
    from: String,
    message_count: usize,
}

/// Shared daemon state — thread-safe, accessible from both the network
/// listener and the local HTTP API.
#[derive(Clone)]
pub struct DaemonState {
    inner: Arc<Inner>,
}

struct Inner {
    /// Fan-out inbox: each registered consumer gets its own message buffer.
    consumers: Mutex<HashMap<ConsumerId, ConsumerSlot>>,
    /// Monotonically increasing counter for consumer IDs.
    next_consumer_id: AtomicU64,
    /// The default consumer used by the legacy GET /messages endpoint.
    default_consumer: Mutex<Option<ConsumerId>>,
    /// Broadcast channel for outbound messages — each peer gets a copy.
    outbox_tx: broadcast::Sender<OutboundMessage>,
    /// Connected peers.
    peers: Mutex<Vec<PeerInfo>>,
    /// This node's name.
    node_name: String,
    /// Optional shell command to run when a message arrives (wake-up hook).
    wake_command: Mutex<Option<String>>,
    /// Path to persist the wake command (`~/.agora/wake.json`).
    wake_path: PathBuf,
    /// Friend graph with trust levels.
    friends: Mutex<FriendsStore>,
    /// Wake debounce state: (pending task handle, accumulated message count, last sender, last trust).
    /// When a message arrives, we start/reset a 3s timer. Only fires wake once.
    wake_debounce: Mutex<Option<WakeDebounce>>,
    /// Most recent wake hook execution recorded for dashboard/status visibility.
    last_wake_event: Mutex<Option<LastWakeEvent>>,
    /// When the daemon started, used by the /health endpoint.
    start_time: chrono::DateTime<chrono::Utc>,
    /// The local HTTP API port (passed to wake hooks as AGORA_API_PORT).
    api_port: u16,
    /// Conversation history: all messages (inbound and outbound) for threading.
    conversation_history: Mutex<Vec<StoredMessage>>,
    /// Path for persisting conversation history to disk.
    conversations_path: PathBuf,
    /// Thread/sub-group manager.
    threads: Mutex<ThreadManager>,
    /// Addresses that should NOT auto-reconnect after explicit disconnect.
    disconnected_addrs: Mutex<HashSet<String>>,
    /// Minimum trust level for accepting peer connections (0 = accept anyone).
    min_trust: AtomicU64,
    /// Auto-accept policy for friend requests and project invitations.
    auto_accept_policy: std::sync::Mutex<crate::config::AutoAcceptPolicy>,
    /// Agent's cryptographic identity (Ed25519 keypair + DID).
    identity: AgentIdentity,
    /// Owner attestation (if this agent has a registered owner).
    owner_attestation: Option<OwnerAttestation>,
    /// Friend request store.
    friend_requests: Mutex<FriendRequestStore>,
    /// Project store.
    projects: Mutex<crate::project::ProjectStore>,
    /// Project invitation store.
    project_invitations: Mutex<crate::project::ProjectInvitationStore>,
    /// Offline message queue for store-and-forward delivery.
    outbox_store: Mutex<crate::outbox::OutboxStore>,
    /// Agent marketplace for capability-based discovery.
    marketplace: Mutex<crate::marketplace::MarketplaceStore>,
    /// Gossip-based network discovery store.
    discovery: Mutex<crate::discovery::DiscoveryStore>,
    /// Reputation tracking for agent contributions.
    reputation: Mutex<crate::reputation::ReputationStore>,
    /// Coordinator suggestions per project.
    coordinator_suggestions:
        Mutex<std::collections::HashMap<Uuid, Vec<crate::coordinator::CoordinatorSuggestion>>>,
    /// Coordinator digests per project.
    coordinator_digests:
        Mutex<std::collections::HashMap<Uuid, Vec<crate::coordinator::ProjectDigest>>>,
    /// API authentication token for dashboard access.
    api_token: String,
}

/// Result of registering a peer via add_peer.
#[derive(Debug, PartialEq)]
pub enum RegisterResult {
    /// Brand new peer — no existing entry with this name.
    Registered,
    /// Replaced an existing entry with a different session_id (genuine reconnect).
    Replaced,
    /// Duplicate — an existing entry with the same session_id is still connected.
    Duplicate,
}

/// Result of checking a peer's DID against their friend record.
#[derive(Debug)]
pub enum DidPinResult {
    /// TOFU: DID stored as pin for the first time.
    FirstSeen,
    /// DID matches the stored pin.
    Match,
    /// DID changed from the stored pin — REJECT.
    Mismatch { expected: String },
    /// Not in friend list — no pinning applied.
    NotAFriend,
}

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub name: String,
    pub address: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Peer's DID (if provided in Hello).
    pub did: Option<String>,
    /// Peer's session ID (if provided in Hello).
    pub session_id: Option<Uuid>,
    /// Whether the peer's cryptographic signature was verified.
    pub verified: bool,
    /// Last heartbeat/message timestamp for presence tracking.
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    /// Peer's owner DID (if provided and verified in Hello).
    pub owner_did: Option<String>,
    /// Whether the peer's owner attestation was verified.
    pub owner_verified: bool,
    /// Signal to disconnect this peer.
    pub disconnect: Arc<Notify>,
}

#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub body: String,
    /// If None, broadcast to all peers. If Some, send to specific peer.
    pub to: Option<String>,
    /// Unique message identifier.
    pub id: Uuid,
    /// If this message is a reply, the id of the parent message.
    pub reply_to: Option<Uuid>,
    /// Conversation thread identifier.
    pub conversation_id: Option<Uuid>,
    /// Message type override (default: Message). Used for thread.* messages.
    pub msg_type: Option<crate::protocol::message::MessageType>,
    /// Project ID to tag this message with for project conversation tracking.
    pub project_id: Option<Uuid>,
    /// Override sender name (for multi-agent daemons where multiple agents share one daemon).
    pub from_override: Option<String>,
}

impl DaemonState {
    pub fn new(node_name: &str, friends_path: &Path, api_port: u16) -> Self {
        let friends = FriendsStore::load(friends_path);
        let count = friends.list().len();
        if count > 0 {
            info!("Loaded {} friend(s) from {}", count, friends_path.display());
        }

        // Load persisted wake command
        let wake_path = wake_command_path();
        let wake_command = load_wake_command(&wake_path);
        if let Some(ref cmd) = wake_command {
            info!("Loaded wake command from {}: {}", wake_path.display(), cmd);
        }

        // Load or create cryptographic identity
        let identity_path = AgentIdentity::default_path();
        let identity = AgentIdentity::load_or_create(&identity_path)
            .expect("Failed to load or create agent identity");
        info!("Agent DID: {}", identity.did());
        info!("Session ID: {}", identity.session_id());

        // Load owner attestation if it exists and is valid for this agent
        let owner_attestation = {
            let att_path = OwnerAttestation::default_path();
            if att_path.exists() {
                match OwnerAttestation::load(&att_path) {
                    Ok(att) => {
                        if att.verify_for_agent(identity.did()) {
                            info!("Owner attestation verified: owner={}", att.owner_did);
                            Some(att)
                        } else {
                            warn!(
                                "Owner attestation invalid for this agent (agent_did mismatch or bad sig) — ignoring"
                            );
                            None
                        }
                    }
                    Err(e) => {
                        warn!("Failed to load owner attestation: {} — ignoring", e);
                        None
                    }
                }
            } else {
                None
            }
        };

        // Load friend requests
        let friend_requests_path = FriendRequestStore::default_path();
        let friend_requests = FriendRequestStore::load(&friend_requests_path);
        let pending_count = friend_requests.pending_inbound().len();
        if pending_count > 0 {
            info!("Loaded {} pending friend request(s)", pending_count);
        }

        // Load projects
        let projects_path = crate::project::ProjectStore::default_path();
        let projects = crate::project::ProjectStore::load(&projects_path);
        let project_count = projects.list().len();
        if project_count > 0 {
            info!("Loaded {} project(s)", project_count);
        }

        // Load project invitations
        let project_invitations_path = crate::project::ProjectInvitationStore::default_path();
        let project_invitations =
            crate::project::ProjectInvitationStore::load(&project_invitations_path);

        let (outbox_tx, _) = broadcast::channel(256);
        let state = Self {
            inner: Arc::new(Inner {
                consumers: Mutex::new(HashMap::new()),
                next_consumer_id: AtomicU64::new(1),
                default_consumer: Mutex::new(None),
                outbox_tx,
                peers: Mutex::new(Vec::new()),
                node_name: node_name.to_string(),
                wake_command: Mutex::new(wake_command),
                wake_path,
                friends: Mutex::new(friends),
                wake_debounce: Mutex::new(None),
                last_wake_event: Mutex::new(None),
                start_time: chrono::Utc::now(),
                api_port,
                conversations_path: Self::conversation_history_path(),
                conversation_history: Mutex::new(Self::load_conversation_history_sync()),
                threads: Mutex::new(ThreadManager::new()),
                disconnected_addrs: Mutex::new(HashSet::new()),
                min_trust: AtomicU64::new(0),
                auto_accept_policy: std::sync::Mutex::new(
                    crate::config::AutoAcceptPolicy::SameOwner,
                ),
                identity,
                owner_attestation,
                friend_requests: Mutex::new(friend_requests),
                projects: Mutex::new(projects),
                project_invitations: Mutex::new(project_invitations),
                outbox_store: Mutex::new(crate::outbox::OutboxStore::new(
                    &crate::outbox::OutboxStore::default_path(),
                )),
                discovery: Mutex::new(crate::discovery::DiscoveryStore::load(
                    &crate::discovery::DiscoveryStore::default_path(),
                )),
                marketplace: Mutex::new(crate::marketplace::MarketplaceStore::load(
                    &crate::marketplace::MarketplaceStore::default_path(),
                )),
                reputation: Mutex::new(crate::reputation::ReputationStore::load(
                    &crate::reputation::ReputationStore::default_path(),
                )),
                coordinator_suggestions: Mutex::new(std::collections::HashMap::new()),
                coordinator_digests: Mutex::new(std::collections::HashMap::new()),
                api_token: crate::auth::load_or_create_token(&crate::auth::default_token_path()),
            }),
        };

        // Spawn stale consumer reaper — removes consumers idle > 5 minutes
        let reaper_state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let now = chrono::Utc::now();
                let mut consumers = reaper_state.inner.consumers.lock().await;
                let stale: Vec<ConsumerId> = consumers
                    .iter()
                    .filter(|(_, slot)| {
                        now.signed_duration_since(slot.last_active).num_seconds() > 300
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in stale {
                    if let Some(slot) = consumers.remove(&id) {
                        warn!(
                            "Reaped stale consumer {} ({}) — idle for >5 minutes",
                            id, slot.label
                        );
                        // If this was the default consumer, clear it so it gets re-created
                        let mut default = reaper_state.inner.default_consumer.lock().await;
                        if *default == Some(id) {
                            *default = None;
                        }
                    }
                }
            }
        });

        state
    }

    pub fn node_name(&self) -> &str {
        &self.inner.node_name
    }

    /// Get the API authentication token.
    pub fn api_token(&self) -> &str {
        &self.inner.api_token
    }

    /// The agent's cryptographic identity.
    pub fn identity(&self) -> &AgentIdentity {
        &self.inner.identity
    }

    /// The agent's DID string.
    pub fn did(&self) -> &str {
        self.inner.identity.did()
    }

    /// The per-process session ID.
    pub fn session_id(&self) -> Uuid {
        self.inner.identity.session_id()
    }

    /// The owner's DID, if an owner attestation is loaded.
    pub fn owner_did(&self) -> Option<&str> {
        self.inner
            .owner_attestation
            .as_ref()
            .map(|a| a.owner_did.as_str())
    }

    /// The owner attestation, if loaded and valid.
    pub fn owner_attestation(&self) -> Option<&OwnerAttestation> {
        self.inner.owner_attestation.as_ref()
    }

    /// Get the highest trust level among all friends that share the given owner DID.
    pub async fn owner_trust_level(&self, owner_did: &str) -> TrustLevel {
        let store = self.inner.friends.lock().await;
        store.owner_trust_level(owner_did)
    }

    /// Check and pin an owner DID for a friend (TOFU, same as DID pinning).
    pub async fn check_and_pin_owner_did(&self, name: &str, owner_did: &str) -> DidPinResult {
        let mut store = self.inner.friends.lock().await;
        let key = store.resolve_key(name);
        match key.and_then(|k| store.friends.get_mut(&k)) {
            None => DidPinResult::NotAFriend,
            Some(friend) => match &friend.owner_did {
                None => {
                    friend.owner_did = Some(owner_did.to_string());
                    let _ = store.save();
                    info!("Pinned owner DID for {}: {}", name, owner_did);
                    DidPinResult::FirstSeen
                }
                Some(existing) if existing == owner_did => DidPinResult::Match,
                Some(existing) => DidPinResult::Mismatch {
                    expected: existing.clone(),
                },
            },
        }
    }

    /// Update a friend's owner DID (after verifying their owner attestation).
    pub async fn update_friend_owner_did(&self, name: &str, owner_did: &str) {
        let mut store = self.inner.friends.lock().await;
        let key = store.resolve_key(name);
        if let Some(friend) = key.and_then(|k| store.friends.get_mut(&k)) {
            friend.owner_did = Some(owner_did.to_string());
            let _ = store.save();
        }
    }

    pub fn set_auto_accept_policy(&self, policy: crate::config::AutoAcceptPolicy) {
        *self.inner.auto_accept_policy.lock().unwrap() = policy;
    }

    pub fn auto_accept_policy(&self) -> crate::config::AutoAcceptPolicy {
        *self.inner.auto_accept_policy.lock().unwrap()
    }

    /// Check if an incoming request should be auto-accepted based on the policy.
    pub async fn should_auto_accept(&self, sender_name: &str) -> bool {
        let policy = self.auto_accept_policy();
        match policy {
            crate::config::AutoAcceptPolicy::Never => false,
            crate::config::AutoAcceptPolicy::SameOwner => {
                // Auto-accept if sender has the same owner DID as us
                let our_owner = self.owner_did().map(|s| s.to_string());
                if our_owner.is_none() {
                    return false;
                }
                let store = self.inner.friends.lock().await;
                store
                    .get(sender_name)
                    .and_then(|f| f.owner_did.as_ref())
                    .map(|did| Some(did.clone()) == our_owner)
                    .unwrap_or(false)
            }
            crate::config::AutoAcceptPolicy::Trusted => {
                self.get_trust_level(sender_name).await.0 >= 3
            }
        }
    }

    pub fn set_min_trust(&self, level: u8) {
        self.inner
            .min_trust
            .store(level as u64, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn min_trust(&self) -> u8 {
        self.inner
            .min_trust
            .load(std::sync::atomic::Ordering::Relaxed) as u8
    }

    pub fn start_time(&self) -> chrono::DateTime<chrono::Utc> {
        self.inner.start_time
    }

    /// Set a shell command to run when messages arrive (wake-up hook).
    /// The command receives AGORA_FROM and AGORA_PREVIEW environment variables.
    /// Persists to `~/.agora/wake.json` so it survives daemon restarts.
    ///
    /// Returns `Err` if the command contains dangerous shell metacharacters.
    pub async fn set_wake_command(&self, cmd: Option<String>) -> Result<(), String> {
        if let Some(ref c) = cmd {
            validate_wake_command(c)?;
        }
        *self.inner.wake_command.lock().await = cmd.clone();
        save_wake_command(&self.inner.wake_path, cmd.as_deref());
        Ok(())
    }

    /// Get the current wake-up command.
    pub async fn get_wake_command(&self) -> Option<String> {
        self.inner.wake_command.lock().await.clone()
    }

    /// Return a snapshot of whether wake is configured, currently armed, and
    /// when it last fired. This is used by the dashboard to avoid treating
    /// daemon reachability as equivalent to wake readiness.
    pub async fn wake_status(&self) -> WakeStatusSnapshot {
        let enabled = self.inner.wake_command.lock().await.is_some();
        let (active_listener_count, mut active_listener_labels) = {
            let now = chrono::Utc::now();
            let consumers = self.inner.consumers.lock().await;
            let mut labels: Vec<String> = consumers
                .values()
                .filter(|slot| {
                    slot.suppresses_wake
                        && now.signed_duration_since(slot.last_active).num_seconds() < 60
                })
                .map(|slot| slot.label.clone())
                .collect();
            labels.sort();
            (labels.len(), labels)
        };
        active_listener_labels.dedup();

        let last_wake = self.inner.last_wake_event.lock().await.clone();
        WakeStatusSnapshot {
            enabled,
            armed: enabled && active_listener_count == 0,
            active_listener_count,
            active_listener_labels,
            last_fired_at: last_wake.as_ref().map(|event| event.fired_at.to_rfc3339()),
            last_fired_from: last_wake.as_ref().map(|event| event.from.clone()),
            last_message_count: last_wake.as_ref().map(|event| event.message_count),
        }
    }

    // --- Consumer management ---

    /// Register a new inbox consumer. Returns its unique ID.
    /// MCP consumers do NOT suppress wake — the wake hook needs to fire
    /// to alert idle agents about incoming messages.
    /// Also auto-clocks the agent into any active projects they're a member of.
    pub async fn register_consumer(&self, label: &str) -> ConsumerId {
        let id = self.register_consumer_inner(label, false).await;
        // Auto-clock-in: if this agent is a member of any project, clock them in
        self.auto_clock_in_agent(label).await;
        // Auto-advertise in marketplace: register this agent as available
        self.auto_advertise_agent(label).await;
        id
    }

    /// Register a new wake-suppressing listener consumer.
    /// This is intended for persistent local child-agent/listener processes.
    pub async fn register_listener_consumer(&self, label: &str) -> ConsumerId {
        let id = self.register_consumer_inner(label, true).await;
        // Auto-clock-in: if this agent is a member of any project, clock them in
        self.auto_clock_in_agent(label).await;
        id
    }

    async fn register_consumer_inner(&self, label: &str, suppresses_wake: bool) -> ConsumerId {
        let mut consumers = self.inner.consumers.lock().await;

        // Reuse existing consumer with the same label (prevents consumer leak on reconnect)
        if let Some((&existing_id, slot)) = consumers.iter_mut().find(|(_, s)| s.label == label) {
            slot.last_active = chrono::Utc::now();
            slot.suppresses_wake = suppresses_wake;
            info!("Consumer reused: {} ({})", existing_id, label);
            return existing_id;
        }

        let id = ConsumerId(self.inner.next_consumer_id.fetch_add(1, Ordering::Relaxed));
        let slot = ConsumerSlot {
            buffer: VecDeque::new(),
            label: label.to_string(),
            registered_at: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
            notify: Arc::new(Notify::new()),
            suppresses_wake,
            rate_limiter: ConsumerRateLimiter::new(100, Duration::from_secs(1)),
        };
        consumers.insert(id, slot);
        info!("Consumer registered: {} ({})", id, label);
        id
    }

    /// Check per-consumer rate limit. Returns `true` if the request is allowed.
    pub async fn check_consumer_rate_limit(&self, id: ConsumerId) -> bool {
        let mut consumers = self.inner.consumers.lock().await;
        if let Some(slot) = consumers.get_mut(&id) {
            slot.rate_limiter.try_acquire()
        } else {
            false // unknown consumer
        }
    }

    /// Unregister a consumer, dropping its buffer.
    pub async fn unregister_consumer(&self, id: ConsumerId) -> bool {
        let removed = self.inner.consumers.lock().await.remove(&id);
        if let Some(slot) = removed {
            info!("Consumer unregistered: {} ({})", id, slot.label);
            // If this was the default consumer, clear it
            let mut default = self.inner.default_consumer.lock().await;
            if *default == Some(id) {
                *default = None;
            }
            true
        } else {
            false
        }
    }

    /// Number of registered consumers.
    pub async fn consumer_count(&self) -> usize {
        self.inner.consumers.lock().await.len()
    }

    /// List all registered consumers with info.
    pub async fn list_consumers(&self) -> Vec<ConsumerInfo> {
        self.inner
            .consumers
            .lock()
            .await
            .iter()
            .map(|(id, slot)| ConsumerInfo {
                id: *id,
                label: slot.label.clone(),
                registered_at: slot.registered_at.to_rfc3339(),
                last_active: slot.last_active.to_rfc3339(),
                buffered_messages: slot.buffer.len(),
                suppresses_wake: slot.suppresses_wake,
            })
            .collect()
    }

    /// Drain all messages from a specific consumer's buffer.
    pub async fn drain_consumer(&self, id: ConsumerId) -> Option<Vec<Message>> {
        let mut consumers = self.inner.consumers.lock().await;
        if let Some(slot) = consumers.get_mut(&id) {
            slot.last_active = chrono::Utc::now();
            Some(slot.buffer.drain(..).collect())
        } else {
            None
        }
    }

    /// Peek at messages in a consumer's buffer without removing them.
    pub async fn peek_consumer(&self, id: ConsumerId) -> Option<Vec<Message>> {
        let mut consumers = self.inner.consumers.lock().await;
        if let Some(slot) = consumers.get_mut(&id) {
            slot.last_active = chrono::Utc::now();
            Some(slot.buffer.iter().cloned().collect())
        } else {
            None
        }
    }

    /// Acknowledge specific messages by ID, removing only those from the buffer.
    /// Returns the number of messages actually removed.
    pub async fn ack_consumer(&self, id: ConsumerId, message_ids: &[Uuid]) -> Option<usize> {
        let mut consumers = self.inner.consumers.lock().await;
        if let Some(slot) = consumers.get_mut(&id) {
            slot.last_active = chrono::Utc::now();
            let before = slot.buffer.len();
            slot.buffer.retain(|m| !message_ids.contains(&m.id));
            Some(before - slot.buffer.len())
        } else {
            None
        }
    }

    /// Refresh a consumer's liveness timestamp without draining any messages.
    pub async fn touch_consumer(&self, id: ConsumerId) -> bool {
        let mut consumers = self.inner.consumers.lock().await;
        if let Some(slot) = consumers.get_mut(&id) {
            slot.last_active = chrono::Utc::now();
            true
        } else {
            false
        }
    }

    /// Wait for messages to arrive in a consumer's buffer, then drain.
    pub async fn wait_for_consumer(
        &self,
        id: ConsumerId,
        timeout: Duration,
    ) -> Option<Vec<Message>> {
        // Get the consumer's notify handle
        let notify = {
            let mut consumers = self.inner.consumers.lock().await;
            let slot = consumers.get_mut(&id)?;
            slot.last_active = chrono::Utc::now();
            if !slot.buffer.is_empty() {
                return Some(slot.buffer.drain(..).collect());
            }
            slot.notify.clone()
        };

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return self.drain_consumer(id).await;
            }
            match tokio::time::timeout(remaining, notify.notified()).await {
                Ok(()) => {
                    // Check if buffer has messages now
                    let mut consumers = self.inner.consumers.lock().await;
                    if let Some(slot) = consumers.get_mut(&id) {
                        slot.last_active = chrono::Utc::now();
                        if !slot.buffer.is_empty() {
                            return Some(slot.buffer.drain(..).collect());
                        }
                    } else {
                        return None; // Consumer was removed
                    }
                }
                Err(_) => return self.drain_consumer(id).await,
            }
        }
    }

    // --- Backward compatibility shims ---

    /// Get or lazily create the default consumer (used by GET /messages).
    pub async fn default_consumer_id(&self) -> ConsumerId {
        let default = self.inner.default_consumer.lock().await;
        if let Some(id) = *default {
            // Verify the consumer still exists (might have been reaped)
            if self.inner.consumers.lock().await.contains_key(&id) {
                return id;
            }
        }
        // Create a new default consumer (does NOT suppress wake)
        drop(default); // release lock before calling register_consumer
        let id = self.register_consumer_inner("http-default", false).await;
        *self.inner.default_consumer.lock().await = Some(id);
        id
    }

    /// Drain all messages from the default consumer (backward compat for GET /messages).
    pub async fn drain_inbox(&self) -> Vec<Message> {
        let id = self.default_consumer_id().await;
        self.drain_consumer(id).await.unwrap_or_default()
    }

    /// Peek at messages without removing them from the consumer buffer.
    pub async fn peek_inbox(&self) -> Vec<Message> {
        let id = self.default_consumer_id().await;
        self.peek_consumer(id).await.unwrap_or_default()
    }

    /// Acknowledge (remove) specific messages by ID from the default consumer.
    pub async fn ack_inbox(&self, message_ids: &[Uuid]) -> usize {
        let id = self.default_consumer_id().await;
        self.ack_consumer(id, message_ids).await.unwrap_or(0)
    }

    /// Wait for messages on the default consumer (backward compat for GET /messages).
    pub async fn wait_for_inbox(&self, timeout: Duration) -> Vec<Message> {
        let id = self.default_consumer_id().await;
        self.wait_for_consumer(id, timeout)
            .await
            .unwrap_or_default()
    }

    // --- Fan-out inbox push ---

    /// Push an incoming message from a remote peer into all consumer buffers.
    /// Also fires the wake-up hook if configured AND sender has trust >= 3
    /// AND no consumer has been actively polling in the last 60 seconds.
    ///
    /// The logic: if a consumer (e.g. MCP monitor) is actively polling, there's
    /// a live session that will receive the message via MCP notification. Spawning
    /// a second `claude -p` would just create a conflicting duplicate session.
    /// Wake only fires when nobody is listening — that's when it's needed.
    pub async fn push_inbox(&self, msg: Message) {
        let from = msg.from.clone();
        let body_preview = msg.body.chars().take(100).collect::<String>();

        // Look up sender's trust level and muted status
        let (trust, sender_muted) = {
            let store = self.inner.friends.lock().await;
            let trust = store.get_trust_level(&from);
            let muted = store.get(&from).is_some_and(|f| f.muted);
            (trust, muted)
        };

        // Clone the message for the wake hook
        let msg_for_wake = msg.clone();

        // Fan-out: push a clone into every registered consumer's buffer.
        // Check if any wake-suppressing consumer has been active recently.
        // Only explicitly registered consumers (e.g., MCP monitor) count.
        // The lazy "http-default" consumer from curl/GET does NOT suppress wake.
        let has_active_consumer = {
            let now = chrono::Utc::now();
            let mut consumers = self.inner.consumers.lock().await;
            let active = consumers.values().any(|slot| {
                slot.suppresses_wake
                    && now.signed_duration_since(slot.last_active).num_seconds() < 60
            });
            for slot in consumers.values_mut() {
                // Cap buffer at 1000 messages to prevent unbounded growth
                while slot.buffer.len() >= 1000 {
                    slot.buffer.pop_front();
                }
                slot.buffer.push_back(msg.clone());
                slot.notify.notify_waiters();
            }
            active
        };

        // Store in conversation history (auto-assign conversation_id for 1:1 DMs)
        {
            let conv_id = msg
                .conversation_id
                .unwrap_or_else(|| self.peer_conversation_id(&from));
            // Extract project_id from project-related message types
            let project_id = Self::extract_project_id(&msg);
            let stored = StoredMessage {
                id: msg.id.to_string(),
                from: msg.from.clone(),
                body: msg.body.clone(),
                timestamp: msg.timestamp.to_rfc3339(),
                reply_to: msg.reply_to.map(|u| u.to_string()),
                conversation_id: Some(conv_id.to_string()),
                direction: "inbound".to_string(),
                project_id,
                signature: msg.signature.clone(),
                public_key: msg.public_key.clone(),
            };
            let mut history = self.inner.conversation_history.lock().await;
            history.push(stored);
            // Per-conversation + global cap trimming
            trim_conversation_history(&mut history);
            self.save_conversation_history(&history);
        }

        // Fire wake-up command only if:
        // 1. Sender is not muted
        // 2. Sender has sufficient trust
        // 3. No consumer has been active in the last 60s (no live session polling)
        if !has_active_consumer && !sender_muted {
            if let Some(cmd) = self.inner.wake_command.lock().await.clone() {
                if trust.can_wake() {
                    let mut debounce = self.inner.wake_debounce.lock().await;

                    if let Some(existing) = debounce.as_mut() {
                        // Already have a pending wake — cancel it and reset with higher count
                        existing.handle.abort();
                        let new_count = existing.count + 1;
                        let mut accumulated_msgs = std::mem::take(&mut existing.messages);
                        accumulated_msgs.push(msg_for_wake);
                        let inner = self.inner.clone();
                        let msgs_clone = accumulated_msgs.clone();
                        let handle = tokio::spawn(fire_wake_after_delay(
                            inner,
                            cmd,
                            from.clone(),
                            body_preview,
                            trust,
                            new_count,
                            msgs_clone,
                        ));
                        *existing = WakeDebounce {
                            handle,
                            count: new_count,
                            messages: accumulated_msgs,
                        };
                    } else {
                        // First message — start debounce timer
                        let wake_msgs = vec![msg_for_wake];
                        let inner = self.inner.clone();
                        let msgs_clone = wake_msgs.clone();
                        let handle = tokio::spawn(fire_wake_after_delay(
                            inner,
                            cmd,
                            from.clone(),
                            body_preview,
                            trust,
                            1,
                            msgs_clone,
                        ));
                        *debounce = Some(WakeDebounce {
                            handle,
                            count: 1,
                            messages: wake_msgs,
                        });
                    }
                } else {
                    info!(
                        "Wake-up hook suppressed for {} (trust level {} — need >= 3)",
                        from, trust
                    );
                }
            }
        } else if sender_muted {
            info!("Wake-up hook skipped — {} is muted", from);
        } else {
            info!("Wake-up hook skipped — active consumer detected (session is polling)");
        }
    }

    /// Queue a message to send to remote peers. All connected peers receive it
    /// via their broadcast subscription.
    pub async fn push_outbox(&self, msg: OutboundMessage) {
        // If targeting a local consumer (same daemon, multi-agent), deliver directly
        if let Some(ref to) = msg.to {
            if self.deliver_to_local_consumer(to, &msg).await {
                return; // Delivered locally, no need for P2P
            }
        }
        // Ignore send error — just means no peers are subscribed
        let _ = self.inner.outbox_tx.send(msg);
    }

    /// Try to deliver a message to a local consumer by label.
    /// Returns true if a matching consumer was found and the message was delivered.
    pub async fn deliver_to_local_consumer(
        &self,
        target_label: &str,
        msg: &OutboundMessage,
    ) -> bool {
        let sender = msg
            .from_override
            .as_deref()
            .unwrap_or(&self.inner.node_name);
        // Don't deliver to yourself
        if target_label == sender {
            return false;
        }
        let mut consumers = self.inner.consumers.lock().await;
        let mut delivered = false;
        for slot in consumers.values_mut() {
            // Match exact label OR label starts with target (e.g. "claude-2" matches "claude-2-listener")
            if slot.label == target_label || slot.label.starts_with(&format!("{}-", target_label)) {
                // Create a synthetic inbound message for the target consumer
                let inbound = Message {
                    version: "1.0".to_string(),
                    msg_type: msg
                        .msg_type
                        .clone()
                        .unwrap_or(crate::protocol::message::MessageType::Message),
                    from: sender.to_string(),
                    body: msg.body.clone(),
                    timestamp: chrono::Utc::now(),
                    id: msg.id,
                    reply_to: msg.reply_to,
                    conversation_id: msg.conversation_id,
                    did: None,
                    public_key: None,
                    session_id: None,
                    signature: None,
                    owner_did: None,
                    owner_attestation: None,
                };
                while slot.buffer.len() >= 1000 {
                    slot.buffer.pop_front();
                }
                slot.buffer.push_back(inbound);
                slot.notify.notify_waiters();
                delivered = true;
            }
        }
        if delivered {
            tracing::info!("Delivered message locally to consumer '{}'", target_label);
        }
        delivered
    }

    /// Subscribe to outbound messages. Each peer connection calls this to get
    /// its own receiver. Messages are delivered to all subscribers.
    pub fn subscribe_outbox(&self) -> broadcast::Receiver<OutboundMessage> {
        self.inner.outbox_tx.subscribe()
    }

    /// Register a connected peer. Returns `RegisterResult` indicating what happened:
    /// - `Registered`: new peer, no prior entry.
    /// - `Replaced`: existing entry with different session_id was evicted (genuine reconnect).
    /// - `Duplicate`: existing entry with same session_id still active (race condition).
    pub async fn add_peer(&self, info: PeerInfo) -> RegisterResult {
        let mut peers = self.inner.peers.lock().await;
        if let Some(existing) = peers.iter().find(|p| p.name == info.name) {
            if existing.session_id == info.session_id && info.session_id.is_some() {
                // Same session — duplicate connection from simultaneous connect race
                return RegisterResult::Duplicate;
            }
            // Different session — genuine reconnect, evict old and notify disconnect
            let old = peers.iter().find(|p| p.name == info.name).cloned();
            if let Some(old) = old {
                old.disconnect.notify_one();
            }
            peers.retain(|p| p.name != info.name);
            peers.push(info);
            RegisterResult::Replaced
        } else {
            peers.push(info);
            RegisterResult::Registered
        }
    }

    /// Check if a peer with the given name is already connected.
    pub async fn is_peer_connected_by_name(&self, name: &str) -> bool {
        self.inner.peers.lock().await.iter().any(|p| p.name == name)
    }

    /// Check if a peer with the given address is already connected.
    pub async fn is_peer_connected_by_addr(&self, addr: &str) -> bool {
        self.inner
            .peers
            .lock()
            .await
            .iter()
            .any(|p| p.address == addr)
    }

    /// Update last_seen timestamp for a peer (heartbeat/presence tracking).
    pub async fn update_peer_last_seen(&self, name: &str) {
        let mut peers = self.inner.peers.lock().await;
        if let Some(peer) = peers.iter_mut().find(|p| p.name == name) {
            peer.last_seen = Some(chrono::Utc::now());
        }
    }

    /// Remove a disconnected peer.
    pub async fn remove_peer(&self, address: &str) {
        self.inner
            .peers
            .lock()
            .await
            .retain(|p| p.address != address);
    }

    /// Get list of connected peers.
    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        self.inner.peers.lock().await.clone()
    }

    /// Disconnect a peer by name. Signals the connection task to shut down,
    /// removes from peer list, and adds address to disconnected set to prevent
    /// auto-reconnect.
    pub async fn disconnect_peer(&self, name: &str) -> bool {
        let mut peers = self.inner.peers.lock().await;
        if let Some(idx) = peers.iter().position(|p| p.name == name) {
            let peer = peers.remove(idx);
            peer.disconnect.notify_one();
            self.inner
                .disconnected_addrs
                .lock()
                .await
                .insert(peer.address.clone());
            info!("Disconnected peer {} ({})", name, peer.address);
            true
        } else {
            false
        }
    }

    /// Check if an address is in the disconnected set (should not auto-reconnect).
    pub async fn is_disconnected(&self, addr: &str) -> bool {
        self.inner.disconnected_addrs.lock().await.contains(addr)
    }

    /// Remove an address from the disconnected set.
    pub async fn clear_disconnected(&self, addr: &str) {
        self.inner.disconnected_addrs.lock().await.remove(addr);
    }

    /// Graceful shutdown: send Close to all peers, drain outbox, save state,
    /// and log a shutdown summary.
    pub async fn graceful_shutdown(&self) {
        use crate::protocol::message::MessageType;

        let start = std::time::Instant::now();
        info!("Graceful shutdown initiated");

        // --- Phase 1: Notify all peers with a Close message ---
        let peers = self.get_peers().await;
        let peer_count = peers.len();
        let peer_names: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();

        if peer_count > 0 {
            info!(
                "Sending Close to {} peer(s): {}",
                peer_count,
                peer_names.join(", ")
            );
            self.push_outbox(OutboundMessage {
                body: "Shutting down".to_string(),
                to: None,
                id: uuid::Uuid::new_v4(),
                reply_to: None,
                conversation_id: None,
                msg_type: Some(MessageType::Close),
                project_id: None,
                from_override: None,
            })
            .await;
        }

        // --- Phase 2: Wait for Close message delivery ---
        // The Close message is broadcast via the outbox channel. Each peer's
        // connection loop picks it up and sends it over the wire. We give the
        // network tasks a brief window to process the message. There is no ack
        // for Close messages (by protocol design), so this is best-effort.
        if peer_count > 0 {
            info!(
                "Waiting for Close message delivery to {} peer(s)...",
                peer_count
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // --- Phase 3: Disconnect all peers ---
        for peer in &peers {
            peer.disconnect.notify_one();
        }

        // --- Phase 4: Save all persistent state ---
        let mut save_errors = Vec::new();

        // Save friends store
        {
            let store = self.inner.friends.lock().await;
            if let Err(e) = store.save() {
                save_errors.push(format!("friends: {}", e));
            }
        }

        // Save friend requests
        {
            let store = self.inner.friend_requests.lock().await;
            if let Err(e) = store.save() {
                save_errors.push(format!("friend_requests: {}", e));
            }
        }

        // Save projects
        {
            let store = self.inner.projects.lock().await;
            if let Err(e) = store.save() {
                save_errors.push(format!("projects: {}", e));
            }
        }

        // Save project invitations
        {
            let store = self.inner.project_invitations.lock().await;
            if let Err(e) = store.save() {
                save_errors.push(format!("project_invitations: {}", e));
            }
        }

        // Save conversation history to disk
        {
            let history = self.inner.conversation_history.lock().await;
            if !history.is_empty() {
                let history_path = Self::conversation_history_path();
                if let Some(parent) = history_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match serde_json::to_string_pretty(&*history) {
                    Ok(data) => {
                        if let Err(e) = std::fs::write(&history_path, data) {
                            save_errors.push(format!("conversation_history: {}", e));
                        } else {
                            info!(
                                "Saved {} conversation message(s) to {}",
                                history.len(),
                                history_path.display()
                            );
                        }
                    }
                    Err(e) => save_errors.push(format!("conversation_history serialize: {}", e)),
                }
            }
        }

        if !save_errors.is_empty() {
            warn!("State save errors during shutdown: {:?}", save_errors);
        }

        // --- Phase 5: Log shutdown summary ---
        let uptime = chrono::Utc::now() - self.inner.start_time;
        let conversation_count = self.inner.conversation_history.lock().await.len();
        let friends_count = self.inner.friends.lock().await.friends.len();
        let outbox_stats = self.inner.outbox_store.lock().await.stats();

        info!("--- Shutdown Summary ---");
        info!("  Node:           {}", self.inner.node_name);
        info!(
            "  Uptime:         {}",
            format_duration(uptime.to_std().unwrap_or_default())
        );
        info!("  Peers:          {} disconnected", peer_count);
        info!("  Friends:        {}", friends_count);
        info!(
            "  Messages:       {} in conversation history",
            conversation_count
        );
        info!(
            "  Outbox:         {} queued, {} delivered",
            outbox_stats.total_queued, outbox_stats.total_delivered
        );
        info!("  Shutdown took:  {:?}", start.elapsed());
        info!("Goodbye.");
    }

    /// Path for persisting conversation history on shutdown.
    fn conversation_history_path() -> std::path::PathBuf {
crate::config::agora_home()
            .join("conversation_history.json")
    }

    /// Load conversation history from disk synchronously (called during construction).
    fn load_conversation_history_sync() -> Vec<StoredMessage> {
        let path = Self::conversation_history_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(data) => match serde_json::from_str::<Vec<StoredMessage>>(&data) {
                    Ok(mut messages) => {
                        // Per-conversation + global cap trimming
                        trim_conversation_history(&mut messages);
                        let count = messages.len();
                        if count > 0 {
                            info!("Restored {} conversation message(s) from disk", count);
                        }
                        messages
                    }
                    Err(e) => {
                        warn!("Failed to parse conversation history: {}", e);
                        Vec::new()
                    }
                },
                Err(e) => {
                    warn!("Failed to read conversation history: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    }

    /// Load conversation history from disk (called on startup, kept for compatibility).
    pub async fn load_conversation_history(&self) {
        let messages = Self::load_conversation_history_sync();
        if !messages.is_empty() {
            *self.inner.conversation_history.lock().await = messages;
        }
    }

    /// Save conversation history to disk (best-effort, non-blocking).
    /// Spawns a background task so the caller is not blocked on I/O errors.
    fn save_conversation_history(&self, history: &[StoredMessage]) {
        let path = self.inner.conversations_path.clone();
        match serde_json::to_string_pretty(history) {
            Ok(data) => {
                // Spawn blocking I/O off the async runtime
                tokio::spawn(async move {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&path, data) {
                        warn!("Failed to save conversation history: {}", e);
                    }
                });
            }
            Err(e) => warn!("Failed to serialize conversation history: {}", e),
        }
    }

    /// Legacy alias — calls graceful_shutdown.
    pub async fn broadcast_close(&self) {
        self.graceful_shutdown().await;
    }

    /// Check a peer's DID against their friend record. Implements TOFU
    /// (Trust On First Use) DID pinning per CONCEPT.md §11.4.
    pub async fn check_and_pin_did(&self, name: &str, did: &str) -> DidPinResult {
        let mut store = self.inner.friends.lock().await;
        let key = store.resolve_key(name);
        match key.and_then(|k| store.friends.get_mut(&k)) {
            None => DidPinResult::NotAFriend,
            Some(friend) => match &friend.did {
                None => {
                    // TOFU: first time seeing this friend's DID — pin it
                    friend.did = Some(did.to_string());
                    let _ = store.save();
                    info!("Pinned DID for {}: {}", name, did);
                    DidPinResult::FirstSeen
                }
                Some(existing) if existing == did => DidPinResult::Match,
                Some(existing) => DidPinResult::Mismatch {
                    expected: existing.clone(),
                },
            },
        }
    }

    // --- Friend management ---

    /// Get the trust level of a peer by name.
    pub async fn get_trust_level(&self, name: &str) -> TrustLevel {
        self.inner.friends.lock().await.get_trust_level(name)
    }

    /// Get all friends.
    pub async fn get_friends(&self) -> Vec<Friend> {
        self.inner
            .friends
            .lock()
            .await
            .list()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Add or update a friend, persisting to disk. Returns optional alias collision warning.
    pub async fn add_friend(&self, friend: Friend) -> anyhow::Result<Option<String>> {
        let mut store = self.inner.friends.lock().await;
        let warning = store.add(friend);
        store.save()?;
        Ok(warning)
    }

    /// Update a friend's last known address, persisting to disk.
    pub async fn update_friend_address(&self, name: &str, address: &str) {
        let mut store = self.inner.friends.lock().await;
        if store.update_address(name, address) {
            let _ = store.save(); // Best-effort
        }
    }

    /// Update a friend's verified DID (after Hello identity exchange).
    pub async fn update_friend_did(&self, name: &str, did: &str) {
        let mut store = self.inner.friends.lock().await;
        let key = store.resolve_key(name);
        if let Some(friend) = key.and_then(|k| store.friends.get_mut(&k)) {
            friend.did = Some(did.to_string());
            let _ = store.save();
        }
    }

    /// Set an alias on an existing friend (for auto-linking similar names).
    pub async fn set_friend_alias(&self, friend_name: &str, alias: &str) {
        let mut store = self.inner.friends.lock().await;
        if let Some(friend) = store.friends.get_mut(friend_name) {
            friend.alias = Some(alias.to_string());
            let _ = store.save();
        }
    }

    /// Find a friend whose name is similar to the given peer name.
    /// Checks: "alice" ↔ "alice-desktop", "bob" ↔ "bob-laptop", etc.
    /// Returns (friend_name, trust_level) if a likely match is found.
    pub async fn find_similar_friend(&self, peer_name: &str) -> Option<(String, TrustLevel)> {
        let store = self.inner.friends.lock().await;
        let peer_lower = peer_name.to_lowercase();
        store
            .friends
            .values()
            .find(|f| {
                let name_lower = f.name.to_lowercase();
                // Check if one name is a prefix of the other
                // "alice" matches "alice-desktop", "alice-laptop", etc.
                (peer_lower.starts_with(&name_lower) || name_lower.starts_with(&peer_lower))
                    && f.name != peer_name
            })
            .map(|f| (f.name.clone(), f.trust_level))
    }

    /// Merge any friends that share the same DID as the connecting peer.
    /// Returns a description of what was merged, or None if no duplicates.
    pub async fn merge_friend_by_did(&self, current_name: &str, did: &str) -> Option<String> {
        let mut store = self.inner.friends.lock().await;
        let result = store.merge_by_did(current_name, did);
        if result.is_some() {
            let _ = store.save();
        }
        result
    }

    /// Get friends that have stored addresses (for auto-connect).
    pub async fn friends_with_addresses(&self) -> Vec<(Friend, String)> {
        self.inner
            .friends
            .lock()
            .await
            .friends_with_addresses()
            .into_iter()
            .map(|(f, addr)| (f.clone(), addr.to_string()))
            .collect()
    }

    /// Remove a friend by name, persisting to disk. Returns true if found.
    pub async fn remove_friend(&self, name: &str) -> anyhow::Result<bool> {
        let mut store = self.inner.friends.lock().await;
        let removed = store.remove(name);
        if removed {
            store.save()?;
        }
        Ok(removed)
    }

    /// Partially update a friend (mute, trust, alias, notes). Returns true if found.
    pub async fn update_friend(&self, name: &str, patch: &FriendPatch) -> anyhow::Result<bool> {
        let mut store = self.inner.friends.lock().await;
        let found = store.update(name, patch);
        if found {
            store.save()?;
        }
        Ok(found)
    }

    // --- Friend request management ---

    /// Get all friend requests.
    pub async fn get_friend_requests(&self) -> Vec<FriendRequest> {
        self.inner.friend_requests.lock().await.list().to_vec()
    }

    /// Get pending inbound friend requests.
    pub async fn get_pending_inbound_requests(&self) -> Vec<FriendRequest> {
        self.inner
            .friend_requests
            .lock()
            .await
            .pending_inbound()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Add a friend request to the store.
    pub async fn add_friend_request(&self, request: FriendRequest) -> anyhow::Result<()> {
        let mut store = self.inner.friend_requests.lock().await;
        store.add(request);
        store.save()?;
        Ok(())
    }

    /// Check if we have a pending outbound request to a peer.
    pub async fn has_pending_outbound_to(&self, name: &str) -> bool {
        self.inner
            .friend_requests
            .lock()
            .await
            .pending_outbound_to(name)
            .is_some()
    }

    /// Get a pending outbound request to a specific peer (for re-send on reconnect).
    pub async fn get_pending_outbound_to(&self, name: &str) -> Option<FriendRequest> {
        self.inner
            .friend_requests
            .lock()
            .await
            .pending_outbound_to(name)
            .cloned()
    }

    /// Get a pending inbound request from a specific peer (for crossed request detection).
    pub async fn get_pending_inbound_from(&self, name: &str) -> Option<FriendRequest> {
        self.inner
            .friend_requests
            .lock()
            .await
            .pending_inbound_from(name)
            .cloned()
    }

    /// Get a friend request by ID.
    pub async fn get_friend_request(&self, id: &Uuid) -> Option<FriendRequest> {
        self.inner.friend_requests.lock().await.get(id).cloned()
    }

    /// Accept an inbound friend request. Adds the peer as a friend and marks the request as accepted.
    pub async fn accept_friend_request(
        &self,
        id: &Uuid,
        trust_level: u8,
    ) -> anyhow::Result<Option<FriendRequest>> {
        let mut req_store = self.inner.friend_requests.lock().await;
        let Some(request) = req_store.get_mut(id) else {
            return Ok(None);
        };
        if request.status != FriendRequestStatus::Pending
            || request.direction != FriendRequestDirection::Inbound
        {
            return Ok(None);
        }
        request.status = FriendRequestStatus::Accepted;
        request.resolved_at = Some(chrono::Utc::now());
        let request_clone = request.clone();
        req_store.save()?;
        drop(req_store);

        // Add as friend with the chosen trust level, setting their_trust to what they offered
        let friend = Friend {
            name: request_clone.peer_name.clone(),
            alias: None,
            trust_level: TrustLevel(trust_level.min(4)),
            added_at: chrono::Utc::now(),
            notes: None,
            muted: false,
            last_address: None,
            did: request_clone.peer_did.clone(),
            owner_did: request_clone.owner_did.clone(),
            their_trust: Some(request_clone.offered_trust),
        };
        let mut friends = self.inner.friends.lock().await;
        friends.add(friend);
        friends.save()?;

        Ok(Some(request_clone))
    }

    /// Reject an inbound friend request.
    pub async fn reject_friend_request(&self, id: &Uuid) -> anyhow::Result<Option<FriendRequest>> {
        let mut store = self.inner.friend_requests.lock().await;
        let Some(request) = store.get_mut(id) else {
            return Ok(None);
        };
        if request.status != FriendRequestStatus::Pending
            || request.direction != FriendRequestDirection::Inbound
        {
            return Ok(None);
        }
        request.status = FriendRequestStatus::Rejected;
        request.resolved_at = Some(chrono::Utc::now());
        let request_clone = request.clone();
        store.save()?;
        Ok(Some(request_clone))
    }

    /// Update the their_trust field on a friend (when we receive friend.accept).
    pub async fn update_their_trust(&self, name: &str, their_trust: u8) {
        let mut store = self.inner.friends.lock().await;
        let key = store.resolve_key(name);
        if let Some(friend) = key.and_then(|k| store.friends.get_mut(&k)) {
            friend.their_trust = Some(their_trust);
            let _ = store.save();
        }
    }

    /// Resolve an outbound request (when we receive friend.accept or friend.reject).
    pub async fn resolve_outbound_request(
        &self,
        peer_name: &str,
        accepted: bool,
    ) -> Option<FriendRequest> {
        let mut store = self.inner.friend_requests.lock().await;
        let request = store.requests.iter_mut().find(|r| {
            r.direction == FriendRequestDirection::Outbound
                && r.status == FriendRequestStatus::Pending
                && r.peer_name == peer_name
        });
        if let Some(req) = request {
            req.status = if accepted {
                FriendRequestStatus::Accepted
            } else {
                FriendRequestStatus::Rejected
            };
            req.resolved_at = Some(chrono::Utc::now());
            let clone = req.clone();
            let _ = store.save();
            Some(clone)
        } else {
            None
        }
    }

    // --- Conversation history ---

    /// Deterministic conversation ID for a 1:1 peer pair.
    /// Sorts the two names so alice↔bob always produces the same UUID.
    pub fn peer_conversation_id(&self, peer: &str) -> Uuid {
        let mut names = [self.inner.node_name.as_str(), peer];
        names.sort();
        let key = format!("agora:dm:{}:{}", names[0], names[1]);
        Uuid::new_v5(&Uuid::NAMESPACE_DNS, key.as_bytes())
    }

    /// Store an outbound message in conversation history.
    pub async fn store_outbound(
        &self,
        body: &str,
        to: Option<&str>,
        id: Uuid,
        reply_to: Option<Uuid>,
        conversation_id: Option<Uuid>,
        project_id: Option<Uuid>,
    ) {
        self.store_outbound_from(body, to, id, reply_to, conversation_id, project_id, None)
            .await;
    }

    /// Like store_outbound but allows overriding the sender name (for multi-agent daemons).
    pub async fn store_outbound_from(
        &self,
        body: &str,
        to: Option<&str>,
        id: Uuid,
        reply_to: Option<Uuid>,
        conversation_id: Option<Uuid>,
        project_id: Option<Uuid>,
        from_override: Option<&str>,
    ) {
        let _ = to; // included for future per-peer history
        // Sign the message body with the daemon's Ed25519 key
        let sig = self.inner.identity.sign(body.as_bytes());
        let sig_b58 = bs58::encode(&sig).into_string();
        let pk_b58 = self.inner.identity.public_key_base58();
        let stored = StoredMessage {
            id: id.to_string(),
            from: from_override.unwrap_or(&self.inner.node_name).to_string(),
            body: body.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            reply_to: reply_to.map(|u| u.to_string()),
            conversation_id: conversation_id.map(|u| u.to_string()),
            direction: "outbound".to_string(),
            project_id: project_id.map(|u| u.to_string()),
            signature: Some(sig_b58),
            public_key: Some(pk_b58),
        };
        let mut history = self.inner.conversation_history.lock().await;
        history.push(stored);
        // Per-conversation + global cap trimming
        trim_conversation_history(&mut history);
        self.save_conversation_history(&history);
    }

    /// Get all conversations grouped by conversation_id, with metadata.
    pub async fn get_conversations(&self) -> Vec<ConversationSummary> {
        let history = self.inner.conversation_history.lock().await;
        let mut convos: HashMap<String, Vec<&StoredMessage>> = HashMap::new();

        for msg in history.iter() {
            let key = msg
                .conversation_id
                .clone()
                .unwrap_or_else(|| msg.id.clone());
            convos.entry(key).or_default().push(msg);
        }

        let mut result: Vec<ConversationSummary> = convos
            .into_iter()
            .map(|(id, msgs)| {
                let message_count = msgs.len();
                let last = msgs.last().unwrap();
                let first = msgs.first().unwrap();
                let participants: Vec<String> = {
                    let mut p: Vec<String> = msgs
                        .iter()
                        .map(|m| m.from.clone())
                        .collect::<HashSet<_>>()
                        .into_iter()
                        .collect();
                    p.sort();
                    p
                };
                ConversationSummary {
                    conversation_id: id,
                    message_count,
                    participants,
                    first_message_at: first.timestamp.clone(),
                    last_message_at: last.timestamp.clone(),
                    preview: last.body.chars().take(100).collect(),
                }
            })
            .collect();

        result.sort_by(|a, b| b.last_message_at.cmp(&a.last_message_at));
        result
    }

    /// Get messages related to a specific project.
    pub async fn get_project_messages(&self, project_id: &str) -> Vec<StoredMessage> {
        let history = self.inner.conversation_history.lock().await;
        history
            .iter()
            .filter(|msg| msg.project_id.as_deref() == Some(project_id))
            .cloned()
            .collect()
    }

    /// Extract project_id from a project-related inbound message by parsing the body JSON.
    fn extract_project_id(msg: &crate::protocol::message::Message) -> Option<String> {
        use crate::protocol::message::MessageType;
        match msg.msg_type {
            MessageType::TaskAssign
            | MessageType::TaskUpdate
            | MessageType::TaskComplete
            | MessageType::ProjectStage
            | MessageType::ProjectClockIn
            | MessageType::ProjectClockOut
            | MessageType::ProjectUpdate
            | MessageType::ProjectInvite
            | MessageType::ProjectAccept
            | MessageType::ProjectDecline
            | MessageType::ProjectLeave
            | MessageType::ProjectSuspend
            | MessageType::ProjectUnsuspend => {
                // All these payloads have a "project_id" field
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg.body) {
                    v.get("project_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // --- Thread / sub-group management ---

    /// Create a new thread.
    pub async fn create_thread(
        &self,
        id: Option<Uuid>,
        creator: &str,
        title: Option<String>,
        participants: Vec<String>,
        min_trust: u8,
        closed: bool,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<Uuid, ThreadError> {
        let mut mgr = self.inner.threads.lock().await;
        mgr.create(
            id,
            creator,
            title,
            participants,
            min_trust,
            closed,
            metadata,
        )
    }

    /// List threads, optionally filtered by participant.
    pub async fn list_threads(&self, participant: Option<&str>) -> Vec<ThreadSummary> {
        let mgr = self.inner.threads.lock().await;
        mgr.list(participant)
    }

    /// Get a thread's details.
    pub async fn get_thread(&self, id: &Uuid) -> Option<crate::thread::Thread> {
        let mgr = self.inner.threads.lock().await;
        mgr.get(id).cloned()
    }

    /// Add a participant to an open thread.
    pub async fn thread_add_participant(
        &self,
        thread_id: &Uuid,
        inviter: &str,
        invitee: &str,
        invitee_trust: u8,
    ) -> Result<(), ThreadError> {
        let mut mgr = self.inner.threads.lock().await;
        mgr.add_participant(thread_id, inviter, invitee, invitee_trust)
    }

    /// Remove a participant from a thread.
    pub async fn thread_remove_participant(
        &self,
        thread_id: &Uuid,
        remover: &str,
        target: &str,
    ) -> Result<(), ThreadError> {
        let mut mgr = self.inner.threads.lock().await;
        mgr.remove_participant(thread_id, remover, target)
    }

    /// Close a thread.
    pub async fn close_thread(
        &self,
        thread_id: &Uuid,
        closer: &str,
        reason: Option<String>,
    ) -> Result<(), ThreadError> {
        let mut mgr = self.inner.threads.lock().await;
        mgr.close_thread(thread_id, closer, reason)
    }

    /// Update thread metadata/title.
    pub async fn update_thread(
        &self,
        thread_id: &Uuid,
        updater: &str,
        title: Option<String>,
        metadata: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), ThreadError> {
        let mut mgr = self.inner.threads.lock().await;
        mgr.update(thread_id, updater, title, metadata)
    }

    /// Get message routing for a thread.
    pub async fn thread_route(
        &self,
        thread_id: &Uuid,
        sender: &str,
    ) -> Result<Vec<String>, ThreadError> {
        let mgr = self.inner.threads.lock().await;
        mgr.route(thread_id, sender)
    }

    /// Get the message history for a specific conversation thread.
    pub async fn get_conversation(&self, conversation_id: &str) -> Vec<StoredMessage> {
        let history = self.inner.conversation_history.lock().await;
        history
            .iter()
            .filter(|m| {
                m.conversation_id.as_deref() == Some(conversation_id) || m.id == conversation_id
            })
            .cloned()
            .collect()
    }

    /// Delete an entire conversation and all its messages from history.
    /// Returns true if any messages were removed.
    pub async fn delete_conversation(&self, conversation_id: &str) -> bool {
        let mut history = self.inner.conversation_history.lock().await;
        let before = history.len();
        history.retain(|m| {
            m.conversation_id.as_deref() != Some(conversation_id) && m.id != conversation_id
        });
        history.len() < before
    }

    /// Delete a single message by id from conversation history.
    /// Returns true if the message was found and removed.
    pub async fn delete_message(&self, message_id: &str) -> bool {
        let mut history = self.inner.conversation_history.lock().await;
        let before = history.len();
        history.retain(|m| m.id != message_id);
        history.len() < before
    }

    // -----------------------------------------------------------------------
    // Project methods
    // -----------------------------------------------------------------------

    /// List all projects.
    pub async fn get_projects(&self) -> Vec<crate::project::Project> {
        self.inner.projects.lock().await.list().to_vec()
    }

    /// Get a specific project by ID.
    pub async fn get_project(&self, id: &Uuid) -> Option<crate::project::Project> {
        self.inner.projects.lock().await.get(id).cloned()
    }

    /// Create a new project. Returns the project ID.
    pub async fn create_project(
        &self,
        name: &str,
        description: Option<String>,
        repo: Option<String>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let project = crate::project::Project {
            id,
            name: name.to_string(),
            description,
            owner_did: self.did().to_string(),
            owner_name: self.node_name().to_string(),
            repo,
            status: crate::project::ProjectStatus::Active,
            agents: vec![crate::project::ProjectAgent {
                name: self.node_name().to_string(),
                did: Some(self.did().to_string()),
                role: crate::project::ProjectRole::Owner,
                joined_at: now,
                clocked_in: false,
                current_focus: None,
                last_clock_in: None,
                suspended: false,
                suspended_reason: None,
                muted: false,
            }],
            created_at: now,
            updated_at: now,
            notes: None,
            tasks: Vec::new(),
            audit_trail: Vec::new(),
            current_stage: None,
            rooms: vec![crate::project::ProjectRoom {
                id: Uuid::new_v4(),
                name: "main".to_string(),
                topic: Some("General project discussion".to_string()),
                conversation_id: crate::project::ProjectRoom::make_conversation_id(&id, "main"),
                created_at: now,
                created_by: self.node_name().to_string(),
            }],
        };
        let mut store = self.inner.projects.lock().await;
        store.add(project);
        let _ = store.save();
        id
    }

    /// Update project fields.
    pub async fn update_project(
        &self,
        id: &Uuid,
        status: Option<crate::project::ProjectStatus>,
        description: Option<String>,
        notes: Option<String>,
    ) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(id) {
            if let Some(s) = status {
                project.status = s;
            }
            if let Some(d) = description {
                project.description = Some(d);
            }
            if let Some(n) = notes {
                project.notes = Some(n);
            }
            project.updated_at = chrono::Utc::now();
            let _ = store.save();
            true
        } else {
            false
        }
    }

    /// Archive (soft-delete) a project.
    pub async fn archive_project(&self, id: &Uuid) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(id) {
            project.status = crate::project::ProjectStatus::Archived;
            project.updated_at = chrono::Utc::now();
            let _ = store.save();
            true
        } else {
            false
        }
    }

    /// Add an agent to a project.
    pub async fn add_project_agent(
        &self,
        project_id: &Uuid,
        name: &str,
        did: Option<String>,
        role: crate::project::ProjectRole,
    ) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            // Don't add duplicates
            if project.agents.iter().any(|a| a.name == name) {
                return false;
            }
            project.agents.push(crate::project::ProjectAgent {
                name: name.to_string(),
                did,
                role,
                joined_at: chrono::Utc::now(),
                clocked_in: false,
                current_focus: None,
                last_clock_in: None,
                suspended: false,
                suspended_reason: None,
                muted: false,
            });
            project.updated_at = chrono::Utc::now();
            let _ = store.save();
            true
        } else {
            false
        }
    }

    /// Remove an agent from a project.
    pub async fn remove_project_agent(&self, project_id: &Uuid, name: &str) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            let before = project.agents.len();
            project.agents.retain(|a| a.name != name);
            if project.agents.len() < before {
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
                return true;
            }
        }
        false
    }

    /// Clock in to a project.
    /// Get all agent names for a project.
    pub async fn get_project_agent_names(&self, project_id: &Uuid) -> Vec<String> {
        let store = self.inner.projects.lock().await;
        store
            .get(project_id)
            .map(|p| p.agents.iter().map(|a| a.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Auto-clock an agent into all active projects they're a member of.
    /// Called when a consumer registers (agent comes online).
    async fn auto_clock_in_agent(&self, agent_name: &str) {
        let store = self.inner.projects.lock().await;
        let project_ids: Vec<Uuid> = store
            .list()
            .iter()
            .filter(|p| {
                p.status == crate::project::ProjectStatus::Active
                    && p.agents.iter().any(|a| a.name == agent_name)
            })
            .map(|p| p.id)
            .collect();
        drop(store);

        for pid in project_ids {
            self.project_clock_in(
                &pid,
                agent_name,
                Some("Auto-clocked in on connect".to_string()),
            )
            .await;
            info!("Auto-clocked '{}' into project {}", agent_name, pid);
        }
    }

    /// Auto-advertise agent in marketplace based on project roles.
    /// Skips if agent already has a richer manually-set profile.
    async fn auto_advertise_agent(&self, agent_name: &str) {
        // Skip internal consumers
        if agent_name == "http-default" || agent_name.ends_with("-listener") {
            return;
        }
        // Skip if agent already has marketplace entry with description (manual/richer)
        if let Some(existing) = self.marketplace_get(agent_name).await {
            if existing.description.is_some() || existing.tools.len() > 2 {
                info!(
                    "Skipping auto-advertise for '{}' — has richer profile",
                    agent_name
                );
                return;
            }
        }

        // Derive capabilities from project roles
        let store = self.inner.projects.lock().await;
        let mut domains = Vec::new();
        let mut tools = Vec::new();

        for project in store.list() {
            if project.status != crate::project::ProjectStatus::Active {
                continue;
            }
            for agent in &project.agents {
                if agent.name == agent_name {
                    use crate::project::ProjectRole;
                    match agent.role {
                        ProjectRole::Developer => {
                            tools.push("code-development".to_string());
                            tools.push("bug-fixing".to_string());
                        }
                        ProjectRole::Reviewer => {
                            tools.push("code-review".to_string());
                            tools.push("testing".to_string());
                        }
                        ProjectRole::Owner | ProjectRole::Overseer => {
                            tools.push("project-management".to_string());
                            tools.push("coordination".to_string());
                        }
                        ProjectRole::Consultant => {
                            tools.push("design-proposals".to_string());
                        }
                        ProjectRole::Tester => {
                            tools.push("testing".to_string());
                            tools.push("qa".to_string());
                        }
                        _ => {}
                    }
                    // Add project repo as a domain hint
                    if let Some(ref repo) = project.repo {
                        if repo.contains("rust") || repo.contains("agora") {
                            domains.push("rust".to_string());
                        }
                    }
                }
            }
        }
        drop(store);

        tools.sort();
        tools.dedup();
        domains.sort();
        domains.dedup();

        // Merge with existing entry (don't overwrite richer manual data)
        let existing = self.marketplace_get(agent_name).await;
        let (merged_domains, merged_tools, description) = if let Some(existing) = existing {
            let mut d = existing.domains.clone();
            d.extend(domains);
            d.sort();
            d.dedup();
            let mut t = existing.tools.clone();
            t.extend(tools);
            t.sort();
            t.dedup();
            (d, t, existing.description.clone())
        } else {
            (domains, tools, None)
        };

        let caps = crate::marketplace::AgentCapabilities {
            agent_name: agent_name.to_string(),
            agent_did: None,
            domains: merged_domains,
            tools: merged_tools,
            availability: crate::marketplace::AgentAvailability::Available,
            description,
            updated_at: chrono::Utc::now(),
            address: None,
        };

        self.marketplace_upsert(caps).await;
        info!("Auto-advertised '{}' in marketplace", agent_name);
    }

    pub async fn project_clock_in(
        &self,
        project_id: &Uuid,
        name: &str,
        focus: Option<String>,
    ) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            if let Some(agent) = project.agents.iter_mut().find(|a| a.name == name) {
                agent.clocked_in = true;
                agent.current_focus = focus;
                agent.last_clock_in = Some(chrono::Utc::now());
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
                return true;
            }
        }
        false
    }

    /// Clock out of a project.
    pub async fn project_clock_out(&self, project_id: &Uuid, name: &str) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            if let Some(agent) = project.agents.iter_mut().find(|a| a.name == name) {
                agent.clocked_in = false;
                agent.current_focus = None;
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
                return true;
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Project invitation methods
    // -----------------------------------------------------------------------

    /// List all project invitations.
    pub async fn get_project_invitations(&self) -> Vec<crate::project::ProjectInvitation> {
        self.inner.project_invitations.lock().await.list().to_vec()
    }

    /// Get a single project invitation by ID.
    pub async fn get_project_invitation(
        &self,
        id: &Uuid,
    ) -> Option<crate::project::ProjectInvitation> {
        self.inner
            .project_invitations
            .lock()
            .await
            .list()
            .iter()
            .find(|i| i.id == *id)
            .cloned()
    }

    /// Add a project invitation.
    pub async fn add_project_invitation(
        &self,
        invitation: crate::project::ProjectInvitation,
    ) -> Result<(), String> {
        let mut store = self.inner.project_invitations.lock().await;
        store.add(invitation);
        store.save()
    }

    /// Accept a project invitation by ID.
    pub async fn accept_project_invitation(
        &self,
        id: &Uuid,
    ) -> Option<crate::project::ProjectInvitation> {
        let mut store = self.inner.project_invitations.lock().await;
        let result = store.accept(id).cloned();
        if result.is_some() {
            let _ = store.save();
        }
        result
    }

    /// Decline a project invitation by ID.
    pub async fn decline_project_invitation(
        &self,
        id: &Uuid,
    ) -> Option<crate::project::ProjectInvitation> {
        let mut store = self.inner.project_invitations.lock().await;
        let result = store.decline(id).cloned();
        if result.is_some() {
            let _ = store.save();
        }
        result
    }

    /// Resolve an outbound project invitation (peer accepted/declined).
    pub async fn resolve_outbound_project_invitation(
        &self,
        peer_name: &str,
        project_id: &Uuid,
        accepted: bool,
    ) {
        let mut store = self.inner.project_invitations.lock().await;
        if let Some(inv) = store.invitations.iter_mut().find(|i| {
            i.peer_name == peer_name
                && i.project_id == *project_id
                && i.direction == crate::project::InvitationDirection::Outbound
                && i.status == crate::project::InvitationStatus::Pending
        }) {
            inv.status = if accepted {
                crate::project::InvitationStatus::Accepted
            } else {
                crate::project::InvitationStatus::Declined
            };
            inv.resolved_at = Some(chrono::Utc::now());
            let _ = store.save();
        }
    }

    /// Create a project locally from an accepted invitation.
    pub async fn create_project_from_invitation(
        &self,
        project_id: Uuid,
        name: &str,
        description: Option<String>,
        repo: Option<String>,
        owner_name: &str,
        my_role: crate::project::ProjectRole,
    ) {
        let now = chrono::Utc::now();
        let project = crate::project::Project {
            id: project_id,
            name: name.to_string(),
            description,
            owner_did: String::new(),
            owner_name: owner_name.to_string(),
            repo,
            status: crate::project::ProjectStatus::Active,
            agents: vec![crate::project::ProjectAgent {
                name: self.node_name().to_string(),
                did: Some(self.did().to_string()),
                role: my_role,
                joined_at: now,
                clocked_in: false,
                current_focus: None,
                last_clock_in: None,
                suspended: false,
                suspended_reason: None,
                muted: false,
            }],
            created_at: now,
            updated_at: now,
            notes: None,
            tasks: Vec::new(),
            audit_trail: Vec::new(),
            current_stage: None,
            rooms: vec![crate::project::ProjectRoom {
                id: Uuid::new_v4(),
                name: "main".to_string(),
                topic: Some("General project discussion".to_string()),
                conversation_id: crate::project::ProjectRoom::make_conversation_id(
                    &project_id,
                    "main",
                ),
                created_at: now,
                created_by: owner_name.to_string(),
            }],
        };
        let mut store = self.inner.projects.lock().await;
        store.add(project);
        let _ = store.save();
    }

    /// Build a ProjectContext for an invitation.
    pub async fn build_project_context(
        &self,
        project_id: &Uuid,
        role: crate::project::ProjectRole,
    ) -> Option<crate::project::ProjectContext> {
        let store = self.inner.projects.lock().await;
        let project = store.get(project_id)?;
        Some(crate::project::ProjectContext {
            project_name: project.name.clone(),
            repo: project.repo.clone(),
            description: project.description.clone(),
            your_role: role,
            your_permissions: role.permissions().iter().map(|s| s.to_string()).collect(),
            current_agents: project.agents.iter().map(|a| a.name.clone()).collect(),
            notes: project.notes.clone(),
        })
    }

    // --- Permission checking ---

    /// Check whether a caller has a given permission in a project.
    ///
    /// Looks up the caller by DID (preferred) or name in the project's agents list.
    /// If the project has a `current_stage`, uses stage-aware permissions;
    /// otherwise uses the role's default permissions.
    ///
    /// Returns `Ok(role)` on success, `Err(reason)` on denial.
    pub async fn check_permission(
        &self,
        project_id: &Uuid,
        caller_name: &str,
        caller_did: Option<&str>,
        permission: &str,
    ) -> Result<crate::project::ProjectRole, String> {
        let store = self.inner.projects.lock().await;
        let project = store.get(project_id).ok_or("Project not found")?;

        // Find agent by DID first, then by name
        let agent = caller_did
            .and_then(|did| {
                project
                    .agents
                    .iter()
                    .find(|a| a.did.as_deref() == Some(did))
            })
            .or_else(|| project.agents.iter().find(|a| a.name == caller_name))
            .ok_or(format!(
                "Agent '{}' is not a member of this project",
                caller_name
            ))?;

        // Check if agent is suspended
        if agent.suspended {
            return Err(format!(
                "Agent '{}' is suspended{}",
                caller_name,
                agent
                    .suspended_reason
                    .as_deref()
                    .map(|r| format!(": {}", r))
                    .unwrap_or_default()
            ));
        }

        // Owner always has full permissions regardless of stage
        if agent.role == crate::project::ProjectRole::Owner {
            return Ok(agent.role);
        }

        // Check permission based on stage or role defaults
        let has_permission = if let Some(ref stage) = project.current_stage {
            stage.role_has_permission(&agent.role, permission)
        } else {
            agent.role.permissions().contains(&permission)
        };

        if has_permission {
            Ok(agent.role)
        } else {
            let stage_info = project
                .current_stage
                .as_ref()
                .map(|s| format!(" (stage: {})", s.name()))
                .unwrap_or_default();
            Err(format!(
                "Permission denied: {} role '{}' lacks '{}' permission{}",
                caller_name,
                agent.role.name(),
                permission,
                stage_info
            ))
        }
    }

    // --- Task management ---

    /// Get all tasks for a project.
    pub async fn get_tasks(&self, project_id: &Uuid) -> Option<Vec<crate::project::Task>> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| p.tasks.clone())
    }

    /// Create a task in a project. Returns the task ID.
    pub async fn create_task(
        &self,
        project_id: &Uuid,
        title: &str,
        description: Option<String>,
        assignee: Option<String>,
        priority: Option<crate::project::TaskPriority>,
        depends_on: Vec<Uuid>,
        created_by: Option<String>,
    ) -> Option<Uuid> {
        self.create_task_with_id(
            project_id,
            None,
            title,
            description,
            assignee,
            priority,
            depends_on,
            created_by,
        )
        .await
    }

    /// Create a task with an explicit ID (used for network-received tasks to keep IDs in sync).
    pub async fn create_task_with_id(
        &self,
        project_id: &Uuid,
        explicit_id: Option<Uuid>,
        title: &str,
        description: Option<String>,
        assignee: Option<String>,
        priority: Option<crate::project::TaskPriority>,
        depends_on: Vec<Uuid>,
        created_by: Option<String>,
    ) -> Option<Uuid> {
        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id)?;
        let now = chrono::Utc::now();
        let task_id = explicit_id.unwrap_or_else(Uuid::new_v4);
        // Skip if task with this ID already exists (idempotent)
        if project.tasks.iter().any(|t| t.id == task_id) {
            return Some(task_id);
        }

        // If task has unresolved dependencies, start as Blocked
        let status = if !depends_on.is_empty() {
            let all_done = depends_on.iter().all(|dep_id| {
                project
                    .tasks
                    .iter()
                    .any(|t| t.id == *dep_id && t.status == crate::project::TaskStatus::Done)
            });
            if all_done {
                crate::project::TaskStatus::Todo
            } else {
                crate::project::TaskStatus::Blocked
            }
        } else {
            crate::project::TaskStatus::Todo
        };

        project.tasks.push(crate::project::Task {
            id: task_id,
            title: title.to_string(),
            description,
            status,
            assignee,
            priority,
            depends_on,
            created_at: now,
            updated_at: now,
            created_by,
            github_issue_number: None,
        });
        project.updated_at = now;
        let _ = store.save();
        Some(task_id)
    }

    /// Import tasks from GitHub (adds tasks that don't already exist by issue number).
    pub async fn import_github_tasks(
        &self,
        project_id: &Uuid,
        tasks: Vec<crate::project::Task>,
    ) -> usize {
        let mut store = self.inner.projects.lock().await;
        let Some(project) = store.get_mut(project_id) else {
            return 0;
        };

        let existing_issue_numbers: std::collections::HashSet<u64> = project
            .tasks
            .iter()
            .filter_map(|t| t.github_issue_number)
            .collect();

        let mut imported = 0;
        for task in tasks {
            if let Some(num) = task.github_issue_number {
                if !existing_issue_numbers.contains(&num) {
                    project.tasks.push(task);
                    imported += 1;
                }
            }
        }

        if imported > 0 {
            project.updated_at = chrono::Utc::now();
            let _ = store.save();
        }
        imported
    }

    /// Get project tasks for GitHub sync.
    pub async fn get_project_tasks(&self, project_id: &Uuid) -> Option<Vec<crate::project::Task>> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| p.tasks.clone())
    }

    /// Get project repo URL.
    pub async fn get_project_repo(&self, project_id: &Uuid) -> Option<Option<String>> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| p.repo.clone())
    }

    /// Update a task's fields. Returns list of unblocked task IDs if status changed to Done.
    pub async fn update_task(
        &self,
        project_id: &Uuid,
        task_id: &Uuid,
        status: Option<crate::project::TaskStatus>,
        title: Option<String>,
        description: Option<String>,
        assignee: Option<String>,
    ) -> Result<Vec<Uuid>, String> {
        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let task = project
            .tasks
            .iter_mut()
            .find(|t| t.id == *task_id)
            .ok_or("Task not found")?;

        let now = chrono::Utc::now();
        let became_done = status == Some(crate::project::TaskStatus::Done)
            && task.status != crate::project::TaskStatus::Done;

        if let Some(s) = status {
            task.status = s;
        }
        if let Some(t) = title {
            task.title = t;
        }
        if let Some(d) = description {
            task.description = Some(d);
        }
        if let Some(a) = assignee {
            task.assignee = Some(a);
        }
        task.updated_at = now;
        project.updated_at = now;

        // Auto-unblock: if this task became Done, check dependents
        let mut unblocked = Vec::new();
        if became_done {
            let completed_id = *task_id;
            // First pass: collect done task IDs for checking deps
            let done_ids: std::collections::HashSet<Uuid> = project
                .tasks
                .iter()
                .filter(|t| t.status == crate::project::TaskStatus::Done)
                .map(|t| t.id)
                .collect();
            // Second pass: find blocked tasks whose deps are now all done
            let to_unblock: Vec<Uuid> = project
                .tasks
                .iter()
                .filter(|t| {
                    t.status == crate::project::TaskStatus::Blocked
                        && t.depends_on.contains(&completed_id)
                        && t.depends_on.iter().all(|dep_id| done_ids.contains(dep_id))
                })
                .map(|t| t.id)
                .collect();
            // Third pass: mutate
            for t in project.tasks.iter_mut() {
                if to_unblock.contains(&t.id) {
                    t.status = crate::project::TaskStatus::Todo;
                    t.updated_at = now;
                    unblocked.push(t.id);
                }
            }
        }

        // Record reputation contribution when task is completed
        let assignee_for_rep = if became_done {
            project
                .tasks
                .iter()
                .find(|t| t.id == *task_id)
                .and_then(|t| t.assignee.clone())
        } else {
            None
        };

        let _ = store.save();
        drop(store);

        // Record reputation outside the lock
        if let Some(assignee) = assignee_for_rep {
            self.reputation_record(crate::reputation::Contribution {
                id: Uuid::new_v4(),
                agent_name: assignee.clone(),
                contribution_type: crate::reputation::ContributionType::TaskCompleted,
                project_id: Some(*project_id),
                quality: 1.0,
                timestamp: chrono::Utc::now(),
                description: None,
            })
            .await;
            info!("Reputation recorded: {} completed a task", assignee);
        }

        Ok(unblocked)
    }

    /// Set the GitHub issue number on a task (after pushing to GitHub).
    pub async fn set_task_github_issue_number(
        &self,
        project_id: &Uuid,
        task_id: &Uuid,
        issue_number: u64,
    ) {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            if let Some(task) = project.tasks.iter_mut().find(|t| t.id == *task_id) {
                task.github_issue_number = Some(issue_number);
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
            }
        }
    }

    /// Delete a task from a project.
    pub async fn delete_task(&self, project_id: &Uuid, task_id: &Uuid) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            let before = project.tasks.len();
            project.tasks.retain(|t| t.id != *task_id);
            if project.tasks.len() < before {
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
                return true;
            }
        }
        false
    }

    /// Assign a task to an agent.
    pub async fn assign_task(&self, project_id: &Uuid, task_id: &Uuid, assignee: &str) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            if let Some(task) = project.tasks.iter_mut().find(|t| t.id == *task_id) {
                task.assignee = Some(assignee.to_string());
                task.updated_at = chrono::Utc::now();
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
                return true;
            }
        }
        false
    }

    // --- Audit trail ---

    /// Append a signed audit entry to a project and broadcast to peers.
    pub async fn append_audit(&self, project_id: &Uuid, action: &str, detail: &str) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            let entry = crate::project::AuditEntry::new_signed(
                self.did(),
                self.node_name(),
                action,
                detail,
                self.identity(),
            );
            let entry_clone = entry.clone();
            project.audit_trail.push(entry);
            let _ = store.save();
            drop(store);

            // Broadcast audit entry to peers
            use crate::protocol::message::{AuditEntryPayload, MessageType};
            let payload = AuditEntryPayload {
                project_id: *project_id,
                entry: entry_clone,
            };
            let body = serde_json::to_string(&payload).unwrap_or_default();
            self.push_outbox(OutboundMessage {
                body,
                to: None,
                id: Uuid::new_v4(),
                reply_to: None,
                conversation_id: None,
                msg_type: Some(MessageType::AuditEntry),
                project_id: Some(*project_id),
                from_override: None,
            })
            .await;

            return true;
        }
        false
    }

    /// Merge a remote audit entry into the local project, deduplicating by ID.
    pub async fn merge_audit_entry(
        &self,
        project_id: &Uuid,
        entry: crate::project::AuditEntry,
    ) -> bool {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            // Dedup by entry UUID
            if project.audit_trail.iter().any(|e| e.id == entry.id) {
                return false; // Already have this entry
            }
            project.audit_trail.push(entry);
            // Sort by timestamp to maintain chronological order
            project
                .audit_trail
                .sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            let _ = store.save();
            return true;
        }
        false
    }

    /// Get audit trail entries for a project, with optional pagination.
    pub async fn get_audit(
        &self,
        project_id: &Uuid,
        offset: usize,
        limit: usize,
    ) -> Option<Vec<crate::project::AuditEntry>> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| {
            p.audit_trail
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect()
        })
    }

    /// Get audit trail count for a project.
    pub async fn get_audit_count(&self, project_id: &Uuid) -> Option<usize> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| p.audit_trail.len())
    }

    // --- Stage management ---

    /// Get the current stage of a project.
    pub async fn get_project_stage(
        &self,
        project_id: &Uuid,
    ) -> Option<Option<crate::project::ProjectStage>> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| p.current_stage.clone())
    }

    /// Set the project stage directly.
    pub async fn set_project_stage(
        &self,
        project_id: &Uuid,
        stage: crate::project::ProjectStage,
    ) -> Result<Option<crate::project::ProjectStage>, String> {
        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let previous = project.current_stage.clone();
        project.current_stage = Some(stage);
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        Ok(previous)
    }

    /// Advance to the next stage. Returns the new stage, or error if can't advance.
    pub async fn advance_project_stage(
        &self,
        project_id: &Uuid,
    ) -> Result<crate::project::ProjectStage, String> {
        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;

        let current = project
            .current_stage
            .clone()
            .unwrap_or(crate::project::ProjectStage::Investigation);

        if !crate::project::ProjectStage::can_advance(project) {
            return Err("Cannot advance: not all tasks are done".to_string());
        }

        let next = current
            .next()
            .ok_or("Already at final stage (Deployment)")?;
        let prev = project.current_stage.clone();
        project.current_stage = Some(next.clone());
        project.updated_at = chrono::Utc::now();
        let _ = store.save();

        drop(store);
        // Auto-audit the stage change
        self.append_audit(
            project_id,
            "project.stage_changed",
            &format!(
                "{} → {}",
                prev.map(|s| s.name().to_string())
                    .unwrap_or("none".to_string()),
                next.name()
            ),
        )
        .await;

        Ok(next)
    }

    // --- Agent suspension ---

    /// Suspend an agent in a project. Requires the caller to have "coordinate" permission.
    /// Change an agent's role in a project.
    pub async fn set_agent_role(
        &self,
        project_id: &Uuid,
        agent_name: &str,
        role: crate::project::ProjectRole,
    ) -> Result<(), String> {
        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let agent = project
            .agents
            .iter_mut()
            .find(|a| a.name == agent_name)
            .ok_or(format!("Agent '{}' not found in project", agent_name))?;
        agent.role = role;
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        Ok(())
    }

    pub async fn suspend_agent(
        &self,
        project_id: &Uuid,
        caller_name: &str,
        target_name: &str,
        reason: Option<String>,
    ) -> Result<(), String> {
        // Check caller has coordinate permission (must drop lock before re-acquiring)
        self.check_permission(project_id, caller_name, None, "coordinate")
            .await?;

        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let agent = project
            .agents
            .iter_mut()
            .find(|a| a.name == target_name)
            .ok_or(format!("Agent '{}' not found in project", target_name))?;

        agent.suspended = true;
        agent.suspended_reason = reason.clone();
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        drop(store);

        self.append_audit(
            project_id,
            "agent.suspended",
            &format!(
                "{} suspended {}{}",
                caller_name,
                target_name,
                reason.map(|r| format!(": {}", r)).unwrap_or_default()
            ),
        )
        .await;

        Ok(())
    }

    /// Unsuspend an agent in a project. Requires "coordinate" permission.
    pub async fn unsuspend_agent(
        &self,
        project_id: &Uuid,
        caller_name: &str,
        target_name: &str,
    ) -> Result<(), String> {
        self.check_permission(project_id, caller_name, None, "coordinate")
            .await?;

        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let agent = project
            .agents
            .iter_mut()
            .find(|a| a.name == target_name)
            .ok_or(format!("Agent '{}' not found in project", target_name))?;

        agent.suspended = false;
        agent.suspended_reason = None;
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        drop(store);

        self.append_audit(
            project_id,
            "agent.unsuspended",
            &format!("{} unsuspended {}", caller_name, target_name),
        )
        .await;

        Ok(())
    }

    /// Apply a remote suspension (permission already verified by caller).
    pub async fn apply_remote_suspend(
        &self,
        project_id: &Uuid,
        target_name: &str,
        reason: Option<String>,
    ) {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            if let Some(agent) = project.agents.iter_mut().find(|a| a.name == target_name) {
                agent.suspended = true;
                agent.suspended_reason = reason;
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
            }
        }
    }

    /// Apply a remote unsuspension (permission already verified by caller).
    pub async fn apply_remote_unsuspend(&self, project_id: &Uuid, target_name: &str) {
        let mut store = self.inner.projects.lock().await;
        if let Some(project) = store.get_mut(project_id) {
            if let Some(agent) = project.agents.iter_mut().find(|a| a.name == target_name) {
                agent.suspended = false;
                agent.suspended_reason = None;
                project.updated_at = chrono::Utc::now();
                let _ = store.save();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Project Rooms
    // -----------------------------------------------------------------------

    /// List all rooms for a project (including the auto-created main room).
    pub async fn get_project_rooms(
        &self,
        project_id: &Uuid,
    ) -> Option<Vec<crate::project::ProjectRoom>> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).map(|p| p.rooms.clone())
    }

    /// Create a breakout room in a project. Returns the new room, or an error.
    pub async fn create_project_room(
        &self,
        project_id: &Uuid,
        name: &str,
        topic: Option<String>,
        created_by: &str,
    ) -> Result<crate::project::ProjectRoom, String> {
        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;

        // Don't allow duplicate room names
        if project.rooms.iter().any(|r| r.name == name) {
            return Err(format!("Room '{}' already exists", name));
        }

        let room = crate::project::ProjectRoom {
            id: Uuid::new_v4(),
            name: name.to_string(),
            topic,
            conversation_id: crate::project::ProjectRoom::make_conversation_id(project_id, name),
            created_at: chrono::Utc::now(),
            created_by: created_by.to_string(),
        };
        let result = room.clone();
        project.rooms.push(room);
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        Ok(result)
    }

    /// Get a specific room in a project by room ID.
    pub async fn get_project_room(
        &self,
        project_id: &Uuid,
        room_id: &Uuid,
    ) -> Option<crate::project::ProjectRoom> {
        let store = self.inner.projects.lock().await;
        store
            .get(project_id)
            .and_then(|p| p.rooms.iter().find(|r| r.id == *room_id).cloned())
    }

    /// Get the main room for a project.
    pub async fn get_main_room(&self, project_id: &Uuid) -> Option<crate::project::ProjectRoom> {
        let store = self.inner.projects.lock().await;
        store
            .get(project_id)
            .and_then(|p| p.rooms.iter().find(|r| r.name == "main").cloned())
    }

    /// Look up which project owns a given conversation_id (i.e. a room thread).
    /// Returns the project UUID if any room across all projects has this conversation_id.
    pub async fn project_id_for_conversation(&self, conversation_id: &Uuid) -> Option<Uuid> {
        let store = self.inner.projects.lock().await;
        for project in store.projects.iter() {
            if project
                .rooms
                .iter()
                .any(|r| r.conversation_id == *conversation_id)
            {
                return Some(project.id);
            }
        }
        None
    }

    /// Check if an agent is muted in a project.
    pub async fn is_agent_muted(&self, project_id: &Uuid, agent_name: &str) -> Option<bool> {
        let store = self.inner.projects.lock().await;
        store.get(project_id).and_then(|p| {
            p.agents
                .iter()
                .find(|a| a.name == agent_name)
                .map(|a| a.muted)
        })
    }

    // -----------------------------------------------------------------------
    // Mute / Unmute
    // -----------------------------------------------------------------------

    /// Mute an agent in a project (they can read but not send in rooms).
    /// Requires "coordinate" permission.
    pub async fn mute_agent(
        &self,
        project_id: &Uuid,
        caller_name: &str,
        target_name: &str,
    ) -> Result<(), String> {
        self.check_permission(project_id, caller_name, None, "coordinate")
            .await?;

        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let agent = project
            .agents
            .iter_mut()
            .find(|a| a.name == target_name)
            .ok_or(format!("Agent '{}' not found in project", target_name))?;

        agent.muted = true;
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        drop(store);

        self.append_audit(
            project_id,
            "agent.muted",
            &format!("{} muted {}", caller_name, target_name),
        )
        .await;

        Ok(())
    }

    /// Unmute an agent in a project. Requires "coordinate" permission.
    pub async fn unmute_agent(
        &self,
        project_id: &Uuid,
        caller_name: &str,
        target_name: &str,
    ) -> Result<(), String> {
        self.check_permission(project_id, caller_name, None, "coordinate")
            .await?;

        let mut store = self.inner.projects.lock().await;
        let project = store.get_mut(project_id).ok_or("Project not found")?;
        let agent = project
            .agents
            .iter_mut()
            .find(|a| a.name == target_name)
            .ok_or(format!("Agent '{}' not found in project", target_name))?;

        agent.muted = false;
        project.updated_at = chrono::Utc::now();
        let _ = store.save();
        drop(store);

        self.append_audit(
            project_id,
            "agent.unmuted",
            &format!("{} unmuted {}", caller_name, target_name),
        )
        .await;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Outbox store (offline message queue)
    // -----------------------------------------------------------------------

    /// Enqueue a message for an offline peer.
    pub async fn outbox_enqueue(&self, msg: crate::outbox::QueuedMessage) -> bool {
        let mut store = self.inner.outbox_store.lock().await;
        store.enqueue(msg)
    }

    /// Mark a message as delivered (ack received).
    pub async fn outbox_ack(&self, peer: &str, message_id: &Uuid) -> bool {
        let mut store = self.inner.outbox_store.lock().await;
        store.ack(peer, message_id)
    }

    /// Get pending (undelivered) messages for a peer.
    pub async fn outbox_pending_for(&self, peer: &str) -> Vec<crate::outbox::QueuedMessage> {
        let store = self.inner.outbox_store.lock().await;
        store.pending_for(peer)
    }

    /// Get outbox stats.
    pub async fn outbox_stats(&self) -> crate::outbox::OutboxStats {
        let store = self.inner.outbox_store.lock().await;
        store.stats()
    }

    /// Garbage-collect delivered messages for a peer.
    pub async fn outbox_gc(&self, peer: &str) {
        let mut store = self.inner.outbox_store.lock().await;
        store.gc_delivered(peer);
    }

    /// Check if an inbound message ID has already been seen (dedup).
    pub async fn outbox_is_seen(&self, id: &Uuid) -> bool {
        let store = self.inner.outbox_store.lock().await;
        store.is_seen(id)
    }

    /// Mark an inbound message ID as seen (dedup).
    pub async fn outbox_mark_seen(&self, id: Uuid) {
        let mut store = self.inner.outbox_store.lock().await;
        store.mark_seen(id);
    }

    // -----------------------------------------------------------------------
    // Marketplace
    // -----------------------------------------------------------------------

    // --- Discovery (gossip-based network discovery) ---

    pub async fn discovery_upsert(&self, agent: crate::discovery::DiscoveredAgent) {
        let mut store = self.inner.discovery.lock().await;
        store.upsert_agent(agent);
        let _ = store.save(&crate::discovery::DiscoveryStore::default_path());
    }

    pub async fn discovery_upsert_project_ad(&self, ad: crate::discovery::ProjectAd) {
        let mut store = self.inner.discovery.lock().await;
        store.upsert_project_ad(ad);
        let _ = store.save(&crate::discovery::DiscoveryStore::default_path());
    }

    pub async fn discovery_list(&self) -> Vec<crate::discovery::DiscoveredAgent> {
        let store = self.inner.discovery.lock().await;
        store.list_agents().into_iter().cloned().collect()
    }

    pub async fn discovery_search(&self, query: &str) -> Vec<crate::discovery::DiscoveredAgent> {
        let store = self.inner.discovery.lock().await;
        store.search_agents(query).into_iter().cloned().collect()
    }

    pub async fn discovery_get(&self, did: &str) -> Option<crate::discovery::DiscoveredAgent> {
        let store = self.inner.discovery.lock().await;
        store.get_agent(did).cloned()
    }

    pub async fn discovery_project_ads(&self) -> Vec<crate::discovery::ProjectAd> {
        let store = self.inner.discovery.lock().await;
        store.list_project_ads().into_iter().cloned().collect()
    }

    pub async fn discovery_stats(&self) -> crate::discovery::DiscoveryStats {
        let store = self.inner.discovery.lock().await;
        store.stats()
    }

    pub async fn discovery_prune(&self) {
        let mut store = self.inner.discovery.lock().await;
        store.prune(crate::discovery::MAX_DISCOVERY_AGE_SECS);
        let _ = store.save(&crate::discovery::DiscoveryStore::default_path());
    }

    // --- Marketplace ---

    pub async fn marketplace_get(
        &self,
        agent_name: &str,
    ) -> Option<crate::marketplace::AgentCapabilities> {
        let store = self.inner.marketplace.lock().await;
        store.get(agent_name).cloned()
    }

    pub async fn marketplace_upsert(&self, caps: crate::marketplace::AgentCapabilities) -> bool {
        let mut store = self.inner.marketplace.lock().await;
        let updated = store.upsert(caps);
        let _ = store.save(&crate::marketplace::MarketplaceStore::default_path());
        updated
    }

    pub async fn marketplace_search(
        &self,
        query: &crate::marketplace::AgentSearchQuery,
    ) -> Vec<crate::marketplace::AgentSearchResult> {
        let store = self.inner.marketplace.lock().await;
        store.search(query)
    }

    pub async fn marketplace_list(&self) -> Vec<crate::marketplace::AgentCapabilities> {
        let store = self.inner.marketplace.lock().await;
        store.list().to_vec()
    }

    pub async fn marketplace_remove(&self, agent_name: &str) -> bool {
        let mut store = self.inner.marketplace.lock().await;
        let removed = store.remove(agent_name);
        let _ = store.save(&crate::marketplace::MarketplaceStore::default_path());
        removed
    }

    // -----------------------------------------------------------------------
    // Reputation
    // -----------------------------------------------------------------------

    pub async fn reputation_record(&self, contribution: crate::reputation::Contribution) {
        let mut store = self.inner.reputation.lock().await;
        store.record(contribution);
        let _ = store.save(&crate::reputation::ReputationStore::default_path());
    }

    pub async fn reputation_get(&self, agent_name: &str) -> crate::reputation::AgentReputation {
        let store = self.inner.reputation.lock().await;
        store.reputation(agent_name)
    }

    pub async fn reputation_leaderboard(&self) -> Vec<crate::reputation::AgentReputation> {
        let store = self.inner.reputation.lock().await;
        store.leaderboard()
    }

    pub async fn reputation_recommendations(&self) -> Vec<crate::reputation::TrustRecommendation> {
        let friends = self.inner.friends.lock().await;
        let trusts: Vec<(String, u8)> = friends
            .list()
            .iter()
            .map(|f| (f.name.clone(), f.trust_level.0))
            .collect();
        drop(friends);
        let store = self.inner.reputation.lock().await;
        store.recommendations(&trusts)
    }

    // -----------------------------------------------------------------------
    // Coordinator
    // -----------------------------------------------------------------------

    pub async fn coordinator_suggestions(
        &self,
        project_id: &Uuid,
    ) -> Vec<crate::coordinator::CoordinatorSuggestion> {
        let store = self.inner.coordinator_suggestions.lock().await;
        store.get(project_id).cloned().unwrap_or_default()
    }

    pub async fn coordinator_add_suggestions(
        &self,
        project_id: Uuid,
        suggestions: Vec<crate::coordinator::CoordinatorSuggestion>,
    ) {
        let mut store = self.inner.coordinator_suggestions.lock().await;
        let entry = store.entry(project_id).or_default();
        // Keep only previously acted-on suggestions, then add fresh ones.
        // This prevents duplicate suggestions from accumulating on repeated calls.
        entry.retain(|s| s.acted_on);
        entry.extend(suggestions);
    }

    pub async fn coordinator_act(&self, project_id: &Uuid, suggestion_id: &Uuid) -> bool {
        let mut store = self.inner.coordinator_suggestions.lock().await;
        if let Some(suggestions) = store.get_mut(project_id) {
            if let Some(s) = suggestions.iter_mut().find(|s| s.id == *suggestion_id) {
                s.acted_on = true;
                return true;
            }
        }
        false
    }

    pub async fn coordinator_add_digest(
        &self,
        project_id: Uuid,
        digest: crate::coordinator::ProjectDigest,
    ) {
        let mut store = self.inner.coordinator_digests.lock().await;
        store.entry(project_id).or_default().push(digest);
    }

    pub async fn coordinator_digests(
        &self,
        project_id: &Uuid,
    ) -> Vec<crate::coordinator::ProjectDigest> {
        let store = self.inner.coordinator_digests.lock().await;
        store.get(project_id).cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Format a Duration into a human-readable string like "2h 15m 30s".
fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        format!("{}h {}m {}s", h, m, s)
    }
}

// ---------------------------------------------------------------------------
// Wake command persistence
// ---------------------------------------------------------------------------

/// Default path: `~/.agora/wake.json`
fn wake_command_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    crate::config::agora_home().join("wake.json")
}

fn load_wake_command(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(val) => val
                .get("command")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            Err(e) => {
                warn!("Failed to parse {}: {}", path.display(), e);
                None
            }
        },
        Err(e) => {
            warn!("Failed to read {}: {}", path.display(), e);
            None
        }
    }
}

fn save_wake_command(path: &Path, cmd: Option<&str>) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!("Failed to create {}: {}", parent.display(), e);
            return;
        }
    }
    let val = serde_json::json!({ "command": cmd });
    match serde_json::to_string_pretty(&val) {
        Ok(data) => {
            if let Err(e) = std::fs::write(path, data) {
                warn!("Failed to write {}: {}", path.display(), e);
            }
        }
        Err(e) => warn!("Failed to serialize wake command: {}", e),
    }
}

/// Wait 3 seconds then fire the wake command. If aborted (by a new message
/// arriving within the window), this task simply stops — the replacement
/// task will fire instead with an incremented count.
///
/// The `messages` parameter contains clones of all messages that triggered
/// the wake — these are written to a temp file so the woken session can
/// access them even if the inbox has already been drained by a background
/// MCP monitor.
async fn fire_wake_after_delay(
    inner: Arc<Inner>,
    cmd: String,
    from: String,
    preview: String,
    trust: TrustLevel,
    count: usize,
    messages: Vec<Message>,
) {
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Clear the debounce slot now that we're firing
    *inner.wake_debounce.lock().await = None;

    let trust_str = trust.0.to_string();
    let count_str = count.to_string();

    // Sanitize env vars to prevent injection through crafted peer names/messages
    let safe_from = sanitize_env_value(&from, 500);
    let safe_preview = sanitize_env_value(&preview, 500);

    // Write messages to a temp file so the wake script can read them
    // even after the inbox has been drained by other consumers.
    let messages_file = write_wake_messages(&messages);

    info!(
        "Wake-up hook firing for {} ({} message(s), trust {}, messages_file={:?})",
        from, count, trust_str, messages_file
    );

    // Execute the wake command directly (not via sh -c) to prevent shell injection.
    // The command path has already been validated by validate_wake_command().
    // Split the command into program and arguments on whitespace boundaries.
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let (program, args) = match parts.split_first() {
        Some((prog, rest)) => (*prog, rest),
        None => {
            warn!("Wake command is empty after splitting — skipping");
            return;
        }
    };
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .env("AGORA_FROM", &safe_from)
        .env("AGORA_PREVIEW", &safe_preview)
        .env("AGORA_TRUST", &trust_str)
        .env("AGORA_MESSAGE_COUNT", &count_str)
        .env("AGORA_API_PORT", inner.api_port.to_string())
        .env(
            "AGORA_API_URL",
            format!("http://127.0.0.1:{}", inner.api_port),
        )
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDECODE");

    // Set AGORA_CONVERSATION_ID if the triggering message has one
    if let Some(conv_id) = messages.first().and_then(|m| m.conversation_id) {
        command.env("AGORA_CONVERSATION_ID", conv_id.to_string());
    }

    // Set AGORA_MESSAGES_FILE if we successfully wrote the temp file
    if let Some(ref path) = messages_file {
        command.env("AGORA_MESSAGES_FILE", path);
    }

    match command.spawn() {
        Ok(_) => {
            *inner.last_wake_event.lock().await = Some(LastWakeEvent {
                fired_at: chrono::Utc::now(),
                from: from.clone(),
                message_count: count,
            });
            info!("Wake-up hook spawned for {} ({} message(s))", from, count);
        }
        Err(e) => warn!("Wake-up hook failed ({}): {}", cmd, e),
    }

    // Clean up old wake message temp files (keep only the latest)
    cleanup_old_wake_files(messages_file.as_deref());
}

/// Remove old agora-wake-messages-*.json files from /tmp, keeping only the current one.
fn cleanup_old_wake_files(current: Option<&str>) {
    let Ok(entries) = std::fs::read_dir("/tmp") else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("agora-wake-messages-") && name.ends_with(".json") {
                let path_str = path.to_string_lossy();
                if current.is_none_or(|c| c != path_str.as_ref()) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// Write wake-triggering messages to a temp file as JSON.
/// Returns the file path on success, or None on failure.
fn write_wake_messages(messages: &[Message]) -> Option<String> {
    // Serialize messages to the same format as the HTTP API (with threading fields)
    let api_msgs: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id.to_string(),
                "from": m.from,
                "body": m.body,
                "timestamp": m.timestamp.to_rfc3339(),
                "reply_to": m.reply_to.map(|u| u.to_string()),
                "conversation_id": m.conversation_id.map(|u| u.to_string()),
            })
        })
        .collect();

    let json = match serde_json::to_string_pretty(&api_msgs) {
        Ok(j) => j,
        Err(e) => {
            warn!("Failed to serialize wake messages: {}", e);
            return None;
        }
    };

    let path = format!(
        "/tmp/agora-wake-messages-{}-{}.json",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    match std::fs::write(&path, &json) {
        Ok(()) => {
            // Set restrictive permissions on the temp file (Unix only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o600);
                let _ = std::fs::set_permissions(&path, perms);
            }
            Some(path)
        }
        Err(e) => {
            warn!("Failed to write wake messages to {}: {}", path, e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Security helpers
// ---------------------------------------------------------------------------

/// Validate a wake command for shell-injection safety.
/// Rejects commands containing dangerous shell metacharacters.
fn validate_wake_command(cmd: &str) -> Result<(), String> {
    if cmd.trim().is_empty() {
        return Err("Wake command cannot be empty".to_string());
    }
    // Reject shell metacharacters that enable injection
    const FORBIDDEN: &[char] = &[
        ';', '|', '&', '`', '$', '(', ')', '{', '}', '>', '<', '\n', '\r',
    ];
    for ch in FORBIDDEN {
        if cmd.contains(*ch) {
            return Err(format!(
                "Wake command contains forbidden character '{}'. Use a script file instead.",
                ch.escape_default()
            ));
        }
    }
    // Require a path-like prefix (absolute or relative)
    if !cmd.starts_with('/') && !cmd.starts_with("./") && !cmd.starts_with("../") {
        return Err(
            "Wake command must start with '/', './', or '../' (use a script file path)".to_string(),
        );
    }
    Ok(())
}

/// Sanitize an environment variable value: strip control characters and cap length.
fn sanitize_env_value(val: &str, max_len: usize) -> String {
    val.chars()
        .filter(|c| !c.is_control())
        .take(max_len)
        .collect()
}

/// Validate a user-supplied name (project name, task title, friend name, etc.).
/// Rejects empty, control chars, and excessive length.
pub fn validate_name(name: &str, field: &str, max_len: usize) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{} cannot be empty", field));
    }
    if name.len() > max_len {
        return Err(format!("{} too long (max {} chars)", field, max_len));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(format!("{} contains control characters", field));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer(name: &str, session_id: Option<Uuid>) -> PeerInfo {
        PeerInfo {
            name: name.to_string(),
            address: format!(
                "127.0.0.1:{}",
                10000 + (uuid::Uuid::new_v4().as_u128() % 50000) as u16
            ),
            connected_at: chrono::Utc::now(),
            did: None,
            session_id,
            verified: false,
            owner_did: None,
            owner_verified: false,
            last_seen: Some(chrono::Utc::now()),
            disconnect: Arc::new(Notify::new()),
        }
    }

    #[tokio::test]
    async fn test_register_fresh_peer() {
        let dir = std::env::temp_dir().join(format!("agora-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let friends_path = dir.join("friends.json");
        let state = DaemonState::new("test-node", &friends_path, 7313);

        let session = Uuid::new_v4();
        let result = state.add_peer(make_peer("alice", Some(session))).await;
        assert_eq!(result, RegisterResult::Registered);
        assert_eq!(state.get_peers().await.len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_register_duplicate_same_session() {
        let dir = std::env::temp_dir().join(format!("agora-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let friends_path = dir.join("friends.json");
        let state = DaemonState::new("test-node", &friends_path, 7313);

        let session = Uuid::new_v4();
        let r1 = state.add_peer(make_peer("alice", Some(session))).await;
        assert_eq!(r1, RegisterResult::Registered);

        let r2 = state.add_peer(make_peer("alice", Some(session))).await;
        assert_eq!(r2, RegisterResult::Duplicate);
        // Should still have exactly 1 peer (the original)
        assert_eq!(state.get_peers().await.len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_register_replace_different_session() {
        let dir = std::env::temp_dir().join(format!("agora-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let friends_path = dir.join("friends.json");
        let state = DaemonState::new("test-node", &friends_path, 7313);

        let session1 = Uuid::new_v4();
        let session2 = Uuid::new_v4();

        let r1 = state.add_peer(make_peer("alice", Some(session1))).await;
        assert_eq!(r1, RegisterResult::Registered);

        let r2 = state.add_peer(make_peer("alice", Some(session2))).await;
        assert_eq!(r2, RegisterResult::Replaced);
        // Should still have exactly 1 peer (the new one replaced the old)
        let peers = state.get_peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].session_id, Some(session2));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_wake_status_tracks_readiness_and_listeners() {
        let dir = std::env::temp_dir().join(format!("agora-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let friends_path = dir.join("friends.json");
        let state = DaemonState::new("test-node", &friends_path, 7313);
        *state.inner.wake_command.lock().await = None;

        let initial = state.wake_status().await;
        assert!(!initial.enabled);
        assert!(!initial.armed);
        assert_eq!(initial.active_listener_count, 0);

        *state.inner.wake_command.lock().await = Some("./daemon/wake-agent.sh".to_string());
        let armed = state.wake_status().await;
        assert!(armed.enabled);
        assert!(armed.armed);
        assert_eq!(armed.active_listener_count, 0);

        let _listener = state.register_consumer("mcp-monitor").await;
        let after_consumer = state.wake_status().await;
        assert!(after_consumer.enabled);
        // Consumers no longer suppress wake (changed for shared daemon support)
        assert!(after_consumer.armed);
        assert_eq!(after_consumer.active_listener_count, 0);

        let _default = state.default_consumer_id().await;
        let after_default = state.wake_status().await;
        assert_eq!(after_default.active_listener_count, 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_listener_consumer_counts_as_active_listener() {
        let dir = std::env::temp_dir().join(format!("agora-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let friends_path = dir.join("friends.json");
        let state = DaemonState::new("test-node", &friends_path, 7313);
        *state.inner.wake_command.lock().await = Some("./daemon/wake-agent.sh".to_string());

        let _listener = state.register_listener_consumer("codex-listener").await;
        let wake = state.wake_status().await;
        assert!(wake.enabled);
        assert!(!wake.armed);
        assert_eq!(wake.active_listener_count, 1);
        assert_eq!(
            wake.active_listener_labels,
            vec!["codex-listener".to_string()]
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_touch_consumer_reactivates_listener_health() {
        let dir = std::env::temp_dir().join(format!("agora-state-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let friends_path = dir.join("friends.json");
        let state = DaemonState::new("test-node", &friends_path, 7313);
        *state.inner.wake_command.lock().await = Some("./daemon/wake-agent.sh".to_string());

        let listener = state.register_listener_consumer("codex-listener").await;
        {
            let mut consumers = state.inner.consumers.lock().await;
            let slot = consumers.get_mut(&listener).unwrap();
            slot.last_active = chrono::Utc::now() - chrono::Duration::seconds(120);
        }

        let stale = state.wake_status().await;
        assert_eq!(stale.active_listener_count, 0);
        assert!(stale.armed);

        assert!(state.touch_consumer(listener).await);

        let refreshed = state.wake_status().await;
        assert_eq!(refreshed.active_listener_count, 1);
        assert!(!refreshed.armed);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_validate_wake_command_valid() {
        assert!(validate_wake_command("/usr/bin/my-script").is_ok());
        assert!(validate_wake_command("./daemon/wake-agent.sh").is_ok());
        assert!(validate_wake_command("../scripts/wake.sh").is_ok());
    }

    #[test]
    fn test_validate_wake_command_injection() {
        assert!(validate_wake_command("echo hello; rm -rf /").is_err());
        assert!(validate_wake_command("script | cat").is_err());
        assert!(validate_wake_command("cmd && evil").is_err());
        assert!(validate_wake_command("$(whoami)").is_err());
        assert!(validate_wake_command("`whoami`").is_err());
        assert!(validate_wake_command("cmd > /etc/passwd").is_err());
    }

    #[test]
    fn test_validate_wake_command_no_path() {
        assert!(validate_wake_command("just-a-name").is_err());
        assert!(validate_wake_command("").is_err());
    }

    #[test]
    fn test_sanitize_env_value() {
        assert_eq!(sanitize_env_value("hello", 500), "hello");
        assert_eq!(sanitize_env_value("hello\nworld", 500), "helloworld");
        assert_eq!(sanitize_env_value("hello\x00evil", 500), "helloevil");
        assert_eq!(sanitize_env_value("a".repeat(600).as_str(), 500).len(), 500);
    }

    #[test]
    fn test_validate_name() {
        assert!(validate_name("Valid Name", "test", 200).is_ok());
        assert!(validate_name("", "test", 200).is_err());
        assert!(validate_name("   ", "test", 200).is_err());
        assert!(validate_name("has\x00null", "test", 200).is_err());
        let long = "a".repeat(201);
        assert!(validate_name(&long, "test", 200).is_err());
    }
}
