//! Durable, non-authoritative turn telemetry checkpoints.
//!
//! Semantic identity and counters are reduced from session authority. Agent
//! events only wake reconciliation and may contribute explicitly observational
//! provider/context values. This stream is neither session authority nor the
//! compaction checkpoint.

use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    session_authority::{SessionFactPayload, TurnOutcome},
    session_consumers::{SessionViewBinding, SessionViewTarget},
    session_replay::{ReplayEnd, SessionReplay},
};

const TELEMETRY_SCHEMA_VERSION: u16 = 1;
const CURSOR_SCHEMA_VERSION: u16 = 1;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_STREAM_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CURSOR_BYTES: u64 = 64 * 1024;
const CHECKPOINT_NAMESPACE: Uuid = Uuid::from_u128(0x64b4_c69e_4b8a_41bf_b622_94e0_87f2_911a);

#[derive(Debug, thiserror::Error)]
pub(crate) enum TelemetryCheckpointError {
    #[error("telemetry checkpoint I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("telemetry checkpoint JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("telemetry checkpoint is invalid: {0}")]
    Invalid(String),
    #[error("semantic telemetry source is unavailable: {0}")]
    Semantic(String),
}

type Result<T> = std::result::Result<T, TelemetryCheckpointError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TelemetrySessionModeV1 {
    Semantic,
    SessionlessCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelemetryBindingV1 {
    pub(crate) session_id: String,
    pub(crate) stream_id: Option<Uuid>,
    pub(crate) host_generation: u64,
    pub(crate) mode: TelemetrySessionModeV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelemetrySourceFrontierV1 {
    pub(crate) stream_id: Uuid,
    pub(crate) sequence: u64,
    pub(crate) event_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanonicalTurnCompletionV1 {
    pub(crate) turn_id: Uuid,
    pub(crate) terminal_event_id: Uuid,
    pub(crate) outcome: TurnOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DerivedTurnTelemetryV1 {
    pub(crate) completed_turns: Option<u64>,
    pub(crate) requests: Option<u64>,
    pub(crate) tool_calls: Option<u64>,
    pub(crate) context_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservationSourceV1 {
    RuntimeTurnEnd,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservationQualityV1 {
    SameGenerationAdvisory,
    UnavailableOrLagged,
    SessionlessCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedTurnTelemetryV1 {
    pub(crate) source: ObservationSourceV1,
    pub(crate) quality: ObservationQualityV1,
    pub(crate) advisory_turn: Option<u32>,
    pub(crate) model: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) estimated_tokens: Option<u64>,
    pub(crate) context_window: Option<u64>,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cache_read_tokens: Option<u64>,
    pub(crate) cache_creation_tokens: Option<u64>,
    pub(crate) context_class: Option<String>,
    pub(crate) thinking_level: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TurnTelemetryCheckpointV1 {
    pub(crate) telemetry_schema_version: u16,
    pub(crate) record_kind: String,
    pub(crate) telemetry_sequence: u64,
    pub(crate) checkpoint_id: Uuid,
    pub(crate) recorded_at: String,
    pub(crate) binding: TelemetryBindingV1,
    pub(crate) canonical_completion: Option<CanonicalTurnCompletionV1>,
    pub(crate) semantic_source_frontier: Option<TelemetrySourceFrontierV1>,
    pub(crate) derived: DerivedTurnTelemetryV1,
    pub(crate) observed: ObservedTurnTelemetryV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelemetryCursorV1 {
    cursor_schema_version: u16,
    session_id: String,
    stream_id: Option<Uuid>,
    last_telemetry_sequence: u64,
    last_checkpoint_id: Uuid,
    last_terminal_event_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompatibleCheckpoint {
    V1(Box<TurnTelemetryCheckpointV1>),
    Legacy(Box<LegacyTurnCheckpoint>),
}

impl CompatibleCheckpoint {
    pub(crate) fn compatibility_label(&self) -> &'static str {
        match self {
            Self::V1(_) => "turn_telemetry_v1",
            Self::Legacy(_) => "legacy_checkpoint_compatibility",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LegacyTurnCheckpoint {
    pub(crate) timestamp_unix_ms: u64,
    pub(crate) session_id: String,
    pub(crate) turn: u32,
    pub(crate) model: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) estimated_tokens: usize,
    pub(crate) context_window: usize,
    pub(crate) actual_input_tokens: u64,
    pub(crate) actual_output_tokens: u64,
    pub(crate) intent: LegacyIntentSnapshot,
    pub(crate) metrics: LegacyMetricsSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LegacyIntentSnapshot {
    pub(crate) current_task: Option<String>,
    pub(crate) lifecycle_phase: String,
    pub(crate) files_read_count: usize,
    pub(crate) files_modified_count: usize,
    pub(crate) stats_turns: u32,
    pub(crate) stats_tool_calls: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LegacyMetricsSnapshot {
    pub(crate) tokens_used: usize,
    pub(crate) context_window: usize,
    pub(crate) context_class: String,
    pub(crate) thinking_level: String,
}

#[derive(Debug, Clone)]
struct RuntimeObservation {
    turn: u32,
    model: Option<String>,
    provider: Option<String>,
    estimated_tokens: usize,
    context_window: usize,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
}

impl From<&omegon_traits::AgentEventTurnEnd> for RuntimeObservation {
    fn from(value: &omegon_traits::AgentEventTurnEnd) -> Self {
        Self {
            turn: value.turn,
            model: value.model.clone(),
            provider: value.provider.clone(),
            estimated_tokens: value.estimated_tokens,
            context_window: value.context_window,
            input_tokens: value.actual_input_tokens,
            output_tokens: value.actual_output_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_creation_tokens: value.cache_creation_tokens,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BindingFence {
    session_id: String,
    stream_id: Option<Uuid>,
    generation: u64,
}

impl From<&SessionViewTarget> for BindingFence {
    fn from(value: &SessionViewTarget) -> Self {
        Self {
            session_id: value.session_id.clone(),
            stream_id: value.stream_id,
            generation: value.generation,
        }
    }
}

#[derive(Debug, Clone)]
struct TelemetryStore {
    root: PathBuf,
}

impl TelemetryStore {
    fn default_store() -> Result<Self> {
        let root = crate::paths::omegon_home()
            .unwrap_or_else(|_| PathBuf::from(".omegon"))
            .join("checkpoints");
        Self::at(root)
    }

    fn at(root: PathBuf) -> Result<Self> {
        ensure_restricted_directory(&root)?;
        Ok(Self { root })
    }

    fn stream_path(&self, session_id: &str) -> Result<PathBuf> {
        validate_session_id(session_id)?;
        Ok(self.root.join(format!("{session_id}.telemetry.v1.jsonl")))
    }

    fn cursor_path(&self, session_id: &str) -> Result<PathBuf> {
        validate_session_id(session_id)?;
        Ok(self
            .root
            .join(format!("{session_id}.telemetry.v1.cursor.json")))
    }

    fn legacy_path(&self, session_id: &str) -> Result<PathBuf> {
        validate_session_id(session_id)?;
        Ok(self.root.join(format!("{session_id}.jsonl")))
    }

    fn read_records(&self, session_id: &str) -> Result<Vec<TurnTelemetryCheckpointV1>> {
        let path = self.stream_path(session_id)?;
        let Some(bytes) = read_optional_bounded(&path, MAX_STREAM_BYTES)? else {
            return Ok(Vec::new());
        };
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry stream has an incomplete tail".into(),
            ));
        }
        let mut records = Vec::new();
        let mut ids = BTreeSet::new();
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            if line.len() > MAX_RECORD_BYTES {
                return Err(TelemetryCheckpointError::Invalid(format!(
                    "telemetry record {} exceeds 64 KiB",
                    index + 1
                )));
            }
            let record: TurnTelemetryCheckpointV1 = strict_json(line)?;
            validate_record(&record)?;
            let expected_sequence = u64::try_from(records.len())
                .map_err(|_| TelemetryCheckpointError::Invalid("record count overflow".into()))?
                + 1;
            if record.telemetry_sequence != expected_sequence {
                return Err(TelemetryCheckpointError::Invalid(format!(
                    "telemetry sequence is not contiguous at record {}",
                    index + 1
                )));
            }
            if !ids.insert(record.checkpoint_id) {
                return Err(TelemetryCheckpointError::Invalid(
                    "duplicate telemetry checkpoint identity".into(),
                ));
            }
            records.push(record);
        }
        self.validate_cursor(session_id, &records)?;
        Ok(records)
    }

    fn validate_cursor(
        &self,
        session_id: &str,
        records: &[TurnTelemetryCheckpointV1],
    ) -> Result<()> {
        let Some(bytes) = read_optional_bounded(&self.cursor_path(session_id)?, MAX_CURSOR_BYTES)?
        else {
            return Ok(());
        };
        let cursor: TelemetryCursorV1 = strict_json(&bytes)?;
        if cursor.cursor_schema_version != CURSOR_SCHEMA_VERSION || cursor.session_id != session_id
        {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry cursor identity or version is invalid".into(),
            ));
        }
        let Some(last) = records.last() else {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry cursor exists without records".into(),
            ));
        };
        if cursor.last_telemetry_sequence > last.telemetry_sequence {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry cursor is ahead of the durable stream".into(),
            ));
        }
        let cursor_record = records
            .get(cursor.last_telemetry_sequence.saturating_sub(1) as usize)
            .ok_or_else(|| {
                TelemetryCheckpointError::Invalid(
                    "telemetry cursor does not identify a durable record".into(),
                )
            })?;
        if cursor.last_checkpoint_id != cursor_record.checkpoint_id
            || cursor.stream_id != cursor_record.binding.stream_id
            || cursor.last_terminal_event_id
                != cursor_record
                    .canonical_completion
                    .as_ref()
                    .map(|completion| completion.terminal_event_id)
        {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry cursor does not identify its durable record".into(),
            ));
        }
        Ok(())
    }

    fn append(&self, mut record: TurnTelemetryCheckpointV1) -> Result<bool> {
        let records = self.read_records(&record.binding.session_id)?;
        if records
            .iter()
            .any(|existing| existing.checkpoint_id == record.checkpoint_id)
        {
            self.write_cursor(records.last().expect("deduplicated record exists"))?;
            return Ok(false);
        }
        record.telemetry_sequence = u64::try_from(records.len())
            .map_err(|_| TelemetryCheckpointError::Invalid("record count overflow".into()))?
            .checked_add(1)
            .ok_or_else(|| TelemetryCheckpointError::Invalid("sequence overflow".into()))?;
        validate_record(&record)?;
        let mut bytes = serde_json::to_vec(&record)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry record exceeds 64 KiB".into(),
            ));
        }
        let path = self.stream_path(&record.binding.session_id)?;
        let existing_len = fs::metadata(&path).map_or(0, |metadata| metadata.len());
        if existing_len.saturating_add(bytes.len() as u64) > MAX_STREAM_BYTES {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry stream exceeds 16 MiB".into(),
            ));
        }
        secure_append_sync(&path, &bytes)?;
        self.write_cursor(&record)?;
        Ok(true)
    }

    fn write_cursor(&self, record: &TurnTelemetryCheckpointV1) -> Result<()> {
        let cursor = TelemetryCursorV1 {
            cursor_schema_version: CURSOR_SCHEMA_VERSION,
            session_id: record.binding.session_id.clone(),
            stream_id: record.binding.stream_id,
            last_telemetry_sequence: record.telemetry_sequence,
            last_checkpoint_id: record.checkpoint_id,
            last_terminal_event_id: record
                .canonical_completion
                .as_ref()
                .map(|completion| completion.terminal_event_id),
        };
        atomic_replace(
            &self.cursor_path(&record.binding.session_id)?,
            &serde_json::to_vec(&cursor)?,
        )?;
        Ok(())
    }

    fn read_compatible_last(&self, session_id: &str) -> Result<Option<CompatibleCheckpoint>> {
        let records = self.read_records(session_id)?;
        if let Some(record) = records.last() {
            return Ok(Some(CompatibleCheckpoint::V1(Box::new(record.clone()))));
        }
        let Some(bytes) = read_optional_bounded(&self.legacy_path(session_id)?, MAX_STREAM_BYTES)?
        else {
            return Ok(None);
        };
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(TelemetryCheckpointError::Invalid(
                "legacy checkpoint stream has an incomplete tail".into(),
            ));
        }
        let mut last = None;
        for line in bytes.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            if line.len() > MAX_RECORD_BYTES {
                return Err(TelemetryCheckpointError::Invalid(
                    "legacy checkpoint record exceeds 64 KiB".into(),
                ));
            }
            last = Some(strict_json::<LegacyTurnCheckpoint>(line)?);
        }
        Ok(last.map(|record| CompatibleCheckpoint::Legacy(Box::new(record))))
    }
}

pub(crate) fn read_last_checkpoint(session_id: &str) -> Result<Option<CompatibleCheckpoint>> {
    TelemetryStore::default_store()?.read_compatible_last(session_id)
}

pub(crate) fn diagnose_startup_consistency(snapshot: &Path, session_id: &str) {
    let checkpoint = match read_last_checkpoint(session_id) {
        Ok(Some(checkpoint)) => checkpoint,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, %session_id, "telemetry checkpoint is unavailable");
            return;
        }
    };
    match checkpoint {
        CompatibleCheckpoint::Legacy(_) => tracing::debug!(
            %session_id,
            compatibility = "legacy_checkpoint_compatibility",
            "legacy checkpoint is observational only; no authority consistency claim made"
        ),
        CompatibleCheckpoint::V1(record) => {
            let Some(frontier) = record.semantic_source_frontier else {
                tracing::debug!(%session_id, "sessionless telemetry has no semantic frontier");
                return;
            };
            match SessionReplay::replay_prefix(
                snapshot,
                session_id,
                frontier.stream_id,
                ReplayEnd::EndOfStream,
            ) {
                Ok(replay)
                    if replay.frontier().sequence() >= frontier.sequence
                        && replay
                            .records()
                            .get(frontier.sequence.saturating_sub(1) as usize)
                            .is_some_and(|record| {
                                record.frontier().event_id() == frontier.event_id
                            }) =>
                {
                    tracing::debug!(
                        %session_id,
                        checkpoint_sequence = frontier.sequence,
                        authority_sequence = replay.frontier().sequence(),
                        "telemetry source frontier is compatible with semantic authority/catalog"
                    );
                }
                Ok(_) => tracing::warn!(
                    %session_id,
                    checkpoint_sequence = frontier.sequence,
                    "telemetry source frontier is incompatible with semantic authority/catalog"
                ),
                Err(error) => tracing::warn!(
                    %session_id,
                    %error,
                    "telemetry source frontier could not be compared with semantic authority/catalog"
                ),
            }
        }
    }
}

pub(crate) fn spawn_checkpoint_subscriber(
    events_tx: &tokio::sync::broadcast::Sender<omegon_traits::AgentEvent>,
    binding: SessionViewBinding,
    context_metrics: std::sync::Arc<
        std::sync::Mutex<crate::features::context::SharedContextMetrics>,
    >,
) -> tokio::task::JoinHandle<()> {
    let mut events = events_tx.subscribe();
    let mut generations = binding.subscribe_generation();
    tokio::spawn(async move {
        let store = match TelemetryStore::default_store() {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(%error, "telemetry checkpoint writer is unavailable");
                return;
            }
        };
        let mut started_turns: HashMap<u32, BindingFence> = HashMap::new();
        reconcile(&store, &binding.snapshot(), None, &context_metrics);
        loop {
            tokio::select! {
                changed = generations.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    started_turns.clear();
                    reconcile(&store, &binding.snapshot(), None, &context_metrics);
                }
                event = events.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "telemetry checkpoint subscriber lagged; reconciling authority");
                            reconcile(&store, &binding.snapshot(), None, &context_metrics);
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    match event {
                        omegon_traits::AgentEvent::TurnStart { turn } => {
                            started_turns.insert(turn, BindingFence::from(&binding.snapshot()));
                        }
                        omegon_traits::AgentEvent::TurnEnd(turn_end) => {
                            let target = binding.snapshot();
                            let current = BindingFence::from(&target);
                            let observation = started_turns
                                .remove(&turn_end.turn)
                                .filter(|started| *started == current)
                                .map(|_| RuntimeObservation::from(turn_end.as_ref()));
                            if observation.is_none() {
                                tracing::debug!(turn = turn_end.turn, "fenced stale or unbound TurnEnd telemetry");
                            }
                            reconcile(&store, &target, observation, &context_metrics);
                        }
                        omegon_traits::AgentEvent::RuntimeTurnLifecycleUpdated { .. }
                        | omegon_traits::AgentEvent::RuntimeQueueUpdated { .. }
                        | omegon_traits::AgentEvent::AgentEnd
                        | omegon_traits::AgentEvent::SessionReset => {
                            reconcile(&store, &binding.snapshot(), None, &context_metrics);
                        }
                        _ => {}
                    }
                }
            }
        }
    })
}

fn reconcile(
    store: &TelemetryStore,
    target: &SessionViewTarget,
    observation: Option<RuntimeObservation>,
    context_metrics: &std::sync::Arc<
        std::sync::Mutex<crate::features::context::SharedContextMetrics>,
    >,
) {
    if let Err(error) = reconcile_inner(store, target, observation, context_metrics) {
        tracing::warn!(
            %error,
            session_id = %target.session_id,
            generation = target.generation,
            "telemetry checkpoint reconciliation failed; terminal commitment remains unaffected"
        );
    }
}

fn reconcile_inner(
    store: &TelemetryStore,
    target: &SessionViewTarget,
    observation: Option<RuntimeObservation>,
    context_metrics: &std::sync::Arc<
        std::sync::Mutex<crate::features::context::SharedContextMetrics>,
    >,
) -> Result<()> {
    let metrics = context_metrics.lock().ok().map(|metrics| {
        (
            metrics.context_class.clone(),
            metrics.thinking_level.clone(),
        )
    });
    let replay = match target.stream_id {
        Some(stream_id) => Some(
            SessionReplay::replay_prefix(
                &target.snapshot,
                &target.session_id,
                stream_id,
                ReplayEnd::EndOfStream,
            )
            .map_err(|error| TelemetryCheckpointError::Semantic(error.to_string()))?,
        ),
        None => {
            let has_authority = crate::session_host_storage::has_authority(&target.snapshot)
                .map_err(|error| TelemetryCheckpointError::Semantic(error.to_string()))?;
            if has_authority {
                Some(
                    SessionReplay::replay_session(
                        &target.snapshot,
                        &target.session_id,
                        ReplayEnd::EndOfStream,
                    )
                    .map_err(|error| TelemetryCheckpointError::Semantic(error.to_string()))?,
                )
            } else {
                None
            }
        }
    };
    let resolved_stream_id = replay.as_ref().map(|replay| replay.frontier().stream_id());
    let binding = TelemetryBindingV1 {
        session_id: target.session_id.clone(),
        stream_id: resolved_stream_id,
        host_generation: target.generation,
        mode: if resolved_stream_id.is_some() {
            TelemetrySessionModeV1::Semantic
        } else {
            TelemetrySessionModeV1::SessionlessCompatibility
        },
    };
    let Some(replay) = replay else {
        if let Some(observation) = observation {
            let checkpoint_id = Uuid::new_v5(
                &CHECKPOINT_NAMESPACE,
                format!(
                    "sessionless\0{}\0{}\0{}",
                    target.session_id, target.generation, observation.turn
                )
                .as_bytes(),
            );
            store.append(TurnTelemetryCheckpointV1 {
                telemetry_schema_version: TELEMETRY_SCHEMA_VERSION,
                record_kind: "turn_telemetry_checkpoint".into(),
                telemetry_sequence: 0,
                checkpoint_id,
                recorded_at: now(),
                binding,
                canonical_completion: None,
                semantic_source_frontier: None,
                derived: DerivedTurnTelemetryV1 {
                    completed_turns: None,
                    requests: None,
                    tool_calls: None,
                    context_revision: None,
                },
                observed: observed(Some(observation), metrics, true),
            })?;
        }
        return Ok(());
    };
    let stream_id = replay.frontier().stream_id();
    let existing_ids = store
        .read_records(&target.session_id)?
        .into_iter()
        .map(|record| record.checkpoint_id)
        .collect::<BTreeSet<_>>();
    let mut completed_turns = 0_u64;
    let mut requests = 0_u64;
    let mut tool_calls = 0_u64;
    let mut context_revision = 0_u64;
    let terminal_count = replay
        .records()
        .iter()
        .filter(|record| matches!(record.payload(), SessionFactPayload::TurnClosed(_)))
        .count();
    let mut terminal_ordinal = 0_usize;
    for record in replay.records() {
        match record.payload() {
            SessionFactPayload::ModelRequestPrepared(_) => requests += 1,
            SessionFactPayload::ToolCallRecorded(_) => tool_calls += 1,
            SessionFactPayload::CompactionApplied(value) => {
                context_revision = value.target_context_revision;
            }
            SessionFactPayload::TurnClosed(closure) => {
                completed_turns += 1;
                terminal_ordinal += 1;
                let checkpoint_id = Uuid::new_v5(
                    &CHECKPOINT_NAMESPACE,
                    format!("{}\0{}", stream_id, record.frontier().event_id()).as_bytes(),
                );
                if existing_ids.contains(&checkpoint_id) {
                    continue;
                }
                let runtime_observation = if terminal_ordinal == terminal_count {
                    observation
                        .clone()
                        .filter(|value| u64::from(value.turn) == completed_turns)
                } else {
                    None
                };
                store.append(TurnTelemetryCheckpointV1 {
                    telemetry_schema_version: TELEMETRY_SCHEMA_VERSION,
                    record_kind: "turn_telemetry_checkpoint".into(),
                    telemetry_sequence: 0,
                    checkpoint_id,
                    recorded_at: now(),
                    binding: binding.clone(),
                    canonical_completion: Some(CanonicalTurnCompletionV1 {
                        turn_id: closure.turn_id,
                        terminal_event_id: record.frontier().event_id(),
                        outcome: closure.outcome.clone(),
                    }),
                    semantic_source_frontier: Some(TelemetrySourceFrontierV1 {
                        stream_id,
                        sequence: record.frontier().sequence(),
                        event_id: record.frontier().event_id(),
                    }),
                    derived: DerivedTurnTelemetryV1 {
                        completed_turns: Some(completed_turns),
                        requests: Some(requests),
                        tool_calls: Some(tool_calls),
                        context_revision: Some(context_revision),
                    },
                    observed: observed(runtime_observation, metrics.clone(), false),
                })?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn observed(
    observation: Option<RuntimeObservation>,
    metrics: Option<(String, String)>,
    sessionless: bool,
) -> ObservedTurnTelemetryV1 {
    match observation {
        Some(observation) => ObservedTurnTelemetryV1 {
            source: ObservationSourceV1::RuntimeTurnEnd,
            quality: if sessionless {
                ObservationQualityV1::SessionlessCompatibility
            } else {
                ObservationQualityV1::SameGenerationAdvisory
            },
            advisory_turn: Some(observation.turn),
            model: observation.model,
            provider: observation.provider,
            estimated_tokens: u64::try_from(observation.estimated_tokens).ok(),
            context_window: u64::try_from(observation.context_window).ok(),
            input_tokens: Some(observation.input_tokens),
            output_tokens: Some(observation.output_tokens),
            cache_read_tokens: Some(observation.cache_read_tokens),
            cache_creation_tokens: Some(observation.cache_creation_tokens),
            context_class: metrics.as_ref().map(|value| value.0.clone()),
            thinking_level: metrics.map(|value| value.1),
        },
        None => ObservedTurnTelemetryV1 {
            source: ObservationSourceV1::None,
            quality: if sessionless {
                ObservationQualityV1::SessionlessCompatibility
            } else {
                ObservationQualityV1::UnavailableOrLagged
            },
            advisory_turn: None,
            model: None,
            provider: None,
            estimated_tokens: None,
            context_window: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            context_class: None,
            thinking_level: None,
        },
    }
}

fn validate_record(record: &TurnTelemetryCheckpointV1) -> Result<()> {
    if record.telemetry_schema_version != TELEMETRY_SCHEMA_VERSION
        || record.record_kind != "turn_telemetry_checkpoint"
        || record.telemetry_sequence == 0
        || record.checkpoint_id.is_nil()
        || record.binding.session_id.is_empty()
        || record.binding.session_id.len() > 256
    {
        return Err(TelemetryCheckpointError::Invalid(
            "telemetry record contains an invalid required field".into(),
        ));
    }
    DateTime::parse_from_rfc3339(&record.recorded_at)
        .map_err(|_| TelemetryCheckpointError::Invalid("recorded_at is not RFC3339".into()))?;
    match (&record.binding.mode, record.binding.stream_id) {
        (TelemetrySessionModeV1::Semantic, Some(_))
        | (TelemetrySessionModeV1::SessionlessCompatibility, None) => {}
        _ => {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry binding mode and stream identity disagree".into(),
            ));
        }
    }
    match (
        &record.canonical_completion,
        &record.semantic_source_frontier,
    ) {
        (Some(completion), Some(frontier))
            if record.binding.stream_id == Some(frontier.stream_id)
                && completion.terminal_event_id == frontier.event_id
                && frontier.sequence > 0
                && !completion.turn_id.is_nil()
                && !completion.terminal_event_id.is_nil()
                && record.derived.completed_turns.is_some()
                && record.derived.requests.is_some()
                && record.derived.tool_calls.is_some()
                && record.derived.context_revision.is_some() => {}
        (None, None)
            if record.binding.mode == TelemetrySessionModeV1::SessionlessCompatibility
                && record.derived.completed_turns.is_none()
                && record.derived.requests.is_none()
                && record.derived.tool_calls.is_none()
                && record.derived.context_revision.is_none() => {}
        _ => {
            return Err(TelemetryCheckpointError::Invalid(
                "canonical completion, source frontier, and derived counters disagree".into(),
            ));
        }
    }
    for value in [
        record.observed.model.as_deref(),
        record.observed.provider.as_deref(),
        record.observed.context_class.as_deref(),
        record.observed.thinking_level.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if value.len() > 512 {
            return Err(TelemetryCheckpointError::Invalid(
                "observed telemetry text exceeds 512 bytes".into(),
            ));
        }
    }
    Ok(())
}

fn strict_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = T::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

fn validate_session_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(TelemetryCheckpointError::Invalid(
            "session ID is not a safe bounded path component".into(),
        ));
    }
    Ok(())
}

fn read_optional_bounded(path: &Path, maximum: u64) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() || metadata.len() > maximum {
        return Err(TelemetryCheckpointError::Invalid(format!(
            "checkpoint file is not regular or exceeds {maximum} bytes"
        )));
    }
    let mut file = open_regular(path, false)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(TelemetryCheckpointError::Invalid(
            "checkpoint file grew beyond its bound while reading".into(),
        ));
    }
    Ok(Some(bytes))
}

fn secure_append_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(TelemetryCheckpointError::Invalid(
            "telemetry stream is not a regular file".into(),
        ));
    }
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    sync_directory(path.parent().ok_or_else(|| {
        TelemetryCheckpointError::Invalid("telemetry stream has no parent".into())
    })?)?;
    Ok(())
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        TelemetryCheckpointError::Invalid("telemetry cursor has no parent".into())
    })?;
    let temporary = parent.join(format!(".telemetry-tmp-{}", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn open_regular(path: &Path, write: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(!write).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(TelemetryCheckpointError::Invalid(
            "checkpoint path is not a regular file".into(),
        ));
    }
    Ok(file)
}

fn ensure_restricted_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(TelemetryCheckpointError::Invalid(
                "telemetry root is not a directory".into(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new().mode(0o700).create(path)?;
            }
            #[cfg(not(unix))]
            fs::create_dir(path)?;
            if let Some(parent) = path.parent() {
                sync_directory(parent)?;
            }
        }
        Err(error) => return Err(error.into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path)?;
        if metadata.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
pub(crate) fn recovery_campaign_probe(root: &Path) -> Result<()> {
    let store = TelemetryStore::at(root.join("campaign-telemetry"))?;
    let stream = Uuid::from_u128(0x10000000_0000_4000_8000_000000000001);
    let record = TurnTelemetryCheckpointV1 {
        telemetry_schema_version: TELEMETRY_SCHEMA_VERSION,
        record_kind: "turn_telemetry_checkpoint".into(),
        telemetry_sequence: 0,
        checkpoint_id: Uuid::from_u128(0x81000000_0000_4000_8000_000000000001),
        recorded_at: "2026-08-22T00:00:00Z".into(),
        binding: TelemetryBindingV1 {
            session_id: "campaign-telemetry".into(),
            stream_id: Some(stream),
            host_generation: 7,
            mode: TelemetrySessionModeV1::Semantic,
        },
        canonical_completion: Some(CanonicalTurnCompletionV1 {
            turn_id: Uuid::from_u128(0x60000000_0000_4000_8000_000000000001),
            terminal_event_id: Uuid::from_u128(0x20000000_0000_4000_8000_000000000004),
            outcome: TurnOutcome::Completed,
        }),
        semantic_source_frontier: Some(TelemetrySourceFrontierV1 {
            stream_id: stream,
            sequence: 4,
            event_id: Uuid::from_u128(0x20000000_0000_4000_8000_000000000004),
        }),
        derived: DerivedTurnTelemetryV1 {
            completed_turns: Some(1),
            requests: Some(0),
            tool_calls: Some(0),
            context_revision: Some(0),
        },
        observed: ObservedTurnTelemetryV1 {
            source: ObservationSourceV1::None,
            quality: ObservationQualityV1::UnavailableOrLagged,
            advisory_turn: None,
            model: None,
            provider: None,
            estimated_tokens: None,
            context_window: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            context_class: None,
            thinking_level: None,
        },
    };
    if !store.append(record.clone())? || store.append(record)? {
        return Err(TelemetryCheckpointError::Invalid(
            "telemetry restart did not deduplicate".into(),
        ));
    }
    let corrupt = TelemetryStore::at(root.join("campaign-telemetry-corrupt"))?;
    fs::write(
        corrupt.stream_path("campaign-corrupt")?,
        b"{\"telemetry_schema_version\":1",
    )?;
    if corrupt.read_records("campaign-corrupt").is_ok() {
        return Err(TelemetryCheckpointError::Invalid(
            "partial telemetry append was accepted".into(),
        ));
    }
    if store.read_records("campaign-telemetry")?.len() != 1 {
        return Err(TelemetryCheckpointError::Invalid(
            "corrupt telemetry stream contaminated a healthy identity".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, TelemetryStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = TelemetryStore::at(directory.path().join("checkpoints")).unwrap();
        (directory, store)
    }

    fn semantic_record(
        session: &str,
        stream: Uuid,
        generation: u64,
        terminal: Uuid,
        turn: Uuid,
    ) -> TurnTelemetryCheckpointV1 {
        TurnTelemetryCheckpointV1 {
            telemetry_schema_version: 1,
            record_kind: "turn_telemetry_checkpoint".into(),
            telemetry_sequence: 0,
            checkpoint_id: Uuid::new_v5(
                &CHECKPOINT_NAMESPACE,
                format!("{stream}\0{terminal}").as_bytes(),
            ),
            recorded_at: "2026-08-22T12:00:00Z".into(),
            binding: TelemetryBindingV1 {
                session_id: session.into(),
                stream_id: Some(stream),
                host_generation: generation,
                mode: TelemetrySessionModeV1::Semantic,
            },
            canonical_completion: Some(CanonicalTurnCompletionV1 {
                turn_id: turn,
                terminal_event_id: terminal,
                outcome: TurnOutcome::Completed,
            }),
            semantic_source_frontier: Some(TelemetrySourceFrontierV1 {
                stream_id: stream,
                sequence: 9,
                event_id: terminal,
            }),
            derived: DerivedTurnTelemetryV1 {
                completed_turns: Some(1),
                requests: Some(1),
                tool_calls: Some(0),
                context_revision: Some(0),
            },
            observed: observed(None, None, false),
        }
    }

    #[test]
    fn v1_schema_sequence_dedup_and_cursor_are_frozen() {
        let (_directory, store) = store();
        let stream = Uuid::new_v4();
        let record = semantic_record("session-1", stream, 1, Uuid::new_v4(), Uuid::new_v4());
        assert!(store.append(record.clone()).unwrap());
        assert!(!store.append(record).unwrap());
        let records = store.read_records("session-1").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].telemetry_sequence, 1);
        let cursor: TelemetryCursorV1 =
            strict_json(&fs::read(store.cursor_path("session-1").unwrap()).unwrap()).unwrap();
        assert_eq!(cursor.last_checkpoint_id, records[0].checkpoint_id);

        let mut value = serde_json::to_value(&records[0]).unwrap();
        value["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<TurnTelemetryCheckpointV1>(value).is_err());
    }

    #[test]
    fn replacement_identity_is_bound_and_old_terminal_cannot_dedup_new_stream() {
        let (_directory, store) = store();
        let first = semantic_record(
            "first-session",
            Uuid::new_v4(),
            1,
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let second = semantic_record(
            "second-session",
            Uuid::new_v4(),
            2,
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        store.append(first).unwrap();
        store.append(second).unwrap();
        assert_eq!(store.read_records("first-session").unwrap().len(), 1);
        let replacement = store.read_records("second-session").unwrap();
        assert_eq!(replacement[0].binding.host_generation, 2);
        assert_eq!(replacement[0].binding.session_id, "second-session");
    }

    #[test]
    fn canonical_terminal_needs_no_agent_end_and_usage_may_lag() {
        let (_directory, store) = store();
        let record = semantic_record(
            "session-1",
            Uuid::new_v4(),
            1,
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        store.append(record).unwrap();
        let record = store.read_records("session-1").unwrap().pop().unwrap();
        assert!(record.canonical_completion.is_some());
        assert_eq!(record.observed.source, ObservationSourceV1::None);
        assert_eq!(
            record.observed.quality,
            ObservationQualityV1::UnavailableOrLagged
        );
        assert_eq!(record.observed.input_tokens, None);
    }

    #[test]
    fn malformed_and_crash_partial_tails_are_not_skipped() {
        let (_directory, store) = store();
        let record = semantic_record(
            "session-1",
            Uuid::new_v4(),
            1,
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        store.append(record).unwrap();
        let path = store.stream_path("session-1").unwrap();
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"{\"telemetry_schema_version\":1").unwrap();
        file.sync_all().unwrap();
        assert!(matches!(
            store.read_records("session-1"),
            Err(TelemetryCheckpointError::Invalid(message)) if message.contains("incomplete tail")
        ));
    }

    #[test]
    fn crash_after_append_before_cursor_recovers_durable_record() {
        let (_directory, store) = store();
        let mut record = semantic_record(
            "session-1",
            Uuid::new_v4(),
            1,
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        record.telemetry_sequence = 1;
        let mut bytes = serde_json::to_vec(&record).unwrap();
        bytes.push(b'\n');
        secure_append_sync(&store.stream_path("session-1").unwrap(), &bytes).unwrap();
        assert!(!store.cursor_path("session-1").unwrap().exists());
        assert_eq!(
            store.read_records("session-1").unwrap(),
            vec![record.clone()]
        );
        assert!(!store.append(record).unwrap());
        assert!(store.cursor_path("session-1").unwrap().exists());
    }

    #[test]
    fn authority_reconciliation_derives_canonical_terminal_and_counters() {
        const FIXTURE: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/session-semantic-v1/slice-1-closed.authority.jsonl"
        );
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("fixture-session.json");
        fs::copy(
            FIXTURE,
            directory.path().join("fixture-session.authority.jsonl"),
        )
        .unwrap();
        let store = TelemetryStore::at(directory.path().join("telemetry")).unwrap();
        let target = SessionViewTarget {
            binding_id: uuid::Uuid::new_v4(),
            snapshot,
            session_id: "fixture-session".into(),
            stream_id: None,
            generation: 7,
            kind: crate::session_consumers::SessionViewKind::Resume,
        };
        let metrics = crate::features::context::SharedContextMetrics::new();

        reconcile_inner(&store, &target, None, &metrics).unwrap();
        reconcile_inner(&store, &target, None, &metrics).unwrap();

        let records = store.read_records("fixture-session").unwrap();
        assert_eq!(records.len(), 1, "duplicate wakeups must deduplicate");
        let record = &records[0];
        let completion = record.canonical_completion.as_ref().unwrap();
        assert_eq!(
            completion.turn_id,
            Uuid::parse_str("60000000-0000-4000-8000-000000000001").unwrap()
        );
        assert_eq!(
            completion.terminal_event_id,
            Uuid::parse_str("20000000-0000-4000-8000-000000000004").unwrap()
        );
        assert_eq!(record.binding.host_generation, 7);
        assert_eq!(
            record.binding.stream_id,
            Some(Uuid::parse_str("10000000-0000-4000-8000-000000000001").unwrap())
        );
        assert_eq!(record.derived.completed_turns, Some(1));
        assert_eq!(record.derived.requests, Some(0));
        assert_eq!(record.derived.tool_calls, Some(0));
        assert_eq!(record.observed.source, ObservationSourceV1::None);
    }

    #[test]
    fn legacy_jsonl_remains_readable_and_labeled_compatibility() {
        let (_directory, store) = store();
        let legacy = LegacyTurnCheckpoint {
            timestamp_unix_ms: 1,
            session_id: "session-1".into(),
            turn: 2,
            model: None,
            provider: None,
            estimated_tokens: 3,
            context_window: 4,
            actual_input_tokens: 5,
            actual_output_tokens: 6,
            intent: LegacyIntentSnapshot {
                current_task: None,
                lifecycle_phase: "unknown".into(),
                files_read_count: 0,
                files_modified_count: 0,
                stats_turns: 2,
                stats_tool_calls: 1,
            },
            metrics: LegacyMetricsSnapshot {
                tokens_used: 3,
                context_window: 4,
                context_class: "unknown".into(),
                thinking_level: "unknown".into(),
            },
        };
        let mut bytes = serde_json::to_vec(&legacy).unwrap();
        bytes.push(b'\n');
        secure_append_sync(&store.legacy_path("session-1").unwrap(), &bytes).unwrap();
        let compatible = store.read_compatible_last("session-1").unwrap().unwrap();
        assert_eq!(
            compatible.compatibility_label(),
            "legacy_checkpoint_compatibility"
        );
        assert!(matches!(compatible, CompatibleCheckpoint::Legacy(value) if value.turn == 2));
    }

    #[test]
    fn old_event_fence_and_second_prompt_use_current_generation() {
        let first = SessionViewTarget {
            binding_id: uuid::Uuid::new_v4(),
            snapshot: PathBuf::from("first.json"),
            session_id: "first".into(),
            stream_id: Some(Uuid::new_v4()),
            generation: 1,
            kind: crate::session_consumers::SessionViewKind::New,
        };
        let second = SessionViewTarget {
            binding_id: uuid::Uuid::new_v4(),
            snapshot: PathBuf::from("second.json"),
            session_id: "second".into(),
            stream_id: Some(Uuid::new_v4()),
            generation: 2,
            kind: crate::session_consumers::SessionViewKind::New,
        };
        let old_start = BindingFence::from(&first);
        assert_ne!(old_start, BindingFence::from(&second));
        let second_prompt_start = BindingFence::from(&second);
        assert_eq!(second_prompt_start, BindingFence::from(&second));
    }

    #[test]
    fn sessionless_records_are_explicitly_nonsemantic() {
        let (_directory, store) = store();
        let observation = RuntimeObservation {
            turn: 1,
            model: Some("model".into()),
            provider: Some("provider".into()),
            estimated_tokens: 10,
            context_window: 20,
            input_tokens: 3,
            output_tokens: 4,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };
        let record = TurnTelemetryCheckpointV1 {
            telemetry_schema_version: 1,
            record_kind: "turn_telemetry_checkpoint".into(),
            telemetry_sequence: 0,
            checkpoint_id: Uuid::new_v5(
                &CHECKPOINT_NAMESPACE,
                format!("sessionless\0{}\0{}\0{}", "sessionless", 0, 1).as_bytes(),
            ),
            recorded_at: now(),
            binding: TelemetryBindingV1 {
                session_id: "sessionless".into(),
                stream_id: None,
                host_generation: 0,
                mode: TelemetrySessionModeV1::SessionlessCompatibility,
            },
            canonical_completion: None,
            semantic_source_frontier: None,
            derived: DerivedTurnTelemetryV1 {
                completed_turns: None,
                requests: None,
                tool_calls: None,
                context_revision: None,
            },
            observed: observed(Some(observation), None, true),
        };
        store.append(record).unwrap();
        let record = store.read_records("sessionless").unwrap().pop().unwrap();
        assert_eq!(
            record.binding.mode,
            TelemetrySessionModeV1::SessionlessCompatibility
        );
        assert!(record.canonical_completion.is_none());
        assert_eq!(
            record.observed.quality,
            ObservationQualityV1::SessionlessCompatibility
        );
    }
}
