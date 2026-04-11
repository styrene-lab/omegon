//! Per-client IPC connection — handshake, dispatch, event push.

use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context as _;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

use omegon_traits::{
    AcceptedResponse, AgentEvent, ControlOutputResponse, ControlRequest,
    DispatcherSwitchRequest, HelloRequest, HelloResponse, IPC_PROTOCOL_VERSION, IpcCapability,
    IpcEnvelope, IpcEnvelopeKind, IpcErrorCode, IpcEventPayload, PingRequest, PingResponse,
    SlashCommandRequest, SlashCommandResponse, SubmitPromptRequest, SubscriptionRequest,
    SubscriptionResponse,
};

use super::snapshot::build_state_snapshot;
use super::wire::{decode_envelope, encode_envelope, read_frame};
use crate::tui::dashboard::DashboardHandles;
use crate::tui::{SharedCancel, TuiCommand};

fn parse_caller_role(raw: Option<&str>) -> crate::control_actions::ControlRole {
    match raw.unwrap_or("admin") {
        "read" => crate::control_actions::ControlRole::Read,
        "edit" => crate::control_actions::ControlRole::Edit,
        _ => crate::control_actions::ControlRole::Admin,
    }
}

/// Passed from the server into each connection task.
pub struct ConnectionConfig {
    pub omegon_version: String,
    pub cwd: String,
    pub started_at: String,
    pub server_instance_id: String,
    pub session_id: String,
    pub handles: DashboardHandles,
    pub events_tx: broadcast::Sender<AgentEvent>,
    pub command_tx: mpsc::Sender<TuiCommand>,
    pub shared_settings: crate::settings::SharedSettings,
    pub shared_cancel: SharedCancel,
    /// Cleared to `false` when this connection closes.
    pub has_controller: Arc<AtomicBool>,
}

pub struct IpcConnection {
    stream: UnixStream,
    cfg: ConnectionConfig,
}

impl IpcConnection {
    pub fn new(stream: UnixStream, cfg: ConnectionConfig) -> Self {
        Self { stream, cfg }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let IpcConnection { stream, cfg } = self;

        // Outbound frame queue — shared between dispatch task and event push task.
        let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);

        let (mut read_half, mut write_half) = tokio::io::split(stream);

        // Task: drain outbound queue → socket write half.
        let write_task = tokio::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if write_half.write_all(&frame).await.is_err() {
                    break;
                }
                let _ = write_half.flush().await;
            }
        });

        // ── Handshake ──────────────────────────────────────────────────
        let hello_raw = match read_frame(&mut read_half).await? {
            Some(r) => r,
            None => {
                cfg.has_controller.store(false, Ordering::SeqCst);
                return Ok(());
            }
        };
        let hello_env = decode_envelope(&hello_raw)?;
        if hello_env.kind != IpcEnvelopeKind::Hello || hello_env.method.as_deref() != Some("hello")
        {
            send_error(
                &out_tx,
                None,
                IpcErrorCode::InvalidPayload,
                "expected hello",
            )
            .await;
            cfg.has_controller.store(false, Ordering::SeqCst);
            return Ok(());
        }

        let client_versions: Vec<u16> = hello_env
            .payload
            .as_ref()
            .and_then(|p| serde_json::from_value::<HelloRequest>(p.clone()).ok())
            .map(|r| r.supported_protocol_versions)
            .unwrap_or_default();

        if !client_versions.contains(&IPC_PROTOCOL_VERSION) {
            send_error(
                &out_tx,
                hello_env.request_id,
                IpcErrorCode::UnsupportedProtocolVersion,
                "no supported protocol version in common",
            )
            .await;
            cfg.has_controller.store(false, Ordering::SeqCst);
            return Ok(());
        }

        let hello_resp = HelloResponse {
            protocol_version: IPC_PROTOCOL_VERSION,
            omegon_version: cfg.omegon_version.clone(),
            server_name: "omegon".into(),
            server_pid: std::process::id(),
            cwd: cfg.cwd.clone(),
            server_instance_id: cfg.server_instance_id.clone(),
            started_at: cfg.started_at.clone(),
            session_id: Some(cfg.session_id.clone()),
            capabilities: IpcCapability::v1_server_set()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        };

        send_response(
            &out_tx,
            hello_env.request_id,
            "hello",
            serde_json::to_value(&hello_resp)?,
        )
        .await;

        // ── Start event push task ──────────────────────────────────────
        let mut events_rx = cfg.events_tx.subscribe();
        let event_out_tx = out_tx.clone();
        let subscriptions: Arc<tokio::sync::Mutex<HashSet<String>>> =
            Arc::new(tokio::sync::Mutex::new(HashSet::new()));
        let sub_ref = subscriptions.clone();

        let event_task = tokio::spawn(async move {
            loop {
                match events_rx.recv().await {
                    Ok(ev) => {
                        if let Some(ipc_ev) = project_event(&ev) {
                            let name = event_name(&ipc_ev);
                            let subs = sub_ref.lock().await;
                            if subs.contains(name) {
                                drop(subs);
                                if let Ok(raw) = build_event_frame(&ipc_ev) {
                                    if event_out_tx.send(raw).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });

        // ── Request dispatch loop ──────────────────────────────────────
        let mut shutdown_requested = false;
        loop {
            let raw = match read_frame(&mut read_half).await {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err(e) => {
                    debug!("IPC frame read error: {e}");
                    break;
                }
            };

            let env = match decode_envelope(&raw) {
                Ok(e) => e,
                Err(e) => {
                    warn!("IPC decode error: {e}");
                    send_error(&out_tx, None, IpcErrorCode::InvalidPayload, &e.to_string()).await;
                    continue;
                }
            };

            if env.kind != IpcEnvelopeKind::Request {
                continue;
            }

            let req_id = env.request_id;
            let method = env.method.as_deref().unwrap_or("").to_string();
            let payload = env.payload.clone().unwrap_or(json!({}));

            match method.as_str() {
                "ping" => {
                    let nonce = serde_json::from_value::<PingRequest>(payload)
                        .map(|r| r.nonce)
                        .unwrap_or_default();
                    send_response(
                        &out_tx,
                        req_id,
                        "ping",
                        serde_json::to_value(PingResponse { nonce })?,
                    )
                    .await;
                }

                "get_state" => {
                    let snap = build_state_snapshot(
                        &cfg.handles,
                        &cfg.omegon_version,
                        &cfg.cwd,
                        &cfg.started_at,
                        &cfg.server_instance_id,
                        &cfg.session_id,
                    );
                    send_response(&out_tx, req_id, "get_state", serde_json::to_value(snap)?).await;
                }

                "submit_prompt" => {
                    let req = serde_json::from_value::<SubmitPromptRequest>(payload)
                        .context("parse submit_prompt")?;
                    let caller_role = parse_caller_role(req.caller_role.as_deref());
                    let required = crate::control_actions::classify_ipc_method("submit_prompt").role;
                    if !crate::control_actions::is_role_sufficient(caller_role, required) {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::InvalidPayload,
                            "caller role is insufficient for submit_prompt",
                        )
                        .await;
                        continue;
                    }
                    let turn_busy = cfg
                        .handles
                        .session
                        .lock()
                        .map(|session| session.busy)
                        .unwrap_or(true);
                    if turn_busy {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::TurnInProgress,
                            "the agent is still processing or unwinding the current turn",
                        )
                        .await;
                        continue;
                    }
                    let accepted = cfg
                        .command_tx
                        .send(TuiCommand::SubmitPrompt(crate::tui::PromptSubmission {
                            text: req.prompt,
                            image_paths: Vec::new(),
                            submitted_by: "ipc-controller".to_string(),
                            via: "ipc",
                        }))
                        .await
                        .is_ok();
                    send_response(
                        &out_tx,
                        req_id,
                        "submit_prompt",
                        serde_json::to_value(AcceptedResponse { accepted })?,
                    )
                    .await;
                }

                "cancel" => {
                    let caller_role = parse_caller_role(
                        payload.get("caller_role").and_then(|v| v.as_str()),
                    );
                    let required = crate::control_actions::classify_ipc_method("cancel").role;
                    if !crate::control_actions::is_role_sufficient(caller_role, required) {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::InvalidPayload,
                            "caller role is insufficient for cancel",
                        )
                        .await;
                        continue;
                    }
                    let accepted = if let Ok(guard) = cfg.shared_cancel.lock()
                        && let Some(ref cancel) = *guard
                    {
                        cancel.cancel();
                        true
                    } else {
                        false
                    };
                    send_response(
                        &out_tx,
                        req_id,
                        "cancel",
                        serde_json::to_value(AcceptedResponse { accepted })?,
                    )
                    .await;
                }

                "subscribe" => {
                    let req = serde_json::from_value::<SubscriptionRequest>(payload)
                        .unwrap_or(SubscriptionRequest { events: vec![] });
                    let valid: Vec<String> = req
                        .events
                        .into_iter()
                        .filter(|e| KNOWN_EVENTS.contains(&e.as_str()))
                        .collect();
                    {
                        let mut subs = subscriptions.lock().await;
                        for e in &valid {
                            subs.insert(e.clone());
                        }
                    }
                    send_response(
                        &out_tx,
                        req_id,
                        "subscribe",
                        serde_json::to_value(SubscriptionResponse { events: valid })?,
                    )
                    .await;
                }

                "unsubscribe" => {
                    let req = serde_json::from_value::<SubscriptionRequest>(payload)
                        .unwrap_or(SubscriptionRequest { events: vec![] });
                    let removed: Vec<String> = {
                        let mut subs = subscriptions.lock().await;
                        req.events.into_iter().filter(|e| subs.remove(e)).collect()
                    };
                    send_response(
                        &out_tx,
                        req_id,
                        "unsubscribe",
                        serde_json::to_value(SubscriptionResponse { events: removed })?,
                    )
                    .await;
                }

                "get_graph" => {
                    let graph = crate::web::api::build_graph_data(&cfg.handles);
                    send_response(
                        &out_tx,
                        req_id,
                        "get_graph",
                        serde_json::to_value(graph)?,
                    )
                    .await;
                }

                "context_status" | "context_compact" | "context_clear" | "new_session"
                | "auth_status" | "model_view" | "model_list" | "skills_view"
                | "skills_install" | "plugin_view" | "plugin_install" | "plugin_remove"
                | "plugin_update" | "secrets_view" | "secrets_set" | "secrets_get"
                | "secrets_delete" | "vault_status" | "vault_unseal" | "vault_login"
                | "vault_configure" | "vault_init_policy" | "cleave_status"
                | "cleave_cancel_child" | "delegate_status" | "set_model"
                | "switch_dispatcher" | "set_thinking" | "list_sessions" => {
                    let req = serde_json::from_value::<ControlRequest>(payload.clone())
                        .unwrap_or_default();
                    let caller_role = parse_caller_role(req.caller_role.as_deref());
                    let required = crate::control_actions::classify_ipc_method(&method).role;
                    if !crate::control_actions::is_role_sufficient(caller_role, required) {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::InvalidPayload,
                            &format!("caller role is insufficient for {method}"),
                        )
                        .await;
                        continue;
                    }
                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                    let request = match method.as_str() {
                        "context_status" => Some(crate::control_runtime::ControlRequest::ContextStatus),
                        "context_compact" => Some(crate::control_runtime::ControlRequest::ContextCompact),
                        "context_clear" => Some(crate::control_runtime::ControlRequest::ContextClear),
                        "new_session" => Some(crate::control_runtime::ControlRequest::NewSession),
                        "auth_status" => Some(crate::control_runtime::ControlRequest::AuthStatus),
                        "model_view" => Some(crate::control_runtime::ControlRequest::ModelView),
                        "model_list" => Some(crate::control_runtime::ControlRequest::ModelList),
                        "skills_view" => Some(crate::control_runtime::ControlRequest::SkillsView),
                        "skills_install" => {
                            Some(crate::control_runtime::ControlRequest::SkillsInstall)
                        }
                        "plugin_view" => Some(crate::control_runtime::ControlRequest::PluginView),
                        "plugin_install" => payload
                            .get("uri")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|uri| crate::control_runtime::ControlRequest::PluginInstall {
                                uri: uri.to_string(),
                            }),
                        "plugin_remove" => payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|name| crate::control_runtime::ControlRequest::PluginRemove {
                                name: name.to_string(),
                            }),
                        "plugin_update" => Some(crate::control_runtime::ControlRequest::PluginUpdate {
                            name: payload
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .filter(|s| !s.is_empty()),
                        }),
                        "secrets_view" => Some(crate::control_runtime::ControlRequest::SecretsView),
                        "secrets_set" => {
                            let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let value = payload.get("value").and_then(|v| v.as_str()).unwrap_or("");
                            if name.is_empty() || value.is_empty() {
                                None
                            } else {
                                Some(crate::control_runtime::ControlRequest::SecretsSet {
                                    name: name.to_string(),
                                    value: value.to_string(),
                                })
                            }
                        }
                        "secrets_get" => payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|name| crate::control_runtime::ControlRequest::SecretsGet {
                                name: name.to_string(),
                            }),
                        "secrets_delete" => payload
                            .get("name")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|name| crate::control_runtime::ControlRequest::SecretsDelete {
                                name: name.to_string(),
                            }),
                        "vault_status" => Some(crate::control_runtime::ControlRequest::VaultStatus),
                        "vault_unseal" => Some(crate::control_runtime::ControlRequest::VaultUnseal),
                        "vault_login" => Some(crate::control_runtime::ControlRequest::VaultLogin),
                        "vault_configure" => {
                            Some(crate::control_runtime::ControlRequest::VaultConfigure)
                        }
                        "vault_init_policy" => {
                            Some(crate::control_runtime::ControlRequest::VaultInitPolicy)
                        }
                        "cleave_status" => Some(crate::control_runtime::ControlRequest::CleaveStatus),
                        "cleave_cancel_child" => payload
                            .get("label")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|label| crate::control_runtime::ControlRequest::CleaveCancelChild {
                                label: label.to_string(),
                            }),
                        "delegate_status" => Some(crate::control_runtime::ControlRequest::DelegateStatus),
                        "list_sessions" => Some(crate::control_runtime::ControlRequest::ListSessions),
                        "set_model" => {
                            let model = payload
                                .get("model")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if model.is_empty() {
                                None
                            } else {
                                let current_model = cfg
                                    .shared_settings
                                    .lock()
                                    .ok()
                                    .map(|s| s.model.clone())
                                    .unwrap_or_default();
                                let classified = crate::control_actions::classify_ipc_set_model_request(
                                    &current_model,
                                    &model,
                                );
                                if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                                    send_error(
                                        &out_tx,
                                        req_id,
                                        IpcErrorCode::InvalidPayload,
                                        "caller role is insufficient for set_model",
                                    )
                                    .await;
                                    continue;
                                }
                                Some(crate::control_runtime::ControlRequest::SetModel {
                                    requested_model: model,
                                })
                            }
                        }
                        "switch_dispatcher" => {
                            let req = serde_json::from_value::<DispatcherSwitchRequest>(payload.clone())
                                .context("parse switch_dispatcher")?;
                            let classified = crate::control_actions::classify_ipc_method("switch_dispatcher");
                            let caller_role = parse_caller_role(req.caller_role.as_deref());
                            if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                                send_error(
                                    &out_tx,
                                    req_id,
                                    IpcErrorCode::InvalidPayload,
                                    "caller role is insufficient for switch_dispatcher",
                                )
                                .await;
                                continue;
                            }
                            if req.request_id.trim().is_empty() || req.profile.trim().is_empty() {
                                None
                            } else {
                                Some(crate::control_runtime::ControlRequest::SwitchDispatcher {
                                    request_id: req.request_id,
                                    profile: req.profile,
                                    model: req.model,
                                })
                            }
                        }
                        "set_thinking" => {
                            let level_raw = payload
                                .get("level")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            crate::settings::ThinkingLevel::parse(level_raw).map(|level| {
                                crate::control_runtime::ControlRequest::SetThinking { level }
                            })
                        }
                        _ => None,
                    };
                    let accepted = if let Some(request) = request {
                        cfg.command_tx
                            .send(TuiCommand::ExecuteControl {
                                request,
                                respond_to: Some(reply_tx),
                            })
                            .await
                            .is_ok()
                    } else {
                        false
                    };
                    let response = if accepted {
                        match reply_rx.await {
                            Ok(response) => response,
                            Err(_) => ControlOutputResponse {
                                accepted: false,
                                output: Some(format!(
                                    "{method} executor dropped response before completion"
                                )),
                            },
                        }
                    } else {
                        ControlOutputResponse {
                            accepted: false,
                            output: Some(format!("failed to enqueue {method}")),
                        }
                    };
                    send_response(
                        &out_tx,
                        req_id,
                        &method,
                        serde_json::to_value(response)?,
                    )
                    .await;
                }

                // Legacy compatibility adapter. Promoted control families should
                // arrive through typed IPC methods that translate directly into
                // `ControlRequest`; this path exists for slash-only commands and
                // older clients.
                "run_slash_command" => {
                    let req = serde_json::from_value::<SlashCommandRequest>(payload)
                        .context("parse run_slash_command")?;
                    let caller_role = parse_caller_role(req.caller_role.as_deref());
                    let classified =
                        crate::control_actions::classify_remote_slash_command(&req.name, &req.args);
                    if !classified.remote_safe {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::InvalidPayload,
                            "run_slash_command targets a local-only action",
                        )
                        .await;
                        continue;
                    }
                    if !crate::control_actions::is_role_sufficient(caller_role, classified.role) {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::InvalidPayload,
                            "caller role is insufficient for run_slash_command",
                        )
                        .await;
                        continue;
                    }
                    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
                    let accepted = cfg
                        .command_tx
                        .send(TuiCommand::RunSlashCommand {
                            name: req.name,
                            args: req.args,
                            respond_to: Some(reply_tx),
                        })
                        .await
                        .is_ok();
                    let response = if accepted {
                        match reply_rx.await {
                            Ok(response) => response,
                            Err(_) => SlashCommandResponse {
                                accepted: false,
                                output: Some(
                                    "slash command executor dropped response before completion"
                                        .to_string(),
                                ),
                            },
                        }
                    } else {
                        SlashCommandResponse {
                            accepted: false,
                            output: None,
                        }
                    };
                    send_response(
                        &out_tx,
                        req_id,
                        "run_slash_command",
                        serde_json::to_value(response)?,
                    )
                    .await;
                }

                "shutdown" => {
                    let caller_role = parse_caller_role(
                        payload.get("caller_role").and_then(|v| v.as_str()),
                    );
                    let required = crate::control_actions::classify_ipc_method("shutdown").role;
                    if !crate::control_actions::is_role_sufficient(caller_role, required) {
                        send_error(
                            &out_tx,
                            req_id,
                            IpcErrorCode::InvalidPayload,
                            "caller role is insufficient for shutdown",
                        )
                        .await;
                        continue;
                    }
                    send_response(
                        &out_tx,
                        req_id,
                        "shutdown",
                        serde_json::to_value(AcceptedResponse { accepted: true })?,
                    )
                    .await;
                    let _ = cfg.command_tx.send(TuiCommand::Quit).await;
                    shutdown_requested = true;
                    break;
                }

                other => {
                    send_error(
                        &out_tx,
                        req_id,
                        IpcErrorCode::UnknownMethod,
                        &format!("unknown method: {other}"),
                    )
                    .await;
                }
            }
        }

        // ── Cleanup ────────────────────────────────────────────────────
        drop(out_tx);
        event_task.abort();
        write_task.await.ok();
        cfg.has_controller.store(false, Ordering::SeqCst);
        debug!("IPC connection closed (shutdown_requested={shutdown_requested})");
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

async fn send_response(
    tx: &mpsc::Sender<Vec<u8>>,
    request_id: Option<[u8; 16]>,
    method: &str,
    payload: serde_json::Value,
) {
    let env = IpcEnvelope {
        protocol_version: IPC_PROTOCOL_VERSION,
        kind: IpcEnvelopeKind::Response,
        request_id,
        method: Some(method.into()),
        payload: Some(payload),
        error: None,
    };
    if let Ok(raw) = encode_envelope(&env) {
        let mut frame = Vec::with_capacity(4 + raw.len());
        frame.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        frame.extend_from_slice(&raw);
        let _ = tx.send(frame).await;
    }
}

async fn send_error(
    tx: &mpsc::Sender<Vec<u8>>,
    request_id: Option<[u8; 16]>,
    code: IpcErrorCode,
    message: &str,
) {
    let env = IpcEnvelope::error_response(request_id, code, message);
    if let Ok(raw) = encode_envelope(&env) {
        let mut frame = Vec::with_capacity(4 + raw.len());
        frame.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        frame.extend_from_slice(&raw);
        let _ = tx.send(frame).await;
    }
}

fn build_event_frame(ev: &IpcEventPayload) -> anyhow::Result<Vec<u8>> {
    let env = IpcEnvelope {
        protocol_version: IPC_PROTOCOL_VERSION,
        kind: IpcEnvelopeKind::Event,
        request_id: None,
        method: None,
        payload: Some(serde_json::to_value(ev)?),
        error: None,
    };
    let raw = encode_envelope(&env)?;
    let mut frame = Vec::with_capacity(4 + raw.len());
    frame.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    frame.extend_from_slice(&raw);
    Ok(frame)
}

/// Map an `AgentEvent` to an `IpcEventPayload`. Returns `None` for internal events
/// that have no IPC equivalent.
fn project_event(ev: &AgentEvent) -> Option<IpcEventPayload> {
    match ev {
        AgentEvent::TurnStart { turn } => Some(IpcEventPayload::TurnStarted { turn: *turn }),
        AgentEvent::TurnEnd {
            turn,
            estimated_tokens,
            actual_input_tokens,
            actual_output_tokens,
            cache_read_tokens,
            provider_telemetry,
            ..
        } => Some(IpcEventPayload::TurnEnded {
            turn: *turn,
            estimated_tokens: *estimated_tokens,
            actual_input_tokens: *actual_input_tokens,
            actual_output_tokens: *actual_output_tokens,
            cache_read_tokens: *cache_read_tokens,
            provider_telemetry: provider_telemetry.clone(),
        }),
        AgentEvent::MessageChunk { text } => {
            Some(IpcEventPayload::MessageDelta { text: text.clone() })
        }
        AgentEvent::ThinkingChunk { text } => {
            Some(IpcEventPayload::ThinkingDelta { text: text.clone() })
        }
        AgentEvent::MessageEnd => Some(IpcEventPayload::MessageCompleted),
        AgentEvent::ToolStart { id, name, args } => Some(IpcEventPayload::ToolStarted {
            id: id.clone(),
            name: name.clone(),
            args: args.clone(),
        }),
        AgentEvent::ToolUpdate { id, partial } => Some(IpcEventPayload::ToolUpdated {
            id: id.clone(),
            partial: partial.clone(),
        }),
        AgentEvent::ToolEnd {
            id,
            name,
            result,
            is_error,
        } => {
            let summary = result
                .content
                .iter()
                .filter_map(|b| b.as_text())
                .collect::<Vec<_>>()
                .join("\n")
                .chars()
                .take(200)
                .collect::<String>();
            Some(IpcEventPayload::ToolEnded {
                id: id.clone(),
                name: name.clone(),
                is_error: *is_error,
                summary: if summary.is_empty() { None } else { Some(summary) },
            })
        }
        AgentEvent::AgentEnd => Some(IpcEventPayload::AgentCompleted),
        AgentEvent::PhaseChanged { phase } => Some(IpcEventPayload::PhaseChanged {
            phase: format!("{:?}", phase),
        }),
        AgentEvent::DecompositionStarted { children } => {
            Some(IpcEventPayload::DecompositionStarted {
                children: children.clone(),
            })
        }
        AgentEvent::DecompositionChildCompleted { label, success } => {
            Some(IpcEventPayload::DecompositionChildCompleted {
                label: label.clone(),
                success: *success,
            })
        }
        AgentEvent::DecompositionCompleted { merged } => {
            Some(IpcEventPayload::DecompositionCompleted { merged: *merged })
        }
        AgentEvent::SystemNotification { message } => Some(IpcEventPayload::SystemNotification {
            message: message.clone(),
        }),
        AgentEvent::HarnessStatusChanged { .. } => Some(IpcEventPayload::HarnessChanged),
        AgentEvent::SessionReset => Some(IpcEventPayload::SessionReset),
        // Internal-only events — not projected to IPC
        AgentEvent::MessageStart { .. } => None,
        AgentEvent::MessageAbort => None,
        AgentEvent::ContextUpdated { .. } => None,
        AgentEvent::WebDashboardStarted { .. } => None,
    }
}

/// Return the stable event name string for a given payload (must match IpcEventPayload rename).
fn event_name(ev: &IpcEventPayload) -> &'static str {
    match ev {
        IpcEventPayload::TurnStarted { .. } => "turn.started",
        IpcEventPayload::TurnEnded { .. } => "turn.ended",
        IpcEventPayload::MessageDelta { .. } => "message.delta",
        IpcEventPayload::ThinkingDelta { .. } => "thinking.delta",
        IpcEventPayload::MessageCompleted => "message.completed",
        IpcEventPayload::ToolStarted { .. } => "tool.started",
        IpcEventPayload::ToolUpdated { .. } => "tool.updated",
        IpcEventPayload::ToolEnded { .. } => "tool.ended",
        IpcEventPayload::AgentCompleted => "agent.completed",
        IpcEventPayload::PhaseChanged { .. } => "phase.changed",
        IpcEventPayload::DecompositionStarted { .. } => "decomposition.started",
        IpcEventPayload::DecompositionChildCompleted { .. } => "decomposition.child_completed",
        IpcEventPayload::DecompositionCompleted { .. } => "decomposition.completed",
        IpcEventPayload::HarnessChanged => "harness.changed",
        IpcEventPayload::StateChanged { .. } => "state.changed",
        IpcEventPayload::SystemNotification { .. } => "system.notification",
        IpcEventPayload::SessionReset => "session.reset",
    }
}

/// All v1 event names. Only names in this set are accepted by `subscribe`.
const KNOWN_EVENTS: &[&str] = &[
    "turn.started",
    "turn.ended",
    "message.delta",
    "thinking.delta",
    "message.completed",
    "tool.started",
    "tool.updated",
    "tool.ended",
    "agent.completed",
    "phase.changed",
    "decomposition.started",
    "decomposition.child_completed",
    "decomposition.completed",
    "harness.changed",
    "state.changed",
    "system.notification",
    "session.reset",
];
