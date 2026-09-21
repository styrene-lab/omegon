//! ACP worker thread — owns the agent session and processes prompts
//! on a dedicated thread with its own tokio runtime.
//!
//! The ACP I/O thread communicates via channels, keeping the agent loop's
//! `!Send` types isolated while allowing the ACP connection to remain
//! responsive (streaming, cancel, notifications).

use omegon_traits::Feature;
use std::collections::VecDeque;
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
    /// Replace the worker conversation with a persisted Omegon session.
    LoadSession {
        path: PathBuf,
        resume_id: String,
        ack: oneshot::Sender<Result<AcpSessionReplay, String>>,
    },
    /// Atomically replace the idle worker with a fresh authority lineage.
    NewSession {
        ack: oneshot::Sender<Result<String, String>>,
    },
    /// Cancel the current prompt.
    Cancel,
    /// Change the model. `ack` (when provided) fires after shared_settings is
    /// updated so the caller can read the applied state without racing the
    /// channel.
    SetModel {
        value: String,
        ack: Option<oneshot::Sender<Result<(), String>>>,
    },
    /// Change thinking level.
    SetThinking {
        value: String,
        ack: Option<oneshot::Sender<Result<(), String>>>,
    },
    /// Change posture.
    SetPosture {
        value: String,
        ack: Option<oneshot::Sender<Result<(), String>>>,
    },
    /// Change the requested context class.
    SetContextClass {
        value: String,
        ack: oneshot::Sender<Result<(), String>>,
    },
    /// Apply one named profile from the project/user profile registry.
    ApplyProfile {
        id: String,
        scope: Option<String>,
        ack: oneshot::Sender<Result<(), String>>,
    },
    /// Execute a control request (slash command) and return the response.
    /// This gives ACP clients access to every operation the TUI has.
    ControlRequest {
        command: String,
        response_tx: oneshot::Sender<WorkerResponse>,
    },
    /// Execute a normalized control request directly, avoiding reparsing when
    /// the ACP transport has already resolved the canonical command.
    CanonicalControlRequest {
        request: crate::operator_commands::InterfaceControlRequest,
        response_tx: oneshot::Sender<WorkerResponse>,
    },
    /// Invoke one extension-owned ACP transport route through kernel admission.
    ExtensionRpcCall {
        extension: String,
        method: String,
        params: serde_json::Value,
        response_tx: oneshot::Sender<Result<serde_json::Value, String>>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanUpdateDisposition {
    Replace,
    Merge,
    Retain,
    Clear,
}

/// Event streamed from the worker during prompt execution.
#[derive(Clone, Debug)]
pub enum WorkerEvent {
    SessionActivity(Box<crate::surfaces::session_activity::TransportActivityProjectionV1>),
    TextChunk(String),
    ThinkingChunk(String),
    ToolStart {
        id: String,
        name: String,
        execution_origin: omegon_traits::ToolExecutionOrigin,
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
        disposition: PlanUpdateDisposition,
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

pub struct AcpReplayMessage {
    pub role: AcpReplayRole,
    pub text: String,
}

pub struct AcpSessionReplay {
    pub status: String,
    pub frontier_sequence: u64,
    pub messages: Vec<AcpReplayMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpReplayRole {
    User,
    Agent,
}

pub fn project_replay_messages(
    view: &crate::session_consumers::SemanticSessionView,
) -> AcpSessionReplay {
    use crate::surfaces::session::{FrontendConversationKindV1, TranscriptContentV1};

    let mut messages = Vec::new();
    if let Some(snapshot) = view.frontend.as_ref() {
        for item in &snapshot.conversation {
            match item.kind {
                FrontendConversationKindV1::CommittedMessage => {
                    let Some(message) = item.transcript_message.as_ref() else {
                        continue;
                    };
                    match &message.content {
                        TranscriptContentV1::Prompt { prompt_content } => {
                            messages.push(AcpReplayMessage {
                                role: AcpReplayRole::User,
                                text: prompt_content.text.clone(),
                            });
                        }
                        TranscriptContentV1::Assistant { assistant_channels } => {
                            let mut text = String::new();
                            for channel in assistant_channels {
                                if channel.content_kind
                                    != crate::session_authority::AssistantContentKind::Text
                                {
                                    continue;
                                }
                                for content_ref in &channel.chunk_refs {
                                    if let Ok(chunk) = view.content_text(content_ref) {
                                        text.push_str(&chunk);
                                    }
                                }
                            }
                            if !text.is_empty() {
                                messages.push(AcpReplayMessage {
                                    role: AcpReplayRole::Agent,
                                    text,
                                });
                            }
                        }
                        TranscriptContentV1::ToolResult { .. } => {}
                    }
                }
                FrontendConversationKindV1::AssistantEvidence => {
                    if item.content_kind
                        != Some(crate::session_authority::AssistantContentKind::Text)
                    {
                        continue;
                    }
                    if let Some(content_ref) = item.content_ref.as_ref()
                        && let Ok(text) = view.content_text(content_ref)
                    {
                        messages.push(AcpReplayMessage {
                            role: AcpReplayRole::Agent,
                            text: format!("[durable {:?} assistant evidence]\n{text}", item.status),
                        });
                    }
                }
            }
        }
    }
    AcpSessionReplay {
        status: view.status.label().into(),
        frontier_sequence: view.frontier_sequence,
        messages,
    }
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
    pub work_snapshot_rx:
        tokio::sync::oneshot::Receiver<Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>>,
    pub lifecycle_binding_rx:
        tokio::sync::oneshot::Receiver<crate::lifecycle_service::LifecycleBinding>,
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
    let (work_snapshot_tx, work_snapshot_rx) = tokio::sync::oneshot::channel();
    let (lifecycle_binding_tx, lifecycle_binding_rx) = tokio::sync::oneshot::channel();

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
                    work_snapshot_tx,
                    lifecycle_binding_tx,
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
        work_snapshot_rx,
        lifecycle_binding_rx,
    }
}

async fn await_turn_with_control<F>(
    turn: F,
    request_rx: &mut mpsc::Receiver<WorkerRequest>,
    event_tx: &tokio::sync::broadcast::Sender<WorkerEvent>,
    cancel: &CancellationToken,
    supervisor: &mut crate::runtime_supervisor::InteractiveRuntimeSupervisor,
    deferred_requests: &mut VecDeque<WorkerRequest>,
    request_channel_open: &mut bool,
) -> F::Output
where
    F: std::future::Future,
{
    tokio::pin!(turn);
    loop {
        tokio::select! {
            result = &mut turn => break result,
            request = request_rx.recv(), if *request_channel_open => match request {
                Some(WorkerRequest::Cancel) => {
                    let admission = supervisor.current_identity().map(|identity| {
                        supervisor.request_durable_interrupt(
                            identity,
                            crate::runtime_prompt::RuntimeActor::from_submission(
                                "acp-client".into(),
                                "acp",
                            ),
                            crate::runtime_prompt::ControlSurface::Acp,
                        )
                    });
                    match admission.transpose() {
                        Ok(Some(crate::runtime_turn::InterruptAdmission::Admitted | crate::runtime_turn::InterruptAdmission::Duplicate)) => {
                            cancel.cancel();
                            let _ = event_tx.send(WorkerEvent::TurnCancelled {
                                reason: "operator_cancelled".to_string(),
                            });
                        }
                        Ok(_) => {}
                        Err(error) => {
                            let _ = event_tx.send(WorkerEvent::StatusUpdate(format!(
                                "Cancellation was not accepted because session authority could not be updated: {error}"
                            )));
                        }
                    }
                }
                Some(request) => deferred_requests.push_back(request),
                None => {
                    *request_channel_open = false;
                    if let Some(identity) = supervisor.current_identity()
                        && supervisor
                            .request_durable_interrupt(
                                identity,
                                crate::runtime_prompt::RuntimeActor::from_submission(
                                    "acp-transport".into(),
                                    "acp",
                                ),
                                crate::runtime_prompt::ControlSurface::Acp,
                            )
                            .is_ok()
                    {
                        cancel.cancel();
                    }
                }
            }
        }
    }
}

async fn finalize_acp_setup_error(agent: &mut crate::setup::AgentSetup, error: anyhow::Error) {
    if let Err(error) = crate::setup::finalize_agent_error::<()>(agent, error).await {
        tracing::error!(%error, "ACP worker setup failed");
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
    work_snapshot_tx: tokio::sync::oneshot::Sender<
        Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>,
    >,
    lifecycle_binding_tx: tokio::sync::oneshot::Sender<crate::lifecycle_service::LifecycleBinding>,
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

    let mut agent_setup = match crate::setup::AgentSetup::new_with_safety_and_mode(
        &cwd,
        None,
        Some(shared_settings.clone()),
        dangerously_bypass_permissions,
        "acp",
    )
    .await
    {
        Ok(mut setup) => {
            setup.workspace_state.lease.owner_agent_id = Some("omegon-acp".into());
            let _ = crate::workspace::runtime::write_workspace_lease(
                &cwd,
                &setup.instance_id,
                &setup.workspace_state.lease,
            );
            setup
        }
        Err(e) => {
            tracing::error!(error = %e, "worker setup failed");
            return;
        }
    };

    let mut session_id = agent_setup.session_id.clone();
    let instance_id = agent_setup.instance_id.clone();
    let composition_generation_id = match agent_setup.bus.composition_generation_id() {
        Some(generation_id) => generation_id.as_str().to_string(),
        None => {
            finalize_acp_setup_error(
                &mut agent_setup,
                anyhow::anyhow!("ACP composition was not published"),
            )
            .await;
            return;
        }
    };
    let workspace_id = agent_setup.workspace_state.lease.workspace_id.clone();
    let session_snapshot = match crate::session::sessions_dir(&cwd) {
        Some(directory) => directory.join(format!("{session_id}.json")),
        None => {
            finalize_acp_setup_error(
                &mut agent_setup,
                anyhow::anyhow!("cannot determine ACP session directory"),
            )
            .await;
            return;
        }
    };
    let authority = match crate::session_authority::SessionAuthority::open(
        &session_snapshot,
        &session_id,
        &workspace_id,
        &composition_generation_id,
        crate::session_authority::ActorIdentity {
            principal: "acp-client".into(),
            ingress: "acp".into(),
        },
        &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    ) {
        Ok(authority) => authority,
        Err(error) => {
            finalize_acp_setup_error(
                &mut agent_setup,
                anyhow::anyhow!("failed to open ACP session authority: {error}"),
            )
            .await;
            return;
        }
    };
    let mut supervisor =
        match crate::runtime_supervisor::InteractiveRuntimeSupervisor::with_authority(authority) {
            Ok(supervisor) => supervisor,
            Err(error) => {
                finalize_acp_setup_error(
                    &mut agent_setup,
                    anyhow::anyhow!("failed to restore ACP session supervisor: {error}"),
                )
                .await;
                return;
            }
        };
    match supervisor.withdraw_recovered_prompts() {
        Ok(0) => {}
        Ok(count) => tracing::warn!(
            count,
            "withdrew recovered ACP prompts whose response channels were lost"
        ),
        Err(error) => {
            finalize_acp_setup_error(
                &mut agent_setup,
                anyhow::anyhow!("failed to withdraw orphaned recovered ACP prompts: {error}"),
            )
            .await;
            return;
        }
    }
    let mut bus = agent_setup.bus;
    let work_snapshot = agent_setup.work_snapshot;
    let lifecycle_binding = agent_setup.lifecycle_binding.clone();
    let behavior_policy = agent_setup.behavior_policy;
    let git_binding = agent_setup.git_binding.clone();
    let mut context_manager = agent_setup.context_manager;
    let mut conversation = agent_setup.conversation;
    let secrets = agent_setup.secrets;
    let extension_metadata = agent_setup.extension_metadata.clone();
    let extension_rpc_handles = agent_setup.extension_rpc_handles.clone();
    let mut dynamic_contributions = std::mem::take(&mut agent_setup.dynamic_contributions);
    let mut resume_id: Option<String> = None;
    let mut resume_info = agent_setup.resume_info;

    let _ = secrets_tx.send(secrets.clone());
    let _ = work_snapshot_tx.send(work_snapshot.clone());
    let _ = lifecycle_binding_tx.send(lifecycle_binding);
    if !extension_metadata.is_empty() {
        let _ = event_tx.send(WorkerEvent::ExtensionMetadata(extension_metadata));
    }
    if !extension_rpc_handles.is_empty() {
        let _ = event_tx.send(WorkerEvent::ExtensionHandles(extension_rpc_handles));
    }
    let host_ctx_arc = host_ctx.map(std::sync::Arc::new);
    tracing::info!(model = %model, "ACP worker ready");
    let mut first_prompt = true;
    let mut deferred_requests = VecDeque::new();
    let mut request_channel_open = true;

    // Process requests
    loop {
        let req = match deferred_requests.pop_front() {
            Some(request) => request,
            None if request_channel_open => match request_rx.recv().await {
                Some(request) => request,
                None => break,
            },
            None => break,
        };
        match req {
            WorkerRequest::Prompt { text, response_tx } => {
                let prompt_id = match supervisor.admit_prompt(
                    text,
                    Vec::new(),
                    crate::runtime_prompt::RuntimeActor::from_submission(
                        "acp-client".into(),
                        "acp",
                    ),
                    crate::runtime_prompt::ControlSurface::Acp,
                    crate::operator_commands::PromptMetadata::default(),
                    Some(crate::runtime_prompt::QueueMode::UntilReady),
                ) {
                    Ok(prompt_id) => prompt_id,
                    Err(error) => {
                        let _ = response_tx.send(WorkerResponse {
                            text: String::new(),
                            error: Some(format!("Session authority rejected the prompt: {error}")),
                            cancelled: false,
                        });
                        continue;
                    }
                };
                let active = match supervisor.start_next_turn() {
                    Ok(Some(active)) if active.prompt.id == prompt_id => active,
                    Ok(Some(_)) => {
                        let _ = response_tx.send(WorkerResponse {
                            text: String::new(),
                            error: Some("Recovered queued work must settle before this ACP prompt can start".into()),
                            cancelled: false,
                        });
                        continue;
                    }
                    Ok(None) => {
                        let _ = response_tx.send(WorkerResponse {
                            text: String::new(),
                            error: Some("ACP session is already running a turn".into()),
                            cancelled: false,
                        });
                        continue;
                    }
                    Err(error) => {
                        let _ = response_tx.send(WorkerResponse {
                            text: String::new(),
                            error: Some(format!(
                                "Session authority could not start the turn: {error}"
                            )),
                            cancelled: false,
                        });
                        continue;
                    }
                };
                let active_identity = supervisor
                    .current_identity()
                    .expect("promoted ACP turn has identity");
                if let Some(activity) = supervisor.session_activity_projection() {
                    let _ = event_tx.send(WorkerEvent::SessionActivity(Box::new(
                        activity.for_transport(
                            crate::surfaces::session_activity::ActivityTransport::Acp,
                        ),
                    )));
                }
                let execution_capture = supervisor
                    .active_execution_capture()
                    .expect("promoted ACP turn has an execution capture");
                if first_prompt {
                    first_prompt = false;
                    let title: String = active
                        .prompt
                        .text
                        .chars()
                        .take(80)
                        .collect::<String>()
                        .trim()
                        .to_string();
                    let title = title.lines().next().unwrap_or(&title).trim().to_string();
                    if !title.is_empty() {
                        let _ = event_tx.send(WorkerEvent::SessionTitle(title));
                    }
                }
                conversation.push_user(active.prompt.text.clone());

                // Resolve the model from settings (may have been changed via SetModel)
                let current_model = shared_settings
                    .lock()
                    .ok()
                    .map(|s| s.model.clone())
                    .unwrap_or_else(|| model.clone());

                let bridge = match crate::providers::auto_detect_bridge(&current_model).await {
                    Some(b) => b,
                    None => {
                        if let Err(authority_error) = supervisor.submit_loop_terminal_intent(
                            crate::runtime_turn::LoopTerminalIntent {
                                identity: active_identity,
                                outcome: crate::runtime_turn::RuntimeTurnOutcome::Failed,
                                reason_code: "provider_unavailable".into(),
                            },
                        ) {
                            let _ = response_tx.send(WorkerResponse {
                                text: String::new(),
                                error: Some(format!(
                                    "Provider is unavailable and session authority could not record closure: {authority_error}"
                                )),
                                cancelled: false,
                            });
                            break;
                        }
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
                let event_forwarder = tokio::task::spawn_local(async move {
                    while let Ok(event) = loop_events_rx.recv().await {
                        let worker_event = match event {
                            omegon_traits::AgentEvent::MessageChunk { text, .. } => {
                                Some(WorkerEvent::TextChunk(text))
                            }
                            omegon_traits::AgentEvent::ThinkingChunk { text, .. } => {
                                Some(WorkerEvent::ThinkingChunk(text))
                            }
                            omegon_traits::AgentEvent::ToolStart {
                                id,
                                name,
                                args,
                                execution_origin,
                                ..
                            } => Some(WorkerEvent::ToolStart {
                                id,
                                name,
                                execution_origin,
                                args: if args.is_null() { None } else { Some(args) },
                            }),
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
                            omegon_traits::AgentEvent::ToolUpdate { id, partial, .. } => {
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
                                Some(WorkerEvent::PlanUpdate {
                                    entries,
                                    disposition: PlanUpdateDisposition::Replace,
                                })
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
                                    disposition: PlanUpdateDisposition::Merge,
                                })
                            }
                            omegon_traits::AgentEvent::DecompositionCompleted { .. } => {
                                // Final plan — all entries completed. The ACP
                                // forwarder will emit the full snapshot.
                                None
                            }
                            omegon_traits::AgentEvent::PlanUpdated { projection } => {
                                let entries = crate::acp::plan_entries_from_projection(&projection);
                                let disposition = if entries.is_empty()
                                    && projection.completed_session.is_some()
                                {
                                    PlanUpdateDisposition::Retain
                                } else if entries.is_empty() {
                                    PlanUpdateDisposition::Clear
                                } else {
                                    PlanUpdateDisposition::Replace
                                };
                                Some(WorkerEvent::PlanUpdate {
                                    entries,
                                    disposition,
                                })
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

                let cancel = CancellationToken::new();
                let invocation_authority = supervisor.invocation_authority();

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
                    cwd: cwd.clone(),
                    extended_context: false,
                    settings: Some(shared_settings.clone()),
                    force_compact: None,
                    allow_commit_nudge: true,
                    enforce_first_turn_execution_bias: false,
                    skill_phases: Vec::new(),
                    compatibility: crate::loop_driver::LoopCompatibilityBindings {
                        invocation_frontend: host_ctx_arc
                            .clone()
                            .map(crate::loop_permission::LoopInvocationFrontend::acp)
                            .unwrap_or_default(),
                        secrets: Some(secrets.clone()),
                        invocation_scope: crate::invocation_service::InvocationScope {
                            principal: "acp-model".into(),
                            principal_class: omegon_traits::RuntimePrincipalClass::Model,
                            surface: omegon_traits::RuntimeSurface::Model,
                            session_id: Some(session_id.clone()),
                            turn_id: active.authority_turn_id,
                            authority: invocation_authority,
                            tool_budget: None,
                        },
                        drain_late_requests: false,
                        work_snapshot: work_snapshot.clone(),
                        behavior_policy: behavior_policy.clone(),
                        memory_binding: agent_setup.memory_binding.clone(),
                        context_compaction: agent_setup.context_compaction.clone(),
                        ..Default::default()
                    },
                    cancel_keeps_prompt: None,
                };

                let execution = await_turn_with_control(
                    execution_capture.execute(
                        bridge.as_ref(),
                        &mut bus,
                        &mut context_manager,
                        &mut conversation,
                        &loop_events_tx,
                        cancel.clone(),
                        &loop_config,
                    ),
                    &mut request_rx,
                    &event_tx,
                    &cancel,
                    &mut supervisor,
                    &mut deferred_requests,
                    &mut request_channel_open,
                )
                .await;

                let cancelled = cancel.is_cancelled();
                let terminal = execution.terminal;
                let result = execution.result;
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
                let terminal_intent = if cancelled {
                    crate::runtime_turn::LoopTerminalIntent {
                        identity: active_identity,
                        outcome: crate::runtime_turn::RuntimeTurnOutcome::Revoked,
                        reason_code: "loop_cancelled".into(),
                    }
                } else {
                    terminal.into_intent(active_identity)
                };
                bridge.shutdown().await;
                drop(loop_events_tx);
                let _ = event_forwarder.await;
                if let Err(authority_error) =
                    supervisor.submit_loop_terminal_intent(terminal_intent)
                {
                    let _ = response_tx.send(WorkerResponse {
                        text: String::new(),
                        error: Some(format!(
                            "Turn finished locally but session authority could not record closure: {authority_error}"
                        )),
                        cancelled,
                    });
                    break;
                }

                if let Some(activity) = supervisor.session_activity_projection() {
                    let _ = event_tx.send(WorkerEvent::SessionActivity(Box::new(
                        activity.for_transport(
                            crate::surfaces::session_activity::ActivityTransport::Acp,
                        ),
                    )));
                }

                let _ = event_tx.send(WorkerEvent::TurnComplete);

                let response_text = conversation.last_assistant_text().unwrap_or("").to_string();

                // Save session, preserving the canonical ID after a load.
                let _ = crate::session::save_session(&conversation, &cwd, resume_id.as_deref());

                let _ = response_tx.send(WorkerResponse {
                    text: response_text,
                    error,
                    cancelled,
                });
            }

            WorkerRequest::LoadSession {
                path,
                resume_id: requested_id,
                ack,
            } => {
                let result = if !crate::session::is_canonical_session_id(&requested_id) {
                    Err(format!("invalid session id `{requested_id}`"))
                } else {
                    crate::session_replacement::resume_request(&cwd, &path)
                        .and_then(|request| {
                            if request.target_session_id != requested_id {
                                return Err(
                                    crate::session_replacement::SessionReplacementError::Target(
                                        "loaded session identity does not match ACP request".into(),
                                    ),
                                );
                            }
                            let outcome = crate::session_replacement::replace(
                                crate::session_replacement::HostSessionPublication {
                                    supervisor: &mut supervisor,
                                    conversation: &mut conversation,
                                    displayed_session_id: &mut session_id,
                                    resume_info: &mut resume_info,
                                },
                                request,
                                crate::session_replacement::SessionReplacementEnvironment {
                                    cwd: &cwd,
                                    persist_current: true,
                                    workspace_identity: &workspace_id,
                                    runtime_generation_id: &composition_generation_id,
                                    actor: crate::session_authority::ActorIdentity {
                                        principal: "acp-client".into(),
                                        ingress: "acp".into(),
                                    },
                                },
                            )?;
                            crate::session_replacement::emit_canonical_session_start(
                                &mut bus, &cwd, &outcome,
                            );
                            resume_id = Some(session_id.clone());
                            first_prompt = false;
                            let target = crate::session_consumers::SessionViewTarget {
                                snapshot: path.clone(),
                                session_id: session_id.clone(),
                                stream_id: Some(outcome.projection.stream_id),
                                generation: outcome.host_generation,
                                kind: crate::session_consumers::SessionViewKind::Resume,
                            };
                            Ok(
                                match crate::session_consumers::SemanticSessionView::load(&target) {
                                    Ok(view) => project_replay_messages(&view),
                                    Err(error) => AcpSessionReplay {
                                        status: format!("semantic history unavailable: {error}"),
                                        frontier_sequence: outcome.projection.last_sequence,
                                        messages: Vec::new(),
                                    },
                                },
                            )
                        })
                        .map_err(|error| error.to_string())
                };
                let _ = ack.send(result);
            }

            WorkerRequest::NewSession { ack } => {
                let result = crate::session_replacement::fresh_request(
                    crate::session_replacement::SessionReplacementKind::New,
                    &cwd,
                )
                .and_then(|request| {
                    let outcome = crate::session_replacement::replace(
                        crate::session_replacement::HostSessionPublication {
                            supervisor: &mut supervisor,
                            conversation: &mut conversation,
                            displayed_session_id: &mut session_id,
                            resume_info: &mut resume_info,
                        },
                        request,
                        crate::session_replacement::SessionReplacementEnvironment {
                            cwd: &cwd,
                            persist_current: true,
                            workspace_identity: &workspace_id,
                            runtime_generation_id: &composition_generation_id,
                            actor: crate::session_authority::ActorIdentity {
                                principal: "acp-client".into(),
                                ingress: "acp".into(),
                            },
                        },
                    )?;
                    crate::session_replacement::emit_canonical_session_start(
                        &mut bus, &cwd, &outcome,
                    );
                    resume_id = Some(session_id.clone());
                    first_prompt = true;
                    Ok(session_id.clone())
                })
                .map_err(|error| error.to_string());
                let _ = ack.send(result);
            }

            WorkerRequest::Cancel => {
                tracing::debug!("ACP cancel ignored while the session is idle");
            }

            WorkerRequest::SetModel { value, ack } => {
                let result =
                    crate::session_settings_commands::set_model(&shared_settings, &cwd, &value);
                if let Some(tx) = ack {
                    let _ = tx.send(result);
                } else if let Err(error) = result {
                    tracing::warn!(%error, "failed to persist ACP model setting");
                }
            }

            WorkerRequest::SetThinking { value, ack } => {
                let result =
                    crate::session_settings_commands::set_thinking(&shared_settings, &cwd, &value);
                if let Some(tx) = ack {
                    let _ = tx.send(result);
                } else if let Err(error) = result {
                    tracing::warn!(%error, "failed to persist ACP thinking setting");
                }
            }

            WorkerRequest::SetPosture { value, ack } => {
                let result =
                    crate::session_settings_commands::set_posture(&shared_settings, &cwd, &value);
                if let Some(tx) = ack {
                    let _ = tx.send(result);
                } else if let Err(error) = result {
                    tracing::warn!(%error, "failed to persist ACP posture setting");
                }
            }

            WorkerRequest::SetContextClass { value, ack } => {
                let result = crate::session_settings_commands::set_context_class(
                    &shared_settings,
                    &cwd,
                    &value,
                );
                let _ = ack.send(result);
            }

            WorkerRequest::ApplyProfile { id, scope, ack } => {
                let result = crate::session_settings_commands::apply_profile(
                    &shared_settings,
                    &cwd,
                    crate::settings::ActiveProfileSelection { id, scope },
                );
                let _ = ack.send(result);
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
                    workspace_ctx(&cwd, &session_id, &instance_id).with_git(&git_binding),
                    &mut bus,
                    dangerously_bypass_permissions,
                )
                .await;
                // Persona switch needs async bus.execute_tool — handle the marker
                if let Some(name) = text.strip_prefix("__async_persona_switch:") {
                    let name = name.to_string();
                    let cancel = CancellationToken::new();
                    let args = serde_json::json!({ "name": name });
                    let call_id = format!("acp-persona-switch:{}", uuid::Uuid::new_v4());
                    let scope = crate::invocation_service::InvocationScope {
                        principal: "kernel:acp-persona-switch".into(),
                        principal_class: omegon_traits::RuntimePrincipalClass::Internal,
                        surface: omegon_traits::RuntimeSurface::Internal,
                        ..Default::default()
                    };
                    match bus
                        .invoke_internal(
                            crate::tool_registry::persona::SWITCH_PERSONA,
                            &call_id,
                            args,
                            cancel,
                            scope,
                        )
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

            WorkerRequest::CanonicalControlRequest {
                request,
                response_tx,
            } => {
                if matches!(
                    &request,
                    crate::operator_commands::InterfaceControlRequest::ContextClear
                        | crate::operator_commands::InterfaceControlRequest::NewSession
                ) {
                    let kind = if matches!(
                        &request,
                        crate::operator_commands::InterfaceControlRequest::ContextClear
                    ) {
                        crate::session_replacement::SessionReplacementKind::ContextClear
                    } else {
                        crate::session_replacement::SessionReplacementKind::New
                    };
                    let result = crate::session_replacement::fresh_request(kind, &cwd).and_then(
                        |replacement| {
                            crate::session_replacement::replace(
                                crate::session_replacement::HostSessionPublication {
                                    supervisor: &mut supervisor,
                                    conversation: &mut conversation,
                                    displayed_session_id: &mut session_id,
                                    resume_info: &mut resume_info,
                                },
                                replacement,
                                crate::session_replacement::SessionReplacementEnvironment {
                                    cwd: &cwd,
                                    persist_current: true,
                                    workspace_identity: &workspace_id,
                                    runtime_generation_id: &composition_generation_id,
                                    actor: crate::session_authority::ActorIdentity {
                                        principal: "acp-client".into(),
                                        ingress: "acp".into(),
                                    },
                                },
                            )?;
                            resume_id = Some(session_id.clone());
                            first_prompt = true;
                            Ok(())
                        },
                    );
                    let response = match result {
                        Ok(()) => WorkerResponse {
                            text: if kind
                                == crate::session_replacement::SessionReplacementKind::ContextClear
                            {
                                "Context cleared. Starting fresh conversation.".into()
                            } else {
                                "Started a fresh session.".into()
                            },
                            error: None,
                            cancelled: false,
                        },
                        Err(error) => WorkerResponse {
                            text: String::new(),
                            error: Some(format!("Session was not replaced: {error}")),
                            cancelled: false,
                        },
                    };
                    let _ = response_tx.send(response);
                    continue;
                }
                let dynamic_control = dynamic_contributions.control();
                let dynamic_response = match &request {
                    crate::operator_commands::InterfaceControlRequest::RuntimeDoctor => Some(
                        crate::control_runtime::runtime_doctor_response(Some(&dynamic_control)),
                    ),
                    crate::operator_commands::InterfaceControlRequest::RuntimeExtensionReplace {
                        name,
                    } => Some(
                        crate::control_runtime::runtime_extension_replace_response(
                            Some(&dynamic_control),
                            name,
                        )
                        .await,
                    ),
                    _ => None,
                };
                if let Some(response) = dynamic_response {
                    let _ = response_tx.send(WorkerResponse {
                        text: response.output.unwrap_or_default(),
                        error: (!response.accepted)
                            .then(|| "runtime contribution control failed".into()),
                        cancelled: false,
                    });
                    continue;
                }
                let handles = crate::runtime_state::RuntimeStateHandles::default();
                let text = crate::control_runtime::execute_stateless_control(
                    &request,
                    &shared_settings,
                    &secrets,
                    &cwd,
                    &handles,
                )
                .await
                .and_then(|response| response.output)
                .unwrap_or_else(|| "Unsupported ACP control request".into());
                let _ = response_tx.send(WorkerResponse {
                    text,
                    error: None,
                    cancelled: false,
                });
            }

            WorkerRequest::ExtensionRpcCall {
                extension,
                method,
                params,
                response_tx,
            } => {
                let route = crate::extensions::extension_rpc_invocation_name(&extension);
                let call_id = format!("acp-extension-rpc:{}", uuid::Uuid::new_v4());
                let scope = crate::invocation_service::InvocationScope {
                    principal: "acp-client".into(),
                    principal_class: omegon_traits::RuntimePrincipalClass::Operator,
                    surface: omegon_traits::RuntimeSurface::Acp,
                    ..Default::default()
                };
                let result = bus
                    .invoke_acp(
                        &route,
                        &call_id,
                        serde_json::json!({"method": method, "params": params}),
                        CancellationToken::new(),
                        scope,
                    )
                    .await
                    .map_err(|error| error.to_string());
                let _ = response_tx.send(result);
            }

            WorkerRequest::ConnectMcpServers { servers } => {
                let server_map: std::collections::HashMap<
                    String,
                    crate::plugins::mcp::McpServerConfig,
                > = servers.into_iter().collect();
                if !server_map.is_empty() {
                    let canonical: std::collections::BTreeMap<_, _> = server_map.iter().collect();
                    let static_config = serde_json::to_string(&canonical)
                        .unwrap_or_else(|_| "acp-mcp-invalid-config".into());
                    let preflight = crate::plugins::mcp::dynamic_preflight(
                        "mcp:acp-client",
                        &static_config,
                        &server_map,
                    );
                    let profile = crate::settings::Profile::load(&cwd);
                    let inventory = dynamic_contributions.inventory();
                    let candidate = preflight.and_then(|preflight| inventory.discover(preflight));
                    let candidate_id = candidate
                        .as_ref()
                        .ok()
                        .map(|candidate| candidate.preflight.id.clone());
                    let trust_admission = candidate.and_then(|candidate| {
                        inventory.admit(
                            &candidate,
                            &crate::dynamic_admission::DynamicAdmissionPolicy::from_profile(
                                &profile,
                            ),
                        )
                    });
                    let Ok(trust_admission) = trust_admission else {
                        let _ = event_tx.send(WorkerEvent::StatusUpdate(
                            "ACP client MCP servers denied: add mcp:acp-client to permissions.trustedContributionCode"
                                .into(),
                        ));
                        continue;
                    };
                    match crate::plugins::mcp::McpFeature::connect(
                        "acp-client",
                        &server_map,
                        Some(secrets.as_ref()),
                        trust_admission,
                    )
                    .await
                    {
                        Ok(mcp_feature) => {
                            let tool_count = mcp_feature.tools().len();
                            let mcp_supervisor = mcp_feature.supervisor();
                            let mut candidate_owner =
                                crate::contribution_lifecycle::DynamicContributionGenerationOwner::new(
                                    inventory.clone(),
                                );
                            candidate_owner.own_mcp(mcp_supervisor);
                            if let Some(candidate_id) = &candidate_id {
                                inventory.ready(candidate_id);
                            }
                            candidate_owner.stage();
                            if tool_count > 0 {
                                bus.register(Box::new(mcp_feature));
                                match bus.try_finalize() {
                                    Ok(()) => {
                                        candidate_owner.publish();
                                        dynamic_contributions.absorb_published(candidate_owner);
                                        tracing::info!(
                                            tools = tool_count,
                                            "ACP client MCP servers connected"
                                        );
                                        let _ = event_tx.send(WorkerEvent::StatusUpdate(format!(
                                            "Connected {tool_count} tools from client MCP servers"
                                        )));
                                    }
                                    Err(error) => {
                                        let cleanup_failures =
                                            candidate_owner.reject(error.to_string()).await;
                                        tracing::warn!(%error, "ACP client MCP candidate rejected");
                                        if !cleanup_failures.is_empty() {
                                            tracing::warn!(failures = ?cleanup_failures, "ACP client MCP candidate cleanup degraded");
                                        }
                                        let _ = event_tx.send(WorkerEvent::StatusUpdate(format!(
                                            "Client MCP candidate rejected: {error}"
                                        )));
                                    }
                                }
                            } else {
                                let cleanup_failures = candidate_owner
                                    .reject("MCP candidate declared no callable tools")
                                    .await;
                                if !cleanup_failures.is_empty() {
                                    tracing::warn!(failures = ?cleanup_failures, "empty ACP MCP candidate cleanup degraded");
                                }
                            }
                        }
                        Err(e) => {
                            if let Some(candidate_id) = &candidate_id {
                                inventory.quarantine(candidate_id, e.to_string());
                            }
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

    let managed_cleanup = bus.shutdown_managed_services_strict().await;
    for failure in dynamic_contributions.shutdown().await {
        tracing::warn!(%failure, "ACP dynamic contribution cleanup degraded");
    }
    if let Err(error) = managed_cleanup {
        tracing::error!(%error, "ACP worker managed cleanup failed");
        let _ = event_tx.send(WorkerEvent::StatusUpdate(format!(
            "ACP worker cleanup failed: {error}"
        )));
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
        "version" => format!(
            "Omegon {}\nBuild {}\nBuilt {}\nBinary {}",
            env!("CARGO_PKG_VERSION"),
            env!("OMEGON_GIT_SHA"),
            env!("OMEGON_BUILD_DATE"),
            std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "unknown".into())
        ),
        "status" => {
            let settings = shared_settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let mut harness = crate::status::HarnessStatus::assemble(workspace_ctx.cwd);
            let operating_profile = settings.operating_profile();
            harness.update_routing(
                settings.effective_requested_class().label(),
                settings.thinking.as_str(),
                &harness.capability_grade.clone(),
                operating_profile.posture.effective.display_name(),
                &operating_profile.summary(),
                operating_profile
                    .identity
                    .principal_id
                    .as_deref()
                    .unwrap_or("anonymous"),
                operating_profile
                    .identity
                    .issuer
                    .as_deref()
                    .unwrap_or("unknown"),
                operating_profile
                    .identity
                    .session_kind
                    .as_deref()
                    .unwrap_or("unknown"),
                &operating_profile.authorization.summary(),
            );
            let bootstrap_markdown = crate::bootstrap_projection::render_bootstrap(&harness, false);
            let mut output = crate::surfaces::diagnostics::HarnessStatusProjection::new(
                serde_json::to_value(harness).unwrap_or(serde_json::Value::Null),
                0,
                workspace_ctx.session_id,
                workspace_ctx.instance_id,
                settings.automation_level.as_str(),
                settings.automation_level.summary(),
                bootstrap_markdown,
            )
            .render_markdown();
            if let Some(composition) = bus.composition_diagnostic_projection() {
                output.push_str(&composition.render_markdown());
            }
            output
        }
        "stats" => {
            let settings = shared_settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let projection = crate::surfaces::diagnostics::SessionStatsProjection {
                version: crate::surfaces::diagnostics::DIAGNOSTIC_PROJECTION_VERSION,
                turns: conversation.turn_count(),
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
            crate::plugins::persona_loader::with_available(cwd, |personas, tones| {
                let mut out = String::new();
                if personas.is_empty() && tones.is_empty() {
                    out.push_str("No personas or tones found.");
                } else {
                    if !personas.is_empty() {
                        out.push_str(&format!("Personas ({}):\n", personas.len()));
                        for p in personas {
                            out.push_str(&format!("  {} — {}\n", p.name, p.description));
                        }
                    }
                    if !tones.is_empty() {
                        out.push_str(&format!("\nTones ({}):\n", tones.len()));
                        for t in tones {
                            out.push_str(&format!("  {} — {}\n", t.name, t.description));
                        }
                    }
                }
                out
            })
        }

        "persona_switch" => {
            if args.is_empty() {
                "Usage: persona_switch <name|off>".into()
            } else {
                // Handled async in the worker loop — return marker
                format!("__async_persona_switch:{args}")
            }
        }

        "profile_open" => {
            let selection = if args.trim().is_empty() {
                crate::settings::active_profile_selection(cwd)
            } else {
                let Some(selection) = crate::settings::parse_scoped_profile_selection(args.trim())
                else {
                    return "Usage: profile_open [project|user|built-in:<id>]".into();
                };
                selection
            };
            let registry = crate::settings::ProfileRegistry::discover(cwd);
            let Some(entry) = registry.find_explicit(&selection) else {
                return format!("Profile `{}` was not found.", selection.id);
            };
            let Some(path) = entry.path.as_ref() else {
                return format!(
                    "Profile `built-in:{}` is read-only and has no file. Copy it first with `/profile copy built-in:{} project:<new-id>`.",
                    entry.id, entry.id
                );
            };
            format!(
                "Profile `{}` is at [`{}`](file://{}). Open that path in the editor to modify it; changes apply after re-selecting the profile.",
                selection.id,
                path.display(),
                path.display()
            )
        }

        "profile_copy" => {
            let mut parts = args.split_whitespace();
            let source = parts
                .next()
                .and_then(crate::settings::parse_scoped_profile_selection);
            let target = parts
                .next()
                .and_then(crate::settings::parse_scoped_profile_selection);
            if parts.next().is_some() || source.is_none() || target.is_none() {
                return "Usage: profile_copy <scope:id> <project|user:new-id>".into();
            }
            let source = source.expect("checked");
            let target = target.expect("checked");
            let scope = match target.scope.as_deref() {
                Some("project") => crate::settings::ProfileRegistryScope::Project,
                Some("user") => crate::settings::ProfileRegistryScope::User,
                _ => return "Target scope must be `project` or `user`.".into(),
            };
            match crate::settings::copy_profile(cwd, &source, scope, &target.id) {
                Ok(path) => format!(
                    "Copied profile to [`{}`](file://{}). Select `{}:{}` from the Profile menu after editing.",
                    path.display(),
                    path.display(),
                    target.scope.as_deref().unwrap_or("project"),
                    target.id
                ),
                Err(error) => format!("failed to copy profile: {error}"),
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
            let enabled = match args {
                "on" | "enable" | "true" => Some(true),
                "off" | "disable" | "false" => Some(false),
                "" | "status" => None,
                _ => return "Usage: profile_mqtt [on|off|status]".into(),
            };
            workspace_response_text(
                crate::control_runtime::profile_set_mqtt_response(cwd, enabled).await,
            )
        }

        "profile_extension_allow" | "profile_extension_deny" => {
            if args.is_empty() {
                return format!("Usage: {cmd} <name>");
            }
            let response = if cmd == "profile_extension_allow" {
                crate::control_runtime::profile_extension_allow_response(cwd, args).await
            } else {
                crate::control_runtime::profile_extension_deny_response(cwd, args).await
            };
            workspace_response_text(response)
        }

        "profile_extension_clear" => workspace_response_text(
            crate::control_runtime::profile_extension_clear_response(cwd).await,
        ),

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
                ).await)
            }
        }
        "workspace_destroy" => {
            if args.is_empty() {
                "Usage: workspace_destroy <workspace_id|label>".into()
            } else {
                workspace_response_text(crate::workspace::control::workspace_destroy_response(
                    &workspace_ctx,
                    args,
                ).await)
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
        "tree_view" => match invoke_acp_command(bus, "design", args) {
            omegon_traits::CommandResult::Display(msg) => msg,
            omegon_traits::CommandResult::Handled => "Design tree command handled.".into(),
            omegon_traits::CommandResult::NotHandled => "Design tree not available.".into(),
        },

        "provider_status" => {
            let providers = ["anthropic", "openai", "ollama"];
            let mut lines = Vec::new();
            for p in &providers {
                let info = crate::providers::resolve_api_key_sync(p);
                let (status, detail) = match info {
                    Some((_, is_oauth)) => {
                        let src = if is_oauth { "oauth" } else { "api_key" };
                        ("authenticated", src.to_string())
                    }
                    None => {
                        let creds = crate::auth::read_credentials(crate::auth::auth_json_key(p));
                        match creds {
                            Some(c) if c.is_expired() => {
                                ("expired", "token expired — /connect for setup guidance".into())
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

    match invoke_acp_command(bus, name, args) {
        omegon_traits::CommandResult::Display(text) => Some(text),
        omegon_traits::CommandResult::Handled => Some(format!("/{name} handled.")),
        omegon_traits::CommandResult::NotHandled => Some(format!(
            "Command /{name} was registered but did not handle the request."
        )),
    }
}

fn invoke_acp_command(
    bus: &mut crate::bus::EventBus,
    name: &str,
    args: &str,
) -> omegon_traits::CommandResult {
    let call_id = format!("acp-command:{}", uuid::Uuid::new_v4());
    let scope = crate::invocation_service::InvocationScope {
        principal: "acp-operator".into(),
        principal_class: omegon_traits::RuntimePrincipalClass::Operator,
        surface: omegon_traits::RuntimeSurface::Acp,
        ..Default::default()
    };
    bus.invoke_command(name, &call_id, args, scope, None)
        .unwrap_or_else(|denial| {
            omegon_traits::CommandResult::Display(format!(
                "{}: {}",
                denial.code.as_str(),
                denial.message
            ))
        })
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
             Use /connect for terminal setup instructions, \
             or switch to a local model via the model dropdown.\n\
             Model: {model}"
        );
    }

    if lower.contains("401") || lower.contains("unauthorized") || lower.contains("invalid.*key") {
        return format!(
            "Authentication failed — your API key or token is invalid or expired.\n\
             Use /connect for terminal setup instructions.\n\
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
             Use /connect for terminal setup instructions, or start Ollama: `ollama serve`"
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

    #[tokio::test]
    async fn active_turn_receives_cancel_without_waiting_for_turn_completion() {
        let (request_tx, mut request_rx) = mpsc::channel(1);
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(2);
        let cancel = CancellationToken::new();
        let observed_cancel = cancel.clone();
        let mut deferred = VecDeque::new();
        let mut channel_open = true;
        let mut supervisor = crate::runtime_supervisor::InteractiveRuntimeSupervisor::default();
        supervisor.enqueue_prompt(
            "active".into(),
            Vec::new(),
            crate::runtime_prompt::RuntimeActor::from_submission("acp-client".into(), "acp"),
            crate::runtime_prompt::ControlSurface::Acp,
            crate::operator_commands::PromptMetadata::default(),
            None,
        );
        supervisor.maybe_start_next_turn().unwrap();
        request_tx.send(WorkerRequest::Cancel).await.unwrap();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            await_turn_with_control(
                async move {
                    observed_cancel.cancelled().await;
                    "cancelled"
                },
                &mut request_rx,
                &event_tx,
                &cancel,
                &mut supervisor,
                &mut deferred,
                &mut channel_open,
            ),
        )
        .await
        .expect("cancel must reach an active turn");

        assert_eq!(result, "cancelled");
        assert!(cancel.is_cancelled());
        assert!(deferred.is_empty());
        assert!(matches!(
            event_rx.try_recv(),
            Ok(WorkerEvent::TurnCancelled { reason }) if reason == "operator_cancelled"
        ));
    }

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
                surface: Default::default(),
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
