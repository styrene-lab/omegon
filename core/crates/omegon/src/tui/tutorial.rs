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
    /// Upper portion — leaves the footer/instruments visible.
    Upper,
}

/// How a step advances to the next one.
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    /// Press Tab to continue (passive step).
    Enter,
    /// Wait for a specific slash command (e.g. "/focus").
    Command(&'static str),
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
    InputBar,
}

/// The compiled tutorial steps — Omegon's built-in onboarding.
pub const STEPS: &[Step] = &[
    Step {
        title: "Welcome to Omegon",
        body: "This is your AI agent cockpit.\n\nThe main area is the conversation — where you\nand the agent talk. The panels at the bottom\nshow engine status and live telemetry.",
        anchor: Anchor::Center,
        trigger: Trigger::Enter,
        highlight: None,
    },
    Step {
        title: "Engine Panel",
        body: "Look at the bottom-left of your screen.\n\nThe engine panel shows:\n  \u{2022} Model name and provider\n  \u{2022} Tier (Victory / Gloriana / Retribution)\n  \u{2022} Thinking level\n  \u{2022} Context usage percentage",
        anchor: Anchor::Upper,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::EnginePanel),
    },
    Step {
        title: "Inference Instruments",
        body: "The left instrument panel shows inference state:\n\n  \u{2022} Context bar \u{2014} navy (empty) \u{2192} teal \u{2192} amber (full)\n  \u{2022} Glitch chars appear when the agent thinks\n  \u{2022} Memory strings show fact counts per mind\n\nWaves on the strings show memory activity:\n  \u{2192} rightward = storing\n  \u{2190} leftward = recalling",
        anchor: Anchor::Upper,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::InstrumentPanel),
    },
    Step {
        title: "Tool Activity",
        body: "The right instrument panel shows tools:\n\n  \u{2022} Sorted list, most recently used at top\n  \u{2022} Recency bars (teal = just called)\n  \u{2022} Timestamps since last call\n\nAsk the agent: \"what files are in this project?\"\nand watch the tools panel light up with 'bash'.",
        anchor: Anchor::Upper,
        trigger: Trigger::AnyInput,
        highlight: Some(Highlight::InstrumentPanel),
    },
    Step {
        title: "Slash Commands",
        body: "Type / in the input bar to see commands:\n\n  /model    \u{2014} switch models\n  /think    \u{2014} adjust thinking level\n  /context  \u{2014} change context class\n  /focus    \u{2014} toggle instruments\n  /help     \u{2014} full command list\n\nTry typing /focus now.",
        anchor: Anchor::Center,
        trigger: Trigger::Command("focus"),
        highlight: Some(Highlight::InputBar),
    },
    Step {
        title: "Focus Mode",
        body: "The instruments disappeared!\n\nFocus mode gives the conversation full height.\n\nType /focus again to bring them back.",
        anchor: Anchor::Center,
        trigger: Trigger::Command("focus"),
        highlight: None,
    },
    Step {
        title: "You're Ready!",
        body: "That's the basics!\n\n  \u{2022} The agent has memory \u{2014} facts persist\n  \u{2022} The design tree tracks ideas\n  \u{2022} /help shows all commands\n  \u{2022} /tutorial restarts this guide",
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

    /// Render the tutorial overlay into the given area.
    /// `footer_height` is the actual footer height so we can position above it.
    /// Highlighting is handled by the widgets themselves — see tutorial_highlight field on App.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &dyn super::theme::Theme, footer_height: u16) {
        if !self.active { return; }

        let step = self.step();

        // Smart positioning: avoid covering highlighted areas
        let overlay = match (&step.anchor, &step.highlight) {
            // Steps highlighting footer elements → position in upper area
            (_, Some(Highlight::EnginePanel | Highlight::InstrumentPanel)) => {
                upper_rect(area, footer_height)
            }
            // Steps highlighting input → position in center-upper (above input bar)
            (_, Some(Highlight::InputBar)) => {
                upper_rect(area, footer_height + 3) // extra 3 for input bar
            }
            // Center for steps with no highlight or Center anchor
            (Anchor::Center, _) => centered_rect(area),
            (Anchor::Upper, _) => upper_rect(area, footer_height),
        };

        // Clear the area behind the overlay
        Clear.render(overlay, buf);

        // Build the call-to-action — prominent line inside the content
        let cta = match &step.trigger {
            Trigger::Enter => "  \u{25b6} Press Tab to continue",
            Trigger::Command("focus") => "  \u{25b6} Type /focus in the input bar below",
            Trigger::Command(cmd) => {
                return self.render_with_cta(overlay, buf, theme, &format!("  \u{25b6} Type /{cmd} in the input bar below"));
            }
            Trigger::AnyInput => "  \u{25b6} Type a message in the input bar below",
        };

        self.render_with_cta(overlay, buf, theme, cta);
    }

    fn render_with_cta(&self, overlay: Rect, buf: &mut Buffer, theme: &dyn super::theme::Theme, cta: &str) {
        let step = self.step();
        let progress = format!(" {}/{} ", self.current + 1, STEPS.len());
        let title_line = format!("\u{1f4d8} {} ", step.title);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent()))
            .title(Span::styled(&title_line, Style::default().fg(theme.accent()).bold()))
            .title_bottom(
                Line::from(vec![
                    Span::styled(&progress, Style::default().fg(theme.muted())),
                    Span::raw("  "),
                    Span::styled("[Esc skip] [Shift+Tab back]", Style::default().fg(theme.muted())),
                ]).right_aligned()
            );

        let inner = block.inner(overlay);
        block.render(overlay, buf);

        // Body text + call-to-action as last line
        let body_with_cta = format!("{}\n\n{}", step.body, cta);
        let text = Paragraph::new(body_with_cta)
            .style(Style::default().fg(theme.fg()))
            .wrap(Wrap { trim: false });
        text.render(inner, buf);

        // Highlight the CTA line by coloring it accent
        // Find the last non-empty line in the inner area and tint it
        let cta_y = inner.bottom().saturating_sub(1);
        if cta_y > inner.y {
            for x in inner.x..inner.right() {
                if let Some(cell) = buf.cell_mut(ratatui::prelude::Position::new(x, cta_y)) {
                    if cell.symbol() != " " {
                        cell.set_fg(theme.accent_bright());
                    }
                }
            }
        }
    }
}

/// Center a rect — fixed max size, always fits content.
fn centered_rect(parent: Rect) -> Rect {
    let w = 50u16.min(parent.width.saturating_sub(4));
    let h = 14u16.min(parent.height.saturating_sub(4));
    let x = parent.x + (parent.width.saturating_sub(w)) / 2;
    let y = parent.y + (parent.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Rect in the upper portion of the screen — leaves footer/instruments visible.
fn upper_rect(parent: Rect, footer_height: u16) -> Rect {
    let w = 50u16.min(parent.width.saturating_sub(4));
    let h = 14u16.min(parent.height.saturating_sub(footer_height + 4));
    let x = parent.x + (parent.width.saturating_sub(w)) / 2;
    // Position in upper third of available space (above footer)
    let available = parent.height.saturating_sub(footer_height);
    let y = parent.y + (available.saturating_sub(h)) / 3;
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

    #[test]
    fn centered_rect_fits_in_parent() {
        let parent = Rect::new(0, 0, 80, 24);
        let r = centered_rect(parent);
        assert!(r.right() <= parent.right());
        assert!(r.bottom() <= parent.bottom());
        assert!(r.width >= 10);
        assert!(r.height >= 10);
    }

    #[test]
    fn upper_rect_leaves_footer_visible() {
        let parent = Rect::new(0, 0, 80, 40);
        let footer_h = 12;
        let r = upper_rect(parent, footer_h);
        // The overlay should not overlap the footer region
        assert!(r.bottom() <= parent.height - footer_h,
            "overlay bottom {} should be above footer at {}", r.bottom(), parent.height - footer_h);
    }

    #[test]
    fn centered_rect_tiny_terminal() {
        // 20x10 terminal — overlay should still fit
        let parent = Rect::new(0, 0, 20, 10);
        let r = centered_rect(parent);
        assert!(r.right() <= parent.right());
        assert!(r.bottom() <= parent.bottom());
        assert!(r.width > 0);
        assert!(r.height > 0);
    }

    #[test]
    fn inactive_tutorial_does_not_consume_input() {
        let mut tut = Tutorial::new();
        tut.dismiss();
        assert!(!tut.check_enter());
        assert!(!tut.check_any_input());
        assert!(!tut.check_command("focus"));
        assert!(tut.current_highlight().is_none());
    }
}
