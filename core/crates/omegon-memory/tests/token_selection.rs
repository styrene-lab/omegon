use omegon_memory::selection::*;
use omegon_memory::*;

async fn fact(backend: &InMemoryBackend, content: &str) -> Fact {
    backend
        .store_fact(StoreFact {
            mind: "test".into(),
            content: content.into(),
            section: Section::Constraints,
            source: None,
            decay_profile: Default::default(),
        })
        .await
        .unwrap()
        .fact
}

#[tokio::test]
async fn conservative_packing_counts_unicode_metadata_and_skips_oversized_claims() {
    let backend = InMemoryBackend::new();
    let large = fact(&backend, &format!("zircon {}", "漢🦀".repeat(100))).await;
    let small = fact(&backend, "zircon small").await;
    let facts = vec![large.clone(), small.clone()];
    let context = ApplicabilityContext::local();
    let selected = select(
        MemorySelectionInput {
            mind: "test",
            query: "zircon task",
            intent: MemorySelectionIntent::Ambient,
            facts: &facts,
            pins: &[],
            episodes: &[],
            context: &context,
            host_budget: 200,
            memory_cap: 1024,
        },
        &ConservativeUtf8Counter,
    )
    .unwrap();
    assert_eq!(selected.report.accounted_tokens, selected.markdown.len());
    assert!(selected.report.accounted_tokens <= 200);
    assert_eq!(selected.report.selected.len(), 1);
    assert_eq!(selected.report.selected[0].id, small.id);
    assert!(
        selected
            .report
            .exclusions
            .iter()
            .any(|entry| entry.id == large.id && entry.reason == MemoryExclusionReason::Budget)
    );
    assert!(selected.markdown.contains("applicability unknown"));
    let zero = select(
        MemorySelectionInput {
            mind: "test",
            query: "zircon",
            intent: MemorySelectionIntent::Ambient,
            facts: &facts,
            pins: &[],
            episodes: &[],
            context: &context,
            host_budget: 0,
            memory_cap: 1024,
        },
        &ConservativeUtf8Counter,
    )
    .unwrap();
    assert!(zero.markdown.is_empty());
    assert_eq!(zero.report.accounted_tokens, 0);
}

#[tokio::test]
async fn selection_reports_distinct_eligibility_reasons_and_deduplicates_pins() {
    let backend = InMemoryBackend::new();
    let pin = fact(&backend, "zircon pinned").await;
    let mut inactive = fact(&backend, "zircon old").await;
    inactive.status = FactStatus::Archived;
    let mut other = fact(&backend, "zircon other").await;
    other.applicability = Some(Box::new(
        RecordedApplicability::new(ApplicabilityConstraints {
            platforms: vec![
                if std::env::consts::OS == "linux" {
                    "macos"
                } else {
                    "linux"
                }
                .into(),
            ],
            ..Default::default()
        })
        .unwrap(),
    ));
    let mut weak = fact(&backend, "zircon weak").await;
    weak.confidence = 0.0;
    let facts = vec![pin.clone(), inactive, other, weak];
    let context = ApplicabilityContext::local();
    let selected = select(
        MemorySelectionInput {
            mind: "test",
            query: "zircon task",
            intent: MemorySelectionIntent::Ambient,
            facts: &facts,
            pins: std::slice::from_ref(&pin),
            episodes: &[],
            context: &context,
            host_budget: 1024,
            memory_cap: 1024,
        },
        &ConservativeUtf8Counter,
    )
    .unwrap();
    assert_eq!(selected.report.selected.len(), 1);
    for reason in [
        MemoryExclusionReason::Duplicate,
        MemoryExclusionReason::Lifecycle,
        MemoryExclusionReason::Applicability,
        MemoryExclusionReason::Confidence,
    ] {
        assert_eq!(selected.report.exclusion_counts.get(&reason), Some(&1));
    }
    assert_eq!(selected.markdown.matches(&pin.id).count(), 1);
}

struct ScalarFixtureCounter;
impl MemoryTokenCounter for ScalarFixtureCounter {
    fn count(&self, text: &str) -> usize {
        text.chars().count()
    }
    fn accounting(&self) -> MemoryTokenAccounting {
        MemoryTokenAccounting::Exact {
            tokenizer: "unicode-scalar-fixture".into(),
        }
    }
}

#[tokio::test]
async fn exact_counter_receives_complete_emitted_text() {
    let backend = InMemoryBackend::new();
    let unicode = fact(&backend, &"漢".repeat(80)).await;
    let context = ApplicabilityContext::local();
    let selected = select(
        MemorySelectionInput {
            mind: "test",
            query: "unicode task",
            intent: MemorySelectionIntent::Ambient,
            facts: std::slice::from_ref(&unicode),
            pins: &[],
            episodes: &[],
            context: &context,
            host_budget: 200,
            memory_cap: 1024,
        },
        &ScalarFixtureCounter,
    )
    .unwrap();
    assert_eq!(selected.report.selected.len(), 1);
    assert_eq!(
        selected.report.accounted_tokens,
        selected.markdown.chars().count()
    );
    assert!(selected.markdown.len() > selected.report.accounted_tokens);
}

#[tokio::test]
async fn low_signal_is_pin_only_and_superseded_pins_resolve_without_revival() {
    let backend = InMemoryBackend::new();
    let original = fact(&backend, "old decision").await;
    let replacement = backend
        .supersede_fact(
            &original.id,
            StoreFact {
                mind: "test".into(),
                content: "replacement decision".into(),
                section: Section::Decisions,
                source: None,
                decay_profile: Default::default(),
            },
        )
        .await
        .unwrap();
    let before = backend.export_jsonl("test").await.unwrap();
    let selected = retrieve_and_select(
        &backend,
        &MemorySelectionRequest {
            mind: "test".into(),
            query: "thanks".into(),
            pins: vec![original.id.clone()],
            context: ApplicabilityContext::local(),
            intent: MemorySelectionIntent::Ambient,
            host_budget: 1024,
            memory_cap: 1024,
            fetch_limit: 512,
        },
        &ConservativeUtf8Counter,
    )
    .await
    .unwrap();
    assert_eq!(selected.report.selected[0].id, replacement.id);
    assert_eq!(selected.report.pin_resolutions[0].requested_id, original.id);
    assert!(!selected.markdown.contains("old decision"));
    assert_eq!(backend.export_jsonl("test").await.unwrap(), before);
}

#[tokio::test]
async fn episodic_additions_require_signal_and_a_bounded_share() {
    let backend = InMemoryBackend::new();
    let fact = fact(&backend, "task evidence").await;
    let episode = backend
        .store_episode(StoreEpisode {
            mind: "test".into(),
            title: "task episode".into(),
            narrative: "x".repeat(400),
            date: None,
            affected_nodes: vec![],
            affected_changes: vec![],
            files_changed: vec![],
            tags: vec![],
            tool_calls_count: None,
            formation: None,
        })
        .await
        .unwrap();
    let context = ApplicabilityContext::local();
    for (query, reason) in [
        ("thanks", MemoryExclusionReason::LowSignal),
        ("task evidence", MemoryExclusionReason::EpisodeBudget),
    ] {
        let selected = select(
            MemorySelectionInput {
                mind: "test",
                query,
                intent: MemorySelectionIntent::Ambient,
                facts: std::slice::from_ref(&fact),
                pins: &[],
                episodes: std::slice::from_ref(&episode),
                context: &context,
                host_budget: 1024,
                memory_cap: 1024,
            },
            &ConservativeUtf8Counter,
        )
        .unwrap();
        assert!(
            selected
                .report
                .exclusions
                .iter()
                .any(|entry| entry.id == episode.id && entry.reason == reason)
        );
        assert!(
            !selected
                .report
                .selected
                .iter()
                .any(|handle| handle.kind == "episode")
        );
    }
}

#[tokio::test]
async fn development_caps_trade_evidence_volume_without_exceeding_accounted_budget() {
    let backend = InMemoryBackend::new();
    let mut facts = Vec::new();
    for index in 0..12 {
        facts.push(fact(&backend,&format!("zircon constraint {index}: preserve deterministic transitions and retain durable source references")).await);
    }
    let context = ApplicabilityContext::local();
    let mut counts = Vec::new();
    for cap in [512, 1024, 2048] {
        let selected = select(
            MemorySelectionInput {
                mind: "test",
                query: "zircon constraints",
                intent: MemorySelectionIntent::Ambient,
                facts: &facts,
                pins: &facts[..1],
                episodes: &[],
                context: &context,
                host_budget: 8192,
                memory_cap: cap,
            },
            &ConservativeUtf8Counter,
        )
        .unwrap();
        assert!(selected.report.accounted_tokens <= cap);
        counts.push(selected.report.selected.len());
        println!(
            "development cap={cap}, selected={}, accounted={}",
            selected.report.selected.len(),
            selected.report.accounted_tokens
        );
    }
    assert!(counts[0] < counts[1] && counts[1] < counts[2]);
}
