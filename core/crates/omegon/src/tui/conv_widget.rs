//! ConversationWidget — segment-based scrollable conversation view.
//!
//! Implements `StatefulWidget` with:
//! - Segment height caching (invalidated on resize/mutation)
//! - Visible-only rendering (only segments in the viewport are drawn)
//! - Scroll state with segment-awareness

use ratatui::prelude::*;

use super::segments::{Segment, SegmentContent, SegmentRenderMode};
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
    /// Number of segments when heights were last computed.
    cached_count: usize,
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
            cached_count: 0,
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

    /// Invalidate height cache — call when segments change.
    pub fn invalidate(&mut self) {
        self.cached_count = 0;
    }

    /// Ensure heights are computed for all segments at the given width.
    fn ensure_heights(&mut self, segments: &[Segment], width: u16, t: &dyn Theme) {
        self.ensure_heights_with_scroll_state(segments, width, t, self.user_scrolled);
    }

    fn ensure_heights_with_scroll_state(
        &mut self,
        segments: &[Segment],
        width: u16,
        t: &dyn Theme,
        user_scrolled: bool,
    ) {
        // Full recompute if width changed
        if width != self.cached_width {
            self.heights.clear();
            self.cached_width = width;
            self.cached_count = 0;
        }

        // Only compute new/changed segments
        if self.cached_count > segments.len() {
            // Segments were removed (shouldn't happen, but handle it)
            self.heights.truncate(segments.len());
            self.cached_count = segments.len();
        }

        // Recompute the last segment only when the viewport is attached to the live tail.
        // When manually detached, the streaming tail is often off-screen, and remeasuring
        // it on every chunk creates avoidable scroll jank.
        if !segments.is_empty() && self.cached_count == segments.len() && !user_scrolled {
            let last = segments.len() - 1;
            self.heights[last] = segments[last].height(width, t);
        }

        // Compute any new segments
        while self.cached_count < segments.len() {
            let h = segments[self.cached_count].height(width, t);
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
        self.heights.iter().copied().sum()
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

        let mut result = Vec::new();
        let mut y_cursor: u16 = 0;
        for (i, segment) in segments.iter().enumerate() {
            let seg_height = self.heights[i];
            let seg_top = y_cursor;
            let seg_bottom = y_cursor + seg_height;
            y_cursor = seg_bottom;

            if seg_bottom <= top_offset {
                continue;
            }
            if seg_top >= top_offset + viewport_height {
                break;
            }

            if matches!(segment.content, SegmentContent::Image { .. }) && seg_top >= top_offset {
                let render_y = viewport.y + (seg_top - top_offset);
                let available_height = viewport.bottom().saturating_sub(render_y);
                if available_height > 2 {
                    // Leave 2 rows for border top/bottom, render image inside
                    result.push((
                        i,
                        Rect {
                            x: viewport.x + 1,
                            y: render_y + 1, // skip top border
                            width: viewport.width.saturating_sub(2),
                            height: seg_height.saturating_sub(3).min(available_height - 2),
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
}

impl<'a> ConversationWidget<'a> {
    pub fn new(segments: &'a [Segment], theme: &'a dyn Theme) -> Self {
        Self {
            segments,
            theme,
            mode: SegmentRenderMode::Full,
        }
    }

    pub fn with_mode(mut self, mode: SegmentRenderMode) -> Self {
        self.mode = mode;
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
        state.ensure_heights(self.segments, area.width, self.theme);

        let viewport_height = area.height;
        let total_height = state.total_height();

        if state.user_scrolled && total_height > state.last_total_height {
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

        // Walk segments to find which ones are visible.
        // Segments partially above the viewport are rendered into a temp buffer
        // and the visible portion is copied into the main buffer (proper clipping).
        let mut y_cursor: u16 = 0;
        for (i, segment) in self.segments.iter().enumerate() {
            let seg_height = state.heights[i];
            let seg_top = y_cursor;
            let seg_bottom = y_cursor + seg_height;
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
                let render_y = area.y + (seg_top - top_offset);
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
                segment.render(seg_area, buf, self.theme, self.mode);
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
                segment.render(temp_area, &mut temp_buf, self.theme, self.mode);

                // Copy the visible portion from temp_buf to main buf
                for row in 0..visible_rows {
                    let src_y = clip_rows + row;
                    let dst_y = area.y + row;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

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
        state.ensure_heights(&segments, 80, &Alpharius);
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
        state.ensure_heights(&segments, 40, &Alpharius);
        state.heights[0] = 7;
        state.cached_count = segments.len();
        state.cached_width = 40;
        state.user_scrolled = true;

        state.ensure_heights_with_scroll_state(&segments, 40, &Alpharius, true);
        assert_eq!(
            state.heights[0], 7,
            "detached viewport should preserve cached tail height instead of remeasuring it"
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
}
