use omegon_memory::*;

fn query(model: &str) -> IdentifiedEmbedding {
    IdentifiedEmbedding {
        space: EmbeddingSpace {
            model: model.into(),
            revision: "revision-1".into(),
            preprocessing: "raw-v1".into(),
            dimensions: 2,
        },
        values: vec![1.0, 0.0],
    }
}

async fn fact(backend: &dyn MemoryBackend, content: &str) -> Fact {
    backend
        .store_fact(StoreFact {
            mind: "wave4".into(),
            content: content.into(),
            section: Section::Architecture,
            decay_profile: Default::default(),
            source: None,
        })
        .await
        .unwrap()
        .fact
}

#[tokio::test]
async fn legacy_vectors_are_not_compared_to_an_identified_query() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let stored = fact(backend.as_ref(), "zircon authentication").await;
        backend
            .store_embedding(&stored.id, "model-a", &[1.0, 0.0])
            .await
            .unwrap();
        let found = backend
            .search_identified(
                "wave4",
                &query("model-b"),
                10,
                0.0,
                &Default::default(),
                &|| false,
            )
            .await
            .unwrap();
        assert!(
            found.results.is_empty(),
            "same dimensions do not establish embedding identity"
        );
        assert_eq!(found.diagnostics.legacy, 1);
    }
}

#[tokio::test]
async fn lexical_scores_are_named_and_conflicts_are_exposed() {
    let backend = InMemoryBackend::new();
    let a = fact(&backend, "zircon uses transactional migration").await;
    let b = fact(&backend, "a conflicting migration observation").await;
    backend
        .create_edge(CreateEdge {
            source_id: a.id.clone(),
            target_id: b.id.clone(),
            relation: "contradicts".into(),
            description: None,
        })
        .await
        .unwrap();
    let seeds = backend.fts_search("wave4", "zircon", 10).await.unwrap();
    let encoded = serde_json::to_value(&seeds[0]).unwrap();
    assert!(
        encoded["scores"]["lexical"].is_number(),
        "FTS rank needs an explicit channel label"
    );
    let expanded = service::expand_edges_filtered_cancellable(
        &backend,
        "wave4",
        seeds,
        10,
        &Default::default(),
        &|| false,
    )
    .await
    .unwrap();
    let neighbor = expanded.iter().find(|item| item.fact.id == b.id).unwrap();
    assert_eq!(
        serde_json::to_value(neighbor).unwrap()["graph_evidence"][0]["kind"],
        "contradiction"
    );
}

#[tokio::test]
async fn identified_spaces_repair_legacy_and_reject_model_revision_pipeline_or_dimension_drift() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let stored = fact(backend.as_ref(), "repairable zircon fact").await;
        backend
            .store_embedding(&stored.id, "legacy", &[1.0, 0.0])
            .await
            .unwrap();
        let embedding = query("model-a");
        assert_eq!(
            backend
                .embedding_index_state(&stored.id, &embedding.space)
                .await
                .unwrap(),
            EmbeddingIndexState::Legacy
        );
        let mutation = MemoryMutation::StoreIdentifiedEmbedding {
            fact: FactPrecondition {
                id: stored.id.clone(),
                expected_version: stored.version,
            },
            embedding: embedding.clone(),
        };
        assert!(
            !backend
                .apply_mutation("repair", mutation.clone())
                .await
                .unwrap()
                .replayed
        );
        assert!(
            backend
                .apply_mutation("repair", mutation)
                .await
                .unwrap()
                .replayed
        );
        assert_eq!(
            backend
                .get_fact(&stored.id)
                .await
                .unwrap()
                .unwrap()
                .reinforcement_count,
            stored.reinforcement_count
        );
        assert_eq!(
            backend
                .embedding_index_state(&stored.id, &embedding.space)
                .await
                .unwrap(),
            EmbeddingIndexState::Ready
        );
        let found = backend
            .search_identified("wave4", &embedding, 10, 0.0, &Default::default(), &|| false)
            .await
            .unwrap();
        assert_eq!(found.results.len(), 1);
        assert_eq!(found.results[0].scores.cosine, Some(1.0));
        for component in ["model", "revision", "preprocessing", "dimensions"] {
            let mut other = embedding.clone();
            match component {
                "model" => other.space.model = "model-b".into(),
                "revision" => other.space.revision = "revision-2".into(),
                "preprocessing" => other.space.preprocessing = "other-pipeline".into(),
                _ => {
                    other.space.dimensions = 3;
                    other.values.push(0.0);
                }
            }
            let report = backend
                .search_identified("wave4", &other, 10, 0.0, &Default::default(), &|| false)
                .await
                .unwrap();
            assert!(report.results.is_empty(), "{component}");
            assert_eq!(report.diagnostics.incompatible, 1);
        }
        assert!(matches!(
            backend.vector_search("wave4", &[1.0, 0.0], 10, 0.0).await,
            Err(MemoryError::EmbeddingIdentityRequired)
        ));
    }
}

#[tokio::test]
async fn changed_raw_content_requires_repair_and_old_versions_cannot_overwrite() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let stored = fact(backend.as_ref(), "Case Sensitive Evidence").await;
        let embedding = query("model-a");
        backend
            .apply_mutation(
                "initial",
                MemoryMutation::StoreIdentifiedEmbedding {
                    fact: FactPrecondition {
                        id: stored.id.clone(),
                        expected_version: stored.version,
                    },
                    embedding: embedding.clone(),
                },
            )
            .await
            .unwrap();
        let exported = backend.export_jsonl("wave4").await.unwrap();
        let mut row: serde_json::Value = serde_json::from_str(
            exported
                .lines()
                .find(|line| line.contains("\"_type\":\"fact\""))
                .unwrap(),
        )
        .unwrap();
        row["content"] = "case sensitive evidence".into();
        row["version"] = (stored.version + 1).into();
        backend.import_jsonl(&row.to_string()).await.unwrap();
        assert_eq!(
            backend
                .embedding_index_state(&stored.id, &embedding.space)
                .await
                .unwrap(),
            EmbeddingIndexState::Stale
        );
        let report = backend
            .search_identified("wave4", &embedding, 10, 0.0, &Default::default(), &|| false)
            .await
            .unwrap();
        assert_eq!(report.diagnostics.stale, 1);
        assert!(report.results.is_empty());
        assert!(matches!(
            backend
                .apply_mutation(
                    "stale-write",
                    MemoryMutation::StoreIdentifiedEmbedding {
                        fact: FactPrecondition {
                            id: stored.id,
                            expected_version: stored.version
                        },
                        embedding,
                    }
                )
                .await,
            Err(MemoryError::FactVersionConflict { .. })
        ));
    }
}

#[tokio::test]
async fn identified_index_survives_reopen_and_corruption_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let stored = fact(&backend, "persistent vector").await;
    let embedding = query("model-a");
    backend
        .apply_mutation(
            "index",
            MemoryMutation::StoreIdentifiedEmbedding {
                fact: FactPrecondition {
                    id: stored.id.clone(),
                    expected_version: stored.version,
                },
                embedding: embedding.clone(),
            },
        )
        .await
        .unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        backend
            .embedding_index_state(&stored.id, &embedding.space)
            .await
            .unwrap(),
        EmbeddingIndexState::Ready
    );
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "UPDATE facts_vec SET model_name='inconsistent' WHERE fact_id=?1",
        [&stored.id],
    )
    .unwrap();
    assert!(
        backend
            .embedding_index_state(&stored.id, &embedding.space)
            .await
            .is_err()
    );
    conn.execute(
        "UPDATE facts_vec SET model_name=?1 WHERE fact_id=?2",
        [&embedding.space.model, &stored.id],
    )
    .unwrap();
    conn.execute(
        "UPDATE facts_vec SET embedding=?1 WHERE fact_id=?2",
        rusqlite::params![vec![1u8, 2, 3], stored.id],
    )
    .unwrap();
    assert!(matches!(
        backend
            .search_identified("wave4", &embedding, 10, 0.0, &Default::default(), &|| false)
            .await,
        Err(MemoryError::Storage(_))
    ));
}

#[tokio::test]
async fn graph_conflicts_do_not_boost_seeds_and_history_preserves_status() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let a = fact(backend.as_ref(), "zircon current rule").await;
        let b = fact(backend.as_ref(), "zircon opposing rule").await;
        backend
            .create_edge(CreateEdge {
                source_id: a.id.clone(),
                target_id: b.id.clone(),
                relation: "contradicts".into(),
                description: None,
            })
            .await
            .unwrap();
        let seeds = backend.fts_search("wave4", "zircon", 10).await.unwrap();
        let expanded = service::expand_edges_filtered_checked(
            backend.as_ref(),
            "wave4",
            seeds.clone(),
            10,
            &Default::default(),
            &|| false,
        )
        .await
        .unwrap();
        for result in &expanded {
            assert_eq!(
                result.score,
                seeds
                    .iter()
                    .find(|seed| seed.fact.id == result.fact.id)
                    .unwrap()
                    .score
            );
            assert_eq!(
                result.graph_evidence[0].kind,
                GraphRelationKind::Contradiction
            );
        }
        backend.archive_facts(&[&a.id, &b.id]).await.unwrap();
        let filter = SearchFilter {
            context: None,
            intent: SearchIntent::Historical,
            section: None,
        };
        let seeds = backend
            .fts_search_filtered("wave4", "current", 1, &filter)
            .await
            .unwrap();
        let expanded = service::expand_edges_filtered_checked(
            backend.as_ref(),
            "wave4",
            seeds,
            10,
            &filter,
            &|| false,
        )
        .await
        .unwrap();
        assert_eq!(expanded.len(), 2);
        assert!(
            expanded
                .iter()
                .all(|item| item.fact.status == FactStatus::Archived)
        );
    }
}

#[test]
fn vector_math_is_finite_for_large_finite_values() {
    assert_eq!(
        vectors::cosine_similarity(&[f32::MAX, f32::MAX], &[f32::MAX, f32::MAX]),
        1.0
    );
    let mut invalid = query("model-a");
    invalid.values = vec![0.0, 0.0];
    assert!(invalid.validate().is_err());
}

#[tokio::test]
async fn cancelled_empty_graph_is_not_a_successful_empty_result() {
    let backend = InMemoryBackend::new();
    assert!(matches!(
        service::expand_edges_filtered_checked(
            &backend,
            "wave4",
            vec![],
            10,
            &Default::default(),
            &|| true
        )
        .await,
        Err(MemoryError::Cancelled)
    ));
}

#[tokio::test]
async fn score_labels_preserve_fused_channels_and_passive_relations() {
    let backend = InMemoryBackend::new();
    let source = fact(&backend, "zircon score labels").await;
    let embedding = query("model-a");
    backend
        .apply_mutation(
            "index",
            MemoryMutation::StoreIdentifiedEmbedding {
                fact: FactPrecondition {
                    id: source.id,
                    expected_version: source.version,
                },
                embedding: embedding.clone(),
            },
        )
        .await
        .unwrap();
    let fts = backend.fts_search("wave4", "zircon", 10).await.unwrap();
    let vector = backend
        .search_identified("wave4", &embedding, 10, 0.0, &Default::default(), &|| false)
        .await
        .unwrap();
    let mut result = rrf_merge(&fts, &vector.results, 60.0, 10).remove(0);
    result.graph_evidence.push(GraphEvidence {
        edge_id: "edge".into(),
        other_fact_id: "supporting-fact".into(),
        relation: "supported_by".into(),
        outgoing: true,
        kind: GraphRelationKind::Support,
    });
    let label = renderer::recall_score_label(&result);
    for name in [
        "lexical=",
        "cosine=",
        "rrf=",
        "self supported_by supporting-fact",
    ] {
        assert!(label.contains(name), "{label}");
    }
    assert!(!label.contains('%'));
}

#[tokio::test]
async fn supersession_direction_is_current_only_and_unknown_relations_do_not_expand() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let new = fact(backend.as_ref(), "zircon newer decision").await;
        let old = fact(backend.as_ref(), "obsolete decision").await;
        let unrelated = fact(backend.as_ref(), "unclassified neighbor").await;
        for (target, relation) in [(&old.id, "supersedes"), (&unrelated.id, "custom-unknown")] {
            backend
                .create_edge(CreateEdge {
                    source_id: new.id.clone(),
                    target_id: target.clone(),
                    relation: relation.into(),
                    description: None,
                })
                .await
                .unwrap();
        }
        let seeds = backend.fts_search("wave4", "zircon", 10).await.unwrap();
        assert_eq!(
            service::expand_edges_filtered_checked(
                backend.as_ref(),
                "wave4",
                seeds,
                10,
                &Default::default(),
                &|| false
            )
            .await
            .unwrap()
            .len(),
            1
        );
        backend.archive_facts(&[&new.id, &old.id]).await.unwrap();
        let filter = SearchFilter {
            context: None,
            intent: SearchIntent::Historical,
            section: None,
        };
        let seeds = backend
            .fts_search_filtered("wave4", "zircon", 10, &filter)
            .await
            .unwrap();
        let history = service::expand_edges_filtered_checked(
            backend.as_ref(),
            "wave4",
            seeds,
            10,
            &filter,
            &|| false,
        )
        .await
        .unwrap();
        assert_eq!(history.len(), 2);
        assert!(history.iter().any(|item| item.fact.id == old.id));
    }
}

#[tokio::test]
async fn schema9_migration_preserves_unknown_identity_and_failed_repair_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let stored = fact(&backend, "legacy index migration").await;
    backend
        .store_embedding(&stored.id, "legacy", &[1.0, 0.0])
        .await
        .unwrap();
    drop(backend);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE facts_vec DROP COLUMN space; ALTER TABLE facts_vec DROP COLUMN source_hash; DELETE FROM schema_version; INSERT INTO schema_version VALUES (9,'fixture');").unwrap();
    drop(db);
    let plan = SqliteBackend::plan_migration(&path).unwrap();
    let migrated = SqliteBackend::apply_migration(&plan).unwrap();
    assert!(migrated.backup.exists());
    let backend = SqliteBackend::open(&path).unwrap();
    let embedding = query("model-a");
    assert_eq!(
        backend
            .embedding_index_state(&stored.id, &embedding.space)
            .await
            .unwrap(),
        EmbeddingIndexState::Legacy
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_repair_receipt BEFORE INSERT ON memory_operation_receipts WHEN NEW.operation_id='failed-repair' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let mutation = MemoryMutation::StoreIdentifiedEmbedding {
        fact: FactPrecondition {
            id: stored.id.clone(),
            expected_version: stored.version,
        },
        embedding,
    };
    assert!(
        backend
            .apply_mutation("failed-repair", mutation)
            .await
            .is_err()
    );
    let identity: Option<String> = db
        .query_row(
            "SELECT space FROM facts_vec WHERE fact_id=?1",
            [&stored.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(identity.is_none());
    let receipts: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM memory_operation_receipts WHERE operation_id='failed-repair'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipts, 0);
}
