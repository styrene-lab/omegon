//! SQLite-backed chunk cache at `.omegon/codescan.db`.
//!
//! Keyed by (path, content_hash). Incremental invalidation: only files
//! whose content_hash has changed since last index need re-chunking.

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::code::CodeChunk;
use crate::knowledge::KnowledgeChunk;

pub struct ScanCache {
    conn: Connection,
}

impl ScanCache {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("failed to create codescan dir")?;
        }
        let conn = Connection::open(db_path).context("failed to open codescan.db")?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS code_chunks (
                 id INTEGER PRIMARY KEY,
                 path TEXT NOT NULL,
                 start_line INTEGER NOT NULL,
                 end_line INTEGER NOT NULL,
                 item_name TEXT NOT NULL,
                 item_kind TEXT NOT NULL,
                 text TEXT NOT NULL,
                 content_hash TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_code_path ON code_chunks(path);
             CREATE TABLE IF NOT EXISTS knowledge_chunks (
                 id INTEGER PRIMARY KEY,
                 path TEXT NOT NULL,
                 heading TEXT NOT NULL,
                 start_line INTEGER NOT NULL,
                 end_line INTEGER NOT NULL,
                 tags TEXT NOT NULL,
                 text TEXT NOT NULL,
                 content_hash TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_knowledge_path ON knowledge_chunks(path);
             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );",
        )
        .context("failed to initialize codescan.db schema")?;
        Ok(Self { conn })
    }

    /// Return a (path → content_hash) map for ALL indexed files.
    ///
    /// Used by the indexer to batch-compare hashes without N individual queries.
    pub fn all_hashes(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT path, content_hash FROM code_chunks GROUP BY path HAVING MAX(rowid)")
        {
            let _ = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map(|rows| {
                    for row in rows.flatten() {
                        map.insert(row.0, row.1);
                    }
                });
        }
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT path, content_hash FROM knowledge_chunks GROUP BY path HAVING MAX(rowid)",
        ) {
            let _ = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map(|rows| {
                    for row in rows.flatten() {
                        map.entry(row.0).or_insert(row.1);
                    }
                });
        }
        map
    }

    /// Return paths whose content_hash has changed (or are not yet indexed).
    ///
    /// Uses a single pair of batch DB queries instead of N individual queries.
    pub fn stale_paths(&self, paths: &[(PathBuf, String)]) -> Vec<PathBuf> {
        let cached = self.all_hashes();
        paths
            .iter()
            .filter(|(p, new_hash)| {
                cached
                    .get(p.to_string_lossy().as_ref())
                    .map(|h| h != new_hash)
                    .unwrap_or(true) // not cached → stale
            })
            .map(|(p, _)| p.clone())
            .collect()
    }

    pub fn upsert_code_chunks(&self, path: &Path, hash: &str, chunks: &[CodeChunk]) -> Result<()> {
        let path_str = path.to_string_lossy();
        self.conn
            .execute("DELETE FROM code_chunks WHERE path = ?1", params![path_str])?;
        for chunk in chunks {
            self.conn.execute(
                "INSERT INTO code_chunks (path, start_line, end_line, item_name, item_kind, text, content_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![path_str, chunk.start_line, chunk.end_line, chunk.item_name, chunk.item_kind, chunk.text, hash],
            )?;
        }
        Ok(())
    }

    pub fn upsert_knowledge_chunks(
        &self,
        path: &Path,
        hash: &str,
        chunks: &[KnowledgeChunk],
    ) -> Result<()> {
        let path_str = path.to_string_lossy();
        self.conn.execute(
            "DELETE FROM knowledge_chunks WHERE path = ?1",
            params![path_str],
        )?;
        for chunk in chunks {
            self.conn.execute(
                "INSERT INTO knowledge_chunks (path, heading, start_line, end_line, tags, text, content_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![path_str, chunk.heading, chunk.start_line, chunk.end_line, chunk.tags.join(","), chunk.text, hash],
            )?;
        }
        Ok(())
    }

    pub fn prune_missing_paths(&self, live_paths: &HashSet<PathBuf>) -> Result<()> {
        let cached_paths = self
            .all_hashes()
            .into_keys()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        for path in cached_paths {
            if live_paths.contains(&path) {
                continue;
            }
            let path_str = path.to_string_lossy();
            self.conn.execute(
                "DELETE FROM code_chunks WHERE path = ?1",
                params![path_str.as_ref()],
            )?;
            self.conn.execute(
                "DELETE FROM knowledge_chunks WHERE path = ?1",
                params![path_str.as_ref()],
            )?;
        }
        Ok(())
    }

    pub fn all_code_chunks(&self) -> Result<Vec<CodeChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, start_line, end_line, item_name, item_kind, text FROM code_chunks",
        )?;
        let chunks = stmt
            .query_map([], |row| {
                Ok(CodeChunk {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    start_line: row.get(1)?,
                    end_line: row.get(2)?,
                    item_name: row.get(3)?,
                    item_kind: row.get(4)?,
                    text: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(chunks)
    }

    pub fn all_knowledge_chunks(&self) -> Result<Vec<KnowledgeChunk>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, heading, start_line, end_line, tags, text FROM knowledge_chunks",
        )?;
        let chunks = stmt
            .query_map([], |row| {
                let tags_str: String = row.get(4)?;
                Ok(KnowledgeChunk {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    heading: row.get(1)?,
                    start_line: row.get(2)?,
                    end_line: row.get(3)?,
                    tags: tags_str
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect(),
                    text: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(chunks)
    }

    pub fn get_meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM code_chunks; DELETE FROM knowledge_chunks;")?;
        Ok(())
    }

    /// Count of indexed code chunks.
    pub fn code_chunk_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM code_chunks", [], |r| r.get(0))
            .unwrap_or(0usize)
    }

    /// Count of indexed knowledge chunks.
    pub fn knowledge_chunk_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM knowledge_chunks", [], |r| r.get(0))
            .unwrap_or(0usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip_code() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(&dir.path().join("t.db")).unwrap();
        let path = Path::new("src/foo.rs");
        let chunk = CodeChunk {
            path: path.to_path_buf(),
            start_line: 1,
            end_line: 10,
            item_name: "foo".into(),
            item_kind: "fn".into(),
            text: "fn foo() {}".into(),
        };
        cache.upsert_code_chunks(path, "h1", &[chunk]).unwrap();
        let loaded = cache.all_code_chunks().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].item_name, "foo");
    }

    #[test]
    fn cache_round_trip_knowledge() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(&dir.path().join("t.db")).unwrap();
        let path = Path::new("docs/foo.md");
        let chunk = KnowledgeChunk {
            path: path.to_path_buf(),
            heading: "Overview".into(),
            start_line: 3,
            end_line: 15,
            tags: vec!["arch".into()],
            text: "text".into(),
        };
        cache.upsert_knowledge_chunks(path, "h1", &[chunk]).unwrap();
        let loaded = cache.all_knowledge_chunks().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].heading, "Overview");
    }

    #[test]
    fn stale_paths_uses_batch_query() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(&dir.path().join("t.db")).unwrap();
        let path_a = PathBuf::from("a.rs");
        let chunk = CodeChunk {
            path: path_a.clone(),
            start_line: 1,
            end_line: 1,
            item_name: "a".into(),
            item_kind: "fn".into(),
            text: "fn a(){}".into(),
        };
        cache
            .upsert_code_chunks(&path_a, "hash_a", &[chunk])
            .unwrap();

        let stale = cache.stale_paths(&[
            (path_a.clone(), "hash_a".into()),     // not stale
            (path_a.clone(), "hash_new".into()),   // stale (changed)
            (PathBuf::from("b.rs"), "any".into()), // stale (new)
        ]);
        assert_eq!(stale.len(), 2, "should detect changed + new: {:?}", stale);
    }

    #[test]
    fn all_hashes_returns_correct_map() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(&dir.path().join("t.db")).unwrap();
        let chunk = CodeChunk {
            path: PathBuf::from("x.rs"),
            start_line: 1,
            end_line: 1,
            item_name: "x".into(),
            item_kind: "fn".into(),
            text: "".into(),
        };
        cache
            .upsert_code_chunks(Path::new("x.rs"), "abc123", &[chunk])
            .unwrap();
        let hashes = cache.all_hashes();
        assert_eq!(hashes.get("x.rs"), Some(&"abc123".to_string()));
    }

    #[test]
    fn meta_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(&dir.path().join("t.db")).unwrap();
        assert_eq!(cache.get_meta("k"), None);
        cache.set_meta("k", "v").unwrap();
        assert_eq!(cache.get_meta("k"), Some("v".into()));
    }

    #[test]
    fn prune_missing_paths_removes_absent_code_and_knowledge_rows() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(&dir.path().join("t.db")).unwrap();

        cache
            .upsert_code_chunks(
                Path::new("src/keep.rs"),
                "h1",
                &[CodeChunk {
                    path: PathBuf::from("src/keep.rs"),
                    start_line: 1,
                    end_line: 1,
                    item_name: "keep".into(),
                    item_kind: "fn".into(),
                    text: "fn keep() {}".into(),
                }],
            )
            .unwrap();
        cache
            .upsert_code_chunks(
                Path::new(".omegon/cleave-workspace/stale.rs"),
                "h2",
                &[CodeChunk {
                    path: PathBuf::from(".omegon/cleave-workspace/stale.rs"),
                    start_line: 1,
                    end_line: 1,
                    item_name: "stale".into(),
                    item_kind: "fn".into(),
                    text: "fn stale() {}".into(),
                }],
            )
            .unwrap();
        cache
            .upsert_knowledge_chunks(
                Path::new("docs/keep.md"),
                "k1",
                &[KnowledgeChunk {
                    path: PathBuf::from("docs/keep.md"),
                    heading: "Keep".into(),
                    start_line: 1,
                    end_line: 1,
                    tags: vec![],
                    text: "keep".into(),
                }],
            )
            .unwrap();
        cache
            .upsert_knowledge_chunks(
                Path::new(".omegon/cleave-workspace/stale.md"),
                "k2",
                &[KnowledgeChunk {
                    path: PathBuf::from(".omegon/cleave-workspace/stale.md"),
                    heading: "Stale".into(),
                    start_line: 1,
                    end_line: 1,
                    tags: vec![],
                    text: "stale".into(),
                }],
            )
            .unwrap();

        let live_paths =
            HashSet::from([PathBuf::from("src/keep.rs"), PathBuf::from("docs/keep.md")]);
        cache.prune_missing_paths(&live_paths).unwrap();

        let code = cache.all_code_chunks().unwrap();
        let knowledge = cache.all_knowledge_chunks().unwrap();
        assert_eq!(code.len(), 1);
        assert_eq!(code[0].path, PathBuf::from("src/keep.rs"));
        assert_eq!(knowledge.len(), 1);
        assert_eq!(knowledge[0].path, PathBuf::from("docs/keep.md"));
    }
}
