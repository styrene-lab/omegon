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
    Query(query): Query<WsQuery>,
    State(state): State<WebState>,
) -> impl IntoResponse {
    if state.web_auth.verify_query_token(query.token.as_deref()) {
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

/// Process a command from a WebSocket client.
async fn handle_client_command(
    cmd: &Value,
    command_tx: &tokio::sync::mpsc::Sender<WebCommand>,
    state: &WebState,
    snapshot_tx: &tokio::sync::mpsc::Sender<Value>,
) {
    let cmd_type = cmd["type"].as_str().unwrap_or("");
    let caller_role = match cmd["caller_role"].as_str().unwrap_or("admin") {
        "read" => crate::control_actions::ControlRole::Read,
        "edit" => crate::control_actions::ControlRole::Edit,
        _ => crate::control_actions::ControlRole::Admin,
    };

    match cmd_type {
        "user_prompt" => {
            if let Some(text) = cmd["text"].as_str() {
                let _ = command_tx
                    .send(WebCommand::UserPrompt(text.to_string()))
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
                                "model_view executor dropped response before completion".to_string(),
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
                                "model_list executor dropped response before completion".to_string(),
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
                                "skills_view executor dropped response before completion".to_string(),
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
                    request: crate::control_runtime::ControlRequest::SkillsInstall,
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
                                "plugin_view executor dropped response before completion".to_string(),
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
                    .and_then(|h| h.providers.iter().find(|p| p.authenticated).and_then(|p| p.model.clone()))
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
                                    "set_model executor dropped response before completion".to_string(),
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
            let model = cmd["model"].as_str().map(|s| s.trim()).filter(|s| !s.is_empty());
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
                                    "set_thinking executor dropped response before completion".to_string(),
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
                        name: cmd["name"].as_str().map(|s| s.to_string()).filter(|s| !s.is_empty()),
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
                                "auth_status executor dropped response before completion".to_string(),
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
                                "context_status executor dropped response before completion".to_string(),
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
                                "context_compact executor dropped response before completion".to_string(),
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
                                "context_clear executor dropped response before completion".to_string(),
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
            let caller_role = match cmd["caller_role"].as_str().unwrap_or("admin") {
                "read" => crate::control_actions::ControlRole::Read,
                "edit" => crate::control_actions::ControlRole::Edit,
                _ => crate::control_actions::ControlRole::Admin,
            };
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
        AgentEvent::TurnEnd { .. } => Some(&["session", "design", "openspec", "cleave"]),
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
        AgentEvent::TurnEnd {
            turn,
            model,
            provider,
            turn_end_reason,
            estimated_tokens,
            actual_input_tokens,
            actual_output_tokens,
            cache_read_tokens,
            provider_telemetry,
            dominant_phase,
            drift_kind,
            progress_nudge_reason,
            streaks,
            ..
        } => json!({
            "type": "turn_end",
            "event_name": "turn.ended",
            "turn": turn,
            "estimated_tokens": estimated_tokens,
            "model": model,
            "provider": provider,
            "turn_end_reason": turn_end_reason,
            "actual_input_tokens": actual_input_tokens,
            "actual_output_tokens": actual_output_tokens,
            "cache_read_tokens": cache_read_tokens,
            "provider_telemetry": provider_telemetry,
            "dominant_phase": dominant_phase,
            "drift_kind": drift_kind,
            "progress_nudge_reason": progress_nudge_reason,
            "streaks": {
                "orientation_churn": streaks.orientation_churn,
                "repeated_action_failure": streaks.repeated_action_failure,
                "validation_thrash": streaks.validation_thrash,
                "closure_stall": streaks.closure_stall,
                "constraint_discovery": streaks.constraint_discovery,
                "evidence_sufficient": streaks.evidence_sufficient,
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
        AgentEvent::MessageAbort => json!({
            "type": "message_abort",
        }),
        AgentEvent::ToolStart { id, name, args } => json!({
            "type": "tool_start",
            "event_name": "tool.started",
            "id": id,
            "name": name,
            "tool_name": name,
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
                "result": escape_html(&result_text),
                "is_error": is_error,
                "block_count": result.content.len(),
            })
        }
        AgentEvent::AgentEnd => json!({
            "type": "agent_end",
            "event_name": "agent.completed",
        }),
        AgentEvent::PhaseChanged { phase } => json!({
            "type": "phase_changed",
            "event_name": "phase.changed",
            "phase": format!("{phase:?}"),
        }),
        AgentEvent::DecompositionStarted { children } => json!({
            "type": "decomposition_started",
            "event_name": "decomposition.started",
            "children": children,
        }),
        AgentEvent::DecompositionChildCompleted { label, success } => json!({
            "type": "decomposition_child_completed",
            "event_name": "decomposition.child_completed",
            "label": escape_html(label),
            "success": success,
        }),
        AgentEvent::DecompositionCompleted { merged } => json!({
            "type": "decomposition_completed",
            "event_name": "decomposition.completed",
            "merged": merged,
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
                    "last_turn": c.last_turn,
                    "tokens_in": c.tokens_in,
                    "tokens_out": c.tokens_out,
                })).collect::<Vec<_>>(),
            },
        }),
        AgentEvent::SystemNotification { message } => json!({
            "type": "system_notification",
            "event_name": "system.notification",
            "message": escape_html(message),
        }),
        AgentEvent::HarnessStatusChanged { status_json } => json!({
            "type": "harness_status_changed",
            "event_name": "harness.changed",
            "status": status_json,
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
    async fn handle_client_command_enqueues_switch_dispatcher_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

        let cmd = serde_json::json!({
            "type": "switch_dispatcher",
            "request_id": "req-123",
            "profile": "victory",
            "model": "anthropic:claude-sonnet-4-6",
            "caller_role": "admin"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl { request, respond_to } => {
                match request {
                    crate::control_runtime::ControlRequest::SwitchDispatcher {
                        request_id,
                        profile,
                        model,
                    } => {
                        assert_eq!(request_id, "req-123");
                        assert_eq!(profile, "victory");
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
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

        let cmd = serde_json::json!({
            "type": "switch_dispatcher",
            "request_id": "req-123",
            "profile": "victory",
            "caller_role": "edit"
        });

        handle_client_command(&cmd, &command_tx, &state, &snapshot_tx).await;
        assert!(command_rx.try_recv().is_err(), "should not enqueue command");
        let msg = snapshot_rx.recv().await.expect("snapshot message");
        assert_eq!(msg["type"], "system_message");
        assert_eq!(msg["message"], "caller role is insufficient for switch_dispatcher");
    }

    #[tokio::test]
    async fn handle_client_command_enqueues_cleave_child_cancel_and_reports_result() {
        let (events_tx, _) = tokio::sync::broadcast::channel(4);
        let (command_tx, mut command_rx) = tokio::sync::mpsc::channel(4);
        let (snapshot_tx, mut snapshot_rx) = tokio::sync::mpsc::channel(4);
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

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
            WebCommand::ExecuteControl { request, respond_to } => {
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
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

        let cmd = serde_json::json!({
            "type": "secrets_view",
            "caller_role": "edit"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl { request, respond_to } => {
                assert!(matches!(request, crate::control_runtime::ControlRequest::SecretsView));
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
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

        let cmd = serde_json::json!({
            "type": "cleave_status",
            "caller_role": "read"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl { request, respond_to } => {
                assert!(matches!(request, crate::control_runtime::ControlRequest::CleaveStatus));
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
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

        let cmd = serde_json::json!({
            "type": "delegate_status",
            "caller_role": "read"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl { request, respond_to } => {
                assert!(matches!(request, crate::control_runtime::ControlRequest::DelegateStatus));
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
        let state = WebState::new(crate::tui::dashboard::DashboardHandles::default(), events_tx);

        let cmd = serde_json::json!({
            "type": "vault_status",
            "caller_role": "read"
        });

        let state_for_handler = state.clone();
        let handler = tokio::spawn(async move {
            handle_client_command(&cmd, &command_tx, &state_for_handler, &snapshot_tx).await;
        });

        match command_rx.recv().await.expect("command") {
            WebCommand::ExecuteControl { request, respond_to } => {
                assert!(matches!(request, crate::control_runtime::ControlRequest::VaultStatus));
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
    fn serialize_turn_start() {
        let event = AgentEvent::TurnStart { turn: 5 };
        let json = serialize_agent_event(&event);
        assert_eq!(json["type"], "turn_start");
        assert_eq!(json["event_name"], "turn.started");
        assert_eq!(json["turn"], 5);
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
        let event = AgentEvent::TurnEnd {
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
        };
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
            AgentEvent::TurnEnd { .. } => {}
            AgentEvent::MessageStart { .. } => {}
            AgentEvent::MessageChunk { .. } => {}
            AgentEvent::ThinkingChunk { .. } => {}
            AgentEvent::MessageEnd => {}
            AgentEvent::MessageAbort => {}
            AgentEvent::ToolStart { .. } => {}
            AgentEvent::ToolUpdate { .. } => {}
            AgentEvent::ToolEnd { .. } => {}
            AgentEvent::AgentEnd => {}
            AgentEvent::PhaseChanged { .. } => {}
            AgentEvent::DecompositionStarted { .. } => {}
            AgentEvent::DecompositionChildCompleted { .. } => {}
            AgentEvent::DecompositionCompleted { .. } => {}
            AgentEvent::FamilyVitalSignsUpdated { .. } => {}
            AgentEvent::SystemNotification { .. } => {}
            AgentEvent::HarnessStatusChanged { .. } => {}
            AgentEvent::WebDashboardStarted { .. } => {}
            AgentEvent::ContextUpdated { .. } => {}
            AgentEvent::SessionReset => {}
        }
    }

    #[test]
    fn serialize_all_event_types() {
        let events = vec![
            AgentEvent::TurnStart { turn: 1 },
            AgentEvent::TurnEnd {
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
            },
            AgentEvent::MessageStart {
                role: "assistant".into(),
            },
            AgentEvent::MessageChunk { text: "hi".into() },
            AgentEvent::ThinkingChunk { text: "hmm".into() },
            AgentEvent::MessageEnd,
            AgentEvent::ToolStart {
                id: "1".into(),
                name: "read".into(),
                args: serde_json::json!({}),
            },
            AgentEvent::ToolUpdate {
                id: "1".into(),
                partial: omegon_traits::PartialToolResult::content("partial", 100),
            },
            AgentEvent::ToolEnd {
                id: "1".into(),
                name: "read".into(),
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
            },
            AgentEvent::DecompositionChildCompleted {
                label: "a".into(),
                success: true,
            },
            AgentEvent::DecompositionCompleted { merged: true },
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
                        last_turn: Some(3),
                        tokens_in: 60,
                        tokens_out: 30,
                    }],
                },
            },
            AgentEvent::SystemNotification {
                message: "test".into(),
            },
            // Variants previously missing from this test — added so the
            // exhaustive `_exhaustive_agent_event_serialization_coverage`
            // guard above is the *only* thing the test relies on for
            // coverage. If you add a new AgentEvent variant, you must
            // both extend the guard above AND add an entry here.
            AgentEvent::MessageAbort,
            AgentEvent::HarnessStatusChanged {
                status_json: serde_json::json!({"thinking_level": "low"}),
            },
            AgentEvent::WebDashboardStarted {
                startup_json: serde_json::json!({"port": 0}),
            },
            AgentEvent::ContextUpdated {
                tokens: 1000,
                context_window: 200_000,
                context_class: "Squad".into(),
                thinking_level: "Low".into(),
            },
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
            21,
            "should cover all 21 AgentEvent variants — see _exhaustive_agent_event_serialization_coverage"
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
                        last_turn: Some(5),
                        tokens_in: 600,
                        tokens_out: 300,
                    },
                    omegon_traits::ChildVitalSigns {
                        label: "api".into(),
                        status: "running".into(),
                        started_at_unix_ms: Some(1_700_000_005_000),
                        last_activity_unix_ms: Some(1_700_000_012_000),
                        duration_secs: None,
                        last_tool: Some("write".into()),
                        last_turn: Some(2),
                        tokens_in: 400,
                        tokens_out: 200,
                    },
                    omegon_traits::ChildVitalSigns {
                        label: "ui".into(),
                        status: "pending".into(),
                        started_at_unix_ms: None,
                        last_activity_unix_ms: None,
                        duration_secs: None,
                        last_tool: None,
                        last_turn: None,
                        tokens_in: 0,
                        tokens_out: 0,
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
