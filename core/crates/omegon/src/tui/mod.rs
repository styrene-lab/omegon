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
pub mod conv_widget;
pub mod conversation;
pub mod dashboard;
pub mod editor;
pub mod effects;
pub mod footer;
pub mod image;
pub mod instruments;
pub mod model_catalog;
pub mod segments;
pub mod selector;
pub mod spinner;
pub mod splash;
pub mod theme;
pub mod tutorial;
pub mod widget_renderer;
pub mod widgets;

#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tests;

use std::io;
use std::time::Duration;

use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use omegon_traits::AgentEvent;

use self::conversation::{ConversationView, Tab};
use self::dashboard::DashboardState;
use self::editor::Editor;
use self::footer::{FooterData, SessionUsageSlice};
use self::instruments::InstrumentPanel;
use self::segments::{
    SegmentContent, SegmentExportMode, SegmentRenderMode, ToolVisualKind, build_meta_tag,
};

#[derive(Debug, Clone)]
pub struct PromptSubmission {
    pub text: String,
    pub image_paths: Vec<std::path::PathBuf>,
    pub submitted_by: String,
    pub via: &'static str,
    pub queue_mode: PromptQueueMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptQueueMode {
    #[default]
    InterruptAfterTurn,
    UntilReady,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptPrefixMode {
    Agent,
    Bash,
    Context,
    MemoryInject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateSeverity {
    Available,
    StaleMinor,
}

/// Messages from TUI to the agent coordinator.
#[derive(Debug)]
pub enum TuiCommand {
    /// User submitted a prompt with optional image attachments.
    SubmitPrompt(PromptSubmission),
    /// Execute a local shell command directly without LLM mediation.
    RunShellCommand {
        command: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Temporarily hand terminal control to the operator's real shell.
    /// Carries the keyboard-enhancement flag so the handler can pop/push
    /// the Kitty protocol around the subprocess without querying the
    /// terminal again (which can fail if stdin is redirected).
    ShellHandoff { keyboard_enhancement: bool },
    /// User wants to quit (double Ctrl+C, or /exit).
    Quit,
    /// Show current model/provider posture.
    ModelView {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Show available models.
    ModelList {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Switch the model for the next turn.
    SetModel {
        model: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Set the thinking level.
    SetThinking {
        level: crate::settings::ThinkingLevel,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Execute a canonical control request directly.
    ExecuteControl {
        request: crate::control_runtime::ControlRequest,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Execute canonical slash semantics from a non-TUI caller.
    RunSlashCommand {
        name: String,
        args: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::SlashCommandResponse>>,
    },
    /// Dispatch a bus command from a feature (name, args).
    BusCommand { name: String, args: String },
    /// Trigger manual compaction.
    Compact,
    /// Show context usage and status.
    ContextStatus {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Compress context and clear history.
    ContextCompact {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Clear context completely (fresh start).
    ContextClear {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// List saved sessions.
    ListSessions {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Start the local browser surface server used by Auspex compatibility flows.
    StartWebDashboard,
    /// Discard the current session and start fresh (saves current first).
    NewSession {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Probe and report auth/provider status.
    AuthStatus {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Start provider login flow.
    AuthLogin {
        provider: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Log out a provider.
    AuthLogout {
        provider: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Unlock secrets/auth backend.
    AuthUnlock {
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
}

/// Shared cancel token — the TUI writes it on Escape/Ctrl+C,
/// the agent loop checks it. Arc so both tasks can access it.
pub type SharedCancel = std::sync::Arc<std::sync::Mutex<Option<CancellationToken>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneFocus {
    Editor,
    Conversation,
    Dashboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiMode {
    Full,
    Slim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UiSurfaces {
    dashboard: bool,
    instruments: bool,
    footer: bool,
}

impl UiSurfaces {
    fn full() -> Self {
        Self {
            dashboard: true,
            instruments: true,
            footer: true,
        }
    }

    fn slim() -> Self {
        Self {
            dashboard: false,
            instruments: false,
            footer: false,
        }
    }
}

struct OperatorEvent {
    message: String,
    color: Color,
    icon: &'static str,
    expires_at: std::time::Instant,
}

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
    /// Last on-screen dashboard area for mouse hit-testing.
    dashboard_area: Option<Rect>,
    /// Last on-screen conversation area for mouse hit-testing.
    conversation_area: Option<Rect>,
    /// Last on-screen editor area for mouse hit-testing.
    editor_area: Option<Rect>,
    /// Which pane currently owns pointer-driven interaction.
    pane_focus: PaneFocus,
    footer_data: FooterData,
    /// CIC instrument panel for telemetry visualization
    instrument_panel: InstrumentPanel,
    /// Focus mode toggle state
    focus_mode: bool,
    ui_mode: UiMode,
    ui_surfaces: UiSurfaces,
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
    /// Active @-file picker popup.
    at_picker: Option<selector::Selector>,
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
    /// Last instrument telemetry update timestamp.
    last_instrument_update: std::time::Instant,
    /// Child tokens already rolled into session_input/output_tokens to avoid double-counting.
    cleave_tokens_accounted_in: u64,
    cleave_tokens_accounted_out: u64,
    /// Turn counter for throttled dashboard refresh.
    dashboard_refresh_turn: u32,
    /// Web dashboard server startup payload (if running).
    web_startup: Option<crate::web::WebStartupInfo>,
    /// Parsed web dashboard socket address (legacy/debug convenience).
    web_server_addr: Option<std::net::SocketAddr>,
    /// Prompts queued while the agent is busy — drained only after authoritative AgentEnd.
    queued_prompts: std::collections::VecDeque<(String, Vec<std::path::PathBuf>)>,
    /// Local default queue policy for interactive submissions.
    queue_mode: PromptQueueMode,
    /// Inline operator-facing transient events (replaces floating toasts).
    operator_events: std::collections::VecDeque<OperatorEvent>,
    /// Previous harness status for diffing on HarnessStatusChanged.
    previous_harness_status: Option<crate::status::HarnessStatus>,
    /// Capability tier detected at startup by systems check probes.
    pub capability_tier: Option<crate::startup::CapabilityTier>,
    /// Tutorial state — active when running /tutorial (lesson-based).
    tutorial: Option<TutorialState>,
    /// Tutorial overlay — game-style first-play advisor.
    /// Renders on top of the UI and guides the operator through steps.
    tutorial_overlay: Option<tutorial::Tutorial>,
    /// Update checker — receives notification when a newer version is available.
    update_rx: Option<crate::update::UpdateReceiver>,
    /// Update checker sender — allows re-checking when channel changes.
    update_tx: Option<crate::update::UpdateSender>,
    /// Headless login prompt — when set, the next Enter submits to the login
    /// flow instead of the agent. Populated by the LoginPrompt callback.
    login_prompt_tx:
        std::sync::Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    /// Whether we enabled the Kitty keyboard protocol (must pop on cleanup).
    keyboard_enhancement: bool,
    /// Whether crossterm mouse capture is enabled.
    mouse_capture_enabled: bool,
    /// When true, terminal-native selection/copy mode is active.
    terminal_copy_mode: bool,
    /// Last left-click press used to detect double-click expansion.
    last_left_click: Option<(u16, u16, std::time::Instant)>,
    /// Extension widgets discovered during setup — keyed by widget_id.
    extension_widgets: std::collections::HashMap<String, crate::extensions::ExtensionTabWidget>,
    /// Broadcast receivers for widget events — one per extension.
    widget_receivers: Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
    /// Active ephemeral modal from extension widget (widget_id, data, auto_dismiss_ms, spawn_time).
    active_modal: Option<(String, serde_json::Value, Option<u64>, std::time::Instant)>,
    /// Active action prompt from extension widget (widget_id, actions).
    active_action_prompt: Option<(String, Vec<String>)>,
    /// Whether the Anthropic subscription ToS notice has been shown this session.
    /// Shown once on first interactive session with an OAuth-only credential.
    oauth_tos_notice_shown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SelectorKind {
    Model,
    ThinkingLevel,
    ContextClass,
    Persona,
    Tone,
    SecretName,
    LoginProvider,
    VaultConfigure,
    UpdateChannel,
    WorkspaceRole,
    WorkspaceKind,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CanonicalSlashCommand {
    ModelList,
    SetModel(String),
    SetThinking(crate::settings::ThinkingLevel),
    StatusView,
    WorkspaceStatusView,
    WorkspaceListView,
    WorkspaceNew(String),
    WorkspaceDestroy(String),
    WorkspaceAdopt,
    WorkspaceRelease,
    WorkspaceArchive,
    WorkspacePrune,
    WorkspaceBindMilestone(String),
    WorkspaceBindNode(String),
    WorkspaceBindClear,
    WorkspaceRoleView,
    WorkspaceRoleSet(crate::workspace::types::WorkspaceRole),
    WorkspaceRoleClear,
    WorkspaceKindView,
    WorkspaceKindSet(crate::workspace::types::WorkspaceKind),
    WorkspaceKindClear,
    SessionStatsView,
    TreeView { args: String },
    NoteAdd { text: String },
    NotesView,
    NotesClear,
    CheckinView,
    ContextStatus,
    ContextCompact,
    ContextClear,
    ContextRequest { kind: String, query: String },
    ContextRequestJson(String),
    SetContextClass(crate::settings::ContextClass),
    NewSession,
    ListSessions,
    AuthStatus,
    AuthUnlock,
    AuthLogin(String),
    AuthLogout(String),
    SkillsView,
    SkillsInstall,
    PluginView,
    PluginInstall(String),
    PluginRemove(String),
    PluginUpdate(Option<String>),
    SecretsView,
    SecretsSet { name: String, value: String },
    SecretsGet(String),
    SecretsDelete(String),
    VaultStatus,
    VaultUnseal,
    VaultLogin,
    VaultConfigure,
    VaultInitPolicy,
    CleaveStatus,
    CleaveCancelChild(String),
    DelegateStatus,
}

pub(crate) fn canonical_slash_command(cmd: &str, args: &str) -> Option<CanonicalSlashCommand> {
    let args = args.trim();
    match cmd {
        "model" if args == "list" => Some(CanonicalSlashCommand::ModelList),
        "model" if !args.is_empty() => Some(CanonicalSlashCommand::SetModel(args.to_string())),
        "think" => {
            crate::settings::ThinkingLevel::parse(args).map(CanonicalSlashCommand::SetThinking)
        }
        "status" if args.is_empty() => Some(CanonicalSlashCommand::StatusView),
        "workspace" if args.is_empty() => Some(CanonicalSlashCommand::WorkspaceStatusView),
        "workspace" if args == "status" => Some(CanonicalSlashCommand::WorkspaceStatusView),
        "workspace" if args == "list" => Some(CanonicalSlashCommand::WorkspaceListView),
        "workspace" if args == "adopt" => Some(CanonicalSlashCommand::WorkspaceAdopt),
        "workspace" if args == "release" => Some(CanonicalSlashCommand::WorkspaceRelease),
        "workspace" if args == "archive" => Some(CanonicalSlashCommand::WorkspaceArchive),
        "workspace" if args == "prune" => Some(CanonicalSlashCommand::WorkspacePrune),
        "workspace" if args == "bind clear" => Some(CanonicalSlashCommand::WorkspaceBindClear),
        "workspace" if args == "role" => Some(CanonicalSlashCommand::WorkspaceRoleView),
        "workspace" if args == "role clear" => Some(CanonicalSlashCommand::WorkspaceRoleClear),
        "workspace" if args == "kind" => Some(CanonicalSlashCommand::WorkspaceKindView),
        "workspace" if args == "kind clear" => Some(CanonicalSlashCommand::WorkspaceKindClear),
        "workspace" => {
            if let Some(label) = args
                .strip_prefix("new ")
                .map(str::trim)
                .filter(|label| !label.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceNew(label.to_string()))
            } else if let Some(target) = args
                .strip_prefix("destroy ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceDestroy(target.to_string()))
            } else if let Some(milestone) = args
                .strip_prefix("bind milestone ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceBindMilestone(
                    milestone.to_string(),
                ))
            } else if let Some(node) = args
                .strip_prefix("bind node ")
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::WorkspaceBindNode(node.to_string()))
            } else if let Some(role) = args
                .strip_prefix("role set ")
                .and_then(crate::workspace::types::WorkspaceRole::parse)
            {
                Some(CanonicalSlashCommand::WorkspaceRoleSet(role))
            } else {
                args.strip_prefix("kind set ")
                    .and_then(crate::workspace::types::WorkspaceKind::parse)
                    .map(CanonicalSlashCommand::WorkspaceKindSet)
            }
        }
        "stats" if args.is_empty() => Some(CanonicalSlashCommand::SessionStatsView),
        "tree" => Some(CanonicalSlashCommand::TreeView {
            args: if args.is_empty() {
                "list".to_string()
            } else {
                args.to_string()
            },
        }),
        "note" if !args.is_empty() => Some(CanonicalSlashCommand::NoteAdd {
            text: args.to_string(),
        }),
        "notes" if args.is_empty() => Some(CanonicalSlashCommand::NotesView),
        "notes" if args == "clear" => Some(CanonicalSlashCommand::NotesClear),
        "checkin" if args.is_empty() => Some(CanonicalSlashCommand::CheckinView),
        "context" if !args.is_empty() => {
            let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
            match sub {
                "status" => Some(CanonicalSlashCommand::ContextStatus),
                "compact" | "compress" => Some(CanonicalSlashCommand::ContextCompact),
                "clear" => Some(CanonicalSlashCommand::ContextClear),
                "request" => {
                    if rest.starts_with('{') {
                        match serde_json::from_str::<serde_json::Value>(rest) {
                            Ok(value)
                                if value.get("requests").and_then(|v| v.as_array()).is_some() =>
                            {
                                Some(CanonicalSlashCommand::ContextRequestJson(rest.to_string()))
                            }
                            _ => None,
                        }
                    } else {
                        let (kind, query) = rest.split_once(' ').unwrap_or((rest, ""));
                        if !kind.is_empty() && !query.trim().is_empty() {
                            Some(CanonicalSlashCommand::ContextRequest {
                                kind: kind.to_string(),
                                query: query.trim().to_string(),
                            })
                        } else {
                            None
                        }
                    }
                }
                _ => crate::settings::ContextClass::parse(sub)
                    .map(CanonicalSlashCommand::SetContextClass),
            }
        }
        "new" if args.is_empty() => Some(CanonicalSlashCommand::NewSession),
        "sessions" if args.is_empty() => Some(CanonicalSlashCommand::ListSessions),
        "auth" => match args {
            "" | "status" => Some(CanonicalSlashCommand::AuthStatus),
            "unlock" => Some(CanonicalSlashCommand::AuthUnlock),
            _ => None,
        },
        "login" if !args.is_empty() => Some(CanonicalSlashCommand::AuthLogin(args.to_string())),
        "logout" if !args.is_empty() => Some(CanonicalSlashCommand::AuthLogout(args.to_string())),
        "skills" => match args {
            "" | "list" => Some(CanonicalSlashCommand::SkillsView),
            "install" => Some(CanonicalSlashCommand::SkillsInstall),
            _ => None,
        },
        "plugin" => {
            if args.is_empty() || args == "list" {
                Some(CanonicalSlashCommand::PluginView)
            } else if let Some(uri) = args.strip_prefix("install ") {
                let uri = uri.trim();
                (!uri.is_empty()).then(|| CanonicalSlashCommand::PluginInstall(uri.to_string()))
            } else if let Some(name) = args.strip_prefix("remove ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::PluginRemove(name.to_string()))
            } else if args == "update" {
                Some(CanonicalSlashCommand::PluginUpdate(None))
            } else if let Some(name) = args.strip_prefix("update ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::PluginUpdate(Some(name.to_string())))
            } else {
                None
            }
        }
        "secrets" => {
            let parts: Vec<&str> = args.splitn(3, ' ').collect();
            match parts.first().copied().unwrap_or("") {
                "" | "list" => Some(CanonicalSlashCommand::SecretsView),
                "set" if parts.len() >= 3 => Some(CanonicalSlashCommand::SecretsSet {
                    name: parts[1].trim().to_string(),
                    value: parts[2].trim().to_string(),
                }),
                "get" if parts.len() >= 2 => Some(CanonicalSlashCommand::SecretsGet(
                    parts[1].trim().to_string(),
                )),
                "delete" if parts.len() >= 2 => Some(CanonicalSlashCommand::SecretsDelete(
                    parts[1].trim().to_string(),
                )),
                _ => None,
            }
        }
        "vault" => match args {
            "" | "status" => Some(CanonicalSlashCommand::VaultStatus),
            "unseal" => Some(CanonicalSlashCommand::VaultUnseal),
            "login" => Some(CanonicalSlashCommand::VaultLogin),
            "configure" => Some(CanonicalSlashCommand::VaultConfigure),
            "init-policy" => Some(CanonicalSlashCommand::VaultInitPolicy),
            _ => None,
        },
        "cleave" => {
            if args.is_empty() || args == "status" {
                Some(CanonicalSlashCommand::CleaveStatus)
            } else if let Some(label) = args.strip_prefix("cancel ") {
                let label = label.trim();
                (!label.is_empty())
                    .then(|| CanonicalSlashCommand::CleaveCancelChild(label.to_string()))
            } else {
                None
            }
        }
        "delegate" => match args {
            "" | "status" => Some(CanonicalSlashCommand::DelegateStatus),
            _ => None,
        },
        _ => None,
    }
}

/// Compute dynamic editor height from the editor's wrapped visual rows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AuspexHandoffMode {
    Env,
    BrowserUrl,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct AuspexAttachPayload {
    version: u16,
    transport: String,
    preferred_handoff: AuspexHandoffMode,
    startup_url: String,
    http_base: String,
    ws_url: String,
    ws_token: String,
    instance: Option<omegon_traits::OmegonInstanceDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuspexProbe {
    target: String,
    source: &'static str,
    compatibility: AuspexCompatibility,
    handoff_modes: Vec<AuspexHandoffMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuspexCompatibility {
    Unknown,
    Compatible,
    Incompatible(String),
}

fn launch_auspex_with_startup(startup: &crate::web::WebStartupInfo) -> anyhow::Result<String> {
    let probe = detect_auspex_target().ok_or_else(|| {
        anyhow::anyhow!("Auspex not detected. Set AUSPEX_BIN or install Auspex first.")
    })?;
    if let AuspexCompatibility::Incompatible(reason) = &probe.compatibility {
        anyhow::bail!(
            "Auspex detected at {} but is not compatible: {reason}",
            probe.target
        );
    }

    let target = probe.target;
    let preferred_handoff = if probe.handoff_modes.contains(&AuspexHandoffMode::Env) {
        AuspexHandoffMode::Env
    } else {
        AuspexHandoffMode::BrowserUrl
    };
    let attach_payload = build_auspex_attach_payload(startup, preferred_handoff.clone())?;

    if matches!(preferred_handoff, AuspexHandoffMode::BrowserUrl) {
        open_browser(&startup.http_base);
        return Ok(format!("{target} via browser-url"));
    }

    let mut command = if let Some(explicit) = target.strip_prefix("AUSPEX_BIN=") {
        std::process::Command::new(explicit)
    } else if target.ends_with(".app") {
        #[cfg(target_os = "macos")]
        {
            let mut cmd = std::process::Command::new("open");
            cmd.arg("-a").arg(target.clone());
            cmd
        }
        #[cfg(not(target_os = "macos"))]
        {
            std::process::Command::new(target.clone())
        }
    } else {
        std::process::Command::new(target.clone())
    };

    command
        .env("AUSPEX_OMEGON_STARTUP_URL", startup.startup_url.clone())
        .env("AUSPEX_OMEGON_WS_URL", startup.ws_url.clone())
        .env("AUSPEX_OMEGON_WS_TOKEN", startup.token.clone())
        .env("AUSPEX_OMEGON_ATTACH_JSON", attach_payload.clone())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(target_os = "macos")]
    if target.ends_with(".app") {
        command
            .arg("--env")
            .arg(format!("AUSPEX_OMEGON_STARTUP_URL={}", startup.startup_url));
        command
            .arg("--env")
            .arg(format!("AUSPEX_OMEGON_WS_URL={}", startup.ws_url));
        command
            .arg("--env")
            .arg(format!("AUSPEX_OMEGON_WS_TOKEN={}", startup.token));
        command
            .arg("--env")
            .arg(format!("AUSPEX_OMEGON_ATTACH_JSON={attach_payload}"));
    }

    command.spawn()?;
    Ok(format!("{target} via env"))
}

fn build_auspex_attach_payload(
    startup: &crate::web::WebStartupInfo,
    preferred_handoff: AuspexHandoffMode,
) -> anyhow::Result<String> {
    let payload = AuspexAttachPayload {
        version: 1,
        transport: "omegon-ipc".into(),
        preferred_handoff,
        startup_url: startup.startup_url.clone(),
        http_base: startup.http_base.clone(),
        ws_url: startup.ws_url.clone(),
        ws_token: startup.token.clone(),
        instance: startup.instance_descriptor.clone(),
    };
    serde_json::to_string(&payload).map_err(Into::into)
}

fn parse_handoff_modes(value: &serde_json::Value) -> Vec<AuspexHandoffMode> {
    let Some(modes) = value.get("handoff_modes").and_then(|v| v.as_array()) else {
        return vec![AuspexHandoffMode::Env];
    };
    let parsed: Vec<AuspexHandoffMode> = modes
        .iter()
        .filter_map(|mode| match mode.as_str() {
            Some("env") => Some(AuspexHandoffMode::Env),
            Some("browser-url") => Some(AuspexHandoffMode::BrowserUrl),
            _ => None,
        })
        .collect();
    if parsed.is_empty() {
        vec![AuspexHandoffMode::Env]
    } else {
        parsed
    }
}

fn probe_auspex_target(target: &str) -> (AuspexCompatibility, Vec<AuspexHandoffMode>) {
    if target.ends_with(".app") {
        return (AuspexCompatibility::Unknown, vec![AuspexHandoffMode::Env]);
    }
    let bin = target.strip_prefix("AUSPEX_BIN=").unwrap_or(target);
    let output = std::process::Command::new(bin)
        .arg("--omegon-compat")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let Ok(output) = output else {
        return (AuspexCompatibility::Unknown, vec![AuspexHandoffMode::Env]);
    };
    if !output.status.success() {
        return (AuspexCompatibility::Unknown, vec![AuspexHandoffMode::Env]);
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
        return (AuspexCompatibility::Unknown, vec![AuspexHandoffMode::Env]);
    };
    let modes = parse_handoff_modes(&value);
    let Some(protocol) = value.get("omegon_ipc_protocol").and_then(|v| v.as_u64()) else {
        return (AuspexCompatibility::Unknown, modes);
    };
    if protocol == omegon_traits::IPC_PROTOCOL_VERSION as u64 {
        (AuspexCompatibility::Compatible, modes)
    } else {
        (
            AuspexCompatibility::Incompatible(format!(
                "reported omegon_ipc_protocol={protocol}, expected {}",
                omegon_traits::IPC_PROTOCOL_VERSION
            )),
            modes,
        )
    }
}

fn path_contains_executable(candidate: &std::path::Path) -> bool {
    candidate.is_file()
}

fn detect_auspex_target() -> Option<AuspexProbe> {
    if let Ok(explicit) = std::env::var("AUSPEX_BIN") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            let path = std::path::Path::new(trimmed);
            if path_contains_executable(path) {
                let target = format!("AUSPEX_BIN={trimmed}");
                let (compatibility, handoff_modes) = probe_auspex_target(&target);
                return Some(AuspexProbe {
                    compatibility,
                    handoff_modes,
                    target,
                    source: "env",
                });
            }
        }
    }

    if let Ok(path_env) = std::env::var("PATH") {
        for entry in std::env::split_paths(&path_env) {
            let candidate = entry.join("auspex");
            if path_contains_executable(&candidate) {
                let target = candidate.display().to_string();
                let (compatibility, handoff_modes) = probe_auspex_target(&target);
                return Some(AuspexProbe {
                    compatibility,
                    handoff_modes,
                    target,
                    source: "path",
                });
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let app_bundle = std::path::Path::new("/Applications/Auspex.app");
        if app_bundle.exists() {
            return Some(AuspexProbe {
                compatibility: AuspexCompatibility::Unknown,
                handoff_modes: vec![AuspexHandoffMode::Env],
                target: app_bundle.display().to_string(),
                source: "app-bundle",
            });
        }
    }

    None
}

fn editor_height_for(editor: &Editor, main_area: Rect) -> u16 {
    let content_width = main_area.width.saturating_sub(2).max(1);
    let editor_rows = editor.visual_line_count(content_width) as u16;
    let max_editor = (main_area.height * 40 / 100).max(5).min(20);
    (editor_rows + 2).clamp(3, max_editor) // +2 for border
}

/// Compact one-line tool summary for focus mode headers.
/// "cargo test" for bash, "src/main.rs · 4→6 lines" for edit, etc.
fn focus_tool_summary(name: &str, detail_args: Option<&str>) -> String {
    let args = match detail_args {
        Some(a) => a,
        None => return name.to_string(),
    };
    match name {
        "bash" => {
            let cmd = args.lines().next().unwrap_or(args);
            crate::util::truncate(cmd, 60)
        }
        "edit" | "change" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                let path = v
                    .get("file")
                    .or(v.get("path"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("?");
                crate::util::truncate(path, 50)
            } else {
                crate::util::truncate(args, 50)
            }
        }
        "read" | "write" | "view" => {
            let first = args.lines().next().unwrap_or(args);
            crate::util::truncate(first, 50)
        }
        _ => {
            let first = args.lines().next().unwrap_or(args);
            crate::util::truncate(first, 40)
        }
    }
}

impl App {
    fn current_persona_state(&self) -> crate::settings::PersonaState {
        let persona_id = self
            .plugin_registry
            .as_ref()
            .and_then(|r| r.active_persona().map(|p| p.id.clone()));
        let mind_id = persona_id.as_ref().map(|id| format!("persona:{id}"));
        crate::settings::PersonaState::from_ids(persona_id, mind_id)
    }

    /// Snapshot current model/provider state into a SegmentMeta.
    fn current_meta(&self) -> segments::SegmentMeta {
        segments::SegmentMeta {
            timestamp: Some(std::time::SystemTime::now()),
            provider: Some(self.footer_data.model_provider.clone()),
            model_id: Some(self.footer_data.model_id.clone()),
            tier: Some(self.footer_data.model_tier.clone()),
            thinking_level: Some(self.footer_data.thinking_level.clone()),
            turn: Some(self.turn),
            est_tokens: Some(self.footer_data.estimated_tokens as u32),
            actual_tokens: None, // stamped on TurnEnd via stamp_turn_tokens
            context_percent: Some(self.footer_data.context_percent),
            persona: self
                .plugin_registry
                .as_ref()
                .and_then(|r| r.active_persona().map(|p| p.id.clone())),
            branch: None,      // populated lazily if needed
            duration_ms: None, // set on completion
        }
    }

    fn auspex_status_text(&self) -> String {
        let cwd = self.cwd().to_path_buf();
        let ipc_cfg =
            crate::ipc::IpcServerConfig::from_cwd(&cwd, env!("CARGO_PKG_VERSION"), "status-probe");
        let socket_exists = ipc_cfg.socket_path.exists();
        let dash_status = self
            .web_startup
            .as_ref()
            .map(|startup| {
                let warning_suffix = if startup.daemon_status.transport_warnings.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n  transport warnings: {}",
                        startup.daemon_status.transport_warnings.join(" | ")
                    )
                };
                format!(
                    "running at {}\n  queued events: {}\n  processed events: {}\n  worker: {}{}",
                    startup.http_base,
                    startup.daemon_status.queued_events,
                    startup.daemon_status.processed_events,
                    if startup.daemon_status.worker_running {
                        "running"
                    } else {
                        "idle"
                    },
                    warning_suffix,
                )
            })
            .unwrap_or_else(|| "not running".into());
        let auspex_status = detect_auspex_target()
            .map(|probe| {
                let compatibility = match probe.compatibility {
                    AuspexCompatibility::Compatible => "compatible".to_string(),
                    AuspexCompatibility::Unknown => "unverified".to_string(),
                    AuspexCompatibility::Incompatible(reason) => {
                        format!("incompatible ({reason})")
                    }
                };
                let modes = probe
                    .handoff_modes
                    .iter()
                    .map(|mode| match mode {
                        AuspexHandoffMode::Env => "env",
                        AuspexHandoffMode::BrowserUrl => "browser-url",
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{} (source: {}, {}, modes: {})",
                    probe.target, probe.source, compatibility, modes
                )
            })
            .unwrap_or_else(|| "not detected".into());

        format!(
            "Auspex attach status\n\nIPC\n  protocol: v{}\n  socket: {}\n  socket exists: {}\n  server instance: {}\n  cwd: {}\n\nSession\n  binding: current interactive session\n  session id: not yet exposed in TUI handoff metadata\n\nRuntime\n  omegon version: {}\n  /dash compatibility view: {}\n\nAuspex\n  app: {}\n\nNext step\n  Use `/auspex open` as the primary local desktop handoff.\n  `/dash` remains the compatibility/debug browser path.",
            omegon_traits::IPC_PROTOCOL_VERSION,
            ipc_cfg.socket_path.display(),
            if socket_exists { "yes" } else { "no" },
            ipc_cfg.server_instance_id,
            ipc_cfg.cwd,
            ipc_cfg.omegon_version,
            dash_status,
            auspex_status,
        )
    }

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
            dashboard_area: None,
            conversation_area: None,
            editor_area: None,
            pane_focus: PaneFocus::Editor,
            footer_data: FooterData {
                model_id,
                model_provider,
                ..Default::default()
            },
            instrument_panel: InstrumentPanel::default(),
            focus_mode: false,
            ui_mode: UiMode::Slim,
            ui_surfaces: UiSurfaces::slim(),
            theme: theme::default_theme(),
            settings,
            cancel: std::sync::Arc::new(std::sync::Mutex::new(None)),
            last_ctrl_c: None,
            session_start: std::time::Instant::now(),
            selector: None,
            selector_kind: None,
            at_picker: None,
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
            last_instrument_update: std::time::Instant::now(),
            cleave_tokens_accounted_in: 0,
            cleave_tokens_accounted_out: 0,
            dashboard_refresh_turn: u32::MAX, // force refresh on first frame
            web_startup: None,
            web_server_addr: None,
            queued_prompts: std::collections::VecDeque::new(),
            queue_mode: PromptQueueMode::InterruptAfterTurn,
            operator_events: std::collections::VecDeque::new(),
            previous_harness_status: None,
            capability_tier: None,
            tutorial: None,
            tutorial_overlay: None,
            update_rx: None,
            update_tx: None,
            login_prompt_tx: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            keyboard_enhancement: false,
            mouse_capture_enabled: false,
            terminal_copy_mode: false,
            last_left_click: None,
            extension_widgets: std::collections::HashMap::new(),
            widget_receivers: Vec::new(),
            active_modal: None,
            active_action_prompt: None,
            oauth_tos_notice_shown: false,
        }
    }

    fn set_mouse_capture(&mut self, enabled: bool) {
        if self.mouse_capture_enabled == enabled {
            return;
        }
        self.mouse_capture_enabled = enabled;
        if enabled {
            let _ = io::stdout().execute(EnableMouseCapture);
        } else {
            let _ = io::stdout().execute(DisableMouseCapture);
        }
    }

    fn enable_mouse_interaction_mode(&mut self) {
        self.terminal_copy_mode = false;
        self.focus_mode = false;
        self.set_mouse_capture(true);
    }

    fn set_ui_mode(&mut self, mode: UiMode) {
        self.ui_mode = mode;
        self.ui_surfaces = match mode {
            UiMode::Full => UiSurfaces::full(),
            UiMode::Slim => UiSurfaces::slim(),
        };
        match mode {
            UiMode::Slim => {
                self.focus_mode = false;
                self.terminal_copy_mode = true;
                self.mouse_capture_enabled = false;
                self.set_mouse_capture(false);
            }
            UiMode::Full => {
                self.terminal_copy_mode = false;
                self.mouse_capture_enabled = true;
                self.set_mouse_capture(true);
            }
        }
    }

    fn toggle_ui_surface(&mut self, surface: &str, enabled: bool) -> Result<(), String> {
        match surface {
            "dashboard" | "dash" | "tree" => self.ui_surfaces.dashboard = enabled,
            "instruments" | "instrument" | "tools" => self.ui_surfaces.instruments = enabled,
            "footer" | "status" => self.ui_surfaces.footer = enabled,
            other => return Err(format!("Unknown UI surface: {other}")),
        }
        Ok(())
    }

    fn ui_status_text(&self) -> String {
        let mode = match self.ui_mode {
            UiMode::Full => "full",
            UiMode::Slim => "slim",
        };
        format!(
            "UI mode: {mode}\n  dashboard: {}\n  instruments: {}\n  footer: {}\n\nPresets\n  /ui full\n  /ui slim\n\nSurfaces\n  /ui show dashboard\n  /ui hide dashboard\n  /ui toggle dashboard\n  /ui show instruments\n  /ui hide instruments\n  /ui toggle instruments\n  /ui show footer\n  /ui hide footer\n  /ui toggle footer",
            if self.ui_surfaces.dashboard {
                "on"
            } else {
                "off"
            },
            if self.ui_surfaces.instruments {
                "on"
            } else {
                "off"
            },
            if self.ui_surfaces.footer { "on" } else { "off" },
        )
    }

    fn set_focus_mode(&mut self, enabled: bool) {
        if self.focus_mode == enabled {
            return;
        }
        self.focus_mode = enabled;
        if enabled {
            // Entering /focus should bias to the live tail, not to a stale
            // previously-selected segment. Operators use /focus as a
            // "show me the current conversation clearly" command; if an old
            // selection remains latched from earlier mouse/keyboard navigation,
            // preserving it here makes focus mode appear off-by-one (or more)
            // relative to the latest assistant turn.
            let focus_idx = self.conversation.last_selectable_segment();
            if let Some(idx) = focus_idx {
                self.conversation.select_segment(idx);
            }
            self.pane_focus = PaneFocus::Conversation;
            self.terminal_copy_mode = false;
            self.set_mouse_capture(false);
            self.show_toast(
                "Focus mode active — timeline navigation enabled for terminal-native reading and selection",
                ratatui_toaster::ToastType::Info,
            );
        } else {
            self.set_mouse_capture(true);
            self.show_toast(
                "Focus mode disabled — full conversation and mouse interaction restored",
                ratatui_toaster::ToastType::Info,
            );
        }
    }

    fn set_terminal_copy_mode(&mut self, enabled: bool) {
        let changed = self.terminal_copy_mode != enabled;
        self.terminal_copy_mode = enabled;
        self.focus_mode = false;
        self.set_mouse_capture(!enabled);
        if !changed {
            return;
        }
        if enabled {
            self.show_toast(
                "Terminal-native selection active — drag to select, then use your terminal's copy shortcut",
                ratatui_toaster::ToastType::Info,
            );
        } else {
            self.show_toast(
                "Mouse interaction mode enabled — pane mouse interaction restored",
                ratatui_toaster::ToastType::Info,
            );
            self.conversation.push_system(
                "🖱 Mouse interaction mode ON — mouse capture enabled. Pane clicks, wheel scroll, and segment targeting are active. Use /mouse off to return to terminal-native selection.",
            );
        }
    }

    fn open_model_selector(&mut self) {
        let current = self.settings().model.clone();

        // Build selector options from the unified model catalog
        let catalog = self::model_catalog::ModelCatalog::discover();
        let mut options: Vec<selector::SelectOption> = Vec::new();

        // Group models by provider for visual organization
        for (provider_name, models) in &catalog.providers {
            for model in models {
                // Format: "Provider: Model Name — description (context, cost tier, capabilities)"
                let context = model.context_str();
                let caps = if model.capabilities.is_empty() {
                    String::new()
                } else {
                    format!(", {}", model.capability_str())
                };
                let label = format!("{}: {}", provider_name, model.name);
                let description = format!(
                    "{} — {} • {}{}",
                    model.description,
                    context,
                    model.cost_tier.as_str(),
                    caps
                );

                options.push(selector::SelectOption {
                    value: model.id.clone(),
                    label,
                    description,
                    active: model.id == current,
                });
            }
        }

        if options.is_empty() {
            self.conversation
                .push_system("Model catalog is empty. Check /model list for available options.");
            return;
        }

        // Sort by provider, then by name for consistency
        options.sort_by(|a, b| a.label.cmp(&b.label));

        self.selector = Some(selector::Selector::new("Select Model", options));
        self.selector_kind = Some(SelectorKind::Model);
    }

    fn open_thinking_selector(&mut self) {
        let current = self.settings().thinking;
        let options = crate::settings::ThinkingLevel::all()
            .iter()
            .map(|level| selector::SelectOption {
                value: level.as_str().to_string(),
                label: format!("{} {}", level.icon(), level.as_str()),
                description: match level {
                    crate::settings::ThinkingLevel::Off => "Servitor — no extended thinking".into(),
                    crate::settings::ThinkingLevel::Minimal => {
                        "Functionary — ~2k token budget".into()
                    }
                    crate::settings::ThinkingLevel::Low => "Adept — ~5k token budget".into(),
                    crate::settings::ThinkingLevel::Medium => "Magos — ~10k token budget".into(),
                    crate::settings::ThinkingLevel::High => "Archmagos — ~50k token budget".into(),
                },
                active: *level == current,
            })
            .collect();
        self.selector = Some(selector::Selector::new("Thinking Level", options));
        self.selector_kind = Some(SelectorKind::ThinkingLevel);
    }

    fn open_context_selector(&mut self) {
        let current = self.settings().context_class;
        let options = crate::settings::ContextClass::all()
            .iter()
            .map(|class| selector::SelectOption {
                value: class.short().to_string(),
                label: class.label().to_string(),
                description: match class {
                    crate::settings::ContextClass::Squad => "Standard sessions".into(),
                    crate::settings::ContextClass::Maniple => "Extended analysis".into(),
                    crate::settings::ContextClass::Clan => "Large codebase".into(),
                    crate::settings::ContextClass::Legion => "Massive context".into(),
                },
                active: *class == current,
            })
            .collect();
        self.selector = Some(selector::Selector::new("Context Class", options));
        self.selector_kind = Some(SelectorKind::ContextClass);
    }

    fn open_persona_selector(&mut self) {
        let (personas, _) = crate::plugins::persona_loader::scan_available();
        if personas.is_empty() {
            self.conversation.push_system(
                "No personas installed. Install with: omegon plugin install <git-url>",
            );
            return;
        }

        let active_id = self
            .plugin_registry
            .as_ref()
            .and_then(|registry| registry.active_persona().map(|persona| persona.id.clone()));
        let options = personas
            .into_iter()
            .map(|persona| selector::SelectOption {
                active: active_id.as_deref() == Some(persona.id.as_str()),
                value: persona.id,
                label: persona.name,
                description: persona.description,
            })
            .collect();
        self.selector = Some(selector::Selector::new("Select Persona", options));
        self.selector_kind = Some(SelectorKind::Persona);
    }

    fn open_tone_selector(&mut self) {
        let (_, tones) = crate::plugins::persona_loader::scan_available();
        if tones.is_empty() {
            self.conversation
                .push_system("No tones installed. Install with: omegon plugin install <git-url>");
            return;
        }

        let active_id = self
            .plugin_registry
            .as_ref()
            .and_then(|registry| registry.active_tone().map(|tone| tone.id.clone()));
        let options = tones
            .into_iter()
            .map(|tone| selector::SelectOption {
                active: active_id.as_deref() == Some(tone.id.as_str()),
                value: tone.id,
                label: tone.name,
                description: tone.description,
            })
            .collect();
        self.selector = Some(selector::Selector::new("Select Tone", options));
        self.selector_kind = Some(SelectorKind::Tone);
    }

    /// Shorthand for the current working directory as a Path.
    fn cwd(&self) -> &std::path::Path {
        std::path::Path::new(&self.footer_data.cwd)
    }

    /// Generate a recovery hint for a tool error, if one applies.
    fn recovery_hint(tool_name: Option<&str>, error_text: &str) -> &'static str {
        let lower = error_text.to_lowercase();
        // Connection / network errors
        if lower.contains("connection refused") || lower.contains("connect timeout") {
            if lower.contains("ollama") || lower.contains("11434") {
                return "Ollama not running. Start with: ollama serve";
            }
            return "Service unreachable. Check if the target is running and the port is correct.";
        }
        // Rate limiting — match HTTP status codes as word boundaries, not substrings
        if lower.contains("rate limit")
            || lower.contains("status 429")
            || lower.contains("http 429")
            || lower.contains("too many requests")
            || lower.contains("error 429")
        {
            return "Rate limited. Use /model to switch provider, or wait a moment and retry.";
        }
        // Authentication — same boundary-aware matching
        if lower.contains("status 401")
            || lower.contains("http 401")
            || lower.contains("error 401")
            || lower.contains("unauthorized")
            || lower.contains("invalid api key")
            || lower.contains("invalid_api_key")
        {
            return "Authentication failed. Use /login to re-authenticate.";
        }
        if lower.contains("status 403")
            || lower.contains("http 403")
            || lower.contains("error 403")
            || lower.contains("forbidden")
            || lower.contains("permission denied")
        {
            return "Permission denied. Check file permissions or API access scope.";
        }
        // Timeout
        if lower.contains("timeout") || lower.contains("timed out") {
            return "Operation timed out. Try a simpler request or increase timeout.";
        }
        // MCP errors
        if lower.contains("mcp")
            && (lower.contains("not connected") || lower.contains("disconnected"))
        {
            return "MCP server disconnected. Check the server process and restart if needed.";
        }
        // Context window
        if lower.contains("context length")
            || lower.contains("too many tokens")
            || lower.contains("context_length")
        {
            return "Context window exceeded. Use /context compact to free space, or /context to select a larger class.";
        }
        // Git errors
        if tool_name == Some("bash")
            && (lower.contains("not a git repository") || lower.contains("fatal: "))
        {
            return "Git error. Check that you're in a git repository and the operation is valid.";
        }
        ""
    }

    /// Count pending notes in .omegon/notes.md
    fn count_notes(cwd: &std::path::Path) -> usize {
        let notes_path = cwd.join(".omegon").join("notes.md");
        std::fs::read_to_string(&notes_path)
            .map(|c| c.lines().filter(|l| l.starts_with("- [")).count())
            .unwrap_or(0)
    }

    fn open_login_selector(&mut self) {
        // Build from canonical provider map — single source of truth
        let options: Vec<selector::SelectOption> = crate::auth::PROVIDERS
            .iter()
            .map(|p| {
                let session_status = crate::auth::provider_session_status(p);
                selector::SelectOption {
                    value: p.id.to_string(),
                    label: match session_status {
                        crate::auth::ProviderSessionStatus::Configured => {
                            format!("✓ {}", p.display_name)
                        }
                        crate::auth::ProviderSessionStatus::Expired => {
                            format!("⚠ {}", p.display_name)
                        }
                        crate::auth::ProviderSessionStatus::Missing => {
                            format!("  {}", p.display_name)
                        }
                    },
                    description: match session_status {
                        crate::auth::ProviderSessionStatus::Configured => "configured ✓".into(),
                        crate::auth::ProviderSessionStatus::Expired => {
                            "expired — re-login required".into()
                        }
                        crate::auth::ProviderSessionStatus::Missing => p.description.to_string(),
                    },
                    active: matches!(
                        session_status,
                        crate::auth::ProviderSessionStatus::Configured
                    ),
                }
            })
            .collect();
        self.selector = Some(selector::Selector::new("Login — choose provider", options));
        self.selector_kind = Some(SelectorKind::LoginProvider);
    }

    fn open_update_channel_selector(&mut self) {
        let current = self.settings().update_channel;
        let options = [
            crate::update::UpdateChannel::Stable,
            crate::update::UpdateChannel::Rc,
            crate::update::UpdateChannel::Nightly,
        ]
        .into_iter()
        .map(|channel| selector::SelectOption {
            value: channel.as_str().to_string(),
            label: channel.as_str().to_string(),
            description: match channel {
                crate::update::UpdateChannel::Stable => "Release builds only".to_string(),
                crate::update::UpdateChannel::Rc => "Release candidates only".to_string(),
                crate::update::UpdateChannel::Nightly => {
                    "Nightly / dev prereleases only".to_string()
                }
            },
            active: current == channel.as_str(),
        })
        .collect();
        self.selector = Some(selector::Selector::new("Update Channel", options));
        self.selector_kind = Some(SelectorKind::UpdateChannel);
    }

    fn open_workspace_role_selector(&mut self) {
        let options = [
            crate::workspace::types::WorkspaceRole::Primary,
            crate::workspace::types::WorkspaceRole::Feature,
            crate::workspace::types::WorkspaceRole::CleaveChild,
            crate::workspace::types::WorkspaceRole::Benchmark,
            crate::workspace::types::WorkspaceRole::Release,
            crate::workspace::types::WorkspaceRole::Exploratory,
            crate::workspace::types::WorkspaceRole::ReadOnly,
        ]
        .into_iter()
        .map(|role| selector::SelectOption {
            value: role.as_str().to_string(),
            label: role.as_str().to_string(),
            description: format!("Set workspace role to {}", role.as_str()),
            active: false,
        })
        .collect();
        self.selector = Some(selector::Selector::new("Workspace Role", options));
        self.selector_kind = Some(SelectorKind::WorkspaceRole);
    }

    fn open_workspace_kind_selector(&mut self) {
        let options = [
            crate::workspace::types::WorkspaceKind::Code,
            crate::workspace::types::WorkspaceKind::Vault,
            crate::workspace::types::WorkspaceKind::Knowledge,
            crate::workspace::types::WorkspaceKind::Spec,
            crate::workspace::types::WorkspaceKind::Mixed,
            crate::workspace::types::WorkspaceKind::Generic,
        ]
        .into_iter()
        .map(|kind| selector::SelectOption {
            value: kind.as_str().to_string(),
            label: kind.as_str().to_string(),
            description: format!("Set workspace kind to {}", kind.as_str()),
            active: false,
        })
        .collect();
        self.selector = Some(selector::Selector::new("Workspace Kind", options));
        self.selector_kind = Some(SelectorKind::WorkspaceKind);
    }

    fn show_status_change_toasts(
        &mut self,
        prev: &crate::status::HarnessStatus,
        current: &crate::status::HarnessStatus,
    ) {
        // Check for persona changes
        if prev.active_persona != current.active_persona {
            match (&prev.active_persona, &current.active_persona) {
                (Some(old), Some(new)) => {
                    if old.id != new.id {
                        self.show_toast(
                            &format!("Persona → {} {}", new.badge, new.name),
                            ratatui_toaster::ToastType::Info,
                        );
                    }
                }
                (Some(old), None) => {
                    self.show_toast(
                        &format!("Persona deactivated: {} {}", old.badge, old.name),
                        ratatui_toaster::ToastType::Warning,
                    );
                }
                (None, Some(new)) => {
                    self.show_toast(
                        &format!("Persona activated: {} {}", new.badge, new.name),
                        ratatui_toaster::ToastType::Info,
                    );
                }
                _ => {}
            }
        }

        // Check for tone changes
        if prev.active_tone != current.active_tone {
            match (&prev.active_tone, &current.active_tone) {
                (Some(old), Some(new)) => {
                    if old.id != new.id {
                        self.show_toast(
                            &format!("Tone → {}", new.name),
                            ratatui_toaster::ToastType::Info,
                        );
                    }
                }
                (Some(old), None) => {
                    self.show_toast(
                        &format!("Tone deactivated: {}", old.name),
                        ratatui_toaster::ToastType::Warning,
                    );
                }
                (None, Some(new)) => {
                    self.show_toast(
                        &format!("Tone activated: {}", new.name),
                        ratatui_toaster::ToastType::Info,
                    );
                }
                _ => {}
            }
        }

        // Check for MCP server changes
        let prev_connected: std::collections::HashSet<&String> = prev
            .mcp_servers
            .iter()
            .filter(|s| s.connected)
            .map(|s| &s.name)
            .collect();
        let current_connected: std::collections::HashSet<&String> = current
            .mcp_servers
            .iter()
            .filter(|s| s.connected)
            .map(|s| &s.name)
            .collect();

        // New connections
        for name in current_connected.difference(&prev_connected) {
            if let Some(server) = current.mcp_servers.iter().find(|s| &s.name == *name) {
                self.show_toast(
                    &format!("MCP connected: {} ({}t)", name, server.tool_count),
                    ratatui_toaster::ToastType::Info,
                );
            }
        }

        // Lost connections
        for name in prev_connected.difference(&current_connected) {
            self.show_toast(
                &format!("MCP disconnected: {}", name),
                ratatui_toaster::ToastType::Warning,
            );
        }

        // Check for auth expiration (simplified - checking provider count as proxy)
        let prev_auth_count = prev.providers.iter().filter(|p| p.authenticated).count();
        let current_auth_count = current.providers.iter().filter(|p| p.authenticated).count();
        if current_auth_count < prev_auth_count {
            self.show_toast(
                "Authentication expired for provider",
                ratatui_toaster::ToastType::Error,
            );
        }

        // Memory backend degradation/recovery
        if prev.memory_available != current.memory_available {
            if current.memory_available {
                self.show_toast(
                    "Memory backend restored",
                    ratatui_toaster::ToastType::Success,
                );
            } else {
                self.show_toast(
                    current
                        .memory_warning
                        .as_deref()
                        .unwrap_or("Memory backend unavailable — memory_* tools disabled"),
                    ratatui_toaster::ToastType::Error,
                );
            }
        }
    }

    fn confirm_selector(&mut self, tx: &mpsc::Sender<TuiCommand>) -> Option<String> {
        let sel = self.selector.take()?;
        let kind = self.selector_kind.take()?;
        let value = sel.selected_value().to_string();

        match kind {
            SelectorKind::Model => {
                let _ = tx.try_send(TuiCommand::SetModel {
                    model: value.clone(),
                    respond_to: None,
                });
                Some(format!("Switching model → {value}"))
            }
            SelectorKind::ThinkingLevel => {
                if let Some(level) = crate::settings::ThinkingLevel::parse(&value) {
                    let _ = tx.try_send(TuiCommand::SetThinking {
                        level,
                        respond_to: None,
                    });
                    Some(format!("Thinking → {} {}", level.icon(), level.as_str()))
                } else {
                    Some(format!("Unknown level: {value}"))
                }
            }
            SelectorKind::ContextClass => {
                if let Some(class) = crate::settings::ContextClass::parse(&value) {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SetContextClass { class },
                        respond_to: None,
                    });
                    Some(format!("Context policy → {}", class.label()))
                } else {
                    Some(format!("Unknown context class: {value}"))
                }
            }
            SelectorKind::Persona => {
                let (personas, _) = crate::plugins::persona_loader::scan_available();
                if let Some(available) = personas.into_iter().find(|persona| persona.id == value) {
                    match crate::plugins::persona_loader::load_persona(&available.path) {
                        Ok(persona) => {
                            let name = persona.name.clone();
                            let badge = persona.badge.clone().unwrap_or_else(|| "⚙".into());
                            let fact_count = persona.mind_facts.len();
                            if let Some(ref mut registry) = self.plugin_registry {
                                registry.activate_persona(persona);
                            }
                            Some(format!(
                                "{badge} Persona activated: {name} ({fact_count} mind facts)"
                            ))
                        }
                        Err(e) => Some(format!("Failed to load persona: {e}")),
                    }
                } else {
                    Some(format!("Persona '{value}' no longer available."))
                }
            }
            SelectorKind::Tone => {
                let (_, tones) = crate::plugins::persona_loader::scan_available();
                if let Some(available) = tones.into_iter().find(|tone| tone.id == value) {
                    match crate::plugins::persona_loader::load_tone(&available.path) {
                        Ok(tone) => {
                            let name = tone.name.clone();
                            if let Some(ref mut registry) = self.plugin_registry {
                                registry.activate_tone(tone);
                            }
                            Some(format!("♪ Tone activated: {name}"))
                        }
                        Err(e) => Some(format!("Failed to load tone: {e}")),
                    }
                } else {
                    Some(format!("Tone '{value}' no longer available."))
                }
            }
            SelectorKind::LoginProvider => {
                // OAuth providers go through the auth login flow (opens browser)
                // API key providers go through secret input mode (hidden input)
                match value.as_str() {
                    "anthropic" | "openai-codex" => {
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_login".to_string(),
                            args: value.clone(),
                        });
                        let label = crate::auth::provider_by_id(&value)
                            .map(|p| p.display_name)
                            .unwrap_or(value.as_str());
                        Some(format!("Opening browser for {label} login…"))
                    }
                    "openai" | "openrouter" | "ollama-cloud" | "brave" | "tavily" | "serper"
                    | "huggingface" => {
                        // Map to the correct env var name for storage
                        let key_name = match value.as_str() {
                            "openai" => "OPENAI_API_KEY",
                            "openrouter" => "OPENROUTER_API_KEY",
                            "ollama-cloud" => "OLLAMA_API_KEY",
                            "brave" => "BRAVE_API_KEY",
                            "tavily" => "TAVILY_API_KEY",
                            "serper" => "SERPER_API_KEY",
                            "huggingface" => "HUGGING_FACE_TOKEN",
                            _ => unreachable!(),
                        };
                        self.editor.start_secret_input(key_name);
                        Some(format!(
                            "🔒 Paste your {} API key (input is hidden):",
                            value
                        ))
                    }
                    "github" => {
                        // GitHub uses dynamic resolution via gh CLI
                        if let Some(request) = crate::control_runtime::control_request_from_slash(
                            &CanonicalSlashCommand::SecretsSet {
                                name: "GITHUB_TOKEN".to_string(),
                                value: "cmd:gh auth token".to_string(),
                            },
                        ) {
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request,
                                respond_to: None,
                            });
                        }
                        Some(
                            "✓ GITHUB_TOKEN → cmd:gh auth token (always fresh from gh CLI)"
                                .to_string(),
                        )
                    }
                    "gitlab" => {
                        self.editor.start_secret_input("GITLAB_TOKEN");
                        Some("🔒 Paste your GitLab token (input is hidden):".to_string())
                    }
                    _ => {
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "auth_login".to_string(),
                            args: value.clone(),
                        });
                        Some(format!("Logging in to {value}…"))
                    }
                }
            }
            SelectorKind::SecretName => {
                if value == "(custom)" {
                    self.editor.set_text("/secrets set ");
                    Some("Type: /secrets set NAME VALUE".to_string())
                } else {
                    let suggested = Self::SECRET_CATALOG
                        .iter()
                        .find(|(name, _, _)| *name == value)
                        .map(|(_, recipe, _)| *recipe)
                        .unwrap_or("");
                    if suggested.is_empty() {
                        // Direct value — enter masked secret input mode
                        self.editor.start_secret_input(&value);
                        Some(format!(
                            "🔒 Paste or type value for {value} (input is hidden):"
                        ))
                    } else {
                        // Dynamic recipe — set immediately
                        if let Some(request) = crate::control_runtime::control_request_from_slash(
                            &CanonicalSlashCommand::SecretsSet {
                                name: value.clone(),
                                value: suggested.to_string(),
                            },
                        ) {
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request,
                                respond_to: None,
                            });
                        }
                        Some(format!("✓ {value} → {suggested}"))
                    }
                }
            }
            SelectorKind::VaultConfigure => {
                let command = format!("/vault configure {}", value);
                self.editor.set_text(&command);
                Some(format!("Vault configure → {value}"))
            }
            SelectorKind::UpdateChannel => {
                if let Some(channel) = crate::update::UpdateChannel::parse(&value) {
                    self.update_settings(|s| s.update_channel = channel.as_str().to_string());
                    if let Some(tx) = self.update_tx.clone() {
                        crate::update::spawn_check(tx, channel);
                    }
                    Some(format!(
                        "Update channel set to {}. Rechecking for updates now.",
                        channel.as_str()
                    ))
                } else {
                    Some(format!("Unknown update channel: {value}"))
                }
            }
            SelectorKind::WorkspaceRole => {
                if let Some(role) = crate::workspace::types::WorkspaceRole::parse(&value) {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::WorkspaceRoleSet { role },
                        respond_to: None,
                    });
                    Some(format!("Workspace role → {}", role.as_str()))
                } else {
                    Some(format!("Unknown workspace role: {value}"))
                }
            }
            SelectorKind::WorkspaceKind => {
                if let Some(kind) = crate::workspace::types::WorkspaceKind::parse(&value) {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::WorkspaceKindSet { kind },
                        respond_to: None,
                    });
                    Some(format!("Workspace kind → {}", kind.as_str()))
                } else {
                    Some(format!("Unknown workspace kind: {value}"))
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
    // ─── Well-known secret names for the /secrets selector ────────
    // Grouped: Omegon providers → cloud/infra → databases → dev tools → AI/ML
    const SECRET_CATALOG: &'static [(&'static str, &'static str, &'static str)] = &[
        // (name, suggested_recipe, description)
        // Omegon providers — these drive the agent
        ("ANTHROPIC_API_KEY", "", "Anthropic Claude API"),
        ("OPENAI_API_KEY", "", "OpenAI API"),
        ("OPENROUTER_API_KEY", "", "OpenRouter (free tier available)"),
        ("OLLAMA_API_KEY", "", "Ollama Cloud API"),
        // Search providers
        ("BRAVE_API_KEY", "", "Brave Search API"),
        ("TAVILY_API_KEY", "", "Tavily Search API"),
        ("SERPER_API_KEY", "", "Serper (Google) Search API"),
        // Git forges
        (
            "GITHUB_TOKEN",
            "cmd:gh auth token",
            "GitHub (dynamic via gh CLI)",
        ),
        (
            "GITLAB_TOKEN",
            "cmd:glab auth token",
            "GitLab (dynamic via glab CLI)",
        ),
        // Cloud
        (
            "AWS_ACCESS_KEY_ID",
            "env:AWS_ACCESS_KEY_ID",
            "AWS access key",
        ),
        (
            "AWS_SECRET_ACCESS_KEY",
            "env:AWS_SECRET_ACCESS_KEY",
            "AWS secret key",
        ),
        (
            "GOOGLE_APPLICATION_CREDENTIALS",
            "env:GOOGLE_APPLICATION_CREDENTIALS",
            "GCP service account",
        ),
        (
            "AZURE_CLIENT_SECRET",
            "env:AZURE_CLIENT_SECRET",
            "Azure service principal",
        ),
        // Databases
        (
            "DATABASE_URL",
            "env:DATABASE_URL",
            "Database connection string",
        ),
        ("POSTGRES_PASSWORD", "env:PGPASSWORD", "PostgreSQL password"),
        ("MONGO_URI", "env:MONGO_URI", "MongoDB connection string"),
        ("REDIS_URL", "env:REDIS_URL", "Redis connection URL"),
        // Container registries
        (
            "DOCKER_PASSWORD",
            "env:DOCKER_PASSWORD",
            "Docker Hub / registry",
        ),
        // Package managers
        (
            "NPM_TOKEN",
            "cmd:npm token get",
            "npm (dynamic via npm CLI)",
        ),
        (
            "CARGO_REGISTRY_TOKEN",
            "env:CARGO_REGISTRY_TOKEN",
            "crates.io publish token",
        ),
        ("PYPI_TOKEN", "env:PYPI_TOKEN", "PyPI publish token"),
        // Messaging / notifications
        ("SLACK_TOKEN", "env:SLACK_TOKEN", "Slack bot/user token"),
        ("DISCORD_TOKEN", "env:DISCORD_TOKEN", "Discord bot token"),
        // AI / ML
        ("HUGGING_FACE_TOKEN", "env:HF_TOKEN", "Hugging Face API"),
        (
            "REPLICATE_API_TOKEN",
            "env:REPLICATE_API_TOKEN",
            "Replicate API",
        ),
        // Custom
        ("(custom)", "", "Enter a custom secret name"),
    ];

    /// Handle /secrets — interactive secret management.
    fn handle_secrets(&mut self, args: &str, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        match parts.first().copied().unwrap_or("") {
            // /secrets configure and /secrets set with no name/value → open selector
            "configure" | "set" if parts.len() < 3 => {
                let existing: Vec<String> = {
                    let _ = tx; // suppress unused warning in this branch
                    Vec::new()
                };
                let options: Vec<selector::SelectOption> = Self::SECRET_CATALOG
                    .iter()
                    .map(|(name, recipe, desc)| {
                        let is_configured = existing.contains(&name.to_string());
                        selector::SelectOption {
                            value: name.to_string(),
                            label: if *name == "(custom)" {
                                "➕ Custom secret...".to_string()
                            } else {
                                format!("{name:<30} {desc}")
                            },
                            description: if recipe.is_empty() {
                                "direct value → OS keyring".to_string()
                            } else {
                                format!("suggested: {recipe}")
                            },
                            active: is_configured,
                        }
                    })
                    .collect();
                self.selector = Some(selector::Selector::new("Set Secret — pick a name", options));
                self.selector_kind = Some(SelectorKind::SecretName);
                SlashResult::Handled
            }
            _ => {
                if let Some(command) = canonical_slash_command("secrets", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /secrets [list|get <name>|set <name> <value>|delete <name>]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /secrets [list|get <name>|set <name> <value>|delete <name>]".into(),
                    )
                }
            }
        }
    }

    /// Handle /tutorial — start, resume, or manage the interactive tutorial overlay.
    fn handle_tutorial(&mut self, args: &str) -> SlashResult {
        match args.trim() {
            "status" => {
                if let Some(ref overlay) = self.tutorial_overlay {
                    return SlashResult::Display(format!(
                        "Tutorial: step {}/{} — \"{}\"\nMode: {}",
                        overlay.step_index() + 1,
                        overlay.total_steps(),
                        overlay.step().title,
                        if overlay.is_demo { "demo" } else { "hands-on" },
                    ));
                }
                if let Some(ref tut) = self.tutorial {
                    return SlashResult::Display(tut.status_line());
                }
                SlashResult::Display("No tutorial active. Type /tutorial to start.".into())
            }
            "reset" => {
                if self.tutorial_overlay.is_some() {
                    self.tutorial_overlay = None;
                    return SlashResult::Display(
                        "Tutorial overlay reset. Type /tutorial to start again.".into(),
                    );
                }
                if let Some(ref mut tut) = self.tutorial {
                    tut.reset();
                    return SlashResult::Display(
                        "Tutorial reset to lesson 1. Type /tutorial to start.".into(),
                    );
                }
                SlashResult::Display("No tutorial active.".into())
            }
            "demo" => {
                // Resume existing overlay if still active
                if let Some(ref overlay) = self.tutorial_overlay {
                    if overlay.active {
                        return SlashResult::Display(format!(
                            "Tutorial overlay active (step {}/{}). Press Tab to advance, Esc to dismiss.",
                            overlay.step_index() + 1,
                            overlay.total_steps(),
                        ));
                    }
                }
                // Start demo overlay
                let has_design = self.dashboard.status_counts.total > 0;
                self.tutorial_overlay = Some(tutorial::Tutorial::new_demo(has_design));
                SlashResult::Display(
                    "Tutorial demo started. Tab to advance, Esc to dismiss.".into(),
                )
            }
            "lessons" => {
                // Explicit opt-in to legacy lesson-based tutorial (if project has lesson files)
                let tutorial_dir = self.cwd().join(".omegon").join("tutorial");
                if tutorial_dir.is_dir() {
                    if let Some(tut) = TutorialState::load(&tutorial_dir) {
                        let lesson = tut.current_lesson().clone();
                        let status = tut.status_line();
                        self.tutorial = Some(tut);
                        self.queue_prompt(lesson.content, Vec::new());
                        return SlashResult::Display(format!(
                            "{status}\n\nLesson queued. The agent will begin when ready."
                        ));
                    }
                }
                SlashResult::Display("No lesson files found in .omegon/tutorial/".into())
            }
            "consent" => {
                // Operator explicitly grants consent for Anthropic OAuth subscription usage.
                // Enables AutoPrompt steps for the hands-on tutorial.
                let has_design = self.dashboard.status_counts.total > 0;
                self.tutorial_overlay = Some(tutorial::Tutorial::with_mode(
                    has_design,
                    tutorial::TutorialMode::Interactive,
                ));
                SlashResult::Display(
                    "Consent recorded. Starting interactive tutorial.\n\
                     Omegon will perform real work using your Anthropic subscription.\n\n\
                     Note: Anthropic's ToS permits interactive TUI use only.\n\
                     Background tasks, /cleave, and --prompt require ANTHROPIC_API_KEY.\n\n\
                     Tab to advance, Esc to dismiss."
                        .into(),
                )
            }
            _ => {
                // Resume existing overlay if still active
                if let Some(ref overlay) = self.tutorial_overlay {
                    if overlay.active {
                        let mode_note = match overlay.mode {
                            tutorial::TutorialMode::ConsentRequired => {
                                "\n\nℹ Anthropic subscription detected. Type /tutorial consent\nto enable interactive agent steps (uses subscription quota)."
                            }
                            tutorial::TutorialMode::OrientationOnly => {
                                "\n\nℹ No Victory-tier cloud model found. Add an API key or\n/login openai-codex for the full interactive tutorial."
                            }
                            tutorial::TutorialMode::Interactive => "",
                        };
                        return SlashResult::Display(format!(
                            "Tutorial overlay active (step {}/{}). Press Tab to advance, Esc to dismiss.{}",
                            overlay.step_index() + 1,
                            overlay.total_steps(),
                            mode_note,
                        ));
                    }
                }
                // Gate: detect what the operator has available
                let has_design = self.dashboard.status_counts.total > 0;
                let mode = tutorial::tutorial_gate();
                let mode_msg = match mode {
                    tutorial::TutorialMode::Interactive => {
                        "Tutorial started. Tab to advance, Esc to dismiss.".to_string()
                    }
                    tutorial::TutorialMode::ConsentRequired => {
                        "Tutorial started (orientation mode).\n\n\
                         Anthropic subscription detected. Omegon's ToS restricts automated use\n\
                         of subscriptions without your explicit consent.\n\n\
                         Type /tutorial consent to enable interactive agent steps,\n\
                         or add an API key / /login openai-codex for automatic access.\n\n\
                         Tab to advance orientation steps, Esc to dismiss."
                            .to_string()
                    }
                    tutorial::TutorialMode::OrientationOnly => {
                        "Tutorial started (orientation mode).\n\n\
                         No Victory-tier cloud model found. Add an API key or\n\
                         /login openai-codex for the full interactive tutorial.\n\n\
                         Tab to advance, Esc to dismiss."
                            .to_string()
                    }
                };
                self.tutorial_overlay = Some(tutorial::Tutorial::with_mode(has_design, mode));
                SlashResult::Display(mode_msg)
            }
        }
    }

    /// Advance to the next tutorial step/lesson.
    fn handle_tutorial_next(&mut self) -> SlashResult {
        if let Some(ref mut overlay) = self.tutorial_overlay {
            if overlay.active {
                overlay.advance();
                return SlashResult::Display(format!(
                    "Tutorial step {}/{}",
                    overlay.step_index() + 1,
                    overlay.total_steps()
                ));
            }
        }
        if let Some(ref mut tut) = self.tutorial {
            if tut.advance() {
                let lesson = tut.current_lesson().clone();
                let status = tut.status_line();
                self.queue_prompt(lesson.content, Vec::new());
                SlashResult::Display(format!("{status}\n\nLesson queued."))
            } else {
                SlashResult::Display(
                    "🎉 You've completed the tutorial! Type /tutorial reset to start over.".into(),
                )
            }
        } else {
            SlashResult::Display("No tutorial active. Type /tutorial to start.".into())
        }
    }

    /// Go back to the previous tutorial step/lesson.
    fn handle_tutorial_prev(&mut self) -> SlashResult {
        if let Some(ref mut overlay) = self.tutorial_overlay {
            if overlay.active {
                overlay.go_back();
                return SlashResult::Display(format!(
                    "Tutorial step {}/{}",
                    overlay.step_index() + 1,
                    overlay.total_steps()
                ));
            }
        }
        if let Some(ref mut tut) = self.tutorial {
            if tut.go_back() {
                let lesson = tut.current_lesson().clone();
                let status = tut.status_line();
                self.queue_prompt(lesson.content, Vec::new());
                SlashResult::Display(format!("{status}\n\nLesson queued."))
            } else {
                SlashResult::Display("Already at the first lesson.".into())
            }
        } else {
            SlashResult::Display("No tutorial active. Type /tutorial to start.".into())
        }
    }

    /// Clone the tutorial project and exec omegon inside it.
    fn launch_tutorial_project(&mut self) -> SlashResult {
        if cfg!(test) || std::env::var("CARGO_TEST").is_ok() {
            return SlashResult::Display(
                "Tutorial: would clone and launch tutorial project".into(),
            );
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
                .args([
                    "clone",
                    "--depth=1",
                    TUTORIAL_REPO,
                    &tutorial_dir.to_string_lossy(),
                ])
                .output();
            if result.is_err() || !tutorial_dir.join(".git").exists() {
                return SlashResult::Display(
                    "Could not download the demo project.\n\n\
                     Try /tutorial instead — it works with your current project,\n\
                     no download needed. Or check your network and try /tutorial demo again."
                        .into(),
                );
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
                .arg("--tutorial")
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
                .arg("--tutorial")
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
        let milestone_dir = self.cwd().join(".omegon");
        let milestone_file = milestone_dir.join("milestones.json");

        match parts.as_slice() {
            // /milestone — list all milestones
            [] | [""] => {
                let milestones = load_milestones(&milestone_file);
                if milestones.is_empty() {
                    return SlashResult::Display("No milestones defined.\n\nUsage:\n  /milestone doctor\n  /milestone v0.15.0 add <node-id>\n  /milestone v0.15.0 status\n  /milestone v0.15.0 freeze".into());
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
            // /milestone doctor — lifecycle drift audit
            ["doctor"] => {
                let repo_root = crate::setup::find_project_root(self.cwd());
                let findings = crate::lifecycle::doctor::audit_repo(&repo_root);
                if findings.is_empty() {
                    SlashResult::Display("✓ No suspicious lifecycle drift found.".into())
                } else {
                    let mut out = format!("Lifecycle doctor: {} finding(s)\n\n", findings.len());
                    for f in findings {
                        out.push_str(&format!("• {} [{}]\n  {}\n  {}\n\n", f.node_id, f.kind.as_str(), f.title, f.detail));
                    }
                    SlashResult::Display(out.trim_end().to_string())
                }
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
                SlashResult::Display("Usage:\n  /milestone                        — list all\n  /milestone doctor                 — lifecycle drift audit\n  /milestone v0.15.0                — show scope\n  /milestone v0.15.0 add <node-id>  — add node\n  /milestone v0.15.0 remove <node>  — remove node\n  /milestone v0.15.0 freeze         — lock scope\n  /milestone v0.15.0 status         — readiness report".into())
            }
        }
    }

    fn detect_prompt_prefix(text: &str) -> (PromptPrefixMode, String) {
        let trimmed = text.trim_start();
        if let Some(rest) = trimmed.strip_prefix('!') {
            return (PromptPrefixMode::Bash, rest.trim_start().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix('@') {
            return (PromptPrefixMode::Context, rest.trim_start().to_string());
        }
        if let Some(rest) = trimmed.strip_prefix('*') {
            return (
                PromptPrefixMode::MemoryInject,
                rest.trim_start().to_string(),
            );
        }
        (PromptPrefixMode::Agent, text.to_string())
    }

    fn queue_prompt_preview(text: &str, attachments: &[std::path::PathBuf]) -> String {
        let preview = text.chars().take(48).collect::<String>();
        if attachments.is_empty() {
            preview
        } else {
            let names = attachments
                .iter()
                .take(3)
                .filter_map(|path| path.file_name().and_then(|name| name.to_str()))
                .collect::<Vec<_>>();
            let suffix = if attachments.len() > names.len() {
                format!(" +{} more", attachments.len() - names.len())
            } else {
                String::new()
            };
            format!("{} [{}{}]", preview, names.join(", "), suffix)
        }
    }

    fn update_severity(current: &str, latest: &str) -> UpdateSeverity {
        let parse_minor = |value: &str| {
            let base = value.split('-').next().unwrap_or(value);
            let mut parts = base.split('.');
            let major = parts
                .next()
                .and_then(|p| p.parse::<u64>().ok())
                .unwrap_or(0);
            let minor = parts
                .next()
                .and_then(|p| p.parse::<u64>().ok())
                .unwrap_or(0);
            (major, minor)
        };
        let (cur_major, cur_minor) = parse_minor(current);
        let (latest_major, latest_minor) = parse_minor(latest);
        if latest_major > cur_major || (latest_major == cur_major && latest_minor > cur_minor + 1) {
            UpdateSeverity::StaleMinor
        } else {
            UpdateSeverity::Available
        }
    }

    fn queue_prompt(&mut self, text: String, attachments: Vec<std::path::PathBuf>) {
        let preview = Self::queue_prompt_preview(&text, &attachments);
        self.queued_prompts.push_back((text, attachments));
        let queued = self.queued_prompts.len();
        self.conversation
            .push_system(&format!("⏳ Queued [{queued}]: {preview}"));
    }

    async fn submit_editor_buffer(&mut self, command_tx: &mpsc::Sender<TuiCommand>) {
        let (raw_text, attachments) = self.editor.take_submission();
        if raw_text.is_empty() && attachments.is_empty() {
            return;
        }

        if let Ok(mut guard) = self.login_prompt_tx.try_lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(raw_text.clone());
            self.conversation.push_system(&format!("> {raw_text}"));
            return;
        }

        if raw_text.starts_with('/') {
            match self.handle_slash_command(&raw_text, command_tx) {
                SlashResult::Display(response) => {
                    self.history.push(raw_text.clone());
                    self.history_idx = None;
                    self.conversation.push_system(&response);
                }
                SlashResult::Handled => {
                    self.history.push(raw_text.clone());
                    self.history_idx = None;
                }
                SlashResult::Quit => {
                    self.history.push(raw_text.clone());
                    self.history_idx = None;
                    self.should_quit = true;
                    let _ = command_tx.send(TuiCommand::Quit).await;
                }
                SlashResult::NotACommand => {
                    self.submit_prefixed_prompt(raw_text, attachments, command_tx)
                        .await;
                }
            }
            return;
        }

        self.submit_prefixed_prompt(raw_text, attachments, command_tx)
            .await;
    }

    async fn submit_prefixed_prompt(
        &mut self,
        raw_text: String,
        attachments: Vec<std::path::PathBuf>,
        command_tx: &mpsc::Sender<TuiCommand>,
    ) {
        let (prefix_mode, text) = Self::detect_prompt_prefix(&raw_text);
        let text = text.trim().to_string();

        match prefix_mode {
            PromptPrefixMode::Bash => {
                if text.is_empty() {
                    if self.agent_active {
                        self.conversation.push_system(
                            "Shell handoff requires an idle terminal. Cancel the active turn first.",
                        );
                        return;
                    }
                    self.history.push(raw_text.clone());
                    self.history_idx = None;
                    let _ = command_tx
                        .send(TuiCommand::ShellHandoff {
                            keyboard_enhancement: self.keyboard_enhancement,
                        })
                        .await;
                    return;
                }

                self.history.push(raw_text.clone());
                self.history_idx = None;
                self.conversation.push_user(&raw_text);
                let _ = command_tx
                    .send(TuiCommand::RunShellCommand {
                        command: text,
                        respond_to: None,
                    })
                    .await;
                return;
            }
            PromptPrefixMode::Agent
            | PromptPrefixMode::Context
            | PromptPrefixMode::MemoryInject => {}
        }

        if text.is_empty() && attachments.is_empty() {
            return;
        }

        let final_text = match prefix_mode {
            PromptPrefixMode::Agent => text,
            PromptPrefixMode::Bash => unreachable!(),
            PromptPrefixMode::Context => format!("Before answering, request focused context for this query and use it in your response:

{}", text),
            PromptPrefixMode::MemoryInject => {
                let memory_line = format!("Memory recall requested for: {}", text);
                self.conversation.push_system(&memory_line);
                format!("Before answering, recall relevant project memory for this request and incorporate the retrieved facts explicitly:

{}", text)
            }
        };

        if self.agent_active {
            let should_interrupt = matches!(self.queue_mode, PromptQueueMode::InterruptAfterTurn);
            let mode_label = match self.queue_mode {
                PromptQueueMode::InterruptAfterTurn => "after-turn",
                PromptQueueMode::UntilReady => "ready",
                PromptQueueMode::Immediate => "now",
            };
            self.queue_prompt(final_text.clone(), attachments.clone());
            self.conversation
                .push_system(&format!("Queue mode: {mode_label}"));
            if should_interrupt {
                let _ = self.interrupt();
            }
            if let Some(ref mut overlay) = self.tutorial_overlay {
                overlay.check_any_input();
            }
            return;
        }

        if attachments.is_empty() {
            self.conversation.push_user(&final_text);
        } else {
            self.conversation
                .push_user_with_attachments(&final_text, &attachments);
        }
        self.history.push(raw_text.clone());
        self.history_idx = None;
        self.agent_active = true;
        if let Ok(mut ss) = self.dashboard_handles.session.lock() {
            ss.busy = true;
        }
        let _ = command_tx
            .send(TuiCommand::SubmitPrompt(PromptSubmission {
                text: final_text,
                image_paths: attachments,
                submitted_by: "local-tui".to_string(),
                via: "tui",
                queue_mode: self.queue_mode,
            }))
            .await;
        if let Some(ref mut overlay) = self.tutorial_overlay {
            overlay.check_any_input();
        }
    }

    fn interrupt(&self) -> bool {
        if let Ok(guard) = self.cancel.lock()
            && let Some(ref token) = *guard
        {
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
                let decisions_count = sections
                    .as_ref()
                    .map(|s| s.decisions.iter().filter(|d| d.status == "decided").count())
                    .unwrap_or(0);
                let readiness = sections
                    .as_ref()
                    .map(|s| s.readiness_score())
                    .unwrap_or(0.0);
                dashboard::FocusedNodeSummary {
                    id: n.id.clone(),
                    title: n.title.clone(),
                    status: n.status,
                    open_questions: n.open_questions.len() - assumptions,
                    assumptions,
                    decisions: decisions_count,
                    readiness,
                    openspec_change: n.openspec_change.clone(),
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
        self.refresh_at_picker();
        let area = frame.area();
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(Style::default().bg(self.theme.bg())),
            area,
        );

        // Check for available update (non-blocking)
        let update_toast: Option<(String, UpdateSeverity)> = self.update_rx.as_ref().and_then(|rx| {
            let info = rx.borrow();
            let info = info.as_ref()?;
            if info.is_newer && self.footer_data.update_available.as_deref() != Some(info.latest.as_str()) {
                let severity = Self::update_severity(&info.current, &info.latest);
                let msg = match severity {
                    UpdateSeverity::Available => format!(
                        "🆕 Update available: v{} → v{} — run /update",
                        info.current, info.latest
                    ),
                    UpdateSeverity::StaleMinor => format!(
                        "⚠ Version lag: v{} → v{} — you are more than one minor behind. Run /update",
                        info.current, info.latest
                    ),
                };
                Some((msg, severity))
            } else {
                None
            }
        });
        if let Some((msg, severity)) = update_toast {
            // Extract version before mutable borrow
            let version = self
                .update_rx
                .as_ref()
                .and_then(|rx| rx.borrow().as_ref().map(|i| i.latest.clone()));
            if let Some(v) = version {
                self.footer_data.update_available = Some(v);
            }
            let toast_kind = match severity {
                UpdateSeverity::Available => ratatui_toaster::ToastType::Info,
                UpdateSeverity::StaleMinor => ratatui_toaster::ToastType::Warning,
            };
            self.show_toast(&msg, toast_kind);
        }

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

        // ── Focus mode: isolate the selected conversation segment ─────────
        if self.focus_mode && self.conversation.tabs.is_conversation_active() {
            self.render_focus_view(frame, area);

            let now = std::time::Instant::now();
            self.operator_events.retain(|e| e.expires_at > now);
            return;
        }

        // ── Horizontal split: main area | dashboard panel ───────────
        // Dashboard appears as a right-side panel when terminal is wide enough.
        let show_dashboard = self.ui_surfaces.dashboard
            && area.width >= 120
            && (self.dashboard.status_counts.total > 0
                || self.dashboard.focused_node.is_some()
                || !self.dashboard.active_changes.is_empty()
                || self.dashboard.cleave.as_ref().is_some_and(|c| c.active));

        let (main_area, dash_area) = if show_dashboard {
            let h = Layout::horizontal([Constraint::Min(60), Constraint::Length(36)]).split(area);
            (h[0], h[1])
        } else {
            (area, Rect::ZERO)
        };

        // ── Vertical layout in the main area ────────────────────────
        // Editor height tracks wrapped visual rows, not just logical newlines,
        // so long prompts expand the input window instead of pretending to be
        // a single infinitely wrapped line.
        let editor_height = editor_height_for(&self.editor, main_area);

        let footer_height = if self.focus_mode || !self.ui_surfaces.footer {
            0
        } else if self.ui_surfaces.instruments {
            self.instrument_panel.preferred_height()
        } else {
            1
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),                // [0] conversation
                Constraint::Length(editor_height), // [1] editor (dynamic)
                Constraint::Length(footer_height), // [2] footer console (dynamic)
            ])
            .split(main_area);

        // Render tab bar + conversation/widget content
        let t = &self.theme;
        let has_multiple_tabs = self.conversation.tabs.tabs.len() > 1;
        let show_tab_bar = has_multiple_tabs
            && !(matches!(self.ui_mode, UiMode::Slim)
                && !self.ui_surfaces.dashboard
                && !self.ui_surfaces.footer);

        let content_area = if show_tab_bar {
            // Split conversation area into tab bar + content
            let conv_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(chunks[0]);
            self.render_tab_bar(frame, conv_chunks[0]);
            conv_chunks[1]
        } else {
            chunks[0]
        };

        // Render content based on active tab
        if self.conversation.tabs.is_conversation_active() {
            // Render conversation widget (can mutate conv_state via frame.render_stateful_widget)
            let (segments, conv_state) = self.conversation.segments_and_state();
            let conv_widget = conv_widget::ConversationWidget::new(segments, t.as_ref()).with_mode(
                if matches!(self.ui_mode, UiMode::Slim) {
                    SegmentRenderMode::Slim
                } else {
                    SegmentRenderMode::Full
                },
            );
            frame.render_stateful_widget(conv_widget, content_area, conv_state);
        } else {
            // Render extension widget with schema-aware formatting
            match self.conversation.tabs.active() {
                Tab::Extension { widget_id, .. } => {
                    if let Some(widget) = self.extension_widgets.get(widget_id) {
                        widget_renderer::render_widget(
                            frame,
                            content_area,
                            &widget.renderer,
                            &widget.current_data,
                            &widget.label,
                        );
                    }
                }
                _ => {}
            }
        }

        self.conversation_area = Some(chunks[0]);
        self.editor_area = Some(chunks[1]);

        // Overlay images on top of placeholders (second pass — needs Frame for StatefulImage)
        {
            let conv_area = chunks[0];
            // Collect image info without holding borrows
            let image_renders: Vec<(usize, Rect, std::path::PathBuf)> = {
                let segments = self.conversation.segments();
                let conv_state = &self.conversation.conv_state;
                conv_state
                    .visible_image_areas(segments, conv_area)
                    .into_iter()
                    .filter_map(|(idx, area)| {
                        if let SegmentContent::Image { ref path, .. } = segments[idx].content {
                            Some((idx, area, path.clone()))
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            // Now render with mutable access to image_cache
            for (seg_idx, area, path) in image_renders {
                if let Some(protocol) = self.conversation.image_cache.get_or_create(seg_idx, &path)
                {
                    image::render_image(area, frame, protocol);
                }
            }
        }

        // Dashboard panel (right side)
        if show_dashboard && dash_area.width > 0 {
            self.dashboard_area = Some(dash_area);
            self.dashboard.render_themed(dash_area, frame, t.as_ref());
        } else {
            self.dashboard_area = None;
        }

        // ── Sync footer data from settings (every frame) ────
        {
            let s = self.settings();
            self.footer_data.model_id = s.model.clone();
            self.footer_data.model_provider = s.provider().to_string();
            self.footer_data.context_class = s.effective_requested_class();
            self.footer_data.actual_context_class = s.context_class;
            self.footer_data.context_mode = s.context_mode;
            self.footer_data.context_window = s.context_window;
            self.footer_data.thinking_level = s.thinking.as_str().to_string();
            self.footer_data.posture = s.posture.effective.display_name().to_string();
            self.footer_data.runtime_brand = if matches!(self.ui_mode, UiMode::Slim) {
                "OM".to_string()
            } else {
                "Omegon".to_string()
            };
            self.footer_data.principal_id = s
                .operating_profile()
                .identity
                .summary_principal()
                .to_string();
            self.footer_data.authorization = s.operating_profile().authorization.summary();
            self.footer_data.provider_connected = s.provider_connected;
            self.footer_data.is_oauth = crate::providers::resolve_api_key_sync(s.provider())
                .is_some_and(|(_, is_oauth)| is_oauth);
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
                    Some("memory_recall")
                    | Some("memory_query")
                    | Some("memory_episodes")
                    | Some("memory_search_archive") => instruments::WaveDirection::Left, // recall ←
                    Some("memory_supersede") => instruments::WaveDirection::Center, // supersede ↔
                    _ => instruments::WaveDirection::Right,                         // store →
                };
                Some((0usize, dir)) // mind 0 = project for now
            } else {
                None
            };
            self.memory_ops_this_frame = 0;

            let memory_fill = if self.footer_data.context_window > 0 {
                // The memory renderer hard-caps its output at 12_000 chars.
                // At ~4 chars/token that is ~3_000 tokens injected regardless of fact count.
                // The old formula (total_facts * 48 / window) grew with DB size and could
                // consume the entire remaining context budget even at 10% total usage,
                // leaving zero for conversation — making the bar appear "all memory."
                const MEMORY_RENDERER_MAX_CHARS: f64 = 12_000.0;
                const CHARS_PER_TOKEN: f64 = 4.0;
                let max_memory_tokens = MEMORY_RENDERER_MAX_CHARS / CHARS_PER_TOKEN;
                max_memory_tokens / self.footer_data.context_window as f64
            } else {
                0.0
            };
            self.instrument_panel.update_mind_facts(
                self.footer_data.harness.memory.project_facts,
                self.footer_data.harness.memory.working_facts,
                self.footer_data.harness.memory.episodes,
                memory_fill,
            );
            let now = std::time::Instant::now();
            let dt = now
                .duration_since(self.last_instrument_update)
                .as_secs_f64()
                .clamp(0.0, 0.050);
            self.last_instrument_update = now;
            self.instrument_panel.update_telemetry(
                self.footer_data.context_percent,
                self.footer_data.context_window,
                tool_name.as_deref(),
                false,
                thinking,
                mem_op,
                self.agent_active,
                dt,
            );

            // Push live cleave progress into the instrument panel each render tick
            // so the tools→cleave swap happens without turn-boundary latency.
            if let Some(ref cp_lock) = self.dashboard_handles.cleave {
                if let Ok(cp) = cp_lock.lock() {
                    let snapshot = if cp.active { Some(cp.clone()) } else { None };
                    self.instrument_panel.set_cleave_progress(snapshot);
                    // Roll new child tokens into session totals (delta only).
                    let new_in = cp
                        .total_tokens_in
                        .saturating_sub(self.cleave_tokens_accounted_in);
                    let new_out = cp
                        .total_tokens_out
                        .saturating_sub(self.cleave_tokens_accounted_out);
                    if new_in > 0 || new_out > 0 {
                        self.footer_data.session_input_tokens += new_in;
                        self.footer_data.session_output_tokens += new_out;
                        self.cleave_tokens_accounted_in += new_in;
                        self.cleave_tokens_accounted_out += new_out;
                    }
                }
            }
        }

        // ── Unified footer console: engine | inference | tools ──────
        // Store instrument areas for cleanup pass to skip.
        let inst_area = if !self.focus_mode && self.ui_surfaces.footer {
            let footer_area = chunks[2];
            if self.ui_surfaces.instruments {
                let footer_cols = Layout::horizontal([
                    Constraint::Percentage(32),
                    Constraint::Length(1),
                    Constraint::Percentage(35),
                    Constraint::Length(1),
                    Constraint::Percentage(32),
                ])
                .split(footer_area);

                self.footer_data
                    .render_left_panel(footer_cols[0], frame, t.as_ref());
                frame.render_widget(
                    Block::default().style(Style::default().bg(t.footer_bg())),
                    footer_cols[1],
                );
                self.instrument_panel
                    .render_inference_panel(footer_cols[2], frame, t.as_ref());
                frame.render_widget(
                    Block::default().style(Style::default().bg(t.footer_bg())),
                    footer_cols[3],
                );
                self.instrument_panel
                    .render_tools_panel(footer_cols[4], frame, t.as_ref());
                footer_cols[2].union(footer_cols[4])
            } else {
                self.footer_data
                    .render_left_panel(footer_area, frame, t.as_ref());
                footer_area
            }
        } else {
            Rect::ZERO
        };

        // Apply theme to textarea each frame (in case theme changed)
        self.editor.apply_theme(t.as_ref());

        // Editor — shows reverse search prompt, secret input, or normal mode
        if let Some((label, masked)) = self.editor.secret_display() {
            let editor_title = Span::styled(
                format!(" 🔒 {label} "),
                Style::default()
                    .fg(t.warning())
                    .bg(t.surface_bg())
                    .add_modifier(Modifier::BOLD),
            );
            let hint_text = if self.agent_active {
                String::new()
            } else {
                "⏎ confirm  Esc cancel ".into()
            };
            let editor_block = if matches!(self.ui_mode, UiMode::Slim) {
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(t.border_dim()).bg(t.surface_bg()))
                    .title(editor_title)
                    .title_bottom(
                        Line::from(Span::styled(hint_text, Style::default().fg(t.border_dim())))
                            .right_aligned(),
                    )
            } else {
                Block::default()
                    .borders(Borders::TOP)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(t.accent_muted()).bg(t.surface_bg()))
                    .title(editor_title)
                    .title_bottom(
                        Line::from(Span::styled(hint_text, Style::default().fg(t.border_dim())))
                            .right_aligned(),
                    )
            };
            let editor_widget = Paragraph::new(masked)
                .style(Style::default().fg(t.accent_muted()).bg(t.surface_bg()))
                .block(editor_block)
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(editor_widget, chunks[1]);
        } else if let editor::EditorMode::ReverseSearch {
            ref query,
            ref match_idx,
        } = *self.editor.mode()
        {
            let match_text = match_idx
                .and_then(|i| self.history.get(i))
                .map(|s| s.as_str())
                .unwrap_or("");
            let editor_title =
                Span::styled(format!(" (reverse-i-search)`{query}': "), t.style_warning());
            let editor_block = Block::default()
                .borders(Borders::TOP)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(t.accent_muted()).bg(t.surface_bg()))
                .title(editor_title);
            let editor_widget = Paragraph::new(match_text.to_string())
                .style(Style::default().fg(t.fg()).bg(t.surface_bg()))
                .block(editor_block)
                .wrap(ratatui::widgets::Wrap { trim: false });
            frame.render_widget(editor_widget, chunks[1]);
        } else {
            let hint_text = if self.agent_active {
                String::new()
            } else if self.editor.is_empty() {
                if self.ui_surfaces.dashboard {
                    "⏎ send  ⇧⏎/⌥⏎ newline  ^F focus  ^D tree  / commands ".into()
                } else {
                    "⏎ send  ⇧⏎/⌥⏎ newline  ^F focus  /ui surfaces  / commands ".into()
                }
            } else {
                "⏎ send  ⇧⏎/⌥⏎ newline  ↑/↓ history ".into()
            };
            let model_short = self
                .footer_data
                .model_id
                .split(':')
                .last()
                .unwrap_or(&self.footer_data.model_id)
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-");
            let editor_title = if self.agent_active {
                let verb_display = spinner::maybe_glitch(self.working_verb)
                    .unwrap_or_else(|| self.working_verb.to_string());
                Line::from(vec![
                    Span::styled(" ⟳ ", Style::default().fg(t.accent_bright()).add_modifier(ratatui::style::Modifier::BOLD)),
                    Span::styled(format!("{verb_display} "), Style::default().fg(t.accent_muted())),
                ])
            } else {
                Line::from(Span::styled(format!(" {model_short} ▸ "), t.style_accent()))
            };
            let editor_block = Block::default()
                .borders(Borders::TOP)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::default().fg(t.accent_muted()).bg(t.surface_bg()))
                .title(editor_title)
                .title_bottom(
                    Line::from(Span::styled(hint_text, Style::default().fg(t.border_dim())))
                        .right_aligned(),
                );

            let editor_rect = chunks[1];
            // Pre-split using char-boundary wrapping (same algorithm as
            // cursor_screen_position) so the terminal cursor always lands on
            // the correct visual cell.  Paragraph::wrap uses word boundaries
            // which diverge from cursor math and compound across rows.
            // Normal editor mode uses Borders::TOP only: content spans the
            // full width and starts one row below the top border.
            let content_width = editor_rect.width.max(1);
            let visible_rows = editor_rect.height.saturating_sub(1).max(1);
            let visual_lines: Vec<Line<'static>> = if self.editor.is_empty() {
                vec![Line::from(Span::styled(
                    "Ask anything, or type / for commands",
                    Style::default().fg(t.dim()),
                ))]
            } else {
                self.editor
                    .visible_visual_lines(content_width, visible_rows)
                    .into_iter()
                    .map(|vl| {
                        if let Some(summary) = vl.strip_prefix("[Pasted text #") {
                            let summary = summary.strip_suffix(']').unwrap_or(summary).to_string();
                            Line::from(vec![
                                Span::styled("▌", Style::default().fg(t.accent())),
                                Span::styled(" paste ", Style::default().fg(t.bg()).bg(t.accent())),
                                Span::raw(" "),
                                Span::styled(summary, Style::default().fg(t.accent_bright())),
                            ])
                        } else {
                            Line::from(Span::styled(vl.to_string(), Style::default().fg(t.fg())))
                        }
                    })
                    .collect()
            };
            let editor_widget = Paragraph::new(visual_lines)
                .style(Style::default().bg(t.surface_bg()))
                .block(editor_block); // no .wrap() — pre-split above
            frame.render_widget(editor_widget, editor_rect);
            if !self.agent_active {
                let (cx, cy) = self.editor.cursor_screen_position(editor_rect);
                frame.set_cursor_position(ratatui::layout::Position { x: cx, y: cy });
            }
        }

        // Command palette popup (above editor when typing /)
        if !self.agent_active {
            let matches = if self.at_picker.is_some() {
                vec![]
            } else {
                self.matching_commands()
            };
            if !matches.is_empty() {
                let palette_height = matches.len().min(8) as u16 + 2; // +2 for borders
                let editor_area = chunks[1];
                let palette_area = Rect {
                    x: editor_area.x,
                    y: editor_area.y.saturating_sub(palette_height),
                    width: editor_area.width.min(50),
                    height: palette_height,
                };

                let items: Vec<Line<'static>> = matches
                    .iter()
                    .map(|(name, desc)| {
                        Line::from(vec![
                            Span::styled(format!(" /{name}"), t.style_accent()),
                            Span::styled(format!("  {desc}"), t.style_muted()),
                        ])
                    })
                    .collect();

                let palette = Paragraph::new(items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .border_style(t.style_border())
                        .title(Span::styled(" commands ", t.style_dim())),
                );

                // Clear the area first (prevents bleed-through)
                frame.render_widget(ratatui::widgets::Clear, palette_area);
                frame.render_widget(palette, palette_area);
            }

            // Textarea renders its own cursor via cursor_style
        }

        if let Some(ref picker) = self.at_picker {
            picker.render(area, frame, t.as_ref());
        }

        // Selector popup (overlays everything when active)
        if let Some(ref sel) = self.selector {
            sel.render(area, frame, t.as_ref());
        }

        // ── Post-render effects (tachyonfx) — each zone processed separately ──
        self.effects
            .process(frame.buffer_mut(), chunks[0], chunks[2], chunks[1]);

        // ── Tutorial overlay — rendered on top of everything except toasts ──
        if let Some(ref overlay) = self.tutorial_overlay {
            let footer_h = if self.focus_mode { 0 } else { chunks[2].height };
            overlay.render(main_area, frame.buffer_mut(), self.theme.as_ref(), footer_h);
        }

        // ── Toast notifications — rendered last, on top of everything ──
        let now = std::time::Instant::now();
        self.operator_events.retain(|e| e.expires_at > now);
        self.footer_data.operator_events = self
            .operator_events
            .iter()
            .rev()
            .take(2)
            .map(|e| crate::tui::footer::OperatorEventLine {
                icon: e.icon,
                message: e.message.clone(),
                color: e.color,
            })
            .collect();

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
                        && x >= inst_area.x
                        && x < inst_area.right()
                        && y >= inst_area.y
                        && y < inst_area.bottom()
                    {
                        continue;
                    }
                    let cell = &mut buf[(x, y)];
                    let bg = cell.bg;
                    match bg {
                        c if c == base || c == card || c == footer => {}
                        c if c == err_bg || c == diff_add || c == diff_rm => {}
                        _ => {
                            cell.set_bg(base);
                        }
                    }
                }
            }
        }

        // Render modal overlay if active
        if let Some((widget_id, data, auto_dismiss_ms, spawn_time)) = &self.active_modal {
            // Check if modal should auto-dismiss
            if let Some(dismiss_ms) = auto_dismiss_ms {
                if spawn_time.elapsed().as_millis() > *dismiss_ms as u128 {
                    self.active_modal = None;
                } else {
                    self.render_modal(frame, widget_id, data);
                }
            } else {
                self.render_modal(frame, widget_id, data);
            }
        }

        // Render action prompt if active
        if let Some((widget_id, actions)) = &self.active_action_prompt {
            self.render_action_prompt(frame, widget_id, actions);
        }
    }

    fn render_focus_view(&mut self, frame: &mut Frame, area: Rect) {
        self.conversation_area = Some(area);
        self.editor_area = None;
        self.dashboard_area = None;

        let viewport_height = area.height.saturating_sub(1);
        let selected = self.conversation.selected_or_focused_segment();

        let mut lines: Vec<Line<'static>> = Vec::new();
        let segments = self.conversation.segments();
        let mut last_turn: Option<u32> = None;

        for (idx, segment) in segments.iter().enumerate() {
            if matches!(segment.content, SegmentContent::TurnSeparator) {
                continue;
            }

            // ── Turn boundary header ────────────────────────────────
            if let Some(turn) = segment.meta.turn {
                if last_turn != Some(turn) {
                    last_turn = Some(turn);
                    if !lines.is_empty() {
                        let mut turn_spans: Vec<Span<'static>> = vec![
                            Span::styled(
                                "─── ",
                                Style::default().fg(self.theme.border_dim()),
                            ),
                            Span::styled(
                                format!("turn {turn}"),
                                Style::default()
                                    .fg(self.theme.accent_muted())
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ];
                        if let Some(ctx) =
                            segment.meta.context_percent.filter(|p| *p > 5.0)
                        {
                            let ctx_color =
                                widgets::percent_color(ctx, self.theme.as_ref());
                            turn_spans.push(Span::styled(
                                format!(" · ctx:{ctx:.0}%"),
                                Style::default().fg(ctx_color),
                            ));
                        }
                        let fill_width = area.width.saturating_sub(40) as usize;
                        turn_spans.push(Span::styled(
                            format!(" {}", "─".repeat(fill_width)),
                            Style::default().fg(self.theme.border_dim()),
                        ));
                        lines.push(Line::from(turn_spans));
                    }
                }
            }

            let is_selected = selected == Some(idx);
            let presentation = segment.presentation();

            // ── Role + color resolution ─────────────────────────────
            let (role, sigil, color) = match segment.role() {
                crate::tui::segments::SegmentRole::Operator => {
                    ("operator", "OP", self.theme.accent())
                }
                crate::tui::segments::SegmentRole::Assistant => {
                    ("assistant", "Ω", self.theme.success())
                }
                crate::tui::segments::SegmentRole::Tool => {
                    let kind = presentation.tool_visual.unwrap_or(ToolVisualKind::Generic);
                    (kind.label(), "⚙", kind.color(self.theme.as_ref()))
                }
                crate::tui::segments::SegmentRole::System => ("system", "ℹ", self.theme.dim()),
                crate::tui::segments::SegmentRole::Lifecycle => {
                    ("event", "⚡", self.theme.dim())
                }
                crate::tui::segments::SegmentRole::Media => {
                    ("media", "◈", self.theme.accent_muted())
                }
                crate::tui::segments::SegmentRole::Separator => {
                    ("separator", "", self.theme.dim())
                }
            };

            let timestamp: Option<String> = segment.meta.timestamp.and_then(|ts| {
                chrono::DateTime::<chrono::Local>::from(ts)
                    .format("%H:%M:%S")
                    .to_string()
                    .into()
            });

            // Gutter styling — colored `▎` left bar runs through the
            // entire segment, creating a visual stripe that ties header
            // to content. Selected segments use `▌` (thicker) + bold.
            let gutter_char = if is_selected { "▌" } else { "▎" };
            let gutter_style = Style::default().fg(color).add_modifier(if is_selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });

            // ── Header line ────────────────────────────────────────
            let mut header_spans: Vec<Span<'static>> = vec![
                Span::styled(gutter_char.to_string(), gutter_style),
                Span::styled(
                    format!(" {sigil} "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(role.to_string(), Style::default().fg(color)),
            ];

            // Tool-specific summary
            if let SegmentContent::ToolCard {
                ref name,
                ref detail_args,
                ..
            } = segment.content
            {
                let tool_summary = focus_tool_summary(name, detail_args.as_deref());
                header_spans.push(Span::styled(
                    format!(" · {tool_summary}"),
                    Style::default().fg(self.theme.muted()),
                ));
            }

            // Meta tag (model · provider · tier · thinking)
            let meta = build_meta_tag(&segment.meta);
            if !meta.is_empty() {
                header_spans.push(Span::styled(
                    format!("  {meta}"),
                    Style::default().fg(self.theme.dim()),
                ));
            }

            // Right-aligned: duration · tokens · timestamp
            let mut right_parts: Vec<String> = Vec::new();
            if let Some(ms) = segment.meta.duration_ms {
                right_parts.push(segments::format_duration_compact(ms));
            }
            if let Some(tokens) = segment.meta.actual_tokens {
                right_parts.push(tokens.format_compact());
            }
            if let Some(ref stamp) = timestamp {
                right_parts.push(stamp.clone());
            }
            if !right_parts.is_empty() {
                header_spans.push(Span::styled(
                    format!("  {}", right_parts.join(" · ")),
                    Style::default().fg(self.theme.dim()),
                ));
            }

            lines.push(Line::from(header_spans));

            // ── Content body with colored gutter ────────────────────
            let mut content = segment.export_text(SegmentExportMode::Plaintext);
            let expanded = matches!(
                segment.content,
                SegmentContent::ToolCard { expanded: true, .. }
            );
            let max_chars = if is_selected || expanded {
                usize::MAX
            } else {
                2000
            };
            if content.chars().count() > max_chars {
                content = crate::util::truncate(&content, 2000);
                content.push_str("\n… truncated (Enter to expand)");
            }

            let max_lines = if is_selected || expanded { 100 } else { 40 };
            let content_color = match segment.role() {
                crate::tui::segments::SegmentRole::Tool if !is_selected => self.theme.muted(),
                _ => self.theme.fg(),
            };
            // Every content line gets the colored gutter so the stripe
            // runs continuously from header through footer.
            for line in content.lines().take(max_lines) {
                lines.push(Line::from(vec![
                    Span::styled(gutter_char.to_string(), gutter_style),
                    Span::styled(
                        format!("  {line}"),
                        Style::default().fg(content_color),
                    ),
                ]));
            }
            let total_content_lines = content.lines().count();
            if total_content_lines > max_lines {
                lines.push(Line::from(vec![
                    Span::styled(gutter_char.to_string(), gutter_style),
                    Span::styled(
                        format!("  ⋯ {} more lines", total_content_lines - max_lines),
                        Style::default().fg(self.theme.dim()),
                    ),
                ]));
            }

            // Footer — colored corner closes the stripe
            lines.push(Line::from(vec![
                Span::styled("╰", Style::default().fg(color)),
                Span::styled(
                    if is_selected { "── ●" } else { "──" },
                    Style::default().fg(color),
                ),
            ]));
            lines.push(Line::default());
        }
        if lines.last().is_some_and(|line| line.spans.is_empty()) {
            lines.pop();
        }

        let total_lines = lines.len() as u16;
        let max_scroll = total_lines.saturating_sub(viewport_height);
        if self.conversation.conv_state.scroll_offset > max_scroll {
            self.conversation.conv_state.scroll_offset = max_scroll;
        }
        self.conversation.conv_state.user_scrolled =
            self.conversation.conv_state.scroll_offset > 0;
        let top_line = max_scroll.saturating_sub(self.conversation.conv_state.scroll_offset);

        let paragraph = Paragraph::new(lines)
            .style(
                Style::default()
                    .fg(self.theme.fg())
                    .bg(self.theme.surface_bg()),
            )
            .wrap(Wrap { trim: false })
            .scroll((top_line, 0));
        let text_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: viewport_height,
        };
        frame.render_widget(paragraph, text_area);

        let overlay = Paragraph::new(
            "↑/↓ scroll · PgUp/PgDn jump · Home/End · Enter expand · ^Y copy · Esc exit",
        )
        .style(
            Style::default()
                .fg(self.theme.dim())
                .bg(self.theme.surface_bg()),
        )
        .alignment(Alignment::Center);
        let overlay_area = Rect {
            x: area.x,
            y: area.bottom().saturating_sub(1),
            width: area.width,
            height: 1,
        };
        frame.render_widget(overlay, overlay_area);
    }

    /// Render an ephemeral modal from an extension widget.
    fn render_modal(&self, frame: &mut Frame, widget_id: &str, data: &serde_json::Value) {
        let area = frame.area();

        // Center modal in viewport (40% of width, 50% of height)
        let modal_width = (area.width as f32 * 0.4) as u16;
        let modal_height = (area.height as f32 * 0.5) as u16;
        let x = (area.width.saturating_sub(modal_width)) / 2;
        let y = (area.height.saturating_sub(modal_height)) / 2;
        let modal_area = Rect {
            x,
            y,
            width: modal_width,
            height: modal_height,
        };

        // Semi-transparent overlay (dim everything behind modal)
        let overlay = ratatui::widgets::Clear; // Clear background for modal
        frame.render_widget(&overlay, modal_area);

        // Modal content
        let title = widget_id.to_string();
        let json_str = serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string());

        let modal_bg = self.theme.card_bg();
        let block = ratatui::widgets::Block::default()
            .title(format!(" {} ", title))
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Cyan)
                    .bg(modal_bg),
            )
            .style(ratatui::style::Style::default().bg(modal_bg));

        let para = ratatui::widgets::Paragraph::new(json_str)
            .block(block)
            .style(ratatui::style::Style::default().bg(modal_bg))
            .wrap(ratatui::widgets::Wrap { trim: true });

        frame.render_widget(para, modal_area);
    }

    /// Render an action prompt from an extension widget.
    /// Shows numbered buttons for each action.
    fn render_action_prompt(&self, frame: &mut Frame, widget_id: &str, actions: &[String]) {
        let area = frame.area();

        // Center prompt in viewport (50% of width, 30% of height)
        let prompt_width = (area.width as f32 * 0.5) as u16;
        let prompt_height = (area.height as f32 * 0.3) as u16;
        let x = (area.width.saturating_sub(prompt_width)) / 2;
        let y = (area.height.saturating_sub(prompt_height)) / 2;
        let prompt_area = Rect {
            x,
            y,
            width: prompt_width,
            height: prompt_height,
        };

        // Clear overlay
        let overlay = ratatui::widgets::Clear;
        frame.render_widget(&overlay, prompt_area);

        // Build action list
        let mut lines = vec![ratatui::text::Line::from("Choose an action:")];
        lines.push(ratatui::text::Line::from(""));
        for (idx, action) in actions.iter().enumerate().take(9) {
            lines.push(ratatui::text::Line::from(ratatui::text::Span::styled(
                format!("  {} {} ", idx + 1, action),
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Yellow)
                    .bold(),
            )));
        }

        let prompt_bg = self.theme.card_bg();
        let block = ratatui::widgets::Block::default()
            .title(format!(" {} ", widget_id))
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Green)
                    .bg(prompt_bg),
            )
            .style(ratatui::style::Style::default().bg(prompt_bg));

        let para = ratatui::widgets::Paragraph::new(lines)
            .block(block)
            .style(ratatui::style::Style::default().bg(prompt_bg));
        frame.render_widget(para, prompt_area);
    }

    /// Show a transient toast notification.
    /// Try to paste a clipboard image. Shows visible feedback in conversation.
    fn try_paste_clipboard_image(&mut self) {
        if let Some(path) = clipboard_image_to_temp() {
            self.show_toast(
                "📎 Image pasted — send a message to include it",
                ratatui_toaster::ToastType::Info,
            );
            self.editor.insert_attachment(path);
        }
        // No feedback on failure — the user might just be pressing Ctrl+V
        // for a normal text paste that crossterm handles separately.
    }

    fn copy_text_to_clipboard(&self, text: &str) -> bool {
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            let mut child = match std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(_) => return false,
            };
            if let Some(mut stdin) = child.stdin.take() {
                if stdin.write_all(text.as_bytes()).is_err() {
                    let _ = child.wait();
                    return false;
                }
            } else {
                let _ = child.wait();
                return false;
            }
            child.wait().is_ok_and(|status| status.success())
        }

        #[cfg(target_os = "linux")]
        {
            use std::io::Write;
            let commands: &[(&str, &[&str])] = if std::env::var("WAYLAND_DISPLAY").is_ok() {
                &[("wl-copy", &[])]
            } else {
                &[
                    ("xclip", &["-selection", "clipboard"]),
                    ("xsel", &["--clipboard", "--input"]),
                ]
            };
            for (cmd, args) in commands {
                let mut child = match std::process::Command::new(cmd)
                    .args(*args)
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(_) => continue,
                };
                if let Some(mut stdin) = child.stdin.take() {
                    if stdin.write_all(text.as_bytes()).is_err() {
                        let _ = child.wait();
                        continue;
                    }
                } else {
                    let _ = child.wait();
                    continue;
                }
                if child.wait().is_ok_and(|status| status.success()) {
                    return true;
                }
            }
            false
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = text;
            false
        }
    }

    fn copy_selected_conversation_segment_with_mode(&mut self, mode: SegmentExportMode) {
        let Some(text) = self.conversation.selected_segment_text_with_mode(mode) else {
            self.show_toast(
                "Nothing selected to copy",
                ratatui_toaster::ToastType::Warning,
            );
            return;
        };
        if self.copy_text_to_clipboard(&text) {
            let label = match mode {
                SegmentExportMode::Raw => "Copied selected conversation segment",
                SegmentExportMode::Plaintext => "Copied selected conversation segment as plaintext",
            };
            self.show_toast(label, ratatui_toaster::ToastType::Success);
        } else {
            self.show_toast(
                "Clipboard unavailable — select text in your terminal or install pbcopy/wl-copy/xclip",
                ratatui_toaster::ToastType::Warning,
            );
        }
    }

    fn copy_selected_conversation_segment(&mut self) {
        self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Raw);
    }

    fn show_toast(&mut self, message: &str, toast_type: ratatui_toaster::ToastType) {
        let (icon, color) = match toast_type {
            ratatui_toaster::ToastType::Error => ("✖", self.theme.error()),
            ratatui_toaster::ToastType::Warning => ("⚠", self.theme.warning()),
            ratatui_toaster::ToastType::Success => ("✓", self.theme.success()),
            _ => ("ℹ", self.theme.accent_muted()),
        };
        self.operator_events.push_back(OperatorEvent {
            message: message.to_string(),
            color,
            icon,
            expires_at: std::time::Instant::now() + std::time::Duration::from_secs(5),
        });
        while self.operator_events.len() > 8 {
            self.operator_events.pop_front();
        }
    }

    /// Command registry: (name, description, subcommands).
    const COMMANDS: &'static [(&'static str, &'static str, &'static [&'static str])] = &[
        ("help", "show available commands", &[]),
        ("copy", "copy selected segment", &["raw", "plain"]),
        (
            "mouse",
            "toggle pane mouse interaction mode",
            &["on", "off"],
        ),
        ("model", "view or switch model", &["list"]),
        (
            "think",
            "set thinking level",
            &["off", "low", "medium", "high"],
        ),
        ("stats", "session telemetry", &[]),
        ("new", "save current session and start fresh", &[]),
        (
            "detail",
            "toggle tool display (compact/detailed)",
            &["compact", "detailed"],
        ),
        (
            "context",
            "context management (class selection or compaction)",
            &[
                "status", "compact", "clear", "squad", "maniple", "clan", "legion",
            ],
        ),
        ("sessions", "list saved sessions", &[]),
        ("memory", "memory stats", &[]),
        (
            "skills",
            "list or install bundled skills",
            &["list", "install"],
        ),
        (
            "plugin",
            "manage installed plugins",
            &["list", "install", "remove", "update"],
        ),
        (
            "cleave",
            "show cleave status or trigger decomposition",
            &["status"],
        ),
        (
            "login",
            "log in to a provider or service",
            &[
                "anthropic",
                "openai",
                "openai-codex",
                "openrouter",
                "ollama-cloud",
                "github",
            ],
        ),
        (
            "logout",
            "log out of provider",
            &[
                "anthropic",
                "openai",
                "openai-codex",
                "openrouter",
                "ollama-cloud",
            ],
        ),
        ("auth", "authentication management", &["status", "unlock"]),
        (
            "chronos",
            "date/time context",
            &[
                "week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all",
            ],
        ),
        (
            "init",
            "initialize project — scan & migrate agent conventions",
            &["scan", "migrate"],
        ),
        (
            "update",
            "check for and install updates",
            &["install", "channel"],
        ),
        (
            "ui",
            "switch UI presets or toggle individual surfaces",
            &["status", "full", "slim", "show", "hide", "toggle"],
        ),
        ("shackle", "switch to slim constrained mode", &[]),
        ("unshackle", "switch to full harness mode", &[]),
        ("warp", "toggle between slim and full harness modes", &[]),
        (
            "migrate",
            "import from other tools",
            &["auto", "claude-code", "pi", "codex", "cursor", "aider"],
        ),
        (
            "dash",
            "open the Auspex compatibility browser surface (legacy/debug path)",
            &["status"],
        ),
        (
            "auspex",
            "primary local desktop handoff — show status or open Auspex",
            &["status", "open"],
        ),
        (
            "secrets",
            "manage stored secrets",
            &["list", "set", "get", "delete"],
        ),
        (
            "vault",
            "Vault status and management",
            &["status", "unseal", "login", "configure", "init-policy"],
        ),
        (
            "persona",
            "switch persona (or 'off' to deactivate)",
            &["off"],
        ),
        ("tone", "switch tone (or 'off' to deactivate)", &["off"]),
        ("delegate", "delegate task management", &["status"]),
        (
            "status",
            "show harness status (providers, MCP, secrets, routing)",
            &[],
        ),
        ("focus", "toggle instrument panel focus mode", &[]),
        (
            "tree",
            "show design tree summary",
            &["list", "frontier", "ready", "blocked"],
        ),
        (
            "tutorial",
            "interactive tutorial (replaces /demo)",
            &["status", "reset", "consent", "demo"],
        ),
        ("next", "advance to next tutorial lesson", &[]),
        ("prev", "go back to previous tutorial lesson", &[]),
        (
            "milestone",
            "release milestone management",
            &["freeze", "status"],
        ),
        ("splash", "replay splash animation", &[]),
        (
            "note",
            "capture a note for later (persists across sessions)",
            &[],
        ),
        ("notes", "show or clear pending notes", &["clear"]),
        ("checkin", "triage what needs attention now", &[]),
        ("version", "show build version and git sha", &[]),
        ("exit", "quit (or double Ctrl+C)", &[]),
    ];

    /// Handle a slash command.
    fn handle_slash_command(&mut self, text: &str, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        let trimmed = text.trim();
        if !trimmed.starts_with('/') {
            return SlashResult::NotACommand;
        }
        let rest = &trimmed[1..];
        let (cmd, args) = rest.split_once(' ').unwrap_or((rest, ""));
        let args = args.trim();

        // Absolute file paths (e.g. /home/user/file.txt) are not commands
        if cmd.contains('/') {
            return SlashResult::NotACommand;
        }

        // Notify the tutorial overlay that a slash command was executed.
        // This advances Command-triggered steps (e.g. /dash on the Auspex browser step).
        if let Some(ref mut overlay) = self.tutorial_overlay {
            overlay.check_command(cmd);
        }

        match cmd {
            "help" => {
                let lines: Vec<String> = Self::COMMANDS
                    .iter()
                    .map(|(n, d, subs)| {
                        if subs.is_empty() {
                            format!("  /{n:<12} {d}")
                        } else {
                            format!("  /{n:<12} {d}  [{}]", subs.join("|"))
                        }
                    })
                    .collect();
                SlashResult::Display(format!(
                    "Commands:\n{}\n\nType / to browse. Tab completes.",
                    lines.join("\n")
                ))
            }

            "mouse" => match args {
                "" => {
                    self.set_terminal_copy_mode(!self.terminal_copy_mode);
                    SlashResult::Handled
                }
                "on" => {
                    self.set_terminal_copy_mode(false);
                    SlashResult::Handled
                }
                "off" => {
                    self.set_terminal_copy_mode(true);
                    SlashResult::Handled
                }
                _ => SlashResult::Display("Usage: /mouse [on|off]".into()),
            },

            "model" => {
                if args.is_empty() {
                    self.open_model_selector();
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("model", args) {
                        Some(CanonicalSlashCommand::ModelList) => {
                            let _ = tx.try_send(TuiCommand::ModelList { respond_to: None });
                            SlashResult::Handled
                        }
                        Some(CanonicalSlashCommand::SetModel(model)) => {
                            let _ = tx.try_send(TuiCommand::SetModel {
                                model: model.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model → {model}"))
                        }
                        _ => SlashResult::Display("Usage: /model [list|<provider:model>]".into()),
                    }
                }
            }

            "think" => {
                if args.is_empty() {
                    // No args → open interactive selector
                    self.open_thinking_selector();
                    SlashResult::Handled
                } else if let Some(CanonicalSlashCommand::SetThinking(level)) =
                    canonical_slash_command("think", args)
                {
                    let _ = tx.try_send(TuiCommand::SetThinking {
                        level,
                        respond_to: None,
                    });
                    SlashResult::Display(format!("Thinking → {} {}", level.icon(), level.as_str()))
                } else {
                    SlashResult::Display(format!(
                        "Unknown level: {args}. Options: off, low, medium, high"
                    ))
                }
            }

            "skills" => {
                if let Some(command) = canonical_slash_command("skills", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display("Usage: /skills [list|install]".into())
                    }
                } else {
                    SlashResult::Display("Usage: /skills [list|install]".into())
                }
            }

            "plugin" => {
                if let Some(command) = canonical_slash_command("plugin", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /plugin [list|install <uri>|remove <name>|update [name]]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /plugin [list|install <uri>|remove <name>|update [name]]".into(),
                    )
                }
            }

            "stats" => {
                if let Some(command) = canonical_slash_command("stats", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /stats".into())
                }
            }

            "status" => {
                if let Some(command) = canonical_slash_command("status", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /status".into())
                }
            }

            "workspace" => {
                if args == "role" {
                    self.open_workspace_role_selector();
                    SlashResult::Handled
                } else if args == "kind" {
                    self.open_workspace_kind_selector();
                    SlashResult::Handled
                } else {
                    let request = if let Some(command) = canonical_slash_command("workspace", args)
                    {
                        match command {
                            CanonicalSlashCommand::WorkspaceStatusView => {
                                crate::control_runtime::ControlRequest::WorkspaceStatusView
                            }
                            CanonicalSlashCommand::WorkspaceListView => {
                                crate::control_runtime::ControlRequest::WorkspaceListView
                            }
                            CanonicalSlashCommand::WorkspaceNew(label) => {
                                crate::control_runtime::ControlRequest::WorkspaceNew {
                                    label: label.clone(),
                                }
                            }
                            CanonicalSlashCommand::WorkspaceDestroy(target) => {
                                crate::control_runtime::ControlRequest::WorkspaceDestroy {
                                    target: target.clone(),
                                }
                            }
                            CanonicalSlashCommand::WorkspaceAdopt => {
                                crate::control_runtime::ControlRequest::WorkspaceAdopt
                            }
                            CanonicalSlashCommand::WorkspaceRelease => {
                                crate::control_runtime::ControlRequest::WorkspaceRelease
                            }
                            CanonicalSlashCommand::WorkspaceArchive => {
                                crate::control_runtime::ControlRequest::WorkspaceArchive
                            }
                            CanonicalSlashCommand::WorkspacePrune => {
                                crate::control_runtime::ControlRequest::WorkspacePrune
                            }
                            CanonicalSlashCommand::WorkspaceBindMilestone(milestone_id) => {
                                crate::control_runtime::ControlRequest::WorkspaceBindMilestone {
                                    milestone_id: milestone_id.clone(),
                                }
                            }
                            CanonicalSlashCommand::WorkspaceBindNode(design_node_id) => {
                                crate::control_runtime::ControlRequest::WorkspaceBindNode {
                                    design_node_id: design_node_id.clone(),
                                }
                            }
                            CanonicalSlashCommand::WorkspaceBindClear => {
                                crate::control_runtime::ControlRequest::WorkspaceBindClear
                            }
                            CanonicalSlashCommand::WorkspaceRoleView => {
                                crate::control_runtime::ControlRequest::WorkspaceRoleView
                            }
                            CanonicalSlashCommand::WorkspaceRoleSet(role) => {
                                crate::control_runtime::ControlRequest::WorkspaceRoleSet { role }
                            }
                            CanonicalSlashCommand::WorkspaceRoleClear => {
                                crate::control_runtime::ControlRequest::WorkspaceRoleClear
                            }
                            CanonicalSlashCommand::WorkspaceKindView => {
                                crate::control_runtime::ControlRequest::WorkspaceKindView
                            }
                            CanonicalSlashCommand::WorkspaceKindSet(kind) => {
                                crate::control_runtime::ControlRequest::WorkspaceKindSet { kind }
                            }
                            CanonicalSlashCommand::WorkspaceKindClear => {
                                crate::control_runtime::ControlRequest::WorkspaceKindClear
                            }
                            _ => crate::control_runtime::ControlRequest::WorkspaceStatusView,
                        }
                    } else {
                        crate::control_runtime::ControlRequest::WorkspaceStatusView
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                }
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
                    self.open_persona_selector();
                    SlashResult::Handled
                } else {
                    // Activate by name (case-insensitive match)
                    let (personas, _) = crate::plugins::persona_loader::scan_available();
                    let target = args.to_lowercase();
                    match personas.iter().find(|p| {
                        p.name.to_lowercase() == target || p.id.to_lowercase().contains(&target)
                    }) {
                        Some(available) => {
                            match crate::plugins::persona_loader::load_persona(&available.path) {
                                Ok(persona) => {
                                    let name = persona.name.clone();
                                    let badge = persona.badge.clone().unwrap_or_else(|| "⚙".into());
                                    let fact_count = persona.mind_facts.len();
                                    if let Some(ref mut registry) = self.plugin_registry {
                                        registry.activate_persona(persona);
                                    }
                                    SlashResult::Display(format!(
                                        "{badge} Persona activated: {name} ({fact_count} mind facts)"
                                    ))
                                }
                                Err(e) => {
                                    SlashResult::Display(format!("Failed to load persona: {e}"))
                                }
                            }
                        }
                        None => SlashResult::Display(format!(
                            "Persona '{args}' not found. Run /persona to list available."
                        )),
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
                    self.open_tone_selector();
                    SlashResult::Handled
                } else {
                    let (_, tones) = crate::plugins::persona_loader::scan_available();
                    let target = args.to_lowercase();
                    match tones.iter().find(|t| {
                        t.name.to_lowercase() == target || t.id.to_lowercase().contains(&target)
                    }) {
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
                        None => SlashResult::Display(format!(
                            "Tone '{args}' not found. Run /tone to list available."
                        )),
                    }
                }
            }

            "detail" => {
                if args.is_empty() {
                    // Toggle
                    let current = self.settings().tool_detail;
                    let next = match current {
                        crate::settings::ToolDetail::Compact => {
                            crate::settings::ToolDetail::Detailed
                        }
                        crate::settings::ToolDetail::Detailed => {
                            crate::settings::ToolDetail::Compact
                        }
                    };
                    self.update_settings(|s| s.tool_detail = next);
                    SlashResult::Display(format!("Tool display → {}", next.as_str()))
                } else if let Some(mode) = crate::settings::ToolDetail::parse(args) {
                    self.update_settings(|s| s.tool_detail = mode);
                    SlashResult::Display(format!("Tool display → {}", mode.as_str()))
                } else {
                    SlashResult::Display(format!(
                        "Unknown mode: {args}. Options: compact, detailed"
                    ))
                }
            }

            "context" => {
                if args.is_empty() {
                    self.open_context_selector();
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("context", args) {
                        Some(CanonicalSlashCommand::ContextStatus) => {
                            let _ = tx.try_send(TuiCommand::ContextStatus { respond_to: None });
                            SlashResult::Handled
                        }
                        Some(CanonicalSlashCommand::ContextCompact) => {
                            let _ = tx.try_send(TuiCommand::ContextCompact { respond_to: None });
                            SlashResult::Display("Requesting context compaction…".into())
                        }
                        Some(CanonicalSlashCommand::ContextClear) => {
                            let _ = tx.try_send(TuiCommand::ContextClear { respond_to: None });
                            SlashResult::Display("Clearing context…".into())
                        }
                        Some(CanonicalSlashCommand::ContextRequest { kind, query }) => {
                            let display =
                                format!("Requesting mediated context pack for {kind}: {query}");
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request: crate::control_runtime::ControlRequest::ContextRequest {
                                    kind,
                                    query,
                                },
                                respond_to: None,
                            });
                            SlashResult::Display(display)
                        }
                        Some(CanonicalSlashCommand::ContextRequestJson(raw)) => {
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request:
                                    crate::control_runtime::ControlRequest::ContextRequestJson {
                                        raw,
                                    },
                                respond_to: None,
                            });
                            SlashResult::Display(
                                "Requesting mediated context pack from JSON payload".into(),
                            )
                        }
                        Some(CanonicalSlashCommand::SetContextClass(class)) => {
                            let _ = tx.try_send(TuiCommand::ExecuteControl {
                                request: crate::control_runtime::ControlRequest::SetContextClass {
                                    class,
                                },
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Context Policy → {}", class.label()))
                        }
                        _ => {
                            let (sub, _) = args.split_once(' ').unwrap_or((args, ""));
                            SlashResult::Display(format!(
                                "Unknown context option: {sub}.\n\
                                 Use: /context [status|compact|compress|clear|<class>]\n\
                                 Classes: squad, maniple, clan, legion"
                            ))
                        }
                    }
                }
            }

            "new" => {
                let _ = tx.try_send(TuiCommand::NewSession { respond_to: None });
                SlashResult::Handled
            }

            "sessions" => {
                let _ = tx.try_send(TuiCommand::ListSessions { respond_to: None });
                SlashResult::Handled
            }

            "memory" => SlashResult::Display(format!(
                "Memory Overview\n\nFacts\n  Total:            {}\n  Injected:         {}\n  Working set:      {}\n  Estimate:         ~{} tokens\n\nHarness\n  Project facts:    {}\n  Persona facts:    {}\n  Episodes:         {}\n  Active persona:   {}",
                self.footer_data.total_facts,
                self.footer_data.injected_facts,
                self.footer_data.working_memory,
                self.footer_data.memory_tokens_est,
                self.footer_data.harness.memory.project_facts,
                self.footer_data.harness.memory.persona_facts,
                self.footer_data.harness.memory.episodes,
                self.footer_data
                    .harness
                    .memory
                    .active_persona_mind
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            )),

            "auth" => match canonical_slash_command("auth", args) {
                Some(CanonicalSlashCommand::AuthStatus) => {
                    let _ = tx.try_send(TuiCommand::AuthStatus { respond_to: None });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthUnlock) => {
                    let _ = tx.try_send(TuiCommand::AuthUnlock { respond_to: None });
                    SlashResult::Handled
                }
                _ => SlashResult::Display(format!(
                    "Unknown auth command: {args}\n\nUsage:\n  /auth status\n  /auth unlock\n\nUse /login <provider> or /logout <provider> for provider authentication."
                )),
            },

            "update" => {
                let trimmed = args.trim();
                if trimmed == "install" {
                    let info = self.update_rx.as_ref().and_then(|rx| rx.borrow().clone());
                    match info {
                        Some(info) if info.is_newer && !info.download_url.is_empty() => {
                            let args: Vec<String> = std::env::args().skip(1).collect();
                            let keyboard_enhancement = self.keyboard_enhancement;
                            let latest = info.latest.clone();
                            tokio::spawn(async move {
                                match crate::update::download_and_replace(&info).await {
                                    Ok(binary) => {
                                        #[cfg(unix)]
                                        {
                                            let _ = io::stdout().execute(crossterm::event::DisableMouseCapture);
                                            if keyboard_enhancement {
                                                let _ = io::stdout().execute(PopKeyboardEnhancementFlags);
                                            }
                                            let _ = disable_raw_mode();
                                            let _ = io::stdout().execute(LeaveAlternateScreen);
                                        }
                                        let _ = crate::update::exec_restart(&binary, &args);
                                    }
                                    Err(e) => tracing::error!("update install failed: {e}"),
                                }
                            });
                            SlashResult::Display(format!(
                                "Installing v{} and restarting... If replacement fails, relaunch Omegon manually.",
                                latest
                            ))
                        }
                        Some(_) => SlashResult::Display("No downloadable update is available for this platform.".into()),
                        None => SlashResult::Display("No update information available yet. Run `/update` after the background check completes.".into()),
                    }
                } else if let Some(channel_arg) = trimmed.strip_prefix("channel") {
                    let channel_arg = channel_arg.trim();
                    if channel_arg.is_empty() {
                        self.open_update_channel_selector();
                        SlashResult::Handled
                    } else if let Some(channel) = crate::update::UpdateChannel::parse(channel_arg) {
                        self.update_settings(|s| s.update_channel = channel.as_str().to_string());
                        if let Some(tx) = self.update_tx.clone() {
                            crate::update::spawn_check(tx, channel);
                        }
                        SlashResult::Display(format!(
                            "Update channel set to {}. Rechecking for updates now.",
                            channel.as_str()
                        ))
                    } else {
                        SlashResult::Display("Usage: /update channel [stable|rc|nightly]".into())
                    }
                } else {
                    // Check if an update is available
                    let info = self.update_rx.as_ref().and_then(|rx| rx.borrow().clone());
                    let channel = self.settings().update_channel;
                    match info {
                        Some(info) if info.is_newer => SlashResult::Display(format!(
                            "🆕 Update available on {channel}: v{} → v{}\n\n{}\n\n{}\n\nCommands:\n  /update install\n  /update channel [stable|rc|nightly]",
                            info.current,
                            info.latest,
                            if info.release_notes.is_empty() {
                                "(no release notes)".into()
                            } else {
                                info.release_notes
                                    .lines()
                                    .take(20)
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            },
                            if info.download_url.is_empty() {
                                String::from("No binary available for this platform")
                            } else {
                                String::from("Run `/update install` to download and restart")
                            },
                        )),
                        _ => SlashResult::Display(format!(
                            "✓ You're up to date on the {channel} channel.\n\nCommands:\n  /update channel stable  — use stable releases only\n  /update channel rc      — follow release candidates only\n  /update channel nightly — opt into nightly prereleases\n  /update channel         — show current channel"
                        )),
                    }
                }
            }

            "init" => {
                let cwd = std::path::Path::new(&self.footer_data.cwd);
                let move_all = args == "migrate";
                let report = crate::migrate::init_project(cwd, move_all);
                SlashResult::Display(report)
            }

            "migrate" => {
                let source = if args.is_empty() { "auto" } else { args };
                let cwd = self.cwd();
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

            "auspex" => match args {
                "" | "status" => SlashResult::Display(self.auspex_status_text()),
                "open" => {
                    if let Some(ref startup) = self.web_startup {
                        match launch_auspex_with_startup(startup) {
                            Ok(target) => SlashResult::Display(format!(
                                "Launching Auspex via the primary local desktop handoff ({target}).\n\nOmegon is passing native attach metadata for the current live session over `AUSPEX_OMEGON_ATTACH_JSON` with `transport=omegon-ipc`. The embedded browser bridge remains available only as compatibility/debug support behind `/dash`."
                            )),
                            Err(e) => SlashResult::Display(format!("Failed to launch Auspex: {e}")),
                        }
                    } else {
                        let _ = tx.try_send(TuiCommand::StartWebDashboard);
                        SlashResult::Display(
                                "Preparing the local compatibility surface so `/auspex open` can complete the native desktop handoff once startup metadata is available. `/dash` remains the explicit compatibility/debug browser path.".into()
                            )
                    }
                }
                other => SlashResult::Display(format!(
                    "Usage: /auspex status | /auspex open\n\nUnknown subcommand: {other}"
                )),
            },

            "dash" => {
                // /dash remains the compatibility/debug command for opening the browser UI.
                // If the server is already running, open the browser.
                // If not, start it (which auto-opens on ready).
                if let Some(addr) = self.web_server_addr {
                    let url = format!("http://{addr}");
                    if args == "status" {
                        let detail = self
                            .web_startup
                            .as_ref()
                            .map(|startup| {
                                let warnings = if startup.daemon_status.transport_warnings.is_empty() {
                                    "none".to_string()
                                } else {
                                    startup.daemon_status.transport_warnings.join(" | ")
                                };
                                format!(
                                    "\nqueue depth: {}\nprocessed events: {}\ntransport warnings: {}",
                                    startup.daemon_status.queued_events,
                                    startup.daemon_status.processed_events,
                                    warnings,
                                )
                            })
                            .unwrap_or_default();
                        SlashResult::Display(format!(
                            "Auspex compatibility/debug browser path running at {url}{detail}"
                        ))
                    } else {
                        open_browser(&url);
                        SlashResult::Display(format!(
                            "Opened Auspex compatibility/debug browser path at {url}"
                        ))
                    }
                } else {
                    let _ = tx.try_send(TuiCommand::StartWebDashboard);
                    SlashResult::Display("Starting Auspex compatibility/debug browser path…".into())
                }
            }

            "splash" => {
                // Set flag to replay splash on next draw cycle
                self.replay_splash = true;
                SlashResult::Handled
            }

            "delegate" => {
                if let Some(command) = canonical_slash_command("delegate", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /delegate status\n\nTo invoke a delegate, use the delegate agent tool."
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /delegate status\n\nTo invoke a delegate, use the delegate agent tool."
                            .into(),
                    )
                }
            }

            "focus" => {
                self.set_focus_mode(!self.focus_mode);
                let status = if self.focus_mode {
                    "enabled"
                } else {
                    "disabled"
                };
                SlashResult::Display(format!(
                    "Focus mode → {status} (selected segment isolated for terminal-native selection)"
                ))
            }

            "ui" => {
                let args = args.trim();
                if args.is_empty() || args == "status" {
                    SlashResult::Display(self.ui_status_text())
                } else if args == "full" {
                    self.set_ui_mode(UiMode::Full);
                    SlashResult::Display(
                        "UI mode → full (dashboard, instruments, footer enabled)".into(),
                    )
                } else if args == "slim" {
                    self.set_ui_mode(UiMode::Slim);
                    SlashResult::Display("UI mode → slim (conversation-first surfaces)".into())
                } else if let Some(surface) = args.strip_prefix("toggle ") {
                    let surface = surface.trim();
                    let enabled = match surface {
                        "dashboard" | "dash" | "tree" => !self.ui_surfaces.dashboard,
                        "instruments" | "instrument" | "tools" => !self.ui_surfaces.instruments,
                        "footer" | "status" => !self.ui_surfaces.footer,
                        other => {
                            return SlashResult::Display(format!("Unknown UI surface: {other}"));
                        }
                    };
                    match self.toggle_ui_surface(surface, enabled) {
                        Ok(()) => SlashResult::Display(format!(
                            "UI surface {}: {}",
                            if enabled { "enabled" } else { "disabled" },
                            surface
                        )),
                        Err(err) => SlashResult::Display(err),
                    }
                } else if let Some(surface) = args.strip_prefix("show ") {
                    match self.toggle_ui_surface(surface.trim(), true) {
                        Ok(()) => {
                            SlashResult::Display(format!("UI surface enabled: {}", surface.trim()))
                        }
                        Err(err) => SlashResult::Display(err),
                    }
                } else if let Some(surface) = args.strip_prefix("hide ") {
                    match self.toggle_ui_surface(surface.trim(), false) {
                        Ok(()) => {
                            SlashResult::Display(format!("UI surface disabled: {}", surface.trim()))
                        }
                        Err(err) => SlashResult::Display(err),
                    }
                } else {
                    SlashResult::Display(self.ui_status_text())
                }
            }

            "copy" => match args {
                "" | "raw" => {
                    self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Raw);
                    SlashResult::Handled
                }
                "plain" | "plaintext" => {
                    self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Plaintext);
                    SlashResult::Handled
                }
                _ => SlashResult::Display("Usage: /copy [raw|plain]".into()),
            },

            "tree" => {
                if let Some(command) = canonical_slash_command("tree", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /tree [list|... ]".into())
                }
            }

            "milestone" => self.handle_milestone(args),

            "tutorial" | "demo" => self.handle_tutorial(args),

            "next" => self.handle_tutorial_next(),

            "prev" => self.handle_tutorial_prev(),

            "secrets" => self.handle_secrets(args, tx),

            "vault" => {
                if args == "configure" {
                    let options = vec![
                        selector::SelectOption {
                            value: "env".to_string(),
                            label: "Set VAULT_ADDR via environment".to_string(),
                            description: "Write /vault configure env into the editor".to_string(),
                            active: false,
                        },
                        selector::SelectOption {
                            value: "file".to_string(),
                            label: "Create ~/.omegon/vault.json".to_string(),
                            description: "Write /vault configure file into the editor".to_string(),
                            active: false,
                        },
                    ];
                    self.selector = Some(selector::Selector::new(
                        "Vault Configuration — pick a setup flow",
                        options,
                    ));
                    self.selector_kind = Some(SelectorKind::VaultConfigure);
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("vault", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(format!(
                            "Unknown vault subcommand: {args}\nOptions: status, unseal, login, configure, init-policy"
                        ))
                    }
                } else {
                    SlashResult::Display(format!(
                        "Unknown vault subcommand: {args}\nOptions: status, unseal, login, configure, init-policy"
                    ))
                }
            }

            // /login [provider] — open selector or login directly
            "login" => {
                if args.is_empty() {
                    self.open_login_selector();
                    SlashResult::Handled
                } else if crate::auth::provider_by_id(args).is_some_and(|p| {
                    matches!(p.auth_method, crate::auth::AuthMethod::ApiKey)
                        && !p.env_vars.is_empty()
                }) {
                    let key_name = crate::auth::provider_by_id(args)
                        .and_then(|p| p.env_vars.first().copied())
                        .unwrap_or("OPENAI_API_KEY");
                    self.editor.start_secret_input(key_name);
                    SlashResult::Display(format!(
                        "🔒 Paste your {args} API key into {key_name} (input is hidden):"
                    ))
                } else if let Some(CanonicalSlashCommand::AuthLogin(provider)) =
                    canonical_slash_command("login", args)
                {
                    let _ = tx.try_send(TuiCommand::AuthLogin {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /login <provider>".into())
                }
            }

            // /logout [provider] — alias for /auth logout <provider>
            "logout" => {
                if let Some(CanonicalSlashCommand::AuthLogout(provider)) =
                    canonical_slash_command("logout", args)
                {
                    let _ = tx.try_send(TuiCommand::AuthLogout {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(format!(
                        "Usage: /logout <provider>\n\nProviders: {}",
                        crate::auth::operator_auth_provider_help_list()
                    ))
                }
            }

            // /note <text> — append a deferred investigation note
            "note" => {
                if args.is_empty() {
                    return self.handle_slash_command("/notes", tx);
                }
                if let Some(command) = canonical_slash_command("note", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /note <text>".into())
                }
            }

            // /notes [clear] — show or clear pending notes
            "notes" => {
                if let Some(command) = canonical_slash_command("notes", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /notes [clear]".into())
                }
            }

            // /checkin — interactive triage of what needs attention
            "checkin" => {
                if let Some(command) = canonical_slash_command("checkin", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display("Usage: /checkin".into())
                }
            }

            "exit" | "quit" => SlashResult::Quit,

            // ── Aliases ─────────────────────────────────────────────
            "shackle" => {
                self.set_ui_mode(UiMode::Slim);
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim: true },
                    respond_to: None,
                });
                SlashResult::Display("Shackled: om mode active.".into())
            }
            "unshackle" => {
                self.set_ui_mode(UiMode::Full);
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim: false },
                    respond_to: None,
                });
                SlashResult::Display("Unshackled: omegon mode active.".into())
            }
            "warp" => {
                let slim_now = self.settings.lock().ok().is_some_and(|s| s.slim_mode);
                let target_slim = !slim_now;
                self.set_ui_mode(if target_slim {
                    UiMode::Slim
                } else {
                    UiMode::Full
                });
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SetRuntimeMode {
                        slim: target_slim,
                    },
                    respond_to: None,
                });
                SlashResult::Display(if target_slim {
                    "Warped to om mode.".into()
                } else {
                    "Warped to omegon mode.".into()
                })
            }
            "thinking" => self.handle_slash_command(&format!("/think {args}"), tx),
            "models" => self.handle_slash_command("/model", tx),
            "version" => SlashResult::Display(format!(
                "omegon {} ({} {})",
                env!("CARGO_PKG_VERSION"),
                env!("OMEGON_GIT_SHA"),
                env!("OMEGON_BUILD_DATE"),
            )),
            "q" => SlashResult::Quit,

            "cleave" => {
                // Warn, but do not block or silently reroute. Operator agency wins.
                if self.footer_data.is_oauth
                    && crate::providers::anthropic_credential_mode()
                        == crate::providers::AnthropicCredentialMode::OAuthOnly
                {
                    self.show_toast(
                        "Anthropic subscription is active. /cleave may violate Anthropic's \
                         Consumer Terms for Claude.ai / Claude Pro automation. Omegon will \
                         proceed with your requested provider/model; the risk is yours. \
                         Reference: https://www.anthropic.com/legal/consumer-terms",
                        ratatui_toaster::ToastType::Warning,
                    );
                }
                if let Some(command) = canonical_slash_command("cleave", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else if self.bus_commands.iter().any(|c| c.name == "cleave") {
                        let _ = tx.try_send(TuiCommand::BusCommand {
                            name: "cleave".to_string(),
                            args: args.to_string(),
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Cleave extension not loaded. Run omegon from a project directory."
                                .into(),
                        )
                    }
                } else if self.bus_commands.iter().any(|c| c.name == "cleave") {
                    let _ = tx.try_send(TuiCommand::BusCommand {
                        name: "cleave".to_string(),
                        args: args.to_string(),
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Cleave extension not loaded. Run omegon from a project directory.".into(),
                    )
                }
            }

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
                    let matches: Vec<&str> = Self::COMMANDS
                        .iter()
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
                            matches
                                .iter()
                                .map(|m| format!("/{m}"))
                                .collect::<Vec<_>>()
                                .join(", ")
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

    fn is_hidden_bus_command(name: &str) -> bool {
        matches!(name, "opus" | "sonnet" | "haiku")
    }

    /// Palette: matching commands + subcommands for the current editor text.
    fn matching_commands(&self) -> Vec<(String, String)> {
        let text = self.editor.render_text();
        if !text.starts_with('/') {
            return vec![];
        }
        let input = &text[1..];
        let parts: Vec<&str> = input.splitn(2, ' ').collect();

        if parts.len() <= 1 {
            let prefix = parts.first().copied().unwrap_or("");
            let mut matches: Vec<(String, String)> = if prefix.is_empty() {
                Self::COMMANDS
                    .iter()
                    .map(|(n, d, _)| (n.to_string(), d.to_string()))
                    .collect()
            } else {
                Self::COMMANDS
                    .iter()
                    .filter(|(name, _, _)| name.starts_with(prefix))
                    .map(|(n, d, _)| (n.to_string(), d.to_string()))
                    .collect()
            };
            let mut seen: std::collections::HashSet<String> =
                matches.iter().map(|(name, _)| name.clone()).collect();
            // Append bus feature commands without duplicating built-ins.
            for cmd in &self.bus_commands {
                if Self::is_hidden_bus_command(&cmd.name) {
                    continue;
                }
                if (prefix.is_empty() || cmd.name.starts_with(prefix))
                    && seen.insert(cmd.name.clone())
                {
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
                bus_cmd
                    .subcommands
                    .iter()
                    .filter(|s| s.starts_with(sub_prefix))
                    .map(|s| (format!("{cmd} {s}"), String::new()))
                    .collect()
            } else {
                vec![]
            }
        }
    }

    fn is_at_file_picker_trigger(text: &str) -> Option<String> {
        let trimmed = text.trim_start();
        let rest = trimmed.strip_prefix('@')?;
        if rest.contains(' ') || rest.contains('\n') {
            return None;
        }
        Some(rest.to_string())
    }

    fn collect_project_file_matches(&self, query: &str) -> Vec<selector::SelectOption> {
        fn visit(
            root: &std::path::Path,
            dir: &std::path::Path,
            out: &mut Vec<String>,
            depth: usize,
        ) {
            if depth > 5 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') && name != ".env.example" {
                    continue;
                }
                if name == "target" || name == "node_modules" || name == ".git" {
                    continue;
                }
                if path.is_dir() {
                    visit(root, &path, out, depth + 1);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }

        let mut files = Vec::new();
        visit(self.cwd(), self.cwd(), &mut files, 0);
        let q = query.to_lowercase();
        let mut filtered: Vec<String> = files
            .into_iter()
            .filter(|path| q.is_empty() || path.to_lowercase().contains(&q))
            .take(40)
            .collect();
        filtered.sort();
        filtered
            .into_iter()
            .map(|path| selector::SelectOption {
                value: path.clone(),
                label: path.clone(),
                description: "Insert file into prompt context".to_string(),
                active: false,
            })
            .collect()
    }

    fn refresh_at_picker(&mut self) {
        let Some(query) = Self::is_at_file_picker_trigger(&self.editor.render_text()) else {
            self.at_picker = None;
            return;
        };
        let options = self.collect_project_file_matches(&query);
        if options.is_empty() {
            self.at_picker = None;
            return;
        }
        self.at_picker = Some(selector::Selector::new("Inject file into context", options));
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

    fn history_recall_up(&mut self) {
        if self.history_idx.is_some() || self.editor.is_empty() {
            self.history_up();
        }
    }

    fn history_recall_down(&mut self) {
        if self.history_idx.is_some() {
            self.history_down();
        }
    }

    fn should_use_arrow_history_recall(&self) -> bool {
        !self.agent_active
    }

    /// Render tab bar showing all tabs, with active tab highlighted.
    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        frame.render_widget(
            Paragraph::new(Line::from(""))
                .style(Style::default().bg(self.theme.surface_bg()).fg(self.theme.fg())),
            area,
        );

        let mut line_spans = vec![];
        for (idx, tab) in self.conversation.tabs.tabs.iter().enumerate() {
            if idx > 0 {
                line_spans.push(Span::raw(" "));
            }

            let label = tab.label();
            let is_active = idx == self.conversation.tabs.active_tab;

            if is_active {
                // Active tab: reverse video
                line_spans.push(Span::styled(
                    format!(" {} ", label),
                    ratatui::style::Style::default()
                        .bg(ratatui::style::Color::Cyan)
                        .fg(ratatui::style::Color::Black),
                ));
            } else {
                // Inactive tab: dim
                line_spans.push(Span::styled(
                    format!(" {} ", label),
                    ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
                ));
            }
        }

        let line = Line::from(line_spans);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { turn } => {
                self.agent_active = true;
                if let Ok(mut ss) = self.dashboard_handles.session.lock() {
                    ss.busy = true;
                }
                self.turn = turn;
                self.working_verb = spinner::next_verb();
                self.effects.start_spinner_glow();
                self.effects.start_border_pulse();
            }
            AgentEvent::TurnEnd {
                turn,
                model,
                provider,
                estimated_tokens,
                context_window,
                context_composition,
                actual_input_tokens,
                actual_output_tokens,
                cache_read_tokens,
                provider_telemetry,
                ..
            } => {
                self.turn = turn;
                // Accumulate session-long token counts
                self.footer_data.session_input_tokens += actual_input_tokens;
                self.footer_data.session_output_tokens += actual_output_tokens;
                if (actual_input_tokens > 0 || actual_output_tokens > 0)
                    && let Some(model_id) = model
                {
                    self.footer_data
                        .session_usage_slices
                        .push(SessionUsageSlice {
                            model_id,
                            provider: provider.unwrap_or_default(),
                            input_tokens: actual_input_tokens,
                            output_tokens: actual_output_tokens,
                        });
                }
                // Forward raw token counts to the instrument panel
                self.instrument_panel.update_turn_tokens(
                    actual_input_tokens as u32,
                    actual_output_tokens as u32,
                    cache_read_tokens as u32,
                    context_composition,
                    context_window,
                );
                let ctx_window = self.footer_data.context_window;
                if ctx_window > 0 {
                    // Footer context posture is total live-context usage, not the last request's
                    // provider-reported input tokens. ContextUpdated is the authoritative source;
                    // TurnEnd may fill gaps when no prior context snapshot was emitted.
                    let tokens = if estimated_tokens > 0 {
                        estimated_tokens
                    } else {
                        self.footer_data.estimated_tokens
                    };
                    self.footer_data.estimated_tokens = tokens;
                    self.footer_data.context_percent =
                        (tokens as f32 / ctx_window as f32 * 100.0).min(100.0);
                    // Context danger pulse: activate >80%, deactivate <75% (hysteresis)
                    let pct = self.footer_data.context_percent;
                    if pct > 80.0 {
                        self.effects.set_context_danger(true);
                    } else if pct < 75.0 {
                        self.effects.set_context_danger(false);
                    }
                }
                self.footer_data.provider_telemetry = provider_telemetry;

                // Stamp the provider-reported actual tokens onto every
                // segment that belongs to this turn so the title-bar
                // annotation (`↑input ↓output` next to the timestamp)
                // shows up across all of them at once. Tool cards,
                // assistant text, and any other segment created during
                // the turn share the same `meta.turn` from
                // `current_meta()` and pick up the stamp here.
                if actual_input_tokens > 0 || actual_output_tokens > 0 {
                    self.conversation.stamp_turn_tokens(
                        turn,
                        segments::TokenUsage {
                            input: actual_input_tokens,
                            output: actual_output_tokens,
                        },
                    );
                }
                self.effects.ping_footer(self.theme.as_ref());
            }
            AgentEvent::MessageChunk { text } => {
                let was_streaming = self.conversation.is_streaming();
                self.conversation.append_streaming(&text);
                if !was_streaming {
                    // First chunk of a new response — stamp model metadata
                    self.conversation.stamp_meta(self.current_meta());
                }
            }
            AgentEvent::ThinkingChunk { text } => {
                self.instrument_panel.note_thinking_activity();
                let was_streaming = self.conversation.is_streaming();
                self.conversation.append_thinking(&text);
                if !was_streaming {
                    self.conversation.stamp_meta(self.current_meta());
                }
            }
            AgentEvent::ToolStart { id, name, args } => {
                self.working_verb = spinner::next_verb();
                self.instrument_panel.tool_started(&name);
                let args_summary = crate::r#loop::summarize_tool_args(&name, &args);
                // Full args for detailed view
                let detail_args = match name.as_str() {
                    "bash" => args.get("command").and_then(|v| v.as_str()).map(|cmd| {
                        // Strip `cd /path && ` wrapper so the card header shows
                        // the actual command, not a misleading `cd`.
                        if let Some(rest) = cmd.strip_prefix("cd ") {
                            rest.split_once(" && ")
                                .map(|(_, after)| after.to_string())
                                .unwrap_or_else(|| cmd.to_string())
                        } else {
                            cmd.to_string()
                        }
                    }),
                    "read" | "edit" | "write" | "view" => args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    "cleave_run" => {
                        let directive = args
                            .get("directive")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(no directive)");
                        let directive_short = crate::util::truncate(directive, 100);
                        // Parse plan_json to extract child labels
                        let children_line = args
                            .get("plan_json")
                            .and_then(|v| v.as_str())
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                            .and_then(|plan| {
                                plan.get("children")
                                    .and_then(|c| c.as_array())
                                    .map(|children| {
                                        children
                                            .iter()
                                            .filter_map(|c| {
                                                let label =
                                                    c.get("label").and_then(|v| v.as_str())?;
                                                let desc = c
                                                    .get("description")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("");
                                                let desc_short = crate::util::truncate(desc, 60);
                                                Some(format!("  • {label}: {desc_short}"))
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                            })
                            .unwrap_or_default();
                        Some(format!("{directive_short}\n{children_line}"))
                    }
                    "cleave_assess" => args
                        .get("directive")
                        .and_then(|v| v.as_str())
                        .map(|s| crate::util::truncate(s, 120)),
                    // Suppress raw JSON dump for all other harness-internal tools
                    "design_tree" | "design_tree_update" | "openspec_manage" | "memory_store"
                    | "memory_recall" | "memory_focus" | "memory_supersede" | "memory_archive"
                    | "memory_query" | "memory_episodes" | "memory_compact" | "cleave_delegate"
                    | "lifecycle_doctor" => None,
                    _ => Some(serde_json::to_string_pretty(&args).unwrap_or_default()),
                };
                self.conversation.push_tool_start(
                    &id,
                    &name,
                    args_summary.as_deref(),
                    detail_args.as_deref(),
                );
                self.conversation.stamp_meta(self.current_meta());
                self.tool_calls += 1;
                self.last_tool_name = Some(name);
            }
            AgentEvent::ToolEnd {
                id,
                name,
                result,
                is_error,
            } => {
                let text_blocks: Vec<&str> = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        omegon_traits::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let full_text = if text_blocks.is_empty() {
                    None
                } else {
                    Some(text_blocks.join("\n\n"))
                };

                // Append recovery hint for tool errors
                let enriched: Option<String> = if is_error {
                    full_text.as_ref().and_then(|text| {
                        let hint = Self::recovery_hint(Some(name.as_str()), text);
                        if hint.is_empty() {
                            None
                        } else {
                            Some(format!("{text}\n\n💡 {hint}"))
                        }
                    })
                } else {
                    None
                };

                // Use enriched message if available, otherwise the full text payload.
                let display = enriched.as_deref().or(full_text.as_deref());
                self.conversation.push_tool_end(&id, is_error, display);

                // Detect image results from view/render tools
                if !is_error
                    && image::is_available()
                    && let Some(ref name) = self.last_tool_name
                    && matches!(
                        name.as_str(),
                        "view"
                            | "render_diagram"
                            | "generate_image_local"
                            | "render_excalidraw"
                            | "render_composition_still"
                            | "render_native_diagram"
                    )
                    && let Some(ref text) = full_text
                {
                    for line in text.lines() {
                        let trimmed = line.trim();
                        if image::is_image_path(trimmed) && std::path::Path::new(trimmed).exists() {
                            self.conversation
                                .push_image(std::path::PathBuf::from(trimmed), "");
                            break;
                        }
                    }
                }

                // Dynamic footer: memory tools update fact count
                let completed_name = name.as_str();
                let is_memory_mutation = matches!(
                    completed_name,
                    "memory_store" | "memory_supersede" | "memory_archive"
                );
                if completed_name == "memory_store" || completed_name == "memory_supersede" {
                    self.footer_data.total_facts += 1;
                    self.instrument_panel.bump_memory_store();
                } else if completed_name == "memory_archive" {
                    self.footer_data.total_facts = self.footer_data.total_facts.saturating_sub(1);
                }
                if is_memory_mutation {
                    self.memory_ops_this_frame += 1;
                    self.effects.ping_footer(self.theme.as_ref());
                }
                // Also count recall/query operations
                if matches!(
                    completed_name,
                    "memory_recall"
                        | "memory_query"
                        | "memory_episodes"
                        | "memory_search_archive"
                        | "memory_focus"
                        | "memory_release"
                ) {
                    self.memory_ops_this_frame += 1;
                    self.instrument_panel.bump_memory_recall();
                }
                self.instrument_panel
                    .tool_finished(completed_name, is_error);
                self.completed_tool_name = self.last_tool_name.take().or(Some(name));
            }
            AgentEvent::AgentEnd => {
                self.agent_active = false;
                if let Ok(mut ss) = self.dashboard_handles.session.lock() {
                    ss.busy = false;
                }
                self.conversation.finalize_message();
                self.effects.stop_spinner_glow();
                self.effects.stop_border_pulse();
                // Advance tutorial overlay if an AutoPrompt step just completed
                if let Some(ref mut overlay) = self.tutorial_overlay {
                    overlay.on_agent_turn_complete();
                }
            }
            AgentEvent::PhaseChanged { phase } => {
                self.conversation
                    .push_lifecycle("◈", &format!("Phase → {phase:?}"));
            }
            AgentEvent::DecompositionStarted { children } => {
                self.conversation.push_lifecycle(
                    "⚡",
                    &format!("Cleave: {} children dispatched", children.len()),
                );
            }
            AgentEvent::DecompositionChildCompleted { label, success } => {
                let icon = if success { "✓" } else { "✗" };
                self.conversation
                    .push_lifecycle(icon, &format!("Child '{label}' completed"));
            }
            AgentEvent::DecompositionCompleted { merged } => {
                let status = if merged {
                    "merged"
                } else {
                    "completed (no merge)"
                };
                self.conversation
                    .push_lifecycle("⚡", &format!("Cleave {status}"));
            }
            AgentEvent::WebDashboardStarted { startup_json } => {
                if let Ok(startup) =
                    serde_json::from_value::<crate::web::WebStartupInfo>(startup_json)
                    && let Ok(addr) = startup.addr.parse()
                {
                    self.web_server_addr = Some(addr);
                    self.web_startup = Some(startup);
                }
            }
            AgentEvent::SystemNotification { message } => {
                // Transient retry notifications → toast (operator sees them but they
                // don't clutter the conversation). Milestone warnings and other
                // persistent messages → conversation.
                if message.starts_with('⟳')
                    || message.starts_with("Retrying")
                    || message.contains("— retrying")
                {
                    self.show_toast(&message, ratatui_toaster::ToastType::Warning);
                } else if message.starts_with('⚡') {
                    self.show_toast(&message, ratatui_toaster::ToastType::Info);
                } else {
                    self.conversation.push_system(&message);
                }
            }
            AgentEvent::SessionReset => {
                self.conversation = ConversationView::new();
                self.turn = 0;
                self.tool_calls = 0;
                self.last_tool_name = None;
                self.completed_tool_name = None;
                self.active_modal = None;
                self.active_action_prompt = None;
                self.instrument_panel.reset();
                self.footer_data.turn = 0;
                self.footer_data.tool_calls = 0;
                self.footer_data.compactions = 0;
                self.footer_data.update_available = None;
                self.conversation
                    .push_system("New session started. Previous session saved.");
            }
            AgentEvent::HarnessStatusChanged { status_json } => {
                // Deserialize and update the footer's harness status snapshot
                if let Ok(status) =
                    serde_json::from_value::<crate::status::HarnessStatus>(status_json)
                {
                    // Compare with previous status and show toasts for changes
                    if let Some(prev) = self.previous_harness_status.take() {
                        self.show_status_change_toasts(&prev, &status);
                    }

                    // Update footer data and store current status as previous
                    let operating_profile_summary = self
                        .settings()
                        .operating_profile()
                        .with_persona(self.current_persona_state())
                        .summary();
                    let mut status = status;
                    status.operating_profile = operating_profile_summary;
                    self.footer_data.update_harness(status.clone());
                    self.previous_harness_status = Some(status);

                    // Visual effect
                    self.effects.ping_footer(self.theme.as_ref());
                }
            }
            AgentEvent::ContextUpdated {
                tokens,
                context_window,
                context_class,
                thinking_level,
            } => {
                self.footer_data.estimated_tokens = tokens as usize;
                self.footer_data.context_window = context_window as usize;
                self.footer_data.context_class =
                    crate::settings::ContextClass::parse(&context_class).unwrap_or_else(|| {
                        crate::settings::ContextClass::from_tokens(context_window as usize)
                    });
                self.footer_data.actual_context_class =
                    crate::settings::ContextClass::from_tokens(context_window as usize);
                self.footer_data.thinking_level = thinking_level;
                let ctx_window = self.footer_data.context_window;
                self.footer_data.context_percent = if ctx_window > 0 {
                    (tokens as f32 / ctx_window as f32 * 100.0).min(100.0)
                } else {
                    0.0
                };
                self.effects.ping_footer(self.theme.as_ref());
            }
            AgentEvent::MessageAbort { .. } => {
                self.conversation.abort_streaming();
            }
            AgentEvent::ToolUpdate { id, partial } => {
                // Stash the latest streaming partial onto the matching
                // open tool card. The conversation segment renderer
                // picks it up via `live_partial` and displays the live
                // tail / progress / heartbeat in place of the empty
                // result section while the tool is still in flight.
                self.conversation.push_tool_update(&id, partial);
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
    /// Present when a prior session was resumed; drives the welcome brief.
    pub resume_info: Option<crate::setup::ResumeInfo>,
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
    /// Start with tutorial overlay active (--tutorial flag).
    pub start_tutorial: bool,
    /// Shared channel for headless login prompt input. The login task stores a
    /// oneshot sender here; the TUI Enter handler consumes it.
    pub login_prompt_tx:
        std::sync::Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
    /// Extension widgets discovered during setup — for tab rendering.
    pub extension_widgets: Vec<crate::extensions::ExtensionTabWidget>,
    /// Widget event receivers — one per discovered extension.
    pub widget_receivers: Vec<tokio::sync::broadcast::Receiver<crate::extensions::WidgetEvent>>,
}

/// Initial state snapshot gathered during setup, before the TUI event loop starts.
/// Populates footer cards and dashboard on the very first frame.
#[derive(Default)]
pub struct TuiInitialState {
    pub total_facts: usize,
    pub focused_node: Option<dashboard::FocusedNodeSummary>,
    pub active_changes: Vec<dashboard::ChangeSummary>,
    pub workspace_status: Option<String>,
}

/// Path to the editor history file — persists across sessions.
/// Open a URL in the default browser (cross-platform).
pub fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn();
    }
}

/// Monotonic counter for unique clipboard temp filenames.
static CLIPBOARD_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Clipboard format markers → (extension, pasteboard type for AppleScript).
/// `osascript -e 'clipboard info'` outputs markers like «class PNGf»,
/// JPEG picture, TIFF picture — NOT UTI strings like public.png.
const CLIPBOARD_FORMATS: &[(&str, &str, &str)] = &[
    ("PNGf", "png", "«class PNGf»"),
    ("JPEG picture", "jpg", "«class JPEG»"),
    ("JPEG", "jpg", "«class JPEG»"),
    ("TIFF picture", "tiff", "«class TIFF»"),
    ("TIFF", "tiff", "«class TIFF»"),
    ("GIF picture", "gif", "«class GIFf»"),
    ("GIFf", "gif", "«class GIFf»"),
    ("BMP", "bmp", "«class BMP »"),
];

/// Match clipboard info output against known image format markers.
/// Returns (extension, pasteboard_type) if a known image format is found.
#[cfg(target_os = "macos")]
fn match_clipboard_image_format(info_str: &str) -> Option<(&'static str, &'static str)> {
    CLIPBOARD_FORMATS
        .iter()
        .find(|(marker, _, _)| info_str.contains(marker))
        .map(|(_, ext, pb)| (*ext, *pb))
}

/// Try to read image data from the system clipboard and save to a temp file.
///
/// Supports PNG, JPEG, TIFF, GIF, BMP, and WebP. On macOS uses `osascript`
/// to probe clipboard info and AppleScript for extraction.
/// On Linux uses `xclip` or `wl-paste`. Returns the temp file path on success.
fn clipboard_image_to_temp() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Ask the clipboard what types are available
        let info = std::process::Command::new("osascript")
            .args(["-e", "clipboard info"])
            .output()
            .ok()?;
        let info_str = String::from_utf8_lossy(&info.stdout);

        let (ext, pb_type) = match_clipboard_image_format(&info_str)?;

        // Read the raw image data via AppleScript
        let script = format!("set imgData to the clipboard as {pb_type}\nreturn imgData");
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
        let filename = format!(
            "omegon-clipboard-{}-{}.{ext}",
            std::process::id(),
            CLIPBOARD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
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
                        let filename = format!(
                            "omegon-clipboard-{}-{}.{ext}",
                            std::process::id(),
                            CLIPBOARD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        );
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

        let (mime, ext) = types
            .iter()
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
        let filename = format!(
            "omegon-clipboard-{}-{}.{ext}",
            std::process::id(),
            CLIPBOARD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
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

fn build_model_selector_options(
    current: &str,
    anthropic_auth: Option<(String, bool)>,
    openai_auth: Option<(String, bool)>,
    openai_codex_auth: Option<(String, bool)>,
) -> Vec<selector::SelectOption> {
    let mut options: Vec<selector::SelectOption> = Vec::new();

    if let Some((_, is_oauth)) = anthropic_auth {
        let auth = if is_oauth { "oauth" } else { "api key" };
        options.push(sel_opt(
            "anthropic:claude-sonnet-4-6",
            "Sonnet 4.6",
            &format!("Anthropic · balanced · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "anthropic:claude-opus-4-6",
            "Opus 4.6",
            &format!("Anthropic · strongest · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "anthropic:claude-haiku-4-5-20251001",
            "Haiku 4.5",
            &format!("Anthropic · fast · cheap · 200k · {auth}"),
            current,
        ));
    }

    if let Some((_, is_oauth)) = openai_auth {
        let auth = if is_oauth { "oauth" } else { "api key" };
        options.push(sel_opt(
            "openai:gpt-5.4",
            "GPT-5.4",
            &format!("OpenAI API · frontier · 1M · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai:o3",
            "o3",
            &format!("OpenAI API · reasoning · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai:o4-mini",
            "o4-mini",
            &format!("OpenAI API · fast reasoning · 200k · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai:gpt-4.1",
            "GPT-4.1",
            &format!("OpenAI API · coding · 1M · {auth}"),
            current,
        ));
    }

    if let Some((_, is_oauth)) = openai_codex_auth {
        let auth = if is_oauth { "oauth" } else { "api key" };
        options.push(sel_opt(
            "openai-codex:gpt-5.4",
            "GPT-5.4",
            &format!("ChatGPT/Codex · GPT route · 1M · {auth}"),
            current,
        ));
        options.push(sel_opt(
            "openai-codex:gpt-5.4-mini",
            "GPT-5.4 mini",
            &format!("ChatGPT/Codex · fast coding · 1M · {auth}"),
            current,
        ));
    }

    options
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

fn save_milestones(
    path: &std::path::Path,
    milestones: &std::collections::BTreeMap<String, Milestone>,
) -> std::io::Result<()> {
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

        let mut entries: Vec<_> = std::fs::read_dir(tutorial_dir)
            .ok()?
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
            lessons.push(TutorialLesson {
                filename,
                title,
                content,
            });
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
        crossterm::style::Color::Rgb { r: 2, g: 4, b: 8 },
    ))?;
    // Clear the screen with our bg so every pixel starts owned.
    io::stdout().execute(crossterm::terminal::Clear(
        crossterm::terminal::ClearType::All,
    ))?;
    // Default to mouse interaction mode. Terminal-native selection remains
    // available via /mouse off.
    io::stdout().execute(crossterm::event::EnableBracketedPaste)?;

    // Enable Kitty keyboard protocol when the terminal supports it.
    // This lets crossterm distinguish Shift+Enter from Enter, which is
    // required for multiline input. Terminals that don't support it
    // silently ignore the escape sequence.
    let has_keyboard_enhancement =
        crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if has_keyboard_enhancement {
        io::stdout().execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ))?;
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Install panic hook that restores terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = io::stdout().execute(crossterm::event::DisableBracketedPaste);
        let _ = io::stdout().execute(DisableMouseCapture);
        if has_keyboard_enhancement {
            let _ = io::stdout().execute(PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    // Initialise spinner: seed from process start time for variety across
    // sessions, and load user extras from ~/.config/omegon/spinner-verbs.txt.
    let extras_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/omegon/spinner-verbs.txt");
    spinner::init(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as usize)
            .unwrap_or(42),
        if extras_path.exists() {
            Some(extras_path.as_path())
        } else {
            None
        },
    );

    let mouse_enabled = settings.lock().map(|s| s.mouse).unwrap_or(true);
    let mut app = App::new(settings.clone());
    app.keyboard_enhancement = has_keyboard_enhancement;
    // Populate extension widgets and receivers from config
    for widget in config.extension_widgets {
        app.extension_widgets
            .insert(widget.widget_id.clone(), widget);
    }
    app.widget_receivers = config.widget_receivers;
    // Respect the persisted mouse setting (default: true).
    if mouse_enabled {
        app.enable_mouse_interaction_mode();
    }
    app.history = App::load_history(&config.cwd);
    app.footer_data.cwd = config.cwd.clone();
    // Load skills from ~/.omegon/skills/ (bundled) and .omegon/skills/ (project-local).
    if let Some(ref mut registry) = app.plugin_registry {
        registry.load_skills(std::path::Path::new(&config.cwd));
    }
    app.footer_data.is_oauth = config.is_oauth;
    app.bus_commands = config.bus_commands;
    app.dashboard_handles = config.dashboard_handles;
    app.cancel = cancel;

    // Add extension widgets as tabs to the conversation view
    for widget in app.extension_widgets.values() {
        app.conversation
            .tabs
            .add_extension_tab(widget.widget_id.clone(), widget.label.clone());
    }

    // Spawn widget event listener task
    // This task polls all widget_receivers for WidgetEvent updates and relays them to the app
    // via a crossbeam channel. For now, just keep receivers alive (they're stored in app).
    // TODO: Spawn tokio::spawn task with tokio::select! over all receivers
    // and send updates back via a crossbeam channel to the main event loop.

    // Spawn background update check
    let (update_tx, update_rx) = crate::update::channel();
    let update_channel = app.settings().update_channel;
    let channel = crate::update::UpdateChannel::parse(&update_channel)
        .unwrap_or(crate::update::UpdateChannel::Stable);
    // Kick the first update check quickly at startup, then poll periodically.
    crate::update::spawn_check(update_tx.clone(), channel);
    app.update_rx = Some(update_rx);
    app.update_tx = Some(update_tx.clone());
    crate::update::spawn_polling(update_tx, channel);
    app.login_prompt_tx = config.login_prompt_tx;

    // Default to slim/conversation-first startup. Operators can elevate
    // to the full harness via /ui full, /unshackle, or /warp.
    app.set_ui_mode(UiMode::Slim);
    if !app.settings().slim_mode {
        if let Ok(mut s) = app.settings.lock() {
            s.set_slim_mode(true);
        }
    }

    // Pre-populate from initial state so first frame isn't empty
    app.footer_data.total_facts = config.initial.total_facts;
    app.dashboard.focused_node = config.initial.focused_node;
    app.dashboard.active_changes = config.initial.active_changes;

    // Build a contextual welcome / resumption message
    {
        let s = app.settings();
        let project = app
            .footer_data
            .cwd
            .split('/')
            .next_back()
            .unwrap_or("project");
        let facts = app.footer_data.total_facts;

        let version = env!("CARGO_PKG_VERSION");
        let sha = env!("OMEGON_GIT_SHA");

        if let Some(ref ri) = config.resume_info {
            // ── Resumed session: standard welcome + one-line brief ───────
            let mut brief = if s.slim_mode {
                format!("Ω OM {version} ({sha}) — {project}")
            } else {
                format!("Ω Omegon {version} ({sha}) — {project}")
            };
            if s.provider_connected {
                let model_short = s.model_short();
                let ctx = s.context_window / 1000;
                brief.push_str(&format!("\n  ▸ {model_short}  ·  {ctx}k context"));
            } else {
                brief.push_str("\n  ⚠ No provider — use /login to connect");
            }
            if !s.slim_mode && facts > 0 {
                brief.push_str(&format!("  ·  {facts} facts loaded"));
            }
            brief.push('\n');
            if s.slim_mode {
                brief.push_str("\n  Lean coding loop: inspect → edit → validate");
                brief.push_str("\n  /ui full  reveal dashboard + instruments");
                brief.push_str("\n  /unshackle  switch to omegon mode   /help  commands");
                brief.push_str("\n  /model      switch provider          Ctrl+R search history");
            } else {
                brief.push_str("\n  /model  switch provider    /think  reasoning level");
                brief.push_str("\n  /shackle  lean OM mode     /help   all commands");
                brief.push_str("\n  Ctrl+R    search history   Ctrl+C  cancel/quit");
            }
            app.conversation.push_system(&brief);
            // Orientation line: what the model was doing last
            let snippet = if ri.last_prompt_snippet.is_empty() {
                String::new()
            } else {
                format!(" · last: \"{}\"", ri.last_prompt_snippet)
            };
            app.conversation.push_system(&format!(
                "↺ Resumed — {} turns{snippet}. History loaded, you have full prior context.",
                ri.turns,
            ));
        } else {
            // ── Fresh session: standard welcome ───────────────────────────
            let mut welcome = if s.slim_mode {
                format!("Ω OM {version} ({sha}) — {project}")
            } else {
                format!("Ω Omegon {version} ({sha}) — {project}")
            };
            if s.provider_connected {
                let model_short = s.model_short();
                let ctx = s.context_window / 1000;
                welcome.push_str(&format!("\n  ▸ {model_short}  ·  {ctx}k context"));
            } else {
                welcome.push_str("\n  ⚠ No provider — use /login to connect");
            }
            if !s.slim_mode && facts > 0 {
                welcome.push_str(&format!("  ·  {facts} facts loaded"));
            }
            welcome.push('\n');
            if s.slim_mode {
                welcome.push_str("\n  Lean coding loop: inspect → edit → validate");
                welcome.push_str("\n  /ui full     reveal dashboard + instruments");
                welcome.push_str("\n  /unshackle   switch to omegon mode   /help commands");
                welcome.push_str("\n  Ctrl+R       search history          Ctrl+C cancel/quit");
            } else {
                welcome.push_str("\n  /model    switch provider    /think    reasoning level");
                welcome.push_str("\n  /shackle  lean OM mode       /context  context class");
                welcome.push_str("\n  Ctrl+R    search history     Ctrl+C   cancel/quit");
            }
            app.conversation.push_system(&welcome);

            // First-run hint: if no memory facts exist, this is likely a new user.
            if facts == 0 {
                app.conversation.push_system(
                    if s.slim_mode {
                        "💡 Lean mode is active. Start with the file or command you want to inspect. Use /ui full any time to reveal the richer harness surfaces."
                    } else {
                        "💡 First time here? Type /tutorial for a guided tour, or just start typing."
                    },
                );
            }
        }
    }

    // ── Splash screen with real systems check ─────────────────────
    if !config.no_splash {
        let size = terminal.size()?;
        if let Some(mut splash) = splash::SplashScreen::new(size.width, size.height) {
            // Mark all items as scanning
            for item in &[
                "cloud",
                "local",
                "hardware",
                "memory",
                "tools",
                "design",
                "secrets",
                "container",
                "mcp",
            ] {
                splash.set_load_state(item, splash::LoadState::Active);
            }

            // Spawn probes on a background thread with its own tokio runtime.
            // The splash loop blocks the main thread with event::poll(), so
            // tokio::spawn would never make progress — the worker is blocked.
            let (probe_tx, probe_rx) = std::sync::mpsc::channel();
            let probe_cwd = config.cwd.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("probe runtime");
                rt.block_on(crate::startup::run_probes(probe_tx, probe_cwd));
            });
            let mut collected_probes: Vec<crate::startup::ProbeResult> = Vec::new();

            // Run splash animation loop
            let splash_start = std::time::Instant::now();
            let safety_timeout = std::time::Duration::from_secs(5);

            loop {
                // Draw splash (includes tachyonfx post-processing)
                {
                    let t = &app.theme;
                    terminal.draw(|f| splash.draw(f, t.as_ref()))?;
                }

                // Exit after dissolve completes
                if splash.is_dissolved() {
                    break;
                }

                // Poll for keypress at animation frame rate
                let interval = splash::SplashScreen::frame_interval();
                if event::poll(interval)?
                    && matches!(event::read()?, Event::Key(_))
                    && (splash.ready_to_dismiss()
                        || splash_start.elapsed() > std::time::Duration::from_millis(300))
                {
                    splash.dismiss(); // starts dissolve — keep rendering
                }

                splash.tick();

                // Receive probe results as they complete
                while let Ok(result) = probe_rx.try_recv() {
                    splash.receive_probe(result.clone());
                    collected_probes.push(result);
                }

                // Drain agent events to prevent broadcast buffer overflow.
                // HarnessStatusChanged carries the startup memory snapshot —
                // keep the latest one so it isn't lost before the main loop.
                while let Ok(ev) = events_rx.try_recv() {
                    if let AgentEvent::HarnessStatusChanged { status_json } = ev {
                        if let Ok(status) =
                            serde_json::from_value::<crate::status::HarnessStatus>(status_json)
                        {
                            app.footer_data.update_harness(status);
                        }
                    }
                }

                // Safety timeout
                if splash_start.elapsed() > safety_timeout {
                    splash.force_done();
                    splash.dismiss();
                }

                // Auto-dismiss after hold period (~4s — enough for one breathing cycle)
                if splash.ready_to_dismiss() && splash.hold_count > splash::HOLD_FRAMES + 90 {
                    splash.dismiss();
                }
            }

            // Drain any stragglers that arrived after the last loop iteration
            while let Ok(result) = probe_rx.try_recv() {
                collected_probes.push(result);
            }
            // Classify capability tier from ALL collected results
            app.capability_tier = Some(crate::startup::classify_tier(&collected_probes));
        }
    }

    // ── Anthropic subscription ToS one-time startup notice ──────────────────
    // Shown once per session when only an OAuth/subscription token is present.
    // Warns early, but does not remove operator agency.
    if app.footer_data.is_oauth
        && crate::providers::anthropic_credential_mode()
            == crate::providers::AnthropicCredentialMode::OAuthOnly
        && !app.oauth_tos_notice_shown
    {
        app.oauth_tos_notice_shown = true;
        app.show_toast(
            "Claude.ai subscription active. Anthropic's Consumer Terms may restrict \
             automated/background use for this credential. Omegon will warn and disclose, \
             but your provider choice remains yours. See: anthropic.com/legal/consumer-terms",
            ratatui_toaster::ToastType::Warning,
        );
    }

    // Queue startup reveal effects (footer sweep-in, conversation fade)


    // Queue initial prompt if provided (--initial-prompt / --initial-prompt-file)
    if let Some(prompt) = config.initial_prompt {
        app.queue_prompt(prompt, Vec::new());
    }

    // Start tutorial overlay if --tutorial flag was passed (e.g. from demo exec)
    if config.start_tutorial {
        let has_design = app.dashboard.status_counts.total > 0;
        app.tutorial_overlay = Some(tutorial::Tutorial::new_demo(has_design));
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
                    if splash.is_dissolved() {
                        break;
                    }
                    let interval = splash::SplashScreen::frame_interval();
                    if event::poll(interval)? {
                        let ev = event::read()?;
                        if matches!(ev, Event::Key(_) | Event::Mouse(_)) {
                            splash.dismiss();
                        }
                    }
                    splash.tick();
                    // Auto-end after full animation + hold
                    if splash.frame > splash::TOTAL_FRAMES + splash::HOLD_FRAMES + 20 {
                        splash.dismiss();
                    }
                }
            }
        }

        // Drain agent events BEFORE drawing — so telemetry counters
        // (memory_ops, tool_calls) are current when draw reads them
        while let Ok(agent_event) = events_rx.try_recv() {
            app.handle_agent_event(agent_event);
        }

        // Poll widget receivers for updates
        for rx in &mut app.widget_receivers {
            while let Ok(event) = rx.try_recv() {
                match event {
                    crate::extensions::WidgetEvent::Update {
                        widget_id,
                        title,
                        data,
                    } => {
                        if let Some(widget) = app.extension_widgets.get_mut(&widget_id) {
                            if let Some(new_title) = title {
                                widget.label = new_title;
                            }
                            widget.current_data = data;
                            // Frame will automatically re-render with updated data
                        }
                    }
                    crate::extensions::WidgetEvent::ShowModal {
                        widget_id,
                        data,
                        auto_dismiss_ms,
                    } => {
                        app.active_modal =
                            Some((widget_id, data, auto_dismiss_ms, std::time::Instant::now()));
                    }
                    crate::extensions::WidgetEvent::ActionRequired { widget_id, actions } => {
                        app.active_action_prompt = Some((widget_id, actions));
                    }
                }
            }
        }

        // Draw
        terminal.draw(|f| app.draw(f))?;

        // Poll for events with timeout (16ms ≈ 60fps)
        let has_terminal_event = event::poll(Duration::from_millis(16))?;

        if has_terminal_event {
            match event::read()? {
                // ── Mouse scroll ────────────────────────────────────────
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) if app.mouse_capture_enabled => {
                        let point_in = |area: Option<Rect>| {
                            area.is_some_and(|area| {
                                mouse.column >= area.x
                                    && mouse.column < area.x + area.width
                                    && mouse.row >= area.y
                                    && mouse.row < area.y + area.height
                            })
                        };

                        if point_in(app.dashboard_area) {
                            app.pane_focus = PaneFocus::Dashboard;
                            app.dashboard.sidebar_active = true;
                        } else if point_in(app.conversation_area) {
                            app.pane_focus = PaneFocus::Conversation;
                            app.dashboard.sidebar_active = false;
                            if let Some(area) = app.conversation_area
                                && let Some(idx) = app.conversation.segment_at(area, mouse.row)
                            {
                                let now = std::time::Instant::now();
                                let is_double = app.last_left_click.is_some_and(|(col, row, t)| {
                                    row == mouse.row
                                        && col.abs_diff(mouse.column) <= 1
                                        && row.abs_diff(mouse.row) <= 1
                                        && now.duration_since(t) <= Duration::from_millis(400)
                                });
                                app.conversation.select_segment(idx);
                                if is_double {
                                    app.conversation.toggle_expand(idx);
                                }
                                app.last_left_click = Some((mouse.column, mouse.row, now));
                            }
                        } else if point_in(app.editor_area) {
                            app.pane_focus = PaneFocus::Editor;
                            app.dashboard.sidebar_active = false;
                        }
                    }
                    MouseEventKind::ScrollUp if app.mouse_capture_enabled => {
                        let over_dashboard = app.dashboard_area.is_some_and(|area| {
                            mouse.column >= area.x
                                && mouse.column < area.x + area.width
                                && mouse.row >= area.y
                                && mouse.row < area.y + area.height
                        });
                        let over_conversation = app.conversation_area.is_some_and(|area| {
                            mouse.column >= area.x
                                && mouse.column < area.x + area.width
                                && mouse.row >= area.y
                                && mouse.row < area.y + area.height
                        });
                        if over_dashboard {
                            app.dashboard.scroll_up(3);
                        } else if over_conversation {
                            app.conversation.scroll_up(3);
                        }
                    }
                    MouseEventKind::ScrollDown if app.mouse_capture_enabled => {
                        let over_dashboard = app.dashboard_area.is_some_and(|area| {
                            mouse.column >= area.x
                                && mouse.column < area.x + area.width
                                && mouse.row >= area.y
                                && mouse.row < area.y + area.height
                        });
                        let over_conversation = app.conversation_area.is_some_and(|area| {
                            mouse.column >= area.x
                                && mouse.column < area.x + area.width
                                && mouse.row >= area.y
                                && mouse.row < area.y + area.height
                        });
                        if over_dashboard {
                            app.dashboard.scroll_down(3);
                        } else if over_conversation {
                            app.conversation.scroll_down(3);
                        }
                    }
                    _ => {}
                },
                // ── Paste — pass directly to textarea ──────────
                Event::Paste(ref text) => {
                    if matches!(app.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                        // In secret mode, paste goes into the hidden buffer
                        for c in text.chars() {
                            app.editor.secret_insert(c);
                        }
                    } else if text.is_empty() {
                        app.try_paste_clipboard_image();
                    } else {
                        app.editor.insert_paste(text);
                    }
                }
                // ── Ctrl+V: check for clipboard image ──────────
                Event::Key(KeyEvent {
                    code: KeyCode::Char('v'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => {
                    if matches!(app.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                        // In secret mode, try to paste from clipboard into hidden buffer
                        // (Ctrl+V may deliver text as a Key event on some terminals)
                    } else {
                        app.try_paste_clipboard_image();
                    }
                }
                Event::Key(key) => {
                    // ── Selector popup intercepts all keys when open ────
                    if app.selector.is_some() {
                        match key.code {
                            KeyCode::Up => {
                                if let Some(ref mut s) = app.selector {
                                    s.move_up();
                                }
                            }
                            KeyCode::Down => {
                                if let Some(ref mut s) = app.selector {
                                    s.move_down();
                                }
                            }
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

                    // ── Secret input mode intercepts keys ────────────
                    if matches!(app.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                        match key.code {
                            KeyCode::Char(c) => {
                                app.editor.secret_insert(c);
                            }
                            KeyCode::Backspace => {
                                app.editor.secret_backspace();
                            }
                            KeyCode::Enter => {
                                if let Some((label, value)) = app.editor.take_secret() {
                                    if value.is_empty() {
                                        app.conversation
                                            .push_system("Cancelled — no value entered.");
                                    } else {
                                        // Store in secrets engine
                                        let _ = command_tx
                                            .send(TuiCommand::ExecuteControl {
                                                request: crate::control_runtime::ControlRequest::SecretsSet {
                                                    name: label.clone(),
                                                    value: value.clone(),
                                                },
                                                respond_to: None,
                                            })
                                            .await;

                                        // For provider keys, also write to auth.json so the
                                        // provider resolution chain finds them (/login checks
                                        // auth.json, not the secrets keyring)
                                        // Look up provider by env var name using canonical map
                                        let provider = crate::auth::PROVIDERS
                                            .iter()
                                            .find(|p| p.env_vars.contains(&label.as_str()));
                                        if let Some(p) = provider {
                                            let creds = crate::auth::OAuthCredentials {
                                                cred_type: "api-key".into(),
                                                access: value.clone(),
                                                refresh: String::new(),
                                                expires: u64::MAX,
                                            };
                                            let _ =
                                                crate::auth::write_credentials(p.auth_key, &creds);
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                app.editor.cancel_secret();
                                app.conversation.push_system("Secret input cancelled.");
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

                    // ── Tutorial overlay intercepts keys when active ────
                    if let Some(ref mut overlay) = app.tutorial_overlay {
                        if overlay.active {
                            let step_trigger = overlay.step().trigger.clone();
                            match key.code {
                                KeyCode::Esc => {
                                    overlay.dismiss();
                                    continue;
                                }
                                KeyCode::BackTab => {
                                    overlay.go_back();
                                    continue;
                                }
                                KeyCode::Tab => {
                                    match &step_trigger {
                                        tutorial::Trigger::Tab => {
                                            // Check BEFORE advance — fire side-effects for the step being dismissed
                                            let leaving_step_title = overlay.step().title;
                                            let should_open_dash =
                                                leaving_step_title == "Auspex Browser View";
                                            overlay.advance();
                                            let auto_prompt = overlay
                                                .pending_auto_prompt()
                                                .map(|s| s.to_string());
                                            if auto_prompt.is_some() {
                                                overlay.mark_auto_prompt_sent();
                                            }
                                            // Drop overlay borrow before touching app
                                            drop(step_trigger);
                                            if let Some(prompt) = auto_prompt {
                                                if !app.agent_active {
                                                    app.conversation.push_system("▸ tutorial step");
                                                    app.agent_active = true;
                                                    if let Ok(mut ss) =
                                                        app.dashboard_handles.session.lock()
                                                    {
                                                        ss.busy = true;
                                                    }
                                                    let _ = command_tx
                                                        .send(TuiCommand::SubmitPrompt(
                                                            PromptSubmission {
                                                                text: prompt,
                                                                image_paths: Vec::new(),
                                                                submitted_by: "local-tui"
                                                                    .to_string(),
                                                                via: "tui",
                                                                queue_mode: app.queue_mode,
                                                            },
                                                        ))
                                                        .await;
                                                } else {
                                                    app.queue_prompt(prompt, Vec::new());
                                                }
                                            }
                                            if should_open_dash {
                                                let _ = command_tx
                                                    .send(TuiCommand::StartWebDashboard)
                                                    .await;
                                            }
                                            continue;
                                        }
                                        tutorial::Trigger::AutoPrompt(prompt) => {
                                            if !overlay.auto_prompt_sent {
                                                // Tab starts the auto-prompt
                                                let prompt = prompt.to_string();
                                                overlay.mark_auto_prompt_sent();
                                                if !app.agent_active {
                                                    app.conversation.push_system("▸ tutorial step");
                                                    app.agent_active = true;
                                                    if let Ok(mut ss) =
                                                        app.dashboard_handles.session.lock()
                                                    {
                                                        ss.busy = true;
                                                    }
                                                    let _ = command_tx
                                                        .send(TuiCommand::SubmitPrompt(
                                                            PromptSubmission {
                                                                text: prompt,
                                                                image_paths: Vec::new(),
                                                                submitted_by: "local-tui"
                                                                    .to_string(),
                                                                via: "tui",
                                                                queue_mode: app.queue_mode,
                                                            },
                                                        ))
                                                        .await;
                                                } else {
                                                    app.queue_prompt(prompt, Vec::new());
                                                }
                                            }
                                            // If already sent, Tab does nothing — wait for agent
                                            continue;
                                        }
                                        tutorial::Trigger::Command(_)
                                        | tutorial::Trigger::AnyInput => {
                                            // Tab passes through to normal key handling (e.g., command completion)
                                        }
                                    }
                                }
                                KeyCode::Left | KeyCode::Right if overlay.showing_choice() => {
                                    overlay.toggle_choice();
                                    continue;
                                }
                                KeyCode::Enter if overlay.showing_choice() => {
                                    overlay.confirm_choice();
                                    if overlay.choice == tutorial::TutorialChoice::Demo {
                                        // Demo mode needs the demo project — dismiss overlay
                                        // and launch the clone+exec flow
                                        overlay.dismiss();
                                        let result = app.launch_tutorial_project();
                                        if let SlashResult::Display(msg) = result {
                                            app.conversation.push_system(&msg);
                                        }
                                    } else {
                                        // MyProject: advance past the choice step to the welcome
                                        overlay.advance();
                                    }
                                    continue;
                                }
                                _ => {
                                    // For Command and AnyInput steps, let keys pass through
                                    // to the editor so the user can type.
                                    // For Enter and AutoPrompt steps, consume the key (overlay blocks).
                                    match &step_trigger {
                                        tutorial::Trigger::Command(_)
                                        | tutorial::Trigger::AnyInput => {
                                            // Fall through to normal key handling
                                        }
                                        _ => {
                                            // Consume — overlay blocks input
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Sidebar navigation mode ──────────────────────
                    // When dashboard sidebar is active, route keys to the tree.
                    // Enter on a selected node triggers design-focus via bus.
                    if app.dashboard.sidebar_active {
                        if key.code == KeyCode::Enter {
                            if let Some(node_id) =
                                app.dashboard.selected_node_id().map(|s| s.to_string())
                            {
                                let _ = command_tx
                                    .send(TuiCommand::BusCommand {
                                        name: "design-focus".into(),
                                        args: node_id,
                                    })
                                    .await;
                                app.dashboard.sidebar_active = false;
                            }
                            continue;
                        }
                        if app.dashboard.handle_key(key) {
                            continue;
                        }
                    }

                    // Handle action prompt input (1-9 keys) before other keys
                    if let Some((widget_id, actions)) = &app.active_action_prompt {
                        if let KeyCode::Char(c) = key.code {
                            if let Some(digit) = c.to_digit(10) {
                                let idx = (digit - 1) as usize;
                                if idx < actions.len() {
                                    let action = actions[idx].clone();
                                    // Log the action selection
                                    app.conversation
                                        .push_system(&format!("✓ {}: {}", widget_id, action));
                                    app.active_action_prompt = None;
                                    // TODO: Send action back to extension via IPC
                                    continue;
                                }
                            }
                        }
                    }

                    if app.focus_mode {
                        match (key.code, key.modifiers) {
                            (KeyCode::Up, _) | (KeyCode::Left, _) => {
                                app.conversation.scroll_up(3);
                                continue;
                            }
                            (KeyCode::Down, _) | (KeyCode::Right, _) => {
                                app.conversation.scroll_down(3);
                                continue;
                            }
                            (KeyCode::PageUp, _) => {
                                app.conversation.scroll_up(20);
                                continue;
                            }
                            (KeyCode::PageDown, _) => {
                                app.conversation.scroll_down(20);
                                continue;
                            }
                            (KeyCode::Home, _) => {
                                app.conversation.conv_state.scroll_offset = u16::MAX;
                                app.conversation.conv_state.user_scrolled = true;
                                continue;
                            }
                            (KeyCode::End, _) => {
                                app.conversation.scroll_down(u16::MAX);
                                continue;
                            }
                            _ => {}
                        }
                    }

                    match (key.code, key.modifiers) {
                        // ── Interrupt: Escape or Ctrl+C ─────────────────
                        (KeyCode::Esc, _) => {
                            // Dismiss modal/focus if active, otherwise interrupt agent
                            if app.active_modal.is_some() {
                                app.active_modal = None;
                            } else if app.active_action_prompt.is_some() {
                                app.active_action_prompt = None;
                            } else if app.focus_mode {
                                app.set_focus_mode(false);
                            } else if app.agent_active {
                                if app.interrupt() {
                                    app.conversation.push_system(
                                        "⎋ Interrupt requested — waiting for turn to stop",
                                    );
                                } else {
                                    app.conversation.push_system("⎋ Interrupt requested");
                                }
                            }
                        }
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            if app.agent_active {
                                if app.interrupt() {
                                    app.conversation.push_system(
                                        "⎋ Interrupt requested (Ctrl+C) — waiting for turn to stop",
                                    );
                                } else {
                                    app.conversation
                                        .push_system("⎋ Interrupt requested (Ctrl+C)");
                                }
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
                            if matches!(app.pane_focus, PaneFocus::Conversation) {
                                app.copy_selected_conversation_segment();
                            } else {
                                app.editor.yank();
                            }
                        }
                        (KeyCode::Char('t'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)
                        | (KeyCode::Char('T'), KeyModifiers::CONTROL) => {
                            app.set_terminal_copy_mode(!app.terminal_copy_mode);
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

                        // Ctrl+O: toggle pin/expand on nearest tool card
                        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                            app.conversation.toggle_pin();
                        }

                        // Ctrl+F: toggle focus mode (copy-first selected segment view)
                        (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                            app.set_focus_mode(!app.focus_mode);
                        }

                        // Ctrl+G: UI full preset
                        (KeyCode::Char('g'), KeyModifiers::CONTROL) => {
                            app.set_ui_mode(UiMode::Full);
                            app.show_toast("UI mode → full", ratatui_toaster::ToastType::Info);
                        }

                        // Ctrl+L: UI slim preset
                        (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                            app.set_ui_mode(UiMode::Slim);
                            app.show_toast("UI mode → slim", ratatui_toaster::ToastType::Info);
                        }

                        // Ctrl+D: toggle sidebar navigation mode (design tree)
                        (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                            app.dashboard.sidebar_active = !app.dashboard.sidebar_active;
                            if app.dashboard.sidebar_active
                                && app.dashboard.tree_state.selected().is_empty()
                            {
                                app.dashboard.tree_state.select_first();
                            }
                        }

                        // Tab: command completion, @-picker insertion, or toggle tool card expansion
                        (KeyCode::Tab, _) => {
                            let text = app.editor.render_text().to_string();
                            if let Some(ref picker) = app.at_picker {
                                let path = picker.selected_value().to_string();
                                let full = app.cwd().join(&path);
                                app.editor.set_text("");
                                app.editor.insert_attachment(full);
                                app.at_picker = None;
                            } else if text.starts_with('/') {
                                let matches = app.matching_commands();
                                if matches.len() == 1 {
                                    let cmd = format!("/{}", matches[0].0);
                                    app.editor.set_text(&cmd);
                                }
                            } else if text.is_empty() {
                                if let Some(idx) = app.conversation.focused_tool_card() {
                                    app.conversation.toggle_expand(idx);
                                }
                            }
                        }

                        // Alt+N: next conversation tab
                        (KeyCode::Char('n'), KeyModifiers::ALT) => {
                            if app.conversation.tabs.tabs.len() > 1 {
                                app.conversation.tabs.next_tab();
                            }
                        }

                        // Alt+P: previous conversation tab
                        (KeyCode::Char('p'), KeyModifiers::ALT) => {
                            if app.conversation.tabs.tabs.len() > 1 {
                                app.conversation.tabs.prev_tab();
                            }
                        }

                        // Shift+Enter or Alt+Enter: insert newline (multiline input)
                        (KeyCode::Enter, m)
                            if m.contains(KeyModifiers::SHIFT) || m.contains(KeyModifiers::ALT) =>
                        {
                            if !app.agent_active {
                                app.editor.insert_newline();
                            }
                        }

                        // Enter in focus mode toggles expansion for the focused segment.
                        (KeyCode::Enter, _) if app.focus_mode => {
                            if let Some(idx) = app.conversation.timeline_focused_segment() {
                                app.conversation.toggle_timeline_expanded_segment(idx);
                            }
                        }

                        // Submit / @-picker confirm
                        (KeyCode::Enter, _) => {
                            if let Some(ref picker) = app.at_picker {
                                let path = picker.selected_value().to_string();
                                let full = app.cwd().join(&path);
                                app.editor.set_text("");
                                app.editor.insert_attachment(full);
                                app.at_picker = None;
                            } else {
                                app.submit_editor_buffer(&command_tx).await;
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
                            if matches!(app.pane_focus, PaneFocus::Editor) {
                                app.editor.move_word_backward();
                            }
                        }
                        (KeyCode::Right, KeyModifiers::ALT) => {
                            if matches!(app.pane_focus, PaneFocus::Editor) {
                                app.editor.move_word_forward();
                            }
                        }
                        (KeyCode::Left, _) => {
                            if matches!(app.pane_focus, PaneFocus::Editor) {
                                app.editor.move_left();
                            }
                        }
                        (KeyCode::Right, _) => {
                            if matches!(app.pane_focus, PaneFocus::Editor) {
                                app.editor.move_right();
                            }
                        }
                        (KeyCode::Home, _) => {
                            if matches!(app.pane_focus, PaneFocus::Editor) {
                                app.editor.move_home();
                            }
                        }
                        (KeyCode::End, _) => {
                            if matches!(app.pane_focus, PaneFocus::Editor) {
                                app.editor.move_end();
                            }
                        }

                        // ── Scrolling ────────────────────────────────
                        (KeyCode::Up, KeyModifiers::SHIFT) => {
                            app.conversation.scroll_up(3);
                        }
                        (KeyCode::Down, KeyModifiers::SHIFT) => {
                            app.conversation.scroll_down(3);
                        }
                        (KeyCode::Up, KeyModifiers::CONTROL) => {
                            app.history_recall_up();
                        }
                        (KeyCode::Down, KeyModifiers::CONTROL) => {
                            app.history_recall_down();
                        }
                        (KeyCode::PageUp, _) => {
                            app.conversation.scroll_up(20);
                        }
                        (KeyCode::PageDown, _) => {
                            app.conversation.scroll_down(20);
                        }
                        (KeyCode::Up, _) => {
                            if let Some(ref mut picker) = app.at_picker {
                                picker.move_up();
                            } else if app.editor.line_count() > 1 && app.editor.cursor_row() > 0 {
                                app.editor.move_up();
                            } else if app.should_use_arrow_history_recall() {
                                app.history_recall_up();
                            }
                        }
                        (KeyCode::Down, _) => {
                            if let Some(ref mut picker) = app.at_picker {
                                picker.move_down();
                            } else if app.editor.line_count() > 1
                                && app.editor.cursor_row() < app.editor.line_count() - 1
                            {
                                app.editor.move_down();
                            } else if app.should_use_arrow_history_recall() {
                                app.history_recall_down();
                            }
                        }
                        _ => {}
                    }
                } // Event::Key
                _ => {} // Other events (resize, etc.)
            } // match event::read()
        } // if has_terminal_event

        // Agent events already drained before draw (above).

        // Drain queued prompts only after authoritative AgentEnd (but not if quitting)
        if !app.agent_active && !app.should_quit && !app.queued_prompts.is_empty() {
            let (text, attachments) = app.queued_prompts.pop_front().unwrap();
            if attachments.is_empty() {
                app.conversation.push_user(&text);
            } else {
                app.conversation
                    .push_user_with_attachments(&text, &attachments);
            }
            app.history.push(text.clone());
            app.history_idx = None;
            app.agent_active = true;
            if let Ok(mut ss) = app.dashboard_handles.session.lock() {
                ss.busy = true;
            }
            if attachments.is_empty() {
                let _ = command_tx
                    .send(TuiCommand::SubmitPrompt(PromptSubmission {
                        text,
                        image_paths: Vec::new(),
                        submitted_by: "local-tui".to_string(),
                        via: "tui",
                        queue_mode: app.queue_mode,
                    }))
                    .await;
            } else {
                let _ = command_tx
                    .send(TuiCommand::SubmitPrompt(PromptSubmission {
                        text,
                        image_paths: attachments,
                        submitted_by: "local-tui".to_string(),
                        via: "tui",
                        queue_mode: app.queue_mode,
                    }))
                    .await;
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Stop non-persist background services
    crate::tools::serve::cleanup_session_services();

    // Save history before restoring terminal
    app.save_history();

    // Restore terminal
    io::stdout().execute(crossterm::event::DisableBracketedPaste)?;
    io::stdout().execute(DisableMouseCapture)?;
    if app.keyboard_enhancement {
        io::stdout().execute(PopKeyboardEnhancementFlags)?;
    }
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

#[cfg(test)]
mod auspex_copy_tests {
    use super::*;

    #[test]
    fn command_copy_marks_auspex_primary_and_dash_compatibility() {
        let dash = App::COMMANDS
            .iter()
            .find(|(name, _, _)| *name == "dash")
            .expect("/dash command must exist");
        assert!(dash.1.contains("compatibility"));
        assert!(dash.1.contains("legacy/debug"));
        assert!(dash.1.contains("Auspex"));

        let auspex = App::COMMANDS
            .iter()
            .find(|(name, _, _)| *name == "auspex")
            .expect("/auspex command must exist");
        assert!(auspex.1.contains("primary"));
        assert!(auspex.1.contains("Auspex"));
        assert!(auspex.1.contains("open"));
    }
}
