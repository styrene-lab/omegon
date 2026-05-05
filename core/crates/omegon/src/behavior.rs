//! Behavioral classification layer for the agent loop.
//!
//! Contains tool-classification predicates, drift/phase classifiers,
//! continuation pressure heuristics, auto-delegation logic, and the
//! `ControllerState` streak tracker. Extracted from `loop.rs` to keep
//! the core state machine focused on turn orchestration.

use crate::conversation::{ConversationState, ToolCall, ToolResultEntry};
pub(crate) use omegon_traits::ProgressSignal;
use omegon_traits::{DriftKind, OodaPhase, ProgressNudgeReason, ToolCapability, ToolDefinition};
use std::collections::{BTreeSet, HashMap};

// ─── Tool classification predicates ────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolCapabilityCatalog {
    capabilities_by_name: HashMap<String, BTreeSet<ToolCapability>>,
}

impl ToolCapabilityCatalog {
    pub fn from_tool_defs(tool_defs: &[ToolDefinition]) -> Self {
        let capabilities_by_name = tool_defs
            .iter()
            .map(|def| {
                (
                    def.name.clone(),
                    def.capabilities.iter().copied().collect::<BTreeSet<_>>(),
                )
            })
            .collect();
        Self {
            capabilities_by_name,
        }
    }

    fn has(&self, tool_name: &str, capability: ToolCapability) -> bool {
        self.capabilities_by_name
            .get(tool_name)
            .is_some_and(|caps| caps.contains(&capability))
    }
}

pub(crate) fn is_orientation_tool(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::Orientation)
}

pub(crate) fn is_repo_inspection_tool(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::RepoInspection)
}

pub(crate) fn is_broad_orientation_tool(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::BroadOrientation)
}

pub(crate) fn is_broad_repo_inspection_tool(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::BroadRepoInspection)
}

pub(crate) fn is_targeted_repo_inspection_tool(
    catalog: &ToolCapabilityCatalog,
    name: &str,
) -> bool {
    catalog.has(name, ToolCapability::TargetedRepoInspection)
}

pub(crate) fn is_mutation_tool_name(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::Mutation)
}

pub(crate) fn mutation_targets_within_limit(
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    max_files: usize,
) -> bool {
    let mut paths = std::collections::BTreeSet::new();
    for call in tool_calls {
        if !is_mutation_tool_name(catalog, &call.name) {
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

pub(crate) fn is_narrow_patch_candidate(
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
) -> bool {
    if !tool_calls
        .iter()
        .any(|call| is_mutation_tool_name(catalog, &call.name))
    {
        return false;
    }
    if !mutation_targets_within_limit(catalog, tool_calls, 2) {
        return false;
    }
    tool_calls.iter().all(|call| {
        is_mutation_tool_name(catalog, &call.name)
            || is_targeted_repo_inspection_tool(catalog, &call.name)
            || is_validation_tool(call)
    })
}

pub(crate) fn is_validation_tool(call: &ToolCall) -> bool {
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

// ─── Phase & drift classification ──────────────────────────────────────────

pub(crate) fn classify_turn_phase(
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> Option<OodaPhase> {
    if tool_calls.is_empty() {
        return None;
    }

    // Tools that produce output or change state are Act.
    if tool_calls.iter().any(|call| {
        catalog.has(&call.name, ToolCapability::StateChanging) || is_validation_tool(call)
    }) {
        return Some(OodaPhase::Act);
    }

    let successful_mutation = tool_calls.iter().any(|call| {
        is_mutation_tool_name(catalog, &call.name)
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
        .all(|call| is_orientation_tool(catalog, &call.name))
    {
        return Some(OodaPhase::Observe);
    }

    if tool_calls
        .iter()
        .all(|call| is_repo_inspection_tool(catalog, &call.name))
    {
        return Some(OodaPhase::Observe);
    }

    if tool_calls.iter().all(is_validation_tool) {
        return Some(OodaPhase::Act);
    }

    if tool_calls
        .iter()
        .any(|call| is_mutation_tool_name(catalog, &call.name) || is_validation_tool(call))
    {
        return Some(OodaPhase::Act);
    }

    Some(OodaPhase::Orient)
}

pub(crate) fn classify_drift_kind(
    catalog: &ToolCapabilityCatalog,
    turn: u32,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> Option<DriftKind> {
    let broad_orientation_calls = tool_calls
        .iter()
        .filter(|call| is_broad_orientation_tool(catalog, &call.name))
        .count();
    let broad_repo_inspection_calls = tool_calls
        .iter()
        .filter(|call| is_broad_repo_inspection_tool(catalog, &call.name))
        .count();
    let targeted_repo_inspection_calls = tool_calls
        .iter()
        .filter(|call| is_targeted_repo_inspection_tool(catalog, &call.name))
        .count();

    if conversation.intent.files_modified.is_empty()
        && !conversation.intent.files_read.is_empty()
        && tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(catalog, &call.name))
        && turn >= 4
        && broad_repo_inspection_calls > 0
        && targeted_repo_inspection_calls <= 1
    {
        return Some(DriftKind::OrientationChurn);
    }

    if conversation.intent.files_modified.is_empty()
        && conversation.intent.files_read.is_empty()
        && turn >= 3
        && broad_orientation_calls == tool_calls.len()
    {
        return Some(DriftKind::OrientationChurn);
    }

    let failing_mutations: Vec<&ToolCall> = tool_calls
        .iter()
        .filter(|call| {
            is_mutation_tool_name(catalog, &call.name)
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
            .all(|call| is_repo_inspection_tool(catalog, &call.name))
        && broad_repo_inspection_calls > 0
    {
        return Some(DriftKind::ClosureStall);
    }

    None
}

pub(crate) fn progress_nudge_reason_for_drift(drift: DriftKind) -> ProgressNudgeReason {
    match drift {
        DriftKind::OrientationChurn => ProgressNudgeReason::AntiOrientation,
        DriftKind::RepeatedActionFailure => ProgressNudgeReason::ActionRecovery,
        DriftKind::ValidationThrash => ProgressNudgeReason::ValidationPressure,
        DriftKind::ClosureStall => ProgressNudgeReason::ClosurePressure,
    }
}

pub(crate) fn is_first_turn_orientation_churn(
    turn: u32,
    config: &super::r#loop::LoopConfig,
    conversation: &ConversationState,
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
) -> bool {
    config.enforce_first_turn_execution_bias
        && turn == 1
        && !tool_calls.is_empty()
        && tool_calls
            .iter()
            .all(|call| is_orientation_tool(catalog, &call.name))
        && conversation.intent.files_read.is_empty()
        && conversation.intent.files_modified.is_empty()
}

pub(crate) fn should_inject_execution_pressure(
    turn: u32,
    _config: &super::r#loop::LoopConfig,
    conversation: &ConversationState,
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    behavior: BehavioralTier,
) -> bool {
    if tool_calls.is_empty()
        || !conversation.intent.files_modified.is_empty()
        || conversation.intent.files_read.is_empty()
        || !tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(catalog, &call.name))
    {
        return false;
    }

    let has_broad_repo_inspection = tool_calls
        .iter()
        .any(|call| is_broad_repo_inspection_tool(catalog, &call.name));

    // Give the agent time to orient before pressuring execution.
    let (broad_threshold, targeted_threshold) = match behavior {
        BehavioralTier::Constrained => (3, 4),
        BehavioralTier::Standard => (5, 6),
    };

    (turn >= broad_threshold && has_broad_repo_inspection)
        || (turn >= targeted_threshold && !has_broad_repo_inspection)
}

// ─── Progress signals & evidence ───────────────────────────────────────────

// ProgressSignal is now defined in omegon-traits and imported above.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceSufficiency {
    None,
    Targeted,
    Actionable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvidenceAssessment {
    pub local: EvidenceSufficiency,
    pub global: EvidenceSufficiency,
}

/// Behavioral tier for loop control. Determines pressure thresholds and nudge style.
/// Frontier/Max models get standard treatment; Mid/Leaf models get a tighter leash
/// with simpler instructions, earlier pressure, and dead-mouse detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BehavioralTier {
    /// Frontier/Max models — current defaults, multi-clause nudges
    Standard,
    /// Mid/Leaf models (Ollama, Groq, etc.) — tighter thresholds, imperative nudges
    Constrained,
}

pub(crate) fn behavioral_tier(config: &super::r#loop::LoopConfig) -> BehavioralTier {
    let tier = crate::routing::infer_model_tier(&config.model);
    match tier {
        crate::routing::CapabilityTier::Max | crate::routing::CapabilityTier::Frontier => {
            BehavioralTier::Standard
        }
        crate::routing::CapabilityTier::Mid | crate::routing::CapabilityTier::Leaf => {
            BehavioralTier::Constrained
        }
    }
}

// ─── Controller state (streak tracker) ─────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ControllerState {
    pub consecutive_tool_continuations: u32,
    pub orientation_churn_streak: u32,
    pub repeated_action_failure_streak: u32,
    pub validation_thrash_streak: u32,
    pub closure_stall_streak: u32,
    pub constraint_discovery_streak: u32,
    pub targeted_evidence_streak: u32,
    pub local_evidence_sufficient_streak: u32,
    pub evidence_sufficient_streak: u32,
}

impl ControllerState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Snapshot the streak counters as the public `ControllerStreaks`
    /// shape that's carried on `AgentEvent::TurnEnd`.
    pub fn streaks(&self) -> omegon_traits::ControllerStreaks {
        omegon_traits::ControllerStreaks {
            orientation_churn: self.orientation_churn_streak,
            repeated_action_failure: self.repeated_action_failure_streak,
            validation_thrash: self.validation_thrash_streak,
            closure_stall: self.closure_stall_streak,
            constraint_discovery: self.constraint_discovery_streak,
            evidence_sufficient: self.evidence_sufficient_streak,
        }
    }

    pub fn observe_turn(
        &mut self,
        turn_end_reason: omegon_traits::TurnEndReason,
        drift_kind: Option<DriftKind>,
        progress_signal: ProgressSignal,
        evidence: EvidenceAssessment,
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

        if matches!(
            turn_end_reason,
            omegon_traits::TurnEndReason::ToolContinuation
        ) {
            self.consecutive_tool_continuations =
                self.consecutive_tool_continuations.saturating_add(1);
        } else {
            self.consecutive_tool_continuations = 0;
        }

        // Drift streaks: increment on match, *decay* (halve) on mismatch
        // instead of hard-resetting.
        self.orientation_churn_streak = if matches!(drift_kind, Some(DriftKind::OrientationChurn)) {
            self.orientation_churn_streak.saturating_add(1)
        } else {
            self.orientation_churn_streak / 2
        };
        self.repeated_action_failure_streak =
            if matches!(drift_kind, Some(DriftKind::RepeatedActionFailure)) {
                self.repeated_action_failure_streak.saturating_add(1)
            } else {
                self.repeated_action_failure_streak / 2
            };
        self.validation_thrash_streak = if matches!(drift_kind, Some(DriftKind::ValidationThrash)) {
            self.validation_thrash_streak.saturating_add(1)
        } else {
            self.validation_thrash_streak / 2
        };
        self.closure_stall_streak = if matches!(drift_kind, Some(DriftKind::ClosureStall)) {
            self.closure_stall_streak.saturating_add(1)
        } else {
            self.closure_stall_streak / 2
        };
        self.constraint_discovery_streak =
            if matches!(progress_signal, ProgressSignal::ConstraintDiscovery) {
                self.constraint_discovery_streak.saturating_add(1)
            } else {
                self.constraint_discovery_streak / 2
            };
        self.targeted_evidence_streak = if matches!(
            evidence.local,
            EvidenceSufficiency::Targeted | EvidenceSufficiency::Actionable
        ) {
            self.targeted_evidence_streak.saturating_add(1)
        } else {
            self.targeted_evidence_streak / 2
        };
        self.local_evidence_sufficient_streak =
            if matches!(evidence.local, EvidenceSufficiency::Actionable) {
                self.local_evidence_sufficient_streak.saturating_add(1)
            } else {
                self.local_evidence_sufficient_streak / 2
            };
        self.evidence_sufficient_streak =
            if matches!(evidence.global, EvidenceSufficiency::Actionable) {
                self.evidence_sufficient_streak.saturating_add(1)
            } else {
                self.evidence_sufficient_streak / 2
            };
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

pub(crate) fn has_successful_tool_call<F>(
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

pub(crate) fn has_progress_boundary(
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> bool {
    has_successful_tool_call(tool_calls, results, |call| {
        is_mutation_tool_name(catalog, &call.name)
    }) || has_successful_tool_call(tool_calls, results, is_validation_tool)
        || has_successful_tool_call(tool_calls, results, |call| call.name == "commit")
}

pub(crate) fn classify_validation_scope(
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

pub(crate) fn detect_constraint_discovery(
    constraints_before: usize,
    constraints_after: usize,
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> bool {
    if constraints_after <= constraints_before {
        return false;
    }

    tool_calls.iter().any(|call| {
        is_repo_inspection_tool(catalog, &call.name)
            || is_validation_tool(call)
            || (is_mutation_tool_name(catalog, &call.name)
                && results
                    .iter()
                    .find(|result| result.call_id == call.id)
                    .is_some_and(|result| result.is_error))
    })
}

pub(crate) fn classify_progress_signal(
    constraints_before: usize,
    constraints_after: usize,
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> ProgressSignal {
    if has_successful_tool_call(tool_calls, results, |call| call.name == "commit") {
        return ProgressSignal::Commit;
    }
    if has_successful_tool_call(tool_calls, results, |call| {
        is_mutation_tool_name(catalog, &call.name)
            || call.name == "commit"
            || call.name == "delegate"
            || call.name == "cleave_run"
    }) {
        return ProgressSignal::Mutation;
    }

    let validation_signal = classify_validation_scope(tool_calls, results);
    if !matches!(validation_signal, ProgressSignal::None) {
        return validation_signal;
    }

    if detect_constraint_discovery(
        constraints_before,
        constraints_after,
        catalog,
        tool_calls,
        results,
    ) {
        return ProgressSignal::ConstraintDiscovery;
    }

    ProgressSignal::None
}

pub(crate) fn assess_evidence(
    conversation: &ConversationState,
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> EvidenceAssessment {
    if conversation.intent.files_read.is_empty() {
        return EvidenceAssessment {
            local: EvidenceSufficiency::None,
            global: EvidenceSufficiency::None,
        };
    }

    if !conversation.intent.files_modified.is_empty() {
        return EvidenceAssessment {
            local: EvidenceSufficiency::Actionable,
            global: EvidenceSufficiency::Actionable,
        };
    }

    let targeted_validation = matches!(
        classify_validation_scope(tool_calls, results),
        ProgressSignal::TargetedValidation
    );
    let failed_mutation_on_known_target = tool_calls.iter().any(|call| {
        is_mutation_tool_name(catalog, &call.name)
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
        is_repo_inspection_tool(catalog, &call.name)
            && results.iter().any(|result| result.is_error)
            && tool_calls.iter().any(is_validation_tool)
    });

    let targeted_reads: Vec<&str> = tool_calls
        .iter()
        .filter(|call| is_targeted_repo_inspection_tool(catalog, &call.name))
        .filter_map(|call| call.arguments.get("path").and_then(|v| v.as_str()))
        .collect();
    let narrow_target_cluster = !targeted_reads.is_empty()
        && tool_calls
            .iter()
            .all(|call| is_repo_inspection_tool(catalog, &call.name))
        && !tool_calls
            .iter()
            .any(|call| is_broad_repo_inspection_tool(catalog, &call.name));
    let targeted_paths_known = narrow_target_cluster
        && targeted_reads.iter().all(|path| {
            conversation
                .intent
                .files_read
                .iter()
                .any(|read| read == std::path::Path::new(path))
        });
    let local_target_count = conversation.intent.files_read.len();
    let local = if targeted_paths_known && local_target_count <= 2 {
        EvidenceSufficiency::Actionable
    } else if targeted_paths_known || local_target_count <= 2 {
        EvidenceSufficiency::Targeted
    } else {
        EvidenceSufficiency::None
    };
    let global = if targeted_validation
        || failed_mutation_on_known_target
        || inspection_backed_by_validation_failure
    {
        EvidenceSufficiency::Actionable
    } else {
        EvidenceSufficiency::None
    };

    EvidenceAssessment { local, global }
}

pub(crate) fn is_slim_execution_bias(config: &super::r#loop::LoopConfig) -> bool {
    config
        .settings
        .as_ref()
        .and_then(|settings| settings.lock().ok().map(|s| s.is_slim()))
        .unwrap_or(false)
}

pub(crate) fn has_local_target_hypothesis(conversation: &ConversationState) -> bool {
    !conversation.intent.files_read.is_empty() && conversation.intent.files_modified.is_empty()
}

// ─── Continuation pressure ─────────────────────────────────────────────────

pub(crate) fn continuation_pressure_tier(
    config: &super::r#loop::LoopConfig,
    controller: &ControllerState,
    conversation: &ConversationState,
    tool_calls: &[ToolCall],
    dominant_phase: Option<OodaPhase>,
    behavior: BehavioralTier,
) -> Option<u8> {
    if tool_calls.is_empty()
        || !matches!(dominant_phase, Some(OodaPhase::Observe | OodaPhase::Orient))
    {
        return None;
    }

    let local_evidence_sufficient = controller.local_evidence_sufficient_streak > 0;
    let evidence_sufficient = controller.evidence_sufficient_streak > 0;
    let om_local_first_lock = is_slim_execution_bias(config)
        && local_evidence_sufficient
        && has_local_target_hypothesis(conversation);
    let constrained = behavior == BehavioralTier::Constrained;
    let (tier1, tier2, tier3) = if om_local_first_lock {
        if constrained { (2, 3, 5) } else { (4, 6, 8) }
    } else if evidence_sufficient {
        if constrained { (3, 4, 6) } else { (6, 8, 10) }
    } else if is_slim_execution_bias(config) {
        if constrained { (4, 6, 8) } else { (8, 12, 16) }
    } else if constrained {
        (3, 5, 7)
    } else {
        (12, 16, 20)
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

pub(crate) fn continuation_pressure_message(tier: u8, behavior: BehavioralTier) -> String {
    match (tier, behavior) {
        (1, BehavioralTier::Constrained) => "[System: You have been exploring. Produce output now — write, edit, or state what's blocking you.]".to_string(),
        (2, BehavioralTier::Constrained) => "[System: Produce output now. Write or edit a file, or state the blocker.]".to_string(),
        (_, BehavioralTier::Constrained) => "[System: You must produce output on this turn. Write, edit, or explain why you cannot.]".to_string(),
        (1, _) => "[System: You have spent several turns exploring without producing output. You likely have enough context. Take the next concrete step toward completing the user's request — write a file, make an edit, run a command that produces a result, or explain what's blocking you.]".to_string(),
        (2, _) => "[System: You are still exploring. Produce a concrete result now: write or edit a file, run a command that changes state, or tell the user exactly what's blocking progress.]".to_string(),
        _ => "[System: You have been exploring for many turns without producing output. On this turn, you must do one of: (1) write or edit a file, (2) run a command that produces the requested result, or (3) tell the user exactly what is preventing you from completing the task.]".to_string(),
    }
}

// ─── Auto-delegation ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoDelegatePlan {
    pub worker_profile: &'static str,
    pub background: bool,
}

/// Auto-delegation is DISABLED. It was an experimental feature that
/// intercepted the agent's tool calls and dispatched them to background
/// workers. In practice, the workers frequently failed silently, causing
/// "content dispatched" messages with no actual work done. Users reported
/// this as "the agent cannot perform work" — the exact opposite of what
/// auto-delegation was supposed to achieve.
///
/// The agent should always execute its own tool calls directly.
/// Delegation is still available as an explicit tool the agent can
/// choose to call — just not as an invisible interception layer.
pub(crate) fn classify_auto_delegate_plan(
    _config: &super::r#loop::LoopConfig,
    _conversation: &ConversationState,
    _tool_calls: &[ToolCall],
    _dominant_phase: Option<OodaPhase>,
    _drift_kind: Option<DriftKind>,
) -> Option<AutoDelegatePlan> {
    None
}

pub(crate) fn evidence_sufficiency_message(behavior: BehavioralTier) -> String {
    match behavior {
        BehavioralTier::Constrained => "[System: You have enough context. Produce output now.]".to_string(),
        BehavioralTier::Standard => "[System: You have gathered enough context to act. Produce a concrete result — write a file, make an edit, or explain what's blocking you.]".to_string(),
    }
}

pub(crate) fn om_local_first_message(behavior: BehavioralTier) -> String {
    match behavior {
        BehavioralTier::Constrained => "[System: Produce output now. Do not search again.]".to_string(),
        BehavioralTier::Standard => "[System: You have enough context. Produce the requested output — write, edit, or state the blocker.]".to_string(),
    }
}

// ─── Auto-delegation ───────────────────────────────────────────────────────

pub(crate) fn auto_delegate_tool_call(
    conversation: &ConversationState,
    plan: AutoDelegatePlan,
) -> ToolCall {
    // Use the tracked task from conversation intent, but validate it.
    // If the tracked task is conversational or too vague, fall back to
    // a generic orientation instruction that the delegate can work with.
    let raw_task = conversation.intent.current_task.clone().unwrap_or_default();
    let task = if raw_task.trim().is_empty()
        || crate::features::delegate::is_conversational_non_task(&raw_task)
    {
        "Inspect the current bounded task and return concise findings.".to_string()
    } else {
        raw_task
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
