//! Delegate feature — spawn subagents with scoped tasks.
//!
//! Provides three tools for delegating tasks:
//! - `delegate`: spawn a background or synchronous subagent
//! - `delegate_result`: retrieve results from background delegates
//! - `delegate_status`: list all active/completed delegates
//!
//! The feature manages a result store for async delegates and emits notifications
//! when background tasks complete.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type DelegateEventSlot = Arc<Mutex<Option<BusRequestSink>>>;
use std::time::SystemTime;

use crate::autonomy::{
    ApprovalRequest, DecisionPolicy, active_subagent_policy, required_approval_details,
    subagent_policy_for_automation,
};
use crate::child_agent::{
    ChildAgentActivity, ChildAgentBoundary, ChildAgentRuntimeProfile, ChildAgentSpawnConfig,
    ChildPromptKind, ChildTaskItem, child_prompt_relative_path, extract_task_items,
    parse_child_activity, spawn_headless_child_agent, spawn_sandboxed_child_agent,
    write_child_prompt_file,
};
use crate::surfaces::{
    conversation::ToolActivitySummary, operations::OperationWorkbenchProjection,
};
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use omegon_traits::{
    AgentEvent, BusEvent, BusRequest, BusRequestSink, CommandDefinition, CommandResult,
    ContentBlock, ContextInjection, ContextSignals, Feature, NotifyLevel, OperationRef,
    ToolDefinition, ToolResult,
};

/// Agent specification loaded from .omegon/agents/*.md
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    pub is_write_agent: bool,
}

#[derive(Debug, Clone)]
pub struct DelegateProgressChild {
    pub task_id: String,
    pub label: String,
    pub status: String,
    pub result_viewed: bool,
    pub last_tool: Option<String>,
    pub last_tool_activity: Option<ToolActivitySummary>,
    pub last_turn: Option<u32>,
    pub started_at: Option<std::time::SystemTime>,
    pub completed_at: Option<std::time::SystemTime>,
    pub result_summary: Option<String>,
    pub failure_kind: Option<DelegateChildFailureKind>,
    pub tasks: Vec<ChildTaskItem>,
    pub tasks_done: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DelegateProgress {
    pub active: bool,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending_results: usize,
    pub children: Vec<DelegateProgressChild>,
}

/// Status of a delegate task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DelegateTaskStatus {
    Running,
    Completed {
        success: bool,
    },
    Failed {
        error: String,
        kind: DelegateChildFailureKind,
    },
    Cancelled {
        reason: Option<String>,
    },
}

/// A delegate task entry in the result store
#[derive(Debug, Clone)]
pub struct DelegateTask {
    pub task_id: String,
    pub agent_name: Option<String>,
    pub task_description: String,
    pub status: DelegateTaskStatus,
    pub result: Option<String>,
    /// Whether the terminal result has been fetched/acknowledged by the parent lane.
    pub result_viewed: bool,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    /// Live tool activity (updated during streaming).
    pub last_tool: Option<String>,
    pub last_tool_activity: Option<ToolActivitySummary>,
    /// Live turn number (updated during streaming).
    pub last_turn: Option<u32>,
    /// Task checklist items extracted from the delegate prompt.
    pub tasks: Vec<ChildTaskItem>,
}

/// Thread-safe store for delegate task results
#[derive(Debug, Clone)]
pub struct DelegateResultStore {
    tasks: Arc<Mutex<HashMap<String, DelegateTask>>>,
    next_id: Arc<Mutex<u32>>,
}

impl Default for DelegateResultStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DelegateResultStore {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }

    pub fn generate_task_id(&self) -> String {
        let mut next_id = self.next_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        format!("delegate_{}", id)
    }

    pub fn store_task(&self, task: DelegateTask) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.insert(task.task_id.clone(), task);
    }

    pub fn get_task(&self, task_id: &str) -> Option<DelegateTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.get(task_id).cloned()
    }

    pub fn mark_result_viewed(&self, task_id: &str) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.result_viewed = true;
        }
    }

    pub fn pending_terminal_count(&self) -> usize {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .values()
            .filter(|task| {
                !task.result_viewed && !matches!(task.status, DelegateTaskStatus::Running)
            })
            .count()
    }

    pub fn update_task_status(
        &self,
        task_id: &str,
        status: DelegateTaskStatus,
        result: Option<String>,
    ) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = status;
            task.result = result;
            task.result_viewed = false;
            if matches!(
                task.status,
                DelegateTaskStatus::Completed { .. }
                    | DelegateTaskStatus::Failed { .. }
                    | DelegateTaskStatus::Cancelled { .. }
            ) {
                task.completed_at = Some(SystemTime::now());
            }
        }
    }

    pub fn cancel_task(
        &self,
        task_id: &str,
        reason: Option<String>,
    ) -> anyhow::Result<DelegateTaskStatus> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;
        match &task.status {
            DelegateTaskStatus::Running => {
                let status = DelegateTaskStatus::Cancelled {
                    reason: reason.clone(),
                };
                task.status = status.clone();
                task.result = reason;
                task.result_viewed = false;
                task.completed_at = Some(SystemTime::now());
                Ok(status)
            }
            status @ (DelegateTaskStatus::Completed { .. }
            | DelegateTaskStatus::Failed { .. }
            | DelegateTaskStatus::Cancelled { .. }) => Ok(status.clone()),
        }
    }

    pub fn list_all_tasks(&self) -> Vec<DelegateTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.values().cloned().collect()
    }

    /// Check if a task with the same description was already completed
    /// successfully. Returns the task_id if found.
    /// Update live activity state for a running task (tool, turn, task progress).
    pub fn update_task_live_state(
        &self,
        task_id: &str,
        last_tool: Option<String>,
        last_tool_args_summary: Option<String>,
        last_turn: Option<u32>,
    ) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id) {
            if let Some(tool) = last_tool {
                task.last_tool_activity = Some(ToolActivitySummary::new(
                    tool.clone(),
                    last_tool_args_summary.clone(),
                ));
                task.last_tool = Some(tool);
            }
            if let Some(turn) = last_turn {
                task.last_turn = Some(turn);
                // Heuristic: turn N implies tasks 0..N-1 are done
                let heuristic = (turn as usize).saturating_sub(1).min(task.tasks.len());
                for t in task.tasks.iter_mut().take(heuristic) {
                    t.done = true;
                }
            }
        }
    }

    /// Mark a specific task item as done (1-indexed).
    pub fn mark_task_done(&self, task_id: &str, task_index: usize) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(task_id)
            && task_index > 0
            && task_index <= task.tasks.len()
        {
            task.tasks[task_index - 1].done = true;
        }
    }

    pub fn find_completed_by_description(&self, description: &str) -> Option<String> {
        let tasks = self.tasks.lock().unwrap();
        let desc_lower = description.to_lowercase();
        tasks.values().find_map(|t| {
            if matches!(t.status, DelegateTaskStatus::Completed { success: true })
                && t.task_description.to_lowercase() == desc_lower
            {
                Some(t.task_id.clone())
            } else {
                None
            }
        })
    }

    /// Check if a task with this description is already running or has already
    /// been attempted (running, completed, or failed). Prevents the model from
    /// spawning the same task repeatedly when earlier attempts time out or fail.
    pub fn find_any_by_description(&self, description: &str) -> Option<(String, &'static str)> {
        let tasks = self.tasks.lock().unwrap();
        let desc_lower = description.to_lowercase();
        tasks.values().find_map(|t| {
            if t.task_description.to_lowercase() == desc_lower {
                let status_label = match &t.status {
                    DelegateTaskStatus::Running => "running",
                    DelegateTaskStatus::Completed { success: true } => "completed",
                    DelegateTaskStatus::Completed { success: false } => "failed",
                    DelegateTaskStatus::Failed { .. } => "failed",
                    DelegateTaskStatus::Cancelled { .. } => "cancelled",
                };
                Some((t.task_id.clone(), status_label))
            } else {
                None
            }
        })
    }

    /// Return the task description from the most recently created delegate,
    /// regardless of completion status. Used to substitute conversational
    /// non-tasks like "let's resume" with the actual prior work description.
    /// Sorted by started_at to ensure deterministic ordering (HashMap
    /// iteration order is nondeterministic).
    pub fn last_task_description(&self) -> Option<String> {
        let tasks = self.tasks.lock().unwrap();
        let mut sorted: Vec<_> = tasks
            .values()
            .filter(|t| !is_conversational_non_task(&t.task_description))
            .collect();
        sorted.sort_by_key(|t| t.started_at);
        sorted.last().map(|t| t.task_description.clone())
    }

    pub fn progress_snapshot(&self) -> DelegateProgress {
        let tasks = self.list_all_tasks();
        let mut progress = DelegateProgress::default();
        for task in tasks {
            if !task.result_viewed && !matches!(task.status, DelegateTaskStatus::Running) {
                progress.pending_results += 1;
            }
            let status = match &task.status {
                DelegateTaskStatus::Running => {
                    progress.active = true;
                    progress.running += 1;
                    "running"
                }
                DelegateTaskStatus::Completed { success: true } => {
                    progress.completed += 1;
                    "completed"
                }
                DelegateTaskStatus::Completed { success: false } => {
                    progress.failed += 1;
                    "failed"
                }
                DelegateTaskStatus::Failed { .. } => {
                    progress.failed += 1;
                    "failed"
                }
                DelegateTaskStatus::Cancelled { .. } => "cancelled",
            };
            progress.children.push(DelegateProgressChild {
                task_id: task.task_id.clone(),
                label: task
                    .agent_name
                    .clone()
                    .unwrap_or_else(|| task.task_id.clone()),
                status: status.to_string(),
                result_viewed: task.result_viewed,
                last_tool: task.last_tool.clone(),
                last_tool_activity: task.last_tool_activity.clone(),
                last_turn: task.last_turn,
                started_at: Some(task.started_at),
                completed_at: task.completed_at,
                result_summary: task.result.as_ref().map(|r| crate::util::truncate(r, 40)),
                failure_kind: match &task.status {
                    DelegateTaskStatus::Failed { kind, .. } => Some(*kind),
                    DelegateTaskStatus::Completed { success: false } => {
                        Some(DelegateChildFailureKind::Unknown)
                    }
                    DelegateTaskStatus::Cancelled { .. } => None,
                    _ => None,
                },
                tasks: task.tasks.clone(),
                tasks_done: task.tasks.iter().filter(|t| t.done).count(),
            });
        }
        progress.children.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        progress
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DelegateWorkerProfile {
    Scout,
    Patch,
    Verify,
}

impl DelegateWorkerProfile {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("scout") {
            "patch" => Self::Patch,
            "verify" => Self::Verify,
            _ => Self::Scout,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Scout => "scout",
            Self::Patch => "patch",
            Self::Verify => "verify",
        }
    }

    fn prompt_preamble(self) -> &'static str {
        match self {
            Self::Scout => {
                "You are a delegated Omegon scout worker. Read/search only. Do not broaden scope, do not mutate files, and return concise evidence."
            }
            Self::Patch => {
                "You are a delegated Omegon patch worker. Make the smallest justified scoped edit, validate narrowly, and report the touched files plainly."
            }
            Self::Verify => {
                "You are a delegated Omegon verify worker. Run bounded checks, summarize outcomes, and do not edit files."
            }
        }
    }

    fn runtime_profile(
        self,
        scope: Option<&[String]>,
        thinking_level: Option<&str>,
        persona: Option<&str>,
    ) -> ChildAgentRuntimeProfile {
        let mut runtime = ChildAgentRuntimeProfile {
            context_class: Some("compact".to_string()),
            thinking_level: Some(thinking_level.unwrap_or("low").to_string()),
            slim: true, // Workers are narrow-scope — always use compact schemas + lazy injection
            disabled_tools: vec![
                "web_search".into(),
                "design_tree".into(),
                "design_tree_update".into(),
                "openspec_manage".into(),
                "lifecycle_doctor".into(),
                "cleave_assess".into(),
                "cleave_run".into(),
                "delegate".into(),
                "delegate_result".into(),
                "delegate_status".into(),
                "delegate_cancel".into(),
                "request_context".into(),
                "context_compact".into(),
                "context_clear".into(),
                "memory_store".into(),
                "memory_recall".into(),
                "memory_query".into(),
                "memory_archive".into(),
                "memory_supersede".into(),
                "memory_focus".into(),
                "memory_release".into(),
                "memory_episodes".into(),
                "memory_compact".into(),
                "manage_tools".into(),
                "set_model_intent".into(),
                "switch_to_offline_driver".into(),
                "set_thinking_level".into(),
                "agent_journal".into(),
                "auth_status".into(),
            ],
            preloaded_files: scope.map(|s| s.to_vec()).unwrap_or_default(),
            persona: persona.map(ToString::to_string),
            ..Default::default()
        };
        runtime.enabled_tools = match self {
            Self::Scout => vec![
                "read".into(),
                "bash".into(),
                "codebase_search".into(),
                "view".into(),
            ],
            Self::Patch => vec!["read".into(), "edit".into(), "bash".into()],
            Self::Verify => vec!["read".into(), "bash".into()],
        };
        runtime
    }

    fn max_turns(self) -> u32 {
        match self {
            Self::Scout => 4,
            Self::Patch => 6,
            Self::Verify => 4,
        }
    }
}

#[derive(Debug, Clone)]
struct DelegateRuntimeRequest {
    scope: Option<Vec<String>>,
    model: Option<String>,
    thinking_level: Option<String>,
    worker_profile: DelegateWorkerProfile,
}

/// Mock delegate runner for this implementation
/// In a real implementation, this would interface with the actual delegate engine
pub struct DelegateRunner {
    cwd: PathBuf,
    result_store: Arc<DelegateResultStore>,
    sandbox: bool,
    child_agent_binary: Option<PathBuf>,
    wall_timeout_secs: u64,
    idle_timeout_secs: u64,
    dangerously_bypass_permissions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegateChildFailureKind {
    MissingLocalModel,
    MissingCredential,
    ProviderStartup,
    WorkspaceStartup,
    Unknown,
}

/// Detect that a task description lacks actionable content — it's conversational
/// rather than instructional. Uses structural signals instead of a static phrase
/// list so it catches "yeah sure boss, getatit" not just "sure, go ahead".
///
/// Heuristic: very short text with no actionable content is conversational,
/// not a task. Errs heavily toward allowing — false negatives (letting a bad
/// task through) are cheap, false positives (blocking a real task) are not.
pub(crate) fn is_conversational_non_task(task: &str) -> bool {
    let t = task.trim();
    if t.is_empty() {
        return true;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    // Anything over 5 words is almost certainly intentional
    if words.len() > 5 {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    let lower = lower.trim_end_matches(['.', '!', ',', '?']);

    // Structural markers anywhere in the text
    let has_path = t.contains('/')
        || t.contains('\\')
        || [
            ".rs", ".py", ".ts", ".js", ".toml", ".json", ".yaml", ".yml", ".md",
        ]
        .iter()
        .any(|ext| t.contains(ext));
    let has_code = t.contains("::")
        || t.contains("()")
        || t.contains("fn ")
        || t.contains("def ")
        || t.contains("class ")
        || t.contains("struct ");
    if has_path || has_code {
        return false;
    }

    // Actionable verbs — must appear as whole words, not substrings.
    // "testing" should not match "test"; "building" should not match "build".
    // We check that the verb is preceded by a word boundary (start of string,
    // space, or punctuation) and followed by a space or end of string.
    const VERBS: &[&str] = &[
        "add",
        "fix",
        "update",
        "remove",
        "create",
        "change",
        "implement",
        "refactor",
        "move",
        "rename",
        "delete",
        "write",
        "test",
        "run",
        "check",
        "verify",
        "search",
        "find",
        "read",
        "edit",
        "install",
        "build",
        "deploy",
        "debug",
        "investigate",
        "analyze",
        "repair",
        "set up",
        "configure",
        "migrate",
        "convert",
        "extract",
        "document",
        "review",
        "merge",
        "revert",
        "inspect",
        "assess",
        "plan",
    ];
    let lower_words: Vec<&str> = lower.split_whitespace().collect();
    for verb in VERBS {
        if lower_words
            .iter()
            .any(|w| w.trim_matches(|c: char| !c.is_alphanumeric()) == *verb)
        {
            return false;
        }
    }

    // ≤5 words, no structural markers, no actionable verbs → conversational
    true
}

fn classify_delegate_child_failure(stderr: &str, model: &str) -> DelegateChildFailureKind {
    let lower = stderr.to_ascii_lowercase();
    let provider = crate::providers::infer_provider_id(model);

    if lower.contains("model") && (lower.contains("not found") || lower.contains("pull")) {
        return DelegateChildFailureKind::MissingLocalModel;
    }
    if lower.contains("api key")
        || lower.contains("missing credential")
        || lower.contains("auth")
        || lower.contains("oauth")
        || lower.contains("not logged in")
        || lower.contains("secrets preflight") && lower.contains("missing")
    {
        return DelegateChildFailureKind::MissingCredential;
    }
    if lower.contains("repo model")
        || lower.contains("workspace")
        || lower.contains("branch=")
        || lower.contains("cwd does not exist")
    {
        return DelegateChildFailureKind::WorkspaceStartup;
    }
    if matches!(
        provider.as_str(),
        "anthropic"
            | "openai"
            | "openai-codex"
            | "openrouter"
            | "groq"
            | "xai"
            | "mistral"
            | "cerebras"
            | "huggingface"
            | "ollama"
            | "ollama-cloud"
    ) {
        return DelegateChildFailureKind::ProviderStartup;
    }
    DelegateChildFailureKind::Unknown
}

fn format_delegate_child_failure(
    status_code: Option<i32>,
    stderr: &str,
    model: &str,
    runtime: &DelegateRuntimeRequest,
    prompt_path: &std::path::Path,
) -> String {
    let provider = crate::providers::infer_provider_id(model);
    let failure_kind = classify_delegate_child_failure(stderr, model);
    let summary = match failure_kind {
        DelegateChildFailureKind::MissingLocalModel => {
            "Delegate worker could not start because the selected local model is unavailable."
        }
        DelegateChildFailureKind::MissingCredential => {
            "Delegate worker could not start because the selected upstream provider is missing credentials or login state."
        }
        DelegateChildFailureKind::ProviderStartup => {
            "Delegate worker failed during provider/runtime startup."
        }
        DelegateChildFailureKind::WorkspaceStartup => {
            "Delegate worker failed before inference while initializing workspace/repo state."
        }
        DelegateChildFailureKind::Unknown => "Delegate worker exited before completing the task.",
    };

    let mut out = String::new();
    out.push_str(summary);
    out.push_str("\n\n");
    out.push_str("Delegate child context\n");
    out.push_str(&format!("  provider: {provider}\n"));
    out.push_str(&format!("  model: {model}\n"));
    out.push_str(&format!(
        "  worker_profile: {}\n",
        runtime.worker_profile.as_str()
    ));
    out.push_str(&format!(
        "  thinking: {}\n",
        runtime.thinking_level.as_deref().unwrap_or("minimal")
    ));
    out.push_str("  context_class: compact\n");
    out.push_str(&format!(
        "  scope: {}\n",
        runtime
            .scope
            .as_ref()
            .map(|s| if s.is_empty() {
                "(none)".to_string()
            } else {
                s.join(", ")
            })
            .unwrap_or_else(|| "(none)".to_string())
    ));
    out.push_str(&format!("  prompt_file: {}\n", prompt_path.display()));
    out.push_str(&format!(
        "  exit_code: {}\n",
        status_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string())
    ));

    match failure_kind {
        DelegateChildFailureKind::MissingLocalModel => {
            out.push_str("\nSuggested next step\n");
            out.push_str("  Choose a non-local delegate model or install the required Ollama model before retrying.\n");
        }
        DelegateChildFailureKind::MissingCredential => {
            out.push_str("\nSuggested next step\n");
            out.push_str("  Log in or configure credentials for the selected provider, or pass an explicit model/provider for delegate.\n");
        }
        DelegateChildFailureKind::ProviderStartup => {
            out.push_str("\nSuggested next step\n");
            out.push_str("  Retry with an explicit model/provider, then inspect provider auth and startup logs if it reproduces.\n");
        }
        DelegateChildFailureKind::WorkspaceStartup => {
            out.push_str("\nSuggested next step\n");
            out.push_str("  Check the child cwd/repo state and whether the delegated scope references files available in this workspace.\n");
        }
        DelegateChildFailureKind::Unknown => {}
    }

    if !stderr.trim().is_empty() {
        out.push_str("\nChild stderr\n");
        out.push_str(stderr.trim());
    }

    out
}

impl DelegateRunner {
    pub fn new(cwd: PathBuf, result_store: Arc<DelegateResultStore>, sandbox: bool) -> Self {
        Self::new_with_safety(
            cwd,
            result_store,
            sandbox,
            std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok(),
        )
    }

    pub fn new_with_safety(
        cwd: PathBuf,
        result_store: Arc<DelegateResultStore>,
        sandbox: bool,
        dangerously_bypass_permissions: bool,
    ) -> Self {
        Self {
            cwd,
            result_store,
            sandbox,
            child_agent_binary: None,
            wall_timeout_secs: 300,
            idle_timeout_secs: 120,
            dangerously_bypass_permissions,
        }
    }

    #[cfg(test)]
    fn with_child_agent_binary(mut self, child_agent_binary: PathBuf) -> Self {
        self.child_agent_binary = Some(child_agent_binary);
        self
    }

    #[cfg(test)]
    fn with_timeouts(mut self, wall_timeout_secs: u64, idle_timeout_secs: u64) -> Self {
        self.wall_timeout_secs = wall_timeout_secs;
        self.idle_timeout_secs = idle_timeout_secs;
        self
    }

    fn build_delegate_prompt(
        &self,
        worker_profile: DelegateWorkerProfile,
        task: &str,
        scope: Option<&[String]>,
        facts: Option<&[String]>,
        field_kit_context: &str,
    ) -> String {
        let mut prompt = String::from(worker_profile.prompt_preamble());
        prompt.push_str("\n\n");

        // Inject workspace goal context if bindings are set.
        match crate::workspace::runtime::read_workspace_lease(&self.cwd) {
            Err(e) => {
                tracing::warn!("failed to read workspace lease for delegate prompt: {e}");
            }
            Ok(Some(lease)) => {
                let b = &lease.bindings;
                let has_bindings = b.milestone_id.is_some()
                    || b.design_node_id.is_some()
                    || b.openspec_change.is_some();
                if has_bindings {
                    prompt.push_str("## Workspace Goal Context\n");
                    if let Some(ref m) = b.milestone_id {
                        prompt.push_str(&format!("- Milestone: {m}\n"));
                    }
                    if let Some(ref d) = b.design_node_id {
                        prompt.push_str(&format!("- Design node: {d}\n"));
                    }
                    if let Some(ref o) = b.openspec_change {
                        prompt.push_str(&format!("- OpenSpec change: {o}\n"));
                    }
                    prompt.push('\n');
                }
            }
            Ok(None) => {}
        }

        prompt.push_str("## Task\n");
        prompt.push_str(task);
        prompt.push('\n');
        if let Some(scope) = scope
            && !scope.is_empty()
        {
            prompt.push_str("\n## Scope\n");
            for entry in scope {
                prompt.push_str(&format!("- {entry}\n"));
            }
            prompt.push_str("Only touch files within this declared scope unless blocked.\n");
        }
        if let Some(facts) = facts
            && !facts.is_empty()
        {
            prompt.push_str("\n## Facts\n");
            for fact in facts {
                prompt.push_str(&format!("- {fact}\n"));
            }
        }
        if !field_kit_context.is_empty() {
            prompt.push_str(field_kit_context);
            prompt.push('\n');
        }
        let runtime = worker_profile.runtime_profile(scope, None, None);
        let boundary = ChildAgentBoundary::from_runtime_with_safety(
            &self.cwd,
            &runtime,
            self.dangerously_bypass_permissions,
        );
        prompt.push('\n');
        prompt.push_str(&boundary.to_prompt_section());
        prompt.push_str(
            "\n## Constraints\n- Stay within the declared scope.\n- Do not broaden into design/lifecycle/spec work.\n- Do not delegate further.\n- Prefer concise output over process narration.\n- Return a single final answer that ends the task cleanly; do not leave a trailing assistant prefill stub.\n",
        );
        prompt.push_str(
            "\n## Output contract\nReturn the concrete result of the delegated task. \
If you edited files, say which ones. If you validated, say what ran. \
If blocked, say the blocker plainly.\n",
        );
        prompt
    }

    fn delegate_prompt_path(task_id: &str) -> anyhow::Result<String> {
        child_prompt_relative_path(ChildPromptKind::Delegate, task_id)
    }

    async fn run_delegate_child(
        &self,
        task_id: &str,
        prompt: &str,
        runtime: &DelegateRuntimeRequest,
        mind: Option<&str>,
        session_model: Option<String>,
    ) -> anyhow::Result<String> {
        let prompt_file = Self::delegate_prompt_path(task_id)?;
        let prompt_path = write_child_prompt_file(&self.cwd, &prompt_file, prompt)?;

        let child_runtime = runtime.worker_profile.runtime_profile(
            runtime.scope.as_deref(),
            runtime.thinking_level.as_deref(),
            mind,
        );
        let model = match runtime
            .model
            .clone()
            .or_else(|| child_runtime.model.clone())
        {
            Some(model) => model,
            None => {
                // Inherit the parent session's model so children use the same
                // provider the operator is actually running on.
                match session_model {
                    Some(m) => m,
                    None => crate::providers::delegate_default_model().await,
                }
            }
        };
        let child_config = ChildAgentSpawnConfig {
            agent_binary: self
                .child_agent_binary
                .clone()
                .map(Ok)
                .unwrap_or_else(std::env::current_exe)
                .context("delegate runner could not locate current executable")?,
            model: model.clone(),
            max_turns: runtime.worker_profile.max_turns(),
            inherited_env: Vec::new(),
            injected_env: Vec::new(),
            runtime: child_runtime,
            dangerously_bypass_permissions: self.dangerously_bypass_permissions,
        };
        let (mut child, _pid) = if self.sandbox {
            match spawn_sandboxed_child_agent(
                &child_config,
                &self.cwd,
                &prompt_path,
                Some("coding"),
            ) {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!(error = %e, "delegate sandbox spawn failed, falling back to subprocess");
                    spawn_headless_child_agent(&child_config, &self.cwd, &prompt_path)?
                }
            }
        } else {
            spawn_headless_child_agent(&child_config, &self.cwd, &prompt_path)?
        };

        // Stream stderr for live activity tracking.
        // Timeouts: delegate workers are short-lived (4–6 turns), so use
        // tighter bounds than cleave's 900s/180s defaults.
        use tokio::io::AsyncBufReadExt;
        let stderr = child.stderr.take().unwrap();
        let mut reader = tokio::io::BufReader::new(stderr).lines();
        let mut stderr_tail: Vec<String> = Vec::new();
        let store = self.result_store.clone();
        let tid = task_id.to_string();

        let wall_timeout = tokio::time::Duration::from_secs(self.wall_timeout_secs);
        let idle_timeout = tokio::time::Duration::from_secs(self.idle_timeout_secs);
        let mut last_activity = std::time::Instant::now();
        let mut last_activity_event = std::time::Instant::now() - std::time::Duration::from_secs(2); // ensure first event fires

        let io_result: Result<(), anyhow::Error> = tokio::select! {
            _ = tokio::time::sleep(wall_timeout) => {
                tracing::warn!(task_id, "delegate wall-clock timeout");
                Err(anyhow::anyhow!("Delegate wall-clock timeout after {}s", self.wall_timeout_secs))
            }
            result = async {
                loop {
                    match tokio::time::timeout(idle_timeout, reader.next_line()).await {
                        Ok(Ok(Some(line))) => {
                            last_activity = std::time::Instant::now();
                            // Keep last 30 lines for error reporting
                            if stderr_tail.len() >= 30 {
                                stderr_tail.remove(0);
                            }
                            stderr_tail.push(line.clone());

                            // Throttle: parse at most once per second
                            if last_activity.duration_since(last_activity_event).as_secs() >= 1
                                && let Some(activity) = parse_child_activity(&line)
                            {
                                last_activity_event = std::time::Instant::now();
                                match activity {
                                    ChildAgentActivity::Tool { tool, target } => {
                                        store.update_task_live_state(&tid, Some(tool), target, None);
                                    }
                                    ChildAgentActivity::Turn { turn } => {
                                        store.update_task_live_state(&tid, None, None, Some(turn));
                                    }
                                    ChildAgentActivity::TaskDone { task_index } => {
                                        store.mark_task_done(&tid, task_index);
                                    }
                                    ChildAgentActivity::Tokens { .. } => {
                                        // Delegate progress does not currently surface child token usage.
                                    }
                                }
                            }
                        }
                        Ok(Ok(None)) => break, // EOF
                        Ok(Err(e)) => {
                            tracing::warn!(task_id, "delegate stderr read error: {e}");
                            break;
                        }
                        Err(_) => {
                            tracing::warn!(task_id, idle_secs = last_activity.elapsed().as_secs(), "delegate idle timeout");
                            return Err(anyhow::anyhow!("Delegate idle timeout — no output for {}s", self.idle_timeout_secs));
                        }
                    }
                }
                Ok(())
            } => { result }
        };

        // Terminate child if still running (timeout path)
        if io_result.is_err() {
            let _ = child.kill().await;
        }

        // Drain stdout concurrently before wait() to prevent deadlock.
        // If the child writes more than the OS pipe buffer (16KB on macOS,
        // 64KB on Linux), it blocks waiting for the parent to read. If
        // the parent is waiting for the child to exit, that's a deadlock.
        let stdout_handle = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt;
                let mut buf = String::new();
                let _ = stdout.read_to_string(&mut buf).await;
                buf
            })
        });

        let status = child
            .wait()
            .await
            .context("delegate child process failed to execute")?;

        let stdout_buf = match stdout_handle {
            Some(handle) => handle.await.unwrap_or_default(),
            None => String::new(),
        };

        // Timeout errors take priority over exit status
        if let Err(timeout_err) = io_result {
            let stderr_text = stderr_tail.join("\n");
            return Err(anyhow::anyhow!(
                "{timeout_err}\n\n--- last stderr ---\n{stderr_text}"
            ));
        }

        if status.success() {
            let stdout = stdout_buf.trim().to_string();
            if stdout.is_empty() {
                Err(anyhow::anyhow!(
                    "Delegate completed without output; no assessment was produced. Treat this as a degraded delegate result, not approval."
                ))
            } else {
                Ok(stdout)
            }
        } else {
            let stderr_text = stderr_tail.join("\n");
            Err(anyhow::anyhow!(format_delegate_child_failure(
                status.code(),
                &stderr_text,
                &model,
                runtime,
                &prompt_path,
            )))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_delegate(
        &self,
        task_id: String,
        agent_name: Option<String>,
        task: String,
        scope: Option<Vec<String>>,
        model: Option<String>,
        thinking_level: Option<String>,
        worker_profile: DelegateWorkerProfile,
        facts: Option<Vec<String>>,
        mind: Option<String>,
        session_model: Option<String>,
        consecutive_failures: Arc<Mutex<u32>>,
        event_sink: Option<BusRequestSink>,
    ) -> anyhow::Result<()> {
        // Assemble field kit: load persona mind if specified
        let mut field_kit_context = String::new();
        if let Some(ref persona_id) = mind {
            // Try to find the persona in installed plugins and load its directive + facts
            let (personas, _) = crate::plugins::persona_loader::scan_available();
            if let Some(available) = personas.iter().find(|p| {
                p.id.contains(persona_id)
                    || p.name.to_lowercase().contains(&persona_id.to_lowercase())
            }) && let Ok(persona) = crate::plugins::persona_loader::load_persona(&available.path)
            {
                field_kit_context.push_str(&format!(
                    "\n## Persona: {}\n{}\n",
                    persona.name, persona.directive
                ));
                if !persona.mind_facts.is_empty() {
                    field_kit_context.push_str(&format!(
                        "\n## Mind Facts ({} facts)\n",
                        persona.mind_facts.len()
                    ));
                    for fact in &persona.mind_facts {
                        field_kit_context
                            .push_str(&format!("- [{}] {}\n", fact.section, fact.content));
                    }
                }
            }
        }
        if let Some(ref fact_list) = facts
            && !fact_list.is_empty()
        {
            field_kit_context.push_str("\n## Injected Facts\n");
            for f in fact_list {
                field_kit_context.push_str(&format!("- {f}\n"));
            }
        }

        let tasks = extract_task_items(&task);
        let task_entry = DelegateTask {
            task_id: task_id.clone(),
            agent_name,
            task_description: task.clone(),
            status: DelegateTaskStatus::Running,
            result: None,
            result_viewed: false,
            started_at: SystemTime::now(),
            completed_at: None,
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks,
        };

        self.result_store.store_task(task_entry);

        let runtime = DelegateRuntimeRequest {
            scope: scope.clone(),
            model,
            thinking_level,
            worker_profile,
        };
        let prompt = self.build_delegate_prompt(
            worker_profile,
            &task,
            scope.as_deref(),
            facts.as_deref(),
            &field_kit_context,
        );

        let store = self.result_store.clone();
        let cwd = self.cwd.clone();
        let parent_model = session_model;
        let fail_counter = consecutive_failures;
        let sandbox = self.sandbox;
        crate::task_spawn::spawn_best_effort_result("delegate-real-task", async move {
            let runner = DelegateRunner::new(cwd, store.clone(), sandbox);
            match runner
                .run_delegate_child(&task_id, &prompt, &runtime, mind.as_deref(), parent_model)
                .await
            {
                Ok(result) => {
                    store.update_task_status(
                        &task_id,
                        DelegateTaskStatus::Completed { success: true },
                        Some(result.clone()),
                    );
                    if let Some(sink) = &event_sink {
                        sink.send(BusRequest::EmitAgentEvent {
                            event: Box::new(AgentEvent::SystemNotification {
                                message: format!(
                                    "✓ {task_id} completed — result ready: /delegate result {task_id}"
                                ),
                            }),
                        });
                    }
                    if let Ok(mut count) = fail_counter.lock() {
                        *count = 0;
                    }
                }
                Err(err) => {
                    // Only increment on actual failure, not pre-spawn.
                    let error = err.to_string();
                    if let Ok(mut count) = fail_counter.lock() {
                        *count += 1;
                    }
                    store.update_task_status(
                        &task_id,
                        DelegateTaskStatus::Failed {
                            error: error.clone(),
                            kind: DelegateChildFailureKind::Unknown,
                        },
                        None,
                    );
                    if let Some(sink) = &event_sink {
                        let first_line = error.lines().next().unwrap_or("delegate failed");
                        sink.send(BusRequest::EmitAgentEvent {
                            event: Box::new(AgentEvent::SystemNotification {
                                message: format!(
                                    "✗ {task_id} failed — {first_line}: /delegate result {task_id}"
                                ),
                            }),
                        });
                    }
                }
            }
            Ok(())
        });

        Ok(())
    }

    pub async fn wait_for_result(
        &self,
        task_id: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<String> {
        // Poll for up to 300s (matching the child wall-clock timeout).
        // Previous 30s limit caused premature "Task timed out" for any
        // delegate doing real work (patch workers routinely take 45-120s).
        for _ in 0..600 {
            if cancel.is_cancelled() {
                anyhow::bail!("Delegate task cancelled")
            }
            if let Some(task) = self.result_store.get_task(task_id) {
                match task.status {
                    DelegateTaskStatus::Completed { success: true } => {
                        return Ok(task.result.unwrap_or_else(|| "Task completed".to_string()));
                    }
                    DelegateTaskStatus::Completed { success: false } => {
                        return Err(anyhow::anyhow!("Task failed"));
                    }
                    DelegateTaskStatus::Failed { error, .. } => {
                        return Err(anyhow::anyhow!("Task failed: {}", error));
                    }
                    DelegateTaskStatus::Cancelled { reason } => {
                        return Err(anyhow::anyhow!(
                            "Task cancelled{}",
                            reason
                                .as_deref()
                                .map(|r| format!(": {r}"))
                                .unwrap_or_default()
                        ));
                    }
                    DelegateTaskStatus::Running => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            } else {
                return Err(anyhow::anyhow!("Task not found"));
            }
        }
        Err(anyhow::anyhow!("Task timed out"))
    }
}

/// The main delegate feature
pub struct DelegateFeature {
    result_store: Arc<DelegateResultStore>,
    available_agents: Vec<AgentSpec>,
    runner: Arc<DelegateRunner>,
    progress_handle: Arc<Mutex<DelegateProgress>>,
    event_slot: DelegateEventSlot,
    /// Parent session model, updated on each TurnEnd. Used as the default
    /// for child delegates so they inherit the operator's active provider
    /// instead of falling back to a hardcoded candidate list.
    session_model: Arc<Mutex<Option<String>>>,
    /// Consecutive delegate failure counter. After 3 consecutive failures,
    /// the delegate tool is hard-disabled for the rest of the session to
    /// prevent infinite retry loops.
    consecutive_failures: Arc<Mutex<u32>>,
    /// Provider inventory for surfacing available models in context injection.
    /// When set, the delegation model catalog is injected into the system prompt
    /// so the orchestrator can route delegate tasks to appropriate models.
    provider_inventory:
        Option<std::sync::Arc<tokio::sync::RwLock<crate::routing::ProviderInventory>>>,
    /// Recent user prompts — bounded window for echo detection.
    /// Capped at 50 entries to prevent unbounded growth in long sessions.
    recent_user_prompts: std::collections::VecDeque<String>,
    /// Recent files the parent agent has read/edited — bounded LRU for
    /// auto-populating delegate scope. Capped at 30 entries.
    recent_parent_files: std::collections::VecDeque<String>,
    /// Live runtime settings used to resolve the selected subagent autonomy policy.
    settings: Option<crate::settings::SharedSettings>,
    sandbox: bool,
}

impl DelegateFeature {
    pub fn new(cwd: &Path, agents: Vec<AgentSpec>, sandbox: bool) -> Self {
        Self::new_with_safety(
            cwd,
            agents,
            sandbox,
            std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok(),
        )
    }

    pub fn new_with_safety(
        cwd: &Path,
        agents: Vec<AgentSpec>,
        sandbox: bool,
        dangerously_bypass_permissions: bool,
    ) -> Self {
        let result_store = Arc::new(DelegateResultStore::new());
        let runner = Arc::new(DelegateRunner::new_with_safety(
            cwd.to_path_buf(),
            result_store.clone(),
            sandbox,
            dangerously_bypass_permissions,
        ));

        // Seed session model from env so delegates on the first turn
        // (before any TurnEnd fires) still inherit the operator's model.
        let initial_model = std::env::var("OMEGON_MODEL").ok().filter(|s| !s.is_empty());

        let progress_handle = Arc::new(Mutex::new(DelegateProgress::default()));
        let event_slot = Arc::new(Mutex::new(None));
        Self {
            result_store,
            available_agents: agents,
            runner,
            progress_handle,
            event_slot,
            session_model: Arc::new(Mutex::new(initial_model)),
            consecutive_failures: Arc::new(Mutex::new(0)),
            provider_inventory: None,
            recent_user_prompts: std::collections::VecDeque::with_capacity(50),
            recent_parent_files: std::collections::VecDeque::with_capacity(30),
            settings: None,
            sandbox,
        }
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

    /// Return the most recent substantive user prompt (>10 chars) that
    /// differs from `exclude`. Used to find a real task when the model
    /// echoed a user prompt as the delegate task.
    fn latest_substantive_prompt_excluding(&self, exclude: &str) -> Option<&str> {
        let exclude_lower = exclude.trim().to_ascii_lowercase();
        self.recent_user_prompts
            .iter()
            .rev()
            .find(|p| p.len() > 10 && p.trim().to_ascii_lowercase() != exclude_lower)
            .map(|s| s.as_str())
    }

    /// Check if a delegate task is echoing ANY user prompt — current or past.
    /// The model should never pass the user's exact words as a delegate task.
    /// It should synthesize a concrete, actionable instruction from context.
    fn is_user_prompt_echo(&self, task: &str) -> bool {
        let lower = task.trim().to_ascii_lowercase();
        if lower.is_empty() || self.recent_user_prompts.is_empty() {
            return false;
        }
        for prompt in &self.recent_user_prompts {
            let prompt_lower = prompt.trim().to_ascii_lowercase();
            if prompt_lower.is_empty() {
                continue;
            }
            // Exact match
            if prompt_lower == lower {
                return true;
            }
            // Delegate task contains the older prompt (model wrapped it)
            if prompt_lower.len() > 15 && lower.contains(&prompt_lower) {
                return true;
            }
            // Older prompt contains the delegate task (model truncated it)
            if lower.len() > 15 && prompt_lower.contains(&lower) {
                return true;
            }
            // Significant prefix overlap (model appended to it)
            let min_len = prompt_lower.len().min(lower.len());
            if min_len > 20 {
                let prefix_len = prompt_lower
                    .chars()
                    .zip(lower.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                if prefix_len > min_len * 2 / 3 {
                    return true;
                }
            }
        }
        false
    }

    /// Attach a provider inventory for model catalog context injection.
    pub fn with_inventory(
        mut self,
        inventory: std::sync::Arc<tokio::sync::RwLock<crate::routing::ProviderInventory>>,
    ) -> Self {
        self.provider_inventory = Some(inventory);
        self
    }
}

impl DelegateFeature {
    pub fn progress_handle(&self) -> Arc<Mutex<DelegateProgress>> {
        self.progress_handle.clone()
    }

    pub fn event_sender_slot(&self) -> DelegateEventSlot {
        self.event_slot.clone()
    }

    fn emit_delegate_event(&self, event: AgentEvent) {
        if let Ok(slot) = self.event_slot.lock()
            && let Some(ref sink) = *slot
        {
            sink.send(BusRequest::EmitAgentEvent {
                event: Box::new(event),
            });
        }
    }

    fn emit_delegate_family_vitals(&self) {
        let progress = self.result_store.progress_snapshot();
        let signs = omegon_traits::FamilyVitalSigns {
            run_id: "delegate".into(),
            active: progress.active,
            total_children: progress.children.len(),
            completed: progress.completed,
            failed: progress.failed,
            running: progress.running,
            pending: progress.pending_results,
            total_tokens_in: 0,
            total_tokens_out: 0,
            children: progress
                .children
                .iter()
                .map(|child| omegon_traits::ChildVitalSigns {
                    label: child.label.clone(),
                    status: child.status.clone(),
                    started_at_unix_ms: child
                        .started_at
                        .and_then(|ts| ts.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64),
                    last_activity_unix_ms: child
                        .completed_at
                        .or(child.started_at)
                        .and_then(|ts| ts.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64),
                    duration_secs: child
                        .started_at
                        .and_then(|ts| ts.elapsed().ok())
                        .map(|d| d.as_secs_f64()),
                    last_tool: child
                        .last_tool
                        .clone()
                        .or_else(|| child.result_summary.clone()),
                    last_tool_activity: child.last_tool_activity.as_ref().map(|activity| {
                        omegon_traits::ToolActivityVitalSigns {
                            raw_name: activity.raw_name.clone(),
                            args_summary: activity.args_summary.clone(),
                        }
                    }),
                    last_turn: child.last_turn,
                    tokens_in: 0,
                    tokens_out: 0,
                    tasks: child
                        .tasks
                        .iter()
                        .map(|t| omegon_traits::VitalSignsTaskItem {
                            description: t.description.clone(),
                            done: t.done,
                        })
                        .collect(),
                    tasks_done: child.tasks_done,
                })
                .collect(),
        };
        self.emit_delegate_event(AgentEvent::FamilyVitalSignsUpdated { signs });
    }
}

#[async_trait]
impl Feature for DelegateFeature {
    fn name(&self) -> &str {
        "delegate"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::delegate::DELEGATE.to_string(),
                label: "Delegate Subagent Task".to_string(),
                description: "Spawn a subagent/delegate to handle a specific task. Omit `model` for the safest same-provider default; only set `model` to route to a known-good local or cheaper model after reliability is established. Worker profiles: scout (read/search only), patch (small scoped edits), verify (run tests/checks).".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "A specific, actionable instruction for the delegate. Must describe WHAT to do, WHERE (files/paths), and the expected OUTCOME. NEVER echo the user's messages as the task — especially not early test messages like 'testing' or 'hello'. Formulate the concrete task yourself from the current conversation context and your most recent analysis."
                        },
                        "agent": { "type": "string" },
                        "scope": { "type": "array", "items": {"type": "string"} },
                        "model": { "type": "string", "description": "Model to use (e.g., `ollama:qwen3:32b`). Omit to inherit session model. Prefer local models for rote tasks." },
                        "worker_profile": {
                            "type": "string",
                            "enum": ["scout", "patch", "verify"],
                            "description": "Micro-worker profile. scout=read/search, patch=small scoped edit, verify=bounded validation",
                            "default": "scout"
                        },
                        "facts": { "type": "array", "items": {"type": "string"} },
                        "mind": { "type": "string" },
                        "background": { "type": "boolean", "default": true }
                    },
                    "required": ["task"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::StateChanging,
                    omegon_traits::ToolCapability::ProgressBoundary,
                ],
            },
            ToolDefinition {
                name: crate::tool_registry::delegate::DELEGATE_RESULT.to_string(),
                label: "Get Delegate Result".to_string(),
                description: "Retrieve result from a background delegate task".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "task_id": {
                            "type": "string",
                            "description": "The task ID to retrieve results for"
                        }
                    },
                    "required": ["task_id"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::delegate::DELEGATE_STATUS.to_string(),
                label: "Delegate Status".to_string(),
                description: "List all delegate tasks and their status".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::delegate::DELEGATE_CANCEL.to_string(),
                label: "Cancel Delegate Task".to_string(),
                description: "Mark a running delegate task as cancelled. This records terminal non-failure state; process termination is best-effort and may already have completed.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "task_id": { "type": "string", "description": "The delegate task ID to cancel" },
                        "reason": { "type": "string", "description": "Optional cancellation reason" }
                    },
                    "required": ["task_id"]
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
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::delegate::DELEGATE => {
                // Hard-stop after 3 consecutive delegate failures to prevent
                // infinite retry loops.
                let failures = self.consecutive_failures.lock().map(|g| *g).unwrap_or(0);
                if failures >= 3 {
                    return Err(anyhow::anyhow!(
                        "Delegate is disabled for this session after {} consecutive \
                         failures. The delegated tasks are failing repeatedly — \
                         consider handling the work directly instead of delegating.",
                        failures
                    ));
                }

                let task: String = args
                    .get("task")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("task parameter is required"))?
                    .to_string();

                let agent = args
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let scope = args.get("scope").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                });
                let model = args
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let thinking_level = args
                    .get("thinking_level")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let facts = args.get("facts").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                });
                let mind = args
                    .get("mind")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let worker_profile = DelegateWorkerProfile::parse(
                    args.get("worker_profile").and_then(|v| v.as_str()),
                );
                let background = args
                    .get("background")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                // Reject tasks that are just system error messages — the LLM
                // is regurgitating stuck-detector warnings as delegate tasks.
                if task.starts_with("[System:")
                    || task.contains("STUCK LOOP DETECTED")
                    || task.contains("last several delegate calls returned errors")
                    || task.contains("last several `delegate` calls returned errors")
                {
                    return Err(anyhow::anyhow!(
                        "Cannot delegate: the task you provided is a recycled system \
                         warning, not a real task. Stop delegating and handle the \
                         work directly."
                    ));
                }

                // Detect when the model parrots a user message or asks a
                // question instead of formulating an actionable task.
                tracing::debug!(
                    task = %task,
                    prompts = self.recent_user_prompts.len(),
                    "Delegate task check — echo={}, non_task={}",
                    self.is_user_prompt_echo(&task),
                    is_conversational_non_task(&task),
                );
                let task = if self.is_user_prompt_echo(&task) {
                    if let Some(recent) = self.latest_substantive_prompt_excluding(&task) {
                        tracing::info!(
                            original = %task,
                            substituted = %recent,
                            "Substituted user echo with latest substantive prompt"
                        );
                        recent.to_string()
                    } else if let Some(prior) = self.result_store.last_task_description() {
                        tracing::info!(
                            original = %task,
                            substituted = %prior,
                            "Substituted user echo with last delegate task"
                        );
                        prior
                    } else {
                        // Echo detected but no substitute available — the model
                        // is parroting the user's first/only prompt. Reject so
                        // it handles the work directly instead of delegating.
                        return Err(anyhow::anyhow!(
                            "Cannot delegate the user's own prompt as a task. \
                             Formulate a specific, actionable instruction for the \
                             delegate based on your analysis of what needs to be done."
                        ));
                    }
                } else if is_conversational_non_task(&task) {
                    // Short conversational phrase with no actionable content.
                    // Try to substitute; if nothing available, pass through.
                    if let Some(prior) = self.result_store.last_task_description() {
                        tracing::info!(
                            original = %task,
                            substituted = %prior,
                            "Substituted conversational delegate task with last task description"
                        );
                        prior
                    } else if let Some(recent) = self.latest_substantive_prompt_excluding(&task) {
                        tracing::info!(
                            original = %task,
                            substituted = %recent,
                            "Substituted conversational delegate task with latest user prompt"
                        );
                        recent.to_string()
                    } else {
                        task
                    }
                } else {
                    task
                };

                // Validate agent if specified. Catch common tool/agent namespace confusion
                // before reporting a generic unknown-agent error.
                if let Some(ref agent_name) = agent
                    && !self.available_agents.iter().any(|a| a.name == *agent_name)
                {
                    if let Some(guidance) = delegate_tool_name_guidance(agent_name) {
                        return Err(anyhow::anyhow!(guidance));
                    }
                    return Err(anyhow::anyhow!(
                        "Unknown delegate agent: {agent_name}. Use the delegate_result/delegate_status tools directly for result retrieval/status; the delegate agent field only accepts configured agent names."
                    ));
                }

                // Dedup: block if an identical task is already running, completed,
                // or failed. Prevents infinite retry loops when tasks time out.
                if let Some((prior_id, status)) = self.result_store.find_any_by_description(&task) {
                    let message = match status {
                        "running" => format!(
                            "A delegate with this exact task is already running ({prior_id}). \
                             Do NOT spawn duplicates. Wait for it to complete or formulate \
                             a DIFFERENT task."
                        ),
                        "completed" => {
                            let result_text = self
                                .result_store
                                .get_task(&prior_id)
                                .and_then(|t| t.result.clone())
                                .unwrap_or_else(|| "completed".into());
                            format!(
                                "This task was already completed ({prior_id}). Do NOT \
                                 re-delegate the same task. If the user gave a new \
                                 instruction, formulate a NEW task description that \
                                 reflects what they asked for NOW.\n\n\
                                 Previous result:\n{result_text}"
                            )
                        }
                        _ => format!(
                            "A previous delegate with this exact task already failed \
                             ({prior_id}). Re-delegating the same task will fail again. \
                             Either handle the work directly or formulate a different, \
                             more specific task."
                        ),
                    };
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: message }],
                        details: serde_json::json!(null),
                    });
                }

                // Auto-populate scope from parent's recently read files when the
                // model doesn't provide one. Without scope, the child starts
                // blind and gets blocked by its own harness for having no target.
                let scope = if scope.is_none() && !self.recent_parent_files.is_empty() {
                    let recent: Vec<String> = self
                        .recent_parent_files
                        .iter()
                        .rev()
                        .take(10)
                        .cloned()
                        .collect();
                    tracing::info!(
                        files = ?recent,
                        "Auto-populated delegate scope from parent's recently read files"
                    );
                    Some(recent)
                } else {
                    scope
                };

                let policy = self.subagent_policy();
                if let Some(result) =
                    enforce_delegate_policy_with_policy(&policy, worker_profile, &task)
                {
                    return Ok(result);
                }

                let task_id = self.result_store.generate_task_id();

                // Spawn the delegate
                let parent_model = self.session_model.lock().ok().and_then(|s| s.clone());
                self.runner.spawn_delegate(
                    task_id.clone(),
                    agent,
                    task,
                    scope,
                    model,
                    thinking_level,
                    worker_profile,
                    facts,
                    mind,
                    parent_model,
                    self.consecutive_failures.clone(),
                    self.event_slot.lock().ok().and_then(|slot| (*slot).clone()),
                )?;
                if let Ok(mut handle) = self.progress_handle.lock() {
                    *handle = self.result_store.progress_snapshot();
                }
                self.emit_delegate_event(AgentEvent::DecompositionStarted {
                    children: vec![task_id.clone()],
                    operation: OperationRef::delegate(task_id.clone()),
                });
                self.emit_delegate_family_vitals();

                if background {
                    // Return task ID for background execution
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format_background_delegate_started(&task_id),
                        }],
                        details: json!({ "task_id": task_id, "background": true }),
                    })
                } else {
                    // Wait for completion and return result
                    let result = self.runner.wait_for_result(&task_id, cancel).await?;
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: result }],
                        details: json!({ "task_id": task_id, "background": false }),
                    })
                }
            }

            crate::tool_registry::delegate::DELEGATE_RESULT => {
                let task_id: String = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("task_id parameter is required"))?
                    .to_string();

                match self.result_store.get_task(&task_id) {
                    Some(task) => match task.status {
                        DelegateTaskStatus::Running => Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: "Task still running".to_string(),
                            }],
                            details: json!({ "status": "running", "task_id": task_id }),
                        }),
                        DelegateTaskStatus::Completed { success: true } => {
                            self.result_store.mark_result_viewed(&task_id);
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: task
                                        .result
                                        .unwrap_or_else(|| "Task completed".to_string()),
                                }],
                                details: json!({ "status": "completed", "success": true, "task_id": task_id, "result_viewed": true }),
                            })
                        }
                        DelegateTaskStatus::Completed { success: false } => {
                            self.result_store.mark_result_viewed(&task_id);
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: "Task completed with failure".to_string(),
                                }],
                                details: json!({ "status": "completed", "success": false, "task_id": task_id, "result_viewed": true }),
                            })
                        }
                        DelegateTaskStatus::Failed { error, .. } => {
                            self.result_store.mark_result_viewed(&task_id);
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: format!("Task failed: {}", error),
                                }],
                                details: json!({ "status": "failed", "error": error, "task_id": task_id, "result_viewed": true }),
                            })
                        }
                        DelegateTaskStatus::Cancelled { reason } => {
                            self.result_store.mark_result_viewed(&task_id);
                            Ok(ToolResult {
                                content: vec![ContentBlock::Text {
                                    text: reason
                                        .as_ref()
                                        .map(|reason| format!("Task cancelled: {reason}"))
                                        .unwrap_or_else(|| "Task cancelled".to_string()),
                                }],
                                details: json!({ "status": "cancelled", "reason": reason, "task_id": task_id, "result_viewed": true }),
                            })
                        }
                    },
                    None => Err(anyhow::anyhow!("Task not found: {}", task_id)),
                }
            }

            crate::tool_registry::delegate::DELEGATE_CANCEL => {
                let task_id: String = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("task_id parameter is required"))?
                    .to_string();
                let reason = args
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let status = self.result_store.cancel_task(&task_id, reason.clone())?;
                if let Ok(mut handle) = self.progress_handle.lock() {
                    *handle = self.result_store.progress_snapshot();
                }
                let text = match status {
                    DelegateTaskStatus::Cancelled { ref reason } => reason
                        .as_ref()
                        .map(|reason| format!("Delegate task {task_id} cancelled: {reason}"))
                        .unwrap_or_else(|| format!("Delegate task {task_id} cancelled")),
                    DelegateTaskStatus::Running => {
                        format!("Delegate task {task_id} is still running")
                    }
                    DelegateTaskStatus::Completed { success: true } => {
                        format!("Delegate task {task_id} already completed")
                    }
                    DelegateTaskStatus::Completed { success: false } => {
                        format!("Delegate task {task_id} already completed with failure")
                    }
                    DelegateTaskStatus::Failed { ref error, .. } => {
                        format!("Delegate task {task_id} already failed: {error}")
                    }
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details: json!({
                        "task_id": task_id,
                        "status": match status {
                            DelegateTaskStatus::Cancelled { .. } => "cancelled",
                            DelegateTaskStatus::Running => "running",
                            DelegateTaskStatus::Completed { success: true } => "completed",
                            DelegateTaskStatus::Completed { success: false } => "completed_failed",
                            DelegateTaskStatus::Failed { .. } => "failed",
                        },
                        "reason": reason,
                    }),
                })
            }

            crate::tool_registry::delegate::DELEGATE_STATUS => {
                let snapshot = self.result_store.progress_snapshot();
                let projection = OperationWorkbenchProjection::from_delegate(&snapshot);
                let mut status_text = format!(
                    "# Delegate Tasks

Running: {} · Completed: {} · Failed: {} · Pending results: {}

| Task ID | Agent | Status | Result | Last Tool | Turn | Tasks | Description |
|---------|-------|--------|--------|-----------|------|-------|-------------|
",
                    snapshot.running, snapshot.completed, snapshot.failed, snapshot.pending_results
                );

                for child in &projection.children {
                    let task = self.result_store.get_task(&child.id);
                    let agent = task
                        .as_ref()
                        .and_then(|t| t.agent_name.as_deref())
                        .unwrap_or("default");
                    let description = task
                        .as_ref()
                        .map(|t| {
                            if t.task_description.len() > 50 {
                                crate::util::truncate(&t.task_description, 50)
                            } else {
                                t.task_description.clone()
                            }
                        })
                        .unwrap_or_else(|| child.label.clone());
                    let last_tool = child
                        .last_activity
                        .as_ref()
                        .map(|activity| activity.label.as_str())
                        .unwrap_or("-");
                    let last_turn = child
                        .last_activity
                        .as_ref()
                        .and_then(|activity| activity.turn)
                        .map(|turn| turn.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let task_progress = child
                        .progress
                        .as_ref()
                        .map(|progress| format!("{}/{}", progress.done, progress.total))
                        .unwrap_or_else(|| "0/0".to_string());
                    let result_state = task
                        .as_ref()
                        .map(|t| match (&t.status, t.result_viewed) {
                            (DelegateTaskStatus::Running, _) => "-",
                            (_, false) => "ready",
                            (_, true) => "viewed",
                        })
                        .unwrap_or("-");
                    status_text.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} | {} | {} |
",
                        child.id,
                        agent,
                        child.status_label,
                        result_state,
                        last_tool,
                        last_turn,
                        task_progress,
                        description
                    ));
                }

                if projection.children.is_empty() {
                    status_text.push_str(
                        "
No delegate tasks found.
",
                    );
                }

                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: status_text }],
                    details: projection.to_status_details(snapshot.active),
                })
            }

            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![
            CommandDefinition {
                name: crate::tool_registry::delegate::DELEGATE.to_string(),
                description: "subagent/delegate task management; same-provider is the default when no model is specified".to_string(),
                subcommands: vec!["status".to_string()],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
            },
            CommandDefinition {
                name: "subagent".to_string(),
                description: "alias for delegate subagent task management".to_string(),
                subcommands: vec!["status".to_string()],
                availability: omegon_traits::CommandAvailability::ALL,
                safety: omegon_traits::CommandSafety::READ_ONLY,
            },
        ]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name == "delegate" || name == "subagent" {
            match args.trim() {
                "status" | "" => {
                    let tasks = self.result_store.list_all_tasks();
                    let mut result =
                        format!("Subagent / Delegate Tasks ({} total):\n\n", tasks.len());

                    if tasks.is_empty() {
                        result.push_str("No delegate tasks found.\n");
                    } else {
                        for task in tasks {
                            let status = match task.status {
                                DelegateTaskStatus::Running => "⟳ Running",
                                DelegateTaskStatus::Completed { success: true } => "✓ Completed",
                                DelegateTaskStatus::Completed { success: false } => "✗ Failed",
                                DelegateTaskStatus::Failed { .. } => "✗ Error",
                                DelegateTaskStatus::Cancelled { .. } => "⊘ Cancelled",
                            };
                            let agent = task.agent_name.as_deref().unwrap_or("default");

                            result.push_str(&format!(
                                "  {} - {} - {} ({})\n",
                                task.task_id, status, task.task_description, agent
                            ));
                        }
                    }

                    CommandResult::Display(result)
                }
                _ => CommandResult::NotHandled,
            }
        } else {
            CommandResult::NotHandled
        }
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        let mut sections = Vec::new();

        // Model catalog from provider inventory
        if let Some(ref inventory_lock) = self.provider_inventory
            && let Ok(inventory) = inventory_lock.try_read()
        {
            // Trigger background re-probe if inventory is stale (>60s)
            if inventory.probed_at.elapsed().as_secs() > 60 {
                let inv = inventory_lock.clone();
                crate::task_spawn::spawn_best_effort("delegate-inventory-refresh", async move {
                    let mut inv = inv.write().await;
                    inv.probe_ollama().await;
                });
            }

            let session_model = self.session_model.lock().ok().and_then(|g| g.clone());
            let catalog = inventory.format_delegation_catalog(session_model.as_deref());
            if !catalog.is_empty() {
                sections.push(catalog);
            }
        }

        let progress = self.result_store.progress_snapshot();
        if progress.active || progress.pending_results > 0 || !progress.children.is_empty() {
            sections.push(format_delegate_queue_context(&progress));
        }

        // Available agents
        if !self.available_agents.is_empty() {
            let agents_list = self
                .available_agents
                .iter()
                .map(|agent| format!("  {} - {}", agent.name, agent.description))
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(format!("Available agents:\n{}", agents_list));
        }

        if sections.is_empty() {
            return None;
        }

        Some(ContextInjection {
            source: "delegate".to_string(),
            content: sections.join("\n\n"),
            priority: 6,
            ttl_turns: 10,
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::ContextBuild { user_prompt, .. } => {
                if !user_prompt.trim().is_empty() {
                    if self.recent_user_prompts.len() >= 50 {
                        self.recent_user_prompts.pop_front();
                    }
                    self.recent_user_prompts.push_back(user_prompt.clone());
                }
                vec![]
            }
            BusEvent::ToolStart { name, args, .. } => {
                // Track files the parent reads/edits so we can auto-populate
                // delegate scope when the model doesn't provide one.
                if matches!(name.as_str(), "read" | "view" | "edit" | "write") {
                    let path = args
                        .get("path")
                        .or_else(|| args.get("file_path"))
                        .or_else(|| args.get("file"))
                        .and_then(|v| v.as_str());
                    if let Some(p) = path {
                        // Move to end on re-access so auto-scope reflects
                        // recency, not first-seen order.
                        let ps = p.to_string();
                        self.recent_parent_files.retain(|f| f != &ps);
                        if self.recent_parent_files.len() >= 30 {
                            self.recent_parent_files.pop_front();
                        }
                        self.recent_parent_files.push_back(ps);
                    }
                }
                vec![]
            }
            BusEvent::TurnEnd(te) => {
                // Capture the parent session's model so delegate children
                // inherit it instead of falling back to hardcoded defaults.
                if let Some(m) = &te.model
                    && let Ok(mut slot) = self.session_model.lock()
                {
                    *slot = Some(m.clone());
                }
                if let Ok(mut handle) = self.progress_handle.lock() {
                    *handle = self.result_store.progress_snapshot();
                }
                // Check for completed background tasks and notify
                let tasks = self.result_store.list_all_tasks();
                let mut requests = Vec::new();

                for task in tasks {
                    if let DelegateTaskStatus::Completed { success } = task.status
                        && let Some(completed_at) = task.completed_at
                    {
                        // Only notify if completed recently (within last 5 seconds)
                        if completed_at.elapsed().unwrap_or_default().as_secs() < 5 {
                            self.emit_delegate_event(AgentEvent::DecompositionChildCompleted {
                                label: task.task_id.clone(),
                                success,
                                operation: OperationRef::delegate(task.task_id.clone()),
                            });
                            self.emit_delegate_family_vitals();
                            let message = if success {
                                format!(
                                    "✓ Delegate {} completed: {}",
                                    task.task_id, task.task_description
                                )
                            } else {
                                format!(
                                    "✗ Delegate {} failed: {}",
                                    task.task_id, task.task_description
                                )
                            };

                            requests.push(BusRequest::Notify {
                                message,
                                level: if success {
                                    NotifyLevel::Info
                                } else {
                                    NotifyLevel::Warning
                                },
                            });
                        }
                    }
                }

                requests
            }
            _ => vec![],
        }
    }
}

/// Agent loader - scans .omegon/agents/*.md files for agent specifications
pub fn scan_agents(cwd: &Path) -> Vec<AgentSpec> {
    let agents_dir = cwd.join(".omegon").join("agents");
    let mut agents = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            if let Some(extension) = entry.path().extension()
                && extension == "md"
                && let Ok(content) = std::fs::read_to_string(entry.path())
                && let Some(agent) = parse_agent_spec(&content)
            {
                agents.push(agent);
            }
        }
    }

    // Add default agents if no custom agents found
    if agents.is_empty() {
        agents.push(AgentSpec {
            name: "general".to_string(),
            description: "General-purpose assistant agent".to_string(),
            is_write_agent: true,
        });
        agents.push(AgentSpec {
            name: "analyzer".to_string(),
            description: "Read-only analysis and research agent".to_string(),
            is_write_agent: false,
        });
    }

    agents
}

fn enforce_delegate_policy_with_policy(
    policy: &crate::autonomy::SubagentPolicy,
    worker_profile: DelegateWorkerProfile,
    task: &str,
) -> Option<ToolResult> {
    if worker_profile != DelegateWorkerProfile::Patch
        || policy.delegate_patch == DecisionPolicy::Allow
    {
        return None;
    }

    let reason =
        "delegate patch worker requires structured approval under the active autonomy policy";
    Some(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Structured approval required: {reason}. Use scout/verify delegates for bounded non-mutating side quests, or approve mutating delegate patch work explicitly."
            ),
        }],
        details: required_approval_details(
            policy,
            ApprovalRequest {
                operation: "delegate",
                reason,
                requested: json!({
                    "worker_profile": worker_profile.as_str(),
                    "task": task,
                }),
                allowed: json!({
                    "delegate_scout": policy.delegate_scout == DecisionPolicy::Allow,
                    "delegate_verify": policy.delegate_verify == DecisionPolicy::Allow,
                    "delegate_patch": policy.delegate_patch == DecisionPolicy::Allow,
                }),
                grants: vec![omegon_traits::AuthorityGrant::DelegatePatch { max_tasks: Some(1) }],
            },
        ),
    })
}

fn enforce_delegate_policy(
    worker_profile: DelegateWorkerProfile,
    task: &str,
) -> Option<ToolResult> {
    let policy = active_subagent_policy();
    enforce_delegate_policy_with_policy(&policy, worker_profile, task)
}

fn format_background_delegate_started(task_id: &str) -> String {
    serde_json::json!({
        "task_id": task_id,
        "background": true,
        "status_hint": "/subagent status",
        "result_tool": "delegate_result",
        "result_tool_call": {
            "tool": "delegate_result",
            "arguments": { "task_id": task_id }
        }
    })
    .to_string()
}

fn delegate_tool_name_guidance(name: &str) -> Option<String> {
    match name {
        crate::tool_registry::delegate::DELEGATE_RESULT => Some(format!(
            "{name} is a tool, not a delegate agent. Retrieve results with delegate_result({{\"task_id\": \"delegate_N\"}}) or /delegate result delegate_N."
        )),
        crate::tool_registry::delegate::DELEGATE_STATUS => Some(format!(
            "{name} is a tool, not a delegate agent. Inspect delegate state with delegate_status({{}}) or /subagent status."
        )),
        crate::tool_registry::delegate::DELEGATE_CANCEL => Some(format!(
            "{name} is a tool, not a delegate agent. Cancel delegates with delegate_cancel({{\"task_id\": \"delegate_N\"}})."
        )),
        crate::tool_registry::delegate::DELEGATE => Some(format!(
            "{name} is the delegate launcher tool, not an agent name. Omit agent or choose a configured agent name."
        )),
        _ => None,
    }
}

fn format_delegate_queue_context(progress: &DelegateProgress) -> String {
    let mut lines = vec![format!(
        "Delegate queue: running={} completed={} failed={} pending_results={}",
        progress.running, progress.completed, progress.failed, progress.pending_results
    )];

    for child in progress.children.iter().take(8) {
        let result_state = if child.status == "running" {
            "active"
        } else if child.result_viewed {
            "viewed"
        } else {
            "result_ready"
        };
        let mut line = format!(
            "- {}: {} ({}) — {}",
            child.task_id, child.status, result_state, child.label
        );
        if let Some(summary) = child.result_summary.as_deref().filter(|s| !s.is_empty()) {
            line.push_str(&format!(" — {}", crate::util::truncate(summary, 120)));
        }
        lines.push(line);
    }

    if progress.children.len() > 8 {
        lines.push(format!(
            "- … {} more delegate task(s) omitted from context",
            progress.children.len() - 8
        ));
    }

    lines.push(
        "Instruction: before claiming no pending work, reconcile running delegates and fetch ready results with delegate_result.".to_string(),
    );
    lines.join("\n")
}

/// Parse agent specification from markdown content
fn parse_agent_spec(content: &str) -> Option<AgentSpec> {
    let lines: Vec<&str> = content.lines().collect();
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut is_write_agent = false;

    for line in lines {
        let line = line.trim();
        if let Some(title) = line.strip_prefix("# ") {
            name = Some(title.to_string());
        } else if let Some(desc) = line.strip_prefix("> ") {
            description = Some(desc.to_string());
        } else if line.contains("write") && (line.contains("agent") || line.contains("mode")) {
            is_write_agent = true;
        }
    }

    if let (Some(name), Some(description)) = (name, description) {
        Some(AgentSpec {
            name,
            description,
            is_write_agent,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn background_delegate_started_includes_machine_result_tool_call() {
        let parsed: serde_json::Value =
            serde_json::from_str(&format_background_delegate_started("delegate_7")).unwrap();

        assert_eq!(parsed["result_tool"], "delegate_result");
        assert_eq!(parsed["result_tool_call"]["tool"], "delegate_result");
        assert_eq!(
            parsed["result_tool_call"]["arguments"]["task_id"],
            "delegate_7"
        );
    }

    #[test]
    fn delegate_tool_name_guidance_rejects_result_tool_as_agent() {
        let guidance = delegate_tool_name_guidance("delegate_result").expect("tool-name guidance");

        assert!(guidance.contains("tool, not a delegate agent"));
        assert!(guidance.contains("delegate_result"));
        assert!(guidance.contains("task_id"));
    }

    fn delegate_policy_requires_approval_for_patch_worker() {
        let result = enforce_delegate_policy(DelegateWorkerProfile::Patch, "edit the file")
            .expect("patch delegates require approval under conservative autonomy");

        assert_eq!(result.details["approval_required"], true);
        assert_eq!(result.details["operation"], "delegate");
        assert_eq!(result.details["autonomy"], "conservative");
        assert_eq!(result.details["requested"]["worker_profile"], "patch");
        assert_eq!(result.details["requested"]["task"], "edit the file");
        assert_eq!(result.details["allowed"]["delegate_scout"], true);
        assert_eq!(result.details["allowed"]["delegate_verify"], true);
        assert_eq!(result.details["allowed"]["delegate_patch"], false);
        assert_eq!(
            result.details["required_approval"]["kind"],
            "approval_required"
        );
        assert_eq!(result.details["required_approval"]["operation"], "delegate");
        assert_eq!(
            result.details["required_approval"]["autonomy"],
            "conservative"
        );
        assert_eq!(
            result.details["required_approval"]["choices"][0]["grants"][0]["kind"],
            "delegate_patch"
        );
    }

    #[test]
    fn delegate_policy_allows_patch_worker_under_orchestrator_policy() {
        let policy = crate::autonomy::SubagentPolicy::for_level(
            crate::autonomy::AutonomyLevel::Orchestrator,
        );
        assert!(
            enforce_delegate_policy_with_policy(
                &policy,
                DelegateWorkerProfile::Patch,
                "edit the file"
            )
            .is_none()
        );
    }

    #[test]
    fn delegate_feature_resolves_live_settings_policy() {
        let temp_dir = TempDir::new().unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        settings.lock().unwrap().automation_level = crate::settings::AutomationLevel::Autonomous;
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false).with_settings(settings);

        let policy = feature.subagent_policy();
        assert_eq!(policy.level, crate::autonomy::AutonomyLevel::Orchestrator);
        assert_eq!(policy.delegate_patch, DecisionPolicy::Allow);
    }

    #[test]
    fn delegate_feature_falls_back_to_conservative_policy_without_settings() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);

        let policy = feature.subagent_policy();
        assert_eq!(policy.level, crate::autonomy::AutonomyLevel::Conservative);
        assert_eq!(policy.delegate_patch, DecisionPolicy::RequireApproval);
    }

    #[test]
    fn delegate_policy_allows_scout_and_verify_workers() {
        assert!(enforce_delegate_policy(DelegateWorkerProfile::Scout, "inspect").is_none());
        assert!(enforce_delegate_policy(DelegateWorkerProfile::Verify, "test").is_none());
    }

    #[test]
    fn delegate_failure_formatter_surfaces_provider_and_runtime_context() {
        let runtime = DelegateRuntimeRequest {
            scope: Some(vec!["src/lib.rs".into()]),
            model: Some("ollama:qwen3:32b".into()),
            thinking_level: Some("minimal".into()),
            worker_profile: DelegateWorkerProfile::Patch,
        };
        let rendered = format_delegate_child_failure(
            Some(1),
            "model not found, try pulling it first",
            "ollama:qwen3:32b",
            &runtime,
            std::path::Path::new("/tmp/.delegate-prompt.md"),
        );
        assert!(rendered.contains("provider: ollama"));
        assert!(rendered.contains("model: ollama:qwen3:32b"));
        assert!(rendered.contains("worker_profile: patch"));
        assert!(rendered.contains("scope: src/lib.rs"));
        assert!(rendered.contains("Suggested next step"));
    }

    #[test]
    fn hidden_change_tool_does_not_seed_recent_parent_files() {
        let temp_dir = TempDir::new().unwrap();
        let mut feature = DelegateFeature::new(temp_dir.path(), vec![], false);

        feature.on_event(&BusEvent::ToolStart {
            id: "1".into(),
            name: "change".into(),
            args: serde_json::json!({"path": "src/lib.rs"}),
            capabilities: vec![],
        });

        assert!(feature.recent_parent_files.is_empty());
    }

    #[test]
    fn delegate_worker_profile_defaults_to_scout_and_is_hyper_scaled_down() {
        let profile = DelegateWorkerProfile::parse(None);
        assert_eq!(profile, DelegateWorkerProfile::Scout);
        let runtime = profile.runtime_profile(None, None, None);
        assert_eq!(runtime.context_class.as_deref(), Some("compact"));
        assert_eq!(runtime.thinking_level.as_deref(), Some("low"));
        assert_eq!(profile.max_turns(), 4);
        assert_eq!(
            runtime.enabled_tools,
            vec!["read", "bash", "codebase_search", "view"]
        );
        assert!(runtime.disabled_tools.iter().any(|t| t == "delegate"));
        assert!(runtime.disabled_tools.iter().any(|t| t == "cleave_run"));
        assert!(runtime.disabled_tools.iter().any(|t| t == "memory_store"));
    }

    #[test]
    fn delegate_prompt_includes_execution_boundary() {
        let temp_dir = TempDir::new().unwrap();
        let runner = DelegateRunner::new(
            temp_dir.path().to_path_buf(),
            std::sync::Arc::new(DelegateResultStore::new()),
            false,
        );
        let prompt = runner.build_delegate_prompt(
            DelegateWorkerProfile::Scout,
            "Inspect the file",
            Some(&["src/lib.rs".into()]),
            None,
            "",
        );

        assert!(prompt.contains("## Execution Boundary"));
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("Unavailable tools/resources"));
        assert!(prompt.contains("delegate"));
        assert!(prompt.contains("cleave_run"));
        assert!(prompt.contains("stop and report the blocker"));
    }

    #[test]
    fn delegate_prompt_path_lives_under_omegon_state_dir() {
        assert_eq!(
            DelegateRunner::delegate_prompt_path("delegate_7").unwrap(),
            ".omegon/delegate-prompts/delegate_7.md"
        );
    }

    #[test]
    fn delegate_prompt_explicitly_forbids_trailing_prefill_stub() {
        let temp_dir = TempDir::new().unwrap();
        let runner = DelegateRunner::new(
            temp_dir.path().to_path_buf(),
            std::sync::Arc::new(DelegateResultStore::new()),
            false,
        );
        let prompt = runner.build_delegate_prompt(
            DelegateWorkerProfile::Patch,
            "Fix the bug",
            Some(&["src/lib.rs".into()]),
            None,
            "",
        );
        assert!(
            prompt.contains("single final answer")
                && prompt.contains("trailing assistant prefill stub"),
            "got: {prompt}"
        );
    }

    #[test]
    fn delegate_worker_profiles_disable_nested_subagent_tools() {
        for profile in [
            DelegateWorkerProfile::Scout,
            DelegateWorkerProfile::Patch,
            DelegateWorkerProfile::Verify,
        ] {
            let runtime = profile.runtime_profile(None, None, None);
            for tool in [
                "delegate",
                "delegate_result",
                "delegate_status",
                "delegate_cancel",
                "cleave_assess",
                "cleave_run",
            ] {
                assert!(
                    runtime
                        .disabled_tools
                        .iter()
                        .any(|disabled| disabled == tool),
                    "{profile:?} should disable nested subagent tool {tool}"
                );
            }
        }
    }

    #[test]
    fn delegate_worker_profiles_specialize_tool_surface() {
        let patch = DelegateWorkerProfile::Patch.runtime_profile(None, Some("minimal"), None);
        assert_eq!(patch.enabled_tools, vec!["read", "edit", "bash"]);
        assert_eq!(DelegateWorkerProfile::Patch.max_turns(), 6);

        let verify = DelegateWorkerProfile::Verify.runtime_profile(None, None, None);
        assert_eq!(verify.enabled_tools, vec!["read", "bash"]);
        assert_eq!(DelegateWorkerProfile::Verify.max_turns(), 4);
    }

    #[test]
    fn background_delegate_started_message_preserves_machine_readable_result() {
        let rendered = format_background_delegate_started("delegate_42");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed["task_id"], "delegate_42");
        assert_eq!(parsed["background"], true);
        assert_eq!(parsed["status_hint"], "/subagent status");
        assert_eq!(parsed["result_tool"], "delegate_result");
    }

    #[test]
    fn delegate_tool_schema_exposes_worker_profile() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);
        let delegate_tool = feature
            .tools()
            .into_iter()
            .find(|tool| tool.name == "delegate")
            .expect("delegate tool exists");
        let worker = &delegate_tool.parameters["properties"]["worker_profile"];
        assert_eq!(worker["default"].as_str(), Some("scout"));
        assert!(
            worker["enum"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some("patch"))
        );
    }

    #[tokio::test]
    async fn test_delegate_feature_tools() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![AgentSpec {
            name: "test_agent".to_string(),
            description: "Test agent".to_string(),
            is_write_agent: true,
        }];

        let feature = DelegateFeature::new(temp_dir.path(), agents, false);
        let tools = feature.tools();

        assert_eq!(tools.len(), 4);
        assert!(tools.iter().any(|t| t.name == "delegate"));
        assert!(tools.iter().any(|t| t.name == "delegate_result"));
        assert!(tools.iter().any(|t| t.name == "delegate_status"));
        assert!(tools.iter().any(|t| t.name == "delegate_cancel"));
    }

    #[test]
    fn delegate_status_commands_are_read_only() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);
        let commands = feature.commands();
        let delegate = commands
            .iter()
            .find(|command| command.name == "delegate")
            .unwrap();
        let subagent = commands
            .iter()
            .find(|command| command.name == "subagent")
            .unwrap();
        assert_eq!(delegate.safety, omegon_traits::CommandSafety::READ_ONLY);
        assert_eq!(subagent.safety, omegon_traits::CommandSafety::READ_ONLY);
    }

    #[test]
    fn delegate_commands_expose_subagent_alias_for_registered_surfaces() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);
        let commands = feature.commands();
        assert!(commands.iter().any(|command| command.name == "delegate"));
        assert!(commands.iter().any(|command| command.name == "subagent"));
    }

    #[tokio::test]
    async fn test_sync_delegate_unknown_agent() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![];

        let feature = DelegateFeature::new(temp_dir.path(), agents, false);

        let args = json!({
            "task": "test task",
            "agent": "unknown_agent",
            "background": false
        });

        let result = feature
            .execute("delegate", "test_call", args, CancellationToken::new())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delegate_result_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);

        let args = json!({ "task_id": "nonexistent_task" });

        let result = feature
            .execute(
                "delegate_result",
                "test_call",
                args,
                CancellationToken::new(),
            )
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_context_lists_agents() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![
            AgentSpec {
                name: "agent1".to_string(),
                description: "First agent".to_string(),
                is_write_agent: true,
            },
            AgentSpec {
                name: "agent2".to_string(),
                description: "Second agent".to_string(),
                is_write_agent: false,
            },
        ];

        let feature = DelegateFeature::new(temp_dir.path(), agents, false);

        let signals = ContextSignals {
            user_prompt: "test",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 1000,
        };

        let context = feature.provide_context(&signals);
        assert!(context.is_some());

        let context = context.unwrap();
        assert!(context.content.contains("agent1"));
        assert!(context.content.contains("agent2"));
        assert!(context.content.contains("First agent"));
        assert!(context.content.contains("Second agent"));
    }

    #[test]
    fn test_parse_agent_spec() {
        let content = r#"# TestAgent

> A test agent for testing purposes

This agent runs in write mode and can modify files.
"#;

        let spec = parse_agent_spec(content);
        assert!(spec.is_some());

        let spec = spec.unwrap();
        assert_eq!(spec.name, "TestAgent");
        assert_eq!(spec.description, "A test agent for testing purposes");
        assert!(spec.is_write_agent);
    }

    #[test]
    fn test_scan_agents() {
        let temp_dir = TempDir::new().unwrap();
        let agents_dir = temp_dir.path().join(".omegon").join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();

        // Create test agent files
        std::fs::write(
            agents_dir.join("test1.md"),
            "# TestAgent1\n\n> Test agent 1\n\nwrite agent capabilities",
        )
        .unwrap();

        std::fs::write(
            agents_dir.join("test2.md"),
            "# TestAgent2\n\n> Test agent 2\n\nread-only analysis",
        )
        .unwrap();

        let agents = scan_agents(temp_dir.path());
        assert_eq!(agents.len(), 2);

        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"TestAgent1"));
        assert!(names.contains(&"TestAgent2"));

        // Check write agent detection
        let write_agent = agents.iter().find(|a| a.name == "TestAgent1").unwrap();
        assert!(write_agent.is_write_agent);

        let read_agent = agents.iter().find(|a| a.name == "TestAgent2").unwrap();
        assert!(!read_agent.is_write_agent);
    }

    #[tokio::test]
    async fn system_error_messages_rejected_as_delegate_tasks() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);

        let cases = [
            "[System: Your last several delegate calls returned errors]",
            "STUCK LOOP DETECTED — stop retrying",
            "Your last several delegate calls returned errors. Read files first.",
            "last several `delegate` calls returned errors",
        ];
        for task in &cases {
            let args = json!({ "task": task, "background": false });
            let result = feature
                .execute("delegate", "c1", args, CancellationToken::new())
                .await;
            assert!(
                result.is_err(),
                "should reject system error as task: {task}"
            );
        }

        // Valid task should not be rejected
        let args = json!({ "task": "Fix the login bug", "background": true });
        let result = feature
            .execute("delegate", "c2", args, CancellationToken::new())
            .await;
        // Will error for other reasons (no binary), but not the system-message guard
        if let Err(e) = &result {
            assert!(
                !e.to_string().contains("recycled system warning"),
                "valid task should not be rejected: {e}"
            );
        }
    }

    #[test]
    fn dedup_finds_completed_tasks_by_description() {
        let store = DelegateResultStore::new();

        store.store_task(DelegateTask {
            task_id: "d_1".into(),
            agent_name: None,
            task_description: "Fix the auth bug".into(),
            status: DelegateTaskStatus::Completed { success: true },
            result: Some("Fixed in auth.rs".into()),
            result_viewed: false,
            started_at: SystemTime::now(),
            completed_at: Some(SystemTime::now()),
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });

        // Exact match (case-insensitive)
        assert!(
            store
                .find_completed_by_description("fix the auth bug")
                .is_some()
        );
        assert!(
            store
                .find_completed_by_description("FIX THE AUTH BUG")
                .is_some()
        );

        // Different description — no match
        assert!(
            store
                .find_completed_by_description("Fix the login bug")
                .is_none()
        );

        // Failed task — not returned
        store.store_task(DelegateTask {
            task_id: "d_2".into(),
            agent_name: None,
            task_description: "Fix the login bug".into(),
            status: DelegateTaskStatus::Failed {
                error: "timeout".into(),
                kind: DelegateChildFailureKind::Unknown,
            },
            result: None,
            result_viewed: false,
            started_at: SystemTime::now(),
            completed_at: Some(SystemTime::now()),
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });
        assert!(
            store
                .find_completed_by_description("Fix the login bug")
                .is_none()
        );
    }

    #[tokio::test]
    async fn consecutive_failures_disable_delegate() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);

        // Simulate 3 failures by directly setting the counter
        if let Ok(mut count) = feature.consecutive_failures.lock() {
            *count = 3;
        }

        let args = json!({ "task": "Do something", "background": false });
        let result = feature
            .execute("delegate", "c1", args, CancellationToken::new())
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("disabled for this session"),
            "should report disabled: {err}"
        );

        // Reset counter — should allow again
        if let Ok(mut count) = feature.consecutive_failures.lock() {
            *count = 0;
        }
        let args = json!({ "task": "Do something else", "background": true });
        let result = feature
            .execute("delegate", "c2", args, CancellationToken::new())
            .await;
        // Won't error with the disabled message
        if let Err(e) = &result {
            assert!(
                !e.to_string().contains("disabled for this session"),
                "should not be disabled after reset: {e}"
            );
        }
    }

    #[tokio::test]
    async fn delegate_cancel_tool_reports_cancelled_status() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(temp_dir.path(), vec![], false);
        let now = SystemTime::now();
        feature.result_store.store_task(DelegateTask {
            task_id: "delegate_1".into(),
            agent_name: Some("verify".into()),
            task_description: "Verify cancellation".into(),
            status: DelegateTaskStatus::Running,
            result: None,
            result_viewed: false,
            started_at: now,
            completed_at: None,
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });

        let result = feature
            .execute(
                "delegate_cancel",
                "cancel_call",
                json!({ "task_id": "delegate_1", "reason": "operator stopped it" }),
                CancellationToken::new(),
            )
            .await
            .expect("cancel tool result");

        let text = result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        assert!(
            text.contains("Delegate task delegate_1 cancelled: operator stopped it"),
            "{text}"
        );
        assert_eq!(result.details["task_id"], "delegate_1");
        assert_eq!(result.details["status"], "cancelled");
        assert_eq!(result.details["reason"], "operator stopped it");

        let snapshot = feature.result_store.progress_snapshot();
        assert_eq!(snapshot.running, 0);
        assert_eq!(snapshot.failed, 0);
        assert_eq!(snapshot.children[0].status, "cancelled");
    }

    #[test]
    fn progress_snapshot_exposes_cancelled_delegate_state_without_failure_count() {
        let store = DelegateResultStore::new();
        let now = SystemTime::now();
        store.store_task(DelegateTask {
            task_id: "delegate_cancelled".into(),
            agent_name: Some("verify".into()),
            task_description: "Verify cancellation".into(),
            status: DelegateTaskStatus::Cancelled {
                reason: Some("operator stopped task".into()),
            },
            result: Some("operator stopped task".into()),
            result_viewed: false,
            started_at: now,
            completed_at: Some(now),
            last_tool: Some("bash".into()),
            last_tool_activity: None,
            last_turn: Some(1),
            tasks: Vec::new(),
        });

        let snapshot = store.progress_snapshot();
        assert!(!snapshot.active);
        assert_eq!(snapshot.running, 0);
        assert_eq!(snapshot.completed, 0);
        assert_eq!(snapshot.failed, 0);
        assert_eq!(snapshot.children.len(), 1);
        assert_eq!(snapshot.children[0].status, "cancelled");
        assert_eq!(snapshot.children[0].failure_kind, None);
        assert_eq!(
            snapshot.children[0].result_summary.as_deref(),
            Some("operator stopped task")
        );
    }

    #[test]
    fn progress_snapshot_exposes_running_completed_and_failed_delegate_state() {
        let store = DelegateResultStore::new();
        let now = SystemTime::now();
        store.store_task(DelegateTask {
            task_id: "delegate_1".into(),
            agent_name: Some("scout".into()),
            task_description: "- [ ] Inspect files\n- [ ] Report findings".into(),
            status: DelegateTaskStatus::Running,
            result: None,
            result_viewed: false,
            started_at: now,
            completed_at: None,
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: extract_task_items("- [ ] Inspect files\n- [ ] Report findings"),
        });
        store.update_task_live_state(
            "delegate_1",
            Some("read".into()),
            Some("core/lib.rs".into()),
            Some(2),
        );
        store.store_task(DelegateTask {
            task_id: "delegate_2".into(),
            agent_name: Some("verify".into()),
            task_description: "Run checks".into(),
            status: DelegateTaskStatus::Completed { success: true },
            result: Some("Validation passed with a long enough result summary to truncate".into()),
            result_viewed: false,
            started_at: now,
            completed_at: Some(now),
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });
        store.store_task(DelegateTask {
            task_id: "delegate_3".into(),
            agent_name: Some("patch".into()),
            task_description: "Patch bug".into(),
            status: DelegateTaskStatus::Failed {
                error: "child exited".into(),
                kind: DelegateChildFailureKind::Unknown,
            },
            result: None,
            result_viewed: false,
            started_at: now,
            completed_at: Some(now),
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });

        let snapshot = store.progress_snapshot();

        assert!(snapshot.active);
        assert_eq!(snapshot.running, 1);
        assert_eq!(snapshot.completed, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.children.len(), 3);
        let running = snapshot
            .children
            .iter()
            .find(|c| c.task_id == "delegate_1")
            .unwrap();
        assert_eq!(running.status, "running");
        assert_eq!(running.last_tool.as_deref(), Some("read"));
        assert_eq!(
            running
                .last_tool_activity
                .as_ref()
                .unwrap()
                .args_summary
                .as_deref(),
            Some("core/lib.rs")
        );
        assert_eq!(running.last_turn, Some(2));
        assert_eq!(running.tasks_done, 1);
        let completed = snapshot
            .children
            .iter()
            .find(|c| c.task_id == "delegate_2")
            .unwrap();
        assert_eq!(completed.status, "completed");
        assert!(
            completed
                .result_summary
                .as_ref()
                .unwrap()
                .starts_with("Validation passed")
        );
        assert!(
            completed.result_summary.as_ref().unwrap().len()
                < "Validation passed with a long enough result summary to truncate".len()
        );
        let failed = snapshot
            .children
            .iter()
            .find(|c| c.task_id == "delegate_3")
            .unwrap();
        assert_eq!(failed.status, "failed");
    }

    fn write_fake_child(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(body.as_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        path
    }

    #[tokio::test]
    async fn delegate_runner_executes_injected_child_successfully() {
        let temp_dir = TempDir::new().unwrap();
        let child = write_fake_child(
            temp_dir.path(),
            "fake-child-success.sh",
            "#!/bin/sh\necho delegate-child-ok\n",
        );
        let store = Arc::new(DelegateResultStore::new());
        let runner = DelegateRunner::new(temp_dir.path().to_path_buf(), store.clone(), false)
            .with_child_agent_binary(child);

        let result = runner
            .run_delegate_child(
                "delegate_success",
                "Do the thing",
                &DelegateRuntimeRequest {
                    scope: None,
                    model: Some("test:model".into()),
                    thinking_level: None,
                    worker_profile: DelegateWorkerProfile::Scout,
                },
                None,
                Some("test:model".into()),
            )
            .await
            .unwrap();

        assert_eq!(result, "delegate-child-ok");
    }

    #[tokio::test]
    async fn delegate_runner_surfaces_injected_child_failure_context() {
        let temp_dir = TempDir::new().unwrap();
        let child = write_fake_child(
            temp_dir.path(),
            "fake-child-fail.sh",
            "#!/bin/sh\necho child stderr line >&2\nexit 7\n",
        );
        let store = Arc::new(DelegateResultStore::new());
        let runner = DelegateRunner::new(temp_dir.path().to_path_buf(), store.clone(), false)
            .with_child_agent_binary(child);

        let err = runner
            .run_delegate_child(
                "delegate_fail",
                "Do the thing",
                &DelegateRuntimeRequest {
                    scope: None,
                    model: Some("test:model".into()),
                    thinking_level: None,
                    worker_profile: DelegateWorkerProfile::Scout,
                },
                None,
                Some("test:model".into()),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("exit_code: 7"), "{err}");
        assert!(err.contains("child stderr line"), "{err}");
        assert!(err.contains("test:model"), "{err}");
    }

    #[tokio::test]
    async fn delegate_runner_times_out_and_kills_silent_child() {
        let temp_dir = TempDir::new().unwrap();
        let child = write_fake_child(
            temp_dir.path(),
            "fake-child-timeout.sh",
            "#!/bin/sh\nexec sleep 10\n",
        );
        let store = Arc::new(DelegateResultStore::new());
        let runner = DelegateRunner::new(temp_dir.path().to_path_buf(), store.clone(), false)
            .with_child_agent_binary(child)
            .with_timeouts(1, 30);
        store.store_task(DelegateTask {
            task_id: "delegate_timeout".to_string(),
            task_description: "Do the thing".to_string(),
            agent_name: Some("timeout-worker".to_string()),
            status: DelegateTaskStatus::Running,
            result: None,
            result_viewed: false,
            started_at: SystemTime::now(),
            completed_at: None,
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });

        let err = runner
            .run_delegate_child(
                "delegate_timeout",
                "Do the thing",
                &DelegateRuntimeRequest {
                    scope: None,
                    model: Some("test:model".into()),
                    thinking_level: None,
                    worker_profile: DelegateWorkerProfile::Scout,
                },
                None,
                Some("test:model".into()),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("Delegate wall-clock timeout after 1s"),
            "{err}"
        );
        store.update_task_status(
            "delegate_timeout",
            DelegateTaskStatus::Failed {
                error: err.clone(),
                kind: DelegateChildFailureKind::Unknown,
            },
            Some(err),
        );
        let progress = store.progress_snapshot();
        assert!(
            !progress.active,
            "timeout should clear active delegate progress"
        );
        assert_eq!(progress.running, 0);
        assert_eq!(progress.failed, 1);
        let child = progress
            .children
            .iter()
            .find(|child| child.task_id == "delegate_timeout")
            .unwrap();
        assert_eq!(child.status, "failed");
        assert!(
            child
                .result_summary
                .as_deref()
                .unwrap_or_default()
                .contains("Delegate wall-clock timeout")
        );
    }

    #[tokio::test]
    async fn delegate_runner_idle_timeout_kills_quiet_child_after_initial_activity() {
        let temp_dir = TempDir::new().unwrap();
        let child = write_fake_child(
            temp_dir.path(),
            "fake-child-idle-timeout.sh",
            "#!/bin/sh
echo initial activity >&2
exec sleep 10
",
        );
        let store = Arc::new(DelegateResultStore::new());
        let runner = DelegateRunner::new(temp_dir.path().to_path_buf(), store.clone(), false)
            .with_child_agent_binary(child)
            .with_timeouts(30, 1);
        store.store_task(DelegateTask {
            task_id: "delegate_idle_timeout".to_string(),
            task_description: "Do the quiet thing".to_string(),
            agent_name: Some("idle-worker".to_string()),
            status: DelegateTaskStatus::Running,
            result: None,
            result_viewed: false,
            started_at: SystemTime::now(),
            completed_at: None,
            last_tool: None,
            last_tool_activity: None,
            last_turn: None,
            tasks: Vec::new(),
        });

        let err = runner
            .run_delegate_child(
                "delegate_idle_timeout",
                "Do the quiet thing",
                &DelegateRuntimeRequest {
                    scope: None,
                    model: Some("test:model".into()),
                    thinking_level: None,
                    worker_profile: DelegateWorkerProfile::Scout,
                },
                None,
                Some("test:model".into()),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("Delegate idle timeout"), "{err}");
        assert!(err.contains("no output for 1s"), "{err}");
        store.update_task_status(
            "delegate_idle_timeout",
            DelegateTaskStatus::Failed {
                error: err.clone(),
                kind: DelegateChildFailureKind::Unknown,
            },
            Some(err),
        );
        let progress = store.progress_snapshot();
        assert!(
            !progress.active,
            "idle timeout should clear active delegate progress"
        );
        assert_eq!(progress.running, 0);
        assert_eq!(progress.failed, 1);
        let child = progress
            .children
            .iter()
            .find(|child| child.task_id == "delegate_idle_timeout")
            .unwrap();
        assert_eq!(child.status, "failed");
        assert!(
            child
                .result_summary
                .as_deref()
                .unwrap_or_default()
                .contains("Delegate idle timeout")
        );
    }
}
