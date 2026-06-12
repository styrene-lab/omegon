//! Selected conversation segment detail pane.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

use super::segments::{Segment, SegmentContent, SegmentExportMode};
use super::theme::Theme;

pub fn preferred_height(segment: Option<&Segment>, available_height: u16) -> u16 {
    if segment.is_none() || available_height < 14 {
        return 0;
    }
    available_height.clamp(0, 12).max(7)
}

pub fn render(area: Rect, buf: &mut Buffer, theme: &dyn Theme, idx: usize, segment: &Segment) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let title = format!(" detail · segment {idx} · {} ", segment_kind_label(segment));
    let body = detail_body(segment);
    let lines = body.lines().map(Line::from).collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(theme.accent()))
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme.accent_bright())
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.surface_bg()));
    Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(theme.fg()).bg(theme.surface_bg()))
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

fn segment_kind_label(segment: &Segment) -> &'static str {
    match segment.content {
        SegmentContent::UserPrompt { .. } => "user",
        SegmentContent::AssistantText { .. } => "assistant",
        SegmentContent::PeerAgentText { .. } => "peer_agent",
        SegmentContent::ToolCard { .. } => "tool",
        SegmentContent::SystemNotification { .. } => "system",
        SegmentContent::LifecycleEvent { .. } => "lifecycle",
        SegmentContent::Image { .. } => "image",
        SegmentContent::TurnSeparator => "separator",
    }
}

fn detail_body(segment: &Segment) -> String {
    match &segment.content {
        SegmentContent::ToolCard {
            id,
            name,
            args_summary,
            result_summary,
            is_error,
            complete,
            live_partial,
            started_at,
            ..
        } => {
            let mut lines = vec![
                format!("tool: {name}"),
                format!("id: {id}"),
                format!(
                    "status: {}",
                    if !complete {
                        "running"
                    } else if *is_error {
                        "error"
                    } else {
                        "complete"
                    }
                ),
            ];
            if let Some(started_at) = started_at {
                lines.push(format!(
                    "elapsed: {:.1}s",
                    started_at.elapsed().as_secs_f32()
                ));
            }
            if let Some(summary) = args_summary.as_deref().filter(|s| !s.trim().is_empty()) {
                lines.push(String::new());
                lines.push(format!("args summary: {summary}"));
            }
            if let Some(summary) = result_summary.as_deref().filter(|s| !s.trim().is_empty()) {
                lines.push(String::new());
                lines.push(format!("result summary: {summary}"));
            }
            if let Some(partial) = live_partial.as_deref() {
                lines.push(String::new());
                lines.push(format!("live elapsed: {}ms", partial.progress.elapsed_ms));
                if partial.progress.heartbeat {
                    lines.push("live heartbeat: true".to_string());
                }
                if !partial.tail.trim().is_empty() {
                    lines.push("live tail:".to_string());
                    lines.push(partial.tail.trim_end().to_string());
                }
            }
            let exported = segment.export_text(SegmentExportMode::Raw);
            if !exported.trim().is_empty() {
                lines.push(String::new());
                lines.push(exported);
            }
            lines.join("\n")
        }
        _ => segment.export_text(SegmentExportMode::Raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::segments::Segment;

    #[test]
    fn preferred_height_requires_room_and_segment() {
        assert_eq!(preferred_height(None, 40), 0);
        let segment = Segment::user_prompt("hello");
        assert_eq!(preferred_height(Some(&segment), 10), 0);
        assert_eq!(preferred_height(Some(&segment), 20), 12);
    }
}
