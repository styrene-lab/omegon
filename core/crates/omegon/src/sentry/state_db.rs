use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use super::types::{RunRecord, RunStatus, TaskError, TaskResult};

#[derive(Debug, Clone)]
pub struct RoutingStats {
    pub total: u32,
    /// (class, total, successes, total_tokens)
    pub by_class: Vec<(String, u32, u32, u64)>,
}

pub struct StateDb {
    conn: Mutex<Connection>,
}

impl StateDb {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS task_runs (
                run_id       TEXT PRIMARY KEY,
                task_id      TEXT NOT NULL,
                started_at   TEXT NOT NULL,
                finished_at  TEXT,
                status       TEXT NOT NULL DEFAULT 'running',
                exit_code    INTEGER,
                summary      TEXT,
                tokens_used  INTEGER NOT NULL DEFAULT 0,
                duration_secs INTEGER NOT NULL DEFAULT 0,
                session_id   TEXT,
                error        TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_task_runs_task_id ON task_runs(task_id);

            CREATE TABLE IF NOT EXISTS task_claims (
                task_id      TEXT PRIMARY KEY,
                claimed_at   TEXT NOT NULL,
                instance_id  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS budget_usage (
                task_id      TEXT NOT NULL,
                window_start TEXT NOT NULL,
                tokens_used  INTEGER NOT NULL DEFAULT 0,
                cost_usd     REAL NOT NULL DEFAULT 0.0,
                PRIMARY KEY (task_id, window_start)
            );

            CREATE TABLE IF NOT EXISTS routing_outcomes (
                id            TEXT PRIMARY KEY,
                task_id       TEXT NOT NULL,
                classified_as TEXT NOT NULL,
                model_used    TEXT NOT NULL,
                success       INTEGER NOT NULL,
                tokens_used   INTEGER NOT NULL DEFAULT 0,
                duration_secs INTEGER NOT NULL DEFAULT 0,
                recorded_at   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_routing_outcomes_task_id ON routing_outcomes(task_id);
        ")?;
        Ok(())
    }

    pub fn claim_task(&self, task_id: &str, instance_id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let now = Utc::now().to_rfc3339();
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO task_claims (task_id, claimed_at, instance_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![task_id, now, instance_id],
        )?;
        Ok(inserted > 0)
    }

    pub fn release_task(&self, task_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        conn.execute("DELETE FROM task_claims WHERE task_id = ?1", [task_id])?;
        Ok(())
    }

    pub fn release_all(&self, instance_id: &str) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut stmt = conn.prepare(
            "SELECT task_id FROM task_claims WHERE instance_id = ?1",
        )?;
        let ids: Vec<String> = stmt
            .query_map([instance_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        conn.execute("DELETE FROM task_claims WHERE instance_id = ?1", [instance_id])?;
        Ok(ids)
    }

    pub fn is_claimed(&self, task_id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_claims WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn record_run_start(&self, run_id: &str, task_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO task_runs (run_id, task_id, started_at, status) VALUES (?1, ?2, ?3, 'running')",
            rusqlite::params![run_id, task_id, now],
        )?;
        Ok(())
    }

    pub fn record_run_complete(&self, run_id: &str, result: &TaskResult) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE task_runs SET finished_at = ?1, status = 'completed', exit_code = ?2, \
             summary = ?3, tokens_used = ?4, duration_secs = ?5, session_id = ?6 \
             WHERE run_id = ?7",
            rusqlite::params![
                now,
                result.exit_code,
                result.summary,
                result.tokens_used,
                result.duration_secs,
                result.session_id,
                run_id,
            ],
        )?;
        Ok(())
    }

    pub fn record_run_failure(&self, run_id: &str, error: &TaskError) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE task_runs SET finished_at = ?1, status = 'failed', error = ?2 WHERE run_id = ?3",
            rusqlite::params![now, error.message, run_id],
        )?;
        Ok(())
    }

    pub fn last_run(&self, task_id: &str) -> anyhow::Result<Option<(DateTime<Utc>, u32)>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;

        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_runs WHERE task_id = ?1",
            [task_id],
            |row| row.get(0),
        )?;

        if count == 0 {
            return Ok(None);
        }

        let last: String = conn.query_row(
            "SELECT started_at FROM task_runs WHERE task_id = ?1 ORDER BY started_at DESC LIMIT 1",
            [task_id],
            |row| row.get(0),
        )?;

        let dt = DateTime::parse_from_rfc3339(&last)?.with_timezone(&Utc);
        Ok(Some((dt, count)))
    }

    pub fn run_history(&self, task_id: &str, limit: u32) -> anyhow::Result<Vec<RunRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut stmt = conn.prepare(
            "SELECT run_id, task_id, started_at, finished_at, status, exit_code, \
             summary, tokens_used, duration_secs, session_id, error \
             FROM task_runs WHERE task_id = ?1 ORDER BY started_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![task_id, limit], parse_run_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_all_runs(&self, limit: u32) -> anyhow::Result<Vec<RunRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut stmt = conn.prepare(
            "SELECT run_id, task_id, started_at, finished_at, status, exit_code, \
             summary, tokens_used, duration_secs, session_id, error \
             FROM task_runs ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], parse_run_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_budget(&self, task_id: &str, window: &str, tokens: u64) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        conn.execute(
            "INSERT INTO budget_usage (task_id, window_start, tokens_used) VALUES (?1, ?2, ?3) \
             ON CONFLICT(task_id, window_start) DO UPDATE SET tokens_used = tokens_used + ?3",
            rusqlite::params![task_id, window, tokens as i64],
        )?;
        Ok(())
    }

    pub fn budget_tokens_today(&self, task_id: &str, window: &str) -> anyhow::Result<u64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let result: rusqlite::Result<i64> = conn.query_row(
            "SELECT tokens_used FROM budget_usage WHERE task_id = ?1 AND window_start = ?2",
            rusqlite::params![task_id, window],
            |row| row.get(0),
        );
        match result {
            Ok(v) => Ok(v as u64),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(e.into()),
        }
    }

    pub fn recent_failure_count(&self, task_id: &str) -> anyhow::Result<u32> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let cutoff = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM task_runs WHERE task_id = ?1 AND status = 'failed' AND started_at > ?2",
            rusqlite::params![task_id, cutoff],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn prune_old_runs(&self, retention_days: u32) -> anyhow::Result<u64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let cutoff = (Utc::now() - chrono::Duration::days(retention_days as i64)).to_rfc3339();
        let deleted = conn.execute(
            "DELETE FROM task_runs WHERE started_at < ?1 AND status != 'running'",
            [&cutoff],
        )?;
        Ok(deleted as u64)
    }

    pub fn record_routing_outcome(
        &self,
        task_id: &str,
        classified_as: &str,
        model_used: &str,
        success: bool,
        tokens: u64,
        duration: u64,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO routing_outcomes \
             (id, task_id, classified_as, model_used, success, tokens_used, duration_secs, recorded_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                task_id,
                classified_as,
                model_used,
                success as i32,
                tokens as i64,
                duration as i64,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn routing_stats(&self) -> anyhow::Result<RoutingStats> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let total: u32 = conn.query_row(
            "SELECT COUNT(*) FROM routing_outcomes",
            [],
            |row| row.get(0),
        )?;
        let mut stmt = conn.prepare(
            "SELECT classified_as, COUNT(*), SUM(success), SUM(tokens_used) \
             FROM routing_outcomes GROUP BY classified_as",
        )?;
        let by_class = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, i64>(3).map(|v| v as u64)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(RoutingStats { total, by_class })
    }
}

fn parse_run_row(row: &rusqlite::Row) -> rusqlite::Result<RunRecord> {
    let started_str: String = row.get(2)?;
    let finished_str: Option<String> = row.get(3)?;
    let status_str: String = row.get(4)?;

    Ok(RunRecord {
        run_id: row.get(0)?,
        task_id: row.get(1)?,
        started_at: DateTime::parse_from_rfc3339(&started_str)
            .unwrap_or_default()
            .with_timezone(&Utc),
        finished_at: finished_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc)),
        status: status_str.parse().unwrap_or(RunStatus::Failed),
        exit_code: row.get(5)?,
        summary: row.get(6)?,
        tokens_used: row.get::<_, i64>(7).unwrap_or(0) as u64,
        duration_secs: row.get::<_, i64>(8).unwrap_or(0) as u64,
        session_id: row.get(9)?,
        error: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> StateDb {
        StateDb::in_memory().unwrap()
    }

    #[test]
    fn claim_and_release() {
        let db = test_db();
        assert!(db.claim_task("task-1", "inst-a").unwrap());
        assert!(db.is_claimed("task-1").unwrap());
        assert!(!db.claim_task("task-1", "inst-b").unwrap());
        db.release_task("task-1").unwrap();
        assert!(!db.is_claimed("task-1").unwrap());
        assert!(db.claim_task("task-1", "inst-b").unwrap());
    }

    #[test]
    fn release_all_returns_task_ids() {
        let db = test_db();
        db.claim_task("t1", "inst-a").unwrap();
        db.claim_task("t2", "inst-a").unwrap();
        db.claim_task("t3", "inst-b").unwrap();

        let released = db.release_all("inst-a").unwrap();
        assert_eq!(released.len(), 2);
        assert!(released.contains(&"t1".to_string()));
        assert!(released.contains(&"t2".to_string()));
        assert!(db.is_claimed("t3").unwrap());
    }

    #[test]
    fn run_lifecycle() {
        let db = test_db();
        db.record_run_start("run-1", "task-1").unwrap();

        let result = TaskResult {
            exit_code: 0,
            summary: "done".into(),
            tokens_used: 1000,
            duration_secs: 30,
            session_id: "sess-1".into(),
        };
        db.record_run_complete("run-1", &result).unwrap();

        let (last, count) = db.last_run("task-1").unwrap().unwrap();
        assert_eq!(count, 1);
        assert!(last <= Utc::now());

        let history = db.run_history("task-1", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, RunStatus::Completed);
        assert_eq!(history[0].exit_code, Some(0));
        assert_eq!(history[0].tokens_used, 1000);
    }

    #[test]
    fn run_failure() {
        let db = test_db();
        db.record_run_start("run-2", "task-1").unwrap();

        let error = TaskError {
            message: "upstream exhausted".into(),
            retriable: true,
            attempt: 1,
        };
        db.record_run_failure("run-2", &error).unwrap();

        let history = db.run_history("task-1", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, RunStatus::Failed);
        assert_eq!(history[0].error.as_deref(), Some("upstream exhausted"));
    }

    #[test]
    fn last_run_empty() {
        let db = test_db();
        assert!(db.last_run("nonexistent").unwrap().is_none());
    }

    #[test]
    fn multiple_runs_counted() {
        let db = test_db();
        let result = TaskResult {
            exit_code: 0,
            summary: "ok".into(),
            tokens_used: 100,
            duration_secs: 5,
            session_id: "s".into(),
        };

        for i in 0..5 {
            let id = format!("run-{i}");
            db.record_run_start(&id, "task-x").unwrap();
            db.record_run_complete(&id, &result).unwrap();
        }

        let (_, count) = db.last_run("task-x").unwrap().unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn list_all_runs_across_tasks() {
        let db = test_db();
        db.record_run_start("r1", "t1").unwrap();
        db.record_run_start("r2", "t2").unwrap();
        db.record_run_start("r3", "t1").unwrap();

        let all = db.list_all_runs(100).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn budget_tracking() {
        let db = test_db();
        db.record_budget("task-1", "2026-05-09", 1000).unwrap();
        db.record_budget("task-1", "2026-05-09", 500).unwrap();
        let total = db.budget_tokens_today("task-1", "2026-05-09").unwrap();
        assert_eq!(total, 1500);

        let other = db.budget_tokens_today("task-1", "2026-05-10").unwrap();
        assert_eq!(other, 0);
    }

    #[test]
    fn prune_old_runs() {
        let db = test_db();
        let result = TaskResult {
            exit_code: 0,
            summary: "ok".into(),
            tokens_used: 100,
            duration_secs: 5,
            session_id: "s".into(),
        };

        db.record_run_start("old-1", "t1").unwrap();
        db.record_run_complete("old-1", &result).unwrap();
        db.record_run_start("new-1", "t1").unwrap();
        db.record_run_complete("new-1", &result).unwrap();

        // With retention of 0 days, both should be prunable (they were just created
        // so they're "today" — won't be pruned). With 365 days, nothing pruned.
        let pruned = db.prune_old_runs(365).unwrap();
        assert_eq!(pruned, 0);

        let all = db.list_all_runs(100).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn routing_outcome_recorded_and_queryable() {
        let db = test_db();
        db.record_routing_outcome("task-1", "Simple", "haiku", true, 500, 10).unwrap();
        db.record_routing_outcome("task-2", "Complex", "opus", true, 5000, 120).unwrap();
        db.record_routing_outcome("task-3", "Simple", "haiku", false, 300, 8).unwrap();

        let stats = db.routing_stats().unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.by_class.len(), 2);

        let simple = stats.by_class.iter().find(|r| r.0 == "Simple").unwrap();
        assert_eq!(simple.1, 2);  // total
        assert_eq!(simple.2, 1);  // successes
        assert_eq!(simple.3, 800); // total_tokens

        let complex = stats.by_class.iter().find(|r| r.0 == "Complex").unwrap();
        assert_eq!(complex.1, 1);
        assert_eq!(complex.2, 1);
        assert_eq!(complex.3, 5000);
    }

    #[test]
    fn routing_stats_empty_when_no_outcomes() {
        let db = test_db();
        let stats = db.routing_stats().unwrap();
        assert_eq!(stats.total, 0);
        assert!(stats.by_class.is_empty());
    }
}
