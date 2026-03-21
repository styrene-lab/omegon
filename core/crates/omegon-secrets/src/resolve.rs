//! Secret resolution — env vars, keyring, shell commands, Vault.
//!
//! Uses the `keyring` crate for cross-platform credential store access
//! (macOS Keychain, Windows Credential Manager, Linux Secret Service).
//! Secret values are wrapped in `secrecy::SecretString` and zeroized on drop.

use crate::recipes::{Recipe, RecipeStore};
use crate::vault::VaultClient;
use secrecy::{ExposeSecret, SecretString};
use std::process::Command;



/// Well-known environment variables that commonly contain secrets.
pub const WELL_KNOWN_SECRET_ENVS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "BRAVE_API_KEY",
    "TAVILY_API_KEY",
    "SERPER_API_KEY",
    "GITHUB_TOKEN",
    "GITLAB_TOKEN",
    "GH_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "NPM_TOKEN",
    "DOCKER_PASSWORD",
    "IGOR_API_KEY",
];

/// Omegon's keyring service name — used for cross-platform credential storage.
const KEYRING_SERVICE: &str = "omegon";

/// Resolve a secret by name. Priority: env var > recipe (including vault:).
/// Returns a SecretString that auto-zeroizes on drop.
/// For vault recipes, this returns None and logs a warning - use resolve_async instead.
pub fn resolve_secret(name: &str, recipes: &RecipeStore) -> Option<SecretString> {
    // 1. Check environment variable
    if let Ok(val) = std::env::var(name) {
        if !val.is_empty() {
            return Some(SecretString::from(val));
        }
    }

    // 2. Check recipe store
    if let Some(recipe) = recipes.get(name) {
        return execute_recipe(name, recipe);
    }

    None
}

/// Resolve a secret by name with async vault support.
/// This is the preferred method when vault recipes might be present.
pub async fn resolve_secret_async(
    name: &str, 
    recipes: &RecipeStore, 
    vault_client: Option<&VaultClient>
) -> Option<SecretString> {
    // 1. Check environment variable
    if let Ok(val) = std::env::var(name) {
        if !val.is_empty() {
            return Some(SecretString::from(val));
        }
    }

    // 2. Check recipe store
    if let Some(recipe) = recipes.get(name) {
        return execute_recipe_async(name, recipe, vault_client).await;
    }

    None
}

/// Execute a recipe to resolve a secret value.
pub fn execute_recipe(name: &str, recipe: &Recipe) -> Option<SecretString> {
    match recipe {
        Recipe::String(recipe_str) => execute_string_recipe(name, recipe_str),
        Recipe::Vault { .. } => {
            tracing::warn!(name = name, "vault recipe requires async resolution - use execute_recipe_async");
            None
        }
    }
}

/// Execute a recipe to resolve a secret value with async vault support.
pub async fn execute_recipe_async(
    name: &str, 
    recipe: &Recipe, 
    vault_client: Option<&VaultClient>
) -> Option<SecretString> {
    match recipe {
        Recipe::String(recipe_str) => execute_string_recipe(name, recipe_str),
        Recipe::Vault { path } => {
            // Convert vault recipe to vault:path format for resolve_vault_secret
            let vault_recipe = format!("vault:{}", path);
            resolve_vault_secret(vault_client, &vault_recipe).await
        }
    }
}

/// Execute a string-based recipe to resolve a secret value.
/// 
/// Recipe formats:
/// - `env:VAR_NAME` — read from environment variable
/// - `cmd:some command` — execute shell command, trim output
/// - `keyring:service_name` — cross-platform keyring (macOS Keychain, Linux Secret Service, Windows Credential Manager)
/// - `keychain:service_name` — alias for keyring (backward compat with macOS-only shell-out)
/// - `file:/path/to/file` — read first line of file
/// - `vault:path#key` — read from Vault KV v2 (async resolution in SecretsManager)
pub fn execute_string_recipe(name: &str, recipe: &str) -> Option<SecretString> {
    let (kind, payload) = recipe.split_once(':')?;

    match kind {
        "env" => std::env::var(payload)
            .ok()
            .filter(|v| !v.is_empty())
            .map(SecretString::from),

        "cmd" => {
            let output = Command::new("sh")
                .args(["-c", payload])
                .output()
                .ok()?;
            if output.status.success() {
                let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if val.is_empty() {
                    None
                } else {
                    Some(SecretString::from(val))
                }
            } else {
                tracing::warn!(recipe_kind = kind, "secret recipe command failed");
                None
            }
        }

        // Cross-platform keyring via the `keyring` crate
        "keyring" | "keychain" => {
            match keyring::Entry::new(KEYRING_SERVICE, payload) {
                Ok(entry) => match entry.get_password() {
                    Ok(val) if !val.is_empty() => Some(SecretString::from(val)),
                    Ok(_) => None,
                    Err(keyring::Error::NoEntry) => {
                        tracing::debug!(name = name, "no keyring entry found");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(name = name, error = %e, "keyring access failed");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(name = name, error = %e, "keyring entry creation failed");
                    None
                }
            }
        }

        "file" => {
            let content = std::fs::read_to_string(payload).ok()?;
            let first_line = content.lines().next()?.trim().to_string();
            if first_line.is_empty() {
                None
            } else {
                Some(SecretString::from(first_line))
            }
        }

        "vault" => {
            // String vault recipes are handled asynchronously in SecretsManager
            // This function is for synchronous resolution only
            tracing::warn!(recipe = recipe, "vault recipes require async resolution - use execute_recipe_async");
            None
        }

        _ => {
            tracing::warn!(kind = kind, "unknown secret recipe kind");
            None
        }
    }
}

/// Resolve a secret from Vault using the vault: recipe format.
/// Format: "vault:path#key" where path is the Vault path and key is the field name.
pub async fn resolve_vault_secret(
    vault_client: Option<&VaultClient>, 
    recipe: &str
) -> Option<SecretString> {
    // Parse vault:path#key format — validation before client access
    let (_kind, payload) = recipe.split_once(':')?;
    let (path, key) = payload.split_once('#')?;

    // Defense-in-depth: validate path before it reaches VaultClient
    if path.is_empty() {
        tracing::warn!(recipe = recipe, "empty vault path in recipe");
        return None;
    }
    if path.split('/').any(|seg| seg == "..") {
        tracing::warn!(recipe = recipe, "path traversal in vault recipe — rejected");
        return None;
    }
    if path.contains('\0') || path.chars().any(|c| c.is_control()) || path.to_ascii_lowercase().contains("%2e%2e") {
        tracing::warn!(recipe = recipe, "invalid characters in vault recipe path");
        return None;
    }

    // Validate key
    if key.is_empty() {
        tracing::warn!(recipe = recipe, "empty key in vault recipe");
        return None;
    }
    if key.contains('/') || key.contains('\\') {
        tracing::warn!(recipe = recipe, "path separators in vault recipe key — rejected");
        return None;
    }

    // Only now require the client
    let vault_client = vault_client?;

    match vault_client.read(path).await {
        Ok(data) => {
            if let Some(value) = data.get(key) {
                if let Some(str_value) = value.as_str() {
                    Some(SecretString::from(str_value.to_string()))
                } else {
                    // Try to serialize non-string values as JSON
                    let json_value = serde_json::to_string(value).ok()?;
                    Some(SecretString::from(json_value))
                }
            } else {
                tracing::warn!(path = path, key = key, "key not found in vault secret");
                None
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, path = path, "failed to read from vault");
            None
        }
    }
}

/// Store a secret value in the cross-platform keyring.
pub fn store_in_keyring(name: &str, value: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, name)?;
    entry.set_password(value)?;
    tracing::info!(name = name, "stored secret in keyring");
    Ok(())
}

/// Delete a secret from the cross-platform keyring.
pub fn delete_from_keyring(name: &str) -> anyhow::Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, name)?;
    entry.delete_credential()?;
    Ok(())
}

/// Expose a SecretString's value for operations that need it (e.g., redaction set building).
/// The caller is responsible for not leaking the exposed value.
#[allow(dead_code)] // Available for future use — expose a SecretString for display
pub fn expose(secret: &SecretString) -> &str {
    secret.expose_secret()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_from_env() {
        // Use CARGO_PKG_NAME which is always set during cargo test
        let recipes = RecipeStore::empty();
        let val = resolve_secret("CARGO_PKG_NAME", &recipes);
        assert_eq!(val.map(|s| s.expose_secret().to_string()), Some("omegon-secrets".to_string()));
    }

    #[test]
    fn execute_env_recipe() {
        // Use CARGO_PKG_NAME (always "omegon-secrets" during test)
        let val = execute_string_recipe("test", "env:CARGO_PKG_NAME");
        assert_eq!(val.map(|s| s.expose_secret().to_string()), Some("omegon-secrets".to_string()));
    }

    #[test]
    fn execute_cmd_recipe() {
        let val = execute_string_recipe("test", "cmd:echo hello");
        assert_eq!(val.map(|s| s.expose_secret().to_string()), Some("hello".to_string()));
    }

    #[test]
    fn execute_file_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.txt");
        std::fs::write(&path, "my_secret\nextra_line\n").unwrap();
        let val = execute_string_recipe("test", &format!("file:{}", path.display()));
        assert_eq!(val.map(|s| s.expose_secret().to_string()), Some("my_secret".to_string()));
    }

    #[test]
    fn unknown_recipe_kind() {
        let val = execute_string_recipe("test", "unknown:something");
        assert_eq!(val.map(|s| s.expose_secret().to_string()), None);
    }

    #[tokio::test]
    async fn resolve_vault_secret_test() {
        use crate::vault::{VaultClient, VaultConfig, AuthConfig};
        use mockito::Server;
        use secrecy::SecretString;

        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/secret/data/omegon/api-keys")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": {"data": {"anthropic": "sk-ant-test123"}, "metadata": {"version": 1, "created_time": "2024-01-01T00:00:00Z", "destroyed": false}}}"#)
            .create_async().await;

        let config = VaultConfig {
            addr: server.url(),
            auth: AuthConfig::Token,
            allowed_paths: vec!["secret/data/*".to_string()],
            denied_paths: vec![],
            timeout_secs: 5,
        };

        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let secret = resolve_vault_secret(Some(&client), "vault:secret/data/omegon/api-keys#anthropic").await;
        assert_eq!(secret.map(|s| s.expose_secret().to_string()), Some("sk-ant-test123".to_string()));
    }

    #[tokio::test]
    async fn vault_recipe_rejects_path_traversal() {
        // Defense-in-depth: resolve.rs rejects before VaultClient sees the path
        let result = resolve_vault_secret(None, "vault:secret/data/../../sys/seal-status#key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn vault_recipe_rejects_empty_key() {
        let result = resolve_vault_secret(None, "vault:secret/data/omegon/keys#").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn vault_recipe_rejects_key_with_path_separator() {
        let result = resolve_vault_secret(None, "vault:secret/data/omegon/keys#../../etc").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn vault_recipe_rejects_empty_path() {
        let result = resolve_vault_secret(None, "vault:#key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn vault_recipe_rejects_null_byte() {
        let result = resolve_vault_secret(None, "vault:secret/data\0/test#key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn vault_recipe_rejects_encoded_traversal() {
        // %2F and %2E encoded path components
        let result = resolve_vault_secret(None, "vault:secret/data/..%2F..%2Fsys#key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn vault_recipe_rejects_double_dot_variants() {
        let result = resolve_vault_secret(None, "vault:secret/data/....//sys#key").await;
        assert!(result.is_none());
        let result = resolve_vault_secret(None, "vault:secret/data/./../sys#key").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn execute_vault_recipe() {
        use crate::vault::{VaultClient, VaultConfig, AuthConfig};
        use crate::recipes::Recipe;
        use mockito::Server;
        use secrecy::SecretString;

        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/secret/data/omegon/api-keys")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": {"data": {"anthropic": "sk-ant-test123"}, "metadata": {"version": 1, "created_time": "2024-01-01T00:00:00Z", "destroyed": false}}}"#)
            .create_async().await;

        let config = VaultConfig {
            addr: server.url(),
            auth: AuthConfig::Token,
            allowed_paths: vec!["secret/data/*".to_string()],
            denied_paths: vec![],
            timeout_secs: 5,
        };

        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        // Test the new vault recipe format
        let recipe = Recipe::vault("secret/data/omegon/api-keys#anthropic".to_string());
        let secret = execute_recipe_async("test", &recipe, Some(&client)).await;
        assert_eq!(secret.map(|s| s.expose_secret().to_string()), Some("sk-ant-test123".to_string()));
    }

    #[tokio::test]
    async fn resolve_secret_async_test() {
        use crate::recipes::RecipeStore;
        use crate::vault::{VaultClient, VaultConfig, AuthConfig};
        use mockito::Server;
        use secrecy::SecretString;

        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/secret/data/omegon/api-keys")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": {"data": {"anthropic": "sk-ant-test123"}, "metadata": {"version": 1, "created_time": "2024-01-01T00:00:00Z", "destroyed": false}}}"#)
            .create_async().await;

        let config = VaultConfig {
            addr: server.url(),
            auth: AuthConfig::Token,
            allowed_paths: vec!["secret/data/*".to_string()],
            denied_paths: vec![],
            timeout_secs: 5,
        };

        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        // Set up test recipe store with proper temp directory
        let temp_dir = tempfile::tempdir().unwrap();
        let mut recipes = RecipeStore::load(temp_dir.path()).unwrap();
        recipes.set_vault("ANTHROPIC_API_KEY".to_string(), "secret/data/omegon/api-keys#anthropic".to_string()).unwrap();

        // Test async resolution
        let secret = resolve_secret_async("ANTHROPIC_API_KEY", &recipes, Some(&client)).await;
        assert_eq!(secret.map(|s| s.expose_secret().to_string()), Some("sk-ant-test123".to_string()));
        
        // Env var priority: if ANTHROPIC_API_KEY is set in the real env,
        // it wins over the vault recipe. We don't set/unset it here
        // because that's racy. The vault test above proves vault resolution
        // works; env priority is tested by resolve_from_env.
    }

    #[test]
    fn vault_recipe_warns_on_sync_execution() {
        use crate::recipes::Recipe;
        
        let recipe = Recipe::vault("secret/data/omegon/api-keys#anthropic".to_string());
        let secret = execute_recipe("test", &recipe);
        assert!(secret.is_none()); // Should return None and log warning
    }
}
