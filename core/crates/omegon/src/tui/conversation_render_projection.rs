//! Ratatui-facing conversation render projection traits.
//!
//! This module is the adapter seam between semantic conversation projections and
//! terminal rendering. It lets the scroll/widget layer measure, render, and query
//! segment render metadata without pattern-matching on the underlying domain
//! segment enum.

use ratatui::prelude::*;

use super::segments::{Segment, SegmentRenderMode};
use super::theme::Theme;
use crate::surfaces::conversation::ToolCategory;

pub fn tool_category_color(kind: ToolCategory, t: &dyn Theme) -> Color {
    match kind {
        ToolCategory::CommandExec => t.warning(),
        ToolCategory::FileRead => t.accent_muted(),
        ToolCategory::FileMutation => t.caution(),
        ToolCategory::DesignTree => t.accent_bright(),
        ToolCategory::Memory => t.accent(),
        ToolCategory::Search => t.accent_muted(),
        ToolCategory::Generic => t.border_dim(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentChrome {
    pub role_label: &'static str,
    pub sigil: &'static str,
    pub role_color: Color,
    pub content_color: Color,
}

pub fn segment_chrome(
    presentation: crate::surfaces::conversation::SegmentPresentation,
    selected: bool,
    t: &dyn Theme,
) -> SegmentChrome {
    use crate::surfaces::conversation::SegmentRole;

    let (role_label, sigil, role_color) = match presentation.role {
        SegmentRole::Operator => ("operator", "OP", t.accent()),
        SegmentRole::Assistant => ("assistant", "Ω", t.success()),
        SegmentRole::Tool => {
            let category = presentation.tool_category.unwrap_or(ToolCategory::Generic);
            (category.label(), "⚙", tool_category_color(category, t))
        }
        SegmentRole::System => ("system", "ℹ", t.dim()),
        SegmentRole::Lifecycle => ("event", "⚡", t.dim()),
        SegmentRole::Media => ("media", "◈", t.accent_muted()),
        SegmentRole::Separator => ("separator", "", t.dim()),
    };
    let content_color = match presentation.role {
        SegmentRole::Tool if !selected => t.muted(),
        _ => t.fg(),
    };

    SegmentChrome {
        role_label,
        sigil,
        role_color,
        content_color,
    }
}

#[derive(Clone, Copy)]
pub struct SegmentRenderContext<'a> {
    pub theme: &'a dyn Theme,
    pub mode: SegmentRenderMode,
    pub density: crate::settings::ToolDetail,
    pub pinned: bool,
}

impl<'a> SegmentRenderContext<'a> {
    pub fn new(theme: &'a dyn Theme, mode: SegmentRenderMode) -> Self {
        Self {
            theme,
            mode,
            density: crate::settings::ToolDetail::Detailed,
            pinned: false,
        }
    }

    pub fn with_density(mut self, density: crate::settings::ToolDetail) -> Self {
        self.density = density;
        self
    }

    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }
}

pub trait SegmentMeasure {
    fn height_in_context(&self, width: u16, ctx: &SegmentRenderContext<'_>) -> u16;
}

pub trait SegmentRender {
    fn render_in_context(&self, area: Rect, buf: &mut Buffer, ctx: &SegmentRenderContext<'_>);
}

pub trait SegmentRenderMetadata {
    fn is_live_render_segment(&self) -> bool;
    fn is_image_render_segment(&self) -> bool;
}

pub trait RenderableConversationSegment:
    SegmentMeasure + SegmentRender + SegmentRenderMetadata
{
}

impl<T> RenderableConversationSegment for T where
    T: SegmentMeasure + SegmentRender + SegmentRenderMetadata
{
}

impl SegmentMeasure for Segment {
    fn height_in_context(&self, width: u16, ctx: &SegmentRenderContext<'_>) -> u16 {
        self.height_in_mode(width, ctx.theme, ctx.mode)
    }
}

impl SegmentRender for Segment {
    fn render_in_context(&self, area: Rect, buf: &mut Buffer, ctx: &SegmentRenderContext<'_>) {
        self.render_with_pinned(area, buf, ctx.theme, ctx.mode, ctx.density, ctx.pinned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{
        SegmentEmphasis, SegmentPresentation, SegmentRole, ToolCategory,
    };
    use crate::tui::theme::Alpharius;

    fn presentation(role: SegmentRole, tool_category: Option<ToolCategory>) -> SegmentPresentation {
        SegmentPresentation {
            role,
            sigil: "",
            emphasis: SegmentEmphasis::Normal,
            tool_category,
        }
    }

    #[test]
    fn segment_chrome_maps_operator_identity() {
        let chrome = segment_chrome(presentation(SegmentRole::Operator, None), false, &Alpharius);
        assert_eq!(chrome.role_label, "operator");
        assert_eq!(chrome.sigil, "OP");
        assert_eq!(chrome.role_color, Alpharius.accent());
        assert_eq!(chrome.content_color, Alpharius.fg());
    }

    #[test]
    fn segment_chrome_mutes_unselected_tool_content() {
        let chrome = segment_chrome(
            presentation(SegmentRole::Tool, Some(ToolCategory::CommandExec)),
            false,
            &Alpharius,
        );
        assert_eq!(chrome.role_label, "exec");
        assert_eq!(chrome.sigil, "⚙");
        assert_eq!(
            chrome.role_color,
            tool_category_color(ToolCategory::CommandExec, &Alpharius)
        );
        assert_eq!(chrome.content_color, Alpharius.muted());
    }

    #[test]
    fn segment_chrome_selected_tool_uses_foreground_content() {
        let chrome = segment_chrome(
            presentation(SegmentRole::Tool, Some(ToolCategory::Memory)),
            true,
            &Alpharius,
        );
        assert_eq!(chrome.role_label, "memory");
        assert_eq!(chrome.content_color, Alpharius.fg());
    }
}
