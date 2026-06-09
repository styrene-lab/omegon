//! Lifecycle event segment component.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::super::theme::Theme;

pub struct LifecycleRenderProps<'a> {
    pub icon: &'a str,
    pub text: &'a str,
}

pub fn render(props: LifecycleRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    if area.width < 4 || area.height == 0 {
        return;
    }
    let line = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("{} ", props.icon),
            Style::default().fg(theme.border()),
        ),
        Span::styled(props.text.to_string(), Style::default().fg(theme.dim())),
    ]);
    Paragraph::new(line).render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

    #[test]
    fn lifecycle_props_preserve_render_inputs() {
        let props = LifecycleRenderProps {
            icon: "⚡",
            text: "event",
        };
        assert_eq!(props.icon, "⚡");
        assert_eq!(props.text, "event");
    }

    #[test]
    fn lifecycle_renders_icon_and_text() {
        let area = Rect::new(0, 0, 16, 1);
        let mut buf = Buffer::empty(area);
        render(
            LifecycleRenderProps {
                icon: "⚡",
                text: "event",
            },
            area,
            &mut buf,
            &Alpharius,
        );
        let text: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(text.contains("⚡"), "icon should render: {text:?}");
        assert!(text.contains("event"), "text should render: {text:?}");
    }
}
