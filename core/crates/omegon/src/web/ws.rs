//! WebSocket handler — bidirectional agent protocol.
//!
//! This is the **full agent interface**. Any web UI can connect to
//! ws://localhost:PORT/ws?token=TOKEN or wss://HOST/ws?token=TOKEN and drive the agent.
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
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};

use super::api::build_snapshot;
use super::{WebCommand, WebState};
use omegon_traits::AgentEvent;

#[derive(Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

/// Upgrade handler — accepts the WebSocket connection after auth check.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(query): Query<WsQuery>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    if state.web_auth.verify_query_token(query.token.as_deref()) {
        if let Err(error) = super::rbac::validate_proxy_identity_headers(&state, &headers) {
            return error.status().into_response();
        }
        tracing::debug!(
            auth_mode = state.web_auth.mode_name(),
            "WebSocket auth OK, upgrading"
        );
    } else if query.token.is_some() {
        tracing::warn!(
            auth_mode = state.web_auth.mode_name(),
            "WebSocket auth failed — token mismatch"
        );
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    } else {
        tracing::warn!(
            auth_mode = state.web_auth.mode_name(),
            "WebSocket auth failed — no token provided"
        );
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(|socket| handle_socket(socket, state))
        .into_response()
}

/// Handle a single WebSocket connection.
async fn handle_socket(socket: WebSocket, state: WebState) {
    tracing::info!("WebSocket client connected");
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Send initial state snapshot
    let snapshot = build_snapshot(&state);
    let init_msg = snapshot_message(snapshot);
    let snapshot_json = init_msg.to_string();
    tracing::debug!(bytes = snapshot_json.len(), "sending initial snapshot");
    if ws_tx
        .send(Message::Text(snapshot_json.into()))
        .await
        .is_err()
    {
        tracing::warn!("WebSocket: initial snapshot send failed — client disconnected");
        return;
    }
    tracing::debug!("initial snapshot sent OK");

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
                            for msg in serialize_ws_messages(&event) {
                                if ws_tx.send(Message::Text(msg.to_string().into())).await.is_err() {
                                    return;
                                }
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
                        handle_client_command(&cmd, &command_tx, &state_for_cmds, &snapshot_tx)
                            .await;
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

    tracing::info!("WebSocket client disconnected");
}

fn websocket_caller_role(cmd: &Value, state: &WebState) -> crate::control_actions::ControlRole {
    if let Some(label) = cmd["caller_role"].as_str() {
        match label {
            "read" => crate::control_actions::ControlRole::Read,
            "edit" => crate::control_actions::ControlRole::Edit,
            "admin" => crate::control_actions::ControlRole::Admin,
            _ => crate::control_actions::ControlRole::Read,
        }
    } else {
        match super::rbac::current_web_role(state) {
            styrene_rbac::Role::Monitor => crate::control_actions::ControlRole::Read,
            styrene_rbac::Role::Operator => crate::control_actions::ControlRole::Edit,
            styrene_rbac::Role::Admin => crate::control_actions::ControlRole::Admin,
            _ => crate::control_actions::ControlRole::Read,
        }
    }
}

/// Process a command from a WebSocket client.
async fn handle_client_command(
    cmd: &Value,
    command_tx: &tokio::sync::mpsc::Sender<WebCommand>,
    state: &WebState,
    snapshot_tx: &tokio::sync::mpsc::Sender<Value>,
) {
    let cmd_type = cmd["type"].as_str().unwrap_or("");
    let caller_role = websocket_caller_role(cmd, state);

    match cmd_type {
        "delegate_dispatch" | "delegate_get" | "delegate_result" | "delegate_cancel" => {
            let classified = crate::control_actions::classify_web_method(cmd_type);
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx.send(json!({"type": format!("{cmd_type}_result"), "accepted": false, "error": "insufficient_role"})).await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let payload = cmd.clone();
            let sent = command_tx.send(WebCommand::ManagedDelegateControl {
                method: cmd_type.to_string(), payload, respond_to: reply_tx,
            }).await.is_ok();
            let reply = if sent { reply_rx.await.unwrap_or_else(|_| json!({"type": format!("{cmd_type}_result"), "accepted": false, "error": "runtime_disconnected"})) } else { json!({"type": format!("{cmd_type}_result"), "accepted": false, "error": "runtime_unavailable"}) };
            let _ = snapshot_tx.send(reply).await;
        }
        "user_prompt" => {
            let classified = crate::control_actions::classify_web_method("user_prompt");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for user_prompt",
                    }))
                    .await;
                return;
            }
            if let Some(text) = cmd["text"].as_str() {
                let _ = command_tx
                    .send(WebCommand::UserPrompt {
                        text: text.to_string(),
                        image_paths: Vec::new(),
                    })
                    .await;
            }
        }
        "model_view" => {
            let classified = crate::control_actions::classify_web_method("model_view");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for model_view",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::ModelView,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("model_view", response),
                    Err(_) => control_result_message(
                        "model_view",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "model_view executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "model_view",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue model_view".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "model_list" => {
            let classified = crate::control_actions::classify_web_method("model_list");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for model_list",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::ModelList,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("model_list", response),
                    Err(_) => control_result_message(
                        "model_list",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "model_list executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "model_list",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue model_list".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "skills_view" => {
            let classified = crate::control_actions::classify_web_method("skills_view");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for skills_view",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SkillsView,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("skills_view", response),
                    Err(_) => control_result_message(
                        "skills_view",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "skills_view executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "skills_view",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue skills_view".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "skills_install" => {
            let classified = crate::control_actions::classify_web_method("skills_install");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for skills_install",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SkillsInstall { name: None },
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("skills_install", response),
                    Err(_) => control_result_message(
                        "skills_install",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "skills_install executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "skills_install",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue skills_install".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }

        "prompts_list" | "prompts_get" | "prompts_preview" | "prompts_resolve" => {
            let classified = crate::control_actions::classify_web_method(cmd_type);
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": format!("caller role is insufficient for {cmd_type}"),
                    }))
                    .await;
                return;
            }
            let response = match cmd_type {
                "prompts_list" => serde_json::json!({
                    "type": "control_result",
                    "method": cmd_type,
                    "accepted": true,
                    "prompts": crate::prompts::list_structured().unwrap_or_default(),
                }),
                "prompts_get" => match cmd
                    .get("name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    Some(name) => match crate::prompts::get_prompt(name) {
                        Ok((manifest, body, path)) => serde_json::json!({
                            "type": "control_result",
                            "method": cmd_type,
                            "accepted": true,
                            "name": name,
                            "id": manifest.id,
                            "title": manifest.title,
                            "description": manifest.description,
                            "tags": manifest.tags,
                            "aliases": manifest.aliases,
                            "safety": crate::prompts::safety_verdict(&body),
                            "body": body,
                            "path": path.display().to_string(),
                        }),
                        Err(err) => serde_json::json!({
                            "type": "control_result",
                            "method": cmd_type,
                            "accepted": false,
                            "output": err.to_string(),
                        }),
                    },
                    None => serde_json::json!({
                        "type": "control_result",
                        "method": cmd_type,
                        "accepted": false,
                        "output": "missing name",
                    }),
                },
                _ => match cmd
                    .get("name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    Some(name) => match crate::prompts::get_prompt(name) {
                        Ok((_manifest, body, path)) => serde_json::json!({
                            "type": "control_result",
                            "method": cmd_type,
                            "accepted": true,
                            "action": "preview",
                            "safety": crate::prompts::safety_verdict(&body),
                            "prompt": body,
                            "path": path.display().to_string(),
                        }),
                        Err(err) => serde_json::json!({
                            "type": "control_result",
                            "method": cmd_type,
                            "accepted": false,
                            "output": err.to_string(),
                        }),
                    },
                    None => serde_json::json!({
                        "type": "control_result",
                        "method": cmd_type,
                        "accepted": false,
                        "output": "missing name",
                    }),
                },
            };
            let _ = snapshot_tx.send(response).await;
        }
        "plugin_view" => {
            let classified = crate::control_actions::classify_web_method("plugin_view");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for plugin_view",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::PluginView,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("plugin_view", response),
                    Err(_) => control_result_message(
                        "plugin_view",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "plugin_view executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "plugin_view",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue plugin_view".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "set_model" => {
            if let Some(model) = cmd["model"].as_str() {
                let current_model: String = state
                    .handles
                    .harness
                    .as_ref()
                    .and_then(|lock| lock.lock().ok())
                    .and_then(|h| {
                        h.providers
                            .iter()
                            .find(|p| p.authenticated)
                            .and_then(|p| p.model.clone())
                    })
                    .unwrap_or_default();
                let classified = if current_model.is_empty() {
                    crate::control_actions::classify_web_method("set_model")
                } else {
                    crate::control_actions::classify_web_set_model_request(&current_model, model)
                };
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for set_model",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SetModel {
                            requested_model: model.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("set_model", response),
                        Err(_) => control_result_message(
                            "set_model",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "set_model executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "set_model",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue set_model".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "switch_dispatcher" => {
            let request_id = cmd["request_id"].as_str().unwrap_or("").trim();
            let profile = cmd["profile"].as_str().unwrap_or("").trim();
            let model = cmd["model"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let classified = crate::control_actions::classify_web_method("switch_dispatcher");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for switch_dispatcher",
                    }))
                    .await;
                return;
            }
            if request_id.is_empty() || profile.is_empty() {
                let _ = snapshot_tx
                    .send(control_result_message(
                        "switch_dispatcher",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("request_id and profile are required".to_string()),
                        },
                    ))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SwitchDispatcher {
                        request_id: request_id.to_string(),
                        profile: profile.to_string(),
                        model: model.map(|s| s.to_string()),
                    },
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("switch_dispatcher", response),
                    Err(_) => control_result_message(
                        "switch_dispatcher",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "switch_dispatcher executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "switch_dispatcher",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue switch_dispatcher".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "set_thinking" => {
            if let Some(level_raw) = cmd["level"].as_str()
                && let Some(level) = crate::settings::ThinkingLevel::parse(level_raw)
            {
                let classified = crate::control_actions::classify_web_method("set_thinking");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for set_thinking",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SetThinking { level },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("set_thinking", response),
                        Err(_) => control_result_message(
                            "set_thinking",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "set_thinking executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "set_thinking",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue set_thinking".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "plugin_install" => {
            if let Some(uri) = cmd["uri"].as_str() {
                let classified = crate::control_actions::classify_web_method("plugin_install");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for plugin_install",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::PluginInstall {
                            uri: uri.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("plugin_install", response),
                        Err(_) => control_result_message(
                            "plugin_install",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "plugin_install executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "plugin_install",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue plugin_install".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "plugin_remove" => {
            if let Some(name) = cmd["name"].as_str() {
                let classified = crate::control_actions::classify_web_method("plugin_remove");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for plugin_remove",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::PluginRemove {
                            name: name.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("plugin_remove", response),
                        Err(_) => control_result_message(
                            "plugin_remove",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "plugin_remove executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "plugin_remove",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue plugin_remove".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "plugin_update" => {
            let classified = crate::control_actions::classify_web_method("plugin_update");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for plugin_update",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::PluginUpdate {
                        name: cmd["name"]
                            .as_str()
                            .map(|s| s.to_string())
                            .filter(|s| !s.is_empty()),
                    },
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("plugin_update", response),
                    Err(_) => control_result_message(
                        "plugin_update",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "plugin_update executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "plugin_update",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue plugin_update".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "secrets_view" => {
            let classified = crate::control_actions::classify_web_method("secrets_view");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for secrets_view",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SecretsView,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("secrets_view", response),
                    Err(_) => control_result_message(
                        "secrets_view",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "secrets_view executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "secrets_view",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue secrets_view".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "secrets_set" => {
            if let (Some(name), Some(value)) = (cmd["name"].as_str(), cmd["value"].as_str()) {
                let classified = crate::control_actions::classify_web_method("secrets_set");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for secrets_set",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SecretsSet {
                            name: name.to_string(),
                            value: value.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("secrets_set", response),
                        Err(_) => control_result_message(
                            "secrets_set",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "secrets_set executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "secrets_set",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue secrets_set".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "secrets_get" => {
            if let Some(name) = cmd["name"].as_str() {
                let classified = crate::control_actions::classify_web_method("secrets_get");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for secrets_get",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SecretsGet {
                            name: name.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("secrets_get", response),
                        Err(_) => control_result_message(
                            "secrets_get",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "secrets_get executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "secrets_get",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue secrets_get".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "secrets_delete" => {
            if let Some(name) = cmd["name"].as_str() {
                let classified = crate::control_actions::classify_web_method("secrets_delete");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for secrets_delete",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SecretsDelete {
                            name: name.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("secrets_delete", response),
                        Err(_) => control_result_message(
                            "secrets_delete",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "secrets_delete executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "secrets_delete",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue secrets_delete".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "vault_status" => {
            let classified = crate::control_actions::classify_web_method("vault_status");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for vault_status",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::VaultStatus,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("vault_status", response),
                    Err(_) => control_result_message(
                        "vault_status",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "vault_status executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "vault_status",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue vault_status".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "vault_unseal" => {
            let classified = crate::control_actions::classify_web_method("vault_unseal");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for vault_unseal",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::VaultUnseal,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("vault_unseal", response),
                    Err(_) => control_result_message(
                        "vault_unseal",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "vault_unseal executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "vault_unseal",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue vault_unseal".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "vault_login" => {
            let classified = crate::control_actions::classify_web_method("vault_login");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for vault_login",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::VaultLogin,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("vault_login", response),
                    Err(_) => control_result_message(
                        "vault_login",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "vault_login executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "vault_login",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue vault_login".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "vault_configure" => {
            let classified = crate::control_actions::classify_web_method("vault_configure");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for vault_configure",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::VaultConfigure,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("vault_configure", response),
                    Err(_) => control_result_message(
                        "vault_configure",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "vault_configure executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "vault_configure",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue vault_configure".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "vault_init_policy" => {
            let classified = crate::control_actions::classify_web_method("vault_init_policy");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for vault_init_policy",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::VaultInitPolicy,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("vault_init_policy", response),
                    Err(_) => control_result_message(
                        "vault_init_policy",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "vault_init_policy executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "vault_init_policy",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue vault_init_policy".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "cleave_status" => {
            let classified = crate::control_actions::classify_web_method("cleave_status");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for cleave_status",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::CleaveStatus,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("cleave_status", response),
                    Err(_) => control_result_message(
                        "cleave_status",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "cleave_status executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "cleave_status",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue cleave_status".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "delegate_status" => {
            let classified = crate::control_actions::classify_web_method("delegate_status");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for delegate_status",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::DelegateStatus,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("delegate_status", response),
                    Err(_) => control_result_message(
                        "delegate_status",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "delegate_status executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "delegate_status",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue delegate_status".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "auth_status" => {
            let classified = crate::control_actions::classify_web_method("auth_status");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for auth_status",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::AuthStatus,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("auth_status", response),
                    Err(_) => control_result_message(
                        "auth_status",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "auth_status executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "auth_status",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue auth_status".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "auth_login" => {
            let classified = crate::control_actions::classify_web_method("auth_login");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for auth_login",
                    }))
                    .await;
                return;
            }
            let provider = cmd["provider"].as_str().unwrap_or("").to_string();
            if provider.is_empty() {
                let _ = snapshot_tx
                    .send(control_result_message(
                        "auth_login",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("missing required field: provider".to_string()),
                        },
                    ))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::AuthLogin { provider },
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("auth_login", response),
                    Err(_) => control_result_message(
                        "auth_login",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "auth_login executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "auth_login",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue auth_login".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "auth_logout" => {
            let classified = crate::control_actions::classify_web_method("auth_logout");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for auth_logout",
                    }))
                    .await;
                return;
            }
            let provider = cmd["provider"].as_str().unwrap_or("").to_string();
            if provider.is_empty() {
                let _ = snapshot_tx
                    .send(control_result_message(
                        "auth_logout",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("missing required field: provider".to_string()),
                        },
                    ))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::AuthLogout { provider },
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("auth_logout", response),
                    Err(_) => control_result_message(
                        "auth_logout",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "auth_logout executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "auth_logout",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue auth_logout".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "set_context_class"
        | "set_runtime_mode"
        | "set_max_turns"
        | "profile_view"
        | "profile_export"
        | "profile_capture"
        | "profile_apply"
        | "profile_mqtt"
        | "profile_extension_allow"
        | "profile_extension_deny"
        | "profile_extension_clear"
        | "profile_persona"
        | "profile_tone"
        | "persona_list"
        | "persona_switch" => {
            let classified = crate::control_actions::classify_web_method(cmd_type);
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": format!("caller role is insufficient for {cmd_type}"),
                    }))
                    .await;
                return;
            }
            let request = match cmd_type {
                "set_context_class" => {
                    let Some(class_str) = cmd["class"].as_str() else {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some("missing required field: class".to_string()),
                                },
                            ))
                            .await;
                        return;
                    };
                    let Some(class) = crate::settings::ContextClass::parse(class_str) else {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some(format!(
                                        "invalid context class: {class_str}. Use: compact, standard, extended, massive"
                                    )),
                                },
                            ))
                            .await;
                        return;
                    };
                    crate::control_runtime::ControlRequest::SetContextClass { class }
                }
                "set_presentation_level" => {
                    let Some(level) = cmd["level"].as_str().and_then(|level| {
                        crate::surfaces::layout::UiPresentationLevel::parse(level).ok()
                    }) else {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some(
                                        "invalid or missing presentation level: use om, active, or full"
                                            .to_string(),
                                    ),
                                },
                            ))
                            .await;
                        return;
                    };
                    crate::control_runtime::ControlRequest::SetPresentationLevel { level }
                }
                "set_runtime_mode" => {
                    let slim = cmd["slim"].as_bool().unwrap_or(false);
                    crate::control_runtime::ControlRequest::SetRuntimeMode { slim }
                }
                "set_max_turns" => {
                    let Some(raw) = cmd["max_turns"].as_u64() else {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some("missing required field: max_turns".to_string()),
                                },
                            ))
                            .await;
                        return;
                    };
                    let Ok(max_turns) = u32::try_from(raw) else {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some(format!("max_turns out of range: {raw}")),
                                },
                            ))
                            .await;
                        return;
                    };
                    crate::control_runtime::ControlRequest::SetMaxTurns { max_turns }
                }
                "profile_view" => crate::control_runtime::ControlRequest::ProfileView,
                "profile_export" => crate::control_runtime::ControlRequest::ProfileExport,
                "profile_capture" => {
                    let target = match cmd["target"].as_str() {
                        Some("project") => crate::settings::ProfileSaveTarget::Project,
                        Some("user") | Some("global") => crate::settings::ProfileSaveTarget::User,
                        Some("named") => {
                            let name = cmd["name"].as_str().unwrap_or("unnamed").to_string();
                            let scope = match cmd["scope"].as_str() {
                                Some("project") => crate::settings::ProfileRegistryScope::Project,
                                _ => crate::settings::ProfileRegistryScope::User,
                            };
                            crate::settings::ProfileSaveTarget::Named { name, scope }
                        }
                        _ => crate::settings::ProfileSaveTarget::ActiveSource,
                    };
                    crate::control_runtime::ControlRequest::ProfileCapture { target }
                }
                "profile_apply" => crate::control_runtime::ControlRequest::ProfileApply,
                "profile_mqtt" => crate::control_runtime::ControlRequest::ProfileSetMqtt {
                    enabled: cmd["enabled"].as_bool(),
                },
                "profile_extension_allow" => {
                    let name = cmd["name"].as_str().unwrap_or("").to_string();
                    if name.is_empty() {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some("missing required field: name".to_string()),
                                },
                            ))
                            .await;
                        return;
                    }
                    crate::control_runtime::ControlRequest::ProfileExtensionAllow { name }
                }
                "profile_extension_deny" => {
                    let name = cmd["name"].as_str().unwrap_or("").to_string();
                    if name.is_empty() {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some("missing required field: name".to_string()),
                                },
                            ))
                            .await;
                        return;
                    }
                    crate::control_runtime::ControlRequest::ProfileExtensionDeny { name }
                }
                "profile_extension_clear" => {
                    crate::control_runtime::ControlRequest::ProfileExtensionClear
                }
                "profile_persona" => crate::control_runtime::ControlRequest::ProfileSetPersona {
                    name: cmd["name"]
                        .as_str()
                        .map(str::to_string)
                        .filter(|s| !s.is_empty()),
                },
                "profile_tone" => crate::control_runtime::ControlRequest::ProfileSetTone {
                    name: cmd["name"]
                        .as_str()
                        .map(str::to_string)
                        .filter(|s| !s.is_empty()),
                },
                "persona_list" => crate::control_runtime::ControlRequest::PersonaList,
                "persona_switch" => {
                    let name = cmd["name"].as_str().unwrap_or("").to_string();
                    if name.is_empty() {
                        let _ = snapshot_tx
                            .send(control_result_message(
                                cmd_type,
                                omegon_traits::ControlOutputResponse {
                                    accepted: false,
                                    output: Some("missing required field: name".to_string()),
                                },
                            ))
                            .await;
                        return;
                    }
                    crate::control_runtime::ControlRequest::PersonaSwitch { name }
                }
                _ => unreachable!(),
            };
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message(cmd_type, response),
                    Err(_) => control_result_message(
                        cmd_type,
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(format!(
                                "{cmd_type} executor dropped response before completion"
                            )),
                        },
                    ),
                }
            } else {
                control_result_message(
                    cmd_type,
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some(format!("failed to enqueue {cmd_type}")),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "context_status" => {
            let classified = crate::control_actions::classify_web_method("context_status");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for context_status",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::ContextStatus,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("context_status", response),
                    Err(_) => control_result_message(
                        "context_status",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "context_status executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "context_status",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue context_status".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "context_compact" => {
            let classified = crate::control_actions::classify_web_method("context_compact");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for context_compact",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::ContextCompact,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("context_compact", response),
                    Err(_) => control_result_message(
                        "context_compact",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "context_compact executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "context_compact",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue context_compact".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "context_clear" => {
            let classified = crate::control_actions::classify_web_method("context_clear");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for context_clear",
                    }))
                    .await;
                return;
            }
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::ContextClear,
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => control_result_message("context_clear", response),
                    Err(_) => control_result_message(
                        "context_clear",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some(
                                "context_clear executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                control_result_message(
                    "context_clear",
                    omegon_traits::ControlOutputResponse {
                        accepted: false,
                        output: Some("failed to enqueue context_clear".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        // Compatibility adapter. If the slash command already canonicalizes to a
        // `ControlRequest`, route it through `ExecuteControl`; otherwise fall back
        // to the slash-specific transport path for residual UX/local commands.
        "slash_command" => {
            let name = cmd["name"].as_str().unwrap_or("").to_string();
            let args = cmd["args"].as_str().unwrap_or("").to_string();
            let caller_role = websocket_caller_role(cmd, state);
            let classified = crate::control_actions::classify_remote_slash_command(&name, &args);
            if !classified.remote_safe {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": format!(
                            "/{} {} is local-only and cannot be executed remotely",
                            name,
                            args
                        ).trim().to_string(),
                    }))
                    .await;
                return;
            }
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": format!(
                            "caller role is insufficient for /{} {}",
                            name,
                            args
                        ).trim().to_string(),
                    }))
                    .await;
                return;
            }

            if let Some(command) = crate::tui::canonical_slash_command(&name, &args)
                && let Some(request) = crate::control_runtime::control_request_from_slash(&command)
            {
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request,
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("slash_command", response),
                        Err(_) => control_result_message(
                            "slash_command",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "slash command control executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "slash_command",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue slash command control".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
                return;
            }

            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let accepted = command_tx
                .send(WebCommand::SlashCommand {
                    name: name.clone(),
                    args: args.clone(),
                    respond_to: Some(reply_tx),
                })
                .await
                .is_ok();
            let message = if accepted {
                match reply_rx.await {
                    Ok(response) => slash_command_result_message(&name, &args, response),
                    Err(_) => slash_command_result_message(
                        &name,
                        &args,
                        omegon_traits::SlashCommandResponse {
                            accepted: false,
                            output: Some(
                                "slash command executor dropped response before completion"
                                    .to_string(),
                            ),
                        },
                    ),
                }
            } else {
                slash_command_result_message(
                    &name,
                    &args,
                    omegon_traits::SlashCommandResponse {
                        accepted: false,
                        output: Some("failed to enqueue slash command".to_string()),
                    },
                )
            };
            let _ = snapshot_tx.send(message).await;
        }
        "cancel" => {
            let classified = crate::control_actions::classify_web_method("cancel");
            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                let _ = snapshot_tx
                    .send(serde_json::json!({
                        "type": "system_message",
                        "role": "system",
                        "message": "caller role is insufficient for cancel",
                    }))
                    .await;
                return;
            }
            let _ = command_tx.send(WebCommand::Cancel).await;
        }
        "cancel_cleave_child" => {
            if let Some(label) = cmd["label"].as_str() {
                let classified = crate::control_actions::classify_web_method("cleave_cancel_child");
                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                    let _ = snapshot_tx
                        .send(serde_json::json!({
                            "type": "system_message",
                            "role": "system",
                            "message": "caller role is insufficient for cancel_cleave_child",
                        }))
                        .await;
                    return;
                }
                let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                let accepted = command_tx
                    .send(WebCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::CleaveCancelChild {
                            label: label.to_string(),
                        },
                        respond_to: Some(reply_tx),
                    })
                    .await
                    .is_ok();
                let message = if accepted {
                    match reply_rx.await {
                        Ok(response) => control_result_message("cancel_cleave_child", response),
                        Err(_) => control_result_message(
                            "cancel_cleave_child",
                            omegon_traits::ControlOutputResponse {
                                accepted: false,
                                output: Some(
                                    "cleave child cancel executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        ),
                    }
                } else {
                    control_result_message(
                        "cancel_cleave_child",
                        omegon_traits::ControlOutputResponse {
                            accepted: false,
                            output: Some("failed to enqueue cleave child cancel".to_string()),
                        },
                    )
                };
                let _ = snapshot_tx.send(message).await;
            }
        }
        "request_snapshot" => {
            let snapshot = build_snapshot(state);
            let _ = snapshot_tx.send(snapshot_message(snapshot)).await;
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

fn snapshot_message(snapshot: impl serde::Serialize) -> Value {
    json!({
        "type": "state_snapshot",
        "event_name": "state.snapshot",
        "name": "state.snapshot",
        "data": snapshot,
    })
}

fn slash_command_result_message(
    name: &str,
    args: &str,
    response: omegon_traits::SlashCommandResponse,
) -> Value {
    let output = response.output.unwrap_or_default();
    json!({
        "type": "slash_command_result",
        "event_name": "slash.command.result",
        "name": name,
        "args": args,
        "accepted": response.accepted,
        "output": escape_html(&output),
    })
}

fn control_result_message(name: &str, response: omegon_traits::ControlOutputResponse) -> Value {
    let output = response.output.unwrap_or_default();
    json!({
        "type": "control_result",
        "event_name": "control.result",
        "name": name,
        "accepted": response.accepted,
        "output": escape_html(&output),
    })
}

fn state_changed_message(sections: &[&str]) -> Value {
    json!({
        "type": "state_changed",
        "event_name": "state.changed",
        "name": "state.changed",
        "sections": sections,
    })
}

fn refresh_sections(event: &AgentEvent) -> Option<&'static [&'static str]> {
    match event {
        AgentEvent::TurnEnd(_) => Some(&["session", "design", "openspec", "cleave"]),
        AgentEvent::HarnessStatusChanged { .. } => Some(&["harness"]),
        AgentEvent::SessionReset => Some(&["session", "design", "openspec", "cleave", "harness"]),
        _ => None,
    }
}

fn serialize_ws_messages(event: &AgentEvent) -> Vec<Value> {
    if matches!(event, AgentEvent::WebDashboardStarted { .. }) {
        return Vec::new();
    }

    let mut messages = vec![serialize_agent_event(event)];
    if let Some(sections) = refresh_sections(event) {
        messages.push(state_changed_message(sections));
    }
    messages
}

/// Serialize an AgentEvent to a JSON event message.
/// Text fields that may contain user-controlled content are HTML-escaped.
fn serialize_agent_event(event: &AgentEvent) -> Value {
    match event {
        AgentEvent::TurnStart { turn } => json!({
            "type": "turn_start",
            "event_name": "turn.started",
            "turn": turn,
        }),
        AgentEvent::TurnEnd(te) => json!({
            "type": "turn_end",
            "event_name": "turn.ended",
            "turn": te.turn,
            "estimated_tokens": te.estimated_tokens,
            "model": te.model,
            "provider": te.provider,
            "turn_end_reason": te.turn_end_reason,
            "actual_input_tokens": te.actual_input_tokens,
            "actual_output_tokens": te.actual_output_tokens,
            "cache_read_tokens": te.cache_read_tokens,
            "provider_telemetry": te.provider_telemetry,
            "dominant_phase": te.dominant_phase,
            "drift_kind": te.drift_kind,
            "progress_nudge_reason": te.progress_nudge_reason,
            "streaks": {
                "orientation_churn": te.streaks.orientation_churn,
                "repeated_action_failure": te.streaks.repeated_action_failure,
                "validation_thrash": te.streaks.validation_thrash,
                "closure_stall": te.streaks.closure_stall,
                "constraint_discovery": te.streaks.constraint_discovery,
                "evidence_sufficient": te.streaks.evidence_sufficient,
            },
        }),
        AgentEvent::MessageStart { role } => json!({
            "type": "message_start",
            "role": role,
        }),
        AgentEvent::MessageChunk { text } => json!({
            "type": "message_chunk",
            "event_name": "message.delta",
            "text": escape_html(text),
        }),
        AgentEvent::ThinkingChunk { text } => json!({
            "type": "thinking_chunk",
            "event_name": "thinking.delta",
            "text": escape_html(text),
        }),
        AgentEvent::MessageEnd => json!({
            "type": "message_end",
            "event_name": "message.completed",
        }),
        AgentEvent::MessageAbort { reason } => json!({
            "type": "message_abort",
            "reason": reason,
        }),
        AgentEvent::ToolStart {
            id,
            name,
            args,
            provenance,
        } => json!({
            "type": "tool_start",
            "event_name": "tool.started",
            "id": id,
            "name": name,
            "tool_name": name,
            "provenance": provenance,
            "args": args,
        }),
        AgentEvent::ToolUpdate { id, partial } => {
            // Surface the typed shape so the dashboard can render live
            // tail + progress instead of guessing from raw text. Heartbeats
            // arrive with `tail` empty and `heartbeat: true` — consumers can
            // use them to refresh a "last seen alive" timestamp without
            // re-rendering content.
            let mut payload = json!({
                "type": "tool_update",
                "event_name": "tool.updated",
                "id": id,
                "partial": escape_html(&partial.tail),
                "heartbeat": partial.progress.heartbeat,
                "elapsed_ms": partial.progress.elapsed_ms,
            });
            if let Some(phase) = &partial.progress.phase {
                payload["phase"] = json!(phase);
            }
            if let Some(units) = &partial.progress.units {
                payload["units"] = json!({
                    "current": units.current,
                    "total": units.total,
                    "unit": units.unit,
                });
            }
            if let Some(tally) = &partial.progress.tally {
                payload["tally"] = json!({
                    "ok": tally.ok,
                    "fail": tally.fail,
                    "skip": tally.skip,
                    "other": tally.other,
                });
            }
            payload
        }
        AgentEvent::ToolEnd {
            id,
            name,
            result,
            is_error,
            provenance,
        } => {
            // Serialize ALL content blocks, not just the first
            let texts: Vec<&str> = result.content.iter().filter_map(|c| c.as_text()).collect();
            let result_text = texts.join("\n");
            json!({
                "type": "tool_end",
                "event_name": "tool.ended",
                "id": id,
                "name": name,
                "tool_name": name,
                "provenance": provenance,
                "result": escape_html(&result_text),
                "is_error": is_error,
                "block_count": result.content.len(),
            })
        }
        AgentEvent::PermissionRequest { .. } | AgentEvent::OperatorWaitRequest { .. } => {
            json!(null)
        } // handled by TUI, not WS
        AgentEvent::AgentEnd => json!({
            "type": "agent_end",
            "event_name": "agent.completed",
        }),
        AgentEvent::PhaseChanged { phase } => json!({
            "type": "phase_changed",
            "event_name": "phase.changed",
            "phase": format!("{phase:?}"),
        }),
        AgentEvent::DecompositionStarted {
            children,
            operation,
        } => json!({
            "type": "decomposition_started",
            "event_name": "decomposition.started",
            "children": children,
            "operation_kind": serde_json::to_value(operation.kind).unwrap_or(serde_json::Value::Null),
            "operation_id": operation.id,
        }),
        AgentEvent::DecompositionChildCompleted {
            label,
            success,
            operation,
        } => json!({
            "type": "decomposition_child_completed",
            "event_name": "decomposition.child_completed",
            "label": escape_html(label),
            "success": success,
            "operation_kind": serde_json::to_value(operation.kind).unwrap_or(serde_json::Value::Null),
            "operation_id": operation.id,
        }),
        AgentEvent::DecompositionCompleted { merged, operation } => json!({
            "type": "decomposition_completed",
            "event_name": "decomposition.completed",
            "merged": merged,
            "operation_kind": serde_json::to_value(operation.kind).unwrap_or(serde_json::Value::Null),
            "operation_id": operation.id,
        }),
        AgentEvent::FamilyVitalSignsUpdated { signs } => json!({
            "type": "family_vital_signs",
            "event_name": "family.vital_signs",
            "signs": {
                "run_id": signs.run_id,
                "active": signs.active,
                "total_children": signs.total_children,
                "completed": signs.completed,
                "failed": signs.failed,
                "running": signs.running,
                "pending": signs.pending,
                "total_tokens_in": signs.total_tokens_in,
                "total_tokens_out": signs.total_tokens_out,
                "children": signs.children.iter().map(|c| json!({
                    "label": c.label,
                    "status": c.status,
                    "started_at_unix_ms": c.started_at_unix_ms,
                    "last_activity_unix_ms": c.last_activity_unix_ms,
                    "duration_secs": c.duration_secs,
                    "last_tool": c.last_tool,
                    "last_tool_activity": c.last_tool_activity.as_ref().map(|activity| json!({
                        "raw_name": activity.raw_name,
                        "args_summary": activity.args_summary,
                    })),
                    "last_turn": c.last_turn,
                    "tokens_in": c.tokens_in,
                    "tokens_out": c.tokens_out,
                })).collect::<Vec<_>>(),
            },
        }),
        AgentEvent::PlanUpdated { projection } => json!({
            "type": "plan_updated",
            "event_name": "plan.updated",
            "snapshot": projection.legacy_snapshot_json(),
        }),
        AgentEvent::RouteChanged {
            state,
            selected,
            serving,
            warning,
            message,
        } => json!({
            "type": "provider_route_changed",
            "event_name": "provider.route_changed",
            "state": state,
            "selected": selected,
            "serving": serving,
            "warning": warning.as_deref().map(escape_html),
            "message": escape_html(message),
        }),
        AgentEvent::SkillActivation { event } => json!({
            "type": "skill_activation",
            "event_name": "skill.activation",
            "active_ref": event.active_ref,
            "activation": event.activation,
            "reason": event.reason,
            "matched_signals": event.matched_signals,
            "suppressing": event.suppressing,
            "resolution": event.resolution,
            "recommendation": event.recommendation,
            "injected": event.injected,
        }),
        AgentEvent::RuntimeLifecycleUpdated { snapshot } => json!({
            "type": "runtime_lifecycle",
            "event_name": "runtime.lifecycle.updated",
            "snapshot": snapshot,
        }),
        AgentEvent::SystemNotification { message } => json!({
            "type": "system_notification",
            "event_name": "system.notification",
            "message": escape_html(message),
        }),
        AgentEvent::OperatorCopyBlock {
            label,
            text,
            kind,
            copy_attempt,
        } => json!({
            "type": "operator_copy_block",
            "event_name": "operator.copy_block",
            "label": escape_html(label),
            "text": escape_html(text),
            "kind": kind.as_str(),
            "copy_status": copy_attempt.as_ref().map(|status| status.label()),
        }),
        AgentEvent::StreamIdle {
            provider,
            model,
            phase,
            idle_secs,
            ambiguous,
            message,
        } => json!({
            "type": "stream_idle",
            "event_name": "stream.idle",
            "provider": provider,
            "model": model,
            "phase": phase,
            "idle_secs": idle_secs,
            "ambiguous": ambiguous,
            "message": escape_html(message),
        }),
        AgentEvent::ProviderRetry {
            provider,
            model,
            attempt,
            delay_ms,
            reason,
            message,
            recoverable,
        } => json!({
            "type": "provider_retry",
            "event_name": "provider.retry",
            "provider": provider,
            "model": model,
            "attempt": attempt,
            "delay_ms": delay_ms,
            "reason": reason,
            "message": escape_html(message),
            "recoverable": recoverable,
        }),
        AgentEvent::ProviderFailure {
            provider,
            model,
            reason,
            attempts,
            message,
            retryable,
            recommended_action,
        } => json!({
            "type": "provider_failure",
            "event_name": "provider.failure",
            "provider": provider,
            "model": model,
            "reason": reason,
            "attempts": attempts,
            "message": escape_html(message),
            "retryable": retryable,
            "recommended_action": recommended_action,
        }),
        AgentEvent::TurnCancelled { reason } => json!({
            "type": "turn_cancelled",
            "event_name": "turn.cancelled",
            "reason": reason,
        }),
        AgentEvent::HarnessStatusChanged { status_json } => json!({
            "type": "harness_status_changed",
            "event_name": "harness.changed",
            "status": status_json,
        }),
        AgentEvent::RuntimeQueueUpdated { snapshot_json } => json!({
            "type": "runtime_queue_updated",
            "event_name": "runtime.queue_updated",
            "snapshot": snapshot_json,
        }),
        AgentEvent::RuntimeTurnLifecycleUpdated { snapshot_json } => json!({
            "type": "runtime_turn_lifecycle_updated",
            "event_name": "runtime.turn_lifecycle_updated",
            "snapshot": snapshot_json,
        }),
        AgentEvent::RuntimePromptStarted {
            runtime_turn_id,
            text,
            image_paths,
        } => json!({
            "type": "runtime_prompt_started",
            "event_name": "runtime.prompt_started",
            "runtime_turn_id": runtime_turn_id,
            "text": text,
            "image_paths": image_paths,
        }),
        AgentEvent::WebDashboardStarted { .. } => unreachable!("filtered by serialize_ws_messages"),
        AgentEvent::ContextUpdated {
            tokens,
            context_window,
            context_class,
            thinking_level,
        } => json!({
            "type": "context_updated",
            "tokens": tokens,
            "context_window": context_window,
            "context_class": context_class,
            "thinking_level": thinking_level,
        }),
        AgentEvent::ContextCompaction(event) => json!({
            "type": "context_compaction",
            "event_name": "context.compaction",
            "trigger": event.trigger,
            "status": event.status,
            "before_tokens": event.before_tokens,
            "after_tokens": event.after_tokens,
            "evicted_messages": event.evicted_messages,
            "summary_chars": event.summary_chars,
            "reason": event.reason,
        }),
        AgentEvent::SessionReset => json!({
            "type": "session_reset",
            "event_name": "session.reset",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_client_command_rejects_user_prompt_for_monitor_default_role() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let mut state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );
        state.web_role = styrene_rbac::Role::Monitor;

        let cmd = serde_json::json!({
            "type": "user_prompt",
            "text": "should not queue"
        });

        handle_client_command(&cmd, &command_tx, &state, &snapshot_tx).await;

        assert!(command_rx.try_recv().is_err(), "should not enqueue prompt");
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "system_message");
        assert_eq!(
            msg["message"],
            "caller role is insufficient for user_prompt"
        );
    }

    #[tokio::test]
    async fn handle_client_command_rejects_cancel_for_monitor_default_role() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let mut state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );
        state.web_role = styrene_rbac::Role::Monitor;

        let cmd = serde_json::json!({
            "type": "cancel"
        });

        handle_client_command(&cmd, &command_tx, &state, &snapshot_tx).await;

        assert!(command_rx.try_recv().is_err(), "should not enqueue cancel");
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "system_message");
        assert_eq!(msg["message"], "caller role is insufficient for cancel");
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_switch_dispatcher_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "switch_dispatcher",
            "request_id": "req-123",
            "profile": "B",
            "model": "anthropic:claude-sonnet-4-6",
            "caller_role": "admin"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl {
                request,
                respond_to,
            } => {
                match request {
                    crate::control_runtime::ControlRequest::SwitchDispatcher {
                        request_id,
                        profile,
                        model,
                    } => {
                        assert_eq!(request_id, "req-123");
                        assert_eq!(profile, "B");
                        assert_eq!(model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
                    }
                    other => panic!("wrong request: {other:?}"),
                }
                respond_to
                    .expect("respond_to")
                    .send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some("dispatcher switched".into()),
                    })
                    .unwrap();
            }
            other => panic!("wrong command: {other:?}"),
        }

        handler.await.unwrap();
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "control_result");
        assert_eq!(msg["name"], "switch_dispatcher");
        assert_eq!(msg["accepted"], true);
        assert_eq!(msg["output"], "dispatcher switched");
    }

    #[tokio::test]
    async fn handle_client_command_rejects_switch_dispatcher_without_admin_role() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "switch_dispatcher",
            "request_id": "req-123",
            "profile": "B",
            "caller_role": "edit"
        });

        handle_client_command(&cmd, &command_tx, &state, &snapshot_tx).await;
        assert!(command_rx.try_recv().is_err(), "should not enqueue command");
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "system_message");
        assert_eq!(
            msg["message"],
            "caller role is insufficient for switch_dispatcher"
        );
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_cleave_child_cancel_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "cancel_cleave_child",
            "label": "alpha",
            "caller_role": "edit"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl {
                request,
                respond_to,
            } => {
                match request {
                    crate::control_runtime::ControlRequest::CleaveCancelChild { label } => {
                        assert_eq!(label, "alpha");
                    }
                    other => panic!("wrong request: {other:?}"),
                }
                respond_to
                    .expect("respond_to")
                    .send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some("Cancelling cleave child 'alpha'...".into()),
                    })
                    .unwrap();
            }
            other => panic!("wrong command: {other:?}"),
        }

        handler.await.unwrap();
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "control_result");
        assert_eq!(msg["name"], "cancel_cleave_child");
        assert_eq!(msg["accepted"], true);
        assert_eq!(msg["output"], "Cancelling cleave child 'alpha'...");
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_secrets_view_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "secrets_view",
            "caller_role": "edit"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl {
                request,
                respond_to,
            } => {
                assert!(matches!(
                    request,
                    crate::control_runtime::ControlRequest::SecretsView
                ));
                respond_to
                    .expect("respond_to")
                    .send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some("Secrets listed".into()),
                    })
                    .unwrap();
            }
            other => panic!("wrong command: {other:?}"),
        }

        handler.await.unwrap();
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "control_result");
        assert_eq!(msg["name"], "secrets_view");
        assert_eq!(msg["accepted"], true);
        assert_eq!(msg["output"], "Secrets listed");
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_cleave_status_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "cleave_status",
            "caller_role": "read"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl {
                request,
                respond_to,
            } => {
                assert!(matches!(
                    request,
                    crate::control_runtime::ControlRequest::CleaveStatus
                ));
                respond_to
                    .expect("respond_to")
                    .send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some("Cleave idle".into()),
                    })
                    .unwrap();
            }
            other => panic!("wrong command: {other:?}"),
        }

        handler.await.unwrap();
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "control_result");
        assert_eq!(msg["name"], "cleave_status");
        assert_eq!(msg["accepted"], true);
        assert_eq!(msg["output"], "Cleave idle");
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_delegate_status_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "delegate_status",
            "caller_role": "read"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl {
                request,
                respond_to,
            } => {
                assert!(matches!(
                    request,
                    crate::control_runtime::ControlRequest::DelegateStatus
                ));
                respond_to
                    .expect("respond_to")
                    .send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some("No delegates running".into()),
                    })
                    .unwrap();
            }
            other => panic!("wrong command: {other:?}"),
        }

        handler.await.unwrap();
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "control_result");
        assert_eq!(msg["name"], "delegate_status");
        assert_eq!(msg["accepted"], true);
        assert_eq!(msg["output"], "No delegates running");
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_vault_status_for_read_role() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(
            crate::tui::dashboard::DashboardHandles::default(),
            events_tx,
        );

        let cmd = serde_json::json!({
            "type": "vault_status",
            "caller_role": "read"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl {
                request,
                respond_to,
            } => {
                assert!(matches!(
                    request,
                    crate::control_runtime::ControlRequest::VaultStatus
                ));
                respond_to
                    .expect("respond_to")
                    .send(omegon_traits::ControlOutputResponse {
                        accepted: true,
                        output: Some("Vault sealed".into()),
                    })
                    .unwrap();
            }
            other => panic!("wrong command: {other:?}"),
        }

        handler.await.unwrap();
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "control_result");
        assert_eq!(msg["name"], "vault_status");
        assert_eq!(msg["accepted"], true);
        assert_eq!(msg["output"], "Vault sealed");
    }

    #[test]
    fn slash_command_result_message_escapes_html_and_preserves_acceptance() {
        let json = slash_command_result_message(
            "context",
            "status",
            omegon_traits::SlashCommandResponse {
                accepted: true,
                output: Some("Context: <b>42</b>".into()),
            },
        );
        assert_eq!(json["type"], "slash_command_result");
        assert_eq!(json["event_name"], "slash.command.result");
        assert_eq!(json["name"], "context");
        assert_eq!(json["args"], "status");
        assert_eq!(json["accepted"], true);
        assert_eq!(json["output"], "Context: &lt;b&gt;42&lt;/b&gt;");
    }

    #[test]
    fn serialize_plan_updated_uses_typed_projection_legacy_snapshot() {
        let event = AgentEvent::PlanUpdated {
            projection: omegon_traits::PlanSurfaceProjection {
                active: Some(omegon_traits::PlanLaneProjection {
                    plan_id: "session:current".into(),
                    mode: "executing".into(),
                    guidance: "keep going".into(),
                    status: "active".into(),
                    scope: "session".into(),
                    source: "session".into(),
                    progress: omegon_traits::PlanProgressProjection {
                        completed: 1,
                        total: 2,
                    },
                    items: vec![
                        omegon_traits::PlanItemProjection {
                            label: "Read".into(),
                            status: "done".into(),
                            ..Default::default()
                        },
                        omegon_traits::PlanItemProjection {
                            label: "Patch".into(),
                            status: "active".into(),
                            ..Default::default()
                        },
                    ],
                }),
                workstreams: vec![omegon_traits::PlanWorkstreamProjection {
                    id: "openspec:demo".into(),
                    title: "demo".into(),
                    status: "paused".into(),
                    progress: omegon_traits::PlanProgressProjection {
                        completed: 3,
                        total: 5,
                    },
                }],
                ..Default::default()
            },
        };

        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "plan_updated");
        assert_eq!(json["event_name"], "plan.updated");
        assert_eq!(json["snapshot"]["mode"], "executing");
        assert_eq!(json["snapshot"]["completed"], 1);
        assert_eq!(json["snapshot"]["total"], 2);
        assert_eq!(json["snapshot"]["items"][0]["description"], "Read");
        assert_eq!(json["snapshot"]["items"][1]["status"], "active");
        assert_eq!(json["snapshot"]["workstreams"][0]["id"], "openspec:demo");
        assert_eq!(json["snapshot"]["workstreams"][0]["completed"], 3);
    }

    #[test]
    fn serialize_runtime_lifecycle_includes_reconnect_contract() {
        let value = serialize_agent_event(&AgentEvent::RuntimeLifecycleUpdated {
            snapshot: omegon_traits::RuntimeLifecycleSnapshot {
                operation_id: "update-1".into(),
                kind: omegon_traits::RuntimeLifecycleKind::UpdateInstall,
                phase: omegon_traits::RuntimeLifecyclePhase::Restarting,
                message: "Restarting".into(),
                session_id: Some("session-1".into()),
                target_version: Some("0.29.0".into()),
                reconnect_required: true,
            },
        });

        assert_eq!(value["event_name"], "runtime.lifecycle.updated");
        assert_eq!(value["snapshot"]["phase"], "restarting");
        assert_eq!(value["snapshot"]["reconnect_required"], true);
        assert_eq!(value["snapshot"]["session_id"], "session-1");
    }

    #[test]
    fn serialize_turn_start() {
        let event = AgentEvent::TurnStart { turn: 5 };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "turn_start");
        assert_eq!(json["event_name"], "turn.started");
        assert_eq!(json["turn"], 5);
    }

    #[test]
    fn serialize_decomposition_started_includes_operation_provenance() {
        let event = AgentEvent::DecompositionStarted {
            children: vec!["delegate_1".into()],
            operation: omegon_traits::OperationRef::delegate("delegate_1"),
        };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "decomposition_started");
        assert_eq!(json["event_name"], "decomposition.started");
        assert_eq!(json["operation_kind"], "delegate");
        assert_eq!(json["operation_id"], "delegate_1");
    }

    #[test]
    fn serialize_message_chunk_escapes_html() {
        let event = AgentEvent::MessageChunk {
            text: "<script>alert(1)</script>".into(),
        };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "message_chunk");
        assert!(!json["text"].as_str().unwrap().contains("<script>"));
        assert!(json["text"].as_str().unwrap().contains("&lt;script&gt;"));
    }

    #[test]
    fn serialize_tool_end_all_blocks() {
        let event = AgentEvent::ToolEnd {
            id: "tc1".into(),
            name: "bash".into(),
            provenance: omegon_traits::ToolProvenance::BuiltIn,
            result: omegon_traits::ToolResult {
                content: vec![
                    omegon_traits::ContentBlock::Text {
                        text: "first".into(),
                    },
                    omegon_traits::ContentBlock::Text {
                        text: "second".into(),
                    },
                ],
                details: serde_json::json!(null),
            },
            is_error: false,
        };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "tool_end");
        assert_eq!(json["event_name"], "tool.ended");
        let result = json["result"].as_str().unwrap();
        assert!(result.contains("first"), "should contain first block");
        assert!(result.contains("second"), "should contain second block");
        assert_eq!(json["block_count"], 2);
    }

    #[test]
    fn serialize_turn_end_includes_usage_and_refresh_hint() {
        let event = AgentEvent::TurnEnd(Box::new(omegon_traits::AgentEventTurnEnd {
            turn: 2,
            turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
            model: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            estimated_tokens: 123,
            context_window: 200_000,
            context_composition: omegon_traits::ContextComposition::default(),
            actual_input_tokens: 45,
            actual_output_tokens: 67,
            cache_read_tokens: 8,
            cache_creation_tokens: 3,
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "anthropic".into(),
                source: "headers".into(),
                ..Default::default()
            }),
            dominant_phase: Some(omegon_traits::OodaPhase::Act),
            drift_kind: Some(omegon_traits::DriftKind::ClosureStall),
            progress_nudge_reason: Some(omegon_traits::ProgressNudgeReason::ClosurePressure),
            intent_task: None,
            intent_phase: None,
            files_read_count: 0,
            files_modified_count: 0,
            stats_tool_calls: 0,
            streaks: omegon_traits::ControllerStreaks {
                closure_stall: 4,
                ..Default::default()
            },
        }));
        let messages = serialize_ws_messages(&event);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["type"], "turn_end");
        assert_eq!(messages[0]["event_name"], "turn.ended");
        assert_eq!(messages[0]["estimated_tokens"], 123);
        assert_eq!(messages[0]["turn_end_reason"], "assistant_completed");
        assert_eq!(messages[0]["actual_input_tokens"], 45);
        assert_eq!(messages[0]["actual_output_tokens"], 67);
        assert_eq!(messages[0]["cache_read_tokens"], 8);
        assert_eq!(messages[0]["provider_telemetry"]["provider"], "anthropic");
        assert_eq!(messages[0]["dominant_phase"], "act");
        assert_eq!(messages[0]["drift_kind"], "closure_stall");
        assert_eq!(messages[0]["progress_nudge_reason"], "closure_pressure");
        // Streak counters: only the closure_stall counter was set in the
        // fixture; the rest should serialize to 0 (not be omitted) so
        // dashboards can rely on the field always being present.
        assert_eq!(messages[0]["streaks"]["closure_stall"], 4);
        assert_eq!(messages[0]["streaks"]["orientation_churn"], 0);
        assert_eq!(messages[0]["streaks"]["repeated_action_failure"], 0);
        assert_eq!(messages[0]["streaks"]["validation_thrash"], 0);
        assert_eq!(messages[0]["streaks"]["constraint_discovery"], 0);
        assert_eq!(messages[0]["streaks"]["evidence_sufficient"], 0);
        assert_eq!(messages[1]["type"], "state_changed");
        assert_eq!(messages[1]["event_name"], "state.changed");
        assert_eq!(
            messages[1]["sections"],
            serde_json::json!(["session", "design", "openspec", "cleave"])
        );
    }

    #[test]
    fn serialize_harness_change_emits_state_refresh() {
        let event = AgentEvent::HarnessStatusChanged {
            status_json: serde_json::json!({"thinking_level": "high"}),
        };
        let messages = serialize_ws_messages(&event);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["type"], "harness_status_changed");
        assert_eq!(messages[0]["event_name"], "harness.changed");
        assert_eq!(messages[1]["sections"], serde_json::json!(["harness"]));
    }

    #[test]
    fn serialize_session_reset_emits_full_refresh() {
        let messages = serialize_ws_messages(&AgentEvent::SessionReset);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["type"], "session_reset");
        assert_eq!(messages[0]["event_name"], "session.reset");
        assert_eq!(
            messages[1]["sections"],
            serde_json::json!(["session", "design", "openspec", "cleave", "harness"])
        );
    }

    #[test]
    fn snapshot_message_uses_canonical_name() {
        let json = snapshot_message(serde_json::json!({"session": {"turns": 1}}));
        assert_eq!(json["type"], "state_snapshot");
        assert_eq!(json["event_name"], "state.snapshot");
        assert_eq!(json["name"], "state.snapshot");
        assert_eq!(json["data"]["session"]["turns"], 1);
    }

    /// Compile-time exhaustivity guard: every `AgentEvent` variant must
    /// appear here as a named arm. There is no `_` wildcard. If anyone
    /// adds a new variant without also adding it to
    /// `serialize_all_event_types` below, this function fails to compile,
    /// which fails the build, which forces the new variant into the test
    /// fixture. The function is never called at runtime.
    #[allow(dead_code)]
    fn _exhaustive_agent_event_serialization_coverage(ev: &AgentEvent) {
        match ev {
            AgentEvent::TurnStart { .. } => {}
            AgentEvent::TurnEnd(_) => {}
            AgentEvent::MessageStart { .. } => {}
            AgentEvent::MessageChunk { .. } => {}
            AgentEvent::ThinkingChunk { .. } => {}
            AgentEvent::MessageEnd => {}
            AgentEvent::MessageAbort { .. } => {}
            AgentEvent::ToolStart { .. } => {}
            AgentEvent::ToolUpdate { .. } => {}
            AgentEvent::ToolEnd { .. } => {}
            AgentEvent::AgentEnd => {}
            AgentEvent::PhaseChanged { .. } => {}
            AgentEvent::DecompositionStarted { .. } => {}
            AgentEvent::DecompositionChildCompleted { .. } => {}
            AgentEvent::DecompositionCompleted { .. } => {}
            AgentEvent::FamilyVitalSignsUpdated { .. } => {}
            AgentEvent::PlanUpdated { .. } => {}
            AgentEvent::RouteChanged { .. } => {}
            AgentEvent::SkillActivation { .. } => {}
            AgentEvent::RuntimeLifecycleUpdated { .. } => {}
            AgentEvent::SystemNotification { .. } => {}
            AgentEvent::OperatorCopyBlock { .. } => {}
            AgentEvent::StreamIdle { .. } => {}
            AgentEvent::ProviderRetry { .. } => {}
            AgentEvent::ProviderFailure { .. } => {}
            AgentEvent::TurnCancelled { .. } => {}
            AgentEvent::HarnessStatusChanged { .. } => {}
            AgentEvent::RuntimeQueueUpdated { .. } => {}
            AgentEvent::RuntimeTurnLifecycleUpdated { .. } => {}
            AgentEvent::RuntimePromptStarted { .. } => {}
            AgentEvent::WebDashboardStarted { .. } => {}
            AgentEvent::ContextUpdated { .. } => {}
            AgentEvent::ContextCompaction(_) => {}
            AgentEvent::SessionReset => {}
            AgentEvent::PermissionRequest { .. } => {}
            AgentEvent::OperatorWaitRequest { .. } => {}
        }
    }

    #[test]
    fn serialize_all_event_types() {
        let events = vec![
            AgentEvent::TurnStart { turn: 1 },
            AgentEvent::TurnEnd(Box::new(omegon_traits::AgentEventTurnEnd {
                turn: 1,
                turn_end_reason: omegon_traits::TurnEndReason::ToolContinuation,
                model: None,
                provider: None,
                estimated_tokens: 0,
                context_window: 200_000,
                context_composition: omegon_traits::ContextComposition::default(),
                actual_input_tokens: 0,
                actual_output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
                dominant_phase: None,
                drift_kind: None,
                progress_nudge_reason: None,
                intent_task: None,
                intent_phase: None,
                files_read_count: 0,
                files_modified_count: 0,
                stats_tool_calls: 0,
                streaks: omegon_traits::ControllerStreaks::default(),
            })),
            AgentEvent::MessageStart {
                role: "assistant".into(),
            },
            AgentEvent::MessageChunk { text: "hi".into() },
            AgentEvent::ThinkingChunk { text: "hmm".into() },
            AgentEvent::MessageEnd,
            AgentEvent::ToolStart {
                id: "1".into(),
                name: "read".into(),
                provenance: omegon_traits::ToolProvenance::BuiltIn,
                args: serde_json::json!({}),
            },
            AgentEvent::ToolUpdate {
                id: "1".into(),
                partial: omegon_traits::PartialToolResult::content("partial", 100),
            },
            AgentEvent::ToolEnd {
                id: "1".into(),
                name: "read".into(),
                provenance: omegon_traits::ToolProvenance::BuiltIn,
                result: omegon_traits::ToolResult {
                    content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
                    details: serde_json::json!(null),
                },
                is_error: false,
            },
            AgentEvent::AgentEnd,
            AgentEvent::PhaseChanged {
                phase: omegon_traits::LifecyclePhase::Idle,
            },
            AgentEvent::DecompositionStarted {
                children: vec!["a".into()],
                operation: omegon_traits::OperationRef::cleave(None),
            },
            AgentEvent::DecompositionChildCompleted {
                label: "a".into(),
                success: true,
                operation: omegon_traits::OperationRef::cleave(None),
            },
            AgentEvent::DecompositionCompleted {
                merged: true,
                operation: omegon_traits::OperationRef::cleave(None),
            },
            AgentEvent::FamilyVitalSignsUpdated {
                signs: omegon_traits::FamilyVitalSigns {
                    run_id: "test-run".into(),
                    active: true,
                    total_children: 2,
                    completed: 1,
                    failed: 0,
                    running: 1,
                    pending: 0,
                    total_tokens_in: 100,
                    total_tokens_out: 50,
                    children: vec![omegon_traits::ChildVitalSigns {
                        label: "alpha".into(),
                        status: "completed".into(),
                        started_at_unix_ms: Some(1_700_000_000_000),
                        last_activity_unix_ms: Some(1_700_000_005_000),
                        duration_secs: Some(5.0),
                        last_tool: Some("bash".into()),
                        last_tool_activity: None,
                        last_turn: Some(3),
                        tokens_in: 60,
                        tokens_out: 30,
                        tasks: Vec::new(),
                        tasks_done: 0,
                    }],
                },
            },
            AgentEvent::PlanUpdated {
                projection: omegon_traits::PlanSurfaceProjection {
                    active: Some(omegon_traits::PlanLaneProjection {
                        mode: "executing".into(),
                        progress: omegon_traits::PlanProgressProjection {
                            completed: 1,
                            total: 2,
                        },
                        items: vec![
                            omegon_traits::PlanItemProjection {
                                label: "Read".into(),
                                status: "done".into(),
                                ..Default::default()
                            },
                            omegon_traits::PlanItemProjection {
                                label: "Patch".into(),
                                status: "active".into(),
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            },
            AgentEvent::RouteChanged {
                state: "fallback".into(),
                selected: Some("openai-codex:gpt-5.5".into()),
                serving: Some("anthropic:claude-fable-5".into()),
                warning: Some("fallback engaged".into()),
                message: "Provider route changed".into(),
            },
            AgentEvent::RuntimeLifecycleUpdated {
                snapshot: omegon_traits::RuntimeLifecycleSnapshot {
                    operation_id: "op-1".into(),
                    kind: omegon_traits::RuntimeLifecycleKind::UpdateInstall,
                    phase: omegon_traits::RuntimeLifecyclePhase::Downloading,
                    message: "Downloading update".into(),
                    session_id: Some("session-1".into()),
                    target_version: Some("0.29.0".into()),
                    reconnect_required: false,
                },
            },
            AgentEvent::SystemNotification {
                message: "test".into(),
            },
            AgentEvent::OperatorCopyBlock {
                label: "Device code".into(),
                text: "432F-FB36".into(),
                kind: omegon_traits::OperatorCopyKind::AuthDeviceCode,
                copy_attempt: Some(omegon_traits::ClipboardCopyStatus::Unavailable),
            },
            // Variants previously missing from this test — added so the
            // exhaustive `_exhaustive_agent_event_serialization_coverage`
            // guard above is the *only* thing the test relies on for
            // coverage. If you add a new AgentEvent variant, you must
            // both extend the guard above AND add an entry here.
            AgentEvent::MessageAbort {
                reason: Some("test abort".into()),
            },
            AgentEvent::HarnessStatusChanged {
                status_json: serde_json::json!({"thinking_level": "low"}),
            },
            AgentEvent::RuntimeQueueUpdated {
                snapshot_json: serde_json::json!({"depth": 1, "items": []}),
            },
            AgentEvent::RuntimeTurnLifecycleUpdated {
                snapshot_json: serde_json::json!({"turn_id": 1, "phase": "worker_spawned"}),
            },
            AgentEvent::RuntimePromptStarted {
                runtime_turn_id: 1,
                text: "queued prompt".into(),
                image_paths: Vec::new(),
            },
            AgentEvent::WebDashboardStarted {
                startup_json: serde_json::json!({"port": 0}),
            },
            AgentEvent::ContextUpdated {
                tokens: 1000,
                context_window: 200_000,
                context_class: "Compact".into(),
                thinking_level: "Low".into(),
            },
            AgentEvent::ContextCompaction(omegon_traits::ContextCompactionEvent {
                trigger: omegon_traits::ContextCompactionTrigger::Manual,
                status: omegon_traits::ContextCompactionStatus::NoPayload,
                before_tokens: 1000,
                after_tokens: Some(1000),
                evicted_messages: Some(0),
                summary_chars: None,
                reason: Some("no payload".into()),
            }),
            AgentEvent::SessionReset,
        ];
        for event in &events {
            // WebDashboardStarted is intentionally filtered by
            // serialize_ws_messages and `unreachable!()` inside
            // serialize_agent_event. Route it through the public
            // interface to verify the filter, and skip the per-event
            // type assertion since it returns an empty Vec.
            if matches!(event, AgentEvent::WebDashboardStarted { .. }) {
                let messages = serialize_ws_messages(event);
                assert!(
                    messages.is_empty(),
                    "WebDashboardStarted should be filtered, got {messages:?}"
                );
                continue;
            }
            let json = serialize_agent_event(event);
            assert!(json["type"].is_string(), "event should have a type field");
        }
        assert_eq!(
            events.len(),
            29,
            "should cover all 29 AgentEvent variants — see _exhaustive_agent_event_serialization_coverage"
        );
    }

    #[test]
    fn serialize_family_vital_signs_renders_full_tree() {
        let event = AgentEvent::FamilyVitalSignsUpdated {
            signs: omegon_traits::FamilyVitalSigns {
                run_id: "run-42".into(),
                active: true,
                total_children: 3,
                completed: 1,
                failed: 0,
                running: 1,
                pending: 1,
                total_tokens_in: 1000,
                total_tokens_out: 500,
                children: vec![
                    omegon_traits::ChildVitalSigns {
                        label: "auth".into(),
                        status: "completed".into(),
                        started_at_unix_ms: Some(1_700_000_000_000),
                        last_activity_unix_ms: Some(1_700_000_010_000),
                        duration_secs: Some(10.0),
                        last_tool: Some("commit".into()),
                        last_tool_activity: None,
                        last_turn: Some(5),
                        tokens_in: 600,
                        tokens_out: 300,
                        tasks: Vec::new(),
                        tasks_done: 0,
                    },
                    omegon_traits::ChildVitalSigns {
                        label: "api".into(),
                        status: "running".into(),
                        started_at_unix_ms: Some(1_700_000_005_000),
                        last_activity_unix_ms: Some(1_700_000_012_000),
                        duration_secs: None,
                        last_tool: Some("write".into()),
                        last_tool_activity: Some(omegon_traits::ToolActivityVitalSigns {
                            raw_name: "bash".into(),
                            args_summary: Some("cargo test -p omegon".into()),
                        }),
                        last_turn: Some(2),
                        tokens_in: 400,
                        tokens_out: 200,
                        tasks: Vec::new(),
                        tasks_done: 0,
                    },
                    omegon_traits::ChildVitalSigns {
                        label: "ui".into(),
                        status: "pending".into(),
                        started_at_unix_ms: None,
                        last_activity_unix_ms: None,
                        duration_secs: None,
                        last_tool: None,
                        last_tool_activity: None,
                        last_turn: None,
                        tokens_in: 0,
                        tokens_out: 0,
                        tasks: Vec::new(),
                        tasks_done: 0,
                    },
                ],
            },
        };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "family_vital_signs");
        assert_eq!(json["event_name"], "family.vital_signs");
        assert_eq!(json["signs"]["run_id"], "run-42");
        assert_eq!(json["signs"]["active"], true);
        assert_eq!(json["signs"]["total_children"], 3);
        assert_eq!(json["signs"]["completed"], 1);
        assert_eq!(json["signs"]["running"], 1);
        assert_eq!(json["signs"]["pending"], 1);
        assert_eq!(json["signs"]["total_tokens_in"], 1000);
        assert_eq!(json["signs"]["children"].as_array().unwrap().len(), 3);
        assert_eq!(json["signs"]["children"][0]["label"], "auth");
        assert_eq!(json["signs"]["children"][0]["status"], "completed");
        assert_eq!(json["signs"]["children"][1]["status"], "running");
        assert_eq!(json["signs"]["children"][1]["last_tool"], "write");
        assert_eq!(
            json["signs"]["children"][1]["last_tool_activity"]["raw_name"],
            "bash"
        );
        assert_eq!(
            json["signs"]["children"][1]["last_tool_activity"]["args_summary"],
            "cargo test -p omegon"
        );
        assert_eq!(
            json["signs"]["children"][1]["duration_secs"],
            serde_json::Value::Null
        );
        assert_eq!(json["signs"]["children"][2]["status"], "pending");
        assert_eq!(
            json["signs"]["children"][2]["started_at_unix_ms"],
            serde_json::Value::Null
        );
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
        let event = AgentEvent::SystemNotification {
            message: "use <br> for newlines".into(),
        };
        let json = serialize_agent_event(&event);
        assert!(!json["message"].as_str().unwrap().contains("<br>"));
    }
}
