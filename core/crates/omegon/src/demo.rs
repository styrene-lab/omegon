//! Bundled demo project — extract, git-init, and exec into it.
//!
//! The demo project (test-project/) is embedded as a compressed tarball
//! at compile time. On launch, it is extracted to a temp directory,
//! initialized as a fresh git repository, and omegon is exec()'d into it.
//!
//! The tarball is ~5KB and adds negligible binary size.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The bundled demo project tarball (test-project/, no .git, no target/).
static DEMO_TARBALL: &[u8] = include_bytes!("../assets/demo-project.tar.gz");

/// Path where the demo project is extracted.
fn demo_dir() -> PathBuf {
    std::env::temp_dir().join("omegon-demo")
}

/// Extract the bundled tarball to the demo dir, initialize a git repo,
/// and exec() omegon into it.
///
/// On Unix, this replaces the current process (exec). On success it never
/// returns. On failure, returns an error string so the caller can surface it.
pub fn launch_bundled_demo() -> Result<(), String> {
    let dir = demo_dir();

    // Extract — always re-extract to ensure a clean, known state
    extract_tarball(&dir).map_err(|e| format!("extract: {e}"))?;

    // Initialize a fresh git repo so cleave worktrees work
    init_git_repo(&dir).map_err(|e| format!("git init: {e}"))?;

    // Restore terminal before exec
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = io::stdout().flush();

    let exe = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("omegon"));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(&exe)
            .args(["--no-splash", "--context-class", "compact", "--tutorial"])
            .current_dir(&dir)
            .exec();
        Err(format!("exec failed: {err}"))
    }

    #[cfg(not(unix))]
    {
        Command::new(&exe)
            .args(["--no-splash", "--context-class", "compact", "--tutorial"])
            .current_dir(&dir)
            .spawn()
            .map_err(|e| format!("spawn: {e}"))?;
        std::process::exit(0);
    }
}

/// Extract the embedded tarball to `dest/`.
/// The tarball contains `test-project/` as the top-level prefix;
/// we strip it so dest/ becomes the project root.
fn extract_tarball(dest: &Path) -> io::Result<()> {
    // Remove old extraction
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::create_dir_all(dest)?;

    // Decompress with flate2, unpack with tar
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(std::io::Cursor::new(DEMO_TARBALL));
    let mut archive = Archive::new(gz);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Strip the leading "test-project/" prefix
        let stripped = path.components()
            .skip(1)
            .collect::<PathBuf>();

        if stripped.as_os_str().is_empty() {
            continue; // root entry
        }

        let target = dest.join(&stripped);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&target)?;
        }
    }

    Ok(())
}

/// Initialize a fresh git repository and make a single initial commit.
/// This is required for cleave to create worktrees.
fn init_git_repo(dir: &Path) -> io::Result<()> {
    let run = |args: &[&str]| -> io::Result<()> {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                format!("git {} failed", args[0])
            ))
        }
    };

    run(&["init"])?;
    run(&["config", "user.email", "demo@omegon.dev"])?;
    run(&["config", "user.name", "Omegon Demo"])?;
    run(&["add", "-A"])?;
    run(&["commit", "-m", "Initial demo project", "--allow-empty"])?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn tarball_is_non_empty() {
        assert!(!DEMO_TARBALL.is_empty(), "demo tarball should be embedded");
        assert!(DEMO_TARBALL.len() > 100, "tarball too small to be valid");
    }

    #[test]
    fn tarball_extracts_cleanly() {
        let tmp = TempDir::new().unwrap();
        extract_tarball(tmp.path()).unwrap();

        // Key files for the sprint board demo
        assert!(tmp.path().join("index.html").exists(), "index.html missing");
        assert!(tmp.path().join("src/board.js").exists(), "src/board.js missing");
        assert!(tmp.path().join("src/styles.css").exists(), "src/styles.css missing");
        assert!(tmp.path().join("ai/docs/sprint-board-overview.md").exists(), "design docs missing");
        assert!(tmp.path().join("ai/openspec/changes/fix-board-bugs/tasks.md").exists(), "tasks.md missing");
        assert!(tmp.path().join("ai/memory/facts.jsonl").exists(), "facts.jsonl missing");
    }

    #[test]
    fn tarball_no_git_dir() {
        let tmp = TempDir::new().unwrap();
        extract_tarball(tmp.path()).unwrap();
        // .git should NOT be in the tarball — we create it fresh
        assert!(!tmp.path().join(".git").exists(), ".git should not be in tarball");
    }
}
