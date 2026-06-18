//! Shared compact-row renderer for slim conversation activity rows.
//!
//! Slim conversation rows use one visual policy: identity is a fixed-width
//! label, state is color, and row details are rendered after a dim separator.
//! This keeps assistant reasoning, tools, and future activity rows from growing
//! their own incompatible chrome.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::super::segments::{apply_rendered_links, apply_rows_bg};
use super::super::theme::Theme;
use crate::surfaces::inline::{InlineCell, InlineCellRole, InlineRow};
use crate::tui::inline_render::{DETAILS_HINT_LABEL, render_inline_text_row};

pub(crate) const COMPACT_ROW_LABEL_WIDTH: usize = 16;

pub(crate) fn label(identity: &str, name: &str) -> String {
    let raw = if identity.is_empty() {
        name.to_string()
    } else {
        format!("{identity} {name}")
    };
    let width = unicode_width::UnicodeWidthStr::width(raw.as_str());
    if width >= COMPACT_ROW_LABEL_WIDTH {
        raw
    } else {
        format!("{raw}{}", " ".repeat(COMPACT_ROW_LABEL_WIDTH - width))
    }
}

pub(crate) fn prefix_width(identity: &str, name: &str, pinned: bool) -> u16 {
    let label = label(identity, name);
    let label = if pinned {
        format!("{label} · pinned · ")
    } else {
        format!("{label} · ")
    };
    unicode_width::UnicodeWidthStr::width(label.as_str()) as u16
}

pub(crate) fn detail_cells_from_rendered_row(row: &str) -> Vec<String> {
    row.split(" · ")
        .filter(|cell| !cell.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn first_detail_row(area_width: u16, prefix_width: u16, row: &str) -> String {
    // Hard invariant: the full rendered line is label + separator + detail, so
    // the detail text must be budgeted against that exact prefix plus a small
    // terminal safety gutter for ambiguous key glyph widths.
    const RIGHT_GUTTER: u16 = 2;
    let budget = area_width
        .saturating_sub(prefix_width)
        .saturating_sub(RIGHT_GUTTER);
    let cells = detail_cells_from_rendered_row(row);
    let (left_cells, right_cells): (Vec<_>, Vec<_>) = cells
        .into_iter()
        .partition(|cell| !cell.contains(DETAILS_HINT_LABEL));
    let inline = InlineRow::new(
        left_cells
            .into_iter()
            .map(|cell| InlineCell::new(cell, InlineCellRole::Value))
            .collect(),
        right_cells
            .into_iter()
            .map(|cell| InlineCell::new(cell, InlineCellRole::Affordance))
            .collect(),
    );
    render_inline_text_row(&inline, budget)
}

pub(crate) struct CompactRows<'a> {
    pub identity: &'a str,
    pub name: &'a str,
    pub label_color: Color,
    pub details: &'a [String],
    pub pinned: bool,
    pub child_indent: &'a str,
    pub label_modifier: Modifier,
}

impl<'a> CompactRows<'a> {
    pub(crate) fn tool(
        identity: &'a str,
        name: &'a str,
        label_color: Color,
        details: &'a [String],
        pinned: bool,
    ) -> Self {
        Self {
            identity,
            name,
            label_color,
            details,
            pinned,
            child_indent: "  ",
            label_modifier: Modifier::BOLD,
        }
    }

    pub(crate) fn metadata(name: &'a str, label_color: Color, details: &'a [String]) -> Self {
        Self {
            identity: "",
            name,
            label_color,
            details,
            pinned: false,
            child_indent: "  ",
            label_modifier: Modifier::ITALIC,
        }
    }
}

fn wrapped_visual_rows(text: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    let display_width = unicode_width::UnicodeWidthStr::width(text);
    ((display_width + width.saturating_sub(1)) / width).max(1) as u16
}

pub(crate) fn truncate_to_width(text: &str, max_width: usize) -> String {
    const ELLIPSIS: &str = "…";
    let text_width = unicode_width::UnicodeWidthStr::width(text);
    if text_width <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let ellipsis_width = unicode_width::UnicodeWidthStr::width(ELLIPSIS);
    if max_width <= ellipsis_width {
        return ELLIPSIS.to_string();
    }

    let body_budget = max_width.saturating_sub(ellipsis_width);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > body_budget {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push_str(ELLIPSIS);
    out
}

fn child_width(row: &CompactRows<'_>, width: u16) -> u16 {
    width
        .saturating_sub(unicode_width::UnicodeWidthStr::width(row.child_indent) as u16)
        .max(1)
}

fn wrap_to_width(text: &str, width: u16) -> Vec<String> {
    let width = width.max(1) as usize;
    let mut rows = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;

    for word in text.split_whitespace() {
        let word_width = unicode_width::UnicodeWidthStr::width(word);
        let sep_width = usize::from(!current.is_empty());
        if !current.is_empty() && used + sep_width + word_width > width {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }

        if word_width > width {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                used = 0;
            }
            let mut chunk = String::new();
            let mut chunk_width = 0usize;
            for ch in word.chars() {
                let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if chunk_width > 0 && chunk_width + ch_width > width {
                    rows.push(std::mem::take(&mut chunk));
                    chunk_width = 0;
                }
                chunk.push(ch);
                chunk_width += ch_width;
            }
            if !chunk.is_empty() {
                current = chunk;
                used = chunk_width;
            }
        } else {
            if !current.is_empty() {
                current.push(' ');
                used += 1;
            }
            current.push_str(word);
            used += word_width;
        }
    }

    if !current.is_empty() {
        rows.push(current);
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

fn wrapped_child_rows(row: &CompactRows<'_>, detail: &str, width: u16) -> Vec<String> {
    wrap_to_width(detail, child_width(row, width))
}

pub(crate) fn measured_height(width: u16, rows: &CompactRows<'_>) -> u16 {
    if width == 0 || rows.details.is_empty() {
        return 0;
    }
    rows.details
        .iter()
        .enumerate()
        .map(|(idx, detail)| {
            if idx == 0 {
                1
            } else {
                wrapped_child_rows(rows, detail, width).len() as u16
            }
        })
        .sum()
}

pub(crate) fn render(
    area: Rect,
    buf: &mut Buffer,
    theme: &dyn Theme,
    bg: Color,
    child_bg: Color,
    rows: CompactRows<'_>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let visible_rows = rows.details.len();
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(visible_rows.max(1));

    for (idx, detail) in rows.details.iter().enumerate() {
        let row_bg = if idx == 0 { bg } else { child_bg };
        let visual_rows = if idx == 0 {
            1
        } else {
            wrapped_visual_rows(detail, child_width(&rows, area.width))
        };
        apply_rows_bg(area, lines.len() as u16, visual_rows, row_bg, buf);
        if idx == 0 {
            lines.push(Line::from(vec![
                Span::styled(
                    label(rows.identity, rows.name),
                    Style::default()
                        .fg(rows.label_color)
                        .bg(row_bg)
                        .add_modifier(rows.label_modifier),
                ),
                Span::styled(
                    if rows.pinned {
                        " · pinned · "
                    } else {
                        " · "
                    },
                    Style::default().fg(theme.dim()).bg(row_bg),
                ),
                Span::styled(
                    first_detail_row(
                        area.width,
                        prefix_width(rows.identity, rows.name, rows.pinned),
                        detail,
                    ),
                    Style::default().fg(theme.muted()).bg(row_bg),
                ),
            ]));
        } else {
            let wrapped = wrap_to_width(detail, child_width(&rows, area.width));
            for (line_idx, wrapped_detail) in wrapped.iter().enumerate() {
                lines.push(Line::from(vec![
                    Span::styled(
                        if line_idx == 0 { rows.child_indent } else { "" },
                        Style::default().fg(theme.dim()).bg(row_bg),
                    ),
                    Span::styled(
                        wrapped_detail.clone(),
                        Style::default().fg(theme.dim()).bg(row_bg),
                    ),
                ]));
            }
        }
    }

    Paragraph::new(lines.clone())
        .style(Style::default().bg(bg))
        .wrap(Wrap { trim: false })
        .render(area, buf);
    apply_rendered_links(
        area,
        &lines,
        buf,
        Style::default()
            .fg(theme.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        area.height,
    );
}
