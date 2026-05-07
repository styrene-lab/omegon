+++
id = "436fbb1e-ed01-46bc-aa1c-92d6b73852eb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory crate interface boundary — MemoryBackend trait + integration with agent loop traits — Design Spec (extracted)

> Auto-extracted from docs/memory-crate-interface.md at decide-time.

## Decisions

### MemoryBackend is an async trait with ~20 methods mirroring the api-types.ts contract (decided)

The trait surface maps 1:1 to the HTTP endpoints defined in api-types.ts, but as direct Rust calls. Methods are async to support both spawn_blocking sqlite and potential future backends. Interior mutability via Mutex — all methods take &self. Two implementations planned: SqliteBackend (production) and InMemoryBackend (tests). The trait lives in omegon-memory crate, separate from the agent binary so it can be developed and tested independently.

### Memory integrates via the three omegon-traits: ToolProvider + ContextProvider + SessionHook (decided)

A MemoryProvider struct wraps a MemoryBackend and implements the three traits. ToolProvider exposes memory_store/recall/query/etc as agent-callable tools. ContextProvider calls render_context() per-turn to inject relevant facts. SessionHook loads facts on startup (import_jsonl) and persists on shutdown (export_jsonl + episode). The agent loop doesn't know about memory internals — it just sees tools, context, and hooks.

### Decay math ported to Rust and verified against TS implementation (decided)

compute_confidence, DecayProfile constants (STANDARD/GLOBAL/RECENT_WORK), resolve_profile, and MAX_HALF_LIFE_DAYS are ported from core.ts to decay.rs. 8 tests verify correctness: fresh fact = 1.0, half-life accuracy, reinforcement extension, max cap, recent work fast decay, reinforcement invariance for recent work, global slower than standard, profile resolution exhaustive.

## Research Summary

### What the agent loop needs from memory — the consumer view

The agent loop touches memory at four points:

1. **Session start** — load facts for the current project (mind), render an injection block for the system prompt. This is the `SessionHook::on_session_start` path.

2. **Per-turn injection** — the `ContextProvider` is called before each LLM request. It receives the user prompt and recent context signals, and returns a `ContextInjection` with relevant facts. This is the hot path — must be fast (<10ms).

3. **Tool calls** — the agent calls `memory_st…

### The MemoryBackend trait — storage abstraction

The crate needs a storage trait so tests can use an in-memory backend while production uses sqlite. The trait surface mirrors the API types but as direct Rust calls:

```rust
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    // ── Facts ────────────────────────────────────────────
    async fn store_fact(&self, req: StoreFact) -> Result<StoredFact>;
    async fn get_fact(&self, id: &str) -> Result<Option<Fact>>;
    async fn list_facts(&self, mind: &str, filter: FactFilter) -> Result<Vec…
