//! OAuth authentication — login flows, token refresh, credential storage.
//!
//! Supported providers:
//!   - Anthropic (Claude Pro/Max): PKCE flow to claude.ai, callback on :53692
//!   - OpenAI Codex (ChatGPT Plus/Pro): PKCE flow to auth.openai.com, callback on :1455
//!
//! Token refresh happens automatically when the stored token is expired.

use crate::status::ProviderStatus;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Authentication status for all providers and backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    pub providers: Vec<ProviderInfo>,
    pub vault: Vec<VaultInfo>,
    pub secrets: Vec<SecretsInfo>,
    pub mcp: Vec<McpInfo>,
}

/// Provider authentication information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub status: ProviderAuthStatus,
    pub is_oauth: bool,
    pub details: Option<String>,
}

/// Provider authentication status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ProviderAuthStatus {
    Authenticated,
    Expired,
    Missing,
    Error,
}

/// Vault backend information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultInfo {
    pub addr: String,
    pub accessible: bool,
    pub sealed: Option<bool>,
}

/// Secrets store information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsInfo {
    pub store: String,
    pub unlocked: bool,
}

/// MCP server information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpInfo {
    pub server: String,
    pub connected: bool,
}

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CALLBACK_PORT: u16 = 53692;
const REDIRECT_URI: &str = "http://localhost:53692/callback";
const SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

/// Stored OAuth credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    #[serde(rename = "type")]
    pub cred_type: String,
    pub access: String,
    pub refresh: String,
    pub expires: u64, // milliseconds since epoch
}

impl OAuthCredentials {
    pub fn is_expired(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms >= self.expires
    }
}

/// Path to auth.json.
pub fn auth_json_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".pi/agent/auth.json"))
}

/// Read credentials for a provider from auth.json.
pub fn read_credentials(provider: &str) -> Option<OAuthCredentials> {
    let path = auth_json_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let auth: Value = serde_json::from_str(&content).ok()?;
    let entry = auth.get(provider)?;
    serde_json::from_value(entry.clone()).ok()
}

/// Write credentials for a provider to auth.json.
pub fn write_credentials(provider: &str, creds: &OAuthCredentials) -> anyhow::Result<()> {
    let path = auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let _ = std::fs::create_dir_all(path.parent().unwrap());

    let mut auth: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };

    auth[provider] = serde_json::to_value(creds)?;
    std::fs::write(&path, serde_json::to_string_pretty(&auth)?)?;
    Ok(())
}

/// Probe all authentication providers to get current status.
pub async fn probe_all_providers() -> AuthStatus {
    let mut providers = Vec::new();
    
    // Probe Anthropic
    let anthropic_info = probe_provider("anthropic").await;
    providers.push(anthropic_info);
    
    // Probe OpenAI
    let openai_info = probe_provider("openai").await;
    providers.push(openai_info);
    
    // TODO: Probe Vault
    let vault = Vec::new(); // probe_vault().await
    
    // TODO: Probe secrets stores
    let secrets = Vec::new(); // probe_secrets().await
    
    // TODO: Probe MCP servers
    let mcp = Vec::new(); // probe_mcp().await
    
    AuthStatus {
        providers,
        vault,
        secrets,
        mcp,
    }
}

/// Probe a single provider for authentication status.
async fn probe_provider(provider: &str) -> ProviderInfo {
    // Check environment variables first
    let env_keys: &[&str] = match provider {
        "anthropic" => &["ANTHROPIC_API_KEY", "ANTHROPIC_OAUTH_TOKEN"],
        "openai" => &["OPENAI_API_KEY"],
        _ => &[],
    };
    
    for key in env_keys {
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                let is_oauth = key.contains("OAUTH");
                return ProviderInfo {
                    name: provider.to_string(),
                    status: ProviderAuthStatus::Authenticated,
                    is_oauth,
                    details: Some(format!("env:{}", key)),
                };
            }
        }
    }
    
    // Check stored credentials
    let auth_key = if provider == "openai" { "openai-codex" } else { provider };
    if let Some(creds) = read_credentials(auth_key) {
        let status = if creds.is_expired() {
            ProviderAuthStatus::Expired
        } else {
            ProviderAuthStatus::Authenticated
        };
        
        return ProviderInfo {
            name: provider.to_string(),
            status,
            is_oauth: creds.cred_type == "oauth",
            details: Some("stored".to_string()),
        };
    }
    
    // No credentials found
    ProviderInfo {
        name: provider.to_string(),
        status: ProviderAuthStatus::Missing,
        is_oauth: false,
        details: None,
    }
}

/// Remove stored credentials for a provider.
pub fn logout_provider(provider: &str) -> anyhow::Result<()> {
    let path = auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    
    if !path.exists() {
        return Err(anyhow::anyhow!("No credentials found for {provider}"));
    }
    
    let content = std::fs::read_to_string(&path)?;
    let mut auth: Value = serde_json::from_str(&content)?;
    
    let auth_key = if provider == "openai" { "openai-codex" } else { provider };
    
    if auth.get(auth_key).is_none() {
        return Err(anyhow::anyhow!("No credentials found for {provider}"));
    }
    
    // Remove the provider's entry
    if let Some(obj) = auth.as_object_mut() {
        obj.remove(auth_key);
    }
    
    // Write back
    std::fs::write(&path, serde_json::to_string_pretty(&auth)?)?;
    Ok(())
}

/// Resolve API key with automatic token refresh.
/// Returns (api_key, is_oauth_token).
pub async fn resolve_with_refresh(provider: &str) -> Option<(String, bool)> {
    // 1. Env vars first (not OAuth)
    let env_keys: &[&str] = match provider {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        _ => &[],
    };
    for key in env_keys {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty() {
                return Some((val, false));
            }
    }

    // Check ANTHROPIC_OAUTH_TOKEN (explicit OAuth token from env)
    if provider == "anthropic"
        && let Ok(val) = std::env::var("ANTHROPIC_OAUTH_TOKEN")
            && !val.is_empty() {
                return Some((val, true));
            }

    // 2. auth.json — with refresh if expired
    // OpenAI subscription stored as "openai-codex" in auth.json
    let auth_key = if provider == "openai" { "openai-codex" } else { provider };
    let mut creds = read_credentials(auth_key)?;
    if creds.cred_type != "oauth" {
        return Some((creds.access, false));
    }

    if creds.is_expired() {
        tracing::info!(provider, auth_key, "OAuth token expired — refreshing");
        match refresh_token(auth_key, &creds.refresh).await {
            Ok(new_creds) => {
                if let Err(e) = write_credentials(auth_key, &new_creds) {
                    tracing::warn!("Failed to save refreshed token: {e}");
                }
                creds = new_creds;
            }
            Err(e) => {
                tracing::warn!("Token refresh failed: {e} — using expired token");
            }
        }
    }

    Some((creds.access, true))
}

/// Refresh an OAuth token.
pub async fn refresh_token(provider: &str, refresh: &str) -> anyhow::Result<OAuthCredentials> {
    if provider == "openai-codex" { return refresh_openai_token(refresh).await }
    let url = match provider {
        "anthropic" => TOKEN_URL,
        _ => anyhow::bail!("OAuth refresh not supported for provider: {provider}"),
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .json(&json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token refresh failed ({status}): {body}");
    }

    let data: Value = resp.json().await?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let expires_in = data["expires_in"].as_u64().unwrap_or(3600);

    Ok(OAuthCredentials {
        cred_type: "oauth".into(),
        access: data["access_token"].as_str().unwrap_or("").into(),
        refresh: data["refresh_token"].as_str().unwrap_or(refresh).into(),
        expires: now_ms + expires_in * 1000 - 5 * 60 * 1000, // 5 min safety margin
    })
}

// ─── PKCE ───────────────────────────────────────────────────────────────────

fn base64url_encode(bytes: &[u8]) -> String {
    
    // Manual base64url encoding — no external crate needed
    let b64 = crate::tools::view::base64_encode_bytes(bytes);
    b64.replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

fn generate_pkce() -> (String, String) {
    let mut verifier_bytes = [0u8; 32];
    getrandom::fill(&mut verifier_bytes).expect("getrandom failed");
    let verifier = base64url_encode(&verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64url_encode(&hash);

    (verifier, challenge)
}

/// Run the Anthropic OAuth login flow.
/// Opens a browser, listens for the callback, exchanges the code for tokens.
/// Progress callback for OAuth login — allows TUI to receive status updates
/// without writing to stderr.
pub type LoginProgress = Box<dyn Fn(&str) + Send + Sync>;

fn default_progress() -> LoginProgress {
    Box::new(|msg| eprintln!("{msg}"))
}

pub async fn login_anthropic() -> anyhow::Result<OAuthCredentials> {
    login_anthropic_with_progress(default_progress()).await
}

pub async fn login_anthropic_with_progress(
    progress: LoginProgress,
) -> anyhow::Result<OAuthCredentials> {
    let (verifier, challenge) = generate_pkce();

    // Build authorization URL
    let auth_url = format!(
        "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code\
         &redirect_uri={REDIRECT_URI}&scope={}&code_challenge={challenge}\
         &code_challenge_method=S256&state={verifier}",
        urlencoding_encode(SCOPES),
    );

    // Start local HTTP server for the callback
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{CALLBACK_PORT}")).await?;
    tracing::debug!(port = CALLBACK_PORT, "OAuth callback server listening");

    // Open browser
    progress("Opening browser for Anthropic login…");
    let browser_ok = open::that(&auth_url).is_ok();
    if !browser_ok {
        progress(&format!("Could not open browser. Visit:\n  {auth_url}"));
    }

    // Wait for callback
    let (mut stream, _addr) = listener.accept().await?;
    let mut buf = [0u8; 4096];
    let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the code from the GET request
    let (code, state) = parse_callback(&request)?;

    // Send success response
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                    <html><body><p>Authentication successful. Return to your terminal.</p></body></html>";
    tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await?;

    // Verify state
    if state != verifier {
        anyhow::bail!("OAuth state mismatch");
    }

    progress("Exchanging authorization code for tokens…");

    // Exchange code for tokens
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .json(&json!({
            "grant_type": "authorization_code",
            "client_id": CLIENT_ID,
            "code": code,
            "state": state,
            "redirect_uri": REDIRECT_URI,
            "code_verifier": verifier,
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed ({status}): {body}");
    }

    let data: Value = resp.json().await?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let expires_in = data["expires_in"].as_u64().unwrap_or(3600);

    let creds = OAuthCredentials {
        cred_type: "oauth".into(),
        access: data["access_token"].as_str().unwrap_or("").into(),
        refresh: data["refresh_token"].as_str().unwrap_or("").into(),
        expires: now_ms + expires_in * 1000 - 5 * 60 * 1000,
    };

    // Save to auth.json
    write_credentials("anthropic", &creds)?;
    progress("✓ Authentication successful. Credentials saved.");

    Ok(creds)
}

// ─── OpenAI Codex (ChatGPT Plus/Pro) ────────────────────────────────────────

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CALLBACK_PORT: u16 = 1455;
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

/// Run the OpenAI Codex OAuth login flow (ChatGPT Plus/Pro subscription).
pub async fn login_openai() -> anyhow::Result<OAuthCredentials> {
    login_openai_with_progress(default_progress()).await
}

pub async fn login_openai_with_progress(
    progress: LoginProgress,
) -> anyhow::Result<OAuthCredentials> {
    let (verifier, challenge) = generate_pkce();

    // Random state parameter
    let mut state_bytes = [0u8; 16];
    getrandom::fill(&mut state_bytes).expect("getrandom failed");
    let state = hex::encode(&state_bytes);

    let auth_url = format!(
        "{OPENAI_AUTHORIZE_URL}?response_type=code&client_id={OPENAI_CLIENT_ID}\
         &redirect_uri={}&scope={}&code_challenge={challenge}\
         &code_challenge_method=S256&state={state}\
         &id_token_add_organizations=true&codex_cli_simplified_flow=true&originator=omegon",
        urlencoding_encode(OPENAI_REDIRECT_URI),
        urlencoding_encode(OPENAI_SCOPE),
    );

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{OPENAI_CALLBACK_PORT}")).await?;
    tracing::debug!(port = OPENAI_CALLBACK_PORT, "OpenAI OAuth callback server listening");

    progress("Opening browser for OpenAI login…");
    let browser_ok = open::that(&auth_url).is_ok();
    if !browser_ok {
        progress(&format!("Could not open browser. Visit:\n  {auth_url}"));
    }

    let (mut stream, _addr) = listener.accept().await?;
    let mut buf = [0u8; 4096];
    let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let (code, recv_state) = parse_callback_at_path(&request, "/auth/callback")?;

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                    <html><body><p>Authentication successful. Return to your terminal.</p></body></html>";
    tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await?;

    if recv_state != state {
        anyhow::bail!("OAuth state mismatch");
    }

    progress("Exchanging authorization code for tokens…");

    let client = reqwest::Client::new();
    let resp = client
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&client_id={OPENAI_CLIENT_ID}\
             &code={code}&code_verifier={verifier}\
             &redirect_uri={}",
            urlencoding_encode(OPENAI_REDIRECT_URI),
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI token exchange failed ({status}): {body}");
    }

    let data: Value = resp.json().await?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let expires_in = data["expires_in"].as_u64().unwrap_or(3600);
    let access = data["access_token"].as_str().unwrap_or("").to_string();

    // Extract accountId from JWT
    let account_id = extract_jwt_claim(&access, "https://api.openai.com/auth", "chatgpt_account_id");

    let creds = OAuthCredentials {
        cred_type: "oauth".into(),
        access,
        refresh: data["refresh_token"].as_str().unwrap_or("").into(),
        expires: now_ms + expires_in * 1000,
    };

    // Store with accountId as extra field
    write_credentials_with_extra("openai-codex", &creds, account_id.as_deref())?;
    progress("✓ OpenAI authentication successful. Credentials saved.");

    Ok(creds)
}

/// Refresh an OpenAI Codex OAuth token.
pub async fn refresh_openai_token(refresh: &str) -> anyhow::Result<OAuthCredentials> {
    let client = reqwest::Client::new();
    let resp = client
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={refresh}&client_id={OPENAI_CLIENT_ID}"
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI token refresh failed ({status}): {body}");
    }

    let data: Value = resp.json().await?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;
    let expires_in = data["expires_in"].as_u64().unwrap_or(3600);

    Ok(OAuthCredentials {
        cred_type: "oauth".into(),
        access: data["access_token"].as_str().unwrap_or("").into(),
        refresh: data["refresh_token"].as_str().unwrap_or(refresh).into(),
        expires: now_ms + expires_in * 1000,
    })
}

/// Extract a claim from a JWT payload (simple base64 decode, no verification).
fn extract_jwt_claim(token: &str, claim_path: &str, field: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 { return None; }
    // Add padding for base64
    let payload = parts[1];
    let padded = match payload.len() % 4 {
        2 => format!("{payload}=="),
        3 => format!("{payload}="),
        _ => payload.to_string(),
    };
    let decoded = base64_decode(&padded)?;
    let json: Value = serde_json::from_slice(&decoded).ok()?;
    json.get(claim_path)?.get(field)?.as_str().map(String::from)
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    // Standard base64 decode (handles URL-safe chars too)
    let input = input.replace('-', "+").replace('_', "/");
    let mut result = Vec::new();
    let chars: Vec<u8> = input.bytes().collect();
    for chunk in chars.chunks(4) {
        let mut buf = [0u8; 4];
        let mut valid = 0;
        for (i, &c) in chunk.iter().enumerate() {
            buf[i] = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => { continue; }
                _ => return None,
            };
            valid = i + 1;
        }
        if valid >= 2 { result.push((buf[0] << 2) | (buf[1] >> 4)); }
        if valid >= 3 { result.push((buf[1] << 4) | (buf[2] >> 2)); }
        if valid >= 4 { result.push((buf[2] << 6) | buf[3]); }
    }
    Some(result)
}

fn write_credentials_with_extra(provider: &str, creds: &OAuthCredentials, account_id: Option<&str>) -> anyhow::Result<()> {
    let path = auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let mut auth: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or(json!({}))
    } else {
        json!({})
    };
    let mut entry = serde_json::to_value(creds)?;
    if let Some(id) = account_id {
        entry["accountId"] = json!(id);
    }
    auth[provider] = entry;
    std::fs::write(&path, serde_json::to_string_pretty(&auth)?)?;
    Ok(())
}

/// Hex encode helper (avoids adding hex crate for this one use).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn parse_callback(request: &str) -> anyhow::Result<(String, String)> {
    parse_callback_at_path(request, "/callback")
}

fn parse_callback_at_path(request: &str, _expected_path: &str) -> anyhow::Result<(String, String)> {
    // Parse "GET /callback?code=XXX&state=YYY HTTP/1.1"
    let path = request
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("GET "))
        .and_then(|l| l.split(' ').next())
        .ok_or_else(|| anyhow::anyhow!("Invalid callback request"))?;

    let url = reqwest::Url::parse(&format!("http://localhost{path}"))?;
    let code = url.query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing authorization code in callback"))?;
    let state = url.query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing state in callback"))?;

    Ok((code, state))
}

fn urlencoding_encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
            String::from(b as char)
        }
        _ => format!("%{b:02X}"),
    }).collect()
}

/// Convert AuthStatus to Vec<ProviderStatus> for HarnessStatus compatibility.
pub fn auth_status_to_provider_statuses(status: &AuthStatus) -> Vec<ProviderStatus> {
    status.providers.iter().map(|p| ProviderStatus {
        name: p.name.clone(),
        authenticated: p.status == ProviderAuthStatus::Authenticated,
        auth_method: p.details.clone(),
        model: None,
    }).collect()
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_generation() {
        let (verifier, challenge) = generate_pkce();
        assert!(!verifier.is_empty());
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);
        // base64url: no +, /, or =
        assert!(!verifier.contains('+'));
        assert!(!verifier.contains('/'));
        assert!(!verifier.contains('='));
    }

    #[test]
    fn parse_callback_request() {
        let request = "GET /callback?code=abc123&state=xyz789 HTTP/1.1\r\nHost: localhost\r\n";
        let (code, state) = parse_callback(request).unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz789");
    }

    #[test]
    fn credentials_expiry() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let expired = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "token".into(),
            refresh: "refresh".into(),
            expires: now_ms - 1000,
        };
        assert!(expired.is_expired());

        let valid = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "token".into(),
            refresh: "refresh".into(),
            expires: now_ms + 3600_000,
        };
        assert!(!valid.is_expired());
    }

    #[test]
    fn urlencoding() {
        assert_eq!(urlencoding_encode("hello world"), "hello%20world");
        assert_eq!(urlencoding_encode("a:b"), "a%3Ab");
    }

    #[tokio::test]
    async fn probe_all_providers_returns_auth_status() {
        let status = probe_all_providers().await;
        // Should always have at least anthropic and openai entries
        assert!(!status.providers.is_empty(), "should have provider entries");
        assert!(status.providers.iter().any(|p| p.name == "anthropic"), "should probe anthropic");
        assert!(status.providers.iter().any(|p| p.name == "openai"), "should probe openai");
    }

    #[test]
    fn auth_status_to_provider_statuses_converts() {
        let status = AuthStatus {
            providers: vec![ProviderInfo {
                name: "anthropic".into(),
                status: ProviderAuthStatus::Authenticated,
                is_oauth: true,
                details: Some("oauth".into()),
            }],
            vault: vec![],
            secrets: vec![],
            mcp: vec![],
        };
        let converted = auth_status_to_provider_statuses(&status);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "anthropic");
        assert!(converted[0].authenticated);
    }
}
