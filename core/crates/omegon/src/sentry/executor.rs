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
use super::{BudgetConfig, RoutingConfig};
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
    routing: Option<Arc<RoutingConfig>>,
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
                    &routing,
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
    routing: &Option<Arc<RoutingConfig>>,
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
                        routing.clone(),
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
                        routing.clone(),
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
                        routing.clone(),
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
                        routing.clone(),
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
                routing.clone(),
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
    routing: Option<Arc<RoutingConfig>>,
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
            &task_id, &model, &cwd, &cancel, &routing,
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
    routing: &Option<Arc<RoutingConfig>>,
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

    let (effective_model, classification) = resolve_model(spec.model.as_deref(), model, &spec.prompt, routing, state_db).await;
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

        let is_code_act = spec.execution_mode.as_deref() == Some("code-act");
        tracing::info!(
            task = %task_id,
            model = %effective_model,
            max_turns,
            timeout_secs,
            attempt,
            code_act = is_code_act,
            "executing sentry task"
        );

        let task_result = if is_code_act {
            run_code_act_task(&spec.prompt, &effective_model, effective_cwd, timeout_secs, cancel).await
        } else {
            run_agent_task(
                &spec.prompt, &effective_model, effective_cwd,
                max_turns, timeout_secs, spec.token_budget, cancel,
            ).await
        };

        match task_result {
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

                if let Some(ref class) = classification {
                    let _ = state_db.record_routing_outcome(
                        task_id, class, &effective_model,
                        result.exit_code == 0, result.tokens_used, result.duration_secs,
                    );
                }
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
                    if let Some(ref class) = classification {
                        let _ = state_db.record_routing_outcome(
                            task_id, class, &effective_model, false, 0, 0,
                        );
                    }
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

async fn resolve_model<'a>(
    spec_model: Option<&'a str>,
    default_model: &'a str,
    task_prompt: &str,
    routing: &Option<Arc<RoutingConfig>>,
    state_db: &StateDb,
) -> (String, Option<String>) {
    match spec_model {
        Some("auto") => {
            if let Some(routing) = routing {
                let complexity = classify_task_complexity(&routing.prefilter_model, task_prompt).await;
                let class_name = format!("{complexity:?}");
                let model = match complexity {
                    TaskComplexity::Simple => {
                        tracing::info!(routed = %routing.light_model, class = %class_name, "model routing");
                        routing.light_model.clone()
                    }
                    TaskComplexity::Moderate => {
                        if should_escalate(&class_name, state_db) {
                            tracing::info!(routed = %routing.heavy_model, class = %class_name, "model routing: adaptive escalation");
                            routing.heavy_model.clone()
                        } else {
                            tracing::info!(routed = %routing.light_model, class = %class_name, "model routing");
                            routing.light_model.clone()
                        }
                    }
                    TaskComplexity::Complex => {
                        tracing::info!(routed = %routing.heavy_model, class = %class_name, "model routing");
                        routing.heavy_model.clone()
                    }
                };
                (model, Some(class_name))
            } else {
                (default_model.to_string(), None)
            }
        }
        Some(explicit) => (explicit.to_string(), None),
        None => (default_model.to_string(), None),
    }
}

pub(crate) fn should_escalate(class_name: &str, state_db: &StateDb) -> bool {
    let stats = match state_db.routing_stats() {
        Ok(s) => s,
        Err(_) => return false,
    };
    if stats.total < 10 {
        return false;
    }
    for (class, total, successes, _tokens) in &stats.by_class {
        if class == class_name && *total >= 5 {
            let success_rate = *successes as f64 / *total as f64;
            if success_rate < 0.7 {
                tracing::info!(
                    class = %class_name,
                    success_rate = %format!("{:.0}%", success_rate * 100.0),
                    "adaptive routing: escalating to heavy model"
                );
                return true;
            }
        }
    }
    false
}

#[derive(Debug)]
pub(crate) enum TaskComplexity {
    Simple,
    Moderate,
    Complex,
}

async fn classify_task_complexity(prefilter_model: &str, prompt: &str) -> TaskComplexity {
    let truncated = if prompt.len() > 500 { &prompt[..500] } else { prompt };
    let classification_prompt = format!(
        "Classify this task's complexity. Respond with exactly one word.\n\
         SIMPLE: single-step check, yes/no answer, status lookup\n\
         MODERATE: multi-step but well-defined, standard review\n\
         COMPLEX: open-ended analysis, architectural decisions, multi-file changes\n\n\
         Task: \"{truncated}\""
    );

    match crate::providers::quick_completion(prefilter_model, &classification_prompt).await {
        Ok(result) => {
            let trimmed = result.text.trim().to_uppercase();
            tracing::debug!(
                response = %trimmed,
                input_tokens = result.input_tokens,
                output_tokens = result.output_tokens,
                "prefilter classification"
            );
            if trimmed.contains("SIMPLE") {
                TaskComplexity::Simple
            } else if trimmed.contains("COMPLEX") {
                TaskComplexity::Complex
            } else {
                TaskComplexity::Moderate
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "prefilter classification failed — falling back to heuristic");
            classify_heuristic(prompt)
        }
    }
}

pub(crate) fn classify_heuristic(prompt: &str) -> TaskComplexity {
    let lower = prompt.to_lowercase();
    let word_count = prompt.split_whitespace().count();

    const COMPLEX: &[&str] = &[
        "architect", "redesign", "refactor", "migrate", "rewrite",
        "investigate", "analyze", "design", "propose", "evaluate",
    ];
    const SIMPLE: &[&str] = &[
        "check", "status", "verify", "confirm", "list", "count",
    ];

    if COMPLEX.iter().any(|s| lower.contains(s)) || word_count > 100 {
        TaskComplexity::Complex
    } else if SIMPLE.iter().any(|s| lower.contains(s)) || word_count < 20 {
        TaskComplexity::Simple
    } else {
        TaskComplexity::Moderate
    }
}

async fn run_code_act_task(
    prompt: &str,
    model: &str,
    cwd: &Path,
    timeout_secs: u64,
    cancel: &CancellationToken,
) -> anyhow::Result<TaskResult> {
    let start = Instant::now();
    let executor = crate::code_act::CodeActExecutor::permitted(cwd.to_path_buf());
    let mut total_tokens = 0u64;

    let mut gen_prompt = executor.build_prompt(prompt, None);
    let mut last_code = String::new();

    for attempt in 1..=3u32 {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }

        let completion = crate::providers::quick_completion(model, &gen_prompt).await?;
        total_tokens += completion.input_tokens + completion.output_tokens;

        let code = match crate::code_act::CodeActExecutor::extract_code(&completion.text) {
            Some(c) => c,
            None => {
                tracing::warn!(attempt, "code-act: LLM did not produce a code block");
                if attempt < 3 {
                    gen_prompt = executor.build_retry_prompt(
                        prompt, &completion.text, "No Python code block found in response",
                    );
                    continue;
                }
                anyhow::bail!("LLM failed to produce a code block after {attempt} attempts");
            }
        };

        last_code = code.clone();
        let result = executor.execute_script(&code, Some(timeout_secs), cancel.clone()).await?;
        let duration = start.elapsed().as_secs();

        if result.is_error && attempt < 3 {
            tracing::info!(attempt, "code-act: script failed, retrying with error context");
            gen_prompt = executor.build_retry_prompt(prompt, &code, &result.output);
            continue;
        }

        return Ok(TaskResult {
            exit_code: result.exit_code,
            summary: if result.is_error {
                format!("code-act failed: {}", result.output.chars().take(500).collect::<String>())
            } else {
                result.output.chars().take(500).collect()
            },
            tokens_used: total_tokens,
            duration_secs: duration,
            session_id: format!("code-act-{}", super::file_board::uuid_v4()),
        });
    }

    Ok(TaskResult {
        exit_code: 1,
        summary: "code-act exhausted retries".into(),
        tokens_used: total_tokens,
        duration_secs: start.elapsed().as_secs(),
        session_id: format!("code-act-{}", super::file_board::uuid_v4()),
    })
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
