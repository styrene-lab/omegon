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
    Tab,
    /// Wait for a specific slash command (e.g. "/focus").
    Command(&'static str),
    /// Wait for any user message to be sent.
    AnyInput,
    /// Auto-send a prompt to the agent, wait for the turn to complete.
    /// The overlay shows a "watching..." indicator while the agent works.
    AutoPrompt(&'static str),
}

/// Side effect to fire when a tutorial step is entered.
#[derive(Debug, Clone, PartialEq)]
pub enum SideEffect {
    /// No side effect.
    None,
    /// Open the web dashboard in the browser.
    OpenDashboard,
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
    /// Side effect to fire when this step is entered.
    pub on_enter: SideEffect,
}

/// A TUI region to visually highlight during a tutorial step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Highlight {
    InstrumentPanel,
    EnginePanel,
    InputBar,
    Dashboard,
}

// ─── Shared Act 1 steps (cockpit tour — same for both modes) ────────────────

const STEP_WELCOME_DEMO: Step = Step {
    title: "Welcome to the Omegon Demo",
    body: "You\u{2019}re looking at a broken sprint board \u{2014}\na browser-based task tracker with 4 bugs:\n\n  \u{2022} Wrong task count\n  \u{2022} Done column always empty\n  \u{2022} Add Task reloads the page\n  \u{2022} Changes lost on refresh\n\nYou\u{2019}re about to watch the agent read the code,\nwrite a fix plan, then fix all 4 bugs\nat the same time in separate branches.\n\nTotal time: about 3 minutes.",
    anchor: Anchor::Center,
    trigger: Trigger::Tab,
    highlight: None,
    on_enter: SideEffect::None,
};

const STEP_WELCOME_HANDS_ON: Step = Step {
    title: "Welcome to Omegon",
    body: "This is Omegon \u{2014} a systems engineering\nagent.\n\nIt works in your terminal, remembers your\nproject across sessions, and can split work\ninto parallel branches.\n\nThis tour uses YOUR project. About 5 minutes.\nThe agent will read your code and do real work.",
    anchor: Anchor::Center,
    trigger: Trigger::Tab,
    highlight: None,
    on_enter: SideEffect::None,
};

const STEP_COCKPIT: Step = Step {
    title: "Your Cockpit",
    body: "Quick orientation:\n\n  Bottom-left \u{2014} model, speed, context\n  Bottom-center \u{2014} live activity display\n  Right panel \u{2014} design notes & decisions\n\nThese all update live while the agent works.\nYou\u{2019}ll see them light up in the next step.",
    anchor: Anchor::Upper,
    trigger: Trigger::Tab,
    highlight: Some(Highlight::InstrumentPanel),
    on_enter: SideEffect::None,
};

const STEP_WEB_DASHBOARD: Step = Step {
    title: "Web Dashboard",
    body: "This just opened in your browser \u{2014}\nthe live web dashboard.\n\nIt shows everything from the right panel\nin a full web page: design notes, specs,\nand a real-time feed of what\u{2019}s happening.\n\nSame data, no polling, instant updates.\n\nPress Tab to continue.",
    anchor: Anchor::Center,
    trigger: Trigger::Tab,
    highlight: None,
    on_enter: SideEffect::OpenDashboard,
};

// ─── Demo mode STEPS (pre-seeded project, specific content) ─────────────────

/// Steps for the guided demo mode — run inside the bundled demo project.
/// References specific pre-seeded artifacts: search-filter design node,
/// fix-board-bugs OpenSpec change with 4-branch tasks.md.
pub const STEPS_DEMO: &[Step] = &[
    // Act 1 — Quick orientation (1 passive step, then action)
    STEP_WELCOME_DEMO,
    STEP_COCKPIT,

    // Act 2 — The Agent Works (watch the agent work)
    Step {
        title: "Reading the Code",
        body: "The agent is about to read the broken app\nand figure out what\u{2019}s wrong.\n\nWatch the bottom panels light up:\n  \u{2022} Tool names appear as files are read\n  \u{2022} The right panel loads design notes\n  \u{2022} Memory facts get stored for next time\n\nThis takes about 30 seconds.",
        anchor: Anchor::Upper,
        trigger: Trigger::AutoPrompt(
            "Read this project. Start with README.md to understand the context, then read \
src/board.js carefully — pay attention to the four BUG comments. \
Read index.html to understand the HTML structure. \
Then store 1 additional memory fact: summarize the relationship between the four bugs \
and why they must be fixed in separate branches (no file conflicts between fixes). \
Finally, confirm what each bug does to the user experience."
        ),
        highlight: Some(Highlight::InstrumentPanel),
    on_enter: SideEffect::None,
    },
    Step {
        title: "Making a Design Decision",
        body: "The right panel tracks design decisions.\n\nThere\u{2019}s an open question about the search\nfeature: should it match partial words\nor require exact matches?\n\nWatch the agent research this, weigh the\noptions, and record a decision.\nThe sidebar will update live.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Use design_tree with action 'node' and node_id 'search-filter' to read the design doc. \
It has an open question: should search be fuzzy (partial substring) or exact (whole word)? \
Research this: consider the user experience for a task list (most users type partial words), \
how browsers handle input events for live filtering, and what 'Array.filter + String.includes' vs \
'fuzzy-match' implementations look like. \
Then use design_tree_update with action 'add_research' to record your findings, \
and action 'add_decision' to record a decision with clear rationale."
        ),
        highlight: Some(Highlight::Dashboard),
    on_enter: SideEffect::None,
    },

    // Act 3 — The Fix (spec → parallel fix → verify)
    Step {
        title: "The Fix Plan",
        body: "Before writing any code, the agent wrote a\nspec \u{2014} a checklist of what each fix must do.\n\nNow it will explain the plan:\n  \u{2022} 4 bugs, 4 separate fix branches\n  \u{2022} Each branch touches one function\n  \u{2022} They all run at the same time\n\nAfter this, YOU\u{2019}ll run the fix.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Use openspec_manage with action 'get' and change_name 'fix-board-bugs' to read the full \
change. Then read ai/openspec/changes/fix-board-bugs/tasks.md using the bash tool. \
Explain clearly and concisely: \
(1) what the spec scenarios require for each of the 4 bugs, \
(2) which function in src/board.js each task touches, \
(3) why these 4 tasks can safely run as parallel branches (no file conflicts), \
(4) what the user will see in their browser once all fixes are applied, \
(5) the exact command to execute: /cleave fix-board-bugs"
        ),
        highlight: None,
    on_enter: SideEffect::None,
    },
    Step {
        title: "Fix All 4 Bugs",
        body: "Time to fix everything. Type this command:\n\n  /cleave fix-board-bugs\n\nThis will create 4 branches, fix one bug\nin each, and merge them all back together.\n\nWatch the activity panel \u{2014} you\u{2019}ll see all\n4 branches working at the same time.\nTakes about 60 seconds.\n\nIf something goes wrong, type /tutorial\nto come back here.",
        anchor: Anchor::Center,
        trigger: Trigger::Command("cleave"),
        highlight: Some(Highlight::InstrumentPanel),
    on_enter: SideEffect::None,
    },

    // Act 4 — Verify, celebrate, explore
    Step {
        title: "Verify and Launch",
        body: "The fixes should be merged. Now the agent\nwill check each fix and open the working\nsprint board in your browser.\n\nYou\u{2019}ll see the result yourself \u{2014}\ntasks count correctly, Done column works,\nAdd Task works, data persists on refresh.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "The cleave should have fixed four bugs in src/board.js. Verify each fix: \
(1) read src/board.js using the bash tool: confirm getTotalCount returns tasks.length only (not DEFAULT_TASKS.length + tasks.length), \
(2) confirm getTasksByStatus maps Done to 'done' (lowercase) not 'Done', \
(3) confirm handleAddTask calls event.preventDefault(), \
(4) confirm addTask and updateTaskStatus both call saveTasks(). \
Then open the sprint board in the default browser: \
run bash command 'open ./index.html 2>/dev/null || xdg-open ./index.html 2>/dev/null || echo \"Open index.html in your browser to see the fixed sprint board\"'. \
Finally, briefly explain what was fixed and that the user can now use the board."
        ),
        highlight: None,
    on_enter: SideEffect::None,
    },
    STEP_WEB_DASHBOARD,
    Step {
        title: "What Just Happened",
        body: "You watched the full workflow:\n\n  1. Agent read the code and stored facts\n  2. Agent made a design decision\n  3. Specs defined what each fix must do\n  4. 4 branches fixed 4 bugs in parallel\n  5. Fixes verified, app opened in browser\n\nThis same workflow works on YOUR project.\nType /help to see all commands.\n/tutorial to replay this demo.\nCtrl+C twice to quit.",
        anchor: Anchor::Center,
        trigger: Trigger::Tab,
        highlight: None,
    on_enter: SideEffect::None,
    },
];

// ─── Hands-on mode STEPS (user's own project, adaptive) ─────────────────────

/// Steps for hands-on mode — run in the operator's own project.
/// Prompts are adaptive: they work in any project, gracefully handle
/// empty design trees, and create real value (facts, nodes, specs).
pub const STEPS_HANDS_ON: &[Step] = &[
    // Act 1 — Quick orientation
    STEP_WELCOME_HANDS_ON,
    STEP_COCKPIT,

    // Act 2 — The Agent Works on YOUR project
    Step {
        title: "Reading Your Code",
        body: "The agent is about to read your project and\nremember what it learns.\n\nThese memories persist \u{2014} next time you\nopen Omegon here, it already knows\nyour codebase. No re-explaining.\n\nWatch the bottom panels light up as\nit reads files and stores facts.\nAbout 30 seconds.",
        anchor: Anchor::Upper,
        trigger: Trigger::AutoPrompt(
            "Read this project. Look for whatever source files exist: src/, lib.rs, main.rs, \
Cargo.toml, package.json, pyproject.toml, go.mod, README.md — whatever is here. \
Understand what it does and how it\u{2019}s structured. \
Then store exactly 3 memory facts using memory_store: \
(1) what this project does and its primary purpose, \
(2) the key code structure (main modules, important files, language/framework), \
(3) testing practices and coverage (or lack thereof). \
Be specific — these facts will be loaded in future sessions."
        ),
        highlight: Some(Highlight::InstrumentPanel),
    on_enter: SideEffect::None,
    },
    Step {
        title: "Design Notes",
        body: "The right panel tracks design decisions.\n\nThe agent will check if you have any existing\nnotes. If not, it will create a first one \u{2014}\nan architecture overview of YOUR project.\n\nThink of it as a living design doc that\nthe agent and you both maintain.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Use design_tree with action 'list' to see what design nodes exist. \
If there are nodes with open questions, pick the most interesting one and explore it: \
use action 'node' to read it, add research with your findings, then add a decision. \
If the design tree is empty, create a first node using action 'create' with: \
  - node_id: a short slug based on the project name or its main concern \
  - title: a clear description of the project\u{2019}s main architectural decision \
  - overview: 2-3 sentences about the architecture based on what you read \
  - status: 'exploring' \
Explain what you did and why this node matters for the project."
        ),
        highlight: Some(Highlight::Dashboard),
    on_enter: SideEffect::None,
    },

    // Act 3 — Spec before code
    Step {
        title: "Writing a Spec",
        body: "Before changing any code, the agent writes\na spec \u{2014} a checklist of what the change\nmust do, written as simple scenarios:\n\n  Given [setup]\n  When [action]\n  Then [expected result]\n\nThe agent will now propose a real improvement\nfor YOUR project and write a spec for it.",
        anchor: Anchor::Center,
        trigger: Trigger::AutoPrompt(
            "Based on what you read about this project, identify ONE concrete, valuable improvement. \
Something specific that would actually help: better error handling, missing validation, \
a test gap, a missing feature. \
Use openspec_manage with action 'propose' to create a change (pick a descriptive slug). \
Then use action 'generate_spec' to write Given/When/Then scenarios for it \
(domain name can match the slug). \
Keep it focused: one clear requirement, 2-3 scenarios. \
This creates a real ai/openspec/ entry in your project."
        ),
        highlight: None,
    on_enter: SideEffect::None,
    },
    STEP_WEB_DASHBOARD,
    Step {
        title: "What\u{2019}s Next",
        body: "You\u{2019}ve seen the core workflow:\n\n  1. Agent reads code and remembers it\n  2. Design notes track decisions\n  3. Specs define what changes must do\n\nThe last piece: /cleave splits a spec\ninto parallel branches and fixes them\nall at once. To see that in action on\na prepared demo project:\n  /tutorial demo\n\nOr just start asking it to help with\nyour code. /help for all commands.\nCtrl+C twice to quit.",
        anchor: Anchor::Center,
        trigger: Trigger::Tab,
        highlight: None,
    on_enter: SideEffect::None,
    },
];



/// Which option is highlighted in the project-choice widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TutorialChoice {
    Demo,
    MyProject,
}

/// Tutorial overlay state.
pub struct Tutorial {
    /// Current step index.
    current: usize,
    /// Whether the tutorial is active (visible).
    pub active: bool,
    /// Whether the current AutoPrompt step has sent its prompt.
    /// Reset to false when advancing to a new step.
    pub auto_prompt_sent: bool,
    /// Whether the project has pre-existing design tree content.
    /// When false, step 0 shows a project-choice widget instead of
    /// the normal passive welcome.
    pub has_design_tree: bool,
    /// True when running inside the bundled demo project (--tutorial flag).
    /// Selects STEPS_DEMO instead of STEPS_HANDS_ON.
    pub is_demo: bool,
    /// Current selection in the project-choice widget (step 0, empty project).
    pub choice: TutorialChoice,
    /// Set to true when the operator confirms a choice — caller reads
    /// `choice` to know which path to take, then advances the tutorial.
    pub choice_confirmed: bool,
}

impl Tutorial {
    pub fn new() -> Self {
        Self::with_context(false)
    }

    /// Create a tutorial with project context so steps can adapt.
    pub fn with_context(has_design_tree: bool) -> Self {
        Self::new_mode(has_design_tree, false)
    }

    /// Create a demo-mode tutorial (--tutorial flag, bundled project).
    pub fn new_demo(has_design_tree: bool) -> Self {
        Self::new_mode(has_design_tree, true)
    }

    fn new_mode(has_design_tree: bool, is_demo: bool) -> Self {
        Self {
            current: 0,
            active: true,
            auto_prompt_sent: false,
            has_design_tree,
            is_demo,
            choice: TutorialChoice::Demo,
            choice_confirmed: false,
        }
    }

    /// The active steps array — STEPS_DEMO in demo mode, STEPS_HANDS_ON otherwise.
    pub fn steps(&self) -> &'static [Step] {
        if self.is_demo { STEPS_DEMO } else { STEPS_HANDS_ON }
    }

    /// Whether the project-choice widget is active (step 0, empty project).
    pub fn showing_choice(&self) -> bool {
        self.current == 0 && !self.has_design_tree && !self.choice_confirmed && !self.is_demo
    }

    /// Toggle the choice selection left/right.
    pub fn toggle_choice(&mut self) {
        self.choice = match self.choice {
            TutorialChoice::Demo => TutorialChoice::MyProject,
            TutorialChoice::MyProject => TutorialChoice::Demo,
        };
    }

    /// Confirm the current choice. Caller should read `self.choice`
    /// and act accordingly (launch demo or advance normally).
    pub fn confirm_choice(&mut self) {
        self.choice_confirmed = true;
    }

    pub fn step(&self) -> &Step {
        &self.steps()[self.current]
    }

    pub fn step_index(&self) -> usize {
        self.current
    }

    pub fn total_steps(&self) -> usize {
        self.steps().len()
    }

    /// Advance to the next step. Returns false if already at the end.
    pub fn advance(&mut self) -> bool {
        if self.current < self.steps().len() - 1 {
            self.current += 1;
            self.auto_prompt_sent = false;
            true
        } else {
            self.active = false;
            false
        }
    }

    /// Check if the current step has a side effect that should fire on entry.
    pub fn pending_side_effect(&self) -> SideEffect {
        if !self.active { return SideEffect::None; }
        self.steps().get(self.current)
            .map(|s| s.on_enter.clone())
            .unwrap_or(SideEffect::None)
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
    pub fn on_agent_turn_complete(&mut self) -> SideEffect {
        if !self.active { return SideEffect::None; }
        if self.auto_prompt_sent {
            if let Trigger::AutoPrompt(_) = &self.step().trigger {
                self.advance();
                return self.pending_side_effect();
            }
        }
        SideEffect::None
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

    /// Check if Tab was pressed and the current step accepts it.
    pub fn check_enter(&mut self) -> bool {
        if !self.active { return false; }
        if self.step().trigger == Trigger::Tab {
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

        // Step 0 on an empty project shows a project-choice widget
        // instead of the normal passive welcome step.
        if self.showing_choice() {
            self.render_choice(area, buf, theme, footer_height);
            return;
        }

        let step = self.step();

        // Smart positioning: avoid covering highlighted areas.
        // AutoPrompt steps that are running get a larger overlay to cover
        // the conversation chaos while the agent works behind the scenes.
        let auto_prompt_active = self.auto_prompt_sent
            && matches!(step.trigger, Trigger::AutoPrompt(_));

        let overlay = if auto_prompt_active {
            // Agent is working — large centered overlay covers the conversation
            large_centered_rect(area, footer_height)
        } else {
            match (&step.anchor, &step.highlight) {
                // Steps highlighting footer elements → position in upper area
                (_, Some(Highlight::EnginePanel | Highlight::InstrumentPanel)) => {
                    upper_rect(area, footer_height)
                }
                // Steps highlighting input → position in center-upper (above input bar)
                (_, Some(Highlight::InputBar)) => {
                    upper_rect(area, footer_height + 3) // extra 3 for input bar
                }
                // Steps highlighting dashboard → center in the conversation area.
                // Dashboard is ~40 cols wide on the right; overlay lives in the
                // conversation zone to its left, visually paired with the sidebar.
                (_, Some(Highlight::Dashboard)) => {
                    let dash_width: u16 = 40;
                    let conv_width = area.width.saturating_sub(dash_width);
                    let w = 50u16.min(conv_width.saturating_sub(4));
                    let h = area.height.saturating_sub(footer_height + 4).min(16);
                    let x = area.x + (conv_width.saturating_sub(w)) / 2;
                    let y = area.y + 2;
                    Rect { x, y, width: w, height: h }
                }
                // Center for steps with no highlight or Center anchor
                (Anchor::Center, _) => centered_rect(area),
                (Anchor::Upper, _) => upper_rect(area, footer_height),
            }
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
            Trigger::Tab => "  \u{25b6} Press Tab to continue",
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
        let progress = format!(" {}/{} ", self.current + 1, self.steps().len());
        let title_line = format!("\u{1f4d8} {} ", step.title);

        let overlay_bg = theme.card_bg();

        // Forward key hint — only shown when Tab actually does something
        let forward_hint = match &step.trigger {
            Trigger::AutoPrompt(_) if self.auto_prompt_sent => "",  // waiting for agent
            _ => "[Tab]",
        };

        let forward_label = format!(" {forward_hint} ");
        let progress_label = format!("  {progress}");
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent()).bg(overlay_bg))
            .style(Style::default().bg(overlay_bg))
            .title(Span::styled(&title_line, Style::default().fg(theme.accent()).bg(overlay_bg).bold()))
            .title_bottom(
                Line::from(vec![
                    Span::styled(
                        forward_label.as_str(),
                        Style::default().fg(theme.accent_bright()).bg(overlay_bg).bold()
                    ),
                    Span::styled(" [Esc] skip  [\u{21e7}Tab] back", Style::default().fg(theme.muted()).bg(overlay_bg)),
                    Span::styled(progress_label.as_str(), Style::default().fg(theme.muted()).bg(overlay_bg)),
                ])
            );

        let inner = block.inner(overlay);
        block.render(overlay, buf);

        // Body text + call-to-action as last line
        let body_with_cta = format!("{}\n\n{}", step.body, cta);
        let text = Paragraph::new(body_with_cta)
            .style(Style::default().fg(theme.fg()).bg(theme.card_bg()))
            .wrap(Wrap { trim: false });
        text.render(inner, buf);

        // Highlight the CTA line by coloring it accent.
        // Scan upward from the bottom to find the first non-blank line —
        // that's where the CTA actually rendered (not always the last row
        // since wrapping can shift content upward in narrow terminals).
        let mut cta_y = None;
        for y in (inner.y..inner.bottom()).rev() {
            let has_content = (inner.x..inner.right()).any(|x| {
                buf.cell(ratatui::prelude::Position::new(x, y))
                    .is_some_and(|c| c.symbol() != " ")
            });
            if has_content {
                cta_y = Some(y);
                break;
            }
        }
        if let Some(y) = cta_y {
            for x in inner.x..inner.right() {
                if let Some(cell) = buf.cell_mut(ratatui::prelude::Position::new(x, y)) {
                    if cell.symbol() != " " {
                        cell.set_fg(theme.accent_bright());
                    }
                }
            }
        }
    }

    fn render_choice(&self, area: Rect, buf: &mut Buffer, theme: &dyn super::theme::Theme, footer_height: u16) {
        let overlay = large_centered_rect(area, footer_height);
        let overlay_bg = theme.card_bg();

        // Fill background
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

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent()).bg(overlay_bg))
            .style(Style::default().bg(overlay_bg))
            .title(Span::styled(
                "\u{1f4d8} Welcome to Omegon ",
                Style::default().fg(theme.accent()).bg(overlay_bg).bold()
            ))
            .title_bottom(
                Line::from(vec![
                    Span::styled(
                        " [\u{2190}/\u{2192}] select  [Tab] confirm  [Esc] exit ",
                        Style::default().fg(theme.muted()).bg(overlay_bg)
                    ),
                ]).centered()
            );

        let inner = block.inner(overlay);
        block.render(overlay, buf);

        // Layout: intro text on top, two side-by-side option boxes below
        use ratatui::layout::{Direction, Layout, Constraint};
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // intro text
                Constraint::Min(6),    // option boxes
            ])
            .split(inner);

        // Intro line
        let intro = Paragraph::new("Choose a tutorial mode:")
            .style(Style::default().fg(theme.muted()).bg(overlay_bg));
        intro.render(rows[0], buf);

        // Two option columns
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(rows[1]);

        self.render_choice_option(
            cols[0], buf, theme, overlay_bg,
            TutorialChoice::Demo,
            "\u{1f4e6} Guided demo",
            "Sprint board web app with\n4 visible bugs to fix.\nFull lifecycle: read \u{2192}\ndesign \u{2192} spec \u{2192} cleave.\n\n~$0.10\u{2013}0.20 in tokens",
        );
        self.render_choice_option(
            cols[1], buf, theme, overlay_bg,
            TutorialChoice::MyProject,
            "\u{1f528} My project",
            "Use your current code.\nReads your files, stores\nfacts, creates your first\ndesign node + spec.\n\n~$0.05 in tokens",
        );
    }

    fn render_choice_option(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &dyn super::theme::Theme,
        overlay_bg: Color,
        option: TutorialChoice,
        title: &str,
        body: &str,
    ) {
        let selected = self.choice == option;
        let (border_style, title_style) = if selected {
            (
                Style::default().fg(theme.accent_bright()).bg(overlay_bg),
                Style::default().fg(theme.accent_bright()).bg(overlay_bg).bold(),
            )
        } else {
            (
                Style::default().fg(theme.muted()).bg(overlay_bg),
                Style::default().fg(theme.fg()).bg(overlay_bg),
            )
        };

        // Shrink area slightly for padding between options
        let padded = Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: area.height,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(format!(" {title} "), title_style));

        let inner = block.inner(padded);
        block.render(padded, buf);

        Paragraph::new(body)
            .style(Style::default().fg(if selected { theme.fg() } else { theme.muted() }).bg(overlay_bg))
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }
}

/// Large centered rect — covers most of the conversation area while the
/// agent works during AutoPrompt steps. Leaves footer visible.
fn large_centered_rect(parent: Rect, footer_height: u16) -> Rect {
    let available_h = parent.height.saturating_sub(footer_height + 2);
    let w = 60u16.min(parent.width.saturating_sub(4));
    let h = available_h.min(18);
    let x = parent.x + (parent.width.saturating_sub(w)) / 2;
    let y = parent.y + (available_h.saturating_sub(h)) / 3;
    Rect::new(x, y, w, h)
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
        for _ in 0..tut.steps().len() - 1 {
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
        // Step 0 has Trigger::Tab
        assert!(tut.check_enter());
        assert_eq!(tut.step_index(), 1);
    }

    #[test]
    fn check_enter_on_command_step_does_nothing() {
        let mut tut = Tutorial::new();
        // Advance to a Command trigger step
        while tut.step().trigger == Trigger::Tab {
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
        for (i, step) in STEPS_HANDS_ON.iter().enumerate() {
            assert!(!step.title.is_empty(), "STEPS_HANDS_ON step {i} has empty title");
            assert!(!step.body.is_empty(), "STEPS_HANDS_ON step {i} has empty body");
        }
        for (i, step) in STEPS_DEMO.iter().enumerate() {
            assert!(!step.title.is_empty(), "STEPS_DEMO step {i} has empty title");
            assert!(!step.body.is_empty(), "STEPS_DEMO step {i} has empty body");
        }
    }

    #[test]
    fn demo_mode_uses_demo_steps() {
        let tut = Tutorial::new_demo(true);
        assert!(tut.is_demo);
        assert_eq!(tut.steps().len(), STEPS_DEMO.len());
        // Demo steps do NOT send user to /tutorial demo — they are in the demo.
        // Hands-on steps MAY reference /tutorial demo (it's the upsell).
        for step in STEPS_DEMO {
            assert!(!step.body.contains("/tutorial demo"),
                "STEPS_DEMO step '{}' tells user to run /tutorial demo — they're already in it",
                step.title);
        }
        // Verify demo steps before the wrapup don't reference "YOUR project"
        // (the final wrapup step may say "try this on YOUR project" as a CTA)
        let wrapup_idx = STEPS_DEMO.iter().position(|s| s.title == "What Just Happened").unwrap();
        for (i, step) in STEPS_DEMO.iter().enumerate() {
            if i >= wrapup_idx { break; }
            assert!(!step.body.contains("YOUR project"),
                "STEPS_DEMO step '{}' references 'YOUR project' before wrapup — demo uses its own project",
                step.title);
        }
    }

    #[test]
    fn hands_on_mode_uses_hands_on_steps() {
        let tut = Tutorial::with_context(false);
        assert!(!tut.is_demo);
        assert_eq!(tut.steps().len(), STEPS_HANDS_ON.len());
    }

    #[test]
    fn hands_on_steps_order_is_correct() {
        // Verify key narrative beats in hands-on mode
        let read_idx = STEPS_HANDS_ON.iter().position(|s| s.title == "Reading Your Code");
        let design_idx = STEPS_HANDS_ON.iter().position(|s| s.title == "Design Notes");
        let spec_idx = STEPS_HANDS_ON.iter().position(|s| s.title == "Writing a Spec");
        assert!(read_idx.is_some(), "Reading Your Code step missing from STEPS_HANDS_ON");
        assert!(design_idx.is_some(), "Design Notes step missing from STEPS_HANDS_ON");
        assert!(spec_idx.is_some(), "Writing a Spec step missing from STEPS_HANDS_ON");
        assert!(
            read_idx.unwrap() < design_idx.unwrap(),
            "Reading Your Code must come before Design Notes in STEPS_HANDS_ON"
        );
        assert!(
            design_idx.unwrap() < spec_idx.unwrap(),
            "Design Notes must come before Writing a Spec in STEPS_HANDS_ON"
        );
    }

    #[test]
    fn demo_steps_order_is_correct() {
        // Verify step order: key narrative beats appear in the right sequence
        let verify_idx = STEPS_DEMO.iter().position(|s| s.title == "Verify and Launch");
        let wrapup_idx = STEPS_DEMO.iter().position(|s| s.title == "What Just Happened");
        let fix_idx = STEPS_DEMO.iter().position(|s| s.title == "Fix All 4 Bugs");
        assert!(fix_idx.is_some(), "Fix All 4 Bugs step missing from STEPS_DEMO");
        assert!(verify_idx.is_some(), "Verify and Launch step missing from STEPS_DEMO");
        assert!(wrapup_idx.is_some(), "What Just Happened step missing from STEPS_DEMO");
        assert!(
            fix_idx.unwrap() < verify_idx.unwrap(),
            "Fix All 4 Bugs ({}) must come before Verify and Launch ({}) in STEPS_DEMO",
            fix_idx.unwrap(), verify_idx.unwrap()
        );
        assert!(
            verify_idx.unwrap() < wrapup_idx.unwrap(),
            "Verify and Launch ({}) must come before What Just Happened ({}) in STEPS_DEMO",
            verify_idx.unwrap(), wrapup_idx.unwrap()
        );
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

    // ── STEPS_DEMO coverage ──

    #[test]
    fn demo_go_back() {
        let mut tut = Tutorial::new_demo(true);
        assert!(!tut.go_back()); // can't go back from 0
        tut.advance();
        assert!(tut.go_back());
        assert_eq!(tut.step_index(), 0);
    }

    #[test]
    fn demo_dismiss() {
        let mut tut = Tutorial::new_demo(true);
        tut.dismiss();
        assert!(!tut.active);
    }

    #[test]
    fn demo_check_enter_on_enter_step() {
        let mut tut = Tutorial::new_demo(true);
        // Find first Enter-triggered step in demo
        while tut.step().trigger != Trigger::Tab {
            tut.advance();
        }
        let idx = tut.step_index();
        assert!(tut.check_enter());
        assert_eq!(tut.step_index(), idx + 1);
    }

    #[test]
    fn demo_check_enter_on_auto_prompt_step_does_nothing() {
        let mut tut = Tutorial::new_demo(true);
        // Find an AutoPrompt step
        while !matches!(tut.step().trigger, Trigger::AutoPrompt(_)) {
            tut.advance();
        }
        let idx = tut.step_index();
        assert!(!tut.check_enter());
        assert_eq!(tut.step_index(), idx); // didn't advance
    }

    #[test]
    fn demo_auto_prompt_lifecycle() {
        let mut tut = Tutorial::new_demo(true);
        // Find an AutoPrompt step
        while !matches!(tut.step().trigger, Trigger::AutoPrompt(_)) {
            tut.advance();
        }
        let idx = tut.step_index();
        // Should have a pending prompt
        let prompt = tut.pending_auto_prompt();
        assert!(prompt.is_some(), "AutoPrompt step should yield a prompt");
        assert!(!tut.auto_prompt_sent);

        // Mark sent
        tut.mark_auto_prompt_sent();
        assert!(tut.auto_prompt_sent);
        assert!(tut.pending_auto_prompt().is_none());

        // Agent turn complete should advance
        tut.on_agent_turn_complete();
        assert_eq!(tut.step_index(), idx + 1);
        assert!(!tut.auto_prompt_sent); // reset for new step
    }

    #[test]
    fn demo_inactive_does_not_consume_input() {
        let mut tut = Tutorial::new_demo(true);
        tut.dismiss();
        assert!(!tut.check_enter());
        assert!(!tut.check_any_input());
        assert!(tut.current_highlight().is_none());
    }

    #[test]
    fn demo_advance_past_last_step_dismisses() {
        let mut tut = Tutorial::new_demo(true);
        let total = tut.steps().len();
        for _ in 0..total {
            tut.advance();
        }
        assert!(!tut.active, "advancing past last step should dismiss tutorial");
    }

    #[test]
    fn demo_all_auto_prompts_are_non_empty() {
        for step in STEPS_DEMO {
            if let Trigger::AutoPrompt(prompt) = step.trigger {
                assert!(!prompt.is_empty(),
                    "STEPS_DEMO step '{}' has empty auto-prompt", step.title);
            }
        }
    }

    #[test]
    fn command_step_allows_check_command_to_advance() {
        // Use demo mode which has Command("cleave") step
        let mut tut = Tutorial::new_demo(true);
        // Find a Command-triggered step
        while !matches!(tut.step().trigger, Trigger::Command(_)) {
            assert!(tut.advance(), "should have a Command step in STEPS_DEMO");
        }
        let idx = tut.step_index();
        let expected_cmd = if let Trigger::Command(cmd) = tut.step().trigger { cmd } else { unreachable!() };

        // Wrong command doesn't advance
        assert!(!tut.check_command("nonexistent"));
        assert_eq!(tut.step_index(), idx);

        // Correct command advances
        assert!(tut.check_command(expected_cmd));
        assert_eq!(tut.step_index(), idx + 1);
    }

    #[test]
    fn demo_command_step_allows_check_command_to_advance() {
        // STEPS_DEMO has "Fix All 4 Bugs" with Trigger::Command("cleave")
        let mut tut = Tutorial::new_demo(true);
        while !matches!(tut.step().trigger, Trigger::Command(_)) {
            tut.advance();
        }
        let idx = tut.step_index();
        assert_eq!(tut.step().title, "Fix All 4 Bugs");

        // Wrong command doesn't advance
        assert!(!tut.check_command("focus"));
        assert_eq!(tut.step_index(), idx);

        // /cleave advances
        assert!(tut.check_command("cleave"));
        assert_eq!(tut.step_index(), idx + 1);
    }

    #[test]
    fn enter_does_not_advance_command_step() {
        // Verifies that Enter (Tab) doesn't skip Command steps — the user
        // must actually type the command. This is the input-passthrough guarantee.
        let mut tut = Tutorial::new_demo(true);
        while !matches!(tut.step().trigger, Trigger::Command(_)) {
            tut.advance();
        }
        let idx = tut.step_index();
        assert!(!tut.check_enter());
        assert_eq!(tut.step_index(), idx);
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
