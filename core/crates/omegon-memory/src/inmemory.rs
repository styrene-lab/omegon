//! In-memory MemoryBackend — HashMap-based, no persistence.
//! Used for unit tests and ephemeral sessions.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::backend::*;
use crate::hash;
use crate::types::*;
use crate::util::{gen_id, now_iso};
use crate::vectors;

struct EmbeddingEntry {
    fact_id: String,
    model_name: String,
    embedding: Vec<f32>,
}

struct State {
    facts: HashMap<String, Fact>,
    edges: Vec<Edge>,
    episodes: Vec<Episode>,
    embeddings: Vec<EmbeddingEntry>,
    version_clock: u64,
}

pub struct InMemoryBackend {
    state: Mutex<State>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                facts: HashMap::new(),
                edges: Vec::new(),
                episodes: Vec::new(),
                embeddings: Vec::new(),
                version_clock: 0,
            }),
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryBackend for InMemoryBackend {
    async fn store_fact(&self, req: StoreFact) -> Result<StoreResult> {
        let mut s = self.state.lock().unwrap();
        let ch = hash::content_hash(&req.content);

        // Check for dedup by content hash within same mind — find ID first, then mutate
        let existing_id = s.facts.iter()
            .find(|(_, f)| {
                f.mind == req.mind
                    && f.content_hash.as_deref() == Some(ch.as_str())
                    && f.status == FactStatus::Active
            })
            .map(|(id, _)| id.clone());

        if let Some(id) = existing_id {
            s.version_clock += 1;
            let vc = s.version_clock;
            let ts = now_iso();
            let existing = s.facts.get_mut(&id).unwrap();
            existing.reinforcement_count += 1;
            existing.last_reinforced = ts;
            existing.version = vc;
            return Ok(StoreResult {
                fact: existing.clone(),
                action: StoreAction::Reinforced,
            });
        }

        s.version_clock += 1;
        let fact = Fact {
            id: gen_id(),
            mind: req.mind,
            content: req.content,
            section: req.section,
            status: FactStatus::Active,
            confidence: 1.0,
            reinforcement_count: 1,
            decay_rate: 0.05,
            decay_profile: req.decay_profile,
            last_reinforced: now_iso(),
            created_at: now_iso(),
            version: s.version_clock,
            superseded_by: None,
            source: req.source,
            content_hash: Some(ch),
            last_accessed: None,
        };
        s.facts.insert(fact.id.clone(), fact.clone());
        Ok(StoreResult {
            fact,
            action: StoreAction::Stored,
        })
    }

    async fn get_fact(&self, id: &str) -> Result<Option<Fact>> {
        let s = self.state.lock().unwrap();
        Ok(s.facts.get(id).filter(|f| f.status == FactStatus::Active).cloned())
    }

    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>> {
        let s = self.state.lock().unwrap();
        let status = filter.status.unwrap_or(FactStatus::Active);
        Ok(s.facts.values()
            .filter(|f| {
                f.mind == mind
                    && f.status == status
                    && filter.section.as_ref().is_none_or(|sec| &f.section == sec)
            })
            .cloned()
            .collect())
    }

    async fn reinforce_fact(&self, id: &str) -> Result<Fact> {
        let mut s = self.state.lock().unwrap();
        if !s.facts.contains_key(id) {
            return Err(MemoryError::FactNotFound(id.into()));
        }
        s.version_clock += 1;
        let vc = s.version_clock;
        let ts = now_iso();
        let fact = s.facts.get_mut(id).unwrap();
        fact.reinforcement_count += 1;
        fact.last_reinforced = ts;
        fact.version = vc;
        Ok(fact.clone())
    }

    async fn archive_facts(&self, ids: &[&str]) -> Result<usize> {
        let mut s = self.state.lock().unwrap();
        let mut count = 0;
        for id in ids {
            // Check if active first, then update
            let is_active = s.facts.get(*id).is_some_and(|f| f.status == FactStatus::Active);
            if is_active {
                s.version_clock += 1;
                let vc = s.version_clock;
                let fact = s.facts.get_mut(*id).unwrap();
                fact.status = FactStatus::Archived;
                fact.version = vc;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn supersede_fact(&self, id: &str, replacement: StoreFact) -> Result<Fact> {
        let mut s = self.state.lock().unwrap();

        if !s.facts.contains_key(id) {
            return Err(MemoryError::FactNotFound(id.into()));
        }

        // Create replacement first (no borrows on s.facts)
        s.version_clock += 1;
        let new_id = gen_id();
        let ch = hash::content_hash(&replacement.content);
        let new_fact = Fact {
            id: new_id.clone(),
            mind: replacement.mind,
            content: replacement.content,
            section: replacement.section,
            status: FactStatus::Active,
            confidence: 1.0,
            reinforcement_count: 1,
            decay_rate: 0.05,
            decay_profile: replacement.decay_profile,
            last_reinforced: now_iso(),
            created_at: now_iso(),
            version: s.version_clock,
            superseded_by: Some(id.to_string()), // "I supersede old_id"
            source: replacement.source,
            content_hash: Some(ch),
            last_accessed: None,
        };

        // Archive original — matches TS: original gets status='superseded', no forward pointer.
        s.version_clock += 1;
        let vc = s.version_clock;
        {
            let original = s.facts.get_mut(id).unwrap();
            original.status = FactStatus::Superseded;
            original.version = vc;
        }

        s.facts.insert(new_id, new_fact.clone());
        Ok(new_fact)
    }

    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>> {
        let s = self.state.lock().unwrap();
        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();

        let mut results: Vec<ScoredFact> = s.facts.values()
            .filter(|f| f.mind == mind && f.status == FactStatus::Active)
            .filter_map(|f| {
                let content_lower = f.content.to_lowercase();
                let matches = terms.iter().filter(|t| content_lower.contains(**t)).count();
                if matches == 0 { return None; }
                let relevance = matches as f64 / terms.len().max(1) as f64;
                Some(ScoredFact {
                    fact: f.clone(),
                    similarity: relevance,
                    score: relevance, // simplified — no decay weighting in memory backend
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);
        Ok(results)
    }

    async fn vector_search(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
    ) -> Result<Vec<ScoredFact>> {
        let s = self.state.lock().unwrap();

        // Find embeddings for this mind
        let mind_embeddings: Vec<&EmbeddingEntry> = s.embeddings.iter()
            .filter(|e| s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind && f.status == FactStatus::Active))
            .collect();

        if mind_embeddings.is_empty() {
            return Err(MemoryError::NoEmbeddings);
        }

        // Check dimension
        let expected_dims = mind_embeddings[0].embedding.len() as u32;
        let got_dims = embedding.len() as u32;
        if expected_dims != got_dims {
            return Err(MemoryError::EmbeddingDimensionMismatch {
                expected: expected_dims,
                got: got_dims,
                stored_model: mind_embeddings[0].model_name.clone(),
            });
        }

        let mut results: Vec<ScoredFact> = mind_embeddings.iter()
            .filter_map(|e| {
                let sim = vectors::cosine_similarity(&e.embedding, embedding);
                if sim < min_similarity { return None; }
                let fact = s.facts.get(&e.fact_id)?.clone();
                Some(ScoredFact {
                    fact,
                    similarity: sim as f64,
                    score: sim as f64,
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);
        Ok(results)
    }

    async fn store_embedding(
        &self,
        fact_id: &str,
        model_name: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let mut s = self.state.lock().unwrap();
        // Remove existing embedding for this fact
        s.embeddings.retain(|e| e.fact_id != fact_id);
        s.embeddings.push(EmbeddingEntry {
            fact_id: fact_id.into(),
            model_name: model_name.into(),
            embedding: embedding.to_vec(),
        });
        Ok(())
    }

    async fn embedding_metadata(&self, mind: &str) -> Result<Option<EmbeddingMetadata>> {
        let s = self.state.lock().unwrap();
        let entry = s.embeddings.iter().find(|e| {
            s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind)
        });
        Ok(entry.map(|e| EmbeddingMetadata {
            model_name: e.model_name.clone(),
            dims: e.embedding.len() as u32,
            inserted_at: now_iso(),
        }))
    }

    async fn create_edge(&self, req: CreateEdge) -> Result<Edge> {
        let mut s = self.state.lock().unwrap();
        let edge = Edge {
            id: gen_id(),
            source_id: req.source_id,
            target_id: req.target_id,
            relation: req.relation,
            description: req.description,
            weight: 1.0,
            created_at: now_iso(),
        };
        s.edges.push(edge.clone());
        Ok(edge)
    }

    async fn get_edges(&self, _mind: &str, fact_id: &str) -> Result<Vec<Edge>> {
        let s = self.state.lock().unwrap();
        Ok(s.edges.iter()
            .filter(|e| e.source_id == fact_id || e.target_id == fact_id)
            .cloned()
            .collect())
    }

    async fn store_episode(&self, req: StoreEpisode) -> Result<Episode> {
        let mut s = self.state.lock().unwrap();
        let episode = Episode {
            id: gen_id(),
            mind: req.mind,
            date: req.date.unwrap_or_else(|| "2026-03-18".into()),
            title: req.title,
            narrative: req.narrative,
            created_at: now_iso(),
            affected_nodes: req.affected_nodes,
            affected_changes: req.affected_changes,
            files_changed: req.files_changed,
            tags: req.tags,
            tool_calls_count: req.tool_calls_count,
        };
        s.episodes.push(episode.clone());
        Ok(episode)
    }

    async fn list_episodes(&self, mind: &str, k: usize) -> Result<Vec<Episode>> {
        let s = self.state.lock().unwrap();
        let mut eps: Vec<Episode> = s.episodes.iter()
            .filter(|e| e.mind == mind)
            .cloned()
            .collect();
        eps.reverse(); // most recent first
        eps.truncate(k);
        Ok(eps)
    }

    async fn search_episodes(&self, mind: &str, query: &str, k: usize) -> Result<Vec<Episode>> {
        let s = self.state.lock().unwrap();
        let query_lower = query.to_lowercase();
        let mut results: Vec<Episode> = s.episodes.iter()
            .filter(|e| e.mind == mind && e.narrative.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();
        results.truncate(k);
        Ok(results)
    }

    async fn export_jsonl(&self, mind: &str) -> Result<String> {
        let s = self.state.lock().unwrap();
        let mut lines = Vec::new();

        // Facts (sorted by id for determinism)
        let mut facts: Vec<&Fact> = s.facts.values()
            .filter(|f| f.mind == mind && f.status == FactStatus::Active)
            .collect();
        facts.sort_by(|a, b| a.id.cmp(&b.id));
        for fact in facts {
            let record = JsonlRecord::Fact(JsonlFact {
                id: fact.id.clone(),
                mind: fact.mind.clone(),
                content: fact.content.clone(),
                section: fact.section.clone(),
                status: fact.status.clone(),
                created_at: fact.created_at.clone(),
                source: fact.source.clone(),
                content_hash: fact.content_hash.clone(),
                supersedes: None,
                version: fact.version,
                decay_profile: fact.decay_profile.clone(),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }

        // Edges
        let mut edges: Vec<&Edge> = s.edges.iter()
            .filter(|e| {
                s.facts.get(&e.source_id).is_some_and(|f| f.mind == mind)
            })
            .collect();
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        for edge in edges {
            lines.push(serde_json::to_string(&JsonlRecord::Edge(edge.clone())).unwrap());
        }

        // Episodes
        let mut eps: Vec<&Episode> = s.episodes.iter()
            .filter(|e| e.mind == mind)
            .collect();
        eps.sort_by(|a, b| a.id.cmp(&b.id));
        for ep in eps {
            lines.push(serde_json::to_string(&JsonlRecord::Episode(ep.clone())).unwrap());
        }

        Ok(lines.join("\n"))
    }

    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        for line in jsonl.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            match serde_json::from_str::<JsonlRecord>(trimmed) {
                Ok(JsonlRecord::Fact(jf)) => {
                    let mut s = self.state.lock().unwrap();
                    if let Some(existing) = s.facts.get(&jf.id) {
                        if jf.version > existing.version {
                            // Higher version wins
                            let mut updated = existing.clone();
                            updated.content = jf.content;
                            updated.section = jf.section;
                            updated.version = jf.version;
                            s.facts.insert(jf.id, updated);
                            stats.reinforced += 1;
                        } else {
                            stats.skipped += 1;
                        }
                    } else {
                        s.version_clock = s.version_clock.max(jf.version) + 1;
                        let fact = Fact {
                            id: jf.id.clone(),
                            mind: jf.mind,
                            content: jf.content,
                            section: jf.section,
                            status: jf.status,
                            confidence: 1.0,
                            reinforcement_count: 1,
                            decay_rate: 0.05,
                            decay_profile: jf.decay_profile,
                            last_reinforced: jf.created_at.clone(),
                            created_at: jf.created_at,
                            version: jf.version,
                            superseded_by: None,
                            source: jf.source,
                            content_hash: jf.content_hash,
                            last_accessed: None,
                        };
                        s.facts.insert(jf.id, fact);
                        stats.imported += 1;
                    }
                }
                Ok(JsonlRecord::Episode(ep)) => {
                    let mut s = self.state.lock().unwrap();
                    s.episodes.push(ep);
                    stats.imported += 1;
                }
                Ok(JsonlRecord::Edge(edge)) => {
                    let mut s = self.state.lock().unwrap();
                    s.edges.push(edge);
                    stats.imported += 1;
                }
                Ok(JsonlRecord::Mind(_)) => {
                    // Minds are informational — no-op for import
                    stats.skipped += 1;
                }
                Err(_) => {
                    stats.errors += 1;
                }
            }
        }
        Ok(stats)
    }

    async fn stats(&self, mind: &str) -> Result<MemoryStats> {
        let s = self.state.lock().unwrap();
        let mind_facts: Vec<&Fact> = s.facts.values().filter(|f| f.mind == mind).collect();
        let active = mind_facts.iter().filter(|f| f.status == FactStatus::Active).count();
        let archived = mind_facts.iter().filter(|f| f.status == FactStatus::Archived).count();
        let superseded = mind_facts.iter().filter(|f| f.status == FactStatus::Superseded).count();
        let with_vectors = s.embeddings.iter()
            .filter(|e| s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind))
            .count();
        let meta = s.embeddings.iter().find(|e| {
            s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind)
        });
        let episodes = s.episodes.iter().filter(|e| e.mind == mind).count();
        let edges = s.edges.iter().filter(|e| {
            s.facts.get(&e.source_id).is_some_and(|f| f.mind == mind)
        }).count();
        let version_hwm = s.facts.values().filter(|f| f.mind == mind).map(|f| f.version).max().unwrap_or(0);

        Ok(MemoryStats {
            total_facts: mind_facts.len(),
            active_facts: active,
            archived_facts: archived,
            superseded_facts: superseded,
            facts_with_vectors: with_vectors,
            embedding_model: meta.map(|e| e.model_name.clone()),
            embedding_dims: meta.map(|e| e.embedding.len() as u32),
            episodes,
            edges,
            version_hwm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::run_backend_tests;

    #[tokio::test]
    async fn inmemory_backend_passes_all_tests() {
        let backend = InMemoryBackend::new();
        run_backend_tests(&backend).await;
    }
}
