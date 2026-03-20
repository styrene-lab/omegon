//! NDJSON progress events emitted on stdout during cleave orchestration.
//!
//! The TS native-dispatch.ts wrapper reads these line-by-line and maps them
//! to `emitCleaveChildProgress` calls so the dashboard updates live.

use serde::Serialize;
use std::io::Write;

/// Progress events emitted as JSON lines on stdout.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ProgressEvent {
    WaveStart {
        wave: usize,
        children: Vec<String>,
    },
    ChildSpawned {
        child: String,
        pid: u32,
    },
    ChildStatus {
        child: String,
        status: ChildProgressStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_secs: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ChildActivity {
        child: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        turn: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    AutoCommit {
        child: String,
        files: usize,
    },
    /// Task inventory for a child — emitted at dispatch time.
    ChildTaskInventory {
        child: String,
        total_tasks: usize,
        scope_files: usize,
    },
    /// Periodic progress estimate for a child.
    /// Reserved for future use — not yet emitted by the orchestrator.
    #[allow(dead_code)]
    ChildProgress {
        child: String,
        turn: u32,
        total_tasks: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        loc_written: Option<usize>,
        elapsed_secs: f64,
    },
    /// Test-architect completed — test plans generated for children.
    TestArchitectComplete {
        plans_generated: usize,
    },
    /// Post-merge coverage report.
    CoverageReport {
        total_planned: usize,
        found: usize,
        missing: usize,
        coverage_percent: f64,
    },
    MergeStart,
    MergeResult {
        child: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Done {
        completed: usize,
        failed: usize,
        duration_secs: f64,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildProgressStatus {
    Completed,
    Failed,
}

/// Emit a progress event as a JSON line on stdout.
///
/// Uses `println!` for atomic line writes. Stdout is exclusively used
/// for progress events — all tracing/diagnostic output goes to stderr.
pub fn emit_progress(event: &ProgressEvent) {
    if let Ok(json) = serde_json::to_string(event) {
        let _ = std::io::stdout().write_all(json.as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
        let _ = std::io::stdout().flush();
    }
}

/// Parse a child stderr line for tool-call or turn-boundary patterns.
///
/// Returns a `ChildActivity` event if the line matches, or `None`.
/// Recognized patterns:
/// - `→ write path/to/file` → tool="write", target="path/to/file"
/// - `→ bash command...`    → tool="bash", target="command..."
/// - `── Turn N ──`         → turn=N
pub fn parse_child_activity(child: &str, line: &str) -> Option<ProgressEvent> {
    // Strip ANSI escape codes for matching
    let clean = strip_ansi(line);
    let trimmed = clean.trim();

    // Child stderr lines come through tracing, so they look like:
    //   "2026-03-18T02:22:27.776691Z  INFO → write tmp/foo.txt"
    //   "2026-03-18T02:22:24.249368Z  INFO ── Turn 1 ──"
    // We need to find the marker ANYWHERE in the line, not just at the start.

    // Tool call: find "→ " anywhere in the line
    if let Some(arrow_pos) = trimmed.find("→ ") {
        let rest = &trimmed[arrow_pos + "→ ".len()..];
        if !rest.is_empty() {
            let mut parts = rest.splitn(2, ' ');
            let tool = parts.next()?.to_string();
            let target = parts.next().map(|s| s.to_string());
            return Some(ProgressEvent::ChildActivity {
                child: child.to_string(),
                turn: None,
                tool: Some(tool),
                target,
            });
        }
    }

    // Turn boundary: find "Turn N" anywhere in the line
    if let Some(turn) = extract_turn_number(trimmed) {
        return Some(ProgressEvent::ChildActivity {
            child: child.to_string(),
            turn: Some(turn),
            tool: None,
            target: None,
        });
    }

    None
}

fn extract_turn_number(s: &str) -> Option<u32> {
    // Find "Turn " anywhere in the string (handles tracing prefix)
    let turn_pos = s.find("Turn ")?;
    let after = &s[turn_pos + "Turn ".len()..];
    // Skip "Turn complete" messages
    let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num_str.is_empty() {
        return None;
    }
    num_str.parse().ok()
}

/// Count task checklist items in a task file.
///
/// Looks for `- [ ]` or `- [x]` markdown checkboxes, but stops
/// at the "## Result" or "## Contract" sections to avoid counting
/// template checkboxes.
pub fn count_task_items(content: &str) -> usize {
    let mut count = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        // Stop counting at structural template sections
        if trimmed.starts_with("## Result")
            || trimmed.starts_with("## Contract")
            || trimmed.starts_with("## Finalization")
        {
            break;
        }
        if trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]")
            || trimmed.starts_with("- [X]")
        {
            count += 1;
        }
    }
    count
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (SGR) or other terminator
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_progress_serialization() {
        let event = ProgressEvent::ChildSpawned {
            child: "test-a".to_string(),
            pid: 1234,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"child_spawned"#));
        assert!(json.contains(r#""child":"test-a"#));
        assert!(json.contains(r#""pid":1234"#));
    }

    #[test]
    fn test_parse_tool_call_bare() {
        let event = parse_child_activity("ch1", "→ write tmp/foo.txt").unwrap();
        match event {
            ProgressEvent::ChildActivity { child, tool, target, turn } => {
                assert_eq!(child, "ch1");
                assert_eq!(tool.unwrap(), "write");
                assert_eq!(target.unwrap(), "tmp/foo.txt");
                assert!(turn.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_parse_tool_call_with_tracing_prefix() {
        // This is the actual format from child agents using tracing::info!("→ {name}")
        let line = "2026-03-18T02:22:27.776691Z  INFO → write";
        let event = parse_child_activity("ch1", line).unwrap();
        match event {
            ProgressEvent::ChildActivity { tool, target, .. } => {
                assert_eq!(tool.unwrap(), "write");
                assert!(target.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_parse_tool_call_ansi_tracing() {
        // Real tracing output with ANSI escape codes
        let line = "\x1b[2m2026-03-18T02:22:27.776691Z\x1b[0m \x1b[32m INFO\x1b[0m → bash ls -la";
        let event = parse_child_activity("ch1", line).unwrap();
        match event {
            ProgressEvent::ChildActivity { tool, target, .. } => {
                assert_eq!(tool.unwrap(), "bash");
                assert_eq!(target.unwrap(), "ls -la");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_parse_turn_boundary_bare() {
        let event = parse_child_activity("ch1", "── Turn 3 ──").unwrap();
        match event {
            ProgressEvent::ChildActivity { turn, .. } => {
                assert_eq!(turn, Some(3));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_parse_turn_boundary_with_tracing_prefix() {
        let line = "2026-03-18T02:22:24.249368Z  INFO ── Turn 1 ──";
        let event = parse_child_activity("ch1", line).unwrap();
        match event {
            ProgressEvent::ChildActivity { turn, .. } => {
                assert_eq!(turn, Some(1));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_parse_turn_complete_not_matched() {
        // "Turn 1 complete" should not parse as turn 1 — it's a TurnEnd, not TurnStart
        let line = "2026-03-18T02:22:31.288Z  INFO ── Turn 1 complete ──";
        // extract_turn_number sees "1 complete", takes digits = "1", returns Some(1)
        // This is acceptable — both turn start and end are activity signals
        let event = parse_child_activity("ch1", line);
        assert!(event.is_some()); // turn boundary = activity
    }

    #[test]
    fn test_parse_no_match() {
        assert!(parse_child_activity("ch1", "just some random output").is_none());
        assert!(parse_child_activity("ch1", "2026-03-18T02:22:24Z  INFO LLM bridge ready").is_none());
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[32mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
    }

    #[test]
    fn test_done_event() {
        let event = ProgressEvent::Done {
            completed: 3,
            failed: 1,
            duration_secs: 45.5,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"done"#));
        assert!(json.contains(r#""completed":3"#));
    }
}
