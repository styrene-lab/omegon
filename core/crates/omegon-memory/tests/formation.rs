use omegon_memory::{MemoryBackend, SqliteBackend};
use serde_json::json;

fn episode_json() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/formation.json")).unwrap()
}

fn covered_episode() -> serde_json::Value {
    let mut row = episode_json();
    for field in [
        "affected_nodes",
        "affected_changes",
        "files_changed",
        "tags",
    ] {
        row[field] = json!([]);
    }
    row["formation"]["version"] = json!(2);
    row["formation"]["coverage"] = json!({"first_sequence":1,"policy_version":1});
    row["formation"]["candidates"] = json!([]);
    row["formation"]["extraction"] = json!({"state":"pending","model":"fixture-model"});
    row
}

#[test]
fn coverage_contract_validates_ranges_and_preserves_legacy_unknowns() {
    let row = covered_episode();
    let formation: omegon_memory::EpisodeFormation =
        serde_json::from_value(row["formation"].clone()).unwrap();
    formation.validate().expect("valid scanned range");
    assert_eq!(serde_json::to_value(formation).unwrap(), row["formation"]);
    let legacy = episode_json()["formation"].clone();
    let formation: omegon_memory::EpisodeFormation =
        serde_json::from_value(legacy.clone()).unwrap();
    formation.validate().unwrap();
    assert_eq!(serde_json::to_value(formation).unwrap(), legacy);

    let mut boundary = row["formation"].clone();
    boundary["coverage"]["first_sequence"] = json!(8);
    boundary["evidence"][0]["sequence"] = json!(8);
    boundary["evidence"][0]["event_id"] = json!("event-8");
    serde_json::from_value::<omegon_memory::EpisodeFormation>(boundary.clone())
        .unwrap()
        .validate()
        .expect("inclusive single-record range");
    boundary["evidence"] = json!([]);
    serde_json::from_value::<omegon_memory::EpisodeFormation>(boundary.clone())
        .unwrap()
        .validate()
        .expect("scanned non-evidence range");
    boundary["source"] = json!({"state":"unavailable","session_id":"fixture","reason":"missing"});
    assert!(
        serde_json::from_value::<omegon_memory::EpisodeFormation>(boundary)
            .unwrap()
            .validate()
            .is_err(),
        "unavailable source cannot declare coverage even without evidence"
    );

    for (field, value) in [
        ("coverage", serde_json::Value::Null),
        ("coverage", json!({"first_sequence":0,"policy_version":1})),
        ("coverage", json!({"first_sequence":9,"policy_version":1})),
        ("coverage", json!({"first_sequence":1,"policy_version":2})),
        ("coverage", json!({"first_sequence":8,"policy_version":1})),
        ("version", json!(1)),
        ("version", json!(3)),
        (
            "source",
            json!({"state":"unavailable","session_id":"fixture","reason":"missing"}),
        ),
    ] {
        let mut invalid = row["formation"].clone();
        invalid[field] = value;
        let parsed = serde_json::from_value::<omegon_memory::EpisodeFormation>(invalid.clone());
        assert!(
            parsed.is_err() || parsed.unwrap().validate().is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn coverage_contract_does_not_silently_discard_legacy_declaration() {
    let mut row = episode_json()["formation"].clone();
    row["coverage"] = json!({"first_sequence":1,"policy_version":1});
    let formation: omegon_memory::EpisodeFormation = serde_json::from_value(row).unwrap();
    assert!(
        formation.validate().is_err(),
        "legacy snapshots must reject coverage rather than silently drop it"
    );
}

async fn coverage_completion_case(backend: &dyn MemoryBackend) {
    let pending = covered_episode();
    backend.import_jsonl(&pending.to_string()).await.unwrap();
    for downgrade in [false, true] {
        let mut changed = pending.clone();
        changed["formation"]["extraction"] = json!({"state":"complete","model":"fixture-model"});
        if downgrade {
            changed["formation"]["version"] = json!(1);
            changed["formation"]
                .as_object_mut()
                .unwrap()
                .remove("coverage");
        } else {
            changed["formation"]["coverage"]["first_sequence"] = json!(2);
        }
        let completion = serde_json::from_value(changed["formation"].clone()).unwrap();
        assert!(
            backend
                .apply_mutation(
                    "changed-coverage",
                    omegon_memory::MemoryMutation::CompleteFormation {
                        episode_id: "evidence-episode".into(),
                        formation: Box::new(completion),
                    }
                )
                .await
                .is_err()
        );
        // Import may return rejected-row statistics rather than an operation error.
        let _ = backend.import_jsonl(&changed.to_string()).await;
        assert_eq!(
            serde_json::to_value(
                backend.list_episodes("wave3", 1).await.unwrap()[0]
                    .formation
                    .as_ref()
                    .unwrap()
            )
            .unwrap(),
            pending["formation"]
        );
    }
    let mut complete = pending;
    complete["formation"]["extraction"] = json!({"state":"complete","model":"fixture-model"});
    backend.import_jsonl(&complete.to_string()).await.unwrap();
    assert_eq!(
        serde_json::to_value(
            backend.list_episodes("wave3", 1).await.unwrap()[0]
                .formation
                .as_ref()
                .unwrap()
        )
        .unwrap(),
        complete["formation"]
    );
}

#[tokio::test]
async fn coverage_contract_completion_is_immutable_across_backends() {
    coverage_completion_case(&omegon_memory::InMemoryBackend::new()).await;
    coverage_completion_case(&SqliteBackend::in_memory().unwrap()).await;
}

#[tokio::test]
async fn coverage_contract_survives_replay_reopen_and_transport() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("coverage.db");
    let row = covered_episode();
    let request: omegon_memory::StoreEpisode = serde_json::from_value(row.clone()).unwrap();
    let mutation = omegon_memory::MemoryMutation::StoreEpisode { request };
    for backend in [
        Box::new(omegon_memory::InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::open(&path).unwrap()),
    ] {
        assert!(
            !backend
                .apply_mutation("coverage-page", mutation.clone())
                .await
                .unwrap()
                .replayed
        );
        assert!(
            backend
                .apply_mutation("coverage-page", mutation.clone())
                .await
                .unwrap()
                .replayed
        );
        let episodes = backend.list_episodes("wave3", 10).await.unwrap();
        assert_eq!(episodes.len(), 1);
        assert_eq!(
            serde_json::to_value(episodes[0].formation.as_ref().unwrap()).unwrap(),
            row["formation"]
        );
        let mirror = omegon_memory::InMemoryBackend::new();
        mirror
            .import_jsonl(&backend.export_jsonl("wave3").await.unwrap())
            .await
            .unwrap();
        assert_eq!(
            mirror.list_episodes("wave3", 10).await.unwrap()[0].formation,
            episodes[0].formation
        );
    }
    let reopened = SqliteBackend::open(&path).unwrap();
    assert!(
        reopened
            .apply_mutation("coverage-page", mutation)
            .await
            .unwrap()
            .replayed
    );
    assert_eq!(
        serde_json::to_value(
            reopened.list_episodes("wave3", 10).await.unwrap()[0]
                .formation
                .as_ref()
                .unwrap()
        )
        .unwrap(),
        row["formation"]
    );
}

async fn recovery_inventory_case(backend: &dyn MemoryBackend) {
    for index in 0..20 {
        let mut row = episode_json();
        row["id"] = json!(format!("episode-{index:02}"));
        row["formation"]["candidates"] = json!([]);
        row["formation"]["extraction"] = if index < 10 {
            json!({"state":"complete","model":"fixture"})
        } else {
            json!({"state":"pending","model":"fixture"})
        };
        backend.import_jsonl(&row.to_string()).await.unwrap();
    }
    let mut other = episode_json();
    other["id"] = json!("other-mind");
    other["mind"] = json!("other");
    other["formation"]["candidates"] = json!([]);
    other["formation"]["extraction"] = json!({"state":"pending","model":"fixture"});
    backend.import_jsonl(&other.to_string()).await.unwrap();
    assert!(
        backend
            .pending_formations("wave3", "different", 8)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        backend
            .pending_formations("wave3", "fixture", 9)
            .await
            .is_err()
    );
    let batch = backend
        .pending_formations("wave3", "fixture", 8)
        .await
        .unwrap();
    assert_eq!(batch.len(), 8);
    assert_eq!(batch[0].id, "episode-10");
    for episode in batch {
        let mut formation = *episode.formation.unwrap();
        formation.extraction = omegon_memory::ExtractionOutcome::Complete {
            model: "fixture".into(),
        };
        backend
            .apply_mutation(
                &format!("recover:{}", episode.id),
                omegon_memory::MemoryMutation::CompleteFormation {
                    episode_id: episode.id,
                    formation: Box::new(formation),
                },
            )
            .await
            .unwrap();
    }
    assert_eq!(
        backend
            .pending_formations("wave3", "fixture", 8)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        backend
            .pending_formations("other", "fixture", 8)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn recovery_inventory_filters_before_limit_and_preserves_remainder() {
    recovery_inventory_case(&omegon_memory::InMemoryBackend::new()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    {
        let backend = SqliteBackend::open(&path).unwrap();
        recovery_inventory_case(&backend).await;
    }
    let reopened = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        reopened
            .pending_formations("wave3", "fixture", 8)
            .await
            .unwrap()
            .len(),
        2
    );
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "UPDATE episodes SET formation='invalid json' WHERE id='episode-18'",
        [],
    )
    .unwrap();
    assert!(
        reopened
            .pending_formations("wave3", "fixture", 8)
            .await
            .is_err()
    );
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
    let mut pending = covered_episode();
    pending["formation"]["candidates"] = json!([]);
    pending["formation"]["extraction"] = json!({"state":"pending","model":"fixture-model"});
    backend.import_jsonl(&pending.to_string()).await.unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("INSERT INTO episodes_vec (episode_id,embedding,model_name,dims,created_at) VALUES ('evidence-episode',?1,'fixture',1,'fixture')", [vec![0u8,0,128,63]]).unwrap();
    db.execute_batch("CREATE TRIGGER reject_completion_receipt BEFORE INSERT ON memory_operation_receipts WHEN NEW.operation_id='failed-complete' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    let mut completion: omegon_memory::EpisodeFormation =
        serde_json::from_value(pending["formation"].clone()).unwrap();
    completion.extraction = omegon_memory::ExtractionOutcome::Complete {
        model: "fixture-model".into(),
    };
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
