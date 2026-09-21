use omegon_memory::*;

fn page(first: u64, last: u64) -> StoreEpisode {
    StoreEpisode {
        mind: "capture".into(),
        title: "page".into(),
        narrative: "source".into(),
        date: Some("2026-09-21".into()),
        affected_nodes: vec![],
        affected_changes: vec![],
        files_changed: vec![],
        tags: vec![],
        tool_calls_count: None,
        formation: Some(Box::new(EpisodeFormation {
            version: 2,
            source: FormationSource::Available {
                session_id: "session".into(),
                stream_id: "stream".into(),
                sequence: last,
                event_id: format!("event-{last}"),
            },
            coverage: Some(FormationCoverage {
                first_sequence: first,
                policy_version: 1,
            }),
            evidence: vec![],
            candidates: vec![],
            extraction: ExtractionOutcome::Disabled,
            truncated: false,
            rejected_candidates: 0,
        })),
    }
}

async fn cursor_case(backend: &dyn MemoryBackend) {
    let request = page(1, 8);
    let key = formation::capture_key(&request).unwrap();
    let mutation = MemoryMutation::StoreCoveragePage {
        request,
        expected: None,
    };
    backend
        .apply_mutation("page-1", mutation.clone())
        .await
        .unwrap();
    let first = backend
        .formation_cursor(&key)
        .await
        .unwrap()
        .expect("durable cursor after page");
    assert_eq!(first.sequence, 8);
    assert!(
        backend
            .apply_mutation("page-1", mutation)
            .await
            .unwrap()
            .replayed
    );
    for (start, expected) in [(10, Some(first.clone())), (9, None)] {
        assert!(
            backend
                .apply_mutation(
                    "conflict",
                    MemoryMutation::StoreCoveragePage {
                        request: page(start, 16),
                        expected
                    }
                )
                .await
                .is_err()
        );
    }
    assert_eq!(backend.list_episodes("capture", 10).await.unwrap().len(), 1);
    backend
        .apply_mutation(
            "page-2",
            MemoryMutation::StoreCoveragePage {
                request: page(9, 16),
                expected: Some(first),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        backend
            .formation_cursor(&key)
            .await
            .unwrap()
            .unwrap()
            .sequence,
        16
    );
    for imported in [
        Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
        Box::new(SqliteBackend::in_memory().unwrap()),
    ] {
        imported
            .import_jsonl(&backend.export_jsonl("capture").await.unwrap())
            .await
            .unwrap();
        assert!(
            imported.formation_cursor(&key).await.unwrap().is_none(),
            "transport cannot invent a local cursor"
        );
    }
    let mut other = key.clone();
    other.mind = "other".into();
    assert!(backend.formation_cursor(&other).await.unwrap().is_none());
    other = key;
    other.model = Some("other-model".into());
    assert!(backend.formation_cursor(&other).await.unwrap().is_none());
}

#[tokio::test]
async fn capture_cursor_is_local_contiguous_and_replayable() {
    cursor_case(&InMemoryBackend::new()).await;
    cursor_case(&SqliteBackend::in_memory().unwrap()).await;
}

#[tokio::test]
async fn capture_cursor_rollback_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let request = page(1, 8);
    let key = formation::capture_key(&request).unwrap();
    let mutation = MemoryMutation::StoreCoveragePage {
        request,
        expected: None,
    };
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_capture BEFORE INSERT ON memory_operation_receipts BEGIN SELECT RAISE(ABORT,'capture fault'); END;").unwrap();
    assert!(
        backend
            .apply_mutation("page", mutation.clone())
            .await
            .is_err()
    );
    assert!(backend.formation_cursor(&key).await.unwrap().is_none());
    assert!(
        backend
            .list_episodes("capture", 10)
            .await
            .unwrap()
            .is_empty()
    );
    sql.execute_batch("DROP TRIGGER fail_capture").unwrap();
    backend
        .apply_mutation("page", mutation.clone())
        .await
        .unwrap();
    drop(backend);
    let reopened = SqliteBackend::open(&path).unwrap();
    assert_eq!(
        reopened
            .formation_cursor(&key)
            .await
            .unwrap()
            .unwrap()
            .sequence,
        8
    );
    assert!(
        reopened
            .apply_mutation("page", mutation)
            .await
            .unwrap()
            .replayed
    );
}

#[tokio::test]
async fn capture_cursor_uses_semantic_key_fields_after_receipt_reformatting() {
    for model in [None, Some("fixture-model")] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");
        let backend = SqliteBackend::open(&path).unwrap();
        let mut request = page(1, 8);
        if let Some(model) = model {
            request.formation.as_mut().unwrap().extraction = ExtractionOutcome::Pending {
                model: model.into(),
            };
        }
        let key = formation::capture_key(&request).unwrap();
        backend
            .apply_mutation(
                "page",
                MemoryMutation::StoreCoveragePage {
                    request,
                    expected: None,
                },
            )
            .await
            .unwrap();
        let expected = backend.formation_cursor(&key).await.unwrap().unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let receipt: String = db
            .query_row(
                "SELECT effect_json FROM memory_operation_receipts WHERE operation_id='page'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let reordered_key = format!(
            "{{ \"model\": {}, \"policy_version\": {}, \"stream_id\": {}, \"session_id\": {}, \"mind\": {} }}",
            serde_json::to_string(&key.model).unwrap(),
            key.policy_version,
            serde_json::to_string(&key.stream_id).unwrap(),
            serde_json::to_string(&key.session_id).unwrap(),
            serde_json::to_string(&key.mind).unwrap()
        );
        let rewritten = receipt.replace(&serde_json::to_string(&key).unwrap(), &reordered_key);
        assert_ne!(receipt, rewritten);
        db.execute(
            "UPDATE memory_operation_receipts SET effect_json=?1 WHERE operation_id='page'",
            [rewritten],
        )
        .unwrap();
        assert_eq!(
            backend.formation_cursor(&key).await.unwrap(),
            Some(expected)
        );
        for field in 0..5 {
            let mut other = key.clone();
            match field {
                0 => other.mind = "other".into(),
                1 => other.session_id = "other".into(),
                2 => other.stream_id = "other".into(),
                3 => other.policy_version += 1,
                _ => {
                    other.model = if model.is_some() {
                        None
                    } else {
                        Some("fixture-model".into())
                    }
                }
            }
            assert!(backend.formation_cursor(&other).await.unwrap().is_none());
        }
    }
}

#[tokio::test]
async fn persisted_candidate_batch_replays_atomically_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("batch.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let mut request = page(1, 8);
    let formation = request.formation.as_mut().unwrap();
    formation.evidence.push(FormationEvidence {
        event_id: "event-8".into(),
        sequence: 8,
        recorded_at: "2026-09-21T00:00:00Z".into(),
        kind: EvidenceKind::UserStatement,
        excerpt: "Use SQLite; preserve rollback.".into(),
        truncated: false,
        outcome: None,
    });
    formation.extraction = ExtractionOutcome::Pending {
        model: "controlled-model".into(),
    };
    let stored = backend
        .apply_mutation(
            "batch-source",
            MemoryMutation::StoreCoveragePage {
                request,
                expected: None,
            },
        )
        .await
        .unwrap();
    let MemoryMutationEffect::CoverageStored { episode_id, .. } = stored.effect else {
        panic!("capture effect");
    };
    let mut completed = backend
        .list_episodes("capture", 1)
        .await
        .unwrap()
        .remove(0)
        .formation
        .unwrap();
    completed.extraction = ExtractionOutcome::Complete {
        model: "controlled-model".into(),
    };
    completed.candidates = vec![
        MemoryCandidate {
            content: "Use SQLite".into(),
            section: Section::Decisions,
            evidence_ids: vec!["event-8".into()],
        },
        MemoryCandidate {
            content: "Preserve rollback".into(),
            section: Section::Constraints,
            evidence_ids: vec!["event-8".into()],
        },
    ];
    let mutation = MemoryMutation::CompleteFormation {
        episode_id,
        formation: completed.clone(),
    };
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_batch BEFORE INSERT ON memory_operation_receipts WHEN NEW.operation_id='batch-complete' BEGIN SELECT RAISE(ABORT,'batch fault'); END;").unwrap();
    assert!(
        backend
            .apply_mutation("batch-complete", mutation.clone())
            .await
            .is_err()
    );
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert!(
        backend.list_episodes("capture", 1).await.unwrap()[0]
            .formation
            .as_ref()
            .unwrap()
            .candidates
            .is_empty()
    );
    sql.execute_batch("DROP TRIGGER reject_batch").unwrap();
    backend
        .apply_mutation("batch-complete", mutation.clone())
        .await
        .unwrap();
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    assert!(
        backend
            .apply_mutation("batch-complete", mutation)
            .await
            .unwrap()
            .replayed
    );
    assert_eq!(
        backend.list_episodes("capture", 1).await.unwrap()[0]
            .formation
            .as_ref()
            .unwrap(),
        &completed
    );
    assert!(
        backend
            .list_facts("capture", Default::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn concurrent_capture_writers_commit_only_one_cursor_transition() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.db");
    let backend = SqliteBackend::open(&path).unwrap();
    let request = page(1, 8);
    let key = formation::capture_key(&request).unwrap();
    backend
        .apply_mutation(
            "initial",
            MemoryMutation::StoreCoveragePage {
                request,
                expected: None,
            },
        )
        .await
        .unwrap();
    let expected = backend.formation_cursor(&key).await.unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = (0..2)
        .map(|index| {
            let backend = SqliteBackend::open(&path).unwrap();
            let barrier = barrier.clone();
            let expected = expected.clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let mut request = page(9, 16);
                request.narrative = format!("writer-{index}");
                barrier.wait();
                runtime.block_on(backend.apply_mutation(
                    &format!("contender-{index}"),
                    MemoryMutation::StoreCoveragePage { request, expected },
                ))
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(backend.list_episodes("capture", 10).await.unwrap().len(), 2);
    assert_eq!(
        backend
            .formation_cursor(&key)
            .await
            .unwrap()
            .unwrap()
            .sequence,
        16
    );
}

#[tokio::test]
async fn multi_session_workflow_preserves_failed_and_verified_attempts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workflow.db");
    let backend = SqliteBackend::open(&path).unwrap();
    for (session, outcome) in [
        ("attempt-failed", EvidenceOutcome::Failed),
        ("attempt-repaired", EvidenceOutcome::Succeeded),
    ] {
        let mut request = page(1, 8);
        let formation = request.formation.as_mut().unwrap();
        let FormationSource::Available { session_id, .. } = &mut formation.source else {
            panic!("available source");
        };
        *session_id = session.into();
        formation.evidence = vec![
            FormationEvidence {
                event_id: "assistant-claim".into(),
                sequence: 4,
                recorded_at: "2026-09-21T00:00:00Z".into(),
                kind: EvidenceKind::AssistantReport,
                excerpt: "The workflow succeeded.".into(),
                truncated: false,
                outcome: None,
            },
            FormationEvidence {
                event_id: "event-8".into(),
                sequence: 8,
                recorded_at: "2026-09-21T00:00:00Z".into(),
                kind: EvidenceKind::ToolResult,
                excerpt: if outcome == EvidenceOutcome::Failed {
                    "Verification failed."
                } else {
                    "Verification passed after the correction."
                }
                .into(),
                truncated: false,
                outcome: Some(outcome),
            },
        ];
        request.narrative = formation.narrative();
        backend
            .apply_mutation(
                session,
                MemoryMutation::StoreCoveragePage {
                    request,
                    expected: None,
                },
            )
            .await
            .unwrap();
    }
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    let copy = InMemoryBackend::new();
    copy.import_jsonl(&backend.export_jsonl("capture").await.unwrap())
        .await
        .unwrap();
    for store in [&backend as &dyn MemoryBackend, &copy] {
        let episodes = store
            .search_episodes("capture", "workflow", 10)
            .await
            .unwrap();
        assert_eq!(episodes.len(), 2);
        for episode in episodes {
            let formation = episode.formation.unwrap();
            let FormationSource::Available { session_id, .. } = formation.source else {
                panic!("source attribution");
            };
            assert_eq!(
                formation.evidence[0].outcome, None,
                "assistant assertion is not verification"
            );
            assert_eq!(
                formation.evidence[1].outcome,
                Some(if session_id == "attempt-failed" {
                    EvidenceOutcome::Failed
                } else {
                    EvidenceOutcome::Succeeded
                })
            );
        }
        assert!(
            store
                .list_facts("capture", Default::default())
                .await
                .unwrap()
                .is_empty()
        );
    }
}
