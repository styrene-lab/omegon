//! Clipboard paste retention.
//!
//! Clipboard image pastes (the `tui::pull_clipboard_image` path on
//! macOS via `osascript`, on Linux via `wl-paste` / `xclip`) are
//! written to the system temp directory under filenames like
//! `omegon-clipboard-{pid}-{counter}.{ext}`. Without explicit cleanup,
//! they accumulate forever — the operator screenshot that motivated
//! this module showed a four-month-old paste backlog in `/tmp`.
//!
//! This module provides a deterministic prune sweep that:
//!
//! 1. Walks the system temp directory.
//! 2. Filters to filenames matching the `omegon-clipboard-` prefix
//!    (no recursive descent, no other patterns — only files this
//!    process's clipboard path actually creates).
//! 3. Deletes any matching file whose modification time is older
//!    than the configured retention threshold.
//!
//! Sweep timing:
//!
//! - **Session start** (called from `main.rs`): once per launched
//!   instance, using `Settings.clipboard_retention_hours` as the
//!   threshold. This is the default cleanup point.
//! - **On demand** via the `/clipboard prune` slash command: same
//!   logic, runs immediately.
//!
//! Concurrency: multiple omegon processes share the same temp dir
//! and may write clipboard pastes concurrently. The prune intentionally
//! does NOT check whether a file's pid is still alive — a 24h-old
//! paste is stale regardless of which process owns it. Cross-process
//! safety comes from `std::fs::remove_file`'s atomic semantics: if
//! another process is mid-read, the file gets unlinked but the
//! reader's open handle keeps working.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Filename prefix that identifies an omegon clipboard paste. Must
/// stay in sync with the `format!("omegon-clipboard-{pid}-{counter}.{ext}")`
/// strings in `tui::mod::pull_clipboard_image`.
const CLIPBOARD_PREFIX: &str = "omegon-clipboard-";

/// Result of a single sweep.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PruneStats {
    /// Files matching the clipboard prefix that were considered.
    pub scanned: usize,
    /// Files actually deleted because they were older than the threshold.
    pub deleted: usize,
    /// Files skipped because they were newer than the threshold.
    pub skipped_recent: usize,
    /// Files where reading metadata or deleting failed (logged, not fatal).
    pub errors: usize,
}

impl PruneStats {
    /// Human-readable one-liner for logging or surfacing in the
    /// `/clipboard prune` slash command output.
    pub fn summary(&self) -> String {
        format!(
            "clipboard prune: deleted {}, kept {} recent, {} error(s) ({} scanned)",
            self.deleted, self.skipped_recent, self.errors, self.scanned
        )
    }
}

/// Prune clipboard pastes in the system temp directory older than
/// `retention`. A `retention` of `Duration::ZERO` disables the sweep
/// entirely (operator opt-out via `clipboard_retention_hours = 0`).
///
/// Returns a [`PruneStats`] summarizing what was touched. Errors on
/// individual files are recorded in `errors` and do not abort the
/// sweep — one unreadable file shouldn't block cleanup of the rest.
pub fn prune_old_pastes(retention: Duration) -> std::io::Result<PruneStats> {
    prune_old_pastes_in(&std::env::temp_dir(), retention)
}

/// Same as [`prune_old_pastes`] but operates on a caller-supplied
/// directory. Exists so tests can target a tempdir instead of the
/// real system temp directory.
pub fn prune_old_pastes_in(
    dir: &Path,
    retention: Duration,
) -> std::io::Result<PruneStats> {
    let mut stats = PruneStats::default();
    if retention.is_zero() {
        // Operator-disabled sweep. Scan the directory anyway so we
        // report a meaningful "0 deleted, N skipped" line, but never
        // delete anything.
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(stats),
            Err(err) => return Err(err),
        };
        for entry in entries.flatten() {
            if is_clipboard_file(&entry.path()) {
                stats.scanned += 1;
                stats.skipped_recent += 1;
            }
        }
        return Ok(stats);
    }

    let now = SystemTime::now();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(stats),
        Err(err) => return Err(err),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_clipboard_file(&path) {
            continue;
        }
        stats.scanned += 1;

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };
        let modified = match metadata.modified() {
            Ok(m) => m,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age < retention {
            stats.skipped_recent += 1;
            continue;
        }

        match std::fs::remove_file(&path) {
            Ok(()) => stats.deleted += 1,
            Err(_) => stats.errors += 1,
        }
    }
    Ok(stats)
}

/// Filter for files this module is allowed to touch. Match by:
/// - filename (not path) starts with the literal `omegon-clipboard-`
///   prefix
/// - is a regular file (not a directory or symlink target)
///
/// Anything else in the temp directory is invisible to the prune sweep,
/// even if it happens to be old. The match is intentionally narrow so
/// we never delete files this module didn't create.
fn is_clipboard_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(CLIPBOARD_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(dir: &Path, name: &str, age: Duration) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"test paste").unwrap();
        let mtime = SystemTime::now() - age;
        // Use filetime via libc-free std API: set_modified is on File on
        // recent Rust, but the simpler portable path is the `filetime`
        // crate. Since we don't want a new dep, fall back to manipulating
        // the file's mtime via the std API on platforms that support it.
        let f = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap();
        f.set_modified(mtime).unwrap();
        path
    }

    #[test]
    fn prune_deletes_files_older_than_retention() {
        let tmp = tempfile::tempdir().unwrap();
        let old = touch(tmp.path(), "omegon-clipboard-123-0.png", Duration::from_secs(48 * 3600));
        let recent = touch(tmp.path(), "omegon-clipboard-123-1.png", Duration::from_secs(60));
        let unrelated = touch(tmp.path(), "some-other-file.png", Duration::from_secs(48 * 3600));

        let stats = prune_old_pastes_in(tmp.path(), Duration::from_secs(24 * 3600)).unwrap();

        assert_eq!(stats.scanned, 2);
        assert_eq!(stats.deleted, 1);
        assert_eq!(stats.skipped_recent, 1);
        assert_eq!(stats.errors, 0);
        assert!(!old.exists(), "48h-old clipboard paste should be deleted");
        assert!(recent.exists(), "1m-old clipboard paste should be kept");
        assert!(
            unrelated.exists(),
            "unrelated files must NOT be touched by the sweep"
        );
    }

    #[test]
    fn prune_with_zero_retention_is_a_noop() {
        // Operator opt-out: clipboard_retention_hours = 0 disables the
        // sweep entirely. Files of any age are scanned but skipped.
        let tmp = tempfile::tempdir().unwrap();
        let old = touch(
            tmp.path(),
            "omegon-clipboard-1-0.png",
            Duration::from_secs(365 * 24 * 3600),
        );
        let stats = prune_old_pastes_in(tmp.path(), Duration::ZERO).unwrap();
        assert_eq!(stats.deleted, 0);
        assert_eq!(stats.skipped_recent, 1);
        assert!(old.exists(), "zero-retention sweep must not delete anything");
    }

    #[test]
    fn prune_handles_missing_directory_gracefully() {
        // Tests can run before the temp dir exists or in environments
        // where it was just removed. The sweep should return an empty
        // stats struct rather than erroring out.
        let nowhere = std::path::PathBuf::from("/var/empty/this-does-not-exist");
        let stats = prune_old_pastes_in(&nowhere, Duration::from_secs(3600)).unwrap();
        assert_eq!(stats, PruneStats::default());
    }

    #[test]
    fn prune_only_matches_omegon_clipboard_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        // These should NOT be considered clipboard files even though
        // they're old. Anything outside the omegon-clipboard- prefix
        // is invisible to the sweep.
        for name in [
            "screenshot.png",
            "clipboard-12-0.png",            // missing omegon- prefix
            "omegon-other-12-0.png",         // wrong middle word
            "OMEGON-CLIPBOARD-12-0.png",     // case-sensitive
            "prefix-omegon-clipboard-1.png", // prefix in middle, not start
        ] {
            touch(tmp.path(), name, Duration::from_secs(48 * 3600));
        }
        let stats = prune_old_pastes_in(tmp.path(), Duration::from_secs(3600)).unwrap();
        assert_eq!(stats.scanned, 0);
        assert_eq!(stats.deleted, 0);

        // All five files should still exist.
        for name in [
            "screenshot.png",
            "clipboard-12-0.png",
            "omegon-other-12-0.png",
            "OMEGON-CLIPBOARD-12-0.png",
            "prefix-omegon-clipboard-1.png",
        ] {
            assert!(
                tmp.path().join(name).exists(),
                "non-clipboard file {name:?} must NOT be deleted"
            );
        }
    }

    #[test]
    fn prune_stats_summary_is_human_readable() {
        let stats = PruneStats {
            scanned: 5,
            deleted: 3,
            skipped_recent: 2,
            errors: 0,
        };
        assert_eq!(
            stats.summary(),
            "clipboard prune: deleted 3, kept 2 recent, 0 error(s) (5 scanned)"
        );
    }
}
