//! TUI Theme — Alpharius color system for ratatui.
//!
//! Loads from `themes/alpharius.json` when available, falls back to
//! compiled-in defaults. The JSON file is the source of truth — it
//! defines `vars` (base tokens) and `colors` (semantic mappings).
//!
//! To add a new theme, implement the Theme trait with different values.

use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

/// Semantic color slots for the TUI.
pub trait Theme: Send + Sync {
    // ─── Core palette ───────────────────────────────────────────────
    fn bg(&self) -> Color;
    fn card_bg(&self) -> Color;
    fn surface_bg(&self) -> Color;
    fn border(&self) -> Color;
    fn border_dim(&self) -> Color;

    // ─── Text ───────────────────────────────────────────────────────
    fn fg(&self) -> Color;
    fn muted(&self) -> Color;
    fn dim(&self) -> Color;

    // ─── Brand ──────────────────────────────────────────────────────
    fn accent(&self) -> Color;
    fn accent_muted(&self) -> Color;
    fn accent_bright(&self) -> Color;

    // ─── Signal ─────────────────────────────────────────────────────
    fn success(&self) -> Color;
    fn error(&self) -> Color;
    fn warning(&self) -> Color;
    fn caution(&self) -> Color;

    // ─── Extended (semantic tool/diff colors) ───────────────────────
    fn footer_bg(&self) -> Color { Color::Rgb(1, 3, 6) }
    fn user_msg_bg(&self) -> Color { self.card_bg() }
    fn tool_success_bg(&self) -> Color { self.card_bg() }
    fn tool_error_bg(&self) -> Color { Color::Rgb(30, 8, 16) }
    fn diff_added(&self) -> Color { self.success() }
    fn diff_removed(&self) -> Color { self.error() }
    fn diff_added_bg(&self) -> Color { Color::Rgb(4, 22, 12) }
    fn diff_removed_bg(&self) -> Color { Color::Rgb(22, 4, 4) }

    // ─── Derived styles ─────────────────────────────────────────────

    fn style_fg(&self) -> Style {
        Style::default().fg(self.fg())
    }
    fn style_muted(&self) -> Style {
        Style::default().fg(self.muted())
    }
    fn style_dim(&self) -> Style {
        Style::default().fg(self.dim())
    }
    fn style_accent(&self) -> Style {
        Style::default().fg(self.accent())
    }
    fn style_accent_bold(&self) -> Style {
        Style::default().fg(self.accent()).add_modifier(Modifier::BOLD)
    }
    fn style_success(&self) -> Style {
        Style::default().fg(self.success())
    }
    fn style_error(&self) -> Style {
        Style::default().fg(self.error())
    }
    fn style_warning(&self) -> Style {
        Style::default().fg(self.warning())
    }
    fn style_heading(&self) -> Style {
        Style::default().fg(self.accent_bright()).add_modifier(Modifier::BOLD)
    }
    fn style_user_input(&self) -> Style {
        Style::default().fg(self.fg()).add_modifier(Modifier::BOLD)
    }
    fn style_footer_bg(&self) -> Style {
        Style::default().bg(self.footer_bg())
    }
    fn style_border(&self) -> Style {
        Style::default().fg(self.border())
    }
    fn style_border_dim(&self) -> Style {
        Style::default().fg(self.border_dim())
    }
}

/// Parse a hex color string (#RRGGBB or RRGGBB) to a ratatui Color.
fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 { return None; }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Resolve a color value — either a hex string or a reference to a var.
fn resolve_color(value: &str, vars: &HashMap<String, String>) -> Option<Color> {
    if value.starts_with('#') {
        parse_hex(value)
    } else {
        // It's a var reference
        vars.get(value).and_then(|hex| parse_hex(hex))
    }
}

/// Theme loaded from alpharius.json — parameterized, not hardcoded.
pub struct JsonTheme {
    vars: HashMap<String, Color>,
}

impl JsonTheme {
    /// Load from a JSON theme file. Returns None if loading fails.
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;

        let vars_obj = json.get("vars")?.as_object()?;
        let mut raw_vars: HashMap<String, String> = HashMap::new();
        for (key, val) in vars_obj {
            if let Some(s) = val.as_str() {
                raw_vars.insert(key.clone(), s.to_string());
            }
        }

        // Resolve colors from the "colors" section (which references vars)
        let mut resolved: HashMap<String, Color> = HashMap::new();

        // First, resolve all vars directly
        for (key, hex) in &raw_vars {
            if let Some(color) = parse_hex(hex) {
                resolved.insert(key.clone(), color);
            }
        }

        // Then resolve semantic colors
        if let Some(colors_obj) = json.get("colors").and_then(|c| c.as_object()) {
            for (key, val) in colors_obj {
                if let Some(s) = val.as_str()
                    && let Some(color) = resolve_color(s, &raw_vars)
                {
                    resolved.insert(key.clone(), color);
                }
            }
        }

        // Also resolve export colors
        if let Some(export_obj) = json.get("export").and_then(|e| e.as_object()) {
            for (key, val) in export_obj {
                if let Some(s) = val.as_str()
                    && let Some(color) = parse_hex(s)
                {
                    resolved.insert(format!("export_{key}"), color);
                }
            }
        }

        Some(Self { vars: resolved })
    }

    fn get(&self, key: &str) -> Color {
        self.vars.get(key).copied().unwrap_or(Color::Reset)
    }
}

impl Theme for JsonTheme {
    fn bg(&self) -> Color { self.get("bg") }
    fn card_bg(&self) -> Color { self.get("cardBg") }
    fn surface_bg(&self) -> Color { self.get("surfaceBg") }
    fn border(&self) -> Color { self.get("borderColor") }
    fn border_dim(&self) -> Color { self.get("borderDim") }

    fn fg(&self) -> Color { self.get("fg") }
    fn muted(&self) -> Color { self.get("mutedFg") }
    fn dim(&self) -> Color { self.get("dimFg") }

    fn accent(&self) -> Color { self.get("primary") }
    fn accent_muted(&self) -> Color { self.get("primaryMuted") }
    fn accent_bright(&self) -> Color { self.get("primaryBright") }

    fn success(&self) -> Color { self.get("green") }
    fn error(&self) -> Color { self.get("red") }
    fn warning(&self) -> Color { self.get("orange") }
    fn caution(&self) -> Color { self.get("yellow") }

    fn user_msg_bg(&self) -> Color { self.get("userMsgBg") }
    fn tool_success_bg(&self) -> Color {
        self.vars.get("toolSuccessBg").copied().unwrap_or_else(|| self.card_bg())
    }
    fn tool_error_bg(&self) -> Color { self.get("toolErrorBg") }
    fn diff_added(&self) -> Color { self.get("toolDiffAdded") }
    fn diff_removed(&self) -> Color { self.get("toolDiffRemoved") }
    fn diff_added_bg(&self) -> Color {
        self.vars.get("toolDiffAddedBg").copied().unwrap_or(Color::Rgb(4, 22, 12))
    }
    fn diff_removed_bg(&self) -> Color {
        self.vars.get("toolDiffRemovedBg").copied().unwrap_or(Color::Rgb(22, 4, 4))
    }
}

/// Hardcoded fallback — used when alpharius.json is not found.
pub struct Alpharius;

impl Theme for Alpharius {
    fn bg(&self) -> Color { Color::Rgb(40, 0, 0) }
    fn card_bg(&self) -> Color { Color::Rgb(2, 6, 12) }
    fn surface_bg(&self) -> Color { Color::Rgb(40, 0, 0) }
    fn border(&self) -> Color { Color::Rgb(48, 112, 140) }
    fn border_dim(&self) -> Color { Color::Rgb(32, 72, 96) }

    fn fg(&self) -> Color { Color::Rgb(196, 216, 228) }
    fn muted(&self) -> Color { Color::Rgb(96, 120, 136) }
    fn dim(&self) -> Color { Color::Rgb(64, 88, 112) }

    fn accent(&self) -> Color { Color::Rgb(42, 180, 200) }
    fn accent_muted(&self) -> Color { Color::Rgb(26, 136, 152) }
    fn accent_bright(&self) -> Color { Color::Rgb(110, 202, 216) }

    fn success(&self) -> Color { Color::Rgb(26, 184, 120) }
    fn error(&self) -> Color { Color::Rgb(224, 72, 72) }
    fn warning(&self) -> Color { Color::Rgb(200, 100, 24) }
    fn caution(&self) -> Color { Color::Rgb(120, 184, 32) }
}

/// Load the theme — try alpharius.json first, fall back to hardcoded.
pub fn default_theme() -> Box<dyn Theme> {
    // Search for alpharius.json relative to cwd
    let search_paths = [
        std::path::PathBuf::from("themes/alpharius.json"),
        std::path::PathBuf::from("../themes/alpharius.json"),
    ];

    // Also check relative to the project root via .git
    let mut project_root = std::env::current_dir().unwrap_or_default();
    for _ in 0..5 {
        if project_root.join(".git").exists() || project_root.join("themes/alpharius.json").exists() {
            let theme_path = project_root.join("themes/alpharius.json");
            if let Some(theme) = JsonTheme::load(&theme_path) {
                tracing::info!(path = %theme_path.display(), "loaded theme from JSON");
                return Box::new(theme);
            }
            break;
        }
        if !project_root.pop() { break; }
    }

    for path in &search_paths {
        if let Some(theme) = JsonTheme::load(path) {
            tracing::info!(path = %path.display(), "loaded theme from JSON");
            return Box::new(theme);
        }
    }

    tracing::debug!("using hardcoded Alpharius theme (alpharius.json not found)");
    Box::new(Alpharius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_works() {
        assert_eq!(parse_hex("#2ab4c8"), Some(Color::Rgb(42, 180, 200)));
        assert_eq!(parse_hex("06080e"), Some(Color::Rgb(6, 8, 14)));
        assert_eq!(parse_hex("nope"), None);
    }

    #[test]
    fn alpharius_fallback_colors_are_distinct() {
        let t = Alpharius;
        assert_ne!(t.bg(), t.fg());
        assert_ne!(t.accent(), t.success());
        assert_ne!(t.error(), t.warning());
        assert_ne!(t.card_bg(), t.surface_bg());
    }

    #[test]
    fn derived_styles_have_correct_color() {
        let t = Alpharius;
        assert_eq!(t.style_accent().fg, Some(t.accent()));
    }

    #[test]
    fn json_theme_loads_from_file() {
        // Resolve relative to the crate manifest, not cwd
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir.join("../../themes/alpharius.json");
        if path.exists() {
            let theme = JsonTheme::load(&path).expect("should load alpharius.json");
            assert_ne!(theme.bg(), Color::Reset, "bg should be loaded");
            assert_ne!(theme.accent(), Color::Reset, "accent should be loaded");
            // Verify known values from the file
            assert_eq!(theme.accent(), Color::Rgb(42, 180, 200), "primary should be #2ab4c8");
        }
    }
}
