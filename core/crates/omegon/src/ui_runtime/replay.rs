//! Replay helpers for semantic UI action outcomes.
//!
//! This module is intentionally small: it gives tests and future transports a
//! deterministic way to wrap internal action outcomes without coupling them to
//! a concrete frontend or wire protocol.

use super::actions::UiActionOutcome;
use super::envelope::{
    UI_RUNTIME_ENVELOPE_VERSION, UiActionOutcomeEnvelope, UiActionOutcomeStatus,
};
use super::revision::{UiRevision, UiRevisionCounter};
use crate::surfaces::episodes::OperationEpisodeState;

/// Minimal replay state for proving the atomic activity-to-outcome handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeReplayState {
    pub revision: UiRevision,
    pub episode_id: String,
    pub activity_visible: bool,
    pub outcome_visible: bool,
    pub state: OperationEpisodeState,
}

#[derive(Debug, Clone)]
pub struct EpisodeReplayFixture {
    revisions: UiRevisionCounter,
    states: Vec<EpisodeReplayState>,
}

impl EpisodeReplayFixture {
    pub fn start(episode_id: impl Into<String>) -> Self {
        let mut revisions = UiRevisionCounter::new();
        let state = EpisodeReplayState {
            revision: revisions.next_revision(),
            episode_id: episode_id.into(),
            activity_visible: true,
            outcome_visible: false,
            state: OperationEpisodeState::Running,
        };
        Self {
            revisions,
            states: vec![state],
        }
    }

    pub fn complete(&mut self, state: OperationEpisodeState) -> &EpisodeReplayState {
        assert!(state != OperationEpisodeState::Running);
        let episode_id = self
            .states
            .last()
            .expect("started episode")
            .episode_id
            .clone();
        self.states.push(EpisodeReplayState {
            revision: self.revisions.next_revision(),
            episode_id,
            activity_visible: false,
            outcome_visible: true,
            state,
        });
        self.states.last().expect("completed episode")
    }

    pub fn states(&self) -> &[EpisodeReplayState] {
        &self.states
    }
}

/// Pure Rust replay fixture builder for semantic UI action outcomes.
#[derive(Debug, Clone)]
pub struct ReplayFixture {
    session_id: String,
    revisions: UiRevisionCounter,
    records: Vec<UiActionOutcomeEnvelope>,
}

impl ReplayFixture {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            revisions: UiRevisionCounter::new(),
            records: Vec::new(),
        }
    }

    pub fn current_revision(&self) -> UiRevision {
        self.revisions.current()
    }

    pub fn records(&self) -> &[UiActionOutcomeEnvelope] {
        &self.records
    }

    pub fn record_outcome(
        &mut self,
        action_id: impl Into<String>,
        outcome: UiActionOutcome,
    ) -> UiActionOutcomeEnvelope {
        let revision_after = if matches!(outcome, UiActionOutcome::Accepted { .. }) {
            Some(self.revisions.next_revision())
        } else {
            None
        };
        let envelope =
            outcome_to_envelope(self.session_id.clone(), action_id, revision_after, outcome);
        self.records.push(envelope.clone());
        envelope
    }
}

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
    fn episode_completion_atomically_replaces_activity_with_outcome() {
        let mut fixture = EpisodeReplayFixture::start("turn:7");
        let complete = fixture.complete(OperationEpisodeState::Complete);

        assert_eq!(complete.episode_id, "turn:7");
        assert!(!complete.activity_visible);
        assert!(complete.outcome_visible);
        assert_eq!(complete.revision.get(), 2);
        assert!(
            fixture
                .states()
                .iter()
                .all(|state| { state.activity_visible || state.outcome_visible })
        );
        assert!(
            fixture
                .states()
                .iter()
                .all(|state| { !(state.activity_visible && state.outcome_visible) })
        );
    }

    #[test]
    fn failed_episode_handoff_preserves_identity_and_durable_failure() {
        let mut fixture = EpisodeReplayFixture::start("operator-shell:shell-1");
        let failed = fixture.complete(OperationEpisodeState::Failed);

        assert_eq!(failed.episode_id, "operator-shell:shell-1");
        assert_eq!(failed.state, OperationEpisodeState::Failed);
        assert!(!failed.activity_visible);
        assert!(failed.outcome_visible);
    }

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

    #[test]
    fn replay_fixture_records_accepted_outcome_with_revision() {
        let mut fixture = ReplayFixture::new("session-1");

        let record =
            fixture.record_outcome("action-1", UiActionOutcome::accepted_message("submitted"));

        assert_eq!(record.session_id, "session-1");
        assert_eq!(record.action_id, "action-1");
        assert_eq!(record.status, UiActionOutcomeStatus::Accepted);
        assert_eq!(record.revision_after, Some(1));
        assert_eq!(record.message.as_deref(), Some("submitted"));
        assert_eq!(fixture.current_revision().get(), 1);
        assert_eq!(fixture.records().len(), 1);
    }

    #[test]
    fn replay_fixture_rejected_outcome_does_not_advance_revision() {
        let mut fixture = ReplayFixture::new("session-1");

        let rejected = fixture.record_outcome("action-1", UiActionOutcome::rejected("bad"));
        let accepted = fixture.record_outcome("action-2", UiActionOutcome::accepted());

        assert_eq!(rejected.status, UiActionOutcomeStatus::Rejected);
        assert_eq!(rejected.revision_after, None);
        assert_eq!(accepted.status, UiActionOutcomeStatus::Accepted);
        assert_eq!(accepted.revision_after, Some(1));
        assert_eq!(fixture.current_revision().get(), 1);
        assert_eq!(fixture.records().len(), 2);
    }
}
