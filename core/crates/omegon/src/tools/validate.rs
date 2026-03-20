//! Post-mutation validation — run the appropriate checker after file edits.
//!
//! Discovers project configuration (Cargo.toml, tsconfig.json, etc.) and
//! runs the lightest validation command relevant to the edited file.
//! Results are appended to the tool result, not returned as a separate call.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;

/// Maximum time to wait for a validation command to complete.
/// cargo check on a large project can take a while; 30s is generous
/// but prevents indefinite hangs from build locks or broken toolchains.
const VALIDATION_TIMEOUT_SECS: u64 = 30;

/// Cached project validators, keyed by the cwd they were discovered from.
/// Re-discovers if cwd changes (Phase 1 multi-project support).
static VALIDATORS: Mutex<Option<(PathBuf, HashMap<ValidatorKind, ValidatorConfig>)>> =
    Mutex::new(None);

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
enum ValidatorKind {
    Rust,
    TypeScript,
    Python,
}

#[derive(Debug, Clone)]
struct ValidatorConfig {
    command: String,
    args: Vec<String>,
}

/// Run validation for a file that was just modified.
/// Returns None if no validator applies, or Some(summary) with results.
pub async fn validate_after_mutation(file_path: &Path, cwd: &Path) -> Option<String> {
    let kind = validator_for_file(file_path)?;
    let config = {
        let validators = discover_validators(cwd);
        validators.get(&kind)?.clone()
    };

    let child = Command::new("bash")
        .args(["-c", &format!("{} {}", config.command, config.args.join(" "))])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();

    let result = tokio::time::timeout(Duration::from_secs(VALIDATION_TIMEOUT_SECS), child).await;

    match result {
        Ok(Ok(output)) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);

            if exit_code == 0 {
                Some(format!("Validation (`{}`): ✓ passed", config.command))
            } else {
                // Extract just the error lines, not the full output
                let errors = extract_error_summary(&stdout, &stderr, &kind);
                Some(format!(
                    "Validation (`{}`): ✗ {} error(s)\n{}",
                    config.command,
                    count_errors(&errors),
                    truncate_validation(&errors, 500),
                ))
            }
        }
        Ok(Err(e)) => {
            tracing::debug!("Validation command failed to execute: {e}");
            None // Don't report if the validator itself fails to run
        }
        Err(_) => {
            tracing::warn!(
                "Validation timed out after {}s for `{}`",
                VALIDATION_TIMEOUT_SECS,
                config.command
            );
            Some(format!(
                "Validation (`{}`): ⏱ timed out after {}s",
                config.command, VALIDATION_TIMEOUT_SECS
            ))
        }
    }
}

/// Determine which validator applies to a file based on extension.
fn validator_for_file(path: &Path) -> Option<ValidatorKind> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Some(ValidatorKind::Rust),
        Some("ts" | "tsx" | "js" | "jsx" | "mts" | "cts") => Some(ValidatorKind::TypeScript),
        Some("py") => Some(ValidatorKind::Python),
        _ => None,
    }
}

/// Discover available validators by scanning for project config files.
/// Caches results per-cwd — re-discovers if cwd changes.
fn discover_validators(cwd: &Path) -> HashMap<ValidatorKind, ValidatorConfig> {
    let mut guard = VALIDATORS.lock().unwrap_or_else(|e| e.into_inner());

    // Return cached if cwd matches
    if let Some((ref cached_cwd, ref validators)) = *guard
        && cached_cwd == cwd {
            return validators.clone();
        }

    // Discover fresh
    let mut validators = HashMap::new();

    // Rust: look for Cargo.toml
    if find_upward(cwd, "Cargo.toml").is_some() {
        validators.insert(
            ValidatorKind::Rust,
            ValidatorConfig {
                command: "cargo".into(),
                args: vec!["check".into(), "--message-format=short".into()],
            },
        );
    }

    // TypeScript: look for tsconfig.json
    if find_upward(cwd, "tsconfig.json").is_some() {
        validators.insert(
            ValidatorKind::TypeScript,
            ValidatorConfig {
                command: "npx".into(),
                args: vec!["tsc".into(), "--noEmit".into(), "--pretty".into()],
            },
        );
    }

    // Python: look for pyproject.toml with mypy or ruff
    if find_upward(cwd, "pyproject.toml").is_some() {
        // Prefer ruff (fast) over mypy (slow)
        validators.insert(
            ValidatorKind::Python,
            ValidatorConfig {
                command: "ruff".into(),
                args: vec!["check".into(), "--quiet".into()],
            },
        );
    }

    *guard = Some((cwd.to_path_buf(), validators.clone()));
    validators
}

/// Walk up from `start` looking for a file named `name`.
fn find_upward(start: &Path, name: &str) -> Option<std::path::PathBuf> {
    let mut dir = start;
    loop {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

/// Extract error-relevant lines from validator output.
fn extract_error_summary(stdout: &str, stderr: &str, kind: &ValidatorKind) -> String {
    let combined = format!("{stdout}\n{stderr}");

    match kind {
        ValidatorKind::Rust => {
            // cargo check --message-format=short outputs "file:line:col: error[E0xxx]: msg"
            combined
                .lines()
                .filter(|l| l.contains("error") || l.contains("warning"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        ValidatorKind::TypeScript => {
            // tsc outputs "file(line,col): error TSxxxx: msg"
            combined
                .lines()
                .filter(|l| l.contains("error TS") || l.contains(": error"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        ValidatorKind::Python => {
            // ruff outputs "file:line:col: EXXX msg"
            combined
                .lines()
                .filter(|l| {
                    !l.is_empty()
                        && !l.starts_with("Found")
                        && !l.starts_with("All checks")
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// Count approximate number of errors from summary text.
fn count_errors(summary: &str) -> usize {
    summary.lines().filter(|l| !l.is_empty()).count()
}

/// Truncate validation output to stay within a byte budget.
/// Safe for multi-byte UTF-8 — finds the last char boundary before the limit.
fn truncate_validation(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    // Find the last valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &text[..end];
    if let Some(last_nl) = truncated.rfind('\n') {
        format!("{}\n... (truncated)", &truncated[..last_nl])
    } else {
        format!("{}... (truncated)", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_for_known_extensions() {
        assert_eq!(
            validator_for_file(Path::new("foo.rs")),
            Some(ValidatorKind::Rust)
        );
        assert_eq!(
            validator_for_file(Path::new("bar.ts")),
            Some(ValidatorKind::TypeScript)
        );
        assert_eq!(
            validator_for_file(Path::new("baz.py")),
            Some(ValidatorKind::Python)
        );
        assert!(validator_for_file(Path::new("readme.md")).is_none());
        assert!(validator_for_file(Path::new("config.json")).is_none());
    }

    #[test]
    fn truncation_at_line_boundary() {
        let text = "line one\nline two\nline three\nline four";
        let truncated = truncate_validation(text, 20);
        assert!(truncated.contains("truncated"));
        assert!(!truncated.contains("line three"));
    }

    #[test]
    fn truncation_safe_for_multibyte_utf8() {
        // "café" has a 2-byte é (0xC3 0xA9) — cutting at byte 4 would
        // split the multi-byte character. This must not panic.
        let text = "café\nbar\nbaz";
        let truncated = truncate_validation(text, 4);
        assert!(truncated.contains("truncated"));
        // Should have backed up to byte 3 ("caf") rather than panicking
        assert!(!truncated.contains('é'));
    }

    #[test]
    fn error_count() {
        assert_eq!(count_errors("error 1\nerror 2\n"), 2);
        assert_eq!(count_errors(""), 0);
        assert_eq!(count_errors("one\n\ntwo"), 2);
    }
}
