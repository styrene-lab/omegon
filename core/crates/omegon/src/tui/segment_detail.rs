//! Selected conversation segment detail pane.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget, Wrap};

use super::segments::{Segment, SegmentContent, SegmentExportMode};
use super::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDetailMode {
    Live,
    Detail,
}

impl ToolDetailMode {
    fn title_prefix(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Detail => "detail",
        }
    }
}

fn tool_output_lines(segment: &Segment) -> Vec<String> {
    match &segment.content {
        SegmentContent::ToolCard {
            detail_args,
            detail_result,
            live_partial,
            ..
        } => {
            let mut lines = Vec::new();
            if let Some(partial) = live_partial.as_deref()
                && !partial.tail.trim().is_empty()
            {
                lines.extend(partial.tail.lines().map(str::to_string));
            }
            if let Some(args) = detail_args.as_deref().filter(|s| !s.trim().is_empty()) {
                lines.push("args:".to_string());
                lines.extend(args.lines().map(str::to_string));
            }
            if let Some(result) = detail_result.as_deref().filter(|s| !s.trim().is_empty()) {
                lines.push("result:".to_string());
                lines.extend(result.lines().map(str::to_string));
            }
            if lines.is_empty() {
                lines.push(segment.export_text(SegmentExportMode::Raw));
            }
            lines
        }
        _ => segment
            .export_text(SegmentExportMode::Raw)
            .lines()
            .map(str::to_string)
            .collect(),
    }
}

pub fn preferred_height(segment: Option<&Segment>, available_height: u16) -> u16 {
    if segment.is_none() || available_height < 14 {
        return 0;
    }
    available_height.clamp(0, 12).max(7)
}

fn classify_tool_content_form(
    segment: &Segment,
    fallback_lines: &[String],
) -> crate::surfaces::conversation::ContentForm {
    if let SegmentContent::ToolCard {
        name,
        is_error,
        complete,
        detail_result,
        live_partial,
        ..
    } = &segment.content
    {
        if let Some(result) = detail_result.as_deref().filter(|s| !s.trim().is_empty()) {
            return crate::tui::tool_inspection::tool_content_form(
                name,
                &[result.to_string()],
                *complete,
                *is_error,
            );
        }
        if let Some(partial) = live_partial
            .as_deref()
            .filter(|p| !p.tail.trim().is_empty())
        {
            return crate::tui::tool_inspection::tool_content_form(
                name,
                &partial.tail.lines().map(str::to_string).collect::<Vec<_>>(),
                *complete,
                *is_error,
            );
        }
        return crate::tui::tool_inspection::tool_content_form(
            name,
            fallback_lines,
            *complete,
            *is_error,
        );
    }
    crate::surfaces::conversation::ContentForm::Prose
}

pub fn render_tool_card(
    area: Rect,
    buf: &mut Buffer,
    theme: &dyn Theme,
    segment: &Segment,
    mode: ToolDetailMode,
) {
    if let SegmentContent::ToolCard {
        name,
        args_summary,
        is_error,
        complete,
        started_at,
        ..
    } = &segment.content
    {
        let lines = tool_output_lines(segment);
        let state = if !complete {
            crate::tui::glyphs::ToolStateGlyphRole::Running
        } else if *is_error {
            crate::tui::glyphs::ToolStateGlyphRole::Failed
        } else {
            crate::tui::glyphs::ToolStateGlyphRole::Completed
        };
        let content_form = classify_tool_content_form(segment, &lines);
        crate::tui::tool_inspection::render_tool_inspection_panel(
            area,
            buf,
            theme,
            crate::tui::tool_inspection::ToolInspection {
                name,
                args_summary: args_summary.as_deref(),
                state,
                title_prefix: mode.title_prefix(),
                elapsed: if *complete {
                    None
                } else {
                    started_at.map(|instant| instant.elapsed())
                },
                content_form,
                lines: &lines,
            },
        );
    }
}

pub fn render(area: Rect, buf: &mut Buffer, theme: &dyn Theme, idx: usize, segment: &Segment) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if matches!(segment.content, SegmentContent::ToolCard { .. }) {
        render_tool_card(area, buf, theme, segment, ToolDetailMode::Detail);
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
        SegmentContent::OperatorCopyBlock { .. } => "operator_copy",
        SegmentContent::SkillEvent { .. } => "skill",
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
