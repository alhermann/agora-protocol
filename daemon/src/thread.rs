//! Thread management — threads double as sub-groups.
//!
//! A thread is a conversation with an explicit participant list, optional
//! trust floor, and open/closed status. Threads are the collaboration
//! primitive in Agora — they organize multi-agent work without the
//! complexity of nested groups.
//!
//! See `protocol/threads.md` for the wire format spec.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Peer identifier — plain String for now, will become `did:agora:...` later.
pub type PeerId = String;

/// A conversation thread / sub-group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Unique thread identifier (also the conversation_id for messages).
    pub id: Uuid,
    /// Human-readable title.
    pub title: Option<String>,
    /// Creator of the thread.
    pub creator: PeerId,
    /// Current participant list.
    pub participants: HashSet<PeerId>,
    /// Minimum trust level to participate.
    pub min_trust: u8,
    /// If true, participant list is fixed — no invites allowed.
    pub closed: bool,
    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
    /// When the thread was created.
    pub created_at: DateTime<Utc>,
    /// If set, thread is closed and accepts no new messages.
    pub closed_at: Option<DateTime<Utc>>,
    /// Reason for closing, if any.
    pub close_reason: Option<String>,
}

/// Summary of a thread for list endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadSummary {
    pub id: String,
    pub title: Option<String>,
    pub creator: String,
    pub participant_count: usize,
    pub participants: Vec<String>,
    pub min_trust: u8,
    pub closed: bool,
    pub created_at: String,
    pub is_active: bool,
}

/// Errors from thread operations.
#[derive(Debug, Serialize)]
pub enum ThreadError {
    NotFound,
    NotAMember,
    NotAuthorized,
    ThreadClosed,
    InsufficientTrust,
    AlreadyExists,
}

impl std::fmt::Display for ThreadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "thread not found"),
            Self::NotAMember => write!(f, "not a member of this thread"),
            Self::NotAuthorized => write!(f, "not authorized"),
            Self::ThreadClosed => write!(f, "thread is closed"),
            Self::InsufficientTrust => write!(f, "insufficient trust level"),
            Self::AlreadyExists => write!(f, "thread already exists"),
        }
    }
}

/// Manages all active threads.
#[derive(Debug, Serialize, Deserialize)]
pub struct ThreadManager {
    threads: HashMap<Uuid, Thread>,
}

impl ThreadManager {
    pub fn default_path() -> PathBuf {
        crate::config::agora_home().join("threads.json")
    }

    pub fn new() -> Self {
        Self {
            threads: HashMap::new(),
        }
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_else(|_| Self::new()),
            Err(_) => Self::new(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Create a new thread. Returns the thread ID.
    pub fn create(
        &mut self,
        id: Option<Uuid>,
        creator: &str,
        title: Option<String>,
        participants: Vec<String>,
        min_trust: u8,
        closed: bool,
        metadata: HashMap<String, String>,
    ) -> Result<Uuid, ThreadError> {
        let id = id.unwrap_or_else(Uuid::new_v4);

        if self.threads.contains_key(&id) {
            return Err(ThreadError::AlreadyExists);
        }

        let mut member_set: HashSet<PeerId> = participants.into_iter().collect();
        // Creator is always a participant
        member_set.insert(creator.to_string());

        let thread = Thread {
            id,
            title,
            creator: creator.to_string(),
            participants: member_set,
            min_trust,
            closed,
            metadata,
            created_at: Utc::now(),
            closed_at: None,
            close_reason: None,
        };

        self.threads.insert(id, thread);
        Ok(id)
    }

    /// Get a thread by ID.
    pub fn get(&self, id: &Uuid) -> Option<&Thread> {
        self.threads.get(id)
    }

    /// Check if a peer is a member of a thread.
    pub fn is_member(&self, thread_id: &Uuid, peer: &str) -> bool {
        self.threads
            .get(thread_id)
            .is_some_and(|t| t.participants.contains(peer))
    }

    /// Add a participant to an open thread.
    pub fn add_participant(
        &mut self,
        thread_id: &Uuid,
        inviter: &str,
        invitee: &str,
        invitee_trust: u8,
    ) -> Result<(), ThreadError> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or(ThreadError::NotFound)?;

        if thread.closed_at.is_some() {
            return Err(ThreadError::ThreadClosed);
        }
        if thread.closed {
            return Err(ThreadError::ThreadClosed);
        }
        if !thread.participants.contains(inviter) {
            return Err(ThreadError::NotAMember);
        }
        if invitee_trust < thread.min_trust {
            return Err(ThreadError::InsufficientTrust);
        }

        thread.participants.insert(invitee.to_string());
        Ok(())
    }

    /// Remove a participant (leave or kick).
    pub fn remove_participant(
        &mut self,
        thread_id: &Uuid,
        remover: &str,
        target: &str,
    ) -> Result<(), ThreadError> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or(ThreadError::NotFound)?;

        // Anyone can leave (remove themselves)
        // Only creator can kick others
        if remover != target && remover != thread.creator {
            return Err(ThreadError::NotAuthorized);
        }
        if !thread.participants.contains(target) {
            return Err(ThreadError::NotAMember);
        }

        thread.participants.remove(target);
        Ok(())
    }

    /// Close a thread — no more messages or participant changes.
    pub fn close_thread(
        &mut self,
        thread_id: &Uuid,
        closer: &str,
        reason: Option<String>,
    ) -> Result<(), ThreadError> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or(ThreadError::NotFound)?;

        // Only creator can close
        if closer != thread.creator {
            return Err(ThreadError::NotAuthorized);
        }

        thread.closed_at = Some(Utc::now());
        thread.close_reason = reason;
        Ok(())
    }

    /// Update thread metadata or title.
    pub fn update(
        &mut self,
        thread_id: &Uuid,
        updater: &str,
        title: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<(), ThreadError> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or(ThreadError::NotFound)?;

        if !thread.participants.contains(updater) {
            return Err(ThreadError::NotAMember);
        }

        if let Some(t) = title {
            thread.title = Some(t);
        }
        if let Some(m) = metadata {
            thread.metadata.extend(m);
        }
        Ok(())
    }

    /// Get participants who should receive a message for this thread.
    /// Returns None if thread not found.
    pub fn route(&self, thread_id: &Uuid, sender: &str) -> Result<Vec<PeerId>, ThreadError> {
        let thread = self.threads.get(thread_id).ok_or(ThreadError::NotFound)?;

        if thread.closed_at.is_some() {
            return Err(ThreadError::ThreadClosed);
        }
        if !thread.participants.contains(sender) {
            return Err(ThreadError::NotAMember);
        }

        // Return all participants except sender
        Ok(thread
            .participants
            .iter()
            .filter(|p| p.as_str() != sender)
            .cloned()
            .collect())
    }

    /// List all threads, optionally filtered by participant.
    pub fn list(&self, participant: Option<&str>) -> Vec<ThreadSummary> {
        let mut threads: Vec<ThreadSummary> = self
            .threads
            .values()
            .filter(|t| participant.is_none_or(|p| t.participants.contains(p)))
            .map(|t| {
                let mut participants: Vec<String> = t.participants.iter().cloned().collect();
                participants.sort();
                ThreadSummary {
                    id: t.id.to_string(),
                    title: t.title.clone(),
                    creator: t.creator.clone(),
                    participant_count: t.participants.len(),
                    participants,
                    min_trust: t.min_trust,
                    closed: t.closed,
                    created_at: t.created_at.to_rfc3339(),
                    is_active: t.closed_at.is_none(),
                }
            })
            .collect();

        threads.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        threads
    }

    /// Number of active threads.
    pub fn active_count(&self) -> usize {
        self.threads
            .values()
            .filter(|t| t.closed_at.is_none())
            .count()
    }
}
