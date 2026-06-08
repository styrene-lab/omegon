//! Shared semantic conversation surface projection.
//!
//! This module is renderer- and transport-neutral. It describes conversation
//! segments in terms that TUI renderers, ACP DTO adapters, exports, and future
//! clients can consume without depending on Ratatui, terminal styling, or ACP
//! wire types.
//!
//! Keep this layer semantic: roles, segment payloads, completion state, and tool
//! categories belong here; colors, protocol field names, redaction policy, and
//! widget layout belong in downstream adapters.

use std::path::{Path, PathBuf};

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

/// Semantic presentation hints common to all surface adapters.
///
/// `tool_category` is intentionally not a color/style. Renderers map it to
/// visual treatment; protocol adapters map it to metadata strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentPresentation {
    pub role: SegmentRole,
    pub sigil: &'static str,
    pub emphasis: SegmentEmphasis,
    pub tool_category: Option<ToolCategory>,
}

/// Typed, presentation-ready segment projection.
///
/// The type parameters let callers choose owned (`String`, `PathBuf`) or
/// borrowed (`&str`, `&Path`) payloads. That keeps this projection layer usable
/// both for cheap per-frame views over `SegmentContent` and for durable tests or
/// export snapshots that need owned data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSegmentProjection<TText = String, TPath = PathBuf>
where
    TText: AsRef<str>,
{
    pub presentation: SegmentPresentation,
    pub kind: ConversationSegmentKind<TText, TPath>,
}

impl<TText, TPath> ConversationSegmentProjection<TText, TPath>
where
    TText: AsRef<str>,
{
    pub fn new(kind: ConversationSegmentKind<TText, TPath>) -> Self {
        let tool_category = kind.tool_category();
        Self {
            presentation: presentation_for_role(kind.role(), tool_category),
            kind,
        }
    }

    pub fn role(&self) -> SegmentRole {
        self.presentation.role
    }
}

/// Segment-specific projection payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationSegmentKind<TText = String, TPath = PathBuf> {
    User(UserSegment<TText>),
    Assistant(AssistantSegment<TText>),
    Tool(ToolSegment<TText>),
    System(SystemSegment<TText>),
    Lifecycle(LifecycleSegment<TText>),
    Image(ImageSegment<TText, TPath>),
    Separator,
}

impl<TText, TPath> ConversationSegmentKind<TText, TPath> {
    pub fn role(&self) -> SegmentRole {
        match self {
            Self::User(_) => SegmentRole::Operator,
            Self::Assistant(_) => SegmentRole::Assistant,
            Self::Tool(_) => SegmentRole::Tool,
            Self::System(_) => SegmentRole::System,
            Self::Lifecycle(_) => SegmentRole::Lifecycle,
            Self::Image(_) => SegmentRole::Media,
            Self::Separator => SegmentRole::Separator,
        }
    }
}

impl<TText, TPath> ConversationSegmentKind<TText, TPath>
where
    TText: AsRef<str>,
{
    pub fn tool_category(&self) -> Option<ToolCategory> {
        match self {
            Self::Tool(tool) => Some(tool_category_for_name(tool.name.as_ref())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSegment<TText = String> {
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantSegment<TText = String> {
    pub text: TText,
    pub thinking: TText,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSegment<TText = String> {
    pub id: TText,
    pub name: TText,
    pub args_summary: Option<TText>,
    pub detail_args: Option<TText>,
    pub result_summary: Option<TText>,
    pub detail_result: Option<TText>,
    pub is_error: bool,
    pub complete: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSegment<TText = String> {
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleSegment<TText = String> {
    pub icon: TText,
    pub text: TText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSegment<TText = String, TPath = PathBuf> {
    pub path: TPath,
    pub alt: TText,
}

pub trait ProjectConversationSegment<'a> {
    type Text: AsRef<str>;
    type Path;

    fn project_conversation_segment(
        &'a self,
    ) -> ConversationSegmentProjection<Self::Text, Self::Path>;
}

pub type BorrowedConversationSegmentProjection<'a> =
    ConversationSegmentProjection<&'a str, &'a Path>;

pub type OwnedConversationSegmentProjection = ConversationSegmentProjection<String, PathBuf>;

/// Semantic category for known tool families.
///
/// This is shared classification, not presentation. TUI maps it to colors and
/// labels; ACP maps it to stable metadata strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    CommandExec,
    FileRead,
    FileMutation,
    DesignTree,
    Memory,
    Search,
    Generic,
}

impl ToolCategory {
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

pub fn tool_category_for_name(name: &str) -> ToolCategory {
    match name {
        "bash" => ToolCategory::CommandExec,
        "read" | "view" => ToolCategory::FileRead,
        "write" | "edit" | "change" => ToolCategory::FileMutation,
        "design_tree" | "design_tree_update" | "openspec_manage" | "lifecycle_doctor" => {
            ToolCategory::DesignTree
        }
        name if name.starts_with("memory_") => ToolCategory::Memory,
        "web_search" => ToolCategory::Search,
        _ => ToolCategory::Generic,
    }
}

pub fn presentation_for_role(
    role: SegmentRole,
    tool_category: Option<ToolCategory>,
) -> SegmentPresentation {
    match role {
        SegmentRole::Operator => SegmentPresentation {
            role,
            sigil: "OP",
            emphasis: SegmentEmphasis::Strong,
            tool_category: None,
        },
        SegmentRole::Assistant => SegmentPresentation {
            role,
            sigil: "Ω",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        },
        SegmentRole::Tool => SegmentPresentation {
            role,
            sigil: "⚙",
            emphasis: SegmentEmphasis::Normal,
            tool_category,
        },
        SegmentRole::System => SegmentPresentation {
            role,
            sigil: "ℹ",
            emphasis: SegmentEmphasis::Muted,
            tool_category: None,
        },
        SegmentRole::Lifecycle => SegmentPresentation {
            role,
            sigil: "⚡",
            emphasis: SegmentEmphasis::Muted,
            tool_category: None,
        },
        SegmentRole::Media => SegmentPresentation {
            role,
            sigil: "◈",
            emphasis: SegmentEmphasis::Normal,
            tool_category: None,
        },
        SegmentRole::Separator => SegmentPresentation {
            role,
            sigil: "",
            emphasis: SegmentEmphasis::Muted,
            tool_category: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_infers_role_and_presentation_from_kind() {
        let projection = ConversationSegmentProjection::<&str>::new(
            ConversationSegmentKind::Assistant(AssistantSegment {
                text: "answer",
                thinking: "",
                complete: true,
            }),
        );

        assert_eq!(projection.role(), SegmentRole::Assistant);
        assert_eq!(projection.presentation.sigil, "Ω");
        assert_eq!(projection.presentation.emphasis, SegmentEmphasis::Normal);
        assert_eq!(projection.presentation.tool_category, None);
    }

    #[test]
    fn projection_parameterization_supports_borrowed_tool_payloads() {
        let projection = ConversationSegmentProjection::<&str>::new(ConversationSegmentKind::Tool(
            ToolSegment {
                id: "tool-1",
                name: "bash",
                args_summary: Some("cargo check"),
                detail_args: None,
                result_summary: Some("ok"),
                detail_result: None,
                is_error: false,
                complete: true,
                expanded: false,
            },
        ));

        assert_eq!(projection.role(), SegmentRole::Tool);
        assert_eq!(
            projection.presentation.tool_category,
            Some(ToolCategory::CommandExec)
        );
    }

    #[test]
    fn non_tool_roles_ignore_supplied_tool_category() {
        let presentation =
            presentation_for_role(SegmentRole::Assistant, Some(ToolCategory::Memory));
        assert_eq!(presentation.tool_category, None);
        assert_eq!(presentation.role, SegmentRole::Assistant);
    }

    #[test]
    fn tool_presentation_preserves_supplied_category() {
        let presentation = presentation_for_role(SegmentRole::Tool, Some(ToolCategory::Memory));
        assert_eq!(presentation.tool_category, Some(ToolCategory::Memory));
        assert_eq!(presentation.sigil, "⚙");
    }

    #[test]
    fn projection_parameterization_supports_owned_image_payloads() {
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::Image(ImageSegment {
                path: PathBuf::from("/tmp/screenshot.png"),
                alt: "screenshot".to_string(),
            }));

        assert_eq!(projection.role(), SegmentRole::Media);
        assert_eq!(projection.presentation.sigil, "◈");
    }
}
