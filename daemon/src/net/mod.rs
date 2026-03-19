pub mod tls;
pub mod ws;

use anyhow::{Context, Result};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::{error, info, warn};

use uuid::Uuid;

use crate::identity::AgentIdentity;
use crate::protocol::framing::{recv_message, send_message};
use crate::protocol::message::{Message, MessageType};
use crate::state::{DaemonState, OutboundMessage, PeerInfo, RegisterResult};

/// Sign a message with the agent's Ed25519 key and send it.
/// Hello messages keep their own signing path; all others get signed here.
async fn sign_and_send<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &mut Message,
    identity: &AgentIdentity,
) -> Result<()> {
    if msg.msg_type != MessageType::Hello {
        let sig = identity.sign(msg.body.as_bytes());
        msg.signature = Some(bs58::encode(&sig).into_string());
    }
    send_message(writer, msg).await
}

/// Start the Agora daemon: listen for incoming TLS connections.
/// Messages are routed through DaemonState instead of stdin.
pub async fn start_listener(state: DaemonState, address: &str, port: u16) -> Result<()> {
    let (cert, key) =
        tls::generate_self_signed_cert().context("Failed to generate TLS certificate")?;
    let tls_config =
        tls::build_server_config(cert, key).context("Failed to build TLS server config")?;
    let acceptor = TlsAcceptor::from(tls_config);

    let bind_addr = format!("{}:{}", address, port);
    let listener = TcpListener::bind(&bind_addr)
        .await
        .context(format!("Failed to bind to {}", bind_addr))?;

    info!("Agora daemon listening on {}", bind_addr);

    loop {
        let (tcp_stream, peer_addr) = listener
            .accept()
            .await
            .context("Failed to accept connection")?;
        info!("TCP connection from {}", peer_addr);

        let acceptor = acceptor.clone();
        let state = state.clone();

        tokio::spawn(async move {
            match acceptor.accept(tcp_stream).await {
                Ok(tls_stream) => {
                    info!("TLS handshake complete with {}", peer_addr);
                    let (reader, writer) = tokio::io::split(tls_stream);
                    if let Err(e) =
                        handle_connection(reader, writer, &state, &peer_addr.to_string()).await
                    {
                        error!("Connection error with {}: {}", peer_addr, e);
                    }
                    state.remove_peer(&peer_addr.to_string()).await;
                    info!("Peer {} disconnected", peer_addr);
                }
                Err(e) => {
                    error!("TLS handshake failed with {}: {}", peer_addr, e);
                }
            }
        });
    }
}

/// Try a single connection attempt to a remote Agora node over TLS.
async fn try_connect_once(
    state: &DaemonState,
    target: &str,
    connector: &TlsConnector,
) -> Result<()> {
    let tcp_stream = TcpStream::connect(target)
        .await
        .context(format!("Failed to connect to {}", target))?;
    info!("TCP connection established to {}", target);

    let server_name =
        ServerName::try_from("localhost".to_string()).context("Invalid server name")?;

    let tls_stream = connector
        .connect(server_name, tcp_stream)
        .await
        .context("TLS handshake failed")?;
    info!("TLS handshake complete with {}", target);

    let (reader, writer) = tokio::io::split(tls_stream);
    let result = handle_connection(reader, writer, state, target).await;
    state.remove_peer(target).await;
    result
}

/// Connect to a remote Agora node over TLS with auto-reconnect.
/// Retries with exponential backoff (1s → 2s → 4s → ... → 60s cap).
/// On connection loss after a successful session, resets backoff to 1s.
pub async fn connect_to_peer(state: DaemonState, target: &str) -> Result<()> {
    let tls_config = tls::build_client_config().context("Failed to build TLS client config")?;
    let connector = TlsConnector::from(tls_config);

    let mut backoff = std::time::Duration::from_secs(1);
    let max_backoff = std::time::Duration::from_secs(60);
    let mut was_connected = false;

    loop {
        // Check if this address was explicitly disconnected — stop reconnecting
        if state.is_disconnected(target).await {
            info!(
                "Stopped reconnecting to {} (explicitly disconnected)",
                target
            );
            state.clear_disconnected(target).await;
            return Ok(());
        }

        // Skip if already connected to this address
        if state.is_peer_connected_by_addr(target).await {
            info!("Already connected to {} — skipping", target);
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }

        info!("Connecting to {}...", target);

        match try_connect_once(&state, target, &connector).await {
            Ok(()) => {
                // Clean disconnect — peer closed gracefully
                info!("Connection to {} closed, reconnecting...", target);
                was_connected = true;
                backoff = std::time::Duration::from_secs(1);
            }
            Err(e) => {
                if was_connected {
                    warn!("Connection lost to {}: {}. Reconnecting...", target, e);
                    backoff = std::time::Duration::from_secs(1);
                    was_connected = false;
                } else {
                    warn!(
                        "Failed to connect to {}: {}. Retrying in {:?}...",
                        target, e, backoff
                    );
                }
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

/// Handle a live connection: exchange Hello, then route messages via DaemonState.
async fn handle_connection<R, W>(
    mut reader: R,
    mut writer: W,
    state: &DaemonState,
    peer_addr: &str,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let node_name = state.node_name().to_string();

    // Send Hello with cryptographic identity + owner attestation
    let hello =
        Message::hello_with_identity(&node_name, state.identity(), state.owner_attestation());
    send_message(&mut writer, &hello).await?;
    info!("Sent Hello to {} (DID: {})", peer_addr, state.did());

    // Receive Hello
    let (
        peer_name,
        peer_did,
        peer_public_key_b58,
        peer_session_id,
        peer_verified,
        peer_owner_did,
        peer_owner_verified,
    ) = match recv_message(&mut reader).await? {
        Some(msg) if msg.msg_type == MessageType::Hello => {
            // Verify agent identity if present
            let verified = if msg.did.is_some() && msg.public_key.is_some() {
                if msg.verify_signature() {
                    info!(
                        "Verified identity: {} (DID: {}, session: {})",
                        msg.from,
                        msg.did.as_deref().unwrap_or("?"),
                        msg.session_id.map(|s| s.to_string()).unwrap_or_default()
                    );
                    true
                } else {
                    warn!(
                        "INVALID signature from {} ({}) — REJECTING connection",
                        msg.from, peer_addr
                    );
                    let close = Message::close(&node_name);
                    let _ = send_message(&mut writer, &close).await;
                    return Ok(());
                }
            } else {
                info!(
                    "Received Hello from {} ({}) — no identity (legacy peer)",
                    msg.from, peer_addr
                );
                false
            };

            // Verify owner attestation if present
            let (owner_did, owner_verified) = if let Some(ref att) = msg.owner_attestation {
                let agent_did = msg.did.as_deref().unwrap_or("");
                if att.verify_for_agent(agent_did) {
                    info!(
                        "Verified owner attestation: {} owns {} (owner: {})",
                        att.owner_did, msg.from, att.owner_did
                    );
                    (Some(att.owner_did.clone()), true)
                } else {
                    warn!(
                        "INVALID owner attestation from {} — ignoring owner claim (connection continues)",
                        msg.from
                    );
                    (None, false)
                }
            } else {
                (msg.owner_did.clone(), false)
            };

            (
                msg.from.clone(),
                msg.did.clone(),
                msg.public_key.clone(),
                msg.session_id,
                verified,
                owner_did,
                owner_verified,
            )
        }
        Some(msg) => {
            warn!("Expected Hello, got {:?} from {}", msg.msg_type, peer_addr);
            ("unknown".to_string(), None, None, None, false, None, false)
        }
        None => {
            info!("Peer {} disconnected before Hello", peer_addr);
            return Ok(());
        }
    };

    // DID pinning + duplicate detection
    if let Some(ref did) = peer_did {
        // Check DID against stored pin (TOFU — Trust On First Use)
        use crate::state::DidPinResult;
        match state.check_and_pin_did(&peer_name, did).await {
            DidPinResult::FirstSeen => {
                info!("TOFU: pinned DID for friend {}", peer_name);
            }
            DidPinResult::Match => {
                info!("DID matches pin for friend {}", peer_name);
            }
            DidPinResult::Mismatch { expected } => {
                warn!(
                    "DID MISMATCH for {} — expected {}, got {} — REJECTING",
                    peer_name, expected, did
                );
                let close = Message::close(&node_name);
                let _ = send_message(&mut writer, &close).await;
                return Ok(());
            }
            DidPinResult::NotAFriend => {
                // Not a friend — no pinning, just store for display
            }
        }
        // Merge any duplicates sharing the same DID
        if let Some(merge_msg) = state.merge_friend_by_did(&peer_name, did).await {
            info!("{}", merge_msg);
        }
    }

    // Owner DID pinning + auto-trust
    if let Some(ref owner_did) = peer_owner_did {
        if peer_owner_verified {
            // Pin owner DID on existing friend (TOFU)
            let trust_before = state.get_trust_level(&peer_name).await;
            if trust_before.0 > 0 {
                use crate::state::DidPinResult;
                match state.check_and_pin_owner_did(&peer_name, owner_did).await {
                    DidPinResult::FirstSeen => {
                        info!("TOFU: pinned owner DID for friend {}", peer_name);
                    }
                    DidPinResult::Match => {
                        info!("Owner DID matches pin for friend {}", peer_name);
                    }
                    DidPinResult::Mismatch { expected } => {
                        warn!(
                            "Owner DID MISMATCH for {} — expected {}, got {} — ignoring owner claim",
                            peer_name, expected, owner_did
                        );
                    }
                    DidPinResult::NotAFriend => {}
                }
            } else {
                // Unknown peer with verified owner — check if owner matches a known friend
                let owner_trust = state.owner_trust_level(owner_did).await;
                if owner_trust.0 > 0 {
                    // Auto-trust: cap at min(owner_trust, 3) — never auto-grant Inner Circle
                    let auto_trust = crate::state::TrustLevel(owner_trust.0.min(3));
                    info!(
                        "Auto-trusting peer {} via owner {} (trust {} → auto {})",
                        peer_name, owner_did, owner_trust, auto_trust
                    );
                    let friend = crate::state::Friend {
                        name: peer_name.clone(),
                        alias: None,
                        trust_level: auto_trust,
                        added_at: chrono::Utc::now(),
                        notes: Some(format!("Auto-trusted via owner {}", owner_did)),
                        muted: false,
                        last_address: Some(peer_addr.to_string()),
                        did: peer_did.clone(),
                        owner_did: Some(owner_did.clone()),
                        their_trust: None,
                    };
                    let _ = state.add_friend(friend).await;
                }
            }

            // Store owner_did on friend record
            state.update_friend_owner_did(&peer_name, owner_did).await;
        }
    }

    // Check connection policy — reject peers below min_trust
    // (done AFTER merge and owner auto-trust so the updated trust level is used)
    let trust = state.get_trust_level(&peer_name).await;
    let min_trust = state.min_trust();
    if trust.0 < min_trust {
        warn!(
            "Rejecting peer {} ({}) — trust {} < min_trust {}",
            peer_name, peer_addr, trust.0, min_trust
        );
        let close = Message::close(state.node_name());
        let _ = send_message(&mut writer, &close).await;
        return Ok(());
    }

    // Register the peer
    let disconnect = std::sync::Arc::new(tokio::sync::Notify::new());
    let disconnect_signal = disconnect.clone();
    let register_result = state
        .add_peer(PeerInfo {
            name: peer_name.clone(),
            address: peer_addr.to_string(),
            connected_at: chrono::Utc::now(),
            did: peer_did.clone(),
            session_id: peer_session_id,
            verified: peer_verified,
            owner_did: peer_owner_did.clone(),
            owner_verified: peer_owner_verified,
            disconnect,
        })
        .await;

    // If this is a duplicate connection (same session_id), close gracefully
    if register_result == RegisterResult::Duplicate {
        info!(
            "Duplicate connection from {} ({}) — same session, closing",
            peer_name, peer_addr
        );
        let mut close = Message::close(&node_name);
        let _ = sign_and_send(&mut writer, &mut close, state.identity()).await;
        return Ok(());
    }

    if trust.0 == 0 {
        // Check if this unknown peer's name is similar to an existing friend
        // (e.g., "alice-desktop" connecting when "alice" is a friend).
        let similar = state.find_similar_friend(&peer_name).await;
        if let Some((friend_name, friend_trust)) = similar {
            info!(
                "Auto-linking unknown peer '{}' to existing friend '{}' (trust {}) — \
                 names match, setting alias for future recognition",
                peer_name, friend_name, friend_trust
            );
            // Auto-set alias on the existing friend so future lookups work
            state.set_friend_alias(&friend_name, &peer_name).await;
            // Also store the DID on the existing friend
            if let Some(ref did) = peer_did {
                state.update_friend_did(&friend_name, did).await;
            }
            // Update address on the existing friend
            state.update_friend_address(&friend_name, peer_addr).await;
        } else {
            warn!(
                "Unknown peer connected: {} ({}) — not in friend list",
                peer_name, peer_addr
            );
        }
    } else {
        info!(
            "Friend connected: {} ({}) — trust level {}",
            peer_name, peer_addr, trust
        );
        // Update last known address for auto-connect
        state.update_friend_address(&peer_name, peer_addr).await;
    }

    info!("Peer registered: {} ({})", peer_name, peer_addr);

    // Replay queued offline messages for this peer
    {
        let pending = state.outbox_pending_for(&peer_name).await;
        if !pending.is_empty() {
            info!(
                "Replaying {} queued message(s) to {}",
                pending.len(),
                peer_name
            );
            for queued in &pending {
                let mut msg = Message::text(&node_name, &queued.body);
                msg.id = queued.id;
                msg.reply_to = queued.reply_to;
                msg.conversation_id = queued.conversation_id;
                if sign_and_send(&mut writer, &mut msg, state.identity())
                    .await
                    .is_err()
                {
                    warn!(
                        "Failed to replay queued message {} to {}",
                        queued.id, peer_name
                    );
                    break;
                }
            }
        }
    }

    // Re-send pending outbound friend request if we have one for this peer
    if let Some(pending_req) = state.get_pending_outbound_to(&peer_name).await {
        use crate::protocol::message::FriendRequestPayload;
        let payload = FriendRequestPayload {
            did: state.did().to_string(),
            public_key: state.identity().public_key_base58(),
            trust_level: pending_req.offered_trust,
            message: pending_req.message.clone(),
            node_name: node_name.clone(),
            owner_did: state.owner_did().map(|s| s.to_string()),
        };
        let mut msg = Message::friend_request(&node_name, &payload);
        if sign_and_send(&mut writer, &mut msg, state.identity())
            .await
            .is_ok()
        {
            info!("Re-sent pending friend request to {}", peer_name);
        }
    }

    // Subscribe to outbox broadcast — this peer gets its own copy of every message
    let mut outbox_rx = state.subscribe_outbox();

    // Message loop: receive from peer → inbox, outbox broadcast → send to peer
    loop {
        tokio::select! {
            // Local disconnect request
            _ = disconnect_signal.notified() => {
                info!("Disconnecting {} by local request", peer_name);
                let mut close = Message::close(&node_name);
                let _ = sign_and_send(&mut writer, &mut close, state.identity()).await;
                break;
            }

            // Incoming message from remote peer
            result = recv_message(&mut reader) => {
                match result? {
                    Some(msg) => {
                        // Verify per-message signature using peer's public key from Hello
                        if let Some(ref peer_pk) = peer_public_key_b58 {
                            if let Some(ref sig_b58) = msg.signature {
                                if !AgentIdentity::verify_base58(peer_pk, msg.body.as_bytes(), sig_b58) {
                                    warn!("INVALID signature on {:?} from {} — DROPPING", msg.msg_type, msg.from);
                                    continue;
                                }
                            } else {
                                // Peer provided a public key in Hello but this message is unsigned.
                                // A MITM could inject unsigned messages — reject.
                                warn!(
                                    "UNSIGNED message from authenticated peer {} (has pubkey) — DROPPING",
                                    msg.from
                                );
                                continue;
                            }
                        }
                        match msg.msg_type {
                            MessageType::Message => {
                                // Inbound dedup: skip if we've already seen this message
                                if state.outbox_is_seen(&msg.id).await {
                                    info!("Skipping duplicate message {} from {}", msg.id, msg.from);
                                    continue;
                                }
                                state.outbox_mark_seen(msg.id).await;

                                info!("[{}] {}", msg.from, msg.body);
                                state.push_inbox(msg.clone()).await;

                                // Send delivery acknowledgement
                                let mut ack = Message::ack(&node_name, msg.id);
                                let _ = sign_and_send(&mut writer, &mut ack, state.identity()).await;
                            }
                            MessageType::Ack => {
                                // Process delivery acknowledgement — dequeue from outbox
                                use crate::protocol::message::AckPayload;
                                if let Some(payload) = msg.parse_payload::<AckPayload>() {
                                    if state.outbox_ack(&msg.from, &payload.message_id).await {
                                        info!("Message {} acked by {}", payload.message_id, msg.from);
                                    }
                                }
                            }
                            MessageType::Close => {
                                info!("{} disconnected gracefully.", msg.from);
                                break;
                            }
                            _ if msg.msg_type.is_thread() => {
                                use crate::protocol::message::{ThreadCreatePayload, ThreadUpdatePayload, ThreadClosePayload};
                                match msg.msg_type {
                                    MessageType::ThreadCreate => {
                                        if let Some(payload) = msg.parse_payload::<ThreadCreatePayload>() {
                                            info!("[{}] thread.create: {:?} ({})", msg.from,
                                                payload.title.as_deref().unwrap_or("untitled"),
                                                payload.conversation_id);
                                            let _ = state.create_thread(
                                                Some(payload.conversation_id),
                                                &msg.from,
                                                payload.title,
                                                payload.participants,
                                                payload.min_trust,
                                                payload.closed,
                                                payload.metadata,
                                            ).await;
                                        }
                                    }
                                    MessageType::ThreadMessage => {
                                        // Thread messages are just regular messages with conversation_id
                                        info!("[{}] thread.message: {}", msg.from,
                                            msg.body.chars().take(100).collect::<String>());
                                        state.push_inbox(msg.clone()).await;
                                    }
                                    MessageType::ThreadUpdate => {
                                        if let Some(payload) = msg.parse_payload::<ThreadUpdatePayload>() {
                                            info!("[{}] thread.update: {}", msg.from, payload.conversation_id);
                                            // Add participants
                                            for name in &payload.add_participants {
                                                let trust = state.get_trust_level(name).await;
                                                let _ = state.thread_add_participant(
                                                    &payload.conversation_id, &msg.from, name, trust.0
                                                ).await;
                                            }
                                            // Remove participants
                                            for name in &payload.remove_participants {
                                                let _ = state.thread_remove_participant(
                                                    &payload.conversation_id, &msg.from, name
                                                ).await;
                                            }
                                            // Update metadata/title
                                            if payload.title.is_some() || payload.metadata.is_some() {
                                                let _ = state.update_thread(
                                                    &payload.conversation_id, &msg.from,
                                                    payload.title, payload.metadata
                                                ).await;
                                            }
                                        }
                                    }
                                    MessageType::ThreadClose => {
                                        if let Some(payload) = msg.parse_payload::<ThreadClosePayload>() {
                                            info!("[{}] thread.close: {}", msg.from, payload.conversation_id);
                                            let _ = state.close_thread(
                                                &payload.conversation_id, &msg.from, payload.reason
                                            ).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            _ if msg.msg_type.is_friend() => {
                                use crate::protocol::message::{
                                    FriendRequestPayload, FriendAcceptPayload,
                                    FriendRejectPayload, FriendRevokePayload,
                                };
                                use crate::state::{
                                    FriendRequest as FReq, FriendRequestDirection,
                                    FriendRequestStatus,
                                };
                                match msg.msg_type {
                                    MessageType::FriendRequest => {
                                        if let Some(payload) = msg.parse_payload::<FriendRequestPayload>() {
                                            info!(
                                                "[{}] friend.request: trust={}, msg={:?}",
                                                msg.from, payload.trust_level,
                                                payload.message.as_deref().unwrap_or("")
                                            );

                                            // Verify DID matches Hello DID
                                            if let Some(ref hello_did) = peer_did {
                                                if payload.did != *hello_did {
                                                    warn!(
                                                        "Friend request DID mismatch from {} — payload: {}, hello: {} — ignoring",
                                                        msg.from, payload.did, hello_did
                                                    );
                                                    continue;
                                                }
                                            }

                                            // Check for crossed requests: we have pending outbound to them
                                            if let Some(outbound) = state.get_pending_outbound_to(&msg.from).await {
                                                // Auto-resolve: both want to be friends
                                                info!(
                                                    "Crossed friend request detected with {} — auto-resolving",
                                                    msg.from
                                                );
                                                // Accept their request: add them as friend
                                                let friend = crate::state::Friend {
                                                    name: msg.from.clone(),
                                                    alias: None,
                                                    trust_level: crate::state::TrustLevel(outbound.offered_trust.min(4)),
                                                    added_at: chrono::Utc::now(),
                                                    notes: None,
                                                    muted: false,
                                                    last_address: Some(peer_addr.to_string()),
                                                    did: peer_did.clone(),
                                                    owner_did: peer_owner_did.clone(),
                                                    their_trust: Some(payload.trust_level),
                                                };
                                                let _ = state.add_friend(friend).await;
                                                // Resolve our outbound request
                                                state.resolve_outbound_request(&msg.from, true).await;
                                                // Send accept back
                                                let accept_payload = FriendAcceptPayload {
                                                    did: state.did().to_string(),
                                                    trust_level: outbound.offered_trust,
                                                    message: Some("Auto-accepted (crossed request)".to_string()),
                                                };
                                                let mut accept_msg = Message::friend_accept(&node_name, &accept_payload);
                                                let _ = sign_and_send(&mut writer, &mut accept_msg, state.identity()).await;
                                                // Notify inbox
                                                let mut notify_msg = msg.clone();
                                                notify_msg.body = format!(
                                                    "Friend request from {} auto-accepted (crossed request). Trust: {} ↔ {}",
                                                    msg.from, outbound.offered_trust, payload.trust_level
                                                );
                                                state.push_inbox(notify_msg).await;
                                            } else if state.get_trust_level(&msg.from).await.0 > 0 {
                                                // Already a friend — auto-accept (upgrading to bilateral)
                                                info!(
                                                    "Friend request from existing friend {} — auto-accepting",
                                                    msg.from
                                                );
                                                let our_trust = state.get_trust_level(&msg.from).await;
                                                // Update their_trust on existing friend
                                                state.update_their_trust(&msg.from, payload.trust_level).await;
                                                // Send accept
                                                let accept_payload = FriendAcceptPayload {
                                                    did: state.did().to_string(),
                                                    trust_level: our_trust.0,
                                                    message: Some("Auto-accepted (already friends)".to_string()),
                                                };
                                                let mut accept_msg = Message::friend_accept(&node_name, &accept_payload);
                                                let _ = sign_and_send(&mut writer, &mut accept_msg, state.identity()).await;
                                                // Store as accepted request
                                                let req = FReq {
                                                    id: Uuid::new_v4(),
                                                    peer_name: msg.from.clone(),
                                                    peer_did: peer_did.clone(),
                                                    offered_trust: payload.trust_level,
                                                    direction: FriendRequestDirection::Inbound,
                                                    status: FriendRequestStatus::Accepted,
                                                    created_at: chrono::Utc::now(),
                                                    resolved_at: Some(chrono::Utc::now()),
                                                    message: payload.message,
                                                    owner_did: payload.owner_did,
                                                };
                                                let _ = state.add_friend_request(req).await;
                                                // Notify inbox
                                                let mut notify_msg = msg.clone();
                                                notify_msg.body = format!(
                                                    "Friend request from {} auto-accepted (already friends). They trust you: {}",
                                                    msg.from, payload.trust_level
                                                );
                                                state.push_inbox(notify_msg).await;
                                            } else {
                                                // Unknown peer — queue as pending inbound
                                                // Check for duplicate pending request
                                                if state.get_pending_inbound_from(&msg.from).await.is_some() {
                                                    info!("Duplicate friend request from {} — ignoring", msg.from);
                                                    continue;
                                                }
                                                let req = FReq {
                                                    id: Uuid::new_v4(),
                                                    peer_name: msg.from.clone(),
                                                    peer_did: peer_did.clone(),
                                                    offered_trust: payload.trust_level,
                                                    direction: FriendRequestDirection::Inbound,
                                                    status: FriendRequestStatus::Pending,
                                                    created_at: chrono::Utc::now(),
                                                    resolved_at: None,
                                                    message: payload.message.clone(),
                                                    owner_did: payload.owner_did,
                                                };
                                                let _ = state.add_friend_request(req).await;
                                                // Notify inbox
                                                let mut notify_msg = msg.clone();
                                                notify_msg.body = format!(
                                                    "New friend request from {}! They offer trust level {}. Message: {}. Use agora friends accept {} to accept.",
                                                    msg.from, payload.trust_level,
                                                    payload.message.as_deref().unwrap_or("(none)"),
                                                    msg.from
                                                );
                                                state.push_inbox(notify_msg).await;
                                            }
                                        }
                                    }
                                    MessageType::FriendAccept => {
                                        if let Some(payload) = msg.parse_payload::<FriendAcceptPayload>() {
                                            info!(
                                                "[{}] friend.accept: trust={}",
                                                msg.from, payload.trust_level
                                            );
                                            // Look up our outbound request to get the trust level we offered
                                            let our_trust = if let Some(outbound) = state.get_pending_outbound_to(&msg.from).await {
                                                outbound.offered_trust
                                            } else {
                                                // No outbound request found — use a default
                                                2
                                            };
                                            // Add friend if not already in list
                                            if state.get_trust_level(&msg.from).await.0 == 0 {
                                                let friend = crate::state::Friend {
                                                    name: msg.from.clone(),
                                                    alias: None,
                                                    trust_level: crate::state::TrustLevel(our_trust.min(4)),
                                                    added_at: chrono::Utc::now(),
                                                    notes: None,
                                                    muted: false,
                                                    last_address: Some(peer_addr.to_string()),
                                                    did: peer_did.clone(),
                                                    owner_did: peer_owner_did.clone(),
                                                    their_trust: Some(payload.trust_level),
                                                };
                                                let _ = state.add_friend(friend).await;
                                                info!(
                                                    "Added {} as friend (trust {}) — they trust us at {}",
                                                    msg.from, our_trust, payload.trust_level
                                                );
                                            } else {
                                                // Already a friend — just update their_trust
                                                state.update_their_trust(&msg.from, payload.trust_level).await;
                                            }
                                            // Resolve our outbound request
                                            state.resolve_outbound_request(&msg.from, true).await;
                                            // Notify inbox
                                            let mut notify_msg = msg.clone();
                                            notify_msg.body = format!(
                                                "{} accepted your friend request! They assigned you trust level {}. Message: {}",
                                                msg.from, payload.trust_level,
                                                payload.message.as_deref().unwrap_or("(none)")
                                            );
                                            state.push_inbox(notify_msg).await;
                                        }
                                    }
                                    MessageType::FriendReject => {
                                        if let Some(payload) = msg.parse_payload::<FriendRejectPayload>() {
                                            info!("[{}] friend.reject: reason={:?}", msg.from, payload.reason);
                                            // Resolve our outbound request as rejected
                                            state.resolve_outbound_request(&msg.from, false).await;
                                            // Notify inbox
                                            let mut notify_msg = msg.clone();
                                            notify_msg.body = format!(
                                                "{} rejected your friend request. Reason: {}",
                                                msg.from,
                                                payload.reason.as_deref().unwrap_or("(none)")
                                            );
                                            state.push_inbox(notify_msg).await;
                                        }
                                    }
                                    MessageType::FriendRevoke => {
                                        if let Some(payload) = msg.parse_payload::<FriendRevokePayload>() {
                                            info!("[{}] friend.revoke: reason={:?}", msg.from, payload.reason);
                                            // Only remove if they are actually a friend
                                            let trust = state.get_trust_level(&msg.from).await;
                                            if trust.0 > 0 {
                                                let _ = state.remove_friend(&msg.from).await;
                                                info!("Removed friend {} (revoked by them)", msg.from);
                                            }
                                            // Notify inbox
                                            let mut notify_msg = msg.clone();
                                            notify_msg.body = format!(
                                                "{} revoked their friendship. Reason: {}",
                                                msg.from,
                                                payload.reason.as_deref().unwrap_or("(none)")
                                            );
                                            state.push_inbox(notify_msg).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            _ if msg.msg_type.is_project() => {
                                use crate::protocol::message::{
                                    ProjectInvitePayload, ProjectAcceptPayload,
                                    ProjectDeclinePayload, ProjectLeavePayload,
                                    ProjectClockInPayload, ProjectClockOutPayload,
                                };
                                use crate::project::{
                                    ProjectInvitation, InvitationDirection, InvitationStatus,
                                };
                                match msg.msg_type {
                                    MessageType::ProjectInvite => {
                                        if let Some(payload) = msg.parse_payload::<ProjectInvitePayload>() {
                                            info!(
                                                "[{}] project.invite: {} as {} (project: {})",
                                                msg.from, peer_name, payload.role, payload.project_name
                                            );
                                            // Only accept invitations from friends
                                            let trust = state.get_trust_level(&msg.from).await;
                                            if trust.0 == 0 {
                                                warn!("Project invite from non-friend {} — ignoring", msg.from);
                                                continue;
                                            }
                                            let role: crate::project::ProjectRole = payload.role.parse().unwrap_or(crate::project::ProjectRole::Developer);
                                            // Auto-accept based on policy
                                            if state.should_auto_accept(&msg.from).await {
                                                info!("Auto-accepting project invite from trusted friend {}", msg.from);
                                                // Create local project from context
                                                let project_exists = state.get_project(&payload.project_id).await.is_some();
                                                if !project_exists {
                                                    let ctx = payload.context.as_ref();
                                                    state.create_project_from_invitation(
                                                        payload.project_id,
                                                        ctx.map(|c| c.project_name.as_str()).unwrap_or(&payload.project_name),
                                                        ctx.and_then(|c| c.description.clone()),
                                                        ctx.and_then(|c| c.repo.clone()),
                                                        &msg.from,
                                                        role,
                                                    ).await;
                                                } else {
                                                    state.add_project_agent(
                                                        &payload.project_id, &node_name, Some(state.did().to_string()), role
                                                    ).await;
                                                }
                                                // Store as accepted invitation
                                                let inv = ProjectInvitation {
                                                    id: Uuid::new_v4(),
                                                    project_id: payload.project_id,
                                                    project_name: payload.project_name.clone(),
                                                    peer_name: msg.from.clone(),
                                                    peer_did: peer_did.clone(),
                                                    role,
                                                    direction: InvitationDirection::Inbound,
                                                    status: InvitationStatus::Accepted,
                                                    created_at: chrono::Utc::now(),
                                                    resolved_at: Some(chrono::Utc::now()),
                                                    message: payload.message.clone(),
                                                    context: payload.context.clone(),
                                                };
                                                let _ = state.add_project_invitation(inv).await;
                                                // Send accept back
                                                let accept_payload = ProjectAcceptPayload {
                                                    project_id: payload.project_id,
                                                    message: Some("Auto-accepted (trusted friend)".to_string()),
                                                };
                                                let mut accept_msg = Message::project_accept(&node_name, &accept_payload);
                                                let _ = sign_and_send(&mut writer, &mut accept_msg, state.identity()).await;
                                                // Notify inbox
                                                let mut notify_msg = msg.clone();
                                                notify_msg.body = format!(
                                                    "Auto-accepted project invitation from {} for '{}' (role: {})",
                                                    msg.from, payload.project_name, payload.role
                                                );
                                                state.push_inbox(notify_msg).await;
                                            } else {
                                                // Queue as pending
                                                let inv = ProjectInvitation {
                                                    id: Uuid::new_v4(),
                                                    project_id: payload.project_id,
                                                    project_name: payload.project_name.clone(),
                                                    peer_name: msg.from.clone(),
                                                    peer_did: peer_did.clone(),
                                                    role,
                                                    direction: InvitationDirection::Inbound,
                                                    status: InvitationStatus::Pending,
                                                    created_at: chrono::Utc::now(),
                                                    resolved_at: None,
                                                    message: payload.message.clone(),
                                                    context: payload.context,
                                                };
                                                let _ = state.add_project_invitation(inv).await;
                                                let mut notify_msg = msg.clone();
                                                notify_msg.body = format!(
                                                    "Project invitation from {} for '{}' (role: {}). Message: {}",
                                                    msg.from, payload.project_name, payload.role,
                                                    payload.message.as_deref().unwrap_or("(none)")
                                                );
                                                state.push_inbox(notify_msg).await;
                                            }
                                        }
                                    }
                                    MessageType::ProjectAccept => {
                                        if let Some(payload) = msg.parse_payload::<ProjectAcceptPayload>() {
                                            info!("[{}] project.accept: {}", msg.from, payload.project_id);
                                            // Add peer to our project
                                            // Look up the role from the outbound invitation
                                            let invitations = state.get_project_invitations().await;
                                            let role = invitations.iter()
                                                .find(|i| i.peer_name == msg.from && i.project_id == payload.project_id
                                                    && i.direction == InvitationDirection::Outbound)
                                                .map(|i| i.role)
                                                .unwrap_or(crate::project::ProjectRole::Developer);
                                            state.add_project_agent(
                                                &payload.project_id, &msg.from, peer_did.clone(), role
                                            ).await;
                                            state.resolve_outbound_project_invitation(&msg.from, &payload.project_id, true).await;
                                            let mut notify_msg = msg.clone();
                                            notify_msg.body = format!(
                                                "{} accepted project invitation (project: {})",
                                                msg.from, payload.project_id
                                            );
                                            state.push_inbox(notify_msg).await;
                                        }
                                    }
                                    MessageType::ProjectDecline => {
                                        if let Some(payload) = msg.parse_payload::<ProjectDeclinePayload>() {
                                            info!("[{}] project.decline: {}", msg.from, payload.project_id);
                                            state.resolve_outbound_project_invitation(&msg.from, &payload.project_id, false).await;
                                            let mut notify_msg = msg.clone();
                                            notify_msg.body = format!(
                                                "{} declined project invitation (project: {}). Reason: {}",
                                                msg.from, payload.project_id,
                                                payload.reason.as_deref().unwrap_or("(none)")
                                            );
                                            state.push_inbox(notify_msg).await;
                                        }
                                    }
                                    MessageType::ProjectLeave => {
                                        if let Some(payload) = msg.parse_payload::<ProjectLeavePayload>() {
                                            info!("[{}] project.leave: {}", msg.from, payload.project_id);
                                            state.remove_project_agent(&payload.project_id, &msg.from).await;
                                            let mut notify_msg = msg.clone();
                                            notify_msg.body = format!(
                                                "{} left the project ({})",
                                                msg.from, payload.project_id
                                            );
                                            state.push_inbox(notify_msg).await;
                                        }
                                    }
                                    MessageType::ProjectClockIn => {
                                        if let Some(payload) = msg.parse_payload::<ProjectClockInPayload>() {
                                            info!("[{}] project.clock_in: {}", msg.from, payload.project_id);
                                            state.project_clock_in(&payload.project_id, &msg.from, payload.focus).await;
                                        }
                                    }
                                    MessageType::ProjectClockOut => {
                                        if let Some(payload) = msg.parse_payload::<ProjectClockOutPayload>() {
                                            info!("[{}] project.clock_out: {}", msg.from, payload.project_id);
                                            state.project_clock_out(&payload.project_id, &msg.from).await;
                                        }
                                    }
                                    MessageType::ProjectStage => {
                                        use crate::protocol::message::ProjectStagePayload;
                                        if let Some(payload) = msg.parse_payload::<ProjectStagePayload>() {
                                            // Permission check: require "coordinate" for stage changes
                                            if let Err(e) = state.check_permission(
                                                &payload.project_id, &msg.from, peer_did.as_deref(), "coordinate"
                                            ).await {
                                                warn!("P2P permission denied for project.stage from {}: {}", msg.from, e);
                                                state.append_audit(&payload.project_id, "access.denied", &e).await;
                                            } else {
                                                info!("[{}] project.stage: {} → {}", msg.from, payload.project_id, payload.stage);
                                                if let Ok(stage) = payload.stage.parse::<crate::project::ProjectStage>() {
                                                    let _ = state.set_project_stage(&payload.project_id, stage).await;
                                                }
                                            }
                                        }
                                        state.push_inbox(msg).await;
                                    }
                                    MessageType::AuditEntry => {
                                        use crate::protocol::message::AuditEntryPayload;
                                        if let Some(payload) = msg.parse_payload::<AuditEntryPayload>() {
                                            let merged = state.merge_audit_entry(
                                                &payload.project_id, payload.entry,
                                            ).await;
                                            if merged {
                                                info!("[{}] project.audit: merged entry for {}", msg.from, payload.project_id);
                                            }
                                        }
                                    }
                                    MessageType::ProjectSuspend => {
                                        use crate::protocol::message::ProjectSuspendPayload;
                                        if let Some(payload) = msg.parse_payload::<ProjectSuspendPayload>() {
                                            if let Err(e) = state.check_permission(
                                                &payload.project_id, &msg.from, peer_did.as_deref(), "coordinate"
                                            ).await {
                                                warn!("P2P permission denied for project.suspend from {}: {}", msg.from, e);
                                                state.append_audit(&payload.project_id, "access.denied", &e).await;
                                            } else {
                                                info!("[{}] project.suspend: {} in {}", msg.from, payload.target, payload.project_id);
                                                state.apply_remote_suspend(
                                                    &payload.project_id, &payload.target, payload.reason.clone(),
                                                ).await;
                                                state.append_audit(
                                                    &payload.project_id,
                                                    "agent.suspended",
                                                    &format!("{} suspended {} (reason: {})", msg.from, payload.target, payload.reason.as_deref().unwrap_or("none")),
                                                ).await;
                                            }
                                        }
                                        state.push_inbox(msg).await;
                                    }
                                    MessageType::ProjectUnsuspend => {
                                        use crate::protocol::message::ProjectUnsuspendPayload;
                                        if let Some(payload) = msg.parse_payload::<ProjectUnsuspendPayload>() {
                                            if let Err(e) = state.check_permission(
                                                &payload.project_id, &msg.from, peer_did.as_deref(), "coordinate"
                                            ).await {
                                                warn!("P2P permission denied for project.unsuspend from {}: {}", msg.from, e);
                                                state.append_audit(&payload.project_id, "access.denied", &e).await;
                                            } else {
                                                info!("[{}] project.unsuspend: {} in {}", msg.from, payload.target, payload.project_id);
                                                state.apply_remote_unsuspend(
                                                    &payload.project_id, &payload.target,
                                                ).await;
                                                state.append_audit(
                                                    &payload.project_id,
                                                    "agent.unsuspended",
                                                    &format!("{} unsuspended {}", msg.from, payload.target),
                                                ).await;
                                            }
                                        }
                                        state.push_inbox(msg).await;
                                    }
                                    _ => {}
                                }
                            }
                            _ if msg.msg_type.is_task() => {
                                use crate::protocol::message::{
                                    TaskAssignPayload, TaskUpdatePayload as TaskUpdatePl,
                                    TaskCompletePayload,
                                };
                                match msg.msg_type {
                                    MessageType::TaskAssign => {
                                        if let Some(payload) = msg.parse_payload::<TaskAssignPayload>() {
                                            // Permission check: require "write" for task creation
                                            if let Err(e) = state.check_permission(
                                                &payload.project_id, &msg.from, peer_did.as_deref(), "write"
                                            ).await {
                                                warn!("P2P permission denied for task.assign from {}: {}", msg.from, e);
                                                state.append_audit(&payload.project_id, "access.denied", &e).await;
                                            } else {
                                                info!("[{}] task.assign: {} in project {}", msg.from, payload.title, payload.project_id);
                                                let deps: Vec<uuid::Uuid> = payload.depends_on;
                                                let priority = payload.priority.and_then(|p| match p.to_lowercase().as_str() {
                                                    "low" => Some(crate::project::TaskPriority::Low),
                                                    "medium" => Some(crate::project::TaskPriority::Medium),
                                                    "high" => Some(crate::project::TaskPriority::High),
                                                    "critical" => Some(crate::project::TaskPriority::Critical),
                                                    _ => None,
                                                });
                                                state.create_task_with_id(
                                                    &payload.project_id,
                                                    Some(payload.task_id),
                                                    &payload.title,
                                                    payload.description,
                                                    payload.assignee,
                                                    priority,
                                                    deps,
                                                    Some(msg.from.clone()),
                                                ).await;
                                            }
                                        }
                                        state.push_inbox(msg).await;
                                    }
                                    MessageType::TaskUpdate => {
                                        if let Some(payload) = msg.parse_payload::<TaskUpdatePl>() {
                                            // Permission check: require "write" for task updates
                                            if let Err(e) = state.check_permission(
                                                &payload.project_id, &msg.from, peer_did.as_deref(), "write"
                                            ).await {
                                                warn!("P2P permission denied for task.update from {}: {}", msg.from, e);
                                                state.append_audit(&payload.project_id, "access.denied", &e).await;
                                            } else {
                                                info!("[{}] task.update: {} in project {}", msg.from, payload.task_id, payload.project_id);
                                                let status = payload.status.and_then(|s| s.parse::<crate::project::TaskStatus>().ok());
                                                let _ = state.update_task(
                                                    &payload.project_id,
                                                    &payload.task_id,
                                                    status,
                                                    payload.title,
                                                    payload.description,
                                                    payload.assignee,
                                                ).await;
                                            }
                                        }
                                        state.push_inbox(msg).await;
                                    }
                                    MessageType::TaskComplete => {
                                        if let Some(payload) = msg.parse_payload::<TaskCompletePayload>() {
                                            // Permission check: require "write" for task completion
                                            if let Err(e) = state.check_permission(
                                                &payload.project_id, &msg.from, peer_did.as_deref(), "write"
                                            ).await {
                                                warn!("P2P permission denied for task.complete from {}: {}", msg.from, e);
                                                state.append_audit(&payload.project_id, "access.denied", &e).await;
                                            } else {
                                                info!("[{}] task.complete: {} in project {}", msg.from, payload.task_id, payload.project_id);
                                                let _ = state.update_task(
                                                    &payload.project_id,
                                                    &payload.task_id,
                                                    Some(crate::project::TaskStatus::Done),
                                                    None,
                                                    None,
                                                    None,
                                                ).await;
                                            }
                                        }
                                        state.push_inbox(msg).await;
                                    }
                                    _ => {}
                                }
                            }
                            MessageType::Unknown => {
                                info!("Ignoring unknown message type from {} (forward compat)", msg.from);
                            }
                            _ => {
                                info!("Received {:?} from {}", msg.msg_type, msg.from);
                            }
                        }
                    }
                    None => {
                        info!("Connection closed by {}", peer_addr);
                        break;
                    }
                }
            }

            // Outbound message from local agent (via HTTP API) — broadcast to all peers
            result = outbox_rx.recv() => {
                match result {
                    Ok(outbound) => {
                        if should_deliver(&outbound, &peer_name, peer_addr) {
                            let sender = outbound.from_override.as_deref().unwrap_or(&node_name);
                            let mut msg = Message::text(sender, &outbound.body);
                            msg.id = outbound.id;
                            msg.reply_to = outbound.reply_to;
                            msg.conversation_id = outbound.conversation_id;
                            if let Some(ref mt) = outbound.msg_type {
                                msg.msg_type = mt.clone();
                            }
                            sign_and_send(&mut writer, &mut msg, state.identity()).await?;
                            info!("Sent {:?} to {} ({})", msg.msg_type, peer_name, peer_addr);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Peer {} lagged, missed {} messages", peer_name, n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Outbox channel closed");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Check if an outbound message should be delivered to a specific peer.
fn should_deliver(msg: &OutboundMessage, peer_name: &str, peer_addr: &str) -> bool {
    match &msg.to {
        None => true, // Broadcast
        Some(target) => target == peer_name || target == peer_addr,
    }
}

/// Connect to an Agora relay server over WebSocket for NAT traversal.
/// The relay forwards messages between agents who can't connect directly.
/// Uses auto-reconnect with exponential backoff (same as `connect_to_peer`).
pub async fn connect_to_relay(state: DaemonState, relay_url: &str) -> Result<()> {
    let mut backoff = std::time::Duration::from_secs(1);
    let max_backoff = std::time::Duration::from_secs(60);
    let mut was_connected = false;

    loop {
        info!("Connecting to relay {}...", relay_url);

        match try_connect_relay_once(&state, relay_url).await {
            Ok(()) => {
                info!("Relay connection to {} closed, reconnecting...", relay_url);
                was_connected = true;
                backoff = std::time::Duration::from_secs(1);
            }
            Err(e) => {
                if was_connected {
                    warn!(
                        "Relay connection lost to {}: {}. Reconnecting...",
                        relay_url, e
                    );
                    backoff = std::time::Duration::from_secs(1);
                    was_connected = false;
                } else {
                    warn!(
                        "Failed to connect to relay {}: {}. Retrying in {:?}...",
                        relay_url, e, backoff
                    );
                }
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

/// Single attempt to connect to the relay.
async fn try_connect_relay_once(state: &DaemonState, relay_url: &str) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};

    let (ws_stream, _) = tokio_tungstenite::connect_async(relay_url)
        .await
        .context(format!("Failed to connect to relay {}", relay_url))?;
    info!("WebSocket connection established to relay {}", relay_url);

    let (mut ws_sink, ws_stream) = ws_stream.split();

    // Send RelayHello to authenticate with the relay
    let identity = state.identity();
    let did = state.did().to_string();
    let sig = identity.sign(did.as_bytes());

    let hello = serde_json::json!({
        "name": state.node_name(),
        "did": did,
        "public_key": identity.public_key_base58(),
        "signature": bs58::encode(&sig).into_string(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    ws_sink
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&hello)?.into(),
        ))
        .await
        .context("Failed to send RelayHello")?;

    // Wrap WS streams as AsyncRead/AsyncWrite for handle_connection
    let reader = ws::WsReader::new(ws_stream);
    let writer = ws::WsWriter::new(ws_sink);

    let result = handle_connection(reader, writer, state, &format!("relay:{}", relay_url)).await;
    state.remove_peer(&format!("relay:{}", relay_url)).await;
    result
}
