//! Exercise the real backfill CLI against a local, deterministic embedding server.
#![cfg(feature = "product")]
use omegon_memory::{
    EmbeddingIndexState, EmbeddingSpace, MemoryBackend, Section, SqliteBackend, StoreFact,
};

#[tokio::test]
async fn backfill_repairs_legacy_vectors_and_skips_ready_facts_without_reinforcement() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("ai/memory")).unwrap();
    std::fs::create_dir_all(root.join(".omegon")).unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    let app = axum::Router::new()
        .route("/api/tags", axum::routing::get(|| async { axum::Json(serde_json::json!({"models":[{"name":"fixture:latest","digest":"a".repeat(64)}]})) }))
        .route("/api/embed", axum::routing::post(|| async { axum::Json(serde_json::json!({"embeddings":[[1.0,0.0]]})) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    std::fs::write(
        root.join(".omegon/profile.json"),
        serde_json::json!({"embedUrl":format!("http://{address}"),"embedModel":"fixture"})
            .to_string(),
    )
    .unwrap();
    let path = root.join("ai/memory/facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let fact = backend
        .store_fact(StoreFact {
            mind: omegon_memory::sqlite::PRIMENSUS_MIND.into(),
            content: "legacy repair evidence".into(),
            section: Section::Architecture,
            decay_profile: Default::default(),
            source: None,
        })
        .await
        .unwrap()
        .fact;
    backend
        .store_embedding(&fact.id, "unverified-old-model", &[1.0, 0.0])
        .await
        .unwrap();
    let mut pending = omegon_memory::EmbeddingIndexingRecord {
        fact: omegon_memory::FactPrecondition {
            id: fact.id.clone(),
            expected_version: fact.version,
        },
        attempt_id: "prior-outage".into(),
        space: Some(EmbeddingSpace {
            model: "retired-model".into(),
            revision: "retired-revision".into(),
            preprocessing: "raw-v1".into(),
            dimensions: 2,
        }),
        reason: omegon_memory::EmbeddingIndexingReason::Pending,
    };
    backend
        .apply_mutation(
            "old-begin",
            omegon_memory::MemoryMutation::RecordEmbeddingIndexing {
                record: pending.clone(),
            },
        )
        .await
        .unwrap();
    pending.reason = omegon_memory::EmbeddingIndexingReason::Timeout;
    backend
        .apply_mutation(
            "old-timeout",
            omegon_memory::MemoryMutation::RecordEmbeddingIndexing {
                record: pending.clone(),
            },
        )
        .await
        .unwrap();
    drop(backend);
    for run in 0..2 {
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tokio::process::Command::new(env!("CARGO_BIN_EXE_omegon"))
                .arg("--cwd")
                .arg(root)
                .args(["embedding", "backfill"])
                .env("HOME", root.join("home"))
                .env("XDG_CONFIG_HOME", root.join("config"))
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Backfill complete"), "{stdout}");
        if run == 0 {
            let backend = SqliteBackend::open(&path).unwrap();
            assert!(
                backend
                    .embedding_indexing_record(&fact.id)
                    .await
                    .unwrap()
                    .is_none()
            );
            pending.attempt_id = "ready-but-cancelled".into();
            pending.space = None;
            pending.reason = omegon_memory::EmbeddingIndexingReason::Pending;
            backend
                .apply_mutation(
                    "ready-begin",
                    omegon_memory::MemoryMutation::RecordEmbeddingIndexing {
                        record: pending.clone(),
                    },
                )
                .await
                .unwrap();
            pending.reason = omegon_memory::EmbeddingIndexingReason::Cancelled;
            backend
                .apply_mutation(
                    "ready-cancel",
                    omegon_memory::MemoryMutation::RecordEmbeddingIndexing {
                        record: pending.clone(),
                    },
                )
                .await
                .unwrap();
            drop(backend);
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch("CREATE TRIGGER reject_ready_vector_rewrite BEFORE INSERT ON facts_vec BEGIN SELECT RAISE(ABORT, 'ready vector rewritten'); END;").unwrap();
        }
        if run == 1 {
            assert!(
                stdout.contains("succeeded=0, failed=0, skipped=1"),
                "{stdout}"
            );
        }
    }
    let backend = SqliteBackend::open(&path).unwrap();
    let space = EmbeddingSpace {
        model: "ollama:fixture:latest".into(),
        revision: "a".repeat(64),
        preprocessing: "ollama-api-embed/raw-v1".into(),
        dimensions: 2,
    };
    assert_eq!(
        backend
            .embedding_index_state(&fact.id, &space)
            .await
            .unwrap(),
        EmbeddingIndexState::Ready
    );
    let after = backend.get_fact(&fact.id).await.unwrap().unwrap();
    assert!(
        backend
            .embedding_indexing_record(&fact.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(after.version, fact.version);
    assert_eq!(after.reinforcement_count, fact.reinforcement_count);
    server.abort();
    let _ = server.await;
}
