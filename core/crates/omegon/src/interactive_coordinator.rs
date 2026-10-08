// Interactive runtime coordination: actors, prompt queue, turn lifecycle, and worker supervision.
//
// Included by the binary composition root so this first relocation preserves existing
// item visibility while removing the coordinator implementation from `main.rs`.

fn runtime_actor_kind_from_via(via: &str) -> RuntimeActorKind {
    match via {
        "tui" => RuntimeActorKind::Tui,
        "ipc" => RuntimeActorKind::IpcClient,
        "websocket" => RuntimeActorKind::WebClient,
        _ => RuntimeActorKind::System,
    }
}

fn control_surface_from_via(via: &str) -> ControlSurface {
    match via {
        "tui" => ControlSurface::Tui,
        "ipc" => ControlSurface::Ipc,
        "websocket" => ControlSurface::WebSocket,
        _ => ControlSurface::Internal,
    }
}

fn interactive_loop_terminal_intent(
    identity: RuntimeTurnIdentity,
    proposal: Option<&crate::loop_driver::LoopTerminalProposal>,
    cancelled: bool,
) -> LoopTerminalIntent {
    if cancelled {
        return LoopTerminalIntent {
            identity,
            outcome: RuntimeTurnOutcome::Revoked,
            reason_code: "loop_cancelled".into(),
        };
    }
    match proposal {
        Some(proposal) => proposal.clone().into_intent(identity),
        None => LoopTerminalIntent {
            identity,
            outcome: RuntimeTurnOutcome::Revoked,
            reason_code: "loop_abandoned".into(),
        }
    }
}

async fn await_interactive_execution<F, T, C>(
    mut execution: std::pin::Pin<&mut F>,
    cancel: &CancellationToken,
    on_cancel: C,
) -> T
where
    F: std::future::Future<Output = T>,
    C: FnOnce(),
{
    tokio::select! {
        result = execution.as_mut() => result,
        _ = cancel.cancelled() => {
            on_cancel();
            execution.await
        }
    }
}

async fn relay_interactive_turn_events(
    mut source: broadcast::Receiver<AgentEvent>,
    destination: broadcast::Sender<AgentEvent>,
    stop: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            event = source.recv() => match event {
                Ok(event) => {
                    let _ = destination.send(event);
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "interactive turn event relay lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

fn handle_runtime_cancel_command(
    runtime: &mut InteractiveRuntimeSupervisor,
    shared_cancel: &operator_commands::SharedCancel,
    events_tx: &broadcast::Sender<AgentEvent>,
    submitted_by: String,
    via: &'static str,
) -> bool {
    let actor = RuntimeActor {
        kind: runtime_actor_kind_from_via(via),
        label: submitted_by,
    };
    let surface = control_surface_from_via(via);
    let admission = match runtime.current_identity() {
        Some(identity) => runtime.request_durable_interrupt(identity, actor, surface),
        None => Ok(InterruptAdmission::Idle),
    };
    match admission {
        Ok(InterruptAdmission::Admitted | InterruptAdmission::Duplicate) => {}
        Ok(InterruptAdmission::Idle | InterruptAdmission::Stale) => {
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: "Cancel requested, but no matching active turn is running.".to_string(),
            });
            return false;
        }
        Err(error) => {
            tracing::error!(%error, "failed to durably admit turn cancellation");
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: format!("Cancel was not accepted because session authority could not be updated: {error}"),
            });
            return false;
        }
    }
    if let Ok(guard) = shared_cancel.lock()
        && let Some(ref cancel) = *guard
    {
        cancel.cancel();
    }
    true
}

fn emit_runtime_queue_notification(
    runtime: &InteractiveRuntimeSupervisor,
    events_tx: &broadcast::Sender<AgentEvent>,
    prompt_id: u64,
) {
    if let Some(prompt) = runtime.queued_prompt(prompt_id) {
        emit_runtime_queue_snapshot(runtime, events_tx);
        let _ = events_tx.send(AgentEvent::SystemNotification {
            message: format!(
                "Queued prompt #{} from {} via {}; queue depth {}.",
                prompt.id,
                prompt.submitted_by.display_label(),
                prompt.via.label(),
                runtime.queue_depth()
            ),
        });
    }
}

fn emit_runtime_queue_snapshot(
    runtime: &InteractiveRuntimeSupervisor,
    events_tx: &broadcast::Sender<AgentEvent>,
) {
    let snapshot_json = runtime.queue_snapshot_json();
    let _ = events_tx.send(AgentEvent::RuntimeQueueUpdated { snapshot_json });
}

pub(crate) struct InteractiveAgentState {
    pub(crate) bus: crate::bus::EventBus,
    pub(crate) context_service: std::sync::Arc<crate::features::context::ContextProvider>,
    pub(crate) context_manager: crate::context::ContextManager,
    pub(crate) conversation: crate::conversation::ConversationState,
    pub(crate) inference_runtime: crate::inference_runtime::InferenceRuntimeState,
    pub(crate) work_snapshot: Option<std::sync::Arc<styrene_work_runtime::WorkSnapshot>>,
    pub(crate) behavior_policy: Option<crate::behavior::BehaviorPolicyBinding>,
    pub(crate) memory_binding: crate::memory_service::MemoryBinding,
    pub(crate) context_compaction:
        crate::context_compaction_service::ContextCompactionBinding,
}

pub(crate) struct InteractiveAgentHost {
    pub(crate) session_id: String,
    pub(crate) session_view_binding: crate::session_consumers::SessionViewBinding,
    pub(crate) instance_id: String,
    pub(crate) runtime_ownership: Option<crate::workspace::runtime::RuntimeOwnership>,
    pub(crate) context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    pub(crate) cwd: PathBuf,
    pub(crate) secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    pub(crate) web_auth_state: crate::web::WebAuthState,
    pub(crate) dashboard_handles: crate::runtime_state::RuntimeStateHandles,
    pub(crate) resume_info: Option<setup::ResumeInfo>,
    pub(crate) workspace_state: setup::WorkspaceStartupState,
    pub(crate) runtime_generation: u64,
    pub(crate) git_binding: crate::git_service::GitBinding,
    pub(crate) dynamic_contribution_control:
        crate::contribution_lifecycle::DynamicContributionControl,
}

pub(crate) struct CliRuntimeView<'a> {
    pub(crate) no_session: bool,
    pub(crate) model: &'a str,
    pub(crate) dangerously_bypass_permissions: bool,
}

fn interactive_resume_mode(cli: &Cli) -> Option<Option<&str>> {
    if cli.fresh {
        None
    } else {
        cli.resume.as_ref().map(|r| r.as_deref())
    }
}

fn split_interactive_agent(
    agent: setup::AgentSetup,
) -> (InteractiveAgentHost, InteractiveAgentState) {
    let host = InteractiveAgentHost {
        session_id: agent.session_id,
        session_view_binding: agent.session_view_binding,
        instance_id: agent.instance_id,
        runtime_ownership: Some(agent.runtime_ownership),
        context_metrics: agent.context_metrics,
        cwd: agent.cwd,
        secrets: agent.secrets,
        web_auth_state: agent.web_auth_state,
        dashboard_handles: agent.dashboard_handles,
        resume_info: agent.resume_info,
        workspace_state: agent.workspace_state,
        runtime_generation: 1,
        git_binding: agent.git_binding,
        dynamic_contribution_control: agent.dynamic_contribution_control,
    };
    let state = InteractiveAgentState {
        bus: agent.bus,
        context_service: agent.context_service,
        context_manager: agent.context_manager,
        conversation: agent.conversation,
        inference_runtime: agent.inference_runtime,
        work_snapshot: agent.work_snapshot,
        behavior_policy: agent.behavior_policy,
        memory_binding: agent.memory_binding,
        context_compaction: agent.context_compaction,
    };
    (host, state)
}

#[derive(Clone)]
struct InteractiveRuntimeResources {
    cwd: PathBuf,
    secrets: std::sync::Arc<omegon_secrets::SecretsManager>,
    context_metrics:
        std::sync::Arc<std::sync::Mutex<crate::features::context::SharedContextMetrics>>,
    bridge_model: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    route_controller: Arc<route::RouteController>,
}

fn build_interactive_loop_config(
    runtime: &InteractiveRuntimeResources,
    shared_settings: &Arc<std::sync::Mutex<settings::Settings>>,
    pending_compact: &Arc<std::sync::atomic::AtomicBool>,
) -> r#loop::LoopConfig {
    let model = shared_settings
        .lock()
        .map(|s| s.model.clone())
        .unwrap_or_default();

    let ollama_manager = if providers::infer_provider_id(&model) == "ollama" {
        Some(ollama::OllamaManager::new())
    } else {
        None
    };

    bootstrap::build_loop_config(
        shared_settings,
        &runtime.cwd,
        &model,
        bootstrap::LoopConfigOverrides {
            secrets: Some(runtime.secrets.clone()),
            force_compact: Some(pending_compact.clone()),
            allow_commit_nudge: true,
            ollama_manager,
            bridge_model: runtime
                .bridge_model
                .lock()
                .ok()
                .and_then(|guard| guard.clone()),
            route_controller: Some(runtime.route_controller.clone()),
            ..Default::default()
        },
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(not(feature = "tui"))]
async fn run_interactive_command(_cli: &Cli) -> anyhow::Result<()> {
    anyhow::bail!(
        "interactive support was not compiled; rebuild with the `tui` feature or use `omegon serve`"
    )
}

#[cfg(feature = "tui")]
#[allow(clippy::too_many_arguments)]
async fn run_interactive_active_turn(
    runtime_state: Arc<tokio::sync::Mutex<InteractiveAgentState>>,
    runtime: InteractiveRuntimeResources,
    bridge: Arc<tokio::sync::RwLock<Box<dyn LlmBridge>>>,
    shared_settings: Arc<std::sync::Mutex<settings::Settings>>,
    shared_cancel: operator_commands::SharedCancel,
    pending_compact: Arc<std::sync::atomic::AtomicBool>,
    events_tx: broadcast::Sender<AgentEvent>,
    active: ActiveTurnMeta,
    active_identity: RuntimeTurnIdentity,
    lifecycle: RuntimeTurnLifecycle,
    cancel: CancellationToken,
    invocation_session_id: Option<String>,
    invocation_authority: Option<crate::session_authority::SessionAuthorityHandle>,
    execution_capture: crate::session_execution::SessionExecutionCapture,
) -> LoopTerminalIntent {
    let mut runtime_state = runtime_state.lock().await;
    let cancel_keeps_prompt = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut loop_config =
        build_interactive_loop_config(&runtime, &shared_settings, &pending_compact);
    loop_config.compatibility.invocation_scope.principal =
        active.prompt.submitted_by.display_label().to_string();
    loop_config.compatibility.invocation_scope.session_id = invocation_session_id;
    loop_config.compatibility.invocation_scope.turn_id = active.authority_turn_id;
    loop_config.compatibility.invocation_scope.authority = invocation_authority;
    loop_config.cancel_keeps_prompt = Some(cancel_keeps_prompt.clone());
    loop_config.compatibility.drain_late_requests = false;
    loop_config.compatibility.work_snapshot = runtime_state.work_snapshot.clone();
    loop_config.compatibility.behavior_policy = runtime_state.behavior_policy.clone();
    loop_config.compatibility.memory_binding = runtime_state.memory_binding.clone();
    loop_config.compatibility.context_compaction = runtime_state.context_compaction.clone();

    if active.prompt.image_paths.is_empty() {
        runtime_state
            .conversation
            .push_user(active.prompt.text.clone());
    } else {
        let mut images = Vec::new();
        for path in &active.prompt.image_paths {
            if let Ok(data) = std::fs::read(path) {
                let media_type = match image::guess_format(&data) {
                    Ok(image::ImageFormat::Png) => Some("image/png"),
                    Ok(image::ImageFormat::Jpeg) => Some("image/jpeg"),
                    Ok(image::ImageFormat::Gif) => Some("image/gif"),
                    Ok(image::ImageFormat::WebP) => Some("image/webp"),
                    _ => None,
                };
                let Some(media_type) = media_type else {
                    tracing::warn!(path = %path.display(), "skipping invalid or provider-unsupported image attachment");
                    continue;
                };
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                images.push(crate::bridge::ImageAttachment {
                    data: b64,
                    media_type: media_type.to_string(),
                    source_path: Some(path.display().to_string()),
                });
            }
        }
        runtime_state
            .conversation
            .push_user_with_images(active.prompt.text.clone(), images);
    }

    lifecycle.emit_phase("conversation_updated", 0, 0, &events_tx, "worker");

    if let Ok(mut guard) = shared_cancel.lock() {
        *guard = Some(cancel.clone());
    }

    let loop_started_at = std::time::Instant::now();
    lifecycle.emit_phase("loop_running", 0, 0, &events_tx, "worker");
    let execution = {
        let bridge_guard = bridge.read().await;
        let state = &mut *runtime_state;
        let mut run = std::pin::pin!(execution_capture.execute(
            bridge_guard.as_ref(),
            &mut state.bus,
            &mut state.context_manager,
            &mut state.conversation,
            &events_tx,
            cancel.clone(),
            &loop_config,
        ));

        await_interactive_execution(run.as_mut(), &cancel, || {
                let keep_prompt = cancel_keeps_prompt.load(std::sync::atomic::Ordering::Relaxed);
                let disposition = if keep_prompt { "interrupted · kept" } else { "aborted · forgotten" };
                tracing::warn!(
                    runtime_turn_id = active.runtime_turn_id,
                    "operator cancellation requested; draining owned provider/tool execution"
                );
                let _ = events_tx.send(AgentEvent::SystemNotification {
                    message: format!("Interrupt requested — releasing the operator surface ({disposition}) while Omegon drains owned provider/tool work. A remote provider request already accepted may still finish externally, but its output is detached from this turn."),
                });
                let _ = events_tx.send(AgentEvent::AgentEnd);
        })
        .await
    };
    let run_result = execution.result;
    let terminal_proposal = execution.terminal;
    let cleanup_started_at = std::time::Instant::now();
    lifecycle.emit_phase("post_loop_cleanup", 0, 0, &events_tx, "worker");
    tracing::info!(
        runtime_turn_id = active.runtime_turn_id,
        loop_elapsed_ms = loop_started_at.elapsed().as_millis() as u64,
        cancelled = cancel.is_cancelled(),
        result = match &run_result {
            Ok(_) => "ok",
            Err(_) => "error",
        },
        "interactive active turn loop returned; starting post-turn cleanup"
    );
    let terminal_intent = interactive_loop_terminal_intent(
        active_identity,
        Some(&terminal_proposal),
        cancel.is_cancelled(),
    );

    if run_result.is_ok() && cancel.is_cancelled() {
        let keep_prompt = cancel_keeps_prompt.load(std::sync::atomic::Ordering::Relaxed);
        if !keep_prompt {
            runtime_state
                .conversation
                .rollback_last_user_if_text(&active.prompt.text);
        }
        let disposition = if keep_prompt {
            "interrupted · kept"
        } else {
            "aborted · forgotten"
        };
        let _ = events_tx.send(AgentEvent::MessageAbort {
            reason: Some(disposition.to_string()),
        });
    }

    if let Err(e) = run_result {
        let terminal_reason = if crate::provider_route_service::is_upstream_exhausted(&e) {
            omegon_traits::TurnEndReason::ProviderExhausted
        } else {
            omegon_traits::TurnEndReason::WorkerFailed
        };
        let _ = events_tx.send(AgentEvent::TurnEnd(Box::new(
            omegon_traits::AgentEventTurnEnd {
                turn: runtime_state.conversation.intent.stats.turns,
                turn_end_reason: terminal_reason,
                model: loop_config
                    .bridge_model
                    .clone()
                    .or_else(|| Some(loop_config.model.clone())),
                provider: loop_config
                    .bridge_model
                    .as_deref()
                    .map(crate::providers::infer_provider_id)
                    .or_else(|| Some(crate::providers::infer_provider_id(&loop_config.model))),
                estimated_tokens: runtime_state.conversation.estimate_tokens(),
                context_window: 0,
                context_composition: omegon_traits::ContextComposition::default(),
                actual_input_tokens: 0,
                actual_output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
                dominant_phase: None,
                drift_kind: None,
                progress_nudge_reason: None,
                intent_task: runtime_state.conversation.intent.current_task.clone(),
                intent_phase: Some(format!(
                    "{:?}",
                    runtime_state.conversation.intent.lifecycle_phase
                )),
                files_read_count: runtime_state.conversation.intent.files_read.len(),
                files_modified_count: runtime_state.conversation.intent.files_modified.len(),
                stats_tool_calls: runtime_state.conversation.intent.stats.tool_calls,
                streaks: omegon_traits::ControllerStreaks::default(),
            },
        )));
        let recent_telemetry = runtime_state.conversation.last_provider_telemetry(None);
        let user_msg = format_agent_error(&e, recent_telemetry.as_ref());
        tracing::error!(
            runtime_turn_id = active.runtime_turn_id,
            "Agent loop error: {e}"
        );
        runtime_state
            .conversation
            .rollback_last_user_if_text(&active.prompt.text);
        let _ = events_tx.send(AgentEvent::SystemNotification { message: user_msg });
        let _ = events_tx.send(AgentEvent::AgentEnd);
    }

    let cancel_lock_started_at = std::time::Instant::now();
    if let Ok(mut guard) = shared_cancel.lock() {
        guard.take();
    }
    let cancel_lock_elapsed = cancel_lock_started_at.elapsed();
    lifecycle.emit_phase(
        "cleanup_cancel_token_cleared",
        cancel_lock_elapsed.as_millis() as u64,
        0,
        &events_tx,
        "worker",
    );
    if cancel_lock_elapsed > std::time::Duration::from_millis(250) {
        tracing::warn!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = cancel_lock_elapsed.as_millis() as u64,
            "post-turn cleanup waited on shared cancel lock"
        );
    } else {
        tracing::debug!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = cancel_lock_elapsed.as_millis() as u64,
            "post-turn cleanup cleared shared cancel token"
        );
    }

    let estimate_started_at = std::time::Instant::now();
    let est = runtime_state.conversation.estimate_tokens();
    let estimate_elapsed = estimate_started_at.elapsed();
    lifecycle.emit_phase(
        "cleanup_tokens_estimated",
        estimate_elapsed.as_millis() as u64,
        0,
        &events_tx,
        "worker",
    );
    if estimate_elapsed > std::time::Duration::from_millis(250) {
        tracing::warn!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = estimate_elapsed.as_millis() as u64,
            estimated_tokens = est,
            "post-turn cleanup spent unusually long estimating transcript tokens"
        );
    } else {
        tracing::debug!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = estimate_elapsed.as_millis() as u64,
            estimated_tokens = est,
            "post-turn cleanup estimated transcript tokens"
        );
    }

    let settings_lock_started_at = std::time::Instant::now();
    let settings = shared_settings.lock().unwrap();
    let settings_lock_elapsed = settings_lock_started_at.elapsed();
    lifecycle.emit_phase(
        "cleanup_settings_acquired",
        settings_lock_elapsed.as_millis() as u64,
        0,
        &events_tx,
        "worker",
    );
    if settings_lock_elapsed > std::time::Duration::from_millis(250) {
        tracing::warn!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = settings_lock_elapsed.as_millis() as u64,
            "post-turn cleanup waited on shared settings lock"
        );
    } else {
        tracing::debug!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = settings_lock_elapsed.as_millis() as u64,
            "post-turn cleanup acquired shared settings lock"
        );
    }
    let projection = settings.inference_projection();
    let est = projection.estimated_visible_input.unwrap_or(est);
    let context_window = projection.assembly_budget.unwrap_or(settings.context_window);
    let context_class = settings.effective_requested_class().label().to_string();
    let thinking_level = projection.effective_reasoning;

    let metrics_lock_started_at = std::time::Instant::now();
    if let Ok(mut metrics) = runtime.context_metrics.lock() {
        metrics.update(est, context_window, &context_class, &thinking_level);
        metrics.measured = projection.estimated_visible_input.is_some();
    }
    let metrics_lock_elapsed = metrics_lock_started_at.elapsed();
    lifecycle.emit_phase(
        "cleanup_metrics_updated",
        metrics_lock_elapsed.as_millis() as u64,
        0,
        &events_tx,
        "worker",
    );
    if metrics_lock_elapsed > std::time::Duration::from_millis(250) {
        tracing::warn!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = metrics_lock_elapsed.as_millis() as u64,
            "post-turn cleanup waited on context metrics lock"
        );
    } else {
        tracing::debug!(
            runtime_turn_id = active.runtime_turn_id,
            elapsed_ms = metrics_lock_elapsed.as_millis() as u64,
            "post-turn cleanup updated context metrics"
        );
    }
    let _ = events_tx.send(AgentEvent::ContextUpdated {
        tokens: est as u64,
        context_window: context_window as u64,
        context_class,
        thinking_level,
    });
    tracing::info!(
        runtime_turn_id = active.runtime_turn_id,
        cleanup_elapsed_ms = cleanup_started_at.elapsed().as_millis() as u64,
        "interactive active turn post-turn cleanup finished"
    );
    lifecycle.emit_phase(
        "worker_returning",
        cleanup_started_at.elapsed().as_millis() as u64,
        0,
        &events_tx,
        "worker",
    );
    terminal_intent
}

async fn stop_voice_session_if_requested(
    prompt: &PromptEnvelope,
    bus: &crate::bus::EventBus,
    events_tx: &tokio::sync::broadcast::Sender<AgentEvent>,
    scope: crate::invocation_service::InvocationScope,
) {
    if !prompt.requests_voice_close() {
        return;
    }

    if !bus.has_tool("voice_session_stop") {
        let _ = events_tx.send(AgentEvent::SystemNotification {
            message: "Voice requested shutdown after this prompt, but no voice_session_stop tool is available.".to_string(),
        });
        return;
    }

    let call_id = format!("voice-over-and-out-stop:{}", uuid::Uuid::new_v4());
    match bus
        .invoke_tool(
            "voice_session_stop",
            &call_id,
            serde_json::json!({}),
            tokio_util::sync::CancellationToken::new(),
            scope,
        )
        .await
    {
        Ok(_) => {
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: "Voice session stop requested after over and out.".to_string(),
            });
        }
        Err(err) => {
            let _ = events_tx.send(AgentEvent::SystemNotification {
                message: format!("Voice requested shutdown, but voice_session_stop failed: {err}"),
            });
        }
    }
}
