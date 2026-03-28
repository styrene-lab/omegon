//! Segment types and per-type rendering for the conversation widget.
//!
//! Each segment renders as an independent widget with its own Block,
//! background, borders, and internal layout. The ConversationWidget
//! composes these into a scrollable view.

use std::sync::OnceLock;

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};
use tui_syntax_highlight::Highlighter;
use unicode_width::UnicodeWidthStr;

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
        SyntaxCache {
            syntax_set: ss,
            theme,
        }
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Segment — rich metadata wrapper + typed content
// ═══════════════════════════════════════════════════════════════════════════

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
    /// Context window fill percentage at time of generation.
    pub context_percent: Option<f32>,
    /// Active persona ID, if any.
    pub persona: Option<String>,
    /// Git branch at time of generation.
    pub branch: Option<String>,
    /// Duration of the operation (for tool calls: execution time).
    pub duration_ms: Option<u64>,
}

/// A segment in the conversation — metadata wrapper + typed content.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Rich metadata captured at creation time.
    pub meta: SegmentMeta,
    /// The typed content of this segment.
    pub content: SegmentContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentRole {
    Operator,
    Assistant,
    Tool,
    System,
    Lifecycle,
    Media,
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentEmphasis {
    Strong,
    Normal,
    Muted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentPresentation {
    pub role: SegmentRole,
    pub sigil: &'static str,
    pub emphasis: SegmentEmphasis,
    pub tool_visual: Option<ToolVisualKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVisualKind {
    CommandExec,
    FileRead,
    FileMutation,
    DesignTree,
    Memory,
    Search,
    Generic,
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

// ═══════════════════════════════════════════════════════════════════════════
// Rendering — each segment type knows how to render into a Rect
// ═══════════════════════════════════════════════════════════════════════════

impl Segment {
    fn tool_visual_kind(&self) -> Option<ToolVisualKind> {
        match &self.content {
            SegmentContent::ToolCard { name, .. } => Some(match name.as_str() {
                "bash" => ToolVisualKind::CommandExec,
                "read" | "view" => ToolVisualKind::FileRead,
                "write" | "edit" | "change" => ToolVisualKind::FileMutation,
                "design_tree" | "design_tree_update" | "openspec_manage" | "lifecycle_doctor" => {
                    ToolVisualKind::DesignTree
                }
                name if name.starts_with("memory_") => ToolVisualKind::Memory,
                "web_search" => ToolVisualKind::Search,
                _ => ToolVisualKind::Generic,
            }),
            _ => None,
        }
    }

    pub fn role(&self) -> SegmentRole {
        match self.content {
            SegmentContent::UserPrompt { .. } => SegmentRole::Operator,
            SegmentContent::AssistantText { .. } => SegmentRole::Assistant,
            SegmentContent::ToolCard { .. } => SegmentRole::Tool,
            SegmentContent::SystemNotification { .. } => SegmentRole::System,
            SegmentContent::LifecycleEvent { .. } => SegmentRole::Lifecycle,
            SegmentContent::Image { .. } => SegmentRole::Media,
            SegmentContent::TurnSeparator => SegmentRole::Separator,
        }
    }

    pub fn presentation(&self) -> SegmentPresentation {
        match self.role() {
            SegmentRole::Operator => SegmentPresentation {
                role: SegmentRole::Operator,
                sigil: "OP",
                emphasis: SegmentEmphasis::Strong,
                tool_visual: None,
            },
            SegmentRole::Assistant => SegmentPresentation {
                role: SegmentRole::Assistant,
                sigil: "Ω",
                emphasis: SegmentEmphasis::Normal,
                tool_visual: None,
            },
            SegmentRole::Tool => SegmentPresentation {
                role: SegmentRole::Tool,
                sigil: "⚙",
                emphasis: SegmentEmphasis::Normal,
                tool_visual: self.tool_visual_kind(),
            },
            SegmentRole::System => SegmentPresentation {
                role: SegmentRole::System,
                sigil: "ℹ",
                emphasis: SegmentEmphasis::Muted,
                tool_visual: None,
            },
            SegmentRole::Lifecycle => SegmentPresentation {
                role: SegmentRole::Lifecycle,
                sigil: "⚡",
                emphasis: SegmentEmphasis::Muted,
                tool_visual: None,
            },
            SegmentRole::Media => SegmentPresentation {
                role: SegmentRole::Media,
                sigil: "◈",
                emphasis: SegmentEmphasis::Normal,
                tool_visual: None,
            },
            SegmentRole::Separator => SegmentPresentation {
                role: SegmentRole::Separator,
                sigil: "",
                emphasis: SegmentEmphasis::Muted,
                tool_visual: None,
            },
        }
    }

    /// Render this segment into the given area of the buffer.
    pub fn render(&self, area: Rect, buf: &mut Buffer, t: &dyn Theme) {
        use SegmentContent::*;
        let presentation = self.presentation();
        match &self.content {
            UserPrompt { text } => render_user_prompt(text, &presentation, &self.meta, area, buf, t),
            AssistantText {
                text,
                thinking,
                complete,
            } => {
                render_assistant_text(
                    text,
                    thinking,
                    *complete,
                    &self.meta,
                    &presentation,
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
                ..
            } => {
                render_tool_card(
                    name,
                    detail_args.as_deref(),
                    detail_result.as_deref(),
                    *is_error,
                    *complete,
                    *expanded,
                    &self.meta,
                    area,
                    buf,
                    t,
                );
            }
            SystemNotification { text } => render_system(text, area, buf, t),
            LifecycleEvent { icon, text } => render_lifecycle(icon, text, area, buf, t),
            Image { path, alt } => render_image_placeholder(path, alt, area, buf, t),
            TurnSeparator => render_separator(area, buf, t),
        }
    }

    /// Calculate the height this segment needs at the given width.
    /// Renders into a temp buffer to get the exact height — matches
    /// Paragraph's word-aware wrapping precisely.
    pub fn height(&self, width: u16, t: &dyn Theme) -> u16 {
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
            UserPrompt { text } => wrapped_rows(text, width.saturating_sub(4)) + 4,
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
                expanded,
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
                let separator_rows = u16::from(compact_arg_rows > 0 && compact_result_rows > 0);
                compact_arg_rows + compact_result_rows + separator_rows + 4
            }
            SystemNotification { text } => wrapped_rows(text, width.saturating_sub(4)) + 3,
            _ => 4,
        };

        // Render into temp buffer — cap at 400 rows to avoid absurd allocations
        let h = estimate.clamp(4, 400);
        let temp_area = Rect::new(0, 0, width, h);
        let mut temp_buf = Buffer::empty(temp_area);
        self.render(temp_area, &mut temp_buf, t);

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

        let trailing_spacing = match self.role() {
            SegmentRole::Operator | SegmentRole::Tool => 0,
            _ => 1,
        };
        (last_used + trailing_spacing).max(1)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Per-type renderers
// ═══════════════════════════════════════════════════════════════════════════

fn wrapped_rows(text: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    text.lines()
        .map(|line| UnicodeWidthStr::width(line).max(1).div_ceil(width) as u16)
        .sum::<u16>()
        .max(1)
}

fn render_user_prompt(
    text: &str,
    presentation: &SegmentPresentation,
    meta: &SegmentMeta,
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
) {
    if area.width < 3 || area.height == 0 {
        return;
    }

    let bg = t.user_msg_bg();
    let border_color = match presentation.emphasis {
        SegmentEmphasis::Strong => t.accent(),
        SegmentEmphasis::Normal => t.accent_muted(),
        SegmentEmphasis::Muted => t.border_dim(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color).bg(bg))
        .title_top(Line::from(Span::styled(
            format!(" {}", presentation.sigil),
            Style::default()
                .fg(border_color)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        )))
        .title_top(
            top_right_timestamp(meta, t)
                .unwrap_or_else(Line::default)
                .right_aligned(),
        )
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(bg));
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content = vec![Line::from(vec![
        Span::styled(
            format!("{} ", presentation.sigil),
            Style::default()
                .fg(border_color)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            text.to_string(),
            Style::default()
                .fg(t.fg())
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    Paragraph::new(content)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
}

/// Build a compact meta tag string from SegmentMeta for display in the response header.
/// Example: "claude-sonnet-4-6 · anthropic · victory · think:medium"
fn build_meta_tag(meta: &SegmentMeta) -> String {
    let mut parts = Vec::new();
    if let Some(ref m) = meta.model_id {
        // Trim provider prefix if present (e.g. "anthropic:claude-..." → "claude-...")
        let short = m.split(':').last().unwrap_or(m);
        parts.push(short.to_string());
    }
    if let Some(ref p) = meta.provider {
        parts.push(p.clone());
    }
    if let Some(ref tier) = meta.tier {
        parts.push(tier.clone());
    }
    if let Some(ref tl) = meta.thinking_level {
        if tl != "off" {
            parts.push(format!("think:{tl}"));
        }
    }
    if let Some(ref persona) = meta.persona {
        parts.push(format!("⌘ {persona}"));
    }
    parts.join(" · ")
}

fn format_timestamp(timestamp: Option<std::time::SystemTime>) -> Option<String> {
    let timestamp = timestamp?;
    let datetime: chrono::DateTime<chrono::Local> = timestamp.into();
    Some(datetime.format("%H:%M").to_string())
}

fn top_right_timestamp<'a>(meta: &SegmentMeta, t: &dyn Theme) -> Option<Line<'a>> {
    format_timestamp(meta.timestamp).map(|stamp| {
        Line::from(Span::styled(
            stamp,
            Style::default().fg(t.dim()).add_modifier(Modifier::DIM),
        ))
    })
}

fn tool_title_line(
    icon: &str,
    status_color: Color,
    display_name: &str,
    area_width: u16,
    timestamp: Option<&str>,
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
    let icon_prefix = format!(" {icon} ");
    let icon_width = UnicodeWidthStr::width(icon_prefix.as_str());
    let name_budget = left_budget.saturating_sub(icon_width).max(1);
    let title_name = crate::util::truncate(display_name, name_budget);
    let title_text = format!("{icon_prefix}{title_name} ");
    let used_width = UnicodeWidthStr::width(title_text.as_str());
    let pad = left_budget.saturating_sub(used_width);

    Line::from(vec![
        Span::styled(icon_prefix, Style::default().fg(status_color)),
        Span::styled(
            format!("{title_name} "),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ".repeat(pad), Style::default().fg(status_color)),
    ])
}

fn render_assistant_text(
    text: &str,
    thinking: &str,
    complete: bool,
    meta: &SegmentMeta,
    presentation: &SegmentPresentation,
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
) {
    if area.width < 3 || area.height == 0 {
        return;
    }

    let bg = t.surface_bg();
    let border_color = if complete {
        t.success()
    } else {
        t.accent_muted()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color).bg(bg))
        .title_top(
            top_right_timestamp(meta, t)
                .unwrap_or_else(Line::default)
                .right_aligned(),
        )
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(bg));
    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    // Assistant identity line.
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", presentation.sigil),
            Style::default()
                .fg(border_color)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if complete { "response" } else { "thinking" },
            Style::default().fg(t.border_dim()).bg(bg),
        ),
    ]));

    // Meta tag line: model / provider / tier — dim secondary header
    let meta_tag = build_meta_tag(meta);
    if !meta_tag.is_empty() {
        lines.push(Line::from(Span::styled(
            meta_tag,
            Style::default().fg(t.border_dim()).bg(bg),
        )));
    }

    // Thinking block — collapsed summary with line count
    if !thinking.is_empty() {
        let think_lines: Vec<&str> = thinking.lines().collect();
        let show = think_lines.len().min(6);
        lines.push(Line::from(vec![
            Span::styled("◌ ", Style::default().fg(t.border()).bg(bg)),
            Span::styled(
                "thinking ",
                Style::default()
                    .fg(t.dim())
                    .bg(bg)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::styled(
                format!("({} lines)", think_lines.len()),
                Style::default().fg(t.border_dim()).bg(bg),
            ),
        ]));
        for line in think_lines.iter().take(show) {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default()
                    .fg(t.border())
                    .bg(bg)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
        if think_lines.len() > show {
            lines.push(Line::from(Span::styled(
                format!("  ⋯ {} more", think_lines.len() - show),
                Style::default().fg(t.border_dim()).bg(bg),
            )));
        }
        lines.push(Line::from(Span::styled(
            "  ─ ─ ─",
            Style::default().fg(t.border_dim()).bg(bg),
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
                Style::default().fg(t.dim()).bg(bg),
            )));
        } else if in_code_fence {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(t.accent_muted()).bg(bg),
            )));
        } else if is_table_line(line) {
            let is_header = match table_state {
                TableState::None => {
                    table_state = TableState::Header;
                    true
                }
                TableState::Header if is_table_separator(line) => {
                    table_state = TableState::Body;
                    false
                }
                _ => {
                    table_state = TableState::Body;
                    false
                }
            };
            lines.push(render_table_line(line, is_header, t));
        } else {
            table_state = TableState::None;
            lines.push(super::widgets::highlight_line(line, t));
        }
    }

    if !complete && text.is_empty() && thinking.is_empty() {
        lines.push(Line::from(Span::styled("…", t.style_dim().bg(bg))));
    }

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(bg))
        .render(inner, buf);
}

fn render_tool_card(
    name: &str,
    detail_args: Option<&str>,
    detail_result: Option<&str>,
    is_error: bool,
    complete: bool,
    expanded: bool,
    meta: &SegmentMeta,
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
) {
    let summarize_args = |tool_name: &str, args: Option<&str>| -> Option<String> {
        let args = args?;
        match tool_name {
            "edit" | "change" => serde_json::from_str::<serde_json::Value>(args)
                .ok()
                .and_then(|v| {
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
                })
                .or_else(|| Some(crate::util::truncate(args, 80))),
            "read" | "write" | "view" | "bash" => {
                Some(args.lines().next().unwrap_or(args).to_string())
            }
            _ => None,
        }
    };

    let (icon, status_color) = if complete {
        if is_error {
            ("✗", t.error())
        } else {
            ("✓", t.success())
        }
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

    let timestamp = format_timestamp(meta.timestamp);
    let title = tool_title_line(icon, status_color, display_name, area.width, timestamp.as_deref());

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
        .title_top(title)
        .title_top(
            timestamp
                .as_deref()
                .map(|stamp| {
                    Line::from(Span::styled(
                        stamp.to_string(),
                        Style::default().fg(t.dim()).add_modifier(Modifier::DIM),
                    ))
                })
                .unwrap_or_else(Line::default)
                .right_aligned(),
        )
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(bg));

    let card_inner = card_block.inner(area);
    card_block.render(area, buf);

    if card_inner.height == 0 || card_inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line<'_>> = Vec::new();

    if let Some(summary) = summarize_args(name, detail_args) {
        lines.push(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(t.accent_muted()).bg(bg)),
            Span::styled(summary, Style::default().fg(t.fg()).bg(bg)),
        ]));
    }

    // ── Args section ────────────────────────────────────────────
    if let Some(args) = detail_args {
        match name {
            "bash" => {
                for (i, line) in args.lines().take(4).enumerate().skip(1) {
                    let prefix = if i == 0 { "$ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(t.dim()).bg(bg)),
                        Span::styled(line.to_string(), Style::default().fg(t.fg()).bg(bg)),
                    ]));
                }
            }
            "edit" | "change" => {
                // Summary line already rendered above; don't dump raw JSON payloads.
            }
            "read" | "write" | "view" => {
                // Summary line already rendered above; body/result carries the useful payload.
            }
            _ => {
                // Pretty-print JSON args if applicable
                let display_args = if args.starts_with('{') || args.starts_with('[') {
                    serde_json::from_str::<serde_json::Value>(args)
                        .ok()
                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                        .unwrap_or_else(|| args.to_string())
                } else {
                    args.to_string()
                };
                for line in display_args.lines().take(if expanded { 50 } else { 4 }) {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(t.dim()).bg(bg),
                    )));
                }
            }
        }
    }

    // ── Result section with distinct background ─────────────────
    if let Some(result) = detail_result {
        if !lines.is_empty() {
            // Separator line — matches card border color (red on error)
            let sep_color = if is_error { t.error() } else { t.border_dim() };
            let sep_bg = if is_error {
                t.tool_error_bg()
            } else {
                t.surface_bg()
            };
            lines.push(Line::from(Span::styled(
                "─".repeat(card_inner.width as usize),
                Style::default().fg(sep_color).bg(sep_bg),
            )));
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
                let spans: Vec<Span<'_>> = line
                    .spans
                    .into_iter()
                    .map(|mut s| {
                        s.style = s.style.bg(t.surface_bg());
                        s
                    })
                    .collect();
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
                        let spans: Vec<Span<'_>> = line
                            .spans
                            .into_iter()
                            .map(|mut s| {
                                // Preserve ANSI foreground, apply surface_bg
                                s.style = s.style.bg(t.surface_bg());
                                // If no foreground was set by ANSI, use muted
                                if s.style.fg.is_none() {
                                    s.style = s.style.fg(t.muted());
                                }
                                s
                            })
                            .collect();
                        lines.push(Line::from(spans));
                    }
                } else {
                    // ANSI parse failed — fall back to plain
                    for line in &result_lines[..show] {
                        lines.push(Line::from(Span::styled(line.to_string(), result_style)));
                    }
                }
            } else {
                for line in &result_lines[..show] {
                    lines.push(Line::from(Span::styled(line.to_string(), result_style)));
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
                Style::default().fg(t.accent_muted()).bg(t.surface_bg()),
            )));
        }
    }

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(card_inner, buf);

    // ── Post-render: OSC 8 hyperlinks for file paths ────────────
    if matches!(name, "read" | "edit" | "write" | "change" | "view")
        && let Some(args) = detail_args
    {
        let file_path = match name {
            "edit" | "change" => serde_json::from_str::<serde_json::Value>(args)
                .ok()
                .and_then(|v| {
                    v.get("file")
                        .or(v.get("path"))
                        .and_then(|f| f.as_str().map(String::from))
                })
                .unwrap_or_else(|| args.lines().next().unwrap_or(args).trim().to_string()),
            _ => args.lines().next().unwrap_or(args).trim().to_string(),
        };
        if !file_path.is_empty() && card_inner.height > 0 {
            let url = format!("file://{file_path}");
            let link_area = Rect {
                x: card_inner.x,
                y: card_inner.y, // first line is the file path
                width: card_inner.width.min(file_path.len() as u16),
                height: 1,
            };
            let link = hyperrat::Link::new(file_path, url).style(
                Style::default()
                    .fg(t.accent_muted())
                    .bg(bg)
                    .add_modifier(Modifier::UNDERLINED),
            );
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
enum TableState {
    None,
    Header,
    Body,
}

/// Detect markdown table lines: `| cell | cell |` or `|---|---|`
fn is_table_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2
}

/// Detect table separator: `|---|---|` or `| --- | --- |`
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.ends_with('|')
        && trimmed
            .chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

/// Render a markdown table line with cell highlighting.
fn render_table_line<'a>(line: &str, is_header: bool, t: &dyn Theme) -> Line<'a> {
    let trimmed = line.trim();
    let row_bg = if is_header {
        t.card_bg()
    } else {
        t.surface_bg()
    };

    // Separator row: |---|---| → render as a thin rule
    if is_table_separator(trimmed) {
        let sep_bg = t.surface_bg();
        let sep_fg = t.border();
        let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();
        let mut spans: Vec<Span<'a>> = Vec::new();
        spans.push(Span::styled("├", Style::default().fg(sep_fg).bg(sep_bg)));
        for (i, cell) in cells.iter().enumerate() {
            let w = cell.len().max(1);
            spans.push(Span::styled(
                "─".repeat(w),
                Style::default().fg(sep_fg).bg(sep_bg),
            ));
            if i < cells.len() - 1 {
                spans.push(Span::styled("┼", Style::default().fg(sep_fg).bg(sep_bg)));
            }
        }
        spans.push(Span::styled("┤", Style::default().fg(sep_fg).bg(sep_bg)));
        return Line::from(spans);
    }

    // Content row: | cell | cell |
    let mut spans: Vec<Span<'a>> = Vec::new();
    let cells: Vec<&str> = trimmed.split('|').filter(|s| !s.is_empty()).collect();

    let pipe = Style::default().fg(t.border()).bg(row_bg);

    spans.push(Span::styled("│", pipe));
    for (i, cell) in cells.iter().enumerate() {
        let cell_text = cell.trim();
        if is_header {
            // Header cells: bright accent, bold, slightly different background
            spans.push(Span::styled(
                format!(" {cell_text} "),
                Style::default()
                    .fg(t.accent_bright())
                    .bg(row_bg)
                    .add_modifier(Modifier::BOLD),
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
    if area.width < 3 || area.height == 0 {
        return;
    }

    let bg = t.card_bg();
    let block = Block::default().style(Style::default().bg(bg));
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
            Style::default()
                .fg(t.accent())
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        } else if i == 0 && (line.starts_with('⚠') || line.starts_with('⟳')) {
            Style::default().fg(t.warning()).bg(bg)
        } else if line.starts_with("  ▸") || line.starts_with("  /") || line.starts_with("  Ctrl")
        {
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
    if area.width < 4 || area.height == 0 {
        return;
    }
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
    path: &std::path::Path,
    alt: &str,
    area: Rect,
    buf: &mut Buffer,
    t: &dyn Theme,
) {
    if area.height == 0 {
        return;
    }

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");
    let label = if alt.is_empty() || alt == "clipboard paste" {
        format!(" 📎 {filename} ")
    } else {
        format!(" 📎 {alt} ")
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(t.border_dim()))
        .title(Span::styled(label, Style::default().fg(t.accent_muted())))
        .style(Style::default().bg(t.surface_bg()));

    // The block is the placeholder — the actual image is rendered on top
    // of this area in a second pass by the ConversationWidget (ratatui-image).
    block.render(area, buf);
}

fn render_separator(area: Rect, buf: &mut Buffer, t: &dyn Theme) {
    if area.height == 0 || area.width < 4 {
        return;
    }
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
        let seg = Segment::user_prompt("hello world");
        let (area, mut buf) = make_buf(40, 5);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert_eq!(seg.role(), SegmentRole::Operator);
        assert_eq!(seg.presentation().sigil, "OP");
        assert!(text.contains("hello world"), "should have text");
        assert!(
            text.contains("╭") || text.contains("╰") || text.contains("│"),
            "should render as a bordered card: {text}"
        );
    }

    #[test]
    fn assistant_segment_has_explicit_presentation_role() {
        let seg = Segment::assistant_text();
        assert_eq!(seg.role(), SegmentRole::Assistant);
        assert_eq!(seg.presentation().sigil, "Ω");
        assert_eq!(seg.presentation().emphasis, SegmentEmphasis::Normal);
        assert_eq!(seg.presentation().tool_visual, None);
    }

    #[test]
    fn tool_visual_kinds_are_classified() {
        let cases = [
            (Segment::tool_card("1", "read"), ToolVisualKind::FileRead),
            (Segment::tool_card("1", "bash"), ToolVisualKind::CommandExec),
            (
                Segment::tool_card("1", "design_tree"),
                ToolVisualKind::DesignTree,
            ),
            (
                Segment::tool_card("1", "memory_query"),
                ToolVisualKind::Memory,
            ),
            (
                Segment::tool_card("1", "web_search"),
                ToolVisualKind::Search,
            ),
            (
                Segment::tool_card("1", "write"),
                ToolVisualKind::FileMutation,
            ),
        ];
        for (seg, expected) in cases {
            assert_eq!(seg.presentation().tool_visual, Some(expected));
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
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(
            text.contains("Ω"),
            "assistant header should include Ω sigil: {text}"
        );
        assert!(
            text.contains("response"),
            "assistant header should describe the segment role: {text}"
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
        assert_eq!(formatted.len(), 5);
        assert_eq!(&formatted[2..3], ":");
        assert!(formatted.chars().take(2).all(|c| c.is_ascii_digit()));
        assert!(formatted.chars().skip(3).all(|c| c.is_ascii_digit()));
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
            },
        };
        let (area, mut buf) = make_buf(80, 8);
        seg.render(area, &mut buf, &Alpharius);
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
            },
        };
        let (area, mut buf) = make_buf(60, 10);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("╭"), "should have top border: {text}");
        assert!(text.contains("╰"), "should have bottom border: {text}");
        assert!(
            text.contains("list"),
            "should have display name for ls: {text}"
        );
        assert!(text.contains("✓"), "should have checkmark: {text}");
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
                detail_args: Some("/very/long/path/to/some_extremely_verbose_filename_that_used_to_bleed.rs".into()),
                result_summary: None,
                detail_result: Some("fn main() {}".into()),
                is_error: false,
                complete: true,
                expanded: false,
            },
        };
        let (area, mut buf) = make_buf(28, 8);
        seg.render(area, &mut buf, &Alpharius);
        let top_row = (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>();
        assert!(top_row.contains("✓"), "top row should retain tool icon: {top_row}");
        assert!(top_row.contains("read") || top_row.contains("rea…"), "top row should retain truncated tool label: {top_row}");
        assert!(!top_row.contains("filename_that_used_to_bleed"), "long header text should be truncated before colliding with the rest of the title row: {top_row}");
    }

    #[test]
    fn tool_title_redraw_clears_stale_suffix_characters() {
        let long = Segment {
            meta: SegmentMeta::default(),
            content: SegmentContent::ToolCard {
                id: "1".into(),
                name: "read".into(),
                args_summary: None,
                detail_args: Some("/Users/cwilson/workspace/black-meridian/omegon/core/Cargo.toml".into()),
                result_summary: None,
                detail_result: Some("[package]".into()),
                is_error: false,
                complete: true,
                expanded: false,
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
            },
        };

        let (area, mut buf) = make_buf(24, 8);
        long.render(area, &mut buf, &Alpharius);
        short.render(area, &mut buf, &Alpharius);

        let top_row = (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>();
        assert!(top_row.contains("read"), "top row should contain the current tool label: {top_row}");
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
            },
        };
        let (area, mut buf) = make_buf(60, 8);
        seg.render(area, &mut buf, &Alpharius);
        let text = buf_text(&buf, area);
        assert!(text.contains("✗"), "should have error icon: {text}");
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
        seg.render(area, &mut buf, &Alpharius);
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
        assert!(h >= 3 && h <= 7, "user prompt height: {h}");

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
        assert!(
            text.contains("Name"),
            "header should contain cell text: {text}"
        );
        assert!(
            text.contains("│"),
            "should contain box drawing separator: {text}"
        );

        let body = render_table_line("| foo | bar |", false, &Alpharius);
        let body_text: String = body.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            body_text.contains("foo"),
            "body should contain cell text: {body_text}"
        );

        let sep = render_table_line("|---|---|", false, &Alpharius);
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
            },
        };
        let (area, mut buf) = make_buf(80, 12);
        seg.render(area, &mut buf, &Alpharius);
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
            },
        };
        let (area, mut buf) = make_buf(80, 10);
        seg.render(area, &mut buf, &Alpharius);
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
    fn meta_tag_omits_thinking_off() {
        let meta = SegmentMeta {
            model_id: Some("gpt-4o".into()),
            thinking_level: Some("off".into()),
            ..Default::default()
        };
        let tag = build_meta_tag(&meta);
        assert!(!tag.contains("think"), "should omit think:off: {tag}");
    }
}
