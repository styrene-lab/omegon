//! ACP-facing surface DTOs derived from semantic projections.
//!
//! These types are protocol adapter scaffolding: they are concrete, owned,
//! serde-serializable shapes with explicit identity and redaction policy. They
//! intentionally do not replace the existing ACP `SessionUpdate` stream yet.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::surfaces::conversation::{
    ConversationSegmentKind, ConversationSegmentProjection, SegmentCopyPolicy, SegmentRole,
    SegmentSelectionTreatment, SegmentSurfaceTreatment, ToolCategory,
};

pub const ACP_SURFACE_SCHEMA_VERSION: u32 = 1;
pub const ACP_CONVERSATION_SURFACE_METHOD: &str = "_surface/conversation/update";
pub const ACP_CONVERSATION_SURFACE_REDACTION: &str = "external_client";

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
    pub surface: AcpSegmentSurface,
    pub kind: AcpConversationSegmentKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSegmentSurface {
    pub treatment: String,
    pub copy_policy: String,
    pub selection: String,
    pub copyable: bool,
    pub selectable: bool,
    pub detail_available: bool,
    pub expandable: bool,
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
    PeerAgent {
        label: String,
        source: String,
        status: String,
        text: String,
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
    OperatorCopy {
        label: String,
        text: String,
        kind: String,
        copy_status: Option<String>,
    },
    Skill {
        active_ref: String,
        reason: String,
        resolution: String,
        suppressing: Vec<String>,
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
        let model = projection.presentation_model();
        Self {
            schema_version: ACP_SURFACE_SCHEMA_VERSION,
            identity,
            role: role_name(projection.presentation.role).to_string(),
            sigil: projection.presentation.sigil.to_string(),
            emphasis: emphasis_name(projection.presentation.emphasis).to_string(),
            tool_category: projection
                .presentation
                .tool_category
                .map(|kind| tool_category_name(kind).to_string()),
            complete: segment_complete(&projection.kind),
            surface: AcpSegmentSurface::from_model(&model),
            kind,
        }
    }
}

impl AcpSegmentSurface {
    fn from_model(model: &crate::surfaces::conversation::SegmentPresentationModel<'_>) -> Self {
        Self {
            treatment: surface_treatment_name(model.surface.surface).to_string(),
            copy_policy: copy_policy_name(model.surface.copy).to_string(),
            selection: selection_treatment_name(model.surface.selection).to_string(),
            copyable: model.affordances.copyable,
            selectable: model.affordances.selectable,
            detail_available: model.affordances.detail_available,
            expandable: model.affordances.expandable,
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
        ConversationSegmentKind::PeerAgent(peer) => AcpConversationSegmentKind::PeerAgent {
            label: redact(peer.label.as_ref()),
            source: peer.source.as_str().to_string(),
            status: peer.status.as_str().to_string(),
            text: redact(peer.text.as_ref()),
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
        ConversationSegmentKind::OperatorCopy(copy) => AcpConversationSegmentKind::OperatorCopy {
            label: redact(copy.label.as_ref()),
            text: redact(copy.text.as_ref()),
            kind: copy.kind.as_str().to_string(),
            copy_status: None,
        },
        ConversationSegmentKind::System(system) => AcpConversationSegmentKind::System {
            text: redact(system.text.as_ref()),
        },
        ConversationSegmentKind::Skill(skill) => AcpConversationSegmentKind::Skill {
            active_ref: redact(skill.active_ref.as_ref()),
            reason: redact(skill.reason.as_ref()),
            resolution: redact(skill.resolution.as_ref()),
            suppressing: skill
                .suppressing
                .iter()
                .map(|text| redact(text.as_ref()))
                .collect(),
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
        ConversationSegmentKind::PeerAgent(peer) => peer.status.is_terminal(),
        ConversationSegmentKind::Tool(tool) => tool.complete,
        _ => true,
    }
}

fn role_name(role: SegmentRole) -> &'static str {
    match role {
        SegmentRole::Operator => "operator",
        SegmentRole::Assistant => "assistant",
        SegmentRole::PeerAgent => "peer_agent",
        SegmentRole::Tool => "tool",
        SegmentRole::System => "system",
        SegmentRole::Lifecycle => "lifecycle",
        SegmentRole::Media => "media",
        SegmentRole::Separator => "separator",
    }
}

fn emphasis_name(emphasis: crate::surfaces::conversation::SegmentEmphasis) -> &'static str {
    match emphasis {
        crate::surfaces::conversation::SegmentEmphasis::Strong => "strong",
        crate::surfaces::conversation::SegmentEmphasis::Normal => "normal",
        crate::surfaces::conversation::SegmentEmphasis::Muted => "muted",
    }
}

fn tool_category_name(kind: ToolCategory) -> &'static str {
    match kind {
        ToolCategory::CommandExec => "command_exec",
        ToolCategory::FileRead => "file_read",
        ToolCategory::FileMutation => "file_mutation",
        ToolCategory::DesignTree => "design_tree",
        ToolCategory::Memory => "memory",
        ToolCategory::Search => "search",
        ToolCategory::Subagent => "subagent",
        ToolCategory::Network => "network",
        ToolCategory::Generic => "generic",
    }
}

fn surface_treatment_name(treatment: SegmentSurfaceTreatment) -> &'static str {
    match treatment {
        SegmentSurfaceTreatment::Transcript => "transcript",
        SegmentSurfaceTreatment::Card => "card",
        SegmentSurfaceTreatment::Panel => "panel",
        SegmentSurfaceTreatment::ChromeOnly => "chrome_only",
    }
}

fn copy_policy_name(policy: SegmentCopyPolicy) -> &'static str {
    match policy {
        SegmentCopyPolicy::None => "none",
        SegmentCopyPolicy::Body => "body",
        SegmentCopyPolicy::Summary => "summary",
        SegmentCopyPolicy::Detail => "detail",
        SegmentCopyPolicy::Full => "full",
    }
}

fn selection_treatment_name(treatment: SegmentSelectionTreatment) -> &'static str {
    match treatment {
        SegmentSelectionTreatment::None => "none",
        SegmentSelectionTreatment::Subtle => "subtle",
        SegmentSelectionTreatment::Explicit => "explicit",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpConversationEvent {
    TextChunk(String),
    ThinkingChunk(String),
    ToolStart {
        id: String,
        name: String,
        args_summary: Option<String>,
        detail_args: Option<String>,
    },
    ToolOutput {
        id: String,
        text: String,
    },
    ToolEnd {
        id: String,
        success: bool,
        result_summary: Option<String>,
        detail_result: Option<String>,
    },
    StatusUpdate(String),
    TurnCancelled {
        reason: String,
    },
    TurnComplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpSurfaceUpdate {
    pub segment: AcpConversationSegment,
    pub completed_segment_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AcpConversationSurfaceAdapter {
    turn_id: Option<String>,
    next_sequence: u64,
    assistant_id: Option<String>,
    assistant_text: String,
    assistant_thinking: String,
    assistant_revision: u64,
    tools: std::collections::BTreeMap<String, ToolSurfaceState>,
}

#[derive(Debug, Clone)]
struct ToolSurfaceState {
    segment_id: String,
    sequence: u64,
    revision: u64,
    name: String,
    args_summary: Option<String>,
    detail_args: Option<String>,
    output: String,
    success: Option<bool>,
    complete: bool,
}

impl AcpConversationSurfaceAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_turn_id(turn_id: impl Into<String>) -> Self {
        Self {
            turn_id: Some(turn_id.into()),
            ..Self::default()
        }
    }

    pub fn ingest<F>(
        &mut self,
        event: AcpConversationEvent,
        policy: SurfaceRedaction,
        redact: F,
    ) -> Vec<AcpSurfaceUpdate>
    where
        F: Fn(&str) -> String,
    {
        match event {
            AcpConversationEvent::TextChunk(text) => {
                vec![self.ingest_assistant_text(text, policy, &redact)]
            }
            AcpConversationEvent::ThinkingChunk(text) => {
                vec![self.ingest_assistant_thinking(text, policy, &redact)]
            }
            AcpConversationEvent::ToolStart {
                id,
                name,
                args_summary,
                detail_args,
            } => vec![self.ingest_tool_start(id, name, args_summary, detail_args, policy, &redact)],
            AcpConversationEvent::ToolOutput { id, text } => self
                .ingest_tool_output(id, text, policy, &redact)
                .into_iter()
                .collect(),
            AcpConversationEvent::ToolEnd {
                id,
                success,
                result_summary,
                detail_result,
            } => self
                .ingest_tool_end(id, success, result_summary, detail_result, policy, &redact)
                .into_iter()
                .collect(),
            AcpConversationEvent::StatusUpdate(text) => {
                vec![self.one_shot_lifecycle("status", "ℹ", text, policy, &redact)]
            }
            AcpConversationEvent::TurnCancelled { reason } => {
                vec![self.one_shot_lifecycle("cancelled", "⚠", reason, policy, &redact)]
            }
            AcpConversationEvent::TurnComplete => self.complete_turn(policy, &redact),
        }
    }

    fn allocate_identity(&mut self, prefix: &str) -> AcpConversationIdentity {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let mut identity = AcpConversationIdentity::new(format!("{prefix}-{sequence}"), sequence);
        if let Some(turn_id) = &self.turn_id {
            identity = identity.with_turn_id(turn_id.clone());
        }
        identity
    }

    fn identity_for(
        &self,
        segment_id: String,
        sequence: u64,
        revision: u64,
    ) -> AcpConversationIdentity {
        let mut identity =
            AcpConversationIdentity::new(segment_id, sequence).with_revision(revision);
        if let Some(turn_id) = &self.turn_id {
            identity = identity.with_turn_id(turn_id.clone());
        }
        identity
    }

    fn assistant_sequence(&self) -> u64 {
        self.assistant_id
            .as_deref()
            .and_then(|id| id.rsplit_once('-'))
            .and_then(|(_, n)| n.parse().ok())
            .unwrap_or(0)
    }

    fn ensure_assistant(&mut self) {
        if self.assistant_id.is_none() {
            let identity = self.allocate_identity("assistant");
            self.assistant_id = Some(identity.segment_id);
        }
    }

    fn ingest_assistant_text<F>(
        &mut self,
        text: String,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> AcpSurfaceUpdate
    where
        F: Fn(&str) -> String,
    {
        self.ensure_assistant();
        self.assistant_text.push_str(&text);
        self.assistant_revision += 1;
        self.assistant_update(false, policy, redact)
    }

    fn ingest_assistant_thinking<F>(
        &mut self,
        text: String,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> AcpSurfaceUpdate
    where
        F: Fn(&str) -> String,
    {
        self.ensure_assistant();
        self.assistant_thinking.push_str(&text);
        self.assistant_revision += 1;
        self.assistant_update(false, policy, redact)
    }

    fn assistant_update<F>(
        &self,
        complete: bool,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> AcpSurfaceUpdate
    where
        F: Fn(&str) -> String,
    {
        let segment_id = self
            .assistant_id
            .clone()
            .unwrap_or_else(|| "assistant-0".to_string());
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::<&str, &Path>::Assistant(
                crate::surfaces::conversation::AssistantSegment {
                    text: self.assistant_text.as_str(),
                    thinking: self.assistant_thinking.as_str(),
                    complete,
                },
            ));
        let identity = self.identity_for(
            segment_id.clone(),
            self.assistant_sequence(),
            self.assistant_revision,
        );
        AcpSurfaceUpdate {
            segment: AcpConversationSegment::from_projection(identity, &projection, policy, redact),
            completed_segment_id: complete.then_some(segment_id),
        }
    }

    fn ingest_tool_start<F>(
        &mut self,
        id: String,
        name: String,
        args_summary: Option<String>,
        detail_args: Option<String>,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> AcpSurfaceUpdate
    where
        F: Fn(&str) -> String,
    {
        let identity = self.allocate_identity("tool");
        self.tools.insert(
            id.clone(),
            ToolSurfaceState {
                segment_id: identity.segment_id,
                sequence: identity.sequence,
                revision: 0,
                name,
                args_summary,
                detail_args,
                output: String::new(),
                success: None,
                complete: false,
            },
        );
        self.tool_update(&id, policy, redact)
            .expect("tool exists after start")
    }

    fn ingest_tool_output<F>(
        &mut self,
        id: String,
        text: String,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> Option<AcpSurfaceUpdate>
    where
        F: Fn(&str) -> String,
    {
        let state = self.tools.get_mut(&id)?;
        state.output.push_str(&text);
        state.revision += 1;
        self.tool_update(&id, policy, redact)
    }

    fn ingest_tool_end<F>(
        &mut self,
        id: String,
        success: bool,
        result_summary: Option<String>,
        detail_result: Option<String>,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> Option<AcpSurfaceUpdate>
    where
        F: Fn(&str) -> String,
    {
        let state = self.tools.get_mut(&id)?;
        state.success = Some(success);
        state.complete = true;
        state.revision += 1;
        for text in [result_summary, detail_result].into_iter().flatten() {
            if !state.output.is_empty() {
                state.output.push('\n');
            }
            state.output.push_str(&text);
        }
        self.tool_update(&id, policy, redact).map(|mut update| {
            update.completed_segment_id = Some(update.segment.identity.segment_id.clone());
            update
        })
    }

    fn tool_update<F>(
        &self,
        event_tool_id: &str,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> Option<AcpSurfaceUpdate>
    where
        F: Fn(&str) -> String,
    {
        let state = self.tools.get(event_tool_id)?;
        let output = (!state.output.is_empty()).then_some(state.output.as_str());
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::<&str, &Path>::Tool(
                crate::surfaces::conversation::ToolSegment {
                    id: event_tool_id,
                    name: state.name.as_str(),
                    args_summary: state.args_summary.as_deref(),
                    detail_args: state.detail_args.as_deref(),
                    result_summary: output,
                    detail_result: output,
                    is_error: matches!(state.success, Some(false)),
                    complete: state.complete,
                    expanded: false,
                },
            ));
        let identity = self.identity_for(state.segment_id.clone(), state.sequence, state.revision);
        Some(AcpSurfaceUpdate {
            segment: AcpConversationSegment::from_projection(identity, &projection, policy, redact),
            completed_segment_id: None,
        })
    }

    fn one_shot_lifecycle<F>(
        &mut self,
        prefix: &str,
        icon: &'static str,
        text: String,
        policy: SurfaceRedaction,
        redact: &F,
    ) -> AcpSurfaceUpdate
    where
        F: Fn(&str) -> String,
    {
        let identity = self.allocate_identity(prefix);
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::<&str, &Path>::Lifecycle(
                crate::surfaces::conversation::LifecycleSegment {
                    icon,
                    text: text.as_str(),
                },
            ));
        AcpSurfaceUpdate {
            segment: AcpConversationSegment::from_projection(
                identity.clone(),
                &projection,
                policy,
                redact,
            ),
            completed_segment_id: Some(identity.segment_id),
        }
    }

    fn complete_turn<F>(&mut self, policy: SurfaceRedaction, redact: &F) -> Vec<AcpSurfaceUpdate>
    where
        F: Fn(&str) -> String,
    {
        let mut updates = Vec::new();
        if self.assistant_id.is_some() {
            self.assistant_revision += 1;
            updates.push(self.assistant_update(true, policy, redact));
        }
        updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surfaces::conversation::{
        AssistantSegment, ConversationSegmentKind, ImageSegment, PeerAgentSegment, PeerAgentSource,
        PeerAgentStatus, ToolSegment,
    };

    fn redact_secret(text: &str) -> String {
        text.replace("SECRET", "[REDACTED]")
    }

    #[test]
    fn adapter_assigns_stable_assistant_identity_and_revisions() {
        let mut adapter = AcpConversationSurfaceAdapter::with_turn_id("turn-1");
        let first = adapter.ingest(
            AcpConversationEvent::TextChunk("hello ".into()),
            SurfaceRedaction::LocalUi,
            redact_secret,
        );
        let second = adapter.ingest(
            AcpConversationEvent::TextChunk("world".into()),
            SurfaceRedaction::LocalUi,
            redact_secret,
        );
        let complete = adapter.ingest(
            AcpConversationEvent::TurnComplete,
            SurfaceRedaction::LocalUi,
            redact_secret,
        );

        assert_eq!(first[0].segment.identity.segment_id, "assistant-0");
        assert_eq!(first[0].segment.identity.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(first[0].segment.identity.sequence, 0);
        assert_eq!(first[0].segment.identity.revision, 1);
        assert_eq!(second[0].segment.identity.segment_id, "assistant-0");
        assert_eq!(second[0].segment.identity.revision, 2);
        assert_eq!(complete[0].segment.identity.segment_id, "assistant-0");
        assert_eq!(complete[0].segment.identity.revision, 3);
        assert_eq!(
            complete[0].completed_segment_id.as_deref(),
            Some("assistant-0")
        );
        assert!(complete[0].segment.complete);
        match &second[0].segment.kind {
            AcpConversationSegmentKind::Assistant { text, .. } => assert_eq!(text, "hello world"),
            other => panic!("expected assistant DTO, got {other:?}"),
        }
    }

    #[test]
    fn adapter_assigns_tool_identity_and_suppresses_external_details() {
        let mut adapter = AcpConversationSurfaceAdapter::new();
        let start = adapter.ingest(
            AcpConversationEvent::ToolStart {
                id: "runtime-tool-id".into(),
                name: "bash".into(),
                args_summary: Some("echo SECRET".into()),
                detail_args: Some("TOKEN=SECRET cargo check".into()),
            },
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );
        let output = adapter.ingest(
            AcpConversationEvent::ToolOutput {
                id: "runtime-tool-id".into(),
                text: "line SECRET".into(),
            },
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );
        let end = adapter.ingest(
            AcpConversationEvent::ToolEnd {
                id: "runtime-tool-id".into(),
                success: false,
                result_summary: Some("failed SECRET".into()),
                detail_result: Some("stack SECRET".into()),
            },
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        assert_eq!(start[0].segment.identity.segment_id, "tool-0");
        assert_eq!(start[0].segment.identity.revision, 0);
        assert_eq!(output[0].segment.identity.segment_id, "tool-0");
        assert_eq!(output[0].segment.identity.revision, 1);
        assert_eq!(end[0].segment.identity.segment_id, "tool-0");
        assert_eq!(end[0].segment.identity.revision, 2);
        assert_eq!(end[0].completed_segment_id.as_deref(), Some("tool-0"));
        assert_eq!(
            end[0].segment.tool_category.as_deref(),
            Some("command_exec")
        );
        match &end[0].segment.kind {
            AcpConversationSegmentKind::Tool {
                args_summary,
                detail_args,
                result_summary,
                detail_result,
                is_error,
                ..
            } => {
                assert_eq!(args_summary.as_deref(), Some("echo [REDACTED]"));
                assert_eq!(detail_args, &None);
                assert_eq!(
                    result_summary.as_deref(),
                    Some(
                        "line [REDACTED]
failed [REDACTED]
stack [REDACTED]"
                    )
                );
                assert_eq!(detail_result, &None);
                assert!(*is_error);
            }
            other => panic!("expected tool DTO, got {other:?}"),
        }
    }

    #[test]
    fn adapter_ignores_unknown_tool_updates() {
        let mut adapter = AcpConversationSurfaceAdapter::new();
        let updates = adapter.ingest(
            AcpConversationEvent::ToolOutput {
                id: "missing".into(),
                text: "orphan".into(),
            },
            SurfaceRedaction::LocalUi,
            redact_secret,
        );
        assert!(updates.is_empty());
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
    fn peer_agent_projection_serializes_role_payload_and_completion() {
        let projection = ConversationSegmentProjection::new(
            ConversationSegmentKind::<&str, &Path>::PeerAgent(PeerAgentSegment {
                label: "scout SECRET",
                source: PeerAgentSource::Delegate,
                status: PeerAgentStatus::Running,
                text: "working SECRET",
            }),
        );
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("peer-1", 9),
            &projection,
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        assert_eq!(dto.role, "peer_agent");
        assert!(!dto.complete);
        match dto.kind {
            AcpConversationSegmentKind::PeerAgent {
                label,
                source,
                status,
                text,
            } => {
                assert_eq!(label, "scout [REDACTED]");
                assert_eq!(source, "delegate");
                assert_eq!(status, "running");
                assert_eq!(text, "working [REDACTED]");
            }
            other => panic!("expected peer agent DTO, got {other:?}"),
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
    fn acp_assistant_projection_includes_transcript_surface_policy() {
        let projection = ConversationSegmentProjection::new(
            ConversationSegmentKind::<&str, &Path>::Assistant(AssistantSegment {
                text: "hello",
                thinking: "hidden",
                complete: true,
            }),
        );
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("assistant-2", 10),
            &projection,
            SurfaceRedaction::LocalUi,
            redact_secret,
        );

        assert_eq!(dto.surface.treatment, "transcript");
        assert_eq!(dto.surface.copy_policy, "body");
        assert_eq!(dto.surface.selection, "subtle");
        assert!(dto.surface.copyable);
        assert!(dto.surface.selectable);
        assert!(!dto.surface.detail_available);
        assert!(!dto.surface.expandable);
    }

    #[test]
    fn acp_tool_projection_includes_card_detail_surface_policy() {
        let projection = ConversationSegmentProjection::new(
            ConversationSegmentKind::<&str, &Path>::Tool(ToolSegment {
                id: "tool-2",
                name: "bash",
                args_summary: Some("echo hi"),
                detail_args: Some("echo hi"),
                result_summary: Some("ok"),
                detail_result: Some("hi"),
                is_error: false,
                complete: true,
                expanded: false,
            }),
        );
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("tool-2", 11),
            &projection,
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        assert_eq!(dto.surface.treatment, "card");
        assert_eq!(dto.surface.copy_policy, "detail");
        assert_eq!(dto.surface.selection, "explicit");
        assert!(dto.surface.copyable);
        assert!(dto.surface.selectable);
        assert!(dto.surface.detail_available);
        assert!(dto.surface.expandable);
    }

    #[test]
    fn acp_image_projection_includes_panel_noncopyable_surface_policy() {
        let projection =
            ConversationSegmentProjection::new(ConversationSegmentKind::Image(ImageSegment {
                path: Path::new("/private/tmp/screenshot.png"),
                alt: "screen",
            }));
        let dto = AcpConversationSegment::from_projection(
            AcpConversationIdentity::new("image-2", 12),
            &projection,
            SurfaceRedaction::ExternalClient,
            redact_secret,
        );

        assert_eq!(dto.surface.treatment, "panel");
        assert_eq!(dto.surface.copy_policy, "none");
        assert_eq!(dto.surface.selection, "explicit");
        assert!(!dto.surface.copyable);
        assert!(dto.surface.selectable);
        assert!(dto.surface.detail_available);
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
