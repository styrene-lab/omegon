use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::profiles::AssistantLaunchStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunSummary {
    pub run_id: String,
    pub assistant_id: String,
    pub status: AssistantRunStatus,
    pub trigger: AssistantRunTrigger,
    pub readiness_status: AssistantLaunchStatus,
    pub blocked: Option<AssistantRunBlocked>,
    pub safe_progress: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Blocked,
}

impl AssistantRunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Blocked => "blocked",
        }
    }

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "blocked" => Ok(Self::Blocked),
            other => anyhow::bail!("unknown assistant run status '{other}'"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunBlocked {
    pub reason: AssistantRunBlockedReason,
    pub summary: String,
    pub required_inputs: Vec<AssistantRunRequiredInput>,
    pub resumable: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunBlockedReason {
    MissingCredential,
    MissingRepoAccess,
    AmbiguousAcceptanceCriteria,
    ApprovalRequired,
    DependencyUnavailable,
    TestEnvironmentUnavailable,
    ExternalServiceUnavailable,
    CannotReproduce,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunRequiredInput {
    pub kind: AssistantRunRequiredInputKind,
    pub label: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunRequiredInputKind {
    Secret,
    Permission,
    Clarification,
    Approval,
    ExternalDependency,
    Environment,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRunTrigger {
    pub source: AssistantRunTriggerSource,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunTriggerSource {
    Console,
    Acp,
    Tui,
    System,
}

impl AssistantRunTriggerSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Acp => "acp",
            Self::Tui => "tui",
            Self::System => "system",
        }
    }

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "console" => Ok(Self::Console),
            "acp" => Ok(Self::Acp),
            "tui" => Ok(Self::Tui),
            "system" => Ok(Self::System),
            other => anyhow::bail!("unknown assistant run trigger source '{other}'"),
        }
    }
}

fn readiness_as_str(status: &AssistantLaunchStatus) -> &'static str {
    match status {
        AssistantLaunchStatus::Ready => "ready",
        AssistantLaunchStatus::Degraded => "degraded",
        AssistantLaunchStatus::Blocked => "blocked",
    }
}

fn readiness_from_str(value: &str) -> anyhow::Result<AssistantLaunchStatus> {
    match value {
        "ready" => Ok(AssistantLaunchStatus::Ready),
        "degraded" => Ok(AssistantLaunchStatus::Degraded),
        "blocked" => Ok(AssistantLaunchStatus::Blocked),
        other => anyhow::bail!("unknown assistant readiness status '{other}'"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewAssistantRun {
    pub run_id: String,
    pub assistant_id: String,
    pub status: AssistantRunStatus,
    pub trigger: AssistantRunTrigger,
    pub readiness_status: AssistantLaunchStatus,
    pub blocked: Option<AssistantRunBlocked>,
    pub safe_progress: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

pub struct SqliteAssistantRunStore {
    conn: Mutex<Connection>,
}

impl SqliteAssistantRunStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS assistant_runs (
                run_id           TEXT PRIMARY KEY,
                assistant_id     TEXT NOT NULL,
                status           TEXT NOT NULL,
                trigger_source   TEXT NOT NULL,
                trigger_label    TEXT,
                readiness_status TEXT NOT NULL,
                safe_progress    TEXT,
                blocked_json     TEXT,
                created_at       TEXT NOT NULL,
                updated_at       TEXT NOT NULL,
                started_at       TEXT,
                completed_at     TEXT,
                error_summary    TEXT,
                executor_kind    TEXT NOT NULL DEFAULT 'local_daemon',
                executor_ref     TEXT,
                schema_version   INTEGER NOT NULL DEFAULT 1
            );

            CREATE INDEX IF NOT EXISTS idx_assistant_runs_updated
                ON assistant_runs(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_assistant_runs_assistant
                ON assistant_runs(assistant_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_assistant_runs_status
                ON assistant_runs(status, updated_at DESC);

            CREATE TABLE IF NOT EXISTS assistant_run_events (
                event_id     TEXT PRIMARY KEY,
                run_id       TEXT NOT NULL,
                event_type   TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES assistant_runs(run_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_assistant_run_events_run
                ON assistant_run_events(run_id, created_at ASC);
            ",
        )?;
        conn.execute_batch("ALTER TABLE assistant_runs ADD COLUMN blocked_json TEXT;")
            .ok();

        Ok(())
    }

    pub fn list(&self) -> anyhow::Result<Vec<AssistantRunSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_id, assistant_id, status, trigger_source, trigger_label,
                    readiness_status, safe_progress, blocked_json, created_at, updated_at
             FROM assistant_runs
             ORDER BY updated_at DESC, created_at DESC, run_id ASC",
        )?;
        let rows = stmt.query_map([], row_to_run)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get(&self, run_id: &str) -> anyhow::Result<Option<AssistantRunSummary>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT run_id, assistant_id, status, trigger_source, trigger_label,
                    readiness_status, safe_progress, blocked_json, created_at, updated_at
             FROM assistant_runs
             WHERE run_id = ?1",
            params![run_id],
            row_to_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn insert(&self, run: NewAssistantRun) -> anyhow::Result<AssistantRunSummary> {
        validate_blocked_contract(&run)?;
        let blocked_json = run
            .blocked
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let created_at = run.created_at.unwrap_or_else(now_sqlite);
        let updated_at = run.updated_at.unwrap_or_else(|| created_at.clone());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO assistant_runs (
                run_id, assistant_id, status, trigger_source, trigger_label,
                readiness_status, safe_progress, blocked_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                run.run_id,
                run.assistant_id,
                run.status.as_str(),
                run.trigger.source.as_str(),
                run.trigger.label,
                readiness_as_str(&run.readiness_status),
                run.safe_progress,
                blocked_json,
                created_at,
                updated_at,
            ],
        )?;
        drop(conn);
        self.get_latest_inserted()
    }

    pub fn mark_blocked(
        &self,
        run_id: &str,
        blocked: AssistantRunBlocked,
        safe_progress: Option<String>,
        updated_at: Option<String>,
    ) -> anyhow::Result<Option<AssistantRunSummary>> {
        let Some(current) = self.get(run_id)? else {
            return Ok(None);
        };
        if !matches!(
            current.status,
            AssistantRunStatus::Queued | AssistantRunStatus::Running
        ) {
            anyhow::bail!(
                "cannot mark assistant run '{}' blocked from terminal status '{}'",
                run_id,
                current.status.as_str()
            );
        }

        let blocked_json = serde_json::to_string(&blocked)?;
        let updated_at = updated_at.unwrap_or_else(now_sqlite);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE assistant_runs
             SET status = ?2, blocked_json = ?3, safe_progress = ?4, updated_at = ?5
             WHERE run_id = ?1",
            params![
                run_id,
                AssistantRunStatus::Blocked.as_str(),
                blocked_json,
                safe_progress,
                updated_at,
            ],
        )?;
        drop(conn);
        self.get(run_id)
    }

    fn get_latest_inserted(&self) -> anyhow::Result<AssistantRunSummary> {
        self.list()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("inserted assistant run missing"))
    }
}

fn validate_blocked_contract(run: &NewAssistantRun) -> anyhow::Result<()> {
    match (run.status, run.blocked.is_some()) {
        (AssistantRunStatus::Blocked, true) => Ok(()),
        (AssistantRunStatus::Blocked, false) => {
            anyhow::bail!("blocked assistant runs require blocked metadata")
        }
        (_, true) => anyhow::bail!("blocked metadata is only valid for blocked assistant runs"),
        (_, false) => Ok(()),
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssistantRunSummary> {
    let status: String = row.get(2)?;
    let trigger_source: String = row.get(3)?;
    let readiness_status: String = row.get(5)?;
    let blocked_json: Option<String> = row.get(7)?;
    let blocked = blocked_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| to_sql_error(anyhow::Error::new(err)))?;
    Ok(AssistantRunSummary {
        run_id: row.get(0)?,
        assistant_id: row.get(1)?,
        status: AssistantRunStatus::from_str(&status).map_err(to_sql_error)?,
        trigger: AssistantRunTrigger {
            source: AssistantRunTriggerSource::from_str(&trigger_source).map_err(to_sql_error)?,
            label: row.get(4)?,
        },
        readiness_status: readiness_from_str(&readiness_status).map_err(to_sql_error)?,
        blocked,
        safe_progress: row.get(6)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn to_sql_error(err: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(err.to_string())),
    )
}

fn now_sqlite() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_store_lists_newest_first_without_payloads() {
        let store = SqliteAssistantRunStore::in_memory().unwrap();
        store
            .insert(run("old", "2026-06-11T00:00:00Z", "2026-06-11T00:01:00Z"))
            .unwrap();
        store
            .insert(run("new", "2026-06-11T00:02:00Z", "2026-06-11T00:03:00Z"))
            .unwrap();

        let runs = store.list().unwrap();
        assert_eq!(runs[0].run_id, "new");
        assert_eq!(
            store.get("old").unwrap().unwrap().safe_progress.as_deref(),
            Some("completed")
        );
        assert!(store.get("missing").unwrap().is_none());
    }

    #[test]
    fn blocked_runs_round_trip_with_required_inputs() {
        let store = SqliteAssistantRunStore::in_memory().unwrap();
        let mut run = run("blocked", "2026-06-11T00:00:00Z", "2026-06-11T00:01:00Z");
        run.status = AssistantRunStatus::Blocked;
        run.safe_progress = Some("blocked: missing credentials".into());
        run.blocked = Some(AssistantRunBlocked {
            reason: AssistantRunBlockedReason::MissingCredential,
            summary: "GitHub credentials are required before issue work can continue.".into(),
            required_inputs: vec![AssistantRunRequiredInput {
                kind: AssistantRunRequiredInputKind::Secret,
                label: "GITHUB_TOKEN".into(),
                details: Some("Provide a token with repo read/write access.".into()),
            }],
            resumable: true,
        });

        let inserted = store.insert(run).unwrap();
        assert_eq!(inserted.status, AssistantRunStatus::Blocked);
        let blocked = inserted.blocked.as_ref().expect("blocked metadata");
        assert_eq!(blocked.reason, AssistantRunBlockedReason::MissingCredential);
        assert_eq!(
            blocked.required_inputs[0].kind,
            AssistantRunRequiredInputKind::Secret
        );
        assert_eq!(blocked.required_inputs[0].label, "GITHUB_TOKEN");
        assert!(blocked.resumable);

        let fetched = store.get("blocked").unwrap().unwrap();
        assert_eq!(fetched.blocked, inserted.blocked);
    }

    #[test]
    fn blocked_contract_rejects_invalid_metadata_combinations() {
        let store = SqliteAssistantRunStore::in_memory().unwrap();
        let mut blocked_without_metadata =
            run("blocked", "2026-06-11T00:00:00Z", "2026-06-11T00:01:00Z");
        blocked_without_metadata.status = AssistantRunStatus::Blocked;
        assert!(store.insert(blocked_without_metadata).is_err());

        let mut running_with_blocked_metadata =
            run("running", "2026-06-11T00:00:00Z", "2026-06-11T00:01:00Z");
        running_with_blocked_metadata.blocked = Some(AssistantRunBlocked {
            reason: AssistantRunBlockedReason::Other,
            summary: "not allowed while running".into(),
            required_inputs: Vec::new(),
            resumable: false,
        });
        assert!(store.insert(running_with_blocked_metadata).is_err());
    }

    #[test]
    fn mark_blocked_updates_running_run_with_structured_metadata() {
        let store = SqliteAssistantRunStore::in_memory().unwrap();
        store
            .insert(run(
                "running",
                "2026-06-11T00:00:00Z",
                "2026-06-11T00:01:00Z",
            ))
            .unwrap();

        let updated = store
            .mark_blocked(
                "running",
                github_token_blocker(),
                Some("blocked: missing GitHub token".into()),
                Some("2026-06-11T00:02:00Z".into()),
            )
            .unwrap()
            .expect("updated run");

        assert_eq!(updated.status, AssistantRunStatus::Blocked);
        assert_eq!(updated.updated_at.as_deref(), Some("2026-06-11T00:02:00Z"));
        assert_eq!(
            updated.safe_progress.as_deref(),
            Some("blocked: missing GitHub token")
        );
        assert_eq!(
            updated.blocked.as_ref().unwrap().reason,
            AssistantRunBlockedReason::MissingCredential
        );
    }

    #[test]
    fn mark_blocked_returns_none_for_missing_run() {
        let store = SqliteAssistantRunStore::in_memory().unwrap();
        let updated = store
            .mark_blocked("missing", github_token_blocker(), None, None)
            .unwrap();
        assert!(updated.is_none());
    }

    #[test]
    fn mark_blocked_rejects_terminal_runs() {
        let store = SqliteAssistantRunStore::in_memory().unwrap();
        let mut terminal = run("done", "2026-06-11T00:00:00Z", "2026-06-11T00:01:00Z");
        terminal.status = AssistantRunStatus::Succeeded;
        store.insert(terminal).unwrap();

        let err = store
            .mark_blocked("done", github_token_blocker(), None, None)
            .unwrap_err();
        assert!(err.to_string().contains("terminal status 'succeeded'"));
    }

    fn github_token_blocker() -> AssistantRunBlocked {
        AssistantRunBlocked {
            reason: AssistantRunBlockedReason::MissingCredential,
            summary: "GitHub credentials are required before issue work can continue.".into(),
            required_inputs: vec![AssistantRunRequiredInput {
                kind: AssistantRunRequiredInputKind::Secret,
                label: "GITHUB_TOKEN".into(),
                details: Some("Provide a token with repo read/write access.".into()),
            }],
            resumable: true,
        }
    }

    fn run(run_id: &str, created_at: &str, updated_at: &str) -> NewAssistantRun {
        NewAssistantRun {
            run_id: run_id.into(),
            assistant_id: "daily".into(),
            status: AssistantRunStatus::Running,
            trigger: AssistantRunTrigger {
                source: AssistantRunTriggerSource::Console,
                label: Some("manual".into()),
            },
            readiness_status: AssistantLaunchStatus::Ready,
            blocked: None,
            safe_progress: Some("completed".into()),
            created_at: Some(created_at.into()),
            updated_at: Some(updated_at.into()),
        }
    }
}
