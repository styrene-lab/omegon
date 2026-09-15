//! Bounded selection-snapshot reuse. Eligibility is strict; rank freshness is bounded.
use crate::*;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_AGE_SECONDS: i64 = 30;
static NEXT_BACKEND: AtomicU64 = AtomicU64::new(1);
pub(crate) fn backend_identity() -> u64 {
    NEXT_BACKEND
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionRevision {
    pub instance: u64,
    pub local: u64,
    pub external: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SelectionTimeBounds {
    pub query_before: Option<DateTime<Utc>>,
    pub wall_before: Option<DateTime<Utc>>,
}
impl SelectionTimeBounds {
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        applicability: Option<&RecordedApplicability>,
        confidence: f64,
        reinforcement_count: u32,
        profile: &DecayProfileName,
        last_reinforced: &str,
        query_at: DateTime<Utc>,
        wall_at: DateTime<Utc>,
    ) {
        for boundary in applicability
            .into_iter()
            .flat_map(|record| {
                [
                    record.constraints.valid_from.as_deref(),
                    record.constraints.valid_until.as_deref(),
                ]
            })
            .flatten()
        {
            if let Ok(at) = DateTime::parse_from_rfc3339(boundary) {
                let at = at.with_timezone(&Utc);
                if at > query_at && self.query_before.is_none_or(|previous| at < previous) {
                    self.query_before = Some(at);
                }
            }
        }
        if let Some(at) = decay::confidence_floor_deadline(
            confidence,
            reinforcement_count,
            profile,
            last_reinforced,
            wall_at,
        ) && self.wall_before.is_none_or(|previous| at < previous)
        {
            self.wall_before = Some(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn time(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }
    fn request(at: DateTime<Utc>) -> MemorySelectionRequest {
        MemorySelectionRequest {
            mind: "test".into(),
            query: "zircon".into(),
            pins: vec![],
            context: ApplicabilityContext {
                at: Some(at.to_rfc3339()),
                ..Default::default()
            },
            intent: MemorySelectionIntent::Ambient,
            host_budget: 1024,
            memory_cap: 1024,
            fetch_limit: 512,
        }
    }
    async fn seed(backend: &dyn MemoryBackend, id: &str, valid_from: Option<&str>) {
        let mut row = serde_json::json!({"_type":"fact","id":id,"mind":"test","content":format!("zircon {id}"),"section":"Constraints","status":"active","created_at":"2026-01-01T00:00:00Z","version":1,
            "operational":{"confidence":1.0,"reinforcement_count":1,"decay_rate":0.05,"last_reinforced":"2100-01-01T00:00:00Z"}});
        if let Some(start) = valid_from {
            row["applicability"] = serde_json::json!({"constraints":{"valid_from":start},"recorded_at":"2026-01-01T00:00:00Z"});
        }
        backend.import_jsonl(&row.to_string()).await.unwrap();
    }
    async fn cached(
        cache: &mut MemorySelectionCache,
        backend: &dyn MemoryBackend,
        request: &MemorySelectionRequest,
        now: DateTime<Utc>,
        calls: &AtomicUsize,
    ) -> MemorySelection {
        cache
            .compute(
                backend,
                request,
                &selection::ConservativeUtf8Counter,
                &MarkdownRenderer,
                now,
                || async {
                    calls.fetch_add(1, Ordering::Relaxed);
                    selection::retrieve_and_select(
                        backend,
                        request,
                        &selection::ConservativeUtf8Counter,
                    )
                    .await
                },
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn cache_skips_retrieval_and_expires_for_previously_excluded_future_facts() {
        for backend in [
            Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
            Box::new(SqliteBackend::in_memory().unwrap()),
        ] {
            seed(backend.as_ref(), "present", None).await;
            seed(backend.as_ref(), "future", Some("2026-01-01T00:00:05Z")).await;
            let mut cache = MemorySelectionCache::default();
            let calls = AtomicUsize::new(0);
            let start = time("2026-01-01T00:00:00Z");
            let first = cached(&mut cache, backend.as_ref(), &request(start), start, &calls).await;
            assert_eq!(first.report.selected.len(), 1);
            let next = start + Duration::seconds(1);
            assert!(
                cached(&mut cache, backend.as_ref(), &request(next), next, &calls)
                    .await
                    .report
                    .cache_hit
            );
            assert_eq!(
                calls.load(Ordering::Relaxed),
                1,
                "cache hit must not invoke retrieval"
            );
            let boundary = start + Duration::seconds(5);
            let after = cached(
                &mut cache,
                backend.as_ref(),
                &request(boundary),
                boundary,
                &calls,
            )
            .await;
            assert!(!after.report.cache_hit);
            assert_eq!(after.report.selected.len(), 2);
            assert_eq!(calls.load(Ordering::Relaxed), 2);
            backend.archive_facts(&["present"]).await.unwrap();
            let changed = cached(
                &mut cache,
                backend.as_ref(),
                &request(boundary),
                boundary,
                &calls,
            )
            .await;
            assert!(!changed.report.cache_hit);
            assert!(
                !changed
                    .report
                    .selected
                    .iter()
                    .any(|handle| handle.id == "present")
            );
        }
    }

    #[tokio::test]
    async fn cache_key_covers_task_target_pins_policy_and_budget_and_clock_reversal() {
        let backend = InMemoryBackend::new();
        seed(&backend, "present", None).await;
        let mut cache = MemorySelectionCache::default();
        let calls = AtomicUsize::new(0);
        let start = time("2026-01-01T00:00:10Z");
        let mut input = request(start);
        cached(&mut cache, &backend, &input, start, &calls).await;
        input.host_budget = 10;
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.memory_cap = 8;
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.query = "other".into();
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.context.workspace = Some("other-workspace".into());
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.pins.push("present".into());
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.context.revision = Some("git:0123456789012345678901234567890123456789".into());
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.intent = MemorySelectionIntent::Explicit;
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        input.fetch_limit = 1;
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        assert!(
            !cached(
                &mut cache,
                &backend,
                &input,
                start - Duration::seconds(1),
                &calls
            )
            .await
            .report
            .cache_hit
        );
        assert!(
            !cached(
                &mut cache,
                &backend,
                &input,
                start + Duration::seconds(31),
                &calls
            )
            .await
            .report
            .cache_hit
        );
    }

    #[tokio::test]
    async fn sqlite_external_commits_and_backend_instances_invalidate_cache() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("facts.db");
        let backend = SqliteBackend::open(&path).unwrap();
        seed(&backend, "present", None).await;
        let mut cache = MemorySelectionCache::default();
        let calls = AtomicUsize::new(0);
        let start = time("2026-01-01T00:00:00Z");
        let input = request(start);
        cached(&mut cache, &backend, &input, start, &calls).await;
        let external = rusqlite::Connection::open(&path).unwrap();
        external
            .execute("UPDATE facts SET status='archived' WHERE id='present'", [])
            .unwrap();
        assert!(
            !cached(&mut cache, &backend, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
        let reopened = SqliteBackend::open(&path).unwrap();
        assert!(
            !cached(&mut cache, &reopened, &input, start, &calls)
                .await
                .report
                .cache_hit
        );
    }

    #[tokio::test]
    async fn cached_guidance_retires_at_exclusive_valid_until() {
        for backend in [
            Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
            Box::new(SqliteBackend::in_memory().unwrap()),
        ] {
            let row = serde_json::json!({"_type":"fact","id":"expires","mind":"test","content":"zircon expires","section":"Constraints","status":"active","created_at":"2026-01-01T00:00:00Z","version":1,
                "operational":{"confidence":1.0,"reinforcement_count":1,"decay_rate":0.05,"last_reinforced":"2100-01-01T00:00:00Z"},
                "applicability":{"constraints":{"valid_until":"2026-01-01T00:00:05Z"},"recorded_at":"2026-01-01T00:00:00Z"}});
            backend.import_jsonl(&row.to_string()).await.unwrap();
            let mut cache = MemorySelectionCache::default();
            let calls = AtomicUsize::new(0);
            let start = time("2026-01-01T00:00:00Z");
            assert_eq!(
                cached(&mut cache, backend.as_ref(), &request(start), start, &calls)
                    .await
                    .report
                    .selected
                    .len(),
                1
            );
            assert!(
                cached(&mut cache, backend.as_ref(), &request(start), start, &calls)
                    .await
                    .report
                    .cache_hit
            );
            let end = start + Duration::seconds(5);
            let retired = cached(&mut cache, backend.as_ref(), &request(end), end, &calls).await;
            assert!(!retired.report.cache_hit);
            assert!(retired.markdown.is_empty());
            assert_eq!(calls.load(Ordering::Relaxed), 2);
        }
    }

    #[test]
    fn confidence_floor_boundary_prevents_eligible_snapshot_reuse() {
        let now = time("2026-01-01T00:00:00Z");
        assert_eq!(
            decay::confidence_floor_deadline(
                0.1,
                1,
                &DecayProfileName::Standard,
                "2026-01-01T00:00:00Z",
                now
            ),
            Some(now)
        );
        assert_eq!(
            decay::confidence_floor_deadline(
                0.1,
                1,
                &DecayProfileName::Standard,
                "2026-01-01T00:00:00Z",
                now + Duration::seconds(1)
            ),
            None
        );
    }

    #[tokio::test]
    async fn backend_stamps_cover_episode_and_edge_writes() {
        for backend in [
            Box::new(InMemoryBackend::new()) as Box<dyn MemoryBackend>,
            Box::new(SqliteBackend::in_memory().unwrap()),
        ] {
            seed(backend.as_ref(), "a", None).await;
            seed(backend.as_ref(), "b", None).await;
            let before = backend.selection_revision().await.unwrap();
            backend
                .store_episode(StoreEpisode {
                    mind: "test".into(),
                    title: "zircon".into(),
                    narrative: "episode".into(),
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
            let episode = backend.selection_revision().await.unwrap();
            assert_ne!(before, episode);
            backend
                .create_edge(CreateEdge {
                    source_id: "a".into(),
                    target_id: "b".into(),
                    relation: "related".into(),
                    description: None,
                })
                .await
                .unwrap();
            assert_ne!(episode, backend.selection_revision().await.unwrap());
        }
    }

    #[tokio::test]
    async fn mutation_during_computation_does_not_publish_a_reusable_snapshot() {
        let backend = InMemoryBackend::new();
        seed(&backend, "present", None).await;
        let mut cache = MemorySelectionCache::default();
        let now = time("2026-01-01T00:00:00Z");
        let input = request(now);
        let selected = cache
            .compute(
                &backend,
                &input,
                &selection::ConservativeUtf8Counter,
                &MarkdownRenderer,
                now,
                || async {
                    let selected = selection::retrieve_and_select(
                        &backend,
                        &input,
                        &selection::ConservativeUtf8Counter,
                    )
                    .await?;
                    backend.archive_facts(&["present"]).await?;
                    Ok(selected)
                },
            )
            .await
            .unwrap();
        assert!(selected.report.cache_expires_at.is_none());
        assert!(cache.entry.is_none());
    }
}

struct Entry {
    key: String,
    revision: SelectionRevision,
    query_at: DateTime<Utc>,
    wall_at: DateTime<Utc>,
    query_before: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    selection: MemorySelection,
}

/// One snapshot per owner. A different task/target/budget replaces it.
#[derive(Default)]
pub struct MemorySelectionCache {
    entry: Option<Entry>,
}
impl MemorySelectionCache {
    pub fn clear(&mut self) {
        self.entry = None;
    }

    pub async fn select(
        &mut self,
        backend: &dyn MemoryBackend,
        request: &MemorySelectionRequest,
        counter: &dyn selection::MemoryTokenCounter,
        renderer: &dyn ContextRenderer,
    ) -> backend::Result<MemorySelection> {
        let filter = SearchFilter {
            context: Some(request.context.clone()),
            ..Default::default()
        }
        .resolved()?;
        let normalized = MemorySelectionRequest {
            context: filter.context.expect("resolved context"),
            ..request.clone()
        };
        let request = &normalized;
        if request.host_budget.min(request.memory_cap) == 0
            || (request.intent == MemorySelectionIntent::Ambient
                && selection::low_signal(&request.query)
                && request.pins.is_empty())
        {
            self.clear();
            return selection::retrieve_and_select_with_renderer(
                backend, request, counter, renderer,
            )
            .await;
        }
        self.compute(backend, request, counter, renderer, Utc::now(), || {
            selection::retrieve_and_select_with_renderer(backend, request, counter, renderer)
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn compute<F, Fut>(
        &mut self,
        backend: &dyn MemoryBackend,
        request: &MemorySelectionRequest,
        counter: &dyn selection::MemoryTokenCounter,
        renderer: &dyn ContextRenderer,
        now: DateTime<Utc>,
        load: F,
    ) -> backend::Result<MemorySelection>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = backend::Result<MemorySelection>>,
    {
        request.context.validate()?;
        let query_at = request
            .context
            .at
            .as_deref()
            .map(DateTime::parse_from_rfc3339)
            .transpose()
            .map_err(|_| MemoryError::InvalidMutation("invalid cache query time".into()))?
            .map(|at| at.with_timezone(&Utc))
            .unwrap_or(now);
        let mut target = request.context.clone();
        target.at = None;
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&(
                &request.mind,
                &request.query,
                &request.pins,
                target,
                request.intent,
                request.host_budget,
                request.memory_cap,
                request.fetch_limit,
                counter.accounting(),
                renderer.memory_cache_identity(),
                "selection-cache-v1",
            ))
            .map_err(|error| MemoryError::Storage(error.into()))?,
        ));
        let revision = backend.selection_revision().await?;
        if let (Some(entry), Some(revision)) = (&self.entry, &revision)
            && renderer.memory_cache_identity().is_some()
            && entry.key == key
            && &entry.revision == revision
            && query_at >= entry.query_at
            && now >= entry.wall_at
            && now < entry.expires_at
            && entry.query_before.is_none_or(|at| query_at < at)
        {
            let mut selected = entry.selection.clone();
            selected.report.cache_hit = true;
            return Ok(selected);
        }
        self.entry = None;
        let bounds = if revision.is_some() && renderer.memory_cache_identity().is_some() {
            backend
                .selection_time_bounds(&request.mind, query_at, now)
                .await?
        } else {
            None
        };
        let mut selected = load().await?;
        selected.report.selected_at = Some(now.to_rfc3339());
        if let (Some(revision), Some(bounds)) = (revision, bounds)
            && backend.selection_revision().await?.as_ref() == Some(&revision)
        {
            let expires_at = bounds
                .wall_before
                .map_or(now + Duration::seconds(MAX_AGE_SECONDS), |at| {
                    at.min(now + Duration::seconds(MAX_AGE_SECONDS))
                });
            if expires_at > now {
                selected.report.cache_expires_at = Some(expires_at.to_rfc3339());
                self.entry = Some(Entry {
                    key,
                    revision,
                    query_at,
                    wall_at: now,
                    query_before: bounds.query_before,
                    expires_at,
                    selection: selected.clone(),
                });
            }
        }
        Ok(selected)
    }
}
