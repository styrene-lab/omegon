//! Runtime settings — mutable configuration shared between TUI and agent loop.
//!
//! This replaces pi's `sharedState` global. All runtime-mutable values live here.
//! The TUI reads for display. Commands write via the shared Arc<Mutex>.
//! The agent loop reads before each turn.
//!
//! Settings persist for the session. Serialized to session snapshot on save.
//!
//! ## Three-axis routing model
//!
//! - **Model capability grade**: F / D / C / B / A / S; local is provider selection
//! - **Thinking level**: off / minimal / low / medium / high
//! - **Context class**: Compact (128k) / Standard (272k) / Extended (400k) / Massive (1M+)

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Posture preset — behavioral stance for the harness.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PosturePreset {
    /// Cheap-first reconnaissance and local hypothesis testing.
    Explorator,
    /// Balanced implementation posture.
    Fabricator,
    /// Systems-engineering posture.
    #[default]
    Architect,
    /// Maximum-force posture.
    Devastator,
}

impl PosturePreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Explorator => "explorator",
            Self::Fabricator => "fabricator",
            Self::Architect => "architect",
            Self::Devastator => "devastator",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "explorator" => Some(Self::Explorator),
            "fabricator" => Some(Self::Fabricator),
            "architect" => Some(Self::Architect),
            "devastator" => Some(Self::Devastator),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Explorator => "Explorator",
            Self::Fabricator => "Fabricator",
            Self::Architect => "Architect",
            Self::Devastator => "Devastator",
        }
    }

    /// First-pass resource defaults for the posture.
    ///
    /// This intentionally covers only the axes already modeled in `settings.rs`.
    /// Model-grade intent handling remains in `model_budget.rs` for now.
    pub fn default_resource_envelope(self) -> ResourceEnvelope {
        match self {
            Self::Explorator => ResourceEnvelope {
                thinking: ThinkingLevel::Minimal,
                requested_context_class: ContextClass::Compact,
                effective_context_cap_tokens: Some(ContextClass::Compact.nominal_tokens()),
                compact_reply_reserve: true,
                compact_tool_schema_reserve: true,
            },
            Self::Fabricator => ResourceEnvelope {
                thinking: ThinkingLevel::Low,
                requested_context_class: ContextClass::Standard,
                effective_context_cap_tokens: None,
                compact_reply_reserve: false,
                compact_tool_schema_reserve: false,
            },
            Self::Architect => ResourceEnvelope {
                thinking: ThinkingLevel::Medium,
                requested_context_class: ContextClass::Extended,
                effective_context_cap_tokens: None,
                compact_reply_reserve: false,
                compact_tool_schema_reserve: false,
            },
            Self::Devastator => ResourceEnvelope {
                thinking: ThinkingLevel::High,
                requested_context_class: ContextClass::Massive,
                effective_context_cap_tokens: None,
                compact_reply_reserve: false,
                compact_tool_schema_reserve: false,
            },
        }
    }
}

/// Posture mode — fixed or adaptive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum PostureMode {
    Fixed { preset: PosturePreset },
    Adaptive { baseline: PosturePreset },
}

impl Default for PostureMode {
    fn default() -> Self {
        Self::Fixed {
            preset: PosturePreset::Architect,
        }
    }
}

/// Effective behavioral posture state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehavioralPosture {
    pub mode: PostureMode,
    pub effective: PosturePreset,
}

impl BehavioralPosture {
    pub fn fixed(preset: PosturePreset) -> Self {
        Self {
            mode: PostureMode::Fixed { preset },
            effective: preset,
        }
    }

    pub fn adaptive(baseline: PosturePreset) -> Self {
        Self {
            mode: PostureMode::Adaptive { baseline },
            effective: baseline,
        }
    }
}

/// First-pass execution envelope derived from posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEnvelope {
    pub thinking: ThinkingLevel,
    pub requested_context_class: ContextClass,
    pub effective_context_cap_tokens: Option<usize>,
    pub compact_reply_reserve: bool,
    pub compact_tool_schema_reserve: bool,
}

/// Placeholder runtime identity shape.
///
/// This remains intentionally skeletal until Styrene Identity and workload/session
/// identity are wired through the harness.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIdentity {
    pub principal_id: Option<String>,
    pub issuer: Option<String>,
    pub session_kind: Option<String>,
}

impl RuntimeIdentity {
    /// Descriptive local interactive identity.
    ///
    /// This is intentionally metadata-only. It is not an authorization grant.
    pub fn local_interactive() -> Self {
        Self {
            principal_id: Some("local-operator".into()),
            issuer: Some("local-session".into()),
            session_kind: Some("interactive".into()),
        }
    }

    /// Descriptive local control-plane identity.
    ///
    /// This exists so daemon/control-plane surfaces can describe themselves
    /// before Styrene Identity is wired.
    pub fn local_control_plane() -> Self {
        Self {
            principal_id: Some("daemon-supervisor".into()),
            issuer: Some("local-daemon".into()),
            session_kind: Some("control-plane".into()),
        }
    }

    pub fn summary_principal(&self) -> &str {
        self.principal_id.as_deref().unwrap_or("anonymous")
    }
}

/// Placeholder authorization shape.
///
/// Capability and role semantics will move here once RBAC is threaded into the
/// runtime profile model.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationContext {
    pub roles: Vec<String>,
    pub capabilities: Vec<String>,
    pub trust_domain: Option<String>,
}

impl AuthorizationContext {
    /// Descriptive local authorization context.
    ///
    /// This is intentionally observational only. It does not imply enforced RBAC.
    pub fn local_descriptive() -> Self {
        Self {
            roles: vec!["operator".into()],
            capabilities: Vec::new(),
            trust_domain: Some("local".into()),
        }
    }

    pub fn summary(&self) -> String {
        let role = self.roles.first().map(String::as_str).unwrap_or("unscoped");
        let domain = self.trust_domain.as_deref().unwrap_or("unknown");
        if self.capabilities.is_empty() {
            format!("{role}@{domain}")
        } else {
            format!("{role}@{domain} +{}cap", self.capabilities.len())
        }
    }
}

/// Current persona/mind state.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaState {
    pub persona_id: Option<String>,
    pub mind_id: Option<String>,
}

impl PersonaState {
    pub fn from_ids(persona_id: Option<String>, mind_id: Option<String>) -> Self {
        Self {
            persona_id,
            mind_id,
        }
    }
}

/// Composed runtime operating profile.
///
/// This is the bridge between the conceptual stack and implementation. The
/// trust layers are placeholders for now; posture and resource envelope are the
/// first active runtime layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatingProfile {
    pub identity: RuntimeIdentity,
    pub authorization: AuthorizationContext,
    pub persona: PersonaState,
    pub posture: BehavioralPosture,
    pub resources: ResourceEnvelope,
}

impl OperatingProfile {
    pub fn with_identity(mut self, identity: RuntimeIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub fn with_persona(mut self, persona: PersonaState) -> Self {
        self.persona = persona;
        self
    }

    pub fn summary(&self) -> String {
        let persona_or_identity = self
            .persona
            .persona_id
            .as_deref()
            .unwrap_or_else(|| self.identity.summary_principal());
        format!(
            "{} / {} / {} / {} / {}",
            persona_or_identity,
            self.posture.effective.display_name(),
            self.resources.thinking.as_str(),
            self.resources.requested_context_class.short(),
            self.authorization.summary()
        )
    }
}

/// Runtime settings that can change mid-session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Active model (provider:model-id format).
    pub model: String,

    /// Human-readable name of the active persisted profile, when provided.
    #[serde(default, skip)]
    pub profile_name: Option<String>,

    /// Behavioral posture for the current session/runtime.
    #[serde(default)]
    pub posture: BehavioralPosture,

    /// Tools disabled by the active custom posture. Applied by the EventBus
    /// alongside slim-mode tool filtering. Empty for built-in postures.
    #[serde(default, skip)]
    pub posture_disabled_tools: Vec<String>,

    /// Tools enabled by the active custom posture (whitelist mode).
    /// When non-empty, only these tools are available — everything else is disabled.
    /// Empty means "no whitelist, use disabled list instead."
    #[serde(default, skip)]
    pub posture_enabled_tools: Vec<String>,

    /// Thinking level: off, minimal, low, medium, high.
    pub thinking: ThinkingLevel,

    /// Maximum turns per agent invocation. 0 = no limit.
    pub max_turns: u32,

    /// Operator-tuned continuation policy. This controls whether the agent
    /// keeps driving after text-only "should I proceed?" style turns; it never
    /// bypasses permission, plan, or security gates.
    #[serde(default)]
    pub automation_level: AutomationLevel,

    /// Context compaction threshold (fraction of context window).
    pub compaction_threshold: f32,

    /// Context window size (tokens). Inferred from model via route matrix.
    pub context_window: usize,

    /// Context class — authoritative model-capacity class.
    /// Updated by `set_model()` and provider probes. Do NOT set directly from commands.
    pub context_class: ContextClass,

    /// Operator's requested working-set policy class.
    /// `None` means "track model capacity" (default until operator explicitly changes).
    /// Set by `/context <class>` — does NOT affect the actual model window.
    #[serde(default)]
    pub requested_context_class: Option<ContextClass>,

    /// Tool display detail level.
    pub tool_detail: ToolDetail,

    /// Preferred UI presentation level. This is a client projection preference;
    /// it never changes runtime posture, permissions, or tool policy.
    #[serde(default)]
    pub ui_presentation: crate::surfaces::layout::UiPresentationLevel,

    /// Source of the active persisted profile loaded for this runtime.
    #[serde(skip)]
    pub profile_source: ProfileSource,

    /// Provider preference order for routing. First = most preferred.
    #[serde(default)]
    pub provider_order: Vec<String>,

    /// Explicit fallback providers for the interactive route controller.
    /// Empty by default: missing selected-provider credentials fail explicitly
    /// instead of silently substituting another provider.
    #[serde(default)]
    pub fallback_providers: Vec<String>,

    /// Update channel for in-app self-update.
    #[serde(default = "default_update_channel")]
    pub update_channel: String,

    /// Automatic update between sessions. When true, downloads and replaces
    /// the binary after session end if a newer version is available on the
    /// configured channel. Default: false (notification only).
    #[serde(default)]
    pub auto_update: bool,

    /// Directories outside the workspace that the agent is allowed to
    /// read/write without per-operation confirmation. Paths are expanded
    /// at check time (~ → $HOME). Persists across sessions.
    #[serde(default)]
    pub trusted_directories: Vec<String>,

    /// Unified permission policy for this runtime.
    #[serde(default)]
    pub permissions: ProfilePermissions,

    /// Whether a live LLM provider is connected. Set to false when NullBridge
    /// is active (no credentials available). The TUI uses this to show
    /// "no provider" instead of a model name that can't actually be used.
    #[serde(skip)]
    pub provider_connected: bool,

    /// Whether the active provider credential path is OAuth-backed. Updated
    /// only when the model changes so rendering does not probe credential files
    /// on every frame.
    #[serde(skip)]
    pub provider_is_oauth: bool,

    /// Sandbox isolation — when true, delegate/cleave children run inside
    /// OCI containers (podman/docker) with resource limits and network isolation.
    #[serde(default)]
    pub sandbox: bool,

    /// Enable the interactive PTY-backed terminal tool. This is useful for
    /// local/debuggable agents and should usually be disabled for hardened
    /// headless OCI profiles that lack `/dev/pts` or writable config storage.
    #[serde(default = "default_terminal_tool")]
    pub terminal_tool: bool,

    /// How long clipboard pastes are retained on disk before automatic
    /// deletion at session start, in hours. Default 24h. Set to 0 to
    /// disable automatic deletion entirely. The setting also feeds the
    /// `/clipboard prune` slash command for on-demand sweeps.
    ///
    /// Clipboard pastes are written to the system temp directory by
    /// `tui::mod::pull_clipboard_image` and named
    /// `omegon-clipboard-{pid}-{counter}.{ext}`. The matching prune
    /// logic in `clipboard::prune_old_pastes` walks that directory at
    /// session start and removes anything matching that pattern whose
    /// modification time is older than `clipboard_retention_hours`.
    #[serde(default = "default_clipboard_retention_hours")]
    pub clipboard_retention_hours: u64,
}

fn default_clipboard_retention_hours() -> u64 {
    24
}

/// Tool card information density in the conversation view.
///
/// Controls how much tool call output is shown by default. Interactive
/// expand (Ctrl+O) overrides this for individual cards.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolDetail {
    /// One-liner per tool: `▸ read src/main.rs → ok`. No args, no output.
    /// Claude Code style. Ctrl+O to expand individual cards.
    Lean,
    /// 2-3 lines: name + summary arg + short result preview.
    Compact,
    /// Current default: 4 lines args, 12 lines results, diffs shown.
    #[default]
    Detailed,
    /// Full output: 50 lines args, 200 lines results. For debugging.
    Verbose,
}

impl ToolDetail {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lean => "lean",
            Self::Compact => "compact",
            Self::Detailed => "detailed",
            Self::Verbose => "verbose",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "lean" | "l" | "tree" | "minimal" => Some(Self::Lean),
            "compact" | "c" => Some(Self::Compact),
            "detailed" | "detail" | "d" => Some(Self::Detailed),
            "verbose" | "v" | "full" | "debug" => Some(Self::Verbose),
            _ => None,
        }
    }

    /// Max lines of tool args to display.
    pub fn args_budget(&self) -> usize {
        match self {
            Self::Lean => 0,
            Self::Compact => 1,
            Self::Detailed => 4,
            Self::Verbose => 50,
        }
    }

    /// Max lines of tool result to display.
    pub fn result_budget(&self) -> usize {
        match self {
            Self::Lean => 0,
            Self::Compact => 3,
            Self::Detailed => 12,
            Self::Verbose => 200,
        }
    }

    /// Max lines of diff content to display.
    pub fn diff_budget(&self) -> usize {
        match self {
            Self::Lean => 0,
            Self::Compact => 3,
            Self::Detailed => 8,
            Self::Verbose => 200,
        }
    }

    /// Max lines of live tail output.
    pub fn tail_budget(&self) -> usize {
        match self {
            Self::Lean => 3,
            Self::Compact => 6,
            Self::Detailed => 12,
            Self::Verbose => 50,
        }
    }

    /// Whether to show the args summary line.
    pub fn show_summary(&self) -> bool {
        true // always show — it's the minimal one-liner
    }

    /// Next density level (for cycling with /density toggle).
    pub fn next(&self) -> Self {
        match self {
            Self::Lean => Self::Compact,
            Self::Compact => Self::Detailed,
            Self::Detailed => Self::Verbose,
            Self::Verbose => Self::Lean,
        }
    }
}

// ─── Context Class ──────────────────────────────────────────────────────────

/// Context class — named context window categories.
///
/// Abstracts provider-specific token ceilings into operator-friendly categories.
/// Internal routing still compares exact token counts; these are the policy
/// and UX abstraction.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "PascalCase")]
pub enum ContextClass {
    /// 128k tokens. Compact context for lightweight tasks.
    #[default]
    #[serde(alias = "Squad")]
    Compact,
    /// 272k tokens. Standard working context.
    #[serde(alias = "Maniple")]
    Standard,
    /// 400k tokens. Extended context for large codebases.
    #[serde(alias = "Clan")]
    Extended,
    /// 1M+ tokens. Massive context for very large sessions.
    #[serde(alias = "Legion")]
    Massive,
}

/// Token ceiling thresholds — a model with ceiling ≤ threshold belongs to that class.
const CONTEXT_CLASS_THRESHOLDS: &[(ContextClass, usize)] = &[
    (ContextClass::Compact, 131_072),  // 128k
    (ContextClass::Standard, 278_528), // ~272k
    (ContextClass::Extended, 450_560), // ~440k (covers 400k models)
                                       // Massive: everything above
];

impl ContextClass {
    /// Classify a raw token count into a context class.
    pub fn from_tokens(tokens: usize) -> Self {
        for &(class, threshold) in CONTEXT_CLASS_THRESHOLDS {
            if tokens <= threshold {
                return class;
            }
        }
        Self::Massive
    }

    /// Nominal token count for this class.
    pub fn nominal_tokens(self) -> usize {
        match self {
            Self::Compact => 131_072,
            Self::Standard => 278_528,
            Self::Extended => 409_600,
            Self::Massive => 1_048_576,
        }
    }

    /// Operator-facing display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact (128k)",
            Self::Standard => "Standard (272k)",
            Self::Extended => "Extended (400k)",
            Self::Massive => "Massive (1M+)",
        }
    }

    /// Short name for dashboard badges.
    pub fn short(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Standard => "Standard",
            Self::Extended => "Extended",
            Self::Massive => "Massive",
        }
    }

    /// Ordinal for comparison and delta calculation.
    pub fn ordinal(self) -> u8 {
        match self {
            Self::Compact => 0,
            Self::Standard => 1,
            Self::Extended => 2,
            Self::Massive => 3,
        }
    }

    /// Delta between two classes. Positive = downgrade (self > other).
    pub fn delta(self, other: Self) -> i8 {
        self.ordinal() as i8 - other.ordinal() as i8
    }

    /// All classes in ascending order.
    pub fn all() -> &'static [Self] {
        &[Self::Compact, Self::Standard, Self::Extended, Self::Massive]
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "compact" | "squad" | "128k" => Some(Self::Compact),
            "standard" | "maniple" | "272k" => Some(Self::Standard),
            "extended" | "clan" | "400k" => Some(Self::Extended),
            "massive" | "legion" | "1m" => Some(Self::Massive),
            _ => None,
        }
    }
}

fn default_update_channel() -> String {
    "stable".to_string()
}

fn default_terminal_tool() -> bool {
    true
}

// ─── Selector Policy ─────────────────────────────────────────────────────────

/// Derived per-turn context assembly policy.
/// Computed from Settings on each turn — separates operator intent from model truth.
#[derive(Debug, Clone, Copy)]
pub struct SelectorPolicy {
    /// Hard model ceiling — from provider probe or route matrix.
    pub model_window: usize,
    /// Operator's working-set breadth request. May exceed model_window.
    pub requested_class: ContextClass,
    /// Tokens reserved for model reply + thinking budget.
    pub reply_reserve: usize,
    /// Tokens reserved for tool schema overhead.
    pub tool_schema_reserve: usize,
}

impl SelectorPolicy {
    /// Effective window used for local context assembly.
    ///
    /// `model_window` is the upstream/provider capacity. `requested_class` is
    /// the operator's desired working-set breadth. A smaller requested class
    /// intentionally constrains local assembly even when the model can accept
    /// more tokens; a larger requested class cannot exceed provider capacity.
    pub fn assembly_window(&self) -> usize {
        self.model_window.min(self.requested_class.nominal_tokens())
    }

    /// Actual token budget available for context assembly this turn.
    pub fn assembly_budget(&self) -> usize {
        self.assembly_window()
            .saturating_sub(self.reply_reserve)
            .saturating_sub(self.tool_schema_reserve)
    }

    /// Whether the operator requested more capacity than the model supports.
    pub fn has_class_mismatch(&self) -> bool {
        self.requested_class > ContextClass::from_tokens(self.model_window)
    }

    /// Actual class derived from model_window.
    pub fn actual_class(&self) -> ContextClass {
        ContextClass::from_tokens(self.model_window)
    }
}

impl Default for Settings {
    fn default() -> Self {
        let context_window = 200_000;
        Self {
            model: "anthropic:claude-sonnet-4-6".into(),
            profile_name: None,
            posture: BehavioralPosture::fixed(PosturePreset::Architect),
            thinking: ThinkingLevel::Medium,
            max_turns: 50,
            automation_level: AutomationLevel::default(),
            compaction_threshold: 0.75,
            context_window,
            context_class: ContextClass::from_tokens(context_window),
            requested_context_class: None,
            tool_detail: ToolDetail::Detailed,
            ui_presentation: crate::surfaces::layout::UiPresentationLevel::Om,
            profile_source: ProfileSource::BuiltInDefault,
            provider_order: Vec::new(),
            fallback_providers: Vec::new(),
            update_channel: default_update_channel(),
            auto_update: false,
            trusted_directories: Vec::new(),
            permissions: ProfilePermissions::default(),
            provider_connected: true, // optimistic default — set false when NullBridge
            provider_is_oauth: false,
            sandbox: false,
            terminal_tool: true,
            clipboard_retention_hours: default_clipboard_retention_hours(),
            posture_disabled_tools: Vec::new(),
            posture_enabled_tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutomationLevel {
    Ask,
    #[default]
    Guarded,
    Flow,
    Autonomous,
}

impl AutomationLevel {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ask" | "manual" | "confirm" => Some(Self::Ask),
            "guarded" | "default" => Some(Self::Guarded),
            "flow" | "proceed" => Some(Self::Flow),
            "autonomous" | "auto" | "run" => Some(Self::Autonomous),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Guarded => "guarded",
            Self::Flow => "flow",
            Self::Autonomous => "autonomous",
        }
    }

    pub fn summary(&self) -> &'static str {
        match self {
            Self::Ask => "ask before continuation",
            Self::Guarded => "continue through low-risk stalls",
            Self::Flow => "continue until task completion",
            Self::Autonomous => "run to completion within hard gates",
        }
    }
}

impl Settings {
    pub fn new(model: &str) -> Self {
        let context_window = infer_context_window(model);
        let context_class = ContextClass::from_tokens(context_window);
        Self {
            model: model.to_string(),
            context_window,
            context_class,
            provider_is_oauth: crate::auth::provider_oauth_for_model(model),
            ..Default::default()
        }
    }

    /// Update model and recalculate derived fields.
    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
        self.context_window = infer_context_window(model);
        self.context_class = ContextClass::from_tokens(self.context_window);
        self.provider_is_oauth = crate::auth::provider_oauth_for_model(model);
    }

    /// The effective working-set policy class for this turn.
    /// Returns the operator's explicit request if set, otherwise the model-derived class.
    pub fn effective_requested_class(&self) -> ContextClass {
        self.requested_context_class.unwrap_or(self.context_class)
    }

    /// Set the operator's requested working-set policy class.
    /// Does NOT change `context_window` or `context_class` — those are model-derived.
    pub fn set_requested_context_class(&mut self, class: ContextClass) {
        self.requested_context_class = Some(class);
    }

    /// Resource envelope currently implied by posture.
    pub fn resource_envelope(&self) -> ResourceEnvelope {
        self.posture.effective.default_resource_envelope()
    }

    /// Composed operating profile for the current runtime state.
    pub fn operating_profile(&self) -> OperatingProfile {
        OperatingProfile {
            identity: RuntimeIdentity::local_interactive(),
            authorization: AuthorizationContext::local_descriptive(),
            persona: PersonaState::default(),
            posture: self.posture,
            resources: self.resource_envelope(),
        }
    }

    pub fn set_posture(&mut self, preset: PosturePreset) {
        self.posture = BehavioralPosture::fixed(preset);

        let envelope = self.resource_envelope();
        self.thinking = envelope.thinking;
        self.requested_context_class = Some(envelope.requested_context_class);
    }

    /// Whether the runtime is in slim mode (Explorator posture).
    pub fn is_slim(&self) -> bool {
        matches!(self.posture.effective, PosturePreset::Explorator)
    }

    /// Derive a SelectorPolicy for this turn's context assembly.
    pub fn selector_policy(&self) -> SelectorPolicy {
        let envelope = self.resource_envelope();
        let thinking_reserve = self.thinking.budget_tokens().unwrap_or(0) as usize;
        let model_window = envelope
            .effective_context_cap_tokens
            .map(|cap| self.context_window.min(cap))
            .unwrap_or(self.context_window);
        let requested_class = if self.is_slim() {
            envelope.requested_context_class
        } else {
            self.requested_context_class
                .unwrap_or(envelope.requested_context_class)
        };
        let reply_reserve = if envelope.compact_reply_reserve {
            4_096 + thinking_reserve
        } else {
            8_192 + thinking_reserve
        };
        let tool_schema_reserve = if envelope.compact_tool_schema_reserve {
            2_048
        } else {
            4_096
        };
        SelectorPolicy {
            model_window,
            requested_class,
            reply_reserve,
            tool_schema_reserve,
        }
    }

    /// Returns the human-readable short name for the model.
    ///
    /// Strips the provider prefix (e.g. `anthropic:`, `ollama:`) and the
    /// Ollama `:latest` tag so the label reads "glm-4.7-flash" not "latest".
    pub fn model_short(&self) -> String {
        humanize_model_id(&self.model)
    }

    pub fn provider(&self) -> String {
        crate::providers::infer_provider_id(&self.model)
    }
}

/// Thinking level — controls extended thinking budget.
///
/// Display names use the Mechanicum cognition ladder:
/// Off → Servitor, Minimal → Functionary, Low → Adept, Medium → Magos, High → Archmagos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
}

impl ThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Operator-facing display name (Mechanicum cognition ladder).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Off => "Servitor",
            Self::Minimal => "Functionary",
            Self::Low => "Adept",
            Self::Medium => "Magos",
            Self::High => "Archmagos",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off" | "none" | "servitor" => Some(Self::Off),
            "minimal" | "functionary" => Some(Self::Minimal),
            "low" | "min" | "adept" => Some(Self::Low),
            "medium" | "med" | "default" | "magos" => Some(Self::Medium),
            "high" | "max" | "archmagos" => Some(Self::High),
            _ => None,
        }
    }

    /// Heuristic reserve for local context planning, not a provider contract.
    ///
    /// This value is used by selector/context budgeting (`selector_policy`) to
    /// leave room for deeper reasoning turns. It is intentionally approximate:
    /// provider-native request knobs differ substantially (Anthropic adaptive
    /// thinking vs manual budgets, OpenAI reasoning effort enums, Ollama `think`).
    /// Do not treat this as the exact upstream budget sent on the wire.
    pub fn budget_tokens(&self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Minimal => Some(2_000),
            Self::Low => Some(5_000),
            Self::Medium => Some(10_000),
            Self::High => Some(50_000),
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Off => "○",
            Self::Minimal => "◔",
            Self::Low => "◔",
            Self::Medium => "◑",
            Self::High => "◉",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Off,
            Self::Minimal,
            Self::Low,
            Self::Medium,
            Self::High,
        ]
    }
}

// ─── Route matrix (compile-time embedded) ───────────────────────────────────

/// Match a model ID against the model registry route patterns.
fn lookup_context_ceiling(provider: &str, model_id: &str) -> Option<usize> {
    crate::model_registry::ModelRegistry::global().context_ceiling(provider, model_id)
}

/// Infer context window from model identifier.
/// Uses the embedded route matrix first, falls back to heuristics.
/// Known provider prefixes used to strip the leading `provider:` segment
/// from model spec strings.
const PROVIDER_PREFIXES: &[&str] = &[
    "anthropic",
    "openai",
    "openai-codex",
    "groq",
    "xai",
    "mistral",
    "cerebras",
    "moonshot",
    "google",
    "google-antigravity",
    "huggingface",
    "openrouter",
    "ollama",
    "ollama-cloud",
    "local",
    "codex",
];

/// Convert a full model spec (e.g. `ollama:glm-4.7-flash:latest`) to a
/// short human-readable label (e.g. `glm-4.7-flash`).
///
/// Rules applied in order:
/// 1. Strip a leading `provider:` prefix if the segment is a known provider.
/// 2. Strip a trailing `:latest` Ollama tag.
/// 3. For HuggingFace-style `org/repo` paths, take the last path segment.
pub(crate) fn humanize_model_id(model_spec: &str) -> String {
    // 1. Strip provider prefix
    let without_provider = if let Some(colon) = model_spec.find(':') {
        let prefix = &model_spec[..colon];
        if PROVIDER_PREFIXES.contains(&prefix) {
            &model_spec[colon + 1..]
        } else {
            model_spec
        }
    } else {
        model_spec
    };

    // 2. Strip trailing :latest
    let without_latest = without_provider
        .strip_suffix(":latest")
        .unwrap_or(without_provider);

    // 3. Take last path segment for HuggingFace-style org/repo names
    without_latest
        .split('/')
        .next_back()
        .unwrap_or(without_latest)
        .to_string()
}

fn infer_context_window(model: &str) -> usize {
    let parts: Vec<&str> = model.splitn(2, ':').collect();
    let (provider, model_id) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("anthropic", model)
    };

    // Exact registry entries are the primary static provider constraint.
    let qualified = format!("{provider}:{model_id}");
    if let Some(entry) = crate::model_registry::ModelRegistry::global().model_info(&qualified) {
        return entry.context_input;
    }

    // Route patterns cover versioned aliases and dynamically discovered models.
    if let Some(ceiling) = lookup_context_ceiling(provider, model_id) {
        return ceiling;
    }

    // Fallback heuristics for models not in the registry or route matrix.
    let name = model_id;
    if name.contains("opus") || name.contains("sonnet") {
        return 200_000;
    }
    if name.contains("haiku") {
        return 200_000;
    }
    if name.contains("gpt-5") {
        return 272_000;
    }
    if name.contains("gpt-4.1") {
        return 200_000;
    }

    // Ollama models: default to 32k — matches the num_ctx we inject in
    // OpenAICompatClient. Using 131k here would cause the harness to keep
    // sending more tokens than the model's KV cache can hold.
    if provider == "ollama" || provider == "local" {
        return 32_768;
    }

    131_072 // fail-closed: default to Compact for unknown cloud providers
}

/// Thread-safe shared settings handle.
pub type SharedSettings = Arc<Mutex<Settings>>;

pub fn shared(model: &str) -> SharedSettings {
    Arc::new(Mutex::new(Settings::new(model)))
}

// ─── Profile persistence ────────────────────────────────────────────────────

/// Profile: settings that persist with the project in .omegon/profile.json.
/// Read on startup, written on change. Travels with git.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum ProfileSource {
    Project(PathBuf),
    User(PathBuf),
    #[default]
    BuiltInDefault,
}

impl ProfileSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Project(_) => "project",
            Self::User(_) => "user",
            Self::BuiltInDefault => "built-in defaults",
        }
    }
}

impl std::fmt::Display for ProfileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Project(path) => write!(f, "project:{}", path.display()),
            Self::User(path) => write!(f, "user:{}", path.display()),
            Self::BuiltInDefault => f.write_str("built-in defaults"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSaveTarget {
    ActiveSource,
    Project,
    User,
    /// Save as a named profile in the profiles registry directory.
    Named {
        name: String,
        scope: ProfileRegistryScope,
    },
}

#[derive(Debug, Clone)]
pub struct LoadedProfile {
    pub profile: Profile,
    pub source: ProfileSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProfileSelection {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileRegistryScope {
    Project,
    User,
    BuiltIn,
}

impl ProfileRegistryScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::BuiltIn => "built-in",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileRegistrySourceKind {
    RegistryFile,
    LegacySingleton,
    BuiltInDefault,
}

#[derive(Debug, Clone)]
pub struct ProfileRegistryEntry {
    pub id: String,
    pub scope: ProfileRegistryScope,
    pub source_kind: ProfileRegistrySourceKind,
    pub path: Option<PathBuf>,
    pub profile: Profile,
    pub editable: bool,
    pub portable: bool,
    pub shadows: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    pub entries: Vec<ProfileRegistryEntry>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Stable, human-readable profile identifier for compact UI surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional longer display label for settings and help surfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_model: Option<ProfileModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_intent: Option<ProfileModelIntent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    /// Operator-requested working-set policy class. This is distinct from the
    /// actual model-derived context window/class.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_context_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,

    // ── Context class routing ──
    /// Provider preference order. First = most preferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_order: Vec<String>,
    /// Explicit fallback providers for interactive routing. Empty means fail
    /// explicitly when the selected provider has no usable credentials.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_providers: Vec<String>,
    // ── Embedding service (hybrid search) ──
    /// Embedding service base URL (Ollama `/api/embed` endpoint).
    /// Overrides `OMEGON_EMBED_URL` env var. Default: `http://localhost:11434`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_url: Option<String>,
    /// Embedding model name (e.g. `nomic-embed-text`).
    /// Overrides `OMEGON_EMBED_MODEL` env var.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_model: Option<String>,

    // ── Default posture ──
    /// Default posture name. Can be a built-in (explorator/fabricator/architect/devastator)
    /// or a custom posture defined in `.omegon/postures/<name>.pkl`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_posture: Option<String>,

    // ── Permissions ──
    /// Unified operator permission policy.
    #[serde(default, skip_serializing_if = "ProfilePermissions::is_empty")]
    pub permissions: ProfilePermissions,
    /// Legacy compatibility alias for `permissions.trustedDirectories`.
    /// New writes migrate this into the `permissions` object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_directories: Vec<String>,

    // ── Automation ──
    /// Operator continuation policy. Does not override permissions or plan gates.
    #[serde(default, skip_serializing_if = "ProfileAutomation::is_empty")]
    pub automation: ProfileAutomation,

    // ── Updates ──
    /// Update channel: "stable" or "nightly".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_channel: Option<String>,
    /// Auto-update on session exit when a newer version is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,

    // ── Display preferences ──
    /// Tool output density: "lean", "compact", "detailed", "verbose".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_detail: Option<String>,
    /// UI presentation preference: "om", "active", or "full". Legacy
    /// "lean"/"slim" values deserialize as Om.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_presentation: Option<String>,

    // ── Sandbox ──
    /// Sandbox isolation for delegate/cleave children.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<bool>,
    /// Enable the interactive PTY-backed terminal tool for this profile.
    /// Set false for hardened/headless OCI agents that should use `bash`,
    /// `serve`, or workload controllers instead of interactive sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_tool: Option<bool>,

    // ── Persona / Tone ──
    /// Active persona name. Restored on next session start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Active tone name. Restored on next session start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,

    // ── Optional integrations ──
    /// Optional bridge integrations. Missing integrations stay disabled unless
    /// explicitly enabled by project/global profile or environment.
    #[serde(default, skip_serializing_if = "ProfileIntegrations::is_empty")]
    pub integrations: ProfileIntegrations,

    // ── Extension loading policy ──
    /// Native extension loading policy for this profile. Empty policy preserves
    /// existing behavior: installed enabled extensions are considered loadable.
    #[serde(default, skip_serializing_if = "ProfileExtensions::is_empty")]
    pub extensions: ProfileExtensions,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileAutomation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<AutomationLevel>,
}

impl ProfileAutomation {
    pub fn is_empty(&self) -> bool {
        self.level.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModel {
    pub provider: String,
    pub model_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModelIntent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_model_override: Option<String>,
}

impl ProfileModelIntent {
    pub fn from_route_intent(intent: &crate::route::ModelIntent) -> Self {
        let provider = match &intent.provider_selection {
            crate::route::ProviderSelection::Auto => "auto".to_string(),
            crate::route::ProviderSelection::Local => "local".to_string(),
            crate::route::ProviderSelection::Upstream => "upstream".to_string(),
            crate::route::ProviderSelection::Endpoint(endpoint) => endpoint.clone(),
        };
        let grade_policy = match &intent.grade_policy {
            crate::route::GradePolicy::Exact => "exact",
            crate::route::GradePolicy::Minimum => "minimum",
            crate::route::GradePolicy::NearestAllowed { .. } => "nearest",
        };
        Self {
            grade: intent
                .grade
                .as_ref()
                .map(|grade| grade.as_str().to_string()),
            provider: Some(provider),
            grade_policy: Some(grade_policy.to_string()),
            provider_policy: intent
                .provider_policy
                .map(|policy| policy.as_str().to_string()),
            exact_model_override: intent.exact_model_override.clone(),
        }
    }

    pub fn to_route_intent(&self) -> Option<crate::route::ModelIntent> {
        let grade = match self.grade.as_deref() {
            Some(raw) => Some(crate::route::ModelGrade::parse(raw)?),
            None => None,
        };
        let provider_selection = self
            .provider
            .as_deref()
            .and_then(crate::route::ProviderSelection::parse)
            .unwrap_or(crate::route::ProviderSelection::Auto);
        let grade_policy = match self.grade_policy.as_deref() {
            Some(raw) => crate::route::GradePolicy::parse(raw)?,
            None => crate::route::GradePolicy::Minimum,
        };
        let provider_policy = match self.provider_policy.as_deref() {
            Some(raw) => Some(crate::semantic_route::ProviderPolicy::parse(raw)?),
            None => None,
        };
        Some(crate::route::ModelIntent {
            grade,
            provider_selection,
            grade_policy,
            provider_policy,
            exact_model_override: self.exact_model_override.clone(),
            ..crate::route::ModelIntent::default()
        })
    }
}

pub fn persist_model_intent(
    cwd: &std::path::Path,
    intent: &crate::route::ModelIntent,
) -> anyhow::Result<()> {
    let mut profile = Profile::load(cwd);
    profile.model_intent = Some(ProfileModelIntent::from_route_intent(intent));
    profile.save(cwd)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePermissions {
    /// Directories outside the workspace that the agent can access without
    /// per-operation confirmation. Paths are expanded at runtime (~ → $HOME).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_directories: Vec<String>,
    /// Mount/environment identities observed when trusted directories were granted.
    /// Legacy profiles may contain only `trustedDirectories`; this sidecar is
    /// advisory and never grants access without the path prefix above.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_directory_grants: Vec<ProfileTrustGrant>,
    /// Per-tool permission policy. Keys are tool names such as `bash`, `write`,
    /// or `edit`; values are allow/prompt/deny rules with optional patterns.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, crate::permissions::ToolPermissionRule>,
    /// Optional Styrene RBAC role for this local operator/runtime. Stored as a
    /// string to keep profile parsing independent of styrene-rbac serde features.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTrustGrant {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mount_identity: Option<ProfileMountIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMountIdentity {
    pub fs_type: String,
    pub source: String,
    pub mount_point: String,
}

impl ProfilePermissions {
    pub fn is_empty(&self) -> bool {
        self.trusted_directories.is_empty()
            && self.trusted_directory_grants.is_empty()
            && self.tools.is_empty()
            && self.role.is_none()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileIntegrations {
    #[serde(default, skip_serializing_if = "ProfileMqttIntegration::is_empty")]
    pub mqtt: ProfileMqttIntegration,
}

impl ProfileIntegrations {
    pub fn is_empty(&self) -> bool {
        self.mqtt.is_empty()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMqttIntegration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_port: Option<u16>,
}

impl ProfileMqttIntegration {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none() && self.broker_host.is_none() && self.broker_port.is_none()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileExtensions {
    /// When non-empty, only these native extension names may load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled: Vec<String>,
    /// Native extension names that must not load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
}

impl ProfileExtensions {
    pub fn is_empty(&self) -> bool {
        self.enabled.is_empty() && self.disabled.is_empty()
    }

    pub fn permits(
        &self,
        extension_name: &str,
        env_enabled: &[String],
        env_disabled: &[String],
    ) -> bool {
        let name = extension_name.to_ascii_lowercase();
        let normalize = |s: &String| s.trim().to_ascii_lowercase();

        if env_disabled
            .iter()
            .map(normalize)
            .any(|disabled| disabled == name)
            || self
                .disabled
                .iter()
                .map(normalize)
                .any(|disabled| disabled == name)
        {
            return false;
        }

        if !env_enabled.is_empty() {
            return env_enabled
                .iter()
                .map(normalize)
                .any(|enabled| enabled == name);
        }

        if !self.enabled.is_empty() {
            return self
                .enabled
                .iter()
                .map(normalize)
                .any(|enabled| enabled == name);
        }

        true
    }
}

impl Profile {
    /// Compact human-readable identity for status surfaces.
    pub fn compact_label(&self) -> Option<&str> {
        self.name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
    }

    /// Load profile. Project-level (`<repo>/.omegon/profile.json`) overrides
    /// user-level (`~/.omegon/profile.json`). Both are optional. Registry
    /// active-profile pointers are resolved before legacy singleton fallbacks.
    pub fn load(cwd: &std::path::Path) -> Self {
        Self::load_with_source(cwd).profile
    }

    /// Load profile with explicit source metadata so save/apply surfaces can
    /// preserve user/project distinctions.
    pub fn load_with_source(cwd: &std::path::Path) -> LoadedProfile {
        ProfileRegistry::discover(cwd).resolve_active()
    }

    fn save_to_path(&self, path: &std::path::Path, label: &str) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut profile = self.clone();
        profile.normalize_permissions();
        let json = serde_json::to_string_pretty(&profile)?;
        crate::filelock::atomic_write_locked(path, json.as_bytes())?;
        tracing::debug!(path = %path.display(), label, "profile saved");
        Ok(())
    }

    /// Save to the project-level profile at the repository root.
    pub fn save(&self, cwd: &std::path::Path) -> anyhow::Result<()> {
        let path = project_profile_path(cwd);
        self.save_to_path(&path, "project")
    }

    /// Save to the user-level profile (~/.omegon/profile.json).
    pub fn save_global(&self) -> anyhow::Result<()> {
        let path = global_profile_path()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
        self.save_to_path(&path, "global")
    }

    pub fn save_to_target(
        &self,
        cwd: &std::path::Path,
        target: ProfileSaveTarget,
        active_source: &ProfileSource,
    ) -> anyhow::Result<ProfileSource> {
        match target {
            ProfileSaveTarget::Project => {
                let path = project_profile_path(cwd);
                self.save_to_path(&path, "project")?;
                Ok(ProfileSource::Project(path))
            }
            ProfileSaveTarget::User => {
                let path = global_profile_path()
                    .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
                self.save_to_path(&path, "global")?;
                Ok(ProfileSource::User(path))
            }
            ProfileSaveTarget::ActiveSource => match active_source {
                ProfileSource::Project(path) => {
                    self.save_to_path(path, "project")?;
                    Ok(ProfileSource::Project(path.clone()))
                }
                ProfileSource::User(path) => {
                    self.save_to_path(path, "global")?;
                    Ok(ProfileSource::User(path.clone()))
                }
                ProfileSource::BuiltInDefault => {
                    anyhow::bail!("profile save target is ambiguous; use --project or --user")
                }
            },
            ProfileSaveTarget::Named { name, scope } => {
                let safe_name = name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>();
                let path = match scope {
                    ProfileRegistryScope::Project => {
                        project_profiles_dir(cwd).join(format!("{safe_name}.json"))
                    }
                    ProfileRegistryScope::User | ProfileRegistryScope::BuiltIn => {
                        global_profiles_dir()
                            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?
                            .join(format!("{safe_name}.json"))
                    }
                };
                self.save_to_path(&path, &safe_name)?;
                match scope {
                    ProfileRegistryScope::Project => Ok(ProfileSource::Project(path)),
                    _ => Ok(ProfileSource::User(path)),
                }
            }
        }
    }

    pub fn effective_trusted_directories(&self) -> Vec<String> {
        let mut dirs = Vec::new();
        for dir in self
            .permissions
            .trusted_directories
            .iter()
            .chain(self.trusted_directories.iter())
        {
            push_unique(&mut dirs, dir);
        }
        dirs
    }

    pub fn set_trusted_directories(&mut self, dirs: Vec<String>) {
        self.permissions.trusted_directories.clear();
        for dir in dirs {
            push_unique(&mut self.permissions.trusted_directories, &dir);
        }
        self.trusted_directories.clear();
    }

    pub fn add_trusted_directory(&mut self, dir: String) {
        self.add_trusted_directory_grant(dir, None, None);
    }

    pub fn add_trusted_directory_grant(
        &mut self,
        dir: String,
        mount_identity: Option<ProfileMountIdentity>,
        environment: Option<String>,
    ) {
        let mut dirs = self.effective_trusted_directories();
        push_unique(&mut dirs, &dir);
        self.set_trusted_directories(dirs);
        self.permissions
            .trusted_directory_grants
            .retain(|grant| !grant.path.eq_ignore_ascii_case(&dir));
        self.permissions
            .trusted_directory_grants
            .push(ProfileTrustGrant {
                path: dir,
                mount_identity,
                environment,
            });
    }

    pub fn remove_trusted_directory(&mut self, dir: &str) {
        let mut dirs = self.effective_trusted_directories();
        retain_not_equal(&mut dirs, dir);
        self.set_trusted_directories(dirs);
        self.permissions
            .trusted_directory_grants
            .retain(|grant| !grant.path.eq_ignore_ascii_case(dir));
    }

    fn normalize_permissions(&mut self) {
        let dirs = self.effective_trusted_directories();
        self.set_trusted_directories(dirs.clone());
        self.permissions
            .trusted_directory_grants
            .retain(|grant| dirs.iter().any(|dir| dir.eq_ignore_ascii_case(&grant.path)));
    }

    /// Apply profile to settings (called at startup).
    pub fn apply_to(&self, settings: &mut Settings) {
        settings.profile_name = self.compact_label().map(ToOwned::to_owned);

        if let Some(ref m) = self.last_used_model {
            settings.set_model(&format!("{}:{}", m.provider, m.model_id));
        }
        if let Some(ref t) = self.thinking_level
            && let Some(level) = ThinkingLevel::parse(t)
        {
            settings.thinking = level;
        }
        if let Some(ref class) = self.requested_context_class
            && let Some(class) = ContextClass::parse(class)
        {
            settings.set_requested_context_class(class);
        }
        if let Some(turns) = self.max_turns {
            settings.max_turns = turns;
        }
        if let Some(level) = self.automation.level {
            settings.automation_level = level;
        }
        if !self.provider_order.is_empty() {
            settings.provider_order = self.provider_order.clone();
        }
        settings.fallback_providers = self.fallback_providers.clone();
        let trusted_directories = self.effective_trusted_directories();
        if !trusted_directories.is_empty() {
            settings.trusted_directories = trusted_directories;
        }
        if let Some(ref ch) = self.update_channel {
            settings.update_channel = ch.clone();
        }
        if let Some(au) = self.auto_update {
            settings.auto_update = au;
        }
        if let Some(ref td) = self.tool_detail
            && let Some(detail) = ToolDetail::parse(td)
        {
            settings.tool_detail = detail;
        }
        if let Some(ref level) = self.ui_presentation
            && let Ok(level) = crate::surfaces::layout::UiPresentationLevel::parse(level)
        {
            settings.ui_presentation = level;
        }
        if let Some(s) = self.sandbox {
            settings.sandbox = s;
        }
        if let Some(enabled) = self.terminal_tool {
            settings.terminal_tool = enabled;
        }
        // persona and tone are restored by the plugin system at session start,
        // not by Settings.apply_to — Profile stores the name for resumption.
    }

    /// Apply profile to settings with posture resolution (needs cwd for custom postures).
    pub fn apply_to_with_posture(&self, settings: &mut Settings, cwd: &std::path::Path) {
        // Apply default posture first (before other overrides)
        if let Some(ref posture_name) = self.default_posture {
            match resolve_posture_by_name(posture_name, cwd) {
                Ok(ResolvedPosture::BuiltIn(preset)) => {
                    settings.set_posture(preset);
                    tracing::info!(posture = posture_name, "default posture from profile");
                }
                Ok(ResolvedPosture::Custom(custom)) => {
                    custom.apply_to(settings);
                    tracing::info!(
                        posture = posture_name,
                        base = custom.def.posture.base,
                        "custom posture from profile"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        posture = posture_name,
                        "failed to resolve default posture: {e}"
                    );
                }
            }
        }

        // Then apply remaining profile fields (may override posture-set values)
        self.apply_to(settings);
    }

    /// Capture current settings into the profile (called on change).
    pub fn capture_from(&mut self, settings: &Settings) {
        self.last_used_model = Some(ProfileModel {
            provider: settings.provider().to_string(),
            model_id: settings.model_short().to_string(),
        });
        self.thinking_level = Some(settings.thinking.as_str().to_string());
        self.requested_context_class = settings
            .requested_context_class
            .map(|class| class.short().to_lowercase());
        self.max_turns = Some(settings.max_turns);
        if settings.automation_level != AutomationLevel::default()
            || self.automation.level.is_some()
        {
            self.automation.level = Some(settings.automation_level);
        } else {
            self.automation.level = None;
        }
        self.provider_order = settings.provider_order.clone();
        self.fallback_providers = settings.fallback_providers.clone();
        self.set_trusted_directories(settings.trusted_directories.clone());
        if settings.update_channel != "stable" {
            self.update_channel = Some(settings.update_channel.clone());
        } else {
            self.update_channel = None;
        }
        if settings.auto_update {
            self.auto_update = Some(true);
        } else {
            self.auto_update = None;
        }
        if settings.tool_detail != ToolDetail::Detailed {
            self.tool_detail = Some(settings.tool_detail.as_str().to_string());
        } else {
            self.tool_detail = None;
        }
        if settings.ui_presentation != crate::surfaces::layout::UiPresentationLevel::Om {
            self.ui_presentation = Some(settings.ui_presentation.name().to_string());
        } else {
            self.ui_presentation = None;
        }
        if settings.sandbox {
            self.sandbox = Some(true);
        } else {
            self.sandbox = None;
        }
        if !settings.terminal_tool {
            self.terminal_tool = Some(false);
        } else {
            self.terminal_tool = None;
        }
    }
}

fn retain_not_equal(values: &mut Vec<String>, target: &str) {
    values.retain(|value| value.trim() != target.trim());
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !values.iter().any(|existing| existing.trim() == value) {
        values.push(value.to_string());
    }
}

fn project_profile_path(cwd: &std::path::Path) -> std::path::PathBuf {
    crate::setup::find_project_root(cwd).join(".omegon/profile.json")
}

fn project_profiles_dir(cwd: &std::path::Path) -> std::path::PathBuf {
    crate::setup::find_project_root(cwd).join(".omegon/profiles")
}

fn project_active_profile_path(cwd: &std::path::Path) -> std::path::PathBuf {
    crate::setup::find_project_root(cwd).join(".omegon/active-profile.json")
}

fn global_profile_path() -> Option<std::path::PathBuf> {
    // Preferred user-level home follows the rest of the harness convention.
    // Fall back to the legacy XDG/App Support path for backward compatibility.
    dirs::home_dir()
        .map(|d| d.join(".omegon/profile.json"))
        .or_else(|| dirs::config_dir().map(|d| d.join("omegon/profile.json")))
}

fn global_profiles_dir() -> Option<std::path::PathBuf> {
    dirs::home_dir()
        .map(|d| d.join(".omegon/profiles"))
        .or_else(|| dirs::config_dir().map(|d| d.join("omegon/profiles")))
}

fn global_active_profile_path() -> Option<std::path::PathBuf> {
    dirs::home_dir()
        .map(|d| d.join(".omegon/active-profile.json"))
        .or_else(|| dirs::config_dir().map(|d| d.join("omegon/active-profile.json")))
}

fn read_active_profile_selection(
    path: &std::path::Path,
    registry: &ProfileRegistry,
) -> Option<ActiveProfileSelection> {
    let content = std::fs::read_to_string(path).ok()?;
    let selection = serde_json::from_str::<ActiveProfileSelection>(&content).ok()?;
    registry.resolve_explicit(&selection)?;
    Some(selection)
}

pub fn active_profile_selection(cwd: &std::path::Path) -> ActiveProfileSelection {
    let registry = ProfileRegistry::discover(cwd);
    if let Some(path) = project_active_profile_path_from_registry(&registry)
        && let Some(selection) = read_active_profile_selection(&path, &registry)
    {
        return selection;
    }

    if let Some(entry) = registry.entries.iter().find(|entry| {
        entry.scope == ProfileRegistryScope::Project
            && entry.source_kind == ProfileRegistrySourceKind::LegacySingleton
    }) {
        return ActiveProfileSelection {
            id: entry.id.clone(),
            scope: Some(entry.scope.as_str().to_string()),
        };
    }

    if let Some(path) = global_active_profile_path()
        && let Some(selection) = read_active_profile_selection(&path, &registry)
    {
        return selection;
    }

    for (scope, kind) in [
        (
            ProfileRegistryScope::User,
            ProfileRegistrySourceKind::LegacySingleton,
        ),
        (
            ProfileRegistryScope::BuiltIn,
            ProfileRegistrySourceKind::BuiltInDefault,
        ),
    ] {
        if let Some(entry) = registry
            .entries
            .iter()
            .find(|entry| entry.scope == scope && entry.source_kind == kind)
        {
            return ActiveProfileSelection {
                id: entry.id.clone(),
                scope: Some(entry.scope.as_str().to_string()),
            };
        }
    }

    ActiveProfileSelection {
        id: "built-in-default".into(),
        scope: Some("built-in".into()),
    }
}

pub fn save_project_active_profile_selection(
    cwd: &std::path::Path,
    selection: &ActiveProfileSelection,
) -> anyhow::Result<std::path::PathBuf> {
    let registry = ProfileRegistry::discover(cwd);
    if registry.resolve_explicit(selection).is_none() {
        let scope = selection.scope.as_deref().unwrap_or("any scope");
        anyhow::bail!("profile `{}` was not found in {}", selection.id, scope);
    }

    let path = project_active_profile_path(cwd);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::filelock::atomic_write_locked(
        &path,
        (serde_json::to_string_pretty(selection)? + "\n").as_bytes(),
    )?;
    Ok(path)
}

impl ProfileRegistryEntry {
    fn source(&self) -> ProfileSource {
        match self.scope {
            ProfileRegistryScope::Project => self
                .path
                .clone()
                .map(ProfileSource::Project)
                .unwrap_or(ProfileSource::BuiltInDefault),
            ProfileRegistryScope::User => self
                .path
                .clone()
                .map(ProfileSource::User)
                .unwrap_or(ProfileSource::BuiltInDefault),
            ProfileRegistryScope::BuiltIn => ProfileSource::BuiltInDefault,
        }
    }
}

impl ProfileRegistry {
    pub fn discover(cwd: &std::path::Path) -> Self {
        let mut registry = Self::default();
        registry.load_registry_dir(ProfileRegistryScope::User, global_profiles_dir());
        registry.load_registry_dir(
            ProfileRegistryScope::Project,
            Some(project_profiles_dir(cwd)),
        );
        registry.load_legacy(
            ProfileRegistryScope::User,
            "user-default",
            global_profile_path(),
        );
        registry.load_legacy(
            ProfileRegistryScope::Project,
            "project-default",
            Some(project_profile_path(cwd)),
        );
        registry.entries.push(ProfileRegistryEntry {
            id: "built-in-default".into(),
            scope: ProfileRegistryScope::BuiltIn,
            source_kind: ProfileRegistrySourceKind::BuiltInDefault,
            path: None,
            profile: Profile::default(),
            editable: false,
            portable: true,
            shadows: Vec::new(),
        });
        registry.compute_shadows();
        registry
    }

    /// Resolve one explicit registry selection without mutating active-profile state.
    pub fn resolve_explicit(&self, selection: &ActiveProfileSelection) -> Option<LoadedProfile> {
        let entry = self.find_selected(selection)?;
        let mut profile = entry.profile.clone();
        if profile.compact_label().is_none() {
            profile.name = Some(entry.id.clone());
        }
        Some(LoadedProfile {
            profile,
            source: entry.source(),
        })
    }

    pub fn resolve_active(&self) -> LoadedProfile {
        if let Some(loaded) =
            self.resolve_selection(project_active_profile_path_from_registry(self))
        {
            return loaded;
        }

        if let Some(entry) = self.entries.iter().find(|entry| {
            entry.scope == ProfileRegistryScope::Project
                && entry.source_kind == ProfileRegistrySourceKind::LegacySingleton
        }) {
            return LoadedProfile {
                profile: entry.profile.clone(),
                source: entry.source(),
            };
        }

        if let Some(loaded) = self.resolve_selection(global_active_profile_path()) {
            return loaded;
        }

        for (scope, kind) in [
            (
                ProfileRegistryScope::User,
                ProfileRegistrySourceKind::LegacySingleton,
            ),
            (
                ProfileRegistryScope::BuiltIn,
                ProfileRegistrySourceKind::BuiltInDefault,
            ),
        ] {
            if let Some(entry) = self
                .entries
                .iter()
                .find(|entry| entry.scope == scope && entry.source_kind == kind)
            {
                return LoadedProfile {
                    profile: entry.profile.clone(),
                    source: entry.source(),
                };
            }
        }

        LoadedProfile {
            profile: Profile::default(),
            source: ProfileSource::BuiltInDefault,
        }
    }

    fn resolve_selection(&self, path: Option<std::path::PathBuf>) -> Option<LoadedProfile> {
        let path = path?;
        let content = std::fs::read_to_string(&path).ok()?;
        let selection = serde_json::from_str::<ActiveProfileSelection>(&content).ok()?;
        let Some(entry) = self.find_selected(&selection) else {
            tracing::warn!(path = %path.display(), profile = %selection.id, "active profile selection did not match any registry entry");
            return None;
        };
        tracing::debug!(path = %path.display(), profile = %entry.id, scope = entry.scope.as_str(), "active profile selection loaded");
        let mut profile = entry.profile.clone();
        if profile.compact_label().is_none() {
            profile.name = Some(entry.id.clone());
        }
        Some(LoadedProfile {
            profile,
            source: entry.source(),
        })
    }

    fn load_registry_dir(&mut self, scope: ProfileRegistryScope, dir: Option<std::path::PathBuf>) {
        let Some(dir) = dir else { return };
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut paths: Vec<_> = read_dir
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        paths.sort();
        for path in paths {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(profile) = serde_json::from_str::<Profile>(&content) else {
                tracing::warn!(path = %path.display(), "profile registry entry could not be parsed");
                continue;
            };
            self.entries.push(ProfileRegistryEntry {
                id: stem.to_string(),
                scope,
                source_kind: ProfileRegistrySourceKind::RegistryFile,
                path: Some(path),
                profile,
                editable: scope != ProfileRegistryScope::BuiltIn,
                portable: scope != ProfileRegistryScope::User,
                shadows: Vec::new(),
            });
        }
    }

    fn load_legacy(
        &mut self,
        scope: ProfileRegistryScope,
        id: &str,
        path: Option<std::path::PathBuf>,
    ) {
        let Some(path) = path else { return };
        let Ok(content) = std::fs::read_to_string(&path) else {
            return;
        };
        let Ok(profile) = serde_json::from_str::<Profile>(&content) else {
            return;
        };
        self.entries.push(ProfileRegistryEntry {
            id: id.into(),
            scope,
            source_kind: ProfileRegistrySourceKind::LegacySingleton,
            path: Some(path),
            profile,
            editable: true,
            portable: scope == ProfileRegistryScope::Project,
            shadows: Vec::new(),
        });
    }

    fn find_selected(&self, selection: &ActiveProfileSelection) -> Option<&ProfileRegistryEntry> {
        self.entries.iter().find(|entry| {
            entry.id == selection.id
                && selection
                    .scope
                    .as_deref()
                    .is_none_or(|scope| scope == entry.scope.as_str())
        })
    }

    fn compute_shadows(&mut self) {
        let entries = self.entries.clone();
        for entry in &mut self.entries {
            entry.shadows = entries
                .iter()
                .filter(|other| other.id == entry.id && other.scope != entry.scope)
                .map(|other| other.scope.as_str().to_string())
                .collect();
        }
    }
}

fn project_active_profile_path_from_registry(
    registry: &ProfileRegistry,
) -> Option<std::path::PathBuf> {
    registry.entries.iter().find_map(
        |entry| match (&entry.scope, entry.source_kind, &entry.path) {
            (
                ProfileRegistryScope::Project,
                ProfileRegistrySourceKind::RegistryFile,
                Some(path),
            ) => path
                .parent()
                .and_then(|profiles_dir| profiles_dir.parent())
                .map(|omegon_dir| omegon_dir.join("active-profile.json")),
            (
                ProfileRegistryScope::Project,
                ProfileRegistrySourceKind::LegacySingleton,
                Some(path),
            ) => path
                .parent()
                .map(|omegon_dir| omegon_dir.join("active-profile.json")),
            _ => None,
        },
    )
}

// ─── Custom posture definitions ─────────────────────────────────────────────

/// A custom posture definition loaded from a `.pkl` file.
///
/// Example `~/.omegon/postures/reviewer.pkl`:
/// ```pkl
/// posture {
///   name = "reviewer"
///   description = "Code review — read everything, suggest, don't edit"
///   base = "architect"
///   thinking = "high"
///   context_class = "extended"
///   slim = false
/// }
///
/// delegation {
///   prefer_local = true
///   default_worker = "scout"
/// }
///
/// tools {
///   disabled = new Listing<String> { "edit"; "write"; "change" }
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct PostureFile {
    pub posture: PostureDef,
    pub delegation: Option<DelegationDef>,
    pub tools: Option<ToolsDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PostureDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Base built-in posture to inherit from (explorator/fabricator/architect/devastator).
    #[serde(default = "default_base")]
    pub base: String,
    /// Thinking level override (off/minimal/low/medium/high).
    pub thinking: Option<String>,
    /// Context class override (compact/standard/extended/massive). Legacy aliases compact/standard/extended/massive are accepted.
    pub context_class: Option<String>,
    /// Whether to enable slim mode.
    pub slim: Option<bool>,
    /// Max turns override.
    pub max_turns: Option<u32>,
}

fn default_base() -> String {
    "architect".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelegationDef {
    /// Prefer local models for delegation.
    #[serde(default)]
    pub prefer_local: bool,
    /// Default worker profile (scout/patch/verify).
    pub default_worker: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolsDef {
    /// Tools to disable in this posture.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Tools to enable (overrides disabled list).
    #[serde(default)]
    pub enabled: Vec<String>,
}

/// Resolved custom posture ready to apply to settings.
#[derive(Debug, Clone)]
pub struct ResolvedCustomPosture {
    pub def: PostureFile,
    pub base_preset: PosturePreset,
}

impl ResolvedCustomPosture {
    /// Apply this custom posture to settings.
    pub fn apply_to(&self, settings: &mut Settings) {
        // Start from base preset
        settings.set_posture(self.base_preset);

        // Override with custom values
        if let Some(ref t) = self.def.posture.thinking
            && let Some(level) = ThinkingLevel::parse(t)
        {
            settings.thinking = level;
        }
        if let Some(ref cc) = self.def.posture.context_class
            && let Some(class) = ContextClass::parse(cc)
        {
            settings.requested_context_class = Some(class);
        }
        if let Some(true) = self.def.posture.slim {
            settings.set_posture(PosturePreset::Explorator);
        }
        if let Some(turns) = self.def.posture.max_turns {
            settings.max_turns = turns;
        }
        // Apply tool overrides from custom posture
        if let Some(ref tools) = self.def.tools {
            settings.posture_disabled_tools = tools.disabled.clone();
            settings.posture_enabled_tools = tools.enabled.clone();
        }
    }
}

/// Resolve a posture name to either a built-in preset or a custom posture file.
/// Resolution order: built-in → project `.omegon/postures/` → user `~/.omegon/postures/`.
pub fn resolve_posture_by_name(
    name: &str,
    cwd: &std::path::Path,
) -> Result<ResolvedPosture, String> {
    // Built-in presets
    match name.to_lowercase().as_str() {
        "explorator" => return Ok(ResolvedPosture::BuiltIn(PosturePreset::Explorator)),
        "fabricator" => return Ok(ResolvedPosture::BuiltIn(PosturePreset::Fabricator)),
        "architect" => return Ok(ResolvedPosture::BuiltIn(PosturePreset::Architect)),
        "devastator" => return Ok(ResolvedPosture::BuiltIn(PosturePreset::Devastator)),
        _ => {}
    }

    // Project-level custom postures
    let project_root = crate::setup::find_project_root(cwd);
    let project_posture = project_root
        .join(".omegon/postures")
        .join(format!("{name}.pkl"));
    if project_posture.exists() {
        return load_custom_posture(&project_posture);
    }

    // User-level custom postures
    if let Some(home) = dirs::home_dir() {
        let user_posture = home.join(format!(".omegon/postures/{name}.pkl"));
        if user_posture.exists() {
            return load_custom_posture(&user_posture);
        }
    }

    Err(format!(
        "unknown posture '{name}': not a built-in (explorator/fabricator/architect/devastator) \
         and no custom posture found at .omegon/postures/{name}.pkl or ~/.omegon/postures/{name}.pkl"
    ))
}

/// The result of posture resolution.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // singleton lifetime — created once, never cloned
pub enum ResolvedPosture {
    BuiltIn(PosturePreset),
    Custom(ResolvedCustomPosture),
}

fn load_custom_posture(path: &std::path::Path) -> Result<ResolvedPosture, String> {
    // Check if pkl binary is available before attempting to parse.
    // rpkl shells out to the pkl binary; if it's not installed, the error
    // is cryptic ("No such file or directory"). Fail clearly instead.
    if std::process::Command::new("pkl")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        return Err(format!(
            "custom posture {} requires the PKL CLI (https://pkl-lang.org/main/current/pkl-cli/index.html). \
             Install it: brew install pkl (macOS) or download from pkl-lang.org",
            path.display()
        ));
    }

    let posture_file: PostureFile =
        rpkl::from_config_with_options(path, crate::pkl_modules::omegon_eval_options())
            .map_err(|e| format!("failed to load posture {}: {e}", path.display()))?;

    let base_preset = match posture_file.posture.base.to_lowercase().as_str() {
        "explorator" => PosturePreset::Explorator,
        "fabricator" => PosturePreset::Fabricator,
        "architect" => PosturePreset::Architect,
        "devastator" => PosturePreset::Devastator,
        other => {
            return Err(format!(
                "invalid base posture '{other}' in {}: must be explorator/fabricator/architect/devastator",
                path.display()
            ));
        }
    };

    Ok(ResolvedPosture::Custom(ResolvedCustomPosture {
        def: posture_file,
        base_preset,
    }))
}

/// List all available postures (built-in + custom from project and user directories).
pub fn list_available_postures(cwd: &std::path::Path) -> Vec<(String, String, bool)> {
    let mut postures: Vec<(String, String, bool)> = vec![
        (
            "explorator".into(),
            "Cheap-first reconnaissance, lean execution".into(),
            true,
        ),
        ("fabricator".into(), "Balanced implementation".into(), true),
        (
            "architect".into(),
            "Systems-engineering orchestrator (default)".into(),
            true,
        ),
        (
            "devastator".into(),
            "Maximum-force deep reasoning".into(),
            true,
        ),
    ];

    // Scan custom posture directories
    for dir in custom_posture_dirs(cwd) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "pkl")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    // Skip if same name as built-in
                    if !postures.iter().any(|(n, _, _)| n == stem) {
                        let desc = rpkl::from_config_with_options::<PostureFile>(
                            &path,
                            crate::pkl_modules::omegon_eval_options(),
                        )
                        .ok()
                        .map(|f| f.posture.description)
                        .unwrap_or_default();
                        postures.push((stem.to_string(), desc, false));
                    }
                }
            }
        }
    }

    postures
}

fn custom_posture_dirs(cwd: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    let project_root = crate::setup::find_project_root(cwd);
    dirs.push(project_root.join(".omegon/postures"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".omegon/postures"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_explicit_profile_does_not_change_active_selection() {
        let tmp = tempfile::tempdir().unwrap();
        let profile_dir = tmp.path().join(".omegon/profiles");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("review.json"),
            r#"{"name":"review","thinkingLevel":"high"}"#,
        )
        .unwrap();

        let registry = ProfileRegistry::discover(tmp.path());
        let loaded = registry
            .resolve_explicit(&ActiveProfileSelection {
                id: "review".into(),
                scope: Some("project".into()),
            })
            .expect("explicit profile");

        assert_eq!(loaded.profile.compact_label(), Some("review"));
        assert!(!tmp.path().join(".omegon/active-profile.json").exists());
    }

    #[test]
    fn posture_default_is_architect() {
        let s = Settings::default();
        assert_eq!(
            s.posture,
            BehavioralPosture::fixed(PosturePreset::Architect)
        );
        assert_eq!(s.resource_envelope().thinking, ThinkingLevel::Medium);
        assert_eq!(
            s.resource_envelope().requested_context_class,
            ContextClass::Extended
        );
    }

    #[test]
    fn posture_preset_resource_defaults_are_stable() {
        let explorator = PosturePreset::Explorator.default_resource_envelope();
        assert_eq!(explorator.thinking, ThinkingLevel::Minimal);
        assert_eq!(explorator.requested_context_class, ContextClass::Compact);
        assert_eq!(
            explorator.effective_context_cap_tokens,
            Some(ContextClass::Compact.nominal_tokens())
        );
        assert!(explorator.compact_reply_reserve);
        assert!(explorator.compact_tool_schema_reserve);

        let fabricator = PosturePreset::Fabricator.default_resource_envelope();
        assert_eq!(fabricator.thinking, ThinkingLevel::Low);
        assert_eq!(fabricator.requested_context_class, ContextClass::Standard);

        let architect = PosturePreset::Architect.default_resource_envelope();
        assert_eq!(architect.thinking, ThinkingLevel::Medium);
        assert_eq!(architect.requested_context_class, ContextClass::Extended);

        let devastator = PosturePreset::Devastator.default_resource_envelope();
        assert_eq!(devastator.thinking, ThinkingLevel::High);
        assert_eq!(devastator.requested_context_class, ContextClass::Massive);
    }

    #[test]
    fn operating_profile_reflects_posture_and_resources() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        s.set_posture(PosturePreset::Fabricator);

        let profile = s.operating_profile();
        assert_eq!(
            profile.posture,
            BehavioralPosture::fixed(PosturePreset::Fabricator)
        );
        assert_eq!(profile.resources.thinking, ThinkingLevel::Low);
        assert_eq!(
            profile.resources.requested_context_class,
            ContextClass::Standard
        );
        assert_eq!(profile.identity, RuntimeIdentity::local_interactive());
        assert_eq!(
            profile.authorization,
            AuthorizationContext::local_descriptive()
        );
        assert_eq!(profile.persona, PersonaState::default());
        assert_eq!(
            profile.summary(),
            "local-operator / Fabricator / low / Standard / operator@local"
        );
    }

    #[test]
    fn authorization_context_presets_are_descriptive_only() {
        let authz = AuthorizationContext::local_descriptive();
        assert_eq!(authz.roles, vec!["operator"]);
        assert!(authz.capabilities.is_empty());
        assert_eq!(authz.trust_domain.as_deref(), Some("local"));
        assert_eq!(authz.summary(), "operator@local");
    }

    #[test]
    fn runtime_identity_presets_are_descriptive_only() {
        let interactive = RuntimeIdentity::local_interactive();
        assert_eq!(interactive.principal_id.as_deref(), Some("local-operator"));
        assert_eq!(interactive.issuer.as_deref(), Some("local-session"));
        assert_eq!(interactive.session_kind.as_deref(), Some("interactive"));
        assert_eq!(interactive.summary_principal(), "local-operator");

        let daemon = RuntimeIdentity::local_control_plane();
        assert_eq!(daemon.principal_id.as_deref(), Some("daemon-supervisor"));
        assert_eq!(daemon.issuer.as_deref(), Some("local-daemon"));
        assert_eq!(daemon.session_kind.as_deref(), Some("control-plane"));
    }

    #[test]
    fn operating_profile_persona_overlay_is_descriptive_only() {
        let profile = Settings::default()
            .operating_profile()
            .with_persona(PersonaState::from_ids(
                Some("dev.styrene.omegon.systems-engineer".into()),
                Some("persona:dev.styrene.omegon.systems-engineer".into()),
            ));
        assert_eq!(
            profile.persona.persona_id.as_deref(),
            Some("dev.styrene.omegon.systems-engineer")
        );
        assert_eq!(
            profile.persona.mind_id.as_deref(),
            Some("persona:dev.styrene.omegon.systems-engineer")
        );
        assert_eq!(
            profile.summary(),
            "dev.styrene.omegon.systems-engineer / Architect / medium / Extended / operator@local"
        );
    }

    #[test]
    fn operating_profile_can_overlay_identity() {
        let profile = Settings::default()
            .operating_profile()
            .with_identity(RuntimeIdentity::local_control_plane());
        assert_eq!(
            profile.summary(),
            "daemon-supervisor / Architect / medium / Extended / operator@local"
        );
    }

    #[test]
    fn set_posture_updates_behavioral_defaults() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        s.set_posture(PosturePreset::Fabricator);
        assert_eq!(
            s.posture,
            BehavioralPosture::fixed(PosturePreset::Fabricator)
        );
        assert!(!s.is_slim());
        assert_eq!(s.thinking, ThinkingLevel::Low);
        assert_eq!(s.requested_context_class, Some(ContextClass::Standard));

        s.set_posture(PosturePreset::Devastator);
        assert_eq!(
            s.posture,
            BehavioralPosture::fixed(PosturePreset::Devastator)
        );
        assert_eq!(s.thinking, ThinkingLevel::High);
        assert_eq!(s.requested_context_class, Some(ContextClass::Massive));
    }

    #[test]
    fn slim_mode_reduces_defaults() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        s.set_posture(PosturePreset::Explorator);
        assert!(s.is_slim());
        assert_eq!(s.thinking, ThinkingLevel::Minimal);
        assert_eq!(s.requested_context_class, Some(ContextClass::Compact));
        let policy = s.selector_policy();
        assert_eq!(policy.requested_class, ContextClass::Compact);
        assert_eq!(policy.model_window, ContextClass::Compact.nominal_tokens());
        assert!(policy.reply_reserve < 8_192);
        assert!(policy.tool_schema_reserve < 4_096);
    }

    #[test]
    fn settings_default() {
        let s = Settings::default();
        assert_eq!(s.thinking, ThinkingLevel::Medium);
        assert_eq!(s.context_window, 200_000);
        assert_eq!(s.context_class, ContextClass::Standard);
    }

    #[test]
    fn model_short_extracts_name() {
        let s = Settings::new("anthropic:claude-opus-4-7");
        assert_eq!(s.model_short(), "claude-opus-4-7");
        assert_eq!(s.provider(), "anthropic");
    }

    #[test]
    fn provider_infers_bare_local_model_ids() {
        // qwen3:30b — 'qwen3' is a model family, not a provider prefix
        // so the full 'qwen3:30b' name is preserved (not just '30b')
        let s = Settings::new("qwen3:30b");
        assert_eq!(s.model_short(), "qwen3:30b");
        assert_eq!(s.provider(), "ollama");

        let local = Settings::new("local:qwen3:30b");
        assert_eq!(local.model_short(), "qwen3:30b");
        assert_eq!(local.provider(), "ollama");
    }

    #[test]
    fn humanize_model_id_strips_provider_and_latest() {
        // Provider prefix stripped
        assert_eq!(
            humanize_model_id("anthropic:claude-opus-4-7"),
            "claude-opus-4-7"
        );
        assert_eq!(humanize_model_id("openai:gpt-4o"), "gpt-4o");
        // Ollama :latest stripped
        assert_eq!(
            humanize_model_id("ollama:glm-4.7-flash:latest"),
            "glm-4.7-flash"
        );
        assert_eq!(humanize_model_id("local:mistral:latest"), "mistral");
        // Non-provider first segment kept
        assert_eq!(humanize_model_id("qwen3:30b"), "qwen3:30b");
        assert_eq!(humanize_model_id("glm-4.7-flash:latest"), "glm-4.7-flash");
        // HuggingFace org/repo
        assert_eq!(humanize_model_id("huggingface:Qwen/Qwen3-32B"), "Qwen3-32B");
        // Bare model
        assert_eq!(humanize_model_id("claude-opus-4-7"), "claude-opus-4-7");
    }

    #[test]
    fn thinking_level_round_trip() {
        for level in ThinkingLevel::all() {
            let s = level.as_str();
            assert_eq!(ThinkingLevel::parse(s), Some(*level));
        }
    }

    #[test]
    fn thinking_level_display_names() {
        assert_eq!(ThinkingLevel::Off.display_name(), "Servitor");
        assert_eq!(ThinkingLevel::Minimal.display_name(), "Functionary");
        assert_eq!(ThinkingLevel::Low.display_name(), "Adept");
        assert_eq!(ThinkingLevel::Medium.display_name(), "Magos");
        assert_eq!(ThinkingLevel::High.display_name(), "Archmagos");
    }

    #[test]
    fn thinking_level_parse_mechanicum_names() {
        assert_eq!(ThinkingLevel::parse("servitor"), Some(ThinkingLevel::Off));
        assert_eq!(
            ThinkingLevel::parse("functionary"),
            Some(ThinkingLevel::Minimal)
        );
        assert_eq!(ThinkingLevel::parse("adept"), Some(ThinkingLevel::Low));
        assert_eq!(ThinkingLevel::parse("magos"), Some(ThinkingLevel::Medium));
        assert_eq!(ThinkingLevel::parse("archmagos"), Some(ThinkingLevel::High));
    }

    #[test]
    fn context_window_from_route_matrix() {
        // These should resolve via the embedded route matrix
        assert_eq!(infer_context_window("anthropic:claude-opus-4-7"), 1_000_000);
        assert_eq!(
            infer_context_window("anthropic:claude-sonnet-4-6"),
            1_000_000
        );
        assert_eq!(infer_context_window("openai:gpt-5.4"), 1_000_000);
        assert_eq!(infer_context_window("anthropic:claude-haiku-4-5"), 200_000);
    }

    #[test]
    fn route_matrix_glob_lookup_matches_prefix_patterns() {
        assert_eq!(
            lookup_context_ceiling("anthropic", "claude-sonnet-4-6"),
            Some(1_000_000)
        );
        assert_eq!(
            lookup_context_ceiling("anthropic", "claude-sonnet-4-6-20260401"),
            Some(1_000_000)
        );
        assert_eq!(lookup_context_ceiling("openai", "gpt-5.4"), Some(1_000_000));
        assert_eq!(
            lookup_context_ceiling("openai", "gpt-5.4-mini"),
            Some(400_000)
        );
        assert_eq!(lookup_context_ceiling("ollama", "qwen3:32b"), None);
        assert_eq!(lookup_context_ceiling("openai", "unknown-model"), None);
    }

    #[test]
    fn infer_context_window_uses_exact_registry_entries_before_fallbacks() {
        assert_eq!(infer_context_window("openrouter:qwen/qwen-qwq-32b"), 32_768);
        assert_eq!(
            infer_context_window("openrouter:qwen/qwen-2.5-72b-instruct"),
            131_072
        );
    }
    #[test]
    fn context_window_fallback_heuristic() {
        // Unknown models fall back to Compact (fail-closed)
        assert_eq!(infer_context_window("mystery:unknown-model"), 131_072);
    }

    #[test]
    fn context_class_from_tokens() {
        assert_eq!(ContextClass::from_tokens(100_000), ContextClass::Compact);
        assert_eq!(ContextClass::from_tokens(131_072), ContextClass::Compact);
        assert_eq!(ContextClass::from_tokens(131_073), ContextClass::Standard);
        assert_eq!(ContextClass::from_tokens(200_000), ContextClass::Standard);
        assert_eq!(ContextClass::from_tokens(278_528), ContextClass::Standard);
        assert_eq!(ContextClass::from_tokens(278_529), ContextClass::Extended);
        assert_eq!(ContextClass::from_tokens(400_000), ContextClass::Extended);
        assert_eq!(ContextClass::from_tokens(450_560), ContextClass::Extended);
        assert_eq!(ContextClass::from_tokens(450_561), ContextClass::Massive);
        assert_eq!(ContextClass::from_tokens(1_000_000), ContextClass::Massive);
    }

    #[test]
    fn context_class_ordering() {
        assert!(ContextClass::Compact < ContextClass::Standard);
        assert!(ContextClass::Standard < ContextClass::Extended);
        assert!(ContextClass::Extended < ContextClass::Massive);
    }

    #[test]
    fn context_class_delta() {
        assert_eq!(ContextClass::Massive.delta(ContextClass::Compact), 3);
        assert_eq!(ContextClass::Compact.delta(ContextClass::Massive), -3);
        assert_eq!(ContextClass::Extended.delta(ContextClass::Extended), 0);
    }

    #[test]
    fn context_class_parse_round_trip() {
        for cls in ContextClass::all() {
            let s = cls.short().to_lowercase();
            assert_eq!(ContextClass::parse(&s), Some(*cls));
        }
    }

    #[test]
    fn settings_new_derives_context_class() {
        let s = Settings::new("anthropic:claude-opus-4-7");
        assert_eq!(s.context_class, ContextClass::Massive);

        let s = Settings::new("openai:gpt-5.4");
        assert_eq!(s.context_class, ContextClass::Massive);
    }

    #[test]
    fn thinking_budget() {
        assert_eq!(ThinkingLevel::Off.budget_tokens(), None);
        assert_eq!(ThinkingLevel::Minimal.budget_tokens(), Some(2_000));
        assert_eq!(ThinkingLevel::High.budget_tokens(), Some(50_000));
    }

    #[test]
    fn profile_serializes_cleanly() {
        let p = Profile {
            last_used_model: Some(ProfileModel {
                provider: "anthropic".into(),
                model_id: "claude-opus-4-7".into(),
            }),
            thinking_level: Some("high".into()),
            max_turns: Some(50),
            provider_order: vec!["anthropic".into(), "openai".into()],
            fallback_providers: vec!["ollama".into()],
            embed_url: None,
            embed_model: None,
            ..Profile::default()
        };
        let json = serde_json::to_string_pretty(&p).unwrap();
        let parsed: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider_order, vec!["anthropic", "openai"]);
        assert_eq!(parsed.fallback_providers, vec!["ollama"]);
    }

    #[test]
    fn old_profile_deserializes_cleanly() {
        // Old profile without new fields — should deserialize without error
        let json = r#"{"lastUsedModel": {"provider": "anthropic", "modelId": "claude-sonnet-4-6"}, "thinkingLevel": "medium"}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert!(p.provider_order.is_empty());
        assert!(p.fallback_providers.is_empty());
        assert!(p.integrations.is_empty());
        assert!(p.extensions.is_empty());
    }

    #[test]
    fn profile_integration_and_extension_policy_round_trip() {
        let json = r#"{
            "integrations": {
                "mqtt": {
                    "enabled": true,
                    "brokerHost": "mqtt.internal",
                    "brokerPort": 1884
                }
            },
            "extensions": {
                "enabled": ["scry"],
                "disabled": ["vox"]
            }
        }"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert_eq!(p.integrations.mqtt.enabled, Some(true));
        assert_eq!(
            p.integrations.mqtt.broker_host.as_deref(),
            Some("mqtt.internal")
        );
        assert_eq!(p.integrations.mqtt.broker_port, Some(1884));
        assert!(p.extensions.permits("scry", &[], &[]));
        assert!(!p.extensions.permits("vox", &[], &[]));
        assert!(!p.extensions.permits("lipstyk", &[], &[]));
    }

    #[test]
    fn profile_permissions_prefer_unified_surface_with_legacy_fallback() {
        let legacy_json = r#"{"trustedDirectories":["/legacy"]}"#;
        let p: Profile = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(p.effective_trusted_directories(), vec!["/legacy"]);

        let unified_json = r#"{
            "permissions": {"trustedDirectories":["/unified"]},
            "trustedDirectories":["/legacy"]
        }"#;
        let p: Profile = serde_json::from_str(unified_json).unwrap();
        assert_eq!(
            p.effective_trusted_directories(),
            vec!["/unified", "/legacy"]
        );
    }

    #[test]
    fn profile_automation_applies_to_settings() {
        let profile: Profile = serde_json::from_str(r#"{"automation":{"level":"flow"}}"#).unwrap();
        let mut settings = Settings::default();
        profile.apply_to(&mut settings);
        assert_eq!(settings.automation_level, AutomationLevel::Flow);

        let mut captured = Profile::default();
        settings.automation_level = AutomationLevel::Autonomous;
        captured.capture_from(&settings);
        assert_eq!(captured.automation.level, Some(AutomationLevel::Autonomous));

        let mut default_capture = Profile::default();
        default_capture.capture_from(&Settings::default());
        assert_eq!(default_capture.automation.level, None);

        let mut explicit_default_capture =
            serde_json::from_str::<Profile>(r#"{"automation":{"level":"guarded"}}"#).unwrap();
        explicit_default_capture.capture_from(&Settings::default());
        assert_eq!(
            explicit_default_capture.automation.level,
            Some(AutomationLevel::Guarded)
        );
    }

    #[test]
    fn profile_terminal_tool_applies_and_only_captures_false() {
        let profile: Profile = serde_json::from_str(r#"{"terminalTool":false}"#).unwrap();
        let mut settings = Settings::default();
        profile.apply_to(&mut settings);
        assert!(!settings.terminal_tool);

        let mut captured = Profile::default();
        captured.capture_from(&settings);
        assert_eq!(captured.terminal_tool, Some(false));

        settings.terminal_tool = true;
        let mut default_capture = Profile::default();
        default_capture.capture_from(&settings);
        assert_eq!(default_capture.terminal_tool, None);

        let mut restored_capture = Profile {
            terminal_tool: Some(false),
            ..Profile::default()
        };
        restored_capture.capture_from(&settings);
        assert_eq!(restored_capture.terminal_tool, None);

        let json = serde_json::to_value(Profile::default()).unwrap();
        assert!(json.get("terminalTool").is_none());
    }

    #[test]
    fn automation_level_parse_accepts_aliases() {
        assert_eq!(AutomationLevel::parse("ask"), Some(AutomationLevel::Ask));
        assert_eq!(
            AutomationLevel::parse("default"),
            Some(AutomationLevel::Guarded)
        );
        assert_eq!(
            AutomationLevel::parse("proceed"),
            Some(AutomationLevel::Flow)
        );
        assert_eq!(
            AutomationLevel::parse("auto"),
            Some(AutomationLevel::Autonomous)
        );
        assert_eq!(AutomationLevel::parse("bogus"), None);
    }

    #[test]
    fn profile_permission_save_migrates_legacy_trusted_directories() {
        let mut p = Profile {
            trusted_directories: vec!["/legacy".into()],
            ..Profile::default()
        };
        p.add_trusted_directory("/unified".into());

        let json = serde_json::to_string_pretty(&p).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("permissions").is_some());
        assert!(value.get("trustedDirectories").is_none());
        assert_eq!(
            p.effective_trusted_directories(),
            vec!["/legacy", "/unified"]
        );
    }

    #[test]
    fn env_extension_policy_overrides_profile_allow_list() {
        let policy = ProfileExtensions {
            enabled: vec!["scry".into()],
            disabled: vec!["vox".into()],
        };
        assert!(policy.permits("lipstyk", &["lipstyk".into()], &[]));
        assert!(!policy.permits("scry", &["lipstyk".into()], &[]));
        assert!(!policy.permits("vox", &["vox".into()], &[]));
        assert!(!policy.permits("scry", &["scry".into()], &["scry".into()]));
    }

    #[test]
    fn profile_registry_discovers_project_and_user_entries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon/profiles")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profiles/build.json"),
            r#"{"name":"build","thinkingLevel":"low"}"#,
        )
        .unwrap();

        let registry = ProfileRegistry::discover(tmp.path());

        let project = registry
            .entries
            .iter()
            .find(|entry| entry.id == "build" && entry.scope == ProfileRegistryScope::Project)
            .expect("project profile registry entry");
        assert_eq!(project.source_kind, ProfileRegistrySourceKind::RegistryFile);
        assert!(project.editable);
        assert!(project.portable);
        assert_eq!(project.profile.name.as_deref(), Some("build"));
        assert!(registry.entries.iter().any(|entry| {
            entry.id == "built-in-default" && entry.scope == ProfileRegistryScope::BuiltIn
        }));
    }

    #[test]
    fn project_scope_profile_switch_writes_pointer_beside_profiles_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let profiles_dir = tmp.path().join(".omegon/profiles");
        std::fs::create_dir_all(&profiles_dir).expect("create profiles dir");
        let path = profiles_dir.join("project-default.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&Profile::default()).expect("serialize profile"),
        )
        .expect("write profile");
        let registry = ProfileRegistry {
            entries: vec![ProfileRegistryEntry {
                id: "project-default".into(),
                scope: ProfileRegistryScope::Project,
                source_kind: ProfileRegistrySourceKind::RegistryFile,
                path: Some(path.clone()),
                profile: Profile::default(),
                editable: true,
                shadows: Vec::new(),
                portable: true,
            }],
        };

        let pointer_path = save_project_active_profile_selection(
            tmp.path(),
            &ActiveProfileSelection {
                id: "project-default".into(),
                scope: Some("project".into()),
            },
        )
        .expect("save project profile selection");

        let switched = registry.resolve_active();
        assert_eq!(switched.source, ProfileSource::Project(path));
        assert_eq!(
            std::fs::canonicalize(&pointer_path).expect("canonical pointer path"),
            std::fs::canonicalize(tmp.path().join(".omegon/active-profile.json"))
                .expect("canonical expected pointer path")
        );
        let selection: ActiveProfileSelection = serde_json::from_slice(
            &std::fs::read(&pointer_path).expect("read active profile pointer"),
        )
        .expect("parse active profile pointer");
        assert_eq!(selection.id, "project-default");
        assert_eq!(selection.scope.as_deref(), Some("project"));
        assert!(!tmp.path().join("active-profile.json").exists());
    }

    #[test]
    fn active_profile_selection_save_rejects_missing_entry_without_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let path = tmp.path().join(".omegon/active-profile.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"id":"default","scope":"user"}"#).unwrap();
        let selection = ActiveProfileSelection {
            id: "missing".into(),
            scope: Some("project".into()),
        };

        let error = save_project_active_profile_selection(tmp.path(), &selection)
            .expect_err("missing profile must be rejected");

        assert!(
            error
                .to_string()
                .contains("profile `missing` was not found")
        );
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            r#"{"id":"default","scope":"user"}"#
        );
    }

    #[test]
    fn active_profile_selection_prefers_project_registry_entry() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon/profiles")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profiles/build.json"),
            r#"{"name":"build","thinkingLevel":"high"}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"name":"legacy","thinkingLevel":"low"}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".omegon/active-profile.json"),
            r#"{"id":"build","scope":"project"}"#,
        )
        .unwrap();

        let loaded = Profile::load_with_source(tmp.path());

        assert_eq!(loaded.profile.name.as_deref(), Some("build"));
        assert_eq!(loaded.profile.thinking_level.as_deref(), Some("high"));
        assert!(
            matches!(loaded.source, ProfileSource::Project(path) if path.ends_with(".omegon/profiles/build.json"))
        );
    }

    #[test]
    fn active_profile_selection_uses_registry_id_when_file_has_no_embedded_name() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon/profiles")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profiles/pig.json"),
            r#"{"thinkingLevel":"high"}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".omegon/active-profile.json"),
            r#"{"id":"pig","scope":"project"}"#,
        )
        .unwrap();

        let loaded = Profile::load_with_source(tmp.path());

        assert_eq!(loaded.profile.name.as_deref(), Some("pig"));
        assert_eq!(loaded.profile.thinking_level.as_deref(), Some("high"));
        assert!(
            matches!(loaded.source, ProfileSource::Project(path) if path.ends_with(".omegon/profiles/pig.json"))
        );
    }

    #[test]
    fn profile_registry_falls_back_to_legacy_project_profile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git/.keep")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".omegon")).unwrap();
        std::fs::write(
            tmp.path().join(".omegon/profile.json"),
            r#"{"name":"legacy-project"}"#,
        )
        .unwrap();

        let loaded = Profile::load_with_source(tmp.path());

        assert_eq!(loaded.profile.name.as_deref(), Some("legacy-project"));
        assert!(
            matches!(loaded.source, ProfileSource::Project(path) if path.ends_with(".omegon/profile.json"))
        );
    }

    #[test]
    fn project_profile_path_resolves_to_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("core/crates/omegon");
        std::fs::create_dir_all(&nested).unwrap();
        let root = tmp.path().canonicalize().unwrap();

        assert_eq!(
            project_profile_path(&nested),
            root.join(".omegon/profile.json")
        );
    }

    #[test]
    fn profile_save_uses_repo_root_not_nested_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("core/crates/omegon");
        std::fs::create_dir_all(&nested).unwrap();

        let profile = Profile {
            last_used_model: Some(ProfileModel {
                provider: "anthropic".into(),
                model_id: "claude-sonnet-4-6".into(),
            }),
            thinking_level: Some("low".into()),
            max_turns: Some(50),
            provider_order: Vec::new(),
            embed_url: None,
            embed_model: None,
            ..Profile::default()
        };

        profile.save(&nested).unwrap();

        assert!(tmp.path().join(".omegon/profile.json").exists());
        assert!(!nested.join(".omegon/profile.json").exists());
    }

    #[test]
    fn profile_requested_context_class_round_trips_and_applies_as_policy() {
        let profile: Profile =
            serde_json::from_str(r#"{"requestedContextClass":"massive"}"#).unwrap();
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["requestedContextClass"], "massive");

        let mut settings = Settings::new("anthropic:claude-sonnet-4-6");
        settings.context_class = ContextClass::Massive;
        settings.context_window = ContextClass::Massive.nominal_tokens();
        profile.apply_to(&mut settings);

        assert_eq!(
            settings.requested_context_class,
            Some(ContextClass::Massive)
        );
        assert_eq!(settings.context_class, ContextClass::Massive);
        assert_eq!(
            settings.context_window,
            ContextClass::Massive.nominal_tokens()
        );
    }

    #[test]
    fn profile_capture_records_requested_context_class() {
        let mut settings = Settings::new("anthropic:claude-sonnet-4-6");
        settings.set_requested_context_class(ContextClass::Extended);

        let mut profile = Profile::default();
        profile.capture_from(&settings);

        assert_eq!(profile.requested_context_class.as_deref(), Some("extended"));
    }

    #[test]
    fn profile_name_round_trips_and_sets_compact_runtime_label() {
        let profile: Profile =
            serde_json::from_str(r#"{"name":"minimal","displayName":"Minimal project profile"}"#)
                .unwrap();
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["name"], "minimal");
        assert_eq!(json["displayName"], "Minimal project profile");
        assert_eq!(profile.compact_label(), Some("minimal"));

        let mut settings = Settings::new("anthropic:claude-sonnet-4-6");
        profile.apply_to(&mut settings);
        assert_eq!(settings.profile_name.as_deref(), Some("minimal"));
    }

    #[test]
    fn profile_compact_label_falls_back_to_display_name() {
        let profile: Profile =
            serde_json::from_str(r#"{"name":"   ","displayName":"Team defaults"}"#).unwrap();

        assert_eq!(profile.compact_label(), Some("Team defaults"));
    }

    #[test]
    fn profile_load_with_source_reports_project_user_and_default_sources() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let project_profile = tmp
            .path()
            .canonicalize()
            .unwrap()
            .join(".omegon/profile.json");
        std::fs::create_dir_all(project_profile.parent().unwrap()).unwrap();
        std::fs::write(&project_profile, r#"{"thinkingLevel":"high"}"#).unwrap();

        let loaded = Profile::load_with_source(tmp.path());
        assert!(
            matches!(loaded.source, ProfileSource::Project(ref path) if path == &project_profile)
        );
        assert_eq!(loaded.profile.thinking_level.as_deref(), Some("high"));

        std::fs::remove_file(&project_profile).unwrap();
        let loaded = Profile::load_with_source(tmp.path());
        assert!(matches!(
            loaded.source,
            ProfileSource::User(_) | ProfileSource::BuiltInDefault
        ));
    }

    #[test]
    fn profile_save_to_active_source_refuses_builtin_default_source() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let profile = Profile::default();

        let err = profile
            .save_to_target(
                tmp.path(),
                ProfileSaveTarget::ActiveSource,
                &ProfileSource::BuiltInDefault,
            )
            .unwrap_err();

        assert!(err.to_string().contains("ambiguous"));
        assert!(!tmp.path().join(".omegon/profile.json").exists());
    }

    #[test]
    fn profile_save_to_active_project_source_writes_project_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let project_profile = tmp.path().join(".omegon/profile.json");
        let profile = Profile {
            thinking_level: Some("high".into()),
            ..Profile::default()
        };

        let saved_source = profile
            .save_to_target(
                tmp.path(),
                ProfileSaveTarget::ActiveSource,
                &ProfileSource::Project(project_profile.clone()),
            )
            .unwrap();

        assert!(
            matches!(saved_source, ProfileSource::Project(ref path) if path == &project_profile)
        );
        let saved: Profile =
            serde_json::from_str(&std::fs::read_to_string(project_profile).unwrap()).unwrap();
        assert_eq!(saved.thinking_level.as_deref(), Some("high"));
    }

    // ─── Regression: posture ↔ slim equivalence ─────────────────────────

    #[test]
    fn is_slim_true_only_for_explorator() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        assert!(
            !s.is_slim(),
            "default posture (Architect) should not be slim"
        );

        s.set_posture(PosturePreset::Explorator);
        assert!(s.is_slim(), "Explorator posture should be slim");

        s.set_posture(PosturePreset::Fabricator);
        assert!(!s.is_slim(), "Fabricator should not be slim");

        s.set_posture(PosturePreset::Devastator);
        assert!(!s.is_slim(), "Devastator should not be slim");

        s.set_posture(PosturePreset::Architect);
        assert!(!s.is_slim(), "Architect should not be slim");
    }

    #[test]
    fn set_posture_applies_correct_resource_envelope() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");

        s.set_posture(PosturePreset::Explorator);
        assert_eq!(s.thinking, ThinkingLevel::Minimal);
        assert_eq!(s.requested_context_class, Some(ContextClass::Compact));

        s.set_posture(PosturePreset::Fabricator);
        assert_eq!(s.thinking, ThinkingLevel::Low);
        assert_eq!(s.requested_context_class, Some(ContextClass::Standard));

        s.set_posture(PosturePreset::Architect);
        assert_eq!(s.thinking, ThinkingLevel::Medium);
        assert_eq!(s.requested_context_class, Some(ContextClass::Extended));

        s.set_posture(PosturePreset::Devastator);
        assert_eq!(s.thinking, ThinkingLevel::High);
        assert_eq!(s.requested_context_class, Some(ContextClass::Massive));
    }

    #[test]
    fn resource_envelope_uses_posture_directly() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");

        s.set_posture(PosturePreset::Explorator);
        let env = s.resource_envelope();
        assert_eq!(env.thinking, ThinkingLevel::Minimal);
        assert_eq!(env.requested_context_class, ContextClass::Compact);

        s.set_posture(PosturePreset::Devastator);
        let env = s.resource_envelope();
        assert_eq!(env.thinking, ThinkingLevel::High);
        assert_eq!(env.requested_context_class, ContextClass::Massive);
    }

    #[test]
    fn selector_policy_respects_explorator_context_class() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        s.set_posture(PosturePreset::Explorator);
        let policy = s.selector_policy();
        assert_eq!(
            policy.requested_class,
            ContextClass::Compact,
            "Explorator should use Compact context class in selector policy"
        );
    }

    #[test]
    fn selector_policy_constrains_assembly_to_requested_lower_class() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        assert_eq!(s.context_window, 1_000_000);
        assert_eq!(s.context_class, ContextClass::Massive);

        s.set_requested_context_class(ContextClass::Compact);

        let policy = s.selector_policy();

        assert_eq!(policy.model_window, 1_000_000);
        assert_eq!(policy.actual_class(), ContextClass::Massive);
        assert_eq!(policy.requested_class, ContextClass::Compact);
        assert_eq!(
            policy.assembly_window(),
            ContextClass::Compact.nominal_tokens()
        );
        assert!(policy.assembly_budget() < 200_000);
        assert!(!policy.has_class_mismatch());
    }

    #[test]
    fn selector_policy_caps_requested_higher_class_at_model_capacity() {
        let mut s = Settings::new("anthropic:claude-haiku-4-5");
        assert_eq!(s.context_class, ContextClass::Standard);
        s.set_requested_context_class(ContextClass::Massive);

        let policy = s.selector_policy();

        assert_eq!(policy.model_window, 200_000);
        assert_eq!(policy.requested_class, ContextClass::Massive);
        assert_eq!(policy.assembly_window(), 200_000);
        assert!(policy.has_class_mismatch());
    }
    #[test]
    fn profile_model_intent_round_trips_route_intent() {
        let intent = crate::route::ModelIntent {
            grade: Some(crate::route::ModelGrade::S),
            provider_selection: crate::route::ProviderSelection::Local,
            grade_policy: crate::route::GradePolicy::Exact,
            provider_policy: Some(crate::semantic_route::ProviderPolicy::CopilotOnly),
            exact_model_override: None,
            ..crate::route::ModelIntent::default()
        };
        let profile = ProfileModelIntent::from_route_intent(&intent);
        assert_eq!(profile.grade.as_deref(), Some("S"));
        assert_eq!(profile.provider.as_deref(), Some("local"));
        assert_eq!(profile.grade_policy.as_deref(), Some("exact"));
        assert_eq!(profile.provider_policy.as_deref(), Some("copilot-only"));
        assert_eq!(profile.to_route_intent(), Some(intent));
    }

    #[test]
    fn profile_model_intent_rejects_local_as_grade() {
        let profile = ProfileModelIntent {
            grade: Some("local".into()),
            provider: Some("auto".into()),
            grade_policy: Some("minimum".into()),
            provider_policy: None,
            exact_model_override: None,
        };
        assert_eq!(profile.to_route_intent(), None);
    }
}
