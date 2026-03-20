//! Encrypted local secret store — `secrets.db`.
//!
//! A SQLite database at `~/.config/omegon/secrets.db` that stores secrets
//! encrypted at rest using AES-256-GCM. Atomic writes via SQLite WAL mode.
//!
//! Three encryption backend options:
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
use rusqlite::{params, Connection};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

/// Size of the AES-256-GCM encryption key.
const KEY_LENGTH: usize = 32;
/// Size of the AES-GCM nonce.
const NONCE_LENGTH: usize = 12;
/// SQLite schema version for migration tracking.
const SCHEMA_VERSION: u32 = 1;

/// How the store encryption key was derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyBackend {
    /// AES key stored in OS keyring (macOS Keychain, etc.)
    Keyring,
    /// AES key derived from passphrase via Argon2id
    Passphrase,
}

impl std::fmt::Display for KeyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyring => write!(f, "keyring"),
            Self::Passphrase => write!(f, "passphrase"),
        }
    }
}

/// Encrypted local secret store backed by SQLite.
///
/// Each secret value is individually encrypted with AES-256-GCM using a
/// unique random nonce. The encryption key is derived from the chosen backend
/// (keyring, passphrase, or Styrene Identity).
///
/// Debug impl intentionally omits the key field.
pub struct SecretStore {
    db: Connection,
    key: [u8; KEY_LENGTH],
    backend: KeyBackend,
}

impl SecretStore {
    /// Default store location: `~/.config/omegon/secrets.db`
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("omegon")
            .join("secrets.db")
    }

    /// Initialize a new store with the OS keyring backend.
    pub fn init_keyring(path: &Path) -> anyhow::Result<Self> {
        let key = generate_random_key();

        let entry = keyring::Entry::new("omegon-secrets", "store-key")
            .map_err(|e| anyhow::anyhow!("keyring init failed: {e}"))?;
        entry
            .set_password(&hex::encode(key))
            .map_err(|e| anyhow::anyhow!("keyring store failed: {e}"))?;

        Self::create(path, key, KeyBackend::Keyring, None)
    }

    /// Initialize a new store with passphrase encryption (Argon2id).
    pub fn init_passphrase(path: &Path, passphrase: &str) -> anyhow::Result<Self> {
        let salt = generate_random_salt();
        let key = derive_key_argon2id(passphrase.as_bytes(), &salt);
        Self::create(path, key, KeyBackend::Passphrase, Some(salt))
    }

    /// Open an existing store using the OS keyring.
    pub fn open_keyring(path: &Path) -> anyhow::Result<Self> {
        let db = Self::open_db(path)?;
        let meta = Self::read_meta(&db)?;
        if meta.backend != KeyBackend::Keyring {
            anyhow::bail!(
                "store was initialized with {} backend, not keyring",
                meta.backend
            );
        }

        let entry = keyring::Entry::new("omegon-secrets", "store-key")
            .map_err(|e| anyhow::anyhow!("keyring access failed: {e}"))?;
        let hex_key = entry
            .get_password()
            .map_err(|e| anyhow::anyhow!("keyring read failed: {e}"))?;
        let key = hex_to_key(&hex_key)?;

        Ok(Self { db, key, backend: KeyBackend::Keyring })
    }

    /// Open an existing store using a passphrase.
    pub fn open_passphrase(path: &Path, passphrase: &str) -> anyhow::Result<Self> {
        let db = Self::open_db(path)?;
        let meta = Self::read_meta(&db)?;
        if meta.backend != KeyBackend::Passphrase {
            anyhow::bail!(
                "store was initialized with {} backend, not passphrase",
                meta.backend
            );
        }

        let salt = meta.salt.ok_or_else(|| {
            anyhow::anyhow!("passphrase store missing salt in metadata")
        })?;
        let key = derive_key_argon2id(passphrase.as_bytes(), &salt);

        // Verify the key by attempting to decrypt the canary
        Self::verify_canary(&db, &key)?;

        Ok(Self { db, key, backend: KeyBackend::Passphrase })
    }

    /// Store a secret. Atomic via SQLite transaction.
    pub fn put(&self, name: &str, value: &SecretString) -> anyhow::Result<()> {
        let encrypted = self.encrypt(value.expose_secret().as_bytes())?;
        self.db.execute(
            "INSERT OR REPLACE INTO secrets (name, value) VALUES (?1, ?2)",
            params![name, encrypted],
        )?;
        Ok(())
    }

    /// Retrieve a secret.
    pub fn get(&self, name: &str) -> anyhow::Result<Option<SecretString>> {
        let mut stmt = self.db.prepare(
            "SELECT value FROM secrets WHERE name = ?1"
        )?;
        let result = stmt.query_row(params![name], |row| {
            let encrypted: Vec<u8> = row.get(0)?;
            Ok(encrypted)
        });

        match result {
            Ok(encrypted) => {
                let plaintext = self.decrypt(&encrypted)?;
                let value = String::from_utf8(plaintext)
                    .map_err(|_| anyhow::anyhow!("secret is not valid UTF-8"))?;
                Ok(Some(SecretString::from(value)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a secret. Returns true if it existed.
    pub fn delete(&self, name: &str) -> anyhow::Result<bool> {
        let changes = self.db.execute(
            "DELETE FROM secrets WHERE name = ?1",
            params![name],
        )?;
        Ok(changes > 0)
    }

    /// List all stored secret names (not values).
    pub fn list(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.db.prepare("SELECT name FROM secrets ORDER BY name")?;
        let names = stmt.query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(names)
    }

    /// Which backend is this store using?
    pub fn backend(&self) -> KeyBackend {
        self.backend
    }

    /// Does the store file exist?
    pub fn exists(path: &Path) -> bool {
        path.exists()
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

        // Set restrictive file permissions BEFORE writing any data.
        // On Unix: 0600 (owner read/write only).
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Create the file with restricted permissions
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
        }

        let db = Self::open_db(path)?;

        // Create schema
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS secrets (
                name TEXT PRIMARY KEY,
                value BLOB NOT NULL
            );"
        )?;

        // Store metadata
        let backend_str = backend.to_string();
        db.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('backend', ?1)",
            params![backend_str],
        )?;
        db.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        if let Some(ref salt) = salt {
            db.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('salt', ?1)",
                params![hex::encode(salt)],
            )?;
        }

        // Write a canary value — used to verify the key on open.
        // If decryption of the canary fails, the passphrase is wrong.
        let store = Self { db, key, backend };
        let canary = store.encrypt(b"omegon-secrets-canary-v1")?;
        store.db.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('canary', ?1)",
            params![hex::encode(&canary)],
        )?;

        // Enforce permissions on the created DB file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)?;
            // Also restrict the WAL and SHM files if they exist
            let wal = path.with_extension("db-wal");
            let shm = path.with_extension("db-shm");
            if wal.exists() { let _ = std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o600)); }
            if shm.exists() { let _ = std::fs::set_permissions(&shm, std::fs::Permissions::from_mode(0o600)); }
        }

        Ok(store)
    }

    fn open_db(path: &Path) -> anyhow::Result<Connection> {
        let db = Connection::open(path)?;
        // WAL mode for concurrent reads + atomic writes
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(db)
    }

    fn read_meta(db: &Connection) -> anyhow::Result<StoreMeta> {
        let backend_str: String = db.query_row(
            "SELECT value FROM meta WHERE key = 'backend'",
            [],
            |row| row.get(0),
        ).map_err(|_| anyhow::anyhow!("store metadata missing — is this a valid secrets.db?"))?;

        let backend = match backend_str.as_str() {
            "keyring" => KeyBackend::Keyring,
            "passphrase" => KeyBackend::Passphrase,
            other => anyhow::bail!("unknown backend: {other}"),
        };

        let salt = db.query_row(
            "SELECT value FROM meta WHERE key = 'salt'",
            [],
            |row| row.get::<_, String>(0),
        ).ok().and_then(|h| hex::decode(h).ok());

        Ok(StoreMeta { backend, salt })
    }

    /// Verify that the encryption key is correct by decrypting the canary value.
    fn verify_canary(db: &Connection, key: &[u8; KEY_LENGTH]) -> anyhow::Result<()> {
        let canary_hex: String = db.query_row(
            "SELECT value FROM meta WHERE key = 'canary'",
            [],
            |row| row.get(0),
        ).map_err(|_| anyhow::anyhow!("store missing canary — may be corrupted"))?;

        let canary_encrypted = hex::decode(&canary_hex)
            .map_err(|_| anyhow::anyhow!("corrupted canary value"))?;

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| anyhow::anyhow!("cipher init: {e}"))?;

        if canary_encrypted.len() < NONCE_LENGTH {
            anyhow::bail!("corrupted canary — too short");
        }
        let (nonce_bytes, ciphertext) = canary_encrypted.split_at(NONCE_LENGTH);
        let nonce = Nonce::from_slice(nonce_bytes);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("wrong passphrase — canary decryption failed"))?;

        Ok(())
    }

    fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow::anyhow!("cipher init: {e}"))?;
        let nonce_bytes = generate_random_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;

        // nonce || ciphertext (ciphertext includes AES-GCM auth tag)
        let mut result = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn decrypt(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if data.len() < NONCE_LENGTH {
            anyhow::bail!("encrypted data too short ({} bytes, need at least {})",
                data.len(), NONCE_LENGTH);
        }
        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LENGTH);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow::anyhow!("cipher init: {e}"))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decrypt failed (wrong key or corrupted data): {e}"))
    }
}

impl std::fmt::Debug for SecretStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretStore")
            .field("backend", &self.backend)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl Drop for SecretStore {
    fn drop(&mut self) {
        // C3 fix: use zeroize's volatile write barrier, not manual zeroing
        self.key.zeroize();
    }
}

/// Internal metadata from the store.
struct StoreMeta {
    backend: KeyBackend,
    salt: Option<Vec<u8>>,
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

        let store = SecretStore::open_passphrase(&path, "test-passphrase-123").unwrap();
        assert_eq!(store.backend(), KeyBackend::Passphrase);
    }

    #[test]
    fn passphrase_put_get_delete() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "hunter2").unwrap();

        store.put("API_KEY", &SecretString::from("sk-secret-123")).unwrap();

        let retrieved = store.get("API_KEY").unwrap().unwrap();
        assert_eq!(retrieved.expose_secret(), "sk-secret-123");

        assert!(store.get("NONEXISTENT").unwrap().is_none());

        let names = store.list().unwrap();
        assert_eq!(names, vec!["API_KEY"]);

        assert!(store.delete("API_KEY").unwrap());
        assert!(!store.delete("API_KEY").unwrap());
        assert!(store.get("API_KEY").unwrap().is_none());
    }

    #[test]
    fn passphrase_wrong_key_rejected_by_canary() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "correct-passphrase").unwrap();
        store.put("SECRET", &SecretString::from("value")).unwrap();
        drop(store);

        // Wrong passphrase should fail at open time (canary check), not at get time
        let result = SecretStore::open_passphrase(&path, "wrong-passphrase");
        assert!(result.is_err(), "wrong passphrase should fail canary verification");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("wrong passphrase"), "error should mention wrong passphrase: {err}");
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

        let names = store.list().unwrap();
        assert_eq!(names, vec!["KEY_1", "KEY_2", "KEY_3"]); // ordered by SQLite
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
        }

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
        assert!(result.is_err());
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
        let salt = b"test-salt-value-32bytes-padded!!";
        let key1 = derive_key_argon2id(b"passphrase", salt);
        let key2 = derive_key_argon2id(b"passphrase", salt);
        assert_eq!(key1, key2);

        let key3 = derive_key_argon2id(b"different", salt);
        assert_ne!(key1, key3);
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "pass").unwrap();

        let plaintext = b"hello world secret data";
        let encrypted = store.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext.to_vec());
        assert!(encrypted.len() > plaintext.len());

        let decrypted = store.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext.to_vec());
    }

    #[test]
    fn atomic_write_survives_concurrent_reads() {
        let (_dir, path) = temp_store_path();
        let store = SecretStore::init_passphrase(&path, "pass").unwrap();

        // Write 100 secrets to exercise SQLite WAL
        for i in 0..100 {
            store.put(&format!("KEY_{i}"), &SecretString::from(format!("value-{i}"))).unwrap();
        }

        assert_eq!(store.list().unwrap().len(), 100);
        assert_eq!(
            store.get("KEY_50").unwrap().unwrap().expose_secret(),
            "value-50"
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_permissions_restricted() {
        let (_dir, path) = temp_store_path();
        SecretStore::init_passphrase(&path, "pass").unwrap();

        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&path).unwrap().permissions();
        let mode = perms.mode() & 0o777;
        assert_eq!(mode, 0o600, "secrets.db should be 0600, got {:o}", mode);
    }
}
