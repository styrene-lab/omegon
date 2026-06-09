//! Replay helpers for semantic UI action outcomes.
//!
//! This module is intentionally small: it gives tests and future transports a
//! deterministic way to wrap internal action outcomes without coupling them to
//! a concrete frontend or wire protocol.

use super::actions::UiActionOutcome;
use super::envelope::{
    UI_RUNTIME_ENVELOPE_VERSION, UiActionOutcomeEnvelope, UiActionOutcomeStatus,
};
use super::revision::UiRevision;

/// Convert an internal action outcome into a versioned replay envelope.
pub fn outcome_to_envelope(
    session_id: impl Into<String>,
    action_id: impl Into<String>,
    revision_after: Option<UiRevision>,
    outcome: UiActionOutcome,
) -> UiActionOutcomeEnvelope {
    let session_id = session_id.into();
    let action_id = action_id.into();
    let revision_after = revision_after.map(Into::into);
    match outcome {
        UiActionOutcome::Accepted { message } => UiActionOutcomeEnvelope {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id,
            action_id,
            status: UiActionOutcomeStatus::Accepted,
            revision_after,
            message,
            error: None,
        },
        UiActionOutcome::Rejected { reason } => UiActionOutcomeEnvelope {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id,
            action_id,
            status: UiActionOutcomeStatus::Rejected,
            revision_after: None,
            message: None,
            error: Some(reason),
        },
        UiActionOutcome::Noop { reason } => UiActionOutcomeEnvelope {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id,
            action_id,
            status: UiActionOutcomeStatus::Noop,
            revision_after,
            message: Some(reason),
            error: None,
        },
        UiActionOutcome::Deferred { reason } => UiActionOutcomeEnvelope {
            protocol_version: UI_RUNTIME_ENVELOPE_VERSION,
            session_id,
            action_id,
            status: UiActionOutcomeStatus::Deferred,
            revision_after,
            message: Some(reason),
            error: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_outcome_replay_envelope_preserves_revision_and_message() {
        let envelope = outcome_to_envelope(
            "session-1",
            "action-1",
            Some(UiRevision::new(12)),
            UiActionOutcome::accepted_message("submitted"),
        );

        assert_eq!(envelope.protocol_version, UI_RUNTIME_ENVELOPE_VERSION);
        assert_eq!(envelope.session_id, "session-1");
        assert_eq!(envelope.action_id, "action-1");
        assert_eq!(envelope.status, UiActionOutcomeStatus::Accepted);
        assert_eq!(envelope.revision_after, Some(12));
        assert_eq!(envelope.message.as_deref(), Some("submitted"));
        assert_eq!(envelope.error, None);
    }

    #[test]
    fn rejected_outcome_replay_envelope_uses_error_without_revision() {
        let envelope = outcome_to_envelope(
            "session-1",
            "action-2",
            Some(UiRevision::new(13)),
            UiActionOutcome::rejected("invalid action"),
        );

        assert_eq!(envelope.status, UiActionOutcomeStatus::Rejected);
        assert_eq!(envelope.revision_after, None);
        assert_eq!(envelope.message, None);
        assert_eq!(envelope.error.as_deref(), Some("invalid action"));
    }
}
