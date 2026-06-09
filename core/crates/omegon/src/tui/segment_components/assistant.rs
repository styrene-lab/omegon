//! Assistant segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::SegmentPresentation;

use super::super::segments::{
    SegmentMeta, SegmentRenderMode, TableState, apply_rendered_links, apply_rows_bg,
    build_meta_tag, clean_inline_text, compute_table_widths, is_table_separator, render_table_line,
    split_trimmed_trailing_empty_lines, subtle_tool_row_bg, top_right_timestamp,
};
use super::super::theme::Theme;

pub struct AssistantRenderProps<'a> {
    pub text: &'a str,
    pub thinking: &'a str,
    pub complete: bool,
    pub meta: &'a SegmentMeta,
    pub presentation: &'a SegmentPresentation,
    pub mode: SegmentRenderMode,
}

pub fn render(props: AssistantRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    if area.width < 3 || area.height == 0 {
        return;
    }

    let bg = theme.surface_bg();
    let border_color = if props.complete {
        theme.success()
    } else {
        theme.accent_muted()
    };
    let block = if matches!(props.mode, SegmentRenderMode::Slim) {
        Block::default()
            .padding(Padding::horizontal(0))
            .style(Style::default().bg(bg))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(bg))
            .title_top(
                top_right_timestamp(props.meta, theme)
                    .unwrap_or_else(Line::default)
                    .right_aligned(),
            )
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(bg))
    };
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Assistant identity line — identify the source, not the current phase.
    // Slim props.mode deliberately omits this chrome so prose remains easy to select
    // and copy like a normal terminal transcript.
    if !matches!(props.mode, SegmentRenderMode::Slim) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", props.presentation.sigil),
                Style::default()
                    .fg(border_color)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("omegon", Style::default().fg(theme.border_dim()).bg(bg)),
        ]));
    }

    // Meta tag line: props.model / provider / tier — dim secondary header.
    // Hidden in slim props.mode to reduce visual noise.
    if !matches!(props.mode, SegmentRenderMode::Slim) {
        let meta_tag = build_meta_tag(props.meta);
        if !meta_tag.is_empty() {
            lines.push(Line::from(Span::styled(
                meta_tag,
                Style::default().fg(theme.border_dim()).bg(bg),
            )));
        }
    }

    // Reasoning block — stream full reasoning live, collapse after completion.
    if !props.thinking.is_empty() {
        let think_lines: Vec<&str> = split_trimmed_trailing_empty_lines(props.thinking);
        if matches!(props.mode, SegmentRenderMode::Slim) {
            let row_bg = subtle_tool_row_bg(bg);
            apply_rows_bg(inner, lines.len() as u16, 1, row_bg, buf);
            let preview = think_lines
                .iter()
                .map(|line| clean_inline_text(line.trim()))
                .find(|line| !line.is_empty())
                .unwrap_or_else(|| "thinking".to_string());
            let budget = inner.width.saturating_sub(24) as usize;
            lines.push(Line::from(vec![
                Span::styled("◌ ", Style::default().fg(theme.border()).bg(row_bg)),
                Span::styled(
                    "reasoning ",
                    Style::default()
                        .fg(theme.dim())
                        .bg(row_bg)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!("({} lines)", think_lines.len()),
                    Style::default().fg(theme.border_dim()).bg(row_bg),
                ),
                Span::styled(" · ", Style::default().fg(theme.border_dim()).bg(row_bg)),
                Span::styled(
                    crate::util::truncate(&preview, budget),
                    Style::default()
                        .fg(theme.border())
                        .bg(row_bg)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        } else {
            let show = if props.complete {
                think_lines.len().min(6)
            } else {
                think_lines.len()
            };
            lines.push(Line::from(vec![
                Span::styled("◌ ", Style::default().fg(theme.border()).bg(bg)),
                Span::styled(
                    "reasoning ",
                    Style::default()
                        .fg(theme.dim())
                        .bg(bg)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    format!("({} lines)", think_lines.len()),
                    Style::default().fg(theme.border_dim()).bg(bg),
                ),
            ]));
            for line in think_lines.iter().take(show) {
                lines.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default()
                        .fg(theme.border())
                        .bg(bg)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
            if props.complete && think_lines.len() > show {
                lines.push(Line::from(Span::styled(
                    format!("  ⋯ {} more", think_lines.len() - show),
                    Style::default().fg(theme.border_dim()).bg(bg),
                )));
            }
            lines.push(Line::from(Span::styled(
                "  ─ ─ ─",
                Style::default().fg(theme.border_dim()).bg(bg),
            )));
        }
    }

    if !props.text.is_empty() && !matches!(props.mode, SegmentRenderMode::Slim) {
        lines.push(Line::from(vec![
            Span::styled("◎ ", Style::default().fg(theme.accent()).bg(bg)),
            Span::styled(
                "answer",
                Style::default()
                    .fg(theme.accent_muted())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Assistant props.text with markdown structural highlighting.
    //
    // Pre-pass: materialize lines into a Vec so we can compute shared
    // table column widths via `compute_table_widths` before rendering.
    // The widths array is parallel to `text_lines` — entries are
    // `Some(widths)` for lines belonging to a markdown table block,
    // `None` otherwise. The rendering loop below looks up its row's
    // shared widths so every row in a table block aligns with its
    // neighbors instead of computing per-row widths in isolation
    // (which produced the column-shred failure props.mode in
    // codebase_search results and other table-bearing tool output).
    let text_lines: Vec<&str> = split_trimmed_trailing_empty_lines(props.text);
    let table_widths_per_line = compute_table_widths(&text_lines, area.width as usize);
    let mut in_code_fence = false;
    let mut table_state = TableState::None;
    for (idx, line) in text_lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            table_state = TableState::None;
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.dim()).bg(bg),
            )));
        } else if in_code_fence {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(theme.accent_muted()).bg(bg),
            )));
        } else if let Some(target_widths) = table_widths_per_line[idx].as_ref() {
            // Pre-pass marked this as a table line — render with the
            // shared widths from its block.
            let is_header = matches!(table_state, TableState::None);
            if is_table_separator(trimmed) || matches!(table_state, TableState::Header) {
                table_state = TableState::Body;
            } else {
                table_state = TableState::Header;
            }
            lines.push(render_table_line(trimmed, is_header, target_widths, theme));
        } else {
            table_state = TableState::None;
            let line = crate::tui::widgets::highlight_line(line, theme);
            let spans: Vec<Span<'_>> = line
                .spans
                .into_iter()
                .map(|mut s| {
                    s.style = s.style.bg(bg);
                    s
                })
                .collect();
            lines.push(Line::from(spans));
        }
    }

    if !props.complete && props.text.is_empty() && props.thinking.is_empty() {
        lines.push(Line::from(Span::styled("…", theme.style_dim().bg(bg))));
    }

    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
    apply_rendered_links(
        inner,
        &lines,
        buf,
        Style::default()
            .fg(theme.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        inner.height,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{SegmentEmphasis, SegmentRole};
    use crate::tui::theme::Alpharius;

    #[test]
    fn assistant_props_preserve_render_inputs() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let props = AssistantRenderProps {
            text: "answer",
            thinking: "reasoning",
            complete: false,
            meta: &meta,
            presentation: &presentation,
            mode: SegmentRenderMode::Full,
        };
        assert_eq!(props.text, "answer");
        assert_eq!(props.thinking, "reasoning");
        assert!(!props.complete);
        assert_eq!(props.presentation.sigil, "Ω");
    }

    #[test]
    fn assistant_renderer_includes_identity_and_answer() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let area = Rect::new(0, 0, 48, 8);
        let mut buf = Buffer::empty(area);
        render(
            AssistantRenderProps {
                text: "reply",
                thinking: "",
                complete: true,
                meta: &meta,
                presentation: &presentation,
                mode: SegmentRenderMode::Full,
            },
            area,
            &mut buf,
            &Alpharius,
        );
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                text.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(text.contains("Ω"), "identity should render: {text:?}");
        assert!(
            text.contains("answer"),
            "answer label should render: {text:?}"
        );
        assert!(text.contains("reply"), "reply text should render: {text:?}");
    }
}
