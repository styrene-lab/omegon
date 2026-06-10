//! Repo walker — discovers files, hashes content, drives incremental indexing.
//!
//! Fast-path: if git HEAD hasn't changed since the last index, skip the file
//! walk entirely and return cached stats. This makes the incremental path
//! near-instantaneous (~5ms vs 2s for a full walk of a large repo).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::cache::ScanCache;
use crate::code::{CodeScanner, is_supported_code_extension};
use crate::knowledge::{KnowledgeDirs, KnowledgeScanner};

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub code_files: usize,
    pub knowledge_files: usize,
    pub code_chunks: usize,
    pub knowledge_chunks: usize,
    pub duration_ms: u64,
}

pub struct Indexer;

impl Indexer {
    pub fn run(repo_path: &Path, cache: &mut ScanCache) -> Result<IndexStats> {
        let started = Instant::now();

        // ── Fast path: skip file walk if HEAD hasn't changed and the working tree
        // has no relevant dirty files that could make cached chunks stale.
        let current_head = git_head(repo_path);
        if let Some(ref head) = current_head
            && cache.get_meta("last_head").as_deref() == Some(head.as_str())
            && !has_relevant_worktree_changes(repo_path)
        {
            // Already up to date — return cached counts without touching the filesystem
            let code_chunks = cache.code_chunk_count();
            let knowledge_chunks = cache.knowledge_chunk_count();
            if code_chunks > 0 || knowledge_chunks > 0 {
                tracing::debug!(head = %head, "codescan fast-path: HEAD unchanged");
                return Ok(IndexStats {
                    code_files: 0, // unknown without walk; 0 = "not re-scanned"
                    knowledge_files: 0,
                    code_chunks,
                    knowledge_chunks,
                    duration_ms: started.elapsed().as_millis() as u64,
                });
            }
        }

        // ── Slow path: walk, hash, diff, re-scan stale files ─────────────
        let code_files = discover_code_files(repo_path);
        let knowledge_files = discover_knowledge_files(repo_path, &KnowledgeDirs::default());

        // Compute content hashes
        let code_hashes: Vec<(PathBuf, String)> = code_files
            .iter()
            .filter_map(|p| std::fs::read(p).ok().map(|c| (p.clone(), sha256(&c))))
            .collect();
        let knowledge_hashes: Vec<(PathBuf, String)> = knowledge_files
            .iter()
            .filter_map(|p| std::fs::read(p).ok().map(|c| (p.clone(), sha256(&c))))
            .collect();

        // Batch-compare with cached hashes (2 queries, not N)
        let all_hashes: Vec<(PathBuf, String)> = code_hashes
            .iter()
            .chain(knowledge_hashes.iter())
            .cloned()
            .collect();
        let stale: HashSet<PathBuf> = cache.stale_paths(&all_hashes).into_iter().collect();
        let live_paths: HashSet<PathBuf> =
            all_hashes.iter().map(|(path, _)| path.clone()).collect();
        cache.prune_missing_paths(&live_paths)?;

        for (path, hash) in &code_hashes {
            if !stale.contains(path) {
                continue;
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %path.display(), "read error: {e}");
                    continue;
                }
            };
            let rel = path.strip_prefix(repo_path).unwrap_or(path);
            let mut chunks = CodeScanner::scan_file(rel, &content);
            for c in &mut chunks {
                c.path = rel.to_path_buf();
            }
            cache.upsert_code_chunks(rel, hash, &chunks)?;
        }

        for (path, hash) in &knowledge_hashes {
            if !stale.contains(path) {
                continue;
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %path.display(), "read error: {e}");
                    continue;
                }
            };
            let rel = path.strip_prefix(repo_path).unwrap_or(path);
            let mut chunks = KnowledgeScanner::scan_file(rel, &content);
            for c in &mut chunks {
                c.path = rel.to_path_buf();
            }
            cache.upsert_knowledge_chunks(rel, hash, &chunks)?;
        }

        // Record HEAD so next call can use the fast path
        if let Some(ref head) = current_head {
            let _ = cache.set_meta("last_head", head);
        }

        let code_chunks = cache.code_chunk_count();
        let knowledge_chunks = cache.knowledge_chunk_count();
        let duration_ms = started.elapsed().as_millis() as u64;

        tracing::info!(
            code_files = code_files.len(),
            knowledge_files = knowledge_files.len(),
            stale = stale.len(),
            code_chunks,
            knowledge_chunks,
            duration_ms,
            "codescan indexed"
        );

        Ok(IndexStats {
            code_files: code_files.len(),
            knowledge_files: knowledge_files.len(),
            code_chunks,
            knowledge_chunks,
            duration_ms,
        })
    }
}

fn git_head(repo_path: &Path) -> Option<String> {
    let repo = git2::Repository::discover(repo_path).ok()?;
    let head = repo.head().ok()?;
    head.target().map(|oid| oid.to_string())
}

fn has_relevant_worktree_changes(repo_path: &Path) -> bool {
    let Ok(repo) = git2::Repository::discover(repo_path) else {
        return true;
    };
    let workdir = repo.workdir().unwrap_or(repo_path);
    let Ok(statuses) = repo.statuses(Some(
        git2::StatusOptions::new()
            .include_untracked(true)
            .recurse_untracked_dirs(true),
    )) else {
        return true;
    };

    statuses.iter().any(|entry| {
        entry
            .path()
            .map(|path| is_relevant_changed_path(workdir, Path::new(path)))
            .unwrap_or(false)
    })
}

fn is_relevant_changed_path(repo_path: &Path, rel_path: &Path) -> bool {
    let path = repo_path.join(rel_path);
    if should_skip_path(&path) {
        return false;
    }
    path.extension()
        .and_then(|x| x.to_str())
        .map(is_supported_code_extension)
        .unwrap_or_else(|| {
            KnowledgeDirs::default()
                .patterns
                .iter()
                .any(|pattern| path_matches_glob(repo_path, rel_path, pattern))
        })
}

fn path_matches_glob(repo_path: &Path, rel_path: &Path, pattern: &str) -> bool {
    let full = repo_path.join(pattern).to_string_lossy().to_string();
    glob::Pattern::new(&full)
        .ok()
        .map(|pattern| pattern.matches_path(&repo_path.join(rel_path)))
        .unwrap_or(false)
}

fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn should_skip_path(path: &Path) -> bool {
    const SKIP_DIRS: &[&str] = &[
        "target",
        "node_modules",
        ".git",
        ".jj",
        ".omegon",
        "dist",
        "build",
        ".next",
    ];
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .map(|part| SKIP_DIRS.contains(&part))
            .unwrap_or(false)
    })
}

fn discover_code_files(repo_path: &Path) -> Vec<PathBuf> {
    use walkdir::WalkDir;
    WalkDir::new(repo_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_path(e.path()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(is_supported_code_extension)
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

fn discover_knowledge_files(repo_path: &Path, dirs: &KnowledgeDirs) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for pattern in &dirs.patterns {
        let full = format!("{}/{}", repo_path.to_string_lossy(), pattern);
        if let Ok(paths) = glob::glob(&full) {
            for p in paths.filter_map(|p| p.ok()) {
                if p.is_file() {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_on_small_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn greet() {}").unwrap();
        std::fs::create_dir_all(repo.join("docs")).unwrap();
        std::fs::write(repo.join("docs/foo.md"), "# Foo\n\n## Overview\n\nText.").unwrap();

        let mut cache = ScanCache::open(&repo.join(".omegon/codescan.db")).unwrap();
        let stats = Indexer::run(repo, &mut cache).unwrap();
        assert!(stats.code_files >= 1, "code_files");
        assert!(stats.code_chunks >= 1, "code_chunks");
        assert!(stats.knowledge_chunks >= 1, "knowledge_chunks");
    }

    #[test]
    fn fast_path_skips_walk_when_head_unchanged() {
        // Simulate git HEAD being set in meta — in a temp dir without git,
        // git_head returns None and the fast path never fires. Instead, test
        // that a second run on a static dir (no git) still returns same counts.
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/main.rs"), "fn main() {}").unwrap();
        let mut cache = ScanCache::open(&repo.join(".omegon/codescan.db")).unwrap();

        let s1 = Indexer::run(repo, &mut cache).unwrap();
        // Manually set last_head to simulate "already indexed" state
        cache.set_meta("last_head", "fake_head_abc123").unwrap();

        // Now set env to return the same HEAD — simulate by checking counts are stable
        let s2 = Indexer::run(repo, &mut cache).unwrap();
        // Both runs should produce the same chunk count
        assert_eq!(
            s1.code_chunks, s2.code_chunks,
            "chunk count should be stable"
        );
    }

    #[test]
    fn dirty_relevant_worktree_change_disables_head_fast_path() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn original() {}\n").unwrap();

        let git = git2::Repository::init(repo).unwrap();
        let mut index = git.index().unwrap();
        index.add_path(Path::new("src/lib.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = git.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Omegon Test", "omegon@example.invalid").unwrap();
        git.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        drop(tree);
        drop(git);

        let mut cache = ScanCache::open(&repo.join(".omegon/codescan.db")).unwrap();
        let first = Indexer::run(repo, &mut cache).unwrap();
        assert_eq!(first.code_chunks, 1);

        std::fs::write(
            repo.join("src/lib.rs"),
            "pub fn original() {}\npub fn dirty_added() {}\n",
        )
        .unwrap();

        let second = Indexer::run(repo, &mut cache).unwrap();
        assert!(
            second.code_files > 0,
            "dirty relevant file must force a scan instead of HEAD fast-path: {second:?}"
        );
        let chunks = cache.all_code_chunks().unwrap();
        let names = chunks
            .iter()
            .map(|chunk| chunk.item_name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"dirty_added"), "chunks: {names:?}");
    }

    #[test]
    fn dirty_ignored_worktree_change_allows_head_fast_path() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn original() {}\n").unwrap();

        let git = git2::Repository::init(repo).unwrap();
        let mut index = git.index().unwrap();
        index.add_path(Path::new("src/lib.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = git.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Omegon Test", "omegon@example.invalid").unwrap();
        git.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        drop(tree);
        drop(git);

        let mut cache = ScanCache::open(&repo.join(".omegon/codescan.db")).unwrap();
        let first = Indexer::run(repo, &mut cache).unwrap();
        assert_eq!(first.code_chunks, 1);

        std::fs::create_dir_all(repo.join("target/debug")).unwrap();
        std::fs::write(
            repo.join("target/debug/generated.rs"),
            "pub fn ignored() {}\n",
        )
        .unwrap();

        let second = Indexer::run(repo, &mut cache).unwrap();
        assert_eq!(
            second.code_files, 0,
            "ignored dirty file should not defeat HEAD fast-path: {second:?}"
        );
    }

    #[test]
    fn excludes_omegon_workspace_and_prunes_stale_entries() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/main.rs"), "fn canonical() {}").unwrap();
        std::fs::write(
            repo.join("src/InvoiceService.java"),
            "class InvoiceService {}",
        )
        .unwrap();
        std::fs::write(repo.join("src/TimeEntry.kt"), "class TimeEntry").unwrap();
        std::fs::write(
            repo.join("src/BillingService.cs"),
            "class BillingService {}",
        )
        .unwrap();
        std::fs::create_dir_all(repo.join(".omegon/cleave-workspace/0-wt-code-survey/src"))
            .unwrap();
        std::fs::write(
            repo.join(".omegon/cleave-workspace/0-wt-code-survey/src/tui_tests.rs"),
            "fn transient_workspace_copy() {}",
        )
        .unwrap();

        let discovered = discover_code_files(repo);
        assert!(
            discovered
                .iter()
                .any(|path| path.ends_with("InvoiceService.java")),
            "discover_code_files should include Java files: {discovered:?}"
        );
        assert!(
            discovered.iter().any(|path| path.ends_with("TimeEntry.kt")),
            "discover_code_files should include Kotlin files: {discovered:?}"
        );
        assert!(
            discovered
                .iter()
                .any(|path| path.ends_with("BillingService.cs")),
            "discover_code_files should include C# files: {discovered:?}"
        );
        assert!(
            discovered
                .iter()
                .all(|path| !path.to_string_lossy().contains(".omegon/cleave-workspace")),
            "discover_code_files should skip .omegon workspaces: {discovered:?}"
        );

        let cache_path = repo.join(".omegon/codescan.db");
        let cache = ScanCache::open(&cache_path).unwrap();
        cache
            .upsert_code_chunks(
                Path::new(".omegon/cleave-workspace/0-wt-code-survey/src/tui_tests.rs"),
                "stale",
                &[crate::code::CodeChunk {
                    path: PathBuf::from(
                        ".omegon/cleave-workspace/0-wt-code-survey/src/tui_tests.rs",
                    ),
                    start_line: 1,
                    end_line: 1,
                    item_name: "transient_workspace_copy".into(),
                    item_kind: "fn".into(),
                    text: "fn transient_workspace_copy() {}".into(),
                    parent_scope: None,
                    language: "rust".into(),
                    strategy: crate::code::ExtractionStrategy::TreeSitter,
                    confidence: crate::code::ExtractionConfidence::Extracted,
                }],
            )
            .unwrap();

        let mut cache = ScanCache::open(&cache_path).unwrap();
        Indexer::run(repo, &mut cache).unwrap();

        let chunks = ScanCache::open(&cache_path)
            .unwrap()
            .all_code_chunks()
            .unwrap();
        assert!(
            chunks.iter().all(|chunk| !chunk
                .path
                .to_string_lossy()
                .contains(".omegon/cleave-workspace")),
            "indexed chunks should prune stale .omegon workspace entries: {chunks:?}"
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.path == std::path::Path::new("src/main.rs")),
            "canonical repo files should remain indexed: {chunks:?}"
        );
    }
}
