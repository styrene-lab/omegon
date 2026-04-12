//! Conversation state — manages the segment list and push/mutation methods.
//!
//! This module holds the data model. Rendering is handled by
//! `conv_widget::ConversationWidget`.

use super::conv_widget::ConvState;
use super::image::ImageCache;
use super::segments::{Segment, SegmentContent, SegmentExportMode, SegmentMeta, TokenUsage};

/// Tab variant — conversation or extension widget
#[derive(Debug, Clone)]
pub enum Tab {
    /// Main conversation tab (index 0, always present)
    Conversation,
    /// Extension widget tab
    Extension { widget_id: String, label: String },
}

impl Tab {
    pub fn label(&self) -> &str {
        match self {
            Tab::Conversation => "Conversation",
            Tab::Extension { label, .. } => label,
        }
    }
}

/// Tab state — manages active tab and list of tabs
#[derive(Debug, Clone)]
pub struct TabState {
    pub tabs: Vec<Tab>,
    pub active_tab: usize, // always valid index into tabs
}

impl TabState {
    pub fn new() -> Self {
        Self {
            tabs: vec![Tab::Conversation],
            active_tab: 0,
        }
    }

    /// Add an extension widget as a new tab
    pub fn add_extension_tab(&mut self, widget_id: String, label: String) {
        self.tabs.push(Tab::Extension { widget_id, label });
    }

    /// Switch to next tab (wrap around)
    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
    }

    /// Switch to previous tab (wrap around)
    pub fn prev_tab(&mut self) {
        self.active_tab = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
    }

    /// Get the active tab
    pub fn active(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// Check if conversation tab is active
    pub fn is_conversation_active(&self) -> bool {
        matches!(self.active(), Tab::Conversation)
    }
}

impl Default for TabState {
    fn default() -> Self {
        Self::new()
    }
}

/// Conversation view state — segment list + scroll.
pub struct ConversationView {
    segments: Vec<Segment>,
    /// Whether we're currently receiving streaming text.
    streaming: bool,
    /// Scroll + height cache state — shared with the widget.
    pub conv_state: ConvState,
    /// Image render state — StatefulProtocol per segment index.
    pub image_cache: ImageCache,
    /// Pinned segment index — when set, this segment stays expanded and visible
    /// at the bottom of the viewport until explicitly unpinned (Ctrl+O again or Esc).
    pub pinned_segment: Option<usize>,
    /// Explicitly selected segment index from operator interaction.
    pub selected_segment: Option<usize>,
    /// Tab state — manages conversation tab and extension widget tabs
    pub tabs: TabState,
}

fn attachment_placeholder(path: &std::path::Path, idx: usize) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    let kind = match ext.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif") => "image",
        Some("pdf") => "pdf",
        _ => "attachment",
    };
    format!("[{kind}{idx}]")
}

fn attachment_alt_text(path: &std::path::Path, idx: usize) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{} {name}", attachment_placeholder(path, idx)))
        .unwrap_or_else(|| attachment_placeholder(path, idx))
}

fn non_image_attachment_summary(attachments: &[std::path::PathBuf]) -> Option<String> {
    let placeholders = attachments
        .iter()
        .enumerate()
        .filter(|(_, path)| !super::image::is_image_path(&path.to_string_lossy()))
        .map(|(idx, path)| attachment_placeholder(path, idx))
        .collect::<Vec<_>>();
    if placeholders.is_empty() {
        None
    } else {
        Some(placeholders.join(" "))
    }
}

impl ConversationView {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            streaming: false,
            conv_state: ConvState::new(),
            image_cache: ImageCache::default(),
            pinned_segment: None,
            selected_segment: None,
            tabs: TabState::new(),
        }
    }

    /// Access segments for rendering.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Whether we're currently receiving streaming text.
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Split borrow — immutable segments + mutable state.
    /// Needed because ConversationWidget borrows segments immutably
    /// while render_stateful_widget needs mutable state.
    pub fn segments_and_state(&mut self) -> (&[Segment], &mut ConvState) {
        (&self.segments, &mut self.conv_state)
    }

    // ─── Push methods ───────────────────────────────────────────

    pub fn push_user(&mut self, text: &str) {
        self.push_user_with_attachments(text, &[]);
    }

    pub fn push_user_with_attachments(&mut self, text: &str, attachments: &[std::path::PathBuf]) {
        if !self.segments.is_empty() {
            self.segments.push(Segment::separator());
        }

        let non_image_summary = non_image_attachment_summary(attachments);
        let rendered = match (text.is_empty(), non_image_summary) {
            (false, Some(summary)) => format!("{text}\n{summary}"),
            (false, None) => text.to_string(),
            (true, Some(summary)) => summary,
            (true, None) => String::new(),
        };

        if !rendered.is_empty() || attachments.is_empty() {
            self.segments.push(Segment::user_prompt(rendered));
        }

        for (idx, path) in attachments.iter().enumerate() {
            if super::image::is_image_path(&path.to_string_lossy()) {
                self.segments
                    .push(Segment::image(path.clone(), attachment_alt_text(path, idx)));
            }
        }

        self.conv_state.invalidate();
        self.conv_state.force_scroll_to_bottom();
    }

    pub fn push_system(&mut self, text: &str) {
        self.segments.push(Segment::system(text));
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_image(&mut self, path: std::path::PathBuf, alt: &str) {
        self.segments.push(Segment::image(path, alt));
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_lifecycle(&mut self, icon: &str, text: &str) {
        self.segments.push(Segment::lifecycle(icon, text));
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn append_streaming(&mut self, delta: &str) {
        // Push a new AssistantText segment if we aren't already writing into one.
        // This handles both the initial case (!streaming) and the case where
        // tool cards were interleaved — the last segment may be a ToolCard even
        // though streaming=true, which previously caused the text to be silently
        // dropped.
        let needs_new_seg = !matches!(
            self.segments.last(),
            Some(Segment {
                content: SegmentContent::AssistantText { .. },
                ..
            })
        );
        if needs_new_seg {
            self.segments.push(Segment::assistant_text());
        }
        self.streaming = true;

        if let Some(seg) = self.segments.last_mut() {
            if let SegmentContent::AssistantText { text, .. } = &mut seg.content {
                text.push_str(delta);
            }
        }
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn append_thinking(&mut self, delta: &str) {
        // Same guard as append_streaming — don't append into a ToolCard.
        let needs_new_seg = !matches!(
            self.segments.last(),
            Some(Segment {
                content: SegmentContent::AssistantText { .. },
                ..
            })
        );
        if needs_new_seg {
            self.segments.push(Segment::assistant_text());
        }
        self.streaming = true;

        if let Some(seg) = self.segments.last_mut() {
            if let SegmentContent::AssistantText { thinking, .. } = &mut seg.content {
                thinking.push_str(delta);
            }
        }
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_tool_start(
        &mut self,
        id: &str,
        name: &str,
        args_summary: Option<&str>,
        detail_args: Option<&str>,
    ) {
        let mut seg = Segment::tool_card(id, name);
        if let SegmentContent::ToolCard {
            args_summary: ref mut a,
            detail_args: ref mut d,
            ..
        } = seg.content
        {
            *a = args_summary.map(|s| s.to_string());
            *d = detail_args.map(|s| s.to_string());
        }
        self.segments.push(seg);
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    /// Stash the latest streaming partial onto the open tool card with
    /// the given id. Called from the `AgentEvent::ToolUpdate` handler;
    /// runners (bash, local_inference, mcp) push these as work happens.
    /// Silently no-op if the card is already complete or not found —
    /// late or stale updates shouldn't crash anything.
    pub fn push_tool_update(&mut self, id: &str, partial: omegon_traits::PartialToolResult) {
        for seg in self.segments.iter_mut().rev() {
            if let SegmentContent::ToolCard {
                id: tool_id,
                complete: c,
                live_partial: lp,
                ..
            } = &mut seg.content
                && tool_id == id
                && !*c
            {
                *lp = Some(partial);
                self.conv_state.invalidate();
                return;
            }
        }
    }

    pub fn push_tool_end(&mut self, id: &str, is_error: bool, result_text: Option<&str>) {
        for seg in self.segments.iter_mut().rev() {
            if let SegmentContent::ToolCard {
                id: tool_id,
                complete: c,
                is_error: e,
                result_summary: r,
                detail_result: dr,
                live_partial: lp,
                ..
            } = &mut seg.content
                && tool_id == id
                && !*c
            {
                *c = true;
                *e = is_error;
                // The completed-result render path takes over now; the
                // last in-flight partial is stale.
                *lp = None;
                *r = result_text.and_then(|text| {
                    let line = text
                        .lines()
                        .find(|l| {
                            let t = l.trim();
                            !t.is_empty() && !t.starts_with("```") && !t.starts_with("---")
                        })
                        .unwrap_or("")
                        .trim();
                    if line.is_empty() {
                        None
                    } else if line.chars().count() > 100 {
                        Some(crate::util::truncate(line, 99))
                    } else {
                        Some(line.to_string())
                    }
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
        if let Some(seg) = self.segments.last_mut() {
            if let SegmentContent::AssistantText { complete, .. } = &mut seg.content {
                *complete = true;
            }
        }
        self.streaming = false;
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn abort_streaming(&mut self) {
        self.streaming = false;
        self.conv_state.invalidate();
    }

    /// Stamp metadata on the most recent segment (call after segment creation
    /// when model/provider info is available from the harness).
    pub fn stamp_meta(&mut self, meta: SegmentMeta) {
        if let Some(seg) = self.segments.last_mut() {
            seg.meta = meta;
        }
    }

    /// Walk back through segments belonging to a given turn and stamp
    /// the provider-reported actual token counts onto each one. Called
    /// from the `AgentEvent::TurnEnd` handler in `tui/mod.rs` once the
    /// real numbers arrive. Segments emitted earlier in the turn (tool
    /// cards, assistant text, etc.) all share the same turn id via
    /// `current_meta()` and pick up the stamp here so the title-bar
    /// token annotation appears across the whole turn at once.
    ///
    /// Walks back from the tail rather than the head because turn-end
    /// stamps usually only need to touch a handful of recent segments.
    /// Stops at the first segment whose `turn` is older than the
    /// target — segments are ordered chronologically so anything older
    /// than the target turn won't have new stamps to apply.
    pub fn stamp_turn_tokens(&mut self, turn: u32, tokens: TokenUsage) {
        for seg in self.segments.iter_mut().rev() {
            match seg.meta.turn {
                Some(t) if t == turn => {
                    seg.meta.actual_tokens = Some(tokens);
                }
                Some(t) if t < turn => {
                    // Older turn — and since segments are chronological,
                    // anything before this is also older. Stop walking.
                    break;
                }
                _ => {
                    // No turn id (rare — pre-turn-tracking segments) or
                    // a future turn (shouldn't happen). Skip and keep
                    // walking back.
                }
            }
        }
        self.conv_state.invalidate();
    }

    // ─── Scroll ─────────────────────────────────────────────────

    pub fn scroll_up(&mut self, amount: u16) {
        self.conv_state.scroll_up(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.conv_state.scroll_down(amount);
    }

    /// Explicitly return to the live tail of the conversation.
    pub fn snap_to_bottom(&mut self) {
        self.conv_state.force_scroll_to_bottom();
    }

    /// Toggle expansion state of a tool card at the given segment index.
    pub fn toggle_expand(&mut self, segment_idx: usize) {
        if let Some(seg) = self.segments.get_mut(segment_idx) {
            if let SegmentContent::ToolCard { expanded, .. } = &mut seg.content {
                *expanded = !*expanded;
                self.conv_state.invalidate();
            }
        }
    }

    /// Toggle pin on the selected tool card if present, otherwise the nearest
    /// visible tool card. When pinned, the segment is expanded and stays visible
    /// at the bottom of the conversation viewport. Pressing Ctrl+O again (or Esc)
    /// unpins and collapses.
    pub fn toggle_pin(&mut self) {
        if let Some(pinned) = self.pinned_segment.take() {
            // Unpin: collapse the segment
            self.toggle_expand(pinned);
            return;
        }

        let target = self
            .selected_segment
            .filter(|&idx| {
                matches!(
                    self.segments.get(idx).map(|s| &s.content),
                    Some(SegmentContent::ToolCard { .. })
                )
            })
            .or_else(|| self.focused_tool_card());

        if let Some(idx) = target {
            // Pin: expand and lock focus
            if let Some(seg) = self.segments.get_mut(idx) {
                if let SegmentContent::ToolCard { expanded, .. } = &mut seg.content {
                    if !*expanded {
                        *expanded = true;
                        self.conv_state.invalidate();
                    }
                }
            }
            self.selected_segment = Some(idx);
            self.pinned_segment = Some(idx);
        }
    }

    /// Unpin the currently pinned segment (if any), collapsing it.
    pub fn unpin(&mut self) {
        if let Some(pinned) = self.pinned_segment.take() {
            self.toggle_expand(pinned);
        }
    }

    /// Find the nearest tool card segment visible in the viewport.
    /// Uses cached heights from the last render (which used the real width).
    pub fn focused_tool_card(&self) -> Option<usize> {
        let offset = self.conv_state.scroll_offset;
        let heights = &self.conv_state.heights;
        if heights.len() != self.segments.len() {
            return self
                .segments
                .iter()
                .rposition(|s| matches!(s.content, SegmentContent::ToolCard { .. }));
        }

        let total: u16 = heights.iter().sum();
        let viewport_top = total.saturating_sub(offset);

        let mut y: u16 = 0;
        for (i, seg) in self.segments.iter().enumerate() {
            y += heights[i];

            if matches!(seg.content, SegmentContent::ToolCard { .. })
                && y > viewport_top.saturating_sub(total / 2)
            {
                return Some(i);
            }
        }
        self.segments
            .iter()
            .rposition(|s| matches!(s.content, SegmentContent::ToolCard { .. }))
    }

    pub fn select_segment(&mut self, idx: usize) {
        if idx < self.segments.len() {
            self.selected_segment = Some(idx);
        }
    }

    pub fn clear_selected_segment(&mut self) {
        self.selected_segment = None;
    }

    pub fn selected_or_focused_segment(&self) -> Option<usize> {
        self.selected_segment
            .or_else(|| self.last_selectable_segment())
    }

    pub fn timeline_focused_segment(&self) -> Option<usize> {
        self.selected_or_focused_segment()
    }

    pub fn timeline_expanded_segment(&self) -> Option<usize> {
        self.pinned_segment
    }

    pub fn set_timeline_expanded_segment(&mut self, idx: Option<usize>) {
        self.pinned_segment = idx;
    }

    pub fn toggle_timeline_expanded_segment(&mut self, idx: usize) -> Option<usize> {
        if self.pinned_segment == Some(idx) {
            self.pinned_segment = None;
        } else if idx < self.segments.len() {
            self.pinned_segment = Some(idx);
            self.selected_segment = Some(idx);
        }
        self.pinned_segment
    }

    pub fn last_selectable_segment(&self) -> Option<usize> {
        self.segments
            .iter()
            .enumerate()
            .rev()
            .find(|(_, seg)| !matches!(seg.content, SegmentContent::TurnSeparator))
            .map(|(idx, _)| idx)
    }

    pub fn first_selectable_segment(&self) -> Option<usize> {
        self.segments
            .iter()
            .enumerate()
            .find(|(_, seg)| !matches!(seg.content, SegmentContent::TurnSeparator))
            .map(|(idx, _)| idx)
    }

    pub fn selected_segment_text(&self) -> Option<String> {
        self.selected_segment_text_with_mode(SegmentExportMode::Raw)
    }

    pub fn selected_segment_text_with_mode(&self, mode: SegmentExportMode) -> Option<String> {
        self.selected_or_focused_segment()
            .and_then(|idx| self.segments.get(idx))
            .map(|segment| segment.export_text(mode))
    }

    pub fn move_selected_segment_prev(&mut self) -> Option<usize> {
        let start = self
            .selected_or_focused_segment()
            .or_else(|| self.last_selectable_segment())?;
        let idx = self.segments[..start]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, seg)| !matches!(seg.content, SegmentContent::TurnSeparator))
            .map(|(idx, _)| idx)
            .unwrap_or(start);
        self.selected_segment = Some(idx);
        Some(idx)
    }

    pub fn move_selected_segment_next(&mut self) -> Option<usize> {
        let start = self
            .selected_or_focused_segment()
            .or_else(|| self.first_selectable_segment())?;
        let idx = self
            .segments
            .iter()
            .enumerate()
            .skip(start.saturating_add(1))
            .find(|(_, seg)| !matches!(seg.content, SegmentContent::TurnSeparator))
            .map(|(idx, _)| idx)
            .unwrap_or(start);
        self.selected_segment = Some(idx);
        Some(idx)
    }

    pub fn segment_at(&self, viewport: ratatui::prelude::Rect, row: u16) -> Option<usize> {
        let heights = &self.conv_state.heights;
        if heights.len() != self.segments.len() || row < viewport.y || row >= viewport.bottom() {
            return None;
        }

        let viewport_height = viewport.height;
        let total_height: u16 = heights.iter().copied().sum();
        let top_offset = if total_height <= viewport_height {
            0
        } else {
            total_height
                - viewport_height
                - self
                    .conv_state
                    .scroll_offset
                    .min(total_height.saturating_sub(viewport_height))
        };

        let target_y = top_offset + (row - viewport.y);
        let mut y_cursor: u16 = 0;
        for (idx, seg_height) in heights.iter().copied().enumerate() {
            let seg_top = y_cursor;
            let seg_bottom = y_cursor + seg_height;
            if target_y >= seg_top && target_y < seg_bottom {
                return Some(idx);
            }
            y_cursor = seg_bottom;
        }
        None
    }

    /// Clear all segments (for /clear command).
    pub fn clear(&mut self) {
        self.segments.clear();
        self.conv_state = ConvState::new();
        self.streaming = false;
        self.image_cache.clear();
        self.pinned_segment = None;
        self.selected_segment = None;
        self.tabs = TabState::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;
    use ratatui::prelude::*;

    #[test]
    fn stamp_turn_tokens_walks_back_and_stamps_matching_segments() {
        // Build a conversation with three segments across two turns:
        //   - turn 1: tool card
        //   - turn 2: tool card
        //   - turn 2: assistant text
        // Then stamp turn 2 with token usage and confirm only the
        // turn-2 segments get the actual_tokens field set.
        let mut cv = ConversationView::new();

        cv.push_tool_start("t1", "bash", Some("ls"), Some("ls"));
        cv.stamp_meta(SegmentMeta {
            turn: Some(1),
            ..SegmentMeta::default()
        });

        cv.push_tool_start("t2", "read", Some("file.rs"), Some("file.rs"));
        cv.stamp_meta(SegmentMeta {
            turn: Some(2),
            ..SegmentMeta::default()
        });

        cv.append_streaming("hello");
        cv.stamp_meta(SegmentMeta {
            turn: Some(2),
            ..SegmentMeta::default()
        });

        cv.stamp_turn_tokens(
            2,
            TokenUsage {
                input: 500,
                output: 100,
            },
        );

        // Turn 1 segment: untouched
        assert!(
            cv.segments[0].meta.actual_tokens.is_none(),
            "turn 1 segment must NOT be stamped with turn 2's tokens"
        );
        // Turn 2 segments: stamped
        assert_eq!(
            cv.segments[1].meta.actual_tokens,
            Some(TokenUsage {
                input: 500,
                output: 100
            })
        );
        assert_eq!(
            cv.segments[2].meta.actual_tokens,
            Some(TokenUsage {
                input: 500,
                output: 100
            })
        );
    }

    #[test]
    fn user_message_creates_segments() {
        let mut cv = ConversationView::new();
        cv.push_user("hello");
        // First user message: just the prompt (no separator)
        assert_eq!(cv.segments.len(), 1);
        assert!(
            matches!(&cv.segments[0], Segment { content: SegmentContent::UserPrompt { text }, .. } if text == "hello")
        );
    }

    #[test]
    fn second_user_message_adds_separator() {
        let mut cv = ConversationView::new();
        cv.push_user("first");
        cv.push_user("second");
        // separator + prompt
        assert_eq!(cv.segments.len(), 3);
        assert!(matches!(
            &cv.segments[1],
            Segment {
                content: SegmentContent::TurnSeparator,
                ..
            }
        ));
    }

    #[test]
    fn image_attachments_render_as_structured_segments() {
        let mut cv = ConversationView::new();
        cv.push_user_with_attachments(
            "describe this",
            &[
                std::path::PathBuf::from("/tmp/paste.png"),
                std::path::PathBuf::from("/tmp/spec.pdf"),
                std::path::PathBuf::from("/tmp/blob.bin"),
            ],
        );
        assert_eq!(cv.segments.len(), 2);
        assert!(matches!(
            &cv.segments[0],
            Segment {
                content: SegmentContent::UserPrompt { text },
                ..
            } if text.contains("describe this")
                && text.contains("[pdf1]")
                && text.contains("[attachment2]")
                && !text.contains("[image0]")
        ));
        assert!(matches!(
            &cv.segments[1],
            Segment {
                content: SegmentContent::Image { path, alt },
                ..
            } if path == &std::path::PathBuf::from("/tmp/paste.png")
                && alt.contains("[image0]")
        ));
    }

    #[test]
    fn image_only_attachments_render_without_placeholder_prompt() {
        let mut cv = ConversationView::new();
        cv.push_user_with_attachments(
            "",
            &[
                std::path::PathBuf::from("/tmp/paste.png"),
                std::path::PathBuf::from("/tmp/other.jpg"),
            ],
        );
        assert_eq!(cv.segments.len(), 2);
        assert!(matches!(
            &cv.segments[0].content,
            SegmentContent::Image { .. }
        ));
        assert!(matches!(
            &cv.segments[1].content,
            SegmentContent::Image { .. }
        ));
    }

    #[test]
    fn streaming_creates_assistant_segment() {
        let mut cv = ConversationView::new();
        cv.append_streaming("Hello ");
        cv.append_streaming("world");
        assert_eq!(cv.segments.len(), 1);
        if let SegmentContent::AssistantText { text, complete, .. } = &cv.segments[0].content {
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
        if let SegmentContent::AssistantText { complete, .. } = &cv.segments[0].content {
            assert!(complete);
        }
    }

    #[test]
    fn tool_lifecycle() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("tc1", "read", Some("src/main.rs"), Some("src/main.rs"));
        cv.push_tool_end("tc1", false, Some("fn main() {}\n// 245 lines"));
        if let SegmentContent::ToolCard {
            complete,
            is_error,
            detail_result,
            ..
        } = &cv.segments[0].content
        {
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
    fn finalize_preserves_manual_scroll() {
        let mut cv = ConversationView::new();
        cv.append_streaming("text");
        cv.scroll_up(10);
        cv.finalize_message();
        assert!(
            cv.conv_state.user_scrolled,
            "manual scroll should remain pinned after finalize"
        );
        assert_eq!(cv.conv_state.scroll_offset, 10);
    }

    #[test]
    fn finalize_preserves_manual_scroll_when_streaming_completes() {
        let mut cv = ConversationView::new();
        cv.append_streaming("text");
        cv.scroll_up(10);
        cv.finalize_message();
        assert!(
            cv.conv_state.user_scrolled,
            "manual scroll should remain pinned after finalize"
        );
        assert_eq!(cv.conv_state.scroll_offset, 10);
    }

    #[test]
    fn snap_to_bottom_resets_scroll() {
        let mut cv = ConversationView::new();
        cv.scroll_up(10);
        cv.snap_to_bottom();
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
            for x in 0..80 {
                row.push_str(buf[(x, y)].symbol());
            }
            if row.contains("hello") {
                found_hello = true;
            }
            if row.contains("echo") {
                found_bash = true;
            } // "echo" from args renders in card
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
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[0].content {
            assert!(!expanded, "should start collapsed");
        }

        // Toggle to expanded
        cv.toggle_expand(0);
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[0].content {
            assert!(expanded, "should be expanded after toggle");
        }

        // Toggle back to collapsed
        cv.toggle_expand(0);
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[0].content {
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
        let long_result = (0..30)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        cv.push_tool_start("t1", "read", Some("file.rs"), Some("file.rs"));
        cv.push_tool_end("t1", false, Some(&long_result));

        let t = Alpharius;
        let collapsed_h = cv.segments[0].height(80, &t);
        cv.toggle_expand(0);
        let expanded_h = cv.segments[0].height(80, &t);
        assert!(
            expanded_h > collapsed_h,
            "expanded ({expanded_h}) should be taller than collapsed ({collapsed_h})"
        );
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
        assert!(matches!(
            &cv.segments[idx].content,
            SegmentContent::ToolCard { .. }
        ));
    }

    /// Regression test: text emitted after tool cards must appear in a new
    /// AssistantText segment, not be silently dropped.
    ///
    /// Sequence: pre-tool text → tool card → post-tool text → finalize
    /// Expected: 3 segments (AssistantText, ToolCard, AssistantText)
    /// Bug was: post-tool text was lost because append_streaming saw
    /// streaming=true and tried to append into the ToolCard segment,
    /// found no AssistantText match, and discarded the delta.
    #[test]
    fn text_after_tool_cards_is_not_dropped() {
        let mut cv = ConversationView::new();

        // Pre-tool response text
        cv.append_streaming("Let me check that for you.");

        // Tool cards interleaved
        cv.push_tool_start("t1", "bash", Some("ls"), Some("ls"));
        cv.push_tool_end("t1", false, Some("file.txt"));

        // Post-tool response text — this was silently dropped before the fix
        cv.append_streaming("Here is where we sit.");
        cv.finalize_message();

        // Should be: AssistantText, ToolCard, AssistantText
        let segment_types: Vec<&str> = cv
            .segments
            .iter()
            .map(|s| match &s.content {
                SegmentContent::AssistantText { .. } => "AssistantText",
                SegmentContent::ToolCard { .. } => "ToolCard",
                _ => "other",
            })
            .collect();
        assert_eq!(
            cv.segments.len(),
            3,
            "expected 3 segments, got {}: {:?}",
            cv.segments.len(),
            segment_types
        );

        // Third segment must contain the post-tool text
        if let SegmentContent::AssistantText { text, complete, .. } = &cv.segments[2].content {
            assert_eq!(text, "Here is where we sit.", "post-tool text was dropped");
            assert!(complete, "should be finalized");
        } else {
            panic!("segment[2] should be AssistantText");
        }

        // First segment should have the pre-tool text
        if let SegmentContent::AssistantText { text, .. } = &cv.segments[0].content {
            assert_eq!(text, "Let me check that for you.");
        } else {
            panic!("segment[0] should be AssistantText");
        }
    }

    #[test]
    fn text_only_response_still_works() {
        // Sanity: no tools, text-only response is still one segment
        let mut cv = ConversationView::new();
        cv.append_streaming("Hello ");
        cv.append_streaming("world");
        cv.finalize_message();
        assert_eq!(cv.segments.len(), 1);
        if let SegmentContent::AssistantText { text, complete, .. } = &cv.segments[0].content {
            assert_eq!(text, "Hello world");
            assert!(complete);
        }
    }

    #[test]
    fn full_result_stored_not_truncated() {
        let mut cv = ConversationView::new();
        let long_result = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        cv.push_tool_start("t1", "read", Some("file.rs"), Some("file.rs"));
        cv.push_tool_end("t1", false, Some(&long_result));

        if let SegmentContent::ToolCard { detail_result, .. } = &cv.segments[0].content {
            let dr = detail_result.as_ref().unwrap();
            assert_eq!(
                dr.lines().count(),
                50,
                "full result should be stored, not truncated"
            );
        }
    }

    #[test]
    fn toggle_pin_expands_and_pins() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("t1", "bash", Some("ls"), Some("ls"));
        cv.push_tool_end("t1", false, Some("file.txt"));

        assert!(cv.pinned_segment.is_none());

        // Pin should expand the card and record the index
        cv.toggle_pin();
        assert!(cv.pinned_segment.is_some());
        let idx = cv.pinned_segment.unwrap();
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[idx].content {
            assert!(expanded, "pinned card should be expanded");
        }

        // Pin again should unpin and collapse
        cv.toggle_pin();
        assert!(cv.pinned_segment.is_none());
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[idx].content {
            assert!(!expanded, "unpinned card should be collapsed");
        }
    }

    #[test]
    fn unpin_collapses_pinned_segment() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("t1", "bash", Some("ls"), Some("ls"));
        cv.push_tool_end("t1", false, Some("file.txt"));

        cv.toggle_pin();
        assert!(cv.pinned_segment.is_some());

        cv.unpin();
        assert!(cv.pinned_segment.is_none());
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[0].content {
            assert!(!expanded, "should be collapsed after unpin");
        }
    }
}
