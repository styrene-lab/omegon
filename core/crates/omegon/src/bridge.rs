//! LLM Bridge — trait abstraction for LLM providers.
//!
//! Native Rust clients (AnthropicClient, OpenAIClient, CodexClient,
//! OpenAICompatClient) implement LlmBridge directly via reqwest + SSE.
//! NullBridge handles the no-provider-configured case.
//! MockBridge provides scripted responses for testing.

use async_trait::async_trait;
use omegon_traits::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

// ─── Omegon wire types ──────────────────────────────────────────────────────
// These types define what Omegon sends and receives.
// The bridge JS translates to/from provider-specific formats.

/// An image attachment on a user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    /// Base64-encoded image data.
    pub data: String,
    /// MIME type (image/png, image/jpeg, etc.)
    pub media_type: String,
}

/// A message in the conversation — Omegon's format, not any provider's.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum LlmMessage {
    #[serde(rename = "user")]
    User {
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageAttachment>,
    },

    #[serde(rename = "assistant")]
    Assistant {
        /// Text content blocks
        #[serde(default)]
        text: Vec<String>,
        /// Thinking content blocks
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        thinking: Vec<String>,
        /// Tool calls made by the assistant
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<WireToolCall>,
        /// The raw provider message — opaque, passed back for multi-turn continuity
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<Value>,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        call_id: String,
        tool_name: String,
        content: String,
        is_error: bool,
        /// Key arguments summarized for decay context. Survives serialization.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args_summary: Option<String>,
    },
}

impl LlmMessage {
    /// Estimate character count for token budget calculations.
    pub fn char_count(&self) -> usize {
        match self {
            LlmMessage::User { content, .. } => content.len(),
            LlmMessage::Assistant {
                text,
                thinking,
                tool_calls,
                ..
            } => {
                let text_len: usize = text.iter().map(|t| t.len()).sum();
                let think_len: usize = thinking.iter().map(|t| t.len()).sum();
                let tc_len: usize = tool_calls
                    .iter()
                    .map(|tc| tc.name.len() + tc.arguments.to_string().len())
                    .sum();
                text_len + think_len + tc_len
            }
            LlmMessage::ToolResult {
                content, tool_name, ..
            } => content.len() + tool_name.len(),
        }
    }
}

/// A tool call in the wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Events streamed from the bridge during an LLM call.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum LlmEvent {
    /// Initial event with partial message — we ignore the content but must accept the variant.
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { delta: String },
    #[serde(rename = "text_start")]
    TextStart,
    #[serde(rename = "text_end")]
    TextEnd,
    #[serde(rename = "thinking_start")]
    ThinkingStart,
    #[serde(rename = "thinking_end")]
    ThinkingEnd,
    #[serde(rename = "toolcall_start")]
    ToolCallStart,
    #[serde(rename = "toolcall_delta")]
    ToolCallDelta { delta: String },
    #[serde(rename = "toolcall_end")]
    ToolCallEnd { tool_call: WireToolCall },
    #[serde(rename = "done")]
    Done {
        /// The complete assistant message in Omegon's format
        message: Value,
        /// Actual input tokens billed by the provider (0 = not reported)
        #[serde(default)]
        input_tokens: u64,
        /// Actual output tokens billed by the provider (0 = not reported)
        #[serde(default)]
        output_tokens: u64,
        /// Cache-read tokens (Anthropic prompt caching; 0 if not applicable)
        #[serde(default)]
        cache_read_tokens: u64,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

/// A bridge response line from the subprocess.
#[derive(Debug, Deserialize)]
struct BridgeResponse {
    id: u64,
    #[serde(default)]
    event: Option<LlmEvent>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

/// A request sent to the bridge subprocess.
#[derive(Serialize)]
struct BridgeRequest {
    id: u64,
    method: String,
    params: Value,
}

// ─── Bridge trait ───────────────────────────────────────────────────────────

/// Options for an LLM stream request.
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    /// Model identifier (e.g. "anthropic:claude-sonnet-4-6")
    pub model: Option<String>,
    /// Reasoning/thinking level
    pub reasoning: Option<String>,
    /// Deprecated — 1M context is native on Sonnet/Opus 4.6, no flag needed.
    /// Kept for struct compatibility but never read.
    pub extended_context: bool,
    /// Extra top-level fields to merge into the HTTP request body.
    /// Used by OpenAICompatClient to inject provider-specific options
    /// (e.g. `options: { num_ctx }` and `keep_alive` for Ollama).
    pub extra_body: std::collections::HashMap<String, serde_json::Value>,
}

/// Abstraction over how we call LLM providers.
/// Native: AnthropicClient, OpenAIClient, CodexClient, OpenAICompatClient.
/// Test: MockBridge (scripted responses).
#[async_trait]
pub trait LlmBridge: Send + Sync {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>>;

    /// Graceful shutdown. Default no-op for native clients.
    async fn shutdown(&self) {}
}

// ─── Null bridge (no provider configured) ──────────────────────────────────

/// Placeholder bridge used when no LLM provider is available.
/// Every stream call returns an error telling the user to /login.
pub struct NullBridge;

#[async_trait]
impl LlmBridge for NullBridge {
    async fn stream(
        &self,
        _system_prompt: &str,
        _messages: &[LlmMessage],
        _tools: &[ToolDefinition],
        _options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        anyhow::bail!(
            "No LLM provider configured. Use /login to authenticate:\n\
             • /login anthropic  — Claude Pro/Max (OAuth)\n\
             • /login openai     — ChatGPT Plus/Pro (OAuth)\n\
             • /login openrouter — Free tier, API key\n\
             Or: export ANTHROPIC_API_KEY=sk-..."
        );
    }
}
// ─── Mock bridge for testing ────────────────────────────────────────────────

#[cfg(test)]
pub struct MockBridge {
    pub events: Vec<LlmEvent>,
}

#[cfg(test)]
#[async_trait]
impl LlmBridge for MockBridge {
    async fn stream(
        &self,
        _system_prompt: &str,
        _messages: &[LlmMessage],
        _tools: &[ToolDefinition],
        _options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let (tx, rx) = mpsc::channel(64);
        let events = self.events.clone();
        tokio::spawn(async move {
            for event in events {
                let _ = tx.send(event).await;
            }
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_message_user_round_trip() {
        let msg = LlmMessage::User {
            content: "hello".into(),
            images: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user"#));
        let parsed: LlmMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LlmMessage::User { content, .. } => assert_eq!(content, "hello"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn llm_message_assistant_with_tool_calls() {
        let msg = LlmMessage::Assistant {
            text: vec!["I'll help".into()],
            thinking: vec![],
            tool_calls: vec![WireToolCall {
                id: "tc1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "ls"}),
            }],
            raw: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"assistant"#));
        assert!(json.contains(r#""name":"bash"#));
        // Thinking should be omitted (skip_serializing_if)
        assert!(!json.contains("thinking"));

        let parsed: LlmMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LlmMessage::Assistant { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "bash");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn llm_message_tool_result_round_trip() {
        let msg = LlmMessage::ToolResult {
            call_id: "tc1".into(),
            tool_name: "read".into(),
            content: "file contents here".into(),
            is_error: false,
            args_summary: Some("test.txt".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"tool_result"#));
        let parsed: LlmMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LlmMessage::ToolResult {
                call_id, is_error, ..
            } => {
                assert_eq!(call_id, "tc1");
                assert!(!is_error);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn llm_event_deserialization() {
        let text_delta = r#"{"type":"text_delta","delta":"hello "}"#;
        let event: LlmEvent = serde_json::from_str(text_delta).unwrap();
        match event {
            LlmEvent::TextDelta { delta } => assert_eq!(delta, "hello "),
            _ => panic!("expected TextDelta"),
        }

        let done = r#"{"type":"done","message":{"text":"done"}}"#;
        let event: LlmEvent = serde_json::from_str(done).unwrap();
        match event {
            LlmEvent::Done { message, .. } => assert!(message.is_object()),
            _ => panic!("expected Done"),
        }

        let error = r#"{"type":"error","message":"rate limited"}"#;
        let event: LlmEvent = serde_json::from_str(error).unwrap();
        match event {
            LlmEvent::Error { message } => assert!(message.contains("rate")),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn llm_event_toolcall_end() {
        let json = r#"{"type":"toolcall_end","tool_call":{"id":"tc1","name":"edit","arguments":{"path":"foo.rs"}}}"#;
        let event: LlmEvent = serde_json::from_str(json).unwrap();
        match event {
            LlmEvent::ToolCallEnd { tool_call } => {
                assert_eq!(tool_call.name, "edit");
                assert_eq!(tool_call.id, "tc1");
            }
            _ => panic!("expected ToolCallEnd"),
        }
    }

    #[test]
    fn bridge_response_with_event() {
        let json = r#"{"id":1,"event":{"type":"text_delta","delta":"hi"}}"#;
        let resp: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.event.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn bridge_response_with_error() {
        let json = r#"{"id":2,"error":"connection refused"}"#;
        let resp: BridgeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, 2);
        assert!(resp.event.is_none());
        assert_eq!(resp.error.unwrap(), "connection refused");
    }

    #[tokio::test]
    async fn null_bridge_returns_login_error() {
        let bridge = NullBridge;
        let result = bridge.stream("", &[], &[], &StreamOptions::default()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("/login"), "should mention /login: {err}");
        assert!(
            err.contains("No LLM provider"),
            "should explain no provider: {err}"
        );
    }
}
