//! [`TaskBoard`] backed by a flynt vault.
//!
//! Reads tasks from the flynt sqlite (default
//! `<vault>/.flynt-local/flynt/flynt-index.db`) and surfaces sentry-
//! managed ones as [`SentryTask`]s. Enables an omegon launched into a
//! flynt vault — including ACP from Zed — to run autonomous tasks
//! against the operator's kanban without a parallel `sentry.toml`.
//!
//! ## Selection criteria
//!
//! Tasks shown to the executor are those that:
//! - have status `"todo"` (kanban Backlog/Scheduled, not Running/Done/
//!   Archived/Failed),
//! - sit in the `"Scheduled"` column (the kanban convention for "ready
//!   for the agent to pick up next", per Phase 0's column rename),
//! - are sentry-managed: carry an execution block with at least one
//!   meaningful field, or carry a `cron:` / `webhook:` external_ref.
//!
//! ## State + lifecycle
//!
//! Claim contention goes through omegon's [`StateDb`] (same pattern as
//! [`super::FileTaskBoard`]). On claim/complete/fail we also mutate
//! the task's column on the kanban side so the operator sees live
//! state without a separate "running tasks" UI:
//!
//! - `Scheduled → Running` on claim
//! - `Running → Done` on complete
//! - `Running → Failed` on fail
//! - `Running → Scheduled` on release (no terminal state)
//!
//! Schema knowledge of the flynt sqlite is inlined here. We don't take
//! a dep on `flynt-store` because that crate pulls in comrak / notify
//! / git2; only `flynt-models` is needed for the [`Task`] type. If
//! flynt's schema drifts, this board falls back to skipping rows it
//! can't deserialize rather than failing the whole list.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use flynt_models::task::{ExecutionSpec, Task};
use rusqlite::{Connection, params};

use super::board::TaskBoard;
use super::file_board::uuid_v4;
use super::state_db::StateDb;
use super::types::{SentryTask, TaskError, TaskResult, TaskSpec, Trigger};

const SCHEDULED: &str = "Scheduled";
const RUNNING: &str = "Running";
const DONE: &str = "Done";
const FAILED: &str = "Failed";

pub struct FlyntTaskBoard {
    /// Vault root (the directory the operator opens in flynt-app).
    vault_root: PathBuf,
    /// Connection to the flynt sqlite. We hold this open + behind a
    /// mutex; flynt's own Vault uses the same locking shape.
    conn: Mutex<Connection>,
    state_db: Arc<StateDb>,
    instance_id: String,
}

impl FlyntTaskBoard {
    /// Open a board pointed at `vault_root`. Resolves the sqlite path
    /// from the same convention flynt-store uses by default
    /// (`<root>/.flynt-local/flynt/flynt-index.db`); pass an explicit
    /// `db_path` if your vault uses a custom location.
    pub fn open(
        vault_root: PathBuf,
        state_db: Arc<StateDb>,
        instance_id: String,
    ) -> anyhow::Result<Self> {
        let db_path = default_db_path(&vault_root);
        Self::open_with_db(vault_root, db_path, state_db, instance_id)
    }

    pub fn open_with_db(
        vault_root: PathBuf,
        db_path: PathBuf,
        state_db: Arc<StateDb>,
        instance_id: String,
    ) -> anyhow::Result<Self> {
        if !db_path.exists() {
            anyhow::bail!(
                "flynt vault sqlite not found at {} — open the vault in flynt-app first",
                db_path.display()
            );
        }
        let conn = Connection::open(&db_path).map_err(|e| {
            anyhow::anyhow!("open flynt vault sqlite at {}: {e}", db_path.display())
        })?;
        Ok(Self {
            vault_root,
            conn: Mutex::new(conn),
            state_db,
            instance_id,
        })
    }

    pub fn vault_root(&self) -> &Path { &self.vault_root }

    /// Load one task by id (UUID string). Returns Err if it can't be
    /// parsed; returns Ok(None) if no row matches.
    fn load_task(&self, id: &str) -> anyhow::Result<Option<Task>> {
        let id_uuid = uuid::Uuid::parse_str(id)
            .map_err(|_| anyhow::anyhow!("task id is not a UUID: {id}"))?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("conn lock: {e}"))?;
        let mut stmt = conn.prepare(SELECT_TASK_BY_ID)?;
        let mut rows = stmt.query(params![id_uuid.to_string()])?;
        let Some(row) = rows.next()? else { return Ok(None) };
        Ok(Some(row_to_task(row)?))
    }

    /// Update a single column on a single task. Used by claim /
    /// complete / fail / release to reflect lifecycle on the kanban.
    fn set_column(&self, id: &str, column: &str) -> anyhow::Result<()> {
        let id_uuid = uuid::Uuid::parse_str(id)
            .map_err(|_| anyhow::anyhow!("task id is not a UUID: {id}"))?;
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("conn lock: {e}"))?;
        conn.execute(
            "UPDATE tasks SET column_name = ?1, updated_at = ?2 WHERE id = ?3",
            params![column, Utc::now().to_rfc3339(), id_uuid.to_string()],
        )?;
        Ok(())
    }
}

impl TaskBoard for FlyntTaskBoard {
    fn list_actionable(&self) -> anyhow::Result<Vec<SentryTask>> {
        // Scope the conn / stmt / rows borrow chain so it drops before
        // we re-borrow self via state_db.last_run() below. Decode is
        // forgiving — schema-ahead flynt vaults skip unparseable rows
        // rather than failing the whole list.
        let tasks: Vec<Task> = {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("conn lock: {e}"))?;
            let mut stmt = conn.prepare(SELECT_ACTIONABLE)?;
            let mut rows = stmt.query(params![SCHEDULED])?;
            let mut acc: Vec<Task> = Vec::new();
            while let Some(row) = rows.next()? {
                match row_to_task(row) {
                    Ok(t) => acc.push(t),
                    Err(e) => tracing::warn!(error = %e, "skipping flynt task row that failed to deserialize"),
                }
            }
            acc
        };

        let mut out = Vec::with_capacity(tasks.len());
        for t in tasks {
            if !t.is_sentry_managed() { continue; }
            let id = t.id.0.to_string();
            let (last_run, run_count) = self.state_db.last_run(&id)?
                .map(|(dt, c)| (Some(dt), c))
                .unwrap_or((None, 0));
            let triggers = collect_triggers(&t);
            out.push(SentryTask {
                id,
                name: t.title.clone(),
                priority: priority_to_u8(t.priority),
                triggers,
                last_run,
                run_count,
            });
        }
        Ok(out)
    }

    fn claim(&self, task_id: &str) -> anyhow::Result<bool> {
        let claimed = self.state_db.claim_task(task_id, &self.instance_id)?;
        if claimed {
            // Best-effort column move. Failure here is logged but does
            // not invalidate the claim — execution proceeds; on next
            // sync the column will re-converge.
            if let Err(e) = self.set_column(task_id, RUNNING) {
                tracing::warn!(task = task_id, error = %e, "could not set column to Running");
            }
        }
        Ok(claimed)
    }

    fn release(&self, task_id: &str) -> anyhow::Result<()> {
        self.state_db.release_task(task_id)?;
        if let Err(e) = self.set_column(task_id, SCHEDULED) {
            tracing::warn!(task = task_id, error = %e, "could not reset column to Scheduled on release");
        }
        Ok(())
    }

    fn complete(&self, task_id: &str, result: &TaskResult) -> anyhow::Result<()> {
        let run_id = format!("{task_id}-{}", uuid_v4());
        self.state_db.record_run_start(&run_id, task_id)?;
        self.state_db.record_run_complete(&run_id, result)?;
        self.state_db.release_task(task_id)?;
        if let Err(e) = self.set_column(task_id, DONE) {
            tracing::warn!(task = task_id, error = %e, "could not set column to Done");
        }
        Ok(())
    }

    fn fail(&self, task_id: &str, error: &TaskError) -> anyhow::Result<()> {
        let run_id = format!("{task_id}-{}", uuid_v4());
        self.state_db.record_run_start(&run_id, task_id)?;
        self.state_db.record_run_failure(&run_id, error)?;
        self.state_db.release_task(task_id)?;
        if let Err(e) = self.set_column(task_id, FAILED) {
            tracing::warn!(task = task_id, error = %e, "could not set column to Failed");
        }
        Ok(())
    }

    fn task_spec(&self, task_id: &str) -> anyhow::Result<TaskSpec> {
        let task = self.load_task(task_id)?
            .ok_or_else(|| anyhow::anyhow!("flynt task '{task_id}' not found"))?;
        let exec = task.execution.clone().unwrap_or_default();
        Ok(TaskSpec {
            // Description is the canonical agent prompt; fall back to
            // title if the operator left it blank so sentry never sees
            // an empty prompt.
            prompt: if task.description.trim().is_empty() {
                task.title.clone()
            } else {
                task.description.clone()
            },
            model: exec.model.clone(),
            skill: exec.skill.clone(),
            max_turns: exec.max_turns,
            timeout_secs: exec.timeout_secs,
            token_budget: exec.token_budget,
            cwd: exec.cwd.clone(),
            env: exec.env.clone().into_iter().collect(),
            design_node_id: task.design_node_id.map(|u| u.to_string()),
            openspec_change: task.openspec_change.clone(),
        })
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

pub fn default_db_path(vault_root: &Path) -> PathBuf {
    vault_root.join(".flynt-local").join("flynt").join("flynt-index.db")
}

/// Lightweight probe — does this directory look like a flynt vault?
/// Used by the omegon main loop to auto-select FlyntTaskBoard when
/// launched into a flynt directory (for example, by Zed via ACP).
pub fn is_flynt_vault(root: &Path) -> bool {
    root.join(".flynt").join("config.toml").exists()
        || default_db_path(root).exists()
}

fn priority_to_u8(p: flynt_models::task::Priority) -> u8 {
    use flynt_models::task::Priority::*;
    match p { Low => 0, Medium => 1, High => 2, Critical => 3 }
}

fn collect_triggers(t: &Task) -> Vec<Trigger> {
    let mut triggers = Vec::new();
    if let Some(cron) = t.cron_trigger() {
        triggers.push(Trigger::Cron { schedule: cron.to_string() });
    }
    if let Some(name) = t.webhook_trigger() {
        triggers.push(Trigger::Webhook { name: name.to_string() });
    }
    if triggers.is_empty() {
        triggers.push(Trigger::Manual);
    }
    triggers
}

// SELECT clauses kept inline so this file is the only place we encode
// flynt schema knowledge. If flynt's migration sequence adds new
// columns, queries here keep working — they pin to the columns we
// need by name. The `unwrap_or_default()` arms in `row_to_task`
// tolerate older schemas missing newer fields.
//
// Status comparison is against the JSON-encoded form `"todo"` (with
// embedded quotes) because that's exactly the shape flynt-store's
// save_task writes via serde_json::to_string(&status).

const SELECT_TASK_BY_ID: &str = "SELECT \
    id, board_id, column_name, title, description, priority, status, tags, \
    document_refs, due_date, position, created_at, updated_at, decay, \
    last_touched_at, external_refs, design_node_id, execution, openspec_change, \
    engagement_id \
    FROM tasks WHERE id = ?1";

const SELECT_ACTIONABLE: &str = "SELECT \
    id, board_id, column_name, title, description, priority, status, tags, \
    document_refs, due_date, position, created_at, updated_at, decay, \
    last_touched_at, external_refs, design_node_id, execution, openspec_change, \
    engagement_id \
    FROM tasks WHERE column_name = ?1 AND status = '\"todo\"'";

fn row_to_task(row: &rusqlite::Row<'_>) -> anyhow::Result<Task> {
    use flynt_models::task::{BoardId, DecayRate, DocumentId, Priority, TaskId, TaskStatus};

    let id: String = row.get(0)?;
    let board_id: String = row.get(1)?;
    let column_name: String = row.get(2)?;
    let title: String = row.get(3)?;
    let description: String = row.get(4)?;
    let priority_json: String = row.get(5)?;
    let status_json: String = row.get(6)?;
    let tags_json: String = row.get(7)?;
    let refs_json: String = row.get(8)?;
    let due: Option<String> = row.get(9)?;
    let position: u32 = row.get(10)?;
    let created_at: String = row.get(11)?;
    let updated_at: String = row.get(12)?;
    let decay_json: Option<String> = row.get(13)?;
    let last_touched: Option<String> = row.get(14)?;
    let external_refs_json: String = row.get(15)?;
    let design_node_id_str: Option<String> = row.get(16)?;
    let execution_json: Option<String> = row.get(17)?;
    let openspec_change: Option<String> = row.get(18)?;
    let engagement_id_str: Option<String> = row.get(19)?;

    Ok(Task {
        id: TaskId(uuid::Uuid::parse_str(&id)?),
        board_id: BoardId(uuid::Uuid::parse_str(&board_id)?),
        column: column_name,
        title,
        description,
        priority: serde_json::from_str(&priority_json).unwrap_or(Priority::Medium),
        status: serde_json::from_str(&status_json).unwrap_or(TaskStatus::Todo),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        document_refs: serde_json::from_str::<Vec<DocumentId>>(&refs_json).unwrap_or_default(),
        external_refs: serde_json::from_str(&external_refs_json).unwrap_or_default(),
        due_date: due.and_then(|s| s.parse().ok()),
        position,
        created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
        updated_at: updated_at.parse().unwrap_or_else(|_| Utc::now()),
        decay: decay_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(DecayRate::Natural),
        last_touched_at: last_touched.and_then(|s| s.parse().ok()),
        design_node_id: design_node_id_str
            .and_then(|s| uuid::Uuid::parse_str(&s).ok()),
        openspec_change,
        engagement_id: engagement_id_str
            .and_then(|s| uuid::Uuid::parse_str(&s).ok())
            .map(flynt_models::engagement::EngagementId),
        execution: execution_json
            .and_then(|s| serde_json::from_str::<ExecutionSpec>(&s).ok())
            .filter(|e| !e.is_empty()),
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use flynt_models::task::{Priority, Task as FlyntTask, TaskId};
    use tempfile::TempDir;

    /// Build a minimal flynt-style sqlite that matches the columns
    /// FlyntTaskBoard SELECTs. We don't pull in flynt-store for this
    /// — the schema we exercise is the surface-level subset
    /// FlyntTaskBoard reads.
    fn fresh_flynt_db() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let db_path = default_db_path(tmp.path());
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(r#"
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                board_id TEXT NOT NULL,
                column_name TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                priority TEXT NOT NULL DEFAULT '"medium"',
                status TEXT NOT NULL DEFAULT '"todo"',
                tags TEXT NOT NULL DEFAULT '[]',
                document_refs TEXT NOT NULL DEFAULT '[]',
                due_date TEXT,
                position INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                decay TEXT NOT NULL DEFAULT '"natural"',
                last_touched_at TEXT,
                external_refs TEXT NOT NULL DEFAULT '[]',
                design_node_id TEXT,
                execution TEXT,
                openspec_change TEXT,
                engagement_id TEXT
            );
        "#).unwrap();
        (tmp, db_path)
    }

    fn insert_task(db: &Path, t: &FlyntTask) {
        let conn = Connection::open(db).unwrap();
        let exec_json = t.execution.as_ref().map(|e| serde_json::to_string(e).unwrap());
        conn.execute(
            "INSERT INTO tasks (id, board_id, column_name, title, description, priority, status, tags, document_refs, due_date, position, created_at, updated_at, decay, last_touched_at, external_refs, design_node_id, execution, openspec_change, engagement_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            params![
                t.id.0.to_string(),
                t.board_id.0.to_string(),
                t.column,
                t.title,
                t.description,
                serde_json::to_string(&t.priority).unwrap(),
                serde_json::to_string(&t.status).unwrap(),
                serde_json::to_string(&t.tags).unwrap(),
                serde_json::to_string(&t.document_refs).unwrap(),
                t.due_date.map(|d| d.to_string()),
                t.position,
                t.created_at.to_rfc3339(),
                t.updated_at.to_rfc3339(),
                serde_json::to_string(&t.decay).unwrap(),
                t.last_touched_at.map(|t| t.to_rfc3339()),
                serde_json::to_string(&t.external_refs).unwrap(),
                t.design_node_id.map(|u| u.to_string()),
                exec_json,
                t.openspec_change.clone(),
                t.engagement_id.as_ref().map(|e| e.0.to_string()),
            ],
        ).unwrap();
    }

    fn fixture() -> (TempDir, FlyntTaskBoard, flynt_models::task::BoardId) {
        let (tmp, db_path) = fresh_flynt_db();
        let board_id = flynt_models::task::BoardId(uuid::Uuid::new_v4());
        let state = Arc::new(StateDb::in_memory().unwrap());
        let board = FlyntTaskBoard::open_with_db(
            tmp.path().to_path_buf(),
            db_path,
            state,
            "test-instance".into(),
        ).unwrap();
        (tmp, board, board_id)
    }

    #[test]
    fn open_fails_when_db_missing() {
        let tmp = TempDir::new().unwrap();
        let state = Arc::new(StateDb::in_memory().unwrap());
        let result = FlyntTaskBoard::open(
            tmp.path().to_path_buf(),
            state,
            "test".into(),
        );
        // Unwrap manually since FlyntTaskBoard isn't Debug (Connection
        // isn't either).
        match result {
            Ok(_) => panic!("open should fail when db is missing"),
            Err(e) => assert!(e.to_string().contains("not found"), "got: {e}"),
        }
    }

    #[test]
    fn list_actionable_returns_only_scheduled_sentry_tasks() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;

        // Sentry-managed (cron in external_refs), in Scheduled column → included.
        let mut t1 = FlyntTask::new(board_id.clone(), "Scheduled", "PR review");
        t1.external_refs = vec!["cron:0 */4 * * *".into()];
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t1);

        // Sentry-managed but not in Scheduled column → excluded (Backlog).
        let mut t2 = FlyntTask::new(board_id.clone(), "Backlog", "Idea");
        t2.external_refs = vec!["cron:* * * * *".into()];
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t2);

        // In Scheduled but NOT sentry-managed → excluded.
        let t3 = FlyntTask::new(board_id.clone(), "Scheduled", "Manual task");
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t3);

        let listed = board.list_actionable().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "PR review");
        assert!(matches!(listed[0].triggers[0], Trigger::Cron { .. }));
    }

    #[test]
    fn claim_moves_task_to_running_column() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;
        let mut t = FlyntTask::new(board_id.clone(), "Scheduled", "Auto");
        t.external_refs = vec!["webhook:gh".into()];
        let id = t.id.0.to_string();
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t);

        assert!(board.claim(&id).unwrap());
        let after = board.load_task(&id).unwrap().unwrap();
        assert_eq!(after.column, RUNNING);

        // Re-claim under same instance → still false (already locked).
        assert!(!board.claim(&id).unwrap());
    }

    #[test]
    fn complete_moves_task_to_done() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;
        let mut t = FlyntTask::new(board_id.clone(), "Scheduled", "Auto");
        t.external_refs = vec!["webhook:gh".into()];
        let id = t.id.0.to_string();
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t);

        board.claim(&id).unwrap();
        let result = TaskResult {
            exit_code: 0, summary: "ok".into(), tokens_used: 100,
            duration_secs: 5, session_id: "s1".into(),
        };
        board.complete(&id, &result).unwrap();
        let after = board.load_task(&id).unwrap().unwrap();
        assert_eq!(after.column, DONE);
    }

    #[test]
    fn fail_moves_task_to_failed_column() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;
        let mut t = FlyntTask::new(board_id.clone(), "Scheduled", "Auto");
        t.external_refs = vec!["webhook:gh".into()];
        let id = t.id.0.to_string();
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t);

        board.claim(&id).unwrap();
        let err = TaskError { message: "boom".into(), retriable: true, attempt: 1 };
        board.fail(&id, &err).unwrap();
        let after = board.load_task(&id).unwrap().unwrap();
        assert_eq!(after.column, FAILED);
    }

    #[test]
    fn release_resets_column_to_scheduled() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;
        let mut t = FlyntTask::new(board_id.clone(), "Scheduled", "Auto");
        t.external_refs = vec!["webhook:gh".into()];
        let id = t.id.0.to_string();
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t);

        board.claim(&id).unwrap();
        board.release(&id).unwrap();
        let after = board.load_task(&id).unwrap().unwrap();
        assert_eq!(after.column, SCHEDULED);
    }

    #[test]
    fn task_spec_pulls_execution_block_into_spec() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;
        let mut t = FlyntTask::new(board_id.clone(), "Scheduled", "Auto");
        t.description = "Walk the repo and propose changes.".into();
        t.priority = Priority::High;
        t.execution = Some(ExecutionSpec {
            model: Some("anthropic:claude-sonnet-4-6".into()),
            max_turns: Some(20),
            ..Default::default()
        });
        t.openspec_change = Some("feature-x".into());
        let id = t.id.0.to_string();
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t);

        let spec = board.task_spec(&id).unwrap();
        assert_eq!(spec.prompt, "Walk the repo and propose changes.");
        assert_eq!(spec.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
        assert_eq!(spec.max_turns, Some(20));
        assert_eq!(spec.openspec_change.as_deref(), Some("feature-x"));
    }

    #[test]
    fn task_spec_falls_back_to_title_when_description_blank() {
        let (tmp, board, board_id) = fixture();
        let _ = tmp;
        let mut t = FlyntTask::new(board_id.clone(), "Scheduled", "Just a title");
        t.external_refs = vec!["webhook:gh".into()];
        let id = t.id.0.to_string();
        insert_task(board.vault_root().join(".flynt-local/flynt/flynt-index.db").as_path(), &t);

        let spec = board.task_spec(&id).unwrap();
        assert_eq!(spec.prompt, "Just a title");
    }

    #[test]
    fn task_spec_returns_err_for_unknown_id() {
        let (tmp, board, _) = fixture();
        let _ = tmp;
        let phantom = TaskId::new().0.to_string();
        assert!(board.task_spec(&phantom).is_err());
    }

    #[test]
    fn is_flynt_vault_detects_marker() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_flynt_vault(tmp.path()));
        std::fs::create_dir_all(tmp.path().join(".flynt")).unwrap();
        std::fs::write(tmp.path().join(".flynt/config.toml"), "vault_name = \"test\"").unwrap();
        assert!(is_flynt_vault(tmp.path()));
    }
}
