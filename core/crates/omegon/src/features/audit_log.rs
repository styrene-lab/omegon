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
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

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
        omegon_traits::AgentEvent::BackgroundOperationCompleted { .. } => {
            "background_operation_completed"
        }
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
        omegon_traits::AgentEvent::CommandSurface { .. } => "command_surface",
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
    session_binding: Option<crate::session_consumers::DeferredSessionViewBinding>,
    cursor_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditConsumerCursorV2 {
    cursor_version: u16,
    consumer_id: String,
    semantic_row_schema_version: u16,
    session_id: String,
    stream_id: Uuid,
    host_generation: u64,
    last_sequence: u64,
    last_event_id: Uuid,
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
            cursor_path: dir.join("audit-consumer-cursor-v2.json"),
            session_id: session_id.to_string(),
            bytes_written: 0,
            size_checked: false,
            tool_starts: HashMap::new(),
            tool_updates: HashMap::new(),
            session_binding: None,
        }
    }

    pub(crate) fn with_session_binding(
        mut self,
        binding: crate::session_consumers::DeferredSessionViewBinding,
    ) -> Self {
        self.session_binding = Some(binding);
        self
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

    fn consume_semantic(&mut self) {
        if let Err(error) = self.try_consume_semantic() {
            tracing::warn!(error = %error, "best-effort semantic runtime audit update failed");
        }
    }

    fn try_consume_semantic(&mut self) -> Result<(), String> {
        let binding = self
            .session_binding
            .as_ref()
            .ok_or_else(|| "sessionless runtime audit has no semantic binding".to_string())?;
        let (target, replay) = crate::session_advisory::load(binding)?;
        let prior = read_audit_cursor(&self.cursor_path)?;
        let start_sequence = match prior.as_ref() {
            Some(cursor)
                if cursor.session_id == target.session_id
                    && cursor.stream_id == replay.frontier().stream_id() =>
            {
                validate_audit_cursor(cursor, &replay)?;
                cursor.last_sequence.saturating_add(1)
            }
            _ => 1,
        };
        let minimum = replay
            .first_full_spine_boundary()
            .map_or(1, |frontier| frontier.sequence());
        let existing = semantic_source_keys(&self.path)?;
        let mut rows = Vec::new();
        let route_by_request = replay
            .records()
            .iter()
            .filter_map(|record| match record.payload() {
                crate::session_authority::SessionFactPayload::RouteLeaseRecorded(route) => Some((
                    route.request_id,
                    (
                        route.serving_provider_id.clone(),
                        route.serving_model_id.clone(),
                    ),
                )),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        let tool_by_call = replay
            .records()
            .iter()
            .filter_map(|record| match record.payload() {
                crate::session_authority::SessionFactPayload::ToolCallRecorded(call) => {
                    Some((call.tool_call_id, call.invocation_name.clone()))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        for record in replay.records().iter().filter(|record| {
            record.frontier().sequence() >= start_sequence
                && record.frontier().sequence() >= minimum
        }) {
            let (kind, data) = match record.payload() {
                crate::session_authority::SessionFactPayload::SessionCreated(_) => {
                    ("semantic_session_committed", serde_json::json!({}))
                }
                crate::session_authority::SessionFactPayload::TurnClosed(turn) => (
                    "semantic_turn_terminal",
                    serde_json::json!({
                        "turn_id": turn.turn_id,
                        "outcome": format!("{:?}", turn.outcome).to_lowercase(),
                        "reason_code": turn.reason_code,
                    }),
                ),
                crate::session_authority::SessionFactPayload::AssistantMessageCommitted(
                    message,
                ) => {
                    let route = route_by_request.get(&message.request_id);
                    (
                        "semantic_message_committed",
                        serde_json::json!({
                            "message_id": message.message_id,
                            "request_id": message.request_id,
                            "provider": route.map(|value| value.0.as_str()),
                            "model": route.map(|value| value.1.as_str()),
                            "usage": message.usage,
                            "tool_call_count": message.tool_call_count,
                        }),
                    )
                }
                crate::session_authority::SessionFactPayload::ToolResultRecorded(result) => (
                    "semantic_tool_terminal",
                    serde_json::json!({
                        "tool_result_id": result.tool_result_id,
                        "tool_call_id": result.tool_call_id,
                        "tool": tool_by_call.get(&result.tool_call_id),
                        "disposition": format!("{:?}", result.disposition).to_lowercase(),
                        "is_error": result.is_error,
                        "reason_code": result.reason_code,
                    }),
                ),
                _ => continue,
            };
            let key = semantic_key(
                replay.frontier().stream_id(),
                record.frontier().event_id(),
                kind,
            );
            if existing.contains(&key) {
                continue;
            }
            rows.push(AuditEntry {
                ts: Self::now_ms(),
                session: target.session_id.clone(),
                kind: kind.into(),
                data: serde_json::json!({
                    "semantic_row_schema_version": 1,
                    "authority_role": "best_effort_diagnostic_not_authority",
                    "source": {
                        "stream_id": replay.frontier().stream_id(),
                        "sequence": record.frontier().sequence(),
                        "event_id": record.frontier().event_id(),
                        "event_kind": record.event_type(),
                    },
                    "recorded_at": record.recorded_at(),
                    "data": data,
                }),
            });
        }
        if !crate::session_advisory::generation_is_current(binding, &target) {
            return Ok(());
        }
        append_audit_rows(&self.path, &rows)?;
        self.bytes_written = fs::metadata(&self.path).map_or(0, |metadata| metadata.len());
        self.size_checked = true;
        if self.bytes_written >= MAX_LOG_BYTES {
            self.rotate();
        }
        let cursor = AuditConsumerCursorV2 {
            cursor_version: 2,
            consumer_id: "runtime-audit-semantic".into(),
            semantic_row_schema_version: 1,
            session_id: target.session_id,
            stream_id: replay.frontier().stream_id(),
            host_generation: target.generation,
            last_sequence: replay.frontier().sequence(),
            last_event_id: replay.frontier().event_id(),
        };
        write_audit_cursor(&self.cursor_path, &cursor)
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
            omegon_traits::AgentEvent::RuntimePromptStarted {
                runtime_turn_id,
                text,
                image_paths,
            } => Some((
                "runtime_prompt_started",
                serde_json::json!({
                    "runtime_turn_id": runtime_turn_id,
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

fn semantic_key(stream_id: Uuid, event_id: Uuid, kind: &str) -> String {
    format!("{stream_id}:{event_id}:{kind}")
}

fn semantic_source_keys(path: &Path) -> Result<HashSet<String>, String> {
    let mut keys = HashSet::new();
    for candidate in [
        path.to_path_buf(),
        path.with_extension("1.jsonl"),
        path.with_extension("2.jsonl"),
        path.with_extension("3.jsonl"),
    ] {
        let Ok(metadata) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_LOG_BYTES.saturating_mul(2) {
            return Err(format!(
                "audit dedup source is unsafe: {}",
                candidate.display()
            ));
        }
        let mut text = String::new();
        std::fs::File::open(&candidate)
            .map_err(|error| error.to_string())?
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        for line in text.lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let semantic = value
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| kind.starts_with("semantic_"));
            let Some(source) = value.get("source") else {
                if semantic {
                    return Err("existing semantic audit row has no source identity".into());
                }
                continue;
            };
            let (Some(stream), Some(event), Some(kind)) = (
                source.get("stream_id").and_then(serde_json::Value::as_str),
                source.get("event_id").and_then(serde_json::Value::as_str),
                value.get("kind").and_then(serde_json::Value::as_str),
            ) else {
                if semantic {
                    return Err("existing semantic audit row has malformed source identity".into());
                }
                continue;
            };
            if Uuid::parse_str(stream).is_err() || Uuid::parse_str(event).is_err() {
                if semantic {
                    return Err("existing semantic audit row has invalid source identity".into());
                }
                continue;
            }
            keys.insert(format!("{stream}:{event}:{kind}"));
        }
    }
    Ok(keys)
}

#[cfg(test)]
pub(crate) fn recovery_campaign_probe(root: &Path, scenario_id: &str) -> Result<(), String> {
    let path = root.join(format!("{scenario_id}.audit.jsonl"));
    match scenario_id {
        "AC38" => {
            let original = b"{\"kind\":\"agent_event\",\"event_kind\":\"runtime\"}\n{\"kind\":\"semantic_turn_terminal\",\"source\":{}}\n";
            fs::write(&path, original).map_err(|error| error.to_string())?;
            if semantic_source_keys(&path).is_ok()
                || fs::read(&path).ok().as_deref() != Some(original)
            {
                return Err("malformed semantic audit row did not fail closed".into());
            }
        }
        "AC40" => {
            let row = "{\"kind\":\"semantic_turn_terminal\",\"source\":{\"stream_id\":\"10000000-0000-4000-8000-000000000001\",\"event_id\":\"20000000-0000-4000-8000-000000000004\"}}\n";
            fs::write(&path, format!("{row}{row}")).map_err(|error| error.to_string())?;
            if semantic_source_keys(&path)?.len() != 1 {
                return Err("semantic audit delivery was not deduplicated by source".into());
            }
        }
        "AC43" => {
            let row = b"{not-json\n{\"kind\":\"agent_event\",\"source\":{}}\n";
            fs::write(&path, row).map_err(|error| error.to_string())?;
            if !semantic_source_keys(&path)?.is_empty()
                || fs::read(&path).ok().as_deref() != Some(row)
            {
                return Err("nonsemantic audit damage affected semantic advancement".into());
            }
        }
        _ => return Err(format!("unsupported audit campaign scenario {scenario_id}")),
    }
    Ok(())
}

fn read_audit_cursor(path: &Path) -> Result<Option<AuditConsumerCursorV2>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
        return Err("semantic audit cursor is not a bounded regular file".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let cursor = AuditConsumerCursorV2::deserialize(&mut deserializer)
        .map_err(|error| format!("semantic audit cursor is malformed: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("semantic audit cursor has trailing data: {error}"))?;
    if cursor.cursor_version != 2
        || cursor.consumer_id != "runtime-audit-semantic"
        || cursor.semantic_row_schema_version != 1
        || cursor.session_id.is_empty()
        || cursor.stream_id.is_nil()
        || cursor.last_sequence == 0
        || cursor.last_event_id.is_nil()
    {
        return Err("semantic audit cursor has invalid required fields".into());
    }
    Ok(Some(cursor))
}

fn validate_audit_cursor(
    cursor: &AuditConsumerCursorV2,
    replay: &crate::session_replay::SessionReplay,
) -> Result<(), String> {
    if cursor.last_sequence > replay.frontier().sequence() {
        return Err("semantic audit cursor is ahead of validated replay".into());
    }
    let event = replay
        .records()
        .get(cursor.last_sequence.saturating_sub(1) as usize)
        .map(|record| record.frontier().event_id());
    if event != Some(cursor.last_event_id) {
        return Err("semantic audit cursor event does not match validated replay".into());
    }
    Ok(())
}

fn append_audit_rows(path: &Path, rows: &[AuditEntry]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    audit_lock(&file).map_err(|error| error.to_string())?;
    let mut current = String::new();
    file.read_to_string(&mut current)
        .map_err(|error| error.to_string())?;
    let current_keys = current
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| {
            Some(format!(
                "{}:{}:{}",
                value.get("source")?.get("stream_id")?.as_str()?,
                value.get("source")?.get("event_id")?.as_str()?,
                value.get("kind")?.as_str()?
            ))
        })
        .collect::<HashSet<_>>();
    for row in rows {
        let Some(source) = row.data.get("source") else {
            continue;
        };
        let key = format!(
            "{}:{}:{}",
            source["stream_id"].as_str().unwrap_or_default(),
            source["event_id"].as_str().unwrap_or_default(),
            row.kind
        );
        if current_keys.contains(&key) {
            continue;
        }
        serde_json::to_writer(&mut file, row).map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    file.flush().map_err(|error| error.to_string())?;
    file.sync_data().map_err(|error| error.to_string())?;
    audit_unlock(&file).map_err(|error| error.to_string())?;
    Ok(())
}

fn write_audit_cursor(path: &Path, cursor: &AuditConsumerCursorV2) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "semantic audit cursor has no parent".to_string())?;
    let temporary = parent.join(format!(".audit-cursor-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut file, cursor).map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        sync_audit_parent(parent).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(unix)]
fn audit_lock(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn audit_lock(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn audit_unlock(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn audit_unlock(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_audit_parent(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_audit_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn semantic_backed_agent_event(event: &omegon_traits::AgentEvent) -> bool {
    matches!(
        event,
        omegon_traits::AgentEvent::TurnStart { .. }
            | omegon_traits::AgentEvent::MessageStart { .. }
            | omegon_traits::AgentEvent::MessageChunk { .. }
            | omegon_traits::AgentEvent::ThinkingChunk { .. }
            | omegon_traits::AgentEvent::MessageEnd
            | omegon_traits::AgentEvent::MessageAbort { .. }
            | omegon_traits::AgentEvent::ToolEnd { .. }
            | omegon_traits::AgentEvent::TurnEnd(_)
            | omegon_traits::AgentEvent::AgentEnd
    )
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
                let _ = cwd;
                self.consume_semantic();
            }

            BusEvent::SessionEnd { .. } => {
                self.consume_semantic();
            }

            BusEvent::TurnEnd(_) => {
                self.consume_semantic();
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
                let _ = (name, result, is_error, duration_ms, update_stats);
                self.consume_semantic();
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
                if let omegon_traits::AgentEvent::ToolUpdate { id, partial, .. } = event.as_ref() {
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
                let tool_origin = match event.as_ref() {
                    omegon_traits::AgentEvent::ToolStart {
                        id,
                        execution_origin,
                        ..
                    }
                    | omegon_traits::AgentEvent::ToolUpdate {
                        id,
                        execution_origin,
                        ..
                    }
                    | omegon_traits::AgentEvent::ToolEnd {
                        id,
                        execution_origin,
                        ..
                    } => Some((id, execution_origin)),
                    _ => None,
                };
                if let Some((id, execution_origin)) = tool_origin {
                    self.append(&AuditEntry {
                        ts,
                        session: session.clone(),
                        kind: "tool_execution_origin".into(),
                        data: serde_json::json!({
                            "id": id,
                            "execution_origin": execution_origin,
                            "lifecycle": event_kind,
                        }),
                    });
                }
                if semantic_backed_agent_event(event) {
                    return vec![];
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

    fn semantic_audit() -> (
        tempfile::TempDir,
        AuditLog,
        crate::session_consumers::DeferredSessionViewBinding,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("fixture-session.json");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/session-semantic-v1/slice-1-closed.authority.jsonl"),
            directory.path().join("fixture-session.authority.jsonl"),
        )
        .unwrap();
        let live = crate::session_consumers::SessionViewBinding::new(
            snapshot.clone(),
            "fixture-session".into(),
        );
        live.replace(crate::session_consumers::SessionViewTarget {
            binding_id: uuid::Uuid::new_v4(),
            snapshot,
            session_id: "fixture-session".into(),
            stream_id: Some(Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap()),
            generation: 9,
            kind: crate::session_consumers::SessionViewKind::Resume,
        });
        let deferred = crate::session_consumers::DeferredSessionViewBinding::default();
        deferred.bind(live);
        let audit = AuditLog::new(directory.path(), "fixture-session")
            .with_session_binding(deferred.clone());
        (directory, audit, deferred)
    }

    fn entries(path: &Path) -> Vec<serde_json::Value> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

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
    fn semantic_audit_is_exactly_deduplicated_across_lag_and_restart() {
        let (directory, mut audit, deferred) = semantic_audit();
        audit.try_consume_semantic().unwrap();
        let first = entries(&audit.path);
        assert_eq!(
            first
                .iter()
                .filter(|entry| entry["kind"].as_str().unwrap().starts_with("semantic_"))
                .count(),
            2
        );
        assert!(audit.cursor_path.exists(), "output must precede cursor");

        let lagged = AuditConsumerCursorV2 {
            cursor_version: 2,
            consumer_id: "runtime-audit-semantic".into(),
            semantic_row_schema_version: 1,
            session_id: "fixture-session".into(),
            stream_id: Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap(),
            host_generation: 9,
            last_sequence: 1,
            last_event_id: Uuid::parse_str("20000000-0000-4000-8000-000000000001").unwrap(),
        };
        write_audit_cursor(&audit.cursor_path, &lagged).unwrap();
        let mut restarted =
            AuditLog::new(directory.path(), "fixture-session").with_session_binding(deferred);
        restarted.try_consume_semantic().unwrap();
        assert_eq!(entries(&restarted.path), first);
        assert_eq!(
            read_audit_cursor(&restarted.cursor_path)
                .unwrap()
                .unwrap()
                .last_sequence,
            4
        );
    }

    #[test]
    fn malformed_cursor_fails_safe_without_duplicate_rows() {
        let (_directory, mut audit, _) = semantic_audit();
        audit.try_consume_semantic().unwrap();
        let before = fs::read(&audit.path).unwrap();
        fs::write(&audit.cursor_path, b"{not-json").unwrap();

        assert!(audit.try_consume_semantic().is_err());
        assert_eq!(fs::read(&audit.path).unwrap(), before);
    }

    #[test]
    fn malformed_existing_semantic_row_stops_cursor_without_rewriting_evidence() {
        let (_directory, mut audit, _) = semantic_audit();
        audit.try_consume_semantic().unwrap();
        let cursor = fs::read(&audit.cursor_path).unwrap();
        let mut evidence = fs::read(&audit.path).unwrap();
        evidence.extend_from_slice(b"{\"kind\":\"semantic_turn_terminal\",\"source\":{}}\n");
        fs::write(&audit.path, &evidence).unwrap();

        assert!(audit.try_consume_semantic().is_err());
        assert_eq!(fs::read(&audit.path).unwrap(), evidence);
        assert_eq!(fs::read(&audit.cursor_path).unwrap(), cursor);
    }

    #[test]
    fn malformed_nonsemantic_row_is_preserved_without_blocking_semantic_dedup() {
        let (_directory, mut audit, _) = semantic_audit();
        fs::write(&audit.path, b"{not-json\n").unwrap();
        audit.try_consume_semantic().unwrap();
        let first = fs::read(&audit.path).unwrap();
        audit.try_consume_semantic().unwrap();
        assert_eq!(fs::read(&audit.path).unwrap(), first);
        assert!(first.starts_with(b"{not-json\n"));
    }

    #[test]
    fn dynamic_replacement_rebinds_audit_identity_and_fences_the_old_cursor() {
        let (directory, mut audit, deferred) = semantic_audit();
        audit.try_consume_semantic().unwrap();
        let fixture = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/session-semantic-v1/slice-1-closed.authority.jsonl"),
        )
        .unwrap()
        .replace("fixture-session", "replacement-session")
        .replace(
            "10000000-0000-4000-8000-000000000001",
            "10000000-0000-4000-8000-000000000099",
        );
        let snapshot = directory.path().join("replacement-session.json");
        fs::write(
            directory.path().join("replacement-session.authority.jsonl"),
            fixture,
        )
        .unwrap();
        let replacement = crate::session_consumers::SessionViewBinding::new(
            snapshot.clone(),
            "replacement-session".into(),
        );
        replacement.replace(crate::session_consumers::SessionViewTarget {
            binding_id: uuid::Uuid::new_v4(),
            snapshot,
            session_id: "replacement-session".into(),
            stream_id: Some(Uuid::parse_str("10000000-0000-4000-8000-000000000099").unwrap()),
            generation: 10,
            kind: crate::session_consumers::SessionViewKind::ContextClear,
        });
        deferred.bind(replacement);

        audit.try_consume_semantic().unwrap();
        let cursor = read_audit_cursor(&audit.cursor_path).unwrap().unwrap();
        assert_eq!(cursor.session_id, "replacement-session");
        assert_eq!(cursor.host_generation, 10);
        assert_eq!(
            entries(&audit.path)
                .iter()
                .filter(|entry| entry["kind"] == "semantic_turn_terminal")
                .count(),
            2
        );
    }

    #[test]
    fn semantic_and_nonsemantic_audit_streams_do_not_duplicate_terminals() {
        let (_directory, mut audit, _) = semantic_audit();
        audit.try_consume_semantic().unwrap();
        audit.on_event(&BusEvent::AgentEventEmitted {
            event: Box::new(omegon_traits::AgentEvent::AgentEnd),
        });
        audit.on_event(&BusEvent::AgentEventEmitted {
            event: Box::new(omegon_traits::AgentEvent::SystemNotification {
                message: "operator-visible policy observation".into(),
            }),
        });

        let rows = entries(&audit.path);
        assert!(
            !rows.iter().any(|entry| {
                entry["kind"] == "agent_event" && entry["event_kind"] == "agent_end"
            })
        );
        assert!(rows.iter().any(|entry| {
            entry["kind"] == "agent_event" && entry["event_kind"] == "system_notification"
        }));
        let text = fs::read_to_string(&audit.path).unwrap();
        assert!(!text.contains("content_ref"));
        assert!(!text.contains("restricted_continuity"));
    }

    #[test]
    fn sessionless_semantic_audit_is_a_noop() {
        let directory = tempfile::tempdir().unwrap();
        let mut audit = AuditLog::new(directory.path(), "sessionless");
        assert!(audit.try_consume_semantic().is_err());
        assert!(!audit.path.exists());
        assert!(!audit.cursor_path.exists());
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
