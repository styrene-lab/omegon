//! Bash tool — execute shell commands with output capture.

use anyhow::Result;
use omegon_traits::{ContentBlock, ToolResult};
use std::path::Path;
use std::time::Instant;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const MAX_OUTPUT_LINES: usize = 2000;

pub async fn execute(
    command: &str,
    cwd: &Path,
    timeout_secs: Option<u64>,
    cancel: CancellationToken,
) -> Result<ToolResult> {
    let start = Instant::now();

    let mut cmd = Command::new("bash");
    cmd.args(["-c", command])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let child = cmd.spawn()?;

    let output = tokio::select! {
        result = child.wait_with_output() => result?,
        _ = cancel.cancelled() => {
            anyhow::bail!("Command aborted");
        }
        _ = async {
            if let Some(secs) = timeout_secs {
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            anyhow::bail!("Command timed out after {} seconds", timeout_secs.unwrap());
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = output.status.code().unwrap_or(-1);

    // Combine stdout + stderr
    let mut full_output = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !full_output.is_empty() {
            full_output.push('\n');
        }
        full_output.push_str(&stderr);
    }

    // Tail-truncate if needed
    let truncated = truncate_tail(&full_output);
    let mut text = truncated.content;

    if exit_code != 0 {
        text.push_str(&format!("\n\nCommand exited with code {exit_code}"));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text }],
        details: serde_json::json!({
            "exitCode": exit_code,
            "durationMs": duration_ms,
            "truncated": truncated.was_truncated,
            "totalLines": truncated.total_lines,
            "totalBytes": truncated.total_bytes,
        }),
    })
}

struct Truncated {
    content: String,
    was_truncated: bool,
    total_lines: usize,
    total_bytes: usize,
}

fn truncate_tail(output: &str) -> Truncated {
    let total_bytes = output.len();
    let lines: Vec<&str> = output.lines().collect();
    let total_lines = lines.len();

    if total_bytes <= MAX_OUTPUT_BYTES && total_lines <= MAX_OUTPUT_LINES {
        return Truncated {
            content: output.to_string(),
            was_truncated: false,
            total_lines,
            total_bytes,
        };
    }

    // Take the last N lines within byte budget
    let mut kept = Vec::new();
    let mut bytes = 0;
    for line in lines.iter().rev() {
        let line_bytes = line.len() + 1; // +1 for newline
        if bytes + line_bytes > MAX_OUTPUT_BYTES || kept.len() >= MAX_OUTPUT_LINES {
            break;
        }
        kept.push(*line);
        bytes += line_bytes;
    }
    kept.reverse();

    let content = kept.join("\n");
    Truncated {
        content,
        was_truncated: true,
        total_lines,
        total_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_tail_no_truncation() {
        let output = "line1\nline2\nline3";
        let result = truncate_tail(output);
        assert!(!result.was_truncated);
        assert_eq!(result.total_lines, 3);
        assert_eq!(result.content, output);
    }

    #[test]
    fn truncate_tail_by_lines() {
        let output = (0..3000).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let result = truncate_tail(&output);
        assert!(result.was_truncated);
        assert_eq!(result.total_lines, 3000);
        assert!(result.content.lines().count() <= MAX_OUTPUT_LINES);
        // Should keep the LAST lines (tail)
        assert!(result.content.contains("line 2999"));
    }

    #[test]
    fn truncate_tail_by_bytes() {
        let output = (0..100).map(|_| "x".repeat(1000)).collect::<Vec<_>>().join("\n");
        let result = truncate_tail(&output);
        assert!(result.was_truncated);
        assert!(result.content.len() <= MAX_OUTPUT_BYTES);
    }

    #[test]
    fn truncate_empty() {
        let result = truncate_tail("");
        assert!(!result.was_truncated);
        assert_eq!(result.total_lines, 0);
    }

    #[tokio::test]
    async fn execute_echo() {
        let cancel = CancellationToken::new();
        let result = execute("echo hello", Path::new("."), None, cancel).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("hello"), "should contain output: {text}");
        assert_eq!(result.details["exitCode"], 0);
    }

    #[tokio::test]
    async fn execute_nonzero_exit() {
        let cancel = CancellationToken::new();
        let result = execute("exit 42", Path::new("."), None, cancel).await.unwrap();
        assert_eq!(result.details["exitCode"], 42);
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("42"), "should mention exit code: {text}");
    }

    #[tokio::test]
    async fn execute_stderr() {
        let cancel = CancellationToken::new();
        let result = execute("echo err >&2", Path::new("."), None, cancel).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("err"), "should capture stderr: {text}");
    }

    #[tokio::test]
    async fn execute_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = execute("sleep 10", Path::new("."), None, cancel).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_timeout() {
        let cancel = CancellationToken::new();
        let result = execute("sleep 10", Path::new("."), Some(1), cancel).await;
        assert!(result.is_err());
    }
}
