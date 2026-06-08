//! Ratatui-facing conversation render projection traits.
//!
//! This module is the adapter seam between semantic conversation projections and
//! terminal rendering. It lets the scroll/widget layer measure, render, and query
//! segment render metadata without pattern-matching on the underlying domain
//! segment enum.

use ratatui::prelude::*;

use super::segments::{Segment, SegmentRenderMode};
use super::theme::Theme;

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
