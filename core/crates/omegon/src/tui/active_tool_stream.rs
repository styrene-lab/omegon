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

    pub fn semantic_content_form(&self) -> crate::surfaces::conversation::ContentForm {
        let detail_result = self.lines.join("\n");
        let detail_result = if detail_result.is_empty() {
            None
        } else {
            Some(detail_result.as_str())
        };
        let projection = crate::surfaces::conversation::ConversationSegmentProjection::<
            &str,
            std::path::PathBuf,
        >::new(
            crate::surfaces::conversation::ConversationSegmentKind::Tool(
                crate::surfaces::conversation::ToolSegment {
                    id: self.id.as_str(),
                    name: self.name.as_str(),
                    args_summary: None,
                    detail_args: None,
                    result_summary: detail_result,
                    detail_result,
                    is_error: false,
                    complete: false,
                    expanded: false,
                },
            ),
        );
        projection.presentation_model().content.form
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

fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut col = 0usize;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let spaces = 4 - (col % 4);
                out.extend(std::iter::repeat_n(' ', spaces));
                col += spaces;
            }
            '\n' | '\r' => {
                out.push(ch);
                col = 0;
            }
            _ => {
                out.push(ch);
                col += 1;
            }
        }
    }
    out
}

fn strip_control_preserving_tabs(text: &str) -> String {
    let without_control: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\t'))
        .collect();
    expand_tabs(&without_control)
}

fn append_tail_line(
    lines: &mut Vec<Line<'_>>,
    prefix: &'static str,
    text: &str,
    text_budget: usize,
    tail_style: Style,
    bg: Color,
    t: &dyn theme::Theme,
) {
    lines.push(Line::from(vec![
        Span::styled(prefix, Style::default().fg(t.dim()).bg(bg)),
        Span::styled(crate::util::truncate(text, text_budget), tail_style),
    ]));
}

struct TailRenderStyle {
    content_form: crate::surfaces::conversation::ContentForm,
    tail_style: Style,
    bg: Color,
}

fn append_visible_tail(
    lines: &mut Vec<Line<'_>>,
    stream: &ActiveToolStream,
    max_tail: usize,
    text_budget: usize,
    style: TailRenderStyle,
    t: &dyn theme::Theme,
) {
    let prefix = match style.content_form {
        crate::surfaces::conversation::ContentForm::Log => "│ ",
        crate::surfaces::conversation::ContentForm::Diff => "╎ ",
        crate::surfaces::conversation::ContentForm::Json
        | crate::surfaces::conversation::ContentForm::Structured => "· ",
        _ => "  ",
    };

    let visible = stream.visible_lines(max_tail);
    let visible_tail = visible.join("\n");
    if visible_tail.contains('\x1b') {
        use ansi_to_tui::IntoText as _;
        if let Ok(text) = visible_tail.clone().into_text() {
            for line in text.lines {
                let plain = line
                    .spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>();
                append_tail_line(
                    lines,
                    prefix,
                    &expand_tabs(&plain),
                    text_budget,
                    style.tail_style,
                    style.bg,
                    t,
                );
            }
            return;
        }
    }

    for line in visible {
        append_tail_line(
            lines,
            prefix,
            &strip_control_preserving_tabs(line),
            text_budget,
            style.tail_style,
            style.bg,
            t,
        );
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
    let content_form = stream.semantic_content_form();
    let (category, tool_name) = (
        glyphs.tool_category(crate::tui::glyphs::tool_category_role_for_name(
            &stream.name,
        )),
        stream.name.as_str(),
    );
    let running = glyphs.tool_state(crate::tui::glyphs::ToolStateGlyphRole::Running);
    let title = match content_form {
        crate::surfaces::conversation::ContentForm::Log => "live log",
        crate::surfaces::conversation::ContentForm::Diff => "live diff",
        crate::surfaces::conversation::ContentForm::Json => "live json",
        crate::surfaces::conversation::ContentForm::Markdown => "live doc",
        crate::surfaces::conversation::ContentForm::Empty => "active tool",
        _ => "live tool",
    };
    lines.push(crate::tui::horizontal_line::horizontal_line(
        crate::tui::horizontal_line::HorizontalLineSpec::title(title)
            .with_title_emphasis(crate::tui::horizontal_line::LineEmphasis::Muted)
            .with_metric(crate::tui::horizontal_line::LineMetric::new("", running))
            .with_metric(crate::tui::horizontal_line::LineMetric::new("", category))
            .with_metric(
                crate::tui::horizontal_line::LineMetric::new("", tool_name)
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
    let text_budget = area.width.saturating_sub(4) as usize;
    let tail_style = match content_form {
        crate::surfaces::conversation::ContentForm::Log => Style::default().fg(t.muted()).bg(bg),
        crate::surfaces::conversation::ContentForm::Diff => Style::default().fg(t.accent()).bg(bg),
        crate::surfaces::conversation::ContentForm::Json
        | crate::surfaces::conversation::ContentForm::Structured => {
            Style::default().fg(t.fg()).bg(bg)
        }
        _ => Style::default().fg(t.muted()).bg(bg),
    };
    append_visible_tail(
        &mut lines,
        stream,
        max_tail,
        text_budget,
        TailRenderStyle {
            content_form,
            tail_style,
            bg,
        },
        t,
    );

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

    fn render_stream_to_string(stream: &ActiveToolStream, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_active_tool_stream_panel(frame.area(), frame, &theme::Alpharius, stream);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let area = buf.area;
        let mut rows = Vec::new();
        for y in 0..area.height {
            let row = (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string();
            rows.push(row);
        }
        rows.join("\n")
    }

    #[test]
    fn active_tool_stream_uses_log_presentation_for_bash() {
        let mut stream = ActiveToolStream::new("1", "bash");
        stream.lines =
            vec!["Smoke reference daemon launcher\tpending\t0\thttps://example.test/job".into()];

        let rendered = render_stream_to_string(&stream, 100, 4);

        assert!(rendered.contains("live log"), "{rendered}");
        assert!(rendered.contains("bash"), "{rendered}");
        assert!(
            rendered.contains("│ Smoke reference daemon launcher"),
            "{rendered}"
        );
        assert!(rendered.contains("pending"), "{rendered}");
        assert!(!rendered.contains('\t'), "{rendered}");
    }

    #[test]
    fn active_tool_stream_sanitizes_ansi_tail_without_raw_escape_noise() {
        let mut stream = ActiveToolStream::new("1", "bash");
        stream.lines = vec!["\u{1b}[32mpass\u{1b}[0m\t4s\thttps://example.test/run".into()];

        let rendered = render_stream_to_string(&stream, 80, 4);

        assert!(rendered.contains("pass"), "{rendered}");
        assert!(rendered.contains("4s"), "{rendered}");
        assert!(!rendered.contains("[32m"), "{rendered}");
        assert!(!rendered.contains("[0m"), "{rendered}");
    }

    #[test]
    fn active_tool_stream_classifies_full_json_tail_not_last_line_only() {
        let mut stream = ActiveToolStream::new("1", "browser_search_receive");
        stream.lines = vec!["{".into(), "  \"ok\": true".into(), "}".into()];

        assert_eq!(
            stream.semantic_content_form(),
            crate::surfaces::conversation::ContentForm::Json
        );
        let rendered = render_stream_to_string(&stream, 80, 5);
        assert!(rendered.contains("live json"), "{rendered}");
    }
}
