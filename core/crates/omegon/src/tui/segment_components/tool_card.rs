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
    self, SegmentMeta, SegmentRenderMode, apply_rendered_links, apply_rows_bg, subtle_tool_row_bg,
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
