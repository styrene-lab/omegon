//! ConversationWidget — segment-based scrollable conversation view.
//!
//! Implements `StatefulWidget` with:
//! - Segment height caching (invalidated on resize/mutation)
//! - Visible-only rendering (only segments in the viewport are drawn)
//! - Scroll state with segment-awareness

use ratatui::prelude::*;

use super::conversation_render_projection::{
    SegmentMeasure, SegmentRender, SegmentRenderContext, SegmentRenderMetadata,
};
use super::segments::{Segment, SegmentRenderMode};
use super::theme::Theme;

/// Scroll state for the conversation widget.
pub struct ConvState {
    /// Pixel (row) offset from the bottom. 0 = showing latest content.
    pub scroll_offset: u16,
    /// True when the user has manually scrolled away from the bottom.
    pub user_scrolled: bool,
    /// Cached heights for each segment at the last known width.
    pub heights: Vec<u16>,
    /// Terminal width when heights were last computed.
    cached_width: u16,
    /// Render mode when heights were last computed.
    cached_mode: Option<SegmentRenderMode>,
    /// Number of segments when heights were last computed.
    cached_count: usize,
    /// Selected segment when heights were last computed.
    cached_selected_segment: Option<usize>,
    /// One-shot request to align the selected projected segment to the viewport top.
    snap_to_selected: bool,
    /// Total rendered height from the previous frame.
    /// Used to preserve a detached viewport when streaming grows content.
    last_total_height: u16,
}

impl ConvState {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            user_scrolled: false,
            heights: Vec::new(),
            cached_width: 0,
            cached_mode: None,
            cached_count: 0,
            cached_selected_segment: None,
            snap_to_selected: false,
            last_total_height: 0,
        }
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
        self.user_scrolled = self.scroll_offset > 0;
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        if self.scroll_offset == 0 {
            self.user_scrolled = false;
        }
    }

    pub fn auto_scroll_to_bottom(&mut self) {
        if !self.user_scrolled {
            self.scroll_offset = 0;
        }
    }

    pub fn force_scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.user_scrolled = false;
    }

    pub fn snap_to_selected(&mut self) {
        self.snap_to_selected = true;
        self.user_scrolled = true;
    }

    /// Invalidate height cache — call when segments change.
    pub fn invalidate(&mut self) {
        self.cached_count = 0;
    }

    /// Ensure heights are computed for all segments at the given width.
    fn ensure_heights(
        &mut self,
        segments: &[Segment],
        width: u16,
        t: &dyn Theme,
        mode: SegmentRenderMode,
    ) {
        let ctx = SegmentRenderContext::new(t, mode);
        self.ensure_heights_with_scroll_state(segments, width, &ctx, self.user_scrolled, None);
    }

    fn ensure_heights_with_scroll_state(
        &mut self,
        segments: &[Segment],
        width: u16,
        ctx: &SegmentRenderContext<'_>,
        user_scrolled: bool,
        selected_segment: Option<usize>,
    ) {
        // Full recompute if width changed
        if width != self.cached_width
            || self.cached_mode != Some(ctx.mode)
            || self.cached_selected_segment != selected_segment
        {
            self.heights.clear();
            self.cached_width = width;
            self.cached_mode = Some(ctx.mode);
            self.cached_selected_segment = selected_segment;
            self.cached_count = 0;
        }

        // Only compute new/changed segments
        if self.cached_count > segments.len() {
            // Segments were removed (shouldn't happen, but handle it)
            self.heights.truncate(segments.len());
            self.cached_count = segments.len();
        }

        // Recompute the last segment while attached, and always once it is
        // no longer live. Detached streaming tails intentionally keep their
        // cached height to preserve the operator's scroll anchor; completed
        // tails must be measured again or a final assistant response can look
        // hard-truncated after the turn is marked done.
        let last_is_live = segments
            .last()
            .is_some_and(SegmentRenderMetadata::is_live_render_segment);
        if !segments.is_empty()
            && self.cached_count == segments.len()
            && (!user_scrolled || !last_is_live)
        {
            let last = segments.len() - 1;
            self.heights[last] = measured_segment_height(
                &segments[last],
                width,
                ctx,
                selected_segment == Some(last),
            );
        }

        // Compute any new segments
        while self.cached_count < segments.len() {
            let h = measured_segment_height(
                &segments[self.cached_count],
                width,
                ctx,
                selected_segment == Some(self.cached_count),
            );
            if self.cached_count < self.heights.len() {
                self.heights[self.cached_count] = h;
            } else {
                self.heights.push(h);
            }
            self.cached_count += 1;
        }
    }

    /// Total height of all segments.
    fn total_height(&self) -> u16 {
        self.heights
            .iter()
            .copied()
            .fold(0u16, |acc, h| acc.saturating_add(h))
    }

    /// Compute on-screen areas for Image segments visible in the viewport.
    /// Called after render to know where to overlay actual images.
    pub fn visible_image_areas(&self, segments: &[Segment], viewport: Rect) -> Vec<(usize, Rect)> {
        if self.heights.len() != segments.len() {
            return vec![];
        }

        let viewport_height = viewport.height;
        let total_height = self.total_height();
        let top_offset = if total_height <= viewport_height {
            0
        } else {
            total_height
                - viewport_height
                - self
                    .scroll_offset
                    .min(total_height.saturating_sub(viewport_height))
        };
        // Keep this projection aligned with ConversationWidget::render. Short
        // conversations are bottom-anchored above the composer; using the raw
        // viewport origin here sends out-of-band terminal image protocols to
        // the top of the screen while their buffered chrome remains below.
        let viewport_origin_y = if total_height < viewport_height {
            viewport.y.saturating_add(viewport_height - total_height)
        } else {
            viewport.y
        };

        let mut result = Vec::new();
        let mut y_cursor: u16 = 0;
        for (i, segment) in segments.iter().enumerate() {
            let seg_height = self.heights[i];
            let seg_top = y_cursor;
            let seg_bottom = y_cursor.saturating_add(seg_height);
            y_cursor = seg_bottom;

            if seg_bottom <= top_offset {
                continue;
            }
            if seg_top >= top_offset + viewport_height {
                break;
            }

            if segment.is_image_render_segment() && seg_top >= top_offset {
                let render_y = viewport_origin_y + (seg_top - top_offset);
                let segment_area = Rect {
                    x: viewport.x,
                    y: render_y,
                    width: viewport.width,
                    height: seg_height,
                };
                let content_area = SelectedSegmentFrame::new(
                    self.cached_selected_segment == Some(i),
                    segment.capabilities().detail_openable,
                    segment.capabilities().copyable,
                    is_collapsed_expandable_tool_card(segment),
                )
                .content_area(segment_area);
                let available_height = viewport.bottom().saturating_sub(render_y);
                if available_height > 3 {
                    // Leave the image placeholder's one-cell border and bottom caption row.
                    result.push((
                        i,
                        Rect {
                            x: content_area.x.saturating_add(1),
                            y: content_area.y.saturating_add(1),
                            width: content_area.width.saturating_sub(2),
                            height: content_area
                                .height
                                .saturating_sub(3)
                                .min(available_height - 3),
                        },
                    ));
                }
            }
        }
        result
    }
}

impl Default for ConvState {
    fn default() -> Self {
        Self::new()
    }
}

/// The conversation widget — renders segments into a scrollable viewport.
pub struct ConversationWidget<'a> {
    segments: &'a [Segment],
    theme: &'a dyn Theme,
    mode: SegmentRenderMode,
    density: crate::settings::ToolDetail,
    pinned_segment: Option<usize>,
    selected_segment: Option<usize>,
    detail_hint_enabled: bool,
}

impl<'a> ConversationWidget<'a> {
    pub fn new(segments: &'a [Segment], theme: &'a dyn Theme) -> Self {
        Self {
            segments,
            theme,
            mode: SegmentRenderMode::Full,
            density: crate::settings::ToolDetail::Detailed,
            pinned_segment: None,
            selected_segment: None,
            detail_hint_enabled: false,
        }
    }

    pub fn with_mode(mut self, mode: SegmentRenderMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_density(mut self, density: crate::settings::ToolDetail) -> Self {
        self.density = density;
        self
    }

    pub fn with_pinned_segment(mut self, pinned_segment: Option<usize>) -> Self {
        self.pinned_segment = pinned_segment;
        self
    }

    pub fn with_selected_segment(mut self, selected_segment: Option<usize>) -> Self {
        self.selected_segment = selected_segment;
        self
    }

    pub fn with_detail_hint_enabled(mut self, enabled: bool) -> Self {
        self.detail_hint_enabled = enabled;
        self
    }
}

impl<'a> StatefulWidget for ConversationWidget<'a> {
    type State = ConvState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ConvState) {
        // Own the entire viewport every frame so shorter/shrinking content,
        // partial clipping, and out-of-band terminal corruption cannot leave
        // stale glyphs behind in the conversation region.
        let bg = self.theme.surface_bg();
        let fg = self.theme.fg();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(' ');
                    cell.set_bg(bg);
                    cell.set_fg(fg);
                }
            }
        }

        if area.width == 0 || area.height == 0 || self.segments.is_empty() {
            return;
        }

        // Ensure all segment heights are computed
        let measure_ctx = SegmentRenderContext::new(self.theme, self.mode)
            .with_density(self.density)
            .with_selected(false);
        state.ensure_heights_with_scroll_state(
            self.segments,
            area.width,
            &measure_ctx,
            state.user_scrolled,
            self.selected_segment,
        );

        let viewport_height = area.height;
        let total_height = state.total_height();

        if state.snap_to_selected {
            if let Some(selected) = self.selected_segment {
                let selected_top: u16 = state.heights[..selected].iter().copied().sum();
                state.scroll_offset = total_height
                    .saturating_sub(viewport_height)
                    .saturating_sub(selected_top);
            }
            state.snap_to_selected = false;
        } else if state.user_scrolled && total_height > state.last_total_height {
            state.scroll_offset = state
                .scroll_offset
                .saturating_add(total_height - state.last_total_height);
        }

        // Clamp scroll offset so we don't scroll past the top
        let max_scroll = total_height.saturating_sub(viewport_height);
        if state.scroll_offset > max_scroll {
            state.scroll_offset = max_scroll;
        }
        state.last_total_height = total_height;

        // The scroll_offset is from the BOTTOM (0 = at bottom).
        // Convert to a top-based offset for rendering.
        let top_offset = if total_height <= viewport_height {
            0 // Content fits — no scrolling
        } else {
            total_height - viewport_height - state.scroll_offset
        };
        // Keep short conversations attached to the composer instead of pinning
        // them to the top of a tall viewport and leaving a false "rendering
        // boundary" below the latest message.
        let viewport_origin_y = if total_height < viewport_height {
            area.y.saturating_add(viewport_height - total_height)
        } else {
            area.y
        };

        // Walk segments to find which ones are visible.
        // Segments partially above the viewport are rendered into a temp buffer
        // and the visible portion is copied into the main buffer (proper clipping).
        let mut y_cursor: u16 = 0;
        for (i, segment) in self.segments.iter().enumerate() {
            let seg_height = state.heights[i];
            let seg_top = y_cursor;
            let seg_bottom = y_cursor.saturating_add(seg_height);
            y_cursor = seg_bottom;

            // Skip segments entirely above the viewport
            if seg_bottom <= top_offset {
                continue;
            }
            // Stop once we're past the viewport bottom
            if seg_top >= top_offset + viewport_height {
                break;
            }

            if seg_top >= top_offset {
                // Segment starts within the viewport — render directly
                let render_y = viewport_origin_y + (seg_top - top_offset);
                let available_height = area.bottom().saturating_sub(render_y);
                if available_height == 0 {
                    continue;
                }

                let seg_area = Rect {
                    x: area.x,
                    y: render_y,
                    width: area.width,
                    height: seg_height.min(available_height),
                };
                let selected = self.selected_segment == Some(i);
                let render_ctx = SegmentRenderContext::new(self.theme, self.mode)
                    .with_density(self.density)
                    .with_pinned(self.pinned_segment == Some(i))
                    .with_selected(selected);
                SelectedSegmentFrame::new(
                    selected,
                    segment.capabilities().detail_openable,
                    segment.capabilities().copyable,
                    is_collapsed_expandable_tool_card(segment),
                )
                .render(seg_area, buf, self.theme, |content_area, buf| {
                    segment.render_in_context(content_area, buf, &render_ctx);
                });
                render_assistant_copy_affordance(seg_area, buf, self.theme, segment);
            } else {
                // Segment starts ABOVE the viewport — partially visible.
                // Render into a temp buffer at full size, then copy the
                // visible portion into the main buffer.
                let clip_rows = top_offset - seg_top; // rows clipped from the top
                let visible_rows = seg_height.saturating_sub(clip_rows).min(viewport_height);
                if visible_rows == 0 {
                    continue;
                }

                let temp_area = Rect::new(0, 0, area.width, seg_height);
                let mut temp_buf = Buffer::empty(temp_area);
                // Fill temp buffer with theme bg so clipped cells don't
                // bleed Color::Reset into the main buffer
                let bg = self.theme.surface_bg();
                let fg = self.theme.fg();
                for y in 0..seg_height {
                    for x in 0..area.width {
                        let cell = &mut temp_buf[(x, y)];
                        cell.set_bg(bg);
                        cell.set_fg(fg);
                    }
                }
                let selected = self.selected_segment == Some(i);
                let render_ctx = SegmentRenderContext::new(self.theme, self.mode)
                    .with_density(self.density)
                    .with_pinned(self.pinned_segment == Some(i))
                    .with_selected(selected);
                SelectedSegmentFrame::new(
                    selected,
                    segment.capabilities().detail_openable,
                    segment.capabilities().copyable,
                    is_collapsed_expandable_tool_card(segment),
                )
                .render(
                    temp_area,
                    &mut temp_buf,
                    self.theme,
                    |content_area, buf| {
                        segment.render_in_context(content_area, buf, &render_ctx);
                    },
                );
                render_assistant_copy_affordance(temp_area, &mut temp_buf, self.theme, segment);
                // Copy the visible portion from temp_buf to main buf
                for row in 0..visible_rows {
                    let src_y = clip_rows + row;
                    let dst_y = viewport_origin_y + row;
                    if dst_y >= area.bottom() {
                        break;
                    }
                    for x in 0..area.width {
                        if src_y < seg_height
                            && let Some(cell) = buf.cell_mut((area.x + x, dst_y))
                        {
                            *cell = temp_buf[(x, src_y)].clone();
                        }
                    }
                }
            }
        }

        if matches!(self.mode, SegmentRenderMode::Slim) && state.scroll_offset > 0 {
            render_detached_viewport_hint(area, buf, self.theme, state.scroll_offset);
        }
        if self.detail_hint_enabled
            && let Some(selected) = self.selected_segment
            && self
                .segments
                .get(selected)
                .is_some_and(|segment| segment.capabilities().detail_openable)
        {
            render_detail_affordance_hint(area, buf, self.theme);
        }
    }
}

fn render_detail_affordance_hint(area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let label = " Enter: details ";
    let label_width = label.chars().count() as u16;
    if label_width > area.width {
        return;
    }

    let x = area.right().saturating_sub(label_width);
    let y = area.bottom().saturating_sub(1);
    let style = Style::default()
        .fg(theme.accent_bright())
        .bg(theme.surface_bg())
        .add_modifier(Modifier::BOLD);
    for (idx, ch) in label.chars().enumerate() {
        if let Some(cell) = buf.cell_mut((x + idx as u16, y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}

fn measured_segment_height(
    segment: &Segment,
    width: u16,
    ctx: &SegmentRenderContext<'_>,
    selected: bool,
) -> u16 {
    let content_width = SelectedSegmentFrame::new(
        selected,
        segment.capabilities().detail_openable,
        segment.capabilities().copyable,
        is_collapsed_expandable_tool_card(segment),
    )
    .content_area(Rect::new(0, 0, width, 1))
    .width;
    segment.height_in_context(content_width, ctx)
}

fn render_assistant_copy_affordance(
    area: Rect,
    buf: &mut Buffer,
    theme: &dyn Theme,
    segment: &Segment,
) {
    const LABEL: &str = " Copy ";
    let eligible = matches!(
        &segment.content,
        super::segments::SegmentContent::AssistantText { complete: true, .. }
    );
    let label_width = LABEL.chars().count() as u16;
    if !eligible || area.height == 0 || area.width < label_width {
        return;
    }

    let x = area.right().saturating_sub(label_width);
    let style = Style::default()
        .fg(theme.bg())
        .bg(theme.accent_bright())
        .add_modifier(Modifier::BOLD);
    for (offset, ch) in LABEL.chars().enumerate() {
        if let Some(cell) = buf.cell_mut((x + offset as u16, area.y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}

fn is_collapsed_expandable_tool_card(segment: &Segment) -> bool {
    matches!(
        &segment.content,
        super::segments::SegmentContent::ToolCard {
            expanded: false,
            complete: true,
            detail_result: Some(_),
            ..
        }
    )
}

struct SelectedSegmentFrame {
    selected: bool,
    detail_openable: bool,
    copyable: bool,
    collapsed_expandable: bool,
}

impl SelectedSegmentFrame {
    fn new(
        selected: bool,
        detail_openable: bool,
        copyable: bool,
        collapsed_expandable: bool,
    ) -> Self {
        Self {
            selected,
            detail_openable,
            copyable,
            collapsed_expandable,
        }
    }

    fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        theme: &dyn Theme,
        render_content: impl FnOnce(Rect, &mut Buffer),
    ) {
        let content_area = self.content_area(area);
        render_content(content_area, buf);
        if self.selected {
            self.render_chrome(area, buf, theme);
        }
    }

    fn content_area(&self, area: Rect) -> Rect {
        if self.selected && area.width > 1 {
            Rect {
                x: area.x.saturating_add(1),
                y: area.y,
                width: area.width.saturating_sub(1),
                height: area.height,
            }
        } else {
            area
        }
    }

    fn render_chrome(&self, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let style = Style::default()
            .fg(theme.accent_bright())
            .bg(theme.surface_bg())
            .add_modifier(Modifier::BOLD);
        for (row, y) in (area.top()..area.bottom()).enumerate() {
            let marker = match (self.detail_openable, row == 0) {
                (true, true) => '◆',
                _ => '│',
            };
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_char(marker);
                cell.set_style(style);
            }
        }
        self.render_hint(area, buf, theme);
    }

    fn render_hint(&self, area: Rect, buf: &mut Buffer, theme: &dyn Theme) {
        if area.width < 24 || area.height == 0 {
            return;
        }
        let label = if self.collapsed_expandable {
            " dbl-click expand "
        } else if self.copyable {
            " dbl-click copy "
        } else if self.detail_openable {
            " Enter details "
        } else {
            " selected "
        };
        let label_width = label.chars().count() as u16;
        if label_width.saturating_add(2) >= area.width {
            return;
        }
        let x = area.right().saturating_sub(label_width);
        let y = area.y;
        let style = Style::default()
            .fg(theme.bg())
            .bg(theme.accent_bright())
            .add_modifier(Modifier::BOLD);
        for (idx, ch) in label.chars().enumerate() {
            if let Some(cell) = buf.cell_mut((x + idx as u16, y)) {
                cell.set_char(ch);
                cell.set_style(style);
            }
        }
    }
}

fn render_detached_viewport_hint(
    area: Rect,
    buf: &mut Buffer,
    theme: &dyn Theme,
    scroll_offset: u16,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let y = area.bottom().saturating_sub(1);
    let label = format!(" more below · End to tail · +{scroll_offset} ");
    let left_rule = "─";
    let label_width = label.chars().count() as u16;
    let right_rule_width = area.width.saturating_sub(label_width + 2) as usize;
    let text = format!("{left_rule}{label}{}", "─".repeat(right_rule_width));
    let style = Style::default()
        .fg(theme.muted())
        .bg(theme.surface_bg())
        .add_modifier(Modifier::BOLD);

    for x in area.left()..area.right() {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(' ');
            cell.set_style(Style::default().bg(theme.surface_bg()));
        }
    }
    for (idx, ch) in text.chars().take(area.width as usize).enumerate() {
        if let Some(cell) = buf.cell_mut((area.x + idx as u16, y)) {
            cell.set_char(ch);
            cell.set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::conversation_render_projection::SegmentRenderContext;
    use crate::tui::segments::{Segment, SegmentContent};
    use crate::tui::theme::Alpharius;
    use ratatui::{buffer::Buffer, layout::Rect};

    fn buffer_text(buf: &Buffer, area: Rect) -> String {
        let mut text = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn empty_segments_renders_nothing() {
        let segments: Vec<Segment> = vec![];
        let widget = ConversationWidget::new(&segments, &Alpharius);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);
        // Should not panic
    }

    #[test]
    fn detached_viewport_hint_is_neutral_navigation_chrome() {
        let area = Rect::new(0, 0, 48, 2);
        let mut buf = Buffer::empty(area);
        render_detached_viewport_hint(area, &mut buf, &Alpharius, 17);

        let y = area.bottom() - 1;
        let styled_cells = (area.left()..area.right())
            .filter_map(|x| buf.cell((x, y)))
            .filter(|cell| cell.symbol() != " ")
            .collect::<Vec<_>>();
        assert!(
            !styled_cells.is_empty(),
            "hint should render visible chrome"
        );
        assert!(
            styled_cells.iter().all(|cell| cell.fg == Alpharius.muted()),
            "detached navigation hint must remain neutral"
        );
        assert!(
            styled_cells
                .iter()
                .all(|cell| cell.fg != Alpharius.warning()),
            "detached navigation hint must not consume the attention color"
        );
    }

    #[test]
    fn single_segment_renders() {
        let segments = vec![Segment::user_prompt("hello")];
        let widget = ConversationWidget::new(&segments, &Alpharius);
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);

        // Check that something was rendered
        let mut found = false;
        for y in 0..24 {
            for x in 0..80 {
                if buf[(x, y)].symbol() != " " {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "should render something");
    }

    #[test]
    fn short_conversation_is_bottom_anchored_to_the_composer() {
        let segments = vec![Segment::user_prompt("hello")];
        let widget = ConversationWidget::new(&segments, &Alpharius);
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);

        let occupied_rows = (0..area.height)
            .filter(|&y| (0..area.width).any(|x| buf[(x, y)].symbol() != " "))
            .collect::<Vec<_>>();
        assert!(!occupied_rows.is_empty(), "prompt should render");
        assert_eq!(
            occupied_rows.last().copied(),
            Some(area.height - 1),
            "short content should end at the bottom of the conversation viewport"
        );
    }

    #[test]
    fn short_image_overlay_is_bottom_anchored_with_its_chrome() {
        let segments = vec![Segment::image("/tmp/paste.png".into(), "pasted image")];
        let widget = ConversationWidget::new(&segments, &Alpharius);
        let area = Rect::new(4, 3, 40, 20);
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 30));
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);

        let image_areas = state.visible_image_areas(&segments, area);
        assert_eq!(image_areas.len(), 1);
        let (_, image_area) = image_areas[0];
        let segment_height = state.heights[0];
        let expected_segment_y = area.bottom() - segment_height;
        assert_eq!(image_area.x, area.x + 1);
        assert_eq!(image_area.y, expected_segment_y + 1);
        assert!(
            image_area.y > area.y,
            "overlay must not be pinned to viewport top"
        );
        assert!(image_area.bottom() < area.bottom());
    }

    #[test]
    fn scroll_state_lifecycle() {
        let mut state = ConvState::new();
        assert_eq!(state.scroll_offset, 0);
        assert!(!state.user_scrolled);

        state.scroll_up(5);
        assert_eq!(state.scroll_offset, 5);
        assert!(state.user_scrolled);

        state.scroll_down(3);
        assert_eq!(state.scroll_offset, 2);
        assert!(state.user_scrolled);

        state.scroll_down(10);
        assert_eq!(state.scroll_offset, 0);
        assert!(!state.user_scrolled);
    }

    #[test]
    fn force_scroll_resets() {
        let mut state = ConvState::new();
        state.scroll_up(10);
        assert!(state.user_scrolled);

        state.force_scroll_to_bottom();
        assert_eq!(state.scroll_offset, 0);
        assert!(!state.user_scrolled);
    }

    #[test]
    fn height_cache_works() {
        let segments = vec![
            Segment::separator(),
            Segment::user_prompt("test"),
            Segment::separator(),
        ];
        let mut state = ConvState::new();
        state.ensure_heights(&segments, 80, &Alpharius, SegmentRenderMode::Full);
        assert_eq!(state.heights.len(), 3);
        assert_eq!(state.heights[0], 1); // separator
        assert_eq!(state.heights[2], 1); // separator
    }

    #[test]
    fn detached_scroll_skips_last_segment_remeasure() {
        let segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "streaming tail".into(),
                thinking: String::new(),
                complete: false,
            },
        }];
        let mut state = ConvState::new();
        state.ensure_heights(&segments, 40, &Alpharius, SegmentRenderMode::Full);
        state.heights[0] = 7;
        state.cached_count = segments.len();
        state.cached_width = 40;
        state.cached_mode = Some(SegmentRenderMode::Full);
        state.user_scrolled = true;

        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Full);
        state.ensure_heights_with_scroll_state(&segments, 40, &ctx, true, None);
        assert_eq!(
            state.heights[0], 7,
            "detached viewport should preserve cached tail height instead of remeasuring it"
        );
    }

    #[test]
    fn detached_scroll_remeasures_completed_last_segment() {
        let segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "completed tail now has enough content to wrap across several terminal rows"
                    .into(),
                thinking: String::new(),
                complete: true,
            },
        }];
        let mut state = ConvState::new();
        state.heights = vec![1];
        state.cached_count = segments.len();
        state.cached_width = 20;
        state.cached_mode = Some(SegmentRenderMode::Slim);
        state.user_scrolled = true;

        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Slim);
        state.ensure_heights_with_scroll_state(&segments, 20, &ctx, true, None);
        assert!(
            state.heights[0] > 1,
            "completed detached tail must be remeasured so it cannot look truncated"
        );
    }

    #[test]
    fn multiple_segments_render() {
        let segments = vec![
            Segment::user_prompt("first"),
            Segment {
                meta: Default::default(),
                content: SegmentContent::AssistantText {
                    text: "response".into(),
                    thinking: String::new(),
                    complete: true,
                },
            },
            Segment {
                meta: Default::default(),
                content: SegmentContent::ToolCard {
                    id: "1".into(),
                    name: "bash".into(),
                    args_summary: None,
                    detail_args: Some("echo hi".into()),
                    result_summary: None,
                    detail_result: Some("hi".into()),
                    is_error: false,
                    complete: true,
                    expanded: false,
                    live_partial: None,
                    started_at: None,
                },
            },
        ];
        let widget = ConversationWidget::new(&segments, &Alpharius);
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);
        // Should render all three without panic
        assert!(state.heights.len() == 3);
    }

    #[test]
    fn detached_viewport_keeps_anchor_when_streaming_grows_last_segment() {
        let area = Rect::new(0, 0, 20, 4);
        let mut state = ConvState::new();

        let initial_segments = vec![
            Segment::user_prompt("operator"),
            Segment {
                meta: Default::default(),
                content: SegmentContent::AssistantText {
                    text: "one two three four five six seven eight".into(),
                    thinking: String::new(),
                    complete: false,
                },
            },
        ];

        ConversationWidget::new(&initial_segments, &Alpharius).render(
            area,
            &mut Buffer::empty(area),
            &mut state,
        );

        state.scroll_up(3);
        let detached_offset = state.scroll_offset;
        let old_total = state.last_total_height;

        let grown_segments = vec![
            Segment::user_prompt("operator"),
            Segment {
                meta: Default::default(),
                content: SegmentContent::AssistantText {
                    text: "one two three four five six seven eight nine ten eleven twelve thirteen fourteen".into(),
                    thinking: String::new(),
                    complete: false,
                },
            },
        ];

        ConversationWidget::new(&grown_segments, &Alpharius).render(
            area,
            &mut Buffer::empty(area),
            &mut state,
        );

        assert_eq!(
            state.last_total_height, old_total,
            "detached viewport should preserve cached total height while the streaming tail is off-screen"
        );
        assert_eq!(
            state.scroll_offset, detached_offset,
            "detached viewport should preserve the operator's scroll anchor instead of chasing the live tail"
        );
    }

    #[test]
    fn live_tail_stays_at_bottom_when_streaming_grows_last_segment() {
        let area = Rect::new(0, 0, 20, 4);
        let mut state = ConvState::new();

        let initial_segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "one two three four five six seven eight".into(),
                thinking: String::new(),
                complete: false,
            },
        }];

        ConversationWidget::new(&initial_segments, &Alpharius).render(
            area,
            &mut Buffer::empty(area),
            &mut state,
        );
        assert_eq!(state.scroll_offset, 0);

        let grown_segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "one two three four five six seven eight nine ten eleven twelve thirteen fourteen".into(),
                thinking: String::new(),
                complete: false,
            },
        }];

        ConversationWidget::new(&grown_segments, &Alpharius).render(
            area,
            &mut Buffer::empty(area),
            &mut state,
        );

        assert_eq!(
            state.scroll_offset, 0,
            "live tail should continue following output"
        );
    }

    #[test]
    fn slim_detached_viewport_marks_more_content_below() {
        let segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "first page\n```json\n{\"pattern\":\"armory\"}\n```\nmore explanation".into(),
                thinking: String::new(),
                complete: true,
            },
        }];
        let area = Rect::new(0, 0, 80, 3);
        let mut state = ConvState::new();
        state.scroll_offset = 2;
        state.user_scrolled = true;

        let mut buf = Buffer::empty(area);
        ConversationWidget::new(&segments, &Alpharius)
            .with_mode(SegmentRenderMode::Slim)
            .render(area, &mut buf, &mut state);

        let text = buffer_text(&buf, area);
        assert!(
            text.contains("more below · End to tail"),
            "detached slim viewport should not look like truncated content: {text}"
        );
    }

    #[test]
    fn selected_segment_height_uses_gutter_content_width() {
        let segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "abcd efgh".into(),
                thinking: String::new(),
                complete: true,
            },
        }];
        let mut state = ConvState::new();
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Slim);

        state.ensure_heights_with_scroll_state(&segments, 9, &ctx, false, None);
        assert_eq!(state.heights, vec![1]);

        state.ensure_heights_with_scroll_state(&segments, 9, &ctx, false, Some(0));
        assert_eq!(state.heights, vec![2]);
    }

    #[test]
    fn selected_plain_prose_segment_preserves_first_character() {
        let segments = vec![Segment {
            meta: Default::default(),
            content: SegmentContent::AssistantText {
                text: "inspect me".into(),
                thinking: String::new(),
                complete: true,
            },
        }];
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();

        ConversationWidget::new(&segments, &Alpharius)
            .with_mode(SegmentRenderMode::Slim)
            .with_selected_segment(Some(0))
            .render(area, &mut buf, &mut state);

        let rendered = buffer_text(&buf, area);
        assert!(
            rendered.lines().any(|line| line.starts_with("◆inspect me")),
            "selection frame should wrap plain prose instead of replacing the first character: {rendered}"
        );
    }

    #[test]
    fn selected_image_area_respects_selection_gutter() {
        let segments = vec![Segment::image("/tmp/example.png".into(), "example")];
        let viewport = Rect::new(10, 0, 20, 14);
        let mut state = ConvState::new();
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Full);
        state.ensure_heights_with_scroll_state(&segments, viewport.width, &ctx, false, None);

        let unselected = state.visible_image_areas(&segments, viewport);
        assert_eq!(unselected.len(), 1);
        assert_eq!(unselected[0].1.x, 11);
        assert_eq!(unselected[0].1.width, 18);

        state.ensure_heights_with_scroll_state(&segments, viewport.width, &ctx, false, Some(0));
        let selected = state.visible_image_areas(&segments, viewport);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].1.x, 12);
        assert_eq!(selected[0].1.width, 17);
    }

    #[test]
    fn selected_detail_openable_segment_shows_detail_hint_and_marker() {
        let segments = vec![Segment::user_prompt("inspect me")];
        let area = Rect::new(0, 0, 60, 6);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();

        ConversationWidget::new(&segments, &Alpharius)
            .with_selected_segment(Some(0))
            .with_detail_hint_enabled(true)
            .render(area, &mut buf, &mut state);

        let rendered = buffer_text(&buf, area);
        assert!(
            rendered.contains("dbl-click copy"),
            "selected copyable segment should advertise double-click copy: {rendered}"
        );
        assert!(
            rendered
                .lines()
                .next()
                .is_some_and(|line| line.starts_with('◆')),
            "detail-openable selection should mark the segment start: {rendered}"
        );
        assert!(
            rendered.contains("││ inspect me"),
            "selection rail should not replace the first content character: {rendered}"
        );
    }

    #[test]
    fn selected_non_openable_segment_marks_focus_without_detail_hint() {
        let segments = vec![Segment::separator()];
        let area = Rect::new(0, 0, 60, 6);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();

        ConversationWidget::new(&segments, &Alpharius)
            .with_selected_segment(Some(0))
            .render(area, &mut buf, &mut state);

        let rendered = buffer_text(&buf, area);
        assert!(!rendered.contains("dbl-click copy"), "{rendered}");
        assert!(
            rendered
                .lines()
                .any(|line| line.starts_with("│ ") || line.starts_with("│─")),
            "selected non-openable segment should still show focus without replacing content: {rendered}"
        );
    }

    #[test]
    fn selected_collapsed_tool_card_advertises_double_click_expand() {
        let mut segment = Segment::tool_card("call-1", "read");
        if let SegmentContent::ToolCard {
            complete,
            detail_result,
            ..
        } = &mut segment.content
        {
            *complete = true;
            *detail_result = Some("file contents".into());
        }
        let segments = vec![segment];
        let area = Rect::new(0, 0, 72, 6);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();

        ConversationWidget::new(&segments, &Alpharius)
            .with_selected_segment(Some(0))
            .render(area, &mut buf, &mut state);

        let rendered = buffer_text(&buf, area);
        assert!(
            rendered.contains("dbl-click expand"),
            "selected collapsed tool card should advertise double-click expand: {rendered}"
        );
    }

    #[test]
    fn projected_success_outcome_is_neutral_in_slim_conversation_buffer() {
        use crate::surfaces::layout::UiPresentationLevel;
        use crate::tui::conversation_projection::project_conversation;

        let mut first = Segment::tool_card("tool-1", "read");
        first.meta.turn = Some(1);
        if let SegmentContent::ToolCard {
            complete,
            detail_result,
            ..
        } = &mut first.content
        {
            *complete = true;
            *detail_result = Some("file contents".into());
        }
        let projected = project_conversation(&[first], UiPresentationLevel::Om);
        assert!(matches!(
            projected.segments.as_slice(),
            [Segment {
                content: SegmentContent::SystemNotification { text },
                ..
            }] if text.starts_with('✓')
        ));

        let area = Rect::new(0, 0, 72, 3);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        ConversationWidget::new(&projected.segments, &Alpharius)
            .with_mode(SegmentRenderMode::Slim)
            .render(area, &mut buf, &mut state);

        let mut colors = Vec::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell((x, y))
                    && cell.symbol() != " "
                {
                    colors.push(cell.fg);
                }
            }
        }
        assert!(!colors.is_empty());
        assert!(
            colors.iter().all(|color| *color != Alpharius.warning()),
            "successful projected outcome must not consume attention orange"
        );
        assert!(
            colors.iter().any(|color| *color == Alpharius.muted()),
            "successful projected outcome should render as neutral transcript evidence"
        );
    }

    #[test]
    fn slim_mode_renders_lighter_segment_chrome() {
        let segments = vec![
            Segment::user_prompt("hello"),
            Segment {
                meta: Default::default(),
                content: SegmentContent::AssistantText {
                    text: "response".into(),
                    thinking: String::new(),
                    complete: true,
                },
            },
        ];
        let widget =
            ConversationWidget::new(&segments, &Alpharius).with_mode(SegmentRenderMode::Slim);
        let area = Rect::new(0, 0, 80, 12);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);

        let mut text = String::new();
        for y in 0..12 {
            for x in 0..80 {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(
            !text.contains("╭"),
            "slim mode should avoid rounded card chrome: {text}"
        );
        assert!(
            !text.contains("│"),
            "slim mode should avoid left-rule chrome too: {text}"
        );
    }

    #[test]
    fn slim_mode_hides_plan_snapshots_from_scrollback() {
        let segments = vec![
            Segment::system("Plan progress\nPlan mode: executing\nProgress: 1/2\n\n1. ◐ Do it"),
            Segment::user_prompt("hello"),
        ];
        let widget =
            ConversationWidget::new(&segments, &Alpharius).with_mode(SegmentRenderMode::Slim);
        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        let mut state = ConvState::new();
        widget.render(area, &mut buf, &mut state);

        assert_eq!(state.heights[0], 0);
        let mut text = String::new();
        for y in 0..8 {
            for x in 0..80 {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(!text.contains("Plan progress"), "{text}");
    }
}
