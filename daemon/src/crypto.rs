//! Data-at-rest encryption using Argon2id (key derivation) + AES-256-GCM.
//!
//! Encrypted file format: `[1-byte version][12-byte nonce][ciphertext + 16-byte tag]`
//! Salt and metadata stored in `~/.agora/crypto.json` (plaintext).

use std::path::{Path, PathBuf};

use ring::aead::{self, AES_256_GCM, BoundKey, OpeningKey, SealingKey, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;

/// Metadata stored alongside the encrypted files (plaintext).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoMeta {
    /// Argon2id salt (hex-encoded).
    pub salt: String,
    /// Version of the encryption format.
    pub version: u8,
}

impl CryptoMeta {
    pub fn default_path() -> PathBuf {
        
        crate::config::agora_home().join("crypto.json")
    }

    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }
}

/// A derived encryption key from a passphrase + salt.
#[derive(Clone)]
pub struct DerivedKey {
    key_bytes: [u8; 32],
}

impl DerivedKey {
    /// Derive a 256-bit key from a passphrase using Argon2id.
    pub fn derive(passphrase: &str, salt: &[u8]) -> anyhow::Result<Self> {
        use argon2::Argon2;

        let mut key_bytes = [0u8; 32];
        let argon2 = Argon2::default();
        argon2
            .hash_password_into(passphrase.as_bytes(), salt, &mut key_bytes)
            .map_err(|e| anyhow::anyhow!("Argon2 key derivation failed: {}", e))?;
        Ok(Self { key_bytes })
    }
}

/// Nonce sequence that yields a single nonce then fails.
struct OneNonceSequence(Option<aead::Nonce>);

impl OneNonceSequence {
    fn new(nonce: aead::Nonce) -> Self {
        Self(Some(nonce))
    }
}

impl aead::NonceSequence for OneNonceSequence {
    fn advance(&mut self) -> Result<aead::Nonce, ring::error::Unspecified> {
        self.0.take().ok_or(ring::error::Unspecified)
    }
}

/// Encrypt bytes with AES-256-GCM.
/// Returns `[version][nonce][ciphertext + tag]`.
pub fn encrypt_bytes(key: &DerivedKey, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| anyhow::anyhow!("Failed to generate random nonce"))?;

    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

    let unbound_key = UnboundKey::new(&AES_256_GCM, &key.key_bytes)
        .map_err(|_| anyhow::anyhow!("Failed to create AES key"))?;
    let mut sealing_key = SealingKey::new(unbound_key, OneNonceSequence::new(nonce));

    let mut in_out = plaintext.to_vec();
    sealing_key
        .seal_in_place_append_tag(aead::Aad::empty(), &mut in_out)
        .map_err(|_| anyhow::anyhow!("AES-256-GCM encryption failed"))?;

    // Build output: version + nonce + ciphertext_with_tag
    let mut output = Vec::with_capacity(1 + NONCE_LEN + in_out.len());
    output.push(VERSION);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&in_out);
    Ok(output)
}

/// Decrypt bytes encrypted with `encrypt_bytes`.
pub fn decrypt_bytes(key: &DerivedKey, encrypted: &[u8]) -> anyhow::Result<Vec<u8>> {
    if encrypted.len() < 1 + NONCE_LEN + 16 {
        anyhow::bail!("Encrypted data too short");
    }

    let version = encrypted[0];
    if version != VERSION {
        anyhow::bail!("Unsupported encryption version: {}", version);
    }

    let nonce_bytes: [u8; NONCE_LEN] = encrypted[1..1 + NONCE_LEN]
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid nonce length"))?;
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);

    let mut ciphertext = encrypted[1 + NONCE_LEN..].to_vec();

    let unbound_key = UnboundKey::new(&AES_256_GCM, &key.key_bytes)
        .map_err(|_| anyhow::anyhow!("Failed to create AES key"))?;
    let mut opening_key = OpeningKey::new(unbound_key, OneNonceSequence::new(nonce));

    let plaintext = opening_key
        .open_in_place(aead::Aad::empty(), &mut ciphertext)
        .map_err(|_| {
            anyhow::anyhow!("AES-256-GCM decryption failed (wrong key or corrupted data)")
        })?;

    Ok(plaintext.to_vec())
}

/// Generate a random 16-byte salt.
pub fn generate_salt() -> anyhow::Result<[u8; 16]> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; 16];
    rng.fill(&mut salt)
        .map_err(|_| anyhow::anyhow!("Failed to generate random salt"))?;
    Ok(salt)
}

/// Encrypt a JSON-serializable value and write to a file.
pub fn encrypt_to_file<T: Serialize>(
    key: &DerivedKey,
    value: &T,
    path: &Path,
) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    let encrypted = encrypt_bytes(key, json.as_bytes())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, encrypted)?;
    Ok(())
}

/// Read an encrypted file and deserialize as JSON.
pub fn decrypt_from_file<T: serde::de::DeserializeOwned>(
    key: &DerivedKey,
    path: &Path,
) -> anyhow::Result<T> {
    let encrypted = std::fs::read(path)?;
    let plaintext = decrypt_bytes(key, &encrypted)?;
    let value: T = serde_json::from_slice(&plaintext)?;
    Ok(value)
}

/// Check if a file appears to be encrypted (starts with our version byte
/// and is not valid JSON).
pub fn is_encrypted(path: &Path) -> bool {
    match std::fs::read(path) {
        Ok(data) if !data.is_empty() => {
            data[0] == VERSION && serde_json::from_slice::<serde_json::Value>(&data).is_err()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_key() -> DerivedKey {
        let salt = [0u8; 16];
        DerivedKey::derive("test-passphrase", &salt).unwrap()
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = b"Hello, World! This is a secret message.";
        let encrypted = encrypt_bytes(&key, plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        assert!(encrypted.len() > plaintext.len());
        let decrypted = decrypt_bytes(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = test_key();
        let key2 = DerivedKey::derive("wrong-passphrase", &[0u8; 16]).unwrap();
        let encrypted = encrypt_bytes(&key1, b"secret").unwrap();
        assert!(decrypt_bytes(&key2, &encrypted).is_err());
    }

    #[test]
    fn test_corrupted_data_fails() {
        let key = test_key();
        let mut encrypted = encrypt_bytes(&key, b"secret").unwrap();
        // Corrupt a byte
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;
        assert!(decrypt_bytes(&key, &encrypted).is_err());
    }

    #[test]
    fn test_empty_plaintext() {
        let key = test_key();
        let encrypted = encrypt_bytes(&key, b"").unwrap();
        let decrypted = decrypt_bytes(&key, &encrypted).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_file_encrypt_decrypt() {
        let dir = TempDir::new().unwrap();
        let key = test_key();
        let path = dir.path().join("test.enc");

        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "alice".to_string(),
            value: 42,
        };

        encrypt_to_file(&key, &data, &path).unwrap();
        assert!(is_encrypted(&path));

        let loaded: TestData = decrypt_from_file(&key, &path).unwrap();
        assert_eq!(loaded, data);
    }

    #[test]
    fn test_is_encrypted_plaintext() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.json");
        std::fs::write(&path, r#"{"hello": "world"}"#).unwrap();
        assert!(!is_encrypted(&path));
    }

    #[test]
    fn test_derive_key() {
        let salt = generate_salt().unwrap();
        let key = DerivedKey::derive("my passphrase", &salt).unwrap();
        // Verify deterministic: same passphrase + salt = same key
        let key2 = DerivedKey::derive("my passphrase", &salt).unwrap();
        assert_eq!(key.key_bytes, key2.key_bytes);
        // Different passphrase = different key
        let key3 = DerivedKey::derive("different", &salt).unwrap();
        assert_ne!(key.key_bytes, key3.key_bytes);
    }

    #[test]
    fn test_crypto_meta_save_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("crypto.json");
        let meta = CryptoMeta {
            salt: "deadbeef".to_string(),
            version: 1,
        };
        meta.save(&path).unwrap();
        let loaded = CryptoMeta::load(&path).unwrap();
        assert_eq!(loaded.salt, "deadbeef");
        assert_eq!(loaded.version, 1);
    }
}
