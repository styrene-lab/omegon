//! Slim-mode session row — persistent operational telemetry below the composer.
//!
//! The engine row above the composer owns provider/model/context capacity.
//! The workbench row directly below the composer owns active plan/workstream
//! progress. This row is the very bottom slim session row: turn lifecycle,
//! transcript state, token I/O, file activity, and version.
//! Fields shed right-to-left as terminal width shrinks, ensuring the line never
//! wraps.

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
pub struct SessionRow {
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
    /// Web-search provider gauge projected from first-party secret readiness.
    pub web_search_providers: Vec<(String, bool)>,
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

impl SessionRow {
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
        self.web_search_providers = projection.engine.web_search_providers;
        self.session_input_tokens = projection.session.session_input_tokens;
        self.session_output_tokens = projection.session.session_output_tokens;
        self.cwd_basename = projection.workspace.cwd_basename;
        self.git_branch = projection.workspace.git_branch;
        self.persona = projection.workspace.persona;
    }

    pub fn preferred_height(width: u16) -> u16 {
        if width < 20 { 0 } else { 1 }
    }

    pub fn preferred_height_for(&self, width: u16) -> u16 {
        if width < 20 {
            0
        } else if self.runtime_warning_row_needed() {
            2
        } else {
            1
        }
    }

    fn runtime_warning_row_needed(&self) -> bool {
        !self.provider_connected || self.drift.is_some()
    }

    pub fn render_for_level(
        &self,
        level: crate::surfaces::layout::UiPresentationLevel,
        area: Rect,
        frame: &mut Frame,
        t: &dyn Theme,
    ) {
        if level == crate::surfaces::layout::UiPresentationLevel::Om {
            self.render_om(area, frame, t);
        } else {
            self.render(area, frame, t);
        }
    }

    fn render_om(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        if area.width < 20 || area.height == 0 {
            return;
        }
        let available = area.width as usize;
        let mut fields = Vec::new();

        if let Some(attention) = self
            .turn_state
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            fields.push((0u8, attention.clone(), t.muted()));
        }
        if !self.cwd_basename.is_empty() {
            let workspace = self
                .git_branch
                .as_ref()
                .map(|branch| format!("{}:{branch}", self.cwd_basename))
                .unwrap_or_else(|| self.cwd_basename.clone());
            fields.push((1, workspace, t.dim()));
        }

        let right_status = format!(
            "{}  {} ",
            web_readiness_glyph(self.web_search_providers.iter().any(|(_, ready)| *ready)),
            omegon_version_label(),
        );
        let right_width = right_status.chars().count();
        let left_budget = available.saturating_sub(right_width);
        let mut selected: Vec<(u8, String, ratatui::style::Color)> = Vec::new();
        let mut used = 1usize;
        for field in fields {
            let cost = field.1.chars().count() + if selected.is_empty() { 0 } else { 3 };
            if used + cost <= left_budget {
                used += cost;
                selected.push(field);
            }
        }
        let mut spans = vec![Span::styled(" om", Style::default().fg(t.accent_muted()))];
        for (_, text, color) in selected {
            spans.push(Span::styled(" · ", Style::default().fg(t.dim())));
            spans.push(Span::styled(text, Style::default().fg(color)));
        }
        let left_width = spans.iter().map(|span| span.width()).sum::<usize>();
        spans.push(Span::raw(
            " ".repeat(available.saturating_sub(left_width + right_width)),
        ));
        spans.push(Span::styled(right_status, Style::default().fg(t.dim())));
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    pub fn render(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let height = Self::preferred_height(area.width).min(area.height);
        if height == 0 {
            return;
        }
        let session_area = Rect::new(area.x, area.y, area.width, 1);
        self.render_session_row(session_area, frame, t);
        if height > 1 && self.runtime_warning_row_needed() {
            let warning_area = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
            self.render_runtime_warning_row(warning_area, frame, t);
        }
    }

    fn render_session_row(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let w = area.width as usize;
        if w < 20 {
            return;
        }

        let sep = Span::styled(" · ", Style::default().fg(t.dim()));
        let sect = Span::styled(" │ ", Style::default().fg(t.dim()));

        // ── Pinned session fields (always shown) ─────────────────

        let turn_str = format!("turn {}", self.turn);
        let in_str = format!("↑{}", fmt_tokens(self.session_input_tokens));
        let out_str = format!("↓{}", fmt_tokens(self.session_output_tokens));
        let tok_str = format!("io {in_str} {out_str}");

        let mut spans: Vec<Span<'static>> = vec![
            Span::styled(" session", Style::default().fg(t.accent_muted())),
            sep.clone(),
            Span::styled(turn_str, Style::default().fg(t.muted())),
            sep.clone(),
            Span::styled(tok_str, Style::default().fg(t.dim())),
        ];

        let mut used: usize = spans.iter().map(|s| s.width()).sum();

        // Web-search liveness is persistent chrome, not a transient nag.
        // Missing readiness data and an all-empty keyed set are both degraded:
        // DDG scraping is a fallback floor, not an acceptable configured state.
        let configured = self
            .web_search_providers
            .iter()
            .filter(|(_, configured)| *configured)
            .count();
        let (label, color) = if configured == 0 {
            ("WEB! ddg-only".to_string(), t.warning())
        } else {
            let ticks: String = self
                .web_search_providers
                .iter()
                .map(|(_, configured)| if *configured { '●' } else { '○' })
                .collect();
            (format!("WEB {ticks}"), t.success())
        };
        let field = Span::styled(label, Style::default().fg(color));
        let cost = sect.width() + field.width();
        if used + cost < w {
            spans.push(sect.clone());
            spans.push(field);
            used += cost;
        }

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
        // in the TUI draw pass and sheds before activity metadata.
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

        // File activity (≥75). Keep the default Slim wording semantic; the
        // older "12r 4w" shorthand was compact but opaque (r = read, w =
        // written/modified).
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

        // Right-align version string. Official tag builds can stay compact;
        // branch/nightly/dev builds include the baked git hash for specificity.
        let version = omegon_version_label();
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

    fn render_runtime_warning_row(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        if area.width < 20 {
            return;
        }

        let text = crate::tui::inline_render::render_inline_text_row(
            &self.project_runtime_warning_row(),
            area.width,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(t.muted()).bg(t.surface_bg()),
            )))
            .style(Style::default().bg(t.surface_bg())),
            area,
        );
    }

    fn project_runtime_warning_row(&self) -> crate::surfaces::inline::InlineRow<String> {
        let connection = if self.provider_connected {
            "online"
        } else {
            "offline"
        };
        crate::surfaces::inline::InlineRow::new(
            vec![
                crate::surfaces::inline::InlineCell::new(
                    "runtime".to_string(),
                    crate::surfaces::inline::InlineCellRole::Label,
                ),
                crate::surfaces::inline::InlineCell::new(
                    self.runtime_brand.clone(),
                    crate::surfaces::inline::InlineCellRole::Value,
                ),
                crate::surfaces::inline::InlineCell::new(
                    format!("posture {}", self.posture),
                    crate::surfaces::inline::InlineCellRole::Metadata,
                ),
                crate::surfaces::inline::InlineCell::new(
                    format!("provider {} {connection}", self.model_provider),
                    crate::surfaces::inline::InlineCellRole::Metadata,
                ),
            ],
            vec![
                crate::surfaces::inline::InlineCell::new(
                    format!("who {}", self.principal_id),
                    crate::surfaces::inline::InlineCellRole::Metadata,
                ),
                crate::surfaces::inline::InlineCell::new(
                    format!("authz {}", self.authorization),
                    crate::surfaces::inline::InlineCellRole::Metadata,
                ),
                crate::surfaces::inline::InlineCell::new(
                    format!("grade {}", self.model_tier),
                    crate::surfaces::inline::InlineCellRole::Metadata,
                ),
                crate::surfaces::inline::InlineCell::new(
                    format!("think {}", self.thinking_level),
                    crate::surfaces::inline::InlineCellRole::Metadata,
                ),
            ],
        )
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

fn web_readiness_glyph(configured: bool) -> &'static str {
    use crate::tui::glyphs::ToolCategoryGlyphRole;

    if configured {
        crate::tui::glyphs::glyphs().tool_category(ToolCategoryGlyphRole::Network)
    } else {
        crate::tui::glyphs::glyphs().tool_state(crate::tui::glyphs::ToolStateGlyphRole::Failed)
    }
}

fn omegon_version_label() -> String {
    if env!("OMEGON_GIT_DESCRIBE").is_empty() && !env!("OMEGON_GIT_SHA").contains("-dirty") {
        concat!("v", env!("CARGO_PKG_VERSION")).to_string()
    } else {
        format!("v{} {}", env!("CARGO_PKG_VERSION"), env!("OMEGON_GIT_SHA"))
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
    fn default_session_row() {
        let sl = SessionRow::default();
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
        assert_eq!(SessionRow::preferred_height(0), 0);
        assert_eq!(SessionRow::preferred_height(19), 0);
        assert_eq!(SessionRow::preferred_height(20), 1);
    }

    #[test]
    fn preferred_height_for_collapses_online_runtime_warning_row() {
        let sl = SessionRow {
            provider_connected: true,
            ..Default::default()
        };
        assert_eq!(sl.preferred_height_for(80), 1);
    }

    #[test]
    fn preferred_height_for_keeps_disconnected_runtime_warning_row() {
        let sl = SessionRow {
            provider_connected: false,
            ..Default::default()
        };
        assert_eq!(sl.preferred_height_for(80), 2);
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
    fn session_row_omits_workspace_context_owned_by_workbench() {
        let mut sl = SessionRow {
            context_percent: 50.0,
            turn: 8,
            model_short: "gpt".into(),
            session_input_tokens: 32_000,
            session_output_tokens: 2_000,
            cwd_basename: "omegon".into(),
            git_branch: Some("fix/footer".into()),
            files_read: 12,
            files_modified: 4,
            phase: Some(OodaPhase::Act),
            ..Default::default()
        };
        sl.operator_hint = Some("plan active · active plan · tracked".into());
        sl.turn_state = Some("turn done".into());

        let backend = ratatui::backend::TestBackend::new(160, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                sl.render_for_level(
                    crate::surfaces::layout::UiPresentationLevel::Om,
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut text = String::new();
        for x in 0..160 {
            text.push_str(buf[(x, 0)].symbol());
        }

        assert!(text.contains("om"), "{text}");
        assert!(text.contains("turn done"), "{text}");
        assert!(!text.contains("plan active"), "{text}");
        assert!(!text.contains("active plan"), "{text}");
        assert!(!text.contains("dir omegon"), "{text}");
        assert!(!text.contains("git fix/footer"), "{text}");
        assert!(!text.contains("50%"), "{text}");
        assert!(!text.contains("gpt"), "{text}");
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

    #[test]
    fn om_row_keeps_web_search_readiness_visible() {
        let sl = SessionRow {
            provider_connected: true,
            web_search_providers: vec![("brave".into(), true), ("tavily".into(), false)],
            ..Default::default()
        };

        let backend = ratatui::backend::TestBackend::new(80, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                sl.render_for_level(
                    crate::surfaces::layout::UiPresentationLevel::Om,
                    frame.area(),
                    frame,
                    &super::super::theme::Alpharius,
                )
            })
            .unwrap();
        let text = (0..80)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(text.contains(web_readiness_glyph(true)), "{text}");
        assert!(!text.contains("WEB"), "{text}");
        assert!(text.trim_end().ends_with(&omegon_version_label()), "{text}");
    }

    #[test]
    fn unavailable_web_search_readiness_is_rendered_as_degraded() {
        let sl = SessionRow {
            provider_connected: true,
            ..Default::default()
        };

        let backend = ratatui::backend::TestBackend::new(160, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| sl.render(frame.area(), frame, &super::super::theme::Alpharius))
            .unwrap();
        let text = (0..160)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(text.contains("WEB! ddg-only"), "{text}");
    }

    #[test]
    fn keyless_web_search_is_rendered_as_degraded() {
        let sl = SessionRow {
            provider_connected: true,
            web_search_providers: vec![
                ("tavily".into(), false),
                ("serper".into(), false),
                ("brave".into(), false),
                ("firecrawl".into(), false),
            ],
            ..Default::default()
        };

        let backend = ratatui::backend::TestBackend::new(160, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| sl.render(frame.area(), frame, &super::super::theme::Alpharius))
            .unwrap();
        let text = (0..160)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(text.contains("WEB! ddg-only"), "{text}");
    }

    #[test]
    fn keyed_web_search_uses_provider_ticks() {
        let sl = SessionRow {
            provider_connected: true,
            web_search_providers: vec![
                ("tavily".into(), false),
                ("serper".into(), false),
                ("brave".into(), true),
                ("firecrawl".into(), false),
            ],
            ..Default::default()
        };

        let backend = ratatui::backend::TestBackend::new(160, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| sl.render(frame.area(), frame, &super::super::theme::Alpharius))
            .unwrap();
        let text = (0..160)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert!(text.contains("WEB ○○●○"), "{text}");
        assert!(!text.contains("ddg-only"), "{text}");
    }

    #[test]
    fn runtime_warning_row_projection_preserves_left_identity_and_right_metadata() {
        let sl = SessionRow {
            runtime_brand: "omegon".into(),
            posture: "agent".into(),
            model_provider: "anthropic".into(),
            provider_connected: false,
            principal_id: "operator".into(),
            authorization: "local".into(),
            model_tier: "balanced".into(),
            thinking_level: "minimal".into(),
            ..Default::default()
        };

        let row = sl.project_runtime_warning_row();
        assert_eq!(row.left[0].text, "runtime");
        assert_eq!(row.left[1].text, "omegon");
        assert!(
            row.left
                .iter()
                .any(|cell| cell.text == "provider anthropic offline")
        );
        assert!(row.right.iter().any(|cell| cell.text == "authz local"));
        assert!(row.right.iter().any(|cell| cell.text == "think minimal"));
    }
}
