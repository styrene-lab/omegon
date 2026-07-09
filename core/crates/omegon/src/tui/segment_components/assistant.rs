//! Assistant segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::{SegmentPresentation, SegmentSurfacePolicy};

use super::super::conversation_render_projection::{SegmentRenderContext, terminal_segment_paint};
use super::super::segments::{
    SegmentMeta, SegmentRenderMode, TableState, apply_rendered_links, build_meta_tag,
    clean_inline_text, compute_table_widths, is_table_separator, render_table_line,
    split_trimmed_trailing_empty_lines, top_right_timestamp,
};
use super::compact_row;

pub struct AssistantRenderProps<'a> {
    pub text: &'a str,
    pub thinking: &'a str,
    pub complete: bool,
    pub meta: &'a SegmentMeta,
    pub presentation: &'a SegmentPresentation,
    pub surface: SegmentSurfacePolicy,
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

fn is_reasoning_artifact_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "<think>" | "</think>" | "<thinking>" | "</thinking>"
    ) || (trimmed.starts_with("<!--") && trimmed.ends_with("-->"))
}

fn strip_reasoning_inline_markup(text: &str) -> String {
    let cleaned = clean_inline_text(text);
    let trimmed = cleaned.trim();
    if trimmed.len() >= 4 && trimmed.starts_with("**") && trimmed.ends_with("**") {
        trimmed
            .trim_start_matches('*')
            .trim_end_matches('*')
            .trim()
            .to_string()
    } else {
        cleaned
    }
}

pub fn sanitized_reasoning_lines(thinking: &str) -> Vec<String> {
    split_trimmed_trailing_empty_lines(thinking)
        .into_iter()
        .filter_map(|line| {
            if is_reasoning_artifact_line(line) {
                return None;
            }
            let cleaned = strip_reasoning_inline_markup(line.trim());
            if cleaned.trim().is_empty() {
                None
            } else {
                Some(cleaned)
            }
        })
        .collect()
}

fn sanitized_reasoning_display_lines(thinking: &str) -> Vec<String> {
    sanitized_reasoning_lines(thinking)
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

pub fn slim_reasoning_detail_rows(
    thinking: &str,
    complete: bool,
    max_completed_lines: usize,
) -> Vec<String> {
    let think_lines = sanitized_reasoning_display_lines(thinking);
    let show = if complete {
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
    detail_rows.extend(think_lines.iter().take(show).cloned());
    if complete && think_lines.len() > show {
        detail_rows.push(format!("⋯ {} more", think_lines.len() - show));
    }
    detail_rows
}

fn push_reasoning_lines<'a>(
    lines: &mut Vec<Line<'a>>,
    props: &AssistantRenderProps<'_>,
    reasoning: AssistantReasoning,
    inner: Rect,
    theme: &dyn crate::tui::theme::Theme,
    bg: Color,
) {
    match reasoning {
        AssistantReasoning::Hidden => {}
        AssistantReasoning::SlimExpanded {
            max_completed_lines,
        } => {
            let detail_rows =
                slim_reasoning_detail_rows(props.thinking, props.complete, max_completed_lines);
            let prefix = compact_row::prefix_width("", "reasoning", false);
            for (idx, detail) in detail_rows.iter().enumerate() {
                if idx == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(
                            compact_row::label("", "reasoning"),
                            Style::default()
                                .fg(theme.border())
                                .bg(bg)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::styled(" · ", Style::default().fg(theme.dim()).bg(bg)),
                        Span::styled(
                            compact_row::first_detail_row(inner.width, prefix, detail),
                            Style::default().fg(theme.muted()).bg(bg),
                        ),
                    ]));
                } else {
                    let budget = inner.width.saturating_sub(2).saturating_sub(2) as usize;
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default().fg(theme.dim()).bg(bg)),
                        Span::styled(
                            compact_row::truncate_to_width(detail, budget),
                            Style::default().fg(theme.dim()).bg(bg),
                        ),
                    ]));
                }
            }
        }
        AssistantReasoning::Expanded {
            max_completed_lines,
        } => {
            let think_lines = sanitized_reasoning_display_lines(props.thinking);
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
    let paint = terminal_segment_paint(props.surface, ctx);
    let bg = paint.text_bg.unwrap_or(paint.clear_bg);
    let block_bg = paint.surface_bg.unwrap_or(paint.clear_bg);
    let border_color = if props.complete {
        theme.success()
    } else {
        theme.accent_muted()
    };
    let block = assistant_block(&props, render_plan, theme, block_bg, border_color);
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut content_area = inner;
    if let AssistantReasoning::SlimExpanded {
        max_completed_lines,
    } = render_plan.reasoning
    {
        let detail_rows =
            slim_reasoning_detail_rows(props.thinking, props.complete, max_completed_lines);
        let rows = compact_row::CompactRows::metadata("reasoning", theme.border(), &detail_rows);
        let reasoning_height = compact_row::measured_height(inner.width, &rows).min(inner.height);
        if reasoning_height > 0 {
            let reasoning_area = Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: reasoning_height,
            };
            compact_row::render(reasoning_area, buf, theme, bg, bg, rows);
            content_area = Rect {
                x: inner.x,
                y: inner.y.saturating_add(reasoning_height),
                width: inner.width,
                height: inner.height.saturating_sub(reasoning_height),
            };
        }
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    if render_plan.chrome.identity_line {
        push_identity_line(&mut lines, &props, theme, bg, border_color);
    }
    if render_plan.chrome.meta_line {
        push_meta_line(&mut lines, &props, theme, bg);
    }
    if !matches!(
        render_plan.reasoning,
        AssistantReasoning::SlimExpanded { .. }
    ) {
        push_reasoning_lines(&mut lines, &props, render_plan.reasoning, inner, theme, bg);
    }
    if render_plan.chrome.answer_label {
        push_answer_label(&mut lines, theme, bg);
    }
    match render_plan.body {
        AssistantBody::MarkdownTranscript => {
            push_answer_body_lines(
                &mut lines,
                props.text,
                content_area.width as usize,
                theme,
                bg,
            );
        }
    }

    if !props.complete && props.text.is_empty() && props.thinking.is_empty() {
        lines.push(Line::from(Span::styled("…", theme.style_dim().bg(bg))));
    }

    if content_area.height == 0 {
        return;
    }

    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(content_area, buf);
    apply_rendered_links(
        content_area,
        &lines,
        buf,
        Style::default()
            .fg(theme.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        content_area.height,
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
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
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
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
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
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
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
            surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
            },
            mode: SegmentRenderMode::Full,
        };

        let plan = super::plan(&props);
        assert_eq!(plan.reasoning, AssistantReasoning::Hidden);
    }

    #[test]
    fn slim_reasoning_rows_do_not_exceed_inner_width() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let area = Rect::new(0, 0, 72, 8);
        let mut buf = Buffer::empty(area);
        render(
            AssistantRenderProps {
                text: "",
                thinking: "Evaluating prefix widths
I need to take a closer look at the actual prefix widths and current assumptions about prefix lengths, especially for bash.",
                complete: false,
                meta: &meta,
                presentation: &presentation,
                surface: crate::surfaces::conversation::SegmentSurfacePolicy { surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript, copy: crate::surfaces::conversation::SegmentCopyPolicy::Body, selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle },
                mode: SegmentRenderMode::Slim,
            },
            area,
            &mut buf,
            &SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Slim),
        );

        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            assert!(
                unicode_width::UnicodeWidthStr::width(line.trim_end()) <= area.width as usize,
                "row {y} overflowed: {line:?}"
            );
        }
    }

    #[test]
    fn slim_reasoning_reserves_space_before_answer() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let area = Rect::new(0, 0, 72, 8);
        let mut buf = Buffer::empty(area);
        render(
            AssistantRenderProps {
                text: "final answer",
                thinking: "**Considering user request**\nI need to respond to the user based on their request, which involves reasoning and utilizing a single tool. This text should be truncated before it can wrap into the answer body.",
                complete: false,
                meta: &meta,
                presentation: &presentation,
                surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                    surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                    copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                    selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
                },
                mode: SegmentRenderMode::Slim,
            },
            area,
            &mut buf,
            &SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Slim),
        );

        let lines: Vec<String> = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();

        assert!(lines[0].contains("reasoning"), "first row: {:?}", lines[0]);
        assert!(
            lines.iter().any(|line| line.contains("final answer")),
            "answer missing from rendered buffer: {lines:?}"
        );
        let answer_row = lines
            .iter()
            .position(|line| line.contains("final answer"))
            .expect("answer row");
        assert!(answer_row >= 2, "answer overlapped reasoning: {lines:?}");
    }

    #[test]
    fn sanitized_reasoning_lines_remove_provider_artifacts() {
        let lines = sanitized_reasoning_lines(
            "**Planning detailed row expansion**\n\n<!-- -->\n\n<think>\n**Designing info detail rows with key focus**\n</think>",
        );

        assert_eq!(
            lines,
            vec![
                "Planning detailed row expansion".to_string(),
                "Designing info detail rows with key focus".to_string(),
            ]
        );
    }

    #[test]
    fn full_reasoning_renderer_sanitizes_markdown_and_html_artifacts() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let area = Rect::new(0, 0, 76, 14);
        let mut buf = Buffer::empty(area);
        render(
            AssistantRenderProps {
                text: "done",
                thinking: "**Planning detailed row expansion**\n\n<!-- -->\n\n**Designing info detail rows with key focus**",
                complete: true,
                meta: &meta,
                presentation: &presentation,
                surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                    surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                    copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                    selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
                },
                mode: SegmentRenderMode::Full,
            },
            area,
            &mut buf,
            &SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Full),
        );
        let text = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Planning detailed row expansion"), "{text}");
        assert!(
            text.contains("Designing info detail rows with key focus"),
            "{text}"
        );
        assert!(!text.contains("<!--"), "{text}");
        assert!(!text.contains("-->"), "{text}");
        assert!(!text.contains("**"), "{text}");
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
                surface: crate::surfaces::conversation::SegmentSurfacePolicy {
                    surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript,
                    copy: crate::surfaces::conversation::SegmentCopyPolicy::Body,
                    selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle,
                },
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
