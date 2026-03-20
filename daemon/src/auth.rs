//! API token authentication — generates, persists, and validates bearer tokens.
//!
//! Tokens are stored in `~/.agora/api_token.json`. On first startup, a random
//! 32-byte token is generated. The dashboard LoginPage validates tokens via
//! `POST /auth/verify`.

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Persisted API token.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiTokenFile {
    token: String,
    created_at: String,
}

/// Default path: `~/.agora/api_token.json`
pub fn default_token_path() -> PathBuf {
    crate::config::agora_home().join("api_token.json")
}

/// Generate a random 32-byte token as hex.
fn generate_token() -> String {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .expect("Failed to generate random bytes");
    hex::encode(bytes)
}

/// Load token from disk, or generate and save a new one.
pub fn load_or_create_token(path: &Path) -> String {
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Ok(file) = serde_json::from_str::<ApiTokenFile>(&contents) {
            info!("Loaded API token from {}", path.display());
            return file.token;
        }
    }

    let token = generate_token();
    let file = ApiTokenFile {
        token: token.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            // Write with restrictive permissions (0600)
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(path)
                {
                    use std::io::Write;
                    let _ = f.write_all(json.as_bytes());
                    info!("Generated new API token at {}", path.display());
                }
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::write(path, &json);
                info!("Generated new API token at {}", path.display());
            }
        }
        Err(e) => warn!("Failed to serialize API token: {}", e),
    }

    token
}

/// Regenerate the API token and save it.
pub fn regenerate_token(path: &Path) -> String {
    let token = generate_token();
    let file = ApiTokenFile {
        token: token.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(path)
                {
                    use std::io::Write;
                    let _ = f.write_all(json.as_bytes());
                }
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::write(path, &json);
            }
        }
        Err(e) => warn!("Failed to serialize API token: {}", e),
    }

    token
}

/// Extract bearer token from request headers.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// Auth middleware — checks `Authorization: Bearer <token>` on all requests
/// except exempted paths.
pub async fn auth_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();

    // Exempt paths that don't require auth
    if is_exempt_path(&path) {
        return next.run(req).await;
    }

    // Check if auth is enabled (token exists in state)
    // We store the expected token in a request extension set by the router layer.
    let expected_token = req.extensions().get::<ApiToken>().map(|t| t.0.clone());

    let expected_token = match expected_token {
        Some(t) => t,
        None => {
            // No token configured — auth disabled, allow all
            return next.run(req).await;
        }
    };

    // Check Authorization header
    match extract_bearer_token(req.headers()) {
        Some(token) if token == expected_token => next.run(req).await,
        Some(_) => {
            warn!("Rejected request to {} — invalid API token", path);
            (StatusCode::UNAUTHORIZED, "Invalid API token").into_response()
        }
        None => {
            // Allow unauthenticated requests ONLY from localhost (MCP, CLI).
            // Remote clients MUST provide an Authorization header.
            //
            // NOTE: MCP bridge (mcp.rs) currently connects without a token.
            // TODO: Have MCP read token from ~/.agora/api_token.json, then
            // require auth for all mutation endpoints even from localhost.
            if is_localhost_request(&req) {
                next.run(req).await
            } else {
                warn!(
                    "Rejected unauthenticated request to {} from non-localhost source",
                    path
                );
                (StatusCode::UNAUTHORIZED, "Authentication required").into_response()
            }
        }
    }
}

/// Check if a request originates from localhost.
///
/// SECURITY: We intentionally do NOT trust X-Forwarded-For headers here.
/// An attacker can trivially spoof `X-Forwarded-For: 127.0.0.1` to bypass
/// auth. Instead, we rely on ConnectInfo (the actual TCP peer address) or
/// fall back to denying access if peer info is unavailable.
fn is_localhost_request(req: &Request) -> bool {
    // Check ConnectInfo if available (set by axum's into_make_service_with_connect_info)
    if let Some(connect_info) = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    {
        return connect_info.0.ip().is_loopback();
    }

    // If ConnectInfo is not available, check if the daemon is bound to
    // loopback only. We store this as a request extension (set by the router).
    if let Some(loopback_only) = req.extensions().get::<LoopbackOnly>() {
        return loopback_only.0;
    }

    // Safe default: deny if we can't determine the source
    false
}

/// Extension type indicating the daemon is bound to loopback only (127.0.0.1).
/// When true, all connections are necessarily local, so auth can be relaxed.
#[derive(Clone)]
pub struct LoopbackOnly(pub bool);

/// Paths that never require authentication.
fn is_exempt_path(path: &str) -> bool {
    // Auth endpoints must be accessible to log in
    if path == "/auth/verify" || path.starts_with("/api/auth/") {
        return true;
    }
    // Health check
    if path == "/health" || path == "/api/health" {
        return true;
    }
    // Dashboard static files (HTML, JS, CSS, images)
    if path == "/"
        || path.starts_with("/assets/")
        || path.ends_with(".html")
        || path.ends_with(".js")
        || path.ends_with(".css")
        || path.ends_with(".ico")
        || path.ends_with(".svg")
        || path.ends_with(".png")
        || path.ends_with(".woff2")
    {
        return true;
    }
    false
}

/// Wrapper type for the API token, stored as a request extension.
#[derive(Clone)]
pub struct ApiToken(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token_length() {
        let token = generate_token();
        assert_eq!(token.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_load_or_create_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api_token.json");

        // First call creates
        let token1 = load_or_create_token(&path);
        assert_eq!(token1.len(), 64);

        // Second call loads same token
        let token2 = load_or_create_token(&path);
        assert_eq!(token1, token2);
    }

    #[test]
    fn test_regenerate_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api_token.json");

        let token1 = load_or_create_token(&path);
        let token2 = regenerate_token(&path);
        assert_ne!(token1, token2);
        assert_eq!(token2.len(), 64);

        // Verify new token persisted
        let token3 = load_or_create_token(&path);
        assert_eq!(token2, token3);
    }

    #[test]
    fn test_exempt_paths() {
        assert!(is_exempt_path("/auth/verify"));
        assert!(is_exempt_path("/api/auth/verify"));
        assert!(is_exempt_path("/health"));
        assert!(is_exempt_path("/"));
        assert!(is_exempt_path("/assets/index-abc123.js"));
        assert!(!is_exempt_path("/status"));
        assert!(!is_exempt_path("/friends"));
        assert!(!is_exempt_path("/api/status"));
    }
}
