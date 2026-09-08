//! Memory Mind service policy layer.
//!
//! This module holds reusable memory-domain behavior that should not depend on
//! Omegon's harness/tool adapter. Keep provider calls, ToolResult formatting,
//! and context-injection TTL policy outside this layer.

use std::sync::Arc;

use crate::{MemoryBackend, ScoredFact};

/// Reusable semantic-memory service over a [`MemoryBackend`].
pub struct MemoryMindService {
    backend: Arc<dyn MemoryBackend>,
    mind: String,
}

impl MemoryMindService {
    pub fn new(backend: Arc<dyn MemoryBackend>, mind: impl Into<String>) -> Self {
        Self {
            backend,
            mind: mind.into(),
        }
    }

    /// 1-hop edge expansion for recall results.
    ///
    /// For each seed fact, fetch edges, load neighbor facts, and score each
    /// neighbor as `parent_score × edge.confidence × 0.5`. Seed facts are not
    /// duplicated. The result is sorted by derived score and truncated to
    /// `limit`.
    pub async fn expand_edges(&self, results: Vec<ScoredFact>, limit: usize) -> Vec<ScoredFact> {
        expand_edges(self.backend.as_ref(), &self.mind, results, limit).await
    }
}

pub async fn expand_edges(
    backend: &dyn MemoryBackend,
    mind: &str,
    results: Vec<ScoredFact>,
    limit: usize,
) -> Vec<ScoredFact> {
    expand_edges_cancellable(backend, mind, results.clone(), limit, &|| false)
        .await
        .unwrap_or(results)
}

pub async fn expand_edges_cancellable(
    backend: &dyn MemoryBackend,
    mind: &str,
    results: Vec<ScoredFact>,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<ScoredFact>> {
    expand_edges_filtered_cancellable(
        backend,
        mind,
        results,
        limit,
        &crate::SearchFilter::default(),
        cancelled,
    )
    .await
}

pub async fn expand_edges_filtered_cancellable(
    backend: &dyn MemoryBackend,
    mind: &str,
    results: Vec<ScoredFact>,
    limit: usize,
    filter: &crate::SearchFilter,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<ScoredFact>> {
    expand_edges_filtered_checked(backend, mind, results, limit, filter, cancelled)
        .await
        .ok()
}

pub async fn expand_edges_filtered_checked<C: Fn() -> bool + ?Sized>(
    backend: &dyn MemoryBackend,
    mind: &str,
    mut results: Vec<ScoredFact>,
    limit: usize,
    filter: &crate::SearchFilter,
    cancelled: &C,
) -> crate::backend::Result<Vec<ScoredFact>> {
    if cancelled() {
        return Err(crate::MemoryError::Cancelled);
    }
    use std::collections::{BTreeMap, HashSet};

    const MAX_SEEDS: usize = 1_000;
    const MAX_EDGES_PER_SEED: usize = 64;
    const MAX_NEIGHBOR_LOADS: usize = 4_096;

    results.retain(|result| result.fact.mind == mind && filter.score(1.0, &result.fact).is_some());

    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.fact.id.cmp(&right.fact.id))
    });
    let seeds: HashSet<String> = results
        .iter()
        .map(|result| result.fact.id.clone())
        .collect();
    let seed_results = results.iter().take(MAX_SEEDS).cloned().collect::<Vec<_>>();
    let mut candidates = BTreeMap::<String, (f64, Vec<crate::GraphEvidence>)>::new();
    for result in &seed_results {
        if cancelled() {
            return Err(crate::MemoryError::Cancelled);
        }
        let mut edges = backend
            .get_edges_filtered(mind, &result.fact.id, filter, MAX_EDGES_PER_SEED)
            .await?;
        edges.sort_by(|left, right| {
            right
                .confidence
                .partial_cmp(&left.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.id.cmp(&right.id))
        });
        for edge in edges.into_iter().take(MAX_EDGES_PER_SEED) {
            if !edge.confidence.is_finite()
                || edge.confidence <= 0.0
                || edge.confidence > 1.0
                || edge.relation.len() > 128
            {
                continue;
            }
            let relation = edge.relation.trim().to_lowercase().replace([' ', '-'], "_");
            let kind = relation_kind(&relation);
            let neighbor_id = if edge.source_id == result.fact.id {
                edge.target_id.clone()
            } else {
                edge.source_id.clone()
            };
            let evidence = crate::GraphEvidence {
                edge_id: edge.id,
                other_fact_id: result.fact.id.clone(),
                relation: edge.relation,
                outgoing: edge.source_id == neighbor_id,
                kind,
            };
            if seeds.contains(&neighbor_id) {
                if let Some(seed) = results.iter_mut().find(|seed| seed.fact.id == neighbor_id) {
                    add_graph_evidence(&mut seed.graph_evidence, evidence);
                }
                continue;
            }
            if kind == crate::GraphRelationKind::Unknown {
                continue;
            }
            if filter.intent == crate::SearchIntent::Current
                && ((relation == "supersedes" && edge.source_id == result.fact.id)
                    || (relation == "superseded_by" && edge.target_id == result.fact.id))
            {
                continue;
            }
            let score = result.score * edge.confidence * 0.5;
            let candidate = candidates.entry(neighbor_id).or_insert((score, vec![]));
            candidate.0 = candidate.0.max(score);
            add_graph_evidence(&mut candidate.1, evidence);
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|(left_id, left), (right_id, right)| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left_id.cmp(right_id))
    });
    for (neighbor_id, (score, evidence)) in candidates.into_iter().take(MAX_NEIGHBOR_LOADS) {
        if cancelled() {
            return Err(crate::MemoryError::Cancelled);
        }
        if let Some(fact) = backend
            .get_fact_filtered(mind, &neighbor_id, filter)
            .await?
        {
            if fact.mind != mind {
                continue;
            }
            let Some(score) = filter.score(score, &fact) else {
                continue;
            };
            for relationship in &evidence {
                if let Some(seed) = results
                    .iter_mut()
                    .find(|seed| seed.fact.id == relationship.other_fact_id)
                {
                    let mut inverse = relationship.clone();
                    inverse.other_fact_id = neighbor_id.clone();
                    inverse.outgoing = !inverse.outgoing;
                    add_graph_evidence(&mut seed.graph_evidence, inverse);
                }
            }
            let mut result = ScoredFact::new(fact, score, score);
            result.scores.graph = Some(score);
            result.graph_evidence = evidence;
            results.push(result);
        }
    }
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.fact.id.cmp(&b.fact.id))
    });
    results.truncate(limit);
    if cancelled() {
        return Err(crate::MemoryError::Cancelled);
    }
    Ok(results)
}

fn relation_kind(relation: &str) -> crate::GraphRelationKind {
    use crate::GraphRelationKind::*;
    match relation {
        "related" | "related_to" | "depends_on" | "required_by" => Related,
        "supports" | "supported_by" => Support,
        "contradicts" | "contradiction" | "conflicts_with" | "contradicted_by" => Contradiction,
        "supersedes" | "superseded_by" => Supersession,
        _ => Unknown,
    }
}

fn add_graph_evidence(items: &mut Vec<crate::GraphEvidence>, item: crate::GraphEvidence) {
    if items.len() < 16
        && !items
            .iter()
            .any(|existing| existing.edge_id == item.edge_id)
    {
        items.push(item);
    }
}

/// Shared early context policy: explicit task matches, otherwise eligible current
/// inventory. Presentation preserves this ranking instead of imposing section order.
pub async fn context_facts(
    backend: &dyn MemoryBackend,
    mind: &str,
    query: Option<&str>,
    limit: usize,
) -> crate::backend::Result<Vec<crate::Fact>> {
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        return Ok(backend
            .fts_search(mind, query, limit)
            .await?
            .into_iter()
            .map(|result| result.fact)
            .collect());
    }
    let mut facts = backend
        .list_facts(mind, crate::FactFilter::default())
        .await?;
    facts.retain(|fact| crate::decay::ambient_score(1.0, fact).is_some());
    facts.truncate(limit);
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreateEdge, DecayProfileName, InMemoryBackend, Section, StoreFact};

    async fn store(backend: &Arc<dyn MemoryBackend>, mind: &str, content: &str) -> ScoredFact {
        let result = backend
            .store_fact(StoreFact {
                mind: mind.to_string(),
                content: content.to_string(),
                section: Section::Architecture,
                source: Some("test".into()),
                decay_profile: DecayProfileName::Standard,
            })
            .await
            .unwrap();
        ScoredFact::new(result.fact, 1.0, 1.0)
    }

    #[tokio::test]
    async fn edge_expansion_adds_scored_neighbors_without_duplicates() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let a = store(&backend, "default", "Fact A about routing boundaries").await;
        let b = store(&backend, "default", "Fact B about adapter boundaries").await;

        backend
            .create_edge(CreateEdge {
                source_id: a.fact.id.clone(),
                target_id: b.fact.id.clone(),
                relation: "related".into(),
                description: None,
            })
            .await
            .unwrap();

        let expanded = expand_edges(backend.as_ref(), "default", vec![a.clone()], 10).await;
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0].fact.id, a.fact.id);
        assert_eq!(expanded[1].fact.id, b.fact.id);
        assert!((expanded[1].score - 0.5).abs() < f64::EPSILON);

        let expanded_again = expand_edges(backend.as_ref(), "default", expanded, 10).await;
        let b_count = expanded_again
            .iter()
            .filter(|fact| fact.fact.id == b.fact.id)
            .count();
        assert_eq!(b_count, 1);
    }

    #[tokio::test]
    async fn edge_expansion_respects_limit() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let a = store(&backend, "default", "Fact A about context").await;
        let b = store(&backend, "default", "Fact B about context").await;

        backend
            .create_edge(CreateEdge {
                source_id: a.fact.id.clone(),
                target_id: b.fact.id.clone(),
                relation: "related".into(),
                description: None,
            })
            .await
            .unwrap();

        let expanded = expand_edges(backend.as_ref(), "default", vec![a], 1).await;
        assert_eq!(expanded.len(), 1);
    }

    #[tokio::test]
    async fn edge_expansion_is_cancellable_and_ties_are_fact_id_ordered() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let seed = store(&backend, "default", "seed").await;
        let left = store(&backend, "default", "left").await;
        let right = store(&backend, "default", "right").await;
        for neighbor in [&right, &left] {
            backend
                .create_edge(CreateEdge {
                    source_id: seed.fact.id.clone(),
                    target_id: neighbor.fact.id.clone(),
                    relation: "related".into(),
                    description: None,
                })
                .await
                .unwrap();
        }
        assert!(
            expand_edges_cancellable(backend.as_ref(), "default", vec![seed.clone()], 10, &|| {
                true
            })
            .await
            .is_none()
        );
        let expanded = expand_edges(backend.as_ref(), "default", vec![seed], 10).await;
        let actual = expanded[1..]
            .iter()
            .map(|result| result.fact.id.as_str())
            .collect::<Vec<_>>();
        let mut expected = vec![left.fact.id.as_str(), right.fact.id.as_str()];
        expected.sort_unstable();
        assert_eq!(actual, expected);
    }
}
