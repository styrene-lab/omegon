//! Ratatui adapters for renderer-neutral inline row projections.

use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::surfaces::inline::{
    ActionHint, InlineAffordance, InlineCell, InlineCellRole, InlineOverflowPolicy, InlineRow,
    KeyChord,
};

use super::theme::Theme;

const ELLIPSIS: &str = "…";
pub const DETAILS_HINT_LABEL: &str = "^O details";

pub fn key_chord_label(chord: KeyChord) -> String {
    match chord {
        KeyChord::Ctrl(ch) => format!("^{}", ch.to_ascii_uppercase()),
        KeyChord::Enter => "↵".to_string(),
        KeyChord::Esc => "Esc".to_string(),
        KeyChord::Tab => "Tab".to_string(),
        KeyChord::ShiftTab => "⇧Tab".to_string(),
    }
}

pub fn action_hint_label(hint: ActionHint) -> String {
    format!("{} {}", key_chord_label(hint.key), hint.action.label())
}

pub fn details_hint_cell() -> InlineCell<String> {
    InlineCell::new(DETAILS_HINT_LABEL.to_string(), InlineCellRole::Affordance)
        .with_priority(crate::surfaces::inline::InlinePriority::Required)
}

pub fn expand_hint_cell() -> InlineCell<String> {
    InlineCell::new(
        action_hint_label(ActionHint::new(
            InlineAffordance::Expand,
            KeyChord::Ctrl('O'),
        )),
        InlineCellRole::Affordance,
    )
    .with_priority(crate::surfaces::inline::InlinePriority::Required)
}

pub fn render_inline_text_row(row: &InlineRow<String>, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }

    let left = join_cells(&row.left);
    let right = join_cells(&row.right);
    match (left.is_empty(), right.is_empty()) {
        (true, true) => String::new(),
        (false, true) => truncate_display(&left, width),
        (true, false) => truncate_display(&right, width),
        (false, false) => render_split_text(&left, &right, width, row.overflow),
    }
}

pub fn render_inline_row(
    row: &InlineRow<String>,
    width: u16,
    t: &dyn Theme,
    bg: Color,
) -> Line<'static> {
    let text = render_inline_text_row(row, width);
    Line::from(Span::styled(text, Style::default().fg(t.fg()).bg(bg)))
}

fn render_split_text(
    left: &str,
    right: &str,
    width: usize,
    overflow: InlineOverflowPolicy,
) -> String {
    let right_width = UnicodeWidthStr::width(right);
    match overflow {
        InlineOverflowPolicy::PreserveRight if right_width < width => {
            let separator = " · ";
            let separator_width = UnicodeWidthStr::width(separator);
            let left_budget = width.saturating_sub(right_width + separator_width);
            let left = truncate_display(left, left_budget);
            if left.is_empty() {
                truncate_display(right, width)
            } else {
                let joined = format!("{left}{separator}{right}");
                truncate_display(&joined, width)
            }
        }
        InlineOverflowPolicy::PreserveRight => truncate_display(right, width),
        InlineOverflowPolicy::DropRightWhenCrowded
            if UnicodeWidthStr::width(left) + 1 + right_width <= width =>
        {
            let gap = width
                .saturating_sub(UnicodeWidthStr::width(left) + right_width)
                .max(1);
            format!("{left}{}{right}", " ".repeat(gap))
        }
        InlineOverflowPolicy::DropRightWhenCrowded => truncate_display(left, width),
    }
}

fn join_cells(cells: &[InlineCell<String>]) -> String {
    cells
        .iter()
        .filter(|cell| !cell.text.is_empty())
        .map(|cell| cell.text.as_str())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn truncate_display(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= budget {
        return text.to_string();
    }
    if budget <= UnicodeWidthStr::width(ELLIPSIS) {
        return ELLIPSIS.to_string();
    }

    let mut out = String::new();
    let mut used = 0usize;
    let ellipsis_width = UnicodeWidthStr::width(ELLIPSIS);
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width + ellipsis_width > budget {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push_str(ELLIPSIS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::inline::{InlineCellRole, InlinePriority};

    #[test]
    fn ctrl_key_uses_ascii_caret() {
        assert_eq!(key_chord_label(KeyChord::Ctrl('o')), "^O");
        assert_eq!(
            action_hint_label(ActionHint::new(
                InlineAffordance::Details,
                KeyChord::Ctrl('O')
            )),
            "^O details"
        );
    }

    #[test]
    fn text_row_keeps_affordance_inline() {
        let row = InlineRow::new(
            vec![InlineCell::new("bash".to_string(), InlineCellRole::Value)],
            vec![details_hint_cell()],
        );
        let rendered = render_inline_text_row(&row, 32);
        assert_eq!(rendered, "bash · ^O details");
        assert_eq!(UnicodeWidthStr::width(rendered.as_str()), 17);
    }

    #[test]
    fn text_row_truncates_left_before_required_right() {
        let row = InlineRow::new(
            vec![InlineCell::new(
                "very long command summary with many tokens".to_string(),
                InlineCellRole::Value,
            )],
            vec![details_hint_cell().with_priority(InlinePriority::Required)],
        );
        let rendered = render_inline_text_row(&row, 28);
        assert_eq!(UnicodeWidthStr::width(rendered.as_str()), 28);
        assert!(rendered.contains('…'), "{rendered:?}");
        assert!(rendered.ends_with(" · ^O details"), "{rendered:?}");
    }
}
