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
pub use resolve::{
    delete_from_keyring, execute_recipe_async, is_refreshable_oauth_secret_env, load_from_keyring,
    resolve_secret_async, resolve_vault_secret, store_in_keyring,
};
pub use store::{KeyBackend, SecretStore};
pub use vault::{AuthConfig, VaultClient, VaultConfig};

use secrecy::{ExposeSecret, SecretString};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

/// Why a secret was preflighted/warmed into the session cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretUse {
    LlmProvider,
    WebSearch,
    Update,
    Other,
}

#[derive(Debug, Clone)]
pub struct CachedSecretMeta {
    pub source: &'static str,
    pub warmed: bool,
    pub required_at_startup: bool,
    pub used_by: HashSet<SecretUse>,
}

#[derive(Debug, Clone)]
pub struct SessionSecretDiagnostic {
    pub name: String,
    pub source: &'static str,
    pub warmed: bool,
    pub required_at_startup: bool,
    pub used_by: Vec<SecretUse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRecipeStatus {
    Resolved,
    Missing,
    Deferred,
}

impl SecretRecipeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolved => "resolves",
            Self::Missing => "missing",
            Self::Deferred => "deferred",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SecretRecipeDiagnostic {
    pub name: String,
    pub recipe: String,
    pub status: SecretRecipeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRecipeDescriptor {
    pub name: String,
    pub recipe: String,
    pub kind: String,
    pub payload: String,
}

fn classify_recipe_descriptor(recipe: &str) -> (String, String) {
    recipe
        .split_once(':')
        .map(|(kind, payload)| (kind.to_string(), payload.to_string()))
        .unwrap_or_else(|| ("unknown".to_string(), recipe.to_string()))
}

/// Central secrets manager — owns the redaction set, recipes, guards, and Vault client.
pub struct SecretsManager {
    /// Resolved secret values for redaction (name → SecretString).
    /// Values are zeroized when dropped.
    redaction_set: Arc<RwLock<HashMap<String, SecretString>>>,
    /// Session-scoped cache of resolved secrets used after startup so runtime
    /// tool execution does not trigger surprise Keychain/UI prompts mid-run.
    session_cache: Arc<RwLock<HashMap<String, SecretString>>>,
    session_meta: Arc<RwLock<HashMap<String, CachedSecretMeta>>>,
    /// Pre-compiled Aho-Corasick redactor (rebuilt when secrets change).
    redactor: Arc<RwLock<Redactor>>,
    /// Recipe store (persisted to ~/.omegon/secrets.json)
    recipes: RwLock<RecipeStore>,
    /// Path guard for sensitive file access
    path_guard: PathGuard,
    /// Audit log
    audit: AuditLog,
    /// Vault client (optional, only if vault.json exists or VAULT_ADDR set)
    vault_client: Arc<Mutex<Option<VaultClient>>>,
}

fn hydrate_static_process_env(
    session_cache: &Arc<RwLock<HashMap<String, SecretString>>>,
    redaction_set: &Arc<RwLock<HashMap<String, SecretString>>>,
) {
    let session = session_cache.read().unwrap();
    let redaction = redaction_set.read().unwrap();
    for env_name in resolve::STATIC_SECRET_ENVS {
        if let Some(value) = session.get(*env_name).or_else(|| redaction.get(*env_name)) {
            // SAFETY: Omegon mutates process env only on the main runtime thread
            // during setup or in direct response to operator secret changes.
            // We do not concurrently iterate the environment while doing this.
            unsafe { std::env::set_var(env_name, value.expose_secret()) };
        }
    }
}

impl SecretsManager {
    /// Create a new secrets manager, loading recipes from the config directory.
    pub fn new(config_dir: &std::path::Path) -> anyhow::Result<Self> {
        let recipes = RecipeStore::load(config_dir)?;
        let audit = AuditLog::new(config_dir);
        let path_guard = PathGuard::new();

        let mgr = Self {
            redaction_set: Arc::new(RwLock::new(HashMap::new())),
            redactor: Arc::new(RwLock::new(Redactor::build(&HashMap::new()))),
            session_cache: Arc::new(RwLock::new(HashMap::new())),
            session_meta: Arc::new(RwLock::new(HashMap::new())),
            recipes: RwLock::new(recipes),
            path_guard,
            audit,
            vault_client: Arc::new(Mutex::new(None)),
        };

        // Pre-resolve non-keyring secrets into the redaction set. Keyring-backed
        // secrets are intentionally lazy: probing Keychain during normal startup
        // causes one macOS authorization dialog per stored item after every
        // ad-hoc rebuilt binary.
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
            let token_secret_name = match &config.auth {
                AuthConfig::Token { secret_name } => secret_name.clone(),
                _ => None,
            };

            match VaultClient::new(config) {
                Ok(mut client) => {
                    if let Some(secret_name) = token_secret_name.as_deref() {
                        match self.resolve(secret_name) {
                            Some(token) => {
                                client.set_token(SecretString::from(token));
                                tracing::info!(
                                    secret_name = secret_name,
                                    "loaded vault token from Omegon secret"
                                );
                            }
                            None => {
                                tracing::warn!(
                                    secret_name = secret_name,
                                    "vault token secret could not be resolved"
                                );
                            }
                        }
                    }
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

    /// Warm a specific secret into the session cache. Intended for startup
    /// preflight so any required Keychain/UI interaction happens at a
    /// deterministic boundary, not mid-session.
    pub fn warm_secret(&self, name: &str, use_case: SecretUse, required_at_startup: bool) -> bool {
        let Some(value) = self.resolve(name) else {
            return false;
        };
        let mut cache = self.session_cache.write().unwrap();
        cache.insert(name.to_string(), SecretString::from(value));
        let mut meta = self.session_meta.write().unwrap();
        meta.entry(name.to_string())
            .and_modify(|m| {
                m.warmed = true;
                m.required_at_startup |= required_at_startup;
                m.used_by.insert(use_case);
            })
            .or_insert_with(|| CachedSecretMeta {
                source: "resolved",
                warmed: true,
                required_at_startup,
                used_by: HashSet::from([use_case]),
            });
        // Note: redactor rebuild happens after ALL secrets are warm, not per-secret,
        // to avoid rebuilding expensive Aho-Corasick DFA multiple times during preflight
        true
    }

    /// Async preflight: resolves all names via resolve_async(), which handles
    /// vault: recipes in addition to keyring, shell, file, and env sources.
    ///
    /// Use this in async contexts (AgentSetup::new) — it replaces the sync
    /// preflight_session_cache() when vault is configured. In interactive mode
    /// the behavior is identical to the sync variant; in headless/vault mode
    /// it's the only path that actually resolves vault: recipes.
    pub async fn preflight_session_cache_async<I, S>(&self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let requested: Vec<String> = names.into_iter().map(|n| n.as_ref().to_string()).collect();
        tracing::info!(
            requested = requested.len(),
            names = ?requested,
            "secrets preflight (async) starting"
        );

        let mut warmed = Vec::new();
        let mut missing = Vec::new();

        for name in &requested {
            match self.resolve_async(name).await {
                Some(value) => {
                    let mut cache = self.session_cache.write().unwrap();
                    cache.insert(name.clone(), SecretString::from(value));
                    let use_case = match name.as_str() {
                        "BRAVE_API_KEY" | "TAVILY_API_KEY" | "SERPER_API_KEY"
                        | "FIRECRAWL_API_KEY" => SecretUse::WebSearch,
                        _ => SecretUse::LlmProvider,
                    };
                    let mut meta = self.session_meta.write().unwrap();
                    meta.entry(name.clone())
                        .and_modify(|m| {
                            m.warmed = true;
                            m.required_at_startup = true;
                            m.used_by.insert(use_case);
                        })
                        .or_insert_with(|| CachedSecretMeta {
                            source: "resolved",
                            warmed: true,
                            required_at_startup: true,
                            used_by: HashSet::from([use_case]),
                        });
                    warmed.push(name.clone());
                }
                None => missing.push(name.clone()),
            }
        }

        self.rebuild_redactor();
        tracing::info!(
            requested = requested.len(),
            warmed = warmed.len(),
            missing = missing.len(),
            warmed_names = ?warmed,
            missing_names = ?missing,
            "secrets preflight (async) finished"
        );
        self.hydrate_process_env();
    }

    /// Startup preflight: warm known interactive/runtime secrets once so the
    /// rest of the session can read them headlessly from memory/env.
    ///
    /// Batches all keyring lookups into a single prompt by pre-loading all
    /// recipes that need keyring access before resolving any of them.
    pub fn preflight_session_cache<I, S>(&self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let requested: Vec<String> = names.into_iter().map(|n| n.as_ref().to_string()).collect();
        tracing::info!(
            requested = requested.len(),
            names = ?requested,
            "secrets preflight starting"
        );

        // Pre-load all recipes so we know which ones require keyring access
        // and can batch them together in a single prompt.
        // SECURITY: Keyring is the authoritative source. Environment variables
        // are only used if no keyring recipe is configured.
        let recipes = self.recipes.read().unwrap();
        let mut keyring_names: Vec<&String> = Vec::new();
        let mut env_fallback_names: Vec<&String> = Vec::new();

        for name in &requested {
            // Check if it has a recipe first (keyring is authoritative)
            if recipes.get(name).is_some() {
                keyring_names.push(name);
            } else if std::env::var(name).is_ok() {
                // Only use env if no recipe is configured
                env_fallback_names.push(name);
            }
        }
        drop(recipes);

        // Resolve secrets in order: all keyring together (single prompt), then env.
        // This batches all Keychain access into a single prompt on macOS.
        let mut warmed = Vec::new();
        let mut missing = Vec::new();

        // Resolve keyring vars all at once (single prompt on macOS)
        // by triggering all keyring lookups in sequence before building the cache
        for name in &keyring_names {
            let use_case = match name.as_str() {
                "BRAVE_API_KEY" | "TAVILY_API_KEY" | "SERPER_API_KEY" | "FIRECRAWL_API_KEY" => {
                    SecretUse::WebSearch
                }
                _ => SecretUse::LlmProvider,
            };
            if self.warm_secret(name, use_case, true) {
                warmed.push((*name).clone());
            } else {
                missing.push((*name).clone());
            }
        }

        // Resolve env vars only if no keyring recipe exists (fallback only)
        for name in env_fallback_names {
            let use_case = match name.as_str() {
                "BRAVE_API_KEY" | "TAVILY_API_KEY" | "SERPER_API_KEY" | "FIRECRAWL_API_KEY" => {
                    SecretUse::WebSearch
                }
                _ => SecretUse::LlmProvider,
            };
            if self.warm_secret(name, use_case, true) {
                warmed.push(name.clone());
            } else {
                missing.push(name.clone());
            }
        }

        // Rebuild redactor once after all secrets are warmed, not per-secret.
        // This avoids rebuilding the expensive Aho-Corasick DFA multiple times
        // and reduces lock contention during the Keychain prompt window.
        self.rebuild_redactor();

        tracing::info!(
            requested = requested.len(),
            warmed = warmed.len(),
            missing = missing.len(),
            warmed_names = ?warmed,
            missing_names = ?missing,
            "secrets preflight finished"
        );
        self.hydrate_process_env();
    }

    /// Export resolved session secrets as environment-variable pairs for child
    /// processes. Intended for headless/cleave children so they inherit the
    /// startup-approved secret set instead of touching keychain/UI mid-run.
    pub fn session_env(&self) -> Vec<(String, String)> {
        let exported: Vec<(String, String)> = self
            .session_cache
            .read()
            .unwrap()
            .iter()
            .map(|(name, value)| (name.clone(), value.expose_secret().to_string()))
            .collect();
        tracing::debug!(
            exported = exported.len(),
            names = ?exported.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            "exporting session secret env pairs"
        );
        exported
    }

    /// Name-only diagnostics for the current startup/session secret state.
    pub fn session_diagnostics(&self) -> Vec<SessionSecretDiagnostic> {
        let meta = self.session_meta.read().unwrap();
        let mut diagnostics: Vec<_> = meta
            .iter()
            .map(|(name, meta)| {
                let mut used_by: Vec<_> = meta.used_by.iter().copied().collect();
                used_by.sort_by_key(|u| match u {
                    SecretUse::LlmProvider => 0,
                    SecretUse::WebSearch => 1,
                    SecretUse::Update => 2,
                    SecretUse::Other => 3,
                });
                SessionSecretDiagnostic {
                    name: name.clone(),
                    source: meta.source,
                    warmed: meta.warmed,
                    required_at_startup: meta.required_at_startup,
                    used_by,
                }
            })
            .collect();
        diagnostics.sort_by(|a, b| a.name.cmp(&b.name));
        diagnostics
    }

    /// Return a secret only when it is already resident in the process.
    ///
    /// This is the side-effect-free lookup for startup/status surfaces: it never
    /// executes a recipe and therefore cannot display Keychain UI or perform
    /// network/file/shell I/O.
    pub fn resolve_cached(&self, name: &str) -> Option<String> {
        if let Some(cached) = self.session_cache.read().unwrap().get(name) {
            return Some(cached.expose_secret().to_string());
        }
        self.redaction_set
            .read()
            .unwrap()
            .get(name)
            .map(|cached| cached.expose_secret().to_string())
    }

    /// Resolve a secret by name. Checks in-memory caches first, then falls back
    /// to recipe resolution. Call only at an explicit operation boundary: a
    /// cache miss may execute Keychain, file, shell, or environment I/O.
    pub fn resolve(&self, name: &str) -> Option<String> {
        // Session cache first — deterministic runtime path after startup preflight.
        {
            let set = self.session_cache.read().unwrap();
            if let Some(cached) = set.get(name) {
                return Some(cached.expose_secret().to_string());
            }
        }
        // Check redaction cache first — the value is already in memory
        // for output redaction purposes. Reading it here avoids a second
        // keyring prompt on macOS.
        {
            let set = self.redaction_set.read().unwrap();
            if let Some(cached) = set.get(name) {
                return Some(cached.expose_secret().to_string());
            }
        }
        // Cache miss — clone recipe out of the lock so we don't hold it
        // across keyring::get_password() which blocks on macOS Keychain UI
        let recipe = {
            let recipes = self.recipes.read().unwrap();
            recipes.get(name).cloned()
        };
        let recipe = recipe?;
        let resolved = resolve::execute_recipe(name, &recipe)?;
        let value = resolved.expose_secret().to_string();
        {
            let mut set = self.redaction_set.write().unwrap();
            set.insert(name.to_string(), resolved);
            let new_redactor = Redactor::build(&set);
            *self.redactor.write().unwrap() = new_redactor;
        }
        Some(value)
    }

    /// Resolve a secret by name with async vault support.
    /// This is the preferred method for vault: recipes.
    pub async fn resolve_async(&self, name: &str) -> Option<String> {
        // Check redaction cache first (same as sync path)
        {
            let set = self.redaction_set.read().unwrap();
            if let Some(cached) = set.get(name) {
                return Some(cached.expose_secret().to_string());
            }
        }

        // Clone recipe out — don't hold across I/O
        // Check recipe FIRST (keyring is authoritative)
        let recipe = {
            let recipes = self.recipes.read().unwrap();
            recipes.get(name).cloned()
        };

        if let Some(recipe) = recipe {
            // Recipe exists — resolve it (may be keyring, vault, or shell)
            // Acquire vault client only when we actually need it for recipe execution
            let client = self.vault_client.lock().await;
            let vault_client = client.as_ref();

            if let Some(secret) = resolve::execute_recipe_async(name, &recipe, vault_client).await {
                let value = secret.expose_secret().to_string();
                let mut set = self.redaction_set.write().unwrap();
                set.insert(name.to_string(), secret);
                let new_redactor = Redactor::build(&set);
                *self.redactor.write().unwrap() = new_redactor;
                return Some(value);
            }
        }

        // No recipe — fall back to environment variable
        if let Ok(val) = std::env::var(name)
            && !val.is_empty()
        {
            let secret = SecretString::from(val);
            let value = secret.expose_secret().to_string();
            let mut set = self.redaction_set.write().unwrap();
            set.insert(name.to_string(), secret);
            let new_redactor = Redactor::build(&set);
            *self.redactor.write().unwrap() = new_redactor;
            return Some(value);
        }

        None
    }

    /// Redact all known secret values from a string.
    pub fn redact(&self, input: &str) -> String {
        let redactor = self.redactor.read().unwrap();
        redactor.redact(input)
    }

    /// Redact secrets from tool result content blocks.
    /// Only available with the `agent` feature (requires omegon-traits).
    #[cfg(feature = "agent")]
    pub fn redact_content(&self, content: &mut [omegon_traits::ContentBlock]) {
        let redactor = self.redactor.read().unwrap();
        redactor.redact_content_blocks(content);
    }

    /// Redact secrets in a single string in place. This is the composable
    /// primitive — works with any container. Call it per-string from your
    /// own iteration logic.
    pub fn redact_in_place(&self, text: &mut String) {
        let redactor = self.redactor.read().unwrap();
        redactor.redact_in_place(text);
    }

    /// Redact secrets across a slice of strings in place.
    pub fn redact_strings(&self, texts: &mut [String]) {
        let redactor = self.redactor.read().unwrap();
        redactor.redact_strings(texts);
    }

    /// Add a runtime-discovered secret value to the redaction set without
    /// creating or changing a persisted recipe. Used for projected credentials
    /// such as provider auth.json entries.
    pub fn register_redaction_secret(&self, name: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        self.redaction_set
            .write()
            .unwrap()
            .insert(name.to_string(), SecretString::from(value.to_string()));
        self.rebuild_redactor();
    }

    /// Check if a tool call should be guarded (sensitive path access).
    pub fn check_guard(&self, tool_name: &str, args: &serde_json::Value) -> Option<GuardDecision> {
        let decision = self.path_guard.check(tool_name, args);
        if let Some(ref d) = decision {
            self.audit.log_guard(tool_name, args, d);
        }
        decision
    }

    /// List all configured secret recipes with their resolution hints.
    pub fn list_recipes(&self) -> Vec<(String, String)> {
        self.recipes
            .read()
            .unwrap()
            .iter()
            .map(|(name, recipe)| (name.clone(), recipe.as_string()))
            .collect()
    }

    /// List configured secret recipes without resolving them.
    ///
    /// This is the safe status path for ACP/settings panels: it exposes recipe
    /// indirection metadata, not resolved secret values, and it never touches
    /// keyring, Vault, files, or command recipes.
    pub fn list_recipe_descriptors(&self) -> Vec<SecretRecipeDescriptor> {
        let mut descriptors: Vec<_> = self
            .recipes
            .read()
            .unwrap()
            .iter()
            .map(|(name, recipe)| {
                let recipe_text = recipe.as_string();
                let (kind, payload) = classify_recipe_descriptor(&recipe_text);
                SecretRecipeDescriptor {
                    name: name.clone(),
                    recipe: recipe_text,
                    kind,
                    payload,
                }
            })
            .collect();
        descriptors.sort_by(|a, b| a.name.cmp(&b.name));
        descriptors
    }

    /// List configured secret recipes with bounded diagnostics.
    ///
    /// This checks only declared recipes, never the whole keychain. Vault recipes
    /// are marked deferred because they require async resolution and may depend
    /// on external service availability.
    pub fn list_recipe_diagnostics(&self) -> Vec<SecretRecipeDiagnostic> {
        let recipes: Vec<(String, crate::recipes::Recipe)> = self
            .recipes
            .read()
            .unwrap()
            .iter()
            .map(|(name, recipe)| (name.clone(), recipe.clone()))
            .collect();

        let mut diagnostics: Vec<_> = recipes
            .into_iter()
            .map(|(name, recipe)| {
                let recipe_text = recipe.as_string();
                let status = if recipe.is_vault() {
                    SecretRecipeStatus::Deferred
                } else if resolve::execute_recipe(&name, &recipe).is_some() {
                    SecretRecipeStatus::Resolved
                } else {
                    SecretRecipeStatus::Missing
                };
                SecretRecipeDiagnostic {
                    name,
                    recipe: recipe_text,
                    status,
                }
            })
            .collect();
        diagnostics.sort_by(|a, b| a.name.cmp(&b.name));
        diagnostics
    }

    /// Resolve well-known provider secrets into process environment variables
    /// so legacy env-based integrations (web search, provider clients) can use
    /// secrets stored in Omegon's keyring/recipe system.
    /// Rebuild the redactor from the current redaction_set.
    /// Called after batch-warming secrets to avoid rebuilding the Aho-Corasick DFA
    /// multiple times during preflight (which can interfere with Keychain prompts).
    fn rebuild_redactor(&self) {
        let set = self.redaction_set.read().unwrap();
        let new_redactor = Redactor::build(&set);
        let mut redactor = self.redactor.write().unwrap();
        *redactor = new_redactor;
    }

    pub fn hydrate_process_env(&self) {
        hydrate_static_process_env(&self.session_cache, &self.redaction_set);
    }

    /// Set a secret recipe (e.g. "env:MY_VAR", "cmd:pass show x", "vault:path").
    ///
    /// Setting recipe metadata must not resolve the recipe: settings panels can
    /// create `cmd:`/`file:`/Vault recipes, and mutation must not execute a
    /// command, read a file, prompt keyring, or contact Vault as a side effect.
    /// Values are resolved at explicit preflight/use boundaries instead.
    pub fn set_recipe(&self, name: &str, recipe_str: &str) -> anyhow::Result<()> {
        self.recipes
            .write()
            .unwrap()
            .set_string(name.to_string(), recipe_str.to_string())?;
        self.evict_secrets(&[name]);
        Ok(())
    }

    /// Repair a named well-known keyring entry after a failed write or explicit
    /// secret mutation. This is deliberately not run as a startup scan: on
    /// macOS, each attempted Keychain read can produce its own authorization
    /// dialog for ad-hoc rebuilt binaries.
    fn repair_well_known_keyring_recipe(&self, name: &str) -> bool {
        if !resolve::STATIC_SECRET_ENVS.contains(&name) {
            return false;
        }
        let has_recipe = self.recipes.read().unwrap().get(name).is_some();
        if has_recipe {
            return false;
        }
        let Ok(Some(secret)) = load_from_keyring(name) else {
            return false;
        };
        if self
            .recipes
            .write()
            .unwrap()
            .set_string(name.to_string(), format!("keyring:{name}"))
            .is_err()
        {
            return false;
        }
        self.redaction_set
            .write()
            .unwrap()
            .insert(name.to_string(), secret.clone());
        self.session_cache
            .write()
            .unwrap()
            .insert(name.to_string(), secret);
        let use_case = match name {
            "BRAVE_API_KEY" | "TAVILY_API_KEY" | "SERPER_API_KEY" | "FIRECRAWL_API_KEY" => {
                SecretUse::WebSearch
            }
            _ => SecretUse::Other,
        };
        self.session_meta.write().unwrap().insert(
            name.to_string(),
            CachedSecretMeta {
                source: "keyring-repaired",
                warmed: true,
                required_at_startup: false,
                used_by: HashSet::from([use_case]),
            },
        );
        tracing::info!(name = name, "repaired orphaned well-known keyring secret");
        true
    }

    /// Store a raw value in the OS keyring and create a keyring: recipe for it.
    pub fn set_keyring_secret(&self, name: &str, value: &str) -> anyhow::Result<()> {
        // Upsert in keyring first. If the platform refuses to update an existing
        // item but readback succeeds, repair metadata/cache using the existing
        // secure value instead of leaving the harness unable to see the secret.
        let secret = match store_in_keyring(name, value) {
            Ok(()) => SecretString::from(value.to_string()),
            Err(write_error) => match load_from_keyring(name)? {
                Some(existing) => {
                    tracing::warn!(
                        name = name,
                        error = %write_error,
                        "keyring write failed but existing item resolved; repairing secret metadata"
                    );
                    existing
                }
                None => return Err(write_error),
            },
        };
        self.repair_well_known_keyring_recipe(name);
        self.recipes
            .write()
            .unwrap()
            .set_string(name.to_string(), format!("keyring:{name}"))?;

        self.redaction_set
            .write()
            .unwrap()
            .insert(name.to_string(), secret.clone());
        self.session_cache
            .write()
            .unwrap()
            .insert(name.to_string(), secret);
        self.session_meta
            .write()
            .unwrap()
            .entry(name.to_string())
            .and_modify(|m| {
                m.warmed = true;
                m.used_by.insert(SecretUse::Other);
            })
            .or_insert_with(|| CachedSecretMeta {
                source: "keyring",
                warmed: true,
                required_at_startup: false,
                used_by: HashSet::from([SecretUse::Other]),
            });
        self.rebuild_redactor();
        self.hydrate_process_env();
        Ok(())
    }

    /// Evict one or more secrets from the session cache, redaction set, and
    /// process environment. Does NOT touch recipes or keyring — this is a
    /// runtime-only purge used by `/logout` so stale provider credentials
    /// cannot resurface via `hydrate_process_env()`.
    pub fn evict_secrets(&self, names: &[&str]) {
        {
            let mut cache = self.session_cache.write().unwrap();
            let mut redaction = self.redaction_set.write().unwrap();
            let mut meta = self.session_meta.write().unwrap();
            for name in names {
                cache.remove(*name);
                redaction.remove(*name);
                meta.remove(*name);
                // SAFETY: logout is an explicit operator action; clearing the env
                // here prevents hydrate_process_env() from re-injecting stale values.
                unsafe { std::env::remove_var(name) };
            }
        }
        // Rebuild the Aho-Corasick automaton without the evicted secrets.
        // All three write locks are released first to minimise contention.
        self.rebuild_redactor();
        tracing::info!(evicted = ?names, "evicted secrets from session cache");
    }

    /// Delete a secret recipe and any same-name keyring entry.
    ///
    /// Deletion is idempotent across the secrets surface: absent recipes and
    /// absent keychain entries are success states. The same-name keyring cleanup
    /// handles repaired/orphaned harness-managed secrets without keychain scans.
    pub fn delete_recipe(&self, name: &str) -> anyhow::Result<()> {
        delete_from_keyring(name)?;
        self.recipes.write().unwrap().remove(name).map(|_| ())?;
        self.evict_secrets(&[name]);
        self.refresh_redaction_set();
        if resolve::STATIC_SECRET_ENVS.contains(&name)
            || resolve::REFRESHABLE_OAUTH_SECRET_ENVS.contains(&name)
        {
            // SAFETY: same reasoning as hydrate_process_env().
            unsafe { std::env::remove_var(name) };
        }
        Ok(())
    }

    /// Re-resolve all secrets and rebuild the redaction automaton.
    fn refresh_redaction_set(&self) {
        let mut set = self.redaction_set.write().unwrap();
        set.clear();

        // Resolve from recipes (sync only - vault recipes will be skipped here)
        // IMPORTANT: Skip keyring: recipes at startup to avoid unexpected Keychain prompts.
        // Keyring values will be resolved on-demand when actually needed (lazy resolution).
        for (name, recipe) in self.recipes.read().unwrap().iter() {
            match recipe {
                crate::recipes::Recipe::String(recipe_str)
                    if recipe_str.starts_with("keyring:") =>
                {
                    // Skip keyring recipes at startup — will resolve on-demand
                    tracing::debug!(
                        name = name,
                        "skipping keyring recipe at startup (will resolve on-demand)"
                    );
                    continue;
                }
                _ => {
                    if let Some(value) = resolve::execute_recipe(name, recipe) {
                        set.insert(name.clone(), value);
                    }
                }
            }
        }

        // Also grab well-known env vars that might contain secrets,
        // but ONLY if they don't already have a recipe-resolved value.
        // Recipe values are authoritative — env is fallback only.
        for env_name in resolve::WELL_KNOWN_SECRET_ENVS {
            if set.contains_key(*env_name) {
                // Already resolved from recipe — skip env override
                continue;
            }
            if let Ok(value) = std::env::var(env_name)
                && !value.is_empty()
                && !set.values().any(|v| v.expose_secret() == value)
            {
                set.insert(env_name.to_string(), SecretString::from(value));
            }
        }

        let count = set.len();

        // Rebuild the Aho-Corasick automaton
        let new_redactor = Redactor::build(&set);
        *self.redactor.write().unwrap() = new_redactor;

        tracing::info!(
            count = count,
            "redaction set refreshed (keyring + aho-corasick) - vault recipes require async refresh"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn runtime_projected_secret_is_redacted_without_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();
        mgr.register_redaction_secret("CHATGPT_OAUTH_TOKEN", "projected-oauth-token");

        let redacted = mgr.redact("Authorization: Bearer projected-oauth-token");
        assert_eq!(
            redacted,
            "Authorization: Bearer [REDACTED:CHATGPT_OAUTH_TOKEN]"
        );
    }

    #[tokio::test]
    async fn init_vault_uses_configured_token_secret_name() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = SecretsManager::new(dir.path()).unwrap();
        secrets
            .set_keyring_secret("VAULT_ROOT_TOKEN", "hvs.test-root")
            .unwrap();
        std::fs::write(
            dir.path().join("vault.json"),
            r#"{
                "addr": "http://127.0.0.1:8200",
                "auth": {
                    "method": "token",
                    "secret_name": "VAULT_ROOT_TOKEN"
                },
                "allowed_paths": ["secret/data/omegon/*"]
            }"#,
        )
        .unwrap();

        secrets.init_vault(dir.path()).await.unwrap();
        let client = secrets.vault_client().await;
        assert!(client.is_some());
        assert!(client.as_ref().unwrap().is_authenticated());
    }

    #[test]
    fn keyring_secret_set_repairs_missing_recipe_and_is_idempotent() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();

        // Simulate the split-brain state: secure storage has a value, but the
        // recipe store has no declaration for the secret.
        store_in_keyring("BRAVE_API_KEY", "old-value").unwrap();
        assert!(mgr.list_recipes().is_empty());
        assert!(mgr.resolve("BRAVE_API_KEY").is_none());

        mgr.set_keyring_secret("BRAVE_API_KEY", "new-value")
            .unwrap();
        assert_eq!(
            mgr.list_recipes(),
            vec![(
                "BRAVE_API_KEY".to_string(),
                "keyring:BRAVE_API_KEY".to_string()
            )]
        );
        assert_eq!(mgr.resolve("BRAVE_API_KEY").as_deref(), Some("new-value"));
        assert_eq!(
            std::env::var("BRAVE_API_KEY").ok().as_deref(),
            Some("new-value")
        );
        assert_eq!(
            mgr.redact("Authorization: Bearer new-value"),
            "Authorization: Bearer [REDACTED:BRAVE_API_KEY]"
        );

        mgr.set_keyring_secret("BRAVE_API_KEY", "newer-value")
            .unwrap();
        assert_eq!(mgr.resolve("BRAVE_API_KEY").as_deref(), Some("newer-value"));

        // SAFETY: cleanup for isolated test env vars.
        unsafe { std::env::remove_var("BRAVE_API_KEY") };
    }

    #[test]
    fn explicit_repair_restores_orphaned_firecrawl_keyring_secret() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        store_in_keyring("FIRECRAWL_API_KEY", "firecrawl-value").unwrap();

        let mgr = SecretsManager::new(dir.path()).unwrap();
        assert!(mgr.resolve("FIRECRAWL_API_KEY").is_none());

        assert!(mgr.repair_well_known_keyring_recipe("FIRECRAWL_API_KEY"));

        assert_eq!(
            mgr.resolve("FIRECRAWL_API_KEY").as_deref(),
            Some("firecrawl-value")
        );
        assert!(mgr.list_recipes().contains(&(
            "FIRECRAWL_API_KEY".to_string(),
            "keyring:FIRECRAWL_API_KEY".to_string()
        )));
        assert!(mgr.session_diagnostics().iter().any(|diag| {
            diag.name == "FIRECRAWL_API_KEY" && diag.used_by.contains(&SecretUse::WebSearch)
        }));

        mgr.delete_recipe("FIRECRAWL_API_KEY").unwrap();
    }

    #[test]
    fn delete_recipe_is_idempotent_and_clears_same_name_keyring_secret() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();

        mgr.set_keyring_secret("BRAVE_API_KEY", "delete-me")
            .unwrap();
        assert_eq!(mgr.resolve("BRAVE_API_KEY").as_deref(), Some("delete-me"));

        mgr.delete_recipe("BRAVE_API_KEY").unwrap();
        mgr.delete_recipe("BRAVE_API_KEY").unwrap();

        assert!(mgr.resolve("BRAVE_API_KEY").is_none());
        assert!(mgr.list_recipes().is_empty());
        assert!(std::env::var("BRAVE_API_KEY").is_err());
        assert_eq!(mgr.redact("delete-me"), "delete-me");
    }

    #[test]

    fn set_recipe_does_not_resolve_side_effectful_recipes() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();
        let marker = dir.path().join("set-recipe-cmd-ran");

        mgr.set_recipe("CMD_SECRET", &format!("cmd:touch {}", marker.display()))
            .unwrap();

        assert!(
            !marker.exists(),
            "setting a recipe must not execute cmd recipes"
        );
        let descriptors = mgr.list_recipe_descriptors();
        assert!(descriptors.iter().any(|entry| {
            entry.name == "CMD_SECRET"
                && entry.kind == "cmd"
                && entry.payload == format!("touch {}", marker.display())
        }));
    }

    #[test]
    fn list_recipe_descriptors_do_not_resolve_recipes() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();
        let marker = dir.path().join("cmd-ran");

        mgr.set_recipe("CMD_SECRET", &format!("cmd:touch {}", marker.display()))
            .unwrap();
        mgr.set_recipe("VAULT_SECRET", "vault:secret/data/omegon/api#token")
            .unwrap();
        if marker.exists() {
            std::fs::remove_file(&marker).unwrap();
        }

        let descriptors = mgr.list_recipe_descriptors();
        assert!(descriptors.iter().any(|entry| {
            entry.name == "CMD_SECRET"
                && entry.kind == "cmd"
                && entry.payload == format!("touch {}", marker.display())
        }));
        assert!(descriptors.iter().any(|entry| {
            entry.name == "VAULT_SECRET"
                && entry.kind == "vault"
                && entry.payload == "secret/data/omegon/api#token"
        }));
        assert!(
            !marker.exists(),
            "describing recipes must not execute cmd recipes"
        );
    }

    #[test]

    fn list_recipe_diagnostics_reports_bounded_status() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();

        mgr.set_keyring_secret("BRAVE_API_KEY", "diagnostic-value")
            .unwrap();
        mgr.set_recipe("TAVILY_API_KEY", "env:OMEGON_TEST_MISSING_TAVILY")
            .unwrap();

        let diagnostics = mgr.list_recipe_diagnostics();
        assert!(diagnostics.iter().any(|entry| {
            entry.name == "BRAVE_API_KEY"
                && entry.recipe == "keyring:BRAVE_API_KEY"
                && entry.status == SecretRecipeStatus::Resolved
        }));
        assert!(diagnostics.iter().any(|entry| {
            entry.name == "TAVILY_API_KEY"
                && entry.recipe == "env:OMEGON_TEST_MISSING_TAVILY"
                && entry.status == SecretRecipeStatus::Missing
        }));

        // SAFETY: cleanup for isolated test env vars.
        unsafe { std::env::remove_var("BRAVE_API_KEY") };
    }

    #[test]
    fn hydrate_process_env_populates_well_known_recipe_secret() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test process controls these env vars and does not iterate env concurrently.
        unsafe {
            std::env::remove_var("BRAVE_API_KEY");
            std::env::set_var("OMEGON_TEST_BRAVE_KEY", "brave-test-key");
        }
        let mgr = SecretsManager::new(dir.path()).unwrap();
        mgr.set_recipe("BRAVE_API_KEY", "env:OMEGON_TEST_BRAVE_KEY")
            .unwrap();
        mgr.preflight_session_cache(["BRAVE_API_KEY"]);
        assert_eq!(
            std::env::var("BRAVE_API_KEY").ok().as_deref(),
            Some("brave-test-key")
        );
        // SAFETY: cleanup for isolated test env vars.
        unsafe {
            std::env::remove_var("OMEGON_TEST_BRAVE_KEY");
            std::env::remove_var("BRAVE_API_KEY");
        }
    }

    #[test]
    fn hydrate_process_env_does_not_populate_refreshable_oauth_secret() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test process controls these env vars and does not iterate env concurrently.
        unsafe {
            std::env::remove_var("CHATGPT_OAUTH_TOKEN");
            std::env::set_var("OMEGON_TEST_CHATGPT_TOKEN", "oauth-test-token");
        }
        let mgr = SecretsManager::new(dir.path()).unwrap();
        mgr.set_recipe("CHATGPT_OAUTH_TOKEN", "env:OMEGON_TEST_CHATGPT_TOKEN")
            .unwrap();
        mgr.preflight_session_cache(["CHATGPT_OAUTH_TOKEN"]);
        assert!(
            std::env::var("CHATGPT_OAUTH_TOKEN").is_err(),
            "refreshable OAuth tokens must not be auto-hydrated into parent env"
        );
        assert!(
            mgr.session_env()
                .iter()
                .any(|(name, value)| name == "CHATGPT_OAUTH_TOKEN" && value == "oauth-test-token"),
            "refreshable OAuth token should remain available for child/delegate inheritance"
        );
        assert_eq!(
            mgr.redact("Authorization: Bearer oauth-test-token"),
            "Authorization: Bearer [REDACTED:CHATGPT_OAUTH_TOKEN]"
        );
        // SAFETY: cleanup for isolated test env vars.
        unsafe {
            std::env::remove_var("OMEGON_TEST_CHATGPT_TOKEN");
            std::env::remove_var("CHATGPT_OAUTH_TOKEN");
        }
    }

    #[test]
    fn delete_recipe_removes_well_known_env_var() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let mgr = SecretsManager::new(dir.path()).unwrap();
        // SAFETY: test process controls this env var and does not iterate env concurrently.
        unsafe { std::env::set_var("BRAVE_API_KEY", "present") };
        mgr.delete_recipe("BRAVE_API_KEY").unwrap();
        assert!(std::env::var("BRAVE_API_KEY").is_err());
    }

    #[test]
    fn evict_secrets_removes_cached_value_and_env_var() {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test process controls these env vars and does not iterate env concurrently.
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::set_var("OMEGON_TEST_ANTH_KEY", "stale-api-key");
        }
        let mgr = SecretsManager::new(dir.path()).unwrap();
        mgr.set_recipe("ANTHROPIC_API_KEY", "env:OMEGON_TEST_ANTH_KEY")
            .unwrap();
        mgr.preflight_session_cache(["ANTHROPIC_API_KEY"]);
        // Verify the secret was hydrated into the process env.
        assert_eq!(
            std::env::var("ANTHROPIC_API_KEY").ok().as_deref(),
            Some("stale-api-key")
        );
        // Evict — simulates what /logout does.
        mgr.evict_secrets(&["ANTHROPIC_API_KEY"]);
        // Env var must be gone.
        assert!(
            std::env::var("ANTHROPIC_API_KEY").is_err(),
            "env var should be cleared after evict"
        );
        // Session cache must be empty for this name.
        assert!(
            mgr.session_env()
                .iter()
                .all(|(name, _)| name != "ANTHROPIC_API_KEY"),
            "session cache should not contain evicted secret"
        );
        // hydrate_process_env must NOT re-inject the stale value.
        mgr.hydrate_process_env();
        assert!(
            std::env::var("ANTHROPIC_API_KEY").is_err(),
            "hydrate_process_env must not resurrect evicted secret"
        );
        // SAFETY: cleanup.
        unsafe {
            std::env::remove_var("OMEGON_TEST_ANTH_KEY");
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
    }
}
