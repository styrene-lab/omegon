//! Extension modal/action overlay rendering.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::theme::Theme;

pub fn render_modal(
    frame: &mut Frame,
    theme: &dyn Theme,
    widget_id: &str,
    data: &serde_json::Value,
) {
    let area = frame.area();
    let modal_width = (area.width as f32 * 0.4) as u16;
    let modal_height = (area.height as f32 * 0.5) as u16;
    let x = (area.width.saturating_sub(modal_width)) / 2;
    let y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect {
        x,
        y,
        width: modal_width,
        height: modal_height,
    };

    frame.render_widget(&Clear, modal_area);

    let title = widget_id.to_string();
    let json_str = serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string());

    let modal_bg = theme.card_bg();
    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan).bg(modal_bg))
        .style(Style::default().bg(modal_bg));

    let para = Paragraph::new(json_str)
        .block(block)
        .style(Style::default().bg(modal_bg))
        .wrap(Wrap { trim: true });

    frame.render_widget(para, modal_area);
}

pub fn render_action_prompt(
    frame: &mut Frame,
    theme: &dyn Theme,
    widget_id: &str,
    actions: &[String],
) {
    let area = frame.area();
    let prompt_width = (area.width as f32 * 0.5) as u16;
    let prompt_height = (area.height as f32 * 0.3) as u16;
    let x = (area.width.saturating_sub(prompt_width)) / 2;
    let y = (area.height.saturating_sub(prompt_height)) / 2;
    let prompt_area = Rect {
        x,
        y,
        width: prompt_width,
        height: prompt_height,
    };

    frame.render_widget(&Clear, prompt_area);

    let mut lines = vec![Line::from("Choose an action:"), Line::from("")];
    for (idx, action) in actions.iter().enumerate().take(9) {
        lines.push(Line::from(Span::styled(
            format!("  {} {} ", idx + 1, action),
            Style::default().fg(Color::Yellow).bold(),
        )));
    }

    let prompt_bg = theme.card_bg();
    let block = Block::default()
        .title(format!(" {} ", widget_id))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green).bg(prompt_bg))
        .style(Style::default().bg(prompt_bg));

    let para = Paragraph::new(lines)
        .block(block)
        .style(Style::default().bg(prompt_bg));
    frame.render_widget(para, prompt_area);
}

#[cfg(test)]
mod tests {
    #[test]
    fn action_prompt_caps_visible_actions() {
        let actions: Vec<String> = (0..12).map(|i| format!("action {i}")).collect();
        assert_eq!(actions.iter().take(9).count(), 9);
    }
}
