//! Edit tool — find exact text and replace, with uniqueness verification.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use std::path::Path;

/// Timeout for filesystem operations during edit.
const EDIT_TIMEOUT_SECS: u64 = 30;

pub async fn execute(path: &Path, old_text: &str, new_text: &str, cwd: &Path) -> Result<ToolResult> {
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    let timeout = std::time::Duration::from_secs(EDIT_TIMEOUT_SECS);
    let content = tokio::time::timeout(timeout, tokio::fs::read_to_string(path))
        .await
        .map_err(|_| anyhow::anyhow!("Read timed out after {EDIT_TIMEOUT_SECS}s: {}", path.display()))??;

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
        if fuzzy_content.contains(&fuzzy_old) {
            anyhow::bail!(
                "Could not find the exact text in {}. A similar match exists but \
                 whitespace differs. The old text must match exactly including all \
                 whitespace and newlines.",
                path.display()
            );
        }
        anyhow::bail!(
            "Could not find the exact text in {}. The old text must match exactly \
             including all whitespace and newlines.",
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

    tokio::time::timeout(timeout, tokio::fs::write(path, &final_content))
        .await
        .map_err(|_| anyhow::anyhow!("Write timed out after {EDIT_TIMEOUT_SECS}s: {}", path.display()))??;

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

    // Run post-mutation validation
    let validation = super::validate::validate_after_mutation(path, cwd).await;
    let mut result_text = format!("Successfully replaced text in {}.", path.display());
    if let Some(ref val) = validation {
        result_text.push('\n');
        result_text.push_str(val);
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: result_text }],
        details: serde_json::json!({
            "path": path.display().to_string(),
            "diff": diff_summary,
            "oldLines": old_lines,
            "newLines": new_lines,
            "validation": validation,
        }),
    })
}

/// Normalize whitespace for fuzzy matching.
fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
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

        let result = execute(&file, "foo bar", "replaced", dir.path()).await.unwrap();
        assert!(result.content[0].clone().into_text().contains("Successfully"));

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

        let err = execute(&file, "foo", "replaced", dir.path()).await.unwrap_err();
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

        let err = execute(&file, "not found", "replaced", dir.path()).await.unwrap_err();
        assert!(err.to_string().contains("Could not find"));
    }
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
