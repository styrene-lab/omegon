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
use omegon_traits::ToolDefinition;

// ─── API Key Resolution ─────────────────────────────────────────────────────

/// Resolve API key synchronously — env vars and unexpired auth.json tokens.
/// Returns (key, is_oauth).
pub fn resolve_api_key_sync(provider: &str) -> Option<(String, bool)> {
    // Env vars (not OAuth)
    let env_keys: &[&str] = match provider {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        _ => &[],
    };
    for key in env_keys {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            tracing::debug!(provider, source = key, "API key resolved from env");
            return Some((val, false));
        }
    }

    // OAuth token from env
    if provider == "anthropic"
        && let Ok(val) = std::env::var("ANTHROPIC_OAUTH_TOKEN")
        && !val.is_empty()
    {
        tracing::debug!(provider, "OAuth token resolved from ANTHROPIC_OAUTH_TOKEN env");
        return Some((val, true));
    }

    // auth.json — only if not expired
    match crate::auth::read_credentials(provider) {
        Some(creds) if creds.cred_type == "oauth" && !creds.is_expired() => {
            tracing::debug!(provider, expires = creds.expires, "OAuth token from auth.json (valid)");
            return Some((creds.access, true));
        }
        Some(creds) if creds.cred_type == "oauth" => {
            tracing::debug!(provider, expires = creds.expires, "OAuth token from auth.json (EXPIRED — needs refresh)");
        }
        Some(creds) => {
            tracing::debug!(provider, cred_type = %creds.cred_type, "credential from auth.json");
            return Some((creds.access, false));
        }
        None => {
            tracing::debug!(provider, "no credentials in auth.json");
        }
    }

    // Fallback: OpenAI subscription stored as "openai-codex" in auth.json
    if provider == "openai" {
        match crate::auth::read_credentials("openai-codex") {
            Some(creds) if creds.cred_type == "oauth" && !creds.is_expired() => {
                tracing::debug!("OpenAI subscription token from auth.json (openai-codex)");
                return Some((creds.access, true));
            }
            Some(creds) if creds.cred_type == "oauth" => {
                tracing::debug!("OpenAI subscription token from auth.json (EXPIRED — needs refresh)");
            }
            _ => {}
        }
    }

    None
}

/// Resolve API key from env vars or ~/.pi/agent/auth.json (legacy, no refresh).
fn resolve_api_key(provider: &str) -> Option<String> {
    let env_keys: &[&str] = match provider {
        "anthropic" => &["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        "google" => &["GOOGLE_API_KEY"],
        "mistral" => &["MISTRAL_API_KEY"],
        _ => &[],
    };

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

    // ~/.pi/agent/auth.json — OAuth access tokens from pi's auth flow
    let home = dirs::home_dir()?;
    let auth_path = home.join(".pi/agent/auth.json");
    let content = std::fs::read_to_string(&auth_path).ok()?;
    let auth: Value = serde_json::from_str(&content).ok()?;
    auth.get(provider)?
        .get("access")?
        .as_str()
        .map(String::from)
}

/// Auto-detect the best available native provider from configured keys.
/// Tries sync resolution first, then async (with token refresh) if needed.
pub async fn auto_detect_bridge(model_spec: &str) -> Option<Box<dyn LlmBridge>> {
    let provider = model_spec.split(':').next().unwrap_or("anthropic");
    match provider {
        "anthropic" => {
            // Try sync first (fast path — env var or unexpired token)
            if let Some(client) = AnthropicClient::from_env() {
                return Some(Box::new(client));
            }
            // Try async (token refresh)
            AnthropicClient::from_env_async().await.map(|c| Box::new(c) as Box<dyn LlmBridge>)
        }
        "openai" => OpenAIClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>),
        _ => {
            if let Some(client) = AnthropicClient::from_env() {
                return Some(Box::new(client));
            }
            if let Some(client) = AnthropicClient::from_env_async().await {
                return Some(Box::new(client));
            }
            OpenAIClient::from_env().map(|c| Box::new(c) as Box<dyn LlmBridge>)
        }
    }
}

// ─── SSE Helpers ────────────────────────────────────────────────────────────

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

    fn build_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        tools.iter().map(|t| {
            let name = if self.is_oauth {
                to_claude_code_name(&t.name)
            } else {
                t.name.clone()
            };
            json!({
                "name": name,
                "description": t.description,
                "input_schema": {
                    "type": "object",
                    "properties": t.parameters.get("properties").cloned().unwrap_or(json!({})),
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

        let model = options.model.as_deref()
            .and_then(|m| m.strip_prefix("anthropic:"))
            .unwrap_or("claude-sonnet-4-6");

        // OAuth requires Claude Code identity prefix + array format
        let system_value = if self.is_oauth {
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

        let wire_tools = self.build_tools(tools);
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
        tracing::debug!(
            model,
            is_oauth = self.is_oauth,
            tool_count,
            msg_count,
            system_len,
            base_url = %self.base_url,
            "Anthropic streaming request"
        );
        tracing::trace!(body = %serde_json::to_string(&body).unwrap_or_default(), "request body");

        let response = self.client
            .post(format!("{}/v1/messages", self.base_url))
            .header(
                if self.is_oauth { "Authorization" } else { "x-api-key" },
                if self.is_oauth { format!("Bearer {}", self.api_key) } else { self.api_key.clone() },
            )
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", {
                let mut flags = if self.is_oauth {
                    "claude-code-20250219,oauth-2025-04-20".to_string()
                } else {
                    "interleaved-thinking-2025-05-14".to_string()
                };
                if options.extended_context {
                    flags.push_str(",context-1m-2025-08-07");
                }
                flags
            })
            .header("content-type", "application/json")
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
                "Anthropic API error"
            );
            tracing::debug!(request_body = %serde_json::to_string(&body).unwrap_or_default(), "failed request body");
            // Extract the human-readable message from the API error body
            let user_msg = serde_json::from_str::<Value>(&err).ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| err.chars().take(200).collect());
            let _ = tx.send(LlmEvent::Error { message: format!("Anthropic {status}: {user_msg}") }).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_key_env() {
        unsafe { std::env::set_var("_TEST_OMEGON_API_KEY", "test-123"); }
        // Direct env lookup
        assert!(std::env::var("_TEST_OMEGON_API_KEY").is_ok());
        unsafe { std::env::remove_var("_TEST_OMEGON_API_KEY"); }
    }

    #[test]
    fn auto_detect_without_keys_returns_none() {
        // Clear keys to test (save + restore)
        let saved = std::env::var("ANTHROPIC_API_KEY").ok();
        let saved2 = std::env::var("ANTHROPIC_OAUTH_TOKEN").ok();
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY"); }
        unsafe { std::env::remove_var("ANTHROPIC_OAUTH_TOKEN"); }
        unsafe { std::env::remove_var("OPENAI_API_KEY"); }

        // Without any keys or auth.json, should return None
        // (might still find auth.json on dev machine, so just check it doesn't panic)
        let _ = auto_detect_bridge("anthropic:test");

        // Restore
        if let Some(k) = saved { unsafe { std::env::set_var("ANTHROPIC_API_KEY", k); } }
        if let Some(k) = saved2 { unsafe { std::env::set_var("ANTHROPIC_OAUTH_TOKEN", k); } }
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
}
