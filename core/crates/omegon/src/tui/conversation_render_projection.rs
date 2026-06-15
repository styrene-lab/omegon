//! Ratatui-facing conversation render projection traits.
//!
//! This module is the adapter seam between semantic conversation projections and
//! terminal rendering. It lets the scroll/widget layer measure, render, and query
//! segment render metadata without pattern-matching on the underlying domain
//! segment enum.

use ratatui::prelude::*;

use super::segments::{Segment, SegmentRenderMode};
use super::theme::Theme;
use crate::surfaces::conversation::ToolCategory;

pub fn tool_category_color(kind: ToolCategory, t: &dyn Theme) -> Color {
    match kind {
        ToolCategory::CommandExec => t.warning(),
        ToolCategory::FileRead => t.accent_muted(),
        ToolCategory::FileMutation => t.caution(),
        ToolCategory::DesignTree => t.accent_bright(),
        ToolCategory::Memory => t.accent(),
        ToolCategory::Search => t.accent_muted(),
        ToolCategory::Generic => t.border_dim(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentChrome {
    pub role_label: &'static str,
    pub sigil: &'static str,
    pub role_color: Color,
    pub content_color: Color,
}

pub fn segment_chrome(
    presentation: crate::surfaces::conversation::SegmentPresentation,
    selected: bool,
    t: &dyn Theme,
) -> SegmentChrome {
    use crate::surfaces::conversation::SegmentRole;

    let (role_label, sigil, role_color) = match presentation.role {
        SegmentRole::Operator => ("operator", "OP", t.accent()),
        SegmentRole::Assistant => ("assistant", "Ω", t.success()),
        SegmentRole::PeerAgent => ("peer agent", "⬡", t.accent()),
        SegmentRole::Tool => {
            let category = presentation.tool_category.unwrap_or(ToolCategory::Generic);
            (category.label(), "⚙", tool_category_color(category, t))
        }
        SegmentRole::System => ("system", "ℹ", t.dim()),
        SegmentRole::Lifecycle => ("event", "↯", t.dim()),
        SegmentRole::Media => ("media", "◈", t.accent_muted()),
        SegmentRole::Separator => ("separator", "", t.dim()),
    };
    let content_color = match presentation.role {
        SegmentRole::Tool if !selected => t.muted(),
        _ => t.fg(),
    };

    SegmentChrome {
        role_label,
        sigil,
        role_color,
        content_color,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCardChrome {
    pub display_name: String,
    pub status_icon: &'static str,
    pub status_color: Color,
    pub border_color: Color,
    pub background: Color,
    pub category_color: Color,
}

pub fn tool_display_name(name: &str, detail_args: Option<&str>) -> String {
    if name == "bash" {
        if let Some(args) = detail_args {
            let command = crate::tui::segments::shell_command_from_args(args);
            let cmd = command
                .as_deref()
                .unwrap_or(args.lines().next().unwrap_or(args));
            let first_word = cmd.split_whitespace().next().unwrap_or("bash");
            match first_word {
                "grep" | "rg" => "search".to_string(),
                "find" => "find".to_string(),
                "ls" | "dir" => "list".to_string(),
                "cat" | "head" | "tail" | "bat" => "read".to_string(),
                "sed" | "awk" => "transform".to_string(),
                "curl" | "wget" => "fetch".to_string(),
                "git" => "git".to_string(),
                "cargo" => "cargo".to_string(),
                "npm" | "npx" | "pnpm" | "yarn" | "bun" => "npm".to_string(),
                "docker" | "podman" => "container".to_string(),
                "kubectl" | "k" => "kubectl".to_string(),
                "make" | "cmake" => "build".to_string(),
                "python" | "python3" | "pip" => "python".to_string(),
                "rustc" | "rustup" => "rust".to_string(),
                "go" => "go".to_string(),
                "dig" | "nslookup" | "host" => "dns".to_string(),
                "ssh" | "scp" | "rsync" => "remote".to_string(),
                "tar" | "zip" | "unzip" | "gzip" => "archive".to_string(),
                "wc" => "count".to_string(),
                "sort" | "uniq" => "sort".to_string(),
                "diff" | "patch" => "diff".to_string(),
                "mkdir" | "rm" | "mv" | "cp" | "chmod" | "chown" => "fs".to_string(),
                "echo" | "printf" => "echo".to_string(),
                "test" | "[" => "test".to_string(),
                "vault" => "vault".to_string(),
                "sh" | "bash" | "zsh" => "shell".to_string(),
                _ => first_word.to_string(),
            }
        } else {
            "shell".to_string()
        }
    } else {
        name.replace('_', " ")
    }
}

pub fn tool_card_chrome(
    name: &str,
    detail_args: Option<&str>,
    is_error: bool,
    complete: bool,
    tool_category: Option<ToolCategory>,
    t: &dyn Theme,
) -> ToolCardChrome {
    let display_name = tool_display_name(name, detail_args);
    let category_color = tool_category
        .map(|k| tool_category_color(k, t))
        .unwrap_or(t.accent_muted());
    let glyphs = crate::tui::glyphs::glyphs();
    let (status_icon, status_color, border_color, background) = if is_error {
        (
            glyphs.tool(crate::tui::glyphs::ToolGlyphRole::Failed),
            t.error(),
            t.error(),
            t.tool_error_bg(),
        )
    } else if !complete {
        (
            glyphs.tool(crate::tui::glyphs::ToolGlyphRole::Running),
            t.warning(),
            t.warning(),
            t.tool_success_bg(),
        )
    } else {
        let muted_border = crate::tui::segments::dim_color(category_color, 0.4);
        (
            glyphs.tool(crate::tui::glyphs::ToolGlyphRole::Completed),
            category_color,
            muted_border,
            t.tool_success_bg(),
        )
    };

    ToolCardChrome {
        display_name,
        status_icon,
        status_color,
        border_color,
        background,
        category_color,
    }
}

#[derive(Clone, Copy)]
pub struct SegmentRenderContext<'a> {
    pub theme: &'a dyn Theme,
    pub mode: SegmentRenderMode,
    pub density: crate::settings::ToolDetail,
    pub pinned: bool,
    pub selected: bool,
}

impl<'a> SegmentRenderContext<'a> {
    pub fn new(theme: &'a dyn Theme, mode: SegmentRenderMode) -> Self {
        Self {
            theme,
            mode,
            density: crate::settings::ToolDetail::Detailed,
            pinned: false,
            selected: false,
        }
    }

    pub fn with_density(mut self, density: crate::settings::ToolDetail) -> Self {
        self.density = density;
        self
    }

    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

pub trait SegmentMeasure {
    fn height_in_context(&self, width: u16, ctx: &SegmentRenderContext<'_>) -> u16;
}

pub trait SegmentRender {
    fn render_in_context(&self, area: Rect, buf: &mut Buffer, ctx: &SegmentRenderContext<'_>);
}

pub trait SegmentRenderMetadata {
    fn is_live_render_segment(&self) -> bool;
    fn is_image_render_segment(&self) -> bool;
}

pub trait RenderableConversationSegment:
    SegmentMeasure + SegmentRender + SegmentRenderMetadata
{
}

impl<T> RenderableConversationSegment for T where
    T: SegmentMeasure + SegmentRender + SegmentRenderMetadata
{
}

impl SegmentMeasure for Segment {
    fn height_in_context(&self, width: u16, ctx: &SegmentRenderContext<'_>) -> u16 {
        self.height_in_mode(width, ctx.theme, ctx.mode)
    }
}

impl SegmentRender for Segment {
    fn render_in_context(&self, area: Rect, buf: &mut Buffer, ctx: &SegmentRenderContext<'_>) {
        self.render_with_pinned(area, buf, ctx.theme, ctx.mode, ctx.density, ctx.pinned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{
        SegmentEmphasis, SegmentPresentation, SegmentRole, ToolCategory,
    };
    use crate::tui::theme::Alpharius;

    fn presentation(role: SegmentRole, tool_category: Option<ToolCategory>) -> SegmentPresentation {
        SegmentPresentation {
            role,
            sigil: "",
            emphasis: SegmentEmphasis::Normal,
            tool_category,
        }
    }

    #[test]
    fn tool_display_name_classifies_shell_commands() {
        assert_eq!(
            tool_display_name("bash", Some(r#"{"command":"cargo check"}"#)),
            "cargo"
        );
        assert_eq!(tool_display_name("bash", Some("rg needle src")), "search");
        assert_eq!(tool_display_name("read", None), "read");
    }

    #[test]
    fn tool_card_chrome_uses_category_color_for_completed_tools() {
        let chrome = tool_card_chrome(
            "bash",
            Some("cargo check"),
            false,
            true,
            Some(ToolCategory::CommandExec),
            &Alpharius,
        );
        assert_eq!(chrome.display_name, "cargo");
        assert_eq!(
            chrome.status_icon,
            crate::tui::glyphs::glyphs().tool(crate::tui::glyphs::ToolGlyphRole::Completed)
        );
        assert_eq!(
            chrome.status_color,
            tool_category_color(ToolCategory::CommandExec, &Alpharius)
        );
        assert_eq!(chrome.background, Alpharius.tool_success_bg());
    }

    #[test]
    fn tool_card_chrome_prioritizes_error_state() {
        let chrome = tool_card_chrome(
            "write",
            None,
            true,
            true,
            Some(ToolCategory::FileMutation),
            &Alpharius,
        );
        assert_eq!(chrome.status_icon, "✗");
        assert_eq!(chrome.status_color, Alpharius.error());
        assert_eq!(chrome.border_color, Alpharius.error());
        assert_eq!(chrome.background, Alpharius.tool_error_bg());
    }

    #[test]
    fn segment_chrome_maps_operator_identity() {
        let chrome = segment_chrome(presentation(SegmentRole::Operator, None), false, &Alpharius);
        assert_eq!(chrome.role_label, "operator");
        assert_eq!(chrome.sigil, "OP");
        assert_eq!(chrome.role_color, Alpharius.accent());
        assert_eq!(chrome.content_color, Alpharius.fg());
    }

    #[test]
    fn segment_chrome_mutes_unselected_tool_content() {
        let chrome = segment_chrome(
            presentation(SegmentRole::Tool, Some(ToolCategory::CommandExec)),
            false,
            &Alpharius,
        );
        assert_eq!(chrome.role_label, "exec");
        assert_eq!(chrome.sigil, "⚙");
        assert_eq!(
            chrome.role_color,
            tool_category_color(ToolCategory::CommandExec, &Alpharius)
        );
        assert_eq!(chrome.content_color, Alpharius.muted());
    }

    #[test]
    fn segment_chrome_selected_tool_uses_foreground_content() {
        let chrome = segment_chrome(
            presentation(SegmentRole::Tool, Some(ToolCategory::Memory)),
            true,
            &Alpharius,
        );
        assert_eq!(chrome.role_label, "memory");
        assert_eq!(chrome.content_color, Alpharius.fg());
    }
}
