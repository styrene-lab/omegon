//! System notification segment component boundary.

use ratatui::prelude::*;

use super::super::segments::{self, SegmentRenderMode};
use super::super::theme::Theme;

pub struct SystemRenderProps<'a> {
    pub text: &'a str,
    pub mode: SegmentRenderMode,
}

pub fn render(props: SystemRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_system(props.text, area, buf, theme, props.mode);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_props_preserve_render_inputs() {
        let props = SystemRenderProps {
            text: "notice",
            mode: SegmentRenderMode::Slim,
        };
        assert_eq!(props.text, "notice");
        assert_eq!(props.mode, SegmentRenderMode::Slim);
    }
}
