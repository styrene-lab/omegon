//! Offline regression evidence. Fixture input contains no expected answers;
//! expected IDs live only in evaluator assertions below.
use omegon_memory::{ContextRenderer, Fact, MarkdownRenderer, MemoryBackend, SqliteBackend};

const FIXTURE: &str = include_str!("fixtures/retrieval.jsonl");

#[tokio::test]
async fn filtered_channels_respect_section_scope_and_read_only_history() {
    use omegon_memory::{CreateEdge, SearchFilter, SearchIntent, Section};
    let backends: Vec<Box<dyn MemoryBackend>> = vec![
        Box::new(SqliteBackend::in_memory().unwrap()),
        Box::new(omegon_memory::InMemoryBackend::new()),
    ];
    for backend in backends {
        backend.import_jsonl(FIXTURE).await.unwrap();
        let before = backend.export_jsonl("wave").await.unwrap();
        let space = omegon_memory::EmbeddingSpace {
            model: "fixture-space".into(),
            revision: "fixture-v1".into(),
            preprocessing: "raw-v1".into(),
            dimensions: 2,
        };
        for id in ["current", "constraint"] {
            let fact = backend.get_fact(id).await.unwrap().unwrap();
            backend
                .apply_mutation(
                    &format!("index-{id}"),
                    omegon_memory::MemoryMutation::StoreIdentifiedEmbedding {
                        fact: omegon_memory::FactPrecondition {
                            id: id.into(),
                            expected_version: fact.version,
                        },
                        embedding: omegon_memory::IdentifiedEmbedding {
                            space: space.clone(),
                            values: vec![1.0, 0.0],
                        },
                    },
                )
                .await
                .unwrap();
        }
        backend
            .create_edge(CreateEdge {
                source_id: "constraint".into(),
                target_id: "current".into(),
                relation: "related".into(),
                description: None,
            })
            .await
            .unwrap();
        let filter = SearchFilter {
            context: None,
            section: Some(Section::Constraints),
            intent: SearchIntent::Current,
        };
        let seeds = backend
            .search_identified(
                "wave",
                &omegon_memory::IdentifiedEmbedding {
                    space,
                    values: vec![1.0, 0.0],
                },
                1,
                0.0,
                &filter,
                &|| false,
            )
            .await
            .unwrap()
            .results;
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].fact.id, "constraint");
        let expanded = omegon_memory::service::expand_edges_filtered_cancellable(
            backend.as_ref(),
            "wave",
            seeds,
            10,
            &filter,
            &|| false,
        )
        .await
        .unwrap();
        assert_eq!(expanded.len(), 1);
        for query in ["\"zircon", "zircon\"", "\"zircon\""] {
            let results = backend
                .fts_search_filtered("wave", query, 1, &filter)
                .await
                .unwrap();
            assert_eq!(results[0].fact.id, "constraint", "{query}");
        }
        assert!(
            backend
                .fts_search("wave", "   ", 10)
                .await
                .unwrap()
                .is_empty()
        );
        let historical = SearchFilter {
            context: None,
            intent: SearchIntent::Historical,
            section: None,
        };
        let a = backend
            .fts_search_filtered("wave", "zircon", 20, &historical)
            .await
            .unwrap();
        let b = backend
            .fts_search_filtered("wave", "zircon", 20, &historical)
            .await
            .unwrap();
        assert_eq!(
            a.iter().map(|f| &f.fact.id).collect::<Vec<_>>(),
            b.iter().map(|f| &f.fact.id).collect::<Vec<_>>()
        );
        assert!(
            a.iter()
                .all(|f| f.fact.mind == "wave" && f.fact.reinforcement_count == 1)
        );
        // Strip edges added for traversal from the before/after fact comparison.
        let facts = |jsonl: String| {
            jsonl
                .lines()
                .filter(|line| line.contains("\"_type\":\"fact\""))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            facts(before),
            facts(backend.export_jsonl("wave").await.unwrap())
        );
    }
}

#[tokio::test]
async fn historical_search_survives_reopen_and_storage_failure_is_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("facts.db");
    let backend = SqliteBackend::open(&path).unwrap();
    populate(&backend).await;
    drop(backend);
    let backend = SqliteBackend::open(&path).unwrap();
    let filter = omegon_memory::SearchFilter {
        context: None,
        intent: omegon_memory::SearchIntent::Historical,
        section: None,
    };
    let found = backend
        .fts_search_filtered("wave", "zircon", 20, &filter)
        .await
        .unwrap();
    assert_eq!(found.len(), 3);
    let sabotage = rusqlite::Connection::open(&path).unwrap();
    sabotage.execute_batch("DROP TABLE facts_fts").unwrap();
    assert!(
        backend
            .fts_search_filtered("wave", "zircon", 20, &filter)
            .await
            .is_err()
    );
}

async fn populate(backend: &impl MemoryBackend) {
    let result = backend
        .import_jsonl(FIXTURE)
        .await
        .expect("load synthetic evidence");
    assert_eq!(result.errors, 0);
}

async fn fresh_fact(id: &str, content: &str) -> Fact {
    let backend = omegon_memory::InMemoryBackend::new();
    let mut fact = backend
        .store_fact(omegon_memory::StoreFact {
            mind: "wave".into(),
            content: content.into(),
            section: omegon_memory::Section::Architecture,
            decay_profile: Default::default(),
            source: Some("synthetic".into()),
        })
        .await
        .expect("store fixture")
        .fact;
    fact.id = id.into();
    fact
}

#[tokio::test]
async fn packing_deduplicates_pins_and_recalled_facts() {
    let fact = fresh_fact("pinned", "one canonical constraint").await;
    let pins = [fact.clone(), fact.clone()];
    let rendered = MarkdownRenderer.render_context(std::slice::from_ref(&fact), &[], &pins, 1000);
    assert_eq!(rendered.facts_injected, 1);
    assert_eq!(
        rendered
            .markdown
            .matches("one canonical constraint")
            .count(),
        1
    );
}

#[tokio::test]
async fn packing_skips_oversized_first_candidate() {
    let large = fresh_fact("large", &"🧠".repeat(2000)).await;
    let small = fresh_fact("small", "useful small evidence").await;
    let rendered = MarkdownRenderer.render_context(&[large, small], &[], &[], 500);
    assert!(rendered.markdown.contains("useful small evidence"));
    assert!(rendered.budget_exhausted);
    assert!(rendered.char_count <= 500);
}

#[cfg(feature = "agent")]
mod tools {
    use super::*;
    use omegon_traits::{ContentBlock, ToolProvider};
    use serde_json::json;

    #[tokio::test]
    async fn task_context_excludes_unrelated_architecture_flood() {
        use omegon_traits::{ContextProvider, ContextSignals, LifecyclePhase};
        let backend = SqliteBackend::in_memory().unwrap();
        populate(&backend).await;
        for i in 0..40 {
            backend
                .store_fact(omegon_memory::StoreFact {
                    mind: "wave".into(),
                    content: format!("unrelated-layout-{i} {}", "padding ".repeat(40)),
                    section: omegon_memory::Section::Architecture,
                    decay_profile: Default::default(),
                    source: None,
                })
                .await
                .unwrap();
        }
        let provider = omegon_memory::MemoryProvider::new(backend, MarkdownRenderer, "wave".into());
        provider
            .execute(
                "memory_focus",
                "foreign-pin",
                json!({"fact_ids":["foreign_current", "obsolete"]}),
                Default::default(),
            )
            .await
            .unwrap();
        let signals = ContextSignals {
            user_prompt: "atomic migration",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::default(),
            turn_number: 1,
            context_budget_tokens: 150,
        };
        let context = provider
            .provide_context(&signals)
            .expect("matching context");
        assert!(context.content.contains("atomic migration"));
        assert!(!context.content.contains("unrelated-layout"));
        assert!(!context.content.contains("[foreign_current]"));
        assert!(!context.content.contains("[obsolete]"));
        assert!(context.content.chars().count() <= 600);
    }

    async fn archive(backend: impl MemoryBackend + 'static) {
        populate(&backend).await;
        let before = backend.export_jsonl("wave").await.unwrap();
        let provider = omegon_memory::MemoryProvider::new(backend, MarkdownRenderer, "wave".into());
        let result = provider
            .execute(
                "memory_search_archive",
                "archive",
                json!({"query":"zircon"}),
                Default::default(),
            )
            .await
            .unwrap();
        let text = result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for id in ["obsolete", "sleeping", "replaced"] {
            assert!(text.contains(&format!("[{id}]")), "missing {id}: {text}");
        }
        for id in ["current", "constraint", "foreign"] {
            assert!(
                !text.contains(&format!("[{id}]")),
                "ineligible {id}: {text}"
            );
        }
        assert_eq!(
            before,
            provider.backend().export_jsonl("wave").await.unwrap()
        );
    }

    async fn section(backend: impl MemoryBackend + 'static) {
        populate(&backend).await;
        // Add many lexical distractors so filtering after retrieval cannot pass.
        for i in 0..24 {
            backend
                .store_fact(omegon_memory::StoreFact {
                    mind: "wave".into(),
                    content: format!("zircon {i}"),
                    section: omegon_memory::Section::Architecture,
                    decay_profile: Default::default(),
                    source: None,
                })
                .await
                .unwrap();
        }
        let provider = omegon_memory::MemoryProvider::new(backend, MarkdownRenderer, "wave".into());
        let result = provider
            .execute(
                "memory_recall",
                "section",
                json!({"query":"zircon", "section":"Constraints", "k":1}),
                Default::default(),
            )
            .await
            .unwrap();
        let text = format!("{:?}", result.content);
        assert!(text.contains("[constraint]"), "{text}");
        assert_eq!(result.details["count"], 1);
    }

    #[tokio::test]
    async fn sqlite_archive_population() {
        archive(SqliteBackend::in_memory().unwrap()).await;
    }
    #[tokio::test]
    async fn inmemory_archive_population() {
        archive(omegon_memory::InMemoryBackend::new()).await;
    }
    #[tokio::test]
    async fn sqlite_section_before_limit() {
        section(SqliteBackend::in_memory().unwrap()).await;
    }
    #[tokio::test]
    async fn inmemory_section_before_limit() {
        section(omegon_memory::InMemoryBackend::new()).await;
    }
}
