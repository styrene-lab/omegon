//! Turn separator segment component.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::super::conversation_render_projection::SegmentRenderContext;

pub fn render(area: Rect, buf: &mut Buffer, ctx: &SegmentRenderContext<'_>) {
    let theme = ctx.theme;
    if area.height == 0 || area.width < 4 {
        return;
    }
    // Thin ruled divider with faded edges.
    let pad = 2;
    let rule_w = (area.width as usize).saturating_sub(pad * 2);
    let line = Line::from(vec![
        Span::styled(" ".repeat(pad), Style::default()),
        Span::styled("─".repeat(rule_w), Style::default().fg(theme.border_dim())),
        Span::styled(" ".repeat(pad), Style::default()),
    ]);
    Paragraph::new(line).render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

    #[test]
    fn separator_renders_rule_when_wide_enough() {
        let area = Rect::new(0, 0, 8, 1);
        let mut buf = Buffer::empty(area);
        let ctx =
            SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Full);
        render(area, &mut buf, &ctx);
        let text: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(
            text.contains("────"),
            "separator rule should render: {text:?}"
        );
    }

    #[test]
    fn separator_skips_too_narrow_area() {
        let area = Rect::new(0, 0, 3, 1);
        let mut buf = Buffer::empty(area);
        let ctx =
            SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Full);
        render(area, &mut buf, &ctx);
        let text: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(text, "   ");
    }
}
