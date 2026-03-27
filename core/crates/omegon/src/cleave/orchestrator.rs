//! Cleave orchestrator — the main dispatch loop.
//!
//! Spawns omegon-agent children in git worktrees, manages dependency waves,
//! tracks state, and merges results.

use super::guardrails;
use super::plan::CleavePlan;
use super::progress::{self, ChildProgressStatus, ProgressEvent, SharedProgressSink};
use super::state::{self, ChildStatus, CleaveState};
use super::waves::compute_waves;
use super::worktree;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Configuration for a cleave run.
pub struct CleaveConfig {
    pub agent_binary: PathBuf,
    pub bridge_path: PathBuf,
    pub node: String,
    pub model: String,
    pub max_parallel: usize,
    pub timeout_secs: u64,
    pub idle_timeout_secs: u64,
    pub max_turns: u32,
    /// Provider inventory for per-child routing. If None, all children use `model`.
    pub inventory: Option<std::sync::Arc<tokio::sync::RwLock<crate::routing::ProviderInventory>>>,
    /// Startup-approved secret env inherited from the parent process.
    pub inherited_env: Vec<(String, String)>,
    /// Embedding-aware sink for live progress events.
    pub progress_sink: SharedProgressSink,
}

/// Result of a cleave run.
pub struct CleaveResult {
    pub state: CleaveState,
    pub merge_results: Vec<(String, MergeOutcome)>,
    pub duration_secs: f64,
}

pub enum MergeOutcome {
    Success,
    Conflict(String),
    Failed(String),
    Skipped(String),
}

/// Run the full cleave orchestration.
pub async fn run_cleave(
    plan: &CleavePlan,
    directive: &str,
    repo_path: &Path,
    workspace_path: &Path,
    config: &CleaveConfig,
    cancel: CancellationToken,
) -> Result<CleaveResult> {
    let started = Instant::now();

    std::fs::create_dir_all(workspace_path).context("Failed to create workspace directory")?;

    let state_path = workspace_path.join("state.json");

    // Resume from existing state.json if present (TS caller pre-populated it
    // with worktree paths, enriched task files, etc.)
    let mut state = if state_path.exists() {
        let mut s = CleaveState::load(&state_path)?;
        s.started_at = Some(Instant::now());
        tracing::info!("resuming from existing state.json");
        s
    } else {
        let run_id = format!("clv-{}-{}", nanoid(8), nanoid(4));
        CleaveState::from_plan(
            &run_id,
            directive,
            repo_path,
            workspace_path,
            plan,
            &config.model,
        )
    };
    state.save(&state_path)?;

    let waves = compute_waves(&plan.children);
    tracing::info!(
        waves = waves.len(),
        children = plan.children.len(),
        "cleave dispatch starting"
    );

    let semaphore = Arc::new(Semaphore::new(config.max_parallel));

    // Discover project guardrails once for all children
    let guardrail_checks = guardrails::discover_guardrails(repo_path);
    let guardrail_section = guardrails::format_guardrail_section(&guardrail_checks);

    for (wave_idx, wave) in waves.iter().enumerate() {
        if cancel.is_cancelled() {
            tracing::warn!("cleave cancelled");
            break;
        }

        let wave_labels: Vec<&str> = wave
            .iter()
            .map(|&i| plan.children[i].label.as_str())
            .collect();
        tracing::info!(wave = wave_idx, children = ?wave_labels, "dispatching wave");
        config.progress_sink.emit(&ProgressEvent::WaveStart {
            wave: wave_idx,
            children: wave_labels.iter().map(|s| s.to_string()).collect(),
        });

        // ── Prepare children (worktrees, task files, status) ────────────
        struct ChildDispatchInfo {
            child_idx: usize,
            wt_path: PathBuf,
            label: String,
            prompt: String,
        }
        let mut to_dispatch: Vec<ChildDispatchInfo> = Vec::new();

        for &child_idx in wave {
            let label = state.children[child_idx].label.clone();
            let branch = state.children[child_idx].branch.clone().unwrap();

            // Use existing worktree if the TS caller already created it,
            // otherwise create one
            let existing_wt = state.children[child_idx]
                .worktree_path
                .as_ref()
                .filter(|p| std::path::Path::new(p).exists());
            let wt_result = if let Some(wt) = existing_wt {
                Ok(PathBuf::from(wt))
            } else {
                worktree::create_worktree(repo_path, workspace_path, child_idx, &label, &branch)
            };
            match wt_result {
                Ok(wt_path) => {
                    state.children[child_idx].worktree_path =
                        Some(wt_path.to_string_lossy().to_string());

                    // Initialize submodules in the worktree so children
                    // can access files inside them
                    if let Err(e) = worktree::submodule_init(&wt_path) {
                        tracing::warn!(child = %label, "submodule init failed: {e}");
                    }

                    // Verify scope files are accessible after submodule init
                    let scope = &state.children[child_idx].scope;
                    let missing = worktree::verify_scope_accessible(&wt_path, scope);
                    if !missing.is_empty() {
                        let msg = format!(
                            "scope file(s) not accessible after submodule init: {}",
                            missing.join(", ")
                        );
                        tracing::error!(child = %label, "{msg}");
                        state.children[child_idx].status = ChildStatus::Failed;
                        state.children[child_idx].error = Some(msg);
                        continue;
                    }

                    // Read existing task file (written by TS with OpenSpec enrichment)
                    // or generate a basic one if absent
                    let task_path = workspace_path.join(format!("{}-task.md", child_idx));
                    let mut task_content = if task_path.exists() {
                        std::fs::read_to_string(&task_path)?
                    } else {
                        let description = &state.children[child_idx].description;
                        let content = build_task_file(
                            child_idx,
                            &label,
                            description,
                            scope,
                            directive,
                            &state.children,
                            &guardrail_section,
                            repo_path,
                        );
                        std::fs::write(&task_path, &content)?;
                        content
                    };

                    // Inject submodule context into task file if scope crosses
                    // a submodule boundary
                    if let Some(submod_note) = worktree::build_submodule_context(&wt_path, scope) {
                        // Insert before the first ## heading after the frontmatter,
                        // or append if no good insertion point
                        if let Some(pos) = task_content.find("\n## ") {
                            let insert_at = pos + 1;
                            task_content.insert_str(insert_at, &format!("{submod_note}\n"));
                        } else {
                            task_content.push_str(&format!("\n{submod_note}"));
                        }
                        // Rewrite the task file with submodule context
                        std::fs::write(&task_path, &task_content)?;
                    }

                    state.children[child_idx].status = ChildStatus::Running;

                    to_dispatch.push(ChildDispatchInfo {
                        child_idx,
                        wt_path,
                        label,
                        prompt: task_content,
                    });
                }
                Err(e) => {
                    state.children[child_idx].status = ChildStatus::Failed;
                    state.children[child_idx].error =
                        Some(format!("Worktree creation failed: {e}"));
                    tracing::error!(child = %label, "worktree failed: {e}");
                }
            }
        }
        state.save(&state_path)?;

        // ── Emit task inventories ────────────────────────────────────────
        for info in &to_dispatch {
            let task_count = progress::count_task_items(&info.prompt);
            let scope_files = state.children[info.child_idx].scope.len();
            config
                .progress_sink
                .emit(&ProgressEvent::ChildTaskInventory {
                    child: info.label.clone(),
                    total_tasks: task_count,
                    scope_files,
                });
        }

        // ── Dispatch children ───────────────────────────────────────────
        let mut handles = Vec::new();

        for info in to_dispatch {
            let sem = semaphore.clone();
            let child_cancel = cancel.clone();
            let agent_binary = config.agent_binary.clone();
            let bridge_path = config.bridge_path.clone();
            let node = config.node.clone();
            let max_turns = config.max_turns;
            let timeout_secs = config.timeout_secs;
            let idle_timeout_secs = config.idle_timeout_secs;
            let progress_sink = config.progress_sink.clone();

            // Route per-child model: if inventory is available and child has no explicit model,
            // infer capability tier from scope size and route to best provider+model.
            let (model, provider_id) = if let Some(ref inv_lock) = config.inventory {
                let child_state = &state.children[info.child_idx];
                if child_state
                    .execute_model
                    .as_deref()
                    .is_none_or(|m| m == config.model)
                {
                    let inv = inv_lock.read().await;
                    let tier = crate::routing::infer_capability_tier(child_state.scope.len());
                    let req = crate::routing::CapabilityRequest {
                        tier,
                        prefer_local: false,
                        avoid_providers: vec![],
                    };
                    let candidates = crate::routing::route(&req, &inv);
                    if let Some(best) = candidates.first() {
                        let routed = format!("{}:{}", best.provider_id, best.model_id);
                        tracing::info!(child = %info.label, tier = %tier, routed = %routed, "per-child routing");
                        (routed, Some(best.provider_id.clone()))
                    } else {
                        (config.model.clone(), Some(crate::providers::infer_provider_id(&config.model)))
                    }
                } else {
                    let model = child_state
                        .execute_model
                        .clone()
                        .unwrap_or_else(|| config.model.clone());
                    let provider_id = child_state
                        .provider_id
                        .clone()
                        .or_else(|| Some(crate::providers::infer_provider_id(&model)));
                    (model, provider_id)
                }
            } else {
                let model = config.model.clone();
                let provider_id = Some(crate::providers::infer_provider_id(&model));
                (model, provider_id)
            };
            state.children[info.child_idx].execute_model = Some(model.clone());
            state.children[info.child_idx].provider_id = provider_id;

            let inherited_env = config.inherited_env.clone();
            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let dispatch_config = ChildDispatchConfig {
                    agent_binary: &agent_binary,
                    bridge_path: &bridge_path,
                    node: &node,
                    model: &model,
                    max_turns,
                    timeout_secs,
                    idle_timeout_secs,
                    inherited_env: &inherited_env,
                    progress_sink,
                };
                let result = dispatch_child(
                    &dispatch_config,
                    &info.wt_path,
                    &info.label,
                    &info.prompt,
                    child_cancel,
                )
                .await;
                (info.child_idx, result)
            });
            handles.push(handle);
        }

        // ── Harvest results ─────────────────────────────────────────────
        for handle in handles {
            let (child_idx, result) = handle.await?;
            let label = &state.children[child_idx].label.clone();

            match result {
                Ok(output) => {
                    state.children[child_idx].status = ChildStatus::Completed;
                    state.children[child_idx].duration_secs = Some(output.duration_secs);
                    state.children[child_idx].stdout_log_path = output.stdout_log_path.clone();
                    state.children[child_idx].stderr_log_path = output.stderr_log_path.clone();
                    state.children[child_idx].session_path = output.session_path.clone();
                    tracing::info!(
                        child = %label,
                        duration = format!("{:.0}s", output.duration_secs),
                        session_path = ?output.session_path,
                        stdout_log = ?output.stdout_log_path,
                        stderr_log = ?output.stderr_log_path,
                        "child completed"
                    );

                    // Salvage any uncommitted work (submodules + parent).
                    let auto_committed =
                        salvage_worktree_changes(&state.children[child_idx], false);
                    if auto_committed > 0 {
                        config.progress_sink.emit(&ProgressEvent::AutoCommit {
                            child: label.clone(),
                            files: auto_committed,
                        });
                    }

                    config.progress_sink.emit(&ProgressEvent::ChildStatus {
                        child: label.clone(),
                        status: ChildProgressStatus::Completed,
                        duration_secs: Some(output.duration_secs),
                        error: None,
                    });
                }
                Err(e) => {
                    state.children[child_idx].status = ChildStatus::Failed;
                    state.children[child_idx].error = Some(format!("{e}"));
                    tracing::error!(child = %label, error = %e, "child failed");

                    // Salvage whatever work the child produced before failing.
                    // A timed-out or errored child may have made real edits
                    // inside a submodule that would otherwise be silently lost.
                    let salvaged = salvage_worktree_changes(&state.children[child_idx], true);
                    if salvaged > 0 {
                        tracing::info!(
                            child = %label,
                            files = salvaged,
                            "salvaged changes from failed child"
                        );
                    }

                    config.progress_sink.emit(&ProgressEvent::ChildStatus {
                        child: label.clone(),
                        status: ChildProgressStatus::Failed,
                        duration_secs: Some(started.elapsed().as_secs_f64()),
                        error: Some(format!("{e}")),
                    });
                }
            }
        }
        state.save(&state_path)?;
    }

    // ── Merge phase ─────────────────────────────────────────────────────
    tracing::info!("merge phase starting");
    config.progress_sink.emit(&ProgressEvent::MergeStart);
    let mut merge_results = Vec::new();

    for child in &mut state.children {
        // Skip children that have no chance of useful commits
        let dominated_skip = match child.status {
            ChildStatus::Completed => false,
            ChildStatus::Failed => false, // Try merge — salvage may have produced commits
            _ => true,
        };
        if dominated_skip {
            merge_results.push((
                child.label.clone(),
                MergeOutcome::Skipped(format!("status: {:?}", child.status)),
            ));
            continue;
        }

        let branch = child.branch.as_deref().unwrap();

        // If the child failed before producing a mergeable branch or any salvaged
        // work, preserve the original child failure instead of overwriting it with
        // a branch-not-found merge error.
        if child.status == ChildStatus::Failed {
            let branch_exists = if omegon_git::jj::is_jj_repo(repo_path) {
                false
            } else {
                std::process::Command::new("git")
                    .args(["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")])
                    .current_dir(repo_path)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            };
            if !branch_exists {
                let reason = child
                    .error
                    .clone()
                    .unwrap_or_else(|| "child failed before producing mergeable work".into());
                merge_results.push((
                    child.label.clone(),
                    MergeOutcome::Skipped(format!("preserving child failure: {reason}")),
                ));
                config.progress_sink.emit(&ProgressEvent::MergeResult {
                    child: child.label.clone(),
                    success: false,
                    detail: Some(format!("preserving child failure: {reason}")),
                });
                continue;
            }
        }

        // Remove worktree first so the branch is unlocked.
        // Clear the path after removal so the final cleanup loop does not
        // attempt a second removal and emit backend warnings.
        if let Some(wt) = child.worktree_path.take() {
            let _ = worktree::remove_worktree(repo_path, Path::new(&wt));
        }

        let is_salvage = child.status == ChildStatus::Failed;
        let merge_msg = format!(
            "feat({}): {}",
            child.label,
            child.description.lines().next().unwrap_or(&child.label)
        );
        // Squash-merge: compress all child diary commits into one clean commit.
        // The intermediate edit/fix/re-edit history has no bisect value for cleave children.
        match worktree::squash_merge_branch(repo_path, branch, &merge_msg) {
            Ok(worktree::MergeResult::Success) => {
                if is_salvage {
                    tracing::info!(child = %child.label, "merged salvaged work from failed child");
                } else {
                    tracing::info!(child = %child.label, "merged successfully");
                }
                let _ = worktree::delete_branch(repo_path, branch);
                merge_results.push((child.label.clone(), MergeOutcome::Success));
                config.progress_sink.emit(&ProgressEvent::MergeResult {
                    child: child.label.clone(),
                    success: true,
                    detail: None,
                });
            }
            Ok(worktree::MergeResult::Conflict(detail)) => {
                tracing::warn!(child = %child.label, "merge conflict");
                merge_results.push((child.label.clone(), MergeOutcome::Conflict(detail.clone())));
                config.progress_sink.emit(&ProgressEvent::MergeResult {
                    child: child.label.clone(),
                    success: false,
                    detail: Some(detail),
                });
            }
            Ok(worktree::MergeResult::Failed(detail)) => {
                tracing::error!(child = %child.label, detail = %detail, "merge failed — demoting child to failed");
                child.status = ChildStatus::Failed;
                child.error = Some(detail.clone());
                let _ = worktree::delete_branch(repo_path, branch);
                merge_results.push((child.label.clone(), MergeOutcome::Failed(detail.clone())));
                config.progress_sink.emit(&ProgressEvent::MergeResult {
                    child: child.label.clone(),
                    success: false,
                    detail: Some(detail),
                });
            }
            Err(e) => {
                child.status = ChildStatus::Failed;
                child.error = Some(format!("{e}"));
                merge_results.push((child.label.clone(), MergeOutcome::Failed(format!("{e}"))));
                config.progress_sink.emit(&ProgressEvent::MergeResult {
                    child: child.label.clone(),
                    success: false,
                    detail: Some(format!("{e}")),
                });
            }
        }
    }

    // Clean up remaining worktrees
    for child in &state.children {
        if let Some(wt) = &child.worktree_path {
            let _ = worktree::remove_worktree(repo_path, Path::new(wt));
        }
    }

    let duration_secs = started.elapsed().as_secs_f64();
    state.save(&state_path)?;

    let completed = state
        .children
        .iter()
        .filter(|c| c.status == ChildStatus::Completed)
        .count();
    let failed = state
        .children
        .iter()
        .filter(|c| c.status == ChildStatus::Failed)
        .count();

    // Post-merge guardrails are handled by the caller (TS wrapper or CLI).
    // The orchestrator only discovers guardrails for task file enrichment.

    config.progress_sink.emit(&ProgressEvent::Done {
        completed,
        failed,
        duration_secs,
    });

    Ok(CleaveResult {
        state,
        merge_results,
        duration_secs,
    })
}

struct ChildOutput {
    duration_secs: f64,
    stdout: String,
    stderr_tail: String,
    stderr_full: String,
    session_path: Option<String>,
    stdout_log_path: Option<String>,
    stderr_log_path: Option<String>,
}

fn summarize_child_failure(stderr_full: &str) -> Option<String> {
    let lines: Vec<&str> = stderr_full
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    for line in lines.iter().rev() {
        let lower = line.to_ascii_lowercase();
        let looks_causal = lower.contains("error")
            || lower.contains("failed")
            || lower.contains("panic")
            || lower.contains("invalid_request")
            || lower.contains("no llm provider available")
            || lower.contains("authentication")
            || lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("timeout");
        let obviously_noise = lower.contains("event bus finalized")
            || lower.contains("default tool profile applied")
            || lower.contains("memory backend loaded")
            || lower.contains("memory snapshot for tui")
            || lower.contains("startup secret preflight")
            || lower.contains("registered feature")
            || lower.contains("repo model active")
            || lower.contains("omegon-agent starting");
        if looks_causal && !obviously_noise {
            return Some(line.to_string());
        }
    }

    lines.last().map(|s| s.to_string())
}

/// Configuration for dispatching a child agent process.
struct ChildDispatchConfig<'a> {
    agent_binary: &'a Path,
    bridge_path: &'a Path,
    node: &'a str,
    model: &'a str,
    max_turns: u32,
    timeout_secs: u64,
    idle_timeout_secs: u64,
    inherited_env: &'a [(String, String)],
    progress_sink: SharedProgressSink,
}

/// Dispatch a single omegon-agent child process.
async fn dispatch_child(
    config: &ChildDispatchConfig<'_>,
    cwd: &Path,
    label: &str,
    prompt: &str,
    cancel: CancellationToken,
) -> Result<ChildOutput> {
    let started = Instant::now();

    tracing::info!(child = %label, cwd = %cwd.display(), "spawning omegon-agent");
    tracing::info!(child = %label, binary = %config.agent_binary.display(), bridge = %config.bridge_path.display(), node = %config.node, model = %config.model, max_turns = config.max_turns, "dispatch params");

    // Verify cwd exists
    if !cwd.exists() {
        anyhow::bail!("Child cwd does not exist: {}", cwd.display());
    }

    // Write prompt to a temp file to avoid CLI arg parsing issues
    // (task file content starting with --- breaks clap's arg parser)
    let prompt_file = cwd.join(".cleave-prompt.md");
    tracing::info!(child = %label, prompt_file = %prompt_file.display(), prompt_len = prompt.len(), "writing prompt file");
    std::fs::write(&prompt_file, prompt)
        .context(format!("Failed to write prompt file for child '{label}'"))?;

    let max_turns_str = config.max_turns.to_string();
    // Don't pass --bridge by default — let children auto-detect native providers.
    // Forcing --bridge bypasses native providers entirely, which breaks children
    // when the bridge script path is relative or node_modules are missing.
    let mut args = vec![
        "--prompt-file",
        prompt_file.to_str().unwrap(),
        "--cwd",
        cwd.to_str().unwrap(),
        "--model",
        config.model,
        "--max-turns",
        &max_turns_str,
    ];
    if std::env::var("OMEGON_FORCE_BRIDGE").is_ok() {
        args.extend(["--bridge", config.bridge_path.to_str().unwrap()]);
        args.extend(["--node", config.node]);
    }
    tracing::info!(child = %label, args = ?args, "spawn args");

    tracing::info!(
        child = %label,
        inherited_env = config.inherited_env.len(),
        inherited_env_names = ?config.inherited_env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        "child secret env inheritance"
    );
    let mut child = Command::new(config.agent_binary);
    child
        .args(&args)
        .env("OMEGON_CHILD", "1") // Signal child mode — read-only memory, no session save
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in config.inherited_env {
        child.env(key, value);
    }
    let mut child = child
        .spawn()
        .context(format!("Failed to spawn omegon-agent for child '{label}'"))?;

    let pid = child.id().unwrap_or(0);
    tracing::info!(child = %label, pid, "child spawned");
    config.progress_sink.emit(&ProgressEvent::ChildSpawned {
        child: label.to_string(),
        pid,
    });
    // Note: child_spawned already signals "running" to the TS handler.
    // No separate child_status(Running) needed.

    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr).lines();
    let mut stderr_tail: std::collections::VecDeque<String> = std::collections::VecDeque::with_capacity(20);
    let logs_dir = cwd.join(".omegon-child-logs");
    let _ = std::fs::create_dir_all(&logs_dir);
    let stdout_log_path = logs_dir.join(format!("{}.stdout.log", label));
    let stderr_log_path = logs_dir.join(format!("{}.stderr.log", label));
    let mut stderr_log = tokio::fs::File::create(&stderr_log_path).await.ok();

    let wall_timeout = tokio::time::Duration::from_secs(config.timeout_secs);
    let idle_timeout = tokio::time::Duration::from_secs(config.idle_timeout_secs);

    let mut last_activity = Instant::now();
    let mut last_activity_event = Instant::now() - std::time::Duration::from_secs(2); // allow first event immediately

    tracing::info!(child = %label, wall_timeout_secs = config.timeout_secs, idle_timeout_secs = config.idle_timeout_secs, "entering IO loop");

    let io_result = tokio::select! {
        _ = tokio::time::sleep(wall_timeout) => {
            tracing::warn!(child = %label, timeout = config.timeout_secs, "wall-clock timeout");
            Err(anyhow::anyhow!("Wall-clock timeout after {}s", config.timeout_secs))
        }
        _ = cancel.cancelled() => {
            tracing::warn!(child = %label, "cancelled");
            Err(anyhow::anyhow!("Cancelled"))
        }
        result = async {
            let mut line_count = 0u64;
            loop {
                match tokio::time::timeout(idle_timeout, reader.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        last_activity = Instant::now();
                        line_count += 1;
                        if let Some(file) = stderr_log.as_mut() {
                            let _ = file.write_all(line.as_bytes()).await;
                            let _ = file.write_all(b"\n").await;
                        }
                        if stderr_tail.len() == 20 {
                            stderr_tail.pop_front();
                        }
                        stderr_tail.push_back(line.clone());

                        // Emit activity events (throttled to 1/sec)
                        if last_activity.duration_since(last_activity_event).as_secs() >= 1
                            && let Some(activity) = progress::parse_child_activity(label, &line) {
                                config.progress_sink.emit(&activity);
                                last_activity_event = Instant::now();
                            }

                        if line_count <= 5 || line_count.is_multiple_of(50) {
                            tracing::info!(child = %label, line_count, "stderr: {line}");
                        } else {
                            tracing::debug!(child = %label, "{line}");
                        }
                    }
                    Ok(Ok(None)) => {
                        tracing::info!(child = %label, line_count, "stderr EOF — child exited");
                        break;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(child = %label, "stderr read error: {e}");
                        break;
                    }
                    Err(_) => {
                        let idle_secs = last_activity.elapsed().as_secs();
                        tracing::warn!(child = %label, idle_secs, line_count, "idle timeout");
                        return Err(anyhow::anyhow!(
                            "Idle timeout — no output for {}s", config.idle_timeout_secs
                        ));
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        } => {
            result
        }
    };

    // kill_on_drop will handle cleanup, but be explicit
    let _ = child.kill().await;
    let exit = child.wait().await?;
    tracing::info!(child = %label, exit_code = ?exit.code(), success = exit.success(), "child process exited");

    let mut stdout_buf = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut stdout_buf).await;
    }
    let _ = std::fs::write(&stdout_log_path, &stdout_buf);
    let stderr_tail_text = stderr_tail.into_iter().collect::<Vec<_>>().join("\n");
    let stderr_full_text = std::fs::read_to_string(&stderr_log_path).unwrap_or_default();
    let session_path = cwd.join(".cleave-session.json");
    let session_path = session_path.exists().then(|| session_path.display().to_string());

    let duration_secs = started.elapsed().as_secs_f64();

    match io_result {
        Ok(()) if exit.success() => Ok(ChildOutput {
            duration_secs,
            stdout: stdout_buf,
            stderr_tail: stderr_tail_text,
            stderr_full: std::fs::read_to_string(&stderr_log_path).unwrap_or_default(),
            session_path,
            stdout_log_path: Some(stdout_log_path.display().to_string()),
            stderr_log_path: Some(stderr_log_path.display().to_string()),
        }),
        Ok(()) => {
            let summary = summarize_child_failure(&stderr_full_text)
                .unwrap_or_else(|| crate::util::truncate(&stderr_tail_text, 1200));
            Err(anyhow::anyhow!(
                "Child exited with code {}\nlog: {}\nsummary: {}",
                exit.code().unwrap_or(-1),
                stderr_log_path.display(),
                summary
            ))
        }
        Err(e) => {
            let summary = summarize_child_failure(&stderr_full_text)
                .unwrap_or_else(|| crate::util::truncate(&stderr_tail_text, 1200));
            Err(anyhow::anyhow!(
                "{}\nlog: {}\nsummary: {}",
                e,
                stderr_log_path.display(),
                summary
            ))
        }
    }
}

/// Salvage uncommitted work from a child's worktree.
///
/// Runs `commit_dirty_submodules` (for files inside submodules) then
/// `auto_commit_worktree` (for remaining parent-level changes).
/// Called on BOTH success and failure paths so that work from timed-out
/// or errored children is preserved.
fn salvage_worktree_changes(child: &state::ChildState, is_failure: bool) -> usize {
    let wt_path = match child.worktree_path.as_deref() {
        Some(wt) => Path::new(wt),
        None => return 0,
    };
    if !wt_path.exists() {
        return 0;
    }

    let label = &child.label;
    let scope = &child.scope;

    if is_failure {
        tracing::warn!(
            child = %label,
            "attempting to salvage changes from failed child worktree"
        );
    }

    // 1. Commit dirty submodules first — children often write inside
    // submodules but only the parent git sees the pointer change.
    match worktree::commit_dirty_submodules(wt_path, label) {
        Ok(n) if n > 0 => {
            tracing::info!(child = %label, submodules = n, "auto-committed dirty submodules");
        }
        Err(e) => {
            tracing::warn!(child = %label, "submodule auto-commit failed: {e}");
        }
        _ => {}
    }

    // 2. Commit any remaining uncommitted changes in the parent worktree.
    auto_commit_worktree(wt_path, label, scope)
}

fn auto_commit_worktree(wt_path: &Path, label: &str, scope: &[String]) -> usize {
    if !wt_path.exists() {
        return 0;
    }

    // Check for uncommitted changes (excluding .cleave-prompt.md which is always present)
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(wt_path)
        .output();

    let changed_files: Vec<String> = match &status {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout
                .lines()
                .filter_map(|line| {
                    let file = line.get(3..)?.trim();
                    if file.is_empty() || file.starts_with(".cleave-prompt") {
                        None
                    } else {
                        Some(file.to_string())
                    }
                })
                .collect()
        }
        Err(_) => return 0,
    };

    if changed_files.is_empty() {
        tracing::info!(child = %label, "no real changes to auto-commit (only .cleave-prompt.md)");
        return 0;
    }

    // Filter to files matching the child's scope (if scope is non-empty).
    // An empty scope means "any file is fine" (trust the child).
    let in_scope: Vec<&String> = if scope.is_empty() {
        changed_files.iter().collect()
    } else {
        changed_files
            .iter()
            .filter(|f| scope.iter().any(|s| f.starts_with(s.trim_end_matches('/'))))
            .collect()
    };

    let out_of_scope = changed_files.len() - in_scope.len();
    if out_of_scope > 0 {
        tracing::warn!(
            child = %label,
            out_of_scope,
            "skipping {out_of_scope} file(s) outside declared scope"
        );
    }

    if in_scope.is_empty() {
        tracing::info!(child = %label, "no in-scope changes to auto-commit");
        return 0;
    }

    let file_count = in_scope.len();
    tracing::info!(child = %label, files = file_count, "auto-committing uncommitted changes in worktree");

    // Stage only in-scope files
    let mut add_args = vec!["add", "--"];
    let in_scope_strs: Vec<&str> = in_scope.iter().map(|s| s.as_str()).collect();
    add_args.extend(in_scope_strs);
    let _ = std::process::Command::new("git")
        .args(&add_args)
        .current_dir(wt_path)
        .output();

    // Commit
    let commit_msg = format!("chore(cleave): auto-commit work from child '{label}'");
    let result = std::process::Command::new("git")
        .args(["commit", "-m", &commit_msg, "--no-verify"])
        .current_dir(wt_path)
        .output();

    match result {
        Ok(out) if out.status.success() => {
            tracing::info!(child = %label, "auto-commit succeeded");
            file_count
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(child = %label, "auto-commit failed: {}", stderr.trim());
            0
        }
        Err(e) => {
            tracing::warn!(child = %label, "auto-commit error: {e}");
            0
        }
    }
}

fn build_task_file(
    child_idx: usize,
    label: &str,
    description: &str,
    scope: &[String],
    directive: &str,
    siblings: &[super::state::ChildState],
    guardrail_section: &str,
    repo_path: &Path,
) -> String {
    let scope_list = scope
        .iter()
        .map(|s| format!("- `{s}`"))
        .collect::<Vec<_>>()
        .join("\n");

    // Sibling context
    let sibling_list: String = siblings
        .iter()
        .filter(|s| s.label != label)
        .map(|s| format!("- **{}**: {}", s.label, s.description))
        .collect::<Vec<_>>()
        .join("\n");

    let depends_on = &siblings
        .iter()
        .find(|s| s.label == label)
        .map(|s| &s.depends_on)
        .cloned()
        .unwrap_or_default();
    let dep_note = if depends_on.is_empty() {
        "**Depends on:** none (independent)".to_string()
    } else {
        format!("**Depends on:** {}", depends_on.join(", "))
    };

    let sibling_section = if sibling_list.is_empty() {
        String::new()
    } else {
        format!("\n## Siblings\n\n{sibling_list}\n")
    };

    // Language-aware test convention
    let test_convention = if scope
        .iter()
        .any(|s| s.ends_with(".rs") || s.contains("crates/"))
    {
        "Write tests as #[test] functions in the same file or a tests submodule"
    } else if scope
        .iter()
        .any(|s| s.ends_with(".py") || s.contains("python"))
    {
        "Write tests using pytest in co-located test_*.py files"
    } else {
        "Write tests for new functions and changed behavior — co-locate as *.test.ts"
    };

    // Discover project context for this child's scope
    let ctx = super::context::discover_child_context(repo_path, scope);
    let context_sections = super::context::format_context_sections(&ctx);

    // Testing section — includes convention and any directives from task content
    let testing_section = if let Some(ref example) = ctx.test_example {
        // When we have a test example, include it in a richer Testing Requirements section
        let directives = super::context::TestingDirectives::default();
        let mut ts = super::context::format_testing_section(&directives, test_convention);
        if ts.is_empty() {
            // No directives but still show convention
            ts = format!("## Testing Requirements\n\n### Test Convention\n\n{test_convention}\n\n");
        }
        ts.push_str(&format!(
            "Example from codebase:\n\n```rust\n{example}\n```\n\n"
        ));
        ts
    } else {
        format!("## Testing Requirements\n\n### Test Convention\n\n{test_convention}\n\n")
    };

    format!(
        r#"---
task_id: {child_idx}
label: {label}
siblings: [{sibling_refs}]
---

# Task {child_idx}: {label}

## Root Directive

> {directive}

## Mission

{description}

## Scope

{scope_list}

{dep_note}
{sibling_section}
{context_sections}
{guardrail_section}
{testing_section}
## Contract

1. Only work on files within your scope
2. Follow the Testing Requirements section above
3. If the task is too complex, set status to NEEDS_DECOMPOSITION

{finalization}
## Result

**Status:** PENDING

**Summary:**

**Artifacts:**

**Decisions Made:**

**Assumptions:**
"#,
        sibling_refs = siblings
            .iter()
            .filter(|s| s.label != label)
            .map(|s| format!("{}:{}", s.child_id, s.label))
            .collect::<Vec<_>>()
            .join(", "),
        finalization = ctx.finalization,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_constants_are_sane() {
        // These mirror the TS-side constants in dispatcher.ts.
        // If the TS defaults change, update the Rust CLI defaults too.
        let wall_clock_secs: u64 = 15 * 60; // 15 minutes
        let idle_secs: u64 = 3 * 60; // 3 minutes

        assert!(
            idle_secs < wall_clock_secs,
            "idle must be shorter than wall-clock"
        );
        assert!(
            idle_secs >= 60,
            "idle timeout must be at least 60s for slow tool calls"
        );
        assert!(
            wall_clock_secs >= 300,
            "wall-clock must be at least 5 minutes"
        );
        assert!(
            wall_clock_secs <= 3600,
            "wall-clock should not exceed 1 hour"
        );
    }

    #[test]
    fn cleave_config_accepts_custom_idle_timeout() {
        let config = CleaveConfig {
            agent_binary: PathBuf::from("/usr/bin/omegon-agent"),
            bridge_path: PathBuf::from("/usr/lib/bridge.mjs"),
            node: "test".into(),
            model: "anthropic:claude-sonnet-4-6".into(),
            max_parallel: 4,
            timeout_secs: 900,
            idle_timeout_secs: 300, // custom: 5 minutes
            max_turns: 50,
            inventory: None,
            inherited_env: vec![],
            progress_sink: crate::cleave::progress::stdout_progress_sink(),
        };
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.timeout_secs, 900);
    }

    #[test]
    fn infer_provider_id_from_model_spec_for_reporting() {
        assert_eq!(crate::providers::infer_provider_id("anthropic:claude-sonnet-4-6"), "anthropic");
        assert_eq!(crate::providers::infer_provider_id("openai:gpt-5.4"), "openai");
        assert_eq!(crate::providers::infer_provider_id("openai-codex:gpt-5.4"), "openai-codex");
    }

    #[test]
    fn build_task_file_handles_sparse_child_state_ids() {
        let siblings = vec![
            crate::cleave::state::ChildState {
                child_id: 0,
                label: "alpha".into(),
                description: "Do alpha work".into(),
                scope: vec!["src/".into()],
                depends_on: vec![],
                status: crate::cleave::state::ChildStatus::Pending,
                error: None,
                branch: Some("cleave/0-alpha".into()),
                worktree_path: None,
                backend: "native".into(),
                execute_model: None,
                provider_id: None,
                duration_secs: None,
            },
            crate::cleave::state::ChildState {
                child_id: 2,
                label: "gamma".into(),
                description: "Do gamma work".into(),
                scope: vec!["tests/".into()],
                depends_on: vec!["alpha".into()],
                status: crate::cleave::state::ChildStatus::Pending,
                error: None,
                branch: Some("cleave/2-gamma".into()),
                worktree_path: None,
                backend: "native".into(),
                execute_model: None,
                provider_id: None,
                duration_secs: None,
            },
        ];

        let task = build_task_file(
            2,
            "gamma",
            "Do gamma work",
            &["tests/".into()],
            "Fix bugs",
            &siblings,
            "",
            Path::new("/tmp/nonexistent"),
        );

        assert!(task.contains("siblings: [0:alpha]"));
        assert!(task.contains("**alpha**: Do alpha work"));
        assert!(!task.contains("1:"));
    }

    #[test]
    fn merge_cleanup_takes_worktree_path_before_final_cleanup() {
        let mut child = crate::cleave::state::ChildState {
            child_id: 0,
            label: "alpha".into(),
            description: "Do alpha work".into(),
            scope: vec!["src/".into()],
            depends_on: vec![],
            status: crate::cleave::state::ChildStatus::Completed,
            error: None,
            branch: Some("cleave/0-alpha".into()),
            worktree_path: Some("/tmp/example-worktree".into()),
            backend: "native".into(),
            execute_model: None,
            provider_id: None,
            duration_secs: None,
        };

        let taken = child.worktree_path.take();
        assert_eq!(taken.as_deref(), Some("/tmp/example-worktree"));
        assert!(child.worktree_path.is_none());
    }
}

#[test]
fn build_task_file_includes_all_sections() {
    let siblings = vec![
        crate::cleave::state::ChildState {
            child_id: 0,
            label: "alpha".into(),
            description: "Do alpha work".into(),
            scope: vec!["src/".into()],
            depends_on: vec![],
            status: crate::cleave::state::ChildStatus::Pending,
            error: None,
            branch: Some("cleave/0-alpha".into()),
            worktree_path: None,
            backend: "native".into(),
            execute_model: None,
            provider_id: None,
            duration_secs: None,
        },
        crate::cleave::state::ChildState {
            child_id: 1,
            label: "beta".into(),
            description: "Do beta work".into(),
            scope: vec!["tests/".into()],
            depends_on: vec!["alpha".into()],
            status: crate::cleave::state::ChildStatus::Pending,
            error: None,
            branch: Some("cleave/1-beta".into()),
            worktree_path: None,
            backend: "native".into(),
            execute_model: None,
            provider_id: None,
            duration_secs: None,
        },
    ];
    let guardrails = "## Project Guardrails\n\n1. **typecheck**: `tsc`\n";

    let task = build_task_file(
        1,
        "beta",
        "Do beta work",
        &["tests/".into()],
        "Fix bugs",
        &siblings,
        guardrails,
        Path::new("/tmp/nonexistent"),
    );

    // Frontmatter
    assert!(task.contains("task_id: 1"), "missing task_id");
    assert!(task.contains("label: beta"), "missing label");
    assert!(task.contains("0:alpha"), "missing sibling ref");

    // Content
    assert!(task.contains("## Mission"), "missing Mission");
    assert!(task.contains("Do beta work"), "missing description");
    assert!(task.contains("- `tests/`"), "missing scope");
    assert!(task.contains("**Depends on:** alpha"), "missing dependency");

    // Siblings section
    assert!(task.contains("## Siblings"), "missing siblings section");
    assert!(
        task.contains("**alpha**: Do alpha work"),
        "missing sibling detail"
    );

    // Guardrails
    assert!(task.contains("## Project Guardrails"), "missing guardrails");
    assert!(task.contains("typecheck"), "missing guardrail check");

    // Contract + Result
    assert!(task.contains("## Contract"), "missing contract");
    assert!(task.contains("## Result"), "missing result");
    assert!(
        task.contains("**Status:** PENDING"),
        "missing pending status"
    );
}

#[test]
fn build_task_file_rust_scope_gets_rust_test_convention() {
    let siblings = vec![crate::cleave::state::ChildState {
        child_id: 0,
        label: "rust-child".into(),
        description: "Fix Rust code".into(),
        scope: vec!["crates/omegon/".into()],
        depends_on: vec![],
        status: crate::cleave::state::ChildStatus::Pending,
        error: None,
        branch: None,
        worktree_path: None,
        backend: "native".into(),
        execute_model: None,
        provider_id: None,
        duration_secs: None,
    }];
    let task = build_task_file(
        0,
        "rust-child",
        "Fix Rust code",
        &["crates/omegon/".into()],
        "Fix",
        &siblings,
        "",
        Path::new("/tmp/nonexistent"),
    );
    assert!(
        task.contains("#[test]"),
        "Rust scope should get #[test] convention, got: {}",
        task.lines().find(|l| l.contains("test")).unwrap_or("none")
    );
}

fn nanoid(len: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let chars = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = String::with_capacity(len);
    let mut n = seed;
    for _ in 0..len {
        result.push(chars[(n % 35) as usize] as char);
        n = n.wrapping_mul(6364136223846793005).wrapping_add(1);
    }
    result
}
