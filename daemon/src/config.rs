use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Returns the Agora data directory.
/// Checks AGORA_HOME env var first, falls back to ~/.agora.
/// All modules should use this instead of hardcoding ~/.agora.
pub fn agora_home() -> PathBuf {
    if let Ok(dir) = std::env::var("AGORA_HOME") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".agora")
}

/// Agora daemon configuration loaded from `~/.agora/config.toml`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgoraConfig {
    /// Node name (identifies this agent on the network).
    pub name: Option<String>,
    /// Address to listen on for P2P connections.
    pub address: Option<String>,
    /// Port for P2P connections (default: 7312).
    pub p2p_port: Option<u16>,
    /// Port for the local HTTP API (default: 7313).
    pub api_port: Option<u16>,
    /// Auto-connect to friends with stored addresses.
    pub auto_connect: Option<bool>,
    /// Minimum trust level to accept connections.
    pub min_trust: Option<u8>,
    /// Shell command to run when a message arrives (wake-up hook).
    pub wake_command: Option<String>,
    /// WebSocket relay URL for NAT traversal (e.g., "ws://relay.example.com:8443/ws").
    pub relay_url: Option<String>,
    /// Auto-accept policy for friend requests and project invitations.
    /// "never" = always queue for manual approval (default)
    /// "same_owner" = auto-accept only from agents with the same owner DID
    /// "trusted" = auto-accept from friends with trust >= 3
    pub auto_accept: Option<String>,
    /// Peers to connect to on startup.
    #[serde(default)]
    pub connect: Vec<ConnectTarget>,
}

/// Auto-accept policy for incoming requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoAcceptPolicy {
    /// Never auto-accept — all requests go to pending for manual approval.
    Never,
    /// Auto-accept only from agents with the same owner DID (same operator).
    SameOwner,
    /// Auto-accept from friends with trust >= 3.
    Trusted,
}

impl AutoAcceptPolicy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "same_owner" | "sameowner" | "same-owner" => Self::SameOwner,
            "trusted" => Self::Trusted,
            _ => Self::Never,
        }
    }
}

/// A remote peer to connect to on startup.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectTarget {
    pub address: String,
}

impl AgoraConfig {
    /// Default config file path: `~/.agora/config.toml`
    pub fn default_path() -> PathBuf {
        agora_home().join("config.toml")
    }

    /// Load config from a file path. Returns default config if file doesn't exist.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => {
                    tracing::info!("Loaded config from {}", path.display());
                    config
                }
                Err(e) => {
                    tracing::warn!("Failed to parse {}: {}", path.display(), e);
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Get connect targets as a list of address strings.
    pub fn connect_addresses(&self) -> Vec<String> {
        self.connect.iter().map(|c| c.address.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml_str = r#"
name = "alice"
api_port = 7313
p2p_port = 7312
auto_connect = true
min_trust = 0
wake_command = "./wake-agent.sh"

[[connect]]
address = "192.168.1.10:7312"

[[connect]]
address = "10.0.0.5:7312"
"#;
        let config: AgoraConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.name.as_deref(), Some("alice"));
        assert_eq!(config.api_port, Some(7313));
        assert_eq!(config.p2p_port, Some(7312));
        assert_eq!(config.auto_connect, Some(true));
        assert_eq!(config.min_trust, Some(0));
        assert_eq!(config.wake_command.as_deref(), Some("./wake-agent.sh"));
        assert_eq!(config.connect.len(), 2);
        assert_eq!(config.connect[0].address, "192.168.1.10:7312");
    }

    #[test]
    fn test_empty_config() {
        let config: AgoraConfig = toml::from_str("").unwrap();
        assert!(config.name.is_none());
        assert!(config.connect.is_empty());
    }

    #[test]
    fn test_missing_file() {
        let config = AgoraConfig::load(Path::new("/nonexistent/path/config.toml"));
        assert!(config.name.is_none());
    }
}
