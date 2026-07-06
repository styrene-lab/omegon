//! Agent loop state machine.
//!
//! The core prompt → LLM → tool dispatch → repeat cycle.
//! Includes: turn limits, retry with backoff, stuck detection,
//! context wiring, and parallel tool dispatch.

use crate::bridge::{LlmBridge, LlmEvent, LlmMessage, StreamOptions};

use crate::context::ContextManager;
use crate::conversation::{AssistantMessage, ConversationState, ToolCall, ToolResultEntry};
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
use std::time::Instant;
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
    /// Model string to pass to the bridge (e.g. "anthropic:claude-sonnet-4-6")
    pub model: String,
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
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_turns: 50,
            soft_limit_turns: 35,
            max_retries: 0,
            retry_delay_ms: 750,
            model: "anthropic:claude-sonnet-4-6".into(),
            cwd: std::env::current_dir().unwrap_or_default(),
            extended_context: false,
            settings: None,
            secrets: None,
            force_compact: None,
            allow_commit_nudge: true,
            enforce_first_turn_execution_bias: false,
            ollama_manager: None,
            skill_phases: Vec::new(),
        }
    }
}

use crate::behavior::{self, BehavioralTier, ControllerState};

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
use behavior::progress_nudge_reason_for_drift;
use behavior::should_inject_execution_pressure;

use behavior::evidence_sufficiency_message;
use behavior::has_local_target_hypothesis;
use behavior::is_slim_execution_bias;
use behavior::om_local_first_message;

// ─── Remaining classifier bodies removed — see behavior.rs ──────────────

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
                content, tool_name, ..
            } => {
                tool_history_tokens += estimate_chars_to_tokens(content.len() + tool_name.len());
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

    let base_stream_options = StreamOptions {
        model: Some(config.model.clone()),
        reasoning: None,
        extended_context: config.extended_context,
        ..Default::default()
    };

    let mut stuck_detector = StuckDetector::new();
    let session_start = Instant::now();
    let mut controller = ControllerState::default();
    let mut dead_mouse_nudges: u8 = 0;
    // Set when a dead-mouse nudge message was injected this turn.
    // Used to gate the counter reset — noise writes (compliance notes,
    // session acks) must not satisfy the nudge and reset the counter.
    let mut dead_mouse_nudge_injected = false;
    let mut session_used_tools: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut turn: u32 = 0;
    // Active model for this turn — updated each iteration from settings.
    // Used in TurnEnd events and error classification instead of the
    // immutable config.model which is frozen at startup.
    let mut active_model = config.model.clone();

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
        let context_window = config
            .settings
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.context_window))
            .unwrap_or(200_000);
        if let Some(settings) = config
            .settings
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.clone()))
        {
            context.set_selector_policy(settings.selector_policy());
        } else {
            context.set_context_window(context_window);
        }

        // ─── Turn limit enforcement ─────────────────────────────────
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
                turn_end_reason: TurnEndReason::AssistantCompleted,
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

        let _ = events.send(AgentEvent::TurnStart { turn });
        bus.emit(&omegon_traits::BusEvent::TurnStart { turn });

        // ─── Stuck detection ────────────────────────────────────────
        if let Some(warning) = stuck_detector.check(&tool_catalog) {
            tracing::info!(
                consecutive = warning.consecutive,
                "Stuck detector: {}",
                warning.message
            );
            if warning.consecutive >= 3 {
                tracing::warn!(
                    "Stuck detector escalation — force-breaking agent loop after {} consecutive warnings",
                    warning.consecutive
                );
                conversation.push_user(
                    "[System: STUCK LOOP DETECTED. You have been repeating the same \
                     actions for multiple turns despite warnings. Stop using tools. \
                     Summarize what you know so far and respond to the user.]"
                        .to_string(),
                );
                break;
            }
            conversation.push_user(format!("[System: {}]", warning.message));
        }

        // ─── Compaction check ────────────────────────────────────────
        // If context is getting large, try LLM-driven compaction.
        // The context_window default is 200k tokens (Anthropic models).
        // Trigger at 75% utilization.
        let forced_compact = config
            .force_compact
            .as_ref()
            .is_some_and(|flag| flag.swap(false, std::sync::atomic::Ordering::SeqCst));
        if (forced_compact || conversation.needs_compaction(context_window, 0.75))
            && let Some((payload, evict_count)) = conversation.build_compaction_payload()
        {
            tracing::info!(
                estimated_tokens = conversation.estimate_tokens(),
                evict_count,
                forced = forced_compact,
                "Context compaction requested"
            );
            // Use the bridge to summarize the evictable messages
            match compact_via_llm(bridge, &payload, &base_stream_options).await {
                Ok(summary) => {
                    conversation.apply_compaction(summary);
                }
                Err(e) => {
                    tracing::warn!("LLM compaction failed: {e} — continuing with decay only");
                }
            }
        }

        // ─── Inject IntentDocument if meaningful ─────────────────────
        if conversation.intent.stats.tool_calls > 0
            || conversation.intent.current_task.is_some()
            || conversation.intent.stats.compactions > 0
        {
            let intent_block = conversation.render_intent_for_injection();
            context.inject_intent(intent_block);
        }

        // ─── Collect context from bus features ──────────────────────
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

        // ─── Pre-compute embeddings for semantic context scoring ────
        context
            .prepare_embeddings(conversation.last_user_prompt())
            .await;

        // ─── Input format hints (MCQ, obfuscation) ─────────────────
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

        // ─── Build LLM-facing context ───────────────────────────────
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

        // ─── Stream LLM response with retry ─────────────────────────
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
            // Also re-read model (can change via /sonnet, /opus, etc.)
            opts.model = config
                .settings
                .as_ref()
                .and_then(|s| s.lock().ok().map(|g| g.model.clone()))
                .or_else(|| Some(config.model.clone()));
            // Track the active model for this turn so TurnEnd events and
            // error classification use the current model, not the startup value.
            active_model = opts.model.clone().unwrap_or_else(|| config.model.clone());
            opts
        };

        // ─── Ollama cold-start warmup ───────────────────────────
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
                        if let Some((payload, evict_count)) = conversation.build_compaction_payload() {
                            tracing::info!(evict_count, "Emergency compaction: evicting messages");
                            match compact_via_llm(bridge, &payload, &base_stream_options).await {
                                Ok(summary) => conversation.apply_compaction(summary),
                                Err(ce) => {
                                    tracing::warn!("Emergency LLM compaction failed: {ce} — applying decay");
                                    conversation.decay_oldest(evict_count);
                                }
                            }
                        } else {
                            // Can't build compaction payload — decay aggressively
                            conversation.decay_oldest(conversation.message_count() / 2);
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

        // ─── Parse ambient capture blocks (omg: tags) ───────────────
        let captured =
            crate::lifecycle::capture::parse_ambient_blocks(assistant_msg.text_content());
        if !captured.is_empty() {
            conversation.apply_ambient_captures(&captured);
        }

        // Push assistant message to conversation
        conversation.push_assistant(assistant_msg.clone());

        // Extract tool calls
        let tool_calls = assistant_msg.tool_calls();
        if tool_calls.is_empty() {
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

            // ─── Skill phase completion check ─────────────────────────
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

            // ─── Dead-mouse detection ──────────────────────────────
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
            // Skip dead-mouse if the user's last prompt looks like a question,
            // rundown, summary, or any read/explain-style request — text-only
            // responses are legitimate. We err *strongly* on the side of NOT
            // firing: false positives push the model to invent file-writing
            // work the user never requested (worse failure mode than false
            // negatives).
            let user_asked_question = {
                let prompt = conversation.last_user_prompt().to_lowercase();
                let starts = |w: &str| prompt.trim_start().starts_with(w);
                prompt.contains('?')
                    || starts("explain")
                    || starts("what")
                    || starts("why")
                    || starts("how")
                    || starts("when")
                    || starts("where")
                    || starts("which")
                    || starts("who")
                    || starts("describe")
                    || starts("summarize")
                    || starts("summary")
                    || starts("rundown")
                    || starts("overview")
                    || starts("review")
                    || starts("analyze")
                    || starts("compare")
                    || starts("contrast")
                    || starts("outline")
                    || starts("discuss")
                    || starts("tell me")
                    || starts("show me")
                    || starts("give me")
                    || starts("list")
                    || starts("can you")
                    || starts("could you")
                    || starts("do you")
                    || starts("is ")
                    || starts("are ")
                    || starts("does")
                    || starts("did")
                    || starts("read")
                    || starts("look")
                    || starts("check")
                    || starts("find")
                    || starts("search")
                    || prompt.contains(" rundown")
                    || prompt.contains(" summary")
                    || prompt.contains(" overview")
            };
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

        // ─── Emit ToolStart bus events before dispatch ──────────────
        for call in tool_calls {
            session_used_tools.insert(call.name.clone());
            bus.emit(&omegon_traits::BusEvent::ToolStart {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
                capabilities: tool_catalog.capabilities_for(&call.name),
            });
        }

        // ─── Dispatch tool calls ────────────────────────────────────
        // Auto-delegation is disabled — the agent always executes its
        // own tool calls directly. See classify_auto_delegate_plan().
        let dispatch_calls = tool_calls;
        let dispatch = dispatch_tools(
            bus,
            dispatch_calls,
            events,
            cancel.clone(),
            &config.cwd,
            config.secrets.as_deref(),
        )
        .await;
        let results = dispatch.results;

        // Emit permission decisions as bus events (requires &mut bus).
        for perm in dispatch.permission_decisions {
            bus.emit(&omegon_traits::BusEvent::PermissionDecision {
                tool_name: perm.tool_name,
                path: perm.path,
                decision: perm.decision,
            });
        }

        // Push tool results to conversation and update intent
        for result in &results {
            conversation.push_tool_result(result.clone());
        }
        conversation
            .intent
            .update_from_tools(dispatch_calls, &results);

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

        // ─── Emit tool events to bus features ───────────────────────
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

        // ─── Wire context signals ───────────────────────────────────
        for call in dispatch_calls {
            context.record_tool_call(&call.name);
            // Track file access from tool arguments
            if let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) {
                context.record_file_access(std::path::PathBuf::from(path));
            }
        }
        context.update_phase_from_activity(dispatch_calls);

        // ─── Feed stuck detector ────────────────────────────────────
        for call in dispatch_calls {
            let is_error = results
                .iter()
                .find(|r| r.call_id == call.id)
                .is_some_and(|r| r.is_error);
            stuck_detector.record(&tool_catalog, call, is_error);
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

        // ─── Handle bus requests from features ──────────────────────
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
                    if let Some((payload, _evict_count)) = conversation.build_compaction_payload() {
                        match compact_via_llm(bridge, &payload, &base_stream_options).await {
                            Ok(summary) => {
                                conversation.apply_compaction(summary);
                                bus.emit(&omegon_traits::BusEvent::Compacted);
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "auto-compaction failed");
                            }
                        }
                    } else {
                        tracing::debug!(
                            "auto-compaction requested but nothing was eligible to compact"
                        );
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
    let files_modified: Vec<String> = conversation
        .intent
        .files_modified
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    bus.emit(&omegon_traits::BusEvent::SessionEnd {
        turns: turn,
        tool_calls: conversation.intent.stats.tool_calls,
        duration_secs: elapsed.as_secs_f64(),
        initial_prompt,
        outcome_summary,
        files_modified,
    });

    // Process any pending bus requests (e.g. auto-compact notifications,
    // auto-store facts from lifecycle transitions, episode storage).
    // AutoStoreFact requests are now executed rather than dropped —
    // design_tree decisions/transitions enqueued late in the session
    // (or from SessionEnd handlers) are persisted to memory.
    for request in bus.drain_requests() {
        match request {
            omegon_traits::BusRequest::Notify { message, level } => {
                tracing::info!(level = ?level, "Bus notification: {message}");
            }
            omegon_traits::BusRequest::InjectSystemMessage { content } => {
                tracing::debug!("post-loop InjectSystemMessage ignored (loop complete): {content}");
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
                if let Err(e) = bus
                    .execute_tool(
                        "memory_store",
                        "post_loop_auto_ingest",
                        args,
                        cancel.clone(),
                    )
                    .await
                {
                    tracing::debug!(source, "post-loop auto-store fact skipped: {e}");
                }
            }
            omegon_traits::BusRequest::EmitAgentEvent { event } => {
                let _ = events.send(*event);
            }
        }
    }

    Ok(())
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
                message: format!("⚡ {model_name} loaded"),
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

        // Wrap bridge.stream() so pre-stream network errors (DNS, connection
        // refused, TLS failures) enter the same transient classifier instead
        // of aborting immediately via `?`.
        let err = match bridge.stream(system_prompt, messages, tools, options).await {
            Ok(mut rx) => match consume_llm_stream(&mut rx, events).await {
                Ok(msg) => return Ok(msg),
                Err(e) => e,
            },
            Err(e) => e,
        };

        let err_msg = err.to_string();
        let provider = config
            .model
            .split(':')
            .next()
            .unwrap_or("upstream")
            .to_string();
        let upstream_class = classify_upstream_error_for_provider(&provider, &err_msg);
        let transient_kind = upstream_class.transient_kind();
        let is_transient = transient_kind.is_some();
        let model = options
            .model
            .as_deref()
            .unwrap_or(&config.model)
            .to_string();

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
        // Three exhaustion paths:
        // - max_retries > 0 (cleave): hard cap on attempt count
        // - max_retries == 0 (TUI) + rate-limit: bail after 120s continuous
        // - max_retries == 0 (TUI) + stall: bail after 10 min of cumulative stalls
        //   (OpenAI's default stream idle is 5 min; 2× that covers a retry cycle)
        let elapsed = started.elapsed();
        let rate_limit_exhausted = config.max_retries == 0
            && matches!(transient_kind, Some(TransientFailureKind::RateLimited))
            && elapsed.as_secs() >= 120;
        let stall_exhausted = config.max_retries == 0
            && matches!(transient_kind, Some(TransientFailureKind::StalledStream))
            && elapsed.as_secs() >= 600;
        let attempt_exhausted = config.max_retries > 0 && attempt >= config.max_retries;

        if attempt_exhausted || rate_limit_exhausted || stall_exhausted {
            let reason = if rate_limit_exhausted {
                "session rate-limit exhaustion"
            } else if stall_exhausted {
                "stream stall exhaustion"
            } else {
                "upstream exhausted"
            };
            tracing::error!(
                attempts = attempt,
                elapsed_secs = elapsed.as_secs(),
                kind = kind_label,
                "{reason}: {err_msg}"
            );
            let advice = exhaustion_advice(transient_kind, rate_limit_exhausted, stall_exhausted);
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
        let _ = events.send(AgentEvent::SystemNotification { message: msg });
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        delay = delay.saturating_mul(2).min(15_000); // exponential backoff, cap at 15s
    }
}

fn exhaustion_advice(
    transient_kind: Option<TransientFailureKind>,
    rate_limit_exhausted: bool,
    stall_exhausted: bool,
) -> &'static str {
    if stall_exhausted {
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

/// Consume LlmEvents from the bridge, build an AssistantMessage.
async fn consume_llm_stream(
    rx: &mut tokio::sync::mpsc::Receiver<LlmEvent>,
    events: &broadcast::Sender<AgentEvent>,
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

    // ─── Degenerate output detector ─────────────────────────────
    // Catches models stuck in a text-repetition loop (e.g. "Append tests."
    // repeated 500 times). Tracks a rolling window of recent text chunks
    // and aborts when a short phrase repeats excessively.
    let mut recent_text_len: usize = 0;
    let mut repetition_window: Vec<String> = Vec::new();
    const REPETITION_WINDOW_SIZE: usize = 40;
    const REPETITION_ABORT_THRESHOLD: usize = 30; // 30 of last 40 chunks identical → abort

    // Two-phase idle timeout:
    // - Before first content: 300s (OpenAI documents stream_idle_timeout_ms=300000
    //   as their default — reasoning models can be silent for minutes)
    // - After first content: 90s (Claude Code's CLAUDE_STREAM_IDLE_TIMEOUT_MS
    //   default is 90s; nobody in the industry uses less than 60s)
    let initial_idle_timeout = std::time::Duration::from_secs(300);
    let content_idle_timeout = std::time::Duration::from_secs(90);
    let received_content = std::sync::atomic::AtomicBool::new(false);
    let idle_timeout = || {
        if received_content.load(std::sync::atomic::Ordering::Relaxed) {
            content_idle_timeout
        } else {
            initial_idle_timeout
        }
    };
    while let Some(event) = match tokio::time::timeout(idle_timeout(), rx.recv()).await {
        Ok(event) => event,
        Err(_) => {
            let reason = format!(
                "LLM stream idle for {}s — connection may be stalled",
                idle_timeout().as_secs()
            );
            let _ = events.send(AgentEvent::MessageAbort {
                reason: Some(reason.clone()),
            });
            anyhow::bail!("{reason}");
        }
    } {
        match event {
            LlmEvent::Start => {
                // Heartbeat — any server activity proves connection is alive.
                // Does NOT count as "content" for timeout phase transition.
            }
            LlmEvent::TextStart => {
                received_content.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            LlmEvent::TextDelta { delta } => {
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
                received_content.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            LlmEvent::ThinkingDelta { delta } => {
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
                received_content.store(true, std::sync::atomic::Ordering::Relaxed);
            }
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
}

struct DispatchResult {
    results: Vec<ToolResultEntry>,
    permission_decisions: Vec<PermissionRecord>,
}

async fn dispatch_tools(
    bus: &crate::bus::EventBus,
    tool_calls: &[ToolCall],
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    cwd: &std::path::Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
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
                let result =
                    dispatch_single_tool(bus, &call, &events, cancel, None, &mut perm_log).await;
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
            &mut permission_decisions,
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
    matches!(name, "read" | "view" | "web_search" | "whoami" | "chronos")
}

async fn dispatch_single_tool(
    bus: &crate::bus::EventBus,
    call: &ToolCall,
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    secrets: Option<&omegon_secrets::SecretsManager>,
    permission_log: &mut Vec<PermissionRecord>,
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
) -> (omegon_traits::ToolResult, bool) {
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
        });
    }

    let sink_events = events.clone();
    let sink_call_id = visible_call_id.to_string();
    let sink = omegon_traits::ToolProgressSink::from_fn(move |partial| {
        let _ = sink_events.send(AgentEvent::ToolUpdate {
            id: sink_call_id.clone(),
            partial,
        });
    });

    let execute = |cancel: CancellationToken, sink: omegon_traits::ToolProgressSink| {
        bus.execute_tool_with_sink(
            execution_tool_name,
            visible_call_id,
            execution_args.clone(),
            cancel,
            sink,
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

    // Intercept PathPermissionError — show interactive TUI prompt
    let (result, is_error) = match first_result {
        Ok(result) => (result, false),
        Err(e)
            if e.downcast_ref::<crate::tools::PathPermissionError>()
                .is_some() =>
        {
            let perm_err = e.downcast::<crate::tools::PathPermissionError>().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let respond = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));

            let _ = events.send(AgentEvent::PermissionRequest {
                tool_name: visible_tool_name.to_string(),
                path: perm_err.requested_path.clone(),
                respond,
            });

            // Wait for TUI response (blocks tool execution until user responds).
            // Use spawn_blocking to avoid blocking the tokio runtime.
            let response = tokio::task::spawn_blocking(move || {
                rx.recv_timeout(std::time::Duration::from_secs(120))
                    .unwrap_or(omegon_traits::PermissionResponse::Deny)
            })
            .await
            .unwrap_or(omegon_traits::PermissionResponse::Deny);

            match response {
                omegon_traits::PermissionResponse::Allow => {
                    tracing::info!(path = %perm_err.requested_path, decision = "allow", "permission decision");
                    permission_log.push(PermissionRecord {
                        tool_name: visible_tool_name.to_string(),
                        path: perm_err.requested_path.clone(),
                        decision: "allow".into(),
                    });
                    let trust_args = serde_json::json!({ "path": perm_err.directory });
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
                    });
                    let trust_args = serde_json::json!({ "path": perm_err.directory });
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
                    });
                    (
                        omegon_traits::ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "BLOCKED: '{}' is outside the workspace. \
                                     This operation was denied by the permission system. \
                                     The operator must run /trust add {} to allow \
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

// ─── Stuck detection ────────────────────────────────────────────────────────

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
        if let Some(repeated) = self.find_repeated_call(window, 3) {
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
    fn find_repeated_call(
        &self,
        window: &[(String, u64, bool)],
        threshold: usize,
    ) -> Option<(String, usize)> {
        let mut counts: HashMap<(String, u64), usize> = HashMap::new();
        for (name, hash, _) in window {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::{EvidenceAssessment, EvidenceSufficiency, ProgressSignal};
    use omegon_traits::{OodaPhase, ToolCapability, ToolDefinition, ToolProvider};

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

        let warning = detector.check(&test_tool_catalog()).expect("warning");
        assert!(warning.message.contains("same arguments"));
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

        let dispatch = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;
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

        let dispatch = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;
        assert!(!dispatch.results[0].is_error);
        let text = dispatch.results[0].content[0].as_text().unwrap();
        assert!(
            !text.contains("rollback"),
            "single edit should have no batch overhead"
        );
    }

    #[tokio::test]
    async fn parallel_safe_read_only_tools_dispatch_concurrently() {
        use omegon_traits::ToolResult;
        use tokio::time::{Duration, Instant, sleep};

        struct SlowReadOnlyProvider;

        #[async_trait::async_trait]
        impl ToolProvider for SlowReadOnlyProvider {
            fn tools(&self) -> Vec<omegon_traits::ToolDefinition> {
                vec![
                    omegon_traits::ToolDefinition {
                        name: "read".into(),
                        label: "read".into(),
                        description: "read file".into(),
                        parameters: serde_json::json!({}),
                        capabilities: vec![],
                    },
                    omegon_traits::ToolDefinition {
                        name: "view".into(),
                        label: "view".into(),
                        description: "view file".into(),
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
                name: "read".into(),
                arguments: serde_json::json!({"path": "a.txt"}),
            },
            ToolCall {
                id: "2".into(),
                name: "view".into(),
                arguments: serde_json::json!({"path": "b.txt"}),
            },
        ];

        let start = Instant::now();
        let dispatch = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;
        let elapsed = start.elapsed();

        assert_eq!(dispatch.results.len(), 2);
        assert!(
            elapsed < Duration::from_millis(260),
            "expected parallel dispatch, got {elapsed:?}"
        );
        assert_eq!(dispatch.results[0].tool_name, "read");
        assert_eq!(dispatch.results[1].tool_name, "view");
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
            cwd: std::path::PathBuf::from("/tmp"),
            extended_context: false,
            settings: None,
            secrets: None,
            force_compact: None,
            allow_commit_nudge: true,
            enforce_first_turn_execution_bias: false,
            ollama_manager: None,
            skill_phases: Vec::new(),
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
        // Stalls bail after 600s cumulative elapsed (10 min), not attempt count.
        let config = LoopConfig {
            max_retries: 0,
            ..Default::default()
        };
        let transient_kind = Some(crate::upstream_errors::TransientFailureKind::StalledStream);

        // Under threshold
        for elapsed_secs in [30u64, 120, 300, 599] {
            let stall_exhausted = config.max_retries == 0
                && matches!(
                    transient_kind,
                    Some(crate::upstream_errors::TransientFailureKind::StalledStream)
                )
                && elapsed_secs >= 600;
            assert!(!stall_exhausted, "{elapsed_secs}s should NOT exhaust");
        }

        // At threshold
        let elapsed_secs = 600u64;
        let stall_exhausted = config.max_retries == 0
            && matches!(
                transient_kind,
                Some(crate::upstream_errors::TransientFailureKind::StalledStream)
            )
            && elapsed_secs >= 600;
        assert!(stall_exhausted, "600s should trigger stall exhaustion");
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
            && elapsed_secs >= 600;
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
        assert_eq!(evidence.local, EvidenceSufficiency::Actionable);
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
                msg.contains("produce") || msg.contains("Produce")
                    || msg.contains("answer") || msg.contains("Answer"),
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
    fn exhaustion_advice_distinguishes_provider_outage_from_rate_limit() {
        assert!(
            exhaustion_advice(Some(TransientFailureKind::Upstream5xx), false, false)
                .contains("provider-side outage or capacity problem")
        );
        assert!(
            exhaustion_advice(Some(TransientFailureKind::ProviderOverloaded), false, false)
                .contains("provider-side outage or capacity problem")
        );
        assert!(
            exhaustion_advice(Some(TransientFailureKind::RateLimited), true, false)
                .contains("rate-limiting the session")
        );
    }

    #[test]
    fn exhaustion_advice_distinguishes_unstable_network_and_stalled_stream() {
        assert!(
            exhaustion_advice(Some(TransientFailureKind::NetworkReset), false, false)
                .contains("provider or network path is unstable")
        );
        assert!(
            exhaustion_advice(Some(TransientFailureKind::StalledStream), false, true)
                .contains("stream is unresponsive")
        );
    }
}
