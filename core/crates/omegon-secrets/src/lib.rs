//! Secret management for Omegon.
//!
//! Layers:
//! 1. Resolution — resolve secrets from env vars, keyring, shell commands
//! 2. Redaction — scrub known secret values from tool output (Aho-Corasick single-pass)
//! 3. Tool guards — block/confirm tool calls accessing sensitive paths
//! 4. Audit log — append-only record of guard decisions
//!
//! Security properties:
//! - Secret values wrapped in `secrecy::SecretString` — zeroized on drop
//! - Keyring access via `keyring` crate — cross-platform (macOS/Linux/Windows)
//! - Redaction via `aho-corasick` — single-pass, no quadratic behavior
//! - Recipes store *how* to resolve secrets, never the secret values themselves

mod audit;
mod guards;
mod recipes;
mod redact;
mod resolve;
pub mod store;
mod vault;

pub use audit::AuditLog;
pub use guards::{GuardDecision, PathGuard};
pub use recipes::{Recipe, RecipeStore};
pub use redact::Redactor;
pub use resolve::{store_in_keyring, delete_from_keyring, resolve_vault_secret};
pub use store::{KeyBackend, SecretStore};
pub use vault::{AuthConfig, VaultClient, VaultConfig};

use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

/// Central secrets manager — owns the redaction set, recipes, guards, and Vault client.
pub struct SecretsManager {
    /// Resolved secret values for redaction (name → SecretString).
    /// Values are zeroized when dropped.
    redaction_set: Arc<RwLock<HashMap<String, SecretString>>>,
    /// Pre-compiled Aho-Corasick redactor (rebuilt when secrets change).
    redactor: Arc<RwLock<Redactor>>,
    /// Recipe store (persisted to ~/.omegon/secrets.json)
    recipes: RecipeStore,
    /// Path guard for sensitive file access
    path_guard: PathGuard,
    /// Audit log
    audit: AuditLog,
    /// Vault client (optional, only if vault.json exists or VAULT_ADDR set)
    vault_client: Arc<Mutex<Option<VaultClient>>>,
}

impl SecretsManager {
    /// Create a new secrets manager, loading recipes from the config directory.
    pub fn new(config_dir: &std::path::Path) -> anyhow::Result<Self> {
        let recipes = RecipeStore::load(config_dir)?;
        let audit = AuditLog::new(config_dir);
        let path_guard = PathGuard::new();

        let mut mgr = Self {
            redaction_set: Arc::new(RwLock::new(HashMap::new())),
            redactor: Arc::new(RwLock::new(Redactor::build(&HashMap::new()))),
            recipes,
            path_guard,
            audit,
            vault_client: Arc::new(Mutex::new(None)),
        };

        // Pre-resolve all known secrets into the redaction set
        mgr.refresh_redaction_set();

        Ok(mgr)
    }

    /// Initialize Vault client if configuration is found.
    ///
    /// **Fail-closed**: only stores `Some(client)` when authentication succeeds.
    /// If auth fails, `vault_client` remains `None` and all `vault:` recipes
    /// will return `None`. Health/seal checks are still available via
    /// `vault_health_probe()` which creates a throwaway unauthenticated client.
    pub async fn init_vault(&self, config_dir: &std::path::Path) -> anyhow::Result<()> {
        if let Some(config) = VaultConfig::load_config(config_dir)? {
            tracing::info!(addr = %config.addr, "initializing vault client");
            
            match VaultClient::new(config) {
                Ok(mut client) => {
                    // Attempt authentication — fail-closed: only store if auth succeeds
                    match client.authenticate().await {
                        Ok(()) => {
                            tracing::info!("vault client authenticated successfully");
                            *self.vault_client.lock().await = Some(client);
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "vault authentication failed — vault client NOT stored, \
                                 vault: recipes will return None"
                            );
                            // Do NOT store the client — fail-closed
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "failed to create vault client");
                }
            }
        } else {
            tracing::debug!("no vault configuration found - vault recipes disabled");
        }
        
        Ok(())
    }

    /// Probe Vault health without requiring authentication.
    ///
    /// Creates a throwaway client for health/seal checks. Used by startup
    /// notifications and `/vault status` when the authenticated client isn't available.
    pub async fn vault_health_probe(config_dir: &std::path::Path) -> Option<vault::HealthStatus> {
        let config = VaultConfig::load_config(config_dir).ok()??;
        let client = VaultClient::new(config).ok()?;
        client.health().await.ok()
    }

    /// Get a locked reference to the vault client for direct access.
    pub async fn vault_client(&self) -> tokio::sync::MutexGuard<'_, Option<VaultClient>> {
        self.vault_client.lock().await
    }

    /// Check vault health and return status info for /whoami.
    pub async fn vault_status(&self) -> Option<String> {
        let client = self.vault_client.lock().await;
        if let Some(ref vault) = *client {
            match vault.health().await {
                Ok(health) => {
                    if health.sealed {
                        Some("vault: sealed".to_string())
                    } else {
                        Some(format!("vault: active ({})", vault.server_addr()))
                    }
                }
                Err(_) => Some("vault: unreachable".to_string()),
            }
        } else {
            None
        }
    }

    /// Resolve a secret by name. Checks env vars first, then recipes.
    /// For vault: recipes, this will try async resolution and fall back to None.
    pub fn resolve(&self, name: &str) -> Option<String> {
        resolve::resolve_secret(name, &self.recipes)
            .map(|s| s.expose_secret().to_string())
    }

    /// Resolve a secret by name with async vault support.
    /// This is the preferred method for vault: recipes.
    pub async fn resolve_async(&self, name: &str) -> Option<String> {
        // 1. Check environment variable first
        if let Ok(val) = std::env::var(name) {
            if !val.is_empty() {
                return Some(val);
            }
        }

        // 2. Check recipe store
        if let Some(recipe) = self.recipes.get(name) {
            if recipe.starts_with("vault:") {
                // Handle vault recipes asynchronously
                let client = self.vault_client.lock().await;
                if let Some(ref vault) = *client {
                    if let Some(secret) = resolve_vault_secret(Some(vault), recipe).await {
                        let value = secret.expose_secret().to_string();
                        
                        // Add to redaction set
                        {
                            let mut set = self.redaction_set.write().unwrap();
                            set.insert(name.to_string(), secret);
                            // Rebuild redactor with new value
                            let new_redactor = Redactor::build(&set);
                            *self.redactor.write().unwrap() = new_redactor;
                        }
                        
                        return Some(value);
                    }
                } else {
                    tracing::warn!(recipe = recipe, "vault recipe requested but no vault client available");
                }
                return None;
            } else {
                // Handle non-vault recipes synchronously
                return resolve::execute_recipe(name, recipe)
                    .map(|s| s.expose_secret().to_string());
            }
        }

        None
    }

    /// Redact all known secret values from a string.
    pub fn redact(&self, input: &str) -> String {
        let redactor = self.redactor.read().unwrap();
        redactor.redact(input)
    }

    /// Redact secrets from tool result content blocks.
    pub fn redact_content(&self, content: &mut Vec<omegon_traits::ContentBlock>) {
        let redactor = self.redactor.read().unwrap();
        redactor.redact_content_blocks(content);
    }

    /// Check if a tool call should be guarded (sensitive path access).
    pub fn check_guard(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Option<GuardDecision> {
        let decision = self.path_guard.check(tool_name, args);
        if let Some(ref d) = decision {
            self.audit.log_guard(tool_name, args, d);
        }
        decision
    }

    /// Re-resolve all secrets and rebuild the redaction automaton.
    fn refresh_redaction_set(&mut self) {
        let mut set = self.redaction_set.write().unwrap();
        set.clear();

        // Resolve from recipes
        for (name, recipe) in self.recipes.iter() {
            if let Some(value) = resolve::execute_recipe(name, recipe) {
                set.insert(name.clone(), value);
            }
        }

        // Also grab well-known env vars that might contain secrets
        for env_name in resolve::WELL_KNOWN_SECRET_ENVS {
            if let Ok(value) = std::env::var(env_name) {
                if !value.is_empty()
                    && !set.values().any(|v| v.expose_secret() == value)
                {
                    set.insert(env_name.to_string(), SecretString::from(value));
                }
            }
        }

        let count = set.len();

        // Rebuild the Aho-Corasick automaton
        let new_redactor = Redactor::build(&set);
        *self.redactor.write().unwrap() = new_redactor;

        tracing::info!(count = count, "redaction set refreshed (keyring + aho-corasick)");
    }
}
