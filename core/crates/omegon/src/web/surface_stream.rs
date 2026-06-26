//! Browser-native surface stream WebSocket.

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::WebState;
use omegon_traits::AgentEvent;

#[derive(Debug, Clone, Serialize)]
struct WebSurfaceStreamEnvelope {
    schema_version: u8,
    session_id: String,
    revision: u64,
    #[serde(rename = "type")]
    event_type: String,
    surface: Option<String>,
    payload: Value,
}

impl WebSurfaceStreamEnvelope {
    fn new(
        session_id: impl Into<String>,
        revision: u64,
        event_type: impl Into<String>,
        surface: Option<&str>,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: 1,
            session_id: session_id.into(),
            revision,
            event_type: event_type.into(),
            surface: surface.map(str::to_string),
            payload,
        }
    }

    fn default_session(
        revision: u64,
        event_type: &str,
        surface: Option<&str>,
        payload: Value,
    ) -> Self {
        Self::new("default", revision, event_type, surface, payload)
    }

    fn lagged(revision: u64, skipped: u64) -> Self {
        Self::default_session(
            revision,
            "stream_lagged",
            None,
            json!({
                "skipped_events": skipped,
                "message": format!("skipped {skipped} events; refetch /api/web/surfaces"),
                "recovery": {
                    "action": "refetch_snapshot",
                    "href": "/api/web/surfaces"
                }
            }),
        )
    }
}

#[derive(Deserialize)]
pub struct WebSurfaceStreamQuery {
    token: Option<String>,
}

pub async fn web_surface_stream_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WebSurfaceStreamQuery>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    if !state.web_auth.verify_query_token(query.token.as_deref()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(|socket| handle_surface_stream(socket, state))
        .into_response()
}

async fn handle_surface_stream(socket: WebSocket, state: WebState) {
    let (mut ws_tx, _ws_rx) = socket.split();
    let mut revision = 0_u64;
    let snapshot = super::surfaces::project_web_surfaces(&state);
    let initial = WebSurfaceStreamEnvelope::new(
        snapshot.session_id.clone(),
        revision,
        "snapshot",
        None,
        serde_json::to_value(snapshot).unwrap_or_else(|_| json!({})),
    );
    if ws_tx
        .send(Message::Text(
            serde_json::to_string(&initial)
                .unwrap_or_else(|_| "{}".to_string())
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    let mut events_rx = state.events_tx.subscribe();
    loop {
        match events_rx.recv().await {
            Ok(event) => {
                revision += 1;
                let message = surface_stream_event(&state, revision, event);
                if ws_tx
                    .send(Message::Text(
                        serde_json::to_string(&message)
                            .unwrap_or_else(|_| "{}".to_string())
                            .into(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                revision += 1;
                let message = WebSurfaceStreamEnvelope::lagged(revision, n);
                let _ = ws_tx
                    .send(Message::Text(
                        serde_json::to_string(&message)
                            .unwrap_or_else(|_| "{}".to_string())
                            .into(),
                    ))
                    .await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn surface_stream_event(
    state: &WebState,
    revision: u64,
    event: AgentEvent,
) -> WebSurfaceStreamEnvelope {
    match event {
        AgentEvent::TurnStart { turn } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "turn_started",
            Some("conversation"),
            json!({ "turn": turn }),
        ),
        AgentEvent::TurnEnd { .. } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "turn_completed",
            Some("conversation"),
            json!({}),
        ),
        AgentEvent::MessageChunk { text } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "conversation_segment_updated",
            Some("conversation"),
            json!({ "text": text }),
        ),
        AgentEvent::ToolStart { id, name, args } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "tool_started",
            Some("instruments"),
            json!({ "id": id, "name": name, "args": args }),
        ),
        AgentEvent::ToolUpdate { id, partial } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "tool_updated",
            Some("instruments"),
            json!({ "id": id, "partial": partial }),
        ),
        AgentEvent::ToolEnd {
            id, name, is_error, ..
        } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "tool_completed",
            Some("instruments"),
            json!({ "id": id, "name": name, "is_error": is_error }),
        ),
        AgentEvent::PermissionRequest {
            tool_name,
            path,
            respond,
            ..
        } => {
            // Capture the responder so POST /api/web/actions can answer it, and
            // hand the browser the stable id to echo back.
            let request_id = state.register_permission(&respond);
            WebSurfaceStreamEnvelope::default_session(
                revision,
                "permission_requested",
                Some("command"),
                json!({ "request_id": request_id, "tool_name": tool_name, "path": path }),
            )
        }
        AgentEvent::OperatorWaitRequest {
            prompt,
            timeout_secs,
            acknowledge,
            respond,
        } => {
            // Acknowledge immediately (2s producer deadline) and capture the
            // responder so POST /api/web/actions can deliver the decision.
            let request_id = state.register_operator_wait(&acknowledge, &respond);
            WebSurfaceStreamEnvelope::default_session(
                revision,
                "operator_wait_requested",
                Some("command"),
                json!({ "request_id": request_id, "prompt": prompt, "timeout_secs": timeout_secs }),
            )
        }
        other => WebSurfaceStreamEnvelope::default_session(
            revision,
            "surface_updated",
            None,
            json!({ "event": format!("{other:?}") }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> WebState {
        WebState::new(
            super::super::DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
        )
    }

    #[test]
    fn surface_stream_maps_tool_start() {
        let value = serde_json::to_value(surface_stream_event(
            &test_state(),
            7,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                args: serde_json::json!({"command":"pwd"}),
            },
        ))
        .expect("serialize envelope");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["revision"], 7);
        assert_eq!(value["type"], "tool_started");
        assert_eq!(value["surface"], "instruments");
        assert_eq!(value["payload"]["id"], "t1");
    }

    #[test]
    fn permission_request_registers_responder_and_emits_id() {
        let state = test_state();
        let (tx, rx) = std::sync::mpsc::channel();
        let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
        let value = serde_json::to_value(surface_stream_event(
            &state,
            3,
            AgentEvent::PermissionRequest {
                tool_name: "bash".into(),
                path: "/tmp".into(),
                kind: omegon_traits::PermissionRequestKind::PathBoundary,
                persistence: omegon_traits::PermissionPersistence::None,
                grant_path: None,
                respond: respond.clone(),
            },
        ))
        .expect("serialize envelope");
        assert_eq!(value["type"], "permission_requested");
        let id = value["payload"]["request_id"]
            .as_str()
            .expect("request_id present")
            .to_string();
        assert!(id.starts_with("perm-"));
        // The responder was captured; answering it drives the agent's channel.
        state
            .answer_permission(&id, omegon_traits::PermissionResponse::Allow)
            .expect("answer succeeds");
        assert_eq!(rx.recv().unwrap(), omegon_traits::PermissionResponse::Allow);
        // Second answer to the same id is rejected (already removed).
        assert!(
            state
                .answer_permission(&id, omegon_traits::PermissionResponse::Deny)
                .is_err()
        );
    }

    #[test]
    fn operator_wait_acknowledges_immediately_and_captures_responder() {
        let state = test_state();
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let acknowledge = std::sync::Arc::new(std::sync::Mutex::new(Some(ack_tx)));
        let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(resp_tx)));
        let value = serde_json::to_value(surface_stream_event(
            &state,
            5,
            AgentEvent::OperatorWaitRequest {
                prompt: "swap the cable".into(),
                timeout_secs: 120,
                acknowledge: acknowledge.clone(),
                respond: respond.clone(),
            },
        ))
        .expect("serialize envelope");
        assert_eq!(value["type"], "operator_wait_requested");
        // Acknowledged synchronously (beats the producer's ~2s deadline).
        assert!(ack_rx.try_recv().is_ok());
        let id = value["payload"]["request_id"]
            .as_str()
            .expect("request_id present")
            .to_string();
        assert!(id.starts_with("wait-"));
        // The browser's later decision reaches the agent channel.
        state
            .answer_operator_wait(&id, true)
            .expect("answer succeeds");
        assert_eq!(
            resp_rx.recv().unwrap(),
            omegon_traits::OperatorWaitResponse::Completed
        );
        assert!(state.answer_operator_wait(&id, false).is_err());
    }
}
