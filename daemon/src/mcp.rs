// Agora MCP Bridge - connects Claude Code to the Agora daemon
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use rmcp::ServerHandler;
use rmcp::ServiceExt;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::schemars::JsonSchema;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use serde::Deserialize;
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

/// MCP server that bridges Claude Code to the Agora daemon HTTP API.
///
/// Runs as a stdio MCP server — Claude Code launches it as a subprocess.
/// Each tool call translates to an HTTP request to the running daemon.
///
/// A background task polls the daemon for incoming messages and pushes
/// MCP logging notifications to Claude Code automatically.
#[derive(Clone)]
pub struct AgoraMcpServer {
    api_url: String,
    client: reqwest::Client,
    tool_router: ToolRouter<Self>,
    /// Messages consumed by the background monitor, waiting for agora_read_messages.
    pending: Arc<Mutex<VecDeque<serde_json::Value>>>,
    /// Wakes agora_read_messages when new messages arrive in the buffer.
    inbox_notify: Arc<Notify>,
    /// Optional agent name for this MCP consumer (used as consumer label).
    agent_name: Option<String>,
    /// Message IDs already shown as INCOMING (prevents showing same message twice).
    seen_ids: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Consumer ID assigned by the daemon after registration.
    consumer_id: Arc<Mutex<Option<u64>>>,
}

impl AgoraMcpServer {
    pub fn new(api_port: u16, agent_name: Option<String>) -> Self {
        Self {
            api_url: format!("http://127.0.0.1:{}", api_port),
            client: reqwest::Client::new(),
            tool_router: Self::tool_router(),
            pending: Arc::new(Mutex::new(VecDeque::new())),
            inbox_notify: Arc::new(Notify::new()),
            agent_name,
            seen_ids: Arc::new(Mutex::new(std::collections::HashSet::new())),
            consumer_id: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the MCP server on stdio.
    pub async fn run(self) -> anyhow::Result<()> {
        info!("Starting MCP server (API at {})", self.api_url);

        // Clone shared state before serve() consumes self
        let api_url = self.api_url.clone();
        let client = self.client.clone();
        let pending = self.pending.clone();
        let inbox_notify = self.inbox_notify.clone();
        let agent_name = self.agent_name.clone();
        let consumer_id_slot = self.consumer_id.clone();

        let transport = rmcp::transport::stdio();
        let service = self
            .serve(transport)
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {}", e))?;

        // Get peer handle for sending logging notifications
        let peer = service.peer().clone();

        // Consumer label: use agent name if provided, otherwise "mcp-monitor"
        let consumer_label = agent_name.unwrap_or_else(|| "mcp-monitor".to_string());

        // Spawn background inbox monitor
        tokio::spawn(inbox_monitor(
            api_url,
            client,
            pending,
            inbox_notify,
            peer,
            consumer_label,
            consumer_id_slot,
        ));

        // Spawn a parent-process watchdog that exits if our parent dies.
        // On Unix, when parent dies, ppid changes to 1 (init/launchd).
        // We check every 5 seconds by reading our ppid.
        tokio::spawn(async move {
            // Get initial parent pid via /proc or sysctl
            let initial_ppid = std::process::Command::new("ps")
                .args(["-o", "ppid=", "-p", &std::process::id().to_string()])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if initial_ppid == 0 {
                return;
            }
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let current_ppid = std::process::Command::new("ps")
                    .args(["-o", "ppid=", "-p", &std::process::id().to_string()])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                if current_ppid != initial_ppid {
                    tracing::info!(
                        "Parent changed from {} to {} — exiting MCP bridge.",
                        initial_ppid,
                        current_ppid
                    );
                    std::process::exit(0);
                }
            }
        });

        service
            .waiting()
            .await
            .map_err(|e| anyhow::anyhow!("MCP server error: {}", e))?;

        info!("MCP connection closed — parent agent disconnected. Exiting.");
        std::process::exit(0);
    }

    // --- HTTP helpers ---

    async fn get(&self, path: &str) -> Result<String, String> {
        self.client
            .get(format!("{}{}", self.api_url, path))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Read failed: {}", e))
    }

    async fn post_json(&self, path: &str, body: &impl serde::Serialize) -> Result<String, String> {
        self.client
            .post(format!("{}{}", self.api_url, path))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Read failed: {}", e))
    }

    async fn delete(&self, path: &str) -> Result<String, String> {
        self.client
            .delete(format!("{}{}", self.api_url, path))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Read failed: {}", e))
    }

    /// Drain any pending inbox messages and append them to a tool result string.
    /// Messages are removed from the buffer so they appear exactly once.
    async fn append_pending(&self, mut result: String) -> String {
        let messages: Vec<serde_json::Value> = {
            let mut buf = self.pending.lock().await;
            buf.drain(..).collect()
        };
        if !messages.is_empty() {
            let summary = messages
                .iter()
                .filter_map(|m| {
                    let from = m.get("from")?.as_str()?;
                    let body = m.get("body")?.as_str()?;
                    Some(format!(
                        "[{}] {}",
                        from,
                        if body.len() > 100 { &body[..100] } else { body }
                    ))
                })
                .collect::<Vec<_>>()
                .join("\n");
            result.push_str(&format!(
                "\n\n--- {} new message(s) ---\n{}\n\nRespond using agora_send_message.",
                messages.len(),
                summary
            ));
        }
        result
    }

    async fn patch_json(&self, path: &str, body: &impl serde::Serialize) -> Result<String, String> {
        self.client
            .patch(format!("{}{}", self.api_url, path))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Read failed: {}", e))
    }
}

/// Background task that continuously polls the daemon for new messages
/// and pushes MCP logging notifications to Claude Code.
///
/// On startup, registers as a dedicated consumer ("mcp-monitor") so it gets
/// its own copy of every message without racing with other consumers.
/// Falls back to legacy GET /messages if consumer registration fails.
async fn inbox_monitor(
    api_url: String,
    client: reqwest::Client,
    pending: Arc<Mutex<VecDeque<serde_json::Value>>>,
    inbox_notify: Arc<Notify>,
    peer: rmcp::Peer<rmcp::RoleServer>,
    consumer_label: String,
    consumer_id_slot: Arc<Mutex<Option<u64>>>,
) {
    info!(
        "Inbox monitor started — registering consumer '{}' at {}",
        consumer_label, api_url
    );

    // Try to register as a consumer
    let consumer_id = register_as_consumer(&api_url, &client, &consumer_label).await;

    let poll_url = match consumer_id {
        Some(id) => {
            info!("Inbox monitor registered as consumer {}", id);
            // Store the consumer ID so agora_read_messages can use it
            *consumer_id_slot.lock().await = Some(id);
            format!("{}/consumers/{}/messages", api_url, id)
        }
        None => {
            warn!("Inbox monitor: consumer registration failed, falling back to legacy /messages");
            format!("{}/messages", api_url)
        }
    };

    loop {
        let url = format!("{}?wait=true&timeout=30", poll_url);
        match client.get(&url).send().await {
            Ok(resp) => {
                // If we get 404, our consumer was reaped — re-register
                if resp.status() == reqwest::StatusCode::NOT_FOUND && consumer_id.is_some() {
                    warn!("Inbox monitor: consumer was reaped, re-registering...");
                    *consumer_id_slot.lock().await = None;
                    break;
                }
                match resp.text().await {
                    Ok(body) => {
                        if let Ok(messages) = serde_json::from_str::<Vec<serde_json::Value>>(&body)
                        {
                            if !messages.is_empty() {
                                let count = messages.len();
                                let summary = build_summary(&messages);

                                {
                                    let mut buf = pending.lock().await;
                                    for msg in &messages {
                                        // Cap at 200 messages to prevent unbounded growth
                                        if buf.len() >= 200 {
                                            buf.pop_front();
                                        }
                                        buf.push_back(msg.clone());
                                    }
                                }
                                inbox_notify.notify_waiters();

                                let _ = peer
                                    .notify_logging_message(LoggingMessageNotificationParam {
                                        level: LoggingLevel::Info,
                                        logger: Some("agora".to_string()),
                                        data: serde_json::json!({
                                            "type": "incoming_messages",
                                            "count": count,
                                            "summary": summary,
                                            "messages": messages,
                                        }),
                                    })
                                    .await;

                                info!(
                                    "Inbox monitor: {} message(s) buffered, notification sent",
                                    count
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Inbox monitor read error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    // If we broke out (consumer reaped), restart the whole monitor
    info!("Inbox monitor restarting...");
    Box::pin(inbox_monitor(
        api_url,
        client,
        pending,
        inbox_notify,
        peer,
        consumer_label,
        consumer_id_slot,
    ))
    .await;
}

/// Try to register as a consumer with the daemon. Retries a few times
/// since the daemon HTTP API may not be ready yet.
async fn register_as_consumer(api_url: &str, client: &reqwest::Client, label: &str) -> Option<u64> {
    for attempt in 0..15 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        match client
            .post(format!("{}/consumers", api_url))
            .json(&serde_json::json!({"label": label}))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text().await {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(id) = parsed.get("consumer_id").and_then(|v| v.as_u64()) {
                            return Some(id);
                        }
                    }
                }
            }
            Ok(resp) => {
                info!(
                    "Consumer registration attempt {} returned status {}",
                    attempt + 1,
                    resp.status()
                );
            }
            Err(e) => {
                info!(
                    "Consumer registration attempt {} failed: {} (daemon may not be ready)",
                    attempt + 1,
                    e
                );
            }
        }
    }
    None
}

/// Build a human-readable summary of incoming messages for the notification.
fn build_summary(messages: &[serde_json::Value]) -> String {
    let mut parts = Vec::new();
    for msg in messages {
        let from = msg
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let body = msg.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let preview: String = body.chars().take(80).collect();
        parts.push(format!("[{}] {}", from, preview));
    }
    parts.join(" | ")
}

// --- Tool parameter types ---

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadMessagesParams {
    /// Wait for messages (long-poll). If true, holds until a message arrives or timeout.
    #[serde(default)]
    pub wait: Option<bool>,
    /// Max seconds to wait (default 30, max 120). Only used when wait=true.
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct SendMessageParams {
    /// The message text to send.
    pub body: String,
    /// Target peer name. Omit to broadcast to all connected peers.
    #[serde(default)]
    pub to: Option<String>,
    /// Reply to a specific message by its id (UUID).
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Conversation thread id (UUID). Groups related messages together.
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct AddFriendParams {
    /// Name of the agent (must match the node name it connects with).
    pub name: String,
    /// Optional alias for this friend.
    #[serde(default)]
    pub alias: Option<String>,
    /// Trust level 0-4 (0=Unknown, 1=Acquaintance, 2=Friend, 3=Trusted, 4=Inner Circle). Default: 2.
    #[serde(default)]
    pub trust_level: Option<u8>,
    /// Optional notes about this friend.
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveFriendParams {
    /// Name of the friend to remove.
    pub name: String,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct SetWakeParams {
    /// Shell command to run when a message arrives from a trusted peer. Set to null to clear.
    pub command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetConversationParams {
    /// Conversation ID to fetch history for.
    pub conversation_id: String,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct SendFriendRequestParams {
    /// Name of the peer to send a friend request to.
    pub name: String,
    /// Trust level to offer (0-4). Default: 2.
    #[serde(default)]
    pub trust_level: Option<u8>,
    /// Optional message to include with the request.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct RespondFriendRequestParams {
    /// The request ID (UUID) to respond to.
    pub request_id: String,
    /// Action: "accept" or "reject".
    pub action: String,
    /// Trust level to assign (only for accept, 0-4). Default: 2.
    #[serde(default)]
    pub trust_level: Option<u8>,
    /// Optional message or reason.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct CreateProjectParams {
    /// Project name.
    pub name: String,
    /// Optional project description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional repository URL.
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct InviteToProjectParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// Name of the peer to invite.
    pub peer_name: String,
    /// Role: owner, overseer, developer, reviewer, consultant, observer, tester.
    pub role: String,
    /// Optional message to include with the invitation.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RespondProjectInvitationParams {
    /// Invitation ID (UUID).
    pub invitation_id: String,
    /// Action: "accept" or "decline".
    pub action: String,
    /// Optional reason (for decline).
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectClockParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// Action: "clock_in" or "clock_out".
    pub action: String,
    /// What you're working on (only for clock_in).
    #[serde(default)]
    pub focus: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectTasksParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// Action: "list", "create", "update", "assign", "complete", "delete".
    pub action: String,
    /// Task ID (for update/assign/complete/delete actions).
    #[serde(default)]
    pub task_id: Option<String>,
    /// Task title (for create).
    #[serde(default)]
    pub title: Option<String>,
    /// Task description (for create/update).
    #[serde(default)]
    pub description: Option<String>,
    /// Assignee name (for create/assign/update).
    #[serde(default)]
    pub assignee: Option<String>,
    /// Priority: low, medium, high, critical (for create).
    #[serde(default)]
    pub priority: Option<String>,
    /// New status: todo, in_progress, done, blocked (for update/complete).
    #[serde(default)]
    pub status: Option<String>,
    /// Task IDs this task depends on (for create, comma-separated UUIDs).
    #[serde(default)]
    pub depends_on: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectAuditParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// Action: "list" or "add".
    pub action: String,
    /// Action type string (for add, e.g. "task.created").
    #[serde(default)]
    pub audit_action: Option<String>,
    /// Detail text (for add).
    #[serde(default)]
    pub detail: Option<String>,
    /// Pagination offset (for list).
    #[serde(default)]
    pub offset: Option<usize>,
    /// Pagination limit (for list).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectStageParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// Action: "get", "set", or "advance".
    pub action: String,
    /// Stage name (for set): investigation, implementation, review, integration, deployment.
    #[serde(default)]
    pub stage: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectOversightParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// Action: "suspend" or "unsuspend".
    pub action: String,
    /// Name of the agent to suspend/unsuspend.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Reason for suspension (optional, for suspend only).
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectConversationsParams {
    /// Project ID (UUID).
    pub project_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GitHubSyncParams {
    /// Project ID (UUID).
    pub project_id: String,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct GitHubConfigParams {
    /// GitHub personal access token (ghp_...). Set to null to clear.
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize, JsonSchema)]
pub struct SendToRoomParams {
    /// Project ID (UUID).
    pub project_id: String,
    /// The message text to send.
    pub body: String,
    /// Room name: "main" or a specific room ID (UUID). Default: "main".
    #[serde(default)]
    pub room: Option<String>,
}

// --- Tool result helpers ---

fn ok(text: String) -> Result<CallToolResult, rmcp::ErrorData> {
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

fn err(text: String) -> Result<CallToolResult, rmcp::ErrorData> {
    let mut result = CallToolResult::success(vec![Content::text(format!("Error: {}", text))]);
    result.is_error = Some(true);
    Ok(result)
}

// --- MCP tool definitions ---

#[tool_router]
impl AgoraMcpServer {
    #[tool(
        description = "Get Agora daemon status and your agent identity. Also shows any pending incoming messages."
    )]
    async fn agora_status(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/status").await {
            Ok(body) => {
                let agent_name = self.agent_name.as_deref().unwrap_or("(default)");
                let header = format!(
                    "YOUR AGENT NAME: {}\n(This is YOUR identity on the Agora network. Messages you send will show from: {})\n\n",
                    agent_name, agent_name
                );
                let mut result = format!("{}{}", header, body);

                // Check ALL conversations with recent messages for unreplied messages
                if let Ok(convos) = self.get("/conversations").await {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&convos) {
                        if let Some(list) = parsed.get("conversations").and_then(|c| c.as_array()) {
                            let mut unreplied = Vec::new();
                            // Check all conversations (not just top 5 — threads need this)
                            for conv in list.iter() {
                                let cid = match conv.get("conversation_id").and_then(|c| c.as_str())
                                {
                                    Some(c) => c,
                                    None => continue,
                                };
                                if let Ok(detail) =
                                    self.get(&format!("/conversations/{}", cid)).await
                                {
                                    if let Ok(d) =
                                        serde_json::from_str::<serde_json::Value>(&detail)
                                    {
                                        if let Some(msgs) =
                                            d.get("messages").and_then(|m| m.as_array())
                                        {
                                            // Check last message per conversation for unreplied
                                            let mut seen = self.seen_ids.lock().await;
                                            for msg in msgs.iter().rev().take(2) {
                                                let from = msg
                                                    .get("from")
                                                    .and_then(|f| f.as_str())
                                                    .unwrap_or("");
                                                let body_text = msg
                                                    .get("body")
                                                    .and_then(|b| b.as_str())
                                                    .unwrap_or("");
                                                let msg_id = msg
                                                    .get("id")
                                                    .and_then(|i| i.as_str())
                                                    .unwrap_or("");
                                                // Skip already-seen messages
                                                if seen.contains(msg_id) { continue; }
                                                if from != agent_name && !body_text.is_empty() {
                                                    let replied = msgs.iter().any(|m2| {
                                                        m2.get("from").and_then(|f| f.as_str())
                                                            == Some(agent_name)
                                                            && m2
                                                                .get("timestamp")
                                                                .and_then(|t| t.as_str())
                                                                .unwrap_or("")
                                                                > msg
                                                                    .get("timestamp")
                                                                    .and_then(|t| t.as_str())
                                                                    .unwrap_or("")
                                                    });
                                                    if !replied {
                                                        // Mark as seen so we don't show it again
                                                        if !msg_id.is_empty() {
                                                            seen.insert(msg_id.to_string());
                                                        }
                                                        unreplied.push(format!(
                                                            "[{}] {}",
                                                            from,
                                                            &body_text[..body_text.len().min(100)]
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            unreplied.dedup();
                            if !unreplied.is_empty() {
                                result.push_str(&format!(
                                    "\n\n--- INCOMING: {} unreplied message(s) ---\n{}\n\nACTION REQUIRED: Reply using agora_send_to_room or agora_send_message.",
                                    unreplied.len(), unreplied.join("\n")
                                ));
                            }
                        }
                    }
                }

                ok(result)
            }
            Err(e) => err(e),
        }
    }

    #[tool(description = "Get this agent's cryptographic identity (DID, public key, session ID)")]
    async fn agora_identity(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/identity").await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "List all connected peers with names and addresses. Also shows any pending incoming messages."
    )]
    async fn agora_list_peers(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/peers").await {
            Ok(body) => ok(self.append_pending(body).await),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Read incoming messages from remote peers. Messages are automatically consumed by a background monitor — this drains the local buffer. Use wait=true to block until new messages arrive."
    )]
    async fn agora_read_messages(
        &self,
        Parameters(params): Parameters<ReadMessagesParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // Drain local pending buffer first
        let mut messages: Vec<serde_json::Value> = {
            let mut buf = self.pending.lock().await;
            buf.drain(..).collect()
        };

        // If local buffer was empty, try draining our own consumer directly
        if messages.is_empty() {
            if let Some(cid) = *self.consumer_id.lock().await {
                if let Ok(msgs_body) = self.get(&format!("/consumers/{}/messages", cid)).await {
                    if let Ok(msgs) = serde_json::from_str::<Vec<serde_json::Value>>(&msgs_body) {
                        messages = msgs;
                    }
                }
            }
        }

        if !messages.is_empty() {
            return ok(serde_json::to_string(&messages).unwrap_or_else(|_| "[]".to_string()));
        }

        // If wait=true and buffer is empty, wait for background monitor to deliver
        if let Some(true) = params.wait {
            let timeout_secs = params.timeout.unwrap_or(30).min(120);
            match tokio::time::timeout(
                Duration::from_secs(timeout_secs),
                self.inbox_notify.notified(),
            )
            .await
            {
                Ok(()) => {
                    // Notification received — drain buffer
                    let messages: Vec<serde_json::Value> = {
                        let mut buf = self.pending.lock().await;
                        buf.drain(..).collect()
                    };
                    ok(serde_json::to_string(&messages).unwrap_or_else(|_| "[]".to_string()))
                }
                Err(_) => ok("[]".to_string()), // Timeout
            }
        } else {
            ok("[]".to_string())
        }
    }

    #[tool(description = "Send a message to connected peers. Omit 'to' to broadcast to all peers.")]
    async fn agora_send_message(
        &self,
        Parameters(params): Parameters<SendMessageParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        // Inject agent_name as "from" so messages identify the correct sender on shared daemons
        let mut payload = serde_json::to_value(&params).unwrap_or_default();
        if let Some(ref name) = self.agent_name {
            payload["from"] = serde_json::Value::String(name.clone());
        }
        match self.post_json("/send", &payload).await {
            Ok(body) => ok(self.append_pending(body).await),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "List all friends with trust levels and metadata. Also shows any pending incoming messages."
    )]
    async fn agora_list_friends(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/friends").await {
            Ok(body) => ok(self.append_pending(body).await),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Add a friend. Trust levels: 0=Unknown, 1=Acquaintance, 2=Friend, 3=Trusted (can wake), 4=Inner Circle (can wake)."
    )]
    async fn agora_add_friend(
        &self,
        Parameters(params): Parameters<AddFriendParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.post_json("/friends", &params).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(description = "Remove a friend by name")]
    async fn agora_remove_friend(
        &self,
        Parameters(params): Parameters<RemoveFriendParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.delete(&format!("/friends/{}", params.name)).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(description = "Get the current wake-up hook command")]
    async fn agora_get_wake(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/wake").await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Set or clear the wake-up hook. This shell command runs when messages arrive from trusted peers (trust >= 3)."
    )]
    async fn agora_set_wake(
        &self,
        Parameters(params): Parameters<SetWakeParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.post_json("/wake", &params).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Get the message history for a conversation thread by its conversation_id"
    )]
    async fn agora_get_conversation(
        &self,
        Parameters(params): Parameters<GetConversationParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self
            .get(&format!("/conversations/{}", params.conversation_id))
            .await
        {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "List friend requests (pending inbound/outbound). Returns request IDs needed for accept/reject."
    )]
    async fn agora_friend_requests(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/friend-requests?status=pending").await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Send a friend request to a connected peer. Trust levels: 0=Unknown, 1=Acquaintance, 2=Friend, 3=Trusted, 4=Inner Circle."
    )]
    async fn agora_send_friend_request(
        &self,
        Parameters(params): Parameters<SendFriendRequestParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let body = serde_json::json!({
            "peer_name": params.name,
            "trust_level": params.trust_level,
            "message": params.message,
        });
        match self.post_json("/friend-requests", &body).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(description = "List all projects with agent counts and status")]
    async fn agora_projects(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.get("/projects").await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(description = "Create a new collaboration project. Returns the project ID.")]
    async fn agora_create_project(
        &self,
        Parameters(params): Parameters<CreateProjectParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.post_json("/projects", &params).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Invite a peer to a project. Role: owner, overseer, developer, reviewer, consultant, observer, tester."
    )]
    async fn agora_invite_to_project(
        &self,
        Parameters(params): Parameters<InviteToProjectParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self.post_json("/project-invitations", &params).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(description = "Accept or decline a project invitation. Action: 'accept' or 'decline'.")]
    async fn agora_respond_project_invitation(
        &self,
        Parameters(params): Parameters<RespondProjectInvitationParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match params.action.as_str() {
            "accept" => {
                match self
                    .post_json(
                        &format!("/project-invitations/{}/accept", params.invitation_id),
                        &serde_json::json!({}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "decline" => {
                let body = serde_json::json!({ "reason": params.reason });
                match self
                    .post_json(
                        &format!("/project-invitations/{}/decline", params.invitation_id),
                        &body,
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!(
                "Invalid action '{}'. Must be 'accept' or 'decline'.",
                other
            )),
        }
    }

    #[tool(
        description = "Clock in/out of a project. Action: 'clock_in' or 'clock_out'. For clock_in, optionally specify focus (what you're working on)."
    )]
    async fn agora_project_clock(
        &self,
        Parameters(params): Parameters<ProjectClockParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match params.action.as_str() {
            "clock_in" => {
                let body = serde_json::json!({ "focus": params.focus });
                match self
                    .post_json(&format!("/projects/{}/clock-in", params.project_id), &body)
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "clock_out" => {
                match self
                    .post_json(
                        &format!("/projects/{}/clock-out", params.project_id),
                        &serde_json::json!({}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!(
                "Invalid action '{}'. Must be 'clock_in' or 'clock_out'.",
                other
            )),
        }
    }

    #[tool(
        description = "Send a message to a project room. Messages sent here appear in the project conversation and are visible to all project members. Use this instead of agora_send_message when communicating within a project (standups, code review, announcements)."
    )]
    async fn agora_send_to_room(
        &self,
        Parameters(params): Parameters<SendToRoomParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let room = params.room.as_deref().unwrap_or("main");
        let url = if room == "main" {
            format!("/projects/{}/rooms/main/send", params.project_id)
        } else {
            format!("/projects/{}/rooms/{}/send", params.project_id, room)
        };
        // Inject agent_name as "from" so the room message is attributed correctly
        let mut payload = serde_json::json!({ "body": params.body });
        if let Some(ref name) = self.agent_name {
            payload["from"] = serde_json::Value::String(name.clone());
        }
        match self.post_json(&url, &payload).await {
            Ok(body) => ok(self.append_pending(body).await),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Manage tasks in a project. Actions: 'list' (all tasks), 'create' (title required), 'update' (task_id + status/assignee/description), 'assign' (task_id + assignee), 'complete' (task_id → done), 'delete' (task_id)."
    )]
    async fn agora_project_tasks(
        &self,
        Parameters(params): Parameters<ProjectTasksParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let pid = &params.project_id;
        match params.action.as_str() {
            "list" => match self.get(&format!("/projects/{}/tasks", pid)).await {
                Ok(body) => ok(body),
                Err(e) => err(e),
            },
            "create" => {
                let title = params.title.unwrap_or_default();
                if title.is_empty() {
                    return err("title is required for create".to_string());
                }
                let depends: Vec<String> = params
                    .depends_on
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let body = serde_json::json!({
                    "title": title,
                    "description": params.description,
                    "assignee": params.assignee,
                    "priority": params.priority,
                    "depends_on": depends,
                });
                match self
                    .post_json(&format!("/projects/{}/tasks", pid), &body)
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "update" => {
                let tid = params.task_id.unwrap_or_default();
                if tid.is_empty() {
                    return err("task_id is required for update".to_string());
                }
                let body = serde_json::json!({
                    "status": params.status,
                    "title": params.title,
                    "description": params.description,
                    "assignee": params.assignee,
                });
                match self
                    .patch_json(&format!("/projects/{}/tasks/{}", pid, tid), &body)
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "assign" => {
                let tid = params.task_id.unwrap_or_default();
                let assignee = params.assignee.unwrap_or_default();
                if tid.is_empty() || assignee.is_empty() {
                    return err("task_id and assignee required for assign".to_string());
                }
                match self
                    .post_json(
                        &format!("/projects/{}/tasks/{}/assign", pid, tid),
                        &serde_json::json!({"assignee": assignee}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "complete" => {
                let tid = params.task_id.unwrap_or_default();
                if tid.is_empty() {
                    return err("task_id is required for complete".to_string());
                }
                match self
                    .patch_json(
                        &format!("/projects/{}/tasks/{}", pid, tid),
                        &serde_json::json!({"status": "done"}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "delete" => {
                let tid = params.task_id.unwrap_or_default();
                if tid.is_empty() {
                    return err("task_id is required for delete".to_string());
                }
                match self
                    .delete(&format!("/projects/{}/tasks/{}", pid, tid))
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!(
                "Unknown action '{}'. Use list/create/update/assign/complete/delete.",
                other
            )),
        }
    }

    #[tool(
        description = "View or add to a project's audit trail. Actions: 'list' (with optional offset/limit), 'add' (audit_action + detail)."
    )]
    async fn agora_project_audit(
        &self,
        Parameters(params): Parameters<ProjectAuditParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let pid = &params.project_id;
        match params.action.as_str() {
            "list" => {
                let offset = params.offset.unwrap_or(0);
                let limit = params.limit.unwrap_or(100);
                match self
                    .get(&format!(
                        "/projects/{}/audit?offset={}&limit={}",
                        pid, offset, limit
                    ))
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "add" => {
                let action = params.audit_action.unwrap_or_default();
                let detail = params.detail.unwrap_or_default();
                if action.is_empty() || detail.is_empty() {
                    return err("audit_action and detail required for add".to_string());
                }
                match self
                    .post_json(
                        &format!("/projects/{}/audit", pid),
                        &serde_json::json!({"action": action, "detail": detail}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!("Unknown action '{}'. Use list or add.", other)),
        }
    }

    #[tool(
        description = "Get or change a project's lifecycle stage. Actions: 'get' (current stage info), 'set' (stage name), 'advance' (move to next stage). Stages: investigation → implementation → review → integration → deployment."
    )]
    async fn agora_project_stage(
        &self,
        Parameters(params): Parameters<ProjectStageParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let pid = &params.project_id;
        match params.action.as_str() {
            "get" => match self.get(&format!("/projects/{}/stage", pid)).await {
                Ok(body) => ok(body),
                Err(e) => err(e),
            },
            "set" => {
                let stage = params.stage.unwrap_or_default();
                if stage.is_empty() {
                    return err("stage is required for set".to_string());
                }
                match self
                    .post_json(
                        &format!("/projects/{}/stage", pid),
                        &serde_json::json!({"stage": stage}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "advance" => {
                match self
                    .post_json(
                        &format!("/projects/{}/stage", pid),
                        &serde_json::json!({"advance": true}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!(
                "Unknown action '{}'. Use get, set, or advance.",
                other
            )),
        }
    }

    #[tool(
        description = "Accept or reject a friend request. Action must be 'accept' or 'reject'. For accept, specify trust_level (0-4, default 2)."
    )]
    async fn agora_respond_friend_request(
        &self,
        Parameters(params): Parameters<RespondFriendRequestParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match params.action.as_str() {
            "accept" => {
                let body = serde_json::json!({
                    "trust_level": params.trust_level,
                    "message": params.message,
                });
                match self
                    .post_json(
                        &format!("/friend-requests/{}/accept", params.request_id),
                        &body,
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "reject" => {
                let body = serde_json::json!({
                    "reason": params.message,
                });
                match self
                    .post_json(
                        &format!("/friend-requests/{}/reject", params.request_id),
                        &body,
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!(
                "Invalid action '{}'. Must be 'accept' or 'reject'.",
                other
            )),
        }
    }

    #[tool(
        description = "Overseer controls for human oversight of agents. Actions: 'suspend' (suspend an agent — blocks all their actions), 'unsuspend' (restore an agent's access). Requires 'coordinate' permission (Owner/Overseer role)."
    )]
    async fn agora_project_oversight(
        &self,
        Parameters(params): Parameters<ProjectOversightParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let pid = &params.project_id;
        match params.action.as_str() {
            "suspend" => {
                let agent = params.agent_name.as_deref().unwrap_or("");
                if agent.is_empty() {
                    return err("agent_name is required for suspend".to_string());
                }
                let body = serde_json::json!({ "reason": params.reason });
                match self
                    .post_json(
                        &format!("/projects/{}/agents/{}/suspend", pid, agent),
                        &body,
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            "unsuspend" => {
                let agent = params.agent_name.as_deref().unwrap_or("");
                if agent.is_empty() {
                    return err("agent_name is required for unsuspend".to_string());
                }
                match self
                    .post_json(
                        &format!("/projects/{}/agents/{}/unsuspend", pid, agent),
                        &serde_json::json!({}),
                    )
                    .await
                {
                    Ok(body) => ok(body),
                    Err(e) => err(e),
                }
            }
            other => err(format!(
                "Unknown action '{}'. Use suspend or unsuspend.",
                other
            )),
        }
    }

    #[tool(
        description = "Get the conversation history for a project. Returns all messages related to project operations (task updates, stage changes, clock in/out, etc.)."
    )]
    async fn agora_project_conversations(
        &self,
        Parameters(params): Parameters<ProjectConversationsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let url = format!("/projects/{}/conversations", params.project_id);
        match self.get(&url).await {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Sync a project's tasks with GitHub issues. Imports new issues as tasks and pushes local tasks as issues. Requires GitHub token (use agora_github_config to set it) and project must have a GitHub repo URL."
    )]
    async fn agora_github_sync(
        &self,
        Parameters(params): Parameters<GitHubSyncParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        match self
            .post_json(
                &format!("/projects/{}/github/sync", params.project_id),
                &serde_json::json!({}),
            )
            .await
        {
            Ok(body) => ok(body),
            Err(e) => err(e),
        }
    }

    #[tool(
        description = "Get or set GitHub configuration (personal access token). To set a token, provide the token parameter. To check status, call with token=null."
    )]
    async fn agora_github_config(
        &self,
        Parameters(params): Parameters<GitHubConfigParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        if params.token.is_some() {
            match self.post_json("/github/config", &params).await {
                Ok(body) => ok(body),
                Err(e) => err(e),
            }
        } else {
            match self.get("/github/config").await {
                Ok(body) => ok(body),
                Err(e) => err(e),
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for AgoraMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Agora Protocol — peer-to-peer AI agent collaboration.\n\n\
                 You are connected to the Agora network. Other AI agents on remote \
                 machines may send you messages at any time.\n\n\
                 MANDATORY COLLABORATION POLICIES (violations = immediate MUTE):\n\
                 1. SINGLE WRITER: Only ONE agent edits a file at a time. Post \
                    'LOCK: <filepath>' in #main room before editing. 'UNLOCK' when done.\n\
                 2. REVIEW GATE: No code ships without Reviewer approval in #code-review.\n\
                 3. NO PARALLEL EDITS: One writes, one reviews. Never both write same file.\n\
                 4. TASK OWNERSHIP: in_progress = exclusive. Announce in #main what you work on.\n\
                 5. ANNOUNCE BEFORE ACTING: Post intent in #main, wait for objections.\n\
                 6. ROOMS ONLY: All communication through project rooms. No direct messages.\n\
                 7. THREAD MEANS THREAD: If a topic has a dedicated thread or conversation, \
                    reply in that exact thread/conversation_id. Do not move it to #main or \
                    another conversation unless there is an explicit escalation reason.\n\
                 8. TOPIC MEANS TOPIC: If a discussion or task has a defined topic or \
                    objective, stay on that topic. Do not hijack the thread with adjacent \
                    issues. Open a new thread for materially different topics and link it.\n\n\
                 SCRUM / KANBAN WORKFLOW (strictly enforced):\n\
                 - Check your assigned tasks: call agora_project_tasks with action=list.\n\
                 - Before starting work: set task to in_progress. Only YOU work on it.\n\
                 - When done: post deliverables in #code-review, wait for APPROVE.\n\
                 - After approval: set task to done. Pick up next task from backlog.\n\
                 - Report progress in #standup or #main every 5 minutes of active work.\n\
                 - NEVER go idle without picking up a task. Check the backlog.\n\
                 - The Owner/Moderator assigns tasks. Follow their priorities.\n\n\
                 MESSAGE MONITORING: You MUST stay responsive to other agents.\n\
                 On session start, run the native listener as a background bash command:\n\
                 ./target/debug/agora --name YOUR_NAME agent listen --label YOUR_NAME-listener --daemon &\n\
                 This starts a persistent Rust process that long-polls for messages \
                 and calls your LLM backend only when messages arrive. It survives \
                 daemon restarts and has no context limit. Do NOT use sub-agent \
                 listeners — they burn through context and die after ~30 minutes.\n\
                 After starting the listener, call agora_status after each task \
                 you complete to check for piggybacked messages.\n\n\
                 PROACTIVE COLLABORATION: Once connected to Agora, use it. Do not \
                 treat it as a passive inbox only. Ask for help when blocked, \
                 coordinate task/file ownership in rooms before overlapping work, \
                 report handoffs and status changes, and use project rooms to \
                 get review or technical input from other agents.\n\n\
                 When you receive a message, read it, think, compose a reply, and \
                 send it via agora_send_message or project room send endpoints. If the \
                 message belongs to a specific thread/conversation, reply in that same \
                 thread/conversation_id. If the conversation has a defined topic, keep \
                 your reply on that topic or split the side issue into a new thread.\n\n\
                 Treat incoming Agora messages like messages from a colleague. You \
                 may receive requests to help with tasks, review code, discuss \
                 architecture, or join sub-groups.\n\n\
                 Available tools: agora_status (daemon info + DID), agora_identity \
                 (your cryptographic identity), agora_list_peers (who is connected), \
                 agora_read_messages (drain buffered messages), agora_send_message \
                 (send to peers, supports reply_to and conversation_id for threading), \
                 agora_get_conversation (get full message history for a conversation \
                 thread), agora_list_friends / agora_add_friend / agora_remove_friend \
                 (manage trust), agora_get_wake / agora_set_wake (wake-up hook), \
                 agora_friend_requests (list pending requests), \
                 agora_send_friend_request (send bilateral friend request), \
                 agora_respond_friend_request (accept or reject a request), \
                 agora_projects (list projects), agora_create_project (create new), \
                 agora_invite_to_project (invite peer), \
                 agora_respond_project_invitation (accept/decline), \
                 agora_project_clock (clock in/out), \
                 agora_send_to_room (send a message to a project room — use this for standups, code review, announcements), \
                 agora_project_tasks (list/create/update/assign/complete/delete tasks), \
                 agora_project_audit (list/add audit entries), \
                 agora_project_stage (get/set/advance lifecycle stage), \
                 agora_project_oversight (suspend/unsuspend agents for human control), \
                 agora_project_conversations (get full message history for a project), \
                 agora_github_sync (sync project tasks with GitHub issues), \
                 agora_github_config (get/set GitHub personal access token)."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_logging()
                .build(),
            ..Default::default()
        }
    }
}
