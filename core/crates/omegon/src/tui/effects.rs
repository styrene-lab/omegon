//! TUI effects — tachyonfx-powered visual polish.
//!
//! Each TUI zone (conversation, footer, editor) has its own `EffectManager`
//! so effects are processed against the correct screen area. Effects run as
//! post-processing passes on the ratatui buffer after widgets are rendered.
//!
//! Integration: `App::draw()` renders widgets normally, then calls
//! `effects.process(buf, conversation_area, footer_area, editor_area)`.
//!
//! Note: conversation-zone effects (fade, flash, dissolve) were tried and
//! removed — whole-zone HSL shifts read as glitches, not transitions.
//! Conversation polish requires per-segment rect targeting, which is a
//! deeper integration. The border heat system in instruments.rs handles
//! the visual feedback role instead.

use std::time::Instant;

use ratatui::prelude::*;
use tachyonfx::{CellFilter, EffectManager, EffectTimer, Interpolation, fx};

use super::theme::Theme;

// ─── Effect slot keys ──────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FooterSlot {
    #[default]
    Ping,
    ContextDanger,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditorSlot {
    #[default]
    SpinnerGlow,
    BorderPulse,
}

/// Manages per-zone effects and tracks frame timing.
pub struct Effects {
    footer: EffectManager<FooterSlot>,
    editor: EffectManager<EditorSlot>,
    last_frame: Instant,
    context_danger_active: bool,
}

impl Effects {
    pub fn new() -> Self {
        Self {
            footer: EffectManager::default(),
            editor: EffectManager::default(),
            last_frame: Instant::now(),
            context_danger_active: false,
        }
    }

    /// Process all active effects on the buffer, each against its target area.
    /// Call after rendering widgets.
    pub fn process(
        &mut self,
        buf: &mut Buffer,
        _conversation_area: Rect,
        footer_area: Rect,
        editor_area: Rect,
    ) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame);
        self.last_frame = now;

        let duration = tachyonfx::Duration::from_millis(delta.as_millis() as u32);
        self.footer.process_effects(duration, buf, footer_area);
        self.editor.process_effects(duration, buf, editor_area);
    }

    // ── Footer ──────────────────────────────────────────────────────────

    /// Flash effect when a footer value changes (fact count, context %, model).
    /// CellFilter::Text prevents painting over instrument panel bars.
    pub fn ping_footer(&mut self, _t: &dyn Theme) {
        let ping = self.footer.unique(
            FooterSlot::Ping,
            fx::sequence(&[
                fx::hsl_shift_fg(
                    [0.0, 0.0, 0.15],
                    EffectTimer::from_ms(120, Interpolation::QuadOut),
                ),
                fx::hsl_shift_fg(
                    [0.0, 0.0, -0.15],
                    EffectTimer::from_ms(200, Interpolation::QuadIn),
                ),
            ])
            .with_filter(CellFilter::Text),
        );
        self.footer.add_effect(ping);
    }

    /// Context usage danger pulse — starts when >80%, stops when <75%.
    pub fn set_context_danger(&mut self, active: bool) {
        if active == self.context_danger_active {
            return;
        }
        self.context_danger_active = active;
        if active {
            let pulse = self.footer.unique(
                FooterSlot::ContextDanger,
                fx::never_complete(fx::ping_pong(
                    fx::hsl_shift_fg(
                        [0.0, 0.0, 0.08],
                        EffectTimer::from_ms(1500, Interpolation::SineInOut),
                    )
                    .with_filter(CellFilter::Text),
                )),
            );
            self.footer.add_effect(pulse);
        } else {
            self.footer.cancel_unique_effect(FooterSlot::ContextDanger);
        }
    }

    // ── Editor ──────────────────────────────────────────────────────────

    /// HSL cycling glow on the editor/spinner area during active turns.
    pub fn start_spinner_glow(&mut self) {
        let glow = self.editor.unique(
            EditorSlot::SpinnerGlow,
            fx::ping_pong(fx::hsl_shift_fg(
                [30.0, 0.0, 0.15],
                EffectTimer::from_ms(2000, Interpolation::SineInOut),
            )),
        );
        self.editor.add_effect(glow);
    }

    /// Stop the spinner glow.
    pub fn stop_spinner_glow(&mut self) {
        self.editor.cancel_unique_effect(EditorSlot::SpinnerGlow);
    }

    /// Subtle border pulse during active turns.
    pub fn start_border_pulse(&mut self) {
        let pulse = self.editor.unique(
            EditorSlot::BorderPulse,
            fx::never_complete(fx::ping_pong(
                fx::hsl_shift_fg(
                    [15.0, 0.0, 0.05],
                    EffectTimer::from_ms(3000, Interpolation::SineInOut),
                ),
            )),
        );
        self.editor.add_effect(pulse);
    }

    /// Stop the border pulse.
    pub fn stop_border_pulse(&mut self) {
        self.editor.cancel_unique_effect(EditorSlot::BorderPulse);
    }

    // ── Query ───────────────────────────────────────────────────────────

    /// True if any effects are active (drives render timing).
    pub fn has_active(&self) -> bool {
        self.footer.is_running() || self.editor.is_running()
    }
}

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

    #[test]
    fn effects_new_has_no_active() {
        let fx = Effects::new();
        assert!(!fx.has_active());
    }

    #[test]
    fn ping_footer_activates_footer() {
        let mut fx = Effects::new();
        let t = Alpharius;
        fx.ping_footer(&t);
        assert!(fx.footer.is_running());
    }

    #[test]
    fn context_danger_toggle() {
        let mut fx = Effects::new();
        fx.set_context_danger(true);
        assert!(fx.footer.is_running());
        assert!(fx.context_danger_active);
        fx.set_context_danger(true);
        assert!(fx.context_danger_active);
    }

    #[test]
    fn spinner_glow_lifecycle() {
        let mut fx = Effects::new();
        fx.start_spinner_glow();
        assert!(fx.has_active());
        fx.stop_spinner_glow();
    }

    #[test]
    fn effects_are_zone_isolated() {
        let mut fx = Effects::new();
        fx.start_spinner_glow();
        assert!(!fx.footer.is_running());
        assert!(fx.editor.is_running());
    }
}
