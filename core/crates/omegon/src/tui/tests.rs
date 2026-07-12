//! TUI integration tests — slash commands, selectors, event handling.
//!
//! These test the App struct as a state machine: feed inputs, check outputs.
//! No terminal rendering — uses App::new() with test settings.

use super::menu_surface::{MenuMode, MenuState};
use super::settings_menu::build_model_selector_options;
use super::workbench::{PlanDisplayItem, PlanDisplayStatus, SlimTurnState};
use super::*;
use crate::settings::{ContextClass, Settings, ThinkingLevel};
use crate::tui::segments::Segment;
use crate::tui::theme::Theme;
use crate::update::UpdateInfo;
use crate::web::WebDaemonStatus;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

struct CurrentDirGuard {
    prev: PathBuf,
    _guard: tokio::sync::MutexGuard<'static, ()>,
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev);
    }
}

fn push_current_dir(path: &Path) -> CurrentDirGuard {
    let guard = crate::test_support::cwd::lock();
    let prev = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(path).expect("set current dir");
    CurrentDirGuard {
        prev,
        _guard: guard,
    }
}

fn test_settings() -> crate::settings::SharedSettings {
    std::sync::Arc::new(std::sync::Mutex::new(Settings::new(
        "anthropic:claude-sonnet-4-6",
    )))
}

fn test_app() -> App {
    let mut app = App::new(test_settings());
    app.apply_ui_preset(UiSurfaces::lean());
    app
}

fn active_test_app() -> App {
    let mut app = test_app();
    app.apply_ui_presentation(UiPresentationPolicy::active());
    app
}

fn full_test_app() -> App {
    let mut app = test_app();
    app.apply_ui_presentation(UiPresentationPolicy::full());
    app
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

fn rendered_cell_styles_for_text(
    app: &mut App,
    width: u16,
    height: u16,
    needle: &str,
) -> Vec<(Color, Color)> {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();

    let buf = terminal.backend().buffer();
    for y in 0..buf.area.height {
        let row_symbols: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        for start in 0..row_symbols.len() {
            let candidate: String = row_symbols[start..]
                .iter()
                .take(needle.chars().count())
                .copied()
                .collect();
            if candidate == needle {
                return (start as u16..start as u16 + needle.chars().count() as u16)
                    .map(|x| {
                        let cell = &buf[(x, y)];
                        (cell.fg, cell.bg)
                    })
                    .collect();
            }
        }
    }
    panic!("needle {needle:?} not found in rendered buffer");
}

fn draw_app_with_dirty_background(app: &mut App, width: u16, height: u16, dirty: char) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
                        cell.set_char(dirty);
                        cell.set_fg(Color::White);
                        cell.set_bg(Color::Black);
                    }
                }
            }
            app.draw(frame);
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[test]
fn draw_clears_tab_bar_and_dashboard_leakage_from_dirty_background() {
    let mut app = test_app();
    app.ui_surfaces.dashboard = true;
    app.ui_surfaces.footer = false;
    app.ui_surfaces.instruments = false;
    app.conversation
        .tabs
        .add_extension_tab("widget-1".into(), "tools".into());
    app.dashboard.status_counts.total = 1;
    app.dashboard.all_nodes = vec![crate::tui::dashboard::NodeSummary {
        id: "runtime-task-spawn-policy".into(),
        title: "Runtime Task Spawn Policy".into(),
        status: crate::lifecycle::types::NodeStatus::Exploring,
        open_questions: 0,
        parent: None,
        priority: Some(1),
        issue_type: None,
        openspec_change: None,
    }];

    let rendered = draw_app_with_dirty_background(&mut app, 140, 24, '¤');
    assert!(!rendered.contains('¤'), "got {rendered}");
}

#[test]
fn draw_clears_narrow_footer_instrument_panels_when_layout_shrinks() {
    let mut app = test_app();
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = true;

    let rendered = draw_app_with_dirty_background(&mut app, 60, 12, '§');
    assert!(!rendered.contains('§'), "got {rendered}");
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
fn delegate_decomposition_event_renders_delegate_not_cleave() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::DecompositionStarted {
        children: vec!["delegate_1".into()],
        operation: omegon_traits::OperationRef::delegate("delegate_1"),
    });

    let rendered = render_app_to_string(&mut app, 100, 16);
    assert!(
        rendered.contains("Delegate: delegate_1 started"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Cleave: 1 children dispatched"),
        "delegate-originated child work must not render as cleave: {rendered}"
    );
}

#[test]
fn cleave_decomposition_event_still_renders_cleave() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::DecompositionStarted {
        children: vec!["a".into(), "b".into()],
        operation: omegon_traits::OperationRef::cleave(None),
    });

    let rendered = render_app_to_string(&mut app, 100, 16);
    assert!(
        rendered.contains("Cleave: 2 children dispatched"),
        "{rendered}"
    );
}

#[test]
fn session_reset_clears_instrument_panel_tool_activity() {
    let mut app = full_test_app();
    let waiting = render_app_to_string(&mut app, 140, 18);
    assert!(
        waiting.contains("waiting: provider request")
            || waiting.contains("transcript live")
            || waiting.contains("0/0 active"),
        "{waiting}"
    );

    app.handle_agent_event(AgentEvent::MessageStart {
        role: "assistant".into(),
    });
    let opening = render_app_to_string(&mut app, 140, 18);
    assert!(
        opening.contains("waiting: stream open")
            || opening.contains("transcript live")
            || opening.contains("0/0 active"),
        "{opening}"
    );

    app.handle_agent_event(AgentEvent::MessageChunk {
        text: "hello".into(),
    });
    let responding = render_app_to_string(&mut app, 140, 18);
    assert!(
        responding.contains("streaming answer")
            || responding.contains("transcript live")
            || responding.contains("0/0 active"),
        "{responding}"
    );

    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "context_clear".into(),
        args: serde_json::json!({}),
    });

    let before = render_app_to_string(&mut app, 140, 36);
    assert!(
        before.contains("context clear") || before.contains("context_cle"),
        "got {before}"
    );

    app.handle_agent_event(AgentEvent::SessionReset);

    let exported = app
        .conversation
        .selected_segment_text_with_mode(SegmentExportMode::Plaintext)
        .unwrap_or_default();
    assert!(
        exported.contains("New session started. Previous session saved."),
        "got {exported:?}"
    );

    let after = render_app_to_string(&mut app, 140, 36);
    assert!(
        !after.contains("4/4 active") && !after.contains("running ·"),
        "reset should clear tool activity chrome: {after}"
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
            ..
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
            ..
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

#[tokio::test]
async fn ui_action_set_ui_preset_updates_surfaces() {
    let mut app = test_app();
    let tx = test_tx();

    let outcome = app
        .handle_ui_action(
            UiAction::SetUiPreset(SetUiPresetAction {
                level: UiPresentationLevel::Full,
            }),
            &tx,
        )
        .await;

    assert_eq!(outcome, UiActionOutcome::accepted_message("UI → full"));
    assert!(app.ui_surfaces.dashboard);
    assert!(app.ui_surfaces.instruments);
    assert!(app.ui_surfaces.footer);
    assert!(app.ui_surfaces.activity);
}

#[tokio::test]
async fn ui_action_set_surface_visible_updates_one_surface() {
    let mut app = test_app();
    let tx = test_tx();

    let outcome = app
        .handle_ui_action(
            UiAction::SetSurfaceVisible(SetSurfaceVisibleAction {
                surface: UiSurfaceToggle::Dashboard,
                visible: true,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("UI surface enabled: dashboard")
    );
    assert!(app.ui_surfaces.dashboard);
    assert!(!app.ui_surfaces.instruments);
    assert!(!app.ui_surfaces.footer);
    assert!(app.ui_surfaces.activity);
}

#[tokio::test]
async fn ui_action_can_hide_activity_surface() {
    let mut app = test_app();
    let tx = test_tx();

    let outcome = app
        .handle_ui_action(
            UiAction::SetSurfaceVisible(SetSurfaceVisibleAction {
                surface: UiSurfaceToggle::Activity,
                visible: false,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("UI surface disabled: activity")
    );
    assert!(!app.ui_surfaces.activity);
    assert!(app.ui_status_text().contains("activity: off"));
}

#[tokio::test]
async fn ui_action_select_conversation_segment_updates_selection() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("first");
    app.conversation.push_user("second");

    let outcome = app
        .handle_ui_action(
            UiAction::SelectConversationSegment(SelectConversationSegmentAction {
                segment: ConversationSegmentRef::by_index(2),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("conversation segment selected: 2")
    );
    assert_eq!(app.conversation.selected_segment, Some(2));
}

#[tokio::test]
async fn ui_action_select_conversation_segment_rejects_invalid_index() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("only");

    let outcome = app
        .handle_ui_action(
            UiAction::SelectConversationSegment(SelectConversationSegmentAction {
                segment: ConversationSegmentRef::by_index(9),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::rejected("conversation segment index out of range: 9")
    );
    assert_eq!(app.conversation.selected_segment, None);
}

#[tokio::test]
async fn ui_action_select_conversation_segment_rejects_separator() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("first");
    app.conversation.push_user("second");

    let outcome = app
        .handle_ui_action(
            UiAction::SelectConversationSegment(SelectConversationSegmentAction {
                segment: ConversationSegmentRef::by_index(1),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::rejected("conversation segment is not selectable: 1")
    );
    assert_eq!(app.conversation.selected_segment, None);
}

#[tokio::test]
async fn ui_action_open_conversation_segment_detail_rejects_separator() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("first");
    app.conversation.push_user("second");

    let outcome = app
        .handle_ui_action(
            UiAction::OpenConversationSegmentDetail(OpenConversationSegmentDetailAction {
                segment: ConversationSegmentRef::by_index(1),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::rejected("conversation segment detail is not openable: 1")
    );
    assert_eq!(app.conversation.timeline_expanded_segment(), None);
}

#[tokio::test]
async fn ui_action_open_conversation_segment_detail_toggles_expansion() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("expand me");

    let outcome = app
        .handle_ui_action(
            UiAction::OpenConversationSegmentDetail(OpenConversationSegmentDetailAction {
                segment: ConversationSegmentRef::by_index(0),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("conversation segment detail toggled: 0")
    );
    assert_eq!(app.conversation.timeline_expanded_segment(), Some(0));
    assert_eq!(app.conversation.selected_segment, Some(0));
}

#[tokio::test]
async fn ui_action_copy_conversation_segment_rejects_invalid_index() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("only");

    let outcome = app
        .handle_ui_action(
            UiAction::CopyConversationSegment(CopyConversationSegmentAction {
                segment: ConversationSegmentRef::by_index(9),
                mode: SegmentCopyMode::Raw,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::rejected("conversation segment index out of range: 9")
    );
}

#[tokio::test]
async fn ui_action_copy_conversation_segment_copies_plaintext_detail_without_modal() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation
        .push_user_with_attachments("", &[PathBuf::from("/tmp/paste.png")]);

    let outcome = app
        .handle_ui_action(
            UiAction::CopyConversationSegment(CopyConversationSegmentAction {
                segment: ConversationSegmentRef::by_index(0),
                mode: SegmentCopyMode::Plaintext,
            }),
            &tx,
        )
        .await;

    assert!(
        matches!(
            outcome,
            UiActionOutcome::Accepted { .. } | UiActionOutcome::Rejected { .. }
        ),
        "{outcome:?}"
    );
    assert!(app.copy_text_modal.is_none());
    assert!(!app.terminal_copy_mode);
}

#[tokio::test]
async fn ui_action_copy_latest_assistant_response_rejects_when_missing() {
    let mut app = test_app();
    let tx = test_tx();
    app.conversation.push_user("only user text");

    let outcome = app
        .handle_ui_action(
            UiAction::CopyLatestAssistantResponse(CopyLatestAssistantResponseAction {
                mode: SegmentCopyMode::Raw,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::rejected("no assistant response to copy")
    );
}

#[tokio::test]
async fn ui_action_submit_prompt_sends_local_tui_prompt() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let outcome = app
        .handle_ui_action(
            UiAction::SubmitPrompt(SubmitPromptAction {
                text: "route through action seam".into(),
                attachments: Vec::new(),
                source: PromptSource::LocalTui,
                queue_mode: app.queue_mode,
                metadata: PromptMetadata::default(),
            }),
            &tx,
        )
        .await;

    assert_eq!(outcome, UiActionOutcome::accepted());
    match rx.recv().await.expect("submission command") {
        TuiCommand::SubmitPrompt(PromptSubmission {
            text,
            image_paths,
            submitted_by,
            via,
            ..
        }) => {
            assert_eq!(text, "route through action seam");
            assert!(image_paths.is_empty());
            assert_eq!(submitted_by, "local-tui");
            assert_eq!(via, "tui");
        }
        other => panic!("expected semantic prompt submission, got {other:?}"),
    }
}

#[tokio::test]
async fn ui_action_replace_composer_draft_updates_editor() {
    let mut app = test_app();
    let tx = test_tx();

    let outcome = app
        .handle_ui_action(
            UiAction::ReplaceComposerDraft(ReplaceComposerDraftAction {
                text: "draft through action seam".into(),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer draft replaced")
    );
    assert_eq!(app.editor.render_text(), "draft through action seam");
}

#[tokio::test]
async fn ui_action_clear_composer_draft_clears_editor() {
    let mut app = test_app();
    let tx = test_tx();
    app.editor.set_text("draft");

    let outcome = app
        .handle_ui_action(UiAction::ClearComposerDraft, &tx)
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer draft cleared")
    );
    assert!(app.editor.is_empty());
}

#[tokio::test]
async fn ui_action_attach_composer_path_inserts_attachment_token() {
    let mut app = test_app();
    let tx = test_tx();
    app.editor.set_text("see ");

    let outcome = app
        .handle_ui_action(
            UiAction::AttachComposerPath(AttachComposerPathAction {
                path: std::path::PathBuf::from("/tmp/screenshot.png"),
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer attachment inserted: /tmp/screenshot.png")
    );
    assert_eq!(app.editor.render_text(), "see [image0]");
}

#[tokio::test]
async fn ui_action_move_composer_cursor_supports_character_and_word_units() {
    let mut app = test_app();
    let tx = test_tx();
    app.editor.set_text("alpha beta");

    let outcome = app
        .handle_ui_action(
            UiAction::MoveComposerCursor(MoveComposerCursorAction {
                direction: ComposerCursorDirection::Backward,
                unit: ComposerCursorUnit::Word,
            }),
            &tx,
        )
        .await;
    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer cursor moved")
    );

    app.editor.insert('!');
    assert_eq!(app.editor.render_text(), "alpha !beta");

    let outcome = app
        .handle_ui_action(
            UiAction::MoveComposerCursor(MoveComposerCursorAction {
                direction: ComposerCursorDirection::Forward,
                unit: ComposerCursorUnit::Character,
            }),
            &tx,
        )
        .await;
    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer cursor moved")
    );

    app.editor.insert('?');
    assert_eq!(app.editor.render_text(), "alpha !b?eta");
}

#[tokio::test]
async fn ui_action_edit_composer_deletes_words_and_exits_history_recall() {
    let mut app = test_app();
    let tx = test_tx();
    app.history = vec!["alpha beta".into()];
    app.history_recall_up();
    assert_eq!(app.history_idx, Some(0));

    let outcome = app
        .handle_ui_action(
            UiAction::EditComposer(EditComposerAction {
                operation: ComposerEditOperation::DeleteWordBackward,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer edited")
    );
    assert_eq!(app.editor.render_text(), "alpha ");
    assert_eq!(app.history_idx, None);
    assert_eq!(app.history_draft, None);
}

#[tokio::test]
async fn ui_action_insert_composer_text_inserts_at_cursor_and_exits_history_recall() {
    let mut app = test_app();
    let tx = test_tx();
    app.history = vec!["alpha beta".into()];
    app.history_recall_up();
    app.editor.move_word_backward();

    let outcome = app
        .handle_ui_action(
            UiAction::InsertComposerText(InsertComposerTextAction { text: "!".into() }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer text inserted")
    );
    assert_eq!(app.editor.render_text(), "alpha !beta");
    assert_eq!(app.history_idx, None);
    assert_eq!(app.history_draft, None);
}

#[tokio::test]
async fn ui_action_insert_composer_text_collapses_large_paste() {
    let mut app = test_app();
    let tx = test_tx();
    let text = format!("one\ntwo\nthree\n{}", "x".repeat(120));

    let outcome = app
        .handle_ui_action(
            UiAction::InsertComposerText(InsertComposerTextAction { text }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("composer text inserted")
    );
    assert_eq!(app.editor.render_text(), "[Pasted text #1 +2 lines]");
}

#[tokio::test]
async fn ui_action_move_composer_cursor_rejects_unsupported_direction_unit_pair() {
    let mut app = test_app();
    let tx = test_tx();

    let outcome = app
        .handle_ui_action(
            UiAction::MoveComposerCursor(MoveComposerCursorAction {
                direction: ComposerCursorDirection::Home,
                unit: ComposerCursorUnit::Word,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        outcome,
        UiActionOutcome::rejected("unsupported composer cursor movement")
    );
}

#[tokio::test]
async fn ui_action_permission_response_unblocks_pending_permission() {
    let mut app = test_app();
    let tx = test_tx();
    let (permission_tx, permission_rx) = std::sync::mpsc::channel();
    app.pending_permission = Some(std::sync::Arc::new(std::sync::Mutex::new(Some(
        permission_tx,
    ))));
    app.pending_permission_context = Some(PendingPermissionContext {
        tool_name: "write".into(),
        target: "src/lib.rs".into(),
        kind: omegon_traits::PermissionRequestKind::PathBoundary,
        persistence: omegon_traits::PermissionPersistence::ProjectDirectory,
        grant_path: Some("src".into()),
    });
    app.command_prompt = Some(crate::surfaces::command::CommandPrompt::new(
        "Permission required",
        "Allow write?",
    ));

    let outcome = app
        .handle_ui_action(
            UiAction::RespondToPermission(PermissionAction {
                request_id: None,
                response: omegon_traits::PermissionResponse::Allow,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        permission_rx.recv().expect("permission response"),
        omegon_traits::PermissionResponse::Allow
    );
    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("→ allowed (this session): write src/lib.rs")
    );
    assert!(app.pending_permission.is_none());
    assert!(app.pending_permission_context.is_none());
    assert!(app.command_prompt.is_none());
}

#[tokio::test]
async fn ui_action_operator_wait_response_unblocks_pending_wait() {
    let mut app = test_app();
    let tx = test_tx();
    let (wait_tx, wait_rx) = std::sync::mpsc::channel();
    app.pending_operator_wait = Some(std::sync::Arc::new(std::sync::Mutex::new(Some(wait_tx))));
    app.pending_operator_wait_context = Some("deploy smoke test".into());
    app.command_prompt = Some(crate::surfaces::command::CommandPrompt::new(
        "Manual action required",
        "deploy smoke test",
    ));

    let outcome = app
        .handle_ui_action(
            UiAction::RespondToOperatorWait(OperatorWaitAction {
                request_id: None,
                response: omegon_traits::OperatorWaitResponse::Completed,
            }),
            &tx,
        )
        .await;

    assert_eq!(
        wait_rx.recv().expect("operator wait response"),
        omegon_traits::OperatorWaitResponse::Completed
    );
    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("-> manual action completed: deploy smoke test")
    );
    assert!(app.pending_operator_wait.is_none());
    assert!(app.pending_operator_wait_context.is_none());
    assert!(app.command_prompt.is_none());
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

#[tokio::test]
async fn bang_prefix_runs_direct_shell_command() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.editor.set_text("!git status");

    app.submit_editor_buffer(&tx).await;

    match rx.recv().await.expect("queued prompt") {
        TuiCommand::RunShellCommand { command, .. } => {
            assert_eq!(command, "git status");
        }
        other => panic!("expected direct shell command, got {other:?}"),
    }
}

#[tokio::test]
async fn bare_bang_requests_shell_handoff() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.editor.set_text("!");

    app.submit_editor_buffer(&tx).await;

    match rx.recv().await.expect("queued prompt") {
        TuiCommand::ShellHandoff { .. } => {}
        other => panic!("expected shell handoff, got {other:?}"),
    }
}

#[tokio::test]
async fn bare_bang_does_not_emit_system_banner_before_handoff() {
    let mut app = test_app();
    let (tx, _rx) = test_tx_with_rx();
    app.editor.set_text("!");

    app.submit_editor_buffer(&tx).await;

    let rendered = render_app_to_string(&mut app, 100, 20);
    assert!(
        !rendered.contains("Entering shell handoff"),
        "unexpected handoff banner in conversation: {rendered}"
    );
}

#[tokio::test]
async fn at_prefix_wraps_prompt_as_context_request() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.editor.set_text("@queue semantics");

    app.submit_editor_buffer(&tx).await;

    match rx.recv().await.expect("queued prompt") {
        TuiCommand::SubmitPrompt(PromptSubmission { text, .. }) => {
            assert!(text.contains("request focused context"), "{text}");
            assert!(text.contains("queue semantics"), "{text}");
        }
        other => panic!("expected prompt submission, got {other:?}"),
    }
}

#[tokio::test]
async fn star_prefix_wraps_prompt_as_memory_injection_request() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.editor.set_text("*document our work");

    app.submit_editor_buffer(&tx).await;

    match rx.recv().await.expect("queued prompt") {
        TuiCommand::SubmitPrompt(PromptSubmission { text, .. }) => {
            assert!(text.contains("recall relevant project memory"), "{text}");
            assert!(text.contains("document our work"), "{text}");
        }
        other => panic!("expected prompt submission, got {other:?}"),
    }
}
#[tokio::test]
async fn submitting_while_agent_active_submits_to_runtime_queue_without_interrupt_by_default() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.agent_active = true;
    app.editor.set_text("follow up after this turn");

    app.submit_editor_buffer(&tx).await;

    assert!(
        !app.interrupt_pending,
        "queued input must not cancel the active turn by default"
    );
    match rx.recv().await.expect("runtime prompt submission") {
        TuiCommand::SubmitPrompt(PromptSubmission {
            text, queue_mode, ..
        }) => {
            assert_eq!(text, "follow up after this turn");
            assert_eq!(queue_mode, PromptQueueMode::UntilReady);
        }
        other => panic!("expected runtime prompt submission, got {other:?}"),
    }
    assert!(
        app.conversation.segments().iter().all(|segment| !matches!(
            &segment.content,
            crate::tui::segments::SegmentContent::UserPrompt { text }
                if text == "follow up after this turn"
        )),
        "queued prompt must not be visible as an operator segment until the runtime starts it"
    );
}

#[tokio::test]
async fn idle_submission_waits_for_runtime_prompt_started_segment() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.editor.set_text("start actual work");

    app.submit_editor_buffer(&tx).await;

    match rx.recv().await.expect("prompt submission") {
        TuiCommand::SubmitPrompt(PromptSubmission { text, .. }) => {
            assert_eq!(text, "start actual work");
        }
        other => panic!("expected prompt submission, got {other:?}"),
    }
    assert!(
        app.conversation.segments().iter().all(|segment| !matches!(
            &segment.content,
            crate::tui::segments::SegmentContent::UserPrompt { text }
                if text == "start actual work"
        )),
        "idle prompt must not be visible until the runtime starts it"
    );

    app.handle_agent_event(AgentEvent::RuntimePromptStarted {
        text: "start actual work".to_string(),
        image_paths: Vec::new(),
    });

    assert!(
        app.conversation.segments().iter().any(|segment| matches!(
            &segment.content,
            crate::tui::segments::SegmentContent::UserPrompt { text }
                if text == "start actual work"
        )),
        "runtime-started prompt must become visible"
    );
}

#[test]
fn runtime_prompt_started_event_displays_operator_segment() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::RuntimePromptStarted {
        text: "follow up after this turn".to_string(),
        image_paths: Vec::new(),
    });

    assert!(
        app.conversation.segments().iter().any(|segment| matches!(
            &segment.content,
            crate::tui::segments::SegmentContent::UserPrompt { text }
                if text == "follow up after this turn"
        )),
        "started queued prompt must be visible as an operator segment"
    );
}

#[tokio::test]
async fn explicit_interrupt_queue_mode_submits_prompt_then_cancel() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.agent_active = true;
    app.queue_mode = PromptQueueMode::InterruptAfterTurn;
    app.editor.set_text("steer immediately");

    app.submit_editor_buffer(&tx).await;

    assert!(
        app.interrupt_pending,
        "explicit interrupt mode should mark local interrupt UI state"
    );
    match rx.recv().await.expect("runtime prompt submission") {
        TuiCommand::SubmitPrompt(PromptSubmission {
            text, queue_mode, ..
        }) => {
            assert_eq!(text, "steer immediately");
            assert_eq!(queue_mode, PromptQueueMode::InterruptAfterTurn);
        }
        other => panic!("expected runtime prompt submission, got {other:?}"),
    }
    match rx.recv().await.expect("runtime cancel submission") {
        TuiCommand::CancelActiveTurn { submitted_by, via } => {
            assert_eq!(submitted_by, "local-tui");
            assert_eq!(via, "tui");
        }
        other => panic!("expected runtime cancel submission, got {other:?}"),
    }
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
        context_class: "Massive".into(),
        thinking_level: "high".into(),
    });

    assert_eq!(app.footer_data.context_class, ContextClass::Massive);
    assert_eq!(app.footer_data.actual_context_class, ContextClass::Compact);
    assert!(app.footer_data.context_percent > 99.0);
}

#[test]
fn turn_end_does_not_overwrite_footer_context_with_last_request_input_tokens() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 144_000,
        context_window: 272_000,
        context_class: "Standard".into(),
        thinking_level: "high".into(),
    });
    let before = app.footer_data.context_percent;
    assert!(
        before > 52.0 && before < 54.0,
        "expected ~53%, got {before}"
    );

    app.handle_agent_event(AgentEvent::TurnEnd(Box::new(
        omegon_traits::AgentEventTurnEnd {
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
        },
    )));

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

    app.handle_agent_event(AgentEvent::TurnEnd(Box::new(
        omegon_traits::AgentEventTurnEnd {
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
        },
    )));
    app.handle_agent_event(AgentEvent::TurnEnd(Box::new(
        omegon_traits::AgentEventTurnEnd {
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
        },
    )));

    assert_eq!(app.footer_data.session_input_tokens, 112_000);
    assert_eq!(app.footer_data.session_output_tokens, 23_000);
    assert_eq!(app.footer_data.session_usage_slices.len(), 2);

    let session_text = crate::tui::footer::format_session_text(
        app.footer_data.turn,
        app.footer_data.session_input_tokens,
        app.footer_data.session_output_tokens,
        app.footer_data.last_turn_input_tokens,
        app.footer_data.last_turn_output_tokens,
        &app.footer_data.session_usage_slices,
    );
    assert_eq!(session_text, "T0 ¤112k/¤23k (turn ¤12k)");
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

#[tokio::test]
async fn ui_action_cancel_active_turn_routes_to_runtime_and_suppresses_input() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    let token = CancellationToken::new();
    *app.cancel.lock().expect("cancel lock") = Some(token.clone());
    app.editor.set_text("draft");

    let outcome = app.handle_ui_action(UiAction::CancelActiveTurn, &tx).await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("active turn cancellation requested")
    );
    assert!(
        !token.is_cancelled(),
        "TUI cancel action must not trip the runtime token directly"
    );
    match rx.try_recv() {
        Ok(TuiCommand::CancelActiveTurn { submitted_by, via }) => {
            assert_eq!(submitted_by, "local-tui");
            assert_eq!(via, "tui");
        }
        other => panic!("expected runtime cancel command, got {other:?}"),
    }
    assert_eq!(app.editor.render_text(), "");
    assert!(app.interrupt_pending);
}

#[tokio::test]
async fn ui_action_cancel_active_turn_without_token_still_routes_to_runtime() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.editor.set_text("draft");

    let outcome = app.handle_ui_action(UiAction::CancelActiveTurn, &tx).await;

    assert_eq!(
        outcome,
        UiActionOutcome::accepted_message("active turn cancellation requested")
    );
    match rx.try_recv() {
        Ok(TuiCommand::CancelActiveTurn { submitted_by, via }) => {
            assert_eq!(submitted_by, "local-tui");
            assert_eq!(via, "tui");
        }
        other => panic!("expected runtime cancel command, got {other:?}"),
    }
    assert_eq!(app.editor.render_text(), "");
    assert!(app.interrupt_pending);
}

#[test]
fn interrupt_suppresses_terminal_protocol_fragments_from_editor() {
    let mut app = test_app();
    app.agent_active = true;
    app.editor.set_text("draft");

    app.prepare_interrupt_ui();

    assert_eq!(app.editor.render_text(), "");
    assert!(app.interrupt_pending);

    let protocol_fragment = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('['),
        crossterm::event::KeyModifiers::NONE,
    );
    assert!(
        app.should_discard_key_after_interrupt(&protocol_fragment),
        "raw CSI-u fragments from Ctrl+C must not enter the composer during the debounce window"
    );

    app.suppress_editor_input_until =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
    assert!(
        !app.should_discard_key_after_interrupt(&protocol_fragment),
        "pending interrupts must not suppress all composer input indefinitely if the agent never emits AgentEnd"
    );

    let ctrl_c = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL,
    );
    assert!(
        !app.should_discard_key_after_interrupt(&ctrl_c),
        "repeat Ctrl+C must remain available while an interrupt is pending"
    );
}

#[test]
fn tail_chars_handles_emoji_at_tail_boundary() {
    let text = "Done. Seed is empty:\n\n\
| Stack | Containers | Status |\n\
|-------|-----------|--------|\n\
| komodo + traefik | komodo-core, periphery, ferretdb, postgres, traefik | ✓ Down |\n\
| netbox | netbox, postgres, redis, redis-cache | ✓ Down |\n\
| rustdesk | hbbs, hbbr | ✓ Down |\n";

    for n in 0..text.len() {
        let tail = App::tail_chars(text, n);
        assert!(tail.is_char_boundary(0));
        assert!(text.ends_with(tail));
    }
}

#[test]
fn non_english_streaming_output_does_not_panic_at_char_boundaries() {
    let mut app = test_app();
    let russian_output = "Запушил. `dcf210b` уехал в `origin/feat/aml-backend-sandbox-env`. \
Запушил. `dcf210b` — `fix: rename sandbox.company.dev DNS record to beta.company.dev` \
теперь на `origin/feat/aml-backend-sandbox-env`.\n\nОдин момент. "
        .repeat(24);

    for chunk_size in 1..128 {
        let chunk = App::tail_chars(&russian_output, chunk_size);
        assert!(chunk.is_char_boundary(0));
        app.handle_agent_event(AgentEvent::MessageChunk {
            text: chunk.to_string(),
        });
    }

    let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
    terminal
        .draw(|frame| app.draw(frame))
        .expect("non-English output should render without panicking");
}

#[test]
fn slim_status_line_marks_detached_conversation_viewport() {
    let mut app = active_test_app();
    app.conversation.conv_state.scroll_offset = 12;
    app.conversation.conv_state.user_scrolled = true;

    let text = render_app_to_string(&mut app, 120, 18);
    assert!(text.contains("view detached ↑12 · End tail"), "{text}");
}

#[test]
fn workstream_only_plan_update_merges_without_clearing_active_lane() {
    let mut app = test_app();
    app.workbench_state.active = Some(PlanDisplaySnapshot {
        mode: "planning".into(),
        completed: 0,
        total: 1,
        items: vec![PlanDisplayItem {
            status: PlanDisplayStatus::Active,
            description: "active plan work".into(),
        }],
    });

    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            version: 1,
            workstreams: vec![omegon_traits::PlanWorkstreamProjection {
                id: "cleave:cleave_live".into(),
                title: "Cleave approval required — 1 child / max_parallel 1".into(),
                status: "pending_approval".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 0,
                    total: 1,
                },
            }],
            ..Default::default()
        },
    });

    assert!(app.workbench_state.active.is_some());
    assert_eq!(app.workbench_state.workstreams.len(), 1);
    assert_eq!(app.workbench_state.workstreams[0].id, "cleave:cleave_live");
    assert_eq!(
        app.workbench_state.workstreams[0].status,
        super::workbench::WorkstreamStatus::PendingApproval
    );
}

#[test]
fn plan_update_without_active_lane_clears_stale_workbench_plan() {
    let mut app = test_app();
    app.workbench_state.active = Some(PlanDisplaySnapshot {
        mode: "planning".into(),
        completed: 0,
        total: 1,
        items: vec![PlanDisplayItem {
            status: PlanDisplayStatus::Active,
            description: "stale work".into(),
        }],
    });

    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection::default(),
    });

    assert!(
        app.workbench_state.active.is_none(),
        "active workbench plan must clear when the authoritative projection has no active lane"
    );
}

#[test]
fn completed_plan_update_clears_live_operation_handles_but_keeps_workstream_summary() {
    let mut app = test_app();
    app.dashboard_handles.cleave = Some(std::sync::Arc::new(std::sync::Mutex::new(
        crate::features::cleave::CleaveProgress {
            active: true,
            run_id: "cleave-activity".into(),
            inventory_generation: None,
            total_children: 1,
            completed: 0,
            failed: 0,
            children: vec![crate::features::cleave::ChildProgress {
                label: "scout/files".into(),
                status: "pending".into(),
                failure_kind: None,
                duration_secs: None,
                supervision_mode: None,
                pid: None,
                last_tool: None,
                last_tool_activity: None,
                last_turn: None,
                tasks: Vec::new(),
                tasks_done: 0,
                started_at: None,
                last_activity_at: None,
                tokens_in: 0,
                tokens_out: 0,
                runtime: None,
            }],
            total_tokens_in: 0,
            total_tokens_out: 0,
        },
    )));

    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            version: 1,
            active: Some(omegon_traits::PlanLaneProjection {
                plan_id: "smoke:cleave-activity".into(),
                mode: "complete".into(),
                status: "complete".into(),
                source: "smoke".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 4,
                    total: 4,
                },
                items: vec![omegon_traits::PlanItemProjection {
                    label: "Cleave child activity".into(),
                    status: "done".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            workstreams: vec![omegon_traits::PlanWorkstreamProjection {
                id: "smoke:cleave-activity".into(),
                title: "Cleave child activity – shared surfaces".into(),
                status: "complete".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 4,
                    total: 4,
                },
            }],
            ..Default::default()
        },
    });

    assert!(app.dashboard_handles.cleave.is_none());
    assert!(app.workbench_state.active.is_none());
    assert_eq!(app.workbench_state.workstreams.len(), 1);
    assert_eq!(app.workbench_state.workstreams[0].completed, 4);
    assert_eq!(app.workbench_state.workstreams[0].total, 4);
}

#[test]
fn plan_update_preserves_workspace_context() {
    let mut app = test_app();
    app.workbench_state.workspace = WorkbenchWorkspaceContext {
        repo: Some("omegon".into()),
        dir: "omegon-secundus".into(),
        git_branch: Some("feature/ui-improvements-polish".into()),
    };

    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                mode: "planning".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 0,
                    total: 1,
                },
                items: vec![omegon_traits::PlanItemProjection {
                    status: "active".into(),
                    label: "do work".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
    });

    assert_eq!(
        app.workbench_state.workspace.repo.as_deref(),
        Some("omegon")
    );
    assert_eq!(
        app.workbench_state.workspace.git_branch.as_deref(),
        Some("feature/ui-improvements-polish")
    );
    assert!(app.workbench_state.active.is_some());
}

#[test]
fn completed_plan_update_clears_active_lane_and_preserves_workspace_context() {
    let mut app = test_app();
    app.workbench_state.workspace = WorkbenchWorkspaceContext {
        repo: Some("omegon".into()),
        dir: "omegon-secundus".into(),
        git_branch: Some("feature/ui-improvements-polish".into()),
    };

    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                mode: "complete".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 1,
                    total: 1,
                },
                items: vec![omegon_traits::PlanItemProjection {
                    status: "done".into(),
                    label: "done work".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
    });

    assert!(app.workbench_state.active.is_none());
    assert_eq!(
        app.workbench_state.workspace.repo.as_deref(),
        Some("omegon")
    );
    assert_eq!(
        app.workbench_state.workspace.git_branch.as_deref(),
        Some("feature/ui-improvements-polish")
    );
}

#[test]
fn completed_plan_update_enables_done_view_hint_without_pinning() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                mode: "complete".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 1,
                    total: 1,
                },
                items: vec![omegon_traits::PlanItemProjection {
                    status: "done".into(),
                    label: "remember me".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        },
    });

    assert!(app.completed_plan_history_available);
    assert!(app.workbench_state.active.is_none());
    let text = render_app_to_string(&mut app, 120, 18);
    assert!(text.contains("plan complete · history available"), "{text}");
    assert!(
        !text.contains("remember me"),
        "completed history should not pin active lane: {text}"
    );
}

#[test]
fn completed_plan_update_reattaches_detached_slim_viewport() {
    let mut app = test_app();
    app.conversation.conv_state.scroll_offset = 46;
    app.conversation.conv_state.user_scrolled = true;
    app.workbench_state.active = Some(PlanDisplaySnapshot {
        mode: "executing".into(),
        completed: 1,
        total: 2,
        items: vec![
            PlanDisplayItem {
                status: PlanDisplayStatus::Done,
                description: "one".into(),
            },
            PlanDisplayItem {
                status: PlanDisplayStatus::Active,
                description: "two".into(),
            },
        ],
    });

    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                mode: "complete".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 2,
                    total: 2,
                },
                items: vec![
                    omegon_traits::PlanItemProjection {
                        status: "done".into(),
                        label: "one".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        status: "done".into(),
                        label: "two".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        },
    });

    assert_eq!(app.conversation.conv_state.scroll_offset, 0);
    assert!(!app.conversation.conv_state.user_scrolled);
    assert!(app.workbench_state.active.is_none());
    assert!(
        app.conversation
            .latest_plan_progress()
            .is_some_and(|text| text.contains("Plan mode: complete")),
        "completed plan should remain as transcript history"
    );

    let text = render_app_to_string(&mut app, 120, 18);
    assert!(!text.contains("view detached"), "{text}");
    assert!(!text.contains("more below · End to tail"), "{text}");
    assert!(!text.contains("plan done · clear"), "{text}");
}

#[test]
fn assistant_completed_turn_keeps_incomplete_live_plan_lane() {
    let mut app = active_test_app();
    app.handle_agent_event(AgentEvent::PlanUpdated {
        projection: omegon_traits::PlanSurfaceProjection {
            active: Some(omegon_traits::PlanLaneProjection {
                mode: "planning".into(),
                progress: omegon_traits::PlanProgressProjection {
                    completed: 3,
                    total: 4,
                },
                items: vec![
                    omegon_traits::PlanItemProjection {
                        status: "active".into(),
                        label: "Harden set_recipe".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        status: "done".into(),
                        label: "Add regression test".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        status: "done".into(),
                        label: "Validate tests".into(),
                        ..Default::default()
                    },
                    omegon_traits::PlanItemProjection {
                        status: "done".into(),
                        label: "Update changelog".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    assert!(app.workbench_state.active.is_some());

    app.handle_agent_event(AgentEvent::TurnEnd(Box::new(
        omegon_traits::AgentEventTurnEnd {
            turn: 1,
            turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
            model: None,
            provider: None,
            estimated_tokens: 0,
            context_window: 0,
            context_composition: omegon_traits::ContextComposition::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
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
        },
    )));

    assert!(app.workbench_state.active.is_some());
    let text = render_app_to_string(&mut app, 140, 18);
    assert!(text.contains("plan active"), "{text}");
    assert!(text.contains("Harden set_recipe"), "{text}");
    assert!(text.contains("turn done"), "{text}");
}

#[test]
fn slim_status_line_marks_turn_state() {
    let mut app = active_test_app();
    app.handle_agent_event(AgentEvent::TurnStart { turn: 1 });
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command":"cargo test"}),
    });
    if let SegmentContent::ToolCard { started_at, .. } =
        &mut app.conversation.segments_mut()[0].content
        && let Some(started_at) = started_at.as_mut()
    {
        *started_at -= std::time::Duration::from_secs(54);
    }
    assert_eq!(app.slim_turn_state, SlimTurnState::Tool("bash".to_string()));
    let SegmentContent::ToolCard {
        name,
        detail_args,
        complete,
        started_at,
        ..
    } = &app.conversation.segments()[0].content
    else {
        panic!("expected running tool card");
    };
    assert_eq!(name, "bash");
    assert_eq!(detail_args.as_deref(), Some("cargo test"));
    assert!(!complete);
    assert!(started_at.is_some());

    let running = render_app_to_string(&mut app, 140, 18);
    assert!(running.contains("live log"), "{running}");

    app.handle_agent_event(AgentEvent::TurnEnd(Box::new(
        omegon_traits::AgentEventTurnEnd {
            turn: 1,
            turn_end_reason: omegon_traits::TurnEndReason::AssistantCompleted,
            model: Some("openai:gpt-5.4".into()),
            provider: Some("openai".into()),
            estimated_tokens: 0,
            context_window: 0,
            context_composition: omegon_traits::ContextComposition::default(),
            actual_input_tokens: 0,
            actual_output_tokens: 0,
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
        },
    )));
    assert_eq!(app.slim_turn_state, SlimTurnState::Finished("done"));
    assert!(
        !app.agent_active,
        "terminal TurnEnd must release the TUI-local active-turn gate even if AgentEnd is delayed"
    );
    let done_before_agent_end = render_app_to_string(&mut app, 140, 18);
    assert!(
        done_before_agent_end.contains("turn done"),
        "{done_before_agent_end}"
    );
    assert!(
        !done_before_agent_end.contains("active turn"),
        "activity row must not disagree with terminal turn state: {done_before_agent_end}"
    );

    app.handle_agent_event(AgentEvent::AgentEnd);
    assert_eq!(app.slim_turn_state, SlimTurnState::Finished("done"));
    let done = render_app_to_string(&mut app, 140, 18);
    assert!(done.contains("turn done"), "{done}");
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
fn mouse_wheel_over_conversation_never_enters_history_recall() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.editor.set_text("draft");

    app.conversation.push_user("user");
    app.conversation
        .append_streaming("line 1\nline 2\nline 3\nline 4\nline 5\nline 6");

    app.conversation_area = Some(Rect::new(0, 0, 80, 12));

    // Model the event-loop wheel routing contract directly: wheel over conversation
    // should scroll the conversation even if the editor currently owns focus.
    app.handle_mouse_scroll_up(1, 1);
    assert_eq!(app.history_idx, None);
    assert_eq!(app.editor.render_text(), "draft");

    let after_up = app.conversation.conv_state.scroll_offset;
    assert!(after_up > 0, "wheel-up should move into history");

    app.handle_mouse_scroll_down(1, 1);
    assert!(app.conversation.conv_state.scroll_offset < after_up);
    assert_eq!(app.history_idx, None);
    assert_eq!(app.editor.render_text(), "draft");
}

#[test]
fn mouse_wheel_over_editor_never_enters_history_recall() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.editor.set_text("draft");
    app.editor_area = Some(Rect::new(0, 20, 80, 3));

    app.handle_mouse_scroll_up(1, 21);
    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);

    app.handle_mouse_scroll_down(1, 21);
    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);
}

#[test]
fn ctrl_y_keeps_editor_yank_available() {
    let mut app = test_app();
    app.editor.set_text("prefix");
    app.editor.clear_line();

    app.editor.yank();

    assert_eq!(app.editor.render_text(), "prefix");
}

#[test]
fn startup_initialization_defaults_to_lean_compact() {
    let app = test_app();

    assert!(
        app.ui_surfaces.is_compact(),
        "default startup should be compact (lean)"
    );
    assert!(
        !app.mouse_capture_enabled,
        "default startup should keep mouse capture off for terminal-native selection"
    );
}

#[test]
fn slim_mode_renders_without_side_gutters_for_copyable_wrapped_lines() {
    let seg = crate::tui::segments::Segment::user_prompt(
        "This is a long wrapped line that should remain copyable without left or right gutter chrome in OM mode. This is a long wrapped line that should remain copyable without left or right gutter chrome in OM mode.",
    );
    let backend = ratatui::backend::TestBackend::new(60, 12);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            seg.render(
                frame.area(),
                frame.buffer_mut(),
                &crate::tui::theme::Alpharius,
                crate::tui::segments::SegmentRenderMode::Slim,
                crate::settings::ToolDetail::Detailed,
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let mut rendered = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            rendered.push_str(buf[(x, y)].symbol());
        }
        rendered.push('\n');
    }

    assert!(
        !rendered.contains("│"),
        "slim mode should avoid side gutters: {rendered}"
    );
    assert!(
        !rendered.contains("╭"),
        "slim mode should avoid card borders: {rendered}"
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

    app.set_terminal_copy_mode(false);
    assert!(!app.terminal_copy_mode);
    assert!(app.mouse_capture_enabled);
}
#[test]
fn text_copy_modal_uses_wide_copy_surface_with_non_copy_footer() {
    let mut app = test_app();
    app.copy_text_modal = Some(CopyTextModal::new("Copy text", "alpha beta gamma"));

    let rendered = render_app_to_string(&mut app, 100, 30);

    assert!(rendered.contains("alpha beta gamma"), "got {rendered}");
    assert!(rendered.contains("Copy all"), "got {rendered}");
    assert!(
        rendered.contains("terminal drag selects text"),
        "got {rendered}"
    );
    assert!(app.copy_text_copy_button_area.is_some());
    assert_eq!(
        app.copy_text_modal
            .as_ref()
            .map(|modal| modal.text.as_str()),
        Some("alpha beta gamma")
    );
}

#[test]
fn conversation_omits_inline_copy_affordance() {
    let mut cv = ConversationView::new();
    cv.push_user("alpha beta gamma");

    let t = crate::tui::theme::Alpharius;
    let area = Rect::new(0, 0, 80, 8);
    let mut buf = Buffer::empty(area);
    {
        let (segments, state) = cv.segments_and_state();
        let widget = crate::tui::conv_widget::ConversationWidget::new(segments, &t);
        widget.render(area, &mut buf, state);
    }

    let rendered = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains(" Copy "), "got {rendered}");
}

#[test]
fn copy_text_modal_scrolls_markdown_and_code_without_mutating_payload() {
    let mut modal = CopyTextModal::new(
        "Copy text",
        "```rust\nfn main() { println!(\"hello\"); }\n```\n\n# Notes\nbody",
    );

    modal.scroll_down(2);
    assert_eq!(modal.scroll_y, 2);
    modal.scroll_up(1);
    assert_eq!(modal.scroll_y, 1);
    assert_eq!(
        modal.text,
        "```rust\nfn main() { println!(\"hello\"); }\n```\n\n# Notes\nbody"
    );
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

    assert!(matches!(
        app.handle_slash_command("/mouse off", &tx),
        SlashResult::Handled
    ));
    assert!(app.terminal_copy_mode);
    assert!(!app.mouse_capture_enabled);
}

#[tokio::test]
async fn empty_enter_preloads_last_history_prompt_before_send() {
    let mut app = test_app();
    app.history = vec!["first".into(), "last prompt".into()];
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);

    app.submit_editor_buffer(&tx).await;
    assert_eq!(app.editor.render_text(), "");
    assert_eq!(app.pending_history_preload.as_deref(), Some("last prompt"));
    assert!(rx.try_recv().is_err());

    app.submit_editor_buffer(&tx).await;
    assert_eq!(app.editor.render_text(), "last prompt");
    assert_eq!(app.pending_history_preload, None);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn typing_after_history_preload_starts_fresh_prompt() {
    let mut app = test_app();
    app.history = vec!["last prompt".into()];
    let (tx, _rx) = tokio::sync::mpsc::channel(1);

    app.submit_editor_buffer(&tx).await;
    assert_eq!(app.pending_history_preload.as_deref(), Some("last prompt"));

    app.pending_history_preload = None;
    app.editor.insert('n');
    assert_eq!(app.editor.render_text(), "n");
}

#[test]
fn alt_up_recalls_latest_history_entry() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];

    assert!(app.editor.is_empty());
    assert_eq!(app.history_idx, None);

    app.history_recall_up();
    assert_eq!(app.editor.render_text(), "third");
    assert_eq!(app.history_idx, Some(2));
}

#[test]
fn alt_up_walks_back_multiple_entries_after_recall_starts() {
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
fn bare_up_does_not_recall_history_from_empty_editor() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into(), "third".into()];
    app.terminal_copy_mode = false;

    app.handle_keyboard_up();

    assert_eq!(app.editor.render_text(), "");
    assert_eq!(app.history_idx, None);
}

#[test]
fn non_empty_editor_alt_up_does_not_start_history_recall() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.editor.set_text("draft");

    app.history_recall_up();

    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);
}

#[test]
fn alt_down_clears_editor_after_latest_entry() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];

    app.history_recall_up();
    app.history_recall_down();
    assert_eq!(app.editor.render_text(), "");
    assert_eq!(app.history_idx, None);
}

#[test]
fn history_down_restores_prefilled_draft_after_recall_session() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.editor.set_text("draft");

    app.history_up();
    assert_eq!(app.editor.render_text(), "second");
    app.history_down();

    assert_eq!(app.editor.render_text(), "draft");
    assert_eq!(app.history_idx, None);
    assert_eq!(app.history_draft, None);
}

#[test]
fn editing_after_history_recall_exits_history_session() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];

    app.history_recall_up();
    app.editor.insert('!');
    app.exit_history_recall();

    assert_eq!(app.editor.render_text(), "second!");
    assert_eq!(app.history_idx, None);
    assert_eq!(app.history_draft, None);
}

#[test]
fn bare_down_does_not_advance_recalled_history() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.terminal_copy_mode = false;

    app.history_recall_up();
    assert_eq!(app.editor.render_text(), "second");

    app.handle_keyboard_down();

    assert_eq!(app.editor.render_text(), "second");
    assert_eq!(app.history_idx, Some(1));
}

#[test]
fn bare_down_does_not_mutate_draft_or_history_index() {
    let mut app = test_app();
    app.history = vec!["first".into(), "second".into()];
    app.editor.set_text("draft");

    app.handle_keyboard_down();

    assert_eq!(app.editor.render_text(), "draft");
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
fn slash_focus_reports_removed_command() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/focus", &tx);
    assert!(matches!(result, SlashResult::Display(message) if message.contains("removed")));
}

#[test]
fn slash_shackle_switches_to_slim_runtime_profile() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/shackle", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(app.ui_surfaces.is_compact());

    match rx.try_recv().expect("queued control") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim },
            ..
        } => assert!(slim),
        other => panic!("expected SetRuntimeMode slim control request, got {other:?}"),
    }
}

#[test]
fn slash_unshackle_switches_to_full_runtime_profile() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    if let Ok(mut s) = app.settings.lock() {
        s.set_posture(crate::settings::PosturePreset::Explorator);
    }
    app.apply_ui_preset(UiSurfaces::lean());

    let result = app.handle_slash_command("/unshackle", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.ui_surfaces.is_compact());

    match rx.try_recv().expect("queued control") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim },
            ..
        } => assert!(!slim),
        other => panic!("expected SetRuntimeMode full control request, got {other:?}"),
    }
}

#[test]
fn slash_warp_toggles_between_slim_and_full_modes() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/warp", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(app.ui_surfaces.is_compact());
    match rx.try_recv().expect("queued control") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim },
            ..
        } => assert!(slim),
        other => panic!("expected SetRuntimeMode slim control request, got {other:?}"),
    }

    if let Ok(mut s) = app.settings.lock() {
        s.set_posture(crate::settings::PosturePreset::Explorator);
    }

    let result = app.handle_slash_command("/warp", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(!app.ui_surfaces.is_compact());
    match rx.try_recv().expect("queued control") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetRuntimeMode { slim },
            ..
        } => assert!(!slim),
        other => panic!("expected SetRuntimeMode full control request, got {other:?}"),
    }
}

#[test]
fn ui_command_switches_between_om_active_and_full_presentations() {
    let mut app = test_app();
    let tx = test_tx();

    for alias in ["om", "lean", "slim"] {
        let result = app.handle_slash_command(&format!("/ui {alias}"), &tx);
        assert!(matches!(result, SlashResult::Display(_)));
        assert_eq!(app.ui_presentation.level, UiPresentationLevel::Om);
        assert_eq!(app.ui_presentation.preset_name(), "om");
        assert!(app.ui_surfaces.is_compact());
    }

    let result = app.handle_slash_command("/ui active", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert_eq!(app.ui_presentation.level, UiPresentationLevel::Active);
    assert_eq!(app.ui_presentation.preset_name(), "active");
    assert!(app.ui_surfaces.is_compact());

    let result = app.handle_slash_command("/ui full", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert_eq!(app.ui_presentation.level, UiPresentationLevel::Full);
    assert!(!app.ui_surfaces.is_compact());
    assert!(app.ui_surfaces.dashboard);
    assert!(app.ui_surfaces.instruments);
    assert!(app.ui_surfaces.footer);

    let result = app.handle_slash_command("/ui standard", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    if let SlashResult::Display(text) = result {
        assert!(text.contains("Unknown UI command"), "{text}");
    }
}

#[test]
fn short_slash_confirmations_use_toast_surface() {
    assert!(super::should_toast_slash_response("UI → full"));
    assert!(super::should_toast_slash_response(
        "Context Policy → Extended (400k)"
    ));
}

#[test]
fn verbose_or_error_slash_responses_still_use_panel_surface() {
    assert!(!super::should_toast_slash_response(
        "Usage: /ui <lean|full>"
    ));
    assert!(!super::should_toast_slash_response(
        "Unknown UI command: standard"
    ));
    assert!(!super::should_toast_slash_response("one\ntwo"));
}

#[test]
fn unknown_command_uses_compact_warning_toast() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/info", &tx);

    let SlashResult::Display(text) = result else {
        panic!("unknown command should display feedback");
    };
    assert_eq!(text, "Unknown command: /info. Type /help for commands.");
    assert!(!super::should_toast_slash_response(&text));

    app.show_slash_response("/info", &text);
    assert!(app.command_panel.is_none());
    assert_eq!(app.operator_events.len(), 1);
    let event = app.operator_events.back().expect("warning toast");
    assert_eq!(event.message, text);
    assert_eq!(event.icon, "⚠");
}

#[test]
fn verbose_informational_slash_responses_become_system_segments() {
    let mut app = test_app();
    let response = "Version\nOmegon: 0.27.0\nGit SHA: test\nBuild Date: today";

    app.show_slash_response("/version", response);

    assert!(app.command_panel.is_none());
    let segment = app.conversation.segments().last().expect("system segment");
    let SegmentContent::SystemNotification { text } = &segment.content else {
        panic!(
            "expected system notification segment, got {:?}",
            segment.content
        );
    };
    assert_eq!(
        text,
        "command · /version\nVersion\nOmegon: 0.27.0\nGit SHA: test\nBuild Date: today"
    );
}

#[test]
fn usage_slash_responses_still_use_command_panel() {
    let mut app = test_app();
    let response = "Usage: /model [list|route]";

    app.show_slash_response("/model nope", response);

    assert!(app.conversation.segments().is_empty());
    let panel = app.command_panel.as_ref().expect("command panel");
    assert_eq!(panel.title, "command · /model nope");
    assert_eq!(panel.body, response);
}

#[test]
fn active_menu_display_commands_open_returnable_command_panel() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_skills_menu().expect("skills menu opens");

    let result = app.execute_active_menu_command("/version".to_string(), &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.active_menu.is_some(),
        "menu remains underneath output panel"
    );
    assert!(app.conversation.segments().is_empty());
    let panel = app.command_panel.as_ref().expect("command output panel");
    assert_eq!(panel.source.as_deref(), Some("/version"));
    assert_eq!(
        panel.return_target.map(|target| target.label()),
        Some("menu")
    );
    assert!(panel.body.starts_with("Version\n"));
}

#[test]
fn returnable_command_panel_escape_preserves_underlying_menu() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_skills_menu().expect("skills menu opens");
    app.execute_active_menu_command("/version".to_string(), &tx);

    app.close_command_panel_to_return_target();

    assert!(app.command_panel.is_none());
    assert!(app.active_menu.is_some(), "Esc should return to the menu");
}

#[test]
fn returnable_command_panel_stack_close_clears_menu_target() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_skills_menu().expect("skills menu opens");
    app.execute_active_menu_command("/version".to_string(), &tx);

    app.close_command_panel_stack();

    assert!(app.command_panel.is_none());
    assert!(
        app.active_menu.is_none(),
        "q should close the whole menu output stack"
    );
}

#[test]
fn returnable_command_panel_scroll_does_not_move_underlying_menu() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_skills_menu().expect("skills menu opens");
    let selected_before = app.active_menu.as_ref().expect("menu").state.selected_row;
    app.execute_active_menu_command("/version".to_string(), &tx);

    app.command_panel.as_mut().expect("panel").scroll_down(20);

    assert_eq!(
        app.active_menu.as_ref().expect("menu").state.selected_row,
        selected_before
    );
    assert!(app.command_panel.as_ref().expect("panel").scroll > 0);
}

#[test]
fn slash_version_displays_multiline_build_info() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/version", &tx);

    match result {
        SlashResult::Display(text) => {
            assert!(text.starts_with("Version\n"), "got: {text}");
            assert!(text.contains("Omegon:"), "got: {text}");
            assert!(text.contains("Git SHA:"), "got: {text}");
            assert!(text.contains("Build Date:"), "got: {text}");
            assert!(!super::should_toast_slash_response(&text));
        }
        other => panic!("/version should display version info, got: {other:?}"),
    }
}

#[test]
fn quit_aliases_are_advertised_and_handled() {
    let mut app = test_app();
    let tx = test_tx();

    for command in ["q", "quit", "exit"] {
        assert!(
            crate::command_registry::BUILTIN_COMMANDS
                .iter()
                .any(|entry| entry.name == command),
            "/{command} should be advertised"
        );
        let result = app.handle_slash_command(&format!("/{command}"), &tx);
        assert!(
            matches!(result, SlashResult::Quit),
            "/{command} got {result:?}"
        );
    }
}

#[test]
fn ctrl_g_presentation_cycle_includes_active() {
    let mut policy = UiPresentationPolicy::om();
    policy = policy.next();
    assert_eq!(policy.preset_name(), "active");
    policy = policy.next();
    assert_eq!(policy.preset_name(), "full");
    policy = policy.next();
    assert_eq!(policy.preset_name(), "om");

    let custom_surfaces = UiSurfaces {
        dashboard: false,
        instruments: false,
        footer: true,
        activity: true,
    };
    let custom = UiPresentationPolicy::active().with_surfaces(custom_surfaces);
    assert_eq!(custom.preset_name(), "custom");
    assert_eq!(custom.next().preset_name(), "full");
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
        !app.ui_surfaces.footer,
        "slim mode surface hiding should leave footer hidden unless explicitly shown"
    );
    let result = app.handle_slash_command("/ui toggle dash", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(app.ui_surfaces.dashboard);

    let result = app.handle_slash_command("/ui toggle tools", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    assert!(app.ui_surfaces.instruments);

    let result = app.handle_slash_command("/ui toggle tree", &tx);
    assert!(
        matches!(result, SlashResult::Display(ref text) if text.contains("Unknown UI surface: tree"))
    );

    let result = app.handle_slash_command("/ui toggle status", &tx);
    assert!(
        matches!(result, SlashResult::Display(ref text) if text.contains("Unknown UI surface: status"))
    );
}

#[test]
fn empty_editor_hint_mentions_ui_surfaces_when_dashboard_hidden() {
    let mut app = test_app();
    app.apply_ui_preset(UiSurfaces::lean());
    let rendered = render_app_to_string(&mut app, 100, 20);
    assert!(rendered.contains("/ui surfaces"), "{rendered}");
    assert!(!rendered.contains("^D tree"), "{rendered}");
}

#[test]
fn slash_ui_opens_shared_menu() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/ui", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("ui menu");
    assert_eq!(menu.projection.id, "ui");
    assert!(
        menu.state
            .visible_rows(&menu.projection)
            .iter()
            .any(|row| row.row.id == "ui.surface.dashboard")
    );
}

#[test]
fn slash_ui_surfaces_opens_shared_menu() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/ui surfaces", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "ui")
    );
}

#[test]
fn slash_ui_status_preserves_text_readout() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/ui status", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected display");
    };
    assert!(text.contains("dashboard"), "{text}");
    assert!(text.contains("instruments"), "{text}");
    assert!(text.contains("footer"), "{text}");
}

#[test]
fn ui_menu_toggle_refreshes_menu_state() {
    let mut app = test_app();
    let tx = test_tx();
    app.apply_ui_preset(UiSurfaces::lean());
    app.open_ui_menu();
    {
        let menu = app.active_menu.as_mut().expect("ui menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "ui.surface.dashboard")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_primary_action(&menu.projection))
        .expect("dashboard toggle");

    assert!(matches!(
        app.execute_active_menu_action(action, &tx),
        SlashResult::Handled
    ));

    let menu = app.active_menu.as_ref().expect("ui menu refreshed");
    let row = menu
        .state
        .visible_rows(&menu.projection)
        .into_iter()
        .find(|row| row.row.id == "ui.surface.dashboard")
        .expect("dashboard row");
    assert_eq!(row.row.value.as_deref(), Some("on"));
}

#[test]
fn ui_menu_preset_hotkey_refreshes_menu_state() {
    let mut app = test_app();
    let tx = test_tx();
    app.apply_ui_preset(UiSurfaces::lean());
    app.open_ui_menu();
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action_for_key(&menu.projection, 'f'))
        .expect("global full action");

    assert_eq!(action.command.as_deref(), Some("/ui full"));
    assert!(matches!(
        app.execute_active_menu_action(action, &tx),
        SlashResult::Handled
    ));

    let menu = app.active_menu.as_ref().expect("ui menu refreshed");
    let row = menu
        .state
        .visible_rows(&menu.projection)
        .into_iter()
        .find(|row| row.row.id == "ui.preset.full")
        .expect("full row");
    assert_eq!(row.row.value.as_deref(), Some("active"));
}

#[test]
fn ui_menu_toggle_rows_use_shared_commands() {
    let mut app = test_app();
    app.open_ui_menu();
    let menu = app.active_menu.as_mut().expect("ui menu");
    assert!(
        menu.state
            .select_row_by_id(&menu.projection, "ui.surface.dashboard")
    );

    assert_eq!(
        menu.state.selected_command(&menu.projection).as_deref(),
        Some("/ui toggle dashboard")
    );
}

#[test]
fn slash_help_opens_command_inventory_menu() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/help", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("command menu");
    assert_eq!(menu.projection.id, "commands");
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(rows.iter().any(|row| row.row.label == "/ui"));
    assert!(rows.iter().any(|row| row.row.label == "/stats"));
    assert!(
        menu.projection
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Slash command inventory"))
    );
}

#[test]
fn empty_editor_hint_mentions_tool_detail_hotkey() {
    let mut app = test_app();
    let rendered = render_app_to_string(&mut app, 100, 20);
    assert!(rendered.contains("^O/Tab details"), "{rendered}");
    assert!(!rendered.contains("^D tree"), "{rendered}");
    assert!(rendered.contains("/ui surfaces"), "{rendered}");
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
fn slash_update_channel_without_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/update channel", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.selector.is_some(),
        "expected update channel selector to open"
    );
    assert_eq!(app.selector_kind, Some(SelectorKind::UpdateChannel));
}

#[test]
fn slash_update_channel_changes_setting() {
    let mut app = test_app();
    let tx = test_tx();
    // RC is deprecated — redirects to stable
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
    app.settings.lock().unwrap().update_channel = "stable".to_string();
    let result = app.handle_slash_command("/update", &tx);
    if let SlashResult::Display(text) = result {
        assert!(text.contains("0.15.3-rc.7"), "{text}");
        assert!(text.contains("/update install"), "{text}");
        assert!(text.contains("/update channel [stable|nightly]"), "{text}");
        assert!(text.contains("rc"), "{text}");
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
        assert!(text.contains("No update is currently cached"), "{text}");
        assert!(text.contains("Checking GitHub now"), "{text}");
        assert!(text.contains("/update channel nightly"), "{text}");
        assert!(text.contains("/update channel stable"), "{text}");
        // RC is no longer listed — only stable and nightly
        assert!(
            !text.contains("channel rc"),
            "RC should not appear in help: {text}"
        );
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
        assert!(text.contains("Checking for updates now"), "{text}");
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
            request: crate::control_runtime::ControlRequest::WorkspaceNew { ref label },
            ..
        } if label == "docs-pass" => {}
        other => panic!("expected workspace new request, got {other:?}"),
    }
}

#[test]
fn slash_workspace_role_without_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/workspace role", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.selector.is_some(),
        "expected workspace role selector to open"
    );
    assert_eq!(app.selector_kind, Some(SelectorKind::WorkspaceRole));
}

#[test]
fn workspace_role_selector_confirm_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.handle_slash_command("/workspace role", &tx);
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "release")
        .expect("release option present");
    selector.cursor = index;

    let message = app.confirm_selector(&tx).expect("confirmation message");
    assert!(message.contains("Workspace role → release"), "{message}");

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
fn slash_workspace_kind_without_args_opens_selector() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/workspace kind", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.selector.is_some(),
        "expected workspace kind selector to open"
    );
    assert_eq!(app.selector_kind, Some(SelectorKind::WorkspaceKind));
}

#[test]
fn workspace_kind_selector_confirm_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.handle_slash_command("/workspace kind", &tx);
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "vault")
        .expect("vault option present");
    selector.cursor = index;

    let message = app.confirm_selector(&tx).expect("confirmation message");
    assert!(message.contains("Workspace kind → vault"), "{message}");

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
fn slash_help_all_returns_display() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/help all", &tx);
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
        TuiCommand::ExecuteControl { request, .. } => {
            assert!(matches!(
                request,
                crate::control_runtime::ControlRequest::SessionStatsView
            ));
        }
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
        TuiCommand::ExecuteControl { request, .. } => {
            assert!(matches!(
                request,
                crate::control_runtime::ControlRequest::StatusView
            ));
        }
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
fn slash_context_no_args_opens_menu() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/context", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_none());
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "context")
    );
}

#[test]
fn context_selector_confirm_enqueues_set_context_class() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.open_context_selector();
    let selector = app.selector.as_mut().expect("selector should be open");
    let index = selector
        .options
        .iter()
        .position(|o| o.value == "Extended")
        .expect("Extended option present");
    selector.cursor = index;

    let message = app
        .confirm_selector(&tx)
        .expect("selector confirmation should return message");
    assert!(
        message.contains("Context policy → Extended"),
        "unexpected message: {message}"
    );

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetContextClass { class },
            ..
        } => assert_eq!(class, crate::settings::ContextClass::Extended),
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

    let _cwd = push_current_dir(dir.path());

    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/persona", &tx);

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

    let _cwd = push_current_dir(dir.path());

    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/tone", &tx);

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
fn slash_resume_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/resume 2026-session", &tx);
    assert!(matches!(result, SlashResult::Display(msg) if msg == "Resuming session 2026-session…"));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ResumeSession { id },
            ..
        } => assert_eq!(id, "2026-session"),
        other => panic!("expected resume session request, got {other:?}"),
    }
}

#[test]
fn slash_sessions_opens_shared_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/sessions", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("sessions menu");
    assert_eq!(menu.projection.id, "sessions");
    assert!(
        menu.state
            .visible_rows(&menu.projection)
            .iter()
            .any(|row| row.row.id == "sessions.empty" || row.row.id.starts_with("session."))
    );
}

#[test]
fn slash_sessions_all_preserves_text_readout() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/sessions all", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(app.active_menu.is_none());
    assert!(matches!(
        rx.try_recv().expect("list sessions"),
        TuiCommand::ListSessions { .. }
    ));
}

#[test]
fn slash_sessions_list_preserves_text_readout() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/sessions list", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(app.active_menu.is_none());
    assert!(matches!(
        rx.try_recv().expect("list sessions"),
        TuiCommand::ListSessions { .. }
    ));
}

#[test]
fn sessions_menu_rows_resume_by_session_id() {
    let mut app = test_app();
    let tmp = tempfile::tempdir().expect("tempdir");
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    let dir = crate::session::sessions_dir(tmp.path()).expect("sessions dir");
    std::fs::create_dir_all(&dir).expect("session dir");
    let session_id = "2026-01-02T03-04-05_deadbeef";
    let session_path = dir.join(format!("{session_id}.json"));
    std::fs::write(&session_path, "{}").expect("session");
    let meta = crate::session::SessionMeta {
        session_id: session_id.into(),
        cwd: tmp.path().to_string_lossy().to_string(),
        created_at: "2026:01:02 03:04:05".into(),
        turns: 3,
        tool_calls: 7,
        description: "Resume target".into(),
        friendly_name: "quiet_anchor".into(),
        last_prompt_snippet: "last prompt".into(),
    };
    std::fs::write(
        session_path.with_extension("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .expect("meta");

    app.open_sessions_menu();
    let menu = app.active_menu.as_ref().expect("sessions menu");
    let row = menu
        .state
        .visible_rows(&menu.projection)
        .into_iter()
        .find(|row| row.row.id == format!("session.{session_id}"))
        .expect("session row");

    assert_eq!(row.row.label, "quiet_anchor");
    assert!(row.row.description.contains("Resume target"));
    assert_eq!(
        row.row
            .primary_action
            .as_ref()
            .and_then(|action| action.command.as_deref()),
        Some("/sessions resume 2026-01-02T03-04-05_deadbeef")
    );
}

#[test]
fn slash_sessions_resume_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/sessions resume abc123", &tx);
    assert!(matches!(result, SlashResult::Display(msg) if msg == "Resuming session abc123…"));

    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ResumeSession { id },
            ..
        } => assert_eq!(id, "abc123"),
        other => panic!("expected resume session request, got {other:?}"),
    }
}

#[test]
fn slash_auth_no_args_opens_provider_menu() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/auth", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("auth menu");
    assert_eq!(menu.projection.id, "auth");
    assert!(
        menu.state
            .visible_rows(&menu.projection)
            .iter()
            .any(|row| row.row.id.starts_with("auth.provider."))
    );
}

#[test]
fn slash_auth_status_preserves_status_command() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    let result = app.handle_slash_command("/auth status", &tx);

    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("auth status command") {
        TuiCommand::AuthStatus { .. } => {}
        other => panic!("expected auth status command, got {other:?}"),
    }
}

#[test]
fn slash_auth_login_redirects_to_top_level_login() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/auth login anthropic", &tx);
    assert!(matches!(result, SlashResult::Handled));
}

#[test]
fn slash_login_provider_opens_hidden_secret_input_for_api_key_provider() {
    let mut app = test_app();
    let tx = test_tx();
    let result = app.handle_slash_command("/login openai", &tx);

    assert!(matches!(result, SlashResult::Display(_)));
    assert!(matches!(
        app.editor.mode(),
        super::editor::EditorMode::SecretInput { .. }
    ));
    assert!(app.active_menu.is_none());
    assert!(app.command_panel.is_none());
}

#[test]
fn model_menu_current_row_closes_menu_before_opening_selector() {
    let mut app = test_app();
    app.open_model_menu();
    {
        let menu = app.active_menu.as_mut().expect("model menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "model.current")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection))
        .expect("model selector action");

    assert!(matches!(
        app.execute_active_menu_action(action, &test_tx()),
        SlashResult::Handled
    ));
    assert!(
        app.active_menu.is_none(),
        "selector must receive arrow keys"
    );
    assert!(app.selector.is_some(), "model selector should be open");
    assert!(matches!(app.selector_kind, Some(SelectorKind::Model)));
}

#[test]
fn slash_model_list_opens_model_selector_instead_of_text_dump() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/model list", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.selector.is_some(),
        "/model list should open interactive selector"
    );
    assert!(matches!(app.selector_kind, Some(SelectorKind::Model)));
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
fn memory_menu_includes_action_rows() {
    let mut app = test_app();
    app.open_memory_menu();
    let menu = app.active_menu.as_mut().expect("memory menu");
    menu.state.active_tab = "actions".into();

    let ids: Vec<_> = menu
        .state
        .visible_rows(&menu.projection)
        .into_iter()
        .map(|row| row.row.id.as_str())
        .collect();

    assert!(ids.contains(&"memory.recall"), "{ids:?}");
    assert!(ids.contains(&"memory.list"), "{ids:?}");
    assert!(ids.contains(&"memory.focus"), "{ids:?}");
    assert!(ids.contains(&"memory.release"), "{ids:?}");
    assert!(ids.contains(&"memory.compact"), "{ids:?}");
}

#[test]
fn memory_menu_argument_rows_prime_editor() {
    let mut app = test_app();
    app.open_memory_menu();
    {
        let menu = app.active_menu.as_mut().expect("memory menu");
        menu.state.active_tab = "actions".into();
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "memory.focus")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection))
        .expect("focus action");

    app.execute_active_menu_action(action, &test_tx());

    assert_eq!(app.editor.render_text(), "/memory focus ");
    assert!(app.active_menu.is_none());
}

#[test]
fn memory_menu_compact_requires_confirmation() {
    let mut app = test_app();
    app.open_memory_menu();
    let menu = app.active_menu.as_mut().expect("memory menu");
    menu.state.active_tab = "actions".into();
    assert!(
        menu.state
            .select_row_by_id(&menu.projection, "memory.compact")
    );

    let action = menu
        .state
        .selected_action(&menu.projection)
        .expect("compact action");

    assert_eq!(action.command.as_deref(), Some("/memory compact"));
    assert!(action.requires_confirmation);
}

#[test]
fn slash_memory_opens_shared_menu() {
    let mut app = test_app();
    app.footer_data.total_facts = 18;
    app.footer_data.injected_facts = 3;
    app.footer_data.working_memory = 4;
    app.footer_data.memory_tokens_est = 1200;
    app.footer_data.harness.memory.project_facts = 11;
    app.footer_data.harness.memory.persona_facts = 2;
    app.footer_data.harness.memory.episodes = 5;
    app.footer_data.harness.memory.active_persona_mind = Some("Engineer".into());
    let tx = test_tx();

    let result = app.handle_slash_command("/memory", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("memory menu");
    assert_eq!(menu.projection.id, "memory");
    assert!(
        menu.projection
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Injected: 3"))
    );
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(
        rows.iter()
            .any(|row| row.row.id == "memory.injected" && row.row.value.as_deref() == Some("3"))
    );
    assert!(
        rows.iter()
            .any(|row| row.row.id == "memory.working_set" && row.row.label == "Working-set facts")
    );
    assert!(rows.iter().any(|row| row.row.id == "memory.persona"
        && row.row.metadata.iter().any(|m| m.contains("Engineer"))));
}

#[test]
fn slash_memory_status_preserves_text_readout() {
    let mut app = test_app();
    app.footer_data.total_facts = 18;
    app.footer_data.injected_facts = 3;
    app.footer_data.working_memory = 4;
    app.footer_data.memory_tokens_est = 1200;
    app.footer_data.harness.memory.project_facts = 11;
    app.footer_data.harness.memory.persona_facts = 2;
    app.footer_data.harness.memory.episodes = 5;
    app.footer_data.harness.memory.active_persona_mind = Some("Engineer".into());
    let tx = test_tx();

    let result = app.handle_slash_command("/memory status", &tx);

    if let SlashResult::Display(text) = result {
        assert!(
            text.contains("Memory Overview"),
            "should show titled memory view: {text}"
        );
        assert!(
            text.contains("Injected"),
            "should show injected facts: {text}"
        );
        assert!(
            text.contains("Project facts"),
            "should show harness memory breakdown: {text}"
        );
        assert!(
            text.contains("Engineer"),
            "should show active persona memory: {text}"
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
            .any(|opt| opt.value == "openai-codex:gpt-5.6-sol"),
        "ChatGPT/Codex-backed GPT-5.6 Sol route should be advertised honestly"
    );
}

#[test]
fn model_selector_options_include_openai_api_choices_when_api_key_is_present() {
    let options = build_model_selector_options(
        "openai:gpt-5.6",
        None,
        Some(("sk-test".into(), false)),
        None,
    );
    assert!(
        options.iter().any(|opt| opt.value == "openai:gpt-5.6"),
        "OpenAI API GPT-5.6 route should be selectable when API creds exist"
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

    let _cwd = push_current_dir(dir.path());

    let mut app = test_app();
    app.open_persona_selector();
    let tx = test_tx();
    let message = app.confirm_selector(&tx);

    assert_eq!(
        message.as_deref(),
        Some("⚙ Persona activated: Test Persona (0 mind facts)")
    );
    let active = app
        .augment_registry
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

    let _cwd = push_current_dir(dir.path());

    let mut app = test_app();
    app.open_tone_selector();
    let tx = test_tx();
    let message = app.confirm_selector(&tx);

    assert_eq!(message.as_deref(), Some("♪ Tone activated: Test Tone"));
    let active = app
        .augment_registry
        .as_ref()
        .and_then(|registry| registry.active_tone())
        .map(|tone| tone.id.as_str());
    assert_eq!(active, Some("dev.test.tone"));
}

// ═══════════════════════════════════════════════════════════════════
// Event handling
// ═══════════════════════════════════════════════════════════════════

#[test]
fn draw_routes_active_cleave_to_workbench_without_instruments() {
    let mut app = test_app();
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = false;
    app.dashboard.cleave = Some(crate::features::cleave::CleaveProgress {
        active: true,
        run_id: "run-1".into(),
        inventory_generation: None,
        total_children: 2,
        completed: 1,
        failed: 0,
        children: vec![crate::features::cleave::ChildProgress {
            label: "ui".into(),
            status: "running".into(),
            failure_kind: None,
            duration_secs: None,
            supervision_mode: None,
            pid: None,
            last_tool: Some("bash".into()),
            last_tool_activity: None,
            last_turn: Some(1),
            tasks: vec![],
            tasks_done: 0,
            started_at: None,
            last_activity_at: None,
            tokens_in: 0,
            tokens_out: 0,
            runtime: None,
        }],
        total_tokens_in: 0,
        total_tokens_out: 0,
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(rendered.contains("cleave"), "{rendered}");
    assert!(rendered.contains("ui"), "{rendered}");
    assert!(rendered.contains("running"), "{rendered}");
}

#[test]
fn draw_routes_active_delegate_to_workbench_without_instruments() {
    let mut app = test_app();
    app.apply_ui_presentation(UiPresentationPolicy::active());
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = false;
    app.dashboard.delegate = Some(crate::features::delegate::DelegateProgress {
        active: true,
        running: 1,
        completed: 2,
        failed: 0,
        pending_results: 0,
        children: vec![crate::features::delegate::DelegateProgressChild {
            task_id: "delegate_1".into(),
            label: "scout".into(),
            status: "running".into(),
            result_viewed: true,
            last_tool: Some("read".into()),
            last_tool_activity: None,
            last_turn: Some(1),
            started_at: None,
            completed_at: None,
            result_summary: None,
            failure_kind: None,
            tasks: vec![],
            tasks_done: 0,
            route_decision: None,
        }],
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(rendered.contains("delegate"), "{rendered}");
    assert!(rendered.contains("scout"), "{rendered}");
    assert!(rendered.contains("running"), "{rendered}");
}

#[test]
fn draw_routes_failed_delegate_summary_to_workbench_without_instruments() {
    let mut app = test_app();
    app.apply_ui_presentation(UiPresentationPolicy::active());
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = false;
    app.dashboard.delegate = Some(crate::features::delegate::DelegateProgress {
        active: true,
        running: 0,
        completed: 0,
        failed: 1,
        pending_results: 0,
        children: vec![crate::features::delegate::DelegateProgressChild {
            task_id: "delegate_2".into(),
            label: "delegate_2".into(),
            status: "failed".into(),
            result_viewed: false,
            last_tool: Some("bash".into()),
            last_tool_activity: None,
            last_turn: Some(3),
            started_at: None,
            completed_at: None,
            result_summary: Some("idle timeout — no output for 120s".into()),
            failure_kind: Some(crate::features::delegate::DelegateChildFailureKind::Unknown),
            tasks: vec![],
            tasks_done: 0,
            route_decision: None,
        }],
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(rendered.contains("delegate"), "{rendered}");
    assert!(rendered.contains("delegate_2"), "{rendered}");
    assert!(rendered.contains("failed"), "{rendered}");
    assert!(rendered.contains("idle timeout"), "{rendered}");
}

#[test]
fn draw_truncates_failed_delegate_summary_in_workbench() {
    let mut app = test_app();
    app.apply_ui_presentation(UiPresentationPolicy::active());
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = false;
    app.dashboard.delegate = Some(crate::features::delegate::DelegateProgress {
        active: true,
        running: 0,
        completed: 0,
        failed: 1,
        pending_results: 0,
        children: vec![crate::features::delegate::DelegateProgressChild {
            task_id: "delegate_2".into(),
            label: "delegate_2".into(),
            status: "failed".into(),
            result_viewed: false,
            last_tool: Some("bash".into()),
            last_tool_activity: None,
            last_turn: Some(3),
            started_at: None,
            completed_at: None,
            result_summary: Some("idle timeout — no output for 120s while validating renderer-neutral operation projection rows".into()),
            failure_kind: Some(crate::features::delegate::DelegateChildFailureKind::Unknown),
            tasks: vec![],
            tasks_done: 0,
            route_decision: None,
        }],
    });

    let rendered = render_app_to_string(&mut app, 70, 28);

    assert!(rendered.contains("delegate"), "{rendered}");
    assert!(rendered.contains("idle"), "{rendered}");
    assert!(rendered.contains('…'), "{rendered}");
}

#[test]
fn draw_clears_stale_completed_cleave_snapshot_from_tools_panel() {
    let mut app = test_app();
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = true;
    app.instrument_panel
        .set_cleave_progress(Some(crate::features::cleave::CleaveProgress {
            active: false,
            run_id: "done-run".into(),
            inventory_generation: None,
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
            inventory_generation: None,
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
        inventory_generation: None,
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
        context_class: "Extended".into(),
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
    assert_eq!(app.footer_data.harness.context_class, "Extended");
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
fn footer_instrument_layout_renders_only_inference_and_tools_panels() {
    let mut app = test_app();
    app.ui_surfaces.footer = true;
    app.ui_surfaces.instruments = true;
    app.footer_data.provider_connected = true;
    app.footer_data.model_id = "anthropic:claude-sonnet-4-6".into();
    app.footer_data.model_provider = "anthropic".into();
    app.footer_data.context_percent = 39.0;
    app.footer_data.context_window = 272_000;
    app.footer_data.harness.memory.project_facts = 704;
    app.footer_data.harness.memory.working_facts = 0;
    app.footer_data.harness.memory.episodes = 624;
    app.instrument_panel.update_mind_facts(704, 0, 624, 0.08);
    app.instrument_panel.update_turn_tokens(
        105_100,
        538,
        0,
        omegon_traits::ContextComposition {
            conversation_tokens: 105_100,
            system_tokens: 538,
            memory_tokens: 0,
            tool_schema_tokens: 0,
            tool_history_tokens: 0,
            thinking_tokens: 0,
            free_tokens: 166_362,
            ..Default::default()
        },
        272_000,
    );
    app.instrument_panel.update_telemetry(
        39.0,
        272_000,
        "medium",
        Some((0, crate::tui::instruments::WaveDirection::Right)),
        true,
        0.016,
    );

    let rendered = render_app_to_string(&mut app, 140, 20);
    assert!(
        rendered.contains("inference"),
        "expected inference panel: {rendered}"
    );
    assert!(
        rendered.contains("tools"),
        "expected tools panel: {rendered}"
    );

    assert!(
        rendered.contains("┌ inference"),
        "missing inference panel: {rendered}"
    );
    assert!(
        rendered.contains("┌ tools"),
        "missing tools panel: {rendered}"
    );
    assert!(
        !rendered.contains("┌ engine"),
        "engine telemetry belongs in the slim status sidecar, not the instrument footer: {rendered}"
    );
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

    for command in crate::command_registry::BUILTIN_COMMANDS {
        let name = command.name;
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

    let known_names: std::collections::HashSet<&str> = crate::command_registry::BUILTIN_COMMANDS
        .iter()
        .map(|command| command.name)
        .collect();

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
fn skills_registry_advertises_help_aliases() {
    let definitions = crate::command_registry::builtin_command_definitions();
    for name in ["skills", "skill"] {
        let definition = definitions
            .iter()
            .find(|definition| definition.name == name)
            .expect("skills registry row");
        assert!(definition.availability.cli, "{name} should be CLI visible");
        assert!(definition.availability.acp, "{name} should be ACP visible");
        for subcommand in ["--help", "-h", "help"] {
            assert!(
                definition.subcommands.iter().any(|item| item == subcommand),
                "{name} should advertise {subcommand}"
            );
        }
    }
}

#[test]
fn slash_skills_opens_structured_menu() {
    for command in ["/skills", "/skills list", "/skill", "/skill list"] {
        let mut app = test_app();
        let (tx, mut rx) = test_tx_with_rx();

        let result = app.handle_slash_command(command, &tx);
        assert!(matches!(result, SlashResult::Handled), "{command}");
        assert!(rx.try_recv().is_err(), "{command} is handled in-TUI");

        let menu = app.active_menu.as_ref().expect("skills menu opened");
        assert_eq!(menu.projection.id, "skills");
        assert!(
            menu.state
                .visible_rows(&menu.projection)
                .iter()
                .any(|row| row.row.label == "code-act"),
            "{command} should show skill rows"
        );
        assert!(
            !menu
                .state
                .visible_rows(&menu.projection)
                .iter()
                .any(|row| row.row.label.contains("/skills get")),
            "{command} should not render command text as inventory rows"
        );
    }
}

#[test]
fn slash_skills_menu_lists_skills_before_actions() {
    let mut app = test_app();
    let (tx, _rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/skills", &tx);
    assert!(matches!(result, SlashResult::Handled));

    let menu = app.active_menu.as_ref().expect("skills menu opened");
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(rows.len() > 2, "expected skill inventory plus actions");
    assert_eq!(rows[0].group_id, "skills");
    assert_ne!(rows[0].row.kind, crate::surfaces::menu::MenuRowKind::Action);
    assert!(
        rows.iter()
            .any(|row| row.group_id == "actions" && row.row.id == "skills.reload")
    );
}

#[test]
fn slash_skills_help_keeps_command_syntax_out_of_inventory() {
    assert_eq!(
        canonical_slash_command("skills", "--help"),
        Some(CanonicalSlashCommand::SkillsHelp)
    );
    assert_eq!(
        canonical_slash_command("skills", "-h"),
        Some(CanonicalSlashCommand::SkillsHelp)
    );
    assert_eq!(
        canonical_slash_command("skills", "help"),
        Some(CanonicalSlashCommand::SkillsHelp)
    );
}

#[test]
fn slash_skills_help_displays_meta_use_without_opening_inventory() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/skills --help", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected display help");
    };

    assert!(text.contains("Usage: /skills"));
    assert!(text.contains("/skills opens the active skills inventory menu"));
    assert!(text.contains("/skills get <name>"));
    assert!(text.contains("/skills get <name>"));
    assert!(app.active_menu.is_none());
}

#[test]
fn slash_skills_menu_rows_have_operator_expected_labels_and_values() {
    let mut app = test_app();
    let (tx, _rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/skills", &tx);
    assert!(matches!(result, SlashResult::Handled));

    let menu = app.active_menu.as_ref().expect("skills menu opened");
    let rows = menu.state.visible_rows(&menu.projection);
    let code_act = rows
        .iter()
        .find(|row| row.row.label == "code-act")
        .expect("code-act skill row");
    assert_eq!(
        code_act
            .row
            .primary_action
            .as_ref()
            .unwrap()
            .command
            .as_deref(),
        None
    );
    assert_eq!(
        code_act
            .row
            .primary_action
            .as_ref()
            .unwrap()
            .target_row_id
            .as_deref(),
        Some("skills.code-act")
    );
    assert!(
        code_act
            .row
            .value
            .as_deref()
            .is_some_and(|value| value.contains("Enter: details"))
    );
    assert!(
        code_act
            .row
            .value
            .as_deref()
            .is_some_and(|value| value.contains("i: install"))
    );
    let full_inspect = code_act
        .row
        .actions
        .iter()
        .find(|action| action.key.as_deref() == Some("g"))
        .expect("full inspect shortcut");
    assert_eq!(
        full_inspect.command.as_deref(),
        Some("/skills get code-act")
    );
}

#[test]
fn slash_skills_menu_exposes_executable_primary_commands() {
    let mut app = test_app();
    let (tx, _rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/skills", &tx);
    assert!(matches!(result, SlashResult::Handled));

    let menu = app.active_menu.as_mut().expect("skills menu opened");
    menu.state.enter_search();
    for ch in "skills.reload".chars() {
        menu.state.push_filter_char(&menu.projection, ch);
    }

    assert_eq!(
        menu.state.selected_command(&menu.projection).as_deref(),
        Some("/skills reload")
    );
}

#[test]
fn slash_help_command_registry_converts_to_menu_projection() {
    let menu = crate::surfaces::command_menu::command_menu_projection(
        crate::command_registry::builtin_command_definitions(),
        Vec::new(),
        &[],
    );
    let projection =
        crate::surfaces::menu::MenuProjection::from_command_menu("commands", "Commands", menu);

    let row = projection.tabs[0].groups[0]
        .rows
        .iter()
        .find(|row| row.id == "help")
        .expect("help row");
    assert_eq!(
        row.primary_action.as_ref().unwrap().command.as_deref(),
        Some("/help")
    );
    assert!(row.availability.unwrap().tui);
    assert!(row.availability.unwrap().cli);
    assert!(row.availability.unwrap().acp);
    assert_eq!(
        row.safety.unwrap().class,
        omegon_traits::CommandSafetyClass::ReadOnly
    );
}

#[test]
fn skill_event_segment_projects_and_exports_single_line() {
    let event = omegon_traits::SkillActivationEvent {
        active_ref: "security".to_string(),
        activation: Some("always".to_string()),
        reason: "always".to_string(),
        matched_signals: Vec::new(),
        suppressing: vec!["bundled:security".to_string()],
        resolution: "active".to_string(),
        recommendation: None,
        injected: true,
    };
    let segment = Segment::skill_event(&event);

    assert!(matches!(segment.content, SegmentContent::SkillEvent { .. }));
    assert_eq!(
        segment.plain_text(),
        "★ skill · security · always · active · suppressing bundled:security"
    );
    assert!(matches!(
        segment.projection().kind,
        crate::surfaces::conversation::ConversationSegmentKind::Skill(_)
    ));
}

#[test]
fn skills_reload_pushes_skill_event_segments() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join(".omegon/skills/reload-event-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: reload-event-skill
description: Reload event skill fixture
activation: always
---

# Reload Event Skill
"#,
    )
    .unwrap();
    let _cwd = push_current_dir(dir.path());

    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    let result = app.handle_slash_command("/skills reload", &tx);

    assert!(matches!(result, SlashResult::Display(_)));
    assert!(rx.try_recv().is_err(), "reload is handled in-TUI");
    assert!(app.conversation.segments().iter().any(|segment| matches!(
        segment.content,
        SegmentContent::SkillEvent { ref active_ref, .. } if active_ref.contains("reload-event-skill")
    )));
}

#[test]
fn slash_skills_reload_displays_current_session_reload() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/skills reload", &tx);
    match result {
        SlashResult::Display(message) => {
            assert!(message.contains("Skills reloaded"), "{message}");
            assert!(message.contains("subsequent model requests"), "{message}");
        }
        other => panic!("expected reload display, got: {other:?}"),
    }
    assert!(rx.try_recv().is_err(), "reload is handled in-TUI");
}

#[test]
fn slash_extension_opens_runtime_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/extension", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "extension-runtime")
    );
}

#[test]
fn slash_ext_opens_runtime_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/ext", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "extension-runtime")
    );
}

#[test]
fn slash_runtime_opens_runtime_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/runtime", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "extension-runtime")
    );
}

#[test]
fn extension_view_preserves_text_readout() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/extension view", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(app.active_menu.is_none());
    match rx.try_recv().expect("extension view command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ExtensionView,
            ..
        } => {}
        other => panic!("expected extension view request, got {other:?}"),
    }
}

#[test]
fn runtime_inventory_status_queues_shared_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    assert!(matches!(
        app.handle_slash_command("/runtime status", &tx),
        SlashResult::Handled
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::RuntimeInventoryStatus,
            ..
        })
    ));
}

#[test]
fn runtime_refresh_aliases_canonicalize() {
    for args in ["refresh", "reload", "restart", "hot-restart"] {
        assert_eq!(
            crate::tui::canonical_slash_command("runtime", args),
            Some(crate::tui::CanonicalSlashCommand::RuntimeSubstrateRefresh),
            "runtime {args}"
        );
    }
}

#[test]
fn runtime_refresh_menu_action_requires_confirmation() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();
    app.open_extension_runtime_menu();
    {
        let menu = app.active_menu.as_mut().expect("runtime menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "runtime.refresh")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_primary_action(&menu.projection))
        .expect("runtime refresh action");

    let first = app.execute_active_menu_action(action.clone(), &tx);
    assert!(matches!(first, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "extension-runtime")
    );
    assert!(app.command_panel.is_none());
    assert_eq!(
        app.pending_menu_confirmation.as_deref(),
        Some("runtime.refresh.primary")
    );

    let second = app.execute_active_menu_action(action, &tx);
    assert!(matches!(second, SlashResult::Handled));
    assert!(app.command_panel.is_none());
    assert!(matches!(
        rx.try_recv(),
        Ok(TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::RuntimeSubstrateRefresh,
            ..
        })
    ));
}

#[test]
fn extension_update_menu_action_requires_confirmation() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_extension_runtime_menu();
    {
        let menu = app.active_menu.as_mut().expect("runtime menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "extension.update")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_primary_action(&menu.projection))
        .expect("extension update action");

    let first = app.execute_active_menu_action(action.clone(), &tx);
    assert!(matches!(first, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "extension-runtime")
    );
    assert!(app.command_panel.is_none());
    assert_eq!(
        app.pending_menu_confirmation.as_deref(),
        Some("extension.update.primary")
    );
}

#[test]
fn extension_search_menu_row_primes_editor_for_query() {
    let mut app = test_app();
    app.open_extension_runtime_menu();
    {
        let menu = app.active_menu.as_mut().expect("runtime menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "extension.search")
        );
    }

    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_primary_action(&menu.projection))
        .expect("extension search action");
    assert!(matches!(
        app.execute_active_menu_action(action, &test_tx()),
        SlashResult::Handled
    ));
    assert_eq!(app.editor.render_text(), "/extension search ");
    assert!(app.active_menu.is_none());
}

#[test]
fn extension_refresh_aliases_execute_shared_runtime_refresh() {
    for command in [
        "/extension refresh",
        "/extension reload",
        "/extension restart",
    ] {
        let mut app = test_app();
        let (tx, mut rx) = test_tx_with_rx();
        assert!(matches!(
            app.handle_slash_command(command, &tx),
            SlashResult::Handled
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(TuiCommand::ExecuteControl {
                request: crate::control_runtime::ControlRequest::RuntimeSubstrateRefresh,
                ..
            })
        ));
    }
}

#[test]
fn slash_runtime_substrate_refresh_queues_shared_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/runtime restart", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(matches!(
        rx.try_recv(),
        Ok(TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::RuntimeSubstrateRefresh,
            ..
        })
    ));
}

#[test]
fn runtime_substrate_refresh_reloads_skill_augments_and_advances_generation() {
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join(".omegon/skills/runtime-refresh-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: runtime-refresh-skill
description: Runtime refresh skill fixture
activation: always
---

# Runtime Refresh Skill

Loaded by runtime substrate refresh.
"#,
    )
    .unwrap();
    let _cwd = push_current_dir(dir.path());

    let mut app = test_app();
    let before_generation = app.runtime_generation;
    assert_eq!(
        app.augment_registry
            .as_ref()
            .map(|registry| registry.skill_count()),
        Some(0)
    );

    let message = app.refresh_runtime_substrate();

    assert_eq!(app.runtime_generation, before_generation + 1);
    let registry = app.augment_registry.as_ref().expect("registry exists");
    assert!(registry.skill_count() > 0);
    assert!(
        registry
            .skill_activation_events()
            .iter()
            .any(|event| event.active_ref.contains("runtime-refresh-skill"))
    );
    assert!(
        message.contains("Active skill directives: 0 ->"),
        "{message}"
    );
    assert!(
        message.contains("partial live refresh completed"),
        "{message}"
    );
}

#[test]
fn secrets_menu_separates_inventory_from_actions() {
    let mut app = test_app();
    app.open_secrets_menu();

    let menu = app.active_menu.as_ref().expect("secrets menu");
    assert_eq!(menu.projection.id, "secrets");
    assert_eq!(menu.projection.tabs[0].id, "inventory");
    assert_eq!(menu.projection.tabs[1].id, "capabilities");
    assert_eq!(menu.projection.tabs[2].id, "actions");
    let inventory_rows = &menu.projection.tabs[0].groups[0].rows;
    assert_eq!(inventory_rows[0].id, "secrets.inventory.unavailable");
    assert!(
        inventory_rows[0]
            .label
            .contains("No secret readiness snapshot")
    );
    assert!(inventory_rows[0].primary_action.is_none());
}

#[test]
fn secrets_menu_actions_tab_contains_prime_editor_rows() {
    let mut app = test_app();
    app.open_secrets_menu();
    let menu = app.active_menu.as_mut().expect("secrets menu");
    menu.state.active_tab = "actions".into();

    let rows: Vec<_> = menu.state.visible_rows(&menu.projection);
    for id in [
        "secrets.set",
        "secrets.recipe.env",
        "secrets.recipe.cmd",
        "secrets.recipe.vault",
        "secrets.get",
        "secrets.delete",
    ] {
        let row = rows.iter().find(|row| row.row.id == id).expect(id);
        let action = row.row.primary_action.as_ref().expect("prime action");
        assert_eq!(
            action.disposition,
            crate::surfaces::menu::MenuActionDisposition::PrimeEditor
        );
        assert!(action.command.is_none());
        assert!(
            action
                .editor_text
                .as_deref()
                .unwrap_or_default()
                .starts_with("/secrets ")
        );
    }
}

#[test]
fn slash_secrets_opens_shared_menu() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/secrets", &tx);
    assert!(matches!(result, SlashResult::Handled));

    assert!(
        rx.try_recv().is_err(),
        "/secrets should not queue control work"
    );
    assert!(app.selector.is_none());
    let menu = app.active_menu.as_ref().expect("secrets menu");
    assert_eq!(menu.projection.id, "secrets");
    assert_eq!(menu.state.active_tab, "inventory");
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(
        rows.iter()
            .any(|row| row.row.id == "secrets.inventory.unavailable")
    );
    assert!(
        rows.iter()
            .all(|row| !row.row.metadata.iter().any(|m| m.contains("super-secret")))
    );
    let menu = app.active_menu.as_mut().expect("secrets menu");
    menu.state.active_tab = "actions".into();
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(rows.iter().any(|row| row.row.id == "secrets.status"));
}

#[test]
fn secrets_menu_status_row_enqueues_execute_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.handle_slash_command("/secrets", &tx);
    let menu = app.active_menu.as_mut().expect("secrets menu");
    menu.state.active_tab = "actions".into();
    assert!(
        menu.state
            .select_row_by_id(&menu.projection, "secrets.status")
    );
    let command = menu
        .state
        .selected_command(&menu.projection)
        .expect("status command");

    assert!(matches!(
        app.execute_active_menu_command(command, &tx),
        SlashResult::Handled
    ));
    match rx.try_recv().expect("queued command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SecretsView,
            ..
        } => {}
        other => panic!("expected secrets view control request, got: {other:?}"),
    }
}

#[test]
fn secrets_menu_template_rows_prime_editor_without_control_request() {
    for (row_id, expected) in [
        ("secrets.set", "/secrets set "),
        ("secrets.recipe.env", "/secrets set "),
        ("secrets.recipe.cmd", "/secrets set "),
        ("secrets.recipe.vault", "/secrets set "),
        ("secrets.get", "/secrets get "),
        ("secrets.delete", "/secrets delete "),
    ] {
        let mut app = test_app();
        let (tx, mut rx) = test_tx_with_rx();

        app.handle_slash_command("/secrets", &tx);
        {
            let menu = app.active_menu.as_mut().expect("secrets menu");
            menu.state.active_tab = "actions".into();
            assert!(menu.state.select_row_by_id(&menu.projection, row_id));
        }

        let action = app
            .active_menu
            .as_ref()
            .and_then(|menu| {
                menu.state
                    .selected_row(&menu.projection)
                    .and_then(|row| row.row.primary_action.clone())
            })
            .expect("prime editor action");
        assert!(matches!(
            app.execute_active_menu_action(action, &tx),
            SlashResult::Handled
        ));
        assert_eq!(app.editor.render_text(), expected);
        assert!(app.active_menu.is_none(), "{row_id} should close the menu");
        assert!(
            rx.try_recv().is_err(),
            "{row_id} should not queue control work"
        );
    }
}

#[test]
fn slash_secrets_configure_opens_shared_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/secrets configure", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_none());
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "secrets")
    );
}

#[test]
fn slash_secrets_unknown_usage_mentions_aliases() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/secrets nope", &tx);

    match result {
        SlashResult::Display(message) => {
            assert!(
                message.contains("status"),
                "usage missing status: {message}"
            );
            assert!(
                message.contains("get <name>"),
                "usage missing get: {message}"
            );
            assert!(
                message.contains("remove"),
                "usage missing remove: {message}"
            );
            assert!(message.contains("rm"), "usage missing rm: {message}");
        }
        other => panic!("expected usage display, got: {other:?}"),
    }
}

#[test]
fn secret_aliases_are_advertised_and_canonical() {
    let secrets = crate::command_registry::BUILTIN_COMMANDS
        .iter()
        .find(|entry| entry.name == "secrets")
        .expect("secrets command advertised");
    for subcommand in ["status", "configure", "remove", "rm"] {
        assert!(
            secrets.subcommands.contains(&subcommand),
            "/secrets {subcommand} should be advertised"
        );
    }

    assert!(matches!(
        canonical_slash_command("secrets", "status"),
        Some(CanonicalSlashCommand::SecretsView)
    ));
    assert!(matches!(
        canonical_slash_command("secrets", "remove FOO"),
        Some(CanonicalSlashCommand::SecretsDelete(name)) if name == "FOO"
    ));
    assert!(matches!(
        canonical_slash_command("secrets", "rm FOO"),
        Some(CanonicalSlashCommand::SecretsDelete(name)) if name == "FOO"
    ));
    assert!(
        canonical_slash_command("secrets", "configure").is_none(),
        "/secrets configure is interactive-only, not canonical SecretsView"
    );
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
    assert!(matches!(
        app.selector_kind,
        Some(super::SelectorKind::VaultConfigure)
    ));
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
fn slash_secrets_set_without_value_opens_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/secrets set", &tx);
    assert!(matches!(result, SlashResult::Handled));
    assert!(app.selector.is_none(), "expected shared menu, not selector");
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "secrets")
    );
}

#[test]
fn slash_secrets_set_name_enters_hidden_secret_input() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/secrets set VAULT_ROOT_TOKEN", &tx);
    assert!(matches!(result, SlashResult::Display(_)));
    let (label, masked) = app
        .editor
        .secret_display()
        .expect("set NAME should enter hidden secret mode");

    assert_eq!(label, "VAULT_ROOT_TOKEN");
    assert!(masked.is_empty(), "secret buffer should start empty");
}

#[test]
fn secret_name_selector_confirm_starts_hidden_secret_input() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_secret_name_selector();
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
fn slash_subagent_status_alias_enqueues_delegate_status_control() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/subagent status", &tx);
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
fn slash_subagents_plural_alias_is_not_supported() {
    let mut app = test_app();
    let (tx, _rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/subagents status", &tx);
    match result {
        SlashResult::Display(message) => {
            assert!(message.contains("Use the explicit singular command: /subagent status"));
        }
        other => panic!("expected explicit singular guidance, got: {other:?}"),
    }
}

#[test]
fn slash_cleave_run_still_uses_bus_path() {
    let mut app = test_app();
    app.bus_commands.push(omegon_traits::CommandDefinition {
        name: "cleave".into(),
        description: "parallel work".into(),
        subcommands: vec!["status".into(), "cancel".into()],
        availability: omegon_traits::CommandAvailability::ALL,
        safety: omegon_traits::CommandSafety::READ_ONLY,
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
fn command_palette_renders_while_agent_active() {
    let mut app = test_app();
    app.agent_active = true;
    app.editor.set_text("/pla");

    let rendered = render_app_to_string(&mut app, 140, 24);

    assert!(rendered.contains("commands"), "{rendered}");
    assert!(rendered.contains("/plan"), "{rendered}");
}

#[test]
fn command_palette_renders_profile_persistence_metadata() {
    let mut app = test_app();
    app.editor.set_text("/think");

    let rendered = render_app_to_string(&mut app, 140, 24);

    assert!(rendered.contains("/think"), "{rendered}");
    assert!(
        rendered.contains("runtime until /profile save"),
        "{rendered}"
    );
}

#[test]
fn hidden_model_aliases_do_not_appear_in_palette() {
    let mut app = test_app();
    app.bus_commands = vec![
        omegon_traits::CommandDefinition {
            name: "sonnet".into(),
            description: "hidden alias".into(),
            subcommands: vec![],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::READ_ONLY,
        },
        omegon_traits::CommandDefinition {
            name: "B".into(),
            description: "visible grade".into(),
            subcommands: vec![],
            availability: omegon_traits::CommandAvailability::ALL,
            safety: omegon_traits::CommandSafety::READ_ONLY,
        },
    ];
    app.editor.set_text("/");
    let matches = app.matching_commands();
    assert!(matches.iter().any(|row| row.name == "B"));
    assert!(!matches.iter().any(|row| row.name == "sonnet"));
}

#[test]
fn command_palette_exposes_registry_metadata_badges() {
    let mut app = test_app();
    app.bus_commands = vec![omegon_traits::CommandDefinition {
        name: "prompt".into(),
        description: "manage prompts".into(),
        subcommands: vec!["list".into()],
        availability: omegon_traits::CommandAvailability::ALL,
        safety: omegon_traits::CommandSafety::QUEUE_MUTATION,
    }];
    app.editor.set_text("/pro");

    let matches = app.matching_commands();
    let prompt = matches
        .iter()
        .find(|row| row.name == "prompt")
        .expect("/prompt should be projected from bus registry");

    assert_eq!(prompt.command, "/prompt");
    assert!(prompt.badges.contains(&"feature".to_string()), "{prompt:?}");
    assert!(prompt.badges.contains(&"queue".to_string()), "{prompt:?}");
    assert!(prompt.badges.contains(&"prompt".to_string()), "{prompt:?}");
}

#[test]
fn command_palette_exposes_builtin_safety_metadata() {
    let mut app = test_app();
    app.editor.set_text("/update");

    let matches = app.matching_commands();
    let update = matches
        .iter()
        .find(|row| row.name == "update")
        .expect("/update should be projected from built-in registry");

    assert_eq!(
        update.source,
        crate::surfaces::command_menu::CommandMenuSource::Builtin
    );
    assert!(
        update.badges.contains(&"external".to_string()),
        "{update:?}"
    );
    assert!(update.badges.contains(&"confirm".to_string()), "{update:?}");
}

#[test]
fn help_uses_command_menu_safety_metadata() {
    let mut app = test_app();
    let tx = test_tx();

    match app.handle_slash_command("/help all", &tx) {
        SlashResult::Display(text) => {
            assert!(text.contains("/update"), "{text}");
            assert!(text.contains("[builtin · external]"), "{text}");
            assert!(text.contains("/context"), "{text}");
            assert!(text.contains("[builtin · queue]"), "{text}");
        }
        other => panic!("expected /help all display, got {other:?}"),
    }
}

#[test]
fn builtin_command_specs_are_not_all_local_only() {
    assert!(
        crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .any(|command| {
                command.safety.class == omegon_traits::CommandSafetyClass::ExternalSideEffect
            })
    );
    assert!(
        crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .any(|command| {
                command.safety.class == omegon_traits::CommandSafetyClass::QueueMutation
            })
    );
    assert!(
        crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .any(|command| {
                command.safety.class == omegon_traits::CommandSafetyClass::StateChanging
            })
    );
    assert!(
        crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .any(|command| {
                command.safety.class == omegon_traits::CommandSafetyClass::Destructive
            })
    );
}

#[test]
fn palette_deduplicates_builtin_and_bus_commands() {
    let mut app = test_app();
    app.bus_commands = vec![omegon_traits::CommandDefinition {
        name: "cleave".into(),
        description: "parallel work".into(),
        subcommands: vec!["status".into()],
        availability: omegon_traits::CommandAvailability::ALL,
        safety: omegon_traits::CommandSafety::READ_ONLY,
    }];
    app.editor.set_text("/cl");
    let matches = app.matching_commands();
    let cleave_count = matches.iter().filter(|row| row.name == "cleave").count();
    assert_eq!(
        cleave_count, 1,
        "expected one /cleave entry, got: {matches:?}"
    );
}

#[test]
fn clear_command_is_not_documented_or_handled() {
    let mut app = test_app();
    let tx = test_tx();

    assert!(
        !crate::command_registry::BUILTIN_COMMANDS
            .iter()
            .any(|command| command.name == "clear")
    );

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
        availability: omegon_traits::CommandAvailability::ALL,
        safety: omegon_traits::CommandSafety::READ_ONLY,
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
fn canonical_slash_commands_are_registry_backed_or_intentional_aliases() {
    let registry_names: std::collections::HashSet<&str> = crate::command_registry::BUILTIN_COMMANDS
        .iter()
        .map(|command| command.name)
        .collect();

    let canonical_names = [
        "model",
        "think",
        "profile",
        "automation",
        "permissions",
        "status",
        "tree",
        "context",
        "new",
        "sessions",
        "auth",
        "settings",
        "config",
        "skills",
        "plan",
        "extension",
        "armory",
        "persona",
        "catalog",
        "plugin",
        "secrets",
        "vault",
        "cleave",
        "delegate",
    ];

    for name in canonical_names {
        assert!(
            registry_names.contains(name),
            "canonical slash command /{name} is parsed but missing from BUILTIN_COMMANDS"
        );
    }

    let intentional_aliases = [
        ("autonomy", "automation"),
        ("workspace", "status"),
        ("login", "auth"),
        ("logout", "auth"),
        ("note", "notes"),
        ("checkin", "notes"),
    ];

    for (alias, canonical) in intentional_aliases {
        assert!(
            !registry_names.contains(alias),
            "compatibility alias /{alias} should stay hidden; register /{canonical} instead"
        );
        assert!(
            registry_names.contains(canonical),
            "compatibility alias /{alias} points at missing canonical /{canonical}"
        );
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
        acp_url: None,
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        web_authority: crate::web::WebAuthorityConfig::default().status(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        daemon_status: WebDaemonStatus {
            queued_events: 2,
            processed_events: 3,
            worker_running: true,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
            ..WebDaemonStatus::default()
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
    assert!(
        text.contains("startup: http://127.0.0.1:7842/api/startup"),
        "got: {text}"
    );
    assert!(
        text.contains("websocket: ws://127.0.0.1:7842/ws?token=test"),
        "got: {text}"
    );
    assert!(
        text.contains("transport: http=insecure-bootstrap, ws=insecure-bootstrap"),
        "got: {text}"
    );
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
        acp_url: None,
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        web_authority: crate::web::WebAuthorityConfig::default().status(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        daemon_status: WebDaemonStatus {
            queued_events: 4,
            processed_events: 7,
            worker_running: true,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
            ..WebDaemonStatus::default()
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
    assert!(
        text.contains("startup: http://127.0.0.1:7842/api/startup"),
        "got: {text}"
    );
    assert!(
        text.contains("websocket: ws://127.0.0.1:7842/ws?token=test"),
        "got: {text}"
    );
    assert!(
        text.contains("transport: http=insecure-bootstrap, ws=insecure-bootstrap"),
        "got: {text}"
    );
    assert!(text.contains("queue depth:"), "got: {text}");
    assert!(text.contains("transport warnings:"), "got: {text}");
}

#[test]
fn slash_dash_status_preserves_tls_startup_urls() {
    let mut app = test_app();
    app.web_server_addr = Some("127.0.0.1:7842".parse().unwrap());
    app.web_startup = Some(crate::web::WebStartupInfo {
        schema_version: 2,
        addr: "127.0.0.1:7842".into(),
        http_base: "https://127.0.0.1:7842".into(),
        state_url: "https://127.0.0.1:7842/api/state".into(),
        startup_url: "https://127.0.0.1:7842/api/startup".into(),
        health_url: "https://127.0.0.1:7842/api/healthz".into(),
        ready_url: "https://127.0.0.1:7842/api/readyz".into(),
        ws_url: "wss://127.0.0.1:7842/ws?token=test".into(),
        acp_url: Some("wss://127.0.0.1:7842/acp?token=test".into()),
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        web_authority: crate::web::WebAuthorityConfig::default().status(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        daemon_status: WebDaemonStatus {
            queued_events: 4,
            processed_events: 7,
            worker_running: true,
            transport_warnings: vec![],
            active_child_runtimes: vec![],
            ..WebDaemonStatus::default()
        },
        instance_descriptor: None,
    });
    let tx = test_tx();

    let result = app.handle_slash_command("/dash status", &tx);
    let SlashResult::Display(text) = result else {
        panic!("expected Display result");
    };

    assert!(
        text.contains("running at https://127.0.0.1:7842"),
        "got: {text}"
    );
    assert!(
        text.contains("startup: https://127.0.0.1:7842/api/startup"),
        "got: {text}"
    );
    assert!(
        text.contains("websocket: wss://127.0.0.1:7842/ws?token=test"),
        "got: {text}"
    );
    assert!(
        text.contains("transport: http=secure, ws=secure"),
        "got: {text}"
    );
    assert!(text.contains("transport warnings: none"), "got: {text}");
    assert!(
        !text.contains("running at http://127.0.0.1:7842"),
        "got: {text}"
    );
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
        acp_url: None,
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        web_authority: crate::web::WebAuthorityConfig::default().status(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        daemon_status: WebDaemonStatus {
            queued_events: 2,
            processed_events: 3,
            worker_running: true,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
            ..WebDaemonStatus::default()
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
        acp_url: None,
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        web_authority: crate::web::WebAuthorityConfig::default().status(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        daemon_status: WebDaemonStatus {
            queued_events: 0,
            processed_events: 0,
            worker_running: false,
            transport_warnings: vec!["HTTP and WebSocket control-plane transports use insecure bootstrap tokens on localhost.".into()],
            active_child_runtimes: vec![],
            ..WebDaemonStatus::default()
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
                context_class: Some("Compact".into()),
                thinking_level: Some("Medium".into()),
                capability_tier: Some("B".into()),
                execution_substrate: None,
            },
        }),
    };

    let payload =
        super::build_auspex_attach_payload(&startup, super::AuspexHandoffMode::Env).unwrap();
    let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(json["transport"], "omegon-ipc");
    assert_eq!(json["preferred_handoff"], "env");
    assert_eq!(json["startup_url"], "http://127.0.0.1:7842/api/startup");
    assert_eq!(json["http_transport_security"], "insecure-bootstrap");
    assert_eq!(json["ws_transport_security"], "insecure-bootstrap");
    assert_eq!(json["ws_token"], "test");
    assert_eq!(json["instance"]["identity"]["instance_id"], "instance-1");
}

#[test]
fn auspex_attach_payload_carries_tls_transport_security_without_instance() {
    let startup = crate::web::WebStartupInfo {
        schema_version: 2,
        addr: "127.0.0.1:7842".into(),
        http_base: "https://127.0.0.1:7842".into(),
        state_url: "https://127.0.0.1:7842/api/state".into(),
        startup_url: "https://127.0.0.1:7842/api/startup".into(),
        health_url: "https://127.0.0.1:7842/api/healthz".into(),
        ready_url: "https://127.0.0.1:7842/api/readyz".into(),
        ws_url: "wss://127.0.0.1:7842/ws?token=test".into(),
        acp_url: Some("wss://127.0.0.1:7842/acp?token=test".into()),
        token: "test".into(),
        auth_mode: "ephemeral-bearer".into(),
        auth_source: "generated".into(),
        web_authority: crate::web::WebAuthorityConfig::default().status(),
        control_plane_state: crate::web::ControlPlaneState::Ready,
        daemon_status: WebDaemonStatus::default(),
        instance_descriptor: None,
    };

    let payload =
        super::build_auspex_attach_payload(&startup, super::AuspexHandoffMode::Env).unwrap();
    let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(json["startup_url"], "https://127.0.0.1:7842/api/startup");
    assert_eq!(json["ws_url"], "wss://127.0.0.1:7842/ws?token=test");
    assert_eq!(json["http_transport_security"], "secure");
    assert_eq!(json["ws_transport_security"], "secure");
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
        matches!(result, SlashResult::Handled),
        "/hel should prefix-match /help and open the command menu"
    );
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "commands")
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

    assert!(super::TutorialState::load(tmp.path()).is_none()); // no tutorial dir
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

#[test]
fn slash_logout_usage_lists_supported_remote_logout_providers() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/logout", &tx);
    let message = match result {
        SlashResult::Display(message) => message,
        other => panic!("expected usage display, got {other:?}"),
    };

    assert!(message.contains("anthropic"), "got: {message}");
    assert!(message.contains("openai"), "got: {message}");
    assert!(message.contains("openai-codex"), "got: {message}");
    assert!(message.contains("openrouter"), "got: {message}");
    assert!(!message.contains("ollama-cloud"), "got: {message}");
    assert!(!message.contains("ollama,"), "got: {message}");
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
    assert!(hint.contains("/auth login"), "should suggest login: {hint}");
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
fn retry_notification_marks_turn_state_as_upstream_retry() {
    let mut app = active_test_app();
    app.handle_agent_event(AgentEvent::TurnStart { turn: 1 });
    app.handle_agent_event(AgentEvent::SystemNotification {
        message: "⚠ Upstream rate_limit — retrying (attempt 3, delay 1500ms): provider busy".into(),
    });

    let rendered = render_app_to_string(&mut app, 150, 18);
    assert!(
        rendered.contains("retrying upstream attempt 3 · 1500ms"),
        "{rendered}"
    );
}

#[test]
fn recovery_hint_no_match() {
    let hint = App::recovery_hint(None, "some random error");
    assert!(hint.is_empty(), "should return empty for unknown errors");
}

#[test]
fn editor_top_line_shows_engine_block_details() {
    let mut settings = Settings::new("anthropic:claude-sonnet-4-6");
    settings.thinking = ThinkingLevel::High;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.harness.capability_grade = "B".into();
    app.footer_data.context_window = 1_048_576;
    app.footer_data.context_percent = 50.0;
    app.footer_data.estimated_tokens = 524_288;

    let rendered = render_app_to_string(&mut app, 140, 18);

    assert!(rendered.contains("claude-sonnet"), "{rendered}");
    assert!(rendered.contains(" anthropic/claude-sonnet"), "{rendered}");
    assert!(rendered.contains("󰿃 B"), "{rendered}");
    assert!(rendered.contains(" high"), "{rendered}");
    assert!(
        rendered.contains(" ctx:msv@1.0M ▕████░░░░▏ 50%"),
        "{rendered}"
    );
}

#[test]
fn editor_top_line_preserves_route_when_context_would_overflow() {
    let mut settings = Settings::new("openai-codex:gpt-5.5");
    settings.thinking = ThinkingLevel::Minimal;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.context_window = 1_048_576;
    app.footer_data.context_percent = 0.0;

    let rendered = render_app_to_string(&mut app, 80, 18);

    assert!(rendered.contains("openai-codex/gpt-5.5"), "{rendered}");
}

#[test]
fn editor_top_line_preserves_route_badge_contrast_after_bg_cleanup() {
    let mut settings = Settings::new("openai-codex:gpt-5.5");
    settings.thinking = ThinkingLevel::Minimal;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.context_window = 1_048_576;
    app.footer_data.context_percent = 0.0;

    let styles = rendered_cell_styles_for_text(&mut app, 140, 18, "openai-codex/gpt-5.5");

    assert!(
        styles
            .iter()
            .all(|(fg, bg)| *fg == crate::tui::theme::Alpharius.bg()
                && *bg == crate::tui::theme::Alpharius.accent_muted()),
        "route text should remain dark-on-accent after final bg cleanup, got {styles:?}"
    );
}

#[test]
fn editor_top_line_dividers_bridge_gradient_segment_backgrounds() {
    let mut settings = Settings::new("openai-codex:gpt-5.5");
    settings.thinking = ThinkingLevel::Minimal;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.context_window = 1_048_576;
    app.footer_data.context_percent = 0.0;

    let backend = ratatui::backend::TestBackend::new(140, 18);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buf = terminal.backend().buffer();
    let mut divider_styles = Vec::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            if cell.symbol() == "" {
                divider_styles.push((cell.fg, cell.bg));
            }
        }
    }

    assert_eq!(
        divider_styles,
        vec![
            (
                crate::tui::theme::Alpharius.accent_muted(),
                crate::tui::theme::Alpharius.accent()
            ),
            (
                crate::tui::theme::Alpharius.accent(),
                crate::tui::theme::Alpharius.card_bg()
            ),
            (
                crate::tui::theme::Alpharius.card_bg(),
                crate::tui::theme::Alpharius.card_bg()
            ),
            (
                crate::tui::theme::Alpharius.card_bg(),
                crate::tui::theme::Alpharius.surface_bg()
            ),
            (
                crate::tui::theme::Alpharius.surface_bg(),
                crate::tui::theme::Alpharius.surface_bg()
            ),
        ],
        "engine ribbon should bridge route → grade → profile → thinking → context → editor backgrounds: {divider_styles:?}"
    );
}

#[test]
fn editor_top_line_restores_context_fill_bar() {
    let mut settings = Settings::new("anthropic:claude-sonnet-4-6");
    settings.thinking = ThinkingLevel::High;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.harness.capability_grade = "B".into();
    app.footer_data.context_class = ContextClass::Compact;
    app.footer_data.actual_context_class = ContextClass::Massive;
    app.footer_data.context_window = 1_048_576;
    app.footer_data.context_percent = 50.0;
    app.footer_data.estimated_tokens = 524_288;

    let rendered = render_app_to_string(&mut app, 180, 18);

    assert!(
        rendered.contains("ctx:msv@1.0M ▕████░░░░▏ 50%"),
        "{rendered}"
    );
    assert!(rendered.contains("▕████░░░░▏"), "{rendered}");
    assert!(!rendered.contains("ctx:cmp→msv"), "{rendered}");
    assert!(!rendered.contains("κ ▰"), "{rendered}");
    assert!(!rendered.contains("◆"), "{rendered}");
}

#[test]
fn editor_top_line_grades_actual_model_not_route_intent() {
    let mut settings = Settings::new("openai-codex:gpt-5.6-sol");
    settings.thinking = ThinkingLevel::Low;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.harness.capability_grade = "B".into();

    let rendered = render_app_to_string(&mut app, 140, 18);

    assert!(
        rendered.contains("openai-codex/gpt-5.6-sol  󰿃 S  default   low   ctx:"),
        "{rendered}"
    );
}

#[test]
fn active_turn_keeps_engine_ribbon_and_moves_spinner_to_status_row() {
    let mut settings = Settings::new("openai-codex:gpt-5.5");
    settings.thinking = ThinkingLevel::High;
    let mut app = App::new(std::sync::Arc::new(std::sync::Mutex::new(settings)));
    app.apply_ui_preset(UiSurfaces::lean());
    app.footer_data.harness.capability_grade = "S".into();
    app.footer_data.context_window = 1_048_576;
    app.footer_data.context_percent = 5.0;
    app.agent_active = true;
    app.working_verb = "thinking";

    let rendered = render_app_to_string(&mut app, 160, 20);

    assert!(
        rendered.contains("openai-codex/gpt-5.5  󰿃 S  default   high   ctx:"),
        "active turn must keep engine route/grade/thinking/context visible: {rendered}"
    );
    assert!(
        rendered.contains("⟳") && rendered.contains("· active turn"),
        "spinner verb should move to shaded status row: {rendered}"
    );
}

#[test]
fn thinking_chunk_marks_runtime_phase_as_thinking() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::TurnStart { turn: 1 });
    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 80_000,
        context_window: 200_000,
        context_class: "Compact".into(),
        thinking_level: "high".into(),
    });
    app.handle_agent_event(AgentEvent::ThinkingChunk {
        text: "deliberating".into(),
    });

    app.instrument_panel
        .update_telemetry(40.0, 200_000, "high", None, true, 0.016);

    assert_eq!(app.instrument_panel.debug_activity_mode(), "think");
}

#[test]
fn active_tool_phase_beats_runtime_thinking_in_tui() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::TurnStart { turn: 1 });
    app.handle_agent_event(AgentEvent::ContextUpdated {
        tokens: 80_000,
        context_window: 200_000,
        context_class: "Compact".into(),
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

    app.instrument_panel
        .update_telemetry(40.0, 200_000, "high", None, true, 0.016);

    assert_eq!(app.instrument_panel.debug_activity_mode(), "tool");
}

#[test]
fn session_transcript_default_is_mode_independent_and_evidence_is_explicit() {
    let mut app = test_app();
    app.conversation
        .push_tool_start("tool-a", "read", Some("src/lib.rs"), Some("src/lib.rs"));
    app.conversation
        .push_tool_end("tool-a", false, Some("86 lines"));
    app.conversation
        .push_tool_start("tool-b", "bash", Some("cargo test"), Some("cargo test"));
    app.conversation
        .push_tool_end("tool-b", false, Some("47 tests passed"));
    for segment in app.conversation.segments_mut() {
        if matches!(segment.content, SegmentContent::ToolCard { .. }) {
            segment.meta.turn = Some(7);
        }
    }

    app.apply_ui_presentation(UiPresentationPolicy::om());
    let om = app.build_session_transcript(SegmentExportMode::Raw);
    app.apply_ui_presentation(UiPresentationPolicy::full());
    let full = app.build_session_transcript(SegmentExportMode::Raw);
    assert_eq!(om, full);
    assert!(om.contains("2 operations"), "{om}");
    assert!(!om.contains("src/lib.rs"), "{om}");

    let evidence = app.build_session_transcript_with_policy(
        SegmentExportMode::Raw,
        conversation_projection::ConversationExportPolicy::Evidence,
    );
    assert!(evidence.contains("src/lib.rs"), "{evidence}");
    assert!(evidence.contains("cargo test"), "{evidence}");
}

#[test]
fn episode_inspection_identity_survives_presentation_switches() {
    let mut app = test_app();
    app.tool_inspection_target = Some(ToolInspectionTarget::Episode {
        episode_id: "turn:7".into(),
        evidence_id: "tool-1".into(),
    });

    app.apply_ui_presentation(UiPresentationPolicy::full());
    assert_eq!(
        app.tool_inspection_target
            .as_ref()
            .and_then(ToolInspectionTarget::episode_id),
        Some("turn:7")
    );
    assert_eq!(
        app.tool_inspection_target
            .as_ref()
            .map(ToolInspectionTarget::evidence_id),
        Some("tool-1")
    );

    app.apply_ui_presentation(UiPresentationPolicy::om());
    assert_eq!(
        app.tool_inspection_target
            .as_ref()
            .and_then(ToolInspectionTarget::episode_id),
        Some("turn:7")
    );
}

#[test]
fn selected_tool_segment_detail_pane_renders_full_tool_context() {
    let mut app = active_test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "cargo test"}),
    });
    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "tool-1".into(),
        name: "bash".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![omegon_traits::ContentBlock::Text {
                text: "test result details".into(),
            }],
            details: serde_json::Value::Null,
        },
    });
    app.activity_tools.clear();
    app.tool_inspection_target = Some(ToolInspectionTarget::Episode {
        episode_id: "tool:tool-1".into(),
        evidence_id: "tool-1".into(),
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(rendered.contains("detail log"), "{rendered}");
    assert!(rendered.contains("bash"), "{rendered}");
    assert!(rendered.contains("test result details"), "{rendered}");
}

#[test]
fn completed_live_tool_lingers_before_activity_clears() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "pwd"}),
    });
    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "tool-1".into(),
        name: "bash".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
            details: serde_json::Value::Null,
        },
    });

    assert!(app.tool_inspection_target.is_none());
    let tool = app
        .activity_tools
        .iter()
        .find(|tool| tool.segment_id == "tool-1")
        .expect("completed tool should remain in transient activity");
    assert_eq!(
        tool.status,
        crate::surfaces::activity::ActivityToolStatus::Complete
    );
    assert!(tool.expires_at.is_some());
}

#[test]
fn expired_live_tool_linger_clears_activity_on_render() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "pwd"}),
    });
    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "tool-1".into(),
        name: "bash".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
            details: serde_json::Value::Null,
        },
    });
    if let Some(tool) = app
        .activity_tools
        .iter_mut()
        .find(|tool| tool.segment_id == "tool-1")
    {
        tool.expires_at = Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
    }

    let _ = render_app_to_string(&mut app, 120, 32);

    assert!(app.tool_inspection_target.is_none());
    assert!(app.activity_tools.is_empty());
}

#[test]
fn activity_tool_start_refreshes_without_duplicate_entries() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "cargo check"}),
    });
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "cargo test"}),
    });

    assert_eq!(
        app.activity_tools
            .iter()
            .filter(|tool| tool.segment_id == "tool-1")
            .count(),
        1
    );
    assert_eq!(
        app.activity_tools.front().map(|tool| tool.status),
        Some(crate::surfaces::activity::ActivityToolStatus::Running)
    );
}

#[test]
fn activity_tool_cap_preserves_running_entries_over_completed_entries() {
    let mut app = test_app();
    for idx in 0..6 {
        let id = format!("done-{idx}");
        app.handle_agent_event(AgentEvent::ToolStart {
            id: id.clone(),
            name: "read".into(),
            args: serde_json::json!({"path": format!("file-{idx}.rs")}),
        });
        app.handle_agent_event(AgentEvent::ToolEnd {
            id,
            name: "read".into(),
            is_error: false,
            result: omegon_traits::ToolResult {
                content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
                details: serde_json::Value::Null,
            },
        });
    }
    for idx in 0..5 {
        app.handle_agent_event(AgentEvent::ToolStart {
            id: format!("run-{idx}"),
            name: "bash".into(),
            args: serde_json::json!({"command": format!("cmd-{idx}")}),
        });
    }

    let running = app
        .activity_tools
        .iter()
        .filter(|tool| {
            matches!(
                tool.status,
                crate::surfaces::activity::ActivityToolStatus::Running
            )
        })
        .count();
    let completed = app.activity_tools.len().saturating_sub(running);

    assert_eq!(running, 5);
    assert!(
        completed <= 4,
        "completed={completed}, tools={:?}",
        app.activity_tools
    );
    assert!(app.activity_tools.len() <= 8);
}

#[test]
fn activity_prune_removes_expired_completed_but_keeps_running_entries() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "done".into(),
        name: "read".into(),
        args: serde_json::json!({"path": "Cargo.toml"}),
    });
    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "done".into(),
        name: "read".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![omegon_traits::ContentBlock::Text { text: "ok".into() }],
            details: serde_json::Value::Null,
        },
    });
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "running".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "sleep 10"}),
    });
    if let Some(tool) = app
        .activity_tools
        .iter_mut()
        .find(|tool| tool.segment_id == "done")
    {
        tool.expires_at = Some(std::time::Instant::now() - std::time::Duration::from_millis(1));
    }

    let _ = render_app_to_string(&mut app, 140, 36);

    assert!(
        app.activity_tools
            .iter()
            .all(|tool| tool.segment_id != "done")
    );
    assert!(
        app.activity_tools
            .iter()
            .any(|tool| tool.segment_id == "running")
    );
}

#[test]
fn active_single_running_activity_tool_uses_full_live_card() {
    let mut app = test_app();
    app.apply_ui_presentation(UiPresentationPolicy::active());
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "cargo check"}),
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(rendered.contains("live log"), "{rendered}");
    assert!(rendered.contains("cargo check"), "{rendered}");
}

#[test]
fn om_single_running_activity_tool_stays_one_line() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "cargo check"}),
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(!rendered.contains("live log"), "{rendered}");
    assert!(rendered.contains("cargo check"), "{rendered}");
}

#[test]
fn completed_tools_handoff_to_one_durable_outcome_without_activity_duplication() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-1".into(),
        name: "read".into(),
        args: serde_json::json!({"path": "Cargo.toml"}),
    });
    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "tool-1".into(),
        name: "read".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![omegon_traits::ContentBlock::Text {
                text: "workspace manifest".into(),
            }],
            details: serde_json::Value::Null,
        },
    });
    app.handle_agent_event(AgentEvent::ToolStart {
        id: "tool-2".into(),
        name: "bash".into(),
        args: serde_json::json!({"command": "cargo check"}),
    });
    app.handle_agent_event(AgentEvent::ToolEnd {
        id: "tool-2".into(),
        name: "bash".into(),
        is_error: false,
        result: omegon_traits::ToolResult {
            content: vec![omegon_traits::ContentBlock::Text {
                text: "Finished dev profile".into(),
            }],
            details: serde_json::Value::Null,
        },
    });

    let rendered = render_app_to_string(&mut app, 140, 36);

    assert!(
        rendered.contains("✓ bash · Finished dev profile · 2 operations"),
        "{rendered}"
    );
    assert!(!rendered.contains("done bash"), "{rendered}");
    assert!(!rendered.contains("done read"), "{rendered}");
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

#[test]
fn copy_full_session_runs_without_panic() {
    let mut app = test_app();

    // Populate conversation with a user prompt, assistant text, and a tool card.
    app.conversation.push_user("Hello, world!");
    app.conversation.append_streaming("Sure, let me help.");
    app.conversation
        .push_tool_start("t1", "bash", Some("echo hi"), Some("echo hi"));
    app.conversation.push_tool_end("t1", false, Some("hi"));

    // copy_full_session may fail to reach the clipboard in CI, but it must not
    // panic and should not leave the app in a bad state.
    app.copy_full_session();

    // Verify the conversation is still intact after the copy.
    assert!(
        app.conversation.segments().len() >= 3,
        "expected at least 3 segments (user + assistant + tool), got {}",
        app.conversation.segments().len()
    );
}

#[test]
fn copy_full_session_on_empty_conversation_shows_warning() {
    let mut app = test_app();

    // No segments — should not panic.
    app.copy_full_session();

    // Conversation still empty.
    assert!(app.conversation.segments().is_empty());
}

#[test]
fn slash_new_is_context_reset_alias() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/new", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(matches!(
        rx.try_recv().unwrap(),
        TuiCommand::ContextClear { .. }
    ));
}

#[test]
fn slash_context_reset_uses_context_clear_control_path() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/context reset", &tx);

    assert!(
        matches!(result, SlashResult::Display(ref text) if text.contains("Starting fresh context"))
    );
    assert!(matches!(
        rx.try_recv().unwrap(),
        TuiCommand::ContextClear { .. }
    ));
}

#[test]
fn runtime_queue_update_renders_authoritative_queue_line() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::RuntimeQueueUpdated {
        snapshot_json: serde_json::json!({
            "depth": 1,
            "active": null,
            "items": [{
                "id": 7,
                "submitted_by": "local-tui",
                "via": "tui",
                "queue_mode": "until_ready",
                "preview": "queued follow-up from runtime",
                "attachments": 0,
                "voice": false
            }],
            "previews": ["#7 ready: queued follow-up from runtime"]
        }),
    });

    let rendered = render_app_to_string(&mut app, 100, 24);
    assert!(rendered.contains("Runtime queue"), "{rendered}");
    assert!(rendered.contains("[1]"), "{rendered}");
    assert!(
        rendered.contains("queued follow-up from runtime"),
        "{rendered}"
    );
}

#[test]
fn runtime_queue_zero_depth_hides_queue_line() {
    let mut app = test_app();
    app.handle_agent_event(AgentEvent::RuntimeQueueUpdated {
        snapshot_json: serde_json::json!({
            "depth": 0,
            "active": null,
            "items": [],
            "previews": []
        }),
    });

    let rendered = render_app_to_string(&mut app, 100, 24);
    assert!(!rendered.contains("Runtime queue"), "{rendered}");
}

#[test]
fn palette_system_notification_matrix_accounts_for_palette_slash_outputs() {
    let cases = [
        ("## Context\nsummary", None),
        ("## Thinking levels\nsummary", Some("/think status")),
        ("## Skills\nsummary", Some("/skills")),
        ("## Prompt library\nsummary", Some("/prompt list")),
        ("## Random\nsummary", None),
    ];

    for (message, expected) in cases {
        assert_eq!(slash_command_for_palette_notification(message), expected);
    }
}

#[test]
fn slash_context_opens_context_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/context", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("context menu");
    assert_eq!(menu.projection.id, "context");
    assert!(
        menu.state
            .visible_rows(&menu.projection)
            .iter()
            .any(|row| row.row.id == "context.class")
    );
}

#[test]
fn context_menu_class_row_opens_existing_selector() {
    let mut app = test_app();
    app.open_context_menu();
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_primary_action(&menu.projection))
        .expect("context class selector action");

    assert!(matches!(
        app.execute_active_menu_action(action, &test_tx()),
        SlashResult::Handled
    ));

    assert_eq!(app.selector_kind, Some(SelectorKind::ContextClass));
    let selector = app.selector.as_ref().expect("context selector");
    assert!(
        selector
            .options
            .iter()
            .any(|option| option.label.contains("Massive"))
    );
}

#[test]
fn context_menu_clear_requires_explicit_command() {
    let mut app = test_app();
    app.open_context_menu();
    let menu = app.active_menu.as_mut().expect("context menu");
    assert!(
        menu.state
            .select_row_by_id(&menu.projection, "context.clear")
    );

    assert_eq!(menu.state.selected_command(&menu.projection), None);
    let row = menu
        .state
        .selected_row(&menu.projection)
        .expect("clear row");
    assert!(
        row.row
            .metadata
            .iter()
            .any(|value| value.contains("/context clear"))
    );
}

#[test]
fn context_menu_compact_action_uses_shared_command_path() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_context_menu();
    app.active_menu.as_mut().unwrap().state.selected_row = 2;

    let command = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_command(&menu.projection));

    assert_eq!(command.as_deref(), Some("/context compact"));
    assert!(matches!(
        app.execute_active_menu_command(command.unwrap(), &tx),
        SlashResult::Handled
    ));
    assert!(app.command_panel.is_some());
}

#[test]
fn slash_context_without_subcommand_is_menu_only_not_canonical_status() {
    assert_eq!(canonical_slash_command("context", ""), None);
    assert_eq!(
        canonical_slash_command("context", "status"),
        Some(CanonicalSlashCommand::ContextStatus)
    );
}

#[test]
fn context_system_notifications_do_not_open_command_panel() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::SystemNotification {
        message:
            "## Context\n4966/1000000 tokens (0%)\n\n### Actions\n- `/context compact` — compact"
                .into(),
    });

    assert!(app.command_panel.is_none());
    assert_eq!(app.conversation.segments().len(), 1);
}

#[test]
fn one_shot_context_notifications_toast_without_command_panel() {
    let mut app = test_app();

    app.handle_agent_event(AgentEvent::SystemNotification {
        message: "Context cleared. Starting fresh conversation.".into(),
    });

    assert!(app.command_panel.is_none());
    assert!(app.conversation.segments().is_empty());
}

#[test]
fn settings_projection_helper_marks_runtime_profile_drift() {
    let _env = crate::test_support::env::lock();
    let mut app = test_app();
    let tmp = tempfile::tempdir().expect("tempdir");
    let profile_path = tmp.path().join(".omegon/profile.json");
    std::fs::create_dir_all(profile_path.parent().unwrap()).expect("profile dir");
    std::fs::write(&profile_path, r#"{"thinkingLevel":"medium"}"#).expect("profile");
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    app.update_settings(|s| {
        s.thinking = ThinkingLevel::Minimal;
        s.set_requested_context_class(ContextClass::Massive);
    });

    let projection = app.settings_projection();
    let runtime = projection
        .tabs
        .iter()
        .find(|tab| tab.id == "runtime")
        .expect("runtime tab");

    assert!(
        runtime
            .rows
            .iter()
            .any(|row| row.id == "runtime.thinking" && row.profile.is_some())
    );
}

#[test]
fn settings_menu_renders_profile_source_and_drift_actions() {
    let mut app = test_app();
    let tmp = tempfile::tempdir().expect("tempdir");
    let profile_path = tmp.path().join(".omegon/profile.json");
    std::fs::create_dir_all(profile_path.parent().unwrap()).expect("profile dir");
    std::fs::write(&profile_path, r#"{"thinkingLevel":"medium"}"#).expect("profile");
    app.footer_data.cwd = tmp.path().to_string_lossy().to_string();
    app.update_settings(|s| {
        s.thinking = ThinkingLevel::Minimal;
        s.set_requested_context_class(ContextClass::Massive);
    });

    app.open_settings_menu();
    let rendered = render_app_to_string(&mut app, 120, 32);

    // The drift hint line wraps at an environment-dependent column (the
    // tempdir path length differs across platforms), so phrase assertions
    // must survive a line break landing mid-phrase. Collapse the render to
    // a single whitespace-normalized line before asserting.
    let flat = rendered
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('│', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    assert!(flat.contains("profile: project"), "{rendered}");
    assert!(flat.contains("file:"), "{rendered}");
    assert!(flat.contains("runtime drift"), "{rendered}");
    assert!(flat.contains("/profile save"), "{rendered}");
    assert!(flat.contains("/profile apply"), "{rendered}");
}

#[test]
fn settings_profile_source_line_separates_source_from_full_path() {
    let path = std::path::PathBuf::from("/tmp/omegon-project/.omegon/profile.json");
    let line = settings_profile_source_line(&crate::settings::ProfileSource::Project(path.clone()));

    assert_eq!(line, format!("profile: project · file: {}", path.display()));
    assert!(!line.contains("project:/tmp"));
}

#[test]
fn slash_profile_opens_profile_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/profile", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("profile menu");
    assert_eq!(menu.projection.id, "profile");
    assert!(
        menu.state
            .visible_rows(&menu.projection)
            .iter()
            .any(|row| row.row.id == "profile.save")
    );
    assert!(
        menu.projection
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("runtime drift"))
    );
}

#[test]
fn profile_menu_lists_discovered_project_profiles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _cwd = push_current_dir(tmp.path());
    let profiles_dir = tmp.path().join(".omegon/profiles");
    std::fs::create_dir_all(&profiles_dir).expect("profiles dir");
    std::fs::write(
        profiles_dir.join("reviewer.json"),
        r#"{"name":"reviewer","thinkingLevel":"high"}"#,
    )
    .expect("profile");

    let mut app = test_app();
    app.footer_data.cwd = tmp.path().display().to_string();
    app.open_profile_menu();
    let menu = app.active_menu.as_ref().expect("profile menu");
    let available = menu.projection.tabs[0]
        .groups
        .iter()
        .find(|group| group.id == "profile.available")
        .expect("available profiles group");
    let reviewer = available
        .rows
        .iter()
        .find(|row| row.id == "profile.registry.project.reviewer")
        .expect("project profile row");

    assert_eq!(reviewer.label, "reviewer");
    assert_eq!(reviewer.value.as_deref(), Some("project"));
    assert!(reviewer.badges.iter().any(|badge| badge.label == "project"));
    assert_eq!(
        reviewer
            .primary_action
            .as_ref()
            .and_then(|action| action.command.as_deref()),
        Some("/profile use reviewer project")
    );
}

#[test]
fn menu_action_confirmation_requires_second_activation() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_profile_menu();
    {
        let menu = app.active_menu.as_mut().expect("profile menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "profile.apply")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_primary_action(&menu.projection))
        .expect("apply action");

    assert!(matches!(
        app.execute_active_menu_action(action.clone(), &tx),
        SlashResult::Handled
    ));
    assert_eq!(
        app.pending_menu_confirmation.as_deref(),
        Some("profile.apply.primary")
    );
    assert!(
        app.active_menu.is_some(),
        "first activation should keep menu open"
    );

    assert!(matches!(
        app.execute_active_menu_action(action, &tx),
        SlashResult::Handled
    ));
    assert_eq!(app.pending_menu_confirmation, None);
    assert!(
        app.active_menu.is_none(),
        "confirmed command should use normal handled close policy"
    );
}

#[test]
fn non_confirming_menu_action_clears_pending_confirmation() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_profile_menu();
    app.pending_menu_confirmation = Some("profile.apply.primary".into());
    let action = crate::surfaces::menu::MenuActionProjection::command(
        "profile.view.test",
        "View",
        "/profile view",
    );

    assert!(matches!(
        app.execute_active_menu_action(action, &tx),
        SlashResult::Handled
    ));

    assert_eq!(app.pending_menu_confirmation, None);
}

#[test]
fn profile_menu_save_and_apply_hotkeys_use_shared_rows() {
    let mut app = test_app();
    app.open_profile_menu();
    {
        let menu = app.active_menu.as_mut().expect("profile menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "profile.save")
        );
    }
    let menu = app.active_menu.as_ref().expect("profile menu");
    assert_eq!(
        menu.state
            .selected_action_command_for_key(&menu.projection, 's')
            .as_deref(),
        Some("/profile save")
    );
    {
        let menu = app.active_menu.as_mut().expect("profile menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "profile.apply")
        );
    }
    let menu = app.active_menu.as_ref().expect("profile menu");
    assert_eq!(
        menu.state
            .selected_action_command_for_key(&menu.projection, 'a'),
        None
    );
    assert_eq!(
        menu.state
            .selected_action_command_for_key(&menu.projection, 'a'),
        None
    );
    assert_eq!(
        menu.state.selected_command(&menu.projection).as_deref(),
        Some("/profile apply")
    );
}

#[test]
fn profile_apply_is_selectable_without_hotkey() {
    let mut app = test_app();
    app.open_profile_menu();
    let menu = app.active_menu.as_mut().expect("profile menu");
    assert!(
        menu.state
            .select_row_by_id(&menu.projection, "profile.apply")
    );

    assert_eq!(
        menu.state
            .selected_action_command_for_key(&menu.projection, 'a'),
        None
    );
    assert_eq!(
        menu.state.selected_command(&menu.projection).as_deref(),
        Some("/profile apply")
    );
}

#[test]
fn profile_view_still_queues_text_readout_command() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/profile view", &tx);

    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("profile view command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ProfileView,
            ..
        } => {}
        other => panic!("expected profile view request, got {other:?}"),
    }
}

#[test]
fn settings_profile_shortcuts_queue_existing_profile_commands() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.queue_settings_profile_save(&tx);
    match rx.try_recv().expect("save command") {
        TuiCommand::ExecuteControl {
            request:
                crate::control_runtime::ControlRequest::ProfileCapture {
                    target: crate::settings::ProfileSaveTarget::ActiveSource,
                },
            ..
        } => {}
        other => panic!("expected profile capture, got {other:?}"),
    }

    app.queue_settings_profile_apply(&tx);
    match rx.try_recv().expect("apply command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::ProfileApply,
            ..
        } => {}
        other => panic!("expected profile apply, got {other:?}"),
    }
}

#[test]
fn slash_settings_opens_active_menu_without_command_panel() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/settings", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("settings menu");
    assert_eq!(menu.projection.id, "settings");
    assert!(app.command_panel.is_none());
}

#[test]
fn slash_settings_and_config_open_the_universal_configuration_menu() {
    for command in ["/settings", "/config"] {
        let mut app = test_app();
        let tx = test_tx();

        let result = app.handle_slash_command(command, &tx);

        assert!(matches!(result, SlashResult::Handled));
        let menu = app.active_menu.as_ref().expect("settings menu");
        assert_eq!(menu.projection.id, "settings");
        assert_eq!(menu.state.active_tab, menu.projection.tabs[0].id);
        assert!(app.command_panel.is_none());
    }
}

#[test]
fn settings_configuration_tab_routes_to_canonical_submenus() {
    let mut app = test_app();
    app.open_settings_menu();

    let menu = app.active_menu.as_ref().expect("settings menu");
    let tab = menu
        .projection
        .tabs
        .iter()
        .find(|tab| tab.id == "configuration")
        .expect("configuration tab");
    let rows = &tab.groups[0].rows;
    for (id, command) in [
        ("skills", "/skills"),
        ("auth", "/auth"),
        ("model", "/model"),
        ("extensions", "/extension"),
    ] {
        let row = rows
            .iter()
            .find(|row| row.id == format!("settings.area.{id}"))
            .unwrap_or_else(|| panic!("missing settings area {id}"));
        assert_eq!(
            row.primary_action
                .as_ref()
                .and_then(|action| action.command.as_deref()),
            Some(command)
        );
    }
}

#[test]
fn settings_and_config_direct_routes_open_canonical_submenus() {
    for command in ["/settings auth", "/config auth"] {
        let mut app = test_app();
        assert!(matches!(
            app.handle_slash_command(command, &test_tx()),
            SlashResult::Handled
        ));
        assert_eq!(
            app.active_menu
                .as_ref()
                .map(|menu| menu.projection.id.as_str()),
            Some("auth")
        );
    }

    for command in ["/settings skills", "/config skills"] {
        let mut app = test_app();
        assert!(matches!(
            app.handle_slash_command(command, &test_tx()),
            SlashResult::Handled
        ));
        assert_eq!(
            app.active_menu
                .as_ref()
                .map(|menu| menu.projection.id.as_str()),
            Some("skills")
        );
    }
}

#[test]
fn settings_menu_opens_choice_rows_from_projection_metadata() {
    let mut app = test_app();

    app.open_settings_menu();
    let runtime_tab = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.projection.tabs.first())
        .map(|tab| tab.id.clone())
        .expect("runtime settings tab");
    let menu = app.active_menu.as_mut().unwrap();
    menu.state.active_tab = runtime_tab;
    menu.state.selected_row = 1;
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection));
    assert!(
        action.is_some(),
        "settings row should expose a typed action"
    );
    app.execute_active_menu_action(action.unwrap(), &test_tx());

    assert_eq!(app.selector_kind, Some(SelectorKind::ThinkingLevel));
    let selector = app.selector.as_ref().expect("thinking selector");
    assert!(selector.options.iter().any(|option| option.value == "high"));
}

#[test]
fn settings_menu_navigation_helpers_bound_rows_and_wrap_tabs() {
    let app = test_app();
    let projection = app.settings_menu_projection();
    let mut state = MenuState::new(&projection);

    state.move_up();
    assert_eq!(state.selected_row, 0);

    for _ in 0..10 {
        state.move_down(&projection);
    }
    assert_eq!(
        state.selected_row,
        state.visible_rows(&projection).len().saturating_sub(1)
    );

    state.next_tab(&projection);
    assert_eq!(state.active_tab, "ui");
    assert_eq!(state.selected_row, 0);

    state.previous_tab(&projection);
    assert_eq!(state.active_tab, "runtime");

    state.previous_tab(&projection);
    assert_eq!(state.active_tab, "configuration");
}

#[test]
fn settings_menu_filter_matches_row_metadata_and_exit_search_clears_in_browse() {
    let app = test_app();
    let projection = app.settings_menu_projection();
    let mut state = MenuState::new(&projection);

    state.enter_search();
    for ch in "auto".chars() {
        state.push_filter_char(&projection, ch);
    }

    let rows = state.visible_rows(&projection);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].row.id, "runtime.max_turns");

    assert!(state.exit_search());
    assert_eq!(state.mode, MenuMode::Browse);
    assert_eq!(state.filter, "auto");

    assert!(state.exit_search());
    assert!(state.filter.is_empty());
}

#[test]
fn settings_menu_filter_backspace_clamps_empty_results() {
    let app = test_app();
    let projection = app.settings_menu_projection();
    let mut state = MenuState::new(&projection);

    state.selected_row = 3;
    state.enter_search();
    for ch in "zzzz".chars() {
        state.push_filter_char(&projection, ch);
    }
    assert!(state.visible_rows(&projection).is_empty());
    assert_eq!(state.selected_row, 0);

    for _ in 0..4 {
        state.pop_filter_char(&projection);
    }
    assert_eq!(state.visible_rows(&projection).len(), 4);
}

#[test]
fn settings_menu_choice_row_closes_menu_before_opening_selector() {
    let mut app = test_app();
    app.open_settings_menu();
    {
        let menu = app.active_menu.as_mut().expect("settings menu");
        assert!(
            menu.state
                .select_row_by_id(&menu.projection, "runtime.thinking")
        );
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection))
        .expect("thinking selector action");

    assert!(matches!(
        app.execute_active_menu_action(action, &test_tx()),
        SlashResult::Handled
    ));
    assert!(
        app.active_menu.is_none(),
        "selector must receive arrow keys"
    );
    assert!(
        app.selector.is_some(),
        "settings choice selector should be open"
    );
    assert!(matches!(
        app.selector_kind,
        Some(SelectorKind::ThinkingLevel)
    ));
}

#[test]
fn settings_menu_max_turns_row_queues_existing_control_request() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.open_settings_menu();
    app.active_menu.as_mut().unwrap().state.selected_row = 3;
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection));
    assert!(
        action.is_some(),
        "max turns row should expose a typed action"
    );
    app.execute_active_menu_action(action.unwrap(), &tx);
    assert!(
        app.active_menu.is_none(),
        "selector must receive arrow keys"
    );
    assert!(app.selector.is_some(), "max turns selector should be open");
    app.selector.as_mut().unwrap().cursor = 4;
    let message = app.confirm_selector(&tx).expect("max turns message");

    assert_eq!(message, "Max turns → 100");
    match rx.try_recv().expect("max turns command") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SetMaxTurns { max_turns },
            ..
        } => assert_eq!(max_turns, 100),
        other => panic!("expected max turns control request, got {other:?}"),
    }
}

#[test]
fn settings_menu_auto_update_row_toggles_persisted_setting() {
    let mut app = test_app();

    app.open_settings_menu();
    {
        let menu = app.active_menu.as_mut().unwrap();
        menu.state.active_tab = "updates".into();
        menu.state.selected_row = 1;
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection));
    assert!(
        action.is_some(),
        "auto-update row should expose a typed action"
    );
    app.execute_active_menu_action(action.unwrap(), &test_tx());

    assert!(app.settings().auto_update);
}

#[test]
fn settings_menu_sandbox_row_disables_persisted_setting() {
    let mut app = test_app();
    app.update_settings(|s| s.sandbox = true);

    app.open_settings_menu();
    {
        let menu = app.active_menu.as_mut().unwrap();
        menu.state.active_tab = "workspace".into();
        menu.state.selected_row = 1;
    }
    let action = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.state.selected_action(&menu.projection));
    assert!(action.is_some(), "sandbox row should expose a typed action");
    app.execute_active_menu_action(action.unwrap(), &test_tx());

    assert!(!app.settings().sandbox);
}

#[test]
fn menu_login_secret_input_closes_menu_without_output_panel() {
    let mut app = test_app();
    let tx = test_tx();
    app.open_auth_menu();

    let result = app.execute_active_menu_command("/login openai".to_string(), &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(matches!(
        app.editor.mode(),
        super::editor::EditorMode::SecretInput { .. }
    ));
    assert!(app.active_menu.is_none());
    assert!(app.command_panel.is_none());
    assert!(
        app.operator_events
            .iter()
            .any(|event| event.message.contains("Paste") || event.message.contains("API key"))
    );
}

#[test]
fn login_selector_routes_github_copilot_through_oauth_flow() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    app.open_login_selector();
    let selector = app.selector.as_mut().expect("login selector");
    selector.cursor = selector
        .options
        .iter()
        .position(|option| option.value == "github-copilot")
        .expect("github copilot login option");

    let message = app.confirm_selector(&tx).expect("selector message");

    assert!(
        message.contains("GitHub Copilot"),
        "selector message should name GitHub Copilot: {message}"
    );
    assert!(!matches!(
        app.editor.mode(),
        super::editor::EditorMode::SecretInput { .. }
    ));
    match rx.try_recv().expect("auth login command") {
        TuiCommand::BusCommand { name, args } => {
            assert_eq!(name, "auth_login");
            assert_eq!(args, "github-copilot");
        }
        other => panic!("expected auth login bus command, got {other:?}"),
    }
}

#[test]
fn auth_menu_rows_cover_operator_auth_providers() {
    let mut app = test_app();
    app.open_auth_menu();
    let menu = app.active_menu.as_ref().expect("auth menu");
    let row_ids: std::collections::HashSet<_> = menu
        .state
        .visible_rows(&menu.projection)
        .into_iter()
        .map(|row| row.row.id.clone())
        .collect();

    for provider in crate::auth::operator_auth_provider_ids() {
        let expected = format!("auth.provider.{provider}");
        assert!(
            row_ids.contains(&expected),
            "auth menu missing operator auth provider row {expected}"
        );
    }
}

#[test]
fn canonical_secrets_set_rejects_plaintext_values() {
    assert_eq!(
        canonical_slash_command("secrets", "set API_TOKEN super-secret-value"),
        None
    );
    assert_eq!(
        canonical_slash_command("secrets", "set API_TOKEN env:API_TOKEN"),
        Some(CanonicalSlashCommand::SecretsSet {
            name: "API_TOKEN".into(),
            value: "env:API_TOKEN".into(),
        })
    );
}

#[test]
fn secrets_set_direct_value_uses_hidden_input_instead_of_control_request() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/secrets set API_TOKEN super-secret-value", &tx);

    assert!(matches!(result, SlashResult::Display(_)));
    assert!(matches!(
        app.editor.mode(),
        super::editor::EditorMode::SecretInput { .. }
    ));
    assert!(
        rx.try_recv().is_err(),
        "direct value must not be queued as a control request"
    );
}

#[test]
fn secrets_set_recipe_still_queues_control_request() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    let result = app.handle_slash_command("/secrets set API_TOKEN env:API_TOKEN", &tx);

    assert!(matches!(result, SlashResult::Handled));
    match rx.try_recv().expect("control request") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::SecretsSet { name, value },
            ..
        } => {
            assert_eq!(name, "API_TOKEN");
            assert_eq!(value, "env:API_TOKEN");
        }
        other => panic!("expected secrets set request, got {other:?}"),
    }
}

#[test]
fn slash_auth_opens_provider_auth_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/auth", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("auth menu");
    assert_eq!(menu.projection.id, "auth");
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(
        rows.iter()
            .any(|row| row.row.id == "auth.provider.anthropic")
    );
    assert!(
        rows.iter()
            .any(|row| row.row.metadata.iter().any(|m| m == "/login openai"))
    );
    let copilot = rows
        .iter()
        .find(|row| row.row.id == "auth.provider.github-copilot")
        .expect("github copilot auth row");
    assert_eq!(copilot.row.label, "GitHub Copilot");
    assert!(
        copilot
            .row
            .metadata
            .iter()
            .any(|m| m == "/login github-copilot")
    );
}

#[test]
fn bare_login_opens_provider_auth_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/login", &tx);

    assert!(matches!(result, SlashResult::Handled));
    assert!(
        app.active_menu
            .as_ref()
            .is_some_and(|menu| menu.projection.id == "auth")
    );
}

#[test]
fn model_and_auth_provider_rows_share_login_metadata() {
    let mut app = test_app();
    app.open_auth_menu();
    let auth_row = app
        .active_menu
        .as_ref()
        .unwrap()
        .state
        .visible_rows(&app.active_menu.as_ref().unwrap().projection)
        .into_iter()
        .find(|row| row.row.id == "auth.provider.openai")
        .expect("auth openai row")
        .row
        .clone();

    app.open_model_menu();
    app.active_menu.as_mut().unwrap().state.active_tab = "providers".into();
    let model_row = app
        .active_menu
        .as_ref()
        .unwrap()
        .state
        .visible_rows(&app.active_menu.as_ref().unwrap().projection)
        .into_iter()
        .find(|row| row.row.id == "provider.openai")
        .expect("model openai row")
        .row
        .clone();

    assert_eq!(auth_row.metadata[0], model_row.metadata[0]);
    assert_eq!(auth_row.description, model_row.description);
}

#[test]
fn provider_rows_mark_settings_model_as_selected_before_route_event() {
    let mut app = test_app();
    app.route_selected_model = None;
    app.update_settings(|settings| settings.model = "openai:gpt-4.1".into());

    let rows = app.provider_status_rows("provider");
    let openai = rows
        .iter()
        .find(|row| row.id == "provider.openai")
        .expect("openai row");

    assert!(openai.badges.iter().any(|badge| badge.label == "selected"));
    assert!(openai.metadata.iter().any(|item| item == "route: selected"));
}

#[test]
fn provider_rows_mark_fallback_serving_provider() {
    let mut app = test_app();
    app.route_state = Some("fallback".into());
    app.route_selected_model = Some("openai-codex:gpt-5.4".into());
    app.route_serving_model = Some("anthropic:claude-sonnet-4-6".into());

    let rows = app.provider_status_rows("provider");
    let anthropic = rows
        .iter()
        .find(|row| row.id == "provider.anthropic")
        .expect("anthropic row");

    assert!(
        anthropic
            .badges
            .iter()
            .any(|badge| badge.label == "serving")
    );
    assert!(
        anthropic
            .badges
            .iter()
            .any(|badge| badge.label == "fallback")
    );
    assert!(
        anthropic
            .metadata
            .iter()
            .any(|item| item == "route: fallback serving")
    );
}

#[test]
fn auth_menu_summary_includes_route_state_and_warning() {
    let mut app = test_app();
    app.route_state = Some("fallback".into());
    app.route_selected_model = Some("openai-codex:gpt-5.4".into());
    app.route_serving_model = Some("anthropic:claude-sonnet-4-6".into());
    app.footer_data.route_warning = Some("selected provider unavailable".into());

    app.open_auth_menu();

    let summary = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.projection.summary.as_deref())
        .expect("summary");
    assert!(summary.contains("route: fallback"), "{summary}");
    assert!(
        summary.contains("selected: openai-codex:gpt-5.4"),
        "{summary}"
    );
    assert!(
        summary.contains("serving: anthropic:claude-sonnet-4-6"),
        "{summary}"
    );
    assert!(
        summary.contains("selected provider unavailable"),
        "{summary}"
    );
}

#[test]
fn model_menu_summary_uses_configured_model_label() {
    let mut app = test_app();

    app.open_model_menu();

    let summary = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.projection.summary.as_deref())
        .expect("summary");
    assert!(summary.contains("Configured model:"), "{summary}");
    assert!(!summary.contains("Current model:"), "{summary}");
}

#[test]
fn provider_rows_mark_selected_and_serving_route_roles() {
    let mut app = test_app();
    app.route_selected_model = Some("openai-codex:gpt-5.4".into());
    app.route_serving_model = Some("anthropic:claude-sonnet-4-6".into());

    let rows = app.provider_status_rows("provider");
    let openai = rows
        .iter()
        .find(|row| row.id == "provider.openai-codex")
        .expect("openai-codex row");
    let anthropic = rows
        .iter()
        .find(|row| row.id == "provider.anthropic")
        .expect("anthropic row");

    assert!(openai.badges.iter().any(|badge| badge.label == "selected"));
    assert!(openai.metadata.iter().any(|item| item == "route: selected"));
    assert!(
        anthropic
            .badges
            .iter()
            .any(|badge| badge.label == "serving")
    );
    assert!(
        anthropic
            .metadata
            .iter()
            .any(|item| item == "route: serving")
    );
}

#[test]
fn model_menu_summary_includes_route_state_and_warning() {
    let mut app = test_app();
    app.route_state = Some("fallback".into());
    app.route_selected_model = Some("openai-codex:gpt-5.4".into());
    app.route_serving_model = Some("anthropic:claude-sonnet-4-6".into());
    app.footer_data.route_warning = Some("selected provider unavailable".into());

    app.open_model_menu();

    let summary = app
        .active_menu
        .as_ref()
        .and_then(|menu| menu.projection.summary.as_deref())
        .expect("summary");
    assert!(summary.contains("route: fallback"), "{summary}");
    assert!(
        summary.contains("selected: openai-codex:gpt-5.4"),
        "{summary}"
    );
    assert!(
        summary.contains("serving: anthropic:claude-sonnet-4-6"),
        "{summary}"
    );
    assert!(
        summary.contains("selected provider unavailable"),
        "{summary}"
    );
}

#[test]
fn slash_model_opens_model_menu() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/model", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("model menu");
    assert_eq!(menu.projection.id, "model");
    assert!(
        menu.state
            .visible_rows(&menu.projection)
            .iter()
            .any(|row| row.row.id == "model.current")
    );
}

#[test]
fn slash_model_providers_opens_provider_status_tab() {
    let mut app = test_app();
    let tx = test_tx();

    let result = app.handle_slash_command("/model providers", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("model menu");
    assert_eq!(menu.projection.id, "model");
    assert_eq!(menu.state.active_tab, "providers");
    let rows = menu.state.visible_rows(&menu.projection);
    assert!(rows.iter().any(|row| row.row.id.starts_with("provider.")));
    assert!(rows.iter().any(|row| {
        row.row
            .metadata
            .iter()
            .any(|item| item.starts_with("/login "))
    }));
}

#[test]
fn model_menu_action_keys_select_intent_rows() {
    let mut app = test_app();
    app.open_model_menu();

    let menu = app.active_menu.as_mut().expect("model menu");
    let target = menu
        .state
        .row_target_for_action_key(&menu.projection, 'g')
        .expect("grade target");
    assert_eq!(target, "model.grade");
    assert!(menu.state.select_row_by_id(&menu.projection, &target));
    assert_eq!(
        menu.state
            .selected_row(&menu.projection)
            .map(|row| row.row.id.as_str()),
        Some("model.grade")
    );
}

#[test]
fn model_grade_slash_command_parses_and_rejects_local_grade() {
    assert_eq!(crate::tui::canonical_slash_command("model", "route"), None);
    assert_eq!(
        crate::tui::canonical_slash_command("model", "providers"),
        Some(crate::tui::CanonicalSlashCommand::ModelList)
    );
    assert_eq!(
        crate::tui::canonical_slash_command("model", "grade S"),
        Some(crate::tui::CanonicalSlashCommand::SetModelGrade("S".into()))
    );
    assert_eq!(
        crate::tui::canonical_slash_command("model", "unpin"),
        Some(crate::tui::CanonicalSlashCommand::ModelUnpin)
    );
    assert_eq!(
        crate::tui::canonical_slash_command("model", "grade local"),
        None
    );
}

#[test]
fn legacy_model_tier_slash_commands_are_unknown() {
    let mut app = test_app();
    let tx = test_tx();

    for command in [
        "/gloriana",
        "/victory",
        "/retribution",
        "/opus",
        "/sonnet",
        "/haiku",
    ] {
        let result = app.handle_slash_command(command, &tx);
        match result {
            SlashResult::Display(text) => {
                assert!(text.contains("Unknown command"), "{command} got: {text}");
            }
            other => panic!("{command} should be unknown, got: {other:?}"),
        }
    }
}

#[test]
fn variables_menu_inventory_rows_offer_update_and_delete() {
    let mut app = test_app();
    let name = format!("PROJECT_ENV_MENU_TEST_{}", std::process::id());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(crate::control::variables::variables_set_response(
        &name, "staging",
    ));
    app.open_variables_menu();

    let menu = app.active_menu.as_ref().expect("variables menu");
    let row_id = format!("variables.inventory.{name}");
    let row = menu
        .projection
        .tabs
        .iter()
        .find(|tab| tab.id == "inventory")
        .and_then(|tab| tab.groups[0].rows.iter().find(|row| row.id == row_id))
        .expect("variable inventory row");

    let update = row.primary_action.as_ref().expect("update primary action");
    assert_eq!(update.label, "Update");
    assert_eq!(
        update.editor_text.as_deref(),
        Some(format!("/variables set {name} ").as_str())
    );

    let get = row
        .actions
        .iter()
        .find(|action| action.label == "Show value")
        .expect("show value action");
    assert_eq!(
        get.command.as_deref(),
        Some(format!("/variables get {name}").as_str())
    );

    let delete = row
        .actions
        .iter()
        .find(|action| action.label == "Delete")
        .expect("delete action");
    assert_eq!(
        delete.editor_text.as_deref(),
        Some(format!("/variables delete {name}").as_str())
    );
    runtime.block_on(crate::control::variables::variables_delete_response(&name));
}

#[test]
fn secrets_menu_inventory_rows_offer_safe_crud_actions() {
    let mut app = test_app();
    app.secret_readiness = Some(crate::capabilities::secrets::SecretReadinessSnapshot {
        secrets: vec![crate::capabilities::secrets::SecretReadiness {
            name: "GITHUB_TOKEN".into(),
            required: true,
            optional: false,
            consumers: vec![],
            status: crate::capabilities::secrets::SecretReadinessStatus::Configured,
            recipe_kind: Some("env".into()),
            warmed: false,
        }],
        harness_capabilities: vec![],
    });

    app.open_secrets_menu();
    let menu = app.active_menu.as_ref().expect("secrets menu");
    let row = menu
        .projection
        .tabs
        .iter()
        .find(|tab| tab.id == "inventory")
        .and_then(|tab| {
            tab.groups[0]
                .rows
                .iter()
                .find(|row| row.id == "secrets.inventory.GITHUB_TOKEN")
        })
        .expect("GITHUB_TOKEN inventory row");

    assert_eq!(
        row.primary_action
            .as_ref()
            .and_then(|action| action.command.as_deref()),
        Some("/secrets get GITHUB_TOKEN")
    );
    assert!(
        row.actions
            .iter()
            .any(|action| action.editor_text.as_deref() == Some("/secrets set GITHUB_TOKEN")),
        "missing hidden set/replace action"
    );
    for expected in [
        "/secrets set GITHUB_TOKEN env:",
        "/secrets set GITHUB_TOKEN cmd:",
        "/secrets set GITHUB_TOKEN vault:",
        "/secrets delete GITHUB_TOKEN",
    ] {
        assert!(
            row.actions
                .iter()
                .any(|action| action.editor_text.as_deref() == Some(expected)),
            "missing safe secret action for {expected}"
        );
    }
    assert!(row.metadata.iter().any(|item| item == "value redacted"));
}

#[test]
fn secrets_menu_inventory_includes_first_party_catalog_rows() {
    let mut app = test_app();
    app.secret_readiness = Some(
        crate::capabilities::secrets::build_secret_readiness_snapshot(
            &[],
            &[],
            crate::capabilities::secrets::SecretReadinessInputs::default(),
        ),
    );

    app.open_secrets_menu();
    let menu = app.active_menu.as_ref().expect("secrets menu");
    let inventory = menu
        .projection
        .tabs
        .iter()
        .find(|tab| tab.id == "inventory")
        .expect("inventory tab");
    assert!(
        inventory.groups[0]
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("Known and declared")
    );

    let row = inventory.groups[0]
        .rows
        .iter()
        .find(|row| row.id == "secrets.inventory.BRAVE_API_KEY")
        .expect("BRAVE_API_KEY inventory row");

    assert_eq!(row.label, "BRAVE_API_KEY");
    assert!(
        row.metadata
            .iter()
            .any(|item| item == "consumer: HarnessCapability:web_search")
    );
    assert_eq!(
        row.primary_action
            .as_ref()
            .and_then(|action| action.command.as_deref()),
        Some("/secrets get BRAVE_API_KEY")
    );
    assert!(
        row.actions
            .iter()
            .any(|action| action.editor_text.as_deref() == Some("/secrets set BRAVE_API_KEY")),
        "missing hidden set/replace action"
    );
    for expected in [
        "/secrets set BRAVE_API_KEY env:",
        "/secrets set BRAVE_API_KEY cmd:",
        "/secrets set BRAVE_API_KEY vault:",
        "/secrets delete BRAVE_API_KEY",
    ] {
        assert!(
            row.actions
                .iter()
                .any(|action| action.editor_text.as_deref() == Some(expected)),
            "missing safe secret action for {expected}"
        );
    }
}

#[test]
fn secrets_menu_capabilities_tab_groups_first_party_secret_readiness() {
    let mut app = test_app();
    app.secret_readiness = Some(
        crate::capabilities::secrets::build_secret_readiness_snapshot(
            &[],
            &[],
            crate::capabilities::secrets::SecretReadinessInputs {
                session_diagnostics: Vec::new(),
                recipe_descriptors: vec![
                    crate::capabilities::secrets::SecretRecipeDescriptorSummary {
                        name: "BRAVE_API_KEY".into(),
                        kind: "env".into(),
                    },
                ],
                checked_names: Vec::new(),
            },
        ),
    );

    app.open_secrets_menu();
    let menu = app.active_menu.as_ref().expect("secrets menu");
    let capabilities = menu
        .projection
        .tabs
        .iter()
        .find(|tab| tab.id == "capabilities")
        .expect("capabilities tab");
    let row = capabilities.groups[0]
        .rows
        .iter()
        .find(|row| row.id == "secrets.capabilities.web_search")
        .expect("web_search capability row");

    assert_eq!(row.label, "Web search and external evidence");
    assert_eq!(row.value.as_deref(), Some("ready"));
    assert!(
        row.metadata
            .iter()
            .any(|item| item == "1 configured · 0 deferred · 4 known providers")
    );
    assert!(
        row.metadata
            .iter()
            .any(|item| { item == "policy: any configured provider enables this capability" })
    );
    assert!(row.metadata.iter().any(|item| item == "category: research"));
    assert!(
        row.metadata
            .iter()
            .any(|item| item == "secret: BRAVE_API_KEY")
    );
    assert!(
        row.metadata
            .iter()
            .any(|item| item == "secret: TAVILY_API_KEY")
    );
    assert!(row.actions.iter().any(|action| {
        action.label == "Set BRAVE_API_KEY"
            && action.editor_text.as_deref() == Some("/secrets set BRAVE_API_KEY")
    }));
}

#[test]
fn slash_variables_set_get_delete_queue_control_requests() {
    let mut app = test_app();
    let (tx, mut rx) = test_tx_with_rx();

    assert!(matches!(
        app.handle_slash_command("/variables set PROJECT_ENV staging", &tx),
        SlashResult::Handled
    ));
    match rx.try_recv().expect("set request") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::VariablesSet { name, value },
            ..
        } => {
            assert_eq!(name, "PROJECT_ENV");
            assert_eq!(value, "staging");
        }
        other => panic!("expected variables set request, got {other:?}"),
    }

    assert!(matches!(
        app.handle_slash_command("/variables get PROJECT_ENV", &tx),
        SlashResult::Handled
    ));
    match rx.try_recv().expect("get request") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::VariablesGet { name },
            ..
        } => assert_eq!(name, "PROJECT_ENV"),
        other => panic!("expected variables get request, got {other:?}"),
    }

    assert!(matches!(
        app.handle_slash_command("/variables delete PROJECT_ENV", &tx),
        SlashResult::Handled
    ));
    match rx.try_recv().expect("delete request") {
        TuiCommand::ExecuteControl {
            request: crate::control_runtime::ControlRequest::VariablesDelete { name },
            ..
        } => assert_eq!(name, "PROJECT_ENV"),
        other => panic!("expected variables delete request, got {other:?}"),
    }
}

#[test]
fn variables_command_is_advertised() {
    let variables = crate::command_registry::BUILTIN_COMMANDS
        .iter()
        .find(|entry| entry.name == "variables")
        .expect("variables command advertised");
    for subcommand in ["list", "status", "set", "get", "delete", "remove", "rm"] {
        assert!(variables.subcommands.contains(&subcommand));
    }
    let alias = crate::command_registry::BUILTIN_COMMANDS
        .iter()
        .find(|entry| entry.name == "vars")
        .expect("vars alias advertised");
    assert!(alias.subcommands.contains(&"set"));
}

#[test]
fn slash_init_opens_harness_init_menu() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let _cwd = push_current_dir(tmp.path());
    let mut app = test_app();
    app.footer_data.cwd = tmp.path().display().to_string();
    let tx = test_tx();

    let result = app.handle_slash_command("/init", &tx);

    assert!(matches!(result, SlashResult::Handled));
    let menu = app.active_menu.as_ref().expect("init menu");
    assert_eq!(menu.projection.id, "init");
    assert!(
        menu.projection
            .summary
            .as_deref()
            .unwrap_or_default()
            .contains("Agent harness initialization defaults")
    );
    assert!(
        menu.projection
            .tabs
            .iter()
            .any(|tab| { tab.groups.iter().any(|group| group.id == "init.skills") })
    );
}

#[test]
fn slash_init_unknown_subcommand_is_non_mutating_usage_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    let nested = repo.join("src/nested");
    std::fs::create_dir_all(&nested).expect("nested project path");
    std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("cargo");
    let _cwd = push_current_dir(&nested);
    let mut app = test_app();
    app.footer_data.cwd = nested.display().to_string();
    let tx = test_tx();

    let nested_omegon_existed = nested.join(".omegon").exists();
    let nested_memory_existed = nested.join("ai/memory").exists();
    let result = app.handle_slash_command("/init typo", &tx);

    match result {
        SlashResult::Display(message) => {
            assert!(message.contains("Usage: /init"), "{message}");
            assert!(message.contains("Unknown subcommand: typo"), "{message}");
        }
        other => panic!("expected init usage error, got {other:?}"),
    }
    assert!(!repo.join(".omegon").exists());
    assert!(!repo.join("ai/memory").exists());
    assert_eq!(nested.join(".omegon").exists(), nested_omegon_existed);
    assert_eq!(nested.join("ai/memory").exists(), nested_memory_existed);
}

#[test]
fn slash_init_scan_targets_detected_project_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    let nested = repo.join("src/nested");
    std::fs::create_dir_all(&nested).expect("nested project path");
    std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("cargo");
    let _cwd = push_current_dir(&nested);
    let mut app = test_app();
    app.footer_data.cwd = nested.display().to_string();
    let tx = test_tx();

    let nested_omegon_existed = nested.join(".omegon").exists();
    let nested_memory_existed = nested.join("ai/memory").exists();
    let result = app.handle_slash_command("/init scan", &tx);

    assert!(matches!(result, SlashResult::Display(_)));
    assert!(repo.join(".omegon").is_dir());
    assert!(repo.join("ai/memory").is_dir());
    assert_eq!(nested.join(".omegon").exists(), nested_omegon_existed);
    assert_eq!(nested.join("ai/memory").exists(), nested_memory_existed);
}

#[test]
fn init_menu_recommends_matching_user_skill_for_project_copy() {
    let _env = crate::test_support::env::lock();
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    std::fs::write(repo.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("cargo");
    let skill_dir = home.join(".omegon/skills/rust-helper");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: rust-helper\ndescription: Rust helper\nactivation: project_detected\nprofile: [coding]\nproject_signals: [Cargo.toml]\n---\n\n# Rust helper\n",
    )
    .expect("skill");
    unsafe { std::env::set_var("HOME", &home) };
    let _cwd = push_current_dir(&repo);
    let mut app = test_app();
    app.footer_data.cwd = repo.display().to_string();

    app.open_init_menu();
    let menu = app.active_menu.as_ref().expect("init menu");
    let skills_group = menu
        .projection
        .tabs
        .iter()
        .flat_map(|tab| &tab.groups)
        .find(|group| group.id == "init.skills")
        .expect("skills group");
    let row = skills_group
        .rows
        .iter()
        .find(|row| row.id == "init.skill.rust-helper")
        .expect("rust-helper recommendation");

    assert!(row.description.contains("Cargo.toml"));
    let action = row.primary_action.as_ref().expect("primary action");
    assert_eq!(action.label, "Copy to project");
    assert!(
        action
            .command
            .as_deref()
            .unwrap_or_default()
            .starts_with("/skills import --project ")
    );
}
