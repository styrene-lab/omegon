//! Write tool — create or overwrite files, auto-creating parent directories.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use std::path::Path;

/// Write timeout — 30 seconds for filesystem operations.
const WRITE_TIMEOUT_SECS: u64 = 30;

pub async fn execute(path: &Path, content: &str, cwd: &Path) -> Result<ToolResult> {
    // Create parent directories if needed
    if let Some(parent) = path.parent()
        && !parent.exists() {
            tokio::fs::create_dir_all(parent).await?;
        }

    let timeout = std::time::Duration::from_secs(WRITE_TIMEOUT_SECS);
    let created = !path.exists();
    tokio::time::timeout(timeout, tokio::fs::write(path, content))
        .await
        .map_err(|_| anyhow::anyhow!("Write timed out after {WRITE_TIMEOUT_SECS}s: {}", path.display()))??;

    let line_count = content.lines().count();
    let byte_count = content.len();
    let action = if created { "Created" } else { "Wrote" };

    // Run post-mutation validation for source files
    let validation = super::validate::validate_after_mutation(path, cwd).await;
    let mut result_text = format!(
        "{action} {path} ({line_count} lines, {byte_count} bytes)",
        path = path.display()
    );
    if let Some(ref val) = validation {
        result_text.push('\n');
        result_text.push_str(val);
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: result_text }],
        details: serde_json::json!({
            "path": path.display().to_string(),
            "created": created,
            "lines": line_count,
            "bytes": byte_count,
            "validation": validation,
        }),
    })
}
