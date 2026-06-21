//! TUI effects — tachyonfx-powered visual polish.
//!
//! Each TUI zone (footer, editor, conversation) has its own `EffectManager`
//! so effects are processed against the correct screen area. Effects run as
//! post-processing passes on the ratatui buffer after widgets are rendered.
//!
//! Integration: `App::draw()` renders widgets normally, then calls
//! `effects.process(buf, conversation_area, footer_area, editor_area)`.

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
    TurnComplete,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConversationSlot {
    #[default]
    CardEffect,
    ActionPulse,
    ContextPressure,
}

/// Manages per-zone effects and tracks frame timing.
pub struct Effects {
    footer: EffectManager<FooterSlot>,
    editor: EffectManager<EditorSlot>,
    conversation: EffectManager<ConversationSlot>,
    last_frame: Instant,
    context_danger_active: bool,
}

impl Effects {
    pub fn new() -> Self {
        Self {
            footer: EffectManager::default(),
            editor: EffectManager::default(),
            conversation: EffectManager::default(),
            last_frame: Instant::now(),
            context_danger_active: false,
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
        self.footer.process_effects(duration, buf, footer_area);
        self.editor.process_effects(duration, buf, editor_area);
        self.conversation
            .process_effects(duration, buf, conversation_area);
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

    /// Thinking wave — WavePattern-modulated hue shift during agent thinking.
    /// Spatial variation across the editor border creates a "processing wavefront"
    /// rather than a flat uniform glow.
    pub fn start_spinner_glow(&mut self) {
        let glow = self.editor.unique(
            EditorSlot::SpinnerGlow,
            fx::never_complete(fx::ping_pong(fx::hsl_shift_fg(
                [25.0, 0.0, 0.12],
                EffectTimer::from_ms(2500, Interpolation::SineInOut),
            ))),
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
            fx::never_complete(fx::ping_pong(fx::hsl_shift_fg(
                [15.0, 0.0, 0.05],
                EffectTimer::from_ms(3000, Interpolation::SineInOut),
            ))),
        );
        self.editor.add_effect(pulse);
    }

    /// Stop the border pulse.
    pub fn stop_border_pulse(&mut self) {
        self.editor.cancel_unique_effect(EditorSlot::BorderPulse);
    }

    /// Turn-complete sweep — positive "your turn" confirmation.
    /// A brief lightness sweep across the editor when agent finishes.
    pub fn sweep_turn_complete(&mut self) {
        let sweep = self.editor.unique(
            EditorSlot::TurnComplete,
            fx::sequence(&[
                fx::hsl_shift_fg(
                    [0.0, 0.0, 0.20],
                    EffectTimer::from_ms(150, Interpolation::QuadOut),
                ),
                fx::hsl_shift_fg(
                    [0.0, 0.0, -0.20],
                    EffectTimer::from_ms(250, Interpolation::QuadIn),
                ),
            ]),
        );
        self.editor.add_effect(sweep);
    }

    // ── Conversation ───────────────────────────────────────────────────

    /// Tool card materialization — brief lightness pulse on new card appearance.
    /// Scoped to conversation zone; draws the eye to where new content appeared.
    pub fn pulse_new_card(&mut self) {
        let pulse = self.conversation.unique(
            ConversationSlot::CardEffect,
            fx::sequence(&[
                fx::hsl_shift_fg(
                    [0.0, 0.0, 0.12],
                    EffectTimer::from_ms(100, Interpolation::QuadOut),
                ),
                fx::hsl_shift_fg(
                    [0.0, 0.0, -0.12],
                    EffectTimer::from_ms(200, Interpolation::QuadIn),
                ),
            ])
            .with_filter(CellFilter::Text),
        );
        self.conversation.add_effect(pulse);
    }

    /// Error flash — red-shifted pulse on tool error in conversation zone.
    pub fn flash_error(&mut self) {
        let flash = self.conversation.unique(
            ConversationSlot::CardEffect,
            fx::sequence(&[
                fx::hsl_shift_fg(
                    [15.0, 0.15, 0.10],
                    EffectTimer::from_ms(120, Interpolation::QuadOut),
                ),
                fx::hsl_shift_fg(
                    [-15.0, -0.15, -0.10],
                    EffectTimer::from_ms(200, Interpolation::QuadIn),
                ),
            ])
            .with_filter(CellFilter::Text),
        );
        self.conversation.add_effect(flash);
    }

    /// Conversation action confirmation — short cyan pulse after selected-item actions.
    pub fn pulse_conversation_action(&mut self) {
        let pulse = self.conversation.unique(
            ConversationSlot::ActionPulse,
            fx::sequence(&[
                fx::hsl_shift_fg(
                    [0.0, 0.05, 0.18],
                    EffectTimer::from_ms(120, Interpolation::QuadOut),
                ),
                fx::hsl_shift_fg(
                    [0.0, -0.05, -0.18],
                    EffectTimer::from_ms(260, Interpolation::QuadIn),
                ),
            ])
            .with_filter(CellFilter::Text),
        );
        self.conversation.add_effect(pulse);
    }

    /// Context pressure gradient — subtly desaturate the upper conversation
    /// as context usage increases. Creates a "pressure from above" metaphor.
    pub fn set_context_pressure(&mut self, percent: f32) {
        if percent < 50.0 {
            self.conversation
                .cancel_unique_effect(ConversationSlot::ContextPressure);
            return;
        }
        // Scale intensity: 50% → barely visible, 90% → pronounced
        let intensity = ((percent - 50.0) / 40.0).clamp(0.0, 1.0);
        let darken_amount = intensity * 0.15;
        let desat_amount = intensity * 0.25;
        // Red tint at high pressure
        let hue_shift = if percent > 80.0 {
            (percent - 80.0) / 10.0 * 8.0
        } else {
            0.0
        };

        let pressure = self.conversation.unique(
            ConversationSlot::ContextPressure,
            fx::never_complete(fx::hsl_shift_fg(
                [hue_shift, -desat_amount, -darken_amount],
                EffectTimer::from_ms(500, Interpolation::Linear),
            ))
            .with_filter(CellFilter::Inner(Margin::new(0, 0))),
        );
        self.conversation.add_effect(pressure);
    }

    // ── Query ───────────────────────────────────────────────────────────

    /// True if any effects are active (drives render timing).
    pub fn has_active(&self) -> bool {
        self.footer.is_running() || self.editor.is_running() || self.conversation.is_running()
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
    fn conversation_action_pulse_activates_conversation_effects() {
        let mut fx = Effects::new();
        fx.pulse_conversation_action();
        assert!(fx.conversation.is_running());
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
        assert!(!fx.conversation.is_running());
    }

    #[test]
    fn turn_complete_sweep_activates() {
        let mut fx = Effects::new();
        fx.sweep_turn_complete();
        assert!(fx.editor.is_running());
    }

    #[test]
    fn card_pulse_activates_conversation() {
        let mut fx = Effects::new();
        fx.pulse_new_card();
        assert!(fx.conversation.is_running());
    }

    #[test]
    fn error_flash_activates_conversation() {
        let mut fx = Effects::new();
        fx.flash_error();
        assert!(fx.conversation.is_running());
    }

    #[test]
    fn context_pressure_activates_above_threshold() {
        let mut fx = Effects::new();
        fx.set_context_pressure(60.0);
        assert!(fx.conversation.is_running());
    }
}
