//! TUI effects — tachyonfx-powered visual polish.
//!
//! Each TUI zone (conversation, footer, editor) has its own `EffectManager`
//! so effects are processed against the correct screen area. Effects run as
//! post-processing passes on the ratatui buffer after widgets are rendered.
//!
//! Integration: `App::draw()` renders widgets normally, then calls
//! `effects.process(buf, conversation_area, footer_area)`.

use std::time::Instant;

use ratatui::prelude::*;
use tachyonfx::{fx, EffectManager, EffectTimer, Interpolation, Motion};

use super::theme::Theme;

/// Effect slot keys — unique effects replace any existing effect with the same key.
/// `Default` is required by `EffectManager<K>`. The default variant (`Startup`)
/// has no semantic significance.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConvSlot {
    #[default]
    Startup,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FooterSlot {
    #[default]
    Reveal,
    Ping,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditorSlot {
    #[default]
    SpinnerGlow,
}

/// Manages per-zone effects and tracks frame timing.
pub struct Effects {
    conversation: EffectManager<ConvSlot>,
    footer: EffectManager<FooterSlot>,
    editor: EffectManager<EditorSlot>,
    last_frame: Instant,
}

impl Effects {
    pub fn new() -> Self {
        Self {
            conversation: EffectManager::default(),
            footer: EffectManager::default(),
            editor: EffectManager::default(),
            last_frame: Instant::now(),
        }
    }

    /// Process all active effects on the buffer, each against its target area.
    /// Call after rendering widgets.
    pub fn process(
        &mut self,
        buf: &mut Buffer,
        conversation_area: Rect,
        footer_area: Rect,
        editor_area: Rect,
    ) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame);
        self.last_frame = now;

        let duration = tachyonfx::Duration::from_millis(delta.as_millis() as u32);
        self.conversation.process_effects(duration, buf, conversation_area);
        self.footer.process_effects(duration, buf, footer_area);
        self.editor.process_effects(duration, buf, editor_area);
    }

    /// Queue the initial startup reveal effects.
    /// Resets the frame timer so effects start from zero delta.
    pub fn queue_startup(&mut self, t: &dyn Theme) {
        self.last_frame = Instant::now();
        // Footer sweeps in from bottom
        let footer_sweep = self.footer.unique(
            FooterSlot::Reveal,
            fx::sweep_in(
                Motion::DownToUp,
                3,   // gradient length
                1,   // randomness
                t.bg(),
                EffectTimer::from_ms(600, Interpolation::CubicOut),
            ),
        );
        self.footer.add_effect(footer_sweep);

        // Conversation fades in from void
        let conv_fade = self.conversation.unique(
            ConvSlot::Startup,
            fx::fade_from(
                t.bg(),
                t.bg(),
                EffectTimer::from_ms(800, Interpolation::CubicOut),
            ),
        );
        self.conversation.add_effect(conv_fade);
    }

    /// Flash effect when a footer value changes (fact count, context %, etc.).
    pub fn ping_footer(&mut self, t: &dyn Theme) {
        let ping = self.footer.unique(
            FooterSlot::Ping,
            fx::fade_from_fg(
                t.accent_bright(),
                EffectTimer::from_ms(300, Interpolation::CubicOut),
            ),
        );
        self.footer.add_effect(ping);
    }

    /// HSL cycling glow on the editor/spinner area.
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

    /// True if any effects are active (drives render timing).
    pub fn has_active(&self) -> bool {
        self.conversation.is_running()
            || self.footer.is_running()
            || self.editor.is_running()
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
    fn queue_startup_adds_effects() {
        let mut fx = Effects::new();
        let t = Alpharius;
        fx.queue_startup(&t);
        assert!(fx.has_active());
    }

    #[test]
    fn ping_footer_adds_effect() {
        let mut fx = Effects::new();
        let t = Alpharius;
        fx.ping_footer(&t);
        assert!(fx.has_active());
    }

    #[test]
    fn spinner_glow_lifecycle() {
        let mut fx = Effects::new();
        fx.start_spinner_glow();
        assert!(fx.has_active());
        fx.stop_spinner_glow();
        // Effect still active until processed — cancel marks it for removal
        // on next process_effects cycle
    }

    #[test]
    fn effects_are_zone_isolated() {
        let mut fx = Effects::new();
        let t = Alpharius;
        fx.ping_footer(&t);
        // Only footer should be active
        assert!(fx.footer.is_running());
        assert!(!fx.conversation.is_running());
        assert!(!fx.editor.is_running());
    }
}
