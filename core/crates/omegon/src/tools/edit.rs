//! Edit tool — find exact text and replace, with uniqueness verification.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use std::path::Path;

/// Timeout for filesystem operations during edit.
const EDIT_TIMEOUT_SECS: u64 = 30;

pub async fn execute(path: &Path, old_text: &str, new_text: &str) -> Result<ToolResult> {
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    let timeout = std::time::Duration::from_secs(EDIT_TIMEOUT_SECS);
    let content = tokio::time::timeout(timeout, tokio::fs::read_to_string(path))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Read timed out after {EDIT_TIMEOUT_SECS}s: {}",
                path.display()
            )
        })??;

    // Normalize line endings for matching
    let normalized = content.replace("\r\n", "\n");
    let normalized_old = old_text.replace("\r\n", "\n");
    let normalized_new = new_text.replace("\r\n", "\n");

    // Count occurrences
    let count = normalized.matches(&normalized_old).count();

    if count == 0 {
        // Try fuzzy match — normalize whitespace
        let fuzzy_content = normalize_whitespace(&normalized);
        let fuzzy_old = normalize_whitespace(&normalized_old);
        let hint = nearest_context(&normalized, &normalized_old)
            .map(|h| format!("\n{h}"))
            .unwrap_or_default();
        if fuzzy_content.contains(&fuzzy_old) {
            anyhow::bail!(
                "Could not find the exact text in {}. A similar match exists but \
                 whitespace differs. The old text must match exactly including all \
                 whitespace and newlines.{hint}",
                path.display()
            );
        }
        anyhow::bail!(
            "Could not find the exact text in {}. The old text must match exactly \
             including all whitespace and newlines.{hint}",
            path.display()
        );
    }

    if count > 1 {
        anyhow::bail!(
            "Found {count} occurrences of the text in {}. The text must be unique. \
             Please provide more context to make it unique.",
            path.display()
        );
    }

    // Perform replacement
    let new_content = normalized.replacen(&normalized_old, &normalized_new, 1);

    if new_content == normalized {
        anyhow::bail!(
            "No changes made to {}. The replacement produced identical content.",
            path.display()
        );
    }

    // Restore original line endings if the file used CRLF
    let final_content = if content.contains("\r\n") && !new_content.contains("\r\n") {
        new_content.replace('\n', "\r\n")
    } else {
        new_content
    };

    // TOCTOU protection: re-read the file and verify it hasn't changed
    // since we read it. If another process (user, build tool, another agent)
    // modified the file between our read and write, abort rather than
    // silently clobbering their changes.
    let current = tokio::time::timeout(timeout, tokio::fs::read_to_string(path))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Re-read timed out after {EDIT_TIMEOUT_SECS}s: {}",
                path.display()
            )
        })??;
    if current != content {
        anyhow::bail!(
            "File {} was modified by another process since it was read. \
             Your edit was NOT applied to avoid overwriting external changes. \
             Read the file again to see the current content, then retry.",
            path.display()
        );
    }

    tokio::time::timeout(timeout, tokio::fs::write(path, &final_content))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Write timed out after {EDIT_TIMEOUT_SECS}s: {}",
                path.display()
            )
        })??;

    // Generate a simple diff summary
    let old_lines = normalized_old.lines().count();
    let new_lines = normalized_new.lines().count();
    let diff_summary = if old_lines == new_lines {
        format!("Changed {old_lines} line(s)")
    } else if new_lines > old_lines {
        format!(
            "Changed {old_lines} → {new_lines} lines (+{} added)",
            new_lines - old_lines
        )
    } else {
        format!(
            "Changed {old_lines} → {new_lines} lines (-{} removed)",
            old_lines - new_lines
        )
    };

    let result_text = format!("Successfully replaced text in {}.", path.display());

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: result_text }],
        details: serde_json::json!({
            "path": path.display().to_string(),
            "diff": diff_summary,
            "oldLines": old_lines,
            "newLines": new_lines,
        }),
    })
}

/// Normalize whitespace for fuzzy matching.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Find the nearest region in `content` that contains a line from `old_text`,
/// and return a short diagnostic snippet.
///
/// Used to enrich "could not find exact text" errors so the caller can
/// correct `oldText` without an extra `read` round-trip.
pub(super) fn nearest_context(content: &str, old_text: &str) -> Option<String> {
    let file_lines: Vec<&str> = content.lines().collect();

    for key_line in old_text.lines() {
        let trimmed = key_line.trim();
        if trimmed.len() < 6 {
            continue;
        }

        // Prefer exact-trim match, fall back to substring. If this line doesn't
        // match anything, continue scanning later lines from old_text.
        let Some(idx) = file_lines
            .iter()
            .position(|l| l.trim() == trimmed)
            .or_else(|| file_lines.iter().position(|l| l.contains(trimmed)))
        else {
            continue;
        };

        let start = idx.saturating_sub(2);
        let end = (idx + 4).min(file_lines.len());

        let context: String = file_lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, l)| format!("  {:>4}: {l}", start + i + 1))
            .collect::<Vec<_>>()
            .join("\n");

        let old_preview: String = old_text
            .lines()
            .take(5)
            .map(|l| format!("  {l}"))
            .collect::<Vec<_>>()
            .join("\n");

        return Some(format!(
            "oldText (first 5 lines):\n{old_preview}\n\nNearest match in file (lines {}-{}):\n{context}",
            start + 1,
            end
        ));
    }
    None
}

// Helper for tests
trait ContentBlockExt {
    fn into_text(self) -> String;
}

impl ContentBlockExt for ContentBlock {
    fn into_text(self) -> String {
        match self {
            ContentBlock::Text { text } => text,
            ContentBlock::Image { .. } => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn edit_replaces_exact_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"hello world\nfoo bar\nbaz")
            .unwrap();

        let result = execute(&file, "foo bar", "replaced").await.unwrap();
        assert!(
            result.content[0]
                .clone()
                .into_text()
                .contains("Successfully")
        );

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello world\nreplaced\nbaz");
    }

    #[tokio::test]
    async fn edit_rejects_ambiguous_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"foo\nfoo\nbar")
            .unwrap();

        let err = execute(&file, "foo", "replaced").await.unwrap_err();
        assert!(err.to_string().contains("2 occurrences"));
    }

    #[tokio::test]
    async fn edit_rejects_missing_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();

        let err = execute(&file, "not found", "replaced").await.unwrap_err();
        assert!(err.to_string().contains("Could not find"));
    }

    #[test]
    fn nearest_context_finds_matching_line() {
        let content = "fn foo() {}\nlet stable_anchor = compute_value();\nfn baz() {}";
        // First line differs, but a later significant line matches and should anchor the hint.
        let old_text = "fn different() {}\nlet stable_anchor = compute_value();\n";
        let hint = nearest_context(content, old_text).unwrap();
        assert!(hint.contains("stable_anchor"), "hint: {hint}");
        assert!(hint.contains("Nearest match"), "hint: {hint}");
    }

    #[test]
    fn nearest_context_returns_none_for_no_match() {
        let content = "fn foo() {}";
        let result = nearest_context(content, "completely_absent_xyz");
        assert!(result.is_none());
    }

    #[test]
    fn nearest_context_skips_trivial_lines() {
        // Only has short/whitespace lines — should return None
        let content = "a\nb\nc";
        let result = nearest_context(content, "x\ny");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn missing_text_error_includes_nearest_context() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"fn hello() {\n    println!(\"hi\");\n}\n")
            .unwrap();

        // oldText has slightly wrong content but a recognizable key line
        let err = execute(&file, "fn hello() {\n    println!(\"wrong\");\n}", "x")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Could not find"), "msg: {msg}");
        // Hint should be present since "fn hello()" is in the file
        assert!(
            msg.contains("fn hello()"),
            "expected context hint, got: {msg}"
        );
    }
}
