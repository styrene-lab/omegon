//! Session log — append-only session tracking with context injection.
//!
//! On session start, reads the last ~80 lines of `.session_log` and injects
//! them as context. The `/session-log` command shows recent entries or
//! prompts the LLM to generate a new entry.

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use omegon_traits::{
    BusEvent, BusRequest, CommandDefinition, CommandResult,
    ContextInjection, ContextSignals, Feature,
};

pub struct SessionLog {
    log_path: PathBuf,
    /// Cached tail of the session log, read once on SessionStart.
    context_tail: Option<String>,
}

impl SessionLog {
    pub fn new(cwd: &Path) -> Self {
        Self {
            log_path: cwd.join(".session_log"),
            context_tail: None,
        }
    }

    fn read_tail(&self, lines: usize) -> Option<String> {
        let content = fs::read_to_string(&self.log_path).ok()?;
        let all_lines: Vec<&str> = content.lines().collect();
        if all_lines.is_empty() {
            return None;
        }
        let start = all_lines.len().saturating_sub(lines);
        let tail = all_lines[start..].join("\n").trim().to_string();
        if tail.is_empty() { None } else { Some(tail) }
    }

    fn read_entries(&self, n: usize) -> CommandResult {
        if !self.log_path.exists() {
            return CommandResult::Display(format!("No .session_log found at {}", self.log_path.display()));
        }

        let content = match fs::read_to_string(&self.log_path) {
            Ok(c) => c,
            Err(e) => return CommandResult::Display(format!("Error reading session log: {e}")),
        };

        // Split by ## headings — first chunk is the file header (if it starts with #)
        let entries: Vec<&str> = content.split("\n## ").collect();
        let has_header = entries.first().is_some_and(|e| e.starts_with('#') && !e.starts_with("## "));
        let total = if has_header { entries.len().saturating_sub(1) } else { entries.len() };

        if total == 0 {
            return CommandResult::Display("No entries found in .session_log".into());
        }

        let recent: Vec<String> = entries.iter()
            .skip(1) // skip header
            .rev()
            .take(n)
            .rev()
            .map(|e| format!("## {e}"))
            .collect();

        CommandResult::Display(format!(
            "Recent .session_log entries ({} of {}):\n\n{}",
            recent.len(), total, recent.join("\n")
        ))
    }
}

#[async_trait]
impl Feature for SessionLog {
    fn name(&self) -> &str {
        "session-log"
    }

    fn commands(&self) -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            name: "session-log".into(),
            description: "Append or read .session_log entries".into(),
            subcommands: vec!["read".into()],
        }]
    }

    fn handle_command(&mut self, name: &str, args: &str) -> CommandResult {
        if name != "session-log" {
            return CommandResult::NotHandled;
        }

        let trimmed = args.trim();
        if trimmed.starts_with("read") {
            let n_str = trimmed.strip_prefix("read").unwrap_or("").trim();
            let n = n_str.parse::<usize>().unwrap_or(5);
            return self.read_entries(n);
        }

        // Default: prompt to generate entry
        CommandResult::Display(
            "To generate a session log entry, ask the agent:\n  \"Write a .session_log entry for this session\"\n\nOr use: /session-log read [n]".into()
        )
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        let tail = self.context_tail.as_ref()?;
        Some(ContextInjection {
            source: "session-log".into(),
            content: format!("[Recent .session_log entries]\n\n{tail}"),
            priority: 50, // Low priority — background context
            ttl_turns: 999, // Persist for the whole session
        })
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        if let BusEvent::SessionStart { .. } = event {
            self.context_tail = self.read_tail(80);
            if self.context_tail.is_some() {
                tracing::info!("Session log context loaded from {}", self.log_path.display());
            }
        }
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let feature = SessionLog::new(dir.path());
        assert!(feature.read_tail(80).is_none());
    }

    #[test]
    fn read_tail_short_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join(".session_log");
        fs::write(&log_path, "# Session Log\n\n## 2026-03-18 — Test\n\nDid stuff.\n").unwrap();

        let feature = SessionLog::new(dir.path());
        let tail = feature.read_tail(80).unwrap();
        assert!(tail.contains("Test"));
        assert!(tail.contains("Did stuff"));
    }

    #[test]
    fn read_tail_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join(".session_log");
        let content = (0..200).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        fs::write(&log_path, &content).unwrap();

        let feature = SessionLog::new(dir.path());
        let tail = feature.read_tail(80).unwrap();
        assert!(tail.lines().count() <= 80);
        assert!(tail.contains("line 199")); // last line present
        assert!(!tail.contains("line 0")); // first line truncated
    }

    #[test]
    fn read_entries_command() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join(".session_log");
        fs::write(&log_path, "# Session Log\n\n## 2026-03-17 — Day 1\n\nStuff.\n\n## 2026-03-18 — Day 2\n\nMore stuff.\n").unwrap();

        let mut feature = SessionLog::new(dir.path());
        let result = feature.handle_command("session-log", "read 1");
        if let CommandResult::Display(text) = result {
            assert!(text.contains("Day 2"), "should show latest entry: {text}");
            assert!(text.contains("1 of 2"), "should show count: {text}");
        } else {
            panic!("Expected Display result");
        }
    }

    #[test]
    fn read_entries_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut feature = SessionLog::new(dir.path());
        let result = feature.handle_command("session-log", "read");
        assert!(matches!(result, CommandResult::Display(ref s) if s.contains("No .session_log")));
    }

    #[test]
    fn context_injection_after_session_start() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join(".session_log");
        fs::write(&log_path, "# Log\n\n## 2026-03-18 — Today\n\nContext here.\n").unwrap();

        let mut feature = SessionLog::new(dir.path());
        // Before session start, no context
        let signals = omegon_traits::ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 4000,
        };
        assert!(feature.provide_context(&signals).is_none());

        // After session start
        feature.on_event(&BusEvent::SessionStart {
            cwd: dir.path().to_path_buf(),
            session_id: "test".into(),
        });
        let ctx = feature.provide_context(&signals).unwrap();
        assert!(ctx.content.contains("Context here"));
    }
}
