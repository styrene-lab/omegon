//! Tool-card segment component boundary.
//!
//! The first extraction slice gives tool cards a component-level API while the
//! large renderer body is moved incrementally out of `segments.rs` in follow-up
//! passes.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::surfaces::conversation::ToolCategory;

use super::super::segments::{
    self, SegmentMeta, SegmentRenderMode, TokenUsage, apply_rendered_links, apply_rows_bg,
    subtle_tool_row_bg, summarize_tool_args,
};
use super::super::theme::Theme;

pub struct ToolCardRenderProps<'a> {
    pub name: &'a str,
    pub detail_args: Option<&'a str>,
    pub detail_result: Option<&'a str>,
    pub is_error: bool,
    pub complete: bool,
    pub expanded: bool,
    pub live_partial: Option<&'a omegon_traits::PartialToolResult>,
    pub started_at: Option<std::time::Instant>,
    pub meta: &'a SegmentMeta,
    pub tool_category: Option<ToolCategory>,
    pub mode: SegmentRenderMode,
    pub density: crate::settings::ToolDetail,
    pub pinned: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_tool_args_section(
    lines: &mut Vec<Line<'_>>,
    name: &str,
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    args_budget: usize,
    complete: bool,
    is_error: bool,
    effective: crate::settings::ToolDetail,
    bg: Color,
    theme: &dyn Theme,
) {
    if let Some(summary) = summarize_tool_args(name, detail_args) {
        lines.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(theme.accent_muted()).bg(bg)),
            Span::styled(summary, Style::default().fg(theme.fg()).bg(bg)),
        ]));
    }

    // In Lean mode, only the summary line above is shown for completed tools.
    // Skip args and results entirely — Ctrl+O expands individual cards.
    if matches!(effective, crate::settings::ToolDetail::Lean) && complete && !is_error {
        if detail_result.is_some() || detail_args.is_some() {
            lines.push(Line::from(Span::styled(
                "  Ctrl+O to expand",
                Style::default()
                    .fg(theme.dim())
                    .bg(bg)
                    .add_modifier(Modifier::DIM),
            )));
        }
        return;
    }

    if args_budget == 0 {
        return;
    }
    let Some(args) = detail_args else {
        return;
    };
    match name {
        "bash" => {
            for (i, line) in args.lines().take(args_budget).enumerate().skip(1) {
                let prefix = if i == 0 { "$ " } else { "  " };
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme.dim()).bg(bg)),
                    Span::styled(line.to_string(), Style::default().fg(theme.fg()).bg(bg)),
                ]));
            }
        }
        "edit" | "change" | "read" | "write" | "view" => {
            // Summary line already rendered above; body/result carries the useful payload.
        }
        _ => {
            let display_args = if args.starts_with('{') || args.starts_with('[') {
                serde_json::from_str::<serde_json::Value>(args)
                    .ok()
                    .and_then(|v| serde_json::to_string_pretty(&v).ok())
                    .unwrap_or_else(|| args.to_string())
            } else {
                args.to_string()
            };
            for line in display_args.lines().take(args_budget) {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(theme.dim()).bg(bg),
                )));
            }
        }
    }
}

pub(crate) fn tool_card_right_title_spans<'a>(
    complete: bool,
    duration_ms: Option<u64>,
    actual_tokens: Option<TokenUsage>,
    timestamp: Option<&'a str>,
    theme: &dyn Theme,
) -> Vec<Span<'a>> {
    let dim_style = Style::default().fg(theme.dim()).add_modifier(Modifier::DIM);
    let sep = Span::styled(" · ", dim_style);
    let mut spans: Vec<Span<'a>> = Vec::new();

    if complete && let Some(ms) = duration_ms {
        spans.push(Span::styled(
            super::super::segments::format_duration_compact(ms),
            dim_style,
        ));
    }
    if let Some(tokens) = actual_tokens {
        if !spans.is_empty() {
            spans.push(sep.clone());
        }
        spans.push(Span::styled(
            tokens.format_compact(),
            Style::default()
                .fg(theme.accent_muted())
                .add_modifier(Modifier::DIM),
        ));
    }
    if let Some(stamp) = timestamp {
        if !spans.is_empty() {
            spans.push(sep);
        }
        spans.push(Span::styled(stamp.to_string(), dim_style));
    }
    spans
}

pub fn render(props: ToolCardRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_tool_card(
        props.name,
        props.detail_args,
        props.detail_result,
        props.is_error,
        props.complete,
        props.expanded,
        props.live_partial,
        props.started_at,
        props.meta,
        props.tool_category,
        area,
        buf,
        theme,
        props.mode,
        props.density,
        props.pinned,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_slim_tool_summary_rows(
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
    bg: Color,
    status_icon: &str,
    status_color: Color,
    display_name: &str,
    detail_rows: &[String],
    pinned: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let child_bg = subtle_tool_row_bg(bg);
    let visible_rows = detail_rows.len().min(area.height as usize);
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(visible_rows.max(1));

    for (idx, detail) in detail_rows.iter().take(visible_rows).enumerate() {
        let row_bg = if idx == 0 { bg } else { child_bg };
        apply_rows_bg(area, idx as u16, 1, row_bg, buf);
        if idx == 0 {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{status_icon} "),
                    Style::default()
                        .fg(status_color)
                        .bg(row_bg)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    if pinned {
                        format!("{display_name} · pinned ")
                    } else {
                        format!("{display_name} ")
                    },
                    Style::default()
                        .fg(status_color)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("· ", Style::default().fg(t.dim()).bg(row_bg)),
                Span::styled(detail.clone(), Style::default().fg(t.muted()).bg(row_bg)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default().fg(t.dim()).bg(row_bg)),
                Span::styled(detail.clone(), Style::default().fg(t.dim()).bg(row_bg)),
            ]));
        }
    }

    Paragraph::new(lines.clone())
        .style(Style::default().bg(bg))
        .render(area, buf);
    apply_rendered_links(
        area,
        &lines,
        buf,
        Style::default()
            .fg(t.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        area.height,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_slim_tool_live_rows(
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
    bg: Color,
    status_icon: &str,
    status_color: Color,
    display_name: &str,
    rows: &[String],
    pinned: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let child_bg = subtle_tool_row_bg(bg);
    let visible_rows = rows.len().min(area.height as usize);
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(visible_rows.max(1));

    for (idx, row) in rows.iter().take(visible_rows).enumerate() {
        let row_bg = if idx == 0 { bg } else { child_bg };
        apply_rows_bg(area, idx as u16, 1, row_bg, buf);
        if idx == 0 {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{status_icon} "),
                    Style::default()
                        .fg(status_color)
                        .bg(row_bg)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    if pinned {
                        format!("{display_name} · pinned ")
                    } else {
                        format!("{display_name} ")
                    },
                    Style::default()
                        .fg(status_color)
                        .bg(row_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("· ", Style::default().fg(t.dim()).bg(row_bg)),
                Span::styled(row.clone(), Style::default().fg(t.muted()).bg(row_bg)),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                row.clone(),
                Style::default().fg(t.dim()).bg(row_bg),
            )));
        }
    }

    Paragraph::new(lines.clone())
        .style(Style::default().bg(bg))
        .render(area, buf);
    apply_rendered_links(
        area,
        &lines,
        buf,
        Style::default()
            .fg(t.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        area.height,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_args_section_adds_lean_expand_hint() {
        let mut lines = Vec::new();
        append_tool_args_section(
            &mut lines,
            "bash",
            Some("{\"cmd\":\"echo hi\"}"),
            Some("ok"),
            4,
            true,
            false,
            crate::settings::ToolDetail::Lean,
            Color::Reset,
            &crate::tui::theme::Alpharius,
        );
        let rendered: String = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            rendered.contains("Ctrl+O to expand"),
            "lean hint should render: {rendered}"
        );
    }

    #[test]
    fn right_title_spans_include_duration_tokens_and_timestamp() {
        let spans = tool_card_right_title_spans(
            true,
            Some(1_250),
            Some(TokenUsage {
                input: 1_200,
                output: 340,
            }),
            Some("14:32"),
            &crate::tui::theme::Alpharius,
        );
        let rendered: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert!(
            rendered.contains("1.2s"),
            "duration should render: {rendered}"
        );
        assert!(
            rendered.contains("↑1.2k ↓340"),
            "tokens should render: {rendered}"
        );
        assert!(
            rendered.contains("14:32"),
            "timestamp should render: {rendered}"
        );
    }

    #[test]
    fn tool_card_props_preserve_render_inputs() {
        let meta = SegmentMeta::default();
        let props = ToolCardRenderProps {
            name: "bash",
            detail_args: Some("cargo check"),
            detail_result: Some("ok"),
            is_error: false,
            complete: true,
            expanded: false,
            live_partial: None,
            started_at: None,
            meta: &meta,
            tool_category: Some(ToolCategory::CommandExec),
            mode: SegmentRenderMode::Full,
            density: crate::settings::ToolDetail::Detailed,
            pinned: true,
        };
        assert_eq!(props.name, "bash");
        assert_eq!(props.detail_args, Some("cargo check"));
        assert_eq!(props.tool_category, Some(ToolCategory::CommandExec));
        assert!(props.complete);
        assert!(props.pinned);
    }
}
