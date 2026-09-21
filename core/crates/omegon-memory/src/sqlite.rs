//! SqliteBackend — production MemoryBackend backed by rusqlite.
//!
//! Schema matches the TypeScript factstore.ts (v5) exactly.
//! WAL mode for concurrent reads. FTS5 for full-text search.
//! Bundled sqlite via rusqlite's `bundled` feature.

use async_trait::async_trait;
use rusqlite::{Connection, DatabaseName, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const MEMORY_SCHEMA_VERSION: i64 = 14;
pub const PRIMENSUS_MIND: &str = "primensus";
pub const LEGACY_MIND: &str = "legacy";
pub const LEGACY_MEMORY_SCHEMA_VERSIONS: std::ops::RangeInclusive<i64> = 5..=13;

const CREATE_INDEXING_ATTEMPTS: &str = "CREATE TABLE IF NOT EXISTS embedding_indexing (
    fact_id TEXT PRIMARY KEY REFERENCES facts(id) ON DELETE CASCADE,
    fact_version INTEGER NOT NULL,
    reason TEXT NOT NULL,
    record_json TEXT NOT NULL
);";
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryMigrationPlan {
    pub source: PathBuf,
    pub source_version: i64,
    pub target_version: i64,
    pub fact_count: u64,
    pub mind_count: u64,
    pub edge_count: u64,
    pub episode_count: u64,
    pub fact_version_hwm: u64,
    pub integrity_check: String,
    pub foreign_key_violations: u64,
    pub backup: PathBuf,
    pub statements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryMigrationResult {
    pub source: PathBuf,
    pub backup: PathBuf,
    pub source_version: i64,
    pub target_version: i64,
    pub fact_count: u64,
    pub mind_count: u64,
    pub edge_count: u64,
    pub episode_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryStoreStatus {
    pub path: PathBuf,
    pub schema_version: i64,
    pub fact_count: u64,
    pub mind_count: u64,
    pub edge_count: u64,
    pub episode_count: u64,
    pub integrity_check: String,
    pub foreign_key_violations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRollbackResult {
    pub source: PathBuf,
    pub restored_from: PathBuf,
    pub preserved_current: PathBuf,
    pub restored_version: i64,
    pub fact_count: u64,
    pub episode_count: u64,
}

impl MemoryMigrationPlan {
    pub fn is_applicable(&self) -> bool {
        self.integrity_check == "ok"
            && self.foreign_key_violations == 0
            && LEGACY_MEMORY_SCHEMA_VERSIONS.contains(&self.source_version)
    }
}

use crate::backend::*;
use crate::hash;
use crate::types::*;
use crate::util::{gen_id, now_iso};
use crate::vectors;

pub struct SqliteBackend {
    cache_identity: u64,
    conn: Mutex<Connection>,
}

impl SqliteBackend {
    pub fn plan_migration(path: &Path) -> anyhow::Result<MemoryMigrationPlan> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let source_version = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;
        if !LEGACY_MEMORY_SCHEMA_VERSIONS.contains(&source_version) {
            anyhow::bail!(
                "memory migration only supports schema v5 through v12 sources; found v{source_version}"
            );
        }
        let integrity_check: String =
            conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let foreign_key_violations = {
            let mut statement = conn.prepare("PRAGMA foreign_key_check")?;
            let mut rows = statement.query([])?;
            let mut count = 0;
            while rows.next()?.is_some() {
                count += 1;
            }
            count
        };
        let count = |table: &str| -> anyhow::Result<u64> {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            Ok(conn.query_row(&sql, [], |row| row.get(0))?)
        };
        let backup = path.with_extension(format!("v{source_version}.backup.db"));
        Ok(MemoryMigrationPlan {
            source: path.to_path_buf(),
            source_version,
            target_version: MEMORY_SCHEMA_VERSION,
            fact_count: count("facts")?,
            mind_count: count("minds")?,
            edge_count: count("edges")?,
            episode_count: count("episodes")?,
            fact_version_hwm: conn.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM facts",
                [],
                |row| row.get(0),
            )?,
            integrity_check,
            foreign_key_violations,
            backup,
            statements: if source_version < 7 {
                vec![
                    format!(
                        "INSERT OR IGNORE INTO minds (name, description, created_at) VALUES ('{PRIMENSUS_MIND}', 'Authoritative ambient memory', datetime('now'))"
                    ),
                    format!(
                        "INSERT OR IGNORE INTO minds (name, description, status, created_at) VALUES ('{LEGACY_MIND}', 'Quarantined pre-v7 memory', 'quarantined', datetime('now'))"
                    ),
                    format!("UPDATE facts SET mind = '{LEGACY_MIND}' WHERE mind = 'default'"),
                    format!("UPDATE episodes SET mind = '{LEGACY_MIND}' WHERE mind = 'default'"),
                    "ALTER TABLE episodes ADD COLUMN formation TEXT; -- if absent".into(),
                    "ALTER TABLE facts_vec ADD COLUMN space TEXT; -- if absent".into(),
                    "ALTER TABLE facts_vec ADD COLUMN source_hash TEXT; -- if absent".into(),
                    "ALTER TABLE facts ADD COLUMN lifecycle_inference TEXT; -- if absent".into(),
                    "ALTER TABLE facts ADD COLUMN applicability TEXT; -- if absent".into(),
                    format!(
                        "INSERT INTO schema_version (version, applied_at) VALUES ({MEMORY_SCHEMA_VERSION}, datetime('now'))"
                    ),
                ]
            } else {
                vec![
                    format!("UPDATE facts SET mind = '{PRIMENSUS_MIND}' WHERE mind = 'default'"),
                    format!("UPDATE episodes SET mind = '{PRIMENSUS_MIND}' WHERE mind = 'default'"),
                    "ALTER TABLE episodes ADD COLUMN formation TEXT; -- if absent".into(),
                    "ALTER TABLE facts_vec ADD COLUMN space TEXT; -- if absent".into(),
                    "ALTER TABLE facts_vec ADD COLUMN source_hash TEXT; -- if absent".into(),
                    "ALTER TABLE facts ADD COLUMN lifecycle_inference TEXT; -- if absent".into(),
                    "ALTER TABLE facts ADD COLUMN applicability TEXT; -- if absent".into(),
                    format!(
                        "INSERT INTO schema_version (version, applied_at) VALUES ({MEMORY_SCHEMA_VERSION}, datetime('now'))"
                    ),
                ]
            },
        })
    }

    /// Repair records written by pre-v7 callers that still used the historical
    /// `default` mind after the store had already migrated to v7.
    ///
    /// Legacy records are moved to `legacy` during migration. Therefore any
    /// `default` records present in an established v7 store are post-migration
    /// writes and belong to the authoritative `primensus` mind.
    pub fn reconcile_current_default_mind(path: &Path) -> anyhow::Result<u64> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let transaction =
            conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;
        if version != MEMORY_SCHEMA_VERSION {
            anyhow::bail!(
                "default-mind reconciliation requires schema v{}, found v{version}",
                MEMORY_SCHEMA_VERSION
            );
        }
        transaction.execute(
            "INSERT OR IGNORE INTO minds (name, description, created_at) VALUES (?1, 'Authoritative ambient memory', datetime('now'))",
            params![PRIMENSUS_MIND],
        )?;
        let facts = transaction.execute(
            "UPDATE facts SET mind = ?1 WHERE mind = 'default'",
            params![PRIMENSUS_MIND],
        )?;
        let episodes = transaction.execute(
            "UPDATE episodes SET mind = ?1 WHERE mind = 'default'",
            params![PRIMENSUS_MIND],
        )?;
        transaction.commit()?;
        Ok((facts + episodes) as u64)
    }

    pub fn status(path: &Path) -> anyhow::Result<MemoryStoreStatus> {
        let inspected = Self::inspect_current(path)?;
        Ok(MemoryStoreStatus {
            path: path.to_path_buf(),
            schema_version: inspected.source_version,
            fact_count: inspected.fact_count,
            mind_count: inspected.mind_count,
            edge_count: inspected.edge_count,
            episode_count: inspected.episode_count,
            integrity_check: inspected.integrity_check,
            foreign_key_violations: inspected.foreign_key_violations,
        })
    }

    pub fn rollback_migration(
        source_path: &Path,
        backup_path: &Path,
    ) -> anyhow::Result<MemoryRollbackResult> {
        if source_path == backup_path {
            anyhow::bail!("memory rollback backup path must differ from source");
        }
        let current = Self::inspect_current(source_path)?;
        if current.source_version != MEMORY_SCHEMA_VERSION {
            anyhow::bail!(
                "memory rollback requires a v{} source; found v{}",
                MEMORY_SCHEMA_VERSION,
                current.source_version
            );
        }
        let backup = Self::inspect_current(backup_path)?;
        if !LEGACY_MEMORY_SCHEMA_VERSIONS.contains(&backup.source_version) {
            anyhow::bail!(
                "memory rollback backup must be schema v5 through v7; found v{}",
                backup.source_version
            );
        }
        Self::verify_migration_counts(&current, &backup)?;

        let preserved_current =
            source_path.with_extension(format!("v{}.rollback-source.db", current.source_version));
        if preserved_current.exists() {
            anyhow::bail!(
                "memory rollback preservation path already exists at {}; refusing to overwrite it",
                preserved_current.display()
            );
        }

        let current_conn = Connection::open_with_flags(
            source_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        current_conn.backup(DatabaseName::Main, &preserved_current, None)?;
        drop(current_conn);

        let result = (|| -> anyhow::Result<MemoryRollbackResult> {
            let mut target = Connection::open(source_path)?;
            target.busy_timeout(std::time::Duration::from_secs(5))?;
            target.restore(
                DatabaseName::Main,
                backup_path,
                None::<fn(rusqlite::backup::Progress)>,
            )?;
            drop(target);

            let restored = Self::inspect_current(source_path)?;
            Self::verify_migration_counts(&backup, &restored)?;
            if restored.source_version != backup.source_version {
                anyhow::bail!(
                    "memory rollback verification expected v{}, found v{}",
                    backup.source_version,
                    restored.source_version
                );
            }
            Ok(MemoryRollbackResult {
                source: source_path.to_path_buf(),
                restored_from: backup_path.to_path_buf(),
                preserved_current: preserved_current.clone(),
                restored_version: restored.source_version,
                fact_count: restored.fact_count,
                episode_count: restored.episode_count,
            })
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&preserved_current);
        }
        result
    }

    pub fn apply_migration(plan: &MemoryMigrationPlan) -> anyhow::Result<MemoryMigrationResult> {
        if !plan.is_applicable() {
            anyhow::bail!("memory migration plan failed source verification");
        }
        if plan.source == plan.backup {
            anyhow::bail!("memory migration backup path must differ from source");
        }
        if plan.backup.exists() {
            anyhow::bail!(
                "memory migration backup already exists at {}; refusing to overwrite it",
                plan.backup.display()
            );
        }
        let backup_temp = plan.backup.with_extension(format!(
            "{}.{}.tmp",
            plan.backup
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("db"),
            gen_id()
        ));
        let backup_claim = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_temp)
            .map_err(|error| {
                anyhow::anyhow!(
                    "memory migration temporary backup cannot be claimed at {}: {error}",
                    backup_temp.display()
                )
            })?;
        drop(backup_claim);

        let mut backup_created = false;
        let mut source_committed = false;
        let result = (|| -> anyhow::Result<MemoryMigrationResult> {
            let mut source = Connection::open_with_flags(
                &plan.source,
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            source.busy_timeout(std::time::Duration::from_secs(5))?;
            source.execute_batch("PRAGMA foreign_keys=ON;")?;
            let data_version_before: i64 =
                source.query_row("PRAGMA data_version", [], |row| row.get(0))?;
            source.backup(DatabaseName::Main, &backup_temp, None)?;
            let backup_plan = Self::plan_migration(&backup_temp)?;
            Self::verify_source_snapshot(plan, &backup_plan)?;
            std::fs::hard_link(&backup_temp, &plan.backup).map_err(|error| {
                anyhow::anyhow!(
                    "memory migration backup cannot be published at {}; refusing to overwrite it: {error}",
                    plan.backup.display()
                )
            })?;
            backup_created = true;
            std::fs::remove_file(&backup_temp)?;

            let transaction =
                source.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive)?;
            let data_version_after: i64 =
                transaction.query_row("PRAGMA data_version", [], |row| row.get(0))?;
            if data_version_after != data_version_before {
                anyhow::bail!("memory store changed while the migration backup was created");
            }
            let current: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )?;
            if current != plan.source_version {
                anyhow::bail!(
                    "memory schema changed after planning: expected v{}, found v{current}",
                    plan.source_version
                );
            }
            let current_snapshot = (
                transaction.query_row("SELECT COUNT(*) FROM facts", [], |row| row.get(0))?,
                transaction.query_row("SELECT COUNT(*) FROM minds", [], |row| row.get(0))?,
                transaction.query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?,
                transaction.query_row("SELECT COUNT(*) FROM episodes", [], |row| row.get(0))?,
                transaction.query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM facts",
                    [],
                    |row| row.get(0),
                )?,
            );
            let backup_snapshot = (
                backup_plan.fact_count,
                backup_plan.mind_count,
                backup_plan.edge_count,
                backup_plan.episode_count,
                backup_plan.fact_version_hwm,
            );
            if current_snapshot != backup_snapshot {
                anyhow::bail!(
                    "memory store changed after backup: expected {backup_snapshot:?}, found {current_snapshot:?}"
                );
            }

            transaction.execute(
                "INSERT OR IGNORE INTO minds (name, description, created_at) VALUES (?1, 'Authoritative ambient memory', datetime('now'))",
                params![PRIMENSUS_MIND],
            )?;
            if plan.source_version < 7 {
                transaction.execute(
                    "INSERT OR IGNORE INTO minds (name, description, status, created_at) VALUES (?1, 'Quarantined pre-v7 memory', 'quarantined', datetime('now'))",
                    params![LEGACY_MIND],
                )?;
                transaction.execute(
                    "UPDATE facts SET mind = ?1 WHERE mind = 'default'",
                    params![LEGACY_MIND],
                )?;
                transaction.execute(
                    "UPDATE episodes SET mind = ?1 WHERE mind = 'default'",
                    params![LEGACY_MIND],
                )?;
            } else {
                transaction.execute(
                    "UPDATE facts SET mind = ?1 WHERE mind = 'default'",
                    params![PRIMENSUS_MIND],
                )?;
                transaction.execute(
                    "UPDATE episodes SET mind = ?1 WHERE mind = 'default'",
                    params![PRIMENSUS_MIND],
                )?;
            }
            Self::add_column_if_missing(&transaction, "facts", "persona_id", "TEXT")?;
            Self::add_column_if_missing(
                &transaction,
                "facts",
                "layer",
                "TEXT NOT NULL DEFAULT 'project'",
            )?;
            Self::add_column_if_missing(&transaction, "facts", "tags", "TEXT")?;
            Self::add_column_if_missing(&transaction, "episodes", "jj_change_id", "TEXT")?;
            Self::add_column_if_missing(
                &transaction,
                "episodes",
                "affected_nodes",
                "TEXT NOT NULL DEFAULT '[]'",
            )?;
            Self::add_column_if_missing(
                &transaction,
                "episodes",
                "affected_changes",
                "TEXT NOT NULL DEFAULT '[]'",
            )?;
            Self::add_column_if_missing(
                &transaction,
                "episodes",
                "files_changed",
                "TEXT NOT NULL DEFAULT '[]'",
            )?;
            Self::add_column_if_missing(
                &transaction,
                "episodes",
                "tags",
                "TEXT NOT NULL DEFAULT '[]'",
            )?;
            Self::add_column_if_missing(&transaction, "episodes", "tool_calls_count", "INTEGER")?;
            Self::add_column_if_missing(&transaction, "episodes", "formation", "TEXT")?;
            Self::add_column_if_missing(&transaction, "facts_vec", "space", "TEXT")?;
            Self::add_column_if_missing(&transaction, "facts_vec", "source_hash", "TEXT")?;
            Self::add_column_if_missing(&transaction, "facts", "lifecycle_inference", "TEXT")?;
            Self::add_column_if_missing(&transaction, "facts", "applicability", "TEXT")?;
            transaction.execute_batch(CREATE_INDEXING_ATTEMPTS)?;
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS memory_operation_receipts (
                    operation_id TEXT PRIMARY KEY,
                    payload_hash TEXT NOT NULL,
                    effect_json TEXT NOT NULL,
                    committed_at TEXT NOT NULL
                );",
            )?;
            transaction.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
                params![MEMORY_SCHEMA_VERSION],
            )?;
            transaction.commit()?;
            source_committed = true;

            let verified = Self::inspect_current(&plan.source)?;
            Self::verify_migration_counts(plan, &verified)?;
            if verified.source_version != MEMORY_SCHEMA_VERSION {
                anyhow::bail!(
                    "memory migration verification expected v{}, found v{}",
                    MEMORY_SCHEMA_VERSION,
                    verified.source_version
                );
            }
            let backend = Self::open(&plan.source)?;
            drop(backend);
            let admitted = Self::inspect_current(&plan.source)?;
            Self::verify_migration_counts(plan, &admitted)?;
            if admitted.source_version != MEMORY_SCHEMA_VERSION {
                anyhow::bail!(
                    "memory migration post-open verification expected v{}, found v{}",
                    MEMORY_SCHEMA_VERSION,
                    admitted.source_version
                );
            }

            Ok(MemoryMigrationResult {
                source: plan.source.clone(),
                backup: plan.backup.clone(),
                source_version: plan.source_version,
                target_version: MEMORY_SCHEMA_VERSION,
                fact_count: verified.fact_count,
                mind_count: verified.mind_count,
                edge_count: verified.edge_count,
                episode_count: verified.episode_count,
            })
        })();

        let _ = std::fs::remove_file(&backup_temp);
        if result.is_err() && backup_created && !source_committed {
            let _ = std::fs::remove_file(&plan.backup);
        }
        result
    }

    fn inspect_current(path: &Path) -> anyhow::Result<MemoryMigrationPlan> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let source_version = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;
        let integrity_check: String =
            conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let foreign_key_violations = {
            let mut statement = conn.prepare("PRAGMA foreign_key_check")?;
            let mut rows = statement.query([])?;
            let mut count = 0;
            while rows.next()?.is_some() {
                count += 1;
            }
            count
        };
        let count = |table: &str| -> anyhow::Result<u64> {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            Ok(conn.query_row(&sql, [], |row| row.get(0))?)
        };
        Ok(MemoryMigrationPlan {
            source: path.to_path_buf(),
            source_version,
            target_version: MEMORY_SCHEMA_VERSION,
            fact_count: count("facts")?,
            mind_count: count("minds")?,
            edge_count: count("edges")?,
            episode_count: count("episodes")?,
            fact_version_hwm: conn.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM facts",
                [],
                |row| row.get(0),
            )?,
            integrity_check,
            foreign_key_violations,
            backup: path.with_extension(format!("v{source_version}.backup.db")),
            statements: Vec::new(),
        })
    }

    fn add_column_if_missing(
        transaction: &Transaction<'_>,
        table: &str,
        column: &str,
        declaration: &str,
    ) -> anyhow::Result<()> {
        let mut statement = transaction.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        if !columns.iter().any(|existing| existing == column) {
            transaction.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {declaration};"
            ))?;
        }
        Ok(())
    }

    fn verify_migration_counts(
        expected: &MemoryMigrationPlan,
        actual: &MemoryMigrationPlan,
    ) -> anyhow::Result<()> {
        if actual.integrity_check != "ok" || actual.foreign_key_violations != 0 {
            anyhow::bail!(
                "memory migration verification failed: integrity={}, foreign_key_violations={}",
                actual.integrity_check,
                actual.foreign_key_violations
            );
        }
        let expected_counts = (
            expected.fact_count,
            expected.edge_count,
            expected.episode_count,
            expected.fact_version_hwm,
        );
        let actual_counts = (
            actual.fact_count,
            actual.edge_count,
            actual.episode_count,
            actual.fact_version_hwm,
        );
        if expected_counts != actual_counts {
            anyhow::bail!(
                "memory migration fact/edge/episode counts changed: expected {expected_counts:?}, found {actual_counts:?}"
            );
        }
        Ok(())
    }

    fn verify_source_snapshot(
        expected: &MemoryMigrationPlan,
        actual: &MemoryMigrationPlan,
    ) -> anyhow::Result<()> {
        Self::verify_migration_counts(expected, actual)?;
        if expected.mind_count != actual.mind_count
            || expected.source_version != actual.source_version
        {
            anyhow::bail!(
                "memory migration source changed: expected schema v{} with {} minds, found v{} with {} minds",
                expected.source_version,
                expected.mind_count,
                actual.source_version,
                actual.mind_count
            );
        }
        Ok(())
    }

    /// Open or create a sqlite DB at the given path.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let existed = path.exists();
        let conn = Connection::open(path)?;
        let backend = Self {
            cache_identity: crate::selection_cache::backend_identity(),
            conn: Mutex::new(conn),
        };
        if let Err(error) = backend.init_schema(existed) {
            drop(backend);
            if !existed {
                let _ = std::fs::remove_file(path);
            }
            return Err(error);
        }
        Ok(backend)
    }

    /// Open an existing sqlite DB without granting SQLite permission to create it.
    pub fn open_existing(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let has_schema_version: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
            [],
            |row| row.get(0),
        )?;
        if !has_schema_version {
            anyhow::bail!("existing file is not an initialized memory store");
        }
        let backend = Self {
            cache_identity: crate::selection_cache::backend_identity(),
            conn: Mutex::new(conn),
        };
        backend.init_schema(true)?;
        Ok(backend)
    }

    /// Create an in-memory sqlite DB (for testing).
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let backend = Self {
            cache_identity: crate::selection_cache::backend_identity(),
            conn: Mutex::new(conn),
        };
        backend.init_schema(false)?;
        Ok(backend)
    }

    fn init_schema(&self, existed: bool) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        // busy_timeout: wait up to 5s for write lock instead of failing immediately.
        // Critical for multi-process access (cleave children share the same DB file).
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;

        if existed {
            let has_schema_version: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_version')",
                [],
                |row| row.get(0),
            )?;
            if has_schema_version {
                let current: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                    [],
                    |row| row.get(0),
                )?;
                if current != MEMORY_SCHEMA_VERSION {
                    anyhow::bail!(
                        "unsupported memory schema version {current}; Omegon requires exactly v{MEMORY_SCHEMA_VERSION}. Run the documented memory migration workflow before opening this store"
                    );
                }
            }
        }

        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS minds (
                name        TEXT PRIMARY KEY,
                description TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                origin_type TEXT,
                origin_path TEXT,
                origin_url  TEXT,
                readonly    INTEGER NOT NULL DEFAULT 0,
                parent      TEXT,
                created_at  TEXT NOT NULL,
                last_sync   TEXT
            );

            INSERT OR IGNORE INTO minds (name, description, created_at)
            VALUES ('primensus', 'Authoritative ambient memory', datetime('now'));

            CREATE TABLE IF NOT EXISTS facts (
                applicability TEXT,
                lifecycle_inference TEXT,
                id                  TEXT PRIMARY KEY,
                mind                TEXT NOT NULL DEFAULT 'primensus',
                section             TEXT NOT NULL,
                content             TEXT NOT NULL,
                status              TEXT NOT NULL DEFAULT 'active',
                created_at          TEXT NOT NULL,
                created_session     TEXT,
                supersedes          TEXT,
                superseded_at       TEXT,
                archived_at         TEXT,
                source              TEXT NOT NULL DEFAULT 'manual',
                content_hash        TEXT NOT NULL,
                confidence          REAL NOT NULL DEFAULT 1.0,
                last_reinforced     TEXT NOT NULL,
                reinforcement_count INTEGER NOT NULL DEFAULT 1,
                decay_rate          REAL NOT NULL DEFAULT 0.05,
                decay_profile       TEXT NOT NULL DEFAULT 'standard',
                version             INTEGER NOT NULL DEFAULT 0,
                last_accessed       TEXT,
                jj_change_id        TEXT,
                persona_id          TEXT,
                layer               TEXT NOT NULL DEFAULT 'project',
                tags                TEXT,
                FOREIGN KEY (mind) REFERENCES minds(name) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_facts_active ON facts(mind, status) WHERE status = 'active';
            CREATE INDEX IF NOT EXISTS idx_facts_persona ON facts(persona_id) WHERE persona_id IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_facts_layer ON facts(mind, layer) WHERE status = 'active';
            CREATE INDEX IF NOT EXISTS idx_facts_hash ON facts(mind, content_hash);
            CREATE INDEX IF NOT EXISTS idx_facts_section ON facts(mind, section) WHERE status = 'active';
            CREATE INDEX IF NOT EXISTS idx_facts_session ON facts(created_session);
            CREATE INDEX IF NOT EXISTS idx_facts_version ON facts(version DESC);

            CREATE TABLE IF NOT EXISTS facts_vec (
                fact_id    TEXT PRIMARY KEY,
                embedding  BLOB NOT NULL,
                space      TEXT,
                source_hash TEXT,
                model_name TEXT NOT NULL DEFAULT '',
                dims       INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS embedding_metadata (
                model_name  TEXT PRIMARY KEY,
                dims        INTEGER NOT NULL,
                inserted_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS edges (
                id                  TEXT PRIMARY KEY,
                source_fact_id      TEXT NOT NULL,
                target_fact_id      TEXT NOT NULL,
                relation            TEXT NOT NULL,
                description         TEXT,
                confidence          REAL NOT NULL DEFAULT 1.0,
                last_reinforced     TEXT,
                reinforcement_count INTEGER NOT NULL DEFAULT 1,
                decay_rate          REAL NOT NULL DEFAULT 0.05,
                status              TEXT NOT NULL DEFAULT 'active',
                created_at          TEXT NOT NULL,
                created_session     TEXT,
                source_mind         TEXT,
                target_mind         TEXT,
                FOREIGN KEY (source_fact_id) REFERENCES facts(id) ON DELETE CASCADE,
                FOREIGN KEY (target_fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS episodes (
                id          TEXT PRIMARY KEY,
                mind        TEXT NOT NULL DEFAULT 'primensus',
                title       TEXT NOT NULL,
                narrative   TEXT NOT NULL,
                date        TEXT NOT NULL,
                session_id  TEXT,
                created_at  TEXT NOT NULL,
                jj_change_id TEXT,
                affected_nodes TEXT NOT NULL DEFAULT '[]',
                affected_changes TEXT NOT NULL DEFAULT '[]',
                files_changed TEXT NOT NULL DEFAULT '[]',
                tags TEXT NOT NULL DEFAULT '[]',
                tool_calls_count INTEGER,
                formation TEXT,
                FOREIGN KEY (mind) REFERENCES minds(name) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_episodes_mind ON episodes(mind, date DESC);

            CREATE TABLE IF NOT EXISTS episode_facts (
                episode_id TEXT NOT NULL,
                fact_id    TEXT NOT NULL,
                PRIMARY KEY (episode_id, fact_id),
                FOREIGN KEY (episode_id) REFERENCES episodes(id) ON DELETE CASCADE,
                FOREIGN KEY (fact_id) REFERENCES facts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS episodes_vec (
                episode_id TEXT PRIMARY KEY,
                embedding  BLOB NOT NULL,
                model_name TEXT NOT NULL DEFAULT '',
                dims       INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (episode_id) REFERENCES episodes(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS memory_operation_receipts (
                operation_id TEXT PRIMARY KEY,
                payload_hash TEXT NOT NULL,
                effect_json TEXT NOT NULL,
                committed_at TEXT NOT NULL
            );

            -- FTS5 for full-text search on facts
            CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
                id UNINDEXED, mind UNINDEXED, section UNINDEXED, content,
                content='facts', content_rowid='rowid'
            );

            -- FTS sync triggers
            CREATE TRIGGER IF NOT EXISTS facts_fts_insert AFTER INSERT ON facts BEGIN
                INSERT INTO facts_fts(rowid, id, mind, section, content)
                VALUES (NEW.rowid, NEW.id, NEW.mind, NEW.section, NEW.content);
            END;
            CREATE TRIGGER IF NOT EXISTS facts_fts_delete AFTER DELETE ON facts BEGIN
                INSERT INTO facts_fts(facts_fts, rowid, id, mind, section, content)
                VALUES ('delete', OLD.rowid, OLD.id, OLD.mind, OLD.section, OLD.content);
            END;
            CREATE TRIGGER IF NOT EXISTS facts_fts_update AFTER UPDATE ON facts BEGIN
                INSERT INTO facts_fts(facts_fts, rowid, id, mind, section, content)
                VALUES ('delete', OLD.rowid, OLD.id, OLD.mind, OLD.section, OLD.content);
                INSERT INTO facts_fts(rowid, id, mind, section, content)
                VALUES (NEW.rowid, NEW.id, NEW.mind, NEW.section, NEW.content);
            END;

            -- FTS5 for episodes
            CREATE VIRTUAL TABLE IF NOT EXISTS episodes_fts USING fts5(
                id UNINDEXED, mind UNINDEXED, title, narrative,
                content='episodes', content_rowid='rowid'
            );
            CREATE TRIGGER IF NOT EXISTS episodes_fts_insert AFTER INSERT ON episodes BEGIN
                INSERT INTO episodes_fts(rowid, id, mind, title, narrative)
                VALUES (NEW.rowid, NEW.id, NEW.mind, NEW.title, NEW.narrative);
            END;
            CREATE TRIGGER IF NOT EXISTS episodes_fts_delete AFTER DELETE ON episodes BEGIN
                INSERT INTO episodes_fts(episodes_fts, rowid, id, mind, title, narrative)
                VALUES ('delete', OLD.rowid, OLD.id, OLD.mind, OLD.title, OLD.narrative);
            END;
            CREATE TRIGGER IF NOT EXISTS episodes_fts_update AFTER UPDATE ON episodes BEGIN
                INSERT INTO episodes_fts(episodes_fts, rowid, id, mind, title, narrative)
                VALUES ('delete', OLD.rowid, OLD.id, OLD.mind, OLD.title, OLD.narrative);
                INSERT INTO episodes_fts(rowid, id, mind, title, narrative)
                VALUES (NEW.rowid, NEW.id, NEW.mind, NEW.title, NEW.narrative);
                DELETE FROM episodes_vec WHERE episode_id = NEW.id AND NEW.narrative != OLD.narrative;
            END;

            -- Schema version tracking (TS compat — factstore.ts checks this)
            CREATE TABLE IF NOT EXISTS schema_version (
                version    INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
        ")?;

        conn.execute_batch(CREATE_INDEXING_ATTEMPTS)?;
        let current: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;

        if current == 0 && !existed {
            conn.execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
                params![MEMORY_SCHEMA_VERSION],
            )?;
        } else if current != MEMORY_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported memory schema version {current}; Omegon requires exactly v{MEMORY_SCHEMA_VERSION}. Run the documented memory migration workflow before opening this store"
            );
        }

        Ok(())
    }

    fn indexing_record(
        conn: &Connection,
        fact_id: &str,
    ) -> Result<Option<EmbeddingIndexingRecord>> {
        let json: Option<String> = conn
            .query_row(
                "SELECT record_json FROM embedding_indexing WHERE fact_id = ?1",
                params![fact_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        json.map(|json| {
            serde_json::from_str(&json).map_err(|error| MemoryError::Storage(error.into()))
        })
        .transpose()
    }

    fn ensure_mind(&self, conn: &Connection, mind: &str) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO minds (name, created_at) VALUES (?1, ?2)",
            params![mind, now_iso()],
        )?;
        Ok(())
    }

    fn next_version_static(conn: &Connection) -> Result<u64> {
        let max: i64 = conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM facts", [], |r| {
                r.get(0)
            })
            .map_err(|error| MemoryError::Storage(error.into()))?;
        max.checked_add(1)
            .map(|version| version as u64)
            .ok_or_else(|| {
                MemoryError::InvalidMutation("Lamport version space is exhausted".into())
            })
    }

    fn row_to_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Fact> {
        let section_str: String = row.get("section")?;
        let status_str: String = row.get("status")?;
        let profile_str: String = row.get("decay_profile")?;

        let section = serde_json::from_value::<Section>(serde_json::Value::String(section_str.clone()))
            .unwrap_or_else(|_| {
                tracing::warn!(section = %section_str, "unknown section in DB — defaulting to Architecture");
                Section::Architecture
            });
        let status =
            serde_json::from_value::<FactStatus>(serde_json::Value::String(status_str.clone()))
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        row.as_ref().column_index("status").unwrap_or(0),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;

        Ok(Fact {
            applicability: row.get::<_,Option<String>>("applicability")?.map(|encoded| {
                let record:RecordedApplicability=serde_json::from_str(&encoded).map_err(|error|rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(error)))?;
                record.validate().map_err(|error|rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(error)))?;
                Ok::<_,rusqlite::Error>(Box::new(record))
            }).transpose()?,
            lifecycle_inference: {
                let inference = row.get::<_, Option<String>>("lifecycle_inference")?.map(|value| {
                let inference: LifecycleInference = serde_json::from_str(&value).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(error)))?;
                validate_inference_status(&status,Some(&inference),&row.get::<_,String>("content")?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(error)))?;
                Ok::<_,rusqlite::Error>(Box::new(inference))
                }).transpose()?;
                validate_inference_status(&status, inference.as_deref(),&row.get::<_,String>("content")?).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(error)))?;
                inference
            },
            id: row.get("id")?,
            mind: row.get("mind")?,
            content: row.get("content")?,
            section,
            status,
            confidence: row.get("confidence")?,
            reinforcement_count: row.get::<_, u32>("reinforcement_count")?,
            decay_rate: row.get("decay_rate")?,
            decay_profile: serde_json::from_value::<DecayProfileName>(serde_json::Value::String(profile_str.clone()))
                .unwrap_or_else(|_| {
                    tracing::warn!(profile = %profile_str, "unknown decay_profile in DB — defaulting to Standard");
                    DecayProfileName::Standard
                }),
            last_reinforced: row.get("last_reinforced")?,
            created_at: row.get("created_at")?,
            version: row.get::<_, i64>("version")? as u64,
            superseded_by: row.get::<_, Option<String>>("supersedes")?,
            source: row.get::<_, Option<String>>("source")?.filter(|source| !source.is_empty()),
            content_hash: Some(row.get::<_, String>("content_hash")?),
            last_accessed: row.get("last_accessed")?,
            created_session: row.get("created_session")?,
            superseded_at: row.get("superseded_at")?,
            archived_at: row.get("archived_at")?,
            jj_change_id: row.get("jj_change_id")?,
            persona_id: row.get("persona_id")?,
            layer: row.get::<_, Option<String>>("layer")?
                .unwrap_or_else(|| "project".into()),
            tags: row.get::<_, Option<String>>("tags")?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
        })
    }

    fn row_to_episode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Episode> {
        fn json_vec(row: &rusqlite::Row<'_>, column: &str) -> rusqlite::Result<Vec<String>> {
            let value: String = row.get(column)?;
            Ok(serde_json::from_str(&value).unwrap_or_default())
        }

        Ok(Episode {
            id: row.get("id")?,
            mind: row.get("mind")?,
            date: row.get("date")?,
            title: row.get("title")?,
            narrative: row.get("narrative")?,
            created_at: row.get("created_at")?,
            affected_nodes: json_vec(row, "affected_nodes")?,
            affected_changes: json_vec(row, "affected_changes")?,
            files_changed: json_vec(row, "files_changed")?,
            tags: json_vec(row, "tags")?,
            tool_calls_count: row.get("tool_calls_count")?,
            formation: row
                .get::<_, Option<String>>("formation")?
                .map(|value| {
                    let formation =
                        serde_json::from_str::<EpisodeFormation>(&value).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?;
                    formation.validate().map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok::<_, rusqlite::Error>(Box::new(formation))
                })
                .transpose()?,
            jj_change_id: row.get("jj_change_id")?,
        })
    }

    fn capture_cursor(
        conn: &Connection,
        key: &FormationCaptureKey,
    ) -> Result<Option<FormationCursor>> {
        let receipt: Option<String> = conn
            .query_row(
                "SELECT effect_json FROM memory_operation_receipts
             WHERE json_extract(effect_json,'$.kind')='coverage_stored'
               AND json_extract(effect_json,'$.key.mind')=?1
               AND json_extract(effect_json,'$.key.session_id')=?2
               AND json_extract(effect_json,'$.key.stream_id')=?3
               AND json_extract(effect_json,'$.key.policy_version')=?4
               AND json_extract(effect_json,'$.key.model') IS ?5
             ORDER BY json_extract(effect_json,'$.cursor.sequence') DESC LIMIT 1",
                params![
                    key.mind,
                    key.session_id,
                    key.stream_id,
                    key.policy_version,
                    key.model
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        receipt
            .map(|receipt| {
                match serde_json::from_str::<MemoryMutationEffect>(&receipt)
                    .map_err(|error| MemoryError::Storage(error.into()))?
                {
                    MemoryMutationEffect::CoverageStored { cursor, .. } => Ok(cursor),
                    _ => Err(MemoryError::InvalidMutation(
                        "invalid capture receipt".into(),
                    )),
                }
            })
            .transpose()
    }

    fn check_fact_precondition(conn: &Connection, fact: &FactPrecondition) -> Result<Fact> {
        let existing = conn
            .query_row(
                "SELECT * FROM facts WHERE id = ?1",
                params![fact.id],
                Self::row_to_fact,
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?
            .ok_or_else(|| MemoryError::FactNotFound(fact.id.clone()))?;
        if existing.version != fact.expected_version {
            return Err(MemoryError::FactVersionConflict {
                id: fact.id.clone(),
                expected: fact.expected_version,
                actual: existing.version,
            });
        }
        Ok(existing)
    }

    fn import_jsonl_transaction(
        &self,
        tx: &rusqlite::Transaction<'_>,
        jsonl: &str,
    ) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        for line in jsonl.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<JsonlRecord>(trimmed) {
                Ok(JsonlRecord::ApplicableFact(ref fact)) if fact.applicability.is_none() => {
                    return Err(MemoryError::InvalidMutation(
                        "applicable_fact requires applicability metadata".into(),
                    ));
                }
                Ok(JsonlRecord::Fact(jf) | JsonlRecord::ApplicableFact(jf)) => {
                    if let Some(applicability) = &jf.applicability {
                        applicability.validate()?;
                    }
                    validate_inference_status(
                        &jf.status,
                        jf.lifecycle_inference.as_deref(),
                        &jf.content,
                    )?;
                    if let Some(operational) = &jf.operational {
                        operational.validate()?;
                    }
                    let incoming_version = persisted_lamport_version(jf.version)?;
                    self.ensure_mind(tx, &jf.mind)
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    let existing_version: Option<i64> = tx
                        .query_row(
                            "SELECT version FROM facts WHERE id = ?1",
                            params![jf.id],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(|error| MemoryError::Storage(error.into()))?
                        .flatten();
                    if existing_version.is_some_and(|version| incoming_version <= version) {
                        stats.skipped += 1;
                        continue;
                    }
                    let section = serde_json::to_string(&jf.section)
                        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                    let existing_inference: Option<Option<String>> = tx
                        .query_row(
                            "SELECT lifecycle_inference FROM facts WHERE id=?1",
                            [&jf.id],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    let prior = existing_inference
                        .flatten()
                        .map(|encoded| serde_json::from_str::<LifecycleInference>(&encoded))
                        .transpose()
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    validate_inference_import(
                        prior.as_ref(),
                        jf.lifecycle_inference.as_deref(),
                        &jf.status,
                    )?;
                    let profile = serde_json::to_string(&jf.decay_profile)
                        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                    let status = serde_json::to_string(&jf.status)
                        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                    let tags = serde_json::to_string(&jf.tags)
                        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                    let content_hash = jf
                        .content_hash
                        .unwrap_or_else(|| hash::content_hash(&jf.content));
                    if existing_version.is_some() {
                        tx.execute(
                            "UPDATE facts SET mind = ?1, content = ?2, section = ?3, status = ?4, source = ?5, content_hash = ?6, supersedes = ?7, decay_profile = ?8, version = ?9, persona_id = ?10, layer = ?11, tags = ?12 WHERE id = ?13",
                            params![jf.mind, jf.content, section.trim_matches('"'),
                                status.trim_matches('"'), jf.source.as_deref().unwrap_or(""),
                                content_hash, jf.supersedes, profile.trim_matches('"'), incoming_version,
                                jf.persona_id, jf.layer, tags, jf.id],
                        ).map_err(|error| MemoryError::Storage(error.into()))?;
                        stats.reinforced += 1;
                    } else {
                        tx.execute(
                            "INSERT INTO facts (id, mind, section, content, status, created_at, source, content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, decay_profile, version, supersedes, persona_id, layer, tags) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1.0,?6,1,0.05,?9,?10,?11,?12,?13,?14)",
                            params![jf.id, jf.mind, section.trim_matches('"'), jf.content,
                                status.trim_matches('"'), jf.created_at,
                                jf.source.as_deref().unwrap_or(""), content_hash,
                                profile.trim_matches('"'), incoming_version, jf.supersedes,
                                jf.persona_id, jf.layer, tags],
                        ).map_err(|error| MemoryError::Storage(error.into()))?;
                        stats.imported += 1;
                    }
                    tx.execute(
                        "UPDATE facts SET created_at=?1 WHERE id=?2",
                        params![jf.created_at, jf.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                    if let Some(op) = jf.operational {
                        tx.execute("UPDATE facts SET confidence=?1, reinforcement_count=?2, decay_rate=?3, last_reinforced=?4, last_accessed=?5, created_session=?6, superseded_at=?7, archived_at=?8, jj_change_id=?9 WHERE id=?10",
                            params![op.confidence,op.reinforcement_count,op.decay_rate,op.last_reinforced,op.last_accessed,op.created_session,op.superseded_at,op.archived_at,op.jj_change_id,jf.id])
                            .map_err(|error| MemoryError::Storage(error.into()))?;
                    }
                    let inference = jf
                        .lifecycle_inference
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    if let Some(applicability) = &jf.applicability {
                        let encoded = serde_json::to_string(applicability)
                            .map_err(|error| MemoryError::Storage(error.into()))?;
                        tx.execute(
                            "UPDATE facts SET applicability=?1 WHERE id=?2",
                            params![encoded, jf.id],
                        )
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    }
                    tx.execute(
                        "UPDATE facts SET lifecycle_inference=?1 WHERE id=?2",
                        params![inference, jf.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                }
                Ok(JsonlRecord::Episode(episode)) => {
                    if let Some(formation) = &episode.formation {
                        formation.validate()?;
                    }
                    let prior = tx
                        .query_row(
                            "SELECT * FROM episodes WHERE id = ?1",
                            params![episode.id],
                            Self::row_to_episode,
                        )
                        .optional()
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    if let Some(prior) = prior
                        && crate::formation::completes_import(&prior, &episode)?
                    {
                        let formation = episode.formation.as_ref().expect("completed formation");
                        let encoded = serde_json::to_string(formation)
                            .map_err(|error| MemoryError::Storage(error.into()))?;
                        tx.execute(
                            "UPDATE episodes SET formation = ?1, narrative = ?2 WHERE id = ?3",
                            params![encoded, formation.narrative(), episode.id],
                        )
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                        stats.imported += 1;
                        continue;
                    }
                    self.ensure_mind(tx, &episode.mind)
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    let inserted = tx.execute(
                        "INSERT OR IGNORE INTO episodes (id, mind, title, narrative, date, created_at, jj_change_id, affected_nodes, affected_changes, files_changed, tags, tool_calls_count, formation) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                        params![episode.id, episode.mind, episode.title, episode.narrative,
                            episode.date, episode.created_at, episode.jj_change_id,
                            serde_json::to_string(&episode.affected_nodes).unwrap_or_else(|_| "[]".into()),
                            serde_json::to_string(&episode.affected_changes).unwrap_or_else(|_| "[]".into()),
                            serde_json::to_string(&episode.files_changed).unwrap_or_else(|_| "[]".into()),
                            serde_json::to_string(&episode.tags).unwrap_or_else(|_| "[]".into()),
                            episode.tool_calls_count, episode.formation.as_ref().map(serde_json::to_string).transpose().map_err(|error| MemoryError::Storage(error.into()))?],
                    ).map_err(|error| MemoryError::Storage(error.into()))?;
                    stats.imported += inserted;
                    stats.skipped += usize::from(inserted == 0);
                }
                Ok(JsonlRecord::Edge(edge)) => {
                    let inserted = tx.execute(
                        "INSERT OR IGNORE INTO edges (id, source_fact_id, target_fact_id, relation, description, confidence, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                        params![edge.id, edge.source_id, edge.target_id, edge.relation,
                            edge.description, edge.confidence, edge.created_at],
                    ).map_err(|error| MemoryError::Storage(error.into()))?;
                    stats.imported += inserted;
                    stats.skipped += usize::from(inserted == 0);
                }
                Ok(JsonlRecord::Mind(_)) => stats.skipped += 1,
                Err(_) => stats.errors += 1,
            }
        }
        Ok(stats)
    }
}

#[async_trait]
impl MemoryBackend for SqliteBackend {
    async fn selection_revision(
        &self,
    ) -> Result<Option<crate::selection_cache::SelectionRevision>> {
        if self.cache_identity == u64::MAX {
            return Ok(None);
        }
        let conn = self.conn.lock().unwrap();
        let external = conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(Some(crate::selection_cache::SelectionRevision {
            instance: self.cache_identity,
            local: conn.total_changes(),
            external,
        }))
    }
    async fn selection_time_bounds(
        &self,
        mind: &str,
        query_at: chrono::DateTime<chrono::Utc>,
        wall_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<crate::selection_cache::SelectionTimeBounds>> {
        let conn = self.conn.lock().unwrap();
        let mut bounds = crate::selection_cache::SelectionTimeBounds::default();
        let mut statement=conn.prepare("SELECT applicability,confidence,reinforcement_count,decay_profile,last_reinforced FROM facts WHERE mind=?1 AND status='active'").map_err(|error|MemoryError::Storage(error.into()))?;
        let rows = statement
            .query_map([mind], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|error| MemoryError::Storage(error.into()))?;
        for row in rows {
            let (encoded, confidence, count, profile, reinforced) =
                row.map_err(|error| MemoryError::Storage(error.into()))?;
            let applicability = match encoded
                .map(|text| serde_json::from_str::<RecordedApplicability>(&text))
                .transpose()
            {
                Ok(value) => value,
                Err(_) => return Ok(None),
            };
            if applicability
                .as_ref()
                .is_some_and(|record| record.validate().is_err())
            {
                return Ok(None);
            }
            let profile = match serde_json::from_value::<DecayProfileName>(
                serde_json::Value::String(profile),
            ) {
                Ok(profile) => profile,
                Err(_) => return Ok(None),
            };
            bounds.observe(
                applicability.as_ref(),
                confidence,
                count,
                &profile,
                &reinforced,
                query_at,
                wall_at,
            );
        }
        Ok(Some(bounds))
    }
    async fn get_fact_record(&self, mind: &str, id: &str) -> Result<Option<Fact>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT * FROM facts WHERE id=?1 AND mind=?2",
                params![id, mind],
                Self::row_to_fact,
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))
    }
    async fn get_pending_fact(&self, mind: &str, id: &str) -> Result<Option<Fact>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT * FROM facts WHERE id=?1 AND mind=?2 AND status='pending'",
                params![id, mind],
                Self::row_to_fact,
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))
    }
    async fn mutation_receipt(
        &self,
        operation_id: &str,
        payload_hash: &str,
    ) -> Result<Option<MemoryMutationOutcome>> {
        if operation_id.trim().is_empty() || payload_hash.trim().is_empty() {
            return Err(MemoryError::InvalidMutation(
                "operation identity and payload hash must not be empty".into(),
            ));
        }
        let conn = self.conn.lock().unwrap();
        let receipt: Option<(String, String)> = conn
            .query_row(
                "SELECT payload_hash, effect_json FROM memory_operation_receipts WHERE operation_id = ?1",
                params![operation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let Some((recorded_hash, effect_json)) = receipt else {
            return Ok(None);
        };
        if recorded_hash != payload_hash {
            return Err(MemoryError::OperationConflict(operation_id.into()));
        }
        let effect = serde_json::from_str(&effect_json)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(Some(MemoryMutationOutcome {
            effect,
            replayed: true,
        }))
    }

    async fn apply_mutation_bound(
        &self,
        operation_id: &str,
        payload_hash: &str,
        mutation: MemoryMutation,
    ) -> Result<MemoryMutationOutcome> {
        if operation_id.trim().is_empty() {
            return Err(MemoryError::InvalidMutation(
                "operation identity must not be empty".into(),
            ));
        }
        let _ = mutation_payload_hash(&mutation)?;
        if payload_hash.trim().is_empty() {
            return Err(MemoryError::InvalidMutation(
                "operation payload hash must not be empty".into(),
            ));
        }
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;

        let receipt: Option<(String, String)> = transaction
            .query_row(
                "SELECT payload_hash, effect_json FROM memory_operation_receipts WHERE operation_id = ?1",
                params![operation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        if let Some((recorded_hash, effect_json)) = receipt {
            if recorded_hash != payload_hash {
                return Err(MemoryError::OperationConflict(operation_id.into()));
            }
            let effect = serde_json::from_str(&effect_json)
                .map_err(|error| MemoryError::Storage(error.into()))?;
            return Ok(MemoryMutationOutcome {
                effect,
                replayed: true,
            });
        }

        let mutation = crate::lifecycle::lower(mutation)?;
        let completing_index =
            matches!(&mutation, MemoryMutation::CompleteEmbeddingIndexing { .. });
        if let MemoryMutation::CompleteEmbeddingIndexing {
            fact,
            attempt_id,
            embedding,
        } = &mutation
        {
            crate::indexing::admit_completion(
                Self::indexing_record(&transaction, &fact.id)?.as_ref(),
                fact,
                embedding,
                Some(attempt_id),
            )?;
        }
        let capture = if let MemoryMutation::StoreCoveragePage { request, expected } = &mutation {
            let key = crate::formation::capture_key(request)?;
            let actual = Self::capture_cursor(&transaction, &key)?;
            let cursor =
                crate::formation::advance_capture(request, expected.as_ref(), actual.as_ref())?;
            Some((key, cursor))
        } else {
            None
        };
        let applicability = match &mutation {
            MemoryMutation::StoreApplicableFact { constraints, .. } => {
                Some(Box::new(RecordedApplicability::new(*constraints.clone())?))
            }
            _ => None,
        };
        let inference = match &mutation {
            MemoryMutation::StoreLifecycleInference { inference, .. } => Some(inference.clone()),
            _ => None,
        };
        let effect = match mutation {
            MemoryMutation::SetFactApplicability { fact, constraints } => {
                Self::check_fact_precondition(&transaction, &fact)?;
                let applicability = RecordedApplicability::new(*constraints)?;
                let encoded = serde_json::to_string(&applicability)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let version = Self::next_version_static(&transaction)?;
                transaction
                    .execute(
                        "UPDATE facts SET applicability=?1,version=?2 WHERE id=?3",
                        params![encoded, version as i64, fact.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::ApplicabilityUpdated {
                    fact: FactPrecondition {
                        id: fact.id,
                        expected_version: version,
                    },
                }
            }
            MemoryMutation::ConfirmLifecycleCandidate {
                candidate,
                snapshot_hash,
                session_id,
                request_id,
                surface,
                supersedes,
            } => {
                let mut fact = Self::check_fact_precondition(&transaction, &candidate)?;
                if let Some(target) = &supersedes {
                    let old = Self::check_fact_precondition(&transaction, target)?;
                    if old.mind != fact.mind || old.status != FactStatus::Active {
                        return Err(MemoryError::InvalidMutation(
                            "confirmation correction requires an active fact in the same mind"
                                .into(),
                        ));
                    }
                }
                crate::lifecycle::confirm_candidate(
                    &mut fact,
                    &snapshot_hash,
                    session_id,
                    request_id,
                    surface,
                    supersedes.as_ref(),
                )?;
                let mut original = None;
                if let Some(target) = supersedes {
                    let version = Self::next_version_static(&transaction)?;
                    transaction.execute("UPDATE facts SET status='superseded',version=?1,superseded_at=?2 WHERE id=?3",params![version as i64,fact.last_reinforced,target.id]).map_err(|error|MemoryError::Storage(error.into()))?;
                    original = Some(FactPrecondition {
                        id: target.id,
                        expected_version: version,
                    });
                }
                let version = Self::next_version_static(&transaction)?;
                let inference = serde_json::to_string(&fact.lifecycle_inference)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                transaction.execute("UPDATE facts SET status='active',confidence=1.0,reinforcement_count=1,last_reinforced=?1,version=?2,lifecycle_inference=?3,supersedes=?4 WHERE id=?5",
                    params![fact.last_reinforced,version as i64,inference,fact.superseded_by,candidate.id]).map_err(|error| MemoryError::Storage(error.into()))?;
                match original {
                    Some(original) => MemoryMutationEffect::FactSuperseded {
                        original,
                        replacement: FactPrecondition {
                            id: candidate.id,
                            expected_version: version,
                        },
                    },
                    None => MemoryMutationEffect::FactStored {
                        fact_id: candidate.id,
                        version,
                        action: StoreAction::Stored,
                    },
                }
            }
            MemoryMutation::StoreLifecycleConclusion { .. } => {
                return Err(MemoryError::InvalidMutation(
                    "unlowered lifecycle conclusion".into(),
                ));
            }
            MemoryMutation::ImportJsonl { jsonl } => {
                jsonl_import_effect(self.import_jsonl_transaction(&transaction, &jsonl)?)
            }
            MemoryMutation::StoreFact { request }
            | MemoryMutation::StoreApplicableFact { request, .. }
            | MemoryMutation::StoreLifecycleInference { request, .. } => {
                self.ensure_mind(&transaction, &request.mind)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let content_hash = hash::content_hash(&request.content);
                let mut candidates=transaction.prepare("SELECT * FROM facts WHERE mind = ?1 AND content_hash = ?2 AND status = 'active' AND ?3=0 AND (?4=0 OR (content=?5 AND source=?6 AND section=?7)) ORDER BY id").map_err(|error|MemoryError::Storage(error.into()))?;
                let rows = candidates
                    .query_map(
                        params![
                            request.mind,
                            content_hash,
                            inference.is_some(),
                            crate::lifecycle::requires_exact_source(request.source.as_deref()),
                            request.content,
                            request.source,
                            serde_json::to_string(&request.section)
                                .unwrap()
                                .trim_matches('"')
                        ],
                        Self::row_to_fact,
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let mut existing_id = None;
                for row in rows {
                    let existing = row.map_err(|error| MemoryError::Storage(error.into()))?;
                    if existing
                        .applicability
                        .as_ref()
                        .map(|record| &record.constraints)
                        == applicability.as_ref().map(|record| &record.constraints)
                    {
                        existing_id = Some(existing.id);
                        break;
                    }
                }
                drop(candidates);
                let version = Self::next_version_static(&transaction)?;
                if let Some(fact_id) = existing_id {
                    transaction.execute(
                        "UPDATE facts SET reinforcement_count = reinforcement_count + 1, last_reinforced = ?1, version = ?2 WHERE id = ?3",
                        params![now_iso(), version as i64, fact_id],
                    ).map_err(|error| MemoryError::Storage(error.into()))?;
                    MemoryMutationEffect::FactStored {
                        fact_id,
                        version,
                        action: StoreAction::Reinforced,
                    }
                } else {
                    let fact_id = gen_id();
                    let timestamp = now_iso();
                    let section = serde_json::to_string(&request.section)
                        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                    let profile = serde_json::to_string(&request.decay_profile)
                        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                    let pending = inference.is_some();
                    let encoded = inference
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    transaction.execute(
                        "INSERT INTO facts (id, mind, section, content, status, created_at, source, content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, decay_profile, version, lifecycle_inference) VALUES (?1,?2,?3,?4,?10,?5,?6,?7,?11,?5,?11,0.05,?8,?9,?12)",
                        params![fact_id, request.mind, section.trim_matches('"'), request.content,
                            timestamp, request.source.as_deref().unwrap_or("manual"), content_hash,
                            profile.trim_matches('"'), version as i64, if pending {"pending"} else {"active"}, if pending {0} else {1}, encoded],
                    ).map_err(|error| MemoryError::Storage(error.into()))?;
                    if let Some(applicability) = &applicability {
                        let encoded = serde_json::to_string(applicability)
                            .map_err(|error| MemoryError::Storage(error.into()))?;
                        transaction
                            .execute(
                                "UPDATE facts SET applicability=?1 WHERE id=?2",
                                params![encoded, fact_id],
                            )
                            .map_err(|error| MemoryError::Storage(error.into()))?;
                    }
                    MemoryMutationEffect::FactStored {
                        fact_id,
                        version,
                        action: StoreAction::Stored,
                    }
                }
            }
            MemoryMutation::ReinforceFact { fact } => {
                let existing = Self::check_fact_precondition(&transaction, &fact)?;
                if existing.status != FactStatus::Active {
                    return Err(MemoryError::FactNotFound(fact.id));
                }
                let version = Self::next_version_static(&transaction)?;
                transaction.execute(
                    "UPDATE facts SET reinforcement_count = reinforcement_count + 1, last_reinforced = ?1, version = ?2 WHERE id = ?3",
                    params![now_iso(), version as i64, fact.id],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::FactReinforced {
                    fact_id: fact.id,
                    version,
                    reinforcement_count: existing.reinforcement_count + 1,
                }
            }
            MemoryMutation::ReinforceFactOnce { fact_id } => {
                let existing = transaction
                    .query_row(
                        "SELECT * FROM facts WHERE id = ?1",
                        params![fact_id],
                        Self::row_to_fact,
                    )
                    .optional()
                    .map_err(|error| MemoryError::Storage(error.into()))?
                    .ok_or_else(|| MemoryError::FactNotFound(fact_id.clone()))?;
                if existing.status != FactStatus::Active {
                    return Err(MemoryError::FactNotFound(fact_id));
                }
                let version = Self::next_version_static(&transaction)?;
                transaction.execute(
                    "UPDATE facts SET reinforcement_count = reinforcement_count + 1, last_reinforced = ?1, version = ?2 WHERE id = ?3",
                    params![now_iso(), version as i64, fact_id],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::FactReinforced {
                    fact_id,
                    version,
                    reinforcement_count: existing.reinforcement_count + 1,
                }
            }
            MemoryMutation::TransitionFacts { facts, status } => {
                if !matches!(status, FactStatus::Dormant | FactStatus::Archived) {
                    return Err(MemoryError::InvalidMutation(
                        "transition target must be dormant or archived".into(),
                    ));
                }
                validate_unique_fact_preconditions(&facts)?;
                let mut existing = Vec::with_capacity(facts.len());
                for fact in &facts {
                    existing.push(Self::check_fact_precondition(&transaction, fact)?);
                }
                let status_json = serde_json::to_string(&status)
                    .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                let mut transitioned = Vec::new();
                for (fact, current) in facts.into_iter().zip(existing) {
                    if current.status != FactStatus::Active {
                        continue;
                    }
                    let version = Self::next_version_static(&transaction)?;
                    transaction
                        .execute(
                            "UPDATE facts SET status = ?1, version = ?2 WHERE id = ?3",
                            params![status_json.trim_matches('"'), version as i64, fact.id],
                        )
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    transitioned.push(FactPrecondition {
                        id: fact.id,
                        expected_version: version,
                    });
                }
                MemoryMutationEffect::FactsTransitioned {
                    facts: transitioned,
                    status,
                }
            }
            MemoryMutation::SupersedeFact { fact, replacement } => {
                let existing = Self::check_fact_precondition(&transaction, &fact)?;
                if existing.mind != replacement.mind {
                    return Err(MemoryError::InvalidMutation(
                        "supersession must remain in the same mind".into(),
                    ));
                }
                if existing.status != FactStatus::Active {
                    return Err(MemoryError::FactNotFound(fact.id));
                }
                self.ensure_mind(&transaction, &replacement.mind)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let original_version = Self::next_version_static(&transaction)?;
                transaction
                    .execute(
                        "UPDATE facts SET status = 'superseded', version = ?1 WHERE id = ?2",
                        params![original_version as i64, fact.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let replacement_version = original_version + 1;
                let replacement_version_sql = persisted_lamport_version(replacement_version)?;
                let replacement_id = gen_id();
                let timestamp = now_iso();
                let section = serde_json::to_string(&replacement.section)
                    .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                let profile = serde_json::to_string(&replacement.decay_profile)
                    .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
                let content_hash = hash::content_hash(&replacement.content);
                transaction.execute(
                    "INSERT INTO facts (id, mind, section, content, status, created_at, source, content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, decay_profile, version, supersedes) VALUES (?1,?2,?3,?4,'active',?5,?6,?7,1.0,?5,1,0.05,?8,?9,?10)",
                    params![replacement_id, replacement.mind, section.trim_matches('"'), replacement.content,
                        timestamp, replacement.source.as_deref().unwrap_or("manual"), content_hash,
                        profile.trim_matches('"'), replacement_version_sql, fact.id],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::FactSuperseded {
                    original: FactPrecondition {
                        id: fact.id,
                        expected_version: original_version,
                    },
                    replacement: FactPrecondition {
                        id: replacement_id,
                        expected_version: replacement_version,
                    },
                }
            }
            MemoryMutation::SupersedeFactWithExisting { fact, replacement } => {
                if fact.id == replacement.id {
                    return Err(MemoryError::InvalidMutation(
                        "a fact cannot supersede itself".into(),
                    ));
                }
                let original = Self::check_fact_precondition(&transaction, &fact)?;
                let existing = Self::check_fact_precondition(&transaction, &replacement)?;
                if original.status != FactStatus::Active
                    || existing.status != FactStatus::Active
                    || original.mind != existing.mind
                {
                    return Err(MemoryError::InvalidMutation(
                        "supersession requires distinct active facts in the same mind".into(),
                    ));
                }
                let original_version = Self::next_version_static(&transaction)?;
                transaction
                    .execute(
                        "UPDATE facts SET status = 'superseded', version = ?1 WHERE id = ?2",
                        params![original_version as i64, fact.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let replacement_version = original_version + 1;
                let replacement_version_sql = persisted_lamport_version(replacement_version)?;
                transaction
                    .execute(
                        "UPDATE facts SET supersedes = ?1, version = ?2 WHERE id = ?3",
                        params![fact.id, replacement_version_sql, replacement.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::FactSuperseded {
                    original: FactPrecondition {
                        id: fact.id,
                        expected_version: original_version,
                    },
                    replacement: FactPrecondition {
                        id: replacement.id,
                        expected_version: replacement_version,
                    },
                }
            }
            MemoryMutation::StoreEmbedding {
                fact,
                model_name,
                embedding,
            } => {
                if Self::check_fact_precondition(&transaction, &fact)?.status == FactStatus::Pending
                {
                    return Err(MemoryError::InvalidMutation(
                        "pending candidates cannot be indexed".into(),
                    ));
                }
                let dims = embedding.len() as u32;
                let recorded_dims: Option<u32> = transaction
                    .query_row(
                        "SELECT dims FROM embedding_metadata WHERE model_name = ?1",
                        params![model_name],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                if let Some(expected) = recorded_dims
                    && expected != dims
                {
                    return Err(MemoryError::EmbeddingDimensionMismatch {
                        expected,
                        got: dims,
                        stored_model: model_name,
                    });
                }
                let timestamp = now_iso();
                transaction.execute(
                    "INSERT OR REPLACE INTO facts_vec (fact_id, embedding, model_name, dims, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![fact.id, vectors::vector_to_blob(&embedding), model_name, dims, timestamp],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                transaction.execute(
                    "INSERT OR IGNORE INTO embedding_metadata (model_name, dims, inserted_at) VALUES (?1, ?2, ?3)",
                    params![model_name, dims, timestamp],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::EmbeddingStored {
                    fact_id: fact.id,
                    model_name,
                    dims,
                }
            }
            MemoryMutation::RecordEmbeddingIndexing { record } => {
                let source = Self::check_fact_precondition(&transaction, &record.fact)?;
                if source.status != FactStatus::Active {
                    return Err(MemoryError::InvalidMutation(
                        "embedding source must be active".into(),
                    ));
                }
                crate::indexing::admit_record(
                    Self::indexing_record(&transaction, &record.fact.id)?.as_ref(),
                    &record,
                )?;
                let json = serde_json::to_string(&record)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let reason = serde_json::to_string(&record.reason)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                transaction.execute("INSERT OR REPLACE INTO embedding_indexing (fact_id,fact_version,reason,record_json) VALUES (?1,?2,?3,?4)",
                    params![record.fact.id, record.fact.expected_version, reason, json])
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::EmbeddingIndexingRecorded {
                    fact: record.fact,
                    attempt_id: record.attempt_id,
                }
            }
            MemoryMutation::StoreIdentifiedEmbedding { fact, embedding }
            | MemoryMutation::CompleteEmbeddingIndexing {
                fact, embedding, ..
            } => {
                embedding.validate()?;
                let source = Self::check_fact_precondition(&transaction, &fact)?;
                crate::indexing::admit_completion(
                    Self::indexing_record(&transaction, &fact.id)?.as_ref(),
                    &fact,
                    &embedding,
                    None,
                )?;
                if source.status != FactStatus::Active {
                    return Err(MemoryError::InvalidMutation(
                        "embedding source must be active".into(),
                    ));
                }
                let space = serde_json::to_string(&embedding.space)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let source_hash = crate::retrieval::raw_content_hash(&source.content);
                let timestamp = now_iso();
                let unchanged = completing_index && transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM facts_vec WHERE fact_id=?1 AND space=?2 AND source_hash=?3 AND model_name=?4 AND dims=?5 AND embedding=?6)",
                    params![fact.id, space, source_hash, embedding.space.model, embedding.space.dimensions, vectors::vector_to_blob(&embedding.values)],
                    |row| row.get::<_, bool>(0),
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                if !unchanged {
                    transaction.execute("INSERT OR REPLACE INTO facts_vec (fact_id,embedding,model_name,dims,created_at,space,source_hash) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![fact.id, vectors::vector_to_blob(&embedding.values), embedding.space.model, embedding.space.dimensions, timestamp, space, source_hash])
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                }
                transaction.execute("INSERT OR IGNORE INTO embedding_metadata (model_name,dims,inserted_at) VALUES (?1,?2,?3)", params![embedding.space.model,embedding.space.dimensions,timestamp])
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                transaction
                    .execute(
                        "DELETE FROM embedding_indexing WHERE fact_id = ?1",
                        params![fact.id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::EmbeddingStored {
                    fact_id: fact.id,
                    model_name: embedding.space.model,
                    dims: embedding.space.dimensions,
                }
            }
            MemoryMutation::CreateEdge { mind, request } => {
                for fact_id in [&request.source_id, &request.target_id] {
                    let endpoint: Option<(String, String)> = transaction
                        .query_row(
                            "SELECT mind, status FROM facts WHERE id = ?1",
                            params![fact_id],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .optional()
                        .map_err(|error| MemoryError::Storage(error.into()))?;
                    let Some((endpoint_mind, status)) = endpoint else {
                        return Err(MemoryError::FactNotFound(fact_id.clone()));
                    };
                    if endpoint_mind != mind || status != "active" {
                        return Err(MemoryError::InvalidMutation(format!(
                            "edge endpoint {fact_id} is outside active mind {mind}"
                        )));
                    }
                }
                let edge_id = gen_id();
                transaction.execute(
                    "INSERT INTO edges (id, source_fact_id, target_fact_id, relation, description, confidence, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 1.0, ?6)",
                    params![edge_id, request.source_id, request.target_id, request.relation, request.description, now_iso()],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::EdgeCreated { edge_id }
            }
            MemoryMutation::StoreEpisode { request }
            | MemoryMutation::StoreCoveragePage { request, .. } => {
                if let Some(formation) = &request.formation {
                    formation.validate()?;
                }
                self.ensure_mind(&transaction, &request.mind)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let episode_id = gen_id();
                let timestamp = now_iso();
                let date = request.date.unwrap_or_else(|| timestamp[..10].to_string());
                transaction.execute(
                    "INSERT INTO episodes (id, mind, title, narrative, date, created_at, affected_nodes, affected_changes, files_changed, tags, tool_calls_count, formation) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![episode_id, request.mind, request.title, request.narrative, date, timestamp,
                        serde_json::to_string(&request.affected_nodes).unwrap_or_else(|_| "[]".into()),
                        serde_json::to_string(&request.affected_changes).unwrap_or_else(|_| "[]".into()),
                        serde_json::to_string(&request.files_changed).unwrap_or_else(|_| "[]".into()),
                        serde_json::to_string(&request.tags).unwrap_or_else(|_| "[]".into()),
                        request.tool_calls_count, request.formation.as_ref().map(serde_json::to_string).transpose().map_err(|error| MemoryError::Storage(error.into()))?],
                ).map_err(|error| MemoryError::Storage(error.into()))?;
                match capture {
                    Some((key, cursor)) => MemoryMutationEffect::CoverageStored {
                        episode_id,
                        key,
                        cursor,
                    },
                    None => MemoryMutationEffect::EpisodeStored { episode_id },
                }
            }
            MemoryMutation::CompleteFormation {
                episode_id,
                formation,
            } => {
                let episode = transaction
                    .query_row(
                        "SELECT * FROM episodes WHERE id = ?1",
                        params![episode_id],
                        Self::row_to_episode,
                    )
                    .optional()
                    .map_err(|error| MemoryError::Storage(error.into()))?
                    .ok_or_else(|| {
                        MemoryError::InvalidMutation("formation episode not found".into())
                    })?;
                crate::formation::validate_completion(episode.formation.as_deref(), &formation)?;
                let encoded = serde_json::to_string(&formation)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                transaction
                    .execute(
                        "UPDATE episodes SET formation = ?1, narrative = ?2 WHERE id = ?3",
                        params![encoded, formation.narrative(), episode_id],
                    )
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                MemoryMutationEffect::FormationCompleted { episode_id }
            }
        };

        let effect_json =
            serde_json::to_string(&effect).map_err(|error| MemoryError::Storage(error.into()))?;
        transaction.execute(
            "INSERT INTO memory_operation_receipts (operation_id, payload_hash, effect_json, committed_at) VALUES (?1, ?2, ?3, ?4)",
            params![operation_id, payload_hash, effect_json, now_iso()],
        ).map_err(|error| MemoryError::Storage(error.into()))?;
        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(MemoryMutationOutcome {
            effect,
            replayed: false,
        })
    }

    async fn store_fact(&self, req: StoreFact) -> Result<StoreResult> {
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        self.ensure_mind(&transaction, &req.mind)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let ch = hash::content_hash(&req.content);
        let section_str = serde_json::to_string(&req.section).unwrap_or_default();
        let section_str = section_str.trim_matches('"');
        let profile_str = serde_json::to_string(&req.decay_profile).unwrap_or_default();
        let profile_str = profile_str.trim_matches('"');

        // Check dedup
        let existing: Option<String> = transaction
            .query_row(
                "SELECT id FROM facts WHERE mind = ?1 AND content_hash = ?2 AND status = 'active' AND applicability IS NULL",
                params![req.mind, ch],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| MemoryError::Storage(e.into()))?;

        if let Some(id) = existing {
            let version = Self::next_version_static(&transaction)?;
            let ts = now_iso();
            transaction
                .execute(
                    "UPDATE facts SET reinforcement_count = reinforcement_count + 1, \
                 last_reinforced = ?1, version = ?2 WHERE id = ?3",
                    params![ts, version as i64, id],
                )
                .map_err(|e| MemoryError::Storage(e.into()))?;

            let fact = transaction
                .query_row(
                    "SELECT * FROM facts WHERE id = ?1",
                    params![id],
                    Self::row_to_fact,
                )
                .map_err(|e| MemoryError::Storage(e.into()))?;
            transaction
                .commit()
                .map_err(|error| MemoryError::Storage(error.into()))?;
            return Ok(StoreResult {
                fact,
                action: StoreAction::Reinforced,
            });
        }

        let id = gen_id();
        let ts = now_iso();
        let version = Self::next_version_static(&transaction)?;
        transaction
            .execute(
                "INSERT INTO facts (id, mind, section, content, status, created_at, source, \
             content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, \
             decay_profile, version) VALUES (?1,?2,?3,?4,'active',?5,?6,?7,1.0,?5,1,0.05,?8,?9)",
                params![
                    id,
                    req.mind,
                    section_str,
                    req.content,
                    ts,
                    req.source.as_deref().unwrap_or("manual"),
                    ch,
                    profile_str,
                    version as i64
                ],
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;

        let fact = transaction
            .query_row(
                "SELECT * FROM facts WHERE id = ?1",
                params![id],
                Self::row_to_fact,
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(StoreResult {
            fact,
            action: StoreAction::Stored,
        })
    }

    async fn get_fact(&self, id: &str) -> Result<Option<Fact>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM facts WHERE id = ?1 AND status = 'active'",
            params![id],
            Self::row_to_fact,
        )
        .optional()
        .map_err(|e| MemoryError::Storage(e.into()))
    }

    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>> {
        let conn = self.conn.lock().unwrap();
        let status_str = filter
            .status
            .as_ref()
            .map(|s| {
                serde_json::to_string(s)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string()
            })
            .unwrap_or_else(|| "active".into());

        let (sql, section_param);
        if let Some(ref sec) = filter.section {
            section_param = serde_json::to_string(sec)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            sql = "SELECT * FROM facts WHERE mind = ?1 AND status = ?2 AND section = ?3 ORDER BY created_at DESC";
            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| MemoryError::Storage(e.into()))?;
            let facts = stmt
                .query_map(params![mind, status_str, section_param], Self::row_to_fact)
                .map_err(|e| MemoryError::Storage(e.into()))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| MemoryError::Storage(error.into()))?;
            Ok(facts)
        } else {
            sql = "SELECT * FROM facts WHERE mind = ?1 AND status = ?2 ORDER BY created_at DESC";
            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| MemoryError::Storage(e.into()))?;
            let facts = stmt
                .query_map(params![mind, status_str], Self::row_to_fact)
                .map_err(|e| MemoryError::Storage(e.into()))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| MemoryError::Storage(error.into()))?;
            Ok(facts)
        }
    }

    async fn list_facts_page(
        &self,
        mind: &str,
        filter: FactFilter,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<FactPage> {
        let conn = self.conn.lock().unwrap();
        let (watermark, after) = match cursor {
            Some(cursor) => {
                let (version, id) = cursor.split_once(':').ok_or_else(|| {
                    MemoryError::InvalidMutation("invalid fact-page cursor".into())
                })?;
                let rowid = version
                    .parse::<u64>()
                    .map_err(|_| MemoryError::InvalidMutation("invalid fact-page cursor".into()))?;
                (rowid, id)
            }
            None => {
                let watermark = conn
                    .query_row("SELECT COALESCE(MAX(rowid), 0) FROM facts", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .map_err(|error| MemoryError::Storage(error.into()))?
                    as u64;
                (watermark, "")
            }
        };
        let status = filter.status.unwrap_or(FactStatus::Active);
        let status = serde_json::to_string(&status)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let section = filter.section.map(|section| {
            serde_json::to_string(&section)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string()
        });
        let total = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = ?2 AND rowid <= ?3 AND (?4 IS NULL OR section = ?4)",
                params![mind, status, watermark as i64, section],
                |row| row.get(0),
            )
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let mut statement = conn
            .prepare(
                "SELECT * FROM facts WHERE mind = ?1 AND status = ?2 AND rowid <= ?3 AND id > ?4 AND (?5 IS NULL OR section = ?5) ORDER BY id ASC LIMIT ?6",
            )
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let facts = statement
            .query_map(
                params![
                    mind,
                    status,
                    watermark as i64,
                    after,
                    section,
                    limit.saturating_add(1) as i64
                ],
                Self::row_to_fact,
            )
            .map_err(|error| MemoryError::Storage(error.into()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let has_more = facts.len() > limit;
        let facts = facts.into_iter().take(limit).collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| facts.last().map(|fact| format!("{watermark}:{}", fact.id)))
            .flatten();
        Ok(FactPage {
            facts,
            next_cursor,
            total,
        })
    }

    async fn reinforce_fact(&self, id: &str) -> Result<Fact> {
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let version = Self::next_version_static(&transaction)?;
        let ts = now_iso();
        let updated = transaction
            .execute(
                "UPDATE facts SET reinforcement_count = reinforcement_count + 1, \
             last_reinforced = ?1, version = ?2 WHERE id = ?3 AND status = 'active'",
                params![ts, version as i64, id],
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        if updated == 0 {
            return Err(MemoryError::FactNotFound(id.into()));
        }
        let fact = transaction
            .query_row(
                "SELECT * FROM facts WHERE id = ?1",
                params![id],
                Self::row_to_fact,
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(fact)
    }

    async fn dormancy_facts(&self, ids: &[&str]) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let mut transitioned = 0;
        for id in ids {
            let version = Self::next_version_static(&transaction)?;
            transitioned += transaction
                .execute(
                    "UPDATE facts SET status = 'dormant', version = ?1 WHERE id = ?2 AND status = 'active'",
                    params![version as i64, id],
                )
                .map_err(|error| MemoryError::Storage(error.into()))?;
        }
        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(transitioned)
    }

    async fn archive_facts(&self, ids: &[&str]) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let mut count = 0usize;
        for id in ids {
            let version = Self::next_version_static(&transaction)?;
            let n = transaction.execute(
                "UPDATE facts SET status = 'archived', version = ?1 WHERE id = ?2 AND status = 'active'",
                params![version as i64, id],
            ).map_err(|e| MemoryError::Storage(e.into()))?;
            count += n;
        }
        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(count)
    }

    async fn supersede_fact(&self, id: &str, replacement: StoreFact) -> Result<Fact> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| MemoryError::Storage(e.into()))?;
        self.ensure_mind(&tx, &replacement.mind)
            .map_err(|error| MemoryError::Storage(error.into()))?;

        // Check original exists inside the write transaction.
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM facts WHERE id = ?1 AND status = 'active'",
                params![id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| MemoryError::Storage(e.into()))?
            .unwrap_or(false);
        if !exists {
            return Err(MemoryError::FactNotFound(id.into()));
        }

        let new_id = gen_id();
        let version = Self::next_version_static(&tx)?;

        // Archive original — matches TS behavior
        tx.execute(
            "UPDATE facts SET status = 'superseded', version = ?1 WHERE id = ?2",
            params![version as i64, id],
        )
        .map_err(|e| MemoryError::Storage(e.into()))?;

        // Insert replacement
        let section_str = serde_json::to_string(&replacement.section).unwrap_or_default();
        let section_str = section_str.trim_matches('"');
        let profile_str = serde_json::to_string(&replacement.decay_profile).unwrap_or_default();
        let profile_str = profile_str.trim_matches('"');
        let ch = hash::content_hash(&replacement.content);
        let ts = now_iso();
        let version2 = version + 1;
        let version2_sql = persisted_lamport_version(version2)?;

        tx.execute(
            "INSERT INTO facts (id, mind, section, content, status, created_at, source, \
             content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, \
             decay_profile, version, supersedes) VALUES (?1,?2,?3,?4,'active',?5,?6,?7,1.0,?5,1,0.05,?8,?9,?10)",
            params![new_id, replacement.mind, section_str, replacement.content, ts,
                    replacement.source.as_deref().unwrap_or("manual"), ch, profile_str, version2_sql, id],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let fact = tx
            .query_row(
                "SELECT * FROM facts WHERE id = ?1",
                params![new_id],
                Self::row_to_fact,
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;

        tx.commit().map_err(|e| MemoryError::Storage(e.into()))?;
        Ok(fact)
    }

    async fn superseding_fact(&self, old_id: &str) -> Result<Option<Fact>> {
        let conn = self.conn.lock().unwrap();
        let original: Option<Fact> = conn
            .query_row(
                "SELECT * FROM facts WHERE id = ?1",
                params![old_id],
                Self::row_to_fact,
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let Some(original) = original else {
            return Ok(None);
        };
        if original.status != FactStatus::Superseded {
            return Ok(None);
        }
        let mut predecessor = old_id.to_owned();
        let mut visited = HashSet::new();
        visited.insert(predecessor.clone());
        loop {
            let replacement: Option<Fact> = conn
                .query_row(
                    "SELECT * FROM facts WHERE supersedes = ?1 ORDER BY version DESC, id DESC LIMIT 1",
                    params![predecessor],
                    Self::row_to_fact,
                )
                .optional()
                .map_err(|error| MemoryError::Storage(error.into()))?;
            let Some(replacement) = replacement else {
                break;
            };
            if !visited.insert(replacement.id.clone()) {
                return Err(MemoryError::Storage(anyhow::anyhow!(
                    "supersession cycle detected"
                )));
            }
            if replacement.status == FactStatus::Active {
                return Ok(Some(replacement));
            }
            if replacement.status != FactStatus::Superseded {
                break;
            }
            predecessor = replacement.id;
        }

        let Some(source) = original
            .source
            .as_deref()
            .filter(|source| source.starts_with("codex-vault:"))
        else {
            return Ok(None);
        };
        let latest: Option<Fact> = conn
            .query_row(
                "SELECT * FROM facts WHERE source = ?1 ORDER BY version DESC, id DESC LIMIT 1",
                params![source],
                Self::row_to_fact,
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let Some(latest) = latest else {
            return Ok(None);
        };
        if latest.status == FactStatus::Active {
            return Ok(Some(latest));
        }
        if latest.status != FactStatus::Superseded {
            return Ok(None);
        }
        conn.query_row(
            "SELECT * FROM facts WHERE supersedes = ?1 AND status = 'active' ORDER BY version DESC, id DESC LIMIT 1",
            params![latest.id],
            Self::row_to_fact,
        )
        .optional()
        .map_err(|error| MemoryError::Storage(error.into()))
    }

    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>> {
        self.fts_search_filtered(mind, query, k, &SearchFilter::default())
            .await
    }

    async fn fts_search_filtered(
        &self,
        mind: &str,
        query: &str,
        k: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<ScoredFact>> {
        let filter = filter.resolved()?;
        let conn = self.conn.lock().unwrap();
        // Use FTS5 OR mode for broader matching
        let fts_query = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() || k == 0 {
            return Ok(Vec::new());
        }
        let section = filter.section.as_ref().map(|section| {
            serde_json::to_string(section)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string()
        });

        let mut stmt = conn
            .prepare(
                "SELECT f.*, rank FROM facts_fts fts \
             JOIN facts f ON f.id = fts.id \
             WHERE facts_fts MATCH ?1 AND f.mind = ?2 \
              AND ((?3 = 0 AND f.status = 'active') OR (?3 = 1 AND f.status IN ('archived','dormant','superseded'))) \
              AND (?4 IS NULL OR f.section = ?4) \
              ORDER BY rank, f.id",
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;

        let mut rows = stmt
            .query(params![
                fts_query,
                mind,
                filter.intent == SearchIntent::Historical,
                section
            ])
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let mut results = Vec::new();
        while let Some(row) = rows
            .next()
            .map_err(|error| MemoryError::Storage(error.into()))?
        {
            let fact =
                Self::row_to_fact(row).map_err(|error| MemoryError::Storage(error.into()))?;
            let relevance = -row
                .get::<_, f64>("rank")
                .map_err(|error| MemoryError::Storage(error.into()))?;
            let Some(score) = filter.score(relevance, &fact) else {
                continue;
            };
            let applicability = filter.applicability_status(&fact);
            let mut result = ScoredFact::new(fact, relevance, score);
            result.applicability = applicability;
            result.scores.lexical = Some(relevance);
            results.push(result);
            if results.len() >= k.saturating_mul(8) {
                break;
            }
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.fact.id.cmp(&b.fact.id))
        });
        results.truncate(k);

        Ok(results)
    }

    async fn vector_search(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
    ) -> Result<Vec<ScoredFact>> {
        let _ = (mind, embedding, k, min_similarity);
        Err(MemoryError::EmbeddingIdentityRequired)
    }

    async fn vector_search_cancellable(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<ScoredFact>> {
        self.vector_search_filtered_cancellable(
            mind,
            embedding,
            k,
            min_similarity,
            &SearchFilter::default(),
            cancelled,
        )
        .await
    }

    async fn vector_search_filtered_cancellable(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
        filter: &SearchFilter,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<ScoredFact>> {
        let _ = (mind, embedding, k, min_similarity, filter);
        if cancelled() {
            return Err(MemoryError::Cancelled);
        }
        Err(MemoryError::EmbeddingIdentityRequired)
    }

    async fn store_embedding(
        &self,
        fact_id: &str,
        model_name: &str,
        embedding: &[f32],
    ) -> Result<()> {
        validate_embedding(embedding)?;
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let blob = vectors::vector_to_blob(embedding);
        let ts = now_iso();
        let dims = embedding.len() as i64;

        let fact_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM facts WHERE id = ?1 AND status != 'pending')",
                params![fact_id],
                |row| row.get(0),
            )
            .map_err(|error| MemoryError::Storage(error.into()))?;
        if !fact_exists {
            return Err(MemoryError::FactNotFound(fact_id.into()));
        }
        let recorded_dims: Option<u32> = transaction
            .query_row(
                "SELECT dims FROM embedding_metadata WHERE model_name = ?1",
                params![model_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        if let Some(expected) = recorded_dims
            && expected != embedding.len() as u32
        {
            return Err(MemoryError::EmbeddingDimensionMismatch {
                expected,
                got: embedding.len() as u32,
                stored_model: model_name.into(),
            });
        }

        transaction.execute(
            "INSERT OR REPLACE INTO facts_vec (fact_id, embedding, model_name, dims, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![fact_id, blob, model_name, dims, ts],
        )
        .map_err(|e| MemoryError::Storage(e.into()))?;

        transaction.execute(
            "INSERT OR IGNORE INTO embedding_metadata (model_name, dims, inserted_at) VALUES (?1, ?2, ?3)",
            params![model_name, dims, ts],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(())
    }

    async fn embedding_metadata(&self, mind: &str) -> Result<Option<EmbeddingMetadata>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT fv.model_name, fv.dims, fv.created_at FROM facts_vec fv \
             JOIN facts f ON f.id = fv.fact_id \
             WHERE f.mind = ?1 LIMIT 1",
            params![mind],
            |row| {
                Ok(EmbeddingMetadata {
                    model_name: row.get(0)?,
                    dims: row.get(1)?,
                    inserted_at: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| MemoryError::Storage(e.into()))
    }

    async fn search_identified(
        &self,
        mind: &str,
        query: &IdentifiedEmbedding,
        k: usize,
        minimum: f32,
        filter: &SearchFilter,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<VectorSearchReport> {
        let filter = filter.resolved()?;
        let mut top = crate::retrieval::VectorAccumulator::new(query, k, minimum)?;
        if cancelled() {
            return Err(MemoryError::Cancelled);
        }
        if k == 0 {
            return Ok(top.finish());
        }
        let conn = self.conn.lock().unwrap();
        let section = filter.section.as_ref().map(|section| {
            serde_json::to_string(section)
                .unwrap()
                .trim_matches('"')
                .to_string()
        });
        let mut statement = conn.prepare("SELECT f.*, fv.embedding, fv.space, fv.source_hash, fv.dims AS vector_dims, fv.model_name AS vector_model FROM facts f JOIN facts_vec fv ON fv.fact_id=f.id WHERE f.mind=?1 AND ((?2=0 AND f.status='active') OR (?2=1 AND f.status IN ('dormant','archived','superseded'))) AND (?3 IS NULL OR f.section=?3) ORDER BY f.id")
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let mut rows = statement
            .query(params![
                mind,
                filter.intent == SearchIntent::Historical,
                section
            ])
            .map_err(|error| MemoryError::Storage(error.into()))?;
        while let Some(row) = rows
            .next()
            .map_err(|error| MemoryError::Storage(error.into()))?
        {
            if cancelled() {
                return Err(MemoryError::Cancelled);
            }
            let fact =
                Self::row_to_fact(row).map_err(|error| MemoryError::Storage(error.into()))?;
            if !filter.matches(&fact) {
                continue;
            }
            let space = crate::retrieval::stored_space(
                row.get("space")
                    .map_err(|error| MemoryError::Storage(error.into()))?,
            )?;
            let source_hash: Option<String> = row
                .get("source_hash")
                .map_err(|error| MemoryError::Storage(error.into()))?;
            if let Some(space) = &space {
                let dims: u32 = row
                    .get("vector_dims")
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                let model: String = row
                    .get("vector_model")
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                if dims != space.dimensions || model != space.model {
                    return Err(MemoryError::Storage(anyhow::anyhow!(
                        "inconsistent vector metadata"
                    )));
                }
            }
            if !top.eligible(crate::retrieval::index_state(
                space.as_ref(),
                source_hash.as_deref(),
                &fact.content,
                &query.space,
            )) {
                continue;
            }
            let blob: Vec<u8> = row
                .get("embedding")
                .map_err(|error| MemoryError::Storage(error.into()))?;
            let vector = crate::retrieval::decode(&blob, &query.space)?;
            let similarity = vectors::cosine_similarity(&vector, &query.values);
            if similarity >= minimum {
                top.push(fact, similarity as f64, &filter);
            }
        }
        Ok(top.finish())
    }

    async fn embedding_indexing_record(
        &self,
        fact_id: &str,
    ) -> Result<Option<EmbeddingIndexingRecord>> {
        Self::indexing_record(&self.conn.lock().unwrap(), fact_id)
    }

    async fn embedding_indexing_summary(&self, mind: &str) -> Result<EmbeddingIndexingSummary> {
        let conn = self.conn.lock().unwrap();
        let mut summary = EmbeddingIndexingSummary::default();
        let mut stmt = conn.prepare("SELECT i.reason, f.version != i.fact_version AS stale, COUNT(*) FROM embedding_indexing i JOIN facts f ON f.id = i.fact_id WHERE f.mind = ?1 AND f.status = 'active' GROUP BY i.reason, stale")
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let rows = stmt
            .query_map(params![mind], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            })
            .map_err(|error| MemoryError::Storage(error.into()))?;
        for row in rows {
            let (reason, stale, count) = row.map_err(|error| MemoryError::Storage(error.into()))?;
            let reason = if stale {
                EmbeddingIndexingReason::SourceChanged
            } else {
                serde_json::from_str(&reason).map_err(|error| MemoryError::Storage(error.into()))?
            };
            summary.observe(reason, count);
        }
        summary.untracked = conn.query_row("SELECT COUNT(*) FROM facts f LEFT JOIN embedding_indexing i ON i.fact_id = f.id LEFT JOIN facts_vec v ON v.fact_id = f.id WHERE f.mind = ?1 AND f.status = 'active' AND i.fact_id IS NULL AND (v.fact_id IS NULL OR v.space IS NULL)", params![mind], |row| row.get(0))
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(summary)
    }

    async fn embedding_index_state(
        &self,
        id: &str,
        space: &EmbeddingSpace,
    ) -> Result<EmbeddingIndexState> {
        space.validate()?;
        let conn = self.conn.lock().unwrap();
        type IndexRow = (
            String,
            Option<String>,
            Option<String>,
            Option<Vec<u8>>,
            Option<u32>,
            Option<String>,
        );
        let row: Option<IndexRow> = conn.query_row(
            "SELECT f.content,v.space,v.source_hash,v.embedding,v.dims,v.model_name FROM facts f LEFT JOIN facts_vec v ON v.fact_id=f.id WHERE f.id=?1", params![id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).optional().map_err(|error| MemoryError::Storage(error.into()))?;
        let Some((content, encoded, hash, blob, dims, model)) = row else {
            return Err(MemoryError::FactNotFound(id.into()));
        };
        let Some(blob) = blob else {
            return Ok(EmbeddingIndexState::Missing);
        };
        let stored = crate::retrieval::stored_space(encoded)?;
        if stored.as_ref().is_some_and(|stored| {
            dims != Some(stored.dimensions) || model.as_deref() != Some(stored.model.as_str())
        }) {
            return Err(MemoryError::Storage(anyhow::anyhow!(
                "inconsistent vector metadata"
            )));
        }
        let state =
            crate::retrieval::index_state(stored.as_ref(), hash.as_deref(), &content, space);
        if state == EmbeddingIndexState::Ready {
            crate::retrieval::decode(&blob, space)?;
        }
        Ok(state)
    }

    async fn get_fact_filtered(
        &self,
        mind: &str,
        id: &str,
        filter: &SearchFilter,
    ) -> Result<Option<Fact>> {
        let filter = filter.resolved()?;
        let conn = self.conn.lock().unwrap();
        let fact = conn
            .query_row(
                "SELECT * FROM facts WHERE id=?1 AND mind=?2",
                params![id, mind],
                Self::row_to_fact,
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(fact.filter(|fact| filter.matches(fact)))
    }

    async fn get_edges_filtered(
        &self,
        mind: &str,
        id: &str,
        filter: &SearchFilter,
        limit: usize,
    ) -> Result<Vec<Edge>> {
        let filter = filter.resolved()?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let section = filter.section.as_ref().map(|section| {
            serde_json::to_string(section)
                .unwrap()
                .trim_matches('"')
                .to_string()
        });
        let mut stmt = conn.prepare("SELECT e.*,s.applicability AS source_app,t.applicability AS target_app FROM edges e JOIN facts s ON s.id=e.source_fact_id JOIN facts t ON t.id=e.target_fact_id WHERE (s.id=?1 OR t.id=?1) AND s.mind=?2 AND t.mind=?2 AND e.status='active' AND ((?3=0 AND s.status='active' AND t.status='active') OR (?3=1 AND s.status IN ('archived','dormant','superseded') AND t.status IN ('archived','dormant','superseded'))) AND (?4 IS NULL OR (s.section=?4 AND t.section=?4)) ORDER BY e.confidence DESC,e.id")
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let rows = stmt
            .query_map(
                params![id, mind, filter.intent == SearchIntent::Historical, section],
                |row| {
                    Ok((
                        Edge {
                            id: row.get("id")?,
                            source_id: row.get("source_fact_id")?,
                            target_id: row.get("target_fact_id")?,
                            relation: row.get("relation")?,
                            description: row.get("description")?,
                            confidence: row.get("confidence")?,
                            created_at: row.get("created_at")?,
                        },
                        row.get::<_, Option<String>>("source_app")?,
                        row.get::<_, Option<String>>("target_app")?,
                    ))
                },
            )
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let mut edges = Vec::new();
        for row in rows {
            let (edge, source, target) = row.map_err(|error| MemoryError::Storage(error.into()))?;
            let mut eligible = true;
            for encoded in [source, target].into_iter().flatten() {
                let record: RecordedApplicability = serde_json::from_str(&encoded)
                    .map_err(|error| MemoryError::Storage(error.into()))?;
                record.validate()?;
                if record
                    .constraints
                    .assess(filter.context.as_ref().expect("resolved context"))
                    == ApplicabilityStatus::Inapplicable
                {
                    eligible = false;
                }
            }
            if eligible {
                edges.push(edge);
                if edges.len() >= limit.min(1024) {
                    break;
                }
            }
        }
        Ok(edges)
    }

    async fn create_edge(&self, req: CreateEdge) -> Result<Edge> {
        let conn = self.conn.lock().unwrap();
        let endpoint = |id: &str| {
            conn.query_row(
                "SELECT mind, status FROM facts WHERE id = ?1",
                params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?
            .ok_or_else(|| MemoryError::FactNotFound(id.into()))
        };
        let source = endpoint(&req.source_id)?;
        let target = endpoint(&req.target_id)?;
        if source.0 != target.0 || source.1 != "active" || target.1 != "active" {
            return Err(MemoryError::InvalidMutation(
                "edge endpoints must be active facts in the same mind".into(),
            ));
        }
        let id = gen_id();
        let ts = now_iso();
        conn.execute(
            "INSERT INTO edges (id, source_fact_id, target_fact_id, relation, description, confidence, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 1.0, ?6)",
            params![id, req.source_id, req.target_id, req.relation, req.description, ts],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        Ok(Edge {
            id,
            source_id: req.source_id,
            target_id: req.target_id,
            relation: req.relation,
            description: req.description,
            confidence: 1.0,
            created_at: ts,
        })
    }

    async fn get_edges(&self, mind: &str, fact_id: &str) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT edges.* FROM edges \
                 JOIN facts source ON source.id = source_fact_id \
                 JOIN facts target ON target.id = target_fact_id \
                 WHERE (source_fact_id = ?1 OR target_fact_id = ?1) \
                 AND edges.status = 'active' \
                 AND source.status = 'active' AND target.status = 'active' \
                 AND source.mind = ?2 AND target.mind = ?2 ORDER BY edges.id",
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;

        let edges = stmt
            .query_map(params![fact_id, mind], |row| {
                Ok(Edge {
                    id: row.get("id")?,
                    source_id: row.get("source_fact_id")?,
                    target_id: row.get("target_fact_id")?,
                    relation: row.get("relation")?,
                    description: row.get("description")?,
                    confidence: row.get("confidence")?,
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        Ok(edges)
    }

    async fn store_episode(&self, req: StoreEpisode) -> Result<Episode> {
        if let Some(formation) = &req.formation {
            formation.validate()?;
        }
        let mut conn = self.conn.lock().unwrap();
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        self.ensure_mind(&transaction, &req.mind)
            .map_err(|error| MemoryError::Storage(error.into()))?;
        let id = gen_id();
        let ts = now_iso();
        let date = req.date.unwrap_or_else(|| ts[..10].to_string());

        transaction.execute(
            "INSERT INTO episodes (id, mind, title, narrative, date, created_at, affected_nodes, affected_changes, files_changed, tags, tool_calls_count, formation) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![id, req.mind, req.title, req.narrative, date, ts,
                serde_json::to_string(&req.affected_nodes).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&req.affected_changes).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&req.files_changed).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&req.tags).unwrap_or_else(|_| "[]".into()),
                req.tool_calls_count, req.formation.as_ref().map(serde_json::to_string).transpose().map_err(|error| MemoryError::Storage(error.into()))?],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let episode = Episode {
            id,
            mind: req.mind,
            date,
            title: req.title,
            narrative: req.narrative,
            created_at: ts,
            affected_nodes: req.affected_nodes,
            affected_changes: req.affected_changes,
            files_changed: req.files_changed,
            tags: req.tags,
            tool_calls_count: req.tool_calls_count,
            formation: req.formation,
            jj_change_id: None,
        };
        transaction
            .commit()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(episode)
    }

    async fn list_episodes(&self, mind: &str, k: usize) -> Result<Vec<Episode>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM episodes WHERE mind = ?1 ORDER BY date DESC, created_at DESC, id LIMIT ?2"
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let episodes = stmt
            .query_map(params![mind, k as i64], Self::row_to_episode)
            .map_err(|e| MemoryError::Storage(e.into()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(episodes)
    }

    async fn formation_cursor(&self, key: &FormationCaptureKey) -> Result<Option<FormationCursor>> {
        Self::capture_cursor(&self.conn.lock().unwrap(), key)
    }

    async fn pending_formations(
        &self,
        mind: &str,
        model: &str,
        limit: usize,
    ) -> Result<Vec<Episode>> {
        crate::formation::validate_recovery_limit(limit)?;
        let conn = self.conn.lock().unwrap();
        let mut statement=conn.prepare("SELECT * FROM episodes WHERE mind=?1 AND json_extract(formation, '$.extraction.state')='pending' AND json_extract(formation, '$.extraction.model')=?2 ORDER BY created_at, id LIMIT ?3").map_err(|error|MemoryError::Storage(error.into()))?;
        let episodes = statement
            .query_map(params![mind, model, limit as i64], Self::row_to_episode)
            .map_err(|error| MemoryError::Storage(error.into()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        for episode in &episodes {
            episode
                .formation
                .as_ref()
                .ok_or_else(|| MemoryError::InvalidMutation("missing pending formation".into()))?
                .validate()?;
        }
        Ok(episodes)
    }

    async fn search_episodes(&self, mind: &str, query: &str, k: usize) -> Result<Vec<Episode>> {
        let conn = self.conn.lock().unwrap();
        let fts_query = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() || k == 0 {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare(
                "SELECT e.* FROM episodes_fts efts \
             JOIN episodes e ON e.id = efts.id \
              WHERE episodes_fts MATCH ?1 AND e.mind = ?2 \
              ORDER BY rank, e.id LIMIT ?3",
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;

        let episodes = stmt
            .query_map(params![fts_query, mind, k as i64], Self::row_to_episode)
            .map_err(|e| MemoryError::Storage(e.into()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(episodes)
    }

    async fn export_jsonl(&self, mind: &str) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let mut lines = Vec::new();

        // Facts
        let mut stmt = conn
            .prepare("SELECT * FROM facts WHERE mind = ?1 ORDER BY id")
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let facts: Vec<Fact> = stmt
            .query_map(params![mind], Self::row_to_fact)
            .map_err(|e| MemoryError::Storage(e.into()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        for f in &facts {
            FactOperationalState::from(f).validate()?;
            let record = JsonlFact {
                applicability: f.applicability.clone(),
                lifecycle_inference: f.lifecycle_inference.clone(),
                operational: Some(Box::new(FactOperationalState::from(f))),
                id: f.id.clone(),
                mind: f.mind.clone(),
                content: f.content.clone(),
                section: f.section.clone(),
                status: f.status.clone(),
                created_at: f.created_at.clone(),
                source: f.source.clone(),
                content_hash: f.content_hash.clone(),
                supersedes: f.superseded_by.clone(),
                version: f.version,
                decay_profile: f.decay_profile.clone(),
                persona_id: f.persona_id.clone(),
                layer: f.layer.clone(),
                tags: f.tags.clone(),
            };
            let record = if record.applicability.is_some() {
                JsonlRecord::ApplicableFact(record)
            } else {
                JsonlRecord::Fact(record)
            };
            lines.push(serde_json::to_string(&record).unwrap());
        }

        // Edges
        let mut stmt = conn.prepare(
            "SELECT * FROM edges WHERE source_fact_id IN (SELECT id FROM facts WHERE mind = ?1) ORDER BY id"
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let edges: Vec<Edge> = stmt
            .query_map(params![mind], |row| {
                Ok(Edge {
                    id: row.get("id")?,
                    source_id: row.get("source_fact_id")?,
                    target_id: row.get("target_fact_id")?,
                    relation: row.get("relation")?,
                    description: row.get("description")?,
                    confidence: row.get("confidence")?,
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        for e in &edges {
            lines.push(serde_json::to_string(&JsonlRecord::Edge(e.clone())).unwrap());
        }

        // Episodes
        let mut stmt = conn
            .prepare("SELECT * FROM episodes WHERE mind = ?1 ORDER BY id")
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let episodes: Vec<Episode> = stmt
            .query_map(params![mind], Self::row_to_episode)
            .map_err(|e| MemoryError::Storage(e.into()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        for ep in &episodes {
            lines.push(serde_json::to_string(&JsonlRecord::Episode(ep.clone())).unwrap());
        }

        Ok(lines.join("\n"))
    }

    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| MemoryError::Storage(e.into()))?;

        let stats = self.import_jsonl_transaction(&tx, jsonl)?;
        tx.commit().map_err(|e| MemoryError::Storage(e.into()))?;
        Ok(stats)
    }

    async fn stats(&self, mind: &str) -> Result<MemoryStats> {
        let conn = self.conn.lock().unwrap();
        let total: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE mind = ?1",
                params![mind],
                |r| r.get(0),
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let active: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = 'active'",
                params![mind],
                |r| r.get(0),
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let archived: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = 'archived'",
                params![mind],
                |r| r.get(0),
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let superseded: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = 'superseded'",
                params![mind],
                |r| r.get(0),
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let with_vecs: usize = conn.query_row(
            "SELECT COUNT(*) FROM facts_vec fv JOIN facts f ON f.id = fv.fact_id WHERE f.mind = ?1",
            params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let episodes: usize = conn
            .query_row(
                "SELECT COUNT(*) FROM episodes WHERE mind = ?1",
                params![mind],
                |r| r.get(0),
            )
            .map_err(|e| MemoryError::Storage(e.into()))?;
        let edges: usize = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE source_fact_id IN (SELECT id FROM facts WHERE mind = ?1)",
            params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let version_hwm: u64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM facts WHERE mind = ?1",
                params![mind],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| MemoryError::Storage(e.into()))? as u64;

        let meta: Option<(String, u32)> = conn
            .query_row(
                "SELECT fv.model_name, fv.dims FROM facts_vec fv \
             JOIN facts f ON f.id = fv.fact_id \
             WHERE f.mind = ?1 LIMIT 1",
                params![mind],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| MemoryError::Storage(e.into()))?;

        Ok(MemoryStats {
            total_facts: total,
            active_facts: active,
            archived_facts: archived,
            superseded_facts: superseded,
            facts_with_vectors: with_vecs,
            embedding_model: meta.as_ref().map(|t: &(String, u32)| t.0.clone()),
            embedding_dims: meta.as_ref().map(|t: &(String, u32)| t.1),
            episodes,
            edges,
            version_hwm,
        })
    }

    async fn inventory_stats(&self) -> Result<MemoryInventoryStats> {
        let conn = self.conn.lock().unwrap();
        let count = |sql: &str| {
            conn.query_row(sql, [], |row| row.get::<_, usize>(0))
                .map_err(|error| MemoryError::Storage(error.into()))
        };
        let active_persona_mind = conn
            .query_row(
                "SELECT mind FROM facts WHERE status = 'active' AND layer = 'persona' \
                 GROUP BY mind ORDER BY COUNT(*) DESC, mind ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| MemoryError::Storage(error.into()))?;
        Ok(MemoryInventoryStats {
            total_facts: count("SELECT COUNT(*) FROM facts")?,
            active_facts: count("SELECT COUNT(*) FROM facts WHERE status = 'active'")?,
            project_facts: count(
                "SELECT COUNT(*) FROM facts WHERE status = 'active' AND layer = 'project'",
            )?,
            persona_facts: count(
                "SELECT COUNT(*) FROM facts WHERE status = 'active' AND layer = 'persona'",
            )?,
            working_facts: count(
                "SELECT COUNT(*) FROM facts WHERE status = 'active' AND layer = 'working'",
            )?,
            episodes: count("SELECT COUNT(*) FROM episodes")?,
            edges: count("SELECT COUNT(*) FROM edges")?,
            active_persona_mind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::run_backend_tests;

    #[tokio::test]
    async fn sqlite_backend_passes_all_tests() {
        let backend = SqliteBackend::in_memory().unwrap();
        run_backend_tests(&backend).await;
    }

    fn create_legacy_fixture(path: &Path, version: i64) {
        let backend = SqliteBackend::open(path).unwrap();
        drop(backend);
        let conn = Connection::open(path).unwrap();
        conn.execute(
            "INSERT INTO minds (name, description, created_at) VALUES ('default', 'Default legacy fixture', datetime('now')) ON CONFLICT(name) DO NOTHING",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO facts (id, mind, section, content, status, created_at, source, content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, decay_profile, version, layer) VALUES ('legacy-fact', 'default', 'architecture', 'legacy content', 'active', datetime('now'), 'test', 'legacy-hash', 1.0, datetime('now'), 1, 0.05, 'standard', 1, 'project')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO episodes (id, mind, title, narrative, date, created_at) VALUES ('legacy-episode', 'default', 'legacy', 'legacy narrative', date('now'), datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM schema_version", []).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
            params![version],
        )
        .unwrap();
        if version < 11 {
            conn.execute_batch("ALTER TABLE facts DROP COLUMN lifecycle_inference;")
                .unwrap();
        }
        if version < 13 {
            conn.execute_batch("ALTER TABLE facts DROP COLUMN applicability;")
                .unwrap();
        }
        if version < 10 {
            conn.execute_batch("ALTER TABLE facts_vec DROP COLUMN space; ALTER TABLE facts_vec DROP COLUMN source_hash;").unwrap();
        }
        if version < 9 {
            conn.execute_batch("ALTER TABLE episodes DROP COLUMN formation;")
                .unwrap();
        }
        if version < 8 {
            conn.execute_batch(
                "DROP TABLE memory_operation_receipts;
             ALTER TABLE episodes DROP COLUMN affected_nodes;
             ALTER TABLE episodes DROP COLUMN affected_changes;
             ALTER TABLE episodes DROP COLUMN files_changed;
             ALTER TABLE episodes DROP COLUMN tags;
             ALTER TABLE episodes DROP COLUMN tool_calls_count;",
            )
            .unwrap();
        }
        if version == 5 {
            conn.execute_batch(
                "DROP INDEX idx_facts_persona;
                 DROP INDEX idx_facts_layer;
                 ALTER TABLE facts DROP COLUMN persona_id;
                 ALTER TABLE facts DROP COLUMN layer;
                 ALTER TABLE facts DROP COLUMN tags;
                 ALTER TABLE episodes DROP COLUMN jj_change_id;",
            )
            .unwrap();
        }
    }

    #[test]
    fn migrates_representative_v5_through_v7_stores_with_rollback_backup() {
        for version in LEGACY_MEMORY_SCHEMA_VERSIONS {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("facts-v{version}.db"));
            create_legacy_fixture(&path, version);

            let plan = SqliteBackend::plan_migration(&path).unwrap();
            assert!(plan.is_applicable());
            assert_eq!(plan.fact_count, 1);
            assert_eq!(plan.episode_count, 1);
            let result = SqliteBackend::apply_migration(&plan).unwrap();
            assert_eq!(result.source_version, version);
            assert_eq!(result.target_version, MEMORY_SCHEMA_VERSION);
            assert!(result.backup.exists());
            assert_eq!(
                SqliteBackend::inspect_current(&result.backup)
                    .unwrap()
                    .source_version,
                version
            );
            assert_eq!(
                SqliteBackend::inspect_current(&path)
                    .unwrap()
                    .source_version,
                MEMORY_SCHEMA_VERSION
            );
            let migrated = Connection::open(&path).unwrap();
            let receipt_table: bool = migrated
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'memory_operation_receipts')",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(receipt_table);
            let episode_columns = {
                let mut statement = migrated.prepare("PRAGMA table_info(episodes)").unwrap();
                statement
                    .query_map([], |row| row.get::<_, String>(1))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            };
            assert!(episode_columns.contains(&"affected_nodes".to_string()));
            assert!(episode_columns.contains(&"tool_calls_count".to_string()));
            assert!(episode_columns.contains(&"formation".to_string()));
            let migrated_mind: String = migrated
                .query_row(
                    "SELECT mind FROM facts WHERE id = 'legacy-fact'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                migrated_mind,
                if version < 7 {
                    LEGACY_MIND
                } else {
                    PRIMENSUS_MIND
                }
            );
            drop(SqliteBackend::open(&path).unwrap());
        }
    }

    #[tokio::test]
    async fn operation_replay_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.db");
        let backend = SqliteBackend::open(&path).unwrap();
        let stored = backend
            .store_fact(StoreFact {
                mind: "reopen".into(),
                content: "Original".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: Some("test".into()),
            })
            .await
            .unwrap();
        let mutation = MemoryMutation::SupersedeFact {
            fact: FactPrecondition {
                id: stored.fact.id,
                expected_version: stored.fact.version,
            },
            replacement: StoreFact {
                mind: "reopen".into(),
                content: "Replacement".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: Some("test".into()),
            },
        };
        let committed = backend
            .apply_mutation("reopen-supersede", mutation.clone())
            .await
            .unwrap();
        drop(backend);

        let reopened = SqliteBackend::open(&path).unwrap();
        let replayed = reopened
            .apply_mutation("reopen-supersede", mutation)
            .await
            .unwrap();
        assert!(replayed.replayed);
        assert_eq!(committed.effect, replayed.effect);
        assert_eq!(
            reopened
                .list_facts("reopen", FactFilter::default())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn compound_mutations_roll_back_on_second_write_failure() {
        let backend = SqliteBackend::in_memory().unwrap();
        let first = backend
            .store_fact(StoreFact {
                mind: "rollback".into(),
                content: "First".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: None,
            })
            .await
            .unwrap();
        let second = backend
            .store_fact(StoreFact {
                mind: "rollback".into(),
                content: "Second".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: None,
            })
            .await
            .unwrap();
        {
            let conn = backend.conn.lock().unwrap();
            conn.execute_batch(&format!(
                "CREATE TRIGGER fail_second_archive BEFORE UPDATE OF status ON facts
                 WHEN OLD.id = '{}' AND NEW.status = 'archived'
                 BEGIN SELECT RAISE(ABORT, 'injected archive failure'); END;",
                second.fact.id
            ))
            .unwrap();
        }
        assert!(
            backend
                .archive_facts(&[&first.fact.id, &second.fact.id])
                .await
                .is_err()
        );
        assert!(backend.get_fact(&first.fact.id).await.unwrap().is_some());
        assert!(backend.get_fact(&second.fact.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn failed_supersede_and_embedding_leave_no_partial_effect() {
        let backend = SqliteBackend::in_memory().unwrap();
        let stored = backend
            .store_fact(StoreFact {
                mind: "rollback".into(),
                content: "Stable original".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: None,
            })
            .await
            .unwrap();
        {
            let conn = backend.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER fail_replacement BEFORE INSERT ON facts
                 WHEN NEW.content = 'Injected replacement failure'
                 BEGIN SELECT RAISE(ABORT, 'injected replacement failure'); END;
                 CREATE TRIGGER fail_embedding_metadata BEFORE INSERT ON embedding_metadata
                 WHEN NEW.model_name = 'fail-model'
                 BEGIN SELECT RAISE(ABORT, 'injected metadata failure'); END;",
            )
            .unwrap();
        }
        let mutation = MemoryMutation::SupersedeFact {
            fact: FactPrecondition {
                id: stored.fact.id.clone(),
                expected_version: stored.fact.version,
            },
            replacement: StoreFact {
                mind: "rollback".into(),
                content: "Injected replacement failure".into(),
                section: Section::Architecture,
                decay_profile: DecayProfileName::Standard,
                source: None,
            },
        };
        assert!(
            backend
                .apply_mutation("failed-supersede", mutation)
                .await
                .is_err()
        );
        let original = backend.get_fact(&stored.fact.id).await.unwrap().unwrap();
        assert_eq!(original.status, FactStatus::Active);
        let receipt_count: i64 = backend
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM memory_operation_receipts WHERE operation_id = 'failed-supersede'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(receipt_count, 0);

        assert!(
            backend
                .store_embedding(&stored.fact.id, "fail-model", &[1.0, 0.0])
                .await
                .is_err()
        );
        let vector_count: i64 = backend
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM facts_vec WHERE fact_id = ?1",
                params![stored.fact.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(vector_count, 0);
    }

    #[test]
    fn v7_reconciliation_moves_post_migration_default_records_to_primensus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.db");
        let backend = SqliteBackend::open(&path).unwrap();
        drop(backend);
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO minds (name, description, created_at) VALUES ('default', 'Stale post-v7 caller', datetime('now'))",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO facts (id, mind, section, content, status, created_at, source, content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, decay_profile, version, layer) VALUES ('stray-fact', 'default', 'architecture', 'post-v7 content', 'active', datetime('now'), 'test', 'stray-hash', 1.0, datetime('now'), 1, 0.05, 'standard', 1, 'project')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO episodes (id, mind, title, narrative, date, created_at) VALUES ('stray-episode', 'default', 'post-v7', 'post-v7 narrative', date('now'), datetime('now'))",
            [],
        )
        .unwrap();
        drop(conn);

        assert_eq!(
            SqliteBackend::reconcile_current_default_mind(&path).unwrap(),
            2
        );
        assert_eq!(
            SqliteBackend::reconcile_current_default_mind(&path).unwrap(),
            0
        );

        let conn = Connection::open(&path).unwrap();
        let default_facts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM facts WHERE mind = 'default'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let default_episodes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM episodes WHERE mind = 'default'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let primensus_records: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM facts WHERE mind = ?1) + (SELECT COUNT(*) FROM episodes WHERE mind = ?1)",
                params![PRIMENSUS_MIND],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!((default_facts, default_episodes), (0, 0));
        assert_eq!(primensus_records, 2);
    }

    #[test]
    fn migration_rejects_unsupported_version_and_existing_backup() {
        let dir = tempfile::tempdir().unwrap();
        let unsupported = dir.path().join("unsupported.db");
        create_legacy_fixture(&unsupported, 4);
        assert!(SqliteBackend::plan_migration(&unsupported).is_err());

        let path = dir.path().join("facts-v6.db");
        create_legacy_fixture(&path, 6);
        let plan = SqliteBackend::plan_migration(&path).unwrap();
        std::fs::write(&plan.backup, b"do not overwrite").unwrap();
        let error = SqliteBackend::apply_migration(&plan).unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite"));
    }

    #[test]
    fn migration_rejects_stale_plan_without_leaving_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts-v6.db");
        create_legacy_fixture(&path, 6);
        let plan = SqliteBackend::plan_migration(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO facts (id, mind, section, content, status, created_at, source, content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, decay_profile, version, layer) VALUES ('late-fact', 'default', 'architecture', 'late content', 'active', datetime('now'), 'test', 'late-hash', 1.0, datetime('now'), 1, 0.05, 'standard', 2, 'project')",
            [],
        )
        .unwrap();
        drop(conn);

        let error = SqliteBackend::apply_migration(&plan).unwrap_err();
        assert!(
            error.to_string().contains("counts changed"),
            "unexpected migration error: {error:#}"
        );
        assert!(!plan.backup.exists());
        assert_eq!(
            SqliteBackend::inspect_current(&path)
                .unwrap()
                .source_version,
            6
        );
    }

    #[test]
    fn migration_preserves_backup_after_post_commit_admission_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts-v7.db");
        create_legacy_fixture(&path, 7);
        let conn = Connection::open(&path).unwrap();
        conn.execute("DROP INDEX idx_facts_persona", []).unwrap();
        conn.execute("CREATE TABLE idx_facts_persona (id TEXT)", [])
            .unwrap();
        drop(conn);

        let plan = SqliteBackend::plan_migration(&path).unwrap();
        let error = SqliteBackend::apply_migration(&plan).unwrap_err();
        assert!(error.to_string().contains("already a table"));
        assert!(
            plan.backup.exists(),
            "committed migration must retain backup"
        );
        assert_eq!(
            SqliteBackend::status(&path).unwrap().schema_version,
            MEMORY_SCHEMA_VERSION
        );
        assert_eq!(
            SqliteBackend::status(&plan.backup).unwrap().schema_version,
            7
        );
    }

    #[test]
    fn rollback_restores_legacy_snapshot_and_preserves_v7_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts-v6.db");
        create_legacy_fixture(&path, 6);
        let plan = SqliteBackend::plan_migration(&path).unwrap();
        let migrated = SqliteBackend::apply_migration(&plan).unwrap();

        let rolled_back = SqliteBackend::rollback_migration(&path, &migrated.backup).unwrap();
        assert_eq!(rolled_back.restored_version, 6);
        assert!(rolled_back.preserved_current.exists());
        assert_eq!(SqliteBackend::status(&path).unwrap().schema_version, 6);
        assert_eq!(
            SqliteBackend::status(&rolled_back.preserved_current)
                .unwrap()
                .schema_version,
            MEMORY_SCHEMA_VERSION
        );
    }

    /// Generate schema-contract.json from the actual Rust schema.
    /// This is the canonical contract — TS validates against it.
    /// If the Rust schema changes, re-run this test to update the contract:
    ///   cargo test -p omegon-memory schema_contract -- --ignored
    /// Then commit the updated schema-contract.json.
    #[test]
    #[ignore] // Run manually: cargo test -p omegon-memory schema_contract -- --ignored
    fn schema_contract_generate() {
        let backend = SqliteBackend::in_memory().unwrap();
        let conn = backend.conn.lock().unwrap();

        let contract = generate_schema_contract(&conn);
        let contract_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema-contract.json");
        std::fs::write(&contract_path, &contract)
            .unwrap_or_else(|e| panic!("Failed to write {}: {}", contract_path.display(), e));
        println!("Updated {}", contract_path.display());
    }

    /// Validate that schema-contract.json is up to date with the actual Rust schema.
    /// Fails CI if someone changed sqlite.rs without regenerating the contract.
    #[test]
    fn schema_contract_is_current() {
        let backend = SqliteBackend::in_memory().unwrap();
        let conn = backend.conn.lock().unwrap();

        let contract_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema-contract.json");
        let on_disk = std::fs::read_to_string(&contract_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}. Run: cargo test -p omegon-memory schema_contract_generate -- --ignored", contract_path.display(), e));
        let generated = generate_schema_contract(&conn);

        assert_eq!(
            on_disk.replace("\r\n", "\n").trim(),
            generated.replace("\r\n", "\n").trim(),
            "schema-contract.json is stale. Regenerate with:\n  cargo test -p omegon-memory schema_contract_generate -- --ignored"
        );
    }

    fn generate_schema_contract(conn: &Connection) -> String {
        use std::collections::BTreeMap;

        // Get all real tables (not sqlite_ internal, not FTS virtual tables)
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '%_fts%' ORDER BY name"
        ).unwrap();
        let table_names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let mut tables = BTreeMap::new();
        for table in &table_names {
            let mut col_stmt = conn
                .prepare(&format!("PRAGMA table_info({})", table))
                .unwrap();
            let cols: Vec<String> = col_stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            tables.insert(table.clone(), cols);
        }

        let mut out = String::from("{\n");
        out.push_str("  \"description\": \"Canonical memory DB schema. Generated from Rust omegon-memory. Do not edit — regenerate with: cargo test -p omegon-memory schema_contract_generate -- --ignored\",\n");
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        out.push_str(&format!("  \"schema_version\": {},\n", version));
        out.push_str("  \"tables\": {\n");
        let table_count = tables.len();
        for (i, (table, cols)) in tables.iter().enumerate() {
            out.push_str(&format!("    \"{}\": [", table));
            for (j, col) in cols.iter().enumerate() {
                out.push_str(&format!("\"{}\"", col));
                if j < cols.len() - 1 {
                    out.push_str(", ");
                }
            }
            out.push(']');
            if i < table_count - 1 {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  }\n");
        out.push_str("}\n");
        out
    }
}
