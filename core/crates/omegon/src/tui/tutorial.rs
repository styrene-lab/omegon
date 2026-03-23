//! Tutorial overlay — game-style first-play advisor.
//!
//! A TUI overlay that guides new operators through Omegon's interface.
//! Steps are compiled into the binary. The overlay renders on top of
//! the normal UI, highlights relevant areas, and advances on keypress
//! or operator action.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap, Clear};

/// Where to anchor the tutorial callout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Anchor {
    /// Centered in the conversation area.
    Center,
    /// Above the footer, spanning full width.
    AboveFooter,
}

/// How a step advances to the next one.
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    /// Press Enter (or any key) to continue.
    Enter,
    /// Wait for a specific slash command (e.g. "/focus").
    Command(& 'static str),
    /// Wait for any user message to be sent.
    AnyInput,
}

/// A single tutorial step.
#[derive(Debug, Clone)]
pub struct Step {
    pub title: &'static str,
    pub body: &'static str,
    pub anchor: Anchor,
    pub trigger: Trigger,
    /// Region to highlight (pulse border).
    pub highlight: Option<Highlight>,
}

/// A TUI region to visually highlight during a tutorial step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Highlight {
    InstrumentPanel,
    EnginePanel,
    ConversationArea,
    InputBar,
}

/// The compiled tutorial steps — Omegon's built-in onboarding.
pub const STEPS: &[Step] = &[
    Step {
        title: "Welcome to Omegon",
        body: "This is your AI agent cockpit. I'll walk you through what everything does.\n\nThe main area above is the conversation — where you and the agent talk.\n\nThe panels at the bottom show engine status and live telemetry.",
        anchor: Anchor::Center,
        trigger: Trigger::Enter,
        highlight: None,
    },
    Step {
        title: "Engine Panel",
        body: "The bottom-left shows the inference engine:\n\n  • Model name and provider\n  • Tier (Victory / Gloriana / Retribution)\n  • Thinking level\n  • Context usage percentage\n\nThis is your at-a-glance status.",
        anchor: Anchor::AboveFooter,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::EnginePanel),
    },
    Step {
        title: "Instrument Panel — Inference",
        body: "The left instrument shows inference state:\n\n  • Context bar — gradient from navy (empty) through teal to amber (full)\n  • Glitch characters appear on the bar when the agent is thinking\n  • Memory strings below show fact counts per linked mind\n\nWaves on the strings show memory activity:\n  → rightward = storing, ← leftward = recalling",
        anchor: Anchor::AboveFooter,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::InstrumentPanel),
    },
    Step {
        title: "Instrument Panel — Tools",
        body: "The right instrument shows tool activity:\n\n  • A sorted list of tools, most recently used at top\n  • Each tool shows a recency bar (teal = just called, fading to navy)\n  • Timestamps show time since last call\n\nSend me a message and watch the tools light up!",
        anchor: Anchor::AboveFooter,
        trigger: Trigger::AnyInput,
        highlight: Some(Highlight::InstrumentPanel),
    },
    Step {
        title: "Slash Commands",
        body: "Type / in the input bar to see available commands:\n\n  /model    — switch models\n  /think    — adjust thinking level\n  /context  — change context class\n  /focus    — toggle instrument panel\n  /help     — full command list\n\nTry typing /focus now to toggle the instruments.",
        anchor: Anchor::Center,
        trigger: Trigger::Command("focus"),
        highlight: Some(Highlight::InputBar),
    },
    Step {
        title: "Focus Mode",
        body: "The instruments disappeared! Focus mode gives the conversation full screen height.\n\nType /focus again to bring them back.",
        anchor: Anchor::Center,
        trigger: Trigger::Command("focus"),
        highlight: None,
    },
    Step {
        title: "You're Ready",
        body: "That's the basics! A few more things:\n\n  • The agent has memory — facts persist across sessions\n  • The design tree tracks ideas from seed to implementation\n  • /tutorial shows this guide again anytime\n\nPress Enter to dismiss and start working.",
        anchor: Anchor::Center,
        trigger: Trigger::Enter,
        highlight: None,
    },
];

/// Tutorial overlay state.
pub struct Tutorial {
    /// Current step index.
    current: usize,
    /// Whether the tutorial is active (visible).
    pub active: bool,
}

impl Tutorial {
    pub fn new() -> Self {
        Self {
            current: 0,
            active: true,
        }
    }

    pub fn step(&self) -> &Step {
        &STEPS[self.current]
    }

    pub fn step_index(&self) -> usize {
        self.current
    }

    pub fn total_steps(&self) -> usize {
        STEPS.len()
    }

    /// Advance to the next step. Returns false if already at the end.
    pub fn advance(&mut self) -> bool {
        if self.current < STEPS.len() - 1 {
            self.current += 1;
            true
        } else {
            self.active = false;
            false
        }
    }

    /// Go back one step.
    pub fn go_back(&mut self) -> bool {
        if self.current > 0 {
            self.current -= 1;
            true
        } else {
            false
        }
    }

    /// Dismiss the tutorial.
    pub fn dismiss(&mut self) {
        self.active = false;
    }

    /// Check if a slash command matches the current step's trigger.
    pub fn check_command(&mut self, cmd: &str) -> bool {
        if !self.active { return false; }
        if let Trigger::Command(expected) = &self.step().trigger {
            if cmd == *expected {
                self.advance();
                return true;
            }
        }
        false
    }

    /// Check if any user input satisfies the current step's trigger.
    pub fn check_any_input(&mut self) -> bool {
        if !self.active { return false; }
        if self.step().trigger == Trigger::AnyInput {
            self.advance();
            return true;
        }
        false
    }

    /// Check if Enter was pressed and the current step accepts it.
    pub fn check_enter(&mut self) -> bool {
        if !self.active { return false; }
        if self.step().trigger == Trigger::Enter {
            self.advance();
            return true;
        }
        false
    }

    /// Get the highlight for the current step (if any).
    pub fn current_highlight(&self) -> Option<Highlight> {
        if !self.active { return None; }
        self.step().highlight
    }

    /// Render the tutorial overlay.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &dyn super::theme::Theme) {
        if !self.active { return; }

        let step = self.step();

        // Calculate overlay position and size
        let overlay = match step.anchor {
            Anchor::Center => centered_rect(area, 60, 50),
            Anchor::AboveFooter => above_footer_rect(area),
        };

        // Clear the area behind the overlay
        Clear.render(overlay, buf);

        // Build the content
        let progress = format!(" {}/{} ", self.current + 1, STEPS.len());
        let trigger_hint = match &step.trigger {
            Trigger::Enter => "▶ Enter".to_string(),
            Trigger::Command(cmd) => format!("▶ Type /{cmd}"),
            Trigger::AnyInput => "▶ Send a message".to_string(),
        };

        let title_line = format!("📘 {} ", step.title);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent()))
            .title(Span::styled(&title_line, Style::default().fg(theme.accent()).bold()))
            .title_bottom(
                Line::from(vec![
                    Span::styled(&progress, Style::default().fg(theme.muted())),
                    Span::raw("  "),
                    Span::styled(&trigger_hint, Style::default().fg(theme.accent())),
                    Span::raw("  "),
                    Span::styled("[Esc skip]", Style::default().fg(theme.muted())),
                ]).right_aligned()
            );

        let inner = block.inner(overlay);
        block.render(overlay, buf);

        let text = Paragraph::new(step.body)
            .style(Style::default().fg(theme.fg()))
            .wrap(Wrap { trim: false });
        text.render(inner, buf);
    }
}

/// Center a rect within the parent, sized as percentage of parent.
fn centered_rect(parent: Rect, pct_w: u16, pct_h: u16) -> Rect {
    let w = (parent.width as u32 * pct_w as u32 / 100).min(70) as u16;
    let h = (parent.height as u32 * pct_h as u32 / 100).min(20) as u16;
    let x = parent.x + (parent.width.saturating_sub(w)) / 2;
    let y = parent.y + (parent.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w.min(parent.width), h.min(parent.height))
}

/// Rect positioned above the footer area.
fn above_footer_rect(parent: Rect) -> Rect {
    let w = (parent.width * 60 / 100).min(70);
    let h = 12u16.min(parent.height / 2);
    let x = parent.x + (parent.width.saturating_sub(w)) / 2;
    // Position in the lower third, above where the footer would be
    let y = parent.y + parent.height.saturating_sub(h + 14);
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tutorial_starts_at_step_0() {
        let tut = Tutorial::new();
        assert_eq!(tut.step_index(), 0);
        assert!(tut.active);
    }

    #[test]
    fn tutorial_advances() {
        let mut tut = Tutorial::new();
        assert!(tut.advance());
        assert_eq!(tut.step_index(), 1);
    }

    #[test]
    fn tutorial_ends_at_last_step() {
        let mut tut = Tutorial::new();
        for _ in 0..STEPS.len() - 1 {
            assert!(tut.advance());
        }
        assert!(!tut.advance()); // at end
        assert!(!tut.active); // auto-dismisses
    }

    #[test]
    fn tutorial_go_back() {
        let mut tut = Tutorial::new();
        assert!(!tut.go_back()); // can't go back from 0
        tut.advance();
        assert!(tut.go_back());
        assert_eq!(tut.step_index(), 0);
    }

    #[test]
    fn tutorial_dismiss() {
        let mut tut = Tutorial::new();
        tut.dismiss();
        assert!(!tut.active);
    }

    #[test]
    fn check_enter_on_enter_step() {
        let mut tut = Tutorial::new();
        // Step 0 has Trigger::Enter
        assert!(tut.check_enter());
        assert_eq!(tut.step_index(), 1);
    }

    #[test]
    fn check_enter_on_command_step_does_nothing() {
        let mut tut = Tutorial::new();
        // Advance to a Command trigger step
        while tut.step().trigger == Trigger::Enter {
            tut.advance();
        }
        let idx = tut.step_index();
        assert!(!tut.check_enter());
        assert_eq!(tut.step_index(), idx); // didn't advance
    }

    #[test]
    fn check_command_matches() {
        let mut tut = Tutorial::new();
        // Find the /focus step
        while !matches!(tut.step().trigger, Trigger::Command("focus")) {
            tut.advance();
        }
        let idx = tut.step_index();
        assert!(!tut.check_command("model")); // wrong command
        assert_eq!(tut.step_index(), idx);
        assert!(tut.check_command("focus")); // right command
        assert_eq!(tut.step_index(), idx + 1);
    }

    #[test]
    fn check_any_input() {
        let mut tut = Tutorial::new();
        // Find the AnyInput step
        while tut.step().trigger != Trigger::AnyInput {
            tut.advance();
        }
        let idx = tut.step_index();
        assert!(tut.check_any_input());
        assert_eq!(tut.step_index(), idx + 1);
    }

    #[test]
    fn all_steps_have_content() {
        for (i, step) in STEPS.iter().enumerate() {
            assert!(!step.title.is_empty(), "step {i} has empty title");
            assert!(!step.body.is_empty(), "step {i} has empty body");
        }
    }
}
