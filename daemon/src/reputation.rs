//! Reputation system — tracks agent contributions and computes
//! reputation scores with exponential decay.
//!
//! Score = sum(quality * weight * 0.95^weeks_old)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Types of contributions that affect reputation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ContributionType {
    TaskCompleted,
    StageAdvanced,
    ReviewApproved,
    ProjectCompleted,
}

impl ContributionType {
    /// Base weight for scoring.
    pub fn weight(&self) -> f64 {
        match self {
            ContributionType::TaskCompleted => 1.0,
            ContributionType::StageAdvanced => 2.0,
            ContributionType::ReviewApproved => 1.5,
            ContributionType::ProjectCompleted => 5.0,
        }
    }
}

/// A recorded contribution from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    pub id: Uuid,
    pub agent_name: String,
    pub contribution_type: ContributionType,
    pub project_id: Option<Uuid>,
    /// Quality multiplier (0.0 - 2.0, default 1.0).
    #[serde(default = "default_quality")]
    pub quality: f64,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_quality() -> f64 {
    1.0
}

/// Aggregated reputation for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReputation {
    pub agent_name: String,
    pub score: f64,
    pub contribution_count: usize,
    pub contributions_by_type: std::collections::HashMap<String, usize>,
    pub recommended_trust: Option<u8>,
}

/// Trust recommendation based on score thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRecommendation {
    pub agent_name: String,
    pub current_trust: u8,
    pub recommended_trust: u8,
    pub score: f64,
    pub reason: String,
}

/// Persistent reputation store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReputationStore {
    pub contributions: Vec<Contribution>,
}

/// Decay factor per week.
const DECAY_RATE: f64 = 0.95;

impl ReputationStore {
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".agora").join("reputation.json")
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

    /// Record a contribution.
    pub fn record(&mut self, contribution: Contribution) {
        self.contributions.push(contribution);
    }

    /// Compute the reputation score for an agent.
    pub fn score(&self, agent_name: &str) -> f64 {
        let now = Utc::now();
        self.contributions
            .iter()
            .filter(|c| c.agent_name == agent_name)
            .map(|c| {
                let weeks = (now - c.timestamp).num_days() as f64 / 7.0;
                let decay = DECAY_RATE.powf(weeks.max(0.0));
                c.quality * c.contribution_type.weight() * decay
            })
            .sum()
    }

    /// Get full reputation info for an agent.
    pub fn reputation(&self, agent_name: &str) -> AgentReputation {
        let score = self.score(agent_name);
        let agent_contributions: Vec<&Contribution> = self
            .contributions
            .iter()
            .filter(|c| c.agent_name == agent_name)
            .collect();

        let mut by_type = std::collections::HashMap::new();
        for c in &agent_contributions {
            let key = serde_json::to_string(&c.contribution_type)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            *by_type.entry(key).or_insert(0usize) += 1;
        }

        let recommended = Self::recommend_trust(score);

        AgentReputation {
            agent_name: agent_name.to_string(),
            score,
            contribution_count: agent_contributions.len(),
            contributions_by_type: by_type,
            recommended_trust: recommended,
        }
    }

    /// Recommend a trust level based on score.
    fn recommend_trust(score: f64) -> Option<u8> {
        if score >= 80.0 {
            Some(4)
        } else if score >= 50.0 {
            Some(3)
        } else if score >= 20.0 {
            Some(2)
        } else {
            None
        }
    }

    /// Get trust recommendations for agents whose score suggests upgrading.
    pub fn recommendations(&self, current_trusts: &[(String, u8)]) -> Vec<TrustRecommendation> {
        let mut recs = Vec::new();
        for (name, current) in current_trusts {
            let score = self.score(name);
            if let Some(recommended) = Self::recommend_trust(score) {
                if recommended > *current {
                    recs.push(TrustRecommendation {
                        agent_name: name.clone(),
                        current_trust: *current,
                        recommended_trust: recommended,
                        score,
                        reason: format!(
                            "Score {:.1} suggests trust {} (currently {})",
                            score, recommended, current
                        ),
                    });
                }
            }
        }
        recs
    }

    /// Get the leaderboard sorted by score.
    pub fn leaderboard(&self) -> Vec<AgentReputation> {
        let mut names: Vec<String> = self
            .contributions
            .iter()
            .map(|c| c.agent_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        names.sort();

        let mut board: Vec<AgentReputation> =
            names.iter().map(|name| self.reputation(name)).collect();

        board.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        board
    }

    /// Get contributions for a specific agent.
    pub fn agent_contributions(&self, agent_name: &str) -> Vec<&Contribution> {
        self.contributions
            .iter()
            .filter(|c| c.agent_name == agent_name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_contribution(agent: &str, ctype: ContributionType) -> Contribution {
        Contribution {
            id: Uuid::new_v4(),
            agent_name: agent.to_string(),
            contribution_type: ctype,
            project_id: None,
            quality: 1.0,
            timestamp: Utc::now(),
            description: None,
        }
    }

    fn make_old_contribution(agent: &str, ctype: ContributionType, weeks_ago: i64) -> Contribution {
        Contribution {
            id: Uuid::new_v4(),
            agent_name: agent.to_string(),
            contribution_type: ctype,
            project_id: None,
            quality: 1.0,
            timestamp: Utc::now() - chrono::Duration::weeks(weeks_ago),
            description: None,
        }
    }

    #[test]
    fn test_score_basic() {
        let mut store = ReputationStore::default();
        store.record(make_contribution("alice", ContributionType::TaskCompleted));
        let score = store.score("alice");
        assert!(score > 0.9 && score <= 1.0);
    }

    #[test]
    fn test_score_with_decay() {
        let mut store = ReputationStore::default();
        store.record(make_old_contribution(
            "alice",
            ContributionType::TaskCompleted,
            10,
        ));
        let score = store.score("alice");
        let expected = 1.0 * DECAY_RATE.powf(10.0);
        assert!((score - expected).abs() < 0.01);
    }

    #[test]
    fn test_score_weighted() {
        let mut store = ReputationStore::default();
        store.record(make_contribution(
            "alice",
            ContributionType::ProjectCompleted,
        ));
        let score = store.score("alice");
        assert!(score > 4.9 && score <= 5.0);
    }

    #[test]
    fn test_trust_recommendations() {
        let mut store = ReputationStore::default();
        // Add enough contributions for score >= 20
        for _ in 0..25 {
            store.record(make_contribution("alice", ContributionType::TaskCompleted));
        }
        let recs = store.recommendations(&[("alice".to_string(), 1)]);
        assert!(!recs.is_empty());
        assert!(recs[0].recommended_trust >= 2);
    }

    #[test]
    fn test_leaderboard() {
        let mut store = ReputationStore::default();
        for _ in 0..5 {
            store.record(make_contribution("alice", ContributionType::TaskCompleted));
        }
        for _ in 0..10 {
            store.record(make_contribution("bob", ContributionType::TaskCompleted));
        }
        let board = store.leaderboard();
        assert_eq!(board.len(), 2);
        assert_eq!(board[0].agent_name, "bob");
    }

    #[test]
    fn test_reputation_by_type() {
        let mut store = ReputationStore::default();
        store.record(make_contribution("alice", ContributionType::TaskCompleted));
        store.record(make_contribution("alice", ContributionType::TaskCompleted));
        store.record(make_contribution("alice", ContributionType::ReviewApproved));
        let rep = store.reputation("alice");
        assert_eq!(rep.contribution_count, 3);
        assert_eq!(rep.contributions_by_type.get("task_completed"), Some(&2));
        assert_eq!(rep.contributions_by_type.get("review_approved"), Some(&1));
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("reputation.json");
        let mut store = ReputationStore::default();
        store.record(make_contribution("alice", ContributionType::TaskCompleted));
        store.save(&path).unwrap();

        let loaded = ReputationStore::load(&path);
        assert_eq!(loaded.contributions.len(), 1);
    }

    #[test]
    fn test_no_recommendation_low_score() {
        let mut store = ReputationStore::default();
        store.record(make_contribution("alice", ContributionType::TaskCompleted));
        let recs = store.recommendations(&[("alice".to_string(), 1)]);
        assert!(recs.is_empty()); // Score ~1.0, not enough for trust upgrade
    }

    #[test]
    fn test_high_score_trust_4() {
        let mut store = ReputationStore::default();
        // Score >= 80 for trust 4
        for _ in 0..20 {
            store.record(make_contribution(
                "alice",
                ContributionType::ProjectCompleted,
            ));
        }
        let rep = store.reputation("alice");
        assert_eq!(rep.recommended_trust, Some(4));
    }
}
