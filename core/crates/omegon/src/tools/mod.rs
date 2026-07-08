//! Core tools — the agent's primary capabilities.
//!
//! Phase 0: primitive tools (bash, read, write, edit).
//! Phase 0+: higher-level tools (change, understand, execute, remember)
//!           that compose the primitives.

pub mod bash;
pub mod change;
pub mod chronos;
pub mod codebase_search;
pub mod edit;
pub mod local_inference;
pub mod native_cmd;
pub mod nex_substrate;
pub mod openapi;
pub mod openapi_config;
pub mod openapi_resolve;
pub(crate) mod output_filter;
pub mod read;
pub mod render;
pub mod serve;
pub mod terminal;
pub mod validate;
pub mod view;
pub mod web_search;
pub mod whoami;
pub mod write;

pub mod secret_tools;

// Phase 0+ stubs:
// pub mod understand;  // tree-sitter + scope graph
// pub mod execute;     // bash with progressive disclosure
// pub mod remember;    // session scratchpad

use async_trait::async_trait;
use omegon_traits::{ContentBlock, ToolDefinition, ToolProgressSink, ToolProvider, ToolResult};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use crate::tool_registry::core as reg;

pub const PLAN_LIST_VISIBLE_ITEM_LIMIT: usize = 5;
pub const PLAN_LIST_CHANGE_LIMIT: usize = 12;
pub const PLAN_LIST_GROUP_LIMIT: usize = 4;

#[derive(Debug, Clone)]
pub struct LifecyclePlanProjection {
    pub entries: Vec<crate::conversation::PlanRegistryEntry>,
    pub tasks: Vec<crate::conversation::PlanItemProjection>,
    pub task_identity_findings: Vec<crate::lifecycle::spec::TaskStableIdFinding>,
}

pub fn lifecycle_plan_projection(repo_root: &Path) -> LifecyclePlanProjection {
    use crate::conversation::{
        PlanBinding, PlanItemProjection, PlanRegistryEntry, PlanScope, PlanSource, PlanStatus,
        PlanTaskSourceRef, PlanTaskStableIdQuality, ProgressSummary, TaskCompletionPolicy,
        TaskIntent, WorkItemStatus,
    };

    let openspec_revision = |change_name: &str, task_id: &str, label: &str| {
        format!(
            "source-v1:openspec:{}:{}:{}",
            change_name,
            task_id,
            stable_hash(label)
        )
    };
    let design_revision = |node_id: &str, anchor: &str, label: &str| {
        format!(
            "source-v1:design:{}:{}:{}",
            node_id,
            anchor,
            stable_hash(label)
        )
    };

    let mut entries = Vec::new();
    let mut tasks = Vec::new();
    let mut task_identity_findings = Vec::new();
    for change in crate::lifecycle::spec::list_changes(repo_root) {
        let plan_id = PlanBinding::openspec_plan_id(&change.name, None);
        let binding = PlanBinding {
            openspec_change: Some(change.name.clone()),
            ..PlanBinding::default()
        };
        let status = if change.has_tasks {
            if change.total_tasks > 0 && change.done_tasks >= change.total_tasks {
                PlanStatus::Completed
            } else {
                PlanStatus::Active
            }
        } else {
            PlanStatus::Stale
        };
        entries.push(PlanRegistryEntry {
            plan_id: plan_id.clone(),
            title: change.name.clone(),
            scope: PlanScope::Repo,
            source: PlanSource::OpenSpec,
            status,
            binding: binding.clone(),
            progress: ProgressSummary {
                completed: change.done_tasks,
                total: change.total_tasks,
            },
            resume_hint: Some(format!("OpenSpec · {}", change.stage.as_str())),
        });
        if change.has_tasks {
            let tasks_path = change.path.join("tasks.md");
            if let Ok(report) = crate::lifecycle::spec::validate_task_stable_ids(&tasks_path) {
                task_identity_findings.extend(report.findings);
            }
        }
        for group in &change.task_groups {
            let group_plan_id = PlanBinding::openspec_plan_id(&change.name, Some(&group.title));
            for task in &group.tasks {
                tasks.push(PlanItemProjection {
                    id: format!("{}:{}", group_plan_id, task.id),
                    stable_id: task
                        .stable_id
                        .clone()
                        .unwrap_or_else(|| format!("openspec:{}:task:{}", change.name, task.id)),
                    stable_id_quality: if task.stable_id.is_some() {
                        PlanTaskStableIdQuality::Explicit
                    } else {
                        PlanTaskStableIdQuality::Fallback
                    },
                    revision: openspec_revision(&change.name, &task.id, &task.description),
                    source: PlanTaskSourceRef {
                        kind: "openspec".to_string(),
                        path: Some(format!("openspec/changes/{}/tasks.md", change.name)),
                        anchor: Some(task.id.clone()),
                    },
                    supported_mutations: Vec::new(),
                    plan_id: plan_id.clone(),
                    label: task.description.clone(),
                    status: if task.done {
                        WorkItemStatus::Done
                    } else {
                        WorkItemStatus::Pending
                    },
                    intent: TaskIntent::Spec,
                    completion_policy: TaskCompletionPolicy::LifecycleStateReached,
                    evidence: Vec::new(),
                    external_task_refs: binding.external_task_refs.clone(),
                    writable: false,
                });
            }
        }
    }
    let design_nodes = crate::lifecycle::design::scan_design_docs(&repo_root.join("docs"));
    for node in design_nodes.values() {
        if !matches!(
            node.status,
            crate::lifecycle::types::NodeStatus::Exploring
                | crate::lifecycle::types::NodeStatus::Decided
                | crate::lifecycle::types::NodeStatus::Implementing
                | crate::lifecycle::types::NodeStatus::Blocked
        ) {
            continue;
        }
        let plan_id = PlanBinding::design_plan_id(&node.id);
        let status = match node.status {
            crate::lifecycle::types::NodeStatus::Blocked => PlanStatus::Blocked,
            crate::lifecycle::types::NodeStatus::Implemented => PlanStatus::Completed,
            _ => PlanStatus::Active,
        };
        let binding = PlanBinding {
            design_node_id: Some(node.id.clone()),
            openspec_change: node.openspec_change.clone(),
            ..PlanBinding::default()
        };
        let total = node.open_questions.len().max(1);
        let completed = if node.open_questions.is_empty() { 1 } else { 0 };
        entries.push(PlanRegistryEntry {
            plan_id: plan_id.clone(),
            title: node.title.clone(),
            scope: PlanScope::Repo,
            source: if node.openspec_change.is_some() {
                PlanSource::Hybrid
            } else {
                PlanSource::Design
            },
            status,
            binding: binding.clone(),
            progress: ProgressSummary { completed, total },
            resume_hint: Some(format!("Design · {}", node.status.as_str())),
        });
        if node.open_questions.is_empty() {
            tasks.push(PlanItemProjection {
                id: format!("{}:decision", plan_id),
                stable_id: format!("design:{}:decision", node.id),
                stable_id_quality: PlanTaskStableIdQuality::Explicit,
                revision: design_revision(
                    &node.id,
                    "decision",
                    "Record or verify design decision evidence",
                ),
                source: PlanTaskSourceRef {
                    kind: if node.openspec_change.is_some() {
                        "hybrid"
                    } else {
                        "design"
                    }
                    .to_string(),
                    path: Some(repo_relative_path(repo_root, &node.file_path)),
                    anchor: Some("decision".to_string()),
                },
                supported_mutations: Vec::new(),
                plan_id: plan_id.clone(),
                label: "Record or verify design decision evidence".to_string(),
                status: WorkItemStatus::Pending,
                intent: TaskIntent::Design,
                completion_policy: TaskCompletionPolicy::EvidenceRequired,
                evidence: Vec::new(),
                external_task_refs: binding.external_task_refs.clone(),
                writable: false,
            });
        } else {
            for (idx, question) in node.open_questions.iter().enumerate() {
                tasks.push(PlanItemProjection {
                    id: format!("{}:question:{}", plan_id, idx + 1),
                    stable_id: format!("design:{}:question:{}", node.id, idx + 1),
                    stable_id_quality: PlanTaskStableIdQuality::Fallback,
                    revision: design_revision(&node.id, &format!("question:{}", idx + 1), question),
                    source: PlanTaskSourceRef {
                        kind: if node.openspec_change.is_some() {
                            "hybrid"
                        } else {
                            "design"
                        }
                        .to_string(),
                        path: Some(repo_relative_path(repo_root, &node.file_path)),
                        anchor: Some(format!("question:{}", idx + 1)),
                    },
                    supported_mutations: Vec::new(),
                    plan_id: plan_id.clone(),
                    label: question.clone(),
                    status: WorkItemStatus::Pending,
                    intent: TaskIntent::Design,
                    completion_policy: TaskCompletionPolicy::EvidenceRequired,
                    evidence: Vec::new(),
                    external_task_refs: binding.external_task_refs.clone(),
                    writable: false,
                });
            }
        }
    }

    LifecyclePlanProjection {
        entries,
        tasks,
        task_identity_findings,
    }
}

/// Leniently extract a numeric tool argument as `usize`.
///
/// Providers and models are inconsistent about numeric JSON encoding in tool
/// calls: the same parameter can arrive as `4237`, `4237.0`, or `"4237"`
/// across generations. A strict `.as_u64()` silently drops the string and
/// float forms, which for `read` meant offset/limit vanished and the whole
/// file came back from line 1.
pub(crate) fn lenient_usize_arg(args: &serde_json::Value, key: &str) -> Option<usize> {
    match args.get(key)? {
        serde_json::Value::Number(n) => n
            .as_u64()
            .map(|n| n as usize)
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as usize)),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<usize>()
            .ok()
            .or_else(|| s.trim().parse::<f64>().ok().filter(|f| *f >= 0.0).map(|f| f as usize)),
        _ => None,
    }
}

fn stable_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn repo_relative_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

pub fn render_lifecycle_plan_list(repo_root: &Path) -> String {
    let changes = crate::lifecycle::spec::list_changes(repo_root);
    let mut lines = vec!["OpenSpec".to_string()];
    if changes.is_empty() {
        lines.push("- none".to_string());
        return lines.join("\n");
    }

    let change_total = changes.len();
    for change in changes.iter().take(PLAN_LIST_CHANGE_LIMIT) {
        lines.push(format!(
            "- {} · {} · {}/{}",
            change.name,
            change.stage.as_str(),
            change.done_tasks,
            change.total_tasks
        ));
        let group_total = change.task_groups.len();
        for group in change.task_groups.iter().take(PLAN_LIST_GROUP_LIMIT) {
            let done = group.tasks.iter().filter(|task| task.done).count();
            lines.push(format!(
                "  - {} · {}/{}",
                group.title,
                done,
                group.tasks.len()
            ));
        }
        if group_total > PLAN_LIST_GROUP_LIMIT {
            lines.push(format!(
                "  - … and {} more groups",
                group_total - PLAN_LIST_GROUP_LIMIT
            ));
        }
    }
    if change_total > PLAN_LIST_CHANGE_LIMIT {
        lines.push(format!(
            "- … and {} more OpenSpec changes",
            change_total - PLAN_LIST_CHANGE_LIMIT
        ));
    }
    lines.join("\n")
}

/// Error returned by `WorkspaceBoundary::check_path` when a path is outside
/// the workspace and not in any trusted directory. The dispatch layer
/// intercepts this to show an interactive permission prompt in the TUI.
#[derive(Debug)]
pub struct PathPermissionError {
    pub requested_path: String,
    pub directory: String,
    pub workspace: String,
}

impl std::fmt::Display for PathPermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PERMISSION REQUIRED — '{}' is outside workspace '{}'",
            self.requested_path, self.workspace
        )
    }
}

impl std::error::Error for PathPermissionError {}

pub const OPERATOR_WAIT_DEFAULT_SECS: u64 = 30 * 60;
pub const OPERATOR_WAIT_MAX_SECS: u64 = 6 * 60 * 60;

/// Error returned by `wait_for_operator`. The dispatch layer intercepts this
/// typed error and owns the interactive wait/confirmation lifecycle.
#[derive(Debug, Clone)]
pub struct OperatorWaitRequired {
    pub prompt: String,
    pub timeout_secs: u64,
}

impl std::fmt::Display for OperatorWaitRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Manual action required: {} (timeout: {}s)",
            self.prompt, self.timeout_secs
        )
    }
}

impl std::error::Error for OperatorWaitRequired {}

// ── Workspace boundary ──────────────────────────────────────────────────

/// Filesystem boundary enforcer — shared by all tools that touch the filesystem.
///
/// Checks whether a path is inside the workspace or a trusted directory.
/// If not, returns a `PathPermissionError`. This is Tier 1 enforcement:
/// defense-in-depth for non-sandboxed operation. The Nex container sandbox
/// (Tier 3) provides the hard kernel-level boundary.
///
/// `Clone` via `Arc` so it can be passed to CoreTools, ViewProvider,
/// native_cmd dispatch, and the bash heuristic scanner.
#[derive(Clone)]
pub struct WorkspaceBoundary {
    cwd: PathBuf,
    settings: Option<crate::settings::SharedSettings>,
    session_approved: std::sync::Arc<std::sync::Mutex<Vec<PathBuf>>>,
    /// When true, all boundary checks are bypassed (--dangerously-bypass-permissions).
    /// Stored as a struct field, not read from env vars, so the model cannot
    /// influence it at runtime.
    bypass: bool,
}

impl WorkspaceBoundary {
    /// Create a boundary anchored at the given workspace directory.
    pub fn new(cwd: PathBuf) -> Self {
        // Read bypass from env only once at construction time — not on
        // every check_path call. The model cannot set env vars in the
        // parent Rust process, so this is safe. But reading a struct
        // field is more explicit than re-checking the env on every call.
        let bypass = std::env::var("OMEGON_BYPASS_PERMISSIONS").is_ok();
        Self {
            cwd,
            settings: None,
            session_approved: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            bypass,
        }
    }

    /// Attach shared settings for trusted_directories resolution.
    pub fn with_settings(mut self, settings: crate::settings::SharedSettings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// The workspace root directory.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Check whether a path is inside the workspace or a trusted directory.
    /// Returns the resolved path on success, or `PathPermissionError` on violation.
    ///
    /// When `OMEGON_BYPASS_PERMISSIONS=1` is set (via `--dangerously-bypass-permissions`),
    /// all paths are allowed without checking.
    pub fn check_path(&self, path_str: &str) -> anyhow::Result<PathBuf> {
        let is_absolute = path_str.starts_with('/') || path_str.starts_with('~');
        let resolved = if is_absolute {
            expand_tilde(path_str)
        } else {
            self.cwd.join(path_str)
        };

        // Bypass mode — all paths allowed (--dangerously-bypass-permissions)
        if self.bypass {
            return Ok(resolved);
        }

        if is_allowed_special_path(&resolved) {
            return Ok(resolved);
        }

        // Canonicalize to resolve symlinks and `..` — but the file may not
        // exist yet (write/edit creating new files). In that case, canonicalize
        // the parent directory and append the filename.
        let canonical = canonicalize_existing_parent(&resolved);

        let cwd_canonical = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());

        // Inside workspace — always allowed
        if canonical.starts_with(&cwd_canonical) {
            return Ok(resolved);
        }

        // Outside workspace — check trusted directories
        if self.is_trusted_path(&canonical) {
            return Ok(resolved);
        }

        // Outside workspace and not trusted — hard block.
        let parent_dir = canonical
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Err(PathPermissionError {
            requested_path: path_str.to_string(),
            directory: parent_dir,
            workspace: cwd_canonical.display().to_string(),
        }
        .into())
    }

    /// Pure predicate — returns true if the path is inside the workspace
    /// or a trusted directory. Does not return error details.
    pub fn is_inside_boundary(&self, path: &Path) -> bool {
        if self.bypass {
            return true;
        }

        if is_allowed_special_path(path) {
            return true;
        }

        let cwd_canonical = self.cwd.canonicalize().unwrap_or_else(|_| self.cwd.clone());
        let canonical = canonicalize_existing_parent(path);

        if canonical.starts_with(&cwd_canonical) {
            return true;
        }

        self.is_trusted_path(&canonical)
    }

    /// Record a directory as approved for this session.
    pub fn approve_directory(&self, dir: PathBuf) {
        let canonical = canonicalize_existing_parent(&dir);
        if let Ok(mut approved) = self.session_approved.lock()
            && !approved.iter().any(|d| d == &canonical)
        {
            tracing::info!(dir = %canonical.display(), count = approved.len() + 1, "session directory approved");
            approved.push(canonical);
        }
    }

    /// Check if a path is within a trusted directory (from settings or
    /// session-level approvals).
    fn is_trusted_path(&self, path: &Path) -> bool {
        // Check session-level approvals
        if let Ok(approved) = self.session_approved.lock() {
            tracing::trace!(
                path = %path.display(),
                approved_count = approved.len(),
                approved_dirs = ?approved.iter().map(|d| d.display().to_string()).collect::<Vec<_>>(),
                "checking trusted path"
            );
            if approved.iter().any(|dir| {
                let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                let matches = path.starts_with(&canonical_dir);
                if !matches {
                    tracing::trace!(
                        path = %path.display(),
                        dir = %dir.display(),
                        canonical = %canonical_dir.display(),
                        "no match"
                    );
                }
                matches
            }) {
                return true;
            }
        }

        // Check settings-level trusted directories
        if let Some(ref settings) = self.settings
            && let Ok(s) = settings.lock()
        {
            for trusted in &s.trusted_directories {
                let expanded = expand_tilde(trusted);
                let canonical = expanded.canonicalize().unwrap_or(expanded);
                if path.starts_with(&canonical) {
                    return true;
                }
            }
        }
        false
    }
}

fn is_allowed_special_path(path: &Path) -> bool {
    let Some(path_str) = path.to_str() else {
        return false;
    };

    #[cfg(windows)]
    if path_str.eq_ignore_ascii_case("NUL") {
        return true;
    }

    if is_allowed_temp_path(path) {
        return true;
    }

    matches!(
        path_str,
        "/dev/null"
            | "/dev/stdin"
            | "/dev/stdout"
            | "/dev/stderr"
            | "/dev/fd/0"
            | "/dev/fd/1"
            | "/dev/fd/2"
            | "/proc/self/fd/0"
            | "/proc/self/fd/1"
            | "/proc/self/fd/2"
    )
}

fn is_allowed_temp_path(path: &Path) -> bool {
    let temp_dir = std::env::temp_dir();
    let normalized_path = lexical_normalize(path);
    let normalized_temp_dir = lexical_normalize(&temp_dir);

    normalized_path == normalized_temp_dir || normalized_path.starts_with(&normalized_temp_dir)
}

/// Expand `~` to the home directory in a path string.
fn expand_tilde(path_str: &str) -> PathBuf {
    if let Some(rest) = path_str.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path_str)
}

// ── Core tool provider ──────────────────────────────────────────────────

/// Core tool provider — registers the primitive tools.
pub struct CoreTools {
    cwd: PathBuf,
    /// Repository model — tracks branch, dirty files, submodules.
    /// None if not inside a git repo.
    repo_model: Option<std::sync::Arc<omegon_git::RepoModel>>,
    /// Workspace boundary enforcer — shared with other tool providers.
    boundary: WorkspaceBoundary,
    terminal_tool_enabled: bool,
    nex_delegations: Vec<crate::nex::substrate::NexSubstrateDelegation>,
}

impl CoreTools {
    pub fn new(cwd: PathBuf) -> Self {
        let boundary = WorkspaceBoundary::new(cwd.clone());
        Self {
            cwd,
            repo_model: None,
            boundary,
            terminal_tool_enabled: true,
            nex_delegations: Vec::new(),
        }
    }

    /// Create with a RepoModel for git-aware operations.
    pub fn with_repo_model(
        cwd: PathBuf,
        repo_model: std::sync::Arc<omegon_git::RepoModel>,
    ) -> Self {
        let boundary = WorkspaceBoundary::new(cwd.clone());
        Self {
            cwd,
            repo_model: Some(repo_model),
            boundary,
            terminal_tool_enabled: true,
            nex_delegations: Vec::new(),
        }
    }

    /// Attach shared settings for trusted directory resolution.
    pub fn with_settings(mut self, settings: crate::settings::SharedSettings) -> Self {
        self.terminal_tool_enabled = settings.lock().map(|s| s.terminal_tool).unwrap_or(true);
        self.boundary = self.boundary.with_settings(settings);
        self
    }

    /// Attach read-only Nex delegations discovered from extension metadata.
    pub fn with_nex_delegations(
        mut self,
        delegations: Vec<crate::nex::substrate::NexSubstrateDelegation>,
    ) -> Self {
        self.nex_delegations = delegations;
        self
    }

    /// Get a clone of the workspace boundary for sharing with other providers.
    pub fn boundary(&self) -> &WorkspaceBoundary {
        &self.boundary
    }

    /// Record a directory as approved for this session.
    pub fn approve_directory(&self, dir: PathBuf) {
        self.boundary.approve_directory(dir);
    }

    /// Resolve a user-provided path against cwd, enforcing workspace boundaries.
    fn resolve_path(&self, path_str: &str) -> anyhow::Result<PathBuf> {
        self.boundary.check_path(path_str)
    }
}

fn canonicalize_existing_parent(path: &Path) -> PathBuf {
    if path.exists() {
        return path
            .canonicalize()
            .unwrap_or_else(|_| lexical_normalize(path));
    }

    let Some(parent) = path.parent() else {
        return lexical_normalize(path);
    };

    if parent.exists() {
        return parent
            .canonicalize()
            .unwrap_or_else(|_| lexical_normalize(parent))
            .join(path.file_name().unwrap_or_default());
    }

    let canonical_parent = canonicalize_existing_parent(parent);
    match path.file_name() {
        Some(file_name) => canonical_parent.join(file_name),
        None => canonical_parent,
    }
}

/// Lexical path normalization — resolve `.` and `..` without filesystem access.
/// Used as a fallback when the path doesn't exist yet.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Only pop if there's a normal component to pop
                if components
                    .last()
                    .is_some_and(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            std::path::Component::CurDir => {} // skip
            _ => components.push(component),
        }
    }
    components.iter().collect()
}

#[async_trait]
impl ToolProvider for CoreTools {
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: reg::BASH.into(),
                label: reg::BASH.into(),
                description: "Execute a bash command in the current working directory. \
                    Returns stdout and stderr. Output is truncated to last 2000 lines \
                    or 50KB. Optionally provide a timeout in seconds."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Bash command to execute"
                        },
                        "timeout": {
                            "type": "number",
                            "description": "Timeout in seconds (optional)"
                        }
                    },
                    "required": ["command"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: reg::READ.into(),
                label: reg::READ.into(),
                description: "Read the contents of a file. Supports text files and images. \
                    Output is truncated to 2000 lines or 50KB. Use offset/limit for \
                    large files. Paths outside the workspace require user approval — \
                    if rejected, ask the user to approve the directory."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read"
                        },
                        "offset": {
                            "type": "number",
                            "description": "Line number to start from (1-indexed)"
                        },
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of lines to read"
                        }
                    },
                    "required": ["path"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::RepoInspection,
                    omegon_traits::ToolCapability::TargetedRepoInspection,
                ],
            },
            ToolDefinition {
                name: reg::WRITE.into(),
                label: reg::WRITE.into(),
                description: "Write content to a file. Creates the file if it doesn't \
                    exist, overwrites if it does. Automatically creates parent directories. \
                    Paths outside the workspace require user approval — if rejected, \
                    ask the user to approve the directory."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write"
                        }
                    },
                    "required": ["path", "content"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::Mutation,
                    omegon_traits::ToolCapability::StateChanging,
                ],
            },
            ToolDefinition {
                name: reg::EDIT.into(),
                label: reg::EDIT.into(),
                description: "Single-target exact-text replacement in one file. The oldText must \
                    match exactly (including whitespace). Use this for one precise, surgical \
                    replacement when you do not need atomic coordination across multiple edits."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to edit"
                        },
                        "oldText": {
                            "type": "string",
                            "description": "Exact text to find and replace"
                        },
                        "newText": {
                            "type": "string",
                            "description": "New text to replace the old text with"
                        }
                    },
                    "required": ["path", "oldText", "newText"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::Mutation,
                    omegon_traits::ToolCapability::StateChanging,
                ],
            },
            ToolDefinition {
                name: reg::VALIDATE.into(),
                label: reg::VALIDATE.into(),
                description: "Run narrow project validation for specific paths. Use this after edits \
                    instead of shelling out through bash for standard typecheck/lint/test validation. \
                    If the result reports validation_skipped, do not retry validate for the same path set; \
                    use the recommended project-specific command or a validator plugin instead."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "description": "One or more file paths whose project validators should run",
                            "items": { "type": "string" }
                        },
                        "level": {
                            "type": "string",
                            "enum": ["quick", "standard", "full"],
                            "description": "Validation depth (default: standard)"
                        }
                    },
                    "required": ["paths"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Validation],
            },
            ToolDefinition {
                name: reg::CHANGE.into(),
                label: reg::CHANGE.into(),
                description: "Atomic multi-file edit with optional explicit validation. Hidden from the \
                    model-facing tool surface; used by the harness to batch coordinated edits."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "edits": {
                            "type": "array",
                            "description": "Array of edits to apply atomically",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string", "description": "File path" },
                                    "oldText": { "type": "string", "description": "Exact text to find" },
                                    "newText": { "type": "string", "description": "Replacement text" }
                                },
                                "required": ["file", "oldText", "newText"]
                            }
                        },
                        "validate": {
                            "type": "string",
                            "description": "Optional validation mode: none (default), quick, standard, full (includes tests)",
                            "default": "none"
                        }
                    },
                    "required": ["edits"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::Mutation,
                    omegon_traits::ToolCapability::StateChanging,
                ],
            },
            ToolDefinition {
                name: reg::COMMIT.into(),
                label: reg::COMMIT.into(),
                description: "Commit changes to git. Stages the specified files (or all \
                    dirty files if none specified), creates a commit with the given message. \
                    Handles submodule commits automatically — if edited files are inside a \
                    submodule, the harness commits inside the submodule first and updates \
                    the parent pointer. Use conventional commit format for the message."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "Commit message (conventional commit format: type(scope): description)"
                        },
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Specific file paths to commit. Omit to commit all dirty files."
                        }
                    },
                    "required": ["message"]
                }),
                capabilities: vec![
                    omegon_traits::ToolCapability::StateChanging,
                    omegon_traits::ToolCapability::ProgressBoundary,
                ],
            },
            ToolDefinition {
                name: reg::PLAN.into(),
                label: reg::PLAN.into(),
                description: "Manage the session work plan — the primary operator-facing \
                    Workbench surface for what's happening now and the agent's guidepost while \
                    working. Use 'list' to inspect the visible plan plus lifecycle/OpenSpec \
                    plan summaries, 'set' to establish a plan at the start of multi-step work, \
                    'approve' to mark operator approval, 'execute' to start mutation work, \
                    'advance' to mark the current item done and move to the next item, \
                    'complete' to mark a specific item done, 'skip' to deliberately bypass an \
                    item, and 'clear' only when the visible plan gate is no longer useful. \
                    If you create or inherit a visible plan, keep it truthful before final \
                    replies: update, complete, skip, or clear stale active/todo items rather \
                    than leaving the Workbench stale."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["set", "approve", "execute", "advance", "complete", "skip", "clear", "status", "list"],
                            "description": "Action to perform"
                        },
                        "items": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Work items (required for 'set')"
                        },
                        "index": {
                            "type": "number",
                            "description": "Item index, 0-based (for 'complete')"
                        }
                    },
                    "required": ["action"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: reg::WAIT_FOR_OPERATOR.into(),
                label: reg::WAIT_FOR_OPERATOR.into(),
                description: "Pause the agent while the operator performs a physical/manual \
                    action, then resume when the operator confirms completion. Use this when \
                    real-world work is required before the next agent step, such as adjusting \
                    hardware, testing an instrument, moving a device, or observing a live \
                    system. The wait is bounded by a safety timeout."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Clear instruction for the operator describing the physical/manual action to perform"
                        },
                        "timeout": {
                            "type": "number",
                            "description": "Safety timeout in seconds. Defaults to 1800 and is capped at 21600."
                        }
                    },
                    "required": ["prompt"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::ProgressBoundary],
            },
            ToolDefinition {
                name: reg::WHOAMI.into(),
                label: reg::WHOAMI.into(),
                description: "Check authentication status across development tools \
                    (git, GitHub, GitLab, AWS, Kubernetes, OCI registries). Returns \
                    structured status with error diagnosis and refresh commands for \
                    expired or missing sessions."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: reg::CHRONOS.into(),
                label: reg::CHRONOS.into(),
                description: "Get authoritative date and time context from the system clock. \
                    Use before any date calculations, weekly/monthly reporting, relative date \
                    references, quarter boundaries, or epoch timestamps. Eliminates AI date \
                    calculation errors.\n\nSubcommands:\n  week (default) — Current/previous \
                    week boundaries (Mon-Fri)\n  month — Current/previous month boundaries\n  \
                    quarter — Calendar quarter, fiscal year (Oct-Sep)\n  relative — Resolve \
                    expression like '3 days ago', 'next Monday'\n  iso — ISO 8601 week number, \
                    year, day-of-year\n  epoch — Unix timestamp (seconds and milliseconds)\n  \
                    tz — Timezone abbreviation and UTC offset\n  range — Calendar and business \
                    days between two dates\n  all — All of the above combined"
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "subcommand": {
                            "type": "string",
                            "enum": ["week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all"],
                            "description": "Subcommand (default: week)"
                        },
                        "expression": {
                            "type": "string",
                            "description": "For 'relative': date expression (e.g. '3 days ago', 'next Monday')"
                        },
                        "from_date": {
                            "type": "string",
                            "description": "For 'range': start date YYYY-MM-DD"
                        },
                        "to_date": {
                            "type": "string",
                            "description": "For 'range': end date YYYY-MM-DD"
                        }
                    }
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: reg::SERVE.into(),
                label: reg::SERVE.into(),
                description: "Manage long-lived background processes (dev servers, watchers, \
                    MCP servers, build daemons). Processes survive bash timeouts and run until \
                    explicitly stopped or session exit.\n\nActions:\n- start: Launch a background \
                    process (command required, name auto-generated)\n- stop: Stop a running \
                    service by name\n- list: Show all managed services with status\n- logs: \
                    Get recent log output from a service\n- check: Check if a service is alive"
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["start", "stop", "list", "logs", "check"],
                            "description": "Action to perform"
                        },
                        "command": {
                            "type": "string",
                            "description": "Command to run (for 'start')"
                        },
                        "name": {
                            "type": "string",
                            "description": "Service name (auto-generated from command if omitted)"
                        },
                        "persist": {
                            "type": "boolean",
                            "description": "If true, service survives session exit (default: false)"
                        },
                        "lines": {
                            "type": "number",
                            "description": "Number of log lines to return (default: 50, for 'logs')"
                        }
                    },
                    "required": ["action"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: reg::TERMINAL.into(),
                label: reg::TERMINAL.into(),
                description: "Manage interactive background terminal sessions. Use this for \
                    long-running commands that need later input or monitoring. Sessions are \
                    created through the same workspace permission checks as bash and are \
                    cleaned up on session exit unless stopped sooner.\n\nActions:\n- start: \
                    Launch an interactive terminal command (command required, name optional)\n- \
                    send: Send stdin to a session\n- read: Read recent output/tail from a session\n- \
                    stop: Stop a session\n- list: Show active sessions"
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["start", "send", "read", "stop", "list"],
                            "description": "Action to perform"
                        },
                        "command": {
                            "type": "string",
                            "description": "Command to run (for 'start')"
                        },
                        "name": {
                            "type": "string",
                            "description": "Human-readable session name"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Terminal session id returned by start"
                        },
                        "input": {
                            "type": "string",
                            "description": "Text to send to stdin (for 'send')"
                        },
                        "newline": {
                            "type": "boolean",
                            "description": "Append a newline to input if missing (default: true)"
                        },
                        "max_bytes": {
                            "type": "number",
                            "description": "Maximum tail bytes to return (for 'read')"
                        },
                        "force": {
                            "type": "boolean",
                            "description": "Force kill instead of graceful terminate (for 'stop')"
                        }
                    },
                    "required": ["action"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: reg::NEX_CAPABILITY.into(),
                label: reg::NEX_CAPABILITY.into(),
                description: "Read-only Nex capability resolver. Checks whether a binary or extension capability is available and recommends host/Nex profile/extension routes without mutating the machine.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["check", "resolve"],
                            "description": "Read-only action to perform"
                        },
                        "capability": {
                            "type": "string",
                            "description": "Capability key such as binary:d2, d2, extension:scratchpad, or omegon-voice"
                        },
                        "profile": {
                            "type": "string",
                            "description": "Optional Nex profile name to include as evidence"
                        }
                    },
                    "required": ["action", "capability"]
                }),
                capabilities: vec![omegon_traits::ToolCapability::RepoInspection],
            },
            // trust_directory is NOT in the tool schema — it's internal harness
            // plumbing called via bus.execute_internal() by the permission
            // dispatch layer. The handler is in CoreTools::execute().
            //
            // NOTE: view, web_search, ask_local_model, list_local_models,
            // manage_ollama, context_status, context_compact, context_clear are
            // provided by their dedicated ToolProvider implementations
            // (ViewProvider, WebSearchProvider, LocalInferenceProvider,
            // ContextProvider) registered separately in setup.rs. Do NOT add
            // them here — duplicates cause Anthropic API 400 "Tool names must
            // be unique" errors.
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
            reg::BASH => {
                let command = args["command"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'command' argument"))?;
                let timeout = args["timeout"].as_u64();

                warn_git_mutation_via_bash(self.repo_model.is_some(), command);

                bash::execute_with_boundary(
                    command,
                    &self.cwd,
                    timeout,
                    cancel,
                    Some(self.boundary.clone()),
                )
                .await
            }
            reg::READ => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let path = self.resolve_path(path_str)?;
                let offset = lenient_usize_arg(&args, "offset");
                let limit = lenient_usize_arg(&args, "limit");
                read::execute(&path, offset, limit).await
            }
            reg::WRITE => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'content' argument"))?;
                let path = self.resolve_path(path_str)?;
                let result = write::execute(&path, content).await;
                if result.is_ok()
                    && let Some(ref model) = self.repo_model
                {
                    model.record_edit(path_str);
                }
                result
            }
            reg::EDIT => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let old_text = args["oldText"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'oldText' argument"))?;
                let new_text = args["newText"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'newText' argument"))?;
                let path = self.resolve_path(path_str)?;
                let result = edit::execute(&path, old_text, new_text).await;
                if result.is_ok()
                    && let Some(ref model) = self.repo_model
                {
                    model.record_edit(path_str);
                }
                result
            }
            reg::CHANGE => {
                let edits_val = args
                    .get("edits")
                    .ok_or_else(|| anyhow::anyhow!("missing 'edits' argument"))?;
                let edits: Vec<change::EditSpec> = serde_json::from_value(edits_val.clone())?;
                let validate_mode = args
                    .get("validate")
                    .and_then(|v| v.as_str())
                    .map(change::ValidationMode::parse)
                    .unwrap_or(change::ValidationMode::None);
                let cwd = self.cwd.clone();
                let cwd2 = cwd.clone();
                let result = change::execute(&edits, validate_mode, &cwd, move |p: &str| {
                    let tools = CoreTools::new(cwd2.clone());
                    tools.resolve_path(p)
                })
                .await;
                // Track all edited files in the working set
                if result.is_ok()
                    && let Some(ref model) = self.repo_model
                {
                    for edit in &edits {
                        model.record_edit(&edit.file);
                    }
                }
                result
            }
            reg::VALIDATE => {
                let path_values = args["paths"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("missing 'paths' argument"))?;
                let mut paths = Vec::new();
                for value in path_values {
                    let path_str = value
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("validation paths must be strings"))?;
                    paths.push(self.resolve_path(path_str)?);
                }
                let level = validate::ValidationLevel::parse(
                    args.get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("standard"),
                );
                validate::execute(&paths, level, &self.cwd).await
            }
            reg::COMMIT => {
                let message = args["message"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'message' argument"))?;

                // ── jj path: describe + new ─────────────────────────────
                // In jj, the working copy is already a mutable change.
                // "Committing" means: describe it, then create a new empty
                // change on top (`jj new`). No staging, no index, no dance.
                if self.repo_model.as_ref().is_some_and(|m| m.is_jj()) {
                    omegon_git::jj::describe(&self.cwd, message)?;
                    omegon_git::jj::new_change(&self.cwd, "")?;
                    omegon_git::jj::sync_to_git_main(&self.cwd)?;

                    // Get the change ID of the just-committed change (parent of @)
                    let committed_id = std::process::Command::new("jj")
                        .args(["log", "-r", "@-", "--no-graph", "-T", "change_id.short()"])
                        .current_dir(&self.cwd)
                        .output()
                        .ok()
                        .and_then(|o| {
                            if o.status.success() {
                                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default();

                    // Refresh model state
                    if let Some(ref model) = self.repo_model {
                        model.clear_working_set();
                        let _ = model.refresh();
                    }

                    let summary = format!("Committed (jj): {committed_id}\n{message}");
                    let branch = git2::Repository::discover(&self.cwd)
                        .ok()
                        .and_then(|r| {
                            r.head()
                                .ok()
                                .and_then(|h| h.shorthand().map(|s| s.to_string()))
                        })
                        .unwrap_or_default();
                    return Ok(ToolResult {
                        content: vec![omegon_traits::ContentBlock::Text { text: summary }],
                        details: json!({
                            "jj_change_id": committed_id,
                            "message": message,
                            "backend": "jj",
                            "git_branch": if branch.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(branch) },
                        }),
                    });
                }

                // ── git path: stage + commit ────────────────────────────
                let paths: Vec<String> = args
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let lifecycle_paths: Vec<String> = self
                    .repo_model
                    .as_ref()
                    .map(|m| m.pending_lifecycle_files().into_iter().collect())
                    .unwrap_or_default();

                let sub_paths = self
                    .repo_model
                    .as_ref()
                    .map(|m| {
                        m.submodules()
                            .into_iter()
                            .map(|s| s.path)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| {
                        omegon_git::submodule::list_submodule_paths(&self.cwd).unwrap_or_default()
                    });

                let mut submodule_commits = 0;
                for sub_path in &sub_paths {
                    let sub_prefix = format!("{}/", sub_path);
                    let touches_sub =
                        paths.is_empty() || paths.iter().any(|p| p.starts_with(&sub_prefix));
                    if touches_sub
                        && let Ok(n) =
                            omegon_git::commit::commit_in_submodule(&self.cwd, sub_path, message)
                    {
                        submodule_commits += n;
                    }
                }

                let include_lifecycle = !lifecycle_paths.is_empty();
                let result = omegon_git::commit::create_commit(
                    &self.cwd,
                    &omegon_git::commit::CommitOptions {
                        message,
                        paths: &paths,
                        include_lifecycle,
                        lifecycle_paths: &lifecycle_paths,
                    },
                )?;

                if let Some(ref model) = self.repo_model {
                    model.clear_working_set();
                    if let Err(e) = model.refresh() {
                        tracing::warn!("failed to refresh repo model after commit: {e}");
                    }
                }

                let mut summary =
                    format!("Committed {} file(s): {}", result.files_staged, result.sha);
                if submodule_commits > 0 {
                    summary.push_str(&format!(
                        "\n({} file(s) committed inside submodule(s) first)",
                        submodule_commits
                    ));
                }
                if include_lifecycle {
                    summary.push_str(&format!(
                        "\n({} lifecycle file(s) included)",
                        lifecycle_paths.len()
                    ));
                }

                Ok(ToolResult {
                    content: vec![omegon_traits::ContentBlock::Text { text: summary }],
                    details: json!({
                        "sha": result.sha,
                        "files_staged": result.files_staged,
                        "submodule_commits": submodule_commits,
                        "lifecycle_files": lifecycle_paths.len(),
                    }),
                })
            }
            reg::PLAN => {
                let action = args["action"].as_str().unwrap_or("status");
                let text = match action {
                    "set" => {
                        let items: Vec<String> = args["items"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if items.is_empty() {
                            "Error: 'items' array required for 'set' action".into()
                        } else {
                            let summary: Vec<String> = items
                                .iter()
                                .enumerate()
                                .map(|(i, s)| format!("{}. {s}", i + 1))
                                .collect();
                            format!(
                                "Work plan set ({} items):\n{}",
                                items.len(),
                                summary.join("\n")
                            )
                        }
                    }
                    "advance" => "Advanced to next work item.".into(),
                    "approve" => {
                        "Plan approved. Mutation-heavy work should wait for execution.".into()
                    }
                    "execute" => "Plan execution started.".into(),
                    "complete" => {
                        let index = args["index"].as_u64().unwrap_or(0) as usize;
                        format!("Marked item {index} complete.")
                    }
                    "skip" => "Skipped current work item.".into(),
                    "clear" => "Cleared the active work plan.".into(),
                    "status" => "Work plan status rendered in context.".into(),
                    "list" => render_lifecycle_plan_list(&self.cwd),
                    other => format!("Unknown plan action: {other}"),
                };
                Ok(ToolResult {
                    content: vec![omegon_traits::ContentBlock::Text { text }],
                    details: Value::Null,
                })
            }
            reg::WAIT_FOR_OPERATOR => {
                let prompt = args["prompt"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("missing 'prompt' argument"))?;
                let timeout_secs = args["timeout"]
                    .as_u64()
                    .unwrap_or(OPERATOR_WAIT_DEFAULT_SECS)
                    .clamp(1, OPERATOR_WAIT_MAX_SECS);

                Err(OperatorWaitRequired {
                    prompt: prompt.to_string(),
                    timeout_secs,
                }
                .into())
            }
            reg::WHOAMI => whoami::execute().await,
            reg::CHRONOS => {
                let sub = args["subcommand"].as_str().unwrap_or("week");
                let expr = args["expression"].as_str();
                let from = args["from_date"].as_str();
                let to = args["to_date"].as_str();
                match chronos::execute(sub, expr, from, to) {
                    Ok(text) => Ok(ToolResult {
                        content: vec![omegon_traits::ContentBlock::Text { text }],
                        details: json!({ "subcommand": sub }),
                    }),
                    Err(e) => anyhow::bail!("{e}"),
                }
            }
            reg::SERVE => {
                let action = args["action"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'action' argument"))?;
                serve::execute(action, &args, &self.cwd).await
            }
            reg::TERMINAL => {
                if !self.terminal_tool_enabled {
                    return Ok(ToolResult {
                        content: vec![omegon_traits::ContentBlock::Text {
                            text: "Terminal tool is disabled by the active profile.".into(),
                        }],
                        details: json!({
                            "is_error": true,
                            "blocked": true,
                            "reason": "terminal_tool_disabled_by_profile",
                        }),
                    });
                }
                let action = args["action"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'action' argument"))?;
                terminal::execute(action, &args, &self.cwd, Some(self.boundary.clone())).await
            }
            reg::NEX_CAPABILITY => {
                let action = args["action"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'action' argument"))?;
                let capability = args["capability"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'capability' argument"))?;
                let profile = args["profile"].as_str().map(str::to_string);
                let resolver = crate::nex::capabilities::CapabilityResolver::bundled()?;
                let context = crate::nex::capabilities::CapabilityContext {
                    path: None,
                    profile,
                };
                let resolution = match action {
                    "check" => resolver.check(capability, context, None),
                    "resolve" => resolver.resolve(capability, context, None),
                    other => anyhow::bail!(
                        "unsupported nex_capability action: {other}; MVP is read-only and supports only check|resolve"
                    ),
                };
                let text = serde_json::to_string_pretty(&resolution)?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details: serde_json::to_value(&resolution)?,
                })
            }
            reg::TRUST_DIRECTORY => {
                let path_str = args["path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing 'path' argument"))?;
                let scope = args["scope"].as_str().unwrap_or("session");
                let expanded = expand_tilde(path_str);
                let canonical = expanded.canonicalize().unwrap_or(expanded.clone());
                self.approve_directory(canonical.clone());
                if matches!(scope, "persistent" | "always" | "project") {
                    let canonical_str = canonical.display().to_string();
                    if let Some(ref settings) = self.boundary.settings
                        && let Ok(mut s) = settings.lock()
                        && !s.trusted_directories.contains(&canonical_str)
                    {
                        s.trusted_directories.push(canonical_str.clone());
                    }
                    let mut profile = crate::settings::Profile::load(&self.cwd);
                    profile.add_trusted_directory(canonical_str);
                    profile.save(&self.cwd)?;
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "✓ Directory approved for {}: {}\n\
                             You can now read and write files in this directory.",
                            if matches!(scope, "persistent" | "always" | "project") {
                                "future sessions"
                            } else {
                                "this session"
                            },
                            canonical.display()
                        ),
                    }],
                    details: serde_json::json!({
                        "path": canonical.display().to_string(),
                        "scope": if matches!(scope, "persistent" | "always" | "project") {
                            "persistent"
                        } else {
                            "session"
                        },
                    }),
                })
            }
            // view, web_search, local_inference tools are handled
            // by their dedicated providers registered in setup.rs.
            _ => anyhow::bail!("Unknown core tool: {tool_name}"),
        }
    }

    /// Sink-aware dispatch. Currently only `bash` actually streams; every
    /// other tool delegates to `execute` (which itself ignores the sink).
    /// As more runners learn to stream they should grow branches here.
    async fn execute_with_sink(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: CancellationToken,
        sink: ToolProgressSink,
    ) -> anyhow::Result<ToolResult> {
        if tool_name == reg::BASH {
            let command = args["command"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing 'command' argument"))?;
            let timeout = args["timeout"].as_u64();

            warn_git_mutation_via_bash(self.repo_model.is_some(), command);

            return bash::execute_streaming(
                command,
                &self.cwd,
                timeout,
                cancel,
                sink,
                Some(self.boundary.clone()),
            )
            .await;
        }

        self.execute(tool_name, call_id, args, cancel).await
    }
}

/// Warn (but don't block) git mutation commands run via bash.
/// The agent should use the structured `commit` tool instead.
fn warn_git_mutation_via_bash(has_repo_model: bool, command: &str) {
    if !has_repo_model {
        return;
    }
    let cmd_lower = command.to_lowercase();
    if cmd_lower.contains("git commit")
        || cmd_lower.contains("git add ")
        || (cmd_lower.contains("git stash") && !cmd_lower.contains("git stash list"))
    {
        tracing::warn!(
            command = command,
            "git mutation via bash — prefer the structured `commit` tool \
             for commits (handles submodules, lifecycle batching, working set)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_plan_projection_projects_openspec_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let change_dir = dir.path().join("openspec/changes/demo");
        std::fs::create_dir_all(&change_dir).unwrap();
        std::fs::write(
            change_dir.join("proposal.md"),
            "# Demo
",
        )
        .unwrap();
        std::fs::write(
            change_dir.join("tasks.md"),
            "## 1. Group

- [x] 1.1 Done task <!-- task-id: stable-done -->
- [ ] 1.2 Pending task
",
        )
        .unwrap();

        let projection = lifecycle_plan_projection(dir.path());

        assert_eq!(projection.entries.len(), 1);
        let entry = &projection.entries[0];
        assert_eq!(entry.plan_id, "openspec:demo");
        assert_eq!(entry.scope, crate::conversation::PlanScope::Repo);
        assert_eq!(entry.source, crate::conversation::PlanSource::OpenSpec);
        assert_eq!(entry.progress.completed, 1);
        assert_eq!(entry.progress.total, 2);

        assert_eq!(projection.tasks.len(), 2);
        assert_eq!(projection.tasks[0].plan_id, "openspec:demo");
        assert_eq!(projection.tasks[0].stable_id, "stable-done");
        assert!(
            projection.tasks[0]
                .revision
                .starts_with("source-v1:openspec:demo:1.1:")
        );
        assert_eq!(projection.tasks[0].source.kind, "openspec");
        assert_eq!(
            projection.tasks[0].source.path.as_deref(),
            Some("openspec/changes/demo/tasks.md")
        );
        assert_eq!(projection.tasks[0].source.anchor.as_deref(), Some("1.1"));
        assert!(projection.tasks[0].supported_mutations.is_empty());
        assert_eq!(
            projection.tasks[0].intent,
            crate::conversation::TaskIntent::Spec
        );
        assert_eq!(
            projection.tasks[0].completion_policy,
            crate::conversation::TaskCompletionPolicy::LifecycleStateReached
        );
        assert!(!projection.tasks[0].writable);
        assert_eq!(
            projection.tasks[0].status,
            crate::conversation::WorkItemStatus::Done
        );
        assert_eq!(
            projection.tasks[1].status,
            crate::conversation::WorkItemStatus::Pending
        );
    }

    #[test]
    fn lifecycle_plan_projection_projects_design_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(
            docs.join("design-node.md"),
            "---
id: plan-node
title: Plan Node
status: exploring
open_questions:
  - What evidence is needed?
---

# Plan Node
",
        )
        .unwrap();

        let projection = lifecycle_plan_projection(dir.path());

        let entry = projection
            .entries
            .iter()
            .find(|entry| entry.plan_id == "design:plan-node")
            .expect("design entry");
        assert_eq!(entry.source, crate::conversation::PlanSource::Design);
        assert_eq!(entry.scope, crate::conversation::PlanScope::Repo);
        assert_eq!(entry.binding.design_node_id.as_deref(), Some("plan-node"));

        let task = projection
            .tasks
            .iter()
            .find(|task| task.plan_id == "design:plan-node")
            .expect("design task");
        assert_eq!(task.intent, crate::conversation::TaskIntent::Design);
        assert_eq!(task.label, "What evidence is needed?");
    }

    #[test]
    fn path_traversal_blocked() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        // Attempting to escape the workspace via ../
        let result = tools.resolve_path("../../../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("PERMISSION REQUIRED"), "error: {err}");
    }

    #[test]
    fn path_within_workspace_allowed() {
        let dir = tempfile::tempdir().unwrap();
        // Create the subdirectory so canonicalize works
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();

        // Use canonical path to match what main.rs does with fs::canonicalize(&cli.cwd)
        let cwd = dir.path().canonicalize().unwrap();
        let tools = CoreTools::new(cwd.clone());
        let result = tools.resolve_path("src/main.rs");
        assert!(result.is_ok(), "error: {:?}", result.unwrap_err());
        assert!(result.unwrap().starts_with(&cwd));
    }

    #[test]
    fn absolute_path_outside_workspace_rejected_by_default() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        let result = tools.resolve_path("/etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("PERMISSION REQUIRED"), "error: {err}");
    }

    #[test]
    fn temp_directory_paths_are_allowed_by_boundary() {
        let tools = CoreTools::new(PathBuf::from("/workspace"));
        let temp_file = std::env::temp_dir()
            .join("omegon-permission-test")
            .join("out.txt");
        let result = tools.resolve_path(temp_file.to_str().unwrap());
        assert!(result.is_ok(), "temp paths should be allowed: {result:?}");
    }

    #[test]
    fn standard_device_streams_are_allowed_by_boundary() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        for path in [
            "/dev/null",
            "/dev/stdin",
            "/dev/stdout",
            "/dev/stderr",
            "/dev/fd/0",
            "/dev/fd/1",
            "/dev/fd/2",
            "/proc/self/fd/0",
            "/proc/self/fd/1",
            "/proc/self/fd/2",
        ] {
            let result = tools.resolve_path(path);
            assert!(result.is_ok(), "{path} should be allowed: {result:?}");
        }
    }

    #[test]
    fn unsafe_device_paths_are_not_allowlisted() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        for path in [
            "/dev/zero",
            "/dev/random",
            "/dev/urandom",
            "/dev/fd/3",
            "/proc/self/fd/3",
        ] {
            let result = tools.resolve_path(path);
            assert!(result.is_err(), "{path} should still require permission");
        }
    }

    #[test]
    fn native_cat_dev_null_is_not_blocked_by_boundary() {
        let boundary = WorkspaceBoundary::new(PathBuf::from("/tmp/workspace"));
        let result = native_cmd::try_dispatch(
            "cat /dev/null",
            Path::new("/tmp/workspace"),
            Some(&boundary),
        )
        .expect("cat should dispatch natively");

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[test]
    fn tilde_expansion_resolves_home_directory() {
        let expanded = expand_tilde("~/Documents/test.md");
        assert!(!expanded.to_str().unwrap().contains('~'));
        assert!(expanded.to_str().unwrap().ends_with("Documents/test.md"));
    }

    #[test]
    fn session_approved_directory_allows_writes() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        // By default, /etc/ is rejected
        assert!(tools.resolve_path("/etc/test.txt").is_err());

        // Approve /etc for this session
        tools.approve_directory(PathBuf::from("/etc"));
        let result = tools.resolve_path("/etc/test.txt");
        assert!(
            result.is_ok(),
            "approved directory should be allowed: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn trusted_directory_from_settings_allows_writes() {
        // Paths must live outside std::env::temp_dir(): everything under the
        // system temp dir is auto-allowed by is_allowed_temp_path (on Linux
        // temp_dir() IS /tmp, which made /tmp-based probes vacuous/wrong).
        let home = dirs::home_dir().expect("home dir");
        let trusted = home.join(format!("omegon-test-trusted-{}", std::process::id()));
        let untrusted = home.join(format!("omegon-test-untrusted-{}", std::process::id()));

        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        if let Ok(mut s) = settings.lock() {
            s.trusted_directories = vec![trusted.display().to_string()];
        }
        let tools = CoreTools::new(PathBuf::from("/nonexistent-workspace")).with_settings(settings);

        // Trusted directory should be allowed
        // Create the directory so canonicalize works
        std::fs::create_dir_all(&trusted).unwrap();
        let result = tools.resolve_path(&trusted.join("eval.md").display().to_string());
        let _ = std::fs::remove_dir_all(&trusted);
        assert!(
            result.is_ok(),
            "trusted directory should be allowed: {:?}",
            result.unwrap_err()
        );

        // A non-trusted sibling outside workspace and temp should be rejected
        let result = tools.resolve_path(&untrusted.join("file.txt").display().to_string());
        assert!(result.is_err());
    }

    #[test]
    fn error_is_typed_permission_error() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        let result = tools.resolve_path("/home/user/obsidian/eval.md");
        let err = result.unwrap_err();
        assert!(
            err.downcast_ref::<PathPermissionError>().is_some(),
            "should return PathPermissionError, got: {err}"
        );
        assert!(
            err.to_string().contains("PERMISSION REQUIRED"),
            "display should contain marker: {err}"
        );
    }

    #[test]
    fn trust_directory_enables_access() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        // Rejected by default
        assert!(tools.resolve_path("/home/user/vault/file.md").is_err());
        // Approve the directory
        tools.approve_directory(PathBuf::from("/home/user/vault"));
        // Now allowed
        let result = tools.resolve_path("/home/user/vault/file.md");
        assert!(
            result.is_ok(),
            "approved directory should be allowed: {:?}",
            result.unwrap_err()
        );
    }

    #[tokio::test]
    async fn trust_directory_persistent_scope_updates_profile_permissions() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("AGENTS.md"), "instructions").unwrap();
        let trusted = tempfile::tempdir().unwrap();
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        let tools = CoreTools::new(project.path().to_path_buf()).with_settings(settings.clone());

        tools
            .execute(
                reg::TRUST_DIRECTORY,
                "test",
                serde_json::json!({
                    "path": trusted.path().display().to_string(),
                    "scope": "persistent",
                }),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let trusted_dir = trusted.path().canonicalize().unwrap().display().to_string();
        assert!(
            settings
                .lock()
                .unwrap()
                .trusted_directories
                .contains(&trusted_dir)
        );
        let profile = crate::settings::Profile::load(project.path());
        assert!(
            profile
                .effective_trusted_directories()
                .contains(&trusted_dir)
        );
        assert!(profile.trusted_directories.is_empty());
        assert_eq!(
            profile.permissions.trusted_directories,
            vec![trusted_dir.clone()]
        );
    }

    #[tokio::test]
    async fn wait_for_operator_returns_typed_wait_request() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        let err = tools
            .execute(
                reg::WAIT_FOR_OPERATOR,
                "test",
                serde_json::json!({
                    "prompt": "Strike the snare once and confirm when the monitor captures it.",
                    "timeout": OPERATOR_WAIT_MAX_SECS + 100,
                }),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        let wait = err
            .downcast_ref::<OperatorWaitRequired>()
            .expect("wait_for_operator should return OperatorWaitRequired");
        assert_eq!(
            wait.prompt,
            "Strike the snare once and confirm when the monitor captures it."
        );
        assert_eq!(wait.timeout_secs, OPERATOR_WAIT_MAX_SECS);
    }

    #[tokio::test]
    async fn terminal_tool_profile_disable_blocks_direct_execution() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-6");
        settings.lock().unwrap().terminal_tool = false;
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace")).with_settings(settings);
        let result = tools
            .execute(
                reg::TERMINAL,
                "terminal-disabled-test",
                serde_json::json!({"action": "list"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(
            result.details.get("blocked").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            result.details.get("reason").and_then(|v| v.as_str()),
            Some("terminal_tool_disabled_by_profile")
        );
    }

    #[test]
    fn lexical_normalize_resolves_dotdot() {
        let result = lexical_normalize(Path::new("/a/b/../c"));
        assert_eq!(result, PathBuf::from("/a/c"));
    }

    #[test]
    fn lexical_normalize_resolves_dot() {
        let result = lexical_normalize(Path::new("/a/./b/./c"));
        assert_eq!(result, PathBuf::from("/a/b/c"));
    }

    #[test]
    fn all_expected_tools_are_registered() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        let all_tools = tools.tools();
        let tool_names: std::collections::HashSet<&str> =
            all_tools.iter().map(|t| t.name.as_str()).collect();

        // Primitive tools
        assert!(tool_names.contains("bash"));
        assert!(tool_names.contains("read"));
        assert!(tool_names.contains("write"));
        assert!(tool_names.contains("edit"));
        assert!(tool_names.contains("validate"));
        assert!(tool_names.contains("change"));

        // Git tools
        assert!(tool_names.contains("commit"));

        // Utility tools
        assert!(tool_names.contains("whoami"));
        assert!(tool_names.contains("chronos"));
        assert!(tool_names.contains("terminal"));
        assert!(tool_names.contains("wait_for_operator"));

        // view, web_search, local_inference tools are provided
        // by dedicated providers, NOT by CoreTools (to avoid duplicates).
        assert!(!tool_names.contains("view"));
        assert!(!tool_names.contains("web_search"));
        assert!(!tool_names.contains("ask_local_model"));
        assert!(!tool_names.contains("list_local_models"));
        assert!(!tool_names.contains("manage_ollama"));

        // 14 registered core tools. trust_directory is internal-only and not in
        // tool_defs; change is registered for harness batching but hidden from
        // the model-facing tool surface by EventBus filtering.
        assert_eq!(
            tool_names.len(),
            14,
            "Expected 14 registered core tools, got {}",
            tool_names.len()
        );
    }

    #[test]
    fn core_tools_do_not_include_provider_tools() {
        // Dedicated providers (ViewProvider, WebSearchProvider, etc.) own these
        // tools. CoreTools must NOT include them to avoid duplicate tool names
        // in the API request.
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        let all_tools = tools.tools();
        let tool_names: std::collections::HashSet<&str> =
            all_tools.iter().map(|t| t.name.as_str()).collect();

        let provider_tools = [
            "view",
            "web_search",
            "ask_local_model",
            "list_local_models",
            "manage_ollama",
        ];
        for name in &provider_tools {
            assert!(
                !tool_names.contains(name),
                "CoreTools should not include '{name}' — it belongs to a dedicated provider"
            );
        }
    }

    #[test]
    fn plan_tool_description_frames_workbench_as_primary_surface() {
        let tools = CoreTools::new(PathBuf::from("/tmp/workspace"));
        let all_tools = tools.tools();
        let plan = all_tools
            .iter()
            .find(|tool| tool.name == crate::tool_registry::core::PLAN)
            .expect("plan tool registered");

        assert!(
            plan.description
                .contains("primary operator-facing Workbench surface")
        );
        assert!(plan.description.contains("agent's guidepost"));
        assert!(
            plan.description
                .contains("keep it truthful before final replies")
        );
    }

    // ── WorkspaceBoundary standalone tests ─────────────────────────────

    #[test]
    fn boundary_blocks_outside_workspace() {
        let b = WorkspaceBoundary::new(PathBuf::from("/tmp/workspace"));
        assert!(b.check_path("/etc/passwd").is_err());
        assert!(b.check_path("/home/user/file.txt").is_err());
    }

    #[test]
    fn boundary_allows_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::write(cwd.join("file.txt"), "test").unwrap();
        let b = WorkspaceBoundary::new(cwd);
        assert!(b.check_path("file.txt").is_ok());
    }

    #[test]
    fn boundary_is_inside_predicate() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        let b = WorkspaceBoundary::new(cwd.clone());
        assert!(b.is_inside_boundary(&cwd.join("sub/file.txt")));
        assert!(!b.is_inside_boundary(Path::new("/etc/passwd")));
    }

    #[test]
    fn boundary_approve_directory_allows_access() {
        let b = WorkspaceBoundary::new(PathBuf::from("/tmp/workspace"));
        assert!(!b.is_inside_boundary(Path::new("/opt/data/file.txt")));
        b.approve_directory(PathBuf::from("/opt/data"));
        assert!(b.is_inside_boundary(Path::new("/opt/data/file.txt")));
    }

    #[test]
    fn boundary_trusted_tmp_allows_missing_child_paths() {
        let b = WorkspaceBoundary::new(PathBuf::from("/tmp/workspace"));
        b.approve_directory(PathBuf::from("/tmp"));
        assert!(b.is_inside_boundary(Path::new("/tmp/omegon-missing-test-file.log")));
        assert!(b.check_path("/tmp/omegon-missing-test-file.log").is_ok());
    }

    #[test]
    fn boundary_clone_shares_approvals() {
        let b = WorkspaceBoundary::new(PathBuf::from("/tmp/workspace"));
        let b2 = b.clone();
        b.approve_directory(PathBuf::from("/opt/shared"));
        // Clone should see the same approval via Arc
        assert!(b2.is_inside_boundary(Path::new("/opt/shared/file.txt")));
    }

    #[test]
    fn lenient_usize_arg_accepts_provider_numeric_variants() {
        let args = serde_json::json!({
            "int": 4237,
            "float": 4237.0,
            "string": "4237",
            "string_float": "4237.0",
            "padded": " 42 ",
            "negative": -5,
            "negative_string": "-5",
            "garbage": "abc",
            "null": null,
            "bool": true,
        });
        assert_eq!(lenient_usize_arg(&args, "int"), Some(4237));
        assert_eq!(lenient_usize_arg(&args, "float"), Some(4237));
        assert_eq!(lenient_usize_arg(&args, "string"), Some(4237));
        assert_eq!(lenient_usize_arg(&args, "string_float"), Some(4237));
        assert_eq!(lenient_usize_arg(&args, "padded"), Some(42));
        assert_eq!(lenient_usize_arg(&args, "negative"), None);
        assert_eq!(lenient_usize_arg(&args, "negative_string"), None);
        assert_eq!(lenient_usize_arg(&args, "garbage"), None);
        assert_eq!(lenient_usize_arg(&args, "null"), None);
        assert_eq!(lenient_usize_arg(&args, "bool"), None);
        assert_eq!(lenient_usize_arg(&args, "missing"), None);
    }
}

pub mod variable_tools;
