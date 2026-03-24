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

/// Runtime settings that can change mid-session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Active model (provider:model-id format).
    pub model: String,

    /// Thinking level: off, minimal, low, medium, high.
    pub thinking: ThinkingLevel,

    /// Maximum turns per agent invocation. 0 = no limit.
    pub max_turns: u32,

    /// Context compaction threshold (fraction of context window).
    pub compaction_threshold: f32,

    /// Context window size (tokens). Inferred from model via route matrix.
    pub context_window: usize,

    /// Context class — named abstraction over context_window.
    /// Derived from context_window, not set directly.
    pub context_class: ContextClass,

    /// Extended context mode — legacy Anthropic 200k/1M toggle.
    /// Deprecated: derived from context_class. Kept for backward compat.
    pub context_mode: ContextMode,

    /// Tool display detail level.
    pub tool_detail: ToolDetail,

    /// Provider preference order for routing. First = most preferred.
    #[serde(default)]
    pub provider_order: Vec<String>,

    /// Whether a live LLM provider is connected. Set to false when NullBridge
    /// is active (no credentials available). The TUI uses this to show
    /// "no provider" instead of a model name that can't actually be used.
    #[serde(skip)]
    pub provider_connected: bool,
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    (ContextClass::Squad, 131_072),     // 128k
    (ContextClass::Maniple, 278_528),   // ~272k
    (ContextClass::Clan, 450_560),      // ~440k (covers 400k models)
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

impl Default for Settings {
    fn default() -> Self {
        let context_window = 200_000;
        Self {
            model: "anthropic:claude-sonnet-4-6".into(),
            thinking: ThinkingLevel::Medium,
            max_turns: 50,
            compaction_threshold: 0.75,
            context_window,
            context_class: ContextClass::from_tokens(context_window),
            context_mode: ContextMode::Standard,
            tool_detail: ToolDetail::Detailed,
            provider_order: Vec::new(),
            provider_connected: true, // optimistic default — set false when NullBridge
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

    pub fn model_short(&self) -> &str {
        self.model.split(':').next_back()
            .or_else(|| self.model.split('/').next_back())
            .unwrap_or(&self.model)
    }

    pub fn provider(&self) -> &str {
        self.model.split(':').next().unwrap_or("anthropic")
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
        &[Self::Off, Self::Minimal, Self::Low, Self::Medium, Self::High]
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
    route_matrix().iter().find_map(|entry| {
        if entry.provider != provider {
            return None;
        }
        let pattern = &entry.model_id_pattern;
        let matches = if pattern.ends_with('*') {
            model_id.starts_with(&pattern[..pattern.len() - 1])
        } else {
            model_id == pattern
        };
        matches.then_some(entry.context_ceiling)
    })
}

/// Infer context window from model identifier.
/// Uses the embedded route matrix first, falls back to heuristics.
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
    if name.contains("opus") || name.contains("sonnet") { return 200_000; }
    if name.contains("haiku") { return 200_000; }
    if name.contains("gpt-5") { return 272_000; }
    if name.contains("gpt-4.1") { return 200_000; }

    131_072 // fail-closed: default to Squad
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModel {
    pub provider: String,
    pub model_id: String,
}

impl Profile {
    /// Load profile. Project-level (`.omegon/profile.json`) overrides
    /// global (`~/.config/omegon/profile.json`). Both are optional.
    pub fn load(cwd: &std::path::Path) -> Self {
        // Project-level first
        let project_path = cwd.join(".omegon/profile.json");
        if let Ok(content) = std::fs::read_to_string(&project_path)
            && let Ok(profile) = serde_json::from_str(&content) {
                tracing::debug!(path = %project_path.display(), "project profile loaded");
                return profile;
            }

        // Global fallback
        if let Some(global_path) = global_profile_path()
            && let Ok(content) = std::fs::read_to_string(&global_path)
                && let Ok(profile) = serde_json::from_str(&content) {
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
        }
    }

    /// Save to the project-level profile.
    pub fn save(&self, cwd: &std::path::Path) -> anyhow::Result<()> {
        let dir = cwd.join(".omegon");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("profile.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        tracing::debug!(path = %path.display(), "project profile saved");
        Ok(())
    }

    /// Save to the global profile (~/.config/omegon/profile.json).
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
            && let Some(level) = ThinkingLevel::parse(t) {
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
        self.context_floor_pin.as_deref().and_then(ContextClass::parse)
    }

    /// Pin the context floor.
    pub fn pin_floor(&mut self, class: ContextClass) {
        self.context_floor_pin = Some(class.short().to_string());
    }
}

fn global_profile_path() -> Option<std::path::PathBuf> {
    // XDG on Linux, ~/Library/Application Support on macOS
    dirs::config_dir().map(|d| d.join("omegon/profile.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(ThinkingLevel::parse("functionary"), Some(ThinkingLevel::Minimal));
        assert_eq!(ThinkingLevel::parse("adept"), Some(ThinkingLevel::Low));
        assert_eq!(ThinkingLevel::parse("magos"), Some(ThinkingLevel::Medium));
        assert_eq!(ThinkingLevel::parse("archmagos"), Some(ThinkingLevel::High));
    }

    #[test]
    fn context_window_from_route_matrix() {
        // These should resolve via the embedded route matrix
        assert_eq!(infer_context_window("anthropic:claude-opus-4-6"), 1_000_000);
        assert_eq!(infer_context_window("anthropic:claude-sonnet-4-6"), 1_000_000);
        assert_eq!(infer_context_window("openai:gpt-5.4"), 272_000);
        assert_eq!(infer_context_window("anthropic:claude-haiku-4-5"), 200_000);
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
}
