//! Footer bar — 4-card telemetry strip at bottom of TUI.
//!
//! Each card is a bordered Block with a title bar. Cards share `card_bg`
//! background for visual cohesion.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Padding};

use super::theme::Theme;
use super::widgets::{self, GaugeConfig};

use crate::settings::{ContextClass, ContextMode};
use crate::status::HarnessStatus;

/// Footer data — updated by the TUI on every event and rendered each frame.
#[derive(Default)]
pub struct FooterData {
    pub model_id: String,
    pub model_provider: String,
    pub context_percent: f32,
    pub context_window: usize,
    pub context_class: ContextClass,
    pub context_mode: ContextMode,
    pub total_facts: usize,
    pub injected_facts: usize,
    pub working_memory: usize,
    pub memory_tokens_est: usize,
    /// Estimated total context tokens (rough heuristic from turn + tool counts).
    pub estimated_tokens: usize,
    pub tool_calls: u32,
    pub turn: u32,
    pub compactions: u32,
    pub cwd: String,
    pub is_oauth: bool,
    /// HarnessStatus — persona, MCP, secrets, inference state.
    /// Updated via BusEvent::HarnessStatusChanged.
    pub harness: HarnessStatus,
}

impl FooterData {
    pub fn render(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let width = area.width as usize;

        // Fill the entire footer zone with card_bg background
        let bg_block = Block::default()
            .style(Style::default().bg(t.card_bg()));
        frame.render_widget(bg_block, area);

        if width < 60 {
            self.render_narrow(area, frame, t);
            return;
        }

        // 4 cards filling the width
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 4),
                Constraint::Ratio(1, 4),
                Constraint::Ratio(1, 4),
                Constraint::Min(10),
            ])
            .split(area);

        self.render_context_card(cols[0], frame, t);
        self.render_model_card(cols[1], frame, t);
        self.render_memory_card(cols[2], frame, t);
        self.render_system_card(cols[3], frame, t);
    }

    /// Card block: bordered, titled, card_bg background.
    fn card_block<'a>(title: &str, t: &dyn Theme) -> Block<'a> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border_dim()).bg(t.card_bg()))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(
                format!(" {title} "),
                Style::default().fg(t.muted()).bg(t.card_bg()),
            ))
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(t.card_bg()))
    }

    fn render_narrow(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let model_short = short_model(&self.model_id);
        let pct = self.context_percent as u32;
        let line = Line::from(vec![
            Span::styled(" Ω ", t.style_accent_bold()),
            Span::styled(format!("{model_short} "), Style::default().fg(t.muted())),
            Span::styled("│ ", Style::default().fg(t.dim())),
            Span::styled(format!("{pct}% "), Style::default().fg(
                widgets::percent_color(self.context_percent, t)
            )),
            Span::styled("│ ", Style::default().fg(t.dim())),
            Span::styled(format!("T·{} ", self.turn), Style::default().fg(t.muted())),
        ]);
        let widget = Paragraph::new(line).style(Style::default().bg(t.card_bg()));
        frame.render_widget(widget, area);
    }

    fn render_context_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("context", t);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // Gauge bar
        let bar_w = (inner.width as usize).saturating_sub(12).min(20);
        let pct = self.context_percent.min(100.0);
        let memory_blocks = if self.memory_tokens_est > 0 && self.context_window > 0 {
            let mem_pct = self.memory_tokens_est as f32 / self.context_window as f32 * 100.0;
            ((mem_pct / 100.0) * bar_w as f32) as usize
        } else {
            0
        };

        let mut bar_spans: Vec<Span<'static>> = Vec::new();
        bar_spans.extend(widgets::gauge_bar(&GaugeConfig {
            percent: pct,
            bar_width: bar_w,
            memory_blocks,
        }, t));

        let pct_str = format!(" {}%", pct as u32);
        bar_spans.push(Span::styled(pct_str, Style::default().fg(
            widgets::percent_color(pct, t)
        )));

        if self.context_window > 0 {
            bar_spans.push(Span::styled(
                format!(" / {}", widgets::format_tokens(self.context_window)),
                Style::default().fg(t.dim()),
            ));
        }
        if self.turn > 0 {
            bar_spans.push(Span::styled(format!("  T·{}", self.turn), Style::default().fg(t.dim())));
        }
        lines.push(Line::from(bar_spans));

        let widget = Paragraph::new(lines).style(Style::default().bg(t.card_bg()));
        frame.render_widget(widget, inner);
    }

    fn render_model_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("model", t);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        let model_short = short_model(&self.model_id);
        let source_icon = if self.model_provider == "local" { "⚡" } else { "☁" };
        let source_color = if self.model_provider == "local" { t.accent() } else { t.dim() };
        let auth_icon = if self.is_oauth { "●" } else { "○" };
        let auth_color = if self.is_oauth { t.success() } else { t.muted() };

        let ctx_class_color = match self.context_class {
            ContextClass::Legion => t.accent(),
            ContextClass::Clan => t.fg(),
            _ => t.dim(),
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{source_icon} "), Style::default().fg(source_color)),
            Span::styled(model_short.to_string(), Style::default().fg(t.fg()).add_modifier(Modifier::BOLD)),
            Span::styled(" · ", Style::default().fg(t.border_dim())),
            Span::styled(self.context_class.short(), Style::default().fg(ctx_class_color)),
        ]));

        // Second line: auth + persona badge
        let mut auth_parts: Vec<Span<'static>> = vec![
            Span::styled(format!("{auth_icon} "), Style::default().fg(auth_color)),
            Span::styled(
                if self.is_oauth { "subscription" } else { "api key" },
                Style::default().fg(t.muted()),
            ),
        ];

        // Persona badge
        if let Some(ref p) = self.harness.active_persona {
            auth_parts.push(Span::styled(" · ", Style::default().fg(t.border_dim())));
            auth_parts.push(Span::styled(
                format!("{} {}", p.badge, p.name),
                Style::default().fg(t.accent()),
            ));
        }
        // Tone badge
        if let Some(ref tone) = self.harness.active_tone {
            auth_parts.push(Span::styled(" · ", Style::default().fg(t.border_dim())));
            auth_parts.push(Span::styled(
                format!("♪ {}", tone.name),
                Style::default().fg(t.dim()),
            ));
        }

        lines.push(Line::from(auth_parts));

        let widget = Paragraph::new(lines).style(Style::default().bg(t.card_bg()));
        frame.render_widget(widget, inner);
    }

    fn render_memory_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("memory", t);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        let sep = Span::styled(" · ", Style::default().fg(t.dim()));
        let mut parts: Vec<Span<'static>> = vec![
            Span::styled("⌗ ", Style::default().fg(t.accent())),
            Span::styled(format!("{}", self.total_facts), Style::default().fg(t.muted())),
        ];

        if self.injected_facts > 0 {
            parts.push(sep.clone());
            parts.push(Span::styled("inj ", Style::default().fg(t.dim())));
            parts.push(Span::styled(format!("{}", self.injected_facts), Style::default().fg(t.muted())));
        }

        if self.working_memory > 0 {
            parts.push(sep.clone());
            parts.push(Span::styled("wm ", Style::default().fg(t.dim())));
            parts.push(Span::styled(format!("{}", self.working_memory), Style::default().fg(t.muted())));
        }

        if self.memory_tokens_est > 0 {
            parts.push(sep);
            parts.push(Span::styled(
                format!("~{}", widgets::format_tokens(self.memory_tokens_est)),
                Style::default().fg(t.dim()),
            ));
        }

        lines.push(Line::from(parts));

        let widget = Paragraph::new(lines).style(Style::default().bg(t.card_bg()));
        frame.render_widget(widget, inner);
    }

    fn render_system_card(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let block = Self::card_block("system", t);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line<'static>> = Vec::new();

        // cwd — shorten home dir
        let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();
        let display_cwd = if !home.is_empty() && self.cwd.starts_with(&home) {
            format!("~{}", &self.cwd[home.len()..])
        } else {
            self.cwd.clone()
        };
        lines.push(Line::from(vec![
            Span::styled("⌂ ", Style::default().fg(t.dim())),
            Span::styled(display_cwd, Style::default().fg(t.muted())),
        ]));

        // Second line: MCP + secrets + tool calls + compactions
        {
            let mut parts: Vec<Span<'static>> = Vec::new();

            // MCP servers
            let mcp_connected = self.harness.mcp_servers.iter().filter(|s| s.connected).count();
            if mcp_connected > 0 {
                let tool_count = self.harness.mcp_tool_count();
                parts.push(Span::styled("MCP ", Style::default().fg(t.dim())));
                parts.push(Span::styled(
                    format!("{}({}t)", mcp_connected, tool_count),
                    Style::default().fg(t.accent()),
                ));
            }

            // Secrets
            if let Some(ref sec) = self.harness.secret_backend {
                if !parts.is_empty() {
                    parts.push(Span::styled(" · ", Style::default().fg(t.dim())));
                }
                let icon = if sec.locked { "🔒" } else { "🔓" };
                parts.push(Span::styled(
                    format!("{icon} {}", sec.stored_count),
                    Style::default().fg(t.muted()),
                ));
            }

            // Tool calls
            if self.tool_calls > 0 {
                if !parts.is_empty() {
                    parts.push(Span::styled(" · ", Style::default().fg(t.dim())));
                }
                parts.push(Span::styled("⚙ ", Style::default().fg(t.dim())));
                parts.push(Span::styled(format!("{}", self.tool_calls), Style::default().fg(t.muted())));
            }

            // Compactions
            if self.compactions > 0 {
                if !parts.is_empty() {
                    parts.push(Span::styled(" · ", Style::default().fg(t.dim())));
                }
                parts.push(Span::styled("↻ ", Style::default().fg(t.dim())));
                parts.push(Span::styled(format!("{}", self.compactions), Style::default().fg(t.muted())));
            }

            if !parts.is_empty() {
                lines.push(Line::from(parts));
            }
        }

        let widget = Paragraph::new(lines).style(Style::default().bg(t.card_bg()));
        frame.render_widget(widget, inner);
    }
}

/// Extract short model name from full ID.
fn short_model(model_id: &str) -> &str {
    model_id.split(':').next_back()
        .or_else(|| model_id.split('/').next_back())
        .unwrap_or(model_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn footer_renders_without_panic() {
        let data = FooterData {
            model_id: "claude-sonnet-4-6".into(),
            model_provider: "anthropic".into(),
            context_percent: 45.0,
            context_window: 200_000,
            total_facts: 150,
            turn: 5,
            tool_calls: 12,
            ..Default::default()
        };
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            data.render(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
    }

    #[test]
    fn footer_narrow_terminal() {
        let data = FooterData::default();
        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            data.render(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
    }

    #[test]
    fn footer_shows_model() {
        let data = FooterData {
            model_id: "claude-opus-4-6".into(),
            model_provider: "anthropic".into(),
            ..Default::default()
        };
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            data.render(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text: String = { let buf = terminal.backend().buffer(); let a = buf.area; (0..a.height).flat_map(|y| (0..a.width).map(move |x| buf[(x, y)].symbol().to_string())).collect() };
        assert!(text.contains("opus"), "should show model: {text}");
    }

    #[test]
    fn footer_shows_context_percent() {
        let data = FooterData {
            context_percent: 75.0,
            context_window: 200_000,
            ..Default::default()
        };
        let backend = TestBackend::new(120, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            data.render(frame.area(), frame, &super::super::theme::Alpharius);
        }).unwrap();
        
        let text: String = { let buf = terminal.backend().buffer(); let a = buf.area; (0..a.height).flat_map(|y| (0..a.width).map(move |x| buf[(x, y)].symbol().to_string())).collect() };
        assert!(text.contains("75") || text.contains("200k"), "should show context info: {text}");
    }

    #[test]
    fn cwd_default_is_empty() {
        let data = FooterData::default();
        assert!(data.model_id.is_empty());
        assert_eq!(data.context_percent, 0.0);
    }
}
