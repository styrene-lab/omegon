//! Runtime settings — mutable configuration shared between TUI and agent loop.
//!
//! This replaces pi's `sharedState` global. All runtime-mutable values live here.
//! The TUI reads for display. Commands write via the shared Arc<Mutex>.
//! The agent loop reads before each turn.
//!
//! Settings persist for the session. Serialized to session snapshot on save.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Runtime settings that can change mid-session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Active model (provider:model-id format).
    pub model: String,

    /// Thinking level: off, low, medium, high.
    pub thinking: ThinkingLevel,

    /// Maximum turns per agent invocation. 0 = no limit.
    pub max_turns: u32,

    /// Context compaction threshold (fraction of context window).
    pub compaction_threshold: f32,

    /// Context window size (tokens). Inferred from model + context_mode.
    pub context_window: usize,

    /// Extended context mode — controls 200k vs 1M for Anthropic models.
    pub context_mode: ContextMode,

    /// Tool display detail level.
    pub tool_detail: ToolDetail,
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

/// Context window mode for providers that support multiple sizes.
/// Anthropic models default to 200k but support 1M via beta header.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextMode {
    /// Standard context window (200k for Anthropic, varies for OpenAI).
    #[default]
    Standard,
    /// Extended 1M context window (Anthropic beta).
    Extended,
}

impl ContextMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "200k",
            Self::Extended => "1M",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "standard" | "200k" | "default" => Some(Self::Standard),
            "extended" | "1m" | "1M" | "million" => Some(Self::Extended),
            _ => None,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Standard => "◇",
            Self::Extended => "◆",
        }
    }

    /// Returns the Anthropic beta header flag needed for this mode, if any.
    pub fn anthropic_beta_flag(&self) -> Option<&'static str> {
        match self {
            Self::Standard => None,
            Self::Extended => Some("context-1m-2025-08-07"),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: "anthropic:claude-sonnet-4-6".into(),
            thinking: ThinkingLevel::Medium,
            max_turns: 50,
            compaction_threshold: 0.75,
            context_window: 200_000,
            context_mode: ContextMode::Standard,
            tool_detail: ToolDetail::Detailed,
        }
    }
}

impl Settings {
    pub fn new(model: &str) -> Self {
        let context_window = infer_context_window(model);
        Self {
            model: model.to_string(),
            context_window,
            ..Default::default()
        }
    }

    /// Recalculate context_window based on current model + context_mode.
    pub fn apply_context_mode(&mut self) {
        let base = infer_context_window(&self.model);
        self.context_window = match self.context_mode {
            ContextMode::Extended if self.provider() == "anthropic" => 1_000_000,
            _ => base,
        };
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
}

impl ThinkingLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "low" | "min" | "minimal" => Some(Self::Low),
            "medium" | "med" | "default" => Some(Self::Medium),
            "high" | "max" => Some(Self::High),
            _ => None,
        }
    }

    pub fn budget_tokens(&self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Low => Some(5_000),
            Self::Medium => Some(10_000),
            Self::High => Some(50_000),
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Off => "○",
            Self::Low => "◔",
            Self::Medium => "◑",
            Self::High => "◉",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Off, Self::Low, Self::Medium, Self::High]
    }
}

/// Infer context window from model identifier.
fn infer_context_window(model: &str) -> usize {
    let name = model.split(':').next_back().unwrap_or(model);
    // Anthropic — all current models are 200k
    if name.contains("opus") { return 200_000; }
    if name.contains("sonnet") { return 200_000; }
    if name.contains("haiku") { return 200_000; }
    // OpenAI — GPT-5.x is 1M, GPT-4.1 is 1M, o-series is 200k
    if name.contains("gpt-5") { return 1_000_000; }
    if name.contains("gpt-4.1") { return 1_000_000; }
    if name.contains("o3") || name.contains("o4") { return 200_000; }
    200_000 // safe default
}

/// Thread-safe shared settings handle.
pub type SharedSettings = Arc<Mutex<Settings>>;

pub fn shared(model: &str) -> SharedSettings {
    Arc::new(Mutex::new(Settings::new(model)))
}

// ─── Profile persistence ────────────────────────────────────────────────────

/// Profile: settings that persist with the project in .pi/config.json.
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

        Self { last_used_model: None, thinking_level: None, max_turns: None }
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
            settings.model = format!("{}:{}", m.provider, m.model_id);
            settings.context_window = infer_context_window(&settings.model);
        }
        if let Some(ref t) = self.thinking_level
            && let Some(level) = ThinkingLevel::parse(t) {
                settings.thinking = level;
            }
        if let Some(turns) = self.max_turns {
            settings.max_turns = turns;
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
    fn context_window_inference() {
        assert_eq!(infer_context_window("anthropic:claude-sonnet-4-6"), 200_000);
        assert_eq!(infer_context_window("anthropic:claude-opus-4-6"), 200_000);
        assert_eq!(infer_context_window("anthropic:claude-haiku-4-5-20251001"), 200_000);
        assert_eq!(infer_context_window("openai:gpt-5.4"), 1_000_000);
        assert_eq!(infer_context_window("openai:gpt-4.1"), 1_000_000);
        assert_eq!(infer_context_window("openai:o3"), 200_000);
        assert_eq!(infer_context_window("openai:o4-mini"), 200_000);
    }

    #[test]
    fn thinking_budget() {
        assert_eq!(ThinkingLevel::Off.budget_tokens(), None);
        assert_eq!(ThinkingLevel::High.budget_tokens(), Some(50_000));
    }
}
