//! Assistant segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::SegmentPresentation;

use super::super::conversation_render_projection::SegmentRenderContext;
use super::super::segments::{
    SegmentMeta, SegmentRenderMode, TableState, apply_rendered_links, build_meta_tag,
    clean_inline_text, compute_table_widths, is_table_separator, render_table_line,
    split_trimmed_trailing_empty_lines, subtle_tool_row_bg, top_right_timestamp,
};
use super::compact_row;

pub struct AssistantRenderProps<'a> {
    pub text: &'a str,
    pub thinking: &'a str,
    pub complete: bool,
    pub meta: &'a SegmentMeta,
    pub presentation: &'a SegmentPresentation,
    pub mode: SegmentRenderMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssistantRenderPlan {
    pub chrome: AssistantChrome,
    pub reasoning: AssistantReasoning,
    pub body: AssistantBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssistantChrome {
    pub bordered: bool,
    pub identity_line: bool,
    pub meta_line: bool,
    pub answer_label: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantReasoning {
    Hidden,
    SlimExpanded { max_completed_lines: usize },
    Expanded { max_completed_lines: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantBody {
    MarkdownTranscript,
}

pub fn plan(props: &AssistantRenderProps<'_>) -> AssistantRenderPlan {
    let slim = matches!(props.mode, SegmentRenderMode::Slim);
    AssistantRenderPlan {
        chrome: AssistantChrome {
            bordered: !slim,
            identity_line: !slim,
            meta_line: !slim && !build_meta_tag(props.meta).is_empty(),
            answer_label: !slim && !props.text.is_empty(),
        },
        reasoning: if props.thinking.is_empty() {
            AssistantReasoning::Hidden
        } else if slim {
            AssistantReasoning::SlimExpanded {
                max_completed_lines: 4,
            }
        } else {
            AssistantReasoning::Expanded {
                max_completed_lines: 6,
            }
        },
        body: AssistantBody::MarkdownTranscript,
    }
}

fn assistant_block<'a>(
    props: &AssistantRenderProps<'_>,
    render_plan: AssistantRenderPlan,
    theme: &'a dyn crate::tui::theme::Theme,
    bg: Color,
    border_color: Color,
) -> Block<'a> {
    if !render_plan.chrome.bordered {
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
    }
}

fn presentation_label(presentation: &SegmentPresentation) -> &'static str {
    match presentation.role {
        crate::surfaces::conversation::SegmentRole::Assistant => "omegon",
        crate::surfaces::conversation::SegmentRole::PeerAgent => "peer agent",
        _ => "omegon",
    }
}

fn push_identity_line<'a>(
    lines: &mut Vec<Line<'a>>,
    props: &AssistantRenderProps<'_>,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
    border_color: Color,
) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", props.presentation.sigil),
            Style::default()
                .fg(border_color)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            presentation_label(props.presentation),
            Style::default().fg(theme.border_dim()).bg(bg),
        ),
    ]));
}

fn push_meta_line<'a>(
    lines: &mut Vec<Line<'a>>,
    props: &AssistantRenderProps<'_>,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
) {
    lines.push(Line::from(Span::styled(
        build_meta_tag(props.meta),
        Style::default().fg(theme.border_dim()).bg(bg),
    )));
}

fn push_reasoning_lines<'a>(
    lines: &mut Vec<Line<'a>>,
    props: &AssistantRenderProps<'_>,
    reasoning: AssistantReasoning,
    inner: Rect,
    buf: &mut Buffer,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
) {
    match reasoning {
        AssistantReasoning::Hidden => {}
        AssistantReasoning::SlimExpanded {
            max_completed_lines,
        } => {
            let think_lines: Vec<&str> = split_trimmed_trailing_empty_lines(props.thinking);
            let show = if props.complete {
                think_lines.len().min(max_completed_lines)
            } else {
                think_lines.len()
            };
            let mut detail_rows = Vec::with_capacity(show.saturating_add(1));
            detail_rows.push(format!(
                "{} line{}",
                think_lines.len(),
                if think_lines.len() == 1 { "" } else { "s" }
            ));
            detail_rows.extend(
                think_lines
                    .iter()
                    .take(show)
                    .map(|line| clean_inline_text(line.trim())),
            );
            if props.complete && think_lines.len() > show {
                detail_rows.push(format!("⋯ {} more", think_lines.len() - show));
            }
            compact_row::render(
                Rect {
                    x: inner.x,
                    y: inner.y + lines.len() as u16,
                    width: inner.width,
                    height: inner.height.saturating_sub(lines.len() as u16),
                },
                buf,
                theme,
                bg,
                subtle_tool_row_bg(bg),
                compact_row::CompactRows::metadata("reasoning", theme.border(), &detail_rows),
            );
            lines.extend((0..detail_rows.len()).map(|_| Line::default()));
        }
        AssistantReasoning::Expanded {
            max_completed_lines,
        } => {
            let think_lines: Vec<&str> = split_trimmed_trailing_empty_lines(props.thinking);
            let show = if props.complete {
                think_lines.len().min(max_completed_lines)
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
}

fn push_answer_label<'a>(
    lines: &mut Vec<Line<'a>>,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
) {
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

fn push_answer_body_lines<'a>(
    lines: &mut Vec<Line<'a>>,
    text: &'a str,
    width: usize,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
) {
    let text_lines: Vec<&str> = split_trimmed_trailing_empty_lines(text);
    let table_widths_per_line = compute_table_widths(&text_lines, width);
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
}

pub fn render(
    props: AssistantRenderProps<'_>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &SegmentRenderContext<'_>,
) {
    let theme = ctx.theme;
    if area.width < 3 || area.height == 0 {
        return;
    }

    let render_plan = plan(&props);
    let bg = theme.surface_bg();
    let border_color = if props.complete {
        theme.success()
    } else {
        theme.accent_muted()
    };
    let block = assistant_block(&props, render_plan, theme, bg, border_color);
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    if render_plan.chrome.identity_line {
        push_identity_line(&mut lines, &props, theme, bg, border_color);
    }
    if render_plan.chrome.meta_line {
        push_meta_line(&mut lines, &props, theme, bg);
    }
    push_reasoning_lines(
        &mut lines,
        &props,
        render_plan.reasoning,
        inner,
        buf,
        theme,
        bg,
    );
    if render_plan.chrome.answer_label {
        push_answer_label(&mut lines, theme, bg);
    }
    match render_plan.body {
        AssistantBody::MarkdownTranscript => {
            push_answer_body_lines(&mut lines, props.text, area.width as usize, theme, bg);
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
    fn assistant_plan_slim_omits_chrome_and_expands_reasoning() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let props = AssistantRenderProps {
            text: "reply",
            thinking: "reasoning",
            complete: false,
            meta: &meta,
            presentation: &presentation,
            mode: SegmentRenderMode::Slim,
        };

        let plan = super::plan(&props);
        assert!(!plan.chrome.bordered);
        assert!(!plan.chrome.identity_line);
        assert!(!plan.chrome.meta_line);
        assert!(!plan.chrome.answer_label);
        assert_eq!(
            plan.reasoning,
            AssistantReasoning::SlimExpanded {
                max_completed_lines: 4
            }
        );
        assert_eq!(plan.body, AssistantBody::MarkdownTranscript);
    }

    #[test]
    fn assistant_plan_full_uses_chrome_and_expanded_reasoning() {
        let meta = SegmentMeta {
            model_id: Some("model".into()),
            ..Default::default()
        };
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let props = AssistantRenderProps {
            text: "reply",
            thinking: "reasoning",
            complete: true,
            meta: &meta,
            presentation: &presentation,
            mode: SegmentRenderMode::Full,
        };

        let plan = super::plan(&props);
        assert!(plan.chrome.bordered);
        assert!(plan.chrome.identity_line);
        assert!(plan.chrome.meta_line);
        assert!(plan.chrome.answer_label);
        assert_eq!(
            plan.reasoning,
            AssistantReasoning::Expanded {
                max_completed_lines: 6
            }
        );
    }

    #[test]
    fn assistant_plan_hides_empty_reasoning() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let props = AssistantRenderProps {
            text: "reply",
            thinking: "",
            complete: true,
            meta: &meta,
            presentation: &presentation,
            mode: SegmentRenderMode::Full,
        };

        let plan = super::plan(&props);
        assert_eq!(plan.reasoning, AssistantReasoning::Hidden);
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
            &SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Full),
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
