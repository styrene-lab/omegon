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
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type DelegateEventSlot = Arc<Mutex<Option<BusRequestSink>>>;
use std::time::SystemTime;

use crate::child_agent::{
    ChildAgentRuntimeProfile, ChildAgentSpawnConfig, spawn_headless_child_agent,
    write_child_prompt_file,
};
use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use omegon_traits::{
    AgentEvent, BusEvent, BusRequest, BusRequestSink, CommandDefinition, CommandResult,
    ContentBlock, ContextInjection, ContextSignals, Feature, NotifyLevel, ToolDefinition,
    ToolResult,
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
    pub last_tool: Option<String>,
    pub started_at: Option<std::time::SystemTime>,
    pub completed_at: Option<std::time::SystemTime>,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DelegateProgress {
    pub active: bool,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<DelegateProgressChild>,
}

/// Status of a delegate task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DelegateTaskStatus {
    Running,
    Completed { success: bool },
    Failed { error: String },
}

/// A delegate task entry in the result store
#[derive(Debug, Clone)]
pub struct DelegateTask {
    pub task_id: String,
    pub agent_name: Option<String>,
    pub task_description: String,
    pub status: DelegateTaskStatus,
    pub result: Option<String>,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
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
            if matches!(
                task.status,
                DelegateTaskStatus::Completed { .. } | DelegateTaskStatus::Failed { .. }
            ) {
                task.completed_at = Some(SystemTime::now());
            }
        }
    }

    pub fn list_all_tasks(&self) -> Vec<DelegateTask> {
        let tasks = self.tasks.lock().unwrap();
        tasks.values().cloned().collect()
    }

    /// Check if a task with the same description was already completed
    /// successfully. Returns the task_id if found.
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

    pub fn progress_snapshot(&self) -> DelegateProgress {
        let tasks = self.list_all_tasks();
        let mut progress = DelegateProgress::default();
        for task in tasks {
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
            };
            progress.children.push(DelegateProgressChild {
                task_id: task.task_id.clone(),
                label: task
                    .agent_name
                    .clone()
                    .unwrap_or_else(|| task.task_id.clone()),
                status: status.to_string(),
                last_tool: None,
                started_at: Some(task.started_at),
                completed_at: task.completed_at,
                result_summary: task.result.as_ref().map(|r| crate::util::truncate(r, 40)),
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
            context_class: Some("squad".to_string()),
            thinking_level: Some(thinking_level.unwrap_or("low").to_string()),
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
                "set_model_tier".into(),
                "switch_to_offline_driver".into(),
                "set_thinking_level".into(),
                "session_log".into(),
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
            Self::Patch => vec!["read".into(), "edit".into(), "change".into(), "bash".into()],
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelegateChildFailureKind {
    MissingLocalModel,
    MissingCredential,
    ProviderStartup,
    WorkspaceStartup,
    Unknown,
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
    if matches!(provider.as_str(), "anthropic" | "openai" | "openai-codex" | "openrouter" | "groq" | "xai" | "mistral" | "cerebras" | "huggingface" | "ollama" | "ollama-cloud") {
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
    out.push_str(&format!("  worker_profile: {}\n", runtime.worker_profile.as_str()));
    out.push_str(&format!(
        "  thinking: {}\n",
        runtime.thinking_level.as_deref().unwrap_or("minimal")
    ));
    out.push_str("  context_class: squad\n");
    out.push_str(&format!(
        "  scope: {}\n",
        runtime
            .scope
            .as_ref()
            .map(|s| if s.is_empty() { "(none)".to_string() } else { s.join(", ") })
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
    pub fn new(cwd: PathBuf, result_store: Arc<DelegateResultStore>) -> Self {
        Self { cwd, result_store }
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
        if let Ok(Some(lease)) = crate::workspace::runtime::read_workspace_lease(&self.cwd) {
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

        prompt.push_str("## Task\n");
        prompt.push_str(task);
        prompt.push_str("\n");
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

    async fn run_delegate_child(
        &self,
        prompt: &str,
        runtime: &DelegateRuntimeRequest,
        mind: Option<&str>,
        session_model: Option<String>,
    ) -> anyhow::Result<String> {
        let prompt_path = write_child_prompt_file(&self.cwd, ".delegate-prompt.md", prompt)?;

        let model = match runtime.model.clone() {
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
            agent_binary: std::env::current_exe()
                .context("delegate runner could not locate current executable")?,
            model: model.clone(),
            max_turns: runtime.worker_profile.max_turns(),
            inherited_env: Vec::new(),
            injected_env: Vec::new(),
            runtime: runtime.worker_profile.runtime_profile(
                runtime.scope.as_deref(),
                runtime.thinking_level.as_deref(),
                mind,
            ),
        };
        let (child, _pid) = spawn_headless_child_agent(&child_config, &self.cwd, &prompt_path)?;
        let output = child
            .wait_with_output()
            .await
            .context("delegate child process failed to execute")?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.is_empty() {
                Ok("Delegate completed with no stdout.".to_string())
            } else {
                Ok(stdout)
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(anyhow::anyhow!(format_delegate_child_failure(
                output.status.code(),
                &stderr,
                &model,
                runtime,
                &prompt_path,
            )))
        }
    }

    async fn spawn_delegate(
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

        let task_entry = DelegateTask {
            task_id: task_id.clone(),
            agent_name,
            task_description: task.clone(),
            status: DelegateTaskStatus::Running,
            result: None,
            started_at: SystemTime::now(),
            completed_at: None,
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
        crate::task_spawn::spawn_best_effort_result("delegate-real-task", async move {
            let runner = DelegateRunner::new(cwd, store.clone());
            match runner
                .run_delegate_child(&prompt, &runtime, mind.as_deref(), parent_model)
                .await
            {
                Ok(result) => {
                    store.update_task_status(
                        &task_id,
                        DelegateTaskStatus::Completed { success: true },
                        Some(result),
                    );
                    // Reset failure counter on success.
                    if let Ok(mut count) = fail_counter.lock() {
                        *count = 0;
                    }
                }
                Err(err) => {
                    store.update_task_status(
                        &task_id,
                        DelegateTaskStatus::Failed {
                            error: err.to_string(),
                        },
                        None,
                    );
                    // Increment failure counter.
                    if let Ok(mut count) = fail_counter.lock() {
                        *count += 1;
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
        for _ in 0..60 {
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
                    DelegateTaskStatus::Failed { error } => {
                        return Err(anyhow::anyhow!("Task failed: {}", error));
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
}

impl DelegateFeature {
    pub fn new(cwd: &PathBuf, agents: Vec<AgentSpec>) -> Self {
        let result_store = Arc::new(DelegateResultStore::new());
        let runner = Arc::new(DelegateRunner::new(cwd.clone(), result_store.clone()));

        let progress_handle = Arc::new(Mutex::new(DelegateProgress::default()));
        let event_slot = Arc::new(Mutex::new(None));
        Self {
            result_store,
            available_agents: agents,
            runner,
            progress_handle,
            event_slot,
            session_model: Arc::new(Mutex::new(None)),
            consecutive_failures: Arc::new(Mutex::new(0)),
        }
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
            sink.send(BusRequest::EmitAgentEvent { event });
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
            pending: 0,
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
                    last_tool: child.result_summary.clone(),
                    last_turn: None,
                    tokens_in: 0,
                    tokens_out: 0,
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
                label: "Delegate Task".to_string(),
                description: "Spawn a subagent to handle a specific task".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "Task description for the delegate"
                        },
                        "agent": { "type": "string" },
                        "scope": { "type": "array", "items": {"type": "string"} },
                        "model": { "type": "string" },
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
            },
            ToolDefinition {
                name: crate::tool_registry::delegate::DELEGATE_STATUS.to_string(),
                label: "Delegate Status".to_string(),
                description: "List all delegate tasks and their status".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {}
                }),
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
                        "Delegate task rejected: the task description is a system error \
                         message, not an actual task. Review the original goal and \
                         formulate a concrete task description."
                    ));
                }

                // Validate agent if specified
                if let Some(ref agent_name) = agent
                    && !self.available_agents.iter().any(|a| a.name == *agent_name)
                {
                    return Err(anyhow::anyhow!("Unknown agent: {}", agent_name));
                }

                // Dedup: if an identical task already completed successfully,
                // return the prior result instead of spawning again.
                if let Some(prior_id) = self.result_store.find_completed_by_description(&task) {
                    if let Some(prior_task) = self.result_store.get_task(&prior_id) {
                        let result_text = prior_task.result.unwrap_or_else(|| "completed".into());
                        return Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "This task was already completed ({}). Result:\n{}",
                                    prior_id, result_text
                                ),
                            }],
                            details: serde_json::json!(null),
                        });
                    }
                }

                let task_id = self.result_store.generate_task_id();

                // Spawn the delegate
                let parent_model = self.session_model.lock().ok().and_then(|s| s.clone());
                self.runner
                    .spawn_delegate(
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
                    )
                    .await?;
                if let Ok(mut handle) = self.progress_handle.lock() {
                    *handle = self.result_store.progress_snapshot();
                }
                self.emit_delegate_event(AgentEvent::DecompositionStarted {
                    children: vec![task_id.clone()],
                });
                self.emit_delegate_family_vitals();

                if background {
                    // Return task ID for background execution
                    Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("{{\"task_id\": \"{}\"}}", task_id),
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
                        DelegateTaskStatus::Completed { success: true } => Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: task.result.unwrap_or_else(|| "Task completed".to_string()),
                            }],
                            details: json!({ "status": "completed", "success": true, "task_id": task_id }),
                        }),
                        DelegateTaskStatus::Completed { success: false } => Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: "Task completed with failure".to_string(),
                            }],
                            details: json!({ "status": "completed", "success": false, "task_id": task_id }),
                        }),
                        DelegateTaskStatus::Failed { error } => Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!("Task failed: {}", error),
                            }],
                            details: json!({ "status": "failed", "error": error, "task_id": task_id }),
                        }),
                    },
                    None => Err(anyhow::anyhow!("Task not found: {}", task_id)),
                }
            }

            crate::tool_registry::delegate::DELEGATE_STATUS => {
                let tasks = self.result_store.list_all_tasks();
                let mut status_text = String::from(
                    "# Delegate Tasks\n\n| Task ID | Agent | Status | Description |\n|---------|-------|--------|-------------|\n",
                );

                for task in tasks {
                    let agent = task.agent_name.as_deref().unwrap_or("default");
                    let status = match task.status {
                        DelegateTaskStatus::Running => "🔄 Running".to_string(),
                        DelegateTaskStatus::Completed { success: true } => {
                            "✅ Completed".to_string()
                        }
                        DelegateTaskStatus::Completed { success: false } => "❌ Failed".to_string(),
                        DelegateTaskStatus::Failed { .. } => "❌ Error".to_string(),
                    };
                    let description = if task.task_description.len() > 50 {
                        crate::util::truncate(&task.task_description, 50)
                    } else {
                        task.task_description.clone()
                    };

                    status_text.push_str(&format!(
                        "| {} | {} | {} | {} |\n",
                        task.task_id, agent, status, description
                    ));
                }

                if self.result_store.list_all_tasks().is_empty() {
                    status_text.push_str("\nNo delegate tasks found.\n");
                }

                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: status_text }],
                    details: json!({ "task_count": self.result_store.list_all_tasks().len() }),
                })
            }

            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: crate::tool_registry::delegate::DELEGATE.to_string(),
            description: "delegate task management".to_string(),
            subcommands: vec!["status".to_string()],
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name == "delegate" {
            match args.trim() {
                "status" | "" => {
                    let tasks = self.result_store.list_all_tasks();
                    let mut result = format!("Delegate Tasks ({} total):\n\n", tasks.len());

                    if tasks.is_empty() {
                        result.push_str("No delegate tasks found.\n");
                    } else {
                        for task in tasks {
                            let status = match task.status {
                                DelegateTaskStatus::Running => "🔄 Running",
                                DelegateTaskStatus::Completed { success: true } => "✅ Completed",
                                DelegateTaskStatus::Completed { success: false } => "❌ Failed",
                                DelegateTaskStatus::Failed { .. } => "❌ Error",
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
        if self.available_agents.is_empty() {
            return None;
        }

        let agents_list = self
            .available_agents
            .iter()
            .map(|agent| format!("  {} - {}", agent.name, agent.description))
            .collect::<Vec<_>>()
            .join("\n");

        Some(ContextInjection {
            source: "delegate".to_string(),
            content: format!("Available agents:\n{}", agents_list),
            priority: 5,
            ttl_turns: 10,
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnEnd { model, .. } => {
                // Capture the parent session's model so delegate children
                // inherit it instead of falling back to hardcoded defaults.
                if let Some(m) = model {
                    if let Ok(mut slot) = self.session_model.lock() {
                        *slot = Some(m.clone());
                    }
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
                            });
                            self.emit_delegate_family_vitals();
                            let message = if success {
                                format!(
                                    "✅ Delegate {} completed: {}",
                                    task.task_id, task.task_description
                                )
                            } else {
                                format!(
                                    "❌ Delegate {} failed: {}",
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
pub fn scan_agents(cwd: &PathBuf) -> Vec<AgentSpec> {
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
    fn delegate_worker_profile_defaults_to_scout_and_is_hyper_scaled_down() {
        let profile = DelegateWorkerProfile::parse(None);
        assert_eq!(profile, DelegateWorkerProfile::Scout);
        let runtime = profile.runtime_profile(None, None, None);
        assert_eq!(runtime.context_class.as_deref(), Some("squad"));
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
    fn delegate_prompt_explicitly_forbids_trailing_prefill_stub() {
        let temp_dir = TempDir::new().unwrap();
        let runner = DelegateRunner::new(
            temp_dir.path().to_path_buf(),
            std::sync::Arc::new(DelegateResultStore::new()),
        );
        let prompt = runner.build_delegate_prompt(
            DelegateWorkerProfile::Patch,
            "Fix the bug",
            Some(&["src/lib.rs".into()]),
            None,
            "",
        );
        assert!(
            prompt.contains("single final answer") && prompt.contains("trailing assistant prefill stub"),
            "got: {prompt}"
        );
    }

    #[test]
    fn delegate_worker_profiles_specialize_tool_surface() {
        let patch = DelegateWorkerProfile::Patch.runtime_profile(None, Some("minimal"), None);
        assert_eq!(patch.enabled_tools, vec!["read", "edit", "change", "bash"]);
        assert_eq!(DelegateWorkerProfile::Patch.max_turns(), 6);

        let verify = DelegateWorkerProfile::Verify.runtime_profile(None, None, None);
        assert_eq!(verify.enabled_tools, vec!["read", "bash"]);
        assert_eq!(DelegateWorkerProfile::Verify.max_turns(), 4);
    }

    #[test]
    fn delegate_tool_schema_exposes_worker_profile() {
        let temp_dir = TempDir::new().unwrap();
        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), vec![]);
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

        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), agents);
        let tools = feature.tools();

        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "delegate"));
        assert!(tools.iter().any(|t| t.name == "delegate_result"));
        assert!(tools.iter().any(|t| t.name == "delegate_status"));
    }

    #[tokio::test]
    async fn test_sync_delegate_unknown_agent() {
        let temp_dir = TempDir::new().unwrap();
        let agents = vec![];

        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), agents);

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
        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), vec![]);

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

        let feature = DelegateFeature::new(&temp_dir.path().to_path_buf(), agents);

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

        let agents = scan_agents(&temp_dir.path().to_path_buf());
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
}
