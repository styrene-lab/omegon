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
    /// Execute a control request (slash command) and return the response.
    /// This gives ACP clients access to every operation the TUI has.
    ControlRequest {
        command: String,
        response_tx: oneshot::Sender<WorkerResponse>,
    },
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
    /// Secrets manager from the worker — arrives asynchronously after setup.
    /// Used by the ACP transport to redact streaming output before emission.
    pub secrets_rx: tokio::sync::oneshot::Receiver<std::sync::Arc<omegon_secrets::SecretsManager>>,
}

/// Spawn the worker thread. Returns a handle for communication.
pub fn spawn_worker(model: String, cwd: PathBuf) -> WorkerHandle {
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
            local.block_on(&rt, worker_loop(model, cwd, worker_settings, request_rx, event_tx, secrets_tx));
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
async fn worker_loop(
    model: String,
    cwd: PathBuf,
    shared_settings: crate::settings::SharedSettings,
    mut request_rx: mpsc::Receiver<WorkerRequest>,
    event_tx: tokio::sync::broadcast::Sender<WorkerEvent>,
    secrets_tx: tokio::sync::oneshot::Sender<std::sync::Arc<omegon_secrets::SecretsManager>>,
) {
    // Set the canonical project root env var so extensions can locate the workspace
    // without depending on embedder-specific names (FLYNT_VAULT, CODEX_VAULT).
    unsafe { std::env::set_var("OMEGON_PROJECT_ROOT", &cwd) };

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

    let _ = secrets_tx.send(secrets.clone());
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

            WorkerRequest::ControlRequest { command, response_tx } => {
                let mut text = handle_control_request(
                    &command,
                    &conversation,
                    &shared_settings,
                    &secrets,
                    &cwd,
                    &mut bus,
                );
                // Persona switch needs async bus.execute_tool — handle the marker
                if let Some(name) = text.strip_prefix("__async_persona_switch:") {
                    let name = name.to_string();
                    let cancel = CancellationToken::new();
                    let args = serde_json::json!({ "name": name });
                    match bus.execute_tool("switch_persona", "ctrl", args, cancel).await {
                        Ok(result) => {
                            text = result.content.iter()
                                .filter_map(|b| if let omegon_traits::ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                                .collect::<Vec<_>>()
                                .join("\n");
                        }
                        Err(e) => text = format!("Persona switch failed: {e}"),
                    }
                }
                let _ = response_tx.send(WorkerResponse { text, error: None });
            }

            WorkerRequest::Shutdown => break,
        }
    }

    tracing::info!("ACP worker shutting down");
}

/// Handle a control request (slash command equivalent) in the worker context.
/// Returns the response text. This gives ACP the same surface as the TUI.
fn handle_control_request(
    command: &str,
    conversation: &crate::conversation::ConversationState,
    shared_settings: &crate::settings::SharedSettings,
    secrets: &std::sync::Arc<omegon_secrets::SecretsManager>,
    cwd: &std::path::Path,
    bus: &mut crate::bus::EventBus,
) -> String {
    let parts: Vec<&str> = command.splitn(2, char::is_whitespace).collect();
    let cmd = parts[0];
    let args = parts.get(1).unwrap_or(&"").trim();

    match cmd {
        "stats" => {
            let settings = shared_settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let est = conversation.estimate_tokens();
            let usage_pct = if settings.context_window > 0 {
                (est as f64 / settings.context_window as f64) * 100.0
            } else {
                0.0
            };
            format!(
                "Model: {}\nThinking: {}\nPosture: {}\nTurns: {}\nContext: ~{} tokens ({:.0}% of {})\nMax turns: {}",
                settings.model,
                settings.thinking.as_str(),
                settings.posture.effective.as_str(),
                conversation.turn_count(),
                est,
                usage_pct,
                settings.context_window,
                settings.max_turns,
            )
        }

        "max_turns" => {
            if args.is_empty() {
                let max = shared_settings.lock().unwrap_or_else(|e| e.into_inner()).max_turns;
                format!("Max turns: {max}")
            } else if let Ok(n) = args.parse::<u32>() {
                let n = n.max(1).min(500);
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
            let settings = shared_settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
            format!(
                "Model: {}\nThinking: {}\nPosture: {}\nContext window: {}\nMax turns: {}",
                settings.model,
                settings.thinking.as_str(),
                settings.posture.effective.as_str(),
                settings.context_window,
                settings.max_turns,
            )
        }

        "context_status" => {
            let est = conversation.estimate_tokens();
            let window = shared_settings.lock().unwrap_or_else(|e| e.into_inner()).context_window;
            let usage_pct = if window > 0 { (est as f64 / window as f64) * 100.0 } else { 0.0 };
            format!("Context: ~{est} tokens ({usage_pct:.0}% of {window})")
        }

        "context_class" => {
            if args.is_empty() {
                let settings = shared_settings.lock().unwrap_or_else(|e| e.into_inner());
                format!("Context class: {:?}", settings.context_class)
            } else {
                format!("Context class changes require restart. Set in profile.json.")
            }
        }

        "runtime_mode" => {
            if args.is_empty() {
                let slim = shared_settings.lock().unwrap_or_else(|e| e.into_inner()).is_slim();
                format!("Runtime mode: {}", if slim { "slim" } else { "standard" })
            } else {
                format!("Runtime mode changes require restart.")
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
                    Ok(()) => format!("Noted."),
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

        "workspace_status" => {
            use crate::workspace::runtime::{read_workspace_lease, read_workspace_registry};
            match read_workspace_lease(cwd).ok().flatten() {
                Some(lease) => {
                    let occupancy = read_workspace_registry(cwd)
                        .ok().flatten()
                        .map(|r| r.workspaces.len())
                        .unwrap_or(1);
                    format!(
                        "Workspace\n  ID: {}\n  Label: {}\n  Path: {}\n  Backend: {}\n  Branch: {}\n  Role: {:?}\n  Kind: {:?}\n  Mutability: {:?}\n  Local views: {}",
                        lease.workspace_id, lease.label, lease.path,
                        lease.backend_kind.as_str(), lease.branch,
                        lease.role, lease.workspace_kind, lease.mutability, occupancy,
                    )
                }
                None => "Workspace: no local runtime metadata yet.".into(),
            }
        }

        "workspace_list" => {
            use crate::workspace::runtime::read_workspace_registry;
            match read_workspace_registry(cwd).ok().flatten() {
                Some(registry) => {
                    let mut out = format!("Workspaces ({}):\n", registry.workspaces.len());
                    for ws in &registry.workspaces {
                        out.push_str(&format!("  {} — {} ({:?})\n", ws.workspace_id, ws.label, ws.role));
                    }
                    out
                }
                None => "No workspace registry found.".into(),
            }
        }

        // ── Design tree ────────────────────────────────

        "tree_view" => {
            match bus.dispatch_command("design", args) {
                omegon_traits::CommandResult::Display(msg) => msg,
                omegon_traits::CommandResult::Handled => "Design tree command handled.".into(),
                omegon_traits::CommandResult::NotHandled => "Design tree not available.".into(),
            }
        }

        "vault_status" => {
            "Vault status requires interactive terminal. Use `omegon vault status` in a shell.".into()
        }

        "auth_status" => {
            "Auth status requires interactive terminal. Use `omegon auth status` in a shell.".into()
        }

        _ => format!("Unknown control request: {command}"),
    }
}
