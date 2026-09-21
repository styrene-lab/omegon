//! Reusable command UI surfaces: panels, toasts, and modal descriptors.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget, Wrap};

use super::theme::Theme;
use crate::surfaces::command::{CommandPanel, CommandPrompt, CommandSeverity};

pub(crate) const COMMAND_MODAL_WIDTH: u16 = 120;
pub(crate) const COMMAND_MODAL_HEIGHT: u16 = 32;
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

fn prompt_action_lines(prompt: &CommandPrompt, width: usize) -> Vec<String> {
    prompt
        .actions
        .iter()
        .flat_map(|action| {
            super::menu_surface::wrap_display(&format!("[{}] {}", action.key, action.label), width)
        })
        .collect()
}

fn prompt_body_lines(prompt: &CommandPrompt, width: usize) -> Vec<String> {
    prompt
        .body
        .lines()
        .flat_map(|line| super::menu_surface::wrap_display(line, width))
        .collect()
}

pub(crate) fn prompt_modal_area(area: Rect, prompt: &CommandPrompt) -> Rect {
    use unicode_width::UnicodeWidthStr;
    let content_width = prompt
        .body
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let actions_width = prompt
        .actions
        .iter()
        .map(|a| {
            UnicodeWidthStr::width(a.key.as_str()) + UnicodeWidthStr::width(a.label.as_str()) + 3
        })
        .max()
        .unwrap_or(0);
    let max_width = area.width.saturating_sub(COMMAND_MODAL_MARGIN).max(1);
    let max_height = area.height.saturating_sub(COMMAND_MODAL_MARGIN).max(1);
    let width = content_width
        .max(actions_width)
        .saturating_add(4)
        .min(usize::from(COMMAND_MODAL_WIDTH.min(max_width))) as u16;
    let width = width.max(48.min(max_width));
    let inner_width = usize::from(width.saturating_sub(2));
    let desired_height = prompt_body_lines(prompt, inner_width).len()
        + prompt_action_lines(prompt, inner_width).len()
        + 3;
    let height = desired_height.min(usize::from(max_height)) as u16;
    let height = height.max(6.min(max_height));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub fn render_prompt(area: Rect, buf: &mut Buffer, theme: &dyn Theme, prompt: &CommandPrompt) {
    let panel_area = prompt_modal_area(area, prompt);
    if panel_area.width < 4 || panel_area.height < 3 {
        return;
    }
    Clear.render(panel_area, buf);
    let border = match prompt.severity {
        CommandSeverity::Info => theme.style_ui_border().fg.unwrap_or(theme.border()),
        CommandSeverity::Success => theme.success(),
        CommandSeverity::Warning => theme.warning(),
        CommandSeverity::Error => theme.error(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" {} ", prompt.title),
            theme.style_ui_title(),
        ))
        .style(theme.style_panel());
    let inner = block.inner(panel_area);
    block.render(panel_area, buf);
    let width = usize::from(inner.width);
    let actions = prompt_action_lines(prompt, width);
    let body_budget = usize::from(inner.height).saturating_sub(actions.len() + 1);
    let mut body = prompt_body_lines(prompt, width);
    if body.len() > body_budget {
        body.truncate(body_budget);
        if let Some(last) = body.last_mut() {
            *last = "… context truncated".into();
        }
    }
    let mut lines: Vec<Line> = body.into_iter().map(Line::from).collect();
    if !actions.is_empty() {
        lines.push(Line::from(""));
    }
    lines.extend(
        actions
            .into_iter()
            .map(|text| Line::styled(text, theme.style_ui_title())),
    );
    Paragraph::new(lines)
        .style(theme.style_panel())
        .render(inner, buf);
}

pub fn render_panel(area: Rect, buf: &mut Buffer, theme: &dyn Theme, panel: &CommandPanel) {
    if area.width < 20 || area.height < 6 {
        return;
    }

    let panel_area = command_modal_area(area);
    // `source` carries the full command line, including arguments — `/usage
    // refresh` and `/limits -r` are the documented refresh forms — so dispatch
    // on the command token, not the whole string.
    let command = panel
        .source
        .as_deref()
        .and_then(|source| source.split_whitespace().next());
    if matches!(command, Some("/usage" | "/limits")) {
        render_usage_panel_in(panel_area, buf, theme, panel);
    } else {
        render_panel_in(panel_area, buf, theme, panel);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UsageSection {
    title: String,
    metrics: Vec<(String, String)>,
}

fn usage_sections(body: &str) -> Vec<UsageSection> {
    let mut sections = Vec::<UsageSection>::new();
    for line in body.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if matches!(line, "Usage" | "Limits") {
            continue;
        }
        if let Some(metric) = line.strip_prefix("- ") {
            let (label, value) = metric
                .split_once(':')
                .map(|(label, value)| (label.trim(), value.trim()))
                .unwrap_or((metric, ""));
            if sections.is_empty() {
                sections.push(UsageSection {
                    title: "Summary".into(),
                    metrics: Vec::new(),
                });
            }
            sections
                .last_mut()
                .unwrap()
                .metrics
                .push((label.into(), value.into()));
        } else {
            sections.push(UsageSection {
                title: line.into(),
                metrics: Vec::new(),
            });
        }
    }
    sections
}

fn render_usage_panel_in(
    panel_area: Rect,
    buf: &mut Buffer,
    theme: &dyn Theme,
    panel: &CommandPanel,
) {
    if panel_area.width < 20 || panel_area.height < 6 {
        return;
    }
    Clear.render(panel_area, buf);
    let title = if panel
        .source
        .as_deref()
        .and_then(|source| source.split_whitespace().next())
        == Some("/usage")
    {
        "Usage telemetry"
    } else {
        "Limits"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.style_ui_border())
        .title(Span::styled(format!(" {title} "), theme.style_ui_title()))
        .title_bottom(Span::styled(
            " Esc close · ↑↓ scroll · ^Y copy ",
            theme.style_ui_hint(),
        ))
        .style(theme.style_panel());
    let inner = block.inner(panel_area);
    block.render(panel_area, buf);

    let sections = usage_sections(&panel.body);
    let mut lines = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            section.title.to_uppercase(),
            theme.style_ui_title(),
        )));
        for (label, value) in &section.metrics {
            lines.push(Line::from(vec![
                Span::styled(format!("  {label:<22}"), theme.style_ui_hint()),
                Span::styled(value, theme.style_ui_text()),
            ]));
        }
    }
    Paragraph::new(lines)
        .style(theme.style_panel())
        .wrap(Wrap { trim: false })
        .scroll((panel.scroll, 0))
        .render(inner, buf);
}

fn render_panel_in(panel_area: Rect, buf: &mut Buffer, theme: &dyn Theme, panel: &CommandPanel) {
    if panel_area.width < 20 || panel_area.height < 6 {
        return;
    }

    Clear.render(panel_area, buf);
    let border = match panel.severity {
        CommandSeverity::Info => theme.style_ui_border().fg.unwrap_or(theme.border()),
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
            theme.style_ui_title(),
        ))
        .title_bottom(Span::styled(footer, theme.style_ui_hint()))
        .style(theme.style_panel());

    let lines = panel
        .body
        .lines()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    Paragraph::new(lines)
        .block(block)
        .style(theme.style_panel())
        .wrap(Wrap { trim: false })
        .scroll((panel.scroll, 0))
        .render(panel_area, buf);
}

#[cfg(test)]
mod tests {
    use super::{
        COMMAND_MODAL_HEIGHT, COMMAND_MODAL_WIDTH, CommandPrompt, command_modal_area,
        prompt_modal_area,
    };

    #[test]
    fn command_modal_area_uses_stable_centered_geometry() {
        let area = ratatui::layout::Rect::new(0, 0, 140, 40);
        let modal = command_modal_area(area);

        assert_eq!(modal.width, COMMAND_MODAL_WIDTH);
        assert_eq!(modal.height, COMMAND_MODAL_HEIGHT);
        assert_eq!(modal.x, 10);
        assert_eq!(modal.y, 4);
    }

    #[test]
    fn prompt_modal_area_fits_content_instead_of_filling_the_screen() {
        let area = ratatui::layout::Rect::new(0, 0, 180, 50);
        let prompt = CommandPrompt::new(
            "Permission required",
            "Tool: read\nTarget: /tmp/project/Cargo.toml\nGrant: /tmp/project",
        )
        .with_actions(vec![
            crate::surfaces::command::CommandPromptAction::new("y", "this operation"),
            crate::surfaces::command::CommandPromptAction::new("a", "this directory · session"),
            crate::surfaces::command::CommandPromptAction::new(
                "Shift+A",
                "this directory · project",
            ),
            crate::surfaces::command::CommandPromptAction::new("n", "deny"),
        ]);

        let modal = prompt_modal_area(area, &prompt);

        assert!(modal.width < COMMAND_MODAL_WIDTH);
        assert_eq!(modal.height, 10);
        assert_eq!(modal.x, (area.width - modal.width) / 2);
        assert_eq!(modal.y, (area.height - modal.height) / 2);
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
