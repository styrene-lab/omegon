//! Conversation state — manages the segment list and push/mutation methods.
//!
//! This module holds the data model. Rendering is handled by
//! `conv_widget::ConversationWidget`.

use super::image::ImageCache;
use super::segments::Segment;
use super::conv_widget::ConvState;

/// Conversation view state — segment list + scroll.
pub struct ConversationView {
    segments: Vec<Segment>,
    /// Whether we're currently receiving streaming text.
    streaming: bool,
    /// Scroll + height cache state — shared with the widget.
    pub conv_state: ConvState,
    /// Image render state — StatefulProtocol per segment index.
    pub image_cache: ImageCache,
}

impl ConversationView {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            streaming: false,
            conv_state: ConvState::new(),
            image_cache: ImageCache::default(),
        }
    }

    /// Access segments for rendering.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Split borrow — immutable segments + mutable state.
    /// Needed because ConversationWidget borrows segments immutably
    /// while render_stateful_widget needs mutable state.
    pub fn segments_and_state(&mut self) -> (&[Segment], &mut ConvState) {
        (&self.segments, &mut self.conv_state)
    }

    // ─── Push methods ───────────────────────────────────────────

    pub fn push_user(&mut self, text: &str) {
        // Turn separator before user messages (except first)
        if !self.segments.is_empty() {
            self.segments.push(Segment::TurnSeparator);
        }
        self.segments.push(Segment::UserPrompt { text: text.to_string() });
        self.conv_state.invalidate();
        self.conv_state.force_scroll_to_bottom();
    }

    pub fn push_system(&mut self, text: &str) {
        self.segments.push(Segment::SystemNotification { text: text.to_string() });
        self.conv_state.invalidate();
        self.conv_state.force_scroll_to_bottom();
    }

    pub fn push_image(&mut self, path: std::path::PathBuf, alt: &str) {
        self.segments.push(Segment::Image {
            path,
            alt: alt.to_string(),
        });
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_lifecycle(&mut self, icon: &str, text: &str) {
        self.segments.push(Segment::LifecycleEvent {
            icon: icon.to_string(),
            text: text.to_string(),
        });
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn append_streaming(&mut self, delta: &str) {
        if !self.streaming {
            self.segments.push(Segment::AssistantText {
                text: String::new(),
                thinking: String::new(),
                complete: false,
            });
            self.streaming = true;
        }

        if let Some(Segment::AssistantText { text, .. }) = self.segments.last_mut() {
            text.push_str(delta);
        }
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn append_thinking(&mut self, delta: &str) {
        if !self.streaming {
            self.segments.push(Segment::AssistantText {
                text: String::new(),
                thinking: String::new(),
                complete: false,
            });
            self.streaming = true;
        }

        if let Some(Segment::AssistantText { thinking, .. }) = self.segments.last_mut() {
            thinking.push_str(delta);
        }
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_tool_start(&mut self, id: &str, name: &str, args_summary: Option<&str>, detail_args: Option<&str>) {
        self.segments.push(Segment::ToolCard {
            id: id.to_string(),
            name: name.to_string(),
            args_summary: args_summary.map(|s| s.to_string()),
            detail_args: detail_args.map(|s| s.to_string()),
            result_summary: None,
            detail_result: None,
            is_error: false,
            complete: false,
            expanded: false,
        });
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_tool_end(&mut self, id: &str, is_error: bool, result_text: Option<&str>) {
        for seg in self.segments.iter_mut().rev() {
            if let Segment::ToolCard {
                id: tool_id,
                complete: c,
                is_error: e,
                result_summary: r,
                detail_result: dr,
                ..
            } = seg
                && tool_id == id && !*c
            {
                *c = true;
                *e = is_error;
                *r = result_text.and_then(|text| {
                    let line = text.lines()
                        .find(|l| {
                            let t = l.trim();
                            !t.is_empty() && !t.starts_with("```") && !t.starts_with("---")
                        })
                        .unwrap_or("").trim();
                    if line.is_empty() { None }
                    else if line.len() > 100 { Some(format!("{}…", &line[..99])) }
                    else { Some(line.to_string()) }
                });
                // Store the full result — truncation happens at render time
                // based on the card's expanded/collapsed state.
                *dr = result_text.map(|text| text.to_string());
                break;
            }
        }
        self.conv_state.invalidate();
    }

    pub fn finalize_message(&mut self) {
        if let Some(Segment::AssistantText { complete, .. }) = self.segments.last_mut() {
            *complete = true;
        }
        self.streaming = false;
        self.conv_state.invalidate();
        self.conv_state.user_scrolled = false;
        self.conv_state.scroll_offset = 0;
    }

    // ─── Scroll ─────────────────────────────────────────────────

    pub fn scroll_up(&mut self, amount: u16) {
        self.conv_state.scroll_up(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.conv_state.scroll_down(amount);
    }

    /// Toggle expansion state of a tool card at the given segment index.
    pub fn toggle_expand(&mut self, segment_idx: usize) {
        if let Some(Segment::ToolCard { expanded, .. }) = self.segments.get_mut(segment_idx) {
            *expanded = !*expanded;
            self.conv_state.invalidate();
        }
    }

    /// Find the nearest tool card segment visible in the viewport.
    /// Uses cached heights from the last render (which used the real width).
    pub fn focused_tool_card(&self) -> Option<usize> {
        let offset = self.conv_state.scroll_offset;
        let heights = &self.conv_state.heights;
        if heights.len() != self.segments.len() {
            // Heights not computed yet — fall back to last tool card
            return self.segments.iter().rposition(|s| matches!(s, Segment::ToolCard { .. }));
        }

        let total: u16 = heights.iter().sum();
        // Convert bottom-based offset to top-based
        let viewport_top = total.saturating_sub(offset);

        let mut y: u16 = 0;
        for (i, seg) in self.segments.iter().enumerate() {
            y += heights[i];

            if matches!(seg, Segment::ToolCard { .. })
                && y > viewport_top.saturating_sub(total / 2)
            {
                return Some(i);
            }
        }
        // Fall back to last tool card
        self.segments.iter().rposition(|s| matches!(s, Segment::ToolCard { .. }))
    }

    /// Clear all segments (for /clear command).
    pub fn clear(&mut self) {
        self.segments.clear();
        self.conv_state = ConvState::new();
        self.streaming = false;
        self.image_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;
    use ratatui::prelude::*;

    #[test]
    fn user_message_creates_segments() {
        let mut cv = ConversationView::new();
        cv.push_user("hello");
        // First user message: just the prompt (no separator)
        assert_eq!(cv.segments.len(), 1);
        assert!(matches!(&cv.segments[0], Segment::UserPrompt { text } if text == "hello"));
    }

    #[test]
    fn second_user_message_adds_separator() {
        let mut cv = ConversationView::new();
        cv.push_user("first");
        cv.push_user("second");
        // separator + prompt
        assert_eq!(cv.segments.len(), 3);
        assert!(matches!(&cv.segments[1], Segment::TurnSeparator));
    }

    #[test]
    fn streaming_creates_assistant_segment() {
        let mut cv = ConversationView::new();
        cv.append_streaming("Hello ");
        cv.append_streaming("world");
        assert_eq!(cv.segments.len(), 1);
        if let Segment::AssistantText { text, complete, .. } = &cv.segments[0] {
            assert_eq!(text, "Hello world");
            assert!(!complete);
        } else {
            panic!("expected AssistantText");
        }
    }

    #[test]
    fn finalize_marks_complete() {
        let mut cv = ConversationView::new();
        cv.append_streaming("Done");
        cv.finalize_message();
        if let Segment::AssistantText { complete, .. } = &cv.segments[0] {
            assert!(complete);
        }
    }

    #[test]
    fn tool_lifecycle() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("tc1", "read", Some("src/main.rs"), Some("src/main.rs"));
        cv.push_tool_end("tc1", false, Some("fn main() {}\n// 245 lines"));
        if let Segment::ToolCard { complete, is_error, detail_result, .. } = &cv.segments[0] {
            assert!(complete);
            assert!(!is_error);
            assert!(detail_result.is_some());
        }
    }

    #[test]
    fn scroll_up_sets_user_scrolled() {
        let mut cv = ConversationView::new();
        cv.scroll_up(3);
        assert!(cv.conv_state.user_scrolled);
    }

    #[test]
    fn push_user_forces_scroll_to_bottom() {
        let mut cv = ConversationView::new();
        cv.scroll_up(10);
        cv.push_user("new prompt");
        assert_eq!(cv.conv_state.scroll_offset, 0);
        assert!(!cv.conv_state.user_scrolled);
    }

    #[test]
    fn finalize_resets_scroll() {
        let mut cv = ConversationView::new();
        cv.append_streaming("text");
        cv.scroll_up(10);
        cv.finalize_message();
        assert!(!cv.conv_state.user_scrolled);
        assert_eq!(cv.conv_state.scroll_offset, 0);
    }

    #[test]
    fn segments_render_via_widget() {
        let mut cv = ConversationView::new();
        cv.push_user("hello");
        cv.append_streaming("response");
        cv.finalize_message();
        cv.push_tool_start("t1", "bash", Some("echo hi"), Some("echo hi"));
        cv.push_tool_end("t1", false, Some("hi"));

        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let (segments, state) = cv.segments_and_state();
        let widget = super::super::conv_widget::ConversationWidget::new(segments, &Alpharius);
        widget.render(area, &mut buf, state);

        // Verify segments were rendered
        let mut found_hello = false;
        let mut found_bash = false;
        for y in 0..40 {
            let mut row = String::new();
            for x in 0..80 { row.push_str(buf[(x, y)].symbol()); }
            if row.contains("hello") { found_hello = true; }
            if row.contains("echo") { found_bash = true; } // "echo" from args renders in card
        }
        assert!(found_hello, "should render user prompt");
        assert!(found_bash, "should render tool card");
    }

    #[test]
    fn toggle_expand_changes_state() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("t1", "read", Some("file.rs"), Some("file.rs"));
        cv.push_tool_end("t1", false, Some("line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10\nline11\nline12\nline13\nline14\nline15"));

        // Default is collapsed
        if let Segment::ToolCard { expanded, .. } = &cv.segments[0] {
            assert!(!expanded, "should start collapsed");
        }

        // Toggle to expanded
        cv.toggle_expand(0);
        if let Segment::ToolCard { expanded, .. } = &cv.segments[0] {
            assert!(expanded, "should be expanded after toggle");
        }

        // Toggle back to collapsed
        cv.toggle_expand(0);
        if let Segment::ToolCard { expanded, .. } = &cv.segments[0] {
            assert!(!expanded, "should be collapsed after second toggle");
        }
    }

    #[test]
    fn toggle_expand_on_non_tool_is_noop() {
        let mut cv = ConversationView::new();
        cv.push_user("hello");
        cv.toggle_expand(0); // UserPrompt — should not panic
    }

    #[test]
    fn expanded_card_has_more_height() {
        let mut cv = ConversationView::new();
        let long_result = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        cv.push_tool_start("t1", "read", Some("file.rs"), Some("file.rs"));
        cv.push_tool_end("t1", false, Some(&long_result));

        let t = Alpharius;
        let collapsed_h = cv.segments[0].height(80, &t);
        cv.toggle_expand(0);
        let expanded_h = cv.segments[0].height(80, &t);
        assert!(expanded_h > collapsed_h,
            "expanded ({expanded_h}) should be taller than collapsed ({collapsed_h})");
    }

    #[test]
    fn focused_tool_card_finds_card() {
        let mut cv = ConversationView::new();
        cv.push_user("hello");
        cv.push_tool_start("t1", "bash", Some("ls"), Some("ls"));
        cv.push_tool_end("t1", false, Some("file.txt"));
        cv.push_user("another");
        cv.push_tool_start("t2", "read", Some("a.rs"), Some("a.rs"));
        cv.push_tool_end("t2", false, Some("fn main(){}"));

        let result = cv.focused_tool_card();
        assert!(result.is_some(), "should find a tool card");
        let idx = result.unwrap();
        assert!(matches!(&cv.segments[idx], Segment::ToolCard { .. }));
    }

    #[test]
    fn full_result_stored_not_truncated() {
        let mut cv = ConversationView::new();
        let long_result = (0..50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        cv.push_tool_start("t1", "read", Some("file.rs"), Some("file.rs"));
        cv.push_tool_end("t1", false, Some(&long_result));

        if let Segment::ToolCard { detail_result, .. } = &cv.segments[0] {
            let dr = detail_result.as_ref().unwrap();
            assert_eq!(dr.lines().count(), 50, "full result should be stored, not truncated");
        }
    }
}
