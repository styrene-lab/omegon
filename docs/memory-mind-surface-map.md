---
title: Memory Mind Surface Map
status: deferred
tags: [architecture, memory, mind, decoupling, correctness]
---

# Memory Mind Surface Map

This document maps Omegon's semantic memory surface before further feature work. The goal is correctness and decoupling: preserve existing capabilities while making ownership, invariants, and extraction seams explicit.

## Current tool surface

Tool names are registered under `core/crates/omegon/src/tool_registry.rs` in `memory::*`.

| Tool | Purpose | Current adapter owner |
|---|---|---|
| `memory_store` | store/reinforce a fact | `core/crates/omegon/src/features/memory.rs` |
| `memory_recall` | retrieve relevant active facts | `core/crates/omegon/src/features/memory.rs` |
| `memory_query` | render active memory | `core/crates/omegon/src/features/memory.rs` |
| `memory_archive` | archive facts | `core/crates/omegon/src/features/memory.rs` |
| `memory_supersede` | replace a fact with a successor | `core/crates/omegon/src/features/memory.rs` |
| `memory_connect` | create fact edge | `core/crates/omegon/src/features/memory.rs` |
| `memory_focus` | pin fact ids into working memory | `core/crates/omegon/src/features/memory.rs` |
| `memory_release` | clear working memory pins | `core/crates/omegon/src/features/memory.rs` |
| `memory_episodes` | search/list session narratives | `core/crates/omegon/src/features/memory.rs` |
| `memory_compact` | request memory/context compaction | `core/crates/omegon/src/features/memory.rs` |
| `memory_search_archive` | search archived memory | `core/crates/omegon/src/features/memory.rs` |
| `memory_ingest_lifecycle` | ingest lifecycle candidates | `core/crates/omegon/src/features/memory.rs` |

## Engine/library surface

Crate:

```text
core/crates/omegon-memory/
```

Important modules:

| Module | Responsibility |
|---|---|
| `backend.rs` | `MemoryBackend` trait, `MemoryError`, `MemoryStats`, `ContextRenderer` |
| `sqlite.rs` | production SQLite backend, schema, FTS, vector persistence |
| `inmemory.rs` | test backend |
| `types.rs` | facts, edges, episodes, requests/responses, sections, statuses |
| `renderer.rs` | `MarkdownRenderer` for prompt context injection |
| `decay.rs` | confidence decay math |
| `embedding.rs` | embedding service trait |
| `vectors.rs` | vector serialization/search helpers |
| `vault_sync.rs` | Codex vault materialization/reinforcement |
| `hash.rs` | normalized content hashing |

Public engine abstractions:

```rust
MemoryBackend
ContextRenderer
SqliteBackend
InMemoryBackend
MarkdownRenderer
EmbeddingService
Fact / Edge / Episode / Section / request DTOs
DecayProfile / compute_confidence
```

## Architectural position

Memory already has a real engine crate, so its basic shape is healthy:

```text
MemoryFeature adapter → omegon-memory engine crate
```

But the feature adapter is still too broad. It currently owns both harness-facing responsibilities and domain policy.

## Adapter-owned responsibilities

These should stay in `core/crates/omegon/src/features/memory.rs` or another harness-layer adapter:

- tool definitions and JSON schemas
- JSON argument parsing
- `ToolResult` markdown/details formatting
- harness status refresh signaling
- context-provider integration and TTL/hash suppression
- session hook wiring
- LLM fact-extraction call wiring (`quick_completion`)
- policy for when to trigger session-end extraction
- operator-visible phrasing

## Engine/service-owned responsibilities

These belong in `omegon-memory` or a service boundary above `MemoryBackend`:

- fact store/reinforce semantics
- archive/supersede correctness
- mind isolation rules
- edge expansion for recall
- recall ranking pipeline
- FTS/vector fallback policy
- embedding side-effect contract as an explicit hook
- context candidate selection before final rendering
- JSONL export/import orchestration
- deterministic sync/projection rules

## Current coupling risks

### `MemoryFeature` is a facade plus policy engine

`memory.rs` currently combines:

- tool adapter
- working-memory pin state
- recall edge expansion
- auto-embedding spawn logic
- session-end extraction
- context render hash/dirty tracking
- Codex vault sync

This makes it hard to test memory correctness without also constructing harness/runtime state.

### Recall behavior is split across backend and feature

Backend owns FTS/vector queries, but feature owns edge expansion and result composition. That means a future non-tool consumer must duplicate or bypass feature behavior to get the same recall semantics.

### Context injection policy is partly hidden in adapter state

`last_context_hash`, `context_dirty`, and working-memory pins influence prompt injection but are not represented as a reusable context selection service. Some of this should stay adapter-side, but the selection contract should be explicit.

### Session extraction is harness-specific but writes engine facts

LLM extraction is correctly a harness concern. The boundary should make it clear that extraction produces candidate `StoreFact` requests; the memory engine should not know about provider calls.

## Correctness invariants

### Mind isolation

- Facts in mind `A` must never appear in list/search results for mind `B`.
- Edges are mind-scoped.
- Episodes are mind-scoped.
- Embedding metadata/search is mind-scoped.
- Deduplication by content hash is scoped by mind.

### Fact identity and deduplication

- Storing the same normalized active fact in the same mind reinforces the existing fact.
- Storing the same content in a different mind creates or reinforces only within that mind.
- Archived and superseded facts must not be treated as active recall results by default.
- Reinforcement advances the version clock and resets the decay/access timer as intended.

### Supersession

- Superseding a fact creates a replacement fact.
- The original becomes `Superseded`, not active.
- Supersession metadata points from old to new consistently.
- JSONL import must not resurrect superseded facts with stale lower-version records.

### Archive

- Archive is a soft delete.
- Archived facts remain available through explicit archive search/filtering.
- Archived facts are excluded from normal context injection and recall.

### Recall/search

- Empty or degenerate queries do not return arbitrary facts.
- Vector dimension mismatch is explicit (`EmbeddingDimensionMismatch`).
- Absence of embeddings is explicit (`NoEmbeddings`) or intentionally degraded by a service policy, not silently confused with no results.
- Edge expansion must not duplicate facts and must preserve deterministic ordering after scoring.

### Context injection

- Pinned working-memory facts render before normal facts.
- Character budget is respected.
- Facts are grouped by section deterministically.
- Context mutation marks context dirty.
- Unchanged render hash suppresses reinjection only when content truly has not changed.

### JSONL sync

- Export order is deterministic.
- Import is idempotent.
- Lamport version/conflict handling is deterministic.
- Import does not leak records across minds.
- Import does not revive stale archived/superseded records.

## Proposed service boundary

Introduce a service layer without adding new user-facing features:

```rust
pub struct MemoryMindService<B> {
    backend: B,
}
```

or object-safe equivalent:

```rust
pub struct MemoryMindService {
    backend: Arc<dyn MemoryBackend>,
}
```

Initial methods should be existing behavior, not new capability:

```rust
recall(query, options) -> Vec<ScoredFact>
store(req, hooks) -> StoreResult
archive(ids) -> usize
supersede(id, replacement) -> Fact
expand_edges(results, limit) -> Vec<ScoredFact>
select_context(mind, working_ids, budget) -> ContextSelection
```

The adapter would parse tool args and call the service. The service would encode memory semantics in one reusable place.

## Low-risk extraction candidates

1. **Edge expansion**
   - Move `MemoryFeature::expand_edges` into a service/helper.
   - Test with `InMemoryBackend`.
   - No tool behavior change.

2. **Fact extraction parser**
   - `parse_extracted_facts` is pure.
   - Add/retain tests for bullets, numbering, `NONE`, short lines.
   - Keep provider call wiring in adapter.

3. **Recall pipeline DTO**
   - Define options/results struct for recall.
   - Preserve current tool output.

4. **Context selection DTO**
   - Separate selecting facts/episodes/pins from rendering/injection TTL behavior.
   - Keep dirty/hash suppression in adapter.

## Relationship to Codebase Mind

Semantic Memory Mind and Codebase Mind should share vocabulary but not the same mutable store.

| Concern | Semantic Memory Mind | Codebase Mind |
|---|---|---|
| Primary unit | fact/episode/edge | file/symbol/relation/chunk/manifest |
| Store | memory facts DB | codebase structural DB |
| Projection | memory JSONL/vault notes | deterministic `ai/codebase/*.jsonl` |
| Context role | semantic guidance | structural repository awareness |
| Mutation source | agent/operator/session extraction | code/indexer/discovery pipeline |

Codebase Mind may eventually publish selected semantic observations into Memory Mind, but that promotion should be explicit. Structural facts should not be blindly stored as semantic memory facts.

## Non-goals for this branch

- No new memory tools.
- No Codebase Mind integration.
- No behavior-changing recall ranking rewrite.
- No new persistence format unless needed to preserve existing correctness.
- No broad refactor of context injection until invariants are covered.

## Recommended next implementation slice

1. Add tests around existing memory correctness invariants that are under-covered.
2. Extract edge expansion into a service/helper if tests can prove behavior preservation.
3. Keep `MemoryFeature` tool behavior unchanged.
4. Validate with `cargo test -p omegon-memory` and targeted `cargo test -p omegon memory` filters.
