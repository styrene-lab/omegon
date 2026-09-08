//! In-memory MemoryBackend — HashMap-based, no persistence.
//! Used for unit tests and ephemeral sessions.

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::backend::*;
use crate::hash;
use crate::types::*;
use crate::util::{gen_id, now_iso};
use crate::vectors;

#[derive(Clone)]
struct EmbeddingEntry {
    fact_id: String,
    model_name: String,
    embedding: Vec<f32>,
    inserted_at: String,
    space: Option<EmbeddingSpace>,
    source_hash: Option<String>,
}

#[derive(Clone)]
struct State {
    facts: HashMap<String, Fact>,
    fact_insertion_sequences: HashMap<String, u64>,
    next_fact_insertion_sequence: u64,
    edges: Vec<Edge>,
    episodes: Vec<Episode>,
    embeddings: Vec<EmbeddingEntry>,
    version_clock: u64,
    operation_receipts: HashMap<String, (String, MemoryMutationEffect)>,
}

pub struct InMemoryBackend {
    state: Mutex<State>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                facts: HashMap::new(),
                fact_insertion_sequences: HashMap::new(),
                next_fact_insertion_sequence: 0,
                edges: Vec::new(),
                episodes: Vec::new(),
                embeddings: Vec::new(),
                version_clock: 0,
                operation_receipts: HashMap::new(),
            }),
        }
    }

    fn check_fact_precondition(state: &State, fact: &FactPrecondition) -> Result<()> {
        let existing = state
            .facts
            .get(&fact.id)
            .ok_or_else(|| MemoryError::FactNotFound(fact.id.clone()))?;
        if existing.version != fact.expected_version {
            return Err(MemoryError::FactVersionConflict {
                id: fact.id.clone(),
                expected: fact.expected_version,
                actual: existing.version,
            });
        }
        Ok(())
    }

    fn insert_fact(state: &mut State, id: String, fact: Fact) -> Result<()> {
        if !state.fact_insertion_sequences.contains_key(&id) {
            state.next_fact_insertion_sequence = state
                .next_fact_insertion_sequence
                .checked_add(1)
                .ok_or_else(|| {
                    MemoryError::InvalidMutation("fact insertion sequence exhausted".into())
                })?;
            state
                .fact_insertion_sequences
                .insert(id.clone(), state.next_fact_insertion_sequence);
        }
        state.facts.insert(id, fact);
        Ok(())
    }

    fn next_version(state: &mut State) -> Result<u64> {
        let version = state.version_clock.checked_add(1).ok_or_else(|| {
            MemoryError::InvalidMutation("Lamport version space is exhausted".into())
        })?;
        persisted_lamport_version(version)?;
        state.version_clock = version;
        Ok(version)
    }

    fn apply_to_state(state: &mut State, mutation: MemoryMutation) -> Result<MemoryMutationEffect> {
        let inference = match &mutation {
            MemoryMutation::StoreLifecycleInference { inference, .. } => Some(inference.clone()),
            _ => None,
        };
        match mutation {
            MemoryMutation::ImportJsonl { jsonl } => {
                let stats = Self::import_jsonl_to_state(state, &jsonl)?;
                Ok(jsonl_import_effect(stats))
            }
            MemoryMutation::StoreFact { request }
            | MemoryMutation::StoreLifecycleInference { request, .. } => {
                let content_hash = hash::content_hash(&request.content);
                let existing_id = state
                    .facts
                    .iter()
                    .find(|(_, fact)| {
                        inference.is_none()
                            && fact.mind == request.mind
                            && fact.content_hash.as_deref() == Some(content_hash.as_str())
                            && fact.status == FactStatus::Active
                    })
                    .map(|(id, _)| id.clone());
                let version = Self::next_version(state)?;
                if let Some(fact_id) = existing_id {
                    let fact = state.facts.get_mut(&fact_id).ok_or_else(|| {
                        MemoryError::Storage(anyhow::anyhow!("deduplicated fact disappeared"))
                    })?;
                    fact.reinforcement_count += 1;
                    fact.last_reinforced = now_iso();
                    fact.version = version;
                    return Ok(MemoryMutationEffect::FactStored {
                        fact_id,
                        version,
                        action: StoreAction::Reinforced,
                    });
                }

                let fact_id = gen_id();
                let timestamp = now_iso();
                Self::insert_fact(
                    state,
                    fact_id.clone(),
                    Fact {
                        id: fact_id.clone(),
                        mind: request.mind,
                        content: request.content,
                        section: request.section,
                        status: if inference.is_some() {
                            FactStatus::Pending
                        } else {
                            FactStatus::Active
                        },
                        confidence: if inference.is_some() { 0.0 } else { 1.0 },
                        reinforcement_count: if inference.is_some() { 0 } else { 1 },
                        lifecycle_inference: inference,
                        decay_rate: 0.05,
                        decay_profile: request.decay_profile,
                        last_reinforced: timestamp.clone(),
                        created_at: timestamp,
                        version,
                        superseded_by: None,
                        source: request.source,
                        content_hash: Some(content_hash),
                        last_accessed: None,
                        created_session: None,
                        superseded_at: None,
                        archived_at: None,
                        jj_change_id: None,
                        persona_id: None,
                        layer: "project".into(),
                        tags: vec![],
                    },
                )?;
                Ok(MemoryMutationEffect::FactStored {
                    fact_id,
                    version,
                    action: StoreAction::Stored,
                })
            }
            MemoryMutation::ReinforceFact { fact } => {
                Self::check_fact_precondition(state, &fact)?;
                let version = Self::next_version(state)?;
                let existing = state.facts.get_mut(&fact.id).ok_or_else(|| {
                    MemoryError::Storage(anyhow::anyhow!("validated fact disappeared"))
                })?;
                if existing.status != FactStatus::Active {
                    return Err(MemoryError::FactNotFound(fact.id));
                }
                existing.reinforcement_count += 1;
                existing.last_reinforced = now_iso();
                existing.version = version;
                Ok(MemoryMutationEffect::FactReinforced {
                    fact_id: existing.id.clone(),
                    version,
                    reinforcement_count: existing.reinforcement_count,
                })
            }
            MemoryMutation::ReinforceFactOnce { fact_id } => {
                let version = Self::next_version(state)?;
                let existing = state
                    .facts
                    .get_mut(&fact_id)
                    .ok_or_else(|| MemoryError::FactNotFound(fact_id.clone()))?;
                if existing.status != FactStatus::Active {
                    return Err(MemoryError::FactNotFound(fact_id));
                }
                existing.reinforcement_count += 1;
                existing.last_reinforced = now_iso();
                existing.version = version;
                Ok(MemoryMutationEffect::FactReinforced {
                    fact_id: existing.id.clone(),
                    version,
                    reinforcement_count: existing.reinforcement_count,
                })
            }
            MemoryMutation::TransitionFacts { facts, status } => {
                if !matches!(status, FactStatus::Dormant | FactStatus::Archived) {
                    return Err(MemoryError::InvalidMutation(
                        "transition target must be dormant or archived".into(),
                    ));
                }
                validate_unique_fact_preconditions(&facts)?;
                for fact in &facts {
                    Self::check_fact_precondition(state, fact)?;
                }
                let mut transitioned = Vec::new();
                for fact in facts {
                    let is_active = state
                        .facts
                        .get(&fact.id)
                        .is_some_and(|existing| existing.status == FactStatus::Active);
                    if !is_active {
                        continue;
                    }
                    let version = Self::next_version(state)?;
                    let existing = state.facts.get_mut(&fact.id).ok_or_else(|| {
                        MemoryError::Storage(anyhow::anyhow!("validated fact disappeared"))
                    })?;
                    existing.status = status.clone();
                    existing.version = version;
                    transitioned.push(FactPrecondition {
                        id: fact.id,
                        expected_version: version,
                    });
                }
                Ok(MemoryMutationEffect::FactsTransitioned {
                    facts: transitioned,
                    status,
                })
            }
            MemoryMutation::SupersedeFact { fact, replacement } => {
                Self::check_fact_precondition(state, &fact)?;
                if state
                    .facts
                    .get(&fact.id)
                    .is_none_or(|existing| existing.status != FactStatus::Active)
                {
                    return Err(MemoryError::FactNotFound(fact.id));
                }
                let original_version = Self::next_version(state)?;
                let original = state.facts.get_mut(&fact.id).ok_or_else(|| {
                    MemoryError::Storage(anyhow::anyhow!("validated fact disappeared"))
                })?;
                original.status = FactStatus::Superseded;
                original.version = original_version;

                let replacement_version = Self::next_version(state)?;
                let replacement_id = gen_id();
                let timestamp = now_iso();
                Self::insert_fact(
                    state,
                    replacement_id.clone(),
                    Fact {
                        id: replacement_id.clone(),
                        lifecycle_inference: None,
                        mind: replacement.mind,
                        content_hash: Some(hash::content_hash(&replacement.content)),
                        content: replacement.content,
                        section: replacement.section,
                        status: FactStatus::Active,
                        confidence: 1.0,
                        reinforcement_count: 1,
                        decay_rate: 0.05,
                        decay_profile: replacement.decay_profile,
                        last_reinforced: timestamp.clone(),
                        created_at: timestamp,
                        version: replacement_version,
                        superseded_by: Some(fact.id.clone()),
                        source: replacement.source,
                        last_accessed: None,
                        created_session: None,
                        superseded_at: None,
                        archived_at: None,
                        jj_change_id: None,
                        persona_id: None,
                        layer: "project".into(),
                        tags: vec![],
                    },
                )?;
                Ok(MemoryMutationEffect::FactSuperseded {
                    original: FactPrecondition {
                        id: fact.id,
                        expected_version: original_version,
                    },
                    replacement: FactPrecondition {
                        id: replacement_id,
                        expected_version: replacement_version,
                    },
                })
            }
            MemoryMutation::SupersedeFactWithExisting { fact, replacement } => {
                if fact.id == replacement.id {
                    return Err(MemoryError::InvalidMutation(
                        "a fact cannot supersede itself".into(),
                    ));
                }
                Self::check_fact_precondition(state, &fact)?;
                Self::check_fact_precondition(state, &replacement)?;
                let original = state.facts.get(&fact.id).unwrap();
                let existing = state.facts.get(&replacement.id).unwrap();
                if original.status != FactStatus::Active
                    || existing.status != FactStatus::Active
                    || original.mind != existing.mind
                {
                    return Err(MemoryError::InvalidMutation(
                        "supersession requires distinct active facts in the same mind".into(),
                    ));
                }
                let original_version = Self::next_version(state)?;
                let original = state.facts.get_mut(&fact.id).unwrap();
                original.status = FactStatus::Superseded;
                original.version = original_version;
                let replacement_version = Self::next_version(state)?;
                let existing = state.facts.get_mut(&replacement.id).unwrap();
                existing.superseded_by = Some(fact.id.clone());
                existing.version = replacement_version;
                Ok(MemoryMutationEffect::FactSuperseded {
                    original: FactPrecondition {
                        id: fact.id,
                        expected_version: original_version,
                    },
                    replacement: FactPrecondition {
                        id: replacement.id,
                        expected_version: replacement_version,
                    },
                })
            }
            MemoryMutation::StoreEmbedding {
                fact,
                model_name,
                embedding,
            } => {
                Self::check_fact_precondition(state, &fact)?;
                if state
                    .facts
                    .get(&fact.id)
                    .is_some_and(|fact| fact.status == FactStatus::Pending)
                {
                    return Err(MemoryError::InvalidMutation(
                        "pending candidates cannot be indexed".into(),
                    ));
                }
                if let Some(existing) = state
                    .embeddings
                    .iter()
                    .find(|entry| entry.model_name == model_name)
                    && existing.embedding.len() != embedding.len()
                {
                    return Err(MemoryError::EmbeddingDimensionMismatch {
                        expected: existing.embedding.len() as u32,
                        got: embedding.len() as u32,
                        stored_model: model_name,
                    });
                }
                let dims = embedding.len() as u32;
                state.embeddings.retain(|entry| entry.fact_id != fact.id);
                state.embeddings.push(EmbeddingEntry {
                    fact_id: fact.id.clone(),
                    model_name: model_name.clone(),
                    embedding,
                    inserted_at: now_iso(),
                    space: None,
                    source_hash: None,
                });
                Ok(MemoryMutationEffect::EmbeddingStored {
                    fact_id: fact.id,
                    model_name,
                    dims,
                })
            }
            MemoryMutation::StoreIdentifiedEmbedding { fact, embedding } => {
                embedding.validate()?;
                Self::check_fact_precondition(state, &fact)?;
                let source = state.facts.get(&fact.id).unwrap();
                if source.status != FactStatus::Active {
                    return Err(MemoryError::InvalidMutation(
                        "embedding source must be active".into(),
                    ));
                }
                let source_hash = crate::retrieval::raw_content_hash(&source.content);
                state.embeddings.retain(|entry| entry.fact_id != fact.id);
                state.embeddings.push(EmbeddingEntry {
                    fact_id: fact.id.clone(),
                    model_name: embedding.space.model.clone(),
                    embedding: embedding.values,
                    inserted_at: now_iso(),
                    source_hash: Some(source_hash),
                    space: Some(embedding.space.clone()),
                });
                Ok(MemoryMutationEffect::EmbeddingStored {
                    fact_id: fact.id,
                    model_name: embedding.space.model,
                    dims: embedding.space.dimensions,
                })
            }
            MemoryMutation::CreateEdge { mind, request } => {
                for fact_id in [&request.source_id, &request.target_id] {
                    let fact = state
                        .facts
                        .get(fact_id)
                        .ok_or_else(|| MemoryError::FactNotFound(fact_id.clone()))?;
                    if fact.mind != mind || fact.status != FactStatus::Active {
                        return Err(MemoryError::InvalidMutation(format!(
                            "edge endpoint {fact_id} is outside active mind {mind}"
                        )));
                    }
                }
                let edge_id = gen_id();
                state.edges.push(Edge {
                    id: edge_id.clone(),
                    source_id: request.source_id,
                    target_id: request.target_id,
                    relation: request.relation,
                    description: request.description,
                    confidence: 1.0,
                    created_at: now_iso(),
                });
                Ok(MemoryMutationEffect::EdgeCreated { edge_id })
            }
            MemoryMutation::StoreEpisode { request } => {
                if let Some(formation) = &request.formation {
                    formation.validate()?;
                }
                let episode_id = gen_id();
                let timestamp = now_iso();
                state.episodes.push(Episode {
                    id: episode_id.clone(),
                    mind: request.mind,
                    date: request.date.unwrap_or_else(|| timestamp[..10].to_string()),
                    title: request.title,
                    narrative: request.narrative,
                    created_at: timestamp,
                    affected_nodes: request.affected_nodes,
                    affected_changes: request.affected_changes,
                    files_changed: request.files_changed,
                    tags: request.tags,
                    tool_calls_count: request.tool_calls_count,
                    formation: request.formation,
                    jj_change_id: None,
                });
                Ok(MemoryMutationEffect::EpisodeStored { episode_id })
            }
            MemoryMutation::CompleteFormation {
                episode_id,
                formation,
            } => {
                let episode = state
                    .episodes
                    .iter_mut()
                    .find(|episode| episode.id == episode_id)
                    .ok_or_else(|| {
                        MemoryError::InvalidMutation("formation episode not found".into())
                    })?;
                crate::formation::validate_completion(episode.formation.as_deref(), &formation)?;
                episode.narrative = formation.narrative();
                episode.formation = Some(formation);
                Ok(MemoryMutationEffect::FormationCompleted { episode_id })
            }
        }
    }

    fn import_jsonl_to_state(state: &mut State, jsonl: &str) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        for line in jsonl.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<JsonlRecord>(trimmed) {
                Ok(JsonlRecord::Fact(jf)) => {
                    validate_inference_status(&jf.status, jf.lifecycle_inference.as_deref())?;
                    if let Some(operational) = &jf.operational {
                        operational.validate()?;
                    }
                    persisted_lamport_version(jf.version)?;
                    let content_hash = jf
                        .content_hash
                        .clone()
                        .unwrap_or_else(|| hash::content_hash(&jf.content));
                    if let Some(existing) = state.facts.get(&jf.id) {
                        if jf.version > existing.version {
                            if existing.lifecycle_inference.is_some()
                                && jf.status != FactStatus::Pending
                            {
                                return Err(MemoryError::InvalidMutation(
                                    "transport cannot confirm lifecycle inference".into(),
                                ));
                            }
                            let mut updated = existing.clone();
                            updated.content = jf.content;
                            updated.section = jf.section;
                            updated.mind = jf.mind;
                            updated.status = jf.status;
                            updated.source = jf.source.filter(|source| !source.is_empty());
                            updated.content_hash = Some(content_hash);
                            updated.superseded_by = jf.supersedes;
                            updated.decay_profile = jf.decay_profile;
                            updated.persona_id = jf.persona_id;
                            updated.layer = jf.layer;
                            updated.tags = jf.tags;
                            updated.version = jf.version;
                            updated.lifecycle_inference = jf.lifecycle_inference;
                            updated.created_at = jf.created_at;
                            if let Some(operational) = jf.operational {
                                operational.apply_to(&mut updated);
                            }
                            state.version_clock = state.version_clock.max(jf.version);
                            Self::insert_fact(state, jf.id, updated)?;
                            stats.reinforced += 1;
                        } else {
                            stats.skipped += 1;
                        }
                    } else {
                        state.version_clock = state.version_clock.max(jf.version);
                        let mut fact = Fact {
                            lifecycle_inference: jf.lifecycle_inference,
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
                            superseded_by: jf.supersedes,
                            source: jf.source.filter(|source| !source.is_empty()),
                            content_hash: Some(content_hash),
                            last_accessed: None,
                            created_session: None,
                            superseded_at: None,
                            archived_at: None,
                            jj_change_id: None,
                            persona_id: jf.persona_id,
                            layer: jf.layer,
                            tags: jf.tags,
                        };
                        if let Some(operational) = jf.operational {
                            operational.apply_to(&mut fact);
                        }
                        Self::insert_fact(state, jf.id, fact)?;
                        stats.imported += 1;
                    }
                }
                Ok(JsonlRecord::Episode(ep)) => {
                    if let Some(formation) = &ep.formation {
                        formation.validate()?;
                    }
                    if let Some(existing) = state
                        .episodes
                        .iter_mut()
                        .find(|existing| existing.id == ep.id)
                    {
                        if crate::formation::completes_import(existing, &ep)? {
                            existing.formation = ep.formation;
                            existing.narrative = existing
                                .formation
                                .as_ref()
                                .expect("completed formation")
                                .narrative();
                            stats.imported += 1;
                        } else {
                            stats.skipped += 1;
                        }
                    } else {
                        state.episodes.push(ep);
                        stats.imported += 1;
                    }
                }
                Ok(JsonlRecord::Edge(edge)) => {
                    if state.edges.iter().any(|existing| existing.id == edge.id) {
                        stats.skipped += 1;
                    } else if !state.facts.contains_key(&edge.source_id) {
                        return Err(MemoryError::FactNotFound(edge.source_id));
                    } else if !state.facts.contains_key(&edge.target_id) {
                        return Err(MemoryError::FactNotFound(edge.target_id));
                    } else {
                        state.edges.push(edge);
                        stats.imported += 1;
                    }
                }
                Ok(JsonlRecord::Mind(_)) => stats.skipped += 1,
                Err(_) => stats.errors += 1,
            }
        }
        Ok(stats)
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryBackend for InMemoryBackend {
    async fn mutation_receipt(
        &self,
        operation_id: &str,
        payload_hash: &str,
    ) -> Result<Option<MemoryMutationOutcome>> {
        if operation_id.trim().is_empty() || payload_hash.trim().is_empty() {
            return Err(MemoryError::InvalidMutation(
                "operation identity and payload hash must not be empty".into(),
            ));
        }
        let state = self.state.lock().unwrap();
        let Some((recorded_hash, effect)) = state.operation_receipts.get(operation_id) else {
            return Ok(None);
        };
        if recorded_hash != payload_hash {
            return Err(MemoryError::OperationConflict(operation_id.into()));
        }
        Ok(Some(MemoryMutationOutcome {
            effect: effect.clone(),
            replayed: true,
        }))
    }

    async fn apply_mutation_bound(
        &self,
        operation_id: &str,
        payload_hash: &str,
        mutation: MemoryMutation,
    ) -> Result<MemoryMutationOutcome> {
        if operation_id.trim().is_empty() {
            return Err(MemoryError::InvalidMutation(
                "operation identity must not be empty".into(),
            ));
        }
        let _ = mutation_payload_hash(&mutation)?;
        if payload_hash.trim().is_empty() {
            return Err(MemoryError::InvalidMutation(
                "operation payload hash must not be empty".into(),
            ));
        }
        let mut state = self.state.lock().unwrap();
        if let Some((recorded_hash, effect)) = state.operation_receipts.get(operation_id) {
            if recorded_hash != payload_hash {
                return Err(MemoryError::OperationConflict(operation_id.into()));
            }
            return Ok(MemoryMutationOutcome {
                effect: effect.clone(),
                replayed: true,
            });
        }

        let mut staged = state.clone();
        let effect = Self::apply_to_state(&mut staged, mutation)?;
        staged
            .operation_receipts
            .insert(operation_id.into(), (payload_hash.into(), effect.clone()));
        *state = staged;
        Ok(MemoryMutationOutcome {
            effect,
            replayed: false,
        })
    }

    async fn store_fact(&self, req: StoreFact) -> Result<StoreResult> {
        let mut s = self.state.lock().unwrap();
        let ch = hash::content_hash(&req.content);

        // Check for dedup by content hash within same mind — find ID first, then mutate
        let existing_id = s
            .facts
            .iter()
            .find(|(_, f)| {
                f.mind == req.mind
                    && f.content_hash.as_deref() == Some(ch.as_str())
                    && f.status == FactStatus::Active
            })
            .map(|(id, _)| id.clone());

        if let Some(id) = existing_id {
            let vc = Self::next_version(&mut s)?;
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

        let version = Self::next_version(&mut s)?;
        let fact = Fact {
            lifecycle_inference: None,
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
            version,
            superseded_by: None,
            source: req.source,
            content_hash: Some(ch),
            last_accessed: None,
            created_session: None,
            superseded_at: None,
            archived_at: None,
            jj_change_id: None,
            persona_id: None,
            layer: "project".into(),
            tags: vec![],
        };
        Self::insert_fact(&mut s, fact.id.clone(), fact.clone())?;
        Ok(StoreResult {
            fact,
            action: StoreAction::Stored,
        })
    }

    async fn get_fact(&self, id: &str) -> Result<Option<Fact>> {
        let s = self.state.lock().unwrap();
        Ok(s.facts
            .get(id)
            .filter(|f| f.status == FactStatus::Active)
            .cloned())
    }

    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>> {
        let s = self.state.lock().unwrap();
        let status = filter.status.unwrap_or(FactStatus::Active);
        let mut facts: Vec<Fact> = s
            .facts
            .values()
            .filter(|f| {
                f.mind == mind
                    && f.status == status
                    && filter.section.as_ref().is_none_or(|sec| &f.section == sec)
            })
            .cloned()
            .collect();
        facts.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(facts)
    }

    async fn list_facts_page(
        &self,
        mind: &str,
        filter: FactFilter,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<FactPage> {
        let state = self.state.lock().unwrap();
        let (watermark, after) = match cursor {
            Some(cursor) => {
                let (version, id) = cursor.split_once(':').ok_or_else(|| {
                    MemoryError::InvalidMutation("invalid fact-page cursor".into())
                })?;
                let version = version
                    .parse::<u64>()
                    .map_err(|_| MemoryError::InvalidMutation("invalid fact-page cursor".into()))?;
                (version, Some(id))
            }
            None => (state.next_fact_insertion_sequence, None),
        };
        let status = filter.status.unwrap_or(FactStatus::Active);
        let matches = |fact: &&Fact| {
            fact.mind == mind
                && fact.status == status
                && state
                    .fact_insertion_sequences
                    .get(&fact.id)
                    .is_some_and(|sequence| *sequence <= watermark)
                && filter
                    .section
                    .as_ref()
                    .is_none_or(|section| &fact.section == section)
        };
        let total = state.facts.values().filter(matches).count();
        let mut matching = state
            .facts
            .values()
            .filter(matches)
            .filter(|fact| after.is_none_or(|after| fact.id.as_str() > after))
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| left.id.cmp(&right.id));
        let has_more = matching.len() > limit;
        let facts = matching
            .into_iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = has_more
            .then(|| facts.last().map(|fact| format!("{watermark}:{}", fact.id)))
            .flatten();
        Ok(FactPage {
            facts,
            next_cursor,
            total,
        })
    }

    async fn reinforce_fact(&self, id: &str) -> Result<Fact> {
        let mut s = self.state.lock().unwrap();
        if s.facts
            .get(id)
            .is_none_or(|fact| fact.status != FactStatus::Active)
        {
            return Err(MemoryError::FactNotFound(id.into()));
        }
        let vc = Self::next_version(&mut s)?;
        let ts = now_iso();
        let fact = s.facts.get_mut(id).unwrap();
        fact.reinforcement_count += 1;
        fact.last_reinforced = ts;
        fact.version = vc;
        Ok(fact.clone())
    }

    async fn dormancy_facts(&self, ids: &[&str]) -> Result<usize> {
        let mut state = self.state.lock().unwrap();
        let mut transitioned = 0;
        for id in ids {
            let is_active = state
                .facts
                .get(*id)
                .is_some_and(|fact| fact.status == FactStatus::Active);
            if is_active {
                let version = Self::next_version(&mut state)?;
                let fact = state.facts.get_mut(*id).ok_or_else(|| {
                    MemoryError::Storage(anyhow::anyhow!("active fact disappeared"))
                })?;
                fact.status = FactStatus::Dormant;
                fact.version = version;
                transitioned += 1;
            }
        }
        Ok(transitioned)
    }

    async fn archive_facts(&self, ids: &[&str]) -> Result<usize> {
        let mut s = self.state.lock().unwrap();
        let mut count = 0;
        for id in ids {
            // Check if active first, then update
            let is_active = s
                .facts
                .get(*id)
                .is_some_and(|f| f.status == FactStatus::Active);
            if is_active {
                let vc = Self::next_version(&mut s)?;
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

        if s.facts
            .get(id)
            .is_none_or(|fact| fact.status != FactStatus::Active)
        {
            return Err(MemoryError::FactNotFound(id.into()));
        }

        // Version the original before the replacement, matching SQLite.
        let original_version = Self::next_version(&mut s)?;
        let original = s
            .facts
            .get_mut(id)
            .ok_or_else(|| MemoryError::Storage(anyhow::anyhow!("validated fact disappeared")))?;
        original.status = FactStatus::Superseded;
        original.version = original_version;

        let replacement_version = Self::next_version(&mut s)?;
        let new_id = gen_id();
        let ch = hash::content_hash(&replacement.content);
        let new_fact = Fact {
            lifecycle_inference: None,
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
            version: replacement_version,
            superseded_by: Some(id.to_string()), // "I supersede old_id"
            source: replacement.source,
            content_hash: Some(ch),
            last_accessed: None,
            created_session: None,
            superseded_at: None,
            archived_at: None,
            jj_change_id: None,
            persona_id: None,
            layer: "project".into(),
            tags: vec![],
        };

        Self::insert_fact(&mut s, new_id, new_fact.clone())?;
        Ok(new_fact)
    }

    async fn superseding_fact(&self, old_id: &str) -> Result<Option<Fact>> {
        let state = self.state.lock().unwrap();
        let Some(original) = state.facts.get(old_id) else {
            return Ok(None);
        };
        if original.status != FactStatus::Superseded {
            return Ok(None);
        }
        let mut predecessor = old_id;
        let mut visited = HashSet::new();
        visited.insert(old_id);
        while let Some(replacement) = state
            .facts
            .values()
            .filter(|fact| fact.superseded_by.as_deref() == Some(predecessor))
            .max_by(|left, right| {
                left.version
                    .cmp(&right.version)
                    .then_with(|| left.id.cmp(&right.id))
            })
        {
            if !visited.insert(replacement.id.as_str()) {
                return Err(MemoryError::Storage(anyhow::anyhow!(
                    "supersession cycle detected"
                )));
            }
            if replacement.status == FactStatus::Active {
                return Ok(Some(replacement.clone()));
            }
            if replacement.status != FactStatus::Superseded {
                break;
            }
            predecessor = &replacement.id;
        }

        let Some(source) = original
            .source
            .as_deref()
            .filter(|source| source.starts_with("codex-vault:"))
        else {
            return Ok(None);
        };
        let Some(latest) = state
            .facts
            .values()
            .filter(|fact| fact.source.as_deref() == Some(source))
            .max_by(|left, right| {
                left.version
                    .cmp(&right.version)
                    .then_with(|| left.id.cmp(&right.id))
            })
        else {
            return Ok(None);
        };
        if latest.status == FactStatus::Active {
            return Ok(Some(latest.clone()));
        }
        if latest.status != FactStatus::Superseded {
            return Ok(None);
        }
        Ok(state
            .facts
            .values()
            .filter(|fact| {
                fact.status == FactStatus::Active
                    && fact.superseded_by.as_deref() == Some(latest.id.as_str())
            })
            .max_by(|left, right| {
                left.version
                    .cmp(&right.version)
                    .then_with(|| left.id.cmp(&right.id))
            })
            .cloned())
    }

    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>> {
        self.fts_search_filtered(mind, query, k, &SearchFilter::default())
            .await
    }

    async fn fts_search_filtered(
        &self,
        mind: &str,
        query: &str,
        k: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<ScoredFact>> {
        let s = self.state.lock().unwrap();
        let query_lower = query.replace('"', " ").to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();

        let mut results: Vec<ScoredFact> = s
            .facts
            .values()
            .filter(|f| f.mind == mind && filter.matches(f))
            .filter_map(|f| {
                let content_lower = f.content.to_lowercase();
                let matches = terms.iter().filter(|t| content_lower.contains(**t)).count();
                if matches == 0 {
                    return None;
                }
                let relevance = matches as f64 / terms.len().max(1) as f64;
                let score = filter.score(relevance, f)?;
                Some(ScoredFact {
                    fact: f.clone(),
                    similarity: relevance,
                    score,
                    scores: RetrievalScores {
                        lexical: Some(relevance),
                        ..Default::default()
                    },
                    graph_evidence: vec![],
                })
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.fact.id.cmp(&b.fact.id))
        });
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
        let _ = (mind, embedding, k, min_similarity);
        Err(MemoryError::EmbeddingIdentityRequired)
    }

    async fn vector_search_cancellable(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<ScoredFact>> {
        self.vector_search_filtered_cancellable(
            mind,
            embedding,
            k,
            min_similarity,
            &SearchFilter::default(),
            cancelled,
        )
        .await
    }

    async fn vector_search_filtered_cancellable(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
        filter: &SearchFilter,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<ScoredFact>> {
        let _ = (mind, embedding, k, min_similarity, filter);
        if cancelled() {
            return Err(MemoryError::Cancelled);
        }
        Err(MemoryError::EmbeddingIdentityRequired)
    }

    async fn store_embedding(
        &self,
        fact_id: &str,
        model_name: &str,
        embedding: &[f32],
    ) -> Result<()> {
        validate_embedding(embedding)?;
        let mut s = self.state.lock().unwrap();
        if s.facts
            .get(fact_id)
            .is_none_or(|fact| fact.status == FactStatus::Pending)
        {
            return Err(MemoryError::FactNotFound(fact_id.into()));
        }
        if let Some(existing) = s
            .embeddings
            .iter()
            .find(|entry| entry.model_name == model_name)
            && existing.embedding.len() != embedding.len()
        {
            return Err(MemoryError::EmbeddingDimensionMismatch {
                expected: existing.embedding.len() as u32,
                got: embedding.len() as u32,
                stored_model: model_name.into(),
            });
        }
        // Remove existing embedding for this fact
        s.embeddings.retain(|e| e.fact_id != fact_id);
        s.embeddings.push(EmbeddingEntry {
            fact_id: fact_id.into(),
            model_name: model_name.into(),
            embedding: embedding.to_vec(),
            inserted_at: now_iso(),
            space: None,
            source_hash: None,
        });
        Ok(())
    }

    async fn embedding_metadata(&self, mind: &str) -> Result<Option<EmbeddingMetadata>> {
        let s = self.state.lock().unwrap();
        let entry = s
            .embeddings
            .iter()
            .find(|e| s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind));
        Ok(entry.map(|e| EmbeddingMetadata {
            model_name: e.model_name.clone(),
            dims: e.embedding.len() as u32,
            inserted_at: e.inserted_at.clone(),
        }))
    }

    async fn search_identified(
        &self,
        mind: &str,
        query: &IdentifiedEmbedding,
        k: usize,
        minimum: f32,
        filter: &SearchFilter,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<VectorSearchReport> {
        let mut top = crate::retrieval::VectorAccumulator::new(query, k, minimum)?;
        if cancelled() {
            return Err(MemoryError::Cancelled);
        }
        if k == 0 {
            return Ok(top.finish());
        }
        let state = self.state.lock().unwrap();
        for entry in &state.embeddings {
            if cancelled() {
                return Err(MemoryError::Cancelled);
            }
            let Some(fact) = state
                .facts
                .get(&entry.fact_id)
                .filter(|fact| fact.mind == mind && filter.matches(fact))
            else {
                continue;
            };
            if !top.eligible(crate::retrieval::index_state(
                entry.space.as_ref(),
                entry.source_hash.as_deref(),
                &fact.content,
                &query.space,
            )) {
                continue;
            }
            IdentifiedEmbedding {
                space: query.space.clone(),
                values: entry.embedding.clone(),
            }
            .validate()?;
            let similarity = vectors::cosine_similarity(&entry.embedding, &query.values);
            if similarity >= minimum {
                top.push(fact.clone(), similarity as f64, filter);
            }
        }
        Ok(top.finish())
    }

    async fn embedding_index_state(
        &self,
        id: &str,
        space: &EmbeddingSpace,
    ) -> Result<EmbeddingIndexState> {
        space.validate()?;
        let state = self.state.lock().unwrap();
        let fact = state
            .facts
            .get(id)
            .ok_or_else(|| MemoryError::FactNotFound(id.into()))?;
        let Some(entry) = state.embeddings.iter().find(|entry| entry.fact_id == id) else {
            return Ok(EmbeddingIndexState::Missing);
        };
        Ok(crate::retrieval::index_state(
            entry.space.as_ref(),
            entry.source_hash.as_deref(),
            &fact.content,
            space,
        ))
    }

    async fn get_fact_filtered(
        &self,
        mind: &str,
        id: &str,
        filter: &SearchFilter,
    ) -> Result<Option<Fact>> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .facts
            .get(id)
            .filter(|fact| fact.mind == mind && filter.matches(fact))
            .cloned())
    }

    async fn get_edges_filtered(
        &self,
        mind: &str,
        id: &str,
        filter: &SearchFilter,
        limit: usize,
    ) -> Result<Vec<Edge>> {
        let state = self.state.lock().unwrap();
        let mut edges = state
            .edges
            .iter()
            .filter(|edge| edge.source_id == id || edge.target_id == id)
            .filter(|edge| {
                [&edge.source_id, &edge.target_id].iter().all(|id| {
                    state
                        .facts
                        .get(*id)
                        .is_some_and(|fact| fact.mind == mind && filter.matches(fact))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        edges.sort_by(|a, b| {
            b.confidence
                .total_cmp(&a.confidence)
                .then_with(|| a.id.cmp(&b.id))
        });
        edges.truncate(limit.min(1024));
        Ok(edges)
    }

    async fn create_edge(&self, req: CreateEdge) -> Result<Edge> {
        let mut s = self.state.lock().unwrap();
        let source = s
            .facts
            .get(&req.source_id)
            .ok_or_else(|| MemoryError::FactNotFound(req.source_id.clone()))?;
        let target = s
            .facts
            .get(&req.target_id)
            .ok_or_else(|| MemoryError::FactNotFound(req.target_id.clone()))?;
        if source.mind != target.mind
            || source.status != FactStatus::Active
            || target.status != FactStatus::Active
        {
            return Err(MemoryError::InvalidMutation(
                "edge endpoints must be active facts in the same mind".into(),
            ));
        }
        let edge = Edge {
            id: gen_id(),
            source_id: req.source_id,
            target_id: req.target_id,
            relation: req.relation,
            description: req.description,
            confidence: 1.0,
            created_at: now_iso(),
        };
        s.edges.push(edge.clone());
        Ok(edge)
    }

    async fn get_edges(&self, mind: &str, fact_id: &str) -> Result<Vec<Edge>> {
        let s = self.state.lock().unwrap();
        if s.facts
            .get(fact_id)
            .is_none_or(|fact| fact.mind != mind || fact.status != FactStatus::Active)
        {
            return Ok(Vec::new());
        }
        Ok(s.edges
            .iter()
            .filter(|edge| edge.source_id == fact_id || edge.target_id == fact_id)
            .filter(|edge| {
                [&edge.source_id, &edge.target_id].into_iter().all(|id| {
                    s.facts
                        .get(id)
                        .is_some_and(|fact| fact.mind == mind && fact.status == FactStatus::Active)
                })
            })
            .cloned()
            .collect())
    }

    async fn store_episode(&self, req: StoreEpisode) -> Result<Episode> {
        if let Some(formation) = &req.formation {
            formation.validate()?;
        }
        let mut s = self.state.lock().unwrap();
        let episode = Episode {
            id: gen_id(),
            mind: req.mind,
            date: req.date.unwrap_or_else(|| now_iso()[..10].to_string()),
            title: req.title,
            narrative: req.narrative,
            created_at: now_iso(),
            affected_nodes: req.affected_nodes,
            affected_changes: req.affected_changes,
            files_changed: req.files_changed,
            tags: req.tags,
            tool_calls_count: req.tool_calls_count,
            formation: req.formation,
            jj_change_id: None,
        };
        s.episodes.push(episode.clone());
        Ok(episode)
    }

    async fn list_episodes(&self, mind: &str, k: usize) -> Result<Vec<Episode>> {
        let s = self.state.lock().unwrap();
        let mut eps: Vec<Episode> = s
            .episodes
            .iter()
            .filter(|e| e.mind == mind)
            .cloned()
            .collect();
        eps.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then_with(|| b.created_at.cmp(&a.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        eps.truncate(k);
        Ok(eps)
    }

    async fn search_episodes(&self, mind: &str, query: &str, k: usize) -> Result<Vec<Episode>> {
        let s = self.state.lock().unwrap();
        let query_lower = query.replace('"', " ").to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();
        if terms.is_empty() || k == 0 {
            return Ok(Vec::new());
        }
        let mut results: Vec<(usize, &Episode)> = s
            .episodes
            .iter()
            .filter(|episode| episode.mind == mind)
            .filter_map(|episode| {
                let text = format!("{} {}", episode.title, episode.narrative).to_lowercase();
                let hits = terms.iter().filter(|term| text.contains(**term)).count();
                (hits > 0).then_some((hits, episode))
            })
            .collect();
        results.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right.date.cmp(&left.date))
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        results.truncate(k);
        Ok(results
            .into_iter()
            .map(|(_, episode)| episode.clone())
            .collect())
    }

    async fn export_jsonl(&self, mind: &str) -> Result<String> {
        let s = self.state.lock().unwrap();
        let mut lines = Vec::new();

        // Facts (sorted by id for determinism)
        let mut facts: Vec<&Fact> = s.facts.values().filter(|f| f.mind == mind).collect();
        facts.sort_by(|a, b| a.id.cmp(&b.id));
        for fact in facts {
            FactOperationalState::from(fact).validate()?;
            let record = JsonlRecord::Fact(JsonlFact {
                lifecycle_inference: fact.lifecycle_inference.clone(),
                operational: Some(Box::new(FactOperationalState::from(fact))),
                id: fact.id.clone(),
                mind: fact.mind.clone(),
                content: fact.content.clone(),
                section: fact.section.clone(),
                status: fact.status.clone(),
                created_at: fact.created_at.clone(),
                source: fact.source.clone(),
                content_hash: fact.content_hash.clone(),
                supersedes: fact.superseded_by.clone(),
                version: fact.version,
                decay_profile: fact.decay_profile.clone(),
                persona_id: fact.persona_id.clone(),
                layer: fact.layer.clone(),
                tags: fact.tags.clone(),
            });
            lines.push(serde_json::to_string(&record).unwrap());
        }

        // Edges
        let mut edges: Vec<&Edge> = s
            .edges
            .iter()
            .filter(|e| s.facts.get(&e.source_id).is_some_and(|f| f.mind == mind))
            .collect();
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        for edge in edges {
            lines.push(serde_json::to_string(&JsonlRecord::Edge(edge.clone())).unwrap());
        }

        // Episodes
        let mut eps: Vec<&Episode> = s.episodes.iter().filter(|e| e.mind == mind).collect();
        eps.sort_by(|a, b| a.id.cmp(&b.id));
        for ep in eps {
            lines.push(serde_json::to_string(&JsonlRecord::Episode(ep.clone())).unwrap());
        }

        Ok(lines.join("\n"))
    }

    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats> {
        let mut state = self.state.lock().unwrap();
        let mut staged = state.clone();
        let stats = Self::import_jsonl_to_state(&mut staged, jsonl)?;
        *state = staged;
        Ok(stats)
    }

    async fn stats(&self, mind: &str) -> Result<MemoryStats> {
        let s = self.state.lock().unwrap();
        let mind_facts: Vec<&Fact> = s.facts.values().filter(|f| f.mind == mind).collect();
        let active = mind_facts
            .iter()
            .filter(|f| f.status == FactStatus::Active)
            .count();
        let archived = mind_facts
            .iter()
            .filter(|f| f.status == FactStatus::Archived)
            .count();
        let superseded = mind_facts
            .iter()
            .filter(|f| f.status == FactStatus::Superseded)
            .count();
        let with_vectors = s
            .embeddings
            .iter()
            .filter(|e| s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind))
            .count();
        let meta = s
            .embeddings
            .iter()
            .find(|e| s.facts.get(&e.fact_id).is_some_and(|f| f.mind == mind));
        let episodes = s.episodes.iter().filter(|e| e.mind == mind).count();
        let edges = s
            .edges
            .iter()
            .filter(|e| s.facts.get(&e.source_id).is_some_and(|f| f.mind == mind))
            .count();
        let version_hwm = s
            .facts
            .values()
            .filter(|f| f.mind == mind)
            .map(|f| f.version)
            .max()
            .unwrap_or(0);

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

    async fn inventory_stats(&self) -> Result<MemoryInventoryStats> {
        let state = self.state.lock().unwrap();
        let active = state
            .facts
            .values()
            .filter(|fact| fact.status == FactStatus::Active)
            .collect::<Vec<_>>();
        let mut persona_counts = std::collections::BTreeMap::<String, usize>::new();
        for fact in active.iter().filter(|fact| fact.layer == "persona") {
            *persona_counts.entry(fact.mind.clone()).or_default() += 1;
        }
        Ok(MemoryInventoryStats {
            total_facts: state.facts.len(),
            active_facts: active.len(),
            project_facts: active.iter().filter(|fact| fact.layer == "project").count(),
            persona_facts: active.iter().filter(|fact| fact.layer == "persona").count(),
            working_facts: active.iter().filter(|fact| fact.layer == "working").count(),
            episodes: state.episodes.len(),
            edges: state.edges.len(),
            active_persona_mind: persona_counts
                .into_iter()
                .max_by(|(left_mind, left_count), (right_mind, right_count)| {
                    left_count
                        .cmp(right_count)
                        .then_with(|| right_mind.cmp(left_mind))
                })
                .map(|(mind, _)| mind),
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

    #[tokio::test]
    async fn keyset_pages_reach_inventories_larger_than_ten_thousand() {
        let backend = InMemoryBackend::new();
        for index in 0..10_025 {
            backend
                .store_fact(StoreFact {
                    mind: "large-page".into(),
                    content: format!("fact {index}"),
                    section: Section::Architecture,
                    decay_profile: DecayProfileName::Standard,
                    source: Some("test".into()),
                })
                .await
                .unwrap();
        }
        let mut cursor = None;
        let mut ids = std::collections::HashSet::new();
        let mut inserted_after_watermark = false;
        loop {
            let page = backend
                .list_facts_page("large-page", FactFilter::default(), 257, cursor.as_deref())
                .await
                .unwrap();
            assert_eq!(page.total, 10_025);
            ids.extend(page.facts.into_iter().map(|fact| fact.id));
            cursor = page.next_cursor;
            if !inserted_after_watermark {
                backend
                    .store_fact(StoreFact {
                        mind: "large-page".into(),
                        content: "inserted after first-page watermark".into(),
                        section: Section::Architecture,
                        decay_profile: DecayProfileName::Standard,
                        source: Some("test".into()),
                    })
                    .await
                    .unwrap();
                inserted_after_watermark = true;
            }
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids.len(), 10_025);
    }
}
