//! Offline message queue — stores messages for peers that are currently
//! disconnected and replays them upon reconnection.
//!
//! Messages are persisted per-peer in `~/.agora/outbox/{peer_name}.json`.
//! Delivery is confirmed via the `Ack` message type. Dedup by message UUID.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum queued messages per peer before oldest are dropped.
const MAX_QUEUE_PER_PEER: usize = 1000;

/// A message waiting to be delivered to an offline peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub id: Uuid,
    pub to: String,
    pub body: String,
    pub msg_type: Option<String>,
    pub enqueued_at: DateTime<Utc>,
    pub reply_to: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    #[serde(default)]
    pub delivered: bool,
}

/// Per-peer outbox queue with persistence and dedup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PeerQueue {
    pub messages: Vec<QueuedMessage>,
}

/// Stats for the outbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxStats {
    pub peers: Vec<PeerOutboxStats>,
    pub total_queued: usize,
    pub total_delivered: usize,
}

/// Per-peer outbox stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerOutboxStats {
    pub peer: String,
    pub queued: usize,
    pub delivered: usize,
}

/// Manages the persistent offline message queue.
pub struct OutboxStore {
    base_dir: PathBuf,
    /// Set of message IDs we've already seen (for dedup on receive).
    seen_ids: HashSet<Uuid>,
}

impl OutboxStore {
    /// Create a new OutboxStore at the given directory.
    pub fn new(base_dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(base_dir);
        Self {
            base_dir: base_dir.to_path_buf(),
            seen_ids: HashSet::new(),
        }
    }

    /// Default outbox directory: `~/.agora/outbox/`
    pub fn default_path() -> PathBuf {
        crate::config::agora_home().join("outbox")
    }

    fn peer_file(&self, peer: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", peer))
    }

    fn load_queue(&self, peer: &str) -> PeerQueue {
        let path = self.peer_file(peer);
        match std::fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => PeerQueue::default(),
        }
    }

    fn save_queue(&self, peer: &str, queue: &PeerQueue) {
        let path = self.peer_file(peer);
        if let Ok(data) = serde_json::to_string_pretty(queue) {
            let _ = std::fs::write(&path, data);
        }
    }

    /// Enqueue a message for an offline peer. Deduplicates by message ID.
    /// Returns true if the message was enqueued (not a duplicate).
    pub fn enqueue(&mut self, msg: QueuedMessage) -> bool {
        let mut queue = self.load_queue(&msg.to);

        // Dedup: skip if already queued
        if queue.messages.iter().any(|m| m.id == msg.id) {
            return false;
        }

        queue.messages.push(msg.clone());

        // Enforce max limit — drop oldest undelivered
        while queue.messages.iter().filter(|m| !m.delivered).count() > MAX_QUEUE_PER_PEER {
            if let Some(pos) = queue.messages.iter().position(|m| !m.delivered) {
                queue.messages.remove(pos);
            } else {
                break;
            }
        }

        self.save_queue(&msg.to, &queue);
        true
    }

    /// Mark a message as delivered (acked) for a peer.
    pub fn ack(&mut self, peer: &str, message_id: &Uuid) -> bool {
        let mut queue = self.load_queue(peer);
        let mut found = false;
        for msg in &mut queue.messages {
            if msg.id == *message_id {
                msg.delivered = true;
                found = true;
                break;
            }
        }
        if found {
            self.save_queue(peer, &queue);
        }
        found
    }

    /// Get all undelivered messages for a peer (for replay on reconnect).
    pub fn pending_for(&self, peer: &str) -> Vec<QueuedMessage> {
        let queue = self.load_queue(peer);
        queue
            .messages
            .into_iter()
            .filter(|m| !m.delivered)
            .collect()
    }

    /// Remove all delivered messages for a peer (garbage collection).
    pub fn gc_delivered(&mut self, peer: &str) {
        let mut queue = self.load_queue(peer);
        queue.messages.retain(|m| !m.delivered);
        self.save_queue(peer, &queue);
    }

    /// Get stats for all peers.
    pub fn stats(&self) -> OutboxStats {
        let mut peers = Vec::new();
        let mut total_queued = 0;
        let mut total_delivered = 0;

        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    let peer = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string();
                    let queue = self.load_queue(&peer);
                    let queued = queue.messages.iter().filter(|m| !m.delivered).count();
                    let delivered = queue.messages.iter().filter(|m| m.delivered).count();
                    total_queued += queued;
                    total_delivered += delivered;
                    peers.push(PeerOutboxStats {
                        peer,
                        queued,
                        delivered,
                    });
                }
            }
        }

        OutboxStats {
            peers,
            total_queued,
            total_delivered,
        }
    }

    /// Check if we've already seen this message ID (inbound dedup).
    pub fn is_seen(&self, id: &Uuid) -> bool {
        self.seen_ids.contains(id)
    }

    /// Mark a message ID as seen (inbound dedup).
    pub fn mark_seen(&mut self, id: Uuid) {
        self.seen_ids.insert(id);
        // Cap at 10000 to prevent unbounded growth
        if self.seen_ids.len() > 10000 {
            // Remove a random entry
            if let Some(&first) = self.seen_ids.iter().next() {
                self.seen_ids.remove(&first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_msg(to: &str) -> QueuedMessage {
        QueuedMessage {
            id: Uuid::new_v4(),
            to: to.to_string(),
            body: "hello".to_string(),
            msg_type: None,
            enqueued_at: Utc::now(),
            reply_to: None,
            conversation_id: None,
            delivered: false,
        }
    }

    #[test]
    fn test_enqueue_and_pending() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        let msg = make_msg("bob");
        assert!(store.enqueue(msg.clone()));
        let pending = store.pending_for("bob");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].body, "hello");
    }

    #[test]
    fn test_dedup() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        let msg = make_msg("bob");
        assert!(store.enqueue(msg.clone()));
        assert!(!store.enqueue(msg)); // duplicate
        assert_eq!(store.pending_for("bob").len(), 1);
    }

    #[test]
    fn test_ack() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        let msg = make_msg("bob");
        let id = msg.id;
        store.enqueue(msg);
        assert_eq!(store.pending_for("bob").len(), 1);
        assert!(store.ack("bob", &id));
        assert_eq!(store.pending_for("bob").len(), 0);
    }

    #[test]
    fn test_gc_delivered() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        let msg = make_msg("bob");
        let id = msg.id;
        store.enqueue(msg);
        store.ack("bob", &id);
        store.gc_delivered("bob");
        let queue = store.load_queue("bob");
        assert!(queue.messages.is_empty());
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();
        let msg = make_msg("alice");
        {
            let mut store = OutboxStore::new(dir.path());
            store.enqueue(msg);
        }
        // Reload from disk
        let store = OutboxStore::new(dir.path());
        assert_eq!(store.pending_for("alice").len(), 1);
    }

    #[test]
    fn test_max_limit() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        for _ in 0..1005 {
            store.enqueue(make_msg("bob"));
        }
        assert_eq!(store.pending_for("bob").len(), MAX_QUEUE_PER_PEER);
    }

    #[test]
    fn test_stats() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        let msg1 = make_msg("bob");
        let id1 = msg1.id;
        store.enqueue(msg1);
        store.enqueue(make_msg("bob"));
        store.enqueue(make_msg("alice"));
        store.ack("bob", &id1);

        let stats = store.stats();
        assert_eq!(stats.total_queued, 2);
        assert_eq!(stats.total_delivered, 1);
        assert_eq!(stats.peers.len(), 2);
    }

    #[test]
    fn test_seen_dedup() {
        let dir = TempDir::new().unwrap();
        let mut store = OutboxStore::new(dir.path());
        let id = Uuid::new_v4();
        assert!(!store.is_seen(&id));
        store.mark_seen(id);
        assert!(store.is_seen(&id));
    }
}
