//! Conversation state — manages the segment list and push/mutation methods.
//!
//! This module holds the data model. Rendering is handled by
//! `conv_widget::ConversationWidget`.

use super::conv_widget::ConvState;
use super::image::ImageCache;
use super::segments::{
    Segment, SegmentContent, SegmentExportMode, SegmentMeta, SegmentRenderMode, TokenUsage,
    is_plan_progress_text,
};
use super::theme::Theme;

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
    /// Control-plane tool calls hidden from the main transcript unless they fail.
    suppressed_tool_calls: std::collections::HashMap<String, SuppressedToolCall>,
}

#[derive(Debug, Clone)]
struct SuppressedToolCall {
    name: String,
    args_summary: Option<String>,
    detail_args: Option<String>,
}

fn is_suppressed_control_plane_tool(name: &str) -> bool {
    name == crate::tool_registry::core::PLAN
}

/// Build a rich one-liner for consolidated tree entries.
/// Extracts tool-specific key info: path + line count, command + status, etc.
fn consolidation_one_liner(
    tool_name: &str,
    args_summary: Option<&str>,
    result_text: Option<&str>,
) -> String {
    let summary = args_summary.unwrap_or("");
    let result = result_text.unwrap_or("");

    match tool_name {
        "read" | "view" => {
            // Path + line count from result
            let line_count = result.lines().count();
            let first_meaningful = result
                .lines()
                .find(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with("```")
                })
                .unwrap_or("")
                .trim();
            let preview = crate::util::truncate(first_meaningful, 50);
            if line_count > 0 {
                format!("{summary} — {line_count} lines · {preview}")
            } else {
                summary.to_string()
            }
        }
        "edit" | "change" => {
            // Path + change summary from result
            let change_line = result
                .lines()
                .find(|l| l.contains("line(s)") || l.contains("→") || l.contains("Changed"))
                .unwrap_or("");
            let change = change_line
                .trim()
                .trim_start_matches("✓ ")
                .trim_start_matches("  ");
            if !change.is_empty() {
                format!("{summary} — {change}")
            } else {
                summary.to_string()
            }
        }
        "bash" => {
            // Command preview + exit status hint from result
            let cmd = summary;
            let lines = result.lines().count();
            if result.contains("exit code") || result.contains("Command exited") {
                let status = result
                    .lines()
                    .rev()
                    .find(|l| l.contains("exit") || l.contains("Command"))
                    .unwrap_or("");
                format!("{cmd} — {}", status.trim())
            } else if lines > 0 {
                format!("{cmd} — {lines} lines output")
            } else {
                cmd.to_string()
            }
        }
        "write" => {
            let lines = result.lines().count();
            if lines > 0 {
                format!("{summary} — wrote {lines} lines")
            } else {
                summary.to_string()
            }
        }
        "glob" => {
            let matches = result.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{summary} — {matches} matches")
        }
        "grep" | "codebase_search" => {
            let matches = result.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{summary} — {matches} results")
        }
        _ => {
            // Generic: summary + first result line
            let first = result
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            if !first.is_empty() && first != summary {
                let preview = crate::util::truncate(first, 60);
                format!("{summary} — {preview}")
            } else {
                summary.to_string()
            }
        }
    }
}

fn tool_consolidation_key(
    tool_name: &str,
    detail_args: Option<&str>,
    args_summary: Option<&str>,
) -> String {
    if tool_name != "bash" {
        return tool_name.to_string();
    }

    let command = detail_args
        .or(args_summary)
        .and_then(|args| args.lines().next())
        .unwrap_or("bash");
    let first_word = command.split_whitespace().next().unwrap_or("bash");
    let family = match first_word {
        "grep" | "rg" => "search",
        "find" => "find",
        "ls" | "dir" => "list",
        "cat" | "head" | "tail" | "bat" => "read",
        "sed" | "awk" => "transform",
        "curl" | "wget" => "fetch",
        "git" => "git",
        "cargo" => "cargo",
        "npm" | "npx" | "pnpm" | "yarn" | "bun" => "npm",
        "docker" | "podman" => "container",
        "kubectl" | "k" => "kubectl",
        "make" | "cmake" => "build",
        "python" | "python3" | "pip" => "python",
        "rustc" | "rustup" => "rust",
        "go" => "go",
        "dig" | "nslookup" | "host" => "dns",
        "ssh" | "scp" | "rsync" => "remote",
        "tar" | "zip" | "unzip" | "gzip" => "archive",
        "wc" => "count",
        "sort" | "uniq" => "sort",
        "diff" | "patch" => "diff",
        "mkdir" | "rm" | "mv" | "cp" | "chmod" | "chown" => "fs",
        "echo" | "printf" => "echo",
        "test" | "[" => "test",
        "vault" => "vault",
        "sh" | "bash" | "zsh" => "shell",
        _ => first_word,
    };
    format!("bash:{family}")
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
            suppressed_tool_calls: std::collections::HashMap::new(),
        }
    }

    /// Access segments for rendering.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Access segments mutably for focused tests and repair paths.
    #[cfg(test)]
    pub fn segments_mut(&mut self) -> &mut [Segment] {
        &mut self.segments
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

    /// Split borrow for frame rendering paths that need segment data, scroll
    /// state, and ratatui-image protocol state in the same frame.
    pub fn segments_state_and_image_cache(
        &mut self,
    ) -> (&[Segment], &mut ConvState, &mut ImageCache) {
        (&self.segments, &mut self.conv_state, &mut self.image_cache)
    }

    // ─── Push methods ───────────────────────────────────────────

    pub fn push_user(&mut self, text: &str) {
        self.push_user_with_attachments(text, &[]);
    }

    pub fn push_user_with_meta(&mut self, text: &str, meta: SegmentMeta) {
        self.push_user_with_attachments_and_meta(text, &[], meta);
    }

    pub fn push_user_with_attachments(&mut self, text: &str, attachments: &[std::path::PathBuf]) {
        self.push_user_with_attachments_and_meta(text, attachments, SegmentMeta::default());
    }

    pub fn push_user_with_attachments_and_meta(
        &mut self,
        text: &str,
        attachments: &[std::path::PathBuf],
        meta: SegmentMeta,
    ) {
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
            let mut segment = Segment::user_prompt(rendered);
            segment.meta = meta;
            self.segments.push(segment);
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

    pub fn push_operator_copy_block(
        &mut self,
        label: impl Into<String>,
        text: impl Into<String>,
        kind: omegon_traits::OperatorCopyKind,
        copy_attempt: Option<omegon_traits::ClipboardCopyStatus>,
    ) {
        self.segments.push(Segment::operator_copy_block(
            label,
            text,
            kind,
            copy_attempt,
        ));
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_system(&mut self, text: &str) {
        if is_plan_progress_text(text)
            && let Some(existing) = self
                .segments
                .iter_mut()
                .rev()
                .filter_map(|segment| match &mut segment.content {
                    SegmentContent::SystemNotification { text } => Some(text),
                    _ => None,
                })
                .find(|existing| is_plan_progress_text(existing))
        {
            *existing = text.to_string();
            self.conv_state.invalidate();
            self.conv_state.auto_scroll_to_bottom();
            return;
        }

        // Merge consecutive system notifications into a single card to avoid
        // excessive vertical padding (each card has border overhead).
        if let Some(last) = self.segments.last_mut()
            && let SegmentContent::SystemNotification {
                text: ref mut existing,
            } = last.content
        {
            existing.push('\n');
            existing.push_str(text);
            self.conv_state.invalidate();
            self.conv_state.auto_scroll_to_bottom();
            return;
        }
        self.segments.push(Segment::system(text));
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_image(&mut self, path: std::path::PathBuf, alt: &str) {
        self.segments.push(Segment::image(path, alt));
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn push_skill_event(&mut self, event: &omegon_traits::SkillActivationEvent) {
        self.segments.push(Segment::skill_event(event));
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

        if let Some(seg) = self.segments.last_mut()
            && let SegmentContent::AssistantText { text, .. } = &mut seg.content
        {
            text.push_str(delta);
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

        if let Some(seg) = self.segments.last_mut()
            && let SegmentContent::AssistantText { thinking, .. } = &mut seg.content
        {
            thinking.push_str(delta);
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
        self.push_tool_start_with_expanded(id, name, args_summary, detail_args, false);
    }

    pub fn push_tool_start_with_expanded(
        &mut self,
        id: &str,
        name: &str,
        args_summary: Option<&str>,
        detail_args: Option<&str>,
        expanded_by_default: bool,
    ) {
        if is_suppressed_control_plane_tool(name) {
            self.suppressed_tool_calls.insert(
                id.to_string(),
                SuppressedToolCall {
                    name: name.to_string(),
                    args_summary: args_summary.map(str::to_string),
                    detail_args: detail_args.map(str::to_string),
                },
            );
            return;
        }

        let mut seg = Segment::tool_card(id, name);
        if let SegmentContent::ToolCard {
            args_summary: ref mut a,
            detail_args: ref mut d,
            expanded: ref mut e,
            ..
        } = seg.content
        {
            *a = args_summary.map(|s| s.to_string());
            *d = detail_args.map(|s| s.to_string());
            *e = expanded_by_default;
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
        if self.suppressed_tool_calls.contains_key(id) {
            return;
        }

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
                *lp = Some(Box::new(partial));
                self.conv_state.invalidate();
                return;
            }
        }
    }

    pub fn push_tool_end(&mut self, id: &str, is_error: bool, result_text: Option<&str>) {
        if let Some(suppressed) = self.suppressed_tool_calls.remove(id) {
            if is_error {
                self.push_tool_start(
                    id,
                    &suppressed.name,
                    suppressed.args_summary.as_deref(),
                    suppressed.detail_args.as_deref(),
                );
                // `push_tool_start` suppresses plan again; remove that pending
                // entry and materialize the failed call as an ordinary card.
                self.suppressed_tool_calls.remove(id);
                let mut seg = Segment::tool_card(id, &suppressed.name);
                if let SegmentContent::ToolCard {
                    args_summary: ref mut a,
                    detail_args: ref mut d,
                    ..
                } = seg.content
                {
                    *a = suppressed.args_summary;
                    *d = suppressed.detail_args;
                }
                self.segments.push(seg);
                self.push_tool_end(id, true, result_text);
            }
            return;
        }

        // Find the card for this tool call and complete it.
        let mut completed_name: Option<String> = None;
        let mut completed_summary: Option<String> = None;
        let mut completed_key: Option<String> = None;
        let mut completed_idx: Option<usize> = None;

        for (i, seg) in self.segments.iter_mut().enumerate().rev() {
            if let SegmentContent::ToolCard {
                id: tool_id,
                name,
                complete: c,
                is_error: e,
                result_summary: r,
                detail_result: dr,
                live_partial: lp,
                args_summary,
                detail_args,
                ..
            } = &mut seg.content
                && tool_id == id
                && !*c
            {
                *c = true;
                *e = is_error;
                *lp = None;
                let summary = result_text.and_then(|text| {
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
                *r = summary.clone();
                *dr = result_text.map(|text| text.to_string());
                completed_name = Some(name.clone());
                // Build a rich one-liner for consolidated tree view
                completed_summary = Some(consolidation_one_liner(
                    name,
                    args_summary.as_deref(),
                    result_text,
                ));
                completed_key = Some(tool_consolidation_key(
                    name,
                    detail_args.as_deref(),
                    args_summary.as_deref(),
                ));
                completed_idx = Some(i);
                break;
            }
        }

        if completed_name.as_deref() == Some("plan")
            && !is_error
            && let Some(idx) = completed_idx
        {
            self.remove_segment(idx);
            self.conv_state.invalidate();
            return;
        }

        // ── CONSOLIDATION ─────────────────────────────────────────
        // Merge consecutive completed cards of the same tool name
        // into a single grouped card with tree-style entries.
        if let (Some(ref name), Some(ref key), Some(idx)) =
            (completed_name, completed_key, completed_idx)
            && idx > 0
            && !is_error
        {
            self.try_merge_with_predecessor(idx, name, key, completed_summary.as_deref());
        }

        self.conv_state.invalidate();
    }

    fn remove_segment(&mut self, idx: usize) {
        self.segments.remove(idx);
        if let Some(ref mut p) = self.pinned_segment {
            if *p == idx {
                self.pinned_segment = None;
            } else if *p > idx {
                *p -= 1;
            }
        }
        if let Some(ref mut s) = self.selected_segment {
            if *s == idx {
                self.selected_segment = None;
            } else if *s > idx {
                *s -= 1;
            }
        }
    }

    /// Attempt to merge segment at `idx` into the preceding segment if both
    /// are completed, non-error tool cards of the same name. Handles index
    /// fixup for pinned_segment and selected_segment after removal.
    fn try_merge_with_predecessor(
        &mut self,
        idx: usize,
        name: &str,
        key: &str,
        summary: Option<&str>,
    ) {
        let should_merge = matches!(
            &self.segments[idx - 1].content,
            SegmentContent::ToolCard {
                name: prev_name,
                args_summary: prev_args_summary,
                detail_args: prev_detail_args,
                complete: true,
                is_error: false,
                ..
            } if prev_name == name
                && tool_consolidation_key(
                    prev_name,
                    prev_detail_args.as_deref(),
                    prev_args_summary.as_deref(),
                ) == key
        );

        if !should_merge {
            return;
        }

        // Grab the current card's full result before removing
        let current_full_result = if let SegmentContent::ToolCard {
            detail_result: ref dr,
            ..
        } = self.segments[idx].content
        {
            dr.clone().unwrap_or_default()
        } else {
            return;
        };

        // Append to predecessor
        if let SegmentContent::ToolCard {
            detail_result: ref mut prev_result_opt,
            args_summary: ref mut prev_args_summary,
            ..
        } = self.segments[idx - 1].content
        {
            let prev_result = prev_result_opt.get_or_insert_with(String::new);
            if let Some(s) = summary {
                prev_result.push('\n');
                prev_result.push_str(&format!("  + {s}"));
            }
            if !current_full_result.is_empty() {
                prev_result.push_str("\n--- merged entry ---\n");
                prev_result.push_str(&current_full_result);
            }
            let count = prev_result.matches("\n  + ").count() + 1;
            *prev_args_summary = Some(format!("{name} ({count} operations)"));
        }

        // Remove and fix up tracked indices
        self.remove_segment(idx);
    }

    pub fn finalize_message(&mut self) {
        if let Some(seg) = self.segments.last_mut()
            && let SegmentContent::AssistantText { complete, .. } = &mut seg.content
        {
            *complete = true;
        }
        self.streaming = false;
        self.conv_state.invalidate();
        self.conv_state.auto_scroll_to_bottom();
    }

    pub fn maybe_scroll_latest_assistant_to_start(
        &mut self,
        viewport_width: u16,
        viewport_height: u16,
        theme: &dyn Theme,
        mode: SegmentRenderMode,
    ) -> bool {
        if viewport_width == 0 || viewport_height == 0 || self.conv_state.user_scrolled {
            return false;
        }

        let Some(idx) = self
            .segments
            .iter()
            .enumerate()
            .rev()
            .find(|(_, segment)| {
                matches!(
                    segment.content,
                    SegmentContent::AssistantText {
                        ref text,
                        complete: true,
                        ..
                    } if !text.trim().is_empty()
                )
            })
            .map(|(idx, _)| idx)
        else {
            return false;
        };

        let heights = self
            .segments
            .iter()
            .map(|segment| segment.height_in_mode(viewport_width, theme, mode))
            .collect::<Vec<_>>();

        let segment_height = heights[idx];
        if segment_height <= viewport_height.saturating_mul(3) / 2 {
            return false;
        }

        let total_height: u16 = heights.iter().copied().sum();
        let max_scroll = total_height.saturating_sub(viewport_height);
        if max_scroll == 0 {
            return false;
        }

        let segment_top: u16 = heights[..idx].iter().copied().sum();
        let desired_scroll = total_height
            .saturating_sub(viewport_height)
            .saturating_sub(segment_top)
            .min(max_scroll);
        if desired_scroll == 0 {
            return false;
        }

        self.conv_state.scroll_offset = desired_scroll;
        self.conv_state.user_scrolled = true;
        self.conv_state.heights = heights;
        self.conv_state.invalidate();
        self.selected_segment = Some(idx);
        true
    }

    pub fn latest_plan_progress(&self) -> Option<&str> {
        for segment in self.segments.iter().rev() {
            if let SegmentContent::SystemNotification { text } = &segment.content
                && is_plan_progress_text(text)
            {
                return if text.lines().next() == Some("Plan cleared") {
                    None
                } else {
                    Some(text.as_str())
                };
            }
        }
        None
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
        if let Some(seg) = self.segments.get_mut(segment_idx)
            && let SegmentContent::ToolCard { expanded, .. } = &mut seg.content
        {
            *expanded = !*expanded;
            self.conv_state.invalidate();
        }
    }

    /// Toggle pin on the selected tool card if present, otherwise the nearest
    /// visible tool card. When pinned, the segment is expanded and stays visible
    /// at the bottom of the conversation viewport. Pressing Ctrl+O again (or Esc)
    /// unpins and collapses.
    pub fn toggle_pin(&mut self) {
        self.toggle_pin_in_viewport(None);
    }

    pub fn toggle_pin_in_viewport(&mut self, viewport_height: Option<u16>) {
        if let Some(pinned) = self.pinned_segment.take() {
            // Unpin: collapse the segment
            self.toggle_expand(pinned);
            return;
        }

        let selected_tool = self.selected_segment.filter(|&idx| {
            matches!(
                self.segments.get(idx).map(|s| &s.content),
                Some(SegmentContent::ToolCard { .. })
            )
        });
        let visible_tool = self.focused_tool_card_in_viewport(viewport_height);
        let target = self
            .latest_running_tool_card()
            .or(visible_tool)
            .or(selected_tool);

        if let Some(idx) = target {
            // Pin: expand and lock focus
            if let Some(seg) = self.segments.get_mut(idx)
                && let SegmentContent::ToolCard { expanded, .. } = &mut seg.content
                && !*expanded
            {
                *expanded = true;
                self.conv_state.invalidate();
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

    fn latest_running_tool_card(&self) -> Option<usize> {
        self.segments.iter().rposition(|s| {
            matches!(
                s.content,
                SegmentContent::ToolCard {
                    complete: false,
                    ..
                }
            )
        })
    }

    /// Find the nearest tool card segment visible in the viewport.
    /// Uses cached heights from the last render (which used the real width).
    pub fn focused_tool_card(&self) -> Option<usize> {
        self.focused_tool_card_in_viewport(None)
    }

    pub fn focused_tool_card_in_viewport(&self, viewport_height: Option<u16>) -> Option<usize> {
        self.visible_tool_cards(viewport_height)
            .last()
            .copied()
            .or_else(|| {
                self.segments
                    .iter()
                    .rposition(|s| matches!(s.content, SegmentContent::ToolCard { .. }))
            })
    }

    pub fn visible_tool_cards(&self, viewport_height: Option<u16>) -> Vec<usize> {
        let heights = &self.conv_state.heights;
        if heights.len() != self.segments.len() {
            return self.recent_tool_cards(viewport_height);
        }

        let total: u16 = heights.iter().sum();
        let viewport_height = viewport_height.unwrap_or(total).min(total);
        let max_scroll = total.saturating_sub(viewport_height);
        let scroll_offset = self.conv_state.scroll_offset.min(max_scroll);
        let viewport_top = total
            .saturating_sub(viewport_height)
            .saturating_sub(scroll_offset);
        let viewport_bottom = viewport_top.saturating_add(viewport_height);

        let mut y: u16 = 0;
        let mut visible = Vec::new();
        for (i, seg) in self.segments.iter().enumerate() {
            let seg_top = y;
            let seg_bottom = seg_top.saturating_add(heights[i]);
            y = seg_bottom;

            if seg_bottom <= viewport_top {
                continue;
            }
            if seg_top >= viewport_bottom {
                break;
            }
            if matches!(seg.content, SegmentContent::ToolCard { .. }) {
                visible.push(i);
            }
        }
        if visible.is_empty() {
            self.recent_tool_cards(Some(viewport_height))
        } else {
            visible
        }
    }

    fn recent_tool_cards(&self, viewport_height: Option<u16>) -> Vec<usize> {
        let limit = viewport_height.unwrap_or(3).clamp(1, 6) as usize;
        let mut recent: Vec<usize> = self
            .segments
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(idx, segment)| {
                matches!(segment.content, SegmentContent::ToolCard { .. }).then_some(idx)
            })
            .take(limit)
            .collect();
        recent.reverse();
        recent
    }

    pub fn select_latest_visible_tool_card(
        &mut self,
        viewport_height: Option<u16>,
    ) -> Option<usize> {
        let idx = self.focused_tool_card_in_viewport(viewport_height)?;
        self.selected_segment = Some(idx);
        Some(idx)
    }

    pub fn tool_segment_by_id(&self, id: &str) -> Option<&Segment> {
        self.segments.iter().rev().find(|seg| {
            matches!(
                &seg.content,
                SegmentContent::ToolCard { id: tool_id, .. } if tool_id == id
            )
        })
    }

    pub fn tool_inspection_height_by_id(&self, id: &str) -> u16 {
        self.tool_segment_by_id(id)
            .and_then(|seg| match &seg.content {
                SegmentContent::ToolCard {
                    live_partial,
                    detail_result,
                    ..
                } => {
                    let line_count = live_partial
                        .as_deref()
                        .and_then(|partial| {
                            (!partial.tail.trim().is_empty())
                                .then_some(partial.tail.lines().count())
                        })
                        .or_else(|| {
                            detail_result
                                .as_deref()
                                .map(|result| result.lines().count())
                        })
                        .unwrap_or(0);
                    Some(crate::tui::tool_inspection::tool_inspection_height(
                        line_count,
                    ))
                }
                _ => None,
            })
            .unwrap_or(0)
    }

    pub fn latest_expandable_tool_card(&self) -> Option<usize> {
        self.segments.iter().rposition(|s| {
            matches!(
                &s.content,
                SegmentContent::ToolCard {
                    detail_args,
                    detail_result,
                    live_partial,
                    ..
                } if detail_args.as_ref().is_some_and(|s| !s.trim().is_empty())
                    || detail_result.as_ref().is_some_and(|s| !s.trim().is_empty())
                    || live_partial.as_ref().is_some_and(|p| !p.tail.trim().is_empty())
            )
        })
    }

    pub fn latest_expandable_tool_id(&self) -> Option<String> {
        self.latest_expandable_tool_card()
            .and_then(|idx| match &self.segments.get(idx)?.content {
                SegmentContent::ToolCard { id, .. } => Some(id.clone()),
                _ => None,
            })
    }

    pub fn select_next_visible_tool_card(&mut self, viewport_height: Option<u16>) -> Option<usize> {
        let visible = self.visible_tool_cards(viewport_height);
        if visible.is_empty() {
            return None;
        }
        let selected = self.selected_segment;
        let idx = selected
            .and_then(|selected| visible.iter().position(|&idx| idx == selected))
            .map(|pos| visible[(pos + 1) % visible.len()])
            .unwrap_or_else(|| *visible.last().expect("visible is not empty"));
        self.selected_segment = Some(idx);
        Some(idx)
    }

    pub fn select_prev_visible_tool_card(&mut self, viewport_height: Option<u16>) -> Option<usize> {
        let visible = self.visible_tool_cards(viewport_height);
        if visible.is_empty() {
            return None;
        }
        let selected = self.selected_segment;
        let idx = selected
            .and_then(|selected| visible.iter().position(|&idx| idx == selected))
            .map(|pos| visible[(pos + visible.len() - 1) % visible.len()])
            .unwrap_or_else(|| *visible.last().expect("visible is not empty"));
        self.selected_segment = Some(idx);
        Some(idx)
    }

    pub fn expand_visible_tool_cards(&mut self, viewport_height: Option<u16>) -> usize {
        let visible = self.visible_tool_cards(viewport_height);
        let mut changed = 0;
        for idx in visible {
            if let Some(seg) = self.segments.get_mut(idx)
                && let SegmentContent::ToolCard { expanded, .. } = &mut seg.content
                && !*expanded
            {
                *expanded = true;
                changed += 1;
            }
        }
        if changed > 0 {
            self.conv_state.invalidate();
        }
        changed
    }

    pub fn select_segment(&mut self, idx: usize) {
        if idx < self.segments.len() {
            self.selected_segment = Some(idx);
        }
    }

    pub fn clear_selected_segment(&mut self) {
        self.selected_segment = None;
    }

    pub fn selected_segment_index(&self) -> Option<usize> {
        self.selected_segment
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
            .find(|(_, seg)| seg.capabilities().selectable)
            .map(|(idx, _)| idx)
    }

    pub fn first_selectable_segment(&self) -> Option<usize> {
        self.segments
            .iter()
            .enumerate()
            .find(|(_, seg)| seg.capabilities().selectable)
            .map(|(idx, _)| idx)
    }

    pub fn selected_segment_text(&self) -> Option<String> {
        self.selected_segment_text_with_mode(SegmentExportMode::Raw)
    }

    pub fn selected_segment_text_with_mode(&self, mode: SegmentExportMode) -> Option<String> {
        self.selected_or_focused_segment()
            .and_then(|idx| self.segments.get(idx))
            .and_then(|segment| segment.export_copy_text(mode))
    }

    pub fn latest_assistant_text_with_mode(&self, mode: SegmentExportMode) -> Option<String> {
        self.segments.iter().rev().find_map(|segment| {
            if matches!(segment.content, SegmentContent::AssistantText { .. }) {
                segment.export_copy_text(mode)
            } else {
                None
            }
        })
    }

    pub fn move_selected_segment_prev(&mut self) -> Option<usize> {
        let start = self
            .selected_or_focused_segment()
            .or_else(|| self.last_selectable_segment())?;
        let idx = self.segments[..start]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, seg)| seg.capabilities().selectable)
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
            .find(|(_, seg)| seg.capabilities().selectable)
            .map(|(idx, _)| idx)
            .unwrap_or(start);
        self.selected_segment = Some(idx);
        Some(idx)
    }

    fn segment_bounds_at(
        &self,
        viewport: ratatui::prelude::Rect,
        row: u16,
    ) -> Option<(usize, u16, u16)> {
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
                return Some((idx, seg_top, top_offset));
            }
            y_cursor = seg_bottom;
        }
        None
    }

    pub fn segment_at(&self, viewport: ratatui::prelude::Rect, row: u16) -> Option<usize> {
        self.segment_bounds_at(viewport, row).map(|(idx, _, _)| idx)
    }

    pub fn is_segment_collapsed_tool_card(&self, segment_idx: usize) -> bool {
        matches!(
            self.segments.get(segment_idx).map(|seg| &seg.content),
            Some(SegmentContent::ToolCard {
                expanded: false,
                ..
            })
        )
    }

    pub fn is_segment_copyable(&self, segment_idx: usize) -> bool {
        self.segments
            .get(segment_idx)
            .is_some_and(|segment| segment.capabilities().copyable)
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
    fn plan_progress_notifications_replace_latest_snapshot_across_tool_cards() {
        let mut cv = ConversationView::new();
        cv.push_system("Plan progress\nPlan mode: executing\nProgress: 2/6\n\n1. ● A\n2. ◐ B");
        cv.push_tool_start(
            "plan-1",
            "plan",
            Some("{\"action\":\"complete\",\"index\":2}"),
            Some("complete 2"),
        );
        assert!(
            cv.segments.iter().all(
                |segment| !matches!(&segment.content, SegmentContent::ToolCard { name, .. } if name == "plan")
            ),
            "routine plan tool cards should not flash into the transcript while running"
        );
        cv.push_tool_end("plan-1", false, Some("Marked item 2 complete."));
        cv.push_system(
            "Plan progress\nPlan mode: executing\nProgress: 3/6\n\n1. ● A\n2. ● B\n3. ◐ C",
        );

        let plan_segments: Vec<&str> = cv
            .segments
            .iter()
            .filter_map(|segment| match &segment.content {
                SegmentContent::SystemNotification { text } if is_plan_progress_text(text) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();

        assert_eq!(plan_segments.len(), 1);
        assert!(plan_segments[0].contains("Progress: 3/6"));
        assert!(!plan_segments[0].contains("Progress: 2/6"));
        assert!(
            cv.segments.iter().all(
                |segment| !matches!(&segment.content, SegmentContent::ToolCard { name, .. } if name == "plan")
            ),
            "successful plan tool cards should not clutter the transcript"
        );
    }

    #[test]
    fn multiple_successful_plan_calls_leave_one_snapshot_and_no_plan_cards() {
        let mut cv = ConversationView::new();
        for (idx, action) in ["set", "approve", "execute", "complete"]
            .into_iter()
            .enumerate()
        {
            let id = format!("plan-{idx}");
            cv.push_tool_start(&id, "plan", Some(action), Some(action));
            cv.push_tool_end(&id, false, Some("ok"));
            cv.push_system(&format!(
                "Plan progress\nPlan mode: executing\nProgress: {idx}/4\n\n1. ◐ Item"
            ));
        }

        let plan_segments: Vec<&str> = cv
            .segments
            .iter()
            .filter_map(|segment| match &segment.content {
                SegmentContent::SystemNotification { text } if is_plan_progress_text(text) => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();

        assert_eq!(plan_segments.len(), 1);
        assert!(plan_segments[0].contains("Progress: 3/4"));
        assert!(cv.segments.iter().all(
            |segment| !matches!(&segment.content, SegmentContent::ToolCard { name, .. } if name == "plan")
        ));
    }

    #[test]
    fn errored_plan_tool_cards_remain_visible() {
        let mut cv = ConversationView::new();
        cv.push_tool_start(
            "plan-1",
            "plan",
            Some("{\"action\":\"complete\",\"index\":99}"),
            Some("complete 99"),
        );
        cv.push_tool_end("plan-1", true, Some("Plan item index out of range."));

        assert!(
            cv.segments.iter().any(
                |segment| matches!(&segment.content, SegmentContent::ToolCard { name, is_error: true, .. } if name == "plan")
            ),
            "errored plan calls should stay visible because they are actionable"
        );
    }

    #[test]
    fn suppressed_successful_plan_calls_do_not_disturb_selected_or_pinned_indices() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("read-1", "read", Some("README.md"), Some("README.md"));
        cv.push_tool_end("read-1", false, Some("contents"));
        cv.push_tool_start("bash-1", "bash", Some("echo hi"), Some("echo hi"));
        cv.push_tool_end("bash-1", false, Some("hi"));
        cv.selected_segment = Some(1);
        cv.pinned_segment = Some(0);

        cv.push_tool_start("plan-1", "plan", Some("complete 1"), Some("complete 1"));
        cv.push_tool_end("plan-1", false, Some("Marked item 1 complete."));

        assert_eq!(cv.selected_segment, Some(1));
        assert_eq!(cv.pinned_segment, Some(0));
        assert_eq!(cv.segments.len(), 2);
    }

    #[test]
    fn non_plan_tool_cards_still_render_normally() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("read-1", "read", Some("README.md"), Some("README.md"));
        cv.push_tool_end("read-1", false, Some("contents"));

        assert!(cv.segments.iter().any(
            |segment| matches!(&segment.content, SegmentContent::ToolCard { name, complete: true, .. } if name == "read")
        ));
    }

    #[test]
    fn non_plan_system_notifications_still_append_after_tool_cards() {
        let mut cv = ConversationView::new();
        cv.push_system("First notice");
        cv.push_tool_start("t1", "read", Some("README.md"), Some("README.md"));
        cv.push_tool_end("t1", false, Some("contents"));
        cv.push_system("Second notice");

        let system_segments: Vec<&str> = cv
            .segments
            .iter()
            .filter_map(|segment| match &segment.content {
                SegmentContent::SystemNotification { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(system_segments, vec!["First notice", "Second notice"]);
    }

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
    fn selected_assistant_copy_uses_answer_body_without_reasoning() {
        let mut cv = ConversationView::new();
        cv.append_streaming("# Final\nanswer");
        if let SegmentContent::AssistantText { thinking, .. } = &mut cv.segments[0].content {
            *thinking = "private reasoning\n".to_string();
        }
        cv.finalize_message();
        cv.selected_segment = Some(0);

        assert_eq!(
            cv.selected_segment_text_with_mode(SegmentExportMode::Raw)
                .as_deref(),
            Some("# Final\nanswer")
        );
    }

    #[test]
    fn selected_tool_copy_uses_detail_result_without_tool_chrome() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("t1", "bash", Some("echo hi"), Some("echo hi"));
        cv.push_tool_end("t1", false, Some("hi\n"));
        cv.selected_segment = Some(0);

        assert_eq!(
            cv.selected_segment_text_with_mode(SegmentExportMode::Raw)
                .as_deref(),
            Some("hi")
        );
    }

    #[test]
    fn selected_image_copy_returns_none_by_policy() {
        let mut cv = ConversationView::new();
        cv.push_user_with_attachments("", &[std::path::PathBuf::from("/tmp/paste.png")]);
        cv.selected_segment = Some(0);

        assert_eq!(
            cv.selected_segment_text_with_mode(SegmentExportMode::Raw),
            None
        );
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
    fn consecutive_bash_commands_merge_only_with_same_command_family() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("t1", "bash", Some("git status"), Some("git status"));
        cv.push_tool_end("t1", false, Some("clean"));
        cv.push_tool_start("t2", "bash", Some("git push 2>&1"), Some("git push 2>&1"));
        cv.push_tool_end("t2", false, Some("To github.com:example/repo.git"));

        assert_eq!(cv.segments.len(), 1);
        assert!(matches!(
            &cv.segments[0].content,
            SegmentContent::ToolCard {
                args_summary: Some(summary),
                detail_result: Some(result),
                ..
            } if summary == "bash (2 operations)" && result.contains("--- merged entry ---")
        ));
    }

    #[test]
    fn consecutive_bash_commands_do_not_merge_different_command_families() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("t1", "bash", Some("git push 2>&1"), Some("git push 2>&1"));
        cv.push_tool_end("t1", false, Some("To github.com:example/repo.git"));
        cv.push_tool_start(
            "t2",
            "bash",
            Some("kubectl --kubeconfig ~/.kube/brutus.yaml -n argocd get secret"),
            Some("kubectl --kubeconfig ~/.kube/brutus.yaml -n argocd get secret"),
        );
        cv.push_tool_end("t2", false, Some("secret/token"));

        assert_eq!(cv.segments.len(), 2);
        assert!(matches!(
            &cv.segments[0].content,
            SegmentContent::ToolCard {
                args_summary: Some(summary),
                detail_result: Some(result),
                ..
            } if summary == "git push 2>&1" && !result.contains("--- merged entry ---")
        ));
        assert!(matches!(
            &cv.segments[1].content,
            SegmentContent::ToolCard {
                args_summary: Some(summary),
                detail_result: Some(result),
                ..
            } if summary.starts_with("kubectl ") && !result.contains("--- merged entry ---")
        ));
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
    fn tool_start_can_opt_into_default_expansion() {
        let mut cv = ConversationView::new();
        cv.push_tool_start_with_expanded("t1", "bash", Some("ls"), Some("ls"), true);
        cv.push_tool_start("t2", "bash", Some("echo hi"), Some("echo hi"));

        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[0].content {
            assert!(expanded, "opt-in card should start expanded");
        }
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[1].content {
            assert!(!expanded, "ordinary tool card should still start collapsed");
        }
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
    fn completed_assistant_response_stays_at_tail() {
        let mut cv = ConversationView::new();
        cv.push_system("older context");
        cv.append_streaming(&format!(
            "{}\n{}",
            "long answer",
            "wrapped response line with enough detail to require many rendered terminal rows"
                .repeat(20)
        ));
        cv.finalize_message();

        assert!(!cv.conv_state.user_scrolled);
        assert_eq!(cv.conv_state.scroll_offset, 0);
    }

    #[test]
    fn long_completed_assistant_pin_ignores_stale_cached_height() {
        let mut cv = ConversationView::new();
        cv.append_streaming(
            "heading\n\n\
             This answer has enough wrapped prose to exceed the viewport by a wide margin. \
             It should be measured from the completed text, not from whatever short height \
             was cached while the response was still streaming.\n\n\
             - first item\n\
             - second item\n\
             - third item\n",
        );
        cv.finalize_message();
        cv.conv_state.heights = vec![1];

        assert!(cv.maybe_scroll_latest_assistant_to_start(
            24,
            4,
            &Alpharius,
            SegmentRenderMode::Slim,
        ));
        assert!(cv.conv_state.user_scrolled);
        assert!(
            cv.conv_state.heights[0] > 1,
            "pinning must refresh stale completed assistant heights"
        );
    }

    #[test]
    fn short_completed_assistant_response_stays_at_tail() {
        let mut cv = ConversationView::new();
        cv.append_streaming("short answer");
        cv.finalize_message();
        cv.conv_state.heights = vec![8];

        assert!(!cv.maybe_scroll_latest_assistant_to_start(
            40,
            10,
            &Alpharius,
            SegmentRenderMode::Full,
        ));
        assert!(!cv.conv_state.user_scrolled);
        assert_eq!(cv.conv_state.scroll_offset, 0);
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
    fn toggle_pin_uses_visible_viewport_not_top_segment() {
        let mut cv = ConversationView::new();
        for idx in 0..5 {
            let id = format!("t{idx}");
            let path = format!("file{idx}.rs");
            let name = format!("read{idx}");
            cv.push_tool_start(&id, &name, Some(&path), Some(&path));
            cv.push_tool_end(&id, false, Some("result"));
        }
        cv.conv_state.heights = vec![2; 5];
        cv.conv_state.scroll_offset = 0;

        cv.toggle_pin_in_viewport(Some(4));

        assert_eq!(cv.pinned_segment, Some(4));
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[4].content {
            assert!(expanded, "bottom visible tool card should expand");
        } else {
            panic!("expected tool card");
        }
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[0].content {
            assert!(
                !expanded,
                "top tool card must not be expanded from bottom viewport"
            );
        }
    }

    #[test]
    fn toggle_pin_ignores_stale_selected_tool_for_new_visible_set() {
        let mut cv = ConversationView::new();
        for idx in 0..8 {
            let id = format!("t{idx}");
            let path = format!("file{idx}.rs");
            let name = format!("read{idx}");
            cv.push_tool_start(&id, &name, Some(&path), Some(&path));
            cv.push_tool_end(&id, false, Some("result"));
        }
        cv.conv_state.heights = vec![1; 8];
        cv.conv_state.scroll_offset = 0;
        cv.selected_segment = Some(2);

        cv.toggle_pin_in_viewport(Some(3));

        assert_eq!(cv.pinned_segment, Some(7));
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[7].content {
            assert!(expanded, "most recent visible tool card should expand");
        } else {
            panic!("expected tool card");
        }
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[2].content {
            assert!(
                !expanded,
                "stale selected tool from previous set must not expand"
            );
        }
    }
    #[test]
    fn focus_entry_prefers_live_tail_over_visible_tool_card() {
        let mut cv = ConversationView::new();
        for idx in 0..6 {
            let id = format!("t{idx}");
            let path = format!("file{idx}.rs");
            let name = format!("read{idx}");
            cv.push_tool_start(&id, &name, Some(&path), Some(&path));
            cv.push_tool_end(&id, false, Some("result"));
        }
        cv.append_thinking("reasoning about the latest response");

        // Simulate a stale/older viewport where the bottom visible rows still
        // contain tool cards. Explicit tool navigation continues to use the
        // visible tool-card target.
        cv.conv_state.heights = vec![1; cv.segments.len()];
        cv.conv_state.scroll_offset = 2;
        cv.selected_segment = Some(1);

        assert_eq!(cv.select_latest_visible_tool_card(Some(3)), Some(4));
        assert_eq!(cv.selected_segment, Some(4));
    }

    #[test]
    fn visible_tool_focus_cycles_and_expands_current_viewport() {
        let mut cv = ConversationView::new();
        for idx in 0..8 {
            let id = format!("t{idx}");
            let path = format!("file{idx}.rs");
            let name = format!("read{idx}");
            cv.push_tool_start(&id, &name, Some(&path), Some(&path));
            cv.push_tool_end(&id, false, Some("result"));
        }
        cv.conv_state.heights = vec![1; 8];
        cv.conv_state.scroll_offset = 0;
        cv.selected_segment = Some(2);

        assert_eq!(cv.visible_tool_cards(Some(3)), vec![5, 6, 7]);
        assert_eq!(cv.select_latest_visible_tool_card(Some(3)), Some(7));
        assert_eq!(cv.select_next_visible_tool_card(Some(3)), Some(5));
        assert_eq!(cv.select_prev_visible_tool_card(Some(3)), Some(7));

        assert_eq!(cv.expand_visible_tool_cards(Some(3)), 3);
        for idx in [5, 6, 7] {
            if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[idx].content {
                assert!(*expanded, "visible tool card {idx} should expand");
            }
        }
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[2].content {
            assert!(!expanded, "non-visible stale selection must stay collapsed");
        }
    }

    #[test]
    fn visible_tool_cards_falls_back_to_recent_when_height_cache_is_stale() {
        let mut cv = ConversationView::new();
        for idx in 0..8 {
            let id = format!("t{idx}");
            let path = format!("file{idx}.rs");
            let name = format!("read{idx}");
            cv.push_tool_start(&id, &name, Some(&path), Some(&path));
            cv.push_tool_end(&id, false, Some("result"));
        }
        cv.conv_state.heights.clear();

        assert_eq!(cv.visible_tool_cards(Some(3)), vec![5, 6, 7]);
        cv.toggle_pin_in_viewport(Some(3));
        assert_eq!(cv.pinned_segment, Some(7));
        if let SegmentContent::ToolCard { expanded, .. } = &cv.segments[7].content {
            assert!(expanded, "recent fallback target should expand");
        } else {
            panic!("expected tool card");
        }
    }

    #[test]
    fn toggle_pin_prefers_latest_running_tool_card() {
        let mut cv = ConversationView::new();
        cv.push_tool_start("done", "read", Some("old"), Some("old"));
        cv.push_tool_end("done", false, Some("old result"));
        cv.push_tool_start("running", "codebase_search", Some("query"), Some("query"));

        cv.toggle_pin();

        assert_eq!(cv.pinned_segment, Some(1));
        if let SegmentContent::ToolCard {
            expanded, complete, ..
        } = &cv.segments[1].content
        {
            assert!(!complete, "test target should still be running");
            assert!(expanded, "running card should expand before ToolEnd");
        } else {
            panic!("expected running tool card");
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
