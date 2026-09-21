use omegon_memory::*;

fn space(revision: &str) -> EmbeddingSpace {
    EmbeddingSpace {
        model: "fixture".into(),
        revision: revision.into(),
        preprocessing: "raw-v1".into(),
        dimensions: 2,
    }
}

async fn fact(backend: &dyn MemoryBackend) -> Fact {
    backend
        .store_fact(StoreFact {
            mind: "indexing".into(),
            content: "durable indexing evidence".into(),
            section: Section::Architecture,
            decay_profile: Default::default(),
            source: None,
        })
        .await
        .unwrap()
        .fact
}

fn record(fact: &Fact, attempt: &str) -> EmbeddingIndexingRecord {
    EmbeddingIndexingRecord {
        fact: FactPrecondition {
            id: fact.id.clone(),
            expected_version: fact.version,
        },
        attempt_id: attempt.into(),
        space: Some(space("revision-a")),
        reason: EmbeddingIndexingReason::Pending,
    }
}

async fn write(backend: &dyn MemoryBackend, id: &str, record: EmbeddingIndexingRecord) {
    backend
        .apply_mutation(id, MemoryMutation::RecordEmbeddingIndexing { record })
        .await
        .unwrap();
}

fn complete(record: &EmbeddingIndexingRecord, revision: &str) -> MemoryMutation {
    MemoryMutation::CompleteEmbeddingIndexing {
        fact: record.fact.clone(),
        attempt_id: record.attempt_id.clone(),
        embedding: IdentifiedEmbedding {
            space: space(revision),
            values: vec![1.0, 0.0],
        },
    }
}

#[tokio::test]
async fn indexing_attempts_preserve_facts_reject_late_callbacks_and_repair_atomically() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let fact = fact(backend.as_ref()).await;
        assert_eq!(
            backend
                .embedding_indexing_summary("indexing")
                .await
                .unwrap()
                .untracked,
            1
        );
        let first = record(&fact, "attempt-a");
        write(backend.as_ref(), "begin-a", first.clone()).await;
        let mut failed = first.clone();
        failed.reason = EmbeddingIndexingReason::Cancelled;
        write(backend.as_ref(), "cancel-a", failed.clone()).await;
        let summary = backend
            .embedding_indexing_summary("indexing")
            .await
            .unwrap();
        assert_eq!(
            (
                summary.pending,
                summary.retryable,
                summary.terminal,
                summary.untracked
            ),
            (1, 1, 0, 0)
        );
        assert_eq!(summary.reasons[&EmbeddingIndexingReason::Cancelled], 1);
        assert_eq!(
            backend
                .fts_search("indexing", "durable", 5)
                .await
                .unwrap()
                .len(),
            1
        );
        let second = record(&fact, "attempt-b");
        write(backend.as_ref(), "begin-b", second.clone()).await;
        assert!(
            backend
                .apply_mutation(
                    "late-failure",
                    MemoryMutation::RecordEmbeddingIndexing { record: failed }
                )
                .await
                .is_err()
        );
        assert!(
            backend
                .apply_mutation("late-success", complete(&first, "revision-a"))
                .await
                .is_err()
        );
        assert!(
            backend
                .apply_mutation("wrong-space", complete(&second, "revision-b"))
                .await
                .is_err()
        );
        assert_eq!(
            backend.embedding_indexing_record(&fact.id).await.unwrap(),
            Some(second.clone())
        );
        assert_eq!(
            backend
                .embedding_index_state(&fact.id, &space("revision-a"))
                .await
                .unwrap(),
            EmbeddingIndexState::Missing
        );
        let completion = complete(&second, "revision-a");
        assert!(
            !backend
                .apply_mutation("complete-b", completion.clone())
                .await
                .unwrap()
                .replayed
        );
        assert!(
            backend
                .apply_mutation("complete-b", completion)
                .await
                .unwrap()
                .replayed
        );
        assert_eq!(
            backend.embedding_indexing_record(&fact.id).await.unwrap(),
            None
        );
        assert_eq!(
            backend
                .embedding_indexing_summary("indexing")
                .await
                .unwrap(),
            EmbeddingIndexingSummary::default()
        );
        assert_eq!(
            backend
                .embedding_index_state(&fact.id, &space("revision-a"))
                .await
                .unwrap(),
            EmbeddingIndexState::Ready
        );
        let after = backend.get_fact(&fact.id).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            serde_json::to_value(fact).unwrap(),
            "indexing must not mutate or reinforce the fact"
        );
    }
}

#[tokio::test]
async fn indexing_old_versions_cannot_complete_or_overwrite_new_attempts() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let original = fact(backend.as_ref()).await;
        let old = record(&original, "old");
        write(backend.as_ref(), "old-begin", old.clone()).await;
        backend.reinforce_fact(&original.id).await.unwrap();
        let newer = backend.get_fact(&original.id).await.unwrap().unwrap();
        assert_ne!(newer.version, original.version);
        let summary = backend
            .embedding_indexing_summary("indexing")
            .await
            .unwrap();
        assert_eq!(summary.reasons[&EmbeddingIndexingReason::SourceChanged], 1);
        assert_eq!(summary.terminal, 1);
        assert!(
            backend
                .apply_mutation("old-complete", complete(&old, "revision-a"))
                .await
                .is_err()
        );
        let new = record(&newer, "new");
        write(backend.as_ref(), "new-begin", new.clone()).await;
        let mut late = old.clone();
        late.reason = EmbeddingIndexingReason::Timeout;
        assert!(
            backend
                .apply_mutation(
                    "old-timeout",
                    MemoryMutation::RecordEmbeddingIndexing { record: late }
                )
                .await
                .is_err()
        );
        backend
            .apply_mutation("new-complete", complete(&new, "revision-a"))
            .await
            .unwrap();
        assert_eq!(
            backend.get_fact(&newer.id).await.unwrap().unwrap().version,
            newer.version
        );
    }
}

#[tokio::test]
async fn indexing_pending_timeout_and_completed_repair_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let fact = fact(&backend).await;
    let mut pending = record(&fact, "crash-window");
    write(&backend, "begin", pending.clone()).await;
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        backend.embedding_indexing_record(&fact.id).await.unwrap(),
        Some(pending.clone())
    );
    pending.reason = EmbeddingIndexingReason::Timeout;
    write(&backend, "timeout", pending.clone()).await;
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        backend.embedding_indexing_record(&fact.id).await.unwrap(),
        Some(pending.clone())
    );
    backend
        .apply_mutation("repair", complete(&pending, "revision-a"))
        .await
        .unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        backend.embedding_indexing_record(&fact.id).await.unwrap(),
        None
    );
    assert_eq!(
        backend
            .embedding_index_state(&fact.id, &space("revision-a"))
            .await
            .unwrap(),
        EmbeddingIndexState::Ready
    );
    assert_eq!(
        serde_json::to_value(backend.get_fact(&fact.id).await.unwrap().unwrap()).unwrap(),
        serde_json::to_value(fact).unwrap()
    );
}

#[tokio::test]
async fn schema13_migration_adds_indexing_without_changing_facts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let fact = fact(&backend).await;
    drop(backend);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("DROP TABLE embedding_indexing; DELETE FROM schema_version; INSERT INTO schema_version VALUES (13,'fixture');").unwrap();
    drop(db);
    assert!(SqliteBackend::open(&path).is_err());
    let plan = SqliteBackend::plan_migration(&path).unwrap();
    SqliteBackend::apply_migration(&plan).unwrap();
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        backend
            .embedding_indexing_summary("indexing")
            .await
            .unwrap()
            .untracked,
        1
    );
    write(
        &backend,
        "post-migration",
        record(&fact, "migration-attempt"),
    )
    .await;
    assert_eq!(
        serde_json::to_value(backend.get_fact(&fact.id).await.unwrap().unwrap()).unwrap(),
        serde_json::to_value(fact).unwrap()
    );
}

#[tokio::test]
async fn indexing_completion_rolls_back_vector_and_receipt_when_pending_cleanup_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let fact = fact(&backend).await;
    let pending = record(&fact, "atomic");
    write(&backend, "begin", pending.clone()).await;
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_index_cleanup BEFORE DELETE ON embedding_indexing BEGIN SELECT RAISE(ABORT, 'fixture cleanup failure'); END;").unwrap();
    assert!(
        backend
            .apply_mutation("complete", complete(&pending, "revision-a"))
            .await
            .is_err()
    );
    assert_eq!(
        backend.embedding_indexing_record(&fact.id).await.unwrap(),
        Some(pending.clone())
    );
    assert_eq!(
        backend
            .embedding_index_state(&fact.id, &space("revision-a"))
            .await
            .unwrap(),
        EmbeddingIndexState::Missing
    );
    db.execute_batch("DROP TRIGGER fail_index_cleanup;")
        .unwrap();
    assert!(
        !backend
            .apply_mutation("complete", complete(&pending, "revision-a"))
            .await
            .unwrap()
            .replayed
    );
    assert_eq!(
        backend.embedding_indexing_record(&fact.id).await.unwrap(),
        None
    );
}
