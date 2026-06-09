//! Image/media segment component.

use std::path::Path;

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use super::super::conversation_render_projection::SegmentRenderContext;
use super::super::segments::{apply_rows_bg, file_url_for_path};

pub struct ImageRenderProps<'a> {
    pub path: &'a Path,
    pub alt: &'a str,
}

pub fn render(
    props: ImageRenderProps<'_>,
    area: Rect,
    buf: &mut Buffer,
    ctx: &SegmentRenderContext<'_>,
) {
    let theme = ctx.theme;
    if area.height == 0 {
        return;
    }

    // Title: full disk path (or alt text if the caller supplied one).
    // The previous behavior used only the filename which left
    // operators guessing about the parent directory.
    let path_str = props.path.display().to_string();
    let label = if props.alt.is_empty() || props.alt == "clipboard paste" {
        format!(" ▦ {path_str} ")
    } else {
        format!(" ▦ {} — {path_str} ", props.alt)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(theme.accent()))
        .title(Span::styled(
            label,
            Style::default()
                .fg(theme.accent_bright())
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme.bg()));

    // The block is the placeholder — the actual image is rendered on top
    // of this area in a second pass by the ConversationWidget (ratatui-image).
    // Repaint the full segment with the main background first so any old
    // card chrome at the edges is replaced by a crisp high-contrast edge.
    apply_rows_bg(area, 0, area.height, theme.bg(), buf);
    block.render(area, buf);

    let line = Line::from(Span::styled(
        path_str.clone(),
        Style::default()
            .fg(theme.accent_muted())
            .bg(theme.bg())
            .add_modifier(Modifier::UNDERLINED),
    ));
    if let Some(url) = file_url_for_path(&path_str) {
        let caption_area = Rect {
            x: area.x.saturating_add(1),
            y: area.bottom().saturating_sub(1),
            width: area.width.saturating_sub(2),
            height: 1,
        };
        hyperrat::Link::new(path_str, url)
            .style(
                Style::default()
                    .fg(theme.accent_muted())
                    .bg(theme.bg())
                    .add_modifier(Modifier::UNDERLINED),
            )
            .render(caption_area, buf);
    } else if area.height > 1 {
        Paragraph::new(line).render(
            Rect {
                x: area.x.saturating_add(1),
                y: area.bottom().saturating_sub(1),
                width: area.width.saturating_sub(2),
                height: 1,
            },
            buf,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

    #[test]
    fn image_props_preserve_render_inputs() {
        let path = Path::new("/tmp/screenshot.png");
        let props = ImageRenderProps {
            path,
            alt: "screen",
        };
        assert_eq!(props.path, path);
        assert_eq!(props.alt, "screen");
    }

    #[test]
    fn image_placeholder_renders_alt_and_path() {
        let path = Path::new("/tmp/screenshot.png");
        let area = Rect::new(0, 0, 48, 4);
        let mut buf = Buffer::empty(area);
        let ctx =
            SegmentRenderContext::new(&Alpharius, crate::tui::segments::SegmentRenderMode::Full);
        render(
            ImageRenderProps {
                path,
                alt: "screen",
            },
            area,
            &mut buf,
            &ctx,
        );
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                text.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(text.contains("screen"), "alt should render: {text:?}");
        assert!(
            text.contains("/tmp/screenshot.png"),
            "path should render: {text:?}"
        );
    }
}
