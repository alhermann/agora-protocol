//! Project collaboration — data model, persistence, and context.
//!
//! A project groups multiple agents working together on a shared task or codebase.
//! Agents have roles (owner, developer, reviewer, etc.) and can clock in/out
//! to signal availability.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Project Rooms
// ---------------------------------------------------------------------------

/// A room within a project — a focused conversation channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRoom {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Deterministic conversation ID: UUID v5 from "{project_id}:{room_name}".
    pub conversation_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

impl ProjectRoom {
    /// Compute a deterministic conversation_id from a project ID and room name.
    pub fn make_conversation_id(project_id: &Uuid, room_name: &str) -> Uuid {
        let key = format!("{}:{}", project_id, room_name);
        Uuid::new_v5(&Uuid::NAMESPACE_DNS, key.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Role an agent plays in a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRole {
    Owner,
    Overseer,
    Developer,
    Reviewer,
    Consultant,
    Observer,
    Tester,
}

impl ProjectRole {
    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Owner => "Owner",
            Self::Overseer => "Overseer",
            Self::Developer => "Developer",
            Self::Reviewer => "Reviewer",
            Self::Consultant => "Consultant",
            Self::Observer => "Observer",
            Self::Tester => "Tester",
        }
    }

    /// Permissions granted by this role.
    pub fn permissions(&self) -> Vec<&'static str> {
        match self {
            Self::Owner => vec![
                "read",
                "write",
                "commit",
                "approve",
                "coordinate",
                "invite",
                "archive",
            ],
            Self::Overseer => vec!["read", "write", "coordinate", "invite", "approve"],
            Self::Developer => vec!["read", "write", "commit"],
            Self::Reviewer => vec!["read", "approve"],
            Self::Consultant => vec!["read", "write"],
            Self::Observer => vec!["read"],
            Self::Tester => vec!["read", "write", "commit"],
        }
    }
}

impl std::fmt::Display for ProjectRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::str::FromStr for ProjectRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "owner" => Ok(Self::Owner),
            "overseer" => Ok(Self::Overseer),
            "developer" => Ok(Self::Developer),
            "reviewer" => Ok(Self::Reviewer),
            "consultant" => Ok(Self::Consultant),
            "observer" => Ok(Self::Observer),
            "tester" => Ok(Self::Tester),
            _ => Err(format!("Unknown role: {}", s)),
        }
    }
}

/// Status of a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Paused,
    Completed,
    Archived,
}

impl ProjectStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Archived => "Archived",
        }
    }
}

impl std::fmt::Display for ProjectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Direction of a project invitation (same pattern as FriendRequest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvitationDirection {
    Inbound,
    Outbound,
}

/// Status of a project invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
}

/// Status of a task on the task board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Done,
    Blocked,
}

impl TaskStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Todo => "Todo",
            Self::InProgress => "In Progress",
            Self::Done => "Done",
            Self::Blocked => "Blocked",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "todo" => Ok(Self::Todo),
            "in_progress" | "in-progress" | "in progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("Unknown task status: {}", s)),
        }
    }
}

/// Priority of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for TaskPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

/// A task on the project task board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<TaskPriority>,
    /// Task IDs that must be completed before this task can be started.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    /// GitHub issue number if synced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_issue_number: Option<u64>,
}

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// An agent participating in a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAgent {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did: Option<String>,
    pub role: ProjectRole,
    pub joined_at: DateTime<Utc>,
    #[serde(default)]
    pub clocked_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_clock_in: Option<DateTime<Utc>>,
    /// Whether the agent is suspended (blocked from all actions).
    #[serde(default)]
    pub suspended: bool,
    /// Reason for suspension, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_reason: Option<String>,
    /// Whether the agent is muted (can read but not send in project rooms).
    #[serde(default)]
    pub muted: bool,
}

/// A collaboration project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub owner_did: String,
    pub owner_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub status: ProjectStatus,
    pub agents: Vec<ProjectAgent>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Task board for this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,
    /// Append-only audit trail for project mutations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit_trail: Vec<AuditEntry>,
    /// Current lifecycle stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_stage: Option<ProjectStage>,
    /// Rooms for organized conversations within this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rooms: Vec<ProjectRoom>,
}

/// A signed audit trail entry recording a project mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    /// DID of the author who performed the action.
    pub author_did: String,
    /// Author's human-readable name.
    pub author_name: String,
    /// Action type (e.g., "task.created", "agent.joined").
    pub action: String,
    /// Human-readable detail about what happened.
    pub detail: String,
    /// Ed25519 signature of the canonical string (base58-encoded).
    pub signature: String,
}

impl AuditEntry {
    /// Create a new signed audit entry.
    pub fn new_signed(
        author_did: &str,
        author_name: &str,
        action: &str,
        detail: &str,
        identity: &crate::identity::AgentIdentity,
    ) -> Self {
        let timestamp = Utc::now();
        let id = Uuid::new_v4();
        let canonical = format!(
            "{}|{}|{}|{}",
            timestamp.to_rfc3339(),
            author_did,
            action,
            detail
        );
        let sig = identity.sign(canonical.as_bytes());
        Self {
            id,
            timestamp,
            author_did: author_did.to_string(),
            author_name: author_name.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
            signature: bs58::encode(&sig).into_string(),
        }
    }

    /// Verify this entry's signature against a known public key.
    pub fn verify(&self, public_key_b58: &str) -> bool {
        let canonical = format!(
            "{}|{}|{}|{}",
            self.timestamp.to_rfc3339(),
            self.author_did,
            self.action,
            self.detail
        );
        crate::identity::AgentIdentity::verify_base58(
            public_key_b58,
            canonical.as_bytes(),
            &self.signature,
        )
    }
}

/// Lifecycle stage of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStage {
    Investigation,
    Implementation,
    Review,
    Integration,
    Deployment,
}

impl ProjectStage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Investigation => "Investigation",
            Self::Implementation => "Implementation",
            Self::Review => "Review",
            Self::Integration => "Integration",
            Self::Deployment => "Deployment",
        }
    }

    /// Ordered list of all stages.
    pub fn all() -> &'static [ProjectStage] {
        &[
            ProjectStage::Investigation,
            ProjectStage::Implementation,
            ProjectStage::Review,
            ProjectStage::Integration,
            ProjectStage::Deployment,
        ]
    }

    /// Index in the stage sequence (0-based).
    pub fn index(&self) -> usize {
        match self {
            Self::Investigation => 0,
            Self::Implementation => 1,
            Self::Review => 2,
            Self::Integration => 3,
            Self::Deployment => 4,
        }
    }

    /// Next stage, if any.
    pub fn next(&self) -> Option<ProjectStage> {
        Self::all().get(self.index() + 1).cloned()
    }

    /// Check if a role has a specific permission in this stage.
    pub fn role_has_permission(&self, role: &ProjectRole, permission: &str) -> bool {
        self.role_permissions(role).contains(&permission)
    }

    /// Get permissions for a role in this stage.
    pub fn role_permissions(&self, role: &ProjectRole) -> Vec<&'static str> {
        match (self, role) {
            // Investigation: everyone reads
            (Self::Investigation, _) => vec!["read"],
            // Implementation: owner/overseer full, dev writes, reviewer reads
            (Self::Implementation, ProjectRole::Owner | ProjectRole::Overseer) => {
                vec!["read", "write", "commit"]
            }
            (Self::Implementation, ProjectRole::Developer | ProjectRole::Tester) => {
                vec!["read", "write", "commit"]
            }
            (Self::Implementation, ProjectRole::Reviewer) => vec!["read"],
            (Self::Implementation, _) => vec!["read"],
            // Review: owner/overseer approve, reviewer approves, dev reads
            (Self::Review, ProjectRole::Owner | ProjectRole::Overseer) => {
                vec!["read", "approve"]
            }
            (Self::Review, ProjectRole::Reviewer) => vec!["read", "approve"],
            (Self::Review, _) => vec!["read"],
            // Integration: owner full, others read
            (Self::Integration, ProjectRole::Owner | ProjectRole::Overseer) => {
                vec!["read", "write", "approve"]
            }
            (Self::Integration, _) => vec!["read"],
            // Deployment: owner full + deploy
            (Self::Deployment, ProjectRole::Owner | ProjectRole::Overseer) => {
                vec!["read", "write", "approve", "deploy"]
            }
            (Self::Deployment, _) => vec!["read"],
        }
    }

    /// Check if advancing from this stage is allowed (all tasks done, etc.).
    pub fn can_advance(project: &Project) -> bool {
        // Can advance if all tasks are Done or there are no tasks
        project.tasks.iter().all(|t| t.status == TaskStatus::Done)
    }
}

impl std::fmt::Display for ProjectStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl std::str::FromStr for ProjectStage {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "investigation" => Ok(Self::Investigation),
            "implementation" => Ok(Self::Implementation),
            "review" => Ok(Self::Review),
            "integration" => Ok(Self::Integration),
            "deployment" => Ok(Self::Deployment),
            _ => Err(format!("Unknown stage: {}", s)),
        }
    }
}

/// A project invitation (inbound or outbound).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInvitation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub peer_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_did: Option<String>,
    pub role: ProjectRole,
    pub direction: InvitationDirection,
    pub status: InvitationStatus,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ProjectContext>,
}

/// Context package sent with an invitation so the invitee understands the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub project_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub your_role: ProjectRole,
    pub your_permissions: Vec<String>,
    pub current_agents: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

// ---------------------------------------------------------------------------
// ProjectStore — persists to ~/.agora/projects.json
// ---------------------------------------------------------------------------

/// Persistent store for projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStore {
    pub projects: Vec<Project>,
    #[serde(skip)]
    path: PathBuf,
}

impl ProjectStore {
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        crate::config::agora_home().join("projects.json")
    }

    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => match serde_json::from_str(&data) {
                    Ok(mut store) => {
                        let store: &mut ProjectStore = &mut store;
                        store.path = path.to_path_buf();
                        return store.clone();
                    }
                    Err(e) => warn!("Failed to parse projects file: {}", e),
                },
                Err(e) => warn!("Failed to read projects file: {}", e),
            }
        }
        Self {
            projects: Vec::new(),
            path: path.to_path_buf(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_string_pretty(&self).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list(&self) -> &[Project] {
        &self.projects
    }

    pub fn get(&self, id: &Uuid) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == *id)
    }

    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut Project> {
        self.projects.iter_mut().find(|p| p.id == *id)
    }

    pub fn add(&mut self, project: Project) {
        info!("Project created: {} ({})", project.name, project.id);
        self.projects.push(project);
    }

    pub fn remove(&mut self, id: &Uuid) -> bool {
        let len = self.projects.len();
        self.projects.retain(|p| p.id != *id);
        self.projects.len() < len
    }
}

// ---------------------------------------------------------------------------
// ProjectInvitationStore — persists to ~/.agora/project_invitations.json
// ---------------------------------------------------------------------------

/// Persistent store for project invitations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInvitationStore {
    pub invitations: Vec<ProjectInvitation>,
    #[serde(skip)]
    path: PathBuf,
}

impl ProjectInvitationStore {
    pub fn default_path() -> PathBuf {
        crate::config::agora_home().join("project_invitations.json")
    }

    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(data) => match serde_json::from_str(&data) {
                    Ok(mut store) => {
                        let store: &mut ProjectInvitationStore = &mut store;
                        store.path = path.to_path_buf();
                        return store.clone();
                    }
                    Err(e) => warn!("Failed to parse project invitations file: {}", e),
                },
                Err(e) => warn!("Failed to read project invitations file: {}", e),
            }
        }
        Self {
            invitations: Vec::new(),
            path: path.to_path_buf(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_string_pretty(&self).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list(&self) -> &[ProjectInvitation] {
        &self.invitations
    }

    pub fn get(&self, id: &Uuid) -> Option<&ProjectInvitation> {
        self.invitations.iter().find(|i| i.id == *id)
    }

    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut ProjectInvitation> {
        self.invitations.iter_mut().find(|i| i.id == *id)
    }

    pub fn pending_inbound(&self) -> Vec<&ProjectInvitation> {
        self.invitations
            .iter()
            .filter(|i| {
                i.direction == InvitationDirection::Inbound && i.status == InvitationStatus::Pending
            })
            .collect()
    }

    pub fn pending_outbound_to(&self, peer_name: &str) -> Option<&ProjectInvitation> {
        self.invitations.iter().find(|i| {
            i.direction == InvitationDirection::Outbound
                && i.status == InvitationStatus::Pending
                && i.peer_name == peer_name
        })
    }

    pub fn add(&mut self, invitation: ProjectInvitation) {
        info!(
            "Project invitation created: {} → {} (project: {})",
            invitation.peer_name,
            invitation.role.name(),
            invitation.project_name
        );
        self.invitations.push(invitation);
    }

    pub fn accept(&mut self, id: &Uuid) -> Option<&ProjectInvitation> {
        if let Some(inv) = self.invitations.iter_mut().find(|i| i.id == *id) {
            inv.status = InvitationStatus::Accepted;
            inv.resolved_at = Some(Utc::now());
            return Some(inv);
        }
        None
    }

    pub fn decline(&mut self, id: &Uuid) -> Option<&ProjectInvitation> {
        if let Some(inv) = self.invitations.iter_mut().find(|i| i.id == *id) {
            inv.status = InvitationStatus::Declined;
            inv.resolved_at = Some(Utc::now());
            return Some(inv);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_role_roundtrip() {
        let role = ProjectRole::Developer;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"developer\"");
        let parsed: ProjectRole = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, role);
    }

    #[test]
    fn test_project_role_from_str() {
        assert_eq!(
            "developer".parse::<ProjectRole>().unwrap(),
            ProjectRole::Developer
        );
        assert_eq!("Owner".parse::<ProjectRole>().unwrap(), ProjectRole::Owner);
        assert!("invalid".parse::<ProjectRole>().is_err());
    }

    #[test]
    fn test_project_role_permissions() {
        let perms = ProjectRole::Developer.permissions();
        assert!(perms.contains(&"read"));
        assert!(perms.contains(&"write"));
        assert!(perms.contains(&"commit"));
        assert!(!perms.contains(&"approve"));

        let owner_perms = ProjectRole::Owner.permissions();
        assert!(owner_perms.contains(&"approve"));
        assert!(owner_perms.contains(&"invite"));
    }

    #[test]
    fn test_project_serialize_roundtrip() {
        let project = Project {
            id: Uuid::new_v4(),
            name: "Test Project".to_string(),
            description: Some("A test".to_string()),
            owner_did: "did:agora:test".to_string(),
            owner_name: "alice".to_string(),
            repo: Some("https://github.com/test/repo".to_string()),
            status: ProjectStatus::Active,
            agents: vec![ProjectAgent {
                name: "alice".to_string(),
                did: Some("did:agora:alice".to_string()),
                role: ProjectRole::Owner,
                joined_at: Utc::now(),
                clocked_in: true,
                current_focus: Some("planning".to_string()),
                last_clock_in: Some(Utc::now()),
                suspended: false,
                suspended_reason: None,
                muted: false,
            }],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            notes: None,
            tasks: Vec::new(),
            audit_trail: Vec::new(),
            current_stage: None,
            rooms: Vec::new(),
        };
        let json = serde_json::to_string(&project).unwrap();
        let parsed: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "Test Project");
        assert_eq!(parsed.agents.len(), 1);
        assert_eq!(parsed.agents[0].role, ProjectRole::Owner);
    }

    #[test]
    fn test_project_store_crud() {
        let dir = std::env::temp_dir().join(format!("agora-proj-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("projects.json");

        let mut store = ProjectStore::load(&path);
        assert!(store.list().is_empty());

        let id = Uuid::new_v4();
        store.add(Project {
            id,
            name: "My Project".to_string(),
            description: None,
            owner_did: "did:agora:test".to_string(),
            owner_name: "alice".to_string(),
            repo: None,
            status: ProjectStatus::Active,
            agents: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            notes: None,
            tasks: Vec::new(),
            audit_trail: Vec::new(),
            current_stage: None,
            rooms: Vec::new(),
        });
        store.save().unwrap();

        let loaded = ProjectStore::load(&path);
        assert_eq!(loaded.list().len(), 1);
        assert_eq!(loaded.get(&id).unwrap().name, "My Project");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_invitation_store_pending() {
        let dir = std::env::temp_dir().join(format!("agora-inv-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("project_invitations.json");

        let mut store = ProjectInvitationStore::load(&path);
        let inv_id = Uuid::new_v4();
        store.add(ProjectInvitation {
            id: inv_id,
            project_id: Uuid::new_v4(),
            project_name: "Test".to_string(),
            peer_name: "bob".to_string(),
            peer_did: None,
            role: ProjectRole::Developer,
            direction: InvitationDirection::Inbound,
            status: InvitationStatus::Pending,
            created_at: Utc::now(),
            resolved_at: None,
            message: Some("Join us!".to_string()),
            context: None,
        });

        assert_eq!(store.pending_inbound().len(), 1);
        store.accept(&inv_id);
        assert_eq!(store.pending_inbound().len(), 0);
        assert_eq!(
            store.get(&inv_id).unwrap().status,
            InvitationStatus::Accepted
        );

        store.save().unwrap();
        let loaded = ProjectInvitationStore::load(&path);
        assert_eq!(loaded.list().len(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_project_context_serialize() {
        let ctx = ProjectContext {
            project_name: "Agora".to_string(),
            repo: Some("https://github.com/test/agora".to_string()),
            description: Some("P2P agent protocol".to_string()),
            your_role: ProjectRole::Developer,
            your_permissions: vec![
                "read".to_string(),
                "write".to_string(),
                "commit".to_string(),
            ],
            current_agents: vec!["alice".to_string()],
            notes: None,
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: ProjectContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.your_role, ProjectRole::Developer);
        assert_eq!(parsed.your_permissions.len(), 3);
    }

    #[test]
    fn test_task_status_roundtrip() {
        let status = TaskStatus::InProgress;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[test]
    fn test_task_serialize_roundtrip() {
        let task = Task {
            id: Uuid::new_v4(),
            title: "Implement feature X".to_string(),
            description: Some("Details here".to_string()),
            status: TaskStatus::Todo,
            assignee: Some("alice".to_string()),
            priority: Some(TaskPriority::High),
            depends_on: vec![Uuid::new_v4()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: Some("bob".to_string()),
            github_issue_number: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        let parsed: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Implement feature X");
        assert_eq!(parsed.status, TaskStatus::Todo);
        assert_eq!(parsed.depends_on.len(), 1);
    }

    #[test]
    fn test_project_with_tasks_backward_compat() {
        // Old projects without tasks/audit_trail/current_stage should deserialize fine
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "name": "Old Project",
            "owner_did": "did:agora:test",
            "owner_name": "alice",
            "status": "active",
            "agents": [],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let parsed: Project = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.name, "Old Project");
        assert!(parsed.tasks.is_empty());
        assert!(parsed.audit_trail.is_empty());
        assert!(parsed.current_stage.is_none());
    }

    #[test]
    fn test_audit_entry_sign_verify() {
        let identity = crate::identity::AgentIdentity::load_or_create(
            &std::env::temp_dir().join(format!("agora-id-test-{}.json", Uuid::new_v4())),
        )
        .unwrap();

        let entry = AuditEntry::new_signed(
            identity.did(),
            "alice",
            "task.created",
            "Created task: Implement X",
            &identity,
        );

        assert!(entry.verify(&identity.public_key_base58()));
        assert!(!entry.verify("invalidkey"));
    }

    #[test]
    fn test_project_stage_sequence() {
        assert_eq!(ProjectStage::Investigation.index(), 0);
        assert_eq!(ProjectStage::Deployment.index(), 4);
        assert_eq!(
            ProjectStage::Investigation.next(),
            Some(ProjectStage::Implementation)
        );
        assert_eq!(ProjectStage::Deployment.next(), None);
    }

    #[test]
    fn test_stage_role_permissions() {
        let perms = ProjectStage::Implementation.role_permissions(&ProjectRole::Developer);
        assert!(perms.contains(&"read"));
        assert!(perms.contains(&"write"));
        assert!(perms.contains(&"commit"));

        let review_dev = ProjectStage::Review.role_permissions(&ProjectRole::Developer);
        assert!(review_dev.contains(&"read"));
        assert!(!review_dev.contains(&"approve"));

        let review_reviewer = ProjectStage::Review.role_permissions(&ProjectRole::Reviewer);
        assert!(review_reviewer.contains(&"approve"));
    }

    #[test]
    fn test_stage_from_str() {
        assert_eq!(
            "investigation".parse::<ProjectStage>().unwrap(),
            ProjectStage::Investigation
        );
        assert_eq!(
            "Deployment".parse::<ProjectStage>().unwrap(),
            ProjectStage::Deployment
        );
        assert!("invalid".parse::<ProjectStage>().is_err());
    }
}
