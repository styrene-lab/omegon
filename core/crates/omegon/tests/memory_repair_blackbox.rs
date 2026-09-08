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
    assert_eq!(after.version, fact.version);
    assert_eq!(after.reinforcement_count, fact.reinforcement_count);
    server.abort();
    let _ = server.await;
}
