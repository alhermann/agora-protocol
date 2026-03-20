//! Agent marketplace — capability-based discovery and advertisement.
//!
//! Agents advertise their capabilities (domains, tools, availability) and
//! other agents can search for matching peers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Agent availability status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentAvailability {
    Available,
    Busy,
    Offline,
}

impl Default for AgentAvailability {
    fn default() -> Self {
        Self::Available
    }
}

/// Capabilities advertised by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_did: Option<String>,
    /// Domains of expertise (e.g., "rust", "ml", "devops").
    #[serde(default)]
    pub domains: Vec<String>,
    /// Tools/services offered (e.g., "code-review", "testing").
    #[serde(default)]
    pub tools: Vec<String>,
    /// Current availability status.
    #[serde(default)]
    pub availability: AgentAvailability,
    /// Free-text description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When the capabilities were last updated.
    pub updated_at: DateTime<Utc>,
    /// Address where the agent can be reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

/// Search query for finding agents by capability.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSearchQuery {
    /// Filter by domains (any match).
    #[serde(default)]
    pub domains: Option<Vec<String>>,
    /// Filter by tools (any match).
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    /// Free-text search across name, description, domains, tools.
    #[serde(default)]
    pub query: Option<String>,
    /// Filter by availability.
    #[serde(default)]
    pub availability: Option<AgentAvailability>,
}

/// Search result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSearchResult {
    pub agent: AgentCapabilities,
    /// Relevance score (higher = better match).
    pub score: f64,
}

/// Persistent marketplace store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketplaceStore {
    pub agents: Vec<AgentCapabilities>,
}

impl MarketplaceStore {
    pub fn default_path() -> PathBuf {
        
        crate::config::agora_home().join("marketplace.json")
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
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Add or update an agent's capabilities. Returns true if updated (vs new).
    pub fn upsert(&mut self, caps: AgentCapabilities) -> bool {
        if let Some(existing) = self
            .agents
            .iter_mut()
            .find(|a| a.agent_name == caps.agent_name)
        {
            *existing = caps;
            true
        } else {
            self.agents.push(caps);
            false
        }
    }

    /// Remove an agent by name.
    pub fn remove(&mut self, agent_name: &str) -> bool {
        let before = self.agents.len();
        self.agents.retain(|a| a.agent_name != agent_name);
        self.agents.len() < before
    }

    /// Get an agent's capabilities by name.
    pub fn get(&self, agent_name: &str) -> Option<&AgentCapabilities> {
        self.agents.iter().find(|a| a.agent_name == agent_name)
    }

    /// Search for agents matching a query.
    pub fn search(&self, query: &AgentSearchQuery) -> Vec<AgentSearchResult> {
        let mut results: Vec<AgentSearchResult> = self
            .agents
            .iter()
            .filter_map(|agent| {
                let mut score: f64 = 0.0;
                let mut matched = false;

                // Filter by availability
                if let Some(ref avail) = query.availability {
                    if agent.availability != *avail {
                        return None;
                    }
                }

                // Score by domain matches
                if let Some(ref domains) = query.domains {
                    for domain in domains {
                        let d = domain.to_lowercase();
                        if agent.domains.iter().any(|ad| ad.to_lowercase() == d) {
                            score += 10.0;
                            matched = true;
                        }
                    }
                    if !matched && query.tools.is_none() && query.query.is_none() {
                        return None; // Domain filter specified but no match
                    }
                }

                // Score by tool matches
                if let Some(ref tools) = query.tools {
                    let mut tool_matched = false;
                    for tool in tools {
                        let t = tool.to_lowercase();
                        if agent.tools.iter().any(|at| at.to_lowercase() == t) {
                            score += 10.0;
                            tool_matched = true;
                            matched = true;
                        }
                    }
                    if !tool_matched && query.domains.is_none() && query.query.is_none() {
                        return None;
                    }
                }

                // Free-text search
                if let Some(ref q) = query.query {
                    let q_lower = q.to_lowercase();
                    let mut text_matched = false;

                    if agent.agent_name.to_lowercase().contains(&q_lower) {
                        score += 5.0;
                        text_matched = true;
                    }
                    if let Some(ref desc) = agent.description {
                        if desc.to_lowercase().contains(&q_lower) {
                            score += 3.0;
                            text_matched = true;
                        }
                    }
                    for d in &agent.domains {
                        if d.to_lowercase().contains(&q_lower) {
                            score += 4.0;
                            text_matched = true;
                        }
                    }
                    for t in &agent.tools {
                        if t.to_lowercase().contains(&q_lower) {
                            score += 4.0;
                            text_matched = true;
                        }
                    }

                    if text_matched {
                        matched = true;
                    }
                }

                // If no domain/tool/query filters specified, return all that pass availability
                if query.domains.is_none() && query.tools.is_none() && query.query.is_none() {
                    matched = true;
                    if score == 0.0 {
                        score = 1.0;
                    }
                }

                if matched {
                    Some(AgentSearchResult {
                        agent: agent.clone(),
                        score,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// List all known agents.
    pub fn list(&self) -> &[AgentCapabilities] {
        &self.agents
    }

    /// Remove stale entries (not updated in the given duration).
    pub fn prune_stale(&mut self, max_age: chrono::Duration) {
        let cutoff = Utc::now() - max_age;
        self.agents.retain(|a| a.updated_at > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_agent(name: &str, domains: &[&str], tools: &[&str]) -> AgentCapabilities {
        AgentCapabilities {
            agent_name: name.to_string(),
            agent_did: None,
            domains: domains.iter().map(|s| s.to_string()).collect(),
            tools: tools.iter().map(|s| s.to_string()).collect(),
            availability: AgentAvailability::Available,
            description: Some(format!("{} agent", name)),
            updated_at: Utc::now(),
            address: None,
        }
    }

    #[test]
    fn test_upsert_and_get() {
        let mut store = MarketplaceStore::default();
        let agent = make_agent("alice", &["rust"], &["code-review"]);
        assert!(!store.upsert(agent.clone()));
        assert!(store.get("alice").is_some());
        assert!(store.upsert(make_agent("alice", &["rust", "python"], &["testing"])));
        assert_eq!(store.get("alice").unwrap().domains.len(), 2);
    }

    #[test]
    fn test_remove() {
        let mut store = MarketplaceStore::default();
        store.upsert(make_agent("alice", &["rust"], &[]));
        assert!(store.remove("alice"));
        assert!(!store.remove("alice"));
        assert!(store.get("alice").is_none());
    }

    #[test]
    fn test_search_by_domain() {
        let mut store = MarketplaceStore::default();
        store.upsert(make_agent("alice", &["rust", "python"], &[]));
        store.upsert(make_agent("bob", &["javascript"], &[]));

        let results = store.search(&AgentSearchQuery {
            domains: Some(vec!["rust".to_string()]),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent.agent_name, "alice");
    }

    #[test]
    fn test_search_by_tool() {
        let mut store = MarketplaceStore::default();
        store.upsert(make_agent("alice", &[], &["code-review", "testing"]));
        store.upsert(make_agent("bob", &[], &["deployment"]));

        let results = store.search(&AgentSearchQuery {
            tools: Some(vec!["testing".to_string()]),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent.agent_name, "alice");
    }

    #[test]
    fn test_search_free_text() {
        let mut store = MarketplaceStore::default();
        store.upsert(make_agent("alice", &["rust"], &["code-review"]));
        store.upsert(make_agent("bob", &["python"], &["ml-training"]));

        let results = store.search(&AgentSearchQuery {
            query: Some("rust".to_string()),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent.agent_name, "alice");
    }

    #[test]
    fn test_search_all() {
        let mut store = MarketplaceStore::default();
        store.upsert(make_agent("alice", &["rust"], &[]));
        store.upsert(make_agent("bob", &["python"], &[]));

        let results = store.search(&AgentSearchQuery::default());
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_availability() {
        let mut store = MarketplaceStore::default();
        let mut alice = make_agent("alice", &["rust"], &[]);
        alice.availability = AgentAvailability::Busy;
        store.upsert(alice);
        store.upsert(make_agent("bob", &["rust"], &[]));

        let results = store.search(&AgentSearchQuery {
            availability: Some(AgentAvailability::Available),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent.agent_name, "bob");
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("marketplace.json");
        let mut store = MarketplaceStore::default();
        store.upsert(make_agent("alice", &["rust"], &["testing"]));
        store.save(&path).unwrap();

        let loaded = MarketplaceStore::load(&path);
        assert_eq!(loaded.agents.len(), 1);
        assert_eq!(loaded.agents[0].agent_name, "alice");
    }

    #[test]
    fn test_serialization() {
        let agent = make_agent("alice", &["rust", "python"], &["code-review"]);
        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: AgentCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.agent_name, "alice");
        assert_eq!(deserialized.domains.len(), 2);
    }
}
