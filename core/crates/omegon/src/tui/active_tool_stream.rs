//! Active tool stream lane rendering for slim/full TUI layouts.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveToolStream {
    pub id: String,
    pub name: String,
    pub started_at: std::time::Instant,
    lines: Vec<String>,
}

impl ActiveToolStream {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            started_at: std::time::Instant::now(),
            lines: Vec::new(),
        }
    }

    pub fn update(&mut self, partial: &omegon_traits::PartialToolResult) {
        if partial.tail.trim().is_empty() {
            return;
        }
        self.lines = partial.tail.lines().map(str::to_string).collect();
    }

    pub fn visible_lines(&self, max_lines: usize) -> &[String] {
        let start = self.lines.len().saturating_sub(max_lines);
        &self.lines[start..]
    }

    pub fn height(&self) -> u16 {
        // Reserve the header immediately on ToolStart so operators can see
        // that the running tool has a live region even before the first
        // stdout/stderr partial arrives.
        1 + (self.lines.len() as u16).min(15)
    }
}

fn format_short_elapsed(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        let mins = secs / 60;
        let rem = secs % 60;
        format!("{mins}m{rem:02}s")
    }
}

pub fn render_active_tool_stream_panel(
    area: Rect,
    frame: &mut Frame,
    t: &dyn theme::Theme,
    stream: &ActiveToolStream,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bg = t.surface_bg();
    let mut lines = Vec::new();
    let glyphs = crate::tui::glyphs::glyphs();
    let category = glyphs.tool_category(crate::tui::glyphs::tool_category_role_for_name(&stream.name));
    let running = glyphs.tool_state(crate::tui::glyphs::ToolStateGlyphRole::Running);
    lines.push(crate::tui::horizontal_line::horizontal_line(
        crate::tui::horizontal_line::HorizontalLineSpec::title("active tool")
            .with_title_emphasis(crate::tui::horizontal_line::LineEmphasis::Muted)
            .with_metric(crate::tui::horizontal_line::LineMetric::new("", running))
            .with_metric(crate::tui::horizontal_line::LineMetric::new("", category))
            .with_metric(
                crate::tui::horizontal_line::LineMetric::new("", stream.name.as_str())
                    .with_emphasis(crate::tui::horizontal_line::LineEmphasis::Strong),
            )
            .with_metric(crate::tui::horizontal_line::LineMetric::new(
                "",
                format_short_elapsed(stream.started_at.elapsed()),
            )),
        area.width,
        t,
        bg,
    ));
    let max_tail = area.height.saturating_sub(1) as usize;
    let text_budget = area.width.saturating_sub(2) as usize;
    for line in stream.visible_lines(max_tail) {
        lines.push(Line::from(Span::styled(
            crate::util::truncate(line, text_budget),
            Style::default().fg(t.fg()).bg(bg),
        )));
    }

    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, frame.buffer_mut());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_tool_stream_reserves_header_and_caps_tail() {
        let mut stream = ActiveToolStream::new("1", "bash");
        assert_eq!(stream.height(), 1);
        stream.lines = (0..20).map(|i| format!("line {i}")).collect();
        assert_eq!(stream.height(), 16);
        assert_eq!(stream.visible_lines(3), &["line 17", "line 18", "line 19"]);
    }
}
