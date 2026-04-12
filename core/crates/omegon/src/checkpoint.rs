//! Turn-boundary state checkpointing — crash recovery for long-running sessions.
//!
//! Subscribes to [`AgentEvent::TurnEnd`] and appends a checkpoint record to an
//! append-only JSONL file. On crash recovery, the most recent checkpoint provides
//! enough metadata to verify conversation consistency and resume.
//!
//! Follows the same append-only pattern as `upstream_errors.rs`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single turn checkpoint entry, appended to JSONL after each turn.
#[derive(Debug, Serialize, Deserialize)]
pub struct TurnCheckpoint {
    pub timestamp_unix_ms: u64,
    pub session_id: String,
    pub turn: u32,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub estimated_tokens: usize,
    pub context_window: usize,
    pub actual_input_tokens: u64,
    pub actual_output_tokens: u64,
    pub intent: IntentSnapshot,
    pub metrics: MetricsSnapshot,
}

/// Snapshot of the intent document at checkpoint time.
#[derive(Debug, Serialize, Deserialize)]
pub struct IntentSnapshot {
    pub current_task: Option<String>,
    pub lifecycle_phase: String,
    pub files_read_count: usize,
    pub files_modified_count: usize,
    pub stats_turns: u32,
    pub stats_tool_calls: u32,
}

/// Snapshot of context metrics at checkpoint time.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub tokens_used: usize,
    pub context_window: usize,
    pub context_class: String,
    pub thinking_level: String,
}

/// Resolve the checkpoint JSONL path for a session.
pub fn checkpoint_path(session_id: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".omegon")
        .join("checkpoints")
        .join(format!("{session_id}.jsonl"))
}

/// Append a checkpoint entry. Follows the upstream_errors.rs append pattern:
/// create parent dirs, append a single JSONL line, set permissions to 0o600.
pub fn append_checkpoint(entry: &TurnCheckpoint) {
    use std::io::Write;
    let path = checkpoint_path(&entry.session_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(line) = serde_json::to_string(entry) else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let _ = writeln!(file, "{line}");
}

/// Read the last checkpoint for a session (for crash recovery verification).
pub fn read_last_checkpoint(session_id: &str) -> Option<TurnCheckpoint> {
    let path = checkpoint_path(session_id);
    let content = std::fs::read_to_string(&path).ok()?;
    content
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
}

/// Spawn a background task that subscribes to agent events and writes
/// checkpoints on each `TurnEnd`. Returns the join handle.
pub fn spawn_checkpoint_subscriber(
    events_tx: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    session_id: String,
    context_metrics: std::sync::Arc<
        std::sync::Mutex<crate::features::context::SharedContextMetrics>,
    >,
) -> tokio::task::JoinHandle<()> {
    let mut rx = events_tx.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let omegon_traits::AgentEvent::TurnEnd {
                turn,
                model,
                provider,
                estimated_tokens,
                context_window,
                actual_input_tokens,
                actual_output_tokens,
                intent_task,
                intent_phase,
                files_read_count,
                files_modified_count,
                stats_tool_calls,
                ..
            } = event
            {
                let metrics = context_metrics
                    .lock()
                    .map(|m| MetricsSnapshot {
                        tokens_used: m.tokens_used,
                        context_window: m.context_window,
                        context_class: m.context_class.clone(),
                        thinking_level: m.thinking_level.clone(),
                    })
                    .unwrap_or_else(|_| MetricsSnapshot {
                        tokens_used: 0,
                        context_window: 0,
                        context_class: "unknown".into(),
                        thinking_level: "unknown".into(),
                    });

                let entry = TurnCheckpoint {
                    timestamp_unix_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    session_id: session_id.clone(),
                    turn,
                    model,
                    provider,
                    estimated_tokens,
                    context_window,
                    actual_input_tokens,
                    actual_output_tokens,
                    intent: IntentSnapshot {
                        current_task: intent_task,
                        lifecycle_phase: intent_phase.unwrap_or_else(|| "unknown".into()),
                        files_read_count,
                        files_modified_count,
                        stats_turns: turn,
                        stats_tool_calls,
                    },
                    metrics,
                };
                append_checkpoint(&entry);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_roundtrip() {
        let entry = TurnCheckpoint {
            timestamp_unix_ms: 1712880000000,
            session_id: "test-session".into(),
            turn: 5,
            model: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            estimated_tokens: 45000,
            context_window: 200000,
            actual_input_tokens: 12000,
            actual_output_tokens: 3000,
            intent: IntentSnapshot {
                current_task: Some("implement auth".into()),
                lifecycle_phase: "Implementing".into(),
                files_read_count: 8,
                files_modified_count: 3,
                stats_turns: 5,
                stats_tool_calls: 12,
            },
            metrics: MetricsSnapshot {
                tokens_used: 45000,
                context_window: 200000,
                context_class: "clan".into(),
                thinking_level: "medium".into(),
            },
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: TurnCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.turn, 5);
        assert_eq!(parsed.session_id, "test-session");
        assert_eq!(
            parsed.intent.current_task.as_deref(),
            Some("implement auth")
        );
        assert_eq!(parsed.metrics.context_class, "clan");
    }

    #[test]
    fn read_last_checkpoint_from_empty_is_none() {
        assert!(read_last_checkpoint("nonexistent-session-id").is_none());
    }
}
