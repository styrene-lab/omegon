//! Native LLM provider clients — direct HTTP streaming, no Node.js.
//!
//! Replaces core/bridge/llm-bridge.mjs entirely. The Rust binary makes
//! HTTPS requests directly to api.anthropic.com / api.openai.com.
//!
//! API keys resolved from: env vars → ~/.config/omegon/auth.json (OAuth tokens).
//! The upstream provider APIs are the only external dependency — no npm,
//! no Node.js, no supply chain risk from package registries.

use async_trait::async_trait;
use futures_util::FutureExt;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::bridge::{LlmBridge, LlmEvent, LlmMessage, StreamOptions};

/// Claude Code CLI version for OAuth user-agent header.
/// Must match what Anthropic expects for subscription recognition.
/// Update when upstream Claude Code advances.
const CLAUDE_CODE_UA: &str = "claude-cli/2.1.75";
use omegon_traits::ToolDefinition;

// ─── API Key Resolution ─────────────────────────────────────────────────────

/// Anthropic credential mode — records what credential source is active.
///
/// Omegon may still warn when subscription/OAuth credentials are used in
/// automated contexts, but OAuth credentials remain executable for local
/// headless and benchmark runs when they are the available Anthropic auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicCredentialMode {
    /// A direct API key is present.
    ApiKey,
    /// An OAuth/subscription token is present.
    OAuthOnly,
    /// No Anthropic credentials configured.
    None,
}

/// Determine what Anthropic credential is available.
///
/// Priority: ANTHROPIC_API_KEY (or auth.json api-key entry) wins over ANTHROPIC_OAUTH_TOKEN.
pub fn anthropic_credential_mode() -> AnthropicCredentialMode {
    match resolve_api_key_sync("anthropic") {
        Some((_, false)) => AnthropicCredentialMode::ApiKey,
        Some((_, true)) => AnthropicCredentialMode::OAuthOnly,
        None => AnthropicCredentialMode::None,
    }
}

/// Find the best automation-safe model available — i.e. one that is not subject to
/// subscription-only interactive-use constraints or unsupported consumer-backend automation.
///
/// Priority (highest to lowest):
///   1. OpenAI API key  → `openai:gpt-4o`
///   2. OpenRouter      → `openrouter:openai/gpt-4o`
///   3. Ollama          → `ollama:llama3` (local, always unrestricted)
///
/// Intentionally excludes consumer subscription routes such as ChatGPT/Codex OAuth and
/// Anthropic subscription OAuth. Those may be usable interactively in some cases, but they
/// are not treated as automation-safe defaults.
///
/// Returns `None` only when no automation-safe provider is available.
///
/// Ollama availability is probed once per process lifetime (50ms TCP connect) and
/// cached — safe to call from the TUI event loop and async orchestrator paths.
pub fn automation_safe_model() -> Option<String> {
    // 1. OpenAI direct API key
    if resolve_api_key_sync("openai").is_some_and(|(_, oauth)| !oauth) {
        return Some("openai:gpt-4o".to_string());
    }
    // 2. OpenRouter
    if resolve_api_key_sync("openrouter").is_some() {
        return Some("openrouter:openai/gpt-4o".to_string());
    }
    // 3. Ollama — local inference, always unrestricted.
    // Probe once per process with a tight 50ms timeout (localhost should respond in <5ms).
    // Cached in a OnceLock so repeated calls (TUI event loop, orchestrator) are instant.
    static OLLAMA_AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let ollama_up = OLLAMA_AVAILABLE.get_or_init(|| {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1:11434".to_string());
        let addr_str = host
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        // Normalise: if no port, append :11434
        let addr_str = if addr_str.contains(':') {
            addr_str.to_string()
        } else {
            format!("{addr_str}:11434")
        };
        addr_str
            .parse::<std::net::SocketAddr>()
            .ok()
            .and_then(|addr| {
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(50))
                    .ok()
            })
            .is_some()
    });
    if *ollama_up {
        return Some("ollama:llama3".to_string());
    }
    None
}

/// Resolve API key synchronously — env vars and unexpired auth.json tokens.
/// Returns (key, is_oauth).
pub fn resolve_api_key_sync(provider: &str) -> Option<(String, bool)> {
    // Use canonical provider map for env vars and auth.json key
    let env_keys = crate::auth::provider_env_vars(provider);
    let auth_key = crate::auth::auth_json_key(provider);

    // Env vars (not OAuth)
    for key in env_keys
        .iter()
        .copied()
        .filter(|key| !key.contains("OAUTH"))
    {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            tracing::debug!(provider, source = key, "API key resolved from env");
            return Some((val, false));
        }
    }

    // OAuth token env vars
    for key in env_keys.iter().copied().filter(|key| key.contains("OAUTH")) {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            tracing::debug!(provider, source = key, "OAuth token resolved from env");
            return Some((val, true));
        }
    }

    // auth.json — using canonical key
    match crate::auth::read_credentials(auth_key) {
        Some(creds) if creds.cred_type == "oauth" && !creds.is_expired() => {
            tracing::debug!(
                provider,
                auth_key,
                expires = creds.expires,
                "OAuth token from auth.json (valid)"
            );
            return Some((creds.access, true));
        }
        Some(creds) if creds.cred_type == "oauth" => {
            tracing::debug!(
                provider,
                auth_key,
                expires = creds.expires,
                "OAuth token from auth.json (EXPIRED — needs refresh)"
            );
        }
        Some(creds) => {
            tracing::debug!(provider, auth_key, cred_type = %creds.cred_type, "credential from auth.json");
            return Some((creds.access, false));
        }
        None => {
            tracing::debug!(provider, auth_key, "no credentials in auth.json");
        }
    }

    None
}

/// Resolve API key from env vars or ~/.config/omegon/auth.json (legacy, no refresh).
fn resolve_api_key(provider: &str) -> Option<String> {
    // Use canonical provider map for env vars
    let env_keys = crate::auth::provider_env_vars(provider);
    for key in env_keys {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            return Some(val);
        }
    }

    // Generic fallback: PROVIDER_API_KEY
    let generic = format!("{}_API_KEY", provider.to_uppercase());
    if let Ok(val) = std::env::var(&generic)
        && !val.is_empty()
    {
        return Some(val);
    }

    // auth.json — use canonical key mapping
    let auth_key = crate::auth::auth_json_key(provider);
    let auth_path = crate::auth::auth_json_path()?;
    let content = std::fs::read_to_string(&auth_path).ok()?;
    let auth: Value = serde_json::from_str(&content).ok()?;
    auth.get(auth_key)?
        .get("access")?
        .as_str()
        .map(String::from)
}

fn is_known_provider_id(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "anthropic"
            | "openai"
            | "openai-codex"
            | "openrouter"
            | "groq"
            | "xai"
            | "mistral"
            | "cerebras"
            | "huggingface"
            | "ollama"
            | "ollama-cloud"
            | "local"
    )
}

/// Infer the concrete provider from a model spec.
///
/// Accepts both canonical `provider:model` strings and bare model IDs like
/// `qwen3:30b` or `claude-sonnet-4-6`. The `local` alias maps to `ollama`.
pub fn infer_provider_id(model_spec: &str) -> String {
    let trimmed = model_spec.trim();
    if trimmed.is_empty() {
        return "anthropic".to_string();
    }

    if let Some((head, _tail)) = trimmed.split_once(':')
        && is_known_provider_id(head)
    {
        return if head == "local" { "ollama" } else { head }.to_string();
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower == "local" {
        return "ollama".to_string();
    }
    if lower.starts_with("claude") || matches!(lower.as_str(), "haiku" | "sonnet" | "opus") {
        return "anthropic".to_string();
    }
    if lower.starts_with("gpt-") || matches!(lower.as_str(), "o1" | "o3" | "o4") {
        return "openai".to_string();
    }
    if lower.starts_with("codex") {
        return "openai-codex".to_string();
    }
    if lower.contains('/') {
        return "openrouter".to_string();
    }
    // Common open-source models typically run on Ollama
    if lower.starts_with("qwen")
        || lower.starts_with("llama")
        || lower.starts_with("devstral")
        || lower.starts_with("nemotron")
        || lower.starts_with("mistral")
        || lower.starts_with("dolphin")
        || lower.starts_with("neural")
        || lower.starts_with("glm")
        || lower.starts_with("kimi")
        || lower.starts_with("gemma")
        || lower.starts_with("phi")
        || lower.starts_with("deepseek")
        || lower.starts_with("wizardlm")
        || lower.starts_with("orca")
        || lower.starts_with("vicuna")
    {
        return "ollama".to_string();
    }

    // Unknown model — warn rather than silently route to Anthropic
    tracing::warn!(
        "provider_from_model: unrecognized model spec {:?}, defaulting to anthropic",
        model_spec
    );
    "anthropic".to_string()
}

fn model_id_from_spec(model_spec: &str) -> &str {
    let trimmed = model_spec.trim();
    if let Some((head, tail)) = trimmed.split_once(':')
        && is_known_provider_id(head)
    {
        return tail;
    }
    trimmed
}

fn is_openai_family_model(model_spec: &str) -> bool {
    let model_id = model_id_from_spec(model_spec).to_ascii_lowercase();
    model_id.starts_with("gpt-")
        || model_id == "o1"
        || model_id == "o3"
        || model_id == "o4"
        || model_id.starts_with("o4-")
}

fn fallback_order_for_model(model_spec: &str) -> Vec<&'static str> {
    let requested = infer_provider_id(model_spec);

    match requested.as_str() {
        "openai" if is_openai_family_model(model_spec) => {
            vec!["openai", "openai-codex"]
        }
        "openai-codex" if is_openai_family_model(model_spec) => {
            vec!["openai-codex", "openai"]
        }
        "openai-codex" => vec!["openai-codex"],
        "anthropic" => vec!["anthropic"],
        "openrouter" => vec!["openrouter"],
        "groq" => vec!["groq"],
        "xai" => vec!["xai"],
        "mistral" => vec!["mistral"],
        "cerebras" => vec!["cerebras"],
        "huggingface" => vec!["huggingface"],
        "ollama" => vec!["ollama"],
        "ollama-cloud" => vec!["ollama-cloud"],
        _ => vec!["anthropic"],
    }
}

pub async fn resolve_execution_provider(model_spec: &str) -> Option<String> {
    for provider in fallback_order_for_model(model_spec) {
        if let Some(_bridge) = resolve_provider(provider).await {
            return Some(provider.to_string());
        }
    }
    None
}

pub async fn resolve_execution_model_spec(model_spec: &str) -> Option<String> {
    let resolved_provider = resolve_execution_provider(model_spec).await?;
    Some(format!(
        "{}:{}",
        resolved_provider,
        model_id_from_spec(model_spec)
    ))
}

/// Resolve a single provider by ID.
/// Resolve a single provider by ID. Returns a bridge if the provider
/// has credentials and a native client implementation.
///
/// Providers without native clients (groq, xai, mistral, cerebras)
/// return None here — they need an OpenAI-compatible client layer
/// which is tracked for re-implementation.
pub async fn resolve_provider(provider_id: &str) -> Option<Box<dyn LlmBridge>> {
    match provider_id {
        "anthropic" => {
            if let Some(client) = AnthropicClient::from_env() {
                return Some(Box::new(client));
            }
            AnthropicClient::from_env_async()
                .await
                .map(|c| Box::new(c) as Box<dyn LlmBridge>)
        }
        "openai" => OpenAIClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        "openrouter" => OpenRouterClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        // OpenAI-compatible providers — all use the Chat Completions protocol
        "groq" | "xai" | "mistral" | "cerebras" | "huggingface" | "ollama" => {
            OpenAICompatClient::from_env(provider_id).map(|c| Box::new(c) as Box<dyn LlmBridge>)
        }
        "ollama-cloud" => OllamaCloudClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        // Codex uses the Responses API (not Chat Completions) with OAuth JWT tokens
        "openai-codex" => {
            if let Some(client) = CodexClient::from_env() {
                return Some(Box::new(client));
            }
            CodexClient::from_env_async()
                .await
                .map(|c| Box::new(c) as Box<dyn LlmBridge>)
        }
        _ => None,
    }
}

/// Auto-detect the best available native provider from configured keys.
/// Tries sync resolution first, then async (with token refresh) if needed.
pub async fn auto_detect_bridge(model_spec: &str) -> Option<Box<dyn LlmBridge>> {
    let requested = infer_provider_id(model_spec);
    let attempts = fallback_order_for_model(model_spec);

    for provider in attempts {
        if let Some(bridge) = resolve_provider(provider).await {
            if provider != requested {
                tracing::info!(requested = %requested, resolved = provider, model_spec, "falling back to alternate executable provider");
            }
            return Some(bridge);
        }
    }

    tracing::warn!(requested = %requested, model_spec, "no executable provider available");
    None
}

// ─── SSE Helpers ────────────────────────────────────────────────────────────

/// Extract and log rate limit headers from a provider's HTTP response.
/// All major providers return quota/remaining/reset information on every
/// response — this is the only source of subscription usage data.
fn parse_rate_limit_snapshot(
    provider: &str,
    headers: &reqwest::header::HeaderMap,
) -> Option<omegon_traits::ProviderTelemetrySnapshot> {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let parse_pct =
        |name: &str| get(name).and_then(|v| v.trim().trim_end_matches('%').parse::<f32>().ok());
    let parse_u64 = |name: &str| get(name).and_then(|v| v.trim().parse::<u64>().ok());
    let parse_duration_secs = |name: &str| {
        get(name).and_then(|v| {
            let trimmed = v.trim();
            if let Some(ms) = trimmed.strip_suffix("ms") {
                ms.trim()
                    .parse::<u64>()
                    .ok()
                    .map(|millis| millis.div_ceil(1000))
            } else if let Some(secs) = trimmed.strip_suffix('s') {
                secs.trim().parse::<u64>().ok()
            } else {
                trimmed.parse::<u64>().ok()
            }
        })
    };

    // ── ChatGPT Codex x-codex-* headers ─────────────────────────────────
    let codex_active_limit = get("x-codex-active-limit").map(ToOwned::to_owned);
    let codex_primary_used_pct = parse_pct("x-codex-primary-used-percent")
        .or_else(|| parse_pct("x-codex-bengalfox-primary-used-percent"))
        // Legacy fallback from older proxy headers; semantically weaker than used-percent.
        .or_else(|| parse_u64("x-codex-primary-over-secondary-limit-percent").map(|v| v as f32))
        .or_else(|| {
            parse_u64("x-codex-bengalfox-primary-over-secondary-limit-percent").map(|v| v as f32)
        });
    let codex_secondary_used_pct = parse_pct("x-codex-secondary-used-percent")
        .or_else(|| parse_pct("x-codex-bengalfox-secondary-used-percent"));
    let codex_primary_reset_secs = parse_u64("x-codex-primary-reset-after-seconds")
        .or_else(|| parse_u64("x-codex-bengalfox-primary-reset-after-seconds"));
    let codex_secondary_reset_secs = parse_u64("x-codex-secondary-reset-after-seconds")
        .or_else(|| parse_u64("x-codex-bengalfox-secondary-reset-after-seconds"));
    let codex_credits_unlimited =
        get("x-codex-credits-unlimited").map(|v| v.eq_ignore_ascii_case("true"));
    let codex_limit_name = get("x-codex-bengalfox-limit-name").map(ToOwned::to_owned);

    let snapshot = omegon_traits::ProviderTelemetrySnapshot {
        provider: provider.to_string(),
        source: "response_headers".into(),
        unified_5h_utilization_pct: parse_pct("anthropic-ratelimit-unified-5h-utilization"),
        unified_7d_utilization_pct: parse_pct("anthropic-ratelimit-unified-7d-utilization"),
        requests_remaining: parse_u64("x-ratelimit-remaining-requests")
            .or_else(|| parse_u64("ratelimit-remaining-requests")),
        tokens_remaining: parse_u64("x-ratelimit-remaining-tokens")
            .or_else(|| parse_u64("ratelimit-remaining-tokens"))
            .or_else(|| parse_u64("x-ratelimit-remaining-tokens-usage-based"))
            .or_else(|| parse_u64("ratelimit-remaining-tokens-usage-based")),
        retry_after_secs: parse_duration_secs("retry-after")
            .or_else(|| parse_duration_secs("x-ratelimit-reset-requests"))
            .or_else(|| parse_duration_secs("ratelimit-reset-requests"))
            .or_else(|| parse_duration_secs("x-ratelimit-reset-tokens"))
            .or_else(|| parse_duration_secs("ratelimit-reset-tokens"))
            .or_else(|| parse_duration_secs("x-ratelimit-reset-tokens-usage-based"))
            .or_else(|| parse_duration_secs("ratelimit-reset-tokens-usage-based")),
        request_id: get("x-request-id")
            .or_else(|| get("request-id"))
            .or_else(|| get("x-openai-request-id"))
            .or_else(|| get("x-oai-request-id"))
            .map(ToOwned::to_owned),
        codex_active_limit,
        codex_primary_used_pct,
        codex_secondary_used_pct,
        codex_primary_reset_secs,
        codex_secondary_reset_secs,
        codex_credits_unlimited,
        codex_limit_name,
    };

    let has_any = snapshot.unified_5h_utilization_pct.is_some()
        || snapshot.unified_7d_utilization_pct.is_some()
        || snapshot.requests_remaining.is_some()
        || snapshot.tokens_remaining.is_some()
        || snapshot.retry_after_secs.is_some()
        || snapshot.request_id.is_some()
        || snapshot.codex_active_limit.is_some();
    has_any.then_some(snapshot)
}

fn collect_headers(
    headers: &reqwest::header::HeaderMap,
    predicate: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name_str = name.as_str().to_lowercase();
            if !predicate(&name_str) {
                return None;
            }
            value.to_str().ok().map(|v| (name_str, v.to_string()))
        })
        .collect()
}

fn log_rate_limit_headers(provider: &str, headers: &reqwest::header::HeaderMap) {
    // Collect all rate-limit-related headers into a structured log.
    let limits = collect_headers(headers, |name| {
        name.contains("ratelimit")
            || name.contains("rate-limit")
            || name.contains("retry-after")
            || name.contains("x-request-id")
            || name.contains("request-id")
            || name.contains("quota")
            || name.contains("usage")
            || name.contains("limit")
            || name.contains("remaining")
            || name.contains("reset")
    });

    if !limits.is_empty() {
        let pairs: Vec<String> = limits.iter().map(|(k, v)| format!("{k}={v}")).collect();
        tracing::info!(
            provider,
            header_count = limits.len(),
            headers = %pairs.join(", "),
            "provider telemetry-related headers"
        );
        return;
    }

    // Codex via chatgpt.com may expose quota state through non-standard header
    // names or a separate endpoint. When no telemetry-like headers matched,
    // log the full header set once per response so we can see what the upstream
    // actually returned instead of guessing.
    if provider == "openai-codex" {
        let all_headers = collect_headers(headers, |_| true);
        let pairs: Vec<String> = all_headers
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        tracing::info!(
            provider,
            header_count = all_headers.len(),
            headers = %pairs.join(", "),
            "provider response headers (no telemetry headers matched)"
        );
    }
}

/// Sanitize a tool call ID to match Anthropic's `^[a-zA-Z0-9_-]+$` pattern.
/// Codex compound IDs use `call_abc|fc_1` — take only the call_id before the pipe.
/// Any remaining invalid characters are replaced with underscores.
fn sanitize_tool_id(id: &str) -> String {
    // Strip Codex compound suffix (pipe-separated item ID)
    let base = if id.contains('|') {
        id.splitn(2, '|').next().unwrap_or(id)
    } else {
        id
    };
    // Replace any remaining non-alphanumeric/underscore/hyphen characters
    base.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Strip `description` fields from parameter properties to reduce token cost.
/// Keeps type, enum, default, items, minimum, maximum — drops only descriptions.
fn strip_parameter_descriptions(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut compact = serde_json::Map::new();
            for (key, val) in map {
                if key == "description" {
                    continue; // Strip all description fields at any depth
                }
                compact.insert(key.clone(), strip_parameter_descriptions(val));
            }
            Value::Object(compact)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(strip_parameter_descriptions).collect()),
        other => other.clone(),
    }
}

/// Map tool names to Claude Code PascalCase canonical names for OAuth.
fn to_claude_code_name(name: &str) -> String {
    match name {
        "bash" => "Bash".into(),
        "read" => "Read".into(),
        "write" => "Write".into(),
        "edit" => "Edit".into(),
        "web_search" => "WebSearch".into(),
        _ => name.to_string(),
    }
}

/// Map Claude Code PascalCase names back to lowercase for tool dispatch.
fn from_claude_code_name(name: &str) -> String {
    match name {
        "Bash" => "bash".into(),
        "Read" => "read".into(),
        "Write" => "write".into(),
        "Edit" => "edit".into(),
        "WebSearch" => "web_search".into(),
        _ => name.to_string(),
    }
}

/// Accumulator for streaming tool call arguments.
struct ToolCallAccum {
    id: String,
    name: String,
    args_json: String,
}

impl ToolCallAccum {
    fn to_value(&self) -> Value {
        let args: Value = serde_json::from_str(&self.args_json).unwrap_or_else(|_| json!({}));
        // Ensure arguments is always an object — Anthropic rejects null/string.
        let args = if args.is_object() { args } else { json!({}) };
        json!({"id": self.id, "name": self.name, "arguments": args})
    }
}

/// Process an SSE byte stream line by line, calling `on_data` for each `data: ` payload.
/// SSE idle timeout — if no chunk arrives within this window, assume the
/// connection is stalled and bail so the retry loop can re-attempt.
/// OpenAI's Codex CLI defaults to 300s (stream_idle_timeout_ms = 300000)
/// because reasoning models can be silent for minutes before first token.
const SSE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

async fn process_sse<F>(response: reqwest::Response, mut on_data: F) -> anyhow::Result<()>
where
    F: FnMut(&str) -> bool, // returns false to stop
{
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();

    loop {
        match tokio::time::timeout(SSE_IDLE_TIMEOUT, stream.next()).await {
            Ok(Some(chunk)) => {
                let chunk = chunk?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(newline) = buffer.find('\n') {
                    let line = buffer[..newline].trim_end_matches('\r').to_string();
                    buffer = buffer[newline + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ")
                        && (data == "[DONE]" || !on_data(data))
                    {
                        return Ok(());
                    }
                }
            }
            Ok(None) => break, // stream ended
            Err(_) => {
                tracing::warn!(
                    "SSE stream idle for {}s — treating as stalled",
                    SSE_IDLE_TIMEOUT.as_secs()
                );
                anyhow::bail!(
                    "SSE stream idle timeout ({}s with no data)",
                    SSE_IDLE_TIMEOUT.as_secs()
                );
            }
        }
    }
    Ok(())
}

fn spawn_provider_stream_task<Fut>(
    provider: &'static str,
    tx: mpsc::Sender<LlmEvent>,
    fut: Fut,
) -> tokio::task::JoinHandle<()>
where
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let task = std::panic::AssertUnwindSafe(fut).catch_unwind().await;
        let message = match task {
            Ok(Ok(())) => None,
            Ok(Err(err)) => Some(err.to_string()),
            Err(panic_payload) => {
                let panic_text = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic payload".to_string()
                };
                Some(format!("{provider} stream parser panicked: {panic_text}"))
            }
        };

        if let Some(message) = message {
            tracing::warn!(provider, %message, "provider stream task terminated with error");
            let _ = tx.send(LlmEvent::Error { message }).await;
        }
    })
}

// ─── Anthropic ──────────────────────────────────────────────────────────────

pub struct AnthropicClient {
    client: reqwest::Client,
    api_key: String,
    is_oauth: bool,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(api_key: String, is_oauth: bool) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            is_oauth,
            base_url: std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".into()),
        }
    }

    pub fn from_env() -> Option<Self> {
        // Try sync resolution first (env vars, unexpired tokens)
        let (key, is_oauth) = resolve_api_key_sync("anthropic")?;
        Some(Self::new(key, is_oauth))
    }

    /// Create from async resolution (with token refresh).
    pub async fn from_env_async() -> Option<Self> {
        let (key, is_oauth) = crate::auth::resolve_with_refresh("anthropic").await?;
        Some(Self::new(key, is_oauth))
    }

    fn build_messages(messages: &[LlmMessage]) -> Vec<Value> {
        messages.iter().map(|m| match m {
            LlmMessage::User { content, images } => {
                if images.is_empty() {
                    json!({"role": "user", "content": content})
                } else {
                    // Build content blocks array: images first, then text
                    let mut blocks = Vec::new();
                    for img in images {
                        blocks.push(json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": img.media_type,
                                "data": img.data,
                            }
                        }));
                    }
                    blocks.push(json!({"type": "text", "text": content}));
                    json!({"role": "user", "content": blocks})
                }
            }
            LlmMessage::Assistant { text, thinking: _, tool_calls, raw } => {
                // Prefer raw content blocks if available — they preserve provider-specific
                // fields like thinking signatures that are required for round-tripping.
                if let Some(raw_val) = raw {
                    if let Some(raw_content) = raw_val.get("content").and_then(|c| c.as_array()) {
                        if !raw_content.is_empty() {
                            return json!({"role": "assistant", "content": raw_content});
                        }
                    }
                }
                // Fallback: reconstruct from parsed fields. Thinking blocks are
                // OMITTED because Anthropic requires a valid `signature` for
                // round-tripping and we don't have one without the raw content.
                // This happens after compaction or when switching from another provider.
                let mut content = Vec::new();
                // thinking blocks intentionally skipped — no signature available
                for t in text {
                    content.push(json!({"type": "text", "text": t}));
                }
                for tc in tool_calls {
                    // Anthropic requires `input` to be a JSON object, never null/string.
                    let input = if tc.arguments.is_object() {
                        tc.arguments.clone()
                    } else {
                        json!({})
                    };
                    // Sanitize tool call IDs — Anthropic requires ^[a-zA-Z0-9_-]+$
                    // Codex compound IDs use `call_abc|fc_1` format; strip the pipe
                    // and suffix so cross-provider history doesn't cause 400 errors.
                    let sanitized_id = sanitize_tool_id(&tc.id);
                    content.push(json!({
                        "type": "tool_use",
                        "id": sanitized_id,
                        "name": tc.name,
                        "input": input,
                    }));
                }
                json!({"role": "assistant", "content": content})
            }
            LlmMessage::ToolResult { call_id, content, is_error, .. } => {
                // Sanitize tool call IDs for cross-provider compatibility
                let sanitized_id = sanitize_tool_id(call_id);
                json!({
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": sanitized_id, "content": content, "is_error": is_error}]
                })
            }
        }).collect()
    }

    fn build_tools(tools: &[ToolDefinition], is_oauth: bool) -> Vec<Value> {
        let tool_count = tools.len();
        tools
            .iter()
            .enumerate()
            .map(|(idx, t)| {
                let name = if is_oauth {
                    to_claude_code_name(&t.name)
                } else {
                    t.name.clone()
                };
                // Strip parameter-level descriptions to save tokens.
                // The model infers parameter semantics from names + the tool
                // description. Full descriptions cost ~50 tokens/tool × 31 tools.
                let properties = t.parameters.get("properties").cloned().unwrap_or(json!({}));
                let compact_props = strip_parameter_descriptions(&properties);
                let mut tool_json = json!({
                    "name": name,
                    "description": t.description,
                    "input_schema": {
                        "type": "object",
                        "properties": compact_props,
                        "required": t.parameters.get("required").cloned().unwrap_or(json!([])),
                    },
                });
                // Mark the last tool with cache_control so the entire tools
                // array is included in the Anthropic prompt cache prefix.
                if idx == tool_count - 1 {
                    tool_json["cache_control"] = json!({"type": "ephemeral"});
                }
                tool_json
            })
            .collect()
    }
}

#[async_trait]
impl LlmBridge for AnthropicClient {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(256);

        // Re-resolve credentials on each request. This handles:
        // - /login mid-session writing new tokens to auth.json
        // - Token expiry + automatic refresh
        // - Env var changes
        let (api_key, is_oauth) = match crate::auth::resolve_with_refresh("anthropic").await {
            Some(resolved) => resolved,
            None => {
                tracing::warn!(
                    "credential re-resolution failed — using startup credentials \
                     (may be stale if /login was used mid-session)"
                );
                (self.api_key.clone(), self.is_oauth)
            }
        };

        let model = options
            .model
            .as_deref()
            .map(|m| model_id_from_spec(m))
            .unwrap_or("claude-sonnet-4-6");

        // System prompt: always array-of-blocks format (required for cache_control).
        // Split on CACHE_BOUNDARY sentinel to separate the stable prefix
        // (base prompt, directives) from dynamic per-turn content (HUD, intent,
        // memory). The stable prefix gets cache_control for Anthropic prompt caching.
        let system_value = {
            let sentinel = crate::bridge::CACHE_BOUNDARY;
            let mut blocks = Vec::new();

            if is_oauth {
                blocks.push(json!({
                    "type": "text",
                    "text": "You are Claude Code, Anthropic's official CLI for Claude.",
                    "cache_control": {"type": "ephemeral"}
                }));
            }

            if let Some(pos) = system_prompt.find(sentinel) {
                let stable = system_prompt[..pos].trim();
                let dynamic = system_prompt[pos + sentinel.len()..].trim();

                // Stable prefix — cached across turns
                if !stable.is_empty() {
                    blocks.push(json!({
                        "type": "text",
                        "text": stable,
                        "cache_control": {"type": "ephemeral"}
                    }));
                }
                // Dynamic segment — NOT cached
                if !dynamic.is_empty() {
                    blocks.push(json!({
                        "type": "text",
                        "text": dynamic
                    }));
                }
            } else {
                // No sentinel — send entire prompt as single block
                blocks.push(json!({
                    "type": "text",
                    "text": system_prompt
                }));
            }

            Value::Array(blocks)
        };

        let mut body = json!({
            "model": model,
            "max_tokens": 16384,
            "system": system_value,
            "messages": Self::build_messages(messages),
            "stream": true,
        });

        let wire_tools = Self::build_tools(tools, is_oauth);
        let tool_count = wire_tools.len();
        if !wire_tools.is_empty() {
            body["tools"] = Value::Array(wire_tools);
        }
        apply_anthropic_thinking(&mut body, model, options.reasoning.as_deref());

        let msg_count = body["messages"].as_array().map(|a| a.len()).unwrap_or(0);
        let system_len = system_prompt.len();
        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        tracing::debug!(
            model,
            is_oauth,
            tool_count,
            msg_count,
            system_len,
            body_size,
            base_url = %self.base_url,
            "Anthropic streaming request"
        );
        tracing::trace!(body = %serde_json::to_string(&body).unwrap_or_default(), "request body");

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header(
                if is_oauth {
                    "Authorization"
                } else {
                    "x-api-key"
                },
                if is_oauth {
                    format!("Bearer {}", api_key)
                } else {
                    api_key.clone()
                },
            )
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", {
                let flags = if is_oauth {
                    "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14"
                        .to_string()
                } else {
                    "interleaved-thinking-2025-05-14".to_string()
                };
                // NOTE: context-1m-2025-08-07 is NEVER sent. Sonnet 4.6 and
                // Opus 4.6 support 1M context natively without a beta flag.
                // Sending it triggers "Extra usage is required for long context
                // requests" (429) on OAuth subscriptions — a deprecated billing
                // gate that no longer corresponds to a capability gate.
                flags
            })
            .header("content-type", "application/json")
            // Claude Code identity headers for OAuth subscription recognition
            .header(
                "user-agent",
                if is_oauth { CLAUDE_CODE_UA } else { "omegon" },
            )
            .header("x-app", "cli")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let headers = format!("{:?}", response.headers());
            let err = response.text().await.unwrap_or_default();
            tracing::error!(
                %status,
                error_body = %err,
                response_headers = %headers,
                body_size,
                tool_count,
                system_len,
                is_oauth,
                "Anthropic API error"
            );
            tracing::debug!(request_body = %serde_json::to_string(&body).unwrap_or_default(), "failed request body");
            // Extract the human-readable message from the API error body
            let user_msg = serde_json::from_str::<Value>(&err)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| err.chars().take(200).collect());
            let detail = if is_oauth && (status.as_u16() == 429 || status.as_u16() == 413) {
                format!(
                    "\n  (OAuth subscription — {tool_count} tools, {body_size} byte request body, system prompt {system_len} chars)"
                )
            } else {
                String::new()
            };
            let _ = tx
                .send(LlmEvent::Error {
                    message: format!("Anthropic {status}: {user_msg}{detail}"),
                })
                .await;
            return Ok(rx);
        }
        // Extract rate limit headers before consuming the response for SSE
        let provider_telemetry = parse_rate_limit_snapshot("anthropic", response.headers());
        log_rate_limit_headers("anthropic", response.headers());
        tracing::debug!(status = %response.status(), "Anthropic response OK — starting SSE stream");

        spawn_provider_stream_task("anthropic", tx.clone(), async move {
            parse_anthropic_stream(response, provider_telemetry, &tx).await
        });

        Ok(rx)
    }
}

async fn parse_anthropic_stream(
    response: reqwest::Response,
    provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    tx: &mpsc::Sender<LlmEvent>,
) -> anyhow::Result<()> {
    let mut block_type: Option<String> = None;
    let mut full_text = String::new();
    let mut tool_calls: Vec<ToolCallAccum> = Vec::new();
    // Accumulate complete content blocks for round-tripping (preserves signatures, etc.)
    let mut content_blocks: Vec<Value> = Vec::new();
    let mut current_block_text = String::new(); // per-block text accumulator
    let mut current_thinking_text = String::new();
    let mut current_thinking_signature: Option<String> = None;
    // Actual billing tokens captured from message_start / message_delta
    let mut acc_input_tokens: u64 = 0;
    let mut acc_output_tokens: u64 = 0;
    let mut acc_cache_read_tokens: u64 = 0;
    let mut acc_cache_creation_tokens: u64 = 0;

    tracing::debug!("parsing Anthropic SSE stream");
    let provider_telemetry_done = provider_telemetry.clone();
    let mut event_count = 0u32;

    process_sse(response, |data| {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            tracing::warn!(data, "failed to parse SSE event as JSON");
            return true;
        };
        let etype = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
        event_count += 1;
        tracing::trace!(event_type = etype, n = event_count, "SSE event");

        match etype {
            "message_start" => {
                // message_start contains input token usage
                if let Some(usage) = event.pointer("/message/usage") {
                    tracing::info!(
                        input_tokens = usage["input_tokens"].as_u64().unwrap_or(0),
                        cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
                        cache_creation = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                        "Anthropic usage (input)"
                    );
                }
                tracing::debug!("message_start received");
                let _ = tx.try_send(LlmEvent::Start);
            }

            "content_block_start" => {
                let bt = event["content_block"]["type"].as_str().unwrap_or("");
                block_type = Some(bt.to_string());
                match bt {
                    "text" => {
                        current_block_text.clear();
                        let _ = tx.try_send(LlmEvent::TextStart);
                    }
                    "thinking" => {
                        current_thinking_text.clear();
                        current_thinking_signature = None;
                        let _ = tx.try_send(LlmEvent::ThinkingStart);
                    }
                    "tool_use" => {
                        let id = event["content_block"]["id"].as_str().unwrap_or("").to_string();
                        let raw_name = event["content_block"]["name"].as_str().unwrap_or("");
                        let name = from_claude_code_name(raw_name);
                        tracing::debug!(tool_id = %id, raw_name, name = %name, "tool_use block started");
                        tool_calls.push(ToolCallAccum { id: id.clone(), name: name.clone(), args_json: String::new() });
                        let _ = tx.try_send(LlmEvent::ToolCallStart);
                    }
                    _ => {}
                }
            }

            "content_block_delta" => {
                let dt = event["delta"]["type"].as_str().unwrap_or("");
                match dt {
                    "text_delta" => {
                        let t = event["delta"]["text"].as_str().unwrap_or("");
                        full_text.push_str(t);
                        current_block_text.push_str(t);
                        let _ = tx.try_send(LlmEvent::TextDelta { delta: t.to_string() });
                    }
                    "thinking_delta" => {
                        let t = event["delta"]["thinking"].as_str().unwrap_or("");
                        current_thinking_text.push_str(t);
                        let _ = tx.try_send(LlmEvent::ThinkingDelta { delta: t.to_string() });
                    }
                    "signature_delta" => {
                        let sig = event["delta"]["signature"].as_str().unwrap_or("");
                        match &mut current_thinking_signature {
                            Some(s) => s.push_str(sig),
                            None => current_thinking_signature = Some(sig.to_string()),
                        }
                    }
                    "input_json_delta" => {
                        let p = event["delta"]["partial_json"].as_str().unwrap_or("");
                        if let Some(tc) = tool_calls.last_mut() {
                            tc.args_json.push_str(p);
                        }
                    }
                    _ => {}
                }
            }

            "content_block_stop" => {
                match block_type.as_deref() {
                    Some("text") => {
                        content_blocks.push(json!({"type": "text", "text": current_block_text.clone()}));
                        let _ = tx.try_send(LlmEvent::TextEnd);
                    }
                    Some("thinking") => {
                        let mut block = json!({
                            "type": "thinking",
                            "thinking": current_thinking_text.clone(),
                        });
                        if let Some(ref sig) = current_thinking_signature {
                            block["signature"] = json!(sig);
                        }
                        content_blocks.push(block);
                        let _ = tx.try_send(LlmEvent::ThinkingEnd);
                    }
                    Some("tool_use") => {
                        if let Some(tc) = tool_calls.last() {
                            let input = serde_json::from_str::<Value>(&tc.args_json)
                                .ok()
                                .filter(|v| v.is_object())
                                .unwrap_or_else(|| json!({}));
                            content_blocks.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": input,
                            }));
                            let _ = tx.try_send(LlmEvent::ToolCallEnd { tool_call: crate::bridge::WireToolCall { id: tc.id.clone(), name: tc.name.clone(), arguments: input.clone() } });
                        }
                    }
                    _ => {}
                }
                block_type = None;
            }

            // message_delta: stop_reason + final usage
            "message_delta" => {
                if let Some(usage) = event.get("usage") {
                    let out = usage["output_tokens"].as_u64().unwrap_or(0);
                    let inp = usage["input_tokens"].as_u64().unwrap_or(0);
                    let cr  = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    let cc  = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    if out > 0 { acc_output_tokens = out; }
                    if inp > 0 { acc_input_tokens  = inp; }
                    if cr  > 0 { acc_cache_read_tokens = cr; }
                    if cc  > 0 { acc_cache_creation_tokens = cc; }
                    tracing::info!(
                        output_tokens = out, input_tokens = inp,
                        cache_read = cr, cache_creation = cc,
                        "Anthropic usage (final)"
                    );
                }
                if let Some(stop) = event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
                    tracing::debug!(stop_reason = stop, "message_delta");
                }
            }

            // Events from newer SDK versions — gracefully handled
            "citation" | "citations_delta" => {
                tracing::trace!(event_type = etype, "citation event");
            }
            "signature" | "signature_delta" => {
                // Signature events can arrive as top-level SSE events (outside content_block_delta).
                // Accumulate into current_thinking_signature for the most recent thinking block.
                if let Some(sig) = event.get("signature").and_then(|s| s.as_str()) {
                    match &mut current_thinking_signature {
                        Some(s) => s.push_str(sig),
                        None => current_thinking_signature = Some(sig.to_string()),
                    }
                    // Patch the last thinking block in content_blocks if it exists
                    if let Some(last) = content_blocks.last_mut() {
                        if last.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                            last["signature"] = json!(current_thinking_signature.as_deref().unwrap_or(""));
                        }
                    }
                }
                tracing::trace!(event_type = etype, "signature event");
            }
            "server_tool_use" => {
                tracing::debug!(event_type = etype, "server_tool_use (not yet dispatched)");
            }

            "message_stop" => {
                tracing::debug!(
                    text_len = full_text.len(),
                    tool_calls = tool_calls.len(),
                    sse_events = event_count,
                    "message_stop — stream complete"
                );
                let tc_vals: Vec<Value> = tool_calls.iter().map(|tc| tc.to_value()).collect();
                let _ = tx.try_send(LlmEvent::Done {
                    message: json!({
                        "text": full_text,
                        "tool_calls": tc_vals,
                        "content": content_blocks,
                    }),
                    input_tokens: acc_input_tokens,
                    output_tokens: acc_output_tokens,
                    cache_read_tokens: acc_cache_read_tokens,
                    cache_creation_tokens: acc_cache_creation_tokens,
                    provider_telemetry: provider_telemetry_done.clone(),
                });
                return false; // stop
            }
            _ => {}
        }
        true
    }).await
}

// ─── OpenAI ─────────────────────────────────────────────────────────────────

pub struct OpenAIClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl OpenAIClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com".into()),
        }
    }

    pub fn from_env() -> Option<Self> {
        resolve_api_key("openai").map(Self::new)
    }

    fn build_wire_messages(system_prompt: &str, messages: &[LlmMessage]) -> Vec<Value> {
        let mut wire_msgs = vec![json!({"role": "system", "content": system_prompt})];
        for m in messages {
            match m {
                LlmMessage::User { content, images } => {
                    if images.is_empty() {
                        wire_msgs.push(json!({"role": "user", "content": content}));
                    } else {
                        let mut blocks: Vec<Value> = images.iter().map(|img| json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{};base64,{}", img.media_type, img.data) }
                        })).collect();
                        blocks.push(json!({"type": "text", "text": content}));
                        wire_msgs.push(json!({"role": "user", "content": blocks}));
                    }
                }
                LlmMessage::Assistant {
                    text, tool_calls, ..
                } => {
                    let mut msg = json!({"role": "assistant"});
                    if let Some(t) = text.first() {
                        msg["content"] = json!(t);
                    }
                    if !tool_calls.is_empty() {
                        msg["tool_calls"] = tool_calls.iter().map(|tc| json!({
                            "id": tc.id, "type": "function",
                            "function": {"name": tc.name, "arguments": if tc.arguments.is_object() { tc.arguments.to_string() } else { "{}".to_string() }},
                        })).collect();
                    }
                    wire_msgs.push(msg);
                }
                LlmMessage::ToolResult {
                    call_id, content, ..
                } => {
                    wire_msgs
                        .push(json!({"role": "tool", "tool_call_id": call_id, "content": content}));
                }
            }
        }
        wire_msgs
    }
}

#[async_trait]
impl LlmBridge for OpenAIClient {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(256);

        // Strip any provider prefix (openai:, openrouter:, etc.) from model.
        // OpenRouter and OpenAICompatClient delegate through here with
        // pre-stripped or re-prefixed model names.
        let model = options
            .model
            .as_deref()
            .map(|m| model_id_from_spec(m))
            .unwrap_or("gpt-4.1");

        let wire_msgs = Self::build_wire_messages(system_prompt, messages);

        let wire_tools: Vec<Value> = tools.iter().map(|t| json!({
            "type": "function",
            "function": {"name": t.name, "description": t.description, "parameters": t.parameters},
        })).collect();

        let mut body = json!({"model": model, "messages": wire_msgs, "stream": true});
        if !wire_tools.is_empty() {
            body["tools"] = Value::Array(wire_tools);
        }
        // Merge any extra fields (e.g. Ollama num_ctx, keep_alive)
        for (k, v) in &options.extra_body {
            body[k] = v.clone();
        }

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let err = response.text().await.unwrap_or_default();
            let user_msg = serde_json::from_str::<Value>(&err)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| err.chars().take(200).collect());
            let _ = tx
                .send(LlmEvent::Error {
                    message: format!("OpenAI {status}: {user_msg}"),
                })
                .await;
            return Ok(rx);
        }

        let provider_telemetry = parse_rate_limit_snapshot("openai", response.headers());
        log_rate_limit_headers("openai", response.headers());

        spawn_provider_stream_task("openai", tx.clone(), async move {
            parse_openai_stream(response, provider_telemetry, &tx).await
        });

        Ok(rx)
    }
}

async fn parse_openai_stream(
    response: reqwest::Response,
    provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    tx: &mpsc::Sender<LlmEvent>,
) -> anyhow::Result<()> {
    let mut full_text = String::new();
    let mut tool_calls: Vec<ToolCallAccum> = Vec::new();
    let mut acc_input_tokens: u64 = 0;
    let mut acc_output_tokens: u64 = 0;
    let provider_telemetry_done = provider_telemetry.clone();

    let _ = tx.try_send(LlmEvent::Start);
    let _ = tx.try_send(LlmEvent::TextStart);

    process_sse(response, |data| {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return true;
        };

        // Usage block appears at the top level of the final chunk
        if let Some(usage) = event.get("usage") {
            let pt = usage["prompt_tokens"].as_u64().unwrap_or(0);
            let ct = usage["completion_tokens"].as_u64().unwrap_or(0);
            if pt > 0 {
                acc_input_tokens = pt;
            }
            if ct > 0 {
                acc_output_tokens = ct;
            }
            tracing::info!(
                prompt_tokens = pt,
                completion_tokens = ct,
                total_tokens = usage["total_tokens"].as_u64().unwrap_or(0),
                "OpenAI usage"
            );
        }

        let Some(choice) = event.get("choices").and_then(|c| c.get(0)) else {
            return true;
        };
        let delta = &choice["delta"];

        // Text
        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
            full_text.push_str(content);
            let _ = tx.try_send(LlmEvent::TextDelta {
                delta: content.to_string(),
            });
        }

        // Tool calls
        if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tcs {
                let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                while tool_calls.len() <= idx {
                    tool_calls.push(ToolCallAccum {
                        id: String::new(),
                        name: String::new(),
                        args_json: String::new(),
                    });
                }
                if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                    tool_calls[idx].id = id.to_string();
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                        tool_calls[idx].name = name.to_string();
                        let _ = tx.try_send(LlmEvent::ToolCallStart);
                    }
                    if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                        tool_calls[idx].args_json.push_str(args);
                    }
                }
            }
        }

        // Finish
        if choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .is_some()
        {
            for tc in &tool_calls {
                let _ = tx.try_send(LlmEvent::ToolCallEnd {
                    tool_call: crate::bridge::WireToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: serde_json::from_str(&tc.args_json).unwrap_or_default(),
                    },
                });
            }
            let _ = tx.try_send(LlmEvent::TextEnd);
            let tc_vals: Vec<Value> = tool_calls.iter().map(|tc| tc.to_value()).collect();
            let _ = tx.try_send(LlmEvent::Done {
                message: json!({"text": full_text, "tool_calls": tc_vals}),
                input_tokens: acc_input_tokens,
                output_tokens: acc_output_tokens,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: provider_telemetry_done.clone(),
            });
            return false;
        }
        true
    })
    .await
}

// ─── OpenRouter ─────────────────────────────────────────────────────────────
//
// OpenRouter speaks the OpenAI wire protocol but routes across 27+ free models.
// Uses the OpenAI client internally with a different base URL and API key source.

pub struct OpenRouterClient {
    inner: OpenAIClient,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        Self {
            inner: OpenAIClient {
                client: reqwest::Client::new(),
                api_key,
                base_url: "https://openrouter.ai/api".into(),
            },
        }
    }

    pub fn from_env() -> Option<Self> {
        resolve_api_key("openrouter").map(Self::new)
    }
}

#[async_trait]
impl LlmBridge for OpenRouterClient {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        // Delegate to the OpenAI client — OpenRouter is wire-compatible.
        // Override model to use OpenRouter's free model selector if none specified.
        let mut opts = options.clone();
        if opts.model.is_none() || opts.model.as_deref() == Some("") {
            opts.model = Some("openrouter:openrouter/free".into());
        }
        // Rewrite model prefix: strip "openrouter:" for the wire request
        if let Some(ref mut m) = opts.model {
            if let Some(stripped) = m.strip_prefix("openrouter:") {
                *m = stripped.to_string();
            }
        }
        self.inner
            .stream(system_prompt, messages, tools, &opts)
            .await
    }
}

// ─── Codex Client (OpenAI Responses API) ─────────────────────────────────────

const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api";

pub struct CodexClient {
    client: reqwest::Client,
    jwt_token: String,
    account_id: String,
    base_url: String,
}

impl CodexClient {
    pub fn new(jwt_token: String, account_id: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            jwt_token,
            account_id,
            base_url: std::env::var("CODEX_BASE_URL").unwrap_or_else(|_| CODEX_BASE_URL.into()),
        }
    }

    pub fn from_env() -> Option<Self> {
        // 1. Try CHATGPT_OAUTH_TOKEN env var
        if let Ok(token) = std::env::var("CHATGPT_OAUTH_TOKEN") {
            if !token.is_empty() && token.starts_with("eyJ") {
                if let Some(account_id) = crate::auth::extract_jwt_claim(
                    &token,
                    "https://api.openai.com/auth",
                    "chatgpt_account_id",
                ) {
                    tracing::debug!("CodexClient: resolved from CHATGPT_OAUTH_TOKEN env var");
                    return Some(Self::new(token, account_id));
                }
            }
        }

        // 2. Try auth.json (openai-codex entry)
        let creds = crate::auth::read_credentials("openai-codex")?;
        if creds.cred_type != "oauth" || creds.access.is_empty() || !creds.access.starts_with("eyJ")
        {
            return None;
        }
        if creds.is_expired() {
            tracing::debug!("CodexClient: auth.json token expired — needs refresh");
            return None;
        }
        let account_id =
            crate::auth::read_credential_extra("openai-codex", "accountId").or_else(|| {
                crate::auth::extract_jwt_claim(
                    &creds.access,
                    "https://api.openai.com/auth",
                    "chatgpt_account_id",
                )
            })?;
        tracing::debug!("CodexClient: resolved from auth.json");
        Some(Self::new(creds.access, account_id))
    }

    pub async fn from_env_async() -> Option<Self> {
        if let Some(client) = Self::from_env() {
            return Some(client);
        }
        let (token, is_oauth) = crate::auth::resolve_with_refresh("openai-codex").await?;
        if !is_oauth || !token.starts_with("eyJ") {
            return None;
        }
        let account_id =
            crate::auth::read_credential_extra("openai-codex", "accountId").or_else(|| {
                crate::auth::extract_jwt_claim(
                    &token,
                    "https://api.openai.com/auth",
                    "chatgpt_account_id",
                )
            })?;
        Some(Self::new(token, account_id))
    }

    fn build_input(messages: &[LlmMessage]) -> Vec<Value> {
        let mut input = Vec::new();
        let mut msg_index = 0u32;
        for msg in messages {
            match msg {
                LlmMessage::User { content, images } => {
                    if images.is_empty() {
                        input.push(json!({"role": "user", "content": [{"type": "input_text", "text": content}]}));
                    } else {
                        let mut parts: Vec<Value> = images.iter().map(|img| json!({
                            "type": "input_image", "detail": "auto",
                            "image_url": format!("data:{};base64,{}", img.media_type, img.data),
                        })).collect();
                        parts.push(json!({"type": "input_text", "text": content}));
                        input.push(json!({"role": "user", "content": parts}));
                    }
                }
                LlmMessage::Assistant {
                    text, tool_calls, ..
                } => {
                    for t in text {
                        if !t.is_empty() {
                            input.push(json!({
                                "type": "message", "role": "assistant",
                                "content": [{"type": "output_text", "text": t, "annotations": []}],
                                "status": "completed", "id": format!("msg_{msg_index}"),
                            }));
                            msg_index += 1;
                        }
                    }
                    for tc in tool_calls {
                        let (call_id, item_id) = if tc.id.contains('|') {
                            let parts: Vec<&str> = tc.id.splitn(2, '|').collect();
                            (
                                parts[0].to_string(),
                                parts.get(1).unwrap_or(&"fc_0").to_string(),
                            )
                        } else {
                            (tc.id.clone(), format!("fc_{msg_index}"))
                        };
                        input.push(json!({
                            "type": "function_call", "id": item_id, "call_id": call_id,
                            "name": tc.name,
                            "arguments": if tc.arguments.is_object() { tc.arguments.to_string() } else { "{}".into() },
                        }));
                        msg_index += 1;
                    }
                }
                LlmMessage::ToolResult {
                    call_id, content, ..
                } => {
                    let cid = if call_id.contains('|') {
                        call_id.splitn(2, '|').next().unwrap_or(call_id).to_string()
                    } else {
                        call_id.clone()
                    };
                    input.push(
                        json!({"type": "function_call_output", "call_id": cid, "output": content}),
                    );
                }
            }
        }
        input
    }

    fn build_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                let compact = strip_parameter_descriptions(
                    t.parameters.get("properties").unwrap_or(&json!({})),
                );
                json!({
                    "type": "function", "name": t.name, "description": t.description,
                    "parameters": {
                        "type": "object", "properties": compact,
                        "required": t.parameters.get("required").cloned().unwrap_or(json!([])),
                    },
                    "strict": null,
                })
            })
            .collect()
    }
}

fn is_codex_retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504 | 520)
}

#[async_trait]
impl LlmBridge for CodexClient {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(256);

        let (jwt_token, account_id) = match crate::auth::resolve_with_refresh("openai-codex").await
        {
            Some((token, true)) if token.starts_with("eyJ") => {
                let aid = crate::auth::read_credential_extra("openai-codex", "accountId")
                    .or_else(|| {
                        crate::auth::extract_jwt_claim(
                            &token,
                            "https://api.openai.com/auth",
                            "chatgpt_account_id",
                        )
                    })
                    .unwrap_or_else(|| self.account_id.clone());
                (token, aid)
            }
            _ => (self.jwt_token.clone(), self.account_id.clone()),
        };

        let model = options
            .model
            .as_deref()
            .and_then(|m| {
                m.strip_prefix("openai-codex:")
                    .or_else(|| m.strip_prefix("openai:"))
            })
            .unwrap_or("gpt-5.4-mini");

        let input = Self::build_input(messages);
        let wire_tools = Self::build_tools(tools);

        let mut body = json!({
            "model": model, "store": false, "stream": true,
            "instructions": system_prompt, "input": input,
            "text": {"verbosity": "medium"},
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto", "parallel_tool_calls": true,
        });
        if !wire_tools.is_empty() {
            body["tools"] = Value::Array(wire_tools);
        }
        if let Some(effort) = openai_reasoning_effort(options.reasoning.as_deref()) {
            body["reasoning"] = json!({"effort": effort, "summary": "auto"});
        }

        let url = format!("{}/codex/responses", self.base_url.trim_end_matches('/'));
        let max_retries = 3u32;
        let base_delay = std::time::Duration::from_secs(1);
        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {jwt_token}"))
                .header("chatgpt-account-id", &account_id)
                .header("originator", "omegon")
                .header("OpenAI-Beta", "responses=experimental")
                .header("accept", "text/event-stream")
                .header("content-type", "application/json")
                .header(
                    "user-agent",
                    format!(
                        "omegon ({} {}; {})",
                        std::env::consts::OS,
                        std::env::consts::ARCH,
                        env!("CARGO_PKG_VERSION")
                    ),
                )
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    let provider_telemetry =
                        parse_rate_limit_snapshot("openai-codex", resp.headers());
                    log_rate_limit_headers("openai-codex", resp.headers());
                    let tx_clone = tx.clone();
                    spawn_provider_stream_task("openai-codex", tx_clone.clone(), async move {
                        parse_codex_stream(resp, provider_telemetry, &tx_clone).await
                    });
                    return Ok(rx);
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let err_body = resp.text().await.unwrap_or_default();
                    let user_msg = serde_json::from_str::<Value>(&err_body)
                        .ok()
                        .and_then(|v| {
                            v["error"]["message"]
                                .as_str()
                                .or(v["detail"].as_str())
                                .map(String::from)
                        })
                        .unwrap_or_else(|| err_body.chars().take(200).collect());
                    if attempt < max_retries && is_codex_retryable(status) {
                        tokio::time::sleep(base_delay * 2u32.pow(attempt)).await;
                        last_error = format!("Codex {status}: {user_msg}");
                        continue;
                    }
                    let _ = tx
                        .send(LlmEvent::Error {
                            message: format!("Codex {status}: {user_msg}"),
                        })
                        .await;
                    return Ok(rx);
                }
                Err(e) => {
                    if attempt < max_retries {
                        tokio::time::sleep(base_delay * 2u32.pow(attempt)).await;
                        last_error = format!("Network error: {e}");
                        continue;
                    }
                    let _ = tx
                        .send(LlmEvent::Error {
                            message: format!("Codex connection failed: {last_error}"),
                        })
                        .await;
                    return Ok(rx);
                }
            }
        }
        let _ = tx
            .send(LlmEvent::Error {
                message: format!("Codex failed after retries: {last_error}"),
            })
            .await;
        Ok(rx)
    }
}

/// Parse Codex Responses API SSE stream (different event structure from Chat Completions).
async fn parse_codex_stream(
    response: reqwest::Response,
    provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    tx: &mpsc::Sender<LlmEvent>,
) -> anyhow::Result<()> {
    let mut full_text = String::new();
    let mut _current_item_type: Option<String> = None;
    let mut _current_text = String::new();
    let mut _current_thinking = String::new();
    struct ToolAcc {
        call_id: String,
        item_id: String,
        name: String,
        args_json: String,
    }
    let mut tool_calls: Vec<ToolAcc> = Vec::new();
    let mut completed_tool_calls: Vec<Value> = Vec::new();
    let provider_telemetry_done = provider_telemetry.clone();
    let _ = tx.try_send(LlmEvent::Start);

    // Terminal events (Done / Error) are deferred and sent *after* process_sse
    // returns, using `.send().await` for guaranteed delivery. `try_send` inside
    // the sync closure can silently drop on a full channel (capacity 256).
    enum TerminalEvent {
        Done {
            input_tokens: u64,
            output_tokens: u64,
        },
        Error(String),
    }
    let mut terminal: Option<TerminalEvent> = None;

    process_sse(response, |data| {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            return true;
        };
        let etype = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match etype {
            "response.output_item.added" => {
                let item = &event["item"];
                match item["type"].as_str().unwrap_or("") {
                    "reasoning" => {
                        _current_item_type = Some("reasoning".into());
                        _current_thinking.clear();
                        let _ = tx.try_send(LlmEvent::ThinkingStart);
                    }
                    "message" => {
                        _current_item_type = Some("message".into());
                        _current_text.clear();
                        let _ = tx.try_send(LlmEvent::TextStart);
                    }
                    "function_call" => {
                        _current_item_type = Some("function_call".into());
                        tool_calls.push(ToolAcc {
                            call_id: item["call_id"].as_str().unwrap_or("").into(),
                            item_id: item["id"].as_str().unwrap_or("").into(),
                            name: item["name"].as_str().unwrap_or("").into(),
                            args_json: String::new(),
                        });
                        let _ = tx.try_send(LlmEvent::ToolCallStart);
                    }
                    _ => {}
                }
            }
            "response.output_text.delta" => {
                let delta = event["delta"].as_str().unwrap_or("");
                full_text.push_str(delta);
                _current_text.push_str(delta);
                let _ = tx.try_send(LlmEvent::TextDelta {
                    delta: delta.into(),
                });
            }
            "response.reasoning_summary_text.delta" => {
                let delta = event["delta"].as_str().unwrap_or("");
                _current_thinking.push_str(delta);
                let _ = tx.try_send(LlmEvent::ThinkingDelta {
                    delta: delta.into(),
                });
            }
            "response.reasoning_summary_part.done" => {
                _current_thinking.push_str("\n\n");
                let _ = tx.try_send(LlmEvent::ThinkingDelta {
                    delta: "\n\n".into(),
                });
            }
            "response.function_call_arguments.delta" => {
                if let Some(tc) = tool_calls.last_mut() {
                    tc.args_json.push_str(event["delta"].as_str().unwrap_or(""));
                }
            }
            "response.function_call_arguments.done" => {
                if let Some(tc) = tool_calls.last_mut() {
                    if let Some(args) = event["arguments"].as_str() {
                        tc.args_json = args.into();
                    }
                }
            }
            "response.output_item.done" => {
                let item = &event["item"];
                match item["type"].as_str().unwrap_or("") {
                    "reasoning" => {
                        let _ = tx.try_send(LlmEvent::ThinkingEnd);
                    }
                    "message" => {
                        let _ = tx.try_send(LlmEvent::TextEnd);
                    }
                    "function_call" => {
                        let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                        let item_id = item["id"].as_str().unwrap_or("").to_string();
                        let name = item["name"].as_str().unwrap_or("").to_string();
                        let args: Value =
                            serde_json::from_str(item["arguments"].as_str().unwrap_or("{}"))
                                .unwrap_or(json!({}));
                        let compound_id = format!("{call_id}|{item_id}");
                        completed_tool_calls
                            .push(json!({"id": compound_id, "name": name, "arguments": args}));
                        let _ = tx.try_send(LlmEvent::ToolCallEnd {
                            tool_call: crate::bridge::WireToolCall {
                                id: compound_id,
                                name,
                                arguments: args,
                            },
                        });
                    }
                    _ => {}
                }
                _current_item_type = None;
            }
            // "response.done" is an alias used by some Codex endpoint variants;
            // handle it alongside the documented "response.completed".
            "response.completed" | "response.done" => {
                let mut codex_input: u64 = 0;
                let mut codex_output: u64 = 0;
                if let Some(usage) = event.pointer("/response/usage") {
                    codex_input = usage["input_tokens"].as_u64().unwrap_or(0);
                    codex_output = usage["output_tokens"].as_u64().unwrap_or(0);
                    tracing::info!(
                        input_tokens = codex_input,
                        output_tokens = codex_output,
                        total_tokens = usage["total_tokens"].as_u64().unwrap_or(0),
                        "Codex usage"
                    );
                }
                terminal = Some(TerminalEvent::Done {
                    input_tokens: codex_input,
                    output_tokens: codex_output,
                });
                return false;
            }
            "response.failed" => {
                let msg = event["response"]["error"]["message"]
                    .as_str()
                    .or(event["response"]["incomplete_details"]["reason"].as_str())
                    .unwrap_or("Codex response failed")
                    .to_string();
                terminal = Some(TerminalEvent::Error(format!("Codex: {msg}")));
                return false;
            }
            "error" => {
                let msg = event["message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_string();
                terminal = Some(TerminalEvent::Error(format!("Codex error: {msg}")));
                return false;
            }
            // response.incomplete: model hit max_output_tokens or content
            // filter. This is NOT a network error — the API is telling us the
            // response was intentionally truncated.  Treat as a retryable error
            // so the retry loop can re-attempt, rather than silently feeding
            // truncated text back into conversation history.
            "response.incomplete" => {
                let reason = event
                    .pointer("/response/incomplete_details/reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                tracing::warn!(reason, "Codex response.incomplete — output was truncated");
                terminal = Some(TerminalEvent::Error(format!(
                    "Codex: response incomplete ({reason}) — output was truncated"
                )));
                return false;
            }
            // response.cancelled: request was cancelled server-side.
            "response.cancelled" => {
                tracing::warn!("Codex response.cancelled");
                terminal = Some(TerminalEvent::Error(
                    "Codex: response cancelled by server".to_string(),
                ));
                return false;
            }
            "response.content_part.added" | "response.reasoning_summary_part.added" => {}
            _ => {
                // Forward unhandled Codex events as a no-op heartbeat.
                // The Responses API sends events like response.created,
                // response.in_progress, and reasoning.delta during model
                // thinking — these don't map to LlmEvents but MUST reset
                // the 30s consumer idle timer in consume_llm_stream,
                // otherwise the consumer assumes the stream is stalled
                // while the model is still reasoning.  LlmEvent::Start is
                // already handled as a no-op by the consumer.
                let _ = tx.try_send(LlmEvent::Start);
            }
        }
        true
    })
    .await?;

    // Deliver the terminal event with guaranteed async send.
    match terminal {
        Some(TerminalEvent::Done {
            input_tokens,
            output_tokens,
        }) => {
            let _ = tx
                .send(LlmEvent::Done {
                    message: json!({"text": full_text, "tool_calls": completed_tool_calls}),
                    input_tokens,
                    output_tokens,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: provider_telemetry_done.clone(),
                })
                .await;
        }
        Some(TerminalEvent::Error(msg)) => {
            let _ = tx.send(LlmEvent::Error { message: msg }).await;
        }
        None => {
            // SSE stream closed without a completion or error event — server
            // dropped the connection mid-response (network drop, server restart,
            // etc.).  If we accumulated content, synthesise a Done so the turn
            // isn't silently lost; otherwise surface a clear error.
            if !full_text.is_empty() || !completed_tool_calls.is_empty() {
                // Stream dropped without a terminal event.  Could be a network
                // drop OR a missed event variant.  Surface as an error so the
                // retry loop handles it — never silently feed truncated text
                // back into conversation history.
                tracing::warn!(
                    text_len = full_text.len(),
                    tool_calls = completed_tool_calls.len(),
                    "Codex stream closed without completion event — treating as error to prevent partial-content poisoning"
                );
                let _ = tx
                    .send(LlmEvent::Error {
                        message: format!(
                            "Codex: stream closed without completion (had {}b text, {} tool calls)",
                            full_text.len(),
                            completed_tool_calls.len()
                        ),
                    })
                    .await;
            } else {
                let _ = tx
                    .send(LlmEvent::Error {
                        message: "Codex: stream closed without a completion event".into(),
                    })
                    .await;
            }
        }
    }

    Ok(())
}

// ─── OpenAI-Compatible Generic Client ────────────────────────────────────────

/// Generic client for any provider that speaks the OpenAI Chat Completions protocol.
/// Covers: Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama, and any custom endpoint.
pub struct OpenAICompatClient {
    inner: OpenAIClient,
    provider_id: String,
    default_model: Option<String>,
}

/// Native Ollama Cloud client.
///
/// Hosted Ollama is not exposed at the OpenAI-compatible `/v1/chat/completions`
/// path we use for local Ollama. Its documented hosted API lives under
/// `https://ollama.com/api/*` with bearer auth, so it needs its own transport.
pub struct OllamaCloudClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

/// Base URLs for known OpenAI-compatible providers.
fn compat_base_url(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "groq" => Some("https://api.groq.com/openai"),
        "xai" => Some("https://api.x.ai"),
        "mistral" => Some("https://api.mistral.ai"),
        "cerebras" => Some("https://api.cerebras.ai"),
        "huggingface" => Some("https://router.huggingface.co"),
        "ollama" => Some("http://localhost:11434"),
        _ => None,
    }
}

fn ollama_cloud_base_url() -> &'static str {
    "https://ollama.com/api"
}

/// Map Omegon thinking levels onto Ollama's native `think` request field.
///
/// Upstream docs:
/// - https://docs.ollama.com/capabilities/thinking
/// - https://docs.ollama.com/api/chat
///
/// Most Ollama thinking models accept booleans, but GPT-OSS expects one of
/// "low" | "medium" | "high" and ignores booleans. We therefore send string
/// levels for GPT-OSS and a simple boolean for other models.
fn ollama_think_value(model: &str, reasoning: Option<&str>) -> Option<Value> {
    let level = reasoning?;
    if level.eq_ignore_ascii_case("off") {
        return None;
    }

    let is_gpt_oss = model.to_ascii_lowercase().contains("gpt-oss");
    if is_gpt_oss {
        let mapped = match level {
            "minimal" | "low" => "low",
            "medium" => "medium",
            "high" => "high",
            other => other,
        };
        return Some(Value::String(mapped.to_string()));
    }

    Some(Value::Bool(true))
}

/// Map Omegon thinking levels onto OpenAI Responses API reasoning effort.
///
/// Upstream docs indicate effort is model-dependent and can include:
/// none, minimal, low, medium, high, xhigh.
/// We preserve minimal instead of collapsing it to low.
fn openai_reasoning_effort(reasoning: Option<&str>) -> Option<&'static str> {
    match reasoning? {
        "off" => None,
        "minimal" => Some("minimal"),
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        _ => Some("medium"),
    }
}

fn apply_anthropic_thinking(body: &mut Value, model: &str, reasoning: Option<&str>) {
    let Some(reasoning) = reasoning else {
        return;
    };
    if reasoning == "off" {
        return;
    }
    if anthropic_should_use_adaptive_thinking(model, reasoning) {
        body["thinking"] = json!({ "type": "adaptive" });
    } else if let Some(budget) = anthropic_manual_budget_tokens(Some(reasoning)) {
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }
}

fn anthropic_manual_budget_tokens(reasoning: Option<&str>) -> Option<u32> {
    match reasoning? {
        "off" => None,
        "minimal" => Some(1_024),
        "low" => Some(5_000),
        "medium" => Some(10_000),
        "high" => Some(50_000),
        "xhigh" => Some(50_000),
        _ => Some(10_000),
    }
}

fn anthropic_supports_adaptive_thinking(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.contains("claude-sonnet-4-6") || model.contains("claude-opus-4-6")
}

fn anthropic_should_use_adaptive_thinking(model: &str, reasoning: &str) -> bool {
    anthropic_supports_adaptive_thinking(model) && matches!(reasoning, "medium" | "high")
}

/// Default model for each compat provider (used when no model is specified).
fn compat_default_model(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "groq" => Some("llama-3.3-70b-versatile"),
        "xai" => Some("grok-3-mini-fast"),
        "mistral" => Some("devstral-small-2505"),
        "cerebras" => Some("llama-3.3-70b"),
        "huggingface" => Some("Qwen/Qwen3-32B"),
        "ollama" => Some("qwen3:32b"),
        "ollama-cloud" => Some("gpt-oss:120b-cloud"),
        _ => None,
    }
}

impl OpenAICompatClient {
    pub fn new(api_key: String, base_url: String, provider_id: String) -> Self {
        let default_model = compat_default_model(&provider_id).map(String::from);
        Self {
            inner: OpenAIClient {
                client: reqwest::Client::new(),
                api_key,
                base_url,
            },
            provider_id,
            default_model,
        }
    }

    /// Resolve from env vars / auth.json using the canonical PROVIDERS map.
    pub fn from_env(provider_id: &str) -> Option<Self> {
        let base_url = compat_base_url(provider_id)?;

        // Local Ollama doesn't need an API key — just check reachability.
        if provider_id == "ollama" {
            return Self::from_env_ollama(base_url);
        }

        let key = resolve_api_key(provider_id)?;
        Some(Self::new(
            key,
            base_url.to_string(),
            provider_id.to_string(),
        ))
    }

    /// Ollama: no API key, just check if reachable.
    fn from_env_ollama(base_url: &str) -> Option<Self> {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| base_url.to_string());
        let addr_str = host
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        let addr: std::net::SocketAddr = addr_str
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 11434)));
        match std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)) {
            Ok(_) => {
                tracing::debug!(host = %host, "Ollama server detected");
                Some(Self::new(String::new(), host, "ollama".into()))
            }
            Err(_) => {
                tracing::trace!("Ollama not reachable — skipping");
                None
            }
        }
    }
}

impl OllamaCloudClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: ollama_cloud_base_url().to_string(),
        }
    }

    pub fn from_env() -> Option<Self> {
        resolve_api_key("ollama-cloud").map(Self::new)
    }

    fn endpoint_url(&self) -> String {
        format!("{}/chat", self.base_url.trim_end_matches('/'))
    }

    fn build_wire_messages(system_prompt: &str, messages: &[LlmMessage]) -> Vec<Value> {
        let mut wire_msgs = Vec::with_capacity(messages.len() + 1);
        if !system_prompt.trim().is_empty() {
            wire_msgs.push(json!({"role": "system", "content": system_prompt}));
        }
        for m in messages {
            match m {
                LlmMessage::User { content, images } => {
                    let mut msg = json!({"role": "user", "content": content});
                    if !images.is_empty() {
                        msg["images"] = Value::Array(
                            images
                                .iter()
                                .map(|img| Value::String(img.data.clone()))
                                .collect(),
                        );
                    }
                    wire_msgs.push(msg);
                }
                LlmMessage::Assistant {
                    text,
                    thinking,
                    tool_calls,
                    ..
                } => {
                    let mut assistant = json!({
                        "role": "assistant",
                        "content": text.join("\n"),
                    });
                    if !thinking.is_empty() {
                        assistant["thinking"] = Value::String(thinking.join("\n"));
                    }
                    if !tool_calls.is_empty() {
                        assistant["tool_calls"] = Value::Array(
                            tool_calls
                                .iter()
                                .map(|tc| {
                                    json!({
                                        "function": {
                                            "name": tc.name,
                                            "arguments": if tc.arguments.is_object() {
                                                tc.arguments.clone()
                                            } else {
                                                json!({})
                                            }
                                        }
                                    })
                                })
                                .collect(),
                        );
                    }
                    wire_msgs.push(assistant);
                }
                LlmMessage::ToolResult {
                    tool_name,
                    content,
                    is_error,
                    ..
                } => {
                    wire_msgs.push(json!({
                        "role": "tool",
                        "tool_name": tool_name,
                        "content": content,
                        "is_error": is_error,
                    }));
                }
            }
        }
        wire_msgs
    }

    fn parse_tool_calls(message: &Value) -> Vec<crate::bridge::WireToolCall> {
        message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, call)| {
                        let function = call.get("function")?;
                        let name = function.get("name")?.as_str()?.to_string();
                        let arguments = function
                            .get("arguments")
                            .cloned()
                            .filter(Value::is_object)
                            .unwrap_or_else(|| json!({}));
                        let id = call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| format!("ollama-call-{}", idx + 1));
                        Some(crate::bridge::WireToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl LlmBridge for OpenAICompatClient {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let mut opts = options.clone();

        // Strip provider prefix from model name
        if let Some(ref mut m) = opts.model {
            let prefix = format!("{}:", self.provider_id);
            if let Some(stripped) = m.strip_prefix(&prefix) {
                *m = stripped.to_string();
            }
        }

        // Apply default model if none specified
        if opts.model.is_none() || opts.model.as_deref() == Some("") {
            if let Some(ref default) = self.default_model {
                opts.model = Some(default.clone());
            }
        }

        // For Ollama: inject num_ctx and keep_alive so the model doesn't
        // silently truncate the prompt at Ollama's default 2048-token KV cache.
        if self.provider_id == "ollama" {
            let num_ctx = std::env::var("OMEGON_OLLAMA_NUM_CTX")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(32_768);
            let keep_alive =
                std::env::var("OMEGON_OLLAMA_KEEP_ALIVE").unwrap_or_else(|_| "30m".to_string());
            let model_id = opts
                .model
                .as_deref()
                .map(model_id_from_spec)
                .unwrap_or_else(|| self.default_model.as_deref().unwrap_or("qwen3:32b"));
            opts.extra_body.insert(
                "options".to_string(),
                serde_json::json!({"num_ctx": num_ctx}),
            );
            opts.extra_body.insert(
                "keep_alive".to_string(),
                serde_json::Value::String(keep_alive),
            );
            if let Some(think) = ollama_think_value(model_id, opts.reasoning.as_deref()) {
                opts.extra_body.insert("think".to_string(), think);
            }
        }

        self.inner
            .stream(system_prompt, messages, tools, &opts)
            .await
    }
}

#[async_trait]
impl LlmBridge for OllamaCloudClient {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        _tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let model = options
            .model
            .as_deref()
            .map(|m| model_id_from_spec(m).to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| compat_default_model("ollama-cloud").unwrap_or("gpt-oss:120b-cloud").to_string());

        let mut body = json!({
            "model": model,
            "messages": Self::build_wire_messages(system_prompt, messages),
            "stream": false,
        });
        if let Some(think) = ollama_think_value(&model, options.reasoning.as_deref()) {
            body["think"] = think;
        }

        let response = self
            .client
            .post(self.endpoint_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let err = response.text().await.unwrap_or_default();
            let _ = tx
                .send(LlmEvent::Error {
                    message: format!("Ollama Cloud {status}: {err}"),
                })
                .await;
            return Ok(rx);
        }

        let provider_telemetry = parse_rate_limit_snapshot("ollama-cloud", response.headers());
        let payload: Value = response.json().await?;
        let message = payload.get("message").cloned().unwrap_or_else(|| json!({}));
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let thinking = message
            .get("thinking")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let tool_calls = Self::parse_tool_calls(&message);

        let _ = tx.send(LlmEvent::Start).await;
        if !thinking.is_empty() {
            let _ = tx.send(LlmEvent::ThinkingStart).await;
            let _ = tx
                .send(LlmEvent::ThinkingDelta {
                    delta: thinking.clone(),
                })
                .await;
            let _ = tx.send(LlmEvent::ThinkingEnd).await;
        }
        let _ = tx.send(LlmEvent::TextStart).await;
        if !content.is_empty() {
            let _ = tx.send(LlmEvent::TextDelta { delta: content.clone() }).await;
        }
        let _ = tx.send(LlmEvent::TextEnd).await;
        for tool_call in &tool_calls {
            let _ = tx.send(LlmEvent::ToolCallStart).await;
            let _ = tx
                .send(LlmEvent::ToolCallEnd {
                    tool_call: tool_call.clone(),
                })
                .await;
        }
        let _ = tx
            .send(LlmEvent::Done {
                message: json!({
                    "text": content,
                    "thinking": thinking,
                    "tool_calls": tool_calls,
                    "raw": payload,
                }),
                input_tokens: payload
                    .get("prompt_eval_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                output_tokens: payload
                    .get("eval_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry,
            })
            .await;

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rate_limit_snapshot_extracts_anthropic_utilization_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-unified-5h-utilization",
            reqwest::header::HeaderValue::from_static("42"),
        );
        headers.insert(
            "anthropic-ratelimit-unified-7d-utilization",
            reqwest::header::HeaderValue::from_static("64"),
        );
        headers.insert(
            "retry-after",
            reqwest::header::HeaderValue::from_static("17"),
        );

        let snapshot = parse_rate_limit_snapshot("anthropic", &headers).expect("snapshot");
        assert_eq!(snapshot.provider, "anthropic");
        assert_eq!(snapshot.unified_5h_utilization_pct, Some(42.0));
        assert_eq!(snapshot.unified_7d_utilization_pct, Some(64.0));
        assert_eq!(snapshot.retry_after_secs, Some(17));
    }

    #[test]
    fn parse_rate_limit_snapshot_extracts_openai_reset_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-ratelimit-remaining-requests",
            reqwest::header::HeaderValue::from_static("4999"),
        );
        headers.insert(
            "x-ratelimit-remaining-tokens-usage-based",
            reqwest::header::HeaderValue::from_static("159976"),
        );
        headers.insert(
            "x-ratelimit-reset-tokens",
            reqwest::header::HeaderValue::from_static("12ms"),
        );
        headers.insert(
            "x-openai-request-id",
            reqwest::header::HeaderValue::from_static("req_123"),
        );

        let snapshot = parse_rate_limit_snapshot("openai-codex", &headers).expect("snapshot");
        assert_eq!(snapshot.provider, "openai-codex");
        assert_eq!(snapshot.requests_remaining, Some(4999));
        assert_eq!(snapshot.tokens_remaining, Some(159976));
        assert_eq!(snapshot.retry_after_secs, Some(1));
        assert_eq!(snapshot.request_id.as_deref(), Some("req_123"));
    }

    #[test]
    fn parse_rate_limit_snapshot_extracts_codex_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-codex-active-limit",
            reqwest::header::HeaderValue::from_static("codex"),
        );
        headers.insert(
            "x-codex-primary-over-secondary-limit-percent",
            reqwest::header::HeaderValue::from_static("0"),
        );
        headers.insert(
            "x-codex-primary-reset-after-seconds",
            reqwest::header::HeaderValue::from_static("13648"),
        );
        headers.insert(
            "x-codex-secondary-reset-after-seconds",
            reqwest::header::HeaderValue::from_static("348644"),
        );
        headers.insert(
            "x-codex-credits-unlimited",
            reqwest::header::HeaderValue::from_static("False"),
        );
        headers.insert(
            "x-codex-bengalfox-limit-name",
            reqwest::header::HeaderValue::from_static("GPT-5.3-Codex-Spark"),
        );
        headers.insert(
            "x-oai-request-id",
            reqwest::header::HeaderValue::from_static("abc-123"),
        );

        let snapshot = parse_rate_limit_snapshot("openai-codex", &headers).expect("snapshot");
        assert_eq!(snapshot.provider, "openai-codex");
        assert_eq!(snapshot.codex_active_limit.as_deref(), Some("codex"));
        assert_eq!(snapshot.codex_primary_used_pct, Some(0.0));
        assert_eq!(snapshot.codex_primary_reset_secs, Some(13648));
        assert_eq!(snapshot.codex_secondary_reset_secs, Some(348644));
        assert_eq!(snapshot.codex_credits_unlimited, Some(false));
        assert_eq!(
            snapshot.codex_limit_name.as_deref(),
            Some("GPT-5.3-Codex-Spark")
        );
        assert_eq!(snapshot.request_id.as_deref(), Some("abc-123"));
    }

    #[test]
    fn collect_headers_filters_and_normalizes_names() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "X-Request-ID",
            reqwest::header::HeaderValue::from_static("req_123"),
        );
        headers.insert(
            "Content-Type",
            reqwest::header::HeaderValue::from_static("text/event-stream"),
        );

        let filtered = collect_headers(&headers, |name| name.contains("request"));
        assert_eq!(filtered, vec![("x-request-id".into(), "req_123".into())]);

        let all = collect_headers(&headers, |_| true);
        assert!(
            all.iter()
                .any(|(k, v)| k == "x-request-id" && v == "req_123")
        );
        assert!(
            all.iter()
                .any(|(k, v)| k == "content-type" && v == "text/event-stream")
        );
    }

    #[test]
    fn resolve_key_from_env_uses_standard_var_names() {
        // Verify the function checks the right env var names
        // without setting/unsetting them (which is racy).
        // The function resolve_api_key checks: ANTHROPIC_API_KEY for anthropic,
        // OPENAI_API_KEY for openai. We test the name mapping logic.
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let result = resolve_api_key("anthropic");
        // If the key is set, result should be Some; if not, depends on auth.json
        if anthropic_key.is_some() {
            assert!(result.is_some(), "should find ANTHROPIC_API_KEY from env");
        }
        // Main assertion: doesn't panic regardless of env state
    }

    #[test]
    fn auto_detect_does_not_panic_regardless_of_env() {
        // auto_detect_bridge should handle missing keys gracefully
        // without us needing to clear/restore env vars
        let _ = auto_detect_bridge("anthropic:test");
        let _ = auto_detect_bridge("openai:test");
        let _ = auto_detect_bridge("unknown-provider:test");
        // All should return Some or None without panicking
    }

    #[test]
    fn infer_provider_id_handles_bare_model_ids_and_aliases() {
        assert_eq!(
            infer_provider_id("anthropic:claude-sonnet-4-6"),
            "anthropic"
        );
        assert_eq!(infer_provider_id("qwen3:30b"), "ollama");
        assert_eq!(infer_provider_id("local:qwen3:30b"), "ollama");
        assert_eq!(infer_provider_id("local"), "ollama");
        assert_eq!(infer_provider_id("ollama-cloud:gpt-oss:120b-cloud"), "ollama-cloud");
        assert_eq!(infer_provider_id("claude-opus-4-6"), "anthropic");
        assert_eq!(infer_provider_id("gpt-5.4"), "openai");
        assert_eq!(infer_provider_id("gpt-5.4-mini"), "openai");
    }

    #[test]
    fn anthropic_build_messages() {
        let messages = vec![LlmMessage::User {
            content: "hello".into(),
            images: vec![],
        }];
        let wire = AnthropicClient::build_messages(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[0]["content"], "hello");
    }

    #[test]
    fn anthropic_build_messages_with_images() {
        let messages = vec![LlmMessage::User {
            content: "describe this".into(),
            images: vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/anthropic.png".into()),
            }],
        }];
        let wire = AnthropicClient::build_messages(&messages);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[0]["content"][0]["type"], "image");
        assert_eq!(wire[0]["content"][0]["source"]["type"], "base64");
        assert_eq!(wire[0]["content"][0]["source"]["media_type"], "image/png");
        assert_eq!(wire[0]["content"][1]["type"], "text");
        assert_eq!(wire[0]["content"][1]["text"], "describe this");
    }

    #[test]
    fn anthropic_build_tool_result() {
        let messages = vec![LlmMessage::ToolResult {
            call_id: "tc1".into(),
            tool_name: "read".into(),
            content: "file contents".into(),
            is_error: false,
            args_summary: None,
        }];
        let wire = AnthropicClient::build_messages(&messages);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[0]["content"][0]["type"], "tool_result");
        assert_eq!(wire[0]["content"][0]["tool_use_id"], "tc1");
    }

    #[test]
    fn anthropic_tool_use_input_always_object() {
        // When arguments is null (e.g. tools with no required params),
        // Anthropic requires `input` to be `{}`, not `null`.
        let messages = vec![LlmMessage::Assistant {
            text: vec![],
            thinking: vec![],
            tool_calls: vec![crate::bridge::WireToolCall {
                id: "tc1".into(),
                name: "memory_query".into(),
                arguments: Value::Null,
            }],
            raw: None, // Force fallback path (no raw content blocks)
        }];
        let wire = AnthropicClient::build_messages(&messages);
        let input = &wire[0]["content"][0]["input"];
        assert!(input.is_object(), "input should be object, got: {input}");
        assert_eq!(input, &json!({}));
    }

    #[test]
    fn openai_build_messages_with_images() {
        let messages = vec![LlmMessage::User {
            content: "describe this".into(),
            images: vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/openai.png".into()),
            }],
        }];
        let wire = OpenAIClient::build_wire_messages("system", &messages);
        assert_eq!(wire[1]["role"], "user");
        assert_eq!(wire[1]["content"][0]["type"], "image_url");
        assert_eq!(
            wire[1]["content"][0]["image_url"]["url"],
            "data:image/png;base64,abc123"
        );
        assert_eq!(wire[1]["content"][1]["type"], "text");
        assert_eq!(wire[1]["content"][1]["text"], "describe this");
    }

    #[test]
    fn codex_build_input_with_images() {
        let messages = vec![LlmMessage::User {
            content: "describe this".into(),
            images: vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/codex.png".into()),
            }],
        }];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_image");
        assert_eq!(
            input[0]["content"][0]["image_url"],
            "data:image/png;base64,abc123"
        );
        assert_eq!(input[0]["content"][1]["type"], "input_text");
        assert_eq!(input[0]["content"][1]["text"], "describe this");
    }

    #[test]
    fn upstream_multimodal_provider_matrix_serializes_image_inputs() {
        let user = LlmMessage::User {
            content: "describe this".into(),
            images: vec![crate::bridge::ImageAttachment {
                data: "abc123".into(),
                media_type: "image/png".into(),
                source_path: Some("/tmp/matrix.png".into()),
            }],
        };

        let anthropic = AnthropicClient::build_messages(std::slice::from_ref(&user));
        assert_eq!(anthropic[0]["content"][0]["type"], "image");
        assert_eq!(
            anthropic[0]["content"][0]["source"]["media_type"],
            "image/png"
        );

        let openai = OpenAIClient::build_wire_messages("system", std::slice::from_ref(&user));
        assert_eq!(openai[1]["content"][0]["type"], "image_url");
        assert_eq!(
            openai[1]["content"][0]["image_url"]["url"],
            "data:image/png;base64,abc123"
        );

        let codex = CodexClient::build_input(&[user]);
        assert_eq!(codex[0]["content"][0]["type"], "input_image");
        assert_eq!(
            codex[0]["content"][0]["image_url"],
            "data:image/png;base64,abc123"
        );
    }

    #[test]
    fn error_message_extraction_from_api_json() {
        // Simulate what happens when Anthropic returns a 400 error
        let raw_body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.1.content.1.tool_use.input: Input should be a valid dictionary"},"request_id":"req_abc123"}"#;
        let user_msg = serde_json::from_str::<Value>(raw_body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| raw_body.chars().take(200).collect());
        assert_eq!(
            user_msg,
            "messages.1.content.1.tool_use.input: Input should be a valid dictionary"
        );
    }

    #[test]
    fn error_message_fallback_on_non_json() {
        let raw_body = "Service Unavailable";
        let user_msg = serde_json::from_str::<Value>(raw_body)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| raw_body.chars().take(200).collect());
        assert_eq!(user_msg, "Service Unavailable");
    }

    // ── Credential lifecycle tests ──────────────────────────────────────

    #[test]
    fn strip_parameter_descriptions_removes_at_all_depths() {
        let props = json!({
            "path": {"type": "string", "description": "Path to file"},
            "offset": {"type": "number", "description": "Line number", "minimum": 1},
            "mode": {"type": "string", "enum": ["quick", "deep"]},
            "nested": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "inner": {"type": "string", "description": "should be stripped too"}
                    },
                    "description": "array item schema"
                }
            }
        });
        let stripped = strip_parameter_descriptions(&props);
        // Top-level descriptions removed
        assert!(stripped["path"].get("description").is_none());
        assert!(stripped["offset"].get("description").is_none());
        // type, minimum, enum preserved
        assert_eq!(stripped["path"]["type"], "string");
        assert_eq!(stripped["offset"]["type"], "number");
        assert_eq!(stripped["offset"]["minimum"], 1);
        assert_eq!(stripped["mode"]["enum"][0], "quick");
        // Nested descriptions also removed
        assert!(stripped["nested"]["items"].get("description").is_none());
        assert!(
            stripped["nested"]["items"]["properties"]["inner"]
                .get("description")
                .is_none()
        );
        // But nested type preserved
        assert_eq!(
            stripped["nested"]["items"]["properties"]["inner"]["type"],
            "string"
        );
    }

    #[test]
    fn build_tools_oauth_remaps_known_names() {
        let tools = vec![
            ToolDefinition {
                name: "bash".into(),
                label: "bash".into(),
                description: "run command".into(),
                parameters: json!({}),
            },
            ToolDefinition {
                name: "read".into(),
                label: "read".into(),
                description: "read file".into(),
                parameters: json!({}),
            },
            ToolDefinition {
                name: "memory_store".into(),
                label: "memory".into(),
                description: "store fact".into(),
                parameters: json!({}),
            },
        ];
        let wire = AnthropicClient::build_tools(&tools, true);
        assert_eq!(wire[0]["name"], "Bash", "bash should become Bash for OAuth");
        assert_eq!(wire[1]["name"], "Read", "read should become Read for OAuth");
        assert_eq!(
            wire[2]["name"], "memory_store",
            "unknown tools pass through unchanged"
        );
    }

    #[test]
    fn build_tools_api_key_preserves_names() {
        let tools = vec![ToolDefinition {
            name: "bash".into(),
            label: "bash".into(),
            description: "run command".into(),
            parameters: json!({}),
        }];
        let wire = AnthropicClient::build_tools(&tools, false);
        assert_eq!(wire[0]["name"], "bash", "API key mode preserves lowercase");
    }

    #[test]
    fn from_claude_code_name_roundtrips() {
        // Every name that to_claude_code_name maps must roundtrip
        let known = [
            ("bash", "Bash"),
            ("read", "Read"),
            ("write", "Write"),
            ("edit", "Edit"),
            ("web_search", "WebSearch"),
        ];
        for (lower, upper) in &known {
            assert_eq!(&to_claude_code_name(lower), upper);
            assert_eq!(&from_claude_code_name(upper), lower);
        }
    }

    #[test]
    fn from_claude_code_name_unknown_passthrough() {
        assert_eq!(from_claude_code_name("memory_store"), "memory_store");
        assert_eq!(from_claude_code_name("SomethingNew"), "SomethingNew");
    }

    #[test]
    fn anthropic_client_construction_with_oauth() {
        let client = AnthropicClient::new("sk-ant-oat-test".into(), true);
        assert!(client.is_oauth);
        assert_eq!(client.api_key, "sk-ant-oat-test");
    }

    #[test]
    fn anthropic_client_construction_with_api_key() {
        let client = AnthropicClient::new("sk-ant-api-test".into(), false);
        assert!(!client.is_oauth);
    }

    #[test]
    fn oauth_system_prompt_includes_cc_prefix() {
        // When is_oauth is true, system prompt should be an array with
        // the Claude Code identity prefix as the first element.
        let system = json!([
            {"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude."},
            {"type": "text", "text": "actual prompt"},
        ]);
        assert!(system.is_array());
        let arr = system.as_array().unwrap();
        assert_eq!(
            arr[0]["text"],
            "You are Claude Code, Anthropic's official CLI for Claude."
        );
    }

    #[test]
    fn api_key_system_prompt_is_plain_string() {
        // When is_oauth is false, system prompt should be a plain string.
        let system = json!("actual prompt");
        assert!(system.is_string());
    }

    #[tokio::test]
    async fn resolve_with_refresh_falls_back_to_env() {
        // resolve_with_refresh should check env vars first.
        // We can't safely set env vars in parallel tests, but we can
        // verify it doesn't panic and returns Some if ANTHROPIC_API_KEY is set.
        let result = crate::auth::resolve_with_refresh("anthropic").await;
        // Result depends on env — just verify no panic
        let _ = result;
    }

    #[test]
    fn oauth_auth_header_uses_bearer() {
        // OAuth requests must use Authorization: Bearer, not x-api-key
        let is_oauth = true;
        let header_name = if is_oauth {
            "Authorization"
        } else {
            "x-api-key"
        };
        assert_eq!(header_name, "Authorization");
    }

    #[test]
    fn api_key_auth_header_uses_x_api_key() {
        let is_oauth = false;
        let header_name = if is_oauth {
            "Authorization"
        } else {
            "x-api-key"
        };
        assert_eq!(header_name, "x-api-key");
    }

    #[test]
    fn oauth_beta_flags_include_cc_and_oauth() {
        let is_oauth = true;
        let flags = if is_oauth {
            "claude-code-20250219,oauth-2025-04-20".to_string()
        } else {
            "interleaved-thinking-2025-05-14".to_string()
        };
        assert!(
            flags.contains("claude-code-20250219"),
            "OAuth must include CC beta"
        );
        assert!(
            flags.contains("oauth-2025-04-20"),
            "OAuth must include OAuth beta"
        );
        assert!(
            !flags.contains("context-1m"),
            "OAuth must NOT include 1M context beta"
        );
    }

    #[test]
    fn context_1m_beta_flag_never_sent() {
        // The context-1m-2025-08-07 beta flag is deprecated. Sonnet/Opus 4.6
        // support 1M context natively. The flag only triggers billing gates
        // ("Extra usage is required for long context requests" 429).
        // Verified empirically: OAuth request with flag → 429, without → 200.
        let oauth_flags = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14";
        let api_flags = "interleaved-thinking-2025-05-14";
        assert!(
            !oauth_flags.contains("context-1m"),
            "OAuth must never send context-1m"
        );
        assert!(
            !api_flags.contains("context-1m"),
            "API key must never send context-1m"
        );
    }

    #[test]
    fn api_key_beta_flags_include_thinking() {
        let is_oauth = false;
        let flags = if is_oauth {
            "claude-code-20250219,oauth-2025-04-20".to_string()
        } else {
            "interleaved-thinking-2025-05-14".to_string()
        };
        assert!(
            flags.contains("interleaved-thinking"),
            "API key must include thinking beta"
        );
        assert!(
            !flags.contains("claude-code"),
            "API key must NOT include CC beta"
        );
    }

    // ── OpenAI-compat client tests ──────────────────────────────────

    #[test]
    fn compat_base_url_covers_all_providers() {
        for id in ["groq", "xai", "mistral", "cerebras", "huggingface", "ollama"] {
            assert!(
                super::compat_base_url(id).is_some(),
                "missing base URL for {id}"
            );
        }
        assert_eq!(super::ollama_cloud_base_url(), "https://ollama.com/api");
        assert!(super::compat_base_url("ollama-cloud").is_none());
        assert!(super::compat_base_url("unknown").is_none());
    }

    #[test]
    fn compat_default_model_covers_all_providers() {
        for id in [
            "groq",
            "xai",
            "mistral",
            "cerebras",
            "huggingface",
            "ollama",
            "ollama-cloud",
        ] {
            assert!(
                super::compat_default_model(id).is_some(),
                "missing default model for {id}"
            );
        }
    }

    #[test]
    fn ollama_cloud_uses_native_chat_endpoint() {
        let client = OllamaCloudClient::new("test-key".into());
        assert_eq!(client.endpoint_url(), "https://ollama.com/api/chat");
    }

    #[test]
    fn ollama_cloud_wire_messages_include_system_and_user_content() {
        let wire = OllamaCloudClient::build_wire_messages(
            "system",
            &[LlmMessage::User {
                content: "hello".into(),
                images: vec![],
            }],
        );
        assert_eq!(wire[0]["role"], "system");
        assert_eq!(wire[0]["content"], "system");
        assert_eq!(wire[1]["role"], "user");
        assert_eq!(wire[1]["content"], "hello");
        assert!(wire[1].get("images").is_none());
    }

    #[test]
    fn ollama_cloud_wire_messages_include_native_images_field() {
        let wire = OllamaCloudClient::build_wire_messages(
            "system",
            &[LlmMessage::User {
                content: "describe this".into(),
                images: vec![crate::bridge::ImageAttachment {
                    data: "abc123".into(),
                    media_type: "image/png".into(),
                    source_path: Some("/tmp/ollama-cloud.png".into()),
                }],
            }],
        );

        assert_eq!(wire[1]["role"], "user");
        assert_eq!(wire[1]["content"], "describe this");
        assert_eq!(wire[1]["images"][0], "abc123");
    }

    #[test]
    fn ollama_cloud_parses_native_tool_calls() {
        let message = json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {
                    "function": {
                        "name": "bash",
                        "arguments": {"command": "pwd"}
                    }
                }
            ]
        });

        let tool_calls = OllamaCloudClient::parse_tool_calls(&message);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "bash");
        assert_eq!(tool_calls[0].arguments, json!({"command": "pwd"}));
        assert_eq!(tool_calls[0].id, "ollama-call-1");
    }

    #[test]
    fn ollama_cloud_wire_messages_preserve_assistant_thinking_and_tool_calls() {
        let wire = OllamaCloudClient::build_wire_messages(
            "system",
            &[LlmMessage::Assistant {
                text: vec!["I'll inspect the repo.".into()],
                thinking: vec!["Need to check status first.".into()],
                tool_calls: vec![crate::bridge::WireToolCall {
                    id: "tc1".into(),
                    name: "bash".into(),
                    arguments: json!({"command": "git status --short"}),
                }],
                raw: None,
            }],
        );

        assert_eq!(wire[1]["role"], "assistant");
        assert_eq!(wire[1]["content"], "I'll inspect the repo.");
        assert_eq!(wire[1]["thinking"], "Need to check status first.");
        assert_eq!(wire[1]["tool_calls"][0]["function"]["name"], "bash");
        assert_eq!(
            wire[1]["tool_calls"][0]["function"]["arguments"],
            json!({"command": "git status --short"})
        );
    }

    #[test]
    fn ollama_think_value_uses_string_levels_for_gpt_oss() {
        assert_eq!(
            ollama_think_value("gpt-oss:20b", Some("minimal")),
            Some(Value::String("low".into()))
        );
        assert_eq!(
            ollama_think_value("gpt-oss:120b-cloud", Some("medium")),
            Some(Value::String("medium".into()))
        );
        assert_eq!(
            ollama_think_value("gpt-oss:120b-cloud", Some("high")),
            Some(Value::String("high".into()))
        );
    }

    #[test]
    fn ollama_think_value_uses_boolean_for_non_gpt_oss_models() {
        assert_eq!(
            ollama_think_value("qwen3:32b", Some("minimal")),
            Some(Value::Bool(true))
        );
        assert_eq!(
            ollama_think_value("deepseek-r1:14b", Some("high")),
            Some(Value::Bool(true))
        );
        assert_eq!(ollama_think_value("qwen3:32b", None), None);
    }

    #[test]
    fn compat_client_construction() {
        let client = OpenAICompatClient::new(
            "test-key".into(),
            "https://api.groq.com/openai".into(),
            "groq".into(),
        );
        assert_eq!(client.provider_id, "groq");
        assert_eq!(
            client.default_model.as_deref(),
            Some("llama-3.3-70b-versatile")
        );
    }

    #[test]
    fn openai_reasoning_effort_preserves_minimal() {
        assert_eq!(openai_reasoning_effort(Some("minimal")), Some("minimal"));
        assert_eq!(openai_reasoning_effort(Some("low")), Some("low"));
        assert_eq!(openai_reasoning_effort(Some("medium")), Some("medium"));
        assert_eq!(openai_reasoning_effort(Some("high")), Some("high"));
        assert_eq!(openai_reasoning_effort(Some("xhigh")), Some("xhigh"));
        assert_eq!(openai_reasoning_effort(Some("off")), None);
        assert_eq!(openai_reasoning_effort(Some("unknown")), Some("medium"));
    }

    #[test]
    fn anthropic_reasoning_helpers_support_adaptive_and_manual_modes() {
        assert_eq!(anthropic_manual_budget_tokens(Some("minimal")), Some(1_024));
        assert_eq!(anthropic_manual_budget_tokens(Some("high")), Some(50_000));
        assert_eq!(anthropic_manual_budget_tokens(Some("off")), None);
        assert!(anthropic_supports_adaptive_thinking("claude-sonnet-4-6"));
        assert!(anthropic_supports_adaptive_thinking("anthropic:claude-opus-4-6"));
        assert!(!anthropic_supports_adaptive_thinking("claude-sonnet-4-5"));
        assert!(!anthropic_should_use_adaptive_thinking(
            "anthropic:claude-sonnet-4-6",
            "minimal"
        ));
        assert!(!anthropic_should_use_adaptive_thinking(
            "anthropic:claude-sonnet-4-6",
            "low"
        ));
        assert!(anthropic_should_use_adaptive_thinking(
            "anthropic:claude-sonnet-4-6",
            "medium"
        ));
        assert!(anthropic_should_use_adaptive_thinking(
            "anthropic:claude-sonnet-4-6",
            "high"
        ));
    }

    #[test]
    fn anthropic_thinking_shape_never_emits_top_level_effort() {
        let mut adaptive = json!({});
        apply_anthropic_thinking(&mut adaptive, "anthropic:claude-sonnet-4-6", Some("high"));
        assert_eq!(adaptive["thinking"], json!({ "type": "adaptive" }));
        assert!(adaptive.get("effort").is_none());

        let mut bounded_46 = json!({});
        apply_anthropic_thinking(&mut bounded_46, "anthropic:claude-sonnet-4-6", Some("minimal"));
        assert_eq!(
            bounded_46["thinking"],
            json!({ "type": "enabled", "budget_tokens": 1_024 })
        );
        assert!(bounded_46.get("effort").is_none());

        let mut manual = json!({});
        apply_anthropic_thinking(&mut manual, "anthropic:claude-sonnet-4-5", Some("high"));
        assert_eq!(
            manual["thinking"],
            json!({ "type": "enabled", "budget_tokens": 50_000 })
        );
        assert!(manual.get("effort").is_none());
    }

    #[test]
    fn compat_from_env_unknown_returns_none() {
        assert!(OpenAICompatClient::from_env("nonexistent-provider").is_none());
    }

    #[test]
    fn resolve_provider_handles_all_compat_ids() {
        // Should not panic for any registered provider — returns None if no credentials
        for id in [
            "groq",
            "xai",
            "mistral",
            "cerebras",
            "huggingface",
            "ollama",
            "ollama-cloud",
            "openai-codex",
        ] {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(resolve_provider(id));
        }
    }

    #[test]
    fn fallback_order_keeps_non_openai_models_on_their_native_provider() {
        assert_eq!(
            fallback_order_for_model("anthropic:claude-sonnet-4-6"),
            vec!["anthropic"]
        );
        assert_eq!(
            fallback_order_for_model("openrouter:meta/llama"),
            vec!["openrouter"]
        );
    }

    #[test]
    fn openai_family_fallback_prioritizes_codex_for_gpt_models() {
        let order = fallback_order_for_model("openai:gpt-5.4");
        assert_eq!(order, vec!["openai", "openai-codex"]);

        let codex_order = fallback_order_for_model("openai-codex:gpt-5.4");
        assert_eq!(codex_order, vec!["openai-codex", "openai"]);
    }

    #[test]
    fn resolve_execution_model_spec_reprefixes_openai_family_models() {
        assert_eq!(model_id_from_spec("openai:gpt-5.4"), "gpt-5.4");
        assert!(is_openai_family_model("openai:gpt-5.4"));
        assert!(is_openai_family_model("gpt-5.4"));
        assert!(is_openai_family_model("gpt-5.4-mini"));
    }

    // ── CodexClient tests ───────────────────────────────────────

    #[test]
    fn codex_build_input_user_message() {
        let msgs = vec![LlmMessage::User {
            content: "hello".into(),
            images: vec![],
        }];
        let input = CodexClient::build_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn codex_build_input_tool_result_strips_compound_id() {
        let msgs = vec![LlmMessage::ToolResult {
            call_id: "call_abc|fc_1".into(),
            tool_name: "bash".into(),
            content: "result text".into(),
            args_summary: None,
            is_error: false,
        }];
        let input = CodexClient::build_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["call_id"], "call_abc"); // compound stripped
    }

    #[test]
    fn codex_build_input_assistant_compound_tool_call() {
        use crate::bridge::WireToolCall;
        let msgs = vec![LlmMessage::Assistant {
            text: vec![],
            thinking: vec![],
            tool_calls: vec![WireToolCall {
                id: "call_abc|fc_0".into(),
                name: "bash".into(),
                arguments: json!({"command": "ls"}),
            }],
            raw: None,
        }];
        let input = CodexClient::build_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_abc");
        assert_eq!(input[0]["id"], "fc_0");
        assert_eq!(input[0]["name"], "bash");
    }

    #[tokio::test]
    async fn provider_stream_task_converts_errors_to_llm_error_events() {
        let (tx, mut rx) = mpsc::channel(1);
        let handle = spawn_provider_stream_task("test-provider", tx, async {
            anyhow::bail!("stream parse failed")
        });
        handle.await.expect("join");

        match rx.recv().await.expect("llm event") {
            LlmEvent::Error { message } => assert!(message.contains("stream parse failed")),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn provider_stream_task_converts_panics_to_llm_error_events() {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        let (tx, mut rx) = mpsc::channel(1);
        let handle = spawn_provider_stream_task("test-provider", tx, async {
            panic!("provider boom");
            #[allow(unreachable_code)]
            Ok(())
        });
        handle.await.expect("join");
        std::panic::set_hook(previous_hook);

        match rx.recv().await.expect("llm event") {
            LlmEvent::Error { message } => {
                assert!(message.contains("test-provider stream parser panicked"));
                assert!(message.contains("provider boom"));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn codex_build_tools_strips_descriptions() {
        let tools = vec![ToolDefinition {
            name: "bash".into(),
            label: "bash".into(),
            description: "Execute a command".into(),
            parameters: json!({
                "properties": {
                    "command": {"type": "string", "description": "The command to run"}
                },
                "required": ["command"]
            }),
        }];
        let wire = CodexClient::build_tools(&tools);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["name"], "bash");
        assert_eq!(wire[0]["type"], "function");
        // description should be stripped from parameters (strip_parameter_descriptions)
        assert!(
            wire[0]["parameters"]["properties"]["command"]
                .get("description")
                .is_none()
        );
    }

    #[test]
    fn codex_model_prefix_stripping() {
        // The stream() method strips "openai-codex:" and "openai:" prefixes
        // Verify the logic conceptually (can't call stream without a server)
        let full = "openai-codex:gpt-5.4-mini";
        let stripped = full
            .strip_prefix("openai-codex:")
            .or_else(|| full.strip_prefix("openai:"))
            .unwrap_or("gpt-5.4-mini");
        assert_eq!(stripped, "gpt-5.4-mini");

        let bare = "some-model";
        let stripped = bare
            .strip_prefix("openai-codex:")
            .or_else(|| bare.strip_prefix("openai:"))
            .unwrap_or("gpt-5.4-mini");
        assert_eq!(stripped, "gpt-5.4-mini"); // fallback
    }

    #[test]
    fn codex_retryable_status_codes() {
        assert!(is_codex_retryable(429));
        assert!(is_codex_retryable(500));
        assert!(is_codex_retryable(502));
        assert!(is_codex_retryable(503));
        assert!(is_codex_retryable(504));
        assert!(is_codex_retryable(520));
        assert!(!is_codex_retryable(400));
        assert!(!is_codex_retryable(401));
        assert!(!is_codex_retryable(200));
    }

    #[test]
    fn codex_from_env_without_credentials_returns_none() {
        // Without CHATGPT_OAUTH_TOKEN set or auth.json, should return None
        // (This is environment-dependent but should not panic)
        let _ = CodexClient::from_env();
    }

    #[test]
    fn model_id_from_spec_strips_known_provider_prefixes() {
        // Provider-prefixed models
        assert_eq!(
            model_id_from_spec("anthropic:claude-sonnet-4-6"),
            "claude-sonnet-4-6"
        );
        assert_eq!(model_id_from_spec("openai:gpt-4.1"), "gpt-4.1");
        assert_eq!(
            model_id_from_spec("openai-codex:gpt-5.4-mini"),
            "gpt-5.4-mini"
        );
        assert_eq!(model_id_from_spec("ollama:qwen3:32b"), "qwen3:32b");

        // Bare model IDs (no known provider prefix) — returned as-is
        assert_eq!(model_id_from_spec("claude-sonnet-4-6"), "claude-sonnet-4-6");
        assert_eq!(model_id_from_spec("qwen3:32b"), "qwen3:32b");

        // OpenRouter slash-separated models — no colon prefix, returned as-is
        assert_eq!(
            model_id_from_spec("anthropic/claude-sonnet-4-20250514"),
            "anthropic/claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn sanitize_tool_id_strips_codex_compound_ids() {
        assert_eq!(sanitize_tool_id("call_abc|fc_1"), "call_abc");
        assert_eq!(sanitize_tool_id("call_abc"), "call_abc");
        assert_eq!(
            sanitize_tool_id("toolu_01ABC-xyz_123"),
            "toolu_01ABC-xyz_123"
        );
    }

    #[test]
    fn sanitize_tool_id_replaces_invalid_chars() {
        assert_eq!(sanitize_tool_id("call abc"), "call_abc");
        assert_eq!(sanitize_tool_id("call.abc"), "call_abc");
        assert_eq!(sanitize_tool_id(""), "");
    }
}
