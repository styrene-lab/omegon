//! Ratatui-facing conversation render projection traits.
//!
//! This module is the adapter seam between semantic conversation projections and
//! terminal rendering. It lets the scroll/widget layer measure, render, and query
//! segment render metadata without pattern-matching on the underlying domain
//! segment enum.

use ratatui::prelude::*;

use super::segments::{Segment, SegmentRenderMode};
use super::theme::Theme;
use crate::surfaces::conversation::{
    SegmentSelectionTreatment, SegmentSurfacePolicy, SegmentSurfaceTreatment, ToolCategory,
};

pub fn tool_category_color(_kind: ToolCategory, t: &dyn Theme) -> Color {
    // Category is identity, not hierarchy. Keep every tool category at the same
    // neutral luminance; glyph and label distinguish kind. Brighter and dimmer
    // grays remain available to encode prominence, selection, and de-emphasis.
    t.muted()
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
    crate::surfaces::conversation::tool_visual_identity(name, detail_args).label
}
pub fn tool_display_label(
    name: &str,
    detail_args: Option<&str>,
    provenance: &omegon_traits::ToolProvenance,
) -> String {
    let display_name = tool_display_name(name, detail_args);
    match provenance {
        omegon_traits::ToolProvenance::BuiltIn => display_name,
        omegon_traits::ToolProvenance::Extension { name: extension } => {
            format!("{display_name} ({extension})")
        }
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
            t.accent(),
            t.accent_muted(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSelectionChrome {
    None,
    Subtle,
    Marker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSegmentPaint {
    pub clear_bg: Color,
    pub text_bg: Option<Color>,
    pub surface_bg: Option<Color>,
    pub full_width_surface: bool,
    pub selection_chrome: TerminalSelectionChrome,
}

pub fn terminal_segment_paint(
    surface: SegmentSurfacePolicy,
    ctx: &SegmentRenderContext<'_>,
) -> TerminalSegmentPaint {
    let transcript = matches!(surface.surface, SegmentSurfaceTreatment::Transcript);
    let copy_friendly_transcript = ctx.copy_friendly && transcript;
    let full_width_surface = !copy_friendly_transcript
        && !matches!(surface.surface, SegmentSurfaceTreatment::ChromeOnly);
    let clear_bg = if copy_friendly_transcript {
        ctx.theme.bg()
    } else {
        ctx.theme.surface_bg()
    };
    let surface_bg = full_width_surface.then_some(match surface.surface {
        SegmentSurfaceTreatment::Transcript => ctx.theme.surface_bg(),
        SegmentSurfaceTreatment::Card => ctx.theme.card_bg(),
        SegmentSurfaceTreatment::Panel => ctx.theme.surface_bg(),
        SegmentSurfaceTreatment::ChromeOnly => ctx.theme.bg(),
    });
    let text_bg = if copy_friendly_transcript {
        None
    } else {
        surface_bg.or(Some(clear_bg))
    };
    let selection_chrome = if !ctx.selected {
        TerminalSelectionChrome::None
    } else if ctx.copy_friendly {
        match surface.selection {
            SegmentSelectionTreatment::None => TerminalSelectionChrome::None,
            SegmentSelectionTreatment::Subtle | SegmentSelectionTreatment::Explicit => {
                TerminalSelectionChrome::Subtle
            }
        }
    } else {
        match surface.selection {
            SegmentSelectionTreatment::None => TerminalSelectionChrome::None,
            SegmentSelectionTreatment::Subtle => TerminalSelectionChrome::Subtle,
            SegmentSelectionTreatment::Explicit => TerminalSelectionChrome::Marker,
        }
    };

    TerminalSegmentPaint {
        clear_bg,
        text_bg,
        surface_bg,
        full_width_surface,
        selection_chrome,
    }
}

#[derive(Clone, Copy)]
pub struct SegmentRenderContext<'a> {
    pub theme: &'a dyn Theme,
    pub mode: SegmentRenderMode,
    pub density: crate::settings::ToolDetail,
    pub pinned: bool,
    pub selected: bool,
    pub copy_friendly: bool,
}

impl<'a> SegmentRenderContext<'a> {
    pub fn new(theme: &'a dyn Theme, mode: SegmentRenderMode) -> Self {
        Self {
            theme,
            mode,
            density: crate::settings::ToolDetail::Detailed,
            pinned: false,
            selected: false,
            copy_friendly: matches!(mode, SegmentRenderMode::Slim),
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

    pub fn with_copy_friendly(mut self, copy_friendly: bool) -> Self {
        self.copy_friendly = copy_friendly;
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
        self.height_with_context(width, ctx)
    }
}

impl SegmentRender for Segment {
    fn render_in_context(&self, area: Rect, buf: &mut Buffer, ctx: &SegmentRenderContext<'_>) {
        self.render_with_context(area, buf, ctx);
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
    fn tool_display_name_shortens_common_compound_tools() {
        assert_eq!(tool_display_name("codebase_search", None), "codebase");
        assert_eq!(tool_display_name("search_documents", None), "docs");
        assert_eq!(tool_display_name("memory_recall", None), "mem read");
        assert_eq!(tool_display_name("request_context", None), "context");
        assert_eq!(tool_display_name("wait_for_operator", None), "tool");
        assert_eq!(tool_display_name("browser_search", None), "browser");
    }

    #[test]
    fn tool_display_label_discloses_extension_without_renaming_builtins() {
        assert_eq!(
            tool_display_label("read", None, &omegon_traits::ToolProvenance::BuiltIn),
            "read"
        );
        assert_eq!(
            tool_display_label(
                "read",
                None,
                &omegon_traits::ToolProvenance::Extension {
                    name: "recro-coe-agent".into(),
                },
            ),
            "read (recro-coe-agent)"
        );
        assert_eq!(
            tool_display_label(
                "bash",
                Some("cargo check"),
                &omegon_traits::ToolProvenance::Extension {
                    name: "recro-coe-agent".into(),
                },
            ),
            "cargo (recro-coe-agent)"
        );
    }

    #[test]
    fn tool_categories_share_one_neutral_luminance() {
        for category in [
            ToolCategory::CommandExec,
            ToolCategory::FileRead,
            ToolCategory::FileMutation,
            ToolCategory::DesignTree,
            ToolCategory::Memory,
            ToolCategory::Search,
            ToolCategory::Subagent,
            ToolCategory::Network,
            ToolCategory::Generic,
        ] {
            assert_eq!(
                tool_category_color(category, &Alpharius),
                Alpharius.muted(),
                "{category:?} encoded category as luminance hierarchy"
            );
        }
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
    fn running_tool_uses_active_teal_without_attention_orange() {
        let chrome = tool_card_chrome(
            "bash",
            Some("cargo check"),
            false,
            false,
            Some(ToolCategory::CommandExec),
            &Alpharius,
        );
        assert_eq!(chrome.status_color, Alpharius.accent());
        assert_eq!(chrome.border_color, Alpharius.accent_muted());
        assert_ne!(chrome.status_color, Alpharius.warning());
        assert_ne!(chrome.border_color, Alpharius.warning());
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
        assert_eq!(
            chrome.status_icon,
            crate::tui::glyphs::glyphs().tool(crate::tui::glyphs::ToolGlyphRole::Failed)
        );
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

    #[test]
    fn terminal_paint_keeps_slim_transcript_copy_friendly() {
        let projection = crate::surfaces::conversation::ConversationSegmentProjection::<&str>::new(
            crate::surfaces::conversation::ConversationSegmentKind::Assistant(
                crate::surfaces::conversation::AssistantSegment {
                    text: "answer",
                    thinking: "",
                    complete: true,
                },
            ),
        );
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Slim);

        let paint = terminal_segment_paint(projection.presentation_model().surface, &ctx);

        assert_eq!(paint.clear_bg, Alpharius.bg());
        assert_eq!(paint.text_bg, None);
        assert_eq!(paint.surface_bg, None);
        assert!(!paint.full_width_surface);
    }

    #[test]
    fn terminal_paint_keeps_tool_cards_structured_in_slim() {
        let projection = crate::surfaces::conversation::ConversationSegmentProjection::<&str>::new(
            crate::surfaces::conversation::ConversationSegmentKind::Tool(
                crate::surfaces::conversation::ToolSegment {
                    id: "tool-1",
                    name: "bash",
                    args_summary: Some("cargo check"),
                    detail_args: None,
                    result_summary: Some("ok"),
                    detail_result: Some("ok"),
                    is_error: false,
                    complete: true,
                    expanded: false,
                },
            ),
        );
        let ctx = SegmentRenderContext::new(&Alpharius, SegmentRenderMode::Slim);

        let paint = terminal_segment_paint(projection.presentation_model().surface, &ctx);

        assert_eq!(paint.surface_bg, Some(Alpharius.card_bg()));
        assert!(paint.full_width_surface);
    }
}
