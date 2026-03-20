//! Vault HTTP client for secret resolution, auth negotiation, and lifecycle management.
//!
//! Features:
//! - KV v2 secret engine support (read/write/list)
//! - Multiple auth methods (token, AppRole, Kubernetes SA) with fallback chain
//! - Client-side path allowlist/denylist enforcement
//! - Child token minting for cleave operations
//! - Health checks and unseal support
//! - Token lifecycle management (lookup/renew)
//!
//! Security:
//! - All tokens are stored as SecretString (zeroized on drop)
//! - Path enforcement happens before HTTP calls
//! - Failed auth attempts are logged but not exposed
//! - Network timeouts prevent hanging

use anyhow::{anyhow, Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use reqwest::{Client, Response};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info};
use url::Url;

/// Vault configuration loaded from vault.json or environment.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VaultConfig {
    /// Vault server address (e.g., "https://vault.example.com:8200")
    pub addr: String,
    /// Authentication method configuration
    #[serde(default)]
    pub auth: AuthConfig,
    /// Allowed paths patterns (glob syntax, e.g., "secret/data/omegon/*")
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    /// Denied paths patterns (takes precedence over allowed)
    #[serde(default)]
    pub denied_paths: Vec<String>,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}

/// Authentication method configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(tag = "method")]
pub enum AuthConfig {
    #[default]
    #[serde(rename = "token")]
    Token,
    #[serde(rename = "approle")]
    AppRole {
        role_id: String,
        /// Secret ID will be read from keyring under this key
        secret_id_key: String,
    },
    #[serde(rename = "kubernetes")]
    Kubernetes {
        role: String,
        /// Path to service account token file
        #[serde(default = "default_k8s_token_path")]
        token_path: String,
    },
}

fn default_k8s_token_path() -> String {
    "/var/run/secrets/kubernetes.io/serviceaccount/token".to_string()
}
/// Vault seal status response.
#[derive(Debug, Deserialize)]
pub struct SealStatus {
    pub sealed: bool,
    pub t: u32,       // threshold
    pub n: u32,       // total shares
    pub progress: u32, // keys provided so far
}

/// Vault health status.
#[derive(Debug, Deserialize)]
pub struct HealthStatus {
    pub sealed: bool,
    pub initialized: bool,
    pub standby: bool,
}

/// Token lookup response.
#[derive(Debug, Deserialize)]
pub struct TokenInfo {
    pub ttl: u64,
    pub renewable: bool,
    pub policies: Vec<String>,
    pub creation_time: i64,
}

/// KV v2 secret response.
#[derive(Debug, Deserialize)]
pub struct KvV2Response {
    pub data: KvV2Data,
}

#[derive(Debug, Deserialize)]
pub struct KvV2Data {
    pub data: HashMap<String, serde_json::Value>,
    #[allow(dead_code)]
    pub metadata: KvV2Metadata,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct KvV2Metadata {
    pub version: u32,
    pub created_time: String,
    pub destroyed: bool,
}

/// Child token creation request.
#[derive(Debug, Serialize)]
pub struct CreateTokenRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_uses: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renewable: Option<bool>,
}

/// Child token creation response.
#[derive(Debug, Deserialize)]
pub struct CreateTokenResponse {
    pub auth: TokenAuth,
}

#[derive(Debug, Deserialize)]
pub struct TokenAuth {
    pub client_token: String,
    #[allow(dead_code)]
    pub lease_duration: u64,
    #[allow(dead_code)]
    pub renewable: bool,
    #[allow(dead_code)]
    pub policies: Vec<String>,
}

/// Path enforcement policy — fail-closed by design.
///
/// An empty or missing allowlist produces `DenyAll`, which rejects every path.
/// The operator must explicitly configure `allowed_paths` to open access.
pub enum PathPolicy {
    /// No paths are allowed — default when allowlist is empty/missing.
    DenyAll,
    /// Paths matching the allow set are permitted, unless they also match the deny set.
    /// Deny takes precedence over allow.
    AllowList {
        allow: GlobSet,
        deny: GlobSet,
    },
}

impl PathPolicy {
    /// Build a PathPolicy from config arrays.
    ///
    /// - Empty `allowed` → `DenyAll` (fail-closed).
    /// - Non-empty `allowed` → `AllowList` with compiled globs.
    /// - `denied` is always compiled (may be empty).
    pub fn from_config(allowed: &[String], denied: &[String]) -> Result<Self> {
        if allowed.is_empty() {
            return Ok(PathPolicy::DenyAll);
        }
        let allow = build_globset(allowed).context("invalid allowed_paths pattern")?;
        let deny = build_globset(denied).context("invalid denied_paths pattern")?;
        Ok(PathPolicy::AllowList { allow, deny })
    }

    /// Check whether a path is permitted under this policy.
    ///
    /// Returns `Ok(())` if allowed, `Err` with reason if denied.
    /// Path traversal, null bytes, and encoded traversal are rejected
    /// before any glob matching.
    fn check(&self, path: &str) -> Result<()> {
        // SECURITY: Reject path traversal before glob matching.
        // Prevents normalization confusion between globs and Url::join.
        if path.split('/').any(|seg| seg == "..") {
            return Err(anyhow!("path traversal detected: {}", path));
        }
        // Case-insensitive check — %2e%2E and %2E%2e are valid per RFC 3986
        let lower = path.to_ascii_lowercase();
        if path.contains('\0') || lower.contains("%2e%2e") {
            return Err(anyhow!("invalid characters in vault path: {}", path));
        }

        match self {
            PathPolicy::DenyAll => {
                Err(anyhow!("no paths allowed — configure allowed_paths in vault.json"))
            }
            PathPolicy::AllowList { allow, deny } => {
                // Deny takes precedence
                if deny.is_match(path) {
                    return Err(anyhow!("path denied by client-side policy: {}", path));
                }
                if !allow.is_match(path) {
                    return Err(anyhow!("path not in allowlist: {}", path));
                }
                Ok(())
            }
        }
    }
}

/// Vault HTTP client with authentication and path enforcement.
pub struct VaultClient {
    /// Base Vault server URL
    base_url: Url,
    /// HTTP client with configured timeout
    client: Client,
    /// Current Vault token (zeroized on drop)
    token: Option<SecretString>,
    /// Configuration
    config: VaultConfig,
    /// Path enforcement policy (fail-closed: DenyAll when no allowlist)
    path_policy: PathPolicy,
}

impl VaultConfig {
    /// Load configuration from vault.json in the config directory, or from environment.
    ///
    /// Priority: vault.json (explicit operator config) > VAULT_ADDR env (implicit).
    /// VAULT_ADDR-only starts with DenyAll to force the operator to create vault.json.
    pub fn load_config(config_dir: &Path) -> Result<Option<VaultConfig>> {
        // 1. vault.json is authoritative — operator explicitly configured paths
        let vault_config_path = config_dir.join("vault.json");
        if vault_config_path.exists() {
            let content = std::fs::read_to_string(&vault_config_path)
                .context("failed to read vault.json")?;
            let config: VaultConfig = serde_json::from_str(&content)
                .context("invalid vault.json format")?;
            debug!("loaded vault config from {}", vault_config_path.display());
            return Ok(Some(config));
        }

        // 2. Fallback: VAULT_ADDR env — starts DenyAll (no vault.json = no allowlist)
        if let Ok(addr) = std::env::var("VAULT_ADDR") && !addr.is_empty() {
            info!("using VAULT_ADDR: {}", addr);
            tracing::warn!(
                "vault configured via VAULT_ADDR but no vault.json found — \
                 all secret paths denied (fail-closed). Create {}/vault.json \
                 with allowed_paths to enable secret resolution",
                config_dir.display()
            );
            return Ok(Some(VaultConfig {
                addr,
                auth: AuthConfig::Token,
                allowed_paths: vec![],  // DenyAll — operator must create vault.json
                denied_paths: vec![],
                timeout_secs: default_timeout(),
            }));
        }

        // No configuration found
        Ok(None)
    }
}

impl VaultClient {
    /// Create a new Vault client from configuration.
    ///
    /// Does not authenticate immediately - call `authenticate()` to establish a session.
    pub fn new(config: VaultConfig) -> Result<Self> {
        let base_url = Url::parse(&config.addr)
            .context("invalid vault address")?;

        // Validate URL scheme — only http/https allowed (prevents file:// SSRF)
        match base_url.scheme() {
            "http" | "https" => {}
            scheme => return Err(anyhow!("unsupported vault URL scheme: {}", scheme)),
        }

        let timeout = Duration::from_secs(config.timeout_secs);
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            // SECURITY: Do not follow redirects — prevents token exfiltration
            // via crafted 3xx responses that forward X-Vault-Token to attacker-controlled hosts
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to create HTTP client")?;

        // Build fail-closed path policy
        let path_policy = PathPolicy::from_config(&config.allowed_paths, &config.denied_paths)?;

        Ok(Self {
            base_url,
            client,
            token: None,
            config,
            path_policy,
        })
    }
    ///
    /// Priority order:
    /// 1. VAULT_TOKEN environment variable
    /// 2. ~/.vault-token file
    /// 3. Configured auth method (AppRole, Kubernetes SA)
    pub async fn authenticate(&mut self) -> Result<()> {
        // 1. Check VAULT_TOKEN environment variable
        if let Ok(token) = std::env::var("VAULT_TOKEN") && !token.is_empty() {
            debug!("using VAULT_TOKEN environment variable");
            self.token = Some(SecretString::from(token));
            return Ok(());
        }

        // 2. Check ~/.vault-token file
        if let Some(home) = dirs::home_dir() {
            let token_file = home.join(".vault-token");
            if token_file.exists()
                && let Ok(token) = std::fs::read_to_string(&token_file)
                && !token.trim().is_empty()
            {
                debug!("using token from ~/.vault-token");
                self.token = Some(SecretString::from(token.trim().to_string()));
                return Ok(());
            }
        }

        // 3. Use configured auth method
        let auth_config = self.config.auth.clone();
        match auth_config {
            AuthConfig::Token => {
                return Err(anyhow!("no token found in VAULT_TOKEN or ~/.vault-token"));
            }
            AuthConfig::AppRole { role_id, secret_id_key } => {
                self.authenticate_approle(&role_id, &secret_id_key).await?;
            }
            AuthConfig::Kubernetes { role, token_path } => {
                self.authenticate_kubernetes(&role, &token_path).await?;
            }
        }

        Ok(())
    }

    /// Authenticate using AppRole method.
    async fn authenticate_approle(&mut self, role_id: &str, secret_id_key: &str) -> Result<()> {
        // Get secret_id from keyring
        let entry = keyring::Entry::new("omegon", secret_id_key)
            .context("failed to create keyring entry")?;
        let secret_id = entry.get_password()
            .with_context(|| format!("secret_id not found in keyring: {}", secret_id_key))?;

        let login_data = serde_json::json!({
            "role_id": role_id,
            "secret_id": secret_id
        });

        let url = self.base_url.join("v1/auth/approle/login")?;
        let response = self.client
            .post(url)
            .json(&login_data)
            .send()
            .await
            .context("AppRole login request failed")?;

        if response.status().is_success() {
            let auth_response: serde_json::Value = response.json().await?;
            if let Some(client_token) = auth_response["auth"]["client_token"].as_str() {
                self.token = Some(SecretString::from(client_token.to_string()));
                info!("authenticated with Vault using AppRole");
            } else {
                return Err(anyhow!("no client_token in AppRole response"));
            }
        } else {
            let status = response.status();
            let body = sanitize_error_body(&response.text().await.unwrap_or_default());
            return Err(anyhow!("AppRole login failed: {} - {}", status, body));
        }

        Ok(())
    }

    /// Authenticate using Kubernetes service account.
    async fn authenticate_kubernetes(&mut self, role: &str, token_path: &str) -> Result<()> {
        let jwt = std::fs::read_to_string(token_path)
            .with_context(|| format!("failed to read K8s SA token from {}", token_path))?;

        let login_data = serde_json::json!({
            "role": role,
            "jwt": jwt.trim()
        });

        let url = self.base_url.join("v1/auth/kubernetes/login")?;
        let response = self.client
            .post(url)
            .json(&login_data)
            .send()
            .await
            .context("Kubernetes login request failed")?;

        if response.status().is_success() {
            let auth_response: serde_json::Value = response.json().await?;
            if let Some(client_token) = auth_response["auth"]["client_token"].as_str() {
                self.token = Some(SecretString::from(client_token.to_string()));
                info!("authenticated with Vault using Kubernetes SA");
            } else {
                return Err(anyhow!("no client_token in Kubernetes response"));
            }
        } else {
            let status = response.status();
            let body = sanitize_error_body(&response.text().await.unwrap_or_default());
            return Err(anyhow!("Kubernetes login failed: {} - {}", status, body));
        }

        Ok(())
    }

    /// Set the token directly (e.g., from VAULT_TOKEN env).
    pub fn set_token(&mut self, token: SecretString) {
        self.token = Some(token);
    }

    /// Check if the client has a valid token.
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }

    /// Get the Vault server address.
    pub fn server_addr(&self) -> &str {
        self.base_url.as_str()
    }

    /// Get Vault health status.
    pub async fn health(&self) -> Result<HealthStatus> {
        let url = self.base_url.join("v1/sys/health?standbyok=true&sealedcode=200")?;
        let response = self.client
            .get(url)
            .send()
            .await
            .context("health check failed")?;

        let health: HealthStatus = response.json().await
            .context("invalid health response")?;
        Ok(health)
    }

    /// Get seal status.
    pub async fn seal_status(&self) -> Result<SealStatus> {
        let url = self.base_url.join("v1/sys/seal-status")?;
        let response = self.client
            .get(url)
            .send()
            .await
            .context("seal status check failed")?;

        let status: SealStatus = response.json().await
            .context("invalid seal status response")?;
        Ok(status)
    }

    /// Submit an unseal key.
    ///
    /// Takes a `SecretString` to ensure unseal keys are never held as
    /// plain strings in agent-visible memory. This method is intended
    /// for TUI-only use — the agent loop should never call it.
    pub async fn unseal(&self, key: &SecretString) -> Result<SealStatus> {
        let url = self.base_url.join("v1/sys/unseal")?;
        let data = serde_json::json!({ "key": key.expose_secret() });
        
        let response = self.client
            .post(url)
            .json(&data)
            .send()
            .await
            .context("unseal request failed")?;

        let status: SealStatus = response.json().await
            .context("invalid unseal response")?;
        Ok(status)
    }

    /// Look up information about the current token.
    pub async fn token_lookup(&self) -> Result<TokenInfo> {

        let url = self.base_url.join("v1/auth/token/lookup-self")?;
        let response = self.authenticated_request(|req| req.get(url.clone())).await?;

        let data: serde_json::Value = response.json().await?;
        let token_data = &data["data"];
        
        Ok(TokenInfo {
            ttl: token_data["ttl"].as_u64().unwrap_or(0),
            renewable: token_data["renewable"].as_bool().unwrap_or(false),
            policies: token_data["policies"].as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            creation_time: token_data["creation_time"].as_i64().unwrap_or(0),
        })
    }

    /// Renew the current token.
    pub async fn token_renew(&self, increment: Option<&str>) -> Result<TokenInfo> {

        let url = self.base_url.join("v1/auth/token/renew-self")?;
        let mut data = serde_json::Map::new();
        if let Some(inc) = increment {
            data.insert("increment".to_string(), serde_json::Value::String(inc.to_string()));
        }

        let response = self.authenticated_request(|req| {
            req.post(url.clone()).json(&data)
        }).await?;

        let resp_data: serde_json::Value = response.json().await?;
        let token_data = &resp_data["auth"];
        
        Ok(TokenInfo {
            ttl: token_data["lease_duration"].as_u64().unwrap_or(0),
            renewable: token_data["renewable"].as_bool().unwrap_or(false),
            policies: token_data["policies"].as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            creation_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        })
    }

    /// Read a secret from KV v2.
    pub async fn read(&self, path: &str) -> Result<HashMap<String, serde_json::Value>> {
        // Enforce path allowlist/denylist
        self.check_path_allowed(path)?;
        let url = self.base_url.join(&format!("v1/{}", path))?;
        let response = self.authenticated_request(|req| req.get(url.clone())).await?;

        if response.status().is_success() {
            let kv_response: KvV2Response = response.json().await
                .context("invalid KV v2 response")?;
            Ok(kv_response.data.data)
        } else if response.status().as_u16() == 404 {
            Err(anyhow!("secret not found at path: {}", path))
        } else {
            let status = response.status();
            let body = sanitize_error_body(&response.text().await.unwrap_or_default());
            Err(anyhow!("read failed: {} - {}", status, body))
        }
    }

    /// Write a secret to KV v2.
    pub async fn write(&self, path: &str, data: HashMap<String, serde_json::Value>) -> Result<()> {
        // Enforce path allowlist/denylist
        self.check_path_allowed(path)?;
        let payload = serde_json::json!({ "data": data });
        let url = self.base_url.join(&format!("v1/{}", path))?;
        
        let response = self.authenticated_request(|req| {
            req.post(url.clone()).json(&payload)
        }).await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = sanitize_error_body(&response.text().await.unwrap_or_default());
            Err(anyhow!("write failed: {} - {}", status, body))
        }
    }

    /// List secrets at a path.
    pub async fn list(&self, path: &str) -> Result<Vec<String>> {
        // Enforce path allowlist/denylist
        self.check_path_allowed(path)?;
        let url = self.base_url.join(&format!("v1/{}", path))?;
        let response = self.authenticated_request(|req| {
            req.request(reqwest::Method::from_bytes(b"LIST").unwrap(), url.clone())
        }).await?;

        if response.status().is_success() {
            let list_response: serde_json::Value = response.json().await?;
            let keys = list_response["data"]["keys"].as_array()
                .ok_or_else(|| anyhow!("invalid list response"))?;
            
            Ok(keys.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
        } else {
            let status = response.status();
            let body = sanitize_error_body(&response.text().await.unwrap_or_default());
            Err(anyhow!("list failed: {} - {}", status, body))
        }
    }

    /// Mint a child token with restricted policies.
    pub async fn mint_child_token(
        &self,
        policies: Option<Vec<String>>,
        ttl: Option<String>,
        use_limit: Option<u32>,
    ) -> Result<String> {

        let request = CreateTokenRequest {
            policies,
            ttl,
            num_uses: use_limit,
            renewable: Some(false), // Child tokens are typically non-renewable
        };

        let url = self.base_url.join("v1/auth/token/create")?;
        let response = self.authenticated_request(|req| {
            req.post(url.clone()).json(&request)
        }).await?;

        if response.status().is_success() {
            let create_response: CreateTokenResponse = response.json().await
                .context("invalid token creation response")?;
            Ok(create_response.auth.client_token)
        } else {
            let status = response.status();
            let body = sanitize_error_body(&response.text().await.unwrap_or_default());
            Err(anyhow!("child token creation failed: {} - {}", status, body))
        }
    }

    /// Check if a path is allowed by the client-side enforcement rules.
    ///
    /// Delegates to `PathPolicy::check()` which is fail-closed:
    /// - DenyAll (empty allowlist) rejects everything
    /// - AllowList checks traversal → deny → allow
    fn check_path_allowed(&self, path: &str) -> Result<()> {
        self.path_policy.check(path)
    }

    /// Make an authenticated HTTP request to Vault.
    async fn authenticated_request<F>(&self, builder: F) -> Result<Response>
    where
        F: FnOnce(&Client) -> reqwest::RequestBuilder,
    {
        let token = self.token.as_ref()
            .ok_or_else(|| anyhow!("no token available"))?;

        let response = builder(&self.client)
            .header("X-Vault-Token", token.expose_secret())
            .send()
            .await
            .context("vault request failed")?;

        Ok(response)
    }
}

/// Sanitize a Vault HTTP error body for inclusion in error messages.
///
/// Truncates to 200 chars and strips patterns that look like tokens
/// (hvs.*, s.*, base64 blobs) to prevent accidental secret leakage.
fn sanitize_error_body(body: &str) -> String {
    use std::sync::LazyLock;

    static TOKEN_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        // Match Vault token patterns and long base64-ish strings
        regex::Regex::new(r"(?i)(hvs\.[a-zA-Z0-9_\-\.]+|s\.[a-zA-Z0-9_\-\.]{20,}|[a-zA-Z0-9+/=]{40,})").unwrap()
    });

    // Truncate at a char boundary — slicing raw bytes panics on multi-byte UTF-8
    let truncated = match body.char_indices().nth(200) {
        Some((byte_idx, _)) => &body[..byte_idx],
        None => body,
    };
    TOKEN_RE.replace_all(truncated, "[REDACTED]").to_string()
}

/// Build a GlobSet from a list of glob patterns.
fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)
            .with_context(|| format!("invalid glob pattern: {}", pattern))?);
    }
    builder.build()
        .context("failed to build glob set")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use secrecy::SecretString;

    fn test_config(server_url: &str) -> VaultConfig {
        VaultConfig {
            addr: server_url.to_string(),
            auth: AuthConfig::Token,
            allowed_paths: vec!["secret/data/omegon/*".to_string()],
            denied_paths: vec!["secret/data/bootstrap/cloudflare/*".to_string()],
            timeout_secs: 5,
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/sys/health?standbyok=true&sealedcode=200")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sealed": false, "initialized": true, "standby": false}"#)
            .create_async().await;

        let config = test_config(&server.url());
        let client = VaultClient::new(config).unwrap();
        let health = client.health().await.unwrap();

        assert!(!health.sealed);
        assert!(health.initialized);
        assert!(!health.standby);
    }

    #[tokio::test]
    async fn test_seal_status() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/sys/seal-status")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sealed": false, "t": 3, "n": 5, "progress": 0}"#)
            .create_async().await;

        let config = test_config(&server.url());
        let client = VaultClient::new(config).unwrap();
        let status = client.seal_status().await.unwrap();

        assert!(!status.sealed);
        assert_eq!(status.t, 3);
        assert_eq!(status.n, 5);
        assert_eq!(status.progress, 0);
    }

    #[tokio::test]
    async fn test_unseal() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/v1/sys/unseal")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sealed": true, "t": 3, "n": 5, "progress": 1}"#)
            .create_async().await;

        let config = test_config(&server.url());
        let client = VaultClient::new(config).unwrap();
        let status = client.unseal(&SecretString::from("test-key")).await.unwrap();

        assert!(status.sealed);
        assert_eq!(status.progress, 1);
    }

    #[tokio::test]
    async fn test_path_allowlist_enforcement() {
        let config = test_config("http://localhost:8200");
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        // Allowed path should pass
        assert!(client.check_path_allowed("secret/data/omegon/api-keys").is_ok());

        // Disallowed path should fail
        let err = client.check_path_allowed("secret/data/bootstrap/keys").unwrap_err();
        assert!(err.to_string().contains("not in allowlist"));

        // Denied path should fail even if it matches allowed pattern
        let err = client.check_path_allowed("secret/data/bootstrap/cloudflare/test").unwrap_err();
        assert!(err.to_string().contains("denied"));
    }

    #[tokio::test]
    async fn test_read_secret() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/secret/data/omegon/api-keys")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": {"data": {"anthropic": "sk-ant-test123"}, "metadata": {"version": 1, "created_time": "2024-01-01T00:00:00Z", "destroyed": false}}}"#)
            .create_async().await;

        let config = test_config(&server.url());
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let data = client.read("secret/data/omegon/api-keys").await.unwrap();
        assert_eq!(data.get("anthropic").unwrap().as_str().unwrap(), "sk-ant-test123");
    }

    #[tokio::test]
    async fn test_write_secret() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/v1/secret/data/omegon/test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .create_async().await;

        let config = test_config(&server.url());
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let mut data = HashMap::new();
        data.insert("key".to_string(), serde_json::Value::String("value".to_string()));

        client.write("secret/data/omegon/test", data).await.unwrap();
    }

    #[tokio::test]
    async fn test_token_lookup() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/auth/token/lookup-self")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": {"ttl": 3600, "renewable": true, "policies": ["default", "omegon"], "creation_time": 1641024000}}"#)
            .create_async().await;

        let config = test_config(&server.url());
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let info = client.token_lookup().await.unwrap();
        assert_eq!(info.ttl, 3600);
        assert!(info.renewable);
        assert!(info.policies.contains(&"omegon".to_string()));
    }

    #[tokio::test]
    async fn test_list_secrets() {
        let mut server = Server::new_async().await;
        let _m = server.mock("LIST", "/v1/secret/metadata/omegon/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": {"keys": ["api-keys", "tokens", "config"]}}"#)
            .create_async().await;

        let config = VaultConfig {
            addr: server.url(),
            auth: AuthConfig::Token,
            allowed_paths: vec!["secret/data/omegon/*".to_string(), "secret/metadata/omegon/*".to_string()],
            denied_paths: vec![],
            timeout_secs: 5,
        };
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let keys = client.list("secret/metadata/omegon/").await.unwrap();
        assert_eq!(keys, vec!["api-keys", "tokens", "config"]);
    }

    #[tokio::test]
    async fn test_list_disallowed_path() {
        let config = test_config("http://localhost:8200");
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let err = client.list("secret/metadata/other/").await.unwrap_err();
        assert!(err.to_string().contains("not in allowlist"));
    }

    #[tokio::test]
    async fn test_mint_child_token() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/v1/auth/token/create")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"auth": {"client_token": "hvs.child123", "lease_duration": 1800, "renewable": false, "policies": ["omegon-child"]}}"#)
            .create_async().await;

        let config = test_config(&server.url());
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.parent"));

        let child_token = client.mint_child_token(
            Some(vec!["omegon-child".to_string()]),
            Some("30m".to_string()),
            Some(100),
        ).await.unwrap();

        assert_eq!(child_token, "hvs.child123");
    }

    #[tokio::test]
    async fn test_approle_auth() {
        // Use a unique keyring key per test run to avoid polluting real keyring
        let test_key = format!("vault-test-approle-{}", std::process::id());

        // Set up keyring entry — skip if keyring unavailable (CI, headless)
        let entry = match keyring::Entry::new("omegon", &test_key) {
            Ok(e) => e,
            Err(_) => return,
        };
        if entry.set_password("secret123").is_err() {
            return;
        }

        // Ensure cleanup runs even on panic
        struct KeyringCleanup(keyring::Entry);
        impl Drop for KeyringCleanup {
            fn drop(&mut self) { let _ = self.0.delete_credential(); }
        }
        let _cleanup = KeyringCleanup(keyring::Entry::new("omegon", &test_key).unwrap());

        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/v1/auth/approle/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"auth": {"client_token": "hvs.approle123", "lease_duration": 7200}}"#)
            .create_async().await;

        let config = VaultConfig {
            addr: server.url(),
            auth: AuthConfig::AppRole {
                role_id: "test-role".to_string(),
                secret_id_key: test_key.clone(),
            },
            allowed_paths: vec![],
            denied_paths: vec![],
            timeout_secs: 5,
        };

        let mut client = VaultClient::new(config).unwrap();
        client.authenticate_approle("test-role", &test_key).await.unwrap();

        assert!(client.is_authenticated());
    }

    // ---- Security-specific tests ----

    #[tokio::test]
    async fn test_path_traversal_rejected() {
        let config = test_config("http://localhost:8200");
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        // Direct traversal
        let err = client.check_path_allowed("secret/data/../../sys/seal-status").unwrap_err();
        assert!(err.to_string().contains("path traversal"));

        // Leading traversal
        let err = client.check_path_allowed("../../../etc/passwd").unwrap_err();
        assert!(err.to_string().contains("path traversal"));

        // Null byte injection
        let err = client.check_path_allowed("secret/data/omegon/test\0/../../../sys/seal-status").unwrap_err();
        assert!(err.to_string().contains("path traversal") || err.to_string().contains("invalid characters"));

        // URL-encoded traversal
        let err = client.check_path_allowed("secret/data/%2e%2e/sys/seal-status").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));

        // Mixed-case URL-encoded traversal (RFC 3986 permits either case)
        let err = client.check_path_allowed("secret/data/%2E%2e/sys/seal-status").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
        let err = client.check_path_allowed("secret/data/%2e%2E/sys/seal-status").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));

        // Normal paths still work
        assert!(client.check_path_allowed("secret/data/omegon/api-keys").is_ok());
    }

    #[test]
    fn test_reject_non_http_scheme() {
        let config = VaultConfig {
            addr: "file:///etc/passwd".to_string(),
            auth: AuthConfig::Token,
            allowed_paths: vec![],
            denied_paths: vec![],
            timeout_secs: 5,
        };
        match VaultClient::new(config) {
            Ok(_) => panic!("expected error for file:// scheme"),
            Err(e) => assert!(e.to_string().contains("unsupported vault URL scheme")),
        }

        // ftp scheme
        let config2 = VaultConfig {
            addr: "ftp://evil.com/".to_string(),
            auth: AuthConfig::Token,
            allowed_paths: vec![],
            denied_paths: vec![],
            timeout_secs: 5,
        };
        match VaultClient::new(config2) {
            Ok(_) => panic!("expected error for ftp:// scheme"),
            Err(e) => assert!(e.to_string().contains("unsupported vault URL scheme")),
        }
    }

    #[tokio::test]
    async fn test_redirect_not_followed() {
        let mut server = Server::new_async().await;
        // Set up a redirect — should NOT be followed (token protection)
        let _m = server.mock("GET", "/v1/sys/health?standbyok=true&sealedcode=200")
            .with_status(301)
            .with_header("Location", "http://evil.attacker.com/steal-token")
            .create_async().await;

        let config = test_config(&server.url());
        let client = VaultClient::new(config).unwrap();
        // health() should fail with a deserialization error (got redirect, not JSON)
        // NOT follow the redirect and leak the token
        let result = client.health().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_traversal_bypass_attempt_on_read() {
        // This is the actual attack: path matches the glob but hits a different URL
        let config = VaultConfig {
            addr: "http://localhost:8200".to_string(),
            auth: AuthConfig::Token,
            allowed_paths: vec!["secret/data/*".to_string()],
            denied_paths: vec![],
            timeout_secs: 5,
        };
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        // This path WOULD match "secret/data/*" glob without normalization check,
        // but after URL::join would resolve to v1/sys/seal-status
        let err = client.read("secret/data/../../sys/seal-status").await.unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }

    // ---- Fail-closed tests ----

    #[test]
    fn test_empty_allowlist_denies_all() {
        let config = VaultConfig {
            addr: "http://localhost:8200".to_string(),
            auth: AuthConfig::Token,
            allowed_paths: vec![],  // Empty = DenyAll
            denied_paths: vec![],
            timeout_secs: 5,
        };
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        let err = client.check_path_allowed("secret/data/omegon/anything").unwrap_err();
        assert!(err.to_string().contains("no paths allowed"), "empty allowlist must deny: {}", err);
    }

    #[tokio::test]
    async fn test_deny_all_still_allows_health() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/v1/sys/health?standbyok=true&sealedcode=200")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sealed": false, "initialized": true, "standby": false}"#)
            .create_async().await;

        let config = VaultConfig {
            addr: server.url(),
            auth: AuthConfig::Token,
            allowed_paths: vec![],  // DenyAll
            denied_paths: vec![],
            timeout_secs: 5,
        };
        let client = VaultClient::new(config).unwrap();

        // Health bypasses path enforcement — it uses a fixed endpoint
        let health = client.health().await.unwrap();
        assert!(health.initialized);
    }

    #[test]
    fn test_path_policy_deny_all_variant() {
        let policy = PathPolicy::from_config(&[], &[]).unwrap();
        assert!(matches!(policy, PathPolicy::DenyAll));
        assert!(policy.check("anything").is_err());
    }

    #[test]
    fn test_path_policy_allow_list_variant() {
        let policy = PathPolicy::from_config(
            &["secret/data/omegon/*".to_string()],
            &["secret/data/omegon/internal/*".to_string()],
        ).unwrap();
        assert!(matches!(policy, PathPolicy::AllowList { .. }));
        assert!(policy.check("secret/data/omegon/api-keys").is_ok());
        assert!(policy.check("secret/data/omegon/internal/tokens").is_err());
        assert!(policy.check("secret/data/other/keys").is_err());
    }

    #[test]
    fn test_sanitize_error_body() {
        // Token-like patterns are redacted
        assert!(sanitize_error_body("error: hvs.CAESIFh123xyz").contains("[REDACTED]"));
        assert!(!sanitize_error_body("error: hvs.CAESIFh123xyz").contains("hvs."));

        // Long base64 is redacted
        let long_b64 = "aAbBcCdDeEfF0123456789+/=".repeat(3); // 75 chars of base64
        assert!(sanitize_error_body(&format!("token: {}", long_b64)).contains("[REDACTED]"));

        // Short normal text passes through
        assert_eq!(sanitize_error_body("permission denied"), "permission denied");

        // Truncation at 200 chars
        let long = "x".repeat(300);
        assert!(sanitize_error_body(&long).len() <= 210); // 200 + possible [REDACTED]
    }

    #[tokio::test]
    async fn test_deny_all_read_rejected() {
        let config = VaultConfig {
            addr: "http://localhost:8200".to_string(),
            auth: AuthConfig::Token,
            allowed_paths: vec![],
            denied_paths: vec![],
            timeout_secs: 5,
        };
        let mut client = VaultClient::new(config).unwrap();
        client.set_token(SecretString::from("hvs.test"));

        // Read with DenyAll should fail without making HTTP request
        let err = client.read("secret/data/omegon/keys").await.unwrap_err();
        assert!(err.to_string().contains("no paths allowed"));
    }
}