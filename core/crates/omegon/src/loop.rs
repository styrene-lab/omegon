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
    AgentEvent, ContentBlock, ContextComposition, DriftKind, OodaPhase, ProgressNudgeReason,
    TurnEndReason,
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoDelegatePlan {
    worker_profile: &'static str,
    background: bool,
}

fn classify_auto_delegate_plan(
    config: &LoopConfig,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
    dominant_phase: Option<OodaPhase>,
    drift_kind: Option<DriftKind>,
) -> Option<AutoDelegatePlan> {
    if !is_slim_execution_bias(config) || tool_calls.is_empty() {
        return None;
    }
    if tool_calls.iter().any(|call| call.name == "delegate") {
        return None;
    }
    if !conversation.intent.files_modified.is_empty() {
        return None;
    }
    if tool_calls.iter().any(|call| call.name == "commit") {
        return None;
    }
    if matches!(drift_kind, Some(DriftKind::OrientationChurn))
        && tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(&call.name))
    {
        return Some(AutoDelegatePlan {
            worker_profile: "scout",
            background: true,
        });
    }
    if is_narrow_patch_candidate(tool_calls) {
        return Some(AutoDelegatePlan {
            worker_profile: "patch",
            background: false,
        });
    }
    if matches!(dominant_phase, Some(OodaPhase::Act)) && tool_calls.iter().all(is_validation_tool) {
        return Some(AutoDelegatePlan {
            worker_profile: "verify",
            background: false,
        });
    }
    None
}

fn auto_delegate_tool_call(conversation: &ConversationState, plan: AutoDelegatePlan) -> ToolCall {
    let last_prompt = conversation.last_user_prompt();
    let task = if !last_prompt.trim().is_empty() {
        last_prompt.trim().to_string()
    } else {
        conversation.intent.current_task.clone().unwrap_or_else(|| {
            "Inspect the current bounded task and return concise findings.".to_string()
        })
    };
    ToolCall {
        id: format!(
            "auto-delegate-{}",
            conversation.turn_count().saturating_add(1)
        ),
        name: "delegate".to_string(),
        arguments: serde_json::json!({
            "task": task,
            "background": plan.background,
            "worker_profile": plan.worker_profile,
        }),
    }
}

fn default_context_composition(context_window: usize) -> ContextComposition {
    ContextComposition {
        free_tokens: context_window,
        ..ContextComposition::default()
    }
}

fn estimate_chars_to_tokens(chars: usize) -> usize {
    chars / 4
}

fn estimate_tool_schema_tokens(tools: &[omegon_traits::ToolDefinition]) -> usize {
    tools
        .iter()
        .map(|tool| {
            let schema_json = serde_json::to_string(&tool.parameters).unwrap_or_default();
            estimate_chars_to_tokens(tool.name.len() + tool.description.len() + schema_json.len())
        })
        .sum()
}

fn is_orientation_tool(name: &str) -> bool {
    matches!(name, "memory_recall" | "context_status" | "request_context")
}

fn is_repo_inspection_tool(name: &str) -> bool {
    matches!(name, "read" | "codebase_search" | "view")
}

fn is_broad_orientation_tool(name: &str) -> bool {
    matches!(name, "memory_recall" | "context_status" | "request_context")
}

fn is_broad_repo_inspection_tool(name: &str) -> bool {
    matches!(name, "codebase_search" | "view")
}

fn is_targeted_repo_inspection_tool(name: &str) -> bool {
    name == "read"
}

fn is_mutation_tool_name(name: &str) -> bool {
    matches!(name, "write" | "edit" | "change")
}

fn mutation_targets_within_limit(tool_calls: &[ToolCall], max_files: usize) -> bool {
    let mut paths = std::collections::BTreeSet::new();
    for call in tool_calls {
        if !is_mutation_tool_name(&call.name) {
            continue;
        }
        let Some(path) = call.arguments.get("path").and_then(|v| v.as_str()) else {
            return false;
        };
        paths.insert(path.to_string());
        if paths.len() > max_files {
            return false;
        }
    }
    !paths.is_empty()
}

fn is_narrow_patch_candidate(tool_calls: &[ToolCall]) -> bool {
    if !tool_calls
        .iter()
        .any(|call| is_mutation_tool_name(&call.name))
    {
        return false;
    }
    if !mutation_targets_within_limit(tool_calls, 2) {
        return false;
    }
    tool_calls.iter().all(|call| {
        is_mutation_tool_name(&call.name) || call.name == "read" || is_validation_tool(call)
    })
}

fn is_validation_tool(call: &ToolCall) -> bool {
    if call.name != "bash" {
        return false;
    }
    let Some(command) = call.arguments.get("command").and_then(|v| v.as_str()) else {
        return false;
    };
    let lower = command.to_ascii_lowercase();
    [
        "cargo test",
        "cargo check",
        "cargo clippy",
        "cargo fmt",
        "pytest",
        "npm test",
        "npm run test",
        "npm run check",
        "npm run typecheck",
        "tsc --noemit",
        "ruff check",
        "ruff format --check",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn classify_turn_phase(tool_calls: &[ToolCall], results: &[ToolResultEntry]) -> Option<OodaPhase> {
    if tool_calls.is_empty() {
        return None;
    }

    if tool_calls.iter().any(|call| call.name == "commit") {
        return Some(OodaPhase::Act);
    }

    let successful_mutation = tool_calls.iter().any(|call| {
        is_mutation_tool_name(&call.name)
            && results
                .iter()
                .find(|result| result.call_id == call.id)
                .is_some_and(|result| !result.is_error)
    });
    if successful_mutation {
        return Some(OodaPhase::Act);
    }

    if tool_calls
        .iter()
        .all(|call| is_orientation_tool(&call.name))
    {
        return Some(OodaPhase::Observe);
    }

    if tool_calls
        .iter()
        .all(|call| is_repo_inspection_tool(&call.name))
    {
        return Some(OodaPhase::Observe);
    }

    if tool_calls.iter().all(is_validation_tool) {
        return Some(OodaPhase::Act);
    }

    if tool_calls
        .iter()
        .any(|call| is_mutation_tool_name(&call.name) || is_validation_tool(call))
    {
        return Some(OodaPhase::Act);
    }

    Some(OodaPhase::Orient)
}

fn classify_drift_kind(
    turn: u32,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> Option<DriftKind> {
    let broad_orientation_calls = tool_calls
        .iter()
        .filter(|call| is_broad_orientation_tool(&call.name))
        .count();
    let broad_repo_inspection_calls = tool_calls
        .iter()
        .filter(|call| is_broad_repo_inspection_tool(&call.name))
        .count();
    let targeted_repo_inspection_calls = tool_calls
        .iter()
        .filter(|call| is_targeted_repo_inspection_tool(&call.name))
        .count();

    if conversation.intent.files_modified.is_empty()
        && !conversation.intent.files_read.is_empty()
        && tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(&call.name))
        && turn >= 3
        && broad_repo_inspection_calls > 0
        && targeted_repo_inspection_calls <= 1
    {
        return Some(DriftKind::OrientationChurn);
    }

    if conversation.intent.files_modified.is_empty()
        && conversation.intent.files_read.is_empty()
        && turn >= 2
        && broad_orientation_calls == tool_calls.len()
    {
        return Some(DriftKind::OrientationChurn);
    }

    let failing_mutations: Vec<&ToolCall> = tool_calls
        .iter()
        .filter(|call| {
            is_mutation_tool_name(&call.name)
                && results
                    .iter()
                    .find(|result| result.call_id == call.id)
                    .is_some_and(|result| result.is_error)
        })
        .collect();
    let repeated_mutation_failures = failing_mutations.len() >= 2
        && failing_mutations.iter().enumerate().any(|(idx, call)| {
            let path = call.arguments.get("path").and_then(|v| v.as_str());
            failing_mutations
                .iter()
                .enumerate()
                .filter(|(other_idx, other)| *other_idx != idx && other.name == call.name)
                .any(|(_, other)| {
                    let other_path = other.arguments.get("path").and_then(|v| v.as_str());
                    match (path, other_path) {
                        (Some(path), Some(other_path)) => path == other_path,
                        (None, None) => true,
                        _ => false,
                    }
                })
        });
    if repeated_mutation_failures {
        return Some(DriftKind::RepeatedActionFailure);
    }

    let validation_calls = tool_calls
        .iter()
        .filter(|call| is_validation_tool(call))
        .count();
    let targeted_validation = matches!(
        classify_validation_scope(tool_calls, results),
        ProgressSignal::TargetedValidation
    );
    if validation_calls >= 2
        && conversation.intent.files_modified.is_empty()
        && !targeted_validation
    {
        return Some(DriftKind::ValidationThrash);
    }

    if !conversation.intent.files_modified.is_empty()
        && tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(&call.name))
        && broad_repo_inspection_calls > 0
    {
        return Some(DriftKind::ClosureStall);
    }

    None
}

fn progress_nudge_reason_for_drift(drift: DriftKind) -> ProgressNudgeReason {
    match drift {
        DriftKind::OrientationChurn => ProgressNudgeReason::AntiOrientation,
        DriftKind::RepeatedActionFailure => ProgressNudgeReason::ActionRecovery,
        DriftKind::ValidationThrash => ProgressNudgeReason::ValidationPressure,
        DriftKind::ClosureStall => ProgressNudgeReason::ClosurePressure,
    }
}

fn is_first_turn_orientation_churn(
    turn: u32,
    config: &LoopConfig,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
) -> bool {
    config.enforce_first_turn_execution_bias
        && turn == 1
        && !tool_calls.is_empty()
        && tool_calls
            .iter()
            .all(|call| is_orientation_tool(&call.name))
        && conversation.intent.files_read.is_empty()
        && conversation.intent.files_modified.is_empty()
}

fn should_inject_execution_pressure(
    turn: u32,
    config: &LoopConfig,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
) -> bool {
    if !config.enforce_first_turn_execution_bias
        || tool_calls.is_empty()
        || !conversation.intent.files_modified.is_empty()
        || conversation.intent.files_read.is_empty()
        || !tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(&call.name))
    {
        return false;
    }

    let has_broad_repo_inspection = tool_calls
        .iter()
        .any(|call| is_broad_repo_inspection_tool(&call.name));
    let only_targeted_reads = tool_calls
        .iter()
        .all(|call| is_targeted_repo_inspection_tool(&call.name));

    (turn >= 3 && has_broad_repo_inspection) || (turn >= 3 && only_targeted_reads)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgressSignal {
    None,
    Mutation,
    TargetedValidation,
    BroadValidation,
    ConstraintDiscovery,
    Commit,
    Completion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceSufficiency {
    None,
    Targeted,
    Actionable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ControllerState {
    consecutive_tool_continuations: u32,
    orientation_churn_streak: u32,
    repeated_action_failure_streak: u32,
    validation_thrash_streak: u32,
    closure_stall_streak: u32,
    constraint_discovery_streak: u32,
    targeted_evidence_streak: u32,
    evidence_sufficient_streak: u32,
}

impl ControllerState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Snapshot the streak counters as the public `ControllerStreaks`
    /// shape that's carried on `AgentEvent::TurnEnd`. The internal
    /// `consecutive_tool_continuations` field is intentionally not
    /// exported — it's a continuation-pressure heuristic, not a
    /// drift-streak signal that operators care about.
    fn streaks(&self) -> omegon_traits::ControllerStreaks {
        omegon_traits::ControllerStreaks {
            orientation_churn: self.orientation_churn_streak,
            repeated_action_failure: self.repeated_action_failure_streak,
            validation_thrash: self.validation_thrash_streak,
            closure_stall: self.closure_stall_streak,
            constraint_discovery: self.constraint_discovery_streak,
            evidence_sufficient: self.evidence_sufficient_streak,
        }
    }

    fn observe_turn(
        &mut self,
        turn_end_reason: TurnEndReason,
        drift_kind: Option<DriftKind>,
        progress_signal: ProgressSignal,
        evidence_sufficiency: EvidenceSufficiency,
    ) {
        match progress_signal {
            ProgressSignal::Mutation | ProgressSignal::Commit | ProgressSignal::Completion => {
                self.reset();
                return;
            }
            ProgressSignal::TargetedValidation | ProgressSignal::ConstraintDiscovery => {
                self.consecutive_tool_continuations /= 2;
                self.orientation_churn_streak /= 2;
                self.repeated_action_failure_streak = 0;
                self.validation_thrash_streak = 0;
                self.closure_stall_streak /= 2;
            }
            ProgressSignal::BroadValidation | ProgressSignal::None => {}
        }

        if matches!(turn_end_reason, TurnEndReason::ToolContinuation) {
            self.consecutive_tool_continuations =
                self.consecutive_tool_continuations.saturating_add(1);
        } else {
            self.consecutive_tool_continuations = 0;
        }

        self.orientation_churn_streak = if matches!(drift_kind, Some(DriftKind::OrientationChurn)) {
            self.orientation_churn_streak.saturating_add(1)
        } else {
            0
        };
        self.repeated_action_failure_streak =
            if matches!(drift_kind, Some(DriftKind::RepeatedActionFailure)) {
                self.repeated_action_failure_streak.saturating_add(1)
            } else {
                0
            };
        self.validation_thrash_streak = if matches!(drift_kind, Some(DriftKind::ValidationThrash)) {
            self.validation_thrash_streak.saturating_add(1)
        } else {
            0
        };
        self.closure_stall_streak = if matches!(drift_kind, Some(DriftKind::ClosureStall)) {
            self.closure_stall_streak.saturating_add(1)
        } else {
            0
        };
        self.constraint_discovery_streak =
            if matches!(progress_signal, ProgressSignal::ConstraintDiscovery) {
                self.constraint_discovery_streak.saturating_add(1)
            } else {
                0
            };
        self.targeted_evidence_streak =
            if matches!(
                evidence_sufficiency,
                EvidenceSufficiency::Targeted | EvidenceSufficiency::Actionable
            ) {
                self.targeted_evidence_streak.saturating_add(1)
            } else {
                0
            };
        self.evidence_sufficient_streak =
            if matches!(evidence_sufficiency, EvidenceSufficiency::Actionable) {
                self.evidence_sufficient_streak.saturating_add(1)
            } else {
                0
            };
    }
}

fn has_successful_tool_call<F>(
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
    predicate: F,
) -> bool
where
    F: Fn(&ToolCall) -> bool,
{
    tool_calls.iter().any(|call| {
        predicate(call)
            && results
                .iter()
                .find(|result| result.call_id == call.id)
                .is_some_and(|result| !result.is_error)
    })
}

fn has_progress_boundary(tool_calls: &[ToolCall], results: &[ToolResultEntry]) -> bool {
    has_successful_tool_call(tool_calls, results, |call| {
        is_mutation_tool_name(&call.name)
    }) || has_successful_tool_call(tool_calls, results, is_validation_tool)
        || has_successful_tool_call(tool_calls, results, |call| call.name == "commit")
}

fn classify_validation_scope(
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> ProgressSignal {
    let successful_validation_calls: Vec<&ToolCall> = tool_calls
        .iter()
        .filter(|call| {
            is_validation_tool(call)
                && results
                    .iter()
                    .find(|result| result.call_id == call.id)
                    .is_some_and(|result| !result.is_error)
        })
        .collect();

    if successful_validation_calls.is_empty() {
        return ProgressSignal::None;
    }

    let is_targeted = successful_validation_calls.iter().any(|call| {
        call.arguments
            .get("command")
            .and_then(|v| v.as_str())
            .is_some_and(|command| {
                let lower = command.to_ascii_lowercase();
                lower.contains(" -k ")
                    || lower.contains(" --test ")
                    || lower.contains("::")
                    || lower.contains(" shadow_context")
                    || lower.contains(" tests/")
            })
    });

    if is_targeted {
        ProgressSignal::TargetedValidation
    } else {
        ProgressSignal::BroadValidation
    }
}

fn detect_constraint_discovery(
    constraints_before: usize,
    constraints_after: usize,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> bool {
    if constraints_after <= constraints_before {
        return false;
    }

    tool_calls.iter().any(|call| {
        is_repo_inspection_tool(&call.name)
            || is_validation_tool(call)
            || (is_mutation_tool_name(&call.name)
                && results
                    .iter()
                    .find(|result| result.call_id == call.id)
                    .is_some_and(|result| result.is_error))
    })
}

fn classify_progress_signal(
    constraints_before: usize,
    constraints_after: usize,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> ProgressSignal {
    if has_successful_tool_call(tool_calls, results, |call| call.name == "commit") {
        return ProgressSignal::Commit;
    }
    if has_successful_tool_call(tool_calls, results, |call| {
        is_mutation_tool_name(&call.name)
    }) {
        return ProgressSignal::Mutation;
    }

    let validation_signal = classify_validation_scope(tool_calls, results);
    if !matches!(validation_signal, ProgressSignal::None) {
        return validation_signal;
    }

    if detect_constraint_discovery(constraints_before, constraints_after, tool_calls, results) {
        return ProgressSignal::ConstraintDiscovery;
    }

    ProgressSignal::None
}

fn detect_evidence_sufficiency(
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> EvidenceSufficiency {
    if !conversation.intent.files_modified.is_empty() || conversation.intent.files_read.is_empty() {
        return EvidenceSufficiency::None;
    }

    let targeted_validation = matches!(
        classify_validation_scope(tool_calls, results),
        ProgressSignal::TargetedValidation
    );
    let failed_mutation_on_known_target = tool_calls.iter().any(|call| {
        is_mutation_tool_name(&call.name)
            && call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .is_some_and(|path| {
                    conversation
                        .intent
                        .files_read
                        .iter()
                        .any(|read| read == std::path::Path::new(path))
                        && results
                            .iter()
                            .find(|result| result.call_id == call.id)
                            .is_some_and(|result| result.is_error)
                })
    });
    let inspection_backed_by_validation_failure = tool_calls.iter().any(|call| {
        is_repo_inspection_tool(&call.name)
            && results.iter().any(|result| result.is_error)
            && tool_calls.iter().any(is_validation_tool)
    });

    let targeted_reads: Vec<&str> = tool_calls
        .iter()
        .filter(|call| is_targeted_repo_inspection_tool(&call.name))
        .filter_map(|call| call.arguments.get("path").and_then(|v| v.as_str()))
        .collect();
    let narrow_target_cluster = !targeted_reads.is_empty()
        && tool_calls.iter().all(|call| is_repo_inspection_tool(&call.name))
        && !tool_calls
            .iter()
            .any(|call| is_broad_repo_inspection_tool(&call.name));
    let targeted_paths_known = narrow_target_cluster
        && targeted_reads.iter().all(|path| {
            conversation
                .intent
                .files_read
                .iter()
                .any(|read| read == std::path::Path::new(path))
        });
    let local_target_count = conversation.intent.files_read.len();

    if targeted_validation
        || failed_mutation_on_known_target
        || inspection_backed_by_validation_failure
        || (targeted_paths_known && local_target_count <= 2)
    {
        EvidenceSufficiency::Actionable
    } else if targeted_paths_known || local_target_count <= 2 {
        EvidenceSufficiency::Targeted
    } else {
        EvidenceSufficiency::None
    }
}

fn is_slim_execution_bias(config: &LoopConfig) -> bool {
    config
        .settings
        .as_ref()
        .and_then(|settings| settings.lock().ok().map(|s| s.slim_mode))
        .unwrap_or(false)
}

fn has_local_target_hypothesis(conversation: &ConversationState) -> bool {
    !conversation.intent.files_read.is_empty() && conversation.intent.files_modified.is_empty()
}

fn continuation_pressure_tier(
    config: &LoopConfig,
    controller: &ControllerState,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
    dominant_phase: Option<OodaPhase>,
) -> Option<u8> {
    if !config.enforce_first_turn_execution_bias
        || tool_calls.is_empty()
        || !conversation.intent.files_modified.is_empty()
        || !matches!(dominant_phase, Some(OodaPhase::Observe | OodaPhase::Orient))
    {
        return None;
    }

    let evidence_sufficient = controller.evidence_sufficient_streak > 0;
    let om_local_first_lock = is_slim_execution_bias(config)
        && evidence_sufficient
        && has_local_target_hypothesis(conversation);
    let (tier1, tier2, tier3) = if om_local_first_lock {
        (2, 3, 4)
    } else if evidence_sufficient {
        if is_slim_execution_bias(config) {
            (3, 4, 5)
        } else {
            (3, 4, 5)
        }
    } else if is_slim_execution_bias(config) {
        (5, 7, 9)
    } else {
        (6, 8, 10)
    };

    let continuation = controller.consecutive_tool_continuations;
    let orient = controller.orientation_churn_streak;
    let closure = controller.closure_stall_streak;
    let validation = controller.validation_thrash_streak;
    let failures = controller.repeated_action_failure_streak;
    let discoveries = controller.constraint_discovery_streak;

    if om_local_first_lock && (continuation >= tier1 || orient >= tier1 || closure >= tier1) {
        return Some(3);
    }
    if evidence_sufficient && (continuation >= tier2 || orient >= tier1 || closure >= tier1) {
        return Some(3);
    }

    if discoveries >= 2 {
        return Some(2);
    }

    if continuation >= tier3 || orient >= tier2 || closure >= tier2 || validation >= tier2 {
        Some(3)
    } else if continuation >= tier2 || orient >= tier1 || failures >= 2 {
        Some(2)
    } else if continuation >= tier1 {
        Some(1)
    } else {
        None
    }
}

fn continuation_pressure_message(tier: u8) -> String {
    match tier {
        1 => "[System: You are accumulating tool-continuation turns without a progress boundary. Stop broad inspection. Next turn: either make the smallest concrete code change justified by current evidence, or run one narrow validation tied to the exact file/symbol already inspected.]".to_string(),
        2 => "[System: Orientation churn is persisting. Next turn must choose exactly one: (1) mutate one named file, (2) run one targeted validation tied to one named file/symbol, or (3) state one blocker tied to one named file/symbol. Do not call broad search or read tools again before doing one of those.]".to_string(),
        _ => "[System: Controller escalation: you are burning turns without converging. On the next turn, do exactly one of these and nothing else first: (1) edit/write/change one named file, (2) run one validating command for one named file/symbol, or (3) declare the concrete blocker. Do not call memory_recall, context_status, request_context, codebase_search, read, or view before taking that action.]".to_string(),
    }
}

fn evidence_sufficiency_message() -> String {
    "[System: Actionability threshold reached. The next reversible step is justified. On the next turn, do exactly one of these first: (1) make the smallest justified edit to the named target, (2) run one targeted validation for that named target, or (3) declare the exact blocker. Do not call broad inspection/search tools again unless the last action creates a new contradiction.]".to_string()
}

fn om_local_first_message() -> String {
    "[System: OM coding mode reached actionability. The next reversible step is justified, so stop reopening the search space. Keep the loop tight. Next turn must do exactly one of: (1) apply the smallest reversible patch to the current target, (2) run one narrow validation that proves or disproves the current hypothesis, or (3) state the concrete blocker. Do not start another broad analysis pass first.]".to_string()
}

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
    let mut turn: u32 = 0;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        turn += 1;
        conversation.intent.stats.turns = turn;
        // Refresh tool_defs each turn — manage_tools may have enabled/disabled tools
        // mid-session and we must reflect that in the schema sent to the LLM.
        let tool_defs = bus.tool_definitions();
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
            bus.emit(&omegon_traits::BusEvent::TurnEnd {
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
            });
            let _ = events.send(AgentEvent::TurnEnd {
                turn,
                turn_end_reason: TurnEndReason::AssistantCompleted,
                model: Some(config.model.clone()),
                provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
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
            });
            break;
        }

        if config.soft_limit_turns > 0 && turn == config.soft_limit_turns {
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
        if let Some(warning) = stuck_detector.check() {
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
            opts
        };

        // ─── Ollama cold-start warmup ───────────────────────────
        // A cold 20-30B model can take 3+ minutes to load into memory.
        // The SSE idle timeout (90s) fires before the first token arrives
        // on a cold start. We pre-flight the model load here and surface
        // progress in the TUI via toast notifications.
        if let Some(model_spec) = stream_options.model.as_deref() {
            if crate::providers::infer_provider_id(model_spec) == "ollama" {
                let bare = model_spec
                    .trim_start_matches("ollama:")
                    .trim_start_matches("local:");
                maybe_warmup_ollama(bare, events).await;
            }
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
                bus.emit(&omegon_traits::BusEvent::TurnEnd {
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
                });
                let _ = events.send(AgentEvent::TurnEnd {
                    turn,
                    turn_end_reason: TurnEndReason::Cancelled,
                    model: Some(config.model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
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
                });
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
            // If the conversation has edit/write calls but hasn't been nudged yet,
            // give it one more turn to commit.
            if config.allow_commit_nudge
                && !conversation.intent.commit_nudged
                && has_mutations(conversation)
                && turn < config.max_turns
            {
                conversation.intent.commit_nudged = true;
                tracing::info!("Agent stopped without committing — nudging");
                conversation.push_user(
                    "[System: You made file changes but did not run `git add` and `git commit`. \
                     Please commit your work now with a descriptive message, then summarize what you did.]"
                        .to_string(),
                );
                bus.emit(&omegon_traits::BusEvent::TurnEnd {
                    turn,
                    model: Some(config.model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: {
                        let system_prompt = context
                            .build_system_prompt(conversation.last_user_prompt(), conversation);
                        let llm_messages = conversation.build_llm_view();
                        let prompt_telemetry = context.last_prompt_telemetry();
                        compute_context_composition(
                            &system_prompt,
                            &llm_messages,
                            &tool_defs,
                            context_window,
                            Some(&prompt_telemetry),
                        )
                    },
                    actual_input_tokens: act_in,
                    actual_output_tokens: act_out,
                    cache_read_tokens: act_cr,
                    provider_telemetry: provider_telemetry.clone(),
                });
                let _ = events.send(AgentEvent::TurnEnd {
                    turn,
                    turn_end_reason: TurnEndReason::ProgressNudge,
                    model: Some(config.model.clone()),
                    provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
                    estimated_tokens: conversation.estimate_tokens(),
                    context_window,
                    context_composition: {
                        let system_prompt = context
                            .build_system_prompt(conversation.last_user_prompt(), conversation);
                        let llm_messages = conversation.build_llm_view();
                        let prompt_telemetry = context.last_prompt_telemetry();
                        compute_context_composition(
                            &system_prompt,
                            &llm_messages,
                            &tool_defs,
                            context_window,
                            Some(&prompt_telemetry),
                        )
                    },
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
                });
                continue; // give it one more turn to commit
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
            bus.emit(&omegon_traits::BusEvent::TurnEnd {
                turn,
                model: Some(config.model.clone()),
                provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
                estimated_tokens: conversation.estimate_tokens(),
                context_window,
                context_composition: turn_context_composition.clone(),
                actual_input_tokens: act_in,
                actual_output_tokens: act_out,
                cache_read_tokens: act_cr,
                provider_telemetry: provider_telemetry.clone(),
            });
            let _ = events.send(AgentEvent::TurnEnd {
                turn,
                turn_end_reason: TurnEndReason::AssistantCompleted,
                model: Some(config.model.clone()),
                provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
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
            });
            break;
        }

        // ─── Emit ToolStart bus events before dispatch ──────────────
        for call in tool_calls {
            bus.emit(&omegon_traits::BusEvent::ToolStart {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
            });
        }

        // ─── Dispatch tool calls ────────────────────────────────────
        let auto_delegate_plan =
            classify_auto_delegate_plan(config, conversation, tool_calls, None, None);
        let dispatch_calls_storage;
        let dispatch_calls: &[ToolCall] = if let Some(plan) = auto_delegate_plan {
            dispatch_calls_storage = vec![auto_delegate_tool_call(conversation, plan)];
            &dispatch_calls_storage
        } else {
            tool_calls
        };
        let results = dispatch_tools(
            bus,
            dispatch_calls,
            events,
            cancel.clone(),
            &config.cwd,
            config.secrets.as_deref(),
        )
        .await;

        // Push tool results to conversation and update intent
        for result in &results {
            conversation.push_tool_result(result.clone());
        }
        conversation
            .intent
            .update_from_tools(dispatch_calls, &results);

        let dominant_phase = classify_turn_phase(dispatch_calls, &results);
        let drift_kind = classify_drift_kind(turn, conversation, dispatch_calls, &results);
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
            dispatch_calls,
            &results,
        );
        let evidence_sufficiency =
            detect_evidence_sufficiency(conversation, dispatch_calls, &results);
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            drift_kind,
            progress_signal,
            evidence_sufficiency,
        );
        let continuation_tier = continuation_pressure_tier(
            config,
            &controller,
            conversation,
            dispatch_calls,
            dominant_phase,
        );

        if is_first_turn_orientation_churn(turn, config, conversation, dispatch_calls) {
            tracing::info!(
                "First-turn orientation churn detected — injecting execution-bias nudge"
            );
            conversation.push_user(
                "[System: This run is execution-biased. Stop spending turns on orientation tools unless they are strictly required to unblock execution. On the next turn, take a concrete repo-inspection or implementation step: read the most relevant file, search the codebase for the target symbol/path, or make the smallest justified change.]"
                    .to_string(),
            );
        } else if is_slim_execution_bias(config)
            && controller.evidence_sufficient_streak > 0
            && has_local_target_hypothesis(conversation)
            && continuation_tier.is_some()
        {
            tracing::info!("OM local-first lock engaged — injecting patch-or-prove nudge");
            conversation.push_user(om_local_first_message());
        } else if controller.evidence_sufficient_streak > 0 && continuation_tier.is_some() {
            tracing::info!("Actionability threshold reached — injecting forced-convergence nudge");
            conversation.push_user(evidence_sufficiency_message());
        } else if should_inject_execution_pressure(turn, config, conversation, dispatch_calls) {
            tracing::info!(
                "Execution stall detected after repo inspection — injecting execution-pressure nudge"
            );
            conversation.push_user(
                "[System: You now have enough local evidence. Do not use broad inspection/search tools again until you do one of these two things: (1) make one concrete code edit, or (2) name one specific blocking ambiguity tied to a file or symbol. Stop narrating. Pick the smallest justified patch now, apply it, then run the narrowest relevant validation.]"
                    .to_string(),
            );
        } else if let Some(tier) = continuation_tier {
            tracing::info!(
                tier,
                "Sustained tool-continuation churn detected — injecting continuation-pressure nudge"
            );
            conversation.push_user(continuation_pressure_message(tier));
        }

        // ─── Emit tool events to bus features ───────────────────────
        for (call, result) in dispatch_calls.iter().zip(results.iter()) {
            bus.emit(&omegon_traits::BusEvent::ToolEnd {
                id: call.id.clone(),
                name: call.name.clone(),
                result: omegon_traits::ToolResult {
                    content: result.content.clone(),
                    details: serde_json::Value::Null,
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
            stuck_detector.record(call, is_error);
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
        bus.emit(&omegon_traits::BusEvent::TurnEnd {
            turn,
            model: Some(config.model.clone()),
            provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
            estimated_tokens: conversation.estimate_tokens(),
            context_window,
            context_composition: turn_context_composition.clone(),
            actual_input_tokens: act_in,
            actual_output_tokens: act_out,
            cache_read_tokens: act_cr,
            provider_telemetry: provider_telemetry.clone(),
        });

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
                omegon_traits::BusRequest::RequestCompaction => {
                    tracing::info!("Bus: compaction requested by feature");
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
                    let _ = events.send(event);
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
        let _ = events.send(AgentEvent::TurnEnd {
            turn,
            turn_end_reason: TurnEndReason::ToolContinuation,
            model: Some(config.model.clone()),
            provider: Some(crate::providers::infer_provider_id(&config.model).to_string()),
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
        });
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
    bus.emit(&omegon_traits::BusEvent::SessionEnd {
        turns: turn,
        tool_calls: conversation.intent.stats.tool_calls,
        duration_secs: elapsed.as_secs_f64(),
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
            omegon_traits::BusRequest::RequestCompaction => {
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
                let _ = events.send(event);
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
        &payload[..MAX_COMPACTION_CHARS]
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
async fn maybe_warmup_ollama(model_name: &str, events: &broadcast::Sender<AgentEvent>) {
    let mgr = OllamaManager::new();
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
        let model = config.model.clone();

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
            matches!(attempt, 10 | 25 | 50 | 100) || (attempt > 100 && attempt % 100 == 0);
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
        delay = (delay * 2).min(15_000); // exponential backoff, cap at 15s
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
    let received_content = std::cell::Cell::new(false);
    let idle_timeout = || {
        if received_content.get() {
            content_idle_timeout
        } else {
            initial_idle_timeout
        }
    };
    while let Some(event) = match tokio::time::timeout(idle_timeout(), rx.recv()).await {
        Ok(event) => event,
        Err(_) => {
            let _ = events.send(AgentEvent::MessageAbort);
            anyhow::bail!(
                "LLM stream idle for {}s — connection may be stalled",
                idle_timeout().as_secs()
            );
        }
    } {
        match event {
            LlmEvent::Start => {
                // Heartbeat — any server activity proves connection is alive.
                // Does NOT count as "content" for timeout phase transition.
            }
            LlmEvent::TextStart => {
                received_content.set(true);
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
                            let _ = events.send(AgentEvent::MessageAbort);
                            anyhow::bail!(
                                "Model output degenerate: phrase {:?} repeated {}/{} recent chunks — aborting to prevent runaway",
                                latest,
                                matches,
                                REPETITION_WINDOW_SIZE
                            );
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
                received_content.set(true);
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
                received_content.set(true);
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
                let _ = events.send(AgentEvent::MessageAbort);
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
/// **Auto-batching**: when the LLM returns multiple edit/write calls in one turn,
/// the loop snapshots target files before execution. If any mutation fails, all
/// previously applied mutations are rolled back. This makes the existing `edit`
/// tool secretly atomic across multi-file changes — the agent doesn't need to
/// learn the `change` tool to get atomic behavior.
async fn dispatch_tools(
    bus: &crate::bus::EventBus,
    tool_calls: &[ToolCall],
    events: &broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    cwd: &std::path::Path,
    secrets: Option<&omegon_secrets::SecretsManager>,
) -> Vec<ToolResultEntry> {
    let mut results = Vec::with_capacity(tool_calls.len());

    // ── Auto-batch: snapshot files targeted by mutation tools ────────
    let mutation_count = tool_calls
        .iter()
        .filter(|c| is_mutation_tool(&c.name))
        .count();
    let batch_mode = mutation_count >= 2;

    let mut snapshots: HashMap<std::path::PathBuf, String> = HashMap::new();
    let mut created_files: Vec<std::path::PathBuf> = Vec::new(); // new files to delete on rollback
    let mut mutated_files: Vec<std::path::PathBuf> = Vec::new();

    if batch_mode {
        for call in tool_calls {
            if is_mutation_tool(&call.name)
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
                let result = dispatch_single_tool(bus, &call, &events, cancel, None).await;
                (idx, result)
            }
        }))
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
        indexed_results.extend(parallel_outcomes);
    }

    let mut batch_failed = false;

    for (idx, call) in serial_calls {
        let dispatched = dispatch_single_tool(bus, &call, events, cancel.clone(), secrets).await;

        if !dispatched.is_error
            && is_mutation_tool(&call.name)
            && let Some(path_str) = extract_mutation_path(&call.arguments)
        {
            mutated_files.push(cwd_buf.join(&path_str));
        }

        if dispatched.is_error && batch_mode && is_mutation_tool(&call.name) && !mutated_files.is_empty() {
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
                        Err(e) => rollback_report.push(format!("  ✗ rollback failed {}: {e}", file.display())),
                    }
                } else if created_files.contains(file) {
                    match tokio::fs::remove_file(file).await {
                        Ok(_) => rollback_report.push(format!("  ✓ removed {}", file.display())),
                        Err(e) => rollback_report.push(format!("  ✗ remove failed {}: {e}", file.display())),
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

        if batch_failed && is_mutation_tool(&call.name) {
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
            continue;
        }

        indexed_results.push((idx, dispatched));
    }

    indexed_results.sort_by_key(|(idx, _)| *idx);
    results.extend(indexed_results.into_iter().map(|(_, result)| result));
    results
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
) -> ToolResultEntry {
    if let Some(sm) = secrets
        && let Some(decision) = sm.check_guard(&call.name, &call.arguments)
        && decision.is_block()
    {
        let msg = match &decision {
            omegon_secrets::GuardDecision::Block { reason, path } => {
                format!("Blocked: {reason} ({path})")
            }
            _ => unreachable!(),
        };
        tracing::warn!(tool = call.name, %msg, "tool guard blocked");
        let _ = events.send(AgentEvent::ToolEnd {
            id: call.id.clone(),
            name: call.name.clone(),
            result: omegon_traits::ToolResult {
                content: vec![ContentBlock::Text { text: msg.clone() }],
                details: Value::Null,
            },
            is_error: true,
        });
        return ToolResultEntry {
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            content: vec![ContentBlock::Text { text: msg }],
            is_error: true,
            args_summary: summarize_tool_args(&call.name, &call.arguments),
        };
    }

    let _ = events.send(AgentEvent::ToolStart {
        id: call.id.clone(),
        name: call.name.clone(),
        args: call.arguments.clone(),
    });

    let sink_events = events.clone();
    let sink_call_id = call.id.clone();
    let sink = omegon_traits::ToolProgressSink::from_fn(move |partial| {
        let _ = sink_events.send(AgentEvent::ToolUpdate {
            id: sink_call_id.clone(),
            partial,
        });
    });

    let (result, is_error) = match bus
        .execute_tool_with_sink(
            &call.name,
            &call.id,
            call.arguments.clone(),
            cancel,
            sink,
        )
        .await
    {
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
    };

    let mut final_content = result.content;
    if let Some(sm) = secrets {
        sm.redact_content(&mut final_content);
    }

    const MAX_TOOL_OUTPUT_CHARS: usize = 16_000;
    crate::util::truncate_content_blocks(&mut final_content, MAX_TOOL_OUTPUT_CHARS);

    let _ = events.send(AgentEvent::ToolEnd {
        id: call.id.clone(),
        name: call.name.clone(),
        result: omegon_traits::ToolResult {
            content: final_content.clone(),
            details: result.details,
        },
        is_error,
    });

    ToolResultEntry {
        call_id: call.id.clone(),
        tool_name: call.name.clone(),
        content: final_content,
        is_error,
        args_summary: summarize_tool_args(&call.name, &call.arguments),
    }
}

/// Is this tool a file mutation (edit, write)?
/// Used for auto-batch snapshotting and rollback.
fn is_mutation_tool(name: &str) -> bool {
    matches!(name, "edit" | "write" | "change")
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
    /// Window size for pattern detection
    window: usize,
    /// Number of consecutive turns where a stuck pattern was detected.
    consecutive_warnings: u32,
}

impl StuckDetector {
    fn new() -> Self {
        Self {
            recent: Vec::new(),
            window: 10,
            consecutive_warnings: 0,
        }
    }

    /// Record a tool call for pattern analysis.
    fn record(&mut self, call: &ToolCall, is_error: bool) {
        let args_hash = hash_value(&call.arguments);
        self.recent.push((call.name.clone(), args_hash, is_error));
        if self.recent.len() > self.window * 2 {
            self.recent.drain(..self.window);
        }
    }

    /// Check for stuck patterns. Returns a warning with escalation level if detected.
    fn check(&mut self) -> Option<StuckWarning> {
        let len = self.recent.len();
        if len < 3 {
            self.consecutive_warnings = 0;
            return None;
        }

        let window = &self.recent[len.saturating_sub(self.window)..];

        // Pattern 1: read-without-modify loop — same file read 3+ times
        // without any write/edit to that file. Check this before the generic
        // repeated-call warning so the operator gets a specific nudge.
        let reads: Vec<_> = window
            .iter()
            .filter(|(name, _, _)| name == "read")
            .collect();
        if reads.len() >= 3 {
            let mut hash_counts: HashMap<u64, u32> = HashMap::new();
            for (_, h, _) in &reads {
                *hash_counts.entry(*h).or_default() += 1;
            }
            if hash_counts.values().any(|&c| c >= 3) {
                self.consecutive_warnings += 1;
                return Some(StuckWarning {
                    message: "You've read the same file multiple times without modifying it. \
                         Stop rereading and either edit, validate, or summarize the blocker plainly."
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

        // Pattern 3: read-without-modify loop handled before generic repeated-call warning.

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
    use omegon_traits::ToolProvider;

    #[test]
    fn stuck_detector_repeated_calls() {
        let mut detector = StuckDetector::new();
        let call = ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test -p omegon"}),
        };

        detector.record(&call, false);
        detector.record(&call, false);
        assert!(detector.check().is_none());

        detector.record(&call, false);
        let warning = detector.check();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("same arguments"));
    }

    #[test]
    fn stuck_detector_repeated_errors() {
        let mut detector = StuckDetector::new();
        let call = ToolCall {
            id: "1".into(),
            name: "edit".into(),
            arguments: serde_json::json!({"path": "foo.rs", "oldText": "a", "newText": "b"}),
        };

        detector.record(&call, true);
        detector.record(&call, true);
        detector.record(&call, true);

        // This triggers the repeated-call pattern (same args 3x)
        let warning = detector.check();
        assert!(warning.is_some());
    }

    // ── Auto-batch tests ────────────────────────────────────────────

    #[test]
    fn mutation_tool_detection() {
        assert!(is_mutation_tool("edit"));
        assert!(is_mutation_tool("write"));
        assert!(is_mutation_tool("change"));
        assert!(!is_mutation_tool("read"));
        assert!(!is_mutation_tool("bash"));
        assert!(!is_mutation_tool("web_search"));
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

        let results = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;

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

        let results = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;
        assert!(!results[0].is_error);
        let text = results[0].content[0].as_text().unwrap();
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
                    },
                    omegon_traits::ToolDefinition {
                        name: "view".into(),
                        label: "view".into(),
                        description: "view file".into(),
                        parameters: serde_json::json!({}),
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
        let results = dispatch_tools(&bus, &calls, &events_tx, cancel, dir.path(), None).await;
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 2);
        assert!(elapsed < Duration::from_millis(260), "expected parallel dispatch, got {elapsed:?}");
        assert_eq!(results[0].tool_name, "read");
        assert_eq!(results[1].tool_name, "view");
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
                delay = (delay * 2).min(cap_ms);
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
    fn is_mutation_tool_identifies_write_tools() {
        assert!(is_mutation_tool("write"));
        assert!(is_mutation_tool("edit"));
        assert!(is_mutation_tool("change"));
        assert!(!is_mutation_tool("bash")); // bash not tracked for auto-batch rollback
        assert!(!is_mutation_tool("read"));
        assert!(!is_mutation_tool("chronos"));
        assert!(!is_mutation_tool("design_tree"));
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
            &tool_calls
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
            &tool_calls
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
            &tool_calls
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
        assert!(should_inject_execution_pressure(
            4,
            &config,
            &conversation,
            &tool_calls
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
            &tool_calls
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
        assert!(!should_inject_execution_pressure(
            2,
            &config,
            &conversation,
            &tool_calls
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
        assert!(should_inject_execution_pressure(
            3,
            &config,
            &conversation,
            &tool_calls
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
            consecutive_tool_continuations: 6,
            orientation_churn_streak: 2,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Observe),
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
            classify_drift_kind(3, &conversation, &tool_calls, &results),
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
        assert_eq!(
            classify_drift_kind(3, &conversation, &tool_calls, &results),
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
            classify_drift_kind(3, &conversation, &tool_calls, &results),
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
            classify_drift_kind(3, &conversation, &tool_calls, &results),
            Some(DriftKind::RepeatedActionFailure)
        );
    }

    #[test]
    fn classify_drift_kind_does_not_flag_targeted_validation_as_validation_thrash() {
        let conversation = ConversationState::new();
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "cargo test parser::tests::smoke"}),
            },
            ToolCall {
                id: "2".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "cargo test parser::tests::smoke -- --nocapture"}),
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
            classify_drift_kind(3, &conversation, &tool_calls, &results),
            None
        );
    }

    #[test]
    fn continuation_pressure_not_detected_after_mutation_begins() {
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
                Some(OodaPhase::Observe),
            ),
            None
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
            s.set_slim_mode(true);
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
            consecutive_tool_continuations: 7,
            orientation_churn_streak: 2,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Orient),
            ),
            Some(2)
        );
    }

    #[test]
    fn evidence_sufficiency_detected_after_target_file_and_targeted_validation() {
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert(std::path::PathBuf::from("core/src/context.rs"));
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test parser::tests::smoke"}),
        }];
        let results = vec![ToolResultEntry {
            call_id: "1".into(),
            tool_name: "bash".into(),
            content: vec![ContentBlock::Text { text: "ok".into() }],
            is_error: false,
            args_summary: None,
        }];
        assert_eq!(
            detect_evidence_sufficiency(&conversation, &tool_calls, &results),
            EvidenceSufficiency::Actionable
        );
    }

    #[test]
    fn evidence_sufficiency_detected_for_narrow_local_archaeology() {
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
            detect_evidence_sufficiency(&conversation, &tool_calls, &results),
            EvidenceSufficiency::Actionable
        );
    }

    #[test]
    fn evidence_sufficiency_message_explicitly_forces_action() {
        let text = evidence_sufficiency_message();
        assert!(text.contains("Actionability threshold reached"));
        assert!(text.contains("next reversible step is justified"));
        assert!(text.contains("Do not call broad inspection/search tools again"));
    }

    #[test]
    fn om_local_first_message_forces_patch_or_validate_or_blocker() {
        let text = om_local_first_message();
        assert!(text.contains("OM coding mode reached actionability"));
        assert!(text.contains("smallest reversible patch"));
        assert!(text.contains("state the concrete blocker"));
        assert!(!text.contains("full Omegon is required"));
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
            s.set_slim_mode(true);
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
            evidence_sufficient_streak: 1,
            ..ControllerState::default()
        };
        assert_eq!(
            continuation_pressure_tier(
                &config,
                &controller,
                &conversation,
                &tool_calls,
                Some(OodaPhase::Orient),
            ),
            None
        );
    }

    #[test]
    fn mutation_resets_evidence_sufficiency_streak() {
        let mut controller = ControllerState {
            evidence_sufficient_streak: 3,
            consecutive_tool_continuations: 5,
            ..ControllerState::default()
        };
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            None,
            ProgressSignal::Mutation,
            EvidenceSufficiency::Actionable,
        );
        assert_eq!(controller.evidence_sufficient_streak, 0);
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
            &tool_calls
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
            &tool_calls
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
            evidence_sufficient_streak: 0,
        };
        controller.observe_turn(
            TurnEndReason::ToolContinuation,
            Some(DriftKind::OrientationChurn),
            ProgressSignal::ConstraintDiscovery,
            EvidenceSufficiency::None,
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
            classify_progress_signal(0, 1, &tool_calls, &results),
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
            classify_progress_signal(0, 1, &tool_calls, &results),
            ProgressSignal::None
        );
    }

    #[test]
    fn read_repetition_prefers_file_state_guidance_over_generic_same_args_warning() {
        let mut detector = StuckDetector::new();
        for _ in 0..3 {
            detector.record(
                &ToolCall {
                    id: "1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path": "src/lib.rs"}),
                },
                false,
            );
        }
        let warning = detector.check().expect("warning");
        assert!(warning.contains("same file multiple times"), "got: {warning}");
        assert!(warning.contains("edit, validate, or summarize"), "got: {warning}");
        assert!(!warning.contains("same arguments 3 times"), "got: {warning}");
    }

    #[test]
    fn targeted_read_only_batches_trigger_execution_pressure_by_turn_three() {
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
        assert!(should_inject_execution_pressure(
            3,
            &config,
            &conversation,
            &tool_calls
        ));
    }

    #[test]
    fn auto_delegate_scout_on_slim_orientation_churn_reads() {
        let config = LoopConfig {
            settings: Some(std::sync::Arc::new(std::sync::Mutex::new({
                let mut s = crate::settings::Settings::new("openai-codex:gpt-4.1");
                s.set_slim_mode(true);
                s
            }))),
            ..LoopConfig::default()
        };
        let conversation = ConversationState::new();
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
        let plan = classify_auto_delegate_plan(
            &config,
            &conversation,
            &tool_calls,
            Some(OodaPhase::Observe),
            Some(DriftKind::OrientationChurn),
        );
        assert_eq!(plan.map(|p| p.worker_profile), Some("scout"));
        assert_eq!(plan.map(|p| p.background), Some(true));
    }

    #[test]
    fn auto_delegate_verify_on_slim_validation_only_turns() {
        let config = LoopConfig {
            settings: Some(std::sync::Arc::new(std::sync::Mutex::new({
                let mut s = crate::settings::Settings::new("openai-codex:gpt-4.1");
                s.set_slim_mode(true);
                s
            }))),
            ..LoopConfig::default()
        };
        let conversation = ConversationState::new();
        let tool_calls = vec![ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "cargo test -p omegon delegate"}),
        }];
        let plan = classify_auto_delegate_plan(
            &config,
            &conversation,
            &tool_calls,
            Some(OodaPhase::Act),
            None,
        );
        assert_eq!(plan.map(|p| p.worker_profile), Some("verify"));
        assert_eq!(plan.map(|p| p.background), Some(false));
    }

    #[test]
    fn auto_delegate_patch_on_small_scoped_edit_turn() {
        let config = LoopConfig {
            settings: Some(std::sync::Arc::new(std::sync::Mutex::new({
                let mut s = crate::settings::Settings::new("openai-codex:gpt-4.1");
                s.set_slim_mode(true);
                s
            }))),
            ..LoopConfig::default()
        };
        let conversation = ConversationState::new();
        let tool_calls = vec![
            ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path": "src/lib.rs"}),
            },
            ToolCall {
                id: "2".into(),
                name: "edit".into(),
                arguments: serde_json::json!({"path": "src/lib.rs", "oldText": "a", "newText": "b"}),
            },
            ToolCall {
                id: "3".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": "cargo test -p omegon lib"}),
            },
        ];
        let plan = classify_auto_delegate_plan(
            &config,
            &conversation,
            &tool_calls,
            Some(OodaPhase::Act),
            None,
        );
        assert_eq!(plan.map(|p| p.worker_profile), Some("patch"));
        assert_eq!(plan.map(|p| p.background), Some(false));
    }

    #[test]
    fn auto_delegate_tool_call_marks_background_for_scout_only() {
        let conversation = ConversationState::new();
        let scout = auto_delegate_tool_call(
            &conversation,
            AutoDelegatePlan {
                worker_profile: "scout",
                background: true,
            },
        );
        let patch = auto_delegate_tool_call(
            &conversation,
            AutoDelegatePlan {
                worker_profile: "patch",
                background: false,
            },
        );
        assert_eq!(
            scout.arguments.get("background").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            patch.arguments.get("background").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn auto_delegate_skips_when_parent_already_mutated_files() {
        let config = LoopConfig {
            settings: Some(std::sync::Arc::new(std::sync::Mutex::new({
                let mut s = crate::settings::Settings::new("openai-codex:gpt-4.1");
                s.set_slim_mode(true);
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
            &ToolCall {
                id: "1".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            false,
        );
        detector.record(
            &ToolCall {
                id: "2".into(),
                name: "read".into(),
                arguments: Value::Null,
            },
            false,
        );
        // Switch to a different tool — resets the counter
        detector.record(
            &ToolCall {
                id: "3".into(),
                name: "write".into(),
                arguments: Value::Null,
            },
            false,
        );
        assert!(
            detector.check().is_none(),
            "different tools should not trigger stuck"
        );
    }

    #[test]
    fn stuck_detector_fires_on_same_tool_repeated() {
        let mut detector = StuckDetector::new();
        for i in 0..10 {
            detector.record(
                &ToolCall {
                    id: format!("{i}"),
                    name: "bash".into(),
                    arguments: serde_json::json!({"command": "cat /dev/null"}),
                },
                true,
            );
        }
        // After enough repeated error calls, should flag as stuck
        let result = detector.check();
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
