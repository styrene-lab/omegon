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
    use std::collections::HashSet;

    let mut seen: HashSet<String> = results.iter().map(|sf| sf.fact.id.clone()).collect();
    let mut expanded = results.clone();

    for sf in &results {
        let edges = match backend.get_edges(mind, &sf.fact.id).await {
            Ok(edges) => edges,
            Err(e) => {
                tracing::debug!(fact_id = %sf.fact.id, error = %e, "edge lookup failed");
                continue;
            }
        };

        for edge in edges {
            let neighbor_id = if edge.source_id == sf.fact.id {
                &edge.target_id
            } else {
                &edge.source_id
            };

            if !seen.insert(neighbor_id.clone()) {
                continue;
            }

            if let Ok(Some(neighbor)) = backend.get_fact(neighbor_id).await {
                let derived_score = sf.score * edge.confidence * 0.5;
                expanded.push(ScoredFact {
                    similarity: derived_score,
                    score: derived_score,
                    fact: neighbor,
                });
            }
        }
    }

    expanded.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    expanded.truncate(limit);
    expanded
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
        ScoredFact {
            similarity: 1.0,
            score: 1.0,
            fact: result.fact,
        }
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
}
