//! Segment types and per-type rendering for the conversation widget.
//!
//! Each segment renders as an independent widget with its own Block,
//! background, borders, and internal layout. The ConversationWidget
//! composes these into a scrollable view.

use std::{path::Path, sync::OnceLock};

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};
use tui_syntax_highlight::Highlighter;
use unicode_width::UnicodeWidthStr;

use super::conversation_render_projection::{SegmentRenderMetadata, tool_card_chrome};
use super::theme::Theme;
use crate::surfaces::conversation::{
    AssistantSegment, BorrowedConversationSegmentProjection, ConversationSegmentKind,
    ConversationSegmentProjection, ImageSegment, LifecycleSegment, ProjectConversationSegment,
    SegmentPresentation, SegmentRole, SystemSegment, ToolCategory, ToolSegment, UserSegment,
};

const FILE_URL_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Cached syntax highlighting resources — loaded once, reused forever.
struct SyntaxCache {
    syntax_set: syntect::parsing::SyntaxSet,
    theme: syntect::highlighting::Theme,
}

fn syntax_cache() -> &'static SyntaxCache {
    static CACHE: OnceLock<SyntaxCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let ss = syntect::parsing::SyntaxSet::load_defaults_newlines();
        let ts = syntect::highlighting::ThemeSet::load_defaults();
        let theme = ts.themes["base16-ocean.dark"].clone();
        SyntaxCache {
            syntax_set: ss,
            theme,
        }
    })
}

fn normalize_markdown_for_plaintext(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push(line.to_string());
        } else {
            out.push(line.trim_end().to_string());
        }
    }
    let normalized = out.join("\n");
    normalized.trim_end().to_string()
}

pub(crate) fn split_preserving_trailing_empty_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return vec![""];
    }
    text.split('\n').collect()
}

pub(crate) fn split_trimmed_trailing_empty_lines(text: &str) -> Vec<&str> {
    let mut lines = split_preserving_trailing_empty_lines(text);
    while lines.len() > 1 && lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

pub(crate) fn clean_inline_text(text: &str) -> String {
    strip_terminal_control(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn strip_terminal_control(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    let mut prev = '\0';
                    for next in chars.by_ref() {
                        if next == '\u{7}' || (prev == '\u{1b}' && next == '\\') {
                            break;
                        }
                        prev = next;
                    }
                }
                Some('P' | '^' | '_' | 'X') => {
                    // DCS/PM/APC/SOS strings terminate with ST (ESC \\). If
                    // upstream command output leaks one into a tool card, strip
                    // the whole control string rather than leaving its payload
                    // as printable garbage in the terminal buffer.
                    chars.next();
                    let mut prev = '\0';
                    for next in chars.by_ref() {
                        if prev == '\u{1b}' && next == '\\' {
                            break;
                        }
                        prev = next;
                    }
                }
                Some(next) if ('@'..='_').contains(&next) => {
                    // Single-character C1 escape sequence, e.g. ESC c reset.
                    chars.next();
                }
                _ => {}
            }
            continue;
        }
        if ch.is_control() && ch != '\t' {
            continue;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod terminal_control_tests {
    use super::*;

    #[test]
    fn strip_terminal_control_removes_csi_and_osc() {
        let input = "pre\x1b[31mred\x1b[0m mid\x1b]0;title\x07 post";
        assert_eq!(strip_terminal_control(input), "prered mid post");
    }

    #[test]
    pub(crate) fn clean_inline_text_drops_control_noise() {
        assert_eq!(
            clean_inline_text("nex \x1b[?25lswitch now"),
            "nex switch now"
        );
    }
}

fn first_arg_line(args: &str) -> String {
    clean_inline_text(args.lines().next().unwrap_or(args))
}

fn json_arg(args: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(args).ok()
}

fn json_string_field<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|v| v.as_str()))
}

fn summarize_json_paths(value: &serde_json::Value) -> Option<String> {
    let paths = value.get("paths")?.as_array()?;
    let rendered = paths
        .iter()
        .filter_map(|path| path.as_str())
        .take(3)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if rendered.is_empty() {
        return None;
    }
    let suffix = if paths.len() > rendered.len() {
        format!(" +{} more", paths.len() - rendered.len())
    } else {
        String::new()
    };
    let joined = rendered.join(", ");
    Some(format!("{joined}{suffix}"))
}

pub(crate) fn shell_command_from_args(args: &str) -> Option<String> {
    if let Some(value) = json_arg(args)
        && let Some(command) = json_string_field(&value, &["command", "cmd"])
    {
        return Some(clean_inline_text(command));
    }
    let raw = first_arg_line(args);
    (!raw.is_empty()).then_some(raw)
}

fn summarize_change_args(args: &str) -> Option<String> {
    let v = serde_json::from_str::<serde_json::Value>(args).ok()?;

    if let Some(edits) = v.get("edits").and_then(|e| e.as_array()) {
        let mut files: Vec<&str> = edits
            .iter()
            .filter_map(|edit| edit.get("file").and_then(|f| f.as_str()))
            .collect();
        files.dedup();
        return match files.as_slice() {
            [] => Some(format!("{} edits", edits.len())),
            [only] => Some(format!(
                "{only} · {} edit{}",
                edits.len(),
                if edits.len() == 1 { "" } else { "s" }
            )),
            [first, second, ..] => Some(format!("{first}, {second} · {} edits", edits.len())),
        };
    }

    let path = v
        .get("file")
        .or(v.get("path"))
        .and_then(|f| f.as_str())
        .unwrap_or("(unknown file)");
    let old_len = v
        .get("oldText")
        .and_then(|s| s.as_str())
        .map(|s| s.lines().count())
        .unwrap_or(0);
    let new_len = v
        .get("newText")
        .and_then(|s| s.as_str())
        .map(|s| s.lines().count())
        .unwrap_or(0);
    Some(format!("{path} · {old_len}→{new_len} lines"))
}

pub(crate) fn summarize_tool_args(tool_name: &str, args: Option<&str>) -> Option<String> {
    let args = args?;
    let fallback = || Some(crate::util::truncate(&first_arg_line(args), 96));

    match tool_name {
        "edit" => json_arg(args)
            .map(|v| {
                let path = json_string_field(&v, &["file", "path"]).unwrap_or("(unknown file)");
                let old_len = v
                    .get("oldText")
                    .and_then(|s| s.as_str())
                    .map(|s| s.lines().count())
                    .unwrap_or(0);
                let new_len = v
                    .get("newText")
                    .and_then(|s| s.as_str())
                    .map(|s| s.lines().count())
                    .unwrap_or(0);
                format!("{path} · {old_len}→{new_len} lines")
            })
            .or_else(fallback),
        "change" => summarize_change_args(args).or_else(fallback),
        "bash" => shell_command_from_args(args).map(|cmd| crate::util::truncate(&cmd, 120)),
        "read" | "view" => {
            if let Some(value) = json_arg(args) {
                let path = json_string_field(&value, &["path", "file", "url"])
                    .map(str::to_string)
                    .or_else(|| summarize_json_paths(&value));
                if let Some(path) = path {
                    let mut extras = Vec::new();
                    if let Some(offset) = value.get("offset").and_then(|v| v.as_u64()) {
                        extras.push(format!("@{offset}"));
                    }
                    if let Some(limit) = value.get("limit").and_then(|v| v.as_u64()) {
                        extras.push(format!("limit {limit}"));
                    }
                    return if extras.is_empty() {
                        Some(path)
                    } else {
                        Some(format!("{path} · {}", extras.join(" · ")))
                    };
                }
            }
            fallback()
        }
        "write" => {
            if let Some(value) = json_arg(args)
                && let Some(path) = json_string_field(&value, &["path", "file"])
            {
                let bytes = value
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|content| format!(" · {} bytes", content.len()))
                    .unwrap_or_default();
                return Some(format!("{path}{bytes}"));
            }
            fallback()
        }
        "validate" => {
            if let Some(value) = json_arg(args) {
                if let Some(paths) = summarize_json_paths(&value) {
                    let source_type = json_string_field(&value, &["source_type", "language"])
                        .map(|s| format!(" · {s}"))
                        .unwrap_or_default();
                    return Some(format!("{paths}{source_type}"));
                }
                if let Some(path) = json_string_field(&value, &["path", "file"]) {
                    return Some(path.to_string());
                }
            }
            fallback()
        }
        "wait_for_operator" => {
            if let Some(value) = json_arg(args) {
                let prompt = json_string_field(&value, &["prompt", "message", "reason"])
                    .map(clean_inline_text)
                    .unwrap_or_else(|| "manual confirmation".to_string());
                let timeout = value
                    .get("timeout_secs")
                    .or_else(|| value.get("timeout"))
                    .and_then(|v| v.as_u64())
                    .map(|secs| format!(" · {secs}s timeout"))
                    .unwrap_or_default();
                return Some(format!("{}{timeout}", crate::util::truncate(&prompt, 96)));
            }
            fallback()
        }
        "terminal" => {
            if let Some(value) = json_arg(args) {
                let action = json_string_field(&value, &["action"]).unwrap_or("terminal");
                return match action {
                    "start" => json_string_field(&value, &["command", "cmd"])
                        .map(clean_inline_text)
                        .map(|cmd| format!("start · {}", crate::util::truncate(&cmd, 96)))
                        .or_else(|| Some("start".to_string())),
                    "send" => {
                        let target = json_string_field(&value, &["session_id", "id", "name"])
                            .unwrap_or("(session)");
                        let bytes = value
                            .get("input")
                            .and_then(|v| v.as_str())
                            .map(|input| format!(" · {} bytes", input.len()))
                            .unwrap_or_default();
                        Some(format!("send · {target}{bytes}"))
                    }
                    "read" => {
                        let target = json_string_field(&value, &["session_id", "id", "name"])
                            .unwrap_or("(session)");
                        let max_bytes = value
                            .get("max_bytes")
                            .and_then(|v| v.as_u64())
                            .map(|bytes| format!(" · {bytes} bytes"))
                            .unwrap_or_default();
                        Some(format!("read · {target}{max_bytes}"))
                    }
                    "stop" => {
                        let target = json_string_field(&value, &["session_id", "id", "name"])
                            .unwrap_or("(session)");
                        let force = value
                            .get("force")
                            .and_then(|v| v.as_bool())
                            .is_some_and(|force| force);
                        Some(format!(
                            "stop · {target}{}",
                            if force { " · force" } else { "" }
                        ))
                    }
                    "list" => Some("list sessions".to_string()),
                    other => Some(other.to_string()),
                };
            }
            fallback()
        }
        "plan" => {
            if let Some(value) = json_arg(args)
                && let Some(action) = json_string_field(&value, &["action"])
            {
                let index = value
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|idx| format!(" #{idx}"))
                    .unwrap_or_default();
                return Some(format!("{action}{index}"));
            }
            fallback()
        }
        _ => json_arg(args)
            .and_then(|v| {
                if let Some(paths) = summarize_json_paths(&v) {
                    return Some(paths);
                }
                let obj = v.as_object()?;
                for key in ["path", "file", "command", "query", "name", "key", "url"] {
                    if let Some(value) = obj.get(key) {
                        let rendered = value
                            .as_str()
                            .map(clean_inline_text)
                            .unwrap_or_else(|| clean_inline_text(&value.to_string()));
                        return Some(format!("{key}: {rendered}"));
                    }
                }
                obj.iter().next().map(|(key, value)| {
                    let rendered = value
                        .as_str()
                        .map(clean_inline_text)
                        .unwrap_or_else(|| clean_inline_text(&value.to_string()));
                    format!("{key}: {rendered}")
                })
            })
            .or_else(fallback),
    }
}

fn summarize_tool_result(tool_name: &str, result: Option<&str>) -> Option<String> {
    let result = result?;
    if tool_name == "terminal" {
        let lines = split_trimmed_trailing_empty_lines(result);
        let status = lines
            .iter()
            .find(|line| line.starts_with("Terminal "))
            .map(|line| clean_inline_text(line))
            .unwrap_or_else(|| "terminal".to_string());
        let transcript = lines
            .iter()
            .find_map(|line| line.strip_prefix("Transcript: "))
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let tail = lines
            .iter()
            .rev()
            .map(|line| clean_inline_text(line.trim()))
            .find(|line| {
                !line.is_empty()
                    && !line.starts_with("Terminal ")
                    && !line.starts_with("Transcript:")
            });
        let mut parts = vec![crate::util::truncate(&status, 72)];
        if let Some(tail) = tail {
            parts.push(crate::util::truncate(&tail, 48));
        }
        if let Some(transcript) = transcript {
            parts.push(crate::util::truncate(transcript, 48));
        }
        return Some(parts.join(" · "));
    }

    let lines = split_trimmed_trailing_empty_lines(result);
    let line_count = if result.is_empty() { 0 } else { lines.len() };
    let first_non_empty = lines
        .iter()
        .map(|line| clean_inline_text(line.trim()))
        .find(|line| !line.is_empty());

    match (line_count, first_non_empty) {
        (0, _) => Some("ok".to_string()),
        (1, Some(line)) => Some(crate::util::truncate(&line, 96)),
        (count, Some(line)) if matches!(tool_name, "read" | "view") => Some(format!(
            "{count} lines · {}",
            crate::util::truncate(&line, 72)
        )),
        (count, Some(line)) => Some(format!(
            "{count} lines · {}",
            crate::util::truncate(&line, 72)
        )),
        (count, None) if count > 0 => Some(format!("{count} blank line(s)")),
        _ => Some("ok".to_string()),
    }
}

fn summarize_live_tool_progress(
    live_partial: Option<&omegon_traits::PartialToolResult>,
    started_at: Option<std::time::Instant>,
) -> String {
    let mut parts = Vec::new();
    let phase = live_partial
        .and_then(|partial| partial.progress.phase.as_deref())
        .unwrap_or("running");
    parts.push(phase.to_string());

    if let Some(partial) = live_partial {
        if let Some(units) = &partial.progress.units {
            let label = match units.total {
                Some(total) => format!("{}/{} {}", units.current, total, units.unit),
                None => format!("{} {}", units.current, units.unit),
            };
            parts.push(label);
        }
        if partial.progress.heartbeat {
            parts.push("idle".to_string());
        }
    }

    let elapsed_ms = started_at
        .map(|started| started.elapsed().as_millis() as u64)
        .or_else(|| live_partial.map(|partial| partial.progress.elapsed_ms))
        .filter(|ms| *ms > 0);
    if let Some(ms) = elapsed_ms {
        parts.push(format_duration_compact(ms));
    }

    if let Some(partial) = live_partial
        && !partial.tail.is_empty()
        && let Some(line) = partial
            .tail
            .lines()
            .rev()
            .map(|line| clean_inline_text(line.trim()))
            .find(|line| !line.is_empty())
    {
        parts.push(crate::util::truncate(&line, 72));
    }

    parts.join(" · ")
}

fn tool_has_expandable_detail(
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    live_partial: Option<&omegon_traits::PartialToolResult>,
) -> bool {
    detail_args.is_some_and(|args| !args.trim().is_empty())
        || detail_result.is_some_and(|result| !result.trim().is_empty())
        || live_partial.is_some_and(|partial| !partial.tail.trim().is_empty())
}

fn slim_tool_summary_cells(
    name: &str,
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    complete: bool,
    live_partial: Option<&omegon_traits::PartialToolResult>,
    started_at: Option<std::time::Instant>,
    duration_ms: Option<u64>,
) -> Vec<String> {
    let mut cells = Vec::new();
    if let Some(summary) = summarize_tool_args(name, detail_args) {
        cells.push(summary);
    }
    if complete {
        if let Some(summary) = summarize_tool_result(name, detail_result) {
            cells.push(summary);
        }
        if let Some(ms) = duration_ms {
            cells.push(format_duration_compact(ms));
        }
    } else {
        cells.push(summarize_live_tool_progress(live_partial, started_at));
    }
    if tool_has_expandable_detail(detail_args, detail_result, live_partial) {
        cells.push("Ctrl+O details".to_string());
    }
    cells
}

fn slim_tool_overflow_hint(hidden_count: usize, hidden_cells: &[&String]) -> String {
    let has_expandable_hidden_cell = hidden_cells
        .iter()
        .any(|cell| cell.contains("Ctrl+O details"));
    if has_expandable_hidden_cell {
        format!("+{hidden_count} more · Ctrl+O details")
    } else {
        format!("+{hidden_count} more")
    }
}

fn slim_tool_detail_lines(width: u16, cells: &[String]) -> Vec<String> {
    if cells.is_empty() {
        return vec![String::new()];
    }

    let one_line_budget = width.saturating_sub(16) as usize;
    let joined = cells.join(" · ");
    if UnicodeWidthStr::width(joined.as_str()) <= one_line_budget {
        return vec![crate::util::truncate(&joined, one_line_budget)];
    }

    let row_budget = width.saturating_sub(16) as usize;
    let max_rows = 4usize;
    let mut rows = Vec::new();
    rows.push(crate::util::truncate(&cells[0], row_budget));

    let remaining = &cells[1..];
    for (idx, cell) in remaining
        .iter()
        .take(max_rows.saturating_sub(1))
        .enumerate()
    {
        let is_last_visible = idx + 1 == remaining.len().min(max_rows.saturating_sub(1));
        let marker = if is_last_visible { "  └ " } else { "  ├ " };
        rows.push(format!(
            "{marker}{}",
            crate::util::truncate(cell, row_budget.saturating_sub(marker.len()))
        ));
    }

    if remaining.len() > max_rows.saturating_sub(1)
        && let Some(last) = rows.last_mut()
    {
        let hidden_start = max_rows.saturating_sub(1);
        let hidden_cells = remaining[hidden_start..].iter().collect::<Vec<_>>();
        *last = format!(
            "  └ {}",
            slim_tool_overflow_hint(hidden_cells.len(), &hidden_cells)
        );
    }

    rows
}

fn slim_tool_collapsed_line(width: u16, cells: &[String]) -> String {
    let budget = width.saturating_sub(16) as usize;
    if cells.is_empty() {
        String::new()
    } else {
        crate::util::truncate(&cells.join(" · "), budget)
    }
}

fn slim_tool_live_rows(width: u16, cells: &[String]) -> Vec<String> {
    if cells.is_empty() {
        return vec![String::new()];
    }

    let row_budget = width.saturating_sub(8) as usize;
    let mut rows = Vec::new();
    let progress_idx = cells
        .iter()
        .position(|cell| cell.contains("running") || cell.contains("idle") || cell.contains('%'))
        .unwrap_or_else(|| cells.len().saturating_sub(1));
    rows.push(crate::util::truncate(&cells[progress_idx], row_budget));

    let detail_cells = cells
        .iter()
        .enumerate()
        .filter_map(|(idx, cell)| (idx != progress_idx).then_some(cell))
        .collect::<Vec<_>>();
    let max_detail_rows = 4usize;
    for (idx, cell) in detail_cells.iter().take(max_detail_rows).enumerate() {
        let is_last_visible = idx + 1 == detail_cells.len().min(max_detail_rows);
        let marker = if is_last_visible {
            "    └ "
        } else {
            "    ├ "
        };
        rows.push(format!(
            "{marker}{}",
            crate::util::truncate(cell, row_budget.saturating_sub(marker.len()))
        ));
    }

    if detail_cells.len() > max_detail_rows
        && let Some(last) = rows.last_mut()
    {
        let hidden_cells = detail_cells[max_detail_rows..].to_vec();
        *last = format!(
            "    └ {}",
            slim_tool_overflow_hint(hidden_cells.len(), &hidden_cells)
        );
    }

    rows
}

pub(crate) fn subtle_tool_row_bg(bg: Color) -> Color {
    match bg {
        Color::Rgb(r, g, b) => Color::Rgb(
            r.saturating_add(3),
            g.saturating_add(5),
            b.saturating_add(8),
        ),
        other => other,
    }
}

pub(crate) fn apply_rows_bg(
    area: Rect,
    start_row: u16,
    row_count: u16,
    bg: Color,
    buf: &mut Buffer,
) {
    let end_row = start_row.saturating_add(row_count).min(area.height);
    for row in start_row..end_row {
        let y = area.y + row;
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_bg(bg);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedLink {
    start_col: u16,
    label: String,
    url: String,
}

fn collect_rendered_links(line: &Line<'_>) -> Vec<RenderedLink> {
    let text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    detect_links(&text)
}

fn detect_links(text: &str) -> Vec<RenderedLink> {
    const SCHEMES: [&str; 3] = ["https://", "http://", "file://"];

    let mut links = Vec::new();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let rest = &text[cursor..];
        let Some((rel_start, scheme)) = SCHEMES
            .iter()
            .filter_map(|scheme| rest.find(scheme).map(|idx| (idx, *scheme)))
            .min_by_key(|(idx, _)| *idx)
        else {
            break;
        };

        let start = cursor + rel_start;
        let after_scheme = start + scheme.len();
        let mut end = text.len();
        for (idx, ch) in text[after_scheme..].char_indices() {
            if ch.is_whitespace() || ch.is_control() || matches!(ch, '<' | '>' | '"' | '\'') {
                end = after_scheme + idx;
                break;
            }
        }
        while end > start {
            let Some(ch) = text[..end].chars().next_back() else {
                break;
            };
            if matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']') {
                end -= ch.len_utf8();
            } else {
                break;
            }
        }

        if end > after_scheme {
            let label = text[start..end].to_string();
            let start_col = UnicodeWidthStr::width(&text[..start]) as u16;
            links.push(RenderedLink {
                start_col,
                url: label.clone(),
                label,
            });
        }
        cursor = end.max(after_scheme);
    }

    links
}

pub(crate) fn apply_rendered_links(
    area: Rect,
    lines: &[Line<'_>],
    buf: &mut Buffer,
    style: Style,
    max_rows: u16,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut visual_row = 0u16;
    for line in lines {
        if visual_row >= area.height || visual_row >= max_rows {
            break;
        }

        let line_width = line.width() as u16;
        if line_width <= area.width {
            for link in collect_rendered_links(line) {
                if link.start_col >= area.width {
                    continue;
                }
                let width = area.width.saturating_sub(link.start_col);
                if width == 0 {
                    continue;
                }
                let label_width = UnicodeWidthStr::width(link.label.as_str()) as u16;
                let width = width.min(label_width.max(1));
                let link_area = Rect {
                    x: area.x + link.start_col,
                    y: area.y + visual_row,
                    width,
                    height: 1,
                };
                hyperrat::Link::new(link.label, link.url)
                    .style(style)
                    .render(link_area, buf);
            }
        }

        visual_row = visual_row.saturating_add(line_width.max(1).div_ceil(area.width));
    }
}

pub(crate) fn file_url_for_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return None;
    }
    if trimmed.starts_with("file://") {
        return Some(trimmed.to_string());
    }

    let path = Path::new(trimmed);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let encoded =
        percent_encoding::utf8_percent_encode(&absolute.to_string_lossy(), FILE_URL_ENCODE_SET)
            .to_string();
    Some(format!("file://{encoded}"))
}

// ═══════════════════════════════════════════════════════════════════════════
// Segment — rich metadata wrapper + typed content
// ═══════════════════════════════════════════════════════════════════════════

/// Provider-reported actual token counts for the turn that produced
/// (or contains) a given segment. Stamped onto `SegmentMeta` after a
/// `TurnEnd` event arrives, by walking back through segments whose
/// `turn` matches the just-ended turn id. Renderers display this next
/// to the timestamp on segments that involved an LLM call so the
/// timeline carries token cost as canon — operators don't have to
/// glance at the inference panel to see what each turn's segments
/// actually cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

impl TokenUsage {
    /// Render as a compact title-bar annotation: `↑1.2k ↓340`. Numbers
    /// > 1000 are shortened with a `k` suffix; smaller numbers render
    /// > as-is. The arrows are non-emoji single-cell glyphs (the same
    /// > constraint as the instruments-panel pass).
    pub fn format_compact(&self) -> String {
        format!(
            "↑{} ↓{}",
            format_token_count(self.input),
            format_token_count(self.output)
        )
    }
}

fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Compact duration: "0.3s", "4.2s", "1m12s", "3m".
pub fn format_duration_compact(ms: u64) -> String {
    let secs = ms / 1000;
    if secs == 0 {
        let tenths = (ms % 1000) / 100;
        format!("0.{tenths}s")
    } else if secs < 60 {
        let tenths = (ms % 1000) / 100;
        format!("{secs}.{tenths}s")
    } else {
        let mins = secs / 60;
        let rem = secs % 60;
        if rem == 0 {
            format!("{mins}m")
        } else {
            format!("{mins}m{rem:02}s")
        }
    }
}

/// Metadata captured at segment creation time. Every segment carries this
/// regardless of type. Fields are Optional — populated when available,
/// never blocking construction.
#[derive(Debug, Clone, Default)]
pub struct SegmentMeta {
    /// Wall-clock time this segment was created.
    pub timestamp: Option<std::time::SystemTime>,
    /// Provider that generated this content (e.g. "anthropic", "ollama").
    pub provider: Option<String>,
    /// Model ID at generation time (e.g. "claude-sonnet-4-20250514").
    pub model_id: Option<String>,
    /// Capability tier at generation time (e.g. "frontier").
    pub tier: Option<String>,
    /// Thinking level active at generation time (e.g. "medium", "high").
    pub thinking_level: Option<String>,
    /// Turn number within the session (1-indexed).
    pub turn: Option<u32>,
    /// Estimated token cost of this segment (input + output).
    pub est_tokens: Option<u32>,
    /// Provider-reported actual tokens for the turn this segment
    /// belongs to. Stamped after `TurnEnd` arrives with the real
    /// counts; `None` until then. Different from `est_tokens` (the
    /// local heuristic) — `actual_tokens` reflects the provider's
    /// authoritative billing numbers and is what the title-bar
    /// annotation displays.
    pub actual_tokens: Option<TokenUsage>,
    /// Context window fill percentage at time of generation.
    pub context_percent: Option<f32>,
    /// Active persona ID, if any.
    pub persona: Option<String>,
    /// Git branch at time of generation.
    pub branch: Option<String>,
    /// Duration of the operation (for tool calls: execution time).
    pub duration_ms: Option<u64>,
    /// Source channel for externally-originated prompts, e.g. voice.
    pub source_channel: Option<String>,
    /// Voice radio cue metadata, e.g. over or over_and_out.
    pub radio_cue: Option<String>,
    /// Whether the voice extension marked the utterance as end-of-turn.
    pub voice_end_of_turn: Option<bool>,
    /// Whether the voice extension requested microphone/session closure.
    pub voice_close_session_requested: Option<bool>,
    /// Voice utterance duration in seconds.
    pub voice_duration_s: Option<f64>,
}

/// A segment in the conversation — metadata wrapper + typed content.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Rich metadata captured at creation time.
    pub meta: SegmentMeta,
    /// The typed content of this segment.
    pub content: SegmentContent,
}

/// Clipboard/export formatting mode for segment content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentExportMode {
    Raw,
    Plaintext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentRenderMode {
    #[default]
    Full,
    Slim,
}

/// The typed content of a conversation segment.
#[derive(Debug, Clone)]
pub enum SegmentContent {
    /// User's input prompt.
    UserPrompt { text: String },

    /// Assistant's response (may be streaming).
    AssistantText {
        text: String,
        thinking: String,
        complete: bool,
    },

    /// Tool call with args and result.
    ToolCard {
        id: String,
        name: String,
        args_summary: Option<String>,
        detail_args: Option<String>,
        result_summary: Option<String>,
        detail_result: Option<String>,
        is_error: bool,
        complete: bool,
        /// When true, show full result instead of truncated preview.
        expanded: bool,
        /// Most recent partial result received from the runner while the
        /// tool is still in flight. Populated by `ToolUpdate` events,
        /// rendered inside the card body until `ToolEnd` flips
        /// `complete` to true. `None` for tools that don't stream or
        /// before the first partial arrives.
        live_partial: Option<Box<omegon_traits::PartialToolResult>>,
        /// Wall-clock instant captured when the tool card was created
        /// (i.e. when `ToolStart` arrived). The renderer prefers this
        /// over `live_partial.progress.elapsed_ms` for the displayed
        /// timer because it ticks with every frame draw — the partial's
        /// elapsed is captured at flush time and freezes between
        /// partials, which looks broken to an operator watching a
        /// long-running tool. `None` for legacy fixtures that don't
        /// set it; the renderer falls back to the partial's value in
        /// that case.
        started_at: Option<std::time::Instant>,
    },

    /// System notification (slash command response, info message).
    SystemNotification { text: String },

    /// Lifecycle event (phase change, decomposition).
    LifecycleEvent { icon: String, text: String },

    /// Inline image from a tool result.
    Image {
        path: std::path::PathBuf,
        /// Alt text shown when image can't be rendered.
        alt: String,
    },

    /// Visual separator between turns.
    TurnSeparator,
}

pub(crate) fn is_plan_progress_text(text: &str) -> bool {
    matches!(
        text.lines().next().unwrap_or_default(),
        "Plan set"
            | "Plan progress"
            | "Plan item skipped"
            | "Plan approved"
            | "Plan executing"
            | "Plan cleared"
            | "Plan status"
            | "Plan updated"
    )
}

/// Convenience constructors — build Segment with default (empty) metadata.
/// Call sites that have model info should set meta fields after construction.
impl Segment {
    pub fn user_prompt(text: impl Into<String>) -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::UserPrompt { text: text.into() },
        }
    }
    pub fn assistant_text() -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: String::new(),
                thinking: String::new(),
                complete: false,
            },
        }
    }
    pub fn tool_card(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: id.into(),
                name: name.into(),
                args_summary: None,
                detail_args: None,
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: None,
                started_at: Some(std::time::Instant::now()),
            },
        }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::SystemNotification { text: text.into() },
        }
    }
    pub fn lifecycle(icon: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::LifecycleEvent {
                icon: icon.into(),
                text: text.into(),
            },
        }
    }
    pub fn image(path: std::path::PathBuf, alt: impl Into<String>) -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::Image {
                path,
                alt: alt.into(),
            },
        }
    }
    pub fn separator() -> Self {
        Self {
            meta: SegmentMeta::default(),
            content: SegmentContent::TurnSeparator,
        }
    }
}

impl<'a> ProjectConversationSegment<'a> for Segment {
    type Text = &'a str;
    type Path = &'a std::path::Path;

    fn project_conversation_segment(&'a self) -> BorrowedConversationSegmentProjection<'a> {
        let kind = match &self.content {
            SegmentContent::UserPrompt { text } => ConversationSegmentKind::User(UserSegment {
                text: text.as_str(),
            }),
            SegmentContent::AssistantText {
                text,
                thinking,
                complete,
            } => ConversationSegmentKind::Assistant(AssistantSegment {
                text: text.as_str(),
                thinking: thinking.as_str(),
                complete: *complete,
            }),
            SegmentContent::ToolCard {
                id,
                name,
                args_summary,
                detail_args,
                result_summary,
                detail_result,
                is_error,
                complete,
                expanded,
                ..
            } => ConversationSegmentKind::Tool(ToolSegment {
                id: id.as_str(),
                name: name.as_str(),
                args_summary: args_summary.as_deref(),
                detail_args: detail_args.as_deref(),
                result_summary: result_summary.as_deref(),
                detail_result: detail_result.as_deref(),
                is_error: *is_error,
                complete: *complete,
                expanded: *expanded,
            }),
            SegmentContent::SystemNotification { text } => {
                ConversationSegmentKind::System(SystemSegment {
                    text: text.as_str(),
                })
            }
            SegmentContent::LifecycleEvent { icon, text } => {
                ConversationSegmentKind::Lifecycle(LifecycleSegment {
                    icon: icon.as_str(),
                    text: text.as_str(),
                })
            }
            SegmentContent::Image { path, alt } => ConversationSegmentKind::Image(ImageSegment {
                path: path.as_path(),
                alt: alt.as_str(),
            }),
            SegmentContent::TurnSeparator => ConversationSegmentKind::Separator,
        };
        ConversationSegmentProjection::new(kind)
    }
}

impl SegmentRenderMetadata for Segment {
    fn is_live_render_segment(&self) -> bool {
        matches!(
            self.content,
            SegmentContent::AssistantText {
                complete: false,
                ..
            } | SegmentContent::ToolCard {
                complete: false,
                ..
            }
        )
    }

    fn is_image_render_segment(&self) -> bool {
        matches!(self.content, SegmentContent::Image { .. })
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Rendering — each segment type knows how to render into a Rect
// ═══════════════════════════════════════════════════════════════════════════

impl Segment {
    pub fn plain_text(&self) -> String {
        self.export_text(SegmentExportMode::Raw)
    }

    pub fn export_text(&self, mode: SegmentExportMode) -> String {
        match &self.content {
            SegmentContent::UserPrompt { text } => text.clone(),
            SegmentContent::AssistantText { text, thinking, .. } => {
                let thinking = match mode {
                    SegmentExportMode::Raw => thinking.trim_end().to_string(),
                    SegmentExportMode::Plaintext => normalize_markdown_for_plaintext(thinking),
                };
                let text = match mode {
                    SegmentExportMode::Raw => text.trim_end().to_string(),
                    SegmentExportMode::Plaintext => normalize_markdown_for_plaintext(text),
                };

                if thinking.trim().is_empty() {
                    text
                } else if text.trim().is_empty() {
                    format!("[thinking]\n{thinking}")
                } else {
                    format!("[thinking]\n{thinking}\n\n[text]\n{text}")
                }
            }
            SegmentContent::ToolCard {
                name,
                detail_args,
                detail_result,
                is_error,
                complete,
                ..
            } => {
                let mut lines = vec![format!("tool: {name}")];
                if !complete {
                    lines.push("status: running".to_string());
                } else if *is_error {
                    lines.push("status: error".to_string());
                } else {
                    lines.push("status: complete".to_string());
                }
                if let Some(args) = detail_args.as_deref()
                    && !args.trim().is_empty()
                {
                    lines.push(String::new());
                    lines.push("args:".to_string());
                    lines.push(args.trim_end().to_string());
                }
                if let Some(result) = detail_result.as_deref()
                    && !result.trim().is_empty()
                {
                    lines.push(String::new());
                    lines.push("result:".to_string());
                    lines.push(result.trim_end().to_string());
                }
                lines.join("\n")
            }
            SegmentContent::SystemNotification { text } => text.clone(),
            SegmentContent::LifecycleEvent { icon, text } => format!("{icon} {text}"),
            SegmentContent::Image { path, alt } => {
                let mut lines = vec![format!("image: {}", path.display())];
                if !alt.trim().is_empty() {
                    lines.push(format!("alt: {alt}"));
                }
                lines.join("\n")
            }
            SegmentContent::TurnSeparator => "───".to_string(),
        }
    }

    pub fn projection(&self) -> BorrowedConversationSegmentProjection<'_> {
        self.project_conversation_segment()
    }

    pub fn role(&self) -> SegmentRole {
        self.projection().role()
    }

    pub fn presentation(&self) -> SegmentPresentation {
        self.projection().presentation
    }

    /// Render this segment into the given area of the buffer.
    pub fn render(
        &self,
        area: Rect,
        buf: &mut Buffer,
        t: &dyn Theme,
        mode: SegmentRenderMode,
        density: crate::settings::ToolDetail,
    ) {
        self.render_with_pinned(area, buf, t, mode, density, false);
    }

    pub fn render_with_pinned(
        &self,
        area: Rect,
        buf: &mut Buffer,
        t: &dyn Theme,
        mode: SegmentRenderMode,
        density: crate::settings::ToolDetail,
        pinned: bool,
    ) {
        use SegmentContent::*;
        let presentation = self.presentation();
        match &self.content {
            UserPrompt { text } => super::segment_components::user_prompt::render(
                super::segment_components::user_prompt::UserPromptRenderProps {
                    text,
                    presentation: &presentation,
                    meta: &self.meta,
                    mode,
                },
                area,
                buf,
                t,
            ),
            AssistantText {
                text,
                thinking,
                complete,
            } => {
                super::segment_components::assistant::render(
                    super::segment_components::assistant::AssistantRenderProps {
                        text,
                        thinking,
                        complete: *complete,
                        meta: &self.meta,
                        presentation: &presentation,
                        mode,
                    },
                    area,
                    buf,
                    t,
                );
            }
            ToolCard {
                name,
                detail_args,
                detail_result,
                is_error,
                complete,
                expanded,
                live_partial,
                started_at,
                ..
            } => {
                super::segment_components::tool_card::render(
                    super::segment_components::tool_card::ToolCardRenderProps {
                        name,
                        detail_args: detail_args.as_deref(),
                        detail_result: detail_result.as_deref(),
                        is_error: *is_error,
                        complete: *complete,
                        expanded: *expanded,
                        live_partial: live_partial.as_deref(),
                        started_at: *started_at,
                        meta: &self.meta,
                        tool_category: presentation.tool_category,
                        mode,
                        density,
                        pinned,
                    },
                    area,
                    buf,
                    t,
                );
            }
            SystemNotification { text } => super::segment_components::system::render(
                super::segment_components::system::SystemRenderProps { text, mode },
                area,
                buf,
                t,
            ),
            LifecycleEvent { icon, text } => super::segment_components::lifecycle::render(
                super::segment_components::lifecycle::LifecycleRenderProps { icon, text },
                area,
                buf,
                t,
            ),
            Image { path, alt } => super::segment_components::image::render(
                super::segment_components::image::ImageRenderProps { path, alt },
                area,
                buf,
                t,
            ),
            TurnSeparator => super::segment_components::separator::render(area, buf, t),
        }
    }

    /// Calculate the height this segment needs at the given width.
    /// Renders into a temp buffer to get the exact height — matches
    /// Paragraph's word-aware wrapping precisely.
    pub fn height(&self, width: u16, t: &dyn Theme) -> u16 {
        self.height_in_mode(width, t, SegmentRenderMode::Full)
    }

    pub fn height_in_mode(&self, width: u16, t: &dyn Theme, mode: SegmentRenderMode) -> u16 {
        if width == 0 {
            return 1;
        }
        use SegmentContent::*;

        // Quick paths for fixed-height types
        match &self.content {
            TurnSeparator => return 1,
            LifecycleEvent { .. } => return 1,
            Image { .. } => return 14, // Fixed: 12 rows image + 1 caption + 1 spacing
            _ => {}
        }

        // Estimate max height for the temp buffer using WRAPPED visual rows,
        // not just logical newline counts. If we underestimate here, the temp
        // buffer clips content and the cached height becomes permanently wrong.
        let estimate = match &self.content {
            UserPrompt { text } => wrapped_rows(text, width.saturating_sub(4)) + 2,
            AssistantText { text, thinking, .. } if matches!(mode, SegmentRenderMode::Slim) => {
                let thinking_rows = if thinking.is_empty() { 0 } else { 1 };
                let text_rows = if text.is_empty() {
                    0
                } else {
                    wrapped_rows(text, width).max(1)
                };
                (text_rows + thinking_rows).max(1)
            }
            AssistantText { text, thinking, .. } => {
                let meta_line = if self.meta.model_id.is_some() || self.meta.provider.is_some() {
                    1u16
                } else {
                    0
                };
                let thinking_rows = if thinking.is_empty() {
                    0
                } else {
                    wrapped_rows(thinking, width.saturating_sub(5)).min(8) + 2
                };
                wrapped_rows(text, width.saturating_sub(3)) + thinking_rows + 4 + meta_line
            }
            ToolCard {
                name,
                detail_args,
                detail_result,
                is_error,
                expanded,
                complete,
                live_partial,
                started_at,
                ..
            } if matches!(mode, SegmentRenderMode::Slim) && !*expanded => {
                let cells = slim_tool_summary_cells(
                    name,
                    detail_args.as_deref(),
                    detail_result.as_deref(),
                    *complete,
                    live_partial.as_deref(),
                    *started_at,
                    self.meta.duration_ms,
                );
                if *complete {
                    1
                } else {
                    slim_tool_live_rows(width, &cells).len().max(1) as u16
                }
            }
            ToolCard {
                name,
                detail_args,
                detail_result,
                expanded,
                complete,
                live_partial,
                ..
            } => {
                let inner_width = width.saturating_sub(4).max(1);
                let compact_arg_rows = match name.as_str() {
                    "bash" => detail_args
                        .as_ref()
                        .map(|a| a.lines().take(4).count() as u16)
                        .unwrap_or(0),
                    "edit" | "change" | "read" | "write" | "view" => {
                        u16::from(detail_args.is_some())
                    }
                    _ => detail_args
                        .as_ref()
                        .map(|a| wrapped_rows(a, inner_width).min(if *expanded { 80 } else { 4 }))
                        .unwrap_or(0),
                };
                let compact_result_rows = detail_result
                    .as_ref()
                    .map(|r| wrapped_rows(r, inner_width).min(if *expanded { 220 } else { 12 }))
                    .unwrap_or(0);
                // Diff section rows: edit/change tools render a real
                // colored diff in place of the boring "Successfully
                // replaced" result text. The estimate is the sum of
                // (old + new) lines per block plus chrome (summary +
                // optional file headers + truncation marker), capped
                // at the same collapsed/expanded budget as the result
                // section. The actual rendering is bounded by
                // `max_diff_lines` (8 collapsed, 200 expanded).
                let compact_diff_rows: u16 = if matches!(name.as_str(), "edit" | "change") {
                    detail_args
                        .as_deref()
                        .and_then(|args| build_edit_diff_blocks(name, args))
                        .map(|blocks| {
                            let multi = blocks.len() > 1;
                            let total: usize = blocks
                                .iter()
                                .map(|b| {
                                    let header = if multi { 1 } else { 0 };
                                    header + b.old_text.lines().count() + b.new_text.lines().count()
                                })
                                .sum();
                            // +1 summary line, +1 truncation marker (worst case)
                            let with_chrome = total + 2;
                            with_chrome.min(if *expanded { 200 } else { 8 }) as u16
                        })
                        .unwrap_or(0)
                } else {
                    0
                };
                // Live section rows: only relevant while the tool is
                // still in flight. Always at least one row (the status
                // header) when incomplete; tail rows on top when a
                // partial with content has arrived.
                let compact_live_rows: u16 = if !*complete {
                    let header = 1u16;
                    let tail = live_partial
                        .as_ref()
                        .map(|p| {
                            let lines = p.tail.lines().count() as u16;
                            lines.min(if *expanded { 50 } else { 12 })
                        })
                        .unwrap_or(0);
                    header + tail
                } else {
                    0
                };
                let live_separator_rows = u16::from(compact_arg_rows > 0 && compact_live_rows > 0);
                // The diff section replaces the result section when
                // present, so we use whichever is larger to over-
                // estimate (under-estimating clips content; over-
                // estimating just allocates a slightly larger temp
                // buffer that the `last_used` scan will trim).
                let body_rows = compact_diff_rows.max(compact_result_rows);
                let result_separator_rows = u16::from(compact_arg_rows > 0 && body_rows > 0);
                compact_arg_rows
                    + compact_live_rows
                    + live_separator_rows
                    + body_rows
                    + result_separator_rows
                    + 4
            }
            SystemNotification { text } if matches!(mode, SegmentRenderMode::Slim) => {
                if is_plan_progress_text(text) {
                    0
                } else {
                    wrapped_rows(text, width).max(1)
                }
            }
            SystemNotification { text } => wrapped_rows(text, width.saturating_sub(4)) + 2,
            _ => 4,
        };

        // Render into a temp buffer and scan actual used rows. Assistant responses from
        // high-verbosity models can legitimately exceed a few hundred wrapped rows;
        // capping them at 400 made the measured height too short and clipped the tail
        // in the conversation pane. Keep a high safety cap to avoid absurd allocations
        // while preserving normal long-form answers.
        const MAX_MEASURE_ROWS: u16 = 4000;
        let h = match (&self.content, mode) {
            // Assistant markdown rendering performs structural transforms (code fences,
            // tables, inline highlighting) before Ratatui wraps the final Line values.
            // The raw-text estimate above can be too small for narrow viewports; if the
            // temporary buffer clips, the last-used-row scan records a permanently short
            // height and the conversation tail appears truncated behind the composer.
            // Add slack only for structurally-marked markdown and let the scan trim
            // unused rows. Plain short prose should keep the old tight estimate.
            (AssistantText { text, thinking, .. }, SegmentRenderMode::Slim) => estimate
                .saturating_add(assistant_measurement_slack(text, thinking))
                .min(MAX_MEASURE_ROWS),
            (AssistantText { text, thinking, .. }, _) => estimate
                .saturating_add(assistant_measurement_slack(text, thinking))
                .clamp(4, MAX_MEASURE_ROWS),
            (_, SegmentRenderMode::Slim) => estimate.min(400),
            _ => estimate.clamp(4, 400),
        };
        if h == 0 {
            return 0;
        }
        let temp_area = Rect::new(0, 0, width, h);
        let mut temp_buf = Buffer::empty(temp_area);
        self.render(
            temp_area,
            &mut temp_buf,
            t,
            mode,
            crate::settings::ToolDetail::Detailed,
        );

        // Find the last row with actual text content.
        // Skip border characters (│╰╯┐┘├┤┌└) in the first/last 2 columns
        // and background-only cells. Only count rows with real text INSIDE
        // the card borders.
        let mut last_used: u16 = 0;
        let _border_chars: &[char] = &[
            '│', '─', '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼',
        ];
        for y in (0..h).rev() {
            let mut has_content = false;
            // Check interior columns only (skip first 2 and last 2 for borders + padding)
            let x_start = if matches!(mode, SegmentRenderMode::Slim) {
                0
            } else {
                2.min(width)
            };
            let x_end = if matches!(mode, SegmentRenderMode::Slim) {
                width
            } else {
                width.saturating_sub(2).max(x_start)
            };
            for x in x_start..x_end {
                let cell = &temp_buf[(x, y)];
                let sym = cell.symbol();
                if sym != " " && !sym.is_empty() {
                    has_content = true;
                    break;
                }
            }
            if has_content {
                last_used = y + 1;
                break;
            }
        }

        (last_used).max(1)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-type renderers
// ═══════════════════════════════════════════════════════════════════════════

/// Scale an RGB color's intensity by `factor` (0.0–1.0). Non-RGB colors pass through.
pub(crate) fn dim_color(color: Color, factor: f32) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(
            (r as f32 * factor) as u8,
            (g as f32 * factor) as u8,
            (b as f32 * factor) as u8,
        ),
        other => other,
    }
}

fn assistant_measurement_slack(text: &str, thinking: &str) -> u16 {
    let structural = text.contains("```")
        || thinking.contains("```")
        || text.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with('#')
                || trimmed.starts_with('-')
                || trimmed.starts_with('*')
                || trimmed.starts_with('>')
                || trimmed.contains("| ")
        });
    if structural { 12 } else { 0 }
}

fn wrapped_rows(text: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    text.lines()
        .map(|line| UnicodeWidthStr::width(line).max(1).div_ceil(width) as u16)
        .sum::<u16>()
        .max(1)
}

/// Build a compact meta tag string from SegmentMeta for display in the response header.
/// Example: "claude-sonnet-4-6 · anthropic · victory · think:medium · ctx:34%"
pub fn build_meta_tag(meta: &SegmentMeta) -> String {
    let mut parts = Vec::new();
    if let Some(ref m) = meta.model_id {
        // Trim provider prefix if present (e.g. "anthropic:claude-..." → "claude-...")
        let short = m.split(':').next_back().unwrap_or(m);
        parts.push(short.to_string());
    }
    if let Some(ref p) = meta.provider {
        parts.push(p.clone());
    }
    if let Some(ref tier) = meta.tier {
        parts.push(tier.clone());
    }
    if let Some(ref tl) = meta.thinking_level
        && tl != "off"
    {
        parts.push(format!("think:{tl}"));
    }
    if let Some(ref persona) = meta.persona {
        parts.push(format!("⌘ {persona}"));
    }
    if let Some(ref channel) = meta.source_channel {
        parts.push(format!("source:{channel}"));
    }
    if let Some(ref cue) = meta.radio_cue {
        parts.push(format!("cue:{cue}"));
    }
    if meta.voice_close_session_requested == Some(true) {
        parts.push("close-session".to_string());
    }
    if let Some(duration) = meta.voice_duration_s {
        parts.push(format!("voice:{duration:.1}s"));
    }
    if let Some(ctx) = meta.context_percent.filter(|p| *p > 5.0) {
        parts.push(format!("ctx:{ctx:.0}%"));
    }
    parts.join(" · ")
}

fn format_timestamp(timestamp: Option<std::time::SystemTime>) -> Option<String> {
    let timestamp = timestamp?;
    let datetime: chrono::DateTime<chrono::Local> = timestamp.into();
    Some(datetime.format("%H:%M:%S").to_string())
}

pub(crate) fn top_right_timestamp<'a>(meta: &SegmentMeta, t: &dyn Theme) -> Option<Line<'a>> {
    let timestamp = format_timestamp(meta.timestamp);
    let tokens = meta.actual_tokens;
    let ctx = meta.context_percent;
    if timestamp.is_none() && tokens.is_none() && ctx.is_none() {
        return None;
    }
    // Combined right-rail title: `ctx:45% · ↑1.2k ↓340 · 14:32`
    let dim_style = Style::default().fg(t.dim()).add_modifier(Modifier::DIM);
    let sep = Span::styled(" · ", dim_style);
    let mut spans: Vec<Span<'a>> = Vec::new();
    // Context fill — only show when above 30% to avoid noise
    if let Some(pct) = ctx.filter(|p| *p > 30.0) {
        let ctx_color = super::widgets::percent_color(pct, t);
        spans.push(Span::styled(
            format!("ctx:{pct:.0}%"),
            Style::default().fg(ctx_color).add_modifier(Modifier::DIM),
        ));
    }
    if let Some(tokens) = tokens {
        if !spans.is_empty() {
            spans.push(sep.clone());
        }
        spans.push(Span::styled(
            tokens.format_compact(),
            Style::default()
                .fg(t.accent_muted())
                .add_modifier(Modifier::DIM),
        ));
    }
    if let Some(stamp) = timestamp {
        if !spans.is_empty() {
            spans.push(sep);
        }
        spans.push(Span::styled(stamp, dim_style));
    }
    if spans.is_empty() {
        return None;
    }
    Some(Line::from(spans))
}

fn tool_title_line(
    status_icon: &str,
    status_color: Color,
    display_name: &str,
    area_width: u16,
    timestamp: Option<&str>,
    pinned: bool,
) -> Line<'static> {
    let timestamp_width = timestamp.map(UnicodeWidthStr::width).unwrap_or(0);
    let reserved_right = if timestamp_width > 0 {
        timestamp_width + 3
    } else {
        0
    };
    let left_budget = area_width
        .saturating_sub(2)
        .saturating_sub(reserved_right as u16)
        .max(6) as usize;
    let status_prefix = format!(" {status_icon} ");
    let prefix_width = UnicodeWidthStr::width(status_prefix.as_str());
    let name_budget = left_budget.saturating_sub(prefix_width).max(1);
    let title_label = if pinned {
        format!("{display_name} · pinned")
    } else {
        display_name.to_string()
    };
    let title_name = crate::util::truncate(&title_label, name_budget);
    let title_text = format!("{status_prefix}{title_name} ");
    let used_width = UnicodeWidthStr::width(title_text.as_str());
    let pad = left_budget.saturating_sub(used_width);

    Line::from(vec![
        Span::styled(
            status_prefix,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(
            format!("{title_name} "),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "─".repeat(pad),
            Style::default().fg(dim_color(status_color, 0.35)),
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_tool_card(
    name: &str,
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    is_error: bool,
    complete: bool,
    expanded: bool,
    live_partial: Option<&omegon_traits::PartialToolResult>,
    started_at: Option<std::time::Instant>,
    meta: &SegmentMeta,
    tool_category: Option<ToolCategory>,
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
    mode: SegmentRenderMode,
    density: crate::settings::ToolDetail,
    pinned: bool,
) {
    // `▶` U+25B6 is in the Unicode emoji set — replaced with `▷` U+25B7
    // for the same reason as the instruments-panel pass. Both `✗` and
    // `▸` are already safe.
    let chrome = tool_card_chrome(name, detail_args, is_error, complete, tool_category, t);
    let display_name = chrome.display_name;
    let status_icon = chrome.status_icon;
    let status_color = chrome.status_color;
    let border_color = chrome.border_color;
    let bg = chrome.background;

    let timestamp = format_timestamp(meta.timestamp);
    let title = tool_title_line(
        status_icon,
        status_color,
        &display_name,
        area.width,
        timestamp.as_deref(),
        pinned,
    );

    // Right-aligned title: duration · ↑1.2k ↓340 · 14:32
    let right_title_spans = super::segment_components::tool_card::tool_card_right_title_spans(
        complete,
        meta.duration_ms,
        meta.actual_tokens,
        timestamp.as_deref(),
        t,
    );

    if matches!(mode, SegmentRenderMode::Slim) && !complete && !expanded {
        let cells = slim_tool_summary_cells(
            name,
            detail_args,
            detail_result,
            complete,
            live_partial,
            started_at,
            meta.duration_ms,
        );
        let detail_rows = slim_tool_live_rows(area.width, &cells);
        super::segment_components::tool_card::render_slim_tool_live_rows(
            area,
            buf,
            t,
            bg,
            status_icon,
            status_color,
            &display_name,
            &detail_rows,
            pinned,
        );
        return;
    }

    if matches!(mode, SegmentRenderMode::Slim) && complete && !expanded {
        let cells = slim_tool_summary_cells(
            name,
            detail_args,
            detail_result,
            complete,
            live_partial,
            started_at,
            meta.duration_ms,
        );
        let detail_rows = vec![slim_tool_collapsed_line(area.width, &cells)];
        super::segment_components::tool_card::render_slim_tool_summary_rows(
            area,
            buf,
            t,
            bg,
            status_icon,
            status_color,
            &display_name,
            &detail_rows,
            pinned,
        );
        return;
    }

    let card_block = if matches!(mode, SegmentRenderMode::Slim) {
        // Slim: top border only, no side borders — maximizes terminal
        // text selection width and avoids │ chars in copied text.
        Block::default()
            .borders(Borders::TOP)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(border_color).bg(bg))
            .title_top(title)
            .title_top(Line::from(right_title_spans).right_aligned())
            .padding(Padding::horizontal(0))
            .style(Style::default().bg(bg))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color).bg(bg))
            .title_top(title)
            .title_top(Line::from(right_title_spans).right_aligned())
            .padding(Padding::horizontal(1))
            .style(Style::default().bg(bg))
    };

    let card_inner = card_block.inner(area);
    card_block.render(area, buf);

    if card_inner.height == 0 || card_inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Effective density: expanded overrides to Verbose.
    let effective = if expanded {
        crate::settings::ToolDetail::Verbose
    } else {
        density
    };
    let args_budget = effective.args_budget();
    let result_budget = effective.result_budget();
    let tail_budget = effective.tail_budget();

    super::segment_components::tool_card::append_tool_args_section(
        &mut lines,
        name,
        detail_args,
        detail_result,
        args_budget,
        complete,
        is_error,
        effective,
        bg,
        t,
    );
    if matches!(effective, crate::settings::ToolDetail::Lean) && complete && !is_error {
        let para = Paragraph::new(lines.clone())
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(bg));
        para.render(card_inner, buf);
        apply_rendered_links(
            card_inner,
            &lines,
            buf,
            Style::default()
                .fg(t.accent_muted())
                .bg(bg)
                .add_modifier(Modifier::UNDERLINED),
            card_inner.height,
        );
        return;
    }

    let mut live_row_fills: Vec<(u16, Color)> = Vec::new();
    super::segment_components::tool_card::append_tool_live_progress_section(
        &mut lines,
        &mut live_row_fills,
        live_partial,
        started_at,
        complete,
        tail_budget,
        card_inner.width,
        bg,
        t,
    );

    // ── Edit/change diff section ────────────────────────────────
    // For mutating-file tools (`edit`, `change`), the standard result
    // text is just "Successfully replaced text in {path}" — useless
    // for an operator who wants to see what actually changed.
    // Replace it with a colored line-by-line diff computed from the
    // tool's args (`oldText` / `newText`), which the renderer already
    // has access to via `detail_args`. The diff rendered here is the
    // intent — what the agent ASKED for — not the post-validation
    // result. On a successful edit they're equivalent; on a failed
    // edit the validation error is rendered separately below.
    let mut result_row_fills: Vec<(u16, Color)> = Vec::new();
    let diff_blocks: Option<Vec<EditDiffBlock>> = if matches!(name, "edit" | "change") {
        detail_args.and_then(|args| build_edit_diff_blocks(name, args))
    } else {
        None
    };
    if let Some(blocks) = diff_blocks {
        if !lines.is_empty() {
            let sep_color = if is_error { t.error() } else { t.border_dim() };
            lines.push(Line::from(Span::styled(
                "─".repeat(card_inner.width as usize),
                Style::default().fg(sep_color).bg(bg),
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
        }
        let max_diff_lines = effective.diff_budget();
        let mut emitted = 0usize;
        let removed_style = Style::default().fg(t.error()).bg(bg);
        let added_style = Style::default().fg(t.success()).bg(bg);
        let header_style = Style::default()
            .fg(t.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::BOLD);
        let summary_style = Style::default().fg(t.muted()).bg(bg);

        // Per-block summary line: total +N -M across all diff blocks
        // (one per file in the change tool's case). The summary is
        // always the first line in the diff section so the operator
        // gets a quick read at the top.
        let total_added: usize = blocks.iter().map(|b| b.new_text.lines().count()).sum();
        let total_removed: usize = blocks.iter().map(|b| b.old_text.lines().count()).sum();
        lines.push(Line::from(vec![
            Span::styled(format!("Δ {} edit(s) · ", blocks.len()), summary_style),
            Span::styled(format!("+{total_added}"), added_style),
            Span::styled(" / ", summary_style),
            Span::styled(format!("-{total_removed}"), removed_style),
            Span::styled(
                if expanded {
                    ""
                } else {
                    "  (expand for full diff)"
                },
                summary_style,
            ),
        ]));
        result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));

        // Per-block diff body. Each block is preceded by a `▸ {file}`
        // header (only when there's more than one block) so the
        // operator can tell which file each hunk belongs to.
        //
        // Each emitted line is filtered for control bytes via
        // `sanitize_diff_line` — the agent's `oldText`/`newText` args
        // shouldn't normally contain ESC bytes, but if they do, we
        // don't want them ending up in cell symbols where the
        // terminal would interpret them as escape sequences.
        let sanitize_diff_line =
            |s: &str| -> String { s.chars().filter(|c| !c.is_control()).collect() };
        let multi_block = blocks.len() > 1;
        'outer: for block in &blocks {
            if multi_block {
                if emitted >= max_diff_lines {
                    break;
                }
                lines.push(Line::from(Span::styled(
                    format!("▸ {}", sanitize_diff_line(&block.file)),
                    header_style,
                )));
                result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                emitted += 1;
            }
            for line in block.old_text.lines() {
                if emitted >= max_diff_lines {
                    break 'outer;
                }
                lines.push(Line::from(Span::styled(
                    format!("- {}", sanitize_diff_line(line)),
                    removed_style,
                )));
                result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                emitted += 1;
            }
            for line in block.new_text.lines() {
                if emitted >= max_diff_lines {
                    break 'outer;
                }
                lines.push(Line::from(Span::styled(
                    format!("+ {}", sanitize_diff_line(line)),
                    added_style,
                )));
                result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                emitted += 1;
            }
        }

        // Truncation marker if we capped before showing the whole diff.
        let total_diff_lines: usize = blocks
            .iter()
            .map(|b| {
                let header = if multi_block { 1 } else { 0 };
                header + b.old_text.lines().count() + b.new_text.lines().count()
            })
            .sum();
        if total_diff_lines > emitted {
            lines.push(Line::from(Span::styled(
                format!("… {} more diff line(s)", total_diff_lines - emitted),
                summary_style,
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
        }

        // If the tool actually erred, surface the error result text
        // below the diff so the operator sees both intent and outcome.
        if is_error && let Some(err_text) = detail_result {
            lines.push(Line::from(Span::styled(
                err_text.lines().next().unwrap_or(err_text).to_string(),
                Style::default().fg(t.error()).bg(bg),
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
        }
    } else if let Some(result) = detail_result {
        let pre_result_line_count = lines.len();
        if !lines.is_empty() {
            // Separator line — matches card border color (red on error)
            let sep_color = if is_error { t.error() } else { t.border_dim() };
            let sep_bg = bg;
            lines.push(Line::from(Span::styled(
                "─".repeat(card_inner.width as usize),
                Style::default().fg(sep_color).bg(sep_bg),
            )));
            result_row_fills.push((pre_result_line_count as u16, sep_bg));
        }

        // Pretty-print JSON results — tool outputs often arrive as compact JSON
        // with literal \n inside string values (e.g. commit messages).
        let pretty_result: std::borrow::Cow<'_, str> =
            if result.starts_with('{') || result.starts_with('[') {
                match serde_json::from_str::<serde_json::Value>(result) {
                    Ok(val) => std::borrow::Cow::Owned(
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| result.to_string()),
                    ),
                    Err(_) => std::borrow::Cow::Borrowed(result),
                }
            } else {
                std::borrow::Cow::Borrowed(result)
            };
        let result_lines: Vec<&str> = pretty_result.lines().collect();
        let max_lines = result_budget;
        let show = result_lines.len().min(max_lines);
        let display_text = result_lines[..show].join("\n");

        // Try syntax highlighting based on file extension from args
        let highlighted = if !is_error {
            try_highlight(&display_text, detail_args, name, t)
        } else {
            None
        };

        if let Some(highlighted_lines) = highlighted {
            for line in highlighted_lines {
                // Apply card bg to each span so result rows stay visually unified.
                let spans: Vec<Span<'_>> = line
                    .spans
                    .into_iter()
                    .map(|mut s| {
                        s.style = s.style.bg(bg);
                        s
                    })
                    .collect();
                lines.push(Line::from(spans));
                result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
            }
        } else {
            let result_style = if is_error {
                Style::default().fg(t.error()).bg(bg)
            } else {
                Style::default().fg(t.muted()).bg(bg)
            };

            let mut table_state = TableState::None;
            let visible_lines = &result_lines[..show];
            let has_table_lines = visible_lines.iter().any(|line| is_table_line(line.trim()));

            if !is_error && has_table_lines {
                // Pre-pass to compute shared per-column widths across
                // each table block — see `compute_table_widths` for the
                // rationale (the column-shred bug in codebase_search
                // results).
                let table_widths_per_line =
                    compute_table_widths(visible_lines, card_inner.width as usize);
                for (idx, line) in visible_lines.iter().copied().enumerate() {
                    let trimmed = line.trim();
                    if let Some(target_widths) = table_widths_per_line[idx].as_ref() {
                        let is_header = matches!(table_state, TableState::None);
                        if is_table_separator(trimmed) || matches!(table_state, TableState::Header)
                        {
                            table_state = TableState::Body;
                        } else {
                            table_state = TableState::Header;
                        }
                        let row_bg = bg;
                        lines.push(render_table_line(trimmed, is_header, target_widths, t));
                        result_row_fills.push((lines.len().saturating_sub(1) as u16, row_bg));
                    } else {
                        table_state = TableState::None;
                        let rendered = if trimmed.is_empty() {
                            Line::from(Span::styled(String::new(), Style::default().bg(bg)))
                        } else {
                            let mut line = super::widgets::highlight_line(line, t);
                            for span in &mut line.spans {
                                span.style = span.style.bg(bg);
                                if span.style.fg.is_none() {
                                    span.style = span.style.fg(t.muted());
                                }
                            }
                            line
                        };
                        lines.push(rendered);
                        result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                    }
                }
            } else {
                // Try ANSI color parsing for tool output (cargo, git diff, etc.)
                let joined = result_lines[..show].join("\n");
                let has_ansi = joined.contains('\x1b');

                if has_ansi {
                    use ansi_to_tui::IntoText as _;
                    if let Ok(text) = joined.into_text() {
                        for line in text.lines {
                            let spans: Vec<Span<'_>> = line
                                .spans
                                .into_iter()
                                .map(|mut s| {
                                    // Preserve ANSI foreground, apply card background
                                    s.style = s.style.bg(bg);
                                    // If no foreground was set by ANSI, use muted
                                    if s.style.fg.is_none() {
                                        s.style = s.style.fg(t.muted());
                                    }
                                    s
                                })
                                .collect();
                            lines.push(Line::from(spans));
                            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                        }
                    } else {
                        // ANSI parse failed — fall back to plain
                        for line in &result_lines[..show] {
                            lines.push(Line::from(Span::styled(line.to_string(), result_style)));
                            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                        }
                    }
                } else {
                    for line in &result_lines[..show] {
                        let trimmed = line.trim();
                        let rendered = if is_error {
                            Line::from(Span::styled(line.to_string(), result_style))
                        } else if trimmed.is_empty() {
                            Line::from(Span::styled(String::new(), Style::default().bg(bg)))
                        } else {
                            let mut line = super::widgets::highlight_line(line, t);
                            for span in &mut line.spans {
                                span.style = span.style.bg(bg);
                                if span.style.fg.is_none() {
                                    span.style = span.style.fg(t.muted());
                                }
                            }
                            line
                        };
                        lines.push(rendered);
                        result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
                    }
                }
            }
        }

        if result_lines.len() > show {
            let hint = if expanded {
                format!("  ── {} lines ── Tab to collapse", result_lines.len())
            } else {
                format!(
                    "  ── {} more lines ── Ctrl+O to expand",
                    result_lines.len() - show
                )
            };
            lines.push(Line::from(Span::styled(
                hint,
                Style::default().fg(t.accent_muted()).bg(bg),
            )));
            result_row_fills.push((lines.len().saturating_sub(1) as u16, bg));
        }
    }

    Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .render(card_inner, buf);

    // Apply background fills for both the live (in-flight) section and
    // the completed result section. Both share the same `bg` color in
    // practice; keeping the two fill streams separate makes the
    // intent obvious and lets future styling diverge them cheaply.
    for (row, fill_bg) in live_row_fills {
        apply_rows_bg(card_inner, row, 1, fill_bg, buf);
    }
    for (row, fill_bg) in result_row_fills {
        apply_rows_bg(card_inner, row, 1, fill_bg, buf);
    }
    apply_rendered_links(
        card_inner,
        &lines,
        buf,
        Style::default()
            .fg(t.accent_muted())
            .bg(bg)
            .add_modifier(Modifier::UNDERLINED),
        card_inner.height,
    );

    // ── Post-render: OSC 8 hyperlinks for single-file tool paths ────────────
    if matches!(name, "read" | "write" | "view")
        && let Some(args) = detail_args
    {
        let file_path = args.lines().next().unwrap_or(args).trim().to_string();
        if !file_path.is_empty() && card_inner.height > 0 {
            let prefix = "▸ ";
            let row_style = Style::default().bg(bg);
            let link_style = Style::default()
                .fg(t.accent_muted())
                .bg(bg)
                .add_modifier(Modifier::UNDERLINED);

            for x in card_inner.left()..card_inner.right() {
                if let Some(cell) = buf.cell_mut((x, card_inner.y)) {
                    cell.set_symbol(" ");
                    cell.set_style(row_style);
                }
            }

            if card_inner.width >= prefix.len() as u16 {
                if let Some(cell) = buf.cell_mut((card_inner.x, card_inner.y)) {
                    cell.set_symbol("▸");
                    cell.set_style(Style::default().fg(t.accent_muted()).bg(bg));
                }
                if let Some(cell) = buf.cell_mut((card_inner.x + 1, card_inner.y)) {
                    cell.set_symbol(" ");
                    cell.set_style(row_style);
                }

                let available = card_inner.width.saturating_sub(prefix.len() as u16);
                if available > 0
                    && let Some(url) = file_url_for_path(&file_path)
                {
                    let link_area = Rect {
                        x: card_inner.x + prefix.len() as u16,
                        y: card_inner.y,
                        width: available,
                        height: 1,
                    };
                    let link = hyperrat::Link::new(file_path, url).style(link_style);
                    link.render(link_area, buf);
                }
            }
        }
    }
}

/// Attempt syntax highlighting for tool result text.
/// Returns None if no syntax can be detected.
fn try_highlight<'a>(
    text: &str,
    detail_args: Option<&str>,
    tool_name: &str,
    _t: &dyn Theme,
) -> Option<Vec<Line<'a>>> {
    // Determine syntax from file extension or tool type
    let syntax_name = if tool_name == "read" || tool_name == "edit" || tool_name == "write" {
        // detail_args is the file path — extract extension
        detail_args.and_then(|path| {
            let ext = path.rsplit('.').next()?;
            match ext {
                "rs" => Some("Rust"),
                "ts" | "tsx" => Some("TypeScript"),
                "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
                "json" => Some("JSON"),
                "toml" => Some("TOML"),
                "yaml" | "yml" => Some("YAML"),
                "py" => Some("Python"),
                "go" => Some("Go"),
                "sh" | "bash" | "zsh" => Some("Bourne Again Shell (bash)"),
                "md" | "markdown" => Some("Markdown"),
                "html" | "htm" => Some("HTML"),
                "css" => Some("CSS"),
                "sql" => Some("SQL"),
                "xml" => Some("XML"),
                "c" | "h" => Some("C"),
                "cpp" | "cc" | "cxx" | "hpp" => Some("C++"),
                "java" => Some("Java"),
                "rb" => Some("Ruby"),
                "swift" => Some("Swift"),
                "kt" | "kts" => Some("Kotlin"),
                "dockerfile" | "Dockerfile" => Some("Dockerfile"),
                _ => None,
            }
        })
    } else if tool_name == "bash" {
        Some("Bourne Again Shell (bash)")
    } else {
        None
    }?;

    let cache = syntax_cache();
    let syntax = cache.syntax_set.find_syntax_by_name(syntax_name)?;
    // Show line numbers for file content, not command output.
    // For read/edit/write tools: always show (it's file content).
    // For bash: show only if the command reads a file (cat, head, tail, sed, etc.)
    let show_line_numbers = match tool_name {
        "read" | "edit" | "write" => true,
        "bash" => detail_args.is_some_and(|cmd| {
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            matches!(
                first_word,
                "cat" | "head" | "tail" | "sed" | "awk" | "less" | "bat" | "nl"
            )
        }),
        _ => false,
    };
    let highlighter = Highlighter::new(cache.theme.clone()).line_numbers(show_line_numbers);
    let text_lines: Vec<&str> = text.lines().collect();
    let highlighted = highlighter
        .highlight_lines(text_lines, syntax, &cache.syntax_set)
        .ok()?;
    Some(
        highlighted
            .lines
            .into_iter()
            .map(|line| {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|span| Span::styled(span.content.to_string(), span.style))
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
    )
}

/// Table parsing state — tracks whether we're in header, separator, or body rows.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TableState {
    None,
    Header,
    Body,
}

/// One file's worth of edit-diff data for the `edit` / `change` tool
/// rendering path. The renderer pulls these out of the tool's args
/// (which it has via `detail_args`) and synthesizes a colored line-by-
/// line diff in place of the boring "Successfully replaced text" result.
#[derive(Debug, Clone)]
struct EditDiffBlock {
    file: String,
    old_text: String,
    new_text: String,
}

/// Parse `detail_args` JSON for an `edit` or `change` tool call and
/// extract one or more `EditDiffBlock`s. Returns `None` for tools whose
/// args don't carry the expected `oldText`/`newText` fields (which is
/// also the bail-out for non-edit/non-change tools and for malformed
/// payloads — in both cases the renderer falls back to the standard
/// result text rendering).
///
/// Tool arg shapes:
/// - **edit**: `{ "path": "...", "oldText": "...", "newText": "..." }`
///   → one `EditDiffBlock`
/// - **change**: `{ "edits": [{ "file": "...", "oldText": "...",
///   "newText": "..." }, ...] }` → one block per edit, in order
fn build_edit_diff_blocks(name: &str, args: &str) -> Option<Vec<EditDiffBlock>> {
    let parsed: serde_json::Value = serde_json::from_str(args).ok()?;
    match name {
        "edit" => {
            let path = parsed
                .get("path")
                .or_else(|| parsed.get("file"))
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown file)")
                .to_string();
            let old_text = parsed.get("oldText").and_then(|v| v.as_str())?.to_string();
            let new_text = parsed.get("newText").and_then(|v| v.as_str())?.to_string();
            Some(vec![EditDiffBlock {
                file: path,
                old_text,
                new_text,
            }])
        }
        "change" => {
            let edits = parsed.get("edits")?.as_array()?;
            let blocks: Vec<EditDiffBlock> = edits
                .iter()
                .filter_map(|edit| {
                    let file = edit
                        .get("file")
                        .or_else(|| edit.get("path"))
                        .and_then(|v| v.as_str())?
                        .to_string();
                    let old_text = edit.get("oldText").and_then(|v| v.as_str())?.to_string();
                    let new_text = edit.get("newText").and_then(|v| v.as_str())?.to_string();
                    Some(EditDiffBlock {
                        file,
                        old_text,
                        new_text,
                    })
                })
                .collect();
            if blocks.is_empty() {
                None
            } else {
                Some(blocks)
            }
        }
        _ => None,
    }
}

/// Detect markdown table lines: `| cell | cell |` or `| cell | cell`
/// (with or without trailing `|`).
///
/// The trailing pipe is optional in the CommonMark / GFM spec and many
/// LLMs omit it on body rows even when the header row has it. The
/// previous implementation required `ends_with('|')`, which caused body
/// rows to fall through to the non-table rendering path and disappear
/// from the operator's view (the "header renders, body is gone" bug
/// from the screenshot).
///
/// The relaxed check: starts with `|`, is longer than 2 chars, and
/// contains at least one more `|` after the leading one (so a line
/// like `| single column no pipe` still doesn't match — but that's
/// not a valid table row in any reasonable interpretation).
fn is_table_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.len() > 2 && trimmed[1..].contains('|')
}

/// Detect table separator: `|---|---|` or `| --- | --- |` or `|---|---`
/// (trailing pipe optional, same rationale as `is_table_line`).
pub(crate) fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.len() > 2
        && trimmed[1..].contains('|')
        && trimmed
            .chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

/// Pre-compute per-column target widths for every markdown table block
/// in `lines`, returning a parallel `Vec` aligned with the input where
/// each entry is `Some(widths)` if the line belongs to a table block and
/// `None` otherwise.
///
/// Why this exists: `render_table_line` was originally called per-row
/// with no cross-row coordination, so each row computed its own column
/// widths from its own cell contents. Body rows with long content (e.g.
/// codebase_search Preview cells) got their last column truncated
/// independently, while the header row computed shorter widths from its
/// short labels — the columns didn't line up and the table looked
/// shredded. This pass collects every consecutive run of table lines
/// into a "block", computes the max-per-column across the block, then
/// shrinks the last column to fit `available_width` if the total
/// overflows. All rows in the same block render with the same target
/// widths, so columns align.
///
/// `available_width` is the inner card width in cells. Returns one
/// `Vec<usize>` (column widths) per table line; non-table lines map to
/// `None`. Separator rows participate in column-count detection but
/// not in width measurement (they're all dashes).
pub(crate) fn compute_table_widths(
    lines: &[&str],
    available_width: usize,
) -> Vec<Option<Vec<usize>>> {
    let mut result: Vec<Option<Vec<usize>>> = vec![None; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        if !is_table_line(lines[i].trim()) {
            i += 1;
            continue;
        }
        // Find the end of this table block (consecutive table lines).
        let start = i;
        let mut end = i;
        while end < lines.len() && is_table_line(lines[end].trim()) {
            end += 1;
        }

        // Compute per-column max widths across all non-separator rows
        // in the block. Separator rows are all dashes and would
        // misreport the width as 3+ cells of `---`, so we skip them
        // for measurement but they still participate in rendering.
        let mut col_widths: Vec<usize> = Vec::new();
        for line in &lines[start..end] {
            let trimmed = line.trim();
            if is_table_separator(trimmed) {
                continue;
            }
            let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
            for (idx, cell) in cells.iter().enumerate() {
                let w = markdown_display_width(cell.trim()).max(1);
                if idx >= col_widths.len() {
                    col_widths.push(w);
                } else if w > col_widths[idx] {
                    col_widths[idx] = w;
                }
            }
        }

        // Constrain to fit available width. Chrome math:
        //   per-cell rendered width = " content " = (target_w + 2) cells
        //   inter-cell pipes = (N - 1) cells
        //   outer pipes = 2 cells
        //   total = sum(target_w) + 3 * N + 1
        // → content budget = available_width - 3*N - 1
        // If the total content overflows the budget, shrink the LAST
        // column (typically Preview / longest content) down to whatever
        // fits, with a minimum of 8 cells so it stays useful. We don't
        // distribute the overflow across columns because the operator
        // generally cares more about File/Lines/Type/Score being
        // legible than the Preview cell being complete.
        let cell_count = col_widths.len();
        if cell_count > 0 {
            let chrome = cell_count.saturating_mul(3).saturating_add(1);
            let content_budget = available_width.saturating_sub(chrome);
            let total: usize = col_widths.iter().sum();
            if total > content_budget {
                let last_idx = cell_count - 1;
                let other_total: usize = col_widths.iter().take(last_idx).sum();
                let last_budget = content_budget.saturating_sub(other_total).max(8);
                col_widths[last_idx] = last_budget;
            }
        }

        // Apply the same widths to every line in the block.
        for slot in &mut result[start..end] {
            *slot = Some(col_widths.clone());
        }
        i = end;
    }
    result
}

/// Render a markdown table line with cell highlighting using
/// pre-computed shared column widths from `compute_table_widths`. The
/// caller is responsible for ensuring `target_widths` reflects the
/// max-per-column across all rows in the same table block — passing
/// per-row-derived widths breaks alignment.
pub(crate) fn render_table_line<'a>(
    line: &str,
    is_header: bool,
    target_widths: &[usize],
    t: &dyn Theme,
) -> Line<'a> {
    let trimmed = line.trim();
    let row_bg = if is_header {
        t.card_bg()
    } else {
        t.surface_bg()
    };
    let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
    let cell_count = target_widths.len().max(cells.len());

    // Separator row: |---|---| → render as a thin rule sized to the content budget.
    if is_table_separator(trimmed) {
        let sep_bg = t.surface_bg();
        let sep_fg = t.border();
        let mut spans: Vec<Span<'a>> = Vec::new();
        spans.push(Span::styled("├", Style::default().fg(sep_fg).bg(sep_bg)));
        for (i, width) in target_widths.iter().enumerate() {
            spans.push(Span::styled(
                "─".repeat(width.saturating_add(2)),
                Style::default().fg(sep_fg).bg(sep_bg),
            ));
            if i < target_widths.len() - 1 {
                spans.push(Span::styled("┼", Style::default().fg(sep_fg).bg(sep_bg)));
            }
        }
        spans.push(Span::styled("┤", Style::default().fg(sep_fg).bg(sep_bg)));
        return Line::from(spans);
    }

    // Iterate by the shared column count from `target_widths`, not by
    // the row's own cell count. Rows with fewer cells than the block's
    // max get padded with empty cells; rows with more get truncated.
    // Both cases keep columns aligned across the table block, which
    // is the whole point of the pre-pass that produces target_widths.
    let pipe = Style::default().fg(t.border()).bg(row_bg);
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::styled("│", pipe));
    for (i, &width) in target_widths.iter().enumerate() {
        let cell_raw = cells.get(i).copied().unwrap_or("").trim();
        let cell_text = truncate_table_cell(cell_raw, width);
        if is_header {
            let padded = super::widgets::pad_right(&cell_text, width);
            spans.push(Span::styled(
                format!(" {padded} "),
                Style::default()
                    .fg(t.accent_bright())
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(" ", Style::default().bg(row_bg)));
            let mut cell_spans = super::widgets::highlight_inline(&cell_text, t);
            let rendered_width: usize = cell_spans
                .iter()
                .map(|s| super::widgets::visible_width(&s.content))
                .sum();
            for mut s in cell_spans.drain(..) {
                s.style = s.style.bg(row_bg);
                spans.push(s);
            }
            // Pad to target width based on rendered display width (after
            // markdown syntax stripping), not raw string width.
            let pad = width.saturating_sub(rendered_width);
            if pad > 0 {
                spans.push(Span::styled(" ".repeat(pad), Style::default().bg(row_bg)));
            }
            spans.push(Span::styled(" ", Style::default().bg(row_bg)));
        }
        if i + 1 < cell_count {
            spans.push(Span::styled("│", pipe));
        }
    }
    spans.push(Span::styled("│", pipe));

    Line::from(spans)
}

fn truncate_table_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    // Truncate based on display width after markdown stripping so that
    // the truncation point aligns with the column budget.
    let display_w = markdown_display_width(text);
    if display_w <= width {
        return text.to_string();
    }
    // Strip markdown first, then truncate the plain text.
    let stripped = strip_inline_markdown(text);
    super::widgets::truncate_str(&stripped, width, "…")
}

/// Approximate display width of text after inline markdown rendering.
/// Strips `**`, `*`, and `` ` `` markers that `highlight_inline` would
/// consume, then measures the remaining visible width.
fn markdown_display_width(text: &str) -> usize {
    super::widgets::visible_width(&strip_inline_markdown(text))
}

/// Strip inline markdown syntax for width measurement.
/// Handles: `**bold**`, `*italic*`, `` `code` ``
fn strip_inline_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            // Skip opening backtick, copy content, skip closing backtick
            i += 1;
            while i < chars.len() && chars[i] != '`' {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip closing `
            }
        } else if chars[i] == '*' {
            // Skip `**` or `*`
            i += 1;
            if i < chars.len() && chars[i] == '*' {
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Render a placeholder for an image (used when StatefulProtocol isn't available).
/// The actual image rendering happens in conv_widget.rs via ratatui-image.
///
/// Visual choices:
/// - **Frame**: doubled-line border in `accent_muted` rather than the
///   default `border_dim`/rounded combo. The image content gets composited
///   on top of this rectangle in a second pass; if the image happens to
///   share colors with the surrounding TUI surface (light screenshots,
///   pasted UI captures, etc.) the doubled frame makes the segment
///   bounds unambiguous.
/// - **Glyph**: `▦` U+25A6 SQUARE WITH ORTHOGONAL CROSSHATCH FILL.
///   Single-cell, not in the Unicode emoji set. The previous `📎`
///   U+1F4CE PAPERCLIP is an emoji-presentation codepoint and is
///   forbidden by the same constraint that drove the instruments-panel
///   glyph audit.
/// - **Title**: full disk path (`path.display()`) rather than just
///   `file_name()`. Operators need to know where on disk the file
///   lives — especially for clipboard-paste files like
///   `omegon-clipboard-78315-16.png` whose names are uninformative
///   without their parent directory.
// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::SegmentEmphasis;
    use crate::tui::theme::Alpharius;
    use crate::tui::widgets;

    fn make_buf(w: u16, h: u16) -> (Rect, Buffer) {
        let area = Rect::new(0, 0, w, h);
        (area, Buffer::empty(area))
    }

    fn buf_text(buf: &Buffer, area: Rect) -> String {
        let mut text = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn find_row_containing(buf: &Buffer, area: Rect, needle: &str) -> Option<u16> {
        for y in area.top()..area.bottom() {
            let mut row = String::new();
            for x in area.left()..area.right() {
                row.push_str(buf[(x, y)].symbol());
            }
            if row.contains(needle) {
                return Some(y);
            }
        }
        None
    }

    #[test]
    fn slim_tool_overflow_hint_does_not_advertise_details_without_expandable_cell() {
        let cells = vec![
            "alpha running".to_string(),
            "beta".to_string(),
            "gamma".to_string(),
            "delta".to_string(),
            "epsilon".to_string(),
            "zeta".to_string(),
        ];

        let detail_rows = slim_tool_detail_lines(42, &cells);
        let live_rows = slim_tool_live_rows(12, &cells);

        assert_eq!(slim_tool_overflow_hint(1, &[]), "+1 more");
        assert!(!detail_rows.iter().any(|row| row.contains("Ctrl+O details")));
        assert!(!live_rows.iter().any(|row| row.contains("Ctrl+O details")));
    }

    #[test]
    fn slim_tool_overflow_hint_keeps_details_when_expandable_cell_is_hidden() {
        let cells = vec![
            "alpha running".to_string(),
            "beta".to_string(),
            "gamma".to_string(),
            "delta".to_string(),
            "epsilon".to_string(),
            "Ctrl+O details".to_string(),
        ];

        let _detail_rows = slim_tool_detail_lines(42, &cells);
        let live_rows = slim_tool_live_rows(12, &cells);

        assert_eq!(
            slim_tool_overflow_hint(1, &[&cells[5]]),
            "+1 more · Ctrl+O details"
        );
        assert!(
            live_rows
                .iter()
                .any(|row| row.contains("+1 more · Ctrl+O details"))
        );
    }

    #[test]
    fn detects_bare_agent_links_without_trailing_punctuation() {
        let links = detect_links("See https://example.com/docs, then file:///tmp/x.");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].label, "https://example.com/docs");
        assert_eq!(links[0].url, "https://example.com/docs");
        assert_eq!(links[1].label, "file:///tmp/x");
        assert_eq!(links[1].url, "file:///tmp/x");
    }

    #[test]
    fn does_not_autolink_bare_markdown_file_paths() {
        let links = detect_links("Transcript: /tmp/omegon-transcript-20260519.md.");
        assert!(
            links.is_empty(),
            "bare markdown paths should stay plain text; terminal file links show misleading cursor affordances"
        );
    }

    #[test]
    fn file_tool_links_resolve_relative_paths_to_file_urls() {
        let url = file_url_for_path("Cargo.toml").expect("relative path should resolve");
        assert!(
            url.starts_with("file:///"),
            "relative file paths should become absolute file URLs: {url}"
        );
        assert!(
            url.ends_with("/Cargo.toml"),
            "resolved URL should preserve the target file name: {url}"
        );
    }

    #[test]
    fn inline_link_rendering_preserves_text_after_the_link() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "See https://example.com/docs for details.".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(90, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("for details."),
            "link overlay must not clear the suffix after the URL: {text}"
        );
    }

    #[test]
    fn slim_assistant_text_renders_without_copy_hostile_headers() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "Plain response text.".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(60, 4);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("Plain response text."), "{text}");
        assert!(!text.contains("answer"), "{text}");
        assert!(!text.contains("omegon"), "{text}");
    }

    #[test]
    fn slim_assistant_reasoning_collapses_to_single_status_row() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: String::new(),
                thinking: "**Considering documentation needs**\n\nI need to modify documents and inspect templates before editing.".into(),
                complete: false,
            },
        };
        assert_eq!(
            seg.height_in_mode(80, &Alpharius, SegmentRenderMode::Slim),
            1
        );

        let (area, mut buf) = make_buf(80, 4);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("reasoning (3 lines)"), "{text}");
        assert!(text.contains("Considering documentation needs"), "{text}");
        assert!(
            !text.contains("I need to modify documents"),
            "slim mode should not dump full reasoning prose between tool rows: {text}"
        );
    }

    #[test]
    fn slim_completed_tool_card_collapses_to_single_line() {
        let mut seg = Segment::tool_card("tool-1", "bash");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args = Some("cargo test".into());
            *detail_result = Some("ok\nmore output".into());
        }
        assert_eq!(
            seg.height_in_mode(80, &Alpharius, SegmentRenderMode::Slim),
            1
        );
    }

    #[test]
    fn slim_completed_error_tool_card_collapses_to_single_line() {
        let mut seg = Segment::tool_card("tool-1", "edit");
        if let SegmentContent::ToolCard {
            complete,
            is_error,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *is_error = true;
            *detail_args = Some("core/crates/omegon/src/tui/segments.rs".into());
            *detail_result =
                Some("Found 2 occurrences of the text. The text must be unique.".into());
        }

        assert_eq!(
            seg.height_in_mode(80, &Alpharius, SegmentRenderMode::Slim),
            1
        );

        let (area, mut buf) = make_buf(100, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("✗"), "should preserve error status: {text}");
        assert!(text.contains("edit"), "should name the tool: {text}");
        assert!(
            !text.contains("─"),
            "slim error cards should not render full bordered cards: {text}"
        );
    }

    fn slim_completed_tool_card_renders_compact_payload() {
        let mut seg = Segment::tool_card("tool-1", "bash");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args = Some("git status --short".into());
            *detail_result = Some(" M src/tui/segments.rs\n M CHANGELOG.md".into());
        }

        let (area, mut buf) = make_buf(100, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("git"), "{text}");
        assert!(text.contains("git status --short"), "{text}");
        assert!(text.contains("2 lines · M src/tui/segments.rs"), "{text}");
        assert!(text.contains("Ctrl+O details"), "{text}");
    }

    #[test]
    fn slim_running_tool_card_uses_indented_live_rows() {
        let mut seg = Segment::tool_card("tool-1", "bash");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            live_partial,
            started_at,
            ..
        } = &mut seg.content
        {
            *complete = false;
            *detail_args =
                Some("git -C /Users/wilson/workspace/styrene-labs/eidolon status --short".into());
            *live_partial = Some(Box::new(omegon_traits::PartialToolResult::content(
                " M crates/eidolon-core/src/lib.rs\n M crates/eidolon-parser/src/lib.rs\n",
                1_200,
            )));
            *started_at = Some(std::time::Instant::now());
        }

        let height = seg.height_in_mode(72, &Alpharius, SegmentRenderMode::Slim);
        assert!(
            height > 1,
            "expected running slim tool to show live detail rows"
        );

        let (area, mut buf) = make_buf(72, height);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("git"), "{text}");
        assert!(text.contains("running"), "{text}");
        assert!(text.contains("├") || text.contains("└"), "{text}");
        assert!(text.contains("git -C /Users/wilson/workspace"), "{text}");
        assert!(text.contains("Ctrl+O details"), "{text}");
    }

    #[test]
    fn slim_completed_tool_card_collapses_long_payload_to_one_row() {
        let mut seg = Segment::tool_card("tool-1", "bash");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args =
                Some("git -C /Users/wilson/workspace/styrene-labs/eidolon status --short".into());
            *detail_result = Some(
                " M crates/eidolon-core/src/lib.rs\n M crates/eidolon-parser/src/lib.rs\n".into(),
            );
        }

        assert_eq!(
            seg.height_in_mode(72, &Alpharius, SegmentRenderMode::Slim),
            1
        );

        let (area, mut buf) = make_buf(72, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("git"), "{text}");
        assert!(!text.contains("├") && !text.contains("└"), "{text}");
        assert!(
            text.contains("Ctrl+O details") || text.contains("…"),
            "{text}"
        );
    }

    #[test]
    fn slim_completed_tool_card_extracts_json_shell_command() {
        let mut seg = Segment::tool_card("tool-1", "bash");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args = Some(r#"{"command":"diskutil list /dev/disk4"}"#.into());
            *detail_result = Some("/dev/disk4 external physical\n62.9 GB\nRemovable".into());
        }

        let (area, mut buf) = make_buf(120, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("diskutil"), "{text}");
        assert!(text.contains("diskutil list /dev/disk4"), "{text}");
        assert!(
            text.contains("3 lines · /dev/disk4 external physical"),
            "{text}"
        );
    }

    #[test]
    fn slim_completed_tool_card_summarizes_read_target_and_output() {
        let mut seg = Segment::tool_card("tool-1", "read");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args = Some(
                r#"{"path":"/Users/wilson/project/src/ops/forge.rs","offset":40,"limit":20}"#
                    .into(),
            );
            *detail_result = Some("fn forge() {}\nlet disk = target;\nwrite_bundle();".into());
        }

        let (area, mut buf) = make_buf(140, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("/Users/wilson/project/src/ops/forge.rs"),
            "{text}"
        );
        assert!(text.contains("@40"), "{text}");
        assert!(text.contains("limit 20"), "{text}");
        assert!(text.contains("3 lines · fn forge() {}"), "{text}");
    }

    #[test]
    fn slim_completed_tool_card_summarizes_validate_scope() {
        let mut seg = Segment::tool_card("tool-1", "validate");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args = Some(
                r#"{"paths":["src/main.rs","src/lib.rs","docs/readme.md"],"source_type":"rust"}"#
                    .into(),
            );
            *detail_result = Some("unsupported source type: markdown".into());
        }

        let (area, mut buf) = make_buf(120, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("validate"), "{text}");
        assert!(text.contains("src/main.rs"), "{text}");
        assert!(text.contains("src/lib.rs"), "{text}");
        assert!(text.contains("docs/readme.md"), "{text}");
        assert!(text.contains("rust"), "{text}");
        assert!(text.contains("unsupported source type: markdown"), "{text}");
    }

    #[test]
    fn slim_completed_tool_card_summarizes_terminal_target() {
        let mut seg = Segment::tool_card("tool-1", "terminal");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            detail_result,
            ..
        } = &mut seg.content
        {
            *complete = true;
            *detail_args =
                Some(r#"{"action":"read","session_id":"forge-build","max_bytes":4096}"#.into());
            *detail_result = Some(
                "Terminal 'forge-build' (abc) — running\nTranscript: /tmp/t.log\n\nready".into(),
            );
        }

        let (area, mut buf) = make_buf(140, 1);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("terminal"), "{text}");
        assert!(text.contains("read · forge-build · 4096 bytes"), "{text}");
        assert!(text.contains("ready"), "{text}");
        assert!(text.contains("/tmp/t.log"), "{text}");
    }

    #[test]
    fn slim_running_tool_card_renders_live_evidence_as_indented_rows() {
        let partial = omegon_traits::PartialToolResult {
            tail: "downloading NixOS minimal ISO...\ncopying closure paths...\nbundle ready".into(),
            progress: omegon_traits::ToolProgress {
                elapsed_ms: 11_400,
                heartbeat: false,
                phase: Some("bundling".into()),
                units: Some(omegon_traits::ProgressUnits {
                    current: 2,
                    total: Some(3),
                    unit: "steps".into(),
                }),
                tally: None,
            },
            details: serde_json::json!(null),
        };
        let mut seg = Segment::tool_card("tool-1", "bash");
        if let SegmentContent::ToolCard {
            complete,
            detail_args,
            live_partial,
            started_at,
            ..
        } = &mut seg.content
        {
            *complete = false;
            *detail_args = Some(r#"{"command":"nex forge --disk /dev/disk4"}"#.into());
            *live_partial = Some(Box::new(partial));
            *started_at = None;
        }

        let height = seg.height_in_mode(120, &Alpharius, SegmentRenderMode::Slim);
        assert!(height > 1, "running tool should show live child rows");
        let (area, mut buf) = make_buf(140, height);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("nex forge --disk /dev/disk4"), "{text}");
        assert!(text.contains("bundling"), "{text}");
        assert!(text.contains("2/3 steps"), "{text}");
        assert!(text.contains("11.4s"), "{text}");
        assert!(text.contains("bundle ready"), "{text}");
        assert!(text.contains("├") || text.contains("└"), "{text}");
        assert!(text.contains("Ctrl+O details"), "{text}");
    }

    #[test]
    fn slim_plan_progress_has_zero_scrollback_height() {
        let seg = Segment::system("Plan progress\nProgress: 1/2\n\n1. ◐ Do it");
        assert_eq!(
            seg.height_in_mode(80, &Alpharius, SegmentRenderMode::Slim),
            0
        );
    }

    #[test]
    fn system_notifications_render_as_rounded_cards_not_legacy_left_banners() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::SystemNotification {
                text: "⚠ Provider connected — active route anthropic:claude-sonnet-4-6".into(),
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        assert!(
            text.contains("╭") || text.contains("╮"),
            "system segment should use rounded card chrome: {text}"
        );
        assert!(
            text.contains("Provider connected"),
            "system message body should render: {text}"
        );
        assert!(
            !text.contains("▎"),
            "legacy left-banner accent bar should be gone: {text}"
        );
    }

    #[test]
    fn token_usage_format_compact_uses_k_and_m_suffixes() {
        assert_eq!(
            TokenUsage {
                input: 0,
                output: 0
            }
            .format_compact(),
            "↑0 ↓0"
        );
        assert_eq!(
            TokenUsage {
                input: 999,
                output: 1
            }
            .format_compact(),
            "↑999 ↓1"
        );
        assert_eq!(
            TokenUsage {
                input: 1_234,
                output: 567
            }
            .format_compact(),
            "↑1.2k ↓567"
        );
        assert_eq!(
            TokenUsage {
                input: 12_500,
                output: 1_000
            }
            .format_compact(),
            "↑12.5k ↓1.0k"
        );
        assert_eq!(
            TokenUsage {
                input: 1_500_000,
                output: 250_000
            }
            .format_compact(),
            "↑1.5M ↓250.0k"
        );
    }

    #[test]
    fn tool_card_title_renders_token_annotation_when_meta_carries_tokens() {
        // The title-bar right-aligned area should show
        // `↑input ↓output · timestamp` when the segment carries
        // actual_tokens (stamped after TurnEnd).
        let meta = SegmentMeta {
            timestamp: Some(std::time::SystemTime::UNIX_EPOCH),
            actual_tokens: Some(TokenUsage {
                input: 1_500,
                output: 240,
            }),
            ..SegmentMeta::default()
        };
        let seg = Segment {
            meta,
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("echo hi".into()),
                result_summary: None,
                detail_result: Some("hi".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("↑1.5k"),
            "tool card title should show input token count: {text}"
        );
        assert!(
            text.contains("↓240"),
            "tool card title should show output token count: {text}"
        );
    }

    #[test]
    fn tool_card_title_omits_token_annotation_when_meta_has_none() {
        // Segments that don't yet have actual_tokens stamped (in-flight,
        // pre-TurnEnd) should NOT show the annotation, just the
        // timestamp on the right rail.
        let seg = Segment {
            meta: SegmentMeta {
                timestamp: Some(std::time::SystemTime::UNIX_EPOCH),
                actual_tokens: None,
                ..SegmentMeta::default()
            },
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("echo hi".into()),
                result_summary: None,
                detail_result: Some("hi".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            !text.contains("↑") && !text.contains("↓"),
            "no token annotation should appear when actual_tokens is None: {text}"
        );
    }

    #[test]
    fn assistant_text_segment_renders_token_annotation_too() {
        // The same right-rail combine logic via top_right_timestamp.
        let seg = Segment {
            meta: SegmentMeta {
                timestamp: Some(std::time::SystemTime::UNIX_EPOCH),
                actual_tokens: Some(TokenUsage {
                    input: 12_345,
                    output: 678,
                }),
                ..SegmentMeta::default()
            },
            content: SegmentContent::AssistantText {
                text: "ok".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(80, 6);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("↑12.3k"),
            "assistant segment title should show input tokens: {text}"
        );
        assert!(
            text.contains("↓678"),
            "assistant segment title should show output tokens: {text}"
        );
    }

    #[test]
    fn edit_tool_card_renders_colored_diff_in_place_of_boring_result() {
        // The edit tool's text result is just "Successfully replaced
        // text in {path}". The renderer should swap that for a real
        // line-by-line diff built from the args' oldText/newText.
        let args = serde_json::json!({
            "path": "src/lib.rs",
            "oldText": "fn old() {\n    println!(\"old\");\n}",
            "newText": "fn new() {\n    println!(\"new\");\n    println!(\"extra\");\n}",
        })
        .to_string();
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "edit".into(),
                args_summary: None,
                detail_args: Some(args),
                result_summary: None,
                detail_result: Some("Successfully replaced text in src/lib.rs".into()),
                is_error: false,
                complete: true,
                expanded: true,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 20);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        // Diff summary line: total +N -M counts.
        assert!(
            text.contains("+4") && text.contains("-3"),
            "diff summary should report 4 additions and 3 removals: {text}"
        );

        // Diff body: removed lines prefixed with `-`, added with `+`.
        assert!(
            text.contains("- fn old() {"),
            "removed line should appear with - prefix: {text}"
        );
        assert!(
            text.contains("+ fn new() {"),
            "added line should appear with + prefix: {text}"
        );
        assert!(
            text.contains("+ "),
            "diff section should have added lines: {text}"
        );

        // The boring "Successfully replaced" text should NOT leak into
        // the rendering — the diff replaces it.
        assert!(
            !text.contains("Successfully replaced"),
            "diff renderer should replace the boring result text: {text}"
        );
    }

    #[test]
    fn change_tool_card_renders_per_file_diff_blocks_with_headers() {
        // The change tool can edit multiple files in one call. Each
        // file gets a header row above its diff hunk.
        let args = serde_json::json!({
            "edits": [
                {
                    "file": "src/a.rs",
                    "oldText": "let a = 1;",
                    "newText": "let a = 2;",
                },
                {
                    "file": "src/b.rs",
                    "oldText": "let b = 1;",
                    "newText": "let b = 2;\nlet c = 3;",
                },
            ],
        })
        .to_string();
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "change".into(),
                args_summary: None,
                detail_args: Some(args),
                result_summary: None,
                detail_result: Some("Changed 2 files".into()),
                is_error: false,
                complete: true,
                expanded: true,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 24);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        // Multi-file: per-file headers with the ▸ glyph and the path.
        assert!(
            text.contains("▸ src/a.rs"),
            "first file header missing: {text}"
        );
        assert!(
            text.contains("▸ src/b.rs"),
            "second file header missing: {text}"
        );
        // Summary line: 2 edits, +3 added, -2 removed
        assert!(
            text.contains("2 edit") && text.contains("+3") && text.contains("-2"),
            "summary should report 2 edits, +3 / -2: {text}"
        );
        // Per-file diff content
        assert!(text.contains("- let a = 1;"));
        assert!(text.contains("+ let a = 2;"));
        assert!(text.contains("- let b = 1;"));
        assert!(text.contains("+ let b = 2;"));
        assert!(text.contains("+ let c = 3;"));
    }

    #[test]
    fn collapsed_edit_card_truncates_diff_with_marker() {
        // Collapsed edit cards cap at 8 diff lines and append a
        // truncation marker showing how many were dropped.
        let old_text = (0..30)
            .map(|i| format!("old line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new_text = (0..30)
            .map(|i| format!("new line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let args = serde_json::json!({
            "path": "big.rs",
            "oldText": old_text,
            "newText": new_text,
        })
        .to_string();
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "edit".into(),
                args_summary: None,
                detail_args: Some(args),
                result_summary: None,
                detail_result: Some("Successfully replaced text in big.rs".into()),
                is_error: false,
                complete: true,
                expanded: false, // collapsed
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 20);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("more diff line"),
            "collapsed cards should show a truncation marker: {text}"
        );
        assert!(
            text.contains("expand for full diff"),
            "collapsed cards should hint at expansion in the summary: {text}"
        );
    }

    #[test]
    fn build_edit_diff_blocks_handles_edit_and_change_shapes() {
        // edit shape: single block from path/oldText/newText
        let edit_args = r#"{"path":"a.rs","oldText":"x","newText":"y"}"#;
        let edit_blocks = build_edit_diff_blocks("edit", edit_args).unwrap();
        assert_eq!(edit_blocks.len(), 1);
        assert_eq!(edit_blocks[0].file, "a.rs");
        assert_eq!(edit_blocks[0].old_text, "x");
        assert_eq!(edit_blocks[0].new_text, "y");

        // change shape: array of edits
        let change_args = r#"{"edits":[{"file":"a.rs","oldText":"1","newText":"2"},{"file":"b.rs","oldText":"3","newText":"4"}]}"#;
        let change_blocks = build_edit_diff_blocks("change", change_args).unwrap();
        assert_eq!(change_blocks.len(), 2);
        assert_eq!(change_blocks[0].file, "a.rs");
        assert_eq!(change_blocks[1].file, "b.rs");

        // Non-edit/change tool: returns None even with valid JSON
        assert!(build_edit_diff_blocks("read", r#"{"path":"a.rs"}"#).is_none());

        // Malformed JSON: returns None
        assert!(build_edit_diff_blocks("edit", "not json").is_none());

        // Edit with missing oldText/newText: returns None
        assert!(build_edit_diff_blocks("edit", r#"{"path":"a.rs"}"#).is_none());
    }

    #[test]
    fn image_placeholder_renders_full_disk_path_without_emoji_glyph() {
        let seg = Segment::image(
            std::path::PathBuf::from("/tmp/omegon-clipboard-78315-16.png"),
            "",
        );
        let (area, mut buf) = make_buf(80, 14);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        // Full disk path is in the title, not just the filename.
        assert!(
            text.contains("/tmp/omegon-clipboard-78315-16.png"),
            "image segment must show the full disk path: {text}"
        );

        // No emoji glyphs — paperclip U+1F4CE was the previous default
        // and is in the Unicode emoji set.
        assert!(
            !text.contains('\u{1F4CE}'),
            "image segment must not use the emoji paperclip glyph"
        );

        // Plain high-contrast edge keeps the segment slim while separating
        // it from image content composited in pass two.
        assert!(
            text.contains('┌') || text.contains('┐') || text.contains('─'),
            "image segment should use a crisp plain frame for visual contrast: {text}"
        );

        // Single-cell crosshatch glyph in the title prefix, in place of
        // the paperclip.
        assert!(
            text.contains('▦'),
            "image segment title should use the ▦ thumbnail glyph: {text}"
        );
    }

    #[test]
    fn image_placeholder_renders_alt_text_with_path_when_provided() {
        let seg = Segment::image(
            std::path::PathBuf::from("/var/captures/screenshot.png"),
            "tui screenshot",
        );
        let (area, mut buf) = make_buf(80, 14);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("tui screenshot"),
            "alt text should appear when provided: {text}"
        );
        assert!(
            text.contains("/var/captures/screenshot.png"),
            "full disk path should appear alongside alt text: {text}"
        );
    }

    #[test]
    fn user_prompt_projects_to_borrowed_semantic_projection() {
        let seg = Segment::user_prompt("hello world");
        let projection = seg.projection();
        assert_eq!(projection.role(), SegmentRole::Operator);
        match projection.kind {
            ConversationSegmentKind::User(user) => assert_eq!(user.text, "hello world"),
            other => panic!("expected user projection, got {other:?}"),
        }
    }

    #[test]
    fn assistant_projects_completion_state() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "answer".into(),
                thinking: "scratch".into(),
                complete: false,
            },
        };
        let projection = seg.projection();
        assert_eq!(projection.role(), SegmentRole::Assistant);
        match projection.kind {
            ConversationSegmentKind::Assistant(assistant) => {
                assert_eq!(assistant.text, "answer");
                assert_eq!(assistant.thinking, "scratch");
                assert!(!assistant.complete);
            }
            other => panic!("expected assistant projection, got {other:?}"),
        }
    }

    #[test]
    fn tool_projects_client_visible_fields() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "tool-1".into(),
                name: "bash".into(),
                args_summary: Some("cargo check".into()),
                detail_args: Some("cargo check -p omegon".into()),
                result_summary: Some("ok".into()),
                detail_result: Some("finished".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let projection = seg.projection();
        assert_eq!(projection.role(), SegmentRole::Tool);
        assert_eq!(
            projection.presentation.tool_category,
            Some(ToolCategory::CommandExec)
        );
        match projection.kind {
            ConversationSegmentKind::Tool(tool) => {
                assert_eq!(tool.id, "tool-1");
                assert_eq!(tool.name, "bash");
                assert_eq!(tool.args_summary, Some("cargo check"));
                assert_eq!(tool.detail_args, Some("cargo check -p omegon"));
                assert_eq!(tool.result_summary, Some("ok"));
                assert_eq!(tool.detail_result, Some("finished"));
                assert!(!tool.is_error);
                assert!(tool.complete);
                assert!(!tool.expanded);
            }
            other => panic!("expected tool projection, got {other:?}"),
        }
    }

    #[test]
    fn image_projects_borrowed_path_and_alt_text() {
        let seg = Segment::image(std::path::PathBuf::from("/tmp/screenshot.png"), "screen");
        let projection = seg.projection();
        assert_eq!(projection.role(), SegmentRole::Media);
        match projection.kind {
            ConversationSegmentKind::Image(image) => {
                assert_eq!(image.path, std::path::Path::new("/tmp/screenshot.png"));
                assert_eq!(image.alt, "screen");
            }
            other => panic!("expected image projection, got {other:?}"),
        }
    }

    #[test]
    fn user_prompt_renders() {
        let seg = Segment::user_prompt("hello world");
        let (area, mut buf) = make_buf(40, 5);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert_eq!(seg.role(), SegmentRole::Operator);
        assert_eq!(seg.presentation().sigil, "OP");
        assert!(text.contains("hello world"), "should have text");
        assert!(
            text.contains("╭") || text.contains("╰") || text.contains("│"),
            "should render as a bordered card: {text}"
        );
        let op_count = text.match_indices("OP").count();
        assert!(
            op_count <= 1,
            "operator card should not duplicate the OP sigil in both title and body: {text}"
        );
    }

    #[test]
    fn assistant_segment_has_explicit_presentation_role() {
        let seg = Segment::assistant_text();
        assert_eq!(seg.role(), SegmentRole::Assistant);
        assert_eq!(seg.presentation().sigil, "Ω");
        assert_eq!(seg.presentation().emphasis, SegmentEmphasis::Normal);
        assert_eq!(seg.presentation().tool_category, None);
    }

    #[test]
    fn tool_categories_are_classified() {
        let cases = [
            (Segment::tool_card("1", "read"), ToolCategory::FileRead),
            (Segment::tool_card("1", "bash"), ToolCategory::CommandExec),
            (
                Segment::tool_card("1", "design_tree"),
                ToolCategory::DesignTree,
            ),
            (
                Segment::tool_card("1", "memory_query"),
                ToolCategory::Memory,
            ),
            (Segment::tool_card("1", "web_search"), ToolCategory::Search),
            (Segment::tool_card("1", "write"), ToolCategory::FileMutation),
        ];
        for (seg, expected) in cases {
            assert_eq!(seg.presentation().tool_category, Some(expected));
        }
    }

    #[test]
    fn assistant_render_includes_identity_header() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "reply text".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(40, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("Ω"),
            "assistant header should include Ω sigil: {text}"
        );
        assert!(
            text.contains("omegon"),
            "assistant header should identify omegon as the source: {text}"
        );
        assert!(
            text.contains("answer"),
            "assistant content should label the answer block explicitly: {text}"
        );
        assert!(
            text.contains("╭") || text.contains("╰") || text.contains("│"),
            "assistant response should now render as a card: {text}"
        );
    }

    #[test]
    fn header_timestamp_formats_as_clock_time() {
        let formatted = format_timestamp(Some(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(13 * 3600 + 5 * 60),
        ))
        .expect("timestamp should format");
        // Format is HH:MM:SS (8 chars)
        assert_eq!(
            formatted.len(),
            8,
            "expected HH:MM:SS format, got: {formatted}"
        );
        assert_eq!(&formatted[2..3], ":");
        assert_eq!(&formatted[5..6], ":");
        assert!(formatted.chars().take(2).all(|c| c.is_ascii_digit()));
        assert!(
            formatted
                .chars()
                .skip(3)
                .take(2)
                .all(|c| c.is_ascii_digit())
        );
        assert!(formatted.chars().skip(6).all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn edit_tool_card_summarizes_args_instead_of_dumping_raw_json() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "edit".into(),
                args_summary: None,
                detail_args: Some(
                    r#"{"file":"src/main.rs","oldText":"a\nb","newText":"c\nd\ne"}"#.into(),
                ),
                result_summary: None,
                detail_result: Some("ok".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("src/main.rs"),
            "edit cards should summarize the file path: {text}"
        );
        assert!(
            text.contains("2→3 lines"),
            "edit cards should summarize line counts: {text}"
        );
        assert!(
            !text.contains("oldText"),
            "edit cards should not dump raw JSON keys into the card header: {text}"
        );
    }

    #[test]
    fn change_tool_card_summarizes_multi_file_edits_without_raw_json_noise() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "change".into(),
                args_summary: None,
                detail_args: Some(
                    r#"{"edits":[{"file":"src/main.rs","oldText":"a","newText":"b"},{"file":"src/lib.rs","oldText":"c","newText":"d"}],"validate":"cargo test"}"#.into(),
                ),
                result_summary: None,
                detail_result: Some("ok".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(90, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("src/main.rs"),
            "change cards should show a real file path: {text}"
        );
        assert!(
            text.contains("2 edits"),
            "change cards should summarize edit count: {text}"
        );
        assert!(
            !text.contains("oldText"),
            "change cards should not leak raw JSON keys: {text}"
        );
        assert!(
            !text.contains("\"edits\""),
            "change cards should not render the raw JSON payload: {text}"
        );
    }

    #[test]
    fn tool_result_highlight_rows_fill_full_card_background() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("/tmp/demo.rs".into()),
                result_summary: None,
                detail_result: Some("fn demo() {\n    println!(\"hi\");\n}".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 12);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );

        let code_row = find_row_containing(&buf, area, "println!").expect("code row in buffer");
        let trailing_content_cell = &buf[(area.right() - 3, code_row)];
        assert_eq!(
            trailing_content_cell.style().bg,
            Some(Alpharius.tool_success_bg())
        );
    }

    #[test]
    fn assistant_markdown_rows_inherit_segment_background() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "plain text with `inline code`".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );

        let row =
            find_row_containing(&buf, area, "plain text with").expect("assistant row in buffer");
        let trailing_content_cell = &buf[(area.right() - 3, row)];
        assert_eq!(
            trailing_content_cell.style().bg,
            Some(Alpharius.surface_bg())
        );
    }

    #[test]
    fn tool_result_markdown_tables_render_as_structured_rows() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "codebase_search".into(),
                args_summary: None,
                detail_args: Some("{\"query\":\"foo\"}".into()),
                result_summary: None,
                detail_result: Some(
                    "## codebase_search: `foo`\n\n**2 result(s)** (scope: `code`)\n\n- `src/app.rs`:10-20 · code · score 45.38\n    fn render()\n\n- `src/lib.rs`:1-9 · code · score 11.20\n    helper\n"
                        .into(),
                ),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(100, 16);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("codebase_search: foo"),
            "heading prose should render human-readably, not as raw markdown: {text}"
        );
        assert!(
            !text.contains("## codebase_search"),
            "heading marker should not leak literally into the rendered card: {text}"
        );
        assert!(
            text.contains("2 result(s) (scope: code)"),
            "summary prose should render without literal markdown markers: {text}"
        );
        assert!(
            !text.contains("**2 result(s)**"),
            "bold markers should not leak literally into the rendered card: {text}"
        );
        for body_cell in [
            "src/app.rs",
            "10-20",
            "45.38",
            "fn render()",
            "src/lib.rs",
            "1-9",
            "11.20",
            "helper",
        ] {
            assert!(
                text.contains(body_cell),
                "body should contain cell {body_cell:?}: {text}"
            );
        }
        assert!(
            text.contains("score 45.38") && text.contains("score 11.20"),
            "search results should render as compact line-oriented blocks: {text}"
        );
    }

    #[test]
    fn incomplete_assistant_renders_full_reasoning_live() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: String::new(),
                thinking: "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8".into(),
                complete: false,
            },
        };
        let (area, mut buf) = make_buf(60, 16);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("omegon"),
            "assistant header should name omegon as the source: {text}"
        );
        assert!(
            text.contains("reasoning"),
            "reasoning block should be labeled explicitly: {text}"
        );
        assert!(
            text.contains("l8"),
            "live reasoning should render the tail: {text}"
        );
        assert!(
            !text.contains("⋯ 2 more"),
            "incomplete assistant reasoning should not be collapsed: {text}"
        );
    }

    #[test]
    fn complete_assistant_collapses_long_reasoning_summary_and_labels_answer() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "done".into(),
                thinking: "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8".into(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(60, 16);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("omegon"),
            "assistant header should remain stable after completion: {text}"
        );
        assert!(
            text.contains("reasoning"),
            "reasoning block should stay labeled after completion: {text}"
        );
        assert!(
            text.contains("answer"),
            "answer block should be labeled explicitly: {text}"
        );
        assert!(
            text.contains("l6"),
            "collapsed reasoning should keep the preview: {text}"
        );
        assert!(
            text.contains("⋯ 2 more"),
            "collapsed reasoning should show a summary hint: {text}"
        );
    }

    #[test]
    fn user_prompt_preserves_multiline_and_trailing_blank_lines() {
        let seg = Segment::user_prompt("alpha\nbeta\n\n");
        let (area, mut buf) = make_buf(30, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("alpha"), "first line should render: {text}");
        assert!(text.contains("beta"), "second line should render: {text}");
        assert!(
            seg.height(30, &Alpharius) >= 5,
            "multiline prompt should reserve height for blank lines"
        );
    }

    #[test]
    fn assistant_text_trims_gratuitous_trailing_blank_lines() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "alpha\nbeta\n\n".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(30, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("alpha"), "first line should render: {text}");
        assert!(text.contains("beta"), "second line should render: {text}");
        assert!(
            !text.contains("beta\n\n\n"),
            "assistant segment should not keep gratuitous trailing blank rows: {text}"
        );
    }

    #[test]
    fn assistant_markdown_tables_render_box_drawing_rows() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "| Name | Value |\n| ---- | ----- |\n| foo | bar |".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(40, 10);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        // Cell content checks (padding is determined by shared column
        // widths from compute_table_widths and shouldn't be locked in
        // by tests).
        for cell in ["Name", "Value", "foo", "bar"] {
            assert!(
                text.contains(cell),
                "table should contain cell {cell:?}: {text}"
            );
        }
        assert!(
            text.contains("│"),
            "table should render with box-drawing pipes: {text}"
        );
        assert!(
            text.contains("├") || text.contains("┼"),
            "separator row should render box drawing characters: {text}"
        );
    }

    #[test]
    fn assistant_markdown_tables_survive_surrounding_prose() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "Here are the strongest matches:\n\n| File | Score |\n| ---- | ----- |\n| src/app.rs | 45.38 |\n| src/lib.rs | 11.20 |\n\nUse `read` for the top result.".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(70, 14);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("Here are the strongest matches:"),
            "leading prose should remain visible: {text}"
        );
        for cell in [
            "File",
            "Score",
            "src/app.rs",
            "45.38",
            "src/lib.rs",
            "11.20",
        ] {
            assert!(
                text.contains(cell),
                "table should contain cell {cell:?}: {text}"
            );
        }
        // Cross-row alignment check: both body rows must start at the
        // same column. Header `File` is 4 chars; body `src/app.rs` is
        // 10 chars. The pre-pass widens the File column to 10 across
        // the whole block, so the header gets padding and both body
        // rows align with each other.
        let row1 = text
            .find("src/app.rs")
            .expect("first body row should be present");
        let row2 = text
            .find("src/lib.rs")
            .expect("second body row should be present");
        let col1 = row1 - text[..row1].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col2 = row2 - text[..row2].rfind('\n').map(|i| i + 1).unwrap_or(0);
        assert_eq!(
            col1, col2,
            "body rows must align across the table block: row1 col={col1} row2 col={col2}"
        );
        assert!(
            text.contains("Use "),
            "trailing prose should remain visible: {text}"
        );
    }

    #[test]
    fn in_flight_tool_card_renders_live_tail_and_status_header() {
        // Construct a still-in-flight bash card with a streaming partial
        // carrying line counts, elapsed time, and tail content. The card
        // should render the tail (last few lines) and a status header
        // showing units + elapsed — replacing the empty body that the
        // pre-streaming code would have shown for an in-flight tool.
        let partial = omegon_traits::PartialToolResult {
            tail: "compiling foo v0.1.0\ncompiling bar v0.2.1\ncompiling baz v0.3.4\nlinking target/debug/myapp".to_string(),
            progress: omegon_traits::ToolProgress {
                elapsed_ms: 12_300,
                heartbeat: false,
                phase: None,
                units: Some(omegon_traits::ProgressUnits {
                    current: 4,
                    total: None,
                    unit: "lines".to_string(),
                }),
                tally: None,
            },
            details: serde_json::json!(null),
        };
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("cargo build".into()),
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: Some(Box::new(partial)),
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 18);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        // Status header populated from progress fields
        assert!(
            text.contains("running"),
            "status header should show 'running' fallback phase: {text}"
        );
        assert!(
            text.contains("4 lines"),
            "status header should show units count from partial: {text}"
        );
        assert!(
            text.contains("12.3s"),
            "status header should show elapsed time from partial: {text}"
        );

        // Tail content from the partial — the last lines, not the first
        assert!(
            text.contains("linking"),
            "live tail should render most recent line: {text}"
        );
        assert!(
            text.contains("compiling baz"),
            "live tail should render recent compile lines: {text}"
        );
    }

    #[test]
    fn in_flight_tool_card_with_no_partial_renders_running_placeholder() {
        // Before any partial arrives, the card should still show a
        // "▶ running" status line so the operator sees something
        // instead of an empty body.
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("sleep 30".into()),
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("running"),
            "in-flight card with no partial should show 'running' placeholder: {text}"
        );
    }

    #[test]
    fn in_flight_tool_card_strips_raw_ansi_bytes_from_live_tail() {
        // Bash output is allowed to carry SGR color escapes (the
        // strip_terminal_noise pass in tools/bash.rs deliberately
        // preserves them for downstream colorization). Without the
        // ansi_to_tui parse on the live tail, those raw ESC bytes
        // would write into the cell buffer and the terminal would
        // misinterpret them — the operator's screenshot showed the
        // resulting fragment leakage in the right-side instruments
        // panel. This test pins the protection: a tail carrying ESC
        // sequences should render as the visible text only, no raw
        // control bytes anywhere in the rendered cells.
        let partial = omegon_traits::PartialToolResult {
            tail: "\x1b[32mcompiling foo\x1b[0m\nlinking target/debug/myapp".to_string(),
            progress: omegon_traits::ToolProgress {
                elapsed_ms: 1_500,
                heartbeat: false,
                phase: None,
                units: None,
                tally: None,
            },
            details: serde_json::json!(null),
        };
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("cargo build".into()),
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: Some(Box::new(partial)),
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 12);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );

        // Walk every cell and assert no control char ended up in the
        // buffer. The visible content should still be present.
        let text = buf_text(&buf, area);
        assert!(
            text.contains("compiling foo"),
            "visible content should survive: {text}"
        );
        assert!(
            text.contains("linking"),
            "second tail line should render: {text}"
        );
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let sym = buf[(x, y)].symbol();
                for ch in sym.chars() {
                    assert!(
                        !ch.is_control(),
                        "rendered cell at ({x}, {y}) contains control char {ch:?} (U+{:04X})",
                        ch as u32
                    );
                }
            }
        }
        // The literal `[32m` and `[0m` SGR parameter strings should
        // NOT appear as visible text either — ansi_to_tui consumes
        // them and applies the styling instead.
        assert!(
            !text.contains("[32m") && !text.contains("[0m"),
            "ANSI parameter sequences should be parsed away, not rendered as text: {text}"
        );
    }

    #[test]
    fn in_flight_tool_card_uses_wall_clock_when_started_at_set() {
        // When `started_at` is populated, the displayed elapsed timer
        // should reflect the wall-clock since that instant — NOT the
        // partial's `elapsed_ms` field. This is the fix for "timer
        // freezes between partials" — bash can go 5 seconds quiet
        // between idle heartbeats, but the displayed timer should
        // keep ticking on every frame draw.
        //
        // Construct a card with `started_at` set 8 seconds in the past
        // and a partial whose internal `elapsed_ms` says only 2 seconds
        // (i.e. the partial was emitted early in the run and is now
        // stale). The rendered output should show ~8s, not 2s.
        let started_in_past = std::time::Instant::now() - std::time::Duration::from_secs(8);
        let stale_partial = omegon_traits::PartialToolResult {
            tail: "still working".to_string(),
            progress: omegon_traits::ToolProgress {
                elapsed_ms: 2_000, // stale: from when the partial was emitted
                heartbeat: false,
                phase: None,
                units: None,
                tally: None,
            },
            details: serde_json::json!(null),
        };
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("sleep 60".into()),
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: Some(Box::new(stale_partial)),
                started_at: Some(started_in_past),
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("8.0s") || text.contains("8.1s") || text.contains("7.9s"),
            "wall-clock should override stale partial elapsed_ms (~8s expected, not 2.0s): {text}"
        );
        assert!(
            !text.contains("2.0s"),
            "stale partial elapsed_ms (2.0s) should NOT appear when started_at is set: {text}"
        );
    }

    #[test]
    fn in_flight_tool_card_renders_idle_marker_for_heartbeat_partials() {
        // Heartbeat partials carry no tail content, just a "still alive"
        // signal. The status header should mark the card as idle so
        // operators know the tool is alive but not actively producing
        // output (vs. wedged with no signal at all).
        let partial = omegon_traits::PartialToolResult {
            tail: String::new(),
            progress: omegon_traits::ToolProgress {
                elapsed_ms: 6_000,
                heartbeat: true,
                phase: None,
                units: None,
                tally: None,
            },
            details: serde_json::json!(null),
        };
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("sleep 30".into()),
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: Some(Box::new(partial)),
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("idle"),
            "heartbeat partial should render 'idle' marker: {text}"
        );
        assert!(
            text.contains("6.0s"),
            "heartbeat should still surface elapsed_ms: {text}"
        );
    }

    #[test]
    fn tool_result_markdown_tables_truncate_wide_preview_cells_in_narrow_cards() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "codebase_search".into(),
                args_summary: None,
                detail_args: Some("{\"query\":\"foo\"}".into()),
                result_summary: None,
                detail_result: Some(
                    "## codebase_search: `foo`\n\n**1 result(s)** (scope: `code`)\n\n| File | Lines | Type | Score | Preview |\n|------|-------|------|-------|---------|\n| `core/crates/omegon/src/tui/tests.rs` | 1163-1177 | code | 16.22 | fn slash_context_request_dispatches_direct_context_pack() { · let mut app = test_app(); · let tx = test_tx(); |\n"
                        .into(),
                ),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(90, 18);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("│ File"),
            "table header should still render: {text}"
        );
        assert!(
            text.contains("Preview"),
            "preview column should remain visible: {text}"
        );
        assert!(
            text.contains("… │") || text.contains("…│"),
            "wide preview cell should be truncated instead of wrapping the whole row: {text}"
        );
        assert!(
            !text.contains("let mut app = test_app();"),
            "overflow preview content should not spill into wrapped continuation lines: {text}"
        );
    }

    #[test]
    fn assistant_markdown_tables_accept_aligned_separator_rows() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "| Name | Value | Notes |\n| ---- | :----: | ----- |\n| foo | bar | baz |"
                    .into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(60, 12);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        // Aligned-separator markdown (`:----:`) should still parse as
        // a table — the separator-detection logic accepts colons.
        for cell in ["Name", "Value", "Notes", "foo", "bar", "baz"] {
            assert!(
                text.contains(cell),
                "table should contain cell {cell:?}: {text}"
            );
        }
        assert!(
            text.contains("├") || text.contains("┼"),
            "aligned separator row should still render box drawing characters: {text}"
        );
    }

    #[test]
    fn tool_card_has_borders() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: Some("ls -la".into()),
                detail_args: Some("ls -la".into()),
                result_summary: Some("total 42".into()),
                detail_result: Some("total 42\ndrwxr-xr-x  5 user staff".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(60, 10);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("╭"), "should have top border: {text}");
        assert!(text.contains("╰"), "should have bottom border: {text}");
        assert!(
            text.contains("list"),
            "should have display name for ls: {text}"
        );
        assert!(
            text.contains("▸"),
            "completed tools should use the same teal indicator family as the tool instrument panel: {text}"
        );
    }

    #[test]
    fn read_tool_hyperlink_row_clears_stale_suffix_when_path_shrinks() {
        let long = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("/Users/cwilson/workspace/black-meridian/omegon/core/crates/omegon/src/tui/really_long_filename.rs".into()),
                result_summary: Some("fn main() {}".into()),
                detail_result: Some("fn main() {}".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let short = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("src/tui/mod.rs".into()),
                result_summary: Some("mod tui;".into()),
                detail_result: Some("mod tui;".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };

        let (area, mut buf) = make_buf(72, 8);
        long.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        short.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );

        let row = (1..area.width.saturating_sub(1))
            .map(|x| buf[(x, 1)].symbol())
            .collect::<String>();
        assert!(
            row.contains("src/tui/mod.rs"),
            "short path should render in filename row: {row}"
        );
        assert!(
            !row.contains("really_long_filename"),
            "filename row should not keep stale suffix text from prior render: {row}"
        );
    }

    #[test]
    fn tool_title_truncates_before_timestamp_collision() {
        let seg = Segment {
            meta: SegmentMeta {
                timestamp: Some(std::time::SystemTime::now()),
                ..Default::default()
            },
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some(
                    "/very/long/path/to/some_extremely_verbose_filename_that_used_to_bleed.rs"
                        .into(),
                ),
                result_summary: None,
                detail_result: Some("fn main() {}".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(28, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let top_row = (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>();
        assert!(
            top_row.contains("▸"),
            "top row should retain completed tool icon: {top_row}"
        );
        assert!(
            top_row.contains("read") || top_row.contains("rea…"),
            "top row should retain truncated tool label: {top_row}"
        );
        assert!(
            !top_row.contains("◇ read"),
            "conversation tool titles should not duplicate status and tool icons: {top_row}"
        );
        assert!(
            !top_row.contains("filename_that_used_to_bleed"),
            "long header text should be truncated before colliding with the rest of the title row: {top_row}"
        );
    }

    #[test]
    fn tool_title_redraw_clears_stale_suffix_characters() {
        let long = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some(
                    "/Users/cwilson/workspace/black-meridian/omegon/core/Cargo.toml".into(),
                ),
                result_summary: None,
                detail_result: Some("[package]".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let short = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("package.json".into()),
                result_summary: None,
                detail_result: Some("{}".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };

        let (area, mut buf) = make_buf(24, 8);
        long.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        short.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );

        let top_row = (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>();
        assert!(
            top_row.contains("read"),
            "top row should contain the current tool label: {top_row}"
        );
        assert!(
            top_row.contains("─"),
            "top border should continue after the tool title instead of stopping early: {top_row}"
        );
        assert!(
            !top_row.contains("Cargo.tomlm") && !top_row.contains("package.jsonon"),
            "shorter redraw should not leave stale suffix characters in the title row: {top_row}"
        );
    }

    #[test]
    fn tool_card_error_styling() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "write".into(),
                args_summary: None,
                detail_args: Some("/tmp/test".into()),
                result_summary: None,
                detail_result: Some("permission denied".into()),
                is_error: true,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(60, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("✗"), "should have error icon: {text}");
        assert!(
            text.contains("write"),
            "error cards should use the full tool name in conversation view: {text}"
        );
        assert!(
            !text.contains("◆ write"),
            "conversation view should not duplicate the status icon with a second tool icon: {text}"
        );
    }

    #[test]
    fn running_tool_card_uses_instrument_panel_indicator() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("Cargo.toml".into()),
                result_summary: None,
                detail_result: None,
                is_error: false,
                complete: false,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(50, 8);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("▶"),
            "running tools should use the amber running indicator from the instrument panel: {text}"
        );
        assert!(
            text.contains("read"),
            "running tools should use a readable conversation title: {text}"
        );
        assert!(
            !text.contains("◇ read"),
            "conversation view should not stack a second tool icon after the running indicator: {text}"
        );
    }

    #[test]
    fn assistant_height_preserves_tail_after_narrow_code_fence() {
        let body = "Expected:

- no `openai:gpt-5.5` unless OpenAI API credentials exist
- if stale current model is OpenAI:
```text
openai:gpt-5.5 (current, unavailable)
gpt-5.5
```
After fence text.
";
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: body.into(),
                thinking: String::new(),
                complete: true,
            },
        };

        let height = seg.height_in_mode(72, &Alpharius, SegmentRenderMode::Slim);
        let (area, mut buf) = make_buf(72, height);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        assert!(
            text.contains("openai:gpt-5.5 (current, unavailable)"),
            "{text}"
        );
        assert!(text.contains("After fence text."), "{text}");
    }

    #[test]
    fn assistant_height_preserves_tail_beyond_legacy_400_row_cap() {
        let mut body = String::new();
        for i in 0..520 {
            body.push_str(&format!(
                "line {i:03}: full response content must remain visible\n"
            ));
        }
        body.push_str("FINAL-LINE-SHOULD-BE-VISIBLE");
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: body,
                thinking: String::new(),
                complete: true,
            },
        };

        let height = seg.height_in_mode(96, &Alpharius, SegmentRenderMode::Slim);
        assert!(
            height > 400,
            "long assistant responses must not be clipped to the old 400-row cap: {height}"
        );
        let (area, mut buf) = make_buf(96, height);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("line 519"), "{text}");
        assert!(text.contains("FINAL-LINE-SHOULD-BE-VISIBLE"), "{text}");
    }

    #[test]
    fn assistant_text_with_code_fence() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: "Here's code:\n```rust\nfn main() {}\n```\nDone.".into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(60, 10);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("fn main"), "should have code: {text}");
    }

    #[test]
    fn height_calculation() {
        let t = Alpharius;
        let sep = Segment::separator();
        assert_eq!(sep.height(80, &t), 1);

        let user = Segment::user_prompt("short");
        let h = user.height(80, &t);
        assert!((3..=7).contains(&h), "user prompt height: {h}");

        let tool = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("echo hello".into()),
                result_summary: None,
                detail_result: Some("hello".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let h = tool.height(80, &t);
        assert!(h >= 4, "tool card height should be >= 4, got {h}");
    }

    #[test]
    fn tool_card_height_accounts_for_wrapped_long_lines() {
        let t = Alpharius;
        let long_line = "x".repeat(400);
        let tool = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("echo hello".into()),
                result_summary: None,
                detail_result: Some(long_line),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let h_narrow = tool.height(40, &t);
        let h_wide = tool.height(120, &t);
        assert!(
            h_narrow > h_wide,
            "narrow tool cards should get taller when output wraps"
        );
        assert!(
            h_narrow >= 8,
            "wrapped tool output should materially increase card height: {h_narrow}"
        );
    }

    #[test]
    fn compact_tool_card_does_not_carry_extra_bottom_padding() {
        let t = Alpharius;
        let tool = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "bash".into(),
                args_summary: None,
                detail_args: Some("echo hi".into()),
                result_summary: None,
                detail_result: Some("hi".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let h = tool.height(80, &t);
        assert!(h <= 7, "compact tool cards should stay tight, got {h}");
    }

    #[test]
    fn read_tool_height_uses_compact_file_row_estimate() {
        let t = Alpharius;
        let tool = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("/tmp/example.rs".into()),
                result_summary: None,
                detail_result: Some("short result".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let h = tool.height(80, &t);
        assert!(
            h <= 7,
            "read cards should stay compact when args collapse to a single file row, got {h}"
        );
    }

    #[test]
    fn system_notification_renders() {
        let seg = Segment::system("Tool display → detailed");
        let (area, mut buf) = make_buf(60, 3);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("detailed"), "should show text: {text}");
    }

    #[test]
    fn table_line_detection() {
        assert!(is_table_line("| a | b |"));
        assert!(is_table_line("|---|---|"));
        assert!(is_table_line("| Name | Value |"));
        assert!(!is_table_line("not a table"));
        assert!(!is_table_line("|")); // too short
        assert!(!is_table_line("||")); // too short
    }

    #[test]
    fn table_separator_detection() {
        assert!(is_table_separator("|---|---|"));
        assert!(is_table_separator("| --- | --- |"));
        assert!(is_table_separator("|:---:|:---:|"));
        assert!(!is_table_separator("| a | b |")); // has letters
    }

    #[test]
    fn table_line_renders() {
        // render_table_line now takes pre-computed shared widths from
        // compute_table_widths instead of computing per-row widths.
        let widths = vec![10, 10];
        let line = render_table_line("| Name | Value |", true, &widths, &Alpharius);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            text.contains("Name"),
            "header should contain cell text: {text}"
        );
        assert!(
            text.contains("│"),
            "should contain box drawing separator: {text}"
        );

        let body = render_table_line("| foo | bar |", false, &widths, &Alpharius);
        let body_text: String = body.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            body_text.contains("foo"),
            "body should contain cell text: {body_text}"
        );

        let sep = render_table_line("|---|---|", false, &widths, &Alpharius);
        let sep_text: String = sep.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            sep_text.contains("─"),
            "separator should use rule chars: {sep_text}"
        );
        assert!(
            sep_text.contains("┼"),
            "separator should have cross: {sep_text}"
        );
    }

    #[test]
    fn table_line_detection_accepts_missing_trailing_pipe() {
        // Many LLMs omit the trailing `|` on body rows even when the
        // header row has it. The previous `ends_with('|')` requirement
        // caused these body rows to fall through to the non-table
        // rendering path and disappear. This test pins the relaxed
        // definition.
        assert!(is_table_line("| a | b |")); // full pipes
        assert!(is_table_line("| a | b")); // no trailing pipe
        assert!(is_table_line("| a | b |   ")); // trailing whitespace
        assert!(is_table_line("| a | b   ")); // trailing whitespace, no pipe
        assert!(!is_table_line("| single")); // only one pipe (not a table row)
        assert!(!is_table_line("not a table row")); // no leading pipe
        assert!(!is_table_line("||")); // too short
        assert!(!is_table_line("|")); // too short

        // Separator rows can also miss the trailing pipe
        assert!(is_table_separator("|---|---|")); // full
        assert!(is_table_separator("|---|---")); // no trailing pipe
        assert!(is_table_separator("| --- | ---")); // spaced, no trailing pipe
    }

    #[test]
    fn assistant_table_renders_body_rows_without_trailing_pipes() {
        // The headline failure mode this test pins: the assistant
        // writes a markdown table where the header and separator have
        // trailing `|` but the body rows don't. Previous code showed
        // the header + separator but the body rows were invisible.
        let text = "Results:\n\n| Setting | Endpoint | Filter |\n|---------|----------|--------|\n| stable | /releases | prerelease=false\n| nightly | /releases | prerelease=true";
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::AssistantText {
                text: text.into(),
                thinking: String::new(),
                complete: true,
            },
        };
        let (area, mut buf) = make_buf(80, 16);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);

        // Header cells should render
        for cell in ["Setting", "Endpoint", "Filter"] {
            assert!(
                text.contains(cell),
                "header should contain {cell:?}: {text}"
            );
        }
        // Body cells MUST render — this is the bug being fixed
        assert!(
            text.contains("stable"),
            "first body row should render even without trailing pipe: {text}"
        );
        assert!(
            text.contains("nightly"),
            "second body row should render even without trailing pipe: {text}"
        );
        assert!(
            text.contains("prerelease=false"),
            "body cell content should be visible: {text}"
        );
        assert!(
            text.contains("prerelease=true"),
            "body cell content should be visible: {text}"
        );
    }

    #[test]
    pub(crate) fn compute_table_widths_aligns_columns_across_rows() {
        // The headline failure mode this fix addresses: a header with
        // narrow cells (`File`/`Lines`/`Score`) followed by a body row
        // with very long content in the last column (Preview). The old
        // per-row computation derived widths from the header's short
        // cells, leaving no budget for the body's long content; the
        // body row got truncated independently and rendered out of
        // alignment. With the pre-pass, every row in the same block
        // shares the same widths.
        let lines = vec![
            "| File | Lines | Score | Preview |",
            "|------|-------|-------|---------|",
            "| `core/crates/omegon/src/tui/segments.rs` | 1234-1456 | 9.13 | pub struct Segment { /* very long preview content here */ } |",
        ];
        let widths_per_line = compute_table_widths(&lines, 90);

        // All three lines should be marked as belonging to the same
        // table block.
        assert!(widths_per_line[0].is_some());
        assert!(widths_per_line[1].is_some());
        assert!(widths_per_line[2].is_some());

        // All three should share the SAME widths array (column
        // alignment is the whole point).
        let h = widths_per_line[0].as_ref().unwrap();
        let s = widths_per_line[1].as_ref().unwrap();
        let b = widths_per_line[2].as_ref().unwrap();
        assert_eq!(h, s, "header and separator should share widths");
        assert_eq!(h, b, "header and body should share widths");

        // The first three columns should reflect the body row's actual
        // content (longer than the header's), not the header's
        // labels — that's the cross-row max we're computing.
        // Column widths are now measured after markdown stripping, so
        // `backtick-wrapped` content is measured without the backticks.
        assert!(
            h[0] >= "core/crates/omegon/src/tui/segments.rs".chars().count(),
            "File column should accommodate the body's long file path (stripped): {h:?}"
        );
        assert!(h[1] >= "1234-1456".chars().count());
        assert!(h[2] >= "9.13".chars().count());

        // The last column (Preview) should have been shrunk to fit the
        // available budget rather than blowing past the card width.
        let total: usize = h.iter().sum();
        let chrome = h.len() * 3 + 1;
        assert!(
            total + chrome <= 90,
            "rendered widths must fit available_width=90: total={total} chrome={chrome} widths={h:?}"
        );
    }

    #[test]
    pub(crate) fn compute_table_widths_returns_none_for_non_table_lines() {
        let lines = vec![
            "Some prose before a table",
            "| col1 | col2 |",
            "|------|------|",
            "| a    | b    |",
            "More prose after",
            "And another paragraph",
        ];
        let widths = compute_table_widths(&lines, 80);
        assert!(widths[0].is_none(), "prose line is not a table");
        assert!(widths[1].is_some(), "header line is a table");
        assert!(widths[2].is_some(), "separator line is a table");
        assert!(widths[3].is_some(), "body line is a table");
        assert!(widths[4].is_none(), "trailing prose is not a table");
        assert!(widths[5].is_none());
    }

    #[test]
    pub(crate) fn compute_table_widths_handles_multiple_blocks() {
        // Two separate table blocks with prose in between. Each block
        // should compute its own widths independently.
        let lines = vec![
            "| a | b |",
            "|---|---|",
            "| 1 | 2 |",
            "",
            "intervening prose",
            "",
            "| longer-header | wider |",
            "|---------------|-------|",
            "| x             | y     |",
        ];
        let widths = compute_table_widths(&lines, 80);
        let block1 = widths[0].as_ref().unwrap();
        let block2 = widths[6].as_ref().unwrap();
        assert_ne!(
            block1, block2,
            "two separate table blocks should compute independent widths"
        );
        // Block 1 first column = max("a", "1") = 1 char
        assert_eq!(block1[0], 1);
        // Block 2 first column = max("longer-header", "x") = 13 chars
        assert_eq!(block2[0], 13);
    }

    #[test]
    pub(crate) fn compute_table_widths_uses_display_width_for_ambiguous_and_wide_cells() {
        let lines = vec![
            "| Tool | What it does |",
            "|------|---------------|",
            "| bash | Execute shell commands, run tests, build, grep, etc. |",
            "| Ω read | Read files (text + images) |",
        ];
        let widths_per_line = compute_table_widths(&lines, 80);
        let widths = widths_per_line[0].as_ref().expect("table widths");

        assert!(
            widths[0] >= widgets::visible_width("Ω read"),
            "first column should use display width, got widths={widths:?}"
        );
        assert!(
            widths[1]
                >= widgets::visible_width("Execute shell commands, run tests, build, grep, etc."),
            "second column should use display width, got widths={widths:?}"
        );
    }

    #[test]
    pub(crate) fn render_table_line_pads_to_display_width_not_char_count() {
        let widths = vec![8, 12];
        let body = render_table_line("| Ω read | text + images |", false, &widths, &Alpharius);
        let text: String = body
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.starts_with("│ "));
        assert!(text.ends_with("│"));
        assert!(text.contains("Ω read"), "{text}");
        assert!(
            text.contains("text + imag…") || text.contains("text + images"),
            "{text}"
        );
    }

    #[test]
    fn expanded_tool_card_shows_more() {
        let long_result = (0..30)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let seg_collapsed = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("file.rs".into()),
                result_summary: None,
                detail_result: Some(long_result.clone()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let seg_expanded = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("file.rs".into()),
                result_summary: None,
                detail_result: Some(long_result),
                is_error: false,
                complete: true,
                expanded: true,
                live_partial: None,
                started_at: None,
            },
        };

        let h_collapsed = seg_collapsed.height(80, &Alpharius);
        let h_expanded = seg_expanded.height(80, &Alpharius);
        assert!(
            h_expanded > h_collapsed,
            "expanded ({h_expanded}) should be taller than collapsed ({h_collapsed})"
        );
    }

    #[test]
    fn slim_collapsed_tool_card_marks_pinned_state() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "t1".into(),
                name: "bash".into(),
                args_summary: Some("echo hi".into()),
                detail_args: Some("echo hi".into()),
                result_summary: None,
                detail_result: Some("hi".into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 3);
        seg.render_with_pinned(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Detailed,
            true,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("pinned"), "pinned marker missing: {text}");
    }

    #[test]
    fn slim_expanded_tool_card_shows_detail_rows() {
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "t1".into(),
                name: "bash".into(),
                args_summary: Some("printf smoke".into()),
                detail_args: Some("printf smoke".into()),
                result_summary: None,
                detail_result: Some("smoke-detail-line".into()),
                is_error: false,
                complete: true,
                expanded: true,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 12);
        seg.render_with_pinned(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Slim,
            crate::settings::ToolDetail::Lean,
            true,
        );
        let text = buf_text(&buf, area);
        assert!(text.contains("pinned"), "pinned marker missing: {text}");
        assert!(
            text.contains("smoke-detail-line"),
            "expanded slim card should render result detail: {text}"
        );
    }

    #[test]
    fn ansi_colored_tool_output_preserves_colors() {
        // Simulate cargo output with ANSI red error
        let ansi_result = "\x1b[31merror\x1b[0m: expected `;`\n  --> src/main.rs:5:10";
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "t1".into(),
                name: "bash".into(),
                args_summary: Some("cargo check".into()),
                detail_args: Some("cargo check".into()),
                result_summary: None,
                detail_result: Some(ansi_result.into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 12);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        // The ANSI escape should be parsed, not rendered as raw escape
        assert!(
            !text.contains("\x1b"),
            "ANSI escapes should be parsed, not raw: {text}"
        );
        assert!(text.contains("error"), "should contain error text: {text}");
        assert!(
            text.contains("main.rs"),
            "should contain file reference: {text}"
        );
    }

    #[test]
    fn non_ansi_tool_output_renders_plain() {
        let plain_result = "hello world\nline 2";
        let seg = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "t1".into(),
                name: "bash".into(),
                args_summary: Some("echo hi".into()),
                detail_args: Some("echo hi".into()),
                result_summary: None,
                detail_result: Some(plain_result.into()),
                is_error: false,
                complete: true,
                expanded: false,
                live_partial: None,
                started_at: None,
            },
        };
        let (area, mut buf) = make_buf(80, 10);
        seg.render(
            area,
            &mut buf,
            &Alpharius,
            SegmentRenderMode::Full,
            crate::settings::ToolDetail::Detailed,
        );
        let text = buf_text(&buf, area);
        assert!(
            text.contains("hello world"),
            "should render plain text: {text}"
        );
    }

    #[test]
    fn meta_tag_formats_model_and_provider() {
        let meta = SegmentMeta {
            model_id: Some("anthropic:claude-sonnet-4-6".into()),
            provider: Some("anthropic".into()),
            tier: Some("victory".into()),
            thinking_level: Some("medium".into()),
            ..Default::default()
        };
        let tag = build_meta_tag(&meta);
        assert!(
            tag.contains("claude-sonnet-4-6"),
            "should strip provider prefix: {tag}"
        );
        assert!(tag.contains("anthropic"), "should include provider: {tag}");
        assert!(tag.contains("victory"), "should include tier: {tag}");
        assert!(
            tag.contains("think:medium"),
            "should include thinking level: {tag}"
        );
    }

    #[test]
    fn meta_tag_empty_when_no_fields() {
        let meta = SegmentMeta::default();
        assert!(build_meta_tag(&meta).is_empty());
    }

    #[test]
    fn meta_tag_includes_voice_prompt_metadata() {
        let meta = SegmentMeta {
            source_channel: Some("voice".to_string()),
            radio_cue: Some("over_and_out".to_string()),
            voice_close_session_requested: Some(true),
            voice_duration_s: Some(2.1),
            ..SegmentMeta::default()
        };
        let tag = build_meta_tag(&meta);
        assert!(tag.contains("source:voice"), "{tag}");
        assert!(tag.contains("cue:over_and_out"), "{tag}");
        assert!(tag.contains("close-session"), "{tag}");
        assert!(tag.contains("voice:2.1s"), "{tag}");
    }

    fn meta_tag_omits_thinking_off() {
        let meta = SegmentMeta {
            model_id: Some("gpt-4o".into()),
            thinking_level: Some("off".into()),
            ..Default::default()
        };
        let tag = build_meta_tag(&meta);
        assert!(!tag.contains("think"), "should omit think:off: {tag}");
    }

    #[test]
    fn strip_inline_markdown_removes_bold() {
        assert_eq!(strip_inline_markdown("**bold**"), "bold");
        assert_eq!(strip_inline_markdown("a **b** c"), "a b c");
    }

    #[test]
    fn strip_inline_markdown_removes_italic() {
        assert_eq!(strip_inline_markdown("*italic*"), "italic");
        assert_eq!(strip_inline_markdown("a *b* c"), "a b c");
    }

    #[test]
    fn strip_inline_markdown_removes_code() {
        assert_eq!(strip_inline_markdown("`code`"), "code");
        assert_eq!(strip_inline_markdown("a `b` c"), "a b c");
    }

    #[test]
    fn strip_inline_markdown_mixed() {
        assert_eq!(
            strip_inline_markdown("**bold** and `code` and *italic*"),
            "bold and code and italic"
        );
    }

    #[test]
    fn strip_inline_markdown_plain_text_unchanged() {
        assert_eq!(strip_inline_markdown("hello world"), "hello world");
    }

    #[test]
    fn markdown_display_width_accounts_for_stripping() {
        // "**bold**" is 8 chars raw but "bold" is 4 display chars
        assert_eq!(markdown_display_width("**bold**"), 4);
        assert_eq!(markdown_display_width("`code`"), 4);
        assert_eq!(markdown_display_width("plain"), 5);
    }

    #[test]
    pub(crate) fn compute_table_widths_uses_markdown_display_width() {
        // Table where header has plain text but body has markdown
        let lines = vec![
            "| Name | Description |",
            "| --- | --- |",
            "| foo | **bold text** |",
        ];
        let widths = compute_table_widths(&lines, 80);
        // "**bold text**" strips to "bold text" (9 chars), which is wider
        // than "Description" (11 chars). The column should be sized to 11.
        let w = widths[0].as_ref().unwrap();
        assert_eq!(w[0], 4, "Name column: max(Name=4, foo=3) = 4");
        assert_eq!(
            w[1], 11,
            "Description column: max(Description=11, bold text=9) = 11"
        );
    }
}
