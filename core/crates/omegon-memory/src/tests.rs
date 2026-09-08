//! Shared test suite for MemoryBackend implementations.
//!
//! Any struct implementing MemoryBackend can be tested by calling
//! `run_backend_tests(backend).await`. Both InMemoryBackend and
//! SqliteBackend are verified against the same expectations.

use crate::backend::*;
use crate::types::*;

/// Run the full backend test suite against any MemoryBackend implementation.
pub async fn run_backend_tests(b: &dyn MemoryBackend) {
    test_store_and_get(b).await;
    test_store_dedup(b).await;
    test_list_facts(b).await;
    test_reinforce(b).await;
    test_archive(b).await;
    test_supersede(b).await;
    test_superseding_fact(b).await;
    test_supersede_with_existing(b).await;
    test_vault_source_lineage_fallback(b).await;
    test_fts_search(b).await;
    test_vector_store_and_search(b).await;
    test_unidentified_vector_query_rejected(b).await;
    test_vector_search_cancellation(b).await;
    test_edges(b).await;
    test_episodes(b).await;
    test_jsonl_round_trip(b).await;
    test_jsonl_version_conflict(b).await;
    test_mutation_replay_and_conflict(b).await;
    test_targeted_mutation_version_conflict(b).await;
    test_duplicate_target_and_nonfinite_embedding_rejected(b).await;
    test_jsonl_batch_rollback(b).await;
    test_jsonl_import_advances_lamport_clock(b).await;
    test_page_snapshot_excludes_late_old_version_import(b).await;
    test_jsonl_rejects_unpersistable_lamport_version(b).await;
    test_deterministic_fts_fallback(b).await;
    test_episode_metadata_round_trip(b).await;
    test_stats(b).await;
    test_inventory_stats(b).await;
}

async fn test_page_snapshot_excludes_late_old_version_import(b: &dyn MemoryBackend) {
    for content in ["page alpha", "page beta", "page gamma"] {
        b.store_fact(store_request("page-snapshot", content))
            .await
            .unwrap();
    }
    let first = b
        .list_facts_page("page-snapshot", FactFilter::default(), 1, None)
        .await
        .unwrap();
    assert_eq!(first.total, 3);
    let late = JsonlRecord::Fact(JsonlFact {
        operational: None,
        id: "late-old-version-import".into(),
        mind: "page-snapshot".into(),
        content: "late import with old Lamport version".into(),
        section: Section::Architecture,
        status: FactStatus::Active,
        created_at: "2020-01-01T00:00:00Z".into(),
        source: Some("test".into()),
        content_hash: None,
        supersedes: None,
        version: 1,
        decay_profile: DecayProfileName::Standard,
        persona_id: None,
        layer: "project".into(),
        tags: vec![],
    });
    b.apply_mutation(
        "page-snapshot-late-import",
        MemoryMutation::ImportJsonl {
            jsonl: serde_json::to_string(&late).unwrap(),
        },
    )
    .await
    .unwrap();

    let mut ids = first
        .facts
        .into_iter()
        .map(|fact| fact.id)
        .collect::<std::collections::HashSet<_>>();
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let page = b
            .list_facts_page("page-snapshot", FactFilter::default(), 1, Some(&next))
            .await
            .unwrap();
        assert_eq!(page.total, 3);
        for fact in page.facts {
            assert!(ids.insert(fact.id), "page snapshot returned a duplicate");
        }
        cursor = page.next_cursor;
    }
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains("late-old-version-import"));
}

async fn test_vector_search_cancellation(b: &dyn MemoryBackend) {
    let space = EmbeddingSpace {
        model: "test-model".into(),
        revision: "fixture-v1".into(),
        preprocessing: "raw-v1".into(),
        dimensions: 4,
    };
    for index in 0..16 {
        let fact = b
            .store_fact(store_request(
                "vector-cancellation",
                &format!("vector candidate {index}"),
            ))
            .await
            .unwrap();
        b.apply_mutation(
            &format!("cancel-index-{index}"),
            MemoryMutation::StoreIdentifiedEmbedding {
                fact: FactPrecondition {
                    id: fact.fact.id,
                    expected_version: fact.fact.version,
                },
                embedding: IdentifiedEmbedding {
                    space: space.clone(),
                    values: vec![1.0, index as f32, 0.0, 0.0],
                },
            },
        )
        .await
        .unwrap();
    }
    let checks = std::sync::atomic::AtomicUsize::new(0);
    let cancelled = || checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 3;
    let result = b
        .search_identified(
            "vector-cancellation",
            &IdentifiedEmbedding {
                space,
                values: vec![1.0, 1.0, 0.0, 0.0],
            },
            16,
            -1.0,
            &Default::default(),
            &cancelled,
        )
        .await;
    assert!(matches!(result, Err(MemoryError::Cancelled)));
}

async fn test_store_and_get(b: &dyn MemoryBackend) {
    let result = b
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "Architecture uses hexagonal pattern".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: Some("manual".into()),
        })
        .await
        .unwrap();

    assert_eq!(result.action, StoreAction::Stored);
    assert_eq!(result.fact.content, "Architecture uses hexagonal pattern");
    assert_eq!(result.fact.section, Section::Architecture);
    assert_eq!(result.fact.status, FactStatus::Active);
    assert_eq!(result.fact.reinforcement_count, 1);
    assert!(result.fact.content_hash.is_some());

    // Get by ID
    let fetched = b.get_fact(&result.fact.id).await.unwrap().unwrap();
    assert_eq!(fetched.id, result.fact.id);
    assert_eq!(fetched.content, "Architecture uses hexagonal pattern");
}

async fn test_store_dedup(b: &dyn MemoryBackend) {
    let r1 = b
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "Dedup test fact".into(),
            section: Section::Decisions,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();
    assert_eq!(r1.action, StoreAction::Stored);

    // Same content again — should deduplicate (reinforce)
    let r2 = b
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "Dedup test fact".into(),
            section: Section::Decisions,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();
    assert!(
        r2.action == StoreAction::Reinforced || r2.action == StoreAction::Deduplicated,
        "expected dedup or reinforce, got {:?}",
        r2.action
    );
    assert_eq!(r2.fact.id, r1.fact.id, "should return same fact ID");
}

async fn test_list_facts(b: &dyn MemoryBackend) {
    // Store facts in different sections
    b.store_fact(StoreFact {
        mind: "list-test".into(),
        content: "List test constraint".into(),
        section: Section::Constraints,
        decay_profile: DecayProfileName::Standard,
        source: None,
    })
    .await
    .unwrap();

    b.store_fact(StoreFact {
        mind: "list-test".into(),
        content: "List test pattern".into(),
        section: Section::PatternsConventions,
        decay_profile: DecayProfileName::Standard,
        source: None,
    })
    .await
    .unwrap();

    // List all
    let all = b
        .list_facts("list-test", FactFilter::default())
        .await
        .unwrap();
    assert!(
        all.len() >= 2,
        "expected at least 2 facts, got {}",
        all.len()
    );

    // Filter by section
    let constraints = b
        .list_facts(
            "list-test",
            FactFilter {
                section: Some(Section::Constraints),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        constraints
            .iter()
            .all(|f| f.section == Section::Constraints)
    );
}

async fn test_reinforce(b: &dyn MemoryBackend) {
    let stored = b
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "Reinforce me please".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();
    assert_eq!(stored.fact.reinforcement_count, 1);

    let reinforced = b.reinforce_fact(&stored.fact.id).await.unwrap();
    assert_eq!(reinforced.reinforcement_count, 2);
    assert_eq!(reinforced.id, stored.fact.id);
}

async fn test_archive(b: &dyn MemoryBackend) {
    let stored = b
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "Archive me".into(),
            section: Section::KnownIssues,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();

    let count = b.archive_facts(&[&stored.fact.id]).await.unwrap();
    assert_eq!(count, 1);

    // get_fact should return None for archived facts (default filter)
    let fetched = b.get_fact(&stored.fact.id).await.unwrap();
    assert!(
        fetched.is_none(),
        "archived fact should not be returned by get_fact"
    );

    // But listing with archived filter should find it
    let archived = b
        .list_facts(
            "test",
            FactFilter {
                status: Some(FactStatus::Archived),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(archived.iter().any(|f| f.id == stored.fact.id));
}

async fn test_supersede(b: &dyn MemoryBackend) {
    let original = b
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "Old fact".into(),
            section: Section::Decisions,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();

    let replacement = b
        .supersede_fact(
            &original.fact.id,
            StoreFact {
                mind: "test".into(),
                content: "New improved fact".into(),
                section: Section::Decisions,
                decay_profile: DecayProfileName::Standard,
                source: None,
            },
        )
        .await
        .unwrap();

    assert_ne!(replacement.id, original.fact.id);
    assert_eq!(replacement.content, "New improved fact");

    // Original should be gone from default get
    let old = b.get_fact(&original.fact.id).await.unwrap();
    assert!(
        old.is_none(),
        "superseded fact should not be returned by get_fact"
    );
}

async fn test_superseding_fact(b: &dyn MemoryBackend) {
    let original = b
        .store_fact(store_request("superseding-lookup", "Original lineage"))
        .await
        .unwrap();
    assert!(
        b.superseding_fact(&original.fact.id)
            .await
            .unwrap()
            .is_none()
    );
    let replacement = b
        .supersede_fact(
            &original.fact.id,
            store_request("superseding-lookup", "Replacement lineage"),
        )
        .await
        .unwrap();
    assert_eq!(
        b.superseding_fact(&original.fact.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        replacement.id
    );
    let current = b
        .supersede_fact(
            &replacement.id,
            store_request("superseding-lookup", "Current lineage"),
        )
        .await
        .unwrap();
    assert_eq!(
        b.superseding_fact(&original.fact.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        current.id
    );
    assert!(b.superseding_fact("unknown").await.unwrap().is_none());
}

async fn test_supersede_with_existing(b: &dyn MemoryBackend) {
    let original = b
        .store_fact(store_request(
            "supersede-existing",
            "Obsolete alias content",
        ))
        .await
        .unwrap()
        .fact;
    let replacement = b
        .store_fact(store_request(
            "supersede-existing",
            "Converged alias content",
        ))
        .await
        .unwrap()
        .fact;
    let reinforcement_count = replacement.reinforcement_count;
    let mutation = MemoryMutation::SupersedeFactWithExisting {
        fact: FactPrecondition {
            id: original.id.clone(),
            expected_version: original.version,
        },
        replacement: FactPrecondition {
            id: replacement.id.clone(),
            expected_version: replacement.version,
        },
    };
    let first = b
        .apply_mutation("supersede-with-existing", mutation.clone())
        .await
        .unwrap();
    assert!(!first.replayed);
    let active = b
        .list_facts("supersede-existing", FactFilter::default())
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, replacement.id);
    assert_eq!(active[0].reinforcement_count, reinforcement_count);
    assert_eq!(
        b.superseding_fact(&original.id).await.unwrap().unwrap().id,
        replacement.id
    );
    let replay = b
        .apply_mutation("supersede-with-existing", mutation)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        b.get_fact(&replacement.id)
            .await
            .unwrap()
            .unwrap()
            .reinforcement_count,
        reinforcement_count
    );
}

async fn test_vault_source_lineage_fallback(b: &dyn MemoryBackend) {
    let vault_request = |content: &str, source: &str| StoreFact {
        mind: "vault-source-lineage".into(),
        content: content.into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: Some(source.into()),
    };
    let original = b
        .store_fact(vault_request("Shared v1", "codex-vault:alias-a"))
        .await
        .unwrap()
        .fact;
    let first_change = b
        .supersede_fact(
            &original.id,
            vault_request("Changed v1", "codex-vault:alias-a"),
        )
        .await
        .unwrap();
    let shared = b
        .store_fact(vault_request("Shared restored", "codex-vault:alias-b"))
        .await
        .unwrap()
        .fact;
    let converge_first = MemoryMutation::SupersedeFactWithExisting {
        fact: FactPrecondition {
            id: first_change.id.clone(),
            expected_version: first_change.version,
        },
        replacement: FactPrecondition {
            id: shared.id.clone(),
            expected_version: shared.version,
        },
    };
    b.apply_mutation("vault-source-converge-first", converge_first)
        .await
        .unwrap();
    let shared = b.get_fact(&shared.id).await.unwrap().unwrap();
    let second_change = b
        .store_fact(vault_request("Changed v2", "codex-vault:alias-a"))
        .await
        .unwrap()
        .fact;
    b.apply_mutation(
        "vault-source-converge-second",
        MemoryMutation::SupersedeFactWithExisting {
            fact: FactPrecondition {
                id: second_change.id.clone(),
                expected_version: second_change.version,
            },
            replacement: FactPrecondition {
                id: shared.id.clone(),
                expected_version: shared.version,
            },
        },
    )
    .await
    .unwrap();
    for historical in [&original, &first_change, &second_change] {
        assert_eq!(
            b.superseding_fact(&historical.id)
                .await
                .unwrap()
                .unwrap()
                .id,
            shared.id
        );
    }
    assert!(b.superseding_fact(&shared.id).await.unwrap().is_none());

    let manual_original = b
        .store_fact(vault_request("Manual original", "manual"))
        .await
        .unwrap()
        .fact;
    let manual_change = b
        .supersede_fact(
            &manual_original.id,
            vault_request("Manual change", "manual"),
        )
        .await
        .unwrap();
    let manual_target = b
        .store_fact(vault_request("Manual target", "other"))
        .await
        .unwrap()
        .fact;
    b.apply_mutation(
        "manual-converge-first",
        MemoryMutation::SupersedeFactWithExisting {
            fact: FactPrecondition {
                id: manual_change.id.clone(),
                expected_version: manual_change.version,
            },
            replacement: FactPrecondition {
                id: manual_target.id.clone(),
                expected_version: manual_target.version,
            },
        },
    )
    .await
    .unwrap();
    let manual_target = b.get_fact(&manual_target.id).await.unwrap().unwrap();
    let latest_manual = b
        .store_fact(vault_request("Latest manual", "manual"))
        .await
        .unwrap()
        .fact;
    b.apply_mutation(
        "manual-converge-second",
        MemoryMutation::SupersedeFactWithExisting {
            fact: FactPrecondition {
                id: latest_manual.id,
                expected_version: latest_manual.version,
            },
            replacement: FactPrecondition {
                id: manual_target.id,
                expected_version: manual_target.version,
            },
        },
    )
    .await
    .unwrap();
    assert!(
        b.superseding_fact(&manual_original.id)
            .await
            .unwrap()
            .is_none()
    );
}

async fn test_fts_search(b: &dyn MemoryBackend) {
    b.store_fact(StoreFact {
        mind: "search-test".into(),
        content: "The authentication system uses JWT tokens with RSA256 signing".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    })
    .await
    .unwrap();

    b.store_fact(StoreFact {
        mind: "search-test".into(),
        content: "Database migrations run automatically on startup".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    })
    .await
    .unwrap();

    let results = b
        .fts_search("search-test", "authentication JWT", 10)
        .await
        .unwrap();
    assert!(!results.is_empty(), "FTS should find auth fact");
    assert_eq!(
        results[0].fact.content,
        "The authentication system uses JWT tokens with RSA256 signing"
    );
}

async fn test_vector_store_and_search(b: &dyn MemoryBackend) {
    let stored = b
        .store_fact(StoreFact {
            mind: "vec-test".into(),
            content: "Vector test fact".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();

    // Store an embedding
    let embedding = vec![1.0f32, 0.0, 0.0, 0.5];
    let space = EmbeddingSpace {
        model: "test-model".into(),
        revision: "fixture-v1".into(),
        preprocessing: "raw-v1".into(),
        dimensions: 4,
    };
    b.apply_mutation(
        "vec-test-index",
        MemoryMutation::StoreIdentifiedEmbedding {
            fact: FactPrecondition {
                id: stored.fact.id.clone(),
                expected_version: stored.fact.version,
            },
            embedding: IdentifiedEmbedding {
                space: space.clone(),
                values: embedding,
            },
        },
    )
    .await
    .unwrap();

    // Search with similar vector
    let query = vec![0.9f32, 0.1, 0.0, 0.4];
    let results = b
        .search_identified(
            "vec-test",
            &IdentifiedEmbedding {
                space,
                values: query,
            },
            10,
            0.5,
            &Default::default(),
            &|| false,
        )
        .await
        .unwrap()
        .results;
    assert!(
        !results.is_empty(),
        "should find the fact by vector similarity"
    );
    assert!(
        results[0].similarity > 0.9,
        "similarity should be high: {}",
        results[0].similarity
    );

    // Check embedding metadata
    let meta = b.embedding_metadata("vec-test").await.unwrap().unwrap();
    assert_eq!(meta.model_name, "test-model");
    assert_eq!(meta.dims, 4);
}

async fn test_unidentified_vector_query_rejected(b: &dyn MemoryBackend) {
    // Store a fact with a 4-dim embedding
    let stored = b
        .store_fact(StoreFact {
            mind: "dim-test".into(),
            content: "Dim mismatch test".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();
    b.store_embedding(&stored.fact.id, "test-4d", &[1.0, 0.0, 0.0, 0.0])
        .await
        .unwrap();

    // The legacy query cannot establish a model identity, irrespective of dimensions.
    let result = b.vector_search("dim-test", &[1.0, 0.0], 10, 0.0).await;
    match result {
        Err(MemoryError::EmbeddingIdentityRequired) => {}
        other => panic!("expected EmbeddingIdentityRequired, got {other:?}"),
    }
}

async fn test_edges(b: &dyn MemoryBackend) {
    let f1 = b
        .store_fact(StoreFact {
            mind: "edge-test".into(),
            content: "Edge source fact".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();

    let f2 = b
        .store_fact(StoreFact {
            mind: "edge-test".into(),
            content: "Edge target fact".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();

    let edge = b
        .create_edge(CreateEdge {
            source_id: f1.fact.id.clone(),
            target_id: f2.fact.id.clone(),
            relation: "depends_on".into(),
            description: Some("F1 depends on F2".into()),
        })
        .await
        .unwrap();

    assert_eq!(edge.relation, "depends_on");

    let edges = b.get_edges("edge-test", &f1.fact.id).await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].target_id, f2.fact.id);
}

async fn test_episodes(b: &dyn MemoryBackend) {
    b.store_episode(StoreEpisode {
        mind: "ep-test".into(),
        title: "First session".into(),
        narrative: "We built the memory system".into(),
        date: Some("2026-03-18".into()),
        affected_nodes: vec!["memory-crate-interface".into()],
        affected_changes: vec![],
        files_changed: vec!["core/crates/omegon-memory/src/lib.rs".into()],
        tags: vec!["architecture".into()],
        tool_calls_count: Some(42),
        formation: None,
    })
    .await
    .unwrap();

    let episodes = b.list_episodes("ep-test", 10).await.unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].title, "First session");
    // Note: tool_calls_count may not survive round-trip through all backends
    // (sqlite episodes table doesn't have this column — it's metadata).

    // Search
    let results = b
        .search_episodes("ep-test", "memory system", 10)
        .await
        .unwrap();
    assert!(!results.is_empty());
}

async fn test_jsonl_round_trip(b: &dyn MemoryBackend) {
    // Store some data
    b.store_fact(StoreFact {
        mind: "jsonl-test".into(),
        content: "JSONL round trip fact".into(),
        section: Section::Specs,
        decay_profile: DecayProfileName::Standard,
        source: None,
    })
    .await
    .unwrap();

    // Export
    let jsonl = b.export_jsonl("jsonl-test").await.unwrap();
    assert!(!jsonl.is_empty(), "export should produce output");
    assert!(jsonl.contains("JSONL round trip fact"));

    // Each line should be valid JSON
    for line in jsonl.lines() {
        let _: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
    }
}

async fn test_jsonl_version_conflict(b: &dyn MemoryBackend) {
    // Store a fact at version N
    let stored = b
        .store_fact(StoreFact {
            mind: "conflict-test".into(),
            content: "Version conflict fact".into(),
            section: Section::Architecture,
            decay_profile: DecayProfileName::Standard,
            source: None,
        })
        .await
        .unwrap();

    // Export, then modify the JSONL with a higher version
    let jsonl = b.export_jsonl("conflict-test").await.unwrap();

    // Import the same JSONL with a HIGHER version — should update
    let modified = jsonl
        .replace(
            &format!("\"version\":{}", stored.fact.version),
            &format!("\"version\":{}", stored.fact.version + 100),
        )
        .replace("Version conflict fact", "UPDATED content");
    let stats = b.import_jsonl(&modified).await.unwrap();
    assert!(
        stats.reinforced > 0 || stats.imported > 0,
        "higher version should update: {stats:?}"
    );

    // Import with a LOWER version — should skip
    let old_version = jsonl.replace(
        &format!("\"version\":{}", stored.fact.version),
        "\"version\":0",
    );
    let stats2 = b.import_jsonl(&old_version).await.unwrap();
    assert!(stats2.skipped > 0, "lower version should skip: {stats2:?}");
}

async fn test_stats(b: &dyn MemoryBackend) {
    let stats = b.stats("test").await.unwrap();
    // We stored several facts in "test" mind across earlier tests
    assert!(stats.active_facts > 0, "should have active facts");
    assert!(stats.total_facts >= stats.active_facts);
}

async fn test_inventory_stats(b: &dyn MemoryBackend) {
    let inventory = b.inventory_stats().await.unwrap();
    assert!(inventory.total_facts >= inventory.active_facts);
    assert_eq!(
        inventory.active_facts,
        inventory.project_facts + inventory.persona_facts + inventory.working_facts
    );
    assert!(
        inventory.active_facts > 0,
        "shared fixture has active facts"
    );
    assert!(inventory.episodes > 0, "shared fixture has episodes");
    assert!(inventory.edges > 0, "shared fixture has edges");
}

fn store_request(mind: &str, content: &str) -> StoreFact {
    StoreFact {
        mind: mind.into(),
        content: content.into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: Some("test".into()),
    }
}

async fn test_mutation_replay_and_conflict(b: &dyn MemoryBackend) {
    let mutation = MemoryMutation::StoreFact {
        request: store_request("operation-replay", "Store exactly once"),
    };
    let first = b
        .apply_mutation("operation-replay-store", mutation.clone())
        .await
        .unwrap();
    assert!(!first.replayed);
    let replay = b
        .apply_mutation("operation-replay-store", mutation)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(first.effect, replay.effect);

    let fact_id = match &first.effect {
        MemoryMutationEffect::FactStored { fact_id, .. } => fact_id.clone(),
        other => panic!("unexpected store effect: {other:?}"),
    };
    let fact = b.get_fact(&fact_id).await.unwrap().unwrap();
    assert_eq!(fact.reinforcement_count, 1, "replay must not reinforce");
    let conflict = b
        .apply_mutation(
            "operation-replay-store",
            MemoryMutation::StoreFact {
                request: store_request("operation-replay", "Different payload"),
            },
        )
        .await;
    assert!(matches!(conflict, Err(MemoryError::OperationConflict(_))));

    let bound = b
        .apply_mutation_bound(
            "operation-bound-store",
            "tool-payload-a",
            MemoryMutation::StoreFact {
                request: store_request("operation-bound", "Bound payload exactly once"),
            },
        )
        .await
        .unwrap();
    let receipt = b
        .mutation_receipt("operation-bound-store", "tool-payload-a")
        .await
        .unwrap()
        .expect("bound receipt");
    assert_eq!(bound.effect, receipt.effect);
    assert!(receipt.replayed);
    assert!(matches!(
        b.mutation_receipt("operation-bound-store", "tool-payload-b")
            .await,
        Err(MemoryError::OperationConflict(_))
    ));

    let jsonl = serde_json::to_string(&JsonlRecord::Fact(JsonlFact {
        operational: None,
        id: "operation-jsonl-fact".into(),
        mind: "operation-jsonl".into(),
        content: "Imported exactly once".into(),
        section: Section::Architecture,
        status: FactStatus::Active,
        created_at: "2026-01-01T00:00:00Z".into(),
        source: Some("test".into()),
        content_hash: None,
        supersedes: None,
        version: 41,
        decay_profile: DecayProfileName::Standard,
        persona_id: None,
        layer: "project".into(),
        tags: vec![],
    }))
    .unwrap();
    let import = MemoryMutation::ImportJsonl {
        jsonl: jsonl.clone(),
    };
    let imported = b
        .apply_mutation("operation-replay-jsonl", import.clone())
        .await
        .unwrap();
    let imported_replay = b
        .apply_mutation("operation-replay-jsonl", import)
        .await
        .unwrap();
    assert!(!imported.replayed);
    assert!(imported_replay.replayed);
    assert_eq!(imported.effect, imported_replay.effect);
    assert!(matches!(
        imported.effect,
        MemoryMutationEffect::JsonlImported {
            imported: 1,
            reinforced: 0,
            ..
        }
    ));
    let import_conflict = b
        .apply_mutation(
            "operation-replay-jsonl",
            MemoryMutation::ImportJsonl {
                jsonl: format!("{jsonl}\n{{\"invalid\":true}}"),
            },
        )
        .await;
    assert!(matches!(
        import_conflict,
        Err(MemoryError::OperationConflict(_))
    ));

    let supersede = MemoryMutation::SupersedeFact {
        fact: FactPrecondition {
            id: fact.id.clone(),
            expected_version: fact.version,
        },
        replacement: store_request("operation-replay", "Replacement exactly once"),
    };
    let superseded = b
        .apply_mutation("operation-replay-supersede", supersede.clone())
        .await
        .unwrap();
    let superseded_replay = b
        .apply_mutation("operation-replay-supersede", supersede)
        .await
        .unwrap();
    assert!(superseded_replay.replayed);
    assert_eq!(superseded.effect, superseded_replay.effect);
    let archived_store_replay = b
        .apply_mutation(
            "operation-replay-store",
            MemoryMutation::StoreFact {
                request: store_request("operation-replay", "Store exactly once"),
            },
        )
        .await
        .unwrap();
    assert!(archived_store_replay.replayed);
    assert_eq!(first.effect, archived_store_replay.effect);
    let replacement_id = match &superseded.effect {
        MemoryMutationEffect::FactSuperseded { replacement, .. } => replacement.id.clone(),
        other => panic!("unexpected supersede effect: {other:?}"),
    };
    assert_eq!(
        b.list_facts("operation-replay", FactFilter::default())
            .await
            .unwrap()
            .len(),
        1,
        "supersede replay must not create another replacement"
    );
    let exported = b.export_jsonl("operation-replay").await.unwrap();
    let replacement = exported
        .lines()
        .filter_map(|line| serde_json::from_str::<JsonlRecord>(line).ok())
        .find_map(|record| match record {
            JsonlRecord::Fact(fact) if fact.id == replacement_id => Some(fact),
            _ => None,
        })
        .expect("replacement must be exported");
    assert_eq!(replacement.supersedes.as_deref(), Some(fact.id.as_str()));

    let source = b
        .store_fact(store_request("operation-edge", "Edge source"))
        .await
        .unwrap();
    let target = b
        .store_fact(store_request("operation-edge", "Edge target"))
        .await
        .unwrap();
    let edge_mutation = MemoryMutation::CreateEdge {
        mind: "operation-edge".into(),
        request: CreateEdge {
            source_id: source.fact.id.clone(),
            target_id: target.fact.id.clone(),
            relation: "depends_on".into(),
            description: None,
        },
    };
    let edge = b
        .apply_mutation("operation-replay-edge", edge_mutation.clone())
        .await
        .unwrap();
    let edge_replay = b
        .apply_mutation("operation-replay-edge", edge_mutation)
        .await
        .unwrap();
    assert_eq!(edge.effect, edge_replay.effect);
    assert_eq!(
        b.get_edges("operation-edge", &source.fact.id)
            .await
            .unwrap()
            .len(),
        1
    );
    let other = b
        .store_fact(store_request("other-mind", "Cross-mind endpoint"))
        .await
        .unwrap();
    let cross_mind = b
        .apply_mutation(
            "operation-cross-mind-edge",
            MemoryMutation::CreateEdge {
                mind: "operation-edge".into(),
                request: CreateEdge {
                    source_id: source.fact.id.clone(),
                    target_id: other.fact.id,
                    relation: "invalid".into(),
                    description: None,
                },
            },
        )
        .await;
    assert!(matches!(cross_mind, Err(MemoryError::InvalidMutation(_))));
    let active_edge_count = b.inventory_stats().await.unwrap().edges;
    b.apply_mutation(
        "operation-edge-archive-target",
        MemoryMutation::TransitionFacts {
            facts: vec![FactPrecondition {
                id: target.fact.id,
                expected_version: target.fact.version,
            }],
            status: FactStatus::Archived,
        },
    )
    .await
    .unwrap();
    assert!(
        b.get_edges("operation-edge", &source.fact.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        b.inventory_stats().await.unwrap().edges,
        active_edge_count,
        "inventory compatibility counts edges even after an endpoint is archived"
    );

    let episode_mutation = MemoryMutation::StoreEpisode {
        request: StoreEpisode {
            mind: "operation-episode".into(),
            title: "Exactly once".into(),
            narrative: "Episode replay".into(),
            date: Some("2026-01-01".into()),
            affected_nodes: vec![],
            affected_changes: vec![],
            files_changed: vec![],
            tags: vec![],
            tool_calls_count: None,
            formation: None,
        },
    };
    let episode = b
        .apply_mutation("operation-replay-episode", episode_mutation.clone())
        .await
        .unwrap();
    let episode_replay = b
        .apply_mutation("operation-replay-episode", episode_mutation)
        .await
        .unwrap();
    assert_eq!(episode.effect, episode_replay.effect);
    assert_eq!(
        b.list_episodes("operation-episode", 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

async fn test_targeted_mutation_version_conflict(b: &dyn MemoryBackend) {
    let stored = b
        .store_fact(store_request("operation-conflict", "Versioned target"))
        .await
        .unwrap();
    let result = b
        .apply_mutation(
            "operation-stale-reinforce",
            MemoryMutation::ReinforceFact {
                fact: FactPrecondition {
                    id: stored.fact.id.clone(),
                    expected_version: stored.fact.version + 1,
                },
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(MemoryError::FactVersionConflict { .. })
    ));
    let unchanged = b.get_fact(&stored.fact.id).await.unwrap().unwrap();
    assert_eq!(unchanged.reinforcement_count, 1);
    assert_eq!(unchanged.version, stored.fact.version);
}

async fn test_duplicate_target_and_nonfinite_embedding_rejected(b: &dyn MemoryBackend) {
    let stored = b
        .store_fact(store_request("invalid-mutation", "Stable target"))
        .await
        .unwrap();
    let precondition = FactPrecondition {
        id: stored.fact.id.clone(),
        expected_version: stored.fact.version,
    };
    let duplicate = b
        .apply_mutation(
            "duplicate-transition",
            MemoryMutation::TransitionFacts {
                facts: vec![precondition.clone(), precondition.clone()],
                status: FactStatus::Archived,
            },
        )
        .await;
    assert!(matches!(duplicate, Err(MemoryError::InvalidMutation(_))));
    assert!(b.get_fact(&stored.fact.id).await.unwrap().is_some());

    let direct = b
        .store_embedding(&stored.fact.id, "non-finite", &[f32::NAN])
        .await;
    assert!(matches!(direct, Err(MemoryError::InvalidMutation(_))));
    let managed = b
        .apply_mutation(
            "non-finite-embedding",
            MemoryMutation::StoreEmbedding {
                fact: precondition,
                model_name: "non-finite".into(),
                embedding: vec![f32::INFINITY],
            },
        )
        .await;
    assert!(matches!(managed, Err(MemoryError::InvalidMutation(_))));
}

async fn test_jsonl_batch_rollback(b: &dyn MemoryBackend) {
    let fact = JsonlRecord::Fact(JsonlFact {
        operational: None,
        id: "rollback-fact".into(),
        mind: "jsonl-rollback".into(),
        content: "Must roll back".into(),
        section: Section::Architecture,
        status: FactStatus::Active,
        created_at: "2026-01-01T00:00:00Z".into(),
        source: Some("test".into()),
        content_hash: None,
        supersedes: None,
        version: 10,
        decay_profile: DecayProfileName::Standard,
        persona_id: None,
        layer: "project".into(),
        tags: vec![],
    });
    let invalid_edge = JsonlRecord::Edge(Edge {
        id: "rollback-edge".into(),
        source_id: "rollback-fact".into(),
        target_id: "missing-target".into(),
        relation: "depends_on".into(),
        description: None,
        confidence: 1.0,
        created_at: "2026-01-01T00:00:00Z".into(),
    });
    let jsonl = format!(
        "{}\n{}",
        serde_json::to_string(&fact).unwrap(),
        serde_json::to_string(&invalid_edge).unwrap()
    );
    assert!(b.import_jsonl(&jsonl).await.is_err());
    assert!(b.get_fact("rollback-fact").await.unwrap().is_none());
}

async fn test_jsonl_import_advances_lamport_clock(b: &dyn MemoryBackend) {
    let imported = JsonlRecord::Fact(JsonlFact {
        id: "lamport-high-water".into(),
        operational: None,
        mind: "lamport-high-water".into(),
        content: "Imported high version".into(),
        section: Section::Architecture,
        status: FactStatus::Active,
        created_at: "2026-01-01T00:00:00Z".into(),
        source: Some("test".into()),
        content_hash: None,
        supersedes: None,
        version: 10_000,
        decay_profile: DecayProfileName::Standard,
        persona_id: None,
        layer: "project".into(),
        tags: vec![],
    });
    b.import_jsonl(&serde_json::to_string(&imported).unwrap())
        .await
        .unwrap();
    let local = b
        .store_fact(store_request("lamport-high-water", "Local after import"))
        .await
        .unwrap();
    assert!(local.fact.version > 10_000);
}

async fn test_jsonl_rejects_unpersistable_lamport_version(b: &dyn MemoryBackend) {
    let imported = JsonlRecord::Fact(JsonlFact {
        id: "lamport-overflow".into(),
        operational: None,
        mind: "lamport-overflow".into(),
        content: "Outside SQLite integer domain".into(),
        section: Section::Architecture,
        status: FactStatus::Active,
        created_at: "2026-01-01T00:00:00Z".into(),
        source: Some("test".into()),
        content_hash: None,
        supersedes: None,
        version: i64::MAX as u64 + 1,
        decay_profile: DecayProfileName::Standard,
        persona_id: None,
        layer: "project".into(),
        tags: vec![],
    });
    let result = b
        .import_jsonl(&serde_json::to_string(&imported).unwrap())
        .await;
    assert!(matches!(result, Err(MemoryError::InvalidMutation(_))));
    assert!(b.get_fact("lamport-overflow").await.unwrap().is_none());
}

async fn test_deterministic_fts_fallback(b: &dyn MemoryBackend) {
    let created_at = crate::util::now_iso();
    let records = (0..20)
        .rev()
        .map(|index| {
            let id = format!("deterministic-{index:02}");
            JsonlRecord::Fact(JsonlFact {
                operational: None,
                id: id.clone(),
                mind: "deterministic-fts".into(),
                content: "identical fallback terms".into(),
                section: Section::Architecture,
                status: FactStatus::Active,
                created_at: created_at.clone(),
                source: Some("test".into()),
                content_hash: Some(id),
                supersedes: None,
                version: 1,
                decay_profile: DecayProfileName::Standard,
                persona_id: None,
                layer: "project".into(),
                tags: vec![],
            })
        })
        .collect::<Vec<_>>();
    let jsonl = records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    b.import_jsonl(&jsonl).await.unwrap();
    let first = b
        .fts_search("deterministic-fts", "identical fallback terms", 2)
        .await
        .unwrap();
    let second = b
        .fts_search("deterministic-fts", "identical fallback terms", 2)
        .await
        .unwrap();
    let ids = |facts: &[ScoredFact]| {
        facts
            .iter()
            .map(|fact| fact.fact.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&first),
        vec![
            "deterministic-00".to_string(),
            "deterministic-01".to_string()
        ]
    );
    assert_eq!(ids(&first), ids(&second));
}

async fn test_episode_metadata_round_trip(b: &dyn MemoryBackend) {
    let episode = b
        .store_episode(StoreEpisode {
            mind: "episode-metadata".into(),
            title: "Metadata".into(),
            narrative: "Episode metadata survives storage".into(),
            date: Some("2026-01-02".into()),
            affected_nodes: vec!["node-a".into()],
            affected_changes: vec!["change-a".into()],
            files_changed: vec!["src/lib.rs".into()],
            tags: vec!["test".into()],
            tool_calls_count: Some(3),
            formation: None,
        })
        .await
        .unwrap();
    let listed = b.list_episodes("episode-metadata", 1).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, episode.id);
    assert_eq!(listed[0].affected_nodes, vec!["node-a"]);
    assert_eq!(listed[0].affected_changes, vec!["change-a"]);
    assert_eq!(listed[0].files_changed, vec!["src/lib.rs"]);
    assert_eq!(listed[0].tags, vec!["test"]);
    assert_eq!(listed[0].tool_calls_count, Some(3));

    let today = crate::util::now_iso()[..10].to_string();
    let without_date = b
        .store_episode(StoreEpisode {
            mind: "episode-default-date".into(),
            title: "Default date".into(),
            narrative: "Date derives from the current clock".into(),
            date: None,
            affected_nodes: vec![],
            affected_changes: vec![],
            files_changed: vec![],
            tags: vec![],
            tool_calls_count: None,
            formation: None,
        })
        .await
        .unwrap();
    assert_eq!(without_date.date, today);
}
