//! Shared smoke drivers for renderer-neutral live surfaces.
//!
//! These drivers exercise the same semantic state consumed by TUI, ACP, IPC,
//! and web projections. They must not write progress as conversation text.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

use crate::child_agent::ChildTaskItem;
use crate::features::cleave::{
    ChildProgress, ChildRuntimeSummary, ChildSupervisionMode, CleaveChildFailureKind,
    CleaveProgress,
};
use crate::surfaces::conversation::ToolActivitySummary;
use omegon_traits::{AgentEvent, PlanItemProjection, PlanLaneProjection, PlanProgressProjection, PlanSurfaceProjection, PlanWorkstreamProjection, SlashCommandResponse};

pub fn launch_cleave_surface_smoke(
    handles: &mut crate::tui::dashboard::DashboardHandles,
    events_tx: Option<broadcast::Sender<AgentEvent>>,
    local_events_tx: Option<std::sync::mpsc::Sender<AgentEvent>>,
) -> SlashCommandResponse {
    let progress = Arc::new(Mutex::new(CleaveProgress {
        active: true,
        run_id: "smoke-cleave".into(),
        total_children: 3,
        completed: 0,
        failed: 0,
        children: vec![
            smoke_child("smoke-pass", "pending", 0, 2),
            smoke_child("smoke-fail", "pending", 0, 2),
            smoke_child("smoke-exhausted", "pending", 0, 2),
        ],
        total_tokens_in: 0,
        total_tokens_out: 0,
    }));

    // DashboardHandles owns the shared surface handle used by web/dashboard
    // projections. It is intentionally replaced for the smoke run so every
    // projection protocol observes the same synthetic operation state.
    handles.cleave = Some(progress.clone());
    let handles_cleave = progress.clone();
    let tx = events_tx.clone();
    let local_tx = local_events_tx.clone();
    std::thread::spawn(move || {
        send_plan(&tx, &local_tx, 0, 0, false);
        update(&handles_cleave, |p| {
            let now = Instant::now();
            for (idx, tool) in ["bash", "read", "delegate"].into_iter().enumerate() {
                p.children[idx].status = "running".into();
                p.children[idx].supervision_mode = Some(ChildSupervisionMode::Attached);
                p.children[idx].started_at = Some(now);
                p.children[idx].last_activity_at = Some(now);
                p.children[idx].last_tool = Some(tool.into());
                p.children[idx].last_turn = Some((idx + 1) as u32);
            }
            p.children[0].last_tool_activity = Some(ToolActivitySummary::new(
                "bash",
                Some("cargo test -p smoke-pass".into()),
            ));
        });
        std::thread::sleep(Duration::from_millis(650));

        update(&handles_cleave, |p| {
            p.completed = 1;
            p.children[0].status = "completed".into();
            p.children[0].duration_secs = Some(0.65);
            p.children[0].tasks_done = 2;
            p.children[0].tokens_in = 1200;
            p.children[0].tokens_out = 240;
            p.children[0].last_tool_activity = Some(ToolActivitySummary::new(
                "bash",
                Some("focused verification passed".into()),
            ));
            p.total_tokens_in = 1200;
            p.total_tokens_out = 240;
        });
        send_plan(&tx, &local_tx, 1, 1, false);
        std::thread::sleep(Duration::from_millis(650));

        update(&handles_cleave, |p| {
            p.failed = 1;
            p.children[1].status = "failed".into();
            p.children[1].failure_kind = Some(CleaveChildFailureKind::ValidationFailed);
            p.children[1].duration_secs = Some(1.3);
            p.children[1].tasks_done = 1;
            p.children[1].tokens_in = 900;
            p.children[1].tokens_out = 180;
            p.children[1].last_tool_activity = Some(ToolActivitySummary::new(
                "validate",
                Some("smoke validation failure".into()),
            ));
            p.total_tokens_in = 2100;
            p.total_tokens_out = 420;
        });
        send_plan(&tx, &local_tx, 2, 2, false);
        std::thread::sleep(Duration::from_millis(650));

        update(&handles_cleave, |p| {
            p.failed = 2;
            p.children[2].status = "upstream_exhausted".into();
            p.children[2].failure_kind = Some(CleaveChildFailureKind::UpstreamExhausted);
            p.children[2].duration_secs = Some(1.95);
            p.children[2].tokens_in = 640;
            p.children[2].tokens_out = 80;
            p.children[2].last_tool_activity = Some(ToolActivitySummary::new(
                "delegate",
                Some("simulated upstream exhaustion".into()),
            ));
            p.total_tokens_in = 2740;
            p.total_tokens_out = 500;
        });
        send_plan(&tx, &local_tx, 3, 3, false);
        std::thread::sleep(Duration::from_millis(650));

        update(&handles_cleave, |p| {
            p.active = false;
        });
        send_plan(&tx, &local_tx, 4, 3, true);
    });

    SlashCommandResponse {
        accepted: true,
        output: Some("Started unified cleave smoke suite on shared live surface projections.".into()),
    }
}

fn update(progress: &Arc<Mutex<CleaveProgress>>, f: impl FnOnce(&mut CleaveProgress)) {
    if let Ok(mut progress) = progress.lock() {
        f(&mut progress);
    }
}

fn send_plan(
    tx: &Option<broadcast::Sender<AgentEvent>>,
    local_tx: &Option<std::sync::mpsc::Sender<AgentEvent>>,
    completed: usize,
    active_idx: usize,
    finished: bool,
) {
    let event = AgentEvent::PlanUpdated {
        projection: smoke_plan(completed, active_idx, finished),
    };
    if let Some(tx) = tx {
        let _ = tx.send(event.clone());
    }
    if let Some(local_tx) = local_tx {
        let _ = local_tx.send(event);
    }
}

fn smoke_plan(completed: usize, active_idx: usize, finished: bool) -> PlanSurfaceProjection {
    let labels = [
        ("start", "Dispatch smoke cleave wave"),
        ("children", "Render child completion/failure events"),
        ("exhaustion", "Render upstream exhaustion signal"),
        ("merge", "Render merge completion"),
    ];
    let items = labels
        .iter()
        .enumerate()
        .map(|(idx, (id, label))| PlanItemProjection {
            id: Some((*id).into()),
            label: (*label).into(),
            status: if idx < completed || finished && idx == 3 {
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
            plan_id: "smoke:cleave".into(),
            mode: "smoke".into(),
            guidance: "Unified surface smoke driving shared live operation projections".into(),
            status: if finished { "done" } else { "active" }.into(),
            scope: "session".into(),
            source: "smoke".into(),
            progress: PlanProgressProjection { completed, total: 4 },
            items,
        }),
        workstreams: vec![PlanWorkstreamProjection {
            id: "smoke:cleave".into(),
            title: "Cleave smoke — shared surfaces".into(),
            status: if finished { "complete" } else { "active" }.into(),
            progress: PlanProgressProjection { completed, total: 4 },
        }],
        ..Default::default()
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
        tasks: (0..total)
            .map(|idx| ChildTaskItem {
                description: format!("{label} task {}", idx + 1),
                done: idx < done,
            })
            .collect(),
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
