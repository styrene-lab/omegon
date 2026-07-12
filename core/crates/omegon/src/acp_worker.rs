//! ACP worker thread — owns the agent session and processes prompts
//! on a dedicated thread with its own tokio runtime.
//!
//! The ACP I/O thread communicates via channels, keeping the agent loop's
//! `!Send` types isolated while allowing the ACP connection to remain
//! responsive (streaming, cancel, notifications).

use omegon_traits::Feature;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// A single plan entry for decomposition/phased progress.
#[derive(Debug, Clone)]
pub struct PlanEntryData {
    pub content: String,
    pub status: PlanEntryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanEntryState {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Request from the ACP thread to the worker.
pub enum WorkerRequest {
    /// Execute a prompt and return the response text.
    Prompt {
        text: String,
        response_tx: oneshot::Sender<WorkerResponse>,
    },
    /// Cancel the current prompt.
    Cancel,
    /// Change the model. `ack` (when provided) fires after shared_settings is
    /// updated so the caller can read the applied state without racing the
    /// channel.
    SetModel {
        value: String,
        ack: Option<oneshot::Sender<()>>,
    },
    /// Change thinking level.
    SetThinking {
        value: String,
        ack: Option<oneshot::Sender<()>>,
    },
    /// Change posture.
    SetPosture {
        value: String,
        ack: Option<oneshot::Sender<()>>,
    },
    /// Execute a control request (slash command) and return the response.
    /// This gives ACP clients access to every operation the TUI has.
    ControlRequest {
        command: String,
        response_tx: oneshot::Sender<WorkerResponse>,
    },
    /// Connect MCP servers forwarded by the ACP client.
    ConnectMcpServers {
        servers: Vec<(String, crate::plugins::mcp::McpServerConfig)>,
    },
    /// Shut down the worker.
    Shutdown,
}

/// Response from the worker to the ACP thread.
pub struct WorkerResponse {
    pub text: String,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Event streamed from the worker during prompt execution.
#[derive(Clone, Debug)]
pub enum WorkerEvent {
    TextChunk(String),
    ThinkingChunk(String),
    ToolStart {
        id: String,
        name: String,
        /// Raw input arguments to the tool (forwarded as ACP ToolCall.raw_input
        /// so clients can render call metadata, not just the bare tool name).
        args: Option<serde_json::Value>,
    },
    ToolEnd {
        id: String,
        success: bool,
        details: serde_json::Value,
    },
    /// Partial tool output for streaming to the client.
    ToolOutput {
        id: String,
        text: String,
    },
    /// Extension deployment metadata discovered after worker setup.
    ExtensionMetadata(std::collections::BTreeMap<String, serde_json::Value>),
    /// Loaded extension RPC handles discovered after worker setup.
    ExtensionHandles(std::collections::BTreeMap<String, crate::extensions::ExtensionPollingHandle>),
    /// Execution plan update (decomposition children, phased work).
    PlanUpdate {
        entries: Vec<PlanEntryData>,
    },
    /// Session title derived from the first prompt or active skill.
    SessionTitle(String),
    /// Status update from the agent loop (e.g., "Loading model into memory…")
    StatusUpdate(String),
    /// Structured stream-idle telemetry.
    StreamIdle {
        provider: String,
        model: String,
        phase: String,
        idle_secs: u64,
        ambiguous: bool,
        message: String,
    },
    /// Structured provider retry telemetry.
    ProviderRetry {
        provider: String,
        model: String,
        attempt: u32,
        delay_ms: u64,
        reason: String,
        message: String,
        recoverable: bool,
    },
    /// Structured provider terminal failure telemetry.
    ProviderFailure {
        provider: String,
        model: String,
        reason: String,
        attempts: u32,
        message: String,
        retryable: bool,
        recommended_action: String,
    },
    /// Structured turn cancellation telemetry.
    TurnCancelled {
        reason: String,
    },
    TurnComplete,
}

/// Handle to communicate with the worker thread.
pub struct WorkerHandle {
    pub request_tx: mpsc::Sender<WorkerRequest>,
    pub event_rx: tokio::sync::broadcast::Receiver<WorkerEvent>,
    /// Live settings owned by the worker. Allows the ACP transport thread to
    /// observe the current effective model/thinking/posture without round-
    /// tripping a request — needed because new_session and dropdown
    /// rebuilds must reflect the actual values the worker is using.
    pub settings: crate::settings::SharedSettings,
    /// Secrets manager from the worker — arrives asynchronously after setup.
    /// Used by the ACP transport to redact streaming output before emission.
    pub secrets_rx: tokio::sync::oneshot::Receiver<std::sync::Arc<omegon_secrets::SecretsManager>>,
}

/// Spawn the worker thread. Returns a handle for communication.
pub fn spawn_worker(
    model: String,
    cwd: PathBuf,
    host_ctx: Option<crate::host_context::HostContext>,
    dangerously_bypass_permissions: bool,
) -> WorkerHandle {
    let (request_tx, request_rx) = mpsc::channel::<WorkerRequest>(16);
    let (event_tx, event_rx) = tokio::sync::broadcast::channel::<WorkerEvent>(256);
    let (secrets_tx, secrets_rx) = tokio::sync::oneshot::channel();

    let shared_settings = crate::settings::shared(&model);
    let worker_settings = shared_settings.clone();

    std::thread::Builder::new()
        .name("omegon-acp-worker".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("worker runtime");

            let local = tokio::task::LocalSet::new();
            local.block_on(
                &rt,
                worker_loop(
                    model,
                    cwd,
                    worker_settings,
                    request_rx,
                    event_tx,
                    secrets_tx,
                    host_ctx,
                    dangerously_bypass_permissions,
                ),
            );
        })
        .expect("failed to spawn worker thread");

    WorkerHandle {
        request_tx,
        event_rx,
        settings: shared_settings,
        secrets_rx,
    }
}

/// The worker's main loop — runs on a dedicated thread with its own runtime.
#[allow(clippy::too_many_arguments)]
async fn worker_loop(
    model: String,
    cwd: PathBuf,
    shared_settings: crate::settings::SharedSettings,
    mut request_rx: mpsc::Receiver<WorkerRequest>,
    event_tx: tokio::sync::broadcast::Sender<WorkerEvent>,
    secrets_tx: tokio::sync::oneshot::Sender<std::sync::Arc<omegon_secrets::SecretsManager>>,
    host_ctx: Option<crate::host_context::HostContext>,
    dangerously_bypass_permissions: bool,
) {
    // Set the canonical project root env var so extensions can locate the workspace
    // without depending on embedder-specific names (FLYNT_VAULT, CODEX_VAULT).
    unsafe { std::env::set_var("OMEGON_PROJECT_ROOT", &cwd) };

    // Apply profile + initial model to the shared settings provided by spawn_worker.
    // Worker mutates these on SetModel/SetThinking/SetPosture; the ACP transport
    // thread reads them when rebuilding ConfigOption lists.
    if let Ok(mut s) = shared_settings.lock() {
        let profile = crate::settings::Profile::load(&cwd);
        let has_profile_model = profile.last_used_model.is_some();
        profile.apply_to_with_posture(&mut s, &cwd);
        if !has_profile_model {
            s.set_model(&model);
        }
    }

    let agent_setup = match crate::setup::AgentSetup::new_with_safety(
        &cwd,
        None,
        Some(shared_settings.clone()),
        dangerously_bypass_permissions,
    )
    .await
    {
        Ok(mut setup) => {
            let setup_instance_id = setup.instance_id.clone();
            setup.instance_id = crate::paths::instance_id("acp");
            setup.workspace_state.lease.owner_agent_id = Some("omegon-acp".into());
            let _ = crate::workspace::runtime::write_workspace_lease(
                &cwd,
                &setup.instance_id,
                &setup.workspace_state.lease,
            );
            crate::workspace::runtime::cleanup_instance(&cwd, &setup_instance_id);
            setup
        }
        Err(e) => {
            tracing::error!(error = %e, "worker setup failed");
            return;
        }
    };

    let session_id = agent_setup.session_id.clone();
    let instance_id = agent_setup.instance_id.clone();
    let mut bus = agent_setup.bus;
    let mut context_manager = agent_setup.context_manager;
    let mut conversation = agent_setup.conversation;
    let secrets = agent_setup.secrets;
    let extension_metadata = agent_setup.extension_metadata.clone();
    let extension_rpc_handles = agent_setup.extension_rpc_handles.clone();
    let mut cancel = CancellationToken::new();

    let _ = secrets_tx.send(secrets.clone());
    if !extension_metadata.is_empty() {
        let _ = event_tx.send(WorkerEvent::ExtensionMetadata(extension_metadata));
    }
    if !extension_rpc_handles.is_empty() {
        let _ = event_tx.send(WorkerEvent::ExtensionHandles(extension_rpc_handles));
    }
    let host_ctx_arc = host_ctx.map(std::sync::Arc::new);
    tracing::info!(model = %model, "ACP worker ready");
    let mut first_prompt = true;

    // Process requests
    while let Some(req) = request_rx.recv().await {
        match req {
            WorkerRequest::Prompt { text, response_tx } => {
                if first_prompt {
                    first_prompt = false;
                    let title: String =
                        text.chars().take(80).collect::<String>().trim().to_string();
                    let title = title.lines().next().unwrap_or(&title).trim().to_string();
                    if !title.is_empty() {
                        let _ = event_tx.send(WorkerEvent::SessionTitle(title));
                    }
                }
                conversation.push_user(text);

                // Resolve the model from settings (may have been changed via SetModel)
                let current_model = shared_settings
                    .lock()
                    .ok()
                    .map(|s| s.model.clone())
                    .unwrap_or_else(|| model.clone());

                let bridge = match crate::providers::auto_detect_bridge(&current_model).await {
                    Some(b) => b,
                    None => {
                        let _ = response_tx.send(WorkerResponse {
                            text: String::new(),
                            error: Some(format!(
                                "No LLM provider available for {current_model}. \
                                 Check Ollama is running or set an API key."
                            )),
                            cancelled: false,
                        });
                        continue;
                    }
                };

                // Events channel — forward to the ACP thread
                let (loop_events_tx, mut loop_events_rx) =
                    tokio::sync::broadcast::channel::<omegon_traits::AgentEvent>(256);

                // Forward agent events to worker events
                let worker_event_tx = event_tx.clone();
                tokio::task::spawn_local(async move {
                    while let Ok(event) = loop_events_rx.recv().await {
                        let worker_event = match event {
                            omegon_traits::AgentEvent::MessageChunk { text, .. } => {
                                Some(WorkerEvent::TextChunk(text))
                            }
                            omegon_traits::AgentEvent::ThinkingChunk { text, .. } => {
                                Some(WorkerEvent::ThinkingChunk(text))
                            }
                            omegon_traits::AgentEvent::ToolStart { id, name, args, .. } => {
                                Some(WorkerEvent::ToolStart {
                                    id,
                                    name,
                                    args: if args.is_null() { None } else { Some(args) },
                                })
                            }
                            omegon_traits::AgentEvent::ToolEnd {
                                id,
                                result,
                                is_error,
                                ..
                            } => Some(WorkerEvent::ToolEnd {
                                id,
                                success: !is_error,
                                details: result.details,
                            }),
                            omegon_traits::AgentEvent::ToolUpdate { id, partial } => {
                                if partial.tail.is_empty() {
                                    None
                                } else {
                                    Some(WorkerEvent::ToolOutput {
                                        id,
                                        text: partial.tail,
                                    })
                                }
                            }
                            omegon_traits::AgentEvent::DecompositionStarted {
                                children, ..
                            } => {
                                let entries = children
                                    .iter()
                                    .map(|label| PlanEntryData {
                                        content: label.clone(),
                                        status: PlanEntryState::Pending,
                                    })
                                    .collect();
                                Some(WorkerEvent::PlanUpdate { entries })
                            }
                            omegon_traits::AgentEvent::DecompositionChildCompleted {
                                label,
                                success,
                                operation: _,
                            } => {
                                // Emit a single-entry update; the ACP forwarder
                                // merges it into the running plan state.
                                let status = if success {
                                    PlanEntryState::Completed
                                } else {
                                    PlanEntryState::Failed
                                };
                                Some(WorkerEvent::PlanUpdate {
                                    entries: vec![PlanEntryData {
                                        content: label,
                                        status,
                                    }],
                                })
                            }
                            omegon_traits::AgentEvent::DecompositionCompleted { .. } => {
                                // Final plan — all entries completed. The ACP
                                // forwarder will emit the full snapshot.
                                None
                            }
                            omegon_traits::AgentEvent::PlanUpdated { projection } => {
                                let entries = crate::acp::plan_entries_from_projection(&projection);
                                Some(WorkerEvent::PlanUpdate { entries })
                            }
                            omegon_traits::AgentEvent::SystemNotification { message } => {
                                Some(WorkerEvent::StatusUpdate(message))
                            }
                            omegon_traits::AgentEvent::StreamIdle {
                                provider,
                                model,
                                phase,
                                idle_secs,
                                ambiguous,
                                message,
                            } => Some(WorkerEvent::StreamIdle {
                                provider,
                                model,
                                phase,
                                idle_secs,
                                ambiguous,
                                message,
                            }),
                            omegon_traits::AgentEvent::ProviderRetry {
                                provider,
                                model,
                                attempt,
                                delay_ms,
                                reason,
                                message,
                                recoverable,
                            } => Some(WorkerEvent::ProviderRetry {
                                provider,
                                model,
                                attempt,
                                delay_ms,
                                reason,
                                message,
                                recoverable,
                            }),
                            omegon_traits::AgentEvent::ProviderFailure {
                                provider,
                                model,
                                reason,
                                attempts,
                                message,
                                retryable,
                                recommended_action,
                            } => Some(WorkerEvent::ProviderFailure {
                                provider,
                                model,
                                reason,
                                attempts,
                                message,
                                retryable,
                                recommended_action,
                            }),
                            omegon_traits::AgentEvent::TurnCancelled { reason } => {
                                Some(WorkerEvent::TurnCancelled { reason })
                            }
                            omegon_traits::AgentEvent::MessageAbort { reason } => {
                                let msg = reason.unwrap_or_else(|| "Request aborted".into());
                                Some(WorkerEvent::StatusUpdate(format!("[Error: {msg}]")))
                            }
                            _ => None,
                        };
                        if let Some(e) = worker_event {
                            let _ = worker_event_tx.send(e);
                        }
                    }
                });

                cancel = CancellationToken::new();

                let loop_config = crate::r#loop::LoopConfig {
                    max_turns: shared_settings
                        .lock()
                        .ok()
                        .map(|s| s.max_turns)
                        .unwrap_or(50),
                    soft_limit_turns: 35,
                    max_retries: 100,
                    retry_delay_ms: 750,
                    model: current_model,
                    bridge_model: None,
                    route_controller: None,
                    cwd: cwd.clone(),
                    extended_context: false,
                    settings: Some(shared_settings.clone()),
                    secrets: Some(secrets.clone()),
                    force_compact: None,
                    allow_commit_nudge: true,
                    enforce_first_turn_execution_bias: false,
                    ollama_manager: None,
                    skill_phases: Vec::new(),
                    host_context: host_ctx_arc.clone(),
                    permission_policy: None,
                    permission_role: None,
                    cancel_keeps_prompt: None,
                    drain_post_loop_requests: false,
                };

                let result = crate::r#loop::run(
                    bridge.as_ref(),
                    &mut bus,
                    &mut context_manager,
                    &mut conversation,
                    &loop_events_tx,
                    cancel.clone(),
                    &loop_config,
                )
                .await;

                drop(loop_events_tx);
                let _ = event_tx.send(WorkerEvent::TurnComplete);

                let response_text = conversation.last_assistant_text().unwrap_or("").to_string();

                let cancelled = cancel.is_cancelled();
                let error = match result {
                    Ok(()) => None,
                    Err(_e) if cancelled => None,
                    Err(e) => {
                        let raw = e.to_string();
                        let model_name = shared_settings
                            .lock()
                            .ok()
                            .map(|s| s.model.clone())
                            .unwrap_or_default();
                        Some(humanize_agent_error(&raw, &model_name))
                    }
                };

                // Save session
                let _ = crate::session::save_session(&conversation, &cwd, None);

                let _ = response_tx.send(WorkerResponse {
                    text: response_text,
                    error,
                    cancelled,
                });
                if cancelled {
                    cancel = CancellationToken::new();
                }
            }

            WorkerRequest::Cancel => {
                cancel.cancel();
                let _ = event_tx.send(WorkerEvent::TurnCancelled {
                    reason: "operator_cancelled".to_string(),
                });
            }

            WorkerRequest::SetModel { value, ack } => {
                if let Ok(mut s) = shared_settings.lock() {
                    s.set_model(&value);
                    let mut profile = crate::settings::Profile::load(&cwd);
                    profile.capture_from(&s);
                    let _ = profile.save(&cwd);
                }
                if let Some(tx) = ack {
                    let _ = tx.send(());
                }
            }

            WorkerRequest::SetThinking { value, ack } => {
                if let Some(l) = crate::settings::ThinkingLevel::parse(&value)
                    && let Ok(mut s) = shared_settings.lock()
                {
                    s.thinking = l;
                    let mut profile = crate::settings::Profile::load(&cwd);
                    profile.capture_from(&s);
                    let _ = profile.save(&cwd);
                }
                if let Some(tx) = ack {
                    let _ = tx.send(());
                }
            }

            WorkerRequest::SetPosture { value, ack } => {
                let posture = match value.as_str() {
                    "fabricator" => Some(crate::settings::PosturePreset::Fabricator),
                    "architect" => Some(crate::settings::PosturePreset::Architect),
                    "explorator" => Some(crate::settings::PosturePreset::Explorator),
                    "devastator" => Some(crate::settings::PosturePreset::Devastator),
                    _ => None,
                };
                if let Some(p) = posture
                    && let Ok(mut s) = shared_settings.lock()
                {
                    s.set_posture(p);
                    let mut profile = crate::settings::Profile::load(&cwd);
                    profile.capture_from(&s);
                    let _ = profile.save(&cwd);
                }
                if let Some(tx) = ack {
                    let _ = tx.send(());
                }
            }

            WorkerRequest::ControlRequest {
                command,
                response_tx,
            } => {
                let mut text = handle_control_request(
                    &command,
                    &conversation,
                    &shared_settings,
                    &secrets,
                    workspace_ctx(&cwd, &session_id, &instance_id),
                    &mut bus,
                    dangerously_bypass_permissions,
                )
                .await;
                // Persona switch needs async bus.execute_tool — handle the marker
                if let Some(name) = text.strip_prefix("__async_persona_switch:") {
                    let name = name.to_string();
                    let cancel = CancellationToken::new();
                    let args = serde_json::json!({ "name": name });
                    match bus
                        .execute_tool("switch_persona", "ctrl", args, cancel)
                        .await
                    {
                        Ok(result) => {
                            text = result
                                .content
                                .iter()
                                .filter_map(|b| {
                                    if let omegon_traits::ContentBlock::Text { text } = b {
                                        Some(text.as_str())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                        }
                        Err(e) => text = format!("Persona switch failed: {e}"),
                    }
                }
                let _ = response_tx.send(WorkerResponse {
                    text,
                    error: None,
                    cancelled: false,
                });
            }

            WorkerRequest::ConnectMcpServers { servers } => {
                let server_map: std::collections::HashMap<
                    String,
                    crate::plugins::mcp::McpServerConfig,
                > = servers.into_iter().collect();
                if !server_map.is_empty() {
                    match crate::plugins::mcp::McpFeature::connect(
                        "acp-client",
                        &server_map,
                        Some(secrets.as_ref()),
                    )
                    .await
                    {
                        Ok(mcp_feature) => {
                            let tool_count = mcp_feature.tools().len();
                            if tool_count > 0 {
                                bus.register(Box::new(mcp_feature));
                                bus.finalize();
                                tracing::info!(
                                    tools = tool_count,
                                    "ACP client MCP servers connected"
                                );
                                let _ = event_tx.send(WorkerEvent::StatusUpdate(format!(
                                    "Connected {tool_count} tools from client MCP servers"
                                )));
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to connect client MCP servers");
                            let _ = event_tx.send(WorkerEvent::StatusUpdate(format!(
                                "MCP server connection failed: {e}"
                            )));
                        }
                    }
                }
            }

            WorkerRequest::Shutdown => break,
        }
    }

    tracing::info!("ACP worker shutting down");
}

/// Handle a control request (slash command equivalent) in the worker context.
/// Returns the response text. This gives ACP the same surface as the TUI.
async fn handle_control_request(
    command: &str,
    conversation: &crate::conversation::ConversationState,
    shared_settings: &crate::settings::SharedSettings,
    secrets: &std::sync::Arc<omegon_secrets::SecretsManager>,
    workspace_ctx: crate::workspace::control::WorkspaceControlContext<'_>,
    bus: &mut crate::bus::EventBus,
    dangerously_bypass_permissions: bool,
) -> String {
    let cwd = workspace_ctx.cwd;
    let parts: Vec<&str> = command.splitn(2, char::is_whitespace).collect();
    let cmd = parts[0];
    let args = parts.get(1).unwrap_or(&"").trim();

    match cmd {
        "stats" => {
            let settings = shared_settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let projection = crate::surfaces::diagnostics::SessionStatsProjection {
                version: crate::surfaces::diagnostics::DIAGNOSTIC_PROJECTION_VERSION,
                turns: conversation.turn_count() as u32,
                tool_calls: None,
                model: settings.model,
                thinking: settings.thinking.as_str().to_string(),
                posture: settings.posture.effective.as_str().to_string(),
                estimated_context_tokens: conversation.estimate_tokens(),
                context_window: settings.context_window,
                max_turns: settings.max_turns,
                persona: None,
                tone: None,
                authenticated_providers: None,
                provider_count: None,
                mcp_servers: None,
                memory_available: None,
                cleave_available: None,
            };
            projection.render_markdown()
        }

        "max_turns" => {
            if args.is_empty() {
                let max = shared_settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .max_turns;
                format!("Max turns: {max}")
            } else if let Ok(n) = args.parse::<u32>() {
                let n = n.clamp(1, 500);
                if let Ok(mut s) = shared_settings.lock() {
                    s.max_turns = n;
                }
                format!("Max turns set to {n}")
            } else {
                "Usage: max_turns <number>".into()
            }
        }

        "persona_list" => {
            let (personas, tones) = crate::plugins::persona_loader::scan_available();
            let mut out = String::new();
            if personas.is_empty() && tones.is_empty() {
                out.push_str("No personas or tones found.");
            } else {
                if !personas.is_empty() {
                    out.push_str(&format!("Personas ({}):\n", personas.len()));
                    for p in &personas {
                        out.push_str(&format!("  {} — {}\n", p.name, p.description));
                    }
                }
                if !tones.is_empty() {
                    out.push_str(&format!("\nTones ({}):\n", tones.len()));
                    for t in &tones {
                        out.push_str(&format!("  {} — {}\n", t.name, t.description));
                    }
                }
            }
            out
        }

        "persona_switch" => {
            if args.is_empty() {
                "Usage: persona_switch <name|off>".into()
            } else {
                // Handled async in the worker loop — return marker
                format!("__async_persona_switch:{args}")
            }
        }

        "profile_view" => {
            let settings = shared_settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let profile = crate::settings::Profile::load(cwd);
            serde_json::json!({
                "live": {
                    "model": settings.model,
                    "thinkingLevel": settings.thinking.as_str(),
                    "posture": settings.posture.effective.as_str(),
                    "contextWindow": settings.context_window,
                    "maxTurns": settings.max_turns,
                },
                "profile": profile,
            })
            .to_string()
        }

        "profile_capture" => {
            let settings = shared_settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let mut profile = crate::settings::Profile::load(cwd);
            profile.capture_from(&settings);
            match profile.save(cwd) {
                Ok(()) => "Profile captured from live ACP runtime.".into(),
                Err(e) => format!("failed to save profile: {e}"),
            }
        }

        "profile_apply" => {
            let profile = crate::settings::Profile::load(cwd);
            if let Ok(mut settings) = shared_settings.lock() {
                profile.apply_to_with_posture(&mut settings, cwd);
                let slim = settings.is_slim();
                let disabled = settings.posture_disabled_tools.clone();
                let enabled = settings.posture_enabled_tools.clone();
                drop(settings);
                bus.apply_operator_tool_profile(slim, &disabled, &enabled);
                "Profile applied to ACP runtime. Integration and extension load policy changes take effect on next startup.".into()
            } else {
                "failed to update settings".into()
            }
        }

        "profile_mqtt" => {
            let mut profile = crate::settings::Profile::load(cwd);
            match args {
                "on" | "enable" | "true" => profile.integrations.mqtt.enabled = Some(true),
                "off" | "disable" | "false" => profile.integrations.mqtt.enabled = Some(false),
                "" | "status" => {
                    return match profile.integrations.mqtt.enabled {
                        Some(true) => "MQTT bridge profile default: enabled".into(),
                        Some(false) => "MQTT bridge profile default: disabled".into(),
                        None => "MQTT bridge profile default: unset (disabled by default)".into(),
                    };
                }
                _ => return "Usage: profile_mqtt [on|off|status]".into(),
            }
            match profile.save(cwd) {
                Ok(()) => {
                    "MQTT bridge profile default updated. Takes effect on next startup.".into()
                }
                Err(e) => format!("failed to save profile: {e}"),
            }
        }

        "profile_extension_allow" | "profile_extension_deny" => {
            if args.is_empty() {
                return format!("Usage: {cmd} <name>");
            }
            let mut profile = crate::settings::Profile::load(cwd);
            let name = args.to_string();
            if cmd == "profile_extension_allow" {
                profile
                    .extensions
                    .disabled
                    .retain(|v| !v.eq_ignore_ascii_case(&name));
                if !profile
                    .extensions
                    .enabled
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(&name))
                {
                    profile.extensions.enabled.push(name);
                }
            } else {
                profile
                    .extensions
                    .enabled
                    .retain(|v| !v.eq_ignore_ascii_case(&name));
                if !profile
                    .extensions
                    .disabled
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(&name))
                {
                    profile.extensions.disabled.push(name);
                }
            }
            match profile.save(cwd) {
                Ok(()) => "Extension profile policy updated. Takes effect on next startup.".into(),
                Err(e) => format!("failed to save profile: {e}"),
            }
        }

        "profile_extension_clear" => {
            let mut profile = crate::settings::Profile::load(cwd);
            profile.extensions.enabled.clear();
            profile.extensions.disabled.clear();
            match profile.save(cwd) {
                Ok(()) => "Extension profile policy cleared.".into(),
                Err(e) => format!("failed to save profile: {e}"),
            }
        }

        "profile_persona" | "profile_tone" => {
            let mut profile = crate::settings::Profile::load(cwd);
            let value =
                (!args.is_empty() && args != "off" && args != "clear").then(|| args.to_string());
            if cmd == "profile_persona" {
                profile.persona = value;
            } else {
                profile.tone = value;
            }
            match profile.save(cwd) {
                Ok(()) => "Profile default updated.".into(),
                Err(e) => format!("failed to save profile: {e}"),
            }
        }

        "context_status" => {
            let est = conversation.estimate_tokens();
            let window = shared_settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .context_window;
            let usage_pct = if window > 0 {
                (est as f64 / window as f64) * 100.0
            } else {
                0.0
            };
            format!("Context: ~{est} tokens ({usage_pct:.0}% of {window})")
        }

        "context_class" => {
            if args.is_empty() {
                let settings = shared_settings.lock().unwrap_or_else(|e| e.into_inner());
                format!("Context class: {:?}", settings.context_class)
            } else {
                "Context class changes require restart. Set in profile.json.".to_string()
            }
        }

        "runtime_mode" => {
            if args.is_empty() {
                let slim = shared_settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_slim();
                format!("Runtime mode: {}", if slim { "slim" } else { "standard" })
            } else {
                "Runtime mode changes require restart.".to_string()
            }
        }

        "secrets_view" => {
            let recipes = secrets.list_recipes();
            if recipes.is_empty() {
                "No secrets configured".into()
            } else {
                let mut out = String::from("Configured secrets:\n");
                for (name, recipe) in &recipes {
                    out.push_str(&format!("  {name}: {recipe}\n"));
                }
                out
            }
        }

        // ── Notes ──────────────────────────────────────────
        "note_add" => {
            if args.is_empty() {
                "Usage: note_add <text>".into()
            } else {
                let notes_path = cwd.join(".omegon/notes.md");
                if let Some(parent) = notes_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
                let entry = format!("- [{timestamp}] {args}\n");
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&notes_path)
                    .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()))
                {
                    Ok(()) => "Noted.".to_string(),
                    Err(e) => format!("Failed to save note: {e}"),
                }
            }
        }

        "notes_view" => {
            let notes_path = cwd.join(".omegon/notes.md");
            match std::fs::read_to_string(&notes_path) {
                Ok(content) if !content.trim().is_empty() => {
                    let count = content.lines().filter(|l| l.starts_with("- [")).count();
                    format!("Notes ({count}):\n\n{content}")
                }
                _ => "No notes.".into(),
            }
        }

        "notes_clear" => {
            let notes_path = cwd.join(".omegon/notes.md");
            let _ = std::fs::remove_file(&notes_path);
            "Notes cleared.".into()
        }

        // ── Workspace ────────────────────────────────────
        "workspace_status" => workspace_response_text(
            crate::workspace::control::workspace_status_view_response(&workspace_ctx),
        ),
        "workspace_list" => workspace_response_text(
            crate::workspace::control::workspace_list_view_response(&workspace_ctx),
        ),
        "workspace_new" => {
            if args.is_empty() {
                "Usage: workspace_new <label>".into()
            } else {
                workspace_response_text(crate::workspace::control::workspace_new_response(
                    &workspace_ctx,
                    args,
                ))
            }
        }
        "workspace_destroy" => {
            if args.is_empty() {
                "Usage: workspace_destroy <workspace_id|label>".into()
            } else {
                workspace_response_text(crate::workspace::control::workspace_destroy_response(
                    &workspace_ctx,
                    args,
                ))
            }
        }
        "workspace_adopt" => workspace_response_text(
            crate::workspace::control::workspace_adopt_response(&workspace_ctx),
        ),
        "workspace_release" => workspace_response_text(
            crate::workspace::control::workspace_release_response(&workspace_ctx),
        ),
        "workspace_archive" => workspace_response_text(
            crate::workspace::control::workspace_archive_response(&workspace_ctx),
        ),
        "workspace_prune" => workspace_response_text(
            crate::workspace::control::workspace_prune_response(&workspace_ctx),
        ),
        "workspace_bind_milestone" => {
            if args.is_empty() {
                "Usage: workspace_bind_milestone <milestone_id>".into()
            } else {
                workspace_response_text(crate::workspace::control::workspace_bind_milestone_response(
                    &workspace_ctx,
                    args,
                ))
            }
        }
        "workspace_bind_node" => {
            if args.is_empty() {
                "Usage: workspace_bind_node <design_node_id>".into()
            } else {
                workspace_response_text(crate::workspace::control::workspace_bind_node_response(
                    &workspace_ctx,
                    args,
                ))
            }
        }
        "workspace_bind_clear" => workspace_response_text(
            crate::workspace::control::workspace_bind_clear_response(&workspace_ctx),
        ),
        "workspace_role" => workspace_response_text(
            crate::workspace::control::workspace_role_view_response(&workspace_ctx),
        ),
        "workspace_role_set" => match crate::workspace::types::WorkspaceRole::parse(args) {
            Some(role) => workspace_response_text(
                crate::workspace::control::workspace_role_set_response(
                    &workspace_ctx,
                    role,
                ),
            ),
            None => "Usage: workspace_role_set <primary|feature|cleave-child|benchmark|release|exploratory|read-only>".into(),
        },
        "workspace_role_clear" => workspace_response_text(
            crate::workspace::control::workspace_role_clear_response(&workspace_ctx),
        ),
        "workspace_kind" => workspace_response_text(
            crate::workspace::control::workspace_kind_view_response(&workspace_ctx),
        ),
        "workspace_kind_set" => match crate::workspace::types::WorkspaceKind::parse(args) {
            Some(kind) => workspace_response_text(
                crate::workspace::control::workspace_kind_set_response(
                    &workspace_ctx,
                    kind,
                ),
            ),
            None => "Usage: workspace_kind_set <code|vault|knowledge|spec|mixed|generic>".into(),
        },
        "workspace_kind_clear" => workspace_response_text(
            crate::workspace::control::workspace_kind_clear_response(&workspace_ctx),
        ),

        // ── Design tree ────────────────────────────────
        "tree_view" => match bus.dispatch_command("design", args) {
            omegon_traits::CommandResult::Display(msg) => msg,
            omegon_traits::CommandResult::Handled => "Design tree command handled.".into(),
            omegon_traits::CommandResult::NotHandled => "Design tree not available.".into(),
        },

        "provider_status" => {
            let providers = ["anthropic", "openai", "ollama"];
            let mut lines = Vec::new();
            for p in &providers {
                let info = crate::auth::resolve_with_refresh(p).await;
                let (status, detail) = match info {
                    Some((_, is_oauth)) => {
                        let src = if is_oauth { "oauth" } else { "api_key" };
                        ("authenticated", src.to_string())
                    }
                    None => {
                        let creds = crate::auth::read_credentials(crate::auth::auth_json_key(p));
                        match creds {
                            Some(c) if c.is_expired() => {
                                ("expired", "token expired — /login to refresh".into())
                            }
                            Some(_) => ("error", "credentials found but resolution failed".into()),
                            None => ("missing", "not configured".into()),
                        }
                    }
                };
                lines.push(format!("{p}:{status}:{detail}"));
            }
            // Check Ollama separately
            let ollama_ok = std::net::TcpStream::connect_timeout(
                &"127.0.0.1:11434".parse().unwrap(),
                std::time::Duration::from_millis(500),
            )
            .is_ok();
            if ollama_ok {
                lines.push("ollama:running:localhost:11434".into());
            } else {
                lines.push("ollama:unavailable:not running".into());
            }
            lines.join("\n")
        }

        "vault_status" => {
            "Vault status requires interactive terminal. Use `omegon vault status` in a shell."
                .into()
        }

        "auth_status" => {
            "Auth status requires interactive terminal. Use `omegon auth status` in a shell.".into()
        }

        _ => handle_registered_acp_command(bus, cmd, args, dangerously_bypass_permissions)
            .unwrap_or_else(|| format!("Unknown control request: {command}")),
    }
}

pub(crate) fn handle_registered_acp_command(
    bus: &mut crate::bus::EventBus,
    name: &str,
    args: &str,
    dangerously_bypass_permissions: bool,
) -> Option<String> {
    let definition = bus
        .command_definitions()
        .iter()
        .map(|(_, definition)| definition)
        .find(|definition| definition.name == name)?;

    if !definition.availability.acp {
        return Some(format!("Command /{name} is not available over ACP."));
    }
    if definition.safety.requires_confirmation && !dangerously_bypass_permissions {
        return Some(format!(
            "Command /{name} requires interactive confirmation and is unavailable over ACP."
        ));
    }

    match bus.dispatch_command(name, args) {
        omegon_traits::CommandResult::Display(text) => Some(text),
        omegon_traits::CommandResult::Handled => Some(format!("/{name} handled.")),
        omegon_traits::CommandResult::NotHandled => Some(format!(
            "Command /{name} was registered but did not handle the request."
        )),
    }
}

fn workspace_ctx<'a>(
    cwd: &'a std::path::Path,
    session_id: &'a str,
    instance_id: &'a str,
) -> crate::workspace::control::WorkspaceControlContext<'a> {
    crate::workspace::control::WorkspaceControlContext::new(cwd, session_id, instance_id)
        .with_owner_agent_id("omegon-acp")
}

fn workspace_response_text(response: omegon_traits::SlashCommandResponse) -> String {
    response.output.unwrap_or_else(|| {
        if response.accepted {
            "Workspace command accepted.".into()
        } else {
            "Workspace command rejected.".into()
        }
    })
}

/// Convert raw agent loop errors into actionable messages for the user.
fn humanize_agent_error(raw: &str, model: &str) -> String {
    let lower = raw.to_lowercase();

    if lower.contains("connection closed")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
    {
        let provider = if model.contains("claude") || model.starts_with("anthropic:") {
            "Anthropic"
        } else if model.contains("gpt") || model.starts_with("openai:") {
            "OpenAI"
        } else if model.contains("llama")
            || model.contains("qwen")
            || model.contains("mistral")
            || model.starts_with("ollama:")
        {
            return format!(
                "Cannot connect to Ollama. Make sure it's running: `ollama serve`\n\
                 Model: {model}"
            );
        } else {
            "The provider"
        };
        return format!(
            "{provider} connection failed — your token may be expired.\n\
             Use /login in the agent panel to re-authenticate, \
             or switch to a local model via the model dropdown.\n\
             Model: {model}"
        );
    }

    if lower.contains("401") || lower.contains("unauthorized") || lower.contains("invalid.*key") {
        return format!(
            "Authentication failed — your API key or token is invalid or expired.\n\
             Use /login to re-authenticate.\n\
             Model: {model}"
        );
    }

    if lower.contains("429") || lower.contains("rate limit") || lower.contains("quota") {
        return format!(
            "Rate limited — you've exceeded the API quota for this provider.\n\
             Wait a few minutes or switch to a different model.\n\
             Model: {model}"
        );
    }

    if lower.contains("timeout") || lower.contains("timed out") {
        return format!(
            "Request timed out — the model took too long to respond.\n\
             Try a smaller model or check your connection.\n\
             Model: {model}"
        );
    }

    if lower.contains("no llm provider") || lower.contains("no executable provider") {
        return format!(
            "No provider available for {model}.\n\
             Configure an API key with /login, or start Ollama: `ollama serve`"
        );
    }

    // Fallback — still include the model for context
    format!("{raw}\n\nModel: {model}")
}

#[cfg(test)]
mod command_safety_tests {
    use super::*;
    use omegon_traits::{
        CommandAvailability, CommandDefinition, CommandResult, CommandSafety, CommandSafetyClass,
        Feature,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct TestCommandFeature {
        definition: CommandDefinition,
        handled: Arc<AtomicBool>,
    }

    impl Feature for TestCommandFeature {
        fn name(&self) -> &str {
            "test-command"
        }

        fn commands(&self) -> Vec<CommandDefinition> {
            vec![self.definition.clone()]
        }

        fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
            if name == self.definition.name {
                self.handled.store(true, Ordering::SeqCst);
                CommandResult::Display(format!("handled {args}"))
            } else {
                CommandResult::NotHandled
            }
        }
    }

    fn bus_with_command(
        availability: CommandAvailability,
        requires_confirmation: bool,
    ) -> (crate::bus::EventBus, Arc<AtomicBool>) {
        let handled = Arc::new(AtomicBool::new(false));
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(TestCommandFeature {
            definition: CommandDefinition {
                name: "unsafe_test".into(),
                description: "test command".into(),
                subcommands: vec![],
                availability,
                safety: CommandSafety {
                    class: CommandSafetyClass::StateChanging,
                    requires_confirmation,
                    prompt_injection_sensitive: false,
                },
            },
            handled: handled.clone(),
        }));
        bus.finalize();
        (bus, handled)
    }

    #[test]
    fn acp_registered_command_requires_confirmation_without_bypass() {
        let (mut bus, handled) = bus_with_command(CommandAvailability::ALL, true);

        let response = handle_registered_acp_command(&mut bus, "unsafe_test", "args", false)
            .expect("registered command response");

        assert!(
            response.contains("requires interactive confirmation"),
            "{response}"
        );
        assert!(!handled.load(Ordering::SeqCst));
    }

    #[test]
    fn acp_registered_command_bypass_allows_confirmation_required_command() {
        let (mut bus, handled) = bus_with_command(CommandAvailability::ALL, true);

        let response = handle_registered_acp_command(&mut bus, "unsafe_test", "args", true)
            .expect("registered command response");

        assert_eq!(response, "handled args");
        assert!(handled.load(Ordering::SeqCst));
    }

    #[test]
    fn acp_registered_command_availability_is_not_bypassed() {
        let (mut bus, handled) = bus_with_command(
            CommandAvailability {
                tui: true,
                cli: true,
                acp: false,
            },
            true,
        );

        let response = handle_registered_acp_command(&mut bus, "unsafe_test", "args", true)
            .expect("registered command response");

        assert!(response.contains("not available over ACP"), "{response}");
        assert!(!handled.load(Ordering::SeqCst));
    }

    #[test]
    fn acp_registered_prompt_command_dispatches_through_command_registry() {
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::prompt::PromptFeature::new()));
        bus.finalize();

        let response = handle_registered_acp_command(&mut bus, "prompt", "list", false)
            .expect("registered prompt command response");

        assert!(response.contains("Prompt library"), "{response}");
        assert!(response.contains("init"), "{response}");
    }

    #[test]
    fn acp_registered_subagent_command_dispatches_through_command_registry() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::delegate::DelegateFeature::new(
            temp_dir.path(),
            vec![],
            false,
        )));
        bus.finalize();

        let response = handle_registered_acp_command(&mut bus, "subagent", "status", false)
            .expect("registered subagent command response");

        assert!(response.contains("Subagent / Delegate Tasks"), "{response}");
    }

    #[test]
    fn acp_registered_prompt_command_preview_uses_safety_boundary() {
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::prompt::PromptFeature::new()));
        bus.finalize();

        let response = handle_registered_acp_command(&mut bus, "prompt", "preview init", false)
            .expect("registered prompt preview response");

        assert!(response.contains("Safety:"), "{response}");
        assert!(response.contains("Prompt:"), "{response}");
    }
}
