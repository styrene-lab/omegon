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
    test_fts_search(b).await;
    test_vector_store_and_search(b).await;
    test_vector_dimension_mismatch(b).await;
    test_edges(b).await;
    test_episodes(b).await;
    test_jsonl_round_trip(b).await;
    test_jsonl_version_conflict(b).await;
    test_stats(b).await;
}

async fn test_store_and_get(b: &dyn MemoryBackend) {
    let result = b.store_fact(StoreFact {
        mind: "test".into(),
        content: "Architecture uses hexagonal pattern".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: Some("manual".into()),
    }).await.unwrap();

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
    let r1 = b.store_fact(StoreFact {
        mind: "test".into(),
        content: "Dedup test fact".into(),
        section: Section::Decisions,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();
    assert_eq!(r1.action, StoreAction::Stored);

    // Same content again — should deduplicate (reinforce)
    let r2 = b.store_fact(StoreFact {
        mind: "test".into(),
        content: "Dedup test fact".into(),
        section: Section::Decisions,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();
    assert!(
        r2.action == StoreAction::Reinforced || r2.action == StoreAction::Deduplicated,
        "expected dedup or reinforce, got {:?}", r2.action
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
    }).await.unwrap();

    b.store_fact(StoreFact {
        mind: "list-test".into(),
        content: "List test pattern".into(),
        section: Section::PatternsConventions,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    // List all
    let all = b.list_facts("list-test", FactFilter::default()).await.unwrap();
    assert!(all.len() >= 2, "expected at least 2 facts, got {}", all.len());

    // Filter by section
    let constraints = b.list_facts("list-test", FactFilter {
        section: Some(Section::Constraints),
        ..Default::default()
    }).await.unwrap();
    assert!(constraints.iter().all(|f| f.section == Section::Constraints));
}

async fn test_reinforce(b: &dyn MemoryBackend) {
    let stored = b.store_fact(StoreFact {
        mind: "test".into(),
        content: "Reinforce me please".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();
    assert_eq!(stored.fact.reinforcement_count, 1);

    let reinforced = b.reinforce_fact(&stored.fact.id).await.unwrap();
    assert_eq!(reinforced.reinforcement_count, 2);
    assert_eq!(reinforced.id, stored.fact.id);
}

async fn test_archive(b: &dyn MemoryBackend) {
    let stored = b.store_fact(StoreFact {
        mind: "test".into(),
        content: "Archive me".into(),
        section: Section::KnownIssues,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    let count = b.archive_facts(&[&stored.fact.id]).await.unwrap();
    assert_eq!(count, 1);

    // get_fact should return None for archived facts (default filter)
    let fetched = b.get_fact(&stored.fact.id).await.unwrap();
    assert!(fetched.is_none(), "archived fact should not be returned by get_fact");

    // But listing with archived filter should find it
    let archived = b.list_facts("test", FactFilter {
        status: Some(FactStatus::Archived),
        ..Default::default()
    }).await.unwrap();
    assert!(archived.iter().any(|f| f.id == stored.fact.id));
}

async fn test_supersede(b: &dyn MemoryBackend) {
    let original = b.store_fact(StoreFact {
        mind: "test".into(),
        content: "Old fact".into(),
        section: Section::Decisions,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    let replacement = b.supersede_fact(&original.fact.id, StoreFact {
        mind: "test".into(),
        content: "New improved fact".into(),
        section: Section::Decisions,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    assert_ne!(replacement.id, original.fact.id);
    assert_eq!(replacement.content, "New improved fact");

    // Original should be gone from default get
    let old = b.get_fact(&original.fact.id).await.unwrap();
    assert!(old.is_none(), "superseded fact should not be returned by get_fact");
}

async fn test_fts_search(b: &dyn MemoryBackend) {
    b.store_fact(StoreFact {
        mind: "search-test".into(),
        content: "The authentication system uses JWT tokens with RSA256 signing".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    b.store_fact(StoreFact {
        mind: "search-test".into(),
        content: "Database migrations run automatically on startup".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    let results = b.fts_search("search-test", "authentication JWT", 10).await.unwrap();
    assert!(!results.is_empty(), "FTS should find auth fact");
    assert_eq!(results[0].fact.content, "The authentication system uses JWT tokens with RSA256 signing");
}

async fn test_vector_store_and_search(b: &dyn MemoryBackend) {
    let stored = b.store_fact(StoreFact {
        mind: "vec-test".into(),
        content: "Vector test fact".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    // Store an embedding
    let embedding = vec![1.0f32, 0.0, 0.0, 0.5];
    b.store_embedding(&stored.fact.id, "test-model", &embedding).await.unwrap();

    // Search with similar vector
    let query = vec![0.9f32, 0.1, 0.0, 0.4];
    let results = b.vector_search("vec-test", &query, 10, 0.5).await.unwrap();
    assert!(!results.is_empty(), "should find the fact by vector similarity");
    assert!(results[0].similarity > 0.9, "similarity should be high: {}", results[0].similarity);

    // Check embedding metadata
    let meta = b.embedding_metadata("vec-test").await.unwrap().unwrap();
    assert_eq!(meta.model_name, "test-model");
    assert_eq!(meta.dims, 4);
}

async fn test_vector_dimension_mismatch(b: &dyn MemoryBackend) {
    // Store a fact with a 4-dim embedding
    let stored = b.store_fact(StoreFact {
        mind: "dim-test".into(),
        content: "Dim mismatch test".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();
    b.store_embedding(&stored.fact.id, "test-4d", &[1.0, 0.0, 0.0, 0.0]).await.unwrap();

    // Search with wrong dimensions — should error
    let result = b.vector_search("dim-test", &[1.0, 0.0], 10, 0.0).await;
    match result {
        Err(MemoryError::EmbeddingDimensionMismatch { expected: 4, got: 2, .. }) => {},
        other => panic!("expected EmbeddingDimensionMismatch, got {other:?}"),
    }
}

async fn test_edges(b: &dyn MemoryBackend) {
    let f1 = b.store_fact(StoreFact {
        mind: "edge-test".into(),
        content: "Edge source fact".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    let f2 = b.store_fact(StoreFact {
        mind: "edge-test".into(),
        content: "Edge target fact".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    let edge = b.create_edge(CreateEdge {
        source_id: f1.fact.id.clone(),
        target_id: f2.fact.id.clone(),
        relation: "depends_on".into(),
        description: Some("F1 depends on F2".into()),
    }).await.unwrap();

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
    }).await.unwrap();

    let episodes = b.list_episodes("ep-test", 10).await.unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].title, "First session");
    // Note: tool_calls_count may not survive round-trip through all backends
    // (sqlite episodes table doesn't have this column — it's metadata).

    // Search
    let results = b.search_episodes("ep-test", "memory system", 10).await.unwrap();
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
    }).await.unwrap();

    // Export
    let jsonl = b.export_jsonl("jsonl-test").await.unwrap();
    assert!(!jsonl.is_empty(), "export should produce output");
    assert!(jsonl.contains("JSONL round trip fact"));

    // Each line should be valid JSON
    for line in jsonl.lines() {
        let _: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
    }
}

async fn test_jsonl_version_conflict(b: &dyn MemoryBackend) {
    // Store a fact at version N
    let stored = b.store_fact(StoreFact {
        mind: "conflict-test".into(),
        content: "Version conflict fact".into(),
        section: Section::Architecture,
        decay_profile: DecayProfileName::Standard,
        source: None,
    }).await.unwrap();

    // Export, then modify the JSONL with a higher version
    let jsonl = b.export_jsonl("conflict-test").await.unwrap();

    // Import the same JSONL with a HIGHER version — should update
    let modified = jsonl.replace(
        &format!("\"version\":{}", stored.fact.version),
        &format!("\"version\":{}", stored.fact.version + 100),
    ).replace("Version conflict fact", "UPDATED content");
    let stats = b.import_jsonl(&modified).await.unwrap();
    assert!(stats.reinforced > 0 || stats.imported > 0, "higher version should update: {stats:?}");

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
