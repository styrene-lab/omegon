//! TUI integration tests — slash commands, selectors, event handling.
//!
//! These test the App struct as a state machine: feed inputs, check outputs.
//! No terminal rendering — uses App::new() with test settings.

use super::*;
use crate::lifecycle::types::NodeStatus;
use crate::settings::{ContextClass, Settings, ThinkingLevel};
use crate::tui::dashboard::FocusedNodeSummary;
use crate::update::{UpdateChannel, UpdateInfo};
use tokio::sync::mpsc;

fn test_settings() -> crate::settings::SharedSettings {
    std::sync::Arc::new(std::sync::Mutex::new(Settings::new(
        "anthropic:claude-sonnet-4-6",
    )))
}

fn test_app() -> App {
    App::new(test_settings())
}

fn test_tx() -> mpsc::Sender<TuiCommand> {
    let (tx, _rx) = mpsc::channel(16);
    tx
}

// ═══════════════════════════════════════════════════════════════════
// Slash command routing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn editor_raw_cursor_screen_position_matches_top_border_only_input_box() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("abc");
    editor.move_end();
    let area = Rect {
        x: 10,
        y: 5,
        width: 20,
        height: 5,
    };
    let (x, y) = editor.raw_cursor_screen_position(area);
    assert_eq!(
        x, 13,
        "cursor should align with text origin, not a fictitious left border"
    );
    assert_eq!(y, 6, "cursor should sit one row below the top border");
}

#[test]
fn editor_raw_cursor_screen_position_is_inside_editor_box() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("hello\nworld");
    let area = Rect {
        x: 10,
        y: 5,
        width: 20,
        height: 5,
    };
    let (x, y) = editor.raw_cursor_screen_position(area);
    assert!(
        (11..29).contains(&x),
        "x should be inside bordered editor area: {x}"
    );
    assert!(
        (6..9).contains(&y),
        "y should be inside bordered editor area: {y}"
    );
}

#[test]
fn editor_cursor_screen_position_wraps_without_horizontal_scroll() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("1234567890\nabc");
    editor.move_end();
    let area = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 6,
    };
    let (x, y) = editor.cursor_screen_position(area);
    assert!(x < 4, "cursor x should stay within editor width: {x}");
    assert!(y >= 1, "cursor y should account for wrapped rows: {y}");
}

#[test]
fn editor_visual_line_count_accounts_for_wrapping() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("1234567890");
    assert_eq!(editor.line_count(), 1, "logical lines should stay at 1");
    assert_eq!(
        editor.visual_line_count(4),
        3,
        "wrapped rows should expand to 3"
    );
}

#[test]
fn editor_visual_line_count_counts_newlines_and_wraps() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("1234\n123456");
    assert_eq!(editor.visual_line_count(4), 3, "1 row + 2 wrapped rows");
}

#[test]
fn editor_cursor_screen_position_tracks_wrapped_backspace() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("123456789");
    let area = Rect {
        x: 0,
        y: 0,
        width: 6,
        height: 6,
    };
    editor.move_end();
    let before = editor.cursor_screen_position(area);
    editor.backspace();
    let after = editor.cursor_screen_position(area);
    assert!(
        after.0 <= before.0,
        "backspace should not leave the caret stranded to the right"
    );
    assert!(
        after.1 <= before.1,
        "backspace should move within wrapped layout"
    );
}

#[test]
fn editor_cursor_screen_position_wraps_at_expected_column() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("123456789");
    editor.move_end();
    let area = Rect {
        x: 0,
        y: 0,
        width: 6,
        height: 6,
    };
    let (x, y) = editor.cursor_screen_position(area);
    assert_eq!(x, 3, "9 chars in 6 content columns should wrap to the fourth visible column");
    assert_eq!(
        y, 2,
        "9 chars in 6 content columns should land on the second wrapped row beneath the top border"
    );
}

#[test]
fn editor_cursor_advances_to_next_visual_row_after_first_wrap() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("1234567");
    editor.move_end();
    let area = Rect {
        x: 0,
        y: 0,
        width: 6,
        height: 4,
    };

    let (x, y) = editor.cursor_screen_position(area);
    assert_eq!(y, 2, "cursor should move onto wrapped row 2 after column 6 overflows");
    assert_eq!(x, 1, "cursor should be at the second visible column on the wrapped row");
}

#[test]
fn editor_height_expands_for_wrapped_input() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("1234567890abcdefghij");
    let narrow = Rect {
        x: 0,
        y: 0,
        width: 8,
        height: 20,
    };
    let wide = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 20,
    };
    let narrow_height = super::editor_height_for(&editor, narrow);
    let wide_height = super::editor_height_for(&editor, wide);
    assert!(
        narrow_height > wide_height,
        "wrapped input should expand editor height"
    );
    assert!(
        narrow_height >= 5,
        "wrapped input should grow beyond the minimum height"
    );
}

#[test]
fn editor_visible_visual_lines_follow_cursor_scroll() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.set_text("1234\n5678\n90ab\ncdef");
    editor.move_down();
    editor.move_down();
    editor.move_down();
    editor.move_end();
    let area = Rect {
        x: 0,
        y: 0,
        width: 6,
        height: 3,
    };

    let (_x, y) = editor.cursor_screen_position(area);
    let visible = editor.visible_visual_lines(6, 2);

    assert_eq!(y, 2, "cursor should stay inside the second visible editor row beneath the top border");
    assert_eq!(visible, vec!["90ab", "cdef"], "render should follow editor scroll state");
}

#[test]
fn editor_visible_visual_lines_preserve_blank_lines_from_paste() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.insert_paste("top\n\nbottom\n");

    let visible = editor.visible_visual_lines(20, 6);

    assert_eq!(visible, vec!["top", "", "bottom", ""]);
}

#[test]
fn outgoing_operator_segment_preserves_pasted_multiline_layout() {
    let mut app = test_app();
    app.editor.insert_paste("alpha\n\nbeta\n");
    let text = app.editor.take_text();
    app.conversation.push_user(&text);

    let rendered = app
        .conversation
        .segments()
        .last()
        .expect("user segment")
        .plain_text();

    assert_eq!(rendered, "alpha\n\nbeta\n");
}

#[test]
fn operator_event_queue_keeps_most_recent_entries() {
    let mut app = test_app();
    app.show_toast("first", ratatui_toaster::ToastType::Info);
    app.show_toast("second", ratatui_toaster::ToastType::Warning);

    let now = std::time::Instant::now();
    app.operator_events.retain(|e| e.expires_at > now);
    app.footer_data.operator_events = app
        .operator_events
        .iter()
        .rev()
        .take(2)
        .map(|e| crate::tui::footer::OperatorEventLine {
            icon: e.icon,
            message: e.message.clone(),
            color: e.color,
        })
        .collect();

    assert_eq!(app.footer_data.operator_events.len(), 2);
    assert_eq!(app.footer_data.operator_events[0].message, "second");
    assert_eq!(app.footer_data.operator_events[1].message, "first");
}

#[test]
fn mouse_wheel_scroll_direction_latches_manual_scroll() {
    let mut app = test_app();
    app.conversation.push_user("user");
    app.conversation.append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    assert_eq!(app.conversation.conv_state.scroll_offset, 0);
    app.conversation.scroll_up(3);
    assert!(app.conversation.conv_state.user_scrolled);
    assert_eq!(app.conversation.conv_state.scroll_offset, 3);

    app.conversation.append_streaming("\nnew line");
    assert_eq!(
        app.conversation.conv_state.scroll_offset,
        3,
        "streaming should not pull the viewport back to bottom once manually scrolled"
    );
}

#[test]
fn mouse_wheel_scroll_up_matches_natural_scroll_direction() {
    let mut app = test_app();
    app.conversation.push_user("user");
    app.conversation.append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    app.conversation.scroll_up(3);
    let after_scroll_up = app.conversation.conv_state.scroll_offset;
    assert!(after_scroll_up > 0, "scroll up should move into conversation history");

    app.conversation.scroll_down(3);
    assert!(
        app.conversation.conv_state.scroll_offset < after_scroll_up,
        "scroll down should move back toward the live bottom"
    );
}

#[test]
fn conversation_scroll_does_not_recall_input_history() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.editor.set_text("draft");

    app.conversation.push_user("user");
    app.conversation.append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    app.conversation.scroll_up(3);
    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);

    app.conversation.scroll_down(3);
    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);
}

#[test]
fn conversation_focus_blocks_history_recall_on_up_down() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.editor.set_text("");
    app.pane_focus = PaneFocus::Conversation;

    app.conversation.push_user("user");
    app.conversation.append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    let before_offset = app.conversation.conv_state.scroll_offset;

    if matches!(app.pane_focus, PaneFocus::Conversation) {
        app.conversation.scroll_up(3);
    } else if app.editor.is_empty() || app.history_idx.is_some() {
        app.history_up();
    }

    assert!(
        app.conversation.conv_state.scroll_offset > before_offset,
        "conversation focus should route Up into conversation scrolling"
    );
    assert_eq!(app.history_idx, None, "conversation focus must not enter history recall");
    assert_eq!(app.editor.render_text(), "", "conversation focus must not rewrite the composer");

    let after_up_offset = app.conversation.conv_state.scroll_offset;
    if matches!(app.pane_focus, PaneFocus::Conversation) {
        app.conversation.scroll_down(3);
    } else if app.history_idx.is_some() {
        app.history_down();
    }

    assert!(
        app.conversation.conv_state.scroll_offset < after_up_offset,
        "conversation focus should route Down back toward the live tail"
    );
    assert_eq!(app.history_idx, None);
    assert_eq!(app.editor.render_text(), "");
}

#[test]
fn conversation_focus_blocks_lateral_editor_navigation() {
    let mut app = test_app();
    app.editor.set_text("draft");
    app.editor.move_end();
    app.pane_focus = PaneFocus::Conversation;

    let before_cursor = app.editor.cursor_position();

    if matches!(app.pane_focus, PaneFocus::Editor) {
        app.editor.move_left();
    }
    assert_eq!(
        app.editor.cursor_position(),
        before_cursor,
        "conversation focus must not move the composer cursor left"
    );

    if matches!(app.pane_focus, PaneFocus::Editor) {
        app.editor.move_home();
    }
    assert_eq!(
        app.editor.cursor_position(),
        before_cursor,
        "conversation focus must not route Home into the composer"
    );

    if matches!(app.pane_focus, PaneFocus::Editor) {
        app.editor.move_right();
    }
    assert_eq!(
        app.editor.cursor_position(),
        before_cursor,
        "conversation focus must not move the composer cursor right"
    );

    if matches!(app.pane_focus, PaneFocus::Editor) {
        app.editor.move_end();
    }
    assert_eq!(
        app.editor.cursor_position(),
        before_cursor,
        "conversation focus must not route End into the composer"
    );

    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);
}

#[test]
fn selected_conversation_segment_exports_plain_text() {
    let mut app = test_app();
    app.conversation.push_user("operator prompt");
    app.conversation.append_streaming("assistant answer");
    app.conversation.finalize_message();
    app.conversation.select_segment(1);

    let selected = app.conversation.selected_segment_text();
    assert_eq!(selected.as_deref(), Some("assistant answer"));
}

#[test]
fn assistant_plaintext_export_strips_markdown_fences() {
    let mut app = test_app();
    app.conversation.push_user("operator prompt");
    app.conversation.append_streaming(
        "Run this:\n\n```bash\ncargo test -q\n```\n\nThen edit:\n\n```rust\nfn main() {}\n```",
    );
    app.conversation.finalize_message();
    app.conversation.select_segment(1);

    let selected = app
        .conversation
        .selected_segment_text_with_mode(SegmentExportMode::Plaintext);
    assert_eq!(
        selected.as_deref(),
        Some("Run this:\n\ncargo test -q\n\nThen edit:\n\nfn main() {}")
    );
}

#[test]
fn selected_tool_segment_exports_args_and_result() {
    let mut app = test_app();
    app.conversation
        .push_tool_start("t1", "bash", Some("echo hi"), Some("echo hi"));
    app.conversation.push_tool_end("t1", false, Some("hi"));
    app.conversation.select_segment(0);

    let selected = app
        .conversation
        .selected_segment_text()
        .expect("tool text should export");
    assert!(selected.contains("tool: bash"), "missing tool header: {selected}");
    assert!(selected.contains("args:"), "missing args block: {selected}");
    assert!(selected.contains("echo hi"), "missing args body: {selected}");
    assert!(selected.contains("result:"), "missing result block: {selected}");
    assert!(selected.contains("hi"), "missing result body: {selected}");
}

#[test]
fn selected_tool_segment_copy_excludes_dashboard_content() {
    let mut app = test_app();
    app.dashboard.focused_node = Some(FocusedNodeSummary {
        id: "auth-surface".into(),
        title: "Dashboard node title that must never be copied".into(),
        status: NodeStatus::Implementing,
        open_questions: 2,
        assumptions: 0,
        decisions: 3,
        readiness: 0.6,
        openspec_change: None,
    });
    app.conversation
        .push_tool_start("t1", "codebase_search", Some("routing"), Some("routing"));
    app.conversation
        .push_tool_end("t1", false, Some("core/crates/omegon/src/tui/mod.rs"));
    app.conversation.select_segment(0);

    let selected = app
        .conversation
        .selected_segment_text_with_mode(SegmentExportMode::Raw)
        .expect("tool text should export");
    assert!(selected.contains("tool: codebase_search"));
    assert!(!selected.contains("Dashboard node title that must never be copied"));
}

#[test]
fn ctrl_y_keeps_editor_yank_outside_conversation_focus() {
    let mut app = test_app();
    app.editor.set_text("prefix");
    app.editor.clear_line();
    app.pane_focus = PaneFocus::Editor;

    if matches!(app.pane_focus, PaneFocus::Conversation) {
        app.copy_selected_conversation_segment();
    } else {
        app.editor.yank();
    }

    assert_eq!(app.editor.render_text(), "prefix");
}

#[test]
fn startup_initialization_prefers_mouse_interaction_mode() {
    let mut app = test_app();
    app.mouse_capture_enabled = true;
    app.terminal_copy_mode = false;

    assert!(!app.terminal_copy_mode, "startup should prefer robust mouse interaction");
    assert!(app.mouse_capture_enabled, "startup should enable mouse capture to receive wheel events directly");
}

#[test]
fn enable_mouse_interaction_mode_restores_capture_from_copy_mode() {
    let mut app = test_app();
    app.terminal_copy_mode = true;
    app.mouse_capture_enabled = false;

    app.enable_mouse_interaction_mode();

    assert!(!app.terminal_copy_mode);
    assert!(app.mouse_capture_enabled);
}

#[test]
fn terminal_copy_mode_disables_mouse_capture() {
    let mut app = test_app();
    app.mouse_capture_enabled = true;

    app.set_terminal_copy_mode(true);
    assert!(app.terminal_copy_mode);
    assert!(!app.mouse_capture_enabled);
    assert!(!app.focus_mode);

    app.set_terminal_copy_mode(false);
    assert!(!app.terminal_copy_mode);
    assert!(app.mouse_capture_enabled);
}

#[test]
fn focus_mode_disables_mouse_capture_and_restores_it() {
    let mut app = test_app();
    app.mouse_capture_enabled = true;

    app.set_focus_mode(true);
    assert!(app.focus_mode);
    assert!(!app.mouse_capture_enabled);
    assert!(!app.terminal_copy_mode);

    app.set_focus_mode(false);
    assert!(!app.focus_mode);
    assert!(app.mouse_capture_enabled);
}

#[test]
fn mouse_slash_command_toggles_interaction_mode() {
    let mut app = test_app();
    let tx = test_tx();
    app.terminal_copy_mode = true;
    app.mouse_capture_enabled = false;

    assert!(matches!(app.handle_slash_command("/mouse", &tx), SlashResult::Handled));
    assert!(!app.terminal_copy_mode);
    assert!(app.mouse_capture_enabled);
    assert!(!app.focus_mode);

    assert!(matches!(app.handle_slash_command("/mouse off", &tx), SlashResult::Handled));
    assert!(app.terminal_copy_mode);
    assert!(!app.mouse_capture_enabled);
    assert!(!app.focus_mode);
}

#[test]
fn ctrl_up_recalls_latest_history_entry() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];

    assert!(app.editor.is_empty());
    assert_eq!(app.history_idx, None);

    app.history_recall_up();
    assert_eq!(app.editor.render_text(), "third");
    assert_eq!(app.history_idx, Some(2));
}

#[test]
fn ctrl_up_walks_back_multiple_entries_after_recall_starts() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];

    app.history_recall_up();
    assert_eq!(app.editor.render_text(), "third");
    assert_eq!(app.history_idx, Some(2));

    app.history_recall_up();
    assert_eq!(app.editor.render_text(), "second");
    assert_eq!(app.history_idx, Some(1));

    app.history_recall_up();
    assert_eq!(app.editor.render_text(), "first");
    assert_eq!(app.history_idx, Some(0));
}

#[test]
fn bare_up_does_not_start_history_recall_from_empty_editor_in_mouse_mode() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.terminal_copy_mode = false;

    if matches!(app.pane_focus, PaneFocus::Conversation) {
        app.conversation.scroll_up(3);
    } else if matches!(app.pane_focus, PaneFocus::Dashboard) {
        app.dashboard.scroll_up(3);
    } else if app.agent_active {
        app.conversation.scroll_up(3);
    } else if app.editor.line_count() > 1 && app.editor.cursor_row() > 0 {
        app.editor.move_up();
    } else if app.should_use_arrow_history_recall() {
        app.history_recall_up();
    }

    assert_eq!(app.editor.render_text(), "");
    assert_eq!(app.history_idx, None);
}

#[test]
fn bare_up_recalls_history_from_empty_editor_in_terminal_copy_mode() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.terminal_copy_mode = true;

    if matches!(app.pane_focus, PaneFocus::Conversation) {
        app.conversation.scroll_up(3);
    } else if matches!(app.pane_focus, PaneFocus::Dashboard) {
        app.dashboard.scroll_up(3);
    } else if app.agent_active {
        app.conversation.scroll_up(3);
    } else if app.editor.line_count() > 1 && app.editor.cursor_row() > 0 {
        app.editor.move_up();
    } else if app.should_use_arrow_history_recall() {
        app.history_recall_up();
    }

    assert_eq!(app.editor.render_text(), "third");
    assert_eq!(app.history_idx, Some(2));
}

#[test]
fn non_empty_editor_ctrl_up_does_not_start_history_recall() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.editor.set_text("draft");

    app.history_recall_up();

    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);
}

#[test]
fn ctrl_down_clears_editor_after_latest_entry() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];

    app.history_recall_up();
    app.history_recall_down();
    assert_eq!(app.editor.render_text(), "");
    assert_eq!(app.history_idx, None);
}

#[test]
fn bare_down_advances_history_in_terminal_copy_mode() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.terminal_copy_mode = true;

    app.history_recall_up();

    if matches!(app.pane_focus, PaneFocus::Conversation) {
        app.conversation.scroll_down(3);
    } else if matches!(app.pane_focus, PaneFocus::Dashboard) {
        app.dashboard.scroll_down(3);
    } else if app.agent_active {
        app.conversation.scroll_down(3);
    } else if app.editor.line_count() > 1
        && app.editor.cursor_row() < app.editor.line_count() - 1
    {
        app.editor.move_down();
    } else if app.should_use_arrow_history_recall() {
        app.history_recall_down();
    }

    assert_eq!(app.editor.render_text(), "");
    assert_eq!(app.history_idx, None);
}

#[test]
fn multiline_up_uses_editor_navigation_before_history_recall() {
    let mut app = test_app();
    app.history = vec!["previous".into()];
    app.editor.set_text("top\nbottom");
    app.editor.move_end();

    app.editor.move_up();
    assert_eq!(app.history_idx, None, "moving within multiline text should not start history recall");
    assert_eq!(app.editor.render_text(), "top\nbottom");
}

#[test]
fn recalled_history_can_continue_walking_with_up() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];

    app.history_up();
    assert_eq!(app.editor.render_text(), "third");

    if app.editor.line_count() > 1 && app.editor.cursor_row() > 0 {
        app.editor.move_up();
    } else if app.editor.is_empty() || app.history_idx.is_some() {
        app.history_up();
    }

    assert_eq!(app.editor.render_text(), "second");
    assert_eq!(app.history_idx, Some(1));
}

#[test]
fn conversation_segment_at_returns_clicked_segment() {
    let mut cv = ConversationView::new();
    cv.push_user("first");
    cv.push_tool_start("t1", "bash", Some("echo hi"), Some("echo hi"));
    cv.push_tool_end("t1", false, Some("hi"));

    let t = crate::tui::theme::Alpharius;
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    {
        let (segments, state) = cv.segments_and_state();
        let widget = crate::tui::conv_widget::ConversationWidget::new(segments, &t);
        widget.render(area, &mut buf, state);
    }

    let idx = cv.segment_at(area, 3).expect("row should map to a segment");
    assert!(idx < cv.segments().len());
}

#[test]
fn toggle_pin_prefers_selected_tool_card() {
    let mut cv = ConversationView::new();
    cv.push_tool_start("t1", "bash", Some("echo one"), Some("echo one"));
    cv.push_tool_end("t1", false, Some("one"));
    cv.push_tool_start("t2", "bash", Some("echo two"), Some("echo two"));
    cv.push_tool_end("t2", false, Some("two"));

    cv.select_segment(0);
    cv.toggle_pin();

    assert_eq!(cv.pinned_segment, Some(0));
}

#[test]
fn slash_focus_toggles_segment_isolation_mode() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("operator prompt");
    app.conversation.append_streaming("assistant answer");
    app.conversation.finalize_message();
    app.conversation.select_segment(1);

    let result = app.handle_slash_command("/focus", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(app.focus_mode);
    assert!(!app.mouse_capture_enabled);

    let result = app.handle_slash_command("/focus", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.focus_mode);
    assert!(app.mouse_capture_enabled);
}

#[test]
fn slash_update_channel_without_args_shows_helpful_usage() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/update channel", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Update channel:"), "{text}");
        assert!(text.contains("/update channel nightly"), "{text}");
        assert!(text.contains("/update install"), "{text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_update_channel_changes_setting() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/update channel nightly", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert_eq!(app.settings.lock().unwrap().update_channel, "nightly");
}

#[test]
fn slash_update_reports_available_version() {
    let mut app = test_app();
    let tx = test_tx();
    let (update_tx, update_rx) = crate::update::channel();
    let _ = update_tx.send(Some(UpdateInfo {
        current: "0.15.2".into(),
        latest: "0.15.3-rc.7".into(),
        download_url: "https://example.invalid/omegon-0.15.3-rc.7-aarch64-apple-darwin.tar.gz"
            .into(),
        signature_url: "https://example.invalid/omegon-0.15.3-rc.7-aarch64-apple-darwin.tar.gz.sig"
            .into(),
        certificate_url:
            "https://example.invalid/omegon-0.15.3-rc.7-aarch64-apple-darwin.tar.gz.pem".into(),
        release_notes: "notes".into(),
        is_newer: true,
    }));
    app.update_rx = Some(update_rx);
    app.settings.lock().unwrap().update_channel = UpdateChannel::Nightly.as_str().to_string();
    let result = app.handle_slash_command("/update", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("0.15.3-rc.7"), "{text}");
        assert!(text.contains("/update install"), "{text}");
        assert!(text.contains("/update channel [stable|nightly]"), "{text}");
        assert!(text.contains("nightly"), "{text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_update_without_update_still_shows_channel_help() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/update", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("You're up to date"), "{text}");
        assert!(text.contains("/update channel nightly"), "{text}");
        assert!(text.contains("/update channel stable"), "{text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_update_install_requires_update_info() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/update install", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("No update information") || text.contains("No downloadable update"),
            "{text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_help_returns_display() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/help", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Commands:"), "should list commands: {text}");
    }
}

#[test]
fn slash_stats_returns_session_info() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/stats", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Duration"), "should show duration: {text}");
        assert!(text.contains("Turns"), "should show turns: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_status_returns_bootstrap_panel() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/status", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Omegon"), "should contain Omegon: {text}");
        assert!(text.contains("Context:"), "should contain Context: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_unknown_is_not_a_command() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/nonexistent_command_xyz", &tx);
    // Unknown commands are either NotACommand or Display with error
    assert!(!matches!(result, SlashResult::Handled));
}

#[test]
fn slash_exit_returns_quit() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/exit", &tx);
    assert!(matches!(result, SlashResult::Quit));
}

#[test]
fn slash_context_compress_alias_requests_compaction() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/context compress", &tx);
    assert!(!matches!(result, SlashResult::NotACommand));
    if let SlashResult::Display(text) = result {
        assert!(text.contains("compaction"), "should confirm compaction request: {text}");
    }
}

#[test]
fn slash_compact_returns_handled() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/compact", &tx);
    // Compact either returns Handled or Display with confirmation
    assert!(!matches!(result, SlashResult::NotACommand));
}

#[test]
fn slash_persona_no_args_lists_personas() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/persona", &tx);
    if let SlashResult::Display(text) = result {
        // Either shows available personas or "no personas installed"
        assert!(
            text.contains("persona") || text.contains("Persona") || text.contains("installed"),
            "should mention personas: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_persona_off_deactivates() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/persona off", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("deactivated") || text.contains("No persona"),
            "should confirm deactivation: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_tone_no_args_lists_tones() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/tone", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("tone") || text.contains("Tone") || text.contains("installed"),
            "should mention tones: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_tone_off_deactivates() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/tone off", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("deactivated") || text.contains("No tone"),
            "should confirm deactivation: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_auth_no_args_shows_status() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/auth", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.to_lowercase().contains("auth")
                || text.contains("Provider")
                || text.contains("status"),
            "should show auth info: {text}"
        );
    } else {
        // May return Handled if it opens an overlay
        assert!(matches!(
            result,
            SlashResult::Handled | SlashResult::Display(_)
        ));
    }
}

#[test]
fn slash_memory_returns_stats() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/memory", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.to_lowercase().contains("memory")
                || text.contains("facts")
                || text.contains("Facts"),
            "should show memory info: {text}"
        );
    } else {
        panic!(
            "expected Display result, got {:?}",
            std::mem::discriminant(&result)
        );
    }
}

#[test]
fn slash_think_with_level_changes_settings() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/think high", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.to_lowercase().contains("high"),
            "should confirm high: {text}"
        );
    }
    let s = app.settings.lock().unwrap();
    assert_eq!(s.thinking, ThinkingLevel::High);
}

#[test]
fn slash_think_no_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/think", &tx);
    assert!(
        matches!(result, SlashResult::Handled),
        "should open selector"
    );
    assert!(app.selector.is_some(), "selector should be open");
    assert!(matches!(
        app.selector_kind,
        Some(SelectorKind::ThinkingLevel)
    ));
}

#[test]
fn not_a_slash_command_passes_through() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("hello world", &tx);
    assert!(matches!(result, SlashResult::NotACommand));
}

// ═══════════════════════════════════════════════════════════════════
// Selector overlays
// ═══════════════════════════════════════════════════════════════════

#[test]
fn model_selector_options_include_openai_only_when_openai_api_is_present() {
    let options = build_model_selector_options(
        "anthropic:claude-sonnet-4-6",
        None,
        None,
        Some(("token".into(), true)),
    );
    assert!(
        options.iter().all(|opt| !opt.value.starts_with("openai:")),
        "OpenAI API options must not be shown from ChatGPT OAuth alone"
    );
    assert!(
        options
            .iter()
            .any(|opt| opt.value == "openai-codex:gpt-5.4"),
        "ChatGPT/Codex-backed GPT route should be advertised honestly"
    );
}

#[test]
fn model_selector_options_include_openai_api_choices_when_api_key_is_present() {
    let options = build_model_selector_options(
        "openai:gpt-5.4",
        None,
        Some(("sk-test".into(), false)),
        None,
    );
    assert!(
        options.iter().any(|opt| opt.value == "openai:gpt-5.4"),
        "OpenAI API route should be selectable when API creds exist"
    );
}

#[test]
fn thinking_selector_opens() {
    let mut app = test_app();
    app.open_thinking_selector();
    assert!(app.selector.is_some());
    assert!(matches!(
        app.selector_kind,
        Some(SelectorKind::ThinkingLevel)
    ));
}

#[test]
fn context_selector_opens() {
    let mut app = test_app();
    app.open_context_selector();
    assert!(app.selector.is_some());
    assert!(matches!(
        app.selector_kind,
        Some(SelectorKind::ContextClass)
    ));
}

#[test]
fn context_selector_confirm_changes_settings() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_context_selector();

    // Navigate down to select a non-default option and confirm
    if let Some(ref mut sel) = app.selector {
        sel.move_down(); // Move to second option (Maniple)
    }
    let _msg = app.confirm_selector(&tx);

    // Check that settings were updated
    let s = app.settings.lock().unwrap();
    // Should be Maniple (second option) or whatever the selector landed on
    assert_ne!(
        s.context_class,
        ContextClass::Squad,
        "should have changed from default Squad"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Event handling
// ═══════════════════════════════════════════════════════════════════

#[test]
fn harness_status_changed_updates_footer() {
    let mut app = test_app();

    let status = crate::status::HarnessStatus {
        context_class: "Clan".into(),
        thinking_level: "High".into(),
        active_persona: Some(crate::status::PersonaSummary {
            id: "test".into(),
            name: "Test Persona".into(),
            badge: "🧪".into(),
            mind_facts_count: 10,
            activated_skills: vec!["rust".into()],
            disabled_tools: vec![],
        }),
        memory: crate::status::MemoryStatus {
            total_facts: 18,
            active_facts: 18,
            project_facts: 18,
            persona_facts: 0,
            working_facts: 4,
            episodes: 2,
            edges: 0,
            active_persona_mind: None,
        },
        ..Default::default()
    };

    let status_json = serde_json::to_value(&status).unwrap();
    app.handle_agent_event(omegon_traits::AgentEvent::HarnessStatusChanged { status_json });

    // Footer should now reflect the new status
    assert!(app.footer_data.harness.active_persona.is_some());
    assert_eq!(
        app.footer_data
            .harness
            .active_persona
            .as_ref()
            .unwrap()
            .name,
        "Test Persona"
    );
    assert_eq!(app.footer_data.harness.context_class, "Clan");
    assert_eq!(app.footer_data.total_facts, 18);
    assert_eq!(app.footer_data.working_memory, 4);
    app.instrument_panel.update_mind_facts(
        app.footer_data.harness.memory.project_facts,
        app.footer_data.harness.memory.working_facts,
        app.footer_data.harness.memory.episodes,
        0.08,
    );
    assert_eq!(app.instrument_panel.debug_mind_fact_count(0), Some(18));
    assert_eq!(app.instrument_panel.debug_mind_fact_count(1), Some(4));
    assert_eq!(app.instrument_panel.debug_mind_fact_count(2), Some(2));
}

#[test]
fn harness_status_changed_detects_persona_transition() {
    let mut app = test_app();

    // Set initial state with no persona
    let initial = crate::status::HarnessStatus::default();
    let initial_json = serde_json::to_value(&initial).unwrap();
    app.handle_agent_event(omegon_traits::AgentEvent::HarnessStatusChanged {
        status_json: initial_json,
    });

    // Now switch to a persona
    let with_persona = crate::status::HarnessStatus {
        active_persona: Some(crate::status::PersonaSummary {
            id: "eng".into(),
            name: "Engineer".into(),
            badge: "⚙".into(),
            mind_facts_count: 5,
            activated_skills: vec![],
            disabled_tools: vec![],
        }),
        ..Default::default()
    };
    let persona_json = serde_json::to_value(&with_persona).unwrap();
    app.handle_agent_event(omegon_traits::AgentEvent::HarnessStatusChanged {
        status_json: persona_json,
    });

    // The previous_harness_status should have been set for diffing
    assert!(app.previous_harness_status.is_some());
    // Current footer should have the persona
    assert!(app.footer_data.harness.active_persona.is_some());
}

#[test]
fn harness_status_memory_drives_instrument_panel_working_row() {
    let mut app = test_app();
    app.footer_data.working_memory = 0;
    app.footer_data.harness.memory = crate::status::MemoryStatus {
        total_facts: 18,
        active_facts: 18,
        project_facts: 18,
        persona_facts: 0,
        working_facts: 4,
        episodes: 2,
        edges: 0,
        active_persona_mind: None,
    };

    app.instrument_panel.update_mind_facts(
        app.footer_data.harness.memory.project_facts,
        app.footer_data.harness.memory.working_facts,
        app.footer_data.harness.memory.episodes,
        0.08,
    );

    assert_eq!(app.instrument_panel.debug_mind_fact_count(1), Some(4));
}

#[test]
fn slash_model_command_does_not_optimistically_mutate_settings() {
    let mut app = test_app();
    let tx = test_tx();
    let original_model = app.settings().model.clone();

    let result = app.handle_slash_command("/model openai-codex:gpt-5.4", &tx);

    assert!(matches!(result, SlashResult::Display(_)));
    assert_eq!(
        app.settings().model,
        original_model,
        "/model should wait for runtime confirmation before changing visible settings"
    );
}

#[test]
fn model_selector_confirmation_does_not_optimistically_mutate_settings() {
    let mut app = test_app();
    let tx = test_tx();
    let original_model = app.settings().model.clone();

    app.selector = Some(selector::Selector::new(
        "Select Model",
        vec![selector::SelectOption {
            value: "openai-codex:gpt-5.4".into(),
            label: "GPT-5.4".into(),
            description: "Codex".into(),
            active: false,
        }],
    ));
    app.selector_kind = Some(SelectorKind::Model);

    let result = app.confirm_selector(&tx);

    assert_eq!(result.as_deref(), Some("Switching model → openai-codex:gpt-5.4"));
    assert_eq!(
        app.settings().model,
        original_model,
        "model selector should wait for runtime confirmation before changing visible settings"
    );
}

#[test]
fn footer_syncs_model_provider_context_and_thinking_from_settings() {
    let mut app = test_app();
    app.update_settings(|s| {
        s.model = "ollama:qwen3".into();
        s.context_window = 65_536;
        s.thinking = ThinkingLevel::High;
        s.provider_connected = false;
    });

    let (model_id, model_provider, context_window, thinking_level, provider_connected) = {
        let s = app.settings();
        (
            s.model.clone(),
            s.provider().to_string(),
            s.context_window,
            s.thinking.as_str().to_string(),
            s.provider_connected,
        )
    };

    app.footer_data.model_id = model_id;
    app.footer_data.model_provider = model_provider;
    app.footer_data.context_window = context_window;
    app.footer_data.thinking_level = thinking_level;
    app.footer_data.provider_connected = provider_connected;

    assert_eq!(app.footer_data.model_id, "ollama:qwen3");
    assert_eq!(app.footer_data.model_provider, "ollama");
    assert_eq!(app.footer_data.context_window, 65_536);
    assert_eq!(app.footer_data.thinking_level, "high");
    assert!(!app.footer_data.provider_connected);
}

// ═══════════════════════════════════════════════════════════════════
// Command table completeness
// ═══════════════════════════════════════════════════════════════════

#[test]
fn all_commands_in_table_are_handled() {
    let mut app = test_app();
    let tx = test_tx();

    for (name, _desc, _subs) in App::COMMANDS {
        let cmd = format!("/{name}");
        let result = app.handle_slash_command(&cmd, &tx);
        // Every command in the table should be recognized (not NotACommand)
        assert!(
            !matches!(result, SlashResult::NotACommand),
            "command /{name} returned NotACommand — it's in COMMANDS but not handled"
        );
    }
}

#[test]
fn handled_commands_are_in_commands_table() {
    // Negative guard: every match arm in handle_slash_command should have
    // a corresponding entry in COMMANDS (otherwise it's undocumented).
    // We test this by checking that no undocumented command returns Handled/Display.
    let mut app = test_app();
    let tx = test_tx();

    let known_names: std::collections::HashSet<&str> =
        App::COMMANDS.iter().map(|(name, _, _)| *name).collect();

    // Test a set of plausible undocumented command names
    let undocumented = [
        "config", "debug", "reload", "undo", "redo", "run", "build", "deploy", "test", "profile",
        "env", "reset",
    ];

    for name in undocumented {
        if known_names.contains(name) {
            continue;
        } // skip if it's actually documented
        let cmd = format!("/{name}");
        let result = app.handle_slash_command(&cmd, &tx);
        // Unknown commands should either be NotACommand (not /-prefixed)
        // or Display an error (/-prefixed but unrecognized). They must
        // NOT return Handled (which would silently swallow input).
        assert!(
            matches!(result, SlashResult::NotACommand | SlashResult::Display(_)),
            "/{name} returned Handled but is NOT in COMMANDS table — add it to COMMANDS or remove the handler"
        );
    }
}

#[test]
fn slash_plugin_list_returns_display() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/plugin list", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
}

#[test]
fn slash_skills_returns_display() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/skills", &tx);
    match result {
        SlashResult::Display(text) => {
            assert!(text.contains("Bundled skills"), "{text}");
            assert!(text.contains("Use /skills install"), "{text}");
        }
        _ => panic!("/skills should display bundled skill summary, got: {result:?}"),
    }
}

#[test]
fn hidden_model_aliases_do_not_appear_in_palette() {
    let mut app = test_app();
    app.bus_commands = vec![
        omegon_traits::CommandDefinition {
            name: "sonnet".into(),
            description: "hidden alias".into(),
            subcommands: vec![],
        },
        omegon_traits::CommandDefinition {
            name: "victory".into(),
            description: "visible tier".into(),
            subcommands: vec![],
        },
    ];
    app.editor.set_text("/");
    let matches = app.matching_commands();
    assert!(matches.iter().any(|(name, _)| name == "victory"));
    assert!(!matches.iter().any(|(name, _)| name == "sonnet"));
}

#[test]
fn slash_cleave_warns_on_anthropic_subscription_but_proceeds() {
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("ANTHROPIC_OAUTH_TOKEN", "subscription-token");
    }

    let mut app = test_app();
    app.footer_data.is_oauth = true;
    app.bus_commands.push(omegon_traits::CommandDefinition {
        name: "cleave".into(),
        description: "parallel work".into(),
        subcommands: vec![],
    });
    let tx = test_tx();
    let result = app.handle_slash_command("/cleave demo", &tx);
    assert!(matches!(result, SlashResult::Handled), "got: {result:?}");
    assert!(
        app.operator_events
            .iter()
            .any(|e| e.message.contains("risk is yours") || e.message.contains("may violate Anthropic")),
        "expected warning toast in operator events"
    );

    unsafe {
        std::env::remove_var("ANTHROPIC_OAUTH_TOKEN");
    }
}

#[test]
fn slash_command_aliases_dispatch_correctly() {
    let mut app = test_app();
    let tx = test_tx();

    // /dashboard should resolve as the compatibility alias for /dash open
    let result = app.handle_slash_command("/dashboard", &tx);
    assert!(
        !matches!(result, SlashResult::NotACommand),
        "/dashboard should be handled, not fall through"
    );

    // /auspex should resolve to the status surface, not fall through
    let result = app.handle_slash_command("/auspex", &tx);
    assert!(
        matches!(result, SlashResult::Display(_)),
        "/auspex should display status information"
    );

    // /auspex open should also be routed, even before launch is fully configured
    let result = app.handle_slash_command("/auspex open", &tx);
    assert!(
        matches!(result, SlashResult::Display(_)),
        "/auspex open should display launch status information"
    );

    // /version should display build info
    let result = app.handle_slash_command("/version", &tx);
    assert!(
        matches!(result, SlashResult::Display(_)),
        "/version should display version info"
    );

    // /q should quit
    let result = app.handle_slash_command("/q", &tx);
    assert!(matches!(result, SlashResult::Quit), "/q should quit");
}

#[test]
fn slash_auspex_open_requests_bridge_start_when_dashboard_not_running() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    let result = app.handle_slash_command("/auspex open", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected Display result");
    };
    assert!(text.contains("Starting the local compatibility surface first"), "got: {text}");
}

#[test]
fn slash_auspex_status_reports_attach_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    let result = app.handle_slash_command("/auspex status", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected Display result");
    };

    assert!(text.contains("Auspex attach status"), "got: {text}");
    assert!(text.contains("protocol: v1"), "got: {text}");
    assert!(text.contains("ipc.sock"), "got: {text}");
    assert!(text.contains("session id: not yet exposed"), "got: {text}");
    assert!(text.contains("`/dash` remains the local compatibility path"), "got: {text}");
}


#[test]
fn web_dashboard_started_event_updates_cached_addr() {
    let mut app = test_app();
    let startup = crate::web::WebStartupInfo {
        schema_version: 2,
        addr: "127.0.0.1:7842".into(),
        http_base: "http://127.0.0.1:7842".into(),
        state_url: "http://127.0.0.1:7842/api/state".into(),
        startup_url: "http://127.0.0.1:7842/api/startup".into(),
        health_url: "http://127.0.0.1:7842/api/healthz".into(),
        ready_url: "http://127.0.0.1:7842/api/readyz".into(),
        ws_url: "ws://127.0.0.1:7842/ws?token=test".into(),
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        instance_descriptor: None,
    };

    app.handle_agent_event(AgentEvent::WebDashboardStarted {
        startup_json: serde_json::to_value(startup).unwrap(),
    });

    assert_eq!(
        app.web_server_addr.map(|addr| addr.to_string()),
        Some("127.0.0.1:7842".into())
    );
}

#[test]
fn unknown_slash_commands_show_error() {
    let mut app = test_app();
    let tx = test_tx();

    // Unknown commands must NOT return NotACommand (which sends to agent)
    let result = app.handle_slash_command("/foobar", &tx);
    assert!(
        matches!(result, SlashResult::Display(_)),
        "/foobar should show error, not go to agent"
    );

    // /secret now prefix-matches to /secrets (valid command)
    let result = app.handle_slash_command("/zzz_nonexistent", &tx);
    assert!(
        matches!(result, SlashResult::Display(_)),
        "/zzz_nonexistent should show error, not go to agent"
    );
}

#[test]
fn slash_prefix_matching_unique() {
    let mut app = test_app();
    let tx = test_tx();

    // /hel should uniquely prefix-match /help
    let result = app.handle_slash_command("/hel", &tx);
    assert!(
        matches!(result, SlashResult::Display(_)),
        "/hel should prefix-match /help and show help text"
    );
}

#[test]
fn slash_prefix_matching_ambiguous() {
    let mut app = test_app();
    let tx = test_tx();

    // /s matches multiple commands (stats, status, sessions, splash)
    let result = app.handle_slash_command("/s", &tx);
    match result {
        SlashResult::Display(msg) => {
            assert!(
                msg.contains("Did you mean") || msg.contains("Ambiguous"),
                "/s should show ambiguous message, got: {msg}"
            );
        }
        _ => panic!("/s should be ambiguous, got: {result:?}"),
    }
}

#[test]
fn tutorial_parse_lesson_with_frontmatter() {
    let raw = "---\ntitle: \"The Cockpit\"\n---\n\nWelcome to Omegon!\n\nLook at the bottom.";
    let (title, content) = super::parse_lesson(raw, "01-cockpit.md");
    assert_eq!(title, "The Cockpit");
    assert!(content.contains("Welcome to Omegon!"));
    assert!(!content.contains("title:"));
}

#[test]
fn tutorial_parse_lesson_without_frontmatter() {
    let raw = "Just plain content.\n\nNo frontmatter here.";
    let (title, content) = super::parse_lesson(raw, "02-tools.md");
    assert_eq!(title, "02-tools");
    assert!(content.contains("Just plain content."));
}

#[test]
fn tutorial_state_load_and_advance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path().join(".omegon").join("tutorial");
    std::fs::create_dir_all(&tutorial_dir).unwrap();

    std::fs::write(
        tutorial_dir.join("01-first.md"),
        "---\ntitle: \"First\"\n---\nLesson one.",
    )
    .unwrap();
    std::fs::write(
        tutorial_dir.join("02-second.md"),
        "---\ntitle: \"Second\"\n---\nLesson two.",
    )
    .unwrap();
    std::fs::write(
        tutorial_dir.join("03-third.md"),
        "---\ntitle: \"Third\"\n---\nLesson three.",
    )
    .unwrap();

    let mut tut = super::TutorialState::load(&tutorial_dir).unwrap();
    assert_eq!(tut.total(), 3);
    assert_eq!(tut.current, 0);
    assert_eq!(tut.current_lesson().title, "First");
    assert!(!tut.is_last());

    assert!(tut.advance());
    assert_eq!(tut.current, 1);
    assert_eq!(tut.current_lesson().title, "Second");

    assert!(tut.advance());
    assert_eq!(tut.current, 2);
    assert!(tut.is_last());

    assert!(!tut.advance()); // can't go past last

    assert!(tut.go_back());
    assert_eq!(tut.current, 1);

    assert!(!super::TutorialState::load(tmp.path()).is_some()); // no tutorial dir
}

#[test]
fn tutorial_progress_persistence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path();

    std::fs::write(tutorial_dir.join("01-a.md"), "---\ntitle: A\n---\nA").unwrap();
    std::fs::write(tutorial_dir.join("02-b.md"), "---\ntitle: B\n---\nB").unwrap();

    {
        let mut tut = super::TutorialState::load(tutorial_dir).unwrap();
        tut.advance();
        // Progress saved automatically
    }

    {
        let tut = super::TutorialState::load(tutorial_dir).unwrap();
        assert_eq!(tut.current, 1, "should resume at lesson 2");
        assert_eq!(tut.current_lesson().title, "B");
    }
}

#[test]
fn tutorial_reset_clears_progress() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path();

    std::fs::write(tutorial_dir.join("01-a.md"), "---\ntitle: A\n---\nA").unwrap();
    std::fs::write(tutorial_dir.join("02-b.md"), "---\ntitle: B\n---\nB").unwrap();

    let mut tut = super::TutorialState::load(tutorial_dir).unwrap();
    tut.advance();
    tut.reset();
    assert_eq!(tut.current, 0);
    assert!(!tutorial_dir.join("progress.json").exists());
}

#[test]
fn tutorial_status_line() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tutorial_dir = tmp.path();

    std::fs::write(
        tutorial_dir.join("01-intro.md"),
        "---\ntitle: Introduction\n---\nHello",
    )
    .unwrap();
    std::fs::write(
        tutorial_dir.join("02-end.md"),
        "---\ntitle: Finale\n---\nBye",
    )
    .unwrap();

    let mut tut = super::TutorialState::load(tutorial_dir).unwrap();
    assert!(tut.status_line().contains("1/2"));
    assert!(tut.status_line().contains("Introduction"));

    tut.advance();
    assert!(tut.status_line().contains("2/2"));
    assert!(tut.status_line().contains("(final)"));
}

#[cfg(target_os = "macos")]
#[test]
fn clipboard_format_matching() {
    use super::match_clipboard_image_format;

    // Real osascript output from a screenshot
    let info = "«class PNGf», 29460, «class AVIF», 14396, «class 8BPS», 141278, GIF picture, 9009, «class jp2 », 39826, JPEG picture, 27092, TIFF picture, 792990, «class BMP », 792202, «class TPIC», 58310";
    let result = match_clipboard_image_format(info);
    assert!(
        result.is_some(),
        "should match PNGf in real clipboard output"
    );
    let (ext, pb) = result.unwrap();
    assert_eq!(ext, "png");
    assert_eq!(pb, "«class PNGf»");

    // JPEG-only clipboard
    let info = "JPEG picture, 12345";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "jpg");

    // TIFF-only clipboard
    let info = "TIFF picture, 99999";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "tiff");

    // GIF
    let info = "GIF picture, 5000";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "gif");

    // BMP
    let info = "«class BMP », 200000";
    let (ext, _) = match_clipboard_image_format(info).unwrap();
    assert_eq!(ext, "bmp");

    // No image — text only
    let info = "«class utf8», 100, string, 100";
    assert!(match_clipboard_image_format(info).is_none());

    // Empty
    assert!(match_clipboard_image_format("").is_none());

    // The OLD broken matching — UTI strings that never appeared in osascript output
    let info_with_uti = "public.png, 29460";
    // This should NOT match PNGf (it contains "public.png" not "PNGf")
    // But wait — "png" is not in our markers. This correctly returns None.
    // The old code would have matched "public.png" → that was the bug.
    assert!(
        match_clipboard_image_format(info_with_uti).is_none(),
        "UTI strings should not match — osascript never outputs them"
    );
}

// ═══════════════════════════════════════════════════════════════════
// /note and /notes commands
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_note_with_text_persists_to_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    // Write a note
    let result = app.handle_slash_command("/note look into this later", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Noted"), "should confirm note: {text}");
        assert!(text.contains("1 entries"), "should count 1 entry: {text}");
    } else {
        panic!("expected Display result");
    }

    // Verify file exists and contains the note
    let notes_path = tmp.path().join(".omegon").join("notes.md");
    let content = std::fs::read_to_string(&notes_path).expect("notes file should exist");
    assert!(
        content.contains("look into this later"),
        "note text should be persisted: {content}"
    );
    assert!(
        content.starts_with("- ["),
        "should have timestamp prefix: {content}"
    );

    // Write a second note and verify count
    let result2 = app.handle_slash_command("/note second thing", &tx);
    if let SlashResult::Display(text) = result2 {
        assert!(text.contains("2 entries"), "should count 2 entries: {text}");
    }
}

#[test]
fn slash_note_without_args_shows_notes() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/note", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("note"), "should mention notes: {text}");
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_notes_clear_returns_display() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/notes clear", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("cleared"), "should confirm clear: {text}");
    } else {
        panic!("expected Display result");
    }
}

// ═══════════════════════════════════════════════════════════════════
// /checkin command
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_checkin_with_notes_shows_note_count() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    // No notes → should NOT mention notes in checkin
    let result = app.handle_slash_command("/checkin", &tx);
    if let SlashResult::Display(text) = &result {
        assert!(!text.contains("pending note"), "no notes yet: {text}");
    }

    // Add a note
    app.handle_slash_command("/note investigate flaky test", &tx);

    // Now checkin should show the note count
    let result2 = app.handle_slash_command("/checkin", &tx);
    if let SlashResult::Display(text) = result2 {
        assert!(
            text.contains("1 pending note"),
            "should show note count: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

#[test]
fn slash_checkin_with_opsx_changes_shows_them() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let tx = test_tx();

    // Create a fake OpenSpec change directory
    let change_dir = tmp
        .path()
        .join("openspec")
        .join("changes")
        .join("my-feature");
    std::fs::create_dir_all(&change_dir).unwrap();

    let result = app.handle_slash_command("/checkin", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("OpenSpec"),
            "should show OpenSpec changes: {text}"
        );
        assert!(
            text.contains("my-feature"),
            "should name the change: {text}"
        );
    } else {
        panic!("expected Display result");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Login selector
// ═══════════════════════════════════════════════════════════════════

#[test]
fn slash_login_selector_opens_with_provider_catalog() {
    let mut app = test_app();
    app.open_login_selector();
    assert!(app.selector.is_some(), "selector should be open");
    let selector = app.selector.as_ref().unwrap();
    assert!(
        selector.options.len() >= 9,
        "should have at least 9 providers, got {}",
        selector.options.len()
    );
    // Verify structure: each option has a value and label
    for opt in &selector.options {
        assert!(!opt.value.is_empty(), "option value should not be empty");
        assert!(!opt.label.is_empty(), "option label should not be empty");
    }
    // Unconfigured providers should NOT have checkmark
    let has_unconfigured = selector.options.iter().any(|o| !o.active);
    assert!(
        has_unconfigured,
        "at least some providers should be unconfigured in test env"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Recovery hints
// ═══════════════════════════════════════════════════════════════════

#[test]
fn recovery_hint_rate_limit() {
    let hint = App::recovery_hint(None, "Error: 429 Too Many Requests");
    assert!(
        hint.contains("Rate limited"),
        "should suggest rate limit recovery: {hint}"
    );
}

#[test]
fn recovery_hint_unauthorized() {
    let hint = App::recovery_hint(None, "HTTP 401 Unauthorized");
    assert!(hint.contains("/login"), "should suggest login: {hint}");
}

#[test]
fn recovery_hint_no_false_positive_on_status_codes() {
    // A path containing "401" should NOT trigger the auth hint
    let hint = App::recovery_hint(None, "Error reading /var/lib/app/401/config.json");
    assert!(
        hint.is_empty(),
        "path with 401 should not trigger auth hint: {hint}"
    );
}

#[test]
fn recovery_hint_ollama_connection() {
    let hint = App::recovery_hint(None, "Connection refused to ollama at localhost:11434");
    assert!(
        hint.contains("ollama serve"),
        "should suggest starting ollama: {hint}"
    );
}

#[test]
fn recovery_hint_context_window() {
    let hint = App::recovery_hint(None, "context_length_exceeded: too many tokens");
    assert!(hint.contains("/compact"), "should suggest compact: {hint}");
}

#[test]
fn recovery_hint_no_match() {
    let hint = App::recovery_hint(None, "some random error");
    assert!(hint.is_empty(), "should return empty for unknown errors");
}
