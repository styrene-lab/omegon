//! Assistant segment component boundary.
//!
//! This slice establishes a component-level API for assistant response rendering.
//! The large renderer body remains in `segments.rs` for now so shared markdown,
//! reasoning, and metadata helpers can be moved deliberately in later passes.

use ratatui::prelude::*;

use crate::surfaces::conversation::SegmentPresentation;

use super::super::segments::{self, SegmentMeta, SegmentRenderMode};
use super::super::theme::Theme;

pub struct AssistantRenderProps<'a> {
    pub text: &'a str,
    pub thinking: &'a str,
    pub complete: bool,
    pub meta: &'a SegmentMeta,
    pub presentation: &'a SegmentPresentation,
    pub mode: SegmentRenderMode,
}

pub fn render(props: AssistantRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_assistant_text(
        props.text,
        props.thinking,
        props.complete,
        props.meta,
        props.presentation,
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
    fn assistant_props_preserve_render_inputs() {
        let meta = SegmentMeta::default();
        let presentation = SegmentPresentation {
            role: SegmentRole::Assistant,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        };
        let props = AssistantRenderProps {
            text: "answer",
            thinking: "reasoning",
            complete: false,
            meta: &meta,
            presentation: &presentation,
            mode: SegmentRenderMode::Full,
        };
        assert_eq!(props.text, "answer");
        assert_eq!(props.thinking, "reasoning");
        assert!(!props.complete);
        assert_eq!(props.presentation.sigil, "Ω");
    }
}
