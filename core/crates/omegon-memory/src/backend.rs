//! MemoryBackend trait — the storage abstraction.
//!
//! Implementations:
//! - `SqliteBackend` (production) — rusqlite + WAL + FTS5 + vector BLOBs
//! - `InMemoryBackend` (tests) — HashMap-based, no persistence
//!
//! The trait surface mirrors api-types.ts endpoints as direct Rust calls.
//! Each method maps 1:1 to an HTTP endpoint in the Omega daemon model,
//! but is called directly when linked into the omegon binary.

use crate::types::*;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Errors specific to the memory backend.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory operation cancelled")]
    Cancelled,

    #[error("Fact not found: {0}")]
    FactNotFound(String),

    #[error(
        "Embedding dimension mismatch: stored model '{stored_model}' has {expected} dims, query has {got}"
    )]
    EmbeddingDimensionMismatch {
        expected: u32,
        got: u32,
        stored_model: String,
    },

    #[error("No embeddings available — run embedding indexer first")]
    NoEmbeddings,

    #[error("Vector comparison requires an explicit embedding-space identity")]
    EmbeddingIdentityRequired,

    #[error("Memory operation identity conflicts with a different payload: {0}")]
    OperationConflict(String),

    #[error("Fact version conflict for {id}: expected {expected}, found {actual}")]
    FactVersionConflict {
        id: String,
        expected: u64,
        actual: u64,
    },

    #[error("Invalid memory mutation: {0}")]
    InvalidMutation(String),

    #[error("Storage error: {0}")]
    Storage(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, MemoryError>;

pub(crate) fn mutation_payload_hash(mutation: &MemoryMutation) -> Result<String> {
    validate_mutation(mutation)?;
    let payload = serde_json::to_vec(mutation)
        .map_err(|error| MemoryError::InvalidMutation(error.to_string()))?;
    Ok(hex::encode(Sha256::digest(payload)))
}

fn validate_mutation(mutation: &MemoryMutation) -> Result<()> {
    if let MemoryMutation::StoreApplicableFact { constraints, .. }
    | MemoryMutation::SetFactApplicability { constraints, .. } = mutation
    {
        constraints.validate()?;
    }
    if let MemoryMutation::StoreLifecycleConclusion {
        request, source, ..
    } = mutation
    {
        source.validate(&request.content, &request.section)?;
    }
    if let MemoryMutation::StoreLifecycleInference { request, inference } = mutation {
        if inference.confirmation.is_some() {
            return Err(MemoryError::InvalidMutation(
                "inference ingestion cannot supply confirmation".into(),
            ));
        }
        inference.validate()?;
        if request.content.trim().is_empty() || request.content.len() > 65_536 {
            return Err(MemoryError::InvalidMutation(
                "invalid lifecycle inference content".into(),
            ));
        }
    }
    if let MemoryMutation::StoreEmbedding { embedding, .. } = mutation {
        validate_embedding(embedding)?;
    }
    if let MemoryMutation::StoreIdentifiedEmbedding { embedding, .. } = mutation {
        embedding.validate()?;
    }
    Ok(())
}

pub(crate) fn jsonl_import_effect(stats: ImportStats) -> MemoryMutationEffect {
    MemoryMutationEffect::JsonlImported {
        imported: stats.imported,
        reinforced: stats.reinforced,
        skipped: stats.skipped,
        errors: stats.errors,
    }
}

pub(crate) fn validate_embedding(embedding: &[f32]) -> Result<()> {
    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(MemoryError::InvalidMutation(
            "embedding values must be finite".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_unique_fact_preconditions(facts: &[FactPrecondition]) -> Result<()> {
    let mut ids = HashSet::with_capacity(facts.len());
    if let Some(duplicate) = facts.iter().find(|fact| !ids.insert(fact.id.as_str())) {
        return Err(MemoryError::InvalidMutation(format!(
            "duplicate fact precondition: {}",
            duplicate.id
        )));
    }
    Ok(())
}

pub(crate) fn persisted_lamport_version(version: u64) -> Result<i64> {
    i64::try_from(version).map_err(|_| {
        MemoryError::InvalidMutation(format!(
            "Lamport version {version} exceeds the persisted i64 domain"
        ))
    })
}

/// Storage abstraction for the memory system.
///
/// All methods take `&self` — implementations must handle interior mutability
/// (e.g., `Mutex<rusqlite::Connection>` for sqlite).
///
/// Methods are async to allow both sync sqlite (wrapped in `spawn_blocking`)
/// and potential future async backends.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Opaque, connection-instance-bound stamp covering all potentially visible writes.
    /// Backends without reliable invalidation leave caching disabled.
    async fn selection_revision(
        &self,
    ) -> Result<Option<crate::selection_cache::SelectionRevision>> {
        Ok(None)
    }

    /// Earliest applicability/floor transitions. Called on cache misses, not hits.
    async fn selection_time_bounds(
        &self,
        _mind: &str,
        _query_at: chrono::DateTime<chrono::Utc>,
        _wall_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<crate::selection_cache::SelectionTimeBounds>> {
        Ok(None)
    }
    /// Exact mind-scoped lookup across all statuses, without reinforcement.
    async fn get_fact_record(&self, _mind: &str, _id: &str) -> Result<Option<Fact>> {
        Err(MemoryError::InvalidMutation(
            "status-neutral lookup unsupported".into(),
        ))
    }
    async fn get_pending_fact(&self, _mind: &str, _id: &str) -> Result<Option<Fact>> {
        Err(MemoryError::InvalidMutation(
            "pending lookup unsupported".into(),
        ))
    }
    /// Apply a payload-bound mutation exactly once. Reusing `operation_id` with
    /// the same payload returns the recorded effect; a different payload fails.
    async fn apply_mutation(
        &self,
        operation_id: &str,
        mutation: MemoryMutation,
    ) -> Result<MemoryMutationOutcome> {
        let payload_hash = mutation_payload_hash(&mutation)?;
        self.apply_mutation_bound(operation_id, &payload_hash, mutation)
            .await
    }

    /// Return a recorded outcome before callers resolve current entity versions.
    /// A reused operation identity with a different payload is a conflict.
    async fn mutation_receipt(
        &self,
        operation_id: &str,
        payload_hash: &str,
    ) -> Result<Option<MemoryMutationOutcome>>;

    /// Atomically apply a mutation while binding its receipt to a caller-owned
    /// canonical payload. Entity preconditions remain part of `mutation`.
    async fn apply_mutation_bound(
        &self,
        operation_id: &str,
        payload_hash: &str,
        mutation: MemoryMutation,
    ) -> Result<MemoryMutationOutcome>;

    // ── Facts ────────────────────────────────────────────────────────────

    /// Store a new fact. Handles deduplication (content hash) and
    /// reinforcement of existing facts automatically.
    async fn store_fact(&self, req: StoreFact) -> Result<StoreResult>;

    /// Get a single fact by ID. Returns None if not found or archived.
    async fn get_fact(&self, id: &str) -> Result<Option<Fact>>;

    /// List facts matching a filter. Returns active facts by default.
    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>>;

    /// Return a deterministic, bounded keyset page. The opaque cursor binds a
    /// first-page insertion watermark and the last returned fact ID.
    async fn list_facts_page(
        &self,
        mind: &str,
        filter: FactFilter,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<FactPage>;

    /// Reinforce a fact — increment reinforcement_count, reset decay timer.
    async fn reinforce_fact(&self, id: &str) -> Result<Fact>;

    /// Transition active facts to dormant. Dormant facts remain stored but are
    /// excluded from ambient retrieval.
    async fn dormancy_facts(&self, ids: &[&str]) -> Result<usize>;

    /// Archive one or more facts. Soft-delete — still retrievable via filter.
    async fn archive_facts(&self, ids: &[&str]) -> Result<usize>;

    /// Supersede a fact — archive the original, store a replacement.
    /// Returns the new replacement fact.
    async fn supersede_fact(&self, id: &str, replacement: StoreFact) -> Result<Fact>;

    /// Resolve an inactive fact ID to its active replacement, following a
    /// supersession chain. Returns None for active, archived, or unknown IDs.
    async fn superseding_fact(&self, old_id: &str) -> Result<Option<Fact>>;

    // ── Search ───────────────────────────────────────────────────────────

    /// Full-text search via FTS5. Returns facts ranked by FTS5 relevance × decay confidence.
    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>>;

    /// Apply population and section eligibility before candidate limits.
    /// Legacy external backends reject unsupported filters rather than leaking results.
    async fn fts_search_filtered(
        &self,
        mind: &str,
        query: &str,
        k: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<ScoredFact>> {
        if filter != &SearchFilter::default() {
            return Err(MemoryError::InvalidMutation(
                "backend does not support filtered search".into(),
            ));
        }
        self.fts_search(mind, query, k).await
    }

    /// Legacy unidentified query. Native backends return EmbeddingIdentityRequired;
    /// use search_identified to compare vectors under an explicit space contract.
    async fn vector_search(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
    ) -> Result<Vec<ScoredFact>>;

    /// Cooperative vector scan used by managed callers. External backends keep
    /// compatibility through the default whole-call cancellation checks.
    async fn vector_search_cancellable(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<ScoredFact>> {
        if cancelled() {
            return Err(MemoryError::Cancelled);
        }
        let results = self
            .vector_search(mind, embedding, k, min_similarity)
            .await?;
        if cancelled() {
            return Err(MemoryError::Cancelled);
        }
        Ok(results)
    }

    /// Filter vector candidates before limits, with cooperative cancellation.
    #[allow(clippy::too_many_arguments)]
    async fn vector_search_filtered_cancellable(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
        filter: &SearchFilter,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<Vec<ScoredFact>> {
        if filter != &SearchFilter::default() {
            return Err(MemoryError::InvalidMutation(
                "backend does not support filtered vectors".into(),
            ));
        }
        self.vector_search_cancellable(mind, embedding, k, min_similarity, cancelled)
            .await
    }

    /// Compare only compatible, content-current vectors and report skipped index states.
    async fn search_identified(
        &self,
        mind: &str,
        query: &IdentifiedEmbedding,
        k: usize,
        min_similarity: f32,
        filter: &SearchFilter,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<VectorSearchReport> {
        let _ = (mind, query, k, min_similarity, filter, cancelled);
        Err(MemoryError::EmbeddingIdentityRequired)
    }

    async fn embedding_index_state(
        &self,
        fact_id: &str,
        space: &EmbeddingSpace,
    ) -> Result<EmbeddingIndexState> {
        let _ = (fact_id, space);
        Err(MemoryError::EmbeddingIdentityRequired)
    }

    async fn get_fact_filtered(
        &self,
        mind: &str,
        id: &str,
        filter: &SearchFilter,
    ) -> Result<Option<Fact>> {
        let filter = filter.resolved()?;
        if filter.intent == SearchIntent::Historical {
            return Err(MemoryError::InvalidMutation(
                "historical lookup unsupported".into(),
            ));
        }
        Ok(self
            .get_fact(id)
            .await?
            .filter(|fact| fact.mind == mind && filter.matches(fact)))
    }

    async fn get_edges_filtered(
        &self,
        mind: &str,
        id: &str,
        filter: &SearchFilter,
        limit: usize,
    ) -> Result<Vec<Edge>> {
        let filter = filter.resolved()?;
        if filter.intent == SearchIntent::Historical {
            return Err(MemoryError::InvalidMutation(
                "filtered edges unsupported".into(),
            ));
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        for edge in self.get_edges(mind, id).await? {
            let mut eligible = true;
            for endpoint in [&edge.source_id, &edge.target_id] {
                if self
                    .get_fact_filtered(mind, endpoint, &filter)
                    .await?
                    .is_none()
                {
                    eligible = false;
                }
            }
            if eligible {
                results.push(edge);
                if results.len() >= limit.min(1024) {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Store an embedding vector for a fact. Registers the model in embedding_metadata
    /// if not already present.
    async fn store_embedding(
        &self,
        fact_id: &str,
        model_name: &str,
        embedding: &[f32],
    ) -> Result<()>;

    /// Get the embedding model metadata for a mind, if any vectors exist.
    async fn embedding_metadata(&self, mind: &str) -> Result<Option<EmbeddingMetadata>>;

    // ── Edges ────────────────────────────────────────────────────────────

    /// Create a directional relationship between two facts.
    async fn create_edge(&self, req: CreateEdge) -> Result<Edge>;

    /// Get all edges involving a fact (as source or target) within a mind.
    async fn get_edges(&self, mind: &str, fact_id: &str) -> Result<Vec<Edge>>;

    // ── Episodes ─────────────────────────────────────────────────────────

    /// Store a session episode narrative.
    async fn store_episode(&self, req: StoreEpisode) -> Result<Episode>;

    /// List the most recent episodes for a mind.
    async fn list_episodes(&self, mind: &str, k: usize) -> Result<Vec<Episode>>;

    /// Search episodes by narrative similarity (FTS5 or embedding).
    async fn search_episodes(&self, mind: &str, query: &str, k: usize) -> Result<Vec<Episode>>;

    // ── JSONL sync ───────────────────────────────────────────────────────

    /// Export all records for a mind as NDJSON.
    /// Deterministic output: sorted by type, then by ID within type.
    async fn export_jsonl(&self, mind: &str) -> Result<String>;

    /// Import records from NDJSON. Uses Lamport version for conflict resolution.
    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats>;

    // ── Stats ────────────────────────────────────────────────────────────

    /// Get summary statistics for a mind's memory store.
    async fn stats(&self, mind: &str) -> Result<MemoryStats>;

    /// Get store-wide layer counts for host status projections.
    async fn inventory_stats(&self) -> Result<MemoryInventoryStats>;
}

// ─── Context Rendering ──────────────────────────────────────────────────────

/// Renders memory facts into a context block for injection.
///
/// Separated from `MemoryBackend` because rendering is a consumer concern,
/// not a storage concern. Different consumers (LLM prompt, web UI, headless
/// debug output) may want different formats from the same backend.
///
/// The default implementation (`MarkdownRenderer`) produces the markdown
/// block used for LLM system prompt injection.
pub trait ContextRenderer: Send + Sync {
    /// Opt in only when this identity captures every rendering-policy dependency.
    fn memory_cache_identity(&self) -> Option<&str> {
        None
    }
    /// Format already selected evidence. Token packing counts this complete output.
    fn render_memory_blocks(
        &self,
        blocks: &[crate::renderer::MemoryFactBlock<'_>],
        episodes: &[&Episode],
        context: &ApplicabilityContext,
    ) -> String {
        let facts = blocks
            .iter()
            .filter(|block| !block.pinned)
            .map(|block| block.fact.clone())
            .collect::<Vec<_>>();
        let pins = blocks
            .iter()
            .filter(|block| block.pinned)
            .map(|block| block.fact.clone())
            .collect::<Vec<_>>();
        let episodes = episodes
            .iter()
            .map(|episode| (*episode).clone())
            .collect::<Vec<_>>();
        self.render_context_scoped(&facts, &episodes, &pins, usize::MAX, context)
            .markdown
    }
    fn render_context_scoped(
        &self,
        facts: &[Fact],
        episodes: &[Episode],
        working_memory: &[Fact],
        max_chars: usize,
        context: &ApplicabilityContext,
    ) -> RenderedContext {
        let eligible = |fact: &&Fact| {
            fact.status == FactStatus::Active
                && fact.applicability.as_ref().is_none_or(|record| {
                    record.constraints.assess(context) != ApplicabilityStatus::Inapplicable
                })
        };
        let facts = facts.iter().filter(eligible).cloned().collect::<Vec<_>>();
        let working = working_memory
            .iter()
            .filter(eligible)
            .cloned()
            .collect::<Vec<_>>();
        self.render_context(&facts, episodes, &working, max_chars)
    }
    /// Render a context block from the given backend.
    /// Selects facts by priority tier, respects character budget, and
    /// includes episode summaries.
    fn render_context(
        &self,
        facts: &[Fact],
        episodes: &[Episode],
        working_memory: &[Fact],
        max_chars: usize,
    ) -> RenderedContext;
}

/// Summary statistics for a memory store.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MemoryStats {
    pub total_facts: usize,
    pub active_facts: usize,
    pub archived_facts: usize,
    pub superseded_facts: usize,
    pub facts_with_vectors: usize,
    pub embedding_model: Option<String>,
    pub embedding_dims: Option<u32>,
    pub episodes: usize,
    pub edges: usize,
    pub version_hwm: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemoryInventoryStats {
    pub total_facts: usize,
    pub active_facts: usize,
    pub project_facts: usize,
    pub persona_facts: usize,
    pub working_facts: usize,
    pub episodes: usize,
    pub edges: usize,
    pub active_persona_mind: Option<String>,
}
