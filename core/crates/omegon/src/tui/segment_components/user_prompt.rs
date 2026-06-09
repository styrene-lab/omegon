//! Operator/user prompt segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use crate::surfaces::conversation::{SegmentEmphasis, SegmentPresentation};

use super::super::segments::{
    SegmentMeta, SegmentRenderMode, apply_rendered_links, split_preserving_trailing_empty_lines,
    top_right_timestamp,
};
use super::super::theme::Theme;

pub struct UserPromptRenderProps<'a> {
    pub text: &'a str,
    pub presentation: &'a SegmentPresentation,
    pub meta: &'a SegmentMeta,
    pub mode: SegmentRenderMode,
}

pub fn render(props: UserPromptRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    if area.width < 3 || area.height == 0 {
        return;
    }

    let bg = theme.user_msg_bg();
    let border_color = match props.presentation.emphasis {
        SegmentEmphasis::Strong => theme.accent(),
        SegmentEmphasis::Normal => theme.accent_muted(),
        SegmentEmphasis::Muted => theme.border_dim(),
    };
    let block = if matches!(props.mode, SegmentRenderMode::Slim) {
        Block::default()
            .padding(Padding::horizontal(0))
            .style(Style::default().bg(bg))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(bg))
            .title_top(Line::from(Span::styled(
                format!(" {}", props.presentation.sigil),
                Style::default()
                    .fg(border_color)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            )))
            .title_top(
                top_right_timestamp(props.meta, theme)
                    .unwrap_or_else(Line::default)
                    .right_aligned(),
            )
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(bg))
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
                meta: &meta,
                mode: SegmentRenderMode::Full,
            },
            area,
            &mut buf,
            &Alpharius,
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
