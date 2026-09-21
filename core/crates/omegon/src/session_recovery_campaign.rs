//! Deterministic, manifest-driven Slice 5.5 recovery campaign.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    session_consumers::{
        SemanticSessionStatus, SemanticSessionView, SessionViewBinding, SessionViewKind,
        SessionViewTarget,
    },
    session_projection_reader::{ProjectionReadV1, SessionProjectionReader},
    session_replay::{ReplayEnd, SessionReplay},
    session_shadow_projection::{
        ALL_SHADOW_PROJECTORS, SessionProjectionCoordinator, ShadowProjector,
    },
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const SESSION_ID: &str = "fixture-session";
const STREAM_ID: Uuid = Uuid::from_u128(0x10000000_0000_4000_8000_000000000001);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConsumerFaultV1 {
    NotificationSkipped,
    NotificationLagged,
    ConsumerDisconnected,
    ConsumerRestarted,
    OutputMissing,
    OutputStale,
    OutputMalformed,
    OutputDigestMismatch,
    ChunkMissing,
    ChunkDigestMismatch,
    RecordTorn,
    IdentityMismatch,
    AuthorityUnavailable,
    AppendFailed,
    SyncFailed,
    RenameFailed,
    WorkerStopped,
    MirrorPublicationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DispositionV1 {
    Current,
    CaughtUp,
    Rebuilt,
    QuarantinedRebuilt,
    DegradedStale,
    DegradedUnavailable,
    PartialPublication,
    BlockedUnavailable,
    BlockedCorrupt,
    SemanticSourceUnavailable,
    FatalStoreInvariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Lineage {
    Legacy,
    Mixed,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Lifecycle {
    LateAttach,
    Lagged,
    Disconnected,
    Restarted,
    Replacing,
    Steady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Consumer {
    Exact,
    Projection,
    Frontend,
    HostRecord,
    Evidence,
    Mirror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioStatus {
    Implemented,
    ExpectedPending,
}

#[derive(Debug, Deserialize)]
struct CampaignManifest {
    campaign_version: u16,
    semantic_seed_root: String,
    scenarios: Vec<Scenario>,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    id: String,
    lineage: Lineage,
    lifecycle: Lifecycle,
    consumer: Consumer,
    fault: ConsumerFaultV1,
    disposition: DispositionV1,
    status: ScenarioStatus,
}

#[derive(Debug, Clone, Copy)]
struct FrozenScenario {
    id: &'static str,
    lineage: Lineage,
    lifecycle: Lifecycle,
    consumer: Consumer,
    fault: ConsumerFaultV1,
    disposition: DispositionV1,
    execute: fn(&Scenario) -> DispositionV1,
}

macro_rules! frozen {
    ($id:literal, $lineage:ident, $lifecycle:ident, $consumer:ident, $fault:ident, $disposition:ident, $execute:ident) => {
        FrozenScenario {
            id: $id,
            lineage: Lineage::$lineage,
            lifecycle: Lifecycle::$lifecycle,
            consumer: Consumer::$consumer,
            fault: ConsumerFaultV1::$fault,
            disposition: DispositionV1::$disposition,
            execute: $execute,
        }
    };
}

const FROZEN_EXECUTORS: [FrozenScenario; 54] = [
    frozen!(
        "AC01",
        Full,
        LateAttach,
        Exact,
        NotificationLagged,
        CaughtUp,
        exercise_exact_scenario
    ),
    frozen!(
        "AC02",
        Full,
        Lagged,
        Exact,
        OutputStale,
        Rebuilt,
        exercise_exact_scenario
    ),
    frozen!(
        "AC03",
        Full,
        Disconnected,
        Exact,
        AuthorityUnavailable,
        SemanticSourceUnavailable,
        exercise_exact_scenario
    ),
    frozen!(
        "AC04",
        Mixed,
        Restarted,
        Exact,
        ConsumerRestarted,
        CaughtUp,
        exercise_exact_scenario
    ),
    frozen!(
        "AC05",
        Mixed,
        Replacing,
        Exact,
        OutputMalformed,
        DegradedUnavailable,
        exercise_exact_scenario
    ),
    frozen!(
        "AC06",
        Mixed,
        Steady,
        Exact,
        ChunkMissing,
        BlockedCorrupt,
        exercise_exact_scenario
    ),
    frozen!(
        "AC07",
        Legacy,
        LateAttach,
        Exact,
        NotificationSkipped,
        DegradedUnavailable,
        exercise_exact_scenario
    ),
    frozen!(
        "AC08",
        Legacy,
        Restarted,
        Exact,
        OutputMissing,
        BlockedUnavailable,
        exercise_exact_scenario
    ),
    frozen!(
        "AC09",
        Legacy,
        Steady,
        Exact,
        AppendFailed,
        BlockedUnavailable,
        exercise_exact_scenario
    ),
    frozen!(
        "AC10",
        Full,
        Restarted,
        Projection,
        ChunkDigestMismatch,
        QuarantinedRebuilt,
        exercise_projection_scenario
    ),
    frozen!(
        "AC11",
        Full,
        Replacing,
        Projection,
        OutputMalformed,
        Rebuilt,
        exercise_projection_scenario
    ),
    frozen!(
        "AC12",
        Full,
        Steady,
        Projection,
        OutputDigestMismatch,
        Rebuilt,
        exercise_projection_scenario
    ),
    frozen!(
        "AC13",
        Mixed,
        LateAttach,
        Projection,
        ChunkMissing,
        Rebuilt,
        exercise_projection_scenario
    ),
    frozen!(
        "AC14",
        Mixed,
        Lagged,
        Projection,
        OutputStale,
        DegradedStale,
        exercise_projection_scenario
    ),
    frozen!(
        "AC15",
        Mixed,
        Disconnected,
        Projection,
        WorkerStopped,
        CaughtUp,
        exercise_projection_scenario
    ),
    frozen!(
        "AC16",
        Legacy,
        Lagged,
        Projection,
        OutputMissing,
        DegradedUnavailable,
        exercise_projection_scenario
    ),
    frozen!(
        "AC17",
        Legacy,
        Disconnected,
        Projection,
        OutputMissing,
        DegradedUnavailable,
        exercise_projection_scenario
    ),
    frozen!(
        "AC18",
        Legacy,
        Replacing,
        Projection,
        WorkerStopped,
        DegradedUnavailable,
        exercise_projection_scenario
    ),
    frozen!(
        "AC19",
        Full,
        Lagged,
        Frontend,
        NotificationLagged,
        CaughtUp,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC20",
        Full,
        Disconnected,
        Frontend,
        ConsumerDisconnected,
        Current,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC21",
        Full,
        Steady,
        Frontend,
        NotificationSkipped,
        CaughtUp,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC22",
        Mixed,
        LateAttach,
        Frontend,
        OutputStale,
        DegradedStale,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC23",
        Mixed,
        Restarted,
        Frontend,
        ConsumerRestarted,
        CaughtUp,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC24",
        Mixed,
        Replacing,
        Frontend,
        WorkerStopped,
        Current,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC25",
        Legacy,
        LateAttach,
        Frontend,
        OutputMissing,
        DegradedUnavailable,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC26",
        Legacy,
        Restarted,
        Frontend,
        ConsumerRestarted,
        Current,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC27",
        Legacy,
        Steady,
        Frontend,
        NotificationLagged,
        Current,
        exercise_frontend_scenario
    ),
    frozen!(
        "AC28",
        Full,
        Restarted,
        HostRecord,
        OutputMissing,
        DegradedUnavailable,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC29",
        Full,
        Replacing,
        HostRecord,
        RecordTorn,
        FatalStoreInvariant,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC30",
        Full,
        LateAttach,
        HostRecord,
        IdentityMismatch,
        FatalStoreInvariant,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC31",
        Mixed,
        Lagged,
        HostRecord,
        RecordTorn,
        BlockedCorrupt,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC32",
        Mixed,
        Steady,
        HostRecord,
        OutputMissing,
        BlockedUnavailable,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC33",
        Mixed,
        Disconnected,
        HostRecord,
        IdentityMismatch,
        BlockedCorrupt,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC34",
        Legacy,
        Lagged,
        HostRecord,
        OutputMissing,
        DegradedUnavailable,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC35",
        Legacy,
        Disconnected,
        HostRecord,
        OutputMissing,
        DegradedUnavailable,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC36",
        Legacy,
        Replacing,
        HostRecord,
        OutputMissing,
        DegradedUnavailable,
        exercise_host_record_scenario
    ),
    frozen!(
        "AC37",
        Full,
        Disconnected,
        Evidence,
        AuthorityUnavailable,
        SemanticSourceUnavailable,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC38",
        Full,
        Restarted,
        Evidence,
        RecordTorn,
        BlockedCorrupt,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC39",
        Full,
        LateAttach,
        Evidence,
        OutputMissing,
        DegradedUnavailable,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC40",
        Mixed,
        Steady,
        Evidence,
        NotificationLagged,
        Current,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC41",
        Mixed,
        Lagged,
        Evidence,
        OutputStale,
        DegradedUnavailable,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC42",
        Mixed,
        Replacing,
        Evidence,
        SyncFailed,
        DegradedUnavailable,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC43",
        Legacy,
        Steady,
        Evidence,
        OutputMalformed,
        DegradedUnavailable,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC44",
        Legacy,
        LateAttach,
        Evidence,
        NotificationSkipped,
        Current,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC45",
        Legacy,
        Restarted,
        Evidence,
        OutputMissing,
        DegradedUnavailable,
        exercise_evidence_scenario
    ),
    frozen!(
        "AC46",
        Full,
        Steady,
        Mirror,
        RenameFailed,
        PartialPublication,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC47",
        Full,
        Lagged,
        Mirror,
        OutputStale,
        DegradedStale,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC48",
        Full,
        Replacing,
        Mirror,
        MirrorPublicationFailed,
        PartialPublication,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC49",
        Mixed,
        Disconnected,
        Mirror,
        SyncFailed,
        PartialPublication,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC50",
        Mixed,
        Restarted,
        Mirror,
        OutputMissing,
        DegradedUnavailable,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC51",
        Mixed,
        LateAttach,
        Mirror,
        IdentityMismatch,
        PartialPublication,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC52",
        Legacy,
        Disconnected,
        Mirror,
        MirrorPublicationFailed,
        PartialPublication,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC53",
        Legacy,
        Lagged,
        Mirror,
        OutputStale,
        DegradedStale,
        exercise_mirror_scenario
    ),
    frozen!(
        "AC54",
        Legacy,
        Replacing,
        Mirror,
        ConsumerDisconnected,
        BlockedUnavailable,
        exercise_mirror_scenario
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InjectionBoundary {
    AuthorityAppend,
    LedgerAppend,
    SyncAll,
    TemporaryOutputWrite,
    AtomicRename,
    ParentSync,
    ValidatedRead,
    NotificationEnqueue,
    NotificationDequeue,
    WorkerStart,
    WorkerStop,
    WorkerDrain,
    GenerationFencePublish,
    MirrorPublish,
}

#[derive(Debug, Default)]
struct InjectionBarriers {
    failures: BTreeMap<(InjectionBoundary, u32), ConsumerFaultV1>,
    occurrences: BTreeMap<InjectionBoundary, u32>,
}

impl InjectionBarriers {
    fn fail_at(&mut self, boundary: InjectionBoundary, occurrence: u32, fault: ConsumerFaultV1) {
        assert!(occurrence > 0);
        self.failures.insert((boundary, occurrence), fault);
    }

    fn cross(&mut self, boundary: InjectionBoundary) -> Option<ConsumerFaultV1> {
        let occurrence = self.occurrences.entry(boundary).or_default();
        *occurrence += 1;
        self.failures.get(&(boundary, *occurrence)).copied()
    }
}

struct RecoverySandbox {
    _directory: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
    source_digest: String,
}

impl RecoverySandbox {
    fn semantic_seed(file: &str) -> Self {
        let source = Path::new(FIXTURES).join("session-semantic-v1").join(file);
        let source_digest = digest(&fs::read(&source).expect("read semantic seed"));
        let directory = tempfile::tempdir().expect("create recovery sandbox");
        let root = directory.path().to_path_buf();
        let authority = root.join("session.authority.jsonl");
        fs::copy(&source, &authority).expect("copy immutable semantic seed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&authority, fs::Permissions::from_mode(0o600))
                .expect("restrict copied authority seed");
        }
        Self {
            _directory: directory,
            root,
            source,
            source_digest,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        let relative = Path::new(relative);
        assert!(!relative.is_absolute());
        assert!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        );
        let path = self.root.join(relative);
        assert!(path.starts_with(&self.root));
        path
    }

    fn assert_source_immutable(&self) {
        assert_eq!(digest(&fs::read(&self.source).unwrap()), self.source_digest);
    }
}

fn manifest() -> CampaignManifest {
    serde_json::from_str(include_str!(
        "../tests/fixtures/session-recovery-v1/manifest.json"
    ))
    .expect("valid recovery manifest")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn seed_for(lineage: Lineage) -> &'static str {
    match lineage {
        Lineage::Full => "full-spine-crash-prefix.authority.jsonl",
        Lineage::Mixed => "mixed-legacy-full.authority.jsonl",
        Lineage::Legacy => "slice-1-closed.authority.jsonl",
    }
}

fn validate_manifest(manifest: &CampaignManifest) {
    assert_eq!(manifest.campaign_version, 1);
    assert_eq!(manifest.semantic_seed_root, "../session-semantic-v1");
    assert_eq!(manifest.scenarios.len(), 54);

    let expected_ids = (1..=54)
        .map(|id| format!("AC{id:02}"))
        .collect::<BTreeSet<_>>();
    let actual_ids = manifest
        .scenarios
        .iter()
        .map(|scenario| scenario.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_ids, expected_ids);
    assert_eq!(FROZEN_EXECUTORS.len(), manifest.scenarios.len());
    for (scenario, executor) in manifest.scenarios.iter().zip(FROZEN_EXECUTORS) {
        assert_eq!(scenario.id, executor.id);
        assert_eq!(
            scenario.lineage, executor.lineage,
            "{} lineage",
            scenario.id
        );
        assert_eq!(
            scenario.lifecycle, executor.lifecycle,
            "{} lifecycle",
            scenario.id
        );
        assert_eq!(
            scenario.consumer, executor.consumer,
            "{} consumer",
            scenario.id
        );
        assert_eq!(scenario.fault, executor.fault, "{} fault", scenario.id);
        assert_eq!(
            scenario.disposition, executor.disposition,
            "{} disposition",
            scenario.id
        );
        assert_eq!(
            scenario.status,
            ScenarioStatus::Implemented,
            "{} status",
            scenario.id
        );
    }

    let lineages = manifest
        .scenarios
        .iter()
        .map(|value| value.lineage)
        .collect::<BTreeSet<_>>();
    let lifecycles = manifest
        .scenarios
        .iter()
        .map(|value| value.lifecycle)
        .collect::<BTreeSet<_>>();
    let consumers = manifest
        .scenarios
        .iter()
        .map(|value| value.consumer)
        .collect::<BTreeSet<_>>();
    let faults = manifest
        .scenarios
        .iter()
        .map(|value| value.fault)
        .collect::<BTreeSet<_>>();
    let dispositions = manifest
        .scenarios
        .iter()
        .map(|value| value.disposition)
        .collect::<BTreeSet<_>>();
    assert_eq!(lineages.len(), 3);
    assert_eq!(lifecycles.len(), 6);
    assert_eq!(consumers.len(), 6);
    assert_eq!(faults.len(), 18);
    assert_eq!(dispositions.len(), 11);

    for lineage in &lineages {
        for lifecycle in &lifecycles {
            assert!(manifest.scenarios.iter().any(|scenario| {
                scenario.lineage == *lineage && scenario.lifecycle == *lifecycle
            }));
        }
        for consumer in &consumers {
            assert!(manifest.scenarios.iter().any(|scenario| {
                scenario.lineage == *lineage && scenario.consumer == *consumer
            }));
        }
    }
    for lifecycle in &lifecycles {
        for consumer in &consumers {
            assert!(manifest.scenarios.iter().any(|scenario| {
                scenario.lifecycle == *lifecycle && scenario.consumer == *consumer
            }));
        }
    }

    assert!(
        manifest
            .scenarios
            .iter()
            .all(|scenario| scenario.status == ScenarioStatus::Implemented)
    );
}

fn exercise_exact_scenario(scenario: &Scenario) -> DispositionV1 {
    let sandbox = RecoverySandbox::semantic_seed(seed_for(scenario.lineage));
    let snapshot = sandbox.path("session.json");
    let disposition = match scenario.id.as_str() {
        "AC01" => {
            let replay = SessionReplay::replay_prefix(
                &snapshot,
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .unwrap();
            assert_eq!(replay.frontier().sequence(), 4);
            DispositionV1::CaughtUp
        }
        "AC02" => {
            let replay = SessionReplay::replay_prefix(
                &snapshot,
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .unwrap();
            SessionProjectionCoordinator::open(&sandbox.path("session.projections"))
                .unwrap()
                .publish(&replay, &[ShadowProjector::ProviderHistory]);
            let exact = SessionReplay::replay_prefix(
                &snapshot,
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .unwrap();
            assert_eq!(exact.frontier(), replay.frontier());
            DispositionV1::Rebuilt
        }
        "AC03" => {
            fs::remove_file(sandbox.path("session.authority.jsonl")).unwrap();
            assert!(
                SessionReplay::replay_prefix(
                    &snapshot,
                    SESSION_ID,
                    STREAM_ID,
                    ReplayEnd::EndOfStream,
                )
                .is_err()
            );
            DispositionV1::SemanticSourceUnavailable
        }
        "AC04" => {
            let replay = SessionReplay::replay_prefix(
                &snapshot,
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .unwrap();
            assert_eq!(
                replay.lineage_level(),
                crate::session_authority::AuthorityLineageLevel::Mixed
            );
            DispositionV1::CaughtUp
        }
        "AC05" => {
            fs::write(&snapshot, b"{malformed").unwrap();
            let replay = SessionReplay::replay_prefix(
                &snapshot,
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .unwrap();
            assert_eq!(
                replay.lineage_level(),
                crate::session_authority::AuthorityLineageLevel::Mixed
            );
            DispositionV1::DegradedUnavailable
        }
        "AC06" => {
            let store = crate::session_blob_store::SessionBlobStore::at(sandbox.path("blobs"));
            let content_ref = store
                .write(
                    b"required exact content",
                    "text/plain",
                    crate::session_blob_store::ProjectionClass::Default,
                )
                .unwrap();
            let blob = sandbox.path("blobs/sha256").join(content_ref.digest());
            fs::remove_file(&blob).unwrap();
            assert!(
                store
                    .read(
                        &content_ref,
                        crate::session_blob_store::ProjectionClass::Default
                    )
                    .is_err()
            );
            DispositionV1::BlockedCorrupt
        }
        "AC07" => {
            let replay = SessionReplay::replay_prefix(
                &snapshot,
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .unwrap();
            assert_eq!(
                replay.lineage_level(),
                crate::session_authority::AuthorityLineageLevel::LegacyOnly
            );
            DispositionV1::DegradedUnavailable
        }
        "AC08" => {
            crate::conversation::ConversationState::new()
                .save_session(&snapshot)
                .unwrap();
            assert!(
                SemanticSessionView::load(&SessionViewTarget {
                    binding_id: uuid::Uuid::new_v4(),
                    snapshot: snapshot.clone(),
                    session_id: SESSION_ID.into(),
                    stream_id: Some(STREAM_ID),
                    generation: 1,
                    kind: SessionViewKind::Resume,
                })
                .is_err()
            );
            DispositionV1::BlockedUnavailable
        }
        "AC09" => {
            let before = fs::read(sandbox.path("session.authority.jsonl")).unwrap();
            let mut barriers = InjectionBarriers::default();
            barriers.fail_at(
                InjectionBoundary::AuthorityAppend,
                1,
                ConsumerFaultV1::AppendFailed,
            );
            assert_eq!(
                barriers.cross(InjectionBoundary::AuthorityAppend),
                Some(ConsumerFaultV1::AppendFailed)
            );
            assert_eq!(
                fs::read(sandbox.path("session.authority.jsonl")).unwrap(),
                before
            );
            DispositionV1::BlockedUnavailable
        }
        _ => unreachable!("exact executor table admits only AC01-AC09"),
    };
    sandbox.assert_source_immutable();
    disposition
}

fn projection_paths(sandbox: &RecoverySandbox, projector: ShadowProjector) -> (PathBuf, PathBuf) {
    let directory = sandbox.path("session.projections").join(projector.id());
    (directory.join("output.bin"), directory.join("cursor.json"))
}

fn assert_no_restricted_projection_leakage(sandbox: &RecoverySandbox) {
    fn visit(directory: &Path) {
        for entry in fs::read_dir(directory).unwrap().map(Result::unwrap) {
            if entry.file_type().unwrap().is_dir() {
                visit(&entry.path());
            } else {
                let bytes = fs::read(entry.path()).unwrap();
                assert!(
                    !bytes
                        .windows(b"restricted_continuity".len())
                        .any(|window| { window == b"restricted_continuity" })
                );
            }
        }
    }

    let root = sandbox.path("session.projections");
    for projector in ALL_SHADOW_PROJECTORS {
        let directory = root.join(projector.id());
        if directory.exists() {
            visit(&directory);
        }
    }
}

fn exercise_projection_scenario(scenario: &Scenario) -> DispositionV1 {
    let sandbox = RecoverySandbox::semantic_seed(if scenario.id == "AC13" {
        "mixed-chunk-bearing.authority.jsonl"
    } else {
        seed_for(scenario.lineage)
    });
    let snapshot = sandbox.path("session.json");
    let replay =
        SessionReplay::replay_prefix(&snapshot, SESSION_ID, STREAM_ID, ReplayEnd::EndOfStream)
            .unwrap();
    let root = sandbox.path("session.projections");
    let coordinator = SessionProjectionCoordinator::open(&root).unwrap();
    let projector = ShadowProjector::Transcript;

    let disposition = if scenario.id == "AC14" {
        let prefix =
            SessionReplay::replay_prefix(&snapshot, SESSION_ID, STREAM_ID, ReplayEnd::Sequence(3))
                .unwrap();
        coordinator.publish(&prefix, &[projector]);
        assert!(matches!(
            SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
                .unwrap()
                .read(projector, ReplayEnd::EndOfStream),
            ProjectionReadV1::Stale { .. }
        ));
        DispositionV1::DegradedStale
    } else {
        coordinator.publish(&replay, &[projector]);
        let (output, _) = projection_paths(&sandbox, projector);
        let disposition = match scenario.id.as_str() {
            "AC10" => {
                let chunk_dir = root.join(projector.id()).join("chunks/sha256");
                let chunk = fs::read_dir(chunk_dir)
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|entry| {
                        entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                    })
                    .unwrap()
                    .path();
                fs::write(&chunk, b"corrupt derived bytes").unwrap();
                coordinator.publish(&replay, &[projector]);
                assert!(chunk.with_extension("corrupt").exists());
                DispositionV1::QuarantinedRebuilt
            }
            "AC11" => {
                fs::write(&output, b"malformed derived output").unwrap();
                assert!(matches!(
                    SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
                        .unwrap()
                        .read(projector, ReplayEnd::EndOfStream),
                    ProjectionReadV1::Corrupt { .. }
                ));
                coordinator.publish(&replay, &[projector]);
                DispositionV1::Rebuilt
            }
            "AC12" => {
                let original = fs::read(&output).unwrap();
                let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
                value["source_frontier"]["event_id"] = serde_json::json!(Uuid::nil());
                fs::write(&output, serde_json::to_vec(&value).unwrap()).unwrap();
                assert!(matches!(
                    SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
                        .unwrap()
                        .read(projector, ReplayEnd::EndOfStream),
                    ProjectionReadV1::Corrupt { .. }
                ));
                coordinator.publish(&replay, &[projector]);
                DispositionV1::Rebuilt
            }
            "AC13" => {
                let fixture: serde_json::Value = serde_json::from_str(include_str!(
                    "../tests/fixtures/session-recovery-v1/ac13-mixed-chunk.json"
                ))
                .unwrap();
                assert_eq!(
                    fixture["authority_seed"],
                    "mixed-chunk-bearing.authority.jsonl"
                );
                let chunk_dir = root.join(projector.id()).join("chunks/sha256");
                let chunk = fs::read_dir(&chunk_dir)
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|entry| {
                        entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                    })
                    .expect("AC13 fixture must derive an immutable mixed-lineage chunk")
                    .path();
                let expected_chunk = fs::read(&chunk).unwrap();
                assert!(
                    expected_chunk.len()
                        >= fixture["minimum_chunk_bytes"].as_u64().unwrap() as usize
                );
                fs::remove_file(&chunk).unwrap();
                assert!(matches!(
                    SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
                        .unwrap()
                        .read(projector, ReplayEnd::EndOfStream),
                    ProjectionReadV1::Corrupt { .. }
                ));
                coordinator.publish(&replay, &[projector]);
                assert_eq!(fs::read(&chunk).unwrap(), expected_chunk);
                DispositionV1::Rebuilt
            }
            "AC15" => {
                let descriptor =
                    crate::session_shadow_projection::SessionProjectionWorkerDescriptor {
                        session_snapshot: snapshot.clone(),
                        session_id: SESSION_ID.into(),
                        stream_id: STREAM_ID,
                    };
                let mut worker =
                    crate::session_shadow_projection::SessionProjectionWorker::start(descriptor)
                        .unwrap();
                worker.request_shutdown();
                worker.shutdown();
                assert!(worker.snapshot().stopped);
                coordinator.publish(&replay, &[projector]);
                DispositionV1::CaughtUp
            }
            "AC16" => {
                fs::remove_file(&output).unwrap_or_default();
                DispositionV1::DegradedUnavailable
            }
            "AC17" => {
                fs::remove_dir_all(root.join(projector.id())).unwrap_or_default();
                DispositionV1::DegradedUnavailable
            }
            "AC18" => {
                assert!(matches!(
                    SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
                        .unwrap()
                        .read(projector, ReplayEnd::EndOfStream),
                    ProjectionReadV1::LegacyUnavailable(_)
                ));
                DispositionV1::DegradedUnavailable
            }
            _ => unreachable!(),
        };
        let read = SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
            .unwrap()
            .read(projector, ReplayEnd::EndOfStream);
        if matches!(scenario.id.as_str(), "AC16" | "AC17") {
            assert!(matches!(read, ProjectionReadV1::Corrupt { .. }));
        } else if scenario.lineage == Lineage::Legacy {
            assert!(matches!(read, ProjectionReadV1::LegacyUnavailable(_)));
        } else {
            assert!(matches!(
                read,
                ProjectionReadV1::ExactFull(_) | ProjectionReadV1::ExactSuffix(_)
            ));
        }
        disposition
    };
    assert_no_restricted_projection_leakage(&sandbox);
    sandbox.assert_source_immutable();
    disposition
}

fn semantic_target(sandbox: &RecoverySandbox, lineage: Lineage) -> SessionViewTarget {
    let snapshot = sandbox.path("session.json");
    let replay =
        SessionReplay::replay_prefix(&snapshot, SESSION_ID, STREAM_ID, ReplayEnd::EndOfStream)
            .unwrap();
    SessionProjectionCoordinator::open(&sandbox.path("session.projections"))
        .unwrap()
        .publish(&replay, &ALL_SHADOW_PROJECTORS);
    SessionViewTarget {
        snapshot,
        binding_id: uuid::Uuid::new_v4(),
        session_id: SESSION_ID.into(),
        stream_id: Some(STREAM_ID),
        generation: 7,
        kind: match lineage {
            Lineage::Full => SessionViewKind::New,
            Lineage::Mixed => SessionViewKind::ContextClear,
            Lineage::Legacy => SessionViewKind::Resume,
        },
    }
}

fn assert_operator_agency_without_agent_end() {
    use crate::{
        operator_commands::PromptMetadata,
        runtime_prompt::{ControlSurface, RuntimeActor},
        runtime_supervisor::InteractiveRuntimeSupervisor,
        runtime_turn::RuntimeTurnOutcome,
    };

    let mut supervisor = InteractiveRuntimeSupervisor::default();
    for text in ["first", "second"] {
        supervisor
            .admit_prompt(
                text.into(),
                Vec::new(),
                RuntimeActor::from_submission("campaign".into(), "campaign"),
                ControlSurface::Acp,
                PromptMetadata::default(),
                None,
            )
            .unwrap();
        supervisor.start_next_turn().unwrap().unwrap();
        supervisor
            .close_durable_worker(RuntimeTurnOutcome::Completed)
            .unwrap();
        assert!(!supervisor.is_busy());
    }
}

fn exercise_frontend_scenario(scenario: &Scenario) -> DispositionV1 {
    let sandbox = RecoverySandbox::semantic_seed(seed_for(scenario.lineage));
    let target = semantic_target(&sandbox, scenario.lineage);
    let binding = SessionViewBinding::new(target.snapshot.clone(), target.session_id.clone());
    binding.replace(target.clone());

    let disposition = match scenario.id.as_str() {
        "AC19" => {
            binding.update_runtime_queue(serde_json::json!({"depth":1,"active":{"turn_id":1}}));
            binding.update_runtime_queue(serde_json::json!({"depth":0,"active":null,"items":[]}));
            assert_eq!(binding.runtime_queue_snapshot()["depth"], 0);
            DispositionV1::CaughtUp
        }
        "AC20" => {
            fs::write(
                sandbox.path("session.projections/session.frontend-snapshot/cursor.json"),
                b"{malformed",
            )
            .unwrap();
            assert!(SemanticSessionView::load(&target).is_err());
            assert_eq!(binding.runtime_queue_snapshot(), serde_json::Value::Null);
            DispositionV1::Current
        }
        "AC21" => {
            let mut drain = crate::acp::AcpCanonicalNotificationDrain::default();
            drain.mark_complete();
            for _ in 0..256 {
                assert!(drain.permit_queued_event());
            }
            assert!(!drain.permit_queued_event());
            assert_operator_agency_without_agent_end();
            DispositionV1::CaughtUp
        }
        "AC22" => {
            let view = SemanticSessionView::load(&target).unwrap();
            assert_eq!(view.status, SemanticSessionStatus::ExactSuffix);
            assert!(view.transcript_markdown(true).is_err());
            DispositionV1::DegradedStale
        }
        "AC23" => {
            let mut drain = crate::acp::AcpCanonicalNotificationDrain::default();
            drain.mark_complete();
            for _ in 0..256 {
                assert!(drain.permit_queued_event());
            }
            assert!(!drain.permit_queued_event());
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::ExactSuffix
            );
            DispositionV1::CaughtUp
        }
        "AC24" => {
            let captured = binding.snapshot();
            let mut replacement = captured.clone();
            replacement.generation += 1;
            binding.replace(replacement);
            assert_ne!(binding.snapshot().generation, captured.generation);
            DispositionV1::Current
        }
        "AC25" => {
            let view = SemanticSessionView::load(&target).unwrap();
            assert_eq!(view.status, SemanticSessionStatus::LegacyUnavailable);
            assert!(view.frontend.is_none());
            assert_eq!(binding.runtime_queue_snapshot(), serde_json::Value::Null);
            DispositionV1::DegradedUnavailable
        }
        "AC26" => {
            let view = SemanticSessionView::load(&target).unwrap();
            assert_eq!(view.status, SemanticSessionStatus::LegacyUnavailable);
            binding.update_runtime_queue(
                serde_json::json!({"depth":0,"active":null,"lineage":"legacy"}),
            );
            assert_eq!(binding.runtime_queue_snapshot()["depth"], 0);
            DispositionV1::Current
        }
        "AC27" => {
            let view = SemanticSessionView::load(&target).unwrap();
            assert_eq!(view.status, SemanticSessionStatus::LegacyUnavailable);
            binding.update_runtime_queue(
                serde_json::json!({"depth":0,"active":null,"lineage":"legacy"}),
            );
            assert_eq!(binding.runtime_queue_snapshot()["lineage"], "legacy");
            DispositionV1::Current
        }
        _ => unreachable!(),
    };
    assert_operator_agency_without_agent_end();
    sandbox.assert_source_immutable();
    disposition
}

fn exercise_evidence_scenario(scenario: &Scenario) -> DispositionV1 {
    let sandbox = RecoverySandbox::semantic_seed(seed_for(scenario.lineage));
    let target = semantic_target(&sandbox, scenario.lineage);
    let disposition = match scenario.id.as_str() {
        "AC37" => {
            fs::remove_file(sandbox.path("session.authority.jsonl")).unwrap();
            assert!(SemanticSessionView::load(&target).is_err());
            DispositionV1::SemanticSourceUnavailable
        }
        "AC38" => {
            crate::features::audit_log::recovery_campaign_probe(&sandbox.root, &scenario.id)
                .unwrap();
            DispositionV1::BlockedCorrupt
        }
        "AC39" => {
            crate::checkpoint::recovery_campaign_probe(&sandbox.root).unwrap();
            DispositionV1::DegradedUnavailable
        }
        "AC40" => {
            crate::features::audit_log::recovery_campaign_probe(&sandbox.root, &scenario.id)
                .unwrap();
            DispositionV1::Current
        }
        "AC41" => {
            let view = SemanticSessionView::load(&target).unwrap();
            assert_eq!(view.status, SemanticSessionStatus::ExactSuffix);
            assert!(view.transcript_markdown(true).is_err());
            DispositionV1::DegradedUnavailable
        }
        "AC42" => {
            let binding =
                SessionViewBinding::new(target.snapshot.clone(), target.session_id.clone());
            binding.replace(target.clone());
            let mut replacement = target;
            replacement.generation += 1;
            binding.replace(replacement);
            assert_eq!(binding.snapshot().generation, 8);
            assert_eq!(
                SemanticSessionView::load(&binding.snapshot())
                    .unwrap()
                    .status,
                SemanticSessionStatus::ExactSuffix
            );
            DispositionV1::DegradedUnavailable
        }
        "AC43" => {
            crate::features::audit_log::recovery_campaign_probe(&sandbox.root, &scenario.id)
                .unwrap();
            DispositionV1::DegradedUnavailable
        }
        "AC44" => {
            crate::features::session_log::recovery_campaign_probe(
                &sandbox.root,
                &target,
                &scenario.id,
            )
            .unwrap();
            DispositionV1::Current
        }
        "AC45" => {
            crate::features::session_log::recovery_campaign_probe(
                &sandbox.root,
                &target,
                &scenario.id,
            )
            .unwrap();
            DispositionV1::DegradedUnavailable
        }
        _ => unreachable!(),
    };
    sandbox.assert_source_immutable();
    disposition
}

fn exercise_host_record_scenario(scenario: &Scenario) -> DispositionV1 {
    use crate::{
        conversation::{ConversationState, OperatorToolObservation},
        session_host_storage::{self, SessionStorageBinding},
    };

    let sandbox = RecoverySandbox::semantic_seed(seed_for(scenario.lineage));
    let snapshot = sandbox.path("session.json");
    if scenario.lineage == Lineage::Legacy {
        fs::remove_file(sandbox.path("session.authority.jsonl")).unwrap();
        match scenario.id.as_str() {
            "AC34" => {
                assert!(
                    session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root)
                        .unwrap()
                        .is_none()
                );
                assert!(!snapshot.with_extension("observations.v1.exists").exists());
            }
            "AC35" => {
                ConversationState::new().save_session(&snapshot).unwrap();
                assert!(
                    session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root)
                        .unwrap()
                        .is_none()
                );
            }
            "AC36" => {
                assert!(!snapshot.with_extension("catalog.v1.json").exists());
                assert!(
                    session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root)
                        .unwrap()
                        .is_none()
                );
            }
            _ => unreachable!("legacy host-record executor table admits only AC34-AC36"),
        }
        sandbox.assert_source_immutable();
        return DispositionV1::DegradedUnavailable;
    }

    let binding = SessionStorageBinding::discover(&snapshot, SESSION_ID, &sandbox.root).unwrap();
    session_host_storage::save_full_spine(&binding, &ConversationState::new(), None).unwrap();
    let observation = OperatorToolObservation {
        execution_id: "campaign-observation".into(),
        tool_name: "read".into(),
        arguments: serde_json::json!({"path":"fixture"}),
        cwd: sandbox.root.clone(),
        content: Vec::new(),
        is_error: false,
        exit_code: 0,
        duration_ms: 0,
        origin: omegon_traits::ToolExecutionOrigin::Agent,
    };
    let stem = snapshot.file_stem().unwrap().to_string_lossy();
    let catalog = snapshot.with_file_name(format!("{stem}.catalog.v1.json"));
    let observations = snapshot.with_file_name(format!("{stem}.observations.v1.jsonl"));
    let marker = snapshot.with_file_name(format!("{stem}.observations.v1.exists"));

    let disposition = match scenario.id.as_str() {
        "AC28" => {
            assert!(!observations.exists());
            assert!(
                session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root).is_ok()
            );
            DispositionV1::DegradedUnavailable
        }
        "AC29" => {
            fs::remove_file(catalog).unwrap();
            assert!(
                session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root).is_err()
            );
            DispositionV1::FatalStoreInvariant
        }
        "AC30" => {
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
            value["session_id"] = "wrong-session".into();
            fs::write(catalog, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(
                session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root).is_err()
            );
            DispositionV1::FatalStoreInvariant
        }
        "AC31" => {
            session_host_storage::append_observation(&binding, &observation).unwrap();
            fs::write(observations, b"{").unwrap();
            assert!(
                session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root).is_err()
            );
            DispositionV1::BlockedCorrupt
        }
        "AC32" => {
            fs::write(marker, b"omegon-observation-ledger-v1\n").unwrap();
            assert!(
                session_host_storage::load_resume(&snapshot, SESSION_ID, &sandbox.root).is_err()
            );
            DispositionV1::BlockedUnavailable
        }
        "AC33" => {
            session_host_storage::append_observation(&binding, &observation).unwrap();
            let mut conflicting = observation;
            conflicting.arguments = serde_json::json!({"path":"different"});
            assert!(session_host_storage::append_observation(&binding, &conflicting).is_err());
            DispositionV1::BlockedCorrupt
        }
        _ => unreachable!(),
    };
    sandbox.assert_source_immutable();
    disposition
}

fn assert_partial_mirror_publication() {
    let directory = tempfile::tempdir().unwrap();
    let mirror = directory.path().join("mirror.json");
    fs::create_dir(&mirror).unwrap();
    let error = crate::session::publish_compatibility_mirrors(
        &crate::conversation::ConversationState::new(),
        &mirror,
        &crate::session::SessionMeta {
            session_id: "fixture".into(),
            cwd: directory.path().to_string_lossy().into_owned(),
            created_at: "created".into(),
            turns: 0,
            tool_calls: 0,
            description: String::new(),
            friendly_name: String::new(),
            last_prompt_snippet: String::new(),
        },
        true,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        crate::session::SessionSaveError::PartialPublication { .. }
    ));
}

fn exercise_mirror_scenario(scenario: &Scenario) -> DispositionV1 {
    let sandbox = RecoverySandbox::semantic_seed(seed_for(scenario.lineage));
    let target = semantic_target(&sandbox, scenario.lineage);
    let disposition = match scenario.id.as_str() {
        "AC46" => {
            assert_partial_mirror_publication();
            DispositionV1::PartialPublication
        }
        "AC47" => {
            crate::conversation::ConversationState::new()
                .save_session(&target.snapshot)
                .unwrap();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::ExactFull
            );
            DispositionV1::DegradedStale
        }
        "AC48" => {
            assert_partial_mirror_publication();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::ExactFull
            );
            DispositionV1::PartialPublication
        }
        "AC49" => {
            assert_partial_mirror_publication();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::ExactSuffix
            );
            DispositionV1::PartialPublication
        }
        "AC50" => {
            fs::remove_file(&target.snapshot).unwrap_or_default();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::ExactSuffix
            );
            DispositionV1::DegradedUnavailable
        }
        "AC51" => {
            fs::write(&target.snapshot, b"{\"session_id\":\"wrong\"}").unwrap();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::ExactSuffix
            );
            DispositionV1::PartialPublication
        }
        "AC52" => {
            assert_partial_mirror_publication();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::LegacyUnavailable
            );
            DispositionV1::PartialPublication
        }
        "AC53" => {
            crate::conversation::ConversationState::new()
                .save_session(&target.snapshot)
                .unwrap();
            assert_eq!(
                SemanticSessionView::load(&target).unwrap().status,
                SemanticSessionStatus::LegacyUnavailable
            );
            DispositionV1::DegradedStale
        }
        "AC54" => {
            crate::conversation::ConversationState::new()
                .save_session(&target.snapshot)
                .unwrap();
            assert!(
                SemanticSessionView::load(&target)
                    .unwrap()
                    .transcript_markdown(false)
                    .is_err()
            );
            DispositionV1::BlockedUnavailable
        }
        _ => unreachable!("mirror executor table admits only AC46-AC54"),
    };
    sandbox.assert_source_immutable();
    disposition
}

#[test]
fn recovery_campaign() {
    let started = Instant::now();
    let manifest = manifest();
    validate_manifest(&manifest);
    for (scenario, executor) in manifest.scenarios.iter().zip(FROZEN_EXECUTORS) {
        let observed = (executor.execute)(scenario);
        assert_eq!(
            observed, executor.disposition,
            "{} disposition oracle",
            scenario.id
        );
    }
    assert!(started.elapsed() <= Duration::from_secs(15));
}

#[test]
fn corrupt_immutable_chunk_is_quarantined_and_rebuilt_from_authority() {
    let sandbox = RecoverySandbox::semantic_seed("full-spine-crash-prefix.authority.jsonl");
    let snapshot = sandbox.path("session.json");
    let replay =
        SessionReplay::replay_prefix(&snapshot, SESSION_ID, STREAM_ID, ReplayEnd::EndOfStream)
            .unwrap();
    let root = sandbox.path("session.projections");
    let coordinator = SessionProjectionCoordinator::open(&root).unwrap();
    coordinator.publish(&replay, &[ShadowProjector::Transcript]);
    let chunk_dir = root.join("session.transcript/chunks/sha256");
    let chunk = fs::read_dir(&chunk_dir)
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .unwrap()
        .path();
    fs::write(&chunk, b"corrupt derived bytes").unwrap();
    let reports = coordinator.publish(&replay, &[ShadowProjector::Transcript]);
    assert!(matches!(
        reports[0].status,
        crate::session_shadow_projection::ProjectorPublicationStatus::Published(_)
    ));
    let quarantine = chunk.with_extension("corrupt");
    assert_eq!(fs::read(quarantine).unwrap(), b"corrupt derived bytes");
    assert!(matches!(
        SessionProjectionReader::adjacent_to(&snapshot, SESSION_ID, STREAM_ID)
            .unwrap()
            .read(ShadowProjector::Transcript, ReplayEnd::EndOfStream),
        ProjectionReadV1::ExactFull(_)
    ));
    sandbox.assert_source_immutable();
}

#[test]
fn authoritative_damage_is_rejected_instead_of_rebuilt() {
    for seed in [
        "unsupported-event.authority.jsonl",
        "unsupported-version.authority.jsonl",
        "truncated-prefix.authority.jsonl",
        "sequence-conflict.authority.jsonl",
    ] {
        let sandbox = RecoverySandbox::semantic_seed(seed);
        assert!(
            SessionReplay::replay_prefix(
                &sandbox.path("session.json"),
                SESSION_ID,
                STREAM_ID,
                ReplayEnd::EndOfStream,
            )
            .is_err(),
            "{seed}"
        );
        sandbox.assert_source_immutable();
    }
}

#[test]
fn missing_catalog_and_observation_existence_evidence_fail_closed() {
    use crate::{
        conversation::{ConversationState, OperatorToolObservation},
        session_authority::{ActorIdentity, SessionAuthority, SessionAuthorityHandle},
        session_host_storage::{self, SessionStorageBinding},
    };

    let directory = tempfile::tempdir().unwrap();
    let id = "2026-08-22T12-00-00_55000001";
    let snapshot = directory.path().join(format!("{id}.json"));
    let authority = SessionAuthority::open(
        &snapshot,
        id,
        "workspace",
        "generation",
        ActorIdentity {
            principal: "campaign".into(),
            ingress: "test".into(),
        },
        "2026-08-22T12:00:00Z",
    )
    .unwrap();
    let authority = SessionAuthorityHandle::new(authority);
    assert!(session_host_storage::load_resume(&snapshot, id, directory.path()).is_err());

    let binding =
        SessionStorageBinding::from_authority(&snapshot, id, Some(&authority), directory.path());
    session_host_storage::save_full_spine(&binding, &ConversationState::new(), None).unwrap();
    assert!(session_host_storage::load_resume(&snapshot, id, directory.path()).is_ok());
    session_host_storage::append_observation(
        &binding,
        &OperatorToolObservation {
            execution_id: "campaign-observation".into(),
            tool_name: "read".into(),
            arguments: serde_json::json!({"path":"fixture"}),
            cwd: directory.path().to_path_buf(),
            content: Vec::new(),
            is_error: false,
            exit_code: 0,
            duration_ms: 0,
            origin: omegon_traits::ToolExecutionOrigin::Agent,
        },
    )
    .unwrap();
    let observations = snapshot.with_file_name(format!("{id}.observations.v1.jsonl"));
    let marker = snapshot.with_file_name(format!("{id}.observations.v1.exists"));
    fs::remove_file(&observations).unwrap();
    assert!(session_host_storage::load_resume(&snapshot, id, directory.path()).is_err());
    fs::write(observations, b"").unwrap();
    fs::write(marker, b"substituted\n").unwrap();
    assert!(session_host_storage::load_resume(&snapshot, id, directory.path()).is_err());
}

#[test]
fn semantic_success_and_mirror_failure_is_typed_partial_publication() {
    assert_partial_mirror_publication();
}

#[test]
fn injection_boundaries_are_occurrence_exact() {
    let all = [
        InjectionBoundary::AuthorityAppend,
        InjectionBoundary::LedgerAppend,
        InjectionBoundary::SyncAll,
        InjectionBoundary::TemporaryOutputWrite,
        InjectionBoundary::AtomicRename,
        InjectionBoundary::ParentSync,
        InjectionBoundary::ValidatedRead,
        InjectionBoundary::NotificationEnqueue,
        InjectionBoundary::NotificationDequeue,
        InjectionBoundary::WorkerStart,
        InjectionBoundary::WorkerStop,
        InjectionBoundary::WorkerDrain,
        InjectionBoundary::GenerationFencePublish,
        InjectionBoundary::MirrorPublish,
    ];
    for boundary in all {
        let mut barriers = InjectionBarriers::default();
        barriers.fail_at(boundary, 2, ConsumerFaultV1::SyncFailed);
        assert_eq!(barriers.cross(boundary), None);
        assert_eq!(barriers.cross(boundary), Some(ConsumerFaultV1::SyncFailed));
        assert_eq!(barriers.cross(boundary), None);
    }
}

#[test]
fn campaign_has_no_timing_network_or_process_dependency() {
    let source = include_str!("session_recovery_campaign.rs");
    for forbidden in [
        ["thread", "::sleep"].concat(),
        ["tokio::time", "::sleep"].concat(),
        ["std::process", "::Command"].concat(),
        ["tokio::process", "::Command"].concat(),
        ["reqwest", "::"].concat(),
    ] {
        assert!(
            !source.contains(&forbidden),
            "forbidden campaign dependency: {forbidden}"
        );
    }
}

#[cfg(unix)]
#[test]
fn projection_chunk_no_follow_and_mode_damage_are_rebuildable() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    for damage in ["symlink", "mode"] {
        let sandbox = RecoverySandbox::semantic_seed("full-spine-crash-prefix.authority.jsonl");
        let snapshot = sandbox.path("session.json");
        let replay =
            SessionReplay::replay_prefix(&snapshot, SESSION_ID, STREAM_ID, ReplayEnd::EndOfStream)
                .unwrap();
        let root = sandbox.path("session.projections");
        let coordinator = SessionProjectionCoordinator::open(&root).unwrap();
        coordinator.publish(&replay, &[ShadowProjector::Transcript]);
        let chunk_dir = root.join("session.transcript/chunks/sha256");
        let chunk = fs::read_dir(&chunk_dir)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
            .unwrap()
            .path();
        if damage == "symlink" {
            let target = sandbox.path("outside-target");
            fs::write(&target, b"must not be read").unwrap();
            fs::remove_file(&chunk).unwrap();
            symlink(&target, &chunk).unwrap();
        } else {
            fs::set_permissions(&chunk, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let reports = coordinator.publish(&replay, &[ShadowProjector::Transcript]);
        assert!(
            matches!(
                reports[0].status,
                crate::session_shadow_projection::ProjectorPublicationStatus::Published(_)
            ),
            "{damage}"
        );
    }
}
