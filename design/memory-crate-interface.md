+++
id = "d3405642-a1ea-476d-803d-57162dd6b86f"
kind = "design_node"
title = "Memory crate interface boundary — MemoryBackend trait + integration with agent loop traits"
status = "implemented"
tags = ["rust", "memory", "traits", "interface", "architecture"]
aliases = ["memory-crate-interface"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "omega-memory-backend"
+++

# Memory crate interface boundary — MemoryBackend trait + integration with agent loop traits

## Overview

Define the Rust trait boundary for the memory crate so it can be developed independently and slotted into the agent loop. The memory crate implements ToolProvider (agent-callable tools), ContextProvider (injection), and SessionHook (startup/shutdown). Internally it owns a MemoryBackend trait that abstracts the storage engine — allowing sqlite in production and in-memory for tests.

## Research

### What the agent loop needs from memory — the consumer view

The agent loop touches memory at four points:

1. **Session start** — load facts for the current project (mind), render an injection block for the system prompt. This is the `SessionHook::on_session_start` path.

2. **Per-turn injection** — the `ContextProvider` is called before each LLM request. It receives the user prompt and recent context signals, and returns a `ContextInjection` with relevant facts. This is the hot path — must be fast (<10ms).

3. **Tool calls** — the agent calls `memory_store`, `memory_recall`, `memory_query`, etc. These are `ToolProvider::execute` dispatch targets. They need access to the storage backend.

4. **Session end** — persist any new facts, update episode narrative, flush JSONL for git sync. This is `SessionHook::on_session_end`.

The critical observation: **the memory crate is both a tool provider AND a context provider**. It registers tools so the agent can explicitly interact with memory, AND it injects memory context into the system prompt so the agent has ambient awareness without tool calls.

For cleave children specifically, the memory needs are simpler:
- Read-only access to the parent project's facts (inject relevant context)
- No extraction subagent (children are short-lived)
- No episode generation (children don't have meaningful session narratives)
- Possibly: store constraints/decisions discovered during execution (write-back)

### The MemoryBackend trait — storage abstraction

The crate needs a storage trait so tests can use an in-memory backend while production uses sqlite. The trait surface mirrors the API types but as direct Rust calls:

```rust
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    // ── Facts ────────────────────────────────────────────
    async fn store_fact(&self, req: StoreFact) -> Result<StoredFact>;
    async fn get_fact(&self, id: &str) -> Result<Option<Fact>>;
    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec<Fact>>;
    async fn reinforce_fact(&self, id: &str) -> Result<Fact>;
    async fn archive_fact(&self, id: &str) -> Result<()>;
    async fn supersede_fact(&self, id: &str, replacement: StoreFact) -> Result<Fact>;

    // ── Search ───────────────────────────────────────────
    async fn fts_search(&self, mind: &str, query: &str, k: usize) -> Result<Vec<ScoredFact>>;
    async fn vector_search(&self, mind: &str, embedding: &[f32], k: usize, min_sim: f32) -> Result<Vec<ScoredFact>>;
    async fn store_embedding(&self, fact_id: &str, model: &str, embedding: &[f32]) -> Result<()>;

    // ── Context rendering ────────────────────────────────
    async fn render_context(&self, req: ContextRequest) -> Result<RenderedContext>;

    // ── Edges ────────────────────────────────────────────
    async fn create_edge(&self, req: CreateEdge) -> Result<Edge>;
    async fn get_edges(&self, fact_id: &str) -> Result<Vec<Edge>>;

    // ── Episodes ─────────────────────────────────────────
    async fn store_episode(&self, req: StoreEpisode) -> Result<Episode>;
    async fn list_episodes(&self, mind: &str, k: usize) -> Result<Vec<Episode>>;
    async fn search_episodes(&self, mind: &str, query: &str, k: usize) -> Result<Vec<Episode>>;

    // ── JSONL sync ───────────────────────────────────────
    async fn export_jsonl(&self, mind: &str) -> Result<String>;
    async fn import_jsonl(&self, jsonl: &str) -> Result<ImportStats>;
}
```

This is ~20 methods. Each maps directly to an api-types.ts endpoint. The sqlite implementation owns the DB connection, WAL mode, migrations, and FTS5 indexing. The in-memory implementation is a simple HashMap for tests.

## Decisions

### Decision: MemoryBackend is an async trait with ~20 methods mirroring the api-types.ts contract

**Status:** decided
**Rationale:** The trait surface maps 1:1 to the HTTP endpoints defined in api-types.ts, but as direct Rust calls. Methods are async to support both spawn_blocking sqlite and potential future backends. Interior mutability via Mutex — all methods take &self. Two implementations planned: SqliteBackend (production) and InMemoryBackend (tests). The trait lives in omegon-memory crate, separate from the agent binary so it can be developed and tested independently.

### Decision: Memory integrates via the three omegon-traits: ToolProvider + ContextProvider + SessionHook

**Status:** decided
**Rationale:** A MemoryProvider struct wraps a MemoryBackend and implements the three traits. ToolProvider exposes memory_store/recall/query/etc as agent-callable tools. ContextProvider calls render_context() per-turn to inject relevant facts. SessionHook loads facts on startup (import_jsonl) and persists on shutdown (export_jsonl + episode). The agent loop doesn't know about memory internals — it just sees tools, context, and hooks.

### Decision: Decay math ported to Rust and verified against TS implementation

**Status:** decided
**Rationale:** compute_confidence, DecayProfile constants (STANDARD/GLOBAL/RECENT_WORK), resolve_profile, and MAX_HALF_LIFE_DAYS are ported from core.ts to decay.rs. 8 tests verify correctness: fresh fact = 1.0, half-life accuracy, reinforcement extension, max cap, recent work fast decay, reinforcement invariance for recent work, global slower than standard, profile resolution exhaustive.

## Open Questions

*No open questions.*
