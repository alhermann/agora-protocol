use std::sync::Arc;

use axum::{
    extract::{ws, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use clap::Parser;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use agora_relay::{RelayControl, RelayEnvelope, RelayHello};

/// Agora Relay Server — WebSocket relay for NAT traversal
#[derive(Parser)]
#[command(name = "agora-relay", version, about)]
struct Cli {
    /// Address to listen on
    #[arg(short, long, default_value = "0.0.0.0")]
    address: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8443)]
    port: u16,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// A connected agent's sender channel.
struct RelayAgent {
    name: String,
    did: String,
    tx: mpsc::UnboundedSender<ws::Message>,
}

/// Shared relay state.
struct RelayState {
    /// DID -> agent connection
    agents: DashMap<String, RelayAgent>,
}

impl RelayState {
    fn new() -> Self {
        Self {
            agents: DashMap::new(),
        }
    }

    fn online_count(&self) -> usize {
        self.agents.len()
    }

    /// Register an agent. Returns false if DID already connected.
    fn register(&self, name: String, did: String, tx: mpsc::UnboundedSender<ws::Message>) -> bool {
        if self.agents.contains_key(&did) {
            // Evict old connection (reconnect)
            self.agents.remove(&did);
        }
        self.agents
            .insert(did.clone(), RelayAgent { name, did, tx });
        true
    }

    fn unregister(&self, did: &str) -> Option<(String, String)> {
        self.agents.remove(did).map(|(_, a)| (a.name, a.did))
    }

    /// Forward a message to a target agent. Returns true if delivered.
    fn forward(&self, to: &str, message: &serde_json::Value) -> bool {
        // Try by DID first, then by name
        if let Some(agent) = self.agents.get(to) {
            let envelope = serde_json::json!({
                "type": "message",
                "message": message,
            });
            let text = serde_json::to_string(&envelope).unwrap_or_default();
            agent.tx.send(ws::Message::Text(text.into())).is_ok()
        } else {
            // Search by name
            let mut found = false;
            for entry in self.agents.iter() {
                if entry.value().name == to {
                    let envelope = serde_json::json!({
                        "type": "message",
                        "message": message,
                    });
                    let text = serde_json::to_string(&envelope).unwrap_or_default();
                    let _ = entry.value().tx.send(ws::Message::Text(text.into()));
                    found = true;
                    break;
                }
            }
            found
        }
    }

    /// Broadcast presence update to all agents except the source.
    fn broadcast_presence(&self, exclude_did: &str, control: &RelayControl) {
        let text = serde_json::to_string(control).unwrap_or_default();
        for entry in self.agents.iter() {
            if entry.key() != exclude_did {
                let _ = entry
                    .value()
                    .tx
                    .send(ws::Message::Text(text.clone().into()));
            }
        }
    }
}

/// Verify an Ed25519 signature (base58-encoded key + signature over message).
fn verify_signature(public_key_b58: &str, message: &[u8], signature_b58: &str) -> bool {
    let Ok(pk_bytes) = bs58::decode(public_key_b58).into_vec() else {
        return false;
    };
    let Ok(sig_bytes) = bs58::decode(signature_b58).into_vec() else {
        return false;
    };
    let Ok(pk) = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &pk_bytes)
        .verify(message, &sig_bytes)
    else {
        return false;
    };
    let _ = pk;
    true
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<RelayState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: ws::WebSocket, state: Arc<RelayState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ws::Message>();

    // Wait for RelayHello as first message
    let hello: RelayHello = loop {
        match ws_rx.next().await {
            Some(Ok(ws::Message::Text(text))) => match serde_json::from_str::<RelayHello>(&text) {
                Ok(hello) => break hello,
                Err(e) => {
                    warn!("Invalid RelayHello: {}", e);
                    let err = RelayControl::Error {
                        message: format!("Expected RelayHello, got invalid JSON: {}", e),
                    };
                    let text = serde_json::to_string(&err).unwrap_or_default();
                    let _ = ws_tx.send(ws::Message::Text(text.into())).await;
                    return;
                }
            },
            Some(Ok(ws::Message::Close(_))) | None => return,
            _ => continue,
        }
    };

    // Verify signature: agent signs their DID with their Ed25519 key
    if !verify_signature(&hello.public_key, hello.did.as_bytes(), &hello.signature) {
        warn!("Invalid signature from {} ({})", hello.name, hello.did);
        let err = RelayControl::Error {
            message: "Invalid Ed25519 signature".to_string(),
        };
        let text = serde_json::to_string(&err).unwrap_or_default();
        let _ = ws_tx.send(ws::Message::Text(text.into())).await;
        return;
    }

    // Verify DID matches public key
    let expected_did = format!("did:agora:{}", hello.public_key);
    if hello.did != expected_did {
        warn!(
            "DID mismatch from {}: expected {}, got {}",
            hello.name, expected_did, hello.did
        );
        let err = RelayControl::Error {
            message: "DID does not match public key".to_string(),
        };
        let text = serde_json::to_string(&err).unwrap_or_default();
        let _ = ws_tx.send(ws::Message::Text(text.into())).await;
        return;
    }

    let agent_did = hello.did.clone();
    let agent_name = hello.name.clone();

    info!("Agent authenticated: {} ({})", agent_name, agent_did);

    // Register agent
    state.register(agent_name.clone(), agent_did.clone(), tx);

    // Send welcome
    let welcome = RelayControl::Welcome {
        agents_online: state.online_count(),
    };
    let text = serde_json::to_string(&welcome).unwrap_or_default();
    let _ = ws_tx.send(ws::Message::Text(text.into())).await;

    // Broadcast presence
    state.broadcast_presence(
        &agent_did,
        &RelayControl::AgentOnline {
            name: agent_name.clone(),
            did: agent_did.clone(),
        },
    );

    // Spawn writer task: forward messages from channel to WS
    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Read loop: receive messages from agent, forward to targets
    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(ws::Message::Text(text)) => match serde_json::from_str::<RelayEnvelope>(&text) {
                Ok(envelope) => {
                    if !state.forward(&envelope.to, &envelope.message) {
                        info!(
                            "Message from {} to {} — target not connected",
                            agent_name, envelope.to
                        );
                    }
                }
                Err(e) => {
                    warn!("Invalid RelayEnvelope from {}: {}", agent_name, e);
                }
            },
            Ok(ws::Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    // Cleanup
    info!("Agent disconnected: {} ({})", agent_name, agent_did);
    state.unregister(&agent_did);
    let did_clone = agent_did.clone();
    state.broadcast_presence(
        &agent_did,
        &RelayControl::AgentOffline {
            name: agent_name,
            did: did_clone,
        },
    );
    write_task.abort();
}

async fn health() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("agora_relay=debug,info")
    } else {
        EnvFilter::new("agora_relay=info,warn")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let state = Arc::new(RelayState::new());

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .with_state(state);

    let bind_addr = format!("{}:{}", cli.address, cli.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Agora relay listening on {}", bind_addr);
    println!("Agora relay server listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_state_register_unregister() {
        let state = RelayState::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        state.register("alice".into(), "did:agora:abc".into(), tx);
        assert_eq!(state.online_count(), 1);
        state.unregister("did:agora:abc");
        assert_eq!(state.online_count(), 0);
    }

    #[test]
    fn test_relay_state_forward_not_connected() {
        let state = RelayState::new();
        assert!(!state.forward("did:agora:nonexistent", &serde_json::json!({})));
    }

    #[test]
    fn test_relay_state_forward_by_name() {
        let state = RelayState::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.register("bob".into(), "did:agora:bob123".into(), tx);
        let msg = serde_json::json!({"body": "hello"});
        assert!(state.forward("bob", &msg));
        let received = rx.try_recv().unwrap();
        match received {
            ws::Message::Text(text) => {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(v["type"], "message");
            }
            _ => panic!("Expected text message"),
        }
    }

    #[test]
    fn test_relay_state_reconnect_evicts_old() {
        let state = RelayState::new();
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (tx2, _rx2) = mpsc::unbounded_channel();
        state.register("alice".into(), "did:agora:abc".into(), tx1);
        state.register("alice".into(), "did:agora:abc".into(), tx2);
        assert_eq!(state.online_count(), 1);
    }
}
