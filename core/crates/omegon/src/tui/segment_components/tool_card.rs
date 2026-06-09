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
    self, EditDiffBlock, SegmentMeta, SegmentRenderMode, TokenUsage, apply_rendered_links,
    apply_rows_bg, strip_terminal_control, subtle_tool_row_bg, summarize_tool_args,
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
pub(crate) fn append_tool_live_progress_section(
    lines: &mut Vec<Line<'_>>,
    live_row_fills: &mut Vec<(u16, Color)>,
    live_partial: Option<&omegon_traits::PartialToolResult>,
    started_at: Option<std::time::Instant>,
    complete: bool,
    tail_budget: usize,
    card_width: u16,
    bg: Color,
    theme: &dyn Theme,
) {
    if complete {
        return;
    }

    let pre_live_line_count = lines.len();
    if !lines.is_empty() {
        let sep_color = theme.border_dim();
        lines.push(Line::from(Span::styled(
            "─".repeat(card_width as usize),
            Style::default().fg(sep_color).bg(bg),
        )));
        live_row_fills.push((pre_live_line_count as u16, bg));
    }

    let mut status_parts: Vec<String> = Vec::new();
    let phase_label = live_partial
        .and_then(|p| p.progress.phase.as_deref())
        .unwrap_or("running");
    status_parts.push(phase_label.to_string());
    if let Some(partial) = live_partial
        && let Some(units) = &partial.progress.units
    {
        let label = match units.total {
            Some(total) => format!("{}/{} {}", units.current, total, units.unit),
            None => format!("{} {}", units.current, units.unit),
        };
        status_parts.push(label);
    }
    let elapsed_ms: Option<u64> = started_at
        .map(|started| started.elapsed().as_millis() as u64)
        .or_else(|| live_partial.map(|p| p.progress.elapsed_ms))
        .filter(|ms| *ms > 0);
    if let Some(ms) = elapsed_ms {
        let secs = ms / 1000;
        if secs >= 60 {
            status_parts.push(format!("{}m{:02}s", secs / 60, secs % 60));
        } else {
            let tenths = (ms % 1000) / 100;
            status_parts.push(format!("{secs}.{tenths}s"));
        }
    }
    if let Some(partial) = live_partial
        && partial.progress.heartbeat
    {
        status_parts.push("idle".to_string());
    }
    let status_text = format!("▶ {}", status_parts.join(" · "));
    lines.push(Line::from(vec![Span::styled(
        status_text,
        Style::default().fg(theme.warning()).bg(bg),
    )]));
    live_row_fills.push((lines.len().saturating_sub(1) as u16, bg));

    if let Some(partial) = live_partial
        && !partial.tail.is_empty()
    {
        let tail_lines: Vec<&str> = partial.tail.lines().collect();
        let take = tail_lines.len().min(tail_budget);
        let start = tail_lines.len().saturating_sub(take);
        let visible_tail: String = tail_lines[start..].join("\n");
        let has_ansi = visible_tail.contains('\x1b');
        let tail_style = Style::default().fg(theme.muted()).bg(bg);

        if has_ansi {
            use ansi_to_tui::IntoText as _;
            if let Ok(text) = visible_tail.into_text() {
                for line in text.lines {
                    let spans: Vec<Span<'_>> = line
                        .spans
                        .into_iter()
                        .map(|mut s| {
                            s.style = s.style.bg(bg);
                            if s.style.fg.is_none() {
                                s.style = s.style.fg(theme.muted());
                            }
                            s
                        })
                        .collect();
                    lines.push(Line::from(spans));
                    live_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                }
            } else {
                for line in &tail_lines[start..] {
                    let stripped = strip_terminal_control(line);
                    lines.push(Line::from(Span::styled(stripped, tail_style)));
                    live_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                }
            }
        } else {
            for line in &tail_lines[start..] {
                let stripped: String = line.chars().filter(|c| !c.is_control()).collect();
                lines.push(Line::from(Span::styled(stripped, tail_style)));
                live_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
            }
        }
    }
}

pub(crate) struct EditDiffSectionProps<'a> {
    pub is_error: bool,
    pub expanded: bool,
    pub detail_result: Option<&'a str>,
    pub diff_budget: usize,
    pub card_width: u16,
    pub bg: Color,
    pub theme: &'a dyn Theme,
}

pub(crate) fn append_edit_diff_section(
    lines: &mut Vec<Line<'_>>,
    result_row_fills: &mut Vec<(u16, Color)>,
    blocks: &[EditDiffBlock],
    props: EditDiffSectionProps<'_>,
) {
    if !lines.is_empty() {
        let sep_color = if props.is_error {
            props.theme.error()
        } else {
            props.theme.border_dim()
        };
        lines.push(Line::from(Span::styled(
            "─".repeat(props.card_width as usize),
            Style::default().fg(sep_color).bg(props.bg),
        )));
        result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));
    }
    let max_diff_lines = props.diff_budget;
    let mut emitted = 0usize;
    let removed_style = Style::default().fg(props.theme.error()).bg(props.bg);
    let added_style = Style::default().fg(props.theme.success()).bg(props.bg);
    let header_style = Style::default()
        .fg(props.theme.accent_muted())
        .bg(props.bg)
        .add_modifier(Modifier::BOLD);
    let summary_style = Style::default().fg(props.theme.muted()).bg(props.bg);

    let total_added: usize = blocks.iter().map(|b| b.new_text.lines().count()).sum();
    let total_removed: usize = blocks.iter().map(|b| b.old_text.lines().count()).sum();
    lines.push(Line::from(vec![
        Span::styled(format!("Δ {} edit(s) · ", blocks.len()), summary_style),
        Span::styled(format!("+{total_added}"), added_style),
        Span::styled(" / ", summary_style),
        Span::styled(format!("-{total_removed}"), removed_style),
        Span::styled(
            if props.expanded {
                ""
            } else {
                "  (expand for full diff)"
            },
            summary_style,
        ),
    ]));
    result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));

    let sanitize_diff_line =
        |s: &str| -> String { s.chars().filter(|c| !c.is_control()).collect() };
    let multi_block = blocks.len() > 1;
    'outer: for block in blocks {
        if multi_block {
            if emitted >= max_diff_lines {
                break;
            }
            lines.push(Line::from(Span::styled(
                format!("▸ {}", sanitize_diff_line(&block.file)),
                header_style,
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));
            emitted += 1;
        }
        for line in block.old_text.lines() {
            if emitted >= max_diff_lines {
                break 'outer;
            }
            lines.push(Line::from(Span::styled(
                format!("- {}", sanitize_diff_line(line)),
                removed_style,
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));
            emitted += 1;
        }
        for line in block.new_text.lines() {
            if emitted >= max_diff_lines {
                break 'outer;
            }
            lines.push(Line::from(Span::styled(
                format!("+ {}", sanitize_diff_line(line)),
                added_style,
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));
            emitted += 1;
        }
    }

    let total_diff_lines: usize = blocks
        .iter()
        .map(|b| {
            let header = if multi_block { 1 } else { 0 };
            header + b.old_text.lines().count() + b.new_text.lines().count()
        })
        .sum();
    if total_diff_lines > emitted {
        lines.push(Line::from(Span::styled(
            format!("… {} more diff line(s)", total_diff_lines - emitted),
            summary_style,
        )));
        result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));
    }

    if props.is_error
        && let Some(err_text) = props.detail_result
    {
        lines.push(Line::from(Span::styled(
            err_text.lines().next().unwrap_or(err_text).to_string(),
            Style::default().fg(props.theme.error()).bg(props.bg),
        )));
        result_row_fills.push((lines.len().saturating_sub(1) as u16, props.bg));
    }
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
    fn live_progress_section_adds_status_and_tail() {
        let mut lines = Vec::new();
        let mut fills = Vec::new();
        let mut partial = omegon_traits::PartialToolResult::content("first\nsecond", 1200);
        partial.progress.phase = Some("running".to_string());
        append_tool_live_progress_section(
            &mut lines,
            &mut fills,
            Some(&partial),
            None,
            false,
            4,
            40,
            Color::Reset,
            &crate::tui::theme::Alpharius,
        );
        let rendered: String = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            rendered.contains("▶ running"),
            "status should render: {rendered}"
        );
        assert!(
            rendered.contains("second"),
            "tail should render: {rendered}"
        );
        assert!(!fills.is_empty());
    }

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
    fn edit_diff_section_renders_summary_and_changed_lines() {
        let blocks = vec![EditDiffBlock {
            file: "src/lib.rs".into(),
            old_text: "old".into(),
            new_text: "new".into(),
        }];
        let mut lines = Vec::new();
        let mut fills = Vec::new();
        append_edit_diff_section(
            &mut lines,
            &mut fills,
            &blocks,
            EditDiffSectionProps {
                is_error: false,
                expanded: false,
                detail_result: None,
                diff_budget: 8,
                card_width: 40,
                bg: Color::Reset,
                theme: &crate::tui::theme::Alpharius,
            },
        );
        let rendered: String = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(
            rendered.contains("Δ 1 edit(s)"),
            "summary should render: {rendered}"
        );
        assert!(
            rendered.contains("- old"),
            "removed line should render: {rendered}"
        );
        assert!(
            rendered.contains("+ new"),
            "added line should render: {rendered}"
        );
        assert!(!fills.is_empty());
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
