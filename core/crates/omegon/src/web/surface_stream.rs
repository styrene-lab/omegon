//! Browser-native surface stream WebSocket.

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;

use super::WebState;
use omegon_traits::AgentEvent;

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
    let initial = json!({
        "schema_version": 1,
        "session_id": snapshot.session_id,
        "revision": revision,
        "type": "snapshot",
        "surface": null,
        "payload": snapshot,
    });
    if ws_tx
        .send(Message::Text(initial.to_string().into()))
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
                    .send(Message::Text(message.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                revision += 1;
                let message = json!({
                    "schema_version": 1,
                    "session_id": "default",
                    "revision": revision,
                    "type": "error",
                    "surface": null,
                    "payload": { "message": format!("skipped {n} events; refetch /api/web/surfaces") },
                });
                let _ = ws_tx.send(Message::Text(message.to_string().into())).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

fn surface_stream_event(state: &WebState, revision: u64, event: AgentEvent) -> serde_json::Value {
    match event {
        AgentEvent::TurnStart { turn } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "turn_started",
            "surface": "conversation",
            "payload": { "turn": turn },
        }),
        AgentEvent::TurnEnd { .. } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "turn_completed",
            "surface": "conversation",
            "payload": {},
        }),
        AgentEvent::MessageChunk { text } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "conversation_segment_updated",
            "surface": "conversation",
            "payload": { "text": text },
        }),
        AgentEvent::ToolStart { id, name, args } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "tool_started",
            "surface": "instruments",
            "payload": { "id": id, "name": name, "args": args },
        }),
        AgentEvent::ToolUpdate { id, partial } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "tool_updated",
            "surface": "instruments",
            "payload": { "id": id, "partial": partial },
        }),
        AgentEvent::ToolEnd {
            id, name, is_error, ..
        } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "tool_completed",
            "surface": "instruments",
            "payload": { "id": id, "name": name, "is_error": is_error },
        }),
        AgentEvent::PermissionRequest {
            tool_name,
            path,
            respond,
            ..
        } => {
            // Capture the responder so POST /api/web/actions can answer it, and
            // hand the browser the stable id to echo back.
            let request_id = state.register_permission(&respond);
            json!({
                "schema_version": 1,
                "session_id": "default",
                "revision": revision,
                "type": "permission_requested",
                "surface": "command",
                "payload": { "request_id": request_id, "tool_name": tool_name, "path": path },
            })
        }
        AgentEvent::OperatorWaitRequest {
            prompt,
            timeout_secs,
            ..
        } => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "operator_wait_requested",
            "surface": "command",
            "payload": { "prompt": prompt, "timeout_secs": timeout_secs },
        }),
        other => json!({
            "schema_version": 1,
            "session_id": "default",
            "revision": revision,
            "type": "surface_updated",
            "surface": null,
            "payload": { "event": format!("{other:?}") },
        }),
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
        let value = surface_stream_event(
            &test_state(),
            7,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                args: serde_json::json!({"command":"pwd"}),
            },
        );
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
        let value = surface_stream_event(
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
        );
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
}
