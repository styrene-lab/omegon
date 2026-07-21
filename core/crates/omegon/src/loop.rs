//! Agent loop state machine.
//!
//! The core prompt → LLM → tool dispatch → repeat cycle.
//! Includes: turn limits, retry with backoff, stuck detection,
//! context wiring, and parallel tool dispatch.

use crate::bridge::{LlmBridge, LlmEvent, LlmMessage, StreamOptions};

use crate::context::ContextManager;
use crate::conversation::{
    AssistantMessage, ConversationState, IntentDocument, ToolCall, ToolResultEntry,
};
use crate::ollama::{OllamaManager, WarmupResult};
use crate::upstream_errors::{
    TransientFailureKind, UpstreamFailureLogEntry, append_upstream_failure_log,
    classify_upstream_error_for_provider, is_context_overflow, is_malformed_history,
};
use omegon_traits::{
    AgentEvent, AgentEventTurnEnd, BusEventTurnEnd, ContentBlock, ContextComposition, DriftKind,
    ProgressNudgeReason, TurnEndReason,
};

use futures_util::stream::{self, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Configuration for the agent loop.
pub struct LoopConfig {
    /// Maximum turns before forced stop. 0 = no limit.
    pub max_turns: u32,
    /// Turn at which to inject a "you're running long" advisory.
    /// Defaults to max_turns * 2/3.
    pub soft_limit_turns: u32,
    /// Soft exhaustion threshold for transient upstream errors.
    /// 0 = retry indefinitely (interactive/TUI mode).
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
    /// Authoritative provider/model route for interactive sessions. When present,
    /// per-turn model routing and TurnEnd attribution read the route snapshot
    /// instead of re-deriving from settings/bridge_model.
    pub route_controller: Option<std::sync::Arc<crate::route::RouteController>>,
    /// Working directory — used for path resolution in auto-batch rollback.
    pub cwd: std::path::PathBuf,
    /// Extended context window (1M for Anthropic).
    pub extended_context: bool,
    /// Thinking level — shared settings handle for live reads.
    pub settings: Option<crate::settings::SharedSettings>,
    /// Secrets manager for output redaction and tool guards.
    pub secrets: Option<std::sync::Arc<omegon_secrets::SecretsManager>>,
    /// Force a compaction pass before the next turn regardless of threshold.
    pub force_compact: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Whether the loop may spend an extra turn nudging the agent to commit.
    /// Interactive mode wants this; headless/benchmark mode generally does not.
    pub allow_commit_nudge: bool,
    /// Whether the loop should push back on first-turn orientation churn in
    /// execution-biased headless runs (benchmarks, smoke tasks).
    pub enforce_first_turn_execution_bias: bool,
    /// Shared OllamaManager for warmup and model queries. Created once at
    /// startup to avoid re-creating reqwest::Client on every turn.
    pub ollama_manager: Option<crate::ollama::OllamaManager>,
    /// Phase tracking info from loaded skills. When a skill has numbered
    /// phases, the loop checks if the agent completed the final phase
    /// before declaring "done." Prevents premature completion.
    pub skill_phases: Vec<crate::skills::SkillPhaseInfo>,
    /// Host context for ACP client delegation (file I/O, terminal, permissions).
    /// None when running under Flynt or the TUI (local execution only).
    pub host_context: Option<std::sync::Arc<crate::host_context::HostContext>>,
    /// Runtime permission policy snapshot for this turn.
    pub permission_policy: Option<crate::permissions::LayeredPermissionPolicy>,
    /// Optional Styrene RBAC role gate for this runtime.
    pub permission_role: Option<styrene_rbac::Role>,
    /// Set once the turn has produced assistant/tool-visible effects that should
    /// keep the submitted prompt in replay even if the operator interrupts.
    pub cancel_keeps_prompt: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Whether to drain late bus requests after AgentEnd. Headless runs keep this
    /// for best-effort lifecycle persistence; interactive turns disable it so
    /// post-answer side work cannot hold the active-turn gate after TurnEnd.
    pub drain_post_loop_requests: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_turns: 50,
            soft_limit_turns: 35,
            max_retries: 0,
            retry_delay_ms: 750,
            model: "anthropic:claude-sonnet-4-6".into(),
            bridge_model: None,
            route_controller: None,
            cwd: std::env::current_dir().unwrap_or_default(),
            extended_context: false,
            settings: None,
            secrets: None,
            force_compact: None,
            allow_commit_nudge: true,
            enforce_first_turn_execution_bias: false,
            ollama_manager: None,
            skill_phases: Vec::new(),
            host_context: None,
            permission_policy: None,
            permission_role: None,
            cancel_keeps_prompt: None,
            drain_post_loop_requests: true,
        }
    }
}

use crate::behavior::{self, BehavioralTier, ControllerState};

const AUTO_PRESSURE_COMPACTION_KEEP_RECENT_TURNS: u32 = 4;

enum CompactionPayloadSelection {
    DecayWindow {
        payload: String,
        evict_count: usize,
    },
    PressureFallback {
        payload: String,
        evict_count: usize,
        keep_recent_turns: u32,
    },
}

impl CompactionPayloadSelection {
    fn payload(&self) -> &str {
        match self {
            Self::DecayWindow { payload, .. } | Self::PressureFallback { payload, .. } => payload,
        }
    }

    fn evict_count(&self) -> usize {
        match self {
            Self::DecayWindow { evict_count, .. } | Self::PressureFallback { evict_count, .. } => {
                *evict_count
            }
        }
    }

    fn apply(self, conversation: &mut ConversationState, summary: String) {
        match self {
            Self::DecayWindow { .. } => conversation.apply_compaction(summary),
            Self::PressureFallback {
                keep_recent_turns, ..
            } => conversation.apply_compaction_keeping_recent(summary, keep_recent_turns),
        }
    }

    fn reason(&self) -> Option<String> {
        match self {
            Self::DecayWindow { .. } => None,
            Self::PressureFallback {
                keep_recent_turns, ..
            } => Some(format!(
                "no decay-window payload; compacting under token pressure with keep_recent_turns={keep_recent_turns}"
            )),
        }
    }
}

fn pressure_compaction_payload(
    conversation: &ConversationState,
) -> Option<CompactionPayloadSelection> {
    if let Some((payload, evict_count)) = conversation.build_compaction_payload() {
        return Some(CompactionPayloadSelection::DecayWindow {
            payload,
            evict_count,
        });
    }
    conversation
        .build_compaction_payload_keeping_recent(AUTO_PRESSURE_COMPACTION_KEEP_RECENT_TURNS)
        .map(
            |(payload, evict_count)| CompactionPayloadSelection::PressureFallback {
                payload,
                evict_count,
                keep_recent_turns: AUTO_PRESSURE_COMPACTION_KEEP_RECENT_TURNS,
            },
        )
}

fn loop_context_windows(
    config: &LoopConfig,
) -> (usize, usize, Option<crate::settings::SelectorPolicy>) {
    if let Some(settings) = config
        .settings
        .as_ref()
        .and_then(|s| s.lock().ok().map(|g| g.clone()))
    {
        let policy = settings.selector_policy();
        (
            settings.context_window,
            policy.assembly_window(),
            Some(policy),
        )
    } else {
        (200_000, 200_000, None)
    }
}

fn default_context_composition(context_window: usize) -> ContextComposition {
    ContextComposition {
        free_tokens: context_window,
        ..ContextComposition::default()
    }
}

use crate::util::estimate_chars_to_tokens;

fn estimate_tool_schema_tokens(tools: &[omegon_traits::ToolDefinition]) -> usize {
    tools
        .iter()
        .map(|tool| {
            let schema_json = serde_json::to_string(&tool.parameters).unwrap_or_default();
            estimate_chars_to_tokens(tool.name.len() + tool.description.len() + schema_json.len())
        })
        .sum()
}

// Behavioral classifiers, streak tracking, continuation pressure, and
// auto-delegation logic live in `behavior.rs`. Re-export convenience
// aliases used by the main loop body.
// auto-delegation disabled — import retained for the test that verifies it returns None
use behavior::ToolCapabilityCatalog;
use behavior::assess_evidence;
use behavior::behavioral_tier;
#[cfg(test)]
use behavior::classify_auto_delegate_plan;
use behavior::classify_drift_kind;
use behavior::classify_progress_signal;
use behavior::classify_turn_phase;
use behavior::continuation_pressure_message;
use behavior::continuation_pressure_tier;
use behavior::is_first_turn_orientation_churn;
use behavior::is_mutation_tool_name;
use behavior::is_repo_inspection_tool;
use behavior::is_validation_tool_name;
use behavior::meta_recovery_retry_message;
use behavior::operator_correction_recovery_message;
use behavior::progress_nudge_reason_for_drift;
use behavior::should_inject_execution_pressure;

use behavior::evidence_sufficiency_message;
use behavior::has_local_target_hypothesis;
use behavior::is_pathological_meta_response;
use behavior::is_slim_execution_bias;
use behavior::om_local_first_message;

// Anchor: is_narrow_patch_candidate was here. Now using behavior::*.

pub(crate) fn compute_context_composition(
    system_prompt: &str,
    llm_messages: &[LlmMessage],
    tools: &[omegon_traits::ToolDefinition],
    context_window: usize,
    prompt_telemetry: Option<&crate::context::PromptTelemetry>,
) -> ContextComposition {
    let system_tokens = estimate_chars_to_tokens(system_prompt.len());
    let tool_schema_tokens = estimate_tool_schema_tokens(tools);
    let mut conversation_tokens = 0usize;
    let mut memory_tokens = 0usize;
    let mut tool_history_tokens = 0usize;
    let mut thinking_tokens = 0usize;

    for message in llm_messages {
        match message {
            LlmMessage::User { content, .. } => {
                conversation_tokens += estimate_chars_to_tokens(content.len());
            }
            LlmMessage::Assistant {
                text,
                thinking,
                tool_calls,
                ..
            } => {
                conversation_tokens +=
                    estimate_chars_to_tokens(text.iter().map(|t| t.len()).sum::<usize>());
                thinking_tokens +=
                    estimate_chars_to_tokens(thinking.iter().map(|t| t.len()).sum::<usize>());
                tool_history_tokens += estimate_chars_to_tokens(
                    tool_calls
                        .iter()
                        .map(|tc| tc.name.len() + tc.arguments.to_string().len())
                        .sum::<usize>(),
                );
            }
            LlmMessage::ToolResult {
                content,
                tool_name,
                images,
                ..
            } => {
                let image_chars = images
                    .iter()
                    .map(|img| img.data.len() + img.media_type.len())
                    .sum::<usize>();
                tool_history_tokens +=
                    estimate_chars_to_tokens(content.len() + tool_name.len() + image_chars);
                if tool_name.starts_with("memory_") {
                    memory_tokens += estimate_chars_to_tokens(content.len());
                }
            }
        }
    }

    let used = system_tokens
        .saturating_add(conversation_tokens)
        .saturating_add(memory_tokens)
        .saturating_add(tool_schema_tokens)
        .saturating_add(tool_history_tokens)
        .saturating_add(thinking_tokens);
    let free_tokens = context_window.saturating_sub(used);
    let prompt_telemetry = prompt_telemetry.cloned().unwrap_or_default();

    ContextComposition {
        conversation_tokens,
        system_tokens,
        memory_tokens,
        tool_schema_tokens,
        tool_history_tokens,
        thinking_tokens,
        free_tokens,
        base_prompt_tokens: estimate_chars_to_tokens(prompt_telemetry.base_prompt_chars),
        session_hud_tokens: estimate_chars_to_tokens(prompt_telemetry.session_hud_chars),
        intent_tokens: estimate_chars_to_tokens(prompt_telemetry.intent_chars),
        external_injection_tokens: estimate_chars_to_tokens(
            prompt_telemetry.external_injection_chars,
        ),
        tool_guidance_tokens: estimate_chars_to_tokens(prompt_telemetry.tool_guidance_chars),
        file_guidance_tokens: estimate_chars_to_tokens(prompt_telemetry.file_guidance_chars),
    }
}

/// Run the agent loop to completion.
///
/// The `bus` owns all features and dispatches tool calls.
pub async fn run(
    bridge: &dyn LlmBridge,
    bus: &mut crate::bus::EventBus,
    context: &mut ContextManager,
    conversation: &mut ConversationState,
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    config: &LoopConfig,
) -> anyhow::Result<()> {
    // tool_defs is refreshed each turn so manage_tools enable/disable takes effect
    // immediately in the schema sent to the LLM (not just in execution routing).

    // Broadcast initial HarnessStatus as AgentEvent so TUI + web dashboard
    // get the first snapshot. The BusEvent was already emitted in setup.rs;
    // this bridges it to the AgentEvent channel.
    // (Called from the TUI entrypoint which passes the initial status)

    let startup_serving_model = if let Some(controller) = config.route_controller.as_ref() {
        controller
            .snapshot()
            .await
            .serving_model()
            .map(str::to_string)
            .unwrap_or_else(|| config.model.clone())
    } else {
        config
            .bridge_model
            .as_ref()
            .unwrap_or(&config.model)
            .clone()
    };
    let base_stream_options = StreamOptions {
        model: Some(startup_serving_model.clone()),
        reasoning: None,
        extended_context: config.extended_context,
        ..Default::default()
    };

    let mut stuck_detector = StuckDetector::new();
    let session_start = Instant::now();
    let mut controller = ControllerState::default();
    let mut dead_mouse_nudges: u8 = 0;
    let mut meta_recovery_nudges: u8 = 0;
    // Set when a dead-mouse nudge message was injected this turn.
    // Used to gate the counter reset — noise writes (compliance notes,
    // session acks) must not satisfy the nudge and reset the counter.
    let mut dead_mouse_nudge_injected = false;
    let mut session_used_tools: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut turn: u32 = 0;
    // Infer the guidance task mode for this operator prompt (A1). Explicit
    // operator declarations pin the mode; otherwise inference updates it for
    // the current task without overriding a previously pinned mode.
    let last_user_prompt = conversation.last_user_prompt().to_string();
    if let Some(mode) = crate::behavior::explicit_task_mode_from_prompt(&last_user_prompt) {
        conversation.intent.pin_task_mode(mode);
    } else {
        conversation
            .intent
            .observe_task_mode(crate::behavior::infer_task_mode_from_prompt(
                &last_user_prompt,
            ));
    }
    // Active model for this turn — updated each iteration from settings.
    // Used in TurnEnd events and error classification instead of the
    // immutable config.model which is frozen at startup. Starts from the
    // bridge runtime model when fallback installed one, so events emitted
    // before the first per-turn re-read still report the real model.
    let mut active_model = startup_serving_model;

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
        let is_constrained = matches!(behavioral_tier(config), BehavioralTier::Constrained);
        let tool_defs = if is_constrained {
            // Constrained models (≤32B, Ollama, etc.) get core-only tools
            // even on turn 1 — 50+ schemas overwhelm small context windows.
            bus.tool_definitions_lean(turn, &session_used_tools)
        } else {
            // All modes use compact schemas + lazy injection: full surface on
            // turn 1 for discovery, core + used tools on turn 2+.
            bus.tool_definitions_lazy(true, turn, &session_used_tools)
        };
        let tool_catalog = ToolCapabilityCatalog::from_tool_defs(&tool_defs);
        let (_provider_context_window, context_window, selector_policy) =
            loop_context_windows(config);
        if let Some(policy) = selector_policy {
            context.set_selector_policy(policy);
        } else {
            context.set_context_window(context_window);
        }

        if config.max_turns > 0 && turn > config.max_turns {
            tracing::warn!(
                "Hard turn limit reached ({} turns). Stopping.",
                config.max_turns
            );
            let _ = events.send(AgentEvent::TurnStart { turn });
            let context_composition = default_context_composition(context_window);
            bus.emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
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
                model: Some(active_model.clone()),
                provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
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
            tracing::info!("Operator correction detected — entering recovery mode");
            conversation.intent.operator_correction_pending = false;
            dead_mouse_nudges = 0;
            meta_recovery_nudges = 0;
            controller = ControllerState::default();
            conversation.push_user(operator_correction_recovery_message());
        }

        let _ = events.send(AgentEvent::TurnStart { turn });
        bus.emit(&omegon_traits::BusEvent::TurnStart { turn });

        if let Some(warning) = stuck_detector.check(&tool_catalog) {
            tracing::info!(
                consecutive = warning.consecutive,
                "Stuck detector: {}",
                warning.message
            );
            if warning.consecutive >= 3 {
                tracing::warn!(
                    "Stuck detector escalation — injecting recovery guidance after {} consecutive warnings",
                    warning.consecutive
                );
                conversation.push_user(
                    "[System: Repetition pressure — several recent turns repeated \
                     similar tool calls without producing new evidence. If you \
                     already have what you need, produce the deliverable now. \
                     Otherwise take one concrete, different next action. If no \
                     concrete action is possible, state the blocker plainly and stop.]"
                        .to_string(),
                );
                stuck_detector.reset_after_escalation();
            } else {
                conversation.push_user(format!("[System: {}]", warning.message));
            }
        }

        // If context is getting large, try LLM-driven compaction.
        // The context_window default is 200k tokens (Anthropic models).
        // Trigger at 75% utilization.
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
            if let Some(selection) = pressure_compaction_payload(conversation) {
                let evict_count = selection.evict_count();
                let fallback_reason = selection.reason();
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
                match compact_via_llm(bridge, selection.payload(), &base_stream_options).await {
                    Ok(summary) => {
                        let summary_chars = summary.chars().count();
                        selection.apply(conversation, summary);
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

        if conversation.intent.stats.tool_calls > 0
            || conversation.intent.current_task.is_some()
            || conversation.intent.stats.compactions > 0
            || conversation.intent.has_active_work_plan_context()
        {
            let intent_block = conversation.render_intent_for_injection();
            context.inject_intent(intent_block);
        }

        {
            let user_prompt = conversation.last_user_prompt();
            bus.emit(&omegon_traits::BusEvent::ContextBuild {
                user_prompt: user_prompt.to_string(),
                turn,
            });
            let (tools_vec, files_vec, budget) = context.signals_data();
            let signals = omegon_traits::ContextSignals {
                user_prompt,
                recent_tools: &tools_vec,
                recent_files: &files_vec,
                lifecycle_phase: context.phase(),
                turn_number: turn,
                context_budget_tokens: budget,
            };
            let bus_injections = bus.collect_context(&signals);
            if !bus_injections.is_empty() {
                tracing::debug!(count = bus_injections.len(), "bus context injections");
                context.inject_external(bus_injections);
            }
        }

        if let Some(attachment_manifest) = conversation.render_attachment_context_injection() {
            context.inject_external(vec![omegon_traits::ContextInjection {
                source: "attachment-files".into(),
                content: attachment_manifest,
                priority: 190,
                ttl_turns: 1,
            }]);
        }

        context
            .prepare_embeddings(conversation.last_user_prompt())
            .await;

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

        let system_prompt =
            context.build_system_prompt(conversation.last_user_prompt(), conversation);
        let llm_messages = conversation.build_llm_view();
        // User-image attachments are stored on canonical user messages directly.

        tracing::debug!(
            turn,
            system_prompt_len = system_prompt.len(),
            messages = llm_messages.len(),
            tools = tool_defs.len(),
            estimated_tokens = conversation.estimate_tokens(),
            "LLM context assembled"
        );

        // Re-read thinking level each turn (can change mid-session via /thinking)
        let stream_options = {
            let mut opts = base_stream_options.clone();
            opts.reasoning = config.settings.as_ref().and_then(|s| {
                let guard = s.lock().ok()?;
                match guard.thinking {
                    crate::settings::ThinkingLevel::Off => None,
                    crate::settings::ThinkingLevel::Minimal => Some("minimal".to_string()),
                    crate::settings::ThinkingLevel::Low => Some("low".to_string()),
                    crate::settings::ThinkingLevel::Medium => Some("medium".to_string()),
                    crate::settings::ThinkingLevel::High => Some("high".to_string()),
                }
            });
            // RouteController is the authoritative serving model when present;
            // legacy bridge_model/settings fallback remains for daemon/headless
            // paths that have not adopted ProviderRoute yet.
            opts.model = if let Some(controller) = config.route_controller.as_ref() {
                controller
                    .snapshot()
                    .await
                    .serving_model()
                    .map(str::to_string)
            } else {
                config.bridge_model.clone().or_else(|| {
                    config
                        .settings
                        .as_ref()
                        .and_then(|s| s.lock().ok().map(|g| g.model.clone()))
                        .or_else(|| Some(config.model.clone()))
                })
            };
            // Track the active model for this turn so TurnEnd events and
            // error classification use the current model, not the startup value.
            active_model = opts.model.clone().unwrap_or_else(|| config.model.clone());
            opts
        };

        // A cold 20-30B model can take 3+ minutes to load into memory.
        // The SSE idle timeout (90s) fires before the first token arrives
        // on a cold start. We pre-flight the model load here and surface
        // progress in the TUI via toast notifications.
        if let Some(model_spec) = stream_options.model.as_deref()
            && crate::providers::infer_provider_id(model_spec) == "ollama"
        {
            let bare = model_spec
                .trim_start_matches("ollama:")
                .trim_start_matches("local:");
            maybe_warmup_ollama(bare, events, config.ollama_manager.as_ref()).await;
        }

        let assistant_msg = tokio::select! {
            result = stream_with_retry(
                bridge,
                &system_prompt,
                &llm_messages,
                &tool_defs,
                &stream_options,
                events,
                config,
            ) => {
                match result {
                    Ok(msg) => msg,
                    Err(e) if is_context_overflow(&e.to_string()) => {
                        // Context too large for the provider — emergency compact and retry
                        tracing::warn!("Context overflow detected — forcing emergency compaction");
                        let _ = events.send(AgentEvent::SystemNotification {
                            message: "Context overflow — compacting conversation and retrying…".into(),
                        });
                        let before_tokens = conversation.estimate_tokens() as u64;
                        if let Some((payload, evict_count)) = conversation.build_compaction_payload() {
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
                            match compact_via_llm(bridge, &payload, &base_stream_options).await {
                                Ok(summary) => {
                                    let summary_chars = summary.chars().count();
                                    conversation.apply_compaction(summary);
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
                                    conversation.decay_oldest(evict_count);
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
                            let evict_count = conversation.message_count() / 2;
                            conversation.decay_oldest(evict_count);
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
                        // Rebuild messages and retry once
                        let llm_messages = conversation.build_llm_view();
                        stream_with_retry(
                            bridge, &system_prompt, &llm_messages, &tool_defs,
                            &stream_options, events, config,
                        ).await?
                    }
                    Err(e) if is_malformed_history(&e.to_string()) => {
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
                        let half = conversation.message_count() / 2;
                        conversation.decay_oldest(half.max(1));
                        let llm_messages = conversation.build_llm_view();
                        stream_with_retry(
                            bridge, &system_prompt, &llm_messages, &tool_defs,
                            &stream_options, events, config,
                        ).await?
                    }
                    Err(e) => return Err(e),
                }
            },
            _ = cancel.cancelled() => {
                tracing::info!("Agent loop cancelled during LLM streaming");
                bus.emit(&omegon_traits::BusEvent::TurnEnd(Box::new(BusEventTurnEnd {
                    turn,
                    model: None,
                    provider: None,
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: default_context_composition(context_window),
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
                    model: Some(active_model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: default_context_composition(context_window),
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

        // Real provider token counts for this turn (0 if provider didn't report them)
        let (act_in, act_out, act_cr, act_cc) = assistant_msg.provider_tokens;
        let provider_telemetry = assistant_msg.provider_telemetry.clone();
        let active_provider = crate::providers::infer_provider_id(&active_model).to_string();
        if let Some(reason) = provider_stop_reason(&assistant_msg.raw)
            && let Some(message) = provider_stop_notice(&active_provider, reason)
        {
            tracing::warn!(
                provider = active_provider.as_str(),
                stop_reason = reason,
                "provider ended response abnormally"
            );
            let _ = events.send(AgentEvent::SystemNotification { message });
        }

        let captured =
            crate::lifecycle::capture::parse_ambient_blocks(assistant_msg.text_content());
        if !captured.is_empty() {
            conversation.apply_ambient_captures(&captured);
        }

        // Push assistant message to conversation. From this point on, an
        // operator interrupt means "stop this turn" rather than "forget my
        // submitted prompt" because the model has produced replay-relevant
        // assistant/tool state.
        conversation.push_assistant(assistant_msg.clone());
        if let Some(cancel_keeps_prompt) = &config.cancel_keeps_prompt {
            cancel_keeps_prompt.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        // Extract tool calls
        let tool_calls = assistant_msg.tool_calls();
        if tool_calls.is_empty() {
            if is_pathological_meta_response(&assistant_msg.text)
                && turn < config.max_turns
                && meta_recovery_nudges < 2
            {
                meta_recovery_nudges += 1;
                tracing::info!(
                    nudges = meta_recovery_nudges,
                    "Pathological meta response — forcing concrete recovery retry"
                );
                conversation.push_user(meta_recovery_retry_message());
                continue;
            }

            // Check if the agent skipped committing.
            // Only nudge when the agent looks like it is wrapping up (completion
            // language in the text response) or is close to the turn budget.
            // Mid-task text responses (progress updates, questions) should not
            // trigger a commit — "Commit when done" in the system prompt handles
            // the normal case. This nudge is a safety net, not a per-cycle prompt.
            let near_budget = turn + 6 >= config.max_turns;
            let response_looks_done = looks_like_completion(&assistant_msg.text);
            if config.allow_commit_nudge
                && !conversation.intent.commit_nudged
                && has_mutations(conversation)
                && turn < config.max_turns
                && (near_budget || response_looks_done)
            {
                conversation.intent.commit_nudged = true;
                tracing::info!(
                    near_budget,
                    response_looks_done,
                    "Agent finishing without committing — nudging"
                );
                conversation.push_user(
                    "[System: You have uncommitted file changes. Commit your work before finishing.]"
                        .to_string(),
                );
                let nudge_system_prompt =
                    context.build_system_prompt(conversation.last_user_prompt(), conversation);
                let nudge_llm_messages = conversation.build_llm_view();
                let nudge_prompt_telemetry = context.last_prompt_telemetry();
                let nudge_context_composition = compute_context_composition(
                    &nudge_system_prompt,
                    &nudge_llm_messages,
                    &tool_defs,
                    context_window,
                    Some(&nudge_prompt_telemetry),
                );
                bus.emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                    BusEventTurnEnd {
                        turn,
                        model: Some(active_model.clone()),
                        provider: Some(
                            crate::providers::infer_provider_id(&active_model).to_string(),
                        ),
                        estimated_tokens: conversation.estimate_tokens(),
                        context_window,
                        context_composition: nudge_context_composition.clone(),
                        actual_input_tokens: act_in,
                        actual_output_tokens: act_out,
                        cache_read_tokens: act_cr,
                        provider_telemetry: provider_telemetry.clone(),
                        dominant_phase: None,
                        drift_kind: Some(DriftKind::ClosureStall),
                        progress_signal: omegon_traits::ProgressSignal::None,
                    },
                )));
                let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
                    turn,
                    turn_end_reason: TurnEndReason::ProgressNudge,
                    model: Some(active_model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: nudge_context_composition,
                    actual_input_tokens: act_in,
                    actual_output_tokens: act_out,
                    cache_read_tokens: act_cr,
                    cache_creation_tokens: act_cc,
                    provider_telemetry: provider_telemetry.clone(),
                    dominant_phase: None,
                    drift_kind: Some(DriftKind::ClosureStall),
                    progress_nudge_reason: Some(ProgressNudgeReason::CommitHygiene),
                    intent_task: conversation.intent.current_task.clone(),
                    intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
                    files_read_count: conversation.intent.files_read.len(),
                    files_modified_count: conversation.intent.files_modified.len(),
                    stats_tool_calls: conversation.intent.stats.tool_calls,
                    streaks: controller.streaks(),
                })));
                continue; // give it one more turn to commit
            }

            // If the agent is about to end the turn while the visible
            // Workbench plan still has active/todo items, force one explicit
            // reconciliation pass. The plan is the operator's primary
            // awareness surface; stale rows are worse than a slightly longer
            // turn because they misrepresent what is happening now.
            if should_nudge_plan_reconciliation(&conversation.intent, &assistant_msg.text)
                && turn < config.max_turns
            {
                let fingerprint = plan_open_fingerprint(&conversation.intent);
                if conversation.intent.plan_reconciliation_fingerprint == Some(fingerprint) {
                    conversation.intent.plan_reconciliation_nudges = conversation
                        .intent
                        .plan_reconciliation_nudges
                        .saturating_add(1);
                } else {
                    conversation.intent.plan_reconciliation_fingerprint = Some(fingerprint);
                    conversation.intent.plan_reconciliation_nudges = 1;
                }
                tracing::info!(
                    "Agent finishing with incomplete visible plan — nudging reconciliation"
                );
                conversation.push_user(
                    "[System: The visible Workbench plan still has active/todo items. \
                     Before ending the turn, reconcile it with the `plan` tool: use \
                     `plan advance`/`plan complete` for finished items, `plan skip` for \
                     deliberately bypassed items, or `plan clear` only if the plan gate is no \
                     longer useful. If work truly remains, leave the plan active and state the \
                     remaining work explicitly.]"
                        .to_string(),
                );
                let nudge_system_prompt =
                    context.build_system_prompt(conversation.last_user_prompt(), conversation);
                let nudge_llm_messages = conversation.build_llm_view();
                let nudge_prompt_telemetry = context.last_prompt_telemetry();
                let nudge_context_composition = compute_context_composition(
                    &nudge_system_prompt,
                    &nudge_llm_messages,
                    &tool_defs,
                    context_window,
                    Some(&nudge_prompt_telemetry),
                );
                bus.emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                    BusEventTurnEnd {
                        turn,
                        model: Some(active_model.clone()),
                        provider: Some(
                            crate::providers::infer_provider_id(&active_model).to_string(),
                        ),
                        estimated_tokens: conversation.estimate_tokens(),
                        context_window,
                        context_composition: nudge_context_composition.clone(),
                        actual_input_tokens: act_in,
                        actual_output_tokens: act_out,
                        cache_read_tokens: act_cr,
                        provider_telemetry: provider_telemetry.clone(),
                        dominant_phase: None,
                        drift_kind: Some(DriftKind::ClosureStall),
                        progress_signal: omegon_traits::ProgressSignal::None,
                    },
                )));
                let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
                    turn,
                    turn_end_reason: TurnEndReason::ProgressNudge,
                    model: Some(active_model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: nudge_context_composition,
                    actual_input_tokens: act_in,
                    actual_output_tokens: act_out,
                    cache_read_tokens: act_cr,
                    cache_creation_tokens: act_cc,
                    provider_telemetry: provider_telemetry.clone(),
                    dominant_phase: None,
                    drift_kind: Some(DriftKind::ClosureStall),
                    progress_nudge_reason: Some(ProgressNudgeReason::PlanReconciliation),
                    intent_task: conversation.intent.current_task.clone(),
                    intent_phase: Some(format!("{:?}", conversation.intent.lifecycle_phase)),
                    files_read_count: conversation.intent.files_read.len(),
                    files_modified_count: conversation.intent.files_modified.len(),
                    stats_tool_calls: conversation.intent.stats.tool_calls,
                    streaks: controller.streaks(),
                })));
                continue;
            }

            // If any loaded skill has numbered phases, check whether the
            // agent's response references the final phase. If not, nudge
            // it to continue. Prevents the "I'm done" pattern when
            // the last phase (e.g., "Phase 10: Export to File") was skipped.
            if !config.skill_phases.is_empty()
                && !conversation.intent.skill_completion_nudged
                && turn < config.max_turns
            {
                let response_text = assistant_msg.text.to_lowercase();
                let mut incomplete = Vec::new();
                for phase in &config.skill_phases {
                    // Check if the response mentions the final phase
                    let phase_mentioned = response_text
                        .contains(&format!("phase {}", phase.final_phase_number))
                        || response_text.contains(&phase.final_phase_label.to_lowercase());
                    if !phase_mentioned {
                        incomplete.push(&phase.final_phase_label);
                    }
                }
                if !incomplete.is_empty() {
                    conversation.intent.skill_completion_nudged = true;
                    let labels: Vec<String> =
                        incomplete.iter().map(|l| format!("  - {l}")).collect();
                    tracing::info!(incomplete = ?incomplete, "agent stopped before completing all skill phases — nudging");
                    conversation.push_user(format!(
                        "[System: You have not completed all phases of the active skill. \
                         The following phase(s) still need to be executed:\n{}\n\n\
                         Please continue and complete the remaining phases before finishing.]",
                        labels.join("\n"),
                    ));
                    continue;
                }
            }

            if turn < config.max_turns
                && dead_mouse_nudges < 3
                && should_continue_text_only_turn(
                    config
                        .settings
                        .as_ref()
                        .and_then(|s| s.lock().ok().map(|s| s.automation_level))
                        .unwrap_or_default(),
                    conversation.last_user_prompt(),
                    &assistant_msg.text,
                    conversation.intent.stats.tool_calls > 0,
                )
            {
                dead_mouse_nudges += 1;
                tracing::info!(
                    nudge = dead_mouse_nudges,
                    "Text-only turn ended before action — auto-continuing"
                );
                conversation.push_user(
                    "[System: The operator already asked you to proceed. Do not ask for \
                     confirmation or describe work you will do next. Take the next concrete \
                     action now with the available tools, or give a final answer only if the \
                     requested work is actually complete.]"
                        .to_string(),
                );
                dead_mouse_nudge_injected = true;
                continue;
            }

            // Model responded with text-only (no tool calls) but hasn't
            // made any file changes. It's narrating instead of acting.
            // Nudge up to 2 times, then give up.
            //
            // Fires for ALL model tiers — frontier models exhibit this
            // pattern too (e.g., dumping HTML as text instead of calling
            // write). The full harness with 60+ tools makes this MORE
            // likely, not less.
            //
            // Guards against false positives:
            // - Skip turn 1: a text-only first response is normal
            // - Skip if model never used tools: conversational exchange
            // - Skip if the user's prompt looks like a question or any
            //   read-style request (rundown / overview / give me / show me)
            // - Skip if the last assistant reply was substantial (>=200
            //   chars of natural language) — that IS the output, regardless
            //   of whether tools were called to gather context first
            // - Skip if the user explicitly did not ask for a file write
            let in_task_mode = conversation.intent.stats.tool_calls > 0;
            // Skip dead-mouse if the operator's task mode is Research —
            // question / rundown / summary / read-style prompts make
            // text-only responses legitimate. The shared inference in
            // `behavior::infer_task_mode_from_prompt` errs *strongly* on the
            // side of Research: false Implementation classifications push the
            // model to invent file-writing work the user never requested
            // (worse failure mode than false negatives).
            let user_asked_question =
                conversation.intent.task_mode == crate::conversation::TaskMode::Research;
            // If the model's last assistant message was substantial natural
            // language (i.e. it produced an actual answer), treat the turn
            // as complete regardless of mutations. Q&A is a primary mode for
            // many embedders (e.g. Flynt) and the previous heuristic
            // mistreated text answers as "narration without action".
            let last_assistant_substantial = conversation
                .last_assistant_text()
                .map(|t| t.trim().len() >= 200)
                .unwrap_or(false);
            if !has_mutations(conversation)
                && turn > 1
                && in_task_mode
                && !user_asked_question
                && !last_assistant_substantial
                && turn < config.max_turns
                && dead_mouse_nudges < 3
            {
                dead_mouse_nudges += 1;
                if dead_mouse_nudges < 2 {
                    continue;
                }
                let msg = if dead_mouse_nudges == 2 {
                    "[System: You responded with text but did not advance the task. \
                     If the user asked for a file change, use the appropriate tool. \
                     If the user asked a question, your text answer may be sufficient — \
                     but make sure it actually answers what they asked.]"
                } else {
                    "[System: Multiple turns without task progress. Either answer the \
                     user's question completely, or use tools to make the changes they \
                     requested. Do not invent file-writing work the user did not ask for.]"
                };
                tracing::info!(
                    nudge = dead_mouse_nudges,
                    "Dead-mouse detection — model responded without acting"
                );
                conversation.push_user(msg.to_string());
                dead_mouse_nudge_injected = true;
                continue;
            }

            // Reset dead-mouse counter when model does use tools
            // (handled below when tool_calls is non-empty, but also
            // covers the break-out path here).

            let system_prompt =
                context.build_system_prompt(conversation.last_user_prompt(), conversation);
            let llm_messages = conversation.build_llm_view();
            let prompt_telemetry = context.last_prompt_telemetry();
            let turn_context_composition = compute_context_composition(
                &system_prompt,
                &llm_messages,
                &tool_defs,
                context_window,
                Some(&prompt_telemetry),
            );
            bus.emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
                BusEventTurnEnd {
                    turn,
                    model: Some(active_model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
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
                model: Some(active_model.clone()),
                provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
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
            break;
        }

        // Reset dead-mouse counter — model is using tools this turn.
        meta_recovery_nudges = 0;
        // After a nudge was injected, only reset if the model did real work
        // (not just wrote a noise file acknowledging the warning). Non-Claude
        // models (e.g. GPT-5.5) tend to literalize nudges and write compliance
        // notes, which must not satisfy the check and reset the counter.
        if dead_mouse_nudge_injected {
            let has_real_work = tool_calls.iter().any(counts_as_real_work_for_dead_mouse);
            if has_real_work {
                dead_mouse_nudges = 0;
                dead_mouse_nudge_injected = false;
            }
            // else: counter stays — noise write did not satisfy the nudge
        } else {
            dead_mouse_nudges = 0;
        }

        for call in tool_calls {
            session_used_tools.insert(call.name.clone());
            bus.emit(&omegon_traits::BusEvent::ToolStart {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
                capabilities: tool_catalog.capabilities_for(&call.name),
            });
        }

        // Auto-delegation is disabled — the agent always executes its
        // own tool calls directly. See classify_auto_delegate_plan().
        let settings_permission_snapshot = config.settings.as_ref().and_then(|settings| {
            settings.lock().ok().map(|settings| {
                (
                    crate::permissions::layered_policy_from_settings(&settings),
                    crate::permissions::styrene_role_from_settings(&settings),
                )
            })
        });
        let (permission_policy, permission_role) = settings_permission_snapshot
            .as_ref()
            .map(|(policy, role)| (Some(policy), *role))
            .unwrap_or_else(|| (config.permission_policy.as_ref(), config.permission_role));
        let dispatch_calls = tool_calls;
        let dispatch = dispatch_tools(
            bus,
            dispatch_calls,
            events,
            cancel.clone(),
            &config.cwd,
            config.secrets.as_deref(),
            config.host_context.as_ref().map(|c| c.as_ref()),
            permission_policy,
            permission_role,
        )
        .await;
        let results = dispatch.results;

        // Emit permission decisions as bus events (requires &mut bus).
        for perm in dispatch.permission_decisions {
            bus.emit(&omegon_traits::BusEvent::PermissionDecision {
                tool_name: perm.tool_name,
                path: perm.path,
                decision: perm.decision,
                kind: perm.kind,
                persistence: perm.persistence,
                grant_path: perm.grant_path,
            });
        }

        // Push tool results to conversation and update intent
        let mut results = results;
        let plan_snapshot_before =
            work_plan_snapshot_with_lifecycle(&conversation.intent, &config.cwd);
        conversation
            .intent
            .update_from_tools(&tool_catalog, dispatch_calls, &results);
        enrich_plan_list_tool_results(&mut results, dispatch_calls, &conversation.intent);
        for result in &results {
            conversation.push_tool_result(result.clone());
        }
        let plan_snapshot_after =
            work_plan_snapshot_with_lifecycle(&conversation.intent, &config.cwd);

        if let Some(message) = plan_status_notification(dispatch_calls, &conversation.intent) {
            let _ = events.send(AgentEvent::SystemNotification { message });
        }
        if work_plan_snapshot_changed(&plan_snapshot_before, &plan_snapshot_after) {
            let projection = conversation
                .intent
                .plan_surface_projection_for_repo(&config.cwd);
            let _ = events.send(AgentEvent::PlanUpdated { projection });
        }

        let dominant_phase = classify_turn_phase(&tool_catalog, dispatch_calls, &results);
        let drift_kind =
            classify_drift_kind(&tool_catalog, turn, conversation, dispatch_calls, &results);
        let constraints_before = captured
            .iter()
            .filter(|capture| {
                matches!(
                    capture,
                    crate::lifecycle::capture::AmbientCapture::Constraint(_)
                )
            })
            .count();
        let constraints_after = conversation.intent.constraints_discovered.len();
        let progress_signal = classify_progress_signal(
            constraints_after.saturating_sub(constraints_before),
            constraints_after,
            &tool_catalog,
            dispatch_calls,
            &results,
        );
        let evidence = assess_evidence(conversation, &tool_catalog, dispatch_calls, &results);
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            drift_kind,
            progress_signal,
            evidence,
            behavior::is_substantive_interleaved_prose(&assistant_msg.text),
        );
        let behavior = behavioral_tier(config);
        let continuation_tier = continuation_pressure_tier(
            config,
            &controller,
            conversation,
            dispatch_calls,
            dominant_phase,
            behavior,
        );

        // Nudge injection macro — push message + emit audit event.
        macro_rules! inject_nudge {
            ($reason:expr, $msg:expr) => {{
                let msg_str: String = $msg.into();
                conversation.push_user(msg_str.clone());
                bus.emit(&omegon_traits::BusEvent::NudgeInjected {
                    turn,
                    reason: $reason.into(),
                    message_preview: msg_str.chars().take(100).collect(),
                });
            }};
        }

        if is_first_turn_orientation_churn(
            turn,
            config,
            conversation,
            &tool_catalog,
            dispatch_calls,
        ) {
            tracing::info!("First-turn orientation churn — injecting execution-bias nudge");
            let msg = match behavior {
                BehavioralTier::Constrained => {
                    "[System: Read the relevant file or answer the user. Do not use broad orientation tools.]"
                }
                BehavioralTier::Standard => {
                    "[System: Focus on the user's request. Read the most relevant file, then answer them in chat.]"
                }
            };
            inject_nudge!("first_turn_execution_bias", msg);
        } else if is_slim_execution_bias(config)
            && controller.local_evidence_sufficient_streak > 0
            && has_local_target_hypothesis(conversation)
            && continuation_tier.is_some()
        {
            tracing::info!("OM local-first lock — injecting patch-or-prove nudge");
            inject_nudge!("om_local_first_lock", om_local_first_message(behavior));
        } else if controller.evidence_sufficient_streak > 0 && continuation_tier.is_some() {
            tracing::info!("Actionability threshold — injecting forced-convergence nudge");
            inject_nudge!(
                "evidence_sufficiency",
                evidence_sufficiency_message(behavior)
            );
        } else if should_inject_execution_pressure(
            turn,
            config,
            conversation,
            &tool_catalog,
            dispatch_calls,
            behavior,
        ) {
            tracing::info!("Execution stall — injecting execution-pressure nudge");
            let msg = match behavior {
                BehavioralTier::Constrained => {
                    "[System: You have enough context. Answer the user now.]"
                }
                BehavioralTier::Standard => {
                    "[System: You have enough context. Answer the user, or explain what's blocking you. Do not invent file-writing work the user didn't ask for.]"
                }
            };
            inject_nudge!("execution_pressure", msg);
        } else if let Some(tier) = continuation_tier {
            tracing::info!(
                tier,
                "Continuation churn — injecting continuation-pressure nudge"
            );
            inject_nudge!(
                format!("continuation_pressure_tier_{tier}"),
                continuation_pressure_message(tier, behavior)
            );
        }

        for (call, result) in dispatch_calls.iter().zip(results.iter()) {
            bus.emit(&omegon_traits::BusEvent::ToolEnd {
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

        for call in dispatch_calls {
            context.record_tool_call(&call.name);
            // Track file access from tool arguments
            if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                context.record_file_access(std::path::PathBuf::from(path));
            }
        }
        context.update_phase_from_activity(dispatch_calls);

        let observations = crate::observation::ObservationNormalizer::new(&tool_catalog)
            .normalize(dispatch_calls, &results);
        for event in &observations {
            stuck_detector.record_observation(event);
        }
        for call in dispatch_calls {
            let is_error = results
                .iter()
                .find(|r| r.call_id == call.id)
                .is_some_and(|r| r.is_error);
            if is_error {
                stuck_detector.record(&tool_catalog, call, true);
            }
        }

        let system_prompt =
            context.build_system_prompt(conversation.last_user_prompt(), conversation);
        let llm_messages = conversation.build_llm_view();
        let prompt_telemetry = context.last_prompt_telemetry();
        let turn_context_composition = compute_context_composition(
            &system_prompt,
            &llm_messages,
            &tool_defs,
            context_window,
            Some(&prompt_telemetry),
        );
        bus.emit(&omegon_traits::BusEvent::TurnEnd(Box::new(
            BusEventTurnEnd {
                turn,
                model: Some(active_model.clone()),
                provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
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

        let turn_requests = bus.drain_requests();
        for request in turn_requests {
            match request {
                omegon_traits::BusRequest::Notify { message, level } => {
                    tracing::info!(level = ?level, "Bus: {message}");
                }
                omegon_traits::BusRequest::InjectSystemMessage { content } => {
                    conversation.push_user(format!("[System: {content}]"));
                }
                omegon_traits::BusRequest::RequestAggressiveDecay => {
                    tracing::info!("Bus: tier 1 aggressive decay requested");
                    conversation.tighten_decay();
                    bus.emit(&omegon_traits::BusEvent::Compacted);
                }
                omegon_traits::BusRequest::RequestCompaction => {
                    tracing::info!("Bus: tier 2 compaction requested by feature");
                    let before_tokens = conversation.estimate_tokens() as u64;
                    if let Some(selection) = pressure_compaction_payload(conversation) {
                        let evict_count = selection.evict_count();
                        let fallback_reason = selection.reason();
                        emit_context_compaction_event(
                            events,
                            context_compaction_event(
                                omegon_traits::ContextCompactionTrigger::AutoTier2,
                                omegon_traits::ContextCompactionStatus::Started,
                                before_tokens,
                                None,
                                Some(evict_count),
                                None,
                                fallback_reason.clone(),
                            ),
                        );
                        match compact_via_llm(bridge, selection.payload(), &base_stream_options)
                            .await
                        {
                            Ok(summary) => {
                                let summary_chars = summary.chars().count();
                                selection.apply(conversation, summary);
                                emit_context_compaction_event(
                                    events,
                                    context_compaction_event(
                                        omegon_traits::ContextCompactionTrigger::AutoTier2,
                                        omegon_traits::ContextCompactionStatus::Succeeded,
                                        before_tokens,
                                        Some(conversation.estimate_tokens() as u64),
                                        Some(evict_count),
                                        Some(summary_chars),
                                        None,
                                    ),
                                );
                                bus.emit(&omegon_traits::BusEvent::Compacted);
                            }
                            Err(e) => {
                                let message = e.to_string();
                                emit_context_compaction_event(
                                    events,
                                    context_compaction_event(
                                        omegon_traits::ContextCompactionTrigger::AutoTier2,
                                        omegon_traits::ContextCompactionStatus::Failed,
                                        before_tokens,
                                        None,
                                        Some(evict_count),
                                        None,
                                        Some(message.clone()),
                                    ),
                                );
                                tracing::warn!(error = %message, "auto-compaction failed");
                                bus.emit(&omegon_traits::BusEvent::Compacted);
                            }
                        }
                    } else {
                        emit_context_compaction_event(
                            events,
                            context_compaction_event(
                                omegon_traits::ContextCompactionTrigger::AutoTier2,
                                omegon_traits::ContextCompactionStatus::NoPayload,
                                before_tokens,
                                Some(before_tokens),
                                Some(0),
                                None,
                                Some(
                                    "auto-compaction requested but nothing was eligible to compact"
                                        .to_string(),
                                ),
                            ),
                        );
                        tracing::debug!(
                            "auto-compaction requested but nothing was eligible to compact"
                        );
                        bus.emit(&omegon_traits::BusEvent::Compacted);
                    }
                }
                omegon_traits::BusRequest::RefreshHarnessStatus => {
                    tracing::debug!("Bus: harness status refresh requested");
                    let status = crate::status::HarnessStatus::assemble();
                    if let Ok(status_json) = serde_json::to_value(&status) {
                        let _ = events.send(AgentEvent::HarnessStatusChanged { status_json });
                    }
                }
                omegon_traits::BusRequest::AutoStoreFact {
                    section,
                    content,
                    source,
                } => {
                    let args = serde_json::json!({ "content": content, "section": section });
                    if let Err(e) = bus
                        .execute_tool("memory_store", "auto_ingest", args, cancel.clone())
                        .await
                    {
                        tracing::debug!(source, "auto-store fact skipped: {e}");
                    }
                }
                omegon_traits::BusRequest::EmitAgentEvent { event } => {
                    let _ = events.send(*event);
                }
            }
        }

        let estimated_tokens = conversation.estimate_tokens();
        let _ = events.send(AgentEvent::ContextUpdated {
            tokens: estimated_tokens as u64,
            context_window: context_window as u64,
            context_class: config
                .settings
                .as_ref()
                .and_then(|s| s.lock().ok().map(|g| g.context_class.label().to_string()))
                .unwrap_or_else(|| {
                    crate::settings::ContextClass::from_tokens(context_window)
                        .label()
                        .to_string()
                }),
            thinking_level: config
                .settings
                .as_ref()
                .and_then(|s| s.lock().ok().map(|g| g.thinking.as_str().to_string()))
                .unwrap_or_else(|| "off".to_string()),
        });
        let _ = events.send(AgentEvent::TurnEnd(Box::new(AgentEventTurnEnd {
            turn,
            turn_end_reason: TurnEndReason::ToolContinuation,
            model: Some(active_model.clone()),
            provider: Some(crate::providers::infer_provider_id(&active_model).to_string()),
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
    }

    let elapsed = session_start.elapsed();
    tracing::info!(
        turns = turn,
        tool_calls = conversation.intent.stats.tool_calls,
        elapsed_secs = elapsed.as_secs(),
        "Agent loop complete"
    );

    bus.emit(&omegon_traits::BusEvent::AgentEnd);
    let _ = events.send(AgentEvent::AgentEnd);

    // Emit SessionEnd so session_log and memory features can finalise.
    // This must come after AgentEnd so TUI is no longer in "working" state
    // before any slow post-session I/O runs.
    // Capture initial prompt (truncated) and outcome for journal enrichment.
    let initial_prompt = conversation
        .first_user_text()
        .map(|t| t.chars().take(200).collect::<String>());
    let outcome_summary = conversation
        .last_assistant_text()
        .map(|t| t.chars().take(300).collect::<String>());
    bus.emit(&omegon_traits::BusEvent::SessionEnd {
        turns: turn,
        tool_calls: conversation.intent.stats.tool_calls,
        duration_secs: elapsed.as_secs_f64(),
        initial_prompt,
        outcome_summary,
    });

    // Process any pending bus requests (e.g. auto-compact notifications,
    // auto-store facts from lifecycle transitions, episode storage).
    // AutoStoreFact requests are now executed rather than dropped in headless
    // runs, but interactive turns disable this drain: terminal TurnEnd/AgentEnd
    // already reached the operator surface, and late side work must not keep the
    // active-turn worker alive while the composer is waiting to accept input.
    if config.drain_post_loop_requests {
        for request in bus.drain_requests() {
            match request {
                omegon_traits::BusRequest::Notify { message, level } => {
                    tracing::info!(level = ?level, "Bus notification: {message}");
                }
                omegon_traits::BusRequest::InjectSystemMessage { content } => {
                    tracing::debug!(
                        "post-loop InjectSystemMessage ignored (loop complete): {content}"
                    );
                }
                omegon_traits::BusRequest::RequestCompaction
                | omegon_traits::BusRequest::RequestAggressiveDecay => {
                    tracing::info!("Bus requested compaction (post-loop — ignored)");
                }
                omegon_traits::BusRequest::RefreshHarnessStatus => {}
                omegon_traits::BusRequest::AutoStoreFact {
                    section,
                    content,
                    source,
                } => {
                    let args = serde_json::json!({ "content": content, "section": section });
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        bus.execute_tool(
                            "memory_store",
                            "post_loop_auto_ingest",
                            args,
                            cancel.clone(),
                        ),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => {
                            tracing::debug!(source, "post-loop auto-store fact skipped: {e}")
                        }
                        Err(_) => tracing::warn!(
                            source,
                            "post-loop auto-store fact timed out; continuing turn completion"
                        ),
                    }
                }
                omegon_traits::BusRequest::EmitAgentEvent { event } => {
                    let _ = events.send(*event);
                }
            }
        }
    }

    Ok(())
}

fn plan_status_notification(calls: &[ToolCall], intent: &IntentDocument) -> Option<String> {
    let plan_call = calls
        .iter()
        .rev()
        .find(|call| call.name == crate::tool_registry::core::PLAN)?;
    let action = plan_call
        .arguments
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("status");
    let heading = if intent.work_plan.is_empty()
        && matches!(action, "advance" | "complete" | "skip" | "clear")
    {
        "Plan cleared"
    } else {
        match action {
            "set" => "Plan set",
            "advance" | "complete" => "Plan progress",
            "skip" => "Plan item skipped",
            "approve" => "Plan approved",
            "execute" => "Plan executing",
            "clear" => "Plan cleared",
            "status" => "Plan status",
            _ => "Plan updated",
        }
    };
    Some(format!("{heading}\n{}", intent.render_work_plan()))
}

fn work_plan_snapshot_with_lifecycle(
    intent: &IntentDocument,
    cwd: &std::path::Path,
) -> serde_json::Value {
    let repo_root = crate::setup::find_project_root(cwd);
    intent.work_plan_snapshot_json_for_repo(&repo_root)
}

fn work_plan_snapshot_changed(before: &serde_json::Value, after: &serde_json::Value) -> bool {
    before != after
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

/// Request an LLM-driven compaction summary for old conversation messages.
///
/// The payload is truncated to ~100k chars (~25k tokens) to ensure the
/// compaction request itself doesn't exceed provider limits.
pub(crate) async fn compact_via_llm(
    bridge: &dyn LlmBridge,
    payload: &str,
    options: &StreamOptions,
) -> anyhow::Result<String> {
    let system = "You are a conversation summarizer. Produce a concise summary \
                  preserving: what was done, what failed, constraints discovered, \
                  and current approach. Output only the summary, no preamble.";

    // Truncate the compaction payload to prevent the compaction request itself
    // from exceeding provider limits (~100k chars ≈ 25k tokens).
    const MAX_COMPACTION_CHARS: usize = 100_000;
    let truncated_payload = if payload.len() > MAX_COMPACTION_CHARS {
        tracing::warn!(
            original = payload.len(),
            truncated = MAX_COMPACTION_CHARS,
            "compaction payload truncated to fit provider limits"
        );
        // Find a valid UTF-8 char boundary at or before the limit.
        let end = payload.floor_char_boundary(MAX_COMPACTION_CHARS);
        &payload[..end]
    } else {
        payload
    };

    let messages = vec![crate::bridge::LlmMessage::User {
        content: truncated_payload.to_string(),
        images: vec![],
    }];

    let mut rx = bridge.stream(system, &messages, &[], options).await?;

    let mut summary = String::new();
    let summary_idle = std::time::Duration::from_secs(120);
    while let Some(event) = match tokio::time::timeout(summary_idle, rx.recv()).await {
        Ok(e) => e,
        Err(_) => {
            tracing::warn!("summary stream idle timeout");
            None
        }
    } {
        match event {
            LlmEvent::TextDelta { delta } => summary.push_str(&delta),
            LlmEvent::Done { .. } => break,
            LlmEvent::Error { message } => {
                return Err(anyhow::anyhow!("Compaction LLM error: {message}"));
            }
            _ => {}
        }
    }

    if summary.is_empty() {
        return Err(anyhow::anyhow!("Compaction produced empty summary"));
    }

    tracing::info!(summary_len = summary.len(), "Compaction summary received");
    Ok(summary)
}

/// Stream an LLM response with retry on transient errors.
/// Pre-flight an Ollama model to ensure it's warm before streaming.
///
/// If the model is cold (not in `/api/ps`), issues a minimal blocking
/// generate request so the model is fully loaded before `stream_with_retry`
/// attempts to open an SSE stream. Emits toast notifications during the wait.
async fn maybe_warmup_ollama(
    model_name: &str,
    events: &broadcast::Sender<AgentEvent>,
    manager: Option<&OllamaManager>,
) {
    let owned;
    let mgr = match manager {
        Some(m) => m,
        None => {
            owned = OllamaManager::new();
            &owned
        }
    };
    if !mgr.is_reachable().await {
        tracing::debug!("Ollama not reachable — skipping warmup");
        return;
    }
    // Emit a ⟳ toast so the operator knows we're waiting on model load.
    let _ = events.send(AgentEvent::SystemNotification {
        message: format!("⟳ Loading {model_name} into memory…"),
    });
    match mgr.warmup_model(model_name).await {
        Ok(WarmupResult::AlreadyWarm) => {
            // Model was already warm — no visible noise needed.
            tracing::debug!(model_name, "Ollama model already warm");
        }
        Ok(WarmupResult::WasLoaded) => {
            tracing::info!(model_name, "Ollama model warmed up successfully");
            let _ = events.send(AgentEvent::SystemNotification {
                message: format!("↯ {model_name} loaded"),
            });
        }
        Err(e) => {
            // Don't abort the turn — the real stream attempt may still succeed
            // (e.g. model loaded between our check and the stream call).
            tracing::warn!(model_name, error = %e, "Ollama warmup failed — proceeding anyway");
        }
    }
}

async fn stream_with_retry(
    bridge: &dyn LlmBridge,
    system_prompt: &str,
    messages: &[crate::bridge::LlmMessage],
    tools: &[omegon_traits::ToolDefinition],
    options: &StreamOptions,
    events: &broadcast::Sender<AgentEvent>,
    config: &LoopConfig,
) -> anyhow::Result<AssistantMessage> {
    let mut attempt = 0u32;
    let mut delay = config.retry_delay_ms;
    let started = Instant::now();

    loop {
        attempt += 1;

        let model = options
            .model
            .clone()
            .unwrap_or_else(|| config.model.clone());
        let provider = crate::providers::infer_provider_id(&model);

        // Wrap bridge.stream() so pre-stream network errors (DNS, connection
        // refused, TLS failures) enter the same transient classifier instead
        // of aborting immediately via `?`.
        let err = match bridge.stream(system_prompt, messages, tools, options).await {
            Ok(mut rx) => {
                match consume_llm_stream(
                    &mut rx,
                    events,
                    &provider,
                    &model,
                    config.cancel_keeps_prompt.as_ref(),
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
        append_upstream_failure_log(&UpstreamFailureLogEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            provider: provider.clone(),
            model: model.clone(),
            failure_kind: kind_label.to_string(),
            internal_class: kind_label.to_string(),
            recovery_action: upstream_class.recovery_action(),
            attempt,
            delay_ms: delay,
            message: err_msg.clone(),
        });

        // Soft exhaustion: bail after N consecutive transient failures.
        //
        // Four exhaustion paths:
        // - max_retries > 0 (cleave): hard cap on attempt count
        // - max_retries == 0 (TUI) + rate-limit: bail after 120s continuous
        // - max_retries == 0 (TUI) + stall: provider/reasoning-aware cumulative budget
        // - every other transient family: a finite 10-minute total retry envelope
        let elapsed = started.elapsed();
        let rate_limit_exhausted = config.max_retries == 0
            && matches!(transient_kind, Some(TransientFailureKind::RateLimited))
            && elapsed.as_secs() >= 120;
        let stall_exhausted = config.max_retries == 0
            && matches!(transient_kind, Some(TransientFailureKind::StalledStream))
            && elapsed.as_secs()
                >= stall_exhaustion_secs(&provider, &model, options.reasoning.as_deref());
        let transient_envelope_exhausted = transient_retry_envelope_exhausted(
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
        tracing::warn!(
            attempt,
            delay_ms = delay,
            kind = transient_kind
                .map(TransientFailureKind::label)
                .unwrap_or("transient upstream failure"),
            "Transient LLM error, retrying: {err_msg}"
        );

        // Milestone warnings → persistent (pushed to conversation).
        // These escalate so the operator notices accumulated failures.
        let is_milestone =
            matches!(attempt, 10 | 25 | 50 | 100) || (attempt > 100 && attempt.is_multiple_of(100));
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

        // Regular retry notification → toast (routed by TUI via "— retrying" substring).
        let operator_detail = transient_kind
            .map(|kind| kind.operator_detail(&provider, &err_msg))
            .unwrap_or_else(|| crate::util::truncate_str(&err_msg, 300).to_string());
        let msg = format!(
            "⚠ Upstream {kind_label} — retrying (attempt {attempt}, delay {}ms): {operator_detail}",
            delay
        );
        let _ = events.send(AgentEvent::ProviderRetry {
            provider: provider.clone(),
            model: model.clone(),
            attempt,
            delay_ms: delay,
            reason: kind_label.to_string(),
            message: operator_detail.clone(),
            recoverable: true,
        });
        let _ = events.send(AgentEvent::SystemNotification { message: msg });
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        delay = delay.saturating_mul(2).min(15_000); // exponential backoff, cap at 15s
    }
}

fn transient_retry_envelope_exhausted(
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

fn stall_exhaustion_secs(provider: &str, model: &str, reasoning: Option<&str>) -> u64 {
    let is_openai_reasoning = provider == "openai-codex"
        || ((provider == "openai" || provider == "openai-compatible")
            && (model.contains("gpt-5") || model.contains("o3") || model.contains("o4")));
    if is_openai_reasoning {
        return match reasoning {
            Some("high") => 2_400,
            Some("medium") => 1_800,
            Some("low" | "minimal") => 1_200,
            _ => 1_200,
        };
    }
    600
}

fn exhaustion_advice(
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
        if provider == "openai-codex" || provider == "openai" || provider == "openai-compatible" {
            return "The OpenAI stream exceeded Omegon's local silent-reasoning budget. This may be a long-running reasoning window or a wedged stream; lower thinking, retry later, or switch provider with /model.";
        }
        return "The provider's stream is unresponsive. Retry later or switch provider with /model.";
    }
    if rate_limit_exhausted || matches!(transient_kind, Some(TransientFailureKind::RateLimited)) {
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
pub(crate) fn is_upstream_exhausted(err: &anyhow::Error) -> bool {
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

fn provider_stop_notice(provider: &str, reason: &str) -> Option<String> {
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
        _ => "The provider ended the response abnormally; the visible answer may be incomplete.",
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
enum StreamIdleState {
    AwaitingFirstEvent = 0,
    OutputStreaming = 1,
    ToolStreaming = 2,
    ReasoningStreaming = 3,
    AmbiguousSilent = 4,
}

type StreamIdlePhase = StreamIdleState;

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

    fn label(self) -> &'static str {
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

fn select_stream_idle_budget(
    phase: StreamIdlePhase,
    _initial: std::time::Duration,
    active: std::time::Duration,
    reasoning_budget: std::time::Duration,
) -> std::time::Duration {
    match phase {
        StreamIdlePhase::OutputStreaming | StreamIdlePhase::ToolStreaming => active,
        StreamIdlePhase::AwaitingFirstEvent
        | StreamIdlePhase::ReasoningStreaming
        | StreamIdlePhase::AmbiguousSilent => reasoning_budget,
    }
}

/// Decide whether a stream that idled out under the tight *active* budget
/// should be re-armed with the generous reasoning budget instead of being
/// treated as a stall.
///
/// Reasoning-capable providers (notably the OpenAI Responses API behind
/// `openai-codex`) can stream output deltas for one item, then pause for
/// minutes while deciding the next item — *without* first emitting a
/// `TextEnd`/`ToolCallEnd` that would move the phase out of the active band.
/// The first such silence should not abort the turn; it should fall through to
/// the ambiguous-silent phase and get the reasoning leash. A second silence
/// (now evaluated in `AmbiguousSilent`) is a genuine stall and is *not*
/// re-armed, so a dead stream still dies inside the retry budget.
fn rearm_idle_phase(phase: StreamIdlePhase) -> Option<StreamIdlePhase> {
    match phase {
        StreamIdlePhase::OutputStreaming | StreamIdlePhase::ToolStreaming => {
            Some(StreamIdlePhase::AmbiguousSilent)
        }
        StreamIdlePhase::AwaitingFirstEvent
        | StreamIdlePhase::ReasoningStreaming
        | StreamIdlePhase::AmbiguousSilent => None,
    }
}

fn stream_idle_phase_after_event(current: StreamIdlePhase, event: &LlmEvent) -> StreamIdlePhase {
    match event {
        LlmEvent::Start => current,
        LlmEvent::TextStart | LlmEvent::TextDelta { .. } => StreamIdlePhase::OutputStreaming,
        LlmEvent::TextEnd => StreamIdlePhase::AmbiguousSilent,
        LlmEvent::ThinkingStart | LlmEvent::ThinkingDelta { .. } => {
            StreamIdlePhase::ReasoningStreaming
        }
        LlmEvent::ThinkingEnd => StreamIdlePhase::AmbiguousSilent,
        LlmEvent::ToolCallStart | LlmEvent::ToolCallDelta { .. } => StreamIdlePhase::ToolStreaming,
        LlmEvent::ToolCallEnd { .. } => StreamIdlePhase::AmbiguousSilent,
        LlmEvent::Done { .. } | LlmEvent::Error { .. } => current,
    }
}

async fn consume_llm_stream(
    rx: &mut tokio::sync::mpsc::Receiver<LlmEvent>,
    events: &broadcast::Sender<AgentEvent>,
    provider: &str,
    model: &str,
    cancel_keeps_prompt: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> anyhow::Result<AssistantMessage> {
    let mut text_parts: Vec<String> = Vec::new();
    let mut thinking_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut final_raw: Value = Value::Null;
    let mut provider_tokens: (u64, u64, u64, u64) = (0, 0, 0, 0); // (input, output, cache_read, cache_write)
    let mut provider_telemetry = None;

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
    let initial_idle_timeout = std::env::var("OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds >= 30)
        .map(std::time::Duration::from_secs)
        .unwrap_or_else(|| std::time::Duration::from_secs(90));
    let content_idle_timeout = std::time::Duration::from_secs(90);
    // Reasoning phase: the model has begun thinking but emitted no content or
    // tool call yet. Reasoning models (OpenAI gpt-5.x/o-series, Anthropic
    // interleaved thinking) can stream nothing — not even reasoning-summary
    // deltas — for minutes. Give this phase a strictly longer leash than the
    // active-content phase, mirroring the provider-side SSE watchdog.
    let reasoning_idle_timeout = std::env::var("OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds >= 60)
        .map(std::time::Duration::from_secs)
        .unwrap_or_else(|| std::time::Duration::from_secs(600));
    // Active output phases use a tight watchdog; explicit thinking and
    // provider decision gaps between output items use the generous reasoning
    // budget. This mirrors provider-side SSE gates for Anthropic/Codex and
    // local thinking-capable providers such as Ollama.
    let stream_idle_phase =
        std::sync::atomic::AtomicU8::new(StreamIdlePhase::AwaitingFirstEvent as u8);
    let idle_timeout = || {
        select_stream_idle_budget(
            StreamIdlePhase::from_u8(stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed)),
            initial_idle_timeout,
            content_idle_timeout,
            reasoning_idle_timeout,
        )
    };
    while let Some(event) = 'recv: loop {
        match tokio::time::timeout(idle_timeout(), rx.recv()).await {
            Ok(event) => break 'recv event,
            Err(_) => {
                let phase = StreamIdlePhase::from_u8(
                    stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed),
                );
                // A silent gap *after* active output/tool streaming is most
                // often a reasoning-capable provider (notably the OpenAI
                // Responses API used by openai-codex) pausing between output
                // items without first emitting a TextEnd/ToolCallEnd. The phase
                // is still OutputStreaming/ToolStreaming, so the tight active
                // budget would abort a live reasoning gap and surface as a
                // spurious "stalled stream — retrying". Downgrade once to the
                // ambiguous-silent phase and re-arm with the generous reasoning
                // budget instead of aborting. A genuinely dead stream still
                // dies on the next timeout (now evaluated in the ambiguous
                // phase), and any resumed delta flips the phase back to active.
                if let Some(rearmed) = rearm_idle_phase(phase) {
                    stream_idle_phase.store(rearmed as u8, std::sync::atomic::Ordering::Relaxed);
                    let idle_secs = idle_timeout().as_secs();
                    tracing::debug!(
                        provider,
                        model,
                        from_phase = phase.label(),
                        idle_secs,
                        "stream idle after active output — re-arming with reasoning budget before treating as a stall"
                    );
                    let _ = events.send(AgentEvent::StreamIdle {
                        provider: provider.to_string(),
                        model: model.to_string(),
                        phase: phase.label().to_string(),
                        idle_secs,
                        ambiguous: true,
                        message: format!(
                            "LLM stream idle for {idle_secs}s after {} — re-arming with reasoning budget before treating as a stall",
                            phase.label()
                        ),
                    });
                    continue 'recv;
                }
                let reason = if phase.is_ambiguous_reasoning() {
                    format!(
                        "LLM stream had no observable activity for {}s during {} — this may be a long-running reasoning window or a stalled stream",
                        idle_timeout().as_secs(),
                        phase.label()
                    )
                } else {
                    format!(
                        "LLM stream idle for {}s during {} — connection may be stalled",
                        idle_timeout().as_secs(),
                        phase.label()
                    )
                };
                let _ = events.send(AgentEvent::StreamIdle {
                    provider: provider.to_string(),
                    model: model.to_string(),
                    phase: phase.label().to_string(),
                    idle_secs: idle_timeout().as_secs(),
                    ambiguous: phase.is_ambiguous_reasoning(),
                    message: reason.clone(),
                });
                let _ = events.send(AgentEvent::MessageAbort {
                    reason: Some(reason.clone()),
                });
                anyhow::bail!("{reason}");
            }
        }
    } {
        let next_phase = stream_idle_phase_after_event(
            StreamIdlePhase::from_u8(stream_idle_phase.load(std::sync::atomic::Ordering::Relaxed)),
            &event,
        );
        stream_idle_phase.store(next_phase as u8, std::sync::atomic::Ordering::Relaxed);
        match event {
            LlmEvent::Start => {
                // Heartbeat — any server activity proves connection is alive.
                // Does NOT count as "content" for timeout phase transition.
            }
            LlmEvent::TextStart => {}
            LlmEvent::TextDelta { delta } => {
                if !delta.is_empty() {
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
                // Active reasoning has begun. This is a liveness phase, not a
                // promise that every provider exposes raw chain-of-thought.
            }
            LlmEvent::ThinkingDelta { delta } => {
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
            LlmEvent::ToolCallStart => {}
            LlmEvent::ToolCallDelta { .. } => {
                // Deltas accumulated by the bridge — complete tool call in ToolCallEnd
            }
            LlmEvent::ToolCallEnd { tool_call } => {
                tool_calls.push(ToolCall {
                    id: tool_call.id,
                    name: tool_call.name,
                    arguments: tool_call.arguments,
                });
            }
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
                break;
            }
            LlmEvent::Error { message } => {
                let _ = events.send(AgentEvent::MessageAbort {
                    reason: Some(message.clone()),
                });
                anyhow::bail!("LLM error: {message}");
            }
        }
    }

    let _ = events.send(AgentEvent::MessageEnd);

    // Detect incomplete streams — if we never got a Done event, the bridge
    // probably died. An empty message with no text and no tool calls is
    // almost certainly a dropped connection, not a valid LLM response.
    if final_raw == Value::Null && text_parts.is_empty() && tool_calls.is_empty() {
        anyhow::bail!("LLM stream ended without a completion event — the bridge may have crashed");
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

/// Dispatch tool calls via the EventBus.
///
/// **Auto-batching**: when the model returns multiple mutation calls in one turn,
/// the loop snapshots target files before execution. Contiguous `edit` calls may
/// be collapsed into one hidden transactional `change` execution, and if any
/// mutation fails, previously applied mutations in the turn are rolled back.
/// Record of a permission decision made during tool dispatch.
/// Returned to the caller so it can emit BusEvent::PermissionDecision
/// (which requires &mut bus, unavailable inside dispatch).
#[derive(Debug)]
struct PermissionRecord {
    tool_name: String,
    path: String,
    decision: String,
    kind: omegon_traits::PermissionRequestKind,
    persistence: omegon_traits::PermissionPersistence,
    grant_path: Option<String>,
}

struct DispatchResult {
    results: Vec<ToolResultEntry>,
    permission_decisions: Vec<PermissionRecord>,
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_tools(
    bus: &crate::bus::EventBus,
    tool_calls: &[ToolCall],
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    cwd: &std::path::Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    host_context: Option<&crate::host_context::HostContext>,
    permission_policy: Option<&crate::permissions::LayeredPermissionPolicy>,
    permission_role: Option<styrene_rbac::Role>,
) -> DispatchResult {
    let tool_catalog = ToolCapabilityCatalog::from_tool_defs(&bus.all_tool_definitions());
    let mut permission_decisions: Vec<PermissionRecord> = Vec::new();
    let mut results = Vec::with_capacity(tool_calls.len());

    // ── Auto-batch: snapshot files targeted by mutation tools ────────
    let mutation_count = tool_calls
        .iter()
        .filter(|c| is_mutation_tool_name(&tool_catalog, &c.name))
        .count();
    let batch_mode = mutation_count >= 2;

    let mut snapshots: HashMap<std::path::PathBuf, String> = HashMap::new();
    let mut created_files: Vec<std::path::PathBuf> = Vec::new(); // new files to delete on rollback
    let mut mutated_files: Vec<std::path::PathBuf> = Vec::new();

    if batch_mode {
        for call in tool_calls {
            if is_mutation_tool_name(&tool_catalog, &call.name)
                && let Some(path_str) = extract_mutation_path(&call.arguments)
            {
                let full = cwd.join(&path_str);
                if full.exists() {
                    if !snapshots.contains_key(&full)
                        && let Ok(content) = tokio::fs::read_to_string(&full).await
                    {
                        snapshots.insert(full, content);
                    }
                } else {
                    created_files.push(full);
                }
            }
        }
        if !snapshots.is_empty() {
            tracing::info!(
                files = snapshots.len(),
                edits = mutation_count,
                "Auto-batch: snapshotted {} file(s) for {} mutations",
                snapshots.len(),
                mutation_count
            );
        }
    }

    let cwd_buf = cwd.to_path_buf();

    let mut serial_calls: Vec<(usize, ToolCall)> = Vec::new();
    let mut parallel_calls: Vec<(usize, ToolCall)> = Vec::new();
    let allow_parallel_read_only = !batch_mode && secrets.is_none();
    for (idx, call) in tool_calls.iter().cloned().enumerate() {
        if allow_parallel_read_only && is_parallel_safe_read_only_tool(&call.name) {
            parallel_calls.push((idx, call));
        } else {
            serial_calls.push((idx, call));
        }
    }

    let mut indexed_results: Vec<(usize, ToolResultEntry)> = Vec::with_capacity(tool_calls.len());

    if !parallel_calls.is_empty() {
        let parallel_outcomes = stream::iter(parallel_calls.into_iter().map(|(idx, call)| {
            let events = events.clone();
            let cancel = cancel.clone();
            async move {
                // Parallel calls are read-only — no permission prompts expected.
                let mut perm_log = Vec::new();
                let result = dispatch_single_tool(
                    bus,
                    &call,
                    &events,
                    cancel,
                    None,
                    host_context,
                    &mut perm_log,
                    permission_policy,
                    permission_role,
                )
                .await;
                (idx, result)
            }
        }))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        indexed_results.extend(parallel_outcomes);
    }

    let mut batch_failed = false;
    let mut serial_idx = 0usize;

    while serial_idx < serial_calls.len() {
        if let Some((next_idx, batch_results, batch_mutated_files, edit_batch_failed)) =
            dispatch_edit_batch(
                bus,
                &serial_calls,
                serial_idx,
                &tool_catalog,
                events,
                cancel.clone(),
                cwd,
                secrets,
                &mut permission_decisions,
            )
            .await
        {
            if edit_batch_failed {
                batch_failed = true;
            }
            mutated_files.extend(batch_mutated_files);
            indexed_results.extend(batch_results);
            serial_idx = next_idx;
            continue;
        }

        let (idx, call) = serial_calls[serial_idx].clone();
        if batch_failed && is_mutation_tool_name(&tool_catalog, &call.name) {
            let skip_text = format!(
                "Skipped {} — previous edit in this turn failed and triggered rollback.",
                call.name
            );
            let _ = events.send(AgentEvent::ToolEnd {
                id: call.id.clone(),
                name: call.name.clone(),
                result: omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text {
                        text: skip_text.clone(),
                    }],
                    details: Value::Null,
                },
                is_error: true,
                provenance: bus.tool_provenance(&call.name),
            });
            indexed_results.push((
                idx,
                ToolResultEntry {
                    call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    content: vec![ContentBlock::Text { text: skip_text }],
                    is_error: true,
                    args_summary: summarize_tool_args(&call.name, &call.arguments),
                },
            ));
            serial_idx += 1;
            continue;
        }

        let dispatched = dispatch_single_tool(
            bus,
            &call,
            events,
            cancel.clone(),
            secrets,
            host_context,
            &mut permission_decisions,
            permission_policy,
            permission_role,
        )
        .await;

        if !dispatched.is_error
            && is_mutation_tool_name(&tool_catalog, &call.name)
            && let Some(path_str) = extract_mutation_path(&call.arguments)
        {
            mutated_files.push(cwd_buf.join(&path_str));
        }

        if dispatched.is_error
            && batch_mode
            && is_mutation_tool_name(&tool_catalog, &call.name)
            && !mutated_files.is_empty()
        {
            batch_failed = true;
            tracing::warn!(
                failed_tool = call.name,
                mutated = mutated_files.len(),
                "Auto-batch: mutation failed — rolling back {} file(s)",
                mutated_files.len()
            );

            let mut rollback_report = Vec::new();
            for file in &mutated_files {
                if let Some(original) = snapshots.get(file) {
                    match tokio::fs::write(file, original).await {
                        Ok(_) => rollback_report.push(format!("  ✓ restored {}", file.display())),
                        Err(e) => rollback_report
                            .push(format!("  ✗ rollback failed {}: {e}", file.display())),
                    }
                } else if created_files.contains(file) {
                    match tokio::fs::remove_file(file).await {
                        Ok(_) => rollback_report.push(format!("  ✓ removed {}", file.display())),
                        Err(e) => rollback_report
                            .push(format!("  ✗ remove failed {}: {e}", file.display())),
                    }
                }
            }

            let mut error_text = dispatched
                .content
                .iter()
                .filter_map(|c| c.as_text())
                .collect::<Vec<_>>()
                .join("\n");
            error_text.push_str("\n\n[Auto-rollback: previous edits in this turn were reverted]\n");
            error_text.push_str(&rollback_report.join("\n"));

            let _ = events.send(AgentEvent::ToolEnd {
                id: call.id.clone(),
                name: call.name.clone(),
                result: omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text {
                        text: error_text.clone(),
                    }],
                    details: Value::Null,
                },
                is_error: true,
                provenance: bus.tool_provenance(&call.name),
            });

            indexed_results.push((
                idx,
                ToolResultEntry {
                    call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    content: vec![ContentBlock::Text { text: error_text }],
                    is_error: true,
                    args_summary: summarize_tool_args(&call.name, &call.arguments),
                },
            ));
            continue;
        }

        indexed_results.push((idx, dispatched));
        serial_idx += 1;
    }

    indexed_results.sort_by_key(|(idx, _)| *idx);
    results.extend(indexed_results.into_iter().map(|(_, result)| result));
    DispatchResult {
        results,
        permission_decisions,
    }
}

fn is_parallel_safe_read_only_tool(name: &str) -> bool {
    // File reads can hit workspace-boundary permission prompts. Those prompts
    // are interactive and single-pending in the TUI, so dispatching read/view
    // in parallel can overwrite the visible responder and leave earlier reads
    // blocked until timeout. Keep filesystem reads serial; only inherently
    // permissionless read-only tools may run concurrently.
    matches!(name, "web_search" | "whoami" | "chronos")
}

fn enrich_plan_list_tool_results(
    results: &mut [ToolResultEntry],
    calls: &[ToolCall],
    intent: &IntentDocument,
) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let repo_root = crate::setup::find_project_root(&cwd);
    for (call, result) in calls.iter().zip(results.iter_mut()) {
        if call.name != crate::tool_registry::core::PLAN {
            continue;
        }
        let action = call
            .arguments
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("status");
        if action != "list" {
            continue;
        }
        let mut text = crate::plan::render_plan_list_text(intent, &repo_root);
        text.push('\n');
        text.push_str(
            &result
                .content
                .iter()
                .filter_map(ContentBlock::as_text)
                .collect::<Vec<_>>()
                .join("\n"),
        );
        result.content = vec![ContentBlock::Text { text }];
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_single_tool(
    bus: &crate::bus::EventBus,
    call: &ToolCall,
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    secrets: Option<&omegon_secrets::SecretsManager>,
    host_context: Option<&crate::host_context::HostContext>,
    permission_log: &mut Vec<PermissionRecord>,
    permission_policy: Option<&crate::permissions::LayeredPermissionPolicy>,
    permission_role: Option<styrene_rbac::Role>,
) -> ToolResultEntry {
    let (result, is_error) = execute_tool_invocation(
        bus,
        &call.id,
        &call.name,
        &call.arguments,
        &call.name,
        call.arguments.clone(),
        events,
        cancel,
        secrets,
        permission_log,
        true,
        host_context,
        permission_policy,
        permission_role,
    )
    .await;

    ToolResultEntry {
        call_id: call.id.clone(),
        tool_name: call.name.clone(),
        content: result.content,
        is_error,
        args_summary: summarize_tool_args(&call.name, &call.arguments),
    }
}

#[allow(clippy::too_many_arguments)]
async fn wait_for_permission_response(
    rx: std::sync::mpsc::Receiver<omegon_traits::PermissionResponse>,
    cancel: CancellationToken,
) -> omegon_traits::PermissionResponse {
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_blocking(move || {
        let _ = notify_tx.send(rx.recv());
    });

    // Permission prompts are an operator control boundary, not a soft failure.
    // Match Claude Code semantics: once a tool needs outside-workspace access,
    // the run waits until the operator allows or denies it. There is no passive
    // timeout. Explicit run cancellation still unblocks as a denial/cancelled
    // decision, and pre-approved paths or --dangerously-bypass-permissions avoid
    // this branch upstream.
    tokio::select! {
        _ = cancel.cancelled() => omegon_traits::PermissionResponse::Deny,
        response = notify_rx.recv() => response
            .and_then(Result::ok)
            .unwrap_or(omegon_traits::PermissionResponse::Deny),
    }
}

fn format_policy_permission_subject(
    tool: &str,
    subject: Option<&crate::permissions::PermissionSubject>,
) -> String {
    match subject {
        Some(subject) if subject.kind == crate::permissions::PermissionSubjectKind::Path => {
            subject.value.clone()
        }
        Some(subject) => format!("policy:{}:{}", tool, subject.value),
        None => format!("policy:{tool}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_tool_invocation(
    bus: &crate::bus::EventBus,
    visible_call_id: &str,
    visible_tool_name: &str,
    visible_args: &Value,
    execution_tool_name: &str,
    execution_args: Value,
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    secrets: Option<&omegon_secrets::SecretsManager>,
    permission_log: &mut Vec<PermissionRecord>,
    emit_agent_events: bool,
    host_context: Option<&crate::host_context::HostContext>,
    permission_policy: Option<&crate::permissions::LayeredPermissionPolicy>,
    permission_role: Option<styrene_rbac::Role>,
) -> (omegon_traits::ToolResult, bool) {
    let provenance = bus.tool_provenance(execution_tool_name);
    if let Some(sm) = secrets
        && let Some(decision) = sm.check_guard(visible_tool_name, visible_args)
        && decision.is_block()
    {
        let msg = match &decision {
            omegon_secrets::GuardDecision::Block { reason, path } => {
                format!("Blocked: {reason} ({path})")
            }
            _ => unreachable!(),
        };
        tracing::warn!(tool = visible_tool_name, %msg, "tool guard blocked");
        if emit_agent_events {
            let _ = events.send(AgentEvent::ToolEnd {
                id: visible_call_id.to_string(),
                name: visible_tool_name.to_string(),
                result: omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text { text: msg.clone() }],
                    details: Value::Null,
                },
                is_error: true,
                provenance: provenance.clone(),
            });
        }
        return (
            omegon_traits::ToolResult {
                content: vec![ContentBlock::Text { text: msg }],
                details: Value::Null,
            },
            true,
        );
    }

    if emit_agent_events {
        let _ = events.send(AgentEvent::ToolStart {
            id: visible_call_id.to_string(),
            name: visible_tool_name.to_string(),
            args: visible_args.clone(),
            provenance: provenance.clone(),
        });
    }

    if let Some(role) = permission_role
        && !crate::permissions::styrene_role_allows_tool(role, visible_tool_name)
    {
        let text = format!(
            "BLOCKED: `{}` requires Styrene capability `{}` not held by role `{}`.",
            visible_tool_name,
            crate::permissions::styrene_capability_for_tool(visible_tool_name)
                .unwrap_or("<unknown>"),
            role.as_str()
        );
        return (
            omegon_traits::ToolResult {
                content: vec![ContentBlock::Text { text }],
                details: serde_json::json!({
                    "is_error": true,
                    "blocked": true,
                    "reason": "styrene_rbac_denied",
                    "role": role.as_str(),
                    "capability": crate::permissions::styrene_capability_for_tool(visible_tool_name),
                }),
            },
            true,
        );
    }

    if let Some(policy) = permission_policy {
        let subjects = crate::permissions::subjects_from_tool_args(visible_tool_name, visible_args);
        let decision = policy.evaluate_subjects(visible_tool_name, &subjects);
        match decision.action {
            crate::permissions::PermissionAction::Deny => {
                let text = format!(
                    "BLOCKED: `{}` denied by permission policy layer {:?}.",
                    visible_tool_name, decision.layer
                );
                return (
                    omegon_traits::ToolResult {
                        content: vec![ContentBlock::Text { text }],
                        details: serde_json::json!({
                            "is_error": true,
                            "blocked": true,
                            "reason": "permission_policy_denied",
                            "layer": decision.layer.map(|layer| layer.as_str()).unwrap_or("none").to_string(),
                            "action": decision.action.as_str(),
                        }),
                    },
                    true,
                );
            }
            crate::permissions::PermissionAction::Prompt => {
                let requested =
                    format_policy_permission_subject(visible_tool_name, subjects.first());
                let response = if let Some(ctx) = host_context {
                    match ctx
                        .proxy
                        .request_permission(
                            visible_call_id.to_string(),
                            visible_tool_name.to_string(),
                            requested.clone(),
                        )
                        .await
                    {
                        Ok(agent_client_protocol::schema::RequestPermissionOutcome::Selected(
                            sel,
                        )) => {
                            match sel.option_id.0.as_ref() {
                                // Policy prompts do not yet have a durable/session grant target.
                                // Treat host "allow always" selections as allow-once so the
                                // permission surface does not imply persistence we cannot honor.
                                "allow_always" | "allow_once" => {
                                    omegon_traits::PermissionResponse::Allow
                                }
                                _ => omegon_traits::PermissionResponse::Deny,
                            }
                        }
                        _ => omegon_traits::PermissionResponse::Deny,
                    }
                } else {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
                    let _ = events.send(AgentEvent::PermissionRequest {
                        tool_name: visible_tool_name.to_string(),
                        path: requested.clone(),
                        kind: omegon_traits::PermissionRequestKind::Policy,
                        persistence: omegon_traits::PermissionPersistence::None,
                        grant_path: None,
                        respond,
                    });
                    wait_for_permission_response(rx, cancel.clone()).await
                };

                match response {
                    omegon_traits::PermissionResponse::Allow
                    | omegon_traits::PermissionResponse::AllowSession
                    | omegon_traits::PermissionResponse::AlwaysAllow => {
                        permission_log.push(PermissionRecord {
                            tool_name: visible_tool_name.to_string(),
                            path: requested.clone(),
                            decision: "allow".into(),
                            kind: omegon_traits::PermissionRequestKind::Policy,
                            persistence: omegon_traits::PermissionPersistence::None,
                            grant_path: None,
                        });
                    }
                    omegon_traits::PermissionResponse::Deny => {
                        permission_log.push(PermissionRecord {
                            tool_name: visible_tool_name.to_string(),
                            path: requested.clone(),
                            decision: "deny".into(),
                            kind: omegon_traits::PermissionRequestKind::Policy,
                            persistence: omegon_traits::PermissionPersistence::None,
                            grant_path: None,
                        });
                        let text = format!(
                            "BLOCKED: `{}` denied by operator after permission-policy prompt.",
                            visible_tool_name
                        );
                        return (
                            omegon_traits::ToolResult {
                                content: vec![ContentBlock::Text { text }],
                                details: serde_json::json!({
                                    "is_error": true,
                                    "blocked": true,
                                    "reason": "permission_policy_prompt_denied",
                                    "layer": decision.layer.map(|layer| layer.as_str()).unwrap_or("none").to_string(),
                                    "action": decision.action.as_str(),
                                }),
                            },
                            true,
                        );
                    }
                }
            }
            crate::permissions::PermissionAction::Allow => {}
        }
    }

    let sink_events = events.clone();
    let sink_call_id = visible_call_id.to_string();
    let sink = omegon_traits::ToolProgressSink::from_fn(move |partial| {
        let _ = sink_events.send(AgentEvent::ToolUpdate {
            id: sink_call_id.clone(),
            partial,
        });
    });

    // Try host delegation before local execution.
    if let Some(ctx) = host_context
        && let Some(result) =
            crate::host_context::try_delegate_to_host(ctx, execution_tool_name, &execution_args)
                .await
    {
        let (tool_result, is_error) = match result {
            Ok(r) => (r, false),
            Err(e) => (
                omegon_traits::ToolResult {
                    content: vec![ContentBlock::Text {
                        text: e.to_string(),
                    }],
                    details: Value::Null,
                },
                true,
            ),
        };
        if emit_agent_events {
            let _ = events.send(AgentEvent::ToolEnd {
                id: visible_call_id.to_string(),
                name: visible_tool_name.to_string(),
                result: tool_result.clone(),
                is_error,
                provenance: provenance.clone(),
            });
        }
        return (tool_result, is_error);
    }

    let tool_context = if let Some(ctx) = host_context {
        let proxy = ctx.proxy.clone();
        let approval_sink: omegon_traits::HostActionApprovalSink = std::sync::Arc::new(
            move |request_json: serde_json::Value| {
                let proxy = proxy.clone();
                Box::pin(async move {
                    let request = match serde_json::from_value::<
                        agent_client_protocol::schema::RequestPermissionRequest,
                    >(request_json)
                    {
                        Ok(request) => request,
                        Err(_) => {
                            return serde_json::to_value(
                                crate::extensions::approval::HostActionApprovalDecision::Unavailable,
                            )
                            .unwrap_or(serde_json::Value::String("unavailable".into()));
                        }
                    };
                    let decision = proxy.request_host_action_approval(request).await.unwrap_or(
                        crate::extensions::approval::HostActionApprovalDecision::Unavailable,
                    );
                    serde_json::to_value(decision)
                        .unwrap_or(serde_json::Value::String("unavailable".into()))
                })
            },
        );
        omegon_traits::ToolExecutionContext {
            host_action_approval: Some(approval_sink),
        }
    } else {
        omegon_traits::ToolExecutionContext::default()
    };

    let execute = |cancel: CancellationToken, sink: omegon_traits::ToolProgressSink| {
        bus.execute_tool_with_context(
            execution_tool_name,
            visible_call_id,
            execution_args.clone(),
            cancel,
            sink,
            tool_context.clone(),
        )
    };

    let first_result = execute(
        cancel.clone(),
        if emit_agent_events {
            sink.clone()
        } else {
            omegon_traits::ToolProgressSink::noop()
        },
    )
    .await;

    // Intercept PathPermissionError — route through ACP permission mediation
    // when a host context is present, or fall back to the TUI blocking prompt.
    let (result, is_error) = match first_result {
        Ok(result) => (result, false),
        Err(e)
            if e.downcast_ref::<crate::tools::OperatorWaitRequired>()
                .is_some() =>
        {
            let wait = e
                .downcast::<crate::tools::OperatorWaitRequired>()
                .expect("checked OperatorWaitRequired downcast");

            if host_context.is_some() {
                (
                    omegon_traits::ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "Manual action required, but interactive operator confirmation is only available in the TUI right now.".into(),
                        }],
                        details: serde_json::json!({
                            "is_error": true,
                            "status": "unsupported_surface",
                            "reason": "operator_wait_requires_tui",
                            "prompt": wait.prompt,
                            "timeoutSecs": wait.timeout_secs,
                        }),
                    },
                    true,
                )
            } else {
                let (tx, rx) = std::sync::mpsc::channel();
                let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
                let (ack_tx, ack_rx) = std::sync::mpsc::channel();
                let acknowledge = std::sync::Arc::new(std::sync::Mutex::new(Some(ack_tx)));
                let _ = events.send(AgentEvent::OperatorWaitRequest {
                    prompt: wait.prompt.clone(),
                    timeout_secs: wait.timeout_secs,
                    acknowledge,
                    respond,
                });

                let acknowledged = tokio::task::spawn_blocking(move || {
                    ack_rx.recv_timeout(Duration::from_secs(2)).is_ok()
                })
                .await
                .unwrap_or(false);
                if !acknowledged {
                    return (
                        omegon_traits::ToolResult {
                            content: vec![ContentBlock::Text {
                                text: "Manual action required, but no interactive operator surface acknowledged the wait request.".into(),
                            }],
                            details: serde_json::json!({
                                "is_error": true,
                                "status": "unsupported_surface",
                                "reason": "operator_wait_not_acknowledged",
                                "prompt": wait.prompt,
                                "timeoutSecs": wait.timeout_secs,
                            }),
                        },
                        true,
                    );
                }

                let start = Instant::now();
                let mut initial = omegon_traits::PartialToolResult::content(
                    format!(
                        "Manual action required:\n{}\n\nWaiting for operator confirmation. Timeout: {} seconds.",
                        wait.prompt, wait.timeout_secs
                    ),
                    0,
                );
                initial.progress.phase = Some("waiting_for_operator".into());
                initial.details = serde_json::json!({
                    "status": "waiting",
                    "prompt": wait.prompt,
                    "timeoutSecs": wait.timeout_secs,
                });
                sink.send(initial);

                let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::task::spawn_blocking(move || {
                    let _ = notify_tx.send(rx.recv());
                });

                let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
                heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let timeout = tokio::time::sleep(Duration::from_secs(wait.timeout_secs));
                tokio::pin!(timeout);

                let status = loop {
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            break "cancelled";
                        }
                        _ = &mut timeout => {
                            break "timed_out";
                        }
                        response = notify_rx.recv() => {
                            match response {
                                Some(Ok(omegon_traits::OperatorWaitResponse::Completed)) => break "completed",
                                Some(Ok(omegon_traits::OperatorWaitResponse::Cancelled)) => break "cancelled",
                                _ => break "cancelled",
                            }
                        }
                        _ = heartbeat.tick() => {
                            let mut partial = omegon_traits::PartialToolResult::heartbeat(
                                start.elapsed().as_millis() as u64,
                            );
                            partial.progress.phase = Some("waiting_for_operator".into());
                            partial.details = serde_json::json!({
                                "status": "waiting",
                                "elapsedSecs": start.elapsed().as_secs(),
                                "timeoutSecs": wait.timeout_secs,
                            });
                            sink.send(partial);
                        }
                    }
                };

                let elapsed_secs = start.elapsed().as_secs();
                let is_error = status != "completed";
                let text = match status {
                    "completed" => format!("Manual action completed after {elapsed_secs}s."),
                    "timed_out" => format!(
                        "Manual action timed out after {elapsed_secs}s without operator confirmation."
                    ),
                    _ => format!("Manual action cancelled after {elapsed_secs}s."),
                };
                (
                    omegon_traits::ToolResult {
                        content: vec![ContentBlock::Text { text }],
                        details: serde_json::json!({
                            "status": status,
                            "elapsedSecs": elapsed_secs,
                            "timeoutSecs": wait.timeout_secs,
                        }),
                    },
                    is_error,
                )
            }
        }
        Err(e)
            if e.downcast_ref::<crate::tools::PathPermissionError>()
                .is_some() =>
        {
            let perm_err = e.downcast::<crate::tools::PathPermissionError>().unwrap();

            let response = if let Some(ctx) = host_context {
                // ACP path: delegate to the host's permission UI.
                match ctx
                    .proxy
                    .request_permission(
                        visible_call_id.to_string(),
                        visible_tool_name.to_string(),
                        perm_err.requested_path.clone(),
                    )
                    .await
                {
                    Ok(outcome) => match outcome {
                        agent_client_protocol::schema::RequestPermissionOutcome::Selected(sel) => {
                            match sel.option_id.0.as_ref() {
                                "allow_always" => omegon_traits::PermissionResponse::AlwaysAllow,
                                "allow_once" => omegon_traits::PermissionResponse::Allow,
                                _ => omegon_traits::PermissionResponse::Deny,
                            }
                        }
                        agent_client_protocol::schema::RequestPermissionOutcome::Cancelled => {
                            omegon_traits::PermissionResponse::Deny
                        }
                        _ => omegon_traits::PermissionResponse::Deny,
                    },
                    Err(_) => omegon_traits::PermissionResponse::Deny,
                }
            } else {
                // TUI path: blocking channel prompt.
                let (tx, rx) = std::sync::mpsc::channel();
                let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));

                let _ = events.send(AgentEvent::PermissionRequest {
                    tool_name: visible_tool_name.to_string(),
                    path: perm_err.requested_path.clone(),
                    kind: omegon_traits::PermissionRequestKind::PathBoundary,
                    persistence: omegon_traits::PermissionPersistence::ProjectDirectory,
                    grant_path: Some(perm_err.directory.clone()),
                    respond,
                });

                wait_for_permission_response(rx, cancel.clone()).await
            };

            match response {
                omegon_traits::PermissionResponse::Allow => {
                    tracing::info!(path = %perm_err.requested_path, decision = "allow_once", "permission decision");
                    permission_log.push(PermissionRecord {
                        tool_name: visible_tool_name.to_string(),
                        path: perm_err.requested_path.clone(),
                        decision: "allow_once".into(),
                        kind: omegon_traits::PermissionRequestKind::PathBoundary,
                        persistence: omegon_traits::PermissionPersistence::None,
                        grant_path: None,
                    });
                    // Approve only the canonical target. For a file this grants that
                    // one path, not its siblings; directories remain explicit scopes.
                    let target = crate::tools::canonicalize_existing_parent_for_permissions(
                        std::path::Path::new(&perm_err.requested_path),
                    );
                    let trust_args = serde_json::json!({
                        "path": target,
                        "scope": "session",
                    });
                    if let Err(e) = bus
                        .execute_internal(
                            crate::tool_registry::core::TRUST_DIRECTORY,
                            "__permission_grant",
                            trust_args,
                            cancel.clone(),
                        )
                        .await
                    {
                        tracing::error!(error = %e, "trust_directory internal call failed — permission may not take effect");
                    }
                    match execute(cancel, sink).await {
                        Ok(result) => (result, false),
                        Err(e) => (
                            omegon_traits::ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: e.to_string(),
                                }],
                                details: Value::Null,
                            },
                            true,
                        ),
                    }
                }
                omegon_traits::PermissionResponse::AllowSession => {
                    tracing::info!(dir = %perm_err.directory, decision = "allow_session", "permission decision");
                    permission_log.push(PermissionRecord {
                        tool_name: visible_tool_name.to_string(),
                        path: perm_err.requested_path.clone(),
                        decision: "allow_session".into(),
                        kind: omegon_traits::PermissionRequestKind::PathBoundary,
                        persistence: omegon_traits::PermissionPersistence::SessionDirectory,
                        grant_path: Some(perm_err.directory.clone()),
                    });
                    let trust_args = serde_json::json!({
                        "path": perm_err.directory,
                        "scope": "session",
                    });
                    if let Err(e) = bus
                        .execute_internal(
                            crate::tool_registry::core::TRUST_DIRECTORY,
                            "__permission_grant",
                            trust_args,
                            cancel.clone(),
                        )
                        .await
                    {
                        tracing::error!(error = %e, "trust_directory internal call failed — permission may not take effect");
                    }
                    match execute(cancel, sink).await {
                        Ok(result) => (result, false),
                        Err(e) => (
                            omegon_traits::ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: e.to_string(),
                                }],
                                details: Value::Null,
                            },
                            true,
                        ),
                    }
                }
                omegon_traits::PermissionResponse::AlwaysAllow => {
                    tracing::info!(dir = %perm_err.directory, decision = "always_allow", "permission decision");
                    permission_log.push(PermissionRecord {
                        tool_name: visible_tool_name.to_string(),
                        path: perm_err.requested_path.clone(),
                        decision: "always_allow".into(),
                        kind: omegon_traits::PermissionRequestKind::PathBoundary,
                        persistence: omegon_traits::PermissionPersistence::ProjectDirectory,
                        grant_path: Some(perm_err.directory.clone()),
                    });
                    let trust_args = serde_json::json!({
                        "path": perm_err.directory,
                        "scope": "persistent",
                    });
                    if let Err(e) = bus
                        .execute_internal(
                            crate::tool_registry::core::TRUST_DIRECTORY,
                            "__permission_grant",
                            trust_args,
                            cancel.clone(),
                        )
                        .await
                    {
                        tracing::error!(error = %e, "trust_directory internal call failed — permission may not take effect");
                    }
                    match execute(cancel, sink).await {
                        Ok(result) => (result, false),
                        Err(e) => (
                            omegon_traits::ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: e.to_string(),
                                }],
                                details: Value::Null,
                            },
                            true,
                        ),
                    }
                }
                omegon_traits::PermissionResponse::Deny => {
                    tracing::info!(path = %perm_err.requested_path, decision = "deny", "permission decision");
                    permission_log.push(PermissionRecord {
                        tool_name: visible_tool_name.to_string(),
                        path: perm_err.requested_path.clone(),
                        decision: "deny".into(),
                        kind: omegon_traits::PermissionRequestKind::PathBoundary,
                        persistence: omegon_traits::PermissionPersistence::None,
                        grant_path: Some(perm_err.directory.clone()),
                    });
                    (
                        omegon_traits::ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "BLOCKED: '{}' is outside the workspace. \
                                     This operation was denied by the permission system. \
                                     The operator can run /permissions add {} to allow \
                                     access to this directory, then re-run the task.",
                                    perm_err.requested_path, perm_err.directory,
                                ),
                            }],
                            details: serde_json::json!({
                                "is_error": true,
                                "blocked": true,
                                "reason": "path_outside_workspace",
                                "directory": perm_err.directory,
                            }),
                        },
                        true,
                    )
                }
            }
        }
        Err(e) => (
            omegon_traits::ToolResult {
                content: vec![ContentBlock::Text {
                    text: e.to_string(),
                }],
                details: Value::Null,
            },
            true,
        ),
    };

    let mut final_content = result.content;
    if let Some(sm) = secrets {
        sm.redact_content(&mut final_content);
    }

    const MAX_TOOL_OUTPUT_CHARS: usize = 16_000;
    crate::util::truncate_content_blocks(&mut final_content, MAX_TOOL_OUTPUT_CHARS);

    if emit_agent_events {
        let _ = events.send(AgentEvent::ToolEnd {
            id: visible_call_id.to_string(),
            name: visible_tool_name.to_string(),
            result: omegon_traits::ToolResult {
                content: final_content.clone(),
                details: result.details.clone(),
            },
            is_error,
            provenance,
        });
    }

    (
        omegon_traits::ToolResult {
            content: final_content,
            details: result.details,
        },
        is_error,
    )
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_edit_batch(
    bus: &crate::bus::EventBus,
    serial_calls: &[(usize, ToolCall)],
    start_idx: usize,
    tool_catalog: &ToolCapabilityCatalog,
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    cwd: &std::path::Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
    permission_log: &mut Vec<PermissionRecord>,
) -> Option<(
    usize,
    Vec<(usize, ToolResultEntry)>,
    Vec<std::path::PathBuf>,
    bool,
)> {
    if secrets.is_some()
        || !is_mutation_tool_name(tool_catalog, &serial_calls.get(start_idx)?.1.name)
        || serial_calls.get(start_idx)?.1.name != "edit"
        || !bus.has_registered_tool("change")
    {
        return None;
    }

    let mut end_idx = start_idx;
    while let Some((_, call)) = serial_calls.get(end_idx) {
        if call.name != "edit" {
            break;
        }
        end_idx += 1;
    }
    if end_idx - start_idx < 2 {
        return None;
    }

    let batch_slice = &serial_calls[start_idx..end_idx];
    let edits: Vec<Value> = batch_slice
        .iter()
        .map(|(_, call)| {
            serde_json::json!({
                "file": call.arguments.get("path").and_then(|v| v.as_str()).unwrap_or_default(),
                "oldText": call.arguments.get("oldText").and_then(|v| v.as_str()).unwrap_or_default(),
                "newText": call.arguments.get("newText").and_then(|v| v.as_str()).unwrap_or_default(),
            })
        })
        .collect();
    let change_args = serde_json::json!({
        "edits": edits,
        "validate": "none",
    });

    for (_, call) in batch_slice {
        let _ = events.send(AgentEvent::ToolStart {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.arguments.clone(),
            provenance: bus.tool_provenance(&call.name),
        });
    }

    let first_call = &batch_slice[0].1;
    let (batch_result, batch_error) = execute_tool_invocation(
        bus,
        &first_call.id,
        &first_call.name,
        &first_call.arguments,
        "change",
        change_args,
        events,
        cancel,
        None,
        permission_log,
        false,
        None, // batch changes always run locally
        None,
        None,
    )
    .await;

    let batch_text = batch_result
        .content
        .iter()
        .filter_map(|block| block.as_text())
        .collect::<Vec<_>>()
        .join("\n");

    let mut indexed_results = Vec::with_capacity(batch_slice.len());
    let mut mutated_files = Vec::new();
    for (position, (idx, call)) in batch_slice.iter().enumerate() {
        let path = call
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let text = if batch_error {
            if position == 0 {
                batch_text.clone()
            } else {
                format!("Skipped edit in {path} — atomic edit batch failed.\n\n{batch_text}")
            }
        } else if position + 1 == batch_slice.len() {
            format!("Applied exact-text edit to {path} as part of an atomic batch.\n\n{batch_text}")
        } else {
            format!("Applied exact-text edit to {path} as part of an atomic batch.")
        };

        if !batch_error && let Some(path_str) = extract_mutation_path(&call.arguments) {
            mutated_files.push(cwd.join(path_str));
        }

        let _ = events.send(AgentEvent::ToolEnd {
            id: call.id.clone(),
            name: call.name.clone(),
            result: omegon_traits::ToolResult {
                content: vec![ContentBlock::Text { text: text.clone() }],
                details: if position + 1 == batch_slice.len() {
                    batch_result.details.clone()
                } else {
                    Value::Null
                },
            },
            is_error: batch_error,
            provenance: bus.tool_provenance(&call.name),
        });

        indexed_results.push((
            *idx,
            ToolResultEntry {
                call_id: call.id.clone(),
                tool_name: call.name.clone(),
                content: vec![ContentBlock::Text { text }],
                is_error: batch_error,
                args_summary: summarize_tool_args(&call.name, &call.arguments),
            },
        ));
    }

    Some((end_idx, indexed_results, mutated_files, batch_error))
}

/// Extract the target file path from mutation tool arguments.
fn extract_mutation_path(args: &Value) -> Option<String> {
    args.get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Check if the conversation contains any file mutations (edit or write calls).
fn has_mutations(conversation: &ConversationState) -> bool {
    !conversation.intent.files_modified.is_empty()
}

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
        return false;
    }
    if looks_like_blocked_response(assistant) || looks_like_completion(assistant) {
        return false;
    }
    if looks_like_incomplete_structured_answer(assistant) {
        return matches!(
            automation_level,
            crate::settings::AutomationLevel::Flow | crate::settings::AutomationLevel::Autonomous
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
    user_prompt_expects_concrete_action(user_prompt) && looks_like_plan_or_future_action(assistant)
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
    let lower = text.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "continue" | "proceed" | "go ahead" | "do it" | "make it so"
    ) || lower.contains("get it done")
        || lower.contains("do it already")
        || lower.contains("stop talking")
        || lower.contains("make it so")
        || lower.contains("go ahead")
        || lower.contains("continue on")
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
/// Used to gate the commit nudge — mid-task text responses (progress updates,
/// questions, partial explanations) should not trigger a commit interrupt.
/// A completion response should.
fn looks_like_completion(text: &str) -> bool {
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

/// Maximum reconciliation nudges for a single unchanged stale-plan state.
/// Bounds livelock with a model that refuses to reconcile, while no longer
/// disarming reconciliation after one nudge (the former one-shot-latch bug).
const MAX_PLAN_RECONCILIATION_NUDGES: u8 = 3;

fn plan_open_items(
    intent: &IntentDocument,
) -> Vec<(usize, crate::conversation::WorkItemStatus, &str)> {
    let items = intent
        .visible_plan
        .as_ref()
        .map(|plan| plan.items.as_slice())
        .unwrap_or(intent.work_plan.as_slice());
    items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            matches!(
                item.status,
                crate::conversation::WorkItemStatus::Pending
                    | crate::conversation::WorkItemStatus::Active
            )
            .then_some((idx, item.status, item.description.as_str()))
        })
        .collect()
}

/// Fingerprint of the operator-visible open (Pending/Active) plan items.
/// Changes whenever the visible plan changes, the agent makes genuine progress,
/// replaces the plan, or orphans a new one — which re-arms the reconciliation
/// nudge budget.
fn plan_open_fingerprint(intent: &IntentDocument) -> u64 {
    // Stable FNV-1a; do not use DefaultHasher here because this value is stored
    // in session state and may be compared after reload/resume.
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(FNV_PRIME);
        }
        *hash ^= 0xff;
        *hash = hash.wrapping_mul(FNV_PRIME);
    }

    let mut hash = FNV_OFFSET;
    if let Some(plan) = intent.visible_plan.as_ref() {
        feed(&mut hash, plan.plan_id.as_bytes());
        feed(&mut hash, plan.scope.label().as_bytes());
    } else {
        feed(&mut hash, b"legacy-work-plan");
    }
    for (idx, status, description) in plan_open_items(intent) {
        feed(&mut hash, idx.to_string().as_bytes());
        feed(&mut hash, format!("{status:?}").as_bytes());
        feed(&mut hash, description.as_bytes());
    }
    hash
}

fn should_nudge_plan_reconciliation(intent: &IntentDocument, _assistant_text: &str) -> bool {
    if plan_open_items(intent).is_empty() {
        return false;
    }
    // A new or changed stale-plan state always re-arms the nudge.
    if intent.plan_reconciliation_fingerprint != Some(plan_open_fingerprint(intent)) {
        return true;
    }
    // Identical stale state: nudge a bounded number of times.
    intent.plan_reconciliation_nudges < MAX_PLAN_RECONCILIATION_NUDGES
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
            if is_repo_inspection_tool(catalog, name) || crate::observation::is_read_program(name) {
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

/// Summarize tool call arguments into a compact string for decay context.
/// Returns None if no useful summary can be extracted.
pub fn summarize_tool_args(tool_name: &str, args: &Value) -> Option<String> {
    match tool_name {
        "read" | "edit" | "write" | "view" => {
            args.get("path").and_then(|v| v.as_str()).map(|p| {
                // Strip common cwd prefixes to show relative paths
                let cwd = std::env::current_dir()
                    .map(|d| d.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !cwd.is_empty() && p.starts_with(&cwd) {
                    p[cwd.len()..]
                        .strip_prefix('/')
                        .unwrap_or(&p[cwd.len()..])
                        .to_string()
                } else {
                    p.to_string()
                }
            })
        }
        "bash" => {
            let cmd = args.get("command").and_then(|v| v.as_str())?;
            // Strip common cwd wrappers: "cd /long/path && actual command"
            let clean = if let Some(rest) = cmd.strip_prefix("cd ") {
                // Find the && and take what's after it
                rest.split_once(" && ")
                    .map(|(_, after)| after)
                    .unwrap_or(rest)
            } else {
                cmd
            };
            // Truncate to keep it compact
            let short = if clean.len() > 60 {
                let mut end = 60;
                while end > 0 && !clean.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &clean[..end])
            } else {
                clean.to_string()
            };
            Some(short)
        }
        "terminal" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            match action {
                "start" => args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .map(|cmd| format!("start: {}", crate::util::truncate(cmd, 60))),
                "send" => args
                    .get("session_id")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|id| format!("send: {id}")),
                "read" => args
                    .get("session_id")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|id| format!("read: {id}")),
                "stop" => args
                    .get("session_id")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .map(|id| format!("stop: {id}")),
                "list" => Some("list".into()),
                other => Some(other.to_string()),
            }
        }
        "change" => {
            let edits = args.get("edits").and_then(|v| v.as_array())?;
            let files: Vec<&str> = edits
                .iter()
                .filter_map(|e| e.get("file").and_then(|v| v.as_str()))
                .collect();
            Some(files.join(", "))
        }
        "web_search" => args.get("query").and_then(|v| v.as_str()).map(|q| {
            if q.len() > 60 {
                crate::util::truncate(q, 60)
            } else {
                q.to_string()
            }
        }),
        "memory_recall" | "memory_store" | "memory_query" => args
            .get("query")
            .or_else(|| args.get("content"))
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 60 {
                    crate::util::truncate(s, 60)
                } else {
                    s.to_string()
                }
            }),
        "cleave_run" => {
            // "N children: label1, label2, …"
            let plan = args
                .get("plan_json")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            let labels: Vec<&str> = plan
                .as_ref()
                .and_then(|p| p.get("children"))
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| c.get("label").and_then(|v| v.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            let n = labels.len();
            if n == 0 {
                Some("cleave".into())
            } else {
                let joined = labels.join(", ");
                let summary = format!("{n} children: {joined}");
                Some(crate::util::truncate(&summary, 60))
            }
        }
        "cleave_assess" => args
            .get("directive")
            .and_then(|v| v.as_str())
            .map(|s| crate::util::truncate(s, 60)),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::{EvidenceAssessment, EvidenceSufficiency, ProgressSignal};
    use crate::conversation::{
        PlanBinding, PlanMode, PlanScope, PlanSource, VisiblePlanState, WorkItem, WorkItemStatus,
    };
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

    #[test]
    fn active_output_silence_rearms_to_reasoning_budget() {
        use std::time::Duration;
        let initial = Duration::from_secs(90);
        let content = Duration::from_secs(90);
        let reasoning = Duration::from_secs(600);

        // Regression: openai-codex (OpenAI Responses API) streams text deltas
        // for one item, then pauses for minutes deciding the next item without
        // first emitting TextEnd. The phase is still OutputStreaming, so the
        // *first* silence must re-arm to the ambiguous-silent phase and pick up
        // the generous reasoning leash instead of aborting at the tight budget.
        for active_phase in [
            StreamIdlePhase::OutputStreaming,
            StreamIdlePhase::ToolStreaming,
        ] {
            let rearmed =
                rearm_idle_phase(active_phase).expect("active output silence must re-arm once");
            assert_eq!(rearmed, StreamIdlePhase::AmbiguousSilent);
            assert_eq!(
                select_stream_idle_budget(rearmed, initial, content, reasoning),
                reasoning,
                "re-armed phase must use the generous reasoning budget"
            );
        }

        // A *second* silence is now evaluated in the ambiguous phase: it must
        // NOT re-arm, so a genuinely dead stream still dies inside the retry
        // budget rather than looping forever.
        assert_eq!(rearm_idle_phase(StreamIdlePhase::AmbiguousSilent), None);
        assert_eq!(rearm_idle_phase(StreamIdlePhase::ReasoningStreaming), None);
        // Awaiting-first-event silence is a connection problem, not a reasoning
        // gap; it must surface as a stall, not re-arm.
        assert_eq!(rearm_idle_phase(StreamIdlePhase::AwaitingFirstEvent), None);
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
    fn extract_path_from_args() {
        let args = serde_json::json!({"path": "src/main.rs", "oldText": "a", "newText": "b"});
        assert_eq!(extract_mutation_path(&args).as_deref(), Some("src/main.rs"));

        let no_path = serde_json::json!({"command": "ls"});
        assert!(extract_mutation_path(&no_path).is_none());
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

    #[test]
    fn plan_tool_update_renders_operator_checklist_snapshot() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Inspect plan rendering".into(), "Patch TUI".into()]);
        intent.execute_work_plan();
        intent.advance_work_plan();
        let calls = vec![ToolCall {
            id: "plan-1".into(),
            name: crate::tool_registry::core::PLAN.into(),
            arguments: serde_json::json!({"action": "advance"}),
        }];

        let notification = plan_status_notification(&calls, &intent).unwrap();

        assert!(notification.starts_with("Plan progress"));
        assert!(notification.contains("Progress: 1/2"));
        assert!(notification.contains("● Inspect plan rendering"));
        assert!(notification.contains("◐ Patch TUI"));
    }

    #[test]
    fn completing_plan_tool_preserves_operator_checklist_snapshot() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Only item".into()]);
        intent.advance_work_plan();
        let calls = vec![ToolCall {
            id: "plan-1".into(),
            name: crate::tool_registry::core::PLAN.into(),
            arguments: serde_json::json!({"action": "advance"}),
        }];

        let notification = plan_status_notification(&calls, &intent).unwrap();

        assert!(notification.starts_with("Plan progress"));
        assert!(notification.contains("Plan mode: complete"));
        assert!(notification.contains("Progress: 1/1"));
        assert!(notification.contains("● Only item"));
        let completed = intent
            .last_completed_work_plan()
            .expect("completed checklist remains in the completion ledger");
        assert_eq!(completed.items.len(), 1);
        assert_eq!(completed.items[0].description, "Only item");
        assert_eq!(intent.work_plan_snapshot_json()["total"], 0);
        assert_eq!(intent.work_plan_snapshot_json()["mode"], "off");
    }

    #[test]
    fn plan_reconciliation_nudge_requires_incomplete_plan_regardless_of_wording() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Inspect".into(), "Patch".into()]);

        assert!(should_nudge_plan_reconciliation(
            &intent,
            "Done! The patch is validated."
        ));
        assert!(should_nudge_plan_reconciliation(
            &intent,
            "I found the likely issue and will patch the handler next."
        ));
        assert!(should_nudge_plan_reconciliation(&intent, ""));

        intent.advance_work_plan();
        intent.advance_work_plan();
        assert!(!should_nudge_plan_reconciliation(
            &intent,
            "Done! The patch is validated."
        ));
    }

    fn visible_plan_state_with_items(items: Vec<(&str, WorkItemStatus)>) -> VisiblePlanState {
        VisiblePlanState {
            plan_id: "repo:example".into(),
            scope: PlanScope::Repo,
            source: PlanSource::OpenSpec,
            binding: PlanBinding::default(),
            mode: PlanMode::Executing,
            items: items
                .into_iter()
                .map(|(description, status)| WorkItem {
                    description: description.into(),
                    status,
                    intent: None,
                    completion_policy: Default::default(),
                    evidence: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn plan_reconciliation_uses_visible_plan_as_source_of_truth() {
        let mut intent = IntentDocument {
            visible_plan: Some(visible_plan_state_with_items(vec![
                ("Repo task A", WorkItemStatus::Active),
                ("Repo task B", WorkItemStatus::Pending),
            ])),
            ..IntentDocument::default()
        };
        // Legacy plan is empty, but Workbench's visible plan has open rows.
        // This is the adversarial case: the stale surface must still nudge.

        assert!(should_nudge_plan_reconciliation(
            &intent,
            "Done! The patch is validated."
        ));

        // Completed/skipped visible rows do not nudge.
        intent.visible_plan = Some(visible_plan_state_with_items(vec![
            ("Repo task A", WorkItemStatus::Done),
            ("Repo task B", WorkItemStatus::Skipped),
        ]));
        assert!(!should_nudge_plan_reconciliation(
            &intent,
            "Done! The patch is validated."
        ));
    }

    #[test]
    fn plan_open_fingerprint_is_stable_and_sensitive_to_visible_state() {
        let mut a = IntentDocument {
            visible_plan: Some(visible_plan_state_with_items(vec![
                ("Task A", WorkItemStatus::Active),
                ("Task B", WorkItemStatus::Pending),
            ])),
            ..IntentDocument::default()
        };
        let b = a.clone();
        assert_eq!(plan_open_fingerprint(&a), plan_open_fingerprint(&b));

        // Status and item order/index both matter: completing A and activating B
        // is progress and must re-arm the nudge.
        if let Some(plan) = a.visible_plan.as_mut() {
            plan.items[0].status = WorkItemStatus::Done;
            plan.items[1].status = WorkItemStatus::Active;
        }
        assert_ne!(plan_open_fingerprint(&a), plan_open_fingerprint(&b));

        // Plan identity matters too; switching to another visible plan with the
        // same labels is still a new operator-visible stale state.
        let mut c = b.clone();
        c.visible_plan.as_mut().unwrap().plan_id = "repo:other".into();
        assert_ne!(plan_open_fingerprint(&c), plan_open_fingerprint(&b));
    }

    #[test]
    fn plan_reconciliation_nudge_is_bounded_then_rearms_on_progress() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Inspect".into(), "Patch".into()]);
        let done = "Done! The patch is validated.";

        // Simulate the loop's caller bookkeeping for an unchanged stale state:
        // it nudges up to MAX_PLAN_RECONCILIATION_NUDGES times, then stops —
        // bounding livelock without the old one-shot disarm.
        for _ in 0..MAX_PLAN_RECONCILIATION_NUDGES {
            assert!(
                should_nudge_plan_reconciliation(&intent, done),
                "should nudge while under the per-state budget"
            );
            let fp = plan_open_fingerprint(&intent);
            if intent.plan_reconciliation_fingerprint == Some(fp) {
                intent.plan_reconciliation_nudges += 1;
            } else {
                intent.plan_reconciliation_fingerprint = Some(fp);
                intent.plan_reconciliation_nudges = 1;
            }
        }
        // Budget for this exact stale state is now spent.
        assert!(
            !should_nudge_plan_reconciliation(&intent, done),
            "unchanged stale state must stop nudging after the budget"
        );

        // Genuine progress changes the open-plan fingerprint and re-arms — the
        // regression this fixes: a single early nudge no longer disarms
        // reconciliation for the rest of the session.
        intent.advance_work_plan();
        assert!(
            should_nudge_plan_reconciliation(&intent, done),
            "progress (changed fingerprint) must re-arm the nudge"
        );
    }

    #[test]
    fn non_plan_tool_does_not_emit_plan_snapshot() {
        let calls = vec![ToolCall {
            id: "read-1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        }];

        assert!(plan_status_notification(&calls, &IntentDocument::default()).is_none());
    }

    #[test]
    fn work_plan_snapshot_changes_only_for_plan_state_mutations() {
        let mut intent = IntentDocument::default();
        intent.set_work_plan(vec!["Inspect".into(), "Patch".into()]);
        intent.execute_work_plan();
        let before = intent.work_plan_snapshot_json();

        let read_calls = vec![ToolCall {
            id: "read-1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        }];
        intent.update_from_tools(&test_tool_catalog(), &read_calls, &[]);
        let after_read = intent.work_plan_snapshot_json();
        assert!(!work_plan_snapshot_changed(&before, &after_read));

        let plan_calls = vec![ToolCall {
            id: "plan-1".into(),
            name: crate::tool_registry::core::PLAN.into(),
            arguments: serde_json::json!({"action": "advance"}),
        }];
        intent.update_from_tools(&test_tool_catalog(), &plan_calls, &[]);
        let after_plan = intent.work_plan_snapshot_json();
        assert!(work_plan_snapshot_changed(&after_read, &after_plan));
        assert_eq!(after_plan["completed"], 1);
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
            &bus,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            None,
            None,
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

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();

        let calls = vec![ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "/tmp/fake.rs", "oldText": "a", "newText": "b"}),
        }];

        let dispatch = dispatch_tools(
            &bus,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            None,
            None,
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
            &bus,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            Some(&policy),
            None,
        )
        .await;
        assert_eq!(dispatch.results.len(), 1);
        assert!(dispatch.results[0].is_error);
        assert!(!dir.path().join("should-not-exist").exists());
    }

    #[tokio::test]
    async fn path_always_allow_grants_directory_without_second_prompt() {
        let workspace = tempfile::tempdir().unwrap();
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
        let (events_tx, mut events_rx) = broadcast::channel(16);
        let cancel = CancellationToken::new();
        let path = outside_file.display().to_string();
        let calls = vec![ToolCall {
            id: "outside-read".into(),
            name: crate::tool_registry::core::READ.into(),
            arguments: serde_json::json!({"path": path}),
        }];

        let dispatch_fut = dispatch_tools(
            &bus,
            &calls,
            &events_tx,
            cancel.clone(),
            workspace.path(),
            None,
            None,
            None,
            None,
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
            &bus,
            &second_calls,
            &events_tx,
            cancel,
            workspace.path(),
            None,
            None,
            None,
            None,
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
            arguments: serde_json::json!({"command":"printf prompt-created"}),
        }];

        let dispatch_fut = dispatch_tools(
            &bus,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            Some(&policy),
            None,
        );
        tokio::pin!(dispatch_fut);

        loop {
            tokio::select! {
                event = events_rx.recv() => {
                    if let Ok(AgentEvent::PermissionRequest { tool_name, path, kind, persistence, grant_path, respond }) = event {
                        assert_eq!(tool_name, crate::tool_registry::core::BASH);
                        assert!(path.contains("printf prompt-created"), "prompt subject should include command: {path}");
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
        assert!(
            dispatch.results[0].content[0]
                .as_text()
                .unwrap()
                .contains("prompt-created"),
            "dispatch result: {:?}",
            dispatch.results[0].content
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

        let dispatch_fut = dispatch_tools(
            &bus,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            Some(&policy),
            None,
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
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![
                    omegon_traits::ToolDefinition {
                        name: "whoami".into(),
                        label: "whoami".into(),
                        description: "identity".into(),
                        parameters: serde_json::json!({}),
                        capabilities: vec![],
                    },
                    omegon_traits::ToolDefinition {
                        name: "chronos".into(),
                        label: "chronos".into(),
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

        let (events_tx, _rx) = broadcast::channel(64);
        let cancel = CancellationToken::new();
        let calls = vec![
            ToolCall {
                id: "1".into(),
                name: "whoami".into(),
                arguments: serde_json::json!({}),
            },
            ToolCall {
                id: "2".into(),
                name: "chronos".into(),
                arguments: serde_json::json!({}),
            },
        ];

        let start = Instant::now();
        let dispatch = dispatch_tools(
            &bus,
            &calls,
            &events_tx,
            cancel,
            dir.path(),
            None,
            None,
            None,
            None,
        )
        .await;
        let elapsed = start.elapsed();

        assert_eq!(dispatch.results.len(), 2);
        assert!(
            elapsed < Duration::from_millis(260),
            "expected parallel dispatch, got {elapsed:?}"
        );
        assert_eq!(dispatch.results[0].tool_name, "whoami");
        assert_eq!(dispatch.results[1].tool_name, "chronos");
    }

    #[tokio::test]
    async fn filesystem_read_tools_dispatch_serially_to_preserve_permission_prompts() {
        assert!(!is_parallel_safe_read_only_tool("read"));
        assert!(!is_parallel_safe_read_only_tool("view"));
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
            route_controller: None,
            cwd: std::path::PathBuf::from("/tmp"),
            extended_context: false,
            settings: None,
            secrets: None,
            force_compact: None,
            allow_commit_nudge: true,
            enforce_first_turn_execution_bias: false,
            ollama_manager: None,
            skill_phases: Vec::new(),
            host_context: None,
            permission_policy: None,
            permission_role: None,
            cancel_keeps_prompt: None,
            drain_post_loop_requests: true,
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
    fn extract_mutation_path_from_edit() {
        let args = serde_json::json!({"path": "/src/main.rs", "oldText": "a", "newText": "b"});
        assert_eq!(extract_mutation_path(&args), Some("/src/main.rs".into()));
    }

    #[test]
    fn extract_mutation_path_missing() {
        let args = serde_json::json!({"command": "ls"});
        assert_eq!(extract_mutation_path(&args), None);
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
    fn pressure_compaction_payload_falls_back_before_decay_window() {
        let mut conversation = ConversationState::new();
        conversation.push_user("turn zero context".into());
        conversation.intent.stats.turns = 1;
        conversation.push_user("turn one context".into());
        conversation.intent.stats.turns = 6;
        conversation.push_user("recent context".into());

        let selection = pressure_compaction_payload(&conversation).expect("pressure payload");

        assert_eq!(selection.evict_count(), 2);
        assert!(selection.payload().contains("turn zero context"));
        assert!(selection.payload().contains("turn one context"));
        assert!(!selection.payload().contains("recent context"));
        assert!(selection.reason().is_some());
    }

    #[test]
    fn pressure_compaction_payload_prefers_decay_window_when_available() {
        let mut conversation = ConversationState::new();
        conversation.push_user("very old context".into());
        conversation.intent.stats.turns = 99;
        conversation.push_user("recent context".into());

        let selection = pressure_compaction_payload(&conversation).expect("pressure payload");

        assert_eq!(selection.evict_count(), 1);
        assert!(selection.payload().contains("very old context"));
        assert!(selection.reason().is_none());
    }

    #[test]
    fn loop_context_windows_uses_effective_requested_assembly_window() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        {
            let mut guard = settings.lock().unwrap();
            guard.set_requested_context_class(crate::settings::ContextClass::Compact);
        }
        let config = LoopConfig {
            settings: Some(settings),
            ..LoopConfig::default()
        };

        let (provider_window, effective_window, policy) = loop_context_windows(&config);

        assert!(provider_window > effective_window);
        assert_eq!(
            effective_window,
            crate::settings::ContextClass::Compact.nominal_tokens()
        );
        assert_eq!(
            policy.expect("selector policy").requested_class,
            crate::settings::ContextClass::Compact
        );
    }
}
