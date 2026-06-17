//! Shared compact-row renderer for slim conversation activity rows.
//!
//! Slim conversation rows use one visual policy: identity is a fixed-width
//! label, state is color, and row details are rendered after a dim separator.
//! This keeps assistant reasoning, tools, and future activity rows from growing
//! their own incompatible chrome.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

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
    let budget = area_width.saturating_sub(prefix_width);
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

    let visible_rows = rows.details.len().min(area.height as usize);
    let mut lines: Vec<Line<'_>> = Vec::with_capacity(visible_rows.max(1));

    for (idx, detail) in rows.details.iter().take(visible_rows).enumerate() {
        let row_bg = if idx == 0 { bg } else { child_bg };
        apply_rows_bg(area, idx as u16, 1, row_bg, buf);
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
            lines.push(Line::from(vec![
                Span::styled(
                    rows.child_indent,
                    Style::default().fg(theme.dim()).bg(row_bg),
                ),
                Span::styled(detail.clone(), Style::default().fg(theme.dim()).bg(row_bg)),
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
            .fg(theme.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        area.height,
    );
}
