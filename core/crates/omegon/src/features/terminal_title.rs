//! terminal_title — Dynamic terminal tab title showing agent state.
//!
//! Sets the terminal tab/window title via ANSI OSC escape sequences.
//! Shows project name, agent state, current tool, and turn count.
//!
//! Format: Ω <project> [<status>] <activity>
//!
//! Examples:
//!   Ω omegon ✦                    — idle
//!   Ω omegon ◆ T4                 — thinking, turn 4
//!   Ω omegon ⚙ Read → Edit       — executing tools
//!   Ω omegon ✦ done               — agent finished
//!
//! Ported from extensions/terminal-title.ts (191 LoC TS → 80 LoC Rust)

use async_trait::async_trait;
use omegon_traits::{BusEvent, BusRequest, Feature};

pub struct TerminalTitle {
    project: String,
    idle: bool,
    turn: u32,
    /// Last 2 tool names for pipeline visibility.
    tool_chain: Vec<String>,
    tool_active: bool,
}

impl TerminalTitle {
    pub fn new(cwd: &str) -> Self {
        let project = cwd
            .rsplit('/')
            .next()
            .unwrap_or("project")
            .to_string();
        Self {
            project,
            idle: true,
            turn: 0,
            tool_chain: Vec::new(),
            tool_active: false,
        }
    }

    fn update_title(&self) {
        // Only set title when stderr is a real terminal (not piped/headless)
        if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            return;
        }
        let status = if self.tool_active {
            let chain = self.tool_chain.join(" → ");
            format!("⚙ {chain}")
        } else if self.idle {
            "✦".to_string()
        } else {
            format!("◆ T{}", self.turn)
        };
        let title = format!("Ω {} {}", self.project, status);
        // OSC 0 — set window title
        eprint!("\x1b]0;{title}\x07");
    }
}

#[async_trait]
impl Feature for TerminalTitle {
    fn name(&self) -> &str { "terminal-title" }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnStart { turn } => {
                self.idle = false;
                self.turn = *turn;
                self.tool_chain.clear();
                self.tool_active = false;
                self.update_title();
            }
            BusEvent::ToolStart { name, .. } => {
                self.tool_active = true;
                self.tool_chain.push(short_tool(name));
                if self.tool_chain.len() > 2 {
                    self.tool_chain.remove(0);
                }
                self.update_title();
            }
            BusEvent::ToolEnd { .. } => {
                self.tool_active = false;
                self.update_title();
            }
            BusEvent::AgentEnd => {
                self.idle = true;
                self.tool_chain.clear();
                self.update_title();
            }
            _ => {}
        }
        vec![]
    }
}

/// Shorten tool name for display (e.g. "memory_store" → "memory")
fn short_tool(name: &str) -> String {
    // Capitalize first letter, strip suffix after underscore
    let base = name.split('_').next().unwrap_or(name);
    let mut chars = base.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_tool_names() {
        assert_eq!(short_tool("read"), "Read");
        assert_eq!(short_tool("memory_store"), "Memory");
        assert_eq!(short_tool("bash"), "Bash");
    }

    #[test]
    fn initial_state_is_idle() {
        let tt = TerminalTitle::new("/home/user/project");
        assert!(tt.idle);
        assert_eq!(tt.project, "project");
    }

    #[test]
    fn tool_chain_tracks_last_two() {
        let mut tt = TerminalTitle::new("/tmp/test");
        tt.on_event(&BusEvent::TurnStart { turn: 1 });
        tt.on_event(&BusEvent::ToolStart { id: "1".into(), name: "read".into(), args: serde_json::json!({}) });
        tt.on_event(&BusEvent::ToolStart { id: "2".into(), name: "edit".into(), args: serde_json::json!({}) });
        tt.on_event(&BusEvent::ToolStart { id: "3".into(), name: "bash".into(), args: serde_json::json!({}) });
        assert_eq!(tt.tool_chain, vec!["Edit", "Bash"]); // last 2
    }
}
