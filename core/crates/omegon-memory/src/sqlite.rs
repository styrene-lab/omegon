//! SqliteBackend — production MemoryBackend backed by rusqlite.
//!
//! Schema matches the TypeScript factstore.ts (v5) exactly.
//! WAL mode for concurrent reads. FTS5 for full-text search.
//! Bundled sqlite via rusqlite's `bundled` feature.

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

use crate::backend::*;
use crate::hash;
use crate::types::*;
use crate::util::{gen_id, now_iso};
use crate::vectors;

pub struct SqliteBackend {
    conn: Mutex<Connection>,
}

impl SqliteBackend {
    /// Open or create a sqlite DB at the given path.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        let backend = Self { conn: Mutex::new(conn) };
        backend.init_schema()?;
        Ok(backend)
    }

    /// Create an in-memory sqlite DB (for testing).
    pub fn in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let backend = Self { conn: Mutex::new(conn) };
        backend.init_schema()?;
        Ok(backend)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        // busy_timeout: wait up to 5s for write lock instead of failing immediately.
        // Critical for multi-process access (cleave children share the same DB file).
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;

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

            INSERT OR IGNORE INTO minds (name, created_at) VALUES ('default', datetime('now'));

            CREATE TABLE IF NOT EXISTS facts (
                id                  TEXT PRIMARY KEY,
                mind                TEXT NOT NULL DEFAULT 'default',
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
                FOREIGN KEY (mind) REFERENCES minds(name) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_facts_active ON facts(mind, status) WHERE status = 'active';
            CREATE INDEX IF NOT EXISTS idx_facts_hash ON facts(mind, content_hash);
            CREATE INDEX IF NOT EXISTS idx_facts_section ON facts(mind, section) WHERE status = 'active';
            CREATE INDEX IF NOT EXISTS idx_facts_session ON facts(created_session);
            CREATE INDEX IF NOT EXISTS idx_facts_version ON facts(version DESC);

            CREATE TABLE IF NOT EXISTS facts_vec (
                fact_id    TEXT PRIMARY KEY,
                embedding  BLOB NOT NULL,
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
                mind        TEXT NOT NULL DEFAULT 'default',
                title       TEXT NOT NULL,
                narrative   TEXT NOT NULL,
                date        TEXT NOT NULL,
                session_id  TEXT,
                created_at  TEXT NOT NULL,
                jj_change_id TEXT,
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

            -- Schema version tracking (TS compat — factstore.ts checks this)
            CREATE TABLE IF NOT EXISTS schema_version (
                version    INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
        ")?;

        // Mark schema version 5 (matches TS factstore.ts v5) if not already set
        let current: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0)
        ).unwrap_or(0);

        // Migration: v4 → v5 (align with TS factstore.ts v5)
        // Adds columns that were in TS but missing from Rust's initial schema:
        //   facts: created_session, superseded_at, archived_at, jj_change_id
        //   episodes: session_id, jj_change_id
        // Also creates episode_facts and episodes_vec tables if missing.
        if current < 5 {
            // Add missing columns (idempotent — ignore "duplicate column" errors)
            for stmt in &[
                // facts
                "ALTER TABLE facts ADD COLUMN created_session TEXT",
                "ALTER TABLE facts ADD COLUMN superseded_at TEXT",
                "ALTER TABLE facts ADD COLUMN archived_at TEXT",
                "ALTER TABLE facts ADD COLUMN jj_change_id TEXT",
                // edges
                "ALTER TABLE edges ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0",
                "ALTER TABLE edges ADD COLUMN last_reinforced TEXT",
                "ALTER TABLE edges ADD COLUMN reinforcement_count INTEGER NOT NULL DEFAULT 1",
                "ALTER TABLE edges ADD COLUMN decay_rate REAL NOT NULL DEFAULT 0.05",
                "ALTER TABLE edges ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
                "ALTER TABLE edges ADD COLUMN created_session TEXT",
                "ALTER TABLE edges ADD COLUMN source_mind TEXT",
                "ALTER TABLE edges ADD COLUMN target_mind TEXT",
                // minds
                "ALTER TABLE minds ADD COLUMN origin_path TEXT",
                "ALTER TABLE minds ADD COLUMN origin_url TEXT",
                "ALTER TABLE minds ADD COLUMN readonly INTEGER NOT NULL DEFAULT 0",
                "ALTER TABLE minds ADD COLUMN parent TEXT",
                "ALTER TABLE minds ADD COLUMN last_sync TEXT",
                // episodes
                "ALTER TABLE episodes ADD COLUMN session_id TEXT",
                "ALTER TABLE episodes ADD COLUMN jj_change_id TEXT",
            ] {
                let _ = conn.execute(stmt, []); // ignore if column already exists
            }

            // Create tables that may not exist in older Rust DBs
            conn.execute_batch("
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
                CREATE INDEX IF NOT EXISTS idx_facts_session ON facts(created_session);
                CREATE INDEX IF NOT EXISTS idx_facts_version ON facts(version DESC);
            ")?;

            conn.execute(
                "INSERT OR REPLACE INTO schema_version (version, applied_at) VALUES (5, datetime('now'))",
                [],
            )?;
        }

        Ok(())
    }

    fn ensure_mind(&self, conn: &Connection, mind: &str) {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO minds (name, created_at) VALUES (?1, ?2)",
            params![mind, now_iso()],
        );
    }

    fn next_version(&self, conn: &Connection) -> u64 {
        Self::next_version_static(conn)
    }

    fn next_version_static(conn: &Connection) -> u64 {
        let max: u64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM facts", [], |r| r.get(0)
        ).unwrap_or(0);
        max + 1
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
        let status = serde_json::from_value::<FactStatus>(serde_json::Value::String(status_str.clone()))
            .unwrap_or_else(|_| {
                tracing::warn!(status = %status_str, "unknown status in DB — defaulting to Active");
                FactStatus::Active
            });

        Ok(Fact {
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
            source: row.get("source")?,
            content_hash: Some(row.get::<_, String>("content_hash")?),
            last_accessed: row.get("last_accessed")?,
            created_session: row.get("created_session")?,
            superseded_at: row.get("superseded_at")?,
            archived_at: row.get("archived_at")?,
            jj_change_id: row.get("jj_change_id")?,
        })
    }
}

#[async_trait]
impl MemoryBackend for SqliteBackend {
    async fn store_fact(&self, req: StoreFact) -> Result<StoreResult> {
        let conn = self.conn.lock().unwrap();
        self.ensure_mind(&conn, &req.mind);
        let ch = hash::content_hash(&req.content);
        let section_str = serde_json::to_string(&req.section).unwrap_or_default();
        let section_str = section_str.trim_matches('"');
        let profile_str = serde_json::to_string(&req.decay_profile).unwrap_or_default();
        let profile_str = profile_str.trim_matches('"');

        // Check dedup
        let existing: Option<String> = conn.query_row(
            "SELECT id FROM facts WHERE mind = ?1 AND content_hash = ?2 AND status = 'active'",
            params![req.mind, ch],
            |r| r.get(0),
        ).optional().map_err(|e| MemoryError::Storage(e.into()))?;

        if let Some(id) = existing {
            let version = self.next_version(&conn);
            let ts = now_iso();
            conn.execute(
                "UPDATE facts SET reinforcement_count = reinforcement_count + 1, \
                 last_reinforced = ?1, version = ?2 WHERE id = ?3",
                params![ts, version as i64, id],
            ).map_err(|e| MemoryError::Storage(e.into()))?;

            let fact = conn.query_row("SELECT * FROM facts WHERE id = ?1", params![id], Self::row_to_fact)
                .map_err(|e| MemoryError::Storage(e.into()))?;
            return Ok(StoreResult { fact, action: StoreAction::Reinforced });
        }

        let id = gen_id();
        let ts = now_iso();
        let version = self.next_version(&conn);
        conn.execute(
            "INSERT INTO facts (id, mind, section, content, status, created_at, source, \
             content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, \
             decay_profile, version) VALUES (?1,?2,?3,?4,'active',?5,?6,?7,1.0,?5,1,0.05,?8,?9)",
            params![id, req.mind, section_str, req.content, ts,
                    req.source.as_deref().unwrap_or("manual"), ch, profile_str, version as i64],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let fact = conn.query_row("SELECT * FROM facts WHERE id = ?1", params![id], Self::row_to_fact)
            .map_err(|e| MemoryError::Storage(e.into()))?;
        Ok(StoreResult { fact, action: StoreAction::Stored })
    }

    async fn get_fact(&self, id: &str) -> Result<Option<Fact>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT * FROM facts WHERE id = ?1 AND status = 'active'",
            params![id], Self::row_to_fact,
        ).optional().map_err(|e| MemoryError::Storage(e.into()))
    }

    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>> {
        let conn = self.conn.lock().unwrap();
        let status_str = filter.status.as_ref()
            .map(|s| serde_json::to_string(s).unwrap_or_default().trim_matches('"').to_string())
            .unwrap_or_else(|| "active".into());

        let (sql, section_param);
        if let Some(ref sec) = filter.section {
            section_param = serde_json::to_string(sec).unwrap_or_default().trim_matches('"').to_string();
            sql = "SELECT * FROM facts WHERE mind = ?1 AND status = ?2 AND section = ?3 ORDER BY created_at DESC";
            let mut stmt = conn.prepare(sql).map_err(|e| MemoryError::Storage(e.into()))?;
            let facts = stmt.query_map(params![mind, status_str, section_param], Self::row_to_fact)
                .map_err(|e| MemoryError::Storage(e.into()))?
                .filter_map(|r| {
                    r.map_err(|e| tracing::debug!("row deser error: {e}")).ok()
                })
                .collect();
            Ok(facts)
        } else {
            sql = "SELECT * FROM facts WHERE mind = ?1 AND status = ?2 ORDER BY created_at DESC";
            let mut stmt = conn.prepare(sql).map_err(|e| MemoryError::Storage(e.into()))?;
            let facts = stmt.query_map(params![mind, status_str], Self::row_to_fact)
                .map_err(|e| MemoryError::Storage(e.into()))?
                .filter_map(|r| {
                    r.map_err(|e| tracing::debug!("row deser error: {e}")).ok()
                })
                .collect();
            Ok(facts)
        }
    }

    async fn reinforce_fact(&self, id: &str) -> Result<Fact> {
        let conn = self.conn.lock().unwrap();
        let version = self.next_version(&conn);
        let ts = now_iso();
        let updated = conn.execute(
            "UPDATE facts SET reinforcement_count = reinforcement_count + 1, \
             last_reinforced = ?1, version = ?2 WHERE id = ?3 AND status = 'active'",
            params![ts, version as i64, id],
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        if updated == 0 {
            return Err(MemoryError::FactNotFound(id.into()));
        }
        conn.query_row("SELECT * FROM facts WHERE id = ?1", params![id], Self::row_to_fact)
            .map_err(|e| MemoryError::Storage(e.into()))
    }

    async fn archive_facts(&self, ids: &[&str]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut count = 0usize;
        for id in ids {
            let version = self.next_version(&conn);
            let n = conn.execute(
                "UPDATE facts SET status = 'archived', version = ?1 WHERE id = ?2 AND status = 'active'",
                params![version as i64, id],
            ).map_err(|e| MemoryError::Storage(e.into()))?;
            count += n;
        }
        Ok(count)
    }

    async fn supersede_fact(&self, id: &str, replacement: StoreFact) -> Result<Fact> {
        let mut conn = self.conn.lock().unwrap();
        self.ensure_mind(&conn, &replacement.mind);

        // Check original exists
        let exists: bool = conn.query_row(
            "SELECT 1 FROM facts WHERE id = ?1 AND status = 'active'", params![id], |_| Ok(true),
        ).optional().map_err(|e| MemoryError::Storage(e.into()))?.unwrap_or(false);
        if !exists {
            return Err(MemoryError::FactNotFound(id.into()));
        }

        let new_id = gen_id();
        let version = Self::next_version_static(&conn);

        // Transaction: archive original + insert replacement atomically
        let tx = conn.transaction().map_err(|e| MemoryError::Storage(e.into()))?;

        // Archive original — matches TS behavior
        tx.execute(
            "UPDATE facts SET status = 'superseded', version = ?1 WHERE id = ?2",
            params![version as i64, id],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        // Insert replacement
        let section_str = serde_json::to_string(&replacement.section).unwrap_or_default();
        let section_str = section_str.trim_matches('"');
        let profile_str = serde_json::to_string(&replacement.decay_profile).unwrap_or_default();
        let profile_str = profile_str.trim_matches('"');
        let ch = hash::content_hash(&replacement.content);
        let ts = now_iso();
        let version2 = version + 1;

        tx.execute(
            "INSERT INTO facts (id, mind, section, content, status, created_at, source, \
             content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, \
             decay_profile, version, supersedes) VALUES (?1,?2,?3,?4,'active',?5,?6,?7,1.0,?5,1,0.05,?8,?9,?10)",
            params![new_id, replacement.mind, section_str, replacement.content, ts,
                    replacement.source.as_deref().unwrap_or("manual"), ch, profile_str, version2 as i64, id],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let fact = tx.query_row("SELECT * FROM facts WHERE id = ?1", params![new_id], Self::row_to_fact)
            .map_err(|e| MemoryError::Storage(e.into()))?;

        tx.commit().map_err(|e| MemoryError::Storage(e.into()))?;
        Ok(fact)
    }

    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>> {
        let conn = self.conn.lock().unwrap();
        // Use FTS5 OR mode for broader matching
        let fts_query = query.split_whitespace()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ");

        let mut stmt = conn.prepare(
            "SELECT f.*, rank FROM facts_fts fts \
             JOIN facts f ON f.id = fts.id \
             WHERE facts_fts MATCH ?1 AND fts.mind = ?2 AND f.status = 'active' \
             ORDER BY rank LIMIT ?3"
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let results: Vec<ScoredFact> = stmt.query_map(
            params![fts_query, mind, k as i64],
            |row| {
                let fact = Self::row_to_fact(row)?;
                let rank: f64 = row.get("rank")?;
                Ok(ScoredFact {
                    similarity: -rank, // FTS5 rank is negative (lower = better)
                    score: -rank,
                    fact,
                })
            },
        ).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();

        Ok(results)
    }

    async fn vector_search(
        &self, mind: &str, embedding: &[f32], k: usize, min_similarity: f32,
    ) -> Result<Vec<ScoredFact>> {
        let conn = self.conn.lock().unwrap();

        // Check if any embeddings exist for this mind
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM facts_vec fv JOIN facts f ON f.id = fv.fact_id WHERE f.mind = ?1",
            params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        if count == 0 {
            return Err(MemoryError::NoEmbeddings);
        }

        // Check dimension match
        let stored_dims: u32 = conn.query_row(
            "SELECT dims FROM facts_vec fv JOIN facts f ON f.id = fv.fact_id WHERE f.mind = ?1 LIMIT 1",
            params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let query_dims = embedding.len() as u32;
        if stored_dims != query_dims {
            let model: String = conn.query_row(
                "SELECT model_name FROM facts_vec fv JOIN facts f ON f.id = fv.fact_id WHERE f.mind = ?1 LIMIT 1",
                params![mind], |r| r.get(0),
            ).map_err(|e| MemoryError::Storage(e.into()))?;
            return Err(MemoryError::EmbeddingDimensionMismatch {
                expected: stored_dims, got: query_dims, stored_model: model,
            });
        }

        // Linear scan — load all vectors and compute cosine similarity
        let mut stmt = conn.prepare(
            "SELECT fv.fact_id, fv.embedding, f.* FROM facts_vec fv \
             JOIN facts f ON f.id = fv.fact_id \
             WHERE f.mind = ?1 AND f.status = 'active'"
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let mut results: Vec<ScoredFact> = stmt.query_map(params![mind], |row| {
            let blob: Vec<u8> = row.get("embedding")?;
            let fact = Self::row_to_fact(row)?;
            Ok((blob, fact))
        }).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .filter_map(|(blob, fact)| {
                let vec = vectors::blob_to_vector(&blob);
                let sim = vectors::cosine_similarity(&vec, embedding);
                if sim < min_similarity { return None; }
                Some(ScoredFact { similarity: sim as f64, score: sim as f64, fact })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);
        Ok(results)
    }

    async fn store_embedding(&self, fact_id: &str, model_name: &str, embedding: &[f32]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let blob = vectors::vector_to_blob(embedding);
        let ts = now_iso();
        let dims = embedding.len() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO facts_vec (fact_id, embedding, model_name, dims, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![fact_id, blob, model_name, dims, ts],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        conn.execute(
            "INSERT OR IGNORE INTO embedding_metadata (model_name, dims, inserted_at) VALUES (?1, ?2, ?3)",
            params![model_name, dims, ts],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        Ok(())
    }

    async fn embedding_metadata(&self, mind: &str) -> Result<Option<EmbeddingMetadata>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT em.model_name, em.dims, em.inserted_at FROM embedding_metadata em \
             JOIN facts_vec fv ON fv.model_name = em.model_name \
             JOIN facts f ON f.id = fv.fact_id \
             WHERE f.mind = ?1 LIMIT 1",
            params![mind],
            |row| Ok(EmbeddingMetadata {
                model_name: row.get(0)?,
                dims: row.get(1)?,
                inserted_at: row.get(2)?,
            }),
        ).optional().map_err(|e| MemoryError::Storage(e.into()))
    }

    async fn create_edge(&self, req: CreateEdge) -> Result<Edge> {
        let conn = self.conn.lock().unwrap();
        let id = gen_id();
        let ts = now_iso();
        conn.execute(
            "INSERT INTO edges (id, source_fact_id, target_fact_id, relation, description, weight, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 1.0, ?6)",
            params![id, req.source_id, req.target_id, req.relation, req.description, ts],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        Ok(Edge { id, source_id: req.source_id, target_id: req.target_id,
            relation: req.relation, description: req.description, weight: 1.0, created_at: ts })
    }

    async fn get_edges(&self, _mind: &str, fact_id: &str) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM edges WHERE source_fact_id = ?1 OR target_fact_id = ?1"
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let edges = stmt.query_map(params![fact_id], |row| {
            Ok(Edge {
                id: row.get("id")?,
                source_id: row.get("source_fact_id")?,
                target_id: row.get("target_fact_id")?,
                relation: row.get("relation")?,
                description: row.get("description")?,
                weight: row.get("weight")?,
                created_at: row.get("created_at")?,
            })
        }).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        Ok(edges)
    }

    async fn store_episode(&self, req: StoreEpisode) -> Result<Episode> {
        let conn = self.conn.lock().unwrap();
        self.ensure_mind(&conn, &req.mind);
        let id = gen_id();
        let ts = now_iso();
        let date = req.date.unwrap_or_else(|| ts[..10].to_string());

        conn.execute(
            "INSERT INTO episodes (id, mind, title, narrative, date, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, req.mind, req.title, req.narrative, date, ts],
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        Ok(Episode {
            id, mind: req.mind, date, title: req.title, narrative: req.narrative,
            created_at: ts,
            affected_nodes: req.affected_nodes, affected_changes: req.affected_changes,
            files_changed: req.files_changed, tags: req.tags,
            tool_calls_count: req.tool_calls_count,
        })
    }

    async fn list_episodes(&self, mind: &str, k: usize) -> Result<Vec<Episode>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM episodes WHERE mind = ?1 ORDER BY date DESC, created_at DESC LIMIT ?2"
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let episodes = stmt.query_map(params![mind, k as i64], |row| {
            Ok(Episode {
                id: row.get("id")?,
                mind: row.get("mind")?,
                date: row.get("date")?,
                title: row.get("title")?,
                narrative: row.get("narrative")?,
                created_at: row.get("created_at")?,
                affected_nodes: vec![], affected_changes: vec![],
                files_changed: vec![], tags: vec![], tool_calls_count: None,
            })
        }).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        Ok(episodes)
    }

    async fn search_episodes(&self, mind: &str, query: &str, k: usize) -> Result<Vec<Episode>> {
        let conn = self.conn.lock().unwrap();
        let fts_query = query.split_whitespace()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ");

        let mut stmt = conn.prepare(
            "SELECT e.* FROM episodes_fts efts \
             JOIN episodes e ON e.id = efts.id \
             WHERE episodes_fts MATCH ?1 AND efts.mind = ?2 \
             ORDER BY rank LIMIT ?3"
        ).map_err(|e| MemoryError::Storage(e.into()))?;

        let episodes = stmt.query_map(params![fts_query, mind, k as i64], |row| {
            Ok(Episode {
                id: row.get("id")?,
                mind: row.get("mind")?,
                date: row.get("date")?,
                title: row.get("title")?,
                narrative: row.get("narrative")?,
                created_at: row.get("created_at")?,
                affected_nodes: vec![], affected_changes: vec![],
                files_changed: vec![], tags: vec![], tool_calls_count: None,
            })
        }).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        Ok(episodes)
    }

    async fn export_jsonl(&self, mind: &str) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let mut lines = Vec::new();

        // Facts
        let mut stmt = conn.prepare(
            "SELECT * FROM facts WHERE mind = ?1 AND status = 'active' ORDER BY section, created_at, id"
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let facts: Vec<Fact> = stmt.query_map(params![mind], Self::row_to_fact)
            .map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        for f in &facts {
            let record = JsonlRecord::Fact(JsonlFact {
                id: f.id.clone(), mind: f.mind.clone(), content: f.content.clone(),
                section: f.section.clone(), status: f.status.clone(), created_at: f.created_at.clone(),
                source: f.source.clone(), content_hash: f.content_hash.clone(),
                supersedes: None, version: f.version, decay_profile: f.decay_profile.clone(),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }

        // Edges
        let mut stmt = conn.prepare(
            "SELECT * FROM edges WHERE source_fact_id IN (SELECT id FROM facts WHERE mind = ?1) ORDER BY id"
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let edges: Vec<Edge> = stmt.query_map(params![mind], |row| {
            Ok(Edge {
                id: row.get("id")?, source_id: row.get("source_fact_id")?,
                target_id: row.get("target_fact_id")?, relation: row.get("relation")?,
                description: row.get("description")?, weight: row.get("weight")?,
                created_at: row.get("created_at")?,
            })
        }).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        for e in &edges {
            lines.push(serde_json::to_string(&JsonlRecord::Edge(e.clone())).unwrap());
        }

        // Episodes
        let mut stmt = conn.prepare(
            "SELECT * FROM episodes WHERE mind = ?1 ORDER BY id"
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let episodes: Vec<Episode> = stmt.query_map(params![mind], |row| {
            Ok(Episode {
                id: row.get("id")?, mind: row.get("mind")?, date: row.get("date")?,
                title: row.get("title")?, narrative: row.get("narrative")?,
                created_at: row.get("created_at")?,
                affected_nodes: vec![], affected_changes: vec![],
                files_changed: vec![], tags: vec![], tool_calls_count: None,
            })
        }).map_err(|e| MemoryError::Storage(e.into()))?
            .filter_map(|r| r.map_err(|e| tracing::debug!("row deser: {e}")).ok())
            .collect();
        for ep in &episodes {
            lines.push(serde_json::to_string(&JsonlRecord::Episode(ep.clone())).unwrap());
        }

        Ok(lines.join("\n"))
    }

    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(|e| MemoryError::Storage(e.into()))?;

        for line in jsonl.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            match serde_json::from_str::<JsonlRecord>(trimmed) {
                Ok(JsonlRecord::Fact(jf)) => {
                    self.ensure_mind(&tx, &jf.mind);
                    let existing_version: Option<i64> = tx.query_row(
                        "SELECT version FROM facts WHERE id = ?1", params![jf.id], |r| r.get(0),
                    ).optional().map_err(|e| MemoryError::Storage(e.into()))?.flatten();

                    if let Some(ev) = existing_version {
                        if (jf.version as i64) > ev {
                            let section_str = serde_json::to_string(&jf.section).unwrap_or_default();
                            let section_str = section_str.trim_matches('"');
                            tx.execute(
                                "UPDATE facts SET content = ?1, section = ?2, version = ?3 WHERE id = ?4",
                                params![jf.content, section_str, jf.version as i64, jf.id],
                            ).map_err(|e| MemoryError::Storage(e.into()))?;
                            stats.reinforced += 1;
                        } else {
                            stats.skipped += 1;
                        }
                    } else {
                        let section_str = serde_json::to_string(&jf.section).unwrap_or_default();
                        let section_str = section_str.trim_matches('"');
                        let profile_str = serde_json::to_string(&jf.decay_profile).unwrap_or_default();
                        let profile_str = profile_str.trim_matches('"');
                        let ch = jf.content_hash.unwrap_or_else(|| hash::content_hash(&jf.content));
                        tx.execute(
                            "INSERT INTO facts (id, mind, section, content, status, created_at, source, \
                             content_hash, confidence, last_reinforced, reinforcement_count, decay_rate, \
                             decay_profile, version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1.0,?6,1,0.05,?9,?10)",
                            params![jf.id, jf.mind, section_str, jf.content,
                                    serde_json::to_string(&jf.status).unwrap_or_default().trim_matches('"'),
                                    jf.created_at, jf.source.as_deref().unwrap_or("manual"),
                                    ch, profile_str, jf.version as i64],
                        ).map_err(|e| MemoryError::Storage(e.into()))?;
                        stats.imported += 1;
                    }
                }
                Ok(JsonlRecord::Episode(ep)) => {
                    self.ensure_mind(&tx, &ep.mind);
                    tx.execute(
                        "INSERT OR IGNORE INTO episodes (id, mind, title, narrative, date, created_at) \
                         VALUES (?1,?2,?3,?4,?5,?6)",
                        params![ep.id, ep.mind, ep.title, ep.narrative, ep.date, ep.created_at],
                    ).map_err(|e| MemoryError::Storage(e.into()))?;
                    stats.imported += 1;
                }
                Ok(JsonlRecord::Edge(edge)) => {
                    tx.execute(
                        "INSERT OR IGNORE INTO edges (id, source_fact_id, target_fact_id, relation, description, weight, created_at) \
                         VALUES (?1,?2,?3,?4,?5,?6,?7)",
                        params![edge.id, edge.source_id, edge.target_id, edge.relation,
                                edge.description, edge.weight, edge.created_at],
                    ).map_err(|e| MemoryError::Storage(e.into()))?;
                    stats.imported += 1;
                }
                Ok(JsonlRecord::Mind(_)) => { stats.skipped += 1; }
                Err(_) => { stats.errors += 1; }
            }
        }
        tx.commit().map_err(|e| MemoryError::Storage(e.into()))?;
        Ok(stats)
    }

    async fn stats(&self, mind: &str) -> Result<MemoryStats> {
        let conn = self.conn.lock().unwrap();
        let total: usize = conn.query_row(
            "SELECT COUNT(*) FROM facts WHERE mind = ?1", params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let active: usize = conn.query_row(
            "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = 'active'", params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let archived: usize = conn.query_row(
            "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = 'archived'", params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let superseded: usize = conn.query_row(
            "SELECT COUNT(*) FROM facts WHERE mind = ?1 AND status = 'superseded'", params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let with_vecs: usize = conn.query_row(
            "SELECT COUNT(*) FROM facts_vec fv JOIN facts f ON f.id = fv.fact_id WHERE f.mind = ?1",
            params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let episodes: usize = conn.query_row(
            "SELECT COUNT(*) FROM episodes WHERE mind = ?1", params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let edges: usize = conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE source_fact_id IN (SELECT id FROM facts WHERE mind = ?1)",
            params![mind], |r| r.get(0),
        ).map_err(|e| MemoryError::Storage(e.into()))?;
        let version_hwm: u64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM facts WHERE mind = ?1", params![mind], |r| r.get::<_, i64>(0),
        ).map_err(|e| MemoryError::Storage(e.into()))? as u64;

        let meta: Option<(String, u32)> = conn.query_row(
            "SELECT em.model_name, em.dims FROM embedding_metadata em \
             JOIN facts_vec fv ON fv.model_name = em.model_name \
             JOIN facts f ON f.id = fv.fact_id \
             WHERE f.mind = ?1 LIMIT 1",
            params![mind],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(|e| MemoryError::Storage(e.into()))?;

        Ok(MemoryStats {
            total_facts: total, active_facts: active, archived_facts: archived,
            superseded_facts: superseded, facts_with_vectors: with_vecs,
            embedding_model: meta.as_ref().map(|t: &(String, u32)| t.0.clone()),
            embedding_dims: meta.as_ref().map(|t: &(String, u32)| t.1),
            episodes, edges, version_hwm,
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

    /// Validate that the actual SQLite schema matches schema-contract.json.
    /// If this test fails, either the Rust schema or the contract needs updating.
    /// The contract is consumed by the TS omegon-pi repo to prevent drift.
    #[test]
    fn schema_matches_contract() {
        let backend = SqliteBackend::in_memory().unwrap();
        let conn = backend.conn.lock().unwrap();

        // Load the contract
        let contract_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema-contract.json");
        let contract_text = std::fs::read_to_string(&contract_path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", contract_path.display(), e));
        let contract: serde_json::Value = serde_json::from_str(&contract_text)
            .expect("schema-contract.json is not valid JSON");

        let tables = contract["tables"].as_object().expect("tables must be an object");
        for (table_name, table_spec) in tables {
            // Get actual columns from SQLite
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table_name))
                .unwrap_or_else(|_| panic!("Table '{}' does not exist in Rust schema", table_name));
            let actual_cols: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();

            let required_cols = table_spec["columns"].as_object()
                .unwrap_or_else(|| panic!("columns must be an object for table {}", table_name));

            for col_name in required_cols.keys() {
                assert!(
                    actual_cols.contains(col_name),
                    "Contract requires column '{}.{}' but it's missing from the Rust schema. \
                     Either add it to sqlite.rs or update schema-contract.json.",
                    table_name, col_name
                );
            }
        }
    }
}
