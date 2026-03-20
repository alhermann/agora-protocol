//! Agent identity — Ed25519 keypairs, DIDs, signing, and verification.
//!
//! Each agent has a persistent Ed25519 keypair stored at `~/.agora/identity.key`.
//! The keypair generates a DID of the form `did:agora:<base58-public-key>`.
//! A per-process session ID (UUID v4) distinguishes multiple instances of the
//! same agent running concurrently.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ring::rand::SystemRandom;
use ring::signature::{self, Ed25519KeyPair, KeyPair};
use tracing::{info, warn};
use uuid::Uuid;

/// Persistent agent identity backed by an Ed25519 keypair.
pub struct AgentIdentity {
    /// The Ed25519 keypair (private + public).
    keypair: Ed25519KeyPair,
    /// Raw PKCS#8 document bytes (for serialization).
    pkcs8_bytes: Vec<u8>,
    /// DID string: `did:agora:<base58-public-key>`.
    did: String,
    /// Per-process session ID — distinguishes concurrent instances.
    session_id: Uuid,
}

impl AgentIdentity {
    /// Default path for the identity key file.
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        crate::config::agora_home().join("identity.key")
    }

    /// Load an existing identity from disk, or generate a new one.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            let identity = Self::generate()?;
            identity.save(path)?;
            info!("Generated new agent identity: {}", identity.did);
            Ok(identity)
        }
    }

    /// Generate a fresh Ed25519 keypair.
    fn generate() -> Result<Self> {
        let rng = SystemRandom::new();
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|e| anyhow::anyhow!("Failed to generate Ed25519 keypair: {}", e))?;
        let pkcs8_vec = pkcs8_bytes.as_ref().to_vec();
        let keypair = Ed25519KeyPair::from_pkcs8(&pkcs8_vec)
            .map_err(|e| anyhow::anyhow!("Failed to parse generated keypair: {}", e))?;
        let did = Self::compute_did(keypair.public_key().as_ref());
        Ok(Self {
            keypair,
            pkcs8_bytes: pkcs8_vec,
            did,
            session_id: Uuid::new_v4(),
        })
    }

    /// Load an identity from a PKCS#8 key file.
    fn load(path: &Path) -> Result<Self> {
        let pkcs8_bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read identity key from {}", path.display()))?;
        let keypair = Ed25519KeyPair::from_pkcs8(&pkcs8_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to parse identity key: {}", e))?;
        let did = Self::compute_did(keypair.public_key().as_ref());
        info!("Loaded agent identity: {} from {}", did, path.display());
        Ok(Self {
            keypair,
            pkcs8_bytes,
            did,
            session_id: Uuid::new_v4(),
        })
    }

    /// Save the PKCS#8 key to disk.
    fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &self.pkcs8_bytes)
            .with_context(|| format!("Failed to write identity key to {}", path.display()))?;
        // Restrict permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
        info!("Saved identity key to {}", path.display());
        Ok(())
    }

    /// Compute the DID from a public key: `did:agora:<base58-pubkey>`.
    fn compute_did(public_key: &[u8]) -> String {
        format!("did:agora:{}", bs58::encode(public_key).into_string())
    }

    /// The agent's DID string.
    pub fn did(&self) -> &str {
        &self.did
    }

    /// The per-process session ID.
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Raw Ed25519 public key bytes (32 bytes).
    pub fn public_key_bytes(&self) -> &[u8] {
        self.keypair.public_key().as_ref()
    }

    /// Base58-encoded public key.
    pub fn public_key_base58(&self) -> String {
        bs58::encode(self.public_key_bytes()).into_string()
    }

    /// Sign arbitrary data with the agent's Ed25519 private key.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        self.keypair.sign(data).as_ref().to_vec()
    }

    /// Verify a signature against a public key.
    /// Returns true if the signature is valid.
    pub fn verify(public_key: &[u8], data: &[u8], sig: &[u8]) -> bool {
        let peer_public_key = signature::UnparsedPublicKey::new(&signature::ED25519, public_key);
        peer_public_key.verify(data, sig).is_ok()
    }

    /// Verify a signature given a base58-encoded public key.
    pub fn verify_base58(public_key_b58: &str, data: &[u8], sig_b58: &str) -> bool {
        let Ok(pubkey) = bs58::decode(public_key_b58).into_vec() else {
            warn!("Invalid base58 public key: {}", public_key_b58);
            return false;
        };
        let Ok(sig) = bs58::decode(sig_b58).into_vec() else {
            warn!("Invalid base58 signature");
            return false;
        };
        Self::verify(&pubkey, data, &sig)
    }
}

// ---------------------------------------------------------------------------
// Owner Identity — represents the human who owns one or more agent instances
// ---------------------------------------------------------------------------

/// Persistent owner identity backed by an Ed25519 keypair.
/// Unlike AgentIdentity, this is never auto-generated — it requires
/// explicit `agora owner init` or `agora owner import`.
pub struct OwnerIdentity {
    keypair: Ed25519KeyPair,
    pkcs8_bytes: Vec<u8>,
    did: String,
}

impl OwnerIdentity {
    /// Default path for the owner key file.
    pub fn default_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        crate::config::agora_home().join("owner.key")
    }

    /// Generate a fresh Ed25519 keypair for the owner.
    pub fn generate() -> Result<Self> {
        let rng = SystemRandom::new();
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|e| anyhow::anyhow!("Failed to generate owner Ed25519 keypair: {}", e))?;
        let pkcs8_vec = pkcs8_bytes.as_ref().to_vec();
        let keypair = Ed25519KeyPair::from_pkcs8(&pkcs8_vec)
            .map_err(|e| anyhow::anyhow!("Failed to parse generated owner keypair: {}", e))?;
        let did = Self::compute_did(keypair.public_key().as_ref());
        Ok(Self {
            keypair,
            pkcs8_bytes: pkcs8_vec,
            did,
        })
    }

    /// Load an owner identity from a PKCS#8 key file.
    pub fn load(path: &Path) -> Result<Self> {
        let pkcs8_bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read owner key from {}", path.display()))?;
        Self::from_pkcs8_bytes(&pkcs8_bytes)
    }

    /// Construct from raw PKCS#8 bytes.
    pub fn from_pkcs8_bytes(pkcs8_bytes: &[u8]) -> Result<Self> {
        let keypair = Ed25519KeyPair::from_pkcs8(pkcs8_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to parse owner key: {}", e))?;
        let did = Self::compute_did(keypair.public_key().as_ref());
        Ok(Self {
            keypair,
            pkcs8_bytes: pkcs8_bytes.to_vec(),
            did,
        })
    }

    /// Save the PKCS#8 key to disk with restrictive permissions.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &self.pkcs8_bytes)
            .with_context(|| format!("Failed to write owner key to {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
        info!("Saved owner key to {}", path.display());
        Ok(())
    }

    /// Compute the owner DID: `did:agora:owner:<base58-pubkey>`.
    fn compute_did(public_key: &[u8]) -> String {
        format!("did:agora:owner:{}", bs58::encode(public_key).into_string())
    }

    /// The owner's DID string.
    pub fn did(&self) -> &str {
        &self.did
    }

    /// Base58-encoded public key.
    pub fn public_key_base58(&self) -> String {
        bs58::encode(self.keypair.public_key().as_ref()).into_string()
    }

    /// Raw PKCS#8 bytes (for export).
    pub fn pkcs8_bytes(&self) -> &[u8] {
        &self.pkcs8_bytes
    }

    /// Sign arbitrary data with the owner's Ed25519 private key.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        self.keypair.sign(data).as_ref().to_vec()
    }

    /// Create an attestation binding this owner to an agent DID.
    pub fn attest_agent(&self, agent_did: &str) -> OwnerAttestation {
        let timestamp = chrono::Utc::now().timestamp();
        let canonical = OwnerAttestation::canonical_message(&self.did, agent_did, timestamp);
        let sig = self.sign(canonical.as_bytes());
        OwnerAttestation {
            owner_did: self.did.clone(),
            owner_public_key: self.public_key_base58(),
            agent_did: agent_did.to_string(),
            created_at: timestamp,
            signature: bs58::encode(&sig).into_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Owner Attestation — cryptographic proof of owner→agent binding
// ---------------------------------------------------------------------------

/// Cryptographic binding: owner (human) → agent (device).
/// The owner's Ed25519 key signs a canonical message containing both DIDs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OwnerAttestation {
    pub owner_did: String,
    pub owner_public_key: String, // base58
    pub agent_did: String,
    pub created_at: i64,   // unix timestamp
    pub signature: String, // base58
}

impl OwnerAttestation {
    /// Default path for the attestation file.
    pub fn default_path() -> PathBuf {
        crate::config::agora_home().join("owner_attestation.json")
    }

    /// Build the canonical message that gets signed.
    /// Domain-separated to prevent cross-protocol attacks.
    pub fn canonical_message(owner_did: &str, agent_did: &str, timestamp: i64) -> String {
        format!(
            "agora:owner-attestation:v1:{}:{}:{}",
            owner_did, agent_did, timestamp
        )
    }

    /// Verify this attestation:
    /// 1. owner_did derives from owner_public_key
    /// 2. Signature is valid over the canonical message
    pub fn verify(&self) -> bool {
        // Check that owner_did matches owner_public_key
        let Ok(_pubkey_bytes) = bs58::decode(&self.owner_public_key).into_vec() else {
            warn!("Invalid base58 owner public key");
            return false;
        };
        let expected_did = format!("did:agora:owner:{}", &self.owner_public_key);
        if self.owner_did != expected_did {
            warn!(
                "Owner DID mismatch: expected {}, got {}",
                expected_did, self.owner_did
            );
            return false;
        }

        // Verify signature
        let canonical = Self::canonical_message(&self.owner_did, &self.agent_did, self.created_at);
        AgentIdentity::verify_base58(
            &self.owner_public_key,
            canonical.as_bytes(),
            &self.signature,
        )
    }

    /// Verify and also check that the agent_did matches what we expect.
    pub fn verify_for_agent(&self, expected_agent_did: &str) -> bool {
        if self.agent_did != expected_agent_did {
            warn!(
                "Attestation agent_did mismatch: expected {}, got {}",
                expected_agent_did, self.agent_did
            );
            return false;
        }
        self.verify()
    }

    /// Load from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read attestation from {}", path.display()))?;
        let att: Self = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse attestation from {}", path.display()))?;
        Ok(att)
    }

    /// Save to disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)
            .with_context(|| format!("Failed to write attestation to {}", path.display()))?;
        info!("Saved owner attestation to {}", path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_sign() {
        let identity = AgentIdentity::generate().unwrap();
        assert!(identity.did().starts_with("did:agora:"));
        assert_eq!(identity.public_key_bytes().len(), 32);

        let data = b"hello agora";
        let sig = identity.sign(data);
        assert!(AgentIdentity::verify(
            identity.public_key_bytes(),
            data,
            &sig
        ));

        // Tampered data should fail
        assert!(!AgentIdentity::verify(
            identity.public_key_bytes(),
            b"tampered",
            &sig
        ));
    }

    #[test]
    fn test_did_format() {
        let identity = AgentIdentity::generate().unwrap();
        let did = identity.did();
        assert!(did.starts_with("did:agora:"));
        // base58 of 32 bytes is typically 43-44 chars
        let key_part = &did["did:agora:".len()..];
        assert!(key_part.len() >= 40 && key_part.len() <= 50);
    }

    #[test]
    fn test_save_and_load() {
        let dir = std::env::temp_dir().join(format!("agora-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("identity.key");

        let id1 = AgentIdentity::generate().unwrap();
        id1.save(&path).unwrap();

        let id2 = AgentIdentity::load(&path).unwrap();
        assert_eq!(id1.did(), id2.did());
        assert_eq!(id1.public_key_bytes(), id2.public_key_bytes());

        // Session IDs should differ (new process)
        assert_ne!(id1.session_id(), id2.session_id());

        // Signature from id1 verifiable with id2's public key
        let sig = id1.sign(b"test");
        assert!(AgentIdentity::verify(id2.public_key_bytes(), b"test", &sig));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_base58_verify() {
        let identity = AgentIdentity::generate().unwrap();
        let data = b"verify me";
        let sig = identity.sign(data);
        let sig_b58 = bs58::encode(&sig).into_string();
        let pk_b58 = identity.public_key_base58();

        assert!(AgentIdentity::verify_base58(&pk_b58, data, &sig_b58));
        assert!(!AgentIdentity::verify_base58(&pk_b58, b"wrong", &sig_b58));
    }

    // --- Owner identity tests ---

    #[test]
    fn test_owner_generate_and_did() {
        let owner = OwnerIdentity::generate().unwrap();
        assert!(owner.did().starts_with("did:agora:owner:"));
        let key_part = &owner.did()["did:agora:owner:".len()..];
        assert!(key_part.len() >= 40 && key_part.len() <= 50);
    }

    #[test]
    fn test_owner_save_and_load() {
        let dir = std::env::temp_dir().join(format!("agora-owner-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("owner.key");

        let o1 = OwnerIdentity::generate().unwrap();
        o1.save(&path).unwrap();

        let o2 = OwnerIdentity::load(&path).unwrap();
        assert_eq!(o1.did(), o2.did());
        assert_eq!(o1.public_key_base58(), o2.public_key_base58());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_owner_attestation_create_and_verify() {
        let owner = OwnerIdentity::generate().unwrap();
        let agent = AgentIdentity::generate().unwrap();

        let att = owner.attest_agent(agent.did());
        assert_eq!(att.owner_did, owner.did());
        assert_eq!(att.agent_did, agent.did());

        // Verify passes
        assert!(att.verify());
        assert!(att.verify_for_agent(agent.did()));

        // Wrong agent DID fails
        assert!(!att.verify_for_agent("did:agora:wrong"));
    }

    #[test]
    fn test_owner_attestation_tamper_detection() {
        let owner = OwnerIdentity::generate().unwrap();
        let agent = AgentIdentity::generate().unwrap();
        let att = owner.attest_agent(agent.did());

        // Tamper with agent_did
        let mut tampered = att.clone();
        tampered.agent_did = "did:agora:tampered".to_string();
        assert!(!tampered.verify());

        // Tamper with owner_did
        let mut tampered2 = att.clone();
        tampered2.owner_did = "did:agora:owner:tampered".to_string();
        assert!(!tampered2.verify());

        // Tamper with timestamp
        let mut tampered3 = att.clone();
        tampered3.created_at += 1;
        assert!(!tampered3.verify());
    }

    #[test]
    fn test_owner_attestation_save_and_load() {
        let dir = std::env::temp_dir().join(format!("agora-att-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("attestation.json");

        let owner = OwnerIdentity::generate().unwrap();
        let agent = AgentIdentity::generate().unwrap();
        let att = owner.attest_agent(agent.did());

        att.save(&path).unwrap();
        let loaded = OwnerAttestation::load(&path).unwrap();

        assert_eq!(att.owner_did, loaded.owner_did);
        assert_eq!(att.agent_did, loaded.agent_did);
        assert!(loaded.verify());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
