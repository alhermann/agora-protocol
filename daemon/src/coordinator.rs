//! Coordinator — automated project coordination with rule-based intelligence.
//!
//! Provides: auto-assign tasks, detect blocked tasks, suggest stage advances,
//! workload balancing, and project digest generation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Coordinator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    /// Whether the coordinator is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Interval between digest generations (seconds).
    #[serde(default = "default_digest_interval")]
    pub digest_interval: u64,
    /// Auto-assign tasks based on role/keyword matching.
    #[serde(default = "default_true")]
    pub auto_assign: bool,
    /// Suggest stage advances when all tasks are done.
    #[serde(default = "default_true")]
    pub auto_advance: bool,
    /// Workload imbalance threshold (ratio of max/min tasks).
    #[serde(default = "default_workload_threshold")]
    pub workload_threshold: f64,
}

fn default_digest_interval() -> u64 {
    3600
}
fn default_true() -> bool {
    true
}
fn default_workload_threshold() -> f64 {
    3.0
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            digest_interval: default_digest_interval(),
            auto_assign: true,
            auto_advance: true,
            workload_threshold: default_workload_threshold(),
        }
    }
}

/// Types of coordinator suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionType {
    TaskAssignment,
    StageAdvance,
    WorkloadRebalance,
    BlockedAlert,
    DigestReady,
}

/// A coordinator suggestion for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorSuggestion {
    pub id: Uuid,
    pub project_id: Uuid,
    pub suggestion_type: SuggestionType,
    pub title: String,
    pub detail: String,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub acted_on: bool,
}

/// A project digest summarizing recent activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDigest {
    pub id: Uuid,
    pub project_id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub summary: String,
    pub tasks_completed: Vec<String>,
    pub tasks_created: Vec<String>,
    pub tasks_blocked: Vec<String>,
    pub agents_active: Vec<String>,
    pub stage_changes: Vec<String>,
}

/// Simplified project/task info for coordinator analysis.
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    pub id: Uuid,
    pub name: String,
    pub stage: String,
    pub agents: Vec<AgentSnapshot>,
    pub tasks: Vec<TaskSnapshot>,
}

#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    pub name: String,
    pub role: String,
    pub clocked_in: bool,
}

#[derive(Debug, Clone)]
pub struct TaskSnapshot {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub assignee: Option<String>,
    pub depends_on: Vec<Uuid>,
}

/// Auto-assign unassigned tasks to agents based on role and keyword matching.
pub fn auto_assign(project: &ProjectSnapshot) -> Vec<CoordinatorSuggestion> {
    let mut suggestions = Vec::new();

    let unassigned: Vec<&TaskSnapshot> = project
        .tasks
        .iter()
        .filter(|t| t.assignee.is_none() && t.status != "done")
        .collect();

    if unassigned.is_empty() {
        return suggestions;
    }

    // Get active developers
    let developers: Vec<&AgentSnapshot> = project
        .agents
        .iter()
        .filter(|a| a.role == "developer" || a.role == "owner")
        .collect();

    if developers.is_empty() {
        return suggestions;
    }

    // Count current assignments per developer
    let mut load: HashMap<&str, usize> = HashMap::new();
    for dev in &developers {
        load.insert(&dev.name, 0);
    }
    for task in &project.tasks {
        if let Some(ref assignee) = task.assignee {
            if task.status != "done" {
                if let Some(count) = load.get_mut(assignee.as_str()) {
                    *count += 1;
                }
            }
        }
    }

    for task in unassigned {
        // Assign to developer with lowest current load
        if let Some((&name, _)) = load.iter().min_by_key(|(_, count)| **count) {
            suggestions.push(CoordinatorSuggestion {
                id: Uuid::new_v4(),
                project_id: project.id,
                suggestion_type: SuggestionType::TaskAssignment,
                title: format!("Assign '{}' to {}", task.title, name),
                detail: format!(
                    "Task '{}' is unassigned. {} has the lowest workload ({} active tasks).",
                    task.title,
                    name,
                    load.get(name).unwrap_or(&0)
                ),
                timestamp: Utc::now(),
                acted_on: false,
            });
            *load.entry(name).or_insert(0) += 1;
        }
    }

    suggestions
}

/// Check if all tasks are done and suggest stage advancement.
pub fn check_stage_advance(project: &ProjectSnapshot) -> Option<CoordinatorSuggestion> {
    let active_tasks: Vec<&TaskSnapshot> = project
        .tasks
        .iter()
        .filter(|t| t.status != "done")
        .collect();

    if !project.tasks.is_empty() && active_tasks.is_empty() {
        let next_stage = match project.stage.as_str() {
            "investigation" => "implementation",
            "implementation" => "review",
            "review" => "integration",
            "integration" => "deployment",
            _ => return None,
        };

        Some(CoordinatorSuggestion {
            id: Uuid::new_v4(),
            project_id: project.id,
            suggestion_type: SuggestionType::StageAdvance,
            title: format!("Advance to {} stage", next_stage),
            detail: format!(
                "All {} tasks are completed. Consider advancing from '{}' to '{}'.",
                project.tasks.len(),
                project.stage,
                next_stage
            ),
            timestamp: Utc::now(),
            acted_on: false,
        })
    } else {
        None
    }
}

/// Detect tasks that are blocked by unmet dependencies.
pub fn detect_blocked(project: &ProjectSnapshot) -> Vec<CoordinatorSuggestion> {
    let mut suggestions = Vec::new();

    let done_ids: std::collections::HashSet<Uuid> = project
        .tasks
        .iter()
        .filter(|t| t.status == "done")
        .map(|t| t.id)
        .collect();

    for task in &project.tasks {
        if task.status == "done" || task.status == "blocked" {
            continue;
        }
        let unmet: Vec<&Uuid> = task
            .depends_on
            .iter()
            .filter(|dep| !done_ids.contains(dep))
            .collect();

        if !unmet.is_empty() {
            let dep_names: Vec<String> = unmet
                .iter()
                .filter_map(|dep_id| {
                    project
                        .tasks
                        .iter()
                        .find(|t| t.id == **dep_id)
                        .map(|t| t.title.clone())
                })
                .collect();

            suggestions.push(CoordinatorSuggestion {
                id: Uuid::new_v4(),
                project_id: project.id,
                suggestion_type: SuggestionType::BlockedAlert,
                title: format!("'{}' is blocked", task.title),
                detail: format!(
                    "Task '{}' depends on {} incomplete task(s): {}",
                    task.title,
                    unmet.len(),
                    dep_names.join(", ")
                ),
                timestamp: Utc::now(),
                acted_on: false,
            });
        }
    }

    suggestions
}

/// Detect workload imbalance among developers.
pub fn workload_balance(
    project: &ProjectSnapshot,
    threshold: f64,
) -> Option<CoordinatorSuggestion> {
    let mut load: HashMap<&str, usize> = HashMap::new();

    for agent in &project.agents {
        if agent.role == "developer" || agent.role == "owner" {
            load.insert(&agent.name, 0);
        }
    }

    for task in &project.tasks {
        if let Some(ref assignee) = task.assignee {
            if task.status != "done" {
                if let Some(count) = load.get_mut(assignee.as_str()) {
                    *count += 1;
                }
            }
        }
    }

    if load.len() < 2 {
        return None;
    }

    let max_load = load.values().copied().max().unwrap_or(0);
    let min_load = load.values().copied().min().unwrap_or(0);

    if min_load == 0 && max_load > 0
        || (min_load > 0 && max_load as f64 / min_load as f64 > threshold)
    {
        let overloaded: Vec<String> = load
            .iter()
            .filter(|(_, count)| **count == max_load)
            .map(|(name, count)| format!("{} ({})", name, count))
            .collect();
        let underloaded: Vec<String> = load
            .iter()
            .filter(|(_, count)| **count == min_load)
            .map(|(name, count)| format!("{} ({})", name, count))
            .collect();

        Some(CoordinatorSuggestion {
            id: Uuid::new_v4(),
            project_id: project.id,
            suggestion_type: SuggestionType::WorkloadRebalance,
            title: "Workload imbalance detected".to_string(),
            detail: format!(
                "Overloaded: {}. Underloaded: {}. Consider redistributing tasks.",
                overloaded.join(", "),
                underloaded.join(", ")
            ),
            timestamp: Utc::now(),
            acted_on: false,
        })
    } else {
        None
    }
}

/// Generate a project digest from audit entries.
pub fn generate_digest(
    project_id: Uuid,
    project_name: &str,
    period_start: DateTime<Utc>,
    tasks_completed: Vec<String>,
    tasks_created: Vec<String>,
    tasks_blocked: Vec<String>,
    agents_active: Vec<String>,
    stage_changes: Vec<String>,
) -> ProjectDigest {
    let now = Utc::now();
    let summary = format!(
        "Project '{}': {} tasks completed, {} created, {} blocked. {} active agent(s).",
        project_name,
        tasks_completed.len(),
        tasks_created.len(),
        tasks_blocked.len(),
        agents_active.len(),
    );

    ProjectDigest {
        id: Uuid::new_v4(),
        project_id,
        period_start,
        period_end: now,
        summary,
        tasks_completed,
        tasks_created,
        tasks_blocked,
        agents_active,
        stage_changes,
    }
}

/// Run all coordinator checks on a project and return suggestions.
pub fn analyze_project(
    project: &ProjectSnapshot,
    config: &CoordinatorConfig,
) -> Vec<CoordinatorSuggestion> {
    let mut suggestions = Vec::new();

    if config.auto_assign {
        suggestions.extend(auto_assign(project));
    }

    if config.auto_advance {
        if let Some(s) = check_stage_advance(project) {
            suggestions.push(s);
        }
    }

    suggestions.extend(detect_blocked(project));

    if let Some(s) = workload_balance(project, config.workload_threshold) {
        suggestions.push(s);
    }

    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project() -> ProjectSnapshot {
        ProjectSnapshot {
            id: Uuid::new_v4(),
            name: "test-project".to_string(),
            stage: "implementation".to_string(),
            agents: vec![
                AgentSnapshot {
                    name: "alice".to_string(),
                    role: "developer".to_string(),
                    clocked_in: true,
                },
                AgentSnapshot {
                    name: "bob".to_string(),
                    role: "developer".to_string(),
                    clocked_in: true,
                },
            ],
            tasks: vec![],
        }
    }

    fn make_task(title: &str, status: &str, assignee: Option<&str>) -> TaskSnapshot {
        TaskSnapshot {
            id: Uuid::new_v4(),
            title: title.to_string(),
            status: status.to_string(),
            assignee: assignee.map(|s| s.to_string()),
            depends_on: vec![],
        }
    }

    #[test]
    fn test_auto_assign_unassigned() {
        let mut project = make_project();
        project.tasks.push(make_task("Fix bug", "todo", None));
        project.tasks.push(make_task("Add feature", "todo", None));

        let suggestions = auto_assign(&project);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(
            suggestions[0].suggestion_type,
            SuggestionType::TaskAssignment
        );
    }

    #[test]
    fn test_auto_assign_no_unassigned() {
        let mut project = make_project();
        project
            .tasks
            .push(make_task("Fix bug", "todo", Some("alice")));
        let suggestions = auto_assign(&project);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_stage_advance_all_done() {
        let mut project = make_project();
        project
            .tasks
            .push(make_task("Task 1", "done", Some("alice")));
        project.tasks.push(make_task("Task 2", "done", Some("bob")));

        let suggestion = check_stage_advance(&project);
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert_eq!(s.suggestion_type, SuggestionType::StageAdvance);
        assert!(s.title.contains("review"));
    }

    #[test]
    fn test_stage_advance_incomplete() {
        let mut project = make_project();
        project
            .tasks
            .push(make_task("Task 1", "done", Some("alice")));
        project
            .tasks
            .push(make_task("Task 2", "in_progress", Some("bob")));

        let suggestion = check_stage_advance(&project);
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_detect_blocked() {
        let mut project = make_project();
        let dep_id = Uuid::new_v4();
        project.tasks.push(TaskSnapshot {
            id: dep_id,
            title: "Dependency".to_string(),
            status: "in_progress".to_string(),
            assignee: Some("alice".to_string()),
            depends_on: vec![],
        });
        project.tasks.push(TaskSnapshot {
            id: Uuid::new_v4(),
            title: "Blocked task".to_string(),
            status: "todo".to_string(),
            assignee: Some("bob".to_string()),
            depends_on: vec![dep_id],
        });

        let suggestions = detect_blocked(&project);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].suggestion_type, SuggestionType::BlockedAlert);
        assert!(suggestions[0].detail.contains("Dependency"));
    }

    #[test]
    fn test_workload_balance() {
        let mut project = make_project();
        // Alice has 5 tasks, Bob has 0
        for i in 0..5 {
            project
                .tasks
                .push(make_task(&format!("Task {}", i), "todo", Some("alice")));
        }

        let suggestion = workload_balance(&project, 3.0);
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert_eq!(s.suggestion_type, SuggestionType::WorkloadRebalance);
    }

    #[test]
    fn test_workload_balanced() {
        let mut project = make_project();
        project
            .tasks
            .push(make_task("Task 1", "todo", Some("alice")));
        project.tasks.push(make_task("Task 2", "todo", Some("bob")));

        let suggestion = workload_balance(&project, 3.0);
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_generate_digest() {
        let digest = generate_digest(
            Uuid::new_v4(),
            "test-project",
            Utc::now() - chrono::Duration::hours(1),
            vec!["Fix bug".to_string()],
            vec!["New feature".to_string()],
            vec![],
            vec!["alice".to_string(), "bob".to_string()],
            vec![],
        );
        assert!(digest.summary.contains("1 tasks completed"));
        assert!(digest.summary.contains("2 active agent"));
    }

    #[test]
    fn test_analyze_project() {
        let mut project = make_project();
        project
            .tasks
            .push(make_task("Task 1", "done", Some("alice")));
        project.tasks.push(make_task("Task 2", "done", Some("bob")));

        let config = CoordinatorConfig::default();
        let suggestions = analyze_project(&project, &config);
        // Should suggest stage advance
        assert!(
            suggestions
                .iter()
                .any(|s| s.suggestion_type == SuggestionType::StageAdvance)
        );
    }
}
