//! Decentralized agent discovery via gossip.
//!
//! When peers connect, they exchange signed capability entries and
//! friend-of-friend introductions. Every claim is cryptographically
//! verifiable via Ed25519 signatures tied to W3C DIDs.
//!
//! Trust is transitive but decays: effective_trust = our_trust(introducer)
//! * (introducer_trust / 4.0) * 0.75^hops. Capped at level 2 (Friend).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::marketplace::AgentCapabilities;

/// Maximum hops a gossip entry can travel before being dropped.
pub const MAX_GOSSIP_HOPS: u8 = 3;

/// Decay factor per hop for transitive trust.
const HOP_DECAY: f64 = 0.75;

/// Maximum effective trust from gossip (never auto-grant Trusted/InnerCircle).
const MAX_GOSSIP_TRUST: f64 = 2.0;

/// Maximum age for discovery entries before pruning (7 days).
pub const MAX_DISCOVERY_AGE_SECS: i64 = 7 * 24 * 3600;

// ---------------------------------------------------------------------------
// Signed capabilities (cryptographically verifiable)
// ---------------------------------------------------------------------------

/// Agent capabilities signed by the originating agent's Ed25519 key.
/// Can be relayed through multiple hops while remaining verifiable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCapabilities {
    pub capabilities: AgentCapabilities,
    /// DID of the agent (did:agora:...)
    pub did: String,
    /// Base58 public key for signature verification
    pub public_key: String,
    /// Ed25519 signature over canonical JSON of capabilities
    pub signature: String,
    /// Number of gossip hops from the originator (0 = direct)
    pub hop_count: u8,
}

impl SignedCapabilities {
    /// Create a signed capabilities entry from our own identity.
    pub fn sign(caps: &AgentCapabilities, identity: &crate::identity::AgentIdentity) -> Self {
        let canonical = serde_json::to_string(caps).expect("serialize capabilities");
        let sig = identity.sign(canonical.as_bytes());
        Self {
            capabilities: caps.clone(),
            did: identity.did().to_string(),
            public_key: identity.public_key_base58(),
            signature: bs58::encode(&sig).into_string(),
            hop_count: 0,
        }
    }

    /// Verify the signature matches the capabilities content.
    pub fn verify(&self) -> bool {
        let canonical = match serde_json::to_string(&self.capabilities) {
            Ok(c) => c,
            Err(_) => return false,
        };
        let sig_bytes = match bs58::decode(&self.signature).into_vec() {
            Ok(b) => b,
            Err(_) => return false,
        };
        let pk_bytes = match bs58::decode(&self.public_key).into_vec() {
            Ok(b) => b,
            Err(_) => return false,
        };
        if pk_bytes.len() != 32 || sig_bytes.len() != 64 {
            return false;
        }
        use ring::signature;
        let pk = signature::UnparsedPublicKey::new(&signature::ED25519, &pk_bytes);
        pk.verify(canonical.as_bytes(), &sig_bytes).is_ok()
    }

    /// Create a relayed copy with incremented hop count.
    pub fn relay(&self) -> Option<Self> {
        if self.hop_count >= MAX_GOSSIP_HOPS {
            return None;
        }
        let mut relayed = self.clone();
        relayed.hop_count += 1;
        Some(relayed)
    }
}

// ---------------------------------------------------------------------------
// Discovery path (how we found an agent)
// ---------------------------------------------------------------------------

/// How an agent was discovered on the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum DiscoveryPath {
    /// Direct peer connection
    Direct,
    /// Friend-of-friend introduction
    Introduction {
        introducer_did: String,
        introducer_name: String,
        introducer_trust: u8,
    },
    /// Multi-hop gossip relay
    Gossip { hop_count: u8, relay_name: String },
}

// ---------------------------------------------------------------------------
// Discovered agent
// ---------------------------------------------------------------------------

/// An agent discovered through the gossip network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredAgent {
    pub did: String,
    pub name: String,
    pub capabilities: Option<AgentCapabilities>,
    pub discovery_path: DiscoveryPath,
    /// Computed transitive trust score (0.0 - 2.0)
    pub effective_trust: f64,
    /// Signed capabilities for verification
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_capabilities: Option<SignedCapabilities>,
    pub first_seen: DateTime<Utc>,
    pub last_refreshed: DateTime<Utc>,
    /// Last known network address
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_address: Option<String>,
    /// Owner DID (if the agent has an owner attestation)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_did: Option<String>,
}

// ---------------------------------------------------------------------------
// Project advertisement
// ---------------------------------------------------------------------------

/// A project advertising open roles to the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAd {
    pub project_id: Uuid,
    pub project_name: String,
    pub description: Option<String>,
    pub repo: Option<String>,
    pub owner_did: String,
    pub owner_name: String,
    pub open_roles: Vec<OpenRoleAd>,
    pub signature: String,
    pub created_at: DateTime<Utc>,
}

/// An open role in a project advertisement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRoleAd {
    pub role: String,
    #[serde(default)]
    pub desired_domains: Vec<String>,
    #[serde(default)]
    pub desired_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Introduction payload
// ---------------------------------------------------------------------------

/// A friend-of-friend introduction sent via gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Introduction {
    pub agent_did: String,
    pub agent_name: String,
    pub signed_capabilities: Option<SignedCapabilities>,
    /// Trust level the introducer assigns to this agent
    pub introducer_trust: u8,
    pub introducer_did: String,
    pub last_address: Option<String>,
    pub owner_did: Option<String>,
}

// ---------------------------------------------------------------------------
// Transitive trust computation
// ---------------------------------------------------------------------------

/// Compute effective trust for a transitively discovered agent.
///
/// Formula: effective_trust = our_trust(introducer) * (introducer_trust / 4.0) * decay^hops
///
/// Example: We trust Bob at 3. Bob trusts Carol at 3.
///   effective = 3 * (3/4) * 0.75^1 = 1.6875 → Acquaintance
///
/// Rules:
///   - Max effective trust = 2.0 (Friend). Never auto-grant Trusted via gossip.
///   - Max hops = 3. Beyond that, 0.
///   - Decay = 0.75 per hop.
pub fn compute_transitive_trust(
    our_trust_of_introducer: u8,
    introducer_trust_of_target: u8,
    hop_count: u8,
) -> f64 {
    if hop_count > MAX_GOSSIP_HOPS {
        return 0.0;
    }
    if our_trust_of_introducer == 0 || introducer_trust_of_target == 0 {
        return 0.0;
    }
    let base = (our_trust_of_introducer as f64) * (introducer_trust_of_target as f64 / 4.0);
    let decayed = base * HOP_DECAY.powi(hop_count as i32);
    decayed.min(MAX_GOSSIP_TRUST)
}

/// Convert effective trust score to a discrete trust level.
pub fn trust_score_to_level(score: f64) -> u8 {
    if score >= 2.0 {
        2 // Friend
    } else if score >= 1.0 {
        1 // Acquaintance
    } else {
        0 // Unknown
    }
}

/// Human-readable trust level name.
pub fn trust_level_name(level: u8) -> &'static str {
    match level {
        0 => "Unknown",
        1 => "Acquaintance",
        2 => "Friend",
        3 => "Trusted",
        4 => "Inner Circle",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// Discovery store (persistence)
// ---------------------------------------------------------------------------

/// Persistent store for gossip-discovered agents and project ads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscoveryStore {
    /// Discovered agents, keyed by DID
    pub agents: HashMap<String, DiscoveredAgent>,
    /// Project advertisements, keyed by project_id
    pub project_ads: HashMap<Uuid, ProjectAd>,
}

impl DiscoveryStore {
    pub fn default_path() -> PathBuf {
        
        crate::config::agora_home().join("discovery.json")
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Upsert a discovered agent. Returns true if updated (vs new).
    pub fn upsert_agent(&mut self, agent: DiscoveredAgent) -> bool {
        let did = agent.did.clone();
        if let Some(existing) = self.agents.get_mut(&did) {
            // Keep the higher trust score
            if agent.effective_trust > existing.effective_trust {
                *existing = agent;
            } else {
                existing.last_refreshed = chrono::Utc::now();
            }
            true
        } else {
            self.agents.insert(did, agent);
            false
        }
    }

    /// Upsert a project advertisement.
    pub fn upsert_project_ad(&mut self, ad: ProjectAd) {
        self.project_ads.insert(ad.project_id, ad);
    }

    /// Remove stale entries older than max_age_secs.
    pub fn prune(&mut self, max_age_secs: i64) {
        let cutoff = Utc::now() - chrono::Duration::seconds(max_age_secs);
        self.agents.retain(|_, a| a.last_refreshed > cutoff);
        self.project_ads.retain(|_, a| a.created_at > cutoff);
    }

    /// List all discovered agents sorted by effective trust (highest first).
    pub fn list_agents(&self) -> Vec<&DiscoveredAgent> {
        let mut agents: Vec<_> = self.agents.values().collect();
        agents.sort_by(|a, b| {
            b.effective_trust
                .partial_cmp(&a.effective_trust)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        agents
    }

    /// Search discovered agents by capability query.
    pub fn search_agents(&self, query: &str) -> Vec<&DiscoveredAgent> {
        let q = query.to_lowercase();
        let mut results: Vec<_> = self
            .agents
            .values()
            .filter(|a| {
                a.name.to_lowercase().contains(&q)
                    || a.capabilities
                        .as_ref()
                        .map(|c| {
                            c.domains.iter().any(|d| d.to_lowercase().contains(&q))
                                || c.tools.iter().any(|t| t.to_lowercase().contains(&q))
                                || c.description
                                    .as_ref()
                                    .map(|d| d.to_lowercase().contains(&q))
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false)
            })
            .collect();
        results.sort_by(|a, b| {
            b.effective_trust
                .partial_cmp(&a.effective_trust)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Get a discovered agent by DID.
    pub fn get_agent(&self, did: &str) -> Option<&DiscoveredAgent> {
        self.agents.get(did)
    }

    /// List project advertisements.
    pub fn list_project_ads(&self) -> Vec<&ProjectAd> {
        self.project_ads.values().collect()
    }

    /// Stats for the discovery network.
    pub fn stats(&self) -> DiscoveryStats {
        let mut direct = 0;
        let mut introductions = 0;
        let mut gossip = 0;
        let mut trust_sum = 0.0;

        for agent in self.agents.values() {
            match &agent.discovery_path {
                DiscoveryPath::Direct => direct += 1,
                DiscoveryPath::Introduction { .. } => introductions += 1,
                DiscoveryPath::Gossip { .. } => gossip += 1,
            }
            trust_sum += agent.effective_trust;
        }

        let total = self.agents.len();
        DiscoveryStats {
            total_discovered: total,
            direct_connections: direct,
            introductions,
            gossip_relayed: gossip,
            project_ads: self.project_ads.len(),
            avg_effective_trust: if total > 0 {
                trust_sum / total as f64
            } else {
                0.0
            },
            network_reach: total,
        }
    }
}

/// Network discovery statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryStats {
    pub total_discovered: usize,
    pub direct_connections: usize,
    pub introductions: usize,
    pub gossip_relayed: usize,
    pub project_ads: usize,
    pub avg_effective_trust: f64,
    pub network_reach: usize,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transitive_trust_direct_friend() {
        // We trust introducer at 3, they trust target at 3, 1 hop
        let score = compute_transitive_trust(3, 3, 1);
        assert!(score > 1.5 && score < 2.0, "score={}", score);
    }

    #[test]
    fn test_transitive_trust_capped_at_2() {
        // Even with max trust both sides, capped at 2.0
        let score = compute_transitive_trust(4, 4, 0);
        assert_eq!(score, 2.0);
    }

    #[test]
    fn test_transitive_trust_zero_if_untrusted() {
        assert_eq!(compute_transitive_trust(0, 4, 1), 0.0);
        assert_eq!(compute_transitive_trust(4, 0, 1), 0.0);
    }

    #[test]
    fn test_transitive_trust_decays_with_hops() {
        let h1 = compute_transitive_trust(3, 3, 1);
        let h2 = compute_transitive_trust(3, 3, 2);
        let h3 = compute_transitive_trust(3, 3, 3);
        assert!(h1 > h2, "h1={} h2={}", h1, h2);
        assert!(h2 > h3, "h2={} h3={}", h2, h3);
    }

    #[test]
    fn test_transitive_trust_zero_beyond_max_hops() {
        assert_eq!(compute_transitive_trust(4, 4, 4), 0.0);
    }

    #[test]
    fn test_trust_score_to_level() {
        assert_eq!(trust_score_to_level(2.0), 2);
        assert_eq!(trust_score_to_level(1.5), 1);
        assert_eq!(trust_score_to_level(0.5), 0);
    }

    #[test]
    fn test_discovery_store_upsert() {
        let mut store = DiscoveryStore::default();
        let agent = DiscoveredAgent {
            did: "did:agora:test123".into(),
            name: "alice".into(),
            capabilities: None,
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 2.0,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        };
        assert!(!store.upsert_agent(agent.clone()));
        assert!(store.upsert_agent(agent));
        assert_eq!(store.agents.len(), 1);
    }

    #[test]
    fn test_discovery_store_search() {
        let mut store = DiscoveryStore::default();
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:a".into(),
            name: "rust-dev".into(),
            capabilities: Some(AgentCapabilities {
                agent_name: "rust-dev".into(),
                agent_did: None,
                domains: vec!["rust".into()],
                tools: vec!["code-review".into()],
                availability: crate::marketplace::AgentAvailability::Available,
                description: None,
                updated_at: Utc::now(),
                address: None,
            }),
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 2.0,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        });
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:b".into(),
            name: "python-dev".into(),
            capabilities: Some(AgentCapabilities {
                agent_name: "python-dev".into(),
                agent_did: None,
                domains: vec!["python".into()],
                tools: vec![],
                availability: crate::marketplace::AgentAvailability::Available,
                description: None,
                updated_at: Utc::now(),
                address: None,
            }),
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 1.0,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        });

        let results = store.search_agents("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "rust-dev");

        let all = store.list_agents();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "rust-dev"); // higher trust first
    }

    #[test]
    fn test_discovery_store_prune() {
        let mut store = DiscoveryStore::default();
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:old".into(),
            name: "stale".into(),
            capabilities: None,
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 1.0,
            signed_capabilities: None,
            first_seen: Utc::now() - chrono::Duration::days(10),
            last_refreshed: Utc::now() - chrono::Duration::days(10),
            last_address: None,
            owner_did: None,
        });
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:new".into(),
            name: "fresh".into(),
            capabilities: None,
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 1.0,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        });

        store.prune(MAX_DISCOVERY_AGE_SECS);
        assert_eq!(store.agents.len(), 1);
        assert!(store.agents.contains_key("did:agora:new"));
    }

    #[test]
    fn test_discovery_stats() {
        let mut store = DiscoveryStore::default();
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:a".into(),
            name: "a".into(),
            capabilities: None,
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 2.0,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        });
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:b".into(),
            name: "b".into(),
            capabilities: None,
            discovery_path: DiscoveryPath::Introduction {
                introducer_did: "did:agora:a".into(),
                introducer_name: "a".into(),
                introducer_trust: 3,
            },
            effective_trust: 1.5,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        });

        let stats = store.stats();
        assert_eq!(stats.total_discovered, 2);
        assert_eq!(stats.direct_connections, 1);
        assert_eq!(stats.introductions, 1);
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("discovery.json");

        let mut store = DiscoveryStore::default();
        store.upsert_agent(DiscoveredAgent {
            did: "did:agora:persist".into(),
            name: "persist".into(),
            capabilities: None,
            discovery_path: DiscoveryPath::Direct,
            effective_trust: 2.0,
            signed_capabilities: None,
            first_seen: Utc::now(),
            last_refreshed: Utc::now(),
            last_address: None,
            owner_did: None,
        });
        store.save(&path).unwrap();

        let loaded = DiscoveryStore::load(&path);
        assert_eq!(loaded.agents.len(), 1);
        assert!(loaded.agents.contains_key("did:agora:persist"));
    }
}
