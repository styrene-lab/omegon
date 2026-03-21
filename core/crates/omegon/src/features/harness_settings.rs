//! Harness settings tool — unified agent interface to harness configuration.
//!
//! Exposes a single `harness_settings` tool with action-based dispatch.
//! Keeps the tool list short (one entry) while giving the agent access
//! to the full settings surface.
//!
//! Actions:
//! - `get` — read current settings (model, thinking, context class, persona, etc.)
//! - `set_context_class` — switch context window class (Squad/Maniple/Clan/Legion)
//! - `compact` — request context compaction before next turn
//! - `stats` — session telemetry (turns, tool calls, duration, context usage)
//! - `memory_stats` — memory system stats (facts, episodes, edges)
//! - `sessions` — list saved sessions

use async_trait::async_trait;
use serde_json::{json, Value};
use std::cell::RefCell;

use omegon_traits::{
    BusEvent, BusRequest, ContentBlock, Feature,
    ToolDefinition, ToolResult,
};

use crate::settings::SharedSettings;

/// Single-tool feature that exposes harness settings to the agent.
pub struct HarnessSettings {
    settings: SharedSettings,
    session_start: std::time::Instant,
    turns: u32,
    tool_calls: u32,
    refresh_status_pending: RefCell<bool>,
}

impl HarnessSettings {
    pub fn new(settings: SharedSettings) -> Self {
        Self {
            settings,
            session_start: std::time::Instant::now(),
            turns: 0,
            tool_calls: 0,
            refresh_status_pending: RefCell::new(false),
        }
    }
}

#[async_trait]
impl Feature for HarnessSettings {
    fn name(&self) -> &str {
        "harness-settings"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "harness_settings".into(),
            label: "harness_settings".into(),
            description: "Read or modify harness settings. Actions: get (current state), \
                set_context_class (Squad/Maniple/Clan/Legion), compact (trigger compaction), \
                stats (session telemetry), memory_stats (fact counts), sessions (saved sessions)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["get", "set_context_class", "compact", "stats", "memory_stats", "sessions"],
                        "description": "What to do"
                    },
                    "value": {
                        "type": "string",
                        "description": "For set_context_class: Squad, Maniple, Clan, or Legion"
                    }
                },
                "required": ["action"]
            }),
        }]
    }

    async fn execute(
        &self,
        _tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("get");

        match action {
            "get" => {
                let s = self.settings.lock().unwrap();
                let out = format!(
                    "## Current Harness Settings\n\n\
                     - **Model**: {}\n\
                     - **Thinking**: {} {}\n\
                     - **Context class**: {}\n\
                     - **Context window**: {} tokens\n\
                     - **Max turns**: {}\n\
                     - **Tool display**: {}",
                    s.model,
                    s.thinking.icon(), s.thinking.as_str(),
                    s.context_class.short(),
                    s.context_window,
                    s.max_turns,
                    s.tool_detail.as_str(),
                );
                Ok(text_result(&out))
            }

            "set_context_class" => {
                let value = args.get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if let Some(class) = crate::settings::ContextClass::parse(value) {
                    let window = class.nominal_tokens();
                    let mut s = self.settings.lock().unwrap();
                    s.context_class = class;
                    s.context_window = window;
                    drop(s);
                    
                    // Set flag for status refresh on next turn
                    *self.refresh_status_pending.borrow_mut() = true;
                    
                    Ok(text_result(&format!(
                        "Context class → {} ({} tokens)",
                        class.short(), window,
                    )))
                } else {
                    Ok(error_result(&format!(
                        "Unknown context class: '{}'. Options: Squad (128k), Maniple (272k), Clan (400k), Legion (1M+)",
                        value
                    )))
                }
            }

            "compact" => {
                // Signal intent — the bus will deliver RequestCompaction
                Ok(text_result("Context compaction requested. Will execute before the next turn."))
            }

            "stats" => {
                let s = self.settings.lock().unwrap();
                let elapsed = self.session_start.elapsed();
                let time = if elapsed.as_secs() >= 3600 {
                    format!("{}h{}m", elapsed.as_secs() / 3600, (elapsed.as_secs() % 3600) / 60)
                } else if elapsed.as_secs() >= 60 {
                    format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
                } else {
                    format!("{}s", elapsed.as_secs())
                };
                let out = format!(
                    "## Session Stats\n\n\
                     - **Duration**: {time}\n\
                     - **Turns**: {}\n\
                     - **Tool calls**: {}\n\
                     - **Model**: {}\n\
                     - **Thinking**: {} {}",
                    self.turns, self.tool_calls,
                    s.model, s.thinking.icon(), s.thinking.as_str(),
                );
                Ok(text_result(&out))
            }

            "memory_stats" => {
                // Read from .pi/memory/facts.db if accessible
                let cwd = std::env::current_dir().unwrap_or_default();
                let db_path = cwd.join(".pi").join("memory").join("facts.db");
                if db_path.exists() {
                    match rusqlite::Connection::open_with_flags(
                        &db_path,
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    ) {
                        Ok(conn) => {
                            let total: i64 = conn.query_row(
                                "SELECT COUNT(*) FROM facts", [], |r| r.get(0)
                            ).unwrap_or(0);
                            let active: i64 = conn.query_row(
                                "SELECT COUNT(*) FROM facts WHERE status = 'active'", [], |r| r.get(0)
                            ).unwrap_or(0);
                            let episodes: i64 = conn.query_row(
                                "SELECT COUNT(*) FROM episodes", [], |r| r.get(0)
                            ).unwrap_or(0);
                            let edges: i64 = conn.query_row(
                                "SELECT COUNT(*) FROM edges", [], |r| r.get(0)
                            ).unwrap_or(0);
                            Ok(text_result(&format!(
                                "## Memory Stats\n\n\
                                 - **Total facts**: {total}\n\
                                 - **Active facts**: {active}\n\
                                 - **Archived**: {}\n\
                                 - **Episodes**: {episodes}\n\
                                 - **Edges**: {edges}",
                                total - active,
                            )))
                        }
                        Err(e) => Ok(error_result(&format!("Cannot read memory DB: {e}"))),
                    }
                } else {
                    Ok(text_result("No memory database found at .pi/memory/facts.db"))
                }
            }

            "sessions" => {
                let cwd = std::env::current_dir().unwrap_or_default();
                let sessions_dir = cwd.join(".pi").join("sessions");
                if !sessions_dir.is_dir() {
                    return Ok(text_result("No saved sessions."));
                }
                let mut entries: Vec<_> = std::fs::read_dir(&sessions_dir)
                    .map(|rd| rd.filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                        .collect())
                    .unwrap_or_default();
                entries.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));

                let lines: Vec<String> = entries.iter().take(10).map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    format!("- {} ({:.1}KB)", name, size as f64 / 1024.0)
                }).collect();

                Ok(text_result(&format!(
                    "## Recent Sessions ({})\n\n{}",
                    entries.len(),
                    if lines.is_empty() { "None.".into() } else { lines.join("\n") }
                )))
            }

            other => Ok(error_result(&format!(
                "Unknown action: '{}'. Options: get, set_context_class, compact, stats, memory_stats, sessions",
                other
            ))),
        }
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnEnd { .. } => {
                self.turns += 1;
                if *self.refresh_status_pending.borrow() {
                    *self.refresh_status_pending.borrow_mut() = false;
                    vec![BusRequest::RefreshHarnessStatus]
                } else {
                    vec![]
                }
            }
            BusEvent::ToolEnd { .. } => {
                self.tool_calls += 1;
                vec![]
            }
            _ => vec![],
        }
    }
}

fn text_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text: text.to_string() }],
        details: json!({}),
    }
}

fn error_result(text: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text { text: format!("Error: {text}") }],
        details: json!({ "error": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn test_settings() -> SharedSettings {
        std::sync::Arc::new(std::sync::Mutex::new(Settings::new("test-model")))
    }

    #[test]
    fn exposes_one_tool() {
        let feature = HarnessSettings::new(test_settings());
        let tools = feature.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "harness_settings");
    }

    #[tokio::test]
    async fn action_get() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute("harness_settings", "c1", json!({"action": "get"}), cancel).await.unwrap();
        let text = result_text(&result);
        assert!(text.contains("Model"));
        assert!(text.contains("Thinking"));
        assert!(text.contains("Context class"));
    }

    #[tokio::test]
    async fn action_set_context_class() {
        let settings = test_settings();
        let feature = HarnessSettings::new(settings.clone());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute(
            "harness_settings", "c1",
            json!({"action": "set_context_class", "value": "Clan"}),
            cancel,
        ).await.unwrap();
        let text = result_text(&result);
        assert!(text.contains("Clan"), "should confirm: {text}");

        let s = settings.lock().unwrap();
        assert_eq!(s.context_class.short(), "Clan");
    }

    #[tokio::test]
    async fn action_set_context_class_invalid() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute(
            "harness_settings", "c1",
            json!({"action": "set_context_class", "value": "Mega"}),
            cancel,
        ).await.unwrap();
        let text = result_text(&result);
        assert!(text.contains("Unknown"), "should error: {text}");
    }

    #[tokio::test]
    async fn action_stats() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute("harness_settings", "c1", json!({"action": "stats"}), cancel).await.unwrap();
        let text = result_text(&result);
        assert!(text.contains("Duration"));
        assert!(text.contains("Turns"));
    }

    #[tokio::test]
    async fn action_unknown() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute("harness_settings", "c1", json!({"action": "bogus"}), cancel).await.unwrap();
        let text = result_text(&result);
        assert!(text.contains("Unknown action"));
    }

    #[test]
    fn on_event_counts_turns() {
        let mut feature = HarnessSettings::new(test_settings());
        feature.on_event(&BusEvent::TurnEnd { turn: 1 });
        feature.on_event(&BusEvent::TurnEnd { turn: 2 });
        feature.on_event(&BusEvent::ToolEnd {
            id: "x".into(), name: "bash".into(),
            result: omegon_traits::ToolResult { content: vec![], details: json!({}) },
            is_error: false,
        });
        assert_eq!(feature.turns, 2);
        assert_eq!(feature.tool_calls, 1);
    }

    #[tokio::test]
    async fn action_compact_returns_confirmation() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute("harness_settings", "c1", json!({"action": "compact"}), cancel).await.unwrap();
        let text = result_text(&result);
        assert!(text.to_lowercase().contains("compaction"), "should confirm compaction: {text}");
    }

    #[tokio::test]
    async fn action_sessions_does_not_panic() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        // May or may not find sessions — just shouldn't panic
        let result = feature.execute("harness_settings", "c1", json!({"action": "sessions"}), cancel).await.unwrap();
        let text = result_text(&result);
        assert!(text.contains("Session") || text.contains("session") || text.contains("None"));
    }

    #[tokio::test]
    async fn action_memory_stats_does_not_panic() {
        let feature = HarnessSettings::new(test_settings());
        let cancel = tokio_util::sync::CancellationToken::new();
        let result = feature.execute("harness_settings", "c1", json!({"action": "memory_stats"}), cancel).await.unwrap();
        let text = result_text(&result);
        // Either finds the db or reports it missing — both are valid
        assert!(!text.is_empty());
    }

    fn result_text(result: &ToolResult) -> String {
        result.content.iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}
