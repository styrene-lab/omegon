//! Tool inspection panel rendering for slim/full TUI layouts.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::theme;

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

fn visible_tail(lines: &[String], max_tail: usize) -> &[String] {
    let start = lines.len().saturating_sub(max_tail);
    &lines[start..]
}

fn append_visible_tail(
    out: &mut Vec<Line<'_>>,
    source_lines: &[String],
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

    let visible = visible_tail(source_lines, max_tail);
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
                    out,
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
            out,
            prefix,
            &strip_control_preserving_tabs(line),
            text_budget,
            style.tail_style,
            style.bg,
            t,
        );
    }
}

pub struct ToolInspection<'a> {
    pub name: &'a str,
    pub state: crate::tui::glyphs::ToolStateGlyphRole,
    pub title_prefix: &'a str,
    pub elapsed: Option<std::time::Duration>,
    pub content_form: crate::surfaces::conversation::ContentForm,
    pub lines: &'a [String],
}

pub fn tool_inspection_height(line_count: usize) -> u16 {
    // Reserve the header immediately on ToolStart so operators can see that
    // the running tool has a live region even before the first partial arrives.
    1 + (line_count as u16).min(15)
}

pub fn tool_content_form(
    name: &str,
    lines: &[String],
    complete: bool,
    is_error: bool,
) -> crate::surfaces::conversation::ContentForm {
    let joined = lines.join("\n");
    let detail_result = if joined.is_empty() {
        None
    } else {
        Some(joined.as_str())
    };
    let projection = crate::surfaces::conversation::ConversationSegmentProjection::<
        &str,
        std::path::PathBuf,
    >::new(
        crate::surfaces::conversation::ConversationSegmentKind::Tool(
            crate::surfaces::conversation::ToolSegment {
                id: "",
                name,
                args_summary: None,
                detail_args: None,
                result_summary: detail_result,
                detail_result,
                is_error,
                complete,
                expanded: false,
            },
        ),
    );
    projection.presentation_model().content.form
}

pub fn render_tool_inspection_panel(
    area: Rect,
    buf: &mut Buffer,
    t: &dyn theme::Theme,
    inspection: ToolInspection<'_>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bg = t.surface_bg();
    let mut lines = Vec::new();
    let glyphs = crate::tui::glyphs::glyphs();
    let category = glyphs.tool_category(crate::tui::glyphs::tool_category_role_for_name(
        inspection.name,
    ));
    let state = glyphs.tool_state(inspection.state);
    let title = match inspection.content_form {
        crate::surfaces::conversation::ContentForm::Log => {
            format!("{} log", inspection.title_prefix)
        }
        crate::surfaces::conversation::ContentForm::Diff => {
            format!("{} diff", inspection.title_prefix)
        }
        crate::surfaces::conversation::ContentForm::Json => {
            format!("{} json", inspection.title_prefix)
        }
        crate::surfaces::conversation::ContentForm::Markdown => {
            format!("{} doc", inspection.title_prefix)
        }
        crate::surfaces::conversation::ContentForm::Empty => {
            format!("{} tool", inspection.title_prefix)
        }
        _ => format!("{} tool", inspection.title_prefix),
    };
    let mut spec = crate::tui::horizontal_line::HorizontalLineSpec::title(title)
        .with_title_emphasis(crate::tui::horizontal_line::LineEmphasis::Muted)
        .with_metric(crate::tui::horizontal_line::LineMetric::new("", state))
        .with_metric(crate::tui::horizontal_line::LineMetric::new("", category))
        .with_metric(
            crate::tui::horizontal_line::LineMetric::new("", inspection.name)
                .with_emphasis(crate::tui::horizontal_line::LineEmphasis::Strong),
        );
    if let Some(elapsed) = inspection.elapsed {
        spec = spec.with_metric(crate::tui::horizontal_line::LineMetric::new(
            "",
            format_short_elapsed(elapsed),
        ));
    }
    lines.push(crate::tui::horizontal_line::horizontal_line(
        spec, area.width, t, bg,
    ));
    let max_tail = area.height.saturating_sub(1) as usize;
    let text_budget = area.width.saturating_sub(4) as usize;
    let tail_style = match inspection.content_form {
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
        inspection.lines,
        max_tail,
        text_budget,
        TailRenderStyle {
            content_form: inspection.content_form,
            tail_style,
            bg,
        },
        t,
    );

    Paragraph::new(lines)
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_inspection_to_string(
        name: &str,
        content_form: crate::surfaces::conversation::ContentForm,
        lines: &[String],
        width: u16,
        height: u16,
    ) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render_tool_inspection_panel(
                    frame.area(),
                    frame.buffer_mut(),
                    &theme::Alpharius,
                    ToolInspection {
                        name,
                        state: crate::tui::glyphs::ToolStateGlyphRole::Running,
                        title_prefix: "live",
                        elapsed: None,
                        content_form,
                        lines,
                    },
                );
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
    fn tool_inspection_reserves_header_and_caps_tail_height() {
        assert_eq!(tool_inspection_height(0), 1);
        assert_eq!(tool_inspection_height(20), 16);
        let lines = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>();
        assert_eq!(visible_tail(&lines, 3), &["line 17", "line 18", "line 19"]);
    }

    #[test]
    fn tool_inspection_uses_log_presentation_for_bash() {
        let lines =
            vec!["Smoke reference daemon launcher\tpending\t0\thttps://example.test/job".into()];
        let rendered = render_inspection_to_string(
            "bash",
            crate::surfaces::conversation::ContentForm::Log,
            &lines,
            100,
            4,
        );

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
    fn tool_inspection_sanitizes_ansi_tail_without_raw_escape_noise() {
        let lines = vec!["\u{1b}[32mpass\u{1b}[0m\t4s\thttps://example.test/run".into()];
        let rendered = render_inspection_to_string(
            "bash",
            crate::surfaces::conversation::ContentForm::Log,
            &lines,
            80,
            4,
        );

        assert!(rendered.contains("pass"), "{rendered}");
        assert!(rendered.contains("4s"), "{rendered}");
        assert!(!rendered.contains("[32m"), "{rendered}");
        assert!(!rendered.contains("[0m"), "{rendered}");
    }

    #[test]
    fn tool_content_form_classifies_full_json_tail_not_last_line_only() {
        let lines = vec!["{".into(), "  \"ok\": true".into(), "}".into()];

        assert_eq!(
            tool_content_form("browser_search_receive", &lines, false, false),
            crate::surfaces::conversation::ContentForm::Json
        );
        let rendered = render_inspection_to_string(
            "browser_search_receive",
            crate::surfaces::conversation::ContentForm::Json,
            &lines,
            80,
            5,
        );
        assert!(rendered.contains("live json"), "{rendered}");
    }
}
