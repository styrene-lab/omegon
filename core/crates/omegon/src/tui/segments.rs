//! Segment types and per-type rendering for the conversation widget.
//!
//! Each segment renders as an independent widget with its own Block,
//! background, borders, and internal layout. The ConversationWidget
//! composes these into a scrollable view.

use std::sync::OnceLock;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, BorderType, Padding, Paragraph, Wrap};
use tui_syntax_highlight::Highlighter;

use super::theme::Theme;

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
        SyntaxCache { syntax_set: ss, theme }
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Segment enum — the typed conversation model
// ═══════════════════════════════════════════════════════════════════════════

/// A segment in the conversation — each renders as its own widget.
#[derive(Debug, Clone)]
pub enum Segment {
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

// ═══════════════════════════════════════════════════════════════════════════
// Rendering — each segment type knows how to render into a Rect
// ═══════════════════════════════════════════════════════════════════════════

impl Segment {
    /// Render this segment into the given area of the buffer.
    pub fn render(&self, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
        match self {
            Self::UserPrompt { text } => render_user_prompt(text, area, buf, t),
            Self::AssistantText { text, thinking, complete } => {
                render_assistant_text(text, thinking, *complete, area, buf, t);
            }
            Self::ToolCard {
                name, detail_args, detail_result, is_error, complete, expanded, ..
            } => {
                render_tool_card(name, detail_args.as_deref(), detail_result.as_deref(),
                    *is_error, *complete, *expanded, area, buf, t);
            }
            Self::SystemNotification { text } => render_system(text, area, buf, t),
            Self::LifecycleEvent { icon, text } => render_lifecycle(icon, text, area, buf, t),
            Self::Image { path, alt } => render_image_placeholder(path, alt, area, buf, t),
            Self::TurnSeparator => render_separator(area, buf, t),
        }
    }

    /// Calculate the height this segment needs at the given width.
    /// Renders into a temp buffer to get the exact height — matches
    /// Paragraph's word-aware wrapping precisely.
    pub fn height(&self, width: u16, t: &dyn Theme) -> u16 {
        if width == 0 { return 1; }

        // Quick paths for fixed-height types
        match self {
            Self::TurnSeparator => return 1,
            Self::LifecycleEvent { .. } => return 1,
            Self::Image { .. } => return 14, // Fixed: 12 rows image + 1 caption + 1 spacing
            _ => {}
        }

        // Estimate max height for the temp buffer
        let estimate = match self {
            Self::UserPrompt { text } => (text.len() / width.max(1) as usize) as u16 + 4,
            Self::AssistantText { text, thinking, .. } => {
                (text.lines().count() + thinking.lines().count()) as u16 + 6
            }
            Self::ToolCard { detail_args, detail_result, expanded, .. } => {
                let max_r = if *expanded { 200 } else { 12 };
                let a = detail_args.as_ref().map(|a| a.lines().count()).unwrap_or(0);
                let r = detail_result.as_ref().map(|r| r.lines().count().min(max_r)).unwrap_or(0);
                // 3 = border top + title + border bottom. Compact cards.
                (a + r + 3) as u16
            }
            Self::SystemNotification { text } => text.lines().count() as u16 + 3,
            _ => 4,
        };

        // Render into temp buffer — cap at 300 rows to avoid absurd allocations
        let h = estimate.clamp(4, 300);
        let temp_area = Rect::new(0, 0, width, h);
        let mut temp_buf = Buffer::empty(temp_area);
        self.render(temp_area, &mut temp_buf, t);

        // Find the last row with actual text content.
        // Skip border characters (│╰╯┐┘├┤┌└) in the first/last 2 columns
        // and background-only cells. Only count rows with real text INSIDE
        // the card borders.
        let mut last_used: u16 = 0;
        let _border_chars: &[char] = &['│', '─', '╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼'];
        for y in (0..h).rev() {
            let mut has_content = false;
            // Check interior columns only (skip first 2 and last 2 for borders + padding)
            let x_start = 2.min(width);
            let x_end = width.saturating_sub(2).max(x_start);
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

        // Add 1 row spacing after the segment
        (last_used + 1).max(1)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-type renderers
// ═══════════════════════════════════════════════════════════════════════════

fn render_user_prompt(text: &str, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
    if area.width < 3 || area.height == 0 { return; }

    // Subtle tinted background for the user prompt region
    let bg = t.user_msg_bg();
    let block = Block::default()
        .style(Style::default().bg(bg));
    block.render(area, buf);

    // Accent bar on the left edge
    for y in area.top()..area.bottom() {
        if let Some(cell) = buf.cell_mut((area.x, y)) {
            cell.set_symbol("▎");
            cell.set_style(Style::default().fg(t.accent()).bg(bg));
        }
    }

    // Content area — offset by 2 for the accent bar + space
    let inner = Rect {
        x: area.x + 2,
        y: area.y,
        width: area.width.saturating_sub(3),
        height: area.height,
    };

    let content = Line::from(vec![
        Span::styled("▸ ", Style::default().fg(t.accent()).bg(bg)),
        Span::styled(text.to_string(), Style::default().fg(t.fg()).bg(bg).add_modifier(Modifier::BOLD)),
    ]);
    Paragraph::new(content)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
}

fn render_assistant_text(
    text: &str, thinking: &str, complete: bool,
    area: Rect, buf: &mut Buffer, t: &dyn Theme,
) {
    if area.width < 3 || area.height == 0 { return; }

    // Left gutter — accent-colored bar for the full height of the response
    // Flush left at area.x, same position as user message bar
    for row in 0..area.height {
        if let Some(cell) = buf.cell_mut((area.x, area.y + row)) {
            cell.set_symbol("│");
            cell.set_style(Style::default().fg(t.success()));
        }
    }

    let inner = Rect {
        x: area.x + 2,
        y: area.y,
        width: area.width.saturating_sub(3),
        height: area.height,
    };

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Thinking block — collapsed summary with line count
    if !thinking.is_empty() {
        let think_lines: Vec<&str> = thinking.lines().collect();
        let show = think_lines.len().min(6);
        lines.push(Line::from(vec![
            Span::styled("◌ ", Style::default().fg(t.border())),
            Span::styled("thinking ", Style::default().fg(t.dim()).add_modifier(Modifier::ITALIC)),
            Span::styled(
                format!("({} lines)", think_lines.len()),
                Style::default().fg(t.border_dim()),
            ),
        ]));
        for line in think_lines.iter().take(show) {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(t.border()).add_modifier(Modifier::ITALIC),
            )));
        }
        if think_lines.len() > show {
            lines.push(Line::from(Span::styled(
                format!("  ⋯ {} more", think_lines.len() - show),
                Style::default().fg(t.border_dim()),
            )));
        }
        // Visual separator between thinking and response
        lines.push(Line::from(Span::styled(
            "  ─ ─ ─",
            Style::default().fg(t.border_dim()),
        )));
    }

    // Assistant text with markdown structural highlighting
    let mut in_code_fence = false;
    let mut table_state = TableState::None;
    for line in text.lines() {
        if line.starts_with("```") {
            in_code_fence = !in_code_fence;
            table_state = TableState::None;
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(t.dim()).bg(t.surface_bg()),
            )));
        } else if in_code_fence {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(t.accent_muted()).bg(t.surface_bg()),
            )));
        } else if is_table_line(line) {
            // Track table context: header → separator → body
            let is_header = match table_state {
                TableState::None => { table_state = TableState::Header; true }
                TableState::Header if is_table_separator(line) => {
                    table_state = TableState::Body; false
                }
                _ => { table_state = TableState::Body; false }
            };
            lines.push(render_table_line(line, is_header, t));
        } else {
            table_state = TableState::None;
            lines.push(super::widgets::highlight_line(line, t));
        }
    }

    // Streaming cursor
    if !complete && text.is_empty() && thinking.is_empty() {
        lines.push(Line::from(Span::styled("…", t.style_dim())));
    }

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(inner, buf);
}

fn render_tool_card(
    name: &str, detail_args: Option<&str>, detail_result: Option<&str>,
    is_error: bool, complete: bool, expanded: bool,
    area: Rect, buf: &mut Buffer, t: &dyn Theme,
) {
    let (icon, status_color) = if complete {
        if is_error { ("✗", t.error()) } else { ("✓", t.success()) }
    } else {
        ("⟳", t.warning())
    };

    // ── Card block with rounded borders ─────────────────────────
    // For bash, show a short description of what the command does
    let display_name = if name == "bash" {
        if let Some(args) = detail_args {
            let cmd = args.lines().next().unwrap_or(args);
            let first_word = cmd.split_whitespace().next().unwrap_or("bash");
            match first_word {
                "grep" | "rg" => "search",
                "find" => "find",
                "ls" | "dir" => "list",
                "cat" | "head" | "tail" | "bat" => "read",
                "sed" | "awk" => "transform",
                "curl" | "wget" => "fetch",
                "git" => "git",
                "cargo" => "cargo",
                "npm" | "npx" | "pnpm" | "yarn" | "bun" => "npm",
                "docker" | "podman" => "container",
                "kubectl" | "k" => "kubectl",
                "make" | "cmake" => "build",
                "python" | "python3" | "pip" => "python",
                "rustc" | "rustup" => "rust",
                "go" => "go",
                "dig" | "nslookup" | "host" => "dns",
                "ssh" | "scp" | "rsync" => "remote",
                "tar" | "zip" | "unzip" | "gzip" => "archive",
                "wc" => "count",
                "sort" | "uniq" => "sort",
                "diff" | "patch" => "diff",
                "mkdir" | "rm" | "mv" | "cp" | "chmod" | "chown" => "fs",
                "echo" | "printf" => "echo",
                "test" | "[" => "test",
                "vault" => "vault",
                "sh" | "bash" | "zsh" => "shell",
                _ => first_word,
            }
        } else {
            "bash"
        }
    } else {
        name
    };

    let title = Line::from(vec![
        Span::styled(format!(" {icon} "), Style::default().fg(status_color)),
        Span::styled(
            format!("{display_name} "),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
    ]);

    // Border color matches status — makes the card visually distinct
    let border_color = if !complete {
        t.warning()
    } else if is_error {
        t.error()
    } else {
        t.success()
    };

    // Card background varies by status
    let bg = if is_error {
        t.tool_error_bg()
    } else {
        t.tool_success_bg()
    };

    let card_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color).bg(bg))
        .title(title)
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(bg));

    let card_inner = card_block.inner(area);
    card_block.render(area, buf);

    if card_inner.height == 0 || card_inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // ── Args section ────────────────────────────────────────────
    if let Some(args) = detail_args {
        match name {
            "bash" => {
                for (i, line) in args.lines().take(4).enumerate() {
                    let prefix = if i == 0 { "$ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(t.dim()).bg(bg)),
                        Span::styled(line.to_string(), Style::default().fg(t.fg()).bg(bg)),
                    ]));
                }
            }
            "edit" => {
                lines.push(Line::from(vec![
                    Span::styled("▸ edit ", Style::default().fg(t.accent_muted()).bg(bg)),
                    Span::styled(args.to_string(), Style::default().fg(t.dim()).bg(bg)),
                ]));
            }
            "read" | "write" => {
                // File path — will be rendered as clickable link post-render
                lines.push(Line::from(Span::styled(
                    args.to_string(),
                    Style::default().fg(t.accent_muted()).bg(bg).add_modifier(Modifier::UNDERLINED),
                )));
            }
            _ => {
                lines.push(Line::from(Span::styled(
                    args.to_string(),
                    Style::default().fg(t.dim()).bg(bg),
                )));
            }
        }
    }

    // ── Result section with distinct background ─────────────────
    if let Some(result) = detail_result {
        if !lines.is_empty() {
            // Separator line
            lines.push(Line::from(Span::styled(
                "─".repeat(card_inner.width as usize),
                Style::default().fg(t.border_dim()).bg(t.surface_bg()),
            )));
        }

        let result_lines: Vec<&str> = result.lines().collect();
        let max_lines = if expanded { 200 } else { 12 };
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
                // Apply surface_bg to each span
                let spans: Vec<Span<'_>> = line.spans.into_iter().map(|mut s| {
                    s.style = s.style.bg(t.surface_bg());
                    s
                }).collect();
                lines.push(Line::from(spans));
            }
        } else {
            let result_style = if is_error {
                Style::default().fg(t.error()).bg(t.surface_bg())
            } else {
                Style::default().fg(t.muted()).bg(t.surface_bg())
            };

            // Try ANSI color parsing for tool output (cargo, git diff, etc.)
            let joined = result_lines[..show].join("\n");
            let has_ansi = joined.contains('\x1b');

            if has_ansi {
                use ansi_to_tui::IntoText as _;
                if let Ok(text) = joined.into_text() {
                    for line in text.lines {
                        let spans: Vec<Span<'_>> = line.spans.into_iter().map(|mut s| {
                            // Preserve ANSI foreground, apply surface_bg
                            s.style = s.style.bg(t.surface_bg());
                            // If no foreground was set by ANSI, use muted
                            if s.style.fg.is_none() {
                                s.style = s.style.fg(t.muted());
                            }
                            s
                        }).collect();
                        lines.push(Line::from(spans));
                    }
                } else {
                    // ANSI parse failed — fall back to plain
                    for line in &result_lines[..show] {
                        lines.push(Line::from(Span::styled(
                            line.to_string(), result_style,
                        )));
                    }
                }
            } else {
                for line in &result_lines[..show] {
                    lines.push(Line::from(Span::styled(
                        line.to_string(), result_style,
                    )));
                }
            }
        }

        if result_lines.len() > show {
            let hint = if expanded {
                format!("  ── {} lines ── Tab to collapse", result_lines.len())
            } else {
                format!("  ── {} more lines ── Tab to expand", result_lines.len() - show)
            };
            lines.push(Line::from(Span::styled(
                hint,
                Style::default().fg(t.accent_muted()).bg(t.surface_bg()),
            )));
        }
    }

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(card_inner, buf);

    // ── Post-render: OSC 8 hyperlinks for file paths ────────────
    if matches!(name, "read" | "edit" | "write")
        && let Some(args) = detail_args
    {
        let file_path = args.lines().next().unwrap_or(args).trim();
        if !file_path.is_empty() && card_inner.height > 0 {
                let url = format!("file://{file_path}");
                let link_area = Rect {
                    x: card_inner.x,
                    y: card_inner.y, // first line is the file path
                    width: card_inner.width.min(file_path.len() as u16),
                    height: 1,
                };
                let link = hyperrat::Link::new(file_path, url)
                    .style(Style::default().fg(t.accent_muted()).bg(bg).add_modifier(Modifier::UNDERLINED));
                link.render(link_area, buf);
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
            matches!(first_word, "cat" | "head" | "tail" | "sed" | "awk" | "less" | "bat" | "nl")
        }),
        _ => false,
    };
    let highlighter = Highlighter::new(cache.theme.clone())
        .line_numbers(show_line_numbers);
    let text_lines: Vec<&str> = text.lines().collect();
    let highlighted = highlighter.highlight_lines(text_lines, syntax, &cache.syntax_set).ok()?;
    Some(highlighted.lines.into_iter().map(|line| {
        Line::from(line.spans.into_iter().map(|span| {
            Span::styled(span.content.to_string(), span.style)
        }).collect::<Vec<_>>())
    }).collect())
}

/// Table parsing state — tracks whether we're in header, separator, or body rows.
#[derive(Clone, Copy, PartialEq)]
enum TableState { None, Header, Body }

/// Detect markdown table lines: `| cell | cell |` or `|---|---|`
fn is_table_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2
}

/// Detect table separator: `|---|---|` or `| --- | --- |`
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|')
        && trimmed.chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

/// Render a markdown table line with cell highlighting.
fn render_table_line<'a>(line: &str, is_header: bool, t: &dyn Theme) -> Line<'a> {
    let trimmed = line.trim();
    let row_bg = if is_header { t.card_bg() } else { t.surface_bg() };

    // Separator row: |---|---| → render as a thin rule
    if is_table_separator(trimmed) {
        let sep_bg = t.surface_bg();
        let sep_fg = t.border();
        let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
        let mut spans: Vec<Span<'a>> = Vec::new();
        spans.push(Span::styled("├", Style::default().fg(sep_fg).bg(sep_bg)));
        for (i, cell) in cells.iter().enumerate() {
            let w = cell.len().max(1);
            spans.push(Span::styled("─".repeat(w), Style::default().fg(sep_fg).bg(sep_bg)));
            if i < cells.len() - 1 {
                spans.push(Span::styled("┼", Style::default().fg(sep_fg).bg(sep_bg)));
            }
        }
        spans.push(Span::styled("┤", Style::default().fg(sep_fg).bg(sep_bg)));
        return Line::from(spans);
    }

    // Content row: | cell | cell |
    let mut spans: Vec<Span<'a>> = Vec::new();
    let cells: Vec<&str> = trimmed.split('|')
        .filter(|s| !s.is_empty())
        .collect();

    let pipe = Style::default().fg(t.border()).bg(row_bg);

    spans.push(Span::styled("│", pipe));
    for (i, cell) in cells.iter().enumerate() {
        let cell_text = cell.trim();
        if is_header {
            // Header cells: bright accent, bold, slightly different background
            spans.push(Span::styled(
                format!(" {cell_text} "),
                Style::default().fg(t.accent_bright()).bg(row_bg).add_modifier(Modifier::BOLD),
            ));
        } else {
            // Content cells: inline highlighting (bold, code, etc.)
            let cell_spans = super::widgets::highlight_inline(cell_text, t);
            spans.push(Span::styled(" ", Style::default().bg(row_bg)));
            for mut s in cell_spans {
                s.style = s.style.bg(row_bg);
                spans.push(s);
            }
            spans.push(Span::styled(" ", Style::default().bg(row_bg)));
        }
        if i < cells.len() - 1 {
            spans.push(Span::styled("│", pipe));
        }
    }
    spans.push(Span::styled("│", pipe));

    Line::from(spans)
}

fn render_system(text: &str, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
    if area.width < 3 || area.height == 0 { return; }

    let bg = t.card_bg();
    let block = Block::default()
        .style(Style::default().bg(bg));
    block.render(area, buf);

    // Accent bar on left edge — muted cyan for system messages
    for y in area.top()..area.bottom() {
        if let Some(cell) = buf.cell_mut((area.x, y)) {
            cell.set_symbol("▎");
            cell.set_style(Style::default().fg(t.accent_muted()).bg(bg));
        }
    }

    let inner = Rect {
        x: area.x + 2,
        y: area.y,
        width: area.width.saturating_sub(3),
        height: area.height,
    };

    let mut lines: Vec<Line<'_>> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let style = if i == 0 && line.starts_with('Ω') {
            Style::default().fg(t.accent()).bg(bg).add_modifier(Modifier::BOLD)
        } else if i == 0 && (line.starts_with('⚠') || line.starts_with('⟳')) {
            Style::default().fg(t.warning()).bg(bg)
        } else if line.starts_with("  ▸") || line.starts_with("  /") || line.starts_with("  Ctrl") {
            Style::default().fg(t.muted()).bg(bg)
        } else {
            Style::default().fg(t.accent_muted()).bg(bg)
        };
        lines.push(Line::from(Span::styled(line.to_string(), style)));
    }

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
}

fn render_lifecycle(icon: &str, text: &str, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
    if area.width < 4 || area.height == 0 { return; }
    let line = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(format!("{icon} "), Style::default().fg(t.border())),
        Span::styled(text.to_string(), Style::default().fg(t.dim())),
    ]);
    Paragraph::new(line).render(area, buf);
}

/// Render a placeholder for an image (used when StatefulProtocol isn't available).
/// The actual image rendering happens in conv_widget.rs via ratatui-image.
fn render_image_placeholder(
    path: &std::path::Path, alt: &str, area: Rect, buf: &mut Buffer, t: &dyn Theme,
) {
    if area.height == 0 { return; }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_dim()))
        .title(Span::styled(" 🖼 image ", Style::default().fg(t.accent_muted())))
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(t.surface_bg()));

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.height == 0 { return; }

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image");
    let caption = if alt.is_empty() { filename.to_string() } else { alt.to_string() };

    let lines = vec![
        Line::from(Span::styled(caption, Style::default().fg(t.muted()))),
        Line::from(Span::styled(
            path.display().to_string(),
            Style::default().fg(t.dim()),
        )),
    ];
    Paragraph::new(lines).render(inner, buf);
}

fn render_separator(area: Rect, buf: &mut Buffer, t: &dyn Theme) {
    if area.height == 0 || area.width < 4 { return; }
    // Thin ruled divider with faded edges
    let pad = 2;
    let rule_w = (area.width as usize).saturating_sub(pad * 2);
    let line = Line::from(vec![
        Span::styled(" ".repeat(pad), Style::default()),
        Span::styled("─".repeat(rule_w), Style::default().fg(t.border_dim())),
        Span::styled(" ".repeat(pad), Style::default()),
    ]);
    Paragraph::new(line).render(area, buf);
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::theme::Alpharius;

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

    #[test]
    fn user_prompt_renders() {
        let seg = Segment::UserPrompt { text: "hello world".into() };
        let (area, mut buf) = make_buf(40, 5);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("▸"), "should have prompt icon");
        assert!(text.contains("hello world"), "should have text");
    }

    #[test]
    fn tool_card_has_borders() {
        let seg = Segment::ToolCard {
            id: "1".into(), name: "bash".into(),
            args_summary: Some("ls -la".into()),
            detail_args: Some("ls -la".into()),
            result_summary: Some("total 42".into()),
            detail_result: Some("total 42\ndrwxr-xr-x  5 user staff".into()),
            is_error: false, complete: true, expanded: false,
        };
        let (area, mut buf) = make_buf(60, 10);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("╭"), "should have top border: {text}");
        assert!(text.contains("╰"), "should have bottom border: {text}");
        assert!(text.contains("list"), "should have display name for ls: {text}");
        assert!(text.contains("✓"), "should have checkmark: {text}");
    }

    #[test]
    fn tool_card_error_styling() {
        let seg = Segment::ToolCard {
            id: "1".into(), name: "write".into(),
            args_summary: None, detail_args: Some("/tmp/test".into()),
            result_summary: None, detail_result: Some("permission denied".into()),
            is_error: true, complete: true, expanded: false,
        };
        let (area, mut buf) = make_buf(60, 8);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("✗"), "should have error icon: {text}");
    }

    #[test]
    fn assistant_text_with_code_fence() {
        let seg = Segment::AssistantText {
            text: "Here's code:\n```rust\nfn main() {}\n```\nDone.".into(),
            thinking: String::new(),
            complete: true,
        };
        let (area, mut buf) = make_buf(60, 10);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("fn main"), "should have code: {text}");
    }

    #[test]
    fn height_calculation() {
        let t = Alpharius;
        let sep = Segment::TurnSeparator;
        assert_eq!(sep.height(80, &t), 1);

        let user = Segment::UserPrompt { text: "short".into() };
        let h = user.height(80, &t);
        assert!(h >= 2 && h <= 5, "user prompt height: {h}");

        let tool = Segment::ToolCard {
            id: "1".into(), name: "bash".into(),
            args_summary: None, detail_args: Some("echo hello".into()),
            result_summary: None, detail_result: Some("hello".into()),
            is_error: false, complete: true, expanded: false,
        };
        let h = tool.height(80, &t);
        assert!(h >= 4, "tool card height should be >= 4, got {h}");
    }

    #[test]
    fn system_notification_renders() {
        let seg = Segment::SystemNotification { text: "Tool display → detailed".into() };
        let (area, mut buf) = make_buf(60, 3);
        seg.render(area, &mut buf, &Alpharius);
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
        let line = render_table_line("| Name | Value |", true, &Alpharius);
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("Name"), "header should contain cell text: {text}");
        assert!(text.contains("│"), "should contain box drawing separator: {text}");

        let body = render_table_line("| foo | bar |", false, &Alpharius);
        let body_text: String = body.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(body_text.contains("foo"), "body should contain cell text: {body_text}");

        let sep = render_table_line("|---|---|", false, &Alpharius);
        let sep_text: String = sep.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(sep_text.contains("─"), "separator should use rule chars: {sep_text}");
        assert!(sep_text.contains("┼"), "separator should have cross: {sep_text}");
    }

    #[test]
    fn expanded_tool_card_shows_more() {
        let long_result = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let seg_collapsed = Segment::ToolCard {
            id: "1".into(), name: "read".into(),
            args_summary: None, detail_args: Some("file.rs".into()),
            result_summary: None, detail_result: Some(long_result.clone()),
            is_error: false, complete: true, expanded: false,
        };
        let seg_expanded = Segment::ToolCard {
            id: "1".into(), name: "read".into(),
            args_summary: None, detail_args: Some("file.rs".into()),
            result_summary: None, detail_result: Some(long_result),
            is_error: false, complete: true, expanded: true,
        };

        let h_collapsed = seg_collapsed.height(80, &Alpharius);
        let h_expanded = seg_expanded.height(80, &Alpharius);
        assert!(h_expanded > h_collapsed,
            "expanded ({h_expanded}) should be taller than collapsed ({h_collapsed})");
    }

    #[test]
    fn ansi_colored_tool_output_preserves_colors() {
        // Simulate cargo output with ANSI red error
        let ansi_result = "\x1b[31merror\x1b[0m: expected `;`\n  --> src/main.rs:5:10";
        let seg = Segment::ToolCard {
            id: "t1".into(),
            name: "bash".into(),
            args_summary: Some("cargo check".into()),
            detail_args: Some("cargo check".into()),
            result_summary: None,
            detail_result: Some(ansi_result.into()),
            is_error: false,
            complete: true,
            expanded: false,
        };
        let (area, mut buf) = make_buf(80, 12);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        // The ANSI escape should be parsed, not rendered as raw escape
        assert!(!text.contains("\x1b"), "ANSI escapes should be parsed, not raw: {text}");
        assert!(text.contains("error"), "should contain error text: {text}");
        assert!(text.contains("main.rs"), "should contain file reference: {text}");
    }

    #[test]
    fn non_ansi_tool_output_renders_plain() {
        let plain_result = "hello world\nline 2";
        let seg = Segment::ToolCard {
            id: "t1".into(),
            name: "bash".into(),
            args_summary: Some("echo hi".into()),
            detail_args: Some("echo hi".into()),
            result_summary: None,
            detail_result: Some(plain_result.into()),
            is_error: false,
            complete: true,
            expanded: false,
        };
        let (area, mut buf) = make_buf(80, 10);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("hello world"), "should render plain text: {text}");
    }
}
