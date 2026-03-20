use crate::project::{Task, TaskPriority, TaskStatus};
use uuid::Uuid;

/// Label applied to all issues created by Agora sync.
/// Used to identify Agora-origin issues on re-import (prevents feedback loops).
const AGORA_LABEL: &str = "agora-sync";

/// Maximum issues to push in a single sync cycle (safety cap).
const MAX_PUSH_PER_SYNC: usize = 20;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GitHubConfig {
    pub token: Option<String>,
}

impl GitHubConfig {
    pub fn path() -> std::path::PathBuf {
        crate::config::agora_home().join("github.json")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(GitHubConfig { token: None })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn parse_github_repo(url: &str) -> Option<(String, String)> {
    let s = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .replace("git@github.com:", "https://github.com/");
    let parts: Vec<&str> = s
        .trim_start_matches("https://github.com/")
        .split('/')
        .collect();
    if parts.len() >= 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn github_to_task_status(state: &str, labels: &[String]) -> TaskStatus {
    if labels.iter().any(|l| l == "blocked") {
        TaskStatus::Blocked
    } else if labels.iter().any(|l| l == "in-progress") {
        TaskStatus::InProgress
    } else if state == "closed" {
        TaskStatus::Done
    } else {
        TaskStatus::Todo
    }
}

fn task_status_to_github(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Done => "closed",
        _ => "open",
    }
}

/// Fetch open pull requests from GitHub.
pub async fn fetch_pull_requests(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<PullRequestInfo>, String> {
    let crab = octocrab::Octocrab::builder()
        .personal_token(token.to_string())
        .build()
        .map_err(|e| format!("Failed to create GitHub client: {}", e))?;

    let page = crab
        .pulls(owner, repo)
        .list()
        .state(octocrab::params::State::Open)
        .per_page(50)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch PRs: {}", e))?;

    let mut prs = Vec::new();
    for pr in page.items {
        let labels: Vec<String> = pr
            .labels
            .as_ref()
            .map(|l| l.iter().map(|label| label.name.clone()).collect())
            .unwrap_or_default();
        prs.push(PullRequestInfo {
            number: pr.number,
            title: pr.title.clone().unwrap_or_default(),
            author: pr
                .user
                .as_ref()
                .map(|u| u.login.clone())
                .unwrap_or_default(),
            state: if pr.merged_at.is_some() {
                "merged".into()
            } else {
                "open".into()
            },
            draft: pr.draft.unwrap_or(false),
            labels,
            created_at: pr.created_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            updated_at: pr.updated_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            html_url: pr
                .html_url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default(),
        });
    }
    Ok(prs)
}

/// Pull request summary for dashboard display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PullRequestInfo {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub labels: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub html_url: String,
}

/// Import issues from GitHub. SKIPS issues labeled with AGORA_LABEL
/// (those were created by us — importing them causes feedback loops).
pub async fn import_issues(token: &str, owner: &str, repo: &str) -> Result<Vec<Task>, String> {
    let crab = octocrab::Octocrab::builder()
        .personal_token(token.to_string())
        .build()
        .map_err(|e| format!("Failed to create GitHub client: {}", e))?;

    let page = crab
        .issues(owner, repo)
        .list()
        .state(octocrab::params::State::All)
        .per_page(100)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch issues: {}", e))?;

    let mut tasks = Vec::new();
    for issue in page.items {
        if issue.pull_request.is_some() {
            continue;
        }
        let labels: Vec<String> = issue.labels.iter().map(|l| l.name.clone()).collect();

        // SAFEGUARD: Skip issues we created (prevents feedback loop)
        if labels.iter().any(|l| l == AGORA_LABEL) {
            continue;
        }

        let status = github_to_task_status(
            if issue.state == octocrab::models::IssueState::Open {
                "open"
            } else {
                "closed"
            },
            &labels,
        );
        let priority = if labels.iter().any(|l| l == "critical") {
            Some(TaskPriority::Critical)
        } else if labels.iter().any(|l| l == "high" || l == "priority:high") {
            Some(TaskPriority::High)
        } else {
            None
        };
        let now = chrono::Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            title: issue.title.clone(),
            description: issue.body.clone(),
            status,
            assignee: issue.assignees.first().map(|a| a.login.clone()),
            priority,
            depends_on: vec![],
            created_at: now,
            updated_at: now,
            created_by: Some("github-sync".to_string()),
            github_issue_number: Some(issue.number),
        };
        tasks.push(task);
    }
    Ok(tasks)
}

/// Push a task as a GitHub issue. Adds AGORA_LABEL so we can identify it on re-import.
pub async fn push_task_as_issue(
    token: &str,
    owner: &str,
    repo: &str,
    task: &Task,
) -> Result<u64, String> {
    let crab = octocrab::Octocrab::builder()
        .personal_token(token.to_string())
        .build()
        .map_err(|e| format!("Failed to create GitHub client: {}", e))?;

    // Ensure the agora-sync label exists
    let _ = crab
        .issues(owner, repo)
        .create_label(AGORA_LABEL, "0e8a16", "Created by Agora sync")
        .await;

    let mut labels = vec![AGORA_LABEL.to_string()];
    match task.status {
        TaskStatus::InProgress => labels.push("in-progress".to_string()),
        TaskStatus::Blocked => labels.push("blocked".to_string()),
        _ => {}
    }
    if let Some(ref p) = task.priority {
        match p {
            TaskPriority::High => labels.push("priority:high".to_string()),
            TaskPriority::Critical => labels.push("critical".to_string()),
            _ => {}
        }
    }

    let issue = crab
        .issues(owner, repo)
        .create(&task.title)
        .body(task.description.as_deref().unwrap_or(""))
        .labels(labels)
        .send()
        .await
        .map_err(|e| format!("Failed to create issue: {}", e))?;

    if task.status == TaskStatus::Done {
        let _ = crab
            .issues(owner, repo)
            .update(issue.number)
            .state(octocrab::models::IssueState::Closed)
            .send()
            .await;
    }

    Ok(issue.number)
}

pub struct SyncResult {
    pub imported: usize,
    pub pushed: usize,
    pub errors: Vec<String>,
    /// Task ID → GitHub issue number, for updating local tasks after push.
    pub pushed_mappings: Vec<(Uuid, u64)>,
}

/// Bidirectional sync with THREE safeguards against feedback loops:
/// 1. Pushed issues get AGORA_LABEL — skipped on re-import
/// 2. Only tasks without github_issue_number AND not from github-sync are pushed
/// 3. Max MAX_PUSH_PER_SYNC issues pushed per cycle (safety cap)
pub async fn sync_bidirectional(
    token: &str,
    owner: &str,
    repo: &str,
    existing_tasks: &[Task],
) -> Result<SyncResult, String> {
    let remote_issues = import_issues(token, owner, repo).await?;
    let mut result = SyncResult {
        imported: 0,
        pushed: 0,
        errors: vec![],
        pushed_mappings: vec![],
    };

    // Count genuinely new issues (not already tracked)
    let existing_issue_numbers: std::collections::HashSet<u64> = existing_tasks
        .iter()
        .filter_map(|t| t.github_issue_number)
        .collect();
    let existing_titles: std::collections::HashSet<String> = existing_tasks
        .iter()
        .map(|t| t.title.to_lowercase())
        .collect();

    for issue_task in &remote_issues {
        if let Some(num) = issue_task.github_issue_number {
            if !existing_issue_numbers.contains(&num)
                && !existing_titles.contains(&issue_task.title.to_lowercase())
            {
                result.imported += 1;
            }
        }
    }

    // Push local tasks — with THREE guards:
    // 1. Must not have github_issue_number (not already synced)
    // 2. Must not be created by github-sync (not imported)
    // 3. Title must not match an existing GitHub issue (dedup)
    let remote_titles: std::collections::HashSet<String> = remote_issues
        .iter()
        .map(|t| t.title.to_lowercase())
        .collect();

    let mut push_count = 0;
    for task in existing_tasks {
        if push_count >= MAX_PUSH_PER_SYNC {
            break;
        }
        if task.github_issue_number.is_some() {
            continue; // Already synced
        }
        if task.created_by.as_deref() == Some("github-sync") {
            continue; // Imported from GitHub
        }
        if remote_titles.contains(&task.title.to_lowercase()) {
            continue; // Title already exists on GitHub
        }
        match push_task_as_issue(token, owner, repo, task).await {
            Ok(num) => {
                result.pushed += 1;
                result.pushed_mappings.push((task.id, num));
                push_count += 1;
            }
            Err(e) => result.errors.push(e),
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_repo_https() {
        let (o, r) = parse_github_repo("https://github.com/user/repo").unwrap();
        assert_eq!(o, "user");
        assert_eq!(r, "repo");
    }

    #[test]
    fn test_parse_github_repo_ssh() {
        let (o, r) = parse_github_repo("git@github.com:user/repo.git").unwrap();
        assert_eq!(o, "user");
        assert_eq!(r, "repo");
    }

    #[test]
    fn test_parse_github_repo_trailing_slash() {
        let (o, r) = parse_github_repo("https://github.com/user/repo/").unwrap();
        assert_eq!(o, "user");
        assert_eq!(r, "repo");
    }

    #[test]
    fn test_parse_github_repo_https_git() {
        let (o, r) = parse_github_repo("https://github.com/user/repo.git").unwrap();
        assert_eq!(o, "user");
        assert_eq!(r, "repo");
    }

    #[test]
    fn test_parse_github_repo_bare() {
        let (o, r) = parse_github_repo("user/repo").unwrap();
        assert_eq!(o, "user");
        assert_eq!(r, "repo");
    }

    #[test]
    fn test_parse_github_repo_invalid() {
        assert!(parse_github_repo("not-a-repo").is_none());
    }

    #[test]
    fn test_github_config_default() {
        let config = GitHubConfig { token: None };
        assert!(config.token.is_none());
    }

    #[test]
    fn test_github_to_task_status() {
        assert_eq!(github_to_task_status("open", &[]), TaskStatus::Todo);
        assert_eq!(github_to_task_status("closed", &[]), TaskStatus::Done);
        assert_eq!(
            github_to_task_status("open", &["blocked".to_string()]),
            TaskStatus::Blocked
        );
    }

    #[test]
    fn test_task_status_mapping() {
        assert_eq!(task_status_to_github(&TaskStatus::Done), "closed");
        assert_eq!(task_status_to_github(&TaskStatus::Todo), "open");
    }
}
