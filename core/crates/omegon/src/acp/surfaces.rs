//! ACP-facing surface DTOs derived from semantic projections.
//!
//! These types are protocol adapter scaffolding: they are concrete, owned,
//! serde-serializable shapes with explicit identity and redaction policy. They
//! intentionally do not replace the existing ACP `SessionUpdate` stream yet.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::tui::conversation_projection::{
    ConversationSegmentKind, ConversationSegmentProjection, SegmentRole, ToolVisualKind,
};

pub const ACP_SURFACE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRedaction {
    /// Internal diagnostic/export mode. Applies the provided redactor but does
    /// not suppress fields by category.
    InternalFull,
    /// Local UI mode. Applies redaction and keeps local affordances such as
    /// thinking text and tool details.
    LocalUi,
    /// External client mode. Applies redaction and suppresses fields that should
    /// not cross a host/protocol boundary by default.
    ExternalClient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpConversationIdentity {
    pub segment_id: String,
    pub turn_id: Option<String>,
    pub sequence: u64,
    pub revision: u64,
}

impl AcpConversationIdentity {
    pub fn new(segment_id: impl Into<String>, sequence: u64) -> Self {
        Self {
            segment_id: segment_id.into(),
            turn_id: None,
            sequence,
            revision: 0,
        }
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpConversationSegment {
    pub schema_version: u32,
    pub identity: AcpConversationIdentity,
    pub role: String,
    pub sigil: String,
    pub emphasis: String,
    pub tool_category: Option<String>,
    pub complete: bool,
    pub kind: AcpConversationSegmentKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpConversationSegmentKind {
    User {
        text: String,
    },
    Assistant {
        text: String,
        thinking: Option<String>,
    },
    Tool {
        id: String,
        name: String,
        args_summary: Option<String>,
        detail_args: Option<String>,
        result_summary: Option<String>,
        detail_result: Option<String>,
        is_error: bool,
        expanded: bool,
    },
    System {
        text: String,
    },
    Lifecycle {
        icon: String,
        text: String,
    },
    Image {
        path: String,
        alt: String,
    },
    Separator,
}

impl AcpConversationSegment {
    pub fn from_projection<TText, TPath, F>(
        identity: AcpConversationIdentity,
        projection: &ConversationSegmentProjection<TText, TPath>,
        policy: SurfaceRedaction,
        redact: F,
    ) -> Self
    where
        TText: AsRef<str>,
        TPath: AsRef<Path>,
        F: Fn(&str) -> String,
    {
        let kind = project_kind(&projection.kind, policy, &redact);
        Self {
            schema_version: ACP_SURFACE_SCHEMA_VERSION,
            identity,
            role: role_name(projection.presentation.role).to_string(),
            sigil: projection.presentation.sigil.to_string(),
            emphasis: emphasis_name(projection.presentation.emphasis).to_string(),
            tool_category: projection
                .presentation
                .tool_visual
                .map(|kind| tool_category_name(kind).to_string()),
            complete: segment_complete(&projection.kind),
            kind,
        }
    }
}

fn project_kind<TText, TPath, F>(
    kind: &ConversationSegmentKind<TText, TPath>,
    policy: SurfaceRedaction,
    redact: &F,
) -> AcpConversationSegmentKind
where
    TText: AsRef<str>,
    TPath: AsRef<Path>,
    F: Fn(&str) -> String,
{
    match kind {
        ConversationSegmentKind::User(user) => AcpConversationSegmentKind::User {
            text: redact(user.text.as_ref()),
        },
        ConversationSegmentKind::Assistant(assistant) => AcpConversationSegmentKind::Assistant {
            text: redact(assistant.text.as_ref()),
            thinking: match policy {
                SurfaceRedaction::ExternalClient => None,
                SurfaceRedaction::InternalFull | SurfaceRedaction::LocalUi => {
                    Some(redact(assistant.thinking.as_ref()))
                }
            },
        },
        ConversationSegmentKind::Tool(tool) => {
            let include_details = !matches!(policy, SurfaceRedaction::ExternalClient);
            AcpConversationSegmentKind::Tool {
                id: redact(tool.id.as_ref()),
                name: redact(tool.name.as_ref()),
                args_summary: tool.args_summary.as_ref().map(|text| redact(text.as_ref())),
                detail_args: include_details
                    .then(|| tool.detail_args.as_ref().map(|text| redact(text.as_ref())))
                    .flatten(),
                result_summary: tool
                    .result_summary
                    .as_ref()
                    .map(|text| redact(text.as_ref())),
                detail_result: include_details
                    .then(|| {
                        tool.detail_result
                            .as_ref()
                            .map(|text| redact(text.as_ref()))
                    })
                    .flatten(),
                is_error: tool.is_error,
                expanded: tool.expanded,
            }
        }
        ConversationSegmentKind::System(system) => AcpConversationSegmentKind::System {
            text: redact(system.text.as_ref()),
        },
        ConversationSegmentKind::Lifecycle(lifecycle) => AcpConversationSegmentKind::Lifecycle {
            icon: redact(lifecycle.icon.as_ref()),
            text: redact(lifecycle.text.as_ref()),
        },
        ConversationSegmentKind::Image(image) => AcpConversationSegmentKind::Image {
            path: match policy {
                SurfaceRedaction::ExternalClient => image
                    .path
                    .as_ref()
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
                SurfaceRedaction::InternalFull | SurfaceRedaction::LocalUi => {
                    image.path.as_ref().display().to_string()
                }
            },
            alt: redact(image.alt.as_ref()),
        },
        ConversationSegmentKind::Separator => AcpConversationSegmentKind::Separator,
    }
}

fn segment_complete<TText, TPath>(kind: &ConversationSegmentKind<TText, TPath>) -> bool {
    match kind {
        ConversationSegmentKind::Assistant(assistant) => assistant.complete,
        ConversationSegmentKind::Tool(tool) => tool.complete,
        _ => true,
    }
}

fn role_name(role: SegmentRole) -> &'static str {
    match role {
        SegmentRole::Operator => "operator",
        SegmentRole::Assistant => "assistant",
        SegmentRole::Tool => "tool",
        SegmentRole::System => "system",
        SegmentRole::Lifecycle => "lifecycle",
        SegmentRole::Media => "media",
        SegmentRole::Separator => "separator",
    }
}

fn emphasis_name(emphasis: crate::tui::conversation_projection::SegmentEmphasis) -> &'static str {
    match emphasis {
        crate::tui::conversation_projection::SegmentEmphasis::Strong => "strong",
        crate::tui::conversation_projection::SegmentEmphasis::Normal => "normal",
        crate::tui::conversation_projection::SegmentEmphasis::Muted => "muted",
    }
}

fn tool_category_name(kind: ToolVisualKind) -> &'static str {
    match kind {
        ToolVisualKind::CommandExec => "command_exec",
        ToolVisualKind::FileRead => "file_read",
        ToolVisualKind::FileMutation => "file_mutation",
        ToolVisualKind::DesignTree => "design_tree",
        ToolVisualKind::Memory => "memory",
        ToolVisualKind::Search => "search",
        ToolVisualKind::Generic => "generic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::conversation_projection::{
        AssistantSegment, ConversationSegmentKind, ImageSegment, ToolSegment,
    };

    fn redact_secret(text: &str) -> String {
        text.replace("SECRET", "[REDACTED]")
    }

    #[test]
    fn external_client_redacts_text_and_suppresses_thinking() {
        let projection = ConversationSegmentProjection::new(
            ConversationSegmentKind::<&str, &Path>::Assistant(AssistantSegment {
                text: "answer SECRET",
                thinking: "chain SECRET",
                complete: false,
            }),
        );
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("assistant-1", 7).with_revision(2),
            &projection,
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        assert_eq!(dto.identity.segment_id, "assistant-1");
        assert_eq!(dto.identity.sequence, 7);
        assert_eq!(dto.identity.revision, 2);
        assert!(!dto.complete);
        match dto.kind {
            AcpConversationSegmentKind::Assistant { text, thinking } => {
                assert_eq!(text, "answer [REDACTED]");
                assert_eq!(thinking, None);
            }
            other => panic!("expected assistant DTO, got {other:?}"),
        }
    }

    #[test]
    fn local_ui_keeps_redacted_thinking_and_tool_details() {
        let projection = ConversationSegmentProjection::new(
            ConversationSegmentKind::<&str, &Path>::Tool(ToolSegment {
                id: "tool-1",
                name: "bash",
                args_summary: Some("echo SECRET"),
                detail_args: Some("TOKEN=SECRET cargo check"),
                result_summary: Some("ok"),
                detail_result: Some("finished SECRET"),
                is_error: false,
                complete: true,
                expanded: true,
            }),
        );
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("tool-1", 3),
            &projection,
            SurfaceRedaction::LocalUi,
            redact_secret,
        );

        assert_eq!(dto.role, "tool");
        assert_eq!(dto.tool_category.as_deref(), Some("command_exec"));
        match dto.kind {
            AcpConversationSegmentKind::Tool {
                detail_args,
                detail_result,
                args_summary,
                ..
            } => {
                assert_eq!(args_summary.as_deref(), Some("echo [REDACTED]"));
                assert_eq!(detail_args.as_deref(), Some("TOKEN=[REDACTED] cargo check"));
                assert_eq!(detail_result.as_deref(), Some("finished [REDACTED]"));
            }
            other => panic!("expected tool DTO, got {other:?}"),
        }
    }

    #[test]
    fn external_client_suppresses_tool_details() {
        let projection = ConversationSegmentProjection::new(
            ConversationSegmentKind::<&str, &Path>::Tool(ToolSegment {
                id: "tool-1",
                name: "bash",
                args_summary: Some("summary SECRET"),
                detail_args: Some("detail SECRET"),
                result_summary: Some("result SECRET"),
                detail_result: Some("detail result SECRET"),
                is_error: true,
                complete: true,
                expanded: false,
            }),
        );
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("tool-1", 4),
            &projection,
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        match dto.kind {
            AcpConversationSegmentKind::Tool {
                args_summary,
                detail_args,
                result_summary,
                detail_result,
                is_error,
                ..
            } => {
                assert_eq!(args_summary.as_deref(), Some("summary [REDACTED]"));
                assert_eq!(result_summary.as_deref(), Some("result [REDACTED]"));
                assert_eq!(detail_args, None);
                assert_eq!(detail_result, None);
                assert!(is_error);
            }
            other => panic!("expected tool DTO, got {other:?}"),
        }
    }

    #[test]
    fn external_client_limits_image_path_to_filename() {
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::Image(ImageSegment {
                path: Path::new("/private/tmp/screenshot.png"),
                alt: "screen SECRET",
            }));
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("image-1", 5),
            &projection,
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        match dto.kind {
            AcpConversationSegmentKind::Image { path, alt } => {
                assert_eq!(path, "screenshot.png");
                assert_eq!(alt, "screen [REDACTED]");
            }
            other => panic!("expected image DTO, got {other:?}"),
        }
    }
}
