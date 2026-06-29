//! Reusable command UI surfaces: panels, toasts, and modal descriptors.

use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget, Wrap};

use super::theme::Theme;
use crate::surfaces::command::{CommandPanel, CommandPrompt, CommandSeverity};

pub(crate) const COMMAND_MODAL_WIDTH: u16 = 96;
pub(crate) const COMMAND_MODAL_HEIGHT: u16 = 24;
pub(crate) const COMMAND_MODAL_MARGIN: u16 = 4;

pub(crate) fn command_modal_area(area: Rect) -> Rect {
    let max_width = area.width.saturating_sub(COMMAND_MODAL_MARGIN).max(1);
    let max_height = area.height.saturating_sub(COMMAND_MODAL_MARGIN).max(1);
    let width = COMMAND_MODAL_WIDTH.min(max_width);
    let height = COMMAND_MODAL_HEIGHT.min(max_height);

    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub fn render_prompt(area: Rect, buf: &mut Buffer, theme: &dyn Theme, prompt: &CommandPrompt) {
    let actions = prompt
        .actions
        .iter()
        .map(|action| format!("[{}] {}", action.key, action.label))
        .collect::<Vec<_>>()
        .join("   ");
    let body = if actions.is_empty() {
        prompt.body.clone()
    } else {
        format!("{}\n\n{}", prompt.body, actions)
    };
    let panel = CommandPanel {
        title: prompt.title.clone(),
        body,
        source: None,
        severity: prompt.severity,
        copyable: false,
        scroll: 0,
        return_target: None,
    };
    render_panel(area, buf, theme, &panel);
}

pub fn render_panel(area: Rect, buf: &mut Buffer, theme: &dyn Theme, panel: &CommandPanel) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let panel_area = command_modal_area(area);

    Clear.render(panel_area, buf);
    let border = match panel.severity {
        CommandSeverity::Info => theme.accent(),
        CommandSeverity::Success => theme.success(),
        CommandSeverity::Warning => theme.warning(),
        CommandSeverity::Error => theme.error(),
    };
    let footer = match (panel.copyable, panel.return_target) {
        (true, Some(target)) => format!(" Esc back to {} · q close · ^Y copy ", target.label()),
        (false, Some(target)) => format!(" Esc back to {} · q close ", target.label()),
        (true, None) => " Esc close · ^Y copy ".to_string(),
        (false, None) => " Esc close ".to_string(),
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

#[cfg(test)]
mod tests {
    use super::{COMMAND_MODAL_HEIGHT, COMMAND_MODAL_WIDTH, command_modal_area};

    #[test]
    fn command_modal_area_uses_stable_centered_geometry() {
        let area = ratatui::layout::Rect::new(0, 0, 140, 40);
        let modal = command_modal_area(area);

        assert_eq!(modal.width, COMMAND_MODAL_WIDTH);
        assert_eq!(modal.height, COMMAND_MODAL_HEIGHT);
        assert_eq!(modal.x, 22);
        assert_eq!(modal.y, 8);
    }

    #[test]
    fn command_modal_area_clamps_to_small_terminals() {
        let area = ratatui::layout::Rect::new(0, 0, 50, 18);
        let modal = command_modal_area(area);

        assert_eq!(modal.width, 46);
        assert_eq!(modal.height, 14);
        assert_eq!(modal.x, 2);
        assert_eq!(modal.y, 2);
    }
}
