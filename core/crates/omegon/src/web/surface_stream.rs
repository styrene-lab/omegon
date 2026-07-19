//! Browser-native surface stream WebSocket.

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::HeaderMap;
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
    headers: HeaderMap,
    Query(query): Query<WebSurfaceStreamQuery>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    authorize_surface_stream(ws, headers, query, state, None)
}

pub async fn native_session_surface_stream_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<WebSurfaceStreamQuery>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    authorize_surface_stream(ws, headers, query, state, Some(session_id))
}

fn validate_surface_stream_session_id(
    session_id: Option<&str>,
) -> Result<(), axum::http::StatusCode> {
    if let Some(session_id) = session_id
        && session_id != "default"
    {
        return Err(axum::http::StatusCode::NOT_FOUND);
    }
    Ok(())
}

fn require_surface_stream_operation(
    principal: &super::rbac::WebPrincipal,
    session_id: Option<&str>,
) -> Result<(), axum::http::StatusCode> {
    super::rbac::require_principal_operation(
        principal,
        omegon_rbac::OmegonOperation::SurfaceStream,
        &super::rbac::RbacContext {
            route: if session_id.is_some() {
                "/api/sessions/{session_id}/surfaces/stream"
            } else {
                "/api/web/surfaces/stream"
            },
            session_id,
            ..super::rbac::RbacContext::default()
        },
    )
    .map_err(|error| error.status())
}

fn authorize_surface_stream(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    query: WebSurfaceStreamQuery,
    state: WebState,
    session_id: Option<String>,
) -> axum::response::Response {
    if let Err(status) = validate_surface_stream_session_id(session_id.as_deref()) {
        return status.into_response();
    }
    let principal = if query.token.is_some() {
        if !state.web_auth.verify_query_token(query.token.as_deref()) {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        let assertion = match super::rbac::proxy_identity_assertion_from_headers(&headers) {
            Ok(assertion) => assertion,
            Err(error) => return error.status().into_response(),
        };
        if let Err(error) =
            super::rbac::validate_proxy_identity_assertion(&state, assertion.as_ref())
        {
            return error.status().into_response();
        }
        if assertion.is_some() {
            match super::rbac::principal_from_headers(&state, &headers) {
                Ok(principal) => principal,
                Err(error) => return error.status().into_response(),
            }
        } else {
            super::rbac::current_web_principal(&state)
        }
    } else {
        match super::rbac::principal_from_headers(&state, &headers) {
            Ok(principal) => principal,
            Err(error) => return error.status().into_response(),
        }
    };
    if let Err(status) = require_surface_stream_operation(&principal, session_id.as_deref()) {
        return status.into_response();
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
        AgentEvent::ToolStart {
            id,
            name,
            args,
            provenance,
        } => {
            let args = state.redact_web_value(&args);
            WebSurfaceStreamEnvelope::default_session(
                revision,
                "tool_started",
                Some("instruments"),
                json!({ "id": id, "name": name, "args": args, "provenance": provenance }),
            )
        }
        AgentEvent::ToolUpdate { id, partial } => {
            let mut partial = partial;
            partial.tail = state.redact_web_text(&partial.tail);
            WebSurfaceStreamEnvelope::default_session(
                revision,
                "tool_updated",
                Some("instruments"),
                json!({ "id": id, "partial": partial }),
            )
        }
        AgentEvent::ToolEnd {
            id,
            name,
            is_error,
            provenance,
            ..
        } => WebSurfaceStreamEnvelope::default_session(
            revision,
            "tool_completed",
            Some("instruments"),
            json!({ "id": id, "name": name, "is_error": is_error, "provenance": provenance }),
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
        AgentEvent::PlanUpdated { projection } => {
            if let Ok(mut plan) = state.plan_surface.lock() {
                *plan = projection.clone();
            }
            WebSurfaceStreamEnvelope::default_session(
                revision,
                "plan_updated",
                Some("plan"),
                serde_json::to_value(&projection).unwrap_or_else(|_| json!({})),
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
        WebState::with_auth_state(
            super::super::DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
            super::super::auth::WebAuthState::ephemeral_generated("test".to_string()),
        )
    }

    fn secret_test_state() -> WebState {
        let dir = tempfile::tempdir().unwrap();
        let secrets = std::sync::Arc::new(omegon_secrets::SecretsManager::new(dir.path()).unwrap());
        secrets.register_redaction_secret("TEST_WEB_TOKEN", "super-secret-token");
        WebState::with_auth_state_and_secrets(
            super::super::DashboardHandles::default(),
            tokio::sync::broadcast::channel(16).0,
            crate::web::auth::WebAuthState::ephemeral_generated("test-token".to_string()),
            Some(secrets),
        )
    }

    fn auth_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer test"),
        );
        headers
    }

    fn trusted_proxy_headers(role: &str) -> HeaderMap {
        let mut headers = auth_headers();
        headers.insert(
            "Omegon-Principal-Issuer",
            axum::http::HeaderValue::from_static("auspex"),
        );
        headers.insert(
            "Omegon-Principal-Subject",
            axum::http::HeaderValue::from_static("user:alice"),
        );
        headers.insert(
            "Omegon-Principal-Role",
            axum::http::HeaderValue::from_str(role).unwrap(),
        );
        headers.insert(
            "Auspex-Proxy-Identity-Fingerprint",
            axum::http::HeaderValue::from_static("fp-123"),
        );
        headers
    }

    fn strict_proxy_state() -> WebState {
        test_state().with_web_authority(crate::web::WebAuthorityConfig {
            trusted_proxy: Some(crate::web::WebTrustedProxyIdentity {
                schema_version: 1,
                subject: "user:alice".to_string(),
                fingerprint: "fp-123".to_string(),
                strict_daemon_identity: true,
            }),
            require_proxy_identity: true,
        })
    }

    #[test]
    fn native_session_surface_stream_rejects_unknown_session_id() {
        assert_eq!(
            validate_surface_stream_session_id(Some("missing")),
            Err(axum::http::StatusCode::NOT_FOUND)
        );
        assert!(validate_surface_stream_session_id(Some("default")).is_ok());
        assert!(validate_surface_stream_session_id(None).is_ok());
    }

    #[test]
    fn surface_stream_operation_denies_blocked_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Blocked;

        assert_eq!(
            require_surface_stream_operation(
                &super::super::rbac::WebPrincipal::from_state(&state),
                Some("default")
            ),
            Err(axum::http::StatusCode::FORBIDDEN)
        );
        assert_eq!(
            require_surface_stream_operation(
                &super::super::rbac::WebPrincipal::from_state(&state),
                None
            ),
            Err(axum::http::StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn surface_stream_operation_allows_monitor_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Monitor;

        assert!(
            require_surface_stream_operation(
                &super::super::rbac::WebPrincipal::from_state(&state),
                Some("default")
            )
            .is_ok()
        );
        assert!(
            require_surface_stream_operation(
                &super::super::rbac::WebPrincipal::from_state(&state),
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn surface_stream_principal_headers_accept_trusted_proxy() {
        let state = test_state();
        let principal =
            super::super::rbac::principal_from_headers(&state, &trusted_proxy_headers("monitor"))
                .expect("trusted proxy principal");

        assert_eq!(
            principal.issuer,
            super::super::rbac::WebPrincipalIssuer::TrustedProxy
        );
        assert_eq!(principal.role, styrene_rbac::Role::Monitor);
        assert!(require_surface_stream_operation(&principal, Some("default")).is_ok());
    }

    #[test]
    fn surface_stream_strict_proxy_state_accepts_matching_assertion() {
        let state = strict_proxy_state();
        let principal =
            super::super::rbac::principal_from_headers(&state, &trusted_proxy_headers("monitor"))
                .expect("strict proxy principal");

        assert_eq!(
            principal.issuer,
            super::super::rbac::WebPrincipalIssuer::TrustedProxy
        );
        assert_eq!(principal.subject, "user:alice");
        assert_eq!(principal.role, styrene_rbac::Role::Monitor);
    }

    #[test]
    fn surface_stream_strict_proxy_state_rejects_local_bearer_only() {
        let state = strict_proxy_state();

        assert!(matches!(
            super::super::rbac::principal_from_headers(&state, &auth_headers()),
            Err(super::super::rbac::RbacError::ProxyIdentityRequired)
        ));
    }

    #[test]
    fn surface_stream_principal_headers_reject_invalid_bearer() {
        let state = test_state();
        let mut headers = trusted_proxy_headers("monitor");
        headers.insert(
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Bearer wrong"),
        );

        assert!(matches!(
            super::super::rbac::principal_from_headers(&state, &headers),
            Err(super::super::rbac::RbacError::Unauthorized)
        ));
    }

    #[test]
    fn surface_stream_query_token_path_uses_local_role() {
        let mut state = test_state();
        state.web_role = styrene_rbac::Role::Blocked;
        let principal = super::super::rbac::current_web_principal(&state);

        assert_eq!(
            require_surface_stream_operation(&principal, None),
            Err(axum::http::StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn surface_stream_maps_tool_start() {
        let value = serde_json::to_value(surface_stream_event(
            &test_state(),
            7,
            AgentEvent::ToolStart {
                id: "t1".into(),
                name: "bash".into(),
                provenance: omegon_traits::ToolProvenance::BuiltIn,
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
    fn surface_stream_redacts_tool_args_and_update_tail() {
        let state = secret_test_state();
        let started = serde_json::to_value(surface_stream_event(
            &state,
            1,
            AgentEvent::ToolStart {
                id: "t-secret".into(),
                name: "bash".into(),
                provenance: omegon_traits::ToolProvenance::BuiltIn,
                args: serde_json::json!({"command":"echo super-secret-token"}),
            },
        ))
        .expect("serialize envelope");
        let updated = serde_json::to_value(surface_stream_event(
            &state,
            2,
            AgentEvent::ToolUpdate {
                id: "t-secret".into(),
                partial: omegon_traits::PartialToolResult {
                    tail: "tail super-secret-token".into(),
                    progress: omegon_traits::ToolProgress::default(),
                    details: serde_json::Value::Null,
                },
            },
        ))
        .expect("serialize envelope");
        let serialized = serde_json::to_string(&(started, updated)).unwrap();
        assert!(!serialized.contains("super-secret-token"));
        assert!(serialized.contains("[REDACTED"));
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
