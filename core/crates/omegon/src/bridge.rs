//! LLM Bridge — trait abstraction for LLM providers.
//!
//! Native Rust clients (AnthropicClient, OpenAIClient, CodexClient,
//! OpenAICompatClient) implement LlmBridge directly via reqwest + SSE.
//! NullBridge handles the no-provider-configured case.
//! MockBridge provides scripted responses for testing.

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

// ─── Cache control ──────────────────────────────────────────────────────────

/// Sentinel inserted between stable and dynamic system prompt segments.
///
/// Providers that support prompt caching (e.g. Anthropic) split on this to
/// place cache_control breakpoints on the stable prefix. Providers without
/// caching ignore it — it's an HTML comment, invisible to the model.
pub const CACHE_BOUNDARY: &str = "\n<!-- cache-boundary -->\n";

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
    /// Original filesystem path when the attachment came from a local file.
    /// Preserved so the agent can redisplay the exact artifact via the
    /// existing view/display pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageAttachment>,
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
                content,
                tool_name,
                images,
                ..
            } => {
                content.len()
                    + tool_name.len()
                    + images
                        .iter()
                        .map(|img| img.data.len() + img.media_type.len())
                        .sum::<usize>()
            }
        }
    }
}

impl ImageAttachment {
    /// Build a user-image attachment from an image content block data URI.
    pub fn from_content_block(block: &ContentBlock, source_path: Option<String>) -> Option<Self> {
        let ContentBlock::Image { url, media_type } = block else {
            return None;
        };
        let (uri_media_type, data) = parse_data_uri(url)?;
        Some(Self {
            data: data.to_string(),
            media_type: if media_type.is_empty() {
                uri_media_type.to_string()
            } else {
                media_type.clone()
            },
            source_path,
        })
    }
}

fn parse_data_uri(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix("data:")?;
    let (media_type, data) = rest.split_once(";base64,")?;
    if media_type.starts_with("image/") && !data.is_empty() {
        Some((media_type, data))
    } else {
        None
    }
}

/// A tool call in the wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Provider-neutral semantic expectation at a block boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryExpectation {
    MoreReasoning,
    MoreContent,
    Terminal,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderContinuityKind {
    HiddenReasoning,
    OpaqueProviderState,
}

/// Events streamed from the bridge during an LLM call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)] // Done variant consumed immediately, never cloned
pub enum LlmEvent {
    /// Initial event with partial message — we ignore the content but must accept the variant.
    #[serde(rename = "start")]
    Start,
    /// Transport activity that carries no model-visible progress. This proves
    /// the connection is alive but must not extend semantic-progress budgets.
    #[serde(rename = "transport_heartbeat")]
    TransportHeartbeat,
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
    /// Provider-normalized expectation after a semantic block boundary.
    /// Adapters emit this only when the upstream protocol provides enough
    /// evidence to distinguish reasoning, content, terminal, or unknown gaps.
    #[serde(rename = "boundary")]
    Boundary { expectation: BoundaryExpectation },
    /// Minimum provider-defined bytes explicitly required for a later request.
    /// Raw responses, headers, credentials, and transport objects are forbidden.
    #[serde(rename = "provider_continuity")]
    ProviderContinuity {
        kind: ProviderContinuityKind,
        bytes: Vec<u8>,
    },
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
        /// Cache-creation tokens (Anthropic prompt caching; 0 if not applicable)
        #[serde(default)]
        cache_creation_tokens: u64,
        /// Parsed provider quota/headroom telemetry from response headers or status endpoints.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
    },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "upstream_failure")]
    UpstreamFailure {
        failure: crate::upstream_errors::UpstreamResponseFailure,
    },
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

/// A definitive loss of the selected serving route, distinct from transient
/// capacity or transport failures. Carries no credentials or upstream payload.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ProviderRouteUnavailable {
    pub(crate) model: String,
    pub(crate) message: String,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointRouteProvenance {
    pub selected_provider_id: String,
    pub endpoint_id: String,
    pub adapter_id: String,
    pub inventory_generation: u64,
    pub contribution_generation_id: String,
    pub schema_dialect: String,
}

/// Abstraction over how we call LLM providers.
/// Native: AnthropicClient, OpenAIClient, CodexClient, OpenAICompatClient.
/// Test: MockBridge (scripted responses).
#[async_trait]
pub trait LlmBridge: Send + Sync {
    /// Validate model-level request capabilities before lease persistence or
    /// transport dispatch. Native bridges accept by default; admitted route
    /// wrappers enforce offering evidence.
    fn validate_request_capabilities(
        &self,
        _tools: &[ToolDefinition],
        _options: &StreamOptions,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>>;

    /// Route identity captured when this bridge was resolved. Provider-neutral
    /// callers use it to retain serving identity when a compatible fallback was
    /// selected before dispatch.
    fn serving_model_hint(&self) -> Option<&str> {
        None
    }

    fn selected_model_hint(&self) -> Option<&str> {
        None
    }

    fn native_model_hint(&self) -> Option<&str> {
        None
    }

    fn endpoint_route_provenance_hint(&self) -> Option<&EndpointRouteProvenance> {
        None
    }

    fn credential_source_class_hint(&self) -> Option<&str> {
        None
    }

    fn route_is_disconnected(&self) -> bool {
        false
    }

    /// Graceful shutdown. Default no-op for native clients.
    async fn shutdown(&self) {}
}

// ─── Null bridge (no provider configured) ──────────────────────────────────

/// Placeholder bridge used when no LLM provider is available.
/// Every stream call returns an error telling the user to /connect.
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
            "No LLM provider available. Use /connect to configure a connection or /model to choose a route."
        );
    }

    fn route_is_disconnected(&self) -> bool {
        true
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
    fn boundary_event_round_trips_without_render_payload() {
        let event = LlmEvent::Boundary {
            expectation: BoundaryExpectation::MoreReasoning,
        };
        let json = serde_json::to_string(&event).expect("serialize boundary");
        assert_eq!(
            json,
            r#"{"type":"boundary","expectation":"more_reasoning"}"#
        );
        let parsed: LlmEvent = serde_json::from_str(&json).expect("deserialize boundary");
        assert!(matches!(
            parsed,
            LlmEvent::Boundary {
                expectation: BoundaryExpectation::MoreReasoning
            }
        ));
    }

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
            images: vec![],
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
    async fn connect_null_bridge_returns_one_action_without_catalog() {
        let bridge = NullBridge;
        let result = bridge.stream("", &[], &[], &StreamOptions::default()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("/connect"), "should mention /connect: {err}");
        assert_eq!(err.lines().count(), 1, "no provider catalog: {err}");
        assert!(!err.contains("/login"));
        assert!(
            err.contains("No LLM provider"),
            "should explain no provider: {err}"
        );
    }
}
