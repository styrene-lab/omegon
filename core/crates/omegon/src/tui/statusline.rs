//! Slim-mode status line — single row of persistent operational telemetry.
//!
//! Renders between conversation and editor in slim mode only.
//! Fields shed right-to-left as terminal width shrinks, ensuring the
//! line never wraps. The leftmost fields (context %, turn, model, tokens)
//! are always visible; workspace, branch, file activity, OODA phase,
//! drift warnings, and persona appear when space allows.

use omegon_traits::{DriftKind, OodaPhase};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::theme::Theme;
use super::widgets;

#[derive(Debug, Default)]
pub struct StatusLine {
    pub context_percent: f32,
    pub turn: u32,
    pub model_short: String,
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub cwd_basename: String,
    pub git_branch: Option<String>,
    pub files_read: usize,
    pub files_modified: usize,
    pub phase: Option<OodaPhase>,
    pub drift: Option<DriftKind>,
    pub persona: Option<String>,
    pub viewport_hint: Option<String>,
    pub turn_state: Option<String>,
    pub operator_hint: Option<String>,
}

impl StatusLine {
    /// Update fields from footer_data at the start of each draw cycle.
    pub fn sync_from_footer(&mut self, footer: &super::footer::FooterData) {
        self.context_percent = footer.context_percent;
        self.turn = footer.turn;
        self.model_short = crate::settings::humanize_model_id(&footer.model_id);
        self.session_input_tokens = footer.session_input_tokens;
        self.session_output_tokens = footer.session_output_tokens;
        self.cwd_basename = std::path::Path::new(&footer.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        self.git_branch = footer.harness.git_branch.clone();
        self.persona = footer
            .harness
            .active_persona
            .as_ref()
            .map(|p| p.name.clone());
    }

    pub fn render(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let w = area.width as usize;
        if w < 20 {
            return;
        }

        let sep = Span::styled(" · ", Style::default().fg(t.dim()));
        let sect = Span::styled(" │ ", Style::default().fg(t.dim()));

        // ── Pinned fields (always shown) ────────────────────────

        let pct_str = format!("{}%", self.context_percent as u32);
        let pct_color = widgets::percent_color(self.context_percent, t);

        let turn_str = format!("t{}", self.turn);
        let in_str = format!("↑{}", fmt_tokens(self.session_input_tokens));
        let out_str = format!("↓{}", fmt_tokens(self.session_output_tokens));
        let tok_str = format!("{in_str} {out_str}");

        let mut spans: Vec<Span<'static>> = vec![
            Span::styled(format!(" {pct_str}"), Style::default().fg(pct_color)),
            sep.clone(),
            Span::styled(turn_str, Style::default().fg(t.muted())),
            sep.clone(),
            Span::styled(self.model_short.clone(), Style::default().fg(t.muted())),
            sep.clone(),
            Span::styled(tok_str, Style::default().fg(t.dim())),
        ];

        let mut used: usize = spans.iter().map(|s| s.width()).sum();

        // Detached conversation viewport. This is deliberately near the left
        // pinned fields: when Slim auto-pins a long answer at its start, the
        // operator must be able to tell that more transcript exists below.
        if let Some(ref hint) = self.viewport_hint {
            let field = Span::styled(hint.clone(), Style::default().fg(t.warning()));
            let cost = sect.width() + field.width();
            if used + cost < w {
                spans.push(sect.clone());
                spans.push(field);
                used += cost;
            }
        }

        // Explicit turn state: makes "done vs still running vs waiting"
        // visible without requiring the operator to infer it from scrollback.
        if let Some(ref state) = self.turn_state {
            let field = Span::styled(state.clone(), Style::default().fg(t.warning()));
            let cost = sect.width() + field.width();
            if used + cost < w {
                spans.push(sect.clone());
                spans.push(field);
                used += cost;
            }
        }

        // Contextual operator hint. This is fed from real session/profile state
        // in the TUI draw pass and sheds before workspace metadata.
        if let Some(ref hint) = self.operator_hint {
            let field = Span::styled(hint.clone(), Style::default().fg(t.accent_muted()));
            let cost = sect.width() + field.width();
            if used + cost < w {
                spans.push(sect.clone());
                spans.push(field);
                used += cost;
            }
        }

        // ── Responsive fields (shed right-to-left) ──────────────

        // CWD basename (≥55)
        if w >= 55 && !self.cwd_basename.is_empty() {
            let field = Span::styled(self.cwd_basename.clone(), Style::default().fg(t.muted()));
            let cost = sect.width() + field.width();
            if used + cost < w {
                spans.push(sect.clone());
                spans.push(field);
                used += cost;
            }
        }

        // Git branch (≥65)
        if w >= 65
            && let Some(ref branch) = self.git_branch
        {
            let field = Span::styled(branch.clone(), Style::default().fg(t.muted()));
            let cost = sep.width() + field.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(field);
                used += cost;
            }
        }

        // Files r/w (≥75)
        if w >= 75 && (self.files_read > 0 || self.files_modified > 0) {
            let field = Span::styled(
                format!("{}r {}w", self.files_read, self.files_modified),
                Style::default().fg(t.muted()),
            );
            let cost = sep.width() + field.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(field);
                used += cost;
            }
        }

        // OODA phase (≥85)
        if w >= 85
            && let Some(phase) = &self.phase
        {
            let (label, color) = match phase {
                OodaPhase::Act => ("Act", t.accent()),
                OodaPhase::Observe => ("Observe", t.muted()),
                OodaPhase::Orient => ("Orient", t.muted()),
                OodaPhase::Decide => ("Decide", t.muted()),
            };
            let field = Span::styled(label, Style::default().fg(color));
            let cost = sep.width() + field.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(field);
                used += cost;
            }
        }

        // Drift warning (≥90)
        if w >= 90
            && let Some(drift) = &self.drift
        {
            let label = match drift {
                DriftKind::OrientationChurn => "⚠ churn",
                DriftKind::RepeatedActionFailure => "⚠ retry",
                DriftKind::ValidationThrash => "⚠ thrash",
                DriftKind::ClosureStall => "⚠ stall",
            };
            let field = Span::styled(label, Style::default().fg(t.warning()));
            let cost = sep.width() + field.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(field);
                used += cost;
            }
        }

        // Persona (≥100)
        if w >= 100
            && let Some(ref persona) = self.persona
        {
            let field = Span::styled(format!("@{persona}"), Style::default().fg(t.accent_muted()));
            let cost = sep.width() + field.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(field);
            }
        }

        // Right-align version string
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        let version_width = version.len() + 1; // +1 for trailing space
        if used + version_width < w {
            let pad = w - used - version_width;
            spans.push(Span::styled(
                " ".repeat(pad),
                Style::default().bg(t.surface_bg()),
            ));
            spans.push(Span::styled(
                format!("{version} "),
                Style::default()
                    .fg(t.dim())
                    .add_modifier(ratatui::style::Modifier::DIM),
            ));
        }

        let line = Line::from(spans);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(line).style(Style::default().bg(t.surface_bg())),
            area,
        );
    }
}

fn fmt_tokens(count: u64) -> String {
    widgets::format_tokens_compact(count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_tokens_ranges() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(500), "500");
        assert_eq!(fmt_tokens(1500), "1k");
        assert_eq!(fmt_tokens(45_000), "45k");
        assert_eq!(fmt_tokens(1_200_000), "1M");
    }

    #[test]
    fn default_status_line() {
        let sl = StatusLine::default();
        assert_eq!(sl.turn, 0);
        assert_eq!(sl.context_percent, 0.0);
        assert!(sl.phase.is_none());
        assert!(sl.drift.is_none());
        assert!(sl.viewport_hint.is_none());
        assert!(sl.turn_state.is_none());
        assert!(sl.operator_hint.is_none());
    }
}
