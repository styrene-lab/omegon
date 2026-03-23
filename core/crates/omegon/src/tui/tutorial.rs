//! Tutorial overlay — game-style first-play advisor.
//!
//! A TUI overlay that guides new operators through Omegon's interface.
//! Steps are compiled into the binary. The overlay renders on top of
//! the normal UI, highlights relevant areas, and advances on keypress
//! or operator action.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

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
    /// Auto-send a prompt to the agent, wait for the turn to complete.
    /// The overlay shows a "watching..." indicator while the agent works.
    AutoPrompt(&'static str),
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
    Dashboard,
}

/// The compiled tutorial steps — Omegon's built-in onboarding.
/// Four acts: Cockpit (passive UI tour), Agent Works (auto-prompted
/// tasks), Lifecycle (live cleave), Ready (wrap-up).
pub const STEPS: &[Step] = &[
    // ═══ Act 1 — The Cockpit ═══════════════════════════════════
    Step {
        title: "Welcome to Omegon",
        body: "This is your AI agent cockpit.\n\nThe main area is the conversation \u{2014} where you\nand the agent work together. The panels at the\nbottom show engine status and live telemetry.\nThe sidebar on the right tracks your design space.",
        anchor: Anchor::Center,
        trigger: Trigger::Enter,
        highlight: None,
    },
    Step {
        title: "Engine Panel",
        body: "Look at the bottom-left of your screen.\n\nThe engine panel shows:\n  \u{2022} Model name and provider\n  \u{2022} Tier (Victory / Gloriana / Retribution)\n  \u{2022} Thinking level\n  \u{2022} Context capacity",
        anchor: Anchor::Upper,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::EnginePanel),
    },
    Step {
        title: "Inference Instruments",
        body: "The right panels show live telemetry:\n\n  \u{2022} Context bar \u{2014} navy \u{2192} teal \u{2192} amber\n  \u{2022} Glitch chars when the agent thinks\n  \u{2022} Memory strings show fact counts\n  \u{2022} Tool recency \u{2014} most recent at top",
        anchor: Anchor::Upper,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::InstrumentPanel),
    },
    Step {
        title: "Design Sidebar",
        body: "The right panel is your design tree.\n\nIt shows every design node, grouped by status:\n  \u{2022} \u{2699} implementing  \u{25cf} decided\n  \u{2022} \u{25d0} exploring     \u{2715} blocked\n\nPress Ctrl+D to navigate it. Enter focuses\na node into the agent's context.\n\nWatch it update live as work progresses.",
        anchor: Anchor::Center,
        trigger: Trigger::Enter,
        highlight: Some(Highlight::Dashboard),
    },
    // ═══ Act 2 — The Agent Works ═══════════════════════════════
    Step {
        title: "Watch the Agent",
        body: "Now let's see the agent work.\n\nIt will read this project, analyze the code,\nand store facts about its architecture.\n\nWatch the instruments \u{2014} tools will light up\nand memory strings will pulse as facts\nare stored.",
        anchor: Anchor::Upper,
        trigger: Trigger::AutoPrompt(
            "Read the project files in src/ and Cargo.toml. Understand the architecture, then store 3 memory facts about what this project does, its structure, and its test coverage."
        ),
        highlight: Some(Highlight::InstrumentPanel),
    },
    Step {
        title: "Design Exploration",
        body: "The agent will now explore a design question.\n\nWatch the sidebar \u{2014} the node's status will\nchange as the agent adds research and makes\na decision.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Focus on the design node 'output-formatting' and explore it. Read the doc, research the open question about color support, add your findings, and make a decision."
        ),
        highlight: Some(Highlight::Dashboard),
    },
    // ═══ Act 3 — The Lifecycle ═════════════════════════════════
    Step {
        title: "Decomposition",
        body: "This is the showpiece \u{2014} live parallel work.\n\nThe agent will decompose a prepared task into\nparallel branches and execute them simultaneously.\n\nThis uses API credits (~$0.10\u{2013}0.30).\n\n  \u{25b6} Press Tab to start the cleave\n  \u{25b6} Press Esc to skip this step",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Run /assess diff to review the current state, then execute the prepared cleave plan in the openspec change 'add-validation'. Use retribution tier for the children."
        ),
        highlight: None,
    },
    Step {
        title: "Verification",
        body: "The branches have merged.\n\nNow the agent will verify the implementation\nagainst the specs. Watch for pass/fail results.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Run the tests with `cargo test` and report the results. Then summarize what was implemented across the cleave branches."
        ),
        highlight: None,
    },
    // ═══ Act 4 — You're Ready ══════════════════════════════════
    Step {
        title: "Power Tools",
        body: "A few more things to know:\n\n  \u{2022} /focus \u{2014} toggle instrument panels\n  \u{2022} /calibrate \u{2014} adjust display colors\n  \u{2022} /model \u{2014} switch AI models\n  \u{2022} /think \u{2014} adjust reasoning depth\n  \u{2022} Ctrl+D \u{2014} navigate the design tree\n  \u{2022} Ctrl+C \u{d7}2 \u{2014} quit",
        anchor: Anchor::Center,
        trigger: Trigger::Enter,
        highlight: None,
    },
    Step {
        title: "You're Ready!",
        body: "That's Omegon.\n\n  \u{2022} Memory persists across sessions\n  \u{2022} Design tree tracks your architecture\n  \u{2022} OpenSpec enforces spec-before-code\n  \u{2022} /help shows all commands\n  \u{2022} /tutorial replays this guide\n\nGo build something.",
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
    /// Whether the current AutoPrompt step has sent its prompt.
    /// Reset to false when advancing to a new step.
    pub auto_prompt_sent: bool,
}

impl Tutorial {
    pub fn new() -> Self {
        Self {
            current: 0,
            active: true,
            auto_prompt_sent: false,
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
            self.auto_prompt_sent = false;
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
            self.auto_prompt_sent = false;
            true
        } else {
            false
        }
    }

    /// Mark the current AutoPrompt step's prompt as sent.
    pub fn mark_auto_prompt_sent(&mut self) {
        self.auto_prompt_sent = true;
    }

    /// Check if the current step is an AutoPrompt that hasn't been sent yet.
    pub fn pending_auto_prompt(&self) -> Option<&'static str> {
        if !self.active || self.auto_prompt_sent {
            return None;
        }
        if let Trigger::AutoPrompt(prompt) = &self.step().trigger {
            Some(prompt)
        } else {
            None
        }
    }

    /// Called when the agent finishes a turn. If the current step is an
    /// AutoPrompt that was sent, advance to the next step.
    pub fn on_agent_turn_complete(&mut self) {
        if !self.active { return; }
        if self.auto_prompt_sent {
            if let Trigger::AutoPrompt(_) = &self.step().trigger {
                self.advance();
            }
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
            // Steps highlighting dashboard → position in left portion
            (_, Some(Highlight::Dashboard)) => {
                // Dashboard is on the right — put overlay on the left
                let w = area.width.min(48).max(30);
                let h = area.height.saturating_sub(footer_height + 3).min(14);
                let x = area.x + 2;
                let y = area.y + 2;
                Rect { x, y, width: w, height: h }
            }
            // Center for steps with no highlight or Center anchor
            (Anchor::Center, _) => centered_rect(area),
            (Anchor::Upper, _) => upper_rect(area, footer_height),
        };

        // Fill the overlay area with card background — a distinct surface
        // on top of the main bg, guaranteed theme-owned. Uses card_bg rather
        // than bg to provide subtle lift and prevent any terminal default
        // color bleed-through.
        let overlay_bg = theme.card_bg();
        for y in overlay.top()..overlay.bottom() {
            for x in overlay.left()..overlay.right() {
                if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                    cell.reset();
                    cell.set_char(' ');
                    cell.set_bg(overlay_bg);
                    cell.set_fg(theme.fg());
                }
            }
        }

        // Build the call-to-action — prominent line inside the content
        let cta = match &step.trigger {
            Trigger::Enter => "  \u{25b6} Press Tab to continue",
            Trigger::Command("focus") => "  \u{25b6} Type /focus in the input bar below",
            Trigger::Command(cmd) => {
                return self.render_with_cta(overlay, buf, theme, &format!("  \u{25b6} Type /{cmd} in the input bar below"));
            }
            Trigger::AnyInput => "  \u{25b6} Type a message in the input bar below",
            Trigger::AutoPrompt(_) => {
                if self.auto_prompt_sent {
                    "  \u{23f3} Agent is working... watch the instruments"
                } else {
                    "  \u{25b6} Press Tab to start"
                }
            }
        };

        self.render_with_cta(overlay, buf, theme, cta);
    }

    fn render_with_cta(&self, overlay: Rect, buf: &mut Buffer, theme: &dyn super::theme::Theme, cta: &str) {
        let step = self.step();
        let progress = format!(" {}/{} ", self.current + 1, STEPS.len());
        let title_line = format!("\u{1f4d8} {} ", step.title);

        let overlay_bg = theme.card_bg();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent()).bg(overlay_bg))
            .style(Style::default().bg(overlay_bg))
            .title(Span::styled(&title_line, Style::default().fg(theme.accent()).bg(overlay_bg).bold()))
            .title_bottom(
                Line::from(vec![
                    Span::styled(&progress, Style::default().fg(theme.muted()).bg(overlay_bg)),
                    Span::styled("  ", Style::default().bg(overlay_bg)),
                    Span::styled("[Esc skip] [Shift+Tab back]", Style::default().fg(theme.muted()).bg(overlay_bg)),
                ]).right_aligned()
            );

        let inner = block.inner(overlay);
        block.render(overlay, buf);

        // Body text + call-to-action as last line
        let body_with_cta = format!("{}\n\n{}", step.body, cta);
        let text = Paragraph::new(body_with_cta)
            .style(Style::default().fg(theme.fg()).bg(theme.card_bg()))
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
    fn check_command_on_non_command_step() {
        // check_command should be a no-op on non-Command steps
        let mut tut = Tutorial::new();
        let idx = tut.step_index();
        assert!(!tut.check_command("focus"));
        assert_eq!(tut.step_index(), idx); // didn't advance
    }

    #[test]
    fn auto_prompt_lifecycle() {
        let mut tut = Tutorial::new();
        // Find an AutoPrompt step
        while !matches!(tut.step().trigger, Trigger::AutoPrompt(_)) {
            assert!(tut.advance(), "should have an AutoPrompt step");
        }
        let idx = tut.step_index();

        // Should have pending auto-prompt
        assert!(tut.pending_auto_prompt().is_some());
        assert!(!tut.auto_prompt_sent);

        // Mark as sent
        tut.mark_auto_prompt_sent();
        assert!(tut.auto_prompt_sent);
        assert!(tut.pending_auto_prompt().is_none());

        // Agent turn complete should advance
        tut.on_agent_turn_complete();
        assert_eq!(tut.step_index(), idx + 1);
        assert!(!tut.auto_prompt_sent); // reset for new step
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
