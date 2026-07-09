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
pub mod command_surfaces;
pub mod conv_widget;
pub mod conversation;
pub mod conversation_render_projection;
pub mod dashboard;
pub mod dashboard_projection;
pub mod editor;
pub mod effects;
pub mod extension_overlays;
pub mod footer;
pub mod footer_projection;
pub mod glyphs;
pub mod horizontal_line;
pub mod image;
pub mod inline_render;
pub mod instruments;
pub mod layout_projection;
pub(crate) mod menu_surface;
pub mod model_catalog;
pub mod permission_lane;
pub mod segment_components;
pub mod segment_detail;
pub mod segments;
pub mod selector;
pub(crate) mod settings_menu;
pub mod spinner;
pub mod splash;
pub mod statusline;
pub mod tab_bar;
pub mod theme;
pub mod tool_inspection;
pub mod tutorial;
pub mod widget_renderer;
pub mod widgets;
pub mod workbench;

#[cfg(test)]
mod snapshot_tests;

fn slash_command_for_palette_notification(message: &str) -> Option<&'static str> {
    const PALETTE_NOTIFICATION_COMMANDS: &[(&str, &str)] = &[
        ("## Thinking levels\n", "/think status"),
        ("## Skills\n", "/skills"),
        ("## Prompt library\n", "/prompt list"),
    ];

    PALETTE_NOTIFICATION_COMMANDS
        .iter()
        .find_map(|(prefix, command)| message.starts_with(prefix).then_some(*command))
}

fn should_toast_slash_response(response: &str) -> bool {
    let trimmed = response.trim();
    !trimmed.is_empty()
        && trimmed.lines().count() <= 1
        && trimmed.chars().count() <= 120
        && !trimmed.starts_with("Usage:")
        && !trimmed.starts_with("Unknown")
}

fn should_modal_slash_response(response: &str) -> bool {
    let trimmed = response.trim_start();
    trimmed.starts_with("Usage:")
        || trimmed.starts_with("Ambiguous command")
        || trimmed.starts_with("Unknown ")
        || trimmed.contains(" failed")
        || trimmed.contains("Failed ")
        || trimmed.lines().count() > 20
}

fn is_one_shot_context_notification(message: &str) -> bool {
    matches!(
        message.trim(),
        "Context cleared. Starting fresh conversation."
            | "Nothing eligible to compact yet — compaction only summarizes older turns after the decay window."
    )
}

#[cfg(test)]
mod tests;

use segments::SegmentMeta;
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
use omegon_traits::{AgentEvent, PermissionPersistence, PermissionRequestKind};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use self::conversation::{ConversationView, Tab};
use self::dashboard::DashboardState;
use self::editor::Editor;
use self::footer::{FooterData, SessionUsageSlice};
use self::instruments::InstrumentPanel;
use self::layout_projection::{TuiLayoutInputs, plan_tui_layout};
use self::menu_surface::{ActiveMenu, MenuMode};
use self::permission_lane::{format_permission_prompt, permission_response_for_key};
use self::segments::{SegmentContent, SegmentExportMode, SegmentRenderMode};
use self::settings_menu::SelectorKind;
use self::workbench::{
    PlanDisplaySnapshot, SlimPlanContext, SlimPlanHintState, SlimTurnState, WorkbenchState,
    WorkbenchWorkspaceContext, active_plan_workspace_context_height, active_workbench_snapshot,
    activity_preferred_height, render_activity_panel, render_workbench_panel,
    slim_completed_plan_hint_available, slim_operator_hint, upstream_retry_hint,
    workbench_preferred_height,
};
use crate::surfaces::command::{
    CommandPanel, CommandPanelReturnTarget, CommandPrompt, CommandPromptAction, CommandSeverity,
    CommandToast,
};
use crate::surfaces::layout::UiSurfaces;
use crate::surfaces::operations::OperationMilestoneProjection;
use crate::ui_runtime::actions::{
    AttachComposerPathAction, ComposerCursorDirection, ComposerCursorUnit, ComposerEditOperation,
    ConversationSegmentRef, CopyConversationSegmentAction, CopyLatestAssistantResponseAction,
    EditComposerAction, InsertComposerTextAction, MoveComposerCursorAction,
    OpenConversationSegmentDetailAction, OperatorWaitAction, PermissionAction, PromptSource,
    ReplaceComposerDraftAction, SegmentCopyMode, SelectConversationSegmentAction,
    SetSurfaceVisibleAction, SetUiPresetAction, SlashCommandAction, SubmitPromptAction, UiAction,
    UiActionOutcome, UiSurfaceToggle,
};

struct PendingPermissionContext {
    tool_name: String,
    target: String,
    kind: PermissionRequestKind,
    persistence: PermissionPersistence,
    grant_path: Option<String>,
}

/// Get current process RSS in megabytes (platform-specific).
/// Uses getrusage(2) on macOS and /proc on Linux — no subprocess spawn.
fn get_rss_mb() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        // getrusage(RUSAGE_SELF) returns ru_maxrss in BYTES on macOS
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 {
            Some(usage.ru_maxrss as f64 / (1024.0 * 1024.0))
        } else {
            None
        }
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb / 1024.0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoicePromptMetadata {
    pub event_id: String,
    pub duration_s: Option<f64>,
    pub radio_cue: Option<String>,
    pub end_of_turn: Option<bool>,
    pub close_session_requested: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PromptMetadata {
    pub voice: Option<VoicePromptMetadata>,
}

#[derive(Debug, Clone)]
pub struct PromptSubmission {
    pub text: String,
    pub image_paths: Vec<std::path::PathBuf>,
    pub submitted_by: String,
    pub via: &'static str,
    pub queue_mode: PromptQueueMode,
    pub metadata: PromptMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptQueueMode {
    InterruptAfterTurn,
    #[default]
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
    /// Request cancellation of the active runtime turn.
    CancelActiveTurn {
        submitted_by: String,
        via: &'static str,
    },
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
    /// Switch model intent to a provider-neutral capability grade.
    SetModelGrade {
        grade: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Switch provider/endpoint selection intent.
    SetModelProvider {
        provider: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Switch model grade policy intent.
    SetModelPolicy {
        policy: String,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
    },
    /// Clear exact model override and resume grade/provider intent routing.
    ModelUnpin {
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
    /// Update the session plan stored in the runtime conversation state.
    UpdatePlan {
        command: CanonicalSlashCommand,
        respond_to: Option<tokio::sync::oneshot::Sender<omegon_traits::ControlOutputResponse>>,
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
    /// Voice transcription submitted by a process-local voice extension.
    VoicePrompt {
        text: String,
        metadata: VoicePromptMetadata,
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

struct OperatorEvent {
    message: String,
    color: Color,
    icon: &'static str,
    expires_at: std::time::Instant,
}

fn segment_meta_from_prompt_metadata(metadata: &PromptMetadata) -> SegmentMeta {
    let mut meta = SegmentMeta::default();
    if let Some(voice) = &metadata.voice {
        meta.source_channel = Some("voice".to_string());
        meta.radio_cue = voice.radio_cue.clone();
        meta.voice_end_of_turn = voice.end_of_turn;
        meta.voice_close_session_requested = voice.close_session_requested;
        meta.voice_duration_s = voice.duration_s;
    }
    meta
}

pub(crate) fn voice_prompt_from_notification(
    notification: &crate::extensions::ExtensionNotification,
) -> Option<TuiCommand> {
    if notification.method != "voice/transcription" {
        return None;
    }
    let text = notification.params.get("text")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let event_id = notification
        .params
        .get("utterance_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{}:{}", notification.extension_name, notification.method));
    let metadata = VoicePromptMetadata {
        event_id,
        duration_s: notification
            .params
            .get("duration_s")
            .and_then(serde_json::Value::as_f64),
        radio_cue: notification
            .params
            .get("radio_cue")
            .and_then(serde_json::Value::as_str)
            .filter(|cue| !cue.trim().is_empty())
            .map(ToString::to_string),
        end_of_turn: notification
            .params
            .get("end_of_turn")
            .and_then(serde_json::Value::as_bool),
        close_session_requested: notification
            .params
            .get("close_session_requested")
            .and_then(serde_json::Value::as_bool),
    };
    Some(TuiCommand::VoicePrompt {
        text: text.to_string(),
        metadata,
    })
}

/// Application state for the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivityToolState {
    segment_id: String,
    name: String,
    args_summary: Option<String>,
    result_summary: Option<String>,
    mode: crate::surfaces::activity::ActivityToolMode,
    status: crate::surfaces::activity::ActivityToolStatus,
    expires_at: Option<std::time::Instant>,
}

impl ActivityToolState {
    fn projection(&self) -> crate::surfaces::activity::ActivityToolProjection {
        crate::surfaces::activity::ActivityToolProjection {
            segment_id: self.segment_id.clone(),
            mode: self.mode,
            status: self.status,
            name: self.name.clone(),
            args_summary: self.args_summary.clone(),
            result_summary: self.result_summary.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolInspectionTarget {
    LiveLatest(String),
    Pinned(String),
}

impl ToolInspectionTarget {
    fn id(&self) -> &str {
        match self {
            Self::LiveLatest(id) | Self::Pinned(id) => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyTextModal {
    title: String,
    text: String,
    scroll_y: u16,
    wrap: bool,
}

impl CopyTextModal {
    fn new(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
            scroll_y: 0,
            wrap: true,
        }
    }

    fn scroll_up(&mut self, rows: u16) {
        self.scroll_y = self.scroll_y.saturating_sub(rows);
    }

    fn scroll_down(&mut self, rows: u16) {
        self.scroll_y = self.scroll_y.saturating_add(rows);
    }

    fn scroll_top(&mut self) {
        self.scroll_y = 0;
    }

    fn scroll_bottom(&mut self) {
        self.scroll_y = u16::MAX;
    }
}

struct App {
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
    /// Draft captured before entering history recall, restored after walking back to newest.
    history_draft: Option<String>,
    pending_history_preload: Option<String>,
    dashboard: DashboardState,
    /// Last on-screen dashboard area for mouse hit-testing.
    dashboard_area: Option<Rect>,
    /// Last on-screen conversation area for mouse hit-testing.
    conversation_area: Option<Rect>,
    /// Last on-screen editor area for mouse hit-testing.
    editor_area: Option<Rect>,
    /// Last on-screen workbench area for mouse hit-testing.
    workbench_area: Option<Rect>,
    footer_data: FooterData,
    /// CIC instrument panel for telemetry visualization
    instrument_panel: InstrumentPanel,
    // ui_mode removed — all behavior driven by ui_surfaces
    ui_surfaces: UiSurfaces,
    theme: Box<dyn theme::Theme>,
    /// Whether durable completed-plan history exists for /plan view recall.
    completed_plan_history_available: bool,
    /// Shared settings — source of truth for model, thinking, etc.
    settings: crate::settings::SharedSettings,
    /// Shared cancel token — Escape/Ctrl+C cancels the active agent turn.
    cancel: SharedCancel,
    /// Timestamp of last Ctrl+C (for double-tap quit detection).
    last_ctrl_c: Option<std::time::Instant>,
    /// True after an operator interrupt until the active turn reports AgentEnd.
    /// While set, editor input is suppressed so terminal protocol fragments
    /// emitted by Ctrl+C/Esc cannot leak into the composer.
    interrupt_pending: bool,
    /// Short post-interrupt grace window for dropping raw keyboard protocol
    /// fragments that may arrive after the logical Ctrl+C/Esc event.
    suppress_editor_input_until: Option<std::time::Instant>,
    /// Session start time for /stats.
    session_start: std::time::Instant,
    /// Active command output panel for slash commands and extension UI output.
    command_panel: Option<CommandPanel>,
    /// Active blocking command prompt for responder-backed operator decisions.
    command_prompt: Option<CommandPrompt>,
    /// Active selector popup (model picker, think level, etc.)
    selector: Option<selector::Selector>,
    /// What the selector is for — determines what happens on confirm.
    selector_kind: Option<SelectorKind>,
    /// Active structured menu popup for command inventories such as /skills.
    active_menu: Option<ActiveMenu>,
    /// Last provider route state observed from route-change events.
    route_state: Option<String>,
    /// Last selected model observed from route-change events.
    route_selected_model: Option<String>,
    /// Last serving model observed from route-change events.
    route_serving_model: Option<String>,
    /// Last safe secret readiness snapshot available for the /secrets inventory menu.
    secret_readiness: Option<crate::capabilities::secrets::SecretReadinessSnapshot>,
    /// Pending confirmation action id for menu actions that require a second activation.
    pending_menu_confirmation: Option<String>,
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
    /// Augment registry — manages active persona, tone, and memory layers.
    augment_registry: Option<crate::plugins::registry::AugmentRegistry>,
    /// Slim-mode session row — bottom telemetry below composer/workbench.
    session_row: statusline::SessionRow,
    /// Structured session plan snapshot for the active Workbench panel.
    workbench_state: WorkbenchState,
    tool_inspection_target: Option<ToolInspectionTarget>,
    activity_tools: std::collections::VecDeque<ActivityToolState>,
    /// Explicit Slim turn state rendered in the session row.
    slim_turn_state: SlimTurnState,
    /// Visual effects manager (tachyonfx).
    effects: effects::Effects,
    /// Command definitions from bus features.
    bus_commands: Vec<omegon_traits::CommandDefinition>,
    /// Current restart-substrate generation shown by runtime restart preview.
    runtime_generation: u64,
    /// Copyable inventory of startup/runtime substrate side channels.
    runtime_inventory: crate::setup::RuntimeSubstrateInventory,
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
    /// Local default queue policy for interactive submissions.
    queue_mode: PromptQueueMode,
    /// Inline operator-facing transient events (replaces floating toasts).
    operator_events: std::collections::VecDeque<OperatorEvent>,
    /// Previous harness status for diffing on HarnessStatusChanged.
    previous_harness_status: Option<crate::status::HarnessStatus>,
    /// Receiver for in-process operator-visible smoke-test events.
    smoke_event_rx: Option<std::sync::mpsc::Receiver<AgentEvent>>,
    /// Startup capability tier detected at startup by systems check probes.
    pub capability_grade: Option<crate::startup::CapabilityTier>,
    /// Tutorial state — active when running /tutorial (lesson-based).
    tutorial: Option<TutorialState>,
    /// Tutorial overlay — game-style first-play advisor.
    /// Renders on top of the UI and guides the operator through steps.
    tutorial_overlay: Option<tutorial::Tutorial>,
    /// Pending permission prompt — waiting for user to press y/a/n.
    pending_permission: Option<
        std::sync::Arc<
            std::sync::Mutex<Option<std::sync::mpsc::Sender<omegon_traits::PermissionResponse>>>,
        >,
    >,
    /// Human-readable context for the pending permission prompt.
    pending_permission_context: Option<PendingPermissionContext>,
    /// Pending manual-action wait prompt — waiting for operator confirmation.
    pending_operator_wait: Option<
        std::sync::Arc<
            std::sync::Mutex<Option<std::sync::mpsc::Sender<omegon_traits::OperatorWaitResponse>>>,
        >,
    >,
    /// Human-readable context for the pending manual-action wait prompt.
    pending_operator_wait_context: Option<String>,
    /// Update checker — receives notification when a newer version is available.
    update_rx: Option<crate::update::UpdateReceiver>,
    /// Update checker sender — allows re-checking when channel changes.
    update_tx: Option<crate::update::UpdateSender>,
    /// When true, the agent's last response looked like it's awaiting
    /// confirmation. An empty Enter will send a continuation prompt.
    awaiting_continuation: bool,
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
    /// Voice notification receivers owned by this TUI process/session.
    voice_notification_receivers:
        Vec<tokio::sync::mpsc::UnboundedReceiver<crate::extensions::ExtensionNotification>>,
    /// First-class selectable plaintext copy surface.
    copy_text_modal: Option<CopyTextModal>,
    /// Last rendered copy-all button area for mouse hit-testing when app mouse is enabled.
    copy_text_copy_button_area: Option<Rect>,
    /// Active ephemeral modal from extension widget (widget_id, data, auto_dismiss_ms, spawn_time).
    active_modal: Option<(String, serde_json::Value, Option<u64>, std::time::Instant)>,
    /// Active action prompt from extension widget (widget_id, actions).
    active_action_prompt: Option<(String, Vec<String>)>,
    /// Whether the Anthropic subscription ToS notice has been shown this session.
    /// Shown once on first interactive session with an OAuth-only credential.
    oauth_tos_notice_shown: bool,
    /// Authoritative runtime prompt queue snapshot emitted by the coordinator.
    runtime_queue_snapshot: Option<serde_json::Value>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCreateScope {
    Project,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalSlashCommand {
    ModelView,
    ModelList,
    SetModel(String),
    SetModelGrade(String),
    SetModelProvider(String),
    SetModelPolicy(String),
    ModelUnpin,
    ThinkingView,
    SetThinking(crate::settings::ThinkingLevel),
    ProfileView,
    ProfileExport,
    ProfileCapture(crate::settings::ProfileSaveTarget),
    ProfileApply,
    ProfileSetMqtt(Option<bool>),
    ProfileExtensionAllow(String),
    ProfileExtensionDeny(String),
    ProfileExtensionClear,
    ProfileSetPersona(Option<String>),
    ProfileSetTone(Option<String>),
    AutomationView,
    AutomationSet(crate::settings::AutomationLevel),
    PermissionsView,
    PermissionTrustAdd(String),
    PermissionTrustRemove(String),
    StatusView,
    RuntimeSubstrateRefresh,
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
    TreeView {
        args: String,
    },
    NoteAdd {
        text: String,
    },
    NotesView,
    NotesClear,
    CheckinView,
    ContextStatus,
    ContextCompact,
    ContextClear,
    ContextRequest {
        kind: String,
        query: String,
    },
    ContextRequestJson(String),
    SetContextClass(crate::settings::ContextClass),
    NewSession,
    ListSessions,
    ResumeSession(String),
    AuthView,
    AuthStatus,
    AuthUnlock,
    AuthLogin(String),
    AuthLogout(String),
    SkillsView,
    SkillsHelp,
    SkillsReload,
    SkillsInstall(Option<String>),
    SkillCreate(Option<SkillCreateScope>),
    SkillImport {
        path: String,
        scope: Option<SkillCreateScope>,
    },
    SkillGet(String),
    SkillDelete(String),
    PlanView,
    PlanList,
    PlanShow(String),
    PlanSwitch(String),
    PlanResume(String),
    PlanBackground(Option<String>),
    PlanDetach(Option<String>),
    PlanPromote(Option<String>),
    PlanBind(String),
    PlanLedger(Option<String>),
    PlanSet(Vec<String>),
    PlanApprove,
    PlanExecute,
    PlanAdvance,
    PlanSkip,
    PlanClear,
    ExtensionView,
    ExtensionGet(String),
    ExtensionInstall(String),
    ExtensionRemove(String),
    ExtensionUpdate(Option<String>),
    ExtensionEnable(String),
    ExtensionDisable(String),
    ExtensionSearch(Option<String>),
    ArmoryBrowse(Option<String>),
    ArmoryInstall(String),
    PersonaList,
    CatalogView,
    CatalogInstall,
    CatalogRemove(String),
    PluginView,
    PluginInstall(String),
    PluginRemove(String),
    PluginUpdate(Option<String>),
    SecretsView,
    SecretsSet {
        name: String,
        value: String,
    },
    SecretsGet(String),
    SecretsDelete(String),
    VariablesView,
    VariablesSet {
        name: String,
        value: String,
    },
    VariablesGet(String),
    VariablesDelete(String),
    VaultStatus,
    VaultConfigure,
    VaultInitPolicy,
    CleaveStatus,
    CleaveCancelChild(String),
    DelegateStatus,
    Smoke(crate::smoke_surface::SmokeCommand),
}

pub(crate) fn canonical_slash_command(cmd: &str, args: &str) -> Option<CanonicalSlashCommand> {
    let args = args.trim();
    match cmd {
        "model" if args.is_empty() || args == "route" => None,
        "model" if matches!(args, "list" | "providers" | "status" | "view") => {
            Some(CanonicalSlashCommand::ModelList)
        }
        "model" if args == "unpin" => Some(CanonicalSlashCommand::ModelUnpin),
        "model" if args.starts_with("policy ") => {
            let policy = args.trim_start_matches("policy ").trim();
            if policy.is_empty() {
                None
            } else {
                Some(CanonicalSlashCommand::SetModelPolicy(policy.to_string()))
            }
        }
        "model" if args.starts_with("provider ") => {
            let provider = args.trim_start_matches("provider ").trim();
            if provider.is_empty() {
                None
            } else {
                Some(CanonicalSlashCommand::SetModelProvider(
                    provider.to_string(),
                ))
            }
        }
        "model" if args.starts_with("grade ") => {
            let grade = args.trim_start_matches("grade ").trim();
            if matches!(grade, "F" | "D" | "C" | "B" | "A" | "S") {
                Some(CanonicalSlashCommand::SetModelGrade(grade.to_string()))
            } else {
                None
            }
        }
        "model" if !args.is_empty() => Some(CanonicalSlashCommand::SetModel(args.to_string())),
        "think" if args == "list" || args == "status" => Some(CanonicalSlashCommand::ThinkingView),
        "think" => {
            crate::settings::ThinkingLevel::parse(args).map(CanonicalSlashCommand::SetThinking)
        }
        "profile" if args.is_empty() => None,
        "profile" if args == "status" || args == "view" => Some(CanonicalSlashCommand::ProfileView),
        "profile" if args == "export" => Some(CanonicalSlashCommand::ProfileExport),
        "profile"
            if matches!(
                args,
                "capture" | "save" | "capture --active" | "save --active"
            ) =>
        {
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::ActiveSource,
            ))
        }
        "profile" if matches!(args, "capture --project" | "save --project") => Some(
            CanonicalSlashCommand::ProfileCapture(crate::settings::ProfileSaveTarget::Project),
        ),
        "profile"
            if matches!(
                args,
                "capture --user" | "save --user" | "capture --global" | "save --global"
            ) =>
        {
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::User,
            ))
        }
        "profile" if args == "apply" || args == "load" => Some(CanonicalSlashCommand::ProfileApply),
        "profile" if args == "mqtt" || args == "mqtt status" => {
            Some(CanonicalSlashCommand::ProfileSetMqtt(None))
        }
        "profile" if args == "mqtt on" || args == "mqtt enable" => {
            Some(CanonicalSlashCommand::ProfileSetMqtt(Some(true)))
        }
        "profile" if args == "mqtt off" || args == "mqtt disable" => {
            Some(CanonicalSlashCommand::ProfileSetMqtt(Some(false)))
        }
        "profile" if args == "extensions clear" || args == "extension clear" => {
            Some(CanonicalSlashCommand::ProfileExtensionClear)
        }
        "profile" => {
            if let Some(name) = args
                .strip_prefix("extension allow ")
                .or_else(|| args.strip_prefix("extensions allow "))
                .or_else(|| args.strip_prefix("extension enable "))
                .or_else(|| args.strip_prefix("extensions enable "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::ProfileExtensionAllow(
                    name.to_string(),
                ))
            } else if let Some(name) = args
                .strip_prefix("extension deny ")
                .or_else(|| args.strip_prefix("extensions deny "))
                .or_else(|| args.strip_prefix("extension disable "))
                .or_else(|| args.strip_prefix("extensions disable "))
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(CanonicalSlashCommand::ProfileExtensionDeny(
                    name.to_string(),
                ))
            } else if let Some(name) = args.strip_prefix("persona ").map(str::trim) {
                Some(CanonicalSlashCommand::ProfileSetPersona(
                    (!name.is_empty() && name != "off" && name != "clear")
                        .then(|| name.to_string()),
                ))
            } else {
                args.strip_prefix("tone ").map(str::trim).map(|name| {
                    CanonicalSlashCommand::ProfileSetTone(
                        (!name.is_empty() && name != "off" && name != "clear")
                            .then(|| name.to_string()),
                    )
                })
            }
        }
        "automation" | "autonomy" if args.is_empty() || args == "status" || args == "view" => {
            Some(CanonicalSlashCommand::AutomationView)
        }
        "automation" | "autonomy" => {
            crate::settings::AutomationLevel::parse(args).map(CanonicalSlashCommand::AutomationSet)
        }
        "permissions" | "permission"
            if args.is_empty() || args == "status" || args == "list" || args == "keys" =>
        {
            Some(CanonicalSlashCommand::PermissionsView)
        }
        "permissions" | "permission" | "trust" => {
            let normalized = args
                .strip_prefix("trusted ")
                .or_else(|| args.strip_prefix("trust "))
                .unwrap_or(args)
                .trim();
            if let Some(path) = normalized
                .strip_prefix("add ")
                .or_else(|| normalized.strip_prefix("allow "))
                .map(str::trim)
                .filter(|path| !path.is_empty())
            {
                Some(CanonicalSlashCommand::PermissionTrustAdd(path.to_string()))
            } else if let Some(path) = normalized
                .strip_prefix("remove ")
                .or_else(|| normalized.strip_prefix("rm "))
                .or_else(|| normalized.strip_prefix("revoke "))
                .or_else(|| normalized.strip_prefix("deny "))
                .map(str::trim)
                .filter(|path| !path.is_empty())
            {
                Some(CanonicalSlashCommand::PermissionTrustRemove(
                    path.to_string(),
                ))
            } else if normalized.is_empty() || normalized == "list" || normalized == "status" {
                Some(CanonicalSlashCommand::PermissionsView)
            } else {
                None
            }
        }
        "status" if args.is_empty() => Some(CanonicalSlashCommand::StatusView),
        "runtime" if matches!(args, "restart" | "hot-restart" | "refresh" | "reload") => {
            Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
        }
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
        "context" if args.is_empty() => None,
        "context" => {
            let (sub, rest) = args.split_once(' ').unwrap_or((args, ""));
            match sub {
                "status" => Some(CanonicalSlashCommand::ContextStatus),
                "compact" | "compress" => Some(CanonicalSlashCommand::ContextCompact),
                "clear" | "reset" | "new" => Some(CanonicalSlashCommand::ContextClear),
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
        "new" if args.is_empty() => Some(CanonicalSlashCommand::ContextClear),
        "sessions" if args.is_empty() => None,
        "sessions" if matches!(args, "list" | "all") => Some(CanonicalSlashCommand::ListSessions),
        "resume" if !args.is_empty() => {
            Some(CanonicalSlashCommand::ResumeSession(args.to_string()))
        }
        "sessions" if args.starts_with("resume ") => {
            let id = args.trim_start_matches("resume ").trim();
            (!id.is_empty()).then(|| CanonicalSlashCommand::ResumeSession(id.to_string()))
        }
        "auth" => match args {
            "" => Some(CanonicalSlashCommand::AuthView),
            "status" | "list" => Some(CanonicalSlashCommand::AuthStatus),
            "unlock" => Some(CanonicalSlashCommand::AuthUnlock),
            _ if args.starts_with("login ") => {
                let provider = args.trim_start_matches("login ").trim();
                (!provider.is_empty())
                    .then(|| CanonicalSlashCommand::AuthLogin(provider.to_string()))
            }
            _ if args.starts_with("logout ") => {
                let provider = args.trim_start_matches("logout ").trim();
                (!provider.is_empty())
                    .then(|| CanonicalSlashCommand::AuthLogout(provider.to_string()))
            }
            _ => None,
        },
        "login" if !args.is_empty() => Some(CanonicalSlashCommand::AuthLogin(args.to_string())),
        "logout" if !args.is_empty() => Some(CanonicalSlashCommand::AuthLogout(args.to_string())),
        "skills" | "skill" => {
            if args.is_empty() || args == "list" {
                Some(CanonicalSlashCommand::SkillsView)
            } else if matches!(args, "--help" | "help" | "-h") {
                Some(CanonicalSlashCommand::SkillsHelp)
            } else if matches!(args, "reload" | "refresh") {
                Some(CanonicalSlashCommand::SkillsReload)
            } else if args == "install" {
                Some(CanonicalSlashCommand::SkillsInstall(None))
            } else if let Some(name) = args.strip_prefix("install ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::SkillsInstall(Some(name.to_string())))
            } else if args == "create" || args == "new" {
                Some(CanonicalSlashCommand::SkillCreate(None))
            } else if args == "create --project" || args == "new --project" {
                Some(CanonicalSlashCommand::SkillCreate(Some(
                    SkillCreateScope::Project,
                )))
            } else if args == "create --user" || args == "new --user" {
                Some(CanonicalSlashCommand::SkillCreate(Some(
                    SkillCreateScope::User,
                )))
            } else if let Some(path) = args.strip_prefix("import --project ") {
                let path = path.trim();
                (!path.is_empty()).then(|| CanonicalSlashCommand::SkillImport {
                    path: path.to_string(),
                    scope: Some(SkillCreateScope::Project),
                })
            } else if let Some(path) = args.strip_prefix("import --user ") {
                let path = path.trim();
                (!path.is_empty()).then(|| CanonicalSlashCommand::SkillImport {
                    path: path.to_string(),
                    scope: Some(SkillCreateScope::User),
                })
            } else if let Some(path) = args.strip_prefix("import ") {
                let path = path.trim();
                (!path.is_empty()).then(|| CanonicalSlashCommand::SkillImport {
                    path: path.to_string(),
                    scope: None,
                })
            } else if let Some(name) = args.strip_prefix("get ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::SkillGet(name.to_string()))
            } else if let Some(name) = args.strip_prefix("delete ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::SkillDelete(name.to_string()))
            } else {
                None
            }
        }
        "plan" => {
            if args.is_empty() || args == "status" {
                Some(CanonicalSlashCommand::PlanView)
            } else if args == "list" {
                Some(CanonicalSlashCommand::PlanList)
            } else if let Some(id) = args.strip_prefix("show ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::PlanShow(id.to_string()))
            } else if let Some(id) = args.strip_prefix("switch ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::PlanSwitch(id.to_string()))
            } else if let Some(id) = args.strip_prefix("resume ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::PlanResume(id.to_string()))
            } else if args == "background" {
                Some(CanonicalSlashCommand::PlanBackground(None))
            } else if let Some(id) = args.strip_prefix("background ") {
                let id = id.trim();
                Some(CanonicalSlashCommand::PlanBackground(
                    (!id.is_empty()).then(|| id.to_string()),
                ))
            } else if args == "detach" {
                Some(CanonicalSlashCommand::PlanDetach(None))
            } else if let Some(id) = args.strip_prefix("detach ") {
                let id = id.trim();
                Some(CanonicalSlashCommand::PlanDetach(
                    (!id.is_empty()).then(|| id.to_string()),
                ))
            } else if args == "promote" {
                Some(CanonicalSlashCommand::PlanPromote(None))
            } else if let Some(target) = args.strip_prefix("promote ") {
                let target = target.trim();
                Some(CanonicalSlashCommand::PlanPromote(
                    (!target.is_empty()).then(|| target.to_string()),
                ))
            } else if let Some(binding) = args.strip_prefix("bind ") {
                let binding = binding.trim();
                (!binding.is_empty()).then(|| CanonicalSlashCommand::PlanBind(binding.to_string()))
            } else if args == "ledger" {
                Some(CanonicalSlashCommand::PlanLedger(None))
            } else if let Some(id) = args.strip_prefix("ledger ") {
                let id = id.trim();
                Some(CanonicalSlashCommand::PlanLedger(
                    (!id.is_empty()).then(|| id.to_string()),
                ))
            } else if let Some(raw_items) = args.strip_prefix("set ") {
                let items = split_plan_items(raw_items);
                (!items.is_empty()).then_some(CanonicalSlashCommand::PlanSet(items))
            } else if args == "approve" {
                Some(CanonicalSlashCommand::PlanApprove)
            } else if args == "execute" || args == "exec" {
                Some(CanonicalSlashCommand::PlanExecute)
            } else if args == "advance" || args == "next" {
                Some(CanonicalSlashCommand::PlanAdvance)
            } else if args == "skip" {
                Some(CanonicalSlashCommand::PlanSkip)
            } else if args == "clear" || args == "off" {
                Some(CanonicalSlashCommand::PlanClear)
            } else {
                None
            }
        }
        "extension" | "ext" => {
            if matches!(args, "" | "list" | "view") {
                Some(CanonicalSlashCommand::ExtensionView)
            } else if let Some(name) = args.strip_prefix("get ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionGet(name.to_string()))
            } else if let Some(uri) = args.strip_prefix("install ") {
                let uri = uri.trim();
                (!uri.is_empty()).then(|| CanonicalSlashCommand::ExtensionInstall(uri.to_string()))
            } else if let Some(name) = args.strip_prefix("remove ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionRemove(name.to_string()))
            } else if matches!(args, "refresh" | "reload" | "restart") {
                Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
            } else if args == "update" {
                Some(CanonicalSlashCommand::ExtensionUpdate(None))
            } else if let Some(name) = args.strip_prefix("update ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::ExtensionUpdate(Some(name.to_string())))
            } else if let Some(name) = args.strip_prefix("enable ") {
                let name = name.trim();
                (!name.is_empty()).then(|| CanonicalSlashCommand::ExtensionEnable(name.to_string()))
            } else if let Some(name) = args.strip_prefix("disable ") {
                let name = name.trim();
                (!name.is_empty())
                    .then(|| CanonicalSlashCommand::ExtensionDisable(name.to_string()))
            } else if args == "search" {
                Some(CanonicalSlashCommand::ExtensionSearch(None))
            } else if let Some(query) = args.strip_prefix("search ") {
                let query = query.trim();
                Some(CanonicalSlashCommand::ExtensionSearch(
                    if query.is_empty() {
                        None
                    } else {
                        Some(query.to_string())
                    },
                ))
            } else {
                None
            }
        }
        "persona" => {
            if args == "list" {
                Some(CanonicalSlashCommand::PersonaList)
            } else {
                None // "off" and <name> are handled directly in TUI handler
            }
        }
        "armory" => {
            if args.is_empty() || args == "browse" || args == "search" || args == "list" {
                Some(CanonicalSlashCommand::ArmoryBrowse(None))
            } else if let Some(query) = args.strip_prefix("browse ") {
                let query = query.trim();
                Some(CanonicalSlashCommand::ArmoryBrowse(if query.is_empty() {
                    None
                } else {
                    Some(query.to_string())
                }))
            } else if let Some(query) = args.strip_prefix("search ") {
                let query = query.trim();
                Some(CanonicalSlashCommand::ArmoryBrowse(if query.is_empty() {
                    None
                } else {
                    Some(query.to_string())
                }))
            } else if let Some(target) = args.strip_prefix("install ") {
                let target = target.trim();
                (!target.is_empty())
                    .then(|| CanonicalSlashCommand::ArmoryInstall(target.to_string()))
            } else if args == "install" {
                None
            } else {
                Some(CanonicalSlashCommand::ArmoryBrowse(Some(args.to_string())))
            }
        }
        "catalog" => {
            if args.is_empty() || args == "list" {
                Some(CanonicalSlashCommand::CatalogView)
            } else if args == "install" {
                Some(CanonicalSlashCommand::CatalogInstall)
            } else if let Some(id) = args.strip_prefix("remove ") {
                let id = id.trim();
                (!id.is_empty()).then(|| CanonicalSlashCommand::CatalogRemove(id.to_string()))
            } else {
                None
            }
        }
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

        "variables" | "vars" => {
            let parts: Vec<&str> = args.splitn(3, ' ').collect();
            match parts.first().copied().unwrap_or("") {
                "" | "list" | "status" => Some(CanonicalSlashCommand::VariablesView),
                "set" if parts.len() >= 3 && !parts[1].trim().is_empty() => {
                    Some(CanonicalSlashCommand::VariablesSet {
                        name: parts[1].trim().to_string(),
                        value: parts[2].trim().to_string(),
                    })
                }
                "get" if parts.len() >= 2 && !parts[1].trim().is_empty() => Some(
                    CanonicalSlashCommand::VariablesGet(parts[1].trim().to_string()),
                ),
                "delete" | "remove" | "rm" if parts.len() >= 2 && !parts[1].trim().is_empty() => {
                    Some(CanonicalSlashCommand::VariablesDelete(
                        parts[1].trim().to_string(),
                    ))
                }
                _ => None,
            }
        }
        "secrets" => {
            let parts: Vec<&str> = args.splitn(3, ' ').collect();
            match parts.first().copied().unwrap_or("") {
                "" | "list" | "status" => Some(CanonicalSlashCommand::SecretsView),
                "set" if parts.len() >= 3 && !parts[1].trim().is_empty() => {
                    let value = parts[2].trim();
                    (value.starts_with("env:")
                        || value.starts_with("cmd:")
                        || value.starts_with("vault:"))
                    .then(|| CanonicalSlashCommand::SecretsSet {
                        name: parts[1].trim().to_string(),
                        value: value.to_string(),
                    })
                }
                "get" if parts.len() >= 2 && !parts[1].trim().is_empty() => Some(
                    CanonicalSlashCommand::SecretsGet(parts[1].trim().to_string()),
                ),
                "delete" | "remove" | "rm" if parts.len() >= 2 && !parts[1].trim().is_empty() => {
                    Some(CanonicalSlashCommand::SecretsDelete(
                        parts[1].trim().to_string(),
                    ))
                }
                _ => None,
            }
        }
        "vault" => match args {
            "" | "status" => Some(CanonicalSlashCommand::VaultStatus),
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
        "delegate" | "subagent" => match args {
            "" | "status" => Some(CanonicalSlashCommand::DelegateStatus),
            _ => None,
        },
        "smoke" => {
            crate::smoke_surface::parse_smoke_command(args).map(CanonicalSlashCommand::Smoke)
        }
        _ => None,
    }
}

fn split_plan_items(raw: &str) -> Vec<String> {
    raw.split('|')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
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
    http_transport_security: omegon_traits::OmegonTransportSecurity,
    ws_transport_security: omegon_traits::OmegonTransportSecurity,
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
    let (http_transport_security, ws_transport_security) = startup_transport_security(startup);
    let payload = AuspexAttachPayload {
        version: 1,
        transport: "omegon-ipc".into(),
        preferred_handoff,
        startup_url: startup.startup_url.clone(),
        http_base: startup.http_base.clone(),
        ws_url: startup.ws_url.clone(),
        ws_token: startup.token.clone(),
        http_transport_security,
        ws_transport_security,
        instance: startup.instance_descriptor.clone(),
    };
    serde_json::to_string(&payload).map_err(Into::into)
}

fn startup_transport_security(
    startup: &crate::web::WebStartupInfo,
) -> (
    omegon_traits::OmegonTransportSecurity,
    omegon_traits::OmegonTransportSecurity,
) {
    let http = startup
        .instance_descriptor
        .as_ref()
        .and_then(|instance| instance.control_plane.http_transport_security.clone())
        .unwrap_or_else(|| {
            if startup.http_base.starts_with("https://") {
                omegon_traits::OmegonTransportSecurity::Secure
            } else {
                omegon_traits::OmegonTransportSecurity::InsecureBootstrap
            }
        });
    let ws = startup
        .instance_descriptor
        .as_ref()
        .and_then(|instance| instance.control_plane.ws_transport_security.clone())
        .unwrap_or_else(|| {
            if startup.ws_url.starts_with("wss://") {
                omegon_traits::OmegonTransportSecurity::Secure
            } else {
                omegon_traits::OmegonTransportSecurity::InsecureBootstrap
            }
        });
    (http, ws)
}

fn format_transport_security(value: &omegon_traits::OmegonTransportSecurity) -> &'static str {
    match value {
        omegon_traits::OmegonTransportSecurity::LocalIpc => "local-ipc",
        omegon_traits::OmegonTransportSecurity::InsecureBootstrap => "insecure-bootstrap",
        omegon_traits::OmegonTransportSecurity::Secure => "secure",
        omegon_traits::OmegonTransportSecurity::IdentityMesh => "identity-mesh",
    }
}

fn dash_browser_url(
    startup: Option<&crate::web::WebStartupInfo>,
    addr: Option<std::net::SocketAddr>,
) -> Option<String> {
    startup
        .map(|startup| startup.http_base.clone())
        .or_else(|| addr.map(|addr| format!("http://{addr}")))
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

fn runtime_queue_depth(snapshot: Option<&serde_json::Value>) -> usize {
    snapshot
        .and_then(|snapshot| snapshot.get("depth"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

fn render_runtime_queue_info_line(
    area: Rect,
    frame: &mut Frame<'_>,
    theme: &dyn crate::tui::theme::Theme,
    snapshot: Option<&serde_json::Value>,
) {
    if area.height == 0 {
        return;
    }
    let Some(snapshot) = snapshot else {
        return;
    };
    let depth = runtime_queue_depth(Some(snapshot));
    if depth == 0 {
        return;
    }
    let preview = snapshot
        .get("items")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("preview"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let line = Line::from(vec![
        Span::styled(" Runtime queue ", theme.style_dim()),
        Span::styled(
            format!("[{depth}]"),
            Style::default()
                .fg(theme.accent_bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", theme.style_dim()),
        Span::styled(preview.to_string(), theme.style_accent()),
    ]);
    let widget = Paragraph::new(line).style(Style::default().bg(theme.surface_bg()));
    frame.render_widget(widget, area);
}

fn editor_height_for(editor: &Editor, main_area: Rect) -> u16 {
    let content_width = main_area.width.saturating_sub(2).max(1);
    let editor_rows = editor.visual_line_count(content_width) as u16;
    let max_editor = (main_area.height * 40 / 100).clamp(5, 20);
    (editor_rows + 2).clamp(3, max_editor) // +2 for border
}

fn settings_profile_source_line(source: &crate::settings::ProfileSource) -> String {
    match source {
        crate::settings::ProfileSource::Project(path) => {
            format!("profile: project · file: {}", path.display())
        }
        crate::settings::ProfileSource::User(path) => {
            format!("profile: user · file: {}", path.display())
        }
        crate::settings::ProfileSource::BuiltInDefault => "profile: built-in defaults".to_string(),
    }
}

fn workbench_repo_display_name(cwd: &std::path::Path) -> Option<String> {
    let repo = git2::Repository::discover(cwd).ok()?;
    git_remote_repo_name(&repo)
}

fn workbench_git_branch(cwd: &std::path::Path) -> Option<String> {
    let repo = git2::Repository::discover(cwd).ok()?;
    workbench_git_branch_for_repo(&repo)
}

fn workbench_git_branch_for_repo(repo: &git2::Repository) -> Option<String> {
    let head = repo.head().ok()?;
    let mut label = if head.is_branch() {
        head.shorthand()
            .filter(|branch| !branch.is_empty())?
            .to_string()
    } else {
        let short = head
            .target()
            .map(|oid| oid.to_string().chars().take(7).collect::<String>())?;
        format!("HEAD@{short}")
    };

    if let Some((ahead, behind)) = git_ahead_behind(repo, &head) {
        if ahead > 0 {
            label.push_str(&format!(" ↑{ahead}"));
        }
        if behind > 0 {
            label.push_str(&format!(" ↓{behind}"));
        }
    }

    if git_has_tracked_changes(repo) {
        label.push_str(" *");
    }

    if let Some(state) = git_state_label(repo.state()) {
        label.push_str(" · ");
        label.push_str(state);
    }

    Some(label)
}

fn git_ahead_behind(repo: &git2::Repository, head: &git2::Reference<'_>) -> Option<(usize, usize)> {
    let branch_name = head.shorthand()?;
    let local_oid = head.target()?;
    let upstream = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .ok()?
        .upstream()
        .ok()?
        .get()
        .target()?;
    repo.graph_ahead_behind(local_oid, upstream).ok()
}

fn git_has_tracked_changes(repo: &git2::Repository) -> bool {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    repo.statuses(Some(&mut opts))
        .map(|statuses| {
            statuses
                .iter()
                .any(|entry| entry.status() != git2::Status::CURRENT)
        })
        .unwrap_or(false)
}

fn git_state_label(state: git2::RepositoryState) -> Option<&'static str> {
    match state {
        git2::RepositoryState::Clean => None,
        git2::RepositoryState::Merge => Some("merge"),
        git2::RepositoryState::Revert | git2::RepositoryState::RevertSequence => Some("revert"),
        git2::RepositoryState::CherryPick | git2::RepositoryState::CherryPickSequence => {
            Some("cherry-pick")
        }
        git2::RepositoryState::Bisect => Some("bisect"),
        git2::RepositoryState::Rebase
        | git2::RepositoryState::RebaseInteractive
        | git2::RepositoryState::RebaseMerge => Some("rebase"),
        git2::RepositoryState::ApplyMailbox | git2::RepositoryState::ApplyMailboxOrRebase => {
            Some("apply")
        }
    }
}

fn git_remote_repo_name(repo: &git2::Repository) -> Option<String> {
    let remote = repo
        .find_remote("upstream")
        .or_else(|_| repo.find_remote("origin"))
        .ok()?;
    remote.url().and_then(repo_name_from_git_remote_url)
}

fn repo_name_from_git_remote_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let tail = without_git
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(without_git)
        .trim();

    if tail.is_empty() {
        None
    } else {
        Some(tail.to_string())
    }
}

fn workspace_dir_basename(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod workspace_context_tests {
    use super::*;

    #[test]
    fn repo_name_from_remote_url_handles_common_forms() {
        assert_eq!(
            repo_name_from_git_remote_url("git@github.com:styrene-labs/omegon.git"),
            Some("omegon".to_string())
        );
        assert_eq!(
            repo_name_from_git_remote_url("https://github.com/styrene-labs/omegon.git"),
            Some("omegon".to_string())
        );
        assert_eq!(
            repo_name_from_git_remote_url("ssh://git@github.com/styrene-labs/omegon"),
            Some("omegon".to_string())
        );
        assert_eq!(repo_name_from_git_remote_url(""), None);
    }

    #[test]
    fn git_remote_repo_name_prefers_upstream_over_origin() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        repo.remote("origin", "git@github.com:fork/local-checkout-name.git")
            .unwrap();
        repo.remote("upstream", "git@github.com:styrene-labs/canonical-name.git")
            .unwrap();

        assert_eq!(
            git_remote_repo_name(&repo),
            Some("canonical-name".to_string())
        );
    }

    #[test]
    fn workbench_repo_display_name_uses_remote_not_checkout_dir() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = dir.path().join("local-checkout-name");
        std::fs::create_dir(&checkout).unwrap();
        let repo = git2::Repository::init(&checkout).unwrap();
        repo.remote("origin", "git@github.com:styrene-labs/canonical-name.git")
            .unwrap();

        assert_eq!(
            workbench_repo_display_name(&checkout),
            Some("canonical-name".to_string())
        );
        assert_eq!(workspace_dir_basename(&checkout), "local-checkout-name");
    }

    #[test]
    fn workbench_git_branch_includes_ahead_behind_and_dirty_markers() {
        let dir = tempfile::tempdir().unwrap();
        let mut init_opts = git2::RepositoryInitOptions::new();
        init_opts.initial_head("main");
        let repo = git2::Repository::init_opts(dir.path(), &init_opts).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Omegon Test").unwrap();
            config
                .set_str("user.email", "omegon@example.invalid")
                .unwrap();
        }
        std::fs::write(dir.path().join("file.txt"), "base\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("file.txt")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let base = repo
            .commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();
        let base_commit = repo.find_commit(base).unwrap();
        repo.branch("upstream", &base_commit, false).unwrap();
        repo.find_branch("main", git2::BranchType::Local)
            .unwrap()
            .set_upstream(Some("upstream"))
            .unwrap();

        std::fs::write(dir.path().join("file.txt"), "next\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("file.txt")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "next", &tree, &[&base_commit])
            .unwrap();
        std::fs::write(dir.path().join("file.txt"), "dirty\n").unwrap();

        assert_eq!(
            workbench_git_branch_for_repo(&repo).as_deref(),
            Some("main ↑1 *")
        );
    }

    #[test]
    fn workbench_git_branch_reports_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "Omegon Test").unwrap();
            config
                .set_str("user.email", "omegon@example.invalid")
                .unwrap();
        }
        std::fs::write(dir.path().join("file.txt"), "base\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("file.txt")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let commit_id = repo
            .commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap();
        repo.set_head_detached(commit_id).unwrap();

        let label = workbench_git_branch_for_repo(&repo).unwrap();
        assert!(label.starts_with("HEAD@"), "{label}");
    }
}

impl App {
    fn displayed_model_grade(model_provider: &str, model_id: &str, fallback: &str) -> String {
        let model = model_id
            .strip_prefix(&format!("{model_provider}:"))
            .unwrap_or(model_id);

        let registry = crate::model_registry::ModelRegistry::global();
        registry
            .exact_grade(model_provider, model)
            .or_else(|| registry.infer_grade(model_provider, model))
            .map(str::to_string)
            .unwrap_or_else(|| fallback.to_string())
    }

    fn context_class_tag(class: crate::settings::ContextClass) -> &'static str {
        match class {
            crate::settings::ContextClass::Compact => "cmp",
            crate::settings::ContextClass::Standard => "std",
            crate::settings::ContextClass::Extended => "ext",
            crate::settings::ContextClass::Massive => "msv",
        }
    }

    fn context_fill_bar(percent: f32) -> String {
        let percent = percent.clamp(0.0, 100.0);
        let filled = ((percent / 100.0) * 8.0).round().clamp(0.0, 8.0) as usize;
        format!("▕{}{}▏", "█".repeat(filled), "░".repeat(8 - filled))
    }

    fn editor_context_widget(
        actual: crate::settings::ContextClass,
        context_window: usize,
        _estimated_tokens: usize,
        context_percent: f32,
    ) -> String {
        let class = Self::context_class_tag(actual);
        let capacity = if context_window > 0 {
            widgets::format_tokens(context_window)
        } else {
            widgets::format_tokens(actual.nominal_tokens())
        };
        let percent = context_percent.clamp(0.0, 100.0).round() as u8;

        let bar = Self::context_fill_bar(context_percent);
        format!("ctx:{class}@{capacity} {bar} {percent}%")
    }

    fn render_engine_status_row(&self, area: Rect, frame: &mut Frame, t: &dyn theme::Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let bg = t.card_bg();
        let verb = if self.agent_active {
            spinner::maybe_glitch(self.working_verb)
                .unwrap_or_else(|| self.working_verb.to_string())
        } else {
            "ready".to_string()
        };
        let mut spans = vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled("⟳ ", Style::default().fg(t.accent_bright()).bg(bg)),
            Span::styled(
                verb,
                Style::default()
                    .fg(t.accent_muted())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if self.agent_active {
            spans.push(Span::styled(
                " · active turn",
                Style::default().fg(t.dim()).bg(bg),
            ));
        } else {
            spans.push(Span::styled(" · idle", Style::default().fg(t.dim()).bg(bg)));
        }
        Paragraph::new(Line::from(spans))
            .style(Style::default().bg(bg))
            .render(area, frame.buffer_mut());
    }

    fn current_persona_state(&self) -> crate::settings::PersonaState {
        let persona_id = self
            .augment_registry
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
                .augment_registry
                .as_ref()
                .and_then(|r| r.active_persona().map(|p| p.id.clone())),
            branch: None,      // populated lazily if needed
            duration_ms: None, // set on completion
            source_channel: None,
            radio_cue: None,
            voice_end_of_turn: None,
            voice_close_session_requested: None,
            voice_duration_s: None,
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
                let (http_security, ws_security) = startup_transport_security(startup);
                let warning_suffix = if startup.daemon_status.transport_warnings.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n  transport warnings: {}",
                        startup.daemon_status.transport_warnings.join(" | ")
                    )
                };
                format!(
                    "running at {}\n  startup: {}\n  websocket: {}\n  transport: http={}, ws={}\n  queued events: {}\n  processed events: {}\n  worker: {}{}",
                    startup.http_base,
                    startup.startup_url,
                    startup.ws_url,
                    format_transport_security(&http_security),
                    format_transport_security(&ws_security),
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
            history_draft: None,
            pending_history_preload: None,
            dashboard: DashboardState::default(),
            dashboard_area: None,
            conversation_area: None,
            editor_area: None,
            workbench_area: None,
            footer_data: FooterData {
                model_id,
                model_provider,
                ..Default::default()
            },
            instrument_panel: InstrumentPanel::default(),
            // ui_mode removed — surfaces drive everything
            ui_surfaces: UiSurfaces::lean(),
            theme: theme::default_theme(),
            settings,
            cancel: std::sync::Arc::new(std::sync::Mutex::new(None)),
            last_ctrl_c: None,
            interrupt_pending: false,
            suppress_editor_input_until: None,
            session_start: std::time::Instant::now(),
            command_panel: None,
            command_prompt: None,
            selector: None,
            selector_kind: None,
            active_menu: None,
            route_state: None,
            route_selected_model: None,
            route_serving_model: None,
            secret_readiness: None,
            pending_menu_confirmation: None,
            at_picker: None,
            last_tool_name: None,
            completed_tool_name: None,
            working_verb: "Working",
            replay_splash: false,
            augment_registry: Some(crate::plugins::registry::AugmentRegistry::new(
                crate::prompt::load_lex_imperialis(),
            )),
            session_row: statusline::SessionRow::default(),
            workbench_state: WorkbenchState::default(),
            completed_plan_history_available: false,
            tool_inspection_target: None,
            activity_tools: std::collections::VecDeque::new(),
            slim_turn_state: SlimTurnState::Ready,
            effects: effects::Effects::new(),
            bus_commands: Vec::new(),
            runtime_generation: 1,
            runtime_inventory: crate::setup::RuntimeSubstrateInventory::default(),
            dashboard_handles: dashboard::DashboardHandles::default(),
            last_instrument_update: std::time::Instant::now(),
            cleave_tokens_accounted_in: 0,
            cleave_tokens_accounted_out: 0,
            dashboard_refresh_turn: u32::MAX, // force refresh on first frame
            web_startup: None,
            web_server_addr: None,
            queue_mode: PromptQueueMode::UntilReady,
            operator_events: std::collections::VecDeque::new(),
            previous_harness_status: None,
            smoke_event_rx: None,
            capability_grade: None,
            tutorial: None,
            tutorial_overlay: None,
            pending_permission: None,
            pending_permission_context: None,
            pending_operator_wait: None,
            pending_operator_wait_context: None,
            update_rx: None,
            update_tx: None,
            awaiting_continuation: false,
            login_prompt_tx: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            keyboard_enhancement: false,
            mouse_capture_enabled: false,
            terminal_copy_mode: false,
            last_left_click: None,
            extension_widgets: std::collections::HashMap::new(),
            widget_receivers: Vec::new(),
            voice_notification_receivers: Vec::new(),
            copy_text_modal: None,
            copy_text_copy_button_area: None,
            active_modal: None,
            active_action_prompt: None,
            oauth_tos_notice_shown: false,
            runtime_queue_snapshot: None,
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
        self.set_mouse_capture(true);
    }

    fn apply_ui_preset(&mut self, surfaces: UiSurfaces) {
        self.ui_surfaces = surfaces;
        if surfaces.is_compact() {}
    }

    fn toggle_ui_surface(&mut self, surface: UiSurfaceToggle, enabled: bool) {
        match surface {
            UiSurfaceToggle::Dashboard => self.ui_surfaces.dashboard = enabled,
            UiSurfaceToggle::Instruments => self.ui_surfaces.instruments = enabled,
            UiSurfaceToggle::Footer => self.ui_surfaces.footer = enabled,
            UiSurfaceToggle::Activity => self.ui_surfaces.activity = enabled,
        }
    }

    /// Check if the agent's last text output looks like it's asking for
    /// confirmation/continuation (e.g., "Shall I proceed?", "Would you like
    /// me to...?"). Updates placeholder text and `awaiting_continuation`.
    fn detect_continuation_request(&mut self) {
        let last_text = self
            .conversation
            .segments()
            .iter()
            .rev()
            .find_map(|seg| {
                if let segments::SegmentContent::AssistantText { text, .. } = &seg.content {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("");

        // Check the last ~200 chars for confirmation-seeking patterns.
        // Assistant text can contain emoji, so never slice by byte offset.
        let tail = Self::tail_chars(last_text, 200);
        let lower = tail.to_ascii_lowercase();
        let seeking = lower.contains("shall i")
            || lower.contains("should i")
            || lower.contains("would you like")
            || lower.contains("do you want me to")
            || lower.contains("ready to proceed")
            || lower.contains("want me to proceed")
            || lower.contains("want me to continue")
            || lower.contains("go ahead?")
            || lower.contains("let me know")
            || lower.ends_with('?')
                && (lower.contains("proceed")
                    || lower.contains("continue")
                    || lower.contains("implement"));

        self.awaiting_continuation = seeking;
        if seeking {
            self.editor
                .textarea
                .set_placeholder_text("Press Enter to continue, or type a new instruction");
        } else {
            self.editor
                .textarea
                .set_placeholder_text("Ask anything, or type / for commands");
        }
    }

    fn ui_status_text(&self) -> String {
        let mode = self.ui_surfaces.preset_name();
        format!(
            "UI preset: {mode}\n  dashboard: {}\n  instruments: {}\n  footer: {}\n  activity: {}\n\nPresets\n  /ui lean    (conversation + activity)\n  /ui full    (+ dashboard + instruments)\n\nSurfaces\n  /ui show|hide|toggle dashboard|instruments|footer|activity",
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
            if self.ui_surfaces.activity {
                "on"
            } else {
                "off"
            },
        )
    }

    fn set_terminal_copy_mode(&mut self, enabled: bool) {
        let changed = self.terminal_copy_mode != enabled;
        self.terminal_copy_mode = enabled;
        self.set_mouse_capture(!enabled);
        if !changed {
            return;
        }
        if enabled {
            self.show_toast(
                "Mouse passthrough — terminal selection owns drag; Ctrl+Shift+T restores app mouse",
                ratatui_toaster::ToastType::Info,
            );
        } else {
            self.show_toast(
                "App mouse restored — wheel/click panes; Ctrl+Shift+Y copies latest answer",
                ratatui_toaster::ToastType::Info,
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
                // Format: "Provider: Model Name — description (context, capabilities)"
                let context = model.context_str();
                let caps = if model.capabilities.is_empty() {
                    String::new()
                } else {
                    format!(", {}", model.capability_str())
                };
                let label = format!("{}: {}", provider_name, model.name);
                let description = format!("{} — {}{}", model.description, context, caps);

                options.push(selector::SelectOption {
                    value: model.id.clone(),
                    label,
                    description,
                    active: model.id == current,
                });
            }
        }

        if options.is_empty() {
            self.show_command_toast(CommandToast::new(
                "Model catalog is empty — use /model list for available options",
                CommandSeverity::Warning,
            ));
            return;
        }

        // Sort by provider, then by name for consistency
        options.sort_by(|a, b| a.label.cmp(&b.label));

        self.selector = Some(selector::Selector::new("Select Model", options));
        self.selector_kind = Some(SelectorKind::Model);
    }

    fn open_model_grade_selector(&mut self) {
        let current = self
            .active_menu
            .as_ref()
            .and_then(|menu| {
                menu.projection
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.groups.iter())
                    .flat_map(|group| group.rows.iter())
                    .find(|row| row.id == "model.grade")
            })
            .and_then(|row| row.value.as_deref())
            .unwrap_or("B");
        self.selector = Some(selector::Selector::new(
            "Select Model Grade",
            settings_menu::model_grade_selector_options(current),
        ));
        self.selector_kind = Some(SelectorKind::ModelGrade);
    }

    fn open_model_provider_selector(&mut self) {
        let current = self
            .active_menu
            .as_ref()
            .and_then(|menu| {
                menu.projection
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.groups.iter())
                    .flat_map(|group| group.rows.iter())
                    .find(|row| row.id == "model.provider")
            })
            .and_then(|row| row.value.as_deref())
            .unwrap_or("auto");
        self.selector = Some(selector::Selector::new(
            "Select Provider Intent",
            settings_menu::model_provider_selector_options(current),
        ));
        self.selector_kind = Some(SelectorKind::ModelProvider);
    }

    fn open_model_policy_selector(&mut self) {
        let current = self
            .active_menu
            .as_ref()
            .and_then(|menu| {
                menu.projection
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.groups.iter())
                    .flat_map(|group| group.rows.iter())
                    .find(|row| row.id == "model.policy")
            })
            .and_then(|row| row.value.as_deref())
            .unwrap_or("minimum");
        self.selector = Some(selector::Selector::new(
            "Select Routing Policy",
            settings_menu::model_policy_selector_options(current),
        ));
        self.selector_kind = Some(SelectorKind::ModelPolicy);
    }

    fn open_thinking_selector(&mut self) {
        let current = self.settings().thinking;
        let options = settings_menu::thinking_selector_options(current);
        self.selector = Some(selector::Selector::new(
            settings_menu::THINKING_DESCRIPTOR.label,
            options,
        ));
        self.selector_kind = Some(SelectorKind::ThinkingLevel);
    }

    fn open_context_selector(&mut self) {
        let current = self.settings().context_class;
        let options = settings_menu::context_class_selector_options(current);
        self.selector = Some(selector::Selector::new(
            settings_menu::CONTEXT_DESCRIPTOR.label,
            options,
        ));
        self.selector_kind = Some(SelectorKind::ContextClass);
    }

    fn open_persona_selector(&mut self) {
        let (personas, _) = crate::plugins::persona_loader::scan_available();
        if personas.is_empty() {
            self.show_toast(
                "No personas installed — install with omegon plugin install <git-url>",
                ratatui_toaster::ToastType::Warning,
            );
            return;
        }

        let active_id = self
            .augment_registry
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
            self.show_toast(
                "No tones installed — install with omegon plugin install <git-url>",
                ratatui_toaster::ToastType::Warning,
            );
            return;
        }

        let active_id = self
            .augment_registry
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

    fn current_workbench_workspace_context(&self) -> WorkbenchWorkspaceContext {
        let cwd = self.cwd();
        let dir = workspace_dir_basename(cwd);
        let repo = workbench_repo_display_name(cwd).or_else(|| {
            crate::setup::find_project_root(cwd)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });
        WorkbenchWorkspaceContext {
            repo,
            dir,
            git_branch: workbench_git_branch(cwd)
                .or_else(|| self.footer_data.harness.git_branch.clone()),
        }
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
            return "Authentication failed. Use /auth login <provider> to re-authenticate.";
        }
        if lower.contains("status 403")
            || lower.contains("http 403")
            || lower.contains("error 403")
            || lower.contains("forbidden")
            || lower.contains("permission denied")
        {
            return "Permission denied. Check file permissions or API access scope.";
        }
        if lower.contains("supported source types")
            || (tool_name == Some("validate") && lower.contains("unsupported"))
        {
            return "Validation skipped one or more paths. Check the rejected path in the tool output, then run a project-specific test or validator for that file type.";
        }
        // Timeout
        if lower.contains("timeout") || lower.contains("timed out") {
            if tool_name == Some(crate::tool_registry::web_search::WEB_SEARCH) {
                return "Web search timed out. Retrying will try the free search engines concurrently; API search keys are more reliable.";
            }
            return "Operation timed out. Retry, or set a larger timeout when the tool supports it.";
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

    fn settings_projection(&self) -> crate::surfaces::settings::SettingsSurfaceProjection {
        let settings = self.settings();
        crate::surfaces::settings::SettingsSurfaceProjection::from_settings_with_profile(
            &settings,
            self.cwd(),
        )
    }

    fn open_menu_projection(&mut self, projection: crate::surfaces::menu::MenuProjection) {
        self.active_menu = Some(ActiveMenu::new(projection));
        self.pending_menu_confirmation = None;
        self.command_panel = None;
        self.command_prompt = None;
    }

    fn open_settings_menu(&mut self) {
        self.open_menu_projection(self.settings_menu_projection());
    }

    fn ui_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let surfaces = self.ui_surfaces;
        let mut menu = MenuProjection::new("ui", "UI");
        menu.summary = Some(format!(
            "TUI surface controls. Preset: {}; dashboard: {}; instruments: {}; footer: {}; activity: {}.",
            surfaces.preset_name(),
            if surfaces.dashboard { "on" } else { "off" },
            if surfaces.instruments { "on" } else { "off" },
            if surfaces.footer { "on" } else { "off" },
            if surfaces.activity { "on" } else { "off" },
        ));
        menu.footer = Some("↑/↓ navigate · / filter · Enter run · l lean · f full · Esc close · /ui status for text readout".into());
        menu.actions = vec![
            {
                let mut action =
                    MenuActionProjection::command("ui.global.lean", "Lean", "/ui lean");
                action.key = Some("l".into());
                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                action
            },
            {
                let mut action =
                    MenuActionProjection::command("ui.global.full", "Full", "/ui full");
                action.key = Some("f".into());
                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                action
            },
        ];
        let surface_row = |id: &str, label: &str, enabled: bool, command: &str| MenuRowProjection {
            id: format!("ui.surface.{id}"),
            label: label.into(),
            description: format!("Toggle the {label} surface."),
            value: Some(if enabled { "on" } else { "off" }.into()),
            kind: MenuRowKind::Action,
            badges: vec![MenuBadgeProjection {
                label: if enabled { "on".into() } else { "off".into() },
                tone: if enabled {
                    MenuBadgeTone::Success
                } else {
                    MenuBadgeTone::Neutral
                },
            }],
            metadata: vec![command.into()],
            primary_action: Some({
                let mut action = MenuActionProjection::command(
                    format!("ui.surface.{id}.toggle"),
                    "Toggle",
                    command,
                );
                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                action
            }),
            actions: vec![],
            safety: None,
            availability: None,
        };
        menu.tabs = vec![MenuTabProjection {
            id: "ui".into(),
            label: "UI".into(),
            groups: vec![
                MenuGroupProjection {
                    id: "ui.presets".into(),
                    label: "Presets".into(),
                    description: Some("Switch coarse TUI surface presets.".into()),
                    rows: vec![
                        MenuRowProjection {
                            id: "ui.preset.lean".into(),
                            label: "Lean preset".into(),
                            description: "Conversation + activity, no dashboard chrome.".into(),
                            value: Some(
                                if surfaces.preset_name() == "lean" {
                                    "active"
                                } else {
                                    ""
                                }
                                .into(),
                            ),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: if surfaces.preset_name() == "lean" {
                                    "active".into()
                                } else {
                                    "preset".into()
                                },
                                tone: if surfaces.preset_name() == "lean" {
                                    MenuBadgeTone::Success
                                } else {
                                    MenuBadgeTone::Info
                                },
                            }],
                            metadata: vec!["/ui lean".into()],
                            primary_action: Some({
                                let mut action = MenuActionProjection::command(
                                    "ui.preset.lean.primary",
                                    "Lean",
                                    "/ui lean",
                                );
                                action.close_policy =
                                    crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }),
                            actions: vec![{
                                let mut action = MenuActionProjection::command(
                                    "ui.preset.lean.action",
                                    "Lean",
                                    "/ui lean",
                                );
                                action.key = Some("l".into());
                                action.close_policy =
                                    crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "ui.preset.full".into(),
                            label: "Full preset".into(),
                            description: "Dashboard, instruments, footer, and activity.".into(),
                            value: Some(
                                if surfaces.preset_name() == "full" {
                                    "active"
                                } else {
                                    ""
                                }
                                .into(),
                            ),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: if surfaces.preset_name() == "full" {
                                    "active".into()
                                } else {
                                    "preset".into()
                                },
                                tone: if surfaces.preset_name() == "full" {
                                    MenuBadgeTone::Success
                                } else {
                                    MenuBadgeTone::Info
                                },
                            }],
                            metadata: vec!["/ui full".into()],
                            primary_action: Some({
                                let mut action = MenuActionProjection::command(
                                    "ui.preset.full.primary",
                                    "Full",
                                    "/ui full",
                                );
                                action.close_policy =
                                    crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }),
                            actions: vec![{
                                let mut action = MenuActionProjection::command(
                                    "ui.preset.full.action",
                                    "Full",
                                    "/ui full",
                                );
                                action.key = Some("f".into());
                                action.close_policy =
                                    crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                    ],
                },
                MenuGroupProjection {
                    id: "ui.surfaces".into(),
                    label: "Surfaces".into(),
                    description: Some("Toggle individual TUI surfaces.".into()),
                    rows: vec![
                        surface_row(
                            "dashboard",
                            "Dashboard",
                            surfaces.dashboard,
                            "/ui toggle dashboard",
                        ),
                        surface_row(
                            "instruments",
                            "Instruments",
                            surfaces.instruments,
                            "/ui toggle instruments",
                        ),
                        surface_row("footer", "Footer", surfaces.footer, "/ui toggle footer"),
                        surface_row(
                            "activity",
                            "Activity",
                            surfaces.activity,
                            "/ui toggle activity",
                        ),
                        MenuRowProjection {
                            id: "ui.detail".into(),
                            label: "Tool output detail".into(),
                            description: "Adjust tool output density/detail level.".into(),
                            value: Some(self.settings().tool_detail.as_str().into()),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "density".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec!["/ui detail".into(), "/detail".into()],
                            primary_action: Some(MenuActionProjection::command(
                                "ui.detail.primary",
                                "Detail",
                                "/ui detail",
                            )),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                    ],
                },
            ],
        }];
        menu
    }

    fn open_ui_menu(&mut self) {
        self.open_menu_projection(self.ui_menu_projection());
    }

    fn context_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let settings = self.settings();
        let requested = settings
            .requested_context_class
            .map(|class| class.label().to_string())
            .unwrap_or_else(|| "track model".to_string());
        let actual = settings.context_class.label().to_string();
        let mut menu = MenuProjection::new("context", "Context");
        menu.summary = Some(format!(
            "Context policy and working-set controls. Requested: {requested}; model capacity: {actual}."
        ));
        menu.footer = Some(
            "↑/↓ navigate · / filter · Enter run/edit · c compact · n new context · Esc close"
                .into(),
        );
        menu.tabs = vec![MenuTabProjection {
            id: "context".into(),
            label: "Context".into(),
            groups: vec![MenuGroupProjection {
                id: "context.controls".into(),
                label: "Context controls".into(),
                description: Some("Inspect usage, choose context policy, compact, or start fresh.".into()),
                rows: vec![
                    MenuRowProjection {
                        id: "context.class".into(),
                        label: "Context policy".into(),
                        description: "Choose the requested working-set policy class.".into(),
                        value: Some(requested),
                        kind: MenuRowKind::Object,
                        badges: vec![MenuBadgeProjection { label: "policy".into(), tone: MenuBadgeTone::Info }],
                        metadata: vec!["/context <compact|standard|extended|massive>".into()],
                        primary_action: Some(MenuActionProjection::open_selector("context.class.select", "Choose", "context.class")),
                        actions: vec![{ let mut action = MenuActionProjection::open_selector("context.class.choose", "Choose", "context.class"); action.key = Some("p".into()); action }],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "context.status".into(),
                        label: "Status".into(),
                        description: "Show current context usage and available actions.".into(),
                        value: Some(actual),
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "read".into(), tone: MenuBadgeTone::Neutral }],
                        metadata: vec!["/context status".into()],
                        primary_action: Some(MenuActionProjection::command("context.status.primary", "Status", "/context status")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "context.compact".into(),
                        label: "Compact".into(),
                        description: "Request context compaction for the current session.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "mutates".into(), tone: MenuBadgeTone::Warning }],
                        metadata: vec!["/context compact".into()],
                        primary_action: Some(MenuActionProjection::command("context.compact.primary", "Compact", "/context compact")),
                        actions: vec![{ let mut action = MenuActionProjection::command("context.compact.action", "Compact", "/context compact"); action.key = Some("c".into()); action }],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "context.clear".into(),
                        label: "Clear conversation context".into(),
                        description: "Clears the current transcript context and starts fresh. Direct command: /context clear.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "destructive".into(), tone: MenuBadgeTone::Danger }],
                        metadata: vec!["explicit command required: /context clear".into(), "/new".into()],
                        primary_action: None,
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                ],
            }],
        }];
        menu
    }

    fn open_context_menu(&mut self) {
        self.open_menu_projection(self.context_menu_projection());
    }

    fn variables_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };

        let mut menu = MenuProjection::new("variables", "Variables");
        menu.summary = Some("Session-scoped runtime configuration. Values are printable by design; put credentials in /secrets instead.".into());
        menu.footer = Some("↑/↓ navigate · Enter run/prepare action · / filter · Esc close · /variables status for text readout".into());

        let snapshot = crate::control::variables::variables_snapshot();
        let variable_rows = if snapshot.is_empty() {
            vec![MenuRowProjection {
                id: "variables.inventory.empty".into(),
                label: "No session variables set".into(),
                description: "Use Actions to set printable runtime config for this session.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection {
                    label: "empty".into(),
                    tone: MenuBadgeTone::Neutral,
                }],
                metadata: vec!["session scope".into(), "printable".into()],
                primary_action: None,
                actions: Vec::new(),
                safety: None,
                availability: None,
            }]
        } else {
            snapshot
                .into_iter()
                .map(|(name, value)| {
                    let sensitive_hint =
                        crate::control::variables::variable_name_has_sensitive_hint(&name);
                    let mut badges = vec![MenuBadgeProjection {
                        label: "session".into(),
                        tone: MenuBadgeTone::Info,
                    }];
                    if sensitive_hint {
                        badges.push(MenuBadgeProjection {
                            label: "sensitive?".into(),
                            tone: MenuBadgeTone::Warning,
                        });
                    }
                    let mut metadata = vec!["value visible".into(), "scope: session".into()];
                    if sensitive_hint {
                        metadata.push("consider /secrets".into());
                    }
                    MenuRowProjection {
                        id: format!("variables.inventory.{name}"),
                        label: name.clone(),
                        description: if sensitive_hint {
                            "Printable variable name looks sensitive; use /secrets for credentials."
                                .into()
                        } else {
                            "Printable session variable.".into()
                        },
                        value: Some(value),
                        kind: MenuRowKind::Object,
                        badges,
                        metadata,
                        primary_action: Some(MenuActionProjection::command(
                            format!("variables.get.{name}"),
                            "Get",
                            format!("/variables get {name}"),
                        )),
                        actions: vec![
                            MenuActionProjection::prime_editor(
                                format!("variables.set.{name}"),
                                "Update",
                                format!("/variables set {name} "),
                                "Type the replacement printable value for this session variable",
                            ),
                            MenuActionProjection::prime_editor(
                                format!("variables.delete.{name}"),
                                "Delete",
                                format!("/variables delete {name}"),
                                "Press Enter to delete this variable from the session",
                            ),
                        ],
                        safety: None,
                        availability: None,
                    }
                })
                .collect()
        };

        menu.tabs = vec![MenuTabProjection {
            id: "inventory".into(),
            label: "Inventory".into(),
            groups: vec![MenuGroupProjection {
                id: "variables.inventory".into(),
                label: "Session variables".into(),
                description: Some("Printable runtime config currently available to Omegon-managed process launches.".into()),
                rows: variable_rows,
            }],
        }, MenuTabProjection {
            id: "actions".into(),
            label: "Actions".into(),
            groups: vec![MenuGroupProjection {
                id: "variables.actions".into(),
                label: "Variable actions".into(),
                description: Some("Prepare variable commands. Values entered here are not secret and may be displayed.".into()),
                rows: vec![
                    MenuRowProjection {
                        id: "variables.status".into(),
                        label: "List variables".into(),
                        description: "Show printable session variables and values.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "read".into(), tone: MenuBadgeTone::Success }],
                        metadata: vec!["/variables status".into(), "values visible".into()],
                        primary_action: Some(MenuActionProjection::command("variables.status.primary", "List", "/variables status")),
                        actions: Vec::new(),
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "variables.set".into(),
                        label: "Set variable".into(),
                        description: "Prepare /variables set NAME VALUE for printable runtime config.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "printable".into(), tone: MenuBadgeTone::Warning }],
                        metadata: vec!["/variables set NAME VALUE".into(), "not for secrets".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("variables.set.prepare", "Prepare", "/variables set ", "Type NAME VALUE; use /secrets for credentials")),
                        actions: Vec::new(),
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "variables.set.model".into(),
                        label: "Set model hint".into(),
                        description: "Prepare a common model-routing variable without retyping the command shape.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "template".into(), tone: MenuBadgeTone::Info }],
                        metadata: vec!["/variables set OMEGON_MODEL VALUE".into(), "printable".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("variables.set.model.prepare", "Prepare", "/variables set OMEGON_MODEL ", "Type the model hint value")),
                        actions: Vec::new(),
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "variables.set.cwd".into(),
                        label: "Set command cwd".into(),
                        description: "Prepare a common working-directory variable without retyping the command shape.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "template".into(), tone: MenuBadgeTone::Info }],
                        metadata: vec!["/variables set OMEGON_CWD PATH".into(), "printable".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("variables.set.cwd.prepare", "Prepare", "/variables set OMEGON_CWD ", "Type the working directory path")),
                        actions: Vec::new(),
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "variables.get".into(),
                        label: "Get variable".into(),
                        description: "Prepare /variables get NAME.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "read".into(), tone: MenuBadgeTone::Success }],
                        metadata: vec!["/variables get NAME".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("variables.get.prepare", "Prepare", "/variables get ", "Type variable name to print")),
                        actions: Vec::new(),
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "variables.delete".into(),
                        label: "Delete variable".into(),
                        description: "Prepare /variables delete NAME.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "mutates".into(), tone: MenuBadgeTone::Danger }],
                        metadata: vec!["/variables delete NAME".into(), "session only".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("variables.delete.prepare", "Prepare", "/variables delete ", "Type the exact variable name to delete")),
                        actions: Vec::new(),
                        safety: None,
                        availability: None,
                    },
                ],
            }],
        }];
        menu
    }

    fn open_variables_menu(&mut self) {
        self.open_menu_projection(self.variables_menu_projection());
    }

    fn secrets_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let mut menu = MenuProjection::new("secrets", "Secrets");
        menu.summary = Some("Secret configuration surface. Values are never displayed; setting plaintext secrets always uses hidden input.".into());
        menu.footer = Some("↑/↓ navigate · Enter prepare action · / filter · Esc close · /secrets status for text readout".into());
        menu.tabs = vec![MenuTabProjection {
            id: "inventory".into(),
            label: "Inventory".into(),
            groups: vec![MenuGroupProjection {
                id: "secrets.inventory".into(),
                label: "Secret inventory".into(),
                description: Some("Known and declared secret bindings from first-party harness capabilities and extension/agent metadata. Values are never resolved while rendering this menu.".into()),
                rows: self.secret_readiness_rows(),
            }],
        }, MenuTabProjection {
            id: "capabilities".into(),
            label: "Capabilities".into(),
            groups: vec![MenuGroupProjection {
                id: "secrets.capabilities".into(),
                label: "Harness capability readiness".into(),
                description: Some("First-party harness capabilities grouped by the secret bindings that make them available or degraded.".into()),
                rows: self.secret_harness_capability_rows(),
            }],
        }, MenuTabProjection {
            id: "actions".into(),
            label: "Actions".into(),
            groups: vec![MenuGroupProjection {
                id: "secrets.actions".into(),
                label: "Secret actions".into(),
                description: Some("Prepare safe secret commands without exposing values in the menu.".into()),
                rows: vec![
                    MenuRowProjection {
                        id: "secrets.status".into(),
                        label: "List configured secrets".into(),
                        description: "Show configured secret names and recipes; never prints resolved values.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "safe".into(), tone: MenuBadgeTone::Success }],
                        metadata: vec!["/secrets status".into(), "values redacted".into()],
                        primary_action: Some(MenuActionProjection::command("secrets.status.primary", "List", "/secrets status")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "secrets.set".into(),
                        label: "Set hidden secret".into(),
                        description: "Prepare /secrets set NAME; Enter then type the name and use hidden input for the value.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "hidden input".into(), tone: MenuBadgeTone::Warning }],
                        metadata: vec!["/secrets set NAME".into(), "plaintext values are never captured from menu rows".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("secrets.set.prepare", "Prepare", "/secrets set ", "Type secret name, then Enter for hidden input")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "secrets.recipe.env".into(),
                        label: "Configure env recipe".into(),
                        description: "Prepare a recipe-backed secret that resolves from an environment variable.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "recipe".into(), tone: MenuBadgeTone::Info }],
                        metadata: vec!["env:VAR".into(), "value redacted".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("secrets.recipe.env.prepare", "Prepare", "/secrets set ", "Type NAME env:VAR; values stay outside the menu")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "secrets.recipe.cmd".into(),
                        label: "Configure cmd recipe".into(),
                        description: "Prepare a recipe-backed secret that resolves from a command.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "recipe".into(), tone: MenuBadgeTone::Info }],
                        metadata: vec!["cmd:COMMAND".into(), "value redacted".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("secrets.recipe.cmd.prepare", "Prepare", "/secrets set ", "Type NAME cmd:COMMAND; command output is never rendered here")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "secrets.recipe.vault".into(),
                        label: "Configure vault recipe".into(),
                        description: "Prepare a recipe-backed secret that resolves from a vault path.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "recipe".into(), tone: MenuBadgeTone::Info }],
                        metadata: vec!["vault:PATH".into(), "value redacted".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("secrets.recipe.vault.prepare", "Prepare", "/secrets set ", "Type NAME vault:PATH; resolved values stay redacted")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "secrets.get".into(),
                        label: "Check resolution".into(),
                        description: "Prepare /secrets get NAME; checks whether a secret resolves without printing it.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "redacted".into(), tone: MenuBadgeTone::Success }],
                        metadata: vec!["/secrets get NAME".into(), "never prints value".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("secrets.get.prepare", "Prepare", "/secrets get ", "Type the secret name to check resolution; value stays redacted")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "secrets.delete".into(),
                        label: "Clear secret binding".into(),
                        description: "Prepare /secrets delete NAME. This clears the local configured value or recipe binding; declared capability requirements remain visible.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection { label: "clear binding".into(), tone: MenuBadgeTone::Danger }],
                        metadata: vec!["/secrets delete NAME".into(), "requirements remain".into()],
                        primary_action: Some(MenuActionProjection::prime_editor("secrets.delete.prepare", "Prepare", "/secrets delete ", "Type the exact secret name to clear its local binding")),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                ],
            }],
        }];
        menu
    }

    fn secret_harness_capability_rows(&self) -> Vec<crate::surfaces::menu::MenuRowProjection> {
        use crate::capabilities::secrets::HarnessCapabilityReadinessStatus;
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuRowKind,
            MenuRowProjection,
        };

        let Some(snapshot) = self.secret_readiness.as_ref() else {
            return vec![MenuRowProjection {
                id: "secrets.capabilities.unavailable".into(),
                label: "No harness capability readiness snapshot loaded".into(),
                description:
                    "No first-party secret-backed capability readiness is currently available."
                        .into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection {
                    label: "metadata only".into(),
                    tone: MenuBadgeTone::Neutral,
                }],
                metadata: vec!["values never displayed".into()],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            }];
        };

        if snapshot.harness_capabilities.is_empty() {
            return vec![MenuRowProjection {
                id: "secrets.capabilities.empty".into(),
                label: "No first-party secret-backed capabilities discovered".into(),
                description: "The first-party secret catalog did not expose any grouped harness capabilities.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection { label: "empty".into(), tone: MenuBadgeTone::Neutral }],
                metadata: vec!["values never displayed".into()],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            }];
        }

        snapshot
            .harness_capabilities
            .iter()
            .map(|capability| {
                let (status_label, status_tone) = match capability.status {
                    HarnessCapabilityReadinessStatus::Ready => ("ready", MenuBadgeTone::Success),
                    HarnessCapabilityReadinessStatus::Partial => {
                        ("partial", MenuBadgeTone::Warning)
                    }
                    HarnessCapabilityReadinessStatus::Missing => ("missing", MenuBadgeTone::Danger),
                };
                let policy_label = match capability.policy {
                    crate::capabilities::secrets::HarnessCapabilitySecretPolicy::AnyOf => {
                        "any configured provider enables this capability"
                    }
                    crate::capabilities::secrets::HarnessCapabilitySecretPolicy::AllOf => {
                        "all listed secrets are needed"
                    }
                };
                let mut metadata = vec![format!(
                    "{} configured · {} deferred · {} known {}",
                    capability.configured_count,
                    capability.deferred_count,
                    capability.candidate_count,
                    capability.candidate_label
                )];
                metadata.push(format!("policy: {policy_label}"));
                metadata.push(format!("category: {}", capability.category.label()));
                metadata.extend(
                    capability
                        .secret_names
                        .iter()
                        .map(|name| format!("secret: {name}")),
                );
                let primary_secret = capability
                    .preferred_secret
                    .clone()
                    .or_else(|| capability.secret_names.first().cloned())
                    .unwrap_or_default();
                MenuRowProjection {
                    id: format!("secrets.capabilities.{}", capability.id),
                    label: capability.label.clone(),
                    description: capability.description.clone(),
                    value: Some(status_label.into()),
                    kind: MenuRowKind::Object,
                    badges: vec![MenuBadgeProjection {
                        label: status_label.into(),
                        tone: status_tone,
                    }],
                    metadata,
                    primary_action: (!primary_secret.is_empty()).then(|| {
                        MenuActionProjection::prime_editor(
                            format!("secrets.capability.configure.{}", capability.id),
                            "Configure",
                            format!("/secrets set {primary_secret}"),
                            "Replace the suggested secret name if you prefer a different provider",
                        )
                    }),
                    actions: capability
                        .secret_names
                        .iter()
                        .map(|name| {
                            MenuActionProjection::prime_editor(
                                format!("secrets.capability.configure.{}.{}", capability.id, name),
                                format!("Set {name}"),
                                format!("/secrets set {name}"),
                                "Press Enter to capture a value with hidden input",
                            )
                        })
                        .collect(),
                    safety: None,
                    availability: None,
                }
            })
            .collect()
    }

    fn secret_readiness_rows(&self) -> Vec<crate::surfaces::menu::MenuRowProjection> {
        use crate::capabilities::secrets::SecretReadinessStatus;
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuRowKind,
            MenuRowProjection,
        };

        let Some(snapshot) = self.secret_readiness.as_ref() else {
            return vec![MenuRowProjection {
                id: "secrets.inventory.unavailable".into(),
                label: "No secret readiness snapshot loaded".into(),
                description: "No known first-party or declared extension/agent secret bindings are currently available in the TUI; use Actions for safe recipes or /secrets status for configured recipe names.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection { label: "metadata only".into(), tone: MenuBadgeTone::Neutral }],
                metadata: vec!["values never displayed".into(), "provider auth lives under /auth".into()],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            }];
        };

        if snapshot.secrets.is_empty() {
            return vec![MenuRowProjection {
                id: "secrets.inventory.empty".into(),
                label: "No known secret bindings discovered".into(),
                description: "No first-party harness secret catalog entries or declared extension/agent secret requirements were discovered for this session.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection { label: "empty".into(), tone: MenuBadgeTone::Neutral }],
                metadata: vec!["values never displayed".into(), "provider auth lives under /auth".into()],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            }];
        }

        snapshot
            .secrets
            .iter()
            .map(|secret| {
                let (status_label, status_tone) = match secret.status {
                    SecretReadinessStatus::Warmed => ("warmed", MenuBadgeTone::Success),
                    SecretReadinessStatus::Configured => ("configured", MenuBadgeTone::Info),
                    SecretReadinessStatus::Deferred => ("deferred", MenuBadgeTone::Warning),
                    SecretReadinessStatus::Missing => ("missing", MenuBadgeTone::Danger),
                };
                let mut badges = vec![MenuBadgeProjection {
                    label: status_label.into(),
                    tone: status_tone,
                }];
                if secret.required {
                    badges.push(MenuBadgeProjection {
                        label: "required".into(),
                        tone: MenuBadgeTone::Danger,
                    });
                }
                if secret.optional {
                    badges.push(MenuBadgeProjection {
                        label: "optional".into(),
                        tone: MenuBadgeTone::Neutral,
                    });
                }
                let mut metadata = vec!["value redacted".into()];
                if let Some(kind) = secret.recipe_kind.as_deref() {
                    metadata.push(format!("recipe: {kind}"));
                }
                if secret.warmed {
                    metadata.push("session: warmed".into());
                }
                for consumer in &secret.consumers {
                    metadata.push(format!("consumer: {:?}:{}", consumer.kind, consumer.id));
                }
                MenuRowProjection {
                    id: format!("secrets.inventory.{}", secret.name),
                    label: secret.name.clone(),
                    description:
                        "Secret readiness metadata only; value is never resolved or displayed."
                            .into(),
                    value: Some(status_label.into()),
                    kind: MenuRowKind::Object,
                    badges,
                    metadata,
                    primary_action: Some(MenuActionProjection::command(
                        format!("secrets.get.{}", secret.name),
                        "Check",
                        format!("/secrets get {}", secret.name),
                    )),
                    actions: vec![
                        MenuActionProjection::prime_editor(
                            format!("secrets.set.hidden.{}", secret.name),
                            "Replace hidden value",
                            format!("/secrets set {}", secret.name),
                            "Press Enter to capture a replacement value with hidden input",
                        ),
                        MenuActionProjection::prime_editor(
                            format!("secrets.recipe.env.{}", secret.name),
                            "Use env recipe",
                            format!("/secrets set {} env:", secret.name),
                            "Type the environment variable name after env:",
                        ),
                        MenuActionProjection::prime_editor(
                            format!("secrets.recipe.cmd.{}", secret.name),
                            "Use cmd recipe",
                            format!("/secrets set {} cmd:", secret.name),
                            "Type the command after cmd:; resolved output stays redacted",
                        ),
                        MenuActionProjection::prime_editor(
                            format!("secrets.recipe.vault.{}", secret.name),
                            "Use vault recipe",
                            format!("/secrets set {} vault:", secret.name),
                            "Type the vault path after vault:",
                        ),
                        MenuActionProjection::prime_editor(
                            format!("secrets.delete.{}", secret.name),
                            "Clear binding",
                            format!("/secrets delete {}", secret.name),
                            "Clear the configured value or recipe; capability requirement remains visible",
                        ),
                    ],
                    safety: None,
                    availability: None,
                }
            })
            .collect()
    }

    fn open_secrets_menu(&mut self) {
        self.open_menu_projection(self.secrets_menu_projection());
    }

    fn sessions_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };

        let entries = crate::session::list_sessions(self.cwd());
        let mut menu = MenuProjection::new("sessions", "Sessions");
        menu.summary = Some("Saved sessions for this workspace. Enter resumes the selected session; use /sessions list for the text readout.".into());
        menu.footer = Some("↑/↓ navigate · / filter · Enter resume · Esc close".into());

        let rows = if entries.is_empty() {
            vec![MenuRowProjection {
                id: "sessions.empty".into(),
                label: "No saved sessions".into(),
                description: "Sessions are saved when an interactive session exits.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection {
                    label: "empty".into(),
                    tone: MenuBadgeTone::Neutral,
                }],
                metadata: vec!["/sessions list".into()],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            }]
        } else {
            entries
                .into_iter()
                .map(|entry| {
                    let id = entry.meta.session_id.clone();
                    let command = format!("/sessions resume {id}");
                    let description = crate::session::session_display_description(&entry.meta);
                    MenuRowProjection {
                        id: format!("session.{id}"),
                        label: crate::session::session_display_name(&entry.meta),
                        description: format!(
                            "{} · {} · {} turns · {} tool calls",
                            description,
                            entry.meta.created_at,
                            entry.meta.turns,
                            entry.meta.tool_calls
                        ),
                        value: Some(id.clone()),
                        kind: MenuRowKind::Object,
                        badges: vec![MenuBadgeProjection {
                            label: "resume".into(),
                            tone: MenuBadgeTone::Info,
                        }],
                        metadata: vec![
                            format!("id: {id}"),
                            format!(
                                "name: {}",
                                crate::session::session_display_name(&entry.meta)
                            ),
                            command.clone(),
                            entry.path.display().to_string(),
                        ],
                        primary_action: Some(MenuActionProjection::command(
                            format!("session.{id}.resume"),
                            "Resume",
                            command.clone(),
                        )),
                        actions: vec![{
                            let mut action = MenuActionProjection::command(
                                format!("session.{id}.resume.action"),
                                "Resume",
                                command,
                            );
                            action.key = Some("r".into());
                            action
                        }],
                        safety: None,
                        availability: None,
                    }
                })
                .collect()
        };

        menu.tabs = vec![MenuTabProjection {
            id: "saved".into(),
            label: "Saved".into(),
            groups: vec![MenuGroupProjection {
                id: "sessions.saved".into(),
                label: "Saved sessions".into(),
                description: Some("Resume a saved conversation by id.".into()),
                rows,
            }],
        }];
        menu
    }

    fn open_sessions_menu(&mut self) {
        self.open_menu_projection(self.sessions_menu_projection());
    }

    fn memory_status_text(&self) -> String {
        format!(
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
        )
    }

    fn memory_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let memory = &self.footer_data.harness.memory;
        let mut menu = MenuProjection::new("memory", "Memory");
        menu.summary = Some(format!(
            "Memory context overview. Injected: {} facts · working set: {} facts · estimate: ~{} tokens.",
            self.footer_data.injected_facts,
            self.footer_data.working_memory,
            self.footer_data.memory_tokens_est
        ));
        menu.footer = Some("↑/↓ navigate · / filter · Enter status readout · Esc close".into());
        menu.tabs = vec![
            MenuTabProjection {
                id: "overview".into(),
                label: "Overview".into(),
                groups: vec![MenuGroupProjection {
                    id: "memory.facts".into(),
                    label: "Memory facts".into(),
                    description: Some(
                        "Read-only memory counters currently injected into this session.".into(),
                    ),
                    rows: vec![
                        MenuRowProjection {
                            id: "memory.status".into(),
                            label: "Memory overview".into(),
                            description: "Show the full text memory overview.".into(),
                            value: Some(format!("{} total facts", self.footer_data.total_facts)),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "read".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec!["/memory status".into(), "/memory overview".into()],
                            primary_action: Some(MenuActionProjection::command(
                                "memory.status.primary",
                                "Status",
                                "/memory status",
                            )),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.injected".into(),
                            label: "Injected facts".into(),
                            description: "Facts currently injected into the prompt context.".into(),
                            value: Some(self.footer_data.injected_facts.to_string()),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "context".into(),
                                tone: MenuBadgeTone::Info,
                            }],
                            metadata: vec![format!(
                                "estimate: ~{} tokens",
                                self.footer_data.memory_tokens_est
                            )],
                            primary_action: None,
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.working_set".into(),
                            label: "Working-set facts".into(),
                            description: "Facts pinned or selected for active working memory."
                                .into(),
                            value: Some(self.footer_data.working_memory.to_string()),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "working".into(),
                                tone: MenuBadgeTone::Info,
                            }],
                            metadata: vec![],
                            primary_action: None,
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.project".into(),
                            label: "Project facts".into(),
                            description: "Durable project memory facts available to this session."
                                .into(),
                            value: Some(memory.project_facts.to_string()),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "project".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec![],
                            primary_action: None,
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.persona".into(),
                            label: "Persona facts".into(),
                            description: "Persona mind facts available to this session.".into(),
                            value: Some(memory.persona_facts.to_string()),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "persona".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec![format!(
                                "active persona: {}",
                                memory
                                    .active_persona_mind
                                    .clone()
                                    .unwrap_or_else(|| "none".into())
                            )],
                            primary_action: None,
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.episodes".into(),
                            label: "Episodes".into(),
                            description: "Session episode narratives available for recall.".into(),
                            value: Some(memory.episodes.to_string()),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "episodes".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec![],
                            primary_action: None,
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                    ],
                }],
            },
            MenuTabProjection {
                id: "actions".into(),
                label: "Actions".into(),
                groups: vec![MenuGroupProjection {
                    id: "memory.actions".into(),
                    label: "Memory actions".into(),
                    description: Some(
                        "Prepare memory tool commands without hiding the current overview.".into(),
                    ),
                    rows: vec![
                        MenuRowProjection {
                            id: "memory.recall".into(),
                            label: "Recall memory".into(),
                            description: "Prime a memory recall query.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "query".into(),
                                tone: MenuBadgeTone::Info,
                            }],
                            metadata: vec!["/memory recall <query>".into()],
                            primary_action: Some(MenuActionProjection::prime_editor(
                                "memory.recall.primary",
                                "Recall",
                                "/memory recall ",
                                "Type a memory recall query, then press Enter",
                            )),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.list".into(),
                            label: "List memory".into(),
                            description: "List available memory facts.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "read".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec!["/memory list".into()],
                            primary_action: Some(MenuActionProjection::command(
                                "memory.list.primary",
                                "List",
                                "/memory list",
                            )),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.focus".into(),
                            label: "Focus memory".into(),
                            description: "Prime a memory focus command.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "pin".into(),
                                tone: MenuBadgeTone::Info,
                            }],
                            metadata: vec!["/memory focus <topic>".into()],
                            primary_action: Some(MenuActionProjection::prime_editor(
                                "memory.focus.primary",
                                "Focus",
                                "/memory focus ",
                                "Type a memory topic to focus, then press Enter",
                            )),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.release".into(),
                            label: "Release memory".into(),
                            description: "Prime a memory release command.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "unpin".into(),
                                tone: MenuBadgeTone::Warning,
                            }],
                            metadata: vec!["/memory release <topic>".into()],
                            primary_action: Some(MenuActionProjection::prime_editor(
                                "memory.release.primary",
                                "Release",
                                "/memory release ",
                                "Type a memory topic to release, then press Enter",
                            )),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "memory.compact".into(),
                            label: "Compact memory".into(),
                            description: "Compact durable memory context.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "mutates".into(),
                                tone: MenuBadgeTone::Warning,
                            }],
                            metadata: vec!["/memory compact".into()],
                            primary_action: Some({
                                let mut action = MenuActionProjection::command(
                                    "memory.compact.primary",
                                    "Compact",
                                    "/memory compact",
                                );
                                action.requires_confirmation = true;
                                action
                            }),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                    ],
                }],
            },
        ];
        menu
    }

    fn open_memory_menu(&mut self) {
        self.open_menu_projection(self.memory_menu_projection());
    }

    fn extension_runtime_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let mut menu = MenuProjection::new("extension-runtime", "Extensions & Runtime");
        menu.summary = Some("Extension inventory and runtime substrate controls. Argument-taking extension operations remain direct slash commands.".into());
        menu.footer = Some("↑/↓ navigate · / filter · Enter run · Esc close".into());
        menu.tabs = vec![MenuTabProjection {
            id: "overview".into(),
            label: "Overview".into(),
            groups: vec![
                MenuGroupProjection {
                    id: "extension.inventory".into(),
                    label: "Extensions".into(),
                    description: Some("Read extension inventory or search/install through explicit slash commands.".into()),
                    rows: vec![
                        MenuRowProjection {
                            id: "extension.view".into(),
                            label: "Extension inventory".into(),
                            description: "Show installed extension inventory as a text readout.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection { label: "read".into(), tone: MenuBadgeTone::Neutral }],
                            metadata: vec!["/extension view".into(), "/extension list".into()],
                            primary_action: Some(MenuActionProjection::command("extension.view.primary", "View", "/extension view")),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "extension.search".into(),
                            label: "Search extensions".into(),
                            description: "Search extension armory/catalog. Add a query with /extension search <query>.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection { label: "read".into(), tone: MenuBadgeTone::Neutral }],
                            metadata: vec!["/extension search".into(), "/extension search <query>".into()],
                            primary_action: Some(MenuActionProjection::prime_editor("extension.search.primary", "Search", "/extension search ", "Type an extension search query, then press Enter")),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "extension.update".into(),
                            label: "Update extensions".into(),
                            description: "Run the extension update flow for installed extensions.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection { label: "mutates".into(), tone: MenuBadgeTone::Warning }],
                            metadata: vec!["/extension update".into()],
                            primary_action: Some({ let mut action = MenuActionProjection::command("extension.update.primary", "Update", "/extension update"); action.requires_confirmation = true; action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu; action }),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                    ],
                },
                MenuGroupProjection {
                    id: "runtime.substrate".into(),
                    label: "Runtime substrate".into(),
                    description: Some("Refresh live skill/extension/runtime candidate inventory.".into()),
                    rows: vec![
                        MenuRowProjection {
                            id: "runtime.refresh".into(),
                            label: "Refresh runtime substrate".into(),
                            description: "Reload skill augments and inspect extension/runtime candidates; unavailable while a model turn is active.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection { label: "runtime".into(), tone: MenuBadgeTone::Warning }],
                            metadata: vec!["/runtime refresh".into(), "/extension refresh".into()],
                            primary_action: Some({ let mut action = MenuActionProjection::command("runtime.refresh.primary", "Refresh", "/runtime refresh"); action.requires_confirmation = true; action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu; action }),
                            actions: vec![{
                                let mut action = MenuActionProjection::command("runtime.refresh.action", "Refresh", "/runtime refresh");
                                action.key = Some("r".into());
                                action.requires_confirmation = true;
                                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                    ],
                },
            ],
        }];
        menu
    }

    fn open_extension_runtime_menu(&mut self) {
        self.open_menu_projection(self.extension_runtime_menu_projection());
    }

    fn profile_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let settings = self.settings();
        let loaded_profile = crate::settings::Profile::load_with_source(self.cwd());
        let drift = crate::surfaces::profile::ProfileDriftProjection::from_profile_and_settings(
            &loaded_profile.profile,
            loaded_profile.source,
            &settings,
        );
        let source_line = settings_profile_source_line(&drift.source);
        let drift_value = if drift.changed_count > 0 {
            format!("Δ{}", drift.changed_count)
        } else {
            "clean".into()
        };
        let mut menu = MenuProjection::new("profile", "Profile");
        menu.summary = Some(format!(
            "Persisted profile controls. {source_line}; runtime drift: {drift_value}."
        ));
        menu.footer = Some("↑/↓ navigate · / filter · Enter run · s save · explicit /profile apply to apply · Esc close".into());
        menu.tabs = vec![MenuTabProjection {
            id: "profile".into(),
            label: "Profile".into(),
            groups: vec![MenuGroupProjection {
                id: "profile.controls".into(),
                label: "Profile controls".into(),
                description: Some(
                    "Inspect, save, apply, and export persisted runtime profile state.".into(),
                ),
                rows: vec![
                    MenuRowProjection {
                        id: "profile.status".into(),
                        label: "Profile status".into(),
                        description: source_line.clone(),
                        value: Some(drift_value.clone()),
                        kind: MenuRowKind::Object,
                        badges: vec![MenuBadgeProjection {
                            label: "status".into(),
                            tone: MenuBadgeTone::Info,
                        }],
                        metadata: vec!["/profile view".into(), source_line.clone()],
                        primary_action: Some(MenuActionProjection::command(
                            "profile.status.primary",
                            "View",
                            "/profile view",
                        )),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "profile.save".into(),
                        label: "Save active profile".into(),
                        description:
                            "Capture current runtime settings to the active profile source.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection {
                            label: "writes".into(),
                            tone: MenuBadgeTone::Warning,
                        }],
                        metadata: vec!["/profile save".into()],
                        primary_action: Some(MenuActionProjection::command(
                            "profile.save.primary",
                            "Save",
                            "/profile save",
                        )),
                        actions: vec![{
                            let mut action = MenuActionProjection::command(
                                "profile.save.action",
                                "Save",
                                "/profile save",
                            );
                            action.key = Some("s".into());
                            action
                        }],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "profile.apply".into(),
                        label: "Apply persisted profile".into(),
                        description:
                            "Apply persisted profile defaults to current runtime settings.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection {
                            label: "mutates".into(),
                            tone: MenuBadgeTone::Warning,
                        }],
                        metadata: vec!["/profile apply".into()],
                        primary_action: Some({
                            let mut action = MenuActionProjection::command(
                                "profile.apply.primary",
                                "Apply",
                                "/profile apply",
                            );
                            action.requires_confirmation = true;
                            action
                        }),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "profile.save_project".into(),
                        label: "Save project profile".into(),
                        description: "Capture current runtime settings to .omegon/profile.json."
                            .into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection {
                            label: "writes".into(),
                            tone: MenuBadgeTone::Warning,
                        }],
                        metadata: vec!["/profile save --project".into()],
                        primary_action: Some(MenuActionProjection::command(
                            "profile.save_project.primary",
                            "Save project",
                            "/profile save --project",
                        )),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "profile.save_user".into(),
                        label: "Save user profile".into(),
                        description: "Capture current runtime settings to the user profile.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection {
                            label: "writes".into(),
                            tone: MenuBadgeTone::Warning,
                        }],
                        metadata: vec!["/profile save --user".into()],
                        primary_action: Some(MenuActionProjection::command(
                            "profile.save_user.primary",
                            "Save user",
                            "/profile save --user",
                        )),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "profile.export".into(),
                        label: "Export profile".into(),
                        description: "Render the current runtime profile as a text readout.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![MenuBadgeProjection {
                            label: "read".into(),
                            tone: MenuBadgeTone::Neutral,
                        }],
                        metadata: vec!["/profile export".into()],
                        primary_action: Some(MenuActionProjection::command(
                            "profile.export.primary",
                            "Export",
                            "/profile export",
                        )),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                ],
            }],
        }];
        menu
    }

    fn open_profile_menu(&mut self) {
        self.open_menu_projection(self.profile_menu_projection());
    }

    fn launch_surface_smoke(
        &mut self,
        scenario: crate::smoke_surface::SmokeScenarioKind,
    ) -> SlashResult {
        let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
        let response = crate::smoke_surface::launch_surface_smoke(
            &mut self.dashboard_handles,
            scenario,
            None,
            Some(tx),
        );
        if response.accepted {
            self.smoke_event_rx = Some(rx);
        }
        if let Some(cp) = self
            .dashboard_handles
            .cleave
            .as_ref()
            .and_then(|lock| lock.lock().ok())
        {
            self.dashboard.cleave = Some(cp.clone());
        }
        SlashResult::Display(
            response
                .output
                .unwrap_or_else(|| "Started unified cleave smoke suite.".into()),
        )
    }

    fn open_command_inventory_menu(&mut self) {
        let mut projection = crate::surfaces::menu::MenuProjection::from_command_menu(
            "commands",
            "Commands",
            self.command_menu_projection(),
        );
        projection.summary = Some(
            "Slash command inventory. Enter runs the selected command; / filters by command, metadata, or subcommand."
                .into(),
        );
        projection.footer = Some(
            "↑/↓ navigate · / filter · Enter run · Esc close · /help all for text readout".into(),
        );
        self.open_menu_projection(projection);
    }

    fn settings_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        use crate::surfaces::settings::{SettingsEditorProjection, SettingsStatusProjection};

        let settings = self.settings_projection();
        let settings_snapshot = self.settings();
        let loaded_profile = crate::settings::Profile::load_with_source(self.cwd());
        let profile_drift =
            crate::surfaces::profile::ProfileDriftProjection::from_profile_and_settings(
                &loaded_profile.profile,
                loaded_profile.source,
                &settings_snapshot,
            );
        let profile_source_line = settings_profile_source_line(&profile_drift.source);
        let drift_line = if profile_drift.changed_count > 0 {
            format!(
                "runtime drift: Δ{} · /profile save or /profile apply · {profile_source_line}",
                profile_drift.changed_count
            )
        } else {
            format!("runtime drift: clean · {profile_source_line}")
        };
        let mut menu = MenuProjection::new("settings", "Settings");
        menu.summary = Some(format!(
            "Runtime, workspace, profile, and update settings. Enter edits the selected row.\n{drift_line}"
        ));
        menu.footer = Some("↑/↓ navigate · Tab switch tabs · / filter · Enter edit · s save profile · a apply profile · Esc close".into());
        menu.tabs = settings
            .tabs
            .into_iter()
            .map(|tab| MenuTabProjection {
                id: tab.id.clone(),
                label: tab.label.clone(),
                groups: vec![MenuGroupProjection {
                    id: format!("settings.{}", tab.id),
                    label: tab.label,
                    description: None,
                    rows: tab
                        .rows
                        .into_iter()
                        .map(|row| {
                            let tone = match row.status {
                                SettingsStatusProjection::Normal => MenuBadgeTone::Neutral,
                                SettingsStatusProjection::Warning => MenuBadgeTone::Warning,
                                SettingsStatusProjection::Error => MenuBadgeTone::Danger,
                                SettingsStatusProjection::Disabled => MenuBadgeTone::Info,
                            };
                            let editor = match row.editor {
                                SettingsEditorProjection::Choice => "choice",
                                SettingsEditorProjection::Toggle => "toggle",
                                SettingsEditorProjection::Text => "text",
                                SettingsEditorProjection::Number => "number",
                                SettingsEditorProjection::Action => "action",
                                SettingsEditorProjection::ReadOnly => "read only",
                            };
                            let mut metadata =
                                vec![row.persistence.label().to_string(), editor.to_string()];
                            if let Some(profile) = row.profile {
                                metadata.push(format!("profile: {}", profile.profile_value));
                            }
                            let row_id = row.id.clone();
                            MenuRowProjection {
                                id: row.id,
                                label: row.label,
                                description: row.description,
                                value: Some(row.value),
                                kind: MenuRowKind::Object,
                                badges: vec![MenuBadgeProjection {
                                    label: format!("{:?}", row.status).to_lowercase(),
                                    tone,
                                }],
                                metadata,
                                primary_action: Some(MenuActionProjection::open_settings_row(
                                    format!("settings.{row_id}.open"),
                                    "Edit",
                                    row_id,
                                )),
                                actions: Vec::new(),
                                safety: None,
                                availability: None,
                            }
                        })
                        .collect(),
                }],
            })
            .collect();
        menu.actions = vec![
            {
                let mut action =
                    MenuActionProjection::command("settings.save", "Save profile", "/profile save");
                action.key = Some("s".into());
                action
            },
            {
                let mut action = MenuActionProjection::command(
                    "settings.apply",
                    "Apply profile",
                    "/profile apply",
                );
                action.key = Some("a".into());
                action
            },
        ];
        menu
    }

    fn open_skills_menu(&mut self) -> Result<(), String> {
        let entries = crate::skills::list_structured()
            .map_err(|err| format!("/skills list failed: {err}"))?;
        if entries.is_empty() {
            return Err("No skills found. Run /skills install to install bundled skills.".into());
        }
        let projection = crate::control_runtime::skills_menu_projection(&entries);
        self.open_menu_projection(projection);
        Ok(())
    }

    fn queue_settings_profile_save(&mut self, tx: &mpsc::Sender<TuiCommand>) {
        let _ = tx.try_send(TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ProfileCapture {
                target: crate::settings::ProfileSaveTarget::ActiveSource,
            },
            respond_to: None,
        });
        self.show_command_toast(CommandToast::new(
            "Saving runtime drift with /profile save",
            CommandSeverity::Info,
        ));
    }

    fn queue_settings_profile_apply(&mut self, tx: &mpsc::Sender<TuiCommand>) {
        let _ = tx.try_send(TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ProfileApply,
            respond_to: None,
        });
        self.show_command_toast(CommandToast::new(
            "Applying profile defaults with /profile apply",
            CommandSeverity::Info,
        ));
    }

    fn rebuild_active_menu(&mut self, menu_id: &str) -> bool {
        let projection = match menu_id {
            "ui" => self.ui_menu_projection(),
            "extension-runtime" => self.extension_runtime_menu_projection(),
            _ => return false,
        };
        self.active_menu = Some(ActiveMenu::new(projection));
        self.pending_menu_confirmation = None;
        true
    }

    fn execute_active_menu_action(
        &mut self,
        action: crate::surfaces::menu::MenuActionProjection,
        tx: &mpsc::Sender<TuiCommand>,
    ) -> SlashResult {
        if action.requires_confirmation {
            if self.pending_menu_confirmation.as_deref() != Some(action.id.as_str()) {
                self.pending_menu_confirmation = Some(action.id.clone());
                self.show_command_toast(CommandToast::new(
                    format!("Press Enter/shortcut again to confirm {}", action.label),
                    CommandSeverity::Warning,
                ));
                return SlashResult::Handled;
            }
            self.pending_menu_confirmation = None;
        } else {
            self.pending_menu_confirmation = None;
        }
        match action.disposition {
            crate::surfaces::menu::MenuActionDisposition::FocusRow => {
                if let Some(target_row_id) = action.target_row_id
                    && let Some(menu) = self.active_menu.as_mut()
                {
                    menu.state
                        .select_row_by_id(&menu.projection, &target_row_id);
                }
                SlashResult::Handled
            }
            crate::surfaces::menu::MenuActionDisposition::PrimeEditor => {
                self.active_menu = None;
                if let Some(text) = action.editor_text {
                    self.editor.set_text(&text);
                }
                if let Some(message) = action.message {
                    self.show_command_toast(CommandToast::new(message, CommandSeverity::Info));
                }
                SlashResult::Handled
            }
            crate::surfaces::menu::MenuActionDisposition::OpenSelector => {
                self.active_menu = None;
                self.pending_menu_confirmation = None;
                match action.target_row_id.as_deref() {
                    Some("context.class") => self.open_context_selector(),
                    Some("model.current") => self.open_model_selector(),
                    Some("model.grade") => self.open_model_grade_selector(),
                    Some("model.provider") => self.open_model_provider_selector(),
                    Some("model.policy") => self.open_model_policy_selector(),
                    Some("secrets.name") => self.open_secret_name_selector(),
                    _ => self.show_command_toast(CommandToast::new(
                        format!("No selector registered for {}", action.label),
                        CommandSeverity::Warning,
                    )),
                }
                SlashResult::Handled
            }
            crate::surfaces::menu::MenuActionDisposition::OpenSettingsRow => {
                if let Some(row_id) = action.target_row_id.as_deref() {
                    self.open_settings_row_by_id(row_id);
                } else {
                    self.show_command_toast(CommandToast::new(
                        format!("No settings row registered for {}", action.label),
                        CommandSeverity::Warning,
                    ));
                }
                SlashResult::Handled
            }
            crate::surfaces::menu::MenuActionDisposition::RunCommand => {
                if let Some(command) = action.command {
                    let menu_id = self
                        .active_menu
                        .as_ref()
                        .map(|menu| menu.projection.id.clone());
                    let result = self.execute_active_menu_command(command, tx);
                    if matches!(result, SlashResult::Handled)
                        && matches!(
                            action.close_policy,
                            crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu
                        )
                        && let Some(menu_id) = menu_id.as_deref()
                        && self.rebuild_active_menu(menu_id)
                    {
                        return SlashResult::Handled;
                    }
                    result
                } else {
                    SlashResult::Handled
                }
            }
        }
    }

    fn execute_active_menu_command(
        &mut self,
        command: String,
        tx: &mpsc::Sender<TuiCommand>,
    ) -> SlashResult {
        match self.handle_slash_command(&command, tx) {
            SlashResult::Display(response) => {
                self.history.push(command.clone());
                self.exit_history_recall();
                if matches!(self.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                    self.active_menu = None;
                    self.show_command_toast(CommandToast::new(response, CommandSeverity::Info));
                } else {
                    self.open_command_panel(
                        CommandPanel::from_slash(&command, response)
                            .with_return_target(CommandPanelReturnTarget::Menu),
                    );
                }
                SlashResult::Handled
            }
            SlashResult::Handled => {
                self.active_menu = None;
                self.history.push(command);
                self.exit_history_recall();
                SlashResult::Handled
            }
            SlashResult::Quit => {
                self.active_menu = None;
                self.history.push(command);
                self.exit_history_recall();
                self.should_quit = true;
                SlashResult::Quit
            }
            SlashResult::NotACommand => SlashResult::Handled,
        }
    }

    fn provider_status_rows(
        &self,
        row_prefix: &str,
    ) -> Vec<crate::surfaces::menu::MenuRowProjection> {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuRowKind,
            MenuRowProjection,
        };
        let provider_ids: Vec<&str> = if row_prefix == "auth.provider" {
            crate::auth::operator_auth_provider_ids()
        } else {
            vec![
                "anthropic",
                "openai-codex",
                "github-copilot",
                "openai",
                "openrouter",
                "google",
                "ollama",
            ]
        };
        let settings_model = self.settings().model.clone();
        let selected_provider = self
            .route_selected_model
            .as_deref()
            .or_else(|| (!settings_model.is_empty()).then_some(settings_model.as_str()))
            .map(crate::providers::infer_provider_id);
        let serving_provider = self
            .route_serving_model
            .as_deref()
            .or_else(|| {
                (!self.footer_data.model_id.is_empty())
                    .then_some(self.footer_data.model_id.as_str())
            })
            .map(crate::providers::infer_provider_id);
        provider_ids
            .into_iter()
            .map(|provider| {
                let status = crate::surfaces::menu::ProviderStatusProjection::from_credential_probe(
                    provider,
                );
                let login_command = status
                    .remediation_command
                    .clone()
                    .unwrap_or_else(|| format!("/login {provider}"));
                let logout_command = format!("/logout {provider}");
                let mut badges = vec![MenuBadgeProjection {
                    label: status.badge_label().into(),
                    tone: status.badge_tone(),
                }];
                let mut metadata = vec![
                    login_command.clone(),
                    logout_command.clone(),
                    format!("provider: {}", status.provider_id),
                ];
                if selected_provider.as_deref() == Some(provider) {
                    badges.push(MenuBadgeProjection {
                        label: "selected".into(),
                        tone: MenuBadgeTone::Info,
                    });
                    metadata.push("route: selected".into());
                }
                if serving_provider.as_deref() == Some(provider) {
                    badges.push(MenuBadgeProjection {
                        label: "serving".into(),
                        tone: MenuBadgeTone::Success,
                    });
                    metadata.push("route: serving".into());
                    if self.route_state.as_deref() == Some("fallback")
                        && selected_provider
                            .as_deref()
                            .is_some_and(|selected| selected != provider)
                    {
                        badges.push(MenuBadgeProjection {
                            label: "fallback".into(),
                            tone: MenuBadgeTone::Warning,
                        });
                        metadata.push("route: fallback serving".into());
                    }
                }
                MenuRowProjection {
                    id: format!("{row_prefix}.{provider}"),
                    label: status.display_name.clone(),
                    description: status.credential_state.clone(),
                    value: Some(status.provider_id.clone()),
                    kind: MenuRowKind::Object,
                    badges,
                    metadata,
                    primary_action: Some(MenuActionProjection::command(
                        format!("{row_prefix}.{provider}.login"),
                        "Login",
                        login_command.clone(),
                    )),
                    actions: vec![
                        {
                            let mut action = MenuActionProjection::command(
                                format!("{row_prefix}.{provider}.login.action"),
                                "Login",
                                login_command,
                            );
                            action.key = Some("l".into());
                            action
                        },
                        {
                            let mut action = MenuActionProjection::command(
                                format!("{row_prefix}.{provider}.logout.action"),
                                "Logout",
                                logout_command,
                            );
                            action.key = Some("o".into());
                            action
                        },
                    ],
                    safety: None,
                    availability: None,
                }
            })
            .collect()
    }

    fn open_auth_menu(&mut self) {
        use crate::surfaces::menu::{MenuGroupProjection, MenuProjection, MenuTabProjection};
        let mut menu = MenuProjection::new("auth", "Authentication");
        let mut summary = "Provider authentication status. Enter logs into the selected provider; l login; o logout; / filters providers.".to_string();
        if self.route_state.is_some()
            || self.route_selected_model.is_some()
            || self.route_serving_model.is_some()
            || self.footer_data.route_warning.is_some()
        {
            let route_state = self.route_state.as_deref().unwrap_or("unknown");
            summary.push_str(&format!(
                "
route: {route_state}"
            ));
            if let Some(selected) = self.route_selected_model.as_deref() {
                summary.push_str(&format!(" · selected: {selected}"));
            }
            if let Some(serving) = self.route_serving_model.as_deref() {
                summary.push_str(&format!(" · serving: {serving}"));
            }
            if let Some(warning) = self.footer_data.route_warning.as_deref() {
                summary.push_str(&format!(
                    "
warning: {warning}"
                ));
            }
        }
        menu.summary = Some(summary);
        menu.footer = Some(
            "↑/↓ navigate · Enter login · l login · o logout · / filter · Esc close · /auth status for text readout".into(),
        );
        menu.tabs = vec![MenuTabProjection {
            id: "providers".into(),
            label: "Providers".into(),
            groups: vec![MenuGroupProjection {
                id: "auth.providers".into(),
                label: "Provider credentials".into(),
                description: Some("Credential probe status and login/logout actions.".into()),
                rows: self.provider_status_rows("auth.provider"),
            }],
        }];
        self.open_menu_projection(menu);
    }

    fn open_model_menu(&mut self) {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };

        let settings = self.settings();
        let selected_model = settings.model.clone();
        let intent = crate::route::ModelIntent::pinned_model(selected_model.clone());
        let grade_value = intent
            .grade
            .as_ref()
            .map(crate::route::ModelGrade::as_str)
            .unwrap_or("auto")
            .to_string();
        let provider_value = match &intent.provider_selection {
            crate::route::ProviderSelection::Auto => "auto".to_string(),
            crate::route::ProviderSelection::Local => "local".to_string(),
            crate::route::ProviderSelection::Upstream => "upstream".to_string(),
            crate::route::ProviderSelection::Endpoint(endpoint) => endpoint.clone(),
        };
        let policy_value = match &intent.grade_policy {
            crate::route::GradePolicy::Exact => "exact".to_string(),
            crate::route::GradePolicy::Minimum => "minimum".to_string(),
            crate::route::GradePolicy::NearestAllowed { .. } => "nearest".to_string(),
        };
        let mut menu = MenuProjection::new("model", "Model");
        let mut summary = format!(
            "Configured model: {selected_model}. Enter opens the provider/model selector; use row actions to route intent."
        );
        if self.route_state.is_some()
            || self.route_selected_model.is_some()
            || self.route_serving_model.is_some()
            || self.footer_data.route_warning.is_some()
        {
            let route_state = self.route_state.as_deref().unwrap_or("unknown");
            let selected = self
                .route_selected_model
                .as_deref()
                .unwrap_or(&selected_model);
            let serving = self
                .route_serving_model
                .as_deref()
                .unwrap_or(&self.footer_data.model_id);
            summary.push_str(&format!(
                "
route: {route_state} · selected: {selected}"
            ));
            if !serving.is_empty() {
                summary.push_str(&format!(" · serving: {serving}"));
            }
            if let Some(warning) = self.footer_data.route_warning.as_deref() {
                summary.push_str(&format!(
                    "
warning: {warning}"
                ));
            }
        }
        menu.summary = Some(summary);
        menu.footer = Some("↑/↓ navigate · Enter choose model · g grade · p provider · o policy · u unpin · / filter · Esc close".into());
        let provider_rows = self.provider_status_rows("provider");
        menu.tabs = vec![
            MenuTabProjection {
                id: "routing".into(),
                label: "Routing".into(),
                groups: vec![MenuGroupProjection {
                    id: "model.routing".into(),
                    label: "Routing".into(),
                    description: Some("Model routing intents and exact pin controls.".into()),
                    rows: vec![
                        MenuRowProjection {
                            id: "model.current".into(),
                            label: "Current model".into(),
                            description:
                                "Open the model selector to choose an exact provider:model route."
                                    .into(),
                            value: Some(selected_model),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "active".into(),
                                tone: MenuBadgeTone::Success,
                            }],
                            metadata: vec!["selector".into(), "exact model".into()],
                            primary_action: Some(MenuActionProjection::open_selector(
                                "model.current.select",
                                "Choose model",
                                "model.current",
                            )),
                            actions: vec![
                                {
                                    let mut action = MenuActionProjection::focus_row(
                                        "model.current.grade",
                                        "Grade row",
                                        "model.grade",
                                    );
                                    action.key = Some("g".into());
                                    action
                                },
                                {
                                    let mut action = MenuActionProjection::focus_row(
                                        "model.current.provider",
                                        "Provider row",
                                        "model.provider",
                                    );
                                    action.key = Some("p".into());
                                    action
                                },
                                {
                                    let mut action = MenuActionProjection::focus_row(
                                        "model.current.policy",
                                        "Policy row",
                                        "model.policy",
                                    );
                                    action.key = Some("o".into());
                                    action
                                },
                            ],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "model.grade".into(),
                            label: "Model grade".into(),
                            description: "Set model quality intent: F, D, C, B, A, or S.".into(),
                            value: Some(grade_value),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "intent".into(),
                                tone: MenuBadgeTone::Info,
                            }],
                            metadata: vec!["/model grade <F|D|C|B|A|S>".into()],
                            primary_action: None,
                            actions: vec![{
                                let mut action = MenuActionProjection::focus_row(
                                    "model.grade.action",
                                    "Choose grade",
                                    "model.grade",
                                );
                                action.key = Some("g".into());
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "model.provider".into(),
                            label: "Provider intent".into(),
                            description: "Set provider intent: auto, local, upstream, or endpoint."
                                .into(),
                            value: Some(provider_value),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "intent".into(),
                                tone: MenuBadgeTone::Info,
                            }],
                            metadata: vec!["/model provider <auto|local|upstream|endpoint>".into()],
                            primary_action: None,
                            actions: vec![{
                                let mut action = MenuActionProjection::focus_row(
                                    "model.provider.action",
                                    "Choose provider",
                                    "model.provider",
                                );
                                action.key = Some("p".into());
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "model.policy".into(),
                            label: "Routing policy".into(),
                            description: "Set routing policy: exact, minimum, or nearest.".into(),
                            value: Some(policy_value),
                            kind: MenuRowKind::Object,
                            badges: vec![MenuBadgeProjection {
                                label: "policy".into(),
                                tone: MenuBadgeTone::Neutral,
                            }],
                            metadata: vec!["/model policy <exact|minimum|nearest>".into()],
                            primary_action: None,
                            actions: vec![{
                                let mut action = MenuActionProjection::focus_row(
                                    "model.policy.action",
                                    "Choose policy",
                                    "model.policy",
                                );
                                action.key = Some("o".into());
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "model.unpin".into(),
                            label: "Clear exact pin".into(),
                            description: "Clear the exact model pin and route by current intent."
                                .into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: "action".into(),
                                tone: MenuBadgeTone::Warning,
                            }],
                            metadata: vec!["/model unpin".into()],
                            primary_action: Some(MenuActionProjection::command(
                                "model.unpin.primary",
                                "Unpin",
                                "/model unpin",
                            )),
                            actions: vec![{
                                let mut action = MenuActionProjection::command(
                                    "model.unpin.action",
                                    "Unpin",
                                    "/model unpin",
                                );
                                action.key = Some("u".into());
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                    ],
                }],
            },
            MenuTabProjection {
                id: "providers".into(),
                label: "Providers".into(),
                groups: vec![MenuGroupProjection {
                    id: "model.providers".into(),
                    label: "Provider status".into(),
                    description: Some(
                        "Credential probe status and login actions for common model providers."
                            .into(),
                    ),
                    rows: provider_rows,
                }],
            },
        ];
        self.open_menu_projection(menu);
    }

    fn open_settings_row_by_id(&mut self, row_id: &str) {
        let projection = self.settings_projection();
        let Some(row) = projection
            .tabs
            .iter()
            .flat_map(|tab| tab.rows.iter())
            .find(|row| row.id == row_id)
        else {
            self.show_command_toast(CommandToast::new(
                format!("No settings row registered for {row_id}"),
                CommandSeverity::Warning,
            ));
            return;
        };
        let row_id = row.id.clone();
        let row_label = row.label.clone();
        let row_choices = row.choices.clone();

        if let Some(kind) = Self::selector_kind_for_settings_row(&row_id)
            && !row_choices.is_empty()
        {
            let options = row_choices
                .into_iter()
                .map(|choice| selector::SelectOption {
                    value: choice.value,
                    label: choice.label,
                    description: row_label.clone(),
                    active: choice.active,
                })
                .collect();
            self.active_menu = None;
            self.pending_menu_confirmation = None;
            self.selector = Some(selector::Selector::new(&row_label, options));
            self.selector_kind = Some(kind);
            return;
        }

        match row_id.as_str() {
            "runtime.model" => {
                self.active_menu = None;
                self.pending_menu_confirmation = None;
                self.open_model_selector();
            }
            "runtime.max_turns" => {
                self.active_menu = None;
                self.pending_menu_confirmation = None;
                self.open_max_turns_selector();
            }
            "workspace.sandbox" => self.toggle_settings_sandbox(),
            "updates.auto_update" => self.toggle_settings_auto_update(),
            "workspace.trusted_directories" => {
                let settings = self.settings();
                if settings.trusted_directories.is_empty() {
                    self.show_command_toast(CommandToast::new(
                        "No trusted directories. Use /permissions add <path> to add one.",
                        CommandSeverity::Info,
                    ));
                } else {
                    self.show_command_toast(CommandToast::new(
                        "Trusted directories are managed with /permissions add|remove <path>",
                        CommandSeverity::Info,
                    ));
                }
            }
            _ => self.show_command_toast(CommandToast::new(
                format!("No editor registered for {}", row.label),
                CommandSeverity::Warning,
            )),
        }
    }

    fn selector_kind_for_settings_row(row_id: &str) -> Option<SelectorKind> {
        match row_id {
            "runtime.thinking" => Some(SelectorKind::ThinkingLevel),
            "runtime.context_class" => Some(SelectorKind::ContextClass),
            "runtime.max_turns" => Some(SelectorKind::MaxTurns),
            "ui.tool_detail" => Some(SelectorKind::ToolDetail),
            "updates.channel" => Some(SelectorKind::UpdateChannel),
            "workspace.role" => Some(SelectorKind::WorkspaceRole),
            "workspace.kind" => Some(SelectorKind::WorkspaceKind),
            _ => None,
        }
    }

    fn open_preferences_selector(&mut self) {
        let settings = self.settings();
        let options = settings_menu::preferences_selector_options(&settings);
        self.selector = Some(selector::Selector::new("Preferences", options));
        self.selector_kind = Some(SelectorKind::Preferences);
    }

    fn open_tool_detail_selector(&mut self) {
        let current = self.settings().tool_detail;
        let options = settings_menu::tool_detail_selector_options(current);
        self.selector = Some(selector::Selector::new("Tool Density", options));
        self.selector_kind = Some(SelectorKind::ToolDetail);
    }

    fn open_max_turns_selector(&mut self) {
        let current = self.settings().max_turns;
        let options = settings_menu::max_turns_selector_options(current);
        self.selector = Some(selector::Selector::new("Max Turns", options));
        self.selector_kind = Some(SelectorKind::MaxTurns);
    }

    fn toggle_settings_sandbox(&mut self) {
        let enabled = self.settings().sandbox;
        if enabled {
            self.update_and_persist(|s| s.sandbox = false);
            self.show_command_toast(CommandToast::new(
                "Sandbox disabled. Children run as local subprocesses.",
                CommandSeverity::Info,
            ));
            return;
        }

        let runtime = crate::nex::spawn::detect_container_runtime_public();
        if let Some(rt) = runtime {
            self.update_and_persist(|s| s.sandbox = true);
            self.show_command_toast(CommandToast::new(
                format!("Sandbox enabled ({rt})"),
                CommandSeverity::Info,
            ));
        } else {
            self.show_command_toast(CommandToast::new(
                "No container runtime found. Sandbox requires podman or docker.",
                CommandSeverity::Warning,
            ));
        }
    }

    fn toggle_settings_auto_update(&mut self) {
        let enabled = self.settings().auto_update;
        let next = !enabled;
        self.update_and_persist(|s| s.auto_update = next);
        self.show_command_toast(CommandToast::new(
            format!("Auto update → {}", if next { "on" } else { "off" }),
            CommandSeverity::Info,
        ));
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
        let options = settings_menu::update_channel_selector_options(&current);
        self.selector = Some(selector::Selector::new("Update Channel", options));
        self.selector_kind = Some(SelectorKind::UpdateChannel);
    }

    fn open_workspace_role_selector(&mut self) {
        let options = settings_menu::workspace_role_selector_options();
        self.selector = Some(selector::Selector::new("Workspace Role", options));
        self.selector_kind = Some(SelectorKind::WorkspaceRole);
    }

    fn open_workspace_kind_selector(&mut self) {
        let options = settings_menu::workspace_kind_selector_options();
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
                (Some(old), Some(new)) if old.id != new.id => {
                    self.show_toast(
                        &format!("Persona → {} {}", new.badge, new.name),
                        ratatui_toaster::ToastType::Info,
                    );
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
                (Some(old), Some(new)) if old.id != new.id => {
                    self.show_toast(
                        &format!("Tone → {}", new.name),
                        ratatui_toaster::ToastType::Info,
                    );
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
            SelectorKind::ModelGrade => {
                let _ = tx.try_send(TuiCommand::SetModelGrade {
                    grade: value.clone(),
                    respond_to: None,
                });
                Some(format!("Switching Model Intent → grade {value}"))
            }
            SelectorKind::ModelProvider => {
                let _ = tx.try_send(TuiCommand::SetModelProvider {
                    provider: value.clone(),
                    respond_to: None,
                });
                Some(format!("Switching Model Provider Intent → {value}"))
            }
            SelectorKind::ModelPolicy => {
                let _ = tx.try_send(TuiCommand::SetModelPolicy {
                    policy: value.clone(),
                    respond_to: None,
                });
                Some(format!("Switching Model Policy Intent → {value}"))
            }
            SelectorKind::ThinkingLevel => {
                let outcome = settings_menu::apply_thinking_selection(&value);
                if let settings_menu::SettingApplyOutcome::Thinking(level) = outcome {
                    let _ = tx.try_send(TuiCommand::SetThinking {
                        level,
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::ContextClass => {
                let outcome = settings_menu::apply_context_class_selection(&value);
                if let settings_menu::SettingApplyOutcome::ContextClass(class) = outcome {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SetContextClass { class },
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::Persona => {
                let (personas, _) = crate::plugins::persona_loader::scan_available();
                if let Some(available) = personas.into_iter().find(|persona| persona.id == value) {
                    match crate::plugins::persona_loader::load_persona(&available.path) {
                        Ok(persona) => {
                            let name = persona.name.clone();
                            let badge = persona.badge.clone().unwrap_or_else(|| "⚙".into());
                            let fact_count = persona.mind_facts.len();
                            if let Some(ref mut registry) = self.augment_registry {
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
                            if let Some(ref mut registry) = self.augment_registry {
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
            SelectorKind::SecretAction => match value.as_str() {
                "list" => {
                    if let Some(request) = crate::control_runtime::control_request_from_slash(
                        &CanonicalSlashCommand::SecretsView,
                    ) {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                    }
                    Some("Listing configured secrets…".to_string())
                }
                "set" => {
                    self.open_secret_name_selector();
                    Some("Pick a secret to configure.".to_string())
                }
                "delete" => {
                    self.editor.set_text("/secrets delete ");
                    Some("Type the secret name to delete, then press Enter.".to_string())
                }
                _ => Some(format!("Unknown secrets action: {value}")),
            },
            SelectorKind::LoginProvider => {
                // OAuth providers go through the auth login flow (opens browser)
                // API key providers go through secret input mode (hidden input)
                match value.as_str() {
                    p if crate::auth::provider_by_id(p).is_some_and(|provider| {
                        provider.auth_method == crate::auth::AuthMethod::OAuth
                    }) =>
                    {
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
                    Some("Type: /secrets set NAME, then press Enter for hidden input".to_string())
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
                let outcome = settings_menu::apply_update_channel_selection(&value);
                if let settings_menu::SettingApplyOutcome::UpdateChannel(channel) = outcome {
                    self.update_settings(|s| s.update_channel = channel.as_str().to_string());
                    if let Some(tx) = self.update_tx.clone() {
                        crate::update::spawn_check_now(tx, channel);
                    }
                }
                Some(outcome.message())
            }
            SelectorKind::WorkspaceRole => {
                let outcome = settings_menu::apply_workspace_role_selection(&value);
                if let settings_menu::SettingApplyOutcome::WorkspaceRole(role) = outcome {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::WorkspaceRoleSet { role },
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::WorkspaceKind => {
                let outcome = settings_menu::apply_workspace_kind_selection(&value);
                if let settings_menu::SettingApplyOutcome::WorkspaceKind(kind) = outcome {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::WorkspaceKindSet { kind },
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::MaxTurns => {
                let outcome = settings_menu::apply_max_turns_selection(&value);
                if let settings_menu::SettingApplyOutcome::MaxTurns(max_turns) = outcome {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::SetMaxTurns { max_turns },
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::Preferences => {
                // Open the sub-selector for the chosen preference category
                match value.as_str() {
                    "model" => {
                        self.open_model_selector();
                        None
                    }
                    "thinking" => {
                        self.open_thinking_selector();
                        None
                    }
                    "context" => {
                        self.open_context_selector();
                        None
                    }
                    "detail" => {
                        self.open_tool_detail_selector();
                        None
                    }
                    "persona" => {
                        self.open_persona_selector();
                        None
                    }
                    "tone" => {
                        self.open_tone_selector();
                        None
                    }
                    "permissions" | "trust" => {
                        let s = self.settings();
                        if s.trusted_directories.is_empty() {
                            Some(
                                "No trusted directories. Use /permissions add <path> to add one."
                                    .into(),
                            )
                        } else {
                            let list = s.trusted_directories.join("\n  ");
                            Some(format!(
                                "Trusted directories:\n  {list}\n\nUse /permissions add|remove <path> to manage."
                            ))
                        }
                    }
                    "update" => {
                        self.open_update_channel_selector();
                        None
                    }
                    _ => Some(format!("Unknown preference: {value}")),
                }
            }
            SelectorKind::ToolDetail => {
                let outcome = settings_menu::apply_tool_detail_selection(&value);
                if let settings_menu::SettingApplyOutcome::ToolDetail(mode) = outcome {
                    self.update_and_persist(|s| s.tool_detail = mode);
                }
                Some(outcome.message())
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

    /// Write a setting AND persist to profile.json.
    fn update_and_persist<F: FnOnce(&mut crate::settings::Settings)>(&self, f: F) {
        let cwd = self.cwd().to_path_buf();
        if let Ok(mut s) = self.settings.lock() {
            f(&mut s);
            let mut profile = crate::settings::Profile::load(&cwd);
            profile.capture_from(&s);
            let _ = profile.save(&cwd);
        }
    }

    /// Try to cancel the active agent turn. Returns true if cancelled.
    /// Queue a prompt to be sent when the agent finishes.
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

    fn open_secret_name_selector(&mut self) {
        let options: Vec<selector::SelectOption> = Self::SECRET_CATALOG
            .iter()
            .map(|(name, recipe, desc)| selector::SelectOption {
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
                active: false,
            })
            .collect();
        self.selector = Some(selector::Selector::new("Set Secret — pick a name", options));
        self.selector_kind = Some(SelectorKind::SecretName);
    }

    /// Handle /variables — non-secret runtime configuration.
    fn handle_variables(&mut self, args: &str, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        if args.trim().is_empty() {
            self.open_variables_menu();
            return SlashResult::Handled;
        }
        if let Some(command) = canonical_slash_command("variables", args) {
            if let Some(request) = crate::control_runtime::control_request_from_slash(&command) {
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request,
                    respond_to: None,
                });
                SlashResult::Handled
            } else {
                SlashResult::Display(
                    "Usage: /variables [list|status|set <name> <value>|get <name>|delete|remove|rm <name>]".into(),
                )
            }
        } else {
            SlashResult::Display(
                "Usage: /variables [list|status|set <name> <value>|get <name>|delete|remove|rm <name>]".into(),
            )
        }
    }

    /// Handle /secrets — interactive secret management.
    fn handle_secrets(&mut self, args: &str, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        match parts.first().copied().unwrap_or("") {
            "" => {
                self.open_secrets_menu();
                SlashResult::Handled
            }
            // /secrets set NAME → enter hidden input mode for arbitrary operator secrets.
            "set" if parts.len() == 2 && !parts[1].trim().is_empty() => {
                let name = parts[1].trim();
                self.editor.start_secret_input(name);
                SlashResult::Display(format!(
                    "🔒 Paste or type value for {name} (input is hidden):"
                ))
            }
            // /secrets configure and /secrets set with no name/value → open shared menu
            "configure" | "set" if parts.len() < 3 => {
                let _ = tx;
                self.open_secrets_menu();
                SlashResult::Handled
            }
            "set" if parts.len() >= 3 && !parts[1].trim().is_empty() => {
                let name = parts[1].trim();
                let value = parts[2].trim();
                let recipe_like = value.starts_with("env:")
                    || value.starts_with("cmd:")
                    || value.starts_with("vault:");
                if recipe_like {
                    if let Some(command) = canonical_slash_command("secrets", args)
                        && let Some(request) =
                            crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /secrets set <name> <env:VAR|cmd:COMMAND|vault:PATH>".into(),
                        )
                    }
                } else {
                    self.editor.start_secret_input(name);
                    SlashResult::Display(format!(
                        "🔒 Direct secret values are captured only through hidden input. Paste or type value for {name} now (input is hidden):"
                    ))
                }
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
                            "Usage: /secrets [list|status|set <name> [env:VAR|cmd:COMMAND|vault:PATH]|get <name>|delete|remove|rm <name>]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /secrets [list|status|set <name> [env:VAR|cmd:COMMAND|vault:PATH]|get <name>|delete|remove|rm <name>]".into(),
                    )
                }
            }
        }
    }

    fn submit_prompt_from_slash(
        tx: &mpsc::Sender<TuiCommand>,
        prompt: PromptSubmission,
    ) -> Result<(), SlashResult> {
        tx.try_send(TuiCommand::SubmitPrompt(prompt)).map_err(|_| {
            SlashResult::Display(
                "Runtime command queue is full; prompt was not queued. Try again shortly.".into(),
            )
        })
    }

    /// Handle /tutorial — start, resume, or manage the interactive tutorial overlay.
    fn handle_tutorial(&mut self, args: &str, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
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
                SlashResult::Display("No tutorial active. Type /help tutorial to start.".into())
            }
            "reset" => {
                if self.tutorial_overlay.is_some() {
                    self.tutorial_overlay = None;
                    return SlashResult::Display(
                        "Tutorial overlay reset. Type /help tutorial to start again.".into(),
                    );
                }
                if let Some(ref mut tut) = self.tutorial {
                    tut.reset();
                    return SlashResult::Display(
                        "Tutorial reset to lesson 1. Type /help tutorial to start.".into(),
                    );
                }
                SlashResult::Display("No tutorial active.".into())
            }
            "demo" => {
                // Resume existing overlay if still active
                if let Some(ref overlay) = self.tutorial_overlay
                    && overlay.active
                {
                    return SlashResult::Display(format!(
                        "Tutorial overlay active (step {}/{}). Press Tab to advance, Esc to dismiss.",
                        overlay.step_index() + 1,
                        overlay.total_steps(),
                    ));
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
                if tutorial_dir.is_dir()
                    && let Some(tut) = TutorialState::load(&tutorial_dir)
                {
                    let lesson = tut.current_lesson().clone();
                    let status = tut.status_line();
                    self.tutorial = Some(tut);
                    if let Err(result) = Self::submit_prompt_from_slash(
                        tx,
                        PromptSubmission {
                            text: lesson.content,
                            image_paths: Vec::new(),
                            submitted_by: "local-tui".to_string(),
                            via: "tui",
                            queue_mode: PromptQueueMode::UntilReady,
                            metadata: PromptMetadata::default(),
                        },
                    ) {
                        return result;
                    }
                    return SlashResult::Display(format!(
                        "{status}\n\nLesson queued. The agent will begin when ready."
                    ));
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
                     Note: Anthropic subscription OAuth is the interactive path.\n\
                     For policy-clean background tasks, /cleave, and --prompt, use ANTHROPIC_API_KEY.\n\n\
                     Tab to advance, Esc to dismiss."
                        .into(),
                )
            }
            _ => {
                // Resume existing overlay if still active
                if let Some(ref overlay) = self.tutorial_overlay
                    && overlay.active
                {
                    let mode_note = match overlay.mode {
                        tutorial::TutorialMode::ConsentRequired => {
                            "\n\nℹ Anthropic subscription detected. Type /help tutorial consent\nto enable interactive agent steps (uses subscription quota)."
                        }
                        tutorial::TutorialMode::OrientationOnly => {
                            "\n\nℹ No B-grade cloud model found. Add an API key or\n/auth login openai-codex for the full interactive tutorial."
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
                         Type /help tutorial consent to enable interactive agent steps,\n\
                         or add an API key / /auth login openai-codex for automatic access.\n\n\
                         Tab to advance orientation steps, Esc to dismiss."
                            .to_string()
                    }
                    tutorial::TutorialMode::OrientationOnly => {
                        "Tutorial started (orientation mode).\n\n\
                         No B-grade cloud model found. Add an API key or\n\
                         /auth login openai-codex for the full interactive tutorial.\n\n\
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
    fn handle_tutorial_next(&mut self, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        if let Some(ref mut overlay) = self.tutorial_overlay
            && overlay.active
        {
            overlay.advance();
            return SlashResult::Display(format!(
                "Tutorial step {}/{}",
                overlay.step_index() + 1,
                overlay.total_steps()
            ));
        }
        if let Some(ref mut tut) = self.tutorial {
            if tut.advance() {
                let lesson = tut.current_lesson().clone();
                let status = tut.status_line();
                if let Err(result) = Self::submit_prompt_from_slash(
                    tx,
                    PromptSubmission {
                        text: lesson.content,
                        image_paths: Vec::new(),
                        submitted_by: "local-tui".to_string(),
                        via: "tui",
                        queue_mode: PromptQueueMode::UntilReady,
                        metadata: PromptMetadata::default(),
                    },
                ) {
                    return result;
                }
                SlashResult::Display(format!("{status}\n\nLesson queued."))
            } else {
                SlashResult::Display(
                    "🎉 You've completed the tutorial! Type /help tutorial reset to start over."
                        .into(),
                )
            }
        } else {
            SlashResult::Display("No tutorial active. Type /help tutorial to start.".into())
        }
    }

    /// Go back to the previous tutorial step/lesson.
    fn handle_tutorial_prev(&mut self, tx: &mpsc::Sender<TuiCommand>) -> SlashResult {
        if let Some(ref mut overlay) = self.tutorial_overlay
            && overlay.active
        {
            overlay.go_back();
            return SlashResult::Display(format!(
                "Tutorial step {}/{}",
                overlay.step_index() + 1,
                overlay.total_steps()
            ));
        }
        if let Some(ref mut tut) = self.tutorial {
            if tut.go_back() {
                let lesson = tut.current_lesson().clone();
                let status = tut.status_line();
                if let Err(result) = Self::submit_prompt_from_slash(
                    tx,
                    PromptSubmission {
                        text: lesson.content,
                        image_paths: Vec::new(),
                        submitted_by: "local-tui".to_string(),
                        via: "tui",
                        queue_mode: PromptQueueMode::UntilReady,
                        metadata: PromptMetadata::default(),
                    },
                ) {
                    return result;
                }
                SlashResult::Display(format!("{status}\n\nLesson queued."))
            } else {
                SlashResult::Display("Already at the first lesson.".into())
            }
        } else {
            SlashResult::Display("No tutorial active. Type /help tutorial to start.".into())
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
                     Try /help tutorial instead — it works with your current project,\n\
                     no download needed. Or check your network and try /help tutorial demo again."
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
                .arg("compact")
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
                .arg("compact")
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
            [version, "status"] => {
                let milestones = load_milestones(&milestone_file);
                if let Some(ms) = milestones.get(*version) {
                    let total = ms.nodes.len();
                    let mut implemented: usize = 0;
                    let mut decided: usize = 0;
                    let mut exploring: usize = 0;
                    let mut seed: usize = 0;
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
                    let progress = (implemented * 100).checked_div(total).unwrap_or(0);
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

    async fn handle_ui_action(
        &mut self,
        action: UiAction,
        command_tx: &mpsc::Sender<TuiCommand>,
    ) -> UiActionOutcome {
        match action {
            UiAction::SubmitPrompt(action) => {
                self.submit_prefixed_prompt(action.text, action.attachments, command_tx)
                    .await;
                UiActionOutcome::accepted()
            }
            UiAction::SubmitContinuation => {
                self.awaiting_continuation = false;
                self.editor
                    .textarea
                    .set_placeholder_text("Ask anything, or type / for commands");
                self.submit_prefixed_prompt(
                    "Continue with the plan as described.".to_string(),
                    vec![],
                    command_tx,
                )
                .await;
                UiActionOutcome::accepted()
            }
            UiAction::CancelActiveTurn => {
                self.prepare_interrupt_ui();
                let _ = command_tx
                    .send(TuiCommand::CancelActiveTurn {
                        submitted_by: "local-tui".to_string(),
                        via: "tui",
                    })
                    .await;
                UiActionOutcome::accepted_message("active turn cancellation requested")
            }
            UiAction::RespondToPermission(action) => self.handle_permission_action(action),
            UiAction::RespondToOperatorWait(action) => self.handle_operator_wait_action(action),
            UiAction::RunSlashCommand(action) => {
                match self.handle_slash_command(&action.raw, command_tx) {
                    SlashResult::Display(response) => {
                        self.history.push(action.raw.clone());
                        self.exit_history_recall();
                        self.open_command_panel(CommandPanel::from_slash(&action.raw, &response));
                        UiActionOutcome::accepted_message(response)
                    }
                    SlashResult::Handled => {
                        self.history.push(action.raw);
                        self.exit_history_recall();
                        UiActionOutcome::accepted()
                    }
                    SlashResult::Quit => {
                        self.history.push(action.raw);
                        self.exit_history_recall();
                        self.should_quit = true;
                        let _ = command_tx.send(TuiCommand::Quit).await;
                        UiActionOutcome::accepted_message("quit requested")
                    }
                    SlashResult::NotACommand => UiActionOutcome::rejected("not a slash command"),
                }
            }
            UiAction::SetUiPreset(action) => self.handle_ui_preset_action(action),
            UiAction::SetSurfaceVisible(action) => self.handle_surface_visible_action(action),
            UiAction::SelectConversationSegment(action) => {
                self.handle_select_conversation_segment_action(action)
            }
            UiAction::OpenConversationSegmentDetail(action) => {
                self.handle_open_conversation_segment_detail_action(action)
            }
            UiAction::ReplaceComposerDraft(action) => {
                self.handle_replace_composer_draft_action(action)
            }
            UiAction::ClearComposerDraft => self.handle_clear_composer_draft_action(),
            UiAction::AttachComposerPath(action) => self.handle_attach_composer_path_action(action),
            UiAction::MoveComposerCursor(action) => self.handle_move_composer_cursor_action(action),
            UiAction::EditComposer(action) => self.handle_edit_composer_action(action),
            UiAction::InsertComposerText(action) => self.handle_insert_composer_text_action(action),
            UiAction::CopyConversationSegment(action) => {
                self.handle_copy_conversation_segment_action(action)
            }
            UiAction::CopyLatestAssistantResponse(action) => {
                self.handle_copy_latest_assistant_response_action(action)
            }
        }
    }

    fn handle_replace_composer_draft_action(
        &mut self,
        action: ReplaceComposerDraftAction,
    ) -> UiActionOutcome {
        self.editor.set_text(&action.text);
        UiActionOutcome::accepted_message("composer draft replaced")
    }

    fn handle_clear_composer_draft_action(&mut self) -> UiActionOutcome {
        if self.editor.is_empty() {
            return UiActionOutcome::noop("composer draft already empty");
        }
        self.editor.clear_line();
        UiActionOutcome::accepted_message("composer draft cleared")
    }

    fn handle_attach_composer_path_action(
        &mut self,
        action: AttachComposerPathAction,
    ) -> UiActionOutcome {
        self.editor.insert_attachment(action.path.clone());
        UiActionOutcome::accepted_message(format!(
            "composer attachment inserted: {}",
            action.path.display()
        ))
    }

    fn handle_move_composer_cursor_action(
        &mut self,
        action: MoveComposerCursorAction,
    ) -> UiActionOutcome {
        match (action.direction, action.unit) {
            (ComposerCursorDirection::Backward, ComposerCursorUnit::Character) => {
                self.editor.move_left();
            }
            (ComposerCursorDirection::Forward, ComposerCursorUnit::Character) => {
                self.editor.move_right();
            }
            (ComposerCursorDirection::Backward, ComposerCursorUnit::Word) => {
                self.editor.move_word_backward();
            }
            (ComposerCursorDirection::Forward, ComposerCursorUnit::Word) => {
                self.editor.move_word_forward();
            }
            (ComposerCursorDirection::Home, ComposerCursorUnit::Line) => {
                self.editor.move_home();
            }
            (ComposerCursorDirection::End, ComposerCursorUnit::Line) => {
                self.editor.move_end();
            }
            _ => return UiActionOutcome::rejected("unsupported composer cursor movement"),
        }
        UiActionOutcome::accepted_message("composer cursor moved")
    }

    fn handle_edit_composer_action(&mut self, action: EditComposerAction) -> UiActionOutcome {
        match action.operation {
            ComposerEditOperation::DeleteBackward => self.editor.backspace(),
            ComposerEditOperation::DeleteWordBackward => self.editor.delete_word_backward(),
            ComposerEditOperation::DeleteWordForward => self.editor.delete_word_forward(),
            ComposerEditOperation::ClearLine => self.editor.clear_line(),
            ComposerEditOperation::KillToEnd => self.editor.kill_to_end(),
            ComposerEditOperation::InsertNewline => self.editor.insert_newline(),
        }
        self.exit_history_recall();
        UiActionOutcome::accepted_message("composer edited")
    }

    fn handle_insert_composer_text_action(
        &mut self,
        action: InsertComposerTextAction,
    ) -> UiActionOutcome {
        self.editor.insert_paste(&action.text);
        self.exit_history_recall();
        UiActionOutcome::accepted_message("composer text inserted")
    }

    fn handle_select_conversation_segment_action(
        &mut self,
        action: SelectConversationSegmentAction,
    ) -> UiActionOutcome {
        let idx = action.segment.index;
        let Some(segment) = self.conversation.segments().get(idx) else {
            return UiActionOutcome::rejected(format!(
                "conversation segment index out of range: {idx}"
            ));
        };
        if !segment.capabilities().selectable {
            return UiActionOutcome::rejected(format!(
                "conversation segment is not selectable: {idx}"
            ));
        }
        self.conversation.select_segment(idx);
        UiActionOutcome::accepted_message(format!("conversation segment selected: {idx}"))
    }

    fn handle_open_conversation_segment_detail_action(
        &mut self,
        action: OpenConversationSegmentDetailAction,
    ) -> UiActionOutcome {
        let idx = action.segment.index;
        let Some(segment) = self.conversation.segments().get(idx) else {
            return UiActionOutcome::rejected(format!(
                "conversation segment index out of range: {idx}"
            ));
        };
        if !segment.capabilities().detail_openable {
            return UiActionOutcome::rejected(format!(
                "conversation segment detail is not openable: {idx}"
            ));
        }
        self.conversation.toggle_timeline_expanded_segment(idx);
        UiActionOutcome::accepted_message(format!("conversation segment detail toggled: {idx}"))
    }

    fn segment_export_mode(mode: SegmentCopyMode) -> SegmentExportMode {
        match mode {
            SegmentCopyMode::Raw => SegmentExportMode::Raw,
            SegmentCopyMode::Plaintext => SegmentExportMode::Plaintext,
        }
    }

    fn segment_copy_mode(mode: SegmentExportMode) -> SegmentCopyMode {
        match mode {
            SegmentExportMode::Raw => SegmentCopyMode::Raw,
            SegmentExportMode::Plaintext => SegmentCopyMode::Plaintext,
        }
    }

    fn handle_copy_conversation_segment_action(
        &mut self,
        action: CopyConversationSegmentAction,
    ) -> UiActionOutcome {
        let idx = action.segment.index;
        let Some(segment) = self.conversation.segments().get(idx) else {
            return UiActionOutcome::rejected(format!(
                "conversation segment index out of range: {idx}"
            ));
        };
        let text = match Self::segment_export_mode(action.mode) {
            SegmentExportMode::Raw => segment
                .export_text(SegmentExportMode::Raw)
                .trim_end()
                .to_string(),
            SegmentExportMode::Plaintext => segment.human_plaintext_detail(),
        };
        if text.trim().is_empty() {
            return UiActionOutcome::rejected(format!(
                "conversation segment has no copyable text: {idx}"
            ));
        }
        if self.copy_text_to_clipboard(&text) {
            UiActionOutcome::accepted_message(format!("conversation segment copied: {idx}"))
        } else {
            UiActionOutcome::rejected(
                "clipboard unavailable — install pbcopy, wl-copy, xclip, or xsel",
            )
        }
    }

    fn handle_copy_latest_assistant_response_action(
        &mut self,
        action: CopyLatestAssistantResponseAction,
    ) -> UiActionOutcome {
        let mode = Self::segment_export_mode(action.mode);
        let Some(text) = self.conversation.latest_assistant_text_with_mode(mode) else {
            return UiActionOutcome::rejected("no assistant response to copy");
        };
        if self.copy_text_to_clipboard(&text) {
            UiActionOutcome::accepted_message("latest assistant response copied")
        } else {
            UiActionOutcome::rejected(
                "clipboard unavailable — select text in your terminal or install pbcopy/wl-copy/xclip",
            )
        }
    }

    fn handle_ui_preset_action(&mut self, action: SetUiPresetAction) -> UiActionOutcome {
        let name = action.surfaces.preset_name();
        self.apply_ui_preset(action.surfaces);
        UiActionOutcome::accepted_message(format!("UI → {name}"))
    }

    fn handle_surface_visible_action(
        &mut self,
        action: SetSurfaceVisibleAction,
    ) -> UiActionOutcome {
        self.toggle_ui_surface(action.surface, action.visible);
        UiActionOutcome::accepted_message(format!(
            "UI surface {}: {}",
            if action.visible {
                "enabled"
            } else {
                "disabled"
            },
            action.surface.label()
        ))
    }

    fn handle_permission_action(&mut self, action: PermissionAction) -> UiActionOutcome {
        if self.pending_permission.is_none() {
            return UiActionOutcome::noop("no pending permission request");
        }
        let context = self.pending_permission_context.take();
        self.command_prompt = None;
        if let Some(respond) = self.pending_permission.take()
            && let Ok(mut slot) = respond.lock()
            && let Some(tx) = slot.take()
        {
            let _ = tx.send(action.response);
        }
        let label = match action.response {
            omegon_traits::PermissionResponse::Allow => "allowed (this session)",
            omegon_traits::PermissionResponse::AlwaysAllow => {
                match context.as_ref().map(|ctx| ctx.persistence) {
                    Some(omegon_traits::PermissionPersistence::ProjectDirectory) => {
                        "always allowed - saved directory grant"
                    }
                    Some(omegon_traits::PermissionPersistence::SessionDirectory) => {
                        "always allowed - session directory grant"
                    }
                    _ => "allowed for this operation",
                }
            }
            omegon_traits::PermissionResponse::Deny => "denied",
        };
        let message = if let Some(context) = context {
            if matches!(
                action.response,
                omegon_traits::PermissionResponse::AlwaysAllow
            ) {
                if let Some(grant_path) = context.grant_path {
                    format!(
                        "→ {label}: {} {} (grant: {})",
                        context.tool_name, context.target, grant_path
                    )
                } else {
                    format!("→ {label}: {} {}", context.tool_name, context.target)
                }
            } else {
                format!("→ {label}: {} {}", context.tool_name, context.target)
            }
        } else {
            format!("→ {label}")
        };
        self.conversation.push_system(&message);
        UiActionOutcome::accepted_message(message)
    }

    fn handle_operator_wait_action(&mut self, action: OperatorWaitAction) -> UiActionOutcome {
        if self.pending_operator_wait.is_none() {
            return UiActionOutcome::noop("no pending operator wait request");
        }
        let context = self.pending_operator_wait_context.take();
        self.command_prompt = None;
        if let Some(respond) = self.pending_operator_wait.take()
            && let Ok(mut slot) = respond.lock()
            && let Some(tx) = slot.take()
        {
            let _ = tx.send(action.response);
        }
        let label = match action.response {
            omegon_traits::OperatorWaitResponse::Completed => "manual action completed",
            omegon_traits::OperatorWaitResponse::Cancelled => "manual action cancelled",
        };
        let message = if let Some(prompt) = context {
            format!("-> {label}: {prompt}")
        } else {
            format!("-> {label}")
        };
        self.conversation.push_system(&message);
        UiActionOutcome::accepted_message(message)
    }

    async fn submit_editor_buffer(&mut self, command_tx: &mpsc::Sender<TuiCommand>) {
        let (raw_text, attachments) = self.editor.take_submission();
        if raw_text.is_empty() && attachments.is_empty() {
            if self.awaiting_continuation && !self.agent_active {
                // Empty Enter while agent is awaiting confirmation — send continuation.
                self.pending_history_preload = None;
                let _ = self
                    .handle_ui_action(UiAction::SubmitContinuation, command_tx)
                    .await;
            } else if let Some(preloaded) = self.pending_history_preload.take() {
                self.editor.set_text(&preloaded);
            } else if !self.agent_active
                && let Some(last_prompt) = self.history.last().cloned()
            {
                self.pending_history_preload = Some(last_prompt);
            }
            return;
        }
        // User typed something — clear continuation and ghost-history state.
        self.awaiting_continuation = false;
        self.pending_history_preload = None;

        if let Ok(mut guard) = self.login_prompt_tx.try_lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(raw_text.clone());
            self.conversation.push_system(&format!("> {raw_text}"));
            return;
        }

        if raw_text.starts_with('/') {
            let _ = self
                .handle_ui_action(
                    UiAction::RunSlashCommand(SlashCommandAction {
                        raw: raw_text,
                        source: PromptSource::LocalTui,
                    }),
                    command_tx,
                )
                .await;
            return;
        }

        let _ = self
            .handle_ui_action(
                UiAction::SubmitPrompt(SubmitPromptAction {
                    text: raw_text,
                    attachments,
                    source: PromptSource::LocalTui,
                    queue_mode: self.queue_mode,
                    metadata: PromptMetadata::default(),
                }),
                command_tx,
            )
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
                    self.exit_history_recall();
                    let _ = command_tx
                        .send(TuiCommand::ShellHandoff {
                            keyboard_enhancement: self.keyboard_enhancement,
                        })
                        .await;
                    return;
                }

                self.history.push(raw_text.clone());
                self.exit_history_recall();
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
            self.history.push(raw_text.clone());
            self.history_idx = None;
            let _ = command_tx
                .send(TuiCommand::SubmitPrompt(PromptSubmission {
                    text: final_text,
                    image_paths: attachments,
                    submitted_by: "local-tui".to_string(),
                    via: "tui",
                    queue_mode: self.queue_mode,
                    metadata: PromptMetadata::default(),
                }))
                .await;
            if should_interrupt {
                self.prepare_interrupt_ui();
                let _ = command_tx
                    .send(TuiCommand::CancelActiveTurn {
                        submitted_by: "local-tui".to_string(),
                        via: "tui",
                    })
                    .await;
            }
            if let Some(ref mut overlay) = self.tutorial_overlay {
                overlay.check_any_input();
            }
            return;
        }

        self.history.push(raw_text.clone());
        self.exit_history_recall();
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
                metadata: PromptMetadata::default(),
            }))
            .await;
        if let Some(ref mut overlay) = self.tutorial_overlay {
            overlay.check_any_input();
        }
    }

    async fn submit_voice_prompt(
        &mut self,
        text: String,
        _event_id: String,
        command_tx: &mpsc::Sender<TuiCommand>,
    ) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let decorated = format!("🎙 {text}");
        if self.agent_active {
            let _ = command_tx
                .send(TuiCommand::SubmitPrompt(PromptSubmission {
                    text: decorated,
                    image_paths: Vec::new(),
                    submitted_by: "voice".to_string(),
                    via: "voice",
                    queue_mode: self.queue_mode,
                    metadata: PromptMetadata::default(),
                }))
                .await;
            return;
        }

        self.conversation.push_user(&decorated);
        self.history.push(decorated.clone());
        self.exit_history_recall();
        self.agent_active = true;
        if let Ok(mut ss) = self.dashboard_handles.session.lock() {
            ss.busy = true;
        }
        let _ = command_tx
            .send(TuiCommand::SubmitPrompt(PromptSubmission {
                text: decorated,
                image_paths: Vec::new(),
                submitted_by: "voice".to_string(),
                via: "voice",
                queue_mode: self.queue_mode,
                metadata: PromptMetadata::default(),
            }))
            .await;
    }

    fn suppress_editor_input_for(&mut self, duration: Duration) {
        self.suppress_editor_input_until = Some(std::time::Instant::now() + duration);
    }

    fn editor_input_suppressed(&mut self) -> bool {
        let suppressed = self.editor_input_suppressed_now();
        if !suppressed {
            self.suppress_editor_input_until = None;
        }
        suppressed
    }

    fn editor_input_suppressed_now(&self) -> bool {
        self.suppress_editor_input_until
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    fn should_discard_key_after_interrupt(&mut self, key: &KeyEvent) -> bool {
        if !self.editor_input_suppressed_now() {
            return false;
        }
        let is_interrupt_key = matches!(key.code, KeyCode::Esc)
            || matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
                && key.modifiers.contains(KeyModifiers::CONTROL);
        !is_interrupt_key
    }

    fn tail_chars(text: &str, max_chars: usize) -> &str {
        if text.chars().count() <= max_chars {
            return text;
        }
        let start = text
            .char_indices()
            .rev()
            .nth(max_chars.saturating_sub(1))
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        &text[start..]
    }

    fn prepare_interrupt_ui(&mut self) {
        self.editor.clear_line();
        self.interrupt_pending = true;
        self.slim_turn_state = SlimTurnState::Interrupting;
        self.suppress_editor_input_for(Duration::from_millis(1500));
    }

    /// Render the bottom footer surface and return the instrument-owned area
    /// that the later cleanup pass must not repaint.
    ///
    /// In compact mode, the engine row above the composer owns provider/model
    /// and context telemetry. When instruments are visible, this footer owns
    /// only live instrumentation panels: inference and tools. When instruments
    /// are hidden, it falls back to the compact engine panel so non-instrument
    /// layouts still expose provider/model state.
    fn render_bottom_footer(&self, area: Rect, frame: &mut Frame, t: &dyn theme::Theme) -> Rect {
        if !self.ui_surfaces.footer {
            return Rect::ZERO;
        }

        if !self.ui_surfaces.instruments {
            self.footer_data
                .render_engine_fallback_panel(area, frame, t);
            return area;
        }

        let footer_cols = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(area);

        self.instrument_panel
            .render_inference_panel(footer_cols[0], frame, t);
        frame.render_widget(
            Block::default().style(Style::default().bg(t.footer_bg())),
            footer_cols[1],
        );
        self.instrument_panel
            .render_tools_panel(footer_cols[2], frame, t);
        footer_cols[0].union(footer_cols[2])
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

        // ── Main surface layout ────────────────────────────────────
        let live_cleave = self
            .dashboard_handles
            .cleave
            .as_ref()
            .and_then(|cp_lock| cp_lock.lock().ok().map(|cp| cp.clone()))
            .filter(|cp| cp.active)
            .or_else(|| self.dashboard.cleave.clone().filter(|cp| cp.active));
        let live_delegate = self
            .dashboard_handles
            .delegate
            .as_ref()
            .and_then(|dp_lock| dp_lock.lock().ok().map(|dp| dp.clone()))
            .filter(|dp| dp.active || dp.running > 0)
            .or_else(|| {
                self.dashboard
                    .delegate
                    .clone()
                    .filter(|dp| dp.active || dp.running > 0)
            });
        let dashboard_has_content = self.dashboard.status_counts.total > 0
            || self.dashboard.focused_node.is_some()
            || !self.dashboard.active_changes.is_empty()
            || live_cleave.is_some()
            || live_delegate.is_some();
        let editor_height = editor_height_for(&self.editor, area);
        let editor_info_height =
            u16::from(runtime_queue_depth(self.runtime_queue_snapshot.as_ref()) > 0);
        let workbench_state = WorkbenchState {
            active: active_workbench_snapshot(self.workbench_state.active.as_ref(), None),
            workstreams: self.workbench_state.workstreams.clone(),
            workspace: self.current_workbench_workspace_context(),
        };
        self.prune_activity_tools(std::time::Instant::now());
        let mut live_activity_tools = self
            .activity_tools
            .iter()
            .filter(|tool| {
                self.conversation
                    .tool_segment_by_id(&tool.segment_id)
                    .is_some()
            })
            .map(ActivityToolState::projection)
            .collect::<Vec<_>>();
        if let Some(ToolInspectionTarget::Pinned(id)) = self.tool_inspection_target.as_ref()
            && let Some(segment) = self.conversation.tool_segment_by_id(id)
            && !live_activity_tools
                .iter()
                .any(|tool| tool.segment_id == *id)
        {
            let (name, args_summary, result_summary) = match &segment.content {
                SegmentContent::ToolCard {
                    name,
                    args_summary,
                    result_summary,
                    ..
                } => (name.clone(), args_summary.clone(), result_summary.clone()),
                _ => ("tool".to_string(), None, None),
            };
            live_activity_tools.push(crate::surfaces::activity::ActivityToolProjection {
                segment_id: id.clone(),
                mode: crate::surfaces::activity::ActivityToolMode::Detail,
                status: crate::surfaces::activity::ActivityToolStatus::Complete,
                name,
                args_summary,
                result_summary,
            });
        }
        let activity_projection = if self.ui_surfaces.activity && self.ui_surfaces.is_compact() {
            crate::surfaces::activity::ActivitySurfaceProjection::from_parts(
                live_activity_tools,
                live_cleave.as_ref(),
                live_delegate.as_ref(),
            )
        } else {
            crate::surfaces::activity::ActivitySurfaceProjection {
                entries: Vec::new(),
            }
        };
        let engine_status_height =
            u16::from(self.ui_surfaces.activity && self.ui_surfaces.is_compact());
        let raw_tool_inspection_height =
            activity_preferred_height(&activity_projection, area.width)
                .saturating_add(engine_status_height);
        let raw_workbench_height = workbench_preferred_height(&workbench_state, area.width);
        self.session_row.sync_from_footer(&self.footer_data);
        let session_height = self.session_row.preferred_height_for(area.width);
        let layout_plan = plan_tui_layout(TuiLayoutInputs {
            area,
            surfaces: self.ui_surfaces,
            dashboard_has_content,
            editor_height,
            editor_info_height,
            instrument_footer_height: self.instrument_panel.preferred_height(),
            session_height,
            pending_permission: false,
            tool_inspection_height: raw_tool_inspection_height,
            workbench_height: raw_workbench_height,
            segment_detail_height: 0,
        });

        let show_dashboard = layout_plan.show_dashboard;
        let main_area = layout_plan.main_area;
        let conversation_area = layout_plan.conversation_area;
        let tool_inspection_area = layout_plan.tool_inspection_area;
        let workbench_area = layout_plan.workbench_area;
        let _segment_detail_area = layout_plan.segment_detail_area;
        let editor_area = layout_plan.editor_area;
        let editor_info_area = layout_plan.editor_info_area;
        let session_area = layout_plan.session_area;
        let footer_area = layout_plan.footer_area;
        let dash_area = if show_dashboard {
            Rect::new(
                layout_plan.main_area.x,
                footer_area.y.saturating_sub(1),
                layout_plan.main_area.width,
                1,
            )
        } else {
            Rect::ZERO
        };

        // Render tab bar + conversation/widget content
        let t = &self.theme;
        if editor_info_area.height > 0 {
            render_runtime_queue_info_line(
                editor_info_area,
                frame,
                t.as_ref(),
                self.runtime_queue_snapshot.as_ref(),
            );
        }
        let has_multiple_tabs = self.conversation.tabs.tabs.len() > 1;
        let show_tab_bar = has_multiple_tabs
            && !(self.ui_surfaces.is_compact()
                && !self.ui_surfaces.dashboard
                && !self.ui_surfaces.footer);

        let content_area = if show_tab_bar {
            // Split conversation area into tab bar + content
            let conv_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(conversation_area);
            tab_bar::render_tab_bar(
                frame,
                conv_chunks[0],
                self.theme.as_ref(),
                &self.conversation.tabs.tabs,
                self.conversation.tabs.active_tab,
            );
            conv_chunks[1]
        } else {
            conversation_area
        };

        // Render content based on active tab
        if self.conversation.tabs.is_conversation_active() {
            // Render conversation widget (can mutate conv_state via frame.render_stateful_widget)
            let density = self.settings().tool_detail;
            let pinned_segment = self.conversation.timeline_expanded_segment();
            let selected_segment = self.conversation.selected_segment_index();
            let (segments, conv_state, image_cache) =
                self.conversation.segments_state_and_image_cache();
            let conv_widget = conv_widget::ConversationWidget::new(segments, t.as_ref())
                .with_mode(if self.ui_surfaces.is_compact() {
                    SegmentRenderMode::Slim
                } else {
                    SegmentRenderMode::Full
                })
                .with_density(density)
                .with_pinned_segment(pinned_segment)
                .with_selected_segment(selected_segment)
                .with_detail_hint_enabled(false);
            frame.render_stateful_widget(conv_widget, content_area, conv_state);
            for (segment_idx, image_area) in conv_state.visible_image_areas(segments, content_area)
            {
                let Some(SegmentContent::Image { path, .. }) =
                    segments.get(segment_idx).map(|segment| &segment.content)
                else {
                    continue;
                };
                if let Some(protocol) = image_cache.get_or_create(segment_idx, path) {
                    image::render_image(image_area, frame, protocol);
                }
            }
        } else {
            // Render extension widget with schema-aware formatting
            if let Tab::Extension { widget_id, .. } = self.conversation.tabs.active()
                && let Some(widget) = self.extension_widgets.get(widget_id)
            {
                widget_renderer::render_widget(
                    frame,
                    content_area,
                    &widget.renderer,
                    &widget.current_data,
                    &widget.label,
                );
            }
        }

        self.conversation_area = Some(conversation_area);
        self.editor_area = Some(editor_area);
        self.workbench_area = (workbench_area.height > 0).then_some(workbench_area);

        if tool_inspection_area.height > 0 {
            let (status_area, activity_area) = if engine_status_height > 0 {
                (
                    Rect::new(
                        tool_inspection_area.x,
                        tool_inspection_area.y,
                        tool_inspection_area.width,
                        1,
                    ),
                    Rect::new(
                        tool_inspection_area.x,
                        tool_inspection_area.y.saturating_add(1),
                        tool_inspection_area.width,
                        tool_inspection_area.height.saturating_sub(1),
                    ),
                )
            } else {
                (Rect::ZERO, tool_inspection_area)
            };
            if status_area.height > 0 {
                self.render_engine_status_row(status_area, frame, self.theme.as_ref());
            }
            render_activity_panel(
                activity_area,
                frame,
                self.theme.as_ref(),
                &self.conversation,
                &activity_projection,
            );
        }

        if (workbench_state.active.is_some()
            || !workbench_state.workstreams.is_empty()
            || workbench_state.workspace.has_visible_context())
            && workbench_area.height > 0
        {
            render_workbench_panel(workbench_area, frame, self.theme.as_ref(), &workbench_state);
        }

        // ── Sync footer data from settings (every frame) ────
        {
            let s = self.settings();
            self.footer_data.model_id = s.model.clone();
            self.footer_data.model_provider = s.provider().to_string();
            self.footer_data.context_class = s.effective_requested_class();
            self.footer_data.actual_context_class = s.context_class;
            self.footer_data.context_window = s.context_window;
            self.footer_data.thinking_level = s.thinking.as_str().to_string();
            self.footer_data.posture = s.posture.effective.display_name().to_string();
            self.footer_data.runtime_brand = if self.ui_surfaces.is_compact() {
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
            self.footer_data.sandbox = s.sandbox;
            self.footer_data.is_oauth = s.provider_is_oauth;
        }
        {
            self.footer_data.model_tier = Self::displayed_model_grade(
                &self.footer_data.model_provider,
                &self.footer_data.model_id,
                &self.footer_data.harness.capability_grade,
            );
        }
        self.footer_data.turn = self.turn;
        self.footer_data.tool_calls = self.tool_calls;
        self.footer_data.compactions = self.dashboard.compactions;

        // ── Session row (slim mode only, below workbench) ───────
        if session_area.height > 0 {
            self.session_row.viewport_hint = if self.conversation.conv_state.scroll_offset > 0 {
                Some(format!(
                    "view detached ↑{} · End tail",
                    self.conversation.conv_state.scroll_offset
                ))
            } else {
                None
            };
            self.session_row.turn_state = Some(self.slim_turn_state.label());
            let plan_state = workbench_state
                .active
                .as_ref()
                .map(|snapshot| {
                    snapshot.hint_state(
                        workbench_area
                            .height
                            .saturating_sub(active_plan_workspace_context_height(&workbench_state)),
                    )
                })
                .unwrap_or_else(|| {
                    if slim_completed_plan_hint_available(self.completed_plan_history_available) {
                        SlimPlanHintState::Complete
                    } else {
                        SlimPlanHintState::None
                    }
                });
            let plan_context = SlimPlanContext::from_dashboard(
                workbench_state.active.is_some(),
                &self.dashboard.active_changes,
                self.dashboard.focused_node.as_ref(),
            );
            self.session_row.operator_hint = Some(slim_operator_hint(
                self.pending_permission.is_some(),
                self.pending_operator_wait.is_some(),
                self.terminal_copy_mode,
                plan_state,
                &plan_context,
            ));
            self.session_row
                .render(session_area, frame, self.theme.as_ref());
        }

        // Project dashboard strip (above footer/tooling/instruments)
        if show_dashboard && dash_area.width > 0 {
            self.dashboard_area = Some(dash_area);
            self.dashboard.render_themed(dash_area, frame, t.as_ref());
        } else {
            self.dashboard_area = None;
        }

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
                thinking,
                mem_op,
                self.agent_active,
                dt,
            );

            // Push live cleave progress into the instrument panel each render tick
            // so the tools→cleave swap happens without turn-boundary latency.
            if let Some(ref cp_lock) = self.dashboard_handles.cleave
                && let Ok(cp) = cp_lock.lock()
            {
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

        let inst_area = self.render_bottom_footer(footer_area, frame, t.as_ref());

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
            let editor_block = if self.ui_surfaces.is_compact() {
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
            frame.render_widget(editor_widget, editor_area);
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
            frame.render_widget(editor_widget, editor_area);
        } else {
            let hint_text = if self.agent_active {
                String::new()
            } else if self.editor.is_empty() {
                if self.ui_surfaces.dashboard {
                    "⏎ send  ⇧⏎/⌥⏎ newline  ^O/Tab details  ^D tree  / commands ".into()
                } else {
                    "⏎ send  ⇧⏎/⌥⏎ newline  ^O/Tab details  /ui surfaces  / commands ".into()
                }
            } else {
                "⏎ send  ⇧⏎/⌥⏎ newline  ⌥↑/⌥↓ history ".into()
            };
            let model_id = self.footer_data.model_id.as_str();
            let model_short = model_id
                .split(':')
                .next_back()
                .unwrap_or(model_id)
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("-");
            let provider_label = self
                .footer_data
                .model_provider
                .trim()
                .split(':')
                .next()
                .unwrap_or("");
            let provider_label = if provider_label.is_empty() {
                model_id
                    .split_once(':')
                    .map(|(provider, _)| provider)
                    .unwrap_or("provider?")
            } else {
                provider_label
            };
            let route_label = format!("{provider_label}/{model_short}");
            let editor_title = {
                use crate::tui::glyphs::EngineGlyphRole;
                let glyphs = crate::tui::glyphs::glyphs();
                let is_local_provider = matches!(provider_label, "ollama" | "llama.cpp" | "local");
                let provider_glyph = if is_local_provider {
                    glyphs.engine(EngineGlyphRole::ProviderLocal)
                } else {
                    glyphs.engine(EngineGlyphRole::ProviderCloud)
                };
                let route_glyph = glyphs.engine(EngineGlyphRole::Route);
                let title_budget = editor_area.width.saturating_sub(2) as usize;
                let grade = self.footer_data.model_tier.trim();
                let grade_text = if grade.is_empty() {
                    glyphs.engine(EngineGlyphRole::GradeEmblem).to_string()
                } else {
                    format!("{} {grade}", glyphs.engine(EngineGlyphRole::GradeEmblem))
                };
                let settings_snapshot = self.settings();
                let profile_source = settings_snapshot.profile_source;
                let profile_name = settings_snapshot.profile_name.clone();
                let source_label = match profile_source {
                    crate::settings::ProfileSource::Project(_) => "project",
                    crate::settings::ProfileSource::User(_) => "user",
                    crate::settings::ProfileSource::BuiltInDefault => "default",
                };
                let profile_text = profile_name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| source_label.to_string());
                let thinking_text = format!(
                    "{} {}",
                    glyphs.engine(EngineGlyphRole::Thinking),
                    self.footer_data.thinking_level
                );
                let context_text = format!(
                    "{} {}",
                    glyphs.engine(EngineGlyphRole::Context),
                    Self::editor_context_widget(
                        self.footer_data.actual_context_class,
                        self.footer_data.context_window,
                        self.footer_data.estimated_tokens,
                        self.footer_data.context_percent,
                    )
                );
                let route_bg = t.accent_muted();
                let grade_bg = t.accent();
                let thinking_bg = t.card_bg();
                let context_bg = t.surface_bg();
                let mut title_spans = vec![Span::styled(
                    " ",
                    Style::default().fg(t.border_dim()).bg(t.surface_bg()),
                )];
                title_spans.push(Span::styled(
                    format!(
                        " {} {provider_glyph} {route_label} ",
                        glyphs.engine(EngineGlyphRole::RibbonMark),
                    ),
                    Style::default()
                        .fg(t.bg())
                        .bg(route_bg)
                        .add_modifier(Modifier::BOLD),
                ));
                let push_segment = |spans: &mut Vec<Span<'static>>,
                                    text: String,
                                    style: Style,
                                    previous_bg: Color,
                                    segment_bg: Color| {
                    spans.push(Span::styled(
                        route_glyph,
                        Style::default().fg(previous_bg).bg(segment_bg),
                    ));
                    spans.push(Span::styled(format!(" {text} "), style.bg(segment_bg)));
                };
                let tail_fields = [
                    (
                        grade_text,
                        Style::default().fg(t.bg()).add_modifier(Modifier::BOLD),
                        grade_bg,
                    ),
                    (
                        profile_text,
                        Style::default().fg(t.fg()).add_modifier(Modifier::BOLD),
                        thinking_bg,
                    ),
                    (
                        thinking_text,
                        Style::default().fg(t.accent_bright()),
                        thinking_bg,
                    ),
                    (context_text, Style::default().fg(t.fg()), context_bg),
                ];
                let mut previous_bg = route_bg;
                for (text, style, segment_bg) in tail_fields {
                    let mut candidate = title_spans.clone();
                    push_segment(&mut candidate, text, style, previous_bg, segment_bg);
                    let candidate_width = candidate.iter().map(|span| span.width()).sum::<usize>()
                        + Span::raw(route_glyph).width();
                    if candidate_width <= title_budget {
                        title_spans = candidate;
                        previous_bg = segment_bg;
                    }
                }
                title_spans.push(Span::styled(
                    route_glyph,
                    Style::default().fg(previous_bg).bg(t.surface_bg()),
                ));
                Line::from(title_spans)
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

            let editor_rect = editor_area;
            // Pre-split using char-boundary wrapping (same algorithm as
            // cursor_screen_position) so the terminal cursor always lands on
            // the correct visual cell.  Paragraph::wrap uses word boundaries
            // which diverge from cursor math and compound across rows.
            // Normal editor mode uses Borders::TOP only: content spans the
            // full width and starts one row below the top border.
            let content_width = editor_rect.width.max(1);
            let visible_rows = editor_rect.height.saturating_sub(1).max(1);
            let visual_lines: Vec<Line<'static>> = if self.editor.is_empty() {
                if let Some(preloaded) = self.pending_history_preload.as_ref() {
                    let preview = preloaded.lines().next().unwrap_or("");
                    let suffix = if preloaded.lines().count() > 1 {
                        " …"
                    } else {
                        ""
                    };
                    vec![Line::from(vec![
                        Span::styled("history preload: ", Style::default().fg(t.border_dim())),
                        Span::styled(
                            format!("{preview}{suffix}"),
                            Style::default().fg(t.dim()).add_modifier(Modifier::ITALIC),
                        ),
                    ])]
                } else {
                    vec![Line::from(Span::styled(
                        "Ask anything, or type / for commands",
                        Style::default().fg(t.dim()),
                    ))]
                }
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
            if !self.editor_input_suppressed_now() {
                let (cx, cy) = self.editor.cursor_screen_position(editor_rect);
                frame.set_cursor_position(ratatui::layout::Position { x: cx, y: cy });
            }
        }

        // Command palette popup (above editor when typing /). Keep this visible
        // during active turns: queued steering prompts still use the same editor,
        // and hiding autocomplete made the command surface feel locked even
        // though key input was still being accepted.
        let matches = if self.at_picker.is_some() || self.editor_input_suppressed_now() {
            vec![]
        } else {
            self.matching_commands()
        };
        if !matches.is_empty() {
            let palette_height = matches.len().min(8) as u16 + 2; // +2 for borders
            let _editor_area_inner = editor_area;
            let palette_area = Rect {
                x: editor_area.x,
                y: editor_area.y.saturating_sub(palette_height),
                width: editor_area.width.min(76),
                height: palette_height,
            };

            let items: Vec<Line<'static>> = matches
                .iter()
                .map(|row| {
                    let badges = if row.badges.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", row.badges.join(" · "))
                    };
                    let metadata = if row.metadata.is_empty() {
                        String::new()
                    } else {
                        format!("  — {}", row.metadata.join(" · "))
                    };
                    Line::from(vec![
                        Span::styled(format!(" {}", row.command), t.style_accent()),
                        Span::styled(format!("  {}", row.description), t.style_muted()),
                        Span::styled(metadata, t.style_dim()),
                        Span::styled(badges, t.style_dim()),
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

        if let Some(ref picker) = self.at_picker {
            picker.render(area, frame, t.as_ref());
        }

        if let Some(menu) = &self.active_menu {
            menu_surface::render_menu_surface(
                frame,
                area,
                self.theme.as_ref(),
                &menu.projection,
                &menu.state,
            );
        }

        // Selector popup (overlays everything when active)
        if let Some(ref sel) = self.selector {
            sel.render(area, frame, t.as_ref());
        }

        // ── Post-render effects (tachyonfx) — each zone processed separately ──
        self.effects.process(
            frame.buffer_mut(),
            conversation_area,
            footer_area,
            editor_area,
        );

        // ── Tutorial overlay — rendered on top of everything except toasts ──
        if let Some(ref overlay) = self.tutorial_overlay {
            let footer_h = footer_area.height;
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
        // Normalize unowned/default background leakage without erasing
        // intentional theme-backed badges or panels. This pass started as a
        // guard against Color::Reset bleed-through from widgets/temp buffers;
        // keep that fence, but make the allow-list semantic instead of a stale
        // hand-picked subset of theme colors.
        {
            let base = self.theme.surface_bg();
            let intentional_backgrounds = self.theme.intentional_backgrounds();
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
                    if cell.bg == Color::Reset || !intentional_backgrounds.contains(&cell.bg) {
                        cell.set_bg(base);
                    }
                }
            }
        }

        // Render command panel above the main surfaces and below blocking prompts/modals.
        if let Some(panel) = &self.command_panel {
            command_surfaces::render_panel(area, frame.buffer_mut(), self.theme.as_ref(), panel);
        }

        // Render responder-backed blocking prompts above passive command panels.
        if let Some(prompt) = &self.command_prompt {
            command_surfaces::render_prompt(area, frame.buffer_mut(), self.theme.as_ref(), prompt);
        }

        // Render first-class copy text surface above command prompts/panels.
        if self.copy_text_modal.is_some() {
            self.render_copy_text_modal(frame);
        }

        // Render operator toast above normal TUI surfaces and copy text surfaces, but below
        // blocking extension overlays/prompts so confirmations never obscure required choices.
        self.render_operator_event_toast(frame);

        // Render modal overlay if active
        if let Some((widget_id, data, auto_dismiss_ms, spawn_time)) = &self.active_modal {
            // Check if modal should auto-dismiss
            if let Some(dismiss_ms) = auto_dismiss_ms {
                if spawn_time.elapsed().as_millis() > *dismiss_ms as u128 {
                    self.active_modal = None;
                } else {
                    extension_overlays::render_modal(frame, self.theme.as_ref(), widget_id, data);
                }
            } else {
                extension_overlays::render_modal(frame, self.theme.as_ref(), widget_id, data);
            }
        }
        // Render action prompt if active
        if let Some((widget_id, actions)) = &self.active_action_prompt {
            extension_overlays::render_action_prompt(
                frame,
                self.theme.as_ref(),
                widget_id,
                actions,
            );
        }
    }

    fn render_operator_event_toast(&self, frame: &mut Frame<'_>) {
        let Some(event) = self.operator_events.back() else {
            return;
        };
        let area = frame.area();
        if area.width < 24 || area.height < 6 {
            return;
        }

        let text = format!("{} {}", event.icon, event.message);
        let text_width = text.chars().count() as u16;
        let toast_width = text_width
            .saturating_add(4)
            .clamp(24, area.width.saturating_sub(4).max(24));
        let toast_height = 3;
        let x = area.x + area.width.saturating_sub(toast_width) / 2;
        let y = area.y + area.height.saturating_sub(toast_height + 3);
        let toast_area = Rect::new(x, y, toast_width, toast_height);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(
                Style::default()
                    .fg(event.color)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().bg(self.theme.card_bg()))
            .title(Span::styled(" action ", Style::default().fg(event.color)));
        let paragraph = Paragraph::new(Line::from(Span::styled(
            text,
            Style::default()
                .fg(self.theme.fg())
                .bg(self.theme.card_bg())
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center)
        .block(block);

        frame.render_widget(Clear, toast_area);
        frame.render_widget(paragraph, toast_area);
    }

    fn expand_workbench_plan_details(&mut self) -> bool {
        let Some(snapshot) = self.workbench_state.active.as_ref().cloned() else {
            return false;
        };
        self.conversation
            .push_system(&snapshot.system_notification_text("Plan details"));
        self.conversation.snap_to_bottom();
        self.show_toast("Expanded plan details", ratatui_toaster::ToastType::Success);
        self.effects.pulse_conversation_action();
        true
    }

    fn close_copy_text_modal(&mut self) {
        self.copy_text_modal = None;
        self.copy_text_copy_button_area = None;
        if self.terminal_copy_mode {
            self.set_terminal_copy_mode(false);
        }
    }

    fn copy_all_from_copy_text_modal(&mut self) -> bool {
        let Some(text) = self
            .copy_text_modal
            .as_ref()
            .map(|modal| modal.text.clone())
        else {
            return false;
        };
        if self.copy_text_to_clipboard(&text) {
            self.show_toast("Copied all text", ratatui_toaster::ToastType::Success);
            true
        } else {
            self.show_toast(
                "Clipboard unavailable — terminal selection still available",
                ratatui_toaster::ToastType::Warning,
            );
            false
        }
    }

    fn render_copy_text_modal(&mut self, frame: &mut Frame<'_>) {
        let Some(modal) = &mut self.copy_text_modal else {
            return;
        };
        let area = frame.area();
        let modal_width = ((area.width as f32 * 0.9) as u16).max(20).min(area.width);
        let modal_height = ((area.height as f32 * 0.85) as u16).max(8).min(area.height);
        let x = (area.width.saturating_sub(modal_width)) / 2;
        let y = (area.height.saturating_sub(modal_height)) / 2;
        let modal_area = Rect {
            x,
            y,
            width: modal_width,
            height: modal_height,
        };
        let button_label = " Copy all ";
        let button_width = button_label.len() as u16;
        self.copy_text_copy_button_area = Some(Rect {
            x: modal_area
                .x
                .saturating_add(modal_area.width.saturating_sub(button_width + 2)),
            y: modal_area.y,
            width: button_width,
            height: 1,
        });
        let inner_height = modal_area.height.saturating_sub(2);
        let body_height = inner_height.saturating_sub(1);
        let max_scroll = modal
            .text
            .lines()
            .count()
            .saturating_sub(body_height as usize) as u16;
        modal.scroll_y = modal.scroll_y.min(max_scroll);

        frame.render_widget(&Clear, modal_area);

        let modal_bg = self.theme.card_bg();
        let block = Block::default()
            .title(format!(" {} ", modal.title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan).bg(modal_bg))
            .style(Style::default().bg(modal_bg));
        let inner = block.inner(modal_area);
        frame.render_widget(block, modal_area);
        if let Some(button_area) = self.copy_text_copy_button_area {
            frame.render_widget(
                Paragraph::new(button_label).style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                button_area,
            );
        }

        let body_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: body_height,
        };
        let footer_area = Rect {
            x: inner.x,
            y: inner.y.saturating_add(body_height),
            width: inner.width,
            height: inner.height.saturating_sub(body_height),
        };

        let mut paragraph = Paragraph::new(modal.text.as_str())
            .style(Style::default().bg(modal_bg))
            .scroll((modal.scroll_y, 0));
        if modal.wrap {
            paragraph = paragraph.wrap(ratatui::widgets::Wrap { trim: false });
        }
        frame.render_widget(paragraph, body_area);

        let footer = format!(
            "Esc close · ↑/↓/PgUp/PgDn scroll · terminal drag selects text · lines {}-{} of {}",
            modal.scroll_y.saturating_add(1),
            modal
                .scroll_y
                .saturating_add(body_height)
                .min(modal.text.lines().count() as u16),
            modal.text.lines().count()
        );
        frame.render_widget(
            Paragraph::new(footer).style(Style::default().fg(Color::DarkGray).bg(modal_bg)),
            footer_area,
        );
    }

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
        let Some(idx) = self.conversation.selected_or_focused_segment() else {
            self.show_toast(
                "Nothing selected to copy",
                ratatui_toaster::ToastType::Warning,
            );
            return;
        };
        let outcome = self.handle_copy_conversation_segment_action(CopyConversationSegmentAction {
            segment: ConversationSegmentRef::by_index(idx),
            mode: Self::segment_copy_mode(mode),
        });
        match outcome {
            UiActionOutcome::Accepted { .. } => {
                let label = match mode {
                    SegmentExportMode::Raw => "Copied selected conversation segment",
                    SegmentExportMode::Plaintext => {
                        "Copied selected conversation segment as plaintext"
                    }
                };
                self.show_toast(label, ratatui_toaster::ToastType::Success);
                self.effects.ping_footer(self.theme.as_ref());
                self.effects.pulse_conversation_action();
            }
            UiActionOutcome::Rejected { reason }
            | UiActionOutcome::Noop { reason }
            | UiActionOutcome::Deferred { reason } => {
                self.show_toast(&reason, ratatui_toaster::ToastType::Warning);
            }
        }
    }

    fn copy_selected_conversation_segment(&mut self) {
        self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Raw);
    }

    fn copy_latest_assistant_response(&mut self, mode: SegmentExportMode) {
        let outcome =
            self.handle_copy_latest_assistant_response_action(CopyLatestAssistantResponseAction {
                mode: Self::segment_copy_mode(mode),
            });
        match outcome {
            UiActionOutcome::Accepted { .. } => {
                let label = match mode {
                    SegmentExportMode::Raw => "Copied latest assistant response",
                    SegmentExportMode::Plaintext => "Copied latest assistant response as plaintext",
                };
                self.show_toast(label, ratatui_toaster::ToastType::Success);
                self.effects.ping_footer(self.theme.as_ref());
                self.effects.pulse_conversation_action();
            }
            UiActionOutcome::Rejected { reason }
            | UiActionOutcome::Noop { reason }
            | UiActionOutcome::Deferred { reason } => {
                self.show_toast(&reason, ratatui_toaster::ToastType::Warning);
            }
        }
    }

    fn build_session_transcript(&self, mode: SegmentExportMode) -> String {
        let segments = self.conversation.segments();
        let mut parts: Vec<String> = Vec::new();
        if let Some(plan) = self.conversation.latest_plan_progress() {
            parts.push(format!("## Plan\n\n{}", plan.trim_end()));
        }
        for segment in segments {
            if matches!(segment.content, SegmentContent::TurnSeparator) {
                continue;
            }
            if let SegmentContent::SystemNotification { text } = &segment.content
                && segments::is_plan_progress_text(text)
            {
                continue;
            }
            let role = match segment.role() {
                crate::surfaces::conversation::SegmentRole::Operator => "## Operator",
                crate::surfaces::conversation::SegmentRole::Assistant => "## Assistant",
                crate::surfaces::conversation::SegmentRole::PeerAgent => "## Peer Agent",
                crate::surfaces::conversation::SegmentRole::Tool => "## Tool",
                crate::surfaces::conversation::SegmentRole::System => "## System",
                crate::surfaces::conversation::SegmentRole::Lifecycle => "## Event",
                crate::surfaces::conversation::SegmentRole::Media => "## Media",
                crate::surfaces::conversation::SegmentRole::Separator => continue,
            };
            let text = segment.export_text(mode);
            if !text.trim().is_empty() {
                parts.push(format!("{role}\n\n{text}"));
            }
        }
        parts.join("\n\n---\n\n")
    }

    fn restore_tui_after_native_scrollback(
        out: &mut io::Stdout,
        keyboard_enhancement: bool,
        mouse_capture: bool,
    ) -> std::io::Result<()> {
        out.execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        if keyboard_enhancement {
            let _ = out.execute(PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            ));
        }
        if mouse_capture {
            let _ = out.execute(EnableMouseCapture);
        }
        Ok(())
    }

    fn write_session_transcript_markdown_to_dir(
        &self,
        dir: &std::path::Path,
    ) -> std::io::Result<std::path::PathBuf> {
        let transcript = self.build_session_transcript(SegmentExportMode::Raw);
        if transcript.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "empty transcript",
            ));
        }

        std::fs::create_dir_all(dir)?;
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
        let path = dir.join(format!("omegon-transcript-{timestamp}.md"));
        let generated_at = chrono::Local::now().to_rfc3339();
        let body = format!("# Omegon transcript\n\nGenerated: {generated_at}\n\n{transcript}\n");
        std::fs::write(&path, body)?;
        Ok(path)
    }

    fn write_session_transcript_markdown(&self) -> std::io::Result<std::path::PathBuf> {
        let cwd = std::env::current_dir()?;
        let project_root = crate::setup::find_project_root(&cwd);
        self.write_session_transcript_markdown_to_dir(
            &project_root.join(".omegon").join("transcripts"),
        )
    }

    fn export_session_transcript_markdown(&mut self) {
        match self.write_session_transcript_markdown() {
            Ok(path) => {
                self.conversation.push_system(&format!(
                    "✓ Transcript written\n  {}\n  Open the linked .md file from your terminal.",
                    path.display()
                ));
                self.show_toast(
                    "Transcript written to Markdown",
                    ratatui_toaster::ToastType::Success,
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::InvalidInput => {
                self.show_toast(
                    "No conversation transcript to write",
                    ratatui_toaster::ToastType::Warning,
                );
            }
            Err(err) => {
                self.show_toast(
                    &format!("Could not write transcript: {err}"),
                    ratatui_toaster::ToastType::Warning,
                );
            }
        }
    }

    fn copy_full_session(&mut self) {
        let full = self.build_session_transcript(SegmentExportMode::Raw);
        if full.is_empty() {
            self.show_toast(
                "No conversation to copy",
                ratatui_toaster::ToastType::Warning,
            );
            return;
        }
        let byte_size = full.len();
        let size_label = if byte_size > 1_048_576 {
            format!("{:.1}MB", byte_size as f64 / 1_048_576.0)
        } else if byte_size > 1024 {
            format!("{}KB", byte_size / 1024)
        } else {
            format!("{}B", byte_size)
        };

        if byte_size > 5_000_000 {
            self.show_toast(
                &format!(
                    "Session too large for clipboard ({size_label}). Use /export to save to file."
                ),
                ratatui_toaster::ToastType::Warning,
            );
            return;
        }

        if self.copy_text_to_clipboard(&full) {
            let segment_count = full.split("\n\n---\n\n").count();
            self.show_toast(
                &format!("Copied full session ({segment_count} segments, {size_label})"),
                ratatui_toaster::ToastType::Success,
            );
        } else {
            self.show_toast("Clipboard unavailable", ratatui_toaster::ToastType::Warning);
        }
    }

    fn print_transcript_to_native_scrollback(&mut self) {
        let transcript = self.build_session_transcript(SegmentExportMode::Raw);
        if transcript.trim().is_empty() {
            self.show_toast(
                "No conversation transcript to print",
                ratatui_toaster::ToastType::Warning,
            );
            return;
        }

        let mouse_capture = self.mouse_capture_enabled;
        let keyboard_enhancement = self.keyboard_enhancement;
        let result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut out = io::stdout();
            let _ = disable_raw_mode();
            let _ = out.execute(DisableMouseCapture);
            if keyboard_enhancement {
                let _ = out.execute(PopKeyboardEnhancementFlags);
            }
            out.execute(LeaveAlternateScreen)?;
            writeln!(out)?;
            writeln!(out, "----- Omegon transcript -----")?;
            writeln!(out, "{transcript}")?;
            writeln!(out, "----- End Omegon transcript -----")?;
            writeln!(out)?;
            out.flush()?;
            Self::restore_tui_after_native_scrollback(&mut out, keyboard_enhancement, mouse_capture)
        })();

        if result.is_ok() {
            self.show_toast(
                "Transcript printed to native scrollback",
                ratatui_toaster::ToastType::Success,
            );
        } else {
            let mut out = io::stdout();
            let _ = Self::restore_tui_after_native_scrollback(
                &mut out,
                keyboard_enhancement,
                mouse_capture,
            );
            self.show_toast(
                "Could not print transcript to native scrollback",
                ratatui_toaster::ToastType::Warning,
            );
        }
    }

    fn show_slash_response(&mut self, command: &str, response: &str) {
        if response.starts_with("Unknown command: /") {
            self.show_command_toast(CommandToast::new(response, CommandSeverity::Warning));
        } else if should_toast_slash_response(response) {
            self.show_command_toast(CommandToast::new(response, CommandSeverity::Info));
        } else if should_modal_slash_response(response) {
            self.open_command_panel(CommandPanel::from_slash(command, response));
        } else {
            self.conversation
                .push_system(&format!("command · {command}\n{response}"));
        }
    }

    fn open_command_panel(&mut self, panel: CommandPanel) {
        self.command_panel = Some(panel);
    }

    fn close_command_panel_to_return_target(&mut self) {
        self.command_panel = None;
    }

    fn close_command_panel_stack(&mut self) {
        let return_target = self
            .command_panel
            .as_ref()
            .and_then(|panel| panel.return_target);
        self.command_panel = None;
        match return_target {
            Some(CommandPanelReturnTarget::Menu) => self.active_menu = None,
            None => {}
        }
    }

    fn show_command_toast(&mut self, toast: CommandToast) {
        let toast_type = match toast.severity {
            CommandSeverity::Info => ratatui_toaster::ToastType::Info,
            CommandSeverity::Success => ratatui_toaster::ToastType::Success,
            CommandSeverity::Warning => ratatui_toaster::ToastType::Warning,
            CommandSeverity::Error => ratatui_toaster::ToastType::Error,
        };
        self.show_toast(&toast.message, toast_type);
    }

    fn show_startup_notice(&mut self) {
        let capability = crate::tui::glyphs::glyph_capability();
        if capability.should_show_fallback_notice() {
            let link = crate::tui::glyphs::nerd_font_install_help_url();
            self.show_toast(
                &format!(
                    "Nerd Font not detected; using portable glyph fallback. Install support: {link}"
                ),
                ratatui_toaster::ToastType::Info,
            );
        } else if capability.profile == crate::tui::glyphs::GlyphProfile::Unicode
            && capability.confidence == crate::tui::glyphs::GlyphConfidence::Medium
        {
            self.show_toast(
                &format!(
                    "Using portable glyphs; Nerd Font support is partially detected ({})",
                    capability.summary()
                ),
                ratatui_toaster::ToastType::Info,
            );
        }
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

    /// Handle a slash command.
    fn refresh_runtime_substrate(&mut self) -> String {
        let cwd = self.cwd().to_path_buf();
        let before_generation = self.runtime_generation;
        let skills_before = self
            .augment_registry
            .as_ref()
            .map(|registry| registry.skill_count())
            .unwrap_or(0);
        let skills_after = if let Some(ref mut registry) = self.augment_registry {
            registry.load_skills(&cwd);
            registry.skill_count()
        } else {
            skills_before
        };
        if self.augment_registry.is_some() {
            self.runtime_generation = self.runtime_generation.saturating_add(1);
        }
        match crate::setup::runtime_substrate_refresh_candidate(&cwd) {
            Ok(dry_run) => {
                let invalid = if dry_run.invalid_manifests.is_empty() {
                    "none".to_string()
                } else {
                    dry_run.invalid_manifests.join("; ")
                };
                format!(
                    "## Runtime substrate refresh\n\nStatus: partial live refresh completed; extension process/widget promotion is not implemented yet.\nRuntime generation: {before_generation} -> {}\n\nPreserved: TUI shell, session id, cwd, model/settings, conversation, workbench state.\nRefreshed now: user/project/extension skill augments.\nInspected only: discovered extensions, widgets, RPC handles, commands/tools, context-provider registrations, harness inventory.\nActive skill directives: {skills_before} -> {skills_after}\nCommand definitions registered: {}\n\nLive substrate inventory:\n- Extension widgets mounted: {}\n- Extension metadata entries: {}\n- Extension RPC handles: {}\n- Widget receivers: {}\n- Voice notification receivers: {}\n- Voice polling handles: {}\n- Vox polling handles: {}\n- Startup skill activation events: {}\n\nCandidate refresh inventory:\n- Extension candidates: {}\n- Skipped by policy: {}\n- Disabled extensions: {}\n- Invalid manifests: {invalid}\n- Candidate widgets: {}\n- Candidate metadata entries: {}\n- Candidate RPC handles: {}\n- Candidate widget receivers: {}\n- Candidate vox polling handles: {}\n- Reloadable skill entries: {}\n\nNext implementation step: promote validated extension-owned handles without replacing the whole runtime bus.",
                    self.runtime_generation,
                    self.bus_commands.len(),
                    self.runtime_inventory.extension_widgets,
                    self.runtime_inventory.extension_metadata_entries,
                    self.runtime_inventory.extension_rpc_handles,
                    self.runtime_inventory.widget_receivers,
                    self.runtime_inventory.voice_notification_receivers,
                    self.runtime_inventory.voice_polling_handles,
                    self.runtime_inventory.vox_polling_handles,
                    self.runtime_inventory.skill_activation_events,
                    dry_run.extension_candidates,
                    dry_run.skipped_by_policy,
                    dry_run.disabled_extensions,
                    dry_run.inventory.extension_widgets,
                    dry_run.inventory.extension_metadata_entries,
                    dry_run.inventory.extension_rpc_handles,
                    dry_run.inventory.widget_receivers,
                    dry_run.inventory.vox_polling_handles,
                    dry_run.inventory.skill_activation_events,
                )
            }
            Err(err) => format!(
                "Runtime substrate refresh candidate inspection failed after skill refresh: {err}"
            ),
        }
    }

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
                if matches!(args, "tutorial" | "tour") {
                    return self.handle_tutorial("", tx);
                }
                if args == "tutorial status" {
                    return self.handle_tutorial("status", tx);
                }
                if args == "tutorial reset" {
                    return self.handle_tutorial("reset", tx);
                }
                if args == "tutorial consent" {
                    return self.handle_tutorial("consent", tx);
                }
                if args == "tutorial demo" {
                    return self.handle_tutorial("demo", tx);
                }
                if args == "copy" {
                    return SlashResult::Display(
                        "Copy contract:
  Ctrl+Shift+Y       copy latest answer as plaintext
  /copy answer       copy latest answer as plaintext
  /copy answer raw   copy latest answer with markdown
  /copy plain        copy selected segment as plaintext
  /copy session      copy full transcript

Scroll transcript:
  PgUp/PgDn          scroll transcript
  Shift+Up/Down      fine scroll transcript"
                            .into(),
                    );
                }
                if args == "mouse" {
                    return SlashResult::Display(
                        "Mouse contract:
  App mouse          wheel/click panes
  Mouse passthrough  terminal drag selects text for this session
  Ctrl+Shift+T       toggle app mouse / mouse passthrough
  /mouse on          restore app mouse
  /mouse off         enable terminal-native drag selection for this session"
                            .into(),
                    );
                }
                if args == "next" {
                    return self.handle_tutorial_next(tx);
                }
                if args == "prev" {
                    return self.handle_tutorial_prev(tx);
                }

                if args.is_empty() || matches!(args, "menu" | "commands") {
                    self.open_command_inventory_menu();
                    return SlashResult::Handled;
                }

                let show_all = args == "all";
                let slim = !show_all && self.settings.lock().ok().is_some_and(|s| s.is_slim());
                // Harness-lifecycle commands hidden in slim/Cruise zone.
                const SLIM_HIDDEN: &[&str] = &["tree", "cleave", "delegate", "milestone"];
                let lines: Vec<String> = self
                    .command_menu_projection()
                    .rows
                    .into_iter()
                    .filter(|row| !slim || !SLIM_HIDDEN.contains(&row.name.as_str()))
                    .map(|row| {
                        let source = row.source.label();
                        let safety = row.safety.class_label();
                        if row.subcommands.is_empty() {
                            format!(
                                "  /{:<12} {}  [{} · {}]",
                                row.name, row.description, source, safety
                            )
                        } else {
                            format!(
                                "  /{:<12} {}  [{}]  [{} · {}]",
                                row.name,
                                row.description,
                                row.subcommands.join("|"),
                                source,
                                safety
                            )
                        }
                    })
                    .collect();
                let suffix = if slim {
                    " /help all for full list."
                } else {
                    ""
                };
                SlashResult::Display(format!(
                    "Commands:\n{}\n\nGuided tour: /help tutorial. Type / to browse. Tab completes.{suffix}",
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
                if args.is_empty() || args == "route" {
                    self.open_model_menu();
                    SlashResult::Handled
                } else if matches!(args, "providers" | "provider") {
                    self.open_model_menu();
                    if let Some(menu) = self.active_menu.as_mut() {
                        menu.state.active_tab = "providers".into();
                        menu.state.selected_row = 0;
                    }
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("model", args) {
                        Some(CanonicalSlashCommand::ModelList) => {
                            self.open_model_selector();
                            SlashResult::Handled
                        }
                        Some(CanonicalSlashCommand::SetModelGrade(grade)) => {
                            let _ = tx.try_send(TuiCommand::SetModelGrade {
                                grade: grade.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model Intent → grade {grade}"))
                        }
                        Some(CanonicalSlashCommand::SetModelProvider(provider)) => {
                            let _ = tx.try_send(TuiCommand::SetModelProvider {
                                provider: provider.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model Provider Intent → {provider}"))
                        }
                        Some(CanonicalSlashCommand::SetModelPolicy(policy)) => {
                            let _ = tx.try_send(TuiCommand::SetModelPolicy {
                                policy: policy.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model Policy Intent → {policy}"))
                        }
                        Some(CanonicalSlashCommand::ModelUnpin) => {
                            let _ = tx.try_send(TuiCommand::ModelUnpin { respond_to: None });
                            SlashResult::Display("Clearing exact model pin".into())
                        }
                        Some(CanonicalSlashCommand::SetModel(model)) => {
                            let _ = tx.try_send(TuiCommand::SetModel {
                                model: model.clone(),
                                respond_to: None,
                            });
                            SlashResult::Display(format!("Switching Model → {model}"))
                        }
                        _ => SlashResult::Display("Usage: /model [list|route|providers|grade <F|D|C|B|A|S>|provider <auto|local|upstream|endpoint>|policy <exact|minimum|nearest>|unpin|<provider:model>]".into()),
                    }
                }
            }

            "think" => {
                if args.is_empty() {
                    // No args → open interactive selector
                    self.open_thinking_selector();
                    SlashResult::Handled
                } else if let Some(CanonicalSlashCommand::ThinkingView) =
                    canonical_slash_command("think", args)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::ThinkingView,
                        respond_to: None,
                    });
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

            "profile" => {
                if args.trim().is_empty() {
                    self.open_profile_menu();
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("profile", args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Usage: /profile [view|export|capture|apply|mqtt on|mqtt off|extension allow <name>|extension deny <name>|extensions clear|persona <name|off>|tone <name|off>]".into(),
                    )
                }
            }

            "permissions" | "permission" | "trust" => {
                if let Some(command) = canonical_slash_command(cmd, args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Usage: /permissions [list|add <path>|remove <path>]\n\
                         Alias: /trust [list|add <path>|remove <path>]"
                            .into(),
                    )
                }
            }

            "automation" | "autonomy" => {
                if let Some(command) = canonical_slash_command(cmd, args)
                    && let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                    SlashResult::Handled
                } else {
                    SlashResult::Display(
                        "Usage: /automation [status|ask|guarded|flow|autonomous]\n\
                         Alias: /autonomy [status|ask|guarded|flow|autonomous]"
                            .into(),
                    )
                }
            }

            "skills" | "skill" => {
                const USAGE: &str = "Usage: /skills [list|reload|refresh|install [name|skills/name]|create|new [--project|--user]|import [--project|--user] <path>|get <name>|delete <name>]";
                if cmd == "skill" && args.trim().is_empty() {
                    return SlashResult::Display("Usage: /skill <skills-subcommand>\nAlias for /skills. Run /skills for the active skills menu or /skills --help for command syntax.".into());
                }
                if let Some(command) = canonical_slash_command("skills", args) {
                    match command {
                        CanonicalSlashCommand::SkillsView => match self.open_skills_menu() {
                            Ok(()) => SlashResult::Handled,
                            Err(message) => SlashResult::Display(message),
                        },
                        CanonicalSlashCommand::SkillsHelp => {
                            SlashResult::Display(crate::control_runtime::skills_help_text().into())
                        }
                        CanonicalSlashCommand::SkillsReload => {
                            let cwd = self.cwd().to_path_buf();
                            if let Some(ref mut registry) = self.augment_registry {
                                let before_generation = self.runtime_generation;
                                registry.load_skills(&cwd);
                                let loaded = registry.skill_count();
                                let events = registry.skill_activation_events();
                                self.runtime_generation = self.runtime_generation.saturating_add(1);
                                let after_generation = self.runtime_generation;
                                let mut out = format!(
                                    "## Skills reloaded\n\nRuntime generation: {before_generation} -> {after_generation}\nLoaded {loaded} active skill directive(s) from user and project skill directories. Changes apply to subsequent model requests in this session.\n"
                                );
                                if !events.is_empty() {
                                    out.push_str("\nActivation events:\n");
                                    for event in events {
                                        self.conversation.push_skill_event(event);
                                        out.push_str(&format!(
                                            "- {} · {} · {}\n",
                                            event.active_ref, event.reason, event.resolution
                                        ));
                                        if !event.suppressing.is_empty() {
                                            out.push_str(&format!(
                                                "  - suppressing: {}\n",
                                                event.suppressing.join(", ")
                                            ));
                                        }
                                    }
                                }
                                SlashResult::Display(out)
                            } else {
                                SlashResult::Display(
                                    "Skills reload unavailable: no active augment registry in this TUI session.".into(),
                                )
                            }
                        }
                        CanonicalSlashCommand::SkillCreate(scope) => {
                            // Queue the skill builder prompt — the agent converses
                            // with the operator to create a new skill.
                            let cwd = self.cwd().to_path_buf();
                            let mut builder_prompt = crate::skills::skill_builder_prompt(&cwd);
                            if let Some(scope) = scope {
                                let scope_label = match scope {
                                    SkillCreateScope::Project => "project-local .omegon/skills",
                                    SkillCreateScope::User => "user-level skills directory",
                                };
                                builder_prompt.push_str(&format!(
                                    "\n\nThe operator requested {scope_label} output. Make that destination explicit before writing files."
                                ));
                            }
                            if let Err(result) = Self::submit_prompt_from_slash(
                                tx,
                                PromptSubmission {
                                    text: builder_prompt,
                                    image_paths: Vec::new(),
                                    submitted_by: "local-tui".to_string(),
                                    via: "tui",
                                    queue_mode: PromptQueueMode::UntilReady,
                                    metadata: PromptMetadata::default(),
                                },
                            ) {
                                return result;
                            }
                            self.queue_mode = PromptQueueMode::UntilReady;
                            tracing::debug!("skill builder submitted to runtime queue");
                            SlashResult::Handled
                        }
                        CanonicalSlashCommand::SkillImport { path, scope } => {
                            let scope_hint = match scope {
                                Some(SkillCreateScope::Project) => " into project-local .omegon/skills",
                                Some(SkillCreateScope::User) => " into the user-level skills directory",
                                None => "",
                            };
                            let safe_path = path.replace('`', "\\`");
                            let prompt = format!(
                                "Import the Omegon skill from `{safe_path}`{scope_hint}. Read and validate the skill frontmatter, copy it to the requested external skill directory, and report any schema or collision issues before overwriting existing files. Do not write to bundled/internal skill paths. After import, tell the operator to run `/skills reload` to activate it in this session, then `/skills get <name>` to inspect it."
                            );
                            if let Err(result) = Self::submit_prompt_from_slash(
                                tx,
                                PromptSubmission {
                                    text: prompt,
                                    image_paths: Vec::new(),
                                    submitted_by: "local-tui".to_string(),
                                    via: "tui",
                                    queue_mode: PromptQueueMode::UntilReady,
                                    metadata: PromptMetadata::default(),
                                },
                            ) {
                                return result;
                            }
                            self.queue_mode = PromptQueueMode::UntilReady;
                            SlashResult::Handled
                        }
                        other => {
                            if let Some(request) =
                                crate::control_runtime::control_request_from_slash(&other)
                            {
                                let _ = tx.try_send(TuiCommand::ExecuteControl {
                                    request,
                                    respond_to: None,
                                });
                                SlashResult::Handled
                            } else {
                                SlashResult::Display(USAGE.into())
                            }
                        }
                    }
                } else {
                    SlashResult::Display(USAGE.into())
                }
            }

            "plan" => {
                const USAGE: &str = "Usage: /plan [status|list|set <item> | <item>|approve|execute|advance|skip|clear]";
                match canonical_slash_command("plan", args) {
                    Some(
                        command @ (CanonicalSlashCommand::PlanView
                        | CanonicalSlashCommand::PlanList
                        | CanonicalSlashCommand::PlanSet(_)
                        | CanonicalSlashCommand::PlanApprove
                        | CanonicalSlashCommand::PlanExecute
                        | CanonicalSlashCommand::PlanAdvance
                        | CanonicalSlashCommand::PlanSkip
                        | CanonicalSlashCommand::PlanClear),
                    ) => {
                        let _ = tx.try_send(TuiCommand::UpdatePlan {
                            command,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    }
                    _ => SlashResult::Display(USAGE.into()),
                }
            }

            "extension" | "ext" => {
                if args.trim().is_empty() {
                    self.open_extension_runtime_menu();
                    SlashResult::Handled
                } else if let Some(command) = canonical_slash_command("extension", args) {
                    if matches!(command, CanonicalSlashCommand::RuntimeSubstrateRefresh) {
                        self.handle_slash_command("/runtime refresh", tx)
                    } else if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display(
                            "Usage: /extension [list|view|get <name>|install <name|url|path>|remove <name>|update [name]|enable <name>|disable <name>|refresh|reload|restart|search [query]]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /extension [list|view|get <name>|install <name|url|path>|remove <name>|update [name]|enable <name>|disable <name>|refresh|reload|restart|search [query]]"
                            .into(),
                    )
                }
            }

            "catalog" => {
                if let Some(command) = canonical_slash_command("catalog", args) {
                    if let Some(request) =
                        crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        SlashResult::Handled
                    } else {
                        SlashResult::Display("Usage: /catalog [list|install|remove <id>]".into())
                    }
                } else {
                    SlashResult::Display("Usage: /catalog [list|install|remove <id>]".into())
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
                            "Usage: /plugin [list|install <git-url|local-path>|remove <name>|update [name]]. Use /armory install <path> for registry plugins."
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /plugin [list|install <git-url|local-path>|remove <name>|update [name]]. Use /armory install <path> for registry plugins."
                            .into(),
                    )
                }
            }

            "armory" => {
                if let Some(command) = canonical_slash_command("armory", args) {
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
                            "Usage: /armory [list|browse [query]|search [query]|install <name|skills/name|personas/name|tones/name|examples/name>]"
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /armory [list|browse [query]|search [query]|install <name|skills/name|personas/name|tones/name|examples/name>]"
                            .into(),
                    )
                }
            }

            "runtime" => {
                if args.trim().is_empty() {
                    self.open_extension_runtime_menu();
                    SlashResult::Handled
                } else if let Some(CanonicalSlashCommand::RuntimeSubstrateRefresh) =
                    canonical_slash_command("runtime", args)
                {
                    if self.agent_active {
                        SlashResult::Display(
                            "Runtime substrate refresh unavailable while a model turn is active. Wait for completion or cancel the turn first.".into(),
                        )
                    } else {
                        SlashResult::Display(self.refresh_runtime_substrate())
                    }
                } else {
                    SlashResult::Display("Usage: /runtime [refresh|reload|restart|hot-restart]".into())
                }
            }

            "stats" => {
                if args == "bench" {
                    return self.handle_slash_command("/bench", tx);
                }
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
                    SlashResult::Display("Usage: /stats [bench]".into())
                }
            }

            // TUI-local command — reads only rendering state (footer_data,
            // session_start). Not routed through Feature dispatch because
            // piping this state through BusEvent would be worse.
            "bench" | "perf" => {
                let session_secs = self.session_start.elapsed().as_secs();
                let turns = self.turn;
                let input_tokens = self.footer_data.session_input_tokens;
                let output_tokens = self.footer_data.session_output_tokens;
                let ctx_pct = self.footer_data.context_percent;
                let ctx_window = self.footer_data.context_window;
                let model = &self.footer_data.model_id;
                let version = env!("CARGO_PKG_VERSION");

                let avg_turn_secs = if turns > 0 {
                    session_secs as f64 / turns as f64
                } else {
                    0.0
                };
                let tokens_per_turn = if turns > 0 {
                    (input_tokens + output_tokens) / turns as u64
                } else {
                    0
                };

                let rss_mb = get_rss_mb().unwrap_or(0.0);

                SlashResult::Display(format!(
                    "Omegon Performance — v{version}\n\n\
                     Startup\n\
                     ────────────────────────────────\n\
                     Process age:        {session_secs}s\n\
                     RSS memory:         {rss_mb:.1} MB\n\n\
                     Session\n\
                     ────────────────────────────────\n\
                     Model:              {model}\n\
                     Turns:              {turns}\n\
                     Avg turn time:      {avg_turn_secs:.1}s\n\
                     Input tokens:       {input_tokens}\n\
                     Output tokens:      {output_tokens}\n\
                     Tokens/turn:        {tokens_per_turn}\n\
                     Context:            {ctx_pct:.0}% of {ctx_window}"
                ))
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
                if args == "create" || args == "new" {
                    let builder_prompt = crate::plugins::persona_loader::persona_builder_prompt();
                    if let Err(result) = Self::submit_prompt_from_slash(
                        tx,
                        PromptSubmission {
                            text: builder_prompt,
                            image_paths: Vec::new(),
                            submitted_by: "local-tui".to_string(),
                            via: "tui",
                            queue_mode: PromptQueueMode::UntilReady,
                            metadata: PromptMetadata::default(),
                        },
                    ) {
                        return result;
                    }
                    self.queue_mode = PromptQueueMode::UntilReady;
                    tracing::debug!("persona builder submitted to runtime queue");
                    SlashResult::Handled
                } else if args == "list" {
                    if let Some(command) = canonical_slash_command("persona", args)
                        && let Some(request) =
                            crate::control_runtime::control_request_from_slash(&command)
                    {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request,
                            respond_to: None,
                        });
                        return SlashResult::Handled;
                    }
                    SlashResult::Display("Usage: /persona [list|create|off|<name>]".into())
                } else if args == "off" {
                    if let Some(ref mut registry) = self.augment_registry {
                        let result = registry.deactivate_persona();
                        match result.removed_id {
                            Some(id) => SlashResult::Display(format!("Persona deactivated: {id}")),
                            None => SlashResult::Display("No persona active.".into()),
                        }
                    } else {
                        SlashResult::Display("Augment registry not initialized.".into())
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
                                    if let Some(ref mut registry) = self.augment_registry {
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
                            "Persona '{args}' not found. Run /persona list to see available, or /persona create to build one."
                        )),
                    }
                }
            }

            "tone" => {
                if args == "off" {
                    if let Some(ref mut registry) = self.augment_registry {
                        let result = registry.deactivate_tone();
                        match result {
                            Some(id) => SlashResult::Display(format!("Tone deactivated: {id}")),
                            None => SlashResult::Display("No tone active.".into()),
                        }
                    } else {
                        SlashResult::Display("Augment registry not initialized.".into())
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
                                    if let Some(ref mut registry) = self.augment_registry {
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

            "detail" | "density" => {
                if args.is_empty() {
                    let current = self.settings().tool_detail;
                    let next = current.next();
                    self.update_and_persist(|s| s.tool_detail = next);
                    SlashResult::Display(format!("Tool density → {}", next.as_str()))
                } else if let Some(mode) = crate::settings::ToolDetail::parse(args) {
                    self.update_and_persist(|s| s.tool_detail = mode);
                    SlashResult::Display(format!("Tool density → {}", mode.as_str()))
                } else {
                    SlashResult::Display(format!(
                        "Unknown density: {args}. Options: lean, compact, detailed, verbose"
                    ))
                }
            }

            "context" => {
                if args.is_empty() {
                    self.open_context_menu();
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
                            SlashResult::Display("Starting fresh context…".into())
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
                                 Use: /context [status|compact|compress|reset|clear|<class>]\n\
                                 Classes: compact, standard, extended, massive"
                            ))
                        }
                    }
                }
            }

            "new" => {
                let _ = tx.try_send(TuiCommand::ContextClear { respond_to: None });
                SlashResult::Handled
            }

            "resume" => {
                let id = args.trim();
                if id.is_empty() {
                    SlashResult::Display("Usage: /resume <session-id>".into())
                } else {
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request: crate::control_runtime::ControlRequest::ResumeSession {
                            id: id.to_string(),
                        },
                        respond_to: None,
                    });
                    SlashResult::Display(format!("Resuming session {id}…"))
                }
            }

            "sessions" => {
                if args.trim().is_empty() {
                    self.open_sessions_menu();
                    SlashResult::Handled
                } else {
                    match canonical_slash_command("sessions", args) {
                    Some(CanonicalSlashCommand::ResumeSession(id)) => {
                        let _ = tx.try_send(TuiCommand::ExecuteControl {
                            request: crate::control_runtime::ControlRequest::ResumeSession {
                                id: id.clone(),
                            },
                            respond_to: None,
                        });
                        SlashResult::Display(format!("Resuming session {id}…"))
                    }
                    _ => {
                        let _ = tx.try_send(TuiCommand::ListSessions { respond_to: None });
                        SlashResult::Handled
                    }
                }
                }
            }

            "memory" => {
                let sub = args.trim();
                if sub.is_empty() {
                    self.open_memory_menu();
                    SlashResult::Handled
                } else if matches!(sub, "status" | "overview") {
                    SlashResult::Display(self.memory_status_text())
                } else {
                    SlashResult::Display(format!(
                        "Unknown memory command: {sub}\n\nUsage: /memory [status|overview]"
                    ))
                }
            }

            "auth" => match canonical_slash_command("auth", args) {
                Some(CanonicalSlashCommand::AuthView) => {
                    self.open_auth_menu();
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthStatus) => {
                    let _ = tx.try_send(TuiCommand::AuthStatus { respond_to: None });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthLogin(provider)) => {
                    let _ = tx.try_send(TuiCommand::AuthLogin {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthLogout(provider)) => {
                    let _ = tx.try_send(TuiCommand::AuthLogout {
                        provider,
                        respond_to: None,
                    });
                    SlashResult::Handled
                }
                Some(CanonicalSlashCommand::AuthUnlock) => {
                    let _ = tx.try_send(TuiCommand::AuthUnlock { respond_to: None });
                    SlashResult::Handled
                }
                _ => SlashResult::Display(format!(
                    "Unknown auth command: {args}\n\nUsage:\n  /auth\n  /auth status\n  /auth unlock\n  /auth login <provider>\n  /auth logout <provider>"
                )),
            },

            "update" => {
                let trimmed = args.trim();
                if trimmed == "install" {
                    let info = self.update_rx.as_ref().and_then(|rx| rx.borrow().clone());
                    match info {
                        Some(info) if info.is_newer && info.has_downloadable_archive() => {
                            let args: Vec<String> = std::env::args().skip(1).collect();
                            let keyboard_enhancement = self.keyboard_enhancement;
                            let latest = info.latest.clone();
                            tokio::spawn(async move {
                                match crate::update::download_and_replace(&info).await {
                                    Ok(binary) => {
                                        #[cfg(unix)]
                                        {
                                            let _ = io::stdout()
                                                .execute(crossterm::event::DisableMouseCapture);
                                            if keyboard_enhancement {
                                                let _ = io::stdout()
                                                    .execute(PopKeyboardEnhancementFlags);
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
                        Some(info) if info.is_newer => {
                            if let Some(tx) = self.update_tx.clone() {
                                let channel = crate::update::UpdateChannel::parse(
                                    &self.settings().update_channel,
                                )
                                .unwrap_or(crate::update::UpdateChannel::Stable);
                                crate::update::spawn_check_now(tx, channel);
                            }
                            SlashResult::Display(format!(
                                "v{} is published, but the signed archive for this platform is not available yet. Rechecking now; run `/update install` again after the release assets finish publishing.",
                                info.latest
                            ))
                        }
                        Some(_) => SlashResult::Display(
                            "No downloadable update is available for this platform.".into(),
                        ),
                        None => {
                            if let Some(tx) = self.update_tx.clone() {
                                let channel = crate::update::UpdateChannel::parse(
                                    &self.settings().update_channel,
                                )
                                .unwrap_or(crate::update::UpdateChannel::Stable);
                                crate::update::spawn_check_now(tx, channel);
                            }
                            SlashResult::Display(
                                "Checking for updates now. Run `/update install` again once the check completes."
                                    .into(),
                            )
                        }
                    }
                } else if let Some(channel_arg) = trimmed.strip_prefix("channel") {
                    let channel_arg = channel_arg.trim();
                    if channel_arg.is_empty() {
                        self.open_update_channel_selector();
                        SlashResult::Handled
                    } else if let Some(channel) = crate::update::UpdateChannel::parse(channel_arg) {
                        self.update_settings(|s| s.update_channel = channel.as_str().to_string());
                        if let Some(tx) = self.update_tx.clone() {
                            crate::update::spawn_check_now(tx, channel);
                        }
                        SlashResult::Display(format!(
                            "Update channel set to {}. Rechecking for updates now.",
                            channel.as_str()
                        ))
                    } else {
                        SlashResult::Display("Usage: /update channel [stable|nightly]".into())
                    }
                } else {
                    // Check if an update is available
                    let info = self.update_rx.as_ref().and_then(|rx| rx.borrow().clone());
                    let channel = self.settings().update_channel;
                    match info {
                        Some(info) if info.is_newer => SlashResult::Display(format!(
                            "🆕 Update available on {channel}: v{} → v{}\n\n{}\n\n{}\n\nCommands:\n  /update install\n  /update channel [stable|nightly]",
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
                            if !info.has_downloadable_archive() {
                                if let Some(tx) = self.update_tx.clone() {
                                    let channel = crate::update::UpdateChannel::parse(
                                        &self.settings().update_channel,
                                    )
                                    .unwrap_or(crate::update::UpdateChannel::Stable);
                                    crate::update::spawn_check_now(tx, channel);
                                }
                                String::from(
                                    "Release assets for this platform are not available yet. Rechecking now.",
                                )
                            } else {
                                String::from("Run `/update install` to download and restart")
                            },
                        )),
                        _ => {
                            if let Some(tx) = self.update_tx.clone() {
                                let channel = crate::update::UpdateChannel::parse(
                                    &self.settings().update_channel,
                                )
                                .unwrap_or(crate::update::UpdateChannel::Stable);
                                crate::update::spawn_check_now(tx, channel);
                            }
                            SlashResult::Display(format!(
                                "✓ No update is currently cached for the {channel} channel. Checking GitHub now.\n\nCommands:\n  /update install         — install a discovered update\n  /update channel stable  — stable releases only\n  /update channel nightly — nightly builds from main\n  /update channel         — show current channel"
                            ))
                        }
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
                    Err(e) => SlashResult::Display(format!("✗ {e}")),
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
                if let Some(url) = dash_browser_url(self.web_startup.as_ref(), self.web_server_addr)
                {
                    if args == "status" {
                        let detail = self
                            .web_startup
                            .as_ref()
                            .map(|startup| {
                                let (http_security, ws_security) =
                                    startup_transport_security(startup);
                                let warnings = if startup.daemon_status.transport_warnings.is_empty() {
                                    "none".to_string()
                                } else {
                                    startup.daemon_status.transport_warnings.join(" | ")
                                };
                                format!(
                                    "\nstartup: {}\nwebsocket: {}\ntransport: http={}, ws={}\nqueue depth: {}\nprocessed events: {}\ntransport warnings: {}",
                                    startup.startup_url,
                                    startup.ws_url,
                                    format_transport_security(&http_security),
                                    format_transport_security(&ws_security),
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

            "delegate" | "subagent" => {
                if let Some(command) = canonical_slash_command(cmd, args) {
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
                            "Usage: /delegate status or /subagent status\n\nTo invoke a delegate/subagent, use the delegate agent tool."
                                .into(),
                        )
                    }
                } else {
                    SlashResult::Display(
                        "Usage: /delegate status or /subagent status\n\nTo invoke a delegate/subagent, use the delegate agent tool."
                            .into(),
                    )
                }
            }

            "subagents" => {
                SlashResult::Display("Use the explicit singular command: /subagent status".into())
            }

            "focus" => SlashResult::Display(
                "Focus mode has been removed. Use Ctrl+O or Tab on an empty composer to toggle the tool detail row."
                    .into(),
            ),

            "ui" => {
                let args = args.trim();
                if let Some(density) = args
                    .strip_prefix("detail ")
                    .or_else(|| args.strip_prefix("density "))
                {
                    return self.handle_slash_command(&format!("/detail {}", density.trim()), tx);
                }
                if matches!(args, "detail" | "density") {
                    return self.handle_slash_command("/detail", tx);
                }
                if args.is_empty() || args == "surfaces" {
                    self.open_ui_menu();
                    SlashResult::Handled
                } else if args == "status" {
                    SlashResult::Display(self.ui_status_text())
                } else if args == "lean" {
                    let outcome = self.handle_ui_preset_action(SetUiPresetAction {
                        surfaces: UiSurfaces::lean(),
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| "UI → lean".into()))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if args == "full" {
                    let outcome = self.handle_ui_preset_action(SetUiPresetAction {
                        surfaces: UiSurfaces::full(),
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => SlashResult::Display(
                            message
                                .unwrap_or_else(|| "UI → full (+ dashboard + instruments)".into()),
                        ),
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if let Some(surface) = args.strip_prefix("toggle ") {
                    let surface = match UiSurfaceToggle::parse(surface) {
                        Ok(surface) => surface,
                        Err(err) => return SlashResult::Display(err),
                    };
                    let enabled = match surface {
                        UiSurfaceToggle::Dashboard => !self.ui_surfaces.dashboard,
                        UiSurfaceToggle::Instruments => !self.ui_surfaces.instruments,
                        UiSurfaceToggle::Footer => !self.ui_surfaces.footer,
                        UiSurfaceToggle::Activity => !self.ui_surfaces.activity,
                    };
                    let outcome = self.handle_surface_visible_action(SetSurfaceVisibleAction {
                        surface,
                        visible: enabled,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| {
                                format!(
                                    "UI surface {}: {}",
                                    if enabled { "enabled" } else { "disabled" },
                                    surface.label()
                                )
                            }))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if let Some(surface) = args.strip_prefix("show ") {
                    let surface = match UiSurfaceToggle::parse(surface) {
                        Ok(surface) => surface,
                        Err(err) => return SlashResult::Display(err),
                    };
                    let outcome = self.handle_surface_visible_action(SetSurfaceVisibleAction {
                        surface,
                        visible: true,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| {
                                format!("UI surface enabled: {}", surface.label())
                            }))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else if let Some(surface) = args.strip_prefix("hide ") {
                    let surface = match UiSurfaceToggle::parse(surface) {
                        Ok(surface) => surface,
                        Err(err) => return SlashResult::Display(err),
                    };
                    let outcome = self.handle_surface_visible_action(SetSurfaceVisibleAction {
                        surface,
                        visible: false,
                    });
                    match outcome {
                        UiActionOutcome::Accepted { message } => {
                            SlashResult::Display(message.unwrap_or_else(|| {
                                format!("UI surface disabled: {}", surface.label())
                            }))
                        }
                        other => SlashResult::Display(format!("UI action failed: {other:?}")),
                    }
                } else {
                    SlashResult::Display(format!(
                        "Unknown UI command: {args}

{}",
                        self.ui_status_text()
                    ))
                }
            }

            "copy" => match args {
                "" | "raw" => {
                    self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Raw);
                    SlashResult::Handled
                }
                "answer" | "answer plain" | "answer plaintext" | "latest plain"
                | "latest plaintext" | "response plain" | "assistant plain" => {
                    self.copy_latest_assistant_response(SegmentExportMode::Plaintext);
                    SlashResult::Handled
                }
                "answer raw" | "latest" | "response" | "assistant" => {
                    self.copy_latest_assistant_response(SegmentExportMode::Raw);
                    SlashResult::Handled
                }
                "plain" | "plaintext" => {
                    self.copy_selected_conversation_segment_with_mode(SegmentExportMode::Plaintext);
                    SlashResult::Handled
                }
                "session" | "all" => {
                    self.copy_full_session();
                    SlashResult::Handled
                }
                _ => SlashResult::Display(
                    "Usage: /copy [raw|plain|answer|answer raw|latest|session]".into(),
                ),
            },

            "transcript" => {
                match args {
                    "" | "open" | "file" | "md" | "markdown" => {
                        self.export_session_transcript_markdown();
                    }
                    "scrollback" | "native" => {
                        self.print_transcript_to_native_scrollback();
                    }
                    _ => {
                        self.conversation.push_system(
                            "Usage: /transcript [file|scrollback]\n  file: write a clickable Markdown transcript\n  scrollback: print transcript to native terminal scrollback",
                        );
                    }
                }
                SlashResult::Handled
            }

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

            "demo" => self.handle_tutorial(args, tx),

            "variables" | "vars" => self.handle_variables(args, tx),

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
                            "Unknown vault subcommand: {args}\nOptions: status, configure, init-policy"
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
                    self.open_auth_menu();
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
                self.apply_ui_preset(UiSurfaces::lean());
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim: true },
                    respond_to: None,
                });
                SlashResult::Display("Shackled: om mode active.".into())
            }
            "unshackle" => {
                self.apply_ui_preset(UiSurfaces::full());
                let _ = tx.try_send(TuiCommand::ExecuteControl {
                    request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim: false },
                    respond_to: None,
                });
                SlashResult::Display("Unshackled: omegon mode active.".into())
            }
            "warp" => {
                let slim_now = self.settings.lock().ok().is_some_and(|s| s.is_slim());
                let target_slim = !slim_now;
                self.apply_ui_preset(if target_slim {
                    UiSurfaces::lean()
                } else {
                    UiSurfaces::full()
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
            "settings" => {
                self.open_settings_menu();
                self.command_panel = None;
                SlashResult::Handled
            }
            "preferences" | "prefs" => {
                self.open_preferences_selector();
                SlashResult::Handled
            }
            "sandbox" => {
                let sub = args.split_whitespace().next().unwrap_or("");
                match sub {
                    "on" | "enable" => {
                        // Check for container runtime before enabling
                        let runtime = crate::nex::spawn::detect_container_runtime_public();
                        if let Some(ref rt) = runtime {
                            let cwd = self.cwd().to_path_buf();
                            if let Ok(mut s) = self.settings.lock() {
                                s.sandbox = true;
                                let mut profile = crate::settings::Profile::load(&cwd);
                                profile.capture_from(&s);
                                let _ = profile.save(&cwd);
                            }
                            SlashResult::Display(format!(
                                "Sandbox enabled ({rt})\n\n\
                                 Delegate and cleave children will now run inside \
                                 isolated containers with:\n\
                                 - Read-only root filesystem\n\
                                 - No network access\n\
                                 - Workspace mounted at /work\n\n\
                                 /sandbox off     disable\n\
                                 /sandbox status  current state"
                            ))
                        } else {
                            SlashResult::Display(
                                "No container runtime found.\n\n\
                                 Sandbox requires podman or docker:\n\
                                 - macOS:  brew install podman\n\
                                 - Linux:  apt install podman  (or docker)\n\
                                 - NixOS:  nix-env -i podman\n\n\
                                 Podman is preferred (rootless, daemonless)."
                                    .into(),
                            )
                        }
                    }
                    "off" | "disable" => {
                        let cwd = self.cwd().to_path_buf();
                        if let Ok(mut s) = self.settings.lock() {
                            s.sandbox = false;
                            let mut profile = crate::settings::Profile::load(&cwd);
                            profile.capture_from(&s);
                            let _ = profile.save(&cwd);
                        }
                        SlashResult::Display(
                            "Sandbox disabled. Children run as local subprocesses.".into(),
                        )
                    }
                    "" | "status" => {
                        let enabled = self
                            .settings
                            .lock()
                            .ok()
                            .map(|s| s.sandbox)
                            .unwrap_or(false);
                        let runtime = crate::nex::spawn::detect_container_runtime_public();
                        let rt_str = runtime.as_deref().unwrap_or("not found");
                        let status = if enabled { "enabled" } else { "disabled" };
                        SlashResult::Display(format!(
                            "Sandbox: {status}\n\
                             Runtime: {rt_str}\n\n\
                             /sandbox on   enable container isolation\n\
                             /sandbox off  disable (use local subprocesses)"
                        ))
                    }
                    _ => SlashResult::Display("Usage: /sandbox [on|off|status]".into()),
                }
            }
            "version" => SlashResult::Display(format!(
                "Version\n  Omegon:     {}\n  Git SHA:    {}\n  Build Date: {}",
                env!("CARGO_PKG_VERSION"),
                env!("OMEGON_GIT_SHA"),
                env!("OMEGON_BUILD_DATE"),
            )),

            "smoke" => match canonical_slash_command("smoke", args) {
                Some(CanonicalSlashCommand::Smoke(crate::smoke_surface::SmokeCommand::List)) => {
                    SlashResult::Display(crate::smoke_surface::smoke_list_text())
                }
                Some(CanonicalSlashCommand::Smoke(crate::smoke_surface::SmokeCommand::Scenario(scenario))) => {
                    self.launch_surface_smoke(scenario)
                }
                _ => SlashResult::Display("Usage: /smoke [list|cleave|delegate|surface]".into()),
            },
            "q" => SlashResult::Quit,

            "editor" => SlashResult::Display(handle_editor_command(args)),

            "cleave" => {
                // /cleave starts background workers from an interactive session, so disclose
                // subscription-credential automation risk there without warning on normal TUI use.
                if self.footer_data.is_oauth
                    && crate::providers::anthropic_credential_mode()
                        == crate::providers::AnthropicCredentialMode::OAuthOnly
                {
                    self.show_toast(
                        "Anthropic subscription is active. /cleave starts background workers, which may be restricted by Anthropic's \
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
                    let matches: Vec<&str> = crate::command_registry::BUILTIN_COMMANDS
                        .iter()
                        .map(|command| command.name)
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
                            "Unknown command: /{cmd}. Type /help for commands."
                        ))
                    }
                }
            }
        }
    }

    fn is_hidden_bus_command(name: &str) -> bool {
        matches!(name, "opus" | "sonnet" | "haiku")
    }

    fn command_menu_projection(&self) -> crate::surfaces::command_menu::CommandMenuProjection {
        crate::surfaces::command_menu::command_menu_projection(
            crate::command_registry::builtin_command_definitions(),
            self.bus_commands.clone(),
            &["opus", "sonnet", "haiku"],
        )
    }

    /// Palette: matching commands + subcommands for the current editor text.
    fn matching_commands(&self) -> Vec<crate::surfaces::command_menu::CommandMenuRowProjection> {
        let text = self.editor.render_text();
        self.command_menu_projection().matching(&text)
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

    fn handle_mouse_scroll_up(&mut self, column: u16, row: u16) {
        let over_dashboard = self.mouse_capture_enabled
            && self.dashboard_area.is_some_and(|area| {
                column >= area.x
                    && column < area.x + area.width
                    && row >= area.y
                    && row < area.y + area.height
            });
        if over_dashboard {
            self.dashboard.scroll_up(3);
        } else {
            self.conversation.scroll_up(3);
        }
    }

    fn handle_mouse_scroll_down(&mut self, column: u16, row: u16) {
        let over_dashboard = self.mouse_capture_enabled
            && self.dashboard_area.is_some_and(|area| {
                column >= area.x
                    && column < area.x + area.width
                    && row >= area.y
                    && row < area.y + area.height
            });
        if over_dashboard {
            self.dashboard.scroll_down(3);
        } else {
            self.conversation.scroll_down(3);
        }
    }

    fn handle_keyboard_up(&mut self) {
        if let Some(ref mut picker) = self.at_picker {
            picker.move_up();
        } else if self.editor.line_count() > 1 && self.editor.cursor_row() > 0 {
            self.editor.move_up();
        }
    }

    fn handle_keyboard_down(&mut self) {
        if let Some(ref mut picker) = self.at_picker {
            picker.move_down();
        } else if self.editor.line_count() > 1
            && self.editor.cursor_row() < self.editor.line_count() - 1
        {
            self.editor.move_down();
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        if self.history_idx.is_none() {
            self.history_draft = Some(self.editor.render_text());
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
                    let draft = self.history_draft.take().unwrap_or_default();
                    self.editor.set_text(&draft);
                }
            }
        }
    }

    fn exit_history_recall(&mut self) {
        self.history_idx = None;
        self.history_draft = None;
    }

    fn history_recall_up(&mut self) {
        self.pending_history_preload = None;
        if self.history_idx.is_some() || self.editor.is_empty() {
            self.history_up();
        }
    }

    fn history_recall_down(&mut self) {
        self.pending_history_preload = None;
        if self.history_idx.is_some() {
            self.history_down();
        }
    }

    fn prune_activity_tools(&mut self, now: std::time::Instant) {
        self.activity_tools.retain(|tool| {
            tool.expires_at
                .map(|deadline| now < deadline)
                .unwrap_or(true)
        });
    }

    fn cap_activity_tools(&mut self) {
        const MAX_COMPLETED_ACTIVITY_TOOLS: usize = 4;
        const MAX_ACTIVITY_TOOLS: usize = 8;

        let mut completed_seen = 0usize;
        self.activity_tools.retain(|tool| {
            if matches!(
                tool.status,
                crate::surfaces::activity::ActivityToolStatus::Running
            ) {
                return true;
            }
            completed_seen += 1;
            completed_seen <= MAX_COMPLETED_ACTIVITY_TOOLS
        });

        while self.activity_tools.len() > MAX_ACTIVITY_TOOLS {
            if let Some(idx) = self.activity_tools.iter().rposition(|tool| {
                !matches!(
                    tool.status,
                    crate::surfaces::activity::ActivityToolStatus::Running
                )
            }) {
                self.activity_tools.remove(idx);
            } else {
                self.activity_tools.pop_back();
            }
        }
    }

    fn push_activity_tool_start(&mut self, id: &str, name: &str, args_summary: Option<String>) {
        self.prune_activity_tools(std::time::Instant::now());
        self.activity_tools.retain(|tool| tool.segment_id != id);
        self.activity_tools.push_front(ActivityToolState {
            segment_id: id.to_string(),
            name: name.to_string(),
            args_summary,
            result_summary: None,
            mode: crate::surfaces::activity::ActivityToolMode::Live,
            status: crate::surfaces::activity::ActivityToolStatus::Running,
            expires_at: None,
        });
        self.cap_activity_tools();
    }

    fn mark_activity_tool_end(&mut self, id: &str, is_error: bool, result_summary: Option<String>) {
        let linger_for = if is_error {
            Duration::from_secs(8)
        } else {
            Duration::from_millis(2200)
        };
        let expires_at = std::time::Instant::now() + linger_for;
        if let Some(activity_tool) = self
            .activity_tools
            .iter_mut()
            .find(|tool| tool.segment_id == id)
        {
            activity_tool.status = if is_error {
                crate::surfaces::activity::ActivityToolStatus::Error
            } else {
                crate::surfaces::activity::ActivityToolStatus::Complete
            };
            activity_tool.result_summary = result_summary;
            activity_tool.expires_at = Some(expires_at);
        }
        self.cap_activity_tools();
    }

    fn expire_running_activity_tools(&mut self, ttl: Duration) {
        let expires_at = std::time::Instant::now() + ttl;
        for tool in &mut self.activity_tools {
            if matches!(
                tool.status,
                crate::surfaces::activity::ActivityToolStatus::Running
            ) {
                tool.status = crate::surfaces::activity::ActivityToolStatus::Cancelled;
                tool.expires_at = Some(expires_at);
            }
        }
        self.cap_activity_tools();
    }

    fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { turn } => {
                self.agent_active = true;
                self.slim_turn_state = SlimTurnState::RequestingProvider;
                if let Ok(mut ss) = self.dashboard_handles.session.lock() {
                    ss.busy = true;
                }
                self.turn = turn;
                self.working_verb = spinner::next_verb();
                self.effects.start_spinner_glow();
                self.effects.start_border_pulse();
            }
            AgentEvent::TurnEnd(te) => {
                self.turn = te.turn;
                if self.runtime_queue_snapshot.is_none() {
                    self.runtime_queue_snapshot = Some(serde_json::json!({
                        "depth": 0,
                        "active": null,
                        "items": [],
                        "previews": [],
                    }));
                }
                let turn_end_reason = te.turn_end_reason;
                self.slim_turn_state = SlimTurnState::Finished(match turn_end_reason {
                    omegon_traits::TurnEndReason::AssistantCompleted => "done",
                    omegon_traits::TurnEndReason::ToolContinuation => "continuing",
                    omegon_traits::TurnEndReason::ProgressNudge => "nudged",
                    omegon_traits::TurnEndReason::Cancelled => "cancelled",
                });
                if matches!(
                    turn_end_reason,
                    omegon_traits::TurnEndReason::AssistantCompleted
                        | omegon_traits::TurnEndReason::Cancelled
                ) {
                    self.agent_active = false;
                    if let Ok(mut ss) = self.dashboard_handles.session.lock() {
                        ss.busy = false;
                    }
                    self.effects.stop_spinner_glow();
                    self.effects.stop_border_pulse();
                }
                if matches!(turn_end_reason, omegon_traits::TurnEndReason::Cancelled) {
                    // Cancellation abandons the in-flight turn, so clear the live workbench
                    // lane. Completed plans are still cleared by PlanUpdated handling; incomplete
                    // plans survive AssistantCompleted so the operator can inspect and continue
                    // the visible work plan between turns.
                    self.workbench_state.active = None;
                }
                // Update session row with behavioral signals
                self.session_row.phase = te.dominant_phase;
                self.session_row.drift = te.drift_kind;
                self.session_row.files_read = te.files_read_count;
                self.session_row.files_modified = te.files_modified_count;
                // Accumulate session-long token counts
                self.footer_data.session_input_tokens += te.actual_input_tokens;
                self.footer_data.session_output_tokens += te.actual_output_tokens;
                self.footer_data.last_turn_input_tokens = te.actual_input_tokens;
                self.footer_data.last_turn_output_tokens = te.actual_output_tokens;
                if (te.actual_input_tokens > 0 || te.actual_output_tokens > 0)
                    && let Some(model_id) = te.model
                {
                    self.footer_data
                        .session_usage_slices
                        .push(SessionUsageSlice {
                            model_id,
                            provider: te.provider.unwrap_or_default(),
                            input_tokens: te.actual_input_tokens,
                            output_tokens: te.actual_output_tokens,
                        });
                }
                // Forward raw token counts to the instrument panel
                self.instrument_panel.update_turn_tokens(
                    te.actual_input_tokens as u32,
                    te.actual_output_tokens as u32,
                    te.cache_read_tokens as u32,
                    te.context_composition.clone(),
                    te.context_window,
                );
                let ctx_window = self.footer_data.context_window;
                if ctx_window > 0 {
                    // Footer context posture is total live-context usage, not the last request's
                    // provider-reported input tokens. ContextUpdated is the authoritative source;
                    // TurnEnd may fill gaps when no prior context snapshot was emitted.
                    let tokens = if te.estimated_tokens > 0 {
                        te.estimated_tokens
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
                    // Context pressure gradient on conversation zone
                    self.effects.set_context_pressure(pct);
                }
                self.footer_data.provider_telemetry = te.provider_telemetry;

                // Stamp the provider-reported actual tokens onto every
                // segment that belongs to this turn so the title-bar
                // annotation (`↑input ↓output` next to the timestamp)
                // shows up across all of them at once. Tool cards,
                // assistant text, and any other segment created during
                // the turn share the same `meta.turn` from
                // `current_meta()` and pick up the stamp here.
                if te.actual_input_tokens > 0 || te.actual_output_tokens > 0 {
                    self.conversation.stamp_turn_tokens(
                        te.turn,
                        segments::TokenUsage {
                            input: te.actual_input_tokens,
                            output: te.actual_output_tokens,
                        },
                    );
                }
                self.effects.ping_footer(self.theme.as_ref());
                // Detect if the agent is asking for confirmation and offer
                // a one-key continuation affordance in the editor.
                self.detect_continuation_request();
            }
            AgentEvent::MessageStart { .. } => {
                self.slim_turn_state = SlimTurnState::OpeningStream;
            }
            AgentEvent::MessageChunk { text } => {
                self.slim_turn_state = SlimTurnState::Responding;
                let was_streaming = self.conversation.is_streaming();
                self.conversation.append_streaming(&text);
                if !was_streaming {
                    // First chunk of a new response — stamp model metadata
                    self.conversation.stamp_meta(self.current_meta());
                }
            }
            AgentEvent::ThinkingChunk { text } => {
                self.slim_turn_state = SlimTurnState::Thinking;
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
                self.slim_turn_state = SlimTurnState::Tool(name.replace('_', " "));
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
                self.tool_inspection_target = Some(ToolInspectionTarget::LiveLatest(id.clone()));
                self.push_activity_tool_start(&id, &name, args_summary.clone());
                self.conversation.push_tool_start_with_expanded(
                    &id,
                    &name,
                    args_summary.as_deref(),
                    detail_args.as_deref(),
                    id.starts_with("shell-"),
                );
                self.conversation.stamp_meta(self.current_meta());
                self.tool_calls += 1;
                self.last_tool_name = Some(name);
            }
            AgentEvent::PermissionRequest {
                tool_name,
                path,
                kind,
                persistence,
                grant_path,
                respond,
            } => {
                self.slim_turn_state = SlimTurnState::Finished("blocked");
                // Show a blocking permission prompt in the TUI.
                let prompt_text = format_permission_prompt(
                    &tool_name,
                    &path,
                    kind,
                    persistence,
                    grant_path.as_deref(),
                );
                self.command_prompt = Some(
                    CommandPrompt::new("Permission required", prompt_text.clone()).with_actions(
                        vec![
                            CommandPromptAction::new("y", "allow once"),
                            CommandPromptAction::new("A", "always allow"),
                            CommandPromptAction::new("n", "deny"),
                        ],
                    ),
                );

                // Store the responder — the next key event (y/a/n) will
                // resolve it. See handle_permission_key below.
                self.pending_permission = Some(respond.clone());
                self.pending_permission_context = Some(PendingPermissionContext {
                    tool_name,
                    target: path,
                    kind,
                    persistence,
                    grant_path,
                });
            }
            AgentEvent::OperatorWaitRequest {
                prompt,
                timeout_secs,
                acknowledge,
                respond,
            } => {
                self.slim_turn_state = SlimTurnState::Finished("waiting");
                let prompt_text = format!(
                    "Manual action required\n   {prompt}\n   [Enter/Space/d] done   [c/Esc] cancel   safety timeout: {timeout_secs}s"
                );
                self.command_prompt = Some(
                    CommandPrompt::new("Manual action required", prompt_text.clone()).with_actions(
                        vec![
                            CommandPromptAction::new("Enter", "done"),
                            CommandPromptAction::new("Space/d", "done"),
                            CommandPromptAction::new("c/Esc", "cancel"),
                        ],
                    ),
                );
                if let Ok(mut slot) = acknowledge.lock()
                    && let Some(tx) = slot.take()
                {
                    let _ = tx.send(());
                }
                self.pending_operator_wait = Some(respond.clone());
                self.pending_operator_wait_context = Some(prompt);
            }
            AgentEvent::ToolEnd {
                id,
                name,
                result,
                is_error,
            } => {
                if name == crate::tool_registry::core::WAIT_FOR_OPERATOR
                    && self.pending_operator_wait.is_some()
                {
                    self.pending_operator_wait = None;
                    self.pending_operator_wait_context = None;
                    self.command_prompt = None;
                }

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
                self.mark_activity_tool_end(
                    &id,
                    is_error,
                    display.map(|text| crate::util::truncate(text, 96)),
                );

                // Visual feedback: error flash or completion pulse
                if is_error {
                    self.effects.flash_error();
                } else {
                    self.effects.pulse_new_card();
                }

                // Detect image results from structured blocks/details first.
                // Text scraping remains as a fallback for legacy render tools.
                if !is_error
                    && result
                        .content
                        .iter()
                        .any(|block| matches!(block, omegon_traits::ContentBlock::Image { .. }))
                {
                    let image_path = result
                        .details
                        .get("path")
                        .or_else(|| result.details.get("output_path"))
                        .and_then(|value| value.as_str())
                        .map(std::path::PathBuf::from)
                        .or_else(|| {
                            full_text.as_ref().and_then(|text| {
                                text.lines().find_map(|line| {
                                    let trimmed = line.trim();
                                    if image::is_image_path(trimmed)
                                        && std::path::Path::new(trimmed).exists()
                                    {
                                        Some(std::path::PathBuf::from(trimmed))
                                    } else {
                                        None
                                    }
                                })
                            })
                        });

                    match (image::is_available(), image_path) {
                        (true, Some(path)) if path.exists() => {
                            self.conversation.push_image(path, "");
                        }
                        (false, Some(path)) => {
                            self.conversation.push_system(&format!(
                                "Image result available, but terminal image rendering is unavailable here: {}",
                                path.display()
                            ));
                        }
                        (_, Some(path)) => {
                            self.conversation.push_system(&format!(
                                "Image result available, but the local render path does not exist: {}",
                                path.display()
                            ));
                        }
                        (_, None) => {
                            self.conversation.push_system(
                                "Image result available, but no local render path was provided.",
                            );
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
                if self
                    .tool_inspection_target
                    .as_ref()
                    .is_some_and(|target| matches!(target, ToolInspectionTarget::LiveLatest(active_id) if active_id == &id))
                {
                    self.tool_inspection_target = None;
                }
                if self.agent_active {
                    self.slim_turn_state = SlimTurnState::RequestingProvider;
                }
            }
            AgentEvent::AgentEnd => {
                self.expire_running_activity_tools(Duration::from_millis(2200));
                self.agent_active = false;
                if !matches!(self.slim_turn_state, SlimTurnState::Finished(_)) {
                    self.slim_turn_state = SlimTurnState::Ready;
                }
                if self.interrupt_pending {
                    self.editor.clear_line();
                    self.interrupt_pending = false;
                    self.suppress_editor_input_for(Duration::from_millis(500));
                }
                if let Ok(mut ss) = self.dashboard_handles.session.lock() {
                    ss.busy = false;
                }
                self.conversation.finalize_message();
                // Keep completed turns anchored at the live tail. The old long-response
                // active-plan heuristic rewound compact sessions to the start of the final
                // assistant segment, which made every completed GPT-5.5 turn land tens
                // of lines above the composer and forced a manual End/scroll recovery.
                self.effects.stop_spinner_glow();
                self.effects.stop_border_pulse();
                self.effects.sweep_turn_complete();
                // Advance tutorial overlay if an AutoPrompt step just completed
                if let Some(ref mut overlay) = self.tutorial_overlay {
                    overlay.on_agent_turn_complete();
                }
            }
            AgentEvent::PhaseChanged { phase } => {
                self.conversation
                    .push_lifecycle("◈", &format!("Phase → {phase:?}"));
            }
            AgentEvent::DecompositionStarted {
                children,
                operation,
            } => {
                let milestone = OperationMilestoneProjection::started(&operation, children.len());
                self.conversation
                    .push_lifecycle(milestone.icon, &milestone.text);
            }
            AgentEvent::DecompositionChildCompleted {
                label,
                success,
                operation,
            } => {
                let milestone =
                    OperationMilestoneProjection::child_completed(&operation, &label, success);
                self.conversation
                    .push_lifecycle(milestone.icon, &milestone.text);
            }
            AgentEvent::DecompositionCompleted { merged, operation } => {
                let milestone = OperationMilestoneProjection::completed(&operation, merged);
                self.conversation
                    .push_lifecycle(milestone.icon, &milestone.text);
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
            AgentEvent::RouteChanged {
                state,
                selected,
                serving,
                warning,
                message,
            } => {
                self.route_state = Some(state.clone());
                self.route_selected_model = selected.clone();
                self.route_serving_model = serving.clone();
                if let Some(serving) = serving.as_ref() {
                    self.footer_data.model_id = serving.clone();
                    self.footer_data.model_provider = crate::providers::infer_provider_id(serving);
                }
                self.footer_data.route_warning = warning.clone().or_else(|| {
                    if state == "serving" {
                        None
                    } else {
                        Some(message.clone())
                    }
                });
                if state == "serving" {
                    self.footer_data.route_warning = None;
                }
                self.show_toast(&message, ratatui_toaster::ToastType::Info);
            }
            AgentEvent::RuntimeQueueUpdated { snapshot_json } => {
                self.runtime_queue_snapshot = Some(snapshot_json);
            }
            AgentEvent::RuntimeTurnLifecycleUpdated { .. } => {}
            AgentEvent::RuntimePromptStarted { text, image_paths } => {
                if image_paths.is_empty() {
                    self.conversation.push_user(&text);
                } else {
                    self.conversation
                        .push_user_with_attachments(&text, &image_paths);
                }
            }
            AgentEvent::SkillActivation { event } => {
                let mut parts = vec![
                    format!("skill active: {}", event.active_ref),
                    event.resolution.clone(),
                ];
                if let Some(activation) = event.activation.as_ref()
                    && !activation.is_empty()
                {
                    parts.push(activation.clone());
                }
                if !event.matched_signals.is_empty() {
                    parts.push(format!("matched {}", event.matched_signals.join(", ")));
                }
                if !event.suppressing.is_empty() {
                    parts.push(format!("suppressing {}", event.suppressing.join(", ")));
                }
                if let Some(recommendation) = event.recommendation.as_ref()
                    && !recommendation.is_empty()
                {
                    parts.push(recommendation.clone());
                }
                let glyph =
                    crate::tui::glyphs::glyphs().engine(crate::tui::glyphs::EngineGlyphRole::Skill);
                self.conversation
                    .push_system(&format!("{glyph} {}", parts.join(" · ")));
            }
            AgentEvent::OperatorCopyBlock {
                label,
                text,
                kind,
                copy_attempt,
            } => {
                self.conversation
                    .push_operator_copy_block(label, text, kind, copy_attempt);
            }
            AgentEvent::SystemNotification { message } => {
                if let Some(detail) = upstream_retry_hint(&message) {
                    self.slim_turn_state = SlimTurnState::UpstreamRetrying(detail);
                }
                // Transient retry notifications → toast (operator sees them but they
                // don't clutter the conversation). Milestone warnings and other
                // persistent messages → conversation.
                if message.starts_with('⟳')
                    || message.starts_with("Retrying")
                    || message.contains("— retrying")
                {
                    self.show_toast(&message, ratatui_toaster::ToastType::Warning);
                } else if message.starts_with('↯') || is_one_shot_context_notification(&message) {
                    self.show_toast(&message, ratatui_toaster::ToastType::Info);
                } else if let Some(command) = slash_command_for_palette_notification(&message) {
                    self.open_command_panel(CommandPanel::from_slash(command, &message));
                } else {
                    self.conversation.push_system(&message);
                }
            }
            AgentEvent::PlanUpdated { projection } => {
                if WorkbenchState::is_workstream_only_projection(&projection) {
                    self.workbench_state
                        .merge_workstream_projection(&projection);
                    return;
                }
                let dock_state = WorkbenchState::from_plan_projection(&projection);
                self.completed_plan_history_available = dock_state
                    .active
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.is_complete())
                    || self.completed_plan_history_available;
                if let Some(snapshot) = dock_state.active.as_ref()
                    && snapshot.is_complete()
                {
                    let latest_is_complete = self
                        .conversation
                        .latest_plan_progress()
                        .and_then(PlanDisplaySnapshot::from_legacy_text)
                        .is_some_and(|latest| latest.is_complete());
                    if !latest_is_complete {
                        self.conversation
                            .push_system(&snapshot.system_notification_text("Plan progress"));
                    }
                    self.conversation.snap_to_bottom();
                    self.dashboard_handles.cleave = None;
                    self.dashboard_handles.delegate = None;
                    self.dashboard.cleave = None;
                    self.dashboard.delegate = None;
                    self.instrument_panel.set_cleave_progress(None);
                    let refreshed_workspace = self.current_workbench_workspace_context();
                    self.workbench_state = WorkbenchState {
                        active: None,
                        workstreams: dock_state.workstreams,
                        workspace: if refreshed_workspace.has_visible_context() {
                            refreshed_workspace
                        } else {
                            self.workbench_state.workspace.clone()
                        },
                    };
                } else {
                    self.workbench_state.active = dock_state.active;
                    self.workbench_state.workstreams = dock_state.workstreams;
                }
            }
            AgentEvent::SessionReset => {
                self.conversation = ConversationView::new();
                self.workbench_state.active = None;
                self.completed_plan_history_available = false;
                self.tool_inspection_target = None;
                self.activity_tools.clear();
                self.turn = 0;
                self.tool_calls = 0;
                self.last_tool_name = None;
                self.completed_tool_name = None;
                self.command_panel = None;
                self.command_prompt = None;
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
                    self.workbench_state.workspace = self.current_workbench_workspace_context();

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
            AgentEvent::MessageAbort { reason } => {
                self.expire_running_activity_tools(Duration::from_secs(4));
                self.conversation.abort_streaming();
                match reason.as_deref() {
                    Some("interrupted · kept") => {
                        self.slim_turn_state = SlimTurnState::InterruptedKept;
                    }
                    Some("aborted · forgotten") => {
                        self.slim_turn_state = SlimTurnState::AbortedForgotten;
                    }
                    _ => {}
                }
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
    /// Present when a prior session was resumed; retained for runtime context.
    pub resume_info: Option<crate::setup::ResumeInfo>,
    /// Pre-populated initial state so the first frame isn't empty.
    pub initial: TuiInitialState,
    /// Skip the splash animation on startup.
    pub no_splash: bool,
    /// Command definitions from bus features — shown in command palette.
    pub bus_commands: Vec<omegon_traits::CommandDefinition>,
    /// Runtime substrate generation shown in restart diagnostics.
    pub runtime_generation: u64,
    /// Startup/runtime substrate inventory for restart diagnostics.
    pub runtime_inventory: crate::setup::RuntimeSubstrateInventory,
    /// Metadata-only secret readiness snapshot for the /secrets inventory menu.
    pub secret_readiness: Option<crate::capabilities::secrets::SecretReadinessSnapshot>,
    /// Skill activation/resolution events emitted while startup augments loaded.
    pub startup_skill_activation_events: Vec<omegon_traits::SkillActivationEvent>,
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
    /// Voice notification receivers — one per voice-capable extension.
    pub voice_notification_receivers:
        Vec<tokio::sync::mpsc::UnboundedReceiver<crate::extensions::ExtensionNotification>>,
    /// Voice idle notification pumps — one per voice-capable extension.
    pub voice_polling_handles: Vec<crate::extensions::ExtensionPollingHandle>,
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

/// Handle `/editor` subcommands — IDE integration setup and status.
fn handle_editor_command(args: &str) -> String {
    let omegon_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "omegon".to_string());

    match args.split_whitespace().next().unwrap_or("") {
        "zed" => {
            // Auto-configure Zed's settings.json with omegon as an agent server
            let config_path = dirs::home_dir()
                .map(|h| h.join(".config/zed/settings.json"))
                .unwrap_or_default();

            let omegon_entry = serde_json::json!({
                "type": "custom",
                "command": omegon_bin,
                "args": ["acp"],
                "env": {}
            });

            let mut result_lines = Vec::new();

            if config_path.exists() {
                // Read existing settings, merge our agent_servers entry
                let content = std::fs::read_to_string(&config_path).unwrap_or_default();
                let mut settings: serde_json::Value =
                    serde_json::from_str(&content).unwrap_or(serde_json::json!({}));

                let servers = settings
                    .as_object_mut()
                    .unwrap()
                    .entry("agent_servers")
                    .or_insert_with(|| serde_json::json!({}));

                if let Some(obj) = servers.as_object_mut() {
                    if obj.contains_key("Omegon") {
                        result_lines.push("Zed settings already contain Omegon agent.".to_string());
                    } else {
                        obj.insert("Omegon".to_string(), omegon_entry);
                        let json = serde_json::to_string_pretty(&settings).unwrap_or_default();
                        if std::fs::write(&config_path, &json).is_ok() {
                            result_lines
                                .push(format!("✓ Added Omegon to {}", config_path.display()));
                        } else {
                            result_lines
                                .push(format!("✗ Failed to write {}", config_path.display()));
                        }
                    }
                }
            } else {
                // Create settings.json from scratch
                let settings = serde_json::json!({
                    "agent_servers": {
                        "Omegon": omegon_entry
                    }
                });
                let _ = std::fs::create_dir_all(
                    config_path.parent().unwrap_or(std::path::Path::new(".")),
                );
                let json = serde_json::to_string_pretty(&settings).unwrap_or_default();
                if std::fs::write(&config_path, &json).is_ok() {
                    result_lines.push(format!(
                        "✓ Created {} with Omegon agent",
                        config_path.display()
                    ));
                } else {
                    result_lines.push(format!("✗ Failed to create {}", config_path.display()));
                }
            }

            // Try to launch Zed — check CLI first, then macOS app bundle
            let launched = if std::process::Command::new("zed").arg(".").spawn().is_ok() {
                true
            } else {
                cfg!(target_os = "macos")
                    && std::process::Command::new("open")
                        .args(["-a", "Zed", "."])
                        .spawn()
                        .is_ok()
            };

            if launched {
                result_lines.push("✓ Launching Zed...".to_string());
                result_lines.push("  Select Omegon from the Agent Panel (+ button).".to_string());
            } else {
                result_lines.push("Zed not found on PATH or in /Applications.".to_string());
                result_lines.push(
                    "Install from https://zed.dev or run: brew install --cask zed".to_string(),
                );
            }

            result_lines.push(
                "\nModes: Code (Fabricator) | Architect | Ask (Explorator) | Agent (Devastator)"
                    .to_string(),
            );

            result_lines.join("\n")
        }
        "vscode" => "VS Code Integration\n\n\
             1. Install the vscode-acp extension:\n\
                https://github.com/formulahendry/vscode-acp\n\n\
             2. Add to VS Code settings.json:\n\n\
             {\n  \
               \"acp.agents\": [\n    \
                 {\n      \
                   \"id\": \"omegon\",\n      \
                   \"name\": \"Omegon\",\n      \
                   \"command\": \"omegon\",\n      \
                   \"args\": [\"acp\"]\n    \
                 }\n  \
               ]\n\
             }\n\n\
             3. Restart VS Code and open the ACP panel."
            .to_string(),
        "status" => {
            let mut lines = vec!["Editor Integration Status\n".to_string()];
            lines.push(format!("  Binary: {omegon_bin}"));
            lines.push("  ACP: omegon acp".to_string());

            // Check if Zed is installed (CLI or macOS app bundle)
            let has_zed_cli = std::process::Command::new("zed")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok();
            let has_zed_app = cfg!(target_os = "macos")
                && std::process::Command::new("open")
                    .args(["-Ra", "Zed"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
            let zed_status = if has_zed_cli {
                "installed (CLI on PATH)"
            } else if has_zed_app {
                "installed (app bundle, CLI not on PATH — run Zed > Install CLI)"
            } else {
                "not found"
            };
            lines.push(format!("  Zed: {zed_status}"));

            // Check if VS Code is installed
            let has_code = std::process::Command::new("code")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok();
            lines.push(format!(
                "  VS Code: {}",
                if has_code { "installed" } else { "not found" }
            ));

            lines.push("\nRun /editor zed or /editor vscode for setup instructions.".to_string());
            lines.join("\n")
        }
        "" => "Editor Integration\n\n\
             /editor zed      Setup instructions for Zed\n\
             /editor vscode   Setup instructions for VS Code\n\
             /editor status   Check installed editors\n\n\
             Omegon integrates with editors via the Agent Client Protocol (ACP).\n\
             The editor spawns `omegon acp` and communicates via JSON-RPC over stdio."
            .to_string(),
        other => {
            format!("Unknown editor: {other}\n\nSupported: zed, vscode\nRun /editor for help.")
        }
    }
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
    // Mouse capture is ON by default: trackpad/wheel scrolling must be owned by
    // the conversation view. `/mouse off` is the explicit opt-in escape hatch
    // for terminal-native drag selection during the current session.
    io::stdout().execute(EnableMouseCapture)?;
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

    // Mouse capture starts enabled because two-finger/trackpad scrolling is a
    // conversation-view invariant. `/mouse off` temporarily gives drag selection
    // back to the terminal for this session.
    let mut app = App::new(settings.clone());
    app.mouse_capture_enabled = true;
    app.keyboard_enhancement = has_keyboard_enhancement;
    app.secret_readiness = config.secret_readiness.clone();
    app.show_startup_notice();
    // Populate extension widgets and receivers from config
    for widget in config.extension_widgets {
        app.extension_widgets
            .insert(widget.widget_id.clone(), widget);
    }
    app.widget_receivers = config.widget_receivers;
    app.voice_notification_receivers = config.voice_notification_receivers;
    for handle in config.voice_polling_handles {
        tokio::spawn(async move {
            loop {
                if handle
                    .pump_notifications_for(std::time::Duration::from_millis(250))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    for mut rx in std::mem::take(&mut app.voice_notification_receivers) {
        let tx = command_tx.clone();
        tokio::spawn(async move {
            while let Some(notification) = rx.recv().await {
                if let Some(cmd) = voice_prompt_from_notification(&notification)
                    && tx.send(cmd).await.is_err()
                {
                    break;
                }
            }
        });
    }
    app.history = App::load_history(&config.cwd);
    app.footer_data.cwd = config.cwd.clone();
    // Load skills from ~/.omegon/skills/ (bundled) and .omegon/skills/ (project-local).
    if let Some(ref mut registry) = app.augment_registry {
        registry.load_skills(std::path::Path::new(&config.cwd));
    }
    app.footer_data.is_oauth = config.is_oauth;
    for event in &config.startup_skill_activation_events {
        app.conversation.push_skill_event(event);
    }
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
    crate::update::spawn_polling(update_tx, app.settings.clone());
    app.login_prompt_tx = config.login_prompt_tx;

    // Default to slim/conversation-first startup. Operators can elevate
    // to the full harness via /ui full, /unshackle, or /warp.
    app.apply_ui_preset(UiSurfaces::lean());
    if !app.settings().is_slim()
        && let Ok(mut s) = app.settings.lock()
    {
        s.set_posture(crate::settings::PosturePreset::Explorator);
    }

    // Pre-populate from initial state so first frame isn't empty
    app.footer_data.total_facts = config.initial.total_facts;
    app.dashboard.focused_node = config.initial.focused_node;
    app.dashboard.active_changes = config.initial.active_changes;

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
                    if let AgentEvent::HarnessStatusChanged { status_json } = ev
                        && let Ok(status) =
                            serde_json::from_value::<crate::status::HarnessStatus>(status_json)
                    {
                        app.footer_data.update_harness(status);
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
            // Classify startup capability tier from ALL collected results
            app.capability_grade = Some(crate::startup::classify_tier(&collected_probes));
        }
    }

    // Queue startup reveal effects (footer sweep-in, conversation fade)

    // Queue initial prompt if provided (--initial-prompt / --initial-prompt-file)
    if let Some(prompt) = config.initial_prompt {
        let _ = command_tx
            .send(TuiCommand::SubmitPrompt(PromptSubmission {
                text: prompt,
                image_paths: Vec::new(),
                submitted_by: "startup".to_string(),
                via: "tui",
                queue_mode: app.queue_mode,
                metadata: PromptMetadata::default(),
            }))
            .await;
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

        if let Some(rx) = &app.smoke_event_rx {
            let mut smoke_events = Vec::new();
            let mut smoke_disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(event) => smoke_events.push(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        smoke_disconnected = true;
                        break;
                    }
                }
            }
            for event in smoke_events {
                app.handle_agent_event(event);
            }
            if smoke_disconnected {
                app.smoke_event_rx = None;
            }
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
                            app.dashboard.sidebar_active = true;
                        } else if point_in(app.workbench_area) {
                            app.dashboard.sidebar_active = false;
                            let now = std::time::Instant::now();
                            let is_double = app.last_left_click.is_some_and(|(col, row, t)| {
                                row == mouse.row
                                    && col.abs_diff(mouse.column) <= 1
                                    && row.abs_diff(mouse.row) <= 1
                                    && now.duration_since(t) <= Duration::from_millis(400)
                            });
                            if is_double {
                                app.expand_workbench_plan_details();
                            }
                            app.last_left_click = Some((mouse.column, mouse.row, now));
                        } else if point_in(app.conversation_area) {
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
                                let _ = app.handle_select_conversation_segment_action(
                                    SelectConversationSegmentAction {
                                        segment: ConversationSegmentRef::by_index(idx),
                                    },
                                );
                                if is_double {
                                    if app.conversation.is_segment_collapsed_tool_card(idx) {
                                        app.conversation.toggle_expand(idx);
                                        app.show_toast(
                                            "Expanded selected tool result",
                                            ratatui_toaster::ToastType::Success,
                                        );
                                        app.effects.pulse_conversation_action();
                                    } else if app.conversation.is_segment_copyable(idx) {
                                        app.copy_selected_conversation_segment_with_mode(
                                            SegmentExportMode::Plaintext,
                                        );
                                    }
                                }
                                app.last_left_click = Some((mouse.column, mouse.row, now));
                            }
                        } else if point_in(app.editor_area) {
                            app.dashboard.sidebar_active = false;
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        // Mouse wheel is scroll provenance, not keyboard Up.
                        // It must never route through editor history recall.
                        app.handle_mouse_scroll_up(mouse.column, mouse.row);
                    }
                    MouseEventKind::ScrollDown => {
                        // Mouse wheel is scroll provenance, not keyboard Down.
                        // It must never route through editor history advance/clear.
                        app.handle_mouse_scroll_down(mouse.column, mouse.row);
                    }
                    _ => {}
                },
                // ── Paste — pass directly to textarea ──────────
                Event::Paste(ref text) => {
                    if app.editor_input_suppressed() {
                        continue;
                    }
                    if matches!(app.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                        // In secret mode, paste goes into the hidden buffer
                        for c in text.chars() {
                            app.editor.secret_insert(c);
                        }
                    } else if text.is_empty() {
                        app.pending_history_preload = None;
                        app.try_paste_clipboard_image();
                    } else {
                        let _ = app
                            .handle_ui_action(
                                UiAction::InsertComposerText(InsertComposerTextAction {
                                    text: text.clone(),
                                }),
                                &command_tx,
                            )
                            .await;
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
                    // Blocking responder-backed prompts own input before passive panels,
                    // scrollback controls, selectors, or editor actions.
                    if app.pending_operator_wait.is_some() {
                        let response = match key.code {
                            KeyCode::Enter
                            | KeyCode::Char(' ')
                            | KeyCode::Char('d')
                            | KeyCode::Char('D') => {
                                Some(omegon_traits::OperatorWaitResponse::Completed)
                            }
                            KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                                Some(omegon_traits::OperatorWaitResponse::Cancelled)
                            }
                            _ => None,
                        };
                        if let Some(response) = response {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::RespondToOperatorWait(OperatorWaitAction {
                                        request_id: None,
                                        response,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        continue;
                    }

                    if app.pending_permission.is_some() {
                        let response = permission_response_for_key(key.code, key.modifiers);
                        if let Some(response) = response {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::RespondToPermission(PermissionAction {
                                        request_id: None,
                                        response,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        continue;
                    }

                    if let Some(copy_modal) = app.copy_text_modal.as_mut() {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                app.close_copy_text_modal();
                                continue;
                            }
                            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                                let _ = app.copy_all_from_copy_text_modal();
                                continue;
                            }
                            (KeyCode::Up, _) => {
                                copy_modal.scroll_up(1);
                                continue;
                            }
                            (KeyCode::Down, _) => {
                                copy_modal.scroll_down(1);
                                continue;
                            }
                            (KeyCode::PageUp, _) => {
                                copy_modal.scroll_up(20);
                                continue;
                            }
                            (KeyCode::PageDown, _) => {
                                copy_modal.scroll_down(20);
                                continue;
                            }
                            (KeyCode::Home, _) => {
                                copy_modal.scroll_top();
                                continue;
                            }
                            (KeyCode::End, _) => {
                                copy_modal.scroll_bottom();
                                continue;
                            }
                            _ => {}
                        }
                    }

                    if let Some(panel) = app.command_panel.as_mut() {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                app.close_command_panel_to_return_target();
                                continue;
                            }
                            (KeyCode::Char('q'), _) if panel.return_target.is_some() => {
                                app.close_command_panel_stack();
                                continue;
                            }
                            (KeyCode::Up, _) => {
                                panel.scroll_up(3);
                                continue;
                            }
                            (KeyCode::Down, _) => {
                                panel.scroll_down(3);
                                continue;
                            }
                            (KeyCode::PageUp, _) => {
                                panel.scroll_up(20);
                                continue;
                            }
                            (KeyCode::PageDown, _) => {
                                panel.scroll_down(20);
                                continue;
                            }
                            (KeyCode::Home, _) => {
                                panel.scroll_top();
                                continue;
                            }
                            (KeyCode::End, _) => {
                                panel.scroll_bottom();
                                continue;
                            }
                            (KeyCode::Char('y'), KeyModifiers::CONTROL) if panel.copyable => {
                                let text = panel.body.clone();
                                if app.copy_text_to_clipboard(&text) {
                                    app.show_toast(
                                        "Copied command panel",
                                        ratatui_toaster::ToastType::Success,
                                    );
                                } else {
                                    app.show_toast(
                                        "Clipboard unavailable — select panel text in your terminal or install pbcopy/wl-copy/xclip",
                                        ratatui_toaster::ToastType::Warning,
                                    );
                                }
                                continue;
                            }
                            _ => {}
                        }
                    }

                    // Global conversation controls must remain live while the
                    // agent/tool loop is active. Handle them before editor,
                    // selector, permission, or interrupt-debounce paths can
                    // consume the key event.
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                            app.conversation.toggle_pin();
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

                    if app.should_discard_key_after_interrupt(&key) {
                        continue;
                    }

                    // ── Structured menu intercepts navigation when open ────
                    if app.active_menu.is_some() {
                        if matches!(key.code, KeyCode::Esc)
                            && app.should_discard_key_after_interrupt(&key)
                        {
                            continue;
                        }
                        match key.code {
                            KeyCode::Up => {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.move_up();
                                }
                            }
                            KeyCode::Down => {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.move_down(&menu.projection);
                                }
                            }
                            KeyCode::Tab => {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.next_tab(&menu.projection);
                                }
                            }
                            KeyCode::BackTab => {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.previous_tab(&menu.projection);
                                }
                            }
                            KeyCode::Char('/') => {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.enter_search();
                                }
                            }
                            KeyCode::Char(ch)
                                if app
                                    .active_menu
                                    .as_ref()
                                    .is_some_and(|menu| menu.state.mode == MenuMode::Search)
                                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                                    && !key.modifiers.contains(KeyModifiers::ALT) =>
                            {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.push_filter_char(&menu.projection, ch);
                                }
                            }
                            KeyCode::Backspace
                                if app
                                    .active_menu
                                    .as_ref()
                                    .is_some_and(|menu| menu.state.mode == MenuMode::Search) =>
                            {
                                if let Some(menu) = app.active_menu.as_mut() {
                                    menu.state.pop_filter_char(&menu.projection);
                                }
                            }
                            KeyCode::Char('s') | KeyCode::Char('S')
                                if app
                                    .active_menu
                                    .as_ref()
                                    .is_some_and(|menu| menu.projection.id == "settings") =>
                            {
                                app.queue_settings_profile_save(&command_tx);
                            }
                            KeyCode::Char('a') | KeyCode::Char('A')
                                if app
                                    .active_menu
                                    .as_ref()
                                    .is_some_and(|menu| menu.projection.id == "settings") =>
                            {
                                app.queue_settings_profile_apply(&command_tx);
                            }
                            KeyCode::Char(ch)
                                if app
                                    .active_menu
                                    .as_ref()
                                    .is_some_and(|menu| menu.state.mode != MenuMode::Search)
                                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                                    && !key.modifiers.contains(KeyModifiers::ALT) =>
                            {
                                let action = app.active_menu.as_ref().and_then(|menu| {
                                    menu.state.selected_action_for_key(&menu.projection, ch)
                                });
                                if let Some(action) = action
                                    && matches!(
                                        app.execute_active_menu_action(action, &command_tx),
                                        SlashResult::Quit
                                    )
                                {
                                    let _ = command_tx.send(TuiCommand::Quit).await;
                                }
                            }
                            KeyCode::Enter => {
                                let action = app.active_menu.as_ref().and_then(|menu| {
                                    menu.state.selected_primary_action(&menu.projection)
                                });
                                if let Some(action) = action
                                    && matches!(
                                        app.execute_active_menu_action(action, &command_tx),
                                        SlashResult::Quit
                                    )
                                {
                                    let _ = command_tx.send(TuiCommand::Quit).await;
                                }
                            }
                            KeyCode::Esc => {
                                let handled = app
                                    .active_menu
                                    .as_mut()
                                    .is_some_and(|menu| menu.state.exit_search());
                                if !handled {
                                    app.active_menu = None;
                                    app.pending_menu_confirmation = None;
                                }
                            }
                            KeyCode::Char('c') | KeyCode::Char('C')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                app.active_menu = None;
                            }
                            _ => {}
                        }
                        continue;
                    }

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
                                    app.show_toast(&msg, ratatui_toaster::ToastType::Info);
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
                                        app.show_command_toast(CommandToast::new(
                                            "Cancelled — no value entered",
                                            CommandSeverity::Warning,
                                        ));
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
                                        // provider resolution chain finds them (/auth login checks
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
                                app.show_command_toast(CommandToast::new(
                                    "Secret input cancelled",
                                    CommandSeverity::Info,
                                ));
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
                    if let Some(ref mut overlay) = app.tutorial_overlay
                        && overlay.active
                    {
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
                                        let auto_prompt =
                                            overlay.pending_auto_prompt().map(|s| s.to_string());
                                        if auto_prompt.is_some() {
                                            overlay.mark_auto_prompt_sent();
                                        }
                                        // overlay borrow is released before touching app
                                        if let Some(prompt) = auto_prompt {
                                            if !app.agent_active {
                                                app.show_command_toast(CommandToast::new(
                                                    "Tutorial step started",
                                                    CommandSeverity::Info,
                                                ));
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
                                                            submitted_by: "local-tui".to_string(),
                                                            via: "tui",
                                                            queue_mode: app.queue_mode,
                                                            metadata: PromptMetadata::default(),
                                                        },
                                                    ))
                                                    .await;
                                            } else {
                                                let _ = command_tx
                                                    .send(TuiCommand::SubmitPrompt(
                                                        PromptSubmission {
                                                            text: prompt,
                                                            image_paths: Vec::new(),
                                                            submitted_by: "local-tui".to_string(),
                                                            via: "tui",
                                                            queue_mode: app.queue_mode,
                                                            metadata: PromptMetadata::default(),
                                                        },
                                                    ))
                                                    .await;
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
                                                app.show_command_toast(CommandToast::new(
                                                    "Tutorial step started",
                                                    CommandSeverity::Info,
                                                ));
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
                                                            submitted_by: "local-tui".to_string(),
                                                            via: "tui",
                                                            queue_mode: app.queue_mode,
                                                            metadata: PromptMetadata::default(),
                                                        },
                                                    ))
                                                    .await;
                                            } else {
                                                let _ = command_tx
                                                    .send(TuiCommand::SubmitPrompt(
                                                        PromptSubmission {
                                                            text: prompt,
                                                            image_paths: Vec::new(),
                                                            submitted_by: "local-tui".to_string(),
                                                            via: "tui",
                                                            queue_mode: app.queue_mode,
                                                            metadata: PromptMetadata::default(),
                                                        },
                                                    ))
                                                    .await;
                                            }
                                        }
                                        // If already sent, Tab does nothing — wait for agent
                                        continue;
                                    }
                                    tutorial::Trigger::Command(_) | tutorial::Trigger::AnyInput => {
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
                                    tutorial::Trigger::Command(_) | tutorial::Trigger::AnyInput => {
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
                    if let Some((widget_id, actions)) = &app.active_action_prompt
                        && let KeyCode::Char(c) = key.code
                        && let Some(digit) = c.to_digit(10)
                    {
                        let idx = (digit - 1) as usize;
                        if idx < actions.len() {
                            let action = actions[idx].clone();
                            // Log the action selection. The response
                            // path to the extension is not yet wired —
                            // when an extension needs bidirectional action
                            // handling, add a TuiCommand::WidgetAction
                            // variant that routes through the bus to the
                            // owning ExtensionFeature's rpc_call.
                            app.show_command_toast(CommandToast::new(
                                format!("{}: {}", widget_id, action),
                                CommandSeverity::Success,
                            ));
                            app.active_action_prompt = None;
                            continue;
                        }
                    }

                    match (key.code, key.modifiers) {
                        // ── Interrupt: Escape or Ctrl+C ─────────────────
                        (KeyCode::Esc, _) => {
                            // Dismiss modal if active, otherwise interrupt agent
                            if app.copy_text_modal.is_some() {
                                app.close_copy_text_modal();
                            } else if app.command_panel.is_some() {
                                app.close_command_panel_to_return_target();
                            } else if app.active_modal.is_some() {
                                app.active_modal = None;
                                if app.terminal_copy_mode {
                                    app.set_terminal_copy_mode(false);
                                }
                            } else if app.active_action_prompt.is_some() {
                                app.active_action_prompt = None;
                            } else if app.agent_active {
                                let outcome = app
                                    .handle_ui_action(UiAction::CancelActiveTurn, &command_tx)
                                    .await;
                                if matches!(outcome, UiActionOutcome::Accepted { .. }) {
                                    app.show_command_toast(CommandToast::new(
                                        "Interrupt requested — waiting for turn to stop",
                                        CommandSeverity::Warning,
                                    ));
                                } else {
                                    app.show_command_toast(CommandToast::new(
                                        "Interrupt requested",
                                        CommandSeverity::Warning,
                                    ));
                                }
                            }
                        }
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            if app.agent_active {
                                let outcome = app
                                    .handle_ui_action(UiAction::CancelActiveTurn, &command_tx)
                                    .await;
                                if matches!(outcome, UiActionOutcome::Accepted { .. }) {
                                    app.show_command_toast(CommandToast::new(
                                        "Interrupt requested (Ctrl+C) — waiting for turn to stop",
                                        CommandSeverity::Warning,
                                    ));
                                } else {
                                    app.show_command_toast(CommandToast::new(
                                        "Interrupt requested (Ctrl+C)",
                                        CommandSeverity::Warning,
                                    ));
                                }
                            } else if !app.editor.is_empty() {
                                // Clear the line first (like a real terminal)
                                app.pending_history_preload = None;
                                app.editor.clear_line();
                                app.last_ctrl_c = None;
                            } else {
                                app.pending_history_preload = None;
                                // Empty editor — double Ctrl+C to quit
                                let now = std::time::Instant::now();
                                if let Some(last) = app.last_ctrl_c {
                                    if now.duration_since(last).as_millis() < 1000 {
                                        app.should_quit = true;
                                        let _ = command_tx.send(TuiCommand::Quit).await;
                                    } else {
                                        app.last_ctrl_c = Some(now);
                                        app.show_command_toast(CommandToast::new(
                                            "Press Ctrl+C again to quit",
                                            CommandSeverity::Info,
                                        ));
                                    }
                                } else {
                                    app.last_ctrl_c = Some(now);
                                    app.show_command_toast(CommandToast::new(
                                        "Press Ctrl+C again to quit",
                                        CommandSeverity::Info,
                                    ));
                                }
                            }
                        }

                        // ── Editor: word/line operations (idle only) ────
                        (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::DeleteWordBackward,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::ClearLine,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::KillToEnd,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Char('Y'), KeyModifiers::CONTROL) => {
                            app.copy_latest_assistant_response(SegmentExportMode::Plaintext);
                        }
                        (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                            app.editor.yank();
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
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::DeleteWordBackward,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Char('d'), KeyModifiers::ALT) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::DeleteWordForward,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Char('b'), KeyModifiers::ALT) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Backward,
                                        unit: ComposerCursorUnit::Word,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Char('f'), KeyModifiers::ALT) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Forward,
                                        unit: ComposerCursorUnit::Word,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }

                        // Ctrl+O: toggle the unified tool inspection target.
                        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                            if matches!(
                                app.tool_inspection_target,
                                Some(ToolInspectionTarget::Pinned(_))
                            ) {
                                app.tool_inspection_target = None;
                            } else if let Some(id) = app.conversation.latest_expandable_tool_id() {
                                app.tool_inspection_target = Some(ToolInspectionTarget::Pinned(id));
                            }
                        }

                        // Ctrl+G: toggle UI preset (lean ↔ full).
                        (KeyCode::Char('g'), KeyModifiers::CONTROL) => {
                            let next = app.ui_surfaces.toggle_preset();
                            let name = next.preset_name();
                            app.apply_ui_preset(next);
                            app.show_toast(
                                &format!("UI → {name}"),
                                ratatui_toaster::ToastType::Info,
                            );
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

                        // Tab: command completion, @-picker insertion, or inline tool-detail toggle.
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
                                    app.editor.set_text(&matches[0].command);
                                }
                            } else if text.is_empty() {
                                if matches!(
                                    app.tool_inspection_target,
                                    Some(ToolInspectionTarget::Pinned(_))
                                ) {
                                    app.tool_inspection_target = None;
                                } else if let Some(id) =
                                    app.conversation.latest_expandable_tool_id()
                                {
                                    app.tool_inspection_target =
                                        Some(ToolInspectionTarget::Pinned(id));
                                }
                            }
                        }

                        // Shift+Tab: collapse the pinned tool detail row.
                        (KeyCode::BackTab, _) => {
                            app.tool_inspection_target = None;
                        }

                        // Alt+N: next conversation tab
                        (KeyCode::Char('n'), KeyModifiers::ALT)
                            if app.conversation.tabs.tabs.len() > 1 =>
                        {
                            app.conversation.tabs.next_tab();
                        }

                        // Alt+P: previous conversation tab
                        (KeyCode::Char('p'), KeyModifiers::ALT)
                            if app.conversation.tabs.tabs.len() > 1 =>
                        {
                            app.conversation.tabs.prev_tab();
                        }

                        // Shift+Enter or Alt+Enter: insert newline (multiline input)
                        (KeyCode::Enter, m)
                            if m.contains(KeyModifiers::SHIFT) || m.contains(KeyModifiers::ALT) =>
                        {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::InsertNewline,
                                    }),
                                    &command_tx,
                                )
                                .await;
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
                            let _ = app
                                .handle_ui_action(
                                    UiAction::InsertComposerText(InsertComposerTextAction {
                                        text: c.to_string(),
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Backspace, _) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::EditComposer(EditComposerAction {
                                        operation: ComposerEditOperation::DeleteBackward,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Left, KeyModifiers::ALT) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Backward,
                                        unit: ComposerCursorUnit::Word,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Right, KeyModifiers::ALT) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Forward,
                                        unit: ComposerCursorUnit::Word,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Left, _) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Backward,
                                        unit: ComposerCursorUnit::Character,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Right, _) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Forward,
                                        unit: ComposerCursorUnit::Character,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::Home, _) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::Home,
                                        unit: ComposerCursorUnit::Line,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }
                        (KeyCode::End, _) => {
                            let _ = app
                                .handle_ui_action(
                                    UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                        direction: ComposerCursorDirection::End,
                                        unit: ComposerCursorUnit::Line,
                                    }),
                                    &command_tx,
                                )
                                .await;
                        }

                        // ── Scrolling ────────────────────────────────
                        (KeyCode::Up, KeyModifiers::SHIFT) => {
                            app.conversation.scroll_up(3);
                        }
                        (KeyCode::Down, KeyModifiers::SHIFT) => {
                            app.conversation.scroll_down(3);
                        }
                        (KeyCode::Up, KeyModifiers::ALT) => {
                            app.history_recall_up();
                        }
                        (KeyCode::Down, KeyModifiers::ALT) => {
                            app.history_recall_down();
                        }
                        (KeyCode::PageUp, _) => {
                            app.conversation.scroll_up(20);
                        }
                        (KeyCode::PageDown, _) => {
                            app.conversation.scroll_down(20);
                        }
                        (KeyCode::Up, _) => {
                            app.handle_keyboard_up();
                        }
                        (KeyCode::Down, _) => {
                            app.handle_keyboard_down();
                        }
                        _ => {}
                    }
                } // Event::Key
                _ => {} // Other events (resize, etc.)
            } // match event::read()
        } // if has_terminal_event

        if app.should_quit {
            break;
        }
    }

    // Stop session-scoped background processes
    crate::tools::serve::cleanup_session_services();
    crate::tools::terminal::cleanup_session_terminals();

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
    fn command_copy_marks_auspex_primary_without_dash_autocomplete() {
        assert!(
            crate::command_registry::BUILTIN_COMMANDS
                .iter()
                .all(|command| command.name != "dash"),
            "/dash is a hidden compatibility/debug handler, not an autocomplete command"
        );

        let auspex = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "auspex")
            .expect("/auspex command must exist");
        assert!(auspex.description.contains("primary"));
        assert!(auspex.description.contains("Auspex"));
        assert!(auspex.description.contains("open"));
    }

    #[test]
    fn validate_errors_get_actionable_recovery_hint() {
        let hint = App::recovery_hint(
            Some("validate"),
            "supported source types: rust python typescript; unsupported file docs/readme.md",
        );
        assert!(hint.contains("project-specific test"));
        assert!(!hint.is_empty());
    }
}

#[cfg(test)]
mod voice_prompt_tests {
    use super::*;
    use serde_json::json;

    fn notification(
        method: &str,
        params: serde_json::Value,
    ) -> crate::extensions::ExtensionNotification {
        crate::extensions::ExtensionNotification {
            extension_name: "voice".to_string(),
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn voice_transcription_notification_becomes_voice_prompt() {
        let cmd = voice_prompt_from_notification(&notification(
            "voice/transcription",
            json!({
                "text": " proceed ",
                "utterance_id": "u1",
                "duration_s": 2.1,
                "radio_cue": "over",
                "end_of_turn": true,
                "close_session_requested": false
            }),
        ))
        .expect("voice prompt");
        match cmd {
            TuiCommand::VoicePrompt { text, metadata } => {
                assert_eq!(text, "proceed");
                assert_eq!(metadata.event_id, "u1");
                assert_eq!(metadata.duration_s, Some(2.1));
                assert_eq!(metadata.radio_cue.as_deref(), Some("over"));
                assert_eq!(metadata.end_of_turn, Some(true));
                assert_eq!(metadata.close_session_requested, Some(false));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn voice_prompt_metadata_preserves_over_and_out_close_intent() {
        let cmd = voice_prompt_from_notification(&notification(
            "voice/transcription",
            json!({
                "text": "stop listening",
                "utterance_id": "u-close",
                "radio_cue": "over_and_out",
                "end_of_turn": true,
                "close_session_requested": true
            }),
        ))
        .expect("voice prompt");

        match cmd {
            TuiCommand::VoicePrompt { text, metadata } => {
                assert_eq!(text, "stop listening");
                assert_eq!(metadata.event_id, "u-close");
                assert_eq!(metadata.radio_cue.as_deref(), Some("over_and_out"));
                assert_eq!(metadata.end_of_turn, Some(true));
                assert_eq!(metadata.close_session_requested, Some(true));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    fn non_transcription_and_malformed_voice_notifications_are_ignored() {
        assert!(
            voice_prompt_from_notification(&notification(
                "voice/state",
                json!({"state": "listening", "mic_open": true}),
            ))
            .is_none()
        );
        assert!(
            voice_prompt_from_notification(&notification(
                "voice/transcription",
                json!({"text": "   "}),
            ))
            .is_none()
        );
        assert!(
            voice_prompt_from_notification(&notification(
                "voice/transcription",
                json!({"text": 42}),
            ))
            .is_none()
        );
    }
}

#[cfg(test)]
mod slash_command_parsing_tests {
    use super::App;
    use super::CanonicalSlashCommand;
    use super::PromptQueueMode;
    use super::SlashResult;
    use super::TuiCommand;
    use super::canonical_slash_command;
    use super::dashboard;
    use super::permission_lane::{
        format_permission_prompt, permission_persist_scope_label, permission_response_for_key,
    };
    use super::workbench::{
        PlanDisplayItem, PlanDisplaySnapshot, PlanDisplayStatus, SlimPlanContext,
        SlimPlanHintState, active_workbench_snapshot, slim_completed_plan_hint_available,
        slim_operator_hint, workbench_rows,
    };
    use crate::lifecycle::types::NodeStatus;
    use crossterm::event::{KeyCode, KeyModifiers};
    use tokio::sync::mpsc;

    impl SlimPlanHintState {
        fn matches_active_next_visible(self) -> bool {
            matches!(self, SlimPlanHintState::Active { next_visible: true })
        }
    }

    // ── Profile ───────────────────────────────────────────

    #[test]
    fn workbench_workstream_only_uses_compact_height() {
        use super::workbench::{
            WorkbenchState, WorkstreamStatus, WorkstreamSummary, workbench_preferred_height,
        };

        let empty = WorkbenchState::default();
        assert_eq!(workbench_preferred_height(&empty, 100), 0);

        let state = WorkbenchState {
            active: None,
            workstreams: vec![WorkstreamSummary {
                id: "release".into(),
                title: "release hardening".into(),
                status: WorkstreamStatus::Paused,
                completed: 2,
                total: 5,
            }],
            ..WorkbenchState::default()
        };
        assert_eq!(workbench_preferred_height(&state, 100), 1);
    }

    #[test]
    fn workbench_workstream_only_renders_summary_without_task_rows() {
        use super::workbench::{
            WorkbenchState, WorkstreamStatus, WorkstreamSummary, render_workbench_panel,
        };

        let state = WorkbenchState {
            active: None,
            workstreams: vec![WorkstreamSummary {
                id: "release".into(),
                title: "release hardening".into(),
                status: WorkstreamStatus::Waiting,
                completed: 2,
                total: 5,
            }],
            ..WorkbenchState::default()
        };
        let backend = ratatui::backend::TestBackend::new(80, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_workbench_panel(frame.area(), frame, &super::theme::Alpharius, &state)
            })
            .unwrap();
        let mut text = String::new();
        for x in 0..80 {
            text.push_str(terminal.backend().buffer()[(x, 0)].symbol());
        }

        assert!(text.contains("workstreams×1"), "{text}");
        assert!(text.contains("waiting 2/5"), "{text}");
        assert!(text.contains("release hardening"), "{text}");
    }

    #[test]
    fn workbench_contract_renders_structured_snapshot() {
        let snapshot = PlanDisplaySnapshot {
            mode: "executing".into(),
            completed: 2,
            total: 4,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "Inspect repo".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Active,
                    description: "Patch UI".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Skipped,
                    description: "Skip old path".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Todo,
                    description: "Validate".into(),
                },
            ],
        };
        assert_eq!(snapshot.summary(), "plan executing · 2/4 · 50%");
        let rows = workbench_rows(&snapshot, 80, 5);
        assert_eq!(
            rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>(),
            vec![
                "● done  1  Inspect repo",
                "▶ next  2/4  Patch UI",
                "⊘ skip  3  Skip old path",
                "○ todo  4  Validate"
            ]
        );
        assert_eq!(rows[2].status, Some(PlanDisplayStatus::Skipped));
    }

    #[test]
    fn workbench_contract_marks_hidden_rows() {
        let snapshot = PlanDisplaySnapshot {
            mode: "executing".into(),
            completed: 1,
            total: 8,
            items: (0..8)
                .map(|idx| PlanDisplayItem {
                    status: if idx == 0 {
                        PlanDisplayStatus::Done
                    } else {
                        PlanDisplayStatus::Todo
                    },
                    description: format!("Step {idx}"),
                })
                .collect(),
        };
        let rows = workbench_rows(&snapshot, 40, 4);
        assert_eq!(
            rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>(),
            vec!["○ todo  2  Step 1", "○ todo  3  Step 2", "⋯ 6 hidden"]
        );
    }

    #[test]
    fn slim_plan_overflow_count_matches_actual_hidden_rows() {
        let snapshot = PlanDisplaySnapshot {
            mode: "executing".into(),
            completed: 3,
            total: 5,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "done one".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "done two".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "done three".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Active,
                    description: "active".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Todo,
                    description: "todo".into(),
                },
            ],
        };

        let rows = workbench_rows(&snapshot, 80, 4);
        assert_eq!(
            rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>(),
            vec!["▶ next  4/5  active", "○ todo  5  todo", "⋯ 3 hidden"]
        );
    }

    #[test]
    fn slim_plan_overflow_hides_done_before_active_or_todo() {
        let snapshot = PlanDisplaySnapshot {
            mode: "executing".into(),
            completed: 3,
            total: 6,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "Copy release handoff docs".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "Normalize changelog".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "Inspect release state".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Active,
                    description: "Record lint blocker".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Todo,
                    description: "Commit mechanics docs".into(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Todo,
                    description: "Push branch".into(),
                },
            ],
        };

        let rows = workbench_rows(&snapshot, 80, 4);
        let text = rows.iter().map(|row| row.text.as_str()).collect::<Vec<_>>();
        assert_eq!(
            text,
            vec![
                "▶ next  4/6  Record lint blocker",
                "○ todo  5  Commit mechanics docs",
                "⋯ 4 hidden"
            ]
        );
        assert!(snapshot.hint_state(4).matches_active_next_visible());
    }

    #[test]
    fn permission_prompt_contract_is_neutral_and_complete() {
        let prompt = format_permission_prompt(
            "read",
            "/tmp/outside",
            omegon_traits::PermissionRequestKind::PathBoundary,
            omegon_traits::PermissionPersistence::ProjectDirectory,
            Some("/tmp"),
        );
        assert!(prompt.contains("Tool: read"));
        assert!(prompt.contains("Target: /tmp/outside"));
        assert!(prompt.contains("Reason: grant required for this operation"));
        assert!(prompt.contains("Persist: project profile directory permission"));
        assert!(prompt.contains("Grant: /tmp"));
        assert!(prompt.contains("[y] once"));
        assert!(prompt.contains("[Shift+A] always for this directory"));
        assert!(!prompt.contains("[a] always + save"));
        assert!(!prompt.contains("outside trusted workspace"));
    }

    #[test]
    fn permission_scope_labels_are_specific() {
        assert_eq!(
            permission_persist_scope_label(
                "read",
                omegon_traits::PermissionRequestKind::PathBoundary,
                omegon_traits::PermissionPersistence::None
            ),
            "always for this file"
        );
        assert_eq!(
            permission_persist_scope_label(
                "edit",
                omegon_traits::PermissionRequestKind::PathBoundary,
                omegon_traits::PermissionPersistence::None
            ),
            "always for this path"
        );
        assert_eq!(
            permission_persist_scope_label(
                "bash",
                omegon_traits::PermissionRequestKind::PathBoundary,
                omegon_traits::PermissionPersistence::None
            ),
            "always for this command"
        );
    }

    #[test]
    fn permission_shortcuts_ignore_lane_visibility_and_require_shift_for_persist() {
        assert_eq!(
            permission_response_for_key(KeyCode::Char('y'), KeyModifiers::empty()),
            Some(omegon_traits::PermissionResponse::Allow)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('n'), KeyModifiers::empty()),
            Some(omegon_traits::PermissionResponse::Deny)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('a'), KeyModifiers::empty()),
            None
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Some(omegon_traits::PermissionResponse::AlwaysAllow)
        );
        assert_eq!(
            permission_response_for_key(KeyCode::Char('a'), KeyModifiers::SHIFT),
            Some(omegon_traits::PermissionResponse::AlwaysAllow)
        );
    }

    #[test]
    fn workbench_legacy_text_remains_fallback_only() {
        let snapshot = PlanDisplaySnapshot::from_legacy_text(
            "Plan progress\nPlan mode: executing\nProgress: 2/3\n\n1. ● Inspect\n2. ◐ Patch\n3. ⊘ Skip",
        )
        .unwrap();
        assert_eq!(snapshot.summary(), "plan executing · 2/3 · 66%");
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|item| item.status)
                .collect::<Vec<_>>(),
            vec![
                PlanDisplayStatus::Done,
                PlanDisplayStatus::Active,
                PlanDisplayStatus::Skipped
            ]
        );
    }

    #[test]
    fn completed_plan_snapshot_is_complete_but_remains_displayable() {
        let snapshot = PlanDisplaySnapshot {
            mode: "complete".to_string(),
            completed: 2,
            total: 2,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "one".to_string(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "two".to_string(),
                },
            ],
        };

        assert!(snapshot.is_complete());
        assert_eq!(snapshot.hint_state(4), SlimPlanHintState::Complete);
    }

    #[test]
    fn completed_legacy_plan_snapshot_is_complete_but_displayable() {
        let snapshot = PlanDisplaySnapshot::from_legacy_text(
            "Plan progress\nPlan mode: complete\nProgress: 2/2\n\n1. ● A\n2. ● B",
        )
        .unwrap();

        assert!(snapshot.is_complete());
    }

    #[test]
    fn completed_legacy_plan_does_not_activate_workbench() {
        let active = active_workbench_snapshot(
            None,
            Some("Plan progress\nPlan mode: complete\nProgress: 2/2\n\n1. ● A\n2. ● B"),
        );

        assert!(active.is_none());
    }

    #[test]
    fn legacy_plan_history_does_not_activate_workbench() {
        let active = active_workbench_snapshot(
            None,
            Some("Plan progress\nPlan mode: executing\nProgress: 1/2\n\n1. ● Old\n2. ◐ Stale"),
        );

        assert!(active.is_none());
    }

    #[test]
    fn live_active_plan_still_activates_workbench() {
        let live = PlanDisplaySnapshot {
            mode: "executing".to_string(),
            completed: 1,
            total: 2,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "A".to_string(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Active,
                    description: "B".to_string(),
                },
            ],
        };
        let active = active_workbench_snapshot(Some(&live), None).unwrap();

        assert_eq!(active.summary(), "plan executing · 1/2 · 50%");
        assert!(!active.is_complete());
    }

    #[test]
    fn completed_live_plan_snapshot_does_not_activate_workbench() {
        let completed = PlanDisplaySnapshot {
            mode: "complete".to_string(),
            completed: 1,
            total: 1,
            items: vec![PlanDisplayItem {
                status: PlanDisplayStatus::Done,
                description: "A".to_string(),
            }],
        };

        assert!(active_workbench_snapshot(Some(&completed), None).is_none());
    }

    #[test]
    fn completed_plan_snapshot_renders_durable_history_text() {
        let snapshot = PlanDisplaySnapshot {
            mode: "complete".to_string(),
            completed: 2,
            total: 2,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "one".to_string(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "two".to_string(),
                },
            ],
        };
        let text = snapshot.system_notification_text("Plan progress");
        assert!(text.contains("Plan mode: complete"), "{text}");
        assert!(text.contains("Progress: 2/2"), "{text}");
        assert!(
            text.lines()
                .any(|line| line.contains("1") && line.contains("●") && line.contains("one")),
            "{text}"
        );
        assert!(
            text.lines()
                .any(|line| line.contains("2") && line.contains("●") && line.contains("two")),
            "{text}"
        );
    }

    #[test]
    fn workbench_hint_matches_actually_visible_next_row() {
        let snapshot = PlanDisplaySnapshot {
            mode: "executing".to_string(),
            completed: 1,
            total: 4,
            items: vec![
                PlanDisplayItem {
                    status: PlanDisplayStatus::Done,
                    description: "done".to_string(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Active,
                    description: "active".to_string(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Todo,
                    description: "next".to_string(),
                },
                PlanDisplayItem {
                    status: PlanDisplayStatus::Todo,
                    description: "later".to_string(),
                },
            ],
        };

        assert_eq!(
            snapshot.hint_state(5),
            SlimPlanHintState::Active { next_visible: true }
        );
        assert_eq!(
            snapshot.hint_state(4),
            SlimPlanHintState::Active { next_visible: true }
        );
    }

    #[test]
    fn slim_completed_plan_hint_available_reads_completed_history_flag() {
        assert!(!slim_completed_plan_hint_available(false));
        assert!(slim_completed_plan_hint_available(true));
    }

    #[test]
    fn slim_operator_hint_prioritizes_blocking_prompts() {
        let active = SlimPlanHintState::Active { next_visible: true };
        let context = SlimPlanContext {
            active: true,
            tracked: true,
            openspec_changes: 2,
            focused_design: true,
        };
        assert_eq!(
            slim_operator_hint(true, true, true, active, &context),
            "permission · y once · Shift+A always · n deny"
        );
        assert_eq!(
            slim_operator_hint(false, true, true, active, &context),
            "manual wait · Enter done · Esc cancel"
        );
        assert_eq!(
            slim_operator_hint(false, false, true, active, &context),
            "mouse passthrough · terminal drag selects · Ctrl+Shift+T restores app mouse"
        );
        assert_eq!(
            slim_operator_hint(false, false, false, active, &context),
            "plan active · active plan · tracked · OpenSpec×2 · design-linked"
        );
        assert_eq!(
            slim_operator_hint(
                false,
                false,
                false,
                SlimPlanHintState::Active {
                    next_visible: false
                },
                &context
            ),
            "plan active · next below · active plan · tracked · OpenSpec×2 · design-linked"
        );
        assert_eq!(
            slim_operator_hint(false, false, false, SlimPlanHintState::Complete, &context),
            "plan complete · history available"
        );
        assert_eq!(
            slim_operator_hint(false, false, false, SlimPlanHintState::None, &context),
            "transcript live"
        );
    }

    #[test]
    fn workbench_context_labels_active_tracking_and_lifecycle_links() {
        let changes = vec![dashboard::ChangeSummary {
            name: "rollup".into(),
            stage: "implementing".into(),
            done_tasks: 1,
            total_tasks: 3,
        }];
        let focused = dashboard::FocusedNodeSummary {
            id: "node".into(),
            title: "Node".into(),
            status: NodeStatus::Exploring,
            open_questions: 0,
            assumptions: 0,
            decisions: 1,
            readiness: 1.0,
            openspec_change: Some("rollup".into()),
        };

        let context = SlimPlanContext::from_dashboard(true, &changes, Some(&focused));
        assert_eq!(
            context.labels(),
            vec!["active plan", "tracked", "OpenSpec×1", "design-linked"]
        );

        let context = SlimPlanContext::from_dashboard(false, &[], None);
        assert_eq!(context.labels(), vec!["no active plan"]);
    }

    #[test]
    fn auth_list_aliases_auth_status() {
        assert_eq!(
            canonical_slash_command("auth", "list"),
            Some(CanonicalSlashCommand::AuthStatus)
        );
    }

    #[test]
    fn auth_root_opens_menu_and_unlock_is_executable() {
        assert_eq!(
            canonical_slash_command("auth", ""),
            Some(CanonicalSlashCommand::AuthView)
        );
        assert_eq!(
            canonical_slash_command("auth", "unlock"),
            Some(CanonicalSlashCommand::AuthUnlock)
        );
    }

    #[test]
    fn auth_provider_ids_are_nested_login_logout_arguments() {
        assert_eq!(canonical_slash_command("auth", "openai-codex"), None);
        assert_eq!(
            canonical_slash_command("auth", "login openai-codex"),
            Some(CanonicalSlashCommand::AuthLogin("openai-codex".into()))
        );
        assert_eq!(
            canonical_slash_command("auth", "logout openai-codex"),
            Some(CanonicalSlashCommand::AuthLogout("openai-codex".into()))
        );
    }

    #[test]
    fn profile_commands_parse() {
        assert_eq!(canonical_slash_command("profile", ""), None);
        assert_eq!(
            canonical_slash_command("profile", "status"),
            Some(CanonicalSlashCommand::ProfileView)
        );
        assert_eq!(
            canonical_slash_command("profile", "capture"),
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::ActiveSource
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "save"),
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::ActiveSource
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "save --active"),
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::ActiveSource
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "save --project"),
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::Project
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "save --user"),
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::User
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "save --global"),
            Some(CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::User
            ))
        );
        assert_eq!(
            canonical_slash_command("profile", "apply"),
            Some(CanonicalSlashCommand::ProfileApply)
        );
        assert_eq!(
            canonical_slash_command("profile", "mqtt on"),
            Some(CanonicalSlashCommand::ProfileSetMqtt(Some(true)))
        );
        assert_eq!(
            canonical_slash_command("profile", "mqtt off"),
            Some(CanonicalSlashCommand::ProfileSetMqtt(Some(false)))
        );
        assert_eq!(
            canonical_slash_command("profile", "mqtt"),
            Some(CanonicalSlashCommand::ProfileSetMqtt(None))
        );
    }

    #[test]
    fn profile_extension_and_persona_commands_parse() {
        assert_eq!(
            canonical_slash_command("profile", "extension allow scry"),
            Some(CanonicalSlashCommand::ProfileExtensionAllow("scry".into()))
        );
        assert_eq!(
            canonical_slash_command("profile", "extension deny vox"),
            Some(CanonicalSlashCommand::ProfileExtensionDeny("vox".into()))
        );
        assert_eq!(
            canonical_slash_command("profile", "extensions clear"),
            Some(CanonicalSlashCommand::ProfileExtensionClear)
        );
        assert_eq!(
            canonical_slash_command("profile", "persona flynt"),
            Some(CanonicalSlashCommand::ProfileSetPersona(Some(
                "flynt".into()
            )))
        );
        assert_eq!(
            canonical_slash_command("profile", "persona off"),
            Some(CanonicalSlashCommand::ProfileSetPersona(None))
        );
        assert_eq!(
            canonical_slash_command("profile", "tone concise"),
            Some(CanonicalSlashCommand::ProfileSetTone(Some(
                "concise".into()
            )))
        );
    }

    #[test]
    fn permissions_commands_parse() {
        assert_eq!(
            canonical_slash_command("permissions", ""),
            Some(CanonicalSlashCommand::PermissionsView)
        );
        assert_eq!(
            canonical_slash_command("permissions", "keys"),
            Some(CanonicalSlashCommand::PermissionsView)
        );
        assert_eq!(
            canonical_slash_command("permissions", "add /tmp/vault"),
            Some(CanonicalSlashCommand::PermissionTrustAdd(
                "/tmp/vault".into()
            ))
        );
        assert_eq!(
            canonical_slash_command("permissions", "remove /tmp/vault"),
            Some(CanonicalSlashCommand::PermissionTrustRemove(
                "/tmp/vault".into()
            ))
        );
        assert_eq!(
            canonical_slash_command("trust", "add /tmp/vault"),
            Some(CanonicalSlashCommand::PermissionTrustAdd(
                "/tmp/vault".into()
            ))
        );
    }

    #[test]
    fn automation_commands_parse() {
        assert_eq!(
            canonical_slash_command("automation", ""),
            Some(CanonicalSlashCommand::AutomationView)
        );
        assert_eq!(
            canonical_slash_command("automation", "flow"),
            Some(CanonicalSlashCommand::AutomationSet(
                crate::settings::AutomationLevel::Flow
            ))
        );
        assert_eq!(
            canonical_slash_command("autonomy", "auto"),
            Some(CanonicalSlashCommand::AutomationSet(
                crate::settings::AutomationLevel::Autonomous
            ))
        );
        assert_eq!(canonical_slash_command("automation", "wild"), None);
    }

    // ── Skills ────────────────────────────────────────────

    #[test]
    fn skills_list() {
        assert!(matches!(
            canonical_slash_command("skills", ""),
            Some(CanonicalSlashCommand::SkillsView)
        ));
        assert!(matches!(
            canonical_slash_command("skills", "list"),
            Some(CanonicalSlashCommand::SkillsView)
        ));
        assert!(matches!(
            canonical_slash_command("skill", "list"),
            Some(CanonicalSlashCommand::SkillsView)
        ));
    }

    #[test]
    fn skills_install() {
        assert!(matches!(
            canonical_slash_command("skills", "install"),
            Some(CanonicalSlashCommand::SkillsInstall(None))
        ));
        match canonical_slash_command("skills", "install security") {
            Some(CanonicalSlashCommand::SkillsInstall(Some(name))) => assert_eq!(name, "security"),
            other => panic!("expected SkillsInstall(Some), got {other:?}"),
        }
    }

    #[test]
    fn skills_reload() {
        assert!(matches!(
            canonical_slash_command("skills", "reload"),
            Some(CanonicalSlashCommand::SkillsReload)
        ));
        assert!(matches!(
            canonical_slash_command("skills", "refresh"),
            Some(CanonicalSlashCommand::SkillsReload)
        ));
    }

    #[test]
    fn runtime_substrate_refresh() {
        assert!(matches!(
            canonical_slash_command("runtime", "restart"),
            Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
        ));
        assert!(matches!(
            canonical_slash_command("runtime", "hot-restart"),
            Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
        ));
    }

    #[test]
    fn skills_reload_advances_runtime_generation() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);
        let before_generation = app.runtime_generation;

        let result = app.handle_slash_command("/skills reload", &tx);

        match result {
            SlashResult::Display(message) => {
                assert!(message.contains("Skills reloaded"), "{message}");
                assert!(message.contains("Runtime generation:"), "{message}");
            }
            other => panic!("expected skills reload display, got {other:?}"),
        }
        assert_eq!(app.runtime_generation, before_generation + 1);
        assert!(rx.try_recv().is_err(), "reload is handled in-TUI");
    }

    #[test]
    fn skills_create() {
        assert!(matches!(
            canonical_slash_command("skills", "create"),
            Some(CanonicalSlashCommand::SkillCreate(None))
        ));
        assert!(matches!(
            canonical_slash_command("skills", "new"),
            Some(CanonicalSlashCommand::SkillCreate(None))
        ));
        assert!(matches!(
            canonical_slash_command("skills", "create --project"),
            Some(CanonicalSlashCommand::SkillCreate(Some(
                super::SkillCreateScope::Project
            )))
        ));
        assert!(matches!(
            canonical_slash_command("skills", "new --user"),
            Some(CanonicalSlashCommand::SkillCreate(Some(
                super::SkillCreateScope::User
            )))
        ));
    }

    #[test]
    fn skills_import_matrix() {
        match canonical_slash_command("skills", "import ./SKILL.md") {
            Some(CanonicalSlashCommand::SkillImport { path, scope }) => {
                assert_eq!(path, "./SKILL.md");
                assert_eq!(scope, None);
            }
            other => panic!("expected SkillImport, got {other:?}"),
        }
        assert!(matches!(
            canonical_slash_command("skills", "import --project ./SKILL.md"),
            Some(CanonicalSlashCommand::SkillImport {
                scope: Some(super::SkillCreateScope::Project),
                ..
            })
        ));
        assert!(matches!(
            canonical_slash_command("skills", "import --user ./SKILL.md"),
            Some(CanonicalSlashCommand::SkillImport {
                scope: Some(super::SkillCreateScope::User),
                ..
            })
        ));
    }

    #[test]
    fn skills_create_submits_runtime_prompt() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);

        let result = app.handle_slash_command("/skills create", &tx);
        assert!(matches!(result, SlashResult::Handled));
        match rx.try_recv() {
            Ok(TuiCommand::SubmitPrompt(prompt)) => {
                assert_eq!(prompt.submitted_by, "local-tui");
                assert_eq!(prompt.via, "tui");
                assert_eq!(prompt.queue_mode, PromptQueueMode::UntilReady);
                assert!(prompt.image_paths.is_empty());
                assert!(prompt.text.contains("skill"));
                assert!(prompt.text.contains("upstream-assisted skill workflow"));
                assert!(
                    prompt
                        .text
                        .contains("Do not blindly install arbitrary prompt packs")
                );
            }
            other => panic!("expected skill builder SubmitPrompt, got {other:?}"),
        }
    }

    #[test]
    fn skills_import_submits_runtime_prompt() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);

        let result = app.handle_slash_command("/skills import --project ./SKILL.md", &tx);
        assert!(matches!(result, SlashResult::Handled));
        match rx.try_recv() {
            Ok(TuiCommand::SubmitPrompt(prompt)) => {
                assert_eq!(prompt.submitted_by, "local-tui");
                assert_eq!(prompt.via, "tui");
                assert_eq!(prompt.queue_mode, PromptQueueMode::UntilReady);
                assert!(prompt.text.contains("Import the Omegon skill"));
                assert!(prompt.text.contains("./SKILL.md"));
                assert!(prompt.text.contains("project-local"));
            }
            other => panic!("expected skill import SubmitPrompt, got {other:?}"),
        }
    }

    #[test]
    fn skills_import_prompt_escapes_markdown_code_fence_path() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);

        let result = app.handle_slash_command("/skills import ./bad`path/SKILL.md", &tx);
        assert!(matches!(result, SlashResult::Handled));
        match rx.try_recv() {
            Ok(TuiCommand::SubmitPrompt(prompt)) => {
                assert!(
                    prompt.text.contains("`./bad\\`path/SKILL.md`"),
                    "{}",
                    prompt.text
                );
            }
            other => panic!("expected skill import SubmitPrompt, got {other:?}"),
        }
    }

    #[test]
    fn persona_create_submits_runtime_prompt() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);

        let result = app.handle_slash_command("/persona create", &tx);
        assert!(matches!(result, SlashResult::Handled));
        match rx.try_recv() {
            Ok(TuiCommand::SubmitPrompt(prompt)) => {
                assert_eq!(prompt.submitted_by, "local-tui");
                assert_eq!(prompt.via, "tui");
                assert_eq!(prompt.queue_mode, PromptQueueMode::UntilReady);
                assert!(prompt.image_paths.is_empty());
                assert!(prompt.text.contains("persona"));
            }
            other => panic!("expected persona builder SubmitPrompt, got {other:?}"),
        }
    }

    #[test]
    fn prompt_slash_submission_reports_full_runtime_queue() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, _rx) = mpsc::channel(1);
        tx.try_send(TuiCommand::Quit).expect("seed full channel");

        let result = app.handle_slash_command("/skills create", &tx);
        match result {
            SlashResult::Display(message) => {
                assert!(message.contains("Runtime command queue is full"));
            }
            other => panic!("expected full-queue display, got {other:?}"),
        }
    }

    #[test]
    fn skills_get() {
        match canonical_slash_command("skills", "get rust") {
            Some(CanonicalSlashCommand::SkillGet(name)) => assert_eq!(name, "rust"),
            other => panic!("expected SkillGet, got {other:?}"),
        }
    }

    #[test]
    fn skills_get_empty_rejected() {
        assert!(canonical_slash_command("skills", "get ").is_none());
        assert!(canonical_slash_command("skills", "get").is_none());
    }

    #[test]
    fn skills_delete() {
        match canonical_slash_command("skills", "delete my-skill") {
            Some(CanonicalSlashCommand::SkillDelete(name)) => assert_eq!(name, "my-skill"),
            other => panic!("expected SkillDelete, got {other:?}"),
        }
    }

    // ── Plan ──────────────────────────────────────────────

    #[test]
    fn plan_status_defaults_to_view() {
        assert!(matches!(
            canonical_slash_command("plan", ""),
            Some(CanonicalSlashCommand::PlanView)
        ));
        assert!(matches!(
            canonical_slash_command("plan", "status"),
            Some(CanonicalSlashCommand::PlanView)
        ));
        assert!(matches!(
            canonical_slash_command("plan", "list"),
            Some(CanonicalSlashCommand::PlanList)
        ));
    }

    #[test]
    fn plan_set_splits_pipe_delimited_items() {
        match canonical_slash_command("plan", "set read files | patch code | test") {
            Some(CanonicalSlashCommand::PlanSet(items)) => {
                assert_eq!(items, vec!["read files", "patch code", "test"]);
            }
            other => panic!("expected PlanSet, got {other:?}"),
        }
    }

    #[test]
    fn plan_gate_commands_parse() {
        assert!(matches!(
            canonical_slash_command("plan", "approve"),
            Some(CanonicalSlashCommand::PlanApprove)
        ));
        assert!(matches!(
            canonical_slash_command("plan", "execute"),
            Some(CanonicalSlashCommand::PlanExecute)
        ));
        assert!(matches!(
            canonical_slash_command("plan", "advance"),
            Some(CanonicalSlashCommand::PlanAdvance)
        ));
        assert!(matches!(
            canonical_slash_command("plan", "clear"),
            Some(CanonicalSlashCommand::PlanClear)
        ));
    }

    #[test]
    fn plan_dispatch_updates_session_intent() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);

        let result = app.handle_slash_command("/plan set read | patch | test", &tx);
        assert!(matches!(result, SlashResult::Handled));
        match rx.try_recv() {
            Ok(TuiCommand::UpdatePlan {
                command: CanonicalSlashCommand::PlanSet(items),
                respond_to: None,
            }) => assert_eq!(items, vec!["read", "patch", "test"]),
            other => panic!("expected PlanSet UpdatePlan, got {other:?}"),
        }

        let result = app.handle_slash_command("/plan approve", &tx);
        assert!(matches!(result, SlashResult::Handled));
        match rx.try_recv() {
            Ok(TuiCommand::UpdatePlan {
                command: CanonicalSlashCommand::PlanApprove,
                respond_to: None,
            }) => {}
            other => panic!("expected PlanApprove UpdatePlan, got {other:?}"),
        }
    }

    // ── Extensions ────────────────────────────────────────

    #[test]
    fn extension_list() {
        assert!(matches!(
            canonical_slash_command("extension", ""),
            Some(CanonicalSlashCommand::ExtensionView)
        ));
        assert!(matches!(
            canonical_slash_command("extension", "list"),
            Some(CanonicalSlashCommand::ExtensionView)
        ));
        assert!(matches!(
            canonical_slash_command("ext", "list"),
            Some(CanonicalSlashCommand::ExtensionView)
        ));
    }

    #[test]
    fn extension_get() {
        match canonical_slash_command("extension", "get scribe") {
            Some(CanonicalSlashCommand::ExtensionGet(name)) => assert_eq!(name, "scribe"),
            other => panic!("expected ExtensionGet, got {other:?}"),
        }
    }

    #[test]
    fn extension_install() {
        match canonical_slash_command("extension", "install https://github.com/ex/foo") {
            Some(CanonicalSlashCommand::ExtensionInstall(uri)) => {
                assert_eq!(uri, "https://github.com/ex/foo");
            }
            other => panic!("expected ExtensionInstall, got {other:?}"),
        }
    }

    #[test]
    fn extension_remove() {
        match canonical_slash_command("extension", "remove scribe") {
            Some(CanonicalSlashCommand::ExtensionRemove(name)) => assert_eq!(name, "scribe"),
            other => panic!("expected ExtensionRemove, got {other:?}"),
        }
    }

    #[test]
    fn extension_update_all() {
        assert!(matches!(
            canonical_slash_command("extension", "update"),
            Some(CanonicalSlashCommand::ExtensionUpdate(None))
        ));
    }

    #[test]
    fn extension_update_named() {
        match canonical_slash_command("extension", "update scribe") {
            Some(CanonicalSlashCommand::ExtensionUpdate(Some(name))) => assert_eq!(name, "scribe"),
            other => panic!("expected ExtensionUpdate(Some), got {other:?}"),
        }
    }

    #[test]
    fn extension_refresh_aliases_runtime_substrate_refresh() {
        for args in ["refresh", "reload", "restart"] {
            assert!(matches!(
                canonical_slash_command("extension", args),
                Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
            ));
        }
    }

    #[test]
    fn extension_enable() {
        match canonical_slash_command("extension", "enable scribe") {
            Some(CanonicalSlashCommand::ExtensionEnable(name)) => assert_eq!(name, "scribe"),
            other => panic!("expected ExtensionEnable, got {other:?}"),
        }
    }

    #[test]
    fn extension_disable() {
        match canonical_slash_command("extension", "disable scribe") {
            Some(CanonicalSlashCommand::ExtensionDisable(name)) => assert_eq!(name, "scribe"),
            other => panic!("expected ExtensionDisable, got {other:?}"),
        }
    }

    #[test]
    fn extension_search_no_query() {
        assert!(matches!(
            canonical_slash_command("extension", "search"),
            Some(CanonicalSlashCommand::ExtensionSearch(None))
        ));
    }

    #[test]
    fn extension_search_with_query() {
        match canonical_slash_command("extension", "search analytics") {
            Some(CanonicalSlashCommand::ExtensionSearch(Some(q))) => assert_eq!(q, "analytics"),
            other => panic!("expected ExtensionSearch(Some), got {other:?}"),
        }
    }

    #[test]
    fn ext_alias_works() {
        assert!(matches!(
            canonical_slash_command("ext", ""),
            Some(CanonicalSlashCommand::ExtensionView)
        ));
        match canonical_slash_command("ext", "install foo") {
            Some(CanonicalSlashCommand::ExtensionInstall(uri)) => assert_eq!(uri, "foo"),
            other => panic!("expected ExtensionInstall via 'ext', got {other:?}"),
        }
    }

    // ── Personas ──────────────────────────────────────────

    #[test]
    fn persona_list() {
        assert!(matches!(
            canonical_slash_command("persona", "list"),
            Some(CanonicalSlashCommand::PersonaList)
        ));
    }

    #[test]
    fn persona_off_handled_by_tui() {
        // "off" is NOT routed through canonical — TUI handles it directly
        assert!(canonical_slash_command("persona", "off").is_none());
    }

    #[test]
    fn persona_name_handled_by_tui() {
        // Arbitrary persona names are NOT routed through canonical — TUI handles directly
        assert!(canonical_slash_command("persona", "my-persona").is_none());
    }

    // ── Armory ────────────────────────────────────────────

    #[test]
    fn armory_browse_defaults_to_all() {
        assert!(matches!(
            canonical_slash_command("armory", ""),
            Some(CanonicalSlashCommand::ArmoryBrowse(None))
        ));
    }

    #[test]
    fn armory_search_uses_query() {
        match canonical_slash_command("armory", "search browser") {
            Some(CanonicalSlashCommand::ArmoryBrowse(Some(query))) => assert_eq!(query, "browser"),
            other => panic!("expected ArmoryBrowse(Some), got {other:?}"),
        }
    }

    #[test]
    fn armory_search_without_query_browses_all() {
        assert!(matches!(
            canonical_slash_command("armory", "search"),
            Some(CanonicalSlashCommand::ArmoryBrowse(None))
        ));
    }

    #[test]
    fn armory_install_routes_to_install() {
        match canonical_slash_command("armory", "install skills/security") {
            Some(CanonicalSlashCommand::ArmoryInstall(target)) => {
                assert_eq!(target, "skills/security")
            }
            other => panic!("expected ArmoryInstall, got {other:?}"),
        }
    }

    #[test]
    fn armory_install_without_target_is_rejected() {
        assert!(canonical_slash_command("armory", "install").is_none());
    }

    #[test]
    fn armory_dispatch_routes_to_control_runtime() {
        let settings = crate::settings::shared("anthropic:claude-sonnet-4-5");
        let mut app = App::new(settings);
        let (tx, mut rx) = mpsc::channel(1);

        assert!(matches!(
            app.handle_slash_command("/armory install skills/security", &tx),
            SlashResult::Handled
        ));

        match rx.try_recv() {
            Ok(TuiCommand::ExecuteControl {
                request: crate::control_runtime::ControlRequest::ArmoryInstall { target },
                respond_to: None,
            }) => assert_eq!(target, "skills/security"),
            other => panic!("expected ArmoryInstall ExecuteControl, got {other:?}"),
        }
    }

    // ── Catalog ───────────────────────────────────────────

    #[test]
    fn catalog_list() {
        assert!(matches!(
            canonical_slash_command("catalog", ""),
            Some(CanonicalSlashCommand::CatalogView)
        ));
        assert!(matches!(
            canonical_slash_command("catalog", "list"),
            Some(CanonicalSlashCommand::CatalogView)
        ));
    }

    #[test]
    fn catalog_install() {
        assert!(matches!(
            canonical_slash_command("catalog", "install"),
            Some(CanonicalSlashCommand::CatalogInstall)
        ));
    }

    #[test]
    fn catalog_remove() {
        match canonical_slash_command("catalog", "remove styrene.coding-agent") {
            Some(CanonicalSlashCommand::CatalogRemove(id)) => {
                assert_eq!(id, "styrene.coding-agent")
            }
            other => panic!("expected CatalogRemove, got {other:?}"),
        }
    }

    // ── COMMANDS array coverage ───────────────────────────

    #[test]
    fn commands_array_includes_extension() {
        let ext = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "extension")
            .expect("/extension command must be in COMMANDS array");
        assert!(ext.subcommands.contains(&"install"));
        assert!(ext.subcommands.contains(&"remove"));
        assert!(ext.subcommands.contains(&"enable"));
        assert!(ext.subcommands.contains(&"search"));
    }

    #[test]
    fn commands_array_includes_catalog() {
        let cat = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "catalog")
            .expect("/catalog command must be in COMMANDS array");
        assert!(cat.subcommands.contains(&"install"));
        assert!(cat.subcommands.contains(&"remove"));
    }

    #[test]
    fn commands_array_includes_armory() {
        let armory = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "armory")
            .expect("/armory command must be in COMMANDS array");
        assert!(armory.description.contains("install"));
        assert!(armory.subcommands.contains(&"browse"));
        assert!(armory.subcommands.contains(&"search"));
        assert!(armory.subcommands.contains(&"install"));
    }

    #[test]
    fn commands_array_skills_includes_reload_affordances() {
        let skills = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "skills")
            .expect("/skills must be in COMMANDS");
        for expected in [
            "create",
            "create --project",
            "create --user",
            "delete <name>",
            "get <name>",
            "import <path>",
            "import --project <path>",
            "import --user <path>",
            "install <name>",
            "new --project",
            "new --user",
            "reload",
            "refresh",
        ] {
            assert!(
                skills.subcommands.contains(&expected),
                "missing /skills {expected}"
            );
        }

        let skill_alias = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "skill")
            .expect("/skill alias must be in COMMANDS");
        assert!(skill_alias.subcommands.contains(&"reload"));
        assert!(skill_alias.subcommands.contains(&"refresh"));
    }

    #[test]
    fn commands_array_context_palette_metadata_is_action_oriented() {
        let context = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "context")
            .expect("/context must be in COMMANDS");
        assert!(context.description.contains("context"));
        for expected in [
            "status", "compact", "reset", "clear", "request", "standard", "extended", "massive",
        ] {
            assert!(
                context.subcommands.contains(&expected),
                "missing /context {expected}"
            );
        }
        let compact_count = context
            .subcommands
            .iter()
            .filter(|sub| **sub == "compact")
            .count();
        assert_eq!(
            compact_count, 1,
            "/context compact should not be duplicated"
        );
    }

    #[test]
    fn commands_array_think_palette_metadata_matches_supported_levels() {
        let think = crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .find(|command| command.name == "think")
            .expect("/think must be in COMMANDS");
        for expected in ["off", "minimal", "low", "medium", "high"] {
            assert!(
                think.subcommands.contains(&expected),
                "missing /think {expected}"
            );
        }
        assert!(
            !think.subcommands.contains(&"max"),
            "/think max should not be advertised unless ThinkingLevel supports it"
        );
    }
}
