//! Cleave orchestrator — the main dispatch loop.
//!
//! Spawns omegon-agent children in git worktrees, manages dependency waves,
//! tracks state, and merges results.

use super::guardrails;
use super::plan::{CleaveChildRuntimeProfile, CleavePlan};
use super::progress::{self, ChildProgressStatus, ProgressEvent, SharedProgressSink};
use super::state::{self, ChildStatus, CleaveState};
use super::waves::compute_waves;
use super::worktree;
use crate::child_agent::{
    ChildAgentRuntimeProfile, ChildAgentSpawnConfig, spawn_headless_child_agent,
    write_child_prompt_file,
};
use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Structured child failure — separates upstream exhaustion from logic/tool failures
/// so the orchestrator can decide whether to retry with a fallback provider.
#[derive(Debug)]
enum ChildError {
    /// Exit code 2: upstream provider exhausted all retries (rate limit, provider down).
    UpstreamExhausted { provider: String, message: String },
    /// Exit code 1 or I/O error: task/logic failure, not retryable by switching provider.
    Failed(String),
}

impl std::fmt::Display for ChildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChildError::UpstreamExhausted { provider, message } => {
                write!(f, "[upstream-exhausted provider={provider}] {message}")
            }
            ChildError::Failed(msg) => write!(f, "{msg}"),
        }
    }
}

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
    /// Extra env vars injected into child agents for deterministic smoke scenarios.
    pub injected_env: Vec<(String, String)>,
    /// Full parent-controlled runtime profile for cleave children.
    pub child_runtime: CleaveChildRuntimeProfile,
    /// Embedding-aware sink for live progress events.
    pub progress_sink: SharedProgressSink,
    /// Optional workflow template for per-phase configuration.
    pub workflow: Option<crate::workflow::WorkflowTemplate>,
}

/// Result of a cleave run.
pub struct CleaveResult {
    pub state: CleaveState,
    pub merge_results: Vec<(String, MergeOutcome)>,
    pub duration_secs: f64,
}

pub enum MergeOutcome {
    Success,
    NoChanges,
    Conflict(String),
    Failed(String),
    Skipped(String),
}

/// Return the requested child model unchanged.
///
/// Cleave must not silently reroute work to another provider behind the operator's
/// back. Warnings and disclosures happen at higher layers; child routing stays
/// honest here.
pub fn resolve_cleave_model(requested_model: &str) -> String {
    requested_model.to_string()
}

/// Run the full cleave orchestration.
pub async fn run_cleave(
    plan: &CleavePlan,
    directive: &str,
    repo_path: &Path,
    workspace_path: &Path,
    config: &CleaveConfig,
    cancel: CancellationToken,
    child_cancel_registry: Option<
        Arc<std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>>,
    >,
) -> Result<CleaveResult> {
    let started = Instant::now();

    // Preserve the operator-requested model for children. Compliance warnings are
    // surfaced elsewhere, but cleave must not silently swap providers.
    let effective_model = resolve_cleave_model(&config.model);

    std::fs::create_dir_all(workspace_path).context("Failed to create workspace directory")?;

    let state_path = workspace_path.join("state.json");

    // Resume from existing state.json if present (TS caller pre-populated it
    // with worktree paths, enriched task files, etc.)
    let mut state = if state_path.exists() {
        let mut s = CleaveState::load(&state_path)?;
        let reconciliation = s.reconcile_running_children();
        s.started_at = Some(Instant::now());
        match (reconciliation.requeued, reconciliation.still_running) {
            (0, 0) => tracing::info!("resuming from existing state.json"),
            (0, still_running) => tracing::warn!(
                still_running,
                "resuming from existing state.json with verified live child processes"
            ),
            (requeued, still_running) => tracing::warn!(
                requeued,
                still_running,
                "resuming from existing state.json after interruption; re-queued stale running children and preserved verified live children"
            ),
        }
        s
    } else {
        let run_id = format!("clv-{}-{}", nanoid(8), nanoid(4));
        CleaveState::from_plan(
            &run_id,
            directive,
            repo_path,
            workspace_path,
            plan,
            &effective_model,
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
            model: String,
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

                    // Route per-child model: explicit plan model wins; if absent, infer from scope size.
                    // Parent model is the floor — we never route to a tier above the parent.
                    let model = if let Some(ref inv_lock) = config.inventory {
                        let child_state = &state.children[child_idx];
                        if let Some(explicit) = &child_state.execute_model {
                            if explicit != &effective_model {
                                tracing::info!(child = %label, model = %explicit, "using explicit plan model");
                                explicit.clone()
                            } else {
                                let inv = inv_lock.read().await;
                                let parent_tier =
                                    crate::routing::infer_model_tier(&effective_model);
                                let scope_tier =
                                    crate::routing::infer_capability_tier(child_state.scope.len());
                                let tier = scope_tier.min(parent_tier);
                                let req = crate::routing::CapabilityRequest {
                                    tier,
                                    prefer_local: false,
                                    avoid_providers: vec![],
                                };
                                let candidates = crate::routing::route(&req, &inv);
                                if let Some(best) = candidates.first() {
                                    let routed = format!("{}:{}", best.provider_id, best.model_id);
                                    tracing::info!(child = %label, scope_tier = %scope_tier, parent_tier = %parent_tier, effective_tier = %tier, routed = %routed, "per-child routing");
                                    routed
                                } else {
                                    effective_model.clone()
                                }
                            }
                        } else {
                            effective_model.clone()
                        }
                    } else {
                        state.children[child_idx]
                            .execute_model
                            .clone()
                            .unwrap_or_else(|| effective_model.clone())
                    };

                    to_dispatch.push(ChildDispatchInfo {
                        child_idx,
                        wt_path,
                        label,
                        prompt: task_content,
                        model,
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
            let child_cancel_registry = child_cancel_registry.clone();
            let agent_binary = config.agent_binary.clone();
            let bridge_path = config.bridge_path.clone();
            let node = config.node.clone();
            let max_turns = config.max_turns;
            let timeout_secs = config.timeout_secs;
            let idle_timeout_secs = config.idle_timeout_secs;
            let progress_sink = config.progress_sink.clone();
            let inherited_env = config.inherited_env.clone();
            let injected_env = config.injected_env.clone();
            let child_runtime = config.child_runtime.clone();

            // Runtime profile model override wins over inventory-routed model.
            let dispatch_model = child_runtime
                .model
                .as_ref()
                .cloned()
                .unwrap_or_else(|| info.model.clone());
            let dispatch_config = ChildDispatchConfig {
                workspace_path: workspace_path.to_path_buf(),
                agent_binary,
                bridge_path,
                node,
                model: dispatch_model,
                max_turns,
                timeout_secs,
                idle_timeout_secs,
                inherited_env,
                injected_env,
                runtime: child_runtime,
                progress_sink,
            };

            let (child_process, pid) =
                spawn_child_process(&dispatch_config, &info.wt_path, &info.label, &info.prompt)?;
            state.mark_child_spawned(info.child_idx, pid);
            if let Some(registry) = &child_cancel_registry
                && let Ok(mut registry) = registry.lock()
            {
                registry.insert(info.label.clone(), child_cancel.clone());
            }
            state.save(&state_path)?;

            let monitor_config = dispatch_config.clone();
            let child_label = info.label.clone();
            let child_idx = info.child_idx;
            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let result = monitor_child_process(
                    monitor_config,
                    child_process,
                    pid,
                    &child_label,
                    child_cancel,
                )
                .await;
                if let Some(registry) = &child_cancel_registry
                    && let Ok(mut registry) = registry.lock()
                {
                    registry.remove(&child_label);
                }
                (child_idx, result)
            });
            handles.push(handle);
        }

        // ── Harvest results ─────────────────────────────────────────────
        for handle in handles {
            let (child_idx, result) = handle.await?;
            let label = &state.children[child_idx].label.clone();

            match result {
                Ok(output) => {
                    state.mark_child_spawned(child_idx, output.pid);
                    state.children[child_idx].status = ChildStatus::Completed;
                    state.children[child_idx].duration_secs = Some(output.duration_secs);
                    state.children[child_idx].stdout = Some(output.stdout.clone());
                    state.children[child_idx].pid = None;
                    tracing::info!(
                        child = %label,
                        duration = format!("{:.0}s", output.duration_secs),
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
                    match e {
                        ChildError::UpstreamExhausted {
                            ref provider,
                            ref message,
                        } => {
                            tracing::warn!(
                                child = %label,
                                provider = %provider,
                                "upstream exhausted — attempting cross-provider fallback"
                            );

                            // Resolve fallback from the inventory, avoiding the failed provider.
                            let fallback_model = if let Some(ref inv_lock) = config.inventory {
                                let inv = inv_lock.read().await;
                                let failed_model = state.children[child_idx]
                                    .execute_model
                                    .as_deref()
                                    .unwrap_or(&config.model);
                                let tier = crate::routing::infer_model_tier(failed_model);
                                let req = crate::routing::CapabilityRequest {
                                    tier,
                                    prefer_local: false,
                                    avoid_providers: vec![provider.clone()],
                                };
                                crate::routing::route(&req, &inv)
                                    .into_iter()
                                    .next()
                                    .map(|c| format!("{}:{}", c.provider_id, c.model_id))
                            } else {
                                None
                            };

                            if let Some(ref fb_model) = fallback_model {
                                tracing::info!(child = %label, fallback = %fb_model, "retrying with fallback model");
                                state.children[child_idx].execute_model = Some(fb_model.clone());

                                let wt_path = state.children[child_idx]
                                    .worktree_path
                                    .as_deref()
                                    .map(PathBuf::from)
                                    .unwrap_or_else(|| {
                                        workspace_path.join(format!("{}-wt-{}", child_idx, label))
                                    });

                                // Re-read the prompt that was written during worktree setup.
                                let fb_prompt =
                                    std::fs::read_to_string(wt_path.join(".cleave-prompt.md"))
                                        .unwrap_or_default();

                                let fb_runtime = state.children[child_idx]
                                    .runtime
                                    .clone()
                                    .unwrap_or_else(|| config.child_runtime.clone());
                                let fb_dispatch = ChildDispatchConfig {
                                    workspace_path: workspace_path.to_path_buf(),
                                    agent_binary: config.agent_binary.clone(),
                                    bridge_path: config.bridge_path.clone(),
                                    node: config.node.clone(),
                                    model: fb_model.clone(),
                                    max_turns: config.max_turns,
                                    timeout_secs: config.timeout_secs,
                                    idle_timeout_secs: config.idle_timeout_secs,
                                    inherited_env: config.inherited_env.clone(),
                                    injected_env: config.injected_env.clone(),
                                    runtime: fb_runtime,
                                    progress_sink: config.progress_sink.clone(),
                                };

                                let fallback_result = match spawn_child_process(
                                    &fb_dispatch,
                                    &wt_path,
                                    label,
                                    &fb_prompt,
                                ) {
                                    Ok((child_process, pid)) => {
                                        state.mark_child_spawned(child_idx, pid);
                                        state.save(&state_path)?;
                                        monitor_child_process(
                                            fb_dispatch,
                                            child_process,
                                            pid,
                                            label,
                                            cancel.clone(),
                                        )
                                        .await
                                    }
                                    Err(e) => Err(classify_child_error(fb_model, e)),
                                };

                                match fallback_result {
                                    Ok(output) => {
                                        state.mark_child_spawned(child_idx, output.pid);
                                        state.children[child_idx].status = ChildStatus::Completed;
                                        state.children[child_idx].duration_secs =
                                            Some(output.duration_secs);
                                        state.children[child_idx].stdout =
                                            Some(output.stdout.clone());
                                        state.children[child_idx].pid = None;
                                        tracing::info!(
                                            child = %label, fallback = %fb_model,
                                            duration = format!("{:.0}s", output.duration_secs),
                                            "child completed via fallback"
                                        );
                                        let ac = salvage_worktree_changes(
                                            &state.children[child_idx],
                                            false,
                                        );
                                        if ac > 0 {
                                            config.progress_sink.emit(&ProgressEvent::AutoCommit {
                                                child: label.clone(),
                                                files: ac,
                                            });
                                        }
                                        config.progress_sink.emit(&ProgressEvent::ChildStatus {
                                            child: label.clone(),
                                            status: ChildProgressStatus::Completed,
                                            duration_secs: Some(output.duration_secs),
                                            error: None,
                                        });
                                    }
                                    Err(fb_e) => {
                                        let combined = format!(
                                            "primary: {message}\nfallback({fb_model}): {fb_e}"
                                        );
                                        tracing::error!(child = %label, "fallback also failed: {fb_e}");
                                        state.children[child_idx].status =
                                            ChildStatus::UpstreamExhausted;
                                        state.children[child_idx].error = Some(combined.clone());
                                        config.progress_sink.emit(&ProgressEvent::ChildStatus {
                                            child: label.clone(),
                                            status: ChildProgressStatus::UpstreamExhausted,
                                            duration_secs: None,
                                            error: Some(combined),
                                        });
                                    }
                                }
                            } else {
                                tracing::error!(child = %label, "no fallback provider available — upstream-exhausted");
                                state.children[child_idx].status = ChildStatus::UpstreamExhausted;
                                state.children[child_idx].error = Some(message.clone());
                                config.progress_sink.emit(&ProgressEvent::ChildStatus {
                                    child: label.clone(),
                                    status: ChildProgressStatus::UpstreamExhausted,
                                    duration_secs: None,
                                    error: Some(message.clone()),
                                });
                            }
                        }

                        ChildError::Failed(msg) => {
                            state.children[child_idx].status = ChildStatus::Failed;
                            state.children[child_idx].error = Some(msg.clone());
                            tracing::error!(child = %label, "child failed: {msg}");

                            let salvaged =
                                salvage_worktree_changes(&state.children[child_idx], true);
                            if salvaged > 0 {
                                tracing::info!(child = %label, files = salvaged, "salvaged changes from failed child");
                            }

                            config.progress_sink.emit(&ProgressEvent::ChildStatus {
                                child: label.clone(),
                                status: ChildProgressStatus::Failed,
                                duration_secs: None,
                                error: Some(msg),
                            });
                        }
                    }
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
            ChildStatus::UpstreamExhausted => false, // Also try merge — fallback may have produced commits
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
                    child.status = ChildStatus::Completed;
                    child.error = Some("merged after salvaging work from a failed child".into());
                    config.progress_sink.emit(&ProgressEvent::ChildStatus {
                        child: child.label.clone(),
                        status: ChildProgressStatus::MergedAfterFailure,
                        duration_secs: child.duration_secs,
                        error: None,
                    });
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
            Ok(worktree::MergeResult::NoChanges) => {
                tracing::info!(child = %child.label, "child completed without repo changes");
                let _ = worktree::delete_branch(repo_path, branch);
                merge_results.push((child.label.clone(), MergeOutcome::NoChanges));
                config.progress_sink.emit(&ProgressEvent::MergeResult {
                    child: child.label.clone(),
                    success: true,
                    detail: Some("no changes".to_string()),
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
    #[allow(dead_code)]
    stdout: String,
    pid: u32,
}

/// Configuration for dispatching a child agent process.
#[derive(Clone)]
struct ChildDispatchConfig {
    workspace_path: PathBuf,
    agent_binary: PathBuf,
    bridge_path: PathBuf,
    node: String,
    model: String,
    max_turns: u32,
    timeout_secs: u64,
    idle_timeout_secs: u64,
    inherited_env: Vec<(String, String)>,
    injected_env: Vec<(String, String)>,
    runtime: CleaveChildRuntimeProfile,
    progress_sink: SharedProgressSink,
}

fn child_runtime_profile(runtime: &CleaveChildRuntimeProfile) -> ChildAgentRuntimeProfile {
    ChildAgentRuntimeProfile {
        context_class: runtime.context_class.clone(),
        thinking_level: runtime.thinking_level.clone(),
        enabled_tools: runtime.enabled_tools.clone(),
        disabled_tools: runtime.disabled_tools.clone(),
        skills: runtime.skills.clone(),
        enabled_extensions: runtime.enabled_extensions.clone(),
        disabled_extensions: runtime.disabled_extensions.clone(),
        preloaded_files: runtime.preloaded_files.clone(),
        persona: runtime.persona.clone(),
    }
}

fn classify_child_error(model: &str, e: anyhow::Error) -> ChildError {
    let provider = model.split(':').next().unwrap_or("unknown").to_string();
    let msg = e.to_string();
    if msg.contains("exit code 2") || msg.contains("upstream exhausted:") {
        ChildError::UpstreamExhausted {
            provider,
            message: msg,
        }
    } else {
        ChildError::Failed(msg)
    }
}

fn child_activity_log_path(config: &ChildDispatchConfig, label: &str) -> PathBuf {
    config
        .workspace_path
        .join(format!("child-{}.activity.log", label))
}

fn append_child_activity_log(path: &Path, line: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

async fn terminate_child_process(
    config: &ChildDispatchConfig,
    child: &mut Child,
) -> Result<std::process::ExitStatus, ChildError> {
    let shutdown_grace = tokio::time::Duration::from_secs(2);
    if child.id().is_some() {
        let _ = child.start_kill();
    }
    match tokio::time::timeout(shutdown_grace, child.wait()).await {
        Ok(Ok(exit)) => Ok(exit),
        Ok(Err(e)) => Err(classify_child_error(&config.model, e.into())),
        Err(_) => {
            let _ = child.kill().await;
            child
                .wait()
                .await
                .map_err(|e| classify_child_error(&config.model, e.into()))
        }
    }
}

fn spawn_child_process(
    config: &ChildDispatchConfig,
    cwd: &Path,
    label: &str,
    prompt: &str,
) -> Result<(Child, u32)> {
    tracing::info!(child = %label, cwd = %cwd.display(), "spawning omegon-agent");
    tracing::info!(child = %label, binary = %config.agent_binary.display(), bridge = %config.bridge_path.display(), node = %config.node, model = %config.model, max_turns = config.max_turns, "dispatch params");
    if !cwd.exists() {
        anyhow::bail!("Child cwd does not exist: {}", cwd.display());
    }
    let prompt_file = write_child_prompt_file(cwd, ".cleave-prompt.md", prompt)?;
    tracing::info!(child = %label, prompt_file = %prompt_file.display(), prompt_len = prompt.len(), "writing prompt file");
    let child_config = ChildAgentSpawnConfig {
        agent_binary: config.agent_binary.clone(),
        model: config.model.clone(),
        max_turns: config.max_turns,
        inherited_env: config.inherited_env.clone(),
        injected_env: config.injected_env.clone(),
        runtime: child_runtime_profile(&config.runtime),
    };
    tracing::info!(child = %label, inherited_env = config.inherited_env.len(), injected_env = config.injected_env.len(), inherited_env_names = ?config.inherited_env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), injected_env_names = ?config.injected_env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), "child env inheritance");
    let (child, pid) = spawn_headless_child_agent(&child_config, cwd, &prompt_file)?;
    tracing::info!(child = %label, pid, "child spawned");
    config.progress_sink.emit(&ProgressEvent::ChildSpawned {
        child: label.to_string(),
        pid,
    });
    Ok((child, pid))
}

async fn monitor_child_process(
    config: ChildDispatchConfig,
    mut child: Child,
    pid: u32,
    label: &str,
    cancel: CancellationToken,
) -> Result<ChildOutput, ChildError> {
    let started = Instant::now();
    let activity_log = child_activity_log_path(&config, label);
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr).lines();
    let mut stderr_tail: VecDeque<String> = VecDeque::with_capacity(30);
    let wall_timeout = tokio::time::Duration::from_secs(config.timeout_secs);
    let idle_timeout = tokio::time::Duration::from_secs(config.idle_timeout_secs);
    let mut last_activity = Instant::now();
    let mut last_activity_event = Instant::now() - std::time::Duration::from_secs(2);
    tracing::info!(child = %label, wall_timeout_secs = config.timeout_secs, idle_timeout_secs = config.idle_timeout_secs, "entering IO loop");
    let io_result = tokio::select! {
        _ = tokio::time::sleep(wall_timeout) => { tracing::warn!(child = %label, timeout = config.timeout_secs, "wall-clock timeout"); Err(classify_child_error(&config.model, anyhow::anyhow!("Wall-clock timeout after {}s", config.timeout_secs))) }
        _ = cancel.cancelled() => { tracing::warn!(child = %label, "cancelled"); Err(classify_child_error(&config.model, anyhow::anyhow!("Cancelled"))) }
        result = async {
            let mut line_count = 0u64;
            loop {
                match tokio::time::timeout(idle_timeout, reader.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        last_activity = Instant::now();
                        line_count += 1;
                        if stderr_tail.len() == 30 { stderr_tail.pop_front(); }
                        stderr_tail.push_back(line.clone());
                        append_child_activity_log(&activity_log, &line);
                        if last_activity.duration_since(last_activity_event).as_secs() >= 1
                            && let Some(activity) = progress::parse_child_activity(label, &line) {
                                config.progress_sink.emit(&activity);
                                last_activity_event = Instant::now();
                            }
                        if line_count <= 5 || line_count.is_multiple_of(50) { tracing::info!(child = %label, line_count, "stderr: {line}"); } else { tracing::debug!(child = %label, "{line}"); }
                    }
                    Ok(Ok(None)) => { tracing::info!(child = %label, line_count, "stderr EOF — child exited"); break; }
                    Ok(Err(e)) => { tracing::warn!(child = %label, "stderr read error: {e}"); break; }
                    Err(_) => {
                        let idle_secs = last_activity.elapsed().as_secs();
                        tracing::warn!(child = %label, idle_secs, line_count, "idle timeout");
                        return Err(classify_child_error(&config.model, anyhow::anyhow!("Idle timeout — no output for {}s", config.idle_timeout_secs)));
                    }
                }
            }
            Ok::<(), ChildError>(())
        } => { result }
    };
    let exit = terminate_child_process(&config, &mut child).await?;
    tracing::info!(child = %label, exit_code = ?exit.code(), success = exit.success(), "child process exited");
    let mut stdout_buf = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use tokio::io::AsyncReadExt;
        let _ = stdout.read_to_string(&mut stdout_buf).await;
    }
    let duration_secs = started.elapsed().as_secs_f64();
    let tail_snippet = |tail: &VecDeque<String>| -> String {
        if tail.is_empty() {
            return String::new();
        }
        let lines: Vec<&str> = tail.iter().map(|s| s.as_str()).collect();
        format!(
            "
--- last {} stderr lines ---
{}
---",
            lines.len(),
            lines.join(
                "
"
            )
        )
    };
    match io_result {
        Ok(()) if exit.success() => Ok(ChildOutput {
            duration_secs,
            stdout: stdout_buf,
            pid,
        }),
        Ok(()) => Err(classify_child_error(
            &config.model,
            anyhow::anyhow!(
                "Child exited with code {}{}",
                exit.code().unwrap_or(-1),
                tail_snippet(&stderr_tail)
            ),
        )),
        Err(e) => Err(classify_child_error(
            &config.model,
            anyhow::anyhow!("{}{}", e, tail_snippet(&stderr_tail)),
        )),
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
            injected_env: vec![],
            child_runtime: crate::cleave::CleaveChildRuntimeProfile::default(),
            progress_sink: crate::cleave::progress::stdout_progress_sink(),
            workflow: None,
        };
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.timeout_secs, 900);
    }

    #[test]
    fn cleave_requested_model_is_not_silently_rewritten_for_oauth_only_anthropic() {
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::set_var("ANTHROPIC_OAUTH_TOKEN", "subscription-token");
            std::env::set_var("OPENAI_API_KEY", "openai-token");
        }

        let resolved = resolve_cleave_model("anthropic:claude-sonnet-4-6");
        assert_eq!(resolved, "anthropic:claude-sonnet-4-6");

        unsafe {
            std::env::remove_var("ANTHROPIC_OAUTH_TOKEN");
            std::env::remove_var("OPENAI_API_KEY");
        }
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
                stdout: None,
                runtime: None,
                pid: None,
                started_at_unix_ms: None,
                last_activity_unix_ms: None,
                adoption_worktree_path: None,
                adoption_model: None,
                supervisor_token: None,
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
                stdout: None,
                runtime: None,
                pid: None,
                started_at_unix_ms: None,
                last_activity_unix_ms: None,
                adoption_worktree_path: None,
                adoption_model: None,
                supervisor_token: None,
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
            stdout: None,
            runtime: None,
            pid: None,
            started_at_unix_ms: None,
            last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
        };

        let taken = child.worktree_path.take();
        assert_eq!(taken.as_deref(), Some("/tmp/example-worktree"));
        assert!(child.worktree_path.is_none());
    }

    #[test]
    fn child_dispatch_paths_are_absolute_to_survive_cwd_switches() {
        let temp = tempfile::tempdir().unwrap();
        let worktree = temp.path().join("child-worktree");
        std::fs::create_dir_all(&worktree).unwrap();

        let canonical_cwd = std::fs::canonicalize(&worktree).unwrap_or_else(|_| worktree.clone());
        let prompt_file = canonical_cwd.join(".cleave-prompt.md");

        assert!(canonical_cwd.is_absolute());
        assert!(prompt_file.is_absolute());
        assert_eq!(prompt_file.parent(), Some(canonical_cwd.as_path()));
        assert_eq!(
            prompt_file.file_name().and_then(|s| s.to_str()),
            Some(".cleave-prompt.md")
        );
    }

    #[test]
    fn resumed_cleave_requeues_stale_running_children() {
        let plan: crate::cleave::plan::CleavePlan = serde_json::from_str(
            r#"{
                "children": [
                    {"label": "alpha", "description": "do alpha", "scope": ["src/"]},
                    {"label": "beta", "description": "do beta", "scope": ["tests/"]}
                ]
            }"#,
        )
        .unwrap();
        let mut state = CleaveState::from_plan(
            "run-1",
            "fix bugs",
            Path::new("/repo"),
            Path::new("/ws"),
            &plan,
            "anthropic:claude-sonnet-4-6",
        );
        state.children[0].status = ChildStatus::Running;
        state.children[0].error = Some("cancelled mid-flight".into());
        state.children[0].duration_secs = Some(9.0);
        state.children[1].status = ChildStatus::Completed;

        let reconciliation = state.reconcile_running_children();

        assert_eq!(reconciliation.requeued, 1);
        assert_eq!(reconciliation.still_running, 0);
        assert_eq!(state.children[0].status, ChildStatus::Pending);
        assert!(state.children[0].error.is_none());
        assert!(state.children[0].duration_secs.is_none());
        assert_eq!(state.children[1].status, ChildStatus::Completed);
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
            stdout: None,
            runtime: None,
            pid: None,
            started_at_unix_ms: None,
            last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
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
            stdout: None,
            runtime: None,
            pid: None,
            started_at_unix_ms: None,
            last_activity_unix_ms: None,
            adoption_worktree_path: None,
            adoption_model: None,
            supervisor_token: None,
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
        stdout: None,
        runtime: None,
        pid: None,
        started_at_unix_ms: None,
        last_activity_unix_ms: None,
        adoption_worktree_path: None,
        adoption_model: None,
        supervisor_token: None,
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
