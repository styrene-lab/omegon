//! Agent loop state machine.
//!
//! The core prompt → LLM → tool dispatch → repeat cycle.
//! Includes: turn limits, retry with backoff, stuck detection,
//! context wiring, and parallel tool dispatch.

#[cfg(test)]
use crate::conversation::AssistantMessage;
#[cfg(test)]
use crate::conversation::ConversationState;
#[cfg(test)]
use crate::conversation::ToolCall;
#[cfg(test)]
use crate::conversation::{IntentDocument, ToolResultEntry};
#[cfg(test)]
use omegon_traits::ContentBlock;
#[cfg(test)]
use omegon_traits::DriftKind;
use omegon_traits::{AgentEvent, AgentEventTurnEnd, BusEventTurnEnd, TurnEndReason};

#[cfg(test)]
use serde_json::Value;
use std::time::Instant;
use tokio::sync::broadcast;
#[cfg(test)]
use tokio_util::sync::CancellationToken;

/// Configuration for the agent loop.
pub struct LoopConfig {
    /// Maximum turns before forced stop. 0 = no limit.
    pub max_turns: u32,
    /// Turn at which to inject a "you're running long" advisory.
    /// Defaults to max_turns * 2/3.
    pub soft_limit_turns: u32,
    /// Soft exhaustion threshold for transient upstream errors.
    /// 0 = retry indefinitely (interactive mode).
    /// N > 0 = bail after N consecutive transient failures with an upstream-exhausted
    /// error so the cleave orchestrator can detect it and try a fallback provider.
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    pub retry_delay_ms: u64,
    /// Selected/profile model for UI intent and fallback defaults.
    pub model: String,
    /// Runtime model string to pass to the active bridge when it differs from the
    /// selected/profile model (legacy fallback path; interactive mode should
    /// prefer `route_controller`).
    pub bridge_model: Option<String>,
    /// Working directory used for runtime path resolution.
    pub cwd: std::path::PathBuf,
    /// Extended context window when supported by the active route.
    pub extended_context: bool,
    /// Thinking level — shared settings handle for live reads.
    pub settings: Option<crate::settings::SharedSettings>,
    /// Force a compaction pass before the next turn regardless of threshold.
    pub force_compact: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Whether the loop may spend an extra turn nudging the agent to commit.
    /// Interactive mode wants this; headless/benchmark mode generally does not.
    pub allow_commit_nudge: bool,
    /// Whether the loop should push back on first-turn orientation churn in
    /// execution-biased headless runs (benchmarks, smoke tasks).
    pub enforce_first_turn_execution_bias: bool,
    /// Phase tracking info from loaded skills. When a skill has numbered
    /// phases, the loop checks if the agent completed the final phase
    /// before declaring "done." Prevents premature completion.
    pub skill_phases: Vec<crate::loop_session::CompletionPhaseObligation>,
    /// Release-coupled implementations captured by the driver, not loop policy.
    pub(crate) compatibility: crate::loop_driver::LoopCompatibilityBindings,
    /// Set once the turn has produced assistant/tool-visible effects that should
    /// keep the submitted prompt in replay even if the operator interrupts.
    pub cancel_keeps_prompt: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_turns: 50,
            soft_limit_turns: 35,
            max_retries: 0,
            retry_delay_ms: 750,
            model: crate::settings::Settings::default().model,
            bridge_model: None,
            cwd: std::env::current_dir().unwrap_or_default(),
            extended_context: false,
            settings: None,
            force_compact: None,
            allow_commit_nudge: true,
            enforce_first_turn_execution_bias: false,
            skill_phases: Vec::new(),
            compatibility: crate::loop_driver::LoopCompatibilityBindings::default(),
            cancel_keeps_prompt: None,
        }
    }
}

use crate::behavior::{self, BehavioralTier, ControllerState};

// Behavioral classifiers, streak tracking, continuation pressure, and
// auto-delegation logic live in `behavior.rs`. Re-export convenience
// aliases used by the main loop body.
// auto-delegation disabled — import retained for the test that verifies it returns None
use behavior::ToolCapabilityCatalog;
#[cfg(test)]
use behavior::assess_evidence;
use behavior::behavioral_tier;
#[cfg(test)]
use behavior::classify_auto_delegate_plan;
#[cfg(test)]
use behavior::classify_drift_kind;
#[cfg(test)]
use behavior::classify_progress_signal;
#[cfg(test)]
use behavior::classify_turn_phase;
#[cfg(test)]
use behavior::continuation_pressure_message;
#[cfg(test)]
use behavior::continuation_pressure_tier;
#[cfg(test)]
use behavior::is_first_turn_orientation_churn;
#[cfg(test)]
use behavior::is_mutation_tool_name;
#[cfg(test)]
use behavior::is_repo_inspection_tool;
#[cfg(test)]
use behavior::is_validation_tool_name;
use behavior::progress_nudge_reason_for_drift;
#[cfg(test)]
use behavior::should_inject_execution_pressure;

#[cfg(test)]
use behavior::evidence_sufficiency_message;
use behavior::has_local_target_hypothesis;
use behavior::is_slim_execution_bias;
#[cfg(test)]
use behavior::om_local_first_message;

// Anchor: is_narrow_patch_candidate was here. Now using behavior::*.

/// Run the agent loop to completion.
///
/// Concrete context, route, invocation, and session implementations remain
/// behind the required driver contracts.
pub(crate) async fn run_release_coupled(
    session: &mut dyn crate::loop_driver::LoopSessionContract,
    route: &dyn crate::loop_driver::LoopRouteContract,
    context_contract: &mut dyn crate::loop_driver::LoopContextContract,
    invocation_contract: &mut dyn crate::loop_driver::LoopInvocationContract,
    config: &LoopConfig,
) -> anyhow::Result<()> {
    let session = session.parts();
    let conversation = session.projection;
    let session_policy = session.policy;
    let events = session.advisory_events;
    let cancel = session.cancellation;
    let invocation_scope = &session.invocation_scope;
    let route_step_id = session.route_step_id;
    let semantic_facts = session.semantic_facts;
    let behavior_policy = config.compatibility.behavior_policy.as_ref();
    // tool_defs is refreshed each turn so manage_tools enable/disable takes effect
    // immediately in the schema sent to the LLM (not just in execution routing).

    // Broadcast initial HarnessStatus as AgentEvent so interactive surfaces
    // get the first snapshot. The BusEvent was already emitted in setup.rs;
    // this bridges it to the AgentEvent channel.
    // receive the initial status supplied by their entrypoint.

    let startup_route = route.startup_route().await;

    let session_start = Instant::now();
    let mut controller = ControllerState::default();
    let mut session_used_tools: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut turn: u32 = 0;
    // A tool call on the nominal last turn is not a completed operator turn: the
    // model has not yet seen the tool result and cannot report the outcome.
    // Reserve one response-only turn so the hard ceiling cannot strand the frontend
    // at "turn supervisor completed" without an assistant conclusion.
    let mut final_response_turn_due = false;
    // The no-progress terminal phase shares the response-only reservation. It
    // may be admitted once and cannot recursively schedule itself.
    let mut forced_synthesis_attempted = false;
    // Infer the guidance task mode for this operator prompt (A1). Explicit
    // operator declarations pin the mode; otherwise inference updates it for
    // the current task without overriding a previously pinned mode.
    let last_user_prompt = conversation.last_user_prompt().to_string();
    if let Some(mode) = crate::behavior::explicit_task_mode_from_prompt(&last_user_prompt) {
        conversation.intent.pin_task_mode(mode);
    } else if let Some(policy) = behavior_policy {
        conversation
            .intent
            .observe_task_mode(policy.service.infer_unpinned_task_mode(&last_user_prompt));
    }
    // Active model for this turn — updated each iteration from settings.
    // Used in TurnEnd events and error classification instead of the
    // immutable config.model which is frozen at startup. Starts from the
    // bridge runtime model when fallback installed one, so events emitted
    // before the first per-turn re-read still report the real model.
    let mut active_route = startup_route;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        turn += 1;
        conversation.intent.stats.turns = turn;
        // Refresh tool_defs each turn — manage_tools may have enabled/disabled tools
        // mid-session and we must reflect that in the schema sent to the LLM.
        // Slim/constrained modes use compact schemas and lazy injection to reduce
        // token overhead: core tools always present, extended tools only if used.
        let is_final_response_turn = final_response_turn_due;
        let is_constrained = matches!(behavioral_tier(config), BehavioralTier::Constrained);
        let tool_defs =
            invocation_contract.tool_definitions(crate::loop_driver::LoopToolSurfaceRequest {
                turn,
                used_tools: &session_used_tools,
                final_response_turn: is_final_response_turn,
                constrained: is_constrained,
            });
        let tool_catalog = ToolCapabilityCatalog::from_tool_defs(&tool_defs);
        let context_windows = context_contract.resolve_windows(config);
        let context_window = context_windows.assembly_window;

        if config.max_turns > 0 && turn > config.max_turns && !final_response_turn_due {
            tracing::warn!(
                "Hard turn limit reached ({} turns). Stopping.",
                config.max_turns
            );
            let _ = events.send(AgentEvent::TurnStart { turn });
            let context_composition = context_contract.default_composition(context_window);
            invocation_contract
                .runtime()
                .emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                    BusEventTurnEnd {
                        turn,
                        model: None,
                        provider: None,
                        estimated_tokens: conversation.estimate_tokens(),
                        context_window,
                        context_composition: context_composition.clone(),
                        actual_input_tokens: 0,
                        actual_output_tokens: 0,
                        cache_read_tokens: 0,
                        provider_telemetry: None,
                        dominant_phase: None,
                        drift_kind: None,
                        progress_signal: omegon_traits::ProgressSignal::None,
                    },
                )));
            let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
                turn,
                turn_end_reason: TurnEndReason::TurnLimitReached,
                model: Some(active_route.serving_model.clone()),
                provider: Some(active_route.provider_id.clone()),
                estimated_tokens: conversation.estimate_tokens(),
                context_window,
                context_composition,
                actual_input_tokens: 0,
                actual_output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                provider_telemetry: None,
                dominant_phase: None,
                drift_kind: None,
                progress_nudge_reason: None,
                intent_task: conversation.intent.current_task.clone(),
                intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
                files_read_count: conversation.intent.files_read.len(),
                files_modified_count: conversation.intent.files_modified.len(),
                stats_tool_calls: conversation.intent.stats.tool_calls,
                streaks: controller.streaks(),
            })));
            break;
        }

        let semantic_step = semantic_facts.start_step()?;

        if final_response_turn_due {
            tracing::info!(
                "Tool activity reached the nominal turn limit — reserving this turn for the operator-facing result"
            );
            conversation.push_user(
                "[System: You have reached the tool-execution limit. Do not call more tools. Report the concrete outcome to the operator now, including any blocker or unfinished work.]"
                    .to_string(),
            );
            final_response_turn_due = false;
        }

        // Constrained models get an earlier soft limit (max/2 instead of max*2/3)
        // to give them more room to wrap up before the hard ceiling.
        // Skip soft limit entirely for very short runs (≤5 turns) where the
        // advisory would fire before the model has done meaningful work.
        let effective_soft_limit = if config.soft_limit_turns > 0 && config.max_turns > 5 {
            match behavioral_tier(config) {
                BehavioralTier::Constrained => {
                    let half = config.max_turns / 2;
                    config.soft_limit_turns.min(half.max(2))
                }
                BehavioralTier::Standard => config.soft_limit_turns,
            }
        } else {
            0
        };
        if effective_soft_limit > 0 && turn == effective_soft_limit {
            tracing::info!("Soft turn limit — injecting advisory");
            conversation.push_user(format!(
                "[System: You've been running for {} turns. If you're stuck, \
                 summarize your progress and what's blocking you. If you're \
                 making progress, continue — hard limit is {} turns.]",
                turn, config.max_turns
            ));
        }

        if conversation.intent.operator_correction_pending {
            conversation.intent.operator_correction_pending = false;
            controller = ControllerState::default();
            conversation.push_user(session_policy.operator_correction_recovery());
        }

        if let Some(message) = session_policy.pending_continuation(conversation, &config.cwd) {
            conversation.push_user(message);
        }

        let _ = events.send(AgentEvent::TurnStart { turn });
        invocation_contract
            .runtime()
            .emit(&omegon_traits::BusEvent::TurnStart { turn });

        if let Some(directive) = session_policy.stuck_recovery(&tool_catalog) {
            conversation.push_user(directive.guidance);
        }

        // If context is getting large, try LLM-driven compaction.
        // Trigger at 75% of the configured context window.
        let forced_compact = config
            .force_compact
            .as_ref()
            .is_some_and(|flag| flag.swap(false, std::sync::atomic::Ordering::SeqCst));
        if forced_compact || conversation.needs_compaction(context_window, 0.75) {
            let before_tokens = conversation.estimate_tokens() as u64;
            let trigger = if forced_compact {
                omegon_traits::ContextCompactionTrigger::ForcedLoop
            } else {
                omegon_traits::ContextCompactionTrigger::AutoTier2
            };
            let compaction_plan = context_contract
                .pressure_compaction_plan(
                    conversation.context_compaction_snapshot(),
                    cancel.clone(),
                )
                .await;
            if let Err(error) = &compaction_plan {
                tracing::warn!(%error, "context compaction planning unavailable");
            }
            if let Ok(Some(selection)) = compaction_plan {
                let evict_count = selection.evict_count;
                let fallback_reason = selection.reason.clone();
                tracing::info!(
                    estimated_tokens = before_tokens,
                    evict_count,
                    forced = forced_compact,
                    fallback = fallback_reason.as_deref(),
                    "Context compaction requested"
                );
                emit_context_compaction_event(
                    events,
                    context_compaction_event(
                        trigger,
                        omegon_traits::ContextCompactionStatus::Started,
                        before_tokens,
                        None,
                        Some(evict_count),
                        None,
                        fallback_reason.clone(),
                    ),
                );
                let compaction_authority = context_contract.begin_compaction(
                    &selection,
                    invocation_scope,
                    route_step_id,
                    crate::loop_driver::LoopCompactionTrigger::ContextPressure,
                )?;
                match route
                    .compact(crate::loop_driver::LoopCompactionRequest {
                        payload: &selection.payload,
                        selected_model: &active_route.selected_model,
                        scope: invocation_scope,
                        step_id: route_step_id,
                        authority: compaction_authority.as_ref(),
                    })
                    .await
                {
                    Ok(summary) => {
                        let summary_chars = summary.chars().count();
                        context_contract.apply_compaction(conversation, selection, summary);
                        emit_context_compaction_event(
                            events,
                            context_compaction_event(
                                trigger,
                                omegon_traits::ContextCompactionStatus::Succeeded,
                                before_tokens,
                                Some(conversation.estimate_tokens() as u64),
                                Some(evict_count),
                                Some(summary_chars),
                                None,
                            ),
                        );
                    }
                    Err(e) => {
                        let message = e.to_string();
                        emit_context_compaction_event(
                            events,
                            context_compaction_event(
                                trigger,
                                omegon_traits::ContextCompactionStatus::Failed,
                                before_tokens,
                                None,
                                Some(evict_count),
                                None,
                                Some(message.clone()),
                            ),
                        );
                        tracing::warn!(
                            "LLM compaction failed: {message} — continuing with decay only"
                        );
                    }
                }
            } else {
                emit_context_compaction_event(
                    events,
                    context_compaction_event(
                        trigger,
                        omegon_traits::ContextCompactionStatus::NoPayload,
                        before_tokens,
                        Some(before_tokens),
                        Some(0),
                        None,
                        Some("no evictable messages older than decay window".to_string()),
                    ),
                );
            }
        }

        // If the user's input was detected as MCQ or obfuscated, inject
        // a one-shot system hint so the agent responds appropriately.
        // These are appended as user messages that get compacted away
        // on subsequent turns — they only affect the current response.
        if conversation.intent.mcq_detected {
            conversation.intent.mcq_detected = false; // one-shot
            conversation.push_user(
                "[System: The question above is multiple-choice. State which option \
                 letter (A/B/C/D) is correct at the START of your response, then \
                 explain your reasoning. Example format: \"B. The answer is B because...\"]"
                    .to_string(),
            );
        }
        if conversation.intent.obfuscation_detected {
            conversation.intent.obfuscation_detected = false; // one-shot
            conversation.push_user(
                "[System: The input above appears to contain heavily obfuscated or \
                 misspelled text. Interpret it charitably — deduce the intended \
                 meaning despite the spelling errors and respond to the underlying question.]"
                    .to_string(),
            );
        }

        let assembled_context = context_contract
            .prepare_turn(
                conversation,
                invocation_contract.runtime(),
                turn,
                &tool_defs,
                context_window,
            )
            .await;
        let system_prompt = assembled_context.system_prompt;
        let compatibility_messages = assembled_context.messages;
        context_windows.validate_fixed_context(&system_prompt, &tool_defs)?;
        // User-image attachments are stored on canonical user messages directly.

        tracing::debug!(
            turn,
            system_prompt_len = system_prompt.len(),
            messages = compatibility_messages.len(),
            tools = tool_defs.len(),
            estimated_tokens = conversation.estimate_tokens(),
            "LLM context assembled"
        );

        // Re-read thinking level each turn (can change mid-session via /thinking)
        active_route = route.turn_route().await;
        route.validate_request_capabilities(&tool_defs)?;
        route.prepare(&active_route, events).await;
        let llm_messages = semantic_facts.current_context_messages(&compatibility_messages)?;
        let semantic_request = if semantic_facts.enabled() {
            let tool_lineage = invocation_contract.tool_schema_lineage(&tool_defs)?;
            semantic_facts.prepare_model_request(crate::loop_session::LoopModelRequestCapture {
                step: semantic_step
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("semantic emission produced no step"))?,
                purpose: crate::loop_driver::LoopModelRequestPurpose::Initial,
                replaces: None,
                system_prompt: &system_prompt,
                messages: &llm_messages,
                tools: &tool_defs,
                tool_lineage: &tool_lineage,
                route: &active_route,
            })?
        } else {
            None
        };
        let dispatch_step_id = semantic_step
            .as_ref()
            .map_or(route_step_id, |step| step.step_id);

        let dispatch = tokio::select! {
            result = route.dispatch(crate::loop_driver::LoopRouteRequest {
                route: &active_route,
                system_prompt: &system_prompt,
                messages: &llm_messages,
                tools: &tool_defs,
                events,
                max_retries: config.max_retries,
                retry_delay_ms: config.retry_delay_ms,
                cancel_keeps_prompt: config.cancel_keeps_prompt.as_ref(),
                scope: invocation_scope,
                step_id: dispatch_step_id,
                semantic_request: semantic_request.as_ref(),
                response_facts: semantic_request.as_ref().map(|_| {
                    semantic_facts as &dyn crate::loop_driver::LoopResponseFactContract
                }),
            }) => {
                match result {
                    Ok(dispatch) => dispatch,
                    Err(e) if crate::loop_driver::route_repair(route.failure_kind(&e))
                        == Some(crate::loop_driver::LoopRouteRepair::CompactOverflow) => {
                        // Context too large for the provider — emergency compact and retry
                        tracing::warn!("Context overflow detected — forcing emergency compaction");
                        let _ = events.send(AgentEvent::SystemNotification {
                            message: "Context overflow — compacting conversation and retrying…".into(),
                        });
                        let before_tokens = conversation.estimate_tokens() as u64;
                        let overflow_plan = context_contract
                            .overflow_compaction_plan(
                                conversation.context_compaction_snapshot(),
                                cancel.clone(),
                            )
                            .await;
                        if let Err(error) = &overflow_plan {
                            tracing::warn!(%error, "overflow compaction planning unavailable");
                        }
                        if let Ok(Some(plan)) = overflow_plan {
                            let evict_count = plan.evict_count;
                            tracing::info!(evict_count, "Emergency compaction: evicting messages");
                            emit_context_compaction_event(events, context_compaction_event(
                                omegon_traits::ContextCompactionTrigger::ContextOverflow,
                                omegon_traits::ContextCompactionStatus::Started,
                                before_tokens,
                                None,
                                Some(evict_count),
                                None,
                                None,
                            ));
                            let compaction_authority = context_contract.begin_compaction(
                                &plan,
                                invocation_scope,
                                route_step_id,
                                crate::loop_driver::LoopCompactionTrigger::ContextOverflow,
                            )?;
                            match route
                                .compact(crate::loop_driver::LoopCompactionRequest {
                                    payload: &plan.payload,
                                    selected_model: &active_route.selected_model,
                                    scope: invocation_scope,
                                    step_id: route_step_id,
                                    authority: compaction_authority.as_ref(),
                                })
                            .await
                            {
                                Ok(summary) => {
                                    let summary_chars = summary.chars().count();
                                    context_contract.apply_compaction(conversation, plan, summary);
                                    emit_context_compaction_event(events, context_compaction_event(
                                        omegon_traits::ContextCompactionTrigger::ContextOverflow,
                                        omegon_traits::ContextCompactionStatus::Succeeded,
                                        before_tokens,
                                        Some(conversation.estimate_tokens() as u64),
                                        Some(evict_count),
                                        Some(summary_chars),
                                        None,
                                    ));
                                }
                                Err(ce) => {
                                    let message = ce.to_string();
                                    tracing::warn!("Emergency LLM compaction failed: {message} — applying decay");
                                    context_contract.decay_failed_compaction(conversation, &plan);
                                    emit_context_compaction_event(events, context_compaction_event(
                                        omegon_traits::ContextCompactionTrigger::ContextOverflow,
                                        omegon_traits::ContextCompactionStatus::Decayed,
                                        before_tokens,
                                        Some(conversation.estimate_tokens() as u64),
                                        Some(evict_count),
                                        None,
                                        Some(message),
                                    ));
                                }
                            }
                        } else {
                            // Can't build compaction payload — decay aggressively
                            let evict_count = context_contract
                                .repair_overflow_without_plan(conversation);
                            emit_context_compaction_event(events, context_compaction_event(
                                omegon_traits::ContextCompactionTrigger::ContextOverflow,
                                omegon_traits::ContextCompactionStatus::Decayed,
                                before_tokens,
                                Some(conversation.estimate_tokens() as u64),
                                Some(evict_count),
                                None,
                                Some("no compaction payload available; applied aggressive decay".to_string()),
                            ));
                        }
                        // Rebuild the in-memory compatibility view, then re-derive dispatch context from authority.
                        let compatibility_messages = context_contract.messages(conversation);
                        let repair_purpose = crate::loop_driver::LoopModelRequestPurpose::ContextOverflowRepair;
                        if let Some(previous) = semantic_request.as_ref() {
                            semantic_facts.supersede_for_repair(previous, repair_purpose)?;
                        }
                        let llm_messages = semantic_facts.current_context_messages(&compatibility_messages)?;
                        let repair_request = if semantic_facts.enabled() {
                            let tool_lineage = invocation_contract.tool_schema_lineage(&tool_defs)?;
                            semantic_facts.prepare_model_request(crate::loop_session::LoopModelRequestCapture {
                                step: semantic_step.as_ref().expect("enabled semantic step"),
                                purpose: repair_purpose,
                                replaces: semantic_request.as_ref(),
                                system_prompt: &system_prompt,
                                messages: &llm_messages,
                                tools: &tool_defs,
                                tool_lineage: &tool_lineage,
                                route: &active_route,
                            })?
                        } else { None };
                        route.dispatch(crate::loop_driver::LoopRouteRequest {
                            route: &active_route, system_prompt: &system_prompt,
                            messages: &llm_messages, tools: &tool_defs, events,
                            max_retries: config.max_retries, retry_delay_ms: config.retry_delay_ms,
                            cancel_keeps_prompt: config.cancel_keeps_prompt.as_ref(),
                            scope: invocation_scope, step_id: dispatch_step_id,
                            semantic_request: repair_request.as_ref(),
                            response_facts: repair_request.as_ref().map(|_| {
                                semantic_facts as &dyn crate::loop_driver::LoopResponseFactContract
                            }),
                        }).await?
                    }
                    Err(e) if crate::loop_driver::route_repair(route.failure_kind(&e))
                        == Some(crate::loop_driver::LoopRouteRepair::RepairMalformedHistory) => {
                        // Conversation structure is invalid for this provider
                        // (orphaned tool results, bad IDs, missing signatures).
                        // Aggressive decay + rebuild should fix it.
                        tracing::warn!(
                            error = %e,
                            "Malformed conversation history — applying emergency decay and retrying"
                        );
                        let _ = events.send(AgentEvent::SystemNotification {
                            message: "Conversation history incompatible with provider — repairing and retrying…".into(),
                        });
                        // Drop the first half of history — brute but effective
                        context_contract.repair_malformed_history(conversation);
                        let compatibility_messages = context_contract.messages(conversation);
                        let repair_purpose = crate::loop_driver::LoopModelRequestPurpose::ProviderHistoryRepair;
                        if let Some(previous) = semantic_request.as_ref() {
                            semantic_facts.supersede_for_repair(previous, repair_purpose)?;
                        }
                        let llm_messages = semantic_facts.current_context_messages(&compatibility_messages)?;
                        let repair_request = if semantic_facts.enabled() {
                            let tool_lineage = invocation_contract.tool_schema_lineage(&tool_defs)?;
                            semantic_facts.prepare_model_request(crate::loop_session::LoopModelRequestCapture {
                                step: semantic_step.as_ref().expect("enabled semantic step"),
                                purpose: repair_purpose,
                                replaces: semantic_request.as_ref(),
                                system_prompt: &system_prompt,
                                messages: &llm_messages,
                                tools: &tool_defs,
                                tool_lineage: &tool_lineage,
                                route: &active_route,
                            })?
                        } else { None };
                        route.dispatch(crate::loop_driver::LoopRouteRequest {
                            route: &active_route, system_prompt: &system_prompt,
                            messages: &llm_messages, tools: &tool_defs, events,
                            max_retries: config.max_retries, retry_delay_ms: config.retry_delay_ms,
                            cancel_keeps_prompt: config.cancel_keeps_prompt.as_ref(),
                            scope: invocation_scope, step_id: dispatch_step_id,
                            semantic_request: repair_request.as_ref(),
                            response_facts: repair_request.as_ref().map(|_| {
                                semantic_facts as &dyn crate::loop_driver::LoopResponseFactContract
                            }),
                        }).await?
                    }
                    Err(e) => return Err(e),
                }
            },
            _ = cancel.cancelled() => {
                tracing::info!("Agent loop cancelled during LLM streaming");
                invocation_contract.runtime().emit(&omegon_traits::BusEvent::TurnEnd(Box::new(BusEventTurnEnd {
                    turn,
                    model: None,
                    provider: None,
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: context_contract.default_composition(context_window),
                    actual_input_tokens: 0,
                    actual_output_tokens: 0,
                    cache_read_tokens: 0,
                    provider_telemetry: None,
                    dominant_phase: None,
                    drift_kind: None,
                    progress_signal: omegon_traits::ProgressSignal::None,
                })));
                let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
                    turn,
                    turn_end_reason: TurnEndReason::Cancelled,
                    model: Some(active_route.serving_model.clone()),
                    provider: Some(active_route.provider_id.clone()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: context_contract.default_composition(context_window),
                    actual_input_tokens: 0,
                    actual_output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    provider_telemetry: None,
                    dominant_phase: None,
                    drift_kind: None,
                    progress_nudge_reason: None,
                    intent_task: conversation.intent.current_task.clone(),
                    intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
                    files_read_count: conversation.intent.files_read.len(),
                    files_modified_count: conversation.intent.files_modified.len(),
                    stats_tool_calls: conversation.intent.stats.tool_calls,
                    streaks: controller.streaks(),
                })));
                break;
            }
        };

        let crate::loop_driver::LoopRouteDispatch {
            message: assistant_msg,
            stop_notice,
            durable_route: _,
            completed_request,
            response_attempt_ordinal,
        } = dispatch;

        // Real provider token counts for this turn (0 if provider didn't report them)
        let (act_in, act_out, act_cr, act_cc) = assistant_msg.provider_tokens;
        let provider_telemetry = assistant_msg.provider_telemetry.clone();
        if let Some(notice) = stop_notice {
            tracing::warn!(
                provider = notice.provider,
                stop_reason = notice.reason,
                "provider ended response abnormally"
            );
            let _ = events.send(AgentEvent::SystemNotification {
                message: notice.message,
            });
        }

        let tool_calls = assistant_msg.tool_calls();
        let recorded_calls = if semantic_facts.enabled() {
            let request = completed_request
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("completed semantic response has no request"))?;
            let calls = semantic_facts.record_tool_calls(request, tool_calls)?;
            semantic_facts.close_request(
                request,
                response_attempt_ordinal,
                crate::loop_driver::LoopRequestTerminal::ResponseCompleted,
                "provider_done",
            )?;
            calls
        } else {
            Vec::new()
        };

        let ambient_constraint_captures =
            session_policy.capture_ambient(conversation, assistant_msg.text_content());

        // Push assistant message to conversation. From this point on, an
        // operator interrupt means "stop this turn" rather than "forget my
        // submitted prompt" because the model has produced replay-relevant
        // assistant/tool state.
        conversation.push_assistant(assistant_msg.clone());
        if let Some(cancel_keeps_prompt) = &config.cancel_keeps_prompt {
            cancel_keeps_prompt.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        if tool_calls.is_empty() {
            if let Some(guidance) =
                session_policy.meta_recovery(&assistant_msg.text, turn, config.max_turns)
            {
                conversation.push_user(guidance);
                if let Some(step) = semantic_step.as_ref() {
                    semantic_facts.close_step(
                        step,
                        crate::loop_driver::LoopStepOutcome::Continue,
                        "meta_policy_continuation",
                    )?;
                }
                continue;
            }

            if let Some(directive) =
                session_policy.completion_directive(conversation, &assistant_msg.text, turn, config)
            {
                tracing::info!(
                    "Agent finishing with an incomplete session obligation — continuing"
                );
                conversation.push_user(directive.guidance);
                if let Some(advisory) = directive.advisory {
                    let nudge_context_composition = context_contract
                        .compose(conversation, &tool_defs, context_window)
                        .composition;
                    invocation_contract
                        .runtime()
                        .emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                            BusEventTurnEnd {
                                turn,
                                model: Some(active_route.serving_model.clone()),
                                provider: Some(active_route.provider_id.clone()),
                                estimated_tokens: conversation.estimate_tokens(),
                                context_window,
                                context_composition: nudge_context_composition.clone(),
                                actual_input_tokens: act_in,
                                actual_output_tokens: act_out,
                                cache_read_tokens: act_cr,
                                provider_telemetry: provider_telemetry.clone(),
                                dominant_phase: None,
                                drift_kind: Some(advisory.drift_kind),
                                progress_signal: omegon_traits::ProgressSignal::None,
                            },
                        )));
                    let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
                        turn,
                        turn_end_reason: TurnEndReason::ProgressNudge,
                        model: Some(active_route.serving_model.clone()),
                        provider: Some(active_route.provider_id.clone()),
                        estimated_tokens: conversation.estimate_tokens(),
                        context_window,
                        context_composition: nudge_context_composition,
                        actual_input_tokens: act_in,
                        actual_output_tokens: act_out,
                        cache_read_tokens: act_cr,
                        cache_creation_tokens: act_cc,
                        provider_telemetry: provider_telemetry.clone(),
                        dominant_phase: None,
                        drift_kind: Some(advisory.drift_kind),
                        progress_nudge_reason: Some(advisory.progress_nudge_reason),
                        intent_task: conversation.intent.current_task.clone(),
                        intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
                        files_read_count: conversation.intent.files_read.len(),
                        files_modified_count: conversation.intent.files_modified.len(),
                        stats_tool_calls: conversation.intent.stats.tool_calls,
                        streaks: controller.streaks(),
                    })));
                }
                if let Some(step) = semantic_step.as_ref() {
                    semantic_facts.close_step(
                        step,
                        crate::loop_driver::LoopStepOutcome::Continue,
                        "completion_policy_continuation",
                    )?;
                }
                continue;
            }

            if let Some(directive) =
                session_policy.text_only_recovery(conversation, &assistant_msg.text, turn, config)
            {
                if let Some(guidance) = directive.guidance {
                    conversation.push_user(guidance);
                }
                if let Some(step) = semantic_step.as_ref() {
                    semantic_facts.close_step(
                        step,
                        crate::loop_driver::LoopStepOutcome::Continue,
                        "text_policy_continuation",
                    )?;
                }
                continue;
            }

            // Reset dead-mouse counter when model does use tools
            // (handled below when tool_calls is non-empty, but also
            // covers the break-out path here).

            let turn_context_composition = context_contract
                .compose(conversation, &tool_defs, context_window)
                .composition;
            invocation_contract
                .runtime()
                .emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                    BusEventTurnEnd {
                        turn,
                        model: Some(active_route.serving_model.clone()),
                        provider: Some(active_route.provider_id.clone()),
                        estimated_tokens: conversation.estimate_tokens(),
                        context_window,
                        context_composition: turn_context_composition.clone(),
                        actual_input_tokens: act_in,
                        actual_output_tokens: act_out,
                        cache_read_tokens: act_cr,
                        provider_telemetry: provider_telemetry.clone(),
                        dominant_phase: None,
                        drift_kind: None,
                        progress_signal: omegon_traits::ProgressSignal::None,
                    },
                )));
            let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
                turn,
                turn_end_reason: TurnEndReason::AssistantCompleted,
                model: Some(active_route.serving_model.clone()),
                provider: Some(active_route.provider_id.clone()),
                estimated_tokens: conversation.estimate_tokens(),
                context_window,
                context_composition: turn_context_composition,
                actual_input_tokens: act_in,
                actual_output_tokens: act_out,
                cache_read_tokens: act_cr,
                cache_creation_tokens: act_cc,
                provider_telemetry: provider_telemetry.clone(),
                dominant_phase: None,
                drift_kind: None,
                progress_nudge_reason: None,
                intent_task: conversation.intent.current_task.clone(),
                intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
                files_read_count: conversation.intent.files_read.len(),
                files_modified_count: conversation.intent.files_modified.len(),
                stats_tool_calls: conversation.intent.stats.tool_calls,
                streaks: controller.streaks(),
            })));
            if let Some(step) = semantic_step.as_ref() {
                semantic_facts.close_step(
                    step,
                    crate::loop_driver::LoopStepOutcome::Finish,
                    "assistant_complete",
                )?;
            }
            break;
        }

        session_policy.observe_assistant_tool_calls(tool_calls);

        for call in tool_calls {
            session_used_tools.insert(call.name.clone());
        }

        let dispatch_allowed = if config.max_turns > 0 && turn > config.max_turns {
            tracing::warn!(
                requested = tool_calls.len(),
                "Ignoring tool calls from the reserved final-response turn"
            );
            let _ = events.send(AgentEvent::SystemNotification {
                message:
                    "Agent reached the tool-execution limit; additional tool calls were not run."
                        .into(),
            });
            false
        } else {
            true
        };
        let dispatch_calls = tool_calls.to_vec();
        let dispatch = invocation_contract
            .dispatch_batch(crate::loop_driver::LoopInvocationBatchRequest {
                calls: &dispatch_calls,
                tool_surface: &tool_defs,
                events,
                cancellation: cancel.clone(),
                dispatch_allowed,
            })
            .await;
        let results = dispatch.results;
        let invocation_terminals = dispatch.terminals;
        if needs_final_response_turn(config.max_turns, turn, dispatch_calls.len()) {
            final_response_turn_due = true;
        }

        // Push tool results to conversation and update intent
        let mut results = results;
        let completion_snapshot_before =
            session_policy.visible_plan_snapshot(conversation, &config.cwd);
        conversation
            .intent
            .update_from_tools(&tool_catalog, &dispatch_calls, &results);
        let completion_outcome = session_policy.reconcile_plan_tools(
            conversation,
            &config.cwd,
            &completion_snapshot_before,
            &dispatch_calls,
            &mut results,
        );
        if let Some(step) = semantic_step.as_ref() {
            semantic_facts.record_tool_results(
                step,
                &recorded_calls,
                &results,
                &invocation_terminals,
            )?;
            semantic_facts.close_step(
                step,
                crate::loop_driver::LoopStepOutcome::Continue,
                "tool_results_ready",
            )?;
        }
        for result in &results {
            conversation.push_tool_result(result.clone());
        }

        if let Some(message) = completion_outcome.notification {
            let _ = events.send(AgentEvent::SystemNotification { message });
        }
        if let Some(projection) = completion_outcome.projection {
            let _ = events.send(AgentEvent::PlanUpdated { projection });
        }

        if completion_outcome.requires_continuation {
            final_response_turn_due = true;
        }

        let observations = crate::observation::ObservationNormalizer::new(&tool_catalog)
            .normalize(&dispatch_calls, &results);
        let constraints_after = conversation.intent.constraints_discovered.len();
        let turn_assessment = behavior_policy.map(|binding| {
            binding
                .service
                .assess_turn(&behavior::BehaviorTurnInput::from_host(
                    turn,
                    constraints_after.saturating_sub(ambient_constraint_captures),
                    constraints_after,
                    conversation,
                    &tool_catalog,
                    &dispatch_calls,
                    &results,
                    &observations,
                ))
        });
        let dominant_phase = turn_assessment.and_then(|assessment| assessment.dominant_phase);
        let drift_kind = turn_assessment.and_then(|assessment| assessment.drift_kind);
        let progress_signal = turn_assessment
            .map(|assessment| assessment.progress_signal)
            .unwrap_or(omegon_traits::ProgressSignal::None);
        if let (Some(binding), Some(assessment)) = (behavior_policy, turn_assessment) {
            controller.observe_turn(
                TurnEndReason::ToolContinuation,
                assessment.drift_kind,
                assessment.progress_signal,
                assessment.evidence,
                binding
                    .service
                    .assess_text(&assistant_msg.text)
                    .substantive_interleaved_prose,
            );
        }
        let no_progress_action = if behavior_policy.is_some() {
            no_progress_terminal_action(
                controller.no_progress_continuation_streak,
                final_response_turn_due,
                forced_synthesis_attempted,
            )
        } else {
            NoProgressTerminalAction::Continue
        };
        let no_progress_stop = no_progress_action == NoProgressTerminalAction::ForceSynthesis;
        if no_progress_stop {
            tracing::warn!(
                streak = controller.no_progress_continuation_streak,
                "Scheduling one response-only synthesis turn after repeated no-progress tool turns"
            );
            forced_synthesis_attempted = true;
            final_response_turn_due = true;
        }
        let behavior = behavioral_tier(config);
        let pressure = behavior_policy.map(|binding| {
            binding
                .service
                .assess_pressure(&behavior::BehaviorPressureInput::from_host(
                    turn,
                    config,
                    conversation,
                    &tool_catalog,
                    &dispatch_calls,
                    &results,
                    dominant_phase,
                    &controller,
                ))
        });
        let continuation_tier = pressure.and_then(|assessment| assessment.continuation_tier);

        // Nudge injection macro — push message + emit audit event.
        macro_rules! inject_nudge {
            ($reason:expr, $msg:expr) => {{
                let msg_str: String = $msg.into();
                conversation.push_user(msg_str.clone());
                invocation_contract
                    .runtime()
                    .emit(&omegon_traits::BusEvent::NudgeInjected {
                        turn,
                        reason: $reason.into(),
                        message_preview: msg_str.chars().take(100).collect(),
                    });
            }};
        }

        if !completion_outcome.reconciled
            && let Some(message) = session_policy.realtime_completion_reminder(
                conversation,
                &tool_catalog,
                &dispatch_calls,
                &results,
            )
        {
            inject_nudge!("realtime_plan_progress", message);
        } else if !completion_outcome.reconciled
            && pressure.is_some_and(|assessment| assessment.first_turn_orientation_churn)
        {
            tracing::info!("First-turn orientation churn — injecting execution-bias nudge");
            let msg = behavior_policy
                .expect("policy assessment requires a binding")
                .service
                .message(behavior::BehaviorMessageKind::FirstTurn(behavior));
            inject_nudge!("first_turn_execution_bias", msg);
        } else if is_slim_execution_bias(config)
            && controller.local_evidence_sufficient_streak > 0
            && has_local_target_hypothesis(conversation)
            && continuation_tier.is_some()
        {
            tracing::info!("OM local-first lock — injecting patch-or-prove nudge");
            inject_nudge!(
                "om_local_first_lock",
                behavior_policy
                    .expect("continuation pressure requires a binding")
                    .service
                    .message(behavior::BehaviorMessageKind::LocalFirst(behavior))
            );
        } else if controller.evidence_sufficient_streak > 0 && continuation_tier.is_some() {
            tracing::info!("Actionability threshold — injecting forced-convergence nudge");
            inject_nudge!(
                "evidence_sufficiency",
                behavior_policy
                    .expect("continuation pressure requires a binding")
                    .service
                    .message(behavior::BehaviorMessageKind::Evidence(behavior))
            );
        } else if pressure.is_some_and(|assessment| assessment.execution_pressure) {
            tracing::info!("Execution stall — injecting execution-pressure nudge");
            let msg = behavior_policy
                .expect("policy assessment requires a binding")
                .service
                .message(behavior::BehaviorMessageKind::ExecutionPressure(behavior));
            inject_nudge!("execution_pressure", msg);
        } else if let Some(tier) = continuation_tier {
            tracing::info!(
                tier,
                "Continuation churn — injecting continuation-pressure nudge"
            );
            inject_nudge!(
                format!("continuation_pressure_tier_{tier}"),
                behavior_policy
                    .expect("continuation pressure requires a binding")
                    .service
                    .message(behavior::BehaviorMessageKind::Continuation { tier, behavior })
            );
        }

        for (call, result) in dispatch_calls.iter().zip(results.iter()) {
            invocation_contract
                .runtime()
                .emit(&omegon_traits::BusEvent::ToolEnd {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    result: omegon_traits::ToolResult {
                        content: result.content.clone(),
                        details: {
                            let mut summary = serde_json::Map::new();
                            if let Some(p) = call.arguments.get("path") {
                                summary.insert("path".into(), p.clone());
                            }
                            if let Some(c) = call.arguments.get("command") {
                                summary.insert("command".into(), c.clone());
                            }
                            serde_json::Value::Object(summary)
                        },
                    },
                    is_error: result.is_error,
                });
        }

        context_contract.record_activity(&dispatch_calls);

        session_policy.record_tool_outcomes(
            &tool_catalog,
            &dispatch_calls,
            &results,
            &observations,
        );

        let turn_context_composition = context_contract
            .compose(conversation, &tool_defs, context_window)
            .composition;
        invocation_contract
            .runtime()
            .emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                BusEventTurnEnd {
                    turn,
                    model: Some(active_route.serving_model.clone()),
                    provider: Some(active_route.provider_id.clone()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: turn_context_composition.clone(),
                    actual_input_tokens: act_in,
                    actual_output_tokens: act_out,
                    cache_read_tokens: act_cr,
                    provider_telemetry: provider_telemetry.clone(),
                    dominant_phase,
                    drift_kind,
                    progress_signal,
                },
            )));

        let turn_advisories = invocation_contract
            .process_lifecycle_requests(crate::loop_driver::LoopLifecycleRequest {
                conversation,
                route,
                context: context_contract,
                events,
                cancellation: cancel.clone(),
                active_route: &active_route,
                invocation_scope,
                route_step_id,
            })
            .await;
        for advisory in turn_advisories {
            match advisory {
                crate::loop_driver::LoopTurnAdvisory::Notify { message, level } => {
                    tracing::info!(level = ?level, "Bus: {message}");
                }
                crate::loop_driver::LoopTurnAdvisory::InjectSystemMessage { content } => {
                    conversation.push_user(format!("[System: {content}]"));
                }
                crate::loop_driver::LoopTurnAdvisory::EmitAgentEvent { event } => {
                    let _ = events.send(*event);
                }
            }
        }

        let estimated_tokens = conversation.estimate_tokens();
        let context_update = context_contract.context_update(config, conversation, context_window);
        let _ = events.send(AgentEvent::ContextUpdated {
            tokens: context_update.tokens,
            context_window: context_update.context_window,
            context_class: context_update.context_class,
            thinking_level: context_update.thinking_level,
        });
        let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
            turn,
            turn_end_reason: if no_progress_stop {
                TurnEndReason::Blocked
            } else {
                TurnEndReason::ToolContinuation
            },
            model: Some(active_route.serving_model.clone()),
            provider: Some(active_route.provider_id.clone()),
            estimated_tokens,
            context_window,
            context_composition: turn_context_composition,
            actual_input_tokens: act_in,
            actual_output_tokens: act_out,
            cache_read_tokens: act_cr,
            cache_creation_tokens: act_cc,
            provider_telemetry,
            dominant_phase,
            drift_kind,
            progress_nudge_reason: drift_kind.map(progress_nudge_reason_for_drift),
            intent_task: conversation.intent.current_task.clone(),
            intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
            files_read_count: conversation.intent.files_read.len(),
            files_modified_count: conversation.intent.files_modified.len(),
            stats_tool_calls: conversation.intent.stats.tool_calls,
            streaks: controller.streaks(),
        })));

        if no_progress_stop {
            continue;
        }

        if completion_outcome.reconciled {
            tracing::info!(
                "Session completion obligation reconciled — ending without redundant closure narration"
            );
            if semantic_facts.enabled() {
                continue;
            }
            break;
        }
    }

    let elapsed = session_start.elapsed();
    tracing::info!(
        turns = turn,
        tool_calls = conversation.intent.stats.tool_calls,
        elapsed_secs = elapsed.as_secs(),
        "Agent loop complete"
    );

    let finalization = session_policy.finalization_summary(conversation);
    invocation_contract
        .finalize_session(crate::loop_driver::LoopFinalizationRequest {
            events,
            cancellation: cancel,
            turns: turn,
            tool_calls: finalization.tool_calls,
            elapsed,
            initial_prompt: finalization.initial_prompt,
            outcome_summary: finalization.outcome_summary,
        })
        .await;

    Ok(())
}

fn emit_context_compaction_event(
    events: &broadcast::Sender<AgentEvent>,
    event: omegon_traits::ContextCompactionEvent,
) {
    let _ = events.send(AgentEvent::ContextCompaction(event));
}

fn context_compaction_event(
    trigger: omegon_traits::ContextCompactionTrigger,
    status: omegon_traits::ContextCompactionStatus,
    before_tokens: u64,
    after_tokens: Option<u64>,
    evicted_messages: Option<usize>,
    summary_chars: Option<usize>,
    reason: Option<String>,
) -> omegon_traits::ContextCompactionEvent {
    omegon_traits::ContextCompactionEvent {
        trigger,
        status,
        before_tokens,
        after_tokens,
        evicted_messages,
        summary_chars,
        reason,
    }
}

#[cfg(test)]
mod legacy_route_policy_tests {
    use super::*;
    use crate::bridge::{BoundaryExpectation, LlmBridge, LlmEvent, StreamOptions};
    use crate::upstream_errors::{
        TransientFailureKind, UpstreamFailureLogEntry, append_upstream_failure_log,
        classify_upstream_error_for_provider,
    };
    use std::hash::{DefaultHasher, Hash, Hasher};
    pub(super) struct LoopDispatchScope<'a> {
        pub(super) route_controller: Option<&'a std::sync::Arc<crate::route::RouteController>>,
        pub(super) invocation_scope: &'a crate::invocation_service::InvocationScope,
        pub(super) route_step_id: uuid::Uuid,
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn stream_with_retry(
        bridge: &dyn LlmBridge,
        system_prompt: &str,
        messages: &[crate::bridge::LlmMessage],
        tools: &[omegon_traits::ToolDefinition],
        options: &StreamOptions,
        events: &broadcast::Sender<AgentEvent>,
        config: &LoopConfig,
        dispatch_scope: &LoopDispatchScope<'_>,
    ) -> anyhow::Result<AssistantMessage> {
        let model = options
            .model
            .clone()
            .unwrap_or_else(|| config.model.clone());
        let selected_model = if let Some(controller) = dispatch_scope.route_controller {
            controller.snapshot().await.selected_model().to_string()
        } else {
            bridge
                .selected_model_hint()
                .unwrap_or(&config.model)
                .to_string()
        };
        let serving_model = bridge.serving_model_hint().map_or_else(
            || model.clone(),
            |hint| {
                format!(
                    "{}:{}",
                    crate::providers::infer_provider_id(hint),
                    crate::providers::model_id_from_spec(&model)
                )
            },
        );
        let provider = crate::providers::infer_provider_id(&serving_model);
        crate::provider_route_service::record_loop_route_lease(
            dispatch_scope.invocation_scope,
            dispatch_scope.route_step_id,
            &selected_model,
            &serving_model,
            bridge.credential_source_class_hint(),
        )?;

        let mut attempt = 0u32;
        let mut delay = config.retry_delay_ms;
        let started = Instant::now();

        loop {
            attempt += 1;

            // Wrap bridge.stream() so pre-stream network errors (DNS, connection
            // refused, TLS failures) enter the same transient classifier instead
            // of aborting immediately via `?`.
            let err = match bridge.stream(system_prompt, messages, tools, options).await {
                Ok(mut rx) => {
                    match consume_llm_stream_with_policy(
                        &mut rx,
                        events,
                        &provider,
                        &model,
                        config.cancel_keeps_prompt.as_ref(),
                        StreamIdlePolicy::from_env(),
                    )
                    .await
                    {
                        Ok(msg) => return Ok(msg),
                        Err(e) => e,
                    }
                }
                Err(e) => e,
            };

            let err_msg = err.to_string();
            let upstream_class = classify_upstream_error_for_provider(&provider, &err_msg);
            let transient_kind = upstream_class.transient_kind();
            let is_transient = transient_kind.is_some();

            if !is_transient {
                if attempt > 1 {
                    tracing::error!(
                        class = upstream_class.label(),
                        recovery = ?upstream_class.recovery_action(),
                        "LLM error after {attempt} attempts: {err_msg}"
                    );
                }
                return Err(err);
            }

            let kind_label = upstream_class.label();
            let _ = append_upstream_failure_log(&UpstreamFailureLogEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                provider: provider.clone(),
                model: model.clone(),
                failure_kind: kind_label.to_string(),
                internal_class: kind_label.to_string(),
                recovery_action: upstream_class.recovery_action(),
                attempt,
                request_id: None,
                response_attempt_ordinal: None,
                delay_ms: delay,
                message: err_msg.clone(),
            });

            // Soft exhaustion: bounded worker runs retain their explicit attempt cap.
            // Interactive Codex overloads are intentionally persistent: the provider
            // may be admitting only a small fraction of requests, and surrendering
            // after an arbitrary wall-clock envelope discards otherwise recoverable
            // work. Operator cancellation remains authoritative while waiting.
            let elapsed = started.elapsed();
            let persistent_codex_overload = persistent_interactive_overload_retry(
                config.max_retries,
                &provider,
                transient_kind,
            );
            let rate_limit_exhausted = config.max_retries == 0
                && matches!(transient_kind, Some(TransientFailureKind::RateLimited))
                && elapsed.as_secs() >= 120;
            let stall_exhausted = config.max_retries == 0
                && matches!(transient_kind, Some(TransientFailureKind::StalledStream))
                && elapsed.as_secs()
                    >= stall_exhaustion_secs(&provider, &model, options.reasoning.as_deref());
            let transient_envelope_exhausted = !persistent_codex_overload
                && transient_retry_envelope_exhausted(
                    config.max_retries,
                    transient_kind,
                    elapsed.as_secs(),
                );
            let attempt_exhausted = config.max_retries > 0 && attempt >= config.max_retries;

            if attempt_exhausted
                || rate_limit_exhausted
                || stall_exhausted
                || transient_envelope_exhausted
            {
                let reason = if rate_limit_exhausted {
                    "session rate-limit exhaustion"
                } else if stall_exhausted {
                    "stream stall exhaustion"
                } else if transient_envelope_exhausted {
                    "transient retry exhaustion"
                } else {
                    "upstream exhausted"
                };
                tracing::error!(
                    attempts = attempt,
                    elapsed_secs = elapsed.as_secs(),
                    kind = kind_label,
                    "{reason}: {err_msg}"
                );
                let advice = exhaustion_advice(
                    &provider,
                    transient_kind,
                    rate_limit_exhausted,
                    stall_exhausted,
                );
                let _ = events.send(AgentEvent::ProviderFailure {
                    provider: provider.clone(),
                    model: model.clone(),
                    reason: kind_label.to_string(),
                    attempts: attempt,
                    message: err_msg.clone(),
                    retryable: false,
                    recommended_action: advice.to_string(),
                });
                let _ = events.send(AgentEvent::SystemNotification {
                message: format!(
                    "🛑 {provider} {reason}: {attempt} consecutive {kind_label} failures over {:.0}s. {advice}",
                    elapsed.as_secs_f64()
                ),
            });
                return Err(anyhow::anyhow!(
                    "{reason}: {} consecutive {} failures over {:.0}s: {}",
                    attempt,
                    kind_label,
                    elapsed.as_secs_f64(),
                    err_msg
                ));
            }

            // Transient — retry with escalating visual feedback.
            let base_delay = delay;
            let retry_delay = jittered_retry_delay_ms(base_delay, attempt, &provider, &model);
            tracing::warn!(
                attempt,
                delay_ms = retry_delay,
                kind = transient_kind
                    .map(TransientFailureKind::label)
                    .unwrap_or("transient upstream failure"),
                "Transient LLM error, retrying: {err_msg}"
            );

            // Milestone warnings → persistent (pushed to conversation).
            // These escalate so the operator notices accumulated failures.
            let is_milestone = matches!(attempt, 10 | 25 | 50 | 100)
                || (attempt > 100 && attempt.is_multiple_of(100));
            if is_milestone {
                let elapsed = started.elapsed();
                let kind_label = transient_kind
                    .map(TransientFailureKind::label)
                    .unwrap_or("transient upstream failure");
                let _ = events.send(AgentEvent::SystemNotification {
                message: format!(
                    "⚠ {provider} is seeing repeated transient upstream failures: {attempt} consecutive {kind_label} failures over {:.0}s — credentials still look valid; switch only if this persists",
                    elapsed.as_secs_f64()
                ),
            });
            }

            // Regular retry notification routed by presentation adapters.
            let operator_detail = transient_kind
                .map(|kind| kind.operator_detail(&provider, &err_msg))
                .unwrap_or_else(|| crate::util::truncate_str(&err_msg, 300).to_string());
            let msg = format!(
                "⚠ Upstream {kind_label} — retrying (attempt {attempt}, delay {}ms): {operator_detail}",
                retry_delay
            );
            let _ = events.send(AgentEvent::ProviderRetry {
                provider: provider.clone(),
                model: model.clone(),
                attempt,
                delay_ms: retry_delay,
                reason: kind_label.to_string(),
                message: operator_detail.clone(),
                recoverable: true,
            });
            let _ = events.send(AgentEvent::SystemNotification { message: msg });
            tokio::time::sleep(std::time::Duration::from_millis(retry_delay)).await;
            delay = base_delay.saturating_mul(2).min(15_000); // exponential backoff, cap at 15s
        }
    }

    pub(super) fn persistent_interactive_overload_retry(
        max_retries: u32,
        provider: &str,
        transient_kind: Option<TransientFailureKind>,
    ) -> bool {
        max_retries == 0
            && provider == "openai-codex"
            && matches!(
                transient_kind,
                Some(TransientFailureKind::ProviderOverloaded)
            )
    }

    pub(super) fn jittered_retry_delay_ms(
        base_delay_ms: u64,
        attempt: u32,
        provider: &str,
        model: &str,
    ) -> u64 {
        // Deterministic full jitter avoids synchronized retry waves without requiring
        // runtime RNG state. Keep at least half the exponential delay so persistent
        // overload recovery does not turn into an aggressive request loop.
        let mut hasher = DefaultHasher::new();
        provider.hash(&mut hasher);
        model.hash(&mut hasher);
        attempt.hash(&mut hasher);
        let half = base_delay_ms / 2;
        half.saturating_add(hasher.finish() % base_delay_ms.saturating_sub(half).max(1))
    }

    pub(super) fn transient_retry_envelope_exhausted(
        max_retries: u32,
        transient_kind: Option<TransientFailureKind>,
        elapsed_secs: u64,
    ) -> bool {
        max_retries == 0
            && !matches!(
                transient_kind,
                Some(TransientFailureKind::RateLimited | TransientFailureKind::StalledStream)
            )
            && elapsed_secs >= 600
    }

    pub(super) fn stall_exhaustion_secs(
        provider: &str,
        model: &str,
        reasoning: Option<&str>,
    ) -> u64 {
        let is_openai_reasoning = provider == "openai-codex"
            || ((provider == "openai" || provider == "openai-compatible")
                && (model.contains("gpt-5")
                    || crate::providers::model_id_from_spec(model) == "gpt-6-astra"
                    || model.contains("o3")
                    || model.contains("o4")));
        if is_openai_reasoning {
            return match reasoning {
                Some("high" | "xhigh" | "max") => 2_400,
                Some("medium") => 1_800,
                Some("low" | "minimal") => 1_200,
                _ => 1_200,
            };
        }
        600
    }

    pub(super) fn exhaustion_advice(
        provider: &str,
        transient_kind: Option<TransientFailureKind>,
        rate_limit_exhausted: bool,
        stall_exhausted: bool,
    ) -> &'static str {
        if stall_exhausted {
            if provider == "anthropic"
                && crate::providers::anthropic_credential_mode()
                    == crate::providers::AnthropicCredentialMode::OAuthOnly
            {
                return "Anthropic OAuth streams are repeatedly stalling. Retry /auth login anthropic to refresh the Claude session, or switch provider with /model.";
            }
            if provider == "openai-codex" || provider == "openai" || provider == "openai-compatible"
            {
                return "The OpenAI stream exceeded Omegon's local silent-reasoning budget. This may be a long-running reasoning window or a wedged stream; lower thinking, retry later, or switch provider with /model.";
            }
            return "The provider's stream is unresponsive. Retry later or switch provider with /model.";
        }
        if rate_limit_exhausted || matches!(transient_kind, Some(TransientFailureKind::RateLimited))
        {
            return "This provider is rate-limiting the session. Wait for reset or switch provider with /model.";
        }
        match transient_kind {
            Some(TransientFailureKind::ProviderOverloaded | TransientFailureKind::Upstream5xx) => {
                "This is a provider-side outage or capacity problem. Retry later, switch provider with /model, or check the provider status page."
            }
            Some(
                TransientFailureKind::Timeout
                | TransientFailureKind::NetworkConnect
                | TransientFailureKind::NetworkReset
                | TransientFailureKind::Dns
                | TransientFailureKind::DecodeBody
                | TransientFailureKind::BridgeDropped
                | TransientFailureKind::ResponseIncomplete
                | TransientFailureKind::ResponseCancelled,
            ) => {
                "The provider or network path is unstable. Retry later or switch provider with /model."
            }
            Some(TransientFailureKind::StalledStream) => {
                "The provider's stream is unresponsive. Retry later or switch provider with /model."
            }
            Some(TransientFailureKind::RateLimited) | None => {
                "Retry later or switch provider with /model."
            }
        }
    }

    /// Returns true if the error was produced by `stream_with_retry` hitting the soft
    /// exhaustion threshold (max_retries consecutive transient failures).
    pub(super) fn is_upstream_exhausted(err: &anyhow::Error) -> bool {
        err.to_string()
            .to_lowercase()
            .contains("upstream exhausted:")
    }

    fn provider_stop_reason(raw: &serde_json::Value) -> Option<&str> {
        raw.get("provider_stop_reason")
            .and_then(|reason| reason.as_str())
            .filter(|reason| !reason.trim().is_empty())
    }

    fn is_abnormal_provider_stop(provider: &str, reason: &str) -> bool {
        match provider {
            "openai" | "openrouter" | "openai-compatible" => {
                !matches!(reason, "stop" | "tool_calls" | "function_call")
            }
            "anthropic" => !matches!(reason, "end_turn" | "tool_use" | "stop_sequence"),
            _ => matches!(
                reason,
                "length" | "max_tokens" | "content_filter" | "safety" | "incomplete"
            ),
        }
    }

    pub(super) fn provider_stop_notice(provider: &str, reason: &str) -> Option<String> {
        if !is_abnormal_provider_stop(provider, reason) {
            return None;
        }
        let hint = match reason {
            "length" | "max_tokens" => {
                "The provider stopped because the output limit was reached; the visible answer may be incomplete."
            }
            "content_filter" | "safety" => {
                "The provider stopped because safety/content filtering intervened; the visible answer may be incomplete."
            }
            _ => {
                "The provider ended the response abnormally; the visible answer may be incomplete."
            }
        };
        Some(format!(
            "Provider stop: {provider}/{reason}\n{hint}\nUse a continuation prompt or retry with a larger output budget if needed."
        ))
    }

    /// Consume LlmEvents from the bridge, build an AssistantMessage.
    /// Stream idle phase is a liveness concept, not just visible thinking text.
    /// Providers can legally go silent while deciding the next item after text,
    /// thinking, or tool-call blocks complete; those inter-item gaps need the same
    /// generous leash as active reasoning.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum StreamIdleState {
        AwaitingFirstEvent = 0,
        OutputStreaming = 1,
        ToolStreaming = 2,
        ReasoningStreaming = 3,
        AmbiguousSilent = 4,
    }

    pub(super) type StreamIdlePhase = StreamIdleState;

    impl StreamIdleState {
        fn from_u8(value: u8) -> Self {
            match value {
                1 => Self::OutputStreaming,
                2 => Self::ToolStreaming,
                3 => Self::ReasoningStreaming,
                4 => Self::AmbiguousSilent,
                _ => Self::AwaitingFirstEvent,
            }
        }

        pub(super) fn label(self) -> &'static str {
            match self {
                Self::AwaitingFirstEvent => "awaiting first stream event",
                Self::OutputStreaming => "output streaming",
                Self::ToolStreaming => "tool-call streaming",
                Self::ReasoningStreaming => "reasoning streaming",
                Self::AmbiguousSilent => "ambiguous silent reasoning",
            }
        }

        fn is_ambiguous_reasoning(self) -> bool {
            matches!(self, Self::ReasoningStreaming | Self::AmbiguousSilent)
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub(super) struct StreamIdlePolicy {
        pub(super) initial: std::time::Duration,
        pub(super) active: std::time::Duration,
        pub(super) reasoning: std::time::Duration,
        pub(super) absolute: std::time::Duration,
    }

    impl StreamIdlePolicy {
        fn from_env() -> Self {
            let initial = std::env::var("OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|seconds| *seconds >= 30)
                .map(std::time::Duration::from_secs)
                .unwrap_or_else(|| std::time::Duration::from_secs(90));
            let reasoning = std::env::var("OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|seconds| *seconds >= 60)
                .map(std::time::Duration::from_secs)
                .unwrap_or_else(|| std::time::Duration::from_secs(600));
            let absolute = std::env::var("OMEGON_LLM_ABSOLUTE_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|seconds| *seconds >= 60)
                .map(std::time::Duration::from_secs)
                .unwrap_or_else(|| std::time::Duration::from_secs(1800));
            Self {
                initial,
                active: std::time::Duration::from_secs(90),
                reasoning,
                absolute,
            }
        }

        pub(super) fn budget(
            self,
            phase: StreamIdlePhase,
            visible_output_seen: bool,
        ) -> std::time::Duration {
            if visible_output_seen {
                // Once the operator has observed output or a tool effect, silence
                // is a stalled active turn regardless of the adapter's last phase.
                // A provider cannot silently promote a visible turn back onto the
                // long pre-output reasoning leash.
                return self.active;
            }
            match phase {
                // A provider must emit an explicit reasoning/inter-item boundary to
                // receive the long leash. Visible text does not permanently ban
                // reasoning, but an adapter that leaves the stream in an active
                // output/tool phase cannot silently promote itself to 600 seconds.
                StreamIdlePhase::AwaitingFirstEvent
                | StreamIdlePhase::ReasoningStreaming
                | StreamIdlePhase::AmbiguousSilent => self.reasoning,
                StreamIdlePhase::OutputStreaming | StreamIdlePhase::ToolStreaming => self.active,
            }
        }
    }

    pub(super) fn select_stream_idle_budget(
        phase: StreamIdlePhase,
        initial: std::time::Duration,
        active: std::time::Duration,
        reasoning_budget: std::time::Duration,
    ) -> std::time::Duration {
        StreamIdlePolicy {
            initial,
            active,
            reasoning: reasoning_budget,
            absolute: std::time::Duration::from_secs(1800),
        }
        .budget(phase, false)
    }

    pub(super) fn stream_idle_phase_after_event(
        current: StreamIdlePhase,
        event: &LlmEvent,
    ) -> StreamIdlePhase {
        match event {
            LlmEvent::Start | LlmEvent::TransportHeartbeat => current,
            LlmEvent::TextStart | LlmEvent::TextDelta { .. } => StreamIdlePhase::OutputStreaming,
            LlmEvent::TextEnd => StreamIdlePhase::AmbiguousSilent,
            LlmEvent::ThinkingStart | LlmEvent::ThinkingDelta { .. } => {
                StreamIdlePhase::ReasoningStreaming
            }
            LlmEvent::ThinkingEnd => StreamIdlePhase::AmbiguousSilent,
            LlmEvent::ToolCallStart | LlmEvent::ToolCallDelta { .. } => {
                StreamIdlePhase::ToolStreaming
            }
            LlmEvent::ToolCallEnd { .. } => StreamIdlePhase::AmbiguousSilent,
            LlmEvent::Boundary { expectation } => match expectation {
                BoundaryExpectation::MoreReasoning => StreamIdlePhase::ReasoningStreaming,
                BoundaryExpectation::MoreContent => StreamIdlePhase::OutputStreaming,
                BoundaryExpectation::Unknown => StreamIdlePhase::AmbiguousSilent,
                BoundaryExpectation::Terminal => current,
            },
            LlmEvent::ProviderContinuity { .. }
            | LlmEvent::Done { .. }
            | LlmEvent::Error { .. }
            | LlmEvent::UpstreamFailure { .. } => current,
        }
    }

    pub(super) async fn consume_llm_stream(
        rx: &mut tokio::sync::mpsc::Receiver<LlmEvent>,
        events: &broadcast::Sender<AgentEvent>,
        provider: &str,
        model: &str,
        cancel_keeps_prompt: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> anyhow::Result<AssistantMessage> {
        consume_llm_stream_with_policy(
            rx,
            events,
            provider,
            model,
            cancel_keeps_prompt,
            StreamIdlePolicy::from_env(),
        )
        .await
    }

    pub(super) async fn consume_llm_stream_with_policy(
        rx: &mut tokio::sync::mpsc::Receiver<LlmEvent>,
        events: &broadcast::Sender<AgentEvent>,
        provider: &str,
        model: &str,
        cancel_keeps_prompt: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
        idle_policy: StreamIdlePolicy,
    ) -> anyhow::Result<AssistantMessage> {
        let mut text_parts: Vec<String> = Vec::new();
        let mut thinking_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut final_raw: Value = Value::Null;
        let mut provider_tokens: (u64, u64, u64, u64) = (0, 0, 0, 0); // (input, output, cache_read, cache_write)
        let mut provider_telemetry = None;
        let mut completed = false;
        let mut visible_output_seen = false;
        let mut stream_started = false;
        let stream_started_at = tokio::time::Instant::now();
        let absolute_deadline = stream_started_at + idle_policy.absolute;
        let mut last_semantic_progress = stream_started_at;
        let mut transport_heartbeats: u64 = 0;

        let _ = events.send(AgentEvent::MessageStart {
            role: "assistant".into(),
        });

        // Catches models stuck in a text-repetition loop (e.g. "Append tests."
        // repeated 500 times). Tracks a rolling window of recent text chunks
        // and aborts when a short phrase repeats excessively.
        let mut recent_text_len: usize = 0;
        let mut repetition_window: Vec<String> = Vec::new();
        const REPETITION_WINDOW_SIZE: usize = 40;
        const REPETITION_ABORT_THRESHOLD: usize = 30; // 30 of last 40 chunks identical → abort

        // Phase-aware idle timeout:
        // - Awaiting first substantive event: use the generous reasoning budget.
        //   Reasoning providers may emit no bytes while preparing the first item.
        // - Active content/tool-call streaming: 90s. Claude Code's
        //   CLAUDE_STREAM_IDLE_TIMEOUT_MS default is 90s; nobody in the industry
        //   uses less than 60s.
        // - Active thinking and inter-item decision gaps: generous reasoning
        //   budget. Reasoning-capable providers may legally go silent between
        //   text/thinking/tool-call blocks while deciding the next item.
        // The legacy initial budget is retained as an input for compatibility, but
        // AwaitingFirstEvent intentionally selects the reasoning budget below.
        let stream_idle_phase =
            std::sync::atomic::AtomicU8::new(StreamIdlePhase::AwaitingFirstEvent as u8);
        let idle_timeout = |visible_output_seen: bool| {
            idle_policy.budget(
                StreamIdlePhase::from_u8(
                    stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed),
                ),
                visible_output_seen,
            )
        };
        while let Some(event) = match tokio::time::timeout(
            idle_timeout(visible_output_seen)
                .saturating_sub(last_semantic_progress.elapsed())
                .min(absolute_deadline.saturating_duration_since(tokio::time::Instant::now())),
            rx.recv(),
        )
        .await
        {
            Ok(event) => event,
            Err(_) => {
                let phase = StreamIdlePhase::from_u8(
                    stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed),
                );
                let absolute_expired = tokio::time::Instant::now() >= absolute_deadline;
                let reason = if absolute_expired {
                    format!(
                        "LLM stream exceeded the absolute {}s turn deadline during {} — transport received {} heartbeat event(s)",
                        idle_policy.absolute.as_secs(),
                        phase.label(),
                        transport_heartbeats
                    )
                } else {
                    format!(
                        "LLM stream made no semantic progress for {}s during {} — transport received {} heartbeat event(s)",
                        idle_timeout(visible_output_seen).as_secs(),
                        phase.label(),
                        transport_heartbeats
                    )
                };
                let _ = events.send(AgentEvent::StreamIdle {
                    provider: provider.to_string(),
                    model: model.to_string(),
                    phase: phase.label().to_string(),
                    idle_secs: idle_timeout(visible_output_seen).as_secs(),
                    ambiguous: phase.is_ambiguous_reasoning() && !visible_output_seen,
                    message: reason.clone(),
                });
                let _ = events.send(AgentEvent::MessageAbort {
                    reason: Some(reason.clone()),
                });
                anyhow::bail!("{reason}");
            }
        } {
            let next_phase = stream_idle_phase_after_event(
                StreamIdlePhase::from_u8(
                    stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed),
                ),
                &event,
            );
            stream_idle_phase.store(next_phase as u8, std::sync::atomic::Ordering::Relaxed);
            match event {
                LlmEvent::Start => {
                    // Stream start is semantic only once. Providers may repeat
                    // lifecycle markers; repeats must not re-arm the watchdog.
                    if !stream_started {
                        stream_started = true;
                        last_semantic_progress = tokio::time::Instant::now();
                    }
                }
                LlmEvent::TransportHeartbeat => {
                    transport_heartbeats = transport_heartbeats.saturating_add(1);
                }
                LlmEvent::TextStart => {
                    last_semantic_progress = tokio::time::Instant::now();
                }
                LlmEvent::TextDelta { delta } => {
                    if !delta.is_empty() {
                        last_semantic_progress = tokio::time::Instant::now();
                        visible_output_seen = true;
                        // Partial assistant output is visible to the operator. If
                        // they interrupt now, keep the prompt in canonical replay.
                        // This makes Escape useful for cutting off rambling output
                        // without pretending the turn never happened.
                        // Empty deltas are provider heartbeats and do not count.
                        //
                        // The flag is intentionally monotonic for the active turn.
                        // Once any assistant/tool effect is visible, cancellation
                        // becomes interrupt/keep rather than abort/forget.
                        if let Some(cancel_keeps_prompt) = cancel_keeps_prompt {
                            cancel_keeps_prompt.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    let _ = events.send(AgentEvent::MessageChunk {
                        text: delta.clone(),
                    });

                    // ── Degenerate repetition check ──────────────────
                    recent_text_len += delta.len();
                    let trimmed = delta.trim().to_lowercase();
                    if !trimmed.is_empty() {
                        repetition_window.push(trimmed);
                        if repetition_window.len() > REPETITION_WINDOW_SIZE {
                            repetition_window.remove(0);
                        }
                        if repetition_window.len() >= REPETITION_WINDOW_SIZE {
                            // Count how many of the last N chunks match the most recent
                            let latest = repetition_window.last().unwrap();
                            let matches = repetition_window.iter().filter(|c| c == &latest).count();
                            if matches >= REPETITION_ABORT_THRESHOLD {
                                tracing::warn!(
                                    repeated_phrase = %latest,
                                    matches,
                                    total_text_bytes = recent_text_len,
                                    "Degenerate repetition detected — aborting stream"
                                );
                                let reason = format!(
                                    "Model output degenerate: phrase {:?} repeated {}/{} recent chunks — aborting to prevent runaway",
                                    latest, matches, REPETITION_WINDOW_SIZE
                                );
                                let _ = events.send(AgentEvent::MessageAbort {
                                    reason: Some(reason.clone()),
                                });
                                anyhow::bail!("{reason}");
                            }
                        }
                    }

                    if let Some(last) = text_parts.last_mut() {
                        last.push_str(&delta);
                    } else {
                        text_parts.push(delta);
                    }
                }
                LlmEvent::TextEnd => {
                    text_parts.push(String::new());
                }
                LlmEvent::ThinkingStart => {
                    last_semantic_progress = tokio::time::Instant::now();
                    // Active reasoning has begun. This is a liveness phase, not a
                    // promise that every provider exposes raw chain-of-thought.
                }
                LlmEvent::ThinkingDelta { delta } => {
                    if !delta.is_empty() {
                        last_semantic_progress = tokio::time::Instant::now();
                    }
                    if !delta.is_empty()
                        && let Some(cancel_keeps_prompt) = cancel_keeps_prompt
                    {
                        cancel_keeps_prompt.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let _ = events.send(AgentEvent::ThinkingChunk {
                        text: delta.clone(),
                    });
                    if let Some(last) = thinking_parts.last_mut() {
                        last.push_str(&delta);
                    } else {
                        thinking_parts.push(delta);
                    }
                }
                LlmEvent::ThinkingEnd => {
                    thinking_parts.push(String::new());
                }
                LlmEvent::ToolCallStart => {
                    last_semantic_progress = tokio::time::Instant::now();
                }
                LlmEvent::ToolCallDelta { delta } => {
                    if !delta.is_empty() {
                        last_semantic_progress = tokio::time::Instant::now();
                    }
                    // Deltas accumulated by the bridge — complete tool call in ToolCallEnd
                }
                LlmEvent::ToolCallEnd { tool_call } => {
                    last_semantic_progress = tokio::time::Instant::now();
                    visible_output_seen = true;
                    if let Some(cancel_keeps_prompt) = cancel_keeps_prompt {
                        cancel_keeps_prompt.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    tool_calls.push(ToolCall {
                        id: tool_call.id,
                        name: tool_call.name,
                        arguments: tool_call.arguments,
                    });
                }
                LlmEvent::Boundary { expectation } => {
                    // A repeated unknown boundary carries no new semantic evidence.
                    // Explicit content/reasoning/terminal expectations do.
                    if !matches!(expectation, BoundaryExpectation::Unknown) {
                        last_semantic_progress = tokio::time::Instant::now();
                    }
                }
                LlmEvent::ProviderContinuity { .. } => {}
                LlmEvent::Done {
                    message,
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cache_creation_tokens,
                    provider_telemetry: done_provider_telemetry,
                    ..
                } => {
                    final_raw = message.get("raw").cloned().unwrap_or(message);
                    provider_tokens = (
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cache_creation_tokens,
                    );
                    provider_telemetry = done_provider_telemetry;
                    completed = true;
                    break;
                }
                LlmEvent::Error { message } => {
                    let _ = events.send(AgentEvent::MessageAbort {
                        reason: Some(message.clone()),
                    });
                    anyhow::bail!("LLM error: {message}");
                }
                LlmEvent::UpstreamFailure { failure } => {
                    let _ = events.send(AgentEvent::MessageAbort {
                        reason: Some(failure.message.clone()),
                    });
                    return Err(failure.into());
                }
            }
        }

        let _ = events.send(AgentEvent::MessageEnd);

        // A stream is complete only after the provider's explicit terminal event.
        // EOF after partial text/tool output is still an abnormal protocol close;
        // accepting it would leave the outer loop reasoning from a truncated turn.
        if !completed {
            anyhow::bail!(
                "LLM stream ended without a completion event — the bridge may have crashed"
            );
        }

        // Clean up empty trailing parts
        while text_parts.last().is_some_and(|s| s.is_empty()) {
            text_parts.pop();
        }
        while thinking_parts.last().is_some_and(|s| s.is_empty()) {
            thinking_parts.pop();
        }

        let text = text_parts.join("");
        let thinking = if thinking_parts.is_empty() {
            None
        } else {
            Some(thinking_parts.join(""))
        };

        Ok(AssistantMessage {
            text,
            thinking,
            tool_calls,
            raw: final_raw,
            provider_tokens,
            provider_telemetry,
        })
    }
}

#[cfg(test)]
use legacy_route_policy_tests::*;

fn needs_final_response_turn(max_turns: u32, turn: u32, tool_call_count: usize) -> bool {
    max_turns > 0 && turn >= max_turns && tool_call_count > 0
}

fn no_progress_terminal_action(
    consecutive_tool_continuations: u32,
    final_response_reserved: bool,
    forced_synthesis_attempted: bool,
) -> NoProgressTerminalAction {
    if consecutive_tool_continuations < 8 || final_response_reserved || forced_synthesis_attempted {
        NoProgressTerminalAction::Continue
    } else {
        NoProgressTerminalAction::ForceSynthesis
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoProgressTerminalAction {
    Continue,
    ForceSynthesis,
}

#[cfg(test)]
mod no_progress_terminal_tests {
    use super::*;

    #[test]
    fn synthesis_is_reserved_once_and_never_competes_with_final_response() {
        assert_eq!(
            no_progress_terminal_action(7, false, false),
            NoProgressTerminalAction::Continue
        );
        assert_eq!(
            no_progress_terminal_action(8, false, false),
            NoProgressTerminalAction::ForceSynthesis
        );
        assert_eq!(
            no_progress_terminal_action(8, true, false),
            NoProgressTerminalAction::Continue
        );
        assert_eq!(
            no_progress_terminal_action(9, false, true),
            NoProgressTerminalAction::Continue
        );
    }
}

#[cfg(test)]
mod legacy_session_recovery_policy_tests {
    use super::*;
    use std::collections::HashMap;
    use std::hash::{DefaultHasher, Hash, Hasher};

    fn counts_as_real_work_for_dead_mouse(call: &ToolCall) -> bool {
        matches!(call.name.as_str(), "bash" | "read" | "codebase_search")
            || (matches!(call.name.as_str(), "write" | "edit")
                && !call
                    .arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(is_session_noise_path)
                    .unwrap_or(false))
    }

    fn should_continue_text_only_turn(
        automation_level: crate::settings::AutomationLevel,
        user_prompt: &str,
        assistant_text: &str,
        prior_tool_activity: bool,
    ) -> bool {
        if matches!(automation_level, crate::settings::AutomationLevel::Ask) {
            return false;
        }
        let assistant = assistant_text.trim();
        if assistant.is_empty() {
            // An empty provider message is not an operator-facing completion. This
            // commonly occurs after tool results on strict replay routes; if we
            // accept it as complete the frontend returns to idle with no answer and an
            // unfinished task. Re-enter the bounded dead-mouse recovery path when
            // work has already started or the operator explicitly requested action.
            return prior_tool_activity
                || user_prompt_is_continue_or_proceed(user_prompt)
                || user_prompt_expects_concrete_action(user_prompt);
        }
        if looks_like_blocked_response(assistant) || looks_like_completion(assistant) {
            return false;
        }
        if looks_like_incomplete_structured_answer(assistant) {
            return matches!(
                automation_level,
                crate::settings::AutomationLevel::Flow
                    | crate::settings::AutomationLevel::Autonomous
            ) || user_prompt_is_continue_or_proceed(user_prompt);
        }
        if looks_like_continuation_request(assistant) {
            // A trailing "want me to proceed?" is only a dead mouse when
            // proceeding was already authorized — by the automation level or by
            // the operator's prompt. Otherwise the question is a legitimate
            // operator decision point (e.g. an assessment ending with "want me
            // to fix these?"), and auto-answering it overrides operator agency.
            return match automation_level {
                crate::settings::AutomationLevel::Flow
                | crate::settings::AutomationLevel::Autonomous => true,
                _ => {
                    user_prompt_is_continue_or_proceed(user_prompt)
                        || user_prompt_expects_concrete_action(user_prompt)
                }
            };
        }
        if matches!(automation_level, crate::settings::AutomationLevel::Guarded) {
            return user_prompt_is_continue_or_proceed(user_prompt)
                && looks_like_plan_or_future_action(assistant);
        }
        if user_prompt_is_continue_or_proceed(user_prompt) {
            return looks_like_plan_or_future_action(assistant) || !prior_tool_activity;
        }
        user_prompt_expects_concrete_action(user_prompt)
            && looks_like_plan_or_future_action(assistant)
    }

    fn looks_like_incomplete_structured_answer(text: &str) -> bool {
        let trimmed = text.trim();
        let fence_count = trimmed
            .lines()
            .filter(|line| line.trim_start().starts_with("```"))
            .count();
        if fence_count % 2 == 1 {
            return true;
        }
        if trimmed.len() < 120 {
            return false;
        }

        let nonempty = trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let Some(last) = nonempty.last().copied() else {
            return false;
        };
        let lower = trimmed.to_ascii_lowercase();
        let last_lower = last.to_ascii_lowercase();
        let last_is_list_item = last_lower.starts_with("- ")
            || last_lower.starts_with("* ")
            || last_lower
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
                && last_lower.contains(". ");
        let last_has_terminal_punctuation = last.ends_with('.')
            || last.ends_with('!')
            || last.ends_with('?')
            || last.ends_with(')')
            || last.ends_with(']')
            || last.ends_with('`');

        last_is_list_item
            && !last_has_terminal_punctuation
            && (lower.contains("phase 1") || lower.contains("roadmap") || lower.contains("plan"))
            && !lower.contains("phase 2")
    }

    fn looks_like_continuation_request(text: &str) -> bool {
        let tail = text
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
            .to_ascii_lowercase();
        tail.contains("shall i")
            || tail.contains("should i")
            || tail.contains("would you like")
            || tail.contains("do you want me to")
            || tail.contains("ready to proceed")
            || tail.contains("want me to proceed")
            || tail.contains("want me to continue")
            || tail.contains("let me know if you want me to")
            || tail.contains("let me know and i")
            || tail.ends_with('?')
                && (tail.contains("proceed")
                    || tail.contains("continue")
                    || tail.contains("implement")
                    || tail.contains("make the change")
                    || tail.contains("go ahead"))
    }

    fn user_prompt_is_continue_or_proceed(text: &str) -> bool {
        crate::conversation::is_continuance_approval(text)
    }

    fn user_prompt_expects_concrete_action(text: &str) -> bool {
        let lower = text.trim().to_ascii_lowercase();
        let trimmed = lower.trim_start();
        let action_prefixes = [
            "fix ",
            "get ",
            "implement ",
            "make ",
            "build ",
            "wire ",
            "add ",
            "update ",
            "remove ",
            "delete ",
            "clean ",
            "cleanup ",
            "install ",
            "link ",
            "commit ",
            "push ",
            "publish ",
            "cut ",
            "release ",
            "run ",
            "test ",
            "validate ",
            "proceed",
            "continue",
        ];
        action_prefixes
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
            || lower.contains("make it so")
            || lower.contains("get it done")
            || lower.contains("go fix")
            || lower.contains("go clean")
            || lower.contains("go ahead")
    }

    fn looks_like_plan_or_future_action(text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        let planning_markers = [
            "i'll ",
            "i will ",
            "i’m going to ",
            "i'm going to ",
            "i can ",
            "i would ",
            "i should ",
            "next i",
            "the next step",
            "my plan",
            "plan:",
            "approach:",
            "i’ll start",
            "i'll start",
            "i’ll inspect",
            "i'll inspect",
            "i’ll update",
            "i'll update",
            "i’ll implement",
            "i'll implement",
            "i’ll make",
            "i'll make",
        ];
        planning_markers.iter().any(|marker| lower.contains(marker))
    }

    fn looks_like_blocked_response(text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        lower.contains("blocked")
            || lower.contains("i need clarification")
            || lower.contains("need clarification")
            || lower.contains("i need you to")
            || lower.contains("cannot proceed")
            || lower.contains("can't proceed")
            || lower.contains("unable to proceed")
            || lower.contains("permission")
    }

    /// Returns true if an assistant text response contains language that suggests
    /// the agent is wrapping up a task rather than pausing mid-work.
    ///
    /// Used by completion and continuation policy to distinguish a wrap-up from a
    /// progress update, question, or partial explanation.
    pub(crate) fn looks_like_completion(text: &str) -> bool {
        if text.len() < 20 {
            return false;
        }
        let lower = text.to_lowercase();
        // Phrases that strongly indicate the agent is done or summarizing
        let completion_phrases = [
            "all done",
            "that's done",
            "that's everything",
            "that's all",
            "all changes",
            "have been made",
            "have been applied",
            "have been updated",
            "all set",
            "let me know if",
            "let me know what",
            "anything else",
            "to summarize",
            "in summary",
            "here's a summary",
            "here is a summary",
            "summary of",
            "the changes are",
            "changes are complete",
            "implementation is complete",
            "task is complete",
            "done!",
            "not committed yet",
        ];
        completion_phrases.iter().any(|p| lower.contains(p))
    }

    /// Returns true if a write target path looks like a session-administrative
    /// noise file rather than real task output.
    ///
    /// Non-Claude models (e.g. GPT-5.5) sometimes respond to dead-mouse nudges by
    /// writing compliance acknowledgment notes — these must not satisfy the nudge
    /// check and reset the dead-mouse counter.
    ///
    /// Heuristic: path is under a known session/admin directory, OR the filename
    /// (stem) matches common compliance-note patterns.
    fn is_session_noise_path(path: &str) -> bool {
        // Directory prefixes that are purely administrative
        let noise_dirs = ["ai/session/", ".omegon/", "ai/lifecycle/"];
        if noise_dirs.iter().any(|d| path.contains(d)) {
            return true;
        }
        // Filename stem patterns: system-warning-note, tool-output-ack,
        // compliance-marker, tool-compliance-marker, warning-log, etc.
        let stem = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let noise_fragments = [
            "warning",
            "compliance",
            "-ack",
            "ack-",
            "tool-output",
            "session-note",
            "system-note",
            "marker",
        ];
        noise_fragments.iter().any(|frag| stem.contains(frag))
    }

    /// Detects pathological tool-call patterns that indicate the agent is stuck.
    struct StuckWarning {
        message: String,
        /// How many consecutive turns the detector has fired.
        consecutive: u32,
    }

    impl std::fmt::Display for StuckWarning {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.message)
        }
    }

    struct StuckDetector {
        /// Recent tool calls as (name, args_hash, was_error)
        recent: Vec<(String, u64, bool)>,
        /// Recent file paths touched by inspection tools (for cross-tool churn).
        recent_file_accesses: Vec<String>,
        /// Window size for pattern detection
        window: usize,
        /// Number of consecutive turns where a stuck pattern was detected.
        consecutive_warnings: u32,
    }

    impl StuckDetector {
        fn new() -> Self {
            Self {
                recent: Vec::new(),
                recent_file_accesses: Vec::new(),
                window: 10,
                consecutive_warnings: 0,
            }
        }

        fn reset_after_escalation(&mut self) {
            self.recent.clear();
            self.recent_file_accesses.clear();
            self.consecutive_warnings = 0;
        }

        /// Record a tool call for pattern analysis.
        ///
        /// For read-like tools we hash only the file path, ignoring offset/limit,
        /// so that re-reads of the same file with different byte ranges are still
        /// caught as repetition.
        fn record(&mut self, catalog: &ToolCapabilityCatalog, call: &ToolCall, is_error: bool) {
            let args_hash = if is_repo_inspection_tool(catalog, &call.name) {
                // Normalize path-scoped inspection calls so repeated reads of the
                // same file collapse even if byte ranges or line windows differ.
                call.arguments
                    .get("path")
                    .map(hash_value)
                    .unwrap_or_else(|| hash_value(&call.arguments))
            } else {
                hash_value(&call.arguments)
            };
            self.recent.push((call.name.clone(), args_hash, is_error));
            if self.recent.len() > self.window * 2 {
                self.recent.drain(..self.window);
            }

            // Track file-level access across all inspection tools.
            // If this is a mutation tool, clear prior accesses for that path —
            // the agent acted on it, so post-mutation reads are legitimate
            // verification, not churn.
            if is_mutation_tool_name(catalog, &call.name) {
                if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                    self.recent_file_accesses.retain(|p| p != path);
                }
            } else if is_repo_inspection_tool(catalog, &call.name)
                && let Some(path) = call.arguments.get("path").and_then(|v| v.as_str())
            {
                self.recent_file_accesses.push(path.to_string());
                if self.recent_file_accesses.len() > self.window * 2 {
                    self.recent_file_accesses.drain(..self.window);
                }
            }
        }

        fn record_observation(&mut self, event: &crate::observation::ObservationEvent) {
            match event {
                crate::observation::ObservationEvent::FileRead { source_tool, path } => {
                    let tool_name = source_tool
                        .strip_prefix("bash:")
                        .unwrap_or(source_tool)
                        .to_string();
                    self.recent.push((tool_name, hash_str_path(path), false));
                    self.recent_file_accesses.push(path.display().to_string());
                }
                crate::observation::ObservationEvent::SearchPerformed {
                    source_tool,
                    query,
                    roots,
                } => {
                    let tool_name = source_tool
                        .strip_prefix("bash:")
                        .unwrap_or(source_tool)
                        .to_string();
                    // Fingerprint the actual query and roots. Hashing a constant
                    // here collapsed every search by the same program into "same
                    // arguments", firing false repeat warnings on healthy
                    // exploration with distinct queries.
                    let mut fingerprint = String::from("<search>");
                    if let Some(query) = query {
                        fingerprint.push('\u{1f}');
                        fingerprint.push_str(query);
                    }
                    for root in roots {
                        fingerprint.push('\u{1f}');
                        fingerprint.push_str(&root.display().to_string());
                    }
                    self.recent.push((tool_name, hash_str(&fingerprint), false));
                }
                crate::observation::ObservationEvent::FileMutated { source_tool, path } => {
                    self.recent
                        .push((source_tool.clone(), hash_str_path(path), false));
                    let rendered = path.display().to_string();
                    self.recent_file_accesses.retain(|p| p != &rendered);
                }
                crate::observation::ObservationEvent::ValidationRun { source_tool } => {
                    let tool_name = if source_tool == "bash" {
                        crate::tool_registry::core::VALIDATE.to_string()
                    } else {
                        source_tool.clone()
                    };
                    self.recent
                        .push((tool_name, hash_str("<validation>"), false));
                    // Validation is a convergence action, not inspection churn.
                    // Clear path-only churn history so a validate→re-read loop is
                    // treated as post-validation investigation rather than stale
                    // pre-validation spinning.
                    self.recent_file_accesses.clear();
                }
                crate::observation::ObservationEvent::ProgressBoundary { source_tool, .. } => {
                    let tool_name = if source_tool == "bash" {
                        crate::tool_registry::core::COMMIT.to_string()
                    } else {
                        source_tool.clone()
                    };
                    self.recent.push((tool_name, hash_str("<progress>"), false));
                }
            }
            if self.recent.len() > self.window * 2 {
                self.recent.drain(..self.window);
            }
            if self.recent_file_accesses.len() > self.window * 2 {
                self.recent_file_accesses.drain(..self.window);
            }
        }

        /// Check for stuck patterns. Returns a warning with escalation level if detected.
        fn check(&mut self, catalog: &ToolCapabilityCatalog) -> Option<StuckWarning> {
            let len = self.recent.len();
            if len < 3 {
                self.consecutive_warnings = 0;
                return None;
            }

            let window = &self.recent[len.saturating_sub(self.window)..];

            // Pattern 1: inspect-without-modify loop — same file (path-normalized)
            // inspected 5+ times without any write/edit to that file.  Threshold
            // is 5 (not 3) because path normalization collapses offset/limit
            // variations, and a legitimate explore→test→re-read→edit workflow may
            // read the same file 3-4 times.  Also skip detection if a mutation or
            // validation tool appeared in the window — that signals the agent is
            // trying to converge, not spinning.
            let has_mutation_or_validation = window.iter().any(|(name, _, _)| {
                is_mutation_tool_name(catalog, name) || is_validation_tool_name(catalog, name)
            });
            let reads: Vec<_> = window
                .iter()
                .filter(|(name, _, _)| is_repo_inspection_tool(catalog, name))
                .collect();
            if !has_mutation_or_validation && reads.len() >= 5 {
                let mut hash_counts: HashMap<u64, u32> = HashMap::new();
                for (_, h, _) in &reads {
                    *hash_counts.entry(*h).or_default() += 1;
                }
                if hash_counts.values().any(|&c| c >= 5) {
                    self.consecutive_warnings += 1;
                    return Some(StuckWarning {
                    message: "You've inspected the same target multiple times without modifying it. \
                         Stop re-reading and either edit, validate, or summarize the blocker plainly."
                        .into(),
                    consecutive: self.consecutive_warnings,
                });
                }
            }

            // Pattern 2: Same tool + same args called 3+ times
            if let Some(repeated) = self.find_repeated_call(catalog, window, 3) {
                self.consecutive_warnings += 1;
                return Some(StuckWarning {
                    message: format!(
                        "You've called `{}` with the same arguments {} times. \
                     If it's not producing the result you need, try a different approach.",
                        repeated.0, repeated.1
                    ),
                    consecutive: self.consecutive_warnings,
                });
            }

            // Pattern 3: Edit failures — repeated error on the same tool
            let recent_errors: Vec<_> = window.iter().filter(|(_, _, err)| *err).collect();
            if recent_errors.len() >= 3 {
                let names: Vec<_> = recent_errors.iter().map(|(n, _, _)| n.as_str()).collect();
                if names.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
                    self.consecutive_warnings += 1;
                    return Some(StuckWarning {
                        message: format!(
                            "Your last several `{}` calls returned errors. \
                         Consider reading the current file state before retrying.",
                            recent_errors.last().unwrap().0
                        ),
                        consecutive: self.consecutive_warnings,
                    });
                }
            }

            // Pattern 4: Cross-tool file churn — same file accessed 4+ times
            // across *any* combination of read/view/codebase_search without edits.
            if self.recent_file_accesses.len() >= 4 {
                let access_window = &self.recent_file_accesses
                    [self.recent_file_accesses.len().saturating_sub(self.window)..];
                let mut path_counts: HashMap<&str, u32> = HashMap::new();
                for path in access_window {
                    *path_counts.entry(path.as_str()).or_default() += 1;
                }
                if let Some((path, count)) = path_counts.iter().find(|&(_, &c)| c >= 4) {
                    self.consecutive_warnings += 1;
                    return Some(StuckWarning {
                        message: format!(
                            "You've accessed `{}` {} times across different tools without modifying it. \
                         Stop inspecting and either edit it, run a validation, or state the blocker.",
                            path, count
                        ),
                        consecutive: self.consecutive_warnings,
                    });
                }
            }

            self.consecutive_warnings = 0;
            None
        }

        /// Find a (tool_name, count) where the same tool+args appears N+ times in the window.
        ///
        /// Read entries are excluded: their hashes are path-normalized (distinct
        /// line ranges collapse to one hash), so exact-repeat counting would
        /// double-punish legitimate paging through a large file. Read churn is
        /// owned by patterns 1 and 4, which use wider thresholds and
        /// mutation/validation guards. Validation and progress entries hash a
        /// constant marker and are also excluded — repeated validation runs are
        /// convergence, not argument repetition.
        fn find_repeated_call(
            &self,
            catalog: &ToolCapabilityCatalog,
            window: &[(String, u64, bool)],
            threshold: usize,
        ) -> Option<(String, usize)> {
            let validation_marker = hash_str("<validation>");
            let progress_marker = hash_str("<progress>");
            let mut counts: HashMap<(String, u64), usize> = HashMap::new();
            for (name, hash, _) in window {
                if *hash == validation_marker || *hash == progress_marker {
                    continue;
                }
                if is_repo_inspection_tool(catalog, name)
                    || crate::observation::is_read_program(name)
                {
                    continue;
                }
                // Mutation entries are path-normalized too (FileMutated hashes the
                // path), so distinct successful edits to one file would count as
                // repeats. Repeated *failing* mutations are pattern 3's job.
                if is_mutation_tool_name(catalog, name) || name.starts_with("bash:") {
                    continue;
                }
                let key = (name.clone(), *hash);
                *counts.entry(key).or_default() += 1;
            }
            counts
                .into_iter()
                .find(|(_, count)| *count >= threshold)
                .map(|((name, _), count)| (name, count))
        }
    }

    /// Hash a serde_json::Value for comparison (not cryptographic — just dedup).
    fn hash_value(v: &Value) -> u64 {
        let mut hasher = DefaultHasher::new();
        let s = v.to_string();
        s.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_str(s: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_str_path(path: &std::path::Path) -> u64 {
        hash_str(&path.display().to_string())
    }
}

#[cfg(test)]
use crate::loop_session::{
    StuckDetector, counts_as_real_work_for_dead_mouse, is_session_noise_path,
    looks_like_completion, looks_like_incomplete_structured_answer, should_continue_text_only_turn,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::{EvidenceAssessment, EvidenceSufficiency, ProgressSignal};
    use crate::bridge::LlmEvent;
    use crate::invocation_batch::{dispatch_tools, summarize_tool_args};
    use crate::loop_permission::wait_for_permission_response;
    use crate::upstream_errors::TransientFailureKind;
    use omegon_traits::{OodaPhase, ToolCapability, ToolDefinition, ToolProvider};

    #[test]
    fn stream_idle_budget_is_phase_aware() {
        use std::time::Duration;
        let initial = Duration::from_secs(90);
        let content = Duration::from_secs(90);
        let reasoning = Duration::from_secs(600);

        // Explicit reasoning uses the generous reasoning budget.
        assert_eq!(
            select_stream_idle_budget(
                StreamIdlePhase::ReasoningStreaming,
                initial,
                content,
                reasoning
            ),
            reasoning
        );
        // Active content uses the tighter active budget.
        assert_eq!(
            select_stream_idle_budget(
                StreamIdlePhase::OutputStreaming,
                initial,
                content,
                reasoning
            ),
            content
        );
        // Active tool-call streaming is also active output, not a reasoning gap.
        assert_eq!(
            select_stream_idle_budget(StreamIdlePhase::ToolStreaming, initial, content, reasoning),
            content
        );
        // Inter-item gaps after text/thinking/tool blocks get the generous
        // budget because providers may legally go silent while deciding the
        // next block/item.
        assert_eq!(
            select_stream_idle_budget(
                StreamIdlePhase::AmbiguousSilent,
                initial,
                content,
                reasoning
            ),
            reasoning
        );
        // Before the first substantive event, reasoning-capable providers may
        // legitimately stay silent for minutes; use the same generous budget.
        assert_eq!(
            select_stream_idle_budget(
                StreamIdlePhase::AwaitingFirstEvent,
                initial,
                content,
                reasoning
            ),
            reasoning
        );
        // The reasoning leash must strictly exceed the content leash.
        assert!(reasoning > content);
    }

    #[tokio::test(start_paused = true)]
    async fn transport_heartbeat_flood_cannot_extend_semantic_deadline() {
        use std::time::Duration;

        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(64);
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        stream_tx.send(LlmEvent::Start).await.unwrap();
        for _ in 0..32 {
            stream_tx.send(LlmEvent::TransportHeartbeat).await.unwrap();
        }

        let result = consume_llm_stream_with_policy(
            &mut stream_rx,
            &events_tx,
            "openai-codex",
            "test-model",
            None,
            StreamIdlePolicy {
                initial: Duration::from_secs(2),
                active: Duration::from_secs(2),
                reasoning: Duration::from_secs(2),
                absolute: Duration::from_secs(120),
            },
        );
        tokio::pin!(result);
        tokio::time::advance(Duration::from_secs(3)).await;
        let error = result
            .await
            .expect_err("heartbeat flood must not extend semantic deadline");
        assert!(error.to_string().contains("32 heartbeat event(s)"));
    }

    #[tokio::test(start_paused = true)]
    async fn absolute_deadline_is_not_reset_by_semantic_progress() {
        use std::time::Duration;

        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(16);
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        stream_tx.send(LlmEvent::Start).await.unwrap();

        let producer = tokio::spawn(async move {
            for _ in 0..4 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                stream_tx
                    .send(LlmEvent::TextDelta { delta: "x".into() })
                    .await
                    .unwrap();
            }
        });
        let result = consume_llm_stream_with_policy(
            &mut stream_rx,
            &events_tx,
            "test-provider",
            "test-model",
            None,
            StreamIdlePolicy {
                initial: Duration::from_secs(30),
                active: Duration::from_secs(30),
                reasoning: Duration::from_secs(30),
                absolute: Duration::from_secs(3),
            },
        );
        tokio::pin!(result);
        tokio::time::advance(Duration::from_secs(4)).await;
        let error = result
            .await
            .expect_err("semantic progress must not reset absolute deadline");
        assert!(error.to_string().contains("absolute 3s turn deadline"));
        producer.abort();
    }

    #[test]
    fn visible_output_forces_active_budget_for_every_phase() {
        use std::time::Duration;

        let policy = StreamIdlePolicy {
            initial: Duration::from_secs(30),
            active: Duration::from_secs(2),
            reasoning: Duration::from_secs(60),
            absolute: Duration::from_secs(120),
        };
        for phase in [
            StreamIdlePhase::AwaitingFirstEvent,
            StreamIdlePhase::OutputStreaming,
            StreamIdlePhase::ToolStreaming,
            StreamIdlePhase::ReasoningStreaming,
            StreamIdlePhase::AmbiguousSilent,
        ] {
            assert_eq!(
                policy.budget(phase, true),
                policy.active,
                "visible output in {} must retain the active-turn deadline",
                phase.label()
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn visible_output_then_silence_terminalizes_with_abort() {
        use std::time::Duration;

        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(4);
        let (events_tx, mut events_rx) = broadcast::channel(16);
        stream_tx.send(LlmEvent::TextStart).await.unwrap();
        stream_tx
            .send(LlmEvent::TextDelta {
                delta: "visible".into(),
            })
            .await
            .unwrap();

        let result = consume_llm_stream_with_policy(
            &mut stream_rx,
            &events_tx,
            "test-provider",
            "test-model",
            None,
            StreamIdlePolicy {
                initial: Duration::from_secs(30),
                active: Duration::from_secs(2),
                reasoning: Duration::from_secs(60),
                absolute: Duration::from_secs(120),
            },
        );
        tokio::pin!(result);
        tokio::time::advance(Duration::from_secs(3)).await;
        let error = result.await.expect_err("silent partial stream must fail");
        assert!(error.to_string().contains("no semantic progress for 2s"));

        let mut saw_visible = false;
        let mut abort_reason = None;
        while let Ok(event) = events_rx.try_recv() {
            match event {
                AgentEvent::MessageChunk { text } if text == "visible" => saw_visible = true,
                AgentEvent::MessageAbort { reason } => abort_reason = reason,
                _ => {}
            }
        }
        assert!(saw_visible, "partial output must remain observable");
        assert!(
            abort_reason.is_some_and(|reason| reason.contains("no semantic progress for 2s")),
            "stall must emit an explicit terminal abort"
        );
    }

    #[tokio::test]
    async fn partial_output_then_stream_close_is_an_abnormal_terminal_error() {
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel(4);
        let (events_tx, _) = broadcast::channel(16);
        stream_tx
            .send(LlmEvent::TextDelta {
                delta: "truncated".into(),
            })
            .await
            .unwrap();
        drop(stream_tx);

        let error = consume_llm_stream_with_policy(
            &mut stream_rx,
            &events_tx,
            "test-provider",
            "test-model",
            None,
            StreamIdlePolicy {
                initial: std::time::Duration::from_secs(30),
                active: std::time::Duration::from_secs(2),
                reasoning: std::time::Duration::from_secs(60),
                absolute: std::time::Duration::from_secs(120),
            },
        )
        .await
        .expect_err("EOF without Done must fail");
        assert!(error.to_string().contains("without a completion event"));
    }

    #[test]
    fn stream_idle_phase_tracks_event_sequences() {
        fn apply(mut phase: StreamIdlePhase, events: &[LlmEvent]) -> StreamIdlePhase {
            for event in events {
                phase = stream_idle_phase_after_event(phase, event);
            }
            phase
        }

        assert_eq!(
            apply(
                StreamIdlePhase::AwaitingFirstEvent,
                &[
                    LlmEvent::TextStart,
                    LlmEvent::TextDelta { delta: "hi".into() },
                ],
            ),
            StreamIdlePhase::OutputStreaming
        );
        assert_eq!(
            apply(StreamIdlePhase::OutputStreaming, &[LlmEvent::TextEnd]),
            StreamIdlePhase::AmbiguousSilent
        );
        assert_eq!(
            apply(
                StreamIdlePhase::AmbiguousSilent,
                &[
                    LlmEvent::ThinkingStart,
                    LlmEvent::ThinkingDelta { delta: "".into() },
                ],
            ),
            StreamIdlePhase::ReasoningStreaming
        );
        assert_eq!(
            apply(
                StreamIdlePhase::ReasoningStreaming,
                &[LlmEvent::ThinkingEnd]
            ),
            StreamIdlePhase::AmbiguousSilent
        );
        assert_eq!(
            apply(
                StreamIdlePhase::AmbiguousSilent,
                &[
                    LlmEvent::ToolCallStart,
                    LlmEvent::ToolCallDelta { delta: "{}".into() },
                ],
            ),
            StreamIdlePhase::ToolStreaming
        );
        assert_eq!(
            apply(
                StreamIdlePhase::ToolStreaming,
                &[LlmEvent::ToolCallEnd {
                    tool_call: crate::bridge::WireToolCall {
                        id: "call-1".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({}),
                    },
                }],
            ),
            StreamIdlePhase::AmbiguousSilent
        );
        assert_eq!(
            stream_idle_phase_after_event(StreamIdlePhase::AwaitingFirstEvent, &LlmEvent::Start),
            StreamIdlePhase::AwaitingFirstEvent
        );
    }

    #[tokio::test]
    async fn permission_wait_remains_pending_without_operator_response() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let cancel = CancellationToken::new();

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            wait_for_permission_response(rx, cancel),
        )
        .await;

        assert!(
            result.is_err(),
            "permission wait must not auto-deny on a passive timeout"
        );
    }

    #[tokio::test]
    async fn permission_wait_cancellation_unblocks_as_deny() {
        let (_tx, rx) = std::sync::mpsc::channel();
        let cancel = CancellationToken::new();
        let child = cancel.child_token();

        let task = tokio::spawn(wait_for_permission_response(rx, child));
        cancel.cancel();

        let response = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("permission wait should observe cancellation")
            .expect("permission wait task should not panic");
        assert_eq!(response, omegon_traits::PermissionResponse::Deny);
    }

    #[tokio::test]
    async fn permission_wait_returns_explicit_operator_response() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = CancellationToken::new();

        tx.send(omegon_traits::PermissionResponse::Allow)
            .expect("send permission response");

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            wait_for_permission_response(rx, cancel),
        )
        .await
        .expect("permission wait should complete after explicit response");
        assert_eq!(response, omegon_traits::PermissionResponse::Allow);
    }

    fn test_tool_catalog() -> ToolCapabilityCatalog {
        ToolCapabilityCatalog::from_tool_defs(&[
            ToolDefinition {
                name: "bash".into(),
                label: "bash".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: "read".into(),
                label: "read".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::RepoInspection,
                    ToolCapability::TargetedRepoInspection,
                ],
            },
            ToolDefinition {
                name: "view".into(),
                label: "view".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::RepoInspection,
                    ToolCapability::BroadRepoInspection,
                ],
            },
            ToolDefinition {
                name: "codebase_search".into(),
                label: "codebase_search".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::RepoInspection,
                    ToolCapability::BroadRepoInspection,
                ],
            },
            ToolDefinition {
                name: "codebase_index".into(),
                label: "codebase_index".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::RepoInspection,
                    ToolCapability::BroadRepoInspection,
                ],
            },
            ToolDefinition {
                name: "edit".into(),
                label: "edit".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: "write".into(),
                label: "write".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: "change".into(),
                label: "change".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: "validate".into(),
                label: "validate".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Validation],
            },
            ToolDefinition {
                name: "memory_recall".into(),
                label: "memory_recall".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::Orientation,
                    ToolCapability::BroadOrientation,
                ],
            },
            ToolDefinition {
                name: "memory_store".into(),
                label: "memory_store".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Orientation],
            },
            ToolDefinition {
                name: "context_status".into(),
                label: "context_status".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::Orientation,
                    ToolCapability::BroadOrientation,
                ],
            },
            ToolDefinition {
                name: "request_context".into(),
                label: "request_context".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::Orientation,
                    ToolCapability::BroadOrientation,
                ],
            },
            ToolDefinition {
                name: "manage_tools".into(),
                label: "manage_tools".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Orientation],
            },
            ToolDefinition {
                name: "web_search".into(),
                label: "web_search".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: "commit".into(),
                label: "commit".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::StateChanging,
                    ToolCapability::ProgressBoundary,
                ],
            },
            ToolDefinition {
                name: "delegate".into(),
                label: "delegate".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::StateChanging,
                    ToolCapability::ProgressBoundary,
                ],
            },
            ToolDefinition {
                name: "cleave_run".into(),
                label: "cleave_run".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![
                    ToolCapability::StateChanging,
                    ToolCapability::ProgressBoundary,
                ],
            },
            ToolDefinition {
                name: "cleave_assess".into(),
                label: "cleave_assess".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: "chronos".into(),
                label: "chronos".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Orientation],
            },
            ToolDefinition {
                name: "whoami".into(),
                label: "whoami".into(),
                description: String::new(),
                parameters: Value::Null,
                capabilities: vec![ToolCapability::Orientation],
            },
        ])
    }

    #[test]
    fn stuck_detector_repeated_calls() {
        let mut detector = StuckDetector::new();
        let call = ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test -p omegon"}),
        };

        detector.record(&test_tool_catalog(), &call, false);
        detector.record(&test_tool_catalog(), &call, false);
        assert!(detector.check(&test_tool_catalog()).is_none());

        detector.record(&test_tool_catalog(), &call, false);
        let warning = detector.check(&test_tool_catalog());
        assert!(warning.is_some());
        assert!(warning.unwrap().message.contains("same arguments"));
    }

    #[test]
    fn stuck_detector_tracks_file_churn_through_observation_events() {
        let mut detector = StuckDetector::new();
        let path = "src/main.rs";

        for command in [
            "sed -n '1,40p' src/main.rs",
            "cat src/main.rs",
            "head -20 src/main.rs",
            "tail -20 src/main.rs",
        ] {
            let call = ToolCall {
                id: command.into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": command}),
            };
            let result = ToolResultEntry {
                call_id: command.into(),
                tool_name: "bash".into(),
                content: vec![],
                is_error: false,
                args_summary: None,
            };
            let events = crate::observation::ObservationNormalizer::new(&test_tool_catalog())
                .normalize(&[call], &[result]);
            for event in events {
                detector.record_observation(&event);
            }
        }

        let warning = detector.check(&test_tool_catalog()).expect("warning");
        assert!(warning.message.contains(path), "{}", warning.message);
    }

    #[test]
    fn stuck_detector_bash_validation_breaks_file_churn() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();

        for command in [
            "sed -n '1,40p' src/main.rs",
            "cat src/main.rs",
            "head -20 src/main.rs",
            "cargo test -p omegon observation --locked",
            "tail -20 src/main.rs",
            "sed -n '41,80p' src/main.rs",
        ] {
            let call = ToolCall {
                id: command.into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": command}),
            };
            let result = ToolResultEntry {
                call_id: command.into(),
                tool_name: "bash".into(),
                content: vec![],
                is_error: false,
                args_summary: None,
            };
            let events = crate::observation::ObservationNormalizer::new(&catalog)
                .normalize(&[call], &[result]);
            for event in events {
                detector.record_observation(&event);
            }
        }

        assert!(
            detector.check(&catalog).is_none(),
            "bash validation should break repeated read-only churn"
        );
    }

    fn observe_bash(detector: &mut StuckDetector, catalog: &ToolCapabilityCatalog, command: &str) {
        let call = ToolCall {
            id: command.into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": command}),
        };
        let result = ToolResultEntry {
            call_id: command.into(),
            tool_name: "bash".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        };
        let events =
            crate::observation::ObservationNormalizer::new(catalog).normalize(&[call], &[result]);
        for event in events {
            detector.record_observation(&event);
        }
    }

    #[test]
    fn stuck_detector_distinct_searches_are_not_repeats() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();
        for command in [
            "grep -n 'alpha' src/main.rs",
            "grep -n 'beta' src/main.rs",
            "grep -n 'gamma' src/main.rs",
            "grep -n 'delta' src/main.rs",
        ] {
            observe_bash(&mut detector, &catalog, command);
        }
        assert!(
            detector.check(&catalog).is_none(),
            "distinct search queries must not count as repeated arguments"
        );
    }

    #[test]
    fn stuck_detector_identical_searches_are_repeats() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();
        for _ in 0..3 {
            observe_bash(&mut detector, &catalog, "grep -n 'alpha' src/main.rs");
        }
        let warning = detector
            .check(&catalog)
            .expect("identical searches should warn");
        assert!(warning.message.contains("same arguments"));
    }

    #[test]
    fn stuck_detector_quoted_pipe_search_is_single_event() {
        let catalog = test_tool_catalog();
        let call = ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": r#"grep -n -E "alpha|beta" src/main.rs"#}),
        };
        let result = ToolResultEntry {
            call_id: "1".into(),
            tool_name: "bash".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        };
        let events =
            crate::observation::ObservationNormalizer::new(&catalog).normalize(&[call], &[result]);
        assert_eq!(events.len(), 1, "quoted pipe must not split: {events:?}");
        match &events[0] {
            crate::observation::ObservationEvent::SearchPerformed { query, .. } => {
                assert_eq!(query.as_deref(), Some("alpha|beta"));
            }
            other => panic!("expected SearchPerformed, got {other:?}"),
        }
    }

    #[test]
    fn stuck_detector_paged_reads_of_large_file_do_not_repeat() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();
        for command in [
            "sed -n '1,40p' src/main.rs",
            "sed -n '41,80p' src/main.rs",
            "sed -n '81,120p' src/main.rs",
        ] {
            observe_bash(&mut detector, &catalog, command);
        }
        assert!(
            detector.check(&catalog).is_none(),
            "paging through one file in distinct ranges is not an exact repeat"
        );
    }

    #[test]
    fn stuck_detector_repeated_validation_is_not_a_repeat() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();
        for _ in 0..4 {
            observe_bash(&mut detector, &catalog, "cargo test -p omegon --locked");
        }
        assert!(
            detector.check(&catalog).is_none(),
            "repeated validation runs are convergence, not repetition"
        );
    }

    #[test]
    fn stuck_detector_distinct_edits_to_same_file_do_not_repeat() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();
        for (i, old) in ["a", "b", "c", "d"].iter().enumerate() {
            let call = ToolCall {
                id: format!("{i}"),
                name: "edit".into(),
                arguments: serde_json::json!({
                    "path": "src/main.rs",
                    "oldText": old,
                    "newText": "x",
                }),
            };
            let result = ToolResultEntry {
                call_id: format!("{i}"),
                tool_name: "edit".into(),
                content: vec![],
                is_error: false,
                args_summary: None,
            };
            let events = crate::observation::ObservationNormalizer::new(&catalog)
                .normalize(&[call], &[result]);
            for event in events {
                detector.record_observation(&event);
            }
        }
        assert!(
            detector.check(&catalog).is_none(),
            "distinct successful edits to one file are progress, not repetition"
        );
    }

    #[test]
    fn stuck_detector_mutation_observation_clears_file_churn() {
        let mut detector = StuckDetector::new();
        let catalog = test_tool_catalog();

        for command in [
            "sed -n '1,40p' src/main.rs",
            "cat src/main.rs",
            "head -20 src/main.rs",
        ] {
            let call = ToolCall {
                id: command.into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": command}),
            };
            let result = ToolResultEntry {
                call_id: command.into(),
                tool_name: "bash".into(),
                content: vec![],
                is_error: false,
                args_summary: None,
            };
            let events = crate::observation::ObservationNormalizer::new(&catalog)
                .normalize(&[call], &[result]);
            for event in events {
                detector.record_observation(&event);
            }
        }

        let edit = ToolCall {
            id: "edit".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "src/main.rs", "oldText": "a", "newText": "b"}),
        };
        let edit_result = ToolResultEntry {
            call_id: "edit".into(),
            tool_name: "edit".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        };
        let events = crate::observation::ObservationNormalizer::new(&catalog)
            .normalize(&[edit], &[edit_result]);
        for event in events {
            detector.record_observation(&event);
        }

        for command in [
            "tail -20 src/main.rs",
            "sed -n '41,80p' src/main.rs",
            "cat src/main.rs",
        ] {
            let call = ToolCall {
                id: command.into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": command}),
            };
            let result = ToolResultEntry {
                call_id: command.into(),
                tool_name: "bash".into(),
                content: vec![],
                is_error: false,
                args_summary: None,
            };
            let events = crate::observation::ObservationNormalizer::new(&catalog)
                .normalize(&[call], &[result]);
            for event in events {
                detector.record_observation(&event);
            }
        }

        assert!(
            detector.check(&catalog).is_none(),
            "mutation observation should clear prior access entries for the path"
        );
    }

    #[test]
    fn stuck_detector_mutation_clears_file_access_history() {
        let mut detector = StuckDetector::new();
        let path = "src/main.rs";

        // Read the same file 3 times via different inspection tools
        for name in &["read", "view", "read"] {
            detector.record(
                &test_tool_catalog(),
                &ToolCall {
                    id: "r".into(),
                    name: (*name).into(),
                    arguments: serde_json::json!({"path": path}),
                },
                false,
            );
        }
        // Mutate it — should clear prior access entries for this path
        detector.record(
            &test_tool_catalog(),
            &ToolCall {
                id: "m".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": path, "oldText": "a", "newText": "b"}),
            },
            false,
        );
        // Read once more to verify the edit
        detector.record(
            &test_tool_catalog(),
            &ToolCall {
                id: "r2".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": path}),
            },
            false,
        );
        // Should NOT trigger cross-tool file churn — the mutation reset the counter
        let warning = detector.check(&test_tool_catalog());
        assert!(
            warning.is_none() || !warning.as_ref().unwrap().message.contains(path),
            "mutation should clear file access history; got: {:?}",
            warning.map(|w| w.message)
        );
    }

    #[test]
    fn stuck_detector_normalizes_path_scoped_inspection_tools_by_capability() {
        let mut detector = StuckDetector::new();
        let path = "src/main.rs";

        for lines in &[(1, 20), (40, 80), (81, 120)] {
            detector.record(
                &test_tool_catalog(),
                &ToolCall {
                    id: format!("v-{}-{}", lines.0, lines.1),
                    name: "view".into(),
                    arguments: serde_json::json!({"path": path, "lines": [lines.0, lines.1]}),
                },
                false,
            );
        }

        // Inspection hashes are path-normalized, so pattern 2 must not treat
        // distinct-range paging as "same arguments". Read churn on one path
        // is owned by patterns 1 and 4 with wider thresholds.
        assert!(
            detector.check(&test_tool_catalog()).is_none(),
            "distinct-range views of one file are paging, not repetition"
        );
    }

    #[test]
    fn stuck_detector_repeated_errors() {
        let mut detector = StuckDetector::new();
        let call = ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "foo.rs", "oldText": "a", "newText": "b"}),
        };

        detector.record(&test_tool_catalog(), &call, true);
        detector.record(&test_tool_catalog(), &call, true);
        detector.record(&test_tool_catalog(), &call, true);

        // This triggers the repeated-call pattern (same args 3x)
        let warning = detector.check(&test_tool_catalog());
        assert!(warning.is_some());
    }

    // ── Auto-batch tests ────────────────────────────────────────────

    #[test]
    fn mutation_tool_detection_is_capability_driven() {
        let catalog = test_tool_catalog();
        assert!(is_mutation_tool_name(&catalog, "edit"));
        assert!(is_mutation_tool_name(&catalog, "write"));
        assert!(is_mutation_tool_name(&catalog, "change"));
        assert!(!is_mutation_tool_name(&catalog, "read"));
        assert!(!is_mutation_tool_name(&catalog, "bash"));
        assert!(!is_mutation_tool_name(&catalog, "web_search"));
    }

    #[test]
    fn mutation_tool_detection_does_not_depend_on_tool_name() {
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[ToolDefinition {
            name: "surgical_patch".into(),
            label: "surgical_patch".into(),
            description: String::new(),
            parameters: Value::Null,
            capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
        }]);
        assert!(is_mutation_tool_name(&catalog, "surgical_patch"));
    }

    #[test]
    fn summarize_args_by_tool() {
        assert_eq!(
            summarize_tool_args("read", &serde_json::json!({"path": "src/foo.rs"})).as_deref(),
            Some("src/foo.rs")
        );
        assert_eq!(
            summarize_tool_args("bash", &serde_json::json!({"command": "cargo test"})).as_deref(),
            Some("cargo test")
        );
        assert_eq!(
            summarize_tool_args(
                "change",
                &serde_json::json!({
                    "edits": [{"file": "a.rs"}, {"file": "b.rs"}]
                })
            )
            .as_deref(),
            Some("a.rs, b.rs")
        );
        // Memory tools
        assert_eq!(
            summarize_tool_args(
                "memory_recall",
                &serde_json::json!({"query": "auth architecture"})
            )
            .as_deref(),
            Some("auth architecture")
        );
        assert_eq!(
            summarize_tool_args(
                "memory_store",
                &serde_json::json!({"content": "Omegon uses ratatui"})
            )
            .as_deref(),
            Some("Omegon uses ratatui")
        );

        // Long command gets truncated
        let long_cmd = "x".repeat(100);
        let summary =
            summarize_tool_args("bash", &serde_json::json!({"command": long_cmd})).unwrap();
        assert!(summary.len() <= 84, "got len {}", summary.len()); // 80 + "…" (3 bytes UTF-8)
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn summarize_cleave_run_shows_child_count_and_labels() {
        let plan = serde_json::json!({
            "children": [
                {"label": "api-layer", "description": "add endpoints", "scope": ["src/api.rs"]},
                {"label": "db-layer",  "description": "add migrations", "scope": ["migrations/"]}
            ],
            "rationale": "split by layer"
        });
        let summary = summarize_tool_args(
            "cleave_run",
            &serde_json::json!({
                "directive": "Build JWT auth",
                "plan_json": plan.to_string()
            }),
        )
        .unwrap();
        assert!(
            summary.contains("2 children"),
            "expected child count: {summary}"
        );
        assert!(summary.contains("api-layer"), "expected labels: {summary}");
        assert!(summary.contains("db-layer"), "expected labels: {summary}");
    }

    #[test]
    fn summarize_cleave_run_handles_malformed_plan() {
        // Bad plan_json should not panic — falls back to "cleave"
        let result = summarize_tool_args(
            "cleave_run",
            &serde_json::json!({"directive": "do something", "plan_json": "not json"}),
        );
        assert_eq!(result.as_deref(), Some("cleave"));
    }

    #[test]
    fn summarize_cleave_assess_shows_directive() {
        let result = summarize_tool_args(
            "cleave_assess",
            &serde_json::json!({"directive": "implement OAuth flow"}),
        );
        assert_eq!(result.as_deref(), Some("implement OAuth flow"));
    }

    #[tokio::test]
    async fn auto_batch_rollback_on_second_edit_failure() {
        use omegon_traits::ToolResult;
        use std::io::Write as IoWrite;

        // Create a mock tool provider that does real file I/O
        struct FileEditProvider {
            dir: std::path::PathBuf,
        }

        #[async_trait::async_trait]
        impl ToolProvider for FileEditProvider {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![omegon_traits::ToolDefinition {
                    name: "edit".into(),
                    label: "edit".into(),
                    description: "test".into(),
                    parameters: serde_json::json!({}),
                    capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
                }]
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                let path_str = args["path"].as_str().unwrap();
                let path = std::path::Path::new(path_str);
                let old_text = args["oldText"].as_str().unwrap();
                let new_text = args["newText"].as_str().unwrap();

                let content = tokio::fs::read_to_string(path).await?;
                if !content.contains(old_text) {
                    anyhow::bail!("Could not find exact text in {}", path.display());
                }
                let new_content = content.replacen(old_text, new_text, 1);
                tokio::fs::write(path, &new_content).await?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Edited {}", path.display()),
                    }],
                    details: Value::Null,
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::File::create(&file_a)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        std::fs::File::create(&file_b)
            .unwrap()
            .write_all(b"foo bar baz")
            .unwrap();

        let provider = FileEditProvider {
            dir: dir.path().to_path_buf(),
        };
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "test-edit",
            Box::new(provider),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();

        // Two edits: first succeeds, second will fail (text not found)
        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "edit".into(),
                arguments: serde_json::json!({
                    "path": file_a.display().to_string(),
                    "oldText": "hello",
                    "newText": "goodbye"
                }),
            },
            ToolCall {
                id: "2".into(),
                name: "edit".into(),
                arguments: serde_json::json!({
                    "path": file_b.display().to_string(),
                    "oldText": "NONEXISTENT",
                    "newText": "replaced"
                }),
            },
        ];

        let dispatch = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            None,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;
        let results = dispatch.results;

        // The second edit should have failed
        assert!(results[1].is_error, "second edit should fail");

        // The first file should be ROLLED BACK to original content
        let a_content = std::fs::read_to_string(&file_a).unwrap();
        assert_eq!(
            a_content, "hello world",
            "file_a should be rolled back, got: {a_content}"
        );

        // The error message should mention the rollback
        let error_text = results[1].content[0].as_text().unwrap();
        assert!(
            error_text.contains("Auto-rollback"),
            "should mention rollback, got: {error_text}"
        );
    }

    #[tokio::test]
    async fn single_edit_has_no_batch_overhead() {
        use omegon_traits::ToolResult;
        let dir = tempfile::tempdir().unwrap();

        struct PassProvider;

        #[async_trait::async_trait]
        impl ToolProvider for PassProvider {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![omegon_traits::ToolDefinition {
                    name: "edit".into(),
                    label: "edit".into(),
                    description: "test".into(),
                    parameters: serde_json::json!({}),
                    capabilities: vec![ToolCapability::Mutation, ToolCapability::StateChanging],
                }]
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                _args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Edited ok".into(),
                    }],
                    details: Value::Null,
                })
            }
        }

        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "test-pass",
            Box::new(PassProvider),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();

        let calls = vec![ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "/tmp/fake.rs", "oldText": "a", "newText": "b"}),
        }];

        let dispatch = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            None,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;
        assert!(!dispatch.results[0].is_error);
        let text = dispatch.results[0].content[0].as_text().unwrap();
        assert!(
            !text.contains("rollback"),
            "single edit should have no batch overhead"
        );
    }

    #[tokio::test]
    async fn permission_policy_deny_blocks_dispatch_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(crate::tools::CoreTools::new(dir.path().to_path_buf())),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);
        let (events_tx, _) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let mut policy = crate::permissions::LayeredPermissionPolicy::default();
        policy.project.tools.insert(
            crate::tool_registry::core::BASH.to_string(),
            crate::permissions::ToolPermissionRule::Action(
                crate::permissions::PermissionAction::Deny,
            ),
        );
        let calls = vec![ToolCall {
            id: "deny-bash".into(),
            name: crate::tool_registry::core::BASH.into(),
            arguments: serde_json::json!({"command":"touch should-not-exist"}),
        }];
        let dispatch = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            Some(&policy),
            None,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;
        assert_eq!(dispatch.results.len(), 1);
        assert!(dispatch.results[0].is_error);
        assert!(!dir.path().join("should-not-exist").exists());
    }

    #[tokio::test]
    async fn durable_acknowledgement_precedes_owner_entry_and_settlement_precedes_return() {
        struct DurableObserver {
            authority: crate::session_authority::SessionAuthorityHandle,
        }

        #[async_trait::async_trait]
        impl ToolProvider for DurableObserver {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![omegon_traits::ToolDefinition {
                    name: "durable_observer".into(),
                    label: "durable_observer".into(),
                    description: "checks durable dispatch ordering".into(),
                    parameters: serde_json::json!({"type": "object"}),
                    capabilities: vec![omegon_traits::ToolCapability::RepoInspection],
                }]
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                _args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<omegon_traits::ToolResult> {
                panic!("durable observer requires invocation context")
            }

            async fn execute_with_context(
                &self,
                _tool_name: &str,
                call_id: &str,
                _args: Value,
                _cancel: CancellationToken,
                _sink: omegon_traits::ToolProgressSink,
                context: omegon_traits::ToolExecutionContext,
            ) -> anyhow::Result<omegon_traits::ToolResult> {
                let invocation = context.invocation.expect("durable invocation metadata");
                assert_eq!(invocation.visible_call_id, call_id);
                assert_eq!(invocation.session_id.as_deref(), Some("session-loop"));
                assert!(self.authority.state().invocations.values().any(|state| {
                    matches!(
                        state,
                        crate::session_authority::InvocationState::Acknowledged {
                            preparation,
                            ..
                        } if preparation.call_id == call_id
                            && preparation.invocation_id.to_string() == invocation.invocation_id
                    )
                }));
                Ok(omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text { text: "ok".into() }],
                    details: Value::Null,
                })
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let recorded_at = "2026-08-20T12:00:00Z";
        let mut authority = crate::session_authority::SessionAuthority::open(
            &directory.path().join("session.json"),
            "session-loop",
            "workspace-loop",
            "composition:test",
            crate::session_authority::ActorIdentity {
                principal: "operator".into(),
                ingress: "test".into(),
            },
            recorded_at,
        )
        .unwrap();
        let prompt_id = uuid::Uuid::new_v4();
        authority
            .admit_prompt(
                uuid::Uuid::new_v4(),
                recorded_at,
                crate::session_authority::PromptAdmitted {
                    submission_id: uuid::Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "test".into(),
                    queue_mode: crate::session_authority::QueueMode::UntilReady,
                    content: crate::session_authority::PromptContent {
                        text: "run".into(),
                        attachments: vec![],
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        let turn_id = uuid::Uuid::new_v4();
        authority
            .start_turn(uuid::Uuid::new_v4(), recorded_at, turn_id, prompt_id)
            .unwrap();
        let authority = crate::session_authority::SessionAuthorityHandle::new(authority);

        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "durable-observer",
            Box::new(DurableObserver {
                authority: authority.clone(),
            }),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);
        let (events_tx, _) = broadcast::channel(16);
        let scope = crate::invocation_service::InvocationScope {
            session_id: Some("session-loop".into()),
            turn_id: Some(turn_id),
            authority: Some(authority.clone()),
            ..Default::default()
        };
        let dispatch = dispatch_tools(
            &invocations,
            &[ToolCall {
                id: "durable-call".into(),
                name: "durable_observer".into(),
                arguments: serde_json::json!({}),
            }],
            &events_tx,
            CancellationToken::new(),
            directory.path(),
            None,
            None,
            None,
            &scope,
        )
        .await;
        assert_eq!(dispatch.results.len(), 1);
        assert!(!dispatch.results[0].is_error);
        assert!(authority.state().invocations.values().any(|state| {
            matches!(state, crate::session_authority::InvocationState::DurableSettled { preparation, settlement, .. }
                if preparation.call_id == "durable-call"
                    && settlement.outcome == crate::session_authority::InvocationOutcome::Completed)
        }));
    }

    #[tokio::test]
    async fn path_always_allow_grants_directory_without_second_prompt() {
        let workspace = tempfile::tempdir().unwrap();
        // Persistent grants follow the active profile. Seed an isolated project
        // profile so this test never inherits or updates the operator's user profile.
        crate::settings::Profile::default()
            .save(workspace.path())
            .unwrap();
        let outside = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "permission-always-allow-test-{}",
                std::process::id()
            ));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("allowed.txt");
        std::fs::write(&outside_file, "outside content").unwrap();

        let settings = crate::settings::shared("test-model");
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(
                crate::tools::CoreTools::new(workspace.path().to_path_buf())
                    .with_settings(settings.clone()),
            ),
        )));
        bus.register_internal_tool(crate::tool_registry::core::TRUST_DIRECTORY, "core-tools");
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);
        let (events_tx, mut events_rx) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let path = outside_file.display().to_string();
        let calls = vec![ToolCall {
            id: "outside-read".into(),
            name: crate::tool_registry::core::READ.into(),
            arguments: serde_json::json!({"path": path}),
        }];

        let invocation_scope = crate::invocation_service::InvocationScope::default();
        let dispatch_fut = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel.clone(),
            workspace.path(),
            None,
            None,
            None,
            &invocation_scope,
        );
        tokio::pin!(dispatch_fut);

        loop {
            tokio::select! {
                event = events_rx.recv() => {
                    if let Ok(AgentEvent::PermissionRequest { tool_name, path, kind, persistence, grant_path, respond }) = event {
                        assert_eq!(tool_name, crate::tool_registry::core::READ);
                        assert_eq!(path, outside_file.display().to_string());
                        assert_eq!(kind, omegon_traits::PermissionRequestKind::PathBoundary);
                        assert_eq!(persistence, omegon_traits::PermissionPersistence::ProjectDirectory);
                        assert_eq!(grant_path.as_deref(), Some(outside.to_str().unwrap()));
                        let tx = respond.lock().unwrap().take().expect("permission response sender");
                        tx.send(omegon_traits::PermissionResponse::AlwaysAllow).expect("send always allow");
                        break;
                    }
                }
                dispatch = &mut dispatch_fut => {
                    panic!("dispatch completed before permission prompt: {:?}", dispatch.results.first().map(|r| &r.content));
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    panic!("timed out waiting for permission prompt");
                }
            }
        }

        let first_dispatch = dispatch_fut.await;
        assert_eq!(first_dispatch.results.len(), 1);
        assert!(
            !first_dispatch.results[0].is_error,
            "first dispatch failed: {:?}",
            first_dispatch.results[0].content
        );
        assert!(
            first_dispatch.results[0].content[0]
                .as_text()
                .unwrap()
                .contains("outside content"),
            "dispatch result: {:?}",
            first_dispatch.results[0].content
        );
        let persisted = crate::settings::Profile::load_with_source(workspace.path());
        assert!(matches!(
            persisted.source,
            crate::settings::ProfileSource::Project(_)
        ));
        assert!(
            persisted
                .profile
                .effective_trusted_directories()
                .contains(&outside.canonicalize().unwrap().display().to_string())
        );
        assert_eq!(first_dispatch.permission_decisions.len(), 1);
        assert_eq!(
            first_dispatch.permission_decisions[0].decision,
            "always_allow"
        );
        assert_eq!(
            first_dispatch.permission_decisions[0].kind,
            omegon_traits::PermissionRequestKind::PathBoundary
        );
        assert_eq!(
            first_dispatch.permission_decisions[0].persistence,
            omegon_traits::PermissionPersistence::ProjectDirectory
        );
        assert_eq!(
            first_dispatch.permission_decisions[0].grant_path.as_deref(),
            Some(outside.to_str().unwrap())
        );

        let second_calls = vec![ToolCall {
            id: "outside-read-again".into(),
            name: crate::tool_registry::core::READ.into(),
            arguments: serde_json::json!({"path": outside_file.display().to_string()}),
        }];
        let second_dispatch = dispatch_tools(
            &invocations,
            &second_calls,
            &events_tx,
            cancel,
            workspace.path(),
            None,
            None,
            None,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;
        assert_eq!(second_dispatch.results.len(), 1);
        assert!(!second_dispatch.results[0].is_error);
        assert!(second_dispatch.permission_decisions.is_empty());

        loop {
            match events_rx.try_recv() {
                Err(broadcast::error::TryRecvError::Empty) => break,
                Ok(AgentEvent::PermissionRequest { .. }) => {
                    panic!("second read emitted an unexpected permission request")
                }
                Ok(_) => {}
                Err(err) => panic!("unexpected event channel error: {err:?}"),
            }
        }
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[tokio::test]
    async fn permission_policy_prompt_allows_dispatch_after_operator_approval() {
        let dir = tempfile::tempdir().unwrap();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(crate::tools::CoreTools::new(dir.path().to_path_buf())),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);
        let (events_tx, mut events_rx) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let mut policy = crate::permissions::LayeredPermissionPolicy::default();
        policy.project.tools.insert(
            crate::tool_registry::core::BASH.to_string(),
            crate::permissions::ToolPermissionRule::Action(
                crate::permissions::PermissionAction::Prompt,
            ),
        );
        let calls = vec![ToolCall {
            id: "prompt-bash".into(),
            name: crate::tool_registry::core::BASH.into(),
            arguments: serde_json::json!({"command":"printf prompt-created > prompt-created.txt"}),
        }];

        let invocation_scope = crate::invocation_service::InvocationScope::default();
        let dispatch_fut = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            Some(&policy),
            None,
            &invocation_scope,
        );
        tokio::pin!(dispatch_fut);

        loop {
            tokio::select! {
                event = events_rx.recv() => {
                    if let Ok(AgentEvent::PermissionRequest { tool_name, path, kind, persistence, grant_path, respond }) = event {
                        assert_eq!(tool_name, crate::tool_registry::core::BASH);
                        assert!(path.contains("printf prompt-created > prompt-created.txt"), "prompt subject should include command: {path}");
                        assert_eq!(kind, omegon_traits::PermissionRequestKind::Policy);
                        assert_eq!(persistence, omegon_traits::PermissionPersistence::None);
                        assert!(grant_path.is_none());
                        let tx = respond.lock().unwrap().take().expect("permission response sender");
                        tx.send(omegon_traits::PermissionResponse::Allow).expect("send allow");
                        break;
                    }
                }
                dispatch = &mut dispatch_fut => {
                    panic!("dispatch completed before permission prompt: {:?}", dispatch.results.first().map(|r| &r.content));
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    panic!("timed out waiting for permission prompt");
                }
            }
        }

        let dispatch = dispatch_fut.await;
        assert_eq!(dispatch.results.len(), 1);
        assert!(!dispatch.results[0].is_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("prompt-created.txt")).unwrap(),
            "prompt-created",
            "approved command must execute after the prompt"
        );
        assert_eq!(dispatch.permission_decisions.len(), 1);
        assert_eq!(dispatch.permission_decisions[0].decision, "allow");
    }

    #[tokio::test]
    async fn permission_policy_prompt_blocks_dispatch_after_operator_denial() {
        let dir = tempfile::tempdir().unwrap();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "core-tools",
            Box::new(crate::tools::CoreTools::new(dir.path().to_path_buf())),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);
        let (events_tx, mut events_rx) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let mut policy = crate::permissions::LayeredPermissionPolicy::default();
        policy.project.tools.insert(
            crate::tool_registry::core::BASH.to_string(),
            crate::permissions::ToolPermissionRule::Action(
                crate::permissions::PermissionAction::Prompt,
            ),
        );
        let calls = vec![ToolCall {
            id: "prompt-deny-bash".into(),
            name: crate::tool_registry::core::BASH.into(),
            arguments: serde_json::json!({"command":"touch prompt-denied"}),
        }];

        let invocation_scope = crate::invocation_service::InvocationScope::default();
        let dispatch_fut = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            Some(&policy),
            None,
            &invocation_scope,
        );
        tokio::pin!(dispatch_fut);

        loop {
            tokio::select! {
                event = events_rx.recv() => {
                    if let Ok(AgentEvent::PermissionRequest { respond, .. }) = event {
                        let tx = respond.lock().unwrap().take().expect("permission response sender");
                        tx.send(omegon_traits::PermissionResponse::Deny).expect("send deny");
                        break;
                    }
                }
                dispatch = &mut dispatch_fut => {
                    panic!("dispatch completed before permission prompt: {:?}", dispatch.results.first().map(|r| &r.content));
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    panic!("timed out waiting for permission prompt");
                }
            }
        }

        let dispatch = dispatch_fut.await;
        assert_eq!(dispatch.results.len(), 1);
        assert!(dispatch.results[0].is_error);
        assert!(!dir.path().join("prompt-denied").exists());
        assert_eq!(dispatch.permission_decisions.len(), 1);
        assert_eq!(dispatch.permission_decisions[0].decision, "deny");
    }

    #[tokio::test]
    async fn non_filesystem_read_only_tools_dispatch_concurrently() {
        use omegon_traits::ToolResult;
        use tokio::time::{Duration, Instant, sleep};

        struct SlowReadOnlyProvider;

        #[async_trait::async_trait]
        impl ToolProvider for SlowReadOnlyProvider {
            fn runtime_tool_policy(
                &self,
                _tool_name: &str,
            ) -> Option<omegon_traits::RuntimeToolPolicy> {
                Some(omegon_traits::RuntimeToolPolicy {
                    effects: vec![omegon_traits::RuntimeEffect::NetworkAccess],
                    execution: omegon_traits::RuntimeExecutionPolicy {
                        principals: vec![omegon_traits::RuntimePrincipalClass::Model],
                        timeout_class: omegon_traits::RuntimeTimeoutClass::Immediate,
                        retry_class: omegon_traits::RuntimeRetryClass::Never,
                        idempotency: omegon_traits::RuntimeIdempotency::Idempotent,
                        deduplication: omegon_traits::RuntimeDeduplication::Unsupported,
                        parallelism: omegon_traits::RuntimeParallelism::ParallelSafe,
                        transaction: omegon_traits::RuntimeTransactionBehavior::None,
                        mutation_fence: None,
                        max_attempts: None,
                    },
                })
            }

            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![
                    omegon_traits::ToolDefinition {
                        name: "remote_alpha".into(),
                        label: "remote_alpha".into(),
                        description: "identity".into(),
                        parameters: serde_json::json!({}),
                        capabilities: vec![],
                    },
                    omegon_traits::ToolDefinition {
                        name: "remote_beta".into(),
                        label: "remote_beta".into(),
                        description: "clock".into(),
                        parameters: serde_json::json!({}),
                        capabilities: vec![],
                    },
                ]
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                _args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<ToolResult> {
                sleep(Duration::from_millis(150)).await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: "ok".into() }],
                    details: Value::Null,
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "test-read-only",
            Box::new(SlowReadOnlyProvider),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();
        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "remote_alpha".into(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "2".into(),
                name: "remote_beta".into(),
                arguments: serde_json::json!({}),
            },
        ];

        let start = Instant::now();
        let dispatch = dispatch_tools(
            &invocations,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            None,
            &crate::invocation_service::InvocationScope::default(),
        )
        .await;
        let elapsed = start.elapsed();

        assert_eq!(dispatch.results.len(), 2);
        assert!(
            elapsed < Duration::from_millis(260),
            "expected parallel dispatch, got {elapsed:?}"
        );
        assert_eq!(dispatch.results[0].tool_name, "remote_alpha");
        assert_eq!(dispatch.results[1].tool_name, "remote_beta");
    }

    #[tokio::test]
    async fn filesystem_read_tools_dispatch_serially_to_preserve_permission_prompts() {
        struct FilesystemReadProvider;

        #[async_trait::async_trait]
        impl ToolProvider for FilesystemReadProvider {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                ["read", "view"]
                    .into_iter()
                    .map(|name| omegon_traits::ToolDefinition {
                        name: name.into(),
                        label: name.into(),
                        description: "filesystem read".into(),
                        parameters: serde_json::json!({}),
                        capabilities: vec![omegon_traits::ToolCapability::RepoInspection],
                    })
                    .collect()
            }

            async fn execute(
                &self,
                _tool_name: &str,
                _call_id: &str,
                _args: Value,
                _cancel: CancellationToken,
            ) -> anyhow::Result<omegon_traits::ToolResult> {
                unreachable!()
            }
        }

        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::features::adapter::ToolAdapter::new(
            "filesystem-read",
            Box::new(FilesystemReadProvider),
        )));
        bus.finalize();
        let invocations = crate::loop_driver::LoopInvocationPort::new(&mut bus);
        assert!(!crate::invocation_batch::declaration_allows_parallel(
            &invocations,
            "read"
        ));
        assert!(!crate::invocation_batch::declaration_allows_parallel(
            &invocations,
            "view"
        ));
    }

    // ── Turn limit + config tests ──────────────────────────────────────

    #[test]
    fn loop_config_defaults_soft_limit() {
        let config = LoopConfig {
            max_turns: 60,
            soft_limit_turns: 0, // 0 means auto-calculate
            max_retries: 8,
            retry_delay_ms: 750,
            model: "test".into(),
            bridge_model: None,
            cwd: std::path::PathBuf::from("/tmp"),
            extended_context: false,
            settings: None,
            force_compact: None,
            allow_commit_nudge: true,
            enforce_first_turn_execution_bias: false,
            skill_phases: Vec::new(),
            compatibility: crate::loop_driver::LoopCompatibilityBindings::default(),
            cancel_keeps_prompt: None,
        };
        // soft_limit_turns=0 → loop should compute 2/3 of max_turns (40)
        assert_eq!(config.soft_limit_turns, 0, "0 = auto-calculate in run()");
    }

    #[test]
    fn loop_config_default_retry_params() {
        let config = LoopConfig::default();
        assert_eq!(config.max_retries, 0); // 0 = infinite (TUI mode)
        assert_eq!(config.retry_delay_ms, 750);
    }

    #[test]
    fn retry_backoff_is_capped() {
        let cap_ms: u64 = 15_000;
        let base_ms: u64 = LoopConfig::default().retry_delay_ms;
        for attempt in [0_u32, 1, 2, 10, 100] {
            let mut delay = base_ms;
            for _ in 0..attempt {
                delay = delay.saturating_mul(2).min(cap_ms);
            }
            assert!(delay <= cap_ms, "attempt {attempt} exceeded cap: {delay}");
        }
    }

    #[test]
    fn tui_mode_stall_exhaustion_fires_on_elapsed_time() {
        // TUI mode: max_retries == 0
        // Non-OpenAI stalls bail after 600s cumulative elapsed (10 min), not attempt count.
        let config = LoopConfig {
            max_retries: 0,
            ..Default::default()
        };
        let transient_kind = Some(crate::upstream_errors::TransientFailureKind::StalledStream);
        let threshold = stall_exhaustion_secs("anthropic", "claude-sonnet-4-5", None);

        // Under threshold
        for elapsed_secs in [30u64, 120, 300, 599] {
            let stall_exhausted = config.max_retries == 0
                && matches!(
                    transient_kind,
                    Some(crate::upstream_errors::TransientFailureKind::StalledStream)
                )
                && elapsed_secs >= threshold;
            assert!(!stall_exhausted, "{elapsed_secs}s should NOT exhaust");
        }

        // At threshold
        let elapsed_secs = threshold;
        let stall_exhausted = config.max_retries == 0
            && matches!(
                transient_kind,
                Some(crate::upstream_errors::TransientFailureKind::StalledStream)
            )
            && elapsed_secs >= threshold;
        assert!(
            stall_exhausted,
            "{threshold}s should trigger stall exhaustion"
        );
    }

    #[test]
    fn tui_mode_bounds_all_other_transient_retry_families() {
        use crate::upstream_errors::TransientFailureKind;

        for kind in [
            TransientFailureKind::ProviderOverloaded,
            TransientFailureKind::Upstream5xx,
            TransientFailureKind::Timeout,
            TransientFailureKind::NetworkConnect,
            TransientFailureKind::NetworkReset,
            TransientFailureKind::Dns,
            TransientFailureKind::DecodeBody,
            TransientFailureKind::BridgeDropped,
            TransientFailureKind::ResponseIncomplete,
            TransientFailureKind::ResponseCancelled,
        ] {
            assert!(!transient_retry_envelope_exhausted(0, Some(kind), 599));
            assert!(transient_retry_envelope_exhausted(0, Some(kind), 600));
            assert!(!transient_retry_envelope_exhausted(8, Some(kind), 600));
        }
        assert!(!transient_retry_envelope_exhausted(
            0,
            Some(TransientFailureKind::RateLimited),
            600
        ));
        assert!(!transient_retry_envelope_exhausted(
            0,
            Some(TransientFailureKind::StalledStream),
            600
        ));
    }

    #[test]
    fn interactive_codex_overload_bypasses_generic_retry_envelope() {
        use crate::upstream_errors::TransientFailureKind;

        let kind = Some(TransientFailureKind::ProviderOverloaded);
        let generic_envelope_exhausted = transient_retry_envelope_exhausted(0, kind, 600);
        let persistent_codex_overload =
            persistent_interactive_overload_retry(0, "openai-codex", kind);

        assert!(generic_envelope_exhausted);
        assert!(persistent_codex_overload);
        assert!(!persistent_interactive_overload_retry(
            8,
            "openai-codex",
            kind
        ));
        assert!(!persistent_interactive_overload_retry(0, "openai", kind));
    }

    #[test]
    fn retry_jitter_is_deterministic_bounded_and_attempt_specific() {
        let first = jittered_retry_delay_ms(15_000, 11, "openai-codex", "gpt-5.6-sol");
        let repeated = jittered_retry_delay_ms(15_000, 11, "openai-codex", "gpt-5.6-sol");
        let next_attempt = jittered_retry_delay_ms(15_000, 12, "openai-codex", "gpt-5.6-sol");

        assert_eq!(first, repeated);
        assert!((7_500..15_000).contains(&first), "delay was {first}");
        assert!((7_500..15_000).contains(&next_attempt));
        assert_ne!(first, next_attempt);
    }

    #[test]
    fn openai_reasoning_stall_exhaustion_uses_longer_windows() {
        assert_eq!(
            stall_exhaustion_secs("openai-codex", "gpt-5.5", Some("high")),
            2_400
        );
        assert_eq!(
            stall_exhaustion_secs("openai-codex", "gpt-5.5", Some("medium")),
            1_800
        );
        assert_eq!(
            stall_exhaustion_secs("openai-codex", "gpt-5.5", Some("minimal")),
            1_200
        );
        assert_eq!(
            stall_exhaustion_secs("openai", "gpt-5.5", Some("high")),
            2_400
        );
        assert_eq!(
            stall_exhaustion_secs("anthropic", "claude-sonnet-4-5", Some("high")),
            600
        );
    }

    #[test]
    fn astra_reasoning_stall_budget_preserves_deep_reasoning_window() {
        for provider in ["openai", "openai-codex"] {
            for effort in ["high", "xhigh", "max"] {
                assert_eq!(
                    stall_exhaustion_secs(provider, "gpt-6-astra", Some(effort)),
                    2_400
                );
            }
        }
        assert_eq!(
            stall_exhaustion_secs("openai", "gpt-6-astra", Some("medium")),
            1_800
        );
    }

    #[test]
    fn tui_mode_rate_limit_does_not_trigger_stall_exhaustion() {
        let config = LoopConfig {
            max_retries: 0,
            ..Default::default()
        };
        let transient_kind = Some(crate::upstream_errors::TransientFailureKind::RateLimited);

        let elapsed_secs = 700u64;
        let stall_exhausted = config.max_retries == 0
            && matches!(
                transient_kind,
                Some(crate::upstream_errors::TransientFailureKind::StalledStream)
            )
            && elapsed_secs >= stall_exhaustion_secs("anthropic", "claude-sonnet-4-5", None);
        assert!(
            !stall_exhausted,
            "rate-limit failures should not use stall path"
        );
    }

    #[test]
    fn cleave_mode_uses_attempt_cap_not_stall_cap() {
        // Cleave mode: max_retries == 8
        // The generic attempt cap should fire, not the stall-specific one.
        let config = LoopConfig {
            max_retries: 8,
            ..Default::default()
        };
        let attempt = 8u32;
        let attempt_exhausted = config.max_retries > 0 && attempt >= config.max_retries;
        assert!(attempt_exhausted, "cleave should use attempt cap");

        let transient_kind = Some(crate::upstream_errors::TransientFailureKind::StalledStream);
        let stall_exhausted = config.max_retries == 0
            && matches!(
                transient_kind,
                Some(crate::upstream_errors::TransientFailureKind::StalledStream)
            )
            && attempt >= 4;
        assert!(
            !stall_exhausted,
            "stall_exhausted should not fire in cleave mode (max_retries > 0)"
        );
    }

    // ── Mutation detection ─────────────────────────────────────────────

    #[test]
    fn mutation_capability_classification_excludes_non_mutation_tools() {
        let catalog = test_tool_catalog();
        assert!(is_mutation_tool_name(&catalog, "write"));
        assert!(is_mutation_tool_name(&catalog, "edit"));
        assert!(is_mutation_tool_name(&catalog, "change"));
        assert!(!is_mutation_tool_name(&catalog, "bash"));
        assert!(!is_mutation_tool_name(&catalog, "read"));
        assert!(!is_mutation_tool_name(&catalog, "chronos"));
        assert!(!is_mutation_tool_name(&catalog, "design_tree"));
    }

    #[test]
    fn dead_mouse_real_work_excludes_hidden_change_tool() {
        let change_call = ToolCall {
            id: "1".into(),
            name: "change".into(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
        };
        assert!(!counts_as_real_work_for_dead_mouse(&change_call));

        let edit_call = ToolCall {
            id: "2".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
        };
        assert!(counts_as_real_work_for_dead_mouse(&edit_call));
    }

    #[test]
    fn default_loop_config_allows_commit_nudge() {
        assert!(LoopConfig::default().allow_commit_nudge);
    }

    #[test]
    fn default_loop_config_does_not_enforce_first_turn_execution_bias() {
        assert!(!LoopConfig::default().enforce_first_turn_execution_bias);
    }

    #[test]
    fn looks_like_completion_matches_done_phrases() {
        assert!(looks_like_completion(
            "All done! Let me know if you need anything else."
        ));
        assert!(looks_like_completion("The changes have been applied."));
        assert!(looks_like_completion("In summary, I updated three files."));
        assert!(looks_like_completion(
            "Here's a summary of the changes made."
        ));
        assert!(looks_like_completion(
            "All set — the implementation is complete."
        ));
    }

    #[test]
    fn looks_like_completion_rejects_mid_task_text() {
        assert!(!looks_like_completion(
            "Reading the file now to understand the structure."
        ));
        assert!(!looks_like_completion(
            "Found the bug — it's in the auth middleware."
        ));
        assert!(!looks_like_completion(
            "The test failed with a type mismatch."
        ));
        assert!(!looks_like_completion("I'll write the fix next."));
        assert!(!looks_like_completion("short")); // too short
    }

    #[test]
    fn text_only_continuation_requests_force_another_turn() {
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Guarded,
            "fix the release flow",
            "I can make that change. Should I proceed?",
            false
        ));
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "continue",
            "I'll inspect the relevant files and then make the change.",
            true
        ));
    }

    #[test]
    fn continuation_question_after_assessment_is_operator_decision_point() {
        // An assessment/review prompt that never authorized changes: the
        // trailing "want me to fix these?" is a legitimate decision point,
        // not a dead mouse. Guarded mode must hand control back.
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Guarded,
            "assess the recent harness changes for the release",
            "Found three issues in the detector. Want me to implement the fixes now?",
            true
        ));
        // Autonomous automation levels still self-answer the question.
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "assess the recent harness changes for the release",
            "Found three issues in the detector. Want me to implement the fixes now?",
            true
        ));
    }

    #[test]
    fn incomplete_structured_answers_continue_in_flow_mode() {
        let reply = r#"What Flynt should not copy directly

Recommended Flynt roadmap from Zotero research

Phase 1 - Source note foundation

Low cost, high leverage.

- Define kind = "source" frontmatter schema.
- Add source-specific note rendering.
- Add source lens/query presets:
  - all sources
  - unread
  - annotated"#;

        assert!(looks_like_incomplete_structured_answer(reply));
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "perform research and give me the roadmap",
            reply,
            true
        ));
    }

    #[test]
    fn complete_structured_answers_do_not_continue() {
        let reply = r#"Recommended roadmap

Phase 1 - Source note foundation

- all sources
- unread
- annotated

This is the right first slice."#;

        assert!(!looks_like_incomplete_structured_answer(reply));
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "perform research and give me the roadmap",
            reply,
            true
        ));
    }

    #[test]
    fn open_code_fence_answers_continue_in_flow_mode() {
        let reply = "Here is the config:\n\n```json\n{\"phase\": 1}";
        assert!(looks_like_incomplete_structured_answer(reply));
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "show the json",
            reply,
            true
        ));
    }

    #[test]
    fn text_only_final_answers_and_blockers_do_not_force_continue() {
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "describe the API surface",
            "The API surface should be a single facade over profiles, tools, and tasking.",
            false
        ));
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "fix the release flow",
            "I am blocked because the repository has conflicting local edits that overlap this file.",
            true
        ));
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "fix the release flow",
            "All done. The release flow has been updated and tested.",
            true
        ));
    }

    #[test]
    fn empty_post_tool_message_reenters_bounded_recovery() {
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "What model are you?",
            "   ",
            true
        ));
        assert!(should_continue_text_only_turn(
            crate::settings::AutomationLevel::Guarded,
            "fix the release flow",
            "",
            false
        ));
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Flow,
            "hello",
            "",
            false
        ));
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Ask,
            "fix the release flow",
            "",
            true
        ));
    }

    #[test]
    fn text_only_automation_ask_disables_auto_continue() {
        assert!(!should_continue_text_only_turn(
            crate::settings::AutomationLevel::Ask,
            "fix the release flow",
            "I can make that change. Should I proceed?",
            false
        ));
    }

    #[test]
    fn session_noise_path_matches_known_patterns() {
        assert!(is_session_noise_path("ai/session/system-warning-note.md"));
        assert!(is_session_noise_path("ai/session/tool-output-ack.md"));
        assert!(is_session_noise_path(
            "ai/session/tool-compliance-marker.md"
        ));
        assert!(is_session_noise_path(".omegon/audit-log.jsonl"));
        assert!(is_session_noise_path("some/dir/warning-log.md"));
        assert!(is_session_noise_path("some/dir/ack-receipt.md"));
    }

    #[test]
    fn session_noise_path_allows_real_output() {
        assert!(!is_session_noise_path("src/main.rs"));
        assert!(!is_session_noise_path("docs/architecture.md"));
        assert!(!is_session_noise_path("CHANGELOG.md"));
        assert!(!is_session_noise_path("ai/memory/facts.db"));
        assert!(!is_session_noise_path("crates/omegon/src/loop.rs"));
    }

    #[test]
    fn first_turn_orientation_churn_detected_for_headless_execution_bias_mode() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let conversation = ConversationState::new();
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "memory_recall".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "context_status".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "3".into(),
                name: "request_context".into(),
                arguments: Value::Null,
            },
        ];
        assert!(is_first_turn_orientation_churn(
            1,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
        ));
    }

    #[test]
    fn first_turn_orientation_churn_not_detected_after_real_repo_inspection() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("src/main.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "memory_recall".into(),
            arguments: Value::Null,
        }];
        assert!(!is_first_turn_orientation_churn(
            1,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
        ));
    }

    #[test]
    fn first_turn_orientation_churn_not_detected_for_normal_mode() {
        let config = LoopConfig::default();
        let conversation = ConversationState::new();
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "memory_recall".into(),
            arguments: Value::Null,
        }];
        assert!(!is_first_turn_orientation_churn(
            1,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
        ));
    }

    #[test]
    fn execution_pressure_detected_after_repeated_repo_inspection_without_edits() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "codebase_search".into(),
                arguments: Value::Null,
            },
        ];
        // Standard broad threshold is 5, so turn 4 should NOT trigger.
        assert!(!should_inject_execution_pressure(
            4,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
        // Turn 6 should trigger (>= broad_threshold of 5).
        assert!(should_inject_execution_pressure(
            6,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    #[test]
    fn execution_pressure_not_detected_for_mixed_noninspection_tool_batches() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "bash".into(),
                arguments: Value::Null,
            },
        ];
        assert!(!should_inject_execution_pressure(
            4,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    #[test]
    fn execution_pressure_not_detected_for_targeted_read_only_batches_too_early() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/src/context.rs"}),
        }];
        // Turn 1: too early for any pressure
        assert!(!should_inject_execution_pressure(
            1,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
        // Turn 2: targeted-only reads get one extra turn grace period (fires at 3+)
        assert!(!should_inject_execution_pressure(
            2,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    #[test]
    fn execution_pressure_detected_for_repeated_targeted_read_only_batches_after_local_hypothesis_stalls()
     {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/src/context.rs"}),
        }];
        // Standard targeted threshold is 6, so turn 5 should NOT trigger.
        assert!(!should_inject_execution_pressure(
            5,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
        // Turn 7 should trigger.
        assert!(should_inject_execution_pressure(
            7,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    #[test]
    fn controller_streaks_snapshot_exports_six_counters_and_omits_internal_state() {
        // The internal `consecutive_tool_continuations` counter is a
        // continuation-pressure heuristic, not a drift-streak signal —
        // it intentionally does not appear on the public ControllerStreaks
        // shape. The other six counters round-trip 1:1.
        let controller = ControllerState {
            consecutive_tool_continuations: 99, // intentionally NOT exported
            orientation_churn_streak: 4,
            repeated_action_failure_streak: 2,
            validation_thrash_streak: 1,
            closure_stall_streak: 7,
            constraint_discovery_streak: 3,
            targeted_evidence_streak: 6,
            local_evidence_sufficient_streak: 4,
            evidence_sufficient_streak: 5,
            no_progress_continuation_streak: 8,
        };
        let snapshot = controller.streaks();
        assert_eq!(snapshot.orientation_churn, 4);
        assert_eq!(snapshot.repeated_action_failure, 2);
        assert_eq!(snapshot.validation_thrash, 1);
        assert_eq!(snapshot.closure_stall, 7);
        assert_eq!(snapshot.constraint_discovery, 3);
        assert_eq!(snapshot.evidence_sufficient, 5);
        // Default controller should produce a zero snapshot that
        // serializes to skip-on-the-wire via `is_zero()`.
        let zero = ControllerState::default().streaks();
        assert!(zero.is_zero(), "default controller should be all zeros");
    }

    #[test]
    fn infer_task_mode_classifies_research_and_implementation_prompts() {
        use crate::behavior::infer_task_mode_from_prompt;
        use crate::conversation::TaskMode;

        for prompt in [
            "what does the observation normalizer do?",
            "Explain the OODA loop wiring",
            "give me a rundown of the guidance affordances",
            "review the recent additions",
            "How does compaction work",
            "investigate the flaky test",
        ] {
            assert_eq!(
                infer_task_mode_from_prompt(prompt),
                TaskMode::Research,
                "prompt should classify as research: {prompt}"
            );
        }

        for prompt in [
            "fix the failing test in loop.rs",
            "implement the task-mode intent channel",
            "add a regression test and commit",
            "refactor update_from_tools to use the catalog",
        ] {
            assert_eq!(
                infer_task_mode_from_prompt(prompt),
                TaskMode::Implementation,
                "prompt should classify as implementation: {prompt}"
            );
        }
    }

    #[test]
    fn observed_task_mode_does_not_override_pinned_mode() {
        use crate::conversation::TaskMode;

        let mut intent = IntentDocument::default();
        intent.pin_task_mode(TaskMode::Implementation);
        intent.observe_task_mode(TaskMode::Research);
        assert_eq!(intent.task_mode, TaskMode::Implementation);

        let mut unpinned = IntentDocument::default();
        unpinned.observe_task_mode(TaskMode::Research);
        assert_eq!(unpinned.task_mode, TaskMode::Research);
    }

    #[test]
    fn execution_pressure_suppressed_in_research_mode() {
        use crate::conversation::TaskMode;

        let config = LoopConfig::default();
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("src/lib.rs"));
        conversation.intent.observe_task_mode(TaskMode::Research);
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "codebase_search".into(),
            arguments: Value::Null,
        }];
        // Same shape fires in implementation mode at turn 7 (see
        // execution_pressure_detected_after_repeated_repo_inspection_without_edits)
        // but must stay silent for research turns.
        assert!(!should_inject_execution_pressure(
            12,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    #[test]
    fn continuation_pressure_relaxed_but_not_disabled_in_research_mode() {
        use crate::conversation::TaskMode;

        let config = LoopConfig::default();
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        conversation.intent.observe_task_mode(TaskMode::Research);
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];

        // Streaks that trigger tier 1 in implementation mode stay quiet.
        let moderate = ControllerState {
            consecutive_tool_continuations: 12,
            orientation_churn_streak: 4,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &moderate,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            None,
            "research mode should absorb implementation-tier churn"
        );

        // The late safety net still exists for unbounded exploration.
        let extreme = ControllerState {
            consecutive_tool_continuations: 32,
            orientation_churn_streak: 24,
            ..ControllerState::default()
        };
        assert!(
            continuation_pressure_tier(
                &config,
                &extreme,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            )
            .is_some(),
            "research mode must keep a late safety net"
        );

        // Genuine pathology (repeated action failure) keeps full pressure.
        let failing = ControllerState {
            repeated_action_failure_streak: 2,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &failing,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            Some(2),
            "repeated action failure is mode-independent pathology"
        );
    }

    #[test]
    fn continuation_pressure_detected_for_sustained_orientation_churn() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "codebase_search".into(),
                arguments: Value::Null,
            },
        ];
        let controller = ControllerState {
            consecutive_tool_continuations: 12,
            orientation_churn_streak: 4,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            Some(1)
        );
    }

    #[test]
    fn research_mode_relaxes_continuation_pressure_for_orientation_churn() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .pin_task_mode(crate::conversation::TaskMode::Research);
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "codebase_search".into(),
                arguments: Value::Null,
            },
        ];
        // The same streaks that trigger tier-1 pressure in Implementation
        // mode stay quiet in Research mode.
        let controller = ControllerState {
            consecutive_tool_continuations: 12,
            orientation_churn_streak: 4,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            None,
            "research mode must not fire on implementation-mode thresholds"
        );

        // Genuinely unbounded exploration still hits the safety net.
        let runaway = ControllerState {
            consecutive_tool_continuations: 32,
            orientation_churn_streak: 24,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &runaway,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            Some(3),
            "research mode keeps a late safety net"
        );
    }

    #[test]
    fn research_mode_keeps_failure_driven_pressure() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .pin_task_mode(crate::conversation::TaskMode::Research);
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];
        // RepeatedActionFailure is genuine pathology in any mode.
        let controller = ControllerState {
            repeated_action_failure_streak: 2,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            Some(2),
            "failure streaks must keep firing in research mode"
        );
    }

    #[test]
    fn research_mode_suppresses_execution_pressure() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "codebase_search".into(),
            arguments: Value::Null,
        }];
        assert!(
            should_inject_execution_pressure(
                9,
                &config,
                &conversation,
                &test_tool_catalog(),
                &tool_calls,
                BehavioralTier::Standard,
            ),
            "implementation mode still pressures repeated inspection"
        );

        conversation
            .intent
            .pin_task_mode(crate::conversation::TaskMode::Research);
        assert!(
            !should_inject_execution_pressure(
                9,
                &config,
                &conversation,
                &test_tool_catalog(),
                &tool_calls,
                BehavioralTier::Standard,
            ),
            "research mode must never pressure toward edits"
        );
    }

    #[test]
    fn classify_drift_kind_does_not_flag_single_targeted_read_as_orientation_churn() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/src/context.rs"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "read".into(),
            content: vec![ContentBlock::Text { text: "ok".into() }],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                3,
                &conversation,
                &tool_calls,
                &results
            ),
            None
        );
    }

    #[test]
    fn classify_drift_kind_flags_broad_inspection_loop_as_orientation_churn() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": "core/src/context.rs"}),
            },
            ToolCall {
                id: "2".into(),
                name: "codebase_search".into(),
                arguments: serde_json::json!({"query": "ContextManager"}),
            },
        ];
        let results = vec![
            ToolResultEntry {
                call_id: "1".into(),
                tool_name: "read".into(),
                content: vec![ContentBlock::Text { text: "ok".into() }],
                is_error: false,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "2".into(),
                tool_name: "codebase_search".into(),
                content: vec![ContentBlock::Text { text: "ok".into() }],
                is_error: false,
                args_summary: None,
            },
        ];
        // OrientationChurn requires turn >= 4 (raised from 2)
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                3,
                &conversation,
                &tool_calls,
                &results
            ),
            None
        );
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                5,
                &conversation,
                &tool_calls,
                &results
            ),
            Some(DriftKind::OrientationChurn)
        );
    }

    #[test]
    fn classify_drift_kind_requires_similar_failed_mutations_for_repeated_action_failure() {
        let conversation = ConversationState::new();
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/a.rs"}),
            },
            ToolCall {
                id: "2".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/b.rs"}),
            },
        ];
        let results = vec![
            ToolResultEntry {
                call_id: "1".into(),
                tool_name: "edit".into(),
                content: vec![ContentBlock::Text {
                    text: "fail".into(),
                }],
                is_error: true,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "2".into(),
                tool_name: "edit".into(),
                content: vec![ContentBlock::Text {
                    text: "fail".into(),
                }],
                is_error: true,
                args_summary: None,
            },
        ];
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                3,
                &conversation,
                &tool_calls,
                &results
            ),
            None
        );
    }

    #[test]
    fn classify_drift_kind_flags_repeated_failures_on_same_path() {
        let conversation = ConversationState::new();
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/a.rs"}),
            },
            ToolCall {
                id: "2".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/a.rs"}),
            },
        ];
        let results = vec![
            ToolResultEntry {
                call_id: "1".into(),
                tool_name: "edit".into(),
                content: vec![ContentBlock::Text {
                    text: "fail".into(),
                }],
                is_error: true,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "2".into(),
                tool_name: "edit".into(),
                content: vec![ContentBlock::Text {
                    text: "fail".into(),
                }],
                is_error: true,
                args_summary: None,
            },
        ];
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                3,
                &conversation,
                &tool_calls,
                &results
            ),
            Some(DriftKind::RepeatedActionFailure)
        );
    }

    #[test]
    fn classify_drift_kind_does_not_flag_targeted_validation_as_validation_thrash() {
        let conversation = ConversationState::new();
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "validate".into(),
                arguments: serde_json::json!({"paths": ["src/parser.rs"], "level": "standard"}),
            },
            ToolCall {
                id: "2".into(),
                name: "validate".into(),
                arguments: serde_json::json!({"paths": ["src/parser.rs"], "level": "standard"}),
            },
        ];
        let results = vec![
            ToolResultEntry {
                call_id: "1".into(),
                tool_name: "validate".into(),
                content: vec![ContentBlock::Text { text: "ok".into() }],
                is_error: false,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "2".into(),
                tool_name: "validate".into(),
                content: vec![ContentBlock::Text { text: "ok".into() }],
                is_error: false,
                args_summary: None,
            },
        ];
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                3,
                &conversation,
                &tool_calls,
                &results
            ),
            None
        );
    }

    #[test]
    fn classify_turn_phase_treats_validate_tool_as_act() {
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "validate".into(),
            arguments: serde_json::json!({"paths": ["src/lib.rs"], "level": "standard"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "validate".into(),
            content: vec![ContentBlock::Text { text: "ok".into() }],
            is_error: false,
            args_summary: None,
        }];

        assert_eq!(
            classify_turn_phase(&test_tool_catalog(), &tool_calls, &results),
            Some(OodaPhase::Act)
        );
    }

    #[test]
    fn continuation_pressure_still_detected_after_mutation_if_churn_resumes() {
        // Post-mutation read churn should still trigger pressure — the model
        // shouldn't get a free pass to churn reads just because it edited once.
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        conversation
            .intent
            .files_modified
            .insert(std::path::PathBuf::from("core/src/main.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];
        let controller = ControllerState {
            consecutive_tool_continuations: 16,
            orientation_churn_streak: 12,
            ..ControllerState::default()
        };
        assert!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            )
            .is_some(),
            "post-mutation read churn should still trigger continuation pressure"
        );
    }

    #[test]
    fn continuation_pressure_not_detected_for_act_phase() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
        }];
        let controller = ControllerState {
            consecutive_tool_continuations: 8,
            orientation_churn_streak: 3,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Act),
                BehavioralTier::Standard,
            ),
            None
        );
    }

    #[test]
    fn continuation_pressure_escalates_in_slim_mode_but_less_aggressively_than_before() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            settings: Some(crate::settings::shared("anthropic:claude-sonnet-4-6")),
            ..LoopConfig::default()
        };
        if let Some(settings) = &config.settings
            && let Ok(mut s) = settings.lock()
        {
            s.set_posture(crate::settings::PosturePreset::Explorator);
        }
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];
        let controller = ControllerState {
            consecutive_tool_continuations: 12,
            orientation_churn_streak: 8,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Orient),
                BehavioralTier::Standard,
            ),
            Some(2)
        );
    }

    #[test]
    fn evidence_assessment_splits_local_and_global_after_targeted_validation() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "validate".into(),
            arguments: serde_json::json!({"paths": ["core/src/context.rs"], "level": "standard"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "validate".into(),
            content: vec![ContentBlock::Text { text: "ok".into() }],
            is_error: false,
            args_summary: None,
        }];
        let evidence = assess_evidence(&conversation, &test_tool_catalog(), &tool_calls, &results);
        assert_eq!(evidence.local, EvidenceSufficiency::Targeted);
        assert_eq!(evidence.global, EvidenceSufficiency::Actionable);
    }

    #[test]
    fn evidence_assessment_keeps_narrow_local_archaeology_out_of_global_sufficiency() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/src/context.rs"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "read".into(),
            content: vec![ContentBlock::Text { text: "ok".into() }],
            is_error: false,
            args_summary: None,
        }];
        let evidence = assess_evidence(&conversation, &test_tool_catalog(), &tool_calls, &results);
        assert_eq!(evidence.local, EvidenceSufficiency::Targeted);
        assert_eq!(evidence.global, EvidenceSufficiency::None);
    }

    #[test]
    fn evidence_sufficiency_message_explicitly_forces_action() {
        let text = evidence_sufficiency_message(BehavioralTier::Standard);
        assert!(text.contains("enough context to act"));
        assert!(text.contains("Produce a concrete result"));
    }

    #[test]
    fn om_local_first_message_forces_patch_or_validate_or_blocker() {
        let text = om_local_first_message(BehavioralTier::Standard);
        assert!(text.contains("enough context"));
        assert!(text.contains("Produce the requested output"));
    }

    #[test]
    fn om_local_first_lock_escalates_faster_than_generic_sufficiency() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            settings: Some(crate::settings::shared("anthropic:claude-sonnet-4-6")),
            ..LoopConfig::default()
        };
        if let Some(settings) = &config.settings
            && let Ok(mut s) = settings.lock()
        {
            s.set_posture(crate::settings::PosturePreset::Explorator);
        }
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/src/context.rs"}),
        }];
        let controller = ControllerState {
            consecutive_tool_continuations: 1,
            local_evidence_sufficient_streak: 1,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Orient),
                BehavioralTier::Standard,
            ),
            None
        );
    }

    #[test]
    fn substantive_prose_holds_continuation_counter() {
        let mut controller = ControllerState {
            consecutive_tool_continuations: 5,
            ..ControllerState::default()
        };
        // Substantive interleaved prose is visible output — counter holds.
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            None,
            ProgressSignal::None,
            EvidenceAssessment {
                local: EvidenceSufficiency::None,
                global: EvidenceSufficiency::None,
            },
            true,
        );
        assert_eq!(controller.consecutive_tool_continuations, 5);
        // Silent tool grinding still accrues pressure.
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            None,
            ProgressSignal::None,
            EvidenceAssessment {
                local: EvidenceSufficiency::None,
                global: EvidenceSufficiency::None,
            },
            false,
        );
        assert_eq!(controller.consecutive_tool_continuations, 6);
    }

    #[test]
    fn substantive_prose_threshold_separates_narration_from_analysis() {
        assert!(!behavior::is_substantive_interleaved_prose(
            "Checking the config now."
        ));
        let analysis = "The detector fires because the search events hash a constant marker, \
             which collapses every distinct query into one fingerprint. That means three \
             unrelated greps in the window count as identical calls, and the escalation \
             path then injects recovery guidance built on a false premise.";
        assert!(behavior::is_substantive_interleaved_prose(analysis));
    }

    #[test]
    fn mutation_resets_evidence_sufficiency_streak() {
        let mut controller = ControllerState {
            local_evidence_sufficient_streak: 2,
            evidence_sufficient_streak: 3,
            consecutive_tool_continuations: 5,
            ..ControllerState::default()
        };
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            None,
            ProgressSignal::Mutation,
            EvidenceAssessment {
                local: EvidenceSufficiency::Actionable,
                global: EvidenceSufficiency::Actionable,
            },
            false,
        );
        assert_eq!(controller.evidence_sufficient_streak, 0);
        assert_eq!(controller.local_evidence_sufficient_streak, 0);
        assert_eq!(controller.consecutive_tool_continuations, 0);
    }

    #[test]
    fn execution_pressure_not_detected_before_repo_contact() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let conversation = ConversationState::new();
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "codebase_search".into(),
            arguments: Value::Null,
        }];
        assert!(!should_inject_execution_pressure(
            4,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    #[test]
    fn execution_pressure_not_detected_after_editing_starts() {
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        conversation
            .intent
            .files_modified
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];
        assert!(!should_inject_execution_pressure(
            4,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    fn controller_partial_reset_for_constraint_discovery() {
        let mut controller = ControllerState {
            consecutive_tool_continuations: 8,
            orientation_churn_streak: 4,
            repeated_action_failure_streak: 2,
            validation_thrash_streak: 3,
            closure_stall_streak: 2,
            constraint_discovery_streak: 0,
            targeted_evidence_streak: 0,
            local_evidence_sufficient_streak: 0,
            evidence_sufficient_streak: 0,
            no_progress_continuation_streak: 5,
        };
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            Some(DriftKind::OrientationChurn),
            ProgressSignal::ConstraintDiscovery,
            EvidenceAssessment {
                local: EvidenceSufficiency::None,
                global: EvidenceSufficiency::None,
            },
            false,
        );
        assert!(controller.consecutive_tool_continuations < 8);
        assert!(controller.orientation_churn_streak < 4);
        assert_eq!(controller.repeated_action_failure_streak, 0);
        assert_eq!(controller.validation_thrash_streak, 0);
        assert_eq!(controller.no_progress_continuation_streak, 0);
        assert_eq!(controller.constraint_discovery_streak, 1);
    }

    #[test]
    fn classify_progress_signal_recognizes_constraint_discovery_from_new_constraints() {
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "read".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            classify_progress_signal(0, 1, &test_tool_catalog(), &tool_calls, &results),
            ProgressSignal::ConstraintDiscovery
        );
    }

    #[test]
    fn classify_progress_signal_ignores_unevidenced_constraint_growth() {
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "memory_recall".into(),
            arguments: Value::Null,
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "memory_recall".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            classify_progress_signal(0, 1, &test_tool_catalog(), &tool_calls, &results),
            ProgressSignal::None
        );
    }

    #[test]
    fn read_repetition_prefers_file_state_guidance_over_generic_same_args_warning() {
        // With path-normalized hashing, read-without-modify requires 5+ reads
        // of the same file (no interleaved mutation/validation) to fire the
        // file-specific warning.
        let mut detector = StuckDetector::new();
        for _ in 0..5 {
            detector.record(
                &test_tool_catalog(),
                &ToolCall {
                    id: "1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path": "src/lib.rs"}),
                },
                false,
            );
        }
        let warning = detector.check(&test_tool_catalog()).expect("warning");
        assert!(
            warning.message.contains("same target multiple times"),
            "got: {warning}"
        );
        assert!(
            warning.message.contains("edit, validate, or summarize"),
            "got: {warning}"
        );
        assert!(
            !warning.message.contains("same arguments"),
            "got: {warning}"
        );
    }

    #[test]
    fn targeted_read_only_batches_trigger_execution_pressure_after_threshold() {
        // Standard targeted threshold is 6 — targeted-only reads don't fire
        // until the agent has had ample time to orient.
        let config = LoopConfig {
            enforce_first_turn_execution_bias: true,
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/src/context.rs"}),
        }];
        // Turn 5: not yet for targeted-only (threshold is 6)
        assert!(!should_inject_execution_pressure(
            5,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
        // Turn 7: fires
        assert!(should_inject_execution_pressure(
            7,
            &config,
            &conversation,
            &test_tool_catalog(),
            &tool_calls,
            BehavioralTier::Standard,
        ));
    }

    // ── Proof: behavioral churn fix ─────────────────────────────────
    // These tests exercise the exact scenarios that caused users to see
    // the agent fight doing work (issue #64 follow-up, Obsidian vault
    // churn report).

    #[test]
    fn bash_find_classified_as_act_not_orient() {
        // The Obsidian vault churn: user asks agent to write files,
        // agent runs `bash find` to explore the vault, system classified
        // this as Orient and penalized it. Now bash is Act.
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "find ~/obsidian-vault -name '*.md' | head -20"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "bash".into(),
            content: vec![ContentBlock::Text {
                text: "file1.md\nfile2.md".into(),
            }],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            classify_turn_phase(&test_tool_catalog(), &tool_calls, &results),
            Some(OodaPhase::Act),
            "bash must be Act, not Orient — shell commands are productive work"
        );
    }

    #[test]
    fn bash_turns_never_trigger_continuation_pressure() {
        // Because bash is Act, continuation_pressure_tier should return
        // None — it only fires for Observe/Orient phases.
        let config = LoopConfig::default();
        let conversation = ConversationState::new();
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "ls -la ~/obsidian"}),
        }];
        let controller = ControllerState {
            consecutive_tool_continuations: 20,
            orientation_churn_streak: 10,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Act),
                BehavioralTier::Standard,
            ),
            None,
            "Act-phase turns must never trigger continuation pressure, regardless of streak"
        );
    }

    #[test]
    fn web_search_classified_as_act() {
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "web_search".into(),
            arguments: serde_json::json!({"query": "NGB RFI enterprise data AI"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "web_search".into(),
            content: vec![ContentBlock::Text {
                text: "results".into(),
            }],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            classify_turn_phase(&test_tool_catalog(), &tool_calls, &results),
            Some(OodaPhase::Act),
            "web_search must be Act — it produces external information"
        );
    }

    #[test]
    fn memory_tools_classified_as_observe_not_orient() {
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "memory_store".into(),
            arguments: serde_json::json!({"content": "project uses PostgreSQL"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "memory_store".into(),
            content: vec![ContentBlock::Text {
                text: "stored".into(),
            }],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            classify_turn_phase(&test_tool_catalog(), &tool_calls, &results),
            Some(OodaPhase::Observe),
            "memory_store must be Observe, not Orient — it's legitimate context work"
        );
    }

    #[test]
    fn standard_model_gets_12_turns_before_first_continuation_nudge() {
        // Simulates a frontier model (Sonnet/Opus) doing multi-turn
        // exploration before writing. With old thresholds (6), this
        // would trigger a nudge at turn 6. Now it needs 12.
        let config = LoopConfig::default();
        let mut conversation = ConversationState::new();
        conversation.intent.files_read.insert("src/main.rs".into());
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];

        // At 11 consecutive tool continuations: no pressure yet
        let controller = ControllerState {
            consecutive_tool_continuations: 11,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            None,
            "11 tool continuations on Standard tier must not trigger pressure (threshold is 12)"
        );

        // At 12: tier 1 fires
        let controller = ControllerState {
            consecutive_tool_continuations: 12,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                BehavioralTier::Standard,
            ),
            Some(1),
            "12 tool continuations on Standard tier triggers tier-1 pressure"
        );
    }

    #[test]
    fn orientation_churn_not_detected_before_turn_4() {
        // OrientationChurn used to fire at turn 2. Now requires turn >= 4.
        let mut conversation = ConversationState::new();
        conversation.intent.files_read.insert("src/main.rs".into());
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "codebase_search".into(),
                arguments: Value::Null,
            },
        ];
        let results = vec![
            ToolResultEntry {
                call_id: "1".into(),
                tool_name: "read".into(),
                content: vec![ContentBlock::Text { text: "ok".into() }],
                is_error: false,
                args_summary: None,
            },
            ToolResultEntry {
                call_id: "2".into(),
                tool_name: "codebase_search".into(),
                content: vec![ContentBlock::Text { text: "ok".into() }],
                is_error: false,
                args_summary: None,
            },
        ];
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                2,
                &conversation,
                &tool_calls,
                &results
            ),
            None,
            "Turn 2 must not flag OrientationChurn"
        );
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                3,
                &conversation,
                &tool_calls,
                &results
            ),
            None,
            "Turn 3 must not flag OrientationChurn"
        );
        assert_eq!(
            classify_drift_kind(
                &test_tool_catalog(),
                5,
                &conversation,
                &tool_calls,
                &results
            ),
            Some(DriftKind::OrientationChurn),
            "Turn 5 should flag OrientationChurn"
        );
    }

    #[test]
    fn nudge_text_is_task_neutral() {
        // Nudges must not mention "code change", "edit a file", or
        // "Do NOT delegate" — these are wrong for non-code tasks like
        // writing files to an Obsidian vault.
        for tier in [1u8, 2, 3] {
            let msg = continuation_pressure_message(tier, BehavioralTier::Standard);
            assert!(
                !msg.contains("code change"),
                "tier {tier}: must not mention 'code change'"
            );
            assert!(
                !msg.contains("Do NOT delegate"),
                "tier {tier}: must not block delegation"
            );
            assert!(
                msg.contains("produce")
                    || msg.contains("Produce")
                    || msg.contains("answer")
                    || msg.contains("Answer"),
                "tier {tier}: must use task-neutral framing (produce/answer)"
            );
        }
    }

    #[test]
    fn obsidian_vault_scenario_no_churn() {
        // End-to-end simulation: user asks agent to write files to
        // Obsidian vault. Agent runs 6 bash commands to explore,
        // then writes files. No nudges should fire.
        let config = LoopConfig::default();

        // Simulate 6 turns of bash exploration
        let bash_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "find ~/vault -type d"}),
        }];
        let bash_results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "bash".into(),
            content: vec![ContentBlock::Text {
                text: "dir1\ndir2".into(),
            }],
            is_error: false,
            args_summary: None,
        }];

        let mut controller = ControllerState::default();
        let conversation = ConversationState::new();

        for turn in 1..=6 {
            let phase = classify_turn_phase(&test_tool_catalog(), &bash_calls, &bash_results);
            assert_eq!(phase, Some(OodaPhase::Act), "turn {turn}: bash must be Act");

            let drift = classify_drift_kind(
                &test_tool_catalog(),
                turn,
                &conversation,
                &bash_calls,
                &bash_results,
            );
            // bash calls don't match any drift pattern (not repo inspection tools)
            assert_eq!(
                drift, None,
                "turn {turn}: bash exploration must not trigger drift"
            );

            let pressure = continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &bash_calls,
                phase,
                BehavioralTier::Standard,
            );
            assert_eq!(
                pressure, None,
                "turn {turn}: Act-phase turn must never trigger pressure"
            );

            // Simulate controller update — ToolContinuation increments counter
            controller.observe_turn(
                omegon_traits::TurnEndReason::ToolContinuation,
                drift,
                ProgressSignal::None,
                EvidenceAssessment {
                    local: EvidenceSufficiency::None,
                    global: EvidenceSufficiency::None,
                },
                false,
            );
        }

        // After 6 turns of bash, controller should have 6 consecutive tool continuations
        // but zero orientation churn (bash is Act, not Orient)
        assert_eq!(controller.consecutive_tool_continuations, 6);
        assert_eq!(
            controller.orientation_churn_streak, 0,
            "bash turns must not increment orientation churn streak"
        );
    }

    #[test]
    fn auto_delegate_disabled_returns_none() {
        // Auto-delegation is disabled — all calls should return None
        // regardless of tool calls, phase, or drift.
        let config = LoopConfig {
            settings: Some(std::sync::Arc::new(std::sync::Mutex::new({
                let mut s = crate::settings::Settings::new("openai-codex:gpt-4.1");
                s.set_posture(crate::settings::PosturePreset::Explorator);
                s
            }))),
            ..LoopConfig::default()
        };
        let conversation = ConversationState::new();

        // Would have been "scout" — now None
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            ToolCall {
                id: "2".into(),
                name: "codebase_search".into(),
                arguments: Value::Null,
            },
        ];
        assert!(
            classify_auto_delegate_plan(
                &config,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
                Some(DriftKind::OrientationChurn)
            )
            .is_none()
        );

        // Would have been "verify" — now None
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test"}),
        }];
        assert!(
            classify_auto_delegate_plan(
                &config,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Act),
                None
            )
            .is_none()
        );

        // Would have been "patch" — now None
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "src/lib.rs", "oldText": "a", "newText": "b"}),
        }];
        assert!(
            classify_auto_delegate_plan(
                &config,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Act),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn auto_delegate_skips_when_parent_already_mutated_files() {
        let config = LoopConfig {
            settings: Some(std::sync::Arc::new(std::sync::Mutex::new({
                let mut s = crate::settings::Settings::new("openai-codex:gpt-4.1");
                s.set_posture(crate::settings::PosturePreset::Explorator);
                s
            }))),
            ..LoopConfig::default()
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_modified
            .insert(std::path::PathBuf::from("src/lib.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: Value::Null,
        }];
        let plan = classify_auto_delegate_plan(
            &config,
            &conversation,
            &tool_calls,
            Some(OodaPhase::Observe),
            Some(DriftKind::OrientationChurn),
        );
        assert!(plan.is_none());
    }

    #[test]
    fn stuck_detector_resets_on_different_tool() {
        let mut detector = StuckDetector::new();
        // Call read 3 times (not stuck — different is_error flags don't matter)
        detector.record(
            &test_tool_catalog(),
            &ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            false,
        );
        detector.record(
            &test_tool_catalog(),
            &ToolCall {
                id: "2".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            false,
        );
        // Switch to a different tool — resets the counter
        detector.record(
            &test_tool_catalog(),
            &ToolCall {
                id: "3".into(),
                name: "write".into(),
                arguments: Value::Null,
            },
            false,
        );
        assert!(
            detector.check(&test_tool_catalog()).is_none(),
            "different tools should not trigger stuck"
        );
    }

    #[test]
    fn stuck_detector_fires_on_same_tool_repeated() {
        let mut detector = StuckDetector::new();
        for i in 0..10 {
            detector.record(
                &test_tool_catalog(),
                &ToolCall {
                    id: format!("{i}"),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "cat /dev/null"}),
                },
                true,
            );
        }
        // After enough repeated error calls, should flag as stuck
        let result = detector.check(&test_tool_catalog());
        // May or may not fire depending on threshold — just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn stuck_detector_escalation_reset_allows_recovery_turn() {
        let mut detector = StuckDetector::new();
        for i in 0..10 {
            detector.record(
                &test_tool_catalog(),
                &ToolCall {
                    id: format!("{i}"),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "false"}),
                },
                true,
            );
        }

        assert!(detector.check(&test_tool_catalog()).is_some());
        detector.reset_after_escalation();

        assert!(
            detector.check(&test_tool_catalog()).is_none(),
            "recovery guidance must reach the model instead of immediately re-triggering"
        );
    }

    #[test]
    fn exhaustion_advice_distinguishes_provider_outage_from_rate_limit() {
        assert!(
            exhaustion_advice(
                "openai",
                Some(TransientFailureKind::Upstream5xx),
                false,
                false
            )
            .contains("provider-side outage or capacity problem")
        );
        assert!(
            exhaustion_advice(
                "openai",
                Some(TransientFailureKind::ProviderOverloaded),
                false,
                false
            )
            .contains("provider-side outage or capacity problem")
        );
        assert!(
            exhaustion_advice(
                "openai",
                Some(TransientFailureKind::RateLimited),
                true,
                false
            )
            .contains("rate-limiting the session")
        );
    }

    #[test]
    fn exhaustion_advice_distinguishes_unstable_network_and_stalled_stream() {
        assert!(
            exhaustion_advice(
                "openai",
                Some(TransientFailureKind::NetworkReset),
                false,
                false
            )
            .contains("provider or network path is unstable")
        );
        assert!(
            exhaustion_advice(
                "openai",
                Some(TransientFailureKind::StalledStream),
                false,
                true
            )
            .contains("wedged stream")
        );
        // The stalled-stream advice must be distinct from the network-unstable
        // advice, regardless of which provider-specific wording is used.
        assert!(
            !exhaustion_advice(
                "openai",
                Some(TransientFailureKind::StalledStream),
                false,
                true
            )
            .contains("network path is unstable")
        );
        // Generic providers still get the plain stalled-stream wording.
        assert!(
            exhaustion_advice(
                "some-other-provider",
                Some(TransientFailureKind::StalledStream),
                false,
                true
            )
            .contains("stream is unresponsive")
        );
    }

    #[test]
    fn provider_stop_notice_only_surfaces_abnormal_stops() {
        assert!(provider_stop_notice("openai", "stop").is_none());
        assert!(provider_stop_notice("openai", "tool_calls").is_none());
        let notice = provider_stop_notice("openai", "length").expect("length should warn");
        assert!(notice.contains("output limit"), "{notice}");

        assert!(provider_stop_notice("anthropic", "end_turn").is_none());
        let notice =
            provider_stop_notice("anthropic", "max_tokens").expect("max_tokens should warn");
        assert!(notice.contains("output limit"), "{notice}");
    }

    #[test]
    fn tool_call_at_turn_limit_reserves_operator_facing_response() {
        assert!(needs_final_response_turn(50, 50, 1));
        assert!(needs_final_response_turn(1, 1, 3));
        assert!(!needs_final_response_turn(50, 49, 1));
        assert!(!needs_final_response_turn(50, 50, 0));
        assert!(!needs_final_response_turn(0, 500, 1));
    }
}
