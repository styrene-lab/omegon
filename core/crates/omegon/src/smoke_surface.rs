//! Shared smoke drivers for renderer-neutral live surfaces.
//!
//! These drivers exercise the same semantic state consumed by TUI, ACP, IPC,
//! and web projections. They must not write progress as conversation text.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::broadcast;

use crate::child_agent::ChildTaskItem;
use crate::features::cleave::{
    ChildProgress, ChildRuntimeSummary, ChildSupervisionMode, CleaveChildFailureKind,
    CleaveProgress,
};
use crate::features::delegate::{DelegateChildFailureKind, DelegateProgress, DelegateProgressChild};
use crate::surfaces::conversation::ToolActivitySummary;
use omegon_traits::{
    AgentEvent, PlanItemProjection, PlanLaneProjection, PlanProgressProjection,
    PlanSurfaceProjection, PlanWorkstreamProjection, SlashCommandResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeCommand {
    List,
    Scenario(SmokeScenarioKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeScenarioKind {
    CleaveBasic,
    CleaveFailureMix,
    CleaveActivity,
    CleaveDocsResearch,
    DelegateBasic,
    DelegatePendingResult,
    SurfaceStress,
}

impl SmokeScenarioKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::CleaveBasic => "cleave-basic",
            Self::CleaveFailureMix => "cleave-failure-mix",
            Self::CleaveActivity => "cleave-activity",
            Self::CleaveDocsResearch => "cleave-docs-research",
            Self::DelegateBasic => "delegate-basic",
            Self::DelegatePendingResult => "delegate-pending-result",
            Self::SurfaceStress => "surface-stress",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::CleaveBasic => "Cleave basic",
            Self::CleaveFailureMix => "Cleave failure mix",
            Self::CleaveActivity => "Cleave child activity",
            Self::CleaveDocsResearch => "Cleave docs/research",
            Self::DelegateBasic => "Delegate basic",
            Self::DelegatePendingResult => "Delegate pending result",
            Self::SurfaceStress => "Surface stress",
        }
    }

    fn from_args(args: &[&str]) -> Option<Self> {
        match args {
            [] => Some(Self::CleaveFailureMix),
            ["cleave"] => Some(Self::CleaveFailureMix),
            ["cleave", "basic"] | ["cleave-basic"] => Some(Self::CleaveBasic),
            ["cleave", "failure-mix"] | ["cleave-failure-mix"] => Some(Self::CleaveFailureMix),
            ["cleave", "activity"] | ["cleave-activity"] => Some(Self::CleaveActivity),
            ["cleave", "docs-research"] | ["cleave-docs-research"] => {
                Some(Self::CleaveDocsResearch)
            }
            ["delegate"] | ["delegate", "basic"] | ["delegate-basic"] => {
                Some(Self::DelegateBasic)
            }
            ["delegate", "pending-result"] | ["delegate-pending-result"] => {
                Some(Self::DelegatePendingResult)
            }
            ["surface", "stress"] | ["surface-stress"] | ["stress"] => {
                Some(Self::SurfaceStress)
            }
            _ => None,
        }
    }
}

pub fn parse_smoke_command(args: &str) -> Option<SmokeCommand> {
    let args = args.split_whitespace().collect::<Vec<_>>();
    match args.as_slice() {
        [] | ["list"] => Some(SmokeCommand::List),
        _ => SmokeScenarioKind::from_args(&args).map(SmokeCommand::Scenario),
    }
}

pub fn smoke_list_text() -> String {
    "Available smoke suites:\n  /smoke cleave basic\n  /smoke cleave failure-mix\n  /smoke cleave activity\n  /smoke cleave docs-research\n  /smoke delegate basic\n  /smoke delegate pending-result\n  /smoke surface stress"
        .into()
}

pub fn launch_surface_smoke(
    handles: &mut crate::tui::dashboard::DashboardHandles,
    scenario: SmokeScenarioKind,
    events_tx: Option<broadcast::Sender<AgentEvent>>,
    local_events_tx: Option<std::sync::mpsc::Sender<AgentEvent>>,
) -> SlashCommandResponse {
    if active_cleave(handles) || active_delegate(handles) {
        return SlashCommandResponse {
            accepted: false,
            output: Some("A smoke or live subagent operation is already running.".into()),
        };
    }

    match scenario {
        SmokeScenarioKind::DelegateBasic | SmokeScenarioKind::DelegatePendingResult => {
            launch_delegate_surface_smoke(handles, scenario, events_tx, local_events_tx)
        }
        _ => launch_cleave_surface_smoke(handles, scenario, events_tx, local_events_tx),
    }
}

pub fn launch_cleave_surface_smoke(
    handles: &mut crate::tui::dashboard::DashboardHandles,
    scenario: SmokeScenarioKind,
    events_tx: Option<broadcast::Sender<AgentEvent>>,
    local_events_tx: Option<std::sync::mpsc::Sender<AgentEvent>>,
) -> SlashCommandResponse {
    let progress = Arc::new(Mutex::new(initial_cleave_progress(scenario)));
    handles.cleave = Some(progress.clone());
    handles.delegate = None;
    let tx = events_tx.clone();
    let local_tx = local_events_tx.clone();
    std::thread::spawn(move || run_cleave_timeline(progress, scenario, tx, local_tx));

    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Started {} smoke suite on shared live surface projections.",
            scenario.title()
        )),
    }
}

fn launch_delegate_surface_smoke(
    handles: &mut crate::tui::dashboard::DashboardHandles,
    scenario: SmokeScenarioKind,
    events_tx: Option<broadcast::Sender<AgentEvent>>,
    local_events_tx: Option<std::sync::mpsc::Sender<AgentEvent>>,
) -> SlashCommandResponse {
    let progress = Arc::new(Mutex::new(initial_delegate_progress(scenario)));
    handles.delegate = Some(progress.clone());
    handles.cleave = None;
    let tx = events_tx.clone();
    let local_tx = local_events_tx.clone();
    std::thread::spawn(move || run_delegate_timeline(progress, scenario, tx, local_tx));

    SlashCommandResponse {
        accepted: true,
        output: Some(format!(
            "Started {} smoke suite on shared live surface projections.",
            scenario.title()
        )),
    }
}

fn active_cleave(handles: &crate::tui::dashboard::DashboardHandles) -> bool {
    handles
        .cleave
        .as_ref()
        .and_then(|progress| progress.lock().ok())
        .is_some_and(|progress| progress.active)
}

fn active_delegate(handles: &crate::tui::dashboard::DashboardHandles) -> bool {
    handles
        .delegate
        .as_ref()
        .and_then(|progress| progress.lock().ok())
        .is_some_and(|progress| progress.active || progress.running > 0)
}

fn run_cleave_timeline(
    progress: Arc<Mutex<CleaveProgress>>,
    scenario: SmokeScenarioKind,
    tx: Option<broadcast::Sender<AgentEvent>>,
    local_tx: Option<std::sync::mpsc::Sender<AgentEvent>>,
) {
    let total = progress.lock().map(|p| p.children.len()).unwrap_or(0).max(1);
    send_plan(&tx, &local_tx, scenario, 0, 0, false);
    update(&progress, |p| {
        let now = Instant::now();
        let tools = ["read", "codebase_search", "edit", "validate", "delegate"];
        for (idx, child) in p.children.iter_mut().enumerate() {
            child.status = "running".into();
            child.supervision_mode = Some(if matches!(scenario, SmokeScenarioKind::SurfaceStress) && idx % 5 == 4 {
                ChildSupervisionMode::RecoveredDegraded
            } else {
                ChildSupervisionMode::Attached
            });
            child.started_at = Some(now);
            child.last_activity_at = Some(now);
            let tool = tools[idx % tools.len()];
            child.last_tool = Some(tool.into());
            child.last_tool_activity = Some(ToolActivitySummary::new(
                tool,
                Some(format!("{} step {}", scenario.id(), idx + 1)),
            ));
            child.last_turn = Some((idx + 1) as u32);
        }
    });

    for idx in 0..total {
        std::thread::sleep(Duration::from_millis(350));
        update(&progress, |p| {
            let scenario_id = scenario.id();
            let child = &mut p.children[idx];
            match cleave_terminal_status(scenario, idx, total) {
                ("completed", None) => {
                    child.status = "completed".into();
                    child.tasks_done = child.tasks.len();
                }
                (status, failure) => {
                    child.status = status.into();
                    child.failure_kind = failure;
                    child.tasks_done = child.tasks.len().saturating_sub(1);
                }
            }
            child.duration_secs = Some(0.35 * (idx + 1) as f64);
            child.tokens_in = 700 + (idx as u64 * 111);
            child.tokens_out = 120 + (idx as u64 * 37);
            child.last_tool_activity = Some(ToolActivitySummary::new(
                child.last_tool.clone().unwrap_or_else(|| "tool".into()),
                Some(format!("{} terminal update", scenario_id)),
            ));
            p.completed = p.children.iter().filter(|child| child.status == "completed").count();
            p.failed = p
                .children
                .iter()
                .filter(|child| matches!(child.status.as_str(), "failed" | "upstream_exhausted"))
                .count();
            p.total_tokens_in = p.children.iter().map(|child| child.tokens_in).sum();
            p.total_tokens_out = p.children.iter().map(|child| child.tokens_out).sum();
        });
        send_plan(&tx, &local_tx, scenario, idx + 1, (idx + 1).min(total - 1), false);
    }

    std::thread::sleep(Duration::from_millis(250));
    update(&progress, |p| {
        p.active = false;
        p.completed = p.children.iter().filter(|child| child.status == "completed").count();
        p.failed = p
            .children
            .iter()
            .filter(|child| matches!(child.status.as_str(), "failed" | "upstream_exhausted"))
            .count();
    });
    send_plan(&tx, &local_tx, scenario, total, total.saturating_sub(1), true);
}

fn cleave_terminal_status(
    scenario: SmokeScenarioKind,
    idx: usize,
    total: usize,
) -> (&'static str, Option<CleaveChildFailureKind>) {
    match scenario {
        SmokeScenarioKind::CleaveBasic => ("completed", None),
        SmokeScenarioKind::CleaveFailureMix => match idx {
            0 => ("completed", None),
            1 => ("failed", Some(CleaveChildFailureKind::ValidationFailed)),
            _ => ("upstream_exhausted", Some(CleaveChildFailureKind::UpstreamExhausted)),
        },
        SmokeScenarioKind::CleaveActivity => match idx {
            0 | 1 | 3 => ("completed", None),
            _ => ("failed", Some(CleaveChildFailureKind::ScopeViolation)),
        },
        SmokeScenarioKind::CleaveDocsResearch => match idx {
            3 => ("failed", Some(CleaveChildFailureKind::ValidationFailed)),
            _ => ("completed", None),
        },
        SmokeScenarioKind::SurfaceStress => match idx % 6 {
            1 => ("failed", Some(CleaveChildFailureKind::WallTimeout)),
            3 => ("upstream_exhausted", Some(CleaveChildFailureKind::UpstreamExhausted)),
            5 if idx + 1 < total => ("failed", Some(CleaveChildFailureKind::MergeConflict)),
            _ => ("completed", None),
        },
        SmokeScenarioKind::DelegateBasic | SmokeScenarioKind::DelegatePendingResult => {
            ("completed", None)
        }
    }
}

fn run_delegate_timeline(
    progress: Arc<Mutex<DelegateProgress>>,
    scenario: SmokeScenarioKind,
    tx: Option<broadcast::Sender<AgentEvent>>,
    local_tx: Option<std::sync::mpsc::Sender<AgentEvent>>,
) {
    let total = progress.lock().map(|p| p.children.len()).unwrap_or(0).max(1);
    send_plan(&tx, &local_tx, scenario, 0, 0, false);
    update_delegate(&progress, |p| {
        let now = SystemTime::now();
        p.active = true;
        p.running = p.children.len();
        for (idx, child) in p.children.iter_mut().enumerate() {
            child.status = "running".into();
            child.started_at = Some(now);
            child.last_tool = Some(["read", "validate", "codebase_search"][idx % 3].into());
            child.last_tool_activity = Some(ToolActivitySummary::new(
                child.last_tool.clone().unwrap_or_else(|| "tool".into()),
                Some(format!("{} delegate activity", scenario.id())),
            ));
            child.last_turn = Some((idx + 1) as u32);
        }
    });

    for idx in 0..total {
        std::thread::sleep(Duration::from_millis(450));
        update_delegate(&progress, |p| {
            let child = &mut p.children[idx];
            let terminal = delegate_terminal_status(scenario, idx);
            child.status = terminal.0.into();
            child.failure_kind = terminal.1;
            child.completed_at = Some(SystemTime::now());
            child.tasks_done = if child.status == "completed" {
                child.tasks.len()
            } else {
                child.tasks.len().saturating_sub(1)
            };
            child.result_summary = Some(format!("{} result for {}", scenario.id(), child.label));
            child.result_viewed = !matches!(child.status.as_str(), "completed_unviewed");
            p.completed = p
                .children
                .iter()
                .filter(|child| matches!(child.status.as_str(), "completed" | "completed_unviewed"))
                .count();
            p.failed = p.children.iter().filter(|child| child.status == "failed").count();
            p.pending_results = p
                .children
                .iter()
                .filter(|child| !child.result_viewed && matches!(child.status.as_str(), "completed" | "completed_unviewed"))
                .count();
            p.running = p
                .children
                .iter()
                .filter(|child| child.status == "running")
                .count();
        });
        send_plan(&tx, &local_tx, scenario, idx + 1, (idx + 1).min(total - 1), false);
    }

    std::thread::sleep(Duration::from_millis(250));
    update_delegate(&progress, |p| {
        p.active = false;
        p.running = 0;
    });
    send_plan(&tx, &local_tx, scenario, total, total.saturating_sub(1), true);
}

fn delegate_terminal_status(
    scenario: SmokeScenarioKind,
    idx: usize,
) -> (&'static str, Option<DelegateChildFailureKind>) {
    match scenario {
        SmokeScenarioKind::DelegateBasic => match idx {
            1 => ("failed", Some(DelegateChildFailureKind::Unknown)),
            _ => ("completed", None),
        },
        SmokeScenarioKind::DelegatePendingResult => match idx {
            0 => ("completed_unviewed", None),
            1 => ("failed", Some(DelegateChildFailureKind::ProviderStartup)),
            _ => ("completed", None),
        },
        _ => ("completed", None),
    }
}

fn update(progress: &Arc<Mutex<CleaveProgress>>, f: impl FnOnce(&mut CleaveProgress)) {
    if let Ok(mut progress) = progress.lock() {
        f(&mut progress);
    }
}

fn update_delegate(progress: &Arc<Mutex<DelegateProgress>>, f: impl FnOnce(&mut DelegateProgress)) {
    if let Ok(mut progress) = progress.lock() {
        f(&mut progress);
    }
}

fn send_plan(
    tx: &Option<broadcast::Sender<AgentEvent>>,
    local_tx: &Option<std::sync::mpsc::Sender<AgentEvent>>,
    scenario: SmokeScenarioKind,
    completed: usize,
    active_idx: usize,
    finished: bool,
) {
    let event = AgentEvent::PlanUpdated {
        projection: smoke_plan(scenario, completed, active_idx, finished),
    };
    if let Some(tx) = tx {
        let _ = tx.send(event.clone());
    }
    if let Some(local_tx) = local_tx {
        let _ = local_tx.send(event);
    }
}

fn smoke_plan(
    scenario: SmokeScenarioKind,
    completed: usize,
    active_idx: usize,
    finished: bool,
) -> PlanSurfaceProjection {
    let labels = scenario_plan_labels(scenario);
    let total = labels.len();
    let items = labels
        .iter()
        .enumerate()
        .map(|(idx, (id, label))| PlanItemProjection {
            id: Some((*id).into()),
            label: (*label).into(),
            status: if idx < completed || finished && idx + 1 == total {
                "done"
            } else if idx == active_idx {
                "active"
            } else {
                "todo"
            }
            .into(),
            intent: None,
            writable: false,
        })
        .collect();
    PlanSurfaceProjection {
        version: 1,
        active: Some(PlanLaneProjection {
            plan_id: format!("smoke:{}", scenario.id()),
            mode: "smoke".into(),
            guidance: format!("{} smoke driving shared live projections", scenario.title()),
            status: if finished { "done" } else { "active" }.into(),
            scope: "session".into(),
            source: "smoke".into(),
            progress: PlanProgressProjection { completed, total },
            items,
        }),
        workstreams: vec![PlanWorkstreamProjection {
            id: format!("smoke:{}", scenario.id()),
            title: format!("{} — shared surfaces", scenario.title()),
            status: if finished { "complete" } else { "active" }.into(),
            progress: PlanProgressProjection { completed, total },
        }],
        ..Default::default()
    }
}

fn scenario_plan_labels(scenario: SmokeScenarioKind) -> Vec<(&'static str, &'static str)> {
    match scenario {
        SmokeScenarioKind::CleaveDocsResearch => vec![
            ("research", "Collect source evidence"),
            ("outline", "Outline document structure"),
            ("draft", "Draft body copy"),
            ("review", "Review claims and citations"),
            ("publish", "Prepare publication notes"),
        ],
        SmokeScenarioKind::DelegateBasic => vec![
            ("dispatch", "Dispatch delegate work"),
            ("review", "Render delegate review result"),
            ("verify", "Render delegate verification failure"),
        ],
        SmokeScenarioKind::DelegatePendingResult => vec![
            ("dispatch", "Dispatch delegate work"),
            ("pending", "Render pending delegate result"),
            ("failure", "Render failed delegate"),
            ("complete", "Render viewed delegate result"),
        ],
        SmokeScenarioKind::SurfaceStress => (0..12)
            .map(|idx| match idx {
                0 => ("child-01", "Render active scout child with a deliberately long label"),
                1 => ("child-02", "Render timeout failure"),
                2 => ("child-03", "Render completed synthesis child"),
                3 => ("child-04", "Render upstream exhaustion"),
                4 => ("child-05", "Render recovered supervision metadata"),
                5 => ("child-06", "Render merge conflict metadata"),
                6 => ("child-07", "Render completed docs child"),
                7 => ("child-08", "Render completed review child"),
                8 => ("child-09", "Render completed validation child"),
                9 => ("child-10", "Render completed publication child"),
                10 => ("child-11", "Render completed planning child"),
                _ => ("child-12", "Render final stress child"),
            })
            .collect(),
        _ => vec![
            ("start", "Dispatch smoke decomposition"),
            ("children", "Render child completion/failure events"),
            ("exhaustion", "Render exhaustion or validation signals"),
            ("reconcile", "Render parent reconciliation"),
        ],
    }
}

fn initial_cleave_progress(scenario: SmokeScenarioKind) -> CleaveProgress {
    let children = match scenario {
        SmokeScenarioKind::CleaveBasic => vec![
            smoke_child("scout/context", "pending", 0, 2),
            smoke_child("patch/change", "pending", 0, 2),
            smoke_child("verify/tests", "pending", 0, 2),
        ],
        SmokeScenarioKind::CleaveActivity => vec![
            smoke_child("scout/files", "pending", 0, 3),
            smoke_child("search/context", "pending", 0, 3),
            smoke_child("patch/edit", "pending", 0, 3),
            smoke_child("verify/check", "pending", 0, 3),
        ],
        SmokeScenarioKind::CleaveDocsResearch => vec![
            smoke_child("research/sources", "pending", 0, 2),
            smoke_child("outline/structure", "pending", 0, 2),
            smoke_child("draft/body", "pending", 0, 2),
            smoke_child("review/claims", "pending", 0, 2),
            smoke_child("publish/changelog", "pending", 0, 2),
        ],
        SmokeScenarioKind::SurfaceStress => (1..=12)
            .map(|idx| {
                smoke_child(
                    &format!("stress/child-{idx:02}-long-label-for-truncation"),
                    "pending",
                    0,
                    3,
                )
            })
            .collect(),
        _ => vec![
            smoke_child("smoke-pass", "pending", 0, 2),
            smoke_child("smoke-fail", "pending", 0, 2),
            smoke_child("smoke-exhausted", "pending", 0, 2),
        ],
    };
    CleaveProgress {
        active: true,
        run_id: scenario.id().into(),
        total_children: children.len(),
        completed: 0,
        failed: 0,
        children,
        total_tokens_in: 0,
        total_tokens_out: 0,
    }
}

fn initial_delegate_progress(scenario: SmokeScenarioKind) -> DelegateProgress {
    let children = match scenario {
        SmokeScenarioKind::DelegatePendingResult => vec![
            delegate_child("delegate_1", "review/security", "running", false, 0, 2),
            delegate_child("delegate_2", "verify/tests", "running", true, 0, 2),
            delegate_child("delegate_3", "research/prior-art", "running", true, 0, 2),
        ],
        _ => vec![
            delegate_child("delegate_1", "review/security", "running", true, 0, 2),
            delegate_child("delegate_2", "verify/tests", "running", true, 0, 2),
        ],
    };
    DelegateProgress {
        active: true,
        running: children.len(),
        completed: 0,
        failed: 0,
        pending_results: 0,
        children,
    }
}

fn smoke_child(label: &str, status: &str, done: usize, total: usize) -> ChildProgress {
    ChildProgress {
        label: label.into(),
        status: status.into(),
        failure_kind: None,
        duration_secs: None,
        supervision_mode: None,
        pid: None,
        last_tool: None,
        last_tool_activity: None,
        last_turn: None,
        tasks: task_items(label, done, total),
        tasks_done: done,
        started_at: None,
        last_activity_at: None,
        tokens_in: 0,
        tokens_out: 0,
        runtime: Some(ChildRuntimeSummary {
            model: Some("smoke:model".into()),
            thinking_level: Some("minimal".into()),
            context_class: Some("compact".into()),
            enabled_tools: vec!["bash".into(), "read".into(), "validate".into()],
            disabled_tools: Vec::new(),
            skills: vec!["smoke".into()],
            enabled_extensions: Vec::new(),
            disabled_extensions: Vec::new(),
            preloaded_files: Vec::new(),
        }),
    }
}

fn delegate_child(
    task_id: &str,
    label: &str,
    status: &str,
    result_viewed: bool,
    done: usize,
    total: usize,
) -> DelegateProgressChild {
    DelegateProgressChild {
        task_id: task_id.into(),
        label: label.into(),
        status: status.into(),
        result_viewed,
        last_tool: None,
        last_tool_activity: None,
        last_turn: None,
        started_at: None,
        completed_at: None,
        result_summary: None,
        failure_kind: None,
        tasks: task_items(label, done, total),
        tasks_done: done,
    }
}

fn task_items(label: &str, done: usize, total: usize) -> Vec<ChildTaskItem> {
    (0..total)
        .map(|idx| ChildTaskItem {
            description: format!("{label} task {}", idx + 1),
            done: idx < done,
        })
        .collect()
}
