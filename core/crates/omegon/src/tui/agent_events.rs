//! Agent-event projection into native TUI application state.
//!
//! Channel receive and scheduling remain in `tui::run_tui`; this adapter owns
//! the deterministic `AgentEvent -> App` state transition boundary.

use super::*;

impl App {
    pub(super) fn prune_activity_tools(&mut self, now: std::time::Instant) {
        self.activity_tools.retain(|tool| {
            tool.expires_at
                .map(|deadline| now < deadline)
                .unwrap_or(true)
        });
    }

    fn cap_activity_tools(&mut self) {
        const MAX_COMPLETED_ACTIVITY_TOOLS: usize = 4;
        const MAX_ACTIVITY_TOOLS: usize = 8;

        let mut completed_seen = 0usize;
        self.activity_tools.retain(|tool| {
            if matches!(
                tool.status,
                crate::surfaces::activity::ActivityToolStatus::Running
            ) {
                return true;
            }
            completed_seen += 1;
            completed_seen <= MAX_COMPLETED_ACTIVITY_TOOLS
        });

        while self.activity_tools.len() > MAX_ACTIVITY_TOOLS {
            if let Some(idx) = self.activity_tools.iter().rposition(|tool| {
                !matches!(
                    tool.status,
                    crate::surfaces::activity::ActivityToolStatus::Running
                )
            }) {
                self.activity_tools.remove(idx);
            } else {
                self.activity_tools.pop_back();
            }
        }
    }

    fn push_activity_tool_start(&mut self, id: &str, name: &str, args_summary: Option<String>) {
        self.prune_activity_tools(std::time::Instant::now());
        self.activity_tools.retain(|tool| tool.segment_id != id);
        self.activity_tools.push_front(ActivityToolState {
            episode_id: if name == "operator_shell" {
                format!("operator-shell:{id}")
            } else {
                format!("turn:{}", self.turn)
            },
            segment_id: id.to_string(),
            name: name.to_string(),
            args_summary,
            result_summary: None,
            mode: crate::surfaces::activity::ActivityToolMode::Live,
            status: crate::surfaces::activity::ActivityToolStatus::Running,
            expires_at: None,
        });
        self.cap_activity_tools();
    }

    fn mark_activity_tool_end(&mut self, id: &str, is_error: bool, result_summary: Option<String>) {
        let linger_for = if is_error {
            Duration::from_secs(8)
        } else {
            Duration::ZERO
        };
        let expires_at = std::time::Instant::now() + linger_for;
        if let Some(activity_tool) = self
            .activity_tools
            .iter_mut()
            .find(|tool| tool.segment_id == id)
        {
            activity_tool.status = if is_error {
                crate::surfaces::activity::ActivityToolStatus::Error
            } else {
                crate::surfaces::activity::ActivityToolStatus::Complete
            };
            activity_tool.result_summary = result_summary;
            activity_tool.expires_at = Some(expires_at);
        }
        self.cap_activity_tools();
    }

    fn expire_running_activity_tools(&mut self, ttl: Duration) {
        let expires_at = std::time::Instant::now() + ttl;
        for tool in &mut self.activity_tools {
            if matches!(
                tool.status,
                crate::surfaces::activity::ActivityToolStatus::Running
            ) {
                tool.status = crate::surfaces::activity::ActivityToolStatus::Cancelled;
                tool.expires_at = Some(expires_at);
            }
        }
        self.cap_activity_tools();
    }

    pub(super) fn handle_agent_event(&mut self, event: AgentEvent) {
        let decision = self.stream_presentation.classify(event.clone());
        if !decision.apply_now {
            return;
        }
        match event {
            AgentEvent::TurnStart { turn } => {
                self.agent_active = true;
                self.slim_turn_state = SlimTurnState::RequestingProvider;
                self.dashboard_handles.session().set_busy(true);
                self.turn = turn;
                self.working_verb = spinner::next_verb();
                self.effects.start_spinner_glow();
                self.effects.start_border_pulse();
            }
            AgentEvent::TurnEnd(te) => {
                self.turn = te.turn;
                if self.runtime_queue_snapshot.is_none() {
                    self.runtime_queue_snapshot = Some(serde_json::json!({
                        "depth": 0,
                        "active": null,
                        "items": [],
                        "previews": [],
                    }));
                }
                let turn_end_reason = te.turn_end_reason;
                self.slim_turn_state = SlimTurnState::Finished(match turn_end_reason {
                    omegon_traits::TurnEndReason::AssistantCompleted => "done",
                    omegon_traits::TurnEndReason::AwaitingOperator => "waiting",
                    omegon_traits::TurnEndReason::Blocked => "blocked",
                    omegon_traits::TurnEndReason::TurnLimitReached => "turn limit",
                    omegon_traits::TurnEndReason::ProviderExhausted => "provider exhausted",
                    omegon_traits::TurnEndReason::WorkerFailed => "worker failed",
                    omegon_traits::TurnEndReason::ToolContinuation => "continuing",
                    omegon_traits::TurnEndReason::ProgressNudge => "nudged",
                    omegon_traits::TurnEndReason::Cancelled => "cancelled",
                });
                if matches!(
                    turn_end_reason,
                    omegon_traits::TurnEndReason::AssistantCompleted
                        | omegon_traits::TurnEndReason::AwaitingOperator
                        | omegon_traits::TurnEndReason::Blocked
                        | omegon_traits::TurnEndReason::TurnLimitReached
                        | omegon_traits::TurnEndReason::ProviderExhausted
                        | omegon_traits::TurnEndReason::WorkerFailed
                        | omegon_traits::TurnEndReason::Cancelled
                ) {
                    self.agent_active = false;
                    self.dashboard_handles.session().set_busy(false);
                    self.effects.stop_spinner_glow();
                    self.effects.stop_border_pulse();
                }
                if matches!(turn_end_reason, omegon_traits::TurnEndReason::Cancelled) {
                    // Cancellation abandons the in-flight turn, so clear the live workbench
                    // lane. Completed plans are still cleared by PlanUpdated handling; incomplete
                    // plans survive AssistantCompleted so the operator can inspect and continue
                    // the visible work plan between turns.
                    self.workbench_state.active = None;
                }
                // Update session row with behavioral signals
                self.session_row.phase = te.dominant_phase;
                self.session_row.drift = te.drift_kind;
                self.session_row.files_read = te.files_read_count;
                self.session_row.files_modified = te.files_modified_count;
                // Accumulate session-long token counts
                self.footer_data.session_input_tokens += te.actual_input_tokens;
                self.footer_data.session_output_tokens += te.actual_output_tokens;
                self.footer_data.last_turn_input_tokens = te.actual_input_tokens;
                self.footer_data.last_turn_output_tokens = te.actual_output_tokens;
                if (te.actual_input_tokens > 0 || te.actual_output_tokens > 0)
                    && let Some(model_id) = te.model
                {
                    self.footer_data
                        .session_usage_slices
                        .push(SessionUsageSlice {
                            model_id,
                            provider: te.provider.unwrap_or_default(),
                            input_tokens: te.actual_input_tokens,
                            output_tokens: te.actual_output_tokens,
                        });
                }
                // Forward raw token counts to the instrument panel
                self.instrument_panel.update_turn_tokens(
                    te.actual_input_tokens as u32,
                    te.actual_output_tokens as u32,
                    te.cache_read_tokens as u32,
                    te.context_composition.clone(),
                    te.context_window,
                );
                let ctx_window = self.footer_data.context_window;
                if ctx_window > 0 {
                    // Footer context posture is total live-context usage, not the last request's
                    // provider-reported input tokens. ContextUpdated is the authoritative source;
                    // TurnEnd may fill gaps when no prior context snapshot was emitted.
                    let tokens = if te.estimated_tokens > 0 {
                        te.estimated_tokens
                    } else {
                        self.footer_data.estimated_tokens
                    };
                    self.footer_data.estimated_tokens = tokens;
                    self.footer_data.context_percent =
                        (tokens as f32 / ctx_window as f32 * 100.0).min(100.0);
                    // Context danger pulse: activate >80%, deactivate <75% (hysteresis)
                    let pct = self.footer_data.context_percent;
                    if pct > 80.0 {
                        self.effects.set_context_danger(true);
                    } else if pct < 75.0 {
                        self.effects.set_context_danger(false);
                    }
                    // Context pressure gradient on conversation zone
                    self.effects.set_context_pressure(pct);
                }
                self.footer_data.provider_telemetry = te.provider_telemetry;

                // Stamp the provider-reported actual tokens onto every
                // segment that belongs to this turn so the title-bar
                // annotation (`↑input ↓output` next to the timestamp)
                // shows up across all of them at once. Tool cards,
                // assistant text, and any other segment created during
                // the turn share the same `meta.turn` from
                // `current_meta()` and pick up the stamp here.
                if te.actual_input_tokens > 0 || te.actual_output_tokens > 0 {
                    self.conversation.stamp_turn_tokens(
                        te.turn,
                        segments::TokenUsage {
                            input: te.actual_input_tokens,
                            output: te.actual_output_tokens,
                        },
                    );
                }
                self.effects.ping_footer(self.theme.as_ref());
                // Detect if the agent is asking for confirmation and offer
                // a one-key continuation affordance in the editor.
                self.detect_continuation_request();
            }
            AgentEvent::MessageStart { .. } => {
                self.slim_turn_state = SlimTurnState::OpeningStream;
            }
            AgentEvent::MessageEnd => {
                self.conversation.finalize_message();
            }
            AgentEvent::MessageChunk { text } => {
                self.slim_turn_state = SlimTurnState::Responding;
                let was_streaming = self.conversation.is_streaming();
                self.conversation.append_streaming(&text);
                if !was_streaming {
                    // First chunk of a new response — stamp model metadata
                    self.conversation.stamp_meta(self.current_meta());
                }
            }
            AgentEvent::ThinkingChunk { text } => {
                self.slim_turn_state = SlimTurnState::Thinking;
                self.instrument_panel.note_thinking_activity();
                let was_streaming = self.conversation.is_streaming();
                self.conversation.append_thinking(&text);
                if !was_streaming {
                    self.conversation.stamp_meta(self.current_meta());
                }
            }
            AgentEvent::ToolStart {
                id,
                name,
                args,
                provenance,
            } => {
                self.working_verb = spinner::next_verb();
                self.instrument_panel.tool_started(&name);
                self.slim_turn_state = SlimTurnState::Tool(name.replace('_', " "));
                let args_summary = crate::r#loop::summarize_tool_args(&name, &args);
                // Full args for detailed view
                let detail_args = match name.as_str() {
                    "bash" => args.get("command").and_then(|v| v.as_str()).map(|cmd| {
                        // Strip `cd /path && ` wrapper so the card header shows
                        // the actual command, not a misleading `cd`.
                        if let Some(rest) = cmd.strip_prefix("cd ") {
                            rest.split_once(" && ")
                                .map(|(_, after)| after.to_string())
                                .unwrap_or_else(|| cmd.to_string())
                        } else {
                            cmd.to_string()
                        }
                    }),
                    "read" | "edit" | "write" | "view" => args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    "cleave_run" => {
                        let directive = args
                            .get("directive")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(no directive)");
                        let directive_short = crate::util::truncate(directive, 100);
                        // Parse plan_json to extract child labels
                        let children_line = args
                            .get("plan_json")
                            .and_then(|v| v.as_str())
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                            .and_then(|plan| {
                                plan.get("children")
                                    .and_then(|c| c.as_array())
                                    .map(|children| {
                                        children
                                            .iter()
                                            .filter_map(|c| {
                                                let label =
                                                    c.get("label").and_then(|v| v.as_str())?;
                                                let desc = c
                                                    .get("description")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                let desc_short = crate::util::truncate(desc, 60);
                                                Some(format!("  • {label}: {desc_short}"))
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                            })
                            .unwrap_or_default();
                        Some(format!("{directive_short}\n{children_line}"))
                    }
                    "cleave_assess" => args
                        .get("directive")
                        .and_then(|v| v.as_str())
                        .map(|s| crate::util::truncate(s, 120)),
                    // Suppress raw JSON dump for all other harness-internal tools
                    "design_tree" | "design_tree_update" | "openspec_manage" | "memory_store"
                    | "memory_recall" | "memory_focus" | "memory_supersede" | "memory_archive"
                    | "memory_query" | "memory_episodes" | "memory_compact" | "cleave_delegate"
                    | "lifecycle_doctor" => None,
                    _ => Some(serde_json::to_string_pretty(&args).unwrap_or_default()),
                };
                self.tool_inspection_target = Some(ToolInspectionTarget::LiveLatest {
                    evidence_id: id.clone(),
                });
                self.push_activity_tool_start(&id, &name, args_summary.clone());
                self.conversation.push_tool_start_with_expanded(
                    &id,
                    &name,
                    provenance,
                    args_summary.as_deref(),
                    detail_args.as_deref(),
                    id.starts_with("shell-"),
                );
                self.conversation.stamp_meta(self.current_meta());
                self.tool_calls += 1;
                self.last_tool_name = Some(name);
            }
            AgentEvent::PermissionRequest {
                tool_name,
                path,
                kind,
                persistence,
                grant_path,
                respond,
            } => {
                self.slim_turn_state = SlimTurnState::Finished("blocked");
                // Show a blocking permission prompt in the TUI.
                let prompt_text = format_permission_prompt(
                    &tool_name,
                    &path,
                    kind,
                    persistence,
                    grant_path.as_deref(),
                );
                self.command_prompt = Some(
                    CommandPrompt::new("Permission required", prompt_text.clone()).with_actions(
                        vec![
                            CommandPromptAction::new("y", "this operation"),
                            CommandPromptAction::new("a", "this directory · session"),
                            CommandPromptAction::new("Shift+A", "this directory · project"),
                            CommandPromptAction::new("n", "deny"),
                        ],
                    ),
                );

                // Store the responder — the next key event (y/a/n) will
                // resolve it. See handle_permission_key below.
                self.pending_permission = Some(respond.clone());
                self.pending_permission_context = Some(PendingPermissionContext {
                    tool_name,
                    target: path,
                    kind,
                    persistence,
                    grant_path,
                });
            }
            AgentEvent::OperatorWaitRequest {
                prompt,
                timeout_secs,
                acknowledge,
                respond,
            } => {
                self.slim_turn_state = SlimTurnState::Finished("waiting");
                let prompt_text = format!(
                    "Manual action required\n   {prompt}\n   [Enter/Space/d] done   [c/Esc] cancel   safety timeout: {timeout_secs}s"
                );
                self.command_prompt = Some(
                    CommandPrompt::new("Manual action required", prompt_text.clone()).with_actions(
                        vec![
                            CommandPromptAction::new("Enter", "done"),
                            CommandPromptAction::new("Space/d", "done"),
                            CommandPromptAction::new("c/Esc", "cancel"),
                        ],
                    ),
                );
                if let Ok(mut slot) = acknowledge.lock()
                    && let Some(tx) = slot.take()
                {
                    let _ = tx.send(());
                }
                self.pending_operator_wait = Some(respond.clone());
                self.pending_operator_wait_context = Some(prompt);
            }
            AgentEvent::ToolEnd {
                id,
                name,
                result,
                is_error,
                provenance,
            } => {
                // Tool execution can mutate repository state (commit, checkout,
                // merge, delegated worktrees). Refresh the branch affordance at
                // the event boundary instead of retaining the startup snapshot.
                self.workbench_state.workspace = self.current_workbench_workspace_context();

                if name == crate::tool_registry::core::WAIT_FOR_OPERATOR
                    && self.pending_operator_wait.is_some()
                {
                    self.pending_operator_wait = None;
                    self.pending_operator_wait_context = None;
                    self.command_prompt = None;
                }

                let text_blocks: Vec<&str> = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let full_text = if text_blocks.is_empty() {
                    None
                } else {
                    Some(text_blocks.join("\n\n"))
                };

                // Append recovery hint for tool errors
                let enriched: Option<String> = if is_error {
                    full_text.as_ref().and_then(|text| {
                        let hint = Self::recovery_hint(Some(name.as_str()), text);
                        if hint.is_empty() {
                            None
                        } else {
                            Some(format!("{text}\n\n💡 {hint}"))
                        }
                    })
                } else {
                    None
                };

                // Use enriched message if available, otherwise the full text payload.
                let display = enriched.as_deref().or(full_text.as_deref());
                let _ = provenance;
                self.conversation.push_tool_end(&id, is_error, display);
                self.mark_activity_tool_end(
                    &id,
                    is_error,
                    display.map(|text| crate::util::truncate(text, 96)),
                );

                // Visual feedback: error flash or completion pulse
                if is_error {
                    self.effects.flash_error();
                } else {
                    self.effects.pulse_new_card();
                }

                // Detect image results from structured blocks/details first.
                // Text scraping remains as a fallback for legacy render tools.
                if !is_error
                    && result
                        .content
                        .iter()
                        .any(|block| matches!(block, omegon_traits::ContentBlock::Image { .. }))
                {
                    let image_path = result
                        .details
                        .get("path")
                        .or_else(|| result.details.get("output_path"))
                        .and_then(|value| value.as_str())
                        .map(std::path::PathBuf::from)
                        .or_else(|| {
                            full_text.as_ref().and_then(|text| {
                                text.lines().find_map(|line| {
                                    let trimmed = line.trim();
                                    if image::is_image_path(trimmed)
                                        && std::path::Path::new(trimmed).exists()
                                    {
                                        Some(std::path::PathBuf::from(trimmed))
                                    } else {
                                        None
                                    }
                                })
                            })
                        });

                    match (image::is_available(), image_path) {
                        (true, Some(path)) if path.exists() => {
                            self.conversation.push_image(path, "");
                        }
                        (false, Some(path)) => {
                            self.conversation.push_system(&format!(
                                "Image result available, but terminal image rendering is unavailable here: {}",
                                path.display()
                            ));
                        }
                        (_, Some(path)) => {
                            self.conversation.push_system(&format!(
                                "Image result available, but the local render path does not exist: {}",
                                path.display()
                            ));
                        }
                        (_, None) => {
                            self.conversation.push_system(
                                "Image result available, but no local render path was provided.",
                            );
                        }
                    }
                }

                // Dynamic footer: memory tools update fact count
                let completed_name = name.as_str();
                let is_memory_mutation = matches!(
                    completed_name,
                    "memory_store" | "memory_supersede" | "memory_archive"
                );
                if completed_name == "memory_store" || completed_name == "memory_supersede" {
                    self.footer_data.total_facts += 1;
                    self.instrument_panel.bump_memory_store();
                } else if completed_name == "memory_archive" {
                    self.footer_data.total_facts = self.footer_data.total_facts.saturating_sub(1);
                }
                if is_memory_mutation {
                    self.memory_ops_this_frame += 1;
                    self.effects.ping_footer(self.theme.as_ref());
                }
                // Also count recall/query operations
                if matches!(
                    completed_name,
                    "memory_recall"
                        | "memory_query"
                        | "memory_episodes"
                        | "memory_search_archive"
                        | "memory_focus"
                        | "memory_release"
                ) {
                    self.memory_ops_this_frame += 1;
                    self.instrument_panel.bump_memory_recall();
                }
                self.instrument_panel
                    .tool_finished(completed_name, is_error);
                self.completed_tool_name = self.last_tool_name.take().or(Some(name));
                if self
                    .tool_inspection_target
                    .as_ref()
                    .is_some_and(|target| matches!(target, ToolInspectionTarget::LiveLatest { evidence_id } if evidence_id == &id))
                {
                    self.tool_inspection_target = None;
                }
                if self.agent_active {
                    self.slim_turn_state = SlimTurnState::RequestingProvider;
                }
            }
            AgentEvent::AgentEnd => {
                self.expire_running_activity_tools(Duration::from_millis(2200));
                self.agent_active = false;
                if !matches!(self.slim_turn_state, SlimTurnState::Finished(_)) {
                    self.slim_turn_state = SlimTurnState::Ready;
                }
                if self.interrupt_pending {
                    self.editor.clear_line();
                    self.interrupt_pending = false;
                    self.suppress_editor_input_for(Duration::from_millis(500));
                }
                self.dashboard_handles.session().set_busy(false);
                self.conversation.finalize_message();
                // Keep completed turns anchored at the live tail. The old long-response
                // active-plan heuristic rewound compact sessions to the start of the final
                // assistant segment, which made every completed GPT-5.5 turn land tens
                // of lines above the composer and forced a manual End/scroll recovery.
                self.effects.stop_spinner_glow();
                self.effects.stop_border_pulse();
                self.effects.sweep_turn_complete();
                // Advance tutorial overlay if an AutoPrompt step just completed
                if let Some(ref mut overlay) = self.tutorial_overlay {
                    overlay.on_agent_turn_complete();
                }
            }
            AgentEvent::PhaseChanged { phase } => {
                self.conversation
                    .push_lifecycle("◈", &format!("Phase → {phase:?}"));
            }
            AgentEvent::DecompositionStarted {
                children,
                operation,
            } => {
                let milestone = OperationMilestoneProjection::started(&operation, children.len());
                self.conversation.push_operation_lifecycle(
                    &operation,
                    milestone.icon,
                    &milestone.text,
                );
            }
            AgentEvent::DecompositionChildCompleted {
                label,
                success,
                operation,
            } => {
                let milestone =
                    OperationMilestoneProjection::child_completed(&operation, &label, success);
                self.conversation.push_operation_lifecycle(
                    &operation,
                    milestone.icon,
                    &milestone.text,
                );
            }
            AgentEvent::DecompositionCompleted { merged, operation } => {
                let milestone = OperationMilestoneProjection::completed(&operation, merged);
                self.conversation.push_operation_lifecycle(
                    &operation,
                    milestone.icon,
                    &milestone.text,
                );
            }
            AgentEvent::WebDashboardStarted { startup_json } => {
                if let Ok(startup) =
                    serde_json::from_value::<crate::web::WebStartupInfo>(startup_json)
                    && let Ok(addr) = startup.addr.parse()
                {
                    self.web_server_addr = Some(addr);
                    self.web_startup = Some(startup);
                }
            }
            AgentEvent::RouteChanged {
                state,
                selected,
                serving,
                warning,
                message,
            } => {
                self.route_state = Some(state.clone());
                self.route_selected_model = selected.clone();
                self.route_serving_model = serving.clone();
                if let Some(serving) = serving.as_ref() {
                    self.footer_data.model_id = serving.clone();
                    self.footer_data.model_provider = crate::providers::infer_provider_id(serving);
                }
                self.footer_data.route_warning = warning.clone().or_else(|| {
                    if state == "serving" {
                        None
                    } else {
                        Some(message.clone())
                    }
                });
                if state == "serving" {
                    self.footer_data.route_warning = None;
                }
                self.show_toast(&message, ratatui_toaster::ToastType::Info);
            }
            AgentEvent::RuntimeQueueUpdated { snapshot_json } => {
                self.runtime_queue_snapshot = Some(snapshot_json);
            }
            AgentEvent::RuntimePromptStarted {
                runtime_turn_id,
                text,
                image_paths,
            } => {
                self.runtime_turn_id = Some(runtime_turn_id);
                if image_paths.is_empty() {
                    self.conversation.push_user(&text);
                } else {
                    self.conversation
                        .push_user_with_attachments(&text, &image_paths);
                }
            }
            AgentEvent::SkillActivation { event } => {
                let mut parts = vec![
                    format!("skill active: {}", event.active_ref),
                    event.resolution.clone(),
                ];
                if let Some(activation) = event.activation.as_ref()
                    && !activation.is_empty()
                {
                    parts.push(activation.clone());
                }
                if !event.matched_signals.is_empty() {
                    parts.push(format!("matched {}", event.matched_signals.join(", ")));
                }
                if !event.suppressing.is_empty() {
                    parts.push(format!("suppressing {}", event.suppressing.join(", ")));
                }
                if let Some(recommendation) = event.recommendation.as_ref()
                    && !recommendation.is_empty()
                {
                    parts.push(recommendation.clone());
                }
                let glyph =
                    crate::tui::glyphs::glyphs().engine(crate::tui::glyphs::EngineGlyphRole::Skill);
                self.conversation
                    .push_system(&format!("{glyph} {}", parts.join(" · ")));
            }
            AgentEvent::OperatorCopyBlock {
                label,
                text,
                kind,
                copy_attempt,
            } => {
                self.conversation
                    .push_operator_copy_block(label, text, kind, copy_attempt);
            }
            AgentEvent::CommandSurface { command, body } => {
                // Provenance-routed: identical policy to a keyboard-submitted
                // slash command, because it is the same command.
                self.show_slash_response(&command, &body);
            }
            AgentEvent::SystemNotification { message } => {
                if let Some(detail) = upstream_retry_hint(&message) {
                    self.slim_turn_state = SlimTurnState::UpstreamRetrying(detail);
                }
                // Transient retry notifications → toast (operator sees them but they
                // don't clutter the conversation). Milestone warnings and other
                // persistent messages → conversation.
                if message.starts_with('⟳')
                    || message.starts_with("Retrying")
                    || message.contains("— retrying")
                {
                    self.show_toast(&message, ratatui_toaster::ToastType::Warning);
                } else if message.starts_with('↯') || is_one_shot_context_notification(&message) {
                    self.show_toast(&message, ratatui_toaster::ToastType::Info);
                } else {
                    self.conversation.push_system(&message);
                }
            }
            AgentEvent::StreamIdle {
                provider,
                model,
                phase,
                idle_secs,
                ambiguous,
                message,
            } => {
                let qualifier = if ambiguous { " · ambiguous" } else { "" };
                self.slim_turn_state =
                    SlimTurnState::StreamIdle(format!("{idle_secs}s · {phase}{qualifier}"));
                self.show_toast(
                    &format!("Stream idle · {provider}/{model} · {message}"),
                    ratatui_toaster::ToastType::Warning,
                );
            }
            AgentEvent::ProviderFailure {
                provider,
                model,
                reason,
                attempts,
                message,
                retryable,
                recommended_action,
            } => {
                self.slim_turn_state = SlimTurnState::Finished("provider failed");
                self.agent_active = false;
                let retry = if retryable { "retryable" } else { "terminal" };
                self.conversation.push_system(&format!(
                    "Provider failure · {provider}/{model} · {reason} · {attempts} attempt(s) · {retry} · {message}\nRecommended action: {recommended_action}"
                ));
            }
            AgentEvent::RuntimeTurnLifecycleUpdated { snapshot_json } => {
                let phase = snapshot_json
                    .get("phase")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("active")
                    .replace('_', " ");
                self.slim_turn_state = SlimTurnState::Lifecycle(format!("turn {phase}"));
            }
            AgentEvent::PlanUpdated { projection } => {
                if WorkbenchState::is_workstream_only_projection(&projection) {
                    self.workbench_state
                        .merge_workstream_projection(&projection);
                    return;
                }
                let dock_state = WorkbenchState::from_plan_projection(&projection);
                self.completed_plan_history_available = dock_state
                    .active
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.is_complete())
                    || self.completed_plan_history_available;
                if let Some(snapshot) = dock_state.active.as_ref()
                    && snapshot.is_complete()
                {
                    let latest_is_complete = self
                        .conversation
                        .latest_plan_progress()
                        .and_then(PlanDisplaySnapshot::from_legacy_text)
                        .is_some_and(|latest| latest.is_complete());
                    if !latest_is_complete {
                        self.conversation
                            .push_system(&snapshot.system_notification_text("Plan progress"));
                    }
                    self.conversation.snap_to_bottom();
                    self.dashboard_handles.clear_cleave();
                    self.dashboard_handles.clear_delegate();
                    self.dashboard.cleave = None;
                    self.dashboard.delegate = None;
                    self.instrument_panel.set_cleave_progress(None);
                    let refreshed_workspace = self.current_workbench_workspace_context();
                    self.workbench_state = WorkbenchState {
                        active: None,
                        workstreams: dock_state.workstreams,
                        workspace: if refreshed_workspace.has_visible_context() {
                            refreshed_workspace
                        } else {
                            self.workbench_state.workspace.clone()
                        },
                    };
                } else {
                    self.workbench_state.active = dock_state.active;
                    self.workbench_state.workstreams = dock_state.workstreams;
                }
            }
            AgentEvent::SessionReset => {
                self.conversation = ConversationView::new();
                self.workbench_state.active = None;
                self.completed_plan_history_available = false;
                self.tool_inspection_target = None;
                self.activity_tools.clear();
                self.turn = 0;
                self.tool_calls = 0;
                self.last_tool_name = None;
                self.completed_tool_name = None;
                self.command_panel = None;
                self.command_prompt = None;
                self.active_modal = None;
                self.active_action_prompt = None;
                self.instrument_panel.reset();
                self.footer_data.turn = 0;
                self.footer_data.tool_calls = 0;
                self.footer_data.compactions = 0;
                self.footer_data.update_available = None;
                self.conversation
                    .push_system("New session started. Previous session saved.");
            }
            AgentEvent::HarnessStatusChanged { status_json } => {
                // Deserialize and update the footer's harness status snapshot
                if let Ok(status) =
                    serde_json::from_value::<crate::status::HarnessStatus>(status_json)
                {
                    // Compare with previous status and show toasts for changes
                    if let Some(prev) = self.previous_harness_status.take() {
                        self.show_status_change_toasts(&prev, &status);
                    }

                    // Update footer data and store current status as previous
                    let operating_profile_summary = self
                        .settings()
                        .operating_profile()
                        .with_persona(self.current_persona_state())
                        .summary();
                    let mut status = status;
                    status.operating_profile = operating_profile_summary;
                    self.footer_data.update_harness(status.clone());
                    self.previous_harness_status = Some(status);
                    self.workbench_state.workspace = self.current_workbench_workspace_context();

                    // Visual effect
                    self.effects.ping_footer(self.theme.as_ref());
                }
            }
            AgentEvent::ContextUpdated {
                tokens,
                context_window,
                context_class,
                thinking_level,
            } => {
                self.footer_data.estimated_tokens = tokens as usize;
                self.footer_data.context_window = context_window as usize;
                self.footer_data.context_class =
                    crate::settings::ContextClass::parse(&context_class).unwrap_or_else(|| {
                        crate::settings::ContextClass::from_tokens(context_window as usize)
                    });
                self.footer_data.actual_context_class =
                    crate::settings::ContextClass::from_tokens(context_window as usize);
                self.footer_data.thinking_level = thinking_level;
                let ctx_window = self.footer_data.context_window;
                self.footer_data.context_percent = if ctx_window > 0 {
                    (tokens as f32 / ctx_window as f32 * 100.0).min(100.0)
                } else {
                    0.0
                };
                self.effects.ping_footer(self.theme.as_ref());
            }
            AgentEvent::MessageAbort { reason } => {
                self.expire_running_activity_tools(Duration::from_secs(4));
                self.conversation.abort_streaming();
                match reason.as_deref() {
                    Some("interrupted · kept") => {
                        self.slim_turn_state = SlimTurnState::InterruptedKept;
                    }
                    Some("aborted · forgotten") => {
                        self.slim_turn_state = SlimTurnState::AbortedForgotten;
                    }
                    _ => {}
                }
            }
            AgentEvent::ToolUpdate { id, partial } => {
                // Stash the latest streaming partial onto the matching
                // open tool card. The conversation segment renderer
                // picks it up via `live_partial` and displays the live
                // tail / progress / heartbeat in place of the empty
                // result section while the tool is still in flight.
                self.conversation.push_tool_update(&id, partial);
            }
            _ => {}
        }
    }
}
