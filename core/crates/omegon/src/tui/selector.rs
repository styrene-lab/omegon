//! Interactive selector — arrow-key navigable list for slash command options.
//!
//! Used by /model, /think, etc. Shows a bordered popup with highlighted
//! current selection. Enter confirms, Escape cancels.

use ratatui::prelude::*;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme::Theme;

/// A selectable option with a label and optional description.
#[derive(Clone)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub description: String,
    pub active: bool, // currently active/selected setting
}

/// State for an active selector popup.
pub struct Selector {
    pub title: String,
    pub options: Vec<SelectOption>,
    pub cursor: usize,
    pub visible: bool,
}

fn selector_visible_window(
    cursor: usize,
    len: usize,
    inner_height: usize,
) -> std::ops::Range<usize> {
    if len == 0 || inner_height == 0 {
        return 0..0;
    }

    let needs_overflow_hint = len.saturating_mul(2) > inner_height;
    let option_lines = if needs_overflow_hint {
        inner_height.saturating_sub(1)
    } else {
        inner_height
    };
    let capacity = (option_lines / 2).max(1).min(len);
    let cursor = cursor.min(len - 1);
    let start = cursor.saturating_add(1).saturating_sub(capacity);
    let end = (start + capacity).min(len);
    start..end
}

impl Selector {
    pub fn new(title: &str, options: Vec<SelectOption>) -> Self {
        // Start cursor on the active option if one exists
        let cursor = options.iter().position(|o| o.active).unwrap_or(0);
        Self {
            title: title.to_string(),
            options,
            cursor,
            visible: true,
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.options.len() {
            self.cursor += 1;
        }
    }

    pub fn selected_value(&self) -> &str {
        &self.options[self.cursor].value
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
    }

    /// Render the selector popup centered in the given area.
    pub fn render(&self, area: Rect, frame: &mut Frame, t: &dyn Theme) {
        let popup_area = super::command_surfaces::command_modal_area(area);
        let inner_height = popup_area.height.saturating_sub(2) as usize;
        let visible_window = selector_visible_window(self.cursor, self.options.len(), inner_height);
        let truncated = visible_window.len() < self.options.len();

        // Two-line layout: label on line 1, description on line 2 (indented)
        // This ensures proper alignment regardless of label length.
        let mut items: Vec<Line<'static>> = Vec::new();

        for i in visible_window {
            let opt = &self.options[i];
            let is_cursor = i == self.cursor;
            let marker = if opt.active && is_cursor {
                "● "
            } else if opt.active {
                "○ "
            } else if is_cursor {
                "▸ "
            } else {
                "  "
            };

            let label_style = if is_cursor {
                Style::default().fg(t.fg()).add_modifier(Modifier::BOLD)
            } else if opt.active {
                Style::default().fg(t.accent())
            } else {
                Style::default().fg(t.muted())
            };

            let marker_style = if is_cursor {
                Style::default().fg(t.accent())
            } else if opt.active {
                Style::default().fg(t.success())
            } else {
                Style::default().fg(t.dim())
            };

            let desc_style = Style::default().fg(if is_cursor { t.muted() } else { t.dim() });

            // Line 1: marker + label
            items.push(Line::from(vec![
                Span::styled(marker.to_string(), marker_style),
                Span::styled(opt.label.clone(), label_style),
            ]));

            // Line 2: description (indented under label)
            if !opt.description.is_empty() {
                items.push(Line::from(Span::styled(
                    format!("  {}", opt.description),
                    desc_style,
                )));
            } else {
                // Empty line to maintain spacing
                items.push(Line::from(""));
            }

            if items.len() >= inner_height {
                break;
            }
        }

        if truncated {
            items.push(Line::from(Span::styled(
                "  … more options; use ↑/↓ to navigate",
                Style::default().fg(t.dim()),
            )));
            items.truncate(inner_height);
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(if true { t.accent() } else { t.border() }))
            .title(Span::styled(
                format!(" {} ", self.title),
                t.style_accent_bold(),
            ));

        let bg_style = Style::default().bg(t.card_bg());

        frame.render_widget(Clear, popup_area);
        let widget = Paragraph::new(items).block(block).style(bg_style);
        frame.render_widget(widget, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_options() -> Vec<SelectOption> {
        vec![
            SelectOption {
                value: "a".into(),
                label: "Alpha".into(),
                description: "First".into(),
                active: false,
            },
            SelectOption {
                value: "b".into(),
                label: "Beta".into(),
                description: "Second".into(),
                active: true,
            },
            SelectOption {
                value: "c".into(),
                label: "Gamma".into(),
                description: "Third".into(),
                active: false,
            },
        ]
    }

    #[test]
    fn visible_window_keeps_cursor_in_view_when_selector_overflows() {
        assert_eq!(selector_visible_window(0, 20, 22), 0..10);
        assert_eq!(selector_visible_window(10, 20, 22), 1..11);
        assert_eq!(selector_visible_window(19, 20, 22), 10..20);
    }

    #[test]
    fn visible_window_handles_tiny_or_empty_selector_areas() {
        assert_eq!(selector_visible_window(0, 0, 22), 0..0);
        assert_eq!(selector_visible_window(5, 20, 1), 5..6);
    }

    #[test]
    fn cursor_starts_on_active() {
        let sel = Selector::new("Test", make_options());
        assert_eq!(sel.cursor, 1, "should start on the active option");
        assert_eq!(sel.selected_value(), "b");
    }

    #[test]
    fn move_up_and_down() {
        let mut sel = Selector::new("Test", make_options());
        assert_eq!(sel.cursor, 1);

        sel.move_up();
        assert_eq!(sel.cursor, 0);
        assert_eq!(sel.selected_value(), "a");

        sel.move_up(); // already at top
        assert_eq!(sel.cursor, 0);

        sel.move_down();
        sel.move_down();
        assert_eq!(sel.cursor, 2);
        assert_eq!(sel.selected_value(), "c");

        sel.move_down(); // already at bottom
        assert_eq!(sel.cursor, 2);
    }

    #[test]
    fn dismiss() {
        let mut sel = Selector::new("Test", make_options());
        assert!(sel.visible);
        sel.dismiss();
        assert!(!sel.visible);
    }

    #[test]
    fn no_active_starts_at_zero() {
        let options = vec![
            SelectOption {
                value: "x".into(),
                label: "X".into(),
                description: "".into(),
                active: false,
            },
            SelectOption {
                value: "y".into(),
                label: "Y".into(),
                description: "".into(),
                active: false,
            },
        ];
        let sel = Selector::new("Test", options);
        assert_eq!(sel.cursor, 0);
    }

    #[test]
    fn single_option() {
        let options = vec![SelectOption {
            value: "only".into(),
            label: "Only".into(),
            description: "".into(),
            active: true,
        }];
        let mut sel = Selector::new("Test", options);
        sel.move_up();
        assert_eq!(sel.cursor, 0);
        sel.move_down();
        assert_eq!(sel.cursor, 0);
        assert_eq!(sel.selected_value(), "only");
    }
}
