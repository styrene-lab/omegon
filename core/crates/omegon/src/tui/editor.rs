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
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

/// Editor mode — normal input, reverse search, or secret input.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorMode {
    Normal,
    /// Reverse incremental search: typing filters history matches.
    ReverseSearch {
        query: String,
        /// Index into history of the current match (None = no match).
        match_idx: Option<usize>,
    },
    /// Secret input — captures text but renders as dots. Used by /secrets set.
    /// The label is shown as the editor title (e.g. "OPENROUTER_API_KEY").
    SecretInput {
        label: String,
        buffer: String,
    },
}

/// A terminal-style text editor with history and reverse search.
pub struct Editor {
    pub textarea: TextArea<'static>,
    mode: EditorMode,
    /// Kill ring — last killed text (Ctrl+K, Ctrl+U).
    kill_ring: Option<String>,
    /// Tracked scroll offset for cursor positioning. Updated each frame
    /// to match the textarea's internal viewport (which is pub(crate)).
    scroll_row: u16,
    scroll_col: u16,
}

impl Editor {
    pub fn new() -> Self {
        let mut ta = TextArea::default();
        ta.set_cursor_line_style(Style::default());
        ta.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
        ta.set_placeholder_text("Ask anything, or type / for commands");
        ta.set_placeholder_style(Style::default().fg(Color::from_u32(0x00405870)));
        Self {
            textarea: ta,
            mode: EditorMode::Normal,
            kill_ring: None,
            scroll_row: 0,
            scroll_col: 0,
        }
    }

    /// Apply theme styles to the textarea.
    pub fn apply_theme(&mut self, t: &dyn Theme) {
        self.textarea
            .set_style(Style::default().fg(t.fg()).bg(t.surface_bg()));
        self.textarea
            .set_cursor_line_style(Style::default().bg(t.surface_bg()));
        self.textarea
            .set_cursor_style(Style::default().fg(t.bg()).bg(t.fg()));
        self.textarea
            .set_placeholder_style(Style::default().fg(t.dim()));
    }

    /// Number of content lines in the editor (for dynamic height).
    pub fn line_count(&self) -> usize {
        self.textarea.lines().len().max(1)
    }

    /// Number of visual rows needed to display the current buffer within the
    /// given content width, accounting for soft wrapping.
    pub fn visual_line_count(&self, content_width: u16) -> usize {
        let width = content_width.max(1) as usize;
        self.textarea
            .lines()
            .iter()
            .map(|line| {
                let display_width = UnicodeWidthStr::width(line.as_str());
                display_width.max(1).div_ceil(width)
            })
            .sum::<usize>()
            .max(1)
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
        if let EditorMode::ReverseSearch {
            ref query,
            ref mut match_idx,
        } = self.mode
        {
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
        if let EditorMode::ReverseSearch {
            ref query,
            ref mut match_idx,
        } = self.mode
        {
            if query.is_empty() || history.is_empty() {
                return None;
            }
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
        if let EditorMode::ReverseSearch {
            match_idx: Some(idx),
            ..
        } = &self.mode
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

    // ─── Secret input mode ──────────────────────────────────────

    /// Enter secret input mode — keystrokes are captured but displayed as dots.
    pub fn start_secret_input(&mut self, label: &str) {
        self.mode = EditorMode::SecretInput {
            label: label.to_string(),
            buffer: String::new(),
        };
    }

    /// Insert a character into the secret buffer.
    pub fn secret_insert(&mut self, c: char) {
        if let EditorMode::SecretInput { ref mut buffer, .. } = self.mode {
            buffer.push(c);
        }
    }

    /// Backspace in secret mode.
    pub fn secret_backspace(&mut self) {
        if let EditorMode::SecretInput { ref mut buffer, .. } = self.mode {
            buffer.pop();
        }
    }

    /// Take the secret value and return to normal mode.
    pub fn take_secret(&mut self) -> Option<(String, String)> {
        if let EditorMode::SecretInput {
            ref label,
            ref buffer,
        } = self.mode
        {
            let result = Some((label.clone(), buffer.clone()));
            self.mode = EditorMode::Normal;
            result
        } else {
            None
        }
    }

    /// Cancel secret input.
    pub fn cancel_secret(&mut self) {
        if matches!(self.mode, EditorMode::SecretInput { .. }) {
            self.mode = EditorMode::Normal;
        }
    }

    /// CRT noise glyphs for secret masking — same aesthetic as the splash screen.
    const SECRET_GLYPHS: &'static [char] = &[
        '▓', '▒', '░', '█', '▄', '▀', '▌', '▐', '▊', '▋', '▍', '▎', '◆', '■', '□', '▪', '◇', '╬',
        '╪', '╫', '┼', '│', '─',
    ];

    /// Get the masked display string for secret mode — CRT noise glyphs
    /// that change per-character based on the buffer position, giving the
    /// appearance of live encrypted data.
    pub fn secret_display(&self) -> Option<(&str, String)> {
        if let EditorMode::SecretInput {
            ref label,
            ref buffer,
        } = self.mode
        {
            let masked: String = buffer
                .bytes()
                .enumerate()
                .map(|(i, b)| {
                    // Deterministic but visually chaotic — hash position + byte value
                    let idx = ((i as u8).wrapping_mul(7).wrapping_add(b).wrapping_mul(13)) as usize;
                    Self::SECRET_GLYPHS[idx % Self::SECRET_GLYPHS.len()]
                })
                .collect();
            Some((label, masked))
        } else {
            None
        }
    }

    // ─── Kill ring operations ───────────────────────────────────

    /// Kill to end of line (Ctrl+K).
    pub fn kill_to_end(&mut self) {
        // Select to end of line and cut
        let (row, col) = self.textarea.cursor();
        let line = self
            .textarea
            .lines()
            .get(row)
            .map(|l| l.as_str())
            .unwrap_or("");
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
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::Back);
    }

    pub fn move_right(&mut self) {
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::Forward);
    }

    pub fn move_home(&mut self) {
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::Head);
    }

    pub fn move_end(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::End);
    }

    pub fn move_up(&mut self) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::Up);
    }

    pub fn move_down(&mut self) {
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::Down);
    }

    /// Insert a newline at the current cursor position (for Shift+Enter multiline input).
    pub fn insert_newline(&mut self) {
        self.textarea.insert_newline();
    }

    /// Current cursor row (0-based). Used to decide if Up/Down should navigate
    /// within the editor or fall through to history/scroll.
    pub fn cursor_row(&self) -> usize {
        self.textarea.cursor().0
    }

    /// Compute the cursor's screen position within the given editor area.
    /// Call this AFTER rendering the textarea widget so the scroll state
    /// is current. Returns (x, y) in absolute screen coordinates.
    pub fn cursor_screen_position(&mut self, editor_area: Rect) -> (u16, u16) {
        let (crow, ccol) = self.textarea.cursor();
        let crow = crow as u16;
        let ccol = ccol as u16;

        // The block has Borders::TOP only, so inner area is 1 row shorter at top.
        let inner_y = editor_area.y + 1;
        let inner_height = editor_area.height.saturating_sub(1);
        let inner_width = editor_area.width;

        // Mirror ratatui-textarea's scroll logic for both axes:
        // keep cursor visible within the viewport.
        if inner_height == 0 || inner_width == 0 {
            return (editor_area.x, inner_y);
        }

        // Vertical scroll
        if crow < self.scroll_row {
            self.scroll_row = crow;
        } else if crow >= self.scroll_row + inner_height {
            self.scroll_row = crow + 1 - inner_height;
        }

        // Horizontal scroll
        if ccol < self.scroll_col {
            self.scroll_col = ccol;
        } else if ccol >= self.scroll_col + inner_width {
            self.scroll_col = ccol + 1 - inner_width;
        }

        let screen_y = inner_y + (crow - self.scroll_row);
        let screen_x = editor_area.x + (ccol - self.scroll_col);

        // Clamp to editor area as a safety net
        let screen_x = screen_x.min(editor_area.x + inner_width.saturating_sub(1));
        let screen_y = screen_y.min(editor_area.y + editor_area.height.saturating_sub(1));
        (screen_x, screen_y)
    }

    pub fn move_word_backward(&mut self) {
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::WordBack);
    }

    pub fn move_word_forward(&mut self) {
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::WordForward);
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

    #[test]
    fn insert_newline_creates_multiline() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert('b');
        e.insert_newline();
        e.insert('c');
        e.insert('d');
        assert_eq!(e.line_count(), 2);
        assert_eq!(e.render_text(), "ab\ncd");
    }

    #[test]
    fn cursor_row_tracks_position() {
        let mut e = Editor::new();
        e.insert('a');
        assert_eq!(e.cursor_row(), 0);
        e.insert_newline();
        assert_eq!(e.cursor_row(), 1);
        e.insert_newline();
        assert_eq!(e.cursor_row(), 2);
        e.move_up();
        assert_eq!(e.cursor_row(), 1);
        e.move_up();
        assert_eq!(e.cursor_row(), 0);
        // At top, move_up stays at row 0 (no panic)
        e.move_up();
        assert_eq!(e.cursor_row(), 0);
    }

    #[test]
    fn move_down_at_bottom_stays() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert_newline();
        e.insert('b');
        assert_eq!(e.cursor_row(), 1);
        assert_eq!(e.line_count(), 2);
        // At bottom, move_down stays at last row
        e.move_down();
        assert_eq!(e.cursor_row(), 1);
    }

    #[test]
    fn take_text_preserves_newlines() {
        let mut e = Editor::new();
        e.insert('l');
        e.insert('1');
        e.insert_newline();
        e.insert('l');
        e.insert('2');
        e.insert_newline();
        e.insert('l');
        e.insert('3');
        let text = e.take_text();
        assert_eq!(text, "l1\nl2\nl3");
        assert!(e.is_empty());
        assert_eq!(e.line_count(), 1);
    }

    #[test]
    fn newline_in_middle_of_text() {
        let mut e = Editor::new();
        e.set_text("abcd");
        // Cursor is at end (col 4). Move back 2 to col 2.
        e.move_left();
        e.move_left();
        e.insert_newline();
        assert_eq!(e.render_text(), "ab\ncd");
        assert_eq!(e.cursor_row(), 1);
        assert_eq!(e.line_count(), 2);
    }

    #[test]
    fn multiline_set_text() {
        let mut e = Editor::new();
        e.set_text("line1\nline2\nline3");
        assert_eq!(e.line_count(), 3);
        assert_eq!(e.render_text(), "line1\nline2\nline3");
    }

    #[test]
    fn line_count_single_line() {
        let mut e = Editor::new();
        assert_eq!(e.line_count(), 1); // empty = at least 1
        e.insert('x');
        assert_eq!(e.line_count(), 1);
    }
}
