use omegon_memory::*;

#[tokio::test]
async fn platform_inapplicable_facts_cannot_consume_candidate_limit() {
    let platform = std::env::consts::OS;
    let other = if platform == "linux" {
        "macos"
    } else {
        "linux"
    };
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let rows=(0..41).map(|index|serde_json::json!({"_type":"fact","id":if index<40 {format!("bad-{index:02}")} else {"eligible".into()},
            "mind":"test","content":"zircon","section":"Constraints","status":"active","created_at":util::now_iso(),"version":1,
            "applicability":{"constraints":{"platforms":[if index<40 {other} else {platform}]},"recorded_at":util::now_iso()}
        }).to_string()).collect::<Vec<_>>().join("\n");
        backend.import_jsonl(&rows).await.unwrap();
        let found = backend.fts_search("test", "zircon", 1).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].fact.id, "eligible",
            "scope filtering must precede candidate limits"
        );
        let space = EmbeddingSpace {
            model: "fixture".into(),
            revision: "v1".into(),
            preprocessing: "raw".into(),
            dimensions: 2,
        };
        for fact in backend
            .list_facts("test", Default::default())
            .await
            .unwrap()
        {
            backend
                .apply_mutation(
                    &format!("embed-{}", fact.id),
                    MemoryMutation::StoreIdentifiedEmbedding {
                        fact: FactPrecondition {
                            id: fact.id,
                            expected_version: fact.version,
                        },
                        embedding: IdentifiedEmbedding {
                            space: space.clone(),
                            values: vec![1.0, 0.0],
                        },
                    },
                )
                .await
                .unwrap();
        }
        let vector = backend
            .search_identified(
                "test",
                &IdentifiedEmbedding {
                    space,
                    values: vec![1.0, 0.0],
                },
                1,
                0.0,
                &Default::default(),
                &|| false,
            )
            .await
            .unwrap();
        assert_eq!(vector.results[0].fact.id, "eligible");
    }
}

#[tokio::test]
async fn graph_neighbor_scope_is_checked_before_edge_limits() {
    let platform = std::env::consts::OS;
    let other = if platform == "linux" {
        "macos"
    } else {
        "linux"
    };
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let seed = backend
            .store_fact(StoreFact {
                mind: "test".into(),
                content: "seed".into(),
                section: Section::Constraints,
                source: None,
                decay_profile: Default::default(),
            })
            .await
            .unwrap()
            .fact;
        for (id, scope) in [("bad", other), ("good", platform)] {
            let row = serde_json::json!({"_type":"fact","id":id,"mind":"test","content":id,"section":"Constraints","status":"active","created_at":util::now_iso(),"version":1,"applicability":{"constraints":{"platforms":[scope]},"recorded_at":util::now_iso()}});
            backend.import_jsonl(&row.to_string()).await.unwrap();
            backend.import_jsonl(&serde_json::json!({"_type":"edge","id":format!("edge-{id}"),"source_id":seed.id,"target_id":id,"relation":"supports","confidence":1.0,"created_at":util::now_iso()}).to_string()).await.unwrap();
        }
        let edges = backend
            .get_edges_filtered("test", &seed.id, &Default::default(), 1)
            .await
            .unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target_id, "good");
        let expanded = service::expand_edges_filtered_checked(
            backend.as_ref(),
            "test",
            vec![ScoredFact::new(seed, 1.0, 1.0)],
            10,
            &Default::default(),
            &|| false,
        )
        .await
        .unwrap();
        assert!(expanded.iter().any(|result| result.fact.id == "good"));
        assert!(!expanded.iter().any(|result| result.fact.id == "bad"));
    }
}

fn scoped(content: &str, constraints: ApplicabilityConstraints) -> MemoryMutation {
    MemoryMutation::StoreApplicableFact {
        request: StoreFact {
            mind: "test".into(),
            content: content.into(),
            section: Section::Constraints,
            source: None,
            decay_profile: Default::default(),
        },
        constraints: Box::new(constraints),
    }
}
fn at(time: &str) -> SearchFilter {
    SearchFilter {
        context: Some(ApplicabilityContext {
            at: Some(time.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn validity_bounds_are_timezone_aware_and_history_retains_recorded_time() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let result = backend
            .apply_mutation(
                "store",
                scoped(
                    "zircon",
                    ApplicabilityConstraints {
                        valid_from: Some("2026-01-01T01:00:00+01:00".into()),
                        valid_until: Some("2026-01-02T00:00:00Z".into()),
                        ..Default::default()
                    },
                ),
            )
            .await
            .unwrap();
        let MemoryMutationEffect::FactStored { fact_id, .. } = result.effect else {
            panic!("store");
        };
        let recorded_at = backend
            .get_fact_record("test", &fact_id)
            .await
            .unwrap()
            .unwrap()
            .applicability
            .unwrap()
            .recorded_at;
        for (time, expected) in [
            ("2025-12-31T23:59:59.999999999Z", 0),
            ("2026-01-01T00:00:00Z", 1),
            ("2026-01-01T23:59:59.999999999Z", 1),
            ("2026-01-02T00:00:00Z", 0),
        ] {
            assert_eq!(
                backend
                    .fts_search_filtered("test", "zircon", 1, &at(time))
                    .await
                    .unwrap()
                    .len(),
                expected,
                "{time}"
            );
        }
        backend.archive_facts(&[&fact_id]).await.unwrap();
        let mut historical = at("2026-01-01T12:00:00Z");
        historical.intent = SearchIntent::Historical;
        let before = backend.export_jsonl("test").await.unwrap();
        let found = backend
            .fts_search_filtered("test", "zircon", 1, &historical)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].fact.status, FactStatus::Archived);
        assert_eq!(
            found[0].fact.applicability.as_ref().unwrap().recorded_at,
            recorded_at
        );
        assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
        assert_eq!(
            backend
                .fts_search_filtered(
                    "test",
                    "zircon",
                    1,
                    &SearchFilter {
                        intent: SearchIntent::Historical,
                        ..Default::default()
                    }
                )
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn scope_matching_distinguishes_missing_context_from_known_mismatch() {
    let rules = ApplicabilityConstraints {
        workspaces: vec!["checkout-a".into()],
        revisions: vec![format!("git:{}", "a".repeat(40))],
        components: vec!["runtime".into()],
        ..Default::default()
    };
    rules.validate().unwrap();
    assert_eq!(
        rules.assess(&Default::default()),
        ApplicabilityStatus::Unknown
    );
    let mut context = ApplicabilityContext {
        workspace: Some("checkout-a".into()),
        revision: Some(format!("git:{}", "a".repeat(40))),
        component: Some("runtime".into()),
        ..Default::default()
    };
    assert_eq!(rules.assess(&context), ApplicabilityStatus::Matches);
    context.revision = Some(format!("git:{}", "b".repeat(40)));
    assert_eq!(rules.assess(&context), ApplicabilityStatus::Inapplicable);
    assert!(
        ApplicabilityConstraints {
            valid_from: Some("2026-01-02T00:00:00Z".into()),
            valid_until: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}

#[tokio::test]
async fn scope_updates_are_versioned_without_reinforcement_and_survive_transport() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let stored = backend
            .apply_mutation(
                "store",
                scoped(
                    "zircon",
                    ApplicabilityConstraints {
                        platforms: vec!["linux".into()],
                        ..Default::default()
                    },
                ),
            )
            .await
            .unwrap();
        let MemoryMutationEffect::FactStored {
            fact_id, version, ..
        } = stored.effect
        else {
            panic!("store");
        };
        let before = backend
            .get_fact_record("test", &fact_id)
            .await
            .unwrap()
            .unwrap();
        let mutation = MemoryMutation::SetFactApplicability {
            fact: FactPrecondition {
                id: fact_id.clone(),
                expected_version: version,
            },
            constraints: Box::new(ApplicabilityConstraints {
                platforms: vec!["macos".into()],
                ..Default::default()
            }),
        };
        backend
            .apply_mutation("scope", mutation.clone())
            .await
            .unwrap();
        assert!(
            backend
                .apply_mutation("scope", mutation.clone())
                .await
                .unwrap()
                .replayed
        );
        assert!(matches!(
            backend.apply_mutation("stale", mutation).await,
            Err(MemoryError::FactVersionConflict { .. })
        ));
        let after = backend
            .get_fact_record("test", &fact_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.last_reinforced, after.last_reinforced);
        assert_eq!(before.reinforcement_count, after.reinforcement_count);
        assert_eq!(before.confidence, after.confidence);
        let exported = backend.export_jsonl("test").await.unwrap();
        let target = SqliteBackend::in_memory().unwrap();
        target.import_jsonl(&exported).await.unwrap();
        assert_eq!(
            target
                .get_fact_record("test", &fact_id)
                .await
                .unwrap()
                .unwrap()
                .applicability,
            after.applicability
        );
        let mut legacy: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(legacy["_type"], "applicable_fact");
        legacy["_type"] = "fact".into();
        legacy.as_object_mut().unwrap().remove("applicability");
        legacy["version"] = (after.version + 1).into();
        backend.import_jsonl(&legacy.to_string()).await.unwrap();
        assert_eq!(
            backend
                .get_fact_record("test", &fact_id)
                .await
                .unwrap()
                .unwrap()
                .applicability,
            after.applicability
        );
    }
}

#[tokio::test]
async fn distinct_scopes_do_not_collapse_and_scope_replay_reuses_the_right_fact() {
    for backend in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        let mut ids = Vec::new();
        for (operation, platform) in [
            ("linux", "linux"),
            ("macos", "macos"),
            ("linux-again", "linux"),
        ] {
            let result = backend
                .apply_mutation(
                    operation,
                    scoped(
                        "zircon",
                        ApplicabilityConstraints {
                            platforms: vec![platform.into()],
                            ..Default::default()
                        },
                    ),
                )
                .await
                .unwrap();
            let MemoryMutationEffect::FactStored { fact_id, .. } = result.effect else {
                panic!("store");
            };
            ids.push(fact_id);
        }
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids[0], ids[2]);
    }
}

#[tokio::test]
async fn schema12_migration_and_failed_scope_write_preserve_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let fact = backend
        .store_fact(StoreFact {
            mind: "test".into(),
            content: "zircon".into(),
            section: Section::Constraints,
            source: None,
            decay_profile: Default::default(),
        })
        .await
        .unwrap()
        .fact;
    drop(backend);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE facts DROP COLUMN applicability; DELETE FROM schema_version; INSERT INTO schema_version VALUES(12,'fixture');").unwrap();
    drop(db);
    let migrated =
        SqliteBackend::apply_migration(&SqliteBackend::plan_migration(&path).unwrap()).unwrap();
    assert!(migrated.backup.exists());
    let backend = SqliteBackend::open(&path).unwrap();
    assert!(
        backend
            .get_fact_record("test", &fact.id)
            .await
            .unwrap()
            .unwrap()
            .applicability
            .is_none()
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_scope BEFORE INSERT ON memory_operation_receipts BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    let update = MemoryMutation::SetFactApplicability {
        fact: FactPrecondition {
            id: fact.id.clone(),
            expected_version: fact.version,
        },
        constraints: Box::new(ApplicabilityConstraints {
            platforms: vec!["linux".into()],
            ..Default::default()
        }),
    };
    assert!(
        backend
            .apply_mutation("scope", update.clone())
            .await
            .is_err()
    );
    assert!(
        backend
            .get_fact_record("test", &fact.id)
            .await
            .unwrap()
            .unwrap()
            .applicability
            .is_none()
    );
    db.execute_batch("DROP TRIGGER reject_scope;").unwrap();
    backend.apply_mutation("scope", update).await.unwrap();
    drop(backend);
    let reopened = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        reopened
            .get_fact_record("test", &fact.id)
            .await
            .unwrap()
            .unwrap()
            .applicability
            .unwrap()
            .constraints
            .platforms,
        vec!["linux"]
    );
}

#[tokio::test]
async fn vault_projection_preserves_declared_scope_without_reinforcement() {
    let backend = InMemoryBackend::new();
    backend
        .apply_mutation(
            "scope",
            scoped(
                "zircon",
                ApplicabilityConstraints {
                    platforms: vec!["linux".into()],
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap();
    let before = backend.export_jsonl("test").await.unwrap();
    let vault = tempfile::tempdir().unwrap();
    vault_sync::materialize_to_vault(&backend, vault.path(), "test")
        .await
        .unwrap();
    let text = std::fs::read_to_string(vault.path().join("ai/memory/constraints.md")).unwrap();
    assert!(text.contains("Declared applicability:"));
    assert!(text.contains("\"platforms\":[\"linux\"]"));
    assert_eq!(
        vault_sync::materialize_to_vault(&backend, vault.path(), "test")
            .await
            .unwrap()
            .files_changed_total,
        0
    );
    assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
}

#[cfg(feature = "agent")]
#[tokio::test]
async fn standalone_scoped_store_and_update_retire_live_context() {
    use omegon_traits::{ContextProvider, ContextSignals, LifecyclePhase, ToolProvider};
    let provider = MemoryProvider::new(InMemoryBackend::new(), MarkdownRenderer, "test".into());
    let stored=provider.execute("memory_store","store",serde_json::json!({"section":"Constraints","content":"zircon provider rule","applicability":{"platforms":[std::env::consts::OS]}}),tokio_util::sync::CancellationToken::new()).await.unwrap();
    let id = stored.details["id"].as_str().unwrap();
    let fact = provider
        .backend()
        .get_fact_record("test", id)
        .await
        .unwrap()
        .unwrap();
    let signals = ContextSignals {
        user_prompt: "zircon",
        recent_tools: &[],
        recent_files: &[],
        lifecycle_phase: &LifecyclePhase::Idle,
        turn_number: 1,
        context_budget_tokens: 500,
    };
    assert!(
        provider
            .provide_context(&signals)
            .unwrap()
            .content
            .contains("zircon provider rule")
    );
    provider.execute("memory_set_applicability","expire",serde_json::json!({"fact_id":id,"expected_version":fact.version,"applicability":{"valid_until":"2000-01-01T00:00:00Z"}}),tokio_util::sync::CancellationToken::new()).await.unwrap();
    let cleared = provider
        .provide_context(&signals)
        .expect("empty replacement retires a known-inapplicable injection");
    assert!(cleared.content.is_empty());
}
