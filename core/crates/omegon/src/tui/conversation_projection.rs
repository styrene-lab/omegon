//! Conversation projection types shared by TUI segment rendering surfaces.
//!
//! This module is the first seam between conversation data (`SegmentContent`) and
//! terminal rendering. It owns presentation classification that can be reasoned
//! about without mutating the underlying conversation state.

use ratatui::prelude::Color;

use super::theme::Theme;

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

impl ToolVisualKind {
    /// Categorical color for this tool kind — subtle tinting for completed
    /// tool card borders and focus-mode gutters. Each kind gets a distinct
    /// hue so operators can scan the timeline by color. The palette stays
    /// within the Alpharius tonal range (no new hues invented).
    pub fn color(&self, t: &dyn Theme) -> Color {
        match self {
            Self::CommandExec => t.warning(),      // orange — shell activity
            Self::FileRead => t.accent_muted(),    // teal — information retrieval
            Self::FileMutation => t.caution(),     // lime — file changes
            Self::DesignTree => t.accent_bright(), // bright cyan — structural
            Self::Memory => t.accent(),            // cyan — storage/recall
            Self::Search => t.accent_muted(),      // teal — lookup
            Self::Generic => t.border_dim(),       // neutral
        }
    }

    /// Short label for focus-mode display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::CommandExec => "exec",
            Self::FileRead => "read",
            Self::FileMutation => "mutate",
            Self::DesignTree => "design",
            Self::Memory => "memory",
            Self::Search => "search",
            Self::Generic => "tool",
        }
    }
}

pub fn tool_visual_kind_for_name(name: &str) -> ToolVisualKind {
    match name {
        "bash" => ToolVisualKind::CommandExec,
        "read" | "view" => ToolVisualKind::FileRead,
        "write" | "edit" | "change" => ToolVisualKind::FileMutation,
        "design_tree" | "design_tree_update" | "openspec_manage" | "lifecycle_doctor" => {
            ToolVisualKind::DesignTree
        }
        name if name.starts_with("memory_") => ToolVisualKind::Memory,
        "web_search" => ToolVisualKind::Search,
        _ => ToolVisualKind::Generic,
    }
}

pub fn presentation_for_role(
    role: SegmentRole,
    tool_visual: Option<ToolVisualKind>,
) -> SegmentPresentation {
    match role {
        SegmentRole::Operator => SegmentPresentation {
            role,
            sigil: "OP",
            emphasis: SegmentEmphasis::Strong,
            tool_visual: None,
        },
        SegmentRole::Assistant => SegmentPresentation {
            role,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_visual: None,
        },
        SegmentRole::Tool => SegmentPresentation {
            role,
            sigil: "⚙",
            emphasis: SegmentEmphasis::Normal,
            tool_visual,
        },
        SegmentRole::System => SegmentPresentation {
            role,
            sigil: "ℹ",
            emphasis: SegmentEmphasis::Muted,
            tool_visual: None,
        },
        SegmentRole::Lifecycle => SegmentPresentation {
            role,
            sigil: "⚡",
            emphasis: SegmentEmphasis::Muted,
            tool_visual: None,
        },
        SegmentRole::Media => SegmentPresentation {
            role,
            sigil: "◈",
            emphasis: SegmentEmphasis::Normal,
            tool_visual: None,
        },
        SegmentRole::Separator => SegmentPresentation {
            role,
            sigil: "",
            emphasis: SegmentEmphasis::Muted,
            tool_visual: None,
        },
    }
}
