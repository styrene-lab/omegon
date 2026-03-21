//! WebSocket handler — bidirectional agent protocol.
//!
//! This is the **full agent interface**. Any web UI can connect to
//! ws://localhost:PORT/ws?token=TOKEN and drive the agent.
//!
//! # Authentication
//!
//! The `token` query parameter must match the server's auth token.
//! The token is generated at server start and displayed in the terminal.
//!
//! # Server → Client (events)
//!
//! All events are JSON with a `type` field. Tool results and user-sourced
//! text are always HTML-escaped to prevent XSS in web UIs.
//!
//! # Client → Server (commands)
//!
//! - `user_prompt` — send a user message to the agent
//! - `slash_command` — execute a slash command
//! - `cancel` — cancel the current agent turn
//! - `request_snapshot` — ask for a fresh state_snapshot event

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{WebCommand, WebState};
use super::api::build_snapshot;
use omegon_traits::AgentEvent;

#[derive(Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

/// Upgrade handler — accepts the WebSocket connection after auth check.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    // Validate auth token
    let expected = state.auth_token.as_str();
    match query.token.as_deref() {
        Some(t) if t == expected => {}
        _ => {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
    }
    ws.on_upgrade(|socket| handle_socket(socket, state)).into_response()
}

/// Handle a single WebSocket connection.
async fn handle_socket(socket: WebSocket, state: WebState) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Send initial state snapshot
    let snapshot = build_snapshot(&state);
    let init_msg = json!({
        "type": "state_snapshot",
        "data": snapshot,
    });
    if ws_tx.send(Message::Text(init_msg.to_string().into())).await.is_err() {
        return;
    }

    // Subscribe to agent events
    let mut events_rx = state.events_tx.subscribe();
    let command_tx = state.command_tx.clone();
    let state_for_cmds = state.clone();

    // Channel for request_snapshot → send_task
    let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel::<Value>(4);

    // Spawn a task to forward agent events to the WebSocket
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                event = events_rx.recv() => {
                    match event {
                        Ok(event) => {
                            let msg = serialize_agent_event(&event);
                            if ws_tx.send(Message::Text(msg.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Slow client — skip missed events, send a notification
                            tracing::debug!("WebSocket client lagged by {n} events");
                            let warning = json!({"type": "system_notification", "message": format!("Skipped {n} events (slow connection)")});
                            let _ = ws_tx.send(Message::Text(warning.to_string().into())).await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                snapshot = snapshot_rx.recv() => {
                    if let Some(snap) = snapshot
                        && ws_tx.send(Message::Text(snap.to_string().into())).await.is_err() {
                            break;
                    }
                }
            }
        }
    });

    // Process inbound messages from the WebSocket client
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(cmd) = serde_json::from_str::<Value>(&text) {
                        handle_client_command(&cmd, &command_tx, &state_for_cmds, &snapshot_tx).await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }

    tracing::debug!("WebSocket client disconnected");
}

/// Process a command from a WebSocket client.
async fn handle_client_command(
    cmd: &Value,
    command_tx: &tokio::sync::mpsc::Sender<WebCommand>,
    state: &WebState,
    snapshot_tx: &tokio::sync::mpsc::Sender<Value>,
) {
    let cmd_type = cmd["type"].as_str().unwrap_or("");

    match cmd_type {
        "user_prompt" => {
            if let Some(text) = cmd["text"].as_str() {
                let _ = command_tx.send(WebCommand::UserPrompt(text.to_string())).await;
            }
        }
        "slash_command" => {
            let name = cmd["name"].as_str().unwrap_or("").to_string();
            let args = cmd["args"].as_str().unwrap_or("").to_string();
            let _ = command_tx.send(WebCommand::SlashCommand { name, args }).await;
        }
        "cancel" => {
            let _ = command_tx.send(WebCommand::Cancel).await;
        }
        "request_snapshot" => {
            let snapshot = build_snapshot(state);
            let msg = json!({ "type": "state_snapshot", "data": snapshot });
            let _ = snapshot_tx.send(msg).await;
        }
        other => {
            tracing::debug!("Unknown WebSocket command: {other}");
        }
    }
}

/// HTML-escape a string to prevent XSS in web UIs.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Serialize an AgentEvent to a JSON event message.
/// Text fields that may contain user-controlled content are HTML-escaped.
fn serialize_agent_event(event: &AgentEvent) -> Value {
    match event {
        AgentEvent::TurnStart { turn } => json!({
            "type": "turn_start",
            "turn": turn,
        }),
        AgentEvent::TurnEnd { turn } => json!({
            "type": "turn_end",
            "turn": turn,
        }),
        AgentEvent::MessageStart { role } => json!({
            "type": "message_start",
            "role": role,
        }),
        AgentEvent::MessageChunk { text } => json!({
            "type": "message_chunk",
            "text": escape_html(text),
        }),
        AgentEvent::ThinkingChunk { text } => json!({
            "type": "thinking_chunk",
            "text": escape_html(text),
        }),
        AgentEvent::MessageEnd => json!({
            "type": "message_end",
        }),
        AgentEvent::ToolStart { id, name, args } => json!({
            "type": "tool_start",
            "id": id,
            "name": name,
            "args": args,
        }),
        AgentEvent::ToolUpdate { id, partial } => {
            let text = partial.content.iter()
                .filter_map(|c| c.as_text())
                .collect::<Vec<_>>()
                .join("\n");
            json!({
                "type": "tool_update",
                "id": id,
                "partial": escape_html(&text),
            })
        }
        AgentEvent::ToolEnd { id, result, is_error } => {
            // Serialize ALL content blocks, not just the first
            let texts: Vec<&str> = result.content.iter()
                .filter_map(|c| c.as_text())
                .collect();
            let result_text = texts.join("\n");
            json!({
                "type": "tool_end",
                "id": id,
                "result": escape_html(&result_text),
                "is_error": is_error,
                "block_count": result.content.len(),
            })
        }
        AgentEvent::AgentEnd => json!({
            "type": "agent_end",
        }),
        AgentEvent::PhaseChanged { phase } => json!({
            "type": "phase_changed",
            "phase": format!("{phase:?}"),
        }),
        AgentEvent::DecompositionStarted { children } => json!({
            "type": "decomposition_started",
            "children": children,
        }),
        AgentEvent::DecompositionChildCompleted { label, success } => json!({
            "type": "decomposition_child_completed",
            "label": escape_html(label),
            "success": success,
        }),
        AgentEvent::DecompositionCompleted { merged } => json!({
            "type": "decomposition_completed",
            "merged": merged,
        }),
        AgentEvent::SystemNotification { message } => json!({
            "type": "system_notification",
            "message": escape_html(message),
        }),
        AgentEvent::HarnessStatusChanged { status_json } => json!({
            "type": "harness_status_changed",
            "status": status_json,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_turn_start() {
        let event = AgentEvent::TurnStart { turn: 5 };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "turn_start");
        assert_eq!(json["turn"], 5);
    }

    #[test]
    fn serialize_message_chunk_escapes_html() {
        let event = AgentEvent::MessageChunk { text: "<script>alert(1)</script>".into() };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "message_chunk");
        assert!(!json["text"].as_str().unwrap().contains("<script>"));
        assert!(json["text"].as_str().unwrap().contains("&lt;script&gt;"));
    }

    #[test]
    fn serialize_tool_end_all_blocks() {
        let event = AgentEvent::ToolEnd {
            id: "tc1".into(),
            result: omegon_traits::ToolResult {
                content: vec![
                    omegon_traits::ContentBlock::Text { text: "first".into() },
                    omegon_traits::ContentBlock::Text { text: "second".into() },
                ],
                details: serde_json::json!(null),
            },
            is_error: false,
        };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "tool_end");
        let result = json["result"].as_str().unwrap();
        assert!(result.contains("first"), "should contain first block");
        assert!(result.contains("second"), "should contain second block");
        assert_eq!(json["block_count"], 2);
    }

    #[test]
    fn serialize_all_event_types() {
        let events = vec![
            AgentEvent::TurnStart { turn: 1 },
            AgentEvent::TurnEnd { turn: 1 },
            AgentEvent::MessageStart { role: "assistant".into() },
            AgentEvent::MessageChunk { text: "hi".into() },
            AgentEvent::ThinkingChunk { text: "hmm".into() },
            AgentEvent::MessageEnd,
            AgentEvent::ToolStart { id: "1".into(), name: "read".into(), args: serde_json::json!({}) },
            AgentEvent::ToolUpdate {
                id: "1".into(),
                partial: omegon_traits::ToolResult {
                    content: vec![omegon_traits::ContentBlock::Text { text: "partial".into() }],
                    details: serde_json::json!(null),
                },
            },
            AgentEvent::ToolEnd {
                id: "1".into(),
                result: omegon_traits::ToolResult {
                    content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
                    details: serde_json::json!(null),
                },
                is_error: false,
            },
            AgentEvent::AgentEnd,
            AgentEvent::PhaseChanged { phase: omegon_traits::LifecyclePhase::Idle },
            AgentEvent::DecompositionStarted { children: vec!["a".into()] },
            AgentEvent::DecompositionChildCompleted { label: "a".into(), success: true },
            AgentEvent::DecompositionCompleted { merged: true },
            AgentEvent::SystemNotification { message: "test".into() },
        ];
        for event in &events {
            let json = serialize_agent_event(event);
            assert!(json["type"].is_string(), "event should have a type field");
        }
        assert_eq!(events.len(), 15, "should cover all 15 AgentEvent variants");
    }

    #[test]
    fn escape_html_works() {
        assert_eq!(escape_html("<b>bold</b>"), "&lt;b&gt;bold&lt;/b&gt;");
        assert_eq!(escape_html("a&b"), "a&amp;b");
        assert_eq!(escape_html("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(escape_html("safe text"), "safe text");
    }

    #[test]
    fn system_notification_escapes_html() {
        let event = AgentEvent::SystemNotification { message: "use <br> for newlines".into() };
        let json = serialize_agent_event(&event);
        assert!(!json["message"].as_str().unwrap().contains("<br>"));
    }
}
