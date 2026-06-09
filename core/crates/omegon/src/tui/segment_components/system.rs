//! System notification segment component.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Widget, Wrap};

use super::super::conversation_render_projection::SegmentRenderContext;
use super::super::segments::{SegmentRenderMode, apply_rendered_links};

pub struct SystemRenderProps<'a> {
    pub text: &'a str,
    pub mode: SegmentRenderMode,
}

pub fn render(
    props: SystemRenderProps<'_>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &SegmentRenderContext<'_>,
) {
    let theme = ctx.theme;
    if area.width < 3 || area.height == 0 {
        return;
    }

    let bg = theme.card_bg();
    let border_color = theme.accent_muted();
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
                " Ω ",
                Style::default()
                    .fg(border_color)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            )))
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(bg))
    };
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();
    for (i, line) in props.text.lines().enumerate() {
        let style = if i == 0 && line.starts_with('Ω') {
            Style::default()
                .fg(theme.accent())
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        } else if i == 0
            && (line.starts_with('⚠') || line.starts_with('⟳') || line.starts_with('✓'))
        {
            Style::default().fg(theme.warning()).bg(bg)
        } else if line.starts_with("  ▸") || line.starts_with("  /") || line.starts_with("  Ctrl")
        {
            Style::default().fg(theme.muted()).bg(bg)
        } else {
            Style::default().fg(theme.accent_muted()).bg(bg)
        };
        lines.push(Line::from(Span::styled(line.to_string(), style)));
    }

    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
    apply_rendered_links(
        inner,
        &lines,
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
    use crate::tui::theme::Alpharius;

    #[test]
    fn system_props_preserve_render_inputs() {
        let props = SystemRenderProps {
            text: "notice",
            mode: SegmentRenderMode::Full,
        };
        assert_eq!(props.text, "notice");
        assert_eq!(props.mode, SegmentRenderMode::Full);
    }

    #[test]
    fn system_renderer_includes_notice_text() {
        let area = Rect::new(0, 0, 40, 5);
        let mut buf = Buffer::empty(area);
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Full);
        render(
            SystemRenderProps {
                text: "notice",
                mode: SegmentRenderMode::Full,
            },
            area,
            &mut buf,
            &ctx,
        );
        let mut rendered = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                rendered.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(
            rendered.contains("notice"),
            "notice should render: {rendered:?}"
        );
    }
}
