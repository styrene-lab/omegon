//! Cross-module integration tests for the sentry subsystem.
//!
//! These tests exercise board discovery, task lifecycle, trigger dispatch,
//! and the FlyntTaskBoard integration against real sqlite databases in
//! temporary directories. No LLM calls — tests validate the orchestration
//! layer (board → executor → state) in isolation.

use std::sync::Arc;
use std::path::Path;

use chrono::Utc;
use rusqlite::{Connection, params};

use super::board::TaskBoard;
use super::file_board::FileTaskBoard;
use super::flynt_board::{FlyntTaskBoard, default_db_path, is_flynt_vault};
use super::state_db::StateDb;
use super::tree_board::TaskTreeBoard;
use super::types::*;
use super::load_config;

fn create_flynt_db(vault_root: &Path) -> std::path::PathBuf {
    let db_path = default_db_path(vault_root);
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            board_id TEXT NOT NULL,
            column_name TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            priority TEXT NOT NULL DEFAULT '\"medium\"',
            status TEXT NOT NULL DEFAULT '\"todo\"',
            tags TEXT NOT NULL DEFAULT '[]',
            document_refs TEXT NOT NULL DEFAULT '[]',
            due_date TEXT,
            position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            decay TEXT NOT NULL DEFAULT '\"natural\"',
            last_touched_at TEXT,
            external_refs TEXT NOT NULL DEFAULT '[]',
            design_node_id TEXT,
            execution TEXT,
            openspec_change TEXT,
            engagement_id TEXT
        );
        CREATE TABLE boards (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            columns TEXT NOT NULL DEFAULT '[]',
            project_id TEXT,
            created_at TEXT NOT NULL
        );"
    ).unwrap();
    db_path
}

fn insert_flynt_task(
    db_path: &Path,
    id: &str,
    board_id: &str,
    column: &str,
    title: &str,
    description: &str,
    external_refs: &[&str],
    execution: Option<serde_json::Value>,
) {
    let conn = Connection::open(db_path).unwrap();
    let now = Utc::now().to_rfc3339();
    let refs_json = serde_json::to_string(
        &external_refs.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    ).unwrap();
    let exec_json = execution.map(|v| serde_json::to_string(&v).unwrap());
    conn.execute(
        "INSERT INTO tasks (id, board_id, column_name, title, description, priority, status,
         tags, document_refs, due_date, position, created_at, updated_at, decay,
         last_touched_at, external_refs, design_node_id, execution, openspec_change, engagement_id)
         VALUES (?1,?2,?3,?4,?5,'\"medium\"','\"todo\"','[]','[]',NULL,0,?6,?7,'\"natural\"',NULL,?8,NULL,?9,NULL,NULL)",
        params![id, board_id, column, title, description, now, now, refs_json, exec_json],
    ).unwrap();
}

fn read_task_column(db_path: &Path, id: &str) -> (String, String) {
    let conn = Connection::open(db_path).unwrap();
    let mut stmt = conn.prepare("SELECT column_name, status FROM tasks WHERE id = ?1").unwrap();
    stmt.query_row(params![id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
}

fn create_task_file(tasks_dir: &Path, slug: &str, content: &str) {
    std::fs::create_dir_all(tasks_dir).unwrap();
    std::fs::write(tasks_dir.join(format!("{slug}.md")), content).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// Flynt vault lifecycle — full claim→complete cycle
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flynt_vault_full_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = create_flynt_db(tmp.path());
    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board_id = uuid::Uuid::new_v4().to_string();
    let task_id = uuid::Uuid::new_v4().to_string();

    insert_flynt_task(
        &db_path, &task_id, &board_id, "Scheduled",
        "Review open PRs",
        "Scan all open pull requests and leave review comments.",
        &["cron:0 */4 * * *"],
        Some(serde_json::json!({
            "model": "anthropic:claude-sonnet-4-6",
            "max_turns": 20,
            "timeout_secs": 300,
        })),
    );

    let board = FlyntTaskBoard::open_with_db(
        tmp.path().to_path_buf(), db_path.clone(), state_db.clone(), "e2e-test".into(),
    ).unwrap();

    let actionable = board.list_actionable().unwrap();
    assert_eq!(actionable.len(), 1);
    assert_eq!(actionable[0].name, "Review open PRs");
    assert!(matches!(actionable[0].triggers[0], Trigger::Cron { .. }));

    let spec = board.task_spec(&task_id).unwrap();
    assert_eq!(spec.prompt, "Scan all open pull requests and leave review comments.");
    assert_eq!(spec.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
    assert_eq!(spec.max_turns, Some(20));
    assert_eq!(spec.timeout_secs, Some(300));

    assert!(board.claim(&task_id).unwrap());
    let (col, status) = read_task_column(&db_path, &task_id);
    assert_eq!(col, "Running");
    assert_eq!(status, "\"in_progress\"");

    assert!(board.list_actionable().unwrap().is_empty());
    assert!(!board.claim(&task_id).unwrap());

    let result = TaskResult {
        exit_code: 0, summary: "Reviewed 3 PRs, left 7 comments".into(),
        tokens_used: 45_000, duration_secs: 120, session_id: "sess-e2e-1".into(),
    };
    board.complete(&task_id, &result).unwrap();
    let (col, status) = read_task_column(&db_path, &task_id);
    assert_eq!(col, "Done");
    assert_eq!(status, "\"done\"");
}

#[test]
fn flynt_vault_fail_and_release_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = create_flynt_db(tmp.path());
    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board_id = uuid::Uuid::new_v4().to_string();
    let task_id = uuid::Uuid::new_v4().to_string();

    insert_flynt_task(
        &db_path, &task_id, &board_id, "Scheduled",
        "Deploy staging", "Run staging deployment pipeline.",
        &["webhook:deploy"], None,
    );

    let board = FlyntTaskBoard::open_with_db(
        tmp.path().to_path_buf(), db_path.clone(), state_db.clone(), "e2e-test".into(),
    ).unwrap();

    assert!(board.claim(&task_id).unwrap());
    assert_eq!(read_task_column(&db_path, &task_id).0, "Running");

    board.fail(&task_id, &TaskError {
        message: "deployment timed out".into(), retriable: true, attempt: 1,
    }).unwrap();
    let (col, status) = read_task_column(&db_path, &task_id);
    assert_eq!(col, "Failed");
    assert_eq!(status, "\"archived\"");

    assert!(board.claim(&task_id).unwrap());
    assert_eq!(read_task_column(&db_path, &task_id).0, "Running");

    board.release(&task_id).unwrap();
    let (col, status) = read_task_column(&db_path, &task_id);
    assert_eq!(col, "Scheduled");
    assert_eq!(status, "\"todo\"");
}

// ═══════════════════════════════════════════════════════════════════════════
// Flynt vault with mixed task types — filtering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn flynt_vault_filters_non_sentry_and_non_scheduled() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = create_flynt_db(tmp.path());
    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board_id = uuid::Uuid::new_v4().to_string();

    let t1 = uuid::Uuid::new_v4().to_string();
    insert_flynt_task(&db_path, &t1, &board_id, "Scheduled", "Auto task",
        "Run automatically.", &["cron:0 * * * *"], None);

    let t2 = uuid::Uuid::new_v4().to_string();
    insert_flynt_task(&db_path, &t2, &board_id, "Backlog", "Backlog idea",
        "Not ready yet.", &["cron:0 9 * * 1"], None);

    let t3 = uuid::Uuid::new_v4().to_string();
    insert_flynt_task(&db_path, &t3, &board_id, "Scheduled", "Manual task",
        "Do this by hand.", &[], None);

    let t4 = uuid::Uuid::new_v4().to_string();
    insert_flynt_task(&db_path, &t4, &board_id, "Scheduled", "Exec-only task",
        "Has an execution block.", &[],
        Some(serde_json::json!({"model": "anthropic:claude-sonnet-4-6"})));

    let t5 = uuid::Uuid::new_v4().to_string();
    insert_flynt_task(&db_path, &t5, &board_id, "Done", "Finished task",
        "Already done.", &["cron:0 * * * *"], None);

    let board = FlyntTaskBoard::open_with_db(
        tmp.path().to_path_buf(), db_path, state_db, "e2e-test".into(),
    ).unwrap();

    let actionable = board.list_actionable().unwrap();
    let names: Vec<&str> = actionable.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Auto task"));
    assert!(names.contains(&"Exec-only task"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Task tree board — .omegon/tasks/ discovery with dependency gating
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn task_tree_dependency_gating_and_completion_unblocks() {
    let tmp = tempfile::tempdir().unwrap();
    let tasks_dir = tmp.path().join(".omegon").join("tasks");

    create_task_file(&tasks_dir, "review-prs", "+++\nid = \"review-prs\"\ntitle = \"Review PRs\"\nstatus = \"todo\"\npriority = \"high\"\n\n[execution]\ncron = \"0 */4 * * *\"\n+++\nReview PRs.\n");
    create_task_file(&tasks_dir, "check-ci", "+++\nid = \"check-ci\"\ntitle = \"Check CI\"\nstatus = \"todo\"\npriority = \"medium\"\n\n[execution]\nwebhook = \"ci-complete\"\n+++\nCheck CI.\n");
    create_task_file(&tasks_dir, "deploy-prod", "+++\nid = \"deploy-prod\"\ntitle = \"Deploy Prod\"\nstatus = \"todo\"\npriority = \"critical\"\ndepends_on = [\"review-prs\"]\n+++\nDeploy after review.\n");
    create_task_file(&tasks_dir, "setup-ci", "+++\nid = \"setup-ci\"\ntitle = \"Setup CI\"\nstatus = \"done\"\npriority = \"low\"\n+++\nAlready done.\n");

    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board = TaskTreeBoard::new(tmp.path().to_path_buf(), state_db, "e2e-test".into());

    let actionable = board.list_actionable().unwrap();
    let ids: Vec<&str> = actionable.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"review-prs"));
    assert!(ids.contains(&"check-ci"));
    assert!(!ids.contains(&"deploy-prod"));

    assert!(board.claim("review-prs").unwrap());
    board.complete("review-prs", &TaskResult {
        exit_code: 0, summary: "done".into(), tokens_used: 1000,
        duration_secs: 10, session_id: "s1".into(),
    }).unwrap();

    let actionable = board.list_actionable().unwrap();
    let ids: Vec<&str> = actionable.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&"deploy-prod"), "deploy-prod should unblock after review-prs completes");
}

// ═══════════════════════════════════════════════════════════════════════════
// File board from sentry.toml — config parsing + lifecycle
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn file_board_lifecycle_from_toml_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config_path = tmp.path().join("sentry.toml");
    std::fs::write(&config_path, r#"
[sentry]
max_concurrent = 2
log_retention_days = 7

[[task]]
name = "lint-check"
prompt = "Run cargo clippy and fix warnings."
model = "anthropic:claude-sonnet-4-6"
max_turns = 10
priority = 2

[task.trigger.cron]
schedule = "0 */2 * * *"

[task.budget]
max_tokens_per_day = 100000

[[task]]
name = "doc-update"
prompt = "Update documentation for recent changes."
max_turns = 15

[task.trigger.webhook]
name = "push-main"
"#).unwrap();

    let config = load_config(&config_path).unwrap();
    assert_eq!(config.tasks.len(), 2);
    assert_eq!(config.sentry.max_concurrent, 2);

    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board = FileTaskBoard::new(
        config.clone(), state_db.clone(), "e2e-test".into(),
        tmp.path().to_path_buf(),
    );

    let actionable = board.list_actionable().unwrap();
    assert_eq!(actionable.len(), 2);

    let lint = actionable.iter().find(|t| t.name == "lint-check").unwrap();
    assert_eq!(lint.priority, 2);
    assert!(matches!(lint.triggers[0], Trigger::Cron { .. }));

    let spec = board.task_spec("lint-check").unwrap();
    assert_eq!(spec.prompt, "Run cargo clippy and fix warnings.");
    assert_eq!(spec.model.as_deref(), Some("anthropic:claude-sonnet-4-6"));
    assert_eq!(spec.max_turns, Some(10));

    assert!(board.claim("lint-check").unwrap());
    board.complete("lint-check", &TaskResult {
        exit_code: 0, summary: "fixed 3 warnings".into(), tokens_used: 5000,
        duration_secs: 30, session_id: "s1".into(),
    }).unwrap();

    let (last_run, count) = state_db.last_run("lint-check").unwrap().unwrap();
    assert_eq!(count, 1);
    assert!(last_run > Utc::now() - chrono::Duration::seconds(10));
}

// ═══════════════════════════════════════════════════════════════════════════
// Budget tracking and enforcement
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn budget_tracking_isolates_per_task_per_day() {
    let state_db = StateDb::in_memory().unwrap();
    let today = Utc::now().format("%Y-%m-%d").to_string();

    state_db.record_budget("heavy-task", &today, 80_000).unwrap();
    state_db.record_budget("heavy-task", &today, 25_000).unwrap();
    assert_eq!(state_db.budget_tokens_today("heavy-task", &today).unwrap(), 105_000);

    state_db.record_budget("light-task", &today, 500).unwrap();
    assert_eq!(state_db.budget_tokens_today("light-task", &today).unwrap(), 500);

    let yesterday = (Utc::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    state_db.record_budget("heavy-task", &yesterday, 999_999).unwrap();
    assert_eq!(state_db.budget_tokens_today("heavy-task", &today).unwrap(), 105_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// Circuit breaker — failure tracking
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn circuit_breaker_counts_recent_failures() {
    let state_db = StateDb::in_memory().unwrap();

    // No failures initially
    assert_eq!(state_db.recent_failure_count("flaky-task").unwrap(), 0);

    // Record 3 consecutive failures
    for i in 1..=3 {
        let run_id = format!("run-{i}");
        state_db.record_run_start(&run_id, "flaky-task").unwrap();
        state_db.record_run_failure(&run_id, &TaskError {
            message: format!("attempt {i} failed"), retriable: true, attempt: i,
        }).unwrap();
    }
    assert_eq!(state_db.recent_failure_count("flaky-task").unwrap(), 3);

    // A success doesn't erase the failure records — it still counts
    // failures within the last hour. The executor uses this count to
    // scale cooldown, not as a binary "healthy" signal.
    state_db.record_run_start("run-ok", "flaky-task").unwrap();
    state_db.record_run_complete("run-ok", &TaskResult {
        exit_code: 0, summary: "ok".into(), tokens_used: 100,
        duration_secs: 1, session_id: "s1".into(),
    }).unwrap();
    // Still 3 failures in the window — success doesn't retroactively clear them
    assert_eq!(state_db.recent_failure_count("flaky-task").unwrap(), 3);

    // Different task is independent
    assert_eq!(state_db.recent_failure_count("other-task").unwrap(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// Run history accumulation across multiple cycles
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn run_history_accumulates_across_executions() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = create_flynt_db(tmp.path());
    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board_id = uuid::Uuid::new_v4().to_string();
    let task_id = uuid::Uuid::new_v4().to_string();

    insert_flynt_task(
        &db_path, &task_id, &board_id, "Scheduled",
        "Recurring check", "Check things.", &["cron:* * * * *"], None,
    );

    let board = FlyntTaskBoard::open_with_db(
        tmp.path().to_path_buf(), db_path.clone(), state_db.clone(), "e2e-test".into(),
    ).unwrap();

    for i in 0..3 {
        if i > 0 {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute(
                "UPDATE tasks SET column_name = 'Scheduled', status = '\"todo\"' WHERE id = ?1",
                params![task_id],
            ).unwrap();
        }

        assert!(board.claim(&task_id).unwrap());
        board.complete(&task_id, &TaskResult {
            exit_code: 0, summary: format!("run {i}"), tokens_used: 1000 * (i + 1),
            duration_secs: 10, session_id: format!("s{i}"),
        }).unwrap();
    }

    let history = state_db.run_history(&task_id, 10).unwrap();
    assert_eq!(history.len(), 3);

    let (last, count) = state_db.last_run(&task_id).unwrap().unwrap();
    assert_eq!(count, 3);
    assert!(last > Utc::now() - chrono::Duration::seconds(10));
}

// ═══════════════════════════════════════════════════════════════════════════
// Concurrent claim contention across instances
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_claim_contention_across_instances() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = create_flynt_db(tmp.path());
    let state_db = Arc::new(StateDb::in_memory().unwrap());
    let board_id = uuid::Uuid::new_v4().to_string();
    let task_id = uuid::Uuid::new_v4().to_string();

    insert_flynt_task(
        &db_path, &task_id, &board_id, "Scheduled",
        "Contested task", "Two instances race.",
        &["cron:* * * * *"], None,
    );

    let board_a = FlyntTaskBoard::open_with_db(
        tmp.path().to_path_buf(), db_path.clone(), state_db.clone(), "instance-a".into(),
    ).unwrap();
    let board_b = FlyntTaskBoard::open_with_db(
        tmp.path().to_path_buf(), db_path.clone(), state_db.clone(), "instance-b".into(),
    ).unwrap();

    assert!(board_a.claim(&task_id).unwrap());
    assert_eq!(read_task_column(&db_path, &task_id).0, "Running");
    assert!(!board_b.claim(&task_id).unwrap());

    board_a.release(&task_id).unwrap();
    assert_eq!(read_task_column(&db_path, &task_id).0, "Scheduled");

    assert!(board_b.claim(&task_id).unwrap());
    assert_eq!(read_task_column(&db_path, &task_id).0, "Running");
}

// ═══════════════════════════════════════════════════════════════════════════
// Config validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn config_rejects_task_without_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sentry.toml");
    std::fs::write(&path, "[sentry]\n[[task]]\nname = \"no-prompt\"\n").unwrap();
    assert!(load_config(&path).unwrap_err().to_string().contains("prompt"));
}

#[test]
fn config_accepts_prompt_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sentry.toml");
    std::fs::write(&path, "[sentry]\n[[task]]\nname = \"ok\"\nprompt_file = \"p.md\"\n").unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.tasks[0].prompt_file.as_deref(), Some("p.md"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Model routing config parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn routing_config_parses_from_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sentry.toml");
    std::fs::write(&path, r#"
[sentry]
max_concurrent = 2

[sentry.routing]
prefilter_model = "anthropic:claude-haiku-4-5-20251001"
light_model = "anthropic:claude-sonnet-4-6"
heavy_model = "anthropic:claude-opus-4-6"

[[task]]
name = "auto-routed"
prompt = "Check CI status"
model = "auto"
"#).unwrap();

    let config = load_config(&path).unwrap();
    let routing = config.sentry.routing.unwrap();
    assert_eq!(routing.prefilter_model, "anthropic:claude-haiku-4-5-20251001");
    assert_eq!(routing.light_model, "anthropic:claude-sonnet-4-6");
    assert_eq!(routing.heavy_model, "anthropic:claude-opus-4-6");
    assert_eq!(config.tasks[0].model.as_deref(), Some("auto"));
}

#[test]
fn routing_config_absent_parses_as_none() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("sentry.toml");
    std::fs::write(&path, "[sentry]\n[[task]]\nname = \"t\"\nprompt = \"do it\"\n").unwrap();
    let config = load_config(&path).unwrap();
    assert!(config.sentry.routing.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// Vault detection heuristics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn vault_detection_by_config_and_db() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!is_flynt_vault(tmp.path()));

    std::fs::create_dir_all(tmp.path().join(".flynt")).unwrap();
    std::fs::write(tmp.path().join(".flynt/config.toml"), "vault_name = \"test\"").unwrap();
    assert!(is_flynt_vault(tmp.path()));

    let tmp2 = tempfile::tempdir().unwrap();
    let db_path = default_db_path(tmp2.path());
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    std::fs::write(&db_path, "").unwrap();
    assert!(is_flynt_vault(tmp2.path()));
}
