//! Encrypted local secret store — secrets.db.
//!
//! A separate SQLite database at `~/.config/omegon/secrets.db` that stores
//! secrets encrypted at rest. Three encryption backend options:
//!
//! 1. **OS keyring** — AES key stored in macOS Keychain / Linux Secret Service / Windows WCM
//! 2. **Passphrase** — AES-256-GCM key derived via Argon2id from operator passphrase
//! 3. **Styrene Identity** — HKDF-derived key from RNS Ed25519/X25519 (feature-gated)
//!
//! The store is never in git, never synced, never archived without explicit operator action.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Size of the AES-256-GCM encryption key.
const KEY_LENGTH: usize = 32;
/// Size of the AES-GCM nonce.
const NONCE_LENGTH: usize = 12;

/// How the store encryption key was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyBackend {
    /// AES key stored in OS keyring (macOS Keychain, etc.)
    Keyring,
    /// AES key derived from passphrase via Argon2id
    Passphrase,
    /// AES key derived from Styrene RNS Identity via HKDF
    #[cfg(feature = "styrene")]
    StyreneIdentity,
}

impl std::fmt::Display for KeyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyring => write!(f, "keyring"),
            Self::Passphrase => write!(f, "passphrase"),
            #[cfg(feature = "styrene")]
            Self::StyreneIdentity => write!(f, "styrene-identity"),
        }
    }
}

/// Store metadata persisted alongside the encrypted DB.
#[derive(Debug, Serialize, Deserialize)]
struct StoreHeader {
    version: u32,
    backend: KeyBackend,
    /// Argon2id salt (only used for passphrase backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    salt: Option<Vec<u8>>,
}

/// Encrypted local secret store.
pub struct SecretStore {
    path: PathBuf,
    header_path: PathBuf,
    key: [u8; KEY_LENGTH],
    backend: KeyBackend,
}

impl SecretStore {
    /// Default store location.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("omegon")
            .join("secrets.db")
    }

    /// Initialize a new store with the OS keyring backend.
    pub fn init_keyring(path: &Path) -> anyhow::Result<Self> {
        let key = generate_random_key();

        // Store the encryption key in the OS keyring
        let entry = keyring::Entry::new("omegon-secrets", "store-key")
            .map_err(|e| anyhow::anyhow!("keyring init failed: {e}"))?;
        entry
            .set_password(&hex::encode(key))
            .map_err(|e| anyhow::anyhow!("keyring store failed: {e}"))?;

        let store = Self::create(path, key, KeyBackend::Keyring, None)?;
        Ok(store)
    }

    /// Initialize a new store with passphrase encryption (Argon2id).
    pub fn init_passphrase(path: &Path, passphrase: &str) -> anyhow::Result<Self> {
        let salt = generate_random_salt();
        let key = derive_key_argon2id(passphrase.as_bytes(), &salt);
        let store = Self::create(path, key, KeyBackend::Passphrase, Some(salt))?;
        Ok(store)
    }

    /// Open an existing store using the OS keyring.
    pub fn open_keyring(path: &Path) -> anyhow::Result<Self> {
        let header = Self::read_header(path)?;
        if header.backend != KeyBackend::Keyring {
            anyhow::bail!(
                "store was initialized with {} backend, not keyring",
                header.backend
            );
        }

        let entry = keyring::Entry::new("omegon-secrets", "store-key")
            .map_err(|e| anyhow::anyhow!("keyring access failed: {e}"))?;
        let hex_key = entry
            .get_password()
            .map_err(|e| anyhow::anyhow!("keyring read failed: {e}"))?;
        let key = hex_to_key(&hex_key)?;

        Ok(Self {
            path: path.to_path_buf(),
            header_path: header_path(path),
            key,
            backend: KeyBackend::Keyring,
        })
    }

    /// Open an existing store using a passphrase.
    pub fn open_passphrase(path: &Path, passphrase: &str) -> anyhow::Result<Self> {
        let header = Self::read_header(path)?;
        if header.backend != KeyBackend::Passphrase {
            anyhow::bail!(
                "store was initialized with {} backend, not passphrase",
                header.backend
            );
        }

        let salt = header.salt.ok_or_else(|| {
            anyhow::anyhow!("passphrase store missing salt in header")
        })?;
        let key = derive_key_argon2id(passphrase.as_bytes(), &salt);

        Ok(Self {
            path: path.to_path_buf(),
            header_path: header_path(path),
            key,
            backend: KeyBackend::Passphrase,
        })
    }

    /// Store a secret.
    pub fn put(&self, name: &str, value: &SecretString) -> anyhow::Result<()> {
        let mut secrets = self.load_all()?;
        let encrypted = self.encrypt(value.expose_secret().as_bytes())?;
        secrets.insert(name.to_string(), encrypted);
        self.save_all(&secrets)
    }

    /// Retrieve a secret.
    pub fn get(&self, name: &str) -> anyhow::Result<Option<SecretString>> {
        let secrets = self.load_all()?;
        match secrets.get(name) {
            Some(encrypted) => {
                let plaintext = self.decrypt(encrypted)?;
                let value = String::from_utf8(plaintext)
                    .map_err(|_| anyhow::anyhow!("secret is not valid UTF-8"))?;
                Ok(Some(SecretString::from(value)))
            }
            None => Ok(None),
        }
    }

    /// Delete a secret.
    pub fn delete(&self, name: &str) -> anyhow::Result<bool> {
        let mut secrets = self.load_all()?;
        let existed = secrets.remove(name).is_some();
        if existed {
            self.save_all(&secrets)?;
        }
        Ok(existed)
    }

    /// List all stored secret names (not values).
    pub fn list(&self) -> anyhow::Result<Vec<String>> {
        let secrets = self.load_all()?;
        Ok(secrets.keys().cloned().collect())
    }

    /// Which backend is this store using?
    pub fn backend(&self) -> KeyBackend {
        self.backend
    }

    /// Does the store file exist?
    pub fn exists(path: &Path) -> bool {
        header_path(path).exists()
    }

    // ── Internal ─────────────────────────────────────────────

    fn create(
        path: &Path,
        key: [u8; KEY_LENGTH],
        backend: KeyBackend,
        salt: Option<Vec<u8>>,
    ) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let header = StoreHeader {
            version: 1,
            backend,
            salt,
        };
        let header_path = header_path(path);
        std::fs::write(&header_path, serde_json::to_string_pretty(&header)?)?;

        // Write empty store
        let store = Self {
            path: path.to_path_buf(),
            header_path,
            key,
            backend,
        };
        store.save_all(&HashMap::new())?;
        Ok(store)
    }

    fn read_header(path: &Path) -> anyhow::Result<StoreHeader> {
        let hp = header_path(path);
        let content = std::fs::read_to_string(&hp)
            .map_err(|e| anyhow::anyhow!("cannot read store header {}: {e}", hp.display()))?;
        Ok(serde_json::from_str(&content)?)
    }

    fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow::anyhow!("cipher init: {e}"))?;
        let nonce_bytes = generate_random_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;

        // Prepend nonce to ciphertext
        let mut result = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn decrypt(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if data.len() < NONCE_LENGTH {
            anyhow::bail!("encrypted data too short");
        }
        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LENGTH);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow::anyhow!("cipher init: {e}"))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decrypt failed (wrong key or corrupted data): {e}"))
    }

    fn load_all(&self) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let data = std::fs::read(&self.path)?;
        if data.is_empty() {
            return Ok(HashMap::new());
        }
        Ok(serde_json::from_slice(&data)?)
    }

    fn save_all(&self, secrets: &HashMap<String, Vec<u8>>) -> anyhow::Result<()> {
        let data = serde_json::to_vec(secrets)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }
}

impl Drop for SecretStore {
    fn drop(&mut self) {
        // Zeroize the key on drop
        self.key.iter_mut().for_each(|b| *b = 0);
    }
}

fn header_path(store_path: &Path) -> PathBuf {
    store_path.with_extension("header.json")
}

fn generate_random_key() -> [u8; KEY_LENGTH] {
    let mut key = [0u8; KEY_LENGTH];
    getrandom::getrandom(&mut key).expect("getrandom failed");
    key
}

fn generate_random_nonce() -> [u8; NONCE_LENGTH] {
    let mut nonce = [0u8; NONCE_LENGTH];
    getrandom::getrandom(&mut nonce).expect("getrandom failed");
    nonce
}

fn generate_random_salt() -> Vec<u8> {
    let mut salt = vec![0u8; 32];
    getrandom::getrandom(&mut salt).expect("getrandom failed");
    salt
}

/// Derive an AES-256 key from a passphrase using Argon2id.
///
/// Argon2id is memory-hard and GPU-resistant — the winner of the
/// Password Hashing Competition. Parameters tuned for ~0.5s on
/// a modern laptop (64MB memory, 3 iterations, 4 parallelism).
fn derive_key_argon2id(passphrase: &[u8], salt: &[u8]) -> [u8; KEY_LENGTH] {
    let params = argon2::Params::new(
        64 * 1024, // 64 MiB memory cost
        3,         // 3 iterations
        4,         // 4 lanes of parallelism
        Some(KEY_LENGTH),
    )
    .expect("valid Argon2 params");

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; KEY_LENGTH];
    argon2
        .hash_password_into(passphrase, salt, &mut key)
        .expect("Argon2id key derivation failed");
    key
}

fn hex_to_key(hex_str: &str) -> anyhow::Result<[u8; KEY_LENGTH]> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| anyhow::anyhow!("invalid hex key: {e}"))?;
    if bytes.len() != KEY_LENGTH {
        anyhow::bail!("key length {} != expected {}", bytes.len(), KEY_LENGTH);
    }
    let mut key = [0u8; KEY_LENGTH];
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.db");
        (dir, path)
    }

    #[test]
    fn passphrase_init_and_open() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "test-passphrase-123").unwrap();
        assert_eq!(store.backend(), KeyBackend::Passphrase);
        assert!(SecretStore::exists(&path));
        drop(store);

        // Re-open with correct passphrase
        let store = SecretStore::open_passphrase(&path, "test-passphrase-123").unwrap();
        assert_eq!(store.backend(), KeyBackend::Passphrase);
    }

    #[test]
    fn passphrase_put_get_delete() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "hunter2").unwrap();

        // Put
        store.put("API_KEY", &SecretString::from("sk-secret-123")).unwrap();

        // Get
        let retrieved = store.get("API_KEY").unwrap().unwrap();
        assert_eq!(retrieved.expose_secret(), "sk-secret-123");

        // Get missing
        assert!(store.get("NONEXISTENT").unwrap().is_none());

        // List
        let names = store.list().unwrap();
        assert_eq!(names, vec!["API_KEY"]);

        // Delete
        assert!(store.delete("API_KEY").unwrap());
        assert!(!store.delete("API_KEY").unwrap()); // already deleted
        assert!(store.get("API_KEY").unwrap().is_none());
    }

    #[test]
    fn passphrase_wrong_key_fails_decrypt() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "correct-passphrase").unwrap();
        store.put("SECRET", &SecretString::from("value")).unwrap();
        drop(store);

        let bad_store = SecretStore::open_passphrase(&path, "wrong-passphrase").unwrap();
        // Decryption should fail
        let result = bad_store.get("SECRET");
        assert!(result.is_err(), "wrong passphrase should fail decryption");
    }

    #[test]
    fn passphrase_multiple_secrets() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "pass").unwrap();

        store.put("KEY_1", &SecretString::from("value-1")).unwrap();
        store.put("KEY_2", &SecretString::from("value-2")).unwrap();
        store.put("KEY_3", &SecretString::from("value-3")).unwrap();

        assert_eq!(store.get("KEY_1").unwrap().unwrap().expose_secret(), "value-1");
        assert_eq!(store.get("KEY_2").unwrap().unwrap().expose_secret(), "value-2");
        assert_eq!(store.get("KEY_3").unwrap().unwrap().expose_secret(), "value-3");

        let mut names = store.list().unwrap();
        names.sort();
        assert_eq!(names, vec!["KEY_1", "KEY_2", "KEY_3"]);
    }

    #[test]
    fn passphrase_overwrite_secret() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "pass").unwrap();

        store.put("KEY", &SecretString::from("old-value")).unwrap();
        store.put("KEY", &SecretString::from("new-value")).unwrap();

        assert_eq!(store.get("KEY").unwrap().unwrap().expose_secret(), "new-value");
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn passphrase_persists_across_reopens() {
        let (_dir, path) = temp_store_path();

        {
            let store = SecretStore::init_passphrase(&path, "pass").unwrap();
            store.put("PERSIST_KEY", &SecretString::from("persist-value")).unwrap();
        } // store dropped

        {
            let store = SecretStore::open_passphrase(&path, "pass").unwrap();
            let val = store.get("PERSIST_KEY").unwrap().unwrap();
            assert_eq!(val.expose_secret(), "persist-value");
        }
    }

    #[test]
    fn backend_mismatch_fails() {
        let (_dir, path) = temp_store_path();
        SecretStore::init_passphrase(&path, "pass").unwrap();

        let result = SecretStore::open_keyring(&path);
        assert!(result.is_err(), "opening passphrase store with keyring should fail");
    }

    #[test]
    fn key_backend_display() {
        assert_eq!(KeyBackend::Keyring.to_string(), "keyring");
        assert_eq!(KeyBackend::Passphrase.to_string(), "passphrase");
    }

    #[test]
    fn nonexistent_store_not_exists() {
        assert!(!SecretStore::exists(Path::new("/tmp/nonexistent/secrets.db")));
    }

    #[test]
    fn derive_key_deterministic() {
        let salt = b"test-salt-value";
        let key1 = derive_key_argon2id(b"passphrase", salt);
        let key2 = derive_key_argon2id(b"passphrase", salt);
        assert_eq!(key1, key2, "same passphrase + salt should produce same key");

        let key3 = derive_key_argon2id(b"different", salt);
        assert_ne!(key1, key3, "different passphrase should produce different key");
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "pass").unwrap();

        let plaintext = b"hello world secret data";
        let encrypted = store.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext.to_vec(), "ciphertext should differ from plaintext");
        assert!(encrypted.len() > plaintext.len(), "ciphertext should be longer (nonce + tag)");

        let decrypted = store.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext.to_vec());
    }
}
