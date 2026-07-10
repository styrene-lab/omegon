//! Inbound semantic UI actions shared by frontend adapters.

use std::path::PathBuf;

use omegon_traits::{OperatorWaitResponse, PermissionResponse};

use crate::surfaces::layout::{UiPresentationLevel, UiSurfaces};
use crate::tui::{PromptMetadata, PromptQueueMode};

/// Semantic operator action emitted by a frontend adapter.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    /// Submit a prompt through the operator loop.
    SubmitPrompt(SubmitPromptAction),
    /// Continue when the last assistant response is awaiting confirmation.
    SubmitContinuation,
    /// Cancel the active turn, if any.
    CancelActiveTurn,
    /// Respond to a pending tool permission prompt.
    RespondToPermission(PermissionAction),
    /// Respond to a pending manual-action wait prompt.
    RespondToOperatorWait(OperatorWaitAction),
    /// Execute a raw slash command string.
    RunSlashCommand(SlashCommandAction),
    /// Apply a coarse UI surface preset.
    SetUiPreset(SetUiPresetAction),
    /// Set one high-level surface visible or hidden.
    SetSurfaceVisible(SetSurfaceVisibleAction),
    /// Select a conversation segment by stable frontend-visible index.
    SelectConversationSegment(SelectConversationSegmentAction),
    /// Open/toggle a conversation segment detail affordance.
    OpenConversationSegmentDetail(OpenConversationSegmentDetailAction),
    /// Replace the current composer draft with frontend-provided text.
    ReplaceComposerDraft(ReplaceComposerDraftAction),
    /// Clear the current composer draft without submitting it.
    ClearComposerDraft,
    /// Attach a path to the composer draft at the current insertion point.
    AttachComposerPath(AttachComposerPathAction),
    /// Move the composer cursor semantically without exposing frontend key events.
    MoveComposerCursor(MoveComposerCursorAction),
    /// Apply a semantic composer editing operation.
    EditComposer(EditComposerAction),
    /// Insert frontend-provided text at the current composer insertion point.
    InsertComposerText(InsertComposerTextAction),
    /// Copy a conversation segment using its semantic copy policy.
    CopyConversationSegment(CopyConversationSegmentAction),
    /// Copy the latest assistant response using semantic body-copy policy.
    CopyLatestAssistantResponse(CopyLatestAssistantResponseAction),
}

/// Prompt submission intent independent of a concrete editor widget.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmitPromptAction {
    pub text: String,
    pub attachments: Vec<PathBuf>,
    pub source: PromptSource,
    pub queue_mode: PromptQueueMode,
    pub metadata: PromptMetadata,
}

/// Source frontend/channel for a prompt-like action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptSource {
    LocalTui,
    Voice,
    Acp,
    ExternalClient { client_id: String },
}

impl PromptSource {
    pub fn submitted_by(&self) -> String {
        match self {
            Self::LocalTui => "local-tui".to_string(),
            Self::Voice => "voice".to_string(),
            Self::Acp => "acp".to_string(),
            Self::ExternalClient { client_id } => client_id.clone(),
        }
    }

    pub const fn via(&self) -> &'static str {
        match self {
            Self::LocalTui => "tui",
            Self::Voice => "voice",
            Self::Acp => "acp",
            Self::ExternalClient { .. } => "external",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionAction {
    pub request_id: Option<String>,
    pub response: PermissionResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorWaitAction {
    pub request_id: Option<String>,
    pub response: OperatorWaitResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandAction {
    pub raw: String,
    pub source: PromptSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetUiPresetAction {
    pub level: UiPresentationLevel,
}

impl SetUiPresetAction {
    pub const fn new(level: UiPresentationLevel) -> Self {
        Self { level }
    }

    /// Compatibility constructor for surface-only frontend adapters. Om and
    /// Active intentionally share their initial surface set, so callers that
    /// need Active semantics must use `new(UiPresentationLevel::Active)`.
    pub const fn from_legacy_surfaces(surfaces: UiSurfaces) -> Self {
        let level = if surfaces.dashboard && surfaces.instruments && surfaces.footer {
            UiPresentationLevel::Full
        } else {
            UiPresentationLevel::Om
        };
        Self { level }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetSurfaceVisibleAction {
    pub surface: UiSurfaceToggle,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversationSegmentRef {
    pub index: usize,
}

impl ConversationSegmentRef {
    pub const fn by_index(index: usize) -> Self {
        Self { index }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectConversationSegmentAction {
    pub segment: ConversationSegmentRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenConversationSegmentDetailAction {
    pub segment: ConversationSegmentRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceComposerDraftAction {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachComposerPathAction {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveComposerCursorAction {
    pub direction: ComposerCursorDirection,
    pub unit: ComposerCursorUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerCursorDirection {
    Backward,
    Forward,
    Home,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerCursorUnit {
    Character,
    Word,
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditComposerAction {
    pub operation: ComposerEditOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerEditOperation {
    DeleteBackward,
    DeleteWordBackward,
    DeleteWordForward,
    ClearLine,
    KillToEnd,
    InsertNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertComposerTextAction {
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyConversationSegmentAction {
    pub segment: ConversationSegmentRef,
    pub mode: SegmentCopyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyLatestAssistantResponseAction {
    pub mode: SegmentCopyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentCopyMode {
    Raw,
    Plaintext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSurfaceToggle {
    Dashboard,
    Instruments,
    Footer,
    Activity,
}

impl UiSurfaceToggle {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "dashboard" | "dash" => Ok(Self::Dashboard),
            "instruments" | "instrument" | "tools" => Ok(Self::Instruments),
            "footer" => Ok(Self::Footer),
            "activity" | "activities" | "live" | "log" => Ok(Self::Activity),
            other => Err(format!("Unknown UI surface: {other}")),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Instruments => "instruments",
            Self::Footer => "footer",
            Self::Activity => "activity",
        }
    }
}

/// Result of handling a semantic UI action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiActionOutcome {
    Accepted { message: Option<String> },
    Rejected { reason: String },
    Noop { reason: String },
    Deferred { reason: String },
}

impl UiActionOutcome {
    pub fn accepted() -> Self {
        Self::Accepted { message: None }
    }

    pub fn accepted_message(message: impl Into<String>) -> Self {
        Self::Accepted {
            message: Some(message.into()),
        }
    }

    pub fn rejected(reason: impl Into<String>) -> Self {
        Self::Rejected {
            reason: reason.into(),
        }
    }

    pub fn noop(reason: impl Into<String>) -> Self {
        Self::Noop {
            reason: reason.into(),
        }
    }

    pub fn deferred(reason: impl Into<String>) -> Self {
        Self::Deferred {
            reason: reason.into(),
        }
    }
}
