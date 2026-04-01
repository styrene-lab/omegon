//! Extension state management — track enable/disable status, crashes, and health.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::{anyhow, Result};
use chrono::Utc;

/// Extension state persisted to .omegon/state.toml in the extension directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionState {
    /// Whether the extension is enabled (should be spawned on TUI startup)
    pub enabled: bool,

    /// ISO 8601 timestamp when extension was last enabled
    #[serde(default)]
    pub last_enabled_at: Option<String>,

    /// ISO 8601 timestamp when extension was last disabled
    #[serde(default)]
    pub last_disabled_at: Option<String>,

    /// Stability metrics
    #[serde(default)]
    pub stability: StabilityMetrics,
}

/// Crash and health tracking for extension.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StabilityMetrics {
    /// Number of crashes in current TUI session
    pub crashes_this_session: u32,

    /// Number of health check failures
    pub health_check_failures: u32,

    /// Last error message (if any)
    #[serde(default)]
    pub last_error: Option<String>,

    /// ISO 8601 timestamp of last error
    #[serde(default)]
    pub last_error_at: Option<String>,

    /// Whether extension was auto-disabled due to crashes
    #[serde(default)]
    pub auto_disabled: bool,
}

impl ExtensionState {
    /// Create a new extension state (enabled by default).
    pub fn new() -> Self {
        Self {
            enabled: true,
            last_enabled_at: None,
            last_disabled_at: None,
            stability: StabilityMetrics::default(),
        }
    }

    /// Load state from .omegon/state.toml in the extension directory.
    /// If file doesn't exist, returns default (enabled, no crashes).
    pub fn load(ext_dir: &PathBuf) -> Result<Self> {
        let state_path = ext_dir.join(".omegon").join("state.toml");

        if !state_path.exists() {
            return Ok(Self::new());
        }

        let content = std::fs::read_to_string(&state_path)?;
        let state: Self = toml::from_str(&content)
            .map_err(|e| anyhow!("failed to parse extension state: {}", e))?;
        Ok(state)
    }

    /// Save state to .omegon/state.toml in the extension directory.
    pub fn save(&self, ext_dir: &PathBuf) -> Result<()> {
        let state_dir = ext_dir.join(".omegon");
        std::fs::create_dir_all(&state_dir)?;

        let state_path = state_dir.join("state.toml");
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&state_path, content)?;
        Ok(())
    }

    /// Mark extension as enabled (called when user enables it).
    pub fn mark_enabled(&mut self) {
        self.enabled = true;
        self.last_enabled_at = Some(Utc::now().to_rfc3339());
        // Reset session crashes (new session of extension being active)
        self.stability.crashes_this_session = 0;
    }

    /// Mark extension as disabled (called when user disables it).
    pub fn mark_disabled(&mut self) {
        self.enabled = false;
        self.last_disabled_at = Some(Utc::now().to_rfc3339());
    }

    /// Record a crash or health check failure.
    pub fn record_error(&mut self, error: String) {
        self.stability.crashes_this_session += 1;
        self.stability.last_error = Some(error);
        self.stability.last_error_at = Some(Utc::now().to_rfc3339());

        // Auto-disable if crashed too many times
        if self.stability.crashes_this_session >= 3 {
            self.enabled = false;
            self.stability.auto_disabled = true;
        }
    }

    /// Record a health check failure.
    pub fn record_health_check_failure(&mut self, reason: String) {
        self.stability.health_check_failures += 1;
        self.stability.last_error = Some(format!("health check failed: {}", reason));
        self.stability.last_error_at = Some(Utc::now().to_rfc3339());
    }

    /// Clear session crashes (called at TUI startup).
    pub fn reset_session_crashes(&mut self) {
        self.stability.crashes_this_session = 0;
        self.stability.auto_disabled = false;
    }

    /// Check if extension is in a failed state.
    pub fn is_failed(&self) -> bool {
        self.stability.crashes_this_session >= 3 || self.stability.auto_disabled
    }

    /// Get human-readable status.
    pub fn status_text(&self) -> String {
        if !self.enabled {
            if let Some(err) = &self.stability.last_error {
                return format!("disabled: {}", err);
            }
            return "disabled".to_string();
        }

        if self.is_failed() {
            return format!("failed ({} crashes)", self.stability.crashes_this_session);
        }

        if self.stability.health_check_failures > 0 {
            return format!("degraded ({} health check failures)", self.stability.health_check_failures);
        }

        "enabled".to_string()
    }
}

impl Default for ExtensionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_creation() {
        let state = ExtensionState::new();
        assert!(state.enabled);
        assert_eq!(state.stability.crashes_this_session, 0);
    }

    #[test]
    fn test_mark_enabled() {
        let mut state = ExtensionState::new();
        state.enabled = false;
        state.mark_enabled();
        assert!(state.enabled);
        assert!(state.last_enabled_at.is_some());
    }

    #[test]
    fn test_mark_disabled() {
        let mut state = ExtensionState::new();
        state.mark_disabled();
        assert!(!state.enabled);
        assert!(state.last_disabled_at.is_some());
    }

    #[test]
    fn test_record_error_auto_disables_after_3() {
        let mut state = ExtensionState::new();
        assert!(state.enabled);

        state.record_error("crash 1".to_string());
        assert!(state.enabled); // Still enabled after 1 crash

        state.record_error("crash 2".to_string());
        assert!(state.enabled); // Still enabled after 2 crashes

        state.record_error("crash 3".to_string());
        assert!(!state.enabled); // Auto-disabled after 3 crashes
        assert!(state.stability.auto_disabled);
    }

    #[test]
    fn test_reset_session_crashes() {
        let mut state = ExtensionState::new();
        state.stability.crashes_this_session = 5;
        state.stability.auto_disabled = true;

        state.reset_session_crashes();
        assert_eq!(state.stability.crashes_this_session, 0);
        assert!(!state.stability.auto_disabled);
    }

    #[test]
    fn test_status_text() {
        let mut state = ExtensionState::new();
        assert_eq!(state.status_text(), "enabled");

        state.mark_disabled();
        assert_eq!(state.status_text(), "disabled");

        let mut state = ExtensionState::new();
        state.record_error("crash 1".to_string());
        state.record_error("crash 2".to_string());
        state.record_error("crash 3".to_string());
        // After 3 crashes, it's auto-disabled, so status shows "disabled: {error}"
        assert!(state.status_text().contains("disabled:"));
    }
}
