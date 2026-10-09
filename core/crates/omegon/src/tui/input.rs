//! Native terminal input routing for the Ratatui frontend.
//!
//! Terminal polling and ownership remain in `run_tui`; this adapter owns modal
//! precedence and routes one decoded crossterm event into `App` state/actions.

use super::interaction::NavigationOwner;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputDisposition {
    Continue,
    SkipLoop,
}

impl App {
    pub(super) async fn handle_terminal_event(
        &mut self,
        input_event: Event,
        command_tx: &OperatorCommandTx,
    ) -> InputDisposition {
        self.expire_navigation_overlay();
        let owner = self.navigation_owner();
        let blocking_owner = self.blocking_owner();
        if owner != NavigationOwner::Composer
            && !matches!(input_event, Event::Key(_) | Event::Resize(..))
        {
            return InputDisposition::SkipLoop;
        }
        match input_event {
            // ── Mouse scroll ────────────────────────────────────────
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) if self.mouse_capture_enabled => {
                    let point_in = |area: Option<Rect>| {
                        area.is_some_and(|area| {
                            mouse.column >= area.x
                                && mouse.column < area.x + area.width
                                && mouse.row >= area.y
                                && mouse.row < area.y + area.height
                        })
                    };

                    if point_in(self.dashboard_area) {
                        self.dashboard.sidebar_active = true;
                    } else if point_in(self.workbench_area) {
                        self.dashboard.sidebar_active = false;
                        let now = std::time::Instant::now();
                        let is_double = self.last_left_click.is_some_and(|(col, row, t, _)| {
                            row == mouse.row
                                && col.abs_diff(mouse.column) <= 1
                                && row.abs_diff(mouse.row) <= 1
                                && now.duration_since(t) <= Duration::from_millis(400)
                        });
                        if is_double {
                            self.expand_workbench_plan_details();
                        }
                        self.last_left_click = Some((mouse.column, mouse.row, now, None));
                    } else if point_in(self.conversation_area) {
                        self.dashboard.sidebar_active = false;
                        if let Some(area) = self.conversation_area {
                            if let Some(idx) = self.conversation.assistant_copy_button_at(
                                area,
                                mouse.column,
                                mouse.row,
                            ) {
                                let _ = self.handle_select_conversation_segment_action(
                                    SelectConversationSegmentAction {
                                        segment: ConversationSegmentRef::by_index(idx),
                                    },
                                );
                                let outcome = self.handle_copy_conversation_segment_action(
                                    CopyConversationSegmentAction {
                                        segment: ConversationSegmentRef::by_index(idx),
                                        mode: SegmentCopyMode::Plaintext,
                                    },
                                );
                                match outcome {
                                    UiActionOutcome::Accepted { .. } => self.show_toast(
                                        "Copied assistant response",
                                        ratatui_toaster::ToastType::Success,
                                    ),
                                    UiActionOutcome::Rejected { reason }
                                    | UiActionOutcome::Noop { reason }
                                    | UiActionOutcome::Deferred { reason } => self
                                        .show_toast(&reason, ratatui_toaster::ToastType::Warning),
                                }
                                return InputDisposition::SkipLoop;
                            }
                            let now = std::time::Instant::now();
                            let prior_double_target = semantic_double_click_target(
                                self.last_left_click,
                                mouse.column,
                                mouse.row,
                                now,
                            );
                            // Selecting the first click adds a focus rail/hint and can
                            // reflow short, bottom-aligned content. Prefer the semantic
                            // target captured on that first press, rather than hit-testing
                            // the second press against the newly shifted frame.
                            let projection = conversation_projection::project_conversation(
                                self.conversation.segments(),
                                self.ui_presentation.level,
                            );
                            let hit_idx = self.conversation.projected_segment_at(
                                area,
                                mouse.row,
                                &projection.canonical_indices,
                            );
                            if let Some(idx) = prior_double_target.or(hit_idx) {
                                let is_double = prior_double_target == Some(idx);
                                let _ = self.handle_select_conversation_segment_action(
                                    SelectConversationSegmentAction {
                                        segment: ConversationSegmentRef::by_index(idx),
                                    },
                                );
                                if is_double {
                                    if self.open_selected_terminal_process_viewer()
                                        || self.conversation.toggle_image_attachments_at(idx) > 0
                                    {
                                        self.effects.pulse_conversation_action();
                                    } else if self.conversation.is_segment_collapsed_tool_card(idx)
                                    {
                                        self.conversation.toggle_expand(idx);
                                        self.show_toast(
                                            "Expanded selected tool result",
                                            ratatui_toaster::ToastType::Success,
                                        );
                                        self.effects.pulse_conversation_action();
                                    } else if self.conversation.is_segment_copyable(idx) {
                                        self.copy_selected_conversation_segment_with_mode(
                                            SegmentExportMode::Plaintext,
                                        );
                                    }
                                }
                                self.last_left_click =
                                    Some((mouse.column, mouse.row, now, Some(idx)));
                            }
                        }
                    } else if point_in(self.editor_area) {
                        self.dashboard.sidebar_active = false;
                    }
                }
                MouseEventKind::ScrollUp => {
                    // Mouse wheel is scroll provenance, not keyboard Up.
                    // It must never route through editor history recall.
                    self.handle_mouse_scroll_up(mouse.column, mouse.row);
                }
                MouseEventKind::ScrollDown => {
                    // Mouse wheel is scroll provenance, not keyboard Down.
                    // It must never route through editor history advance/clear.
                    self.handle_mouse_scroll_down(mouse.column, mouse.row);
                }
                _ => {}
            },
            // ── Paste — pass directly to textarea ──────────
            Event::Paste(ref text) => {
                if self.editor_input_suppressed() {
                    return InputDisposition::SkipLoop;
                }
                if matches!(self.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                    // In secret mode, paste goes into the hidden buffer
                    for c in text.chars() {
                        self.editor.secret_insert(c);
                    }
                } else if text.is_empty() {
                    self.pending_history_preload = None;
                    self.try_paste_clipboard_image();
                } else {
                    let _ = self
                        .handle_ui_action(
                            UiAction::InsertComposerText(InsertComposerTextAction {
                                text: text.clone(),
                            }),
                            command_tx,
                        )
                        .await;
                }
            }
            // ── Ctrl+V: check for clipboard image ──────────
            Event::Key(KeyEvent {
                code: KeyCode::Char('v'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => {
                if owner != NavigationOwner::Composer {
                    return InputDisposition::SkipLoop;
                }
                if matches!(self.editor.mode(), editor::EditorMode::SecretInput { .. }) {
                    // In secret mode, try to paste from clipboard into hidden buffer
                    // (Ctrl+V may deliver text as a Key event on some terminals)
                } else {
                    self.try_paste_clipboard_image();
                }
            }
            Event::Key(key) => {
                if blocking_owner.is_none()
                    && self.agent_active
                    && key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    let _ = self
                        .handle_ui_action(UiAction::CancelActiveTurn, command_tx)
                        .await;
                    return InputDisposition::SkipLoop;
                }
                if owner == NavigationOwner::Composer && key.code == KeyCode::F(2) {
                    self.open_project_browser();
                    return InputDisposition::SkipLoop;
                }
                match owner {
                    NavigationOwner::Project => {
                        self.handle_project_browser_key(key, command_tx);
                        return InputDisposition::SkipLoop;
                    }
                    NavigationOwner::ExtensionAction => {
                        if key.code == KeyCode::Esc {
                            self.active_action_prompt = None;
                        } else if let KeyCode::Char(c) = key.code
                            && let Some(index) = c.to_digit(10).and_then(|n| n.checked_sub(1))
                            && self
                                .active_action_prompt
                                .as_ref()
                                .is_some_and(|(_, actions)| (index as usize) < actions.len())
                        {
                            self.show_toast(
                                "This extension does not accept action responses",
                                ratatui_toaster::ToastType::Warning,
                            );
                        }
                        return InputDisposition::SkipLoop;
                    }
                    NavigationOwner::ExtensionModal => {
                        if key.code == KeyCode::Esc {
                            self.active_modal = None;
                        }
                        return InputDisposition::SkipLoop;
                    }
                    NavigationOwner::Prompt => {
                        if key.code == KeyCode::Esc {
                            self.command_prompt = None;
                        }
                        return InputDisposition::SkipLoop;
                    }
                    _ => {}
                }

                // A passive process viewer can be dismissed when no decision
                // owns input. During a decision, Escape belongs to its prompt.
                if owner == NavigationOwner::Process
                    && matches!(
                        key.code,
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
                    )
                {
                    let cancelled = self
                        .process_viewer
                        .as_mut()
                        .is_some_and(|viewer| viewer.cancel_confirmation());
                    if !cancelled {
                        self.process_viewer = None;
                    }
                    return InputDisposition::SkipLoop;
                }

                // Blocking responder-backed prompts own input before passive panels,
                // scrollback controls, selectors, or editor actions.
                if blocking_owner == Some(interaction::BlockingOwner::OperatorWait) {
                    let response = match key.code {
                        KeyCode::Enter
                        | KeyCode::Char(' ')
                        | KeyCode::Char('d')
                        | KeyCode::Char('D') => {
                            Some(omegon_traits::OperatorWaitResponse::Completed)
                        }
                        KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                            Some(omegon_traits::OperatorWaitResponse::Cancelled)
                        }
                        _ => None,
                    };
                    if let Some(response) = response {
                        let _ = self
                            .handle_ui_action(
                                UiAction::RespondToOperatorWait(OperatorWaitAction {
                                    request_id: None,
                                    response,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    return InputDisposition::SkipLoop;
                }

                if blocking_owner == Some(interaction::BlockingOwner::Permission) {
                    let response = permission_response_for_key(key.code, key.modifiers);
                    if let Some(response) = response {
                        let _ = self
                            .handle_ui_action(
                                UiAction::RespondToPermission(PermissionAction {
                                    request_id: None,
                                    response,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    return InputDisposition::SkipLoop;
                }

                if owner == NavigationOwner::Copy
                    && let Some(copy_modal) = self.copy_text_modal.as_mut()
                {
                    match (key.code, key.modifiers) {
                        (KeyCode::Esc, _) => {
                            self.close_copy_text_modal();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                            let _ = self.copy_all_from_copy_text_modal();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Up, _) => {
                            copy_modal.scroll_up(1);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Down, _) => {
                            copy_modal.scroll_down(1);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::PageUp, _) => {
                            copy_modal.scroll_up(20);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::PageDown, _) => {
                            copy_modal.scroll_down(20);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Home, _) => {
                            copy_modal.scroll_top();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::End, _) => {
                            copy_modal.scroll_bottom();
                            return InputDisposition::SkipLoop;
                        }
                        _ => {}
                    }
                    return InputDisposition::SkipLoop;
                }

                if owner == NavigationOwner::Panel
                    && let Some(panel) = self.command_panel.as_mut()
                {
                    match (key.code, key.modifiers) {
                        (KeyCode::Esc, _) => {
                            self.close_command_panel_to_return_target();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Char('q'), _) if panel.return_target.is_some() => {
                            self.close_command_panel_stack();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Up, _) => {
                            panel.scroll_up(3);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Down, _) => {
                            panel.scroll_down(3);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::PageUp, _) => {
                            panel.scroll_up(20);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::PageDown, _) => {
                            panel.scroll_down(20);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Home, _) => {
                            panel.scroll_top();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::End, _) => {
                            panel.scroll_bottom();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Char('y'), KeyModifiers::CONTROL) if panel.copyable => {
                            let text = panel.body.clone();
                            if self.copy_text_to_clipboard(&text) {
                                self.show_toast(
                                    "Copied command panel",
                                    ratatui_toaster::ToastType::Success,
                                );
                            } else {
                                self.show_toast(
                                        "Clipboard unavailable — select panel text in your terminal or install pbcopy/wl-copy/xclip",
                                        ratatui_toaster::ToastType::Warning,
                                    );
                            }
                            return InputDisposition::SkipLoop;
                        }
                        _ => {}
                    }
                    return InputDisposition::SkipLoop;
                }

                // Global conversation controls must remain live while the
                // agent/tool loop is active. Handle them before editor,
                // selector, permission, or interrupt-debounce paths can
                // consume the key event.
                if owner == NavigationOwner::Composer {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                            self.conversation.toggle_pin();
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Up, modifiers)
                            if modifiers.contains(KeyModifiers::ALT)
                                && modifiers.contains(KeyModifiers::SHIFT) =>
                        {
                            if self.conversation.move_to_operator_prompt(true).is_some() {
                                self.conversation.conv_state.snap_to_selected();
                            }
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Down, modifiers)
                            if modifiers.contains(KeyModifiers::ALT)
                                && modifiers.contains(KeyModifiers::SHIFT) =>
                        {
                            if self.conversation.move_to_operator_prompt(false).is_some() {
                                self.conversation.conv_state.snap_to_selected();
                            }
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::PageUp, _) => {
                            self.conversation.scroll_up(20);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::PageDown, _) => {
                            self.conversation.scroll_down(20);
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::Home, _) => {
                            self.conversation.conv_state.scroll_offset = u16::MAX;
                            self.conversation.conv_state.user_scrolled = true;
                            return InputDisposition::SkipLoop;
                        }
                        (KeyCode::End, _) => {
                            self.conversation.scroll_down(u16::MAX);
                            return InputDisposition::SkipLoop;
                        }
                        _ => {}
                    }
                }

                if owner == NavigationOwner::Composer
                    && self.should_discard_key_after_interrupt(&key)
                {
                    return InputDisposition::SkipLoop;
                }

                // ── Structured menu intercepts navigation when open ────
                if owner == NavigationOwner::Process {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.scroll_up(1);
                            }
                        }
                        KeyCode::PageUp | KeyCode::Char('u') => {
                            let page_size = crossterm::terminal::size()
                                .map(|(width, height)| {
                                    process_viewer::process_viewer_page_size(
                                        ratatui::layout::Rect::new(0, 0, width, height),
                                    )
                                })
                                .unwrap_or(10);
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.scroll_up(page_size);
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.scroll_down(1);
                            }
                        }
                        KeyCode::PageDown | KeyCode::Char('d') => {
                            let page_size = crossterm::terminal::size()
                                .map(|(width, height)| {
                                    process_viewer::process_viewer_page_size(
                                        ratatui::layout::Rect::new(0, 0, width, height),
                                    )
                                })
                                .unwrap_or(10);
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.scroll_down(page_size);
                            }
                        }
                        KeyCode::Home => {
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.jump_to_top();
                            }
                        }
                        KeyCode::End | KeyCode::Char('f') | KeyCode::Char('F') => {
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.follow_tail();
                            }
                        }
                        KeyCode::Left => {
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.switch_session(-1);
                            }
                        }
                        KeyCode::Right => {
                            if let Some(viewer) = self.process_viewer.as_mut() {
                                viewer.switch_session(1);
                            }
                        }
                        KeyCode::Char('x') | KeyCode::Char('X') => {
                            let session_id = self.process_viewer.as_mut().and_then(|viewer| {
                                viewer.request_stop().then(|| viewer.session_id.clone())
                            });
                            if let Some(session_id) = session_id {
                                match crate::tools::terminal::stop_execution_session(&session_id)
                                    .await
                                {
                                    Ok(()) => {
                                        let replacement =
                                            crate::tools::terminal::execution_session_snapshots()
                                                .into_iter()
                                                .next()
                                                .map(|snapshot| snapshot.id);
                                        self.process_viewer = replacement
                                            .map(process_viewer::ProcessViewerState::new);
                                        self.show_command_toast(CommandToast::new(
                                            "Managed process stopped",
                                            CommandSeverity::Success,
                                        ));
                                    }
                                    Err(err) => self.show_command_toast(CommandToast::new(
                                        format!("Could not stop managed process: {err}"),
                                        CommandSeverity::Error,
                                    )),
                                }
                            }
                        }
                        KeyCode::Esc => {
                            let cancelled = self
                                .process_viewer
                                .as_mut()
                                .is_some_and(|viewer| viewer.cancel_confirmation());
                            if !cancelled {
                                self.process_viewer = None;
                            }
                        }
                        _ => {}
                    }
                    return InputDisposition::SkipLoop;
                }
                if owner == NavigationOwner::Menu {
                    if self.menu_input.is_some() {
                        match key.code {
                            KeyCode::Char(ch)
                                if !key.modifiers.contains(KeyModifiers::CONTROL)
                                    && !key.modifiers.contains(KeyModifiers::ALT) =>
                            {
                                if let Some(input) = self.menu_input.as_mut() {
                                    input.value.push(ch);
                                    if let Some(menu) = self.active_menu.as_mut() {
                                        menu.projection.footer = Some(format!(
                                            "{}: {}▌ · Enter execute · Esc cancel",
                                            input.action_label, input.value
                                        ));
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if let Some(input) = self.menu_input.as_mut() {
                                    input.value.pop();
                                    if let Some(menu) = self.active_menu.as_mut() {
                                        menu.projection.footer = Some(format!(
                                            "{}: {}▌ · Enter execute · Esc cancel",
                                            input.action_label, input.value
                                        ));
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(input) = self.menu_input.take() {
                                    let value = input.value.trim();
                                    if value.is_empty() {
                                        if let Some(menu) = self.active_menu.as_mut() {
                                            menu.projection.footer = input.original_footer;
                                        }
                                    } else {
                                        let command = format!("{}{}", input.command_prefix, value);
                                        self.execute_active_menu_command(command, command_tx);
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                if let Some(input) = self.menu_input.take()
                                    && let Some(menu) = self.active_menu.as_mut()
                                {
                                    menu.projection.footer = input.original_footer;
                                }
                            }
                            _ => {}
                        }
                        return InputDisposition::SkipLoop;
                    }
                    if matches!(key.code, KeyCode::Esc)
                        && self.should_discard_key_after_interrupt(&key)
                    {
                        return InputDisposition::SkipLoop;
                    }
                    match key.code {
                        KeyCode::Up => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.move_up();
                            }
                        }
                        KeyCode::Down => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.move_down(&menu.projection);
                            }
                        }
                        KeyCode::PageUp => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.move_page(&menu.projection, -1);
                            }
                        }
                        KeyCode::PageDown => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.move_page(&menu.projection, 1);
                            }
                        }
                        KeyCode::Home => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.move_home();
                            }
                        }
                        KeyCode::End => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.move_end(&menu.projection);
                            }
                        }
                        KeyCode::Tab => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.next_tab(&menu.projection);
                            }
                        }
                        KeyCode::BackTab => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.previous_tab(&menu.projection);
                            }
                        }
                        KeyCode::Char('/') => {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.enter_search();
                            }
                        }
                        KeyCode::Char(ch)
                            if self
                                .active_menu
                                .as_ref()
                                .is_some_and(|menu| menu.state.mode == MenuMode::Search)
                                && !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.push_filter_char(&menu.projection, ch);
                            }
                        }
                        KeyCode::Backspace
                            if self
                                .active_menu
                                .as_ref()
                                .is_some_and(|menu| menu.state.mode == MenuMode::Search) =>
                        {
                            if let Some(menu) = self.active_menu.as_mut() {
                                menu.state.pop_filter_char(&menu.projection);
                            }
                        }
                        KeyCode::Char('s') | KeyCode::Char('S')
                            if self
                                .active_menu
                                .as_ref()
                                .is_some_and(|menu| menu.projection.id == "settings") =>
                        {
                            self.queue_settings_profile_save(command_tx);
                        }
                        KeyCode::Char('a') | KeyCode::Char('A')
                            if self
                                .active_menu
                                .as_ref()
                                .is_some_and(|menu| menu.projection.id == "settings") =>
                        {
                            self.queue_settings_profile_apply(command_tx);
                        }
                        KeyCode::Char(ch)
                            if self
                                .active_menu
                                .as_ref()
                                .is_some_and(|menu| menu.state.mode != MenuMode::Search)
                                && !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            let action = self.active_menu.as_ref().and_then(|menu| {
                                menu.state.selected_action_for_key(&menu.projection, ch)
                            });
                            if let Some(action) = action
                                && matches!(
                                    self.execute_active_menu_action(action, command_tx),
                                    SlashResult::Quit
                                )
                            {
                                let _ = command_tx.send(TuiCommand::Quit { confirmed: true }).await;
                            }
                        }
                        KeyCode::Enter => {
                            let action = self.active_menu.as_ref().and_then(|menu| {
                                menu.state.selected_primary_action(&menu.projection)
                            });
                            if let Some(action) = action
                                && matches!(
                                    self.execute_active_menu_action(action, command_tx),
                                    SlashResult::Quit
                                )
                            {
                                let _ = command_tx.send(TuiCommand::Quit { confirmed: true }).await;
                            }
                        }
                        KeyCode::Esc => {
                            let handled = self
                                .active_menu
                                .as_mut()
                                .is_some_and(|menu| menu.state.exit_search());
                            if !handled {
                                let is_extension_detail =
                                    self.active_menu.as_ref().is_some_and(|menu| {
                                        menu.projection.id.starts_with("extension-detail:")
                                    });
                                if is_extension_detail {
                                    self.open_extension_runtime_menu();
                                } else {
                                    self.active_menu = None;
                                    self.pending_menu_confirmation = None;
                                }
                            }
                        }
                        KeyCode::Char('c') | KeyCode::Char('C')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            self.active_menu = None;
                        }
                        _ => {}
                    }
                    return InputDisposition::SkipLoop;
                }

                // ── Selector popup intercepts all keys when open ────
                if owner == NavigationOwner::Selector {
                    match key.code {
                        KeyCode::Up => {
                            if let Some(ref mut s) = self.selector {
                                s.move_up();
                            }
                        }
                        KeyCode::Down => {
                            if let Some(ref mut s) = self.selector {
                                s.move_down();
                            }
                        }
                        KeyCode::Char(' ')
                            if matches!(
                                self.selector_kind,
                                Some(SelectorKind::ModelProviderInventory)
                            ) =>
                        {
                            let route = self
                                .selector
                                .as_ref()
                                .map(|s| s.selected_value().to_string());
                            if let Some(route) = route {
                                let mut preferences =
                                    crate::model_preferences::ModelMenuPreferences::load_default();
                                match preferences.toggle(&route).and_then(|favorite| {
                                    preferences.save_default()?;
                                    Ok(favorite)
                                }) {
                                    Ok(favorite) => {
                                        let provider = self.model_inventory_provider.clone();
                                        if let Some(provider) = provider {
                                            self.open_model_provider_inventory_selector(&provider);
                                        }
                                        self.show_toast(
                                            if favorite {
                                                "Added model favorite"
                                            } else {
                                                "Removed model favorite"
                                            },
                                            ratatui_toaster::ToastType::Info,
                                        );
                                    }
                                    Err(error) => self.show_toast(
                                        &format!("Could not save model favorite: {error}"),
                                        ratatui_toaster::ToastType::Error,
                                    ),
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(msg) = self.confirm_selector(command_tx) {
                                self.show_toast(&msg, ratatui_toaster::ToastType::Info);
                            }
                        }
                        KeyCode::Esc => {
                            self.selector = None;
                            self.selector_kind = None;
                        }
                        _ => {}
                    }
                    return InputDisposition::SkipLoop;
                }

                // ── Secret input mode intercepts keys ────────────
                if owner == NavigationOwner::Composer
                    && matches!(self.editor.mode(), editor::EditorMode::SecretInput { .. })
                {
                    match key.code {
                        KeyCode::Char(c) => {
                            self.editor.secret_insert(c);
                        }
                        KeyCode::Backspace => {
                            self.editor.secret_backspace();
                        }
                        KeyCode::Enter => {
                            if let Some((label, value)) = self.editor.take_secret() {
                                if value.is_empty() {
                                    self.operator_events.clear();
                                    self.show_command_toast(CommandToast::new(
                                        "Cancelled — no value entered",
                                        CommandSeverity::Warning,
                                    ));
                                } else {
                                    // The acquisition hint has served its purpose once a
                                    // value is submitted; do not leave it obscuring the TUI.
                                    self.operator_events.clear();
                                    // Store in secrets engine
                                    let Some(request) =
                                        crate::operator_commands::control_request_from_slash_command(
                                            &CanonicalSlashCommand::SecretsSet {
                                                name: label.clone(),
                                                value: value.clone(),
                                            },
                                        )
                                    else {
                                        self.show_command_toast(CommandToast::new(
                                            "Secret update is unavailable",
                                            CommandSeverity::Error,
                                        ));
                                        return InputDisposition::Continue;
                                    };
                                    let _ = command_tx
                                        .send(TuiCommand::ExecuteControl {
                                            request,
                                            respond_to: None,
                                        })
                                        .await;

                                    // Reflect keyed search readiness immediately in footer
                                    // chrome; the secret write has already been queued and
                                    // no restart is required by web_search resolution.
                                    if let Some(provider) = self
                                        .footer_data
                                        .web_search_providers
                                        .iter_mut()
                                        .find(|provider| provider.secret_name == label)
                                    {
                                        provider.configured = true;
                                    }

                                    // For provider keys, also write to auth.json so the
                                    // provider resolution chain finds them (/auth login checks
                                    // auth.json, not the secrets keyring)
                                    // Look up provider by env var name using canonical map
                                    let provider = crate::auth::PROVIDERS
                                        .iter()
                                        .find(|p| p.env_vars.contains(&label.as_str()));
                                    if let Some(p) = provider {
                                        let creds = crate::auth::OAuthCredentials {
                                            cred_type: "api-key".into(),
                                            access: value.clone(),
                                            refresh: String::new(),
                                            expires: u64::MAX,
                                        };
                                        let _ = crate::auth::write_credentials(p.auth_key, &creds);
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => {
                            self.editor.cancel_secret();
                            self.operator_events.clear();
                            self.active_menu = None;
                            self.show_command_toast(CommandToast::new(
                                "Secret input cancelled",
                                CommandSeverity::Info,
                            ));
                        }
                        _ => {}
                    }
                    return InputDisposition::SkipLoop;
                }

                // ── Reverse search mode intercepts keys ─────────
                if owner == NavigationOwner::Composer
                    && matches!(self.editor.mode(), editor::EditorMode::ReverseSearch { .. })
                {
                    match key.code {
                        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            // Ctrl+R again: search further back
                            self.editor.search_prev(&self.history);
                        }
                        KeyCode::Char(c) => {
                            self.editor.search_insert(c);
                            self.editor.search_update(&self.history);
                        }
                        KeyCode::Backspace => {
                            self.editor.search_backspace();
                            self.editor.search_update(&self.history);
                        }
                        KeyCode::Enter => {
                            self.editor.accept_search(&self.history);
                        }
                        KeyCode::Esc => {
                            self.editor.cancel_search();
                        }
                        _ => {
                            // Any other key: accept search + process key normally
                            self.editor.accept_search(&self.history);
                        }
                    }
                    return InputDisposition::SkipLoop;
                }

                // ── Tutorial overlay intercepts keys when active ────
                if let Some(ref mut overlay) = self.tutorial_overlay
                    && overlay.active
                {
                    let step_trigger = overlay.step().trigger.clone();
                    match key.code {
                        KeyCode::Esc => {
                            overlay.dismiss();
                            return InputDisposition::SkipLoop;
                        }
                        KeyCode::BackTab => {
                            overlay.go_back();
                            return InputDisposition::SkipLoop;
                        }
                        KeyCode::Tab => {
                            match &step_trigger {
                                tutorial::Trigger::Tab => {
                                    // Check BEFORE advance — fire side-effects for the step being dismissed
                                    let leaving_step_title = overlay.step().title;
                                    let should_open_dash =
                                        leaving_step_title == "Auspex Browser View";
                                    overlay.advance();
                                    let auto_prompt =
                                        overlay.pending_auto_prompt().map(|s| s.to_string());
                                    if auto_prompt.is_some() {
                                        overlay.mark_auto_prompt_sent();
                                    }
                                    // overlay borrow is released before touching self
                                    if let Some(prompt) = auto_prompt {
                                        if !self.agent_active {
                                            self.show_command_toast(CommandToast::new(
                                                "Tutorial step started",
                                                CommandSeverity::Info,
                                            ));
                                            self.agent_active = true;
                                            self.dashboard_handles.session().set_busy(true);
                                            let _ = command_tx
                                                .send(TuiCommand::SubmitPrompt(PromptSubmission {
                                                    text: prompt,
                                                    image_paths: Vec::new(),
                                                    submitted_by: "local-tui".to_string(),
                                                    via: "tui",
                                                    queue_mode: self.queue_mode,
                                                    metadata: PromptMetadata::default(),
                                                }))
                                                .await;
                                        } else {
                                            let _ = command_tx
                                                .send(TuiCommand::SubmitPrompt(PromptSubmission {
                                                    text: prompt,
                                                    image_paths: Vec::new(),
                                                    submitted_by: "local-tui".to_string(),
                                                    via: "tui",
                                                    queue_mode: self.queue_mode,
                                                    metadata: PromptMetadata::default(),
                                                }))
                                                .await;
                                        }
                                    }
                                    if should_open_dash {
                                        let _ =
                                            command_tx.send(TuiCommand::StartWebDashboard).await;
                                    }
                                    return InputDisposition::SkipLoop;
                                }
                                tutorial::Trigger::AutoPrompt(prompt) => {
                                    if !overlay.auto_prompt_sent {
                                        // Tab starts the auto-prompt
                                        let prompt = prompt.to_string();
                                        overlay.mark_auto_prompt_sent();
                                        if !self.agent_active {
                                            self.show_command_toast(CommandToast::new(
                                                "Tutorial step started",
                                                CommandSeverity::Info,
                                            ));
                                            self.agent_active = true;
                                            self.dashboard_handles.session().set_busy(true);
                                            let _ = command_tx
                                                .send(TuiCommand::SubmitPrompt(PromptSubmission {
                                                    text: prompt,
                                                    image_paths: Vec::new(),
                                                    submitted_by: "local-tui".to_string(),
                                                    via: "tui",
                                                    queue_mode: self.queue_mode,
                                                    metadata: PromptMetadata::default(),
                                                }))
                                                .await;
                                        } else {
                                            let _ = command_tx
                                                .send(TuiCommand::SubmitPrompt(PromptSubmission {
                                                    text: prompt,
                                                    image_paths: Vec::new(),
                                                    submitted_by: "local-tui".to_string(),
                                                    via: "tui",
                                                    queue_mode: self.queue_mode,
                                                    metadata: PromptMetadata::default(),
                                                }))
                                                .await;
                                        }
                                    }
                                    // If already sent, Tab does nothing — wait for agent
                                    return InputDisposition::SkipLoop;
                                }
                                tutorial::Trigger::Command(_) | tutorial::Trigger::AnyInput => {
                                    // Tab passes through to normal key handling (e.g., command completion)
                                }
                            }
                        }
                        KeyCode::Left | KeyCode::Right if overlay.showing_choice() => {
                            overlay.toggle_choice();
                            return InputDisposition::SkipLoop;
                        }
                        KeyCode::Enter if overlay.showing_choice() => {
                            overlay.confirm_choice();
                            if overlay.choice == tutorial::TutorialChoice::Demo {
                                // Demo mode needs the demo project — dismiss overlay
                                // and launch the clone+exec flow
                                overlay.dismiss();
                                let result = self.launch_tutorial_project();
                                if let SlashResult::Display(msg) = result {
                                    self.conversation.push_system(&msg);
                                }
                            } else {
                                // MyProject: advance past the choice step to the welcome
                                overlay.advance();
                            }
                            return InputDisposition::SkipLoop;
                        }
                        _ => {
                            // For Command and AnyInput steps, let keys pass through
                            // to the editor so the user can type.
                            // For Enter and AutoPrompt steps, consume the key (overlay blocks).
                            match &step_trigger {
                                tutorial::Trigger::Command(_) | tutorial::Trigger::AnyInput => {
                                    // Fall through to normal key handling
                                }
                                _ => {
                                    // Consume — overlay blocks input
                                    return InputDisposition::SkipLoop;
                                }
                            }
                        }
                    }
                }

                // ── Sidebar navigation mode ──────────────────────
                // When dashboard sidebar is active, route keys to the tree.
                // Enter on a selected node triggers design-focus via bus.
                if self.dashboard.sidebar_active {
                    if key.code == KeyCode::Enter {
                        if let Some(node_id) =
                            self.dashboard.selected_node_id().map(|s| s.to_string())
                        {
                            let _ = command_tx
                                .send(TuiCommand::BusCommand {
                                    name: "design-focus".into(),
                                    args: node_id,
                                })
                                .await;
                            self.dashboard.sidebar_active = false;
                        }
                        return InputDisposition::SkipLoop;
                    }
                    if self.dashboard.handle_key(key) {
                        return InputDisposition::SkipLoop;
                    }
                }

                match (key.code, key.modifiers) {
                    // ── Interrupt: Escape or Ctrl+C ─────────────────
                    (KeyCode::Esc, _) => {
                        // Dismiss modal if active, otherwise interrupt agent
                        if self.copy_text_modal.is_some() {
                            self.close_copy_text_modal();
                        } else if self.command_panel.is_some() {
                            self.close_command_panel_to_return_target();
                        } else if self.active_modal.is_some() {
                            self.active_modal = None;
                            if self.terminal_copy_mode {
                                self.set_terminal_copy_mode(false);
                            }
                        } else if self.active_action_prompt.is_some() {
                            self.active_action_prompt = None;
                        } else if self.agent_active {
                            let outcome = self
                                .handle_ui_action(UiAction::CancelActiveTurn, command_tx)
                                .await;
                            if matches!(outcome, UiActionOutcome::Accepted { .. }) {
                                self.show_command_toast(CommandToast::new(
                                    "Interrupt requested — waiting for turn to stop",
                                    CommandSeverity::Warning,
                                ));
                            } else {
                                self.show_command_toast(CommandToast::new(
                                    "Interrupt requested",
                                    CommandSeverity::Warning,
                                ));
                            }
                        }
                    }
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        if self.agent_active {
                            let outcome = self
                                .handle_ui_action(UiAction::CancelActiveTurn, command_tx)
                                .await;
                            if matches!(outcome, UiActionOutcome::Accepted { .. }) {
                                self.show_command_toast(CommandToast::new(
                                    "Interrupt requested (Ctrl+C) — waiting for turn to stop",
                                    CommandSeverity::Warning,
                                ));
                            } else {
                                self.show_command_toast(CommandToast::new(
                                    "Interrupt requested (Ctrl+C)",
                                    CommandSeverity::Warning,
                                ));
                            }
                        } else if !self.editor.is_empty() {
                            // Clear the line first (like a real terminal)
                            self.pending_history_preload = None;
                            self.editor.clear_line();
                            self.last_ctrl_c = None;
                        } else {
                            self.pending_history_preload = None;
                            // Empty editor — double Ctrl+C to quit
                            let now = std::time::Instant::now();
                            if let Some(last) = self.last_ctrl_c {
                                if now.duration_since(last).as_millis() < 1000 {
                                    self.should_quit = true;
                                    let _ =
                                        command_tx.send(TuiCommand::Quit { confirmed: true }).await;
                                } else {
                                    self.last_ctrl_c = Some(now);
                                    self.show_command_toast(CommandToast::new(
                                        "Press Ctrl+C again to quit",
                                        CommandSeverity::Info,
                                    ));
                                }
                            } else {
                                self.last_ctrl_c = Some(now);
                                self.show_command_toast(CommandToast::new(
                                    "Press Ctrl+C again to quit",
                                    CommandSeverity::Info,
                                ));
                            }
                        }
                    }

                    // ── Editor: word/line operations (idle only) ────
                    (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::DeleteWordBackward,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::ClearLine,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::KillToEnd,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Char('Y'), KeyModifiers::CONTROL) => {
                        self.copy_latest_assistant_response(SegmentExportMode::Plaintext);
                    }
                    (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                        self.editor.yank();
                    }
                    (KeyCode::Char('t'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)
                    | (KeyCode::Char('T'), KeyModifiers::CONTROL) => {
                        self.set_terminal_copy_mode(!self.terminal_copy_mode);
                    }
                    (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                        self.editor.move_home();
                    }
                    (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                        self.editor.move_end();
                    }
                    (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                        self.editor.start_reverse_search();
                    }

                    // Meta (Alt) key combos for word operations
                    (KeyCode::Backspace, KeyModifiers::ALT) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::DeleteWordBackward,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Char('d'), KeyModifiers::ALT) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::DeleteWordForward,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Char('b'), KeyModifiers::ALT) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Backward,
                                    unit: ComposerCursorUnit::Word,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Char('f'), KeyModifiers::ALT) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Forward,
                                    unit: ComposerCursorUnit::Word,
                                }),
                                command_tx,
                            )
                            .await;
                    }

                    // Ctrl+O: toggle the unified tool inspection target.
                    (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                        if self
                            .tool_inspection_target
                            .as_ref()
                            .is_some_and(ToolInspectionTarget::is_episode)
                        {
                            self.tool_inspection_target = None;
                        } else if let Some(id) = self.conversation.latest_expandable_tool_id() {
                            let episode_id = self
                                .conversation
                                .episode_id_for_tool(&id)
                                .unwrap_or_else(|| format!("tool:{id}"));
                            self.tool_inspection_target = Some(ToolInspectionTarget::Episode {
                                episode_id,
                                evidence_id: id,
                            });
                        }
                    }

                    // Ctrl+G: cycle UI presentation (om → active → full).
                    (KeyCode::Char('g'), KeyModifiers::CONTROL) => {
                        let next = self.ui_presentation.level.next();
                        let name = next.name();
                        self.apply_ui_presentation(UiPresentationPolicy::named(next));
                        self.show_toast(&format!("UI → {name}"), ratatui_toaster::ToastType::Info);
                    }

                    // Ctrl+D: toggle sidebar navigation mode (design tree)
                    (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                        self.dashboard.sidebar_active = !self.dashboard.sidebar_active;
                        if self.dashboard.sidebar_active
                            && self.dashboard.tree_state.selected().is_empty()
                        {
                            self.dashboard.tree_state.select_first();
                        }
                    }

                    // Tab: command completion, @-picker insertion, or inline tool-detail toggle.
                    (KeyCode::Tab, _) => {
                        let text = self.editor.render_text().to_string();
                        if let Some(ref picker) = self.at_picker {
                            let path = picker.selected_value().to_string();
                            let full = self.cwd().join(&path);
                            self.editor.set_text("");
                            self.editor.insert_attachment(full);
                            self.at_picker = None;
                        } else if text.starts_with('/') {
                            let matches = self.matching_commands();
                            if matches.len() == 1 {
                                self.editor.set_text(&matches[0].command);
                            }
                        } else if text.is_empty() {
                            if self
                                .tool_inspection_target
                                .as_ref()
                                .is_some_and(ToolInspectionTarget::is_episode)
                            {
                                self.tool_inspection_target = None;
                            } else if let Some(id) = self.conversation.latest_expandable_tool_id() {
                                let episode_id = self
                                    .conversation
                                    .episode_id_for_tool(&id)
                                    .unwrap_or_else(|| format!("tool:{id}"));
                                self.tool_inspection_target = Some(ToolInspectionTarget::Episode {
                                    episode_id,
                                    evidence_id: id,
                                });
                            }
                        }
                    }

                    // Shift+Tab: collapse the pinned tool detail row.
                    (KeyCode::BackTab, _) => {
                        self.tool_inspection_target = None;
                    }

                    // Alt+N: next conversation tab
                    (KeyCode::Char('n'), KeyModifiers::ALT)
                        if self.conversation.tabs.tabs.len() > 1 =>
                    {
                        self.conversation.tabs.next_tab();
                    }

                    // Alt+P: previous conversation tab
                    (KeyCode::Char('p'), KeyModifiers::ALT)
                        if self.conversation.tabs.tabs.len() > 1 =>
                    {
                        self.conversation.tabs.prev_tab();
                    }

                    // Enter on a selected terminal result opens the canonical process viewer.
                    (KeyCode::Enter, _)
                        if self.editor.is_empty()
                            && self.selected_terminal_session_id().is_some() =>
                    {
                        if let Some(session_id) = self.selected_terminal_session_id() {
                            self.open_process_viewer(&session_id);
                        }
                    }

                    // Shift+Enter or Alt+Enter: insert newline (multiline input)
                    (KeyCode::Enter, m)
                        if m.contains(KeyModifiers::SHIFT) || m.contains(KeyModifiers::ALT) =>
                    {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::InsertNewline,
                                }),
                                command_tx,
                            )
                            .await;
                    }

                    // Submit / @-picker confirm
                    (KeyCode::Enter, _) => {
                        if let Some(ref picker) = self.at_picker {
                            let path = picker.selected_value().to_string();
                            let full = self.cwd().join(&path);
                            self.editor.set_text("");
                            self.editor.insert_attachment(full);
                            self.at_picker = None;
                        } else {
                            self.submit_editor_buffer(command_tx).await;
                        }
                    }

                    // Basic editing — only insert if no Ctrl modifier
                    // (Ctrl+letter arms above handle those explicitly)
                    (KeyCode::Char(c), mods) if !mods.contains(KeyModifiers::CONTROL) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::InsertComposerText(InsertComposerTextAction {
                                    text: c.to_string(),
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Backspace, _) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::EditComposer(EditComposerAction {
                                    operation: ComposerEditOperation::DeleteBackward,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Left, KeyModifiers::ALT) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Backward,
                                    unit: ComposerCursorUnit::Word,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Right, KeyModifiers::ALT) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Forward,
                                    unit: ComposerCursorUnit::Word,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Left, _) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Backward,
                                    unit: ComposerCursorUnit::Character,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Right, _) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Forward,
                                    unit: ComposerCursorUnit::Character,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::Home, _) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::Home,
                                    unit: ComposerCursorUnit::Line,
                                }),
                                command_tx,
                            )
                            .await;
                    }
                    (KeyCode::End, _) => {
                        let _ = self
                            .handle_ui_action(
                                UiAction::MoveComposerCursor(MoveComposerCursorAction {
                                    direction: ComposerCursorDirection::End,
                                    unit: ComposerCursorUnit::Line,
                                }),
                                command_tx,
                            )
                            .await;
                    }

                    // ── Scrolling ────────────────────────────────
                    (KeyCode::Up, KeyModifiers::SHIFT) => {
                        self.conversation.scroll_up(3);
                    }
                    (KeyCode::Down, KeyModifiers::SHIFT) => {
                        self.conversation.scroll_down(3);
                    }
                    (KeyCode::Up, KeyModifiers::ALT) => {
                        self.history_recall_up();
                    }
                    (KeyCode::Down, KeyModifiers::ALT) => {
                        self.history_recall_down();
                    }
                    (KeyCode::PageUp, _) => {
                        self.conversation.scroll_up(20);
                    }
                    (KeyCode::PageDown, _) => {
                        self.conversation.scroll_down(20);
                    }
                    (KeyCode::Up, _) => {
                        self.handle_keyboard_up();
                    }
                    (KeyCode::Down, _) => {
                        self.handle_keyboard_down();
                    }
                    _ => {}
                }
            } // Event::Key
            _ => {} // Other events (resize, etc.)
        }
        InputDisposition::Continue
    }
}
