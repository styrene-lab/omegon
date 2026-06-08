//! Conversation tab bar rendering.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::conversation::Tab;
use super::theme::Theme;

pub fn render_tab_bar(
    frame: &mut Frame,
    area: Rect,
    theme: &dyn Theme,
    tabs: &[Tab],
    active_tab: usize,
) {
    frame.render_widget(
        Paragraph::new(Line::from(""))
            .style(Style::default().bg(theme.surface_bg()).fg(theme.fg())),
        area,
    );

    let mut line_spans = vec![];
    for (idx, tab) in tabs.iter().enumerate() {
        if idx > 0 {
            line_spans.push(Span::raw(" "));
        }

        let label = tab.label();
        if idx == active_tab {
            line_spans.push(Span::styled(
                format!(" {label} "),
                Style::default().bg(Color::Cyan).fg(Color::Black),
            ));
        } else {
            line_spans.push(Span::styled(
                format!(" {label} "),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(line_spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_labels_are_available_for_rendering() {
        let tabs = vec![
            Tab::Conversation,
            Tab::Extension {
                widget_id: "w".into(),
                label: "Widget".into(),
            },
        ];
        let labels: Vec<_> = tabs.iter().map(Tab::label).collect();
        assert_eq!(labels, vec!["Conversation", "Widget"]);
    }
}
