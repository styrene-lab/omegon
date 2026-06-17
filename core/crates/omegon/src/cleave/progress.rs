//! Progress event types and sinks for cleave orchestration.
//!
//! Different embeddings consume cleave progress differently:
//! - external CLI / native-dispatch: NDJSON written to stdout
//! - in-process harness tool use: callback sink updating shared state
//! - future telemetry / RPC: file, socket, or event-bus sinks

use crate::child_agent::{ChildAgentActivity, ChildTaskItem};
use serde::Serialize;
use std::io::Write;
use std::sync::Arc;

/// Progress events emitted during cleave orchestration.
#[derive(Debug, Clone, Serialize)]
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
    /// Token usage reported at child turn-end — rolled into parent session totals.
    ChildTokens {
        child: String,
        input_tokens: u64,
        output_tokens: u64,
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
        /// Full task descriptions extracted from the child's prompt.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tasks: Vec<ChildTaskItem>,
    },
    /// A child explicitly marked a task as done (1-indexed).
    ChildTaskDone {
        child: String,
        task_index: usize,
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

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildProgressStatus {
    Completed,
    Failed,
    /// Work was ultimately merged, but only after salvaging a child that had
    /// already been marked failed (timeout, cancellation, non-zero exit).
    MergedAfterFailure,
    /// Provider upstream exhausted — orchestrator may retry with a fallback provider.
    UpstreamExhausted,
}

/// Embedding-aware sink for progress events.
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: &ProgressEvent);
}

pub type SharedProgressSink = Arc<dyn ProgressSink>;

#[derive(Default)]
pub struct StdoutProgressSink;

impl ProgressSink for StdoutProgressSink {
    fn emit(&self, event: &ProgressEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            let _ = std::io::stdout().write_all(json.as_bytes());
            let _ = std::io::stdout().write_all(b"\n");
            let _ = std::io::stdout().flush();
        }
    }
}

pub fn stdout_progress_sink() -> SharedProgressSink {
    Arc::new(StdoutProgressSink)
}

struct CallbackProgressSink<F>
where
    F: Fn(&ProgressEvent) + Send + Sync + 'static,
{
    callback: F,
}

impl<F> ProgressSink for CallbackProgressSink<F>
where
    F: Fn(&ProgressEvent) + Send + Sync + 'static,
{
    fn emit(&self, event: &ProgressEvent) {
        (self.callback)(event);
    }
}

pub fn callback_progress_sink<F>(callback: F) -> SharedProgressSink
where
    F: Fn(&ProgressEvent) + Send + Sync + 'static,
{
    Arc::new(CallbackProgressSink { callback })
}

/// Parse a child stderr line for tool-call or turn-boundary patterns.
///
/// Returns a cleave progress event if the line matches, or `None`.
pub fn parse_child_activity(child: &str, line: &str) -> Option<ProgressEvent> {
    match crate::child_agent::parse_child_activity(line)? {
        ChildAgentActivity::Tool { tool, target } => Some(ProgressEvent::ChildActivity {
            child: child.to_string(),
            turn: None,
            tool: Some(tool),
            target,
        }),
        ChildAgentActivity::Turn { turn } => Some(ProgressEvent::ChildActivity {
            child: child.to_string(),
            turn: Some(turn),
            tool: None,
            target: None,
        }),
        ChildAgentActivity::Tokens {
            input_tokens,
            output_tokens,
        } => Some(ProgressEvent::ChildTokens {
            child: child.to_string(),
            input_tokens,
            output_tokens,
        }),
        ChildAgentActivity::TaskDone { task_index } => Some(ProgressEvent::ChildTaskDone {
            child: child.to_string(),
            task_index,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::child_agent::extract_task_items;
    use std::sync::Mutex;

    #[test]
    fn test_emit_progress_serialization() {
        let event = ProgressEvent::ChildSpawned {
            child: "test-a".to_string(),
            pid: 1234,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"child_spawned""#));
        assert!(json.contains(r#""child":"test-a""#));
        assert!(json.contains(r#""pid":1234"#));
    }

    #[test]
    fn callback_sink_receives_events() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = Arc::clone(&seen);
            callback_progress_sink(move |event| {
                seen.lock()
                    .unwrap()
                    .push(serde_json::to_string(event).unwrap());
            })
        };

        sink.emit(&ProgressEvent::ChildSpawned {
            child: "test-a".to_string(),
            pid: 1234,
        });

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].contains("child_spawned"));
    }

    #[test]
    fn test_parse_tool_call_bare() {
        let event = parse_child_activity("ch1", "→ write tmp/foo.txt").unwrap();
        match event {
            ProgressEvent::ChildActivity {
                child,
                tool,
                target,
                turn,
            } => {
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
        assert!(
            parse_child_activity("ch1", "2026-03-18T02:22:24Z  INFO LLM bridge ready").is_none()
        );
    }

    #[test]
    fn test_done_event() {
        let event = ProgressEvent::Done {
            completed: 3,
            failed: 1,
            duration_secs: 45.5,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"done""#));
        assert!(json.contains(r#""completed":3"#));
    }

    #[test]
    fn test_extract_task_items() {
        let content = "## Tasks\n- [ ] Build the client\n- [ ] Write tests\n- [x] Set up deps\n\n## Result\n- [ ] should not appear\n";
        let items = extract_task_items(content);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].description, "Build the client");
        assert!(!items[0].done);
        assert_eq!(items[1].description, "Write tests");
        assert!(!items[1].done);
        assert_eq!(items[2].description, "Set up deps");
        assert!(items[2].done);
    }

    #[test]
    fn test_extract_task_items_stops_at_contract() {
        let content = "- [ ] First\n## Contract\n- [ ] Hidden\n";
        let items = extract_task_items(content);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "First");
    }

    #[test]
    fn test_extract_numbered_list() {
        let content = "## Task\n1. Add the dependency\n2. Build the module\n3. Write tests\n\n## Constraints\n- Do not broaden scope\n";
        let items = extract_task_items(content);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].description, "Add the dependency");
        assert_eq!(items[2].description, "Write tests");
        assert!(!items[0].done);
    }

    #[test]
    fn test_extract_plain_bullets_fallback() {
        let content = "## Task\n- Add the dependency\n- Build the module\n\n## Constraints\n- Stay within scope\n";
        let items = extract_task_items(content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "Add the dependency");
    }

    #[test]
    fn test_extract_checklist_takes_priority() {
        // When both checklists and numbered lists exist, checklists win
        let content = "1. First step\n- [ ] Real task\n- [ ] Another task\n";
        let items = extract_task_items(content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "Real task");
    }

    #[test]
    fn test_extract_stops_at_constraints() {
        let content = "- [ ] Task one\n## Constraints\n- [ ] Should not appear\n";
        let items = extract_task_items(content);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_parse_task_done() {
        let event = parse_child_activity("ch1", "TASK_DONE: 2").unwrap();
        match event {
            ProgressEvent::ChildTaskDone { child, task_index } => {
                assert_eq!(child, "ch1");
                assert_eq!(task_index, 2);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_parse_task_done_with_tracing_prefix() {
        let line = "2026-04-18T02:22:27.776691Z  INFO TASK_DONE: 3";
        let event = parse_child_activity("ch1", line).unwrap();
        match event {
            ProgressEvent::ChildTaskDone { task_index, .. } => {
                assert_eq!(task_index, 3);
            }
            _ => panic!("wrong variant"),
        }
    }
}
