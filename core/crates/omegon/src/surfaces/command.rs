//! Renderer-neutral command/modal surface projections.
//!
//! These DTOs describe command panels, prompts, toasts, and modal descriptors
//! without depending on any terminal renderer. TUI, ACP, CLI, and future web
//! clients can project the same command state into their own presentation layer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSurfaceKind {
    Panel,
    Toast,
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandPanelReturnTarget {
    Menu,
}

impl CommandPanelReturnTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Menu => "menu",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPanel {
    pub title: String,
    pub body: String,
    pub source: Option<String>,
    pub severity: CommandSeverity,
    pub copyable: bool,
    pub scroll: u16,
    pub return_target: Option<CommandPanelReturnTarget>,
}

impl CommandPanel {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            source: None,
            severity: CommandSeverity::Info,
            copyable: true,
            scroll: 0,
            return_target: None,
        }
    }

    pub fn from_slash(command: impl Into<String>, body: impl Into<String>) -> Self {
        let command = command.into();
        Self {
            title: format!("command · {command}"),
            body: body.into(),
            source: Some(command),
            severity: CommandSeverity::Info,
            copyable: true,
            scroll: 0,
            return_target: None,
        }
    }

    pub fn with_return_target(mut self, target: CommandPanelReturnTarget) -> Self {
        self.return_target = Some(target);
        self
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_add(amount).min(self.max_scroll());
    }

    pub fn scroll_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_bottom(&mut self) {
        self.scroll = self.max_scroll();
    }

    fn max_scroll(&self) -> u16 {
        self.body.lines().count().saturating_sub(1) as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandToast {
    pub message: String,
    pub severity: CommandSeverity,
}

impl CommandToast {
    pub fn new(message: impl Into<String>, severity: CommandSeverity) -> Self {
        Self {
            message: message.into(),
            severity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPromptAction {
    pub key: String,
    pub label: String,
}

impl CommandPromptAction {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPrompt {
    pub title: String,
    pub body: String,
    pub actions: Vec<CommandPromptAction>,
    pub severity: CommandSeverity,
}

impl CommandPrompt {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
            severity: CommandSeverity::Warning,
        }
    }

    pub fn with_actions(mut self, actions: Vec<CommandPromptAction>) -> Self {
        self.actions = actions;
        self
    }

    pub fn with_severity(mut self, severity: CommandSeverity) -> Self {
        self.severity = severity;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandModal {
    pub title: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_panel_preserves_command_source_and_body() {
        let panel = CommandPanel::from_slash("/status", "runtime ok");

        assert_eq!(panel.title, "command · /status");
        assert_eq!(panel.source.as_deref(), Some("/status"));
        assert_eq!(panel.body, "runtime ok");
        assert!(panel.copyable);
        assert_eq!(panel.scroll, 0);
        assert_eq!(panel.return_target, None);
    }

    #[test]
    fn panel_return_target_marks_parent_surface() {
        let panel = CommandPanel::from_slash("/skills get rust", "details")
            .with_return_target(CommandPanelReturnTarget::Menu);

        assert_eq!(panel.return_target, Some(CommandPanelReturnTarget::Menu));
        assert_eq!(panel.return_target.unwrap().label(), "menu");
        assert_eq!(panel.source.as_deref(), Some("/skills get rust"));
    }

    #[test]
    fn panel_scroll_saturates_at_top_and_bottom() {
        let mut panel = CommandPanel::new("long", "one\ntwo\nthree");

        panel.scroll_down(99);
        assert_eq!(panel.scroll, 2);

        panel.scroll_up(1);
        assert_eq!(panel.scroll, 1);

        panel.scroll_up(99);
        assert_eq!(panel.scroll, 0);
    }

    #[test]
    fn panel_scroll_jumps_to_top_and_bottom() {
        let mut panel = CommandPanel::new("long", "one\ntwo\nthree\nfour");

        panel.scroll_bottom();
        assert_eq!(panel.scroll, 3);

        panel.scroll_top();
        assert_eq!(panel.scroll, 0);
    }

    #[test]
    fn toast_constructor_sets_message_and_severity() {
        let toast = CommandToast::new("saved", CommandSeverity::Success);

        assert_eq!(toast.message, "saved");
        assert_eq!(toast.severity, CommandSeverity::Success);
    }

    #[test]
    fn prompt_builder_sets_actions_and_severity() {
        let prompt = CommandPrompt::new("Permission", "Allow read?")
            .with_actions(vec![CommandPromptAction::new("y", "allow")])
            .with_severity(CommandSeverity::Error);

        assert_eq!(prompt.title, "Permission");
        assert_eq!(prompt.body, "Allow read?");
        assert_eq!(prompt.actions[0].key, "y");
        assert_eq!(prompt.actions[0].label, "allow");
        assert_eq!(prompt.severity, CommandSeverity::Error);
    }
}
