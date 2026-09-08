use omegon_memory::{MemoryBackend, SqliteBackend};
use serde_json::json;

fn episode_json() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/formation.json")).unwrap()
}

async fn episode_search_case(backend: &dyn MemoryBackend) {
    backend
        .import_jsonl(&episode_json().to_string())
        .await
        .unwrap();
    for query in ["\"Migration", "investigation"] {
        let found = backend
            .search_episodes("wave3", query, 1)
            .await
            .expect("literal query must remain searchable");
        assert_eq!(found.len(), 1, "missing quote/title match for {query}");
    }
    assert!(
        backend
            .search_episodes("wave3", "  ", 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn adversarial_episode_search_sqlite() {
    episode_search_case(&SqliteBackend::in_memory().unwrap()).await;
}

#[tokio::test]
async fn adversarial_episode_search_inmemory() {
    episode_search_case(&omegon_memory::InMemoryBackend::new()).await;
}

#[test]
fn adversarial_formation_rejects_contradictory_evidence_identity() {
    let original: omegon_memory::EpisodeFormation =
        serde_json::from_value(episode_json()["formation"].clone()).unwrap();
    let mut duplicate = original.clone();
    let mut item = duplicate.evidence[0].clone();
    item.event_id = "other-event-at-same-sequence".into();
    duplicate.evidence.push(item);
    assert!(
        duplicate.validate().is_err(),
        "one sequence cannot identify two source events"
    );
}

#[test]
fn adversarial_formation_rejects_false_frontier_and_control_ids() {
    let original: omegon_memory::EpisodeFormation =
        serde_json::from_value(episode_json()["formation"].clone()).unwrap();
    let mut false_frontier = original.clone();
    false_frontier.evidence[0].sequence = 8;
    assert!(
        false_frontier.validate().is_err(),
        "frontier event identity must agree"
    );
    let mut control = original;
    control.evidence[0].event_id = "event\nforged-heading".into();
    control.candidates[0].evidence_ids = vec![control.evidence[0].event_id.clone()];
    assert!(
        control.validate().is_err(),
        "control characters are not source identifiers"
    );
}

#[test]
fn adversarial_formation_rejects_complete_without_evidence() {
    let mut empty: omegon_memory::EpisodeFormation =
        serde_json::from_value(episode_json()["formation"].clone()).unwrap();
    empty.evidence.clear();
    empty.candidates.clear();
    assert!(
        empty.validate().is_err(),
        "completed extraction needs an available nonempty evidence set"
    );
}

#[tokio::test]
async fn adversarial_legacy_invalid_model_diagnostic_preserves_valid_source() {
    let mut legacy = episode_json();
    legacy["formation"]["candidates"] = json!([]);
    legacy["formation"]["extraction"] =
        json!({"state":"unavailable", "model":"legacy:\tinvalid", "reason":"request_failed"});
    for backend in [
        Box::new(omegon_memory::InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        backend.import_jsonl(&legacy.to_string()).await.unwrap();
        let episodes = backend.list_episodes("wave3", 1).await.unwrap();
        assert_eq!(episodes[0].formation.as_ref().unwrap().evidence.len(), 1);
    }
}

#[tokio::test]
async fn formation_evidence_and_pending_candidates_survive_transport() {
    for backend in [
        Box::new(omegon_memory::InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let fixture = episode_json();
        let stats = backend.import_jsonl(&fixture.to_string()).await.unwrap();
        assert_eq!(stats.errors, 0);
        let episodes = backend.list_episodes("wave3", 10).await.unwrap();
        assert_eq!(
            serde_json::to_value(&episodes[0]).unwrap()["formation"],
            fixture["formation"]
        );
        assert!(
            backend
                .list_facts("wave3", Default::default())
                .await
                .unwrap()
                .is_empty()
        );
        let copy = omegon_memory::InMemoryBackend::new();
        copy.import_jsonl(&backend.export_jsonl("wave3").await.unwrap())
            .await
            .unwrap();
        let copies = copy.list_episodes("wave3", 10).await.unwrap();
        assert_eq!(
            serde_json::to_value(&copies[0]).unwrap()["formation"],
            fixture["formation"]
        );
    }
}

#[tokio::test]
async fn formation_evidence_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    backend
        .import_jsonl(&episode_json().to_string())
        .await
        .unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    let episodes = backend.list_episodes("wave3", 1).await.unwrap();
    assert_eq!(
        serde_json::to_value(&episodes[0]).unwrap()["formation"],
        episode_json()["formation"]
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE episodes SET formation = 'not-json' WHERE id = 'evidence-episode'",
        [],
    )
    .unwrap();
    assert!(
        backend.list_episodes("wave3", 1).await.is_err(),
        "corrupt evidence must not silently disappear"
    );
}

#[test]
fn candidates_reject_fabricated_refs_authority_and_malformed_siblings() {
    let formation: omegon_memory::EpisodeFormation =
        serde_json::from_value(episode_json()["formation"].clone()).unwrap();
    let good = formation.candidates[0].clone();
    let mut invented = serde_json::to_value(&good).unwrap();
    invented["evidence_ids"] = json!(["nonexistent"]);
    let mut elevated = serde_json::to_value(&good).unwrap();
    elevated["authority"] = json!("verified");
    let output = json!([good, invented, elevated, {"section":"unknown"}]).to_string();
    let (accepted, rejected) =
        omegon_memory::formation::parse_candidates(&output, &formation.evidence).unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].section, omegon_memory::Section::Constraints);
    assert_eq!(rejected, 3);
    assert!(
        omegon_memory::formation::parse_candidates(
            "plain text is not structured evidence",
            &formation.evidence
        )
        .is_err()
    );
    assert!(
        omegon_memory::formation::parse_candidates(&"x".repeat(65_537), &formation.evidence)
            .is_err()
    );
    let mut invalid = formation.clone();
    invalid.evidence[0].sequence = 9;
    assert!(
        invalid.validate().is_err(),
        "future evidence cannot cross the source frontier"
    );
    invalid = formation;
    invalid.evidence[0].outcome = Some(omegon_memory::EvidenceOutcome::Succeeded);
    assert!(
        invalid.validate().is_err(),
        "a user statement cannot claim a tool outcome"
    );
}

#[tokio::test]
async fn schema8_migration_preserves_legacy_unknowns_and_rolls_back_on_failure() {
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let backend = SqliteBackend::open(&path).unwrap();
        let mut legacy = episode_json();
        legacy.as_object_mut().unwrap().remove("formation");
        backend.import_jsonl(&legacy.to_string()).await.unwrap();
        drop(backend);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("ALTER TABLE episodes DROP COLUMN formation; DELETE FROM schema_version; INSERT INTO schema_version VALUES (8, 'fixture');").unwrap();
        if fail {
            db.execute_batch("CREATE TRIGGER reject_upgrade BEFORE INSERT ON schema_version BEGIN SELECT RAISE(ABORT,'fixture migration failure'); END;").unwrap();
        }
        drop(db);
        let plan = SqliteBackend::plan_migration(&path).unwrap();
        let migrated = SqliteBackend::apply_migration(&plan);
        if fail {
            assert!(migrated.is_err());
            let db = rusqlite::Connection::open(&path).unwrap();
            let version: i64 = db
                .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(version, 8);
            assert!(db.prepare("SELECT formation FROM episodes").is_err());
        } else {
            assert!(migrated.unwrap().backup.exists());
            let backend = SqliteBackend::open(&path).unwrap();
            let episodes = backend.list_episodes("wave3", 1).await.unwrap();
            assert!(episodes[0].formation.is_none());
            assert_eq!(episodes[0].narrative, legacy["narrative"].as_str().unwrap());
        }
    }
}

#[tokio::test]
async fn formation_completion_is_atomic_replayable_searchable_and_transportable() {
    for backend in [
        Box::new(omegon_memory::InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let mut pending = episode_json();
        pending["formation"]["candidates"] = json!([]);
        pending["formation"]["extraction"] = json!({"state":"pending","model":"fixture-model"});
        backend.import_jsonl(&pending.to_string()).await.unwrap();
        let mirror = omegon_memory::InMemoryBackend::new();
        mirror.import_jsonl(&pending.to_string()).await.unwrap();
        let mut completion: omegon_memory::EpisodeFormation =
            serde_json::from_value(episode_json()["formation"].clone()).unwrap();
        completion.candidates[0].content = "xylophonemigration must be atomic".into();
        let mut forged = completion.clone();
        forged.evidence[0].excerpt = "replacement evidence".into();
        assert!(
            backend
                .apply_mutation(
                    "forged",
                    omegon_memory::MemoryMutation::CompleteFormation {
                        episode_id: "evidence-episode".into(),
                        formation: Box::new(forged),
                    }
                )
                .await
                .is_err()
        );
        assert!(matches!(
            backend.list_episodes("wave3", 1).await.unwrap()[0]
                .formation
                .as_ref()
                .unwrap()
                .extraction,
            omegon_memory::ExtractionOutcome::Pending { .. }
        ));
        let mutation = omegon_memory::MemoryMutation::CompleteFormation {
            episode_id: "evidence-episode".into(),
            formation: Box::new(completion.clone()),
        };
        assert!(
            !backend
                .apply_mutation("complete", mutation.clone())
                .await
                .unwrap()
                .replayed
        );
        assert!(
            backend
                .apply_mutation("complete", mutation)
                .await
                .unwrap()
                .replayed
        );
        assert_eq!(
            backend
                .search_episodes("wave3", "xylophonemigration", 1)
                .await
                .unwrap()
                .len(),
            1
        );
        mirror
            .import_jsonl(&backend.export_jsonl("wave3").await.unwrap())
            .await
            .unwrap();
        assert_eq!(
            mirror.list_episodes("wave3", 1).await.unwrap()[0]
                .formation
                .as_deref()
                .unwrap(),
            &completion
        );
        assert!(
            backend
                .list_facts("wave3", Default::default())
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn failed_completion_rolls_back_episode_index_vector_and_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atomic.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let mut pending = episode_json();
    pending["formation"]["candidates"] = json!([]);
    pending["formation"]["extraction"] = json!({"state":"pending","model":"fixture-model"});
    backend.import_jsonl(&pending.to_string()).await.unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("INSERT INTO episodes_vec (episode_id,embedding,model_name,dims,created_at) VALUES ('evidence-episode',?1,'fixture',1,'fixture')", [vec![0u8,0,128,63]]).unwrap();
    db.execute_batch("CREATE TRIGGER reject_completion_receipt BEFORE INSERT ON memory_operation_receipts WHEN NEW.operation_id='failed-complete' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    let completion: omegon_memory::EpisodeFormation =
        serde_json::from_value(episode_json()["formation"].clone()).unwrap();
    let mutation = omegon_memory::MemoryMutation::CompleteFormation {
        episode_id: "evidence-episode".into(),
        formation: Box::new(completion),
    };
    assert!(
        backend
            .apply_mutation("failed-complete", mutation.clone())
            .await
            .is_err()
    );
    let episode = backend.list_episodes("wave3", 1).await.unwrap().remove(0);
    assert!(matches!(
        episode.formation.unwrap().extraction,
        omegon_memory::ExtractionOutcome::Pending { .. }
    ));
    let vectors: i64 = db
        .query_row("SELECT COUNT(*) FROM episodes_vec", [], |row| row.get(0))
        .unwrap();
    assert_eq!(vectors, 1);
    let receipts: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM memory_operation_receipts WHERE operation_id='failed-complete'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipts, 0);
    backend.apply_mutation("complete", mutation).await.unwrap();
    let vectors: i64 = db
        .query_row("SELECT COUNT(*) FROM episodes_vec", [], |row| row.get(0))
        .unwrap();
    assert_eq!(vectors, 0);
    assert_eq!(
        backend
            .search_episodes("wave3", "atomic", 1)
            .await
            .unwrap()
            .len(),
        1
    );
}
