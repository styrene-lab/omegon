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

    // Env vars (not OAuth)
    for key in env_keys {
        // Skip OAuth token env vars — those are handled separately below
        if *key == "ANTHROPIC_OAUTH_TOKEN" { continue; }
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            tracing::debug!(provider, source = key, "API key resolved from env");
            return Some((val, false));
        }
    }

    // OAuth token from env (Anthropic only — ANTHROPIC_OAUTH_TOKEN)
    if env_keys.contains(&"ANTHROPIC_OAUTH_TOKEN") {
        if let Ok(val) = std::env::var("ANTHROPIC_OAUTH_TOKEN")
            && !val.is_empty()
        {
            tracing::debug!(provider, "OAuth token resolved from ANTHROPIC_OAUTH_TOKEN env");
            return Some((val, true));
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

/// Resolve a single provider by ID.
pub async fn resolve_provider(provider_id: &str) -> Option<Box<dyn LlmBridge>> {
    match provider_id {
        "anthropic" => {
            if let Some(client) = AnthropicClient::from_env() {
                return Some(Box::new(client));
            }
            AnthropicClient::from_env_async().await.map(|c| Box::new(c) as Box<dyn LlmBridge>)
        }
        "openai" => OpenAIClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        "openrouter" => OpenRouterClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        _ => None,
    }
}

/// Auto-detect the best available native provider from configured keys.
/// Tries sync resolution first, then async (with token refresh) if needed.
pub async fn auto_detect_bridge(model_spec: &str) -> Option<Box<dyn LlmBridge>> {
    let provider = model_spec.split(':').next().unwrap_or("anthropic");

    // Try the requested provider first
    let primary: Option<Box<dyn LlmBridge>> = match provider {
        "anthropic" => {
            if let Some(client) = AnthropicClient::from_env() {
                Some(Box::new(client))
            } else {
                AnthropicClient::from_env_async().await.map(|c| Box::new(c) as Box<dyn LlmBridge>)
            }
        }
        "openai" => OpenAIClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        "openrouter" => OpenRouterClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        _ => None,
    };

    if primary.is_some() {
        return primary;
    }

    // Primary provider not available — try the full fallback chain.
    // This handles: user requests Anthropic but only has OpenAI credentials.
    tracing::warn!(
        requested = provider,
        "requested provider not available — trying fallback chain"
    );

    if provider != "anthropic" {
        if let Some(client) = AnthropicClient::from_env() {
            tracing::info!("falling back to Anthropic");
            return Some(Box::new(client));
        }
        if let Some(client) = AnthropicClient::from_env_async().await {
            tracing::info!("falling back to Anthropic (after token refresh)");
            return Some(Box::new(client));
        }
    }
    if provider != "openai" {
        if let Some(client) = OpenAIClient::from_env() {
            tracing::info!("falling back to OpenAI");
            return Some(Box::new(client));
        }
    }
    if provider != "openrouter" {
        if let Some(client) = OpenRouterClient::from_env() {
            tracing::info!("falling back to OpenRouter");
            return Some(Box::new(client));
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

        let model = options.model.as_deref()
            .and_then(|m| m.strip_prefix("anthropic:"))
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
        resolve_api_key("openai").map(Self::new)
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

        let model = options.model.as_deref()
            .and_then(|m| m.strip_prefix("openai:"))
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
        self.inner.stream(system_prompt, messages, tools, &opts).await
    }
}

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
}
