//! Behavioral classification layer for the agent loop.
//!
//! Contains tool-classification predicates, drift/phase classifiers,
//! continuation pressure heuristics, auto-delegation logic, and the
//! `ControllerState` streak tracker. Extracted from `loop.rs` to keep
//! the core state machine focused on turn orchestration.

use crate::conversation::{ConversationState, TaskMode, ToolCall, ToolResultEntry};
pub(crate) use omegon_traits::ProgressSignal;
use omegon_traits::{DriftKind, OodaPhase, ProgressNudgeReason, ToolCapability, ToolDefinition};
use std::collections::{BTreeSet, HashMap};

// ─── Task-mode inference ────────────────────────────────────────────────────

/// Infer the guidance task mode from the operator's prompt.
///
/// Research-style prompts (questions, explain/summarize/review requests, any
/// read-oriented ask) legitimately spend many turns in read/search without
/// mutating files, so convergence pressure must relax for them. The heuristic
/// errs strongly toward `Research`: a false `Implementation` classification
/// pushes the model to invent file-writing work the user never requested,
/// which is the worse failure mode.
pub(crate) fn explicit_task_mode_from_prompt(prompt: &str) -> Option<TaskMode> {
    let normalized = prompt.trim_start().to_lowercase();
    let first_line = normalized.lines().next().unwrap_or("").trim();
    match first_line {
        "/mode research" | "/mode: research" | "[mode: research]" => Some(TaskMode::Research),
        "/mode implementation" | "/mode: implementation" | "[mode: implementation]" => {
            Some(TaskMode::Implementation)
        }
        _ => None,
    }
}

pub(crate) fn infer_task_mode_from_prompt(prompt: &str) -> TaskMode {
    if let Some(mode) = explicit_task_mode_from_prompt(prompt) {
        return mode;
    }
    let prompt = prompt.to_lowercase();
    let starts = |w: &str| prompt.trim_start().starts_with(w);
    let research = prompt.contains('?')
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
        || starts("assess")
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
        || starts("investigate")
        || starts("research")
        || prompt.contains(" rundown")
        || prompt.contains(" summary")
        || prompt.contains(" overview");
    if research {
        TaskMode::Research
    } else {
        TaskMode::Implementation
    }
}

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

    pub fn capabilities_for(&self, tool_name: &str) -> Vec<ToolCapability> {
        self.capabilities_by_name
            .get(tool_name)
            .map(|caps| caps.iter().copied().collect())
            .unwrap_or_default()
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

pub(crate) fn is_validation_tool_name(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::Validation)
}

pub(crate) fn is_progress_boundary_tool(catalog: &ToolCapabilityCatalog, name: &str) -> bool {
    catalog.has(name, ToolCapability::ProgressBoundary)
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
            || is_validation_tool_name(catalog, &call.name)
    })
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
        catalog.has(&call.name, ToolCapability::StateChanging)
            || is_validation_tool_name(catalog, &call.name)
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

    if tool_calls
        .iter()
        .all(|call| is_validation_tool_name(catalog, &call.name))
    {
        return Some(OodaPhase::Act);
    }

    if tool_calls.iter().any(|call| {
        is_mutation_tool_name(catalog, &call.name) || is_validation_tool_name(catalog, &call.name)
    }) {
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

    let research_mode = conversation.intent.task_mode == TaskMode::Research;

    if !research_mode
        && conversation.intent.files_modified.is_empty()
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

    if !research_mode
        && conversation.intent.files_modified.is_empty()
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
        .filter(|call| is_validation_tool_name(catalog, &call.name))
        .count();
    let targeted_validation = matches!(
        classify_validation_scope(catalog, tool_calls, results),
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
    // Research turns legitimately read/search without mutating files; do not
    // pressure them toward edits they were never asked to make.
    if conversation.intent.task_mode == TaskMode::Research {
        return false;
    }
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
    let tier = crate::routing::infer_model_grade_band(&config.model);
    match tier {
        crate::routing::CapabilityGradeBand::Max
        | crate::routing::CapabilityGradeBand::Frontier => BehavioralTier::Standard,
        crate::routing::CapabilityGradeBand::Mid | crate::routing::CapabilityGradeBand::Leaf => {
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

/// Minimum trimmed length for interleaved assistant prose to count as
/// visible output for continuation-pressure purposes. Short narration
/// ("Checking the config...") stays below this; substantive analysis
/// delivered alongside tool calls clears it.
const SUBSTANTIVE_PROSE_MIN_CHARS: usize = 240;

/// True when the assistant text emitted alongside tool calls is substantive
/// output rather than transitional narration.
pub(crate) fn is_substantive_interleaved_prose(text: &str) -> bool {
    text.trim().len() >= SUBSTANTIVE_PROSE_MIN_CHARS
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
        substantive_prose: bool,
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
            // Substantive interleaved prose IS visible output — the operator
            // is being answered while tools run. Hold the counter instead of
            // incrementing so "exploring without producing output" pressure
            // only accrues on genuinely silent tool grinding. Short narration
            // ("Let me check X...") still counts as silent.
            if !substantive_prose {
                self.consecutive_tool_continuations =
                    self.consecutive_tool_continuations.saturating_add(1);
            }
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
    }) || has_successful_tool_call(tool_calls, results, |call| {
        is_validation_tool_name(catalog, &call.name)
    }) || has_successful_tool_call(tool_calls, results, |call| {
        is_progress_boundary_tool(catalog, &call.name)
    })
}

pub(crate) fn classify_validation_scope(
    catalog: &ToolCapabilityCatalog,
    tool_calls: &[ToolCall],
    results: &[ToolResultEntry],
) -> ProgressSignal {
    let successful_validation_calls: Vec<&ToolCall> = tool_calls
        .iter()
        .filter(|call| {
            is_validation_tool_name(catalog, &call.name)
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
        let level = call
            .arguments
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("standard");
        if level == "full" {
            return false;
        }

        call.arguments
            .get("paths")
            .and_then(|v| v.as_array())
            .is_some_and(|paths| !paths.is_empty() && paths.len() <= 2)
            || call.arguments.get("path").is_some()
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
            || is_validation_tool_name(catalog, &call.name)
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
    if has_successful_tool_call(tool_calls, results, |call| {
        is_progress_boundary_tool(catalog, &call.name)
    }) {
        return ProgressSignal::Commit;
    }
    if has_successful_tool_call(tool_calls, results, |call| {
        is_mutation_tool_name(catalog, &call.name) || is_progress_boundary_tool(catalog, &call.name)
    }) {
        return ProgressSignal::Mutation;
    }

    let validation_signal = classify_validation_scope(catalog, tool_calls, results);
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
        classify_validation_scope(catalog, tool_calls, results),
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
            && tool_calls
                .iter()
                .any(|validation| is_validation_tool_name(catalog, &validation.name))
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
    let low_novelty_revisit_streak = conversation
        .intent
        .evidence_ledger
        .low_novelty_revisit_streak();
    let global = if targeted_validation
        || failed_mutation_on_known_target
        || inspection_backed_by_validation_failure
    {
        EvidenceSufficiency::Actionable
    } else {
        EvidenceSufficiency::None
    };
    if conversation.intent.task_mode == TaskMode::Research
        && global != EvidenceSufficiency::Actionable
    {
        return EvidenceAssessment {
            local: if targeted_paths_known {
                EvidenceSufficiency::Targeted
            } else {
                EvidenceSufficiency::None
            },
            global,
        };
    }

    let local = if targeted_paths_known && low_novelty_revisit_streak >= 2 {
        EvidenceSufficiency::Actionable
    } else if targeted_paths_known || !conversation.intent.files_read.is_empty() {
        EvidenceSufficiency::Targeted
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
    let research_mode = conversation.intent.task_mode == TaskMode::Research;
    let om_local_first_lock = !research_mode
        && is_slim_execution_bias(config)
        && local_evidence_sufficient
        && has_local_target_hypothesis(conversation);
    let constrained = behavior == BehavioralTier::Constrained;
    let (tier1, tier2, tier3) = if research_mode {
        // Research turns legitimately spend many turns in read/search.
        // Keep only a late safety net against genuinely unbounded exploration.
        if constrained {
            (8, 12, 16)
        } else {
            (16, 24, 32)
        }
    } else if om_local_first_lock {
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

    if discoveries >= 2 && !research_mode {
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
    // IMPORTANT: A direct text reply IS valid output. Do NOT bias toward file
    // mutations — many sessions are Q&A / explanation work where writing a
    // file is wrong (e.g. answering "summarize this doc" by creating a new
    // summary file the user never asked for). File writes are listed only as
    // an option, after answering, and only when the user explicitly asked to
    // change a file.
    match (tier, behavior) {
        (1, BehavioralTier::Constrained) => "[System: You have been exploring. Produce output now — answer the user, or state what's blocking you. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        (2, BehavioralTier::Constrained) => "[System: Produce output now. Answer the user, or (only if they explicitly asked you to change a file) write/edit one. Otherwise state the blocker. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        (_, BehavioralTier::Constrained) => "[System: You must produce output on this turn. Answer the user, or explain why you cannot. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        (1, _) => "[System: You have spent several turns exploring without producing output. You likely have enough context. Take the next concrete step toward completing the user's request — answer them directly. If — and only if — they explicitly asked you to modify a file, do that instead. Otherwise reply in chat. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        (2, _) => "[System: You are still exploring. Produce a concrete result now: answer the user's question, or (only if they explicitly asked) write/edit a file. Do not invent file-writing tasks the user did not request. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        _ => "[System: You have been exploring for many turns without producing output. On this turn, you must do one of: (1) answer the user directly in chat, (2) write or edit a file ONLY if the user explicitly asked for that, or (3) tell the user exactly what is preventing you from completing the task. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
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
        BehavioralTier::Constrained => "[System: You have enough context. Produce output now — answer the user. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        BehavioralTier::Standard => "[System: You have gathered enough context to act. Produce a concrete result — answer the user's question. If they explicitly asked you to modify a file, do that. Otherwise reply in chat; do not invent file-writing work. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
    }
}

pub(crate) fn om_local_first_message(behavior: BehavioralTier) -> String {
    match behavior {
        BehavioralTier::Constrained => "[System: Produce output now. Do not search again. Answer the user. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
        BehavioralTier::Standard => "[System: You have enough context. Produce the requested output — answer the user. If they explicitly asked you to modify a file, do that; otherwise reply in chat. Do not apologize, self-criticize, mirror operator frustration, or explain your process.]".to_string(),
    }
}

pub(crate) fn operator_correction_recovery_message() -> String {
    "[System: The operator corrected your behavior. Treat this as a control signal. Do not apologize, self-criticize, mirror profanity, or explain your process. Preserve the active task, stop broad exploration, and take the smallest concrete next action. If blocked, state the blocker and the exact operator decision needed.]".to_string()
}

pub(crate) fn meta_recovery_retry_message() -> String {
    "[System: Your previous response was meta-commentary rather than task progress. Retry now with no apology, self-critique, profanity mirroring, or process narration. Take the next concrete action, answer the user's request, or state the precise blocker.]".to_string()
}

pub(crate) fn is_pathological_meta_response(text: &str) -> bool {
    let normalized = text.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    let self_rebuke = [
        "i'm wasting",
        "i am wasting",
        "i've been wasting",
        "i have been wasting",
        "i was wasting",
        "my mistake was",
        "my failure was",
        "i over-investigated",
        "i over investigated",
        "i over-read",
        "i over read",
        "i should have just",
        "i should stop",
    ];
    let apology = [
        "sorry",
        "i apologize",
        "apologies",
        "you're right",
        "you are right",
        "that was wrong",
    ];
    let process_only = [
        "let me stop",
        "i'll stop exploring",
        "i will stop exploring",
        "i'll just do it",
        "i will just do it",
        "just doing it",
    ];

    let has_meta_marker = self_rebuke
        .iter()
        .chain(apology.iter())
        .chain(process_only.iter())
        .any(|marker| normalized.contains(marker));
    if !has_meta_marker {
        return false;
    }

    let has_concrete_work_marker = [
        "changed ",
        "updated ",
        "fixed ",
        "implemented ",
        "added ",
        "removed ",
        "ran ",
        "verified ",
        "tested ",
        "committed ",
        "pushed ",
        "blocked:",
        "blocker:",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));

    !has_concrete_work_marker
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_recovery_directive(message: &str) {
        assert!(message.contains("Do not apologize"));
        assert!(message.contains("self-criticize"));
        assert!(message.contains("mirror operator frustration"));
        assert!(message.contains("explain your process"));
    }

    #[test]
    fn continuation_pressure_messages_prohibit_meta_recovery() {
        for behavior in [BehavioralTier::Constrained, BehavioralTier::Standard] {
            for tier in [1, 2, 3] {
                let message = continuation_pressure_message(tier, behavior);
                assert_recovery_directive(&message);
            }
        }
    }

    #[test]
    fn task_mode_inference_classifies_research_prompts() {
        for prompt in [
            "what does the observation layer do?",
            "Explain the OODA loop wiring",
            "summarize the recent changes",
            "give me a rundown of loop.rs",
            "review the pressure heuristics",
            "How does compaction work",
            "investigate the flaky test",
            "can you check whether the tests pass",
        ] {
            assert_eq!(
                infer_task_mode_from_prompt(prompt),
                TaskMode::Research,
                "prompt should infer Research: {prompt}"
            );
        }
    }

    #[test]
    fn task_mode_inference_classifies_implementation_prompts() {
        for prompt in [
            "fix the bug in conversation.rs",
            "implement the observation normalizer",
            "add a regression test for orphaned tool results",
            "refactor the pressure tiers into policy rows",
            "commit the changes",
        ] {
            assert_eq!(
                infer_task_mode_from_prompt(prompt),
                TaskMode::Implementation,
                "prompt should infer Implementation: {prompt}"
            );
        }
    }

    #[test]
    fn repeated_observation_flow_makes_target_actionable_after_revisits() {
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[omegon_traits::ToolDefinition {
            name: "read".into(),
            label: String::new(),
            description: String::new(),
            parameters: serde_json::json!({}),
            capabilities: vec![
                omegon_traits::ToolCapability::RepoInspection,
                omegon_traits::ToolCapability::TargetedRepoInspection,
            ],
        }]);
        let mut conversation = ConversationState::new();
        let call = ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/crates/omegon/src/behavior.rs"}),
        };
        let result = ToolResultEntry {
            call_id: "1".into(),
            tool_name: "read".into(),
            content: vec![],
            is_error: false,
            args_summary: None,
        };
        conversation.intent.update_from_tools(
            &catalog,
            std::slice::from_ref(&call),
            std::slice::from_ref(&result),
        );
        conversation.intent.update_from_tools(
            &catalog,
            std::slice::from_ref(&call),
            std::slice::from_ref(&result),
        );
        conversation.intent.update_from_tools(
            &catalog,
            std::slice::from_ref(&call),
            std::slice::from_ref(&result),
        );
        let evidence = assess_evidence(&conversation, &catalog, &[call], &[result]);
        assert_eq!(evidence.local, EvidenceSufficiency::Actionable);
    }

    #[test]
    fn first_targeted_read_is_targeted_not_actionable() {
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[omegon_traits::ToolDefinition {
            name: "read".into(),
            label: String::new(),
            description: String::new(),
            parameters: serde_json::json!({}),
            capabilities: vec![omegon_traits::ToolCapability::RepoInspection],
        }]);
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert("core/crates/omegon/src/behavior.rs".into());
        let call = ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/crates/omegon/src/behavior.rs"}),
        };
        let evidence = assess_evidence(&conversation, &catalog, &[call], &[]);
        assert_eq!(evidence.local, EvidenceSufficiency::Targeted);
        assert_eq!(evidence.global, EvidenceSufficiency::None);
    }

    #[test]
    fn repeated_low_novelty_revisits_make_known_target_actionable() {
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[omegon_traits::ToolDefinition {
            name: "read".into(),
            label: String::new(),
            description: String::new(),
            parameters: serde_json::json!({}),
            capabilities: vec![
                omegon_traits::ToolCapability::RepoInspection,
                omegon_traits::ToolCapability::TargetedRepoInspection,
            ],
        }]);
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert("core/crates/omegon/src/behavior.rs".into());
        conversation
            .intent
            .evidence_ledger
            .turns
            .push(crate::conversation::EvidenceTurn {
                observations: 1,
                novel_paths: 0,
                revisits: 1,
                searches: 0,
                search_roots: Vec::new(),
                mutation_or_validation: false,
            });
        conversation
            .intent
            .evidence_ledger
            .turns
            .push(crate::conversation::EvidenceTurn {
                observations: 1,
                novel_paths: 0,
                revisits: 1,
                searches: 0,
                search_roots: Vec::new(),
                mutation_or_validation: false,
            });
        let call = ToolCall {
            id: "1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path": "core/crates/omegon/src/behavior.rs"}),
        };
        let evidence = assess_evidence(&conversation, &catalog, &[call], &[]);
        assert_eq!(evidence.local, EvidenceSufficiency::Actionable);
    }

    #[test]
    fn explicit_task_mode_marker_is_recognized() {
        assert_eq!(
            explicit_task_mode_from_prompt("/mode research\nreview the loop"),
            Some(TaskMode::Research)
        );
        assert_eq!(
            infer_task_mode_from_prompt("[mode: implementation]\nwhat file should change?"),
            TaskMode::Implementation
        );
    }

    #[test]
    fn research_mode_suppresses_orientation_churn_drift() {
        let catalog = ToolCapabilityCatalog::from_tool_defs(&[omegon_traits::ToolDefinition {
            name: "codebase_search".into(),
            label: String::new(),
            description: String::new(),
            parameters: serde_json::json!({}),
            capabilities: vec![
                omegon_traits::ToolCapability::RepoInspection,
                omegon_traits::ToolCapability::BroadRepoInspection,
            ],
        }]);
        let call = ToolCall {
            id: "1".into(),
            name: "codebase_search".into(),
            arguments: serde_json::json!({"query": "loop"}),
        };
        let mut conversation = ConversationState::new();
        conversation
            .intent
            .files_read
            .insert("core/crates/omegon/src/loop.rs".into());
        assert_eq!(
            classify_drift_kind(&catalog, 4, &conversation, std::slice::from_ref(&call), &[]),
            Some(DriftKind::OrientationChurn)
        );
        conversation.intent.pin_task_mode(TaskMode::Research);
        assert_eq!(
            classify_drift_kind(&catalog, 4, &conversation, &[call], &[]),
            None
        );
    }

    #[test]
    fn observed_task_mode_does_not_override_pinned_mode() {
        let mut conversation = ConversationState::new();
        conversation.intent.pin_task_mode(TaskMode::Research);
        conversation
            .intent
            .observe_task_mode(TaskMode::Implementation);
        assert_eq!(conversation.intent.task_mode, TaskMode::Research);

        let mut unpinned = ConversationState::new();
        unpinned.intent.observe_task_mode(TaskMode::Research);
        assert_eq!(unpinned.intent.task_mode, TaskMode::Research);
        unpinned.intent.observe_task_mode(TaskMode::Implementation);
        assert_eq!(unpinned.intent.task_mode, TaskMode::Implementation);
    }

    #[test]
    fn evidence_and_local_first_messages_prohibit_meta_recovery() {
        for behavior in [BehavioralTier::Constrained, BehavioralTier::Standard] {
            assert_recovery_directive(&evidence_sufficiency_message(behavior));
            assert_recovery_directive(&om_local_first_message(behavior));
        }
    }

    #[test]
    fn operator_correction_recovery_message_preserves_task_and_forces_action() {
        let message = operator_correction_recovery_message();
        assert!(message.contains("operator corrected your behavior"));
        assert!(message.contains("Preserve the active task"));
        assert!(message.contains("smallest concrete next action"));
        assert!(message.contains("Do not apologize"));
    }

    #[test]
    fn pathological_meta_response_detects_self_rebuke_without_progress() {
        assert!(is_pathological_meta_response(
            "You're right. I'm wasting turns reading things I already know."
        ));
        assert!(is_pathological_meta_response(
            "The user is frustrated. Let me stop exploring and just do it."
        ));
        assert!(!is_pathological_meta_response(
            "Updated src/main.rs and ran cargo test -p omegon."
        ));
        assert!(!is_pathological_meta_response(
            "Blocked: ssh requires an operator-provided key."
        ));
    }
}
