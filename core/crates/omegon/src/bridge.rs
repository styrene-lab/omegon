//! LLM Bridge — subprocess interface to LLM providers.
//!
//! Spawns a long-lived Node.js process that translates between Omegon's
//! wire format and pi-ai's provider-specific protocols. The bridge is a
//! translator, not a passthrough — Omegon defines the message contract,
//! the bridge adapts it for whatever provider library is on the other side.
//!
//! Wire format: ndjson over stdin/stdout. Rust defines the types; JS conforms.

use async_trait::async_trait;
use omegon_traits::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};

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
            LlmMessage::Assistant { text, thinking, tool_calls, .. } => {
                let text_len: usize = text.iter().map(|t| t.len()).sum();
                let think_len: usize = thinking.iter().map(|t| t.len()).sum();
                let tc_len: usize = tool_calls.iter().map(|tc| {
                    tc.name.len() + tc.arguments.to_string().len()
                }).sum();
                text_len + think_len + tc_len
            }
            LlmMessage::ToolResult { content, tool_name, .. } => {
                content.len() + tool_name.len()
            }
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
}

/// Abstraction over how we call LLM providers.
/// Primary: SubprocessBridge (pi-ai via Node.js).
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

// ─── Subprocess bridge ─────────────────────────────────────────────────────

pub struct SubprocessBridge {
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    next_id: AtomicU64,
    // FIXME: single-consumer receiver can't support multiplexed requests.
    // Phase 0 is sequential (one stream at a time). Phase 1+ needs a
    // HashMap<u64, Sender> routing table for concurrent requests.
    response_rx: Arc<Mutex<mpsc::Receiver<BridgeResponse>>>,
    _child: Child,
}

impl SubprocessBridge {
    /// Send a graceful shutdown message to the bridge subprocess.
    /// The bridge JS exits cleanly on receiving this, which is better
    /// than relying on kill_on_drop's SIGKILL.
    pub async fn shutdown(&self) {
        let _ = self.send_request("shutdown", serde_json::json!({})).await;
        // Give the bridge 500ms to exit cleanly before kill_on_drop fires
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

impl SubprocessBridge {
    /// Spawn the Node.js bridge subprocess.
    pub async fn spawn(bridge_script: &Path, node_path: &str) -> anyhow::Result<Self> {
        let mut child = Command::new(node_path)
            .arg(bridge_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        // Wait for readiness signal on stderr, log everything else.
        // The bridge emits exactly "llm-bridge: ready\n" — match the prefix
        // to avoid false positives from unrelated warnings containing "ready".
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let mut ready_tx = Some(ready_tx);
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.starts_with("llm-bridge: ready")
                    && let Some(tx) = ready_tx.take() {
                        let _ = tx.send(());
                    }
                tracing::debug!(target: "llm_bridge", "{}", line);
            }
        });

        // Wait up to 10s for the bridge to signal readiness
        tokio::select! {
            _ = ready_rx => {
                tracing::debug!("Bridge signaled ready");
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                tracing::warn!("Bridge did not signal ready within 10s — proceeding anyway");
            }
        }

        let (response_tx, response_rx) = mpsc::channel(256);

        // Reader task: parse ndjson lines from stdout
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match serde_json::from_str::<BridgeResponse>(&line) {
                    Ok(resp) => {
                        if response_tx.send(resp).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(target: "llm_bridge", "Parse error: {e}\n  line: {line}");
                    }
                }
            }
        });

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: AtomicU64::new(1),
            response_rx: Arc::new(Mutex::new(response_rx)),
            _child: child,
        })
    }

    async fn send_request(&self, method: &str, params: Value) -> anyhow::Result<u64> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = BridgeRequest {
            id,
            method: method.to_string(),
            params,
        };
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(id)
    }

    pub fn default_bridge_path() -> PathBuf {
        let exe = std::env::current_exe().unwrap_or_default();
        let exe_dir = exe.parent().unwrap_or(Path::new("."));

        for candidate in [
            exe_dir.join("../bridge/llm-bridge.mjs"),
            PathBuf::from("core/bridge/llm-bridge.mjs"),
            PathBuf::from("bridge/llm-bridge.mjs"),
        ] {
            if candidate.exists() {
                return candidate;
            }
        }

        exe_dir.join("../bridge/llm-bridge.mjs")
    }
}

#[async_trait]
impl LlmBridge for SubprocessBridge {
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> anyhow::Result<mpsc::Receiver<LlmEvent>> {
        let tool_schemas: Vec<Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();

        let model = options
            .model
            .as_deref()
            .unwrap_or("anthropic:claude-sonnet-4-6");
        let reasoning = options.reasoning.as_deref().unwrap_or("medium");

        let params = serde_json::json!({
            "systemPrompt": system_prompt,
            "messages": messages,
            "tools": tool_schemas,
            "model": model,
            "reasoning": reasoning,
        });

        let req_id = self.send_request("stream", params).await?;

        let (event_tx, event_rx) = mpsc::channel(64);
        let response_rx = self.response_rx.clone();

        tokio::spawn(async move {
            let idle_timeout = std::time::Duration::from_secs(120);
            let mut rx = response_rx.lock().await;
            loop {
                let resp = match tokio::time::timeout(idle_timeout, rx.recv()).await {
                    Ok(Some(resp)) => resp,
                    Ok(None) => break, // channel closed
                    Err(_) => {
                        tracing::warn!("bridge stream idle for {}s — treating as stalled", idle_timeout.as_secs());
                        let _ = event_tx.send(LlmEvent::Error {
                            message: format!("Bridge stream idle timeout ({}s)", idle_timeout.as_secs()),
                        }).await;
                        break;
                    }
                };

                if resp.id != req_id {
                    continue;
                }

                if let Some(event) = resp.event {
                    let is_terminal =
                        matches!(event, LlmEvent::Done { .. } | LlmEvent::Error { .. });
                    let _ = event_tx.send(event).await;
                    if is_terminal {
                        break;
                    }
                }

                if resp.error.is_some() {
                    let err_msg = resp.error.unwrap_or_default();
                    let _ = event_tx
                        .send(LlmEvent::Error { message: err_msg })
                        .await;
                    break;
                }

                if resp.result.is_some() {
                    break;
                }
            }
        });

        Ok(event_rx)
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
        let msg = LlmMessage::User { content: "hello".into(), images: vec![] };
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
            LlmMessage::ToolResult { call_id, is_error, .. } => {
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
            LlmEvent::Done { message } => assert!(message.is_object()),
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
}
