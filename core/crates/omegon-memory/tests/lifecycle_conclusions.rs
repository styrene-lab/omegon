use omegon_memory::*;

fn request(content: &str) -> StoreFact {
    StoreFact {
        mind: "test".into(),
        content: content.into(),
        section: Section::Decisions,
        source: None,
        decay_profile: Default::default(),
    }
}
fn source(content: &str) -> Box<LifecycleConclusionSource> {
    Box::new(LifecycleConclusionSource {
        kind: LifecycleConclusionKind::Decision,
        artifact_path: "docs/design/zircon.md".into(),
        artifact_id: Some("zircon".into()),
        artifact_sub: "Use transactions".into(),
        artifact_sha256: "a".repeat(64),
        statement_sha256: retrieval::raw_content_hash(content),
    })
}

#[tokio::test]
async fn explicit_corrections_are_atomic_versioned_and_portable() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let old = backend
            .store_fact(request("old decision"))
            .await
            .unwrap()
            .fact;
        let mutation = MemoryMutation::StoreLifecycleConclusion {
            request: request("Use transactions: Keep corrections atomic."),
            source: source("Use transactions: Keep corrections atomic."),
            supersedes: Some(FactPrecondition {
                id: old.id.clone(),
                expected_version: old.version,
            }),
        };
        let result = backend
            .apply_mutation("correction", mutation.clone())
            .await
            .unwrap();
        assert!(
            backend
                .apply_mutation("correction", mutation)
                .await
                .unwrap()
                .replayed
        );
        let MemoryMutationEffect::FactSuperseded { replacement, .. } = result.effect else {
            panic!("correction must change lifecycle state");
        };
        let fact = backend.get_fact(&replacement.id).await.unwrap().unwrap();
        let label = renderer::recall_score_label(&ScoredFact::new(fact.clone(), 1.0, 1.0));
        assert!(label.contains(&format!("version={}", fact.version)));
        assert_eq!(fact.superseded_by.as_deref(), Some(old.id.as_str()));
        assert_eq!(
            fact.lifecycle_conclusion().unwrap().unwrap(),
            *source(&fact.content)
        );
        assert!(backend.get_fact(&old.id).await.unwrap().is_none());
        let exported = backend.export_jsonl("test").await.unwrap();
        let target = SqliteBackend::in_memory().unwrap();
        target.import_jsonl(&exported).await.unwrap();
        assert_eq!(
            target
                .get_fact(&fact.id)
                .await
                .unwrap()
                .unwrap()
                .lifecycle_conclusion()
                .unwrap(),
            fact.lifecycle_conclusion().unwrap()
        );
        let before = backend.export_jsonl("test").await.unwrap();
        let bad = MemoryMutation::StoreLifecycleConclusion {
            request: request("newer"),
            source: source("newer"),
            supersedes: Some(FactPrecondition {
                id: fact.id.clone(),
                expected_version: 0,
            }),
        };
        assert!(matches!(
            backend.apply_mutation("bad", bad).await,
            Err(MemoryError::FactVersionConflict { .. })
        ));
        assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
        let mut other = request("other scope");
        other.mind = "other".into();
        assert!(
            backend
                .apply_mutation(
                    "cross-mind",
                    MemoryMutation::StoreLifecycleConclusion {
                        request: other,
                        source: source("other scope"),
                        supersedes: Some(FactPrecondition {
                            id: fact.id.clone(),
                            expected_version: fact.version
                        })
                    }
                )
                .await
                .is_err()
        );
        assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
    }
}

#[tokio::test]
async fn attribution_aware_deduplication_does_not_drop_new_source_evidence() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let content = "Use transactions: Keep corrections atomic.";
        let legacy = backend.store_fact(request(content)).await.unwrap().fact;
        let mutation = MemoryMutation::StoreLifecycleConclusion {
            request: request(content),
            source: source(content),
            supersedes: None,
        };
        let first = backend
            .apply_mutation("first", mutation.clone())
            .await
            .unwrap();
        let second = backend.apply_mutation("second", mutation).await.unwrap();
        let MemoryMutationEffect::FactStored {
            fact_id,
            action: StoreAction::Stored,
            ..
        } = first.effect
        else {
            panic!("expected attributed store");
        };
        let MemoryMutationEffect::FactStored {
            fact_id: second_id,
            action: StoreAction::Reinforced,
            ..
        } = second.effect
        else {
            panic!("expected reuse");
        };
        assert_eq!(fact_id, second_id);
        assert_ne!(fact_id, legacy.id);
        assert_eq!(
            backend
                .get_fact(&legacy.id)
                .await
                .unwrap()
                .unwrap()
                .reinforcement_count,
            legacy.reinforcement_count
        );
        let exported = backend.export_jsonl("test").await.unwrap();
        let mut duplicate: serde_json::Value = exported
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|row| row["id"] == fact_id)
            .unwrap();
        duplicate["id"] = "!earliest".into();
        backend.import_jsonl(&duplicate.to_string()).await.unwrap();
        let third = backend
            .apply_mutation(
                "third",
                MemoryMutation::StoreLifecycleConclusion {
                    request: request(content),
                    source: source(content),
                    supersedes: None,
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(third.effect,MemoryMutationEffect::FactStored {fact_id,..} if fact_id == "!earliest")
        );
    }
}

#[tokio::test]
async fn receipt_failure_rolls_back_correction_and_reopen_preserves_attribution() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let old = backend.store_fact(request("old")).await.unwrap().fact;
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_conclusion BEFORE INSERT ON memory_operation_receipts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let mutation = MemoryMutation::StoreLifecycleConclusion {
        request: request("new"),
        source: source("new"),
        supersedes: Some(FactPrecondition {
            id: old.id.clone(),
            expected_version: old.version,
        }),
    };
    assert!(
        backend
            .apply_mutation("correction", mutation.clone())
            .await
            .is_err()
    );
    assert!(backend.get_fact(&old.id).await.unwrap().is_some());
    assert_eq!(
        backend
            .list_facts("test", Default::default())
            .await
            .unwrap()
            .len(),
        1
    );
    db.execute_batch("DROP TRIGGER reject_conclusion;").unwrap();
    backend
        .apply_mutation("correction", mutation.clone())
        .await
        .unwrap();
    let before = backend.export_jsonl("test").await.unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
    assert!(
        backend
            .apply_mutation("correction", mutation)
            .await
            .unwrap()
            .replayed
    );
}
