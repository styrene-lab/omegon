//! Interactive TUI — ratatui-based terminal interface.
//!
//! Minimum viable interactive agent:
//! - Editor: single-line text input with line editing
//! - Conversation: scrollable message display with streaming
//! - Ctrl+C: cancel during execution, exit at editor
//!
//! The TUI runs in a separate tokio task from the agent loop.
//! They communicate via channels:
//!   - user_input_tx → agent loop receives prompts
//!   - AgentEvent broadcast → TUI receives streaming updates

pub mod bootstrap;
pub mod conversation;
pub mod conv_widget;
pub mod dashboard;
pub mod image;
pub mod editor;
pub mod effects;
pub mod footer;
pub mod instruments;
pub mod segments;
pub mod selector;
pub mod spinner;
pub mod splash;
pub mod theme;
pub mod tutorial;
pub mod widgets;


#[cfg(test)]
mod tests;
#[cfg(test)]
mod snapshot_tests;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use crossterm::event::{EnableMouseCapture, DisableMouseCapture};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use omegon_traits::AgentEvent;

use self::conversation::ConversationView;
use self::dashboard::DashboardState;
use self::segments::Segment;
use self::footer::FooterData;
use self::editor::Editor;
use self::instruments::InstrumentPanel;

/// Messages from TUI to the agent coordinator.
#[derive(Debug)]
pub enum TuiCommand {
    /// User submitted a prompt with optional image attachments.
    UserPrompt(String),
    /// User submitted a prompt with image attachments (paths).
    UserPromptWithImages(String, Vec<std::path::PathBuf>),
    /// User wants to quit (double Ctrl+C, or /exit).
    Quit,
    /// Switch the model for the next turn.
    SetModel(String),
    /// Dispatch a bus command from a feature (name, args).
    BusCommand { name: String, args: String },
    /// Trigger manual compaction.
    Compact,
    /// List saved sessions.
    ListSessions,
    /// Start the web dashboard server.
    StartWebDashboard,
}

/// Shared cancel token — the TUI writes it on Escape/Ctrl+C,
/// the agent loop checks it. Arc so both tasks can access it.
pub type SharedCancel = std::sync::Arc<std::sync::Mutex<Option<CancellationToken>>>;

/// Application state for the TUI.
pub struct App {
    editor: Editor,
    conversation: ConversationView,
    agent_active: bool,
    should_quit: bool,
    turn: u32,
    tool_calls: u32,
    /// Previous tool_calls count — used to compute delta for instrument telemetry
    prev_tool_calls: u32,
    /// Memory operations this frame — drives memory instrument
    memory_ops_this_frame: u32,
    history: Vec<String>,
    history_idx: Option<usize>,
    dashboard: DashboardState,
    footer_data: FooterData,
    /// CIC instrument panel for telemetry visualization
    instrument_panel: InstrumentPanel,
    /// Focus mode toggle state
    focus_mode: bool,
    theme: Box<dyn theme::Theme>,
    /// Shared settings — source of truth for model, thinking, etc.
    settings: crate::settings::SharedSettings,
    /// Shared cancel token — Escape/Ctrl+C cancels the active agent turn.
    cancel: SharedCancel,
    /// Timestamp of last Ctrl+C (for double-tap quit detection).
    last_ctrl_c: Option<std::time::Instant>,
    /// Session start time for /stats.
    session_start: std::time::Instant,
    /// Active selector popup (model picker, think level, etc.)
    selector: Option<selector::Selector>,
    /// What the selector is for — determines what happens on confirm.
    selector_kind: Option<SelectorKind>,
    /// Last tool name from ToolStart — used to track memory mutations.
    last_tool_name: Option<String>,
    /// Tool name that completed this frame — consumed by instrument telemetry
    completed_tool_name: Option<String>,
    /// Current spinner verb — rotates on each tool call.
    working_verb: &'static str,
    /// When true, replay the splash animation.
    replay_splash: bool,
    /// Plugin registry — manages active persona, tone, and memory layers.
    plugin_registry: Option<crate::plugins::registry::PluginRegistry>,
    /// Visual effects manager (tachyonfx).
    effects: effects::Effects,
    /// Command definitions from bus features.
    bus_commands: Vec<omegon_traits::CommandDefinition>,
    /// Shared handles for live dashboard updates.
    dashboard_handles: dashboard::DashboardHandles,
    /// Turn counter for throttled dashboard refresh.
    dashboard_refresh_turn: u32,
    /// Web dashboard server address (if running).
    web_server_addr: Option<std::net::SocketAddr>,
    /// Prompt queued while agent was busy — sent on next AgentEnd.
    queued_prompt: Option<String>,
    /// Toast notification engine.
    toasts: ratatui_toaster::ToastEngine<()>,
    /// Pending image attachment from clipboard paste.
    pending_image: Option<std::path::PathBuf>,
    /// Previous harness status for diffing on HarnessStatusChanged.
    previous_harness_status: Option<crate::status::HarnessStatus>,
    /// Tutorial overlay — game-style first-play advisor.
    tutorial_overlay: Option<tutorial::Tutorial>,
    /// Tutorial highlight target — the widget should pulse/flash its border.
    tutorial_highlight: Option<tutorial::Highlight>,
    /// Legacy tutorial state — lesson-file based (for sandbox projects).
    tutorial: Option<TutorialState>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SelectorKind {
    Model,
    ThinkingLevel,
    ContextClass,
}

/// Result of handling a slash command.
#[derive(Debug)]
enum SlashResult {
    /// Display this text as a system message.
    Display(String),
    /// Command was handled silently (e.g. opened a popup).
    Handled,
    /// Not a recognized command — pass through as user prompt.
    NotACommand,
    /// Quit requested.
    Quit,
}

impl App {
    pub fn new(settings: crate::settings::SharedSettings) -> Self {
        let (model_id, model_provider) = {
            let s = settings.lock().unwrap();
            (s.model.clone(), s.provider().to_string())
        };
        Self {
            editor: Editor::new(),
            conversation: ConversationView::new(),
            agent_active: false,
            should_quit: false,
            turn: 0,
            tool_calls: 0,
            prev_tool_calls: 0,
            memory_ops_this_frame: 0,
            history: Vec::new(),
            history_idx: None,
            dashboard: DashboardState::default(),
            footer_data: FooterData {
                model_id,
                model_provider,
                ..Default::default()
            },
            instrument_panel: InstrumentPanel::default(),
            focus_mode: false,
            theme: theme::default_theme(),
            settings,
            cancel: std::sync::Arc::new(std::sync::Mutex::new(None)),
            last_ctrl_c: None,
            session_start: std::time::Instant::now(),
            selector: None,
            selector_kind: None,
            last_tool_name: None,
            completed_tool_name: None,
            working_verb: "Working",
            replay_splash: false,
            plugin_registry: Some(crate::plugins::registry::PluginRegistry::new(
                crate::prompt::load_lex_imperialis(),
            )),
            effects: effects::Effects::new(),
            bus_commands: Vec::new(),
            dashboard_handles: dashboard::DashboardHandles::default(),
            dashboard_refresh_turn: u32::MAX, // force refresh on first frame
            web_server_addr: None,
            queued_prompt: None,
            toasts: ratatui_toaster::ToastEngineBuilder::new(ratatui::prelude::Rect::default())
                .default_duration(std::time::Duration::from_secs(4))
                .build(),
            pending_image: None,
            previous_harness_status: None,
            tutorial_overlay: None,
            tutorial_highlight: None,
            tutorial: None,
        }
    }

    fn open_model_selector(&mut self) {
        let current = self.settings().model.clone();
        let mut options: Vec<selector::SelectOption> = Vec::new();

        // Only show providers the user is actually authenticated with
        let anthropic_auth = crate::providers::resolve_api_key_sync("anthropic");
        let openai_auth = crate::providers::resolve_api_key_sync("openai");

        if let Some((_, is_oauth)) = anthropic_auth {
            let auth = if is_oauth { "oauth" } else { "key" };
            options.push(sel_opt("anthropic:claude-sonnet-4-6",          "Sonnet 4.6",  &format!("Anthropic · balanced · 200k · {auth}"), &current));
            options.push(sel_opt("anthropic:claude-opus-4-6",            "Opus 4.6",    &format!("Anthropic · strongest · 200k · {auth}"), &current));
            options.push(sel_opt("anthropic:claude-haiku-4-5-20251001",  "Haiku 4.5",   &format!("Anthropic · fast · cheap · 200k · {auth}"), &current));
        }

        if let Some((_, is_oauth)) = openai_auth {
            let auth = if is_oauth { "oauth" } else { "key" };
            options.push(sel_opt("openai:gpt-5.4",   "GPT-5.4",   &format!("OpenAI · frontier · 1M · {auth}"), &current));
            options.push(sel_opt("openai:o3",         "o3",        &format!("OpenAI · reasoning · 200k · {auth}"), &current));
            options.push(sel_opt("openai:o4-mini",    "o4-mini",   &format!("OpenAI · fast reasoning · 200k · {auth}"), &current));
            options.push(sel_opt("openai:gpt-4.1",    "GPT-4.1",  &format!("OpenAI · coding · 1M · {auth}"), &current));
        }

        if options.is_empty() {
            self.conversation.push_system(
                "No providers authenticated.\n\
                 Run: omegon-agent login anthropic  (Claude subscription)\n\
                 Run: omegon-agent login openai     (ChatGPT subscription)\n\
                 Or:  export ANTHROPIC_API_KEY=...   (API key)"
            );
            return;
        }

        self.selector = Some(selector::Selector::new("Select Model", options));
        self.selector_kind = Some(SelectorKind::Model);
    }

    fn open_thinking_selector(&mut self) {
        let current = self.settings().thinking;
        let options = crate::settings::ThinkingLevel::all().iter().map(|level| {
            selector::SelectOption {
                value: level.as_str().to_string(),
                label: format!("{} {}", level.icon(), level.as_str()),
                description: match level {
                    crate::settings::ThinkingLevel::Off => "Servitor — no extended thinking".into(),
                    crate::settings::ThinkingLevel::Minimal => "Functionary — ~2k token budget".into(),
                    crate::settings::ThinkingLevel::Low => "Adept — ~5k token budget".into(),
                    crate::settings::ThinkingLevel::Medium => "Magos — ~10k token budget".into(),
                    crate::settings::ThinkingLevel::High => "Archmagos — ~50k token budget".into(),
                },
                active: *level == current,
            }
        }).collect();
        self.selector = Some(selector::Selector::new("Thinking Level", options));
        self.selector_kind = Some(SelectorKind::ThinkingLevel);
    }

    fn open_context_selector(&mut self) {
        let current = self.settings().context_class;
        let options = crate::settings::ContextClass::all().iter().map(|class| {
            selector::SelectOption {
                value: class.short().to_string(),
                label: class.label().to_string(),
                description: match class {
                    crate::settings::ContextClass::Squad => "Standard sessions".into(),
                    crate::settings::ContextClass::Maniple => "Extended analysis".into(),
                    crate::settings::ContextClass::Clan => "Large codebase".into(),
                    crate::settings::ContextClass::Legion => "Massive context".into(),
                },
                active: *class == current,
            }
        }).collect();
        self.selector = Some(selector::Selector::new("Context Class", options));
        self.selector_kind = Some(SelectorKind::ContextClass);
    }

    fn show_status_change_toasts(&mut self, prev: &crate::status::HarnessStatus, current: &crate::status::HarnessStatus) {
        // Check for persona changes
        if prev.active_persona != current.active_persona {
            match (&prev.active_persona, &current.active_persona) {
                (Some(old), Some(new)) => {
                    if old.id != new.id {
                        self.show_toast(&format!("Persona → {} {}", new.badge, new.name), ratatui_toaster::ToastType::Info);
                    }
                }
                (Some(old), None) => {
                    self.show_toast(&format!("Persona deactivated: {} {}", old.badge, old.name), ratatui_toaster::ToastType::Warning);
                }
                (None, Some(new)) => {
                    self.show_toast(&format!("Persona activated: {} {}", new.badge, new.name), ratatui_toaster::ToastType::Info);
                }
                _ => {}
            }
        }

        // Check for tone changes
        if prev.active_tone != current.active_tone {
            match (&prev.active_tone, &current.active_tone) {
                (Some(old), Some(new)) => {
                    if old.id != new.id {
                        self.show_toast(&format!("Tone → {}", new.name), ratatui_toaster::ToastType::Info);
                    }
                }
                (Some(old), None) => {
                    self.show_toast(&format!("Tone deactivated: {}", old.name), ratatui_toaster::ToastType::Warning);
                }
                (None, Some(new)) => {
                    self.show_toast(&format!("Tone activated: {}", new.name), ratatui_toaster::ToastType::Info);
                }
                _ => {}
            }
        }

        // Check for MCP server changes
        let prev_connected: std::collections::HashSet<&String> = prev.mcp_servers.iter()
            .filter(|s| s.connected)
            .map(|s| &s.name)
            .collect();
        let current_connected: std::collections::HashSet<&String> = current.mcp_servers.iter()
            .filter(|s| s.connected)
            .map(|s| &s.name)
            .collect();

        // New connections
        for name in current_connected.difference(&prev_connected) {
            if let Some(server) = current.mcp_servers.iter().find(|s| &s.name == *name) {
                self.show_toast(&format!("MCP connected: {} ({}t)", name, server.tool_count), ratatui_toaster::ToastType::Info);
            }
        }

        // Lost connections
        for name in prev_connected.difference(&current_connected) {
            self.show_toast(&format!("MCP disconnected: {}", name), ratatui_toaster::ToastType::Warning);
        }

        // Check for auth expiration (simplified - checking provider count as proxy)
        let prev_auth_count = prev.providers.iter().filter(|p| p.authenticated).count();
        let current_auth_count = current.providers.iter().filter(|p| p.authenticated).count();
        if current_auth_count < prev_auth_count {
            self.show_toast("Authentication expired for provider", ratatui_toaster::ToastType::Error);
        }
    }

    fn confirm_selector(&mut self, tx: &mpsc::Sender<TuiCommand>) -> Option<String> {
        let sel = self.selector.take()?;
        let kind = self.selector_kind.take()?;
        let value = sel.selected_value().to_string();

        match kind {
            SelectorKind::Model => {
                self.update_settings(|s| {
                    s.model = value.clone();
                    s.context_window = crate::settings::Settings::new(&value).context_window;
                });
                let _ = tx.try_send(TuiCommand::SetModel(value.clone()));
                Some(format!("Model → {value}"))
            }
            SelectorKind::ThinkingLevel => {
                if let Some(level) = crate::settings::ThinkingLevel::parse(&value) {
                    self.update_settings(|s| s.thinking = level);
                    Some(format!("Thinking → {} {}", level.icon(), level.as_str()))
                } else {
                    Some(format!("Unknown level: {value}"))
                }
            }
            SelectorKind::ContextClass => {
                if let Some(class) = crate::settings::ContextClass::parse(&value) {
                    self.update_settings(|s| {
                        s.context_class = class;
                        s.context_window = class.nominal_tokens();
                        s.context_mode = class.context_mode();
                    });
                    Some(format!("Context class → {}", class.label()))
                } else {
                    Some(format!("Unknown context class: {value}"))
                }
            }
        }
    }

    /// Read a snapshot of current settings (for display).
    fn settings(&self) -> crate::settings::Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Write a setting (for commands like /model, /think).
    fn update_settings<F: FnOnce(&mut crate::settings::Settings)>(&self, f: F) {
        if let Ok(mut s) = self.settings.lock() {
            f(&mut s);
        }
    }

    /// Try to cancel the active agent turn. Returns true if cancelled.
    /// Queue a prompt to be sent when the agent finishes.
    /// Handle /tutorial — start, resume, or manage the interactive tutorial.
    fn handle_tutorial(&mut self, args: &str) -> SlashResult {
        match args.trim() {
            "status" => {
                if let Some(ref overlay) = self.tutorial_overlay {
                    if overlay.active {
                        return SlashResult::Display(format!(
                            "Tutorial: step {}/{} — \"{}\"",
                            overlay.step_index() + 1,
                            overlay.total_steps(),
                            overlay.step().title,
                        ));
                    }
                }
                if let Some(ref tut) = self.tutorial {
                    return SlashResult::Display(tut.status_line());
                }
                SlashResult::Display("No tutorial active. Type /tutorial to start.".into())
            }
            "reset" => {
                self.tutorial_overlay = None;
                if let Some(ref mut tut) = self.tutorial {
                    tut.reset();
                }
                SlashResult::Display("Tutorial reset. Type /tutorial to start again.".into())
            }
            _ => {
                // Start/resume the overlay tutorial
                if let Some(ref mut overlay) = self.tutorial_overlay {
                    overlay.active = true;
                    return SlashResult::Handled;
                }
                // Check for lesson-file tutorial (sandbox project)
                let tutorial_dir = std::path::Path::new(&self.footer_data.cwd)
                    .join(".omegon").join("tutorial");
                if tutorial_dir.is_dir() {
                    if let Some(tut) = TutorialState::load(&tutorial_dir) {
                        let lesson = tut.current_lesson().clone();
                        let status = tut.status_line();
                        self.tutorial = Some(tut);
                        self.queue_prompt(lesson.content);
                        return SlashResult::Display(format!("{status}\n\nLesson queued."));
                    }
                }
                // No lesson files — start the built-in overlay tutorial
                self.tutorial_overlay = Some(tutorial::Tutorial::new());
                SlashResult::Handled
            }
        }
    }

    /// Advance to the next tutorial step (overlay or lesson-file).
    fn handle_tutorial_next(&mut self) -> SlashResult {
        if let Some(ref mut overlay) = self.tutorial_overlay {
            if overlay.active {
                if !overlay.advance() {
                    return SlashResult::Display("🎉 Tutorial complete!".into());
                }
                return SlashResult::Handled;
            }
        }
        if let Some(ref mut tut) = self.tutorial {
            if tut.advance() {
                let lesson = tut.current_lesson().clone();
                let status = tut.status_line();
                self.queue_prompt(lesson.content);
                return SlashResult::Display(format!("{status}\n\nLesson queued."));
            } else {
                return SlashResult::Display("🎉 Tutorial complete!".into());
            }
        }
        SlashResult::Display("No tutorial active.".into())
    }

    /// Go back to the previous tutorial step.
    fn handle_tutorial_prev(&mut self) -> SlashResult {
        if let Some(ref mut overlay) = self.tutorial_overlay {
            if overlay.active {
                if !overlay.go_back() {
                    return SlashResult::Display("Already at the first step.".into());
                }
                return SlashResult::Handled;
            }
        }
        if let Some(ref mut tut) = self.tutorial {
            if tut.go_back() {
                let lesson = tut.current_lesson().clone();
                let status = tut.status_line();
                self.queue_prompt(lesson.content);
                return SlashResult::Display(format!("{status}\n\nLesson queued."));
            }
            return SlashResult::Display("Already at the first lesson.".into());
        }
        SlashResult::Display("No tutorial active.".into())
    }

    /// Clone the tutorial project and exec omegon inside it.
    fn launch_tutorial_project(&mut self) -> SlashResult {
        if cfg!(test) || std::env::var("CARGO_TEST").is_ok() {
            return SlashResult::Display("Tutorial: would clone and launch tutorial project".into());
        }

        const TUTORIAL_REPO: &str = "https://github.com/styrene-lab/omegon-demo.git";
        let tutorial_dir = std::env::temp_dir().join("omegon-tutorial");

        // Clone or pull
        if tutorial_dir.join(".git").exists() {
            let _ = std::process::Command::new("git")
                .args(["pull", "--rebase"])
                .current_dir(&tutorial_dir)
                .output();
        } else {
            let _ = std::fs::remove_dir_all(&tutorial_dir);
            let result = std::process::Command::new("git")
                .args(["clone", "--depth=1", TUTORIAL_REPO, &tutorial_dir.to_string_lossy()])
                .output();
            if result.is_err() || !tutorial_dir.join(".git").exists() {
                return SlashResult::Display("Failed to clone tutorial project. Check network connectivity.".into());
            }
        }

        let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("omegon"));

        // Restore terminal before exec
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = io::stdout().execute(crossterm::terminal::LeaveAlternateScreen);
        let _ = io::stdout().execute(crossterm::event::DisableBracketedPaste);
        let _ = io::stdout().execute(crossterm::event::DisableMouseCapture);

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new(&exe)
                .arg("--no-splash")
                .arg("--context-class")
                .arg("squad")
                .current_dir(&tutorial_dir)
                .exec();
            SlashResult::Display(format!("Failed to launch tutorial: {err}"))
        }
        #[cfg(not(unix))]
        {
            let _ = std::process::Command::new(&exe)
                .arg("--no-splash")
                .arg("--context-class")
                .arg("squad")
                .current_dir(&tutorial_dir)
                .spawn();
            self.should_quit = true;
            SlashResult::Handled
        }
    }

    /// Handle /milestone command — release milestone management.
    fn handle_milestone(&self, args: &str) -> SlashResult {
        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let milestone_dir = std::path::Path::new(&self.footer_data.cwd).join(".omegon");
        let milestone_file = milestone_dir.join("milestones.json");

        match parts.as_slice() {
            // /milestone — list all milestones
            [] | [""] => {
                let milestones = load_milestones(&milestone_file);
                if milestones.is_empty() {
                    return SlashResult::Display("No milestones defined.\n\nUsage:\n  /milestone v0.15.0 add <node-id>\n  /milestone v0.15.0 status\n  /milestone v0.15.0 freeze".into());
                }
                let mut out = String::new();
                for (name, ms) in &milestones {
                    let frozen = if ms.frozen { " 🔒 FROZEN" } else { "" };
                    out.push_str(&format!("{}{}  ({} nodes)\n", name, frozen, ms.nodes.len()));
                    for node_id in &ms.nodes {
                        out.push_str(&format!("  • {}\n", node_id));
                    }
                }
                SlashResult::Display(out.trim_end().to_string())
            }
            // /milestone <version> — show specific milestone
            [version] => {
                let milestones = load_milestones(&milestone_file);
                if let Some(ms) = milestones.get(*version) {
                    let frozen = if ms.frozen { " 🔒 FROZEN" } else { "" };
                    let mut out = format!("{}{}\n\n", version, frozen);
                    if ms.nodes.is_empty() {
                        out.push_str("  (no nodes)\n");
                    }
                    for node_id in &ms.nodes {
                        // Check if the node exists in the dashboard
                        let status = self.dashboard.all_nodes.iter()
                            .find(|n| n.id == *node_id)
                            .map(|n| format!("{:?}", n.status))
                            .unwrap_or_else(|| "unknown".into());
                        out.push_str(&format!("  • {} ({})\n", node_id, status));
                    }
                    SlashResult::Display(out.trim_end().to_string())
                } else {
                    SlashResult::Display(format!("Milestone '{}' not found. Create it with: /milestone {} add <node-id>", version, version))
                }
            }
            // /milestone <version> add <node-id>
            [version, "add", node_id] => {
                let mut milestones = load_milestones(&milestone_file);
                let ms = milestones.entry(version.to_string()).or_insert_with(|| Milestone { nodes: vec![], frozen: false });
                if ms.frozen {
                    return SlashResult::Display(format!("Milestone {} is frozen. No new nodes can be added.", version));
                }
                if !ms.nodes.contains(&node_id.to_string()) {
                    ms.nodes.push(node_id.to_string());
                }
                let _ = std::fs::create_dir_all(&milestone_dir);
                let _ = save_milestones(&milestone_file, &milestones);
                SlashResult::Display(format!("Added '{}' to milestone {}", node_id, version))
            }
            // /milestone <version> remove <node-id>
            [version, "remove", node_id] => {
                let mut milestones = load_milestones(&milestone_file);
                if let Some(ms) = milestones.get_mut(*version) {
                    ms.nodes.retain(|n| n != node_id);
                    let _ = save_milestones(&milestone_file, &milestones);
                    SlashResult::Display(format!("Removed '{}' from milestone {}", node_id, version))
                } else {
                    SlashResult::Display(format!("Milestone '{}' not found.", version))
                }
            }
            // /milestone <version> freeze
            [version, "freeze"] => {
                let mut milestones = load_milestones(&milestone_file);
                if let Some(ms) = milestones.get_mut(*version) {
                    ms.frozen = true;
                    let _ = save_milestones(&milestone_file, &milestones);
                    SlashResult::Display(format!("🔒 Milestone {} is now frozen. No new nodes can be added.", version))
                } else {
                    SlashResult::Display(format!("Milestone '{}' not found.", version))
                }
            }
            // /milestone <version> unfreeze
            [version, "unfreeze"] => {
                let mut milestones = load_milestones(&milestone_file);
                if let Some(ms) = milestones.get_mut(*version) {
                    ms.frozen = false;
                    let _ = save_milestones(&milestone_file, &milestones);
                    SlashResult::Display(format!("🔓 Milestone {} unfrozen.", version))
                } else {
                    SlashResult::Display(format!("Milestone '{}' not found.", version))
                }
            }
            // /milestone <version> status
            [version, "status"] => {
                let milestones = load_milestones(&milestone_file);
                if let Some(ms) = milestones.get(*version) {
                    let total = ms.nodes.len();
                    let mut implemented = 0;
                    let mut decided = 0;
                    let mut exploring = 0;
                    let mut seed = 0;
                    for node_id in &ms.nodes {
                        if let Some(node) = self.dashboard.all_nodes.iter().find(|n| n.id == *node_id) {
                            match node.status {
                                crate::lifecycle::types::NodeStatus::Implemented => implemented += 1,
                                crate::lifecycle::types::NodeStatus::Decided => decided += 1,
                                crate::lifecycle::types::NodeStatus::Exploring => exploring += 1,
                                _ => seed += 1,
                            }
                        } else {
                            seed += 1;
                        }
                    }
                    let frozen = if ms.frozen { "🔒 FROZEN" } else { "open" };
                    let progress = if total > 0 { implemented * 100 / total } else { 0 };
                    SlashResult::Display(format!(
                        "{} — {}\n\n  {} nodes total\n  {} implemented ({}%)\n  {} decided\n  {} exploring\n  {} seed/unknown",
                        version, frozen, total, implemented, progress, decided, exploring, seed
                    ))
                } else {
                    SlashResult::Display(format!("Milestone '{}' not found.", version))
                }
            }
            _ => {
                SlashResult::Display("Usage:\n  /milestone                        — list all\n  /milestone v0.15.0                — show scope\n  /milestone v0.15.0 add <node-id>  — add node\n  /milestone v0.15.0 remove <node>  — remove node\n  /milestone v0.15.0 freeze         — lock scope\n  /milestone v0.15.0 status         — readiness report".into())
            }
        }
    }

    fn queue_prompt(&mut self, text: String) {
        if let Some(ref prev) = self.queued_prompt {
            self.conversation.push_system(&format!("⏳ Replaced queued: {}", &prev[..prev.len().min(40)]));
        }
        self.conversation.push_system(&format!("⏳ Queued: {text}"));
        self.queued_prompt = Some(text);
    }

    fn interrupt(&self) -> bool {
        if let Ok(guard) = self.cancel.lock()
            && let Some(ref token) = *guard {
                token.cancel();
                return true;
            }
        false
    }

    /// Update the dashboard with lifecycle context.
    pub fn update_dashboard_from_lifecycle(
        &mut self,
        nodes: &std::collections::HashMap<String, crate::lifecycle::types::DesignNode>,
        changes: &[crate::lifecycle::types::ChangeInfo],
        focused_id: Option<&str>,
    ) {
        self.dashboard.focused_node = focused_id.and_then(|id| {
            nodes.get(id).map(|n| {
                let sections = crate::lifecycle::design::read_node_sections(n);
                let assumptions = n.assumption_count();
                let decisions_count = sections.as_ref().map(|s| s.decisions.iter().filter(|d| d.status == "decided").count()).unwrap_or(0);
                let readiness = sections.as_ref().map(|s| s.readiness_score()).unwrap_or(0.0);
                dashboard::FocusedNodeSummary {
                    id: n.id.clone(),
                    title: n.title.clone(),
                    status: n.status,
                    open_questions: n.open_questions.len() - assumptions,
                    assumptions,
                    decisions: decisions_count,
                    readiness,
                }
            })
        });
        self.dashboard.active_changes = changes
            .iter()
            .filter(|c| !matches!(c.stage, crate::lifecycle::types::ChangeStage::Archived))
            .map(|c| dashboard::ChangeSummary {
                name: c.name.clone(),
                stage: c.stage,
                done_tasks: c.done_tasks,
                total_tasks: c.total_tasks,
            })
            .collect();
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Update dashboard stats
        self.dashboard.turns = self.turn;
        self.dashboard.tool_calls = self.tool_calls;

        // Refresh dashboard from shared feature handles (throttled)
        if self.turn != self.dashboard_refresh_turn {
            self.dashboard_refresh_turn = self.turn;
            self.dashboard_handles.refresh_into(&mut self.dashboard);
            // Write session stats for the web API
            if let Ok(mut ss) = self.dashboard_handles.session.lock() {
                ss.turns = self.turn;
                ss.tool_calls = self.tool_calls;
                ss.compactions = self.dashboard.compactions;
            }

            // Feed context gauge into dashboard
            self.dashboard.context_used_pct = self.footer_data.context_percent;
            self.dashboard.context_window_k = self.footer_data.context_window;
        }



        let area = frame.area();

        // ── Global background fill ──────────────────────────────────
        // Fill the entire frame with our theme background BEFORE any widgets
        // render. This ensures no cell inherits the terminal's default
        // background (Color::Reset). Every pixel is ours.
        let bg = self.theme.surface_bg();
        let fg = self.theme.fg();
        frame.buffer_mut().reset();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut frame.buffer_mut()[(x, y)];
                cell.reset();
                cell.set_char(' ');
                cell.set_bg(bg);
                cell.set_fg(fg);
            }
        }

        

        // ── Horizontal split: main area | dashboard panel ───────────
        // Dashboard appears as a right-side panel when terminal is wide enough.
        let show_dashboard = area.width >= 120
            && (self.dashboard.status_counts.total > 0
                || self.dashboard.focused_node.is_some()
                || !self.dashboard.active_changes.is_empty()
                || self.dashboard.cleave.as_ref().is_some_and(|c| c.active || c.total_children > 0));

        let (main_area, dash_area) = if show_dashboard {
            let h = Layout::horizontal([
                Constraint::Min(60),
                Constraint::Length(36),
            ]).split(area);
            (h[0], h[1])
        } else {
            (area, Rect::ZERO)
        };

        // ── Vertical layout in the main area ────────────────────────
        let footer_height = if self.focus_mode { 0u16 } else { 12 };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),                    // [0] conversation (all remaining space)
                Constraint::Length(3),                 // [1] editor (input box)
                Constraint::Length(footer_height),     // [2] instrument panel (0 in focus mode)
            ])
            .split(main_area);

        // Conversation view — segment-based widget.
        let t = &self.theme;
        let (segments, conv_state) = self.conversation.segments_and_state();
        let conv_widget = conv_widget::ConversationWidget::new(segments, t.as_ref());
        frame.render_stateful_widget(conv_widget, chunks[0], conv_state);

        // Overlay images on top of placeholders (second pass — needs Frame for StatefulImage)
        {
            let conv_area = chunks[0];
            // Collect image info without holding borrows
            let image_renders: Vec<(usize, Rect, std::path::PathBuf)> = {
                let segments = self.conversation.segments();
                let conv_state = &self.conversation.conv_state;
                conv_state.visible_image_areas(segments, conv_area)
                    .into_iter()
                    .filter_map(|(idx, area)| {
                        if let Segment::Image { ref path, .. } = segments[idx] {
                            Some((idx, area, path.clone()))
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            // Now render with mutable access to image_cache
            for (seg_idx, area, path) in image_renders {
                if let Some(protocol) = self.conversation.image_cache.get_or_create(seg_idx, &path) {
                    image::render_image(area, frame, protocol);
                }
            }
        }

        // Dashboard panel (right side)
        if show_dashboard && dash_area.width > 0 {
            self.dashboard.render_themed(dash_area, frame, t.as_ref());
        }

        // ── Sync footer data from settings (every frame) ────
        {
            let s = self.settings();
            self.footer_data.model_id = s.model.clone();
            self.footer_data.model_provider = s.provider().to_string();
            self.footer_data.context_class = s.context_class;
            self.footer_data.context_mode = s.context_mode;
            self.footer_data.thinking_level = s.thinking.as_str().to_string();
        }
        {
            self.footer_data.model_tier = self.footer_data.harness.capability_tier.clone();
        }
        self.footer_data.turn = self.turn;
        self.footer_data.tool_calls = self.tool_calls;
        self.footer_data.compactions = self.dashboard.compactions;

        // ── CIC Instrument Panel telemetry update ────
        {
            let thinking = match self.settings().thinking {
                crate::settings::ThinkingLevel::Off => "off",
                crate::settings::ThinkingLevel::Minimal => "minimal",
                crate::settings::ThinkingLevel::Low => "low",
                crate::settings::ThinkingLevel::Medium => "medium",
                crate::settings::ThinkingLevel::High => "high",
            };

            // Consume memory ops accumulated since last telemetry update.
            // These accumulate from ToolEnd events between draws.
            // Tool name: use the completed tool name (set on ToolEnd, consumed here)
            let tool_name = self.completed_tool_name.take();

            // Memory op: determine direction from completed tool name
            let mem_op = if self.memory_ops_this_frame > 0 {
                let dir = match tool_name.as_deref() {
                    Some("memory_recall") | Some("memory_query") |
                    Some("memory_episodes") | Some("memory_search_archive") =>
                        instruments::WaveDirection::Left,   // recall ←
                    Some("memory_supersede") =>
                        instruments::WaveDirection::Center,  // supersede ↔
                    _ =>
                        instruments::WaveDirection::Right,   // store →
                };
                Some((0usize, dir)) // mind 0 = project for now
            } else { None };
            self.memory_ops_this_frame = 0;

            self.instrument_panel.update_mind_facts(
                self.footer_data.total_facts,
                self.footer_data.working_memory,
            );
            self.instrument_panel.update_telemetry(
                self.footer_data.context_percent,
                tool_name.as_deref(),
                false,
                thinking,
                mem_op,
                self.agent_active,
                0.016,
            );
        }

        // ── Sync tutorial highlight into widget state ──────────────
        self.tutorial_highlight = self.tutorial_overlay.as_ref()
            .and_then(|t| t.current_highlight());

        // ── Split-panel footer: text left, instruments right ─────────
        // Store inst_area for cleanup pass to skip
        let inst_area = if !self.focus_mode {
            let footer_area = chunks[2];
            let footer_cols = Layout::horizontal([
                Constraint::Percentage(40),
                Constraint::Percentage(60),
            ]).split(footer_area);

            // Pulse engine panel border when tutorial highlights it
            let engine_highlight = matches!(self.tutorial_highlight, Some(tutorial::Highlight::EnginePanel));
            self.footer_data.render_left_panel_with_highlight(footer_cols[0], frame, t.as_ref(), engine_highlight);

            // Pulse instrument panel border when tutorial highlights it
            let inst_highlight = matches!(self.tutorial_highlight, Some(tutorial::Highlight::InstrumentPanel));
            self.instrument_panel.render_with_highlight(footer_cols[1], frame, inst_highlight, t.as_ref());
            footer_cols[1]
        } else {
            Rect::ZERO
        };

        // Apply theme to textarea each frame (in case theme changed)
        self.editor.apply_theme(t.as_ref());

        // Editor — shows reverse search prompt when active
        let (editor_title, editor_content) = if let editor::EditorMode::ReverseSearch { ref query, ref match_idx } = *self.editor.mode() {
            let match_text = match_idx
                .and_then(|i| self.history.get(i))
                .map(|s| s.as_str())
                .unwrap_or("");
            (
                Span::styled(
                    format!(" (reverse-i-search)`{query}': "),
                    t.style_warning(),
                ),
                match_text.to_string(),
            )
        } else if self.agent_active {
            (
                Span::styled(
                    format!(" ⟳ {}... ", self.working_verb),
                    t.style_warning(),
                ),
                String::new(),
            )
        } else {
            (Span::styled(" ▸ ", t.style_accent()), String::new())
        };

        let editor_border_style = if matches!(self.tutorial_highlight, Some(tutorial::Highlight::InputBar)) {
            Style::default().fg(t.accent_bright()).bg(t.surface_bg())
        } else {
            Style::default().fg(t.accent_muted()).bg(t.surface_bg())
        };
        let editor_block = Block::default()
            .borders(Borders::TOP)
            .border_style(editor_border_style)
            .title(editor_title);

        if !editor_content.is_empty() {
            // Reverse search mode — show the matched text
            let editor_widget = Paragraph::new(editor_content)
                .style(Style::default().fg(t.fg()).bg(t.surface_bg()))
                .block(editor_block);
            frame.render_widget(editor_widget, chunks[1]);
        } else {
            // Normal mode — render the textarea widget directly
            self.editor.textarea.set_block(editor_block);
            frame.render_widget(&self.editor.textarea, chunks[1]);
        }

        // Command palette popup (above editor when typing /)
        if !self.agent_active {
            let matches = self.matching_commands();
            if !matches.is_empty() {
                let palette_height = matches.len().min(8) as u16 + 2; // +2 for borders
                let editor_area = chunks[1];
                let palette_area = Rect {
                    x: editor_area.x,
                    y: editor_area.y.saturating_sub(palette_height),
                    width: editor_area.width.min(50),
                    height: palette_height,
                };

                let items: Vec<Line<'static>> = matches.iter().map(|(name, desc)| {
                    Line::from(vec![
                        Span::styled(format!(" /{name}"), t.style_accent()),
                        Span::styled(format!("  {desc}"), t.style_muted()),
                    ])
                }).collect();

                let palette = Paragraph::new(items)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .border_style(t.style_border())
                        .title(Span::styled(" commands ", t.style_dim())));

                // Clear the area first (prevents bleed-through)
                frame.render_widget(ratatui::widgets::Clear, palette_area);
                frame.render_widget(palette, palette_area);
            }

            // Textarea renders its own cursor via cursor_style
        }

        // Selector popup (overlays everything when active)
        if let Some(ref sel) = self.selector {
            sel.render(area, frame, t.as_ref());
        }

        // ── Post-render effects (tachyonfx) — each zone processed separately ──
        self.effects.process(frame.buffer_mut(), chunks[0], chunks[2], chunks[1]);

        // ── Toast notifications — rendered last, on top of everything ──
        self.toasts.set_area(frame.area());
        frame.render_widget(&self.toasts, frame.area());

        // ── Final bg cleanup pass ───────────────────────────────────
        // Force every cell to have a known-good background color.
        // Skip the instrument panel area — it renders its own pixels
        // with half-block characters where bg carries color data.
        {
            let base = self.theme.surface_bg();
            let card = self.theme.card_bg();
            let footer = self.theme.footer_bg();
            let err_bg = Color::Rgb(30, 8, 16);
            let diff_add = Color::Rgb(4, 22, 12);
            let diff_rm = Color::Rgb(22, 4, 4);
            // inst_area already computed above — no duplicate layout calc
            let buf = frame.buffer_mut();
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    // Skip instrument panel — it owns its pixels
                    if inst_area.width > 0
                        && x >= inst_area.x && x < inst_area.right()
                        && y >= inst_area.y && y < inst_area.bottom()
                    {
                        continue;
                    }
                    let cell = &mut buf[(x, y)];
                    let bg = cell.bg;
                    match bg {
                        c if c == base || c == card || c == footer => {}
                        c if c == err_bg || c == diff_add || c == diff_rm => {}
                        _ => { cell.set_bg(base); }
                    }
                }
            }
        }

        // ── Tutorial overlay — rendered absolutely last, on top of everything ──
        if let Some(ref tut) = self.tutorial_overlay {
            if tut.active {
                let footer_h: u16 = if self.focus_mode { 0 } else { 12 };
                tut.render(area, frame.buffer_mut(), self.theme.as_ref(), footer_h);
            }
        }
    }

    /// Mark tutorial as completed so it doesn't auto-start again.
    fn mark_tutorial_completed(&self) {
        let marker = std::path::Path::new(&self.footer_data.cwd)
            .join(".omegon").join("tutorial_completed");
        let _ = std::fs::create_dir_all(marker.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&marker, "");
    }

    /// Show a transient toast notification.
    fn show_toast(&mut self, message: &str, toast_type: ratatui_toaster::ToastType) {
        use ratatui_toaster::{ToastBuilder, ToastPosition};
        self.toasts.show_toast(
            ToastBuilder::new(std::borrow::Cow::Owned(message.to_string()))
                .toast_type(toast_type)
                .position(ToastPosition::TopRight),
        );
    }

    /// Command registry: (name, description, subcommands).
    const COMMANDS: &'static [(&'static str, &'static str, &'static [&'static str])] = &[
        ("help",     "show available commands",              &[]),
        ("model",    "view or switch model",                 &["list"]),
        ("think",    "set thinking level",                   &["off", "low", "medium", "high"]),
        ("stats",    "session telemetry",                    &[]),
        ("compact",  "trigger context compaction",           &[]),
        ("clear",    "clear conversation display",           &[]),
        ("detail",   "toggle tool display (compact/detailed)", &["compact", "detailed"]),
        ("context",  "select context class (Squad/Maniple/Clan/Legion)",       &["squad", "maniple", "clan", "legion"]),
        ("sessions", "list saved sessions",                  &[]),
        ("memory",   "memory stats",                        &[]),
        ("login",    "log in to provider (default: anthropic)", &["anthropic", "openai"]),
        ("logout",   "log out of provider",                   &["anthropic", "openai"]),
        ("auth",     "authentication management",             &["status", "login", "logout", "unlock"]),
        ("chronos",  "date/time context",                      &["week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all"]),
        ("migrate",  "import from other tools",               &["auto", "claude-code", "pi", "codex", "cursor", "aider"]),
        ("dash",     "toggle dashboard panel / open web UI",  &["open"]),
        ("vault",    "Vault status and management",           &["status", "unseal", "login", "configure", "init-policy"]),
        ("persona",  "switch persona (or 'off' to deactivate)",  &["off"]),
        ("tone",     "switch tone (or 'off' to deactivate)",    &["off"]),
        ("delegate", "delegate task management",              &["status"]),
        ("status",   "show harness status (providers, MCP, secrets, routing)", &[]),
        ("focus",    "toggle instrument panel focus mode",   &[]),
        ("tutorial", "interactive tutorial",                           &["status", "reset"]),
        ("milestone","release milestone management",                 &["freeze", "status"]),
        ("splash",   "replay splash animation",              &[]),
        ("dashboard", "open web dashboard (alias for /dash open)", &[]),
        ("version",  "show build version and git sha",       &[]),
        ("exit",     "quit (or double Ctrl+C)",              &[]),
    ];

    /// Handle a slash command.
    fn handle_slash_command(&mut self, text: &str, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        let trimmed = text.trim();
        if !trimmed.starts_with('/') { return SlashResult::NotACommand; }
        let rest = &trimmed[1..];
        let (cmd, args) = rest.split_once(' ').unwrap_or((rest, ""));
        let args = args.trim();

        // Absolute file paths (e.g. /home/user/file.txt) are not commands
        if cmd.contains('/') { return SlashResult::NotACommand; }

        match cmd {
            "help" => {
                let lines: Vec<String> = Self::COMMANDS.iter()
                    .map(|(n, d, subs)| {
                        if subs.is_empty() {
                            format!("  /{n:<12} {d}")
                        } else {
                            format!("  /{n:<12} {d}  [{}]", subs.join("|"))
                        }
                    }).collect();
                SlashResult::Display(format!("Commands:\n{}\n\nType / to browse. Tab completes.", lines.join("\n")))
            }

            "model" => {
                if args.is_empty() {
                    // No args → open interactive selector
                    self.open_model_selector();
                    SlashResult::Handled
                } else {
                    // Direct switch: /model anthropic:claude-opus-4-6
                    self.update_settings(|s| {
                        s.model = args.to_string();
                        s.context_window = crate::settings::Settings::new(args).context_window;
                    });
                    let _ = tx.try_send(TuiCommand::SetModel(args.to_string()));
                    SlashResult::Display(format!("Model → {args}"))
                }
            }

            "think" => {
                if args.is_empty() {
                    // No args → open interactive selector
                    self.open_thinking_selector();
                    SlashResult::Handled
                } else if let Some(level) = crate::settings::ThinkingLevel::parse(args) {
                    self.update_settings(|s| s.thinking = level);
                    SlashResult::Display(format!("Thinking → {} {}", level.icon(), level.as_str()))
                } else {
                    SlashResult::Display(format!("Unknown level: {args}. Options: off, low, medium, high"))
                }
            }

            "stats" => {
                let s = self.settings();
                let elapsed = self.session_start.elapsed();
                let time = if elapsed.as_secs() >= 3600 {
                    format!("{}h{}m", elapsed.as_secs() / 3600, (elapsed.as_secs() % 3600) / 60)
                } else if elapsed.as_secs() >= 60 {
                    format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
                } else {
                    format!("{}s", elapsed.as_secs())
                };
                SlashResult::Display(format!(
                    "Session:\n  Duration:    {time}\n  Turns:       {}\n  Tool calls:  {}\n  Compactions: {}\n\n\
                     Context:\n  Usage:       {:.0}%\n  Window:      {} tokens\n  Model:       {}\n  Thinking:    {} {}",
                    self.turn, self.tool_calls, self.dashboard.compactions,
                    self.footer_data.context_percent, s.context_window,
                    s.model_short(), s.thinking.icon(), s.thinking.as_str(),
                ))
            }

            "status" => {
                let panel = crate::tui::bootstrap::render_bootstrap(
                    &self.footer_data.harness,
                    false, // no ANSI — SlashResult::Display renders via ratatui
                );
                SlashResult::Display(panel)
            }

            "persona" => {
                if args == "off" {
                    if let Some(ref mut registry) = self.plugin_registry {
                        let result = registry.deactivate_persona();
                        match result.removed_id {
                            Some(id) => SlashResult::Display(format!("Persona deactivated: {id}")),
                            None => SlashResult::Display("No persona active.".into()),
                        }
                    } else {
                        SlashResult::Display("Plugin registry not initialized.".into())
                    }
                } else if args.is_empty() {
                    // List available personas
                    let (personas, _) = crate::plugins::persona_loader::scan_available();
                    if personas.is_empty() {
                        SlashResult::Display("No personas installed.\n  Install with: omegon plugin install <git-url>".into())
                    } else {
                        let active_id = self.plugin_registry.as_ref()
                            .and_then(|r| r.active_persona().map(|p| p.id.clone()));
                        let lines: Vec<String> = personas.iter().map(|p| {
                            let marker = if active_id.as_deref() == Some(&p.id) { " ●" } else { "" };
                            format!("  {:<20} {}{}", p.name, p.description, marker)
                        }).collect();
                        SlashResult::Display(format!("Available personas:\n{}\n\n  /persona <name> to activate, /persona off to deactivate", lines.join("\n")))
                    }
                } else {
                    // Activate by name (case-insensitive match)
                    let (personas, _) = crate::plugins::persona_loader::scan_available();
                    let target = args.to_lowercase();
                    match personas.iter().find(|p| p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target)) {
                        Some(available) => {
                            match crate::plugins::persona_loader::load_persona(&available.path) {
                                Ok(persona) => {
                                    let name = persona.name.clone();
                                    let badge = persona.badge.clone().unwrap_or_else(|| "⚙".into());
                                    let fact_count = persona.mind_facts.len();
                                    if let Some(ref mut registry) = self.plugin_registry {
                                        registry.activate_persona(persona);
                                    }
                                    SlashResult::Display(format!("{badge} Persona activated: {name} ({fact_count} mind facts)"))
                                }
                                Err(e) => SlashResult::Display(format!("Failed to load persona: {e}")),
                            }
                        }
                        None => SlashResult::Display(format!("Persona '{args}' not found. Run /persona to list available.")),
                    }
                }
            }

            "tone" => {
                if args == "off" {
                    if let Some(ref mut registry) = self.plugin_registry {
                        let result = registry.deactivate_tone();
                        match result {
                            Some(id) => SlashResult::Display(format!("Tone deactivated: {id}")),
                            None => SlashResult::Display("No tone active.".into()),
                        }
                    } else {
                        SlashResult::Display("Plugin registry not initialized.".into())
                    }
                } else if args.is_empty() {
                    let (_, tones) = crate::plugins::persona_loader::scan_available();
                    if tones.is_empty() {
                        SlashResult::Display("No tones installed.\n  Install with: omegon plugin install <git-url>".into())
                    } else {
                        let active_id = self.plugin_registry.as_ref()
                            .and_then(|r| r.active_tone().map(|t| t.id.clone()));
                        let lines: Vec<String> = tones.iter().map(|t| {
                            let marker = if active_id.as_deref() == Some(&t.id) { " ●" } else { "" };
                            format!("  {:<20} {}{}", t.name, t.description, marker)
                        }).collect();
                        SlashResult::Display(format!("Available tones:\n{}\n\n  /tone <name> to activate, /tone off to deactivate", lines.join("\n")))
                    }
                } else {
                    let (_, tones) = crate::plugins::persona_loader::scan_available();
                    let target = args.to_lowercase();
                    match tones.iter().find(|t| t.name.to_lowercase() == target || t.id.to_lowercase().contains(&target)) {
                        Some(available) => {
                            match crate::plugins::persona_loader::load_tone(&available.path) {
                                Ok(tone) => {
                                    let name = tone.name.clone();
                                    if let Some(ref mut registry) = self.plugin_registry {
                                        registry.activate_tone(tone);
                                    }
                                    SlashResult::Display(format!("♪ Tone activated: {name}"))
                                }
                                Err(e) => SlashResult::Display(format!("Failed to load tone: {e}")),
                            }
                        }
                        None => SlashResult::Display(format!("Tone '{args}' not found. Run /tone to list available.")),
                    }
                }
            }

            "detail" => {
                if args.is_empty() {
                    // Toggle
                    let current = self.settings().tool_detail;
                    let next = match current {
                        crate::settings::ToolDetail::Compact => crate::settings::ToolDetail::Detailed,
                        crate::settings::ToolDetail::Detailed => crate::settings::ToolDetail::Compact,
                    };
                    self.update_settings(|s| s.tool_detail = next);
                    SlashResult::Display(format!("Tool display → {}", next.as_str()))
                } else if let Some(mode) = crate::settings::ToolDetail::parse(args) {
                    self.update_settings(|s| s.tool_detail = mode);
                    SlashResult::Display(format!("Tool display → {}", mode.as_str()))
                } else {
                    SlashResult::Display(format!("Unknown mode: {args}. Options: compact, detailed"))
                }
            }

            "context" => {
                if args.is_empty() {
                    // No args → open interactive context class selector
                    self.open_context_selector();
                    SlashResult::Handled
                } else if let Some(mode) = crate::settings::ContextMode::parse(args) {
                    self.update_settings(|s| {
                        s.context_mode = mode;
                        s.apply_context_mode();
                    });
                    let s = self.settings();
                    self.footer_data.context_window = s.context_window;
                    SlashResult::Display(format!("Context → {} {}", mode.icon(), mode.as_str()))
                } else {
                    SlashResult::Display(format!("Unknown mode: {args}. Options: 200k, 1m"))
                }
            }

            "compact" => {
                let _ = tx.try_send(TuiCommand::Compact);
                SlashResult::Display("Compaction queued — runs before next turn.".into())
            }

            "clear" => {
                self.conversation = ConversationView::new();
                SlashResult::Display("Display cleared.".into())
            }

            "sessions" => {
                let _ = tx.try_send(TuiCommand::ListSessions);
                SlashResult::Handled
            }

            "memory" => {
                SlashResult::Display(format!(
                    "Memory:\n  Facts:          {}\n  Injected:       {}\n  Working memory: {}\n  ~{} tokens",
                    self.footer_data.total_facts, self.footer_data.injected_facts,
                    self.footer_data.working_memory, self.footer_data.memory_tokens_est,
                ))
            }

            "auth" => {
                match args {
                    "" | "status" => {
                        // Show authentication status
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_status".to_string(),
                            args: String::new(),
                        });
                        SlashResult::Handled
                    }
                    "login" | "login anthropic" | "login claude" => {
                        // Login to Anthropic
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_login".to_string(),
                            args: "anthropic".to_string(),
                        });
                        SlashResult::Handled
                    }
                    "login openai" | "login chatgpt" => {
                        // Login to OpenAI
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_login".to_string(),
                            args: "openai".to_string(),
                        });
                        SlashResult::Handled
                    }
                    "logout anthropic" | "logout claude" => {
                        // Logout from Anthropic
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_logout".to_string(),
                            args: "anthropic".to_string(),
                        });
                        SlashResult::Handled
                    }
                    "logout openai" | "logout chatgpt" => {
                        // Logout from OpenAI
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_logout".to_string(),
                            args: "openai".to_string(),
                        });
                        SlashResult::Handled
                    }
                    "unlock" => {
                        // Unlock secrets store
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_unlock".to_string(),
                            args: String::new(),
                        });
                        SlashResult::Handled
                    }
                    _ => {
                        if args.starts_with("login ") {
                            let provider = &args[6..];
                            if provider.is_empty() {
                                SlashResult::Display("Usage: /auth login <provider>\nSupported: anthropic, openai".into())
                            } else {
                                let _ = tx.try_send(TuiCommand::BusCommand {
                                    name: "auth_login".to_string(),
                                    args: provider.to_string(),
                                });
                                SlashResult::Handled
                            }
                        } else if args.starts_with("logout ") {
                            let provider = &args[7..];
                            if provider.is_empty() {
                                SlashResult::Display("Usage: /auth logout <provider>\nSupported: anthropic, openai".into())
                            } else {
                                let _ = tx.try_send(TuiCommand::BusCommand {
                                    name: "auth_logout".to_string(),
                                    args: provider.to_string(),
                                });
                                SlashResult::Handled
                            }
                        } else {
                            SlashResult::Display(format!(
                                "Unknown auth command: {args}\n\nUsage:\n  /auth status\n  /auth login <provider>\n  /auth logout <provider>\n  /auth unlock\n\nSupported providers: anthropic, openai"
                            ))
                        }
                    }
                }
            }

            "migrate" => {
                let source = if args.is_empty() { "auto" } else { args };
                let cwd = std::path::Path::new(&self.footer_data.cwd);
                let report = crate::migrate::run(source, cwd);
                SlashResult::Display(report.summary())
            }

            "chronos" => {
                let sub = if args.is_empty() { "week" } else { args };
                match crate::tools::chronos::execute(sub, None, None, None) {
                    Ok(text) => SlashResult::Display(text),
                    Err(e) => SlashResult::Display(format!("❌ {e}")),
                }
            }

            "dash" => {
                if args == "open" {
                    if let Some(addr) = self.web_server_addr {
                        let url = format!("http://{addr}");
                        open_browser(&url);
                        SlashResult::Display(format!("Dashboard at {url}"))
                    } else {
                        let _ = tx.try_send(TuiCommand::StartWebDashboard);
                        SlashResult::Display("Starting web dashboard…".into())
                    }
                } else if let Some(addr) = self.web_server_addr {
                    let url = format!("http://{addr}");
                    SlashResult::Display(format!("Dashboard running at {url}\nUse /dash open to open in browser"))
                } else {
                    SlashResult::Display("Use /dash open to start the web dashboard".into())
                }
            }

            "splash" => {
                // Set flag to replay splash on next draw cycle
                self.replay_splash = true;
                SlashResult::Handled
            }

            "delegate" => {
                match args {
                    "" | "status" => {
                        // Show active/completed delegate tasks
                        SlashResult::Display("Delegate status: use the delegate_status agent tool for full details.\n\nActive delegates shown in dashboard when running.".into())
                    }
                    _ => {
                        SlashResult::Display("Usage: /delegate status\n\nTo invoke a delegate, use the delegate agent tool.".into())
                    }
                }
            }

            "focus" => {
                // Toggle instrument panel focus mode
                self.focus_mode = !self.focus_mode;
                let status = if self.focus_mode { "enabled" } else { "disabled" };
                SlashResult::Display(format!("Instrument panel focus mode → {status}"))
            }

            "milestone" => {
                self.handle_milestone(args)
            }

            "tutorial" | "demo" => {
                self.handle_tutorial(args)
            }

            // "next" and "prev" removed — tutorial uses Tab/Shift+Tab, not slash commands

            "vault" => {
                match args {
                    "" | "status" => {
                        // Check Vault status via CLI
                        let addr = std::env::var("VAULT_ADDR").unwrap_or_default();
                        if addr.is_empty() {
                            SlashResult::Display("Vault: not configured (VAULT_ADDR not set)\n\nUse `/vault configure` or set VAULT_ADDR".into())
                        } else {
                            match std::process::Command::new("vault").args(["status", "-format=json"]).output() {
                                Ok(out) if out.status.success() => {
                                    let body = String::from_utf8_lossy(&out.stdout);
                                    let info = serde_json::from_str::<serde_json::Value>(&body).ok();
                                    let sealed = info.as_ref().and_then(|v| v["sealed"].as_bool()).unwrap_or(true);
                                    let version = info.as_ref().and_then(|v| v["version"].as_str()).unwrap_or("unknown");
                                    let icon = if sealed { "🔒" } else { "🔓" };
                                    SlashResult::Display(format!(
                                        "Vault {icon}\n  Address:  {addr}\n  Status:   {}\n  Version:  {version}",
                                        if sealed { "sealed" } else { "unsealed" },
                                    ))
                                }
                                Ok(out) => {
                                    let stderr = String::from_utf8_lossy(&out.stderr);
                                    if stderr.contains("sealed") || stderr.contains("Sealed") {
                                        SlashResult::Display(format!("Vault 🔒\n  Address:  {addr}\n  Status:   sealed\n\nUse `/vault unseal` to provide unseal keys"))
                                    } else {
                                        SlashResult::Display(format!("Vault ✗\n  Address:  {addr}\n  Status:   unreachable\n  Error:    {}", stderr.chars().take(200).collect::<String>()))
                                    }
                                }
                                Err(_) => SlashResult::Display(format!("Vault ✗\n  Address:  {addr}\n  Status:   vault CLI not found")),
                            }
                        }
                    }
                    "unseal" => {
                        // TODO: implement masked multi-key input mode
                        // For now, direct operators to the vault CLI
                        SlashResult::Display(
                            "Vault Unseal:\n\n\
                             Masked unseal input is not yet implemented in the TUI.\n\
                             Use the vault CLI directly:\n\
                             \n  vault operator unseal\n\
                             \nThis will prompt for unseal keys without echoing them.\n\
                             Repeat until the threshold is met.".into()
                        )
                    }
                    "login" => {
                        // TODO: implement interactive token/AppRole credential entry
                        SlashResult::Display(
                            "Vault Login:\n\n\
                             Interactive login is not yet implemented in the TUI.\n\
                             Use the vault CLI:\n\
                             \n  vault login                         # token (interactive)\n\
                             \n  vault login -method=approle \\       # AppRole\n\
                               role_id=<role> secret_id=<secret>\n\
                             \nThe token will be stored in ~/.vault-token automatically.".into()
                        )
                    }
                    "configure" => {
                        SlashResult::Display(
                            "Vault Configuration:\n\n\
                             Set VAULT_ADDR to your Vault server address:\n\
                             \n  export VAULT_ADDR=https://vault.example.com\n\
                             \nAuthenticate with:\n\
                             \n  vault login                  # interactive\n\
                             \n  vault login -method=approle  # AppRole\n\
                             \nOr create ~/.omegon/vault.json:\n\
                             \n  {\"addr\": \"https://vault.example.com\", \"auth\": \"token\", \"allowed_paths\": [\"secret/data/omegon/*\"], \"denied_paths\": []}".into()
                        )
                    }
                    "init-policy" => {
                        SlashResult::Display(
                            "# Omegon Agent Vault Policy\n\
                             # Apply with: vault policy write omegon-agent omegon-policy.hcl\n\n\
                             ```hcl\n\
                             # Read/write agent-scoped secrets\n\
                             path \"secret/data/omegon/*\" {\n  capabilities = [\"read\", \"create\", \"update\"]\n}\n\
                             path \"secret/metadata/omegon/*\" {\n  capabilities = [\"read\", \"list\"]\n}\n\n\
                             # Read-only access to shared infra secrets\n\
                             path \"secret/data/bootstrap/*\" {\n  capabilities = [\"read\"]\n}\n\n\
                             # Allow minting child tokens for cleave\n\
                             path \"auth/token/create\" {\n  capabilities = [\"create\", \"update\"]\n  allowed_parameters = {\n    \"policies\" = [\"omegon-child\"]\n    \"ttl\" = [\"30m\"]\n    \"num_uses\" = [\"100\"]\n  }\n}\n\
                             ```\n\n\
                             Save to a file and apply: `vault policy write omegon-agent <file>`".into()
                        )
                    }
                    _ => SlashResult::Display(format!("Unknown vault subcommand: {args}\nOptions: status, unseal, login, configure, init-policy")),
                }
            }

            // /login [provider] — alias for /auth login <provider>
            "login" => {
                let provider = if args.is_empty() { "anthropic" } else { args };
                let _ = tx.try_send(TuiCommand::BusCommand {
                    name: "auth_login".to_string(),
                    args: provider.to_string(),
                });
                SlashResult::Handled
            }

            // /logout [provider] — alias for /auth logout <provider>
            "logout" => {
                let provider = if args.is_empty() { "anthropic" } else { args };
                let _ = tx.try_send(TuiCommand::BusCommand {
                    name: "auth_logout".to_string(),
                    args: provider.to_string(),
                });
                SlashResult::Handled
            }

            "exit" | "quit" => SlashResult::Quit,

            // ── Aliases ─────────────────────────────────────────────
            "dashboard" => self.handle_slash_command("/dash open", tx),
            "thinking"  => self.handle_slash_command(&format!("/think {args}"), tx),
            "models"    => self.handle_slash_command("/model", tx),
            "version"   => SlashResult::Display(format!(
                "omegon {} ({} {})",
                env!("CARGO_PKG_VERSION"),
                env!("OMEGON_GIT_SHA"),
                env!("OMEGON_BUILD_DATE"),
            )),
            "q" => SlashResult::Quit,

            _ => {
                // Check if a bus feature handles this command
                if self.bus_commands.iter().any(|c| c.name == cmd) {
                    let _ = tx.try_send(TuiCommand::BusCommand {
                        name: cmd.to_string(),
                        args: args.to_string(),
                    });
                    SlashResult::Handled
                } else {
                    // Try prefix match — e.g. "/das" matches "/dash"
                    let matches: Vec<&str> = Self::COMMANDS.iter()
                        .map(|(name, _, _)| *name)
                        .filter(|name| name.starts_with(cmd) && *name != cmd)
                        .collect();
                    if matches.len() == 1 {
                        // Unique prefix match — execute it
                        let full_cmd = if args.is_empty() {
                            format!("/{}", matches[0])
                        } else {
                            format!("/{} {args}", matches[0])
                        };
                        self.handle_slash_command(&full_cmd, tx)
                    } else if !matches.is_empty() {
                        // Ambiguous prefix
                        SlashResult::Display(format!(
                            "Ambiguous command /{cmd}. Did you mean: {}",
                            matches.iter().map(|m| format!("/{m}")).collect::<Vec<_>>().join(", ")
                        ))
                    } else {
                        // No match at all — show error, do NOT send to agent
                        SlashResult::Display(format!(
                            "Unknown command: /{cmd}\n\nType /help for available commands."
                        ))
                    }
                }
            }
        }
    }

    /// Palette: matching commands + subcommands for the current editor text.
    fn matching_commands(&self) -> Vec<(String, String)> {
        let text = self.editor.render_text();
        if !text.starts_with('/') { return vec![]; }
        let input = &text[1..];
        let parts: Vec<&str> = input.splitn(2, ' ').collect();

        if parts.len() <= 1 {
            let prefix = parts.first().copied().unwrap_or("");
            let mut matches: Vec<(String, String)> = if prefix.is_empty() {
                Self::COMMANDS.iter().map(|(n, d, _)| (n.to_string(), d.to_string())).collect()
            } else {
                Self::COMMANDS.iter()
                    .filter(|(name, _, _)| name.starts_with(prefix))
                    .map(|(n, d, _)| (n.to_string(), d.to_string()))
                    .collect()
            };
            // Append bus feature commands
            for cmd in &self.bus_commands {
                if prefix.is_empty() || cmd.name.starts_with(prefix) {
                    matches.push((cmd.name.clone(), cmd.description.clone()));
                }
            }
            matches
        } else {
            let cmd = parts[0];
            let sub_prefix = parts.get(1).copied().unwrap_or("");
            // Check built-in commands first, then bus commands
            if let Some((_, _, subs)) = Self::COMMANDS.iter().find(|(n, _, _)| *n == cmd) {
                subs.iter()
                    .filter(|s| s.starts_with(sub_prefix))
                    .map(|s| (format!("{cmd} {s}"), String::new()))
                    .collect()
            } else if let Some(bus_cmd) = self.bus_commands.iter().find(|c| c.name == cmd) {
                bus_cmd.subcommands.iter()
                    .filter(|s| s.starts_with(sub_prefix))
                    .map(|s| (format!("{cmd} {s}"), String::new()))
                    .collect()
            } else {
                vec![]
            }
        }
    }

    /// Load editor history from disk.
    fn load_history(cwd: &str) -> Vec<String> {
        let path = history_path(cwd);
        match std::fs::read_to_string(&path) {
            Ok(content) => content
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Save editor history to disk.
    fn save_history(&self) {
        let path = history_path(&self.footer_data.cwd);
        if self.history.is_empty() {
            return;
        }
        // Keep last 500 entries
        let start = self.history.len().saturating_sub(500);
        let content = self.history[start..].join("\n");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, content) {
            tracing::debug!("Failed to save history: {e}");
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_idx {
            None => self.history.len().saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        };
        self.history_idx = Some(idx);
        self.editor.set_text(&self.history[idx]);
    }

    fn history_down(&mut self) {
        match self.history_idx {
            None => {}
            Some(i) => {
                if i + 1 < self.history.len() {
                    self.history_idx = Some(i + 1);
                    self.editor.set_text(&self.history[i + 1]);
                } else {
                    self.history_idx = None;
                    self.editor.set_text("");
                }
            }
        }
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { turn } => {
                self.agent_active = true;
                self.turn = turn;
                self.working_verb = spinner::next_verb();
                self.effects.start_spinner_glow();
            }
            AgentEvent::TurnEnd { turn } => {
                self.turn = turn;
                // Estimate context usage from turn count + tool calls
                // (rough heuristic: ~2k tokens per turn average)
                let est_tokens = (turn as usize) * 2000 + (self.tool_calls as usize) * 500;
                let ctx_window = self.footer_data.context_window;
                if ctx_window > 0 {
                    self.footer_data.estimated_tokens = est_tokens;
                    self.footer_data.context_percent =
                        (est_tokens as f32 / ctx_window as f32 * 100.0).min(100.0);
                }
            }
            AgentEvent::MessageChunk { text } => {
                self.conversation.append_streaming(&text);
            }
            AgentEvent::ThinkingChunk { text } => {
                self.conversation.append_thinking(&text);
            }
            AgentEvent::ToolStart { id, name, args } => {
                self.working_verb = spinner::next_verb();
                let args_summary = crate::r#loop::summarize_tool_args(&name, &args);
                // Full args for detailed view
                let detail_args = match name.as_str() {
                    "bash" => args.get("command").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    "read" | "edit" | "write" | "view" => args.get("path").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    _ => Some(serde_json::to_string_pretty(&args).unwrap_or_default()),
                };
                self.conversation.push_tool_start(&id, &name, args_summary.as_deref(), detail_args.as_deref());
                self.tool_calls += 1;
                self.last_tool_name = Some(name);
            }
            AgentEvent::ToolEnd { id, result, is_error } => {
                let summary = result.content.first().and_then(|c| match c {
                    omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                });
                self.conversation.push_tool_end(&id, is_error, summary);

                // Signal tool error to instrument panel
                if is_error {
                    if let Some(ref name) = self.last_tool_name {
                        self.instrument_panel.set_tool_error(name);
                    }
                }

                // Detect image results from view/render tools
                if !is_error && image::is_available()
                    && let Some(ref name) = self.last_tool_name
                    && matches!(name.as_str(), "view" | "render_diagram" | "generate_image_local"
                        | "render_excalidraw" | "render_composition_still" | "render_native_diagram")
                    && let Some(text) = summary
                {
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if image::is_image_path(trimmed) && std::path::Path::new(trimmed).exists() {
                            self.conversation.push_image(
                                std::path::PathBuf::from(trimmed),
                                "",
                            );
                            break;
                        }
                    }
                }

                // Dynamic footer: memory tools update fact count
                if let Some(ref name) = self.last_tool_name {
                    let is_memory_mutation = matches!(name.as_str(),
                        "memory_store" | "memory_supersede" | "memory_archive");
                    if name == "memory_store" || name == "memory_supersede" {
                        self.footer_data.total_facts += 1;
                    } else if name == "memory_archive" {
                        self.footer_data.total_facts = self.footer_data.total_facts.saturating_sub(1);
                    }
                    if is_memory_mutation {
                        self.memory_ops_this_frame += 1;
                        self.effects.ping_footer(self.theme.as_ref());
                    }
                    // Also count recall/query operations
                    if matches!(name.as_str(), "memory_recall" | "memory_query" | "memory_episodes" | "memory_search_archive" | "memory_focus" | "memory_release") {
                        self.memory_ops_this_frame += 1;
                    }
                }
                // Save for instrument telemetry before clearing
                self.completed_tool_name = self.last_tool_name.take();
            }
            AgentEvent::AgentEnd => {
                self.agent_active = false;
                self.conversation.finalize_message();
                self.effects.stop_spinner_glow();
            }
            AgentEvent::PhaseChanged { phase } => {
                self.conversation.push_lifecycle("◈", &format!("Phase → {phase:?}"));
            }
            AgentEvent::DecompositionStarted { children } => {
                self.conversation.push_lifecycle(
                    "⚡",
                    &format!("Cleave: {} children dispatched", children.len()),
                );
            }
            AgentEvent::DecompositionChildCompleted { label, success } => {
                let icon = if success { "✓" } else { "✗" };
                self.conversation.push_lifecycle(icon, &format!("Child '{label}' completed"));
            }
            AgentEvent::DecompositionCompleted { merged } => {
                let status = if merged { "merged" } else { "completed (no merge)" };
                self.conversation.push_lifecycle("⚡", &format!("Cleave {status}"));
            }
            AgentEvent::SystemNotification { message } => {
                // Transient notifications → toast; persistent ones → conversation
                if message.starts_with('⟳') || message.starts_with("Retrying") {
                    self.show_toast(&message, ratatui_toaster::ToastType::Warning);
                } else if message.starts_with('⚡') {
                    self.show_toast(&message, ratatui_toaster::ToastType::Info);
                } else {
                    self.conversation.push_system(&message);
                }
            }
            AgentEvent::HarnessStatusChanged { status_json } => {
                // Deserialize and update the footer's harness status snapshot
                if let Ok(status) = serde_json::from_value::<crate::status::HarnessStatus>(status_json) {
                    // Compare with previous status and show toasts for changes
                    if let Some(prev) = self.previous_harness_status.take() {
                        self.show_status_change_toasts(&prev, &status);
                    }
                    
                    // Update footer data and store current status as previous
                    self.footer_data.update_harness(status.clone());
                    self.previous_harness_status = Some(status);
                    
                    // Visual effect
                    self.effects.ping_footer(self.theme.as_ref());
                }
            }
            _ => {}
        }
    }
}

/// Run the interactive TUI. Returns when the user quits.
///
/// This spawns the ratatui event loop and communicates with the agent
/// coordinator through channels.
/// Configuration for the TUI — passed from main.
pub struct TuiConfig {
    pub cwd: String,
    pub is_oauth: bool,
    /// Pre-populated initial state so the first frame isn't empty.
    pub initial: TuiInitialState,
    /// Skip the splash animation on startup.
    pub no_splash: bool,
    /// Command definitions from bus features — shown in command palette.
    pub bus_commands: Vec<omegon_traits::CommandDefinition>,
    /// Shared handles for live dashboard updates during the session.
    pub dashboard_handles: dashboard::DashboardHandles,
    /// Initial prompt to queue after startup (sent automatically, TUI stays open).
    pub initial_prompt: Option<String>,
}

/// Initial state snapshot gathered during setup, before the TUI event loop starts.
/// Populates footer cards and dashboard on the very first frame.
#[derive(Default)]
pub struct TuiInitialState {
    pub total_facts: usize,
    pub focused_node: Option<dashboard::FocusedNodeSummary>,
    pub active_changes: Vec<dashboard::ChangeSummary>,
}

/// Path to the editor history file — persists across sessions.
/// Open a URL in the default browser (cross-platform).
pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(url).spawn(); }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/c", "start", url]).spawn(); }
}

/// Try to read image data from the system clipboard and save to a temp file.
///
/// Supports PNG, JPEG, TIFF, GIF, BMP, and WebP. On macOS uses `osascript`
/// to probe clipboard info and `pbpaste` or AppleScript for extraction.
/// On Linux uses `xclip`. Returns the temp file path on success.
fn clipboard_image_to_temp() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Ask the clipboard what types are available
        let info = std::process::Command::new("osascript")
            .args(["-e", "clipboard info"])
            .output()
            .ok()?;
        let info_str = String::from_utf8_lossy(&info.stdout);

        // Map clipboard UTI → (extension, pasteboard type for AppleScript)
        let formats: &[(&str, &str, &str)] = &[
            ("public.png",          "png",  "«class PNGf»"),
            ("public.jpeg",         "jpg",  "«class JPEG»"),
            ("public.tiff",         "tiff", "«class TIFF»"),
            ("com.compuserve.gif",  "gif",  "«class GIFf»"),
            ("com.microsoft.bmp",   "bmp",  "«class BMP »"),
            ("public.webp",         "webp", "«class PNGf»"), // WebP often comes as PNG on pasteboard
        ];

        let (ext, pb_type) = formats.iter()
            .find(|(uti, _, _)| info_str.contains(uti))
            .map(|(_, ext, pb)| (*ext, *pb))?;

        // Read the raw image data via AppleScript
        let script = format!(
            "set imgData to the clipboard as {pb_type}\nreturn imgData"
        );
        let output = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .ok()?;

        if !output.status.success() || output.stdout.is_empty() {
            return None;
        }

        // osascript returns the data with a «data ....» wrapper — extract raw bytes
        // Actually, osascript binary output is unreliable. Use a write-to-file approach instead.
        let tmp_dir = std::env::temp_dir();
        let filename = format!("omegon-clipboard-{}.{ext}", std::process::id());
        let tmp_path = tmp_dir.join(&filename);

        let write_script = format!(
            r#"set imgData to the clipboard as {pb_type}
set filePath to POSIX file "{}" as text
set fileRef to open for access file filePath with write permission
set eof fileRef to 0
write imgData to fileRef
close access fileRef"#,
            tmp_path.display()
        );

        let result = std::process::Command::new("osascript")
            .args(["-e", &write_script])
            .output()
            .ok()?;

        if result.status.success() && tmp_path.exists() {
            let meta = std::fs::metadata(&tmp_path).ok()?;
            if meta.len() > 0 {
                return Some(tmp_path);
            }
        }
        let _ = std::fs::remove_file(&tmp_path);
        None
    }

    #[cfg(target_os = "linux")]
    {
        // Try each MIME type in order of preference
        let types = &[
            ("image/png", "png"),
            ("image/jpeg", "jpg"),
            ("image/gif", "gif"),
            ("image/bmp", "bmp"),
            ("image/webp", "webp"),
            ("image/tiff", "tiff"),
        ];

        // Try wl-paste first (Wayland), fall back to xclip (X11)
        let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

        if is_wayland {
            // wl-paste: try each MIME type
            for &(mime, ext) in types.iter() {
                let output = std::process::Command::new("wl-paste")
                    .args(["--type", mime, "--no-newline"])
                    .output()
                    .ok();
                if let Some(output) = output {
                    if output.status.success() && !output.stdout.is_empty() {
                        let tmp_dir = std::env::temp_dir();
                        let filename = format!("omegon-clipboard-{}.{ext}", std::process::id());
                        let tmp_path = tmp_dir.join(&filename);
                        std::fs::write(&tmp_path, &output.stdout).ok()?;
                        return Some(tmp_path);
                    }
                }
            }
            return None;
        }

        // X11: use xclip
        let targets = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
            .output()
            .ok()?;
        let targets_str = String::from_utf8_lossy(&targets.stdout);

        let (mime, ext) = types.iter()
            .find(|(mime, _)| targets_str.contains(mime))
            .copied()?;

        let output = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", mime, "-o"])
            .output()
            .ok()?;

        if !output.status.success() || output.stdout.is_empty() {
            return None;
        }

        let tmp_dir = std::env::temp_dir();
        let filename = format!("omegon-clipboard-{}.{ext}", std::process::id());
        let tmp_path = tmp_dir.join(&filename);

        std::fs::write(&tmp_path, &output.stdout).ok()?;
        Some(tmp_path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn history_path(cwd: &str) -> std::path::PathBuf {
    let project_root = crate::setup::find_project_root(std::path::Path::new(cwd));
    project_root.join(".omegon").join("history")
}

fn sel_opt(value: &str, label: &str, desc: &str, current: &str) -> selector::SelectOption {
    selector::SelectOption {
        value: value.to_string(),
        label: label.to_string(),
        description: desc.to_string(),
        active: value == current,
    }
}

// ─── Milestone system ───────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Milestone {
    nodes: Vec<String>,
    frozen: bool,
}

fn load_milestones(path: &std::path::Path) -> std::collections::BTreeMap<String, Milestone> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_milestones(path: &std::path::Path, milestones: &std::collections::BTreeMap<String, Milestone>) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(milestones)?;
    std::fs::write(path, json)
}

// ─── Tutorial system ────────────────────────────────────────────────────

/// A single tutorial lesson.
#[derive(Debug, Clone)]
struct TutorialLesson {
    /// Filename (e.g. "01-cockpit.md")
    filename: String,
    /// Title from frontmatter
    title: String,
    /// The lesson prompt content (body after frontmatter)
    content: String,
}

/// Tutorial runner state — tracks lessons and progress.
#[derive(Debug)]
struct TutorialState {
    lessons: Vec<TutorialLesson>,
    current: usize, // 0-indexed
    tutorial_dir: std::path::PathBuf,
}

/// Persisted tutorial progress.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct TutorialProgress {
    current_lesson: usize,
    completed: Vec<usize>,
}

impl TutorialState {
    /// Load tutorial lessons from a directory.
    fn load(tutorial_dir: &std::path::Path) -> Option<Self> {
        if !tutorial_dir.is_dir() {
            return None;
        }

        let mut entries: Vec<_> = std::fs::read_dir(tutorial_dir).ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.ends_with(".md") && name.chars().next().is_some_and(|c| c.is_ascii_digit())
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        let mut lessons = Vec::new();
        for entry in entries {
            let filename = entry.file_name().to_string_lossy().to_string();
            let raw = std::fs::read_to_string(entry.path()).ok()?;
            let (title, content) = parse_lesson(&raw, &filename);
            lessons.push(TutorialLesson { filename, title, content });
        }

        if lessons.is_empty() {
            return None;
        }

        // Load progress
        let progress = load_tutorial_progress(tutorial_dir);
        let current = progress.current_lesson.min(lessons.len().saturating_sub(1));

        Some(Self {
            lessons,
            current,
            tutorial_dir: tutorial_dir.to_path_buf(),
        })
    }

    fn current_lesson(&self) -> &TutorialLesson {
        &self.lessons[self.current]
    }

    fn total(&self) -> usize {
        self.lessons.len()
    }

    fn is_last(&self) -> bool {
        self.current >= self.lessons.len() - 1
    }

    fn advance(&mut self) -> bool {
        if self.current < self.lessons.len() - 1 {
            self.current += 1;
            self.save_progress();
            true
        } else {
            false
        }
    }

    fn go_back(&mut self) -> bool {
        if self.current > 0 {
            self.current -= 1;
            self.save_progress();
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.current = 0;
        let progress_path = self.tutorial_dir.join("progress.json");
        let _ = std::fs::remove_file(progress_path);
    }

    fn save_progress(&self) {
        let progress = TutorialProgress {
            current_lesson: self.current,
            completed: (0..self.current).collect(),
        };
        let progress_path = self.tutorial_dir.join("progress.json");
        if let Ok(json) = serde_json::to_string_pretty(&progress) {
            let _ = std::fs::write(progress_path, json);
        }
    }

    fn status_line(&self) -> String {
        let lesson = self.current_lesson();
        format!(
            "Tutorial: lesson {}/{} — \"{}\"{}",
            self.current + 1,
            self.total(),
            lesson.title,
            if self.is_last() { " (final)" } else { "" }
        )
    }
}

fn parse_lesson(raw: &str, filename: &str) -> (String, String) {
    // Extract title from frontmatter if present
    let mut title = filename.trim_end_matches(".md").to_string();
    let content;

    if let Some(rest) = raw.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let frontmatter = &rest[..end];
            for line in frontmatter.lines() {
                if let Some(t) = line.strip_prefix("title:") {
                    title = t.trim().trim_matches('"').trim_matches('\'').to_string();
                }
            }
            content = rest[end + 4..].trim().to_string();
        } else {
            content = raw.to_string();
        }
    } else {
        content = raw.to_string();
    }

    (title, content)
}

fn load_tutorial_progress(tutorial_dir: &std::path::Path) -> TutorialProgress {
    let path = tutorial_dir.join("progress.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub async fn run_tui(
    mut events_rx: broadcast::Receiver<AgentEvent>,
    command_tx: mpsc::Sender<TuiCommand>,
    config: TuiConfig,
    cancel: SharedCancel,
    settings: crate::settings::SharedSettings,
) -> io::Result<()> {
    // Set up terminal with mouse capture for scroll events
    enable_raw_mode()?;

    // Initialize image protocol detection AFTER raw mode (suppresses echo)
    // but BEFORE alt screen (picker queries need the primary screen).
    image::init_picker();

    io::stdout().execute(EnterAlternateScreen)?;
    // Set the terminal's own background color to our theme bg.
    // This ensures the alternate screen buffer is filled with our color,
    // not the user's terminal profile background. Without this, crossterm's
    // diff optimizer may skip cells that haven't changed from the initial
    // state, leaving the terminal's native background visible.
    io::stdout().execute(crossterm::style::SetBackgroundColor(
        crossterm::style::Color::Rgb { r: 0, g: 1, b: 3 },
    ))?;
    // Clear the screen with our bg so every pixel starts owned.
    io::stdout().execute(crossterm::terminal::Clear(crossterm::terminal::ClearType::All))?;
    io::stdout().execute(EnableMouseCapture)?;
    io::stdout().execute(crossterm::event::EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Install panic hook that restores terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = io::stdout().execute(crossterm::event::DisableBracketedPaste);
        let _ = io::stdout().execute(DisableMouseCapture);
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    // Seed spinner from process start time for variety across sessions
    spinner::seed(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as usize)
        .unwrap_or(42));

    let mut app = App::new(settings);
    app.history = App::load_history(&config.cwd);
    app.footer_data.cwd = config.cwd;
    app.footer_data.is_oauth = config.is_oauth;
    app.bus_commands = config.bus_commands;
    app.dashboard_handles = config.dashboard_handles;
    app.cancel = cancel;

    // Pre-populate from initial state so first frame isn't empty
    app.footer_data.total_facts = config.initial.total_facts;
    app.dashboard.focused_node = config.initial.focused_node;
    app.dashboard.active_changes = config.initial.active_changes;

    // Build a contextual welcome message
    {
        let s = app.settings();
        let model_short = s.model_short();
        let project = app.footer_data.cwd.split('/').next_back().unwrap_or("project");
        let facts = app.footer_data.total_facts;
        let ctx = s.context_window / 1000;

        let version = env!("CARGO_PKG_VERSION");
        let sha = env!("OMEGON_GIT_SHA");
        let mut welcome = format!("Ω Omegon {version} ({sha}) — {project}");
        welcome.push_str(&format!("\n  ▸ {model_short}  ·  {ctx}k context"));
        if facts > 0 {
            welcome.push_str(&format!("  ·  {facts} facts loaded"));
        }
        welcome.push('\n');
        welcome.push_str("\n  /model  switch provider    /think  reasoning level");
        welcome.push_str("\n  /context  context class      /help   all commands");
        welcome.push_str("\n  Ctrl+R  search history      Ctrl+C  cancel/quit");

        app.conversation.push_system(&welcome);
    }

    // ── Splash screen ───────────────────────────────────────────────
    if !config.no_splash {
        let size = terminal.size()?;
        if let Some(mut splash) = splash::SplashScreen::new(size.width, size.height) {
            // Mark loading items done immediately — we load fast
            splash.set_load_state("providers", splash::LoadState::Active);
            splash.set_load_state("memory", splash::LoadState::Active);
            splash.set_load_state("tools", splash::LoadState::Active);

            // Run splash animation loop
            let splash_start = std::time::Instant::now();
            let safety_timeout = std::time::Duration::from_secs(5);

            loop {
                // Draw splash
                {
                    let t = &app.theme;
                    terminal.draw(|f| splash.draw(f, t.as_ref()))?;
                }

                // Poll for keypress at animation frame rate
                let interval = splash::SplashScreen::frame_interval();
                if event::poll(interval)?
                    && matches!(event::read()?, Event::Key(_))
                    && (splash.ready_to_dismiss()
                        || splash_start.elapsed() > std::time::Duration::from_millis(300))
                {
                    break;
                }

                splash.tick();

                // Cosmetic loading animation — the binary loads in ~50ms so
                // real subsystem tracking would be invisible. These frame
                // thresholds create a visual cascade for branding purposes.
                if splash.frame >= 8 {
                    splash.set_load_state("providers", splash::LoadState::Done);
                }
                if splash.frame >= 12 {
                    splash.set_load_state("memory", splash::LoadState::Done);
                }
                if splash.frame >= 16 {
                    splash.set_load_state("tools", splash::LoadState::Done);
                }

                // Drain agent events to prevent broadcast buffer overflow.
                // Events are silently discarded — the splash is a branding moment,
                // not functional UI.
                while events_rx.try_recv().is_ok() {}

                // Safety timeout
                if splash_start.elapsed() > safety_timeout {
                    splash.force_done();
                    break;
                }

                // Auto-dismiss after hold period
                if splash.ready_to_dismiss() && splash.hold_count > splash::HOLD_FRAMES + 30 {
                    break;
                }
            }
        }
    }

    // Queue startup reveal effects (footer sweep-in, conversation fade)
    {
        let t = &app.theme;
        app.effects.queue_startup(t.as_ref());
    }

    // Queue initial prompt if provided (--initial-prompt / --initial-prompt-file)
    if let Some(prompt) = config.initial_prompt {
        app.queue_prompt(prompt);
    }

    // First-run hint: suggest /tutorial if this looks like a fresh session.
    // "First run" = no .omegon/tutorial_completed marker AND no memory facts exist.
    // A project with memory facts has been used before — don't nag.
    {
        let omegon_dir = std::path::Path::new(&app.footer_data.cwd).join(".omegon");
        let tutorial_done = omegon_dir.join("tutorial_completed");
        let has_memory = omegon_dir.join("memory").join("facts.jsonl").exists()
            || std::path::Path::new(&app.footer_data.cwd).join(".pi").join("memory").join("facts.jsonl").exists();
        if !tutorial_done.exists() && !has_memory && app.queued_prompt.is_none() {
            app.show_toast(
                "Welcome to Omegon! Type /tutorial for an interactive guide.",
                ratatui_toaster::ToastType::Info,
            );
        }
    }

    loop {
        // ── Splash replay (/splash command) ─────────────────────────
        if app.replay_splash {
            app.replay_splash = false;
            let size = terminal.size()?;
            if let Some(mut splash) = splash::SplashScreen::new(size.width, size.height) {
                splash.force_done(); // No loading checklist on replay
                loop {
                    {
                        let t = &app.theme;
                        terminal.draw(|f| splash.draw(f, t.as_ref()))?;
                    }
                    let interval = splash::SplashScreen::frame_interval();
                    if event::poll(interval)? {
                        let ev = event::read()?;
                        // Any key or mouse click dismisses the replay
                        if matches!(ev, Event::Key(_) | Event::Mouse(_)) {
                            break;
                        }
                    }
                    splash.tick();
                    // Auto-end after full animation + hold
                    if splash.frame > splash::TOTAL_FRAMES + splash::HOLD_FRAMES + 20 {
                        break;
                    }
                }
            }
        }

        // Drain agent events BEFORE drawing — so telemetry counters
        // (memory_ops, tool_calls) are current when draw reads them
        while let Ok(agent_event) = events_rx.try_recv() {
            app.handle_agent_event(agent_event);
        }

        // Draw
        terminal.draw(|f| app.draw(f))?;

        // Poll for events with timeout (16ms ≈ 60fps)
        let has_terminal_event = event::poll(Duration::from_millis(16))?;

        if has_terminal_event {
            match event::read()? {
                // ── Mouse scroll ────────────────────────────────────────
                // macOS natural scrolling inverts at the OS level BEFORE
                // the terminal sees it. crossterm's ScrollUp means "the OS
                // sent scroll-up" which, with natural scrolling, means the
                // user swiped fingers DOWN (wanting to see newer content).
                // So: ScrollUp → scroll toward bottom, ScrollDown → scroll toward top.
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            // Terminal sends ScrollUp = show older content
                            // (natural scrolling is handled by the terminal emulator)
                            app.conversation.scroll_up(3);
                        }
                        MouseEventKind::ScrollDown => {
                            app.conversation.scroll_down(3);
                        }
                        _ => {}
                    }
                }
                // ── Paste — pass directly to textarea ──────────
                Event::Paste(ref text) => {
                    app.editor.textarea.insert_str(text);
                }
                // ── Ctrl+V: check for clipboard image ──────────
                Event::Key(KeyEvent { code: KeyCode::Char('v'), modifiers: KeyModifiers::CONTROL, .. }) => {
                    if let Some(path) = clipboard_image_to_temp() {
                        app.conversation.push_image(path.clone(), "clipboard paste");
                        app.show_toast("📎 Image pasted", ratatui_toaster::ToastType::Info);
                        // Store the path for attachment on next prompt
                        app.pending_image = Some(path);
                    }
                    // If no image, Ctrl+V is handled by crossterm as bracketed paste
                }
                Event::Key(key) => {
                // ── Selector popup intercepts all keys when open ────
                if app.selector.is_some() {
                    match key.code {
                        KeyCode::Up => { if let Some(ref mut s) = app.selector { s.move_up(); } }
                        KeyCode::Down => { if let Some(ref mut s) = app.selector { s.move_down(); } }
                        KeyCode::Enter => {
                            if let Some(msg) = app.confirm_selector(&command_tx) {
                                app.conversation.push_system(&msg);
                            }
                        }
                        KeyCode::Esc => {
                            app.selector = None;
                            app.selector_kind = None;
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── Reverse search mode intercepts keys ─────────
                if matches!(app.editor.mode(), editor::EditorMode::ReverseSearch { .. }) {
                    match key.code {
                        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+R again: search further back
                            app.editor.search_prev(&app.history);
                        }
                        KeyCode::Char(c) => {
                            app.editor.search_insert(c);
                            app.editor.search_update(&app.history);
                        }
                        KeyCode::Backspace => {
                            app.editor.search_backspace();
                            app.editor.search_update(&app.history);
                        }
                        KeyCode::Enter => {
                            app.editor.accept_search(&app.history);
                        }
                        KeyCode::Esc => {
                            app.editor.cancel_search();
                        }
                        _ => {
                            // Any other key: accept search + process key normally
                            app.editor.accept_search(&app.history);
                        }
                    }
                    continue;
                }

                // ── Tutorial overlay input ────────────────────────
                // Tab advances, Shift+Tab goes back, Escape dismisses.
                // Tab is unambiguous — doesn't conflict with Enter (submit)
                // or any other keybinding.
                if let Some(ref mut tut) = app.tutorial_overlay {
                    if tut.active {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                tut.dismiss();
                                app.mark_tutorial_completed();
                                continue;
                            }
                            (KeyCode::Tab, mods) if mods.contains(KeyModifiers::SHIFT) => {
                                tut.go_back();
                                continue;
                            }
                            (KeyCode::Tab, _) => {
                                if tut.step().trigger == tutorial::Trigger::Enter {
                                    tut.advance();
                                    if !tut.active { app.mark_tutorial_completed(); }
                                }
                                // For Command/AnyInput triggers, Tab does nothing —
                                // the operator must do the action
                                continue;
                            }
                            _ => {}
                        }
                    }
                }

                match (key.code, key.modifiers) {
                    // ── Interrupt: Escape or Ctrl+C ─────────────────
                    (KeyCode::Esc, _) => {
                        if app.agent_active {
                            app.interrupt();
                            app.agent_active = false; // Unblock editor immediately
                            app.conversation.finalize_message();
                            app.conversation.push_system("⎋ Interrupted");
                        }
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        if app.agent_active {
                            app.interrupt();
                            app.agent_active = false; // Unblock editor immediately
                            app.conversation.finalize_message();
                            app.conversation.push_system("⎋ Interrupted (Ctrl+C)");
                        } else if !app.editor.is_empty() {
                            // Clear the line first (like a real terminal)
                            app.editor.clear_line();
                            app.last_ctrl_c = None;
                        } else {
                            // Empty editor — double Ctrl+C to quit
                            let now = std::time::Instant::now();
                            if let Some(last) = app.last_ctrl_c {
                                if now.duration_since(last).as_millis() < 1000 {
                                    app.should_quit = true;
                                    let _ = command_tx.send(TuiCommand::Quit).await;
                                } else {
                                    app.last_ctrl_c = Some(now);
                                    app.conversation.push_system("Press Ctrl+C again to quit");
                                }
                            } else {
                                app.last_ctrl_c = Some(now);
                                app.conversation.push_system("Press Ctrl+C again to quit");
                            }
                        }
                    }

                    // ── Editor: word/line operations (idle only) ────
                    (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                        app.editor.delete_word_backward();
                    }
                    (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                        app.editor.clear_line();
                    }
                    (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                        app.editor.kill_to_end();
                    }
                    (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                        app.editor.yank();
                    }
                    (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                        app.editor.move_home();
                    }
                    (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                        app.editor.move_end();
                    }
                    (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                        app.editor.start_reverse_search();
                    }

                    // Meta (Alt) key combos for word operations
                    (KeyCode::Backspace, KeyModifiers::ALT) => {
                        app.editor.delete_word_backward();
                    }
                    (KeyCode::Char('d'), KeyModifiers::ALT) => {
                        app.editor.delete_word_forward();
                    }
                    (KeyCode::Char('b'), KeyModifiers::ALT) => {
                        app.editor.move_word_backward();
                    }
                    (KeyCode::Char('f'), KeyModifiers::ALT) => {
                        app.editor.move_word_forward();
                    }

                    // Tab: command completion when typing a slash command
                    (KeyCode::Tab, _) => {
                        let text = app.editor.render_text().to_string();
                        if text.starts_with('/') {
                            let matches = app.matching_commands();
                            if matches.len() == 1 {
                                let cmd = format!("/{}", matches[0].0);
                                app.editor.set_text(&cmd);
                            }
                        }
                    }

                    // Ctrl+O: toggle tool card expansion (shadows pi's output toggle)
                    (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                        if let Some(idx) = app.conversation.focused_tool_card() {
                            app.conversation.toggle_expand(idx);
                        }
                    }

                    // Submit
                    (KeyCode::Enter, _) => {
                        let text = app.editor.take_text();
                        if !text.is_empty() {
                            // Slash commands always execute immediately
                            if text.starts_with('/') {
                                // Notify tutorial overlay of slash command
                                if let Some(ref mut tut) = app.tutorial_overlay {
                                    let cmd = text[1..].split_whitespace().next().unwrap_or("");
                                    tut.check_command(cmd);
                                    if !tut.active { app.mark_tutorial_completed(); }
                                }
                                match app.handle_slash_command(&text, &command_tx) {
                                    SlashResult::Display(response) => {
                                        app.conversation.push_system(&response);
                                    }
                                    SlashResult::Handled => {}
                                    SlashResult::Quit => {
                                        app.should_quit = true;
                                        let _ = command_tx.send(TuiCommand::Quit).await;
                                    }
                                    SlashResult::NotACommand => {
                                        // Tutorial overlay: AnyInput trigger
                                        if let Some(ref mut tut) = app.tutorial_overlay {
                                            tut.check_any_input();
                                            if !tut.active { app.mark_tutorial_completed(); }
                                        }
                                        if app.agent_active {
                                            app.queue_prompt(text.clone());
                                        } else {
                                            app.conversation.push_user(&text);
                                            app.history.push(text.clone());
                                            app.history_idx = None;
                                            app.agent_active = true;
                                            let _ = command_tx.send(TuiCommand::UserPrompt(text)).await;
                                        }
                                    }
                                }
                            } else {
                                // Non-slash text — check tutorial AnyInput trigger
                                if let Some(ref mut tut) = app.tutorial_overlay {
                                    tut.check_any_input();
                                    if !tut.active { app.mark_tutorial_completed(); }
                                }

                                if app.agent_active {
                                    // Agent busy — queue the prompt
                                    app.queue_prompt(text.clone());
                                } else {
                                    // Agent idle — send immediately
                                    app.conversation.push_user(&text);
                                    app.history.push(text.clone());
                                    app.history_idx = None;
                                    app.agent_active = true;
                                    if let Some(img) = app.pending_image.take() {
                                        let _ = command_tx.send(TuiCommand::UserPromptWithImages(text, vec![img])).await;
                                    } else {
                                        let _ = command_tx.send(TuiCommand::UserPrompt(text)).await;
                                    }
                                }
                            }
                        }
                    }

                    // Basic editing — only insert if no Ctrl modifier
                    // (Ctrl+letter arms above handle those explicitly)
                    (KeyCode::Char(c), mods) if !mods.contains(KeyModifiers::CONTROL) => {
                        app.editor.insert(c);
                    }
                    (KeyCode::Backspace, _) => {
                        app.editor.backspace();
                    }
                    (KeyCode::Left, KeyModifiers::ALT) => {
                        app.editor.move_word_backward();
                    }
                    (KeyCode::Right, KeyModifiers::ALT) => {
                        app.editor.move_word_forward();
                    }
                    (KeyCode::Left, _) => {
                        app.editor.move_left();
                    }
                    (KeyCode::Right, _) => {
                        app.editor.move_right();
                    }
                    (KeyCode::Home, _) => {
                        app.editor.move_home();
                    }
                    (KeyCode::End, _) => {
                        app.editor.move_end();
                    }

                    // ── Scrolling ────────────────────────────────
                    (KeyCode::Up, KeyModifiers::SHIFT) => {
                        app.conversation.scroll_up(3);
                    }
                    (KeyCode::Down, KeyModifiers::SHIFT) => {
                        app.conversation.scroll_down(3);
                    }
                    (KeyCode::PageUp, _) => {
                        app.conversation.scroll_up(20);
                    }
                    (KeyCode::PageDown, _) => {
                        app.conversation.scroll_down(20);
                    }
                    (KeyCode::Up, _) => {
                        if app.agent_active || !app.editor.render_text().is_empty() {
                            app.conversation.scroll_up(3);
                        } else {
                            app.history_up();
                        }
                    }
                    (KeyCode::Down, _) => {
                        if app.agent_active || !app.editor.render_text().is_empty() {
                            app.conversation.scroll_down(3);
                        } else {
                            app.history_down();
                        }
                    }
                    _ => {}
                }
            } // Event::Key
            _ => {} // Other events (resize, etc.)
        } // match event::read()
        } // if has_terminal_event

        // Agent events already drained before draw (above).

        // Drain queued prompt after agent finishes (but not if quitting)
        if !app.agent_active && !app.should_quit && app.queued_prompt.is_some() {
            let text = app.queued_prompt.take().unwrap();
            app.conversation.push_user(&text);
            app.history.push(text.clone());
            app.history_idx = None;
            app.agent_active = true;
            let _ = command_tx.send(TuiCommand::UserPrompt(text)).await;
        }

        if app.should_quit {
            break;
        }
    }

    // Save history before restoring terminal
    app.save_history();

    // Restore terminal
    io::stdout().execute(crossterm::event::DisableBracketedPaste)?;
    io::stdout().execute(DisableMouseCapture)?;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
