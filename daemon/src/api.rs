use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::auth::ApiToken;
use crate::dashboard;

use uuid::Uuid;

use crate::project::{
    InvitationDirection, InvitationStatus, ProjectInvitation, ProjectRole, ProjectStatus,
};
use crate::state::{
    ConsumerId, DaemonState, Friend, FriendPatch, FriendRequest, FriendRequestStatus,
    OutboundMessage, TrustLevel,
};
use crate::thread::ThreadSummary;

/// Build the core API routes (without middleware layers).
fn api_routes() -> Router<DaemonState> {
    Router::new()
        .route("/messages", get(get_messages).post(ack_messages))
        .route("/send", post(send_message))
        .route("/peers", get(get_peers))
        .route("/status", get(get_status))
        .route("/wake", get(get_wake).post(set_wake))
        .route("/consumers", get(list_consumers).post(register_consumer))
        .route("/consumers/{id}/messages", get(get_consumer_messages))
        .route("/consumers/{id}/touch", post(touch_consumer))
        .route("/consumers/{id}", delete(unregister_consumer))
        .route("/friends", get(list_friends).post(add_friend))
        .route(
            "/friends/{name}",
            delete(remove_friend).patch(update_friend),
        )
        .route("/peers/{name}/disconnect", post(disconnect_peer))
        .route("/conversations", get(list_conversations))
        .route(
            "/conversations/{id}",
            get(get_conversation).delete(delete_conversation),
        )
        .route(
            "/conversations/{id}/messages/{msg_id}",
            delete(delete_message),
        )
        .route("/threads", get(list_threads).post(create_thread))
        .route("/threads/{id}", get(get_thread).delete(close_thread))
        .route("/threads/{id}/participants", post(thread_add_participant))
        .route(
            "/threads/{id}/participants/{name}",
            delete(thread_remove_participant),
        )
        .route("/health", get(health))
        .route("/identity", get(get_identity))
        .route("/connect", post(connect_to_peer))
        .route(
            "/friend-requests",
            get(list_friend_requests).post(send_friend_request),
        )
        .route("/friend-requests/{id}/accept", post(accept_friend_request))
        .route("/friend-requests/{id}/reject", post(reject_friend_request))
        .route("/projects", get(list_projects).post(create_project))
        .route(
            "/projects/{id}",
            get(get_project)
                .patch(update_project)
                .delete(archive_project),
        )
        .route("/projects/{id}/clock-in", post(project_clock_in))
        .route("/projects/{id}/clock-out", post(project_clock_out))
        .route(
            "/project-invitations",
            get(list_project_invitations).post(send_project_invitation),
        )
        .route(
            "/project-invitations/{id}/accept",
            post(accept_project_invitation),
        )
        .route(
            "/project-invitations/{id}/decline",
            post(decline_project_invitation),
        )
        // Task board
        .route("/projects/{id}/tasks", get(list_tasks).post(create_task))
        .route(
            "/projects/{id}/tasks/{task_id}",
            get(get_task).patch(update_task).delete(delete_task),
        )
        .route("/projects/{id}/tasks/{task_id}/assign", post(assign_task))
        // Project conversations
        .route(
            "/projects/{id}/conversations",
            get(get_project_conversations),
        )
        // Audit trail
        .route("/projects/{id}/audit", get(list_audit).post(add_audit))
        // Stage management
        .route("/projects/{id}/stage", get(get_stage).post(set_stage))
        // Agent management
        .route("/projects/{id}/agents", post(add_project_agent))
        // Agent oversight
        .route("/projects/{id}/agents/{name}/suspend", post(suspend_agent))
        .route(
            "/projects/{id}/agents/{name}/unsuspend",
            post(unsuspend_agent),
        )
        .route("/projects/{id}/agents/{name}/role", post(set_agent_role))
        .route("/projects/{id}/agents/{name}", delete(remove_project_agent))
        // Project rooms
        .route(
            "/projects/{id}/rooms",
            get(list_project_rooms).post(create_project_room),
        )
        .route("/projects/{id}/rooms/{room_id}/send", post(send_to_room))
        .route("/projects/{id}/rooms/main/send", post(send_to_main_room))
        // Agent mute/unmute
        .route("/projects/{id}/agents/{name}/mute", post(mute_agent))
        .route("/projects/{id}/agents/{name}/unmute", post(unmute_agent))
        // GitHub integration
        .route("/projects/{id}/github/sync", post(github_sync))
        .route("/projects/{id}/github/status", get(github_status))
        .route(
            "/github/config",
            get(get_github_config).post(set_github_config),
        )
        // Outbox stats
        .route("/outbox", get(get_outbox_stats))
        // Marketplace
        .route("/marketplace/search", get(marketplace_search))
        .route("/marketplace/advertise", post(marketplace_advertise))
        .route("/marketplace/agents", get(marketplace_list))
        // Discovery (gossip network)
        .route("/discovery/agents", get(discovery_list_agents))
        .route("/discovery/search", get(discovery_search))
        .route("/discovery/agent/{did}", get(discovery_get_agent))
        .route("/discovery/projects", get(discovery_list_project_ads))
        .route("/discovery/stats", get(discovery_stats))
        // Reputation
        .route("/friends/{name}/reputation", get(get_reputation))
        .route("/reputation/leaderboard", get(reputation_leaderboard))
        .route(
            "/reputation/recommendations",
            get(reputation_recommendations),
        )
        // Coordinator
        .route(
            "/projects/{id}/coordinator/suggestions",
            get(coordinator_suggestions),
        )
        .route("/projects/{id}/coordinator/act", post(coordinator_act))
        .route(
            "/projects/{id}/coordinator/digest",
            post(coordinator_digest),
        )
        .route(
            "/projects/{id}/coordinator/digests",
            get(coordinator_digests),
        )
        .route("/projects/{id}/coordinator/status", get(coordinator_status))
        // Auth
        .route("/auth/verify", post(verify_token))
        .route("/auth/token", get(get_api_token))
}

/// Build the local HTTP API router with auth, dashboard, and /api prefix.
/// `loopback_only` should be true when the daemon binds to 127.0.0.1 (default).
pub fn router(state: DaemonState, loopback_only: bool) -> Router {
    let api_token = ApiToken(state.api_token().to_string());

    // Core API routes — served at both root and /api prefix
    let api = api_routes();

    Router::new()
        // Dashboard static files
        .route("/", get(dashboard::serve_index))
        .route("/assets/{*path}", get(dashboard::serve_dashboard))
        // API at root (for CLI, MCP, direct access)
        .merge(api.clone())
        // API at /api prefix (for dashboard frontend)
        .nest("/api", api)
        .with_state(state)
        .layer(axum::Extension(api_token))
        .layer(axum::Extension(crate::auth::LoopbackOnly(loopback_only)))
        .layer(axum::middleware::from_fn(crate::auth::auth_middleware))
}

/// POST /auth/verify — validates a bearer token. Returns 200 if valid, 401 if not.
async fn verify_token(
    State(state): State<DaemonState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let expected = state.api_token();
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match provided {
        Some(token) if token == expected => {
            (StatusCode::OK, Json(serde_json::json!({"valid": true})))
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"valid": false})),
        ),
    }
}

/// GET /auth/token — returns the current API token (localhost only).
async fn get_api_token(State(state): State<DaemonState>) -> impl IntoResponse {
    Json(serde_json::json!({ "token": state.api_token() }))
}

/// GET /messages — read incoming messages from remote peers.
/// Query params:
///   ?wait=true  — long-poll: hold connection until a message arrives (up to ?timeout=30 seconds)
///   ?timeout=N  — max seconds to wait (default 30, max 120). Only used with wait=true.
///   ?peek=true  — return messages without removing them (use POST /messages to ack)
async fn get_messages(
    State(state): State<DaemonState>,
    Query(params): Query<MessagesQuery>,
) -> Json<Vec<InboxMessage>> {
    let peek = params.peek.unwrap_or(false);
    let messages = if peek {
        state.peek_inbox().await
    } else if params.wait.unwrap_or(false) {
        let secs = params.timeout.unwrap_or(30).min(120);
        state.wait_for_inbox(Duration::from_secs(secs)).await
    } else {
        state.drain_inbox().await
    };
    let out: Vec<InboxMessage> = messages
        .into_iter()
        .map(|m| InboxMessage {
            id: m.id.to_string(),
            from: m.from,
            body: m.body,
            timestamp: m.timestamp.to_rfc3339(),
            reply_to: m.reply_to.map(|u| u.to_string()),
            conversation_id: m.conversation_id.map(|u| u.to_string()),
        })
        .collect();
    Json(out)
}

/// POST /messages — acknowledge (remove) specific messages by ID.
/// Body: { "ids": ["uuid1", "uuid2", ...] }
async fn ack_messages(
    State(state): State<DaemonState>,
    Json(req): Json<AckRequest>,
) -> Json<AckResponse> {
    let ids: Vec<Uuid> = req
        .ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();
    let acked = state.ack_inbox(&ids).await;
    Json(AckResponse { acked })
}

#[derive(Deserialize)]
struct AckRequest {
    ids: Vec<String>,
}

#[derive(Serialize)]
struct AckResponse {
    acked: usize,
}

/// POST /send — queue a message to send to remote peers.
async fn send_message(
    State(state): State<DaemonState>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate message body size (max 1 MB)
    const MAX_BODY_SIZE: usize = 1_048_576;
    if req.body.len() > MAX_BODY_SIZE {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse {
                error: format!(
                    "Message body too large ({} bytes, max {})",
                    req.body.len(),
                    MAX_BODY_SIZE
                ),
            }),
        ));
    }
    if req.body.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Message body cannot be empty".to_string(),
            }),
        ));
    }

    // Validate from_override: only allow the daemon's own node name exactly,
    // or a registered consumer label. Reject arbitrary sender impersonation.
    if let Some(ref from) = req.from {
        if !can_send_as(&state, from).await {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: format!(
                        "Cannot send as '{}' — from_override must be this daemon's name, \
                         a registered consumer label, or the base name of a '<name>-...' child consumer",
                        from
                    ),
                }),
            ));
        }
    }

    let id = Uuid::new_v4();
    let reply_to = req.reply_to.and_then(|s| Uuid::parse_str(&s).ok());
    // Use explicit conversation_id if provided, otherwise auto-assign for 1:1 DMs
    let conversation_id = req
        .conversation_id
        .and_then(|s| Uuid::parse_str(&s).ok())
        .or_else(|| req.to.as_ref().map(|to| state.peer_conversation_id(to)));

    // If conversation_id belongs to a project room, attach the project_id so
    // the message shows up in agora_project_conversations.
    let project_id = match conversation_id {
        Some(cid) => state.project_id_for_conversation(&cid).await,
        None => None,
    };

    state
        .store_outbound_from(
            &req.body,
            req.to.as_deref(),
            id,
            reply_to,
            conversation_id,
            project_id,
            req.from.as_deref(),
        )
        .await;

    // Enqueue for offline delivery if the target is specified and not connected
    if let Some(ref to) = req.to {
        if !state.is_peer_connected_by_name(to).await && !state.is_peer_connected_by_addr(to).await
        {
            let queued = crate::outbox::QueuedMessage {
                id,
                to: to.clone(),
                body: req.body.clone(),
                msg_type: None,
                enqueued_at: chrono::Utc::now(),
                reply_to,
                conversation_id,
                delivered: false,
            };
            state.outbox_enqueue(queued).await;
        }
    }

    let sender = req
        .from
        .clone()
        .unwrap_or_else(|| state.node_name().to_string());
    let msg_body = req.body.clone();
    let msg_from = req.from.clone();

    state
        .push_outbox(OutboundMessage {
            body: req.body,
            to: req.to,
            id,
            reply_to,
            conversation_id,
            msg_type: None,
            project_id,
            from_override: req.from,
        })
        .await;

    // Also deliver to ALL local consumers (so listeners see dashboard messages)
    let consumers = state.list_consumers().await;
    for c in &consumers {
        if c.label != "http-default"
            && c.label != sender
            && !c.label.starts_with(&format!("{}-", sender))
        {
            state
                .deliver_to_local_consumer(
                    &c.label,
                    &OutboundMessage {
                        body: msg_body.clone(),
                        to: None,
                        id,
                        reply_to,
                        conversation_id,
                        msg_type: None,
                        project_id,
                        from_override: msg_from.clone(),
                    },
                )
                .await;
        }
    }

    Ok(Json(SendResponse {
        status: "queued".to_string(),
        id: id.to_string(),
    }))
}

/// GET /peers — list connected peers.
async fn get_peers(State(state): State<DaemonState>) -> Json<PeersResponse> {
    let peers = state.get_peers().await;
    Json(PeersResponse {
        count: peers.len(),
        peers: peers
            .into_iter()
            .map(|p| PeerEntry {
                name: p.name,
                address: p.address,
                connected_at: p.connected_at.to_rfc3339(),
                did: p.did,
                session_id: p.session_id.map(|s| s.to_string()),
                verified: p.verified,
                owner_did: p.owner_did,
                owner_verified: p.owner_verified,
            })
            .collect(),
    })
}

/// GET /status — daemon status info.
async fn get_status(State(state): State<DaemonState>) -> Json<StatusResponse> {
    let peers = state.get_peers().await;
    let wake = state.wake_status().await;
    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        node_name: state.node_name().to_string(),
        peers_connected: peers.len(),
        running: true,
        did: state.did().to_string(),
        session_id: state.session_id().to_string(),
        owner_did: state.owner_did().map(|s| s.to_string()),
        wake_enabled: wake.enabled,
        wake_armed: wake.armed,
        wake_listener_count: wake.active_listener_count,
        wake_listener_labels: wake.active_listener_labels,
        last_wake_at: wake.last_fired_at,
        last_wake_from: wake.last_fired_from,
        last_wake_message_count: wake.last_message_count,
    })
}

/// GET /health — simple health check with uptime.
async fn health(State(state): State<DaemonState>) -> Json<HealthResponse> {
    let uptime = chrono::Utc::now()
        .signed_duration_since(state.start_time())
        .num_seconds();
    Json(HealthResponse {
        healthy: true,
        uptime_seconds: uptime,
    })
}

/// GET /wake — get current wake-up hook.
async fn get_wake(State(state): State<DaemonState>) -> Json<WakeResponse> {
    Json(WakeResponse {
        command: state.get_wake_command().await,
    })
}

/// POST /wake — set or clear the wake-up hook.
/// Send {"command": "some-script.sh"} to set, or {"command": null} to clear.
async fn set_wake(
    State(state): State<DaemonState>,
    Json(req): Json<WakeRequest>,
) -> Result<Json<WakeResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.set_wake_command(req.command.clone()).await {
        Ok(()) => Ok(Json(WakeResponse {
            command: req.command,
        })),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e }))),
    }
}

// --- Request / Response types ---

#[derive(Deserialize)]
struct MessagesQuery {
    wait: Option<bool>,
    timeout: Option<u64>,
    /// If true, return messages without removing them from the buffer.
    peek: Option<bool>,
}

#[derive(Serialize)]
struct InboxMessage {
    id: String,
    from: String,
    body: String,
    timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
}

#[derive(Deserialize)]
struct SendRequest {
    body: String,
    /// If omitted, broadcasts to all peers.
    to: Option<String>,
    /// Override the sender name (for multi-agent setups where multiple agents share one daemon).
    from: Option<String>,
    /// Reply to a specific message by its id.
    reply_to: Option<String>,
    /// Conversation thread id.
    conversation_id: Option<String>,
}

#[derive(Serialize)]
struct SendResponse {
    status: String,
    id: String,
}

#[derive(Serialize)]
struct PeersResponse {
    count: usize,
    peers: Vec<PeerEntry>,
}

#[derive(Serialize)]
struct PeerEntry {
    name: String,
    address: String,
    connected_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    did: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_did: Option<String>,
    owner_verified: bool,
}

#[derive(Serialize)]
struct StatusResponse {
    version: String,
    node_name: String,
    peers_connected: usize,
    running: bool,
    did: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_did: Option<String>,
    wake_enabled: bool,
    wake_armed: bool,
    wake_listener_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    wake_listener_labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_wake_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_wake_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_wake_message_count: Option<usize>,
}

#[derive(Serialize)]
struct HealthResponse {
    healthy: bool,
    uptime_seconds: i64,
}

#[derive(Deserialize)]
struct WakeRequest {
    command: Option<String>,
}

#[derive(Serialize)]
struct WakeResponse {
    command: Option<String>,
}

/// Generic error response body.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

async fn can_send_as(state: &DaemonState, from: &str) -> bool {
    if from == state.node_name() {
        return true;
    }
    let consumers = state.list_consumers().await;
    consumers
        .iter()
        .any(|c| c.label == *from || c.label.starts_with(&format!("{}-", from)))
}

async fn resolve_sender_name(state: &DaemonState, from: Option<&String>) -> String {
    match from {
        Some(from) if can_send_as(state, from).await => from.clone(),
        _ => state.node_name().to_string(),
    }
}

// --- Consumer endpoints ---

/// POST /consumers — register a new inbox consumer.
async fn register_consumer(
    State(state): State<DaemonState>,
    Json(req): Json<RegisterConsumerRequest>,
) -> Json<RegisterConsumerResponse> {
    let label = req.label.unwrap_or_else(|| "unnamed".to_string());
    let id = if req.suppress_wake.unwrap_or(false) {
        state.register_listener_consumer(&label).await
    } else {
        state.register_consumer(&label).await
    };
    Json(RegisterConsumerResponse {
        consumer_id: id.0,
        label,
    })
}

/// GET /consumers — list all registered consumers.
async fn list_consumers(State(state): State<DaemonState>) -> Json<ConsumersListResponse> {
    let consumers = state.list_consumers().await;
    Json(ConsumersListResponse {
        count: consumers.len(),
        consumers: consumers
            .into_iter()
            .map(|c| ConsumerEntry {
                consumer_id: c.id.0,
                label: c.label,
                registered_at: c.registered_at,
                last_active: c.last_active,
                buffered_messages: c.buffered_messages,
                suppresses_wake: c.suppresses_wake,
            })
            .collect(),
    })
}

/// GET /consumers/{id}/messages — drain messages from a specific consumer.
async fn get_consumer_messages(
    State(state): State<DaemonState>,
    Path(id): Path<u64>,
    Query(params): Query<MessagesQuery>,
) -> Result<Json<Vec<InboxMessage>>, (StatusCode, &'static str)> {
    let consumer_id = ConsumerId(id);

    // Per-consumer rate limit check
    if !state.check_consumer_rate_limit(consumer_id).await {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "Consumer rate limit exceeded",
        ));
    }

    let messages = if params.peek.unwrap_or(false) {
        state
            .peek_consumer(consumer_id)
            .await
            .ok_or((StatusCode::NOT_FOUND, "Consumer not found"))?
    } else if params.wait.unwrap_or(false) {
        let secs = params.timeout.unwrap_or(30).min(120);
        state
            .wait_for_consumer(consumer_id, Duration::from_secs(secs))
            .await
            .ok_or((StatusCode::NOT_FOUND, "Consumer not found"))?
    } else {
        state
            .drain_consumer(consumer_id)
            .await
            .ok_or((StatusCode::NOT_FOUND, "Consumer not found"))?
    };
    let out: Vec<InboxMessage> = messages
        .into_iter()
        .map(|m| InboxMessage {
            id: m.id.to_string(),
            from: m.from,
            body: m.body,
            timestamp: m.timestamp.to_rfc3339(),
            reply_to: m.reply_to.map(|u| u.to_string()),
            conversation_id: m.conversation_id.map(|u| u.to_string()),
        })
        .collect();
    Ok(Json(out))
}

/// DELETE /consumers/{id} — unregister a consumer.
async fn unregister_consumer(
    State(state): State<DaemonState>,
    Path(id): Path<u64>,
) -> Result<Json<UnregisterConsumerResponse>, StatusCode> {
    if state.unregister_consumer(ConsumerId(id)).await {
        Ok(Json(UnregisterConsumerResponse {
            status: "unregistered".to_string(),
            consumer_id: id,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /consumers/{id}/touch — refresh liveness without draining messages.
async fn touch_consumer(
    State(state): State<DaemonState>,
    Path(id): Path<u64>,
) -> Result<Json<TouchConsumerResponse>, StatusCode> {
    if state.touch_consumer(ConsumerId(id)).await {
        Ok(Json(TouchConsumerResponse {
            status: "touched".to_string(),
            consumer_id: id,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Deserialize)]
struct RegisterConsumerRequest {
    label: Option<String>,
    suppress_wake: Option<bool>,
}

#[derive(Serialize)]
struct RegisterConsumerResponse {
    consumer_id: u64,
    label: String,
}

#[derive(Serialize)]
struct ConsumersListResponse {
    count: usize,
    consumers: Vec<ConsumerEntry>,
}

#[derive(Serialize)]
struct ConsumerEntry {
    consumer_id: u64,
    label: String,
    registered_at: String,
    last_active: String,
    buffered_messages: usize,
    suppresses_wake: bool,
}

#[derive(Serialize)]
struct UnregisterConsumerResponse {
    status: String,
    consumer_id: u64,
}

#[derive(Serialize)]
struct TouchConsumerResponse {
    status: String,
    consumer_id: u64,
}

// --- Friends endpoints ---

/// GET /friends — list all friends with trust levels.
async fn list_friends(State(state): State<DaemonState>) -> Json<FriendsResponse> {
    let friends = state.get_friends().await;
    Json(FriendsResponse {
        count: friends.len(),
        friends: friends
            .into_iter()
            .map(|f| FriendEntry {
                name: f.name,
                alias: f.alias,
                trust_level: f.trust_level.0,
                trust_name: f.trust_level.name().to_string(),
                can_wake: f.trust_level.can_wake(),
                added_at: f.added_at.to_rfc3339(),
                notes: f.notes,
                muted: f.muted,
                did: f.did,
                last_address: f.last_address,
                owner_did: f.owner_did,
                their_trust_name: f.their_trust.map(|t| TrustLevel(t).name().to_string()),
                their_trust: f.their_trust,
            })
            .collect(),
    })
}

/// POST /friends — add a friend.
async fn add_friend(
    State(state): State<DaemonState>,
    Json(req): Json<AddFriendRequest>,
) -> Result<Json<AddFriendResponse>, (StatusCode, Json<ErrorResponse>)> {
    if let Err(e) = crate::state::validate_name(&req.name, "Friend name", 100) {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
    }
    let trust = req.trust_level.unwrap_or(2).min(4);
    let trust_level = TrustLevel(trust);
    let friend = Friend {
        name: req.name.clone(),
        alias: req.alias,
        trust_level,
        added_at: chrono::Utc::now(),
        notes: req.notes,
        muted: false,
        last_address: None,
        did: None,
        owner_did: None,
        their_trust: None,
    };
    let warning = state.add_friend(friend).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;
    Ok(Json(AddFriendResponse {
        status: "added".to_string(),
        name: req.name,
        trust_level: trust,
        can_wake: trust_level.can_wake(),
        warning,
    }))
}

/// DELETE /friends/{name} — remove a friend and disconnect them.
async fn remove_friend(
    State(state): State<DaemonState>,
    Path(name): Path<String>,
    Query(params): Query<RemoveFriendQuery>,
) -> Result<Json<RemoveFriendResponse>, StatusCode> {
    match state.remove_friend(&name).await {
        Ok(true) => {
            // Send friend.revoke P2P message if peer is connected
            {
                use crate::protocol::message::{FriendRevokePayload, MessageType};
                let payload = FriendRevokePayload { reason: None };
                let body = serde_json::to_string(&payload).unwrap_or_default();
                state
                    .push_outbox(OutboundMessage {
                        body,
                        to: Some(name.clone()),
                        id: Uuid::new_v4(),
                        reply_to: None,
                        conversation_id: None,
                        msg_type: Some(MessageType::FriendRevoke),
                        project_id: None,
                        from_override: None,
                    })
                    .await;
            }

            // Disconnect the peer unless explicitly told not to
            let disconnect = params.disconnect.unwrap_or(true);
            let disconnected = if disconnect {
                state.disconnect_peer(&name).await
            } else {
                false
            };
            Ok(Json(RemoveFriendResponse {
                status: "removed".to_string(),
                name,
                disconnected,
            }))
        }
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// PATCH /friends/{name} — partially update a friend (mute, trust, alias, notes).
async fn update_friend(
    State(state): State<DaemonState>,
    Path(name): Path<String>,
    Json(patch): Json<FriendPatch>,
) -> Result<Json<UpdateFriendResponse>, StatusCode> {
    match state.update_friend(&name, &patch).await {
        Ok(true) => Ok(Json(UpdateFriendResponse {
            status: "updated".to_string(),
            name,
        })),
        Ok(false) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Serialize)]
struct UpdateFriendResponse {
    status: String,
    name: String,
}

// --- Conversation endpoints ---

/// GET /conversations — list all conversation threads.
async fn list_conversations(State(state): State<DaemonState>) -> Json<ConversationsResponse> {
    let conversations = state.get_conversations().await;
    Json(ConversationsResponse {
        count: conversations.len(),
        conversations,
    })
}

/// GET /conversations/{id} — get messages for a specific conversation thread.
async fn get_conversation(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Json<ConversationResponse> {
    let messages = state.get_conversation(&id).await;
    Json(ConversationResponse {
        conversation_id: id,
        message_count: messages.len(),
        messages,
    })
}

/// DELETE /conversations/{id} — delete a conversation and all its messages.
async fn delete_conversation(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.delete_conversation(&id).await {
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "deleted", "conversation_id": id })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "not_found", "conversation_id": id })),
        )
    }
}

/// DELETE /conversations/{id}/messages/{msg_id} — delete a single message.
async fn delete_message(
    State(state): State<DaemonState>,
    Path((id, msg_id)): Path<(String, String)>,
) -> impl IntoResponse {
    if state.delete_message(&msg_id).await {
        (
            StatusCode::OK,
            Json(
                serde_json::json!({ "status": "deleted", "conversation_id": id, "message_id": msg_id }),
            ),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "status": "not_found", "conversation_id": id, "message_id": msg_id }),
            ),
        )
    }
}

#[derive(Serialize)]
struct ConversationsResponse {
    count: usize,
    conversations: Vec<crate::state::ConversationSummary>,
}

#[derive(Serialize)]
struct ConversationResponse {
    conversation_id: String,
    message_count: usize,
    messages: Vec<crate::state::StoredMessage>,
}

#[derive(Serialize)]
struct FriendsResponse {
    count: usize,
    friends: Vec<FriendEntry>,
}

#[derive(Serialize)]
struct FriendEntry {
    name: String,
    alias: Option<String>,
    trust_level: u8,
    trust_name: String,
    can_wake: bool,
    added_at: String,
    notes: Option<String>,
    muted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    did: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_did: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    their_trust: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    their_trust_name: Option<String>,
}

#[derive(Deserialize)]
struct AddFriendRequest {
    name: String,
    alias: Option<String>,
    trust_level: Option<u8>,
    notes: Option<String>,
}

#[derive(Serialize)]
struct AddFriendResponse {
    status: String,
    name: String,
    trust_level: u8,
    can_wake: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Deserialize)]
struct RemoveFriendQuery {
    disconnect: Option<bool>,
}

#[derive(Serialize)]
struct RemoveFriendResponse {
    status: String,
    name: String,
    disconnected: bool,
}

// --- Peer disconnect ---

/// POST /peers/{name}/disconnect — disconnect a connected peer.
async fn disconnect_peer(
    State(state): State<DaemonState>,
    Path(name): Path<String>,
) -> Result<Json<DisconnectResponse>, StatusCode> {
    if state.disconnect_peer(&name).await {
        Ok(Json(DisconnectResponse {
            status: "disconnected".to_string(),
            name,
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Serialize)]
struct DisconnectResponse {
    status: String,
    name: String,
}

// --- Thread endpoints ---

/// GET /threads — list all threads, optionally filtered by participant.
async fn list_threads(
    State(state): State<DaemonState>,
    Query(params): Query<ThreadListQuery>,
) -> Json<ThreadListResponse> {
    let threads = state.list_threads(params.participant.as_deref()).await;
    Json(ThreadListResponse {
        count: threads.len(),
        threads,
    })
}

/// POST /threads — create a new thread and broadcast to peers.
async fn create_thread(
    State(state): State<DaemonState>,
    Json(req): Json<CreateThreadRequest>,
) -> Result<Json<CreateThreadResponse>, StatusCode> {
    use crate::protocol::message::{MessageType, ThreadCreatePayload};

    let id = req.conversation_id.and_then(|s| Uuid::parse_str(&s).ok());
    let creator = state.node_name().to_string();
    let title = req.title.clone();
    let participants = req.participants.clone().unwrap_or_default();
    let min_trust = req.min_trust.unwrap_or(0);
    let closed = req.closed.unwrap_or(false);
    let metadata = req.metadata.clone().unwrap_or_default();

    match state
        .create_thread(
            id,
            &creator,
            title.clone(),
            participants.clone(),
            min_trust,
            closed,
            metadata.clone(),
        )
        .await
    {
        Ok(thread_id) => {
            // Broadcast thread.create to connected peers
            let payload = ThreadCreatePayload {
                conversation_id: thread_id,
                title,
                participants,
                min_trust,
                closed,
                metadata,
            };
            let body = serde_json::to_string(&payload).unwrap_or_default();
            state
                .push_outbox(OutboundMessage {
                    body,
                    to: None,
                    id: Uuid::new_v4(),
                    reply_to: None,
                    conversation_id: Some(thread_id),
                    msg_type: Some(MessageType::ThreadCreate),
                    project_id: None,
                    from_override: None,
                })
                .await;
            Ok(Json(CreateThreadResponse {
                status: "created".to_string(),
                thread_id: thread_id.to_string(),
            }))
        }
        Err(e) => {
            tracing::warn!("Thread create failed: {}", e);
            Err(StatusCode::CONFLICT)
        }
    }
}

/// GET /threads/{id} — get a thread's details.
async fn get_thread(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    match state.get_thread(&uuid).await {
        Some(thread) => Ok(Json(serde_json::to_value(thread).unwrap())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// DELETE /threads/{id} — close a thread and broadcast to peers.
async fn close_thread(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<CloseThreadRequest>,
) -> Result<Json<StatusMessage>, StatusCode> {
    use crate::protocol::message::{MessageType, ThreadClosePayload};

    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let closer = state.node_name().to_string();
    let reason = req.reason.clone();
    state
        .close_thread(&uuid, &closer, req.reason)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Broadcast thread.close to connected peers
    let payload = ThreadClosePayload {
        conversation_id: uuid,
        reason,
    };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: None,
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: Some(uuid),
            msg_type: Some(MessageType::ThreadClose),
            project_id: None,
            from_override: None,
        })
        .await;

    Ok(Json(StatusMessage {
        status: "closed".to_string(),
    }))
}

/// POST /threads/{id}/participants — add a participant.
async fn thread_add_participant(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<AddParticipantRequest>,
) -> Result<Json<StatusMessage>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let inviter = state.node_name().to_string();

    // Look up invitee's trust level
    let trust = state
        .get_friends()
        .await
        .iter()
        .find(|f| f.name == req.name)
        .map(|f| f.trust_level.0)
        .unwrap_or(0);

    state
        .thread_add_participant(&uuid, &inviter, &req.name, trust)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(StatusMessage {
        status: "added".to_string(),
    }))
}

/// DELETE /threads/{id}/participants/{name} — remove a participant.
async fn thread_remove_participant(
    State(state): State<DaemonState>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Json<StatusMessage>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let remover = state.node_name().to_string();
    state
        .thread_remove_participant(&uuid, &remover, &name)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(StatusMessage {
        status: "removed".to_string(),
    }))
}

#[derive(Deserialize)]
struct ThreadListQuery {
    participant: Option<String>,
}

#[derive(Serialize)]
struct ThreadListResponse {
    count: usize,
    threads: Vec<ThreadSummary>,
}

#[derive(Deserialize)]
struct CreateThreadRequest {
    conversation_id: Option<String>,
    title: Option<String>,
    participants: Option<Vec<String>>,
    min_trust: Option<u8>,
    closed: Option<bool>,
    metadata: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize)]
struct CreateThreadResponse {
    status: String,
    thread_id: String,
}

#[derive(Deserialize)]
struct CloseThreadRequest {
    reason: Option<String>,
}

#[derive(Deserialize)]
struct AddParticipantRequest {
    name: String,
}

#[derive(Serialize)]
struct StatusMessage {
    status: String,
}

// ---------------------------------------------------------------------------
// GET /identity — agent's cryptographic identity
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct IdentityResponse {
    did: String,
    public_key: String,
    session_id: String,
    node_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_did: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_attestation: Option<IdentityOwnerAttestation>,
}

#[derive(Serialize)]
struct IdentityOwnerAttestation {
    owner_did: String,
    agent_did: String,
    created_at: i64,
    valid: bool,
}

async fn get_identity(State(state): State<DaemonState>) -> Json<IdentityResponse> {
    let owner_att = state.owner_attestation().map(|a| IdentityOwnerAttestation {
        owner_did: a.owner_did.clone(),
        agent_did: a.agent_did.clone(),
        created_at: a.created_at,
        valid: a.verify_for_agent(state.did()),
    });
    Json(IdentityResponse {
        did: state.did().to_string(),
        public_key: state.identity().public_key_base58(),
        session_id: state.session_id().to_string(),
        node_name: state.node_name().to_string(),
        owner_did: state.owner_did().map(|s| s.to_string()),
        owner_attestation: owner_att,
    })
}

// ---------------------------------------------------------------------------
// Friend request endpoints
// ---------------------------------------------------------------------------

/// GET /friend-requests — list friend requests, optionally filtered by status.
async fn list_friend_requests(
    State(state): State<DaemonState>,
    Query(params): Query<FriendRequestQuery>,
) -> Json<FriendRequestListResponse> {
    let requests = state.get_friend_requests().await;
    let filtered: Vec<FriendRequestEntry> = requests
        .into_iter()
        .filter(|r| {
            if let Some(ref status) = params.status {
                match status.as_str() {
                    "pending" => r.status == FriendRequestStatus::Pending,
                    "accepted" => r.status == FriendRequestStatus::Accepted,
                    "rejected" => r.status == FriendRequestStatus::Rejected,
                    _ => true,
                }
            } else {
                true
            }
        })
        .map(|r| FriendRequestEntry {
            id: r.id.to_string(),
            peer_name: r.peer_name,
            peer_did: r.peer_did,
            offered_trust: r.offered_trust,
            offered_trust_name: TrustLevel(r.offered_trust).name().to_string(),
            direction: format!("{:?}", r.direction).to_lowercase(),
            status: format!("{:?}", r.status).to_lowercase(),
            created_at: r.created_at.to_rfc3339(),
            resolved_at: r.resolved_at.map(|t| t.to_rfc3339()),
            message: r.message,
            owner_did: r.owner_did,
        })
        .collect();
    Json(FriendRequestListResponse {
        count: filtered.len(),
        requests: filtered,
    })
}

/// POST /friend-requests — send a friend request to a peer.
async fn send_friend_request(
    State(state): State<DaemonState>,
    Json(req): Json<SendFriendRequestBody>,
) -> Result<Json<SendFriendRequestResponse>, StatusCode> {
    use crate::protocol::message::{FriendRequestPayload, MessageType};
    use crate::state::{FriendRequestDirection, FriendRequestStatus};

    let trust_level = req.trust_level.unwrap_or(2).min(4);

    // Check for duplicate pending outbound
    if state.has_pending_outbound_to(&req.peer_name).await {
        return Err(StatusCode::CONFLICT);
    }

    // Create outbound request entry
    let request_id = Uuid::new_v4();
    let request = FriendRequest {
        id: request_id,
        peer_name: req.peer_name.clone(),
        peer_did: None,
        offered_trust: trust_level,
        direction: FriendRequestDirection::Outbound,
        status: FriendRequestStatus::Pending,
        created_at: chrono::Utc::now(),
        resolved_at: None,
        message: req.message.clone(),
        owner_did: None,
    };
    state
        .add_friend_request(request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Send P2P message if peer is connected
    let payload = FriendRequestPayload {
        did: state.did().to_string(),
        public_key: state.identity().public_key_base58(),
        trust_level,
        message: req.message,
        node_name: state.node_name().to_string(),
        owner_did: state.owner_did().map(|s| s.to_string()),
    };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: Some(req.peer_name.clone()),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            msg_type: Some(MessageType::FriendRequest),
            project_id: None,
            from_override: None,
        })
        .await;

    Ok(Json(SendFriendRequestResponse {
        status: "sent".to_string(),
        request_id: request_id.to_string(),
        peer_name: req.peer_name,
        trust_level,
    }))
}

/// POST /friend-requests/{id}/accept — accept an inbound friend request.
async fn accept_friend_request(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<AcceptFriendRequestBody>,
) -> Result<Json<AcceptFriendRequestResponse>, StatusCode> {
    use crate::protocol::message::{FriendAcceptPayload, MessageType};

    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let trust_level = req.trust_level.unwrap_or(2).min(4);

    let request = state
        .accept_friend_request(&uuid, trust_level)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Send friend.accept P2P message
    let payload = FriendAcceptPayload {
        did: state.did().to_string(),
        trust_level,
        message: req.message,
    };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: Some(request.peer_name.clone()),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            msg_type: Some(MessageType::FriendAccept),
            project_id: None,
            from_override: None,
        })
        .await;

    Ok(Json(AcceptFriendRequestResponse {
        status: "accepted".to_string(),
        peer_name: request.peer_name,
        trust_level,
    }))
}

/// POST /friend-requests/{id}/reject — reject an inbound friend request.
async fn reject_friend_request(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<RejectFriendRequestBody>,
) -> Result<Json<RejectFriendRequestResponse>, StatusCode> {
    use crate::protocol::message::{FriendRejectPayload, MessageType};

    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let request = state
        .reject_friend_request(&uuid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Send friend.reject P2P message
    let payload = FriendRejectPayload { reason: req.reason };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: Some(request.peer_name.clone()),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            msg_type: Some(MessageType::FriendReject),
            project_id: None,
            from_override: None,
        })
        .await;

    Ok(Json(RejectFriendRequestResponse {
        status: "rejected".to_string(),
        peer_name: request.peer_name,
    }))
}

#[derive(Deserialize)]
struct FriendRequestQuery {
    status: Option<String>,
}

#[derive(Serialize)]
struct FriendRequestListResponse {
    count: usize,
    requests: Vec<FriendRequestEntry>,
}

#[derive(Serialize)]
struct FriendRequestEntry {
    id: String,
    peer_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_did: Option<String>,
    offered_trust: u8,
    offered_trust_name: String,
    direction: String,
    status: String,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_did: Option<String>,
}

#[derive(Deserialize)]
struct SendFriendRequestBody {
    peer_name: String,
    trust_level: Option<u8>,
    message: Option<String>,
}

#[derive(Serialize)]
struct SendFriendRequestResponse {
    status: String,
    request_id: String,
    peer_name: String,
    trust_level: u8,
}

#[derive(Deserialize)]
struct AcceptFriendRequestBody {
    trust_level: Option<u8>,
    message: Option<String>,
}

#[derive(Serialize)]
struct AcceptFriendRequestResponse {
    status: String,
    peer_name: String,
    trust_level: u8,
}

#[derive(Deserialize)]
struct RejectFriendRequestBody {
    reason: Option<String>,
}

#[derive(Serialize)]
struct RejectFriendRequestResponse {
    status: String,
    peer_name: String,
}

// ---------------------------------------------------------------------------
// Project endpoints
// ---------------------------------------------------------------------------

/// GET /projects — list all projects.
async fn list_projects(State(state): State<DaemonState>) -> Json<ProjectListResponse> {
    let projects = state.get_projects().await;
    Json(ProjectListResponse {
        count: projects.len(),
        projects: projects
            .into_iter()
            .map(|p| ProjectEntry {
                id: p.id.to_string(),
                name: p.name,
                description: p.description,
                owner_name: p.owner_name,
                repo: p.repo,
                status: p.status.name().to_lowercase(),
                agent_count: p.agents.len(),
                active_agents: p.agents.iter().filter(|a| a.clocked_in).count(),
                agent_names: p.agents.iter().map(|a| a.name.clone()).collect(),
                created_at: p.created_at.to_rfc3339(),
                updated_at: p.updated_at.to_rfc3339(),
            })
            .collect(),
    })
}

/// POST /projects — create a project.
async fn create_project(
    State(state): State<DaemonState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<CreateProjectResponse>, (StatusCode, Json<ErrorResponse>)> {
    if let Err(e) = crate::state::validate_name(&req.name, "Project name", 200) {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
    }
    let id = state
        .create_project(&req.name, req.description, req.repo)
        .await;
    state
        .append_audit(
            &id,
            "project.created",
            &format!("Project created: {}", req.name),
        )
        .await;
    Ok(Json(CreateProjectResponse {
        status: "created".to_string(),
        id: id.to_string(),
        name: req.name,
    }))
}

/// GET /projects/{id} — project detail with agents.
async fn get_project(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<ProjectDetailResponse>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let project = state
        .get_project(&uuid)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(ProjectDetailResponse {
        id: project.id.to_string(),
        name: project.name,
        description: project.description,
        owner_did: project.owner_did,
        owner_name: project.owner_name,
        repo: project.repo,
        status: project.status.name().to_lowercase(),
        agents: project
            .agents
            .into_iter()
            .map(|a| ProjectAgentEntry {
                name: a.name,
                did: a.did,
                role: a.role.name().to_lowercase(),
                joined_at: a.joined_at.to_rfc3339(),
                clocked_in: a.clocked_in,
                current_focus: a.current_focus,
                last_clock_in: a.last_clock_in.map(|t| t.to_rfc3339()),
            })
            .collect(),
        created_at: project.created_at.to_rfc3339(),
        updated_at: project.updated_at.to_rfc3339(),
        notes: project.notes,
    }))
}

/// PATCH /projects/{id} — update project.
async fn update_project(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Json<StatusMessage>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let status_str = req.status.clone();
    let status = status_str.as_deref().and_then(|s| match s {
        "active" => Some(ProjectStatus::Active),
        "paused" => Some(ProjectStatus::Paused),
        "completed" => Some(ProjectStatus::Completed),
        "archived" => Some(ProjectStatus::Archived),
        _ => None,
    });
    if state
        .update_project(&uuid, status, req.description, req.notes)
        .await
    {
        // Broadcast update to peers
        use crate::protocol::message::{MessageType, ProjectUpdatePayload};
        let payload = ProjectUpdatePayload {
            project_id: uuid,
            status: req.status,
            description: None,
            notes: None,
        };
        let body = serde_json::to_string(&payload).unwrap_or_default();
        state
            .push_outbox(OutboundMessage {
                body,
                to: None,
                id: Uuid::new_v4(),
                reply_to: None,
                conversation_id: None,
                msg_type: Some(MessageType::ProjectUpdate),
                project_id: Some(uuid),
                from_override: None,
            })
            .await;
        Ok(Json(StatusMessage {
            status: "updated".to_string(),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// DELETE /projects/{id} — archive project.
async fn archive_project(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<StatusMessage>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if state.archive_project(&uuid).await {
        Ok(Json(StatusMessage {
            status: "archived".to_string(),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /projects/{id}/clock-in
async fn project_clock_in(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<ClockInRequest>,
) -> Result<Json<StatusMessage>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let name = state.node_name().to_string();
    let focus_clone = req.focus.clone();
    if state.project_clock_in(&uuid, &name, req.focus).await {
        // Broadcast clock-in to peers
        use crate::protocol::message::{MessageType, ProjectClockInPayload};
        let payload = ProjectClockInPayload {
            project_id: uuid,
            focus: focus_clone,
        };
        let body = serde_json::to_string(&payload).unwrap_or_default();
        state
            .push_outbox(OutboundMessage {
                body,
                to: None,
                id: Uuid::new_v4(),
                reply_to: None,
                conversation_id: None,
                msg_type: Some(MessageType::ProjectClockIn),
                project_id: Some(uuid),
                from_override: None,
            })
            .await;
        state
            .append_audit(&uuid, "agent.clocked_in", &format!("{} clocked in", name))
            .await;
        Ok(Json(StatusMessage {
            status: "clocked_in".to_string(),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /projects/{id}/clock-out
async fn project_clock_out(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<StatusMessage>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let name = state.node_name().to_string();
    if state.project_clock_out(&uuid, &name).await {
        // Broadcast clock-out to peers
        use crate::protocol::message::{MessageType, ProjectClockOutPayload};
        let payload = ProjectClockOutPayload { project_id: uuid };
        let body = serde_json::to_string(&payload).unwrap_or_default();
        state
            .push_outbox(OutboundMessage {
                body,
                to: None,
                id: Uuid::new_v4(),
                reply_to: None,
                conversation_id: None,
                msg_type: Some(MessageType::ProjectClockOut),
                project_id: Some(uuid),
                from_override: None,
            })
            .await;
        state
            .append_audit(&uuid, "agent.clocked_out", &format!("{} clocked out", name))
            .await;
        Ok(Json(StatusMessage {
            status: "clocked_out".to_string(),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// GET /project-invitations — list project invitations.
async fn list_project_invitations(
    State(state): State<DaemonState>,
    Query(params): Query<ProjectInvitationQuery>,
) -> Json<ProjectInvitationListResponse> {
    let invitations = state.get_project_invitations().await;
    let filtered: Vec<ProjectInvitationEntry> = invitations
        .into_iter()
        .filter(|i| {
            if let Some(ref status) = params.status {
                match status.as_str() {
                    "pending" => i.status == InvitationStatus::Pending,
                    "accepted" => i.status == InvitationStatus::Accepted,
                    "declined" => i.status == InvitationStatus::Declined,
                    _ => true,
                }
            } else {
                true
            }
        })
        .map(|i| ProjectInvitationEntry {
            id: i.id.to_string(),
            project_id: i.project_id.to_string(),
            project_name: i.project_name,
            peer_name: i.peer_name,
            role: i.role.name().to_lowercase(),
            direction: match i.direction {
                InvitationDirection::Inbound => "inbound".to_string(),
                InvitationDirection::Outbound => "outbound".to_string(),
            },
            status: match i.status {
                InvitationStatus::Pending => "pending".to_string(),
                InvitationStatus::Accepted => "accepted".to_string(),
                InvitationStatus::Declined => "declined".to_string(),
            },
            created_at: i.created_at.to_rfc3339(),
            resolved_at: i.resolved_at.map(|t| t.to_rfc3339()),
            message: i.message,
        })
        .collect();
    Json(ProjectInvitationListResponse {
        count: filtered.len(),
        invitations: filtered,
    })
}

/// POST /project-invitations — invite a peer to a project.
async fn send_project_invitation(
    State(state): State<DaemonState>,
    Json(req): Json<SendProjectInvitationRequest>,
) -> Result<Json<SendProjectInvitationResponse>, StatusCode> {
    use crate::protocol::message::{MessageType, ProjectInvitePayload};

    let project_id = Uuid::parse_str(&req.project_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let role: ProjectRole = req.role.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    // Check project exists
    let project = state
        .get_project(&project_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Build context package
    let context = state.build_project_context(&project_id, role).await;

    // Store outbound invitation
    let invitation_id = Uuid::new_v4();
    let invitation = ProjectInvitation {
        id: invitation_id,
        project_id,
        project_name: project.name.clone(),
        peer_name: req.peer_name.clone(),
        peer_did: None,
        role,
        direction: InvitationDirection::Outbound,
        status: InvitationStatus::Pending,
        created_at: chrono::Utc::now(),
        resolved_at: None,
        message: req.message.clone(),
        context: context.clone(),
    };
    state
        .add_project_invitation(invitation)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Send P2P invite message
    let payload = ProjectInvitePayload {
        project_id,
        project_name: project.name.clone(),
        role: role.name().to_lowercase(),
        message: req.message,
        context,
    };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: Some(req.peer_name.clone()),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            msg_type: Some(MessageType::ProjectInvite),
            project_id: Some(project_id),
            from_override: None,
        })
        .await;

    Ok(Json(SendProjectInvitationResponse {
        status: "sent".to_string(),
        invitation_id: invitation_id.to_string(),
        project_name: project.name,
        peer_name: req.peer_name,
        role: role.name().to_lowercase(),
    }))
}

/// POST /project-invitations/{id}/accept — accept a project invitation.
async fn accept_project_invitation(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<StatusMessage>, StatusCode> {
    use crate::protocol::message::{MessageType, ProjectAcceptPayload};

    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;

    // Only inbound invitations can be accepted locally
    if let Some(inv) = state.get_project_invitation(&uuid).await {
        if inv.direction == InvitationDirection::Outbound {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let invitation = state
        .accept_project_invitation(&uuid)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Add ourselves to the project (create it locally if it came from remote)
    let project_exists = state.get_project(&invitation.project_id).await.is_some();
    if !project_exists {
        state
            .create_project_from_invitation(
                invitation.project_id,
                invitation
                    .context
                    .as_ref()
                    .map(|c| c.project_name.as_str())
                    .unwrap_or(&invitation.project_name),
                invitation
                    .context
                    .as_ref()
                    .and_then(|c| c.description.clone()),
                invitation.context.as_ref().and_then(|c| c.repo.clone()),
                &invitation.peer_name,
                invitation.role,
            )
            .await;
    } else {
        state
            .add_project_agent(
                &invitation.project_id,
                state.node_name(),
                Some(state.did().to_string()),
                invitation.role,
            )
            .await;
    }

    // Send accept message to inviter
    let payload = ProjectAcceptPayload {
        project_id: invitation.project_id,
        message: Some("Joined the project".to_string()),
    };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: Some(invitation.peer_name),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            msg_type: Some(MessageType::ProjectAccept),
            project_id: Some(invitation.project_id),
            from_override: None,
        })
        .await;

    Ok(Json(StatusMessage {
        status: "accepted".to_string(),
    }))
}

/// POST /project-invitations/{id}/decline — decline a project invitation.
async fn decline_project_invitation(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<DeclineProjectInvitationRequest>,
) -> Result<Json<StatusMessage>, StatusCode> {
    use crate::protocol::message::{MessageType, ProjectDeclinePayload};

    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let invitation = state
        .decline_project_invitation(&uuid)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Send decline message
    let payload = ProjectDeclinePayload {
        project_id: invitation.project_id,
        reason: req.reason,
    };
    let body = serde_json::to_string(&payload).unwrap_or_default();
    state
        .push_outbox(OutboundMessage {
            body,
            to: Some(invitation.peer_name),
            id: Uuid::new_v4(),
            reply_to: None,
            conversation_id: None,
            msg_type: Some(MessageType::ProjectDecline),
            project_id: Some(invitation.project_id),
            from_override: None,
        })
        .await;

    Ok(Json(StatusMessage {
        status: "declined".to_string(),
    }))
}

// --- Project request/response types ---

#[derive(Serialize)]
struct ProjectListResponse {
    count: usize,
    projects: Vec<ProjectEntry>,
}

#[derive(Serialize)]
struct ProjectEntry {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    owner_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    status: String,
    agent_count: usize,
    active_agents: usize,
    agent_names: Vec<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct CreateProjectRequest {
    name: String,
    description: Option<String>,
    repo: Option<String>,
}

#[derive(Serialize)]
struct CreateProjectResponse {
    status: String,
    id: String,
    name: String,
}

#[derive(Serialize)]
struct ProjectDetailResponse {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    owner_did: String,
    owner_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    status: String,
    agents: Vec<ProjectAgentEntry>,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Serialize)]
struct ProjectAgentEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    did: Option<String>,
    role: String,
    joined_at: String,
    clocked_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_clock_in: Option<String>,
}

#[derive(Deserialize)]
struct UpdateProjectRequest {
    status: Option<String>,
    description: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct ClockInRequest {
    focus: Option<String>,
}

#[derive(Serialize)]
struct ProjectInvitationListResponse {
    count: usize,
    invitations: Vec<ProjectInvitationEntry>,
}

#[derive(Serialize)]
struct ProjectInvitationEntry {
    id: String,
    project_id: String,
    project_name: String,
    peer_name: String,
    role: String,
    direction: String,
    status: String,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Deserialize)]
struct ProjectInvitationQuery {
    status: Option<String>,
}

#[derive(Deserialize)]
struct SendProjectInvitationRequest {
    project_id: String,
    peer_name: String,
    role: String,
    message: Option<String>,
}

#[derive(Serialize)]
struct SendProjectInvitationResponse {
    status: String,
    invitation_id: String,
    project_name: String,
    peer_name: String,
    role: String,
}

#[derive(Deserialize)]
struct DeclineProjectInvitationRequest {
    reason: Option<String>,
}

// ---------------------------------------------------------------------------
// POST /connect — connect to a remote peer by address
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ConnectRequest {
    address: String,
}

#[derive(Serialize)]
struct ConnectResponse {
    status: String,
    address: String,
}

async fn connect_to_peer(
    State(state): State<DaemonState>,
    Json(req): Json<ConnectRequest>,
) -> Json<ConnectResponse> {
    let address = req.address.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::net::connect_to_peer(state, &address).await {
            tracing::error!("Connection to {} failed: {}", address, e);
        }
    });
    Json(ConnectResponse {
        status: "connecting".to_string(),
        address: req.address,
    })
}

// ---------------------------------------------------------------------------
// Project conversation endpoint
// ---------------------------------------------------------------------------

/// GET /projects/{id}/conversations — get all messages related to a project.
async fn get_project_conversations(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<ProjectConversationResponse>, StatusCode> {
    // Verify project exists
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    state
        .get_project(&uuid)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let messages = state.get_project_messages(&id).await;
    Ok(Json(ProjectConversationResponse {
        project_id: id,
        count: messages.len(),
        messages,
    }))
}

#[derive(Serialize)]
struct ProjectConversationResponse {
    project_id: String,
    count: usize,
    messages: Vec<crate::state::StoredMessage>,
}

// ---------------------------------------------------------------------------
// Task Board endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct TaskListResponse {
    count: usize,
    tasks: Vec<serde_json::Value>,
}

/// GET /projects/{id}/tasks
async fn list_tasks(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<TaskListResponse>, StatusCode> {
    let project_id = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let tasks = state
        .get_tasks(&project_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let task_values: Vec<serde_json::Value> = tasks
        .iter()
        .map(|t| serde_json::to_value(t).unwrap_or_default())
        .collect();
    Ok(Json(TaskListResponse {
        count: task_values.len(),
        tasks: task_values,
    }))
}

#[derive(Deserialize)]
struct CreateTaskRequest {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
}

/// POST /projects/{id}/tasks
async fn create_task(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    // Permission check: require "write" to create tasks
    if let Err(e) = state
        .check_permission(&project_id, state.node_name(), Some(state.did()), "write")
        .await
    {
        state.append_audit(&project_id, "access.denied", &e).await;
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }
    if let Err(e) = crate::state::validate_name(&req.title, "Task title", 500) {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 5000 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Task description too long (max 5000 chars)".to_string(),
                }),
            ));
        }
    }
    let title_clone = req.title.clone();
    let assignee_clone = req.assignee.clone();
    let desc_clone = req.description.clone();
    let priority_str = req.priority.clone();
    let priority = req.priority.and_then(|p| match p.to_lowercase().as_str() {
        "low" => Some(crate::project::TaskPriority::Low),
        "medium" => Some(crate::project::TaskPriority::Medium),
        "high" => Some(crate::project::TaskPriority::High),
        "critical" => Some(crate::project::TaskPriority::Critical),
        _ => None,
    });
    let depends_on: Vec<Uuid> = req
        .depends_on
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();
    let depends_clone = depends_on.clone();
    match state
        .create_task(
            &project_id,
            &req.title,
            req.description,
            req.assignee,
            priority,
            depends_on,
            Some(state.node_name().to_string()),
        )
        .await
    {
        Some(task_id) => {
            // Auto-audit
            state
                .append_audit(
                    &project_id,
                    "task.created",
                    &format!("Created task: {}", title_clone),
                )
                .await;
            // Broadcast to peers
            use crate::protocol::message::{MessageType, TaskAssignPayload};
            let payload = TaskAssignPayload {
                project_id,
                task_id,
                title: title_clone,
                description: desc_clone,
                assignee: assignee_clone,
                priority: priority_str,
                depends_on: depends_clone,
            };
            let body = serde_json::to_string(&payload).unwrap_or_default();
            state
                .push_outbox(OutboundMessage {
                    body,
                    to: None,
                    id: Uuid::new_v4(),
                    reply_to: None,
                    conversation_id: None,
                    msg_type: Some(MessageType::TaskAssign),
                    project_id: Some(project_id),
                    from_override: None,
                })
                .await;
            Ok(Json(serde_json::json!({
                "status": "created",
                "task_id": task_id.to_string(),
            })))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Project not found".to_string(),
            }),
        )),
    }
}

/// GET /projects/{id}/tasks/{task_id}
async fn get_task(
    State(state): State<DaemonState>,
    Path((id, task_id_str)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let project_id = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let task_id = Uuid::parse_str(&task_id_str).map_err(|_| StatusCode::BAD_REQUEST)?;
    let tasks = state
        .get_tasks(&project_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let task = tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::to_value(task).unwrap_or_default()))
}

#[derive(Deserialize)]
struct UpdateTaskRequest {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
}

/// PATCH /projects/{id}/tasks/{task_id}
async fn update_task(
    State(state): State<DaemonState>,
    Path((id, task_id_str)): Path<(String, String)>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    let task_id = Uuid::parse_str(&task_id_str).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid task ID".to_string(),
            }),
        )
    })?;
    // Permission check: require "write" to update tasks
    if let Err(e) = state
        .check_permission(&project_id, state.node_name(), Some(state.did()), "write")
        .await
    {
        state.append_audit(&project_id, "access.denied", &e).await;
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }
    let status = req
        .status
        .and_then(|s| s.parse::<crate::project::TaskStatus>().ok());

    let status_clone = status.clone();
    match state
        .update_task(
            &project_id,
            &task_id,
            status,
            req.title,
            req.description,
            req.assignee,
        )
        .await
    {
        Ok(unblocked) => {
            // Auto-audit
            if let Some(ref s) = status_clone {
                let detail = format!("Task {} status → {}", task_id, s.name());
                state
                    .append_audit(&project_id, "task.updated", &detail)
                    .await;
                if *s == crate::project::TaskStatus::Done {
                    state
                        .append_audit(
                            &project_id,
                            "task.completed",
                            &format!("Task {} completed", task_id),
                        )
                        .await;
                }
            }
            // Broadcast to peers
            use crate::protocol::message::{MessageType, TaskUpdatePayload};
            let payload = TaskUpdatePayload {
                project_id,
                task_id,
                status: status_clone.map(|s| s.name().to_string()),
                title: None,
                description: None,
                assignee: None,
            };
            let body = serde_json::to_string(&payload).unwrap_or_default();
            state
                .push_outbox(OutboundMessage {
                    body,
                    to: None,
                    id: Uuid::new_v4(),
                    reply_to: None,
                    conversation_id: None,
                    msg_type: Some(MessageType::TaskUpdate),
                    project_id: Some(project_id),
                    from_override: None,
                })
                .await;
            Ok(Json(serde_json::json!({
                "status": "updated",
                "unblocked_task_ids": unblocked.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
            })))
        }
        Err(e) => Err((StatusCode::NOT_FOUND, Json(ErrorResponse { error: e }))),
    }
}

/// DELETE /projects/{id}/tasks/{task_id}
async fn delete_task(
    State(state): State<DaemonState>,
    Path((id, task_id_str)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    let task_id = Uuid::parse_str(&task_id_str).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid task ID".to_string(),
            }),
        )
    })?;
    // Permission check: require "write" to delete tasks
    if let Err(e) = state
        .check_permission(&project_id, state.node_name(), Some(state.did()), "write")
        .await
    {
        state.append_audit(&project_id, "access.denied", &e).await;
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }
    if state.delete_task(&project_id, &task_id).await {
        Ok(Json(serde_json::json!({ "status": "deleted" })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Task not found".to_string(),
            }),
        ))
    }
}

#[derive(Deserialize)]
struct AssignTaskRequest {
    assignee: String,
}

/// POST /projects/{id}/tasks/{task_id}/assign
async fn assign_task(
    State(state): State<DaemonState>,
    Path((id, task_id_str)): Path<(String, String)>,
    Json(req): Json<AssignTaskRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    let task_id = Uuid::parse_str(&task_id_str).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid task ID".to_string(),
            }),
        )
    })?;
    // Permission check: require "coordinate" to assign tasks
    if let Err(e) = state
        .check_permission(
            &project_id,
            state.node_name(),
            Some(state.did()),
            "coordinate",
        )
        .await
    {
        state.append_audit(&project_id, "access.denied", &e).await;
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }
    if state
        .assign_task(&project_id, &task_id, &req.assignee)
        .await
    {
        state
            .append_audit(
                &project_id,
                "task.assigned",
                &format!("Task {} assigned to {}", task_id, req.assignee),
            )
            .await;
        // Broadcast to peers
        use crate::protocol::message::{MessageType, TaskAssignPayload};
        let payload = TaskAssignPayload {
            project_id,
            task_id,
            title: String::new(),
            description: None,
            assignee: Some(req.assignee),
            priority: None,
            depends_on: vec![],
        };
        let body = serde_json::to_string(&payload).unwrap_or_default();
        state
            .push_outbox(OutboundMessage {
                body,
                to: None,
                id: Uuid::new_v4(),
                reply_to: None,
                conversation_id: None,
                msg_type: Some(MessageType::TaskAssign),
                project_id: Some(project_id),
                from_override: None,
            })
            .await;
        Ok(Json(serde_json::json!({ "status": "assigned" })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Task not found".to_string(),
            }),
        ))
    }
}

// ---------------------------------------------------------------------------
// Audit Trail endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AuditQuery {
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize)]
struct AuditListResponse {
    count: usize,
    total: usize,
    entries: Vec<serde_json::Value>,
}

/// GET /projects/{id}/audit
async fn list_audit(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<AuditListResponse>, StatusCode> {
    let project_id = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(100);
    let total = state
        .get_audit_count(&project_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let entries = state
        .get_audit(&project_id, offset, limit)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let entry_values: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| serde_json::to_value(e).unwrap_or_default())
        .collect();
    Ok(Json(AuditListResponse {
        count: entry_values.len(),
        total,
        entries: entry_values,
    }))
}

#[derive(Deserialize)]
struct AddAuditRequest {
    action: String,
    detail: String,
}

/// POST /projects/{id}/audit
async fn add_audit(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<AddAuditRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let project_id = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if state
        .append_audit(&project_id, &req.action, &req.detail)
        .await
    {
        Ok(Json(serde_json::json!({ "status": "appended" })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ---------------------------------------------------------------------------
// Stage endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct StageResponse {
    current_stage: Option<String>,
    stage_index: Option<usize>,
    stages: Vec<String>,
    can_advance: bool,
}

/// GET /projects/{id}/stage
async fn get_stage(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<StageResponse>, StatusCode> {
    let project_id = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let project = state
        .get_project(&project_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let stage = project.current_stage.as_ref();
    let all_stages: Vec<String> = crate::project::ProjectStage::all()
        .iter()
        .map(|s| s.name().to_string())
        .collect();
    Ok(Json(StageResponse {
        current_stage: stage.map(|s| s.name().to_string()),
        stage_index: stage.map(|s| s.index()),
        stages: all_stages,
        can_advance: crate::project::ProjectStage::can_advance(&project),
    }))
}

#[derive(Deserialize)]
struct SetStageRequest {
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    advance: Option<bool>,
}

/// POST /projects/{id}/stage
async fn set_stage(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<SetStageRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    // Permission check: require "coordinate" to change stages
    if let Err(e) = state
        .check_permission(
            &project_id,
            state.node_name(),
            Some(state.did()),
            "coordinate",
        )
        .await
    {
        state.append_audit(&project_id, "access.denied", &e).await;
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }

    if req.advance == Some(true) {
        match state.advance_project_stage(&project_id).await {
            Ok(new_stage) => {
                // Broadcast to peers
                use crate::protocol::message::{MessageType, ProjectStagePayload};
                let payload = ProjectStagePayload {
                    project_id,
                    stage: new_stage.name().to_string(),
                    previous_stage: None,
                };
                let body = serde_json::to_string(&payload).unwrap_or_default();
                state
                    .push_outbox(OutboundMessage {
                        body,
                        to: None,
                        id: Uuid::new_v4(),
                        reply_to: None,
                        conversation_id: None,
                        msg_type: Some(MessageType::ProjectStage),
                        project_id: Some(project_id),
                        from_override: None,
                    })
                    .await;
                return Ok(Json(serde_json::json!({
                    "status": "advanced",
                    "stage": new_stage.name(),
                })));
            }
            Err(e) => {
                return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
            }
        }
    }

    if let Some(stage_str) = req.stage {
        let stage: crate::project::ProjectStage = stage_str
            .parse()
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?;
        match state.set_project_stage(&project_id, stage.clone()).await {
            Ok(previous) => {
                state
                    .append_audit(
                        &project_id,
                        "project.stage_changed",
                        &format!(
                            "{} → {}",
                            previous
                                .as_ref()
                                .map(|s| s.name().to_string())
                                .unwrap_or("none".to_string()),
                            stage.name()
                        ),
                    )
                    .await;
                // Broadcast to peers
                use crate::protocol::message::{MessageType, ProjectStagePayload};
                let payload = ProjectStagePayload {
                    project_id,
                    stage: stage.name().to_string(),
                    previous_stage: previous.as_ref().map(|s| s.name().to_string()),
                };
                let body = serde_json::to_string(&payload).unwrap_or_default();
                state
                    .push_outbox(OutboundMessage {
                        body,
                        to: None,
                        id: Uuid::new_v4(),
                        reply_to: None,
                        conversation_id: None,
                        msg_type: Some(MessageType::ProjectStage),
                        project_id: Some(project_id),
                        from_override: None,
                    })
                    .await;
                Ok(Json(serde_json::json!({
                    "status": "set",
                    "stage": stage.name(),
                })))
            }
            Err(e) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e }))),
        }
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Specify 'stage' or 'advance: true'".to_string(),
            }),
        ))
    }
}

// ---------------------------------------------------------------------------
// Agent oversight endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SuspendRequest {
    #[serde(default)]
    reason: Option<String>,
}

/// POST /projects/{id}/agents/{name}/suspend
async fn suspend_agent(
    State(state): State<DaemonState>,
    Path((id, agent_name)): Path<(String, String)>,
    Json(req): Json<SuspendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    let reason = req.reason.clone();
    match state
        .suspend_agent(&project_id, state.node_name(), &agent_name, req.reason)
        .await
    {
        Ok(()) => {
            // Broadcast suspension to peers
            let payload = crate::protocol::message::ProjectSuspendPayload {
                project_id,
                target: agent_name.clone(),
                reason,
            };
            let body = serde_json::to_string(&payload).unwrap_or_default();
            state
                .push_outbox(crate::state::OutboundMessage {
                    id: uuid::Uuid::new_v4(),
                    body,
                    to: None,
                    reply_to: None,
                    conversation_id: None,
                    msg_type: Some(crate::protocol::message::MessageType::ProjectSuspend),
                    project_id: Some(project_id),
                    from_override: None,
                })
                .await;
            Ok(Json(serde_json::json!({
                "status": "suspended",
                "agent": agent_name,
            })))
        }
        Err(e) => Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e }))),
    }
}

/// POST /projects/{id}/agents/{name}/unsuspend
async fn unsuspend_agent(
    State(state): State<DaemonState>,
    Path((id, agent_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    match state
        .unsuspend_agent(&project_id, state.node_name(), &agent_name)
        .await
    {
        Ok(()) => {
            // Broadcast unsuspension to peers
            let payload = crate::protocol::message::ProjectUnsuspendPayload {
                project_id,
                target: agent_name.clone(),
            };
            let body = serde_json::to_string(&payload).unwrap_or_default();
            state
                .push_outbox(crate::state::OutboundMessage {
                    id: uuid::Uuid::new_v4(),
                    body,
                    to: None,
                    reply_to: None,
                    conversation_id: None,
                    msg_type: Some(crate::protocol::message::MessageType::ProjectUnsuspend),
                    project_id: Some(project_id),
                    from_override: None,
                })
                .await;
            Ok(Json(serde_json::json!({
                "status": "unsuspended",
                "agent": agent_name,
            })))
        }
        Err(e) => Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e }))),
    }
}

/// POST /projects/{id}/agents/{name}/role — change an agent's role in a project.
async fn set_agent_role(
    State(state): State<DaemonState>,
    Path((id, agent_name)): Path<(String, String)>,
    Json(req): Json<SetRoleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    let role: crate::project::ProjectRole = req.role.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid role: {}", req.role),
            }),
        )
    })?;

    // Only owner can change roles
    if let Err(e) = state
        .check_permission(
            &project_id,
            state.node_name(),
            Some(state.did()),
            "coordinate",
        )
        .await
    {
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }

    state
        .set_agent_role(&project_id, &agent_name, role)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, Json(ErrorResponse { error: e })))?;

    Ok(Json(serde_json::json!({
        "status": "updated",
        "agent": agent_name,
        "role": req.role,
    })))
}

#[derive(Deserialize)]
struct SetRoleRequest {
    role: String,
}

/// Request body for POST /projects/{id}/agents.
#[derive(Deserialize)]
struct AddAgentRequest {
    name: String,
    role: String,
}

/// POST /projects/{id}/agents — directly add an agent to a project (operator use).
async fn add_project_agent(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<AddAgentRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    let role: ProjectRole = req.role.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid role: {}", req.role),
            }),
        )
    })?;

    // Only owner can add agents directly
    if let Err(e) = state
        .check_permission(
            &project_id,
            state.node_name(),
            Some(state.did()),
            "coordinate",
        )
        .await
    {
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }

    if !state
        .add_project_agent(&project_id, &req.name, None, role)
        .await
    {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!(
                    "Agent '{}' already exists in project or project not found",
                    req.name
                ),
            }),
        ));
    }

    Ok(Json(serde_json::json!({
        "status": "added",
        "name": req.name,
        "role": req.role,
    })))
}

/// DELETE /projects/{id}/agents/{name} — remove an agent from a project.
async fn remove_project_agent(
    State(state): State<DaemonState>,
    Path((id, agent_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    // Only owner can remove agents
    if let Err(e) = state
        .check_permission(
            &project_id,
            state.node_name(),
            Some(state.did()),
            "coordinate",
        )
        .await
    {
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })));
    }

    if !state.remove_project_agent(&project_id, &agent_name).await {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Agent '{}' not found", agent_name),
            }),
        ));
    }

    Ok(Json(serde_json::json!({
        "status": "removed",
        "agent": agent_name,
    })))
}

// --- GitHub integration ---

/// POST /projects/{id}/github/sync — sync tasks with GitHub issues.
async fn github_sync(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    let cfg = crate::github::GitHubConfig::load();
    let token = cfg.token.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "GitHub token not configured. POST /github/config first.".to_string(),
            }),
        )
    })?;

    let repo_url = state
        .get_project_repo(&project_id)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Project not found".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Project has no repository URL".to_string(),
                }),
            )
        })?;

    let (owner, repo) = crate::github::parse_github_repo(&repo_url).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Could not parse GitHub repo from URL".to_string(),
            }),
        )
    })?;

    let existing_tasks = state.get_project_tasks(&project_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Project not found".to_string(),
            }),
        )
    })?;

    let result = crate::github::sync_bidirectional(&token, &owner, &repo, &existing_tasks)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

    // Import new tasks into the project
    if result.imported > 0 {
        let remote_tasks = crate::github::import_issues(&token, &owner, &repo)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse { error: e }),
                )
            })?;
        state.import_github_tasks(&project_id, remote_tasks).await;
    }

    // Update local tasks with their new GitHub issue numbers (prevents re-push)
    for (task_id, issue_number) in &result.pushed_mappings {
        state
            .set_task_github_issue_number(&project_id, task_id, *issue_number)
            .await;
    }

    Ok(Json(serde_json::json!({
        "status": "synced",
        "imported": result.imported,
        "pushed": result.pushed,
        "errors": result.errors,
    })))
}

/// GET /projects/{id}/github/status — GitHub sync status for a project.
async fn github_status(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    let cfg = crate::github::GitHubConfig::load();
    let has_token = cfg.token.is_some();

    let project = state.get_project(&project_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Project not found".to_string(),
            }),
        )
    })?;

    let repo_url = project.repo.clone();
    let parsed = repo_url
        .as_deref()
        .and_then(crate::github::parse_github_repo);
    let tasks = state
        .get_project_tasks(&project_id)
        .await
        .unwrap_or_default();
    let github_tasks = tasks
        .iter()
        .filter(|t| t.github_issue_number.is_some())
        .count();
    let local_only = tasks
        .iter()
        .filter(|t| t.github_issue_number.is_none())
        .count();

    Ok(Json(serde_json::json!({
        "has_token": has_token,
        "repo_url": repo_url,
        "parsed_repo": parsed.map(|(o, r)| format!("{}/{}", o, r)),
        "github_linked_tasks": github_tasks,
        "local_only_tasks": local_only,
    })))
}

/// GET /github/config — get GitHub configuration.
async fn get_github_config() -> Json<serde_json::Value> {
    let cfg = crate::github::GitHubConfig::load();
    Json(serde_json::json!({
        "has_token": cfg.token.is_some(),
        "token_preview": cfg.token.as_ref().map(|t| {
            if t.len() > 8 { format!("{}...{}", &t[..4], &t[t.len()-4..]) } else { "***".to_string() }
        }),
    }))
}

#[derive(Deserialize)]
struct GitHubConfigRequest {
    token: Option<String>,
}

/// POST /github/config — set GitHub token.
async fn set_github_config(
    Json(req): Json<GitHubConfigRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let mut cfg = crate::github::GitHubConfig::load();
    cfg.token = req.token;
    cfg.save().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to save config: {}", e),
            }),
        )
    })?;
    Ok(Json(serde_json::json!({
        "status": "saved",
        "has_token": cfg.token.is_some(),
    })))
}

/// GET /outbox — show outbox queue stats (offline message queue).
async fn get_outbox_stats(State(state): State<DaemonState>) -> Json<serde_json::Value> {
    let stats = state.outbox_stats().await;
    Json(serde_json::json!(stats))
}

// ---------------------------------------------------------------------------
// Marketplace endpoints
// ---------------------------------------------------------------------------

async fn marketplace_search(
    State(state): State<DaemonState>,
    Query(params): Query<crate::marketplace::AgentSearchQuery>,
) -> Json<Vec<crate::marketplace::AgentSearchResult>> {
    Json(state.marketplace_search(&params).await)
}

async fn marketplace_advertise(
    State(state): State<DaemonState>,
    Json(caps): Json<crate::marketplace::AgentCapabilities>,
) -> Json<serde_json::Value> {
    let updated = state.marketplace_upsert(caps).await;
    Json(serde_json::json!({ "status": "ok", "updated": updated }))
}

async fn marketplace_list(
    State(state): State<DaemonState>,
) -> Json<Vec<crate::marketplace::AgentCapabilities>> {
    Json(state.marketplace_list().await)
}

// ---------------------------------------------------------------------------
// Discovery endpoints (gossip network)
// ---------------------------------------------------------------------------

/// GET /discovery/agents — list all agents discovered via gossip
async fn discovery_list_agents(State(state): State<DaemonState>) -> Json<serde_json::Value> {
    let agents = state.discovery_list().await;
    let friends = state.get_friends().await;
    let friend_names: std::collections::HashSet<String> =
        friends.iter().map(|f| f.name.clone()).collect();

    let entries: Vec<serde_json::Value> = agents
        .iter()
        .map(|a| {
            let trust_level = crate::discovery::trust_score_to_level(a.effective_trust);
            serde_json::json!({
                "did": a.did,
                "name": a.name,
                "domains": a.capabilities.as_ref().map(|c| &c.domains).cloned().unwrap_or_default(),
                "tools": a.capabilities.as_ref().map(|c| &c.tools).cloned().unwrap_or_default(),
                "availability": a.capabilities.as_ref().map(|c| match c.availability {
                    crate::marketplace::AgentAvailability::Available => "available",
                    crate::marketplace::AgentAvailability::Busy => "busy",
                    crate::marketplace::AgentAvailability::Offline => "offline",
                }).unwrap_or("unknown"),
                "description": a.capabilities.as_ref().and_then(|c| c.description.clone()),
                "effective_trust": a.effective_trust,
                "trust_level": trust_level,
                "trust_level_name": crate::discovery::trust_level_name(trust_level),
                "discovery_method": match &a.discovery_path {
                    crate::discovery::DiscoveryPath::Direct => "direct",
                    crate::discovery::DiscoveryPath::Introduction { .. } => "introduction",
                    crate::discovery::DiscoveryPath::Gossip { .. } => "gossip",
                },
                "is_friend": friend_names.contains(&a.name),
                "verified": a.signed_capabilities.as_ref().map(|s| s.verify()).unwrap_or(false),
                "first_seen": a.first_seen.to_rfc3339(),
                "last_refreshed": a.last_refreshed.to_rfc3339(),
                "last_address": a.last_address,
                "owner_did": a.owner_did,
            })
        })
        .collect();

    Json(serde_json::json!({ "count": entries.len(), "agents": entries }))
}

/// GET /discovery/search?query=...
async fn discovery_search(
    State(state): State<DaemonState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let query = params.get("query").cloned().unwrap_or_default();
    let agents = state.discovery_search(&query).await;
    let entries: Vec<serde_json::Value> = agents
        .iter()
        .map(|a| {
            serde_json::json!({
                "did": a.did,
                "name": a.name,
                "domains": a.capabilities.as_ref().map(|c| &c.domains).cloned().unwrap_or_default(),
                "tools": a.capabilities.as_ref().map(|c| &c.tools).cloned().unwrap_or_default(),
                "effective_trust": a.effective_trust,
                "trust_level": crate::discovery::trust_score_to_level(a.effective_trust),
            })
        })
        .collect();
    Json(serde_json::json!({ "count": entries.len(), "results": entries }))
}

/// GET /discovery/agent/{did}
async fn discovery_get_agent(
    State(state): State<DaemonState>,
    Path(did): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    match state.discovery_get(&did).await {
        Some(a) => Ok(Json(serde_json::json!({
            "did": a.did,
            "name": a.name,
            "capabilities": a.capabilities,
            "discovery_path": a.discovery_path,
            "effective_trust": a.effective_trust,
            "verified": a.signed_capabilities.as_ref().map(|s| s.verify()).unwrap_or(false),
            "first_seen": a.first_seen.to_rfc3339(),
            "last_refreshed": a.last_refreshed.to_rfc3339(),
            "last_address": a.last_address,
            "owner_did": a.owner_did,
        }))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// GET /discovery/projects — list project advertisements
async fn discovery_list_project_ads(State(state): State<DaemonState>) -> Json<serde_json::Value> {
    let ads = state.discovery_project_ads().await;
    Json(serde_json::json!({ "count": ads.len(), "projects": ads }))
}

/// GET /discovery/stats
async fn discovery_stats(
    State(state): State<DaemonState>,
) -> Json<crate::discovery::DiscoveryStats> {
    Json(state.discovery_stats().await)
}

// ---------------------------------------------------------------------------
// Reputation endpoints
// ---------------------------------------------------------------------------

async fn get_reputation(
    State(state): State<DaemonState>,
    Path(name): Path<String>,
) -> Json<crate::reputation::AgentReputation> {
    Json(state.reputation_get(&name).await)
}

async fn reputation_leaderboard(
    State(state): State<DaemonState>,
) -> Json<Vec<crate::reputation::AgentReputation>> {
    Json(state.reputation_leaderboard().await)
}

async fn reputation_recommendations(
    State(state): State<DaemonState>,
) -> Json<Vec<crate::reputation::TrustRecommendation>> {
    Json(state.reputation_recommendations().await)
}

// ---------------------------------------------------------------------------
// Coordinator endpoints
// ---------------------------------------------------------------------------

async fn coordinator_suggestions(
    State(state): State<DaemonState>,
    Path(id): Path<Uuid>,
) -> Json<Vec<crate::coordinator::CoordinatorSuggestion>> {
    // Run coordinator analysis on the project
    let projects = state.get_projects().await;
    if let Some(project) = projects.iter().find(|p| p.id == id) {
        let stage_name = project
            .current_stage
            .as_ref()
            .map(|s| s.name().to_lowercase())
            .unwrap_or_else(|| "investigation".to_string());
        let snapshot = crate::coordinator::ProjectSnapshot {
            id: project.id,
            name: project.name.clone(),
            stage: stage_name,
            agents: project
                .agents
                .iter()
                .map(|a| crate::coordinator::AgentSnapshot {
                    name: a.name.clone(),
                    role: a.role.name().to_lowercase(),
                    clocked_in: a.clocked_in,
                })
                .collect(),
            tasks: project
                .tasks
                .iter()
                .map(|t| {
                    let status = match t.status {
                        crate::project::TaskStatus::Todo => "todo",
                        crate::project::TaskStatus::InProgress => "in_progress",
                        crate::project::TaskStatus::Done => "done",
                        crate::project::TaskStatus::Blocked => "blocked",
                    };
                    crate::coordinator::TaskSnapshot {
                        id: t.id,
                        title: t.title.clone(),
                        status: status.to_string(),
                        assignee: t.assignee.clone(),
                        depends_on: t.depends_on.clone(),
                    }
                })
                .collect(),
        };
        let config = crate::coordinator::CoordinatorConfig::default();
        let suggestions = crate::coordinator::analyze_project(&snapshot, &config);
        if !suggestions.is_empty() {
            state.coordinator_add_suggestions(id, suggestions).await;
        }
    }
    Json(state.coordinator_suggestions(&id).await)
}

#[derive(Deserialize)]
struct CoordinatorActRequest {
    suggestion_id: Uuid,
}

async fn coordinator_act(
    State(state): State<DaemonState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CoordinatorActRequest>,
) -> Json<serde_json::Value> {
    let acted = state.coordinator_act(&id, &req.suggestion_id).await;
    Json(serde_json::json!({ "status": if acted { "ok" } else { "not_found" } }))
}

async fn coordinator_digest(
    State(state): State<DaemonState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let projects = state.get_projects().await;
    if let Some(project) = projects.iter().find(|p| p.id == id) {
        let digest = crate::coordinator::generate_digest(
            id,
            &project.name,
            chrono::Utc::now() - chrono::Duration::hours(24),
            project
                .tasks
                .iter()
                .filter(|t| matches!(t.status, crate::project::TaskStatus::Done))
                .map(|t| t.title.clone())
                .collect(),
            project.tasks.iter().map(|t| t.title.clone()).collect(),
            project
                .tasks
                .iter()
                .filter(|t| matches!(t.status, crate::project::TaskStatus::Blocked))
                .map(|t| t.title.clone())
                .collect(),
            project
                .agents
                .iter()
                .filter(|a| a.clocked_in)
                .map(|a| a.name.clone())
                .collect(),
            vec![],
        );
        state.coordinator_add_digest(id, digest.clone()).await;
        Json(serde_json::json!({ "status": "ok", "digest": digest }))
    } else {
        Json(serde_json::json!({ "status": "not_found" }))
    }
}

async fn coordinator_digests(
    State(state): State<DaemonState>,
    Path(id): Path<Uuid>,
) -> Json<Vec<crate::coordinator::ProjectDigest>> {
    Json(state.coordinator_digests(&id).await)
}

async fn coordinator_status(
    State(state): State<DaemonState>,
    Path(id): Path<Uuid>,
) -> Json<serde_json::Value> {
    let suggestions = state.coordinator_suggestions(&id).await;
    let digests = state.coordinator_digests(&id).await;
    Json(serde_json::json!({
        "project_id": id,
        "total_suggestions": suggestions.len(),
        "pending_suggestions": suggestions.iter().filter(|s| !s.acted_on).count(),
        "total_digests": digests.len(),
    }))
}

// ---------------------------------------------------------------------------
// Project Rooms endpoints
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProjectRoomEntry {
    room_id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    conversation_id: String,
    created_at: String,
    created_by: String,
}

#[derive(Serialize)]
struct ProjectRoomsResponse {
    project_id: String,
    count: usize,
    rooms: Vec<ProjectRoomEntry>,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    name: String,
    #[serde(default)]
    topic: Option<String>,
}

#[derive(Serialize)]
struct CreateRoomResponse {
    room_id: String,
    name: String,
    conversation_id: String,
}

#[derive(Deserialize)]
struct RoomSendRequest {
    body: String,
    /// Optional sender override (for multi-agent daemons where MCP consumers have distinct names).
    #[serde(default)]
    from: Option<String>,
}

/// GET /projects/{id}/rooms — list all rooms for a project.
async fn list_project_rooms(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
) -> Result<Json<ProjectRoomsResponse>, StatusCode> {
    let uuid = Uuid::parse_str(&id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let rooms = state
        .get_project_rooms(&uuid)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(ProjectRoomsResponse {
        project_id: id,
        count: rooms.len(),
        rooms: rooms
            .into_iter()
            .map(|r| ProjectRoomEntry {
                room_id: r.id.to_string(),
                name: r.name,
                topic: r.topic,
                conversation_id: r.conversation_id.to_string(),
                created_at: r.created_at.to_rfc3339(),
                created_by: r.created_by,
            })
            .collect(),
    }))
}

/// POST /projects/{id}/rooms — create a breakout room.
async fn create_project_room(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, (StatusCode, Json<ErrorResponse>)> {
    let uuid = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    if let Err(e) = crate::state::validate_name(&req.name, "Room name", 100) {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
    }

    let caller = state.node_name().to_string();
    let room = state
        .create_project_room(&uuid, &req.name, req.topic, &caller)
        .await
        .map_err(|e| (StatusCode::CONFLICT, Json(ErrorResponse { error: e })))?;

    state
        .append_audit(
            &uuid,
            "room.created",
            &format!("{} created room '{}'", caller, room.name),
        )
        .await;

    Ok(Json(CreateRoomResponse {
        room_id: room.id.to_string(),
        name: room.name,
        conversation_id: room.conversation_id.to_string(),
    }))
}

/// POST /projects/{id}/rooms/{room_id}/send — send a message in a room.
async fn send_to_room(
    State(state): State<DaemonState>,
    Path((id, room_id_str)): Path<(String, String)>,
    Json(req): Json<RoomSendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;
    let room_id = Uuid::parse_str(&room_id_str).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid room ID".to_string(),
            }),
        )
    })?;

    // Use from override if provided and it's a registered consumer, otherwise daemon name
    let caller = resolve_sender_name(&state, req.from.as_ref()).await;

    // Check if the agent is muted
    match state.is_agent_muted(&project_id, &caller).await {
        Some(true) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "You are muted in this project".to_string(),
                }),
            ));
        }
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Project not found or you are not a member".to_string(),
                }),
            ));
        }
        _ => {}
    }

    let room = state
        .get_project_room(&project_id, &room_id)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Room not found".to_string(),
                }),
            )
        })?;

    let msg_id = Uuid::new_v4();
    let conversation_id = room.conversation_id;
    let from_override = if caller != state.node_name() {
        Some(caller.as_str())
    } else {
        None
    };

    // Store in conversation history with correct sender
    state
        .store_outbound_from(
            &req.body,
            None,
            msg_id,
            None,
            Some(conversation_id),
            Some(project_id),
            from_override,
        )
        .await;

    // Deliver to all project agent consumers locally
    let agents = state.get_project_agent_names(&project_id).await;
    for agent_name in &agents {
        if *agent_name != caller {
            state
                .deliver_to_local_consumer(
                    agent_name,
                    &OutboundMessage {
                        body: req.body.clone(),
                        to: Some(agent_name.clone()),
                        id: msg_id,
                        reply_to: None,
                        conversation_id: Some(conversation_id),
                        msg_type: None,
                        project_id: Some(project_id),
                        from_override: Some(caller.clone()),
                    },
                )
                .await;
        }
    }

    // Also push to outbox for remote peers
    state
        .push_outbox(OutboundMessage {
            body: req.body,
            to: None,
            id: msg_id,
            reply_to: None,
            conversation_id: Some(conversation_id),
            msg_type: None,
            project_id: Some(project_id),
            from_override: from_override.map(|s| s.to_string()),
        })
        .await;

    Ok(Json(serde_json::json!({
        "status": "sent",
        "id": msg_id.to_string(),
        "room_id": room_id_str,
        "conversation_id": conversation_id.to_string(),
    })))
}

/// POST /projects/{id}/rooms/main/send — send a message to the main room.
async fn send_to_main_room(
    State(state): State<DaemonState>,
    Path(id): Path<String>,
    Json(req): Json<RoomSendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    // Use from override if provided and it's a registered consumer, otherwise daemon name
    let caller = resolve_sender_name(&state, req.from.as_ref()).await;

    // Check if the agent is muted
    match state.is_agent_muted(&project_id, &caller).await {
        Some(true) => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(ErrorResponse {
                    error: "You are muted in this project".to_string(),
                }),
            ));
        }
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Project not found or you are not a member".to_string(),
                }),
            ));
        }
        _ => {}
    }

    let room = state.get_main_room(&project_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Main room not found".to_string(),
            }),
        )
    })?;

    let msg_id = Uuid::new_v4();
    let conversation_id = room.conversation_id;
    let from_override = if caller != state.node_name() {
        Some(caller.as_str())
    } else {
        None
    };

    // Store in conversation history with correct sender
    state
        .store_outbound_from(
            &req.body,
            None,
            msg_id,
            None,
            Some(conversation_id),
            Some(project_id),
            from_override,
        )
        .await;

    // Push to outbox for delivery to all project peers
    state
        .push_outbox(OutboundMessage {
            body: req.body,
            to: None,
            id: msg_id,
            reply_to: None,
            conversation_id: Some(conversation_id),
            msg_type: None,
            project_id: Some(project_id),
            from_override: from_override.map(|s| s.to_string()),
        })
        .await;

    Ok(Json(serde_json::json!({
        "status": "sent",
        "id": msg_id.to_string(),
        "room_name": "main",
        "conversation_id": conversation_id.to_string(),
    })))
}

// ---------------------------------------------------------------------------
// Mute / Unmute endpoints
// ---------------------------------------------------------------------------

/// POST /projects/{id}/agents/{name}/mute — mute an agent in a project.
async fn mute_agent(
    State(state): State<DaemonState>,
    Path((id, agent_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    let caller = state.node_name().to_string();
    state
        .mute_agent(&project_id, &caller, &agent_name)
        .await
        .map_err(|e| (StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })))?;

    Ok(Json(serde_json::json!({
        "status": "muted",
        "agent": agent_name,
    })))
}

/// POST /projects/{id}/agents/{name}/unmute — unmute an agent in a project.
async fn unmute_agent(
    State(state): State<DaemonState>,
    Path((id, agent_name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let project_id = Uuid::parse_str(&id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid project ID".to_string(),
            }),
        )
    })?;

    let caller = state.node_name().to_string();
    state
        .unmute_agent(&project_id, &caller, &agent_name)
        .await
        .map_err(|e| (StatusCode::FORBIDDEN, Json(ErrorResponse { error: e })))?;

    Ok(Json(serde_json::json!({
        "status": "unmuted",
        "agent": agent_name,
    })))
}
