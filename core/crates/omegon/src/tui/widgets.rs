//! Shared TUI widget primitives — DRY building blocks for all views.
//!
//! Every visual pattern that appears in 2+ places (footer, dashboard,
//! conversation, raised mode) lives here. All functions produce ratatui
//! `Line`/`Span` values themed through the `Theme` trait.
//!
//! # Layout primitives
//! - `section_divider` — `── label ─────────`
//! - `left_right` — flush-left + flush-right on one line
//! - `merge_columns` — side-by-side column rendering
//! - `pad_right` — pad a line to exact visible width
//! - `truncate_line` — truncate with ellipsis at column boundary
//! - `gauge_bar` — `▐▓▓██░░░▌ 43% / 200k`
//!
//! # Semantic primitives
//! - `badge` — icon + colored text (`✓ edit`, `◌ proposed`)
//! - `status_badge` — lifecycle status with canonical icon/color
//! - `boxed_region` — `╭─╮│╰─╯` chrome for raised panels
//! - `tool_card` — 1-2 line tool call rendering with args + result
//! - `markdown_spans` — structural highlighting (headers, bold, code fences)

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::theme::Theme;

// ─── Box-drawing characters ─────────────────────────────────────────

/// Box-drawing character set. Unicode rounded-box by default,
/// ASCII fallback when `TERM=dumb` or `PI_ASCII=1`.
pub struct BoxChars {
    pub tl: &'static str,
    pub tr: &'static str,
    pub bl: &'static str,
    pub br: &'static str,
    pub h: &'static str,
    pub v: &'static str,
    pub vr: &'static str, // ├
    pub vl: &'static str, // ┤
}

pub const BOX_UNICODE: BoxChars = BoxChars {
    tl: "╭", tr: "╮", bl: "╰", br: "╯",
    h: "─", v: "│", vr: "├", vl: "┤",
};

pub const BOX_ASCII: BoxChars = BoxChars {
    tl: "+", tr: "+", bl: "+", br: "+",
    h: "-", v: "|", vr: "+", vl: "+",
};

/// Select box chars based on environment.
pub fn box_chars() -> &'static BoxChars {
    use std::sync::OnceLock;
    static CHARS: OnceLock<bool> = OnceLock::new();
    let use_ascii = *CHARS.get_or_init(|| {
        if std::env::var("PI_ASCII").as_deref() == Ok("1") { return true; }
        if std::env::var("TERM").as_deref() == Ok("dumb") { return true; }
        let locale = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_CTYPE"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .to_uppercase();
        if !locale.is_empty() && !locale.contains("UTF") { return true; }
        false
    });
    if use_ascii { &BOX_ASCII } else { &BOX_UNICODE }
}

// ─── Text measurement & manipulation ────────────────────────────────

/// Visible width of a string (Unicode-aware, accounts for wide chars).
pub fn visible_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Visible width of a Line (sum of span widths).
pub fn line_width(line: &Line<'_>) -> usize {
    line.spans.iter().map(|s| visible_width(&s.content)).sum()
}

/// Truncate a string to fit within `max_width` visible columns.
/// Appends `suffix` (typically "…") if truncated.
pub fn truncate_str(s: &str, max_width: usize, suffix: &str) -> String {
    let w = visible_width(s);
    if w <= max_width {
        return s.to_string();
    }
    let suffix_w = visible_width(suffix);
    let target = max_width.saturating_sub(suffix_w);
    let mut width = 0;
    let mut byte_end = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > target {
            break;
        }
        width += cw;
        byte_end += ch.len_utf8();
    }
    format!("{}{}", &s[..byte_end], suffix)
}

/// Pad a string with spaces to exactly `width` visible columns.
/// If already at or wider than `width`, returns unchanged.
pub fn pad_right(s: &str, width: usize) -> String {
    let w = visible_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

// ─── Layout primitives ──────────────────────────────────────────────

/// Section divider: `── label ─────────────────────`
///
/// Used in footer cards, dashboard sections, raised mode panels.
pub fn section_divider<'a>(label: &str, width: usize, t: &dyn Theme) -> Line<'a> {
    let prefix = "── ";
    let label_str = format!("{label} ");
    let suffix_len = width.saturating_sub(prefix.len() + label_str.len());
    Line::from(vec![
        Span::styled(prefix.to_string(), Style::default().fg(t.border())),
        Span::styled(label_str, Style::default().fg(t.accent_muted())),
        Span::styled("─".repeat(suffix_len), Style::default().fg(t.border_dim())),
    ])
}

/// Render left-aligned and right-aligned text on one line within `width`.
/// If both don't fit, truncates `left` to make room for `right`.
pub fn left_right<'a>(left: Vec<Span<'a>>, right: Vec<Span<'a>>, width: usize, t: &dyn Theme) -> Line<'a> {
    let left_w: usize = left.iter().map(|s| visible_width(&s.content)).sum();
    let right_w: usize = right.iter().map(|s| visible_width(&s.content)).sum();
    let gap = width.saturating_sub(left_w + right_w);

    let mut spans = left;
    if gap > 0 {
        spans.push(Span::styled(" ".repeat(gap), Style::default().fg(t.bg())));
    }
    spans.extend(right);
    Line::from(spans)
}

/// Merge two column arrays side-by-side with a divider.
/// Row count = max(left.len(), right.len()).
/// Each column is padded/truncated to its width.
pub fn merge_columns<'a>(
    left: &[Vec<Span<'a>>],
    right: &[Vec<Span<'a>>],
    left_width: usize,
    right_width: usize,
    t: &dyn Theme,
) -> Vec<Line<'a>> {
    let b = box_chars();
    let rows = left.len().max(right.len());
    let mut result = Vec::with_capacity(rows);
    let divider_style = Style::default().fg(t.dim());

    for i in 0..rows {
        let mut spans: Vec<Span<'a>> = Vec::new();

        // Left column
        if i < left.len() {
            let mut w = 0;
            for span in &left[i] {
                let sw = visible_width(&span.content);
                if w + sw <= left_width {
                    spans.push(span.clone());
                    w += sw;
                } else {
                    let remaining = left_width.saturating_sub(w);
                    if remaining > 0 {
                        spans.push(Span::styled(
                            truncate_str(&span.content, remaining, "…"),
                            span.style,
                        ));
                    }
                    w = left_width;
                    break;
                }
            }
            if w < left_width {
                spans.push(Span::raw(" ".repeat(left_width - w)));
            }
        } else {
            spans.push(Span::raw(" ".repeat(left_width)));
        }

        // Divider
        spans.push(Span::styled(b.v.to_string(), divider_style));

        // Right column
        if i < right.len() {
            let mut w = 0;
            for span in &right[i] {
                let sw = visible_width(&span.content);
                if w + sw <= right_width {
                    spans.push(span.clone());
                    w += sw;
                } else {
                    let remaining = right_width.saturating_sub(w);
                    if remaining > 0 {
                        spans.push(Span::styled(
                            truncate_str(&span.content, remaining, "…"),
                            span.style,
                        ));
                    }
                    break;
                }
            }
        }

        result.push(Line::from(spans));
    }

    result
}

// ─── Gauge bar ──────────────────────────────────────────────────────

/// Context gauge configuration.
pub struct GaugeConfig {
    pub percent: f32,
    pub bar_width: usize,
    /// Memory portion of the filled bar (accent color, ▓).
    pub memory_blocks: usize,
}

/// Render a gauge bar: `▐▓▓██░░░░░░▌ 43% / 200k  T·8`
pub fn gauge_bar<'a>(cfg: &GaugeConfig, t: &dyn Theme) -> Vec<Span<'a>> {
    let pct = cfg.percent.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * cfg.bar_width as f32) as usize;
    let empty = cfg.bar_width.saturating_sub(filled);
    let memory_blocks = cfg.memory_blocks.min(filled);
    let other_blocks = filled.saturating_sub(memory_blocks);

    let bar_color = if pct > 70.0 { t.error() } else if pct > 45.0 { t.warning() } else { t.accent_muted() };

    let mut spans = vec![
        Span::styled("▐", Style::default().fg(t.dim())),
    ];
    if memory_blocks > 0 {
        spans.push(Span::styled("▓".repeat(memory_blocks), Style::default().fg(t.accent())));
    }
    if other_blocks > 0 {
        spans.push(Span::styled("█".repeat(other_blocks), Style::default().fg(bar_color)));
    }
    if empty > 0 {
        spans.push(Span::styled("░".repeat(empty), Style::default().fg(t.border())));
    }
    spans.push(Span::styled("▌", Style::default().fg(t.dim())));

    spans
}

/// Color for a percentage value (green/yellow/red thresholds).
pub fn percent_color(percent: f32, t: &dyn Theme) -> ratatui::style::Color {
    if percent > 70.0 { t.error() } else if percent > 45.0 { t.warning() } else { t.muted() }
}

// ─── Semantic primitives ────────────────────────────────────────────

/// A badge: icon + colored text. `badge("✓", "edit", t.success(), t)`
pub fn badge<'a>(icon: &str, text: &str, color: ratatui::style::Color) -> Vec<Span<'a>> {
    vec![
        Span::styled(format!("{icon} "), Style::default().fg(color)),
        Span::styled(text.to_string(), Style::default().fg(color)),
    ]
}

/// Tool call card — compact single-line with colored left bar.
pub fn tool_card<'a>(
    name: &str,
    is_error: bool,
    complete: bool,
    args_summary: Option<&str>,
    result_summary: Option<&str>,
    t: &dyn Theme,
) -> Line<'a> {
    let (icon, color) = if complete {
        if is_error { ("✗", t.error()) } else { ("✓", t.success()) }
    } else {
        ("⟳", t.warning())
    };

    let mut spans = vec![
        Span::styled("▎", Style::default().fg(color)),
        Span::styled(format!(" {icon} "), Style::default().fg(color).bg(t.card_bg())),
        Span::styled(name.to_string(), Style::default().fg(color).bg(t.card_bg()).add_modifier(Modifier::BOLD)),
    ];

    if let Some(args) = args_summary {
        let display = if args.len() > 50 {
            format!(" {}…", &args[..49.min(args.len())])
        } else {
            format!(" {args}")
        };
        spans.push(Span::styled(display, Style::default().fg(t.dim()).bg(t.card_bg())));
    }

    if let Some(summary) = result_summary {
        let display = if summary.len() > 40 {
            format!("  {}…", &summary[..39.min(summary.len())])
        } else {
            format!("  {summary}")
        };
        spans.push(Span::styled(display, Style::default().fg(t.muted()).bg(t.card_bg())));
    }

    Line::from(spans)
}

/// Detailed tool card — colored left bar with contrasting background.
///
/// Visual style matches the TS Omegon dashboard:
/// - Thick colored left bar (▎) — green for success, red for error, yellow for in-progress
/// - card_bg background on all lines — visually distinct from conversation text
/// - Prominent tool name header
/// - Output in muted text
pub fn tool_card_detailed<'a>(
    name: &str,
    is_error: bool,
    complete: bool,
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    t: &dyn Theme,
) -> Vec<Line<'a>> {
    let (icon, bar_color) = if complete {
        if is_error { ("✗", t.error()) } else { ("✓", t.success()) }
    } else {
        ("⟳", t.warning())
    };

    let border = Style::default().fg(t.border_dim()).bg(t.card_bg());
    let header_style = Style::default().fg(bar_color).bg(t.card_bg());
    let card_dim = Style::default().fg(t.dim()).bg(t.card_bg());
    let card_fg = Style::default().fg(t.fg()).bg(t.card_bg());
    let surface = Style::default().fg(t.muted()).bg(t.surface_bg());
    let surface_err = Style::default().fg(t.error()).bg(t.surface_bg());

    let mut lines = Vec::new();

    // ── Top border with tool name ───────────────────────────────────
    // ╭─ ▸ bash ──────────────────────────────────────────────────────╮
    lines.push(Line::from(vec![
        Span::styled("╭─ ", border),
        Span::styled(format!("{icon} "), header_style),
        Span::styled(
            format!("{name} "),
            Style::default()
                .fg(bar_color)
                .bg(t.card_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(60), border),
    ]));

    // ── Args section (command / path) ───────────────────────────────
    if let Some(args) = detail_args {
        match name {
            "bash" => {
                // Show command with $ prefix
                for (i, line) in args.lines().take(4).enumerate() {
                    let prefix = if i == 0 { "│ $ " } else { "│   " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, border),
                        Span::styled(line.to_string(), card_fg),
                    ]));
                }
            }
            "edit" => {
                // Show file path with edit-specific formatting
                lines.push(Line::from(vec![
                    Span::styled("│ ", border),
                    Span::styled("▸ edit ", Style::default().fg(t.accent_muted()).bg(t.card_bg())),
                    Span::styled(args.to_string(), card_dim),
                ]));
            }
            _ => {
                // Generic: show path or args
                lines.push(Line::from(vec![
                    Span::styled("│ ", border),
                    Span::styled(args.to_string(), card_dim),
                ]));
            }
        }
    }

    // ── Result section (output on surface background) ───────────────
    if let Some(result) = detail_result {
        // Separator between args and result
        if detail_args.is_some() {
            lines.push(Line::from(vec![
                Span::styled("├─", border),
                Span::styled("─".repeat(60), Style::default().fg(t.border_dim()).bg(t.surface_bg())),
            ]));
        }

        let result_lines: Vec<&str> = result.lines().collect();
        let total = result_lines.len();
        let show = total.min(12);
        let style = if is_error { surface_err } else { surface };

        for line in &result_lines[..show] {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(t.border_dim()).bg(t.surface_bg())),
                Span::styled(line.to_string(), style),
            ]));
        }

        if total > show {
            lines.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(t.border_dim()).bg(t.surface_bg())),
                Span::styled(
                    format!("  … {total} lines total"),
                    Style::default().fg(t.dim()).bg(t.surface_bg()),
                ),
            ]));
        }
    }

    // ── Bottom border ───────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        format!("╰─{}─╯", "─".repeat(58)),
        border,
    )));

    lines
}

/// Lifecycle event card — phase change, decomposition, etc.
///
/// `◈ Phase → implement`
/// `⚡ Cleave: 3 children dispatched`
pub fn lifecycle_event<'a>(icon: &str, text: &str, t: &dyn Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled("│ ", Style::default().fg(t.border_dim())),
        Span::styled(format!("{icon} "), Style::default().fg(t.accent_muted())),
        Span::styled(text.to_string(), Style::default().fg(t.muted())),
    ])
}

/// Error block — red-accented message with icon.
pub fn error_block<'a>(text: &str, t: &dyn Theme) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let gutter = if i == 0 { "✗ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(gutter.to_string(), Style::default().fg(t.error())),
            Span::styled(line.to_string(), Style::default().fg(t.error())),
        ]));
    }
    lines
}

// ─── Boxed region ───────────────────────────────────────────────────

/// Render content inside a rounded box: `╭─ title ──╮ │ content │ ╰──────────╯`
///
/// Returns a Vec<Line> ready to render. `inner_width` is the usable width
/// inside the box (total width - 4 for borders + padding).
pub fn boxed_region<'a>(
    title: &str,
    content: Vec<Line<'a>>,
    footer_lines: Vec<Line<'a>>,
    total_width: usize,
    t: &dyn Theme,
) -> Vec<Line<'a>> {
    let b = box_chars();
    let inner_width = total_width.saturating_sub(4); // │ + space + content + space + │
    let border_style = Style::default().fg(t.border());
    let dim_style = Style::default().fg(t.dim());

    // Top border: ╭─ title ─────────────╮
    let title_str = if title.is_empty() {
        String::new()
    } else {
        format!(" {} ", title)
    };
    let top_fill = inner_width.saturating_sub(visible_width(&title_str) + 1); // +1 for the initial ─
    let mut lines = vec![Line::from(vec![
        Span::styled(b.tl.to_string(), border_style),
        Span::styled(b.h.to_string(), border_style),
        Span::styled(title_str, Style::default().fg(t.accent()).add_modifier(Modifier::BOLD)),
        Span::styled(b.h.repeat(top_fill), border_style),
        Span::styled(b.tr.to_string(), border_style),
    ])];

    // Content lines: │ content │
    let wrap_line = |line: Line<'a>| -> Line<'a> {
        let mut spans = vec![
            Span::styled(format!("{} ", b.v), border_style),
        ];
        spans.extend(line.spans);
        // Pad to inner_width + closing border
        // (We can't easily measure the line width here without cloning,
        // so we just append the closing border)
        spans.push(Span::styled(format!(" {}", b.v), border_style));
        Line::from(spans)
    };

    for line in content {
        lines.push(wrap_line(line));
    }

    // Separator + footer (if any)
    if !footer_lines.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(b.vr.to_string(), border_style),
            Span::styled(b.h.repeat(total_width.saturating_sub(2)), dim_style),
            Span::styled(b.vl.to_string(), border_style),
        ]));
        for line in footer_lines {
            lines.push(wrap_line(line));
        }
    }

    // Bottom border: ╰────────────────────╯
    lines.push(Line::from(vec![
        Span::styled(b.bl.to_string(), border_style),
        Span::styled(b.h.repeat(total_width.saturating_sub(2)), border_style),
        Span::styled(b.br.to_string(), border_style),
    ]));

    lines
}

// ─── Markdown structural highlighting ───────────────────────────────

/// Parse a line of assistant text and apply structural highlighting.
///
/// Recognizes:
/// - `# Header` → accent_bright + bold
/// - `**bold**` → bold
/// - `` `inline code` `` → surface_bg + accent_muted
/// - `- list item` → accent bullet + text
///
/// Does NOT handle multi-line constructs (code fences, block quotes).
/// Those are handled by the conversation view's state machine.
pub fn highlight_line<'a>(line: &str, t: &dyn Theme) -> Line<'a> {
    // Headers: # through ####
    if let Some(rest) = line.strip_prefix("# ") {
        return Line::from(Span::styled(rest.to_string(), t.style_heading()));
    }
    if let Some(rest) = line.strip_prefix("## ") {
        return Line::from(Span::styled(
            rest.to_string(),
            Style::default().fg(t.accent_bright()),
        ));
    }
    if let Some(rest) = line.strip_prefix("### ") {
        return Line::from(Span::styled(
            rest.to_string(),
            Style::default().fg(t.accent()).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(rest) = line.strip_prefix("#### ") {
        return Line::from(Span::styled(
            rest.to_string(),
            Style::default().fg(t.accent()),
        ));
    }

    // List items: - or *
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        let mut spans = vec![
            Span::styled("• ", Style::default().fg(t.accent())),
        ];
        spans.extend(highlight_inline(rest, t));
        return Line::from(spans);
    }

    // Numbered lists: 1. 2. etc.
    if line.len() > 2
        && line.as_bytes()[0].is_ascii_digit()
        && let Some(rest) = line.strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|s| s.strip_prefix(". "))
    {
        let num_part = &line[..line.len() - rest.len() - 2];
        let mut spans = vec![
            Span::styled(format!("{num_part}. "), Style::default().fg(t.accent())),
        ];
        spans.extend(highlight_inline(rest, t));
        return Line::from(spans);
    }

    // Regular line — apply inline highlighting
    Line::from(highlight_inline(line, t))
}

/// Apply inline highlighting: **bold**, `code`, *italic*.
pub fn highlight_inline<'a>(text: &str, t: &dyn Theme) -> Vec<Span<'a>> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut buf = String::new();

    let flush = |buf: &mut String, spans: &mut Vec<Span<'a>>, style: Style| {
        if !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(buf), style));
        }
    };

    let default_style = Style::default().fg(t.fg());
    let bold_style = Style::default().fg(t.fg()).add_modifier(Modifier::BOLD);
    let code_style = Style::default().fg(t.accent_muted()).bg(t.surface_bg());
    let italic_style = Style::default().fg(t.fg()).add_modifier(Modifier::ITALIC);

    while let Some((i, ch)) = chars.next() {
        match ch {
            '`' => {
                flush(&mut buf, &mut spans, default_style);
                // Collect until closing backtick
                let mut code = String::new();
                let mut closed = false;
                for (_j, c) in chars.by_ref() {
                    if c == '`' { closed = true; break; }
                    code.push(c);
                }
                if closed && !code.is_empty() {
                    spans.push(Span::styled(code, code_style));
                } else {
                    buf.push('`');
                    buf.push_str(&code);
                }
            }
            '*' => {
                // Check for ** (bold) or * (italic)
                let next_star = text.get(i + 1..i + 2) == Some("*");
                if next_star {
                    // **bold**
                    flush(&mut buf, &mut spans, default_style);
                    chars.next(); // consume second *
                    let mut bold_text = String::new();
                    let mut closed = false;
                    while let Some((_j, c)) = chars.next() {
                        if c == '*'
                            && let Some(&(_, '*')) = chars.peek()
                        {
                            chars.next();
                            closed = true;
                            break;
                        }
                        bold_text.push(c);
                    }
                    if closed && !bold_text.is_empty() {
                        spans.push(Span::styled(bold_text, bold_style));
                    } else {
                        buf.push_str("**");
                        buf.push_str(&bold_text);
                    }
                } else {
                    // *italic*
                    flush(&mut buf, &mut spans, default_style);
                    let mut ital_text = String::new();
                    let mut closed = false;
                    for (_j, c) in chars.by_ref() {
                        if c == '*' { closed = true; break; }
                        ital_text.push(c);
                    }
                    if closed && !ital_text.is_empty() {
                        spans.push(Span::styled(ital_text, italic_style));
                    } else {
                        buf.push('*');
                        buf.push_str(&ital_text);
                    }
                }
            }
            _ => {
                buf.push(ch);
            }
        }
    }

    flush(&mut buf, &mut spans, default_style);
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), default_style));
    }
    spans
}

// ─── Formatting helpers ─────────────────────────────────────────────

/// Format a token count compactly: 1.2k, 45k, 1.3M.
pub fn format_tokens(count: usize) -> String {
    if count < 1000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1000.0)
    } else if count < 1_000_000 {
        format!("{}k", count / 1000)
    } else {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

    #[test]
    fn visible_width_ascii() {
        assert_eq!(visible_width("hello"), 5);
        assert_eq!(visible_width(""), 0);
    }

    #[test]
    fn visible_width_unicode() {
        // CJK wide chars are 2 columns each
        assert_eq!(visible_width("你好"), 4);
        // Emoji width varies — but the function shouldn't panic
        let _ = visible_width("🎉");
    }

    #[test]
    fn truncate_str_no_op() {
        assert_eq!(truncate_str("hello", 10, "…"), "hello");
        assert_eq!(truncate_str("hello", 5, "…"), "hello");
    }

    #[test]
    fn truncate_str_cuts() {
        assert_eq!(truncate_str("hello world", 8, "…"), "hello w…");
        assert_eq!(truncate_str("hello world", 5, "…"), "hell…");
    }

    #[test]
    fn truncate_str_unicode() {
        // Should not panic on multi-byte chars
        let s = "héllo wörld";
        let result = truncate_str(s, 6, "…");
        assert!(visible_width(&result) <= 6);
    }

    #[test]
    fn pad_right_pads() {
        assert_eq!(pad_right("hi", 5), "hi   ");
        assert_eq!(pad_right("hello", 3), "hello"); // wider than target
    }

    #[test]
    fn section_divider_renders() {
        let t = Alpharius;
        let line = section_divider("context", 40, &t);
        assert!(!line.spans.is_empty());
        // Label should appear
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("context"));
    }

    #[test]
    fn format_tokens_ranges() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(45000), "45k");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn gauge_bar_renders() {
        let t = Alpharius;
        let cfg = GaugeConfig {
            percent: 50.0,
            bar_width: 20,
            memory_blocks: 3,
        };
        let spans = gauge_bar(&cfg, &t);
        // Should have: ▐ + memory + other + empty + ▌
        assert!(spans.len() >= 3);
    }

    #[test]
    fn tool_card_complete() {
        let t = Alpharius;
        let line = tool_card("read", false, true, Some("src/main.rs"), Some("245 lines"), &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("✓"));
        assert!(text.contains("read"));
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("245 lines"));
    }

    #[test]
    fn tool_card_in_progress() {
        let t = Alpharius;
        let line = tool_card("bash", false, false, Some("cargo test"), None, &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("⟳"));
        assert!(text.contains("bash"));
    }

    #[test]
    fn tool_card_error() {
        let t = Alpharius;
        let line = tool_card("edit", true, true, Some("lib.rs"), Some("oldText not found"), &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("✗"));
    }

    #[test]
    fn highlight_header() {
        let t = Alpharius;
        let line = highlight_line("# Hello World", &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn highlight_bold() {
        let t = Alpharius;
        let line = highlight_line("this is **bold** text", &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("bold"));
        // The bold span should have BOLD modifier
        let bold_span = line.spans.iter().find(|s| s.content.as_ref() == "bold");
        assert!(bold_span.is_some());
        assert!(bold_span.unwrap().style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn highlight_inline_code() {
        let t = Alpharius;
        let line = highlight_line("use `cargo test` here", &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("cargo test"));
        // Code span should have surface_bg
        let code_span = line.spans.iter().find(|s| s.content.as_ref() == "cargo test");
        assert!(code_span.is_some());
        assert_eq!(code_span.unwrap().style.bg, Some(t.surface_bg()));
    }

    #[test]
    fn highlight_list_item() {
        let t = Alpharius;
        let line = highlight_line("- first item", &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("•"));
        assert!(text.contains("first item"));
    }

    #[test]
    fn highlight_plain_text() {
        let t = Alpharius;
        let line = highlight_line("just regular text", &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, "just regular text");
    }

    #[test]
    fn boxed_region_renders() {
        let t = Alpharius;
        let content = vec![
            Line::from(Span::raw("line 1")),
            Line::from(Span::raw("line 2")),
        ];
        let result = boxed_region("Title", content, vec![], 40, &t);
        // Top border + 2 content + bottom border = 4 lines
        assert_eq!(result.len(), 4);
        let top: String = result[0].spans.iter().map(|s| s.content.to_string()).collect();
        assert!(top.contains("╭") || top.contains("+"));
        assert!(top.contains("Title"));
    }

    #[test]
    fn boxed_region_with_footer() {
        let t = Alpharius;
        let content = vec![Line::from(Span::raw("body"))];
        let footer = vec![Line::from(Span::raw("footer"))];
        let result = boxed_region("T", content, footer, 30, &t);
        // Top + 1 content + separator + 1 footer + bottom = 5
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn merge_columns_basic() {
        let t = Alpharius;
        let left = vec![
            vec![Span::raw("a1")],
            vec![Span::raw("a2")],
        ];
        let right = vec![
            vec![Span::raw("b1")],
        ];
        let result = merge_columns(&left, &right, 10, 10, &t);
        assert_eq!(result.len(), 2); // max of left/right lengths
    }

    #[test]
    fn box_chars_returns_valid() {
        let b = box_chars();
        assert!(!b.tl.is_empty());
        assert!(!b.h.is_empty());
    }

    #[test]
    fn error_block_renders() {
        let t = Alpharius;
        let lines = error_block("something\nwent wrong", &t);
        assert_eq!(lines.len(), 2);
        let first: String = lines[0].spans.iter().map(|s| s.content.to_string()).collect();
        assert!(first.contains("✗"));
    }

    #[test]
    fn lifecycle_event_renders() {
        let t = Alpharius;
        let line = lifecycle_event("◈", "Phase → implement", &t);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("◈"));
        assert!(text.contains("Phase → implement"));
    }
}
