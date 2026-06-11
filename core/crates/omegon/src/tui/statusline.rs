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
use crate::surfaces::footer::ProjectFooterSurface;

#[derive(Debug, Default)]
pub struct StatusLine {
    pub context_percent: f32,
    pub turn: u32,
    pub model_short: String,
    pub model_provider: String,
    pub model_tier: String,
    pub thinking_level: String,
    pub posture: String,
    pub runtime_brand: String,
    pub principal_id: String,
    pub authorization: String,
    pub provider_connected: bool,
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
        let projection = footer.project_footer_surface();
        self.context_percent = projection.context.percent;
        self.turn = projection.session.turn;
        self.model_short = projection.engine.model_short;
        self.model_provider = projection.engine.model_provider;
        self.model_tier = projection.engine.model_tier;
        self.thinking_level = projection.engine.thinking_level;
        self.posture = projection.engine.posture;
        self.runtime_brand = projection.engine.runtime_brand;
        self.principal_id = projection.engine.principal_id;
        self.authorization = projection.engine.authorization;
        self.provider_connected = projection.engine.provider_connected;
        self.session_input_tokens = projection.session.session_input_tokens;
        self.session_output_tokens = projection.session.session_output_tokens;
        self.cwd_basename = projection.workspace.cwd_basename;
        self.git_branch = projection.workspace.git_branch;
        self.persona = projection.workspace.persona;
    }

    pub fn preferred_height(width: u16) -> u16 {
        if width < 20 { 0 } else { 2 }
    }

    pub fn render(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let height = Self::preferred_height(area.width).min(area.height);
        if height == 0 {
            return;
        }
        let lifecycle_area = Rect::new(area.x, area.y, area.width, 1);
        self.render_lifecycle_row(lifecycle_area, frame, t);
        if height > 1 {
            let engine_area = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
            self.render_engine_row(engine_area, frame, t);
        }
    }

    fn render_lifecycle_row(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
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
            let field = Span::styled(turn_state_field(state), Style::default().fg(t.warning()));
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
            let field = Span::styled(
                format!("dir {}", self.cwd_basename),
                Style::default().fg(t.muted()),
            );
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
            let field = Span::styled(format!("git {branch}"), Style::default().fg(t.muted()));
            let cost = sep.width() + field.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(field);
                used += cost;
            }
        }

        // File activity (≥75). Keep the default Slim wording semantic; the
        // older "12r 4w" shorthand was compact but opaque (r = read, w =
        // written/modified), especially next to git branch metadata.
        if w >= 75 && (self.files_read > 0 || self.files_modified > 0) {
            let label = file_activity_label(self.files_read, self.files_modified, w);
            let field = Span::styled(label, Style::default().fg(t.muted()));
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
            let ooda = ooda_phase_spans(*phase, t);
            let cost = sep.width() + ooda.iter().map(|span| span.width()).sum::<usize>();
            if used + cost < w {
                spans.push(sep.clone());
                spans.extend(ooda);
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

    fn render_engine_row(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let w = area.width as usize;
        if w < 20 {
            return;
        }

        let sep = Span::styled(" · ", Style::default().fg(t.dim()));
        let connection = if self.provider_connected {
            "online"
        } else {
            "offline"
        };
        let mut spans: Vec<Span<'static>> = vec![
            Span::styled(" engine ", Style::default().fg(t.accent())),
            Span::styled(self.runtime_brand.clone(), Style::default().fg(t.muted())),
        ];

        let mut used: usize = spans.iter().map(|s| s.width()).sum();
        for (field, style) in [
            (
                format!("posture {}", self.posture),
                Style::default().fg(t.muted()),
            ),
            (
                format!("provider {} {connection}", self.model_provider),
                Style::default().fg(t.dim()),
            ),
            (
                format!("who {}", self.principal_id),
                Style::default().fg(t.dim()),
            ),
            (
                format!("authz {}", self.authorization),
                Style::default().fg(t.dim()),
            ),
            (
                format!("tier {}", self.model_tier),
                Style::default().fg(t.dim()),
            ),
            (
                format!("think {}", self.thinking_level),
                Style::default().fg(t.dim()),
            ),
        ] {
            let span = Span::styled(field, style);
            let cost = sep.width() + span.width();
            if used + cost < w {
                spans.push(sep.clone());
                spans.push(span);
                used += cost;
            }
        }

        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(t.surface_bg())),
            area,
        );
    }
}

fn fmt_tokens(count: u64) -> String {
    widgets::format_tokens_compact(count as usize)
}

fn ooda_phase_spans(phase: OodaPhase, t: &dyn Theme) -> Vec<Span<'static>> {
    let active = match phase {
        OodaPhase::Observe => 0,
        OodaPhase::Orient => 1,
        OodaPhase::Decide => 2,
        OodaPhase::Act => 3,
    };
    let label = match phase {
        OodaPhase::Observe => "Observe",
        OodaPhase::Orient => "Orient",
        OodaPhase::Decide => "Decide",
        OodaPhase::Act => "Act",
    };
    let letters = ['o', 'o', 'd', 'a'];
    let mut spans = Vec::new();
    for (idx, ch) in letters.into_iter().enumerate() {
        if idx == active {
            spans.push(Span::styled(
                ch.to_ascii_uppercase().to_string(),
                Style::default().fg(t.accent()),
            ));
        } else {
            spans.push(Span::styled(ch.to_string(), Style::default().fg(t.dim())));
        }
    }
    spans.push(Span::styled(" ".to_string(), Style::default().fg(t.dim())));
    spans.push(Span::styled(
        label.to_string(),
        Style::default().fg(t.accent()),
    ));
    spans
}

fn turn_state_field(state: &str) -> String {
    // Keep the early-turn wait labels width-stable so transitions like
    // "provider request" -> "stream open" don't shove the rest of the
    // one-line Slim footer back and forth every frame.
    const WAITING_WIDTH: usize = "waiting: provider request".len();
    if matches!(state, "waiting: provider request" | "waiting: stream open") {
        format!("{state:<WAITING_WIDTH$}")
    } else {
        state.to_string()
    }
}

fn file_activity_label(read: usize, modified: usize, width: usize) -> String {
    let total = read + modified;
    if width >= 115 && read > 0 && modified > 0 {
        format!("files: {total} touched · {modified} changed · {read} read")
    } else if modified > 0 {
        format!("files: {total} touched · {modified} changed")
    } else {
        format!("files: {read} read")
    }
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

    #[test]
    fn preferred_height_matches_render_contract() {
        assert_eq!(StatusLine::preferred_height(0), 0);
        assert_eq!(StatusLine::preferred_height(19), 0);
        assert_eq!(StatusLine::preferred_height(20), 2);
    }

    #[test]
    fn ooda_phase_label_lights_active_letter() {
        let t = super::super::theme::Alpharius;
        let rendered = |phase| {
            ooda_phase_spans(phase, &t)
                .into_iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };

        assert_eq!(rendered(OodaPhase::Observe), "Ooda Observe");
        assert_eq!(rendered(OodaPhase::Orient), "oOda Orient");
        assert_eq!(rendered(OodaPhase::Decide), "ooDa Decide");
        assert_eq!(rendered(OodaPhase::Act), "oodA Act");
    }

    #[test]
    fn status_line_icons_label_directory_branch_and_ooda() {
        let mut sl = StatusLine {
            context_percent: 50.0,
            turn: 8,
            model_short: "gpt".into(),
            session_input_tokens: 32_000,
            session_output_tokens: 2_000,
            cwd_basename: "omegon".into(),
            git_branch: Some("fix/footer".into()),
            phase: Some(OodaPhase::Act),
            ..Default::default()
        };
        sl.operator_hint = Some("plan active".into());

        let backend = ratatui::backend::TestBackend::new(160, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| sl.render(frame.area(), frame, &super::super::theme::Alpharius))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..160 {
            text.push_str(buf[(x, 0)].symbol());
        }

        assert!(text.contains("dir omegon"), "{text}");
        assert!(text.contains("git fix/footer"), "{text}");
        assert!(text.contains("oodA Act"), "{text}");
    }

    #[test]
    fn turn_state_waiting_labels_are_width_stable() {
        let provider = turn_state_field("waiting: provider request");
        let stream = turn_state_field("waiting: stream open");

        assert_eq!(provider.len(), stream.len());
        assert_eq!(provider, "waiting: provider request");
        assert_eq!(stream.trim_end(), "waiting: stream open");
    }

    #[test]
    fn file_activity_label_is_semantic() {
        assert_eq!(
            file_activity_label(12, 4, 120),
            "files: 16 touched · 4 changed · 12 read"
        );
        assert_eq!(
            file_activity_label(12, 4, 90),
            "files: 16 touched · 4 changed"
        );
        assert_eq!(file_activity_label(12, 0, 90), "files: 12 read");
    }
}
