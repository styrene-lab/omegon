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

mod agent_events;
mod auspex;
mod auth_menu_projection;
pub mod command_surfaces;
pub mod conv_widget;
pub mod conversation;
pub mod conversation_projection;
pub mod conversation_render_projection;
pub mod dashboard;
pub mod dashboard_projection;
pub mod editor;
pub mod effects;
pub mod extension_overlays;
pub mod footer;
pub mod footer_projection;
mod frame_scheduler;
pub mod glyphs;
mod history;
pub mod horizontal_line;
pub mod image;
pub mod inline_render;
mod input;
pub mod instruments;
pub mod layout_projection;
mod markdown_publication;
mod menu_effects;
pub(crate) mod menu_surface;
pub mod operation_lifecycle_projection;
pub mod permission_lane;
pub mod process_viewer;
mod render;
mod runtime_trace;
pub mod segment_components;
pub mod segment_detail;
pub mod segments;
pub mod selector;
pub(crate) mod settings_menu;
mod settings_menu_projection;
mod slash_commands;
pub mod spinner;
pub mod splash;
mod startup_splash;
pub mod statusline;
mod streaming_presentation;
pub mod tab_bar;
pub mod theme;
pub mod tool_inspection;
pub mod turn_tool_projection;
pub mod tutorial;
mod tutorial_state;
mod ui_actions;
pub mod widget_renderer;
pub mod widgets;
pub mod workbench;
mod workspace_context;

#[cfg(test)]
mod snapshot_tests;

/// Resolve a command's declared presentation from the registry.
///
/// Commands declare their own surface at definition; renderers never infer it
/// from output text. An unknown command has no declaration, so it falls back to
/// the content-shape heuristics in `show_slash_response`.
fn declared_command_surface(
    definitions: &[omegon_traits::CommandDefinition],
    command: &str,
) -> Option<omegon_traits::CommandSurface> {
    let name = command
        .split_whitespace()
        .next()?
        .trim_start_matches('/')
        .trim();
    definitions
        .iter()
        .find(|definition| definition.name == name)
        .map(|definition| definition.surface)
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
use tokio::sync::broadcast;

use self::auspex::{
    AuspexCompatibility, AuspexHandoffMode, browser_url as dash_browser_url,
    detect_target as detect_auspex_target, format_transport_security,
    launch_with_startup as launch_auspex_with_startup,
    transport_security as startup_transport_security,
};
use self::conversation::{ConversationView, Tab};
use self::dashboard::{DashboardHandleExt, DashboardState};
use self::editor::Editor;
use self::footer::{FooterData, SessionUsageSlice};
use self::frame_scheduler::{AgentDrainBudget, DrainOutcome, TuiDrawReason, TuiFrameScheduler};
use self::input::InputDisposition;
use self::instruments::InstrumentPanel;
use self::layout_projection::{TuiLayoutInputs, plan_tui_layout};
use self::menu_effects::{
    MenuCommandOutcome, MenuRefreshTarget, SettingsRowAction, SettingsRowTarget,
};
use self::menu_surface::{ActiveMenu, MenuMode};
use self::permission_lane::{format_permission_prompt, permission_response_for_key};
use self::segments::{SegmentContent, SegmentExportMode, SegmentRenderMode};
use self::settings_menu::SelectorKind;
#[cfg(test)]
use self::settings_menu_projection::settings_profile_source_line;
use self::slash_commands::SlashResult;
use self::tutorial_state::TutorialState;
#[cfg(test)]
use self::tutorial_state::parse_lesson;
use self::workbench::{
    PlanDisplaySnapshot, SlimPlanContext, SlimPlanHintState, SlimTurnState, WorkbenchState,
    WorkbenchWorkspaceContext, active_plan_workspace_context_height, active_workbench_snapshot,
    activity_preferred_height_for_level, render_activity_panel_for_level, render_workbench_panel,
    slim_completed_plan_hint_available, slim_operator_hint, upstream_retry_hint,
    workbench_preferred_height_for_level,
};
use self::workspace_context::{git_branch, repo_display_name, workspace_dir_basename};
#[cfg(test)]
use crate::runtime_commands::SkillCreateScope;
use crate::runtime_commands::{CanonicalSlashCommand, canonical_slash_command};
use crate::surfaces::command::{
    CommandPanel, CommandPanelReturnTarget, CommandPrompt, CommandPromptAction, CommandSeverity,
    CommandToast,
};
use crate::surfaces::layout::{UiPresentationLevel, UiPresentationPolicy, UiSurface, UiSurfaces};
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

use crate::operator_commands::{
    OperatorCommand as TuiCommand, OperatorCommandTx, PromptMetadata, PromptQueueMode,
    PromptSubmission, SharedCancel, VoicePromptMetadata,
};
#[cfg(test)]
use tokio_util::sync::CancellationToken;

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
    episode_id: String,
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
            episode_id: self.episode_id.clone(),
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
    LiveLatest {
        evidence_id: String,
    },
    Episode {
        episode_id: String,
        evidence_id: String,
    },
}

impl ToolInspectionTarget {
    fn evidence_id(&self) -> &str {
        match self {
            Self::LiveLatest { evidence_id } | Self::Episode { evidence_id, .. } => evidence_id,
        }
    }

    fn episode_id(&self) -> Option<&str> {
        match self {
            Self::LiveLatest { .. } => None,
            Self::Episode { episode_id, .. } => Some(episode_id),
        }
    }

    fn is_episode(&self) -> bool {
        matches!(self, Self::Episode { .. })
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuInput {
    action_label: String,
    command_prefix: String,
    value: String,
    original_footer: Option<String>,
}

struct App {
    editor: Editor,
    conversation: ConversationView,
    stream_presentation: streaming_presentation::StreamingPresentationController,
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
    /// Phase timings captured by the most recent draw callback.
    last_draw_phase_timings: runtime_trace::DrawPhaseTimings,
    /// Last on-screen editor area for mouse hit-testing.
    editor_area: Option<Rect>,
    /// Last on-screen workbench area for mouse hit-testing.
    workbench_area: Option<Rect>,
    footer_data: FooterData,
    /// CIC instrument panel for telemetry visualization
    instrument_panel: InstrumentPanel,
    /// Presentation density is independent from individual surface visibility.
    ui_presentation: UiPresentationPolicy,
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
    /// Read-only detail viewer for a retained managed execution session.
    process_viewer: Option<process_viewer::ProcessViewerState>,
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
    /// Inline argument editor for menu actions that require operator input.
    menu_input: Option<MenuInput>,
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
    /// Last left-click press, including its semantic conversation target. The
    /// target survives selection-induced reflow between the two presses.
    last_left_click: Option<LastLeftClick>,
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
    /// Monotonic identity of the active interactive runtime prompt.
    runtime_turn_id: Option<u64>,
}

type LastLeftClick = (u16, u16, std::time::Instant, Option<usize>);

fn semantic_double_click_target(
    previous: Option<LastLeftClick>,
    column: u16,
    row: u16,
    now: std::time::Instant,
) -> Option<usize> {
    previous.and_then(|(previous_column, previous_row, pressed_at, target)| {
        (previous_column.abs_diff(column) <= 1
            && previous_row.abs_diff(row) <= 1
            && now.duration_since(pressed_at) <= Duration::from_millis(400))
        .then_some(target)
        .flatten()
    })
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
            runtime_turn: self.runtime_turn_id,
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
        let (model_id, model_provider, presentation_level) = {
            let s = settings.lock().unwrap();
            (s.model.clone(), s.provider().to_string(), s.ui_presentation)
        };
        Self {
            editor: Editor::new(),
            conversation: ConversationView::new(),
            stream_presentation: streaming_presentation::StreamingPresentationController::default(),
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
            last_draw_phase_timings: runtime_trace::DrawPhaseTimings::default(),
            editor_area: None,
            workbench_area: None,
            footer_data: FooterData {
                model_id,
                model_provider,
                ..Default::default()
            },
            instrument_panel: InstrumentPanel::default(),
            ui_presentation: UiPresentationPolicy::named(presentation_level),
            ui_surfaces: UiPresentationPolicy::named(presentation_level).surfaces,
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
            process_viewer: None,
            route_state: None,
            route_selected_model: None,
            route_serving_model: None,
            secret_readiness: None,
            pending_menu_confirmation: None,
            menu_input: None,
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
            runtime_turn_id: None,
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

    fn apply_ui_presentation(&mut self, policy: UiPresentationPolicy) {
        self.ui_presentation = policy;
        self.ui_surfaces = policy.surfaces;
        self.update_and_persist(|settings| settings.ui_presentation = policy.level);
    }

    fn apply_ui_preset(&mut self, surfaces: UiSurfaces) {
        let next = if surfaces == self.ui_presentation.surfaces {
            self.ui_presentation.with_surfaces(surfaces)
        } else {
            let level = if surfaces == UiSurfaces::full() {
                UiPresentationLevel::Full
            } else {
                UiPresentationLevel::Om
            };
            UiPresentationPolicy::named(level)
        };
        self.apply_ui_presentation(next);
    }

    fn toggle_ui_surface(&mut self, surface: UiSurfaceToggle, enabled: bool) {
        let semantic_surface = match surface {
            UiSurfaceToggle::Dashboard => UiSurface::Dashboard,
            UiSurfaceToggle::Instruments => UiSurface::Instruments,
            UiSurfaceToggle::Footer => UiSurface::Footer,
            UiSurfaceToggle::Activity => UiSurface::Activity,
        };
        self.ui_presentation.set_surface(semantic_surface, enabled);
        self.ui_surfaces = self.ui_presentation.surfaces;
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
        let mode = self.ui_presentation.preset_name();
        format!(
            "UI presentation: {mode}\n  dashboard: {}\n  instruments: {}\n  footer: {}\n  activity: {}\n\nPresentation levels\n  /ui om      (quiet outcomes + essential attention)\n  /ui active  (bounded live workflow visibility)\n  /ui full    (persistent operational evidence)\n\nCompatibility aliases\n  /ui lean | /ui slim → om\n\nSurfaces\n  /ui show|hide|toggle dashboard|instruments|footer|activity",
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
        let catalog = crate::model_catalog::ModelCatalog::discover();
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
                let freshness = catalog
                    .freshness
                    .get(provider_name)
                    .map(|state| format!(" · inventory {state}"))
                    .unwrap_or_default();
                let pricing = model
                    .context_pricing_notice
                    .as_ref()
                    .map(|notice| format!(" · {}", notice.summary()))
                    .unwrap_or_default();
                let description = format!(
                    "{} — {}{}{}{}",
                    model.description, context, caps, freshness, pricing
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
        let repo = repo_display_name(cwd).or_else(|| {
            crate::setup::find_project_root(cwd)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });
        WorkbenchWorkspaceContext {
            repo,
            dir,
            git_branch: git_branch(cwd).or_else(|| self.footer_data.harness.git_branch.clone()),
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
        let presentation = self.ui_presentation;
        let mut menu = MenuProjection::new("ui", "UI");
        menu.summary = Some(format!(
            "Presentation: {}; dashboard: {}; instruments: {}; footer: {}; activity: {}.",
            presentation.preset_name(),
            if surfaces.dashboard { "on" } else { "off" },
            if surfaces.instruments { "on" } else { "off" },
            if surfaces.footer { "on" } else { "off" },
            if surfaces.activity { "on" } else { "off" },
        ));
        menu.footer = Some("↑/↓ navigate · / filter · Enter run · o om · a active · f full · Esc close · /ui status for text readout".into());
        menu.actions = vec![
            {
                let mut action = MenuActionProjection::command("ui.global.om", "Om", "/ui om");
                action.key = Some("o".into());
                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                action
            },
            {
                let mut action =
                    MenuActionProjection::command("ui.global.active", "Active", "/ui active");
                action.key = Some("a".into());
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
                    label: "Presentation levels".into(),
                    description: Some("Choose conversation and operational evidence density independently from surface visibility.".into()),
                    rows: vec![
                        MenuRowProjection {
                            id: "ui.preset.om".into(),
                            label: "Om".into(),
                            description: "Quiet outcomes and essential attention.".into(),
                            value: Some(if presentation.level == UiPresentationLevel::Om && presentation.preset_name() != "custom" { "active" } else { "" }.into()),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: if presentation.level == UiPresentationLevel::Om && presentation.preset_name() != "custom" { "active".into() } else { "level".into() },
                                tone: if presentation.level == UiPresentationLevel::Om && presentation.preset_name() != "custom" { MenuBadgeTone::Success } else { MenuBadgeTone::Info },
                            }],
                            metadata: vec!["/ui om".into(), "/ui lean".into(), "/ui slim".into()],
                            primary_action: Some({
                                let mut action = MenuActionProjection::command("ui.preset.om.primary", "Om", "/ui om");
                                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }),
                            actions: vec![{
                                let mut action = MenuActionProjection::command("ui.preset.om.action", "Om", "/ui om");
                                action.key = Some("o".into());
                                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "ui.preset.active".into(),
                            label: "Active".into(),
                            description: "Bounded live workflow visibility with grouped outcomes.".into(),
                            value: Some(if presentation.level == UiPresentationLevel::Active && presentation.preset_name() != "custom" { "active" } else { "" }.into()),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: if presentation.level == UiPresentationLevel::Active && presentation.preset_name() != "custom" { "active".into() } else { "level".into() },
                                tone: if presentation.level == UiPresentationLevel::Active && presentation.preset_name() != "custom" { MenuBadgeTone::Success } else { MenuBadgeTone::Info },
                            }],
                            metadata: vec!["/ui active".into()],
                            primary_action: Some({
                                let mut action = MenuActionProjection::command("ui.preset.active.primary", "Active", "/ui active");
                                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }),
                            actions: vec![{
                                let mut action = MenuActionProjection::command("ui.preset.active.action", "Active", "/ui active");
                                action.key = Some("a".into());
                                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                action
                            }],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "ui.preset.full".into(),
                            label: "Full".into(),
                            description: "Persistent operational evidence and diagnostic surfaces.".into(),
                            value: Some(
                                if presentation.level == UiPresentationLevel::Full && presentation.preset_name() != "custom" {
                                    "active"
                                } else {
                                    ""
                                }
                                .into(),
                            ),
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection {
                                label: if presentation.level == UiPresentationLevel::Full && presentation.preset_name() != "custom" {
                                    "active".into()
                                } else {
                                    "preset".into()
                                },
                                tone: if presentation.level == UiPresentationLevel::Full && presentation.preset_name() != "custom" {
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
        menu.summary = Some("Manage session-scoped runtime configuration. Values are printable by design; put credentials in /secrets instead.".into());
        menu.footer = Some("↑/↓ navigate · Enter set/update · / filter · Esc close · /variables status for text readout".into());

        let snapshot = crate::control::variables::variables_snapshot();
        let variable_rows = if snapshot.is_empty() {
            vec![MenuRowProjection {
                id: "variables.inventory.empty".into(),
                label: "No session variables set".into(),
                description: "Press Enter to set printable runtime config for this session.".into(),
                value: None,
                kind: MenuRowKind::Action,
                badges: vec![MenuBadgeProjection {
                    label: "set".into(),
                    tone: MenuBadgeTone::Info,
                }],
                metadata: vec![
                    "/variables set NAME VALUE".into(),
                    "use /secrets for credentials".into(),
                ],
                primary_action: Some(MenuActionProjection::prime_editor(
                    "variables.inventory.empty.set",
                    "Set variable",
                    "/variables set ",
                    "Type NAME VALUE; use /secrets for credentials",
                )),
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
                        primary_action: Some(MenuActionProjection::prime_editor(
                            format!("variables.set.{name}"),
                            "Update",
                            format!("/variables set {name} "),
                            "Type the replacement printable value for this session variable",
                        )),
                        actions: vec![
                            MenuActionProjection::command(
                                format!("variables.get.{name}"),
                                "Show value",
                                format!("/variables get {name}"),
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
            label: "Manage".into(),
            groups: vec![MenuGroupProjection {
                id: "variables.inventory".into(),
                label: "Manage session variables".into(),
                description: Some("Printable runtime config available to Omegon-managed process launches. Enter updates the selected variable; values are intentionally visible.".into()),
                rows: variable_rows,
            }],
        }, MenuTabProjection {
            id: "actions".into(),
            label: "Templates".into(),
            groups: vec![MenuGroupProjection {
                id: "variables.actions".into(),
                label: "Variable command templates".into(),
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
        menu.summary = Some("Secret readiness and recovery surface. Enter follows the selected binding's diagnosed next step; values are never displayed and plaintext replacement always uses hidden input.".into());
        menu.footer = Some(
            "↑/↓ navigate · Enter recommended action · / filter · Esc close · → alternatives"
                .into(),
        );
        menu.tabs = vec![MenuTabProjection {
            id: "inventory".into(),
            label: "Manage".into(),
            groups: vec![MenuGroupProjection {
                id: "secrets.inventory".into(),
                label: "Manage secret bindings".into(),
                description: Some("Known and declared secret bindings. Enter follows the diagnosed recovery path: missing values open hidden input, ready or deferred bindings verify resolution, and failed configured sources open source-specific repair actions.".into()),
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
            label: "Templates".into(),
            groups: vec![MenuGroupProjection {
                id: "secrets.actions".into(),
                label: "Secret command templates".into(),
                description: Some("Prepare safe secret commands without exposing values in the menu.".into()),
                rows: vec![
                    MenuRowProjection {
                        id: "secrets.status".into(),
                        label: "List secret bindings".into(),
                        description: "Show configured and declared secret bindings; never prints resolved values.".into(),
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
                let (status_label, status_tone) = match secret.reason.as_ref() {
                    Some(crate::capabilities::secrets::SecretReadinessReason::EnvironmentNotInherited) => {
                        ("environment not inherited", MenuBadgeTone::Warning)
                    }
                    Some(crate::capabilities::secrets::SecretReadinessReason::SourceFailed) => {
                        ("source failed", MenuBadgeTone::Danger)
                    }
                    None => match secret.status {
                        SecretReadinessStatus::Warmed => ("ready this session", MenuBadgeTone::Success),
                        SecretReadinessStatus::Configured => ("configured — not loaded", MenuBadgeTone::Info),
                        SecretReadinessStatus::Deferred => ("loads on demand", MenuBadgeTone::Info),
                        SecretReadinessStatus::Unchecked => ("not checked", MenuBadgeTone::Neutral),
                        SecretReadinessStatus::Missing => ("not configured", MenuBadgeTone::Danger),
                    },
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
                metadata.push(format!(
                    "process environment: {}",
                    if secret.process_env_available {
                        "available"
                    } else {
                        "not inherited"
                    }
                ));
                if let Some(kind) = secret.recipe_kind.as_deref() {
                    metadata.push(format!("recipe: {kind}"));
                    if kind == "env"
                        && let Some(source) = secret.recipe_source.as_deref()
                    {
                        metadata.push(format!("configured source: env:{source}"));
                    }
                    if matches!(secret.status, SecretReadinessStatus::Missing) {
                        metadata.push(match secret.reason.as_ref() {
                            Some(crate::capabilities::secrets::SecretReadinessReason::EnvironmentNotInherited) => {
                                "evidence: configured environment variable is absent from the current process"
                            }
                            _ => "resolution: configured source did not resolve; no policy denial was observed",
                        }.into());
                    }
                } else {
                    metadata.push("recipe: none".into());
                }
                if secret.warmed {
                    metadata.push("session: warmed".into());
                } else {
                    metadata.push("session: not warmed".into());
                }
                for consumer in &secret.consumers {
                    metadata.push(format!("consumer: {:?}:{}", consumer.kind, consumer.id));
                }
                let configured_source_failed = matches!(secret.status, SecretReadinessStatus::Missing)
                    && secret.recipe_kind.is_some();
                let truly_missing = matches!(secret.status, SecretReadinessStatus::Missing)
                    && secret.recipe_kind.is_none()
                    && !secret.process_env_available;
                let (description, primary_action) = if truly_missing {
                    (
                        "No usable source is configured for this secret. Enter a new value using hidden input.",
                        MenuActionProjection::prime_editor(
                            format!("secrets.set.hidden.{}", secret.name),
                            "Enter value",
                            format!("/secrets set {}", secret.name),
                            "Capture a new value with hidden input",
                        ),
                    )
                } else if matches!(
                    secret.reason.as_ref(),
                    Some(crate::capabilities::secrets::SecretReadinessReason::EnvironmentNotInherited)
                ) {
                    (
                        "The configured environment variable is absent from this process. Enter re-checks the evidence; repair the source or replace it with a known-good value.",
                        MenuActionProjection::command(
                            format!("secrets.get.{}", secret.name),
                            "Re-check environment",
                            format!("/secrets get {}", secret.name),
                        ),
                    )
                } else if configured_source_failed {
                    (
                        "The configured source did not resolve in this session. Inspect or replace the source; values remain redacted.",
                        MenuActionProjection::command(
                            format!("secrets.get.{}", secret.name),
                            "Inspect failing source",
                            format!("/secrets get {}", secret.name),
                        ),
                    )
                } else {
                    (
                        "This binding has a configured or session-available source. Enter verifies redacted resolution; replacement remains available as an explicit alternative.",
                        MenuActionProjection::command(
                            format!("secrets.get.{}", secret.name),
                            "Check readiness",
                            format!("/secrets get {}", secret.name),
                        ),
                    )
                };
                let mut actions = vec![
                    MenuActionProjection::prime_editor(
                        format!("secrets.replace.hidden.{}", secret.name),
                        "Replace entirely",
                        format!("/secrets set {}", secret.name),
                        "Replace the current source with a value captured using hidden input",
                    ),
                    MenuActionProjection::prime_editor(
                        format!("secrets.recipe.env.{}", secret.name),
                        if secret.recipe_kind.as_deref() == Some("env") {
                            "Repair env source"
                        } else {
                            "Use env source"
                        },
                        format!("/secrets set {} env:", secret.name),
                        "Type the environment variable name after env:; Omegon will prove whether the current process inherited it",
                    ),
                    MenuActionProjection::prime_editor(
                        format!("secrets.recipe.cmd.{}", secret.name),
                        "Use cmd source",
                        format!("/secrets set {} cmd:", secret.name),
                        "Type the command after cmd:; resolved output stays redacted",
                    ),
                    MenuActionProjection::prime_editor(
                        format!("secrets.recipe.vault.{}", secret.name),
                        "Use vault source",
                        format!("/secrets set {} vault:", secret.name),
                        "Type the vault path after vault:",
                    ),
                    MenuActionProjection::prime_editor(
                        format!("secrets.delete.{}", secret.name),
                        "Clear binding",
                        format!("/secrets delete {}", secret.name),
                        "Clear the configured value or recipe; capability requirement remains visible",
                    ),
                ];
                if !matches!(secret.status, SecretReadinessStatus::Missing) {
                    actions.insert(
                        0,
                        MenuActionProjection::command(
                            format!("secrets.get.{}", secret.name),
                            "Check resolution",
                            format!("/secrets get {}", secret.name),
                        ),
                    );
                }
                MenuRowProjection {
                    id: format!("secrets.inventory.{}", secret.name),
                    label: secret.name.clone(),
                    description: description.into(),
                    value: Some(status_label.into()),
                    kind: MenuRowKind::Object,
                    badges,
                    metadata,
                    primary_action: Some(primary_action),
                    actions,
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
        let installed_extensions = crate::extension_cli::extensions_dir()
            .ok()
            .and_then(|dir| {
                crate::capabilities::extensions::list_extension_installations_from_dir(&dir).ok()
            })
            .unwrap_or_default();
        let extension_rows = if installed_extensions.is_empty() {
            vec![MenuRowProjection {
                id: "extension.empty".into(),
                label: "No extensions installed".into(),
                description: "Search the extension catalog or install from a URL/path.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection {
                    label: "empty".into(),
                    tone: MenuBadgeTone::Neutral,
                }],
                metadata: vec![],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            }]
        } else {
            installed_extensions
                .into_iter()
                .map(|installation| {
                    use crate::capabilities::extensions::ExtensionInstallationDiagnosis;
                    let filesystem_name = installation.filesystem_name;
                    match installation.diagnosis {
                        ExtensionInstallationDiagnosis::Valid { capability: extension } => {
                            let runtime = match extension.runtime {
                                crate::capabilities::extensions::ExtensionRuntimeSummary::Native { .. } => "native",
                                crate::capabilities::extensions::ExtensionRuntimeSummary::Oci { .. } => "oci",
                            };
                            let name = extension.name;
                            let enabled = extension.enabled;
                            MenuRowProjection {
                                id: format!("extension.installed.{filesystem_name}"),
                                label: name.clone(),
                                description: extension.description,
                                value: Some(format!("v{} · {runtime}", extension.version)),
                                kind: MenuRowKind::Action,
                                badges: vec![MenuBadgeProjection {
                                    label: if enabled { "enabled" } else { "disabled" }.into(),
                                    tone: if enabled { MenuBadgeTone::Success } else { MenuBadgeTone::Warning },
                                }],
                                metadata: vec![extension.status, installation.source_path],
                                primary_action: Some(MenuActionProjection::open_extension_detail(
                                    format!("extension.{name}.open"),
                                    "Open",
                                    filesystem_name.clone(),
                                )),
                                actions: vec![{
                                    let mut toggle = MenuActionProjection::command(
                                        format!("extension.{name}.toggle"),
                                        if enabled { "Disable" } else { "Enable" },
                                        format!("/extension {} {filesystem_name}", if enabled { "disable" } else { "enable" }),
                                    );
                                    toggle.key = Some(" ".into());
                                    toggle.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                                    toggle
                                }],
                                safety: None,
                                availability: None,
                            }
                        }
                        diagnosis => {
                            let (state, problem) = match diagnosis {
                                ExtensionInstallationDiagnosis::Invalid { problem } => ("invalid", problem),
                                ExtensionInstallationDiagnosis::BrokenLink { problem } => ("broken link", problem),
                                ExtensionInstallationDiagnosis::Unreadable { problem } => ("unreadable", problem),
                                ExtensionInstallationDiagnosis::Valid { .. } => unreachable!(),
                            };
                            let mut remove = MenuActionProjection::command(
                                format!("extension.{filesystem_name}.remove"),
                                "Remove",
                                format!("/extension remove {filesystem_name}"),
                            );
                            remove.requires_confirmation = true;
                            remove.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                            MenuRowProjection {
                                id: format!("extension.installed.{filesystem_name}"),
                                label: filesystem_name.clone(),
                                description: problem.clone(),
                                value: Some(state.into()),
                                kind: MenuRowKind::Action,
                                badges: vec![MenuBadgeProjection { label: "invalid".into(), tone: MenuBadgeTone::Warning }],
                                metadata: vec![problem, installation.source_path],
                                primary_action: None,
                                actions: vec![remove],
                                safety: None,
                                availability: None,
                            }
                        }
                    }
                })
                .collect()
        };
        let mut menu = MenuProjection::new("extension-runtime", "Extensions & Runtime");
        menu.summary = Some("Browse installed extensions, toggle state with Space, or open an extension to manage it.".into());
        menu.footer =
            Some("↑/↓ navigate · Space toggle · Enter open · / filter · Esc close".into());
        menu.tabs = vec![MenuTabProjection {
            id: "overview".into(),
            label: "Overview".into(),
            groups: vec![
                MenuGroupProjection {
                    id: "extension.inventory".into(),
                    label: "Extensions".into(),
                    description: Some("Installed extensions and their live state. Space enables or disables; Enter opens extension details and management actions.".into()),
                    rows: extension_rows,
                },
                MenuGroupProjection {
                    id: "extension.actions".into(),
                    label: "Discover & maintain".into(),
                    description: Some("Create or install extensions, discover catalog entries, and update the installed set.".into()),
                    rows: vec![
                        MenuRowProjection {
                            id: "extension.create".into(),
                            label: "Create extension".into(),
                            description: "Scaffold a new extension project. Enter a lowercase name, then press Enter.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection { label: "create".into(), tone: MenuBadgeTone::Warning }],
                            metadata: vec!["/extension init <name>".into()],
                            primary_action: Some(MenuActionProjection::inline_input("extension.create.primary", "Create", "/extension init ")),
                            actions: vec![],
                            safety: None,
                            availability: None,
                        },
                        MenuRowProjection {
                            id: "extension.install".into(),
                            label: "Install extension".into(),
                            description: "Install from a catalog name, URL, or local path.".into(),
                            value: None,
                            kind: MenuRowKind::Action,
                            badges: vec![MenuBadgeProjection { label: "create".into(), tone: MenuBadgeTone::Warning }],
                            metadata: vec!["/extension install <source>".into()],
                            primary_action: Some(MenuActionProjection::inline_input("extension.install.primary", "Install", "/extension install ")),
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
                            primary_action: Some(MenuActionProjection::inline_input("extension.search.primary", "Search", "/extension search ")),
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
                            label: "Reload live configuration".into(),
                            description: "Reload skills and re-scan extension/runtime candidates without closing this session. Running extensions are inspected but not replaced.".into(),
                            value: Some("keeps session open".into()),
                            kind: MenuRowKind::Action,
                            badges: vec![
                                MenuBadgeProjection { label: "live".into(), tone: MenuBadgeTone::Warning },
                                MenuBadgeProjection { label: "keeps session".into(), tone: MenuBadgeTone::Neutral },
                            ],
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

    fn extension_detail_menu_projection(
        &self,
        filesystem_name: &str,
    ) -> Option<crate::surfaces::menu::MenuProjection> {
        use crate::capabilities::extensions::ExtensionInstallationDiagnosis;
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };
        let installation = crate::extension_cli::extensions_dir()
            .ok()
            .and_then(|dir| {
                crate::capabilities::extensions::list_extension_installations_from_dir(&dir).ok()
            })?
            .into_iter()
            .find(|installation| installation.filesystem_name == filesystem_name)?;
        let mut rows = Vec::new();
        let title = match installation.diagnosis {
            ExtensionInstallationDiagnosis::Valid {
                capability: extension,
            } => {
                let name = extension.name;
                let enabled = extension.enabled;
                rows.push(MenuRowProjection {
                    id: format!("extension.detail.{filesystem_name}.toggle"),
                    label: if enabled {
                        "Disable extension"
                    } else {
                        "Enable extension"
                    }
                    .into(),
                    description:
                        "Change whether this extension is loaded. The detail page remains open."
                            .into(),
                    value: Some(if enabled { "enabled" } else { "disabled" }.into()),
                    kind: MenuRowKind::Action,
                    badges: vec![MenuBadgeProjection {
                        label: "Space".into(),
                        tone: MenuBadgeTone::Info,
                    }],
                    metadata: vec![],
                    primary_action: Some({
                        let mut action = MenuActionProjection::command(
                            format!("extension.{name}.toggle"),
                            if enabled { "Disable" } else { "Enable" },
                            format!(
                                "/extension {} {filesystem_name}",
                                if enabled { "disable" } else { "enable" }
                            ),
                        );
                        action.key = Some(" ".into());
                        action.close_policy =
                            crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                        action
                    }),
                    actions: vec![],
                    safety: None,
                    availability: None,
                });
                rows.push(MenuRowProjection {
                    id: format!("extension.detail.{filesystem_name}.update"),
                    label: "Update extension".into(),
                    description: "Fetch and install the latest available extension revision."
                        .into(),
                    value: Some(format!("v{}", extension.version)),
                    kind: MenuRowKind::Action,
                    badges: vec![MenuBadgeProjection {
                        label: "mutates".into(),
                        tone: MenuBadgeTone::Warning,
                    }],
                    metadata: vec![],
                    primary_action: Some({
                        let mut action = MenuActionProjection::command(
                            format!("extension.{name}.update"),
                            "Update",
                            format!("/extension update {name}"),
                        );
                        action.requires_confirmation = true;
                        action.close_policy =
                            crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                        action
                    }),
                    actions: vec![],
                    safety: None,
                    availability: None,
                });
                name
            }
            diagnosis => {
                let problem = match diagnosis {
                    ExtensionInstallationDiagnosis::Invalid { problem }
                    | ExtensionInstallationDiagnosis::BrokenLink { problem }
                    | ExtensionInstallationDiagnosis::Unreadable { problem } => problem,
                    ExtensionInstallationDiagnosis::Valid { .. } => unreachable!(),
                };
                rows.push(MenuRowProjection {
                    id: format!("extension.detail.{filesystem_name}.diagnosis"),
                    label: "Installation problem".into(),
                    description: problem,
                    value: Some("invalid".into()),
                    kind: MenuRowKind::Object,
                    badges: vec![MenuBadgeProjection {
                        label: "diagnostic".into(),
                        tone: MenuBadgeTone::Warning,
                    }],
                    metadata: vec![],
                    primary_action: None,
                    actions: vec![],
                    safety: None,
                    availability: None,
                });
                filesystem_name.to_string()
            }
        };
        rows.push(MenuRowProjection {
            id: format!("extension.detail.{filesystem_name}.remove"),
            label: "Remove extension".into(),
            description: "Remove this extension installation from Omegon.".into(),
            value: None,
            kind: MenuRowKind::Action,
            badges: vec![MenuBadgeProjection {
                label: "destructive".into(),
                tone: MenuBadgeTone::Warning,
            }],
            metadata: vec![],
            primary_action: Some({
                let mut action = MenuActionProjection::command(
                    format!("extension.{filesystem_name}.remove"),
                    "Remove",
                    format!("/extension remove {filesystem_name}"),
                );
                action.requires_confirmation = true;
                action.close_policy = crate::surfaces::menu::MenuActionClosePolicy::RefreshMenu;
                action
            }),
            actions: vec![],
            safety: None,
            availability: None,
        });
        let mut menu = MenuProjection::new(
            format!("extension-detail:{filesystem_name}"),
            format!("Extension · {title}"),
        );
        menu.summary = Some(format!(
            "Installed as {filesystem_name}. Manage this extension without leaving the menu surface."
        ));
        menu.footer = Some("↑/↓ navigate · Space toggle · Enter run · Esc back".into());
        menu.tabs = vec![MenuTabProjection {
            id: "manage".into(),
            label: "Manage".into(),
            groups: vec![MenuGroupProjection {
                id: "extension.detail.actions".into(),
                label: "Extension actions".into(),
                description: None,
                rows,
            }],
        }];
        Some(menu)
    }

    fn open_extension_detail_menu(&mut self, filesystem_name: &str) {
        if let Some(projection) = self.extension_detail_menu_projection(filesystem_name) {
            self.open_menu_projection(projection);
        }
    }

    fn open_extension_runtime_menu(&mut self) {
        self.open_menu_projection(self.extension_runtime_menu_projection());
    }

    fn init_menu_projection(&self) -> crate::surfaces::menu::MenuProjection {
        use crate::surfaces::menu::{
            MenuActionProjection, MenuBadgeProjection, MenuBadgeTone, MenuGroupProjection,
            MenuProjection, MenuRowKind, MenuRowProjection, MenuTabProjection,
        };

        let cwd = self.cwd();
        let project_root = crate::setup::find_project_root(cwd);
        let project_profile = project_root.join(".omegon/profile.json");
        let project_registry = project_root.join(".omegon/profiles");
        let project_active = project_root.join(".omegon/active-profile.json");
        let user_home = dirs::home_dir();
        let user_profile = user_home
            .as_ref()
            .map(|home| home.join(".omegon/profile.json"));
        let user_active = user_home
            .as_ref()
            .map(|home| home.join(".omegon/active-profile.json"));

        let mut pending = Vec::new();
        pending.push("inspect repository architecture and refresh agent guidance with the bundled codebase-init skill");
        if !project_root.join(".omegon").exists() {
            pending.push("create .omegon/ harness configuration directory");
        }
        if !project_root.join("ai/memory").exists() {
            pending.push("create ai/memory/ for durable project facts");
        }
        if !project_root.join("AGENTS.md").exists()
            && !project_root.join(".omegon/AGENTS.md").exists()
        {
            pending.push("create AGENTS.md if known project directives are discovered");
        }
        if !project_active.exists() && project_registry.is_dir() {
            pending.push("select an active project profile pointer for the project registry");
        }

        let pending_summary = if pending.is_empty() {
            "Harness substrate is already present; repair/import actions are still available."
                .to_string()
        } else {
            pending
                .iter()
                .enumerate()
                .map(|(idx, action)| format!("{}. {action}", idx + 1))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let signal_matches = |signal: &str| {
            let signal = signal.trim().trim_start_matches("./");
            if signal.is_empty() {
                return false;
            }
            if !signal.contains('*') {
                return project_root.join(signal).exists();
            }
            let (prefix, suffix) = signal.split_once('*').unwrap_or((signal, ""));
            std::fs::read_dir(&project_root)
                .ok()
                .is_some_and(|entries| {
                    entries.filter_map(|entry| entry.ok()).any(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        name.starts_with(prefix) && name.ends_with(suffix)
                    })
                })
        };
        let mut skill_rows: Vec<MenuRowProjection> = crate::skills::list_structured()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|skill| {
                let matched: Vec<String> = skill
                    .project_signals
                    .iter()
                    .filter(|signal| signal_matches(signal))
                    .cloned()
                    .collect();
                if matched.is_empty() {
                    return None;
                }
                let tone = if skill.project_local {
                    MenuBadgeTone::Success
                } else if skill.source == "user" {
                    MenuBadgeTone::Info
                } else {
                    MenuBadgeTone::Neutral
                };
                let (command, action_label, action_note) = if skill.project_local {
                    (
                        format!("/skills get {}", skill.name),
                        "Inspect",
                        "already project-local",
                    )
                } else if skill.source == "user" {
                    (
                        format!("/skills import --project {}", skill.path),
                        "Copy to project",
                        "user skill can be localized for this repo",
                    )
                } else if skill.bundled && !skill.installed {
                    (
                        format!("/skills install {}", skill.name),
                        "Install",
                        "bundled skill can be installed for reuse",
                    )
                } else {
                    (
                        format!("/skills get {}", skill.name),
                        "Inspect",
                        "inspect before changing project policy",
                    )
                };
                Some(MenuRowProjection {
                    id: format!("init.skill.{}", skill.name),
                    label: format!("{} skill", skill.name),
                    description: format!(
                        "Detected {}. Source: {}; activation: {}; {action_note}.",
                        matched.join(", "),
                        skill.source,
                        skill.activation.as_deref().unwrap_or("manual")
                    ),
                    value: Some(skill.source.clone()),
                    kind: MenuRowKind::Action,
                    badges: vec![MenuBadgeProjection {
                        label: if skill.project_local {
                            "project"
                        } else {
                            skill.source.as_str()
                        }
                        .into(),
                        tone,
                    }],
                    metadata: vec![format!("signals: {}", matched.join(", ")), command.clone()],
                    primary_action: Some(MenuActionProjection::command(
                        format!("init.skill.{}.primary", skill.name),
                        action_label,
                        command,
                    )),
                    actions: vec![],
                    safety: None,
                    availability: None,
                })
            })
            .collect();
        skill_rows.sort_by(|a, b| a.label.cmp(&b.label));
        if skill_rows.is_empty() {
            skill_rows.push(MenuRowProjection {
                id: "init.skills.none".into(),
                label: "No skill recommendations detected".into(),
                description: "No installed, bundled, user, project, or extension skill project_signals matched this repository.".into(),
                value: Some("none".into()),
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection {
                    label: "clean".into(),
                    tone: MenuBadgeTone::Neutral,
                }],
                metadata: vec![],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            });
        }

        let row = |id: &str,
                   label: &str,
                   description: &str,
                   value: Option<String>,
                   badge: &str,
                   tone: MenuBadgeTone,
                   command: &str,
                   confirm: bool| {
            let mut primary =
                MenuActionProjection::command(format!("{id}.primary"), "Run", command.to_string());
            primary.requires_confirmation = confirm;
            MenuRowProjection {
                id: id.into(),
                label: label.into(),
                description: description.into(),
                value,
                kind: MenuRowKind::Action,
                badges: vec![MenuBadgeProjection {
                    label: badge.into(),
                    tone,
                }],
                metadata: vec![command.into()],
                primary_action: Some(primary),
                actions: vec![],
                safety: None,
                availability: None,
            }
        };

        let mut menu = MenuProjection::new("init", "Init");
        menu.summary = Some(format!(
            "Agent harness initialization defaults. Pending plan:\n{pending_summary}"
        ));
        menu.footer = Some("↑/↓ navigate · / filter · Enter run selected init action · edit command before running if needed · Esc close".into());
        menu.tabs = vec![MenuTabProjection {
            id: "defaults".into(),
            label: "Defaults".into(),
            groups: vec![
                MenuGroupProjection {
                    id: "init.pending".into(),
                    label: "Pending harness initialization".into(),
                    description: Some("Discovered harness substrate actions. Defaults are ready to run, but each row is still an explicit operator action.".into()),
                    rows: vec![MenuRowProjection {
                        id: "init.pending.summary".into(),
                        label: "What /init will initialize".into(),
                        description: pending_summary,
                        value: Some(format!("{} pending", pending.len())),
                        kind: MenuRowKind::Object,
                        badges: vec![MenuBadgeProjection {
                            label: if pending.is_empty() { "clean" } else { "plan" }.into(),
                            tone: if pending.is_empty() { MenuBadgeTone::Success } else { MenuBadgeTone::Info },
                        }],
                        metadata: vec![format!("project: {}", project_root.display())],
                        primary_action: None,
                        actions: vec![],
                        safety: None,
                        availability: None,
                    }],
                },
                MenuGroupProjection {
                    id: "init.decisions".into(),
                    label: "Detected compatibility decisions".into(),
                    description: Some("Ad-hoc prompts for mid-state upgrades. These are not standard init steps; run them only when the detected legacy state is the source of confusion.".into()),
                    rows: {
                        let mut rows = Vec::new();
                        if project_profile.exists() {
                            rows.push(row(
                                "init.profile.project_migrate",
                                "Legacy project profile detected",
                                "Keep compatibility as-is, or copy .omegon/profile.json into .omegon/profiles/ and select it for this project.",
                                Some("decision".into()),
                                "detected",
                                MenuBadgeTone::Warning,
                                "/init profile migrate --project",
                                true,
                            ));
                        }
                        if user_profile.as_ref().is_some_and(|path| path.exists()) {
                            rows.push(row(
                                "init.profile.user_migrate",
                                "Legacy user profile detected",
                                "Keep compatibility as-is, or copy ~/.omegon/profile.json into ~/.omegon/profiles/ and select it as the user fallback.",
                                Some("decision".into()),
                                "detected",
                                MenuBadgeTone::Warning,
                                "/init profile migrate --user",
                                true,
                            ));
                        }
                        if rows.is_empty() {
                            rows.push(MenuRowProjection {
                                id: "init.decisions.none".into(),
                                label: "No compatibility decisions detected".into(),
                                description: "No legacy mid-state prompts are currently needed.".into(),
                                value: Some("clean".into()),
                                kind: MenuRowKind::Object,
                                badges: vec![MenuBadgeProjection {
                                    label: "clean".into(),
                                    tone: MenuBadgeTone::Success,
                                }],
                                metadata: vec![],
                                primary_action: None,
                                actions: vec![],
                                safety: None,
                                availability: None,
                            });
                        }
                        rows
                    },
                },
                MenuGroupProjection {
                    id: "init.analysis".into(),
                    label: "Codebase orientation".into(),
                    description: Some("Evidence-led repository analysis is distinct from harness mutation. Load the bundled skill to assess architecture, workflow, directive drift, and strategic nested guidance before applying changes.".into()),
                    rows: vec![row(
                        "init.analysis.skill",
                        "Inspect the codebase initialization playbook",
                        "Load the first-order codebase-init skill before analyzing an unfamiliar repository or refreshing AGENTS.md guidance. Inspection is read-only until the operator approves an edit plan.",
                        Some("bundled skill".into()),
                        "inspect",
                        MenuBadgeTone::Info,
                        "/skills get codebase-init",
                        false,
                    )],
                },
                MenuGroupProjection {
                    id: "init.skills".into(),
                    label: "Recommended skills".into(),
                    description: Some("Skills whose declared project_signals match this repository. Inspect before enabling or copying anything project-local.".into()),
                    rows: skill_rows,
                },
                MenuGroupProjection {
                    id: "init.bootstrap".into(),
                    label: "Harness bootstrap".into(),
                    description: Some("Standard initialization fast paths for missing harness substrate.".into()),
                    rows: vec![
                        row(
                            "init.scan",
                            "Initialize missing harness defaults",
                            "Create missing non-destructive harness substrate such as .omegon/ and ai/memory/, import discovered directives, and report layout.",
                            None,
                            "safe",
                            MenuBadgeTone::Info,
                            "/init scan",
                            false,
                        ),
                        row(
                            "init.migrate",
                            "Repair legacy layout",
                            "Optional repair action: move supported legacy docs/OpenSpec paths into ai/ where applicable.",
                            None,
                            "moves",
                            MenuBadgeTone::Warning,
                            "/init migrate",
                            true,
                        ),
                    ],
                },
            ],
        }];
        if let Some(path) = user_active
            && path.exists()
        {
            menu.summary = menu.summary.map(|summary| {
                format!("{summary}\nUser active profile pointer: {}", path.display())
            });
        }
        menu
    }

    fn open_init_menu(&mut self) {
        self.open_menu_projection(self.init_menu_projection());
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
        let source_line = match &drift.source {
            crate::settings::ProfileSource::Project(path) => {
                format!("profile: project · file: {}", path.display())
            }
            crate::settings::ProfileSource::User(path) => {
                format!("profile: user · file: {}", path.display())
            }
            crate::settings::ProfileSource::BuiltInDefault => {
                "profile: built-in defaults".to_string()
            }
        };
        let drift_value = if drift.changed_count > 0 {
            format!("Δ{}", drift.changed_count)
        } else {
            "clean".into()
        };
        let mut menu = MenuProjection::new("profile", "Profile");
        menu.summary = Some(format!(
            "Persisted profile controls. {source_line}; runtime drift: {drift_value}."
        ));
        menu.footer = Some("↑/↓ navigate · / filter · Enter use/view · s save · explicit /profile apply to apply · Esc close".into());
        let registry = crate::settings::ProfileRegistry::discover(self.cwd());
        let active_source = drift.source.clone();
        let mut registry_rows: Vec<MenuRowProjection> = registry
            .entries
            .iter()
            .filter(|entry| {
                entry.source_kind != crate::settings::ProfileRegistrySourceKind::BuiltInDefault
            })
            .map(|entry| {
                let is_active = entry
                    .path
                    .as_ref()
                    .is_some_and(|path| match &active_source {
                        crate::settings::ProfileSource::Project(active)
                        | crate::settings::ProfileSource::User(active) => active == path,
                        crate::settings::ProfileSource::BuiltInDefault => {
                            entry.source_kind
                                == crate::settings::ProfileRegistrySourceKind::BuiltInDefault
                        }
                    });
                let mut badges = vec![MenuBadgeProjection {
                    label: entry.scope.as_str().into(),
                    tone: MenuBadgeTone::Info,
                }];
                if is_active {
                    badges.push(MenuBadgeProjection {
                        label: "active".into(),
                        tone: MenuBadgeTone::Success,
                    });
                }
                if entry.source_kind == crate::settings::ProfileRegistrySourceKind::LegacySingleton
                {
                    badges.push(MenuBadgeProjection {
                        label: "legacy".into(),
                        tone: MenuBadgeTone::Neutral,
                    });
                }
                MenuRowProjection {
                    id: format!("profile.registry.{}.{}", entry.scope.as_str(), entry.id),
                    label: entry.id.clone(),
                    description: entry
                        .profile
                        .display_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("Saved profile")
                        .to_string(),
                    value: Some(entry.scope.as_str().into()),
                    kind: MenuRowKind::Object,
                    badges,
                    metadata: vec![
                        format!("source: {:?}", entry.source_kind),
                        entry
                            .path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "built-in".into()),
                    ],
                    primary_action: (!is_active).then(|| {
                        MenuActionProjection::command(
                            format!("profile.use.{}.{}", entry.scope.as_str(), entry.id),
                            "Use",
                            format!(
                                "/profile use {} {}",
                                shlex::try_quote(&entry.id).unwrap_or_else(|_| "''".into()),
                                entry.scope.as_str()
                            ),
                        )
                    }),
                    actions: vec![],
                    safety: None,
                    availability: None,
                }
            })
            .collect();
        if registry_rows.is_empty() {
            registry_rows.push(MenuRowProjection {
                id: "profile.registry.empty".into(),
                label: "No profiles discovered".into(),
                description: "Create a project or user profile to switch dynamically.".into(),
                value: None,
                kind: MenuRowKind::Object,
                badges: vec![MenuBadgeProjection {
                    label: "empty".into(),
                    tone: MenuBadgeTone::Neutral,
                }],
                metadata: vec![],
                primary_action: None,
                actions: vec![],
                safety: None,
                availability: None,
            });
        }
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
                        id: "profile.save_named_user".into(),
                        label: "Save as named user profile".into(),
                        description: "Capture runtime settings to a named profile in ~/.omegon/profiles/<name>.json.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![
                            MenuBadgeProjection {
                                label: "writes".into(),
                                tone: MenuBadgeTone::Warning,
                            },
                            MenuBadgeProjection {
                                label: "user".into(),
                                tone: MenuBadgeTone::Info,
                            },
                        ],
                        metadata: vec!["/profile save --name <name>".into()],
                        primary_action: Some(MenuActionProjection::prime_editor(
                            "profile.save_named_user.primary",
                            "Name & save (user)",
                            "/profile save --name ",
                            "Type the profile name and press Enter — saved to ~/.omegon/profiles/<name>.json",
                        )),
                        actions: vec![],
                        safety: None,
                        availability: None,
                    },
                    MenuRowProjection {
                        id: "profile.save_named_project".into(),
                        label: "Save as named project profile".into(),
                        description: "Capture runtime settings to a named profile in .omegon/profiles/<name>.json.".into(),
                        value: None,
                        kind: MenuRowKind::Action,
                        badges: vec![
                            MenuBadgeProjection {
                                label: "writes".into(),
                                tone: MenuBadgeTone::Warning,
                            },
                            MenuBadgeProjection {
                                label: "project".into(),
                                tone: MenuBadgeTone::Info,
                            },
                        ],
                        metadata: vec!["/profile save --name <name> --project".into()],
                        primary_action: Some(MenuActionProjection::prime_editor(
                            "profile.save_named_project.primary",
                            "Name & save (project)",
                            "/profile save --name ",
                            "Type the profile name followed by ' --project' and press Enter — saved to .omegon/profiles/<name>.json",
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
            },
            MenuGroupProjection {
                id: "profile.available".into(),
                label: "Available profiles".into(),
                description: Some(
                    "Discovered user and project profiles. Enter switches to the selected profile."
                        .into(),
                ),
                rows: registry_rows,
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
        if let Ok(Some(cp)) = self.dashboard_handles.observe_cleave() {
            self.dashboard.cleave = Some(cp);
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
        let settings = self.settings_projection();
        let settings_snapshot = self.settings();
        let loaded_profile = crate::settings::Profile::load_with_source(self.cwd());
        let profile_drift =
            crate::surfaces::profile::ProfileDriftProjection::from_profile_and_settings(
                &loaded_profile.profile,
                loaded_profile.source,
                &settings_snapshot,
            );
        settings_menu_projection::build_settings_menu_projection(
            settings_menu_projection::SettingsMenuInputs::new(settings, profile_drift),
        )
    }

    fn open_skills_menu(&mut self) -> Result<(), String> {
        let entries = crate::skills::list_structured()
            .map_err(|err| format!("/skills list failed: {err}"))?;
        if entries.is_empty() {
            return Err("No skills found. Run /skills install to install bundled skills.".into());
        }
        let projection = crate::operator_commands::skills_menu_projection(&entries);
        self.open_menu_projection(projection);
        Ok(())
    }

    fn queue_settings_profile_save(&mut self, tx: &OperatorCommandTx) {
        let Some(request) = crate::operator_commands::control_request_from_slash_command(
            &CanonicalSlashCommand::ProfileCapture(
                crate::settings::ProfileSaveTarget::ActiveSource,
            ),
        ) else {
            return;
        };
        let _ = tx.try_send(TuiCommand::ExecuteControl {
            request,
            respond_to: None,
        });
        self.show_command_toast(CommandToast::new(
            "Saving runtime drift with /profile save",
            CommandSeverity::Info,
        ));
    }

    fn queue_settings_profile_apply(&mut self, tx: &OperatorCommandTx) {
        let Some(request) = crate::operator_commands::control_request_from_slash_command(
            &CanonicalSlashCommand::ProfileApply,
        ) else {
            return;
        };
        let _ = tx.try_send(TuiCommand::ExecuteControl {
            request,
            respond_to: None,
        });
        self.show_command_toast(CommandToast::new(
            "Applying profile defaults with /profile apply",
            CommandSeverity::Info,
        ));
    }

    fn rebuild_active_menu(&mut self, target: MenuRefreshTarget) -> bool {
        let projection = match target {
            MenuRefreshTarget::Ui => self.ui_menu_projection(),
            MenuRefreshTarget::ExtensionRuntime => self.extension_runtime_menu_projection(),
            MenuRefreshTarget::ExtensionDetail(extension_name) => {
                let Some(projection) = self.extension_detail_menu_projection(&extension_name)
                else {
                    return false;
                };
                projection
            }
            MenuRefreshTarget::Unsupported(_) => return false,
        };
        self.active_menu = Some(ActiveMenu::new(projection));
        self.pending_menu_confirmation = None;
        true
    }

    fn execute_active_menu_command(
        &mut self,
        command: String,
        tx: &OperatorCommandTx,
    ) -> MenuCommandOutcome {
        let slash_result = self.handle_slash_command(&command, tx);
        let secret_input = matches!(self.editor.mode(), editor::EditorMode::SecretInput { .. });
        let outcome = MenuCommandOutcome::from_slash_result(slash_result, secret_input);

        self.apply_menu_command_outcome(&command, &outcome);

        outcome
    }

    fn provider_route_snapshot(&self) -> auth_menu_projection::ProviderRouteSnapshot {
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
        auth_menu_projection::ProviderRouteSnapshot {
            selected_provider,
            serving_provider,
            route_state: self.route_state.clone(),
        }
    }

    fn provider_status_rows(
        &self,
        row_prefix: &str,
    ) -> Vec<crate::surfaces::menu::MenuRowProjection> {
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
        auth_menu_projection::build_provider_status_rows(
            row_prefix,
            provider_ids
                .into_iter()
                .map(crate::surfaces::menu::ProviderStatusProjection::from_credential_probe)
                .collect(),
            &self.provider_route_snapshot(),
        )
    }

    fn open_auth_menu(&mut self) {
        let providers = crate::auth::operator_auth_provider_ids()
            .into_iter()
            .map(crate::surfaces::menu::ProviderStatusProjection::from_credential_probe)
            .collect();
        let menu = auth_menu_projection::build_authentication_menu(
            auth_menu_projection::AuthenticationMenuInputs {
                providers,
                route: self.provider_route_snapshot(),
                selected_model: self.route_selected_model.clone(),
                serving_model: self.route_serving_model.clone(),
                route_warning: self.footer_data.route_warning.clone(),
            },
        );
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

    fn open_settings_row(&mut self, target: SettingsRowTarget) {
        let Some(row_id) = target.id() else {
            return;
        };
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

        match target.action() {
            SettingsRowAction::OpenModelSelector => {
                self.active_menu = None;
                self.pending_menu_confirmation = None;
                self.open_model_selector();
            }
            SettingsRowAction::OpenMaxTurnsSelector => {
                self.active_menu = None;
                self.pending_menu_confirmation = None;
                self.open_max_turns_selector();
            }
            SettingsRowAction::ToggleSandbox => self.toggle_settings_sandbox(),
            SettingsRowAction::ToggleAutoUpdate => self.toggle_settings_auto_update(),
            SettingsRowAction::ExplainTrustedDirectories => {
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
            SettingsRowAction::ProjectedEditor => self.show_command_toast(CommandToast::new(
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

        let runtime = crate::container_runtime::detect();
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

    fn confirm_selector(&mut self, tx: &OperatorCommandTx) -> Option<String> {
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
                    let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(
                            &CanonicalSlashCommand::SetContextClass(class),
                        )
                    else {
                        return Some("Context class update is unavailable".to_string());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
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
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(
                            &CanonicalSlashCommand::SecretsView,
                        )
                    {
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
                    | "firecrawl" | "huggingface" => {
                        // Map to the correct env var name for storage
                        let key_name = match value.as_str() {
                            "openai" => "OPENAI_API_KEY",
                            "openrouter" => "OPENROUTER_API_KEY",
                            "ollama-cloud" => "OLLAMA_API_KEY",
                            "brave" => "BRAVE_API_KEY",
                            "tavily" => "TAVILY_API_KEY",
                            "serper" => "SERPER_API_KEY",
                            "firecrawl" => "FIRECRAWL_API_KEY",
                            "huggingface" => "HUGGING_FACE_TOKEN",
                            _ => unreachable!(),
                        };
                        let acquisition =
                            crate::capabilities::secrets::secret_console_url(key_name);
                        if let Some(url) = acquisition {
                            let url = url.to_string();
                            std::thread::spawn(move || {
                                let _ = open::that(url);
                            });
                        }
                        self.editor.start_secret_input(key_name);
                        // A login selector can be opened from the auth menu. Once hidden
                        // input owns the keyboard, remove that underlying menu so the
                        // first pasted character/Enter is not intercepted by stale UI.
                        self.active_menu = None;
                        Some(if acquisition.is_some() {
                            format!(
                                "Opening the {value} key console… 🔒 paste {key_name} here (input is hidden):"
                            )
                        } else {
                            format!("🔒 Paste your {value} API key (input is hidden):")
                        })
                    }
                    "github" => {
                        // GitHub uses dynamic resolution via gh CLI
                        if let Some(request) =
                            crate::operator_commands::control_request_from_slash_command(
                                &CanonicalSlashCommand::SecretsSet {
                                    name: "GITHUB_TOKEN".to_string(),
                                    value: "cmd:gh auth token".to_string(),
                                },
                            )
                        {
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
                        // Direct value — enter masked secret input mode. Search
                        // providers additionally open their fixed key-console URL
                        // in the operator's browser; the key itself never transits
                        // model context.
                        let acquisition = crate::capabilities::secrets::secret_console_url(&value);
                        if let Some(url) = acquisition {
                            let url = url.to_string();
                            std::thread::spawn(move || {
                                let _ = open::that(url);
                            });
                        }
                        self.editor.start_secret_input(&value);
                        Some(if acquisition.is_some() {
                            format!(
                                "Opening the provider key console… 🔒 paste {value} here (input is hidden):"
                            )
                        } else {
                            format!("🔒 Paste or type value for {value} (input is hidden):")
                        })
                    } else {
                        // Dynamic recipe — set immediately
                        if let Some(request) =
                            crate::operator_commands::control_request_from_slash_command(
                                &CanonicalSlashCommand::SecretsSet {
                                    name: value.clone(),
                                    value: suggested.to_string(),
                                },
                            )
                        {
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
                    let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(
                            &CanonicalSlashCommand::WorkspaceRoleSet(role),
                        )
                    else {
                        return Some("Workspace role update is unavailable".to_string());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::WorkspaceKind => {
                let outcome = settings_menu::apply_workspace_kind_selection(&value);
                if let settings_menu::SettingApplyOutcome::WorkspaceKind(kind) = outcome {
                    let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(
                            &CanonicalSlashCommand::WorkspaceKindSet(kind),
                        )
                    else {
                        return Some("Workspace kind update is unavailable".to_string());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
                        respond_to: None,
                    });
                }
                Some(outcome.message())
            }
            SelectorKind::MaxTurns => {
                let outcome = settings_menu::apply_max_turns_selection(&value);
                if let settings_menu::SettingApplyOutcome::MaxTurns(max_turns) = outcome {
                    let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(
                            &CanonicalSlashCommand::SetMaxTurns { max_turns },
                        )
                    else {
                        return Some("Max turns update is unavailable".to_string());
                    };
                    let _ = tx.try_send(TuiCommand::ExecuteControl {
                        request,
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

    fn publish_stream_presentation(&mut self) -> Option<(u64, u64)> {
        let publication = self.stream_presentation.publish()?;
        for (kind, delta) in publication.deltas {
            if delta.is_empty() {
                continue;
            }
            let was_streaming = self.conversation.is_streaming();
            match kind {
                streaming_presentation::StreamContentKind::Assistant => {
                    self.conversation.append_streaming(&delta);
                    self.slim_turn_state = SlimTurnState::Responding;
                }
                streaming_presentation::StreamContentKind::Thinking => {
                    self.conversation.append_thinking(&delta);
                    self.slim_turn_state = SlimTurnState::Thinking;
                    self.instrument_panel.note_thinking_activity();
                }
            }
            if !was_streaming {
                self.conversation.stamp_meta(self.current_meta());
            }
        }
        Some((publication.generation, publication.revision))
    }

    fn acknowledge_stream_presentation_draw(&mut self, generation: u64, revision: u64) -> bool {
        self.stream_presentation
            .acknowledge_draw(generation, revision);
        let mut released = false;
        while let Some(event) = self.stream_presentation.take_drawn_event() {
            self.handle_agent_event(event);
            released = true;
        }
        debug_assert!(
            !self.stream_presentation.has_blocked_events(),
            "drawn deferred events must drain completely"
        );
        released
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
        ("FIRECRAWL_API_KEY", "", "Firecrawl Search API"),
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
                description: if let Some(url) =
                    crate::capabilities::secrets::secret_console_url(name)
                {
                    format!("opens {url} → masked key input")
                } else if recipe.is_empty() {
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
    fn handle_variables(&mut self, args: &str, tx: &OperatorCommandTx) -> SlashResult {
        if args.trim().is_empty() {
            self.open_variables_menu();
            return SlashResult::Handled;
        }
        if let Some(command) = canonical_slash_command("variables", args) {
            if let Some(request) =
                crate::operator_commands::control_request_from_slash_command(&command)
            {
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
    fn handle_secrets(&mut self, args: &str, tx: &OperatorCommandTx) -> SlashResult {
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
                SlashResult::Display(format!("Paste {name} — input hidden"))
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
                            crate::operator_commands::control_request_from_slash_command(&command)
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
                    SlashResult::Display(format!("Paste {name} — input hidden"))
                }
            }
            _ => {
                if let Some(command) = canonical_slash_command("secrets", args) {
                    if let Some(request) =
                        crate::operator_commands::control_request_from_slash_command(&command)
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
        tx: &OperatorCommandTx,
        prompt: PromptSubmission,
    ) -> Result<(), SlashResult> {
        tx.try_send(TuiCommand::SubmitPrompt(prompt)).map_err(|_| {
            SlashResult::Display(
                "Runtime command queue is full; prompt was not queued. Try again shortly.".into(),
            )
        })
    }

    /// Handle /tutorial — start, resume, or manage the interactive tutorial overlay.
    fn handle_tutorial(&mut self, args: &str, tx: &OperatorCommandTx) -> SlashResult {
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
    fn handle_tutorial_next(&mut self, tx: &OperatorCommandTx) -> SlashResult {
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
    fn handle_tutorial_prev(&mut self, tx: &OperatorCommandTx) -> SlashResult {
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

    async fn submit_editor_buffer(&mut self, command_tx: &OperatorCommandTx) {
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
        command_tx: &OperatorCommandTx,
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
        self.dashboard_handles.session().set_busy(true);
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
        command_tx: &OperatorCommandTx,
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
        self.dashboard_handles.session().set_busy(true);
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
        self.conversation.conv_state.force_scroll_to_bottom();
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
        if let Some(path) = crate::native_io::clipboard_image_to_temp() {
            self.show_toast(
                "📎 Image pasted — send a message to include it",
                ratatui_toaster::ToastType::Info,
            );
            self.editor.insert_attachment(path);
        }
    }

    fn copy_text_to_clipboard(&self, text: &str) -> bool {
        crate::native_io::copy_text_to_clipboard(text)
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

    fn build_session_transcript_with_policy(
        &self,
        mode: SegmentExportMode,
        policy: conversation_projection::ConversationExportPolicy,
    ) -> String {
        let projection = conversation_projection::project_conversation_for_export(
            self.conversation.segments(),
            policy,
        );
        let segments = projection.segments.as_slice();
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

    fn build_session_transcript(&self, mode: SegmentExportMode) -> String {
        self.build_session_transcript_with_policy(
            mode,
            conversation_projection::ConversationExportPolicy::Semantic,
        )
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
        match self.declared_surface(command) {
            Some(omegon_traits::CommandSurface::Panel) => {
                self.open_command_panel(CommandPanel::from_slash(command, response));
                return;
            }
            Some(omegon_traits::CommandSurface::Inline) if !response.trim().is_empty() => {
                // Declared inline, but errors and long output still deserve a
                // readable surface — fall through to the shape heuristics.
            }
            _ => {}
        }
        if matches!(self.editor.mode(), editor::EditorMode::SecretInput { .. }) {
            // Secret entry already owns the normal editor. Keep its acquisition
            // guidance compact instead of obscuring the workspace with a modal.
            self.command_panel = None;
            self.show_command_toast(CommandToast::new(response, CommandSeverity::Info));
        } else if response.starts_with("Unknown command: /") {
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

    /// Presentation declared by the command itself: feature-provided bus
    /// commands first, then the built-in registry.
    fn declared_surface(&self, command: &str) -> Option<omegon_traits::CommandSurface> {
        declared_command_surface(&self.bus_commands, command).or_else(|| {
            declared_command_surface(
                &crate::command_registry::builtin_command_definitions(),
                command,
            )
        })
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
            for event in registry.skill_activation_events() {
                self.conversation.push_skill_event(event);
            }
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
                    "## Reload complete\n\nYour session stayed open. Skills active for future requests: {skills_before} → {skills_after}.\n\nReloaded now:\n- User, project, and extension-provided skills\n- Inference inventory generation {before_generation} → {}\n\nNot restarted:\n- Running extension processes and widgets\n- The Omegon executable\n\nUse `/runtime restart` after installing new Omegon code or when a component explicitly says a process restart is required.\n\nDetails:\n- Extension candidates found: {}\n- Skipped by policy: {}\n- Disabled extensions: {}\n- Invalid extension manifests: {}\n- Registered commands: {}",
                    self.runtime_generation,
                    dry_run.extension_candidates,
                    dry_run.skipped_by_policy,
                    dry_run.disabled_extensions,
                    invalid,
                    self.bus_commands.len(),
                )
            }
            Err(err) => format!(
                "Reload partially completed: skills were reloaded, but extension/runtime inspection failed: {err}\n\nThe current session remains usable. Run `/runtime status` for current state or `/runtime restart` if a full process restart is required."
            ),
        }
    }

    fn selected_terminal_session_id(&self) -> Option<String> {
        let segment = self.conversation.selected_segment()?;
        let crate::tui::segments::SegmentContent::ToolCard {
            name,
            detail_result,
            ..
        } = &segment.content
        else {
            return None;
        };
        if name != "terminal" {
            return None;
        }
        let result = detail_result.as_deref()?;
        let after = ["Started terminal '", "Terminal session '"]
            .into_iter()
            .find_map(|marker| {
                result
                    .find(marker)
                    .map(|index| &result[index + marker.len()..])
            })?;
        let (_, after_name) = after.split_once("' (")?;
        let (id, _) = after_name.split_once(')')?;
        crate::tools::terminal::execution_session_snapshot_by_id(id).map(|snapshot| snapshot.id)
    }

    fn open_selected_terminal_process_viewer(&mut self) -> bool {
        let Some(segment) = self.conversation.selected_segment() else {
            return false;
        };
        let crate::tui::segments::SegmentContent::ToolCard { name, .. } = &segment.content else {
            return false;
        };
        if name != "terminal" {
            return false;
        }

        // A terminal card such as `list` may not carry a session id even when a
        // later `start` result has been collapsed out of the Slim projection.
        // Prefer the selected card's retained id, then use the same running/most-
        // recent fallback as `/processes` rather than dropping into generic copy.
        let session_id = self.selected_terminal_session_id().unwrap_or_default();
        self.open_process_viewer(&session_id);
        self.process_viewer.is_some()
    }

    fn open_process_viewer(&mut self, session: &str) {
        let requested = (!session.is_empty()).then(|| {
            crate::tools::terminal::execution_session_snapshot_by_id(session)
                .map(|snapshot| snapshot.id)
                .unwrap_or_else(|| session.to_string())
        });
        let session_id = requested.or_else(|| {
            crate::tools::terminal::execution_session_snapshots()
                .into_iter()
                .find(|snapshot| {
                    snapshot.state == crate::tools::terminal::ExecutionSessionState::Running
                })
                .or_else(|| {
                    crate::tools::terminal::execution_session_snapshots()
                        .into_iter()
                        .next()
                })
                .map(|snapshot| snapshot.id)
        });
        if let Some(session_id) = session_id {
            self.process_viewer = Some(process_viewer::ProcessViewerState::new(session_id));
        } else {
            self.show_command_toast(CommandToast::new(
                "No managed background processes or terminal sessions are retained",
                CommandSeverity::Info,
            ));
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

    /// Untyped suffix for the first registry-ranked command match. Keeping this
    /// derived from the shared command projection makes the editor hint and
    /// palette agree without a renderer-local command inventory.
    fn command_ghost_suffix(&self) -> Option<String> {
        let typed = self.editor.render_text();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            return None;
        }
        let command = self.matching_commands().first()?.command.clone();
        command
            .strip_prefix(&typed)
            .filter(|suffix| !suffix.is_empty())
            .map(str::to_string)
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
            .map(|path| {
                let label = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&path)
                    .to_string();
                let description = std::path::Path::new(&path)
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(|parent| {
                        format!("{} · reference; agent inspects on demand", parent.display())
                    })
                    .unwrap_or_else(|| {
                        "project root · reference; agent inspects on demand".to_string()
                    });
                selector::SelectOption {
                    value: path,
                    label,
                    description,
                    active: false,
                }
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
        self.at_picker = Some(selector::Selector::new("Reference project file", options));
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
    pub initial: crate::setup::InteractiveInitialState,
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
    pub debug_tui: bool,
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

fn merge_zed_agent_server(
    content: &str,
    omegon_entry: serde_json::Value,
) -> anyhow::Result<String> {
    let parsed = jsonc_parser::parse_to_serde_value(
        content,
        &jsonc_parser::ParseOptions {
            allow_comments: true,
            allow_loose_object_property_names: false,
            allow_trailing_commas: true,
        },
    )?
    .unwrap_or_else(|| serde_json::json!({}));
    let mut settings = parsed;
    let root = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Zed settings root must be a JSON object"))?;
    let servers = root
        .entry("agent_servers")
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Zed `agent_servers` must be a JSON object"))?;
    servers.insert("Omegon".to_string(), omegon_entry);
    Ok(serde_json::to_string_pretty(&settings)? + "\n")
}

#[cfg(test)]
mod editor_config_tests {
    use super::merge_zed_agent_server;

    #[test]
    fn merges_zed_jsonc_with_comments_and_trailing_commas() {
        let content = r#"{
  // Existing editor preference
  "theme": "One Dark",
  "agent_servers": {
    "Other": { "command": "other", },
  },
}"#;
        let merged = merge_zed_agent_server(
            content,
            serde_json::json!({"type":"custom","command":"omegon","args":["acp"]}),
        )
        .expect("JSONC merges");
        let parsed: serde_json::Value = serde_json::from_str(&merged).expect("normalized JSON");

        assert_eq!(parsed["theme"], "One Dark");
        assert_eq!(parsed["agent_servers"]["Other"]["command"], "other");
        assert_eq!(parsed["agent_servers"]["Omegon"]["command"], "omegon");
    }

    #[test]
    fn refuses_invalid_zed_settings_instead_of_replacing_them() {
        let error = merge_zed_agent_server(
            "{ this is not valid JSONC }",
            serde_json::json!({"command":"omegon"}),
        )
        .expect_err("invalid settings must fail closed");

        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn refuses_non_object_agent_servers() {
        let error = merge_zed_agent_server(
            r#"{ "agent_servers": [] }"#,
            serde_json::json!({"command":"omegon"}),
        )
        .expect_err("wrong shape must fail closed");

        assert!(error.to_string().contains("agent_servers"));
    }
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
                let content = std::fs::read_to_string(&config_path).unwrap_or_default();
                match merge_zed_agent_server(&content, omegon_entry) {
                    Ok(json) => {
                        if crate::filelock::atomic_write_locked(&config_path, json.as_bytes())
                            .is_ok()
                        {
                            result_lines.push(format!(
                                "✓ Added or updated Omegon in {}",
                                config_path.display()
                            ));
                        } else {
                            result_lines
                                .push(format!("✗ Failed to write {}", config_path.display()));
                        }
                    }
                    Err(error) => result_lines.push(format!(
                        "✗ Refused to modify {}: {error}",
                        config_path.display()
                    )),
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

fn drain_agent_events_budgeted(
    events_rx: &mut broadcast::Receiver<AgentEvent>,
    app: &mut App,
    budget: AgentDrainBudget,
) -> DrainOutcome {
    let started = std::time::Instant::now();
    let mut handled = 0;

    while handled < budget.max_events && started.elapsed() < budget.max_duration {
        match events_rx.try_recv() {
            Ok(agent_event) => {
                app.handle_agent_event(agent_event);
                handled += 1;
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {
                return DrainOutcome {
                    handled,
                    hit_budget: false,
                };
            }
        }
    }

    DrainOutcome {
        handled,
        hit_budget: handled == budget.max_events || started.elapsed() >= budget.max_duration,
    }
}

fn runtime_contention_snapshot(app: &App) -> runtime_trace::RuntimeContentionSnapshot {
    let terminal_sessions = crate::tools::terminal::execution_session_snapshots();
    runtime_trace::RuntimeContentionSnapshot {
        process_rss_mb: get_rss_mb().unwrap_or_default().round() as u64,
        managed_terminal_sessions: terminal_sessions.len() as u64,
        running_terminal_sessions: terminal_sessions
            .iter()
            .filter(|session| {
                session.state == crate::tools::terminal::ExecutionSessionState::Running
            })
            .count() as u64,
        extension_widgets: app.runtime_inventory.extension_widgets as u64,
        extension_rpc_handles: app.runtime_inventory.extension_rpc_handles as u64,
        extension_polling_handles: (app.runtime_inventory.vox_polling_handles
            + app.runtime_inventory.voice_polling_handles)
            as u64,
        widget_receivers: app.runtime_inventory.widget_receivers as u64,
    }
}

pub async fn run_tui(
    mut events_rx: broadcast::Receiver<AgentEvent>,
    command_tx: OperatorCommandTx,
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
    // Mouse capture is ON by default for wheel and pane interaction. Native
    // terminal selection remains available without changing modes by holding
    // Shift while dragging (the standard terminal mouse-capture override).
    // `/mouse off` remains the guaranteed passthrough fallback for terminals
    // that do not implement the Shift override.
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
    // conversation-view invariant. Shift-drag asks the terminal to bypass
    // capture for native selection; `/mouse off` is the guaranteed fallback.
    let mut app = App::new(settings.clone());
    app.mouse_capture_enabled = true;
    app.keyboard_enhancement = has_keyboard_enhancement;
    app.secret_readiness = config.secret_readiness.clone();
    if let Some(snapshot) = app.secret_readiness.as_ref() {
        app.footer_data.web_search_providers =
            crate::capabilities::secrets::web_search_provider_readiness(snapshot);
    }
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

    // ── Splash screen with live capability inspection ─────────────
    if !config.no_splash {
        startup_splash::run_startup_splash(
            &mut terminal,
            &mut app,
            &mut events_rx,
            config.cwd.clone(),
        )
        .await?;
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

    let mut scheduler = TuiFrameScheduler::new(std::time::Instant::now());
    let mut runtime_trace = runtime_trace::TuiRuntimeTrace::new(config.debug_tui);

    loop {
        // ── Splash replay (/splash command) ─────────────────────────
        if app.replay_splash {
            app.replay_splash = false;
            startup_splash::run_replay_splash(&mut terminal, &mut app).await?;
        }

        // Operator input is latency-sensitive. Service a bounded batch before
        // ingesting producer traffic so streaming cannot starve scrolling,
        // cancellation, or editor control.
        let mut handled_input = false;
        let mut handled_input_count = 0_u64;
        let input_started = std::time::Instant::now();
        for _ in 0..16 {
            if !event::poll(Duration::ZERO)? {
                break;
            }
            let input_event = event::read()?;
            handled_input = true;
            handled_input_count += 1;
            if matches!(
                app.handle_terminal_event(input_event, &command_tx).await,
                InputDisposition::SkipLoop
            ) {
                break;
            }
        }
        if handled_input {
            scheduler.mark_dirty(TuiDrawReason::OperatorInput);
            if let Some(trace) = &mut runtime_trace {
                trace.record_input(handled_input_count, input_started);
            }
        }

        // Agent traffic is throughput-sensitive. Bound each pass by both event
        // count and wall time so token streams cannot monopolize the UI task.
        let agent_drain =
            drain_agent_events_budgeted(&mut events_rx, &mut app, scheduler.agent_budget());
        if agent_drain.handled > 0 {
            scheduler.mark_dirty(TuiDrawReason::BackgroundEvent);
        }
        if let Some(trace) = &mut runtime_trace {
            trace.record_agent_drain(agent_drain.handled, agent_drain.hit_budget);
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
                scheduler.mark_dirty(TuiDrawReason::BackgroundEvent);
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
                            scheduler.mark_dirty(TuiDrawReason::BackgroundEvent);
                        }
                    }
                    crate::extensions::WidgetEvent::ShowModal {
                        widget_id,
                        data,
                        auto_dismiss_ms,
                    } => {
                        app.active_modal =
                            Some((widget_id, data, auto_dismiss_ms, std::time::Instant::now()));
                        scheduler.mark_dirty(TuiDrawReason::BackgroundEvent);
                    }
                    crate::extensions::WidgetEvent::ActionRequired { widget_id, actions } => {
                        app.active_action_prompt = Some((widget_id, actions));
                        scheduler.mark_dirty(TuiDrawReason::BackgroundEvent);
                    }
                }
            }
        }

        // Coalesce background mutations to the frame interval. Operator input
        // remains urgent and draws immediately.
        let now = std::time::Instant::now();
        scheduler.mark_timer_due(now);
        if scheduler.should_draw(now) {
            let urgent = scheduler.is_urgent();
            let publication_revision = app.publish_stream_presentation();
            let draw_started = std::time::Instant::now();
            let mut callback_elapsed = Duration::ZERO;
            terminal.draw(|f| {
                let callback_started = std::time::Instant::now();
                app.draw(f);
                callback_elapsed = callback_started.elapsed();
            })?;
            if let Some((generation, revision)) = publication_revision
                && app.acknowledge_stream_presentation_draw(generation, revision)
            {
                scheduler.mark_dirty(TuiDrawReason::BackgroundEvent);
            }
            let draw_finished = std::time::Instant::now();
            if let Some(trace) = &mut runtime_trace {
                let segments = app.conversation.segments().len();
                let scroll_offset = app.conversation.conv_state.scroll_offset;
                let streaming = app.conversation.is_streaming();
                let detached = app.conversation.conv_state.user_scrolled || scroll_offset > 0;
                trace.record_draw(runtime_trace::DrawObservation {
                    urgent,
                    elapsed: draw_finished.duration_since(draw_started),
                    callback_elapsed,
                    phases: app.last_draw_phase_timings,
                    completed_at: draw_finished,
                    conversation_segments: segments,
                    scroll_offset,
                    streaming,
                    detached,
                });
                trace.flush_if_due(draw_finished, runtime_contention_snapshot(&app));
            }
            scheduler.after_draw(draw_finished);
        } else if let Some(trace) = &mut runtime_trace {
            trace.record_dirty_without_draw();
            trace.flush_if_due(now, runtime_contention_snapshot(&app));
        }

        if app.should_quit {
            break;
        }

        // If the agent budget was exhausted, yield only long enough to service
        // ready input, then continue draining on the next fair scheduling pass.
        let poll_timeout = if agent_drain.hit_budget {
            Duration::ZERO
        } else {
            scheduler.idle_poll_timeout(std::time::Instant::now())
        };
        if event::poll(poll_timeout)? {
            let input_event = event::read()?;
            let input_at = std::time::Instant::now();
            let _ = app.handle_terminal_event(input_event, &command_tx).await;
            scheduler.mark_dirty(TuiDrawReason::OperatorInput);
            if let Some(trace) = &mut runtime_trace {
                trace.record_input(1, input_at);
            }
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
            Some(omegon_traits::PermissionResponse::AllowSession)
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
        let changes = vec![crate::runtime_state::ChangeSummary {
            name: "rollup".into(),
            stage: "implementing".into(),
            done_tasks: 1,
            total_tasks: 3,
        }];
        let focused = crate::runtime_state::FocusedNodeSummary {
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
    fn runtime_reload_and_restart_aliases_refresh_the_runtime_substrate() {
        for args in ["refresh", "reload", "hup", "kick", "restart", "hot-restart"] {
            assert!(matches!(
                canonical_slash_command("runtime", args),
                Some(CanonicalSlashCommand::RuntimeSubstrateRefresh)
            ));
        }
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
                assert!(message.contains("Reload complete"), "{message}");
                assert!(message.contains("session stayed open"), "{message}");
                assert!(message.contains("Skills active"), "{message}");
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
    fn extension_init() {
        match canonical_slash_command("extension", "init telemetry") {
            Some(CanonicalSlashCommand::ExtensionInit(name)) => assert_eq!(name, "telemetry"),
            other => panic!("expected ExtensionInit, got {other:?}"),
        }
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
    fn extension_reload_and_restart_alias_runtime_refresh() {
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
                request: crate::operator_commands::InterfaceControlRequest::ArmoryInstall { target },
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
