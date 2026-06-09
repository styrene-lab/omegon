//! Inbound semantic UI actions shared by frontend adapters.

use std::path::PathBuf;

use omegon_traits::{OperatorWaitResponse, PermissionResponse};

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
