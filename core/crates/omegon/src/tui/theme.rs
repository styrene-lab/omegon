//! Semantic TUI styles. The default inherits the terminal's foreground,
//! background and ANSI palette. Legacy palettes remain available for explicit
//! theme integration; startup does not discover or load a theme file.

use ratatui::style::{Color, Modifier, Style};
use std::collections::HashMap;

/// Semantic color slots for the TUI.
pub trait Theme: Send + Sync {
    /// Resolve fixed-color legacy widgets after all surfaces and overlays render.
    fn finish_frame(&self, _buffer: &mut ratatui::buffer::Buffer) {}

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
    fn footer_bg(&self) -> Color {
        Color::Rgb(3, 7, 14)
    }
    fn user_msg_bg(&self) -> Color {
        self.card_bg()
    }
    fn tool_success_bg(&self) -> Color {
        self.card_bg()
    }
    fn tool_error_bg(&self) -> Color {
        Color::Rgb(30, 8, 16)
    }
    fn diff_added(&self) -> Color {
        self.success()
    }
    fn diff_removed(&self) -> Color {
        self.error()
    }
    fn diff_added_bg(&self) -> Color {
        Color::Rgb(4, 22, 12)
    }
    fn diff_removed_bg(&self) -> Color {
        Color::Rgb(22, 4, 4)
    }

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
        Style::default()
            .fg(self.accent())
            .add_modifier(Modifier::BOLD)
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
        Style::default()
            .fg(self.accent_bright())
            .add_modifier(Modifier::BOLD)
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

    /// Background colors that widgets may intentionally paint into the final
    /// frame. The TUI has a post-render cleanup pass to normalize accidental
    /// terminal/default-color bleed-through; this list keeps that pass from
    /// erasing deliberate badge, panel, diff, and signal backgrounds.
    fn intentional_backgrounds(&self) -> Vec<Color> {
        vec![
            self.bg(),
            self.surface_bg(),
            self.card_bg(),
            self.footer_bg(),
            self.user_msg_bg(),
            self.tool_success_bg(),
            self.tool_error_bg(),
            self.diff_added_bg(),
            self.diff_removed_bg(),
            self.accent(),
            self.accent_muted(),
            self.accent_bright(),
            self.border(),
            self.border_dim(),
            self.success(),
            self.error(),
            self.warning(),
            self.caution(),
        ]
    }
}

/// Parse a hex color string (#RRGGBB or RRGGBB) to a ratatui Color.
fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
        return None;
    }
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
    fn bg(&self) -> Color {
        self.get("bg")
    }
    fn card_bg(&self) -> Color {
        self.get("cardBg")
    }
    fn surface_bg(&self) -> Color {
        self.get("surfaceBg")
    }
    fn border(&self) -> Color {
        self.get("borderColor")
    }
    fn border_dim(&self) -> Color {
        self.get("borderDim")
    }

    fn fg(&self) -> Color {
        self.get("fg")
    }
    fn muted(&self) -> Color {
        self.get("mutedFg")
    }
    fn dim(&self) -> Color {
        self.get("dimFg")
    }

    fn accent(&self) -> Color {
        self.get("primary")
    }
    fn accent_muted(&self) -> Color {
        self.get("primaryMuted")
    }
    fn accent_bright(&self) -> Color {
        self.get("primaryBright")
    }

    fn success(&self) -> Color {
        self.muted()
    }
    fn error(&self) -> Color {
        self.get("orange")
    }
    fn warning(&self) -> Color {
        self.get("orange")
    }
    fn caution(&self) -> Color {
        self.get("orange")
    }

    fn footer_bg(&self) -> Color {
        self.vars
            .get("footerBg")
            .copied()
            .unwrap_or(Color::Rgb(1, 3, 6))
    }
    fn user_msg_bg(&self) -> Color {
        self.get("userMsgBg")
    }
    fn tool_success_bg(&self) -> Color {
        self.vars
            .get("toolSuccessBg")
            .copied()
            .unwrap_or_else(|| self.card_bg())
    }
    fn tool_error_bg(&self) -> Color {
        self.get("toolErrorBg")
    }
    fn diff_added(&self) -> Color {
        self.get("toolDiffAdded")
    }
    fn diff_removed(&self) -> Color {
        self.get("toolDiffRemoved")
    }
    fn diff_added_bg(&self) -> Color {
        self.vars
            .get("toolDiffAddedBg")
            .copied()
            .unwrap_or(Color::Rgb(4, 22, 12))
    }
    fn diff_removed_bg(&self) -> Color {
        self.vars
            .get("toolDiffRemovedBg")
            .copied()
            .unwrap_or(Color::Rgb(22, 4, 4))
    }
}

/// Legacy palette retained for explicit theme integration and reference tests.
pub struct Alpharius;

impl Theme for Alpharius {
    fn bg(&self) -> Color {
        Color::Rgb(2, 4, 8)
    } // Thunderhawk-tinted near-black
    fn card_bg(&self) -> Color {
        Color::Rgb(4, 10, 18)
    } // subtle lift for conversation cards
    fn surface_bg(&self) -> Color {
        Color::Rgb(2, 4, 8)
    } // matches bg
    fn border(&self) -> Color {
        Color::Rgb(48, 112, 140)
    }
    fn border_dim(&self) -> Color {
        Color::Rgb(36, 80, 104)
    } // brighter than before (was 32,72,96)

    fn fg(&self) -> Color {
        Color::Rgb(196, 216, 228)
    }
    fn muted(&self) -> Color {
        Color::Rgb(108, 136, 152)
    } // brighter (was 96,120,136)
    fn dim(&self) -> Color {
        Color::Rgb(72, 100, 124)
    } // brighter (was 64,88,112)

    fn accent(&self) -> Color {
        Color::Rgb(42, 180, 200)
    }
    fn accent_muted(&self) -> Color {
        Color::Rgb(26, 136, 152)
    }
    fn accent_bright(&self) -> Color {
        Color::Rgb(110, 202, 216)
    }

    fn success(&self) -> Color {
        self.muted()
    }
    fn error(&self) -> Color {
        self.warning()
    }
    fn warning(&self) -> Color {
        Color::Rgb(200, 100, 24)
    }
    fn caution(&self) -> Color {
        self.warning()
    }
}

/// Use SGR default colors for surfaces and the terminal's ANSI palette for signals.
pub struct TerminalTheme;

impl Theme for TerminalTheme {
    fn finish_frame(&self, buffer: &mut ratatui::buffer::Buffer) {
        for cell in &mut buffer.content {
            if matches!(cell.fg, Color::Rgb(..) | Color::Indexed(16..=255)) {
                cell.set_fg(Color::Reset);
            }
            if matches!(cell.bg, Color::Rgb(..) | Color::Indexed(16..=255)) {
                cell.set_bg(Color::Reset);
            }
        }
    }

    fn bg(&self) -> Color {
        Color::Reset
    }
    fn card_bg(&self) -> Color {
        Color::Reset
    }
    fn surface_bg(&self) -> Color {
        Color::Reset
    }
    fn border(&self) -> Color {
        Color::Reset
    }
    fn fg(&self) -> Color {
        Color::Reset
    }
    fn accent(&self) -> Color {
        Color::Reset
    }
    fn accent_muted(&self) -> Color {
        Color::Reset
    }
    fn accent_bright(&self) -> Color {
        Color::Reset
    }
    fn footer_bg(&self) -> Color {
        Color::Reset
    }
    fn user_msg_bg(&self) -> Color {
        Color::Reset
    }
    fn tool_success_bg(&self) -> Color {
        Color::Reset
    }
    fn tool_error_bg(&self) -> Color {
        Color::Reset
    }
    fn diff_added_bg(&self) -> Color {
        Color::Reset
    }
    fn diff_removed_bg(&self) -> Color {
        Color::Reset
    }
    fn border_dim(&self) -> Color {
        Color::DarkGray
    }
    fn muted(&self) -> Color {
        Color::DarkGray
    }
    fn dim(&self) -> Color {
        Color::DarkGray
    }
    fn success(&self) -> Color {
        Color::Green
    }
    fn error(&self) -> Color {
        Color::Red
    }
    fn warning(&self) -> Color {
        Color::Yellow
    }
    fn caution(&self) -> Color {
        Color::Yellow
    }
    fn style_dim(&self) -> Style {
        Style::default()
            .fg(Color::Reset)
            .add_modifier(Modifier::DIM)
    }
    fn style_muted(&self) -> Style {
        self.style_dim()
    }
}

pub fn default_theme() -> Box<dyn Theme> {
    Box::new(TerminalTheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_default_preserves_terminal_colors() {
        let theme = default_theme();
        for color in [
            theme.bg(),
            theme.fg(),
            theme.surface_bg(),
            theme.card_bg(),
            theme.footer_bg(),
            theme.tool_error_bg(),
            theme.diff_added_bg(),
            theme.diff_removed_bg(),
            theme.accent(),
            theme.accent_muted(),
            theme.accent_bright(),
        ] {
            assert_eq!(color, Color::Reset);
        }
        assert!(theme.style_dim().add_modifier.contains(Modifier::DIM));
        assert!(
            !theme
                .intentional_backgrounds()
                .iter()
                .any(|c| matches!(c, Color::Rgb(..)))
        );
    }

    #[test]
    fn terminal_default_resolves_fixed_colors_without_erasing_signals() {
        let theme = default_theme();
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 2, 1));
        buffer[(0, 0)]
            .set_fg(Color::Rgb(42, 180, 200))
            .set_bg(Color::Indexed(232));
        buffer[(1, 0)].set_fg(Color::Red).set_bg(Color::Reset);
        theme.finish_frame(&mut buffer);
        assert_eq!(buffer[(0, 0)].fg, Color::Reset);
        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
        assert_eq!(buffer[(1, 0)].fg, Color::Red);
    }

    #[test]
    fn parse_hex_works() {
        assert_eq!(parse_hex("#2ab4c8"), Some(Color::Rgb(42, 180, 200)));
        assert_eq!(parse_hex("06080e"), Some(Color::Rgb(6, 8, 14)));
        assert_eq!(parse_hex("nope"), None);
    }

    #[test]
    fn alpharius_fallback_colors_follow_conservative_semantics() {
        let t = Alpharius;
        assert_ne!(t.bg(), t.fg());
        assert_ne!(t.accent(), t.success());
        assert_eq!(t.success(), t.muted());
        assert_eq!(t.error(), t.warning());
        assert_eq!(t.caution(), t.warning());
        assert_ne!(t.warning(), t.accent());
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
            assert_eq!(theme.success(), theme.muted());
            assert_eq!(theme.error(), theme.warning());
            assert_eq!(theme.caution(), theme.warning());
            // Verify known values from the file
            assert_eq!(
                theme.accent(),
                Color::Rgb(42, 180, 200),
                "primary should be #2ab4c8"
            );
        }
    }
}
