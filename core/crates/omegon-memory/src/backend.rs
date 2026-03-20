//! MemoryBackend trait — the storage abstraction.
//!
//! Implementations:
//! - `SqliteBackend` (production) — rusqlite + WAL + FTS5 + vector BLOBs
//! - `InMemoryBackend` (tests) — HashMap-based, no persistence
//!
//! The trait surface mirrors api-types.ts endpoints as direct Rust calls.
//! Each method maps 1:1 to an HTTP endpoint in the Omega daemon model,
//! but is called directly when linked into the omegon-agent binary.

use async_trait::async_trait;
use crate::types::*;

/// Errors specific to the memory backend.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Fact not found: {0}")]
    FactNotFound(String),

    #[error("Embedding dimension mismatch: stored model '{stored_model}' has {expected} dims, query has {got}")]
    EmbeddingDimensionMismatch {
        expected: u32,
        got: u32,
        stored_model: String,
    },

    #[error("No embeddings available — run embedding indexer first")]
    NoEmbeddings,

    #[error("Storage error: {0}")]
    Storage(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, MemoryError>;

/// Storage abstraction for the memory system.
///
/// All methods take `&self` — implementations must handle interior mutability
/// (e.g., `Mutex<rusqlite::Connection>` for sqlite).
///
/// Methods are async to allow both sync sqlite (wrapped in `spawn_blocking`)
/// and potential future async backends.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    // ── Facts ────────────────────────────────────────────────────────────

    /// Store a new fact. Handles deduplication (content hash) and
    /// reinforcement of existing facts automatically.
    async fn store_fact(&self, req: StoreFact) -> Result<StoreResult>;

    /// Get a single fact by ID. Returns None if not found or archived.
    async fn get_fact(&self, id: &str) -> Result<Option<Fact>>;

    /// List facts matching a filter. Returns active facts by default.
    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>>;

    /// Reinforce a fact — increment reinforcement_count, reset decay timer.
    async fn reinforce_fact(&self, id: &str) -> Result<Fact>;

    /// Archive one or more facts. Soft-delete — still retrievable via filter.
    async fn archive_facts(&self, ids: &[&str]) -> Result<usize>;

    /// Supersede a fact — archive the original, store a replacement.
    /// Returns the new replacement fact.
    async fn supersede_fact(&self, id: &str, replacement: StoreFact) -> Result<Fact>;

    // ── Search ───────────────────────────────────────────────────────────

    /// Full-text search via FTS5. Returns facts ranked by FTS5 relevance × decay confidence.
    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>>;

    /// Vector similarity search. Returns facts ranked by cosine similarity × decay confidence.
    /// Returns `Err(EmbeddingDimensionMismatch)` if query dims don't match stored model.
    /// Returns `Err(NoEmbeddings)` if no vectors exist for this mind.
    async fn vector_search(
        &self,
        mind: &str,
        embedding: &[f32],
        k: usize,
        min_similarity: f32,
    ) -> Result<Vec<ScoredFact>>;

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
    async fn search_episodes(
        &self,
        mind: &str,
        query: &str,
        k: usize,
    ) -> Result<Vec<Episode>>;

    // ── JSONL sync ───────────────────────────────────────────────────────

    /// Export all records for a mind as NDJSON.
    /// Deterministic output: sorted by type, then by ID within type.
    async fn export_jsonl(&self, mind: &str) -> Result<String>;

    /// Import records from NDJSON. Uses Lamport version for conflict resolution.
    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats>;

    // ── Stats ────────────────────────────────────────────────────────────

    /// Get summary statistics for a mind's memory store.
    async fn stats(&self, mind: &str) -> Result<MemoryStats>;
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
#[derive(Debug, Clone, Default)]
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
