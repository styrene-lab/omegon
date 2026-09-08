use std::path::Path;

use uuid::Uuid;

use crate::session_authority::{
    AuthorityError, AuthorityLineageLevel, FullSpineBoundary, InvocationState, ModelRequestOutcome,
    ModelRequestState, SessionAuthorityState, SessionAuthorityStore, SessionFact,
    SessionFactPayload, StepTerminalState,
};
use crate::session_blob_store::{ContentRef, ProjectionClass};

type Result<T> = std::result::Result<T, AuthorityError>;

#[cfg(test)]
pub(crate) use tests::open_joined_request as test_open_joined_request;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayEnd {
    EndOfStream,
    Sequence(u64),
    Event(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthorityFrontier {
    session_id: String,
    stream_id: Uuid,
    sequence: u64,
    event_id: Uuid,
}

impl AuthorityFrontier {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn event_id(&self) -> Uuid {
        self.event_id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplayRecord {
    frontier: AuthorityFrontier,
    command_id: Uuid,
    command_fingerprint: String,
    causation_event_id: Option<Uuid>,
    recorded_at: String,
    payload: SessionFactPayload,
}

impl ReplayRecord {
    pub(crate) fn frontier(&self) -> &AuthorityFrontier {
        &self.frontier
    }

    pub(crate) fn event_type(&self) -> &'static str {
        self.payload.event_type()
    }

    pub(crate) fn command_id(&self) -> Uuid {
        self.command_id
    }

    pub(crate) fn command_fingerprint(&self) -> &str {
        &self.command_fingerprint
    }

    pub(crate) fn causation_event_id(&self) -> Option<Uuid> {
        self.causation_event_id
    }

    pub(crate) fn recorded_at(&self) -> &str {
        &self.recorded_at
    }

    pub(crate) fn payload(&self) -> &SessionFactPayload {
        &self.payload
    }
}

impl From<SessionFact> for ReplayRecord {
    fn from(fact: SessionFact) -> Self {
        Self {
            frontier: AuthorityFrontier {
                session_id: fact.session_id,
                stream_id: fact.stream_id,
                sequence: fact.sequence,
                event_id: fact.event_id,
            },
            command_id: fact.command_id,
            command_fingerprint: fact.command_fingerprint,
            causation_event_id: fact.causation_event_id,
            recorded_at: fact.recorded_at,
            payload: fact.payload,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncompleteInvocationKind {
    Registered,
    PreparedUnhandedOff,
    Dispatched,
    Acknowledged,
    UnknownCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IncompleteInvocation {
    pub(crate) invocation_id: Uuid,
    pub(crate) kind: IncompleteInvocationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplaySemanticState {
    pub(crate) active_turn_id: Option<Uuid>,
    pub(crate) active_step_id: Option<Uuid>,
    pub(crate) active_request_id: Option<Uuid>,
    pub(crate) incomplete_invocations: Vec<IncompleteInvocation>,
    pub(crate) abandoned_request_ids: Vec<Uuid>,
    pub(crate) abandoned_step_ids: Vec<Uuid>,
    pub(crate) active_compaction_id: Option<Uuid>,
    pub(crate) context_revision: u64,
    pub(crate) applied_compaction_ids: Vec<Uuid>,
    pub(crate) abandoned_compaction_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestrictedContinuityAuthorization {
    session_id: String,
    stream_id: Uuid,
    target_request_id: Uuid,
    continuity_id: Uuid,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionReplay {
    store: SessionAuthorityStore,
    records: Vec<ReplayRecord>,
    state: SessionAuthorityState,
    frontier: AuthorityFrontier,
}

impl SessionReplay {
    pub(crate) fn replay_session(
        session_snapshot: &Path,
        expected_session_id: &str,
        end: ReplayEnd,
    ) -> Result<Self> {
        let store = SessionAuthorityStore::adjacent_to(session_snapshot)?;
        let facts = store.read_stable_facts()?;
        let first = facts.first().ok_or_else(|| {
            AuthorityError::Invalid("authority replay requires a non-empty stream".into())
        })?;
        if first.session_id != expected_session_id {
            return Err(AuthorityError::Invalid(
                "authority replay session identity does not match".into(),
            ));
        }
        Self::replay_prefix(session_snapshot, expected_session_id, first.stream_id, end)
    }

    pub(crate) fn replay_prefix(
        session_snapshot: &Path,
        expected_session_id: &str,
        expected_stream_id: Uuid,
        end: ReplayEnd,
    ) -> Result<Self> {
        let store = SessionAuthorityStore::adjacent_to(session_snapshot)?;
        let mut facts = store.read_stable_facts()?;
        if facts.is_empty() {
            return Err(AuthorityError::Invalid(
                "authority replay requires a non-empty stream".into(),
            ));
        }
        if facts[0].session_id != expected_session_id || facts[0].stream_id != expected_stream_id {
            return Err(AuthorityError::Invalid(
                "authority replay session or stream identity does not match".into(),
            ));
        }

        let selected = match end {
            ReplayEnd::EndOfStream => facts.len(),
            ReplayEnd::Sequence(sequence) => {
                if sequence == 0 {
                    return Err(AuthorityError::Invalid(
                        "authority replay sequence zero is invalid".into(),
                    ));
                }
                let index = usize::try_from(sequence).map_err(|_| {
                    AuthorityError::Invalid("authority replay sequence is out of range".into())
                })?;
                if index > facts.len() {
                    return Err(AuthorityError::Invalid(
                        "authority replay sequence is beyond end of stream".into(),
                    ));
                }
                index
            }
            ReplayEnd::Event(event_id) => {
                let matches = facts
                    .iter()
                    .enumerate()
                    .filter(|(_, fact)| fact.event_id == event_id)
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(AuthorityError::Invalid(
                        "authority replay event selector is absent or non-unique".into(),
                    ));
                }
                matches[0].0 + 1
            }
        };
        facts.truncate(selected);
        for fact in &facts {
            if let SessionFactPayload::PromptAdmitted(prompt) = &fact.payload {
                for attachment in &prompt.content.attachments {
                    store.validate_attachment(attachment)?;
                }
            }
        }
        let state = crate::session_authority::reconstruct(&facts)?;
        store.validate_state_content(&state)?;
        let last = facts.last().expect("non-empty selected replay prefix");
        let frontier = AuthorityFrontier {
            session_id: last.session_id.clone(),
            stream_id: last.stream_id,
            sequence: last.sequence,
            event_id: last.event_id,
        };
        let records = facts.into_iter().map(ReplayRecord::from).collect();
        Ok(Self {
            store,
            records,
            state,
            frontier,
        })
    }

    pub(crate) fn records(&self) -> &[ReplayRecord] {
        &self.records
    }

    pub(crate) fn frontier(&self) -> &AuthorityFrontier {
        &self.frontier
    }

    pub(crate) fn lineage_level(&self) -> AuthorityLineageLevel {
        self.state.lineage_level
    }

    pub(crate) fn workspace_identity(&self) -> Option<&str> {
        self.state.workspace_identity.as_deref()
    }

    pub(crate) fn first_full_spine_boundary(&self) -> Option<AuthorityFrontier> {
        self.state
            .full_spine_boundary
            .map(
                |FullSpineBoundary { sequence, event_id }| AuthorityFrontier {
                    session_id: self.frontier.session_id.clone(),
                    stream_id: self.frontier.stream_id,
                    sequence,
                    event_id,
                },
            )
    }

    pub(crate) fn semantic_state(&self) -> ReplaySemanticState {
        let mut incomplete_invocations = self
            .state
            .invocations
            .iter()
            .filter_map(|(invocation_id, invocation)| {
                let kind = match invocation {
                    InvocationState::Registered { .. } => IncompleteInvocationKind::Registered,
                    InvocationState::Prepared { .. } => {
                        IncompleteInvocationKind::PreparedUnhandedOff
                    }
                    InvocationState::Dispatched { .. } => IncompleteInvocationKind::Dispatched,
                    InvocationState::Acknowledged { .. } => IncompleteInvocationKind::Acknowledged,
                    InvocationState::Unknown { .. } | InvocationState::DurableUnknown { .. } => {
                        IncompleteInvocationKind::UnknownCompletion
                    }
                    InvocationState::Settled { .. } | InvocationState::DurableSettled { .. } => {
                        return None;
                    }
                };
                Some(IncompleteInvocation {
                    invocation_id: *invocation_id,
                    kind,
                })
            })
            .collect::<Vec<_>>();
        incomplete_invocations.sort_by_key(|value| value.invocation_id);
        let mut abandoned_request_ids = self
            .state
            .model_requests
            .iter()
            .filter_map(|(request_id, request)| match request {
                ModelRequestState::Closed { closure, .. }
                    if closure.outcome == ModelRequestOutcome::Abandoned =>
                {
                    Some(*request_id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        abandoned_request_ids.sort();
        let mut abandoned_step_ids = self
            .state
            .terminal_steps
            .iter()
            .filter_map(|(step_id, terminal)| {
                matches!(terminal, StepTerminalState::Abandoned { .. }).then_some(*step_id)
            })
            .collect::<Vec<_>>();
        abandoned_step_ids.sort();
        let mut applied_compaction_ids = Vec::new();
        let mut abandoned_compaction_ids = Vec::new();
        for (compaction_id, terminal) in &self.state.compaction_terminals {
            match terminal {
                crate::session_authority::CompactionTerminalState::Applied { .. } => {
                    applied_compaction_ids.push(*compaction_id)
                }
                crate::session_authority::CompactionTerminalState::Abandoned { .. } => {
                    abandoned_compaction_ids.push(*compaction_id)
                }
            }
        }
        ReplaySemanticState {
            active_turn_id: self.state.active_turn.as_ref().map(|turn| turn.turn_id),
            active_step_id: self
                .state
                .active_step
                .as_ref()
                .map(|step| step.start.step_id),
            active_request_id: self
                .state
                .active_step
                .as_ref()
                .and_then(|step| step.active_request_id),
            incomplete_invocations,
            abandoned_request_ids,
            abandoned_step_ids,
            active_compaction_id: self.state.active_compaction,
            context_revision: self.state.context_revision,
            applied_compaction_ids,
            abandoned_compaction_ids,
        }
    }

    pub(crate) fn read_default_content(&self, content_ref: &ContentRef) -> Result<Vec<u8>> {
        self.store
            .read_content(content_ref, ProjectionClass::Default)
    }

    pub(crate) fn read_attachment(
        &self,
        attachment: &crate::session_authority::AttachmentRef,
    ) -> Result<Vec<u8>> {
        std::fs::read(self.store.validate_attachment(attachment)?).map_err(AuthorityError::Io)
    }

    pub(crate) fn authorize_restricted_continuity(
        &self,
        target_request_id: Uuid,
        continuity_id: Uuid,
        serving_provider_id: &str,
        serving_model_id: &str,
        provider_generation_id: &str,
    ) -> Result<RestrictedContinuityAuthorization> {
        let request = self
            .state
            .model_requests
            .get(&target_request_id)
            .ok_or_else(|| AuthorityError::Invalid("continuity target request is absent".into()))?;
        if !request
            .preparation()
            .continuity_refs
            .contains(&continuity_id)
        {
            return Err(AuthorityError::Invalid(
                "continuity is outside the target request lineage".into(),
            ));
        }
        let continuity = self
            .state
            .provider_continuity
            .get(&continuity_id)
            .ok_or_else(|| AuthorityError::Invalid("continuity fact is absent".into()))?;
        if continuity.serving_provider_id != serving_provider_id
            || continuity.serving_model_id != serving_model_id
            || continuity.provider_contribution_generation_id != provider_generation_id
        {
            return Err(AuthorityError::Invalid(
                "continuity authorization does not match its serving lineage".into(),
            ));
        }
        Ok(RestrictedContinuityAuthorization {
            session_id: self.frontier.session_id.clone(),
            stream_id: self.frontier.stream_id,
            target_request_id,
            continuity_id,
        })
    }

    pub(crate) fn read_restricted_continuity(
        &self,
        content_ref: &ContentRef,
        authorization: &RestrictedContinuityAuthorization,
    ) -> Result<Vec<u8>> {
        if authorization.session_id != self.frontier.session_id
            || authorization.stream_id != self.frontier.stream_id
        {
            return Err(AuthorityError::Invalid(
                "continuity authorization belongs to another authority lineage".into(),
            ));
        }
        let request = self
            .state
            .model_requests
            .get(&authorization.target_request_id)
            .ok_or_else(|| AuthorityError::Invalid("continuity target request is absent".into()))?;
        if !request
            .preparation()
            .continuity_refs
            .contains(&authorization.continuity_id)
        {
            return Err(AuthorityError::Invalid(
                "continuity authorization is stale for its request lineage".into(),
            ));
        }
        let continuity = &self.state.provider_continuity[&authorization.continuity_id];
        if &continuity.content_ref != content_ref {
            return Err(AuthorityError::Invalid(
                "continuity authorization does not cover this content reference".into(),
            ));
        }
        self.store
            .read_content(content_ref, ProjectionClass::RestrictedContinuity)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use omegon_traits::{
        RuntimeCompositionGenerationId, RuntimeContributionGenerationId, RuntimeContributionId,
    };
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::session_authority::{
        ActorIdentity, AssistantContentAppended, AssistantContentKind, AssistantContentManifest,
        AssistantMessageCommitted, ModelRequestClosed, ModelRequestPrepared, ModelRequestPurpose,
        ModelRequestRouteJoined, ModelSchemaSet, PromptAdmitted, PromptContent,
        ProviderCompletionEvidence, ProviderContinuityKind, ProviderContinuityRequiredFor,
        ProviderContinuityStored, QueueMode, RestrictedContinuityPolicy, RouteLeaseRecorded,
        SessionAuthority, StepClosed, StepOutcome, StepStarted, TurnClosed, TurnOutcome,
    };

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/session-semantic-v1"
    );
    const SESSION_ID: &str = "fixture-session";
    const STREAM_ID: Uuid = Uuid::from_u128(0x10000000_0000_4000_8000_000000000001);
    const NOW: &str = "2026-08-20T00:00:00Z";

    #[derive(Deserialize)]
    struct Manifest {
        corpus_version: u16,
        checked_vectors: Vec<CheckedVector>,
        builder_vectors: Vec<String>,
    }

    #[derive(Deserialize)]
    struct CheckedVector {
        name: String,
        file: String,
        outcome: Option<String>,
        sequence: Option<u64>,
        full_spine_boundary: Option<u64>,
        failure: Option<String>,
    }

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(Path::new(FIXTURES).join(name)).unwrap()
    }

    fn replay_fixture(name: &str, end: ReplayEnd) -> Result<SessionReplay> {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("session.json");
        fs::write(
            directory.path().join("session.authority.jsonl"),
            fixture(name),
        )
        .unwrap();
        SessionReplay::replay_prefix(&snapshot, SESSION_ID, STREAM_ID, end)
    }

    fn request(
        request_id: Uuid,
        step_id: Uuid,
        turn_id: Uuid,
        ordinal: u32,
        purpose: ModelRequestPurpose,
        replaces_request_id: Option<Uuid>,
        continuity_refs: Vec<Uuid>,
    ) -> ModelRequestPrepared {
        ModelRequestPrepared {
            request_id,
            step_id,
            turn_id,
            request_ordinal: ordinal,
            purpose,
            replaces_request_id,
            continuity_refs,
            context_manifest_id: "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
                .into(),
            context_items: Vec::new(),
            schema_set_id: "5f1988a2d75b0c21dc55ad94d2d16d3b3f10409becdad4e94dec41f616bc1b50"
                .into(),
            schema_set: ModelSchemaSet {
                schema_set_version: 1,
                composition_generation_id: RuntimeCompositionGenerationId::new("composition:test")
                    .unwrap(),
                normalizer_contribution_id: RuntimeContributionId::new("feature:schema-normalizer")
                    .unwrap(),
                normalizer_generation_id: RuntimeContributionGenerationId::new(
                    "contribution:schema-normalizer-v1",
                )
                .unwrap(),
                schemas: Vec::new(),
            },
        }
    }

    fn route(request_id: Uuid, turn_id: Uuid, lease_id: Uuid) -> RouteLeaseRecorded {
        RouteLeaseRecorded {
            lease_id,
            request_id,
            turn_id,
            selected_provider_id: "fixture".into(),
            selected_model_id: "model".into(),
            serving_provider_id: "fixture".into(),
            serving_model_id: "model".into(),
            schema_dialect: "open_ai".into(),
            credential_source_class: "test".into(),
            fallback_reason: None,
            contribution_generation_id: "provider:fixture/v1".into(),
            route_policy: "direct".into(),
        }
    }

    pub(crate) fn open_joined_request(
        directory: &tempfile::TempDir,
    ) -> (SessionAuthority, ModelRequestPrepared, Uuid, Uuid) {
        let snapshot = directory.path().join("session.json");
        let mut authority = SessionAuthority::open(
            &snapshot,
            SESSION_ID,
            "fixture-workspace",
            "composition:test",
            ActorIdentity {
                principal: "operator".into(),
                ingress: "fixture".into(),
            },
            NOW,
        )
        .unwrap();
        let prompt_id = Uuid::new_v4();
        let turn_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id,
                    principal: "operator".into(),
                    ingress: "fixture".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "fixture request".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        authority
            .start_turn(Uuid::new_v4(), NOW, turn_id, prompt_id)
            .unwrap();
        authority
            .start_step(
                Uuid::new_v4(),
                NOW,
                StepStarted {
                    step_id,
                    turn_id,
                    step_ordinal: 0,
                },
            )
            .unwrap();
        let model_request = request(
            Uuid::new_v4(),
            step_id,
            turn_id,
            0,
            ModelRequestPurpose::Initial,
            None,
            Vec::new(),
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, model_request.clone())
            .unwrap();
        let lease_id = Uuid::new_v4();
        authority
            .record_route_lease(NOW, route(model_request.request_id, turn_id, lease_id))
            .unwrap();
        authority
            .join_model_request_route(
                Uuid::new_v4(),
                NOW,
                ModelRequestRouteJoined {
                    request_id: model_request.request_id,
                    step_id,
                    turn_id,
                    lease_id,
                },
            )
            .unwrap();
        (authority, model_request, step_id, turn_id)
    }

    #[test]
    fn canonical_fixture_bytes_and_scenario_inventory_are_frozen() {
        let expected = [
            (
                "manifest.json",
                "f1a07becb075ead7939cda83684d306139ecc11266ab4d2eb8a78621056593be",
            ),
            (
                "builder-recipes.json",
                "83886a29a1da244bbd1c26ceb2560a939023125c3e14d7556c83af52fa837ad2",
            ),
            (
                "slice-1-closed.authority.jsonl",
                "0113307663a1690a2b01df5915a3211de03174f79852caf141428bdbb77f9f69",
            ),
            (
                "legacy-open-recovery.authority.jsonl",
                "8ce2fb5666e91f96ba1f3e79a5e97241718f6596db1ee5bcc77b29148f7ce7ce",
            ),
            (
                "legacy-route-only.authority.jsonl",
                "47bb4f30d74386cd2a926b0a29bf51826c54bd4e9aa4f11fc00e00f3ae7abc00",
            ),
            (
                "mixed-legacy-full.authority.jsonl",
                "89d74c08f044a70be4f8bd76e9925505b644f9e17d349cce41c310d03ed7f87f",
            ),
            (
                "full-spine-crash-prefix.authority.jsonl",
                "0e4963dc3c166a1024d28f0ab7facb45dae6c37124bf320c480f868824fb3c76",
            ),
            (
                "unsupported-event.authority.jsonl",
                "657585138530b816ff20a3748970538f981a8689edbcb41e08437da3fe301412",
            ),
            (
                "unsupported-version.authority.jsonl",
                "512321b52f3ab0dd9d4396dc29271fab20e956369317c6e98103d84d98081310",
            ),
            (
                "sequence-conflict.authority.jsonl",
                "5d4126055c63da3185255fc38e3a3ed93f7de53337119af8c9424b7ff4d9345d",
            ),
            (
                "event-conflict.authority.jsonl",
                "7de5519c5f06de1920c1c327d1f38fccddf8614f66c9a31fdf1da660ff2ce8c1",
            ),
            (
                "command-conflict.authority.jsonl",
                "dea694a76b3c6d84b36523239025c243d36989e1c1925e51cfa57fb30264d3a5",
            ),
            (
                "truncated-prefix.authority.jsonl",
                "09c653bbde1a6ea6294b114393ebfa63c1d32824382fb225e47c05209738d5d3",
            ),
            (
                "blobs/sha256/36f5211e9b196ea1059f1a0a2df4911c951830a042e74d77e91df91f2c72a37c",
                "36f5211e9b196ea1059f1a0a2df4911c951830a042e74d77e91df91f2c72a37c",
            ),
            (
                "blobs/sha256/36f5211e9b196ea1059f1a0a2df4911c951830a042e74d77e91df91f2c72a37c.meta.json",
                "679781f7dae171c5194bfb38ddba89d6e52c57f4843052680f876978cfd61f79",
            ),
            (
                "blobs/sha256/f759398a26aba43305a3bfcdb7cf58cb97d65b323580e61b57eb1e2323bbebe8",
                "f759398a26aba43305a3bfcdb7cf58cb97d65b323580e61b57eb1e2323bbebe8",
            ),
            (
                "blobs/sha256/f759398a26aba43305a3bfcdb7cf58cb97d65b323580e61b57eb1e2323bbebe8.meta.json",
                "8c3db8ad68167fd3c2338a61df7b88ef00c9f3fa7f72e8c9f32c6d2e85fa3858",
            ),
        ];
        for (name, digest) in expected {
            assert_eq!(
                format!("{:x}", Sha256::digest(fixture(name))),
                digest,
                "{name}"
            );
        }

        let manifest: Manifest = serde_json::from_slice(&fixture("manifest.json")).unwrap();
        assert_eq!(manifest.corpus_version, 1);
        assert_eq!(manifest.checked_vectors.len(), 11);
        assert_eq!(manifest.builder_vectors.len(), 13);
        assert!(manifest.builder_vectors.contains(&"text-response".into()));
        assert!(
            manifest
                .builder_vectors
                .contains(&"default-vs-restricted-blob".into())
        );
        assert!(
            manifest
                .builder_vectors
                .contains(&"turn-compaction-success".into())
        );
        assert!(
            manifest
                .builder_vectors
                .contains(&"idle-compaction-success".into())
        );
        let recipes: serde_json::Value =
            serde_json::from_slice(&fixture("builder-recipes.json")).unwrap();
        assert_eq!(recipes["recipe_version"], 1);
        assert_eq!(recipes["vectors"].as_array().unwrap().len(), 13);
        for vector in manifest.checked_vectors {
            assert!(!vector.name.is_empty());
            assert!(!vector.file.is_empty());
            assert!(vector.outcome.is_some() || vector.failure.is_some());
            if vector.outcome.is_some() {
                assert!(vector.sequence.is_some());
            }
            if vector.full_spine_boundary.is_some() {
                assert!(vector.outcome.is_some());
            }
        }
    }

    #[test]
    fn checked_lineages_replay_to_exact_immutable_frontiers() {
        let closed =
            replay_fixture("slice-1-closed.authority.jsonl", ReplayEnd::EndOfStream).unwrap();
        assert_eq!(closed.lineage_level(), AuthorityLineageLevel::LegacyOnly);
        assert_eq!(closed.frontier().session_id(), SESSION_ID);
        assert_eq!(closed.frontier().stream_id(), STREAM_ID);
        assert_eq!(closed.frontier().sequence(), 4);
        assert_eq!(closed.records().len(), 4);
        assert!(closed.first_full_spine_boundary().is_none());
        assert_eq!(closed.records()[3].event_type(), "turn.closed");

        let legacy_open = replay_fixture(
            "legacy-open-recovery.authority.jsonl",
            ReplayEnd::EndOfStream,
        )
        .unwrap();
        assert_eq!(
            legacy_open.semantic_state().active_turn_id,
            Some(Uuid::from_u128(0x60000000_0000_4000_8000_000000000001))
        );
        assert!(legacy_open.semantic_state().active_step_id.is_none());

        let route_only =
            replay_fixture("legacy-route-only.authority.jsonl", ReplayEnd::EndOfStream).unwrap();
        assert_eq!(
            route_only.lineage_level(),
            AuthorityLineageLevel::LegacyOnly
        );
        assert_eq!(route_only.records()[3].event_type(), "route.lease_recorded");

        let full = replay_fixture(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::EndOfStream,
        )
        .unwrap();
        assert_eq!(full.lineage_level(), AuthorityLineageLevel::FullSpine);
        assert_eq!(full.first_full_spine_boundary().unwrap().sequence(), 4);
        assert_eq!(
            full.semantic_state().active_step_id,
            Some(Uuid::from_u128(0x90000000_0000_4000_8000_000000000001))
        );

        let mixed =
            replay_fixture("mixed-legacy-full.authority.jsonl", ReplayEnd::EndOfStream).unwrap();
        assert_eq!(mixed.lineage_level(), AuthorityLineageLevel::Mixed);
        assert_eq!(mixed.first_full_spine_boundary().unwrap().sequence(), 8);
    }

    #[test]
    fn prefix_selectors_reduce_exactly_without_recovery_or_cache_writes() {
        let by_sequence = replay_fixture(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::Sequence(3),
        )
        .unwrap();
        assert_eq!(by_sequence.frontier().sequence(), 3);
        assert_eq!(
            by_sequence.lineage_level(),
            AuthorityLineageLevel::LegacyOnly
        );
        assert!(by_sequence.semantic_state().active_step_id.is_none());

        let by_event = replay_fixture(
            "full-spine-crash-prefix.authority.jsonl",
            ReplayEnd::Event(Uuid::from_u128(0x20000000_0000_4000_8000_000000000004)),
        )
        .unwrap();
        assert_eq!(by_event.frontier().sequence(), 4);
        assert_eq!(by_event.records().len(), 4);
    }

    #[test]
    fn unsupported_and_conflicting_checked_vectors_fail_closed() {
        let vectors = [
            (
                "unsupported-event.authority.jsonl",
                "unsupported authority event type",
            ),
            ("unsupported-version.authority.jsonl", "event version 2"),
            (
                "sequence-conflict.authority.jsonl",
                "expected sequence 2, got 3",
            ),
            ("event-conflict.authority.jsonl", "duplicate event ID"),
            ("command-conflict.authority.jsonl", "duplicate command ID"),
            ("truncated-prefix.authority.jsonl", "authority JSON failed"),
        ];
        for (name, expected) in vectors {
            let error = replay_fixture(name, ReplayEnd::EndOfStream).unwrap_err();
            assert!(error.to_string().contains(expected), "{name}: {error}");
        }
    }

    #[test]
    fn builder_text_response_replays_content_and_blob_loss_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("session.json");
        let (mut authority, request, step_id, turn_id) = open_joined_request(&directory);
        let stream_id = authority.state().stream_id.unwrap();
        let message_id = Uuid::new_v4();
        let bytes = b"hello replay";
        let content_ref = authority
            .write_content(bytes, "text/plain", ProjectionClass::Default)
            .unwrap();
        authority
            .append_assistant_content(
                Uuid::new_v4(),
                NOW,
                AssistantContentAppended {
                    message_id,
                    request_id: request.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    content_kind: AssistantContentKind::Text,
                    chunk_ordinal: 0,
                    content_ref: content_ref.clone(),
                },
            )
            .unwrap();
        authority
            .commit_assistant_message(
                Uuid::new_v4(),
                NOW,
                AssistantMessageCommitted {
                    message_id,
                    request_id: request.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    completion_evidence: ProviderCompletionEvidence::ProviderDone,
                    content: vec![AssistantContentManifest {
                        content_kind: AssistantContentKind::Text,
                        chunk_refs: vec![content_ref.clone()],
                        content_digest: format!("{:x}", Sha256::digest(bytes)),
                    }],
                    usage: None,
                    tool_call_count: 0,
                },
            )
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: request.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::ResponseCompleted,
                    reason_code: "provider_done".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        authority
            .close_step(
                Uuid::new_v4(),
                NOW,
                StepClosed {
                    step_id,
                    turn_id,
                    outcome: StepOutcome::TurnCompleted,
                    reason_code: "assistant_complete".into(),
                },
            )
            .unwrap();
        authority
            .close_turn(
                Uuid::new_v4(),
                NOW,
                TurnClosed {
                    turn_id,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        let downgraded_prompt_id = Uuid::new_v4();
        let downgraded_turn_id = Uuid::new_v4();
        authority
            .admit_prompt(
                Uuid::new_v4(),
                NOW,
                PromptAdmitted {
                    submission_id: Uuid::new_v4(),
                    prompt_id: downgraded_prompt_id,
                    principal: "operator".into(),
                    ingress: "fixture".into(),
                    queue_mode: QueueMode::UntilReady,
                    content: PromptContent {
                        text: "old writer downgrade".into(),
                        attachments: Vec::new(),
                    },
                    metadata: serde_json::json!({}),
                },
            )
            .unwrap();
        authority
            .start_turn(
                Uuid::new_v4(),
                NOW,
                downgraded_turn_id,
                downgraded_prompt_id,
            )
            .unwrap();
        let error = authority
            .close_turn(
                Uuid::new_v4(),
                NOW,
                TurnClosed {
                    turn_id: downgraded_turn_id,
                    outcome: TurnOutcome::Completed,
                    reason_code: "completed".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("completed full-spine turn"));
        drop(authority);

        let replay =
            SessionReplay::replay_prefix(&snapshot, SESSION_ID, stream_id, ReplayEnd::EndOfStream)
                .unwrap();
        assert_eq!(replay.read_default_content(&content_ref).unwrap(), bytes);

        let blob = directory
            .path()
            .join("session.authority.blobs/sha256")
            .join(content_ref.digest());
        fs::remove_file(&blob).unwrap();
        let error =
            SessionReplay::replay_prefix(&snapshot, SESSION_ID, stream_id, ReplayEnd::EndOfStream)
                .unwrap_err();
        assert!(error.to_string().contains("session blob I/O failed"));

        fs::write(&blob, b"hello tamper").unwrap();
        let error =
            SessionReplay::replay_prefix(&snapshot, SESSION_ID, stream_id, ReplayEnd::EndOfStream)
                .unwrap_err();
        assert!(matches!(error, AuthorityError::Blob(_)));
    }

    #[test]
    fn builder_restricted_continuity_requires_exact_request_and_serving_authorization() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("session.json");
        let (mut authority, first, step_id, turn_id) = open_joined_request(&directory);
        let stream_id = authority.state().stream_id.unwrap();
        let continuity_id = Uuid::new_v4();
        let restricted_ref = authority
            .write_content(
                b"opaque-state",
                "application/octet-stream",
                ProjectionClass::RestrictedContinuity,
            )
            .unwrap();
        authority
            .store_provider_continuity(
                Uuid::new_v4(),
                NOW,
                ProviderContinuityStored {
                    continuity_id,
                    request_id: first.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    serving_provider_id: "fixture".into(),
                    serving_model_id: "model".into(),
                    provider_contribution_generation_id: "provider:fixture/v1".into(),
                    continuity_kind: ProviderContinuityKind::OpaqueProviderState,
                    required_for: ProviderContinuityRequiredFor::NextRequest,
                    restricted_required: RestrictedContinuityPolicy {
                        allowed_kinds: vec![ProviderContinuityKind::OpaqueProviderState],
                        max_blob_bytes: 1024,
                    },
                    content_ref: restricted_ref.clone(),
                },
            )
            .unwrap();
        authority
            .close_model_request(
                Uuid::new_v4(),
                NOW,
                ModelRequestClosed {
                    request_id: first.request_id,
                    step_id,
                    response_attempt_ordinal: 0,
                    outcome: ModelRequestOutcome::SupersededForHistoryRepair,
                    reason_code: "history_repair".into(),
                    recovery_rule_version: None,
                },
            )
            .unwrap();
        let second = request(
            Uuid::new_v4(),
            step_id,
            turn_id,
            1,
            ModelRequestPurpose::ProviderHistoryRepair,
            Some(first.request_id),
            vec![continuity_id],
        );
        authority
            .prepare_model_request(Uuid::new_v4(), NOW, second.clone())
            .unwrap();
        let lease_id = Uuid::new_v4();
        authority
            .record_route_lease(NOW, route(second.request_id, turn_id, lease_id))
            .unwrap();
        authority
            .join_model_request_route(
                Uuid::new_v4(),
                NOW,
                ModelRequestRouteJoined {
                    request_id: second.request_id,
                    step_id,
                    turn_id,
                    lease_id,
                },
            )
            .unwrap();
        drop(authority);

        let replay =
            SessionReplay::replay_prefix(&snapshot, SESSION_ID, stream_id, ReplayEnd::EndOfStream)
                .unwrap();
        assert!(replay.read_default_content(&restricted_ref).is_err());
        assert!(
            replay
                .authorize_restricted_continuity(
                    second.request_id,
                    continuity_id,
                    "other-provider",
                    "model",
                    "provider:fixture/v1",
                )
                .is_err()
        );
        let authorization = replay
            .authorize_restricted_continuity(
                second.request_id,
                continuity_id,
                "fixture",
                "model",
                "provider:fixture/v1",
            )
            .unwrap();
        assert_eq!(
            replay
                .read_restricted_continuity(&restricted_ref, &authorization)
                .unwrap(),
            b"opaque-state"
        );
    }
}
