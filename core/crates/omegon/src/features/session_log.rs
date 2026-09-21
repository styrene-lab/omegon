//! Session log — append-only session tracking with context injection.
//!
//! On session start, reads the last 3 narrative entries from `.omegon/agent-journal.md`
//! and injects them as context. Raw file-op lines are stripped.
//!
//! On session end, auto-appends a structured entry:
//!   - date, git branch, recent commits
//!   - session stats (turns, tool calls, duration)
//!   - active OpenSpec changes

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContentBlock, ContextComposition,
    ContextInjection, ContextSignals, Feature, ToolDefinition, ToolResult,
};

pub struct SessionLog {
    log_path: PathBuf,
    cwd: PathBuf,
    /// Cached narrative context, injected once on SessionStart.
    context_snippet: Option<String>,
    /// Per-turn provider/model/telemetry snapshots collected during the live session.
    turn_summaries: Vec<TurnSummary>,
    session_binding: Option<crate::session_consumers::DeferredSessionViewBinding>,
    event_session_id: Option<String>,
    lifecycle: Option<crate::runtime_state::LifecycleHostHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalProvenanceV1 {
    schema_version: u16,
    authority_role: String,
    session_id: Option<String>,
    stream_id: Option<Uuid>,
    host_generation: Option<u64>,
    turn_ids: Vec<Uuid>,
    source_frontier: Option<JournalFrontierV1>,
    semantic_turn_count: u32,
    semantic_tool_terminal_count: u32,
    sessionless: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalFrontierV1 {
    sequence: u64,
    event_id: Uuid,
}

struct JournalNarrativeV1 {
    provenance: JournalProvenanceV1,
    initial_prompt: Option<String>,
    outcome: Option<String>,
    routes: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct TurnSummary {
    session_generation: Option<(String, u64)>,
    turn: u32,
    model: Option<String>,
    provider: Option<String>,
    estimated_tokens: usize,
    context_window: usize,
    context_composition: ContextComposition,
    actual_input_tokens: u64,
    actual_output_tokens: u64,
    cache_read_tokens: u64,
    provider_telemetry: Option<omegon_traits::ProviderTelemetrySnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UsageStats {
    sessions: usize,
    turns: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_cache_read_tokens: u64,
    max_input_tokens: u64,
    max_output_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UsageBucket {
    turns: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_cache_read_tokens: u64,
    total_ctx_est_tokens: u64,
    total_ctx_window_tokens: u64,
    total_system_tokens: u64,
    total_tool_schema_tokens: u64,
    total_conversation_tokens: u64,
    total_memory_tokens: u64,
    total_tool_history_tokens: u64,
    total_thinking_tokens: u64,
    total_free_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTurnUsage<'a> {
    provider: &'a str,
    model: &'a str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    context: Option<ParsedContextComposition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedContextComposition {
    est_tokens: u64,
    window_tokens: u64,
    system_tokens: u64,
    tool_schema_tokens: u64,
    conversation_tokens: u64,
    memory_tokens: u64,
    tool_history_tokens: u64,
    thinking_tokens: u64,
    free_tokens: u64,
    base_prompt_tokens: u64,
    session_hud_tokens: u64,
    intent_tokens: u64,
    external_injection_tokens: u64,
    tool_guidance_tokens: u64,
    file_guidance_tokens: u64,
}

impl SessionLog {
    pub fn new(cwd: &Path) -> Self {
        let omegon_dir = cwd.join(".omegon");
        let _ = fs::create_dir_all(&omegon_dir);
        Self {
            log_path: omegon_dir.join("agent-journal.md"),
            cwd: cwd.to_path_buf(),
            context_snippet: None,
            turn_summaries: Vec::new(),
            session_binding: None,
            event_session_id: None,
            lifecycle: None,
        }
    }

    pub(crate) fn with_session_binding(
        mut self,
        binding: crate::session_consumers::DeferredSessionViewBinding,
    ) -> Self {
        self.session_binding = Some(binding);
        self
    }

    pub(crate) fn with_lifecycle(
        mut self,
        lifecycle: crate::runtime_state::LifecycleHostHandle,
    ) -> Self {
        self.lifecycle = Some(lifecycle);
        self
    }

    /// Read the last `n` narrative entries (## YYYY-MM-DD headings).
    /// Strips lines that look like raw file-op dumps (long comma-separated paths).
    fn read_narrative_entries(&self, n: usize) -> Option<String> {
        let content = fs::read_to_string(&self.log_path).ok()?;
        if content.trim().is_empty() {
            return None;
        }

        // Split on ## date headings
        let parts: Vec<&str> = content
            .split("\n## ")
            .filter(|s| {
                // Keep entries that start with a date pattern YYYY-MM-DD or ISO timestamp
                let first = s.trim_start_matches('#').trim();
                first.starts_with("20") // any 2000s date
            })
            .collect();

        if parts.is_empty() {
            return None;
        }

        let recent: Vec<String> = parts
            .iter()
            .rev()
            .take(n)
            .rev()
            .map(|entry| {
                // Strip file-op dump lines: lines starting with "Session:", "Written:", "Edited:"
                // and lines that are a wall of comma-separated paths
                let cleaned: Vec<&str> = entry
                    .lines()
                    .filter(|line| {
                        let t = line.trim();
                        if t.starts_with("Session:")
                            || t.starts_with("Written:")
                            || t.starts_with("Edited:")
                        {
                            return false;
                        }
                        // Heuristic: a line with >5 commas and path separators is a file dump
                        let comma_count = t.chars().filter(|&c| c == ',').count();
                        let slash_count = t.chars().filter(|&c| c == '/').count();
                        !(comma_count > 5 && slash_count > 3)
                    })
                    .collect();
                format!("## {}", cleaned.join("\n")).trim().to_string()
            })
            .filter(|e| e.len() > 10) // skip empty entries
            .collect();

        if recent.is_empty() {
            return None;
        }

        Some(recent.join("\n\n"))
    }

    /// Run a git command in cwd, return stdout trimmed.
    fn git(&self, args: &[&str]) -> Option<String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.cwd)
            .output()
            .ok()?;
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        } else {
            None
        }
    }

    /// Collect active OpenSpec changes (any with incomplete tasks).
    fn active_openspec(&self) -> Vec<String> {
        let mut active = self
            .lifecycle
            .as_ref()
            .and_then(|lifecycle| lifecycle.observe().ok()?.repository)
            .map(|repository| {
                repository
                    .lifecycle
                    .openspec
                    .changes
                    .iter()
                    .filter(|change| {
                        change.total_tasks > 0 && change.done_tasks < change.total_tasks
                    })
                    .map(|change| {
                        format!(
                            "{} ({}/{})",
                            change.name, change.done_tasks, change.total_tasks
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        active.sort();
        active
    }

    /// Current date as YYYY-MM-DD.
    fn today() -> String {
        // Use system time without chrono dependency
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs();
        let days = secs / 86400;
        let (y, m, d) = crate::session::days_to_ymd(days);
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// Append a structured entry to `.omegon/agent-journal.md`.
    fn append_entry(
        &self,
        duration_secs: f64,
        narrative: JournalNarrativeV1,
    ) -> std::io::Result<bool> {
        if !narrative.provenance.sessionless
            && !self.session_binding.as_ref().is_some_and(|binding| {
                binding.snapshot().is_some_and(|target| {
                    Some(target.session_id) == narrative.provenance.session_id
                        && target.stream_id == narrative.provenance.stream_id
                        && Some(target.generation) == narrative.provenance.host_generation
                })
            })
        {
            return Ok(false);
        }
        let date = Self::today();
        let branch = self
            .git(&["branch", "--show-current"])
            .unwrap_or_else(|| "main".to_string());
        let commits = self
            .git(&["log", "--oneline", "-3", "--no-decorate"])
            .unwrap_or_default();
        let openspec = self.active_openspec();

        let duration = if duration_secs >= 3600.0 {
            format!(
                "{:.0}h{:.0}m",
                duration_secs / 3600.0,
                (duration_secs % 3600.0) / 60.0
            )
        } else if duration_secs >= 60.0 {
            format!("{:.0}m{:.0}s", duration_secs / 60.0, duration_secs % 60.0)
        } else {
            format!("{:.0}s", duration_secs)
        };

        let mut lines = vec![
            format!(
                "## {} — {} ({}t {}tc {})",
                date,
                branch,
                narrative.provenance.semantic_turn_count,
                narrative.provenance.semantic_tool_terminal_count,
                duration
            ),
            String::new(),
        ];

        let provenance =
            serde_json::to_string(&narrative.provenance).map_err(std::io::Error::other)?;
        lines.push(format!(
            "<!-- omegon-journal-provenance-v1 {provenance} -->"
        ));
        lines.push(String::new());

        if narrative.provenance.sessionless {
            lines.push("**Semantic source:** sessionless; authority replay unavailable".into());
            lines.push(String::new());
        } else if let Some(frontier) = &narrative.provenance.source_frontier {
            lines.push(format!(
                "**Semantic source:** validated replay through {} / {}",
                frontier.sequence, frontier.event_id
            ));
            lines.push(String::new());
        } else {
            lines.push("**Semantic source:** semantic_source_unavailable".into());
            lines.push(String::new());
        }

        if let Some(prompt) = narrative.initial_prompt.as_deref() {
            lines.push(format!("**Task:** {prompt}"));
            lines.push(String::new());
        }

        if let Some(outcome) = narrative.outcome.as_deref() {
            lines.push(format!("**Outcome:** {outcome}"));
            lines.push(String::new());
        }

        if !narrative.routes.is_empty() {
            lines.push("**Committed routes:**".into());
            for (provider, model) in narrative.routes {
                lines.push(format!("- {provider} / {model}"));
            }
            lines.push(String::new());
        }

        let turn_summaries = self
            .turn_summaries
            .iter()
            .filter(|summary| {
                narrative.provenance.sessionless
                    || summary.session_generation.as_ref()
                        == narrative
                            .provenance
                            .session_id
                            .as_ref()
                            .zip(narrative.provenance.host_generation.as_ref())
                            .map(|(session, generation)| (session.clone(), *generation))
                            .as_ref()
            })
            .collect::<Vec<_>>();
        if !turn_summaries.is_empty() {
            // Compact summary: just model and total tokens, not full per-turn telemetry.
            let model = turn_summaries
                .last()
                .and_then(|s| s.model.as_deref())
                .unwrap_or("unknown");
            let total_in: u64 = turn_summaries.iter().map(|s| s.actual_input_tokens).sum();
            let total_out: u64 = turn_summaries.iter().map(|s| s.actual_output_tokens).sum();
            lines.push(format!(
                "**Observed usage:** {model} — {total_in} in / {total_out} out tokens across {} turns",
                turn_summaries.len()
            ));
            lines.push(String::new());
        }

        if !openspec.is_empty() || !commits.is_empty() {
            lines.push("**Workspace observations (non-authoritative):**".to_string());
            lines.push(String::new());
        }

        if !openspec.is_empty() {
            lines.push("**Active:**".to_string());
            for s in &openspec {
                lines.push(format!("- {s}"));
            }
            lines.push(String::new());
        }

        if !commits.is_empty() {
            lines.push("**Commits:**".to_string());
            for line in commits.lines() {
                lines.push(format!("  {line}"));
            }
            lines.push(String::new());
        }

        let entry = lines.join("\n");

        // Bootstrap header if file doesn't exist
        let header = "# Agent Journal\n\nAppend-only record of agent sessions. Read recent entries for context.\n\n";
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.log_path)?;
        lock_file(&file)?;
        let mut existing = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut existing)?;
        let mut duplicate = false;
        for line in existing.lines() {
            let Some(value) = line.strip_prefix("<!-- omegon-journal-provenance-v1 ") else {
                continue;
            };
            let value = value.strip_suffix(" -->").ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "journal contains partial semantic provenance",
                )
            })?;
            let prior = serde_json::from_str::<JournalProvenanceV1>(value).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "journal contains malformed semantic provenance",
                )
            })?;
            duplicate |= same_journal_source(&prior, &narrative.provenance);
        }
        if duplicate {
            unlock_file(&file)?;
            return Ok(false);
        }
        let bytes = if existing.is_empty() {
            format!("{header}{entry}\n")
        } else {
            format!("\n{entry}\n")
        };
        file.write_all(bytes.as_bytes())?;
        file.flush()?;
        file.sync_data()?;
        unlock_file(&file)?;
        sync_parent(&self.log_path)?;
        Ok(true)
    }

    fn narrative(&self, advisory_turns: u32) -> JournalNarrativeV1 {
        let sessionless = || JournalNarrativeV1 {
            provenance: JournalProvenanceV1 {
                schema_version: 1,
                authority_role: "workspace_narrative_not_authority".into(),
                session_id: self.event_session_id.clone(),
                stream_id: None,
                host_generation: None,
                turn_ids: Vec::new(),
                source_frontier: None,
                semantic_turn_count: advisory_turns,
                semantic_tool_terminal_count: 0,
                sessionless: true,
            },
            initial_prompt: None,
            outcome: None,
            routes: Vec::new(),
        };
        let Some(binding) = &self.session_binding else {
            return sessionless();
        };
        let target = match binding.snapshot() {
            Some(target) => target,
            None => return sessionless(),
        };
        let replay = match crate::session_advisory::load(binding) {
            Ok((_, replay)) => replay,
            Err(_) => {
                return JournalNarrativeV1 {
                    provenance: JournalProvenanceV1 {
                        schema_version: 1,
                        authority_role: "semantic_source_unavailable".into(),
                        session_id: Some(target.session_id),
                        stream_id: target.stream_id,
                        host_generation: Some(target.generation),
                        turn_ids: Vec::new(),
                        source_frontier: None,
                        semantic_turn_count: 0,
                        semantic_tool_terminal_count: 0,
                        sessionless: false,
                    },
                    initial_prompt: None,
                    outcome: None,
                    routes: Vec::new(),
                };
            }
        };
        let minimum = replay
            .first_full_spine_boundary()
            .map_or(1, |frontier| frontier.sequence());
        let records = replay
            .records()
            .iter()
            .filter(|record| record.frontier().sequence() >= minimum)
            .collect::<Vec<_>>();
        let turn_ids = records
            .iter()
            .filter_map(|record| match record.payload() {
                crate::session_authority::SessionFactPayload::TurnClosed(value) => {
                    Some(value.turn_id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_count = records
            .iter()
            .filter(|record| {
                matches!(
                    record.payload(),
                    crate::session_authority::SessionFactPayload::ToolResultRecorded(_)
                )
            })
            .count() as u32;
        let initial_prompt = records.iter().find_map(|record| match record.payload() {
            crate::session_authority::SessionFactPayload::PromptAdmitted(value) => {
                Some(value.content.text.clone())
            }
            _ => None,
        });
        let routes = records
            .iter()
            .filter_map(|record| match record.payload() {
                crate::session_authority::SessionFactPayload::RouteLeaseRecorded(value) => Some((
                    value.serving_provider_id.clone(),
                    value.serving_model_id.clone(),
                )),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !crate::session_advisory::generation_is_current(binding, &target) {
            return sessionless();
        }
        JournalNarrativeV1 {
            provenance: JournalProvenanceV1 {
                schema_version: 1,
                authority_role: "workspace_narrative_not_authority".into(),
                session_id: Some(target.session_id),
                stream_id: Some(replay.frontier().stream_id()),
                host_generation: Some(target.generation),
                turn_ids,
                source_frontier: Some(JournalFrontierV1 {
                    sequence: replay.frontier().sequence(),
                    event_id: replay.frontier().event_id(),
                }),
                semantic_turn_count: records
                    .iter()
                    .filter(|record| {
                        matches!(
                            record.payload(),
                            crate::session_authority::SessionFactPayload::TurnClosed(_)
                        )
                    })
                    .count() as u32,
                semantic_tool_terminal_count: tool_count,
                sessionless: false,
            },
            initial_prompt,
            outcome: crate::session_advisory::default_assistant_outcome(&replay, minimum, 500),
            routes,
        }
    }

    fn read_entries_text(&self, n: usize) -> anyhow::Result<(String, Value)> {
        if !self.log_path.exists() {
            return Ok((
                format!("No agent journal found at {}", self.log_path.display()),
                json!({"entries": [], "total": 0}),
            ));
        }

        let content = fs::read_to_string(&self.log_path)?;
        let entries: Vec<&str> = content.split("\n## ").collect();
        let has_header = entries
            .first()
            .is_some_and(|e| e.starts_with('#') && !e.starts_with("## "));
        let total = if has_header {
            entries.len().saturating_sub(1)
        } else {
            entries.len()
        };

        if total == 0 {
            return Ok((
                "No entries found in agent journal".into(),
                json!({"entries": [], "total": 0}),
            ));
        }

        let recent_raw: Vec<&str> = entries
            .iter()
            .skip(1)
            .rev()
            .take(n)
            .rev()
            .copied()
            .collect();
        let recent_text: Vec<String> = recent_raw.iter().map(|e| format!("## {e}")).collect();
        let recent_structured: Vec<Value> = recent_raw
            .iter()
            .map(|e| json!({"entry": format!("## {e}")}))
            .collect();

        Ok((
            format!(
                "Recent agent journal entries ({} of {}):\n\n{}",
                recent_text.len(),
                total,
                recent_text.join("\n")
            ),
            json!({"entries": recent_structured, "total": total, "returned": recent_text.len()}),
        ))
    }

    fn read_entries(&self, n: usize) -> CommandResult {
        match self.read_entries_text(n) {
            Ok((text, _details)) => CommandResult::Display(text),
            Err(e) => CommandResult::Display(format!("Error reading session log: {e}")),
        }
    }

    fn usage_report_text(&self, n: usize) -> anyhow::Result<(String, Value)> {
        if !self.log_path.exists() {
            return Ok((
                format!("No agent journal found at {}", self.log_path.display()),
                json!({"sessions": 0, "turns": 0}),
            ));
        }

        let content = fs::read_to_string(&self.log_path)?;
        let entries: Vec<&str> = content.split("\n## ").collect();
        let raw_entries: Vec<&str> = entries
            .iter()
            .skip(1)
            .filter(|entry| !entry.trim().is_empty())
            .copied()
            .collect();

        let selected: Vec<&str> = raw_entries.iter().rev().take(n).rev().copied().collect();
        let stats = summarize_usage_entries(&selected);
        let provider_breakdown = summarize_usage_by_provider(&selected);
        let model_breakdown = summarize_usage_by_model(&selected);
        let avg_input = if stats.turns > 0 {
            stats.total_input_tokens / stats.turns as u64
        } else {
            0
        };
        let avg_output = if stats.turns > 0 {
            stats.total_output_tokens / stats.turns as u64
        } else {
            0
        };
        let provider_lines = format_usage_breakdown(&provider_breakdown);
        let model_lines = format_usage_breakdown(&model_breakdown);

        let text = format!(
            "Session usage summary ({returned} of {total} sessions)\n\n- sessions: {sessions}\n- turns: {turns}\n- input tokens: {input}\n- output tokens: {output}\n- cache read tokens: {cache}\n- avg input/turn: {avg_input}\n- avg output/turn: {avg_output}\n- max input/turn: {max_input}\n- max output/turn: {max_output}\n\nProvider breakdown:\n{provider_lines}\n\nModel breakdown:\n{model_lines}",
            returned = selected.len(),
            total = raw_entries.len(),
            sessions = stats.sessions,
            turns = stats.turns,
            input = stats.total_input_tokens,
            output = stats.total_output_tokens,
            cache = stats.total_cache_read_tokens,
            avg_input = avg_input,
            avg_output = avg_output,
            max_input = stats.max_input_tokens,
            max_output = stats.max_output_tokens,
            provider_lines = provider_lines,
            model_lines = model_lines,
        );

        Ok((
            text,
            json!({
                "sessions": stats.sessions,
                "turns": stats.turns,
                "total_input_tokens": stats.total_input_tokens,
                "total_output_tokens": stats.total_output_tokens,
                "total_cache_read_tokens": stats.total_cache_read_tokens,
                "avg_input_tokens": avg_input,
                "avg_output_tokens": avg_output,
                "max_input_tokens": stats.max_input_tokens,
                "max_output_tokens": stats.max_output_tokens,
                "provider_breakdown": usage_breakdown_json(&provider_breakdown),
                "model_breakdown": usage_breakdown_json(&model_breakdown),
                "returned": selected.len(),
                "total": raw_entries.len(),
            }),
        ))
    }

    fn usage_report(&self, n: usize) -> CommandResult {
        match self.usage_report_text(n) {
            Ok((text, _details)) => CommandResult::Display(text),
            Err(e) => CommandResult::Display(format!("Error reading session log usage: {e}")),
        }
    }
}

#[cfg(test)]
pub(crate) fn recovery_campaign_probe(
    root: &Path,
    target: &crate::session_consumers::SessionViewTarget,
    scenario_id: &str,
) -> std::io::Result<()> {
    let binding = crate::session_consumers::SessionViewBinding::new(
        target.snapshot.clone(),
        target.session_id.clone(),
    );
    binding.replace(target.clone());
    let deferred = crate::session_consumers::DeferredSessionViewBinding::default();
    deferred.bind(binding);
    let feature = SessionLog::new(root).with_session_binding(deferred);

    if scenario_id == "AC45" {
        fs::remove_file(target.snapshot.with_extension("authority.jsonl"))?;
        let narrative = feature.narrative(0);
        if narrative.provenance.sessionless
            || narrative.provenance.authority_role != "semantic_source_unavailable"
            || narrative.provenance.source_frontier.is_some()
        {
            return Err(std::io::Error::other(
                "authority-backed journal failure fabricated a sessionless source",
            ));
        }
    } else {
        let narrative = feature.narrative(0);
        if narrative.provenance.sessionless
            || narrative.initial_prompt.as_deref() != Some("legacy request")
        {
            return Err(std::io::Error::other(
                "legacy journal narrative lost its compatibility provenance",
            ));
        }
    }

    let partial_root = root.join(format!("{scenario_id}-partial"));
    let partial = SessionLog::new(&partial_root);
    fs::write(
        &partial.log_path,
        "# Agent Journal\n\n<!-- omegon-journal-provenance-v1 {\"schema_version\":1\n",
    )?;
    if partial.append_entry(0.0, partial.narrative(0)).is_ok() {
        return Err(std::io::Error::other(
            "partial journal provenance did not stop duplicate publication",
        ));
    }
    Ok(())
}

fn format_context_composition(comp: &ContextComposition, context_window: usize) -> String {
    format!(
        "ctx est:{} win:{} sys:{} tools:{} conv:{} mem:{} hist:{} think:{} free:{} base:{} hud:{} intent:{} ext:{} tguide:{} fguide:{}",
        comp.conversation_tokens
            + comp.system_tokens
            + comp.memory_tokens
            + comp.tool_schema_tokens
            + comp.tool_history_tokens
            + comp.thinking_tokens,
        context_window,
        comp.system_tokens,
        comp.tool_schema_tokens,
        comp.conversation_tokens,
        comp.memory_tokens,
        comp.tool_history_tokens,
        comp.thinking_tokens,
        comp.free_tokens,
        comp.base_prompt_tokens,
        comp.session_hud_tokens,
        comp.intent_tokens,
        comp.external_injection_tokens,
        comp.tool_guidance_tokens,
        comp.file_guidance_tokens,
    )
}

fn parse_turn_usage_line(line: &str) -> Option<ParsedTurnUsage<'_>> {
    let rest = line.strip_prefix("- turn ")?;
    let after_turn = rest.split_once("—")?.1.trim();
    let (provider_model, usage_and_context) = after_turn.split_once(" in:")?;
    let (provider, model) = provider_model.split_once(" / ")?;
    let input_tokens = usage_and_context
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let output_tokens = line
        .split("out:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    let cache_read_tokens = line
        .split("cache:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    Some(ParsedTurnUsage {
        provider: provider.trim(),
        model: model.trim(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        context: parse_context_composition(line),
    })
}

fn parse_context_composition(line: &str) -> Option<ParsedContextComposition> {
    Some(ParsedContextComposition {
        est_tokens: parse_context_field(line, "ctx est:")?,
        window_tokens: parse_context_field(line, "win:")?,
        system_tokens: parse_context_field(line, "sys:")?,
        tool_schema_tokens: parse_context_field(line, "tools:")?,
        conversation_tokens: parse_context_field(line, "conv:")?,
        memory_tokens: parse_context_field(line, "mem:")?,
        tool_history_tokens: parse_context_field(line, "hist:")?,
        thinking_tokens: parse_context_field(line, "think:")?,
        free_tokens: parse_context_field(line, "free:")?,
        base_prompt_tokens: parse_context_field(line, "base:").unwrap_or(0),
        session_hud_tokens: parse_context_field(line, "hud:").unwrap_or(0),
        intent_tokens: parse_context_field(line, "intent:").unwrap_or(0),
        external_injection_tokens: parse_context_field(line, "ext:").unwrap_or(0),
        tool_guidance_tokens: parse_context_field(line, "tguide:").unwrap_or(0),
        file_guidance_tokens: parse_context_field(line, "fguide:").unwrap_or(0),
    })
}

fn parse_context_field(line: &str, key: &str) -> Option<u64> {
    line.split(key)
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
}

fn summarize_usage_entries(entries: &[&str]) -> UsageStats {
    let mut stats = UsageStats {
        sessions: entries.len(),
        ..UsageStats::default()
    };

    for entry in entries {
        for line in entry.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("- turn ") {
                continue;
            }
            let Some(parsed) = parse_turn_usage_line(trimmed) else {
                continue;
            };
            stats.turns += 1;
            stats.total_input_tokens += parsed.input_tokens;
            stats.total_output_tokens += parsed.output_tokens;
            stats.total_cache_read_tokens += parsed.cache_read_tokens;
            stats.max_input_tokens = stats.max_input_tokens.max(parsed.input_tokens);
            stats.max_output_tokens = stats.max_output_tokens.max(parsed.output_tokens);
        }
    }

    stats
}

fn summarize_usage_by_provider(entries: &[&str]) -> BTreeMap<String, UsageBucket> {
    summarize_usage_breakdown(entries, |parsed| parsed.provider.to_string())
}

fn summarize_usage_by_model(entries: &[&str]) -> BTreeMap<String, UsageBucket> {
    summarize_usage_breakdown(entries, |parsed| parsed.model.to_string())
}

fn summarize_usage_breakdown<F>(entries: &[&str], key_fn: F) -> BTreeMap<String, UsageBucket>
where
    F: Fn(&ParsedTurnUsage<'_>) -> String,
{
    let mut buckets = BTreeMap::new();
    for entry in entries {
        for line in entry.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("- turn ") {
                continue;
            }
            let Some(parsed) = parse_turn_usage_line(trimmed) else {
                continue;
            };
            let bucket = buckets
                .entry(key_fn(&parsed))
                .or_insert_with(UsageBucket::default);
            bucket.turns += 1;
            bucket.total_input_tokens += parsed.input_tokens;
            bucket.total_output_tokens += parsed.output_tokens;
            bucket.total_cache_read_tokens += parsed.cache_read_tokens;
            if let Some(context) = parsed.context {
                bucket.total_ctx_est_tokens += context.est_tokens;
                bucket.total_ctx_window_tokens += context.window_tokens;
                bucket.total_system_tokens += context.system_tokens;
                bucket.total_tool_schema_tokens += context.tool_schema_tokens;
                bucket.total_conversation_tokens += context.conversation_tokens;
                bucket.total_memory_tokens += context.memory_tokens;
                bucket.total_tool_history_tokens += context.tool_history_tokens;
                bucket.total_thinking_tokens += context.thinking_tokens;
                bucket.total_free_tokens += context.free_tokens;
            }
        }
    }
    buckets
}

fn format_usage_breakdown(buckets: &BTreeMap<String, UsageBucket>) -> String {
    if buckets.is_empty() {
        return "- none".to_string();
    }
    buckets
        .iter()
        .map(|(name, bucket)| {
            let avg = |total: u64| -> u64 {
                if bucket.turns > 0 {
                    total / bucket.turns as u64
                } else {
                    0
                }
            };
            format!(
                "- {name}: {turns} turn(s), in {input}, out {output}, cache {cache}, avg ctx est {ctx_est}, avg win {win}, avg sys {sys}, avg tools {tools}, avg conv {conv}, avg mem {mem}, avg hist {hist}, avg think {think}, avg free {free}",
                turns = bucket.turns,
                input = bucket.total_input_tokens,
                output = bucket.total_output_tokens,
                cache = bucket.total_cache_read_tokens,
                ctx_est = avg(bucket.total_ctx_est_tokens),
                win = avg(bucket.total_ctx_window_tokens),
                sys = avg(bucket.total_system_tokens),
                tools = avg(bucket.total_tool_schema_tokens),
                conv = avg(bucket.total_conversation_tokens),
                mem = avg(bucket.total_memory_tokens),
                hist = avg(bucket.total_tool_history_tokens),
                think = avg(bucket.total_thinking_tokens),
                free = avg(bucket.total_free_tokens),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn usage_breakdown_json(buckets: &BTreeMap<String, UsageBucket>) -> Value {
    Value::Array(
        buckets
            .iter()
            .map(|(name, bucket)| {
                json!({
                    "name": name,
                    "turns": bucket.turns,
                    "total_input_tokens": bucket.total_input_tokens,
                    "total_output_tokens": bucket.total_output_tokens,
                    "total_cache_read_tokens": bucket.total_cache_read_tokens,
                    "avg_ctx_est_tokens": if bucket.turns > 0 { bucket.total_ctx_est_tokens / bucket.turns as u64 } else { 0 },
                    "avg_ctx_window_tokens": if bucket.turns > 0 { bucket.total_ctx_window_tokens / bucket.turns as u64 } else { 0 },
                    "avg_system_tokens": if bucket.turns > 0 { bucket.total_system_tokens / bucket.turns as u64 } else { 0 },
                    "avg_tool_schema_tokens": if bucket.turns > 0 { bucket.total_tool_schema_tokens / bucket.turns as u64 } else { 0 },
                    "avg_conversation_tokens": if bucket.turns > 0 { bucket.total_conversation_tokens / bucket.turns as u64 } else { 0 },
                    "avg_memory_tokens": if bucket.turns > 0 { bucket.total_memory_tokens / bucket.turns as u64 } else { 0 },
                    "avg_tool_history_tokens": if bucket.turns > 0 { bucket.total_tool_history_tokens / bucket.turns as u64 } else { 0 },
                    "avg_thinking_tokens": if bucket.turns > 0 { bucket.total_thinking_tokens / bucket.turns as u64 } else { 0 },
                    "avg_free_tokens": if bucket.turns > 0 { bucket.total_free_tokens / bucket.turns as u64 } else { 0 },
                })
            })
            .collect(),
    )
}

fn format_turn_summary(summary: &TurnSummary) -> String {
    let provider = summary.provider.as_deref().unwrap_or("unknown");
    let model = summary.model.as_deref().unwrap_or("unknown-model");
    let mut parts = vec![format!(
        "turn {} — {} / {} in:{} out:{} cache:{}",
        summary.turn,
        provider,
        model,
        summary.actual_input_tokens,
        summary.actual_output_tokens,
        summary.cache_read_tokens
    )];
    parts.push(format_context_composition(
        &summary.context_composition,
        summary.context_window,
    ));

    if let Some(telemetry) = &summary.provider_telemetry {
        match telemetry.provider.as_str() {
            "anthropic" => {
                if let Some(pct) = telemetry.unified_5h_utilization_pct {
                    parts.push(format!("5h {:.0}%", pct));
                }
                if let Some(pct) = telemetry.unified_7d_utilization_pct {
                    parts.push(format!("7d {:.0}%", pct));
                }
            }
            "openai-codex" => {
                if let Some(ref name) = telemetry.codex_limit_name {
                    parts.push(name.clone());
                }
                if let Some(active) = &telemetry.codex_active_limit {
                    parts.push(active.clone());
                }
                if let Some(pct) = telemetry.codex_primary_used_pct {
                    parts.push(format!("primary used {:.0}%", pct));
                    parts.push(format!(
                        "primary left {:.0}%",
                        (100.0 - pct).clamp(0.0, 100.0)
                    ));
                }
                if let Some(secs) = telemetry.codex_primary_reset_secs {
                    parts.push(format!("primary↻ {}s", secs));
                }
            }
            _ => {
                if let Some(rem) = telemetry.requests_remaining {
                    parts.push(format!("req {}", rem));
                }
                if let Some(rem) = telemetry.tokens_remaining {
                    parts.push(format!("tok {}", rem));
                }
                if let Some(secs) = telemetry.retry_after_secs {
                    parts.push(format!("retry {}s", secs));
                }
            }
        }
    }

    parts.join(" · ")
}

fn same_journal_source(left: &JournalProvenanceV1, right: &JournalProvenanceV1) -> bool {
    left.session_id == right.session_id
        && left.stream_id == right.stream_id
        && left.turn_ids == right.turn_ids
        && left.source_frontier == right.source_frontier
        && left.sessionless == right.sessionless
}

#[cfg(unix)]
fn lock_file(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn lock_file(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn unlock_file(file: &std::fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn unlock_file(_file: &std::fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[async_trait]
impl Feature for SessionLog {
    fn name(&self) -> &str {
        "session-log"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: crate::tool_registry::session_log::SESSION_LOG.into(),
            label: "agent_journal".into(),
            description: "Read recent agent-journal narrative so the harness can inspect prior work without operator slash commands.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "recent", "usage"],
                        "description": "Read session log entries or summarize token usage"
                        ..Default::default()
                    },
                    "count": {
                        "type": "number",
                        "description": "Number of recent entries to return (default 5)"
                    }
                },
                "required": ["action"]
            }),
            capabilities: vec![omegon_traits::ToolCapability::Orientation],
        }]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        if tool_name != crate::tool_registry::session_log::SESSION_LOG {
            anyhow::bail!("Unknown tool: {tool_name}");
        }

        let action = args["action"].as_str().unwrap_or("read");
        let count = args["count"].as_u64().unwrap_or(5) as usize;
        match action {
            "read" | "recent" => {
                let (text, details) = self.read_entries_text(count)?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details,
                })
            }
            "usage" => {
                let (text, details) = self.usage_report_text(count)?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text }],
                    details,
                })
            }
            _ => anyhow::bail!("Unknown action: {action}"),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "session-log".into(),
            description: "Read .omegon/agent-journal.md entries and summarize usage".into(),
            subcommands: vec!["read".into(), "usage".into()],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::READ_ONLY,
            surface: Default::default(),
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name != "session-log" {
            return CommandResult::NotHandled;
        }

        let trimmed = args.trim();
        if trimmed.starts_with("read") || trimmed.is_empty() {
            let n_str = trimmed.strip_prefix("read").unwrap_or("").trim();
            let n = n_str.parse::<usize>().unwrap_or(5);
            return self.read_entries(n);
        }
        if trimmed.starts_with("usage") {
            let n_str = trimmed.strip_prefix("usage").unwrap_or("").trim();
            let n = n_str.parse::<usize>().unwrap_or(10);
            return self.usage_report(n);
        }

        CommandResult::Display("Usage: /session-log [read [n] | usage [n]]".into())
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        let snippet = self.context_snippet.as_ref()?;
        Some(ContextInjection {
            source: "session-log".into(),
            content: format!("[Recent sessions]\n\n{snippet}"),
            priority: 50,
            ttl_turns: 999,
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::SessionStart { session_id, .. } => {
                self.event_session_id = Some(session_id.clone());
                self.context_snippet = self.read_narrative_entries(3);
                self.turn_summaries.clear();
                if self.context_snippet.is_some() {
                    tracing::info!(
                        "Session log context loaded from {}",
                        self.log_path.display()
                    );
                }
            }
            BusEvent::TurnEnd(te) => {
                self.turn_summaries.push(TurnSummary {
                    session_generation: self.session_binding.as_ref().and_then(|binding| {
                        binding
                            .snapshot()
                            .map(|target| (target.session_id, target.generation))
                    }),
                    turn: te.turn,
                    model: te.model.clone(),
                    provider: te.provider.clone(),
                    estimated_tokens: te.estimated_tokens,
                    context_window: te.context_window,
                    context_composition: te.context_composition.clone(),
                    actual_input_tokens: te.actual_input_tokens,
                    actual_output_tokens: te.actual_output_tokens,
                    cache_read_tokens: te.cache_read_tokens,
                    provider_telemetry: te.provider_telemetry.clone(),
                });
            }
            BusEvent::SessionEnd {
                turns,
                duration_secs,
                ..
            }
                // Only write an entry if the session did meaningful work
                if *turns > 0 => {
                    let narrative = self.narrative(*turns);
                    match self.append_entry(*duration_secs, narrative) {
                        Ok(true) => tracing::info!(
                            "Session log entry appended to {}",
                            self.log_path.display()
                        ),
                        Ok(false) => {}
                        Err(error) => tracing::warn!(
                            error = %error,
                            "best-effort session journal append failed"
                        ),
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
    use crate::bus::EventBus;

    fn semantic_feature(
        fixture: &str,
    ) -> (
        tempfile::TempDir,
        SessionLog,
        crate::session_consumers::DeferredSessionViewBinding,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("fixture-session.json");
        let authority = directory.path().join("fixture-session.authority.jsonl");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/session-semantic-v1")
                .join(fixture),
            authority,
        )
        .unwrap();
        let binding = crate::session_consumers::SessionViewBinding::new(
            snapshot.clone(),
            "fixture-session".into(),
        );
        binding.replace(crate::session_consumers::SessionViewTarget {
            binding_id: uuid::Uuid::new_v4(),
            snapshot,
            session_id: "fixture-session".into(),
            stream_id: Some(Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap()),
            generation: 7,
            kind: crate::session_consumers::SessionViewKind::Resume,
        });
        let deferred = crate::session_consumers::DeferredSessionViewBinding::default();
        deferred.bind(binding);
        let feature = SessionLog::new(directory.path()).with_session_binding(deferred.clone());
        (directory, feature, deferred)
    }

    fn test_narrative(
        turns: u32,
        tool_calls: u32,
        prompt: Option<&str>,
        outcome: Option<&str>,
    ) -> JournalNarrativeV1 {
        JournalNarrativeV1 {
            provenance: JournalProvenanceV1 {
                schema_version: 1,
                authority_role: "workspace_narrative_not_authority".into(),
                session_id: Some("test-session".into()),
                stream_id: None,
                host_generation: None,
                turn_ids: Vec::new(),
                source_frontier: None,
                semantic_turn_count: turns,
                semantic_tool_terminal_count: tool_calls,
                sessionless: true,
            },
            initial_prompt: prompt.map(str::to_owned),
            outcome: outcome.map(str::to_owned),
            routes: Vec::new(),
        }
    }

    #[test]
    fn no_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let feature = SessionLog::new(dir.path());
        assert!(feature.read_narrative_entries(3).is_none());
    }

    #[test]
    fn read_narrative_strips_file_ops() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        fs::write(
            &log_path,
            "# Session Log\n\n\
             ## 2026-03-24T00:59:23.896Z\n\
             Session: 880 file operations\n\
             Written: core/crates/omegon/src/tool_registry.rs, core/crates/omegon/build.rs\n\
             Edited: core/crates/omegon/src/tools/mod.rs, core/crates/omegon/src/bus.rs\n\n\
             ## 2026-03-25 — main (12t 45tc 8m)\n\n\
             **Active:**\n- orchestratable-provider-model (43/43)\n\n\
             **Commits:**\n  abc1234 feat: session log narrative\n",
        )
        .unwrap();

        let feature = SessionLog::new(dir.path());
        let snippet = feature.read_narrative_entries(3).unwrap();

        // The file-op dump entry should have its noise stripped
        assert!(
            !snippet.contains("880 file operations"),
            "should strip file-op header"
        );
        assert!(!snippet.contains("Written:"), "should strip Written: lines");
        assert!(!snippet.contains("Edited:"), "should strip Edited: lines");

        // Narrative entry should be intact
        assert!(
            snippet.contains("orchestratable-provider-model"),
            "should keep narrative"
        );
        assert!(
            snippet.contains("feat: session log narrative"),
            "should keep commits"
        );
    }

    #[test]
    fn read_narrative_last_n_entries() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        let mut content = "# Session Log\n\n".to_string();
        for i in 1..=5 {
            content.push_str(&format!(
                "## 2026-03-{i:02} — main (1t 2tc 30s)\n\nEntry {i}.\n\n"
            ));
        }
        fs::write(&log_path, &content).unwrap();

        let feature = SessionLog::new(dir.path());
        let snippet = feature.read_narrative_entries(2).unwrap();

        // Should have last 2 entries
        assert!(snippet.contains("Entry 4"), "should include 4th entry");
        assert!(snippet.contains("Entry 5"), "should include 5th entry");
        assert!(!snippet.contains("Entry 1"), "should not include 1st entry");
        assert!(!snippet.contains("Entry 2"), "should not include 2nd entry");
        assert!(!snippet.contains("Entry 3"), "should not include 3rd entry");
    }

    #[test]
    fn append_entry_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let feature = SessionLog::new(dir.path());

        feature
            .append_entry(
                300.0,
                test_narrative(5, 20, Some("build the widget"), Some("widget built")),
            )
            .unwrap();

        assert!(feature.log_path.exists(), "log file should be created");
        let content = fs::read_to_string(&feature.log_path).unwrap();
        assert!(content.contains("# Agent Journal"), "should have header");
        assert!(content.contains("(5t 20tc 5m0s)"), "should have stats");
        assert!(
            content.contains("**Task:** build the widget"),
            "should have task"
        );
        assert!(
            content.contains("**Outcome:** widget built"),
            "should have outcome"
        );
    }

    #[test]
    fn append_entry_appends_not_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        fs::write(
            &log_path,
            "# Agent Journal\n\n## 2026-03-01 — existing entry\n",
        )
        .unwrap();

        let feature = SessionLog::new(dir.path());
        feature
            .append_entry(60.0, test_narrative(3, 10, None, None))
            .unwrap();

        let content = fs::read_to_string(&log_path).unwrap();
        assert!(
            content.contains("existing entry"),
            "should preserve existing content"
        );
        assert!(
            content.contains("(3t 10tc 1m0s)"),
            "should append new entry"
        );
    }

    #[test]
    fn semantic_journal_preserves_legacy_markdown_and_deduplicates_provenance() {
        let (directory, mut feature, _) = semantic_feature("slice-1-closed.authority.jsonl");
        let legacy = "# Agent Journal\n\n## 2025-01-01 — legacy\n\nHand-written history.\n";
        fs::write(&feature.log_path, legacy).unwrap();
        feature.on_event(&BusEvent::SessionStart {
            cwd: directory.path().into(),
            session_id: "fixture-session".into(),
        });
        let end = BusEvent::SessionEnd {
            turns: 99,
            tool_calls: 99,
            duration_secs: 2.0,
            initial_prompt: Some("advisory prompt must not win".into()),
            outcome_summary: Some("advisory outcome must not win".into()),
        };
        feature.on_event(&end);
        feature.on_event(&end);

        let journal = fs::read_to_string(&feature.log_path).unwrap();
        assert!(journal.starts_with(legacy));
        assert_eq!(journal.matches("omegon-journal-provenance-v1").count(), 1);
        assert!(journal.contains("legacy request"));
        assert!(journal.contains("(1t 0tc"));
        assert!(!journal.contains("advisory prompt must not win"));
        assert!(!journal.contains("advisory outcome must not win"));
    }

    #[test]
    fn replacement_fences_an_old_generation_journal_entry() {
        let (_directory, feature, deferred) = semantic_feature("slice-1-closed.authority.jsonl");
        let old = feature.narrative(1);
        let target = deferred.snapshot().unwrap();
        let replacement = crate::session_consumers::SessionViewBinding::new(
            target.snapshot.clone(),
            target.session_id.clone(),
        );
        replacement.replace(crate::session_consumers::SessionViewTarget {
            generation: target.generation + 1,
            ..target
        });
        deferred.bind(replacement);

        assert!(!feature.append_entry(1.0, old).unwrap());
        assert!(!feature.log_path.exists());
    }

    #[test]
    fn authority_backed_journal_failure_is_not_labeled_sessionless() {
        let (_directory, feature, _) = semantic_feature("slice-1-closed.authority.jsonl");
        let target = feature
            .session_binding
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap();
        fs::remove_file(target.snapshot.with_extension("authority.jsonl")).unwrap();
        let narrative = feature.narrative(9);
        assert!(!narrative.provenance.sessionless);
        assert_eq!(
            narrative.provenance.authority_role,
            "semantic_source_unavailable"
        );
        assert_eq!(narrative.provenance.semantic_turn_count, 0);
        assert!(narrative.initial_prompt.is_none());
    }

    #[test]
    fn partial_existing_provenance_stops_journal_append_without_duplicate() {
        let directory = tempfile::tempdir().unwrap();
        let feature = SessionLog::new(directory.path());
        let existing =
            "# Agent Journal\n\n<!-- omegon-journal-provenance-v1 {\"schema_version\":1\n";
        fs::write(&feature.log_path, existing).unwrap();
        assert!(feature.append_entry(0.0, feature.narrative(1)).is_err());
        assert_eq!(fs::read_to_string(&feature.log_path).unwrap(), existing);
    }

    #[test]
    fn mixed_lineage_journal_never_regenerates_pre_boundary_text() {
        let (_directory, feature, binding) = semantic_feature("mixed-legacy-full.authority.jsonl");
        let (_, replay) = crate::session_advisory::load(&binding).unwrap();
        assert_eq!(
            replay
                .first_full_spine_boundary()
                .map(|frontier| frontier.sequence()),
            Some(8)
        );
        let narrative = feature.narrative(2);
        assert!(narrative.initial_prompt.is_none());
        assert_ne!(narrative.initial_prompt.as_deref(), Some("legacy request"));
        assert_eq!(narrative.provenance.semantic_turn_count, 0);
    }

    #[test]
    fn sessionless_journal_is_explicit_and_excludes_advisory_content() {
        let directory = tempfile::tempdir().unwrap();
        let mut feature = SessionLog::new(directory.path());
        feature.on_event(&BusEvent::SessionStart {
            cwd: directory.path().into(),
            session_id: "sessionless-label".into(),
        });
        feature.on_event(&BusEvent::SessionEnd {
            turns: 1,
            tool_calls: 1,
            duration_secs: 1.0,
            initial_prompt: Some("uncommitted restricted advisory".into()),
            outcome_summary: Some("uncommitted outcome".into()),
        });
        let journal = fs::read_to_string(feature.log_path).unwrap();
        assert!(journal.contains("sessionless; authority replay unavailable"));
        assert!(!journal.contains("uncommitted restricted advisory"));
        assert!(!journal.contains("uncommitted outcome"));
    }

    #[test]
    fn session_end_event_writes_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = SessionLog::new(dir.path());
        feature.on_event(&BusEvent::TurnEnd(Box::new(
            omegon_traits::BusEventTurnEnd {
                turn: 7,
                model: Some("anthropic:claude-sonnet-4-6".into()),
                provider: Some("anthropic".into()),
                estimated_tokens: 12_000,
                context_window: 200_000,
                context_composition: omegon_traits::ContextComposition {
                    system_tokens: 1000,
                    tool_schema_tokens: 500,
                    conversation_tokens: 3000,
                    memory_tokens: 250,
                    tool_history_tokens: 1200,
                    thinking_tokens: 700,
                    free_tokens: 193_350,
                    ..Default::default()
                },
                actual_input_tokens: 1200,
                actual_output_tokens: 300,
                cache_read_tokens: 40,
                provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                    provider: "anthropic".into(),
                    source: "response_headers".into(),
                    unified_5h_utilization_pct: Some(42.0),
                    ..Default::default()
                }),
                dominant_phase: None,
                drift_kind: None,
                progress_signal: omegon_traits::ProgressSignal::None,
            },
        )));

        feature.on_event(&BusEvent::SessionEnd {
            turns: 7,
            tool_calls: 30,
            duration_secs: 450.0,
            initial_prompt: Some("test prompt".into()),
            outcome_summary: Some("test outcome".into()),
        });

        assert!(
            feature.log_path.exists(),
            "log should be written on SessionEnd"
        );
        let content = fs::read_to_string(&feature.log_path).unwrap();
        assert!(content.contains("7t"), "should record turns");
        assert!(content.contains("sessionless"));
        assert!(!content.contains("test prompt"));
        assert!(!content.contains("test outcome"));
        assert!(
            content.contains("tokens across"),
            "should record token summary"
        );
    }

    #[test]
    fn summarize_usage_entries_counts_turn_tokens() {
        let entries = vec![
            "2026-04-08 — main (2t 10tc 1m)\n\n**Turns:**\n- turn 1 — anthropic / anthropic:claude-sonnet-4-6 in:1200 out:300 cache:40 · ctx est:6655 win:200000 sys:1000 tools:500 conv:3000 mem:250 hist:1200 think:705 free:193345\n- turn 2 — anthropic / anthropic:claude-sonnet-4-6 in:1500 out:250 cache:60 · ctx est:7050 win:200000 sys:1100 tools:500 conv:3200 mem:250 hist:1300 think:700 free:192950",
            "2026-04-09 — main (1t 2tc 10s)\n\n**Turns:**\n- turn 1 — openai-codex / openai-codex:gpt-5.4 in:900 out:100 cache:0 · ctx est:5100 win:200000 sys:900 tools:400 conv:2000 mem:200 hist:900 think:700 free:194900",
        ];
        let stats = summarize_usage_entries(&entries);
        assert_eq!(stats.sessions, 2);
        assert_eq!(stats.turns, 3);
        assert_eq!(stats.total_input_tokens, 3600);
        assert_eq!(stats.total_output_tokens, 650);
        assert_eq!(stats.total_cache_read_tokens, 100);
        assert_eq!(stats.max_input_tokens, 1500);
        assert_eq!(stats.max_output_tokens, 300);

        let providers = summarize_usage_by_provider(&entries);
        assert_eq!(providers["anthropic"].turns, 2);
        assert_eq!(providers["anthropic"].total_input_tokens, 2700);
        assert_eq!(providers["anthropic"].total_ctx_est_tokens, 13_705);
        assert_eq!(providers["anthropic"].total_system_tokens, 2_100);
        assert_eq!(providers["openai-codex"].turns, 1);
        assert_eq!(providers["openai-codex"].total_output_tokens, 100);
        assert_eq!(providers["openai-codex"].total_conversation_tokens, 2_000);

        let models = summarize_usage_by_model(&entries);
        assert_eq!(models["anthropic:claude-sonnet-4-6"].turns, 2);
        assert_eq!(models["openai-codex:gpt-5.4"].total_cache_read_tokens, 0);
    }

    #[test]
    fn usage_report_summarizes_recent_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        fs::write(
            &log_path,
            "# Agent Journal\n\n## 2026-04-08 — main (2t 10tc 1m)\n\n**Turns:**\n- turn 1 — anthropic / anthropic:claude-sonnet-4-6 in:1200 out:300 cache:40 · ctx est:6655 win:200000 sys:1000 tools:500 conv:3000 mem:250 hist:1200 think:705 free:193345\n- turn 2 — anthropic / anthropic:claude-sonnet-4-6 in:1500 out:250 cache:60 · ctx est:7050 win:200000 sys:1100 tools:500 conv:3200 mem:250 hist:1300 think:700 free:192950\n\n## 2026-04-09 — main (1t 2tc 10s)\n\n**Turns:**\n- turn 1 — openai-codex / openai-codex:gpt-5.4 in:900 out:100 cache:0 · ctx est:5100 win:200000 sys:900 tools:400 conv:2000 mem:200 hist:900 think:700 free:194900\n",
        )
        .unwrap();

        let feature = SessionLog::new(dir.path());
        let (text, details) = feature.usage_report_text(10).unwrap();
        assert!(text.contains("sessions: 2"), "got: {text}");
        assert!(text.contains("turns: 3"), "got: {text}");
        assert!(text.contains("input tokens: 3600"), "got: {text}");
        assert!(text.contains("Provider breakdown:"), "got: {text}");
        assert!(
            text.contains("- anthropic: 2 turn(s), in 2700, out 550, cache 100, avg ctx est 6852, avg win 200000, avg sys 1050, avg tools 500, avg conv 3100, avg mem 250, avg hist 1250, avg think 702, avg free 193147"),
            "got: {text}"
        );
        assert!(
            text.contains("- openai-codex:gpt-5.4: 1 turn(s), in 900, out 100, cache 0, avg ctx est 5100, avg win 200000, avg sys 900, avg tools 400, avg conv 2000, avg mem 200, avg hist 900, avg think 700, avg free 194900"),
            "got: {text}"
        );
        assert_eq!(details["total_input_tokens"].as_u64(), Some(3600));
        assert_eq!(details["turns"].as_u64(), Some(3));
        assert_eq!(
            details["provider_breakdown"][0]["name"].as_str(),
            Some("anthropic")
        );
        assert_eq!(details["provider_breakdown"][0]["turns"].as_u64(), Some(2));
        assert_eq!(
            details["provider_breakdown"][0]["avg_ctx_est_tokens"].as_u64(),
            Some(6852)
        );
        assert_eq!(
            details["provider_breakdown"][0]["avg_tool_history_tokens"].as_u64(),
            Some(1250)
        );
    }

    #[test]
    fn format_turn_summary_formats_codex_session_specific_fields() {
        let summary = TurnSummary {
            session_generation: None,
            turn: 1,
            model: Some("openai-codex:gpt-5.4".into()),
            provider: Some("openai-codex".into()),
            estimated_tokens: 150,
            context_window: 200_000,
            context_composition: omegon_traits::ContextComposition {
                system_tokens: 20,
                tool_schema_tokens: 10,
                conversation_tokens: 80,
                memory_tokens: 5,
                tool_history_tokens: 15,
                thinking_tokens: 20,
                free_tokens: 199_850,
                ..Default::default()
            },
            actual_input_tokens: 100,
            actual_output_tokens: 20,
            cache_read_tokens: 0,
            provider_telemetry: Some(omegon_traits::ProviderTelemetrySnapshot {
                provider: "openai-codex".into(),
                source: "response_headers".into(),
                codex_active_limit: Some("codex".into()),
                codex_primary_used_pct: Some(0.0),
                codex_primary_reset_secs: Some(13648),
                codex_limit_name: Some("GPT-5.3-Codex-Spark".into()),
                ..Default::default()
            }),
        };

        let text = format_turn_summary(&summary);
        assert!(text.contains("GPT-5.3-Codex-Spark"), "got: {text}");
        assert!(text.contains("codex"), "got: {text}");
        assert!(text.contains("primary used 0%"), "got: {text}");
        assert!(text.contains("primary left 100%"), "got: {text}");
        assert!(text.contains("primary↻ 13648s"), "got: {text}");
        assert!(!text.contains("5h 0%"), "got: {text}");
    }

    #[test]
    fn session_end_skips_empty_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = SessionLog::new(dir.path());

        feature.on_event(&BusEvent::SessionEnd {
            turns: 0,
            tool_calls: 0,
            duration_secs: 1.0,
            initial_prompt: None,
            outcome_summary: None,
        });

        assert!(
            !feature.log_path.exists(),
            "should not write for 0-turn sessions"
        );
    }

    #[test]
    fn read_entries_command() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        fs::write(
            &log_path,
            "# Session Log\n\n\
             ## 2026-03-17 — Day 1\n\nStuff.\n\n\
             ## 2026-03-18 — Day 2\n\nMore stuff.\n",
        )
        .unwrap();

        let mut feature = SessionLog::new(dir.path());
        let result = feature.handle_command("session-log", "read 1");
        if let CommandResult::Display(text) = result {
            assert!(text.contains("Day 2"), "should show latest entry: {text}");
        } else {
            panic!("Expected Display result");
        }
    }

    #[tokio::test]
    async fn session_log_tool_reads_entries_via_bus() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        fs::write(
            &log_path,
            "# Session Log\n\n\
             ## 2026-03-17 — Day 1\n\nStuff.\n\n\
             ## 2026-03-18 — Day 2\n\nMore stuff.\n",
        )
        .unwrap();

        let mut bus = EventBus::new();
        bus.register(Box::new(SessionLog::new(dir.path())));
        bus.finalize();

        let result = bus
            .execute_tool(
                crate::tool_registry::session_log::SESSION_LOG,
                "tc1",
                json!({"action": "read", "count": 1}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();

        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Day 2"), "should show latest entry: {text}");
        assert_eq!(result.details["total"].as_u64(), Some(2));
        assert_eq!(result.details["returned"].as_u64(), Some(1));
    }

    #[test]
    fn context_injection_after_session_start() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = {
            let d = dir.path().join(".omegon");
            std::fs::create_dir_all(&d).unwrap();
            d.join("agent-journal.md")
        };
        fs::write(
            &log_path,
            "# Log\n\n## 2026-03-18 — main (5t 20tc 3m)\n\nContext here.\n",
        )
        .unwrap();

        let mut feature = SessionLog::new(dir.path());
        let signals = omegon_traits::ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 4000,
        };
        assert!(feature.provide_context(&signals).is_none());

        feature.on_event(&BusEvent::SessionStart {
            cwd: dir.path().to_path_buf(),
            session_id: "test".into(),
        });
        let ctx = feature.provide_context(&signals).unwrap();
        assert!(ctx.content.contains("Context here"));
        assert!(ctx.content.starts_with("[Recent sessions]"));
    }
}
