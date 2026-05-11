//! Project-local task tree — markdown files with TOML frontmatter in `.omegon/tasks/`.
//!
//! A lightweight, git-tracked task system available to every omegon user.
//! No external dependencies. Tasks are markdown files that can be created,
//! listed, updated, and consumed by sentry as a TaskBoard.

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

const TASKS_DIR: &str = ".omegon/tasks";

// ─── Task Status ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    InProgress,
    Done,
    Blocked,
    Failed,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Todo => "○",
            Self::InProgress => "◐",
            Self::Done => "●",
            Self::Blocked => "✕",
            Self::Failed => "✗",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }
}

// ─── Priority ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl Priority {
    pub fn as_u8(&self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

// ─── Execution Spec ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<String>,
}

// ─── Task ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMeta {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openspec_change: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionSpec>,
}

pub struct Task {
    pub meta: TaskMeta,
    pub body: String,
    pub file_path: PathBuf,
}

// ─── Serialization ──────────────────────────────────────────────────────────

pub fn serialize_task(task: &Task) -> String {
    let mut out = String::from("+++\n");
    out.push_str(&format!("id = \"{}\"\n", task.meta.id));
    out.push_str(&format!("title = \"{}\"\n", task.meta.title.replace('"', "\\\"")));
    out.push_str(&format!("status = \"{}\"\n", task.meta.status.as_str()));
    out.push_str(&format!("priority = \"{}\"\n", serde_json::to_string(&task.meta.priority)
        .unwrap_or_default().trim_matches('"')));

    if let Some(ref parent) = task.meta.parent {
        out.push_str(&format!("parent = \"{parent}\"\n"));
    }
    if !task.meta.depends_on.is_empty() {
        let deps: Vec<String> = task.meta.depends_on.iter().map(|d| format!("\"{d}\"")).collect();
        out.push_str(&format!("depends_on = [{}]\n", deps.join(", ")));
    }
    if !task.meta.tags.is_empty() {
        let tags: Vec<String> = task.meta.tags.iter().map(|t| format!("\"{t}\"")).collect();
        out.push_str(&format!("tags = [{}]\n", tags.join(", ")));
    }
    if let Some(ref node) = task.meta.design_node_id {
        out.push_str(&format!("design_node_id = \"{node}\"\n"));
    }
    if let Some(ref change) = task.meta.openspec_change {
        out.push_str(&format!("openspec_change = \"{change}\"\n"));
    }
    if let Some(ref due) = task.meta.due_date {
        out.push_str(&format!("due_date = \"{due}\"\n"));
    }
    if let Some(ref created) = task.meta.created_at {
        out.push_str(&format!("created_at = \"{}\"\n", created.to_rfc3339()));
    }
    if let Some(ref updated) = task.meta.updated_at {
        out.push_str(&format!("updated_at = \"{}\"\n", updated.to_rfc3339()));
    }

    if let Some(ref exec) = task.meta.execution {
        out.push_str("\n[execution]\n");
        if let Some(ref m) = exec.model { out.push_str(&format!("model = \"{m}\"\n")); }
        if let Some(ref s) = exec.skill { out.push_str(&format!("skill = \"{s}\"\n")); }
        if let Some(t) = exec.max_turns { out.push_str(&format!("max_turns = {t}\n")); }
        if let Some(t) = exec.timeout_secs { out.push_str(&format!("timeout_secs = {t}\n")); }
        if let Some(b) = exec.token_budget { out.push_str(&format!("token_budget = {b}\n")); }
        if let Some(ref c) = exec.cron { out.push_str(&format!("cron = \"{c}\"\n")); }
        if let Some(ref w) = exec.webhook { out.push_str(&format!("webhook = \"{w}\"\n")); }
    }

    out.push_str("+++\n\n");
    out.push_str(&task.body);
    if !task.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn parse_task(content: &str, file_path: PathBuf) -> anyhow::Result<Task> {
    let (frontmatter, body) = split_frontmatter(content)
        .ok_or_else(|| anyhow::anyhow!("missing +++ frontmatter delimiters"))?;

    let meta: TaskMeta = toml::from_str(frontmatter).map_err(|e| {
        anyhow::anyhow!("invalid task frontmatter: {e}")
    })?;

    Ok(Task { meta, body: body.to_string(), file_path })
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("+++") {
        return None;
    }
    let after_open = &trimmed[3..];
    let close = after_open.find("+++")?;
    let frontmatter = after_open[..close].trim();
    let body = after_open[close + 3..].trim_start_matches('\n');
    Some((frontmatter, body))
}

// ─── File Operations ────────────────────────────────────────────────────────

pub fn tasks_dir(cwd: &Path) -> PathBuf {
    cwd.join(TASKS_DIR)
}

pub fn task_path(cwd: &Path, id: &str) -> PathBuf {
    tasks_dir(cwd).join(format!("{id}.md"))
}

pub fn list_tasks(cwd: &Path) -> anyhow::Result<Vec<Task>> {
    let dir = tasks_dir(cwd);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let content = std::fs::read_to_string(&path)?;
            match parse_task(&content, path.clone()) {
                Ok(task) => tasks.push(task),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "skipping invalid task file");
                }
            }
        }
    }

    tasks.sort_by(|a, b| {
        b.meta.priority.as_u8().cmp(&a.meta.priority.as_u8())
            .then_with(|| a.meta.title.cmp(&b.meta.title))
    });

    Ok(tasks)
}

pub fn get_task(cwd: &Path, id: &str) -> anyhow::Result<Task> {
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        anyhow::bail!("invalid task id: path traversal rejected");
    }
    let path = task_path(cwd, id);
    let content = std::fs::read_to_string(&path).map_err(|e| {
        anyhow::anyhow!("task '{id}' not found: {e}")
    })?;
    parse_task(&content, path)
}

pub fn save_task(cwd: &Path, task: &Task) -> anyhow::Result<PathBuf> {
    let dir = tasks_dir(cwd);
    std::fs::create_dir_all(&dir)?;
    let path = task_path(cwd, &task.meta.id);
    let content = serialize_task(task);
    std::fs::write(&path, &content)?;
    Ok(path)
}

pub fn create_task(cwd: &Path, title: &str, body: &str) -> anyhow::Result<Task> {
    let slug: String = title.to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if slug.is_empty() || slug.chars().all(|c| c == '-') {
        anyhow::bail!(
            "invalid task title '{title}' — produces empty or invalid slug. \
             Title must contain at least one ASCII alphanumeric character."
        );
    }

    let path = task_path(cwd, &slug);
    if path.exists() {
        anyhow::bail!("task '{slug}' already exists");
    }

    let now = Utc::now();
    let task = Task {
        meta: TaskMeta {
            id: slug,
            title: title.to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            parent: None,
            depends_on: Vec::new(),
            tags: Vec::new(),
            design_node_id: None,
            openspec_change: None,
            due_date: None,
            created_at: Some(now),
            updated_at: Some(now),
            execution: None,
        },
        body: body.to_string(),
        file_path: path.clone(),
    };

    save_task(cwd, &task)?;
    Ok(task)
}

pub fn update_status(cwd: &Path, id: &str, status: TaskStatus) -> anyhow::Result<Task> {
    let mut task = get_task(cwd, id)?;
    task.meta.status = status;
    task.meta.updated_at = Some(Utc::now());
    save_task(cwd, &task)?;
    Ok(task)
}

pub fn delete_task(cwd: &Path, id: &str) -> anyhow::Result<()> {
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        anyhow::bail!("invalid task id: path traversal rejected");
    }
    let path = task_path(cwd, id);
    if !path.exists() {
        anyhow::bail!("task '{id}' not found");
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

pub fn actionable_tasks(cwd: &Path) -> anyhow::Result<Vec<Task>> {
    let all = list_tasks(cwd)?;
    let done_ids: std::collections::HashSet<String> = all.iter()
        .filter(|t| t.meta.status.is_terminal())
        .map(|t| t.meta.id.clone())
        .collect();

    Ok(all.into_iter().filter(|t| {
        if t.meta.status.is_terminal() { return false; }
        if matches!(t.meta.status, TaskStatus::Blocked) { return false; }
        t.meta.depends_on.iter().all(|dep| done_ids.contains(dep))
    }).collect())
}

// ─── CLI Display ────────────────────────────────────────────────────────────

pub fn cmd_list(cwd: &Path) -> anyhow::Result<()> {
    let tasks = list_tasks(cwd)?;
    if tasks.is_empty() {
        println!("No tasks. Create one with: omegon task create \"<title>\"");
        return Ok(());
    }

    for task in &tasks {
        let status = task.meta.status;
        let pri = match task.meta.priority {
            Priority::Critical => " [CRIT]",
            Priority::High => " [HIGH]",
            Priority::Low => " [low]",
            _ => "",
        };
        let deps = if task.meta.depends_on.is_empty() {
            String::new()
        } else {
            format!(" (depends: {})", task.meta.depends_on.join(", "))
        };
        let node = task.meta.design_node_id.as_ref()
            .map(|n| format!(" -> {n}"))
            .unwrap_or_default();

        println!("  {} {}{}{}{} — {}", status.icon(), task.meta.id, pri, deps, node, task.meta.title);
    }

    let done = tasks.iter().filter(|t| t.meta.status == TaskStatus::Done).count();
    let total = tasks.len();
    println!("\n  {done}/{total} done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let task = Task {
            meta: TaskMeta {
                id: "auth-rewrite".into(),
                title: "Rewrite authentication handler".into(),
                status: TaskStatus::InProgress,
                priority: Priority::High,
                parent: None,
                depends_on: vec!["schema-migration".into()],
                tags: vec!["security".into(), "backend".into()],
                design_node_id: Some("auth-node-2026".into()),
                openspec_change: Some("auth-rewrite".into()),
                due_date: Some(NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()),
                created_at: None,
                updated_at: None,
                execution: Some(ExecutionSpec {
                    model: Some("anthropic:claude-sonnet-4-6".into()),
                    max_turns: Some(50),
                    cron: Some("0 9 * * 1-5".into()),
                    ..Default::default()
                }),
            },
            body: "Implement the new auth handler using JWT tokens.\n\nAcceptance criteria:\n- All existing tests pass\n- New integration tests added\n".into(),
            file_path: PathBuf::from("test.md"),
        };

        let serialized = serialize_task(&task);
        let parsed = parse_task(&serialized, PathBuf::from("test.md")).unwrap();

        assert_eq!(parsed.meta.id, "auth-rewrite");
        assert_eq!(parsed.meta.title, "Rewrite authentication handler");
        assert_eq!(parsed.meta.status, TaskStatus::InProgress);
        assert_eq!(parsed.meta.priority, Priority::High);
        assert_eq!(parsed.meta.depends_on, vec!["schema-migration"]);
        assert_eq!(parsed.meta.tags, vec!["security", "backend"]);
        assert_eq!(parsed.meta.design_node_id.as_deref(), Some("auth-node-2026"));
        assert_eq!(parsed.meta.openspec_change.as_deref(), Some("auth-rewrite"));
        assert_eq!(parsed.meta.due_date, Some(NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()));
        assert_eq!(parsed.meta.execution.as_ref().unwrap().model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(parsed.meta.execution.as_ref().unwrap().max_turns, Some(50));
        assert_eq!(parsed.meta.execution.as_ref().unwrap().cron.as_deref(), Some("0 9 * * 1-5"));
        assert!(parsed.body.contains("JWT tokens"));
    }

    #[test]
    fn minimal_task() {
        let content = r#"+++
id = "quick-fix"
title = "Fix the bug"
+++

Just fix it.
"#;
        let task = parse_task(content, PathBuf::from("test.md")).unwrap();
        assert_eq!(task.meta.id, "quick-fix");
        assert_eq!(task.meta.status, TaskStatus::Todo);
        assert_eq!(task.meta.priority, Priority::Medium);
        assert!(task.meta.depends_on.is_empty());
    }

    #[test]
    fn missing_frontmatter_fails() {
        let content = "# No frontmatter\nJust a markdown file.\n";
        assert!(parse_task(content, PathBuf::from("test.md")).is_err());
    }

    #[test]
    fn create_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        create_task(cwd, "First task", "Do the first thing").unwrap();
        create_task(cwd, "Second task", "Do the second thing").unwrap();

        let tasks = list_tasks(cwd).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn create_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        create_task(cwd, "My task", "Body").unwrap();
        assert!(create_task(cwd, "My task", "Body").is_err());
    }

    #[test]
    fn update_status_persists() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        create_task(cwd, "Test task", "Body").unwrap();
        update_status(cwd, "test-task", TaskStatus::Done).unwrap();

        let task = get_task(cwd, "test-task").unwrap();
        assert_eq!(task.meta.status, TaskStatus::Done);
    }

    #[test]
    fn delete_task_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        create_task(cwd, "Doomed task", "Bye").unwrap();
        assert!(task_path(cwd, "doomed-task").exists());
        delete_task(cwd, "doomed-task").unwrap();
        assert!(!task_path(cwd, "doomed-task").exists());
    }

    #[test]
    fn actionable_filters_blocked_and_done() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        create_task(cwd, "Ready", "Go").unwrap();
        create_task(cwd, "Also ready", "Go").unwrap();
        create_task(cwd, "Blocked one", "Wait").unwrap();
        update_status(cwd, "blocked-one", TaskStatus::Blocked).unwrap();
        create_task(cwd, "Done one", "Finished").unwrap();
        update_status(cwd, "done-one", TaskStatus::Done).unwrap();

        let actionable = actionable_tasks(cwd).unwrap();
        assert_eq!(actionable.len(), 2);
    }

    #[test]
    fn dependency_gating() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        create_task(cwd, "Prereq", "First").unwrap();

        let mut blocked = create_task(cwd, "Gated", "Second").unwrap();
        blocked.meta.depends_on = vec!["prereq".into()];
        save_task(cwd, &blocked).unwrap();

        // Gated task not actionable because prereq is Todo
        let actionable = actionable_tasks(cwd).unwrap();
        let ids: Vec<&str> = actionable.iter().map(|t| t.meta.id.as_str()).collect();
        assert!(ids.contains(&"prereq"));
        assert!(!ids.contains(&"gated"));

        // Complete prereq
        update_status(cwd, "prereq", TaskStatus::Done).unwrap();

        // Now gated is actionable
        let actionable = actionable_tasks(cwd).unwrap();
        let ids: Vec<&str> = actionable.iter().map(|t| t.meta.id.as_str()).collect();
        assert!(ids.contains(&"gated"));
    }

    #[test]
    fn path_traversal_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(get_task(dir.path(), "../etc/passwd").is_err());
        assert!(delete_task(dir.path(), "../../bad").is_err());
    }

    #[test]
    fn status_icons() {
        assert_eq!(TaskStatus::Todo.icon(), "○");
        assert_eq!(TaskStatus::Done.icon(), "●");
        assert!(!TaskStatus::Todo.is_terminal());
        assert!(TaskStatus::Done.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
    }
}
