//! Shared trait definitions for the Omegon agent runtime.
//!
//! This crate defines the vocabulary shared between the binary, feature
//! modules, and extracted crates (omegon-memory). It provides:
//!
//! - **`Feature`** — the unified trait for integrated features (tools,
//!   context injection, event handling, commands, session lifecycle)
//! - **`BusEvent`** — typed events flowing from the agent loop to features
//! - **`BusRequest`** — typed requests flowing from features back to the runtime
//! - **Legacy traits** — `ToolProvider`, `ContextProvider`, `EventSubscriber`,
//!   `SessionHook` retained for `omegon-memory` compatibility during migration
//!
//! # Architecture
//!
//! ```text
//! Agent Loop ──emit──→ EventBus ──deliver──→ Feature::on_event(&mut self)
//!                          ↑                          │
//!                          └──── BusRequest ──────────┘
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════════════════
// Tool types
// ═══════════════════════════════════════════════════════════════════════════

/// Content block in a tool result — text or image.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { url: String, media_type: String },
}

impl ContentBlock {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            ContentBlock::Image { .. } => None,
        }
    }
}

/// Result returned from a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub details: Value,
}

/// JSON Schema definition for a tool's parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub label: String,
    pub description: String,
    pub parameters: Value,
}

// ═══════════════════════════════════════════════════════════════════════════
// Bus events — flow DOWN from agent loop → features → TUI
// ═══════════════════════════════════════════════════════════════════════════

/// Events emitted by the agent loop and delivered to features.
///
/// These are the typed replacement for pi's `pi.on("event_name")` strings.
/// The bus delivers events to each feature's `on_event(&mut self)` in
/// registration order.
#[derive(Debug, Clone)]
pub enum BusEvent {
    // ── Session lifecycle ───────────────────────────────────────────
    SessionStart {
        cwd: PathBuf,
        session_id: String,
    },
    SessionEnd {
        turns: u32,
        tool_calls: u32,
        duration_secs: f64,
    },

    // ── Turn lifecycle ──────────────────────────────────────────────
    TurnStart {
        turn: u32,
    },
    TurnEnd {
        turn: u32,
    },

    // ── Message streaming ───────────────────────────────────────────
    MessageChunk {
        text: String,
    },
    ThinkingChunk {
        text: String,
    },
    MessageEnd,

    // ── Tool lifecycle ──────────────────────────────────────────────
    ToolStart {
        id: String,
        name: String,
        args: Value,
    },
    ToolEnd {
        id: String,
        name: String,
        result: ToolResult,
        is_error: bool,
    },

    // ── Agent lifecycle ─────────────────────────────────────────────
    AgentEnd,

    // ── Lifecycle subsystem ─────────────────────────────────────────
    PhaseChanged {
        phase: LifecyclePhase,
    },
    DecompositionStarted {
        children: Vec<String>,
    },
    DecompositionChildCompleted {
        label: String,
        success: bool,
    },
    DecompositionCompleted {
        merged: bool,
    },

    // ── Context ─────────────────────────────────────────────────────
    /// Fired before each LLM request. Features can respond by returning
    /// context injections from `provide_context()`.
    ContextBuild {
        user_prompt: String,
        turn: u32,
    },

    /// Context compaction was triggered.
    Compacted,

    // ── Harness status ──────────────────────────────────────────────
    /// Emitted when observable harness state changes (persona switch,
    /// MCP connect/disconnect, secret store unlock, etc.).
    /// Carries the full HarnessStatus snapshot as serialized JSON.
    /// TUI re-renders footer, web dashboard broadcasts over WebSocket.
    HarnessStatusChanged {
        /// Serialized HarnessStatus JSON. Using Value instead of the concrete
        /// type to avoid a circular dependency (omegon-traits → omegon).
        status_json: Value,
    },
}

/// Requests from features back to the runtime.
///
/// Features return these from `on_event()` or accumulate them for the bus
/// to collect after event delivery.
#[derive(Debug, Clone)]
pub enum BusRequest {
    /// Display a notification to the user (TUI hint bar or system message).
    Notify { message: String, level: NotifyLevel },
    /// Inject a system message into the conversation.
    InjectSystemMessage { content: String },
    /// Request context compaction before the next turn.
    RequestCompaction,
    /// Request the harness to refresh and re-emit its status.
    /// Used when features change state that affects the HarnessStatus.
    RefreshHarnessStatus,
}

/// Notification severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyLevel {
    Info,
    Warning,
    Error,
}

// ═══════════════════════════════════════════════════════════════════════════
// Lifecycle phase
// ═══════════════════════════════════════════════════════════════════════════

/// The lifecycle phase the agent loop is currently in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LifecyclePhase {
    #[default]
    Idle,
    Exploring {
        node_id: Option<String>,
    },
    Specifying {
        change_id: Option<String>,
    },
    Decomposing,
    Implementing {
        change_id: Option<String>,
    },
    Verifying {
        change_id: Option<String>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// Slash commands
// ═══════════════════════════════════════════════════════════════════════════

/// Definition of a slash command that a feature registers.
#[derive(Debug, Clone)]
pub struct CommandDefinition {
    /// Command name without the leading `/` (e.g. "compact", "memory").
    pub name: String,
    /// One-line description shown in the command palette.
    pub description: String,
    /// Subcommand completions (e.g. ["200k", "1m"] for /context).
    pub subcommands: Vec<String>,
}

/// Result of handling a slash command.
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Display this text as a system message.
    Display(String),
    /// Command handled silently (e.g. toggled a setting).
    Handled,
    /// This feature doesn't handle this command.
    NotHandled,
}

// ═══════════════════════════════════════════════════════════════════════════
// Context injection
// ═══════════════════════════════════════════════════════════════════════════

/// Signals available to features for deciding what context to inject.
#[derive(Debug)]
pub struct ContextSignals<'a> {
    pub user_prompt: &'a str,
    pub recent_tools: &'a [String],
    pub recent_files: &'a [PathBuf],
    pub lifecycle_phase: &'a LifecyclePhase,
    pub turn_number: u32,
    pub context_budget_tokens: usize,
}

/// A piece of context to inject into the system prompt.
#[derive(Debug, Clone)]
pub struct ContextInjection {
    pub source: String,
    pub content: String,
    pub priority: u8,
    pub ttl_turns: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// The Feature trait — unified interface for integrated features
// ═══════════════════════════════════════════════════════════════════════════

/// A feature is an integrated subsystem that participates in the agent runtime.
///
/// Features can:
/// - Provide tools callable by the agent
/// - Inject context into the system prompt each turn
/// - React to bus events (turns, tool calls, session lifecycle)
/// - Register slash commands
/// - Send requests back to the runtime (notifications, message injection)
///
/// All methods have default no-op implementations so features only override
/// what they need.
///
/// # Lifetime
///
/// Features are created during setup, receive `on_event()` calls for the
/// duration of the session, and are dropped at shutdown. The bus delivers
/// events sequentially in registration order — `&mut self` is safe.
#[async_trait]
pub trait Feature: Send + Sync {
    /// Human-readable name for logging and debugging.
    fn name(&self) -> &str;

    /// Tool definitions this feature provides. Called once at startup.
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![]
    }

    /// Execute a tool call. Only called for tools returned by `tools()`.
    async fn execute(
        &self,
        _tool_name: &str,
        _call_id: &str,
        _args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        anyhow::bail!("not implemented")
    }

    /// Slash commands this feature registers. Called once at startup.
    fn commands(&self) -> Vec<CommandDefinition> {
        vec![]
    }

    /// Handle a slash command. Return `NotHandled` if this feature
    /// doesn't own the command.
    fn handle_command(&mut self, _name: &str, _args: &str) -> CommandResult {
        CommandResult::NotHandled
    }

    /// Provide context for the system prompt this turn.
    /// Called once per turn before the LLM request.
    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        None
    }

    /// React to a bus event. Called sequentially for each event.
    /// Return any requests to send back to the runtime.
    fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
        vec![]
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Legacy traits — retained for omegon-memory compatibility
// ═══════════════════════════════════════════════════════════════════════════

/// Legacy: AgentEvent is retained for the TUI broadcast channel.
/// The bus uses BusEvent internally, but the TUI still receives AgentEvent
/// via tokio::broadcast for rendering. These will converge once the TUI
/// consumes BusEvent directly.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    TurnStart {
        turn: u32,
    },
    MessageStart {
        role: String,
    },
    MessageChunk {
        text: String,
    },
    ThinkingChunk {
        text: String,
    },
    MessageEnd,
    ToolStart {
        id: String,
        name: String,
        args: Value,
    },
    ToolUpdate {
        id: String,
        partial: ToolResult,
    },
    ToolEnd {
        id: String,
        result: ToolResult,
        is_error: bool,
    },
    TurnEnd {
        turn: u32,
        /// Real token estimate from conversation history. Zero on early-exit paths.
        estimated_tokens: usize,
    },
    AgentEnd,
    PhaseChanged {
        phase: LifecyclePhase,
    },
    DecompositionStarted {
        children: Vec<String>,
    },
    DecompositionChildCompleted {
        label: String,
        success: bool,
    },
    DecompositionCompleted {
        merged: bool,
    },
    /// System notification — displayed in TUI but not sent to the LLM.
    SystemNotification {
        message: String,
    },
    /// Harness status changed — persona switch, MCP connect, secret unlock, etc.
    /// Serialized HarnessStatus JSON. Web dashboard renders the snapshot.
    HarnessStatusChanged {
        status_json: Value,
    },
    /// Session was reset mid-session via /new. TUI clears its display.
    SessionReset,
}

/// Session configuration for legacy SessionHook.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub cwd: PathBuf,
    pub session_id: String,
}

/// Session stats for legacy SessionHook.
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub turns: u32,
    pub tool_calls: u32,
    pub duration_secs: f64,
}

/// Legacy: ToolProvider for omegon-memory (will migrate to Feature).
#[async_trait]
pub trait ToolProvider: Send + Sync {
    fn tools(&self) -> Vec<ToolDefinition>;
    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult>;
}

/// Legacy: ContextProvider for omegon-memory.
pub trait ContextProvider: Send + Sync {
    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection>;
}

/// Legacy: EventSubscriber (unused — will be removed).
pub trait EventSubscriber: Send + Sync {
    fn on_event(&self, event: &AgentEvent);
}

/// Legacy: SessionHook for omegon-memory.
#[async_trait]
pub trait SessionHook: Send + Sync {
    async fn on_session_start(&mut self, _config: &SessionConfig) -> anyhow::Result<()> {
        Ok(())
    }
    async fn on_session_end(&mut self, _stats: &SessionStats) -> anyhow::Result<()> {
        Ok(())
    }
}
