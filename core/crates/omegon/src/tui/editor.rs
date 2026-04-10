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

use std::path::{Path, PathBuf};

use ratatui::prelude::*;
use ratatui_textarea::TextArea;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme::Theme;

/// Split `text` into visual rows of at most `width` display columns using
/// character-boundary wrapping (not word-boundary wrapping).
///
/// Both the operator input renderer and `cursor_screen_position` use this
/// function so they always agree on which visual cell each character occupies.
/// Using different wrapping algorithms causes cursor drift that compounds
/// across wrapped rows.
pub fn wrap_chars_at(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut col: usize = 0;
    for ch in text.chars() {
        let ch_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch_w == 0 {
            current.push(ch);
            continue;
        }
        if col + ch_w > width {
            lines.push(std::mem::take(&mut current));
            col = 0;
        }
        current.push(ch);
        col += ch_w;
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

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
    /// Tracked vertical scroll offset for wrapped multiline rendering.
    scroll_row: u16,
    /// Internal text model. Inline tokens are stored as OBJECT REPLACEMENT
    /// characters and projected into visible placeholders for rendering.
    model_text: String,
    /// Inline token payloads in token order as they appear in `model_text`.
    inline_tokens: Vec<InlineToken>,
}

impl Editor {
    const INLINE_TOKEN_SENTINEL: char = '\u{FFFC}';
    const COLLAPSIBLE_PASTE_MIN_LINES: usize = 3;
    const COLLAPSIBLE_PASTE_MIN_CHARS: usize = 120;

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
            model_text: String::new(),
            inline_tokens: Vec::new(),
        }
    }

    fn attachment_placeholder(path: &Path, idx: usize) -> String {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());
        let kind = match ext.as_deref() {
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif") => "image",
            Some("pdf") => "pdf",
            _ => "attachment",
        };
        format!("[{kind}{idx}]")
    }

    fn paste_placeholder(text: &str, idx: usize) -> String {
        let newline_count = text.chars().filter(|ch| *ch == '\n').count();
        let extra_lines = newline_count.saturating_sub(1);
        if extra_lines > 0 {
            format!("[Pasted text #{} +{} lines]", idx + 1, extra_lines)
        } else {
            format!("[Pasted text #{}]", idx + 1)
        }
    }

    fn token_placeholder(token: &InlineToken, idx: usize) -> String {
        match token {
            InlineToken::Attachment(path) => Self::attachment_placeholder(path, idx),
            InlineToken::CollapsedPaste { text } => Self::paste_placeholder(text, idx),
        }
    }

    fn should_collapse_paste(text: &str) -> bool {
        let line_count = text.split('\n').count();
        let has_blank_line = text.contains("\n\n");
        (line_count >= Self::COLLAPSIBLE_PASTE_MIN_LINES && has_blank_line)
            || text.chars().count() >= Self::COLLAPSIBLE_PASTE_MIN_CHARS
    }

    fn char_to_byte_idx(text: &str, char_idx: usize) -> usize {
        text.char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(text.len())
    }

    fn model_lines(&self) -> Vec<&str> {
        if self.model_text.is_empty() {
            vec![""]
        } else {
            self.model_text.split('\n').collect()
        }
    }

    fn projected_lines(&self) -> Vec<String> {
        let text = self.render_text();
        if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(ToOwned::to_owned).collect()
        }
    }

    fn raw_set_textarea_text(&mut self, text: &str) {
        self.textarea.select_all();
        self.textarea.cut();
        if !text.is_empty() {
            self.textarea.insert_str(text);
        }
    }

    fn projection(&self) -> Projection {
        let mut text = String::new();
        let mut token_spans = Vec::new();
        let mut token_ord = 0usize;
        let mut projected_char_idx = 0usize;

        for (model_char_idx, ch) in self.model_text.chars().enumerate() {
            if ch == Self::INLINE_TOKEN_SENTINEL {
                let label = self
                    .inline_tokens
                    .get(token_ord)
                    .map(|token| Self::token_placeholder(token, token_ord))
                    .unwrap_or_else(|| format!("[token{}]", token_ord));
                let label_len = label.chars().count();
                token_spans.push(TokenSpan {
                    model_char_idx,
                    token_ord,
                    start: projected_char_idx,
                    end: projected_char_idx + label_len,
                });
                text.push_str(&label);
                projected_char_idx += label_len;
                token_ord += 1;
            } else {
                text.push(ch);
                projected_char_idx += 1;
            }
        }

        Projection { text, token_spans }
    }

    fn projected_cursor(&self) -> usize {
        let (cursor_row, cursor_col) = self.textarea.cursor();
        let mut idx = 0usize;
        for (row_idx, line) in self.textarea.lines().iter().enumerate() {
            if row_idx < cursor_row {
                idx += line.chars().count() + 1;
            } else {
                idx += cursor_col;
                break;
            }
        }
        idx
    }

    fn set_projected_cursor(&mut self, projected_idx: usize) {
        self.textarea.move_cursor(ratatui_textarea::CursorMove::Head);
        while self.textarea.cursor().0 > 0 {
            self.textarea.move_cursor(ratatui_textarea::CursorMove::Up);
            self.textarea.move_cursor(ratatui_textarea::CursorMove::Head);
        }
        for _ in 0..projected_idx {
            self.textarea
                .move_cursor(ratatui_textarea::CursorMove::Forward);
        }
    }

    fn sync_textarea_from_model(&mut self, projected_cursor: usize) {
        let projection = self.projection();
        let text = projection.text.clone();
        let projected_len = text.chars().count();
        self.raw_set_textarea_text(&text);
        self.set_projected_cursor(projected_cursor.min(projected_len));
    }

    fn projected_cursor_to_model_insert_idx(&self, projected_idx: usize) -> usize {
        let projection = self.projection();
        for span in &projection.token_spans {
            if projected_idx <= span.start {
                return span.model_char_idx;
            }
            if projected_idx <= span.end {
                return span.model_char_idx + 1;
            }
        }

        let mut model_idx = 0usize;
        let mut projected_count = 0usize;
        let mut token_ord = 0usize;
        for ch in self.model_text.chars() {
            if ch == Self::INLINE_TOKEN_SENTINEL {
                let width = self
                    .inline_tokens
                    .get(token_ord)
                    .map(|token| Self::token_placeholder(token, token_ord).chars().count())
                    .unwrap_or(1);
                if projected_count >= projected_idx {
                    return model_idx;
                }
                projected_count += width;
                token_ord += 1;
            } else {
                if projected_count >= projected_idx {
                    return model_idx;
                }
                projected_count += 1;
            }
            model_idx += 1;
        }
        model_idx
    }

    fn token_span_for_backspace(&self, projected_idx: usize) -> Option<TokenSpan> {
        self.projection()
            .token_spans
            .into_iter()
            .find(|span| projected_idx >= span.start && projected_idx <= span.end)
    }

    fn token_span_containing_cursor(&self, projected_idx: usize) -> Option<TokenSpan> {
        self.projection()
            .token_spans
            .into_iter()
            .find(|span| projected_idx >= span.start && projected_idx < span.end)
    }

    fn token_span_for_edit_cursor(&self, projected_idx: usize) -> Option<TokenSpan> {
        self.projection()
            .token_spans
            .into_iter()
            .find(|span| projected_idx >= span.start && projected_idx <= span.end)
    }

    fn token_ord_before_model_idx(&self, model_idx: usize) -> usize {
        self.model_text
            .chars()
            .take(model_idx)
            .filter(|ch| *ch == Self::INLINE_TOKEN_SENTINEL)
            .count()
    }

    fn projected_char_positions(&self) -> Vec<usize> {
        let mut positions = Vec::new();
        let mut projected = 0usize;
        let mut token_ord = 0usize;
        for ch in self.model_text.chars() {
            positions.push(projected);
            if ch == Self::INLINE_TOKEN_SENTINEL {
                let width = self
                    .inline_tokens
                    .get(token_ord)
                    .map(|token| Self::token_placeholder(token, token_ord).chars().count())
                    .unwrap_or(1);
                projected += width;
                token_ord += 1;
            } else {
                projected += 1;
            }
        }
        positions
    }

    fn normalize_cursor_outside_token(&mut self, prefer_end: bool) {
        let projected_idx = self.projected_cursor();
        if let Some(span) = self.token_span_containing_cursor(projected_idx) {
            let target = if prefer_end { span.end } else { span.start };
            self.set_projected_cursor(target);
        }
    }

    fn expand_collapsed_paste_at_cursor(&mut self) -> Option<usize> {
        let projected_idx = self.projected_cursor();
        let Some(span) = self.token_span_for_edit_cursor(projected_idx) else {
            return None;
        };
        let projection = self.projection();
        let is_sole_token = span.start == 0 && span.end == projection.text.chars().count();
        let should_expand = projected_idx < span.end || is_sole_token;
        if !should_expand {
            return None;
        }
        let Some(InlineToken::CollapsedPaste { text }) = self.inline_tokens.get(span.token_ord).cloned() else {
            return None;
        };

        let start = Self::char_to_byte_idx(&self.model_text, span.model_char_idx);
        let end = Self::char_to_byte_idx(&self.model_text, span.model_char_idx + 1);
        self.model_text.replace_range(start..end, &text);
        self.inline_tokens.remove(span.token_ord);
        let expanded_projection_len = self.projection().text.chars().count();
        let cursor = if is_sole_token { 0 } else { span.start.min(expanded_projection_len) };
        self.sync_textarea_from_model(cursor);
        Some(cursor)
    }

    fn remove_token(&mut self, span: TokenSpan) {
        let start = Self::char_to_byte_idx(&self.model_text, span.model_char_idx);
        let end = Self::char_to_byte_idx(&self.model_text, span.model_char_idx + 1);
        self.model_text.replace_range(start..end, "");
        if span.token_ord < self.inline_tokens.len() {
            self.inline_tokens.remove(span.token_ord);
        }
        self.sync_textarea_from_model(span.start);
    }

    fn delete_model_char_before(&mut self, model_insert_idx: usize) {
        if model_insert_idx == 0 {
            return;
        }
        let start = Self::char_to_byte_idx(&self.model_text, model_insert_idx - 1);
        let end = Self::char_to_byte_idx(&self.model_text, model_insert_idx);
        self.model_text.replace_range(start..end, "");
        let new_cursor = self.projected_cursor().saturating_sub(1);
        self.sync_textarea_from_model(new_cursor);
    }

    fn delete_projected_range(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }

        let positions = self.projected_char_positions();
        let model_chars: Vec<char> = self.model_text.chars().collect();
        let mut new_model = String::new();
        let mut new_tokens = Vec::new();
        let mut token_ord = 0usize;

        for (idx, ch) in model_chars.iter().copied().enumerate() {
            let projected_pos = positions.get(idx).copied().unwrap_or(0);
            let remove = if ch == Self::INLINE_TOKEN_SENTINEL {
                let width = self
                    .inline_tokens
                    .get(token_ord)
                    .map(|token| Self::token_placeholder(token, token_ord).chars().count())
                    .unwrap_or(1);
                projected_pos < end && projected_pos + width > start
            } else {
                projected_pos >= start && projected_pos < end
            };

            if ch == Self::INLINE_TOKEN_SENTINEL {
                if !remove {
                    new_model.push(ch);
                    if let Some(token) = self.inline_tokens.get(token_ord).cloned() {
                        new_tokens.push(token);
                    }
                }
                token_ord += 1;
            } else if !remove {
                new_model.push(ch);
            }
        }

        self.model_text = new_model;
        self.inline_tokens = new_tokens;
        self.sync_textarea_from_model(start);
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
        self.model_lines().len().max(1)
    }

    /// Number of visual rows needed to display the current buffer within the
    /// given content width, accounting for soft wrapping.
    pub fn visual_line_count(&self, content_width: u16) -> usize {
        let width = content_width.max(1) as usize;
        self.textarea
            .lines()
            .iter()
            .map(|line| wrap_chars_at(line, width).len())
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
            self.insert_paste(text);
        }
    }

    // ─── Buffer access ──────────────────────────────────────────

    /// Take the current text and clear the editor.
    pub fn take_text(&mut self) -> String {
        self.take_submission().0
    }

    pub fn take_submission(&mut self) -> (String, Vec<PathBuf>) {
        self.mode = EditorMode::Normal;
        let mut text = String::new();
        let mut attachments = Vec::new();
        let mut token_ord = 0usize;
        for ch in self.model_text.chars() {
            if ch == Self::INLINE_TOKEN_SENTINEL {
                if let Some(token) = self.inline_tokens.get(token_ord) {
                    match token {
                        InlineToken::Attachment(path) => attachments.push(path.clone()),
                        InlineToken::CollapsedPaste { text: pasted } => text.push_str(pasted),
                    }
                }
                token_ord += 1;
            } else {
                text.push(ch);
            }
        }
        self.inline_tokens.clear();
        self.model_text.clear();
        self.raw_set_textarea_text("");
        self.scroll_row = 0;
        (text, attachments)
    }

    /// Get cursor column position (display width).
    pub fn cursor_position(&self) -> usize {
        let (_, col) = self.textarea.cursor();
        col
    }

    /// Set the buffer text (for history navigation).
    pub fn set_text(&mut self, text: &str) {
        self.model_text = text.to_string();
        self.inline_tokens.clear();
        let projection = self.projection();
        let projected_len = projection.text.chars().count();
        self.raw_set_textarea_text(&projection.text);
        self.set_projected_cursor(projected_len);
        self.scroll_row = 0;
    }

    /// Get current text for display/inspection.
    pub fn render_text(&self) -> String {
        self.projection().text
    }

    pub fn is_empty(&self) -> bool {
        self.model_text.is_empty()
    }

    // ─── Input passthrough ──────────────────────────────────────

    /// Pass a crossterm event to the textarea for handling.
    /// Returns true if the textarea consumed the event.
    pub fn input(&mut self, event: &crossterm::event::Event) -> bool {
        let input: ratatui_textarea::Input = event.clone().into();
        self.textarea.input(input)
    }

    /// Insert pasted text with normalized line endings.
    /// Terminals may deliver CRLF or bare CR; the editor should treat both as LF.
    pub fn insert_paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let projected_idx = self.projected_cursor();
        let model_idx = self.projected_cursor_to_model_insert_idx(projected_idx);
        let byte_idx = Self::char_to_byte_idx(&self.model_text, model_idx);
        if Self::should_collapse_paste(&normalized) {
            self.model_text.insert(byte_idx, Self::INLINE_TOKEN_SENTINEL);
            let ord = self.token_ord_before_model_idx(model_idx + 1).saturating_sub(1);
            self.inline_tokens
                .insert(ord, InlineToken::CollapsedPaste { text: normalized.clone() });
            let label_len = self
                .inline_tokens
                .get(ord)
                .map(|token| Self::token_placeholder(token, ord).chars().count())
                .unwrap_or(1);
            self.sync_textarea_from_model(projected_idx + label_len);
        } else {
            self.model_text.insert_str(byte_idx, &normalized);
            self.sync_textarea_from_model(projected_idx + normalized.chars().count());
        }
    }

    /// Insert a character directly (for compat with old API).
    pub fn insert(&mut self, c: char) {
        let projected_idx = self
            .expand_collapsed_paste_at_cursor()
            .unwrap_or_else(|| self.projected_cursor());
        let model_idx = self.projected_cursor_to_model_insert_idx(projected_idx);
        let byte_idx = Self::char_to_byte_idx(&self.model_text, model_idx);
        self.model_text.insert(byte_idx, c);
        self.sync_textarea_from_model(projected_idx + 1);
    }

    /// Delete backward (for compat).
    pub fn backspace(&mut self) {
        let projected_idx = self.projected_cursor();
        if let Some(span) = self.token_span_for_backspace(projected_idx) {
            self.remove_token(span);
        } else {
            let model_idx = self.projected_cursor_to_model_insert_idx(projected_idx);
            self.delete_model_char_before(model_idx);
        }
    }

    pub fn insert_attachment(&mut self, path: PathBuf) {
        let projected_idx = self.projected_cursor();
        let model_idx = self.projected_cursor_to_model_insert_idx(projected_idx);
        let byte_idx = Self::char_to_byte_idx(&self.model_text, model_idx);
        self.model_text.insert(byte_idx, Self::INLINE_TOKEN_SENTINEL);
        let ord = self.token_ord_before_model_idx(model_idx + 1).saturating_sub(1);
        self.inline_tokens.insert(ord, InlineToken::Attachment(path));
        let label_len = self
            .inline_tokens
            .get(ord)
            .map(|token| Self::token_placeholder(token, ord).chars().count())
            .unwrap_or(1);
        self.sync_textarea_from_model(projected_idx + label_len);
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

    /// Hardware cursor position for raw textarea rendering.
    ///
    /// The operator input block uses `Borders::TOP`, not a full border box, so
    /// the text origin is at `x` (no left border) and `y + 1` (one top border).
    /// Match that geometry exactly or the terminal cursor drifts by one column.
    pub fn raw_cursor_screen_position(&self, editor_area: Rect) -> (u16, u16) {
        let (row, col) = self.textarea.cursor();
        let inner_x = editor_area.x;
        let inner_y = editor_area.y.saturating_add(1);
        let inner_w = editor_area.width.max(1);
        let inner_h = editor_area.height.saturating_sub(1).max(1);

        let screen_x = inner_x + (col as u16).min(inner_w.saturating_sub(1));
        let screen_y = inner_y + (row as u16).min(inner_h.saturating_sub(1));
        (screen_x, screen_y)
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
        self.insert('\n');
    }

    /// Current cursor row (0-based). Used to decide if Up/Down should navigate
    /// within the editor or fall through to history/scroll.
    pub fn cursor_row(&self) -> usize {
        self.textarea.cursor().0
    }

    /// Compute the wrapped text used for multiline rendering.
    pub fn wrapped_text(&self) -> String {
        self.render_text()
    }

    /// Compute the cursor's screen position within the given editor area for
    /// wrapped multiline rendering. Returns (x, y) in absolute coordinates.
    ///
    /// Uses `wrap_chars_at` — same algorithm as the renderer — so the
    /// terminal cursor always points to the correct visual cell.
    pub fn cursor_screen_position(&mut self, editor_area: Rect) -> (u16, u16) {
        let (cursor_row, cursor_col) = self.textarea.cursor();
        // Normal editor mode uses Borders::TOP only: no left/right border,
        // one top border row. Cursor math must match that exact geometry.
        let content_width = editor_area.width.max(1) as usize;
        let inner_x = editor_area.x;
        let inner_y = editor_area.y + 1;
        let inner_height = editor_area.height.saturating_sub(1).max(1);

        let mut visual_row: u16 = 0;
        let mut visual_col: u16 = 0;
        let projected_lines = self.projected_lines();

        for (row_idx, line) in projected_lines.iter().enumerate() {
            if row_idx < cursor_row {
                visual_row =
                    visual_row.saturating_add(wrap_chars_at(line, content_width).len() as u16);
                continue;
            }
            // Cursor is in this logical row of the projected text the user sees.
            let prefix: String = line.chars().take(cursor_col).collect();
            let prefix_width = UnicodeWidthStr::width(prefix.as_str());
            visual_row = visual_row.saturating_add((prefix_width / content_width) as u16);
            visual_col = (prefix_width % content_width) as u16;
            break;
        }

        if visual_row < self.scroll_row {
            self.scroll_row = visual_row;
        } else if visual_row >= self.scroll_row + inner_height {
            self.scroll_row = visual_row + 1 - inner_height;
        }

        let screen_y = inner_y + visual_row.saturating_sub(self.scroll_row);
        let screen_x = inner_x + visual_col.min(content_width.saturating_sub(1) as u16);
        (
            screen_x,
            screen_y.min(inner_y + inner_height.saturating_sub(1)),
        )
    }

    pub fn visible_visual_lines(&self, content_width: u16, visible_rows: u16) -> Vec<String> {
        let width = content_width.max(1) as usize;
        let mut lines = Vec::new();
        if self.is_empty() {
            return lines;
        }
        for logical in self.render_text().split('\n') {
            lines.extend(wrap_chars_at(logical, width));
        }
        let max_start = lines.len().saturating_sub(visible_rows.max(1) as usize);
        let start = (self.scroll_row as usize).min(max_start);
        let end = (start + visible_rows.max(1) as usize).min(lines.len());
        lines[start..end].to_vec()
    }

    pub fn move_word_backward(&mut self) {
        let projected_idx = self.projected_cursor();
        if let Some(span) = self.token_span_for_backspace(projected_idx) {
            self.set_projected_cursor(span.start);
            return;
        }
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::WordBack);
        self.normalize_cursor_outside_token(false);
    }

    pub fn move_word_forward(&mut self) {
        let projected_idx = self.projected_cursor();
        if let Some(span) = self
            .projection()
            .token_spans
            .into_iter()
            .find(|span| projected_idx <= span.start)
        {
            self.set_projected_cursor(span.end);
            return;
        }
        self.textarea
            .move_cursor(ratatui_textarea::CursorMove::WordForward);
        self.normalize_cursor_outside_token(true);
    }

    pub fn delete_word_backward(&mut self) {
        let end = self.projected_cursor();
        let start = self
            .token_span_for_backspace(end)
            .map(|span| span.start)
            .unwrap_or_else(|| {
                self.textarea.delete_word();
                let new_end = self.projected_cursor();
                new_end
            });
        if start < end {
            self.delete_projected_range(start, end);
        }
    }

    pub fn delete_word_forward(&mut self) {
        let start = self.projected_cursor();
        if let Some(span) = self.token_span_for_edit_cursor(start) {
            self.delete_projected_range(span.start, span.end);
            return;
        }
        self.textarea.delete_next_word();
        let end = self.projected_cursor();
        if end > start {
            self.delete_projected_range(start, end);
        }
    }
}

#[derive(Clone)]
struct Projection {
    text: String,
    token_spans: Vec<TokenSpan>,
}

#[derive(Clone, Debug)]
enum InlineToken {
    Attachment(PathBuf),
    CollapsedPaste { text: String },
}

#[derive(Clone, Copy)]
struct TokenSpan {
    model_char_idx: usize,
    token_ord: usize,
    start: usize,
    end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_paste_normalizes_crlf_and_cr() {
        let mut e = Editor::new();
        e.insert_paste("alpha\r\nbeta\rgamma\n");
        assert_eq!(e.render_text(), "alpha\nbeta\ngamma\n");
    }

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
    fn attachment_insert_is_inline_at_cursor() {
        let mut e = Editor::new();
        e.set_text("hello world");
        for _ in 0..5 {
            e.move_left();
        }
        e.insert_attachment(PathBuf::from("/tmp/paste.png"));
        assert_eq!(e.render_text(), "hello [image0]world");
    }

    #[test]
    fn large_multiline_paste_collapses_into_single_token() {
        let mut e = Editor::new();
        e.insert_paste("alpha\n\nbeta\n");
        assert_eq!(e.render_text(), "[Pasted text #1 +2 lines]");
        let (text, attachments) = e.take_submission();
        assert_eq!(text, "alpha\n\nbeta\n");
        assert!(attachments.is_empty());
    }

    #[test]
    fn backspace_inside_collapsed_paste_removes_whole_token() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert_paste("alpha\n\nbeta\n");
        e.insert('b');
        assert_eq!(e.render_text(), "a[Pasted text #1 +2 lines]b");
        e.move_left();
        e.move_left();
        e.move_left();
        e.backspace();
        assert_eq!(e.render_text(), "ab");
    }

    #[test]
    fn collapsed_paste_token_stays_collapsed_during_lateral_navigation() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert_paste("alpha\n\nbeta\n");
        e.insert('b');
        assert_eq!(e.render_text(), "a[Pasted text #1 +2 lines]b");

        e.move_left();
        e.move_left();

        assert_eq!(e.render_text(), "a[Pasted text #1 +2 lines]b");
    }

    #[test]
    fn typing_into_collapsed_paste_expands_then_inserts() {
        let mut e = Editor::new();
        e.insert_paste("alpha\n\nbeta\n");
        assert_eq!(e.render_text(), "[Pasted text #1 +2 lines]");

        e.insert('!');

        assert_eq!(e.render_text(), "!alpha\n\nbeta\n");
    }

    #[test]
    fn backspace_inside_attachment_removes_whole_token() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert_attachment(PathBuf::from("/tmp/paste.png"));
        e.insert('b');
        assert_eq!(e.render_text(), "a[image0]b");
        e.move_left();
        e.move_left();
        e.move_left();
        e.backspace();
        assert_eq!(e.render_text(), "ab");
    }

    #[test]
    fn take_submission_strips_tokens_and_returns_attachments_in_order() {
        let mut e = Editor::new();
        e.insert('x');
        e.insert_attachment(PathBuf::from("/tmp/one.png"));
        e.insert('y');
        e.insert_attachment(PathBuf::from("/tmp/two.png"));
        let (text, attachments) = e.take_submission();
        assert_eq!(text, "xy");
        assert_eq!(
            attachments,
            vec![PathBuf::from("/tmp/one.png"), PathBuf::from("/tmp/two.png")]
        );
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
    fn wrapped_cursor_screen_position_moves_between_visual_rows() {
        let mut e = Editor::new();
        e.set_text("123456789");
        let area = Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 6,
        };

        e.move_end();
        let end = e.cursor_screen_position(area);
        e.move_left();
        e.move_left();
        e.move_left();
        let previous_visual_row = e.cursor_screen_position(area);

        assert!(
            previous_visual_row.1 < end.1 || previous_visual_row.0 < end.0,
            "moving left across a wrap boundary should move the cursor to an earlier visual cell"
        );
    }

    #[test]
    fn attachment_cursor_screen_position_uses_projected_token_width() {
        let mut e = Editor::new();
        e.set_text("ab");
        e.move_left();
        e.insert_attachment(PathBuf::from("image.png"));
        assert_eq!(e.render_text(), "a[image0]b");

        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 6,
        };

        let cursor = e.cursor_screen_position(area);
        assert_eq!(cursor, (9, 1));
    }

    #[test]
    fn word_motion_skips_inside_attachment_placeholder() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert(' ');
        e.insert_attachment(PathBuf::from("/tmp/paste.png"));
        e.insert(' ');
        e.insert('b');
        e.move_home();
        e.move_right();
        e.move_word_forward();
        e.insert('X');
        assert_eq!(e.render_text(), "a [image0]X b");
    }

    #[test]
    fn delete_word_backward_removes_attachment_token_atomically() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert(' ');
        e.insert_attachment(PathBuf::from("/tmp/paste.png"));
        e.insert(' ');
        e.insert('b');
        e.move_end();
        e.move_word_backward();
        e.delete_word_backward();
        assert_eq!(e.render_text(), "a b");
        let (text, attachments) = e.take_submission();
        assert_eq!(text, "a b");
        assert!(attachments.is_empty());
    }

    #[test]
    fn delete_word_forward_removes_attachment_token_atomically() {
        let mut e = Editor::new();
        e.insert('a');
        e.insert(' ');
        e.insert_attachment(PathBuf::from("/tmp/paste.png"));
        e.insert(' ');
        e.insert('b');
        e.move_home();
        e.move_right();
        e.move_right();
        e.delete_word_forward();
        assert_eq!(e.render_text(), "a  b");
        let (text, attachments) = e.take_submission();
        assert_eq!(text, "a  b");
        assert!(attachments.is_empty());
    }

    #[test]
    fn line_count_single_line() {
        let mut e = Editor::new();
        assert_eq!(e.line_count(), 1); // empty = at least 1
        e.insert('x');
        assert_eq!(e.line_count(), 1);
    }
}
