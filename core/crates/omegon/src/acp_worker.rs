//! ACP worker thread — owns the agent session and processes prompts
//! on a dedicated thread with its own tokio runtime.
//!
//! The ACP I/O thread communicates via channels, keeping the agent loop's
//! `!Send` types isolated while allowing the ACP connection to remain
//! responsive (streaming, cancel, notifications).

use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

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
    SetModel { value: String, ack: Option<oneshot::Sender<()>> },
    /// Change thinking level.
    SetThinking { value: String, ack: Option<oneshot::Sender<()>> },
    /// Change posture.
    SetPosture { value: String, ack: Option<oneshot::Sender<()>> },
    /// Shut down the worker.
    Shutdown,
}

/// Response from the worker to the ACP thread.
pub struct WorkerResponse {
    pub text: String,
    pub error: Option<String>,
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
    },
    /// Status update from the agent loop (e.g., "Loading model into memory…")
    StatusUpdate(String),
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
}

/// Spawn the worker thread. Returns a handle for communication.
pub fn spawn_worker(model: String, cwd: PathBuf) -> WorkerHandle {
    let (request_tx, request_rx) = mpsc::channel::<WorkerRequest>(16);
    let (event_tx, event_rx) = tokio::sync::broadcast::channel::<WorkerEvent>(256);

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
            local.block_on(&rt, worker_loop(model, cwd, worker_settings, request_rx, event_tx));
        })
        .expect("failed to spawn worker thread");

    WorkerHandle {
        request_tx,
        event_rx,
        settings: shared_settings,
    }
}

/// The worker's main loop — runs on a dedicated thread with its own runtime.
async fn worker_loop(
    model: String,
    cwd: PathBuf,
    shared_settings: crate::settings::SharedSettings,
    mut request_rx: mpsc::Receiver<WorkerRequest>,
    event_tx: tokio::sync::broadcast::Sender<WorkerEvent>,
) {
    // Apply profile + initial model to the shared settings provided by spawn_worker.
    // Worker mutates these on SetModel/SetThinking/SetPosture; the ACP transport
    // thread reads them when rebuilding ConfigOption lists.
    if let Ok(mut s) = shared_settings.lock() {
        let profile = crate::settings::Profile::load(&cwd);
        profile.apply_to_with_posture(&mut s, &cwd);
        s.set_model(&model);
    }

    let agent_setup =
        match crate::setup::AgentSetup::new(&cwd, None, Some(shared_settings.clone())).await {
            Ok(setup) => setup,
            Err(e) => {
                tracing::error!(error = %e, "worker setup failed");
                return;
            }
        };

    let mut bus = agent_setup.bus;
    let mut context_manager = agent_setup.context_manager;
    let mut conversation = agent_setup.conversation;
    let secrets = agent_setup.secrets;
    let mut cancel = CancellationToken::new();

    tracing::info!(model = %model, "ACP worker ready");

    // Process requests
    while let Some(req) = request_rx.recv().await {
        match req {
            WorkerRequest::Prompt { text, response_tx } => {
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
                            omegon_traits::AgentEvent::ToolEnd { id, is_error, .. } => {
                                Some(WorkerEvent::ToolEnd {
                                    id,
                                    success: !is_error,
                                })
                            }
                            omegon_traits::AgentEvent::SystemNotification { message } => {
                                Some(WorkerEvent::StatusUpdate(message))
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
                    cwd: cwd.clone(),
                    extended_context: false,
                    settings: Some(shared_settings.clone()),
                    secrets: Some(secrets.clone()),
                    force_compact: None,
                    allow_commit_nudge: true,
                    enforce_first_turn_execution_bias: false,
                    ollama_manager: None,
                    skill_phases: Vec::new(),
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

                let error = match result {
                    Ok(()) => None,
                    Err(e) => Some(e.to_string()),
                };

                // Save session
                let _ = crate::session::save_session(&conversation, &cwd, None);

                let _ = response_tx.send(WorkerResponse {
                    text: response_text,
                    error,
                });
            }

            WorkerRequest::Cancel => {
                cancel.cancel();
            }

            WorkerRequest::SetModel { value, ack } => {
                if let Ok(mut s) = shared_settings.lock() {
                    s.set_model(&value);
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
                }
                if let Some(tx) = ack {
                    let _ = tx.send(());
                }
            }

            WorkerRequest::Shutdown => break,
        }
    }

    tracing::info!("ACP worker shutting down");
}
