//! Renderer-local effects derived from semantic menu actions.
//!
//! This is the boundary between renderer-neutral `MenuActionProjection` values
//! and mutations performed by the native TUI `App` adapter.

use crate::surfaces::menu::{MenuActionClosePolicy, MenuActionDisposition, MenuActionProjection};

use super::slash_commands::SlashResult;
use super::{
    App, CommandPanel, CommandPanelReturnTarget, CommandSeverity, CommandToast, MenuInput,
    OperatorCommandTx,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MenuRefreshTarget {
    Ui,
    ExtensionRuntime,
    ExtensionDetail(String),
    Unsupported(String),
}

impl MenuRefreshTarget {
    fn from_menu_id(menu_id: String) -> Self {
        match menu_id.as_str() {
            "ui" => Self::Ui,
            "extension-runtime" => Self::ExtensionRuntime,
            _ if menu_id.starts_with("extension-detail:") => {
                Self::ExtensionDetail(menu_id.trim_start_matches("extension-detail:").to_string())
            }
            _ => Self::Unsupported(menu_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelectorTarget {
    ContextClass,
    CurrentModel,
    ModelGrade,
    ModelProvider,
    ModelPolicy,
    SecretName,
    ConnectionProviders,
    FreeModels,
    LocalModels,
    ConnectionModels(String),
    Unknown(Option<String>),
}

impl SelectorTarget {
    fn from_id(selector_id: Option<String>) -> Self {
        match selector_id.as_deref() {
            Some("context.class") => Self::ContextClass,
            Some("model.current") => Self::CurrentModel,
            Some("model.grade") => Self::ModelGrade,
            Some("model.provider") => Self::ModelProvider,
            Some("model.policy") => Self::ModelPolicy,
            Some("secrets.name") => Self::SecretName,
            Some("auth.providers") => Self::ConnectionProviders,
            Some("auth.free") => Self::FreeModels,
            Some("auth.local") => Self::LocalModels,
            Some(id) if id.starts_with("auth.models.") => {
                Self::ConnectionModels(id.trim_start_matches("auth.models.").into())
            }
            _ => Self::Unknown(selector_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsRowAction {
    OpenModelSelector,
    OpenMaxTurnsSelector,
    ToggleSandbox,
    ToggleAutoUpdate,
    ExplainTrustedDirectories,
    ProjectedEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SettingsRowTarget {
    id: Option<String>,
}

impl SettingsRowTarget {
    fn from_id(id: Option<String>) -> Self {
        Self { id }
    }

    pub(super) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub(super) fn action(&self) -> SettingsRowAction {
        match self.id() {
            Some("runtime.model") => SettingsRowAction::OpenModelSelector,
            Some("runtime.max_turns") => SettingsRowAction::OpenMaxTurnsSelector,
            Some("workspace.sandbox") => SettingsRowAction::ToggleSandbox,
            Some("updates.auto_update") => SettingsRowAction::ToggleAutoUpdate,
            Some("workspace.trusted_directories") => SettingsRowAction::ExplainTrustedDirectories,
            _ => SettingsRowAction::ProjectedEditor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MenuCommandPresentation {
    None,
    Toast { message: String },
    CommandPanel { response: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MenuCommandOutcome {
    pub(super) result: SlashResult,
    pub(super) record_history: bool,
    pub(super) close_menu: bool,
    pub(super) request_quit: bool,
    pub(super) presentation: MenuCommandPresentation,
}

impl MenuCommandOutcome {
    pub(super) fn from_slash_result(result: SlashResult, secret_input: bool) -> Self {
        match result {
            SlashResult::Display(response) if secret_input => Self {
                result: SlashResult::Handled,
                record_history: true,
                close_menu: true,
                request_quit: false,
                presentation: MenuCommandPresentation::Toast { message: response },
            },
            SlashResult::Display(response) => Self {
                result: SlashResult::Handled,
                record_history: true,
                close_menu: false,
                request_quit: false,
                presentation: MenuCommandPresentation::CommandPanel { response },
            },
            SlashResult::Handled => Self {
                result: SlashResult::Handled,
                record_history: true,
                close_menu: true,
                request_quit: false,
                presentation: MenuCommandPresentation::None,
            },
            SlashResult::Quit => Self {
                result: SlashResult::Quit,
                record_history: true,
                close_menu: true,
                request_quit: true,
                presentation: MenuCommandPresentation::None,
            },
            SlashResult::NotACommand => Self {
                result: SlashResult::Handled,
                record_history: false,
                close_menu: false,
                request_quit: false,
                presentation: MenuCommandPresentation::None,
            },
        }
    }

    pub(super) fn should_refresh_menu(&self, close_policy: MenuActionClosePolicy) -> bool {
        self.result == SlashResult::Handled && close_policy == MenuActionClosePolicy::RefreshMenu
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MenuEffect {
    FocusRow {
        target_row_id: Option<String>,
    },
    PrimeEditor {
        text: Option<String>,
        message: Option<String>,
    },
    BeginInlineInput {
        label: String,
        command_prefix: Option<String>,
    },
    OpenSelector {
        target: SelectorTarget,
        label: String,
    },
    OpenExtensionDetail {
        target_row_id: Option<String>,
    },
    OpenSettingsRow {
        target: SettingsRowTarget,
        label: String,
    },
    RunCommand {
        command: Option<String>,
        close_policy: MenuActionClosePolicy,
    },
}

impl From<MenuActionProjection> for MenuEffect {
    fn from(action: MenuActionProjection) -> Self {
        match action.disposition {
            MenuActionDisposition::FocusRow => Self::FocusRow {
                target_row_id: action.target_row_id,
            },
            MenuActionDisposition::PrimeEditor => Self::PrimeEditor {
                text: action.editor_text,
                message: action.message,
            },
            MenuActionDisposition::InlineInput => Self::BeginInlineInput {
                label: action.label,
                command_prefix: action.editor_text,
            },
            MenuActionDisposition::OpenSelector => Self::OpenSelector {
                target: SelectorTarget::from_id(action.target_row_id),
                label: action.label,
            },
            MenuActionDisposition::OpenExtensionDetail => Self::OpenExtensionDetail {
                target_row_id: action.target_row_id,
            },
            MenuActionDisposition::OpenSettingsRow => Self::OpenSettingsRow {
                target: SettingsRowTarget::from_id(action.target_row_id),
                label: action.label,
            },
            MenuActionDisposition::RunCommand => Self::RunCommand {
                command: action.command,
                close_policy: action.close_policy,
            },
        }
    }
}

impl App {
    pub(super) fn execute_active_menu_action(
        &mut self,
        action: crate::surfaces::menu::MenuActionProjection,
        tx: &OperatorCommandTx,
    ) -> SlashResult {
        if action.requires_confirmation {
            if self.pending_menu_confirmation.as_deref() != Some(action.id.as_str()) {
                self.pending_menu_confirmation = Some(action.id.clone());
                self.show_command_toast(CommandToast::new(
                    format!("Press Enter/shortcut again to confirm {}", action.label),
                    CommandSeverity::Warning,
                ));
                return SlashResult::Handled;
            }
            self.pending_menu_confirmation = None;
        } else {
            self.pending_menu_confirmation = None;
        }
        match MenuEffect::from(action) {
            MenuEffect::FocusRow { target_row_id } => {
                if let Some(target_row_id) = target_row_id
                    && let Some(menu) = self.active_menu.as_mut()
                {
                    menu.state
                        .select_row_by_id(&menu.projection, &target_row_id);
                }
                SlashResult::Handled
            }
            MenuEffect::PrimeEditor { text, message } => {
                self.active_menu = None;
                if let Some(text) = text {
                    self.editor.set_text(&text);
                }
                if let Some(message) = message {
                    self.show_command_toast(CommandToast::new(message, CommandSeverity::Info));
                }
                SlashResult::Handled
            }
            MenuEffect::BeginInlineInput {
                label,
                command_prefix,
            } => {
                if let Some(command_prefix) = command_prefix {
                    let original_footer = self
                        .active_menu
                        .as_ref()
                        .and_then(|menu| menu.projection.footer.clone());
                    self.menu_input = Some(MenuInput {
                        action_label: label,
                        command_prefix,
                        value: String::new(),
                        original_footer,
                    });
                    if let Some(menu) = self.active_menu.as_mut() {
                        menu.projection.footer =
                            Some("Type value · Enter execute · Esc cancel".into());
                    }
                }
                SlashResult::Handled
            }
            MenuEffect::OpenSelector { target, label } => {
                self.active_menu = None;
                self.pending_menu_confirmation = None;
                match target {
                    SelectorTarget::ContextClass => self.open_context_selector(),
                    SelectorTarget::CurrentModel => self.open_model_selector(),
                    SelectorTarget::ModelGrade => self.open_model_grade_selector(),
                    SelectorTarget::ModelProvider => self.open_model_provider_selector(),
                    SelectorTarget::ModelPolicy => self.open_model_policy_selector(),
                    SelectorTarget::SecretName => self.open_secret_name_selector(),
                    SelectorTarget::ConnectionProviders => self.open_auth_provider_menu(),
                    SelectorTarget::FreeModels => self.open_free_model_menu(),
                    SelectorTarget::LocalModels => self.open_local_model_menu(),
                    SelectorTarget::ConnectionModels(provider) => {
                        self.open_model_provider_inventory_selector(&provider)
                    }
                    SelectorTarget::Unknown(selector_id) => {
                        self.show_command_toast(CommandToast::new(
                            format!(
                                "No selector registered for {label}{}",
                                selector_id
                                    .as_deref()
                                    .map(|id| format!(" ({id})"))
                                    .unwrap_or_default()
                            ),
                            CommandSeverity::Warning,
                        ))
                    }
                }
                SlashResult::Handled
            }
            MenuEffect::OpenExtensionDetail { target_row_id } => {
                if let Some(extension_name) = target_row_id.as_deref() {
                    self.open_extension_detail_menu(extension_name);
                } else {
                    self.show_command_toast(CommandToast::new(
                        "Extension detail target is unavailable",
                        CommandSeverity::Warning,
                    ));
                }
                SlashResult::Handled
            }
            MenuEffect::OpenSettingsRow { target, label } => {
                if target.id().is_some() {
                    self.open_settings_row(target);
                } else {
                    self.show_command_toast(CommandToast::new(
                        format!("No settings row registered for {label}"),
                        CommandSeverity::Warning,
                    ));
                }
                SlashResult::Handled
            }
            MenuEffect::RunCommand {
                command,
                close_policy,
            } => {
                if let Some(command) = command {
                    let refresh_target = self
                        .active_menu
                        .as_ref()
                        .map(|menu| MenuRefreshTarget::from_menu_id(menu.projection.id.clone()));
                    let outcome = self.execute_active_menu_command(command, tx);
                    if outcome.should_refresh_menu(close_policy)
                        && let Some(refresh_target) = refresh_target
                        && self.rebuild_active_menu(refresh_target)
                    {
                        return SlashResult::Handled;
                    }
                    outcome.result
                } else {
                    SlashResult::Handled
                }
            }
        }
    }
}

impl App {
    pub(super) fn apply_menu_command_outcome(
        &mut self,
        command: &str,
        outcome: &MenuCommandOutcome,
    ) {
        if outcome.record_history {
            self.history.push(command.to_string());
            self.exit_history_recall();
        }
        if outcome.close_menu {
            self.active_menu = None;
        }
        if outcome.request_quit {
            self.should_quit = true;
        }
        match &outcome.presentation {
            MenuCommandPresentation::None => {}
            MenuCommandPresentation::Toast { message } => {
                self.show_command_toast(CommandToast::new(message, CommandSeverity::Info));
            }
            MenuCommandPresentation::CommandPanel { response } => {
                self.open_command_panel(
                    CommandPanel::from_slash(command, response.clone())
                        .with_return_target(CommandPanelReturnTarget::Menu),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::menu::MenuActionProjection;

    #[test]
    fn prime_editor_projection_becomes_explicit_tui_effect() {
        let action = MenuActionProjection::prime_editor(
            "profile.save",
            "Save profile",
            "/profile save --name ",
            "Type a profile name",
        );

        assert_eq!(
            MenuEffect::from(action),
            MenuEffect::PrimeEditor {
                text: Some("/profile save --name ".into()),
                message: Some("Type a profile name".into()),
            }
        );
    }

    #[test]
    fn command_effect_preserves_refresh_policy() {
        let mut action = MenuActionProjection::command("ui.toggle", "Toggle", "/ui dashboard off");
        action.close_policy = MenuActionClosePolicy::RefreshMenu;

        assert_eq!(
            MenuEffect::from(action),
            MenuEffect::RunCommand {
                command: Some("/ui dashboard off".into()),
                close_policy: MenuActionClosePolicy::RefreshMenu,
            }
        );
    }

    #[test]
    fn selector_effect_preserves_target_and_operator_label() {
        let action = MenuActionProjection::open_selector(
            "model.provider.open",
            "Choose provider",
            "model.provider",
        );

        assert_eq!(
            MenuEffect::from(action),
            MenuEffect::OpenSelector {
                target: SelectorTarget::ModelProvider,
                label: "Choose provider".into(),
            }
        );
    }

    #[test]
    fn unknown_selector_target_remains_diagnostic() {
        let action = MenuActionProjection::open_selector(
            "future.selector.open",
            "Choose future value",
            "future.selector",
        );

        assert_eq!(
            MenuEffect::from(action),
            MenuEffect::OpenSelector {
                target: SelectorTarget::Unknown(Some("future.selector".into())),
                label: "Choose future value".into(),
            }
        );
    }

    #[test]
    fn settings_row_effect_types_local_actions() {
        let action = MenuActionProjection::open_settings_row(
            "settings.sandbox.open",
            "Sandbox",
            "workspace.sandbox",
        );

        let MenuEffect::OpenSettingsRow { target, label } = MenuEffect::from(action) else {
            panic!("expected settings row effect");
        };
        assert_eq!(target.id(), Some("workspace.sandbox"));
        assert_eq!(target.action(), SettingsRowAction::ToggleSandbox);
        assert_eq!(label, "Sandbox");
    }

    #[test]
    fn settings_row_effect_preserves_projected_editor_fallback() {
        let action = MenuActionProjection::open_settings_row(
            "settings.future.open",
            "Future setting",
            "future.setting",
        );

        let MenuEffect::OpenSettingsRow { target, .. } = MenuEffect::from(action) else {
            panic!("expected settings row effect");
        };
        assert_eq!(target.id(), Some("future.setting"));
        assert_eq!(target.action(), SettingsRowAction::ProjectedEditor);
    }

    #[test]
    fn refresh_target_types_supported_menu_ids() {
        assert_eq!(
            MenuRefreshTarget::from_menu_id("ui".into()),
            MenuRefreshTarget::Ui
        );
        assert_eq!(
            MenuRefreshTarget::from_menu_id("extension-runtime".into()),
            MenuRefreshTarget::ExtensionRuntime
        );
        assert_eq!(
            MenuRefreshTarget::from_menu_id("extension-detail:demo".into()),
            MenuRefreshTarget::ExtensionDetail("demo".into())
        );
        assert_eq!(
            MenuRefreshTarget::from_menu_id("settings".into()),
            MenuRefreshTarget::Unsupported("settings".into())
        );
    }

    #[test]
    fn display_outcome_keeps_menu_return_path() {
        let outcome =
            MenuCommandOutcome::from_slash_result(SlashResult::Display("status".into()), false);

        assert_eq!(outcome.result, SlashResult::Handled);
        assert!(outcome.record_history);
        assert!(!outcome.close_menu);
        assert_eq!(
            outcome.presentation,
            MenuCommandPresentation::CommandPanel {
                response: "status".into()
            }
        );
    }

    #[test]
    fn secret_display_outcome_closes_menu_and_uses_toast() {
        let outcome = MenuCommandOutcome::from_slash_result(
            SlashResult::Display("secret stored".into()),
            true,
        );

        assert!(outcome.close_menu);
        assert_eq!(
            outcome.presentation,
            MenuCommandPresentation::Toast {
                message: "secret stored".into()
            }
        );
    }

    #[test]
    fn handled_outcome_can_request_menu_refresh() {
        let outcome = MenuCommandOutcome::from_slash_result(SlashResult::Handled, false);

        assert!(outcome.should_refresh_menu(MenuActionClosePolicy::RefreshMenu));
        assert!(!outcome.should_refresh_menu(MenuActionClosePolicy::CloseMenu));
    }

    #[test]
    fn applying_quit_outcome_records_history_closes_menu_and_requests_quit() {
        let mut app = super::super::tests::test_app();
        app.open_settings_menu();
        app.history_idx = Some(0);
        app.history_draft = Some("draft".into());
        let outcome = MenuCommandOutcome::from_slash_result(SlashResult::Quit, false);

        app.apply_menu_command_outcome("/quit", &outcome);

        assert_eq!(app.history.last().map(String::as_str), Some("/quit"));
        assert!(app.history_idx.is_none());
        assert!(app.history_draft.is_none());
        assert!(app.active_menu.is_none());
        assert!(app.should_quit);
    }

    #[test]
    fn applying_display_outcome_preserves_menu_and_opens_returnable_panel() {
        let mut app = super::super::tests::test_app();
        app.open_settings_menu();
        let outcome =
            MenuCommandOutcome::from_slash_result(SlashResult::Display("status".into()), false);

        app.apply_menu_command_outcome("/status", &outcome);

        assert!(app.active_menu.is_some());
        assert!(app.command_panel.is_some());
        assert_eq!(app.history.last().map(String::as_str), Some("/status"));
    }

    #[test]
    fn applying_prime_editor_effect_closes_menu_and_sets_text() {
        let mut app = super::super::tests::test_app();
        app.open_settings_menu();
        let action = MenuActionProjection::prime_editor(
            "profile.save",
            "Save profile",
            "/settings profile save ",
            "Enter a profile name",
        );

        app.execute_active_menu_action(action, &super::super::tests::test_tx());

        assert!(app.active_menu.is_none());
        assert_eq!(app.editor.render_text(), "/settings profile save ");
    }

    #[test]
    fn applying_inline_input_effect_preserves_menu_and_starts_capture() {
        let mut app = super::super::tests::test_app();
        app.open_settings_menu();
        let action = MenuActionProjection::inline_input(
            "extension.search",
            "Search extensions",
            "/extension search ",
        );

        app.execute_active_menu_action(action, &super::super::tests::test_tx());

        assert!(app.active_menu.is_some());
        let input = app.menu_input.as_ref().expect("inline input");
        assert_eq!(input.action_label, "Search extensions");
        assert_eq!(input.command_prefix, "/extension search ");
    }

    #[test]
    fn applying_unknown_selector_effect_closes_menu_and_warns() {
        let mut app = super::super::tests::test_app();
        app.open_settings_menu();
        let action = MenuActionProjection::open_selector(
            "future.selector.open",
            "Choose future value",
            "future.selector",
        );

        app.execute_active_menu_action(action, &super::super::tests::test_tx());

        assert!(app.active_menu.is_none());
        assert!(app.operator_events.iter().any(|event| {
            event.message.contains("future.selector")
                && event.message.contains("No selector registered")
        }));
    }
}
