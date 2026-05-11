+++
id = "a146341f-e462-4b60-9bbd-84b5a5749d1d"
kind = "design_node"
title = "Semantic memory with local embeddings — vector retrieval for omegon-memory"
status = "decided"
tags = ["memory", "embeddings", "semantic-search", "privacy", "ort"]
aliases = ["semantic-memory-embeddings"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = ["model size vs quality tradeoff", "embedding dim for sqlite storage"]
parent = "omega-memory-backend"
priority = "1"
related = ["memory-crate-interface"]
+++

# Semantic memory with local embeddings — vector retrieval for omegon-memory

## Problem

omegon-memory stores facts and retrieves them by exact key or recency. The agent can save "Wilson prefers terse responses" but can't find it when asking "what communication style does the user like?" — there's no semantic similarity search. Delfhos solves this with local sentence-transformer embeddings + cosine similarity over SQLite, keeping everything private and offline.

## Design

### Embedding backend

Use ONNX Runtime (`ort` crate) to run a small embedding model locally. No Python, no torch, no network calls.

**Model selection:**
- **Default:** `all-MiniLM-L6-v2` — 22M params, 384-dim, ~80MB ONNX, fast on CPU
- **Quality:** `bge-small-en-v1.5` — 33M params, 384-dim, better retrieval accuracy
- **Tiny:** `all-MiniLM-L12-v2` — for constrained environments

Model is downloaded on first use to `~/.config/omegon/models/` (or bundled in the distribution). Configurable via `OMEGON_EMBEDDING_MODEL` env var.

### Storage schema

Extend the existing `omegon-memory` SQLite schema:

```sql
ALTER TABLE memories ADD COLUMN embedding BLOB;
CREATE INDEX idx_memories_has_embedding ON memories (embedding IS NOT NULL);
```

The embedding is stored as a raw `f32` array (384 dims = 1536 bytes). SQLite handles BLOBs efficiently; no vector extension needed at this scale (typical memory stores hold hundreds to low thousands of entries, not millions).

### Retrieval

```rust
pub trait MemoryBackend: Send + Sync {
    // existing
    async fn store(&self, key: &str, value: &str, metadata: Metadata) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Option<MemoryEntry>>;
    async fn list(&self, prefix: &str) -> Result<Vec<MemoryEntry>>;
    async fn delete(&self, key: &str) -> Result<()>;

    // new
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<ScoredEntry>>;
}

pub struct ScoredEntry {
    pub entry: MemoryEntry,
    pub score: f32,  // cosine similarity, 0.0–1.0
}
```

`search()` flow:
1. Embed the query string using the local model
2. Load all embeddings from SQLite (or cache in memory on first query)
3. Compute cosine similarity against each stored embedding
4. Return top-k by score

For the expected scale (<5000 memories), brute-force cosine similarity over cached vectors is <1ms. No need for HNSW or ANN indices.

### Embedding computation

```rust
pub struct Embedder {
    session: ort::Session,
    tokenizer: tokenizers::Tokenizer,
}

impl Embedder {
    pub fn load(model_dir: &Path) -> Result<Self>;
    pub fn embed(&self, text: &str) -> Result<Vec<f32>>;
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
}
```

The embedder is initialized once at startup (or lazily on first memory operation) and shared via `Arc<Embedder>`. Embedding a single sentence takes ~5ms on modern hardware.

### Auto-extraction at session end

When a session ends (or at compaction boundaries), the agent loop already has the full conversation. Add a hook:

1. Summarize the session into candidate facts (existing compaction logic can feed this)
2. For each candidate fact, check if a semantically similar memory already exists (search with threshold 0.85)
3. If duplicate, skip or merge. If novel, store with embedding.

This mirrors Delfhos's session-close extraction but uses omegon's existing compaction infrastructure instead of a separate LLM call.

### Integration with agent loop

At turn start, when building the system prompt context:
1. Embed the user's message
2. `memory.search(user_message, limit=5)` — retrieve top 5 relevant memories
3. Inject as a `[Relevant memories]` section in the context, same as today's memory injection but ranked by relevance instead of recency

## Scope

### Phase 1: Embedder + search
- `Embedder` struct wrapping ort + tokenizers
- Model download/cache management
- `MemoryBackend::search()` trait extension
- SQLite schema migration for embedding column
- Brute-force cosine similarity retrieval
- ~300 lines in omegon-memory

### Phase 2: Auto-embedding on store
- Every `store()` call computes and persists the embedding
- Backfill command for existing memories without embeddings
- ~50 lines

### Phase 3: Semantic dedup + auto-extraction
- Session-end extraction hook
- Duplicate detection via similarity threshold
- ~150 lines in agent loop

## Dependencies

- `ort = "2"` — ONNX Runtime bindings (links to onnxruntime shared lib)
- `tokenizers = "0.20"` — HuggingFace tokenizer (pure Rust, no Python)
- Model files (~80MB) downloaded to `~/.config/omegon/models/`

## Risk

`ort` links to the ONNX Runtime C++ library. This adds ~15MB to the binary and requires the shared lib on each platform. The release pipeline already builds for 5 platforms; need to verify ort's prebuilt libs cover all of them (they do — ort publishes for linux-x64, linux-aarch64, macos-x64, macos-aarch64, windows-x64).
