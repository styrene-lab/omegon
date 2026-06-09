//! Image/media segment component boundary.

use std::path::Path;

use ratatui::prelude::*;

use super::super::segments;
use super::super::theme::Theme;

pub struct ImageRenderProps<'a> {
    pub path: &'a Path,
    pub alt: &'a str,
}

pub fn render(props: ImageRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_image_placeholder(props.path, props.alt, area, buf, theme);
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
