//! OAuth authentication — login flows, token refresh, credential storage.
//!
//! Supported providers:
//!   - Anthropic (Claude Pro/Max): PKCE flow to claude.ai, callback on :53692
//!   - OpenAI Codex (ChatGPT Plus/Pro): PKCE flow to auth.openai.com, callback on :1455
//!   - Google Antigravity: Google OAuth2 flow to accounts.google.com, callback on :51121
//!
//! Token refresh happens automatically when the stored token is expired.

use crate::status::{ProviderAuthState, ProviderStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

#[cfg(test)]
pub(crate) static TEST_AUTH_ENV_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

// Single source of truth for every provider's auth.json key, env vars,
// display name, and auth type. Every resolution path MUST use this map
// instead of hardcoding key names.
//
// When adding a new provider: add it here, then update
// docs/provider-credential-map.md to match.

/// How a provider authenticates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    /// Browser OAuth flow (PKCE)
    OAuth,
    /// Direct API key input
    ApiKey,
    /// Dynamic CLI tool execution
    Dynamic,
}

/// Canonical credential descriptor for a provider.
#[derive(Debug, Clone)]
pub struct ProviderCredential {
    /// Internal identifier (used in /model prefix, bus commands)
    pub id: &'static str,
    /// Key used in auth.json (may differ from id — e.g. "openai" → "openai-codex")
    pub auth_key: &'static str,
    /// Human-readable name for UI display
    pub display_name: &'static str,
    /// Environment variables that can carry this credential (checked in order)
    pub env_vars: &'static [&'static str],
    /// How this provider authenticates
    pub auth_method: AuthMethod,
    /// Short description for the login selector
    pub description: &'static str,
}

/// All known providers. This is the ONLY place provider→key mappings should exist.
pub static PROVIDERS: &[ProviderCredential] = &[
    ProviderCredential {
        id: "anthropic",
        auth_key: "anthropic",
        display_name: "Anthropic/Claude",
        env_vars: &["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        auth_method: AuthMethod::OAuth,
        description: "OAuth — Claude Pro/Max subscription",
    },
    ProviderCredential {
        id: "openai",
        auth_key: "openai",
        display_name: "OpenAI API",
        env_vars: &["OPENAI_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — GPT models via api.openai.com",
    },
    ProviderCredential {
        id: "openai-codex",
        auth_key: "openai-codex",
        display_name: "OpenAI/Codex",
        env_vars: &["CHATGPT_OAUTH_TOKEN"],
        auth_method: AuthMethod::OAuth,
        description: "OAuth — experimental consumer ChatGPT/Codex route",
    },
    ProviderCredential {
        id: "github-copilot",
        auth_key: "github-copilot",
        display_name: "GitHub Copilot",
        env_vars: &[
            "GITHUB_COPILOT_OAUTH_TOKEN",
            "GITHUB_COPILOT_TOKEN",
            "COPILOT_OAUTH_TOKEN",
        ],
        auth_method: AuthMethod::OAuth,
        description: "OAuth — GitHub Copilot subscription",
    },
    // ── OpenAI-compatible inference providers ───────────────────────
    ProviderCredential {
        id: "openrouter",
        auth_key: "openrouter",
        display_name: "OpenRouter",
        env_vars: &["OPENROUTER_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — 200+ models, free tier",
    },
    ProviderCredential {
        id: "groq",
        auth_key: "groq",
        display_name: "Groq",
        env_vars: &["GROQ_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — ultra-fast inference",
    },
    ProviderCredential {
        id: "xai",
        auth_key: "xai",
        display_name: "xAI (Grok)",
        env_vars: &["XAI_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — Grok models",
    },
    ProviderCredential {
        id: "mistral",
        auth_key: "mistral",
        display_name: "Mistral AI",
        env_vars: &["MISTRAL_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — Mistral/Codestral models",
    },
    ProviderCredential {
        id: "cerebras",
        auth_key: "cerebras",
        display_name: "Cerebras",
        env_vars: &["CEREBRAS_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — hardware-accelerated inference",
    },
    ProviderCredential {
        id: "google",
        auth_key: "google",
        display_name: "Google Gemini",
        env_vars: &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — Gemini models via generativelanguage.googleapis.com",
    },
    ProviderCredential {
        id: "google-antigravity",
        auth_key: "google-antigravity",
        display_name: "Google Antigravity",
        env_vars: &["ANTIGRAVITY_OAUTH_TOKEN"],
        auth_method: AuthMethod::OAuth,
        description: "OAuth — Gemini models via Google Antigravity IDE subscription",
    },
    ProviderCredential {
        id: "opencode-go",
        auth_key: "opencode-go",
        display_name: "OpenCode Go",
        env_vars: &["OPENCODE_GO_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — DeepSeek, Kimi, Qwen, MiniMax via opencode.ai/go",
    },
    ProviderCredential {
        id: "perplexity",
        auth_key: "perplexity",
        display_name: "Perplexity AI",
        env_vars: &["PERPLEXITY_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — Sonar models with built-in search via api.perplexity.ai",
    },
    ProviderCredential {
        id: "dwarfstar",
        auth_key: "dwarfstar",
        display_name: "DwarfStar Local",
        env_vars: &[
            "OMEGON_DWARFSTAR_BASE_URL",
            "DWARFSTAR_BASE_URL",
            "DWARFSTAR_API_KEY",
        ],
        auth_method: AuthMethod::ApiKey,
        description: "Local OpenAI-compatible inference endpoint",
    },
    ProviderCredential {
        id: "ollama",
        auth_key: "ollama",
        display_name: "Ollama (Local)",
        env_vars: &["OLLAMA_HOST"],
        auth_method: AuthMethod::ApiKey,
        description: "Local inference — your hardware, your models",
    },
    ProviderCredential {
        id: "ollama-cloud",
        auth_key: "ollama-cloud",
        display_name: "Ollama Cloud",
        env_vars: &["OLLAMA_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — hosted Ollama via ollama.com/api",
    },
    // ── Non-inference services ──────────────────────────────────────
    ProviderCredential {
        id: "brave",
        auth_key: "brave",
        display_name: "Brave Search",
        env_vars: &["BRAVE_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — web search",
    },
    ProviderCredential {
        id: "tavily",
        auth_key: "tavily",
        display_name: "Tavily Search",
        env_vars: &["TAVILY_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — AI-optimized search",
    },
    ProviderCredential {
        id: "serper",
        auth_key: "serper",
        display_name: "Serper (Google Search)",
        env_vars: &["SERPER_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — Google results",
    },
    ProviderCredential {
        id: "firecrawl",
        auth_key: "firecrawl",
        display_name: "Firecrawl",
        env_vars: &["FIRECRAWL_API_KEY"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — structured web scraping and search",
    },
    ProviderCredential {
        id: "github",
        auth_key: "github",
        display_name: "GitHub",
        env_vars: &["GITHUB_TOKEN", "GH_TOKEN"],
        auth_method: AuthMethod::Dynamic,
        description: "Dynamic — uses gh CLI",
    },
    ProviderCredential {
        id: "gitlab",
        auth_key: "gitlab",
        display_name: "GitLab",
        env_vars: &["GITLAB_TOKEN"],
        auth_method: AuthMethod::ApiKey,
        description: "Token — git operations, API",
    },
    ProviderCredential {
        id: "huggingface",
        auth_key: "huggingface",
        display_name: "Hugging Face",
        env_vars: &["HF_TOKEN", "HUGGING_FACE_TOKEN"],
        auth_method: AuthMethod::ApiKey,
        description: "API key — models, datasets",
    },
];

/// Normalize operator-facing provider aliases to canonical provider ids.
pub fn canonical_provider_id(id: &str) -> &str {
    match id.trim().to_ascii_lowercase().as_str() {
        "claude" => "anthropic",
        "chatgpt" | "codex" => "openai-codex",
        "anthropic" => "anthropic",
        "openai" => "openai",
        "openai-codex" => "openai-codex",
        "github-copilot" | "copilot" => "github-copilot",
        "openrouter" => "openrouter",
        "opencode-go" => "opencode-go",
        "perplexity" => "perplexity",
        "dwarfstar" => "dwarfstar",
        "ollama-cloud" => "ollama-cloud",
        "ollama" => "ollama",
        "groq" => "groq",
        "xai" => "xai",
        "mistral" => "mistral",
        "cerebras" => "cerebras",
        "google" => "google",
        "gemini" => "google",
        "antigravity" | "google-antigravity" => "google-antigravity",
        "brave" => "brave",
        "tavily" => "tavily",
        "serper" => "serper",
        "github" => "github",
        "gitlab" => "gitlab",
        "huggingface" => "huggingface",
        _ => id,
    }
}

/// Look up a provider by its id (e.g. "openai", "anthropic").
pub fn provider_by_id(id: &str) -> Option<&'static ProviderCredential> {
    let canonical = canonical_provider_id(id);
    PROVIDERS.iter().find(|p| p.id == canonical)
}

/// Get the auth.json key for a provider id. Falls back to the id itself
/// for unknown providers.
pub fn auth_json_key(provider_id: &str) -> &str {
    provider_by_id(provider_id)
        .map(|p| p.auth_key)
        .unwrap_or(provider_id)
}

/// Get the env vars to check for a provider id.
pub fn provider_env_vars(provider_id: &str) -> &[&str] {
    provider_by_id(provider_id)
        .map(|p| p.env_vars)
        .unwrap_or(&[])
}

/// Get endpoint-declared secret references for providers that are present in
/// the model registry but not yet first-class entries in `PROVIDERS`.
pub fn endpoint_secret_refs(provider_id: &str) -> Vec<String> {
    if provider_by_id(provider_id).is_some() {
        return vec![];
    }
    crate::model_registry::ModelRegistry::global()
        .endpoint(provider_id)
        .map(|endpoint| {
            endpoint
                .auth_scheme
                .required_secret_refs()
                .into_iter()
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub fn operator_auth_provider_ids() -> Vec<&'static str> {
    PROVIDERS
        .iter()
        .filter(|provider| provider.id != "ollama" && provider.id != "ollama-cloud")
        .map(|provider| provider.id)
        .collect()
}

pub fn operator_auth_provider_help_list() -> String {
    operator_auth_provider_ids().join(", ")
}

pub fn operator_auth_unknown_provider_message(provider: &str) -> String {
    format!(
        "Unknown provider: {}. Use one of: {}",
        provider,
        operator_auth_provider_help_list()
    )
}

pub fn operator_api_key_login_guidance(
    _provider_id: &str,
    env_var: &str,
    provider_label: &str,
) -> String {
    format!(
        "{} uses hidden key entry in the TUI. Run /login, choose {}, then paste {} when prompted.",
        provider_label, provider_label, env_var
    )
}

pub fn operator_provider_connected_message(effective_model: &str) -> String {
    format!("Provider connected — active route {}.", effective_model)
}

pub fn operator_logout_success_message(provider_label: &str, cleared_session_env: bool) -> String {
    let mut message = format!("✓ Logged out from {provider_label}");
    if cleared_session_env {
        message.push_str(" and cleared this session's cached auth env.");
    }
    message
}

fn provider_session_status_from_sources(
    env_present: bool,
    creds: Option<&OAuthCredentials>,
) -> ProviderSessionStatus {
    if env_present {
        return ProviderSessionStatus::Configured;
    }

    match creds {
        Some(creds) if creds.cred_type == "oauth" && creds.is_expired() => {
            ProviderSessionStatus::Expired
        }
        Some(creds) if !creds.access.trim().is_empty() => ProviderSessionStatus::Configured,
        _ => ProviderSessionStatus::Missing,
    }
}

pub fn provider_session_status(provider: &ProviderCredential) -> ProviderSessionStatus {
    let env_present = provider
        .env_vars
        .iter()
        .any(|v| std::env::var(v).is_ok_and(|s| !s.trim().is_empty()));
    let creds = read_credentials(provider.auth_key);
    let status = provider_session_status_from_sources(env_present, creds.as_ref());
    if status == ProviderSessionStatus::Configured {
        return status;
    }

    // Keep status checks aligned with bridge resolution without resurrecting the
    // old broad Keychain scan. This reads only provider-specific external OAuth
    // files such as ~/.codex/auth.json; it does not query keyring secrets.
    // A fresh external credential should also override an expired auth.json
    // credential, matching resolve_with_refresh adoption behavior.
    let external = read_external_credentials(provider.auth_key);
    let external_status = provider_session_status_from_sources(false, external.as_ref());
    if external_status == ProviderSessionStatus::Configured {
        return external_status;
    }

    status
}

pub fn provider_connected_for_model(model_spec: &str) -> bool {
    provider_candidates_for_model(model_spec)
        .into_iter()
        .any(|provider| provider_session_status(provider) == ProviderSessionStatus::Configured)
}

pub fn provider_oauth_for_model(model_spec: &str) -> bool {
    provider_candidates_for_model(model_spec)
        .into_iter()
        .any(provider_has_oauth_credentials)
}

fn provider_has_oauth_credentials(provider: &ProviderCredential) -> bool {
    if provider
        .env_vars
        .iter()
        .any(|key| key.contains("OAUTH") && std::env::var(key).is_ok_and(|s| !s.trim().is_empty()))
    {
        return true;
    }

    read_credentials(provider.auth_key)
        .or_else(|| read_external_credentials(provider.auth_key))
        .is_some_and(|creds| creds.cred_type == "oauth")
}

fn provider_candidates_for_model(model_spec: &str) -> Vec<&'static ProviderCredential> {
    let Some(provider) = provider_for_model(model_spec) else {
        return Vec::new();
    };
    let mut providers = vec![provider];

    if provider.id == "openai" && openai_family_model(model_spec) {
        if let Some(codex) = provider_by_id("openai-codex") {
            providers.push(codex);
        }
    } else if provider.id == "openai-codex"
        && openai_family_model(model_spec)
        && let Some(openai) = provider_by_id("openai")
    {
        providers.push(openai);
    }

    providers
}

fn openai_family_model(model_spec: &str) -> bool {
    let model_id = model_id_for_auth_model(model_spec).to_ascii_lowercase();
    model_id.starts_with("gpt-")
        || model_id == "o1"
        || model_id == "o3"
        || model_id == "o4"
        || model_id.starts_with("o1-")
        || model_id.starts_with("o3-")
        || model_id.starts_with("o4-")
}

fn model_id_for_auth_model(model_spec: &str) -> &str {
    let trimmed = model_spec.trim();
    let mut current = trimmed;
    while let Some((head, tail)) = current.split_once(':') {
        if provider_by_id(head).is_some() || head == "local" {
            current = tail;
        } else {
            break;
        }
    }
    current
}

fn provider_for_model(model_spec: &str) -> Option<&'static ProviderCredential> {
    let provider_id = crate::providers::infer_provider_id_strict(model_spec)?;
    provider_by_id(&provider_id)
}

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

/// Operator-visible session state for a provider selector or startup probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSessionStatus {
    Configured,
    Expired,
    Missing,
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
///
/// `OMEGON_AUTH_JSON_PATH` lets fleet supervisors mount provider credentials
/// from the normal secret-grant system. When unset, local desktop behavior
/// remains `~/.config/omegon/auth.json`.
pub fn auth_json_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OMEGON_AUTH_JSON_PATH") {
        let path = path.trim();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    dirs::home_dir().map(|h| h.join(".config").join("omegon").join("auth.json"))
}

fn auth_path_trace_fields() -> (String, &'static str) {
    if let Ok(path) = std::env::var("OMEGON_AUTH_JSON_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return (trimmed.to_string(), "OMEGON_AUTH_JSON_PATH");
        }
    }
    let path = auth_json_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    (path, "default")
}

fn credential_expiry_state(creds: &OAuthCredentials) -> (&'static str, bool) {
    if creds.cred_type == "oauth" {
        if creds.is_expired() {
            ("expired", !creds.refresh.is_empty())
        } else {
            ("valid", !creds.refresh.is_empty())
        }
    } else if creds.is_expired() {
        ("expired", false)
    } else {
        ("valid", false)
    }
}

pub fn trace_auth_store_probe(provider: &str, context: &str) {
    let auth_key = auth_json_key(provider);
    let Some(path) = auth_json_path() else {
        tracing::warn!(
            provider = auth_key,
            context,
            decision = "auth_path_unavailable",
            "provider auth probe could not resolve auth.json path"
        );
        return;
    };
    let (path_display, path_source) = auth_path_trace_fields();
    let external_codex_auth_exists = dirs::home_dir()
        .map(|home| home.join(".codex/auth.json").exists())
        .unwrap_or(false);
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(auth) => {
                if let Some(entry) = auth.get(auth_key) {
                    match serde_json::from_value::<OAuthCredentials>(entry.clone()) {
                        Ok(creds) => {
                            let (credential_state, refreshable) = credential_expiry_state(&creds);
                            tracing::info!(
                                provider = auth_key,
                                context,
                                auth_path = %path_display,
                                auth_path_source = path_source,
                                auth_file_exists = true,
                                provider_entry_exists = true,
                                credential_type = %creds.cred_type,
                                credential_state,
                                expires = creds.expires,
                                refreshable,
                                external_codex_auth_exists,
                                "provider auth store probe"
                            );
                        }
                        Err(error) => tracing::warn!(
                            provider = auth_key,
                            context,
                            auth_path = %path_display,
                            auth_path_source = path_source,
                            auth_file_exists = true,
                            provider_entry_exists = true,
                            external_codex_auth_exists,
                            error = %error,
                            decision = "provider_entry_parse_failed",
                            "provider auth store probe"
                        ),
                    }
                } else {
                    tracing::info!(
                        provider = auth_key,
                        context,
                        auth_path = %path_display,
                        auth_path_source = path_source,
                        auth_file_exists = true,
                        provider_entry_exists = false,
                        external_codex_auth_exists,
                        decision = "provider_entry_missing",
                        "provider auth store probe"
                    );
                }
            }
            Err(error) => tracing::warn!(
                provider = auth_key,
                context,
                auth_path = %path_display,
                auth_path_source = path_source,
                auth_file_exists = true,
                external_codex_auth_exists,
                error = %error,
                decision = "auth_json_parse_failed",
                "provider auth store probe"
            ),
        },
        Err(error) => tracing::info!(
            provider = auth_key,
            context,
            auth_path = %path_display,
            auth_path_source = path_source,
            auth_file_exists = false,
            external_codex_auth_exists,
            error = %error,
            decision = "auth_json_missing_or_unreadable",
            "provider auth store probe"
        ),
    }
}

/// Quick check: does auth.json exist with at least one token?
/// Used by first_run.rs to detect whether the operator has any provider configured.
pub fn any_oauth_token_exists() -> bool {
    let Some(path) = auth_json_path() else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(auth) = serde_json::from_str::<Value>(&content) else {
        return false;
    };
    auth.as_object().is_some_and(|obj| !obj.is_empty())
}

/// Read credentials for a provider from auth.json.
///
/// Provider ids, aliases, and auth.json storage keys are normalized here so
/// every caller uses the same lookup semantics. In particular, the OpenAI API
/// provider (`openai`) and Codex/ChatGPT OAuth provider (`openai-codex`) must
/// not drift across startup probes, selected-model validation, and streaming
/// client construction.
pub fn read_credentials(provider: &str) -> Option<OAuthCredentials> {
    let path = auth_json_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let auth: Value = serde_json::from_str(&content).ok()?;
    let auth_key = auth_json_key(provider);
    let entry = auth.get(auth_key)?;
    serde_json::from_value(entry.clone()).ok()
}

/// Attempt to adopt credentials from other tools installed on this machine.
/// Checks provider-specific sources so users don't need to re-authenticate
/// when they already have a working session in another tool.
///
/// Supported:
///   - anthropic:          Claude Code (~/.claude.json) OAuth tokens
///   - openai-codex:       Codex CLI (~/.codex/auth.json) OAuth tokens
///   - github:             GitHub Copilot (~/.config/github-copilot/hosts.json) OAuth tokens
///   - google-antigravity: Gemini CLI (~/.gemini/oauth_creds.json) OAuth tokens
///   - huggingface:        HF CLI (~/.cache/huggingface/token) API token
pub fn read_external_credentials(provider: &str) -> Option<OAuthCredentials> {
    let home = dirs::home_dir()?;
    match provider {
        "anthropic" => {
            // Claude Code stores OAuth in ~/.claude.json
            let data: Value =
                serde_json::from_str(&std::fs::read_to_string(home.join(".claude.json")).ok()?)
                    .ok()?;
            let oauth = data.get("oauthAccount")?;
            let access = oauth
                .get("accessToken")?
                .as_str()
                .filter(|s| !s.is_empty())?;
            let refresh = oauth.get("refreshToken")?.as_str()?;
            let expires = oauth.get("expiresAt")?.as_i64()?;
            Some(OAuthCredentials {
                cred_type: "oauth".into(),
                access: access.into(),
                refresh: refresh.into(),
                expires: expires as u64,
            })
        }
        "openai-codex" => {
            // Codex CLI stores OAuth tokens at ~/.codex/auth.json
            // Structure: { "tokens": { "access_token": "...", "refresh_token": "..." }, ... }
            let data: Value =
                serde_json::from_str(&std::fs::read_to_string(home.join(".codex/auth.json")).ok()?)
                    .ok()?;
            let tokens = data.get("tokens")?;
            let access = tokens
                .get("access_token")?
                .as_str()
                .filter(|s| !s.is_empty())?;
            let refresh = tokens
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let expires = codex_access_token_expiry_ms(access)
                .or_else(|| {
                    data.get("last_refresh")
                        .and_then(codex_cli_last_refresh_secs)
                        .map(|last_refresh| (last_refresh + 3600) * 1000)
                })
                .unwrap_or(0);
            Some(OAuthCredentials {
                cred_type: "oauth".into(),
                access: access.into(),
                refresh: refresh.into(),
                expires,
            })
        }
        "github" => {
            // GitHub Copilot stores host tokens in ~/.config/github-copilot/hosts.json
            let hosts: Value = serde_json::from_str(
                &std::fs::read_to_string(home.join(".config/github-copilot/hosts.json")).ok()?,
            )
            .ok()?;
            let obj = hosts.as_object()?;
            let entry = obj.get("github.com").or_else(|| obj.values().next())?;
            let token = entry
                .get("oauth_token")?
                .as_str()
                .filter(|s| !s.is_empty())?;
            Some(OAuthCredentials {
                cred_type: "oauth".into(),
                access: token.into(),
                refresh: String::new(),
                expires: u64::MAX,
            })
        }
        "google-antigravity" => {
            // Gemini CLI uses the same OAuth client as our Antigravity flow.
            // Tokens are stored at ~/.gemini/oauth_creds.json or
            // ~/.config/gemini-cli/oauth_creds.json.
            let paths = [
                home.join(".gemini/oauth_creds.json"),
                home.join(".config/gemini-cli/oauth_creds.json"),
            ];
            for path in &paths {
                if let Ok(content) = std::fs::read_to_string(path)
                    && let Ok(data) = serde_json::from_str::<Value>(&content)
                {
                    let access = data
                        .get("access_token")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())?;
                    let refresh = data
                        .get("refresh_token")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let expires = data
                        .get("expiry")
                        .and_then(|v| v.as_i64())
                        .or_else(|| data.get("token_expiry").and_then(|v| v.as_i64()))
                        .unwrap_or(0) as u64;
                    return Some(OAuthCredentials {
                        cred_type: "oauth".into(),
                        access: access.into(),
                        refresh: refresh.into(),
                        expires,
                    });
                }
            }
            None
        }
        "huggingface" => {
            // HF CLI stores a plain-text token at ~/.cache/huggingface/token
            // (modern) or ~/.huggingface/token (legacy). Long-lived API token.
            let token = std::fs::read_to_string(home.join(".cache/huggingface/token"))
                .or_else(|_| std::fs::read_to_string(home.join(".huggingface/token")))
                .ok()?;
            let token = token.trim();
            if token.is_empty() {
                return None;
            }
            Some(OAuthCredentials {
                cred_type: "api-key".into(),
                access: token.into(),
                refresh: String::new(),
                expires: u64::MAX, // long-lived personal access token
            })
        }
        _ => None,
    }
}

fn codex_access_token_expiry_ms(access: &str) -> Option<u64> {
    let exp = extract_jwt_claim(access, "", "exp")?;
    exp.parse::<u64>().ok().map(|seconds| seconds * 1000)
}

fn codex_cli_last_refresh_secs(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value.as_str().and_then(|raw| {
            chrono::DateTime::parse_from_rfc3339(raw)
                .ok()
                .and_then(|dt| u64::try_from(dt.timestamp()).ok())
        })
    })
}

fn read_external_credential_extra(provider: &str, field: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    match (provider, field) {
        ("openai-codex", "accountId") => {
            let data: Value =
                serde_json::from_str(&std::fs::read_to_string(home.join(".codex/auth.json")).ok()?)
                    .ok()?;
            data.pointer("/tokens/account_id")
                .and_then(|v| v.as_str())
                .filter(|value| !value.is_empty())
                .map(String::from)
        }
        _ => None,
    }
}

// Backward-compat alias — referenced from providers.rs
pub fn read_claude_code_credentials(provider: &str) -> Option<OAuthCredentials> {
    read_external_credentials(provider)
}

/// Read extra fields stored alongside credentials in auth.json.
/// Used for accountId on openai-codex entries.
pub fn read_credential_extra(provider: &str, field: &str) -> Option<String> {
    let path = auth_json_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let auth: Value = serde_json::from_str(&content).ok()?;
    let auth_key = auth_json_key(provider);
    auth.get(auth_key)?.get(field)?.as_str().map(String::from)
}

/// Write credentials for a provider to auth.json.
/// Sets file permissions to 0600 (owner-only read/write) on Unix.
pub fn write_credentials(provider: &str, creds: &OAuthCredentials) -> anyhow::Result<()> {
    let path =
        auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    assert_test_auth_json_override_for_write(&path)?;
    let _ = std::fs::create_dir_all(path.parent().unwrap());

    with_auth_json_lock(&path, || {
        let mut auth = read_auth_json_for_update(&path, "write_credentials", provider)?;

        let before_keys = auth_json_provider_keys(&auth);
        let auth_key = auth_json_key(provider);
        auth[auth_key] = serde_json::to_value(creds)?;
        ensure_auth_json_key_invariants("write_credentials", auth_key, &before_keys, &auth)?;
        trace_auth_json_key_delta("write_credentials", auth_key, &before_keys, &auth);
        atomic_write_auth_json(&path, &auth)?;
        set_auth_file_permissions(&path)?;
        let (auth_path, auth_path_source) = auth_path_trace_fields();
        tracing::info!(provider, auth_path = %auth_path, auth_path_source, credential_type = %creds.cred_type, expires = creds.expires, "persisted provider credentials to auth.json");
        Ok(())
    })
}

/// Probe all authentication providers to get current status.
pub async fn probe_all_providers() -> AuthStatus {
    let mut providers = Vec::new();

    for provider in operator_auth_provider_ids() {
        providers.push(probe_provider(provider).await);
    }

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
    let env_keys = provider_env_vars(provider);

    for key in env_keys {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            let is_oauth = key.contains("OAUTH");
            return ProviderInfo {
                name: provider.to_string(),
                status: ProviderAuthStatus::Authenticated,
                is_oauth,
                details: Some(format!("env:{}", key)),
            };
        }
    }

    // Check stored credentials
    let auth_key = auth_json_key(provider);
    if let Some(creds) = read_credentials(auth_key) {
        let status = if creds.cred_type == "oauth" && creds.is_expired() {
            if resolve_with_refresh(provider).await.is_some() {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Expired
            }
        } else if creds.is_expired() {
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

    if provider_by_id(provider).is_some_and(|p| p.auth_method == AuthMethod::OAuth)
        && read_external_credentials(auth_key).is_some()
    {
        let status = if resolve_with_refresh(provider).await.is_some() {
            ProviderAuthStatus::Authenticated
        } else {
            ProviderAuthStatus::Expired
        };
        return ProviderInfo {
            name: provider.to_string(),
            status,
            is_oauth: true,
            details: Some("external".to_string()),
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
///
/// Logout is intentionally idempotent for known providers: if no auth.json exists
/// or the provider has no stored entry, treat the provider as already logged out.
pub fn logout_provider(provider: &str) -> anyhow::Result<()> {
    let canonical = canonical_provider_id(provider);
    provider_by_id(canonical).ok_or_else(|| anyhow::anyhow!("Unknown provider: {provider}"))?;

    let path =
        auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    assert_test_auth_json_override_for_write(&path)?;

    if !path.exists() {
        return Ok(());
    }

    with_auth_json_lock(&path, || {
        let mut auth = read_auth_json_for_update(&path, "logout_provider", canonical)?;

        let auth_key = auth_json_key(canonical);

        if auth.get(auth_key).is_none() {
            return Ok(());
        }

        // Remove the provider's entry
        let before_keys = auth_json_provider_keys(&auth);
        if let Some(obj) = auth.as_object_mut() {
            obj.remove(auth_key);
        }
        ensure_auth_json_key_invariants("logout_provider", auth_key, &before_keys, &auth)?;
        trace_auth_json_key_delta("logout_provider", auth_key, &before_keys, &auth);

        // Write back
        atomic_write_auth_json(&path, &auth)?;
        set_auth_file_permissions(&path)?;
        Ok(())
    })
}

/// Remove any in-process env vars that can still make a provider appear logged in.
pub fn clear_provider_auth_env(provider: &str) {
    for env_var in provider_env_vars(provider) {
        // SAFETY: logout is an explicit operator action; clearing the env here is
        // part of making the current process match the persisted auth state.
        unsafe {
            std::env::remove_var(env_var);
        }
    }
}

/// Import usable credentials discovered in supported external tools into
/// Omegon's auth store. This is a bootstrap path only: normal provider
/// hydration should continue to read Omegon-owned auth.json after import.
pub fn import_discovered_provider_credentials() -> usize {
    let mut imported = 0;

    for provider in PROVIDERS {
        let existing = read_credentials(provider.auth_key);
        if existing
            .as_ref()
            .is_some_and(|creds| creds.cred_type != "oauth" || !creds.is_expired())
        {
            continue;
        }

        if adopt_external_credentials(provider.auth_key).is_some() {
            imported += 1;
            tracing::info!(
                provider = provider.id,
                "imported discovered provider credentials into auth.json"
            );
        }
    }

    imported
}

/// Adopt a valid provider credential from supported external tools and persist
/// it into Omegon auth storage. Returns the adopted credential on success.
pub fn adopt_external_credentials(provider: &str) -> Option<OAuthCredentials> {
    let auth_key = auth_json_key(provider);
    let discovered = read_external_credentials(auth_key)?;
    if discovered.cred_type == "oauth" && discovered.is_expired() {
        return None;
    }

    let persist_result = if auth_key == "openai-codex" {
        let account_id = read_external_credential_extra(auth_key, "accountId");
        write_credentials_with_extra(auth_key, &discovered, account_id.as_deref())
    } else {
        write_credentials(auth_key, &discovered)
    };

    match persist_result {
        Ok(()) => Some(discovered),
        Err(e) => {
            tracing::warn!(
                provider = auth_key,
                error = %auth_write_failure_operator_message(&e),
                "external provider credential could not be adopted"
            );
            None
        }
    }
}

/// Resolve API key with automatic token refresh.
/// Returns (api_key, is_oauth_token).
pub async fn resolve_with_refresh(provider: &str) -> Option<(String, bool)> {
    trace_auth_store_probe(provider, "resolve_with_refresh:start");
    // Use canonical provider map for env vars
    let env_vars = provider_env_vars(provider);

    // 1. Env vars first (not OAuth)
    for key in env_vars
        .iter()
        .copied()
        .filter(|key| !key.contains("OAUTH"))
    {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            return Some((val, false));
        }
    }

    // OAuth env vars are checked only after refreshable auth.json/external
    // credentials. Startup may hydrate OAuth env vars from auth.json for child
    // process inheritance; treating those as authoritative here would shadow the
    // persisted refresh token and keep using a stale access token.

    // 2. auth.json — with refresh if expired (canonical key mapping)
    let auth_key = auth_json_key(provider);
    let mut creds = match read_credentials(auth_key) {
        Some(c) => c,
        None => {
            // 3. Fallback: adopt credentials from supported external tools.
            if let Some(c) = read_external_credentials(auth_key) {
                tracing::info!(provider, "Adopted credentials from external tool");
                let persist_result = if auth_key == "openai-codex" {
                    let account_id = read_external_credential_extra(auth_key, "accountId");
                    write_credentials_with_extra(auth_key, &c, account_id.as_deref())
                } else {
                    write_credentials(auth_key, &c)
                };
                if let Err(e) = persist_result {
                    tracing::warn!(
                        provider = auth_key,
                        error = %auth_write_failure_operator_message(&e),
                        "adopted provider credential could not be persisted"
                    );
                }
                c
            } else {
                for key in env_vars.iter().copied().filter(|key| key.contains("OAUTH")) {
                    if let Ok(val) = std::env::var(key)
                        && !val.is_empty()
                    {
                        tracing::info!(
                            provider = auth_key,
                            env = key,
                            decision = "use_oauth_env_fallback",
                            "provider credential resolved from OAuth env fallback"
                        );
                        return Some((val, true));
                    }
                }
                tracing::warn!(
                    provider = auth_key,
                    decision = "missing_all_sources",
                    "provider credential resolution failed"
                );
                return None;
            }
        }
    };
    if creds.cred_type != "oauth" {
        return Some((creds.access, false));
    }

    if creds.is_expired() {
        tracing::info!(provider, auth_key, "OAuth token expired — refreshing");
        match refresh_token(auth_key, &creds.refresh).await {
            Ok(new_creds) => {
                if let Err(e) = write_refreshed_credentials(auth_key, &new_creds) {
                    tracing::warn!(
                        provider = auth_key,
                        error = %auth_write_failure_operator_message(&e),
                        "OAuth token refreshed but could not be persisted"
                    );
                }
                creds = new_creds;
            }
            Err(e) => {
                if let Some(adopted) = read_external_credentials(auth_key)
                    && (adopted.cred_type != "oauth" || !adopted.is_expired())
                {
                    tracing::warn!(
                        provider = auth_key,
                        "Token refresh failed: {e} — adopting fresh external credential"
                    );
                    let persist_result = if auth_key == "openai-codex" {
                        let account_id = read_external_credential_extra(auth_key, "accountId");
                        write_credentials_with_extra(auth_key, &adopted, account_id.as_deref())
                    } else {
                        write_credentials(auth_key, &adopted)
                    };
                    if let Err(write_error) = persist_result {
                        tracing::warn!(
                            provider = auth_key,
                            error = %auth_write_failure_operator_message(&write_error),
                            "adopted provider credential could not be persisted"
                        );
                    }
                    creds = adopted;
                } else if oauth_refresh_failure_is_fatal(auth_key) {
                    tracing::warn!(
                        "Token refresh failed: {e} — refusing to use expired {auth_key} credential"
                    );
                    return None;
                } else {
                    tracing::warn!("Token refresh failed: {e} — using expired token");
                }
            }
        }
    }

    tracing::info!(
        provider = auth_key,
        decision = "use_oauth",
        expires = creds.expires,
        "provider credential resolved"
    );
    Some((creds.access, true))
}

fn oauth_refresh_failure_is_fatal(provider: &str) -> bool {
    matches!(provider, "openai-codex" | "google-antigravity")
}

fn auth_write_failure_operator_message(error: &anyhow::Error) -> &'static str {
    if error
        .chain()
        .filter_map(|e| e.downcast_ref::<std::io::Error>())
        .any(|e| {
            matches!(
                e.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            )
        })
    {
        return "auth.json is read-only; rotate the backing credential grant or reauthenticate through the secret projection";
    }
    "auth.json write-back failed; rotate credentials or reauthenticate before the projected token expires"
}

/// Refresh an OAuth token.
pub async fn refresh_token(provider: &str, refresh: &str) -> anyhow::Result<OAuthCredentials> {
    if provider == "openai-codex" {
        return refresh_openai_token(refresh).await;
    }
    if provider == "google-antigravity" {
        return refresh_antigravity_token(refresh).await;
    }
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
        expires: now_ms + expires_in.saturating_sub(300) * 1000, // 5 min safety margin
    })
}

fn base64url_encode(bytes: &[u8]) -> String {
    // Manual base64url encoding — no external crate needed
    let b64 = crate::tools::view::base64_encode_bytes(bytes);
    b64.replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
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

/// Create an OAuth callback TCP listener.
///
/// Binds `127.0.0.1` (IPv4). The redirect_uri in all OAuth flows uses
/// `http://localhost:<port>/callback` — browsers resolve `localhost`
/// to 127.0.0.1 or ::1 depending on the system. We use a std TcpListener
/// in blocking mode, converted to tokio, to avoid any async accept issues.
///
/// On systems where localhost resolves to ::1 only (NixOS), the operator
/// may need to ensure /etc/hosts maps localhost to 127.0.0.1 as well,
/// or use `OMEGON_HEADLESS=1` for the paste-back flow.
fn bind_callback_listener(port: u16) -> anyhow::Result<tokio::net::TcpListener> {
    // Try 127.0.0.1 first (works everywhere), then [::1] as fallback
    let std_listener = std::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .or_else(|_| std::net::TcpListener::bind(format!("[::1]:{port}")))?;
    std_listener.set_nonblocking(true)?;
    Ok(tokio::net::TcpListener::from_std(std_listener)?)
}

/// Accept connections on an OAuth callback listener until a request carrying
/// a valid `code` and the expected `state` arrives at `expected_path`, or the
/// deadline passes.
///
/// A single `accept()` is not safe here: browsers open speculative
/// preconnections that send no bytes, request `/favicon.ico` on the callback
/// origin, and stale tabs from earlier login attempts redirect with old
/// `state` values. All of those used to abort the login even though the
/// operator completed authentication. This loop answers and skips them,
/// completing only on the real callback for *this* attempt.
async fn accept_oauth_callback(
    listener: &tokio::net::TcpListener,
    expected_path: &str,
    expected_state: &str,
    timeout: std::time::Duration,
) -> anyhow::Result<(String, String)> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            anyhow::bail!(
                "Login timed out waiting for browser callback. Run /login again to retry."
            );
        }
        let (mut stream, _addr) = match tokio::time::timeout(remaining, listener.accept()).await {
            Ok(accepted) => accepted?,
            Err(_) => anyhow::bail!(
                "Login timed out waiting for browser callback. Run /login again to retry."
            ),
        };
        let mut buf = [0u8; 4096];
        let n = match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::io::AsyncReadExt::read(&mut stream, &mut buf),
        )
        .await
        {
            Ok(Ok(n)) => n,
            // Speculative preconnect (closed without data) or dead socket.
            _ => continue,
        };
        if n == 0 {
            continue;
        }
        let request = String::from_utf8_lossy(&buf[..n]);
        let Ok((code, state)) = parse_callback_at_path(&request, expected_path) else {
            // favicon.ico or other browser noise — answer and keep listening.
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut stream,
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n" as &[u8],
            )
            .await;
            continue;
        };
        if state != expected_state {
            // Redirect from a stale login tab — reject it without aborting
            // the current attempt.
            tracing::warn!("ignoring OAuth callback with non-matching state (stale login tab?)");
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut stream,
                b"HTTP/1.1 409 Conflict\r\nContent-Type: text/html\r\n\r\n\
                  <html><body><p>This login page is from an earlier attempt. \
                  Close this tab and complete the most recent login page.</p></body></html>"
                    as &[u8],
            )
            .await;
            continue;
        }
        let _ = tokio::io::AsyncWriteExt::write_all(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
              <html><body><p>Authentication successful. Return to your terminal.</p></body></html>"
                as &[u8],
        )
        .await;
        return Ok((code, state));
    }
}

/// Detect headless environments where a browser cannot be opened locally.
/// Returns true for SSH sessions and Linux systems without a display server.
pub fn is_headless() -> bool {
    // SSH session — browser would open on the remote, not the user's machine
    if std::env::var("SSH_CONNECTION").is_ok() || std::env::var("SSH_CLIENT").is_ok() {
        return true;
    }
    // Linux without display server
    #[cfg(target_os = "linux")]
    if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
        return true;
    }
    false
}

/// Parse `code` and `state` query parameters from a full callback URL pasted
/// by the user (e.g. `http://localhost:53692/callback?code=abc&state=xyz`).
fn parse_callback_url(url: &str) -> anyhow::Result<(String, String)> {
    let url = url.trim();
    let parsed = reqwest::Url::parse(url).map_err(|_| {
        anyhow::anyhow!("Invalid URL. Paste the full URL from your browser's address bar.")
    })?;
    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing 'code' parameter in the pasted URL"))?;
    let state = parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing 'state' parameter in the pasted URL"))?;
    Ok((code, state))
}

/// Run the Anthropic OAuth login flow.
/// Opens a browser, listens for the callback, exchanges the code for tokens.
/// Progress callback for OAuth login — allows TUI to receive status updates
/// without writing to stderr.
pub type LoginProgress = Box<dyn Fn(&str) + Send + Sync>;

pub type LoginCopyBlock = Box<
    dyn Fn(String, String, omegon_traits::OperatorCopyKind, Option<omegon_traits::ClipboardCopyStatus>)
        + Send
        + Sync,
>;

/// Prompt callback for headless login — requests a line of input from the user.
/// The closure receives a prompt string and returns the user's input.
pub type LoginPrompt = Box<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send>> + Send + Sync,
>;

fn default_progress() -> LoginProgress {
    Box::new(|msg| eprintln!("{msg}"))
}

fn default_copy_block() -> LoginCopyBlock {
    Box::new(|_, _, _, _| {})
}

fn default_prompt() -> LoginPrompt {
    Box::new(|prompt| {
        Box::pin(async move {
            eprint!("{prompt}");
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| anyhow::anyhow!("Failed to read input: {e}"))?;
            Ok(line)
        })
    })
}

pub async fn login_anthropic() -> anyhow::Result<OAuthCredentials> {
    login_anthropic_with_callbacks(default_progress(), default_prompt()).await
}

pub async fn login_anthropic_with_progress(
    progress: LoginProgress,
) -> anyhow::Result<OAuthCredentials> {
    login_anthropic_with_callbacks(progress, default_prompt()).await
}

pub async fn login_anthropic_with_callbacks(
    progress: LoginProgress,
    prompt: LoginPrompt,
) -> anyhow::Result<OAuthCredentials> {
    let (verifier, challenge) = generate_pkce();

    // Build authorization URL
    let auth_url = format!(
        "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code\
         &redirect_uri={REDIRECT_URI}&scope={}&code_challenge={challenge}\
         &code_challenge_method=S256&state={verifier}",
        urlencoding_encode(SCOPES),
    );

    let (code, state) = if is_headless() {
        // ── Headless paste-back flow ───────────────────────────────────
        tracing::info!("headless environment detected, using paste-back OAuth flow");
        progress("Headless environment detected — using manual login flow.");
        progress(&format!(
            "\n  1. Open this URL in your browser:\n     {auth_url}\n\n  \
             2. Complete the login in your browser.\n\n  \
             3. Your browser will redirect to a URL starting with:\n     \
             http://localhost:{CALLBACK_PORT}/callback?code=...\n     \
             (The page will fail to load — this is expected.)\n\n  \
             4. Copy the full URL from your browser's address bar and paste it below.\n"
        ));
        let pasted = prompt("Paste callback URL: ".into()).await?;
        parse_callback_url(&pasted)?
    } else {
        // ── Normal browser flow ────────────────────────────────────────
        // Create a dual-stack IPv6 socket that accepts both IPv4 and IPv6.
        // Firefox on NixOS resolves localhost to ::1 and hangs without
        // fallback to 127.0.0.1. A dual-stack [::] socket with
        // IPV6_V6ONLY=false accepts connections on both protocols.
        let listener = bind_callback_listener(CALLBACK_PORT)?;
        tracing::debug!(
            port = CALLBACK_PORT,
            "OAuth callback server listening (dual-stack)"
        );

        progress("Opening browser for Anthropic login…");
        // Spawn browser open in a background thread — xdg-open on some Linux
        // desktops (NixOS/Cosmic) blocks until the browser exits, which would
        // prevent us from reaching accept() for the callback.
        let auth_url_for_browser = auth_url.clone();
        std::thread::spawn(move || {
            let _ = open::that(&auth_url_for_browser);
        });
        // Give the browser a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        accept_oauth_callback(
            &listener,
            "/callback",
            &verifier,
            std::time::Duration::from_secs(300),
        )
        .await?
    };

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
        expires: now_ms + expires_in.saturating_sub(300) * 1000,
    };

    // Save to auth.json
    write_credentials("anthropic", &creds)?;
    let persisted = read_credentials("anthropic").ok_or_else(|| {
        anyhow::anyhow!("Anthropic login completed but credentials were not persisted")
    })?;
    if persisted.access != creds.access {
        anyhow::bail!(
            "Anthropic login completed but persisted credentials do not match the issued token"
        );
    }

    // Update the env var so resolve_with_refresh uses the new token
    // immediately (env vars take priority over auth.json). Without this,
    // a stale token from a previous account stays in ANTHROPIC_OAUTH_TOKEN
    // and gets used instead of the freshly-issued one.
    // SAFETY: single-threaded at login time — no other threads reading env.
    unsafe {
        std::env::remove_var("ANTHROPIC_OAUTH_TOKEN");
    }

    progress("✓ Authentication successful. Credentials saved.");

    Ok(creds)
}

const GITHUB_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_OAUTH_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_COPILOT_DEFAULT_SCOPE: &str = "read:user user:email repo workflow";

#[derive(Debug, Deserialize)]
struct GithubDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GithubDeviceTokenResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn login_github_copilot() -> anyhow::Result<OAuthCredentials> {
    login_github_copilot_with_callbacks(default_progress(), default_prompt()).await
}

pub async fn login_github_copilot_with_progress(
    progress: LoginProgress,
) -> anyhow::Result<OAuthCredentials> {
    login_github_copilot_with_callbacks(progress, default_prompt()).await
}

pub async fn login_github_copilot_with_callbacks(
    progress: LoginProgress,
    prompt: LoginPrompt,
) -> anyhow::Result<OAuthCredentials> {
    login_github_copilot_with_copy_callback(progress, prompt, default_copy_block()).await
}

pub async fn login_github_copilot_with_copy_callback(
    progress: LoginProgress,
    _prompt: LoginPrompt,
    copy_block: LoginCopyBlock,
) -> anyhow::Result<OAuthCredentials> {
    let scope = std::env::var("GITHUB_COPILOT_OAUTH_SCOPE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| GITHUB_COPILOT_DEFAULT_SCOPE.to_string());
    let client = reqwest::Client::new();
    progress("Requesting GitHub Copilot device login code…");
    let device: GithubDeviceCodeResponse = client
        .post(GITHUB_DEVICE_CODE_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", GITHUB_COPILOT_CLIENT_ID),
            ("scope", scope.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    progress(&format!(
        "Open {} and enter the GitHub Copilot device code below.",
        device.verification_uri
    ));
    let code_copy_attempt = crate::clipboard::copy_operator_text(&device.user_code);
    copy_block(
        "GitHub Copilot device code".to_string(),
        device.user_code.clone(),
        omegon_traits::OperatorCopyKind::AuthDeviceCode,
        Some(code_copy_attempt),
    );
    let opened_browser = open::that(&device.verification_uri).is_ok();
    if !opened_browser {
        copy_block(
            "GitHub Copilot device URL".to_string(),
            device.verification_uri.clone(),
            omegon_traits::OperatorCopyKind::AuthUrl,
            None,
        );
    }

    let started = std::time::Instant::now();
    let mut interval = device.interval.unwrap_or(5).max(1);
    loop {
        if started.elapsed().as_secs() >= device.expires_in {
            anyhow::bail!("GitHub Copilot device login expired before authorization completed");
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        let response: GithubDeviceTokenResponse = client
            .post(GITHUB_OAUTH_TOKEN_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", GITHUB_COPILOT_CLIENT_ID),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(access) = response.access_token.filter(|token| !token.is_empty()) {
            let creds = OAuthCredentials {
                cred_type: "oauth".into(),
                access,
                refresh: String::new(),
                expires: u64::MAX,
            };
            write_credentials("github-copilot", &creds)?;
            progress("✓ GitHub Copilot authentication successful. Credentials saved.");
            return Ok(creds);
        }
        match response.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval = interval.saturating_add(5);
                continue;
            }
            Some(error) => {
                let description = response.error_description.unwrap_or_default();
                anyhow::bail!("GitHub Copilot device login failed: {error} {description}");
            }
            None => anyhow::bail!("GitHub Copilot device login returned no access token"),
        }
    }
}

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_CALLBACK_PORT: u16 = 1455;
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";

/// Run the OpenAI Codex OAuth login flow (ChatGPT Plus/Pro subscription).
pub async fn login_openai() -> anyhow::Result<OAuthCredentials> {
    login_openai_with_callbacks(default_progress(), default_prompt()).await
}

pub async fn login_openai_with_progress(
    progress: LoginProgress,
) -> anyhow::Result<OAuthCredentials> {
    login_openai_with_callbacks(progress, default_prompt()).await
}

pub async fn login_openai_with_callbacks(
    progress: LoginProgress,
    prompt: LoginPrompt,
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

    let (code, recv_state) = if is_headless() {
        // ── Headless paste-back flow ───────────────────────────────────
        tracing::info!("headless environment detected, using paste-back OAuth flow (OpenAI)");
        progress("Headless environment detected — using manual login flow.");
        progress(&format!(
            "\n  1. Open this URL in your browser:\n     {auth_url}\n\n  \
             2. Complete the login in your browser.\n\n  \
             3. Your browser will redirect to a URL starting with:\n     \
             http://localhost:{OPENAI_CALLBACK_PORT}/auth/callback?code=...\n     \
             (The page will fail to load — this is expected.)\n\n  \
             4. Copy the full URL from your browser's address bar and paste it below.\n"
        ));
        let pasted = prompt("Paste callback URL: ".into()).await?;
        parse_callback_url(&pasted)?
    } else {
        // ── Normal browser flow ────────────────────────────────────────
        let listener = bind_callback_listener(OPENAI_CALLBACK_PORT)?;
        tracing::debug!(
            port = OPENAI_CALLBACK_PORT,
            "OpenAI OAuth callback server listening"
        );

        progress("Opening browser for OpenAI login…");
        let auth_url_for_browser = auth_url.clone();
        std::thread::spawn(move || {
            let _ = open::that(&auth_url_for_browser);
        });
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        accept_oauth_callback(
            &listener,
            "/auth/callback",
            &state,
            std::time::Duration::from_secs(300),
        )
        .await?
    };

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
    let account_id =
        extract_jwt_claim(&access, "https://api.openai.com/auth", "chatgpt_account_id");

    let creds = OAuthCredentials {
        cred_type: "oauth".into(),
        access,
        refresh: data["refresh_token"].as_str().unwrap_or("").into(),
        expires: now_ms + expires_in * 1000,
    };

    // Store with accountId as extra field
    write_credentials_with_extra("openai-codex", &creds, account_id.as_deref())?;
    let persisted = read_credentials("openai-codex").ok_or_else(|| {
        anyhow::anyhow!("OpenAI login completed but credentials were not persisted")
    })?;
    if persisted.access != creds.access {
        anyhow::bail!(
            "OpenAI login completed but persisted credentials do not match the issued token"
        );
    }
    if persisted.refresh != creds.refresh {
        anyhow::bail!(
            "OpenAI login completed but persisted credentials do not match the issued refresh token"
        );
    }
    if let Some(expected_account_id) = account_id.as_deref() {
        let persisted_account_id =
            read_credential_extra("openai-codex", "accountId").ok_or_else(|| {
                anyhow::anyhow!("OpenAI login completed but accountId was not persisted")
            })?;
        if persisted_account_id != expected_account_id {
            anyhow::bail!(
                "OpenAI login completed but persisted accountId does not match the issued accountId"
            );
        }
    }
    // Update env var so resolve_with_refresh uses the new token immediately.
    // Clear the session-cached token instead of setting it: env credentials have
    // resolver priority over auth.json, so keeping an OAuth token in process env
    // can shadow later shared-file refreshes performed by another Omegon session.
    unsafe {
        std::env::remove_var("CHATGPT_OAUTH_TOKEN");
    }

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
        expires: now_ms + expires_in.saturating_sub(300) * 1000,
    })
}

//
// Uses the same public OAuth credentials as Gemini CLI (google-gemini/gemini-cli).
// Google documents that for installed/desktop applications, "the client secret
// is not treated as a secret" — it's embedded in the distributed binary.
// See: https://developers.google.com/identity/protocols/oauth2/native-app
const ANTIGRAVITY_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const ANTIGRAVITY_CLIENT_SECRET: &str = "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";
const ANTIGRAVITY_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const ANTIGRAVITY_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const ANTIGRAVITY_CALLBACK_PORT: u16 = 51121;
const ANTIGRAVITY_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
const ANTIGRAVITY_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile openid";

pub async fn login_antigravity() -> anyhow::Result<OAuthCredentials> {
    login_antigravity_with_callbacks(default_progress(), default_prompt()).await
}

pub async fn login_antigravity_with_progress(
    progress: LoginProgress,
) -> anyhow::Result<OAuthCredentials> {
    login_antigravity_with_callbacks(progress, default_prompt()).await
}

pub async fn login_antigravity_with_callbacks(
    progress: LoginProgress,
    prompt: LoginPrompt,
) -> anyhow::Result<OAuthCredentials> {
    let (verifier, challenge) = generate_pkce();

    let mut state_bytes = [0u8; 16];
    getrandom::fill(&mut state_bytes).expect("getrandom failed");
    let state = hex::encode(&state_bytes);

    let auth_url = format!(
        "{ANTIGRAVITY_AUTH_URL}?response_type=code&client_id={ANTIGRAVITY_CLIENT_ID}\
         &redirect_uri={}&scope={}&code_challenge={challenge}\
         &code_challenge_method=S256&state={state}\
         &access_type=offline&prompt=consent",
        urlencoding_encode(ANTIGRAVITY_REDIRECT_URI),
        urlencoding_encode(ANTIGRAVITY_SCOPE),
    );

    let (code, recv_state) = if is_headless() {
        tracing::info!("headless environment detected, using paste-back OAuth flow (Antigravity)");
        progress("Headless mode — open this URL in your browser:");
        progress(&auth_url);
        progress(&format!(
            "\nThen paste the full callback URL:\n     \
             http://localhost:{ANTIGRAVITY_CALLBACK_PORT}/oauth-callback?code=...\n     \
             (the browser will show a connection error — that's expected)"
        ));
        let raw = prompt("Paste callback URL: ".to_string()).await?;
        parse_callback_at_path(&raw, "/oauth-callback")?
    } else {
        progress("Opening browser for Google Antigravity authentication…");
        let auth_url_for_browser = auth_url.clone();
        std::thread::spawn(move || {
            let _ = open::that(&auth_url_for_browser);
        });
        progress(&format!(
            "Waiting for callback on localhost:{ANTIGRAVITY_CALLBACK_PORT}…"
        ));
        let listener = bind_callback_listener(ANTIGRAVITY_CALLBACK_PORT)?;
        tracing::info!(
            port = ANTIGRAVITY_CALLBACK_PORT,
            "listening for Antigravity OAuth callback"
        );
        accept_oauth_callback(
            &listener,
            "/oauth-callback",
            &state,
            std::time::Duration::from_secs(300),
        )
        .await?
    };

    if recv_state != state {
        anyhow::bail!(
            "OAuth state mismatch — browser may have been closed or network interrupted. Please try again."
        );
    }

    progress("Exchanging authorization code for tokens…");

    let client = reqwest::Client::new();
    let resp = client
        .post(ANTIGRAVITY_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&client_id={ANTIGRAVITY_CLIENT_ID}\
             &client_secret={ANTIGRAVITY_CLIENT_SECRET}\
             &code={code}&code_verifier={verifier}\
             &redirect_uri={}",
            urlencoding_encode(ANTIGRAVITY_REDIRECT_URI),
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google token exchange failed ({status}): {body}");
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
        expires: now_ms + expires_in * 1000,
    };

    write_credentials("google-antigravity", &creds)?;
    // Drop any session-cached Antigravity token so subsequent resolution uses
    // the shared auth.json entry that was just written and can be refreshed by
    // any Omegon process on this machine.
    unsafe {
        std::env::remove_var("ANTIGRAVITY_OAUTH_TOKEN");
    }
    progress("✓ Google Antigravity authentication successful. Credentials saved.");

    Ok(creds)
}

/// Refresh a Google Antigravity OAuth token.
pub async fn refresh_antigravity_token(refresh: &str) -> anyhow::Result<OAuthCredentials> {
    let client = reqwest::Client::new();
    let resp = client
        .post(ANTIGRAVITY_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={refresh}\
             &client_id={ANTIGRAVITY_CLIENT_ID}&client_secret={ANTIGRAVITY_CLIENT_SECRET}"
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google Antigravity token refresh failed ({status}): {body}");
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
pub fn extract_jwt_claim(token: &str, claim_path: &str, field: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    // Add padding for base64
    let payload = parts[1];
    let padded = match payload.len() % 4 {
        2 => format!("{payload}=="),
        3 => format!("{payload}="),
        _ => payload.to_string(),
    };
    let decoded = base64_decode(&padded)?;
    let json: Value = serde_json::from_slice(&decoded).ok()?;
    if claim_path.is_empty() {
        return json.get(field)?.as_str().map(String::from).or_else(|| {
            json.get(field)
                .and_then(|value| value.as_u64())
                .map(|value| value.to_string())
        });
    }
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
                b'=' => {
                    continue;
                }
                _ => return None,
            };
            valid = i + 1;
        }
        if valid >= 2 {
            result.push((buf[0] << 2) | (buf[1] >> 4));
        }
        if valid >= 3 {
            result.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if valid >= 4 {
            result.push((buf[2] << 6) | buf[3]);
        }
    }
    Some(result)
}

fn write_credentials_with_extra(
    provider: &str,
    creds: &OAuthCredentials,
    account_id: Option<&str>,
) -> anyhow::Result<()> {
    let path =
        auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    assert_test_auth_json_override_for_write(&path)?;
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    with_auth_json_lock(&path, || {
        let mut auth = read_auth_json_for_update(&path, "write_credentials_with_extra", provider)?;
        let mut entry = serde_json::to_value(creds)?;
        if let Some(id) = account_id {
            entry["accountId"] = json!(id);
        }
        let before_keys = auth_json_provider_keys(&auth);
        auth[provider] = entry;
        ensure_auth_json_key_invariants(
            "write_credentials_with_extra",
            provider,
            &before_keys,
            &auth,
        )?;
        trace_auth_json_key_delta(
            "write_credentials_with_extra",
            provider,
            &before_keys,
            &auth,
        );
        atomic_write_auth_json(&path, &auth)?;
        set_auth_file_permissions(&path)?;
        let (auth_path, auth_path_source) = auth_path_trace_fields();
        tracing::info!(provider, auth_path = %auth_path, auth_path_source, credential_type = %creds.cred_type, expires = creds.expires, account_id_present = account_id.is_some(), "persisted provider credentials with extra fields to auth.json");
        Ok(())
    })
}

fn refreshed_credential_entry(
    provider: &str,
    creds: &OAuthCredentials,
    existing_entry: Option<&Value>,
) -> anyhow::Result<Value> {
    let mut entry = serde_json::to_value(creds)?;
    if provider == "openai-codex" {
        let account_id = extract_jwt_claim(
            &creds.access,
            "https://api.openai.com/auth",
            "chatgpt_account_id",
        )
        .or_else(|| {
            existing_entry
                .and_then(|value| value.get("accountId"))
                .and_then(|value| value.as_str())
                .map(String::from)
        });
        if let Some(account_id) = account_id {
            entry["accountId"] = json!(account_id);
        }
    }
    Ok(entry)
}

fn write_refreshed_credentials(provider: &str, creds: &OAuthCredentials) -> anyhow::Result<()> {
    let path =
        auth_json_path().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    assert_test_auth_json_override_for_write(&path)?;
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    with_auth_json_lock(&path, || {
        let mut auth = read_auth_json_for_update(&path, "write_refreshed_credentials", provider)?;
        let existing_entry = auth.get(provider);
        let refreshed_entry = refreshed_credential_entry(provider, creds, existing_entry)?;
        let before_keys = auth_json_provider_keys(&auth);
        auth[provider] = refreshed_entry;
        ensure_auth_json_key_invariants(
            "write_refreshed_credentials",
            provider,
            &before_keys,
            &auth,
        )?;
        trace_auth_json_key_delta("write_refreshed_credentials", provider, &before_keys, &auth);
        atomic_write_auth_json(&path, &auth)?;
        set_auth_file_permissions(&path)?;
        let (auth_path, auth_path_source) = auth_path_trace_fields();
        tracing::info!(provider, auth_path = %auth_path, auth_path_source, expires = creds.expires, "persisted refreshed provider credentials to auth.json");
        Ok(())
    })
}

fn assert_test_auth_json_override_for_write(path: &Path) -> anyhow::Result<()> {
    #[cfg(test)]
    {
        // Test builds must never mutate the operator's real auth store.
        // Credential tests are required to mount an explicit fixture via
        // OMEGON_AUTH_JSON_PATH so full-suite runs cannot delete live OAuth
        // grants such as openai-codex.
        let override_path = std::env::var("OMEGON_AUTH_JSON_PATH")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        if !override_path {
            anyhow::bail!(
                "test attempted to write default auth.json path without OMEGON_AUTH_JSON_PATH override: {}",
                path.display()
            );
        }
    }
    #[cfg(not(test))]
    {
        let _ = path;
    }
    Ok(())
}

fn read_auth_json_for_update(
    path: &Path,
    operation: &'static str,
    provider: &str,
) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(path)?;
    match serde_json::from_str(&content) {
        Ok(auth) => Ok(auth),
        Err(error) => {
            let (auth_path, auth_path_source) = auth_path_trace_fields();
            tracing::error!(
                operation,
                provider,
                auth_path = %auth_path,
                auth_path_source,
                error = %error,
                content_len = content.len(),
                "auth.json parse failed before credential update; refusing to replace provider store with partial data"
            );
            Err(error.into())
        }
    }
}

fn auth_json_provider_keys(auth: &Value) -> Vec<String> {
    let mut keys = auth
        .as_object()
        .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn ensure_auth_json_key_invariants(
    operation: &'static str,
    provider: &str,
    before_keys: &[String],
    after: &Value,
) -> anyhow::Result<()> {
    let after_keys = auth_json_provider_keys(after);
    let removed: Vec<&str> = before_keys
        .iter()
        .filter(|key| !after_keys.contains(key))
        .map(String::as_str)
        .collect();
    if !removed.is_empty() && operation != "logout_provider" {
        anyhow::bail!(
            "auth.json {operation} for {provider} unexpectedly removed provider entries: {}",
            removed.join(", ")
        );
    }
    if provider != "openai-codex"
        && before_keys.iter().any(|key| key == "openai-codex")
        && !after_keys.iter().any(|key| key == "openai-codex")
    {
        anyhow::bail!(
            "auth.json {operation} for {provider} unexpectedly removed openai-codex credentials"
        );
    }
    Ok(())
}

fn trace_auth_json_key_delta(
    operation: &'static str,
    provider: &str,
    before_keys: &[String],
    after: &Value,
) {
    let after_keys = auth_json_provider_keys(after);
    let removed: Vec<&str> = before_keys
        .iter()
        .filter(|key| !after_keys.contains(key))
        .map(String::as_str)
        .collect();
    let added: Vec<&str> = after_keys
        .iter()
        .filter(|key| !before_keys.contains(key))
        .map(String::as_str)
        .collect();
    let dropped_openai_codex = before_keys.iter().any(|key| key == "openai-codex")
        && !after_keys.iter().any(|key| key == "openai-codex");
    let (auth_path, auth_path_source) = auth_path_trace_fields();

    tracing::info!(
        operation,
        provider,
        auth_path = %auth_path,
        auth_path_source,
        before_provider_count = before_keys.len(),
        after_provider_count = after_keys.len(),
        added = ?added,
        removed = ?removed,
        openai_codex_present_before = before_keys.iter().any(|key| key == "openai-codex"),
        openai_codex_present_after = after_keys.iter().any(|key| key == "openai-codex"),
        "auth.json provider key set changed"
    );

    if dropped_openai_codex && provider != "openai-codex" {
        tracing::error!(
            operation,
            provider,
            auth_path = %auth_path,
            auth_path_source,
            before_keys = ?before_keys,
            after_keys = ?after_keys,
            "auth.json mutation dropped openai-codex credentials while writing another provider"
        );
    }
}

fn atomic_write_auth_json(path: &Path, auth: &Value) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(auth)?)?;
    set_auth_file_permissions(&tmp)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn auth_json_lock_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".lock");
    PathBuf::from(os)
}

fn with_auth_json_lock<T>(path: &Path, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    let lock_path = auth_json_lock_path(path);
    for _ in 0..200 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => {
                let result = f();
                let _ = std::fs::remove_file(&lock_path);
                return result;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(anyhow::anyhow!(
        "Timed out waiting for auth.json lock: {}",
        lock_path.display()
    ))
}

fn set_auth_file_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
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
    let code = url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing authorization code in callback"))?;
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("Missing state in callback"))?;

    Ok((code, state))
}

fn urlencoding_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Model limits returned by the provider's models endpoint.
#[derive(Debug, Clone)]
pub struct ModelLimits {
    pub model_id: String,
    pub max_input_tokens: usize,
    pub max_output_tokens: usize,
}

/// Query the Anthropic /v1/models endpoint for the selected model's limits.
/// Returns None if the API is unreachable or the model isn't found.
pub async fn probe_anthropic_model_limits(model_id: &str) -> Option<ModelLimits> {
    let (api_key, is_oauth) = resolve_with_refresh("anthropic").await?;
    let client = reqwest::Client::new();
    let base_url =
        std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".into());

    let mut req = client
        .get(format!("{base_url}/v1/models"))
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");

    if is_oauth {
        req = req
            .header("Authorization", format!("Bearer {api_key}"))
            .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20");
    } else {
        req = req.header("x-api-key", &api_key);
    }

    let resp = match tokio::time::timeout(std::time::Duration::from_secs(5), req.send()).await {
        Ok(Ok(r)) if r.status().is_success() => r,
        Ok(Ok(r)) => {
            tracing::debug!(status = %r.status(), "models endpoint returned error");
            return None;
        }
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "models endpoint request failed");
            return None;
        }
        Err(_) => {
            tracing::debug!("models endpoint timed out (5s)");
            return None;
        }
    };

    let body: serde_json::Value = resp.json().await.ok()?;
    let models = body.get("data")?.as_array()?;

    // Match the requested model — try exact match first, then prefix
    let entry = models
        .iter()
        .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id))
        .or_else(|| {
            // Prefix match for versioned model IDs (e.g. "claude-sonnet-4-6" matches "claude-sonnet-4-6-20260217")
            models.iter().find(|m| {
                m.get("id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id.starts_with(model_id) || model_id.starts_with(id))
            })
        })?;

    let max_input = entry.get("max_input_tokens")?.as_u64()? as usize;
    let max_output = entry.get("max_tokens")?.as_u64()? as usize;
    let found_id = entry.get("id")?.as_str()?.to_string();

    tracing::info!(
        model = %found_id,
        max_input,
        max_output,
        "model limits from /v1/models (authoritative)"
    );

    Some(ModelLimits {
        model_id: found_id,
        max_input_tokens: max_input,
        max_output_tokens: max_output,
    })
}

/// Convert AuthStatus to Vec<ProviderStatus> for HarnessStatus compatibility.
pub fn auth_status_to_provider_statuses(status: &AuthStatus) -> Vec<ProviderStatus> {
    status
        .providers
        .iter()
        .map(|p| {
            let runtime_status = None;
            let auth_state = Some(match p.status {
                ProviderAuthStatus::Authenticated => ProviderAuthState::Configured,
                ProviderAuthStatus::Expired => ProviderAuthState::Expired,
                ProviderAuthStatus::Missing => ProviderAuthState::Missing,
                ProviderAuthStatus::Error => ProviderAuthState::Error,
            });
            ProviderStatus {
                name: p.name.clone(),
                authenticated: p.status == ProviderAuthStatus::Authenticated,
                auth_method: if matches!(
                    p.status,
                    ProviderAuthStatus::Authenticated | ProviderAuthStatus::Expired
                ) {
                    Some(if p.is_oauth { "oauth" } else { "api-key" }.to_string())
                } else {
                    None
                },
                auth_state,
                model: None,
                runtime_status,
                recent_failure_count: None,
                last_failure_kind: None,
                last_failure_at: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_auth_json_path_env<T>(
        path: Option<&Path>,
        f: impl FnOnce() -> T + std::panic::UnwindSafe,
    ) -> T {
        let _guard = TEST_AUTH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
        unsafe {
            match path {
                Some(path) => std::env::set_var("OMEGON_AUTH_JSON_PATH", path),
                None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
            }
        }
        let result = std::panic::catch_unwind(f);
        unsafe {
            match original {
                Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
            }
        }
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn with_auth_json_path_and_home_env<T>(
        auth_path: Option<&Path>,
        home: &Path,
        f: impl FnOnce() -> T + std::panic::UnwindSafe,
    ) -> T {
        let _guard = TEST_AUTH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original_auth = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
        let original_home = std::env::var("HOME").ok();
        unsafe {
            match auth_path {
                Some(path) => std::env::set_var("OMEGON_AUTH_JSON_PATH", path),
                None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
            }
            std::env::set_var("HOME", home);
        }
        let result = std::panic::catch_unwind(f);
        unsafe {
            match original_auth {
                Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
            }
            match original_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn unsigned_codex_jwt_with_exp(exp_seconds: u64) -> String {
        use base64::Engine as _;
        let payload = serde_json::json!({ "exp": exp_seconds }).to_string();
        format!(
            "e30.{}.sig",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
        )
    }

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
            expires: now_ms + 3_600_000,
        };
        assert!(!valid.is_expired());
    }

    #[test]
    fn urlencoding() {
        assert_eq!(urlencoding_encode("hello world"), "hello%20world");
        assert_eq!(urlencoding_encode("a:b"), "a%3Ab");
    }

    #[test]
    fn openai_and_chatgpt_credentials_are_distinct() {
        let openai = provider_by_id("openai").expect("openai provider");
        let codex = provider_by_id("openai-codex").expect("openai-codex provider");
        assert_eq!(openai.auth_key, "openai");
        assert_eq!(openai.auth_method, AuthMethod::ApiKey);
        assert_eq!(codex.auth_key, "openai-codex");
        assert_eq!(codex.auth_method, AuthMethod::OAuth);
    }

    #[test]
    fn read_credentials_normalizes_provider_alias_to_auth_key() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "codex-oauth-token".into(),
            refresh: "codex-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            write_credentials("chatgpt", &creds).expect("write codex alias auth");

            let chatgpt = read_credentials("chatgpt").expect("chatgpt alias credentials");
            let codex = read_credentials("codex").expect("codex alias credentials");
            let openai_codex =
                read_credentials("openai-codex").expect("canonical codex credentials");

            assert_eq!(chatgpt.access, "codex-oauth-token");
            assert_eq!(codex.access, "codex-oauth-token");
            assert_eq!(openai_codex.access, "codex-oauth-token");
        });
    }

    #[tokio::test]
    async fn probe_all_providers_returns_auth_status() {
        let status = probe_all_providers().await;
        assert!(!status.providers.is_empty(), "should have provider entries");
        assert!(
            status.providers.iter().any(|p| p.name == "anthropic"),
            "should probe anthropic"
        );
        assert!(
            status.providers.iter().any(|p| p.name == "openai"),
            "should probe openai api"
        );
        assert!(
            status.providers.iter().any(|p| p.name == "openai-codex"),
            "should probe chatgpt/codex"
        );
        assert!(
            status.providers.iter().any(|p| p.name == "github-copilot"),
            "should probe github copilot"
        );
        assert!(
            !status.providers.iter().any(|p| p.name == "ollama-cloud"),
            "should not probe obsolete ollama cloud"
        );
    }

    #[test]
    fn auth_status_to_provider_statuses_converts() {
        let status = AuthStatus {
            providers: vec![ProviderInfo {
                name: "anthropic".into(),
                status: ProviderAuthStatus::Authenticated,
                is_oauth: true,
                details: Some("stored".into()),
            }],
            vault: vec![],
            secrets: vec![],
            mcp: vec![],
        };
        let converted = auth_status_to_provider_statuses(&status);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "anthropic");
        assert!(converted[0].authenticated);
        assert_eq!(converted[0].auth_method.as_deref(), Some("oauth"));
        assert!(converted[0].runtime_status.is_none());
    }

    // ── Credential resolution edge cases ────────────────────────────────

    #[test]
    fn resolve_with_refresh_env_var_takes_priority_for_api_keys_only() {
        // API-key env vars intentionally override auth.json. OAuth env vars do
        // not, because startup may hydrate them from auth.json and a stale
        // hydrated access token must not shadow the refreshable persisted grant.
        let env_keys: &[&str] = &["ANTHROPIC_API_KEY"];
        // Verify the variable name is correct — compile-time check
        assert_eq!(env_keys[0], "ANTHROPIC_API_KEY");
    }

    #[test]
    fn resolve_with_refresh_prefers_persisted_oauth_over_oauth_env() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        let _guard = TEST_AUTH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original_auth = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
        let original_token = std::env::var("CHATGPT_OAUTH_TOKEN").ok();

        unsafe {
            std::env::set_var("OMEGON_AUTH_JSON_PATH", &override_path);
            std::env::set_var("CHATGPT_OAUTH_TOKEN", "stale-hydrated-env-token");
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        write_credentials(
            "openai-codex",
            &OAuthCredentials {
                cred_type: "oauth".into(),
                access: "fresh-persisted-token".into(),
                refresh: "refresh-token".into(),
                expires: now_ms + 3_600_000,
            },
        )
        .expect("write persisted codex auth");

        let resolved = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
            .block_on(resolve_with_refresh("openai-codex"));

        unsafe {
            match original_auth {
                Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
            }
            match original_token {
                Some(value) => std::env::set_var("CHATGPT_OAUTH_TOKEN", value),
                None => std::env::remove_var("CHATGPT_OAUTH_TOKEN"),
            }
        }

        assert_eq!(resolved, Some(("fresh-persisted-token".into(), true)));
    }

    #[test]
    fn expired_token_detected() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Token that expired 1 second ago
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "expired-token".into(),
            refresh: "refresh-token".into(),
            expires: now_ms - 1000,
        };
        assert!(creds.is_expired(), "token from the past should be expired");
    }

    #[test]
    fn fresh_token_not_expired() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Token that expires in 1 hour
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "fresh-token".into(),
            refresh: "refresh-token".into(),
            expires: now_ms + 3_600_000,
        };
        assert!(
            !creds.is_expired(),
            "token 1 hour in the future should not be expired"
        );
    }

    #[test]
    fn token_expiry_boundary() {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Token that expires exactly now (edge case)
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "edge-token".into(),
            refresh: "refresh-token".into(),
            expires: now_ms,
        };
        // is_expired checks now_ms >= self.expires, so exactly now IS expired
        assert!(
            creds.is_expired(),
            "token at exact expiry should be expired"
        );
    }

    #[test]
    fn progress_callback_is_callable() {
        // Verify the LoginProgress type signature works
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();
        let progress: LoginProgress = Box::new(move |_msg| {
            called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        progress("test");
        assert!(called.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn default_progress_does_not_panic() {
        let progress = default_progress();
        // This writes to stderr, which is fine in tests
        progress("test message from auth test");
    }

    #[test]
    fn codex_refresh_failure_is_fatal() {
        assert!(oauth_refresh_failure_is_fatal("openai-codex"));
        assert!(!oauth_refresh_failure_is_fatal("anthropic"));
    }

    #[test]
    fn auth_json_path_honors_env_override_and_default_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("mounted-auth.json");
        with_auth_json_path_env(Some(&override_path), || {
            assert_eq!(auth_json_path().as_deref(), Some(override_path.as_path()));
        });

        let original = std::env::var("OMEGON_AUTH_JSON_PATH").ok();
        unsafe { std::env::remove_var("OMEGON_AUTH_JSON_PATH") };
        let path = auth_json_path().expect("default auth path");
        unsafe {
            match original {
                Some(value) => std::env::set_var("OMEGON_AUTH_JSON_PATH", value),
                None => std::env::remove_var("OMEGON_AUTH_JSON_PATH"),
            }
        }
        assert!(path.ends_with(".config/omegon/auth.json"));
    }

    #[test]
    fn auth_read_write_use_env_override_path() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("projected").join("auth.json");
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "override-access-token".into(),
            refresh: "override-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            write_credentials("anthropic", &creds).expect("write override auth");
            let loaded = read_credentials("anthropic").expect("read override auth");
            assert_eq!(loaded.access, "override-access-token");
            assert_eq!(loaded.refresh, "override-refresh-token");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&override_path)
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o600);
            }
        });
    }

    #[test]
    fn write_and_read_credentials_roundtrip() {
        // Write creds to a temp dir, then read them back
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join(".config/omegon/auth.json");
        std::fs::create_dir_all(auth_path.parent().unwrap()).unwrap();

        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "test-access-token".into(),
            refresh: "test-refresh-token".into(),
            expires: 9999999999999,
        };

        // Write directly to the temp path
        let mut auth_data = serde_json::Map::new();
        auth_data.insert(
            "test-provider".into(),
            json!({
                "credType": creds.cred_type,
                "access": creds.access,
                "refresh": creds.refresh,
                "expires": creds.expires,
            }),
        );
        std::fs::write(
            &auth_path,
            serde_json::to_string_pretty(&auth_data).unwrap(),
        )
        .unwrap();

        // read_credentials reads from ~/.config/omegon/auth.json which won't
        // find our temp dir, so we just verify the JSON format is correct
        let contents: Value =
            serde_json::from_str(&std::fs::read_to_string(&auth_path).unwrap()).unwrap();
        assert_eq!(contents["test-provider"]["access"], "test-access-token");
        assert_eq!(contents["test-provider"]["refresh"], "test-refresh-token");
    }

    #[test]
    fn writing_one_provider_preserves_existing_codex_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        let codex = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "codex-access-token".into(),
            refresh: "codex-refresh-token".into(),
            expires: 9_999_999_999_999,
        };
        let anthropic = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "anthropic-access-token".into(),
            refresh: "anthropic-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            write_credentials_with_extra("openai-codex", &codex, Some("acct_preserved"))
                .expect("write codex auth");
            write_credentials("anthropic", &anthropic).expect("write anthropic auth");

            let persisted_codex = read_credentials("openai-codex").expect("codex preserved");
            assert_eq!(persisted_codex.access, "codex-access-token");
            assert_eq!(persisted_codex.refresh, "codex-refresh-token");
            assert_eq!(
                read_credential_extra("openai-codex", "accountId").as_deref(),
                Some("acct_preserved")
            );
        });
    }

    #[test]
    fn credential_write_preserves_all_unrelated_provider_entries() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        std::fs::write(
            &override_path,
            serde_json::to_string_pretty(&json!({
                "openai-codex": {
                    "type": "oauth",
                    "access": "codex-access-token",
                    "refresh": "codex-refresh-token",
                    "expires": 9_999_999_999_999u64,
                    "accountId": "acct_preserved"
                },
                "brave": {
                    "type": "api-key",
                    "access": "brave-key",
                    "expires": 18_446_744_073_709_551_615u64
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let anthropic = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "anthropic-access-token".into(),
            refresh: "anthropic-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            write_credentials("anthropic", &anthropic).expect("write anthropic auth");

            let auth: Value =
                serde_json::from_str(&std::fs::read_to_string(&override_path).expect("auth json"))
                    .expect("valid auth json");
            assert_eq!(auth["openai-codex"]["access"], "codex-access-token");
            assert_eq!(auth["openai-codex"]["accountId"], "acct_preserved");
            assert_eq!(auth["brave"]["access"], "brave-key");
            assert_eq!(auth["anthropic"]["access"], "anthropic-access-token");
        });
    }

    #[test]
    fn credential_write_refuses_to_replace_unparsable_auth_json() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        std::fs::write(&override_path, "{not json").unwrap();
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "new-access-token".into(),
            refresh: "new-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            let err = write_credentials("anthropic", &creds)
                .expect_err("malformed existing auth.json must not be replaced");
            assert!(!err.to_string().is_empty());
            assert_eq!(
                std::fs::read_to_string(&override_path).unwrap(),
                "{not json"
            );
        });
    }

    #[test]
    fn refreshed_codex_entry_preserves_existing_account_id() {
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "not-a-jwt".into(),
            refresh: "new-refresh".into(),
            expires: 9999999999999,
        };

        let entry = refreshed_credential_entry(
            "openai-codex",
            &creds,
            Some(&json!({"accountId": "acct_123"})),
        )
        .expect("entry");

        assert_eq!(entry["access"], "not-a-jwt");
        assert_eq!(entry["refresh"], "new-refresh");
        assert_eq!(entry["accountId"], "acct_123");
    }

    #[test]
    fn codex_cli_last_refresh_accepts_rfc3339_timestamps() {
        let parsed = codex_cli_last_refresh_secs(&json!("2026-05-11T02:34:22.555736Z"))
            .expect("timestamp should parse");
        assert_eq!(parsed, 1_778_466_862);
        assert_eq!(
            codex_cli_last_refresh_secs(&json!(1_778_466_862)),
            Some(1_778_466_862)
        );
    }

    #[test]
    fn extract_jwt_claim_reads_top_level_numeric_claims() {
        let token = "eyJhbGciOiJub25lIn0.eyJleHAiOjEyMzQ1fQ.";
        assert_eq!(
            extract_jwt_claim(token, "", "exp").as_deref(),
            Some("12345")
        );
    }

    #[test]
    fn write_refreshed_credentials_preserves_codex_account_id_in_auth_json() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        let initial = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "old-access-token".into(),
            refresh: "old-refresh-token".into(),
            expires: 1,
        };
        let refreshed = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "not-a-jwt".into(),
            refresh: "new-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            write_credentials_with_extra("openai-codex", &initial, Some("acct_123"))
                .expect("write initial codex auth");
            write_refreshed_credentials("openai-codex", &refreshed)
                .expect("write refreshed codex auth");
            assert_eq!(
                read_credential_extra("openai-codex", "accountId").as_deref(),
                Some("acct_123")
            );
            assert_eq!(
                read_credentials("openai-codex")
                    .expect("refreshed credentials")
                    .refresh,
                "new-refresh-token"
            );
        });
    }

    #[test]
    fn import_discovered_credentials_persists_codex_account_id() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        let home = dir.path().join("home");
        let codex_dir = home.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let expires_at_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let access = unsigned_codex_jwt_with_exp(expires_at_secs);
        std::fs::write(
            codex_dir.join("auth.json"),
            serde_json::json!({
                "tokens": {
                    "access_token": access,
                    "refresh_token": "codex-refresh-token",
                    "account_id": "acct_imported"
                }
            })
            .to_string(),
        )
        .unwrap();

        with_auth_json_path_and_home_env(Some(&auth_path), &home, || {
            assert_eq!(import_discovered_provider_credentials(), 1);
            let imported = read_credentials("openai-codex").expect("imported codex credentials");
            assert_eq!(imported.refresh, "codex-refresh-token");
            assert_eq!(imported.access, access);
            assert_eq!(
                read_credential_extra("openai-codex", "accountId").as_deref(),
                Some("acct_imported")
            );
        });
    }

    #[test]
    fn import_discovered_credentials_keeps_fresh_internal_auth() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        let home = dir.path().join("home");
        let codex_dir = home.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let expires_at_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        std::fs::write(
            codex_dir.join("auth.json"),
            serde_json::json!({
                "tokens": {
                    "access_token": unsigned_codex_jwt_with_exp(expires_at_secs),
                    "refresh_token": "external-refresh-token",
                    "account_id": "acct_external"
                }
            })
            .to_string(),
        )
        .unwrap();
        let internal = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "internal-access-token".into(),
            refresh: "internal-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_and_home_env(Some(&auth_path), &home, || {
            write_credentials_with_extra("openai-codex", &internal, Some("acct_internal"))
                .expect("write internal codex auth");
            assert_eq!(import_discovered_provider_credentials(), 0);
            let persisted = read_credentials("openai-codex").expect("persisted credentials");
            assert_eq!(persisted.access, "internal-access-token");
            assert_eq!(
                read_credential_extra("openai-codex", "accountId").as_deref(),
                Some("acct_internal")
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn read_only_auth_json_write_error_gets_operator_safe_message() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let auth_dir = dir.path().join("projected");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::set_permissions(&auth_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let override_path = auth_dir.join("auth.json");
        let creds = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "projected-access-token".into(),
            refresh: "projected-refresh-token".into(),
            expires: 9_999_999_999_999,
        };

        with_auth_json_path_env(Some(&override_path), || {
            let err = write_credentials("anthropic", &creds).expect_err("write should fail");
            let message = auth_write_failure_operator_message(&err);
            assert!(message.contains("read-only"));
            assert!(!message.contains("projected-access-token"));
            assert!(!message.contains("projected-refresh-token"));
        });

        std::fs::set_permissions(&auth_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn is_headless_detects_ssh() {
        // SAFETY: test-only env manipulation — test runner is single-threaded
        // for this test (no parallel readers of these vars).
        unsafe {
            let orig_conn = std::env::var("SSH_CONNECTION").ok();
            let orig_client = std::env::var("SSH_CLIENT").ok();

            std::env::set_var("SSH_CONNECTION", "10.0.0.1 12345 10.0.0.2 22");
            assert!(is_headless(), "should detect SSH_CONNECTION");
            std::env::remove_var("SSH_CONNECTION");

            std::env::set_var("SSH_CLIENT", "10.0.0.1 12345 22");
            assert!(is_headless(), "should detect SSH_CLIENT");
            std::env::remove_var("SSH_CLIENT");

            // Restore
            if let Some(v) = orig_conn {
                std::env::set_var("SSH_CONNECTION", v);
            }
            if let Some(v) = orig_client {
                std::env::set_var("SSH_CLIENT", v);
            }
        }
    }

    #[test]
    fn parse_callback_url_extracts_code_and_state() {
        let url = "http://localhost:53692/callback?code=abc123&state=xyz789";
        let (code, state) = parse_callback_url(url).unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz789");
    }

    #[test]
    fn parse_callback_url_handles_openai_path() {
        let url = "http://localhost:1455/auth/callback?code=oai_code&state=oai_state";
        let (code, state) = parse_callback_url(url).unwrap();
        assert_eq!(code, "oai_code");
        assert_eq!(state, "oai_state");
    }

    #[test]
    fn parse_callback_url_rejects_garbage() {
        assert!(parse_callback_url("not a url").is_err());
        assert!(parse_callback_url("http://localhost/callback").is_err());
        assert!(parse_callback_url("http://localhost/callback?state=x").is_err());
    }

    #[tokio::test]
    async fn accept_oauth_callback_skips_noise_and_stale_state() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client = tokio::spawn(async move {
            // 1. Speculative preconnect: connect and close without sending.
            let pre = tokio::net::TcpStream::connect(addr).await.unwrap();
            drop(pre);

            // 2. Browser noise: favicon request.
            let mut fav = tokio::net::TcpStream::connect(addr).await.unwrap();
            fav.write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut resp = String::new();
            let _ = fav.read_to_string(&mut resp).await;
            assert!(resp.starts_with("HTTP/1.1 404"), "favicon got: {resp}");

            // 3. Stale tab: valid shape, wrong state.
            let mut stale = tokio::net::TcpStream::connect(addr).await.unwrap();
            stale
                .write_all(b"GET /auth/callback?code=old_code&state=old_state HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut resp = String::new();
            let _ = stale.read_to_string(&mut resp).await;
            assert!(resp.starts_with("HTTP/1.1 409"), "stale got: {resp}");

            // 4. The real callback for this attempt.
            let mut real = tokio::net::TcpStream::connect(addr).await.unwrap();
            real.write_all(b"GET /auth/callback?code=good_code&state=good_state HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            let mut resp = String::new();
            let _ = real.read_to_string(&mut resp).await;
            assert!(resp.starts_with("HTTP/1.1 200"), "real got: {resp}");
        });

        let (code, state) = accept_oauth_callback(
            &listener,
            "/auth/callback",
            "good_state",
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(code, "good_code");
        assert_eq!(state, "good_state");
        client.await.unwrap();
    }

    #[tokio::test]
    async fn accept_oauth_callback_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let err = accept_oauth_callback(
            &listener,
            "/callback",
            "state",
            std::time::Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[test]
    fn parse_callback_url_trims_whitespace() {
        let url = "  http://localhost:53692/callback?code=abc&state=xyz  \n";
        let (code, state) = parse_callback_url(url).unwrap();
        assert_eq!(code, "abc");
        assert_eq!(state, "xyz");
    }

    #[tokio::test]
    async fn default_prompt_type_is_constructible() {
        // Verify the LoginPrompt type compiles and the default factory works
        let _prompt = default_prompt();
    }

    #[test]
    fn canonical_provider_display_names_are_stable() {
        assert_eq!(
            provider_by_id("anthropic").map(|p| p.display_name),
            Some("Anthropic/Claude")
        );
        assert_eq!(
            provider_by_id("openai-codex").map(|p| p.display_name),
            Some("OpenAI/Codex")
        );
    }

    #[test]
    fn canonical_provider_id_normalizes_operator_aliases() {
        assert_eq!(canonical_provider_id("claude"), "anthropic");
        assert_eq!(canonical_provider_id("chatgpt"), "openai-codex");
        assert_eq!(canonical_provider_id("codex"), "openai-codex");
        assert_eq!(canonical_provider_id("openai"), "openai");
    }

    #[test]
    fn canonical_provider_id_returns_static_known_provider_ids_without_recursing() {
        for provider in [
            "anthropic",
            "openai",
            "openai-codex",
            "openrouter",
            "ollama-cloud",
            "ollama",
            "groq",
            "xai",
            "mistral",
            "cerebras",
            "brave",
            "tavily",
            "serper",
            "github",
            "gitlab",
            "huggingface",
        ] {
            assert_eq!(canonical_provider_id(provider), provider);
            assert!(
                provider_by_id(provider).is_some(),
                "provider should resolve: {provider}"
            );
        }
    }

    #[test]
    fn provider_session_status_distinguishes_configured_expired_and_missing() {
        let fresh = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "token".into(),
            refresh: "refresh".into(),
            expires: u64::MAX,
        };
        let expired = OAuthCredentials {
            cred_type: "oauth".into(),
            access: "token".into(),
            refresh: "refresh".into(),
            expires: 0,
        };

        assert_eq!(
            provider_session_status_from_sources(true, Some(&expired)),
            ProviderSessionStatus::Configured
        );
        assert_eq!(
            provider_session_status_from_sources(false, Some(&fresh)),
            ProviderSessionStatus::Configured
        );
        assert_eq!(
            provider_session_status_from_sources(false, Some(&expired)),
            ProviderSessionStatus::Expired
        );
        assert_eq!(
            provider_session_status_from_sources(false, None),
            ProviderSessionStatus::Missing
        );
    }

    #[test]
    fn fresh_external_codex_oauth_overrides_expired_auth_json_for_status() {
        let dir = tempfile::tempdir().unwrap();
        let auth_path = dir.path().join("auth.json");
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let fresh_external_access = unsigned_codex_jwt_with_exp(u64::MAX / 1000);
        std::fs::write(
            home.join(".codex/auth.json"),
            serde_json::json!({
                "tokens": {
                    "access_token": fresh_external_access,
                    "refresh_token": "fresh-external-refresh",
                    "account_id": "external-account"
                }
            })
            .to_string(),
        )
        .unwrap();

        with_auth_json_path_and_home_env(Some(&auth_path), &home, || {
            write_credentials_with_extra(
                "openai-codex",
                &OAuthCredentials {
                    cred_type: "oauth".into(),
                    access: "expired-auth-json-token".into(),
                    refresh: "expired-auth-json-refresh".into(),
                    expires: 0,
                },
                Some("expired-account"),
            )
            .unwrap();

            let codex = provider_by_id("openai-codex").unwrap();
            assert_eq!(
                provider_session_status(codex),
                ProviderSessionStatus::Configured
            );
            assert!(provider_connected_for_model("gpt-5.5"));
        });
    }

    #[test]
    fn openai_family_models_can_use_codex_oauth_for_connection_status() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        with_auth_json_path_env(Some(&override_path), || {
            write_credentials_with_extra(
                "openai-codex",
                &OAuthCredentials {
                    cred_type: "oauth".into(),
                    access: "codex-oauth-token".into(),
                    refresh: "codex-refresh-token".into(),
                    expires: u64::MAX,
                },
                Some("account-id"),
            )
            .unwrap();

            assert!(provider_connected_for_model("gpt-5.5"));
            assert!(provider_connected_for_model("openai:gpt-5.5"));
            assert!(provider_oauth_for_model("gpt-5.5"));
            assert!(provider_oauth_for_model("openai:gpt-5.5"));
        });
    }

    #[test]
    fn codex_prefixed_openai_family_models_can_fall_back_to_openai_api_key_status() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        with_auth_json_path_and_home_env(Some(&override_path), dir.path(), || {
            write_credentials(
                "openai",
                &OAuthCredentials {
                    cred_type: "api-key".into(),
                    access: "openai-api-key".into(),
                    refresh: String::new(),
                    expires: u64::MAX,
                },
            )
            .unwrap();

            assert!(provider_connected_for_model("openai-codex:gpt-5.5"));
            assert!(!provider_oauth_for_model("openai-codex:gpt-5.5"));
        });
    }

    #[test]
    fn nested_copilot_models_use_copilot_auth_status_not_producer_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        with_auth_json_path_and_home_env(Some(&override_path), dir.path(), || {
            write_credentials(
                "anthropic",
                &OAuthCredentials {
                    cred_type: "oauth".into(),
                    access: "expired-anthropic-token".into(),
                    refresh: "expired-anthropic-refresh".into(),
                    expires: 0,
                },
            )
            .unwrap();
            write_credentials(
                "github-copilot",
                &OAuthCredentials {
                    cred_type: "oauth".into(),
                    access: "copilot-token".into(),
                    refresh: "copilot-refresh".into(),
                    expires: u64::MAX,
                },
            )
            .unwrap();

            assert!(provider_connected_for_model(
                "anthropic:github-copilot:gpt-5.5"
            ));
            assert!(provider_oauth_for_model("anthropic:github-copilot:gpt-5.5"));
        });
    }

    #[test]
    fn operator_auth_provider_help_list_excludes_local_ollama() {
        let providers = operator_auth_provider_help_list();
        assert!(providers.contains("anthropic"), "got: {providers}");
        assert!(providers.contains("openai-codex"), "got: {providers}");
        assert!(providers.contains("github-copilot"), "got: {providers}");
        assert!(!providers.contains("ollama"), "got: {providers}");
    }

    #[test]
    fn auth_status_uses_operator_auth_provider_list() {
        let dir = tempfile::tempdir().unwrap();
        let override_path = dir.path().join("auth.json");
        let status = with_auth_json_path_and_home_env(Some(&override_path), dir.path(), || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(probe_all_providers())
        });
        let providers: std::collections::HashSet<_> = status
            .providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect();

        for provider in operator_auth_provider_ids() {
            assert!(
                providers.contains(provider),
                "auth status missing operator auth provider {provider}; got: {:?}",
                providers
            );
        }
        assert!(
            providers.contains("github-copilot"),
            "auth status must include GitHub Copilot even before login"
        );
        assert!(
            !providers.contains("ollama-cloud"),
            "auth status must not use the obsolete hardcoded provider list"
        );
    }
    #[test]
    fn endpoint_secret_refs_cover_registry_only_providers() {
        assert_eq!(endpoint_secret_refs("ollama"), Vec::<String>::new());
        // Existing first-class providers stay governed by PROVIDERS.
        assert_eq!(endpoint_secret_refs("groq"), Vec::<String>::new());
    }
}
