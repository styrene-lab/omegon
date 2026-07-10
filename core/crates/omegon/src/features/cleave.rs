//! Cleave Feature — task decomposition and parallel dispatch.
//!
//! Provides:
//! - Tool: `cleave_assess` — fast-path complexity assessment
//! - Tool: `cleave_run` — execute a cleave plan (spawn children, merge)
//! - Command: `/cleave` — trigger decomposition from TUI
//! - Dashboard state: live child progress during runs
//!
//! The orchestrator runs async in a spawned task. Progress events are
//! collected and surfaced through the dashboard and conversation segments.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::autonomy::{
    ApprovalRequest, DecisionPolicy, active_subagent_policy, required_approval_details,
    subagent_policy_for_automation,
};
use crate::child_agent::ChildTaskItem;
use crate::surfaces::conversation::ToolActivitySummary;
use crate::surfaces::operations::OperationWorkbenchProjection;

use omegon_traits::{
    AgentEvent, BusEvent, BusRequest, BusRequestSink, CommandDefinition, CommandResult,
    ContentBlock, Feature, OperationRef, PlanProgressProjection, PlanSurfaceProjection,
    PlanWorkstreamProjection, ToolDefinition, ToolResult,
};

/// Shared slot for the runtime-supplied [`BusRequestSink`].
///
/// CleaveFeature needs to emit `AgentEvent::Decomposition*` and
/// `AgentEvent::FamilyVitalSignsUpdated` events to surface tree state
/// changes to consumers (web dashboard, IPC / Auspex, eventual TUI).
/// The features layer is constructed in `setup.rs` *before* the runtime's
/// broadcast channel exists in `main.rs`, so we hand out a shared slot
/// at feature-construction time and let main.rs install a typed
/// `BusRequestSink` into it once the channel is up.
///
/// Going through `BusRequestSink` (rather than holding a
/// `broadcast::Sender<AgentEvent>` directly) keeps the cleave feature
/// decoupled from the transport layer — the runtime is the only thing
/// that touches the broadcast channel, and it routes incoming
/// `BusRequest::EmitAgentEvent` requests there. Features only see the
/// typed `BusRequest` contract.
///
/// Until the slot is populated, emissions are silently dropped — correct
/// for tests and for any code path that constructs the feature without
/// a live runtime.
pub type CleaveEventSlot = Arc<Mutex<Option<BusRequestSink>>>;

use crate::cleave::{
    self, CleavePlan,
    progress::{self, ChildProgressStatus, ProgressEvent},
    state::ChildStatus,
};

// ═══════════════════════════════════════════════════════════════════════════
// Complexity assessment — pure pattern matching
// ═══════════════════════════════════════════════════════════════════════════

/// Known patterns for fast-path assessment.
struct Pattern {
    id: &'static str,
    label: &'static str,
    keywords: &'static [&'static str],
    systems: u8,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        id: "crud-api",
        label: "CRUD / API Endpoint",
        keywords: &["endpoint", "api", "handler", "route", "crud", "rest"],
        systems: 2,
    },
    Pattern {
        id: "data-pipeline",
        label: "Data Pipeline / ETL",
        keywords: &["pipeline", "etl", "transform", "ingest", "export"],
        systems: 3,
    },
    Pattern {
        id: "ui-feature",
        label: "UI Feature / Component",
        keywords: &[
            "component",
            "widget",
            "view",
            "form",
            "dialog",
            "panel",
            "ui",
        ],
        systems: 2,
    },
    Pattern {
        id: "refactor",
        label: "Refactor / Rename",
        keywords: &[
            "refactor",
            "rename",
            "extract",
            "inline",
            "dedup",
            "consolidat",
        ],
        systems: 1,
    },
    Pattern {
        id: "infra-tooling",
        label: "Infrastructure & Tooling",
        keywords: &[
            "ci",
            "cd",
            "docker",
            "deploy",
            "container",
            "workflow",
            "script",
            "tool",
            "config",
            "lint",
            "format",
        ],
        systems: 1,
    },
    Pattern {
        id: "auth-security",
        label: "Auth / Security",
        keywords: &[
            "auth",
            "login",
            "permission",
            "rbac",
            "oauth",
            "token",
            "secret",
            "encrypt",
        ],
        systems: 3,
    },
    Pattern {
        id: "multi-service",
        label: "Multi-Service Integration",
        keywords: &[
            "service",
            "microservice",
            "grpc",
            "queue",
            "message",
            "event-driven",
            "kafka",
            "nats",
        ],
        systems: 4,
    },
    Pattern {
        id: "migration",
        label: "Data Migration / Schema Change",
        keywords: &[
            "migration",
            "schema",
            "alter",
            "migrate",
            "upgrade",
            "backward",
        ],
        systems: 2,
    },
    Pattern {
        id: "test-coverage",
        label: "Test Coverage / Quality",
        keywords: &["test", "coverage", "spec", "assert", "mock", "fixture"],
        systems: 1,
    },
    Pattern {
        id: "cross-cutting",
        label: "Cross-Cutting Concern",
        keywords: &[
            "logging",
            "tracing",
            "metrics",
            "telemetry",
            "i18n",
            "l10n",
            "error-handling",
        ],
        systems: 3,
    },
];

/// Modifiers that increase complexity.
const MODIFIERS: &[(&str, &[&str])] = &[
    (
        "validation",
        &["validate", "constraint", "schema", "boundary"],
    ),
    (
        "backward-compat",
        &["backward", "compatible", "deprecat", "legacy"],
    ),
    (
        "multi-platform",
        &["platform", "cross-platform", "os-specific", "arch"],
    ),
    (
        "performance",
        &["performance", "benchmark", "optimize", "cache", "latency"],
    ),
    (
        "concurrent",
        &["concurrent", "parallel", "async", "thread", "lock", "mutex"],
    ),
];

fn assess_directive(directive: &str, threshold: f64) -> Value {
    let lower = directive.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    // Find best matching pattern — use word-boundary matching:
    // a keyword matches if it equals a word OR the word starts with it
    // (catches "refactoring" for "refactor") but not substring matches
    // (avoids "stool" matching "tool", "build" matching "ui").
    let word_matches = |word: &str, kw: &str| -> bool {
        word == kw || word.starts_with(kw) && word.len() <= kw.len() + 4
    };

    let mut best: Option<(&Pattern, f64)> = None;
    for pattern in PATTERNS {
        let matches = pattern
            .keywords
            .iter()
            .filter(|kw| words.iter().any(|w| word_matches(w, kw)))
            .count();
        if matches > 0 {
            let confidence = (matches as f64 / pattern.keywords.len() as f64).min(1.0);
            if best.is_none() || confidence > best.unwrap().1 {
                best = Some((pattern, confidence));
            }
        }
    }

    // Count modifiers (same word-boundary matching)
    let active_modifiers: Vec<&str> = MODIFIERS
        .iter()
        .filter(|(_, kws)| {
            kws.iter()
                .any(|kw| words.iter().any(|w| word_matches(w, kw)))
        })
        .map(|(name, _)| *name)
        .collect();

    let (systems, pattern_label, pattern_id, confidence) = if let Some((p, conf)) = best {
        (p.systems as f64, p.label, p.id, conf)
    } else {
        (1.0, "Unknown", "unknown", 0.0)
    };

    let modifier_count = active_modifiers.len() as f64;
    let complexity = systems * (1.0 + 0.5 * modifier_count);
    let effective = complexity + 1.0; // +1 for validation offset

    let legacy_decision = if effective > threshold {
        "cleave"
    } else {
        "execute"
    };
    let strategy = assess_decomposition_strategy(
        directive,
        &lower,
        &words,
        legacy_decision,
        confidence,
        &active_modifiers,
    );

    json!({
        "decision": legacy_decision,
        "complexity": complexity,
        "systems": systems as u8,
        "modifiers": active_modifiers,
        "method": if confidence > 0.0 { "fast-path" } else { "needs_assessment" },
        "pattern": format!("{} ({}%)", pattern_label, (confidence * 100.0) as u8),
        "pattern_id": pattern_id,
        "confidence": confidence,
        "threshold": threshold,
        "strategy": strategy["strategy"].clone(),
        "confidence_breakdown": strategy["confidence_breakdown"].clone(),
        "warnings": strategy["warnings"].clone(),
        "assumptions": strategy["assumptions"].clone(),
        "evidence": strategy["evidence"].clone(),
        "approval": cleave_assessment_approval(legacy_decision, &strategy),
    })
}

fn cleave_assessment_approval(legacy_decision: &str, strategy: &Value) -> Value {
    let mode = strategy["strategy"]["mode"]
        .as_str()
        .unwrap_or("direct_execution");
    let requires = legacy_decision == "cleave"
        || matches!(mode, "parallel_cleave" | "sequential_children" | "hybrid");
    json!({
        "required": requires,
        "operation": "cleave_run",
        "surface": "menu",
        "workbench_role": "process_tree",
        "reason": if requires {
            "Cleave execution may launch cloves, create private workspaces, run long validation, and consume paid tokens; operator menu approval is required before execution."
        } else {
            "Assessment does not recommend cleave execution."
        },
        "actions": if requires {
            json!([
                "review_details",
                "approve_and_run",
                "deny",
                "view_evidence",
                "reassess"
            ])
        } else {
            json!(["review_details"])
        },
        "confirmation": {
            "required_for_high_cost": true,
            "prompt": "Approve and run cleave cloves? y/N"
        }
    })
}

fn assess_decomposition_strategy(
    directive: &str,
    lower: &str,
    words: &[&str],
    legacy_decision: &str,
    pattern_confidence: f64,
    modifiers: &[&str],
) -> Value {
    let explicit_paths: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| {
            w.contains('/')
                || w.ends_with(".rs")
                || w.ends_with(".ts")
                || w.ends_with(".tsx")
                || w.ends_with(".md")
                || w.ends_with(".toml")
                || w.ends_with(".json")
        })
        .collect();
    let has_scope = !explicit_paths.is_empty();
    let vague = pattern_confidence == 0.0
        || ["thing", "stuff", "better", "improve", "fix it", "make it"]
            .iter()
            .any(|needle| lower.contains(needle));
    let high_overlap = [
        "same file",
        "shared file",
        "single file",
        "one file",
        "overlap",
        "orchestrator.rs",
        "tui/mod.rs",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let spec_backed = [
        "openspec",
        "spec-backed",
        "scenario",
        "tasks.md",
        "acceptance",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let side_quest = [
        "scout",
        "inspect",
        "review",
        "verify",
        "run tests",
        "check results",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let dirty_or_submodule = ["dirty", "submodule", "uncommitted", "worktree"]
        .iter()
        .any(|needle| lower.contains(needle));
    let parallel_signal = [
        "parallel",
        "independent",
        "low-overlap",
        "separate",
        "workstreams",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    let (mode, rationale, overall, warnings) = if vague && !has_scope && !spec_backed {
        (
            "needs_scope_discovery",
            "Directive lacks concrete scope evidence; structural assessment must discover affected files/domains before decomposition.",
            0.35,
            vec!["no concrete scope evidence"],
        )
    } else if side_quest && !parallel_signal {
        (
            "lightweight_delegate",
            "Directive looks like a bounded side quest better handled by one delegate than cleave orchestration.",
            0.72,
            vec![],
        )
    } else if high_overlap {
        (
            "phased_execution",
            "Large work appears to share central files; parallel worktrees would raise merge risk, so use parent phases or sequential cloves.",
            0.70,
            vec!["shared write scope risk"],
        )
    } else if spec_backed && (parallel_signal || legacy_decision == "cleave") {
        (
            "parallel_cleave",
            "Spec/acceptance evidence plus cleave complexity supports bounded parallel workstreams if mechanical VCS gates pass.",
            0.84,
            vec![],
        )
    } else if legacy_decision == "cleave" && has_scope {
        (
            "sequential_children",
            "Task is complex and scoped, but evidence is insufficient for confident parallelism; use sequenced clove workstreams.",
            0.68,
            vec!["parallelism not yet structurally proven"],
        )
    } else {
        (
            "direct_execution",
            "Current evidence does not justify clove workstream orchestration.",
            0.62,
            vec![],
        )
    };

    let mut warnings_json: Vec<Value> = warnings
        .into_iter()
        .map(|message| json!({"kind": "assessment_gate", "message": message}))
        .collect();
    if dirty_or_submodule {
        warnings_json.push(json!({
            "kind": "vcs_checkpoint_required",
            "message": "Directive or scope mentions dirty tree/worktree/submodule ambiguity; checkpoint before private clove workspaces."
        }));
    }
    if mode == "parallel_cleave" && !spec_backed {
        warnings_json.push(json!({
            "kind": "acceptance_criteria_weak",
            "message": "Parallel cleave requires concrete acceptance criteria for every child."
        }));
    }

    let perforation_lines = if mode == "parallel_cleave" {
        json!([
            {
                "id": "foundation",
                "domain": "foundation/interface workstream",
                "rationale": "Initial foundation slice; replace with scope-graph-derived domain in v2 planner.",
                "write_scope": explicit_paths,
                "read_scope": [],
                "forbidden_scope": [],
                "depends_on": [],
                "acceptance": ["clove reports concrete diff and validation evidence"],
                "validation": ["run focused tests for owned scope"],
                "conflict_risk": "unknown",
                "confidence": overall
            }
        ])
    } else {
        json!([])
    };

    json!({
        "strategy": {
            "mode": mode,
            "rationale": rationale,
            "confidence": overall,
            "perforation_lines": perforation_lines,
            "waves": if mode == "parallel_cleave" { json!([["foundation"]]) } else { json!([]) },
            "parent_obligations": [
                "validate clove claims against harness-observed diffs/tests",
                "own merge, final validation, synthesis, and release-memory updates"
            ],
            "communication_policy": "ParentOnly"
        },
        "confidence_breakdown": {
            "scope_resolution": if has_scope { 0.75 } else { 0.35 },
            "dependency_graph": if spec_backed { 0.70 } else { 0.30 },
            "conflict_analysis": if high_overlap { 0.25 } else { 0.65 },
            "acceptance_criteria": if spec_backed { 0.80 } else { 0.35 },
            "vcs_substrate": if dirty_or_submodule { 0.40 } else { 0.75 },
            "model_judgment": pattern_confidence,
            "overall": overall
        },
        "warnings": warnings_json,
        "assumptions": if mode == "parallel_cleave" {
            json!(["scope graph and VCS gates must validate these draft perforation lines before cleave_run"])
        } else {
            json!([])
        },
        "evidence": [
            {"kind": "directive", "summary": directive},
            {"kind": "heuristic_pattern", "confidence": pattern_confidence, "modifiers": modifiers}
        ]
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Live progress tracking
// ═══════════════════════════════════════════════════════════════════════════

/// Live progress of an active cleave run, for dashboard rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleaveApprovalState {
    ApprovalRequired,
    Approved,
    Modified,
    Denied,
    Phased,
    Saved,
}

impl CleaveApprovalState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval_required",
            Self::Approved => "approved",
            Self::Modified => "modified",
            Self::Denied => "denied",
            Self::Phased => "phased",
            Self::Saved => "saved",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PendingCleaveApproval {
    pub id: String,
    pub directive: String,
    pub plan_json: String,
    pub max_parallel: usize,
    pub children: usize,
    pub state: CleaveApprovalState,
    pub modification_request: Option<String>,
    pub plan_digest: String,
    pub high_cost_confirmation: bool,
}

impl Default for PendingCleaveApproval {
    fn default() -> Self {
        Self {
            id: String::new(),
            directive: String::new(),
            plan_json: String::new(),
            max_parallel: 1,
            children: 0,
            state: CleaveApprovalState::ApprovalRequired,
            modification_request: None,
            plan_digest: String::new(),
            high_cost_confirmation: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct CleaveApprovalsState {
    version: u32,
    approvals: Vec<PendingCleaveApproval>,
}

impl Default for CleaveApprovalsState {
    fn default() -> Self {
        Self {
            version: 1,
            approvals: Vec::new(),
        }
    }
}

impl PendingCleaveApproval {
    fn is_high_cost(&self) -> bool {
        self.children > 1 || self.max_parallel > 1
    }

    fn new(
        id: String,
        directive: String,
        plan_json: String,
        max_parallel: usize,
        children: usize,
    ) -> Self {
        let plan_digest = cleave_plan_digest(&directive, &plan_json, max_parallel);
        Self {
            id,
            directive,
            plan_json,
            max_parallel,
            children,
            state: CleaveApprovalState::ApprovalRequired,
            modification_request: None,
            plan_digest,
            high_cost_confirmation: false,
        }
    }
}

#[derive(Default, Clone)]
pub struct CleaveProgress {
    pub active: bool,
    pub run_id: String,
    /// Inventory generation pinned for the complete run.
    pub inventory_generation: Option<u64>,
    pub total_children: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<ChildProgress>,
    /// Accumulated child input tokens — for parent session rollup.
    pub total_tokens_in: u64,
    /// Accumulated child output tokens — for parent session rollup.
    pub total_tokens_out: u64,
}

#[derive(Default, Clone)]
pub struct ChildRuntimeSummary {
    pub model: Option<String>,
    pub route_decision: Option<crate::subagent_route::SubagentRouteDecision>,
    pub thinking_level: Option<String>,
    pub context_class: Option<String>,
    pub enabled_tools: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub skills: Vec<String>,
    pub enabled_extensions: Vec<String>,
    pub disabled_extensions: Vec<String>,
    pub preloaded_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildSupervisionMode {
    Attached,
    RecoveredDegraded,
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleaveChildFailureKind {
    ChildProcessExit,
    IdleTimeout,
    WallTimeout,
    MergeConflict,
    ScopeViolation,
    UpstreamExhausted,
    ValidationFailed,
    Unknown,
}

#[derive(Clone)]
pub struct ChildProgress {
    pub label: String,
    pub status: String, // "pending", "running", "completed", "failed", "merged_after_failure", "upstream_exhausted"
    pub failure_kind: Option<CleaveChildFailureKind>,
    pub duration_secs: Option<f64>,
    /// Current supervision continuity for this child runtime.
    pub supervision_mode: Option<ChildSupervisionMode>,
    /// Spawned child PID while the orchestrator still owns the subprocess.
    pub pid: Option<u32>,
    /// Most recent tool active inside this child (e.g. "bash", "write").
    pub last_tool: Option<String>,
    pub last_tool_activity: Option<ToolActivitySummary>,
    /// Most recent turn number reported by this child.
    pub last_turn: Option<u32>,
    /// Task checklist items extracted from the child's prompt.
    pub tasks: Vec<ChildTaskItem>,
    /// Number of tasks marked done (explicit or heuristic).
    pub tasks_done: usize,
    /// Wall-clock instant when status transitioned to "running".
    pub started_at: Option<std::time::Instant>,
    /// Most recent progress/activity timestamp observed from this child.
    pub last_activity_at: Option<std::time::Instant>,
    /// Cumulative input tokens consumed by this child.
    pub tokens_in: u64,
    /// Cumulative output tokens consumed by this child.
    pub tokens_out: u64,
    pub runtime: Option<ChildRuntimeSummary>,
}

fn child_runtime_summary(
    runtime: &crate::cleave::CleaveChildRuntimeProfile,
) -> ChildRuntimeSummary {
    ChildRuntimeSummary {
        model: runtime.model.clone(),
        route_decision: None,
        thinking_level: runtime.thinking_level.clone(),
        context_class: runtime.context_class.clone(),
        enabled_tools: runtime.enabled_tools.clone(),
        disabled_tools: runtime.disabled_tools.clone(),
        skills: runtime.skills.clone(),
        enabled_extensions: runtime.enabled_extensions.clone(),
        disabled_extensions: runtime.disabled_extensions.clone(),
        preloaded_files: runtime.preloaded_files.clone(),
    }
}

fn recompute_progress_counts(progress: &mut CleaveProgress) {
    progress.completed = progress
        .children
        .iter()
        .filter(|child| matches!(child.status.as_str(), "completed" | "merged_after_failure"))
        .count();
    progress.failed = progress
        .children
        .iter()
        .filter(|child| matches!(child.status.as_str(), "failed" | "upstream_exhausted"))
        .count();
}

pub(crate) fn apply_progress_event(shared: &Arc<Mutex<CleaveProgress>>, event: &ProgressEvent) {
    let mut progress = shared.lock().unwrap();

    match event {
        ProgressEvent::ChildSpawned { child, pid } => {
            progress.active = true;
            let now = std::time::Instant::now();
            if let Some(existing) = progress.children.iter_mut().find(|c| c.label == *child) {
                existing.status = "running".into();
                existing.duration_secs = None;
                existing.supervision_mode = Some(ChildSupervisionMode::Attached);
                existing.pid = Some(*pid);
                existing.started_at = Some(now);
                existing.last_activity_at = Some(now);
            } else {
                progress.children.push(ChildProgress {
                    label: child.clone(),
                    status: "running".into(),
                    failure_kind: None,
                    duration_secs: None,
                    supervision_mode: Some(ChildSupervisionMode::Attached),
                    pid: Some(*pid),
                    last_tool: None,
                    last_tool_activity: None,
                    last_turn: None,
                    tasks: Vec::new(),
                    tasks_done: 0,
                    started_at: Some(now),
                    last_activity_at: Some(now),
                    tokens_in: 0,
                    tokens_out: 0,
                    runtime: None,
                });
                progress.total_children = progress.children.len();
            }
        }
        ProgressEvent::ChildActivity {
            child,
            turn,
            tool,
            target,
        } => {
            if let Some(c) = progress.children.iter_mut().find(|c| c.label == *child) {
                if let Some(t) = turn {
                    c.last_turn = Some(*t);
                    // Heuristic: turn N implies tasks 0..N-1 are done
                    if !c.tasks.is_empty() {
                        let heuristic = ((*t as usize).saturating_sub(1)).min(c.tasks.len());
                        if heuristic > c.tasks_done {
                            for task in c.tasks.iter_mut().take(heuristic) {
                                task.done = true;
                            }
                            c.tasks_done = heuristic;
                        }
                    }
                }
                if let Some(t) = tool {
                    c.last_tool_activity =
                        Some(ToolActivitySummary::new(t.clone(), target.clone()));
                    c.last_tool = Some(t.clone());
                }
                c.last_activity_at = Some(std::time::Instant::now());
            }
        }
        ProgressEvent::ChildTaskInventory { child, tasks, .. } => {
            if let Some(c) = progress.children.iter_mut().find(|c| c.label == *child)
                && !tasks.is_empty()
            {
                c.tasks = tasks.clone();
                c.tasks_done = tasks.iter().filter(|t| t.done).count();
            }
        }
        ProgressEvent::ChildTaskDone { child, task_index } => {
            if let Some(c) = progress.children.iter_mut().find(|c| c.label == *child)
                && *task_index > 0
                && *task_index <= c.tasks.len()
            {
                c.tasks[task_index - 1].done = true;
                c.tasks_done = c.tasks.iter().filter(|t| t.done).count();
            }
        }
        ProgressEvent::ChildTokens {
            child,
            input_tokens,
            output_tokens,
        } => {
            progress.total_tokens_in += input_tokens;
            progress.total_tokens_out += output_tokens;
            if let Some(c) = progress.children.iter_mut().find(|c| c.label == *child) {
                c.tokens_in += input_tokens;
                c.tokens_out += output_tokens;
                c.last_activity_at = Some(std::time::Instant::now());
            }
        }
        ProgressEvent::ChildStatus {
            child,
            status,
            duration_secs,
            ..
        } => {
            let status_text = match status {
                ChildProgressStatus::Completed => "completed",
                ChildProgressStatus::Failed => "failed",
                ChildProgressStatus::MergedAfterFailure => "merged_after_failure",
                ChildProgressStatus::UpstreamExhausted => "upstream_exhausted",
            };
            if let Some(existing) = progress.children.iter_mut().find(|c| c.label == *child) {
                existing.status = status_text.into();
                existing.duration_secs = *duration_secs;
                existing.supervision_mode = None;
                existing.pid = None;
                existing.last_activity_at = Some(std::time::Instant::now());
            } else {
                progress.children.push(ChildProgress {
                    label: child.clone(),
                    status: status_text.into(),
                    failure_kind: None,
                    duration_secs: *duration_secs,
                    supervision_mode: None,
                    pid: None,
                    last_tool: None,
                    last_tool_activity: None,
                    last_turn: None,
                    tasks: Vec::new(),
                    tasks_done: 0,
                    started_at: None,
                    last_activity_at: Some(std::time::Instant::now()),
                    tokens_in: 0,
                    tokens_out: 0,
                    runtime: None,
                });
                progress.total_children = progress.children.len();
            }
            recompute_progress_counts(&mut progress);
        }
        ProgressEvent::Done {
            completed, failed, ..
        } => {
            progress.active = false;
            progress.completed = *completed;
            progress.failed = *failed;
        }
        _ => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Feature implementation
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone)]
pub struct CleaveFeature {
    repo_path: PathBuf,
    /// Shared progress state — updated by the spawned orchestrator task,
    /// read by the dashboard renderer.
    progress: Arc<Mutex<CleaveProgress>>,
    /// Pending cleave approval states keyed by approval id. This is the
    /// harness/menu authority gate; Workbench may summarize it, but the menu
    /// owns operator choice.
    pending_approvals: Arc<Mutex<HashMap<String, PendingCleaveApproval>>>,
    /// In-process cancel handles for active cleave children, keyed by label.
    child_cancel_tokens: Arc<Mutex<HashMap<String, tokio_util::sync::CancellationToken>>>,
    /// Provider inventory for legacy per-child routing.
    pub inventory: Option<std::sync::Arc<tokio::sync::RwLock<crate::routing::ProviderInventory>>>,
    /// Shared inference runtime used to pin and resolve one snapshot per run.
    inference_runtime: Option<crate::inference_runtime::InferenceRuntimeState>,
    /// Startup-approved secret env inherited by child runs.
    session_secret_env: Vec<(String, String)>,
    /// Slot holding the runtime-supplied `BusRequestSink` once the
    /// runtime has constructed it. See [`CleaveEventSlot`] for the
    /// rationale.
    bus_request_sink: CleaveEventSlot,
    /// Live runtime settings used to resolve the selected subagent autonomy policy.
    settings: Option<crate::settings::SharedSettings>,
    sandbox: bool,
    dangerously_bypass_permissions: bool,
}

impl CleaveFeature {
    pub fn new(
        repo_path: &std::path::Path,
        session_secret_env: Vec<(String, String)>,
        sandbox: bool,
    ) -> Self {
        Self::new_with_safety(
            repo_path,
            session_secret_env,
            sandbox,
            std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok(),
        )
    }

    pub fn new_with_safety(
        repo_path: &std::path::Path,
        session_secret_env: Vec<(String, String)>,
        sandbox: bool,
        dangerously_bypass_permissions: bool,
    ) -> Self {
        let progress = Arc::new(Mutex::new(CleaveProgress::default()));
        let feature = Self {
            repo_path: repo_path.to_path_buf(),
            progress,
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            child_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
            inventory: None,
            inference_runtime: None,
            session_secret_env,
            bus_request_sink: Arc::new(Mutex::new(None)),
            settings: None,
            sandbox,
            dangerously_bypass_permissions,
        };
        feature.load_pending_approvals();
        feature.refresh_progress_from_workspace_state();
        feature
    }

    pub fn with_inference_runtime(
        mut self,
        runtime: crate::inference_runtime::InferenceRuntimeState,
    ) -> Self {
        self.inference_runtime = Some(runtime);
        self
    }

    pub fn with_settings(mut self, settings: crate::settings::SharedSettings) -> Self {
        self.settings = Some(settings);
        self
    }

    fn subagent_policy(&self) -> crate::autonomy::SubagentPolicy {
        self.settings
            .as_ref()
            .and_then(|settings| settings.lock().ok().map(|guard| guard.automation_level))
            .map(subagent_policy_for_automation)
            .unwrap_or_else(active_subagent_policy)
    }

    /// Hand out a clone of the bus-request slot so the runtime can install
    /// a typed [`BusRequestSink`] once the broadcast channel exists.
    /// Must be called *before* `bus.register(Box::new(feature))` consumes
    /// the typed `CleaveFeature`.
    pub fn event_sender_slot(&self) -> CleaveEventSlot {
        Arc::clone(&self.bus_request_sink)
    }

    /// Push an `AgentEvent` through the runtime's `BusRequestSink` if one
    /// is installed. Silently dropped otherwise — tests and headless runs
    /// don't have a runtime sink and shouldn't fail because of it.
    fn emit_decomposition_event(&self, event: AgentEvent) {
        if let Ok(slot) = self.bus_request_sink.lock()
            && let Some(sink) = slot.as_ref()
        {
            sink.send(BusRequest::EmitAgentEvent {
                event: Box::new(event),
            });
        }
    }

    fn emit_pending_approval_workstreams(&self) {
        let workstreams = self.approval_workstreams();
        if workstreams.is_empty() {
            return;
        }
        self.emit_decomposition_event(AgentEvent::PlanUpdated {
            projection: PlanSurfaceProjection {
                version: 1,
                active: None,
                workstreams,
                completed_session: None,
                reconciliation_issues: Vec::new(),
                promotion_nudges: Vec::new(),
                resume_candidates: Vec::new(),
            },
        });
    }

    fn approvals_state_path(&self) -> PathBuf {
        self.repo_path.join(".omegon/cleave/approvals.json")
    }

    fn load_pending_approvals(&self) {
        let path = self.approvals_state_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(mut state) = serde_json::from_str::<CleaveApprovalsState>(&content) else {
            tracing::warn!(path = %path.display(), "failed to parse cleave approvals state");
            return;
        };
        let mut approvals = HashMap::new();
        for mut approval in state.approvals.drain(..) {
            if approval.id.trim().is_empty() {
                continue;
            }
            approval.plan_digest = cleave_plan_digest(
                &approval.directive,
                &approval.plan_json,
                approval.max_parallel,
            );
            if approval.state == CleaveApprovalState::Approved {
                approval.state = CleaveApprovalState::ApprovalRequired;
                approval.high_cost_confirmation = false;
            }
            approvals.insert(approval.id.clone(), approval);
        }
        if let Ok(mut guard) = self.pending_approvals.lock() {
            *guard = approvals;
        }
    }

    fn save_pending_approvals(&self) {
        let path = self.approvals_state_path();
        let approvals = self
            .pending_approvals
            .lock()
            .map(|approvals| {
                let mut values = approvals.values().cloned().collect::<Vec<_>>();
                values.sort_by(|a, b| a.id.cmp(&b.id));
                values
            })
            .unwrap_or_default();
        let state = CleaveApprovalsState {
            version: 1,
            approvals,
        };
        let Ok(content) = serde_json::to_vec_pretty(&state) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(path = %parent.display(), error = %err, "failed to create cleave approval state directory");
            return;
        }
        let tmp = path.with_extension("json.tmp");
        if let Err(err) = std::fs::write(&tmp, content) {
            tracing::warn!(path = %tmp.display(), error = %err, "failed to write cleave approval state temp file");
            return;
        }
        if let Err(err) = std::fs::rename(&tmp, &path) {
            tracing::warn!(from = %tmp.display(), to = %path.display(), error = %err, "failed to install cleave approval state file");
        }
    }

    fn workspace_state_path(&self) -> PathBuf {
        self.repo_path.join(".omegon/cleave-workspace/state.json")
    }

    fn child_activity_log_path(&self, label: &str) -> PathBuf {
        self.repo_path
            .join(".omegon/cleave-workspace")
            .join(format!("child-{}.activity.log", label))
    }

    fn replay_child_activity_log(&self, progress: &mut ChildProgress) {
        let path = self.child_activity_log_path(&progress.label);
        let Ok(content) = std::fs::read_to_string(path) else {
            return;
        };
        for line in content.lines() {
            if let Some(event) =
                crate::cleave::progress::parse_child_activity(&progress.label, line)
            {
                match event {
                    crate::cleave::progress::ProgressEvent::ChildActivity {
                        turn,
                        tool,
                        target,
                        ..
                    } => {
                        if let Some(turn) = turn {
                            progress.last_turn = Some(turn);
                        }
                        if let Some(tool) = tool {
                            progress.last_tool_activity =
                                Some(ToolActivitySummary::new(tool.clone(), target));
                            progress.last_tool = Some(tool);
                        }
                    }
                    crate::cleave::progress::ProgressEvent::ChildTokens {
                        input_tokens,
                        output_tokens,
                        ..
                    } => {
                        progress.tokens_in = progress.tokens_in.saturating_add(input_tokens);
                        progress.tokens_out = progress.tokens_out.saturating_add(output_tokens);
                        if let Some(turn_pos) = line.find("Turn ") {
                            let rest = &line[turn_pos + 5..];
                            if let Some(num) = rest
                                .split_whitespace()
                                .next()
                                .and_then(|v| v.parse::<u32>().ok())
                            {
                                progress.last_turn = Some(num);
                            }
                        }
                    }
                    _ => {}
                }
                progress.last_activity_at = Some(std::time::Instant::now());
            }
        }
    }

    fn refresh_progress_from_workspace_state(&self) {
        let state_path = self.workspace_state_path();
        let raw_json = match std::fs::read_to_string(&state_path) {
            Ok(raw) => raw,
            Err(_) => return,
        };
        let raw_state: serde_json::Value = match serde_json::from_str(&raw_json) {
            Ok(value) => value,
            Err(_) => return,
        };
        let Ok(mut state) = crate::cleave::state::CleaveState::load(&state_path) else {
            return;
        };
        let reconciliation = state.reconcile_running_children();
        if reconciliation.requeued > 0 {
            let _ = state.save(&state_path);
        }
        let mut progress = self.progress.lock().unwrap();
        progress.run_id = state.run_id.clone();
        progress.total_children = state.children.len();
        progress.completed = state
            .children
            .iter()
            .filter(|c| c.status == ChildStatus::Completed)
            .count();
        progress.failed = state
            .children
            .iter()
            .filter(|c| c.status == ChildStatus::Failed)
            .count();
        progress.active = false;
        let raw_children = raw_state
            .get("children")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        progress.children = state
            .children
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                let raw_status = raw_children
                    .get(idx)
                    .and_then(|value| value.get("status"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let raw_pid = raw_children
                    .get(idx)
                    .and_then(|value| value.get("pid"))
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32);
                let mut child = ChildProgress {
                    label: c.label.clone(),
                    status: match c.status {
                        ChildStatus::Completed => "completed".into(),
                        ChildStatus::Failed => "failed".into(),
                        ChildStatus::UpstreamExhausted => "upstream_exhausted".into(),
                        ChildStatus::Running => "running".into(),
                        ChildStatus::Pending => "pending".into(),
                    },
                    failure_kind: match c.status {
                        ChildStatus::UpstreamExhausted => {
                            Some(CleaveChildFailureKind::UpstreamExhausted)
                        }
                        ChildStatus::Failed => Some(CleaveChildFailureKind::Unknown),
                        _ => None,
                    },
                    duration_secs: c.duration_secs,
                    supervision_mode: if c.status == ChildStatus::Running {
                        Some(ChildSupervisionMode::RecoveredDegraded)
                    } else if c.status == ChildStatus::Pending && raw_status == "running" {
                        Some(ChildSupervisionMode::Lost)
                    } else {
                        None
                    },
                    pid: if c.status == ChildStatus::Pending && raw_status == "running" {
                        raw_pid
                    } else {
                        c.pid
                    },
                    last_tool: None,
                    last_tool_activity: None,
                    last_turn: None,
                    tasks: Vec::new(),
                    tasks_done: 0,
                    started_at: None,
                    last_activity_at: None,
                    tokens_in: 0,
                    tokens_out: 0,
                    runtime: c.runtime.as_ref().map(child_runtime_summary),
                };
                self.replay_child_activity_log(&mut child);
                child
            })
            .collect();
        progress.active = progress.children.iter().any(|c| {
            matches!(
                c.supervision_mode,
                Some(
                    ChildSupervisionMode::Attached
                        | ChildSupervisionMode::RecoveredDegraded
                        | ChildSupervisionMode::Lost
                )
            ) || c.status == "running"
        });
    }

    /// Get a clone of the current progress for dashboard rendering.
    pub fn progress(&self) -> CleaveProgress {
        self.progress.lock().unwrap().clone()
    }

    fn render_status(progress: &CleaveProgress) -> String {
        if !progress.active && progress.total_children == 0 {
            return "No active cleave run.".into();
        }
        let mut lines = Vec::new();
        if progress.active {
            lines.push(format!(
                "Cleave active: {}/{} cloves ({} completed, {} failed)",
                progress.completed + progress.failed,
                progress.total_children,
                progress.completed,
                progress.failed
            ));
        } else {
            lines.push(format!(
                "Last cleave: {} completed, {} failed of {} cloves",
                progress.completed, progress.failed, progress.total_children
            ));
        }
        for child in &progress.children {
            let icon = match child.status.as_str() {
                "completed" | "merged_after_failure" => "✓",
                "failed" | "upstream_exhausted" => "✗",
                "running" => "⏳",
                _ => "○",
            };
            let dur = child
                .duration_secs
                .map(|d| format!(" duration={:.0}s", d))
                .unwrap_or_default();
            let pid = child
                .pid
                .map(|pid| format!(" pid={pid}"))
                .unwrap_or_default();
            let supervision = child
                .supervision_mode
                .map(|mode| format!(" supervision={mode:?}"))
                .unwrap_or_default();
            let last_tool = child
                .last_tool
                .as_deref()
                .map(|tool| format!(" tool={tool}"))
                .unwrap_or_default();
            let last_turn = child
                .last_turn
                .map(|turn| format!(" turn={turn}"))
                .unwrap_or_default();
            let tasks = if child.tasks.is_empty() {
                String::new()
            } else {
                format!(" tasks={}/{}", child.tasks_done, child.tasks.len())
            };
            let tokens = if child.tokens_in > 0 || child.tokens_out > 0 {
                format!(" tokens={}/{}", child.tokens_in, child.tokens_out)
            } else {
                String::new()
            };
            lines.push(format!(
                "  {} {} [{}]{}{}{}{}{}{}{}",
                icon,
                child.label,
                child.status,
                dur,
                pid,
                supervision,
                last_tool,
                last_turn,
                tasks,
                tokens
            ));
        }
        lines.join("\n")
    }

    /// Get a shared handle to the progress for live dashboard updates.
    pub fn shared_progress(&self) -> Arc<Mutex<CleaveProgress>> {
        Arc::clone(&self.progress)
    }

    /// Cancel a running child by label. Returns true if a live cancel handle existed
    /// or a persisted PID could be terminated.
    pub fn cancel_child(&self, label: &str) -> bool {
        let token = self
            .child_cancel_tokens
            .lock()
            .ok()
            .and_then(|map| map.get(label).cloned());
        if let Some(token) = token {
            token.cancel();
            return true;
        }

        let fallback_pid = self.progress.lock().ok().and_then(|progress| {
            progress
                .children
                .iter()
                .find(|child| child.label == label)
                .and_then(|child| child.pid)
        });

        let state_path = self.workspace_state_path();
        let Ok(mut state) = crate::cleave::state::CleaveState::load(&state_path) else {
            return false;
        };
        let Some(child) = state.children.iter_mut().find(|c| c.label == label) else {
            return false;
        };
        let Some(pid) = child.pid.or(fallback_pid) else {
            return false;
        };

        #[cfg(unix)]
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        child.status = ChildStatus::Failed;
        child.error = Some("cancelled via persisted pid fallback".into());
        child.pid = None;
        child.last_activity_unix_ms = None;
        child.started_at_unix_ms = None;
        let _ = state.save(&state_path);
        self.refresh_progress_from_workspace_state();
        true
    }

    fn record_pending_approval(
        &self,
        id: &str,
        directive: &str,
        plan_json: &str,
        max_parallel: usize,
        children: usize,
    ) {
        if let Ok(mut approvals) = self.pending_approvals.lock() {
            approvals.insert(
                id.to_string(),
                PendingCleaveApproval::new(
                    id.to_string(),
                    directive.to_string(),
                    plan_json.to_string(),
                    max_parallel,
                    children,
                ),
            );
        }
        self.save_pending_approvals();
        self.emit_pending_approval_workstreams();
    }

    fn update_pending_approval_state(
        &self,
        id: &str,
        state: CleaveApprovalState,
    ) -> Option<PendingCleaveApproval> {
        let mut approvals = self.pending_approvals.lock().ok()?;
        let approval = approvals.get_mut(id)?;
        approval.state = state;
        let approval = approval.clone();
        drop(approvals);
        self.save_pending_approvals();
        self.emit_pending_approval_workstreams();
        Some(approval)
    }

    fn confirm_high_cost_pending_approval(&self, id: &str) -> Option<PendingCleaveApproval> {
        let mut approvals = self.pending_approvals.lock().ok()?;
        let approval = approvals.get_mut(id)?;
        approval.high_cost_confirmation = true;
        let approval = approval.clone();
        drop(approvals);
        self.save_pending_approvals();
        self.emit_pending_approval_workstreams();
        Some(approval)
    }

    fn update_pending_approval_modification(
        &self,
        id: &str,
        request: String,
    ) -> Option<PendingCleaveApproval> {
        let mut approvals = self.pending_approvals.lock().ok()?;
        let approval = approvals.get_mut(id)?;
        approval.state = CleaveApprovalState::Modified;
        approval.modification_request = Some(request);
        let approval = approval.clone();
        drop(approvals);
        self.save_pending_approvals();
        self.emit_pending_approval_workstreams();
        Some(approval)
    }

    fn pending_approval(&self, id: &str) -> Option<PendingCleaveApproval> {
        self.pending_approvals.lock().ok()?.get(id).cloned()
    }

    fn cleave_run_has_approved_gate(&self, args: &Value) -> bool {
        if !cleave_run_has_menu_approval(args) {
            return false;
        }
        let Some(approval_id) = args
            .get("approval_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return false;
        };
        let Some(directive) = args.get("directive").and_then(Value::as_str) else {
            return false;
        };
        let Ok((plan_json, _)) = parse_plan_argument(args) else {
            return false;
        };
        let max_parallel = args["max_parallel"].as_u64().unwrap_or(1) as usize;
        let requested_digest = cleave_plan_digest(directive, &plan_json, max_parallel);
        self.pending_approval(approval_id).is_some_and(|approval| {
            approval.state == CleaveApprovalState::Approved
                && approval.plan_digest == requested_digest
                && (!approval.is_high_cost() || approval.high_cost_confirmation)
        })
    }

    fn approval_workstreams(&self) -> Vec<PlanWorkstreamProjection> {
        let mut approvals: Vec<PendingCleaveApproval> = self
            .pending_approvals
            .lock()
            .map(|approvals| approvals.values().cloned().collect())
            .unwrap_or_default();
        approvals.sort_by(|a, b| a.id.cmp(&b.id));
        approvals
            .into_iter()
            .map(|approval| {
                let (status, title_prefix) = match approval.state {
                    CleaveApprovalState::ApprovalRequired => {
                        ("pending_approval", "Cleave approval required")
                    }
                    CleaveApprovalState::Modified => {
                        ("pending_approval", "Cleave approval modification requested")
                    }
                    CleaveApprovalState::Approved => ("active", "Cleave approval approved"),
                    CleaveApprovalState::Denied => ("blocked", "Cleave approval denied"),
                    CleaveApprovalState::Phased => ("blocked", "Cleave approval marked phased"),
                    CleaveApprovalState::Saved => ("complete", "Cleave approval saved"),
                };
                PlanWorkstreamProjection {
                    id: format!("cleave:{}", approval.id),
                    title: format!(
                        "{title_prefix} — {} clove{} / max_parallel {}",
                        approval.children,
                        if approval.children == 1 { "" } else { "ren" },
                        approval.max_parallel
                    ),
                    status: status.into(),
                    progress: PlanProgressProjection {
                        completed: usize::from(approval.state == CleaveApprovalState::Approved),
                        total: approval.children.max(1),
                    },
                }
            })
            .collect()
    }

    pub fn pending_approval_workstreams(&self) -> Vec<PlanWorkstreamProjection> {
        let mut approvals: Vec<PendingCleaveApproval> = self
            .pending_approvals
            .lock()
            .map(|approvals| approvals.values().cloned().collect())
            .unwrap_or_default();
        approvals.sort_by(|a, b| a.id.cmp(&b.id));
        approvals
            .into_iter()
            .filter(|approval| {
                matches!(
                    approval.state,
                    CleaveApprovalState::ApprovalRequired | CleaveApprovalState::Modified
                )
            })
            .map(|approval| PlanWorkstreamProjection {
                id: format!("cleave:{}", approval.id),
                title: format!(
                    "Cleave approval required — {} clove{} / max_parallel {}",
                    approval.children,
                    if approval.children == 1 { "" } else { "ren" },
                    approval.max_parallel
                ),
                status: "pending_approval".into(),
                progress: PlanProgressProjection {
                    completed: 0,
                    total: approval.children.max(1),
                },
            })
            .collect()
    }

    fn approved_run_args_for_pending_approval(&self, id: &str) -> Option<Value> {
        let approval = self.pending_approval(id)?;
        if approval.state != CleaveApprovalState::Approved {
            return None;
        }
        Some(json!({
            "directive": approval.directive,
            "plan_json": approval.plan_json,
            "max_parallel": approval.max_parallel,
            "background": true,
            "approved": true,
            "approval_id": approval.id,
        }))
    }

    fn approval_command_response(&self, action: &str, rest: &str) -> CommandResult {
        let (id, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        let id = id.trim();
        let tail = tail.trim();
        if id.is_empty() {
            return CommandResult::Display(format!("Usage: /cleave {action} <approval-id>"));
        }
        let state = match action {
            "approve" => {
                let Some(existing) = self.pending_approval(id) else {
                    return CommandResult::Display(format!("No pending cleave approval '{id}'."));
                };
                if existing.is_high_cost() && tail != "confirm" {
                    return CommandResult::Display(format!(
                        "Cleave approval {id}: high-cost confirmation required for {} clove{} / max_parallel {}. Run `/cleave approve {id} confirm` to launch.",
                        existing.children,
                        if existing.children == 1 { "" } else { "ren" },
                        existing.max_parallel
                    ));
                }
                if existing.is_high_cost() {
                    let _ = self.confirm_high_cost_pending_approval(id);
                }
                CleaveApprovalState::Approved
            }
            "modify" => {
                if tail.is_empty() {
                    return CommandResult::Display(
                        "Usage: /cleave modify <approval-id> <change request>".into(),
                    );
                }
                if let Some(approval) =
                    self.update_pending_approval_modification(id, tail.to_string())
                {
                    return CommandResult::Display(format!(
                        "Cleave approval {id}: {}. Modification request recorded; approval remains blocked until the menu/action backend regenerates the plan.",
                        approval.state.as_str()
                    ));
                }
                return CommandResult::Display(format!("No pending cleave approval '{id}'."));
            }
            "deny" => CleaveApprovalState::Denied,
            "phased" => CleaveApprovalState::Phased,
            "save" => CleaveApprovalState::Saved,
            "evidence" => {
                if let Some(approval) = self.pending_approval(id) {
                    let modification = approval
                        .modification_request
                        .as_deref()
                        .map(|request| {
                            format!(
                                "
Modification request: {request}"
                            )
                        })
                        .unwrap_or_default();
                    return CommandResult::Display(format!(
                        "Cleave approval {id}: state={}, children={}, max_parallel={}, digest={}, high_cost_confirmed={}
Directive: {}{}",
                        approval.state.as_str(),
                        approval.children,
                        approval.max_parallel,
                        approval.plan_digest,
                        approval.high_cost_confirmation,
                        approval.directive,
                        modification
                    ));
                }
                return CommandResult::Display(format!("No pending cleave approval '{id}'."));
            }
            "reassess" => CleaveApprovalState::ApprovalRequired,
            _ => return CommandResult::NotHandled,
        };
        if let Some(approval) = self.update_pending_approval_state(id, state.clone()) {
            if state == CleaveApprovalState::Approved {
                let run_args = self
                    .approved_run_args_for_pending_approval(id)
                    .unwrap_or_else(|| json!({}));
                return CommandResult::Display(format!(
                    "Cleave approval {id}: approved and ready to run. The approval is bound to this exact cleave_run payload; menu surfaces should dispatch it directly instead of asking the operator to copy JSON.
{}",
                    serde_json::to_string_pretty(&run_args)
                        .unwrap_or_else(|_| run_args.to_string())
                ));
            }
            CommandResult::Display(format!(
                "Cleave approval {id}: {}. Workbench remains process-only; use the approval menu/action backend to continue.",
                approval.state.as_str()
            ))
        } else {
            CommandResult::Display(format!("No pending cleave approval '{id}'."))
        }
    }

    fn execute_assess(&self, args: &Value) -> anyhow::Result<ToolResult> {
        let directive = args["directive"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("directive required"))?;
        let threshold = args["threshold"].as_f64().unwrap_or(2.0);

        let assessment = assess_directive(directive, threshold);
        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: serde_json::to_string_pretty(&assessment)?,
            }],
            details: assessment,
        })
    }

    async fn execute_run(
        &self,
        args: &Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !background {
            return self.execute_run_attached(args, cancel, None).await;
        }

        let directive = args["directive"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("directive required"))?
            .to_string();
        let (plan_json, plan) = parse_plan_argument(args).map_err(anyhow::Error::msg)?;
        let max_parallel = args["max_parallel"].as_u64().unwrap_or(1) as usize;
        if !self.cleave_run_has_approved_gate(args) {
            let result = cleave_run_menu_approval_required(&plan, max_parallel, args);
            let approval_id = result.details["approval_id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(generated_cleave_approval_id);
            self.record_pending_approval(
                &approval_id,
                &directive,
                &plan_json,
                max_parallel,
                plan.children.len(),
            );
            return Ok(result);
        }
        let policy = self.subagent_policy();
        if let Some(result) = enforce_cleave_run_policy_with_policy(&policy, &plan, max_parallel) {
            return Ok(result);
        }

        let run_id = format!(
            "cleave-bg-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        {
            let mut prog = self.progress.lock().unwrap();
            prog.run_id = run_id.clone();
            prog.active = true;
            prog.total_children = plan.children.len();
            prog.completed = 0;
            prog.failed = 0;
            prog.children = plan
                .children
                .iter()
                .map(|c| ChildProgress {
                    label: c.label.clone(),
                    status: "pending".into(),
                    failure_kind: None,
                    duration_secs: None,
                    supervision_mode: None,
                    pid: None,
                    last_tool: None,
                    last_tool_activity: None,
                    last_turn: None,
                    tasks: Vec::new(),
                    tasks_done: 0,
                    started_at: None,
                    last_activity_at: None,
                    tokens_in: 0,
                    tokens_out: 0,
                    runtime: c.runtime.as_ref().map(child_runtime_summary),
                })
                .collect();
            prog.total_tokens_in = 0;
            prog.total_tokens_out = 0;
        }
        self.emit_decomposition_event(AgentEvent::DecompositionStarted {
            children: plan.children.iter().map(|c| c.label.clone()).collect(),
            operation: OperationRef::cleave(Some(run_id.clone())),
        });

        let feature = self.clone();
        let task_args = args.clone();
        let background_run_id = run_id.clone();
        crate::task_spawn::spawn_best_effort_result("cleave-background-run", async move {
            let background_cancel = tokio_util::sync::CancellationToken::new();
            if let Err(err) = feature
                .execute_run_attached(
                    &task_args,
                    background_cancel,
                    Some(background_run_id.clone()),
                )
                .await
            {
                tracing::warn!(error = %err, "background cleave run failed");
                if let Ok(mut prog) = feature.progress.lock() {
                    prog.active = false;
                    if prog.failed == 0 {
                        prog.failed = prog.total_children.saturating_sub(prog.completed);
                    }
                }
                feature.emit_decomposition_event(AgentEvent::DecompositionCompleted {
                    merged: false,
                    operation: OperationRef::cleave(Some(background_run_id.clone())),
                });
            }
            Ok(())
        });

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: serde_json::json!({
                    "run_id": run_id,
                    "background": true,
                    "status_hint": "/cleave status",
                })
                .to_string(),
            }],
            details: json!({ "run_id": run_id, "background": true }),
        })
    }

    async fn execute_run_attached(
        &self,
        args: &Value,
        cancel: tokio_util::sync::CancellationToken,
        operation_id: Option<String>,
    ) -> anyhow::Result<ToolResult> {
        let directive = args["directive"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("directive required"))?;
        let (plan_json, plan) = parse_plan_argument(args).map_err(anyhow::Error::msg)?;
        let max_parallel = args["max_parallel"].as_u64().unwrap_or(1) as usize;
        if !self.cleave_run_has_approved_gate(args) {
            let result = cleave_run_menu_approval_required(&plan, max_parallel, args);
            let approval_id = result.details["approval_id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(generated_cleave_approval_id);
            self.record_pending_approval(
                &approval_id,
                directive,
                &plan_json,
                max_parallel,
                plan.children.len(),
            );
            return Ok(result);
        }
        let policy = self.subagent_policy();
        if let Some(result) = enforce_cleave_run_policy_with_policy(&policy, &plan, max_parallel) {
            return Ok(result);
        }

        // Internal tool invocations should start from a fresh workspace.
        // Reusing a stale state.json from a previous run can mismatch the new
        // plan and panic when wave indices reference missing children.
        let workspace = self.repo_path.join(".omegon/cleave-workspace");
        if workspace.exists() {
            std::fs::remove_dir_all(&workspace)?;
        }
        std::fs::create_dir_all(&workspace)?;

        // Resolve agent binary
        let agent_binary = std::env::current_exe()?;

        // Initialize progress tracking
        {
            let mut prog = self.progress.lock().unwrap();
            prog.active = true;
            prog.total_children = plan.children.len();
            prog.completed = 0;
            prog.failed = 0;
            prog.children = plan
                .children
                .iter()
                .map(|c| ChildProgress {
                    label: c.label.clone(),
                    status: "pending".into(),
                    failure_kind: None,
                    duration_secs: None,
                    supervision_mode: None,
                    pid: None,
                    last_tool: None,
                    last_tool_activity: None,
                    last_turn: None,
                    tasks: Vec::new(),
                    tasks_done: 0,
                    started_at: None,
                    last_activity_at: None,
                    tokens_in: 0,
                    tokens_out: 0,
                    runtime: c.runtime.as_ref().map(child_runtime_summary),
                })
                .collect();
            prog.total_tokens_in = 0;
            prog.total_tokens_out = 0;
        }

        // ── Decomposition lifecycle event 1 of 3 ──────────────────────────
        // Tree exists and is about to start spawning children. Consumers
        // (web dashboard, IPC, TUI) get the list of child labels so they
        // can render placeholder rows immediately.
        self.emit_decomposition_event(AgentEvent::DecompositionStarted {
            children: plan.children.iter().map(|c| c.label.clone()).collect(),
            operation: OperationRef::cleave(operation_id.clone()),
        });

        let progress_sink = {
            let shared = self.shared_progress();
            let event_slot = self.event_sender_slot();
            let progress_operation_id = operation_id.clone();
            progress::callback_progress_sink(move |event| {
                // Update internal cleave progress state first.
                apply_progress_event(&shared, event);

                // Snapshot the bus sink once per callback so we don't
                // hold the slot lock across emissions. The sink itself
                // is `Clone` (Arc-backed) so cloning is cheap.
                let sink_snapshot = event_slot
                    .lock()
                    .ok()
                    .and_then(|slot| slot.as_ref().cloned());
                let Some(sink) = sink_snapshot else {
                    return;
                };

                // ── Decomposition lifecycle event 2 of 3 ──────────────
                // Per-child terminal transitions get surfaced as
                // DecompositionChildCompleted. The four terminal statuses
                // (Completed, Failed, MergedAfterFailure, UpstreamExhausted)
                // map to a single boolean: did the child do something
                // useful that contributed to the merged result?
                if let ProgressEvent::ChildStatus { child, status, .. } = event {
                    let success = matches!(
                        status,
                        ChildProgressStatus::Completed | ChildProgressStatus::MergedAfterFailure
                    );
                    sink.send(BusRequest::EmitAgentEvent {
                        event: Box::new(AgentEvent::DecompositionChildCompleted {
                            label: child.clone(),
                            success,
                            operation: OperationRef::cleave(progress_operation_id.clone()),
                        }),
                    });
                }

                // ── Family vital signs (L3 rollup) ─────────────────────
                // Snapshot the family tree on every progress event so
                // subscribers see fresh per-child digest without having
                // to reconstruct it from the per-event stream. The
                // orchestrator doesn't fire ProgressEvents at high
                // frequency (a handful per child per minute) so no
                // rate-limiting is needed here.
                if let Ok(progress_guard) = shared.lock() {
                    let signs = build_family_vital_signs(&progress_guard);
                    sink.send(BusRequest::EmitAgentEvent {
                        event: Box::new(AgentEvent::FamilyVitalSignsUpdated { signs }),
                    });
                }
            })
        };
        let child_cancel_tokens = Arc::clone(&self.child_cancel_tokens);

        let config = cleave::orchestrator::CleaveConfig {
            agent_binary,
            bridge_path: PathBuf::new(), // Not used in native mode
            node: String::new(),
            model: std::env::var("OMEGON_MODEL")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(crate::providers::automation_safe_model)
                .unwrap_or_else(|| "anthropic:claude-sonnet-4-6".into()),
            max_parallel,
            timeout_secs: 900,
            idle_timeout_secs: 180,
            max_turns: 50,
            inventory: self.inventory.clone().or_else(|| {
                // Probe on demand if no inventory was injected at startup
                Some(std::sync::Arc::new(tokio::sync::RwLock::new(
                    crate::routing::ProviderInventory::probe(),
                )))
            }),
            inherited_env: self.session_secret_env.clone(),
            injected_env: Vec::new(),
            child_runtime: crate::cleave::CleaveChildRuntimeProfile::default(),
            progress_sink,
            workflow: crate::workflow::discover_workflow(&self.repo_path),
            sandbox: self.sandbox,
            dangerously_bypass_permissions: self.dangerously_bypass_permissions,
        };

        let result = cleave::run_cleave(
            &plan,
            directive,
            &self.repo_path,
            &workspace,
            &config,
            cancel,
            Some(Arc::clone(&child_cancel_tokens)),
        )
        .await?;

        {
            let mut tokens = self.child_cancel_tokens.lock().unwrap();
            tokens.clear();
        }

        // ── Decomposition lifecycle event 3 of 3 ──────────────────────────
        // Run completed. `merged` is true iff at least one child's work
        // was successfully merged into the parent branch — this matches
        // the dashboard semantics ("did the family produce anything?")
        // rather than the per-child success tally, which is already
        // covered by the stream of DecompositionChildCompleted events.
        let merged = result
            .merge_results
            .iter()
            .any(|(_, outcome)| matches!(outcome, cleave::orchestrator::MergeOutcome::Success));
        self.emit_decomposition_event(AgentEvent::DecompositionCompleted {
            merged,
            operation: OperationRef::cleave(operation_id.clone()),
        });

        if should_cleanup_workspace(&result) {
            cleanup_workspace_dir(&workspace)?;
        }

        // Update progress to final state
        {
            let mut prog = self.progress.lock().unwrap();
            prog.active = false;
            prog.completed = result
                .state
                .children
                .iter()
                .filter(|c| c.status == ChildStatus::Completed)
                .count();
            prog.failed = result
                .state
                .children
                .iter()
                .filter(|c| {
                    c.status == ChildStatus::Failed || c.status == ChildStatus::UpstreamExhausted
                })
                .count();
            for (i, child) in result.state.children.iter().enumerate() {
                if let Some(p) = prog.children.get_mut(i) {
                    p.status = match child.status {
                        ChildStatus::Completed => {
                            if child.error.as_deref()
                                == Some("merged after salvaging work from a failed child")
                            {
                                "merged_after_failure".into()
                            } else {
                                "completed".into()
                            }
                        }
                        ChildStatus::Failed => "failed".into(),
                        ChildStatus::UpstreamExhausted => "upstream_exhausted".into(),
                        ChildStatus::Running => "running".into(),
                        ChildStatus::Pending => "pending".into(),
                    };
                    p.duration_secs = child.duration_secs;
                    p.supervision_mode = if child.status == ChildStatus::Running {
                        Some(ChildSupervisionMode::Attached)
                    } else {
                        None
                    };
                }
            }
        }

        // Build report
        let completed = result
            .state
            .children
            .iter()
            .filter(|c| c.status == ChildStatus::Completed)
            .count();
        let failed = result
            .state
            .children
            .iter()
            .filter(|c| c.status == ChildStatus::Failed)
            .count();
        let exhausted = result
            .state
            .children
            .iter()
            .filter(|c| c.status == ChildStatus::UpstreamExhausted)
            .count();

        let summary_suffix = if exhausted > 0 {
            format!(", {} upstream-exhausted", exhausted)
        } else {
            String::new()
        };

        let mut report = format!(
            "## Cleave Report: {}\n**Duration:** {:.0}s\n**Children:** {} completed, {} failed{} of {}\n\n",
            result.state.run_id,
            result.duration_secs,
            completed,
            failed,
            summary_suffix,
            result.state.children.len()
        );

        for child in &result.state.children {
            let icon = match child.status {
                ChildStatus::Completed => {
                    if child.error.as_deref()
                        == Some("merged after salvaging work from a failed child")
                    {
                        "↺"
                    } else {
                        "✓"
                    }
                }
                ChildStatus::Failed => "✗",
                ChildStatus::UpstreamExhausted => "↯",
                ChildStatus::Running => "⏳",
                ChildStatus::Pending => "○",
            };
            let dur = child
                .duration_secs
                .map(|d| format!(" ({:.0}s)", d))
                .unwrap_or_default();
            let model_note = child
                .execute_model
                .as_deref()
                .map(|m| format!(" `{m}`"))
                .unwrap_or_default();
            report.push_str(&format!(
                "  {} **{}**{}{}\n",
                icon, child.label, dur, model_note
            ));
            if child.error.as_deref() == Some("merged after salvaging work from a failed child") {
                report.push_str("    ↺ Worktree changes were salvaged and merged after the child hit a terminal execution failure.\n");
            } else if child.status == ChildStatus::UpstreamExhausted {
                report.push_str("    ↯ Provider upstream exhausted — check inventory for available fallbacks.\n");
            }
            if let Some(err) = &child.error {
                // Truncate long error details (stderr tails can be long)
                let short = if err.len() > 400 {
                    format!("{}…", crate::util::truncate_str(err, 400))
                } else {
                    err.clone()
                };
                report.push_str(&format!("    Error: {}\n", short));
            }
        }

        report.push_str("\n### Merge Results\n");
        for (label, outcome) in &result.merge_results {
            match outcome {
                cleave::orchestrator::MergeOutcome::Success => {
                    let child = result
                        .state
                        .children
                        .iter()
                        .find(|child| child.label == *label);
                    if child.and_then(|child| child.error.as_deref())
                        == Some("merged after salvaging work from a failed child")
                    {
                        report.push_str(&format!(
                            "  ↺ {} salvaged and merged after failure\n",
                            label
                        ));
                    } else {
                        report.push_str(&format!("  ✓ {} merged\n", label));
                    }
                }
                cleave::orchestrator::MergeOutcome::NoChanges => {
                    report.push_str(&format!("  ○ {} completed (no changes)\n", label));
                }
                cleave::orchestrator::MergeOutcome::Conflict(d) => {
                    report.push_str(&format!(
                        "  ✗ {} CONFLICT: {}\n",
                        label,
                        d.lines().next().unwrap_or("")
                    ));
                }
                cleave::orchestrator::MergeOutcome::Failed(d) => {
                    report.push_str(&format!(
                        "  ✗ {} FAILED: {}\n",
                        label,
                        d.lines().next().unwrap_or("")
                    ));
                }
                cleave::orchestrator::MergeOutcome::Skipped(d) => {
                    report.push_str(&format!("  ○ {} skipped: {}\n", label, d));
                }
            }
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text: report }],
            details: json!({
                "run_id": result.state.run_id,
                "completed": completed,
                "failed": failed,
                "total": result.state.children.len(),
                "duration_secs": result.duration_secs,
                "merged": result.merge_results.iter()
                    .filter(|(_, o)| matches!(o, cleave::orchestrator::MergeOutcome::Success))
                    .count(),
                "no_change": result.merge_results.iter()
                    .filter(|(_, o)| matches!(o, cleave::orchestrator::MergeOutcome::NoChanges))
                    .count(),
            }),
        })
    }
}

#[async_trait]
impl Feature for CleaveFeature {
    fn name(&self) -> &str {
        "cleave"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::cleave::CLEAVE_ASSESS.into(),
                label: "cleave_assess".into(),
                description: "Assess the complexity of a task directive to determine if it should be decomposed. Returns complexity score, matched pattern, and decision (execute/cleave).".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "directive": {
                            "type": "string",
                            "description": "The task directive to assess"
                        },
                        "threshold": {
                            "type": "number",
                            "description": "Complexity threshold (default: 2.0)"
                        }
                    },
                    "required": ["directive"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::cleave::CLEAVE_RUN.into(),
                label: "cleave_run".into(),
                description: "Execute a cleave decomposition plan. Creates git worktrees for each child, dispatches child processes, harvests results, and merges branches back.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "directive": {
                            "type": "string",
                            "description": "The original task directive"
                        },
                        "plan_json": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "description": "Preferred native split plan",
                                    "properties": {
                                        "children": {
                                            "type": "array",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "label": { "type": "string" },
                                                    "description": { "type": "string" },
                                                    "scope": { "type": "array", "items": { "type": "string" } },
                                                    "depends_on": { "type": "array", "items": { "type": "string" } },
                                                    "model": { "type": "string" },
                                                    "profile": { "type": "string", "enum": ["scout", "patch", "verify", "coordinator"] }
                                                },
                                                "required": ["label", "description", "scope"]
                                            }
                                        },
                                        "rationale": { "type": "string" },
                                        "default_model": { "type": "string" }
                                    },
                                    "required": ["children"]
                                },
                                {
                                    "type": "string",
                                    "description": "Legacy JSON-encoded split plan; native object form is preferred"
                                }
                            ]
                        },
                        "max_parallel": {
                            "type": "number",
                            "description": "Maximum parallel children. Defaults to 1 under conservative policy; runtime policy may permit more"
                        },
                        "background": {
                            "type": "boolean",
                            "description": "Run cleave in the background and return immediately (default: true)",
                            "default": true
                        },
                        "approved": {
                            "type": "boolean",
                            "description": "True only after the harness/operator cleave approval menu has approved this exact run plan",
                            "default": false
                        },
                        "approval_id": {
                            "type": "string",
                            "description": "Pending cleave approval identifier from the menu gate, when available"
                        }
                    },
                    "required": ["directive", "plan_json"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::StateChanging,
                    omegon_traits::ToolCapability::ProgressBoundary,
                ],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::cleave::CLEAVE_ASSESS => self.execute_assess(&args),
            crate::tool_registry::cleave::CLEAVE_RUN => self.execute_run(&args, cancel).await,
            _ => anyhow::bail!("Unknown tool: {tool_name}"),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "cleave".into(),
            description: "Show cleave status or trigger decomposition".into(),
            subcommands: vec![
                "status".into(),
                "cancel <label>".into(),
                "approve <id>".into(),
                "modify <id>".into(),
                "deny <id>".into(),
                "phased <id>".into(),
                "save <id>".into(),
                "evidence <id>".into(),
                "reassess <id>".into(),
            ],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::STATE_CHANGING,
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        match name {
            "cleave" => {
                let sub = args.trim();
                if sub == "status" || sub.is_empty() {
                    let prog = self.progress.lock().unwrap();
                    let pending_workstreams = self.pending_approval_workstreams();
                    if !prog.active && prog.total_children == 0 && pending_workstreams.is_empty() {
                        return CommandResult::Display("No active cleave run.".into());
                    }
                    if !prog.active && prog.total_children == 0 {
                        let mut lines = vec![format!(
                            "Pending cleave approval{}:",
                            if pending_workstreams.len() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        )];
                        for workstream in pending_workstreams {
                            lines.push(format!("  ○ {} — {}", workstream.id, workstream.title));
                        }
                        return CommandResult::Display(lines.join("\n"));
                    }
                    let projection = OperationWorkbenchProjection::from_cleave(&prog);
                    let mut lines = Vec::new();
                    if prog.active {
                        lines.push(format!(
                            "Cleave active: {}/{} cloves",
                            projection.completed + projection.failed,
                            prog.total_children
                        ));
                    } else {
                        lines.push(format!(
                            "Last cleave: {} completed, {} failed of {} cloves",
                            projection.completed, projection.failed, prog.total_children
                        ));
                    }
                    for (child, raw_child) in projection.children.iter().zip(&prog.children) {
                        let icon = match child.status.as_str() {
                            "completed" => "✓",
                            "failed" => "✗",
                            "running" => "⏳",
                            _ => "○",
                        };
                        let dur = raw_child
                            .duration_secs
                            .map(|d| format!(" ({:.0}s)", d))
                            .unwrap_or_default();
                        let failure = child
                            .failure
                            .as_ref()
                            .map(|failure| format!(" — {}", failure.kind.as_str()))
                            .unwrap_or_default();
                        lines.push(format!("  {} {}{}{}", icon, child.label, dur, failure));
                    }
                    CommandResult::Display(lines.join("\n"))
                } else if let Some((action, rest)) = sub.split_once(' ')
                    && matches!(
                        action,
                        "approve" | "modify" | "deny" | "phased" | "save" | "evidence" | "reassess"
                    )
                {
                    self.approval_command_response(action, rest.trim())
                } else if let Some(label) = sub.strip_prefix("cancel ").map(str::trim) {
                    if label.is_empty() {
                        CommandResult::Display("Usage: /cleave cancel <label>".into())
                    } else if self.cancel_child(label) {
                        CommandResult::Display(format!("Cancelling cleave child '{label}'..."))
                    } else {
                        CommandResult::Display(format!("No active cleave child '{label}'."))
                    }
                } else {
                    CommandResult::Display("Usage: /cleave [status|cancel <label>|approve <id>|modify <id>|deny <id>|phased <id>|save <id>|evidence <id>|reassess <id>]".into())
                }
            }
            _ => CommandResult::NotHandled,
        }
    }

    fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
        vec![]
    }
}

fn parse_plan_argument(args: &Value) -> Result<(String, CleavePlan), String> {
    let value = args
        .get("plan_json")
        .ok_or_else(|| "Missing plan_json".to_string())?;
    let encoded = match value {
        Value::String(encoded) => encoded.clone(),
        Value::Object(_) => serde_json::to_string(value)
            .map_err(|error| format!("Failed to encode native plan_json: {error}"))?,
        _ => return Err("plan_json must be an object or legacy JSON string".into()),
    };
    let plan = CleavePlan::from_json(&encoded).map_err(|error| format!("Invalid plan: {error}"))?;
    Ok((encoded, plan))
}

fn cleave_plan_digest(directive: &str, plan_json: &str, max_parallel: usize) -> String {
    // Stable FNV-1a digest over the authority-bearing run payload. This is not
    // a cryptographic boundary; it prevents accidental or model-side plan drift
    // between the approved menu item and the eventual cleave_run call.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in directive
        .as_bytes()
        .iter()
        .chain([0xff].iter())
        .chain(plan_json.as_bytes().iter())
        .chain([0xfe].iter())
        .chain(max_parallel.to_string().as_bytes().iter())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn generated_cleave_approval_id() -> String {
    static NEXT_APPROVAL_ID: AtomicU64 = AtomicU64::new(1);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = NEXT_APPROVAL_ID.fetch_add(1, Ordering::Relaxed);
    format!("cleave-approval-{millis}-{seq}")
}

fn cleave_run_has_menu_approval(args: &Value) -> bool {
    args.get("approved")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn cleave_run_menu_approval_required(
    plan: &CleavePlan,
    max_parallel: usize,
    args: &Value,
) -> ToolResult {
    let requested_children = plan.children.len();
    let approval_id = args
        .get("approval_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(generated_cleave_approval_id);
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Cleave approval required: open the approval menu for {approval_id} before launching {requested_children} clove workstream(s) with max_parallel={max_parallel}."
            ),
        }],
        details: json!({
            "approval_required": true,
            "kind": "cleave_menu_approval_required",
            "operation": "cleave_run",
            "approval_id": approval_id,
            "surface": "menu",
            "workbench_role": "process_tree_after_approval",
            "requested": {
                "children": requested_children,
                "max_parallel": max_parallel,
            },
            "menu": {
                "title": "Cleave approval required",
                "actions": [
                    {"id": "review_details", "label": "Review details", "hotkey": "enter"},
                    {"id": "approve_and_run", "label": "Approve and run", "hotkey": "a"},
                    {"id": "deny", "label": "Deny", "hotkey": "d"},
                    {"id": "view_evidence", "label": "View evidence", "hotkey": "v"},
                    {"id": "reassess", "label": "Reassess", "hotkey": "r"}
                ]
            }
        }),
    }
}

fn enforce_cleave_run_policy_with_policy(
    policy: &crate::autonomy::SubagentPolicy,
    plan: &CleavePlan,
    max_parallel: usize,
) -> Option<ToolResult> {
    if policy.cleave_run == DecisionPolicy::Allow {
        return None;
    }

    let requested_children = plan.children.len();
    let over_child_limit = requested_children > policy.max_children;
    let over_parallel_limit = max_parallel > policy.max_parallel;
    if !over_child_limit && !over_parallel_limit {
        return None;
    }

    let reason = match (over_child_limit, over_parallel_limit) {
        (true, true) => "cleave_run exceeds active child and parallelism limits",
        (true, false) => "cleave_run exceeds active child limit",
        (false, true) => "cleave_run exceeds active parallelism limit",
        (false, false) => unreachable!(),
    };
    Some(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Structured approval required: {reason}. Requested {requested_children} clove(s) with max_parallel={max_parallel}; active policy allows at most {} clove(s) and max_parallel={}.",
                policy.max_children, policy.max_parallel
            ),
        }],
        details: required_approval_details(
            policy,
            ApprovalRequest {
                operation: "cleave_run",
                reason,
                requested: json!({
                    "children": requested_children,
                    "max_parallel": max_parallel,
                }),
                allowed: json!({
                    "children": policy.max_children,
                    "max_parallel": policy.max_parallel,
                }),
                grants: vec![omegon_traits::AuthorityGrant::CleaveRun {
                    max_children: requested_children,
                    max_parallel,
                }],
            },
        ),
    })
}

fn enforce_cleave_run_policy(plan: &CleavePlan, max_parallel: usize) -> Option<ToolResult> {
    let policy = active_subagent_policy();
    enforce_cleave_run_policy_with_policy(&policy, plan, max_parallel)
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        details: json!(null),
    }
}

/// Snapshot the live `CleaveProgress` into the public typed
/// [`omegon_traits::FamilyVitalSigns`] shape that's carried on
/// `AgentEvent::FamilyVitalSignsUpdated`. The internal representation uses
/// `std::time::Instant` for timestamps; the public type uses absolute unix
/// milliseconds, so we convert via `(Instant::now(), SystemTime::now())`
/// at snapshot time to get a stable wall-clock anchor.
fn build_family_vital_signs(progress: &CleaveProgress) -> omegon_traits::FamilyVitalSigns {
    let now_instant = std::time::Instant::now();
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let to_unix_ms = |inst: std::time::Instant| -> u64 {
        // Saturate at 0 if the Instant is somehow ahead of "now" — should
        // never happen in practice but the conversion is fallible.
        let elapsed_ms = now_instant
            .checked_duration_since(inst)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now_unix_ms.saturating_sub(elapsed_ms)
    };

    let mut running = 0usize;
    let mut pending = 0usize;
    for child in &progress.children {
        match child.status.as_str() {
            "running" => running += 1,
            "pending" => pending += 1,
            _ => {}
        }
    }

    let children = progress
        .children
        .iter()
        .map(|c| omegon_traits::ChildVitalSigns {
            label: c.label.clone(),
            status: c.status.clone(),
            started_at_unix_ms: c.started_at.map(to_unix_ms),
            last_activity_unix_ms: c.last_activity_at.map(to_unix_ms),
            duration_secs: c.duration_secs,
            last_tool: c.last_tool.clone(),
            last_tool_activity: c.last_tool_activity.as_ref().map(|activity| {
                omegon_traits::ToolActivityVitalSigns {
                    raw_name: activity.raw_name.clone(),
                    args_summary: activity.args_summary.clone(),
                }
            }),
            last_turn: c.last_turn,
            tokens_in: c.tokens_in,
            tokens_out: c.tokens_out,
            tasks: c
                .tasks
                .iter()
                .map(|t| omegon_traits::VitalSignsTaskItem {
                    description: t.description.clone(),
                    done: t.done,
                })
                .collect(),
            tasks_done: c.tasks_done,
        })
        .collect();

    omegon_traits::FamilyVitalSigns {
        run_id: progress.run_id.clone(),
        active: progress.active,
        total_children: progress.total_children,
        completed: progress.completed,
        failed: progress.failed,
        running,
        pending,
        total_tokens_in: progress.total_tokens_in,
        total_tokens_out: progress.total_tokens_out,
        children,
    }
}

fn should_cleanup_workspace(result: &cleave::orchestrator::CleaveResult) -> bool {
    result
        .state
        .children
        .iter()
        .all(|child| child.status == ChildStatus::Completed)
}

fn cleanup_workspace_dir(workspace: &std::path::Path) -> anyhow::Result<()> {
    if workspace.exists() {
        std::fs::remove_dir_all(workspace)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use omegon_traits::OperationKind;

    #[test]
    fn cleave_run_menu_gate_blocks_unapproved_launch() {
        let plan = CleavePlan::from_json(
            r#"{
                "children": [
                    {"label":"one","description":"first","scope":["a.rs"]}
                ]
            }"#,
        )
        .unwrap();
        let args = json!({"approval_id": "cleave_27"});

        let result = cleave_run_menu_approval_required(&plan, 1, &args);

        assert_eq!(result.details["approval_required"], true);
        assert_eq!(result.details["kind"], "cleave_menu_approval_required");
        assert_eq!(result.details["operation"], "cleave_run");
        assert_eq!(result.details["approval_id"], "cleave_27");
        assert_eq!(result.details["surface"], "menu");
        assert_eq!(
            result.details["workbench_role"],
            "process_tree_after_approval"
        );
        assert_eq!(result.details["requested"]["children"], 1);
        let actions = result.details["menu"]["actions"].as_array().unwrap();
        assert!(
            actions
                .iter()
                .any(|action| action["id"] == "approve_and_run")
        );
        assert!(actions.iter().any(|action| action["id"] == "deny"));
        assert!(actions.iter().any(|action| action["id"] == "view_evidence"));
        assert!(
            actions
                .iter()
                .all(|action| action["id"] != "run_phased_in_parent")
        );
        assert!(actions.iter().all(|action| action["id"] != "modify_plan"));
        assert!(
            actions
                .iter()
                .all(|action| action["id"] != "save_assessment")
        );
    }

    #[test]
    fn cleave_approval_commands_update_pending_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_27",
            "do large work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );

        let approved = feature.handle_command("cleave", "approve cleave_27");
        match approved {
            CommandResult::Display(text) => assert!(text.contains("approved"), "{text}"),
            other => panic!("unexpected command result: {other:?}"),
        }
        assert_eq!(
            feature.pending_approval("cleave_27").unwrap().state,
            CleaveApprovalState::Approved
        );

        let evidence = feature.handle_command("cleave", "evidence cleave_27");
        match evidence {
            CommandResult::Display(text) => {
                assert!(text.contains("children=1"), "{text}");
                assert!(text.contains("Directive: do large work"), "{text}");
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }

    #[test]
    fn persisted_pending_approval_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let plan_json = r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#;
        feature.record_pending_approval("cleave_persist", "do persisted work", plan_json, 1, 1);

        let recovered = CleaveFeature::new(dir.path(), vec![], false);
        let approval = recovered.pending_approval("cleave_persist").unwrap();
        assert_eq!(approval.state, CleaveApprovalState::ApprovalRequired);
        assert_eq!(approval.directive, "do persisted work");
        assert_eq!(
            approval.plan_digest,
            cleave_plan_digest("do persisted work", plan_json, 1)
        );
    }

    #[test]
    fn persisted_modified_approval_survives_reload() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_modified",
            "do editable work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );
        feature.update_pending_approval_modification("cleave_modified", "remove docs child".into());

        let recovered = CleaveFeature::new(dir.path(), vec![], false);
        let approval = recovered.pending_approval("cleave_modified").unwrap();
        assert_eq!(approval.state, CleaveApprovalState::Modified);
        assert_eq!(
            approval.modification_request.as_deref(),
            Some("remove docs child")
        );
    }

    #[test]
    fn persisted_approved_approval_requires_review_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let plan_json = r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]},{"label":"two","description":"second","scope":["b.rs"]}]}"#;
        feature.record_pending_approval("cleave_approved", "do high cost work", plan_json, 2, 2);
        feature.confirm_high_cost_pending_approval("cleave_approved");
        feature.update_pending_approval_state("cleave_approved", CleaveApprovalState::Approved);

        let recovered = CleaveFeature::new(dir.path(), vec![], false);
        let approval = recovered.pending_approval("cleave_approved").unwrap();
        assert_eq!(approval.state, CleaveApprovalState::ApprovalRequired);
        assert!(!approval.high_cost_confirmation);
        assert!(!recovered.cleave_run_has_approved_gate(&json!({
            "approved": true,
            "approval_id": "cleave_approved",
            "directive": "do high cost work",
            "plan_json": plan_json,
            "max_parallel": 2
        })));
    }

    #[test]
    fn corrupt_persisted_approval_state_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".omegon/cleave");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("approvals.json"), b"not json").unwrap();

        let recovered = CleaveFeature::new(dir.path(), vec![], false);
        assert!(recovered.pending_approval("anything").is_none());
        assert!(recovered.pending_approval_workstreams().is_empty());
    }

    #[test]
    fn recovered_pending_approval_projects_to_workbench_row() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_recovered",
            "do recovered work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );

        let recovered = CleaveFeature::new(dir.path(), vec![], false);
        let rows = recovered.pending_approval_workstreams();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "cleave:cleave_recovered");
        assert_eq!(rows[0].status, "pending_approval");
    }

    #[test]
    fn pending_cleave_approval_projects_as_workstream_summary() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_29",
            "do visible work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            2,
            3,
        );

        let workstreams = feature.pending_approval_workstreams();

        assert_eq!(workstreams.len(), 1);
        assert_eq!(workstreams[0].id, "cleave:cleave_29");
        assert_eq!(workstreams[0].status, "pending_approval");
        assert_eq!(workstreams[0].progress.completed, 0);
        assert_eq!(workstreams[0].progress.total, 3);
        assert!(workstreams[0].title.contains("approval required"));
    }

    #[test]
    fn cleave_status_lists_pending_approval_rows() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_30",
            "do blocked work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            2,
            3,
        );

        let result = feature.handle_command("cleave", "status");

        match result {
            CommandResult::Display(text) => {
                assert!(text.contains("Pending cleave approval"), "{text}");
                assert!(text.contains("cleave:cleave_30"), "{text}");
                assert!(text.contains("3 cloveren / max_parallel 2"), "{text}");
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }

    #[test]
    fn cleave_approval_commands_cover_alternate_states_and_missing_ids() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_28",
            "do alternate work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );

        for (command, expected) in [
            ("deny cleave_28", CleaveApprovalState::Denied),
            ("phased cleave_28", CleaveApprovalState::Phased),
            ("save cleave_28", CleaveApprovalState::Saved),
            ("reassess cleave_28", CleaveApprovalState::ApprovalRequired),
        ] {
            let result = feature.handle_command("cleave", command);
            match result {
                CommandResult::Display(text) => {
                    assert!(text.contains(expected.as_str()), "{command}: {text}");
                }
                other => panic!("unexpected command result for {command}: {other:?}"),
            }
            assert_eq!(
                feature.pending_approval("cleave_28").unwrap().state,
                expected
            );
        }

        let missing = feature.handle_command("cleave", "deny missing");
        match missing {
            CommandResult::Display(text) => {
                assert!(text.contains("No pending cleave approval"), "{text}")
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }

    #[test]
    fn approved_gate_requires_matching_approved_pending_state_when_id_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_32",
            "do gated work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );

        let matching_args = json!({
            "approved": true,
            "approval_id": "cleave_32",
            "directive": "do gated work",
            "plan_json": r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            "max_parallel": 1
        });

        assert!(!feature.cleave_run_has_approved_gate(&matching_args));

        feature.update_pending_approval_state("cleave_32", CleaveApprovalState::Approved);
        assert!(feature.cleave_run_has_approved_gate(&matching_args));
        assert!(!feature.cleave_run_has_approved_gate(&json!({
            "approved": true,
            "approval_id": "cleave_32",
            "directive": "changed directive",
            "plan_json": r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            "max_parallel": 1
        })));
        assert!(!feature.cleave_run_has_approved_gate(&json!({
            "approved": true,
            "approval_id": "missing",
            "directive": "do gated work",
            "plan_json": r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            "max_parallel": 1
        })));
    }

    fn cleave_modify_records_change_request_and_evidence_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.record_pending_approval(
            "cleave_31",
            "do editable work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );

        let modified = feature.handle_command(
            "cleave",
            "modify cleave_31 remove docs child and cap max_parallel at 1",
        );
        match modified {
            CommandResult::Display(text) => assert!(text.contains("modified"), "{text}"),
            other => panic!("unexpected command result: {other:?}"),
        }
        let approval = feature.pending_approval("cleave_31").unwrap();
        assert_eq!(approval.state, CleaveApprovalState::Modified);
        assert_eq!(
            approval.modification_request.as_deref(),
            Some("remove docs child and cap max_parallel at 1")
        );

        let evidence = feature.handle_command("cleave", "evidence cleave_31");
        match evidence {
            CommandResult::Display(text) => {
                assert!(text.contains("Modification request"), "{text}");
                assert!(text.contains("remove docs child"), "{text}");
            }
            other => panic!("unexpected command result: {other:?}"),
        }
    }

    #[test]
    fn approved_gate_rejects_non_approved_states_and_payload_drift() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let plan_json = r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#;
        let args = json!({
            "approved": true,
            "approval_id": "cleave_34",
            "directive": "do gated work",
            "plan_json": plan_json,
            "max_parallel": 1
        });

        for state in [
            CleaveApprovalState::Denied,
            CleaveApprovalState::Modified,
            CleaveApprovalState::Phased,
            CleaveApprovalState::Saved,
            CleaveApprovalState::ApprovalRequired,
        ] {
            feature.record_pending_approval("cleave_34", "do gated work", plan_json, 1, 1);
            feature.update_pending_approval_state("cleave_34", state);
            assert!(!feature.cleave_run_has_approved_gate(&args));
        }

        feature.record_pending_approval("cleave_34", "do gated work", plan_json, 1, 1);
        feature.update_pending_approval_state("cleave_34", CleaveApprovalState::Approved);
        assert!(!feature.cleave_run_has_approved_gate(&json!({
            "approved": true,
            "approval_id": "cleave_34",
            "directive": "do gated work",
            "plan_json": r#"{"children":[{"label":"changed","description":"first","scope":["a.rs"]}]}"#,
            "max_parallel": 1
        })));
        assert!(!feature.cleave_run_has_approved_gate(&json!({
            "approved": true,
            "approval_id": "cleave_34",
            "directive": "do gated work",
            "plan_json": plan_json,
            "max_parallel": 2
        })));
        assert!(!feature.cleave_run_has_approved_gate(&json!({
            "approval_id": "cleave_34",
            "directive": "do gated work",
            "plan_json": plan_json,
            "max_parallel": 1
        })));
    }

    #[test]
    fn high_cost_gate_rejects_approved_state_without_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let plan_json = r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]},{"label":"two","description":"second","scope":["b.rs"]}]}"#;
        feature.record_pending_approval("cleave_35", "do high cost work", plan_json, 2, 2);
        feature.update_pending_approval_state("cleave_35", CleaveApprovalState::Approved);

        let args = json!({
            "approved": true,
            "approval_id": "cleave_35",
            "directive": "do high cost work",
            "plan_json": plan_json,
            "max_parallel": 2
        });

        assert!(!feature.cleave_run_has_approved_gate(&args));
        feature.confirm_high_cost_pending_approval("cleave_35");
        assert!(feature.cleave_run_has_approved_gate(&args));
    }

    #[test]
    fn generated_cleave_approval_ids_are_unique_under_rapid_calls() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..128 {
            let id = generated_cleave_approval_id();
            assert!(seen.insert(id));
        }
    }

    #[test]
    fn high_cost_approval_requires_second_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![], false);
        let plan_json = r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]},{"label":"two","description":"second","scope":["b.rs"]}]}"#;
        feature.record_pending_approval("cleave_33", "do high cost work", plan_json, 2, 2);

        let args = json!({
            "approved": true,
            "approval_id": "cleave_33",
            "directive": "do high cost work",
            "plan_json": plan_json,
            "max_parallel": 2
        });

        let first = feature.handle_command("cleave", "approve cleave_33");
        match first {
            CommandResult::Display(text) => {
                assert!(text.contains("high-cost confirmation required"), "{text}");
                assert!(text.contains("approve cleave_33 confirm"), "{text}");
            }
            other => panic!("unexpected command result: {other:?}"),
        }
        let pending = feature.pending_approval("cleave_33").unwrap();
        assert_eq!(pending.state, CleaveApprovalState::ApprovalRequired);
        assert!(!pending.high_cost_confirmation);
        assert!(!feature.cleave_run_has_approved_gate(&args));

        let confirmed = feature.handle_command("cleave", "approve cleave_33 confirm");
        match confirmed {
            CommandResult::Display(text) => {
                assert!(text.contains("approved"), "{text}");
                assert!(text.contains("approved and ready to run"), "{text}");
            }
            other => panic!("unexpected command result: {other:?}"),
        }
        let approved = feature.pending_approval("cleave_33").unwrap();
        assert_eq!(approved.state, CleaveApprovalState::Approved);
        assert!(approved.high_cost_confirmation);
        assert!(feature.cleave_run_has_approved_gate(&args));
    }

    #[test]
    fn cleave_run_policy_requires_approval_when_over_conservative_limits() {
        let plan = CleavePlan::from_json(
            r#"{
                "children": [
                    {"label":"one","description":"first","scope":["a.rs"]},
                    {"label":"two","description":"second","scope":["b.rs"]},
                    {"label":"three","description":"third","scope":["c.rs"]}
                ]
            }"#,
        )
        .unwrap();

        let result = enforce_cleave_run_policy(&plan, 2).expect("over-limit cleave must gate");
        assert_eq!(result.details["approval_required"], true);
        assert_eq!(result.details["operation"], "cleave_run");
        assert_eq!(result.details["autonomy"], "conservative");
        assert_eq!(result.details["requested"]["children"], 3);
        assert_eq!(result.details["requested"]["max_parallel"], 2);
        assert_eq!(result.details["allowed"]["children"], 2);
        assert_eq!(result.details["allowed"]["max_parallel"], 1);
        assert_eq!(
            result.details["required_approval"]["kind"],
            "approval_required"
        );
        assert_eq!(
            result.details["required_approval"]["operation"],
            "cleave_run"
        );
        assert_eq!(
            result.details["required_approval"]["autonomy"],
            "conservative"
        );
        assert_eq!(
            result.details["required_approval"]["choices"][0]["grants"][0]["kind"],
            "cleave_run"
        );
    }

    #[test]
    fn cleave_run_policy_allows_within_conservative_limits() {
        let plan = CleavePlan::from_json(
            r#"{
                "children": [
                    {"label":"one","description":"first","scope":["a.rs"]}
                ]
            }"#,
        )
        .unwrap();

        assert!(enforce_cleave_run_policy(&plan, 1).is_none());
    }

    #[test]
    fn cleave_run_policy_allows_orchestrator_policy_with_larger_fanout() {
        let plan = CleavePlan::from_json(
            r#"{
                "children": [
                    {"label":"one","description":"first","scope":["a.rs"]},
                    {"label":"two","description":"second","scope":["b.rs"]},
                    {"label":"three","description":"third","scope":["c.rs"]}
                ]
            }"#,
        )
        .unwrap();
        let policy = crate::autonomy::SubagentPolicy::for_level(
            crate::autonomy::AutonomyLevel::Orchestrator,
        );

        assert!(enforce_cleave_run_policy_with_policy(&policy, &plan, 2).is_none());
    }

    #[test]
    fn cleave_feature_resolves_live_settings_policy() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        settings.lock().unwrap().automation_level = crate::settings::AutomationLevel::Autonomous;
        let feature = CleaveFeature::new(dir.path(), vec![], false).with_settings(settings);

        let policy = feature.subagent_policy();
        assert_eq!(policy.level, crate::autonomy::AutonomyLevel::Orchestrator);
        assert_eq!(policy.cleave_run, DecisionPolicy::Allow);
        assert_eq!(policy.max_children, 8);
        assert_eq!(policy.max_parallel, 4);
    }

    #[test]
    fn cleave_feature_falls_back_to_conservative_policy_without_settings() {
        let dir = tempfile::TempDir::new().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);

        let policy = feature.subagent_policy();
        assert_eq!(policy.level, crate::autonomy::AutonomyLevel::Conservative);
        assert_eq!(policy.cleave_run, DecisionPolicy::RequireApproval);
        assert_eq!(policy.max_children, 2);
        assert_eq!(policy.max_parallel, 1);
    }

    #[test]
    fn assess_simple_directive() {
        let result = assess_directive("Refactor the utils module to extract helpers", 2.0);
        assert_eq!(
            result["decision"], "execute",
            "simple refactor should be execute: {result}"
        );
        assert!(result["complexity"].as_f64().unwrap() >= 1.0);
    }

    #[test]
    fn assess_complex_directive() {
        let result = assess_directive(
            "Build a multi-service integration with gRPC, authentication, and backward compatibility for legacy clients with concurrent processing",
            2.0,
        );
        assert_eq!(result["decision"], "cleave");
        assert!(result["complexity"].as_f64().unwrap() >= 3.0);
    }

    #[test]
    fn assess_complex_directive_requires_menu_approval() {
        let result = assess_directive(
            "Build a multi-service integration with gRPC, authentication, and backward compatibility for legacy clients with concurrent processing",
            2.0,
        );

        assert_eq!(result["approval"]["required"], true);
        assert_eq!(result["approval"]["operation"], "cleave_run");
        assert_eq!(result["approval"]["surface"], "menu");
        assert_eq!(result["approval"]["workbench_role"], "process_tree");
        let actions = result["approval"]["actions"].as_array().unwrap();
        assert!(actions.iter().any(|action| action == "approve_and_run"));
        assert!(actions.iter().any(|action| action == "view_evidence"));
        assert!(actions.iter().all(|action| action != "modify_plan"));
        assert!(
            actions
                .iter()
                .all(|action| action != "run_phased_in_parent")
        );
        assert!(actions.iter().all(|action| action != "save_assessment"));
    }

    #[test]
    fn assess_unknown_pattern() {
        let result = assess_directive("do something vague", 2.0);
        assert_eq!(result["method"], "needs_assessment");
    }

    #[test]
    fn assess_with_modifiers() {
        let result = assess_directive(
            "Deploy a containerized service with performance optimization and backward compatibility",
            2.0,
        );
        let mods = result["modifiers"].as_array().unwrap();
        assert!(!mods.is_empty(), "should detect modifiers");
    }

    #[test]
    fn feature_provides_tools() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let tools = feature.tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "cleave_assess"));
        assert!(tools.iter().any(|t| t.name == "cleave_run"));
    }

    #[test]
    fn cleave_status_no_active_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![], false);
        let result = feature.handle_command("cleave", "status");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("No active")));
    }

    #[test]
    fn event_slot_starts_empty_and_drops_emissions() {
        // Without a sender installed, emit_decomposition_event must be a
        // silent no-op (used in tests, headless runs, anywhere without an
        // event bus). Reaching this assertion at all proves we don't panic.
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        feature.emit_decomposition_event(AgentEvent::DecompositionStarted {
            children: vec!["a".into(), "b".into()],
            operation: OperationRef::cleave(None),
        });
        assert!(
            feature.event_sender_slot().lock().unwrap().is_none(),
            "slot should still be empty"
        );
    }

    #[test]
    fn build_family_vital_signs_snapshots_progress_state() {
        // Construct a CleaveProgress with a mix of statuses and partial
        // per-child fields and confirm the snapshot helper produces a
        // typed FamilyVitalSigns with the right derived counts and
        // children in plan order.
        let mut prog = CleaveProgress::default();
        prog.run_id = "test-run".into();
        prog.active = true;
        prog.total_children = 3;
        prog.completed = 1;
        prog.failed = 0;
        prog.total_tokens_in = 1500;
        prog.total_tokens_out = 750;
        prog.children = vec![
            ChildProgress {
                label: "alpha".into(),
                status: "completed".into(),
                failure_kind: None,
                duration_secs: Some(12.5),
                supervision_mode: None,
                pid: None,
                last_tool: Some("commit".into()),
                last_tool_activity: None,
                last_turn: Some(8),
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: Some(std::time::Instant::now()),
                last_activity_at: Some(std::time::Instant::now()),
                tokens_in: 800,
                tokens_out: 400,
                runtime: None,
            },
            ChildProgress {
                label: "beta".into(),
                status: "running".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: Some(42_u32),
                last_tool: Some("write".into()),
                last_tool_activity: None,
                last_turn: Some(3),
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: Some(std::time::Instant::now()),
                last_activity_at: Some(std::time::Instant::now()),
                tokens_in: 700,
                tokens_out: 350,
                runtime: None,
            },
            ChildProgress {
                label: "gamma".into(),
                status: "pending".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_tool_activity: None,
                last_turn: None,
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: None,
                last_activity_at: None,
                tokens_in: 0,
                tokens_out: 0,
                runtime: None,
            },
        ];

        let signs = build_family_vital_signs(&prog);
        assert_eq!(signs.run_id, "test-run");
        assert!(signs.active);
        assert_eq!(signs.total_children, 3);
        assert_eq!(signs.completed, 1);
        assert_eq!(signs.failed, 0);
        // Derived counts from the children list
        assert_eq!(signs.running, 1, "one child in running status");
        assert_eq!(signs.pending, 1, "one child in pending status");
        assert_eq!(signs.total_tokens_in, 1500);
        assert_eq!(signs.total_tokens_out, 750);
        assert_eq!(signs.children.len(), 3);
        assert_eq!(signs.children[0].label, "alpha");
        assert_eq!(signs.children[0].status, "completed");
        assert_eq!(signs.children[0].duration_secs, Some(12.5));
        assert!(signs.children[0].started_at_unix_ms.is_some());
        assert_eq!(signs.children[1].label, "beta");
        assert_eq!(signs.children[1].duration_secs, None);
        assert_eq!(signs.children[2].label, "gamma");
        assert_eq!(signs.children[2].started_at_unix_ms, None);
        assert_eq!(signs.children[2].last_activity_unix_ms, None);
    }

    /// Test helper: build a `BusRequestSink` that forwards
    /// `BusRequest::EmitAgentEvent` requests onto a broadcast channel,
    /// returning the receiver so the test can assert what arrived.
    fn test_sink_with_receiver() -> (BusRequestSink, tokio::sync::broadcast::Receiver<AgentEvent>) {
        let (tx, rx) = tokio::sync::broadcast::channel::<AgentEvent>(8);
        let sink = BusRequestSink::from_fn(move |request| {
            if let BusRequest::EmitAgentEvent { event } = request {
                let _ = tx.send(*event);
            }
        });
        (sink, rx)
    }

    #[tokio::test]
    async fn pending_approval_emits_workstream_only_plan_update() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let (sink, mut rx) = test_sink_with_receiver();
        *feature.event_sender_slot().lock().unwrap() = Some(sink);

        feature.record_pending_approval(
            "cleave_live",
            "do gated work",
            r#"{"children":[{"label":"one","description":"first","scope":["a.rs"]}]}"#,
            1,
            1,
        );

        match rx.recv().await.unwrap() {
            AgentEvent::PlanUpdated { projection } => {
                assert!(projection.active.is_none());
                assert_eq!(projection.workstreams.len(), 1);
                assert_eq!(projection.workstreams[0].id, "cleave:cleave_live");
                assert_eq!(projection.workstreams[0].status, "pending_approval");
            }
            other => panic!("expected PlanUpdated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_slot_routes_family_vital_signs() {
        // Install a BusRequestSink wrapping a broadcast channel into the
        // slot and assert that FamilyVitalSignsUpdated reaches a subscribed
        // receiver via the BusRequest::EmitAgentEvent pathway.
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let (sink, mut rx) = test_sink_with_receiver();
        *feature.event_sender_slot().lock().unwrap() = Some(sink);

        let signs = omegon_traits::FamilyVitalSigns {
            run_id: "test".into(),
            active: true,
            total_children: 1,
            completed: 0,
            failed: 0,
            running: 1,
            pending: 0,
            total_tokens_in: 0,
            total_tokens_out: 0,
            children: vec![omegon_traits::ChildVitalSigns {
                label: "alpha".into(),
                status: "running".into(),
                started_at_unix_ms: Some(1),
                last_activity_unix_ms: Some(2),
                duration_secs: None,
                last_tool: Some("bash".into()),
                last_tool_activity: None,
                last_turn: Some(1),
                tokens_in: 0,
                tokens_out: 0,
                tasks: Vec::new(),
                tasks_done: 0,
            }],
        };
        feature.emit_decomposition_event(AgentEvent::FamilyVitalSignsUpdated {
            signs: signs.clone(),
        });

        match rx.recv().await.unwrap() {
            AgentEvent::FamilyVitalSignsUpdated { signs: got } => {
                assert_eq!(got.run_id, "test");
                assert_eq!(got.children.len(), 1);
                assert_eq!(got.children[0].label, "alpha");
            }
            other => panic!("expected FamilyVitalSignsUpdated, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_slot_routes_emissions_when_populated() {
        // Install a BusRequestSink into the slot and verify all three
        // decomposition variants reach a subscribed receiver via the
        // BusRequest::EmitAgentEvent pathway. The mechanism the cleave
        // run path uses; the call sites in execute_run are reviewed
        // manually since they require a real subprocess dispatch to
        // exercise end-to-end.
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let (sink, mut rx) = test_sink_with_receiver();
        *feature.event_sender_slot().lock().unwrap() = Some(sink);

        feature.emit_decomposition_event(AgentEvent::DecompositionStarted {
            children: vec!["alpha".into(), "beta".into()],
            operation: OperationRef::cleave(None),
        });
        feature.emit_decomposition_event(AgentEvent::DecompositionChildCompleted {
            label: "alpha".into(),
            success: true,
            operation: OperationRef::cleave(None),
        });
        feature.emit_decomposition_event(AgentEvent::DecompositionCompleted {
            merged: true,
            operation: OperationRef::cleave(None),
        });

        match rx.recv().await.unwrap() {
            AgentEvent::DecompositionStarted {
                children,
                operation,
            } => {
                assert_eq!(children, vec!["alpha".to_string(), "beta".to_string()]);
                assert_eq!(operation.kind, OperationKind::Cleave);
            }
            other => panic!("expected DecompositionStarted, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            AgentEvent::DecompositionChildCompleted {
                label,
                success,
                operation,
            } => {
                assert_eq!(label, "alpha");
                assert!(success);
                assert_eq!(operation.kind, OperationKind::Cleave);
            }
            other => panic!("expected DecompositionChildCompleted, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            AgentEvent::DecompositionCompleted { merged, operation } => {
                assert!(merged);
                assert_eq!(operation.kind, OperationKind::Cleave);
                assert_eq!(operation.id, None);
            }
            other => panic!("expected DecompositionCompleted, got {other:?}"),
        }
    }

    #[test]
    fn cancel_child_uses_registered_token() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let token = tokio_util::sync::CancellationToken::new();
        {
            let mut registry = feature.child_cancel_tokens.lock().unwrap();
            registry.insert("alpha".into(), token.clone());
        }

        assert!(feature.cancel_child("alpha"));
        assert!(token.is_cancelled());
        assert!(!feature.cancel_child("beta"));
    }

    #[test]
    fn new_loads_running_children_from_workspace_state() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join(".omegon/cleave-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let state_path = workspace.join("state.json");
        let worktree = workspace.join("alpha-wt");
        std::fs::create_dir_all(&worktree).unwrap();
        let state_json = serde_json::json!({
            "runId": "run-1",
            "directive": "test",
            "repoPath": dir.path().display().to_string(),
            "workspacePath": workspace.display().to_string(),
            "supervisorToken": "test-supervisor",
            "children": [{
                "childId": 0,
                "label": "alpha",
                "description": "do alpha",
                "scope": [],
                "dependsOn": [],
                "status": "running",
                "backend": "native",
                "worktreePath": worktree.display().to_string(),
                "executeModel": "model",
                "pid": std::process::id(),
                "adoptionWorktreePath": std::fs::canonicalize(&worktree).unwrap().to_string_lossy().to_string(),
                "adoptionModel": "model",
                "supervisorToken": "test-supervisor"
            }],
            "plan": {"children": []}
        });
        std::fs::write(
            &state_path,
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let progress = feature.progress();
        assert!(progress.active);
        assert_eq!(progress.run_id, "run-1");
        assert_eq!(progress.total_children, 1);
        assert_eq!(progress.children[0].label, "alpha");
        assert_eq!(progress.children[0].status, "running");
        assert_eq!(
            progress.children[0].supervision_mode,
            Some(ChildSupervisionMode::RecoveredDegraded)
        );
        assert_eq!(progress.children[0].pid, Some(std::process::id()));
    }

    #[test]
    fn new_marks_unadoptable_running_children_as_lost() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join(".omegon/cleave-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let state_path = workspace.join("state.json");
        let worktree = workspace.join("alpha-wt");
        std::fs::create_dir_all(&worktree).unwrap();
        let state_json = serde_json::json!({
            "runId": "run-1",
            "directive": "test",
            "repoPath": dir.path().display().to_string(),
            "workspacePath": workspace.display().to_string(),
            "supervisorToken": "test-supervisor",
            "children": [{
                "childId": 0,
                "label": "alpha",
                "description": "do alpha",
                "scope": [],
                "dependsOn": [],
                "status": "running",
                "backend": "native",
                "worktreePath": worktree.display().to_string(),
                "executeModel": "model",
                "pid": std::process::id(),
                "adoptionWorktreePath": std::fs::canonicalize(&worktree).unwrap().to_string_lossy().to_string(),
                "adoptionModel": "different-model",
                "supervisorToken": "test-supervisor"
            }],
            "plan": {"children": []}
        });
        std::fs::write(
            &state_path,
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let progress = feature.progress();
        assert!(progress.active);
        assert_eq!(progress.children[0].status, "pending");
        assert_eq!(
            progress.children[0].supervision_mode,
            Some(ChildSupervisionMode::Lost)
        );
        assert_eq!(progress.children[0].pid, Some(std::process::id()));
    }

    #[test]
    fn cancel_child_falls_back_to_persisted_pid_state() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join(".omegon/cleave-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let state_path = workspace.join("state.json");
        let worktree = workspace.join("alpha-wt");
        std::fs::create_dir_all(&worktree).unwrap();
        let state_json = serde_json::json!({
            "runId": "run-1",
            "directive": "test",
            "repoPath": dir.path().display().to_string(),
            "workspacePath": workspace.display().to_string(),
            "supervisorToken": "test-supervisor",
            "children": [{
                "childId": 0,
                "label": "alpha",
                "description": "do alpha",
                "scope": [],
                "dependsOn": [],
                "status": "running",
                "backend": "native",
                "worktreePath": worktree.display().to_string(),
                "executeModel": "model",
                "pid": 999999,
                "adoptionWorktreePath": std::fs::canonicalize(&worktree).unwrap().to_string_lossy().to_string(),
                "adoptionModel": "model",
                "supervisorToken": "test-supervisor"
            }],
            "plan": {"children": []}
        });
        std::fs::write(
            &state_path,
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![], false);
        assert!(feature.cancel_child("alpha"));
        let progress = feature.progress();
        assert!(!progress.active);
        assert_eq!(progress.children[0].supervision_mode, None);
        assert_eq!(progress.children[0].status, "failed");
        assert_eq!(progress.children[0].pid, None);
        let saved = crate::cleave::state::CleaveState::load(&state_path).unwrap();
        assert_eq!(saved.children[0].status, ChildStatus::Failed);
        assert!(saved.children[0].pid.is_none());
    }

    #[test]
    fn new_replays_activity_log_from_workspace_state() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join(".omegon/cleave-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let worktree = workspace.join("alpha-wt");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            workspace.join("child-alpha.activity.log"),
            "2026-01-01T00:00:00Z  INFO → bash cargo test
2026-01-01T00:00:01Z  INFO ── Turn 3 complete — in:123 out:45 ──
",
        )
        .unwrap();
        let state_json = serde_json::json!({
            "runId": "run-1",
            "directive": "test",
            "repoPath": dir.path().display().to_string(),
            "workspacePath": workspace.display().to_string(),
            "supervisorToken": "test-supervisor",
            "children": [{
                "childId": 0,
                "label": "alpha",
                "description": "do alpha",
                "scope": [],
                "dependsOn": [],
                "status": "running",
                "backend": "native",
                "worktreePath": worktree.display().to_string(),
                "executeModel": "model",
                "pid": std::process::id(),
                "adoptionWorktreePath": std::fs::canonicalize(&worktree).unwrap().to_string_lossy().to_string(),
                "adoptionModel": "model",
                "supervisorToken": "test-supervisor"
            }],
            "plan": {"children": []}
        });
        std::fs::write(
            workspace.join("state.json"),
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let progress = feature.progress();
        let child = &progress.children[0];
        let last_activity = child
            .last_tool_activity
            .as_ref()
            .expect("replayed tool activity should keep semantic tool metadata");
        assert_eq!(last_activity.raw_name, "bash");
        assert_eq!(last_activity.args_summary.as_deref(), Some("cargo test"));
        assert_eq!(child.last_tool.as_deref(), Some("bash"));
        assert_eq!(child.last_turn, Some(3));
        assert_eq!(
            child.supervision_mode,
            Some(ChildSupervisionMode::RecoveredDegraded)
        );
        assert_eq!(child.tokens_in, 123);
        assert_eq!(child.tokens_out, 45);
        assert!(child.last_activity_at.is_some());
    }

    #[test]
    fn progress_default_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let prog = feature.progress();
        assert!(!prog.active);
        assert_eq!(prog.total_children, 0);
    }

    #[test]
    fn apply_progress_event_updates_child_statuses() {
        let shared = Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            inventory_generation: None,
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "pending".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_tool_activity: None,
                last_turn: None,
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: None,
                last_activity_at: None,
                tokens_in: 0,
                tokens_out: 0,
                runtime: None,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildSpawned {
                child: "alpha".into(),
                pid: 42,
            },
        );
        {
            let progress = shared.lock().unwrap();
            assert_eq!(progress.children[0].status, "running");
            assert_eq!(
                progress.children[0].supervision_mode,
                Some(ChildSupervisionMode::Attached)
            );
            assert_eq!(progress.children[0].pid, Some(42));
            assert!(progress.children[0].started_at.is_some());
            assert!(progress.children[0].last_activity_at.is_some());
            assert!(progress.active);
        }

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildStatus {
                child: "alpha".into(),
                status: ChildProgressStatus::Completed,
                duration_secs: Some(1.5),
                error: None,
            },
        );
        let progress = shared.lock().unwrap();
        assert_eq!(progress.children[0].status, "completed");
        assert_eq!(progress.children[0].duration_secs, Some(1.5));
        assert_eq!(progress.children[0].supervision_mode, None);
        assert_eq!(progress.children[0].pid, None);
        assert!(progress.children[0].last_activity_at.is_some());
        assert_eq!(progress.completed, 1);
        assert_eq!(progress.failed, 0);
    }

    #[test]
    fn apply_progress_event_merged_after_failure_counts_as_completed_not_failed() {
        let shared = Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            inventory_generation: None,
            total_children: 1,
            completed: 0,
            failed: 1,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "failed".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_tool_activity: None,
                last_turn: None,
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: None,
                last_activity_at: None,
                tokens_in: 0,
                tokens_out: 0,
                runtime: None,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildStatus {
                child: "alpha".into(),
                status: ChildProgressStatus::MergedAfterFailure,
                duration_secs: Some(1.5),
                error: None,
            },
        );

        let progress = shared.lock().unwrap();
        assert_eq!(progress.children[0].status, "merged_after_failure");
        assert_eq!(progress.children[0].duration_secs, Some(1.5));
        assert_eq!(progress.completed, 1);
        assert_eq!(progress.failed, 0);
    }

    #[test]
    fn apply_progress_event_activity_refreshes_last_seen() {
        let shared = Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            inventory_generation: None,
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "running".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: Some(42),
                last_tool: None,
                last_tool_activity: None,
                last_turn: None,
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: Some(std::time::Instant::now()),
                last_activity_at: None,
                tokens_in: 0,
                tokens_out: 0,
                runtime: None,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildActivity {
                child: "alpha".into(),
                turn: Some(3),
                tool: Some("bash".into()),
                target: None,
            },
        );

        let progress = shared.lock().unwrap();
        let child = &progress.children[0];
        assert_eq!(child.last_turn, Some(3));
        assert_eq!(child.last_tool.as_deref(), Some("bash"));
        assert!(child.last_activity_at.is_some());
    }

    #[test]
    fn apply_progress_event_accumulates_tokens() {
        let shared = Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            inventory_generation: None,
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "pending".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_tool_activity: None,
                last_turn: None,
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: None,
                last_activity_at: None,
                tokens_in: 0,
                tokens_out: 0,
                runtime: None,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildTokens {
                child: "alpha".into(),
                input_tokens: 100,
                output_tokens: 50,
            },
        );

        let progress = shared.lock().unwrap();
        let child = &progress.children[0];
        assert_eq!(child.tokens_in, 100);
        assert_eq!(child.tokens_out, 50);
        assert_eq!(progress.total_tokens_in, 100);
        assert_eq!(progress.total_tokens_out, 50);
    }

    #[test]
    fn apply_progress_event_done_marks_run_inactive() {
        let shared = Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            inventory_generation: None,
            total_children: 2,
            completed: 1,
            failed: 0,
            children: vec![],
            total_tokens_in: 0,
            total_tokens_out: 0,
        }));

        apply_progress_event(
            &shared,
            &ProgressEvent::Done {
                completed: 1,
                failed: 1,
                duration_secs: 3.0,
            },
        );

        let progress = shared.lock().unwrap();
        assert!(!progress.active);
        assert_eq!(progress.completed, 1);
        assert_eq!(progress.failed, 1);
    }

    #[tokio::test]
    async fn assess_tool_execution() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![], false);
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature
            .execute(
                "cleave_assess",
                "tc1",
                json!({"directive": "Refactor the auth module", "threshold": 2.0}),
                cancel,
            )
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("decision"),
            "should return assessment: {text}"
        );
    }

    #[test]
    fn cleanup_workspace_dir_removes_existing_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join(".omegon/cleave-workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("state.json"), "{}").unwrap();

        cleanup_workspace_dir(&workspace).unwrap();

        assert!(!workspace.exists());
    }

    #[test]
    fn should_cleanup_workspace_only_when_all_children_completed() {
        let mk_child = |status| crate::cleave::state::ChildState {
            child_id: 0,
            label: "alpha".into(),
            description: "Do alpha work".into(),
            scope: vec!["src/".into()],
            depends_on: vec![],
            status,
            error: None,
            branch: Some("cleave/0-alpha".into()),
            worktree_path: None,
            backend: "native".into(),
            execute_model: None,
            provider_id: None,
            duration_secs: None,
            stdout: None,
            runtime: None,
            pid: None,
            started_at_unix_ms: None,
            last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        };

        let completed = cleave::orchestrator::CleaveResult {
            state: crate::cleave::state::CleaveState {
                run_id: "run-1".into(),
                directive: "test".into(),
                repo_path: "/tmp/repo".into(),
                workspace_path: "/tmp/workspace".into(),
                supervisor_token: "test-supervisor".into(),
                children: vec![mk_child(ChildStatus::Completed)],
                plan: json!({}),
                started_at: None,
            },
            merge_results: vec![],
            duration_secs: 1.0,
        };
        assert!(should_cleanup_workspace(&completed));

        let failed = cleave::orchestrator::CleaveResult {
            state: crate::cleave::state::CleaveState {
                run_id: "run-2".into(),
                directive: "test".into(),
                repo_path: "/tmp/repo".into(),
                workspace_path: "/tmp/workspace".into(),
                supervisor_token: "test-supervisor".into(),
                children: vec![
                    mk_child(ChildStatus::Completed),
                    mk_child(ChildStatus::Failed),
                ],
                plan: json!({}),
                started_at: None,
            },
            merge_results: vec![],
            duration_secs: 1.0,
        };
        assert!(!should_cleanup_workspace(&failed));
    }
}

#[cfg(test)]
mod assessment_tests {
    use super::*;

    #[test]
    fn ui_component_matches() {
        let r = assess_directive("Build a dialog component for settings", 2.0);
        assert_eq!(r["pattern_id"], "ui-feature");
    }

    #[test]
    fn auth_matches() {
        let r = assess_directive("Add OAuth token refresh with encryption", 2.0);
        assert_eq!(r["pattern_id"], "auth-security");
        assert_eq!(r["decision"], "cleave"); // systems=3 + modifier
    }

    #[test]
    fn test_coverage_is_simple() {
        let r = assess_directive("Add unit test fixtures for the parser", 2.0);
        assert_eq!(r["pattern_id"], "test-coverage");
        assert_eq!(r["decision"], "execute"); // systems=1
    }

    #[test]
    fn multi_service_is_complex() {
        let r = assess_directive("Integrate the gRPC service with the message queue", 2.0);
        assert_eq!(r["pattern_id"], "multi-service");
        assert_eq!(r["decision"], "cleave"); // systems=4
    }

    #[test]
    fn no_keywords_returns_needs_assessment() {
        let r = assess_directive("make it better", 2.0);
        assert_eq!(r["method"], "needs_assessment");
        assert_eq!(r["decision"], "execute");
    }

    #[test]
    fn all_modifiers_stack() {
        let r = assess_directive(
            "concurrent performance optimization with backward compatibility for cross-platform validation",
            100.0, // High threshold so we can just check complexity
        );
        let mods = r["modifiers"].as_array().unwrap();
        assert!(
            mods.len() >= 3,
            "should detect multiple modifiers: {mods:?}"
        );
        assert!(r["complexity"].as_f64().unwrap() > 1.0);
    }

    #[test]
    fn custom_threshold() {
        let r = assess_directive("simple refactor extract helpers", 100.0);
        assert_eq!(
            r["decision"], "execute",
            "high threshold should always execute"
        );

        let r = assess_directive("simple refactor extract helpers", 0.5);
        assert_eq!(
            r["decision"], "cleave",
            "low threshold should always cleave"
        );
    }

    #[test]
    fn confidence_between_0_and_1() {
        let r = assess_directive("Deploy a containerized service", 2.0);
        let conf = r["confidence"].as_f64().unwrap();
        assert!(
            conf > 0.0 && conf <= 1.0,
            "confidence should be (0,1]: {conf}"
        );
    }

    #[test]
    fn apply_progress_event_tracks_spawn_activity_tasks_failure_and_done() {
        let shared = Arc::new(Mutex::new(CleaveProgress::default()));

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildSpawned {
                child: "alpha".into(),
                pid: 4242,
            },
        );
        {
            let progress = shared.lock().unwrap();
            assert!(progress.active);
            assert_eq!(progress.total_children, 1);
            let child = &progress.children[0];
            assert_eq!(child.label, "alpha");
            assert_eq!(child.status, "running");
            assert_eq!(child.pid, Some(4242));
            assert_eq!(child.supervision_mode, Some(ChildSupervisionMode::Attached));
        }

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildTaskInventory {
                child: "alpha".into(),
                total_tasks: 2,
                scope_files: 1,
                tasks: vec![
                    ChildTaskItem {
                        description: "Inspect".into(),
                        done: false,
                    },
                    ChildTaskItem {
                        description: "Report".into(),
                        done: false,
                    },
                ],
            },
        );
        apply_progress_event(
            &shared,
            &ProgressEvent::ChildActivity {
                child: "alpha".into(),
                turn: Some(2),
                tool: Some("bash".into()),
                target: Some("cargo test".into()),
            },
        );
        apply_progress_event(
            &shared,
            &ProgressEvent::ChildTokens {
                child: "alpha".into(),
                input_tokens: 11,
                output_tokens: 7,
            },
        );
        {
            let progress = shared.lock().unwrap();
            assert_eq!(progress.total_tokens_in, 11);
            assert_eq!(progress.total_tokens_out, 7);
            let child = &progress.children[0];
            assert_eq!(child.last_turn, Some(2));
            assert_eq!(child.last_tool.as_deref(), Some("bash"));
            assert_eq!(child.tasks_done, 1);
            assert_eq!(child.tokens_in, 11);
            assert_eq!(child.tokens_out, 7);
        }

        apply_progress_event(
            &shared,
            &ProgressEvent::ChildStatus {
                child: "alpha".into(),
                status: ChildProgressStatus::Failed,
                duration_secs: Some(3.5),
                error: Some("non-zero exit".into()),
            },
        );
        {
            let progress = shared.lock().unwrap();
            assert_eq!(progress.completed, 0);
            assert_eq!(progress.failed, 1);
            let child = &progress.children[0];
            assert_eq!(child.status, "failed");
            assert_eq!(child.pid, None);
            assert_eq!(child.supervision_mode, None);
            assert_eq!(child.duration_secs, Some(3.5));
        }

        apply_progress_event(
            &shared,
            &ProgressEvent::Done {
                completed: 0,
                failed: 1,
                duration_secs: 4.0,
            },
        );
        let progress = shared.lock().unwrap();
        assert!(!progress.active);
        assert_eq!(progress.failed, 1);
    }

    #[test]
    fn cleave_status_render_exposes_child_runtime_activity_and_tasks() {
        let mut progress = CleaveProgress {
            active: true,
            run_id: "run-test".into(),
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "running".into(),
                failure_kind: None,
                duration_secs: Some(12.0),
                supervision_mode: Some(ChildSupervisionMode::Attached),
                pid: Some(1234),
                last_tool: Some("bash".into()),
                last_tool_activity: None,
                last_turn: Some(3),
                tasks: vec![
                    ChildTaskItem {
                        description: "Inspect".into(),
                        done: true,
                    },
                    ChildTaskItem {
                        description: "Patch".into(),
                        done: false,
                    },
                ],
                tasks_done: 1,
                started_at: None,
                last_activity_at: None,
                tokens_in: 20,
                tokens_out: 5,
                runtime: None,
            }],
            total_tokens_in: 20,
            total_tokens_out: 5,
        };

        let rendered = CleaveFeature::render_status(&progress);

        assert!(rendered.contains("Cleave active: 0/1 cloves"), "{rendered}");
        assert!(rendered.contains("alpha [running]"), "{rendered}");
        assert!(rendered.contains("pid=1234"), "{rendered}");
        assert!(rendered.contains("supervision=Attached"), "{rendered}");
        assert!(rendered.contains("tool=bash"), "{rendered}");
        assert!(rendered.contains("turn=3"), "{rendered}");
        assert!(rendered.contains("tasks=1/2"), "{rendered}");
        assert!(rendered.contains("tokens=20/5"), "{rendered}");

        progress.active = false;
        progress.completed = 1;
        progress.children[0].status = "completed".into();
        let rendered = CleaveFeature::render_status(&progress);
        assert!(
            rendered.contains("Last cleave: 1 completed, 0 failed of 1"),
            "{rendered}"
        );
        assert!(rendered.contains("alpha [completed]"), "{rendered}");
    }
}
