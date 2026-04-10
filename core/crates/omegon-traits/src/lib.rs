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
// Native IPC contract — transport-facing Auspex/Omegon boundary (v1)
//
// Transport:  Unix domain socket
// Framing:    [u32 BE length][msgpack envelope bytes]
// Max frame:  8 MiB — oversized frames are a protocol error (disconnect)
// Ordering:   in-order delivery guaranteed on a single connection
// Controller: single controlling client; second attach rejected with Busy
// Disconnect: graceful cancel of any active turn; process continues
// Security:   filesystem permissions on socket path (same-user, mode 0600)
// ═══════════════════════════════════════════════════════════════════════════

pub const IPC_PROTOCOL_VERSION: u16 = 1;
/// Maximum allowed encoded frame size (8 MiB).
pub const IPC_MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

// ── Envelope ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcEnvelopeKind {
    Hello,
    Request,
    Response,
    Event,
    Error,
}

/// Error codes that Omegon may return. Auspex should handle all variants
/// defensively; unknown codes should be treated as `InternalError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcErrorCode {
    UnsupportedProtocolVersion,
    UnknownMethod,
    InvalidPayload,
    InternalError,
    NotSubscribed,
    /// A second controller tried to attach while one is active.
    Busy,
    /// The agent is already processing a turn.
    TurnInProgress,
    /// A shutdown request was accepted; process will exit shortly.
    ShutdownInitiated,
}

impl std::fmt::Display for IpcErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{self:?}"));
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcEnvelope {
    pub protocol_version: u16,
    pub kind: IpcEnvelopeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<[u8; 16]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

impl IpcEnvelope {
    pub fn encode_msgpack(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(self)
    }

    pub fn decode_msgpack(raw: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(raw)
    }

    /// Build a typed error response envelope.
    pub fn error_response(
        request_id: Option<[u8; 16]>,
        code: IpcErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: IPC_PROTOCOL_VERSION,
            kind: IpcEnvelopeKind::Error,
            request_id,
            method: None,
            payload: None,
            error: Some(IpcError {
                code,
                message: message.into(),
                details: None,
            }),
        }
    }
}

// ── Capabilities ────────────────────────────────────────────────────────────

/// Well-known capability tokens used in HelloRequest / HelloResponse.
///
/// Semantics:
/// - A capability advertised by the server means the method/event is available.
/// - A capability NOT advertised by the server must not be relied upon.
/// - Adding a new capability name is a non-breaking change.
/// - Removing a capability is a breaking change requiring a protocol bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcCapability {
    /// `get_state` is available and returns `IpcStateSnapshot`.
    StateSnapshot,
    /// Server pushes `IpcEventPayload` events.
    EventsStream,
    /// `submit_prompt` is available.
    PromptSubmit,
    /// `cancel` is available.
    TurnCancel,
    /// `get_graph` is available.
    GraphRead,
    /// `context_status` is available.
    ContextView,
    /// `context_compact` is available.
    ContextCompact,
    /// `context_clear` is available.
    ContextClear,
    /// `new_session` is available.
    SessionNew,
    /// `list_sessions` is available.
    SessionList,
    /// `auth_status` is available.
    AuthStatus,
    /// `model_view` is available.
    ModelView,
    /// `model_list` is available.
    ModelList,
    /// `set_model` is available.
    ModelSet,
    /// `set_thinking` is available.
    ThinkingSet,
    /// `switch_dispatcher` is available.
    DispatcherSwitch,
    /// `run_slash_command` is available.
    SlashCommands,
    /// `shutdown` is available.
    Shutdown,
}

impl IpcCapability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StateSnapshot => "state.snapshot",
            Self::EventsStream => "events.stream",
            Self::PromptSubmit => "prompt.submit",
            Self::TurnCancel => "turn.cancel",
            Self::GraphRead => "graph.read",
            Self::ContextView => "context.view",
            Self::ContextCompact => "context.compact",
            Self::ContextClear => "context.clear",
            Self::SessionNew => "session.new",
            Self::SessionList => "session.list",
            Self::AuthStatus => "auth.status",
            Self::ModelView => "model.view",
            Self::ModelList => "model.list",
            Self::ModelSet => "model.set",
            Self::ThinkingSet => "thinking.set",
            Self::DispatcherSwitch => "dispatcher.switch",
            Self::SlashCommands => "slash_commands",
            Self::Shutdown => "shutdown",
        }
    }

    /// The full set of capabilities a v1 server exposes.
    pub fn v1_server_set() -> Vec<&'static str> {
        vec![
            Self::StateSnapshot.as_str(),
            Self::EventsStream.as_str(),
            Self::PromptSubmit.as_str(),
            Self::TurnCancel.as_str(),
            Self::GraphRead.as_str(),
            Self::ContextView.as_str(),
            Self::ContextCompact.as_str(),
            Self::ContextClear.as_str(),
            Self::SessionNew.as_str(),
            Self::SessionList.as_str(),
            Self::AuthStatus.as_str(),
            Self::ModelView.as_str(),
            Self::ModelList.as_str(),
            Self::ModelSet.as_str(),
            Self::ThinkingSet.as_str(),
            Self::DispatcherSwitch.as_str(),
            Self::SlashCommands.as_str(),
            Self::Shutdown.as_str(),
        ]
    }
}

// ── Handshake ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloRequest {
    pub client_name: String,
    pub client_version: String,
    /// Client lists every protocol version it supports in preference order.
    /// Server selects the highest version present in both lists.
    pub supported_protocol_versions: Vec<u16>,
    /// Client-advertised capabilities (informational for the server).
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResponse {
    /// Negotiated protocol version (≤ client max, ≤ server max).
    pub protocol_version: u16,
    pub omegon_version: String,
    pub server_name: String,
    pub server_pid: u32,
    pub cwd: String,
    /// Stable opaque identifier for this process lifetime.
    /// Changes on every process restart. Auspex uses this to detect restarts.
    pub server_instance_id: String,
    /// RFC 3339 UTC timestamp when the server process started.
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Server-advertised capabilities. Client must not assume any capability
    /// not present in this list.
    pub capabilities: Vec<String>,
}

// ── Request / Response payloads ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingRequest {
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResponse {
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitPromptRequest {
    pub prompt: String,
    /// Optional hint to the server ("tui", "auspex", "api", …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Optional caller role for transport-side authorization. Defaults to admin
    /// when omitted for backward compatibility with existing clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ControlRequest {
    /// Optional caller role for transport-side authorization. Defaults to admin
    /// when omitted for backward compatibility with existing clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatcherSwitchRequest {
    pub request_id: String,
    pub profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional caller role for transport-side authorization. Defaults to admin
    /// when omitted for backward compatibility with existing clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlOutputResponse {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommandRequest {
    pub name: String,
    pub args: String,
    /// Optional caller role for transport-side authorization. Defaults to admin
    /// when omitted for backward compatibility with existing clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashCommandResponse {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionRequest {
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionResponse {
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DaemonEventEnvelope {
    pub event_id: String,
    pub source: String,
    pub trigger_kind: String,
    pub payload: Value,
    /// Optional caller role for transport-side authorization. Defaults to admin
    /// when omitted for backward compatibility with existing clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_role: Option<String>,
}

// ── Typed state snapshot ─────────────────────────────────────────────────────

/// Top-level attach-time state snapshot. All sections are required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcStateSnapshot {
    pub schema_version: u16,
    pub omegon_version: String,
    pub instance: OmegonInstanceDescriptor,
    pub session: IpcSessionSnapshot,
    pub design_tree: IpcDesignTreeSnapshot,
    pub openspec: IpcOpenSpecSnapshot,
    pub cleave: IpcCleaveSnapshot,
    pub harness: IpcHarnessSnapshot,
    pub health: IpcHealthSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmegonInstanceDescriptor {
    pub schema_version: u16,
    pub identity: OmegonIdentity,
    pub ownership: OmegonOwnership,
    pub placement: OmegonPlacement,
    pub control_plane: OmegonControlPlane,
    pub runtime: OmegonRuntime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmegonIdentity {
    pub instance_id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub role: OmegonRole,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonRuntimeProfile {
    PrimaryInteractive,
    LongRunningDaemon,
    RemoteAgent,
}

impl OmegonRuntimeProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PrimaryInteractive => "primary-interactive",
            Self::LongRunningDaemon => "long-running-daemon",
            Self::RemoteAgent => "remote-agent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonAutonomyMode {
    OperatorDriven,
    GuardedAutonomous,
    Autonomous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmegonOwnership {
    pub owner_kind: OmegonOwnerKind,
    pub owner_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_instance_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmegonPlacement {
    pub kind: OmegonPlacementKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OmegonTransportSecurity {
    LocalIpc,
    InsecureBootstrap,
    Secure,
    IdentityMesh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmegonControlPlane {
    pub server_instance_id: String,
    pub protocol_version: u16,
    pub schema_version: u16,
    pub omegon_version: String,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipc_socket_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_transport_security: Option<OmegonTransportSecurity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_transport_security: Option<OmegonTransportSecurity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmegonRuntime {
    pub deployment_kind: OmegonDeploymentKind,
    pub runtime_mode: OmegonRuntimeMode,
    pub runtime_profile: OmegonRuntimeProfile,
    pub autonomy_mode: OmegonAutonomyMode,
    pub health: OmegonRuntimeHealth,
    pub provider_ok: bool,
    pub memory_ok: bool,
    pub cleave_available: bool,
    pub queued_events: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transport_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonRole {
    PrimaryDriver,
    EmbeddedBackend,
    Delegate,
    Worker,
    RemoteAgent,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonOwnerKind {
    Operator,
    Auspex,
    Dispatcher,
    Cleave,
    Kubernetes,
    Systemd,
    Ci,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonPlacementKind {
    LocalProcess,
    RemoteHost,
    Container,
    KubernetesPod,
    CiRunner,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonDeploymentKind {
    InteractiveTui,
    EmbeddedBackend,
    HomelabService,
    KubernetesWorker,
    CleaveChild,
    RemoteAgent,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonRuntimeMode {
    Standalone,
    AuspexManaged,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmegonRuntimeHealth {
    Ready,
    Degraded,
    Starting,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcSessionSnapshot {
    pub cwd: String,
    pub pid: u32,
    pub started_at: String,
    pub turns: u32,
    pub tool_calls: u32,
    pub compactions: u32,
    pub busy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub git_detached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcDesignTreeSnapshot {
    pub counts: IpcDesignCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<IpcFocusedNode>,
    pub implementing: Vec<IpcNodeBrief>,
    pub actionable: Vec<IpcNodeBrief>,
    pub nodes: Vec<IpcNodeBrief>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcDesignCounts {
    pub total: usize,
    pub seed: usize,
    pub exploring: usize,
    pub resolved: usize,
    pub decided: usize,
    pub implementing: usize,
    pub implemented: usize,
    pub blocked: usize,
    pub deferred: usize,
    pub open_questions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcFocusedNode {
    pub id: String,
    pub title: String,
    pub status: String,
    pub open_questions: Vec<String>,
    pub decisions: usize,
    pub children: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcNodeBrief {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub open_questions: usize,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcOpenSpecSnapshot {
    pub changes: Vec<IpcChangeSnapshot>,
    pub total_tasks: usize,
    pub done_tasks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcChangeSnapshot {
    pub name: String,
    pub stage: String,
    pub total_tasks: usize,
    pub done_tasks: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcCleaveSnapshot {
    pub active: bool,
    pub total_children: usize,
    pub completed: usize,
    pub failed: usize,
    pub children: Vec<IpcChildSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcChildSnapshot {
    pub label: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
}

/// Public projection of `HarnessStatus`. Intentionally curated —
/// not every internal field is exposed. Add fields here deliberately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcHarnessSnapshot {
    pub context_class: String,
    pub thinking_level: String,
    pub capability_tier: String,
    pub runtime_profile: String,
    pub autonomy_mode: String,
    pub dispatcher: IpcDispatcherSnapshot,
    pub memory_available: bool,
    pub cleave_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_warning: Option<String>,
    pub memory: IpcMemorySnapshot,
    pub providers: Vec<IpcProviderSnapshot>,
    pub mcp_server_count: usize,
    pub mcp_tool_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_tone: Option<String>,
    pub active_delegate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcDispatcherSnapshot {
    pub available_options: Vec<String>,
    pub switch_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcMemorySnapshot {
    pub active_facts: usize,
    pub project_facts: usize,
    pub working_facts: usize,
    pub episodes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcProviderSnapshot {
    pub name: String,
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_failure_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcHealthSnapshot {
    pub state: IpcHealthState,
    pub memory_ok: bool,
    pub provider_ok: bool,
    /// RFC 3339 UTC timestamp of last health update.
    pub checked_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcHealthState {
    Ready,
    Degraded,
    Starting,
    Failed,
}

// ── Typed event stream ───────────────────────────────────────────────────────

/// Every event pushed over the wire. Auspex matches on this type.
///
/// Wire shape: `{"name": "turn.started", "data": { "turn": 7 }}`
/// Unit events (no payload) omit the `data` key.
///
/// Stability rules:
/// - Adding a new variant is non-breaking if Auspex ignores unknown event names.
/// - Changing a variant's payload is a breaking change (requires protocol bump).
/// - Unknown variants must be silently ignored by the client, not crashed on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProviderTelemetrySnapshot {
    pub provider: String,
    pub source: String,
    // ── Anthropic ────────────────────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unified_5h_utilization_pct: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unified_7d_utilization_pct: Option<f32>,
    // ── OpenAI / generic ────────────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests_remaining: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_remaining: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    // ── ChatGPT Codex (x-codex-* headers) ───────────────────────────────
    /// Which limit family is active (e.g. "codex").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_active_limit: Option<String>,
    /// Primary window used percent from Codex rate-limit headers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_primary_used_pct: Option<f32>,
    /// Secondary window used percent from Codex rate-limit headers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_secondary_used_pct: Option<f32>,
    /// Seconds until the primary (active) window resets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_primary_reset_secs: Option<u64>,
    /// Seconds until the secondary (longer) window resets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_secondary_reset_secs: Option<u64>,
    /// Whether the account has unlimited credits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_credits_unlimited: Option<bool>,
    /// Model-specific limit name (e.g. "GPT-5.3-Codex-Spark").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_limit_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", content = "data", rename_all = "snake_case")]
pub enum IpcEventPayload {
    // ── Turn lifecycle ──────────────────────────────────────────────────────
    #[serde(rename = "turn.started")]
    TurnStarted { turn: u32 },

    /// Emitted when a turn completes. Always follows all tool and message events.
    #[serde(rename = "turn.ended")]
    TurnEnded {
        turn: u32,
        /// Estimate from local heuristic (chars/4). Always set.
        estimated_tokens: usize,
        /// Actual input tokens billed by the provider this turn. 0 = not reported.
        #[serde(default)]
        actual_input_tokens: u64,
        /// Actual output tokens billed by the provider this turn. 0 = not reported.
        #[serde(default)]
        actual_output_tokens: u64,
        /// Provider-reported cache-read tokens (Anthropic). 0 if not applicable.
        #[serde(default)]
        cache_read_tokens: u64,
        /// Parsed provider quota/headroom telemetry from response headers or status endpoints.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_telemetry: Option<ProviderTelemetrySnapshot>,
    },

    // ── Message streaming ──────────────────────────────────────────────────
    /// Streaming text delta. Every delta is in-order; do not drop.
    #[serde(rename = "message.delta")]
    MessageDelta { text: String },

    #[serde(rename = "thinking.delta")]
    ThinkingDelta { text: String },

    /// Marks the end of one assistant message block.
    #[serde(rename = "message.completed")]
    MessageCompleted,

    // ── Tool lifecycle ─────────────────────────────────────────────────────
    #[serde(rename = "tool.started")]
    ToolStarted {
        id: String,
        name: String,
        /// Serialized tool arguments. May be large; clients may truncate for display.
        args: Value,
    },

    /// Partial result while tool is still running (optional; not all tools emit).
    #[serde(rename = "tool.updated")]
    ToolUpdated { id: String },

    #[serde(rename = "tool.ended")]
    ToolEnded {
        id: String,
        name: String,
        is_error: bool,
        /// Human-readable summary of the result for display purposes.
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },

    // ── Agent lifecycle ────────────────────────────────────────────────────
    /// The agent loop finished all turns and is idle. Always emitted
    /// after the last `turn.ended`.
    #[serde(rename = "agent.completed")]
    AgentCompleted,

    // ── Lifecycle phase ────────────────────────────────────────────────────
    #[serde(rename = "phase.changed")]
    PhaseChanged { phase: String },

    // ── Decomposition ─────────────────────────────────────────────────────
    #[serde(rename = "decomposition.started")]
    DecompositionStarted { children: Vec<String> },

    #[serde(rename = "decomposition.child_completed")]
    DecompositionChildCompleted { label: String, success: bool },

    #[serde(rename = "decomposition.completed")]
    DecompositionCompleted { merged: bool },

    // ── Harness ────────────────────────────────────────────────────────────
    /// Harness state changed. Call `get_state` to refresh the `harness` section.
    #[serde(rename = "harness.changed")]
    HarnessChanged,

    // ── State invalidation ─────────────────────────────────────────────────
    /// One or more snapshot sections are stale. `sections` names which
    /// top-level keys of `IpcStateSnapshot` should be re-fetched.
    #[serde(rename = "state.changed")]
    StateChanged { sections: Vec<String> },

    // ── Notifications ──────────────────────────────────────────────────────
    #[serde(rename = "system.notification")]
    SystemNotification { message: String },

    // ── Session ────────────────────────────────────────────────────────────
    /// Session was reset (e.g. `/new`). Client should clear conversation state.
    #[serde(rename = "session.reset")]
    SessionReset,
}

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

/// A breakdown of prompt/context surface within Omegon's own token-estimation model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSectionMetric {
    pub key: String,
    pub label: String,
    pub chars: usize,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptComposition {
    pub sections: Vec<PromptSectionMetric>,
    pub total_chars: usize,
    pub total_estimated_tokens: usize,
}

/// A breakdown of prompt/context surface within Omegon's own token-estimation model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextComposition {
    pub conversation_tokens: usize,
    pub system_tokens: usize,
    pub memory_tokens: usize,
    /// Provider-request tool definition/schema overhead for the active tool set.
    pub tool_schema_tokens: usize,
    /// Tool-call and tool-result payload preserved in conversation history.
    pub tool_history_tokens: usize,
    pub thinking_tokens: usize,
    pub free_tokens: usize,
}

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
        model: Option<String>,
        provider: Option<String>,
        estimated_tokens: usize,
        context_window: usize,
        context_composition: ContextComposition,
        actual_input_tokens: u64,
        actual_output_tokens: u64,
        cache_read_tokens: u64,
        provider_telemetry: Option<ProviderTelemetrySnapshot>,
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
    RefreshHarnessStatus,
    /// Automatically store a fact in memory when a lifecycle event fires.
    /// The runtime routes this to the memory feature's store handler,
    /// bypassing the schema-disabled memory_ingest_lifecycle tool.
    AutoStoreFact {
        section: String,
        content: String,
        source: String,
    },
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEndReason {
    AssistantCompleted,
    ToolContinuation,
    CommitNudge,
    Cancelled,
}

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
    MessageAbort,
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
        name: String,
        result: ToolResult,
        is_error: bool,
    },
    TurnEnd {
        turn: u32,
        /// Why the loop ended or continued after this turn.
        turn_end_reason: TurnEndReason,
        /// Model that produced this turn's usage. Optional on legacy/early-exit paths.
        model: Option<String>,
        /// Provider that produced this turn's usage. Optional on legacy/early-exit paths.
        provider: Option<String>,
        /// Real token estimate from conversation history. Zero on early-exit paths.
        estimated_tokens: usize,
        /// Context window used for the turn.
        context_window: usize,
        /// Composition breakdown within Omegon's chars/4 accounting model.
        context_composition: ContextComposition,
        /// Actual input tokens reported by the provider. 0 = not available.
        actual_input_tokens: u64,
        /// Actual output tokens reported by the provider. 0 = not available.
        actual_output_tokens: u64,
        /// Cache-read tokens (Anthropic). 0 if not applicable.
        cache_read_tokens: u64,
        /// Cache-write / cache-creation tokens (Anthropic). 0 if not applicable.
        cache_creation_tokens: u64,
        /// Parsed provider quota/headroom telemetry from response headers or status endpoints.
        provider_telemetry: Option<ProviderTelemetrySnapshot>,
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
    /// Embedded web compatibility surface started — carries discovery metadata
    /// so the TUI can track the local compatibility server without scraping
    /// human-readable notifications.
    WebDashboardStarted {
        startup_json: Value,
    },
    /// Context updated — authoritative snapshot after compaction, clear, or turn completion.
    /// TUI + web consumers should use this as the canonical context status source.
    ContextUpdated {
        tokens: u64,
        context_window: u64,
        context_class: String,
        thinking_level: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_state_snapshot() -> IpcStateSnapshot {
        IpcStateSnapshot {
            schema_version: 1,
            omegon_version: "0.15.4-rc.20".into(),
            instance: OmegonInstanceDescriptor {
                schema_version: 1,
                identity: OmegonIdentity {
                    instance_id: "instance-1".into(),
                    workspace_id: "workspace-1".into(),
                    session_id: "session-1".into(),
                    role: OmegonRole::PrimaryDriver,
                    profile: "primary-interactive".into(),
                },
                ownership: OmegonOwnership {
                    owner_kind: OmegonOwnerKind::Operator,
                    owner_id: "local-terminal".into(),
                    parent_instance_id: None,
                },
                placement: OmegonPlacement {
                    kind: OmegonPlacementKind::LocalProcess,
                    host: Some("localhost".into()),
                    pid: Some(12345),
                    cwd: "/tmp/project".into(),
                    namespace: None,
                    pod_name: None,
                    container_name: None,
                },
                control_plane: OmegonControlPlane {
                    server_instance_id: "instance-1".into(),
                    protocol_version: 1,
                    schema_version: 1,
                    omegon_version: "0.15.4-rc.20".into(),
                    capabilities: vec!["state.snapshot".into(), "events.stream".into()],
                    ipc_socket_path: Some("/tmp/project/.omegon/ipc.sock".into()),
                    http_base: None,
                    startup_url: None,
                    state_url: None,
                    ws_url: None,
                    auth_mode: None,
                    auth_source: None,
                    http_transport_security: None,
                    ws_transport_security: None,
                },
                runtime: OmegonRuntime {
                    deployment_kind: OmegonDeploymentKind::InteractiveTui,
                    runtime_mode: OmegonRuntimeMode::Standalone,
                    runtime_profile: OmegonRuntimeProfile::PrimaryInteractive,
                    autonomy_mode: OmegonAutonomyMode::OperatorDriven,
                    health: OmegonRuntimeHealth::Ready,
                    provider_ok: true,
                    memory_ok: true,
                    cleave_available: false,
                    queued_events: 0,
                    transport_warnings: Vec::new(),
                    runtime_dir: Some("/tmp/project/.omegon/runtime".into()),
                    context_class: Some("Squad".into()),
                    thinking_level: Some("Medium".into()),
                    capability_tier: Some("victory".into()),
                },
            },
            session: IpcSessionSnapshot {
                cwd: "/tmp/project".into(),
                pid: 12345,
                started_at: "2026-03-29T00:00:00Z".into(),
                turns: 1,
                tool_calls: 3,
                compactions: 0,
                busy: false,
                git_branch: Some("main".into()),
                git_detached: false,
                session_id: None,
            },
            design_tree: IpcDesignTreeSnapshot {
                counts: IpcDesignCounts {
                    total: 10,
                    seed: 1,
                    exploring: 2,
                    resolved: 1,
                    decided: 3,
                    implementing: 1,
                    implemented: 2,
                    blocked: 0,
                    deferred: 0,
                    open_questions: 4,
                },
                focused: None,
                implementing: vec![],
                actionable: vec![],
                nodes: vec![],
            },
            openspec: IpcOpenSpecSnapshot {
                changes: vec![],
                total_tasks: 0,
                done_tasks: 0,
            },
            cleave: IpcCleaveSnapshot {
                active: false,
                total_children: 0,
                completed: 0,
                failed: 0,
                children: vec![],
            },
            harness: IpcHarnessSnapshot {
                context_class: "Squad".into(),
                thinking_level: "Medium".into(),
                capability_tier: "victory".into(),
                runtime_profile: "primary-interactive".into(),
                autonomy_mode: "operator-driven".into(),
                memory_available: true,
                cleave_available: false,
                memory_warning: None,
                memory: IpcMemorySnapshot {
                    active_facts: 42,
                    project_facts: 20,
                    working_facts: 5,
                    episodes: 3,
                },
                providers: vec![IpcProviderSnapshot {
                    name: "Anthropic".into(),
                    authenticated: true,
                    model: Some("claude-sonnet-4-6".into()),
                    runtime_status: None,
                    recent_failure_count: None,
                    last_failure_kind: None,
                }],
                mcp_server_count: 0,
                mcp_tool_count: 0,
                active_persona: None,
                active_tone: None,
                active_delegate_count: 0,
            },
            health: IpcHealthSnapshot {
                state: IpcHealthState::Ready,
                memory_ok: true,
                provider_ok: true,
                checked_at: "2026-03-29T00:00:00Z".into(),
            },
        }
    }

    #[test]
    fn ipc_envelope_msgpack_roundtrip_preserves_contract_shape() {
        let env = IpcEnvelope {
            protocol_version: IPC_PROTOCOL_VERSION,
            kind: IpcEnvelopeKind::Request,
            request_id: Some(*b"1234567890abcdef"),
            method: Some("get_state".into()),
            payload: Some(json!({"verbose": false})),
            error: None,
        };

        let raw = env.encode_msgpack().unwrap();
        let decoded = IpcEnvelope::decode_msgpack(&raw).unwrap();
        assert_eq!(decoded, env);
    }

    #[test]
    fn error_envelope_carries_typed_code() {
        let env = IpcEnvelope::error_response(
            Some(*b"1234567890abcdef"),
            IpcErrorCode::UnknownMethod,
            "no such method",
        );
        let raw = env.encode_msgpack().unwrap();
        let decoded = IpcEnvelope::decode_msgpack(&raw).unwrap();
        assert_eq!(decoded.kind, IpcEnvelopeKind::Error);
        assert_eq!(decoded.error.unwrap().code, IpcErrorCode::UnknownMethod);
    }

    #[test]
    fn hello_request_msgpack_roundtrip() {
        let req = HelloRequest {
            client_name: "auspex".into(),
            client_version: "0.1.0".into(),
            supported_protocol_versions: vec![1],
            capabilities: vec![
                IpcCapability::StateSnapshot.as_str().into(),
                IpcCapability::EventsStream.as_str().into(),
            ],
        };
        let raw = rmp_serde::to_vec_named(&req).unwrap();
        let decoded: HelloRequest = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn hello_response_carries_instance_identity() {
        let resp = HelloResponse {
            protocol_version: 1,
            omegon_version: "0.15.4-rc.20".into(),
            server_name: "omegon".into(),
            server_pid: 99,
            cwd: "/project".into(),
            server_instance_id: "abc123".into(),
            started_at: "2026-03-29T00:00:00Z".into(),
            session_id: None,
            capabilities: IpcCapability::v1_server_set()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        };
        let raw = rmp_serde::to_vec_named(&resp).unwrap();
        let decoded: HelloResponse = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded.server_instance_id, "abc123");
        assert_eq!(decoded.started_at, "2026-03-29T00:00:00Z");
        assert!(decoded.capabilities.contains(&"state.snapshot".to_string()));
        assert!(decoded.capabilities.contains(&"shutdown".to_string()));
    }

    #[test]
    fn v1_capability_set_is_complete() {
        let set = IpcCapability::v1_server_set();
        assert!(set.contains(&"state.snapshot"));
        assert!(set.contains(&"events.stream"));
        assert!(set.contains(&"prompt.submit"));
        assert!(set.contains(&"turn.cancel"));
        assert!(set.contains(&"graph.read"));
        assert!(set.contains(&"context.view"));
        assert!(set.contains(&"context.compact"));
        assert!(set.contains(&"context.clear"));
        assert!(set.contains(&"session.new"));
        assert!(set.contains(&"session.list"));
        assert!(set.contains(&"auth.status"));
        assert!(set.contains(&"model.view"));
        assert!(set.contains(&"model.list"));
        assert!(set.contains(&"model.set"));
        assert!(set.contains(&"thinking.set"));
        assert!(set.contains(&"slash_commands"));
        assert!(set.contains(&"shutdown"));
    }

    #[test]
    fn ipc_state_snapshot_all_sections_present() {
        let snap = sample_state_snapshot();
        let v = serde_json::to_value(&snap).unwrap();
        for field in &[
            "schema_version",
            "omegon_version",
            "session",
            "design_tree",
            "openspec",
            "cleave",
            "harness",
            "health",
        ] {
            assert!(v.get(field).is_some(), "missing field: {field}");
        }
        // Check session fields
        assert!(v["session"].get("cwd").is_some());
        assert!(v["session"].get("pid").is_some());
        assert!(v["session"].get("started_at").is_some());
        assert!(v["session"].get("busy").is_some());
        assert!(v["session"].get("git_branch").is_some());
        // Check harness fields
        assert!(v["harness"].get("context_class").is_some());
        assert!(v["harness"].get("memory").is_some());
        assert!(v["harness"].get("providers").is_some());
        // Check health fields
        assert!(v["health"].get("state").is_some());
        assert!(v["health"].get("memory_ok").is_some());
        assert!(v["health"].get("provider_ok").is_some());
    }

    #[test]
    fn ipc_state_snapshot_msgpack_roundtrip() {
        let snap = sample_state_snapshot();
        let raw = rmp_serde::to_vec_named(&snap).unwrap();
        let decoded: IpcStateSnapshot = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded.omegon_version, snap.omegon_version);
        assert_eq!(decoded.session.turns, 1);
        assert_eq!(decoded.harness.memory.active_facts, 42);
        assert_eq!(decoded.health.state, IpcHealthState::Ready);
    }

    #[test]
    fn ipc_event_payload_turn_started_roundtrip() {
        let ev = IpcEventPayload::TurnStarted { turn: 7 };
        let raw = rmp_serde::to_vec_named(&ev).unwrap();
        let decoded: IpcEventPayload = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, ev);
    }

    #[test]
    fn ipc_event_payload_tool_ended_roundtrip() {
        let ev = IpcEventPayload::ToolEnded {
            id: "call-1".into(),
            name: "bash".into(),
            is_error: false,
            summary: Some("exit 0".into()),
        };
        let raw = rmp_serde::to_vec_named(&ev).unwrap();
        let decoded: IpcEventPayload = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, ev);
    }

    #[test]
    fn ipc_event_payload_state_changed_carries_sections() {
        let ev = IpcEventPayload::StateChanged {
            sections: vec!["harness".into(), "session".into()],
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["name"], "state.changed");
        // With adjacent tagging, payload lives under "data" key.
        let secs = v["data"]["sections"].as_array().unwrap();
        assert_eq!(secs.len(), 2);
    }

    #[test]
    fn daemon_event_envelope_roundtrip() {
        let ev = DaemonEventEnvelope {
            event_id: "evt-1".into(),
            source: "webhook/github".into(),
            trigger_kind: "webhook".into(),
            payload: serde_json::json!({"ref": "refs/heads/main"}),
            caller_role: None,
        };
        let raw = rmp_serde::to_vec_named(&ev).unwrap();
        let decoded: DaemonEventEnvelope = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, ev);
    }

    #[test]
    fn transport_security_serializes_kebab_case() {
        let v = serde_json::to_value(OmegonTransportSecurity::InsecureBootstrap).unwrap();
        assert_eq!(v, serde_json::Value::String("insecure-bootstrap".into()));
    }

    #[test]
    fn runtime_mode_serializes_snake_case() {
        let v = serde_json::to_value(OmegonRuntimeMode::AuspexManaged).unwrap();
        assert_eq!(v, serde_json::Value::String("auspex_managed".into()));
    }

    #[test]
    fn slash_command_request_roundtrip() {
        let req = SlashCommandRequest {
            name: "compact".into(),
            args: "200k".into(),
            caller_role: None,
        };
        let raw = rmp_serde::to_vec_named(&req).unwrap();
        let decoded: SlashCommandRequest = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn control_request_roundtrip() {
        let req = ControlRequest {
            caller_role: Some("edit".into()),
        };
        let raw = rmp_serde::to_vec_named(&req).unwrap();
        let decoded: ControlRequest = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn control_output_response_roundtrip() {
        let resp = ControlOutputResponse {
            accepted: true,
            output: Some("ok".into()),
        };
        let raw = rmp_serde::to_vec_named(&resp).unwrap();
        let decoded: ControlOutputResponse = rmp_serde::from_slice(&raw).unwrap();
        assert_eq!(decoded, resp);
    }

    #[test]
    fn max_frame_constant_is_8mib() {
        assert_eq!(IPC_MAX_FRAME_BYTES, 8 * 1024 * 1024);
    }
}
