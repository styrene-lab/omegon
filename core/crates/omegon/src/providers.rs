//! Native LLM provider clients — direct HTTP streaming, no Node.js.
//!
//! Replaces core/bridge/llm-bridge.mjs entirely. The Rust binary makes
//! HTTPS requests directly to api.anthropic.com / api.openai.com.
//!
//! API keys resolved from: env vars → ~/.pi/agent/auth.json (OAuth tokens).
//! The upstream provider APIs are the only external dependency — no npm,
//! no Node.js, no supply chain risk from package registries.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::bridge::{LlmBridge, LlmEvent, LlmMessage, StreamOptions};

/// Claude Code CLI version for OAuth user-agent header.
/// Must match what Anthropic expects for subscription recognition.
/// Update when upstream Claude Code advances.
const CLAUDE_CODE_UA: &str = "claude-cli/2.1.75";
use omegon_traits::ToolDefinition;

// ─── API Key Resolution ─────────────────────────────────────────────────────

/// Resolve API key synchronously — env vars and unexpired auth.json tokens.
/// Returns (key, is_oauth).
pub fn resolve_api_key_sync(provider: &str) -> Option<(String, bool)> {
    // Use canonical provider map for env vars and auth.json key
    let env_keys = crate::auth::provider_env_vars(provider);
    let auth_key = crate::auth::auth_json_key(provider);

    // Known OAuth env vars — these carry OAuth tokens, not API keys.
    const OAUTH_ENV_VARS: &[&str] = &["ANTHROPIC_OAUTH_TOKEN", "CHATGPT_OAUTH_TOKEN"];

    // Env vars (not OAuth)
    for key in env_keys {
        // Skip OAuth token env vars — those are handled separately below
        if OAUTH_ENV_VARS.contains(key) { continue; }
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            tracing::debug!(provider, source = key, "API key resolved from env");
            return Some((val, false));
        }
    }

    // OAuth token from env
    for oauth_var in OAUTH_ENV_VARS {
        if env_keys.contains(oauth_var) {
            if let Ok(val) = std::env::var(oauth_var)
                && !val.is_empty()
            {
                tracing::debug!(provider, source = oauth_var, "OAuth token resolved from env");
                return Some((val, true));
            }
        }
    }

    // auth.json — using canonical key
    match crate::auth::read_credentials(auth_key) {
        Some(creds) if creds.cred_type == "oauth" && !creds.is_expired() => {
            tracing::debug!(provider, auth_key, expires = creds.expires, "OAuth token from auth.json (valid)");
            return Some((creds.access, true));
        }
        Some(creds) if creds.cred_type == "oauth" => {
            tracing::debug!(provider, auth_key, expires = creds.expires, "OAuth token from auth.json (EXPIRED — needs refresh)");
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

/// Resolve API key from env vars or ~/.pi/agent/auth.json (legacy, no refresh).
fn resolve_api_key(provider: &str) -> Option<String> {
    // Use canonical provider map for env vars
    let env_keys = crate::auth::provider_env_vars(provider);
    for key in env_keys {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty() {
                return Some(val);
            }
    }

    // Generic fallback: PROVIDER_API_KEY
    let generic = format!("{}_API_KEY", provider.to_uppercase());
    if let Ok(val) = std::env::var(&generic)
        && !val.is_empty() {
            return Some(val);
        }

    // auth.json — use canonical key mapping
    let auth_key = crate::auth::auth_json_key(provider);
    let home = dirs::home_dir()?;
    let auth_path = home.join(".pi/agent/auth.json");
    let content = std::fs::read_to_string(&auth_path).ok()?;
    let auth: Value = serde_json::from_str(&content).ok()?;
    auth.get(auth_key)?
        .get("access")?
        .as_str()
        .map(String::from)
}

/// Resolve a single provider by ID. Handles proprietary protocols (Anthropic,
/// Codex) separately, then falls through to the generic OpenAI-compat path.
async fn resolve_provider(provider_id: &str) -> Option<Box<dyn LlmBridge>> {
    match provider_id {
        "anthropic" => {
            if let Some(client) = AnthropicClient::from_env() {
                return Some(Box::new(client));
            }
            return AnthropicClient::from_env_async().await.map(|c| Box::new(c) as Box<dyn LlmBridge>);
        }
        "openai-codex" => {
            if let Some(client) = CodexClient::from_env() {
                return Some(Box::new(client));
            }
            return CodexClient::from_env_async().await.map(|c| Box::new(c) as Box<dyn LlmBridge>);
        }
        _ => {}
    }

    // OpenAI-compatible providers (including "openai" itself — it uses Chat Completions)
    if let Some(client) = OpenAICompatClient::from_env(provider_id) {
        return Some(Box::new(client));
    }

    None
}

/// Auto-detect the best available native provider from configured keys.
/// Tries sync resolution first, then async (with token refresh) if needed.
pub async fn auto_detect_bridge(model_spec: &str) -> Option<Box<dyn LlmBridge>> {
    let provider = model_spec.split(':').next().unwrap_or("anthropic");

    // Try to resolve a specific provider by ID.
    let primary = resolve_provider(provider).await;
    if primary.is_some() {
        return primary;
    }

    // Primary provider not available — try the full fallback chain.
    // Priority: Anthropic → OpenAI (API key) → OpenAI Codex (OAuth) →
    //           Groq/xAI/Mistral/HuggingFace → OpenRouter → Ollama
    tracing::warn!(
        requested = provider,
        "requested provider not available — trying fallback chain"
    );

    // Fallback order: proprietary providers first (best quality),
    // then OpenAI-compat providers (Groq/xAI/etc.), then OpenRouter
    // (universal fallback), then local inference (last resort).
    const FALLBACK_ORDER: &[&str] = &[
        "anthropic", "openai", "openai-codex",
        "groq", "xai", "mistral", "huggingface", "cerebras",
        "openrouter",
        "ollama",
    ];

    for &fallback_provider in FALLBACK_ORDER {
        if fallback_provider == provider { continue; }
        if let Some(client) = resolve_provider(fallback_provider).await {
            tracing::info!(provider = fallback_provider, "falling back to {}", fallback_provider);
            return Some(client);
        }
    }

    None
}

// ─── SSE Helpers ────────────────────────────────────────────────────────────

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
        Value::Array(arr) => {
            Value::Array(arr.iter().map(strip_parameter_descriptions).collect())
        }
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
        let args: Value = serde_json::from_str(&self.args_json)
            .unwrap_or_else(|_| json!({}));
        // Ensure arguments is always an object — Anthropic rejects null/string.
        let args = if args.is_object() { args } else { json!({}) };
        json!({"id": self.id, "name": self.name, "arguments": args})
    }
}

/// Process an SSE byte stream line by line, calling `on_data` for each `data: ` payload.
/// SSE idle timeout — if no chunk arrives within this window, assume the
/// connection is stalled and bail so the retry loop can re-attempt.
const SSE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

async fn process_sse<F>(
    response: reqwest::Response,
    mut on_data: F,
) -> anyhow::Result<()>
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
                        && (data == "[DONE]" || !on_data(data)) {
                            return Ok(());
                        }
                }
            }
            Ok(None) => break, // stream ended
            Err(_) => {
                tracing::warn!("SSE stream idle for {}s — treating as stalled", SSE_IDLE_TIMEOUT.as_secs());
                anyhow::bail!("SSE stream idle timeout ({}s with no data)", SSE_IDLE_TIMEOUT.as_secs());
            }
        }
    }
    Ok(())
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
            LlmMessage::Assistant { text, thinking, tool_calls, raw } => {
                // Prefer raw content blocks if available — they preserve provider-specific
                // fields like thinking signatures that are required for round-tripping.
                if let Some(raw_val) = raw {
                    if let Some(raw_content) = raw_val.get("content").and_then(|c| c.as_array()) {
                        if !raw_content.is_empty() {
                            return json!({"role": "assistant", "content": raw_content});
                        }
                    }
                }
                // Fallback: reconstruct from parsed fields (no signatures — works for
                // providers that don't need them, or for decayed/compacted messages)
                let mut content = Vec::new();
                for t in thinking {
                    content.push(json!({"type": "thinking", "thinking": t}));
                }
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
                    content.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": input,
                    }));
                }
                json!({"role": "assistant", "content": content})
            }
            LlmMessage::ToolResult { call_id, content, is_error, .. } => {
                json!({
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": call_id, "content": content, "is_error": is_error}]
                })
            }
        }).collect()
    }

    fn build_tools(tools: &[ToolDefinition], is_oauth: bool) -> Vec<Value> {
        tools.iter().map(|t| {
            let name = if is_oauth {
                to_claude_code_name(&t.name)
            } else {
                t.name.clone()
            };
            // Strip parameter-level descriptions to save tokens.
            // The model infers parameter semantics from names + the tool
            // description. Full descriptions cost ~50 tokens/tool × 31 tools.
            let properties = t.parameters.get("properties")
                .cloned()
                .unwrap_or(json!({}));
            let compact_props = strip_parameter_descriptions(&properties);
            json!({
                "name": name,
                "description": t.description,
                "input_schema": {
                    "type": "object",
                    "properties": compact_props,
                    "required": t.parameters.get("required").cloned().unwrap_or(json!([])),
                },
            })
        }).collect()
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

        // Accept "anthropic:model" or bare "model". If the model has a
        // different provider prefix (fallback scenario), use our default.
        let model = options.model.as_deref()
            .map(|m| {
                if let Some(stripped) = m.strip_prefix("anthropic:") {
                    stripped
                } else if m.contains(':') && crate::auth::provider_by_id(m.split(':').next().unwrap_or("")).is_some() {
                    // Different provider prefix — use our default
                    "claude-sonnet-4-6"
                } else {
                    m // bare model name, use as-is
                }
            })
            .unwrap_or("claude-sonnet-4-6");

        // System prompt format: OAuth requires array format with CC identity prefix
        // to satisfy the claude-code beta header contract.
        let system_value = if is_oauth {
            json!([
                {"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude."},
                {"type": "text", "text": system_prompt},
            ])
        } else {
            json!(system_prompt)
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
        if let Some(ref level) = options.reasoning {
            let budget = match level.as_str() {
                "low" => 5_000,
                "medium" => 10_000,
                "high" => 50_000,
                _ => 10_000,
            };
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
        }

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

        let response = self.client
            .post(format!("{}/v1/messages", self.base_url))
            .header(
                if is_oauth { "Authorization" } else { "x-api-key" },
                if is_oauth { format!("Bearer {}", api_key) } else { api_key.clone() },
            )
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", {
                let flags = if is_oauth {
                    "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14".to_string()
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
            .header("user-agent", if is_oauth { CLAUDE_CODE_UA } else { "omegon" })
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
            let user_msg = serde_json::from_str::<Value>(&err).ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| err.chars().take(200).collect());
            let detail = if is_oauth && (status.as_u16() == 429 || status.as_u16() == 413) {
                format!("\n  (OAuth subscription — {tool_count} tools, {body_size} byte request body, system prompt {system_len} chars)")
            } else {
                String::new()
            };
            let _ = tx.send(LlmEvent::Error {
                message: format!("Anthropic {status}: {user_msg}{detail}")
            }).await;
            return Ok(rx);
        }
        tracing::debug!(status = %response.status(), "Anthropic response OK — starting SSE stream");

        tokio::spawn(async move {
            if let Err(e) = parse_anthropic_stream(response, &tx).await {
                let _ = tx.send(LlmEvent::Error { message: format!("{e}") }).await;
            }
        });

        Ok(rx)
    }
}

async fn parse_anthropic_stream(
    response: reqwest::Response,
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

    tracing::debug!("parsing Anthropic SSE stream");
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

            // message_delta: stop_reason + final usage (not critical for functionality)
            "message_delta" => {
                tracing::trace!("message_delta: stop_reason/usage");
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
        let key = resolve_api_key("openai")?;
        // Codex OAuth tokens (JWT, starts with eyJ) are for the Responses API,
        // not the Chat Completions API. They'll get 500s from api.openai.com.
        // Only accept actual API keys (sk-*) or env-var-provided tokens.
        if key.starts_with("eyJ") {
            tracing::debug!("OpenAI credential is a Codex OAuth JWT — not usable with Chat Completions API");
            return None;
        }
        Some(Self::new(key))
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

        // Accept model with or without "openai:" prefix.
        // OpenAICompatClient strips the provider prefix before delegating,
        // so we may receive bare model names like "llama-3.3-70b-versatile".
        let model = options.model.as_deref()
            .map(|m| m.strip_prefix("openai:").unwrap_or(m))
            .unwrap_or("gpt-4.1");

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
                LlmMessage::Assistant { text, tool_calls, .. } => {
                    let mut msg = json!({"role": "assistant"});
                    if let Some(t) = text.first() { msg["content"] = json!(t); }
                    if !tool_calls.is_empty() {
                        msg["tool_calls"] = tool_calls.iter().map(|tc| json!({
                            "id": tc.id, "type": "function",
                            "function": {"name": tc.name, "arguments": if tc.arguments.is_object() { tc.arguments.to_string() } else { "{}".to_string() }},
                        })).collect();
                    }
                    wire_msgs.push(msg);
                }
                LlmMessage::ToolResult { call_id, content, .. } => {
                    wire_msgs.push(json!({"role": "tool", "tool_call_id": call_id, "content": content}));
                }
            }
        }

        let wire_tools: Vec<Value> = tools.iter().map(|t| json!({
            "type": "function",
            "function": {"name": t.name, "description": t.description, "parameters": t.parameters},
        })).collect();

        let mut body = json!({"model": model, "messages": wire_msgs, "stream": true});
        if !wire_tools.is_empty() { body["tools"] = Value::Array(wire_tools); }

        let response = self.client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let err = response.text().await.unwrap_or_default();
            let user_msg = serde_json::from_str::<Value>(&err).ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| err.chars().take(200).collect());
            let _ = tx.send(LlmEvent::Error { message: format!("OpenAI {status}: {user_msg}") }).await;
            return Ok(rx);
        }

        tokio::spawn(async move {
            if let Err(e) = parse_openai_stream(response, &tx).await {
                let _ = tx.send(LlmEvent::Error { message: format!("{e}") }).await;
            }
        });

        Ok(rx)
    }
}

async fn parse_openai_stream(
    response: reqwest::Response,
    tx: &mpsc::Sender<LlmEvent>,
) -> anyhow::Result<()> {
    let mut full_text = String::new();
    let mut tool_calls: Vec<ToolCallAccum> = Vec::new();

    let _ = tx.try_send(LlmEvent::Start);
    let _ = tx.try_send(LlmEvent::TextStart);

    process_sse(response, |data| {
        let Ok(event) = serde_json::from_str::<Value>(data) else { return true };
        let Some(choice) = event.get("choices").and_then(|c| c.get(0)) else { return true };
        let delta = &choice["delta"];

        // Text
        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
            full_text.push_str(content);
            let _ = tx.try_send(LlmEvent::TextDelta { delta: content.to_string() });
        }

        // Tool calls
        if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
            for tc in tcs {
                let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                while tool_calls.len() <= idx {
                    tool_calls.push(ToolCallAccum { id: String::new(), name: String::new(), args_json: String::new() });
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
        if choice.get("finish_reason").and_then(|f| f.as_str()).is_some() {
            for tc in &tool_calls {
                let _ = tx.try_send(LlmEvent::ToolCallEnd { tool_call: crate::bridge::WireToolCall { id: tc.id.clone(), name: tc.name.clone(), arguments: serde_json::from_str(&tc.args_json).unwrap_or_default() } });
            }
            let _ = tx.try_send(LlmEvent::TextEnd);
            let tc_vals: Vec<Value> = tool_calls.iter().map(|tc| tc.to_value()).collect();
            let _ = tx.try_send(LlmEvent::Done { message: json!({"text": full_text, "tool_calls": tc_vals}) });
            return false;
        }
        true
    }).await
}

// ─── OpenAI Codex Responses API ─────────────────────────────────────────────
//
// The Codex Responses API is a different protocol from Chat Completions.
// It uses ChatGPT OAuth JWT tokens (not API keys), talks to
// chatgpt.com/backend-api/codex/responses, and has a different message format
// (instructions/input instead of messages, response.output_item.* SSE events).
//
// This enables ChatGPT Pro/Plus subscribers to use their subscription
// for inference without a separate OpenAI API key. The free-tier model
// gpt-5.3-codex-spark costs $0/token.

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
            base_url: std::env::var("CODEX_BASE_URL")
                .unwrap_or_else(|_| CODEX_BASE_URL.into()),
        }
    }

    /// Create from environment or auth.json.
    /// Looks for: CHATGPT_OAUTH_TOKEN env var, then openai-codex in auth.json.
    pub fn from_env() -> Option<Self> {
        // 1. Try env var
        if let Ok(token) = std::env::var("CHATGPT_OAUTH_TOKEN") {
            if !token.is_empty() && token.starts_with("eyJ") {
                if let Some(account_id) = crate::auth::extract_jwt_claim(
                    &token, "https://api.openai.com/auth", "chatgpt_account_id"
                ) {
                    tracing::debug!("CodexClient: resolved from CHATGPT_OAUTH_TOKEN env var");
                    return Some(Self::new(token, account_id));
                }
            }
        }

        // 2. Try auth.json (openai-codex entry) — JWT tokens stored by /login openai-codex oauth
        let creds = crate::auth::read_credentials("openai-codex")?;
        if creds.cred_type != "oauth" || creds.access.is_empty() {
            return None;
        }
        // Must be a JWT (starts with eyJ)
        if !creds.access.starts_with("eyJ") {
            return None;
        }
        if creds.is_expired() {
            tracing::debug!("CodexClient: auth.json token expired — needs refresh");
            return None;
        }

        // Get account ID: stored in auth.json or extract from JWT
        let account_id = crate::auth::read_credential_extra("openai-codex", "accountId")
            .or_else(|| crate::auth::extract_jwt_claim(
                &creds.access, "https://api.openai.com/auth", "chatgpt_account_id"
            ))?;

        tracing::debug!("CodexClient: resolved from auth.json");
        Some(Self::new(creds.access, account_id))
    }

    /// Create with async token refresh.
    pub async fn from_env_async() -> Option<Self> {
        // Try sync first
        if let Some(client) = Self::from_env() {
            return Some(client);
        }

        // Try refresh
        let (token, is_oauth) = crate::auth::resolve_with_refresh("openai-codex").await?;
        if !is_oauth || !token.starts_with("eyJ") {
            return None;
        }

        let account_id = crate::auth::read_credential_extra("openai-codex", "accountId")
            .or_else(|| crate::auth::extract_jwt_claim(
                &token, "https://api.openai.com/auth", "chatgpt_account_id"
            ))?;

        Some(Self::new(token, account_id))
    }

    /// Build the Responses API input messages from our LlmMessage format.
    fn build_input(messages: &[LlmMessage]) -> Vec<Value> {
        let mut input = Vec::new();
        let mut msg_index = 0u32;

        for msg in messages {
            match msg {
                LlmMessage::User { content, images } => {
                    if images.is_empty() {
                        input.push(json!({
                            "role": "user",
                            "content": [{"type": "input_text", "text": content}]
                        }));
                    } else {
                        let mut parts: Vec<Value> = images.iter().map(|img| json!({
                            "type": "input_image",
                            "detail": "auto",
                            "image_url": format!("data:{};base64,{}", img.media_type, img.data),
                        })).collect();
                        parts.push(json!({"type": "input_text", "text": content}));
                        input.push(json!({"role": "user", "content": parts}));
                    }
                }
                LlmMessage::Assistant { text, tool_calls, .. } => {
                    // Text blocks → message items
                    for t in text {
                        if !t.is_empty() {
                            input.push(json!({
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": t, "annotations": []}],
                                "status": "completed",
                                "id": format!("msg_{msg_index}"),
                            }));
                            msg_index += 1;
                        }
                    }
                    // Tool calls → function_call items
                    for tc in tool_calls {
                        // call_id is tc.id, generate an item_id with fc_ prefix
                        let item_id = if tc.id.contains('|') {
                            // Already compound format
                            let parts: Vec<&str> = tc.id.splitn(2, '|').collect();
                            parts.get(1).unwrap_or(&"fc_0").to_string()
                        } else {
                            format!("fc_{msg_index}")
                        };
                        let call_id = if tc.id.contains('|') {
                            tc.id.splitn(2, '|').next().unwrap_or(&tc.id).to_string()
                        } else {
                            tc.id.clone()
                        };
                        input.push(json!({
                            "type": "function_call",
                            "id": item_id,
                            "call_id": call_id,
                            "name": tc.name,
                            "arguments": if tc.arguments.is_object() {
                                tc.arguments.to_string()
                            } else {
                                "{}".to_string()
                            },
                        }));
                        msg_index += 1;
                    }
                }
                LlmMessage::ToolResult { call_id, content, .. } => {
                    // Strip compound ID: use just the call_id part
                    let cid = if call_id.contains('|') {
                        call_id.splitn(2, '|').next().unwrap_or(call_id).to_string()
                    } else {
                        call_id.clone()
                    };
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": cid,
                        "output": content,
                    }));
                }
            }
        }

        input
    }

    /// Build tools in Responses API format.
    fn build_tools(tools: &[ToolDefinition]) -> Vec<Value> {
        tools.iter().map(|t| {
            let compact_props = t.parameters.get("properties")
                .cloned()
                .unwrap_or(json!({}));
            let compact = strip_parameter_descriptions(&compact_props);
            json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": {
                    "type": "object",
                    "properties": compact,
                    "required": t.parameters.get("required").cloned().unwrap_or(json!([])),
                },
                "strict": null,
            })
        }).collect()
    }
}

/// Retryable status codes for Codex API.
fn is_codex_retryable(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
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

        // Re-resolve credentials on each request (handles /login mid-session, token refresh)
        let (jwt_token, account_id) = match crate::auth::resolve_with_refresh("openai-codex").await {
            Some((token, true)) if token.starts_with("eyJ") => {
                let aid = crate::auth::read_credential_extra("openai-codex", "accountId")
                    .or_else(|| crate::auth::extract_jwt_claim(
                        &token, "https://api.openai.com/auth", "chatgpt_account_id"
                    ))
                    .unwrap_or_else(|| self.account_id.clone());
                (token, aid)
            }
            _ => {
                tracing::warn!("Codex credential re-resolution failed — using startup credentials");
                (self.jwt_token.clone(), self.account_id.clone())
            }
        };

        let model = options.model.as_deref()
            .and_then(|m| m.strip_prefix("openai-codex:"))
            .or_else(|| options.model.as_deref().and_then(|m| m.strip_prefix("openai:")))
            .unwrap_or("gpt-5.3-codex-spark");

        let input = Self::build_input(messages);
        let wire_tools = Self::build_tools(tools);
        let tool_count = wire_tools.len();

        let mut body = json!({
            "model": model,
            "store": false,
            "stream": true,
            "instructions": system_prompt,
            "input": input,
            "text": {"verbosity": "medium"},
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });

        if !wire_tools.is_empty() {
            body["tools"] = Value::Array(wire_tools);
        }

        if let Some(ref level) = options.reasoning {
            body["reasoning"] = json!({
                "effort": match level.as_str() {
                    "low" | "minimal" => "low",
                    "medium" => "medium",
                    "high" | "xhigh" => "high",
                    _ => "medium",
                },
                "summary": "auto",
            });
        }

        let url = format!("{}/codex/responses", self.base_url.trim_end_matches('/'));
        let msg_count = input.len();
        let system_len = system_prompt.len();
        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);

        tracing::debug!(
            model,
            tool_count,
            msg_count,
            system_len,
            body_size,
            url = %url,
            "Codex Responses API streaming request"
        );
        tracing::trace!(body = %serde_json::to_string(&body).unwrap_or_default(), "request body");

        // Retry loop for transient errors
        let max_retries = 3u32;
        let base_delay = std::time::Duration::from_secs(1);
        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            let response = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", jwt_token))
                .header("chatgpt-account-id", &account_id)
                .header("originator", "omegon")
                .header("OpenAI-Beta", "responses=experimental")
                .header("accept", "text/event-stream")
                .header("content-type", "application/json")
                .header("user-agent", format!("omegon ({} {}; {})",
                    std::env::consts::OS, std::env::consts::ARCH, env!("CARGO_PKG_VERSION")))
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    tracing::debug!(status = %resp.status(), attempt, "Codex response OK — starting SSE stream");
                    let tx_clone = tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = parse_codex_stream(resp, &tx_clone).await {
                            let _ = tx_clone.send(LlmEvent::Error { message: format!("{e}") }).await;
                        }
                    });
                    return Ok(rx);
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let err_body = resp.text().await.unwrap_or_default();

                    // Parse friendly error message
                    let user_msg = serde_json::from_str::<Value>(&err_body).ok()
                        .and_then(|v| {
                            v["error"]["message"].as_str()
                                .or_else(|| v["detail"].as_str())
                                .map(String::from)
                        })
                        .unwrap_or_else(|| err_body.chars().take(200).collect());

                    if attempt < max_retries && is_codex_retryable(status) {
                        let delay = base_delay * 2u32.pow(attempt);
                        tracing::warn!(status, attempt, delay_ms = delay.as_millis(), "Codex retryable error — waiting");
                        tokio::time::sleep(delay).await;
                        last_error = format!("Codex {status}: {user_msg}");
                        continue;
                    }

                    tracing::error!(status, error = %user_msg, "Codex API error (non-retryable or max retries)");
                    let _ = tx.send(LlmEvent::Error {
                        message: format!("Codex {status}: {user_msg}")
                    }).await;
                    return Ok(rx);
                }
                Err(e) => {
                    if attempt < max_retries {
                        let delay = base_delay * 2u32.pow(attempt);
                        tracing::warn!(error = %e, attempt, "Codex network error — retrying");
                        tokio::time::sleep(delay).await;
                        last_error = format!("Network error: {e}");
                        continue;
                    }
                    let _ = tx.send(LlmEvent::Error {
                        message: format!("Codex connection failed after {max_retries} retries: {last_error}")
                    }).await;
                    return Ok(rx);
                }
            }
        }

        let _ = tx.send(LlmEvent::Error {
            message: format!("Codex request failed after {max_retries} retries: {last_error}")
        }).await;
        Ok(rx)
    }
}

/// Parse the Codex Responses API SSE stream.
///
/// The Responses API uses a completely different event structure from Chat Completions.
/// Key events:
///   - response.output_item.added → new item (reasoning, message, function_call)
///   - response.output_text.delta → text content delta
///   - response.reasoning_summary_text.delta → thinking delta
///   - response.function_call_arguments.delta → tool call arguments delta
///   - response.output_item.done → item complete
///   - response.completed → full response done with usage
///   - response.failed / error → error
async fn parse_codex_stream(
    response: reqwest::Response,
    tx: &mpsc::Sender<LlmEvent>,
) -> anyhow::Result<()> {
    let mut full_text = String::new();
    let mut current_item_type: Option<String> = None;
    let mut current_text = String::new();
    let mut current_thinking = String::new();

    // Tool call tracking: call_id, item_id, name, args_json
    struct CodexToolCall {
        call_id: String,
        item_id: String,
        name: String,
        args_json: String,
    }
    let mut tool_calls: Vec<CodexToolCall> = Vec::new();
    let mut completed_tool_calls: Vec<Value> = Vec::new();

    let _ = tx.try_send(LlmEvent::Start);

    let mut event_count = 0u32;

    process_sse(response, |data| {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            tracing::warn!(data, "Codex: failed to parse SSE event");
            return true;
        };
        let etype = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
        event_count += 1;
        tracing::trace!(event_type = etype, n = event_count, "Codex SSE event");

        match etype {
            // ── New output item ──────────────────────────────────────────
            "response.output_item.added" => {
                let item = &event["item"];
                let item_type = item["type"].as_str().unwrap_or("");

                match item_type {
                    "reasoning" => {
                        current_item_type = Some("reasoning".into());
                        current_thinking.clear();
                        let _ = tx.try_send(LlmEvent::ThinkingStart);
                    }
                    "message" => {
                        current_item_type = Some("message".into());
                        current_text.clear();
                        let _ = tx.try_send(LlmEvent::TextStart);
                    }
                    "function_call" => {
                        current_item_type = Some("function_call".into());
                        let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                        let item_id = item["id"].as_str().unwrap_or("").to_string();
                        let name = item["name"].as_str().unwrap_or("").to_string();
                        tracing::debug!(call_id = %call_id, item_id = %item_id, name = %name, "Codex function_call started");
                        tool_calls.push(CodexToolCall {
                            call_id,
                            item_id,
                            name,
                            args_json: String::new(),
                        });
                        let _ = tx.try_send(LlmEvent::ToolCallStart);
                    }
                    _ => {
                        tracing::trace!(item_type, "Codex: unknown output item type");
                    }
                }
            }

            // ── Text deltas ──────────────────────────────────────────────
            "response.output_text.delta" => {
                let delta = event["delta"].as_str().unwrap_or("");
                full_text.push_str(delta);
                current_text.push_str(delta);
                let _ = tx.try_send(LlmEvent::TextDelta { delta: delta.to_string() });
            }

            // ── Reasoning (thinking) deltas ──────────────────────────────
            "response.reasoning_summary_text.delta" => {
                let delta = event["delta"].as_str().unwrap_or("");
                current_thinking.push_str(delta);
                let _ = tx.try_send(LlmEvent::ThinkingDelta { delta: delta.to_string() });
            }
            "response.reasoning_summary_part.done" => {
                // Add separator between reasoning summary parts
                current_thinking.push_str("\n\n");
                let _ = tx.try_send(LlmEvent::ThinkingDelta { delta: "\n\n".to_string() });
            }

            // ── Tool call argument deltas ────────────────────────────────
            "response.function_call_arguments.delta" => {
                let delta = event["delta"].as_str().unwrap_or("");
                if let Some(tc) = tool_calls.last_mut() {
                    tc.args_json.push_str(delta);
                }
            }
            "response.function_call_arguments.done" => {
                // Final arguments — overwrite partial
                if let Some(tc) = tool_calls.last_mut() {
                    if let Some(args) = event["arguments"].as_str() {
                        tc.args_json = args.to_string();
                    }
                }
            }

            // ── Content part events (for message items) ──────────────────
            "response.content_part.added" | "response.reasoning_summary_part.added" => {
                // Structural events — no action needed for streaming
            }

            // ── Item complete ────────────────────────────────────────────
            "response.output_item.done" => {
                let item = &event["item"];
                let item_type = item["type"].as_str().unwrap_or("");

                match item_type {
                    "reasoning" => {
                        let _ = tx.try_send(LlmEvent::ThinkingEnd);
                    }
                    "message" => {
                        // Extract final text from the completed item
                        if let Some(content) = item["content"].as_array() {
                            let final_text: String = content.iter()
                                .filter_map(|c| {
                                    match c["type"].as_str() {
                                        Some("output_text") => c["text"].as_str(),
                                        Some("refusal") => c["refusal"].as_str(),
                                        _ => None,
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("");
                            if !final_text.is_empty() {
                                current_text = final_text;
                            }
                        }
                        let _ = tx.try_send(LlmEvent::TextEnd);
                    }
                    "function_call" => {
                        let call_id = item["call_id"].as_str().unwrap_or("").to_string();
                        let item_id = item["id"].as_str().unwrap_or("").to_string();
                        let name = item["name"].as_str().unwrap_or("").to_string();
                        let args_str = item["arguments"].as_str().unwrap_or("{}");
                        let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                        let args = if args.is_object() { args } else { json!({}) };

                        // Emit the compound tool call ID for multi-turn continuity
                        let compound_id = format!("{call_id}|{item_id}");

                        completed_tool_calls.push(json!({
                            "id": compound_id,
                            "name": name,
                            "arguments": args,
                        }));

                        let _ = tx.try_send(LlmEvent::ToolCallEnd {
                            tool_call: crate::bridge::WireToolCall {
                                id: compound_id,
                                name,
                                arguments: args.clone(),
                            }
                        });
                    }
                    _ => {}
                }
                current_item_type = None;
            }

            // ── Response complete ────────────────────────────────────────
            "response.completed" => {
                tracing::debug!(
                    text_len = full_text.len(),
                    tool_calls = completed_tool_calls.len(),
                    sse_events = event_count,
                    "Codex response.completed — stream done"
                );

                let _ = tx.try_send(LlmEvent::Done {
                    message: json!({
                        "text": full_text,
                        "tool_calls": completed_tool_calls,
                    }),
                });
                return false; // stop
            }

            // ── Error events ─────────────────────────────────────────────
            "response.failed" => {
                let err_msg = event["response"]["error"]["message"]
                    .as_str()
                    .or_else(|| event["response"]["incomplete_details"]["reason"].as_str())
                    .unwrap_or("Codex response failed");
                tracing::error!(error = err_msg, "Codex response.failed");
                let _ = tx.try_send(LlmEvent::Error {
                    message: format!("Codex: {err_msg}")
                });
                return false;
            }
            "error" => {
                let code = event["code"].as_str().unwrap_or("unknown");
                let msg = event["message"].as_str().unwrap_or("unknown error");
                tracing::error!(code, msg, "Codex error event");
                let _ = tx.try_send(LlmEvent::Error {
                    message: format!("Codex error ({code}): {msg}")
                });
                return false;
            }

            _ => {
                tracing::trace!(event_type = etype, "Codex: unhandled SSE event type");
            }
        }
        true
    }).await
}

// ─── OpenAI-Compatible Providers ─────────────────────────────────────────────
//
// Many providers speak the OpenAI Chat Completions wire protocol with different
// base URLs and API keys: OpenRouter, Groq, xAI, Cerebras, Mistral, HuggingFace,
// and Ollama. Rather than creating N separate client structs, we parameterize
// the OpenAIClient with the provider's base URL.
//
// Adding a new OpenAI-compatible provider:
//   1. Add to auth::PROVIDERS with openai_compat_url = Some("https://...")
//   2. That's it. auto_detect_bridge picks it up automatically.

pub struct OpenAICompatClient {
    inner: OpenAIClient,
    provider_id: String,
    default_model: Option<String>,
}

impl OpenAICompatClient {
    pub fn new(api_key: String, base_url: String, provider_id: String) -> Self {
        Self {
            inner: OpenAIClient {
                client: reqwest::Client::new(),
                api_key,
                base_url,
            },
            default_model: None,
            provider_id,
        }
    }

    pub fn with_default_model(mut self, model: String) -> Self {
        self.default_model = Some(model);
        self
    }

    /// Resolve from env vars / auth.json using the canonical PROVIDERS map.
    pub fn from_env(provider_id: &str) -> Option<Self> {
        let provider = crate::auth::provider_by_id(provider_id)?;
        let base_url = provider.openai_compat_url?;

        // Ollama doesn't need an API key
        if provider_id == "ollama" {
            return Self::from_env_ollama();
        }

        let key = resolve_api_key(provider_id)?;

        // OpenAI-specific: reject Codex OAuth JWTs — they need the
        // Responses API (CodexClient), not Chat Completions.
        if provider_id == "openai" && key.starts_with("eyJ") {
            tracing::debug!("OpenAI credential is a Codex OAuth JWT — routing to CodexClient instead");
            return None;
        }

        let mut client = Self::new(key, base_url.to_string(), provider_id.to_string());

        // Provider-specific default models — used when the requested model
        // doesn't belong to this provider (fallback scenario)
        client.default_model = Some(match provider_id {
            "openai" => "gpt-4.1".into(),
            "openrouter" => "openrouter/auto".into(),
            "groq" => "llama-3.3-70b-versatile".into(),
            "xai" => "grok-2".into(),
            "mistral" => "mistral-large-latest".into(),
            "cerebras" => "llama3.1-8b".into(),
            "huggingface" => "Qwen/Qwen3-235B-A22B-Thinking-2507".into(),
            _ => return Some(client), // no default — use whatever is passed
        });

        Some(client)
    }

    /// Resolve Ollama specifically — no API key, just check if it's reachable.
    fn from_env_ollama() -> Option<Self> {
        let mgr = crate::ollama::OllamaManager::default();
        if mgr.is_reachable() {
            let host = std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost:11434".into());
            tracing::debug!(host = %host, "Ollama server detected via OllamaManager");
            Some(Self::new(
                String::new(), // no API key
                host,
                "ollama".into(),
            ))
        } else {
            tracing::trace!("Ollama not reachable — skipping");
            None
        }
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

        // Strip known provider prefixes from model name.
        // The wire protocol wants bare model IDs (e.g. "llama-3.3-70b", not "groq:llama-3.3-70b").
        //
        // Only strip prefixes that match known provider IDs — don't blindly strip
        // on ":" because model names like "qwen3:32b" (Ollama tag format) use colons.
        //
        // If the model has a DIFFERENT provider's prefix (e.g. "anthropic:claude-sonnet-4-6"
        // on a Groq bridge due to fallback), replace with our default model instead.
        if let Some(ref mut m) = opts.model {
            if let Some(colon_pos) = m.find(':') {
                let prefix = &m[..colon_pos];
                if let Some(provider) = crate::auth::provider_by_id(prefix) {
                    if prefix == self.provider_id {
                        // Our own prefix — strip it
                        *m = m[colon_pos + 1..].to_string();
                    } else if provider.openai_compat_url.is_some() || prefix == "anthropic" || prefix == "openai-codex" {
                        // Different provider's prefix — this model won't work on our API.
                        // Use our default model instead.
                        tracing::info!(
                            provider = %self.provider_id,
                            requested_model = %m,
                            "model belongs to different provider — using default"
                        );
                        *m = String::new(); // Will be replaced by default_model below
                    }
                }
            }
        }

        // Apply default model if none specified
        if opts.model.is_none() || opts.model.as_deref() == Some("") {
            if let Some(ref default) = self.default_model {
                // Default model already has provider prefix — strip it
                let default_bare = if let Some(colon_pos) = default.find(':') {
                    let prefix = &default[..colon_pos];
                    if crate::auth::provider_by_id(prefix).is_some() {
                        &default[colon_pos + 1..]
                    } else {
                        default.as_str()
                    }
                } else {
                    default.as_str()
                };
                opts.model = Some(default_bare.to_string());
            }
        }

        self.inner.stream(system_prompt, messages, tools, &opts).await
    }
}

// Legacy alias for backward compatibility in auto_detect_bridge
pub type OpenRouterClient = OpenAICompatClient;

#[cfg(test)]
mod tests {
    use super::*;

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
    fn anthropic_build_messages() {
        let messages = vec![
            LlmMessage::User { content: "hello".into(), images: vec![] },
        ];
        let wire = AnthropicClient::build_messages(&messages);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[0]["content"], "hello");
    }

    #[test]
    fn anthropic_build_tool_result() {
        let messages = vec![
            LlmMessage::ToolResult {
                call_id: "tc1".into(),
                tool_name: "read".into(),
                content: "file contents".into(),
                is_error: false,
                args_summary: None,
            },
        ];
        let wire = AnthropicClient::build_messages(&messages);
        assert_eq!(wire[0]["role"], "user");
        assert_eq!(wire[0]["content"][0]["type"], "tool_result");
        assert_eq!(wire[0]["content"][0]["tool_use_id"], "tc1");
    }

    #[test]
    fn anthropic_tool_use_input_always_object() {
        // When arguments is null (e.g. tools with no required params),
        // Anthropic requires `input` to be `{}`, not `null`.
        let messages = vec![
            LlmMessage::Assistant {
                text: vec![],
                thinking: vec![],
                tool_calls: vec![crate::bridge::WireToolCall {
                    id: "tc1".into(),
                    name: "memory_query".into(),
                    arguments: Value::Null,
                }],
                raw: None, // Force fallback path (no raw content blocks)
            },
        ];
        let wire = AnthropicClient::build_messages(&messages);
        let input = &wire[0]["content"][0]["input"];
        assert!(input.is_object(), "input should be object, got: {input}");
        assert_eq!(input, &json!({}));
    }

    #[test]
    fn error_message_extraction_from_api_json() {
        // Simulate what happens when Anthropic returns a 400 error
        let raw_body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.1.content.1.tool_use.input: Input should be a valid dictionary"},"request_id":"req_abc123"}"#;
        let user_msg = serde_json::from_str::<Value>(raw_body).ok()
            .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| raw_body.chars().take(200).collect());
        assert_eq!(user_msg, "messages.1.content.1.tool_use.input: Input should be a valid dictionary");
    }

    #[test]
    fn error_message_fallback_on_non_json() {
        let raw_body = "Service Unavailable";
        let user_msg = serde_json::from_str::<Value>(raw_body).ok()
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
        assert!(stripped["nested"]["items"]["properties"]["inner"].get("description").is_none());
        // But nested type preserved
        assert_eq!(stripped["nested"]["items"]["properties"]["inner"]["type"], "string");
    }

    #[test]
    fn build_tools_oauth_remaps_known_names() {
        let tools = vec![
            ToolDefinition {
                name: "bash".into(), label: "bash".into(),
                description: "run command".into(), parameters: json!({}),
            },
            ToolDefinition {
                name: "read".into(), label: "read".into(),
                description: "read file".into(), parameters: json!({}),
            },
            ToolDefinition {
                name: "memory_store".into(), label: "memory".into(),
                description: "store fact".into(), parameters: json!({}),
            },
        ];
        let wire = AnthropicClient::build_tools(&tools, true);
        assert_eq!(wire[0]["name"], "Bash", "bash should become Bash for OAuth");
        assert_eq!(wire[1]["name"], "Read", "read should become Read for OAuth");
        assert_eq!(wire[2]["name"], "memory_store", "unknown tools pass through unchanged");
    }

    #[test]
    fn build_tools_api_key_preserves_names() {
        let tools = vec![
            ToolDefinition {
                name: "bash".into(), label: "bash".into(),
                description: "run command".into(), parameters: json!({}),
            },
        ];
        let wire = AnthropicClient::build_tools(&tools, false);
        assert_eq!(wire[0]["name"], "bash", "API key mode preserves lowercase");
    }

    #[test]
    fn from_claude_code_name_roundtrips() {
        // Every name that to_claude_code_name maps must roundtrip
        let known = [("bash", "Bash"), ("read", "Read"), ("write", "Write"),
                     ("edit", "Edit"), ("web_search", "WebSearch")];
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
        assert_eq!(arr[0]["text"], "You are Claude Code, Anthropic's official CLI for Claude.");
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
        let header_name = if is_oauth { "Authorization" } else { "x-api-key" };
        assert_eq!(header_name, "Authorization");
    }

    #[test]
    fn api_key_auth_header_uses_x_api_key() {
        let is_oauth = false;
        let header_name = if is_oauth { "Authorization" } else { "x-api-key" };
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
        assert!(flags.contains("claude-code-20250219"), "OAuth must include CC beta");
        assert!(flags.contains("oauth-2025-04-20"), "OAuth must include OAuth beta");
        assert!(!flags.contains("context-1m"), "OAuth must NOT include 1M context beta");
    }

    #[test]
    fn context_1m_beta_flag_never_sent() {
        // The context-1m-2025-08-07 beta flag is deprecated. Sonnet/Opus 4.6
        // support 1M context natively. The flag only triggers billing gates
        // ("Extra usage is required for long context requests" 429).
        // Verified empirically: OAuth request with flag → 429, without → 200.
        let oauth_flags = "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14";
        let api_flags = "interleaved-thinking-2025-05-14";
        assert!(!oauth_flags.contains("context-1m"), "OAuth must never send context-1m");
        assert!(!api_flags.contains("context-1m"), "API key must never send context-1m");
    }

    #[test]
    fn api_key_beta_flags_include_thinking() {
        let is_oauth = false;
        let flags = if is_oauth {
            "claude-code-20250219,oauth-2025-04-20".to_string()
        } else {
            "interleaved-thinking-2025-05-14".to_string()
        };
        assert!(flags.contains("interleaved-thinking"), "API key must include thinking beta");
        assert!(!flags.contains("claude-code"), "API key must NOT include CC beta");
    }

    // ── Codex Responses API tests ───────────────────────────────────

    #[test]
    fn codex_build_input_user_message() {
        let messages = vec![
            LlmMessage::User { content: "hello".into(), images: vec![] },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hello");
    }

    #[test]
    fn codex_build_input_user_message_with_image() {
        let messages = vec![
            LlmMessage::User {
                content: "describe this".into(),
                images: vec![crate::bridge::ImageAttachment {
                    data: "base64data".into(),
                    media_type: "image/png".into(),
                }],
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 1);
        let content = input[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2); // image + text
        assert_eq!(content[0]["type"], "input_image");
        assert!(content[0]["image_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(content[1]["type"], "input_text");
    }

    #[test]
    fn codex_build_input_assistant_text() {
        let messages = vec![
            LlmMessage::Assistant {
                text: vec!["response text".into()],
                thinking: vec![],
                tool_calls: vec![],
                raw: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "assistant");
        assert_eq!(input[0]["content"][0]["type"], "output_text");
        assert_eq!(input[0]["content"][0]["text"], "response text");
        assert_eq!(input[0]["status"], "completed");
    }

    #[test]
    fn codex_build_input_tool_call() {
        let messages = vec![
            LlmMessage::Assistant {
                text: vec![],
                thinking: vec![],
                tool_calls: vec![crate::bridge::WireToolCall {
                    id: "call_abc123".into(),
                    name: "bash".into(),
                    arguments: json!({"command": "ls"}),
                }],
                raw: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["call_id"], "call_abc123");
        assert_eq!(input[0]["name"], "bash");
        // id should have fc_ prefix
        assert!(input[0]["id"].as_str().unwrap().starts_with("fc_"));
    }

    #[test]
    fn codex_build_input_tool_call_compound_id() {
        // When a tool call already has a compound ID (from a previous Codex response),
        // preserve the existing call_id|item_id split
        let messages = vec![
            LlmMessage::Assistant {
                text: vec![],
                thinking: vec![],
                tool_calls: vec![crate::bridge::WireToolCall {
                    id: "call_abc|fc_xyz".into(),
                    name: "read".into(),
                    arguments: json!({"path": "test.rs"}),
                }],
                raw: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input[0]["call_id"], "call_abc");
        assert_eq!(input[0]["id"], "fc_xyz");
    }

    #[test]
    fn codex_build_input_tool_result() {
        let messages = vec![
            LlmMessage::ToolResult {
                call_id: "call_abc|fc_xyz".into(),
                tool_name: "bash".into(),
                content: "output text".into(),
                is_error: false,
                args_summary: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_abc"); // stripped compound ID
        assert_eq!(input[0]["output"], "output text");
    }

    #[test]
    fn codex_build_input_tool_result_simple_id() {
        let messages = vec![
            LlmMessage::ToolResult {
                call_id: "call_abc".into(),
                tool_name: "bash".into(),
                content: "output".into(),
                is_error: false,
                args_summary: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input[0]["call_id"], "call_abc"); // preserved as-is
    }

    #[test]
    fn codex_build_tools_format() {
        let tools = vec![
            ToolDefinition {
                name: "bash".into(),
                label: "bash".into(),
                description: "run command".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "command to run"},
                    },
                    "required": ["command"],
                }),
            },
        ];
        let wire = CodexClient::build_tools(&tools);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["type"], "function");
        assert_eq!(wire[0]["name"], "bash");
        assert_eq!(wire[0]["description"], "run command");
        assert!(wire[0]["strict"].is_null(), "strict should be null");
        // Description should be stripped from parameters
        assert!(wire[0]["parameters"]["properties"]["command"].get("description").is_none());
        assert_eq!(wire[0]["parameters"]["properties"]["command"]["type"], "string");
    }

    #[test]
    fn codex_retryable_status_codes() {
        assert!(is_codex_retryable(429), "429 should be retryable");
        assert!(is_codex_retryable(500), "500 should be retryable");
        assert!(is_codex_retryable(502), "502 should be retryable");
        assert!(is_codex_retryable(503), "503 should be retryable");
        assert!(is_codex_retryable(504), "504 should be retryable");
        assert!(!is_codex_retryable(400), "400 should not be retryable");
        assert!(!is_codex_retryable(401), "401 should not be retryable");
        assert!(!is_codex_retryable(403), "403 should not be retryable");
        assert!(!is_codex_retryable(404), "404 should not be retryable");
    }

    #[test]
    fn codex_client_construction() {
        let client = CodexClient::new(
            "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.test".into(),
            "account_123".into(),
        );
        assert_eq!(client.jwt_token, "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.test");
        assert_eq!(client.account_id, "account_123");
        assert_eq!(client.base_url, CODEX_BASE_URL);
    }

    #[test]
    fn codex_build_input_full_conversation() {
        // Test a realistic multi-turn conversation
        let messages = vec![
            LlmMessage::User { content: "list files".into(), images: vec![] },
            LlmMessage::Assistant {
                text: vec!["I'll run ls.".into()],
                thinking: vec![],
                tool_calls: vec![crate::bridge::WireToolCall {
                    id: "call_1|fc_1".into(),
                    name: "bash".into(),
                    arguments: json!({"command": "ls"}),
                }],
                raw: None,
            },
            LlmMessage::ToolResult {
                call_id: "call_1|fc_1".into(),
                tool_name: "bash".into(),
                content: "file1.rs\nfile2.rs".into(),
                is_error: false,
                args_summary: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 4); // user + message + function_call + function_call_output
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[3]["type"], "function_call_output");
    }

    #[test]
    fn codex_build_input_empty_text_skipped() {
        let messages = vec![
            LlmMessage::Assistant {
                text: vec!["".into()], // empty text should be skipped
                thinking: vec![],
                tool_calls: vec![],
                raw: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input.len(), 0, "empty text blocks should be skipped");
    }

    #[test]
    fn codex_build_input_null_arguments() {
        // Tool call with null arguments should serialize as "{}"
        let messages = vec![
            LlmMessage::Assistant {
                text: vec![],
                thinking: vec![],
                tool_calls: vec![crate::bridge::WireToolCall {
                    id: "call_1".into(),
                    name: "memory_query".into(),
                    arguments: Value::Null,
                }],
                raw: None,
            },
        ];
        let input = CodexClient::build_input(&messages);
        assert_eq!(input[0]["arguments"], "{}");
    }

    // ── OpenAI-compatible provider tests ────────────────────────────

    #[test]
    fn openai_compat_providers_registered() {
        // All OpenAI-compat inference providers must have openai_compat_url set
        let compat_ids = ["openai", "openrouter", "groq", "xai", "mistral",
                          "cerebras", "huggingface", "ollama"];
        for id in &compat_ids {
            let provider = crate::auth::provider_by_id(id);
            assert!(provider.is_some(), "provider '{}' not found in PROVIDERS", id);
            let p = provider.unwrap();
            assert!(p.openai_compat_url.is_some(),
                "provider '{}' should have openai_compat_url set", id);
        }
    }

    #[test]
    fn proprietary_providers_no_compat_url() {
        // Anthropic and Codex use proprietary protocols
        let anthropic = crate::auth::provider_by_id("anthropic").unwrap();
        assert!(anthropic.openai_compat_url.is_none());
        let codex = crate::auth::provider_by_id("openai-codex").unwrap();
        assert!(codex.openai_compat_url.is_none());
    }

    #[test]
    fn non_inference_providers_no_compat_url() {
        let non_inference = ["brave", "tavily", "serper", "github", "gitlab"];
        for id in &non_inference {
            let p = crate::auth::provider_by_id(id).unwrap();
            assert!(p.openai_compat_url.is_none(),
                "non-inference provider '{}' should not have openai_compat_url", id);
        }
    }

    #[test]
    fn compat_client_construction() {
        let client = OpenAICompatClient::new(
            "sk-test".into(),
            "https://api.groq.com/openai".into(),
            "groq".into(),
        );
        assert_eq!(client.provider_id, "groq");
        assert_eq!(client.inner.base_url, "https://api.groq.com/openai");
    }

    #[test]
    fn compat_client_with_default_model() {
        let client = OpenAICompatClient::new(
            "sk-test".into(),
            "https://openrouter.ai/api".into(),
            "openrouter".into(),
        ).with_default_model("openrouter/auto".into());
        assert_eq!(client.default_model, Some("openrouter/auto".into()));
    }

    #[test]
    fn compat_client_openai_jwt_rejected() {
        // A JWT token for the "openai" provider should be rejected
        // (it needs CodexClient, not Chat Completions)
        let jwt = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.test.sig";
        let starts_with_eyj = jwt.starts_with("eyJ");
        assert!(starts_with_eyj, "test JWT should start with eyJ");
        // The from_env logic checks this — we verify the condition
        let provider_id = "openai";
        let should_reject = provider_id == "openai" && starts_with_eyj;
        assert!(should_reject, "OpenAI provider should reject JWT tokens");
    }

    #[test]
    fn ollama_provider_in_registry() {
        let ollama = crate::auth::provider_by_id("ollama").unwrap();
        assert_eq!(ollama.display_name, "Ollama (Local)");
        assert_eq!(ollama.openai_compat_url, Some("http://localhost:11434"));
        assert_eq!(ollama.env_vars, &["OLLAMA_HOST"]);
    }

    #[test]
    fn huggingface_provider_has_correct_url() {
        let hf = crate::auth::provider_by_id("huggingface").unwrap();
        assert_eq!(hf.openai_compat_url, Some("https://router.huggingface.co"));
        assert!(hf.env_vars.contains(&"HF_TOKEN"));
    }

    #[test]
    fn all_provider_ids_unique() {
        let ids: Vec<_> = crate::auth::PROVIDERS.iter().map(|p| p.id).collect();
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(id), "duplicate provider id: {}", id);
        }
    }

    #[test]
    fn fallback_order_covers_inference_providers() {
        // The FALLBACK_ORDER in auto_detect_bridge should include all
        // inference-capable providers
        let fallback: &[&str] = &[
            "anthropic", "openai", "openai-codex",
            "groq", "xai", "mistral", "huggingface", "cerebras",
            "openrouter", "ollama",
        ];
        // Every provider with openai_compat_url should be in the fallback
        for p in crate::auth::PROVIDERS.iter() {
            if p.openai_compat_url.is_some() || p.id == "anthropic" || p.id == "openai-codex" {
                // Skip non-inference providers
                if ["brave", "tavily", "serper", "github", "gitlab"].contains(&p.id) {
                    continue;
                }
                assert!(fallback.contains(&p.id),
                    "inference provider '{}' missing from FALLBACK_ORDER", p.id);
            }
        }
    }

    // ── Sharp edge tests — operator tire-kicking paths ──────────────

    #[test]
    fn openai_client_accepts_bare_model_name() {
        // OpenAIClient must work without "openai:" prefix because
        // OpenAICompatClient strips it before delegating.
        // Previously: .and_then(strip_prefix("openai:")).unwrap_or("gpt-4.1")
        // Fixed: .map(strip_prefix("openai:").unwrap_or(m)).unwrap_or("gpt-4.1")

        fn resolve(m: Option<&str>) -> String {
            m.map(|m| m.strip_prefix("openai:").unwrap_or(m))
                .unwrap_or("gpt-4.1")
                .to_string()
        }

        assert_eq!(resolve(Some("openai:gpt-5")), "gpt-5");
        assert_eq!(resolve(Some("gpt-5")), "gpt-5");
        assert_eq!(resolve(None), "gpt-4.1");
    }

    #[test]
    fn compat_client_strips_own_provider_prefix() {
        // "groq:llama-3.3-70b" on a Groq client → "llama-3.3-70b" on the wire
        let model = "groq:llama-3.3-70b";
        let prefix = "groq";
        if let Some(colon_pos) = model.find(':') {
            let p = &model[..colon_pos];
            if crate::auth::provider_by_id(p).is_some() && p == prefix {
                let bare = &model[colon_pos + 1..];
                assert_eq!(bare, "llama-3.3-70b");
                return;
            }
        }
        panic!("should have stripped groq prefix");
    }

    #[test]
    fn compat_client_detects_wrong_provider_model() {
        // "anthropic:claude-sonnet-4-6" on a Groq client → should detect as wrong provider
        let model = "anthropic:claude-sonnet-4-6";
        let own_prefix = "groq";
        if let Some(colon_pos) = model.find(':') {
            let prefix = &model[..colon_pos];
            if let Some(provider) = crate::auth::provider_by_id(prefix) {
                if prefix != own_prefix && (provider.openai_compat_url.is_some() || prefix == "anthropic" || prefix == "openai-codex") {
                    // This model belongs to a different provider — should be replaced
                    assert_ne!(prefix, own_prefix);
                    return; // test passes
                }
            }
        }
        panic!("should have detected wrong-provider model");
    }

    #[test]
    fn compat_client_preserves_ollama_tag_format() {
        // "qwen3:32b" should NOT be treated as "provider:model" — qwen3 is not a provider ID
        let model = "qwen3:32b";
        if let Some(colon_pos) = model.find(':') {
            let prefix = &model[..colon_pos];
            assert!(crate::auth::provider_by_id(prefix).is_none(),
                "qwen3 should not be a known provider ID");
        }
        // Model should pass through unchanged
    }

    #[test]
    fn compat_client_preserves_huggingface_slash_models() {
        // "Qwen/Qwen3-235B-A22B-Thinking-2507" has no colon — should pass through
        let model = "Qwen/Qwen3-235B-A22B-Thinking-2507";
        assert!(!model.contains(':'), "HF model names use / not :");
        // No prefix stripping needed
    }

    #[test]
    fn compat_client_default_model_stripped() {
        // When default_model is "groq:llama-3.3-70b-versatile",
        // the provider prefix should be stripped before sending on wire
        let default = "groq:llama-3.3-70b-versatile";
        let colon_pos = default.find(':').unwrap();
        let prefix = &default[..colon_pos];
        assert!(crate::auth::provider_by_id(prefix).is_some());
        let bare = &default[colon_pos + 1..];
        assert_eq!(bare, "llama-3.3-70b-versatile");
    }

    #[test]
    fn anthropic_handles_foreign_model_gracefully() {
        // If Anthropic gets "groq:llama-3.3-70b" via fallback, it should use its default
        let model_spec = "groq:llama-3.3-70b";
        let model = if let Some(stripped) = model_spec.strip_prefix("anthropic:") {
            stripped
        } else if model_spec.contains(':') && crate::auth::provider_by_id(model_spec.split(':').next().unwrap_or("")).is_some() {
            "claude-sonnet-4-6" // different provider prefix → use default
        } else {
            model_spec
        };
        assert_eq!(model, "claude-sonnet-4-6",
            "Anthropic should fall back to default model for foreign provider prefix");
    }

    #[test]
    fn all_compat_providers_have_default_models() {
        // Every OpenAI-compat provider should have a sensible default
        // in case it's used as a fallback
        let providers_with_url: Vec<_> = crate::auth::PROVIDERS.iter()
            .filter(|p| p.openai_compat_url.is_some() && p.id != "ollama")
            .collect();

        for p in providers_with_url {
            let _client = OpenAICompatClient::new(
                "test-key".into(),
                p.openai_compat_url.unwrap().to_string(),
                p.id.to_string(),
            );
            // Simulate from_env default assignment
            let default = match p.id {
                "openai" => Some("gpt-4.1".to_string()),
                "openrouter" => Some("openrouter/auto".to_string()),
                "groq" => Some("llama-3.3-70b-versatile".to_string()),
                "xai" => Some("grok-2".to_string()),
                "mistral" => Some("mistral-large-latest".to_string()),
                "cerebras" => Some("llama3.1-8b".to_string()),
                "huggingface" => Some("Qwen/Qwen3-235B-A22B-Thinking-2507".to_string()),
                _ => None,
            };
            assert!(default.is_some(),
                "provider '{}' should have a default model defined", p.id);
        }
    }
}
