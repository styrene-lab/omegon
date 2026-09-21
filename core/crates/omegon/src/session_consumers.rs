//! Validated semantic session adapters for interactive consumers.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use uuid::Uuid;

use crate::{
    session_projection_model::SessionProjectionModel,
    session_projection_reader::{ProjectionReadV1, SessionProjectionReader, ValidatedProjectionV1},
    session_replay::{ReplayEnd, SessionReplay},
    session_shadow_projection::ShadowProjector,
    surfaces::session::{
        ActiveTurnStatusV1, FrontendSnapshotV1, ProjectionChunkItemsV1, ProjectionLineageV1,
        ProjectionPayloadV1, TranscriptContentV1, TranscriptMessageV1,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionViewKind {
    Resume,
    New,
    ContextClear,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionViewTarget {
    /// Process-local identity of this publication, independent of caller generations.
    pub(crate) binding_id: Uuid,
    pub(crate) snapshot: PathBuf,
    pub(crate) session_id: String,
    pub(crate) stream_id: Option<Uuid>,
    pub(crate) generation: u64,
    pub(crate) kind: SessionViewKind,
}

#[derive(Debug, Clone)]
struct PublishedSessionView {
    target: SessionViewTarget,
    runtime_queue: serde_json::Value,
    activity: crate::surfaces::session_activity::SessionActivityCache,
}

#[derive(Clone, Debug)]
pub(crate) struct SessionViewBinding {
    published: Arc<RwLock<PublishedSessionView>>,
    generation_tx: tokio::sync::watch::Sender<u64>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DeferredSessionViewBinding {
    binding: Arc<RwLock<Option<SessionViewBinding>>>,
}

impl DeferredSessionViewBinding {
    pub(crate) fn bind(&self, binding: SessionViewBinding) {
        binding
            .published
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .target
            .binding_id = Uuid::new_v4();
        *self
            .binding
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(binding);
    }

    pub(crate) fn snapshot(&self) -> Option<SessionViewTarget> {
        self.binding
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(SessionViewBinding::snapshot)
    }
}

impl SessionViewBinding {
    pub(crate) fn new(snapshot: PathBuf, session_id: String) -> Self {
        let target = SessionViewTarget {
            binding_id: Uuid::new_v4(),
            snapshot,
            session_id,
            stream_id: None,
            generation: 1,
            kind: SessionViewKind::New,
        };
        let (generation_tx, _) = tokio::sync::watch::channel(target.generation);
        Self {
            published: Arc::new(RwLock::new(PublishedSessionView {
                target,
                runtime_queue: empty_runtime_queue(),
                activity: Default::default(),
            })),
            generation_tx,
        }
    }

    pub(crate) fn snapshot(&self) -> SessionViewTarget {
        self.published
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .target
            .clone()
    }

    pub(crate) fn replace(&self, mut target: SessionViewTarget) {
        target.binding_id = Uuid::new_v4();
        let generation = target.generation;
        *self
            .published
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = PublishedSessionView {
            target,
            runtime_queue: empty_runtime_queue(),
            activity: Default::default(),
        };
        self.generation_tx.send_replace(generation);
    }

    pub(crate) fn subscribe_generation(&self) -> tokio::sync::watch::Receiver<u64> {
        self.generation_tx.subscribe()
    }

    pub(crate) fn update_runtime_queue(&self, snapshot: serde_json::Value) {
        let mut published = self
            .published
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let incoming = snapshot
            .get("activity")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok());
        let accept = match incoming {
            Some(activity) => matches!(
                published.activity.reconcile(activity),
                Ok(
                    crate::surfaces::session_activity::ReconcileDisposition::Applied
                        | crate::surfaces::session_activity::ReconcileDisposition::Idempotent
                )
            ),
            None if published.activity.current().is_some() => {
                let _ = published.activity.reconcile_unversioned_active();
                false
            }
            None => true,
        };
        if accept {
            published.runtime_queue = snapshot;
        }
    }

    pub(crate) fn runtime_queue_snapshot(&self) -> serde_json::Value {
        self.published
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .runtime_queue
            .clone()
    }

    pub(crate) fn activity_snapshot(
        &self,
    ) -> Option<crate::surfaces::session_activity::SessionActivityProjectionV1> {
        self.published
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .activity
            .current()
            .cloned()
    }
}

/// Prepare the first client view before a frontend can observe the binding.
pub(crate) fn prepare_initial_view(target: &SessionViewTarget) -> Result<(), SessionConsumerError> {
    use crate::session_shadow_projection::{
        ALL_SHADOW_PROJECTORS, ProjectorPublicationStatus, SessionProjectionCoordinator,
    };
    let replay =
        SessionReplay::replay_session(&target.snapshot, &target.session_id, ReplayEnd::EndOfStream)
            .map_err(|error| SessionConsumerError::Unavailable(error.to_string()))?;
    let coordinator =
        SessionProjectionCoordinator::open(&target.snapshot.with_extension("projections"))
            .map_err(|error| SessionConsumerError::Unavailable(error.to_string()))?;
    for report in coordinator.publish(&replay, &ALL_SHADOW_PROJECTORS) {
        if let ProjectorPublicationStatus::Failed { error, .. } = report.status {
            return Err(SessionConsumerError::Unavailable(error.to_string()));
        }
    }
    SemanticSessionView::load(target).map(|_| ())
}

fn empty_runtime_queue() -> serde_json::Value {
    serde_json::Value::Null
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticSessionStatus {
    ExactFull,
    ExactSuffix,
    LegacyUnavailable,
}

impl SemanticSessionStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ExactFull => "exact full session",
            Self::ExactSuffix => "exact semantic suffix; pre-boundary session content is not exact",
            Self::LegacyUnavailable => "semantic transcript unavailable for legacy session",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SemanticSessionView {
    pub(crate) status: SemanticSessionStatus,
    pub(crate) stream_id: Uuid,
    pub(crate) frontier_sequence: u64,
    pub(crate) frontend: Option<FrontendSnapshotV1>,
    pub(crate) transcript: Vec<TranscriptMessageV1>,
    replay: SessionReplay,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionConsumerError {
    #[error("semantic session projection unavailable: {0}")]
    Unavailable(String),
    #[error("semantic session content is invalid: {0}")]
    Content(String),
}

impl SemanticSessionView {
    pub(crate) fn load(target: &SessionViewTarget) -> Result<Self, SessionConsumerError> {
        let replay = match target.stream_id {
            Some(stream_id) => SessionReplay::replay_prefix(
                &target.snapshot,
                &target.session_id,
                stream_id,
                ReplayEnd::EndOfStream,
            ),
            None => SessionReplay::replay_session(
                &target.snapshot,
                &target.session_id,
                ReplayEnd::EndOfStream,
            ),
        }
        .map_err(|error| SessionConsumerError::Unavailable(error.to_string()))?;
        let stream_id = replay.frontier().stream_id();
        let reader =
            SessionProjectionReader::adjacent_to(&target.snapshot, &target.session_id, stream_id)
                .map_err(|error| SessionConsumerError::Unavailable(error.to_string()))?;

        let frontend_read = reader.read(ShadowProjector::FrontendSnapshot, ReplayEnd::EndOfStream);
        let transcript_read = reader.read(ShadowProjector::Transcript, ReplayEnd::EndOfStream);
        let stale = matches!(frontend_read, ProjectionReadV1::Stale { .. })
            || matches!(transcript_read, ProjectionReadV1::Stale { .. });

        let (status, frontend, transcript) = if stale {
            let model = SessionProjectionModel::from_replay(&replay)
                .map_err(|error| SessionConsumerError::Unavailable(error.to_string()))?;
            let status = status_from_lineage(model.lineage());
            let frontend = (status != SemanticSessionStatus::LegacyUnavailable)
                .then(|| model.frontend_snapshot().clone());
            (status, frontend, model.transcript().to_vec())
        } else {
            let (status, frontend) = frontend_from_read(frontend_read)?;
            let (transcript_status, transcript) = transcript_from_read(transcript_read)?;
            if status != transcript_status {
                return Err(SessionConsumerError::Unavailable(
                    "frontend and transcript lineage disagree".into(),
                ));
            }
            (status, frontend, transcript)
        };

        Ok(Self {
            status,
            stream_id,
            frontier_sequence: replay.frontier().sequence(),
            frontend,
            transcript,
            replay,
        })
    }

    pub(crate) fn content_text(
        &self,
        content_ref: &crate::session_blob_store::ContentRef,
    ) -> Result<String, SessionConsumerError> {
        let bytes = self
            .replay
            .read_default_content(content_ref)
            .map_err(|error| SessionConsumerError::Content(error.to_string()))?;
        String::from_utf8(bytes).map_err(|error| SessionConsumerError::Content(error.to_string()))
    }

    pub(crate) fn transcript_markdown(
        &self,
        require_full: bool,
    ) -> Result<String, SessionConsumerError> {
        if self.status == SemanticSessionStatus::LegacyUnavailable {
            return Err(SessionConsumerError::Unavailable(
                self.status.label().into(),
            ));
        }
        if require_full && self.status == SemanticSessionStatus::ExactSuffix {
            return Err(SessionConsumerError::Unavailable(
                "full-session exact transcript unavailable for mixed lineage; request the exact semantic suffix explicitly".into(),
            ));
        }
        let mut output = format!(
            "# Omegon semantic transcript\n\nExactness: {}\nStream: {}\nFrontier: {}\n",
            self.status.label(),
            self.stream_id,
            self.frontier_sequence
        );
        for message in &self.transcript {
            match &message.content {
                TranscriptContentV1::Prompt { prompt_content } => {
                    output.push_str("\n## User\n\n");
                    output.push_str(&prompt_content.text);
                    if !prompt_content.attachments.is_empty() {
                        output.push_str(&format!(
                            "\n\n[{} committed attachment(s)]",
                            prompt_content.attachments.len()
                        ));
                    }
                }
                TranscriptContentV1::Assistant { assistant_channels } => {
                    output.push_str("\n\n## Assistant\n\n");
                    for channel in assistant_channels {
                        for content_ref in &channel.chunk_refs {
                            output.push_str(&self.content_text(content_ref)?);
                        }
                    }
                }
                TranscriptContentV1::ToolResult {
                    content_ref,
                    disposition,
                    is_error,
                    ..
                } => {
                    output.push_str(&format!(
                        "\n\n## Tool result ({disposition:?}, error={is_error})\n\n"
                    ));
                    output.push_str(&self.content_text(content_ref)?);
                }
            }
        }
        output.push('\n');
        Ok(output)
    }
}

fn status_from_lineage(lineage: ProjectionLineageV1) -> SemanticSessionStatus {
    match lineage {
        ProjectionLineageV1::Full => SemanticSessionStatus::ExactFull,
        ProjectionLineageV1::Mixed => SemanticSessionStatus::ExactSuffix,
        ProjectionLineageV1::Legacy => SemanticSessionStatus::LegacyUnavailable,
    }
}

fn frontend_from_read(
    read: ProjectionReadV1,
) -> Result<(SemanticSessionStatus, Option<FrontendSnapshotV1>), SessionConsumerError> {
    match read {
        ProjectionReadV1::ExactFull(value) => frontend(value, SemanticSessionStatus::ExactFull),
        ProjectionReadV1::ExactSuffix(value) => frontend(value, SemanticSessionStatus::ExactSuffix),
        ProjectionReadV1::LegacyUnavailable(_) => {
            Ok((SemanticSessionStatus::LegacyUnavailable, None))
        }
        ProjectionReadV1::Stale { .. } => unreachable!("stale reads derive synchronously"),
        ProjectionReadV1::SessionlessUnavailable => Err(SessionConsumerError::Unavailable(
            "sessionless execution has no semantic frontend".into(),
        )),
        ProjectionReadV1::Corrupt { reason } => Err(SessionConsumerError::Unavailable(reason)),
    }
}

fn frontend(
    value: ValidatedProjectionV1,
    status: SemanticSessionStatus,
) -> Result<(SemanticSessionStatus, Option<FrontendSnapshotV1>), SessionConsumerError> {
    let ProjectionPayloadV1::FrontendSnapshot { snapshot } = value.envelope.payload else {
        return Err(SessionConsumerError::Unavailable(
            "validated frontend payload is absent".into(),
        ));
    };
    Ok((status, Some(snapshot)))
}

fn transcript_from_read(
    read: ProjectionReadV1,
) -> Result<(SemanticSessionStatus, Vec<TranscriptMessageV1>), SessionConsumerError> {
    let (status, value) = match read {
        ProjectionReadV1::ExactFull(value) => (SemanticSessionStatus::ExactFull, value),
        ProjectionReadV1::ExactSuffix(value) => (SemanticSessionStatus::ExactSuffix, value),
        ProjectionReadV1::LegacyUnavailable(_) => {
            return Ok((SemanticSessionStatus::LegacyUnavailable, Vec::new()));
        }
        ProjectionReadV1::Stale { .. } => unreachable!("stale reads derive synchronously"),
        ProjectionReadV1::SessionlessUnavailable => {
            return Err(SessionConsumerError::Unavailable(
                "sessionless execution has no semantic transcript".into(),
            ));
        }
        ProjectionReadV1::Corrupt { reason } => {
            return Err(SessionConsumerError::Unavailable(reason));
        }
    };
    let mut messages = Vec::new();
    for chunk in value.chunks {
        let ProjectionChunkItemsV1::TranscriptMessages(mut chunk_messages) = chunk.items else {
            return Err(SessionConsumerError::Unavailable(
                "validated transcript chunk has the wrong item type".into(),
            ));
        };
        messages.append(&mut chunk_messages);
    }
    Ok((status, messages))
}

pub(crate) fn active_turn_label(snapshot: &FrontendSnapshotV1) -> &'static str {
    match snapshot.active_turn.as_ref().map(|turn| turn.status) {
        Some(ActiveTurnStatusV1::Active) => "active",
        Some(ActiveTurnStatusV1::Interrupted) => "interrupted",
        None => "idle",
    }
}

pub(crate) fn snapshot_path(cwd: &Path, session_id: &str) -> Option<PathBuf> {
    crate::session::sessions_dir(cwd).map(|directory| directory.join(format!("{session_id}.json")))
}

#[cfg(test)]
pub(crate) fn publish_test_projection(snapshot: &Path, fixture: &str, session_id: &str) -> Uuid {
    use crate::session_shadow_projection::{ALL_SHADOW_PROJECTORS, SessionProjectionCoordinator};

    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/session-semantic-v1")
        .join(fixture);
    let authority = std::fs::read_to_string(fixture_path)
        .unwrap()
        .replace("fixture-session", session_id);
    let stem = snapshot.file_stem().unwrap().to_string_lossy();
    let directory = snapshot.parent().unwrap();
    std::fs::write(directory.join(format!("{stem}.authority.jsonl")), authority).unwrap();
    let replay =
        SessionReplay::replay_session(snapshot, session_id, ReplayEnd::EndOfStream).unwrap();
    SessionProjectionCoordinator::open(&directory.join(format!("{stem}.projections")))
        .unwrap()
        .publish(&replay, &ALL_SHADOW_PROJECTORS);
    replay.frontier().stream_id()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::session_shadow_projection::{ALL_SHADOW_PROJECTORS, SessionProjectionCoordinator};

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/session-semantic-v1"
    );
    const SESSION_ID: &str = "fixture-session";

    fn target(fixture: &str, publish_end: ReplayEnd) -> (tempfile::TempDir, SessionViewTarget) {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("session.json");
        fs::write(
            directory.path().join("session.authority.jsonl"),
            fs::read(Path::new(FIXTURES).join(fixture)).unwrap(),
        )
        .unwrap();
        let replay = SessionReplay::replay_session(&snapshot, SESSION_ID, publish_end).unwrap();
        SessionProjectionCoordinator::open(&directory.path().join("session.projections"))
            .unwrap()
            .publish(&replay, &ALL_SHADOW_PROJECTORS);
        let target = SessionViewTarget {
            snapshot,
            binding_id: Uuid::new_v4(),
            session_id: SESSION_ID.into(),
            stream_id: Some(replay.frontier().stream_id()),
            generation: 7,
            kind: SessionViewKind::Resume,
        };
        (directory, target)
    }

    #[test]
    fn initial_view_is_available_before_background_projection_starts() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("fresh.json");
        let _authority = crate::session_authority::SessionAuthority::open(
            &snapshot,
            "fresh",
            "workspace",
            "generation",
            crate::session_authority::ActorIdentity {
                principal: "operator".into(),
                ingress: "interactive".into(),
            },
            "2026-09-05T00:00:00Z",
        )
        .unwrap();
        let binding = SessionViewBinding::new(snapshot, "fresh".into());
        prepare_initial_view(&binding.snapshot()).unwrap();
        let view = SemanticSessionView::load(&binding.snapshot()).unwrap();
        // A session with no semantic step yet retains its truthful lineage;
        // initialization must not fabricate a full-spine boundary.
        assert_eq!(view.status, SemanticSessionStatus::LegacyUnavailable);
        assert!(view.frontier_sequence > 0);
        assert!(view.transcript.is_empty());
    }

    #[test]
    fn capture_binding_identity_fences_same_generation_rebinds_and_paths() {
        let binding = DeferredSessionViewBinding::default();
        let first =
            SessionViewBinding::new(PathBuf::from("first/session.json"), "same-session".into());
        binding.bind(first.clone());
        let original = binding.snapshot().unwrap();
        assert!(crate::session_advisory::generation_is_current(
            &binding, &original
        ));
        let mut wrong_path = original.clone();
        wrong_path.snapshot = PathBuf::from("second/session.json");
        assert!(!crate::session_advisory::generation_is_current(
            &binding,
            &wrong_path
        ));
        binding.bind(SessionViewBinding::new(
            original.snapshot.clone(),
            original.session_id.clone(),
        ));
        assert_eq!(binding.snapshot().unwrap().generation, original.generation);
        assert!(!crate::session_advisory::generation_is_current(
            &binding, &original
        ));
        binding.bind(first.clone());
        let rebound = binding.snapshot().unwrap();
        assert!(!crate::session_advisory::generation_is_current(
            &binding, &original
        ));
        first.replace(rebound.clone());
        assert!(!crate::session_advisory::generation_is_current(
            &binding, &rebound
        ));
        assert!(crate::session_advisory::generation_is_current(
            &binding,
            &binding.snapshot().unwrap()
        ));
    }

    #[test]
    fn full_mixed_and_legacy_statuses_do_not_overclaim_exactness() {
        let (_full_dir, full) = target(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::EndOfStream,
        );
        let full = SemanticSessionView::load(&full).unwrap();
        assert_eq!(full.status, SemanticSessionStatus::ExactFull);
        assert!(full.frontend.is_some());

        let (_mixed_dir, mixed) =
            target("mixed-legacy-full.authority.jsonl", ReplayEnd::EndOfStream);
        let mixed = SemanticSessionView::load(&mixed).unwrap();
        assert_eq!(mixed.status, SemanticSessionStatus::ExactSuffix);
        assert!(mixed.transcript_markdown(true).is_err());

        let (_legacy_dir, legacy) =
            target("slice-1-closed.authority.jsonl", ReplayEnd::EndOfStream);
        let legacy = SemanticSessionView::load(&legacy).unwrap();
        assert_eq!(legacy.status, SemanticSessionStatus::LegacyUnavailable);
        assert!(legacy.frontend.is_none());
        assert!(legacy.transcript_markdown(false).is_err());
    }

    #[test]
    fn stale_projection_is_synchronously_derived_at_exact_frontier() {
        let (_directory, target) = target(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::Sequence(3),
        );
        let view = SemanticSessionView::load(&target).unwrap();
        assert_eq!(view.frontier_sequence, 4);
        assert_eq!(view.status, SemanticSessionStatus::ExactFull);
    }

    #[test]
    fn binding_generation_fences_a_late_projection_load() {
        let (_directory, first) = target(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::EndOfStream,
        );
        let binding = SessionViewBinding::new(first.snapshot.clone(), first.session_id.clone());
        binding.replace(first.clone());
        let captured = binding.snapshot();
        let mut second = first;
        second.generation += 1;
        second.kind = SessionViewKind::ContextClear;
        binding.replace(second);
        assert_ne!(binding.snapshot().generation, captured.generation);
    }

    #[test]
    fn replacement_atomically_rebinds_identity_and_resets_authoritative_queue() {
        let (_directory, first) = target(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::EndOfStream,
        );
        let binding = SessionViewBinding::new(first.snapshot.clone(), first.session_id.clone());
        binding.update_runtime_queue(serde_json::json!({"depth": 1, "active": {"turn_id": 9}}));
        let generation_rx = binding.subscribe_generation();
        let mut second = first;
        second.session_id = "replacement-session".into();
        second.generation += 1;

        binding.replace(second.clone());

        assert!(generation_rx.has_changed().unwrap());
        assert_eq!(binding.snapshot().session_id, second.session_id);
        assert!(binding.runtime_queue_snapshot()["depth"].is_null());
        assert!(binding.runtime_queue_snapshot()["active"].is_null());
    }

    #[test]
    fn versioned_runtime_queue_rejects_stale_active_over_durable_idle() {
        fn queue(revision: u64, active: bool) -> serde_json::Value {
            serde_json::json!({
                "depth": usize::from(active),
                "active": active.then(|| serde_json::json!({"turn_id": 9})),
                "items": [],
                "activity": {
                    "schema_version": 1,
                    "lineage": {
                        "session_id": "session-1",
                        "stream_id": "stream-1",
                        "runtime_generation": "runtime-1",
                        "composition_generation": "composition-1"
                    },
                    "activity_revision": revision,
                    "queue": [],
                    "active_turn": active.then(|| serde_json::json!({
                        "turn_id": "turn-1",
                        "prompt_id": "prompt-1",
                        "phase": "running"
                    })),
                    "terminal_turn": (!active).then(|| serde_json::json!({
                        "turn_id": "turn-1",
                        "outcome": "completed",
                        "reason_code": "done",
                        "authority_sequence": revision
                    })),
                    "lifecycle_health": "healthy",
                    "lifecycle_detail": null,
                    "actions": []
                }
            })
        }

        let binding = SessionViewBinding::new(PathBuf::from("snapshot.json"), "session-1".into());
        binding.update_runtime_queue(queue(8, false));
        binding.update_runtime_queue(queue(7, true));
        binding.update_runtime_queue(serde_json::json!({
            "depth": 1,
            "active": {"turn_id": 9},
            "items": []
        }));

        assert_eq!(binding.runtime_queue_snapshot()["depth"], 0);
        assert!(binding.runtime_queue_snapshot()["active"].is_null());
        assert!(binding.activity_snapshot().unwrap().is_durably_closed());
    }
}
