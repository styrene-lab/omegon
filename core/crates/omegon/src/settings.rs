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
//! - **Capability tier**: local / retribution / victory / gloriana
//! - **Thinking level**: off / minimal / low / medium / high
//! - **Context class**: Squad (128k) / Maniple (272k) / Clan (400k) / Legion (1M+)

use serde::{Deserialize, Serialize};
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
    /// Model-tier unification remains in `model_budget.rs` for now.
    pub fn default_resource_envelope(self) -> ResourceEnvelope {
        match self {
            Self::Explorator => ResourceEnvelope {
                thinking: ThinkingLevel::Minimal,
                requested_context_class: ContextClass::Squad,
                effective_context_cap_tokens: Some(ContextClass::Squad.nominal_tokens()),
                compact_reply_reserve: true,
                compact_tool_schema_reserve: true,
            },
            Self::Fabricator => ResourceEnvelope {
                thinking: ThinkingLevel::Low,
                requested_context_class: ContextClass::Maniple,
                effective_context_cap_tokens: None,
                compact_reply_reserve: false,
                compact_tool_schema_reserve: false,
            },
            Self::Architect => ResourceEnvelope {
                thinking: ThinkingLevel::Medium,
                requested_context_class: ContextClass::Clan,
                effective_context_cap_tokens: None,
                compact_reply_reserve: false,
                compact_tool_schema_reserve: false,
            },
            Self::Devastator => ResourceEnvelope {
                thinking: ThinkingLevel::High,
                requested_context_class: ContextClass::Legion,
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

    /// Slim runtime mode — reduce prompt and tool surface for quick interactive work.
    #[serde(default)]
    pub slim_mode: bool,

    /// Behavioral posture for the current session/runtime.
    #[serde(default)]
    pub posture: BehavioralPosture,

    /// Thinking level: off, minimal, low, medium, high.
    pub thinking: ThinkingLevel,

    /// Maximum turns per agent invocation. 0 = no limit.
    pub max_turns: u32,

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

    /// Extended context mode — legacy Anthropic 200k/1M toggle.
    /// Deprecated: derived from context_class. Kept for backward compat.
    pub context_mode: ContextMode,

    /// Tool display detail level.
    pub tool_detail: ToolDetail,

    /// Provider preference order for routing. First = most preferred.
    #[serde(default)]
    pub provider_order: Vec<String>,

    /// Update channel for in-app self-update.
    #[serde(default = "default_update_channel")]
    pub update_channel: String,

    /// Whether a live LLM provider is connected. Set to false when NullBridge
    /// is active (no credentials available). The TUI uses this to show
    /// "no provider" instead of a model name that can't actually be used.
    #[serde(skip)]
    pub provider_connected: bool,

    /// Enable mouse capture (pane clicks, wheel scroll, segment targeting).
    /// Defaults to true. Set to false to restore terminal-native text selection.
    #[serde(default = "default_mouse")]
    pub mouse: bool,

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

/// Tool card display mode in the conversation view.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolDetail {
    /// Single-line cards with truncated args + result preview.
    Compact,
    /// Bordered cards showing full command + output (first 8 lines).
    #[default]
    Detailed,
}

impl ToolDetail {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Detailed => "detailed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "compact" | "c" => Some(Self::Compact),
            "detailed" | "detail" | "d" | "verbose" | "v" => Some(Self::Detailed),
            _ => None,
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
    Squad,
    /// 272k tokens. Standard working context.
    Maniple,
    /// 400k tokens. Extended context for large codebases.
    Clan,
    /// 1M+ tokens. Full context for massive sessions.
    Legion,
}

/// Token ceiling thresholds — a model with ceiling ≤ threshold belongs to that class.
const CONTEXT_CLASS_THRESHOLDS: &[(ContextClass, usize)] = &[
    (ContextClass::Squad, 131_072),   // 128k
    (ContextClass::Maniple, 278_528), // ~272k
    (ContextClass::Clan, 450_560),    // ~440k (covers 400k models)
                                      // Legion: everything above
];

impl ContextClass {
    /// Classify a raw token count into a context class.
    pub fn from_tokens(tokens: usize) -> Self {
        for &(class, threshold) in CONTEXT_CLASS_THRESHOLDS {
            if tokens <= threshold {
                return class;
            }
        }
        Self::Legion
    }

    /// Nominal token count for this class.
    pub fn nominal_tokens(self) -> usize {
        match self {
            Self::Squad => 131_072,
            Self::Maniple => 278_528,
            Self::Clan => 409_600,
            Self::Legion => 1_048_576,
        }
    }

    /// Operator-facing display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Squad => "Squad (128k)",
            Self::Maniple => "Maniple (272k)",
            Self::Clan => "Clan (400k)",
            Self::Legion => "Legion (1M)",
        }
    }

    /// Short name for dashboard badges.
    pub fn short(self) -> &'static str {
        match self {
            Self::Squad => "Squad",
            Self::Maniple => "Maniple",
            Self::Clan => "Clan",
            Self::Legion => "Legion",
        }
    }

    /// Ordinal for comparison and delta calculation.
    pub fn ordinal(self) -> u8 {
        match self {
            Self::Squad => 0,
            Self::Maniple => 1,
            Self::Clan => 2,
            Self::Legion => 3,
        }
    }

    /// Delta between two classes. Positive = downgrade (self > other).
    pub fn delta(self, other: Self) -> i8 {
        self.ordinal() as i8 - other.ordinal() as i8
    }

    /// Derive the legacy ContextMode from this class.
    /// All classes map to Standard now — Extended was a beta flag concept.
    pub fn context_mode(self) -> ContextMode {
        ContextMode::Standard
    }

    /// All classes in ascending order.
    pub fn all() -> &'static [Self] {
        &[Self::Squad, Self::Maniple, Self::Clan, Self::Legion]
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "squad" | "128k" => Some(Self::Squad),
            "maniple" | "272k" => Some(Self::Maniple),
            "clan" | "400k" => Some(Self::Clan),
            "legion" | "1m" => Some(Self::Legion),
            _ => None,
        }
    }
}

// ─── Context Mode (legacy, derived from ContextClass) ───────────────────────

/// Context window mode for providers that support multiple sizes.
/// **Deprecated**: use `ContextClass` instead. Kept for Anthropic beta header derivation
/// and backward compatibility with existing profile.json files.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextMode {
    /// Standard context window — model-native (200K–1M depending on model).
    /// Sonnet/Opus 4.6 support 1M natively, no beta flag needed.
    #[default]
    Standard,
    /// Legacy: was "Extended 1M via beta flag". The beta flag is now
    /// deprecated — it only triggers billing gates. Kept for config compat.
    Extended,
}

impl ContextMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            // Both modes use the model's native context window now
            Self::Standard | Self::Extended => "native",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "standard" | "200k" | "default" | "native" => Some(Self::Standard),
            "extended" | "1m" | "1M" | "million" => Some(Self::Standard), // silently normalize
            _ => None,
        }
    }

    pub fn icon(&self) -> &'static str {
        "◆" // always native full context
    }
}

fn default_update_channel() -> String {
    "stable".to_string()
}

fn default_mouse() -> bool {
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
    /// Actual token budget available for context assembly this turn.
    pub fn assembly_budget(&self) -> usize {
        self.model_window
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
            slim_mode: false,
            posture: BehavioralPosture::fixed(PosturePreset::Architect),
            thinking: ThinkingLevel::Medium,
            max_turns: 50,
            compaction_threshold: 0.75,
            context_window,
            context_class: ContextClass::from_tokens(context_window),
            requested_context_class: None,
            context_mode: ContextMode::Standard,
            tool_detail: ToolDetail::Detailed,
            provider_order: Vec::new(),
            update_channel: default_update_channel(),
            provider_connected: true, // optimistic default — set false when NullBridge
            mouse: true,
            clipboard_retention_hours: default_clipboard_retention_hours(),
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
            context_mode: context_class.context_mode(),
            ..Default::default()
        }
    }

    /// Recalculate context_window and context_class based on current model.
    /// Context window is always the model's native capability — no beta flags.
    pub fn apply_context_mode(&mut self) {
        self.context_window = infer_context_window(&self.model);
        self.context_class = ContextClass::from_tokens(self.context_window);
    }

    /// Update model and recalculate derived fields.
    pub fn set_model(&mut self, model: &str) {
        self.model = model.to_string();
        self.context_window = infer_context_window(model);
        self.context_class = ContextClass::from_tokens(self.context_window);
        self.context_mode = self.context_class.context_mode();
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
        if self.slim_mode {
            PosturePreset::Explorator.default_resource_envelope()
        } else {
            self.posture.effective.default_resource_envelope()
        }
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
        self.slim_mode = matches!(preset, PosturePreset::Explorator);

        let envelope = self.resource_envelope();
        self.thinking = envelope.thinking;
        self.requested_context_class = Some(envelope.requested_context_class);
    }

    pub fn set_slim_mode(&mut self, slim: bool) {
        self.slim_mode = slim;
        if slim {
            self.posture = BehavioralPosture::fixed(PosturePreset::Explorator);
            let envelope = self.resource_envelope();
            self.thinking = envelope.thinking;
            if self.requested_context_class.is_none() {
                self.requested_context_class = Some(envelope.requested_context_class);
            }
        }
    }

    /// Derive a SelectorPolicy for this turn's context assembly.
    pub fn selector_policy(&self) -> SelectorPolicy {
        let envelope = self.resource_envelope();
        let thinking_reserve = self.thinking.budget_tokens().unwrap_or(0) as usize;
        let model_window = envelope
            .effective_context_cap_tokens
            .map(|cap| self.context_window.min(cap))
            .unwrap_or(self.context_window);
        let requested_class = if self.slim_mode {
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

    pub fn provider(&self) -> &str {
        match crate::providers::infer_provider_id(&self.model).as_str() {
            "anthropic" => "anthropic",
            "openai" => "openai",
            "openai-codex" => "openai-codex",
            "openrouter" => "openrouter",
            "groq" => "groq",
            "xai" => "xai",
            "mistral" => "mistral",
            "cerebras" => "cerebras",
            "huggingface" => "huggingface",
            "ollama" => "ollama",
            "ollama-cloud" => "ollama-cloud",
            _ => "anthropic",
        }
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

/// Reviewed route matrix — embedded from the same JSON the TS side loads.
/// Updated by the Argo refresh pipeline, checked in, compiled into the binary.
const ROUTE_MATRIX_JSON: &str = include_str!("../../../../data/route-matrix.json");

/// Parsed route entry from the embedded matrix.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteEntry {
    provider: String,
    model_id_pattern: String,
    context_ceiling: usize,
}

#[derive(Debug, Deserialize)]
struct RouteMatrix {
    routes: Vec<RouteEntry>,
}

/// Lazy-parsed route matrix.
fn route_matrix() -> &'static [RouteEntry] {
    use std::sync::OnceLock;
    static MATRIX: OnceLock<Vec<RouteEntry>> = OnceLock::new();
    MATRIX.get_or_init(|| {
        serde_json::from_str::<RouteMatrix>(ROUTE_MATRIX_JSON)
            .map(|m| m.routes)
            .unwrap_or_default()
    })
}

/// Match a model ID against the route matrix using glob-style patterns.
fn lookup_context_ceiling(provider: &str, model_id: &str) -> Option<usize> {
    route_matrix()
        .iter()
        .filter(|entry| entry.provider == provider)
        .filter_map(|entry| {
            let pattern = &entry.model_id_pattern;
            let matches = if let Some(prefix) = pattern.strip_suffix('*') {
                model_id.starts_with(prefix)
            } else {
                model_id == pattern
            };
            matches.then_some((pattern.trim_end_matches('*').len(), entry.context_ceiling))
        })
        .max_by_key(|(specificity, _)| *specificity)
        .map(|(_, ceiling)| ceiling)
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
    "huggingface",
    "openrouter",
    "ollama",
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

    // Try route matrix lookup
    if let Some(ceiling) = lookup_context_ceiling(provider, model_id) {
        return ceiling;
    }

    // Fallback heuristics for models not in the matrix
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

    131_072 // fail-closed: default to Squad for unknown cloud providers
}

/// Thread-safe shared settings handle.
pub type SharedSettings = Arc<Mutex<Settings>>;

pub fn shared(model: &str) -> SharedSettings {
    Arc::new(Mutex::new(Settings::new(model)))
}

// ─── Profile persistence ────────────────────────────────────────────────────

/// Profile: settings that persist with the project in .omegon/profile.json.
/// Read on startup, written on change. Travels with git.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_model: Option<ProfileModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,

    // ── Context class routing ──
    /// Provider preference order. First = most preferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_order: Vec<String>,
    /// Providers to skip unless no alternative exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub avoid_providers: Vec<String>,
    /// Pinned context floor — minimum context class the session should maintain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_floor_pin: Option<String>,
    /// Durable downgrade overrides — accepted transitions that won't prompt again.
    /// Format: "Legion→Squad" (from_class→to_class).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downgrade_overrides: Vec<String>,

    // ── Embedding service (hybrid search) ──
    /// Embedding service base URL (Ollama `/api/embed` endpoint).
    /// Overrides `OMEGON_EMBED_URL` env var. Default: `http://localhost:11434`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_url: Option<String>,
    /// Embedding model name (e.g. `nomic-embed-text`).
    /// Overrides `OMEGON_EMBED_MODEL` env var.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embed_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModel {
    pub provider: String,
    pub model_id: String,
}

impl Profile {
    /// Load profile. Project-level (`<repo>/.omegon/profile.json`) overrides
    /// user-level (`~/.omegon/profile.json`). Both are optional.
    pub fn load(cwd: &std::path::Path) -> Self {
        let project_path = project_profile_path(cwd);
        if let Ok(content) = std::fs::read_to_string(&project_path)
            && let Ok(profile) = serde_json::from_str(&content)
        {
            tracing::debug!(path = %project_path.display(), "project profile loaded");
            return profile;
        }

        // User-level fallback
        if let Some(global_path) = global_profile_path()
            && let Ok(content) = std::fs::read_to_string(&global_path)
            && let Ok(profile) = serde_json::from_str(&content)
        {
            tracing::debug!(path = %global_path.display(), "global profile loaded");
            return profile;
        }

        Self {
            last_used_model: None,
            thinking_level: None,
            max_turns: None,
            provider_order: Vec::new(),
            avoid_providers: Vec::new(),
            context_floor_pin: None,
            downgrade_overrides: Vec::new(),
            embed_url: None,
            embed_model: None,
        }
    }

    /// Save to the project-level profile at the repository root.
    pub fn save(&self, cwd: &std::path::Path) -> anyhow::Result<()> {
        let path = project_profile_path(cwd);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        tracing::debug!(path = %path.display(), "project profile saved");
        Ok(())
    }

    /// Save to the user-level profile (~/.omegon/profile.json).
    pub fn save_global(&self) -> anyhow::Result<()> {
        let path = global_profile_path()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        tracing::debug!(path = %path.display(), "global profile saved");
        Ok(())
    }

    /// Apply profile to settings (called at startup).
    pub fn apply_to(&self, settings: &mut Settings) {
        if let Some(ref m) = self.last_used_model {
            settings.set_model(&format!("{}:{}", m.provider, m.model_id));
        }
        if let Some(ref t) = self.thinking_level
            && let Some(level) = ThinkingLevel::parse(t)
        {
            settings.thinking = level;
        }
        if let Some(turns) = self.max_turns {
            settings.max_turns = turns;
        }
        if !self.provider_order.is_empty() {
            settings.provider_order = self.provider_order.clone();
        }
    }

    /// Capture current settings into the profile (called on change).
    pub fn capture_from(&mut self, settings: &Settings) {
        self.last_used_model = Some(ProfileModel {
            provider: settings.provider().to_string(),
            model_id: settings.model_short().to_string(),
        });
        self.thinking_level = Some(settings.thinking.as_str().to_string());
        self.max_turns = Some(settings.max_turns);
        if !settings.provider_order.is_empty() {
            self.provider_order = settings.provider_order.clone();
        }
    }

    /// Check if a downgrade transition has been accepted durably.
    pub fn is_downgrade_accepted(&self, from: ContextClass, to: ContextClass) -> bool {
        let key = format!("{}→{}", from.short(), to.short());
        self.downgrade_overrides.contains(&key)
    }

    /// Accept a downgrade transition durably.
    pub fn accept_downgrade(&mut self, from: ContextClass, to: ContextClass) {
        let key = format!("{}→{}", from.short(), to.short());
        if !self.downgrade_overrides.contains(&key) {
            self.downgrade_overrides.push(key);
        }
    }

    /// Get the pinned context floor, if set.
    pub fn pinned_floor(&self) -> Option<ContextClass> {
        self.context_floor_pin
            .as_deref()
            .and_then(ContextClass::parse)
    }

    /// Pin the context floor.
    pub fn pin_floor(&mut self, class: ContextClass) {
        self.context_floor_pin = Some(class.short().to_string());
    }
}

fn project_profile_path(cwd: &std::path::Path) -> std::path::PathBuf {
    crate::setup::find_project_root(cwd).join(".omegon/profile.json")
}

fn global_profile_path() -> Option<std::path::PathBuf> {
    // Preferred user-level home follows the rest of the harness convention.
    // Fall back to the legacy XDG/App Support path for backward compatibility.
    dirs::home_dir()
        .map(|d| d.join(".omegon/profile.json"))
        .or_else(|| dirs::config_dir().map(|d| d.join("omegon/profile.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ContextClass::Clan
        );
    }

    #[test]
    fn posture_preset_resource_defaults_are_stable() {
        let explorator = PosturePreset::Explorator.default_resource_envelope();
        assert_eq!(explorator.thinking, ThinkingLevel::Minimal);
        assert_eq!(explorator.requested_context_class, ContextClass::Squad);
        assert_eq!(
            explorator.effective_context_cap_tokens,
            Some(ContextClass::Squad.nominal_tokens())
        );
        assert!(explorator.compact_reply_reserve);
        assert!(explorator.compact_tool_schema_reserve);

        let fabricator = PosturePreset::Fabricator.default_resource_envelope();
        assert_eq!(fabricator.thinking, ThinkingLevel::Low);
        assert_eq!(fabricator.requested_context_class, ContextClass::Maniple);

        let architect = PosturePreset::Architect.default_resource_envelope();
        assert_eq!(architect.thinking, ThinkingLevel::Medium);
        assert_eq!(architect.requested_context_class, ContextClass::Clan);

        let devastator = PosturePreset::Devastator.default_resource_envelope();
        assert_eq!(devastator.thinking, ThinkingLevel::High);
        assert_eq!(devastator.requested_context_class, ContextClass::Legion);
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
            ContextClass::Maniple
        );
        assert_eq!(profile.identity, RuntimeIdentity::local_interactive());
        assert_eq!(
            profile.authorization,
            AuthorizationContext::local_descriptive()
        );
        assert_eq!(profile.persona, PersonaState::default());
        assert_eq!(
            profile.summary(),
            "local-operator / Fabricator / low / Maniple / operator@local"
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
            "dev.styrene.omegon.systems-engineer / Architect / medium / Clan / operator@local"
        );
    }

    #[test]
    fn operating_profile_can_overlay_identity() {
        let profile = Settings::default()
            .operating_profile()
            .with_identity(RuntimeIdentity::local_control_plane());
        assert_eq!(
            profile.summary(),
            "daemon-supervisor / Architect / medium / Clan / operator@local"
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
        assert!(!s.slim_mode);
        assert_eq!(s.thinking, ThinkingLevel::Low);
        assert_eq!(s.requested_context_class, Some(ContextClass::Maniple));

        s.set_posture(PosturePreset::Devastator);
        assert_eq!(
            s.posture,
            BehavioralPosture::fixed(PosturePreset::Devastator)
        );
        assert_eq!(s.thinking, ThinkingLevel::High);
        assert_eq!(s.requested_context_class, Some(ContextClass::Legion));
    }

    #[test]
    fn slim_mode_reduces_defaults() {
        let mut s = Settings::new("anthropic:claude-sonnet-4-6");
        s.set_slim_mode(true);
        assert!(s.slim_mode);
        assert_eq!(s.thinking, ThinkingLevel::Minimal);
        assert_eq!(s.requested_context_class, Some(ContextClass::Squad));
        let policy = s.selector_policy();
        assert_eq!(policy.requested_class, ContextClass::Squad);
        assert_eq!(policy.model_window, ContextClass::Squad.nominal_tokens());
        assert!(policy.reply_reserve < 8_192);
        assert!(policy.tool_schema_reserve < 4_096);
    }

    #[test]
    fn settings_default() {
        let s = Settings::default();
        assert_eq!(s.thinking, ThinkingLevel::Medium);
        assert_eq!(s.context_window, 200_000);
        assert_eq!(s.context_class, ContextClass::Maniple);
    }

    #[test]
    fn model_short_extracts_name() {
        let s = Settings::new("anthropic:claude-opus-4-6");
        assert_eq!(s.model_short(), "claude-opus-4-6");
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
            humanize_model_id("anthropic:claude-opus-4-6"),
            "claude-opus-4-6"
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
        assert_eq!(humanize_model_id("claude-opus-4-6"), "claude-opus-4-6");
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
        assert_eq!(infer_context_window("anthropic:claude-opus-4-6"), 1_000_000);
        assert_eq!(
            infer_context_window("anthropic:claude-sonnet-4-6"),
            1_000_000
        );
        assert_eq!(infer_context_window("openai:gpt-5.4"), 272_000);
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
        assert_eq!(lookup_context_ceiling("openai", "gpt-5.4"), Some(272_000));
        assert_eq!(
            lookup_context_ceiling("openai", "gpt-5.4-mini"),
            Some(400_000)
        );
        assert_eq!(lookup_context_ceiling("ollama", "qwen3:32b"), None);
        assert_eq!(lookup_context_ceiling("openai", "unknown-model"), None);
    }

    #[test]
    fn context_window_fallback_heuristic() {
        // Unknown models fall back to Squad (fail-closed)
        assert_eq!(infer_context_window("mystery:unknown-model"), 131_072);
    }

    #[test]
    fn context_class_from_tokens() {
        assert_eq!(ContextClass::from_tokens(100_000), ContextClass::Squad);
        assert_eq!(ContextClass::from_tokens(131_072), ContextClass::Squad);
        assert_eq!(ContextClass::from_tokens(131_073), ContextClass::Maniple);
        assert_eq!(ContextClass::from_tokens(200_000), ContextClass::Maniple);
        assert_eq!(ContextClass::from_tokens(278_528), ContextClass::Maniple);
        assert_eq!(ContextClass::from_tokens(278_529), ContextClass::Clan);
        assert_eq!(ContextClass::from_tokens(400_000), ContextClass::Clan);
        assert_eq!(ContextClass::from_tokens(450_560), ContextClass::Clan);
        assert_eq!(ContextClass::from_tokens(450_561), ContextClass::Legion);
        assert_eq!(ContextClass::from_tokens(1_000_000), ContextClass::Legion);
    }

    #[test]
    fn context_class_ordering() {
        assert!(ContextClass::Squad < ContextClass::Maniple);
        assert!(ContextClass::Maniple < ContextClass::Clan);
        assert!(ContextClass::Clan < ContextClass::Legion);
    }

    #[test]
    fn context_class_delta() {
        assert_eq!(ContextClass::Legion.delta(ContextClass::Squad), 3);
        assert_eq!(ContextClass::Squad.delta(ContextClass::Legion), -3);
        assert_eq!(ContextClass::Clan.delta(ContextClass::Clan), 0);
    }

    #[test]
    fn context_class_derives_context_mode() {
        // All classes now return Standard — Extended was a deprecated beta flag concept.
        // Context window is the model's native capability, not a mode toggle.
        assert_eq!(ContextClass::Squad.context_mode(), ContextMode::Standard);
        assert_eq!(ContextClass::Maniple.context_mode(), ContextMode::Standard);
        assert_eq!(ContextClass::Clan.context_mode(), ContextMode::Standard);
        assert_eq!(ContextClass::Legion.context_mode(), ContextMode::Standard);
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
        let s = Settings::new("anthropic:claude-opus-4-6");
        assert_eq!(s.context_class, ContextClass::Legion);

        let s = Settings::new("openai:gpt-5.4");
        assert_eq!(s.context_class, ContextClass::Maniple);
    }

    #[test]
    fn profile_downgrade_overrides() {
        let mut p = Profile {
            last_used_model: None,
            thinking_level: None,
            max_turns: None,
            provider_order: Vec::new(),
            avoid_providers: Vec::new(),
            context_floor_pin: None,
            downgrade_overrides: Vec::new(),
            embed_url: None,
            embed_model: None,
        };
        assert!(!p.is_downgrade_accepted(ContextClass::Legion, ContextClass::Squad));
        p.accept_downgrade(ContextClass::Legion, ContextClass::Squad);
        assert!(p.is_downgrade_accepted(ContextClass::Legion, ContextClass::Squad));
        // Idempotent
        p.accept_downgrade(ContextClass::Legion, ContextClass::Squad);
        assert_eq!(p.downgrade_overrides.len(), 1);
    }

    #[test]
    fn profile_pin_floor_round_trip() {
        let mut p = Profile {
            last_used_model: None,
            thinking_level: None,
            max_turns: None,
            provider_order: Vec::new(),
            avoid_providers: Vec::new(),
            context_floor_pin: None,
            downgrade_overrides: Vec::new(),
            embed_url: None,
            embed_model: None,
        };
        assert_eq!(p.pinned_floor(), None);
        p.pin_floor(ContextClass::Clan);
        assert_eq!(p.pinned_floor(), Some(ContextClass::Clan));
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
                model_id: "claude-opus-4-6".into(),
            }),
            thinking_level: Some("high".into()),
            max_turns: Some(50),
            provider_order: vec!["anthropic".into(), "openai".into()],
            avoid_providers: vec![],
            context_floor_pin: Some("Clan".into()),
            downgrade_overrides: vec!["Legion→Squad".into()],
            embed_url: None,
            embed_model: None,
        };
        let json = serde_json::to_string_pretty(&p).unwrap();
        let parsed: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider_order, vec!["anthropic", "openai"]);
        assert_eq!(parsed.context_floor_pin, Some("Clan".into()));
        assert_eq!(parsed.downgrade_overrides, vec!["Legion→Squad"]);
    }

    #[test]
    fn old_profile_deserializes_cleanly() {
        // Old profile without new fields — should deserialize without error
        let json = r#"{"lastUsedModel": {"provider": "anthropic", "modelId": "claude-sonnet-4-6"}, "thinkingLevel": "medium"}"#;
        let p: Profile = serde_json::from_str(json).unwrap();
        assert!(p.provider_order.is_empty());
        assert!(p.avoid_providers.is_empty());
        assert!(p.context_floor_pin.is_none());
        assert!(p.downgrade_overrides.is_empty());
    }

    #[test]
    fn project_profile_path_resolves_to_repo_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let nested = tmp.path().join("core/crates/omegon");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            project_profile_path(&nested),
            tmp.path().join(".omegon/profile.json")
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
            avoid_providers: Vec::new(),
            context_floor_pin: None,
            downgrade_overrides: Vec::new(),
            embed_url: None,
            embed_model: None,
        };

        profile.save(&nested).unwrap();

        assert!(tmp.path().join(".omegon/profile.json").exists());
        assert!(!nested.join(".omegon/profile.json").exists());
    }
}
