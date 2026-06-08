//! Focus-mode conversation rendering helpers.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::conversation_render_projection::segment_chrome;
use super::segments::{self, Segment, SegmentContent, SegmentExportMode, build_meta_tag};
use super::theme::Theme;
use super::widgets;

pub struct FocusLines {
    pub lines: Vec<Line<'static>>,
    pub viewport_height: u16,
    pub content_width: u16,
}

pub fn focus_tool_summary(name: &str, detail_args: Option<&str>) -> String {
    let args = match detail_args {
        Some(a) => a,
        None => return name.to_string(),
    };
    match name {
        "bash" => {
            let cmd = args.lines().next().unwrap_or(args);
            crate::util::truncate(cmd, 60)
        }
        "edit" | "change" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
                let path = v
                    .get("file")
                    .or(v.get("path"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("?");
                crate::util::truncate(path, 50)
            } else {
                crate::util::truncate(args, 50)
            }
        }
        "read" | "write" | "view" => {
            let first = args.lines().next().unwrap_or(args);
            crate::util::truncate(first, 50)
        }
        _ => {
            let first = args.lines().next().unwrap_or(args);
            crate::util::truncate(first, 40)
        }
    }
}

pub fn build_focus_lines(
    segments: &[Segment],
    selected: Option<usize>,
    area: Rect,
    theme: &dyn Theme,
) -> FocusLines {
    let viewport_height = area.height.saturating_sub(1);
    let content_width = area.width.saturating_sub(1).max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut last_turn: Option<u32> = None;

    for (idx, segment) in segments.iter().enumerate() {
        if matches!(segment.content, SegmentContent::TurnSeparator) {
            continue;
        }

        if let Some(turn) = segment.meta.turn
            && last_turn != Some(turn)
        {
            last_turn = Some(turn);
            if !lines.is_empty() {
                let mut turn_spans: Vec<Span<'static>> = vec![
                    Span::styled("─── ", Style::default().fg(theme.border_dim())),
                    Span::styled(
                        format!("turn {turn}"),
                        Style::default()
                            .fg(theme.accent_muted())
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                if let Some(ctx) = segment.meta.context_percent.filter(|p| *p > 5.0) {
                    let ctx_color = widgets::percent_color(ctx, theme);
                    turn_spans.push(Span::styled(
                        format!(" · ctx:{ctx:.0}%"),
                        Style::default().fg(ctx_color),
                    ));
                }
                let fill_width = content_width.saturating_sub(40) as usize;
                turn_spans.push(Span::styled(
                    format!(" {}", "─".repeat(fill_width)),
                    Style::default().fg(theme.border_dim()),
                ));
                lines.push(Line::from(turn_spans));
            }
        }

        let is_selected = selected == Some(idx);
        let presentation = segment.presentation();
        let chrome = segment_chrome(presentation, is_selected, theme);
        let role = chrome.role_label;
        let sigil = chrome.sigil;
        let color = chrome.role_color;

        let timestamp: Option<String> = segment.meta.timestamp.and_then(|ts| {
            chrono::DateTime::<chrono::Local>::from(ts)
                .format("%H:%M:%S")
                .to_string()
                .into()
        });

        let gutter_char = if is_selected { "▌" } else { "▎" };
        let gutter_style = Style::default().fg(color).add_modifier(if is_selected {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });

        let mut header_spans: Vec<Span<'static>> = vec![
            Span::styled(gutter_char.to_string(), gutter_style),
            Span::styled(
                format!(" {sigil} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(role.to_string(), Style::default().fg(color)),
        ];

        if let SegmentContent::ToolCard {
            ref name,
            ref detail_args,
            ..
        } = segment.content
        {
            let tool_summary = focus_tool_summary(name, detail_args.as_deref());
            header_spans.push(Span::styled(
                format!(" · {tool_summary}"),
                Style::default().fg(theme.muted()),
            ));
        }

        let meta = build_meta_tag(&segment.meta);
        if !meta.is_empty() {
            header_spans.push(Span::styled(
                format!("  {meta}"),
                Style::default().fg(theme.dim()),
            ));
        }

        let mut right_parts: Vec<String> = Vec::new();
        if let Some(ms) = segment.meta.duration_ms {
            right_parts.push(segments::format_duration_compact(ms));
        }
        if let Some(tokens) = segment.meta.actual_tokens {
            right_parts.push(tokens.format_compact());
        }
        if let Some(ref stamp) = timestamp {
            right_parts.push(stamp.clone());
        }
        if !right_parts.is_empty() {
            header_spans.push(Span::styled(
                format!("  {}", right_parts.join(" · ")),
                Style::default().fg(theme.dim()),
            ));
        }

        lines.push(Line::from(header_spans));

        let mut content = segment.export_text(SegmentExportMode::Plaintext);
        let expanded = matches!(
            segment.content,
            SegmentContent::ToolCard { expanded: true, .. }
        );
        let max_chars = if is_selected || expanded {
            usize::MAX
        } else {
            2000
        };
        if content.chars().count() > max_chars {
            content = crate::util::truncate(&content, 2000);
            content.push_str("\n… truncated (Enter to expand)");
        }

        let max_lines = if is_selected || expanded { 100 } else { 40 };
        let content_color = chrome.content_color;
        for line in content.lines().take(max_lines) {
            lines.push(Line::from(vec![
                Span::styled(gutter_char.to_string(), gutter_style),
                Span::styled(format!("  {line}"), Style::default().fg(content_color)),
            ]));
        }
        let total_content_lines = content.lines().count();
        if total_content_lines > max_lines {
            lines.push(Line::from(vec![
                Span::styled(gutter_char.to_string(), gutter_style),
                Span::styled(
                    format!("  ⋯ {} more lines", total_content_lines - max_lines),
                    Style::default().fg(theme.dim()),
                ),
            ]));
        }

        lines.push(Line::from(vec![
            Span::styled("╰", Style::default().fg(color)),
            Span::styled(
                if is_selected { "── ●" } else { "──" },
                Style::default().fg(color),
            ),
        ]));
        lines.push(Line::default());
    }
    if lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }

    FocusLines {
        lines,
        viewport_height,
        content_width,
    }
}

pub fn render_focus_lines(
    frame: &mut Frame,
    area: Rect,
    theme: &dyn Theme,
    focus: FocusLines,
    top_line: u16,
) {
    let paragraph = Paragraph::new(focus.lines)
        .style(Style::default().fg(theme.fg()).bg(theme.surface_bg()))
        .wrap(Wrap { trim: false })
        .scroll((top_line, 0));
    let text_area = Rect {
        x: area.x,
        y: area.y,
        width: focus.content_width,
        height: focus.viewport_height,
    };
    frame.render_widget(paragraph, text_area);

    let overlay = Paragraph::new(
        "↑/↓ scroll · PgUp/PgDn jump · Home/End · Enter expand · ^Y copy · Esc exit",
    )
    .style(Style::default().fg(theme.dim()).bg(theme.surface_bg()))
    .alignment(Alignment::Center);
    let overlay_area = Rect {
        x: area.x,
        y: area.bottom().saturating_sub(1),
        width: area.width,
        height: 1,
    };
    frame.render_widget(overlay, overlay_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_tool_summary_classifies_common_tools() {
        assert_eq!(focus_tool_summary("bash", Some("cargo test")), "cargo test");
        assert_eq!(
            focus_tool_summary("read", Some("src/main.rs\nignored")),
            "src/main.rs"
        );
        assert_eq!(focus_tool_summary("unknown", None), "unknown");
    }
}
