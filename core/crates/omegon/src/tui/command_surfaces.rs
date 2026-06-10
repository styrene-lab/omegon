//! Reusable command UI surfaces: panels, toasts, and modal descriptors.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget, Wrap};

use super::theme::Theme;

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

#[derive(Debug, Clone)]
pub struct CommandPanel {
    pub title: String,
    pub body: String,
    pub source: Option<String>,
    pub severity: CommandSeverity,
    pub copyable: bool,
    pub scroll: u16,
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
        }
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

#[cfg(test)]
mod tests {
    use super::CommandPanel;

    #[test]
    fn slash_panel_preserves_command_source_and_body() {
        let panel = CommandPanel::from_slash("/status", "runtime ok");

        assert_eq!(panel.title, "command · /status");
        assert_eq!(panel.source.as_deref(), Some("/status"));
        assert_eq!(panel.body, "runtime ok");
        assert!(panel.copyable);
        assert_eq!(panel.scroll, 0);
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
        let toast = super::CommandToast::new("saved", super::CommandSeverity::Success);

        assert_eq!(toast.message, "saved");
        assert_eq!(toast.severity, super::CommandSeverity::Success);
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct CommandModal {
    pub title: String,
    pub body: String,
}

pub fn render_panel(area: Rect, buf: &mut Buffer, theme: &dyn Theme, panel: &CommandPanel) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let width = area
        .width
        .saturating_mul(4)
        .saturating_div(5)
        .clamp(20, 100);
    let height = area.height.saturating_mul(3).saturating_div(4).clamp(6, 28);
    let panel_area = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };

    Clear.render(panel_area, buf);
    let border = match panel.severity {
        CommandSeverity::Info => theme.accent(),
        CommandSeverity::Success => theme.success(),
        CommandSeverity::Warning => theme.warning(),
        CommandSeverity::Error => theme.error(),
    };
    let footer = if panel.copyable {
        " Esc close · ^Y copy "
    } else {
        " Esc close "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" {} ", panel.title),
            Style::default()
                .fg(theme.accent_bright())
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(footer, Style::default().fg(theme.dim())))
        .style(Style::default().bg(theme.card_bg()));

    let lines = panel
        .body
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
        .block(block)
        .style(Style::default().fg(theme.fg()).bg(theme.card_bg()))
        .wrap(Wrap { trim: false })
        .scroll((panel.scroll, 0))
        .render(panel_area, buf);
}
