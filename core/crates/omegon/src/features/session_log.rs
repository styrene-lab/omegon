//! Session log — append-only session tracking with context injection.
//!
//! On session start, reads the last 3 narrative entries from `.omegon/agent-journal.md`
//! and injects them as context. Raw file-op lines are stripped.
//!
//! On session end, auto-appends a structured entry:
//!   - date, git branch, recent commits
//!   - session stats (turns, tool calls, duration)
//!   - active OpenSpec changes

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use serde_json::{Value, json};

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult, ContentBlock, ContextInjection,
    ContextSignals, Feature, ToolDefinition, ToolResult,
};

pub struct SessionLog {
    log_path: PathBuf,
    cwd: PathBuf,
    /// Cached narrative context, injected once on SessionStart.
    context_snippet: Option<String>,
}

impl SessionLog {
    pub fn new(cwd: &Path) -> Self {
        let omegon_dir = cwd.join(".omegon");
        let _ = fs::create_dir_all(&omegon_dir);
        Self {
            log_path: omegon_dir.join("agent-journal.md"),
            cwd: cwd.to_path_buf(),
            context_snippet: None,
        }
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
        let changes_dir = self.cwd.join("openspec/changes");
        if !changes_dir.is_dir() {
            return vec![];
        }

        let mut active = vec![];
        if let Ok(entries) = fs::read_dir(&changes_dir) {
            for entry in entries.flatten() {
                let tasks_path = entry.path().join("tasks.md");
                if !tasks_path.exists() {
                    continue;
                }
                if let Ok(content) = fs::read_to_string(&tasks_path) {
                    let total = content
                        .lines()
                        .filter(|l| l.trim_start().starts_with("- ["))
                        .count();
                    let done = content
                        .lines()
                        .filter(|l| l.trim_start().starts_with("- [x]"))
                        .count();
                    if total > 0 {
                        let name = entry.file_name().to_string_lossy().to_string();
                        active.push(format!("{name} ({done}/{total})"));
                    }
                }
            }
        }
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
    fn append_entry(&self, turns: u32, tool_calls: u32, duration_secs: f64) {
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
                date, branch, turns, tool_calls, duration
            ),
            String::new(),
        ];

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
        let prefix = if self.log_path.exists() {
            String::new()
        } else {
            header.to_string()
        };

        let existing = fs::read_to_string(&self.log_path).unwrap_or_default();
        let new_content = if existing.is_empty() {
            format!("{}{}\n", prefix, entry)
        } else {
            format!("{}\n{}\n", existing.trim_end(), entry)
        };

        let _ = fs::write(&self.log_path, new_content);
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
}

#[async_trait]
impl Feature for SessionLog {
    fn name(&self) -> &str {
        "session-log"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: crate::tool_registry::session_log::SESSION_LOG.into(),
            label: "session_log".into(),
            description: "Read recent agent-journal narrative so the harness can inspect prior work without operator slash commands.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "recent"],
                        "description": "Read session log entries"
                    },
                    "count": {
                        "type": "number",
                        "description": "Number of recent entries to return (default 5)"
                    }
                },
                "required": ["action"]
            }),
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
            _ => anyhow::bail!("Unknown action: {action}"),
        }
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "session-log".into(),
            description: "Read .omegon/agent-journal.md entries".into(),
            subcommands: vec!["read".into()],
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

        CommandResult::Display("Usage: /session-log [read [n]]".into())
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
            BusEvent::SessionStart { .. } => {
                self.context_snippet = self.read_narrative_entries(3);
                if self.context_snippet.is_some() {
                    tracing::info!(
                        "Session log context loaded from {}",
                        self.log_path.display()
                    );
                }
            }
            BusEvent::SessionEnd {
                turns,
                tool_calls,
                duration_secs,
            } => {
                // Only write an entry if the session did meaningful work
                if *turns > 0 {
                    self.append_entry(*turns, *tool_calls, *duration_secs);
                    tracing::info!("Session log entry appended to {}", self.log_path.display());
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

    #[test]
    fn no_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let feature = SessionLog::new(dir.path());
        assert!(feature.read_narrative_entries(3).is_none());
    }

    #[test]
    fn read_narrative_strips_file_ops() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = { let d = dir.path().join(".omegon"); std::fs::create_dir_all(&d).unwrap(); d.join("agent-journal.md") };
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
        let log_path = { let d = dir.path().join(".omegon"); std::fs::create_dir_all(&d).unwrap(); d.join("agent-journal.md") };
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

        feature.append_entry(5, 20, 300.0);

        assert!(feature.log_path.exists(), "log file should be created");
        let content = fs::read_to_string(&feature.log_path).unwrap();
        assert!(content.contains("# Agent Journal"), "should have header");
        assert!(content.contains("(5t 20tc 5m0s)"), "should have stats");
    }

    #[test]
    fn append_entry_appends_not_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = { let d = dir.path().join(".omegon"); std::fs::create_dir_all(&d).unwrap(); d.join("agent-journal.md") };
        fs::write(
            &log_path,
            "# Agent Journal\n\n## 2026-03-01 — existing entry\n",
        )
        .unwrap();

        let feature = SessionLog::new(dir.path());
        feature.append_entry(3, 10, 60.0);

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
    fn session_end_event_writes_entry() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = SessionLog::new(dir.path());

        feature.on_event(&BusEvent::SessionEnd {
            turns: 7,
            tool_calls: 30,
            duration_secs: 450.0,
        });

        assert!(
            feature.log_path.exists(),
            "log should be written on SessionEnd"
        );
        let content = fs::read_to_string(&feature.log_path).unwrap();
        assert!(content.contains("7t"), "should record turns");
    }

    #[test]
    fn session_end_skips_empty_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = SessionLog::new(dir.path());

        feature.on_event(&BusEvent::SessionEnd {
            turns: 0,
            tool_calls: 0,
            duration_secs: 1.0,
        });

        assert!(
            !feature.log_path.exists(),
            "should not write for 0-turn sessions"
        );
    }

    #[test]
    fn read_entries_command() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = { let d = dir.path().join(".omegon"); std::fs::create_dir_all(&d).unwrap(); d.join("agent-journal.md") };
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
        let log_path = { let d = dir.path().join(".omegon"); std::fs::create_dir_all(&d).unwrap(); d.join("agent-journal.md") };
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
        let log_path = { let d = dir.path().join(".omegon"); std::fs::create_dir_all(&d).unwrap(); d.join("agent-journal.md") };
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
