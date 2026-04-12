//! change tool — atomic multi-file edits with automatic validation.
//!
//! Accepts an array of edits, applies them atomically (all-or-nothing),
//! and runs validation (type checker, linter) automatically. One tool call
//! replaces 3 edits + 1 bash.
//!
//! If any edit fails, all changes are rolled back.
//! If validation fails, changes are kept but errors are reported inline.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct EditSpec {
    pub file: String,
    #[serde(rename = "oldText", alias = "old")]
    pub old_text: String,
    #[serde(rename = "newText", alias = "new")]
    pub new_text: String,
}

/// Validation mode — determines what checks to run after edits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValidationMode {
    /// No validation
    None,
    /// Syntax check only (tree-sitter parse — not yet implemented, falls back to Standard)
    Quick,
    /// Syntax + type check (cargo check / tsc / ruff)
    Standard,
    /// Syntax + type check + affected tests
    Full,
}

impl ValidationMode {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "none" | "false" | "off" => Self::None,
            "quick" => Self::Quick,
            "standard" | "default" => Self::Standard,
            "full" => Self::Full,
            _ => Self::Standard,
        }
    }
}

/// Execute an atomic multi-file change.
///
/// 1. Snapshot all target files (for rollback)
/// 2. Apply all edits — if any fails, rollback everything
/// 3. Run validation if requested
/// 4. Return comprehensive result
pub async fn execute(
    edits: &[EditSpec],
    validate: ValidationMode,
    cwd: &Path,
    resolve_path: impl Fn(&str) -> Result<PathBuf>,
) -> Result<ToolResult> {
    if edits.is_empty() {
        anyhow::bail!("No edits provided");
    }

    // Phase 1: Resolve all paths and snapshot original content
    let mut snapshots: HashMap<PathBuf, String> = HashMap::new();
    let mut resolved_edits: Vec<(PathBuf, &str, &str)> = Vec::new();

    for edit in edits {
        let path = resolve_path(&edit.file)?;
        if !path.exists() {
            anyhow::bail!("File not found: {}", edit.file);
        }
        if !snapshots.contains_key(&path) {
            let content = tokio::fs::read_to_string(&path).await?;
            snapshots.insert(path.clone(), content);
        }
        resolved_edits.push((path, &edit.old_text, &edit.new_text));
    }

    // Phase 2: Validate all edits against ORIGINAL snapshots and resolve
    // byte offsets.  Checking against the original (rather than the
    // cumulatively modified content) prevents a previous edit's new_text
    // from creating phantom duplicates that fail the uniqueness check for
    // a later edit in the same batch.
    struct Positioned {
        index: usize,
        path: PathBuf,
        old_norm: String,
        new_norm: String,
        offset: usize,
    }
    let mut positioned: Vec<Positioned> = Vec::new();

    for (i, (path, old_text, new_text)) in resolved_edits.iter().enumerate() {
        let original = snapshots.get(path).cloned().unwrap_or_default();
        let normalized_original = original.replace("\r\n", "\n");
        let normalized_old = old_text.replace("\r\n", "\n");
        let normalized_new = new_text.replace("\r\n", "\n");

        let positions: Vec<usize> = normalized_original
            .match_indices(&normalized_old)
            .map(|(pos, _)| pos)
            .collect();

        if positions.is_empty() {
            let hint = super::edit::nearest_context(&original, old_text)
                .map(|h| {
                    format!("\n\nNearest matching context to help you anchor the next edit:\n{h}")
                })
                .unwrap_or_default();
            anyhow::bail!(
                "Edit {}/{}: could not find exact text in {}. All changes rolled back. Read the file again and anchor on the current source before retrying.{hint}",
                i + 1,
                edits.len(),
                edits[i].file
            );
        }

        if positions.len() > 1 {
            anyhow::bail!(
                "Edit {}/{}: found {} occurrences in {}. Text must be unique. All changes rolled back.",
                i + 1,
                edits.len(),
                positions.len(),
                edits[i].file
            );
        }

        positioned.push(Positioned {
            index: i,
            path: path.clone(),
            old_norm: normalized_old,
            new_norm: normalized_new,
            offset: positions[0],
        });
    }

    // Phase 3: Group by file, apply edits bottom-up (by descending offset)
    // so earlier replacements don't shift the byte positions of later ones.
    let mut written_files: HashMap<PathBuf, String> = HashMap::new();
    let mut results: Vec<(usize, String)> = Vec::new();

    let mut edits_by_file: HashMap<PathBuf, Vec<&Positioned>> = HashMap::new();
    for p in &positioned {
        edits_by_file.entry(p.path.clone()).or_default().push(p);
    }

    for (path, mut file_edits) in edits_by_file {
        file_edits.sort_by(|a, b| b.offset.cmp(&a.offset));

        let mut content = snapshots
            .get(&path)
            .cloned()
            .unwrap_or_default()
            .replace("\r\n", "\n");

        for edit in &file_edits {
            let end = edit.offset + edit.old_norm.len();
            let before = &content[..edit.offset];
            let after = &content[end..];
            let new_content = format!("{}{}{}", before, edit.new_norm, after);

            if new_content == content {
                results.push((
                    edit.index,
                    format!("  {}: no change (identical)", edits[edit.index].file),
                ));
                continue;
            }
            content = new_content;

            let old_lines = edit.old_norm.lines().count();
            let new_lines = edit.new_norm.lines().count();
            let diff = if old_lines == new_lines {
                format!("{old_lines} line(s)")
            } else {
                format!("{old_lines}→{new_lines} lines")
            };
            results.push((
                edit.index,
                format!("  ✓ {}: {diff}", edits[edit.index].file),
            ));
        }

        tokio::fs::write(&path, &content).await.map_err(|e| {
            tracing::error!("Write failed during atomic change, partial state: {e}");
            e
        })?;
        written_files.insert(path, content);
    }

    // Sort results back into the original edit order for stable output.
    results.sort_by_key(|(idx, _)| *idx);
    let results: Vec<String> = results.into_iter().map(|(_, s)| s).collect();

    let files_changed = written_files.len();
    let mut output = format!(
        "Applied {} edit(s) across {} file(s):\n{}",
        edits.len(),
        files_changed,
        results.join("\n")
    );

    // Phase 3: Validation
    if validate != ValidationMode::None && files_changed > 0 {
        let mut validation_results = Vec::new();
        let unique_files: Vec<&PathBuf> = written_files.keys().collect();

        for file in &unique_files {
            if let Some(val) = super::validate::validate_after_mutation(file, cwd).await {
                validation_results.push(val);
            }
        }

        if !validation_results.is_empty() {
            output.push_str("\n\nValidation:\n");
            output.push_str(&validation_results.join("\n"));
        }

        if validate == ValidationMode::Full {
            // Run affected tests
            if let Some(test_result) = run_affected_tests(cwd, &unique_files).await {
                output.push_str("\n\nTests:\n");
                output.push_str(&test_result);
            }
        }
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: output }],
        details: json!({
            "files_changed": files_changed,
            "edits_applied": edits.len(),
        }),
    })
}

/// Rollback all modified files to their snapshot state.
async fn rollback(snapshots: &HashMap<PathBuf, String>, written_files: &HashMap<PathBuf, String>) {
    for (path, original) in snapshots {
        if written_files.contains_key(path)
            && let Err(e) = tokio::fs::write(path, original).await
        {
            tracing::error!("Rollback failed for {}: {e}", path.display());
        }
    }
}

/// Run tests affected by the changed files. Very simple heuristic:
/// look for co-located test files.
async fn run_affected_tests(cwd: &Path, files: &[&PathBuf]) -> Option<String> {
    // Find test files co-located with changed files
    let mut test_files = Vec::new();
    for file in files {
        let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
            continue; // Skip files without a stem (binary, extensionless)
        };
        let Some(ext) = file.extension().and_then(|e| e.to_str()) else {
            continue; // Skip files without an extension
        };
        let Some(parent) = file.parent() else {
            continue;
        };

        let patterns = [
            format!("{stem}.test.{ext}"),
            format!("{stem}_test.{ext}"),
            format!("test_{stem}.{ext}"),
        ];

        for pattern in &patterns {
            let test_path = parent.join(pattern);
            if test_path.exists() {
                test_files.push(test_path);
            }
        }
    }

    if test_files.is_empty() {
        return None;
    }

    // Determine test runner by the first file's extension
    let ext = match files
        .first()
        .and_then(|f| f.extension())
        .and_then(|e| e.to_str())
    {
        Some(e) => e,
        None => return None,
    };
    let (cmd, args) = match ext {
        "rs" => ("cargo", vec!["test".to_string()]),
        "ts" | "tsx" => {
            let test_file_args: Vec<String> =
                test_files.iter().map(|p| p.display().to_string()).collect();
            ("npx", {
                let mut a = vec!["vitest".to_string(), "run".to_string()];
                a.extend(test_file_args);
                a
            })
        }
        "py" => {
            let test_file_args: Vec<String> =
                test_files.iter().map(|p| p.display().to_string()).collect();
            ("pytest", test_file_args)
        }
        _ => return None,
    };

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new(cmd)
            .args(&args)
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await;

    match output {
        Ok(Ok(o)) => {
            let exit = o.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let stdout = String::from_utf8_lossy(&o.stdout);
            if exit == 0 {
                Some(format!("✓ {} test file(s) passed", test_files.len()))
            } else {
                let combined = format!("{stdout}\n{stderr}");
                let tail: Vec<&str> = combined.lines().rev().take(10).collect();
                let tail_str: String = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                Some(format!("✗ Tests failed (exit {exit}):\n{tail_str}"))
            }
        }
        Ok(Err(e)) => Some(format!("Test runner error: {e}")),
        Err(_) => Some("Tests timed out after 60s".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as IoWrite;

    #[tokio::test]
    async fn atomic_multi_file_edit() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::File::create(&file_a)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        std::fs::File::create(&file_b)
            .unwrap()
            .write_all(b"foo bar baz")
            .unwrap();

        let edits = vec![
            EditSpec {
                file: "a.txt".into(),
                old_text: "hello".into(),
                new_text: "goodbye".into(),
            },
            EditSpec {
                file: "b.txt".into(),
                old_text: "foo".into(),
                new_text: "qux".into(),
            },
        ];

        let cwd = dir.path().to_path_buf();
        let resolve = |p: &str| Ok(cwd.join(p));
        let result = execute(&edits, ValidationMode::None, &cwd, resolve)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("2 edit(s) across 2 file(s)"));

        assert_eq!(std::fs::read_to_string(&file_a).unwrap(), "goodbye world");
        assert_eq!(std::fs::read_to_string(&file_b).unwrap(), "qux bar baz");
    }

    #[tokio::test]
    async fn rollback_on_second_edit_failure() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        std::fs::File::create(&file_a)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        std::fs::File::create(&file_b)
            .unwrap()
            .write_all(b"foo bar baz")
            .unwrap();

        let edits = vec![
            EditSpec {
                file: "a.txt".into(),
                old_text: "hello".into(),
                new_text: "goodbye".into(),
            },
            EditSpec {
                file: "b.txt".into(),
                old_text: "NONEXISTENT".into(),
                new_text: "qux".into(),
            },
        ];

        let cwd = dir.path().to_path_buf();
        let resolve = |p: &str| Ok(cwd.join(p));
        let err = execute(&edits, ValidationMode::None, &cwd, resolve)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("rolled back"));

        // file_a should be restored to original
        assert_eq!(std::fs::read_to_string(&file_a).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn multiple_edits_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("code.rs");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"fn foo() {}\nfn bar() {}\nfn baz() {}")
            .unwrap();

        let edits = vec![
            EditSpec {
                file: "code.rs".into(),
                old_text: "fn foo() {}".into(),
                new_text: "fn foo() -> i32 { 42 }".into(),
            },
            EditSpec {
                file: "code.rs".into(),
                old_text: "fn bar() {}".into(),
                new_text: "fn bar() -> bool { true }".into(),
            },
        ];

        let cwd = dir.path().to_path_buf();
        let resolve = |p: &str| Ok(cwd.join(p));
        let result = execute(&edits, ValidationMode::None, &cwd, resolve)
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("2 edit(s) across 1 file(s)"));

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(content.contains("fn foo() -> i32 { 42 }"));
        assert!(content.contains("fn bar() -> bool { true }"));
        assert!(content.contains("fn baz() {}"));
    }

    /// Regression: when edit 1 inserts text that contains edit 2's old_text,
    /// the old code would count 2 occurrences in the running content and bail
    /// with "Text must be unique".  The fix validates against the original
    /// snapshot so the phantom duplicate from edit 1's new_text is ignored.
    #[tokio::test]
    async fn edit_batch_does_not_fail_on_phantom_duplicate_from_prior_edit() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("footer.rs");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(
                b"fn helper() -> String { String::new() }\n\
                  fn format_version_text(v: Option<&str>) -> String {\n\
                      match v { Some(s) => s.into(), None => String::new() }\n\
                  }\n",
            )
            .unwrap();

        let edits = vec![
            // Edit 1: replace helper, inserting text that contains the
            // signature of the function that edit 2 will target.
            EditSpec {
                file: "footer.rs".into(),
                old_text: "fn helper() -> String { String::new() }".into(),
                new_text: "fn helper() -> String { format_version_text(None) }\n\
                           fn format_version_text(v: Option<&str>) -> String { String::from(\"new\") }"
                    .into(),
            },
            // Edit 2: replace the ORIGINAL format_version_text body.
            EditSpec {
                file: "footer.rs".into(),
                old_text: "fn format_version_text(v: Option<&str>) -> String {\n\
                      match v { Some(s) => s.into(), None => String::new() }\n\
                  }"
                    .into(),
                new_text: "fn format_version_text(v: Option<&str>) -> String {\n\
                      v.unwrap_or(\"unknown\").to_string()\n\
                  }"
                    .into(),
            },
        ];

        let cwd = dir.path().to_path_buf();
        let resolve = |p: &str| Ok(cwd.join(p));
        let result = execute(&edits, ValidationMode::None, &cwd, resolve)
            .await
            .expect("batch should succeed — phantom duplicate must not cause failure");
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("2 edit(s)"));

        let content = std::fs::read_to_string(&file).unwrap();
        // Edit 1's new helper body should be present
        assert!(content.contains("format_version_text(None)"));
        // Edit 2 should have replaced the original function body
        assert!(content.contains("unwrap_or(\"unknown\")"));
        // The original match body should be gone
        assert!(!content.contains("match v {"));
    }

    #[tokio::test]
    async fn empty_edits_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_path_buf();
        let resolve = |p: &str| Ok(cwd.join(p));
        let err = execute(&[], ValidationMode::None, &cwd, resolve)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No edits"));
    }

    #[tokio::test]
    async fn failed_edit_error_includes_nearest_context() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lib.rs");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"fn compute() -> i32 {\n    42\n}\n")
            .unwrap();

        // Wrong body — but key line "fn compute() -> i32 {" is present
        let edits = vec![EditSpec {
            file: "lib.rs".into(),
            old_text: "fn compute() -> i32 {\n    99\n}".into(),
            new_text: "fn compute() -> i32 { 0 }".into(),
        }];

        let cwd = dir.path().to_path_buf();
        let resolve = |p: &str| Ok(cwd.join(p));
        let err = execute(&edits, ValidationMode::None, &cwd, resolve)
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("rolled back"), "msg: {msg}");
        assert!(
            msg.contains("Read the file again and anchor on the current source"),
            "msg: {msg}"
        );
        assert!(msg.contains("Nearest matching context"), "msg: {msg}");
        // Context hint should surface the actual line
        assert!(
            msg.contains("fn compute()"),
            "expected nearest-context hint in error, got: {msg}"
        );
    }
}
