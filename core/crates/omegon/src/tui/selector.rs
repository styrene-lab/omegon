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
        // Two-line layout: label on line 1, description on line 2 (indented)
        // This ensures proper alignment regardless of label length.
        
        // Calculate popup dimensions
        let max_label_w = self
            .options
            .iter()
            .map(|o| o.label.len())
            .max()
            .unwrap_or(10);
        let max_desc_w = self
            .options
            .iter()
            .map(|o| o.description.len())
            .max()
            .unwrap_or(0);
        
        // Width: max of (label + marker) or (description + indent)
        // Height: 2 lines per option (label + desc) + borders
        let label_line_w = max_label_w + 2; // +2 for marker
        let desc_line_w = max_desc_w + 4;   // +4 for indent
        let content_w = label_line_w.max(desc_line_w).min(area.width as usize - 4);
        let popup_w = (content_w + 4) as u16; // +4 for padding
        let popup_h = ((self.options.len() * 2) as u16 + 2).min(area.height.saturating_sub(1));

        // Center the popup, but keep it within bounds
        let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
        let popup_area = Rect::new(x, y, popup_w, popup_h);

        let mut items: Vec<Line<'static>> = Vec::new();
        
        for (i, opt) in self.options.iter().enumerate() {
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
