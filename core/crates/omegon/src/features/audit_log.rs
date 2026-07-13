//! Audit log — persistent structured event trail for postmortem and diagnostics.
//!
//! Writes a JSONL file at `.omegon/audit-log.jsonl` with every significant
//! event in the session. Each line is a self-contained JSON object.
//!
//! Events captured:
//! - session_start / session_end
//! - turn_end (model, tokens, OODA phase, drift, progress, context breakdown)
//! - tool_start (name, args summary)
//! - tool_end (name, result preview, error flag, details)
//! - permission_decision (path, approve/deny)
//! - nudge_injected (reason, message preview)
//! - compacted (context was compacted)
//!
//! Diagnostic queries:
//!   jq 'select(.kind=="nudge")' .omegon/audit-log.jsonl
//!   jq 'select(.kind=="tool_end" and .is_error==true)' .omegon/audit-log.jsonl
//!   jq 'select(.kind=="permission")' .omegon/audit-log.jsonl
//!   jq 'select(.kind=="turn") | {turn, phase, drift}' .omegon/audit-log.jsonl

use async_trait::async_trait;
use omegon_traits::{BusEvent, BusRequest, ContentBlock, Feature};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

fn agent_event_kind(event: &omegon_traits::AgentEvent) -> &'static str {
    match event {
        omegon_traits::AgentEvent::TurnStart { .. } => "turn_start",
        omegon_traits::AgentEvent::MessageStart { .. } => "message_start",
        omegon_traits::AgentEvent::MessageChunk { .. } => "message_chunk",
        omegon_traits::AgentEvent::ThinkingChunk { .. } => "thinking_chunk",
        omegon_traits::AgentEvent::MessageEnd => "message_end",
        omegon_traits::AgentEvent::MessageAbort { .. } => "message_abort",
        omegon_traits::AgentEvent::ToolStart { .. } => "tool_start",
        omegon_traits::AgentEvent::ToolUpdate { .. } => "tool_update",
        omegon_traits::AgentEvent::ToolEnd { .. } => "tool_end",
        omegon_traits::AgentEvent::PermissionRequest { .. } => "permission_request",
        omegon_traits::AgentEvent::OperatorWaitRequest { .. } => "operator_wait_request",
        omegon_traits::AgentEvent::TurnEnd(_) => "turn_end",
        omegon_traits::AgentEvent::AgentEnd => "agent_end",
        omegon_traits::AgentEvent::PhaseChanged { .. } => "phase_changed",
        omegon_traits::AgentEvent::DecompositionStarted { .. } => "decomposition_started",
        omegon_traits::AgentEvent::DecompositionChildCompleted { .. } => {
            "decomposition_child_completed"
        }
        omegon_traits::AgentEvent::DecompositionCompleted { .. } => "decomposition_completed",
        omegon_traits::AgentEvent::FamilyVitalSignsUpdated { .. } => "family_vital_signs_updated",
        omegon_traits::AgentEvent::RouteChanged { .. } => "route_changed",
        omegon_traits::AgentEvent::SkillActivation { .. } => "skill_activation",
        omegon_traits::AgentEvent::RuntimeLifecycleUpdated { .. } => "runtime_lifecycle_updated",
        omegon_traits::AgentEvent::SystemNotification { .. } => "system_notification",
        omegon_traits::AgentEvent::OperatorCopyBlock { .. } => "operator_copy_block",
        omegon_traits::AgentEvent::StreamIdle { .. } => "stream_idle",
        omegon_traits::AgentEvent::ProviderRetry { .. } => "provider_retry",
        omegon_traits::AgentEvent::ProviderFailure { .. } => "provider_failure",
        omegon_traits::AgentEvent::TurnCancelled { .. } => "turn_cancelled",
        omegon_traits::AgentEvent::PlanUpdated { .. } => "plan_updated",
        omegon_traits::AgentEvent::HarnessStatusChanged { .. } => "harness_status_changed",
        omegon_traits::AgentEvent::WebDashboardStarted { .. } => "web_dashboard_started",
        omegon_traits::AgentEvent::RuntimeQueueUpdated { .. } => "runtime_queue_updated",
        omegon_traits::AgentEvent::RuntimeTurnLifecycleUpdated { .. } => {
            "runtime_turn_lifecycle_updated"
        }
        omegon_traits::AgentEvent::RuntimePromptStarted { .. } => "runtime_prompt_started",
        omegon_traits::AgentEvent::ContextUpdated { .. } => "context_updated",
        omegon_traits::AgentEvent::ContextCompaction { .. } => "context_compaction",
        omegon_traits::AgentEvent::SessionReset => "session_reset",
    }
}

/// Maximum audit log size before rotation (5 MB).
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Number of rotated archives to keep (audit-log.1.jsonl, .2.jsonl, .3.jsonl).
const MAX_ROTATED_FILES: usize = 3;

pub struct AuditLog {
    path: PathBuf,
    session_id: String,
    /// Bytes written this session — avoids stat() on every append.
    bytes_written: u64,
    /// Checked once at startup to seed bytes_written.
    size_checked: bool,
    tool_starts: HashMap<String, u64>,
    tool_updates: HashMap<String, ToolUpdateStats>,
}

#[derive(Debug, Default, Clone)]
struct ToolUpdateStats {
    count: u64,
    heartbeat_count: u64,
    first_update_ms: Option<u64>,
    last_update_ms: Option<u64>,
    max_tail_chars: usize,
}

impl AuditLog {
    pub fn new(cwd: &std::path::Path, session_id: &str) -> Self {
        let dir = crate::setup::find_project_root(cwd).join(".omegon");
        let _ = fs::create_dir_all(&dir);
        Self {
            path: dir.join("audit-log.jsonl"),
            session_id: session_id.to_string(),
            bytes_written: 0,
            size_checked: false,
            tool_starts: HashMap::new(),
            tool_updates: HashMap::new(),
        }
    }

    fn append(&mut self, entry: &AuditEntry) {
        // Lazy size check on first write — avoids startup I/O.
        if !self.size_checked {
            self.size_checked = true;
            self.bytes_written = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
            if self.bytes_written >= MAX_LOG_BYTES {
                self.rotate();
            }
        }

        let Ok(json) = serde_json::to_string(entry) else {
            return;
        };
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let _ = writeln!(file, "{json}");
        self.bytes_written += json.len() as u64 + 1; // +1 for newline

        // Check after write — rotate if we crossed the threshold mid-session.
        if self.bytes_written >= MAX_LOG_BYTES {
            self.rotate();
        }
    }

    /// Rotate: audit-log.jsonl → .1.jsonl, .1 → .2, .2 → .3, delete .3.
    fn rotate(&mut self) {
        for i in (1..MAX_ROTATED_FILES).rev() {
            let from = self.path.with_extension(format!("{i}.jsonl"));
            let to = self.path.with_extension(format!("{}.jsonl", i + 1));
            if from.exists() {
                let _ = fs::rename(&from, &to);
            }
        }
        let archive_1 = self.path.with_extension("1.jsonl");
        if self.path.exists() {
            let _ = fs::rename(&self.path, &archive_1);
        }
        self.bytes_written = 0;
        tracing::debug!(
            rotated_to = %archive_1.display(),
            "audit log rotated (>{} MB)",
            MAX_LOG_BYTES / 1024 / 1024
        );
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn text_preview(result: &omegon_traits::ToolResult, max: usize) -> String {
        result
            .content
            .iter()
            .filter_map(|c| match c {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(max)
            .collect()
    }

    fn str_preview(s: &str, max: usize) -> &str {
        crate::util::truncate_str(s, max)
    }

    fn args_summary(args: &serde_json::Value) -> serde_json::Value {
        // Keep path, command, action — drop large content fields
        let mut summary = serde_json::Map::new();
        if let Some(obj) = args.as_object() {
            for (k, v) in obj {
                match k.as_str() {
                    "content" | "old_string" | "new_string" | "source" => {
                        // Truncate large string values
                        if let Some(s) = v.as_str() {
                            summary.insert(
                                k.clone(),
                                serde_json::Value::String(
                                    s.chars().take(80).collect::<String>()
                                        + if s.len() > 80 { "…" } else { "" },
                                ),
                            );
                        } else {
                            summary.insert(k.clone(), v.clone());
                        }
                    }
                    _ => {
                        summary.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        serde_json::Value::Object(summary)
    }

    fn structured_agent_event(
        event: &omegon_traits::AgentEvent,
    ) -> Option<(&'static str, serde_json::Value)> {
        match event {
            omegon_traits::AgentEvent::RuntimeQueueUpdated { snapshot_json } => Some((
                "runtime_queue",
                serde_json::json!({
                    "snapshot": snapshot_json,
                }),
            )),
            omegon_traits::AgentEvent::RuntimeTurnLifecycleUpdated { snapshot_json } => Some((
                "runtime_turn_lifecycle",
                serde_json::json!({
                    "snapshot": snapshot_json,
                }),
            )),
            omegon_traits::AgentEvent::RuntimePromptStarted { text, image_paths } => Some((
                "runtime_prompt_started",
                serde_json::json!({
                    "text_chars": text.chars().count(),
                    "attachments": image_paths.len(),
                    "preview": Self::str_preview(text, 120),
                }),
            )),
            omegon_traits::AgentEvent::ContextUpdated {
                tokens,
                context_window,
                context_class,
                thinking_level,
            } => Some((
                "context_updated",
                serde_json::json!({
                    "tokens": tokens,
                    "context_window": context_window,
                    "usage_percent": if *context_window == 0 { 0 } else { tokens.saturating_mul(100) / context_window },
                    "context_class": context_class,
                    "thinking_level": thinking_level,
                }),
            )),
            omegon_traits::AgentEvent::AgentEnd => Some(("agent_end", serde_json::json!({}))),
            omegon_traits::AgentEvent::StreamIdle {
                provider,
                model,
                phase,
                idle_secs,
                ambiguous,
                message,
            } => Some((
                "stream_idle",
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "phase": phase,
                    "idle_secs": idle_secs,
                    "ambiguous": ambiguous,
                    "message": message,
                }),
            )),
            omegon_traits::AgentEvent::ProviderRetry {
                provider,
                model,
                attempt,
                delay_ms,
                reason,
                message,
                recoverable,
            } => Some((
                "provider_retry",
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "attempt": attempt,
                    "delay_ms": delay_ms,
                    "reason": reason,
                    "message": message,
                    "recoverable": recoverable,
                }),
            )),
            omegon_traits::AgentEvent::ProviderFailure {
                provider,
                model,
                reason,
                attempts,
                message,
                retryable,
                recommended_action,
            } => Some((
                "provider_failure",
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "reason": reason,
                    "attempts": attempts,
                    "message": message,
                    "retryable": retryable,
                    "recommended_action": recommended_action,
                }),
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditEntry {
    ts: u64,
    session: String,
    kind: String,
    #[serde(flatten)]
    data: serde_json::Value,
}

#[async_trait]
impl Feature for AuditLog {
    fn name(&self) -> &str {
        "audit-log"
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        let ts = Self::now_ms();
        let session = self.session_id.clone();

        match event {
            BusEvent::SessionStart { session_id, cwd } => {
                self.session_id = session_id.clone();
                self.tool_starts.clear();
                self.tool_updates.clear();
                self.append(&AuditEntry {
                    ts,
                    session: session_id.clone(),
                    kind: "session_start".into(),
                    data: serde_json::json!({ "cwd": cwd.display().to_string() }),
                });
            }

            BusEvent::SessionEnd {
                turns,
                tool_calls,
                duration_secs,
                initial_prompt,
                outcome_summary,
            } => {
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "session_end".into(),
                    data: serde_json::json!({
                        "turns": turns,
                        "tool_calls": tool_calls,
                        "duration_secs": duration_secs,
                        "open_tools": self.tool_starts.len(),
                        "tools_with_updates": self.tool_updates.len(),
                        "initial_prompt": initial_prompt.as_deref().map(|s| Self::str_preview(s, 200)),
                        "outcome": outcome_summary.as_deref().map(|s| Self::str_preview(s, 200)),
                    }),
                });
            }

            BusEvent::TurnEnd(te) => {
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "turn".into(),
                    data: serde_json::json!({
                        "turn": te.turn,
                        "model": te.model,
                        "provider": te.provider,
                        "est_tokens": te.estimated_tokens,
                        "ctx_window": te.context_window,
                        "in": te.actual_input_tokens,
                        "out": te.actual_output_tokens,
                        "cache": te.cache_read_tokens,
                        "phase": te.dominant_phase.map(|p| format!("{p:?}")),
                        "drift": te.drift_kind.map(|d| format!("{d:?}")),
                        "progress": format!("{:?}", te.progress_signal),
                        "ctx": {
                            "sys": te.context_composition.system_tokens,
                            "tools": te.context_composition.tool_schema_tokens,
                            "conv": te.context_composition.conversation_tokens,
                            "mem": te.context_composition.memory_tokens,
                            "think": te.context_composition.thinking_tokens,
                            "free": te.context_composition.free_tokens,
                        },
                        "quota": te.provider_telemetry.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default()),
                    }),
                });
            }

            BusEvent::ToolStart { id, name, args, .. } => {
                self.tool_starts.insert(id.clone(), ts);
                self.tool_updates.remove(id);
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "tool_start".into(),
                    data: serde_json::json!({
                        "id": id,
                        "tool": name,
                        "args": Self::args_summary(args),
                    }),
                });
            }

            BusEvent::ToolEnd {
                id,
                name,
                result,
                is_error,
            } => {
                let duration_ms = self
                    .tool_starts
                    .remove(id)
                    .map(|started| ts.saturating_sub(started));
                let update_stats = self.tool_updates.remove(id).unwrap_or_default();
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "tool_end".into(),
                    data: serde_json::json!({
                        "id": id,
                        "tool": name,
                        "error": is_error,
                        "duration_ms": duration_ms,
                        "updates": update_stats.count,
                        "heartbeat_updates": update_stats.heartbeat_count,
                        "first_update_latency_ms": update_stats.first_update_ms.zip(duration_ms).map(|(first, _)| first),
                        "last_update_age_ms": update_stats.last_update_ms.map(|last| ts.saturating_sub(last)),
                        "max_tail_chars": update_stats.max_tail_chars,
                        "preview": Self::text_preview(result, 200),
                        "details": result.details,
                    }),
                });
            }

            BusEvent::PermissionDecision {
                tool_name,
                path,
                decision,
                kind,
                persistence,
                grant_path,
            } => {
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "permission".into(),
                    data: serde_json::json!({
                        "tool": tool_name,
                        "path": path,
                        "decision": decision,
                        "kind": format!("{kind:?}"),
                        "persistence": format!("{persistence:?}"),
                        "grant_path": grant_path,
                    }),
                });
            }

            BusEvent::NudgeInjected {
                turn,
                reason,
                message_preview,
            } => {
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "nudge".into(),
                    data: serde_json::json!({
                        "turn": turn,
                        "reason": reason,
                        "message": message_preview,
                    }),
                });
            }

            BusEvent::Compacted => {
                self.append(&AuditEntry {
                    ts,
                    session,
                    kind: "compacted".into(),
                    data: serde_json::json!({}),
                });
            }

            BusEvent::AgentEventEmitted { event } => {
                let event_kind = agent_event_kind(event);
                if let omegon_traits::AgentEvent::ToolUpdate { id, partial } = event.as_ref() {
                    let stats = self.tool_updates.entry(id.clone()).or_default();
                    stats.count = stats.count.saturating_add(1);
                    if partial.progress.heartbeat {
                        stats.heartbeat_count = stats.heartbeat_count.saturating_add(1);
                    }
                    if stats.first_update_ms.is_none() {
                        stats.first_update_ms = self
                            .tool_starts
                            .get(id)
                            .map(|started| ts.saturating_sub(*started));
                    }
                    stats.last_update_ms = Some(ts);
                    stats.max_tail_chars = stats.max_tail_chars.max(partial.tail.chars().count());
                }
                self.append(&AuditEntry {
                    ts,
                    session: session.clone(),
                    kind: "agent_event".into(),
                    data: serde_json::json!({
                        "event_kind": event_kind,
                        "event_debug": format!("{event:?}"),
                    }),
                });
                if let Some((kind, data)) = Self::structured_agent_event(event) {
                    self.append(&AuditEntry {
                        ts,
                        session: session.clone(),
                        kind: kind.into(),
                        data,
                    });
                }
                if let omegon_traits::AgentEvent::SkillActivation { event } = event.as_ref() {
                    self.append(&AuditEntry {
                        ts,
                        session,
                        kind: "skill_activation".into(),
                        data: serde_json::json!(event),
                    });
                }
            }

            _ => {}
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrored_agent_event_writes_generic_audit_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let mut audit = AuditLog::new(tmp.path(), "session-1");
        audit.path = tmp.path().join("audit-log.jsonl");

        audit.on_event(&omegon_traits::BusEvent::AgentEventEmitted {
            event: Box::new(omegon_traits::AgentEvent::SystemNotification {
                message: "hello".into(),
            }),
        });

        let content = std::fs::read_to_string(&audit.path).unwrap();
        let entries: Vec<serde_json::Value> = content
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["kind"], "agent_event");
        assert_eq!(entries[0]["event_kind"], "system_notification");
        assert!(
            entries[0]["event_debug"]
                .as_str()
                .unwrap()
                .contains("hello")
        );
    }

    #[test]
    fn mirrored_skill_activation_writes_structured_audit_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let mut audit = AuditLog::new(tmp.path(), "session-1");
        audit.path = tmp.path().join("audit-log.jsonl");
        let activation = omegon_traits::SkillActivationEvent {
            active_ref: "extension:recro/recro-rust-dev".into(),
            activation: Some("project_detected".into()),
            reason: "startup".into(),
            matched_signals: vec!["Cargo.toml".into()],
            suppressing: vec!["bundled/rust".into()],
            resolution: "merge_recommended".into(),
            recommendation: Some("Create a project-local merged skill override.".into()),
            injected: true,
        };

        audit.on_event(&omegon_traits::BusEvent::AgentEventEmitted {
            event: Box::new(omegon_traits::AgentEvent::SkillActivation { event: activation }),
        });

        let content = std::fs::read_to_string(&audit.path).unwrap();
        let entries: Vec<serde_json::Value> = content
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["kind"], "agent_event");
        assert_eq!(entries[0]["event_kind"], "skill_activation");
        assert_eq!(entries[1]["kind"], "skill_activation");
        assert_eq!(entries[1]["active_ref"], "extension:recro/recro-rust-dev");
        assert_eq!(entries[1]["suppressing"][0], "bundled/rust");
    }

    #[test]
    fn str_preview_handles_emoji_at_limit() {
        let prefix = "a".repeat(199);
        let text = format!("{prefix}✓ trailing text");

        let preview = AuditLog::str_preview(&text, 200);

        assert!(preview.is_char_boundary(preview.len()));
        assert!(preview.len() <= text.len());
    }

    #[test]
    fn str_preview_matches_real_audit_crash_case() {
        let text = "Jellyfin is now scheduled and pulling its image. Here's the current status:\n\n\
| Service | Status | Notes |\n\
|---|---|---|\n\
| **Sonarr** | ✓ Running | |\n\
| **Radarr** | ✓ Running | |\n\
| **Prowlarr** | ✓ Running | |\n\
| **Jellyseerr** | ✓ Running | |\n\
| **Jellyfin** | ✓ Pulling | |";

        let preview = AuditLog::str_preview(text, 200);

        assert!(preview.is_char_boundary(preview.len()));
    }

    #[test]
    fn str_preview_matches_pipewire_recovery_crash_case() {
        let text = "I likely wedged PipeWire / the session shell by touching the live routing stack again. I should not have run another pipeWire link probe after we already knew this machine can hang on that path. That's on me.\n\nDo this recovery first - **don't trouble";

        let preview = AuditLog::str_preview(text, 200);

        assert!(preview.is_char_boundary(preview.len()));
        assert!(preview.len() <= text.len());
    }
}
