//! Lifecycle event segment component boundary.

use ratatui::prelude::*;

use super::super::segments;
use super::super::theme::Theme;

pub struct LifecycleRenderProps<'a> {
    pub icon: &'a str,
    pub text: &'a str,
}

pub fn render(props: LifecycleRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_lifecycle(props.icon, props.text, area, buf, theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_props_preserve_render_inputs() {
        let props = LifecycleRenderProps {
            icon: "⚡",
            text: "event",
        };
        assert_eq!(props.icon, "⚡");
        assert_eq!(props.text, "event");
    }
}
