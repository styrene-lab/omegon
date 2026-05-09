use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::board::TaskBoard;
use super::state_db::StateDb;
use super::types::{TaskError, TaskResult, Trigger};
use super::BudgetConfig;
use crate::triggers::TriggerEvent;

const MAX_RETRIES: u32 = 3;

pub struct BudgetLimits {
    limits: HashMap<String, BudgetConfig>,
}

impl BudgetLimits {
    pub fn from_config(config: &super::SentryConfig) -> Self {
        let mut limits = HashMap::new();
        for task in &config.tasks {
            if let Some(ref budget) = task.budget {
                limits.insert(task.name.clone(), budget.clone());
            }
        }
        Self { limits }
    }

    fn check(&self, state_db: &StateDb, task_id: &str) -> Option<String> {
        let config = self.limits.get(task_id)?;
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let used = state_db.budget_tokens_today(task_id, &today).unwrap_or(0);

        if let Some(limit) = config.max_tokens_per_day {
            if used >= limit {
                return Some(format!(
                    "token budget exhausted: {used}/{limit} tokens today"
                ));
            }
        }

        if let Some(cost_limit) = config.max_cost_per_day_usd {
            let estimated_cost = (used as f64) / 1_000_000.0 * 3.0;
            if estimated_cost >= cost_limit {
                return Some(format!(
                    "cost budget exhausted: ${estimated_cost:.2}/${cost_limit:.2} today (estimated)"
                ));
            }
        }

        None
    }
}

/// In-flight task tracker — prevents double-execution and enables shutdown cleanup.
struct InFlight {
    tasks: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl InFlight {
    fn new() -> Self {
        Self { tasks: std::sync::Mutex::new(std::collections::HashSet::new()) }
    }

    fn try_insert(&self, task_id: &str) -> bool {
        let mut set = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        set.insert(task_id.to_string())
    }

    fn remove(&self, task_id: &str) {
        let mut set = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        set.remove(task_id);
    }

    fn active_ids(&self) -> Vec<String> {
        let set = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        set.iter().cloned().collect()
    }
}

pub async fn run_sentry_loop(
    board: Arc<dyn TaskBoard>,
    state_db: Arc<StateDb>,
    budget_limits: Arc<BudgetLimits>,
    mut trigger_rx: mpsc::Receiver<TriggerEvent>,
    cancel: CancellationToken,
    model: String,
    cwd: PathBuf,
    max_concurrent: usize,
) {
    let in_flight = Arc::new(InFlight::new());
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent.max(1)));

    tracing::info!("sentry loop started — consuming trigger events");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("sentry loop shutting down");
                break;
            }
            Some(event) = trigger_rx.recv() => {
                handle_trigger_event(
                    event,
                    &board,
                    &state_db,
                    &budget_limits,
                    &in_flight,
                    &semaphore,
                    &cancel,
                    &model,
                    &cwd,
                ).await;
            }
        }
    }

    // Wait briefly for in-flight tasks to notice cancellation
    let active = in_flight.active_ids();
    if !active.is_empty() {
        tracing::info!(count = active.len(), "waiting for in-flight tasks to finish");
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

async fn handle_trigger_event(
    event: TriggerEvent,
    board: &Arc<dyn TaskBoard>,
    state_db: &Arc<StateDb>,
    budget_limits: &Arc<BudgetLimits>,
    in_flight: &Arc<InFlight>,
    semaphore: &Arc<tokio::sync::Semaphore>,
    cancel: &CancellationToken,
    model: &str,
    cwd: &Path,
) {
    match event {
        TriggerEvent::Scheduled(config) => {
            tracing::info!(trigger = %config.trigger.name, "scheduled trigger fired");
            let tasks = match board.list_actionable() {
                Ok(t) => t,
                Err(e) => { tracing::error!(error = %e, "list_actionable failed"); return; }
            };
            for task in &tasks {
                let matches = task.triggers.iter().any(|t| match t {
                    Trigger::Cron { .. } => true,
                    Trigger::Manual => true,
                    _ => false,
                });
                if matches {
                    spawn_task_execution(
                        board.clone(), state_db.clone(), budget_limits.clone(),
                        in_flight.clone(), semaphore.clone(), cancel.clone(),
                        task.id.clone(), model.to_string(), cwd.to_path_buf(),
                    );
                }
            }
        }
        TriggerEvent::Webhook { name, payload: _ } => {
            tracing::info!(trigger = %name, "webhook trigger fired");
            let tasks = match board.list_actionable() {
                Ok(t) => t,
                Err(e) => { tracing::error!(error = %e, "list_actionable failed"); return; }
            };
            for task in &tasks {
                let matches = task.triggers.iter().any(|t| matches!(
                    t, Trigger::Webhook { name: n } if *n == name
                ));
                if matches {
                    spawn_task_execution(
                        board.clone(), state_db.clone(), budget_limits.clone(),
                        in_flight.clone(), semaphore.clone(), cancel.clone(),
                        task.id.clone(), model.to_string(), cwd.to_path_buf(),
                    );
                }
            }
        }
        TriggerEvent::FileChanged { trigger_name, paths } => {
            tracing::info!(trigger = %trigger_name, paths = ?paths, "file change trigger fired");
            let tasks = match board.list_actionable() {
                Ok(t) => t,
                Err(e) => { tracing::error!(error = %e, "list_actionable failed"); return; }
            };
            for task in &tasks {
                let matches = task.triggers.iter().any(|t| matches!(t, Trigger::FileWatch { .. }));
                if matches {
                    spawn_task_execution(
                        board.clone(), state_db.clone(), budget_limits.clone(),
                        in_flight.clone(), semaphore.clone(), cancel.clone(),
                        task.id.clone(), model.to_string(), cwd.to_path_buf(),
                    );
                }
            }
        }
        TriggerEvent::GitChanged { trigger_name, kind, detail } => {
            tracing::info!(trigger = %trigger_name, kind = %kind, detail = %detail, "git change trigger fired");
            let tasks = match board.list_actionable() {
                Ok(t) => t,
                Err(e) => { tracing::error!(error = %e, "list_actionable failed"); return; }
            };
            for task in &tasks {
                let matches = task.triggers.iter().any(|t| matches!(t, Trigger::GitEvent { .. }));
                if matches {
                    spawn_task_execution(
                        board.clone(), state_db.clone(), budget_limits.clone(),
                        in_flight.clone(), semaphore.clone(), cancel.clone(),
                        task.id.clone(), model.to_string(), cwd.to_path_buf(),
                    );
                }
            }
        }
        TriggerEvent::ForceRun { task_id } => {
            tracing::info!(task = %task_id, "force-run requested");
            spawn_task_execution(
                board.clone(), state_db.clone(), budget_limits.clone(),
                in_flight.clone(), semaphore.clone(), cancel.clone(),
                task_id, model.to_string(), cwd.to_path_buf(),
            );
        }
    }
}

fn spawn_task_execution(
    board: Arc<dyn TaskBoard>,
    state_db: Arc<StateDb>,
    budget_limits: Arc<BudgetLimits>,
    in_flight: Arc<InFlight>,
    semaphore: Arc<tokio::sync::Semaphore>,
    cancel: CancellationToken,
    task_id: String,
    model: String,
    cwd: PathBuf,
) {
    if !in_flight.try_insert(&task_id) {
        tracing::debug!(task = %task_id, "already in-flight — skipping");
        return;
    }

    let in_flight_cleanup = in_flight.clone();
    let task_id_cleanup = task_id.clone();

    tokio::spawn(async move {
        let _permit = match semaphore.acquire().await {
            Ok(p) => p,
            Err(_) => {
                in_flight_cleanup.remove(&task_id_cleanup);
                return;
            }
        };
        execute_task_with_retry(
            &board, &state_db, &budget_limits,
            &task_id, &model, &cwd, &cancel,
        ).await;
        in_flight_cleanup.remove(&task_id_cleanup);
    });
}

async fn execute_task_with_retry(
    board: &Arc<dyn TaskBoard>,
    state_db: &Arc<StateDb>,
    budget_limits: &BudgetLimits,
    task_id: &str,
    model: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) {
    match board.claim(task_id) {
        Ok(true) => {}
        Ok(false) => {
            tracing::debug!(task = %task_id, "task already claimed — skipping");
            return;
        }
        Err(e) => {
            tracing::error!(task = %task_id, error = %e, "failed to claim task");
            return;
        }
    }

    let spec = match board.task_spec(task_id) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(task = %task_id, error = %e, "failed to get task spec");
            let _ = board.release(task_id);
            return;
        }
    };

    if let Some(reason) = budget_limits.check(state_db, task_id) {
        tracing::warn!(task = %task_id, reason = %reason, "budget exceeded — skipping");
        let _ = board.release(task_id);
        return;
    }

    if let Ok(Some((last_run, _))) = state_db.last_run(task_id) {
        let recent_failures = state_db.recent_failure_count(task_id).unwrap_or(0);
        if recent_failures >= 3 {
            let cooldown = chrono::Duration::minutes((recent_failures as i64) * 10);
            if Utc::now() - last_run < cooldown {
                tracing::warn!(
                    task = %task_id,
                    failures = recent_failures,
                    cooldown_mins = cooldown.num_minutes(),
                    "circuit breaker: too many recent failures — cooling down"
                );
                let _ = board.release(task_id);
                return;
            }
        }
    }

    let effective_model = spec.model.as_deref().unwrap_or(model);
    let effective_cwd = spec.cwd.as_deref().unwrap_or(cwd);
    let max_turns = spec.max_turns.unwrap_or(30);
    let timeout_secs = spec.timeout_secs.unwrap_or(600);

    for attempt in 1..=MAX_RETRIES {
        if cancel.is_cancelled() {
            let _ = board.release(task_id);
            return;
        }

        let run_id = format!("{task_id}-{}", super::file_board::uuid_v4());

        if let Err(e) = state_db.record_run_start(&run_id, task_id) {
            tracing::error!(task = %task_id, error = %e, "failed to record run start");
        }

        tracing::info!(
            task = %task_id,
            model = %effective_model,
            max_turns,
            timeout_secs,
            attempt,
            "executing sentry task"
        );

        match run_agent_task(
            &spec.prompt,
            effective_model,
            effective_cwd,
            max_turns,
            timeout_secs,
            spec.token_budget,
            cancel,
        ).await {
            Ok(result) => {
                tracing::info!(
                    task = %task_id,
                    exit_code = result.exit_code,
                    tokens = result.tokens_used,
                    duration = result.duration_secs,
                    "task completed"
                );
                let _ = state_db.record_run_complete(&run_id, &result);
                record_budget_usage(state_db, task_id, result.tokens_used);

                if result.exit_code == 0 {
                    apply_lifecycle_hooks(effective_cwd, &spec, task_id);
                    let _ = board.complete(task_id, &result);
                } else {
                    let _ = board.release(task_id);
                }
                return;
            }
            Err(e) => {
                let retriable = !e.to_string().contains("authentication")
                    && !e.to_string().contains("config");
                let error = TaskError {
                    message: e.to_string(),
                    retriable,
                    attempt,
                };
                let _ = state_db.record_run_failure(&run_id, &error);

                if !retriable || attempt >= MAX_RETRIES {
                    tracing::error!(
                        task = %task_id,
                        attempt,
                        error = %e,
                        "task failed permanently"
                    );
                    let _ = board.fail(task_id, &error);
                    return;
                }

                let backoff = match attempt {
                    1 => 30,
                    2 => 60,
                    _ => 120,
                };
                tracing::warn!(
                    task = %task_id,
                    attempt,
                    backoff_secs = backoff,
                    error = %e,
                    "task failed — retrying"
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(backoff)).await;
            }
        }
    }
}

fn apply_lifecycle_hooks(cwd: &Path, spec: &super::types::TaskSpec, task_id: &str) {
    if let Some(ref node_id) = spec.design_node_id {
        let docs_dir = cwd.join("docs");
        if docs_dir.is_dir() {
            let nodes = crate::lifecycle::design::scan_design_docs(&docs_dir);
            if let Some(mut node) = nodes.into_values().find(|n| n.id == *node_id) {
                use crate::lifecycle::types::NodeStatus;
                let target = match node.status {
                    NodeStatus::Decided => Some(NodeStatus::Implementing),
                    NodeStatus::Implementing => Some(NodeStatus::Implemented),
                    _ => None,
                };
                if let Some(new_status) = target {
                    match crate::lifecycle::design::update_node(&mut node, |n| {
                        n.status = new_status;
                    }) {
                        Ok(()) => {
                            tracing::info!(
                                task = %task_id,
                                node = %node_id,
                                status = %new_status.as_str(),
                                "advanced design node status"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                task = %task_id,
                                node = %node_id,
                                error = %e,
                                "failed to advance design node"
                            );
                        }
                    }
                }
            } else {
                tracing::debug!(task = %task_id, node = %node_id, "design node not found in docs/");
            }
        }
    }

    if let Some(ref change) = spec.openspec_change {
        tracing::info!(
            task = %task_id,
            change = %change,
            "openspec change linked — stage advancement deferred to tasks.md mutation tool"
        );
    }
}

fn record_budget_usage(state_db: &StateDb, task_id: &str, tokens: u64) {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    if let Err(e) = state_db.record_budget(task_id, &today, tokens) {
        tracing::warn!(task = %task_id, error = %e, "failed to record budget usage");
    }
}

async fn run_agent_task(
    prompt: &str,
    model: &str,
    cwd: &Path,
    max_turns: u32,
    timeout_secs: u64,
    token_budget: Option<u64>,
    global_cancel: &CancellationToken,
) -> anyhow::Result<TaskResult> {
    use omegon_traits::AgentEvent;

    let start = Instant::now();

    let shared_settings = crate::bootstrap::initialize_shared_settings(
        &crate::bootstrap::SettingsInit {
            model,
            cwd,
            cli_posture: None,
            slim: true,
            max_turns,
            apply_profile_posture: false,
        },
    );
    if let Ok(mut s) = shared_settings.lock() {
        s.set_model(model);
    }

    let mut agent = crate::setup::AgentSetup::new(cwd, None, Some(shared_settings.clone())).await?;
    agent.instance_id = crate::paths::instance_id("sentry");
    crate::bootstrap::apply_runtime_posture(
        &mut agent,
        omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
        omegon_traits::OmegonAutonomyMode::OperatorDriven,
    );
    agent.conversation.push_user(prompt.to_string());

    let loop_config = crate::bootstrap::build_loop_config(
        &shared_settings,
        &agent.cwd,
        model,
        crate::bootstrap::LoopConfigOverrides {
            max_retries: 100,
            secrets: Some(agent.secrets.clone()),
            enforce_first_turn_execution_bias: true,
            ..Default::default()
        },
    );

    let bridge = crate::bootstrap::resolve_bridge_or_bail(model).await?;
    let (events_tx, mut events_rx) = crate::bootstrap::wire_event_channel(&agent, 256);

    let total_in = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total_out = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let total_in_t = total_in.clone();
    let total_out_t = total_out.clone();

    let event_task = tokio::spawn(async move {
        while let Ok(event) = events_rx.recv().await {
            match event {
                AgentEvent::TurnStart { turn } => {
                    tracing::info!("sentry ── Turn {turn} ──");
                }
                AgentEvent::ToolStart { name, .. } => {
                    tracing::info!("sentry → {name}");
                }
                AgentEvent::TurnEnd(te) => {
                    total_in_t.fetch_add(te.actual_input_tokens, std::sync::atomic::Ordering::Relaxed);
                    total_out_t.fetch_add(te.actual_output_tokens, std::sync::atomic::Ordering::Relaxed);
                }
                AgentEvent::AgentEnd => break,
                _ => {}
            }
        }
    });

    let cancel = CancellationToken::new();
    let cancel_timeout = cancel.clone();
    let cancel_global = cancel.clone();
    let global_token = global_cancel.clone();

    let timeout_handle = tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(timeout_secs)).await;
        tracing::warn!("sentry task timeout ({timeout_secs}s)");
        cancel_timeout.cancel();
    });

    let global_handle = tokio::spawn(async move {
        global_token.cancelled().await;
        cancel_global.cancel();
    });

    let loop_result = crate::r#loop::run(
        bridge.as_ref(),
        &mut agent.bus,
        &mut agent.context_manager,
        &mut agent.conversation,
        &events_tx,
        cancel.clone(),
        &loop_config,
    ).await;

    timeout_handle.abort();
    global_handle.abort();
    bridge.shutdown().await;
    drop(events_tx);
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), event_task).await;

    let session_id = format!("sentry-{}", Utc::now().format("%Y%m%dT%H%M%S"));
    if let Err(e) = crate::session::save_session(&agent.conversation, cwd, None) {
        tracing::debug!(error = %e, "failed to save sentry session");
    }

    crate::workspace::runtime::cleanup_instance(cwd, &agent.instance_id);

    let elapsed = start.elapsed();
    let in_tokens = total_in.load(std::sync::atomic::Ordering::Relaxed);
    let out_tokens = total_out.load(std::sync::atomic::Ordering::Relaxed);

    if let Some(budget) = token_budget {
        if in_tokens + out_tokens > budget {
            tracing::warn!(
                "sentry token budget exceeded: {} > {budget}",
                in_tokens + out_tokens
            );
        }
    }

    let summary = agent.conversation.last_assistant_text()
        .unwrap_or_default()
        .to_string();

    let exit_code = match &loop_result {
        Ok(()) if cancel.is_cancelled() && global_cancel.is_cancelled() => {
            anyhow::bail!("sentry shutdown during task execution");
        }
        Ok(()) if cancel.is_cancelled() => 3,
        Ok(()) => 0,
        Err(e) if crate::r#loop::is_upstream_exhausted(e) => 2,
        Err(_) => 1,
    };

    if let Err(ref e) = loop_result {
        if exit_code != 0 {
            tracing::error!(error = %e, "sentry task error");
        }
    }

    Ok(TaskResult {
        exit_code,
        summary,
        tokens_used: in_tokens + out_tokens,
        duration_secs: elapsed.as_secs(),
        session_id,
    })
}
