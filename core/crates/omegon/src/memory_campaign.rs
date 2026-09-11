//! Serial cross-boundary campaign for the portable managed-memory contract.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use omegon_memory::{
    CreateEdge, DecayProfileName, FactFilter, FactPrecondition, MemoryMutation,
    MemoryMutationEffect, Section, StoreEpisode, StoreFact,
};
use omegon_traits::{
    BusEvent, BusRequest, ContextSignals, Feature, LifecyclePhase, ManagedServiceCallError,
};
use tokio::sync::Barrier;
use tokio_util::sync::CancellationToken;

use crate::memory_service::{
    MemoryBinding, MemoryDeclarationFeature, MemoryPayloadV1, MemoryRequestV1, MemoryScopeV1,
    MemoryService, MemoryServiceErrorCodeV1, MemoryVaultConfigV1, MemoryWorkerConfig,
};

const MIND_A: &str = "campaign-alpha";
const MIND_B: &str = "campaign-beta";

fn config(root: &Path, vault: Option<PathBuf>) -> MemoryWorkerConfig {
    let vault = vault.map(|root| {
        MemoryVaultConfigV1::validated(
            root,
            &crate::codex_config::MemorySync {
                import_on_session_start: false,
                materialize_on_session_end: true,
                reinforce_references: false,
                max_episodes: 5,
            },
        )
        .expect("campaign vault configuration")
    });
    MemoryWorkerConfig {
        workspace_root: Some(root.to_path_buf()),
        project_memory_root: root.to_path_buf(),
        project_db_path: root.join("facts.db"),
        project_jsonl_path: root.join("facts.jsonl"),
        global_db_path: None,
        vault,
        startup_sync_enabled: false,
    }
}

async fn managed(root: &Path, vault: Option<PathBuf>) -> (crate::bus::EventBus, MemoryBinding) {
    managed_with_config(root, config(root, vault)).await
}

async fn managed_with_config(
    root: &Path,
    config: MemoryWorkerConfig,
) -> (crate::bus::EventBus, MemoryBinding) {
    let mut bus = crate::bus::EventBus::new();
    bus.set_project_root(root.to_path_buf());
    bus.register(Box::new(MemoryDeclarationFeature));
    bus.stage_managed_generation(
        "memory",
        crate::memory_service::start_candidate(config)
            .await
            .expect("campaign memory candidate"),
    )
    .expect("stage campaign memory");
    bus.try_finalize_managed()
        .await
        .expect("publish campaign memory");
    let binding = MemoryBinding::default();
    binding.capture(&bus).expect("capture campaign memory");
    (bus, binding)
}

fn store(mind: &str, content: &str) -> MemoryMutation {
    MemoryMutation::StoreFact {
        request: StoreFact {
            mind: mind.into(),
            content: content.into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: Some("memory-campaign".into()),
        },
    }
}

fn mutate(operation_id: &str, mutation: MemoryMutation) -> MemoryRequestV1 {
    MemoryRequestV1::ApplyMutation {
        scope: MemoryScopeV1::Project,
        operation_id: operation_id.into(),
        mutation,
        cancellation: CancellationToken::new(),
    }
}

async fn outcome(
    binding: &MemoryBinding,
    operation_id: &str,
    mutation: MemoryMutation,
) -> omegon_memory::MemoryMutationOutcome {
    let response = binding
        .invoke(mutate(operation_id, mutation))
        .await
        .expect("campaign mutation");
    let MemoryPayloadV1::Mutation(outcome) = response.payload else {
        panic!("campaign mutation returned wrong payload")
    };
    outcome
}

fn stored_fact(outcome: &omegon_memory::MemoryMutationOutcome) -> (String, u64) {
    let MemoryMutationEffect::FactStored {
        fact_id, version, ..
    } = &outcome.effect
    else {
        panic!("expected stored fact effect")
    };
    (fact_id.clone(), *version)
}

async fn facts(binding: &MemoryBinding, mind: &str) -> Vec<omegon_memory::Fact> {
    let response = binding
        .invoke(MemoryRequestV1::ListFactsPage {
            scope: MemoryScopeV1::Project,
            mind: mind.into(),
            filter: FactFilter::default(),
            limit: 100,
            cursor: None,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("campaign fact page");
    let MemoryPayloadV1::FactPage(page) = response.payload else {
        panic!("campaign fact page returned wrong payload")
    };
    page.facts
}

fn files_below(root: &Path) -> Vec<PathBuf> {
    fn visit(path: &Path, files: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(path).expect("campaign cleanup directory") {
            let path = entry.expect("campaign cleanup entry").path();
            if path.is_dir() {
                visit(&path, files);
            } else {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    if root.is_dir() {
        visit(root, &mut files);
    }
    files
}

fn assert_reopen_rename_delete(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let bytes = std::fs::read(path).expect("reopen settled campaign file");
    let renamed = path.with_extension("memory-campaign-released");
    std::fs::rename(path, &renamed).expect("rename settled campaign file");
    assert_eq!(
        std::fs::read(&renamed).expect("read renamed campaign file"),
        bytes
    );
    std::fs::remove_file(&renamed).expect("delete settled campaign file");
    true
}

#[tokio::test]
#[ignore = "portable managed-memory campaign runs serially in its dedicated CI job"]
async fn memory_campaign_portable_round_trip_isolated_minds_and_releases_every_file() {
    let directory = tempfile::tempdir().expect("campaign directory");
    let root = directory.path().join("memory");
    let vault = directory.path().join("vault");
    std::fs::create_dir(&root).expect("campaign memory root");
    std::fs::create_dir(&vault).expect("campaign vault root");
    let (mut bus, binding) = managed(&root, Some(vault.clone())).await;

    let first = outcome(
        &binding,
        "round-first",
        store(MIND_A, "Portable OAuth boundary"),
    )
    .await;
    let second = outcome(
        &binding,
        "round-second",
        store(MIND_A, "Portable adapter graph"),
    )
    .await;
    outcome(
        &binding,
        "round-other-mind",
        store(MIND_B, "Isolated beta-only fact"),
    )
    .await;
    let (first_id, first_version) = stored_fact(&first);
    let (second_id, _) = stored_fact(&second);
    outcome(
        &binding,
        "round-vector",
        MemoryMutation::StoreIdentifiedEmbedding {
            fact: FactPrecondition {
                id: first_id.clone(),
                expected_version: first_version,
            },
            embedding: omegon_memory::IdentifiedEmbedding {
                space: omegon_memory::EmbeddingSpace {
                    model: "campaign-model".into(),
                    revision: "fixture-v1".into(),
                    preprocessing: "raw-v1".into(),
                    dimensions: 3,
                },
                values: vec![1.0, 0.0, 0.0],
            },
        },
    )
    .await;
    outcome(
        &binding,
        "round-edge",
        MemoryMutation::CreateEdge {
            mind: MIND_A.into(),
            request: CreateEdge {
                source_id: first_id.clone(),
                target_id: second_id.clone(),
                relation: "supports".into(),
                description: Some("campaign edge".into()),
            },
        },
    )
    .await;
    outcome(
        &binding,
        "round-episode",
        MemoryMutation::StoreEpisode {
            request: StoreEpisode {
                mind: MIND_A.into(),
                title: "Portable campaign episode".into(),
                narrative: "Persisted through a managed generation.".into(),
                date: Some("2026-08-26".into()),
                affected_nodes: vec!["slice-6.1.9.5".into()],
                affected_changes: vec!["portable-memory".into()],
                files_changed: vec!["src/memory_campaign.rs".into()],
                tags: vec!["campaign".into(), "portable".into()],
                tool_calls_count: Some(7),
                formation: None,
            },
        },
    )
    .await;
    let exported = binding
        .invoke(MemoryRequestV1::ExportConfiguredJsonl {
            scope: MemoryScopeV1::Project,
            mind: MIND_A.into(),
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("campaign JSONL export");
    assert!(matches!(exported.payload, MemoryPayloadV1::Jsonl(report) if report.changed));
    let alpha_jsonl =
        std::fs::read_to_string(root.join("facts.jsonl")).expect("read alpha campaign JSONL");
    binding
        .invoke(MemoryRequestV1::ExportConfiguredJsonl {
            scope: MemoryScopeV1::Project,
            mind: MIND_B.into(),
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("campaign beta JSONL export");
    let beta_jsonl =
        std::fs::read_to_string(root.join("facts.jsonl")).expect("read beta campaign JSONL");
    let recovery_jsonl = format!("{}\n{}\n", alpha_jsonl.trim_end(), beta_jsonl.trim_end());
    binding
        .invoke(MemoryRequestV1::ExportConfiguredJsonl {
            scope: MemoryScopeV1::Project,
            mind: MIND_A.into(),
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("restore authoritative alpha JSONL");
    let materialized = binding
        .invoke(MemoryRequestV1::VaultSessionEnd {
            scope: MemoryScopeV1::Project,
            mind: MIND_A.into(),
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("campaign vault materialization");
    assert!(
        matches!(materialized.payload, MemoryPayloadV1::Vault(report)
        if report.files_written > 0 && report.episodes_written == 1)
    );
    bus.shutdown_managed_services_strict()
        .await
        .expect("strict first-generation shutdown");

    let recovery_root = directory.path().join("recovery");
    std::fs::create_dir(&recovery_root).expect("campaign recovery root");
    std::fs::write(recovery_root.join("facts.jsonl"), recovery_jsonl)
        .expect("campaign recovery JSONL fixture");
    let (mut recovery_bus, recovery) = managed(&recovery_root, None).await;
    let first_import = recovery
        .invoke(MemoryRequestV1::ImportConfiguredJsonl {
            scope: MemoryScopeV1::Project,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("managed recovery import");
    let replay_import = recovery
        .invoke(MemoryRequestV1::ImportConfiguredJsonl {
            scope: MemoryScopeV1::Project,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("managed recovery replay");
    let (MemoryPayloadV1::Jsonl(first_import), MemoryPayloadV1::Jsonl(replay_import)) =
        (first_import.payload, replay_import.payload)
    else {
        panic!("managed recovery import returned wrong payload")
    };
    assert!(first_import.changed);
    assert!(!replay_import.changed);
    assert_eq!(first_import.content_hash, replay_import.content_hash);
    assert_eq!(first_import.imported, replay_import.imported);
    assert_eq!(facts(&recovery, MIND_A).await.len(), 2);
    assert_eq!(facts(&recovery, MIND_B).await.len(), 1);
    assert!(
        facts(&recovery, MIND_A)
            .await
            .iter()
            .all(|fact| fact.mind == MIND_A)
    );
    recovery_bus
        .shutdown_managed_services_strict()
        .await
        .expect("strict recovery-generation shutdown");

    let (mut reopened_bus, reopened) = managed(&root, Some(vault.clone())).await;
    let alpha = facts(&reopened, MIND_A).await;
    let beta = facts(&reopened, MIND_B).await;
    assert_eq!(alpha.len(), 2);
    assert_eq!(beta.len(), 1);
    assert!(alpha.iter().all(|fact| fact.mind == MIND_A));
    assert!(beta.iter().all(|fact| fact.mind == MIND_B));
    assert!(!alpha.iter().any(|fact| fact.content.contains("beta-only")));

    let episodes = reopened
        .invoke(MemoryRequestV1::ListEpisodes {
            scope: MemoryScopeV1::Project,
            mind: MIND_A.into(),
            limit: 5,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("reopened episodes");
    assert!(
        matches!(episodes.payload, MemoryPayloadV1::Episodes(episodes)
        if episodes.len() == 1
            && episodes[0].date == "2026-08-26"
            && episodes[0].affected_nodes == ["slice-6.1.9.5"]
            && episodes[0].tool_calls_count == Some(7))
    );

    let search =
        |mind: &str, query: &str, query_vector: Option<Vec<f32>>| MemoryRequestV1::HybridSearch {
            scope: MemoryScopeV1::Project,
            filter: Default::default(),
            mind: mind.into(),
            query: query.into(),
            query_space: query_vector
                .as_ref()
                .map(|vector| omegon_memory::EmbeddingSpace {
                    model: "campaign-model".into(),
                    revision: "fixture-v1".into(),
                    preprocessing: "raw-v1".into(),
                    dimensions: vector.len() as u32,
                }),
            query_vector,
            include_diagnostics: false,
            limit: 2,
            fetch_limit: 4,
            min_similarity: 0.0,
            cancellation: CancellationToken::new(),
        };
    let first_fts = reopened
        .invoke(search(MIND_B, "Isolated beta", Some(vec![1.0, 0.0, 0.0])))
        .await
        .expect("first no-vector fallback search");
    let second_fts = reopened
        .invoke(search(MIND_B, "Isolated beta", Some(vec![1.0, 0.0, 0.0])))
        .await
        .expect("second no-vector fallback search");
    let ids = |payload| match payload {
        MemoryPayloadV1::ScoredFacts(facts) => facts
            .into_iter()
            .map(|fact| fact.fact.id)
            .collect::<Vec<_>>(),
        _ => panic!("campaign search returned wrong payload"),
    };
    let first_fts = ids(first_fts.payload);
    let second_fts = ids(second_fts.payload);
    assert_eq!(first_fts, second_fts);
    assert_eq!(first_fts, vec![beta[0].id.clone()]);
    let dimension_fallback = reopened
        .invoke(search(MIND_A, "Portable OAuth", Some(vec![1.0, 0.0])))
        .await
        .expect("embedding dimension failure falls back to FTS");
    assert!(
        matches!(dimension_fallback.payload, MemoryPayloadV1::ScoredFacts(facts)
        if facts.first().is_some_and(|fact| fact.fact.id == first_id))
    );
    let vector = reopened
        .invoke(search(MIND_A, "Portable OAuth", Some(vec![1.0, 0.0, 0.0])))
        .await
        .expect("reopened vector search");
    assert!(matches!(vector.payload, MemoryPayloadV1::ScoredFacts(facts)
        if facts.first().is_some_and(|fact| fact.fact.id == first_id)));
    let metadata = reopened
        .invoke(MemoryRequestV1::EmbeddingMetadata {
            scope: MemoryScopeV1::Project,
            mind: MIND_A.into(),
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("reopened embedding metadata");
    assert!(
        matches!(metadata.payload, MemoryPayloadV1::EmbeddingMetadata(Some(metadata))
        if metadata.model_name == "campaign-model" && metadata.dims == 3)
    );
    let edges = reopened
        .invoke(MemoryRequestV1::GetEdges {
            scope: MemoryScopeV1::Project,
            mind: MIND_A.into(),
            fact_id: first_id,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("reopened graph");
    assert!(matches!(edges.payload, MemoryPayloadV1::Edges(edges)
        if edges.len() == 1 && edges[0].target_id == second_id));
    assert!(
        matches!(reopened.invoke(MemoryRequestV1::ExportConfiguredJsonl {
        scope: MemoryScopeV1::Project,
        mind: MIND_A.into(),
        cancellation: CancellationToken::new(),
    }).await.expect("idempotent JSONL").payload, MemoryPayloadV1::Jsonl(report) if !report.changed)
    );
    assert!(matches!(reopened.invoke(MemoryRequestV1::VaultSessionEnd {
        scope: MemoryScopeV1::Project,
        mind: MIND_A.into(),
        cancellation: CancellationToken::new(),
    }).await.expect("idempotent vault").payload, MemoryPayloadV1::Vault(report) if report.files_written == 0));
    reopened_bus
        .shutdown_managed_services_strict()
        .await
        .expect("strict reopened-generation shutdown");

    let vault_files = files_below(&vault);
    assert!(root.join("facts.db").is_file());
    assert!(root.join("facts.jsonl").is_file());
    assert!(vault_files.iter().any(|path| path.is_file()));
    let scanned = files_below(&root)
        .into_iter()
        .chain(vault_files.iter().cloned())
        .chain(files_below(&recovery_root));
    assert!(scanned.into_iter().all(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.contains(".tmp"))
    }));
    assert!(assert_reopen_rename_delete(&root.join("facts.db")));
    assert!(assert_reopen_rename_delete(&root.join("facts.jsonl")));
    for sidecar in [root.join("facts.db-wal"), root.join("facts.db-shm")] {
        let existed = sidecar.exists();
        assert_eq!(assert_reopen_rename_delete(&sidecar), existed);
    }
    for path in vault_files {
        assert!(assert_reopen_rename_delete(&path));
    }
    for path in files_below(&recovery_root) {
        assert!(assert_reopen_rename_delete(&path));
    }
}

#[tokio::test]
#[ignore = "portable managed-memory campaign runs serially in its dedicated CI job"]
async fn memory_campaign_concurrent_commits_replays_and_version_conflicts_are_typed() {
    let directory = tempfile::tempdir().expect("campaign directory");
    let root = directory.path().join("memory");
    std::fs::create_dir(&root).expect("campaign memory root");
    let (mut bus, binding) = managed(&root, None).await;
    let race = |left: MemoryRequestV1, right: MemoryRequestV1| async {
        let barrier = Arc::new(Barrier::new(3));
        let task = |request, barrier: Arc<Barrier>| {
            let binding = binding.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                binding.invoke(request).await
            })
        };
        let left = task(left, Arc::clone(&barrier));
        let right = task(right, Arc::clone(&barrier));
        barrier.wait().await;
        [
            left.await.expect("left race join"),
            right.await.expect("right race join"),
        ]
    };

    let independent = race(
        mutate("independent-left", store(MIND_A, "independent left")),
        mutate("independent-right", store(MIND_A, "independent right")),
    )
    .await;
    assert!(independent.iter().all(Result::is_ok));
    let replay = race(
        mutate("exact-replay", store(MIND_A, "exact replay")),
        mutate("exact-replay", store(MIND_A, "exact replay")),
    )
    .await;
    let replayed = replay
        .into_iter()
        .map(
            |response| match response.expect("commit or replay").payload {
                MemoryPayloadV1::Mutation(outcome) => outcome,
                _ => panic!("race returned wrong payload"),
            },
        )
        .collect::<Vec<_>>();
    assert_eq!(
        replayed.iter().filter(|outcome| outcome.replayed).count(),
        1
    );
    assert_eq!(replayed[0].effect, replayed[1].effect);

    let target = outcome(&binding, "version-target", store(MIND_A, "version target")).await;
    let (id, version) = stored_fact(&target);
    let reinforce = |operation_id: &str| {
        mutate(
            operation_id,
            MemoryMutation::ReinforceFact {
                fact: FactPrecondition {
                    id: id.clone(),
                    expected_version: version,
                },
            },
        )
    };
    let targeted = race(reinforce("version-left"), reinforce("version-right")).await;
    assert_eq!(targeted.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        targeted
            .iter()
            .filter(|result| matches!(result,
        Err(ManagedServiceCallError::Operation(error))
            if error.code == MemoryServiceErrorCodeV1::FactVersionConflict))
            .count(),
        1
    );
    bus.shutdown_managed_services_strict()
        .await
        .expect("strict concurrent campaign shutdown");
}

async fn wait_started(receiver: std::sync::mpsc::Receiver<()>) {
    tokio::task::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(2)))
        .await
        .expect("campaign start wait task")
        .expect("campaign request started");
}

#[tokio::test]
#[ignore = "portable managed-memory campaign runs serially in its dedicated CI job"]
async fn memory_campaign_cancellation_settles_queue_atomic_vector_and_vault_work() {
    let directory = tempfile::tempdir().expect("campaign directory");
    let root = directory.path().join("memory");
    let vault = directory.path().join("vault");
    std::fs::create_dir(&root).expect("campaign memory root");
    std::fs::create_dir_all(vault.join("ai/memory")).expect("campaign vault input root");
    std::fs::write(
        vault.join("ai/memory/cancellation.md"),
        "+++\nkind = \"memory_fact\"\ntopic = \"Architecture\"\n+++\nCancellation fixture\n",
    )
    .expect("campaign vault input");
    let mut worker_config = config(&root, None);
    worker_config.vault = Some(
        MemoryVaultConfigV1::validated(
            vault,
            &crate::codex_config::MemorySync {
                import_on_session_start: true,
                materialize_on_session_end: false,
                reinforce_references: false,
                max_episodes: 0,
            },
        )
        .expect("campaign cancellation vault configuration"),
    );
    let (mut bus, binding) = managed_with_config(&root, worker_config).await;

    let vector_fact = outcome(
        &binding,
        "cancellation-vector-fact",
        store(MIND_A, "vector cancellation candidate"),
    )
    .await;
    let (vector_fact_id, vector_fact_version) = stored_fact(&vector_fact);
    outcome(
        &binding,
        "cancellation-vector-embedding",
        MemoryMutation::StoreEmbedding {
            fact: FactPrecondition {
                id: vector_fact_id,
                expected_version: vector_fact_version,
            },
            model_name: "campaign-cancellation-model".into(),
            embedding: vec![1.0, 0.0],
        },
    )
    .await;

    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let (started, receiver) = std::sync::mpsc::sync_channel(1);
    let blocker = tokio::spawn({
        let binding = binding.clone();
        let release = Arc::clone(&release);
        async move {
            binding
                .invoke(MemoryRequestV1::TestBlock {
                    started,
                    release,
                    cancellation: CancellationToken::new(),
                })
                .await
        }
    });
    wait_started(receiver).await;
    let queued_cancel = CancellationToken::new();
    let executions = Arc::new(AtomicUsize::new(0));
    let queued = tokio::spawn({
        let binding = binding.clone();
        let cancellation = queued_cancel.clone();
        let executions = Arc::clone(&executions);
        async move {
            binding
                .invoke(MemoryRequestV1::TestRecord {
                    executions,
                    cancellation,
                })
                .await
        }
    });
    tokio::task::yield_now().await;
    queued_cancel.cancel();
    assert!(matches!(queued.await.expect("queued join"),
        Err(ManagedServiceCallError::Operation(error)) if error.code == MemoryServiceErrorCodeV1::Cancelled));
    *release.0.lock().expect("release blocker") = true;
    release.1.notify_all();
    blocker
        .await
        .expect("blocker join")
        .expect("blocker result");
    assert_eq!(executions.load(Ordering::Acquire), 0);

    let atomic_cancel = CancellationToken::new();
    let atomic_release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let (started, receiver) = std::sync::mpsc::sync_channel(1);
    let atomic = tokio::spawn({
        let binding = binding.clone();
        let cancellation = atomic_cancel.clone();
        let release = Arc::clone(&atomic_release);
        async move {
            binding
                .invoke(MemoryRequestV1::TestAtomicMutation {
                    started,
                    release,
                    operation_id: "cancelled-atomic".into(),
                    mutation: store(MIND_A, "atomic settlement survives caller cancellation"),
                    cancellation,
                })
                .await
        }
    });
    wait_started(receiver).await;
    atomic_cancel.cancel();
    assert!(matches!(atomic.await.expect("atomic join"),
        Err(ManagedServiceCallError::Operation(error)) if error.code == MemoryServiceErrorCodeV1::Cancelled));
    *atomic_release.0.lock().expect("release atomic") = true;
    atomic_release.1.notify_all();
    let replay = outcome(
        &binding,
        "cancelled-atomic",
        store(MIND_A, "atomic settlement survives caller cancellation"),
    )
    .await;
    assert!(replay.replayed);

    for request in ["vector", "vault"] {
        let cancellation = CancellationToken::new();
        let (started, receiver) = std::sync::mpsc::sync_channel(1);
        let active = tokio::spawn({
            let binding = binding.clone();
            let cancellation = cancellation.clone();
            async move {
                let request = if request == "vector" {
                    MemoryRequestV1::TestVectorSearch {
                        started,
                        mind: MIND_A.into(),
                        vector: vec![1.0, 0.0],
                        cancellation,
                    }
                } else {
                    MemoryRequestV1::TestVaultSessionStart {
                        started,
                        mind: MIND_A.into(),
                        cancellation,
                    }
                };
                binding.invoke(request).await
            }
        });
        wait_started(receiver).await;
        cancellation.cancel();
        assert!(matches!(active.await.expect("active cancellation join"),
            Err(ManagedServiceCallError::Operation(error)) if error.code == MemoryServiceErrorCodeV1::Cancelled));
    }
    tokio::time::timeout(
        Duration::from_secs(3),
        bus.shutdown_managed_services_strict(),
    )
    .await
    .expect("strict cancellation cleanup deadline")
    .expect("strict cancellation cleanup");
}

struct EventContinuity(Arc<AtomicUsize>);

#[async_trait]
impl Feature for EventContinuity {
    fn name(&self) -> &str {
        "memory-campaign-event-continuity"
    }

    fn on_event(&mut self, _event: &BusEvent) -> Vec<BusRequest> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    }
}

#[tokio::test]
#[ignore = "portable managed-memory campaign runs serially in its dedicated CI job"]
async fn memory_campaign_typed_absence_preserves_tools_context_and_events() {
    let directory = tempfile::tempdir().expect("absent memory campaign directory");
    let mut bus = crate::bus::EventBus::new();
    bus.set_project_root(directory.path().to_path_buf());
    let events = Arc::new(AtomicUsize::new(0));
    bus.register(Box::new(EventContinuity(Arc::clone(&events))));
    let binding = MemoryBinding::default();
    bus.register(Box::new(crate::features::memory::MemoryFeature::new(
        binding.clone(),
        MIND_A.into(),
    )));
    bus.try_finalize_managed()
        .await
        .expect("optional absent memory composition");
    binding.capture(&bus).expect("capture absent memory");
    assert!(!binding.available());
    let names = bus
        .tool_definitions()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<HashSet<_>>();
    for expected in [
        crate::tool_registry::memory::MEMORY_STORE,
        crate::tool_registry::memory::MEMORY_RECALL,
        crate::tool_registry::memory::MEMORY_QUERY,
        crate::tool_registry::memory::MEMORY_ARCHIVE,
        crate::tool_registry::memory::MEMORY_SUPERSEDE,
        crate::tool_registry::memory::MEMORY_CONNECT,
        crate::tool_registry::memory::MEMORY_FOCUS,
        crate::tool_registry::memory::MEMORY_RELEASE,
        crate::tool_registry::memory::MEMORY_EPISODES,
        crate::tool_registry::memory::MEMORY_COMPACT,
        crate::tool_registry::memory::MEMORY_SEARCH_ARCHIVE,
        crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE,
    ] {
        assert!(
            names.contains(expected),
            "missing declared memory tool {expected}"
        );
    }
    assert!(matches!(binding.invoke(MemoryRequestV1::Status {
        scope: MemoryScopeV1::Project,
        cancellation: CancellationToken::new(),
    }).await, Err(ManagedServiceCallError::Operation(error))
        if error.code == MemoryServiceErrorCodeV1::Unavailable));
    let probe = crate::features::memory::MemoryFeature::new(binding.clone(), MIND_A.into());
    let tool_error = probe
        .execute(
            crate::tool_registry::memory::MEMORY_QUERY,
            "campaign-absent-query",
            serde_json::Value::Null,
            CancellationToken::new(),
        )
        .await
        .expect_err("absent memory tool must fail with typed evidence");
    assert_eq!(tool_error.to_string(), "memory:unavailable");
    let signals = ContextSignals {
        user_prompt: "Summarize durable policy and current state",
        recent_tools: &[crate::tool_registry::memory::MEMORY_QUERY.into()],
        recent_files: &[directory.path().join("src/lib.rs")],
        lifecycle_phase: &LifecyclePhase::Idle,
        turn_number: 3,
        context_budget_tokens: 2_000,
    };
    assert!(probe.provide_context(&signals).is_none());

    let context = crate::features::context::ContextProvider::new_with_sources(
        crate::features::context::SharedContextMetrics::new(),
        crate::features::context::new_shared_command_tx(),
        None,
        None,
        binding.clone(),
        MIND_A.into(),
        None,
    );
    let mixed = context
        .request_context(serde_json::json!({"requests": [
            {"kind": "memory", "query": "policy", "reason": "need durable context"},
            {"kind": "session_state", "query": "state", "reason": "preserve unrelated context"}
        ]}))
        .await
        .expect("mixed context request survives absent memory");
    let mixed_text = mixed
        .content
        .iter()
        .filter_map(omegon_traits::ContentBlock::as_text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(mixed_text.contains("managed memory unavailable (memory:unavailable)"));
    assert!(mixed_text.contains("Session State"));
    assert_eq!(mixed.details["supported"], 1);
    assert_eq!(mixed.details["unsupported"], 1);
    crate::status::refresh_managed_memory_status(&binding, directory.path()).await;
    assert!(!crate::status::managed_memory_status_snapshot_for(directory.path()).available);
    let unavailable = crate::status::HarnessStatus::assemble(directory.path());
    assert!(!unavailable.memory_available);
    assert_eq!(
        unavailable.memory_warning.as_deref(),
        Some("memory:status_binding_unavailable")
    );
    assert!(
        crate::bootstrap_projection::render_bootstrap(&unavailable, false)
            .contains("memory:status_binding_unavailable")
    );
    let web = crate::web::surfaces::project_memory_status(Some(&unavailable));
    assert_eq!((web.active_facts, web.total_facts), (0, 0));

    bus.emit(&BusEvent::TurnStart { turn: 1 });
    assert_eq!(events.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "portable managed-memory campaign runs serially in its dedicated CI job"]
async fn memory_campaign_generations_rollback_transfer_and_shared_status_project_identically() {
    let directory = tempfile::tempdir().expect("campaign directory");
    let root = directory.path().join("memory");
    std::fs::create_dir(&root).expect("campaign memory root");

    let mut rejected = crate::bus::EventBus::new();
    rejected.register(Box::new(MemoryDeclarationFeature));
    rejected.register(Box::new(MemoryDeclarationFeature));
    rejected
        .stage_managed_generation(
            "memory",
            crate::memory_service::start_candidate(config(&root, None))
                .await
                .expect("rejected candidate fixture"),
        )
        .expect("stage rejected candidate");
    assert!(rejected.try_finalize_managed().await.is_err());
    assert!(
        rejected
            .shutdown_managed_services()
            .await
            .all_resources_settled()
    );

    let (first_candidate, transferred_candidate) =
        crate::memory_service::campaign_exact_transfer_candidates(config(&root, None)).await;
    let mut transfer = crate::bus::EventBus::new();
    transfer.register(Box::new(MemoryDeclarationFeature));
    transfer
        .stage_managed_generation("memory", first_candidate)
        .expect("stage first transfer generation");
    transfer
        .try_finalize_managed()
        .await
        .expect("publish first transfer");
    let old_handle = transfer
        .managed_service::<MemoryService>(
            &crate::memory_service::memory_capability_id(),
            &crate::memory_service::memory_interface_id(),
        )
        .expect("lookup transferred service")
        .expect("transferred service available");
    transfer
        .stage_managed_generation("memory", transferred_candidate)
        .expect("stage exact transfer");
    transfer
        .try_finalize_managed()
        .await
        .expect("publish exact transfer");
    old_handle
        .invoke(MemoryRequestV1::Status {
            scope: MemoryScopeV1::Project,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("exact-generation old handle remains usable");
    transfer
        .shutdown_managed_services_strict()
        .await
        .expect("worker settles before dependent writer");

    let (mut old_bus, old_binding) = managed(&root, None).await;
    let status_first = outcome(
        &old_binding,
        "status-fixture-first",
        store(MIND_A, "status fixture first"),
    )
    .await;
    let status_second = outcome(
        &old_binding,
        "status-fixture-second",
        store(MIND_A, "status fixture second"),
    )
    .await;
    let (status_first_id, _) = stored_fact(&status_first);
    let (status_second_id, _) = stored_fact(&status_second);
    outcome(
        &old_binding,
        "status-fixture-edge",
        MemoryMutation::CreateEdge {
            mind: MIND_A.into(),
            request: CreateEdge {
                source_id: status_first_id,
                target_id: status_second_id,
                relation: "status-parity".into(),
                description: None,
            },
        },
    )
    .await;
    outcome(
        &old_binding,
        "status-fixture-episode",
        MemoryMutation::StoreEpisode {
            request: StoreEpisode {
                mind: MIND_A.into(),
                title: "Status parity episode".into(),
                narrative: "Shared managed status fixture.".into(),
                date: Some("2026-08-26".into()),
                affected_nodes: Vec::new(),
                affected_changes: Vec::new(),
                files_changed: Vec::new(),
                tags: vec!["status".into()],
                tool_calls_count: Some(1),
                formation: None,
            },
        },
    )
    .await;
    crate::status::refresh_managed_memory_status_for_mind(&old_binding, &root, MIND_A).await;
    old_bus
        .shutdown_managed_services_strict()
        .await
        .expect("old generation shutdown");
    assert!(matches!(
        old_binding
            .invoke(MemoryRequestV1::Status {
                scope: MemoryScopeV1::Project,
                cancellation: CancellationToken::new(),
            })
            .await,
        Err(ManagedServiceCallError::GenerationRetired)
    ));
    let (mut new_bus, new_binding) = managed(&root, None).await;
    new_binding
        .invoke(MemoryRequestV1::Status {
            scope: MemoryScopeV1::Project,
            cancellation: CancellationToken::new(),
        })
        .await
        .expect("new generation usable");
    crate::status::refresh_managed_memory_status_for_mind(&new_binding, &root, MIND_A).await;

    let managed_status = crate::status::managed_memory_status_snapshot_for(&root);
    assert!(managed_status.available);
    assert!(managed_status.warning.is_none());
    assert_eq!(managed_status.status.total_facts, 2);
    assert_eq!(managed_status.status.active_facts, 2);
    assert_eq!(managed_status.status.project_facts, 2);
    assert_eq!(managed_status.status.working_facts, 0);
    assert_eq!(managed_status.status.episodes, 1);
    assert_eq!(managed_status.status.edges, 1);
    assert_eq!(
        managed_status.authority,
        crate::memory_service::ManagedMemoryAuthorityV1::LocalIndexOnly
    );
    assert_eq!(
        managed_status.index_state,
        crate::memory_service::ManagedMemoryIndexStateV1::Fresh
    );
    let harness = crate::status::HarnessStatus::assemble(&root);
    assert!(harness.memory_available);
    assert!(harness.memory_warning.is_none());
    assert_eq!(harness.memory.total_facts, 2);
    assert_eq!(harness.memory.active_facts, 2);
    assert_eq!(harness.memory.project_facts, 2);
    assert_eq!(harness.memory.working_facts, 0);
    assert_eq!(harness.memory.episodes, 1);
    assert_eq!(harness.memory.edges, 1);
    let bootstrap = crate::bootstrap_projection::render_bootstrap(&harness, false);
    assert!(bootstrap.contains("2 facts, 1 episodes, 1 edges"));
    let federation = crate::surfaces::memory_status::project_memory_federation_status(
        crate::status::memory_federation_surface_observation(&root),
    );
    assert_eq!(
        federation.memory_authority,
        crate::surfaces::memory_status::MemoryAuthority::LocalIndexOnly
    );
    assert_eq!(
        federation.memory_index,
        crate::surfaces::memory_status::MemoryIndexState::Fresh
    );
    let web = crate::web::surfaces::project_memory_status(Some(&harness));
    assert_eq!((web.active_facts, web.total_facts), (2, 2));

    let host = crate::runtime_state::LifecycleHostHandle::new(Default::default());
    let handles = crate::runtime_state::RuntimeStateHandles::new(host, None, None, None, None);
    handles.install_harness(Arc::new(Mutex::new(harness.clone())));
    let session = crate::session_consumers::SessionViewBinding::new(
        root.join("session.json"),
        "memory-campaign".into(),
    );
    let ipc = crate::ipc::snapshot::build_state_snapshot(
        &handles,
        "test",
        &root.to_string_lossy(),
        "2026-08-26T00:00:00Z",
        "memory-campaign",
        &session,
        crate::surfaces::layout::UiPresentationLevel::Om,
    );
    assert!(ipc.harness.memory_available);
    assert!(ipc.harness.memory_warning.is_none());
    assert_eq!(ipc.harness.memory.active_facts, 2);
    assert_eq!(ipc.harness.memory.project_facts, 2);
    assert_eq!(ipc.harness.memory.working_facts, 0);
    assert_eq!(ipc.harness.memory.episodes, 1);
    let bootstrap_markdown = crate::bootstrap_projection::render_bootstrap(&harness, false);
    let acp = serde_json::to_value(crate::surfaces::diagnostics::HarnessStatusProjection::new(
        serde_json::to_value(harness.clone()).expect("harness snapshot"),
        1,
        "memory-campaign",
        "memory-campaign",
        "operator-driven",
        "campaign",
        bootstrap_markdown,
    ))
    .expect("ACP harness projection");
    assert_eq!(acp["harness"]["memory"]["active_facts"], 2);
    assert_eq!(acp["harness"]["memory"]["project_facts"], 2);
    assert_eq!(acp["harness"]["memory"]["working_facts"], 0);
    assert_eq!(acp["harness"]["memory"]["episodes"], 1);
    assert_eq!(acp["harness"]["memory"]["edges"], 1);
    assert_eq!(acp["harness"]["memory_available"], true);
    assert!(acp["harness"]["memory_warning"].is_null());
    let settings = crate::features::harness_settings::HarnessSettings::new(
        Arc::new(Mutex::new(crate::settings::Settings::default())),
        root.clone(),
    );
    assert!(
        settings
            .campaign_memory_stats_overview()
            .contains("Total facts**: 2")
    );
    let settings_status = settings.campaign_memory_stats_overview();
    assert!(settings_status.contains("Active facts**: 2"));
    assert!(settings_status.contains("Episodes**: 1"));
    assert!(settings_status.contains("Edges**: 1"));
    assert_eq!(
        serde_json::to_value(&harness).expect("harness wire")["memory"]["active_facts"],
        acp["harness"]["memory"]["active_facts"]
    );
    new_bus
        .shutdown_managed_services_strict()
        .await
        .expect("strict status campaign shutdown");
}
