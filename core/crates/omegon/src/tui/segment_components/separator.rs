//! Turn separator segment component boundary.

use ratatui::prelude::*;

use super::super::segments;
use super::super::theme::Theme;

pub fn render(area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_separator(area, buf, theme);
}

#[cfg(test)]
mod tests {
    #[test]
    fn separator_component_has_no_payload() {
        assert_eq!(std::mem::size_of::<()>(), 0);
    }
}
