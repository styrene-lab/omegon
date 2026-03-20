//! Terminal-style text editor backed by ratatui-textarea.
//!
//! Wraps `ratatui_textarea::TextArea` with our API surface:
//! - Single-line default (Enter submits, not inserts newline)
//! - History navigation (Up/Down when empty)
//! - Reverse incremental search (Ctrl+R)
//! - Kill ring (Ctrl+K, Ctrl+U, Ctrl+Y)
//!
//! The textarea handles all basic editing: cursor movement, word ops,
//! clipboard paste (bracketed paste), undo/redo, and character insertion.

use ratatui::prelude::*;
use ratatui_textarea::TextArea;

use super::theme::Theme;

/// Editor mode — normal input or reverse search.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorMode {
    Normal,
    /// Reverse incremental search: typing filters history matches.
    ReverseSearch {
        query: String,
        /// Index into history of the current match (None = no match).
        match_idx: Option<usize>,
    },
}

/// A terminal-style text editor with history and reverse search.
pub struct Editor {
    pub textarea: TextArea<'static>,
    mode: EditorMode,
    /// Kill ring — last killed text (Ctrl+K, Ctrl+U).
    kill_ring: Option<String>,
}

impl Editor {
    pub fn new() -> Self {
        let mut ta = TextArea::default();
        ta.set_cursor_line_style(Style::default());
        ta.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
        Self {
            textarea: ta,
            mode: EditorMode::Normal,
            kill_ring: None,
        }
    }

    /// Apply theme styles to the textarea.
    pub fn apply_theme(&mut self, t: &dyn Theme) {
        self.textarea.set_style(Style::default().fg(t.fg()).bg(t.surface_bg()));
        self.textarea.set_cursor_line_style(Style::default().bg(t.surface_bg()));
        self.textarea.set_cursor_style(Style::default().fg(t.bg()).bg(t.fg()));
    }

    pub fn mode(&self) -> &EditorMode {
        &self.mode
    }

    // ─── Reverse search ─────────────────────────────────────────

    pub fn start_reverse_search(&mut self) {
        self.mode = EditorMode::ReverseSearch {
            query: String::new(),
            match_idx: None,
        };
    }

    pub fn search_insert(&mut self, c: char) {
        if let EditorMode::ReverseSearch { ref mut query, .. } = self.mode {
            query.push(c);
        }
    }

    pub fn search_backspace(&mut self) {
        if let EditorMode::ReverseSearch { ref mut query, .. } = self.mode {
            query.pop();
        }
    }

    pub fn search_update(&mut self, history: &[String]) -> Option<String> {
        if let EditorMode::ReverseSearch { ref query, ref mut match_idx } = self.mode {
            if query.is_empty() || history.is_empty() {
                *match_idx = None;
                return None;
            }
            let start = match_idx
                .map(|i| i.saturating_sub(1))
                .unwrap_or(history.len() - 1);
            for i in (0..=start).rev() {
                if history[i].contains(query.as_str()) {
                    *match_idx = Some(i);
                    return Some(history[i].clone());
                }
            }
            for i in (0..history.len()).rev() {
                if history[i].contains(query.as_str()) {
                    *match_idx = Some(i);
                    return Some(history[i].clone());
                }
            }
            *match_idx = None;
            None
        } else {
            None
        }
    }

    pub fn search_prev(&mut self, history: &[String]) -> Option<String> {
        if let EditorMode::ReverseSearch { ref query, ref mut match_idx } = self.mode {
            if query.is_empty() || history.is_empty() { return None; }
            let start = match_idx.map(|i| i.saturating_sub(1)).unwrap_or(0);
            for i in (0..=start).rev() {
                if history[i].contains(query.as_str()) && Some(i) != *match_idx {
                    *match_idx = Some(i);
                    return Some(history[i].clone());
                }
            }
            None
        } else {
            None
        }
    }

    pub fn accept_search(&mut self, history: &[String]) {
        if let EditorMode::ReverseSearch { match_idx: Some(idx), .. } = &self.mode
            && let Some(entry) = history.get(*idx)
        {
            self.set_text(entry);
        }
        self.mode = EditorMode::Normal;
    }

    pub fn cancel_search(&mut self) {
        self.mode = EditorMode::Normal;
    }

    pub fn search_query(&self) -> Option<&str> {
        if let EditorMode::ReverseSearch { ref query, .. } = self.mode {
            Some(query)
        } else {
            None
        }
    }

    // ─── Kill ring operations ───────────────────────────────────

    /// Kill to end of line (Ctrl+K).
    pub fn kill_to_end(&mut self) {
        // Select to end of line and cut
        let (row, col) = self.textarea.cursor();
        let line = self.textarea.lines().get(row).map(|l| l.as_str()).unwrap_or("");
        if col < line.len() {
            let killed = line[col..].to_string();
            self.textarea.delete_line_by_end();
            self.kill_ring = Some(killed);
        }
    }

    /// Clear entire line (Ctrl+U).
    pub fn clear_line(&mut self) {
        let text = self.render_text().to_string();
        if !text.is_empty() {
            self.kill_ring = Some(text);
            self.set_text("");
        }
    }

    /// Yank (paste) from kill ring (Ctrl+Y).
    pub fn yank(&mut self) {
        if let Some(ref text) = self.kill_ring.clone() {
            self.textarea.insert_str(text);
        }
    }

    // ─── Buffer access ──────────────────────────────────────────

    /// Take the current text and clear the editor.
    pub fn take_text(&mut self) -> String {
        self.mode = EditorMode::Normal;
        let text = self.textarea.lines().join("\n");
        self.set_text("");
        text
    }

    /// Get cursor column position (display width).
    pub fn cursor_position(&self) -> usize {
        let (_, col) = self.textarea.cursor();
        col
    }

    /// Set the buffer text (for history navigation).
    pub fn set_text(&mut self, text: &str) {
        // Clear and replace
        self.textarea.select_all();
        self.textarea.cut();
        if !text.is_empty() {
            self.textarea.insert_str(text);
        }
    }

    /// Get current text for display/inspection.
    pub fn render_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    // ─── Input passthrough ──────────────────────────────────────

    /// Pass a crossterm event to the textarea for handling.
    /// Returns true if the textarea consumed the event.
    pub fn input(&mut self, event: &crossterm::event::Event) -> bool {
        let input: ratatui_textarea::Input = event.clone().into();
        self.textarea.input(input)
    }

    /// Insert a character directly (for compat with old API).
    pub fn insert(&mut self, c: char) {
        self.textarea.insert_char(c);
    }

    /// Delete backward (for compat).
    pub fn backspace(&mut self) {
        self.textarea.delete_char();
    }

    pub fn move_left(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::Back);
    }

    pub fn move_right(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::Forward);
    }

    pub fn move_home(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::Head);
    }

    pub fn move_end(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::End);
    }

    pub fn move_word_backward(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::WordBack);
    }

    pub fn move_word_forward(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::WordForward);
    }

    pub fn delete_word_backward(&mut self) {
        self.textarea.delete_word();
    }

    pub fn delete_word_forward(&mut self) {
        self.textarea.delete_next_word();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_and_take() {
        let mut e = Editor::new();
        e.insert('h');
        e.insert('i');
        assert_eq!(e.render_text(), "hi");
        assert_eq!(e.take_text(), "hi");
        assert_eq!(e.render_text(), "");
    }

    #[test]
    fn backspace() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert('b');
        e.insert('c');
        e.backspace();
        assert_eq!(e.render_text(), "ab");
    }

    #[test]
    fn cursor_movement() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert('b');
        e.insert('c');
        e.move_left();
        e.insert('x');
        assert_eq!(e.render_text(), "abxc");
    }

    #[test]
    fn home_end() {
        let mut e = Editor::new();
        e.set_text("abc");
        e.move_home();
        e.insert('0');
        assert_eq!(e.render_text(), "0abc");
        e.move_end();
        e.insert('9');
        assert_eq!(e.render_text(), "0abc9");
    }

    #[test]
    fn clear_line() {
        let mut e = Editor::new();
        e.set_text("hello");
        e.clear_line();
        assert_eq!(e.render_text(), "");
        assert_eq!(e.kill_ring.as_deref(), Some("hello"));
    }

    #[test]
    fn yank() {
        let mut e = Editor::new();
        e.set_text("hello world");
        e.clear_line();
        assert_eq!(e.render_text(), "");
        e.yank();
        assert_eq!(e.render_text(), "hello world");
    }

    #[test]
    fn reverse_search() {
        let history = vec![
            "cargo build".to_string(),
            "cargo test".to_string(),
            "git status".to_string(),
            "cargo clippy".to_string(),
        ];
        let mut e = Editor::new();
        e.start_reverse_search();
        assert!(matches!(e.mode(), EditorMode::ReverseSearch { .. }));

        e.search_insert('t');
        e.search_insert('e');
        e.search_insert('s');
        e.search_insert('t');
        let result = e.search_update(&history);
        assert_eq!(result.as_deref(), Some("cargo test"));

        e.accept_search(&history);
        assert_eq!(e.render_text(), "cargo test");
        assert!(matches!(e.mode(), EditorMode::Normal));
    }

    #[test]
    fn reverse_search_cancel() {
        let mut e = Editor::new();
        e.set_text("original");
        e.start_reverse_search();
        e.search_insert('x');
        e.cancel_search();
        assert_eq!(e.render_text(), "original");
        assert!(matches!(e.mode(), EditorMode::Normal));
    }

    #[test]
    fn reverse_search_backspace() {
        let mut e = Editor::new();
        e.start_reverse_search();
        e.search_insert('t');
        e.search_insert('e');
        e.search_insert('s');
        assert_eq!(e.search_query(), Some("tes"));
        e.search_backspace();
        assert_eq!(e.search_query(), Some("te"));
    }

    #[test]
    fn unicode_handling() {
        let mut e = Editor::new();
        e.insert('é');
        e.insert('→');
        assert_eq!(e.render_text(), "é→");
        e.backspace();
        assert_eq!(e.render_text(), "é");
    }

    #[test]
    fn reverse_search_empty_history_no_panic() {
        let empty: Vec<String> = vec![];
        let mut e = Editor::new();
        e.start_reverse_search();
        e.search_insert('x');
        let result = e.search_update(&empty);
        assert!(result.is_none());
        let result2 = e.search_prev(&empty);
        assert!(result2.is_none());
    }

    #[test]
    fn set_text_replaces() {
        let mut e = Editor::new();
        e.set_text("first");
        assert_eq!(e.render_text(), "first");
        e.set_text("second");
        assert_eq!(e.render_text(), "second");
    }

    #[test]
    fn empty_editor() {
        let e = Editor::new();
        assert!(e.is_empty());
        assert_eq!(e.render_text(), "");
    }
}
