use omegon_memory::*;

fn candidate() -> MemoryMutation {
    serde_json::from_value(serde_json::json!({"kind":"store_lifecycle_inference","request":{
        "mind":"test","content":"zircon inferred conclusion","section":"Decisions","decay_profile":"standard","source":null
    },"inference":{"source_kind":"design-tree","artifact_ref_type":null,"artifact_ref_path":null,"artifact_ref_sub":null,"proposed_supersedes":null}})).unwrap()
}

fn confirmation(fact: &Fact, supersedes: Option<FactPrecondition>) -> MemoryMutation {
    MemoryMutation::ConfirmLifecycleCandidate {
        candidate: FactPrecondition {
            id: fact.id.clone(),
            expected_version: fact.version,
        },
        snapshot_hash: lifecycle::candidate_snapshot_hash(fact).unwrap(),
        session_id: "session".into(),
        request_id: "review".into(),
        surface: ConfirmationSurface::NativeEvent,
        supersedes,
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
async fn reviewed_candidate_confirmation_is_version_checked_and_durable() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        backend
            .apply_mutation("candidate", candidate())
            .await
            .unwrap();
        let fact = backend
            .list_facts(
                "test",
                FactFilter {
                    status: Some(FactStatus::Pending),
                    section: None,
                },
            )
            .await
            .unwrap()
            .remove(0);
        let hash = retrieval::raw_content_hash(&serde_json::to_string(&fact).unwrap());
        let mutation: MemoryMutation =
            serde_json::from_value(serde_json::json!({"kind":"confirm_lifecycle_candidate",
            "candidate":{"id":fact.id,"expected_version":fact.version},"snapshot_hash":hash,
            "session_id":"session","request_id":"review","surface":"native_event","supersedes":null}))
            .expect("typed operator confirmation must be supported");
        backend
            .apply_mutation("review", mutation.clone())
            .await
            .unwrap();
        assert!(
            backend
                .apply_mutation("review", mutation)
                .await
                .unwrap()
                .replayed
        );
        let confirmed = backend.get_fact(&fact.id).await.unwrap().unwrap();
        let encoded = serde_json::to_value(&confirmed).unwrap();
        assert_eq!(
            encoded["lifecycle_inference"]["confirmation"]["request_id"],
            "review"
        );
        assert_eq!(confirmed.reinforcement_count, 1);
        assert_eq!(
            backend
                .fts_search("test", "zircon", 10)
                .await
                .unwrap()
                .len(),
            1
        );
        let target = SqliteBackend::in_memory().unwrap();
        target
            .import_jsonl(&backend.export_jsonl("test").await.unwrap())
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(target.get_fact(&fact.id).await.unwrap().unwrap()).unwrap(),
            encoded
        );
        backend.archive_facts(&[&fact.id]).await.unwrap();
        target
            .import_jsonl(&backend.export_jsonl("test").await.unwrap())
            .await
            .unwrap();
        let archived = target
            .list_facts(
                "test",
                FactFilter {
                    status: Some(FactStatus::Archived),
                    section: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(archived.len(), 1);
        assert!(
            archived[0]
                .lifecycle_inference
                .as_ref()
                .unwrap()
                .confirmation
                .is_some()
        );
    }
}

#[tokio::test]
async fn changed_snapshot_and_correction_versions_reject_without_partial_admission() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let old = backend
            .store_fact(StoreFact {
                mind: "test".into(),
                content: "old zircon".into(),
                section: Section::Decisions,
                source: None,
                decay_profile: Default::default(),
            })
            .await
            .unwrap()
            .fact;
        let mut proposed = candidate();
        if let MemoryMutation::StoreLifecycleInference { inference, .. } = &mut proposed {
            inference.proposed_supersedes = Some(old.id.clone());
        }
        backend.apply_mutation("candidate", proposed).await.unwrap();
        let fact = pending(backend.as_ref()).await;
        let target = Some(FactPrecondition {
            id: old.id.clone(),
            expected_version: old.version,
        });
        let mut wrong = confirmation(&fact, target.clone());
        if let MemoryMutation::ConfirmLifecycleCandidate { snapshot_hash, .. } = &mut wrong {
            *snapshot_hash = "unreviewed".into();
        }
        assert!(backend.apply_mutation("review", wrong).await.is_err());
        backend.reinforce_fact(&old.id).await.unwrap();
        assert!(matches!(
            backend
                .apply_mutation("review", confirmation(&fact, target))
                .await,
            Err(MemoryError::FactVersionConflict { .. })
        ));
        assert!(backend.get_fact(&fact.id).await.unwrap().is_none());
        assert!(backend.get_fact(&old.id).await.unwrap().is_some());
        let old = backend.get_fact(&old.id).await.unwrap().unwrap();
        let correct = confirmation(
            &fact,
            Some(FactPrecondition {
                id: old.id.clone(),
                expected_version: old.version,
            }),
        );
        backend.apply_mutation("review", correct).await.unwrap();
        assert!(backend.get_fact(&old.id).await.unwrap().is_none());
        assert_eq!(
            backend
                .get_fact(&fact.id)
                .await
                .unwrap()
                .unwrap()
                .superseded_by,
            Some(old.id)
        );
    }
}

#[tokio::test]
async fn confirmation_rolls_back_on_receipt_failure_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    backend
        .apply_mutation("candidate", candidate())
        .await
        .unwrap();
    let fact = pending(&backend).await;
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_review BEFORE INSERT ON memory_operation_receipts WHEN NEW.operation_id='review' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let mutation = confirmation(&fact, None);
    assert!(
        backend
            .apply_mutation("review", mutation.clone())
            .await
            .is_err()
    );
    assert!(backend.get_fact(&fact.id).await.unwrap().is_none());
    assert!(
        pending(&backend)
            .await
            .lifecycle_inference
            .unwrap()
            .confirmation
            .is_none()
    );
    db.execute_batch("DROP TRIGGER reject_review;").unwrap();
    backend
        .apply_mutation("review", mutation.clone())
        .await
        .unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert!(
        backend
            .apply_mutation("review", mutation)
            .await
            .unwrap()
            .replayed
    );
    let exported = backend.export_jsonl("test").await.unwrap();
    let mut forged: serde_json::Value = serde_json::from_str(&exported).unwrap();
    forged["version"] = 999.into();
    forged["content"] = "a different assertion".into();
    assert!(backend.import_jsonl(&forged.to_string()).await.is_err());
    forged = serde_json::from_str(&exported).unwrap();
    forged["version"] = 999.into();
    forged["lifecycle_inference"]["confirmation"]["request_id"] = "different-review".into();
    assert!(backend.import_jsonl(&forged.to_string()).await.is_err());
    let vault = tempfile::tempdir().unwrap();
    vault_sync::materialize_to_vault(&backend, vault.path(), "test")
        .await
        .unwrap();
    let note = vault.path().join("ai/memory/decisions.md");
    let before = std::fs::read(&note).unwrap();
    db.execute(
        "UPDATE facts SET content='tampered after confirmation' WHERE id=?1",
        [&fact.id],
    )
    .unwrap();
    assert!(
        vault_sync::materialize_to_vault(&backend, vault.path(), "test")
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&note).unwrap(), before);
}

#[tokio::test]
async fn schema11_pending_records_migrate_without_fabricating_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let backend = SqliteBackend::open(&path).unwrap();
    backend
        .apply_mutation("candidate", candidate())
        .await
        .unwrap();
    drop(backend);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "DELETE FROM schema_version; INSERT INTO schema_version VALUES(11,'fixture');",
    )
    .unwrap();
    drop(db);
    let migration =
        SqliteBackend::apply_migration(&SqliteBackend::plan_migration(&path).unwrap()).unwrap();
    assert!(migration.backup.exists());
    assert_eq!(migration.target_version, 12);
    let backend = SqliteBackend::open(&path).unwrap();
    let fact = pending(&backend).await;
    assert!(
        fact.lifecycle_inference
            .as_ref()
            .unwrap()
            .confirmation
            .is_none()
    );
    backend
        .apply_mutation("review", confirmation(&fact, None))
        .await
        .unwrap();
    assert!(backend.get_fact(&fact.id).await.unwrap().is_some());
}
