//! Operator/user prompt segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::{
    SegmentEmphasis, SegmentPresentation, SegmentSurfacePolicy,
};

use super::super::conversation_render_projection::{
    SegmentRenderContext, terminal_segment_paint,
};
use super::super::segments::{
    SegmentMeta, SegmentRenderMode, apply_rendered_links, split_preserving_trailing_empty_lines,
    top_right_timestamp,
};

pub struct UserPromptRenderProps<'a> {
    pub text: &'a str,
    pub presentation: &'a SegmentPresentation,
    pub surface: SegmentSurfacePolicy,
    pub meta: &'a SegmentMeta,
    pub mode: SegmentRenderMode,
}

pub fn render(
    props: UserPromptRenderProps<'_>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &SegmentRenderContext<'_>,
) {
    let theme = ctx.theme;
    if area.width < 3 || area.height == 0 {
        return;
    }

    let paint = terminal_segment_paint(props.surface, ctx);
    let bg = paint.text_bg.unwrap_or(paint.clear_bg);
    let block_bg = paint.surface_bg.unwrap_or(paint.clear_bg);
    let border_color = match props.presentation.emphasis {
        SegmentEmphasis::Strong => theme.accent(),
        SegmentEmphasis::Normal => theme.accent_muted(),
        SegmentEmphasis::Muted => theme.border_dim(),
    };
    let block = if matches!(props.mode, SegmentRenderMode::Slim) {
        Block::default()
            .padding(Padding::horizontal(0))
            .style(Style::default().bg(block_bg))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(block_bg))
            .title_top(Line::from(Span::styled(
                format!(" {}", props.presentation.sigil),
                Style::default()
                    .fg(border_color)
                    .bg(block_bg)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_top(
                top_right_timestamp(props.meta, theme)
                    .unwrap_or_else(Line::default)
                    .right_aligned(),
            )
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(block_bg))
    };
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content: Vec<Line<'_>> = split_preserving_trailing_empty_lines(props.text)
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::default()
                    .fg(theme.fg())
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    Paragraph::new(content.clone())
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
    apply_rendered_links(
        inner,
        &content,
        buf,
        Style::default()
            .fg(theme.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        inner.height,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{SegmentEmphasis, SegmentRole};
    use crate::tui::theme::Alpharius;

    #[test]
    fn user_prompt_props_preserve_render_inputs() {
        let presentation = SegmentPresentation {
            role: SegmentRole::Operator,
            sigil: "OP",
            emphasis: SegmentEmphasis::Strong,
            tool_category: None,
        };
        let meta = SegmentMeta::default();
        let props = UserPromptRenderProps {
            text: "hello",
            presentation: &presentation,
            surface: crate::surfaces::conversation::SegmentSurfacePolicy { surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript, copy: crate::surfaces::conversation::SegmentCopyPolicy::Body, selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle },
            meta: &meta,
            mode: SegmentRenderMode::Full,
        };
        assert_eq!(props.text, "hello");
        assert_eq!(props.presentation.sigil, "OP");
        assert_eq!(props.mode, SegmentRenderMode::Full);
    }

    #[test]
    fn user_prompt_renders_text() {
        let presentation = SegmentPresentation {
            role: SegmentRole::Operator,
            sigil: "OP",
            emphasis: SegmentEmphasis::Strong,
            tool_category: None,
        };
        let meta = SegmentMeta::default();
        let area = Rect::new(0, 0, 32, 5);
        let mut buf = Buffer::empty(area);
        render(
            UserPromptRenderProps {
                text: "hello",
                presentation: &presentation,
                surface: crate::surfaces::conversation::SegmentSurfacePolicy { surface: crate::surfaces::conversation::SegmentSurfaceTreatment::Transcript, copy: crate::surfaces::conversation::SegmentCopyPolicy::Body, selection: crate::surfaces::conversation::SegmentSelectionTreatment::Subtle },
                meta: &meta,
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
        assert!(text.contains("hello"), "prompt should render: {text:?}");
    }
}
