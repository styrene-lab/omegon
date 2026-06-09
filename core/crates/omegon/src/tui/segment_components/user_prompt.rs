//! Operator/user prompt segment component boundary.

use ratatui::prelude::*;

use crate::surfaces::conversation::SegmentPresentation;

use super::super::segments::{self, SegmentMeta, SegmentRenderMode};
use super::super::theme::Theme;

pub struct UserPromptRenderProps<'a> {
    pub text: &'a str,
    pub presentation: &'a SegmentPresentation,
    pub meta: &'a SegmentMeta,
    pub mode: SegmentRenderMode,
}

pub fn render(props: UserPromptRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_user_prompt(
        props.text,
        props.presentation,
        props.meta,
        area,
        buf,
        theme,
        props.mode,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{SegmentEmphasis, SegmentRole};

    #[test]
    fn user_prompt_props_preserve_render_inputs() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Operator,
            sigil: "OP",
            emphasis: SegmentEmphasis::Strong,
            tool_category: None,
        };
        let props = UserPromptRenderProps {
            text: "hello",
            presentation: &presentation,
            meta: &meta,
            mode: SegmentRenderMode::Slim,
        };
        assert_eq!(props.text, "hello");
        assert_eq!(props.presentation.role, SegmentRole::Operator);
        assert_eq!(props.mode, SegmentRenderMode::Slim);
    }
}
