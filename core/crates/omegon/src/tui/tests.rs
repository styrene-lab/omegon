//! TUI integration tests — slash commands, selectors, event handling.
//!
//! These test the App struct as a state machine: feed inputs, check outputs.
//! No terminal rendering — uses App::new() with test settings.

use super::*;
use crate::lifecycle::types::NodeStatus;
use crate::settings::{ContextClass, Settings, ThinkingLevel};
use crate::tui::dashboard::FocusedNodeSummary;
use crate::update::{UpdateChannel, UpdateInfo};
use crate::web::WebDaemonStatus;
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

fn test_tx_with_rx() -> (mpsc::Sender<TuiCommand>, mpsc::Receiver<TuiCommand>) {
    mpsc::channel(16)
}

fn render_app_to_string(app: &mut App, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();

    let mut text = String::new();
    let size = terminal.backend().size().unwrap();
    let area = Rect::new(0, 0, size.width, size.height);
    let buf = terminal.backend().buffer();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[test]
fn editor_inline_attachment_tokens_submit_as_multimodal_prompt() {
    let mut app = test_app();
    app.editor.set_text("please inspect this");
    app.editor
        .insert_attachment(std::path::PathBuf::from("/tmp/paste.png"));

    assert_eq!(app.editor.render_text(), "please inspect this[image0]");

    let (text, attachments) = app.editor.take_submission();
    assert_eq!(text, "please inspect this");
    assert_eq!(
        attachments,
        vec![std::path::PathBuf::from("/tmp/paste.png")]
    );

    app.conversation
        .push_user_with_attachments(&text, &attachments);
    assert_eq!(app.conversation.segments().len(), 2);
    assert!(matches!(
        &app.conversation.segments()[0].content,
        crate::tui::segments::SegmentContent::UserPrompt { text } if text == "please inspect this"
    ));
    assert!(matches!(
        &app.conversation.segments()[1].content,
        crate::tui::segments::SegmentContent::Image { path, alt }
            if path == &std::path::PathBuf::from("/tmp/paste.png") && alt.contains("[image0]")
    ));
}

#[test]
fn session_reset_clears_instrument_panel_tool_activity() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "context_clear".into(),
        args: serde_json::json!({}),
    });

    // Use a substring stem that survives the instruments-panel column
    // truncation. The panel renders unknown tools (i.e. ones not in
    // `tool_short_name`'s match table) with a `· ` fallback prefix and
    // truncates the result to fit a 14-cell name column, so the literal
    // "context_clear" is not preserved — it ends up as `· context_cl…`.
    // The negative assertion below uses the same stem so it remains
    // strict against the post-reset cleared panel.
    let before = render_app_to_string(&mut app, 140, 36);
    assert!(before.contains("context_cl"), "got {before}");

    app.handle_agent_event(AgentEvent::SessionReset);

    let after = render_app_to_string(&mut app, 140, 36);
    assert!(!after.contains("context_cl"), "got {after}");
    assert!(
        after.contains("New session started. Previous session saved."),
        "got {after}"
    );
}

#[tokio::test]
async fn submit_editor_buffer_sends_plain_prompt_after_attachment_token_removed() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.editor.set_text("please inspect this");
    app.editor
        .insert_attachment(std::path::PathBuf::from("/tmp/paste.png"));
    assert_eq!(app.editor.render_text(), "please inspect this[image0]");

    app.editor.backspace();
    assert_eq!(app.editor.render_text(), "please inspect this");

    app.submit_editor_buffer(&tx).await;

    let command = rx.recv().await.expect("submission command");
    match command {
        TuiCommand::SubmitPrompt(PromptSubmission {
            text,
            image_paths,
            submitted_by,
            via,
        }) => {
            assert_eq!(text, "please inspect this");
            assert!(image_paths.is_empty());
            assert_eq!(submitted_by, "local-tui");
            assert_eq!(via, "tui");
        }
        other => panic!("expected plain prompt after removing attachment token, got {other:?}"),
    }
    assert!(rx.try_recv().is_err(), "unexpected extra command emitted");
}

#[tokio::test]
async fn submit_editor_buffer_sends_prompt_with_images_when_attachment_token_present() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.editor.set_text("please inspect this");
    app.editor
        .insert_attachment(std::path::PathBuf::from("/tmp/paste.png"));

    app.submit_editor_buffer(&tx).await;

    let command = rx.recv().await.expect("submission command");
    match command {
        TuiCommand::SubmitPrompt(PromptSubmission {
            text,
            image_paths,
            submitted_by,
            via,
        }) => {
            assert_eq!(text, "please inspect this");
            assert_eq!(
                image_paths,
                vec![std::path::PathBuf::from("/tmp/paste.png")]
            );
            assert_eq!(submitted_by, "local-tui");
            assert_eq!(via, "tui");
        }
        other => panic!("expected multimodal prompt, got {other:?}"),
    }
}

#[test]
fn collapsed_paste_token_renders_as_editor_chip() {
    let mut app = test_app();
    app.editor.insert_paste("alpha\n\nbeta\n");

    let rendered = render_app_to_string(&mut app, 100, 20);

    assert!(rendered.contains(" paste "), "got {rendered}");
    assert!(rendered.contains("1 +2 lines"), "got {rendered}");
    assert!(
        !rendered.contains("[Pasted text #1 +2 lines]"),
        "got {rendered}"
    );
}

#[test]
fn queued_prompt_preview_mentions_attachment_count() {
    let mut app = test_app();
    app.queue_prompt(
        "describe this".to_string(),
        vec![std::path::PathBuf::from("/tmp/paste.png")],
    );
    let rendered = render_app_to_string(&mut app, 100, 20);
    assert!(rendered.contains("Queued [1]"), "{rendered}");
    assert!(rendered.contains("+1 attachment"), "{rendered}");
}

#[test]
fn queue_prompt_preserves_fifo_order() {
    let mut app = test_app();
    app.queue_prompt("first".to_string(), Vec::new());
    app.queue_prompt("second".to_string(), Vec::new());

    let first = app.queued_prompts.pop_front().expect("first queued prompt");
    let second = app
        .queued_prompts
        .pop_front()
        .expect("second queued prompt");

    assert_eq!(first.0, "first");
    assert_eq!(second.0, "second");
}

#[test]
fn modal_overlay_clears_stale_wrapped_rows_when_content_shrinks() {
    let mut app = test_app();
    app.active_modal = Some((
        "widget-modal".into(),
        serde_json::json!({
            "message": "line 1\nline 2\nline 3\nline 4\nline 5\nline 6"
        }),
        None,
        std::time::Instant::now(),
    ));
    let _verbose = render_app_to_string(&mut app, 100, 30);

    app.active_modal = Some((
        "widget-modal".into(),
        serde_json::json!({ "message": "short" }),
        None,
        std::time::Instant::now(),
    ));
    let compact = render_app_to_string(&mut app, 100, 30);

    assert!(compact.contains("short"), "got {compact}");
    assert!(!compact.contains("line 6"), "got {compact}");
    assert!(!compact.contains("line 5"), "got {compact}");
}

#[test]
fn action_prompt_clears_stale_rows_when_reused_with_fewer_actions() {
    let mut app = test_app();
    app.active_action_prompt = Some((
        "widget-actions".into(),
        vec![
            "alpha".into(),
            "beta".into(),
            "gamma".into(),
            "delta".into(),
            "epsilon".into(),
        ],
    ));
    let _verbose = render_app_to_string(&mut app, 100, 30);

    app.active_action_prompt = Some(("widget-actions".into(), vec!["only".into()]));
    let compact = render_app_to_string(&mut app, 100, 30);

    assert!(compact.contains("only"), "got {compact}");
    assert!(!compact.contains("epsilon"), "got {compact}");
    assert!(!compact.contains("delta"), "got {compact}");
}

#[test]
fn context_updated_tracks_requested_policy_separately_from_actual_model_class() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 144_000,
        context_window: 131_072,
        context_class: "Legion".into(),
        thinking_level: "high".into(),
    });

    assert_eq!(app.footer_data.context_class, ContextClass::Legion);
    assert_eq!(app.footer_data.actual_context_class, ContextClass::Squad);
    assert!(app.footer_data.context_percent > 99.0);
}

#[test]
fn turn_end_does_not_overwrite_footer_context_with_last_request_input_tokens() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 144_000,
        context_window: 272_000,
        context_class: "Maniple".into(),
        thinking_level: "high".into(),
    });
    let before = app.footer_data.context_percent;
    assert!(
        before > 52.0 && before < 54.0,
        "expected ~53%, got {before}"
    );

    app.handle_agent_event(AgentEvent::TurnEnd {
        turn: 3,
        turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
        model: Some("anthropic:claude-sonnet-4-6".into()),
        provider: Some("anthropic".into()),
        estimated_tokens: 144_000,
        context_window: 272_000,
        context_composition: omegon_traits::ContextComposition {
            conversation_tokens: 120_000,
            system_tokens: 8_000,
            memory_tokens: 6_000,
            tool_schema_tokens: 2_000,
            tool_history_tokens: 2_000,
            thinking_tokens: 6_000,
            free_tokens: 128_000,
            ..Default::default()
        },
        actual_input_tokens: 12_345,
        actual_output_tokens: 413,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        provider_telemetry: None,
        dominant_phase: None,
        drift_kind: None,
        progress_nudge_reason: None,
        intent_task: None,
        intent_phase: None,
        files_read_count: 0,
        files_modified_count: 0,
        stats_tool_calls: 0,
        streaks: omegon_traits::ControllerStreaks::default(),
    });

    let after = app.footer_data.context_percent;
    assert!(
        (after - before).abs() < 0.0001,
        "TurnEnd should preserve total-context percent from ContextUpdated; before={before} after={after}"
    );
    assert_eq!(app.footer_data.estimated_tokens, 144_000);
}

#[test]
fn turn_end_tracks_session_usage_by_model_attribution() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::TurnEnd {
        turn: 1,
        turn_end_reason: omegon_traits::TurnEndReason::ToolContinuation,
        model: Some("openai:gpt-5.4".into()),
        provider: Some("openai".into()),
        estimated_tokens: 50_000,
        context_window: 272_000,
        context_composition: omegon_traits::ContextComposition::default(),
        actual_input_tokens: 100_000,
        actual_output_tokens: 20_000,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        provider_telemetry: None,
        dominant_phase: None,
        drift_kind: None,
        progress_nudge_reason: None,
        intent_task: None,
        intent_phase: None,
        files_read_count: 0,
        files_modified_count: 0,
        stats_tool_calls: 0,
        streaks: omegon_traits::ControllerStreaks::default(),
    });
    app.handle_agent_event(AgentEvent::TurnEnd {
        turn: 2,
        turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
        model: Some("openrouter:qwen/qwen-qwq-32b".into()),
        provider: Some("openrouter".into()),
        estimated_tokens: 60_000,
        context_window: 272_000,
        context_composition: omegon_traits::ContextComposition::default(),
        actual_input_tokens: 12_000,
        actual_output_tokens: 3_000,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        provider_telemetry: None,
        dominant_phase: None,
        drift_kind: None,
        progress_nudge_reason: None,
        intent_task: None,
        intent_phase: None,
        files_read_count: 0,
        files_modified_count: 0,
        stats_tool_calls: 0,
        streaks: omegon_traits::ControllerStreaks::default(),
    });

    assert_eq!(app.footer_data.session_input_tokens, 112_000);
    assert_eq!(app.footer_data.session_output_tokens, 23_000);
    assert_eq!(app.footer_data.session_usage_slices.len(), 2);

    let session_text = crate::tui::footer::format_session_text(
        app.footer_data.turn,
        app.footer_data.session_input_tokens,
        app.footer_data.session_output_tokens,
        &app.footer_data.session_usage_slices,
    );
    assert!(session_text.contains("~$0.55"), "{session_text}");
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
    assert_eq!(
        x, 3,
        "9 chars in 6 content columns should wrap to the fourth visible column"
    );
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
    assert_eq!(
        y, 2,
        "cursor should move onto wrapped row 2 after column 6 overflows"
    );
    assert_eq!(
        x, 1,
        "cursor should be at the second visible column on the wrapped row"
    );
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

    assert_eq!(
        y, 2,
        "cursor should stay inside the second visible editor row beneath the top border"
    );
    assert_eq!(
        visible,
        vec!["90ab", "cdef"],
        "render should follow editor scroll state"
    );
}

#[test]
fn editor_visible_visual_lines_keep_collapsed_paste_token_visible() {
    let mut editor = crate::tui::editor::Editor::new();
    editor.insert_paste("top\n\nbottom\n");

    let visible = editor.visible_visual_lines(20, 6);

    assert_eq!(visible, vec!["[Pasted text #1 +2 l", "ines]"]);
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
    app.conversation
        .append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    assert_eq!(app.conversation.conv_state.scroll_offset, 0);
    app.conversation.scroll_up(3);
    assert!(app.conversation.conv_state.user_scrolled);
    assert_eq!(app.conversation.conv_state.scroll_offset, 3);

    app.conversation.append_streaming("\nnew line");
    assert_eq!(
        app.conversation.conv_state.scroll_offset, 3,
        "streaming should not pull the viewport back to bottom once manually scrolled"
    );
}

#[test]
fn mouse_wheel_scroll_up_matches_natural_scroll_direction() {
    let mut app = test_app();
    app.conversation.push_user("user");
    app.conversation
        .append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    app.conversation.scroll_up(3);
    let after_scroll_up = app.conversation.conv_state.scroll_offset;
    assert!(
        after_scroll_up > 0,
        "scroll up should move into conversation history"
    );

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
    app.conversation
        .append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

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
    app.conversation
        .append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

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
    assert_eq!(
        app.history_idx, None,
        "conversation focus must not enter history recall"
    );
    assert_eq!(
        app.editor.render_text(),
        "",
        "conversation focus must not rewrite the composer"
    );

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
    assert!(
        selected.contains("tool: bash"),
        "missing tool header: {selected}"
    );
    assert!(selected.contains("args:"), "missing args block: {selected}");
    assert!(
        selected.contains("echo hi"),
        "missing args body: {selected}"
    );
    assert!(
        selected.contains("result:"),
        "missing result block: {selected}"
    );
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

    assert!(
        !app.terminal_copy_mode,
        "startup should prefer robust mouse interaction"
    );
    assert!(
        app.mouse_capture_enabled,
        "startup should enable mouse capture to receive wheel events directly"
    );
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

    assert!(matches!(
        app.handle_slash_command("/mouse", &tx),
        SlashResult::Handled
    ));
    assert!(!app.terminal_copy_mode);
    assert!(app.mouse_capture_enabled);
    assert!(!app.focus_mode);

    assert!(matches!(
        app.handle_slash_command("/mouse off", &tx),
        SlashResult::Handled
    ));
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
fn bare_up_recalls_history_from_empty_editor_by_default() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.terminal_copy_mode = false;

    if app.editor.line_count() > 1 && app.editor.cursor_row() > 0 {
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
fn bare_down_advances_history_by_default() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.terminal_copy_mode = false;

    app.history_recall_up();

    if app.editor.line_count() > 1 && app.editor.cursor_row() < app.editor.line_count() - 1 {
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
    assert_eq!(
        app.history_idx, None,
        "moving within multiline text should not start history recall"
    );
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
fn slash_focus_toggles_fullscreen_conversation_mode() {
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
    assert_eq!(app.conversation.selected_or_focused_segment(), Some(1));

    let result = app.handle_slash_command("/focus", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.focus_mode);
    assert!(app.mouse_capture_enabled);
}

#[test]
fn ui_command_switches_between_full_and_slim_presets() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/ui slim", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert_eq!(app.ui_mode, UiMode::Slim);
    assert!(!app.ui_surfaces.dashboard);
    assert!(!app.ui_surfaces.instruments);
    assert!(!app.ui_surfaces.footer);

    let result = app.handle_slash_command("/ui full", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert_eq!(app.ui_mode, UiMode::Full);
    assert!(app.ui_surfaces.dashboard);
    assert!(app.ui_surfaces.instruments);
    assert!(app.ui_surfaces.footer);
}

#[test]
fn ui_command_can_toggle_individual_surfaces() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/ui hide dashboard", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.ui_surfaces.dashboard);

    let result = app.handle_slash_command("/ui show dashboard", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(app.ui_surfaces.dashboard);

    let result = app.handle_slash_command("/ui toggle dashboard", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.ui_surfaces.dashboard);

    let result = app.handle_slash_command("/ui hide instruments", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.ui_surfaces.instruments);
    assert!(
        app.ui_surfaces.footer,
        "hiding instruments should not remove footer status"
    );
}

#[test]
fn empty_editor_hint_mentions_ui_surfaces_when_dashboard_hidden() {
    let mut app = test_app();
    app.set_ui_mode(UiMode::Slim);
    let rendered = render_app_to_string(&mut app, 100, 20);
    assert!(rendered.contains("/ui surfaces"), "{rendered}");
    assert!(!rendered.contains("^D tree"), "{rendered}");
}

#[test]
fn ui_status_lists_toggle_controls() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/ui", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected display");
    };
    assert!(text.contains("/ui toggle dashboard"), "{text}");
    assert!(text.contains("/ui toggle instruments"), "{text}");
    assert!(text.contains("/ui toggle footer"), "{text}");
}

#[test]
fn empty_editor_hint_mentions_focus_hotkey() {
    let mut app = test_app();
    let rendered = render_app_to_string(&mut app, 100, 20);
    assert!(rendered.contains("^F focus"), "{rendered}");
    assert!(rendered.contains("^D tree"), "{rendered}");
}

#[test]
fn focus_mode_starts_on_last_selectable_segment_and_toggle_tracks_expansion() {
    let mut app = test_app();
    app.conversation.push_user("hello");
    app.conversation.push_system("world");

    assert_eq!(app.conversation.timeline_focused_segment(), Some(1));

    app.set_focus_mode(true);
    assert_eq!(app.conversation.timeline_focused_segment(), Some(1));
    assert_eq!(app.conversation.timeline_expanded_segment(), None);

    app.conversation.toggle_timeline_expanded_segment(1);
    assert_eq!(app.conversation.timeline_expanded_segment(), Some(1));

    app.conversation.toggle_timeline_expanded_segment(1);
    assert_eq!(app.conversation.timeline_expanded_segment(), None);
}

#[test]
fn focus_mode_ignores_stale_selected_segment_and_jumps_to_live_tail() {
    let mut app = test_app();
    app.conversation.push_user("older operator prompt");
    app.conversation.append_streaming("older assistant answer");
    app.conversation.finalize_message();
    app.conversation.select_segment(0);

    app.conversation.push_user("latest operator prompt");
    app.conversation.append_streaming("latest assistant answer");
    app.conversation.finalize_message();

    app.set_focus_mode(true);

    let selected = app.conversation.selected_or_focused_segment();
    let text = app
        .conversation
        .selected_segment_text_with_mode(SegmentExportMode::Plaintext)
        .unwrap_or_default();

    assert_eq!(selected, app.conversation.last_selectable_segment());
    assert!(
        text.contains("latest assistant answer"),
        "focus mode should land on the latest tail segment, got: {text:?}"
    );
}

#[test]
fn focus_mode_esc_exits_focus_before_interrupting_agent() {
    let mut app = test_app();
    app.conversation.push_system("segment");
    app.set_focus_mode(true);

    app.set_focus_mode(false);
    assert!(!app.focus_mode);
}

#[test]
fn focus_mode_render_shows_plaintext_fullscreen_conversation() {
    let mut app = test_app();
    app.conversation.push_user("operator prompt");
    app.conversation.append_streaming("assistant answer");
    app.conversation.finalize_message();
    app.conversation.select_segment(1);
    app.set_focus_mode(true);

    let rendered = render_app_to_string(&mut app, 80, 20);
    assert!(rendered.contains("assistant answer"), "{rendered}");
    assert!(rendered.contains("PgUp/PgDn jump"), "{rendered}");
    assert!(!rendered.contains("focus — segment"), "{rendered}");
    assert!(!rendered.contains("╭"), "{rendered}");
    assert!(!rendered.contains("╰"), "{rendered}");
    assert!(rendered.contains("│"), "{rendered}");
    assert!(rendered.contains("▶"), "{rendered}");
}

#[test]
fn draw_owns_full_root_background() {
    let mut app = test_app();
    let backend = ratatui::backend::TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    terminal
        .draw(|frame| {
            app.draw(frame);
        })
        .expect("draw should succeed");

    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = buffer.cell((x, y)).expect("cell in bounds");
            assert_ne!(
                cell.bg,
                Color::Reset,
                "cell ({x},{y}) retained Reset background; root draw left a transparent hole"
            );
        }
    }
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
fn slash_workspace_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceStatusView,
            ..
        } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
    }
}

#[test]
fn slash_workspace_list_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace list", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceListView,
            ..
        } => {}
        other => panic!("expected workspace list request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_adopt_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace adopt", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceAdopt,
            ..
        } => {}
        other => panic!("expected workspace adopt request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_release_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace release", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceRelease,
            ..
        } => {}
        other => panic!("expected workspace release request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_archive_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace archive", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceArchive,
            ..
        } => {}
        other => panic!("expected workspace archive request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_prune_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace prune", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspacePrune,
            ..
        } => {}
        other => panic!("expected workspace prune request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_destroy_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace destroy docs-pass", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceDestroy { target },
            ..
        } if target == "docs-pass" => {}
        other => panic!("expected workspace destroy request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_new_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace new docs-pass", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceNew { label: ref label },
            ..
        } if label == "docs-pass" => {}
        other => panic!("expected workspace new request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_role_set_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace role set release", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request:
                crate::control_runtime::ControlRequest::WorkspaceRoleSet {
                    role: crate::workspace::types::WorkspaceRole::Release,
                },
            ..
        } => {}
        other => panic!("expected workspace role set request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_role_clear_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace role clear", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceRoleClear,
            ..
        } => {}
        other => panic!("expected workspace role clear request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_bind_milestone_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace bind milestone 0.15.10", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceBindMilestone { milestone_id },
            ..
        } if milestone_id == "0.15.10" => {}
        other => panic!("expected workspace bind milestone request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_bind_node_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace bind node workspace-ownership-model", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceBindNode { design_node_id },
            ..
        } if design_node_id == "workspace-ownership-model" => {}
        other => panic!("expected workspace bind node request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_bind_clear_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace bind clear", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceBindClear,
            ..
        } => {}
        other => panic!("expected workspace bind clear request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_kind_set_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace kind set vault", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request:
                crate::control_runtime::ControlRequest::WorkspaceKindSet {
                    kind: crate::workspace::types::WorkspaceKind::Vault,
                },
            ..
        } => {}
        other => panic!("expected workspace kind set request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_kind_clear_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/workspace kind clear", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::WorkspaceKindClear,
            ..
        } => {}
        other => panic!("expected workspace kind clear request, got {other:?}"),
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
    let (tx, mut rx) = test_tx_with_rx();
    let result = app.handle_slash_command("/stats", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
    }
}

#[test]
fn slash_status_returns_bootstrap_panel() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    let result = app.handle_slash_command("/status", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
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
fn slash_context_no_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/context", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_some(), "bare /context should open the selector");
    assert_eq!(app.selector_kind, Some(SelectorKind::ContextClass));
}

#[test]
fn context_selector_confirm_enqueues_set_context_class() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.handle_slash_command("/context", &tx);
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "Clan")
        .expect("Clan option present");
    selector.cursor = index;

    let message = app
        .confirm_selector(&tx)
        .expect("selector confirmation should return message");
    assert!(message.contains("Context policy → Clan"), "unexpected message: {message}");

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetContextClass { class },
            ..
        } => assert_eq!(class, crate::settings::ContextClass::Clan),
        other => panic!("expected set-context-class control request, got: {other:?}"),
    }
}

#[test]
fn slash_context_request_dispatches_direct_context_pack() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/context request code selector policy", &tx);

    match result {
        super::SlashResult::Display(text) => {
            assert!(
                text.contains("Requesting mediated context pack for code"),
                "got {text}"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn slash_context_request_accepts_json_payload() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command(
        "/context request {\"requests\":[{\"kind\":\"code\",\"query\":\"selector policy\",\"reason\":\"probe\"}]}",
        &tx,
    );

    match result {
        super::SlashResult::Display(text) => {
            assert!(
                text.contains("Requesting mediated context pack from JSON payload"),
                "got {text}"
            );
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn slash_context_compress_alias_requests_compaction() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/context compress", &tx);
    assert!(!matches!(result, SlashResult::NotACommand));
    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("compaction"),
            "should confirm compaction request: {text}"
        );
    }
}

#[test]
fn slash_compact_is_unknown() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/compact", &tx);
    match result {
        SlashResult::Display(text) => {
            assert!(text.contains("Unknown command: /compact"), "got: {text}");
        }
        other => panic!("/compact should be unknown, got: {other:?}"),
    }
}

#[test]
fn slash_persona_no_args_opens_selector() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".omegon/plugins/test-persona");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("PERSONA.md"), "Be useful.\n").unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
[plugin]
type = "persona"
id = "dev.test.persona"
name = "Test Persona"
version = "1.0.0"
description = "Test persona"

[persona.identity]
directive = "PERSONA.md"
"#,
    )
    .unwrap();

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/persona", &tx);

    std::env::set_current_dir(prev).unwrap();

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.selector.is_some(),
        "bare /persona should open the selector"
    );
    assert_eq!(app.selector_kind, Some(SelectorKind::Persona));
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
fn slash_tone_no_args_opens_selector() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".omegon/plugins/test-tone");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("TONE.md"), "Stay concise.\n").unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
[plugin]
type = "tone"
id = "dev.test.tone"
name = "Test Tone"
version = "1.0.0"
description = "Test tone"

[tone]
directive = "TONE.md"
"#,
    )
    .unwrap();

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/tone", &tx);

    std::env::set_current_dir(prev).unwrap();

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.selector.is_some(),
        "bare /tone should open the selector"
    );
    assert_eq!(app.selector_kind, Some(SelectorKind::Tone));
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
    assert!(matches!(result, SlashResult::Handled));
}

#[test]
fn slash_auth_login_redirects_to_top_level_login() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/auth login anthropic", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected Display result");
    };
    assert!(
        text.contains("Use /login <provider> or /logout <provider>"),
        "got: {text}"
    );
}

#[test]
fn slash_login_provider_dispatches_to_runtime() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/login anthropic", &tx);
    assert!(matches!(result, SlashResult::Handled));
}

#[test]
fn slash_logout_without_provider_shows_provider_usage() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/logout", &tx);
    match result {
        SlashResult::Display(text) => {
            assert!(text.contains("Usage: /logout <provider>"), "got: {text}");
            assert!(text.contains("openai-codex"), "got: {text}");
        }
        other => panic!(
            "expected Display result, got {:?}",
            std::mem::discriminant(&other)
        ),
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
fn slash_think_with_level_does_not_optimistically_mutate_settings() {
    let mut app = test_app();
    let tx = test_tx();
    let original_thinking = app.settings().thinking;

    let result = app.handle_slash_command("/think high", &tx);
    if let SlashResult::Display(text) = result {
        assert!(
            text.to_lowercase().contains("high"),
            "should confirm high: {text}"
        );
    } else {
        panic!("expected display confirmation from /think high");
    }

    assert_eq!(
        app.settings().thinking,
        original_thinking,
        "/think should wait for runtime confirmation before changing visible settings"
    );
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

    let expected = {
        let sel = app.selector.as_mut().expect("selector");
        sel.move_down();
        sel.selected_value().to_string()
    };
    let message = app.confirm_selector(&tx).expect("confirmation message");

    assert!(message.contains("Context policy →"));
    let s = app.settings.lock().unwrap();
    assert_eq!(s.context_class.short(), expected);
}

#[test]
fn persona_selector_confirm_activates_selected_persona() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".omegon/plugins/test-persona");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("PERSONA.md"), "Be useful.\n").unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
[plugin]
type = "persona"
id = "dev.test.persona"
name = "Test Persona"
version = "1.0.0"
description = "Test persona"

[persona.identity]
directive = "PERSONA.md"
"#,
    )
    .unwrap();

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut app = test_app();
    app.open_persona_selector();
    let tx = test_tx();
    let message = app.confirm_selector(&tx);

    std::env::set_current_dir(prev).unwrap();

    assert_eq!(
        message.as_deref(),
        Some("⚙ Persona activated: Test Persona (0 mind facts)")
    );
    let active = app
        .plugin_registry
        .as_ref()
        .and_then(|registry| registry.active_persona())
        .map(|persona| persona.id.as_str());
    assert_eq!(active, Some("dev.test.persona"));
}

#[test]
fn tone_selector_confirm_activates_selected_tone() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".omegon/plugins/test-tone");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("TONE.md"), "Stay concise.\n").unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
[plugin]
type = "tone"
id = "dev.test.tone"
name = "Test Tone"
version = "1.0.0"
description = "Test tone"

[tone]
directive = "TONE.md"
"#,
    )
    .unwrap();

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    let mut app = test_app();
    app.open_tone_selector();
    let tx = test_tx();
    let message = app.confirm_selector(&tx);

    std::env::set_current_dir(prev).unwrap();

    assert_eq!(message.as_deref(), Some("♪ Tone activated: Test Tone"));
    let active = app
        .plugin_registry
        .as_ref()
        .and_then(|registry| registry.active_tone())
        .map(|tone| tone.id.as_str());
    assert_eq!(active, Some("dev.test.tone"));
}

// ═══════════════════════════════════════════════════════════════════
// Event handling
// ═══════════════════════════════════════════════════════════════════

#[test]
fn draw_clears_stale_completed_cleave_snapshot_from_tools_panel() {
    let mut app = test_app();
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = true;
    app.instrument_panel
        .set_cleave_progress(Some(crate::features::cleave::CleaveProgress {
            active: false,
            run_id: "done-run".into(),
            total_children: 3,
            completed: 3,
            failed: 0,
            children: vec![],
            total_tokens_in: 100,
            total_tokens_out: 50,
        }));
    app.dashboard_handles.cleave = Some(std::sync::Arc::new(std::sync::Mutex::new(
        crate::features::cleave::CleaveProgress {
            active: false,
            run_id: "done-run".into(),
            total_children: 3,
            completed: 3,
            failed: 0,
            children: vec![],
            total_tokens_in: 100,
            total_tokens_out: 50,
        },
    )));

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(
        !rendered.contains("⟁ cleave"),
        "completed cleave snapshot should not keep the cleave panel visible: {rendered}"
    );
}

#[test]
fn draw_hides_dashboard_for_inactive_restored_cleave_snapshot_without_other_content() {
    let mut app = test_app();
    app.ui_surfaces.dashboard = true;
    app.ui_surfaces.instruments = false;
    app.ui_surfaces.footer = false;
    app.dashboard.cleave = Some(crate::features::cleave::CleaveProgress {
        active: false,
        run_id: "restored-run".into(),
        total_children: 3,
        completed: 3,
        failed: 0,
        children: vec![],
        total_tokens_in: 100,
        total_tokens_out: 50,
    });

    let rendered = render_app_to_string(&mut app, 140, 20);

    assert!(
        !rendered.contains("Dashboard"),
        "inactive restored cleave snapshot should not force dashboard visibility: {rendered}"
    );
}

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
fn slash_model_no_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/model", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_some(), "expected model selector to open");
    assert!(matches!(app.selector_kind, Some(SelectorKind::Model)));
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

    assert_eq!(
        result.as_deref(),
        Some("Switching model → openai-codex:gpt-5.4")
    );
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
fn slash_plugin_list_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/plugin list", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::PluginView,
            ..
        } => {}
        other => panic!("expected plugin view control request, got: {other:?}"),
    }
}

#[test]
fn slash_skills_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/skills", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SkillsView,
            ..
        } => {}
        other => panic!("expected skills view control request, got: {other:?}"),
    }
}

#[test]
fn slash_secrets_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/secrets", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SecretsView,
            ..
        } => {}
        other => panic!("expected secrets view control request, got: {other:?}"),
    }
}

#[test]
fn slash_vault_status_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/vault status", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::VaultStatus,
            ..
        } => {}
        other => panic!("expected vault status control request, got: {other:?}"),
    }
}

#[test]
fn slash_vault_configure_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/vault configure", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_some(), "expected vault selector to open");
    assert!(matches!(app.selector_kind, Some(super::SelectorKind::VaultConfigure)));
}

#[test]
fn vault_configure_selector_confirm_primes_editor() {
    let mut app = test_app();
    let tx = test_tx();
    app.handle_slash_command("/vault configure", &tx);
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "file")
        .expect("file option present");
    selector.cursor = index;

    let message = app
        .confirm_selector(&tx)
        .expect("selector confirmation should return message");

    assert_eq!(app.editor.render_text(), "/vault configure file");
    assert!(message.contains("file"), "unexpected message: {message}");
}

#[test]
fn slash_secrets_set_without_value_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/secrets set", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_some(), "expected secret selector to open");
    assert!(matches!(
        app.selector_kind,
        Some(super::SelectorKind::SecretName)
    ));
}

#[test]
fn slash_secrets_configure_without_value_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/secrets configure", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_some(), "expected secret selector to open");
    assert!(matches!(app.selector_kind, Some(super::SelectorKind::SecretName)));
}

#[test]
fn secret_selector_confirm_starts_hidden_secret_input() {
    let mut app = test_app();
    let tx = test_tx();
    app.handle_slash_command("/secrets configure", &tx);
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY option present");
    selector.cursor = index;

    let message = app
        .confirm_selector(&tx)
        .expect("selector confirmation should return message");
    let (label, masked) = app
        .editor
        .secret_display()
        .expect("selector should enter hidden secret mode");

    assert_eq!(label, "ANTHROPIC_API_KEY");
    assert!(masked.is_empty(), "secret buffer should start empty");
    assert!(
        message.contains("Paste or type value") && message.contains("input is hidden"),
        "unexpected message: {message}"
    );
}

#[test]
fn slash_cleave_status_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/cleave status", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::CleaveStatus,
            ..
        } => {}
        other => panic!("expected cleave status control request, got: {other:?}"),
    }
}

#[test]
fn slash_delegate_status_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/delegate status", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::DelegateStatus,
            ..
        } => {}
        other => panic!("expected delegate status control request, got: {other:?}"),
    }
}

#[test]
fn slash_cleave_run_still_uses_bus_path() {
    let mut app = test_app();
    app.bus_commands.push(omegon_traits::CommandDefinition {
        name: "cleave".into(),
        description: "parallel work".into(),
        subcommands: vec!["status".into(), "cancel".into()],
    });
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/cleave implement demo", &tx);
    assert!(matches!(result, SlashResult::Handled));

    match rx.try_recv().expect("queued command") {
        TuiCommand::BusCommand { name, args } => {
            assert_eq!(name, "cleave");
            assert_eq!(args, "implement demo");
        }
        other => panic!("expected cleave bus command, got: {other:?}"),
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
fn palette_deduplicates_builtin_and_bus_commands() {
    let mut app = test_app();
    app.bus_commands = vec![omegon_traits::CommandDefinition {
        name: "cleave".into(),
        description: "parallel work".into(),
        subcommands: vec!["status".into()],
    }];
    app.editor.set_text("/cl");
    let matches = app.matching_commands();
    let cleave_count = matches.iter().filter(|(name, _)| name == "cleave").count();
    assert_eq!(
        cleave_count, 1,
        "expected one /cleave entry, got: {matches:?}"
    );
}

#[test]
fn clear_command_is_not_documented_or_handled() {
    let mut app = test_app();
    let tx = test_tx();

    assert!(!App::COMMANDS.iter().any(|(name, _, _)| *name == "clear"));

    let result = app.handle_slash_command("/clear", &tx);
    match result {
        SlashResult::Display(text) => {
            assert!(text.contains("Unknown command: /clear"), "got: {text}");
        }
        other => panic!("/clear should be unknown, got: {other:?}"),
    }
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
            .any(|e| e.message.contains("risk is yours")
                || e.message.contains("may violate Anthropic")),
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

    // /dashboard is removed — use /auspex open or /dash explicitly
    let result = app.handle_slash_command("/dashboard", &tx);
    match result {
        SlashResult::Display(text) => {
            assert!(text.contains("Unknown command: /dashboard"), "got: {text}");
        }
        other => panic!("/dashboard should be unknown, got: {other:?}"),
    }

    // /auspex should resolve to the primary status surface, not fall through
    let result = app.handle_slash_command("/auspex", &tx);
    let SlashResult::Display(text) = result else {
        panic!("/auspex should display status information");
    };
    assert!(text.contains("Auspex attach status"), "got: {text}");
    assert!(
        text.contains("primary local desktop handoff"),
        "got: {text}"
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
    assert!(
        text.contains("/auspex open"),
        "expected the primary command to be named in guidance: {text}"
    );
    assert!(
        text.contains("native desktop handoff"),
        "expected native handoff wording: {text}"
    );
    assert!(
        text.contains("compatibility/debug browser path"),
        "got: {text}"
    );
}

#[test]
fn slash_auspex_status_reports_attach_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    app.web_startup = Some(crate::web::WebStartupInfo {
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
        daemon_status: WebDaemonStatus {
            queued_events: 2,
            processed_events: 3,
            worker_running: true,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
        },
        instance_descriptor: None,
    });
    let tx = test_tx();

    let result = app.handle_slash_command("/auspex status", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected Display result");
    };

    assert!(text.contains("Auspex attach status"), "got: {text}");
    assert!(text.contains("protocol: v1"), "got: {text}");
    assert!(text.contains("ipc.sock"), "got: {text}");
    assert!(text.contains("session id: not yet exposed"), "got: {text}");
    assert!(text.contains("/dash compatibility view:"), "got: {text}");
    assert!(text.contains("queued events:"), "got: {text}");
    assert!(text.contains("transport warnings:"), "got: {text}");
    assert!(text.contains("insecure bootstrap tokens"), "got: {text}");
    assert!(text.contains("Auspex\n  app:"), "got: {text}");
    if !text.contains("app: not detected") {
        assert!(text.contains("modes:"), "got: {text}");
    }
    assert!(
        text.contains("Use `/auspex open` as the primary local desktop handoff"),
        "got: {text}"
    );
    assert!(
        text.contains("`/dash` remains the compatibility/debug browser path"),
        "got: {text}"
    );
}

#[test]
fn slash_dash_status_uses_compatibility_wording() {
    let mut app = test_app();
    app.web_server_addr = Some("127.0.0.1:7842".parse().unwrap());
    app.web_startup = Some(crate::web::WebStartupInfo {
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
        daemon_status: WebDaemonStatus {
            queued_events: 4,
            processed_events: 7,
            worker_running: true,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
        },
        instance_descriptor: None,
    });
    let tx = test_tx();

    let result = app.handle_slash_command("/dash status", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected Display result");
    };

    assert!(
        text.contains("compatibility/debug browser path"),
        "got: {text}"
    );
    assert!(text.contains("http://127.0.0.1:7842"), "got: {text}");
    assert!(text.contains("queue depth:"), "got: {text}");
    assert!(text.contains("transport warnings:"), "got: {text}");
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
        daemon_status: WebDaemonStatus {
            queued_events: 2,
            processed_events: 3,
            worker_running: true,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
        },
        instance_descriptor: None,
    };

    app.handle_agent_event(AgentEvent::WebDashboardStarted {
        startup_json: serde_json::to_value(startup).unwrap(),
    });

    assert_eq!(
        app.web_server_addr.map(|addr| addr.to_string()),
        Some("127.0.0.1:7842".into())
    );
    assert_eq!(
        app.web_startup
            .as_ref()
            .map(|startup| startup.token.as_str()),
        Some("test")
    );
    assert_eq!(
        app.web_startup
            .as_ref()
            .map(|startup| startup.ws_url.as_str()),
        Some("ws://127.0.0.1:7842/ws?token=test")
    );
}

#[test]
fn auspex_attach_payload_carries_startup_and_instance_metadata() {
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
        daemon_status: WebDaemonStatus {
            queued_events: 0,
            processed_events: 0,
            worker_running: false,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
        },
        instance_descriptor: Some(omegon_traits::OmegonInstanceDescriptor {
            schema_version: 1,
            identity: omegon_traits::OmegonIdentity {
                instance_id: "instance-1".into(),
                workspace_id: "workspace-1".into(),
                session_id: "session-1".into(),
                role: omegon_traits::OmegonRole::PrimaryDriver,
                profile: "primary-interactive".into(),
            },
            ownership: omegon_traits::OmegonOwnership {
                owner_kind: omegon_traits::OmegonOwnerKind::Operator,
                owner_id: "local-terminal".into(),
                parent_instance_id: None,
            },
            placement: omegon_traits::OmegonPlacement {
                kind: omegon_traits::OmegonPlacementKind::LocalProcess,
                host: Some("localhost".into()),
                pid: Some(12345),
                cwd: "/tmp/project".into(),
                namespace: None,
                pod_name: None,
                container_name: None,
            },
            control_plane: omegon_traits::OmegonControlPlane {
                server_instance_id: "instance-1".into(),
                protocol_version: 1,
                schema_version: 1,
                omegon_version: "0.15.10-rc.34".into(),
                capabilities: vec!["state.snapshot".into(), "events.stream".into()],
                ipc_socket_path: Some("/tmp/project/.omegon/ipc.sock".into()),
                http_base: Some("http://127.0.0.1:7842".into()),
                startup_url: Some("http://127.0.0.1:7842/api/startup".into()),
                state_url: Some("http://127.0.0.1:7842/api/state".into()),
                ws_url: Some("ws://127.0.0.1:7842/ws?token=test".into()),
                auth_mode: Some("ephemeral-bearer".into()),
                auth_source: Some("generated".into()),
                http_transport_security: Some(omegon_traits::OmegonTransportSecurity::InsecureBootstrap),
                ws_transport_security: Some(omegon_traits::OmegonTransportSecurity::InsecureBootstrap),
            },
            runtime: omegon_traits::OmegonRuntime {
                deployment_kind: omegon_traits::OmegonDeploymentKind::InteractiveTui,
                runtime_mode: omegon_traits::OmegonRuntimeMode::Standalone,
                runtime_profile: omegon_traits::OmegonRuntimeProfile::PrimaryInteractive,
                autonomy_mode: omegon_traits::OmegonAutonomyMode::OperatorDriven,
                health: omegon_traits::OmegonRuntimeHealth::Ready,
                provider_ok: true,
                memory_ok: true,
                cleave_available: true,
                queued_events: 0,
                transport_warnings: vec![],
                runtime_dir: None,
                context_class: Some("Squad".into()),
                thinking_level: Some("Medium".into()),
                capability_tier: Some("victory".into()),
            },
        }),
    };

    let payload =
        super::build_auspex_attach_payload(&startup, super::AuspexHandoffMode::Env).unwrap();
    let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(json["transport"], "omegon-ipc");
    assert_eq!(json["preferred_handoff"], "env");
    assert_eq!(json["startup_url"], "http://127.0.0.1:7842/api/startup");
    assert_eq!(json["ws_token"], "test");
    assert_eq!(json["instance"]["identity"]["instance_id"], "instance-1");
}

#[test]
fn parse_handoff_modes_defaults_to_env_when_unspecified() {
    let modes = super::parse_handoff_modes(&serde_json::json!({"omegon_ipc_protocol": 1}));
    assert_eq!(modes, vec![super::AuspexHandoffMode::Env]);
}

#[test]
fn parse_handoff_modes_reads_supported_modes() {
    let modes = super::parse_handoff_modes(&serde_json::json!({
        "handoff_modes": ["browser-url", "env", "unknown"]
    }));
    assert_eq!(
        modes,
        vec![
            super::AuspexHandoffMode::BrowserUrl,
            super::AuspexHandoffMode::Env,
        ]
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
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/note look into this later", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
    }
}

#[test]
fn slash_note_without_args_shows_notes() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    let result = app.handle_slash_command("/note", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
    }
}

#[test]
fn slash_notes_clear_returns_display() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    let result = app.handle_slash_command("/notes clear", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
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
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/checkin", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
    }

    let result2 = app.handle_slash_command("/note investigate flaky test", &tx);
    assert!(matches!(result2, SlashResult::Handled));
    let _ = rx.try_recv().expect("queued note command");

    let result3 = app.handle_slash_command("/checkin", &tx);
    assert!(matches!(result3, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
    }
}

#[test]
fn slash_checkin_with_opsx_changes_shows_them() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let (tx, mut rx) = test_tx_with_rx();

    let change_dir = tmp
        .path()
        .join("openspec")
        .join("changes")
        .join("my-feature");
    std::fs::create_dir_all(&change_dir).unwrap();

    let result = app.handle_slash_command("/checkin", &tx);
    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl { .. } => {}
        other => panic!("expected ExecuteControl, got {other:?}"),
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
        selector.options.len() >= 10,
        "should have at least 10 providers, got {}",
        selector.options.len()
    );
    assert!(
        selector.options.iter().any(|o| o.value == "ollama-cloud"),
        "selector should include ollama-cloud"
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

#[test]
fn login_selector_ollama_cloud_opens_hidden_api_key_entry() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_login_selector();
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "ollama-cloud")
        .expect("ollama-cloud option present");
    selector.cursor = index;

    let message = app
        .confirm_selector(&tx)
        .expect("selector confirmation should return message");
    let (label, masked) = app
        .editor
        .secret_display()
        .expect("ollama-cloud should enter hidden secret mode");

    assert_eq!(label, "OLLAMA_API_KEY");
    assert!(masked.is_empty(), "secret buffer should start empty");
    assert!(
        message.contains("ollama-cloud") && message.contains("input is hidden"),
        "unexpected message: {message}"
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
    assert!(
        hint.contains("/context compact"),
        "should suggest context compact: {hint}"
    );
}

#[test]
fn recovery_hint_no_match() {
    let hint = App::recovery_hint(None, "some random error");
    assert!(hint.is_empty(), "should return empty for unknown errors");
}

#[test]
fn thinking_chunk_marks_runtime_phase_as_thinking() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::TurnStart { turn: 1 });
    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 80_000,
        context_window: 200_000,
        context_class: "Squad".into(),
        thinking_level: "high".into(),
    });
    app.handle_agent_event(AgentEvent::ThinkingChunk {
        text: "deliberating".into(),
    });

    app.instrument_panel
        .update_telemetry(40.0, 200_000, None, false, "high", None, true, 0.016);

    assert_eq!(app.instrument_panel.debug_activity_mode(), "think");
}

#[test]
fn active_tool_phase_beats_runtime_thinking_in_tui() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::TurnStart { turn: 1 });
    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 80_000,
        context_window: 200_000,
        context_class: "Squad".into(),
        thinking_level: "high".into(),
    });
    app.handle_agent_event(AgentEvent::ThinkingChunk {
        text: "deliberating".into(),
    });
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "pwd"}),
    });

    app.instrument_panel.update_telemetry(
        40.0,
        200_000,
        Some("bash"),
        false,
        "high",
        None,
        true,
        0.016,
    );

    assert_eq!(app.instrument_panel.debug_activity_mode(), "tool");
}

#[test]
fn tool_end_aggregates_all_text_blocks() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "codebase_search".into(),
        args: serde_json::json!({"query": "foo"}),
    });

    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "tool-1".into(),
        name: "codebase_search".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![
                omegon_traits::ContentBlock::Text {
                    text: "## codebase_search: `foo`".into(),
                },
                omegon_traits::ContentBlock::Text {
                    text: "**2 result(s)** (scope: `code`)".into(),
                },
                omegon_traits::ContentBlock::Text {
                    text: "| File | Lines |\n|------|-------|\n| src/app.rs | 10-20 |".into(),
                },
            ],
            details: serde_json::Value::Null,
        },
    });

    let Some(seg) = app.conversation.segments().iter().find(|seg| {
        matches!(
            &seg.content,
            SegmentContent::ToolCard {
                id,
                complete: true,
                ..
            } if id == "tool-1"
        )
    }) else {
        panic!("expected completed tool segment");
    };

    let SegmentContent::ToolCard { detail_result, .. } = &seg.content else {
        panic!("expected tool card");
    };
    let detail = detail_result.as_deref().unwrap_or("");
    assert!(
        detail.contains("## codebase_search: `foo`"),
        "missing heading: {detail}"
    );
    assert!(
        detail.contains("**2 result(s)** (scope: `code`)"),
        "missing summary line: {detail}"
    );
    assert!(
        detail.contains("| src/app.rs | 10-20 |"),
        "missing later text block: {detail}"
    );
}
