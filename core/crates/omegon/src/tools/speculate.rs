//! speculate tool — git checkpoint/rollback for exploratory changes.
//!
//! speculate_start(label) — create a named checkpoint (git stash snapshot)
//! speculate_check()      — run validation against the current state
//! speculate_commit()     — keep the changes, discard the checkpoint
//! speculate_rollback()   — revert to checkpoint, discard changes
//!
//! Uses git stash internally for lightweight, reliable checkpointing.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use serde_json::json;
use std::path::Path;
use std::sync::Mutex;
use tokio::process::Command;

/// Active speculation state — at most one at a time.
static ACTIVE: Mutex<Option<SpeculationState>> = Mutex::new(None);

#[derive(Debug, Clone)]
struct SpeculationState {
    label: String,
    /// The stash ref created at speculation start (e.g., "stash@{0}")
    stash_ref: Option<String>,
    /// Files that were modified at checkpoint time (for reporting)
    dirty_files: Vec<String>,
}

/// Start a new speculation — checkpoint the current working tree.
pub async fn start(label: &str, cwd: &Path) -> Result<ToolResult> {
    {
        let guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref active) = *guard {
            anyhow::bail!(
                "Already speculating: '{}'. Commit or rollback before starting a new speculation.",
                active.label
            );
        }
    }

    // Capture current dirty files
    let dirty = git_command(cwd, &["diff", "--name-only"]).await?;
    let staged = git_command(cwd, &["diff", "--staged", "--name-only"]).await?;
    let dirty_files: Vec<String> = dirty
        .lines()
        .chain(staged.lines())
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect();

    // Create a stash of the current state (including untracked files)
    // git stash push --include-untracked --message "speculate: <label>"
    // If there's nothing to stash, that's fine — we still track the checkpoint
    let stash_msg = format!("omegon-speculate: {label}");
    let stash_result = git_command(
        cwd,
        &["stash", "push", "--include-untracked", "--message", &stash_msg],
    )
    .await;

    let stash_ref = match stash_result {
        Ok(output) if output.contains("No local changes") => {
            // Nothing to stash — clean working tree
            None
        }
        Ok(_) => Some("stash@{0}".to_string()),
        Err(e) => {
            // git stash can fail if not in a git repo
            anyhow::bail!("Failed to create checkpoint: {e}");
        }
    };

    // If we stashed, pop it back — we want the working tree to remain as-is.
    // The stash is our rollback point. We immediately restore the state.
    if stash_ref.is_some() {
        // Apply (not pop) so the stash stays in the stash list
        let _ = git_command(cwd, &["stash", "apply", "--index"]).await;
    }

    let state = SpeculationState {
        label: label.to_string(),
        stash_ref: stash_ref.clone(),
        dirty_files: dirty_files.clone(),
    };

    {
        let mut guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(state);
    }

    let checkpoint_info = if stash_ref.is_some() {
        format!(
            "Checkpoint created with {} dirty file(s). Make your changes freely — \
             use speculate_rollback to undo everything back to this point.",
            dirty_files.len()
        )
    } else {
        "Checkpoint created (clean working tree). Make your changes freely — \
         use speculate_rollback to undo everything back to this point."
            .to_string()
    };

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!("Speculation '{}' started. {}", label, checkpoint_info),
        }],
        details: json!({
            "label": label,
            "dirty_files_at_checkpoint": dirty_files,
            "has_stash": stash_ref.is_some(),
        }),
    })
}

/// Check the current state — run validation on all modified files.
pub async fn check(cwd: &Path) -> Result<ToolResult> {
    let label = {
        let guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(ref s) => s.label.clone(),
            None => anyhow::bail!("No active speculation. Call speculate_start first."),
        }
    };

    // Find currently modified files
    let diff = git_command(cwd, &["diff", "--name-only"]).await.unwrap_or_default();
    let staged = git_command(cwd, &["diff", "--staged", "--name-only"]).await.unwrap_or_default();
    let modified: Vec<String> = diff
        .lines()
        .chain(staged.lines())
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect();

    // Run validation on each modified file
    let mut validations = Vec::new();
    for file in &modified {
        let path = cwd.join(file);
        if let Some(val) = super::validate::validate_after_mutation(&path, cwd).await {
            validations.push(format!("  {file}: {val}"));
        }
    }

    let validation_summary = if validations.is_empty() {
        "No validators matched the modified files.".to_string()
    } else {
        validations.join("\n")
    };

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Speculation '{}' check:\n  {} file(s) modified since checkpoint.\n\n{}",
                label,
                modified.len(),
                validation_summary
            ),
        }],
        details: json!({
            "label": label,
            "modified_files": modified,
        }),
    })
}

/// Commit the speculation — keep all changes, discard the checkpoint.
pub async fn commit(cwd: &Path) -> Result<ToolResult> {
    let state = {
        let mut guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        match guard.take() {
            Some(s) => s,
            None => anyhow::bail!("No active speculation to commit."),
        }
    };

    // Drop the stash — changes are kept as-is
    if state.stash_ref.is_some() {
        let _ = git_command(cwd, &["stash", "drop", "stash@{0}"]).await;
    }

    // Report what changed since checkpoint
    let diff_stat = git_command(cwd, &["diff", "--stat"]).await.unwrap_or_default();

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Speculation '{}' committed. Changes preserved.\n{}",
                state.label,
                if diff_stat.is_empty() {
                    "No uncommitted changes.".to_string()
                } else {
                    diff_stat
                }
            ),
        }],
        details: json!({ "label": state.label }),
    })
}

/// Rollback the speculation — revert to checkpoint, discard all changes since.
pub async fn rollback(cwd: &Path) -> Result<ToolResult> {
    let state = {
        let mut guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        match guard.take() {
            Some(s) => s,
            None => anyhow::bail!("No active speculation to rollback."),
        }
    };

    // Discard all current changes
    let _ = git_command(cwd, &["checkout", "--", "."]).await;
    // Remove untracked files created during speculation
    let _ = git_command(cwd, &["clean", "-fd"]).await;

    // If we had a stash, apply it to restore the pre-speculation dirty state
    if state.stash_ref.is_some() {
        let _ = git_command(cwd, &["stash", "pop", "--index"]).await;
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text {
            text: format!(
                "Speculation '{}' rolled back. Working tree restored to checkpoint state.",
                state.label
            ),
        }],
        details: json!({
            "label": state.label,
            "restored_dirty_files": state.dirty_files,
        }),
    })
}

/// Run a git command and return stdout.
async fn git_command(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {}: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Global speculation state means these tests must run serially.
    // We use a shared mutex to serialize all speculation tests.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_global() {
        let mut guard = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    #[tokio::test]
    async fn start_requires_git_repo() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_global();
        let dir = tempfile::tempdir().unwrap();
        let result = start("test", dir.path()).await;
        assert!(result.is_err());
        reset_global();
    }

    #[tokio::test]
    async fn commit_without_active_fails() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_global();
        let dir = tempfile::tempdir().unwrap();
        let result = commit(dir.path()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No active"));
    }

    #[tokio::test]
    async fn rollback_without_active_fails() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_global();
        let dir = tempfile::tempdir().unwrap();
        let result = rollback(dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn full_lifecycle_in_git_repo() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        // Reset global state
        {
            let mut guard = ACTIVE.lock().unwrap();
            *guard = None;
        }

        // Init git repo
        git_command(cwd, &["init"]).await.unwrap();
        git_command(cwd, &["config", "user.email", "test@test.com"]).await.unwrap();
        git_command(cwd, &["config", "user.name", "Test"]).await.unwrap();

        // Create and commit a file
        std::fs::write(cwd.join("file.txt"), "original").unwrap();
        git_command(cwd, &["add", "."]).await.unwrap();
        git_command(cwd, &["commit", "-m", "initial"]).await.unwrap();

        // Start speculation
        let result = start("try-approach-a", cwd).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("try-approach-a"));

        // Make a change
        std::fs::write(cwd.join("file.txt"), "modified").unwrap();

        // Check
        let result = check(cwd).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("1 file(s) modified"));

        // Rollback
        let result = rollback(cwd).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("rolled back"));

        // File should be restored
        assert_eq!(std::fs::read_to_string(cwd.join("file.txt")).unwrap(), "original");
    }

    #[tokio::test]
    async fn commit_keeps_changes() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_global();
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        // Init git repo
        git_command(cwd, &["init"]).await.unwrap();
        git_command(cwd, &["config", "user.email", "test@test.com"]).await.unwrap();
        git_command(cwd, &["config", "user.name", "Test"]).await.unwrap();

        std::fs::write(cwd.join("file.txt"), "original").unwrap();
        git_command(cwd, &["add", "."]).await.unwrap();
        git_command(cwd, &["commit", "-m", "initial"]).await.unwrap();

        // Start, modify, commit
        start("keep-it", cwd).await.unwrap();
        std::fs::write(cwd.join("file.txt"), "kept changes").unwrap();
        commit(cwd).await.unwrap();

        // Changes should be preserved
        assert_eq!(std::fs::read_to_string(cwd.join("file.txt")).unwrap(), "kept changes");
    }
}
