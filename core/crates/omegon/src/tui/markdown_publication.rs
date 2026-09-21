//! Stable Markdown publication boundaries for progressively streamed assistant text.
//!
//! Transport chunks are intentionally absent from this module. Projection is a
//! pure function of canonical message text and completion state, so a chunking
//! change cannot alter the rendered document boundary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkdownPublication<'a> {
    pub(crate) committed: &'a str,
    pub(crate) provisional: &'a str,
}

pub(crate) fn project(text: &str, complete: bool) -> MarkdownPublication<'_> {
    if complete || text.is_empty() {
        return MarkdownPublication {
            committed: text,
            provisional: "",
        };
    }

    let mut in_fence = false;
    let mut fence_start = None;
    let mut stable_end = 0;
    let mut offset = 0;

    for line_with_ending in text.split_inclusive('\n') {
        let terminated = line_with_ending.ends_with('\n');
        let line = line_with_ending
            .strip_suffix('\n')
            .unwrap_or(line_with_ending);
        let trimmed = line.trim();
        let line_end = offset + line_with_ending.len();

        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
                fence_start = None;
                if terminated {
                    stable_end = line_end;
                }
            } else {
                in_fence = true;
                fence_start = Some(offset);
            }
        } else if terminated
            && !in_fence
            && (trimmed.is_empty() || is_completed_standalone_block(trimmed))
        {
            stable_end = line_end;
        }

        offset = line_end;
    }

    // An open fence and everything after its opener must remain provisional,
    // even if a prior scan observed boundary-looking lines inside the fence.
    if let Some(start) = fence_start {
        stable_end = stable_end.min(start);
    }

    MarkdownPublication {
        committed: &text[..stable_end],
        provisional: &text[stable_end..],
    }
}

fn is_completed_standalone_block(trimmed: &str) -> bool {
    is_atx_heading(trimmed) || is_horizontal_rule(trimmed)
}

fn is_atx_heading(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(u8::is_ascii_whitespace)
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let mut marker = None;
    let mut count = 0;
    for ch in trimmed.chars().filter(|ch| !ch.is_whitespace()) {
        if !matches!(ch, '-' | '*' | '_') {
            return false;
        }
        match marker {
            Some(expected) if expected != ch => return false,
            None => marker = Some(ch),
            _ => {}
        }
        count += 1;
    }
    count >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unclosed_fence_and_its_body_remain_provisional() {
        let text = "stable paragraph\n\n```rust\nfn main() {\n";
        let projection = project(text, false);
        assert_eq!(projection.committed, "stable paragraph\n\n");
        assert_eq!(projection.provisional, "```rust\nfn main() {\n");
    }

    #[test]
    fn closed_fence_is_a_stable_boundary() {
        let text = "```rust\nfn main() {}\n```\ntrailing";
        let projection = project(text, false);
        assert_eq!(projection.committed, "```rust\nfn main() {}\n```\n");
        assert_eq!(projection.provisional, "trailing");
    }

    #[test]
    fn incomplete_table_remains_one_provisional_tail() {
        let text = "intro\n\n| name | value |\n| --- | --- |\n| alpha";
        let projection = project(text, false);
        assert_eq!(projection.committed, "intro\n\n");
        assert_eq!(
            projection.provisional,
            "| name | value |\n| --- | --- |\n| alpha"
        );
    }

    #[test]
    fn unterminated_heading_line_remains_provisional() {
        let projection = project("stable\n\n## partial", false);
        assert_eq!(projection.committed, "stable\n\n");
        assert_eq!(projection.provisional, "## partial");
    }

    #[test]
    fn closed_fence_without_terminal_newline_remains_provisional() {
        let projection = project("intro\n\n```rust\nfn main() {}\n```", false);
        assert_eq!(projection.committed, "intro\n\n");
        assert_eq!(projection.provisional, "```rust\nfn main() {}\n```");
    }

    #[test]
    fn completed_heading_line_is_a_stable_boundary() {
        let projection = project("## stable heading\ntail", false);
        assert_eq!(projection.committed, "## stable heading\n");
        assert_eq!(projection.provisional, "tail");
    }

    #[test]
    fn completion_commits_only_the_remaining_tail() {
        let text = "intro\n\n| name | value |\n| --- | --- |\n| alpha | one |";
        let streaming = project(text, false);
        let completed = project(text, true);
        assert_eq!(streaming.committed, "intro\n\n");
        assert_eq!(completed.committed, text);
        assert_eq!(completed.provisional, "");
        assert_eq!(
            &completed.committed[streaming.committed.len()..],
            streaming.provisional
        );
    }
}

// Native scrollback is immutable. Retain only the unfinished logical line and a
// bounded table block; materialized rows keep their spans through insertion.
use ratatui::{
    style::Modifier,
    text::{Line, Span},
};
use std::collections::VecDeque;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NativeMarkdown {
    pub(super) pending: String,
    ready: VecDeque<NativeLine>,
    fence: Option<(char, usize)>,
    table: Vec<String>,
    table_layout: Option<TableLayout>,
    continued: bool,
    indent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TableLayout {
    headers: Vec<String>,
    widths: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeLine {
    line: Line<'static>,
    indent: usize,
    continuation: bool,
    code: bool,
    ended: bool,
}

impl NativeMarkdown {
    #[cfg(test)]
    pub(super) fn retained_bytes(&self) -> usize {
        self.pending.len()
            + self.table_layout.as_ref().map_or(0, |layout| {
                layout.headers.iter().map(String::len).sum::<usize>()
            })
            + self.table.iter().map(String::len).sum::<usize>()
            + self
                .ready
                .iter()
                .map(|row| {
                    row.line
                        .spans
                        .iter()
                        .map(|span| span.content.len())
                        .sum::<usize>()
                })
                .sum::<usize>()
    }

    pub(super) fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    pub(super) fn has_publishable(&self, width: u16) -> bool {
        self.ready.front().is_some_and(|line| {
            line.ended
                || line.line.width() + usize::from(line.continuation) * line.indent
                    > usize::from(width)
        })
    }

    pub(super) fn push(&mut self, visible: &str, width: u16) -> bool {
        if visible.contains('\n') {
            let raw = std::mem::take(&mut self.pending);
            self.logical_line(&raw, width);
            self.continued = false;
            self.indent = 0;
            true
        } else {
            self.pending.push_str(visible);
            // A closed inline construct can be rendered and wrapped immediately.
            // Keep the unfinished word/construct so transport chunking cannot
            // turn a partial marker into literal text in permanent scrollback.
            if self.pending.len() > usize::from(width)
                && (visible.chars().all(char::is_whitespace)
                    || self
                        .pending
                        .rsplit_once(char::is_whitespace)
                        .map_or(self.pending.len(), |(_, tail)| tail.len())
                        > usize::from(width) * 2)
                && self.table.is_empty()
                && !self.pending.trim_start().starts_with('|')
                && fence_marker(&self.pending).is_none()
                && balanced_inline(&self.pending)
                && self.render(&self.pending).width() > usize::from(width)
            {
                let raw = std::mem::take(&mut self.pending);
                self.enqueue(&raw, false);
                self.continued = true;
                return true;
            }
            false
        }
    }

    pub(super) fn preview(&self, width: u16, limit: usize) -> Vec<Line<'static>> {
        let mut line = self
            .ready
            .back()
            .filter(|line| !line.ended)
            .cloned()
            .unwrap_or_else(|| NativeLine {
                line: Line::default(),
                indent: self.indent,
                continuation: self.continued,
                code: self.fence.is_some(),
                ended: false,
            });
        line.line.spans.extend(self.render(&self.pending).spans);
        if line.line.width() == 0 {
            return Vec::new();
        }
        let mut rows = Vec::new();
        for _ in 0..limit {
            let (row, remains) = take_wrapped(&mut line, usize::from(width));
            rows.push(row);
            if !remains {
                break;
            }
        }
        rows
    }

    pub(super) fn finish(&mut self, width: u16) {
        if !self.pending.is_empty() {
            let raw = std::mem::take(&mut self.pending);
            self.logical_line(&raw, width);
        }
        self.flush_table(width);
        if let Some(tail) = self.ready.back_mut() {
            tail.ended = true;
        }
    }

    fn render(&self, raw: &str) -> Line<'static> {
        let theme = super::theme::TerminalTheme;
        if self.fence.is_some() {
            return Line::raw(raw.to_owned());
        }
        if !self.continued && is_atx_heading(raw.trim_start()) {
            let text = raw.trim_start().trim_start_matches('#').trim_start();
            return Line::from(super::widgets::highlight_inline(text, &theme))
                .patch_style(ratatui::style::Style::default().add_modifier(Modifier::BOLD));
        }
        if !self.continued {
            let indent = hanging_indent(raw);
            if indent > 0 {
                let leading = raw.len() - raw.trim_start().len();
                let marker = &raw[leading..indent];
                let mut spans = vec![
                    Span::raw(" ".repeat(leading)),
                    Span::raw(if matches!(marker, "- " | "* " | "+ ") {
                        "• ".to_owned()
                    } else {
                        marker.to_owned()
                    }),
                ];
                spans.extend(super::widgets::highlight_inline(&raw[indent..], &theme));
                return Line::from(spans);
            }
        }
        Line::from(super::widgets::highlight_inline(raw, &theme))
    }

    fn enqueue(&mut self, raw: &str, ended: bool) {
        if !self.continued && self.fence.is_none() {
            self.indent = hanging_indent(raw);
        }
        let rendered = self.render(raw);
        if self.continued
            && let Some(tail) = self.ready.back_mut()
            && !tail.ended
        {
            tail.line.spans.extend(rendered.spans);
            tail.ended = ended;
            return;
        }
        self.ready.push_back(NativeLine {
            line: rendered,
            indent: self.indent,
            continuation: self.continued,
            code: self.fence.is_some(),
            ended,
        });
    }

    fn logical_line(&mut self, raw: &str, width: u16) {
        if let Some((marker, count)) = fence_marker(raw) {
            if let Some((open_marker, open_count)) = self.fence {
                if marker == open_marker
                    && count >= open_count
                    && raw.trim_start()[count..].trim().is_empty()
                {
                    self.fence = None;
                    return;
                }
            } else {
                self.flush_table(width);
                self.fence = Some((marker, count));
                return;
            }
        }
        if self.fence.is_none() && raw.trim_start().starts_with('|') {
            if self.table_layout.is_some() {
                self.table_row(raw, false, width);
            } else {
                self.table.push(raw.to_owned());
                if self.table.len() >= 2 {
                    self.flush_table(width);
                }
            }
            return;
        }
        self.flush_table(width);
        self.table_layout = None;
        self.enqueue(raw, true);
    }

    fn flush_table(&mut self, width: u16) {
        if self.table.is_empty() {
            return;
        }
        let rows = std::mem::take(&mut self.table);
        if !rows
            .get(1)
            .is_some_and(|line| super::segments::is_table_separator(line))
        {
            for row in rows {
                self.enqueue(&row, true);
            }
            return;
        }
        let headers = table_cells(&rows[0]);
        let columns = headers.len();
        let available = usize::from(width).saturating_sub(columns * 3 + 1);
        let widths = if available < columns * 3 {
            Vec::new()
        } else {
            vec![available / columns; columns]
        };
        self.table_layout = Some(TableLayout { headers, widths });
        self.table_row(&rows[0], true, width);
        if let Some(layout) = &self.table_layout
            && !layout.widths.is_empty()
        {
            let rule = format!(
                "├{}┤",
                layout
                    .widths
                    .iter()
                    .map(|width| "─".repeat(width + 2))
                    .collect::<Vec<_>>()
                    .join("┼")
            );
            self.ready.push_back(NativeLine {
                line: Line::raw(rule),
                indent: 0,
                continuation: false,
                code: true,
                ended: true,
            });
        }
    }

    fn table_row(&mut self, raw: &str, header: bool, width: u16) {
        let Some(layout) = self.table_layout.as_ref() else {
            return;
        };
        let theme = super::theme::TerminalTheme;
        let cells = table_cells(raw);
        let columns = layout.widths.len();
        // A resize can invalidate an immutable table's original geometry. Use
        // labeled cells at the new width rather than clip any column. Likewise
        // avoid eagerly expanding a huge cell into thousands of padded rows.
        let expanded_estimate = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                cell.len()
                    .div_ceil(layout.widths.get(index).copied().unwrap_or(1).max(1))
            })
            .max()
            .unwrap_or(1)
            .saturating_mul(usize::from(width));
        if columns == 0
            || cells.len() != columns
            || layout.widths.iter().sum::<usize>() + columns * 3 + 1 > usize::from(width)
            || expanded_estimate > 16 * 1024
        {
            for (index, cell) in cells.iter().enumerate() {
                let label = layout
                    .headers
                    .get(index)
                    .map(String::as_str)
                    .unwrap_or("Column");
                let raw = if header {
                    cell.to_owned()
                } else {
                    format!("{label}: {cell}")
                };
                self.ready.push_back(NativeLine {
                    line: Line::from(super::widgets::highlight_inline(&raw, &theme)),
                    indent: 0,
                    continuation: false,
                    code: false,
                    ended: true,
                });
            }
            return;
        }
        let mut wrapped = Vec::new();
        for (index, cell_width) in layout.widths.iter().enumerate() {
            let cell = cells.get(index).map(String::as_str).unwrap_or("");
            let mut line = NativeLine {
                line: Line::from(super::widgets::highlight_inline(cell, &theme)),
                indent: 0,
                continuation: false,
                code: false,
                ended: true,
            };
            if header {
                line.line = line
                    .line
                    .patch_style(ratatui::style::Style::default().add_modifier(Modifier::BOLD));
            }
            let mut output = Vec::new();
            loop {
                let (row, remains) = take_wrapped(&mut line, *cell_width);
                output.push(row);
                if !remains {
                    break;
                }
            }
            wrapped.push(output);
        }
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for row in 0..height {
            let mut spans = vec![Span::raw("│")];
            for (index, cell_rows) in wrapped.iter().enumerate() {
                spans.push(Span::raw(" "));
                let cell = cell_rows.get(row).cloned().unwrap_or_default();
                let padding = layout.widths[index].saturating_sub(cell.width());
                spans.extend(cell.spans);
                spans.push(Span::raw(format!("{} │", " ".repeat(padding))));
            }
            self.ready.push_back(NativeLine {
                line: Line::from(spans),
                indent: 0,
                continuation: false,
                code: true,
                ended: true,
            });
        }
    }

    pub(super) fn pop_row(&mut self, width: u16, max_bytes: usize) -> Option<Line<'static>> {
        let line = self.ready.front_mut()?;
        if !line.ended
            && line.line.width() + usize::from(line.continuation) * line.indent
                <= usize::from(width)
        {
            return None;
        }
        let mut next = line.clone();
        let (row, remains) = take_wrapped(&mut next, usize::from(width));
        if row
            .spans
            .iter()
            .map(|span| span.content.len())
            .sum::<usize>()
            > max_bytes
        {
            return None;
        }
        *line = next;
        if !remains {
            self.ready.pop_front();
        }
        Some(row)
    }
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = trimmed.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    (count >= 3).then_some((marker, count))
}

fn balanced_inline(text: &str) -> bool {
    let mut code = false;
    let mut bold = false;
    let mut italic = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            code = !code;
        } else if ch == '*' && !code {
            if chars.peek() == Some(&'*') {
                chars.next();
                bold = !bold;
            } else {
                italic = !italic;
            }
        }
    }
    !code && !bold && !italic
}

fn hanging_indent(raw: &str) -> usize {
    let leading = raw.len() - raw.trim_start().len();
    let trimmed = raw.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return leading + 2;
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        return leading + digits + 2;
    }
    0
}

fn table_cells(raw: &str) -> Vec<String> {
    let raw = raw.trim().strip_prefix('|').unwrap_or(raw.trim());
    let raw = if raw.ends_with('|') && !raw[..raw.len() - 1].ends_with('\\') {
        &raw[..raw.len() - 1]
    } else {
        raw
    };
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    let mut code = false;
    for ch in raw.chars() {
        if escaped {
            if !matches!(ch, '|' | '\\') {
                cell.push('\\');
            }
            cell.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '`' => {
                code = !code;
                cell.push(ch);
            }
            '|' if !code => {
                cells.push(cell.trim().to_owned());
                cell.clear();
            }
            _ => cell.push(ch),
        }
    }
    if escaped {
        cell.push('\\');
    }
    cells.push(cell.trim().to_owned());
    cells
}

fn take_wrapped(line: &mut NativeLine, width: usize) -> (Line<'static>, bool) {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let indent = if line.continuation {
        line.indent.min(width.saturating_sub(2))
    } else {
        0
    };
    let budget = width.saturating_sub(indent).max(1);
    let mut cells = 0;
    let mut end = 0;
    let mut word_boundary = None;
    let plain = line
        .line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    for (index, grapheme) in plain.grapheme_indices(true) {
        if cells + grapheme.width() > budget {
            break;
        }
        cells += grapheme.width();
        end = index + grapheme.len();
        if grapheme.chars().all(char::is_whitespace) {
            word_boundary = Some(end);
        }
    }
    if end < plain.len()
        && !line.code
        && let Some(boundary) = word_boundary
    {
        end = boundary;
    }
    // The caller rejects graphemes wider than the terminal. Avoid a stuck queue
    // if a table allocates one cell to a wide glyph: let the containing row wrap.
    if end == 0 && !plain.is_empty() {
        end = plain.graphemes(true).next().map_or(0, str::len);
    }
    let mut output = Vec::new();
    if indent > 0 {
        output.push(Span::raw(" ".repeat(indent)));
    }
    let mut remainder = Vec::new();
    let mut offset = 0;
    for span in std::mem::take(&mut line.line.spans) {
        let split = end.saturating_sub(offset).min(span.content.len());
        if split > 0 {
            output.push(Span::styled(span.content[..split].to_owned(), span.style));
        }
        if split < span.content.len() {
            remainder.push(Span::styled(span.content[split..].to_owned(), span.style));
        }
        offset += span.content.len();
    }
    let style = line.line.style;
    line.line = Line::from(remainder).style(style);
    line.continuation = true;
    (Line::from(output).style(style), !line.line.spans.is_empty())
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    fn feed(state: &mut NativeMarkdown, text: &str, width: u16) -> Vec<Line<'static>> {
        let mut output = Vec::new();
        for grapheme in text.graphemes(true) {
            state.push(grapheme, width);
            while let Some(row) = state.pop_row(width, 65_536) {
                output.push(row);
            }
        }
        output
    }
    fn plain(rows: &[Line<'_>]) -> String {
        rows.iter()
            .map(|row| {
                row.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn native_markdown_streams_packed_prose_and_preserves_bold_across_rows() {
        let mut state = NativeMarkdown::default();
        let text = format!(
            "**{}** {}",
            "bold words ".repeat(20),
            "ordinary readable words ".repeat(200)
        );
        let rows = feed(&mut state, &text, 40);
        assert!(
            rows.len() > 100,
            "long paragraphs must publish before newline or completion"
        );
        assert!(rows.iter().any(|row| {
            row.spans
                .iter()
                .any(|span| span.style.add_modifier.contains(Modifier::BOLD))
        }));
        assert!(!plain(&rows).contains("**"));
        for row in &rows {
            assert!(
                row.width() >= 30 && row.width() <= 40,
                "fragmented or overwide row: {row:?}"
            );
        }
        assert!(
            !state.preview(40, 3).is_empty(),
            "unfinished wrapped tail must stay visible"
        );
    }

    #[test]
    fn native_markdown_holds_open_bold_without_publishing_delimiters() {
        let mut state = NativeMarkdown::default();
        let first = feed(
            &mut state,
            "plain words before **unfinished bold words that cross several rows ",
            24,
        );
        assert!(!plain(&first).contains("**"));
        let rest = feed(&mut state, "and close here**.\n", 24);
        assert!(!plain(&rest).contains("**"));
        assert!(
            rest.iter()
                .filter(|row| row
                    .spans
                    .iter()
                    .any(|span| span.style.add_modifier.contains(Modifier::BOLD)))
                .count()
                >= 2
        );
    }

    #[test]
    fn native_markdown_keeps_hanging_list_indentation_and_heading_style() {
        let mut state = NativeMarkdown::default();
        let rows = feed(
            &mut state,
            "## Heading\n- persistent project context remains readable across wrapped lines\n  12. nested ordered list remains readable across wrapped lines\n",
            24,
        );
        assert!(rows[0].style.add_modifier.contains(Modifier::BOLD));
        let text = plain(&rows);
        assert!(text.contains("• persistent project"));
        assert!(text.contains("\n  context"));
        assert!(text.contains("  12. nested ordered"));
        assert!(text.contains("\n      list"));
    }

    #[test]
    fn native_markdown_keeps_fence_context_and_literal_code() {
        let mut state = NativeMarkdown::default();
        let rows = feed(
            &mut state,
            "````rust\n    **literal** `code`\n```\n````\n~~~text\n    next literal\n~~~\n",
            80,
        );
        assert_eq!(
            plain(&rows),
            "    **literal** `code`\n```\n    next literal"
        );
    }

    #[test]
    fn native_markdown_streams_large_tables_with_bounded_retained_state() {
        let mut state = NativeMarkdown::default();
        let header = feed(&mut state, "| **Name** | Value |\n| --- | --- |\n", 48);
        assert!(plain(&header).contains("Name"));
        assert!(!plain(&header).contains("**"));
        for index in 0..2000 {
            let rows = feed(
                &mut state,
                &format!("| entry{index} | {} |\n", "ordinary value ".repeat(5)),
                48,
            );
            assert!(
                plain(&rows).contains(&format!("entry{index}")),
                "closed body row must publish immediately"
            );
            assert!(state.retained_bytes() < 1024);
            assert!(rows.iter().all(|row| row.width() <= 48));
        }
    }

    #[test]
    fn native_markdown_table_preserves_empty_columns_backslashes_and_pipes() {
        assert_eq!(
            table_cells(r"| left | | C:\Users\wilson | `a|b` | x\|y |"),
            ["left", "", r"C:\Users\wilson", "`a|b`", "x|y"]
        );
        let mut state = NativeMarkdown::default();
        feed(&mut state, "| Name | Value |\n| --- | --- |\n", 80);
        let rows = feed(
            &mut state,
            "| alpha | content that remains available after narrowing |\n",
            12,
        );
        assert!(rows.iter().all(|row| row.width() <= 12));
        assert!(plain(&rows).contains("alpha"));
        assert!(plain(&rows).contains("available"));
        assert!(plain(&rows).contains("narrowing"));
    }
}
