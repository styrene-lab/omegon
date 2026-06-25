//! Shared UI surface projection primitives.
//!
//! These types describe which high-level surfaces are active without tying that
//! decision to a specific renderer. The Ratatui app currently consumes them for
//! layout, and future client surfaces can use the same vocabulary when exposing
//! coarse UI/view capabilities.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePreset {
    Lean,
    Full,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiSurfaces {
    pub dashboard: bool,
    pub instruments: bool,
    pub footer: bool,
    pub activity: bool,
}

impl UiSurfaces {
    pub fn lean() -> Self {
        Self {
            dashboard: false,
            instruments: false,
            footer: false,
            activity: true,
        }
    }

    pub fn full() -> Self {
        Self {
            dashboard: true,
            instruments: true,
            footer: true,
            activity: true,
        }
    }

    /// True when surfaces should use compact rendering (no dashboard chrome).
    pub fn is_compact(&self) -> bool {
        !self.dashboard
    }

    pub fn preset(&self) -> SurfacePreset {
        match (self.dashboard, self.instruments, self.footer, self.activity) {
            (false, false, false, true) => SurfacePreset::Lean,
            (true, true, true, true) => SurfacePreset::Full,
            _ => SurfacePreset::Custom,
        }
    }

    /// Preset name for display and command responses.
    pub fn preset_name(&self) -> &'static str {
        match self.preset() {
            SurfacePreset::Lean => "lean",
            SurfacePreset::Full => "full",
            SurfacePreset::Custom => "custom",
        }
    }

    /// Toggle between the two named presets. Partial surface combinations are custom.
    pub fn toggle_preset(&self) -> Self {
        match self.preset() {
            SurfacePreset::Lean => Self::full(),
            SurfacePreset::Full | SurfacePreset::Custom => Self::lean(),
        }
    }
}
