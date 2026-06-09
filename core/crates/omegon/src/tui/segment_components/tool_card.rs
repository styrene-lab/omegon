//! Tool-card segment component boundary.
//!
//! The first extraction slice gives tool cards a component-level API while the
//! large renderer body is moved incrementally out of `segments.rs` in follow-up
//! passes.

use ratatui::prelude::*;

use crate::surfaces::conversation::ToolCategory;

use super::super::segments::{self, SegmentMeta, SegmentRenderMode};
use super::super::theme::Theme;

pub struct ToolCardRenderProps<'a> {
    pub name: &'a str,
    pub detail_args: Option<&'a str>,
    pub detail_result: Option<&'a str>,
    pub is_error: bool,
    pub complete: bool,
    pub expanded: bool,
    pub live_partial: Option<&'a omegon_traits::PartialToolResult>,
    pub started_at: Option<std::time::Instant>,
    pub meta: &'a SegmentMeta,
    pub tool_category: Option<ToolCategory>,
    pub mode: SegmentRenderMode,
    pub density: crate::settings::ToolDetail,
    pub pinned: bool,
}

pub fn render(props: ToolCardRenderProps<'_>, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    segments::render_tool_card(
        props.name,
        props.detail_args,
        props.detail_result,
        props.is_error,
        props.complete,
        props.expanded,
        props.live_partial,
        props.started_at,
        props.meta,
        props.tool_category,
        area,
        buf,
        theme,
        props.mode,
        props.density,
        props.pinned,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_card_props_preserve_render_inputs() {
        let meta = SegmentMeta::default();
        let props = ToolCardRenderProps {
            name: "bash",
            detail_args: Some("cargo check"),
            detail_result: Some("ok"),
            is_error: false,
            complete: true,
            expanded: false,
            live_partial: None,
            started_at: None,
            meta: &meta,
            tool_category: Some(ToolCategory::CommandExec),
            mode: SegmentRenderMode::Full,
            density: crate::settings::ToolDetail::Detailed,
            pinned: true,
        };
        assert_eq!(props.name, "bash");
        assert_eq!(props.detail_args, Some("cargo check"));
        assert_eq!(props.tool_category, Some(ToolCategory::CommandExec));
        assert!(props.complete);
        assert!(props.pinned);
    }
}
