//! Tool-card segment component boundary.
//!
//! The first extraction slice gives tool cards a component-level API while the
//! large renderer body is moved incrementally out of `segments.rs` in follow-up
//! passes.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::{
    ContentForm, SegmentAffordances, SegmentContentPresentation, SegmentMetric, SegmentProducer,
    SegmentState, ToolCategory,
};

use super::super::conversation_render_projection::SegmentRenderContext;
use super::super::segments::{
    self, EditDiffBlock, SegmentMeta, SegmentRenderMode, TableState, TokenUsage,
    apply_rendered_links, apply_rows_bg, compute_table_widths, is_table_line, is_table_separator,
    render_table_line, strip_terminal_control, subtle_tool_row_bg, summarize_tool_args,
    try_highlight,
};
use super::super::theme::Theme;
use super::compact_row;
use crate::tui::inline_render::{DETAILS_HINT_LABEL, expand_hint_cell};
use crate::tui::widgets;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SlimSegmentHeader<'a> {
    pub producer: SegmentProducer<'a>,
    pub state: SegmentState,
    pub content: SegmentContentPresentation<'a>,
    pub metrics: Vec<SegmentMetric<'a>>,
    pub affordances: SegmentAffordances,
    pub display_name: String,
    pub category_icon: &'static str,
}

impl<'a> SlimSegmentHeader<'a> {
    #[allow(clippy::too_many_arguments)]
    fn from_tool_fields(
        name: &'a str,
        detail_args: Option<&'a str>,
        detail_result: Option<&'a str>,
        is_error: bool,
        complete: bool,
        live_partial: Option<&'a omegon_traits::PartialToolResult>,
        tool_category: Option<ToolCategory>,
        display_name: String,
    ) -> Self {
        let state = if is_error {
            SegmentState::Failed
        } else if complete {
            SegmentState::Completed
        } else {
            SegmentState::Running
        };
        let category = tool_category
            .unwrap_or_else(|| crate::surfaces::conversation::tool_category_for_name(name));
        let visual_identity =
            crate::surfaces::conversation::tool_visual_identity(name, detail_args);
        let category_icon = crate::tui::glyphs::glyphs().tool_category(
            crate::tui::glyphs::tool_category_role_for_identity(&visual_identity),
        );
        let form = if name == "bash" {
            ContentForm::Log
        } else if matches!(name, "edit" | "change") {
            ContentForm::Diff
        } else if matches!(name, "read" | "view") {
            detail_result
                .map(|text| {
                    let trimmed = text.trim_start();
                    if trimmed.is_empty() {
                        ContentForm::Empty
                    } else if trimmed.starts_with('#')
                        || trimmed.starts_with("```")
                        || trimmed.contains("\n#")
                        || trimmed.contains("\n- ")
                        || trimmed.contains("\n* ")
                    {
                        ContentForm::Markdown
                    } else {
                        ContentForm::Prose
                    }
                })
                .unwrap_or(ContentForm::Empty)
        } else {
            ContentForm::Structured
        };
        Self {
            producer: SegmentProducer::Tool { name, category },
            state,
            content: SegmentContentPresentation {
                form,
                title: Some(name),
                summary: detail_result,
                body: detail_result,
            },
            metrics: Vec::new(),
            affordances: SegmentAffordances {
                detail_available: segments::tool_has_expandable_detail(
                    detail_args,
                    detail_result,
                    live_partial,
                ),
                expandable: detail_args.is_some() || detail_result.is_some(),
                selectable: true,
                copyable: detail_result.is_some(),
            },
            display_name,
            category_icon,
        }
    }
}

pub(crate) fn state_color_for_segment_state(state: SegmentState, t: &dyn Theme) -> Color {
    match state {
        SegmentState::Pending | SegmentState::Informational => t.dim(),
        SegmentState::Running => t.accent_muted(),
        SegmentState::Completed | SegmentState::Cancelled => t.muted(),
        SegmentState::Failed => t.warning(),
    }
}

fn completed_slim_tool_label_color(t: &dyn Theme) -> Color {
    // Completed tool rows are transcript evidence, not attention states. Resolve
    // their color at the final render boundary so neither category chrome nor a
    // stale status projection can reintroduce warning orange.
    t.muted()
}

fn slim_tool_header_cells(
    header: &SlimSegmentHeader<'_>,
    legacy_cells: Vec<String>,
    include_detail_hint: bool,
) -> Vec<String> {
    let mut cells = Vec::new();
    cells.extend(legacy_cells);
    let _ = (header, include_detail_hint);
    cells
}

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
        Style::default().fg(theme.accent()).bg(bg),
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

pub(crate) struct GenericResultSectionProps<'a> {
    pub name: &'a str,
    pub detail_args: Option<&'a str>,
    pub detail_result: Option<&'a str>,
    pub is_error: bool,
    pub expanded: bool,
    pub result_budget: usize,
    pub card_width: u16,
    pub bg: Color,
    pub theme: &'a dyn Theme,
}

pub(crate) fn append_generic_result_section(
    lines: &mut Vec<Line<'_>>,
    result_row_fills: &mut Vec<(u16, Color)>,
    props: GenericResultSectionProps<'_>,
) {
    let name = props.name;
    let detail_args = props.detail_args;
    let detail_result = props.detail_result;
    let is_error = props.is_error;
    let expanded = props.expanded;
    let result_budget = props.result_budget;
    let card_width = props.card_width;
    let bg = props.bg;
    let t = props.theme;
    if matches!(props.name, "context_status") {
        return;
    }

    if let Some(result) = detail_result {
        let pre_result_line_count = lines.len();
        if !lines.is_empty() {
            // Separator line — matches card border color (red on error)
            let sep_color = if is_error { t.error() } else { t.border_dim() };
            let sep_bg = bg;
            lines.push(Line::from(Span::styled(
                "─".repeat(card_width as usize),
                Style::default().fg(sep_color).bg(sep_bg),
            )));
            result_row_fills.push((pre_result_line_count as u16, sep_bg));
        }

        // Pretty-print JSON results — tool outputs often arrive as compact JSON
        // with literal \n inside string values (e.g. commit messages).
        let pretty_result: std::borrow::Cow<'_, str> =
            if result.starts_with('{') || result.starts_with('[') {
                match serde_json::from_str::<serde_json::Value>(result) {
                    Ok(val) => std::borrow::Cow::Owned(
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| result.to_string()),
                    ),
                    Err(_) => std::borrow::Cow::Borrowed(result),
                }
            } else {
                std::borrow::Cow::Borrowed(result)
            };
        let result_lines: Vec<&str> = pretty_result.lines().collect();
        let max_lines = result_budget;
        let show = result_lines.len().min(max_lines);
        let display_text = result_lines[..show].join("\n");

        if name == "bash" && !is_error && display_text.contains('\x1b') {
            use ansi_to_tui::IntoText as _;
            let sanitized = strip_terminal_control(&display_text);
            let parsed = display_text
                .into_text()
                .or_else(|_| sanitized.as_str().into_text());
            if let Ok(text) = parsed {
                for line in text.lines {
                    let spans = line
                        .spans
                        .into_iter()
                        .map(|mut span| {
                            span.style = span.style.bg(bg);
                            if span.style.fg.is_none() {
                                span.style = span.style.fg(t.muted());
                            }
                            span
                        })
                        .collect::<Vec<_>>();
                    lines.push(Line::from(spans));
                    result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                }
                return;
            }
        }

        // Try syntax highlighting based on file extension from args
        let highlighted = if !is_error {
            try_highlight(&display_text, detail_args, name, t)
        } else {
            None
        };

        if let Some(highlighted_lines) = highlighted {
            for line in highlighted_lines {
                // Apply card bg to each span so result rows stay visually unified.
                let spans: Vec<Span<'_>> = line
                    .spans
                    .into_iter()
                    .map(|mut s| {
                        s.style = s.style.bg(bg);
                        s
                    })
                    .collect();
                lines.push(Line::from(spans));
                result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
            }
        } else {
            let result_style = if is_error {
                Style::default().fg(t.error()).bg(bg)
            } else {
                Style::default().fg(t.muted()).bg(bg)
            };

            let mut table_state = TableState::None;
            let visible_lines = &result_lines[..show];
            let has_table_lines = visible_lines.iter().any(|line| is_table_line(line.trim()));

            if !is_error && has_table_lines {
                // Pre-pass to compute shared per-column widths across
                // each table block — see `compute_table_widths` for the
                // rationale (the column-shred bug in codebase_search
                // results).
                let table_widths_per_line =
                    compute_table_widths(visible_lines, card_width as usize);
                for (idx, line) in visible_lines.iter().copied().enumerate() {
                    let trimmed = line.trim();
                    if let Some(target_widths) = table_widths_per_line[idx].as_ref() {
                        let is_header = matches!(table_state, TableState::None);
                        if is_table_separator(trimmed) || matches!(table_state, TableState::Header)
                        {
                            table_state = TableState::Body;
                        } else {
                            table_state = TableState::Header;
                        }
                        let row_bg = bg;
                        lines.push(render_table_line(trimmed, is_header, target_widths, t));
                        result_row_fills.push((lines.len().saturating_sub(1) as u16, row_bg));
                    } else {
                        table_state = TableState::None;
                        let rendered = if trimmed.is_empty() {
                            Line::from(Span::styled(String::new(), Style::default().bg(bg)))
                        } else {
                            let mut line = widgets::highlight_line(line, t);
                            for span in &mut line.spans {
                                span.style = span.style.bg(bg);
                                if span.style.fg.is_none() {
                                    span.style = span.style.fg(t.muted());
                                }
                            }
                            line
                        };
                        lines.push(rendered);
                        result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                    }
                }
            } else {
                // Try ANSI color parsing for tool output (cargo, git diff, etc.)
                let joined = result_lines[..show].join("\n");
                let has_ansi = joined.contains('\x1b');

                if has_ansi {
                    use ansi_to_tui::IntoText as _;
                    if let Ok(text) = joined.into_text() {
                        for line in text.lines {
                            let spans: Vec<Span<'_>> = line
                                .spans
                                .into_iter()
                                .map(|mut s| {
                                    // Preserve ANSI foreground, apply card background
                                    s.style = s.style.bg(bg);
                                    // If no foreground was set by ANSI, use muted
                                    if s.style.fg.is_none() {
                                        s.style = s.style.fg(t.muted());
                                    }
                                    s
                                })
                                .collect();
                            lines.push(Line::from(spans));
                            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                        }
                    } else {
                        // ANSI parse failed — fall back to plain
                        for line in &result_lines[..show] {
                            lines.push(Line::from(Span::styled(line.to_string(), result_style)));
                            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                        }
                    }
                } else {
                    for line in &result_lines[..show] {
                        let trimmed = line.trim();
                        let rendered = if is_error {
                            Line::from(Span::styled(line.to_string(), result_style))
                        } else if trimmed.is_empty() {
                            Line::from(Span::styled(String::new(), Style::default().bg(bg)))
                        } else {
                            let mut line = widgets::highlight_line(line, t);
                            for span in &mut line.spans {
                                span.style = span.style.bg(bg);
                                if span.style.fg.is_none() {
                                    span.style = span.style.fg(t.muted());
                                }
                            }
                            line
                        };
                        lines.push(rendered);
                        result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                    }
                }
            }
        }

        if result_lines.len() > show {
            let hint = if expanded {
                format!("  ── {} lines ── Tab to collapse", result_lines.len())
            } else {
                format!(
                    "  ── {} more lines ── {}",
                    result_lines.len() - show,
                    expand_hint_cell().text
                )
            };
            lines.push(Line::from(Span::styled(
                hint,
                Style::default().fg(t.accent_muted()).bg(bg),
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
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
                format!("  {}", expand_hint_cell().text),
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

pub fn render(
    props: ToolCardRenderProps<'_>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &SegmentRenderContext<'_>,
) {
    let theme = ctx.theme;
    render_tool_card(
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
fn render_tool_card(
    name: &str,
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    is_error: bool,
    complete: bool,
    expanded: bool,
    live_partial: Option<&omegon_traits::PartialToolResult>,
    started_at: Option<std::time::Instant>,
    meta: &SegmentMeta,
    tool_category: Option<ToolCategory>,
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
    mode: SegmentRenderMode,
    density: crate::settings::ToolDetail,
    pinned: bool,
) {
    // Tool status glyphs are semantic policy from `tui::glyphs`; this component
    // only renders the projected chrome.
    let chrome = crate::tui::conversation_render_projection::tool_card_chrome(
        name,
        detail_args,
        is_error,
        complete,
        tool_category,
        t,
    );
    let display_name = chrome.display_name;
    let status_icon = chrome.status_icon;
    let status_color = chrome.status_color;
    let border_color = chrome.border_color;
    let bg = chrome.background;

    let timestamp = segments::format_timestamp(meta.timestamp);
    let title = segments::tool_title_line(
        status_icon,
        status_color,
        &display_name,
        area.width,
        timestamp.as_deref(),
        pinned,
    );

    // Right-aligned title: duration · ↑1.2k ↓340 · 14:32
    let right_title_spans = tool_card_right_title_spans(
        complete,
        meta.duration_ms,
        meta.actual_tokens,
        timestamp.as_deref(),
        t,
    );

    if matches!(mode, SegmentRenderMode::Slim) && !complete && !expanded {
        let header = SlimSegmentHeader::from_tool_fields(
            name,
            detail_args,
            detail_result,
            is_error,
            complete,
            live_partial,
            tool_category,
            display_name.clone(),
        );
        let cells = slim_tool_header_cells(
            &header,
            segments::slim_tool_summary_cells(
                name,
                detail_args,
                detail_result,
                complete,
                live_partial,
                started_at,
                meta.duration_ms,
            )
            .into_iter()
            .filter(|cell| !cell.contains(DETAILS_HINT_LABEL))
            .collect(),
            false,
        );
        let detail_rows = segments::slim_tool_live_rows(area.width, &cells);
        render_slim_tool_live_rows(
            area,
            buf,
            t,
            bg,
            header.category_icon,
            state_color_for_segment_state(header.state, t),
            &header.display_name,
            &detail_rows,
            pinned,
        );
        return;
    }

    if matches!(mode, SegmentRenderMode::Slim) && complete && !expanded {
        let header = SlimSegmentHeader::from_tool_fields(
            name,
            detail_args,
            detail_result,
            is_error,
            complete,
            live_partial,
            tool_category,
            display_name.clone(),
        );
        let cells = slim_tool_header_cells(
            &header,
            segments::slim_tool_summary_cells(
                name,
                detail_args,
                detail_result,
                complete,
                live_partial,
                started_at,
                meta.duration_ms,
            ),
            true,
        );
        let detail_rows = vec![segments::slim_tool_first_detail_for_prefix(
            area.width,
            crate::tui::segment_components::compact_row::prefix_width(
                header.category_icon,
                &header.display_name,
                pinned,
            ),
            &cells,
        )];
        render_slim_tool_summary_rows(
            area,
            buf,
            t,
            bg,
            header.category_icon,
            if header.state == SegmentState::Completed {
                completed_slim_tool_label_color(t)
            } else {
                state_color_for_segment_state(header.state, t)
            },
            &header.display_name,
            &detail_rows,
            pinned,
        );
        return;
    }

    let card_block = if matches!(mode, SegmentRenderMode::Slim) {
        // Slim: top border only, no side borders — maximizes terminal
        // text selection width and avoids │ chars in copied text.
        Block::default()
            .borders(Borders::TOP)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(border_color).bg(bg))
            .title_top(title)
            .title_top(Line::from(right_title_spans).right_aligned())
            .padding(Padding::horizontal(0))
            .style(Style::default().bg(bg))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(bg))
            .title_top(title)
            .title_top(Line::from(right_title_spans).right_aligned())
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(bg))
    };

    let card_inner = card_block.inner(area);
    card_block.render(area, buf);

    if card_inner.height == 0 || card_inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Effective density: expanded overrides to Verbose.
    let effective = if expanded {
        crate::settings::ToolDetail::Verbose
    } else {
        density
    };
    let args_budget = effective.args_budget();
    let result_budget = effective.result_budget();
    let tail_budget = effective.tail_budget();

    append_tool_args_section(
        &mut lines,
        name,
        detail_args,
        detail_result,
        args_budget,
        complete,
        is_error,
        effective,
        bg,
        t,
    );
    if matches!(effective, crate::settings::ToolDetail::Lean) && complete && !is_error {
        let para = Paragraph::new(lines.clone())
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(bg));
        para.render(card_inner, buf);
        apply_rendered_links(
            card_inner,
            &lines,
            buf,
            Style::default()
                .fg(t.accent_muted())
                .bg(bg)
                .add_modifier(Modifier::UNDERLINED),
            card_inner.height,
        );
        return;
    }

    let mut live_row_fills: Vec<(u16, Color)> = Vec::new();
    append_tool_live_progress_section(
        &mut lines,
        &mut live_row_fills,
        live_partial,
        started_at,
        complete,
        tail_budget,
        card_inner.width,
        bg,
        t,
    );

    // ── Edit/change diff section ────────────────────────────────
    // For mutating-file tools (`edit`, `change`), the standard result
    // text is just "Successfully replaced text in {path}" — useless
    // for an operator who wants to see what actually changed.
    // Replace it with a colored line-by-line diff computed from the
    // tool's args (`oldText` / `newText`), which the renderer already
    // has access to via `detail_args`. The diff rendered here is the
    // intent — what the agent ASKED for — not the post-validation
    // result. On a successful edit they're equivalent; on a failed
    // edit the validation error is rendered separately below.
    let mut result_row_fills: Vec<(u16, Color)> = Vec::new();
    let diff_blocks: Option<Vec<EditDiffBlock>> = if matches!(name, "edit" | "change") {
        detail_args.and_then(|args| segments::build_edit_diff_blocks(name, args))
    } else {
        None
    };
    if let Some(blocks) = diff_blocks {
        append_edit_diff_section(
            &mut lines,
            &mut result_row_fills,
            &blocks,
            EditDiffSectionProps {
                is_error,
                expanded,
                detail_result,
                diff_budget: effective.diff_budget(),
                card_width: card_inner.width,
                bg,
                theme: t,
            },
        );
    } else {
        append_generic_result_section(
            &mut lines,
            &mut result_row_fills,
            GenericResultSectionProps {
                name,
                detail_args,
                detail_result,
                is_error,
                expanded,
                result_budget,
                card_width: card_inner.width,
                bg,
                theme: t,
            },
        );
    }

    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .render(card_inner, buf);

    // Apply background fills for both the live (in-flight) section and
    // the completed result section. Both share the same `bg` color in
    // practice; keeping the two fill streams separate makes the
    // intent obvious and lets future styling diverge them cheaply.
    for (row, fill_bg) in live_row_fills {
        apply_rows_bg(card_inner, row, 1, fill_bg, buf);
    }
    for (row, fill_bg) in result_row_fills {
        apply_rows_bg(card_inner, row, 1, fill_bg, buf);
    }
    apply_rendered_links(
        card_inner,
        &lines,
        buf,
        Style::default()
            .fg(t.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        card_inner.height,
    );

    // ── Post-render: OSC 8 hyperlinks for single-file tool paths ────────────
    if matches!(name, "read" | "write" | "view")
        && let Some(args) = detail_args
    {
        let file_path = args.lines().next().unwrap_or(args).trim().to_string();
        if !file_path.is_empty() && card_inner.height > 0 {
            let prefix = format!(
                "{} ",
                crate::tui::glyphs::glyphs().tool(crate::tui::glyphs::ToolGlyphRole::Detail)
            );
            let row_style = Style::default().bg(bg);
            let link_style = Style::default()
                .fg(t.accent_muted())
                .bg(bg)
                .add_modifier(Modifier::UNDERLINED);

            for x in card_inner.left()..card_inner.right() {
                if let Some(cell) = buf.cell_mut((x, card_inner.y)) {
                    cell.set_symbol(" ");
                    cell.set_style(row_style);
                }
            }

            if card_inner.width >= prefix.len() as u16 {
                if let Some(cell) = buf.cell_mut((card_inner.x, card_inner.y)) {
                    cell.set_symbol(
                        crate::tui::glyphs::glyphs()
                            .tool(crate::tui::glyphs::ToolGlyphRole::Detail),
                    );
                    cell.set_style(Style::default().fg(t.accent_muted()).bg(bg));
                }
                if let Some(cell) = buf.cell_mut((card_inner.x + 1, card_inner.y)) {
                    cell.set_symbol(" ");
                    cell.set_style(row_style);
                }

                let available = card_inner.width.saturating_sub(prefix.len() as u16);
                if available > 0
                    && let Some(url) = segments::file_url_for_path(&file_path)
                {
                    let link_area = Rect {
                        x: card_inner.x + prefix.len() as u16,
                        y: card_inner.y,
                        width: available,
                        height: 1,
                    };
                    let link = hyperrat::Link::new(file_path, url).style(link_style);
                    link.render(link_area, buf);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_slim_tool_summary_rows(
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
    bg: Color,
    category_icon: &str,
    status_color: Color,
    display_name: &str,
    detail_rows: &[String],
    pinned: bool,
) {
    compact_row::render(
        area,
        buf,
        t,
        bg,
        subtle_tool_row_bg(bg),
        compact_row::CompactRows::tool(
            category_icon,
            display_name,
            status_color,
            detail_rows,
            pinned,
        ),
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_slim_tool_live_rows(
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
    bg: Color,
    category_icon: &str,
    status_color: Color,
    display_name: &str,
    rows: &[String],
    pinned: bool,
) {
    compact_row::render(
        area,
        buf,
        t,
        bg,
        subtle_tool_row_bg(bg),
        compact_row::CompactRows::tool(category_icon, display_name, status_color, rows, pinned),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_tool_segment_is_neutral_at_the_final_render_boundary() {
        let theme = crate::tui::theme::Alpharius;
        let segment = segments::Segment {
            meta: SegmentMeta::default(),
            content: segments::SegmentContent::ToolCard {
                id: "tool-1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("git status".into()),
                result_summary: None,
                detail_result: Some("command completed".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        segment.render(
            area,
            &mut buf,
            &theme,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );

        let label_width = compact_row::prefix_width("⌘", "bash", false)
            .saturating_sub(" · ".len() as u16);
        let label_cells = (area.left()..area.left() + label_width)
            .filter_map(|x| buf.cell((x, area.y)))
            .collect::<Vec<_>>();
        assert!(!label_cells.is_empty());
        assert!(
            label_cells.iter().all(|cell| cell.fg == theme.muted()),
            "completed tool label must be neutral in the final segment buffer"
        );
        assert!(
            label_cells.iter().all(|cell| cell.fg != theme.warning()),
            "completed tool label must not consume attention orange"
        );
    }

    #[test]
    fn rendered_slim_tool_labels_preserve_state_semantics() {
        let theme = crate::tui::theme::Alpharius;
        let cases = [
            (SegmentState::Running, theme.accent_muted()),
            (SegmentState::Completed, theme.muted()),
            (SegmentState::Failed, theme.warning()),
        ];

        for (state, expected) in cases {
            let area = Rect::new(0, 0, 64, 1);
            let mut buf = Buffer::empty(area);
            render_slim_tool_summary_rows(
                area,
                &mut buf,
                &theme,
                theme.bg(),
                "⌘",
                state_color_for_segment_state(state, &theme),
                "bash",
                &["git status".to_string()],
                false,
            );

            let label_end = compact_row::prefix_width("⌘", "bash", false)
                .saturating_sub(" · ".len() as u16);
            let label_cells = (area.left()..area.left() + label_end)
                .filter_map(|x| buf.cell((x, area.y)))
                .collect::<Vec<_>>();
            assert!(!label_cells.is_empty());
            assert!(
                label_cells.iter().all(|cell| cell.fg == expected),
                "rendered {state:?} label should use {expected:?}"
            );
            if state != SegmentState::Failed {
                assert!(
                    label_cells.iter().all(|cell| cell.fg != theme.warning()),
                    "non-failed {state:?} label must not consume attention orange"
                );
            }
        }
    }

    #[test]
    fn slim_tool_state_colors_reserve_orange_for_attention() {
        let theme = crate::tui::theme::Alpharius;
        assert_eq!(
            state_color_for_segment_state(SegmentState::Running, &theme),
            theme.accent_muted()
        );
        assert_eq!(
            state_color_for_segment_state(SegmentState::Completed, &theme),
            theme.muted()
        );
        assert_eq!(
            state_color_for_segment_state(SegmentState::Failed, &theme),
            theme.warning()
        );
        assert_ne!(
            state_color_for_segment_state(SegmentState::Running, &theme),
            theme.warning()
        );
        assert_ne!(
            state_color_for_segment_state(SegmentState::Completed, &theme),
            theme.warning()
        );
    }

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
            rendered.contains("^O expand"),
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
    fn generic_result_section_renders_plain_result() {
        let mut lines = Vec::new();
        let mut fills = Vec::new();
        append_generic_result_section(
            &mut lines,
            &mut fills,
            GenericResultSectionProps {
                name: "bash",
                detail_args: None,
                detail_result: Some("hello"),
                is_error: false,
                expanded: false,
                result_budget: 8,
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
            rendered.contains("hello"),
            "result should render: {rendered}"
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
