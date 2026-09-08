use omegon_memory::*;

fn record(id: &str, status: &str) -> serde_json::Value {
    serde_json::json!({
        "_type":"fact", "id":id, "mind":"history", "content":format!("zircon {id}"),
        "section":"Decisions", "status":status, "created_at":"2025-01-01T00:00:00Z",
        "version":7, "operational":{
            "confidence":0.23, "reinforcement_count":4, "decay_rate":0.02,
            "last_reinforced":"2025-02-01T00:00:00Z", "last_accessed":"2025-02-02T00:00:00Z",
            "created_session":"session-original", "superseded_at":null,
            "archived_at":null, "jj_change_id":"original-change"
        }
    })
}

fn backends() -> Vec<Box<dyn MemoryBackend>> {
    vec![
        Box::new(InMemoryBackend::new()),
        Box::new(SqliteBackend::in_memory().unwrap()),
    ]
}

#[tokio::test]
async fn transport_preserves_history_and_operational_state_across_backends() {
    for source in backends() {
        let rows =
            ["active", "archived", "dormant", "superseded"].map(|status| record(status, status));
        let input = rows
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        source.import_jsonl(&input).await.unwrap();
        let export = source.export_jsonl("history").await.unwrap();
        assert_eq!(
            export.lines().count(),
            4,
            "historical records must survive transport"
        );
        for line in export.lines() {
            let actual: serde_json::Value = serde_json::from_str(line).unwrap();
            let expected = rows.iter().find(|row| row["id"] == actual["id"]).unwrap();
            assert_eq!(actual["operational"], expected["operational"]);
        }
        for target in backends() {
            target.import_jsonl(&export).await.unwrap();
            for status in ["active", "archived", "dormant", "superseded"] {
                let filter = FactFilter {
                    status: Some(serde_json::from_value(status.into()).unwrap()),
                    section: None,
                };
                let facts = target.list_facts("history", filter).await.unwrap();
                assert_eq!(facts.len(), 1);
                let fact = &facts[0];
                assert_eq!(fact.confidence, 0.23);
                assert_eq!(fact.reinforcement_count, 4);
                assert_eq!(fact.last_reinforced, "2025-02-01T00:00:00Z");
                assert_eq!(fact.created_session.as_deref(), Some("session-original"));
                assert_eq!(
                    fact.source, None,
                    "transport must not invent a manual source"
                );
            }
            assert_eq!(target.export_jsonl("history").await.unwrap(), export);
            target.import_jsonl(&export).await.unwrap();
            assert_eq!(target.export_jsonl("history").await.unwrap(), export);
        }
    }
}

#[tokio::test]
async fn legacy_updates_preserve_known_operational_state_and_modern_updates_replace_it() {
    for backend in backends() {
        let original = record("fact", "active");
        backend.import_jsonl(&original.to_string()).await.unwrap();
        let mut legacy = original.clone();
        legacy.as_object_mut().unwrap().remove("operational");
        legacy["source"] = "".into();
        legacy["version"] = 8.into();
        backend.import_jsonl(&legacy.to_string()).await.unwrap();
        let fact = backend.get_fact("fact").await.unwrap().unwrap();
        assert_eq!(fact.confidence, 0.23);
        assert_eq!(fact.source, None);
        assert_eq!(fact.last_reinforced, "2025-02-01T00:00:00Z");
        let mut modern = original;
        modern["version"] = 9.into();
        modern["operational"]["confidence"] = 0.12.into();
        modern["operational"]["last_accessed"] = serde_json::Value::Null;
        backend.import_jsonl(&modern.to_string()).await.unwrap();
        backend.import_jsonl(&legacy.to_string()).await.unwrap();
        let fact = backend.get_fact("fact").await.unwrap().unwrap();
        assert_eq!(fact.confidence, 0.12);
        assert_eq!(fact.last_accessed, None);
        assert_eq!(fact.version, 9);
    }
}

#[tokio::test]
async fn invalid_operational_state_rolls_back_batch_and_receipt() {
    for backend in backends() {
        for (field, invalid) in [
            ("confidence", serde_json::json!(1.2)),
            ("last_reinforced", serde_json::json!("yesterday")),
            ("decay_rate", serde_json::json!(-1.0)),
        ] {
            let mut bad = record("bad", "active");
            bad["operational"][field] = invalid;
            let jsonl = format!("{}\n{}", record("good", "active"), bad);
            assert!(
                backend
                    .apply_mutation("bad-import", MemoryMutation::ImportJsonl { jsonl })
                    .await
                    .is_err()
            );
            assert!(backend.export_jsonl("history").await.unwrap().is_empty());
        }
        // Failed imports must not leave an operation receipt occupying the identity.
        backend
            .apply_mutation(
                "bad-import",
                MemoryMutation::ImportJsonl {
                    jsonl: record("good", "active").to_string(),
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn history_edges_and_operational_state_survive_reopen_and_failed_export_is_not_partial() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let mut replacement = record("new", "active");
    replacement["supersedes"] = "old".into();
    let mut old = record("old", "superseded");
    old["operational"]["superseded_at"] = "2025-03-01T00:00:00Z".into();
    let edge = serde_json::json!({"_type":"edge", "id":"edge", "source_id":"new", "target_id":"old", "relation":"supersedes", "confidence":1.0, "created_at":"2025-03-01T00:00:00Z"});
    backend
        .import_jsonl(&format!("{old}\n{replacement}\n{edge}"))
        .await
        .unwrap();
    let before = backend.export_jsonl("history").await.unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert_eq!(backend.export_jsonl("history").await.unwrap(), before);
    for target in backends() {
        target.import_jsonl(&before).await.unwrap();
        assert_eq!(target.export_jsonl("history").await.unwrap(), before);
    }
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute("UPDATE facts SET reinforcement_count=-1 WHERE id='old'", [])
        .unwrap();
    assert!(
        backend.export_jsonl("history").await.is_err(),
        "corrupt history cannot disappear from export"
    );
}

#[tokio::test]
async fn corrupt_history_status_is_not_reinterpreted_as_active_knowledge() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    backend
        .import_jsonl(&record("old", "archived").to_string())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute(
        "UPDATE facts SET status='unknown-status' WHERE id='old'",
        [],
    )
    .unwrap();
    assert!(backend.export_jsonl("history").await.is_err());
}
