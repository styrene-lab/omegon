#[path = "support/evaluation.rs"]
mod evaluation;
use evaluation::*;
use omegon_memory::*;
#[tokio::test]
async fn cutoff_and_labels_never_reach_writers_or_readers() {
    let case = cases("development").unwrap().remove(0);
    let view = case.view().unwrap();
    assert_eq!(view.events.len(), 1);
    let prompt = extraction_prompt(&view);
    assert!(!prompt.contains("FUTURE_CANARY"));
    assert!(!prompt.contains("required"));
    let malicious = r#"[{"content":"future claim","section":"Constraints","evidence_ids":["d2"]}]"#;
    let trace = adapt(
        &view,
        Adapter::CurrentMemory,
        &InMemoryBackend::new(),
        Ok(malicious),
        1024,
    )
    .await
    .unwrap();
    assert!(trace.formation_error.is_some());
    assert!(trace.retained.is_empty());
    assert!(!reader_prompt(&view.query, &trace).contains("future claim"));
}
#[tokio::test]
async fn deterministic_real_backends_and_paired_budgets() {
    for sqlite in [false, true] {
        let first = offline("development", sqlite).await.unwrap();
        assert_eq!(first, offline("development", sqlite).await.unwrap());
        assert_eq!(first.len(), 16);
        for row in &first {
            assert!(row.trace.injected_tokens <= row.trace.budget);
            if row.trace.adapter != Adapter::NoMemory {
                assert_eq!(row.task, Outcome::Pass, "{} {:?}", row.case_id, row.trace);
            }
        }
        let summary = summarize(&first);
        assert_eq!(summary[2]["budgets"], serde_json::json!([1024]));
        assert_eq!(summary[3]["budgets"], serde_json::json!([256]));
    }
}
#[tokio::test]
async fn packing_miss_absence_and_failed_dependencies_are_separate() {
    let case = cases("development").unwrap().remove(0);
    let view = case.view().unwrap();
    let extraction = fake_extraction(&view);
    let trace = adapt(
        &view,
        Adapter::CurrentMemory,
        &InMemoryBackend::new(),
        Ok(&extraction),
        1,
    )
    .await
    .unwrap();
    let row = score(&case.id, &labels()[&case.id], trace, Some("ABSTAIN".into()));
    assert_eq!(row.formation, Outcome::Pass);
    assert_eq!(row.retrieval, Outcome::Pass);
    assert_eq!(row.selection, Outcome::Miss);
    assert_eq!(row.task, Outcome::Miss);
    let trace = adapt(
        &view,
        Adapter::CurrentMemory,
        &InMemoryBackend::new(),
        Err("extractor unavailable"),
        1024,
    )
    .await
    .unwrap();
    let mut row = score(&case.id, &labels()[&case.id], trace, None);
    row.trace.ingestion_cost = Cost::unavailable();
    assert_eq!(row.formation, Outcome::Unavailable);
    assert_eq!(row.retrieval, Outcome::Unavailable);
    assert_eq!(row.task, Outcome::Unavailable);
    assert!(
        serde_json::to_value(&row).unwrap()["trace"]["ingestion_cost"]["input_tokens"].is_null()
    );
    assert_eq!(summarize(&[row])[2]["incomplete"], 1);
    let absent = cases("development").unwrap().remove(1);
    let trace = adapt(
        &absent.view().unwrap(),
        Adapter::NoMemory,
        &InMemoryBackend::new(),
        Ok("[]"),
        1024,
    )
    .await
    .unwrap();
    let row = score(
        &absent.id,
        &labels()[&absent.id],
        trace,
        Some("certainly healthy".into()),
    );
    assert_eq!(row.retrieval, Outcome::NotApplicable);
    assert_eq!(row.task, Outcome::Miss);
}
#[tokio::test]
async fn heldout_offline_report() {
    let rows = offline("heldout", true).await.unwrap();
    assert_eq!(rows.len(), 16);
    if let Ok(path) = std::env::var("OMEGON_MEMORY_EVAL_OFFLINE_REPORT") {
        let manifest = serde_json::json!({
            "revision":REVISION, "tier":"offline", "split":"heldout", "backend":"sqlite_in_memory",
            "corpus_sha256":retrieval::raw_content_hash(CORPUS), "labels_sha256":retrieval::raw_content_hash(LABELS),
            "extractor":"controlled_supported_observation_copy_v1", "reader":"scripted_verbatim_claims_or_abstain_v1",
            "scorer_revision":"cutoff-grounded-verbatim-claims-v1",
            "clock":"case cutoff for applicability; fixed 9999 reinforcement epoch neutralizes wall-clock decay",
            "embedding":"disabled", "current_cap":1024, "candidate_cap":256,
            "accounting":"conservative_utf8_bytes", "provider_tokens":0, "provider_cost_usd":0,
            "latency":"not measured in deterministic contract tier", "file_curation_labor":"unmeasured",
            "quality_scope":"synthetic deterministic policy contract, not model quality"
        });
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"manifest":manifest,"rows":rows,"summary":summarize(&rows)}),
            )
            .unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn fixture_schema_rejects_labels_and_compares_actual_instants() {
    let leaked = r#"{"id":"bad","split":"development","query":"zircon","cutoff":"2026-01-02T00:00:00Z","events":[],"gold_answer":"SECRET_GOLD_CANARY"}"#;
    assert!(serde_json::from_str::<Case>(leaked).is_err());
    let offset = r#"{"id":"offset","split":"development","query":"zircon","cutoff":"2026-01-02T00:00:00Z","events":[{"id":"earlier","at":"2026-01-02T01:00:00+02:00","text":"earlier","kind":"user_statement"},{"id":"later","at":"2026-01-01T23:30:00-02:00","text":"FUTURE_CANARY","kind":"user_statement"}]}"#;
    let view = serde_json::from_str::<Case>(offset)
        .unwrap()
        .view()
        .unwrap();
    assert_eq!(view.events.len(), 1);
    assert_eq!(view.events[0].id, "earlier");
}

#[tokio::test]
async fn budget_ablation_preserves_small_evidence_after_oversized_lexical_noise() {
    let case = cases("development").unwrap().remove(0);
    let mut view = case.view().unwrap();
    view.events.insert(
        0,
        Event {
            id: "noise".into(),
            at: view.cutoff.clone(),
            text: format!("zircon {}", "irrelevant architecture ".repeat(20)),
            kind: EvidenceKind::UserStatement,
        },
    );
    let extraction = fake_extraction(&view);
    for backend_kind in [false, true] {
        let mut traces = vec![];
        for adapter in [Adapter::CurrentMemory, Adapter::CandidatePolicy] {
            let backend: Box<dyn MemoryBackend> = if backend_kind {
                Box::new(SqliteBackend::in_memory().unwrap())
            } else {
                Box::new(InMemoryBackend::new())
            };
            let trace = adapt(&view, adapter, backend.as_ref(), Ok(&extraction), 1024)
                .await
                .unwrap();
            assert!(trace.selected.contains(&"d1".into()));
            traces.push(trace);
        }
        assert!(traces[0].injected_tokens > traces[1].injected_tokens);
        assert!(!traces[1].selected.contains(&"noise".into()));
    }
}

#[tokio::test]
async fn unsupported_correct_guess_does_not_hide_missing_evidence() {
    let case = cases("development").unwrap().remove(0);
    let trace = adapt(
        &case.view().unwrap(),
        Adapter::NoMemory,
        &InMemoryBackend::new(),
        Ok("[]"),
        1024,
    )
    .await
    .unwrap();
    let row = score(
        &case.id,
        &labels()[&case.id],
        trace,
        Some("atomic migration".into()),
    );
    assert_eq!(row.task, Outcome::Miss);
    let view = case.view().unwrap();
    let extraction = fake_extraction(&view);
    let trace = adapt(
        &view,
        Adapter::CurrentMemory,
        &InMemoryBackend::new(),
        Ok(&extraction),
        1024,
    )
    .await
    .unwrap();
    let row = score(
        &case.id,
        &labels()[&case.id],
        trace,
        Some("do not use atomic migration".into()),
    );
    assert_eq!(row.task, Outcome::Uncertain);
    assert_eq!(summarize(&[row])[2]["judge_uncertain"], 1);
}

#[tokio::test]
async fn keyword_overlap_cannot_turn_contradictions_into_success() {
    for (split, id, contradiction) in [
        (
            "development",
            "dev-cutoff",
            "Atomic migration is unnecessary.",
        ),
        (
            "heldout",
            "held-constraint",
            "macOS is optional for signing.",
        ),
    ] {
        let case = cases(split)
            .unwrap()
            .into_iter()
            .find(|case| case.id == id)
            .unwrap();
        let view = case.view().unwrap();
        let extraction = fake_extraction(&view);
        let mut trace = adapt(
            &view,
            Adapter::CurrentMemory,
            &InMemoryBackend::new(),
            Ok(&extraction),
            1024,
        )
        .await
        .unwrap();
        assert_eq!(
            score(id, &labels()[id], trace.clone(), Some(fake_reader(&trace))).task,
            Outcome::Pass
        );
        assert_ne!(
            score(id, &labels()[id], trace.clone(), Some(contradiction.into())).task,
            Outcome::Pass
        );
        let answer = serde_json::json!({"claims":[contradiction]}).to_string();
        assert_eq!(
            score(id, &labels()[id], trace.clone(), Some(answer.clone())).task,
            Outcome::Miss
        );
        // Even a writer that retained the right source ID while changing its
        // modality must not manufacture support for the opposite conclusion.
        trace.selected_claims.push(contradiction.into());
        assert_eq!(
            score(id, &labels()[id], trace, Some(answer)).task,
            Outcome::Miss
        );
    }
}
