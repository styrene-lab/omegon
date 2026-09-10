use omegon_memory::*;

fn mutation() -> MemoryMutation {
    MemoryMutation::StoreLifecycleInference {
        request: StoreFact {
            mind: "test".into(),
            content: "zircon inferred success".into(),
            section: Section::Decisions,
            source: Some("lifecycle:design-tree".into()),
            decay_profile: Default::default(),
        },
        inference: Box::new(LifecycleInference {
            confirmation: None,
            source_kind: "design-tree".into(),
            artifact_ref_type: Some("design".into()),
            artifact_ref_path: Some("docs/design/zircon.md".into()),
            artifact_ref_sub: Some("unverified-summary".into()),
            proposed_supersedes: Some("original".into()),
        }),
    }
}

async fn pending(backend: &dyn MemoryBackend) -> Fact {
    backend
        .list_facts(
            "test",
            FactFilter {
                status: Some(FactStatus::Pending),
                section: None,
            },
        )
        .await
        .unwrap()
        .remove(0)
}

#[tokio::test]
async fn inference_replay_preserves_provenance_without_admitting_or_reinforcing_knowledge() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let active = backend
            .store_fact(StoreFact {
                mind: "test".into(),
                content: "zircon inferred success".into(),
                section: Section::Decisions,
                source: None,
                decay_profile: Default::default(),
            })
            .await
            .unwrap()
            .fact;
        let first = backend
            .apply_mutation("candidate", mutation())
            .await
            .unwrap();
        assert!(
            backend
                .apply_mutation("candidate", mutation())
                .await
                .unwrap()
                .replayed
        );
        let candidate = pending(backend.as_ref()).await;
        assert_ne!(candidate.id, active.id);
        assert_eq!(candidate.reinforcement_count, 0);
        assert_eq!(
            backend
                .get_fact(&active.id)
                .await
                .unwrap()
                .unwrap()
                .reinforcement_count,
            active.reinforcement_count
        );
        let inference = candidate.lifecycle_inference.as_ref().unwrap();
        assert_eq!(
            inference.artifact_ref_path.as_deref(),
            Some("docs/design/zircon.md")
        );
        assert_eq!(inference.proposed_supersedes.as_deref(), Some("original"));
        assert_eq!(candidate.superseded_by, None);
        let rendered = MarkdownRenderer.render_context(
            std::slice::from_ref(&candidate),
            &[],
            std::slice::from_ref(&candidate),
            10_000,
        );
        assert_eq!(rendered.facts_injected, 0);
        let recalled = backend.fts_search("test", "zircon", 10).await.unwrap();
        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].fact.id, active.id);
        let historical = backend
            .fts_search_filtered(
                "test",
                "zircon",
                10,
                &SearchFilter {
                    intent: SearchIntent::Historical,
                    section: None,
                },
            )
            .await
            .unwrap();
        assert!(historical.is_empty());
        assert_eq!(
            first.effect,
            backend
                .apply_mutation("candidate", mutation())
                .await
                .unwrap()
                .effect
        );
        let precondition = FactPrecondition {
            id: candidate.id.clone(),
            expected_version: candidate.version,
        };
        assert!(
            backend
                .store_embedding(&candidate.id, "legacy", &[1.0, 0.0])
                .await
                .is_err()
        );
        assert!(
            backend
                .apply_mutation(
                    "legacy-embed",
                    MemoryMutation::StoreEmbedding {
                        fact: precondition.clone(),
                        model_name: "legacy".into(),
                        embedding: vec![1.0, 0.0]
                    }
                )
                .await
                .is_err()
        );
        assert!(
            backend
                .apply_mutation(
                    "reinforce",
                    MemoryMutation::ReinforceFact {
                        fact: precondition.clone()
                    }
                )
                .await
                .is_err()
        );
        assert!(
            backend
                .apply_mutation(
                    "embed",
                    MemoryMutation::StoreIdentifiedEmbedding {
                        fact: precondition,
                        embedding: IdentifiedEmbedding {
                            space: EmbeddingSpace {
                                model: "fixture".into(),
                                revision: "v1".into(),
                                preprocessing: "raw".into(),
                                dimensions: 2
                            },
                            values: vec![1.0, 0.0],
                        }
                    }
                )
                .await
                .is_err()
        );
        let exported = backend.export_jsonl("test").await.unwrap();
        let target = InMemoryBackend::new();
        target.import_jsonl(&exported).await.unwrap();
        assert_eq!(
            serde_json::to_value(pending(&target).await).unwrap(),
            serde_json::to_value(&candidate).unwrap()
        );
        let mut forged: serde_json::Value = serde_json::from_str(
            exported
                .lines()
                .find(|line| line.contains(&candidate.id))
                .unwrap(),
        )
        .unwrap();
        forged["version"] = (candidate.version + 10).into();
        forged["status"] = "active".into();
        assert!(backend.import_jsonl(&forged.to_string()).await.is_err());
        forged
            .as_object_mut()
            .unwrap()
            .remove("lifecycle_inference");
        assert!(backend.import_jsonl(&forged.to_string()).await.is_err());
    }
}

#[tokio::test]
async fn candidate_survives_reopen_and_schema10_migration_preserves_legacy_unknowns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    backend
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "legacy".into(),
            section: Section::Architecture,
            source: None,
            decay_profile: Default::default(),
        })
        .await
        .unwrap();
    drop(backend);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE facts DROP COLUMN lifecycle_inference; DELETE FROM schema_version; INSERT INTO schema_version VALUES(10,'fixture');").unwrap();
    drop(db);
    let plan = SqliteBackend::plan_migration(&path).unwrap();
    assert!(
        SqliteBackend::apply_migration(&plan)
            .unwrap()
            .backup
            .exists()
    );
    let backend = SqliteBackend::open(&path).unwrap();
    assert!(
        backend
            .list_facts("test", Default::default())
            .await
            .unwrap()[0]
            .lifecycle_inference
            .is_none()
    );
    backend
        .apply_mutation("candidate", mutation())
        .await
        .unwrap();
    let before = backend.export_jsonl("test").await.unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
    assert!(
        backend
            .apply_mutation("candidate", mutation())
            .await
            .unwrap()
            .replayed
    );
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "UPDATE facts SET lifecycle_inference=NULL WHERE status='pending'",
        [],
    )
    .unwrap();
    assert!(backend.export_jsonl("test").await.is_err());
}

#[tokio::test]
async fn candidate_failure_is_atomic_and_vault_does_not_publish_pending_claims() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_candidate BEFORE INSERT ON memory_operation_receipts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        backend
            .apply_mutation("candidate", mutation())
            .await
            .is_err()
    );
    assert!(backend.export_jsonl("test").await.unwrap().is_empty());
    db.execute_batch("DROP TRIGGER reject_candidate;").unwrap();
    backend
        .apply_mutation("candidate", mutation())
        .await
        .unwrap();
    let vault = tempfile::tempdir().unwrap();
    let first = vault_sync::materialize_to_vault(&backend, vault.path(), "test")
        .await
        .unwrap();
    assert_eq!(first.facts_written, 0);
    assert!(!vault.path().join("ai/memory/decisions.md").exists());
    let second = vault_sync::materialize_to_vault(&backend, vault.path(), "test")
        .await
        .unwrap();
    assert_eq!(second.facts_written, 0);
    assert_eq!(second.files_changed_total, 0);
}
