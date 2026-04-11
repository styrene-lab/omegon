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
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use omegon_traits::{
    AgentEvent, BusEvent, BusRequest, BusRequestSink, CommandDefinition, CommandResult,
    ContentBlock, Feature, ToolDefinition, ToolResult,
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

    let decision = if effective > threshold {
        "cleave"
    } else {
        "execute"
    };

    json!({
        "decision": decision,
        "complexity": complexity,
        "systems": systems as u8,
        "modifiers": active_modifiers,
        "method": if confidence > 0.0 { "fast-path" } else { "needs_assessment" },
        "pattern": format!("{} ({}%)", pattern_label, (confidence * 100.0) as u8),
        "pattern_id": pattern_id,
        "confidence": confidence,
        "threshold": threshold,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Live progress tracking
// ═══════════════════════════════════════════════════════════════════════════

/// Live progress of an active cleave run, for dashboard rendering.
#[derive(Default, Clone)]
pub struct CleaveProgress {
    pub active: bool,
    pub run_id: String,
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

#[derive(Clone)]
pub struct ChildProgress {
    pub label: String,
    pub status: String, // "pending", "running", "completed", "failed", "merged_after_failure", "upstream_exhausted"
    pub duration_secs: Option<f64>,
    /// Current supervision continuity for this child runtime.
    pub supervision_mode: Option<ChildSupervisionMode>,
    /// Spawned child PID while the orchestrator still owns the subprocess.
    pub pid: Option<u32>,
    /// Most recent tool active inside this child (e.g. "bash", "write").
    pub last_tool: Option<String>,
    /// Most recent turn number reported by this child.
    pub last_turn: Option<u32>,
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

fn child_runtime_summary(runtime: &crate::cleave::CleaveChildRuntimeProfile) -> ChildRuntimeSummary {
    ChildRuntimeSummary {
        model: runtime.model.clone(),
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

fn apply_progress_event(shared: &Arc<Mutex<CleaveProgress>>, event: &ProgressEvent) {
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
                    duration_secs: None,
                    supervision_mode: Some(ChildSupervisionMode::Attached),
                    pid: Some(*pid),
                    last_tool: None,
                    last_turn: None,
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
            child, turn, tool, ..
        } => {
            if let Some(c) = progress.children.iter_mut().find(|c| c.label == *child) {
                if let Some(t) = turn {
                    c.last_turn = Some(*t);
                }
                if let Some(t) = tool {
                    c.last_tool = Some(t.clone());
                }
                c.last_activity_at = Some(std::time::Instant::now());
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
                    duration_secs: *duration_secs,
                    supervision_mode: None,
                    pid: None,
                    last_tool: None,
                    last_turn: None,
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

pub struct CleaveFeature {
    repo_path: PathBuf,
    /// Shared progress state — updated by the spawned orchestrator task,
    /// read by the dashboard renderer.
    progress: Arc<Mutex<CleaveProgress>>,
    /// In-process cancel handles for active cleave children, keyed by label.
    child_cancel_tokens: Arc<Mutex<HashMap<String, tokio_util::sync::CancellationToken>>>,
    /// Provider inventory for per-child routing.
    pub inventory: Option<std::sync::Arc<tokio::sync::RwLock<crate::routing::ProviderInventory>>>,
    /// Startup-approved secret env inherited by child runs.
    session_secret_env: Vec<(String, String)>,
    /// Slot holding the runtime-supplied `BusRequestSink` once the
    /// runtime has constructed it. See [`CleaveEventSlot`] for the
    /// rationale.
    bus_request_sink: CleaveEventSlot,
}

impl CleaveFeature {
    pub fn new(repo_path: &std::path::Path, session_secret_env: Vec<(String, String)>) -> Self {
        let progress = Arc::new(Mutex::new(CleaveProgress::default()));
        let feature = Self {
            repo_path: repo_path.to_path_buf(),
            progress,
            child_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
            inventory: None,
            session_secret_env,
            bus_request_sink: Arc::new(Mutex::new(None)),
        };
        feature.refresh_progress_from_workspace_state();
        feature
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
        if let Ok(slot) = self.bus_request_sink.lock() {
            if let Some(sink) = slot.as_ref() {
                sink.send(BusRequest::EmitAgentEvent { event });
            }
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
            if let Some(event) = crate::cleave::progress::parse_child_activity(&progress.label, line) {
                match event {
                    crate::cleave::progress::ProgressEvent::ChildActivity { turn, tool, .. } => {
                        if let Some(turn) = turn { progress.last_turn = Some(turn); }
                        if let Some(tool) = tool { progress.last_tool = Some(tool); }
                    }
                    crate::cleave::progress::ProgressEvent::ChildTokens { input_tokens, output_tokens, .. } => {
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
                    last_turn: None,
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
        progress.active = progress.children.iter().any(|c| matches!(c.supervision_mode, Some(ChildSupervisionMode::Attached | ChildSupervisionMode::RecoveredDegraded | ChildSupervisionMode::Lost)) || c.status == "running");
    }

    /// Get a clone of the current progress for dashboard rendering.
    pub fn progress(&self) -> CleaveProgress {
        self.progress.lock().unwrap().clone()
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

        let fallback_pid = self
            .progress
            .lock()
            .ok()
            .and_then(|progress| {
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
        let directive = args["directive"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("directive required"))?;
        let plan_json = args["plan_json"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("plan_json required"))?;
        let max_parallel = args["max_parallel"].as_u64().unwrap_or(4) as usize;

        let plan = CleavePlan::from_json(plan_json)?;

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
                    duration_secs: None,
                    supervision_mode: None,
                    pid: None,
                    last_tool: None,
                    last_turn: None,
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
        });

        let progress_sink = {
            let shared = self.shared_progress();
            let event_slot = self.event_sender_slot();
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
                        ChildProgressStatus::Completed
                            | ChildProgressStatus::MergedAfterFailure
                    );
                    sink.send(BusRequest::EmitAgentEvent {
                        event: AgentEvent::DecompositionChildCompleted {
                            label: child.clone(),
                            success,
                        },
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
                        event: AgentEvent::FamilyVitalSignsUpdated { signs },
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
                .unwrap_or_else(|_| "anthropic:claude-sonnet-4-6".into()),
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
        self.emit_decomposition_event(AgentEvent::DecompositionCompleted { merged });

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
                .filter(|c| c.status == ChildStatus::Failed || c.status == ChildStatus::UpstreamExhausted)
                .count();
            for (i, child) in result.state.children.iter().enumerate() {
                if let Some(p) = prog.children.get_mut(i) {
                    p.status = match child.status {
                        ChildStatus::Completed => {
                            if child.error.as_deref() == Some("merged after salvaging work from a failed child") {
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
                    if child.error.as_deref() == Some("merged after salvaging work from a failed child") {
                        "↺"
                    } else {
                        "✓"
                    }
                }
                ChildStatus::Failed => "✗",
                ChildStatus::UpstreamExhausted => "⚡",
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
                report.push_str("    ⚡ Provider upstream exhausted — check inventory for available fallbacks.\n");
            }
            if let Some(err) = &child.error {
                // Truncate long error details (stderr tails can be long)
                let short = if err.len() > 400 {
                    format!("{}…", &err[..400])
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
                    let child = result.state.children.iter().find(|child| child.label == *label);
                    if child
                        .and_then(|child| child.error.as_deref())
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
                            "type": "string",
                            "description": "JSON string of the split plan: {\"children\": [{\"label\": \"...\", \"description\": \"...\", \"scope\": [...], \"depends_on\": [...]}]}"
                        },
                        "max_parallel": {
                            "type": "number",
                            "description": "Maximum parallel children (default: 4)"
                        }
                    },
                    "required": ["directive", "plan_json"]
                }),
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
            subcommands: vec!["status".into(), "cancel <label>".into()],
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        match name {
            "cleave" => {
                let sub = args.trim();
                let prog = self.progress.lock().unwrap();
                if sub == "status" || sub.is_empty() {
                    if !prog.active && prog.total_children == 0 {
                        return CommandResult::Display("No active cleave run.".into());
                    }
                    let mut lines = Vec::new();
                    if prog.active {
                        lines.push(format!(
                            "Cleave active: {}/{} children",
                            prog.completed + prog.failed,
                            prog.total_children
                        ));
                    } else {
                        lines.push(format!(
                            "Last cleave: {} completed, {} failed of {}",
                            prog.completed, prog.failed, prog.total_children
                        ));
                    }
                    for child in &prog.children {
                        let icon = match child.status.as_str() {
                            "completed" => "✓",
                            "failed" => "✗",
                            "running" => "⏳",
                            _ => "○",
                        };
                        let dur = child
                            .duration_secs
                            .map(|d| format!(" ({:.0}s)", d))
                            .unwrap_or_default();
                        lines.push(format!("  {} {}{}", icon, child.label, dur));
                    }
                    CommandResult::Display(lines.join("\n"))
                } else if let Some(label) = sub.strip_prefix("cancel ").map(str::trim) {
                    if label.is_empty() {
                        CommandResult::Display("Usage: /cleave cancel <label>".into())
                    } else if self.cancel_child(label) {
                        CommandResult::Display(format!("Cancelling cleave child '{label}'..."))
                    } else {
                        CommandResult::Display(format!("No active cleave child '{label}'."))
                    }
                } else {
                    CommandResult::Display("Usage: /cleave [status|cancel <label>]".into())
                }
            }
            _ => CommandResult::NotHandled,
        }
    }

    fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
        vec![]
    }
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
            last_turn: c.last_turn,
            tokens_in: c.tokens_in,
            tokens_out: c.tokens_out,
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
    use super::*;

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
        let feature = CleaveFeature::new(dir.path(), vec![]);
        let tools = feature.tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "cleave_assess"));
        assert!(tools.iter().any(|t| t.name == "cleave_run"));
    }

    #[test]
    fn cleave_status_no_active_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = CleaveFeature::new(dir.path(), vec![]);
        let result = feature.handle_command("cleave", "status");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("No active")));
    }

    #[test]
    fn event_slot_starts_empty_and_drops_emissions() {
        // Without a sender installed, emit_decomposition_event must be a
        // silent no-op (used in tests, headless runs, anywhere without an
        // event bus). Reaching this assertion at all proves we don't panic.
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![]);
        feature.emit_decomposition_event(AgentEvent::DecompositionStarted {
            children: vec!["a".into(), "b".into()],
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
                duration_secs: Some(12.5),
                supervision_mode: None,
                pid: None,
                last_tool: Some("commit".into()),
                last_turn: Some(8),
                started_at: Some(std::time::Instant::now()),
                last_activity_at: Some(std::time::Instant::now()),
                tokens_in: 800,
                tokens_out: 400,
                runtime: None,
            },
            ChildProgress {
                label: "beta".into(),
                status: "running".into(),
                duration_secs: None,
                supervision_mode: None,
                pid: Some(42_u32),
                last_tool: Some("write".into()),
                last_turn: Some(3),
                started_at: Some(std::time::Instant::now()),
                last_activity_at: Some(std::time::Instant::now()),
                tokens_in: 700,
                tokens_out: 350,
                runtime: None,
            },
            ChildProgress {
                label: "gamma".into(),
                status: "pending".into(),
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_turn: None,
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
    fn test_sink_with_receiver() -> (BusRequestSink, tokio::sync::broadcast::Receiver<AgentEvent>)
    {
        let (tx, rx) = tokio::sync::broadcast::channel::<AgentEvent>(8);
        let sink = BusRequestSink::from_fn(move |request| {
            if let BusRequest::EmitAgentEvent { event } = request {
                let _ = tx.send(event);
            }
        });
        (sink, rx)
    }

    #[tokio::test]
    async fn event_slot_routes_family_vital_signs() {
        // Install a BusRequestSink wrapping a broadcast channel into the
        // slot and assert that FamilyVitalSignsUpdated reaches a subscribed
        // receiver via the BusRequest::EmitAgentEvent pathway.
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![]);
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
                last_turn: Some(1),
                tokens_in: 0,
                tokens_out: 0,
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
        let feature = CleaveFeature::new(dir.path(), vec![]);
        let (sink, mut rx) = test_sink_with_receiver();
        *feature.event_sender_slot().lock().unwrap() = Some(sink);

        feature.emit_decomposition_event(AgentEvent::DecompositionStarted {
            children: vec!["alpha".into(), "beta".into()],
        });
        feature.emit_decomposition_event(AgentEvent::DecompositionChildCompleted {
            label: "alpha".into(),
            success: true,
        });
        feature.emit_decomposition_event(AgentEvent::DecompositionCompleted { merged: true });

        match rx.recv().await.unwrap() {
            AgentEvent::DecompositionStarted { children } => {
                assert_eq!(children, vec!["alpha".to_string(), "beta".to_string()]);
            }
            other => panic!("expected DecompositionStarted, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            AgentEvent::DecompositionChildCompleted { label, success } => {
                assert_eq!(label, "alpha");
                assert!(success);
            }
            other => panic!("expected DecompositionChildCompleted, got {other:?}"),
        }
        match rx.recv().await.unwrap() {
            AgentEvent::DecompositionCompleted { merged } => {
                assert!(merged);
            }
            other => panic!("expected DecompositionCompleted, got {other:?}"),
        }
    }

    #[test]
    fn cancel_child_uses_registered_token() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![]);
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
        std::fs::write(&state_path, serde_json::to_string_pretty(&state_json).unwrap()).unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![]);
        let progress = feature.progress();
        assert!(progress.active);
        assert_eq!(progress.run_id, "run-1");
        assert_eq!(progress.total_children, 1);
        assert_eq!(progress.children[0].label, "alpha");
        assert_eq!(progress.children[0].status, "running");
        assert_eq!(progress.children[0].supervision_mode, Some(ChildSupervisionMode::RecoveredDegraded));
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
        std::fs::write(&state_path, serde_json::to_string_pretty(&state_json).unwrap()).unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![]);
        let progress = feature.progress();
        assert!(progress.active);
        assert_eq!(progress.children[0].status, "pending");
        assert_eq!(progress.children[0].supervision_mode, Some(ChildSupervisionMode::Lost));
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
        std::fs::write(&state_path, serde_json::to_string_pretty(&state_json).unwrap()).unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![]);
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
        std::fs::write(workspace.join("state.json"), serde_json::to_string_pretty(&state_json).unwrap()).unwrap();

        let feature = CleaveFeature::new(dir.path(), vec![]);
        let progress = feature.progress();
        let child = &progress.children[0];
        assert_eq!(child.last_tool.as_deref(), Some("bash"));
        assert_eq!(child.last_turn, Some(3));
        assert_eq!(child.supervision_mode, Some(ChildSupervisionMode::RecoveredDegraded));
        assert_eq!(child.tokens_in, 123);
        assert_eq!(child.tokens_out, 45);
        assert!(child.last_activity_at.is_some());
    }

    #[test]
    fn progress_default_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let feature = CleaveFeature::new(dir.path(), vec![]);
        let prog = feature.progress();
        assert!(!prog.active);
        assert_eq!(prog.total_children, 0);
    }

    #[test]
    fn apply_progress_event_updates_child_statuses() {
        let shared = Arc::new(Mutex::new(CleaveProgress {
            active: true,
            run_id: "run-1".into(),
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "pending".into(),
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_turn: None,
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
            assert_eq!(progress.children[0].supervision_mode, Some(ChildSupervisionMode::Attached));
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
            total_children: 1,
            completed: 0,
            failed: 1,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "failed".into(),
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_turn: None,
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
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "running".into(),
                duration_secs: None,
                supervision_mode: None,
                pid: Some(42),
                last_tool: None,
                last_turn: None,
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
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![ChildProgress {
                label: "alpha".into(),
                status: "pending".into(),
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_turn: None,
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
        let feature = CleaveFeature::new(dir.path(), vec![]);
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
                children: vec![mk_child(ChildStatus::Completed), mk_child(ChildStatus::Failed)],
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
}
