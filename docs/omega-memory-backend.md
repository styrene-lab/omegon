---
id: omega-memory-backend
title: Omega memory backend — Rust-native fact store and retrieval engine
status: decided
parent: omega
tags: [rust, memory, sqlite, embeddings, architecture]
open_questions: []
---

# Omega memory backend — Rust-native fact store and retrieval engine

## Overview

The project-memory extension (~10,365 lines TS) has the same correctness problems as the cleave and lifecycle code: an `any`-typed SQLite boundary, no schema enforcement, silent dimension mismatches on vector retrieval, and decay math that could silently use the wrong profile. The core storage engine (SQLite, decay, FTS5, cosine similarity, JSONL I/O) has zero pi API dependencies and is a direct Rust migration target.

The critical boundary: the memory system has two separable concerns currently fused in factstore.ts + index.ts:
1. Storage and retrieval (SQLite, decay, FTS5, vector search, JSONL) — deterministic, no pi API → Omega
2. Tool registration, extraction subagent, injection orchestration — pi API surface → stays TS

The auspex bridge pattern applies here exactly as with cleave: the TS extension registers tools (memory_store, memory_recall, memory_query, etc.) and forwards calls to Omega's /api/memory/* HTTP endpoints. Omega owns the DB, computes decay, runs vector search, and renders the injection markdown. The TS extension gets a pre-rendered context block from /api/memory/context rather than building it locally.

## Research

### Correctness failures in the current TS implementation

**1. Fully `any`-typed SQLite boundary**
`loadDatabase()` returns `any`. Every `db.prepare()` returns `any`. Every `stmt.get()` and `stmt.all()` returns `any`. The type system is entirely bypassed at the database boundary. Row deserialization silently succeeds even if the schema has drifted. In Rust, `rusqlite` with `serde` maps rows to typed structs — schema drift produces a compile-time or explicit runtime error, not silent garbage data.

**2. Embedding dimension mismatch is silently skipped, not caught**
`semanticSearch` skips rows where `row.dims !== queryDims` (factstore.ts:1672). This means if the embedding model changes (e.g., qwen3-embedding 768-dim → a 1024-dim model), thousands of existing vectors become silently invisible — they don't error, they just don't participate in search. There's no warning, no fact count delta, no user signal. The DB has no `embedding_model` column — you can't even tell which model generated which vector.

**3. Decay profile is not stored with the fact**
`computeConfidence` takes a `DecayProfile` parameter but facts don't store which profile they were written with. A "Recent Work" fact (halfLife=2d) could be read and decayed with the standard DECAY profile (halfLife=60d), producing a confidence of ~0.98 for a fact that should have decayed to 0.1. The DB schema has `decay_rate: number` (a single float) but the decay profile has four interdependent parameters. Storing one float is insufficient to reconstruct the profile.

**4. Content hash deduplication has no collision detection**
`contentHash` returns a 16-char truncated sha256 hex. Collision probability is ~1/2^64 — negligible in practice, but a collision produces a silent deduplication failure (a new fact is treated as identical to an existing one). More importantly, hash normalization (`normalizeForHash`) strips punctuation and lowercases — two semantically different facts can normalize to the same hash and get deduped. In Rust this would at minimum be a debug assertion.

**5. The `node:sqlite` fallback wrapper is `any` typed throughout**
The `NodeSqliteWrapper` class (lines 46-79) wraps `node:sqlite`'s `DatabaseSync` with `any` types for all arguments and returns. It's an escape hatch that bypasses both TS and the underlying SQLite library's safety guarantees. The two codepaths (better-sqlite3 vs node:sqlite) are not guaranteed to behave identically — edge cases in transaction rollback, BLOB handling, or PRAGMA behavior may differ silently.

**6. `semanticSearch` params array is `any[]`**
`const params: any[] = [mind]` — SQL parameter binding is type-erased. A wrong param type (passing a number where a string is expected) would only surface as a runtime query error, not a type error.

### What moves to Rust and what stays TS — the boundary

**Moves to Omega (Rust):**
- SQLite schema, migrations, WAL configuration → rusqlite + refinery (typed migrations)
- `computeConfidence` decay math → pure Rust fn, fully typed DecayProfile enum
- `cosineSimilarity`, `vectorToBlob`, `blobToVector` → Rust with ndarray or raw f32 slices; SIMD auto-vectorized by LLVM
- `semanticSearch` → typed row structs, no params: any[]
- FTS5 full-text search → same rusqlite
- JSONL export/import → serde_json, typed Fact struct
- `renderForInjection` / injection markdown rendering → Omega's /api/memory/context endpoint
- Decay profile stored with each fact (new schema column: profile enum)
- Embedding model + dimension versioned in DB: new `embedding_metadata` table

**Stays in TypeScript (pi API surface):**
- `memory_store`, `memory_recall`, `memory_query`, `memory_focus`, `memory_connect` etc. tool registration
- Extraction subagent: the background LLM call to extract facts from conversation — this needs pi.sendUserMessage, stays in TS
- Injection orchestration: deciding *when* to call /api/memory/context (on first turn, on each turn, etc.)
- `memory_compact`: triggers context compaction via pi's core API — TS only
- Working memory buffer (pinned fact IDs): session-local TS state, referenced when building context requests

**The API surface between TS and Omega:**
- POST /api/memory/facts — store a fact (params: content, section, mind, confidence?)
- GET /api/memory/context?mind=X&query=Y&mode=semantic|bulk — returns pre-rendered markdown block
- POST /api/memory/recall — semantic search (params: query embedding or raw text, k, section filter)
- GET /api/memory/facts?mind=X — list active facts (for memory_query)
- PATCH /api/memory/facts/:id — reinforce, supersede, archive
- POST /api/memory/edges — create a relationship
- GET /api/memory/export?mind=X — JSONL dump for git sync
- POST /api/memory/import — JSONL import from git sync
- GET /api/memory/episodes?mind=X&k=N — recent session episodes
- POST /api/memory/episodes — store a new episode narrative

### Preparatory work achievable NOW in the TS codebase

These changes make the migration cheaper without requiring Omega to exist yet:

**1. Add embedding model metadata to the DB schema (fix dimension mismatch bug)**
Add `embedding_metadata` table: { model_name TEXT, dims INTEGER, created_at TEXT }.
Add `model_name TEXT` column to `facts_vec`. Before semantic search, verify query vector dims match the stored model dims. If mismatch: return empty results AND emit a warning telling the operator to run memory_purge_vectors. This is a correctness fix AND a migration prerequisite.

**2. Store the decay profile enum with each fact**
Add `decay_profile TEXT NOT NULL DEFAULT 'standard'` column to `facts` table.
Values: 'standard' | 'global' | 'recent_work'. Modify `computeConfidence` callers to read `fact.decay_profile` and select the right DecayProfile. This eliminates the wrong-profile decay bug.

**3. Extract pure-computation functions into a separate module**
Create `extensions/project-memory/core.ts` — contains only `computeConfidence`, `cosineSimilarity`, `vectorToBlob`, `blobToVector`, `contentHash`, `normalizeForHash`. No DB imports, no pi imports, no Node imports. This module becomes a direct Rust port target. Identical interface in both TS and Rust during the migration window.

**4. Eliminate the `any`-typed SQLite wrapper — use typed row interfaces everywhere**
Define explicit row interfaces for every `stmt.get()` and `stmt.all()` call:
`const row = stmt.get(mind) as FactRow;` → `const row: FactRow = stmt.get(mind);`
Combined with a runtime assertion on DB open that checks schema version. This doesn't eliminate the underlying `any` from the library but makes schema drift visible.

**5. Define the /api/memory/* contract in TypeScript types now**
Create `extensions/project-memory/api-types.ts` with the full HTTP request/response envelope types. These types become the shared contract between the TS auspex bridge and the Rust Omega implementation. When Omega is built, the Rust structs derive from this spec (serde attribute names match the TypeScript field names exactly).

**6. Cap the extraction subagent output to typed ExtractionAction[]**
`parseExtractionOutput` (factstore.ts:1924) already returns `ExtractionAction[]`, but its input is free-form LLM text. Add a schema validator (zod or manual) so malformed extraction output is rejected rather than silently stored as a garbage fact.

### Six additional architectural issues surfaced under deep analysis

**Issue 7: Git-sync conflict resolution is broken for archive/supersede (CRITICAL)**
Union merge + dedup-by-reinforcement_count resurrects archived facts when a concurrent reinforcement has higher reinforcement_count. Fix: Lamport timestamp — higher version always wins, regardless of reinforcement_count. This is the motivation for adding version to the JSONL format. See decision above.

**Issue 8: Access patterns don't influence decay — only time and explicit reinforcement**
A fact retrieved by memory_recall 3 turns ago is demonstrably relevant but its confidence continues decaying as if untouched. Fix: `last_accessed: Option<DateTime>` column. Effective decay uses `max(last_reinforced, last_accessed)` as the time reference. This is a soft reinforcement — doesn't increment reinforcement_count or extend half-life, just resets the decay timer.

**Issue 9: Extraction subagent has no debounce — can fire multiple times per turn**
Rapid tool-call sequences can trigger multiple extraction runs in one turn. Each is a full LLM round-trip. contentHash dedup prevents duplicate storage but the calls are wasted. Fix: 60-second session-local debounce timer in TS. Trivial to implement.

**Issue 10: Context injection has no budget parameter — always renders to section caps**
The injection renders up to section cap regardless of remaining context budget. If context is 80% full, 12,000 chars of memory can push it over. Fix: ContextRequest includes max_chars; Omega renders and measures, stopping when budget exhausted while maintaining priority ordering (working_memory > semantic_hits > recent_architecture_facts).

**Issue 11: Episodes are opaque text blobs — structurally unqueryable**
Episodes have no metadata beyond date+title+narrative. You can't ask "which sessions touched the omega design node?" Fix: add affected_nodes, affected_changes, files_changed, tags, tool_calls_count to EpisodeRecord. The extraction subagent can populate these from conversation context. Already reflected in api-types.ts::EpisodeRecord.

**Issue 12: Global DB has no concurrent migration guard**
Multiple Omega instances opening ~/.pi/memory/global.db simultaneously (different project sessions starting at the same time) can race on schema migration. Fix: advisory fcntl file lock on ~/.pi/memory/global.lock acquired before checking PRAGMA user_version. Hold time is milliseconds. Standard Rust fs2::FileExt::lock_exclusive().

## Decisions

### Decision: Decay profile stored as an enum column on each fact — not inferred at read time

**Status:** decided
**Rationale:** Currently facts don't record which decay profile they were written with. A Recent Work fact (halfLife=2d) could be decayed with the standard profile (halfLife=60d) and appear artificially alive. Storing `decay_profile: 'standard' | 'global' | 'recent_work'` per fact eliminates this. In Rust this becomes a `#[derive(Serialize, Deserialize)] enum DecayProfile` — the DB value must match a known variant or deserialization fails at insert time, not silently at read time.

### Decision: Embedding model name and dimension versioned in DB — dimension mismatch is a hard error, not a silent skip

**Status:** decided
**Rationale:** Current code silently skips vectors with mismatched dimensions. This means a model change silently degrades semantic search to zero results with no user signal. In Omega: an `embedding_metadata` table records the active model name and dimension. On search, if the query vector dimension doesn't match the stored model dimension, return an explicit error with guidance to run `omega memory purge-vectors --model old-model-name`. In Rust, dimension mismatch is a typed error variant, not a conditional skip.

### Decision: Pure computation core extracted to standalone module before Rust port

**Status:** decided
**Rationale:** computeConfidence, cosineSimilarity, vectorToBlob/blobToVector, contentHash have zero pi/Node/DB dependencies. Extracting them to core.ts (TS) now lets us write tests against them in isolation, then port those exact tests as the Rust test suite. The behavioral spec travels with the code. During the migration window, both TS and Rust implementations run the same tests against the same inputs — behavioral equivalence is verified before the DB layer switches over.

### Decision: JSONL format stays identical; Lamport version field added as optional with default 0 on import

**Status:** decided
**Rationale:** Vectors are not in the JSONL (DB-local only), so embedding_metadata adds zero JSONL impact. decay_profile is additive with default "standard". The one meaningful addition is `version: u64` (Lamport logical timestamp) which fixes the git-sync conflict resolution bug: when union-merge produces competing mutations (e.g., one machine archives a fact, another reinforces it), higher version wins unconditionally. Without version, the dedup logic resurrects archived facts by comparing reinforcement_count — a correctness bug for multi-machine operators. version defaults to 0 on import from old files; Lamport clock initializes to MAX(version)+1 after each import. No breaking change, no conversion tool.

### Decision: SQLite BLOB + Rust linear scan permanently — no HNSW index, no sqlite-vec

**Status:** decided
**Rationale:** At 384 dims, linear scan of 1335 facts takes 0.018ms vs. 100-500ms for Ollama embedding generation — 5,000-25,000x faster than the prerequisite step. Break-even requires ~7.4M facts (384-dim) or ~3.7M facts (768-dim). A prolific operator storing 50 facts/day accumulates 182,500 facts after 10 years; scan time at that scale is 2.5ms. HNSW is the wrong tool: it trades recall for speed at millions of vectors and cannot express our composite scoring function (similarity × decay-adjusted confidence requires per-fact decay parameters, which a pure distance metric cannot encode). sqlite-vec fails the same test and adds a C FFI dependency. LLVM auto-vectorizes Rust f32 slice iteration — we get SIMD for free.

### Decision: api-types.ts is the canonical migration contract — Rust Axum handlers must satisfy it exactly

**Status:** decided
**Rationale:** extensions/project-memory/api-types.ts defines all /api/memory/* HTTP request/response envelope types. Field names are snake_case to match Rust serde conventions. The Rust structs derive Serialize+Deserialize with field names identical to these TypeScript interfaces. Any deviation is a bug in the Rust port. The file also defines JsonlRecord (JSONL wire format discriminated union), EmbeddingMetadata, and ExtractionAction — the full set of types crossing the TS/Rust boundary.

### Decision: Access reinforcement via last_accessed column — soft decay timer reset on retrieval

**Status:** decided
**Rationale:** The decay model previously only considered time-since-last-reinforced and explicit reinforcement_count. But when memory_recall returns a fact, that's a strong signal of active relevance — the fact shouldn't decay during the current work session. Fix: semanticSearch calls touchFact(id) on returned results, setting last_accessed to now. The confidence computation uses max(last_reinforced, last_accessed) as the effective time anchor. This doesn't increment reinforcement_count (avoids inflating the long-term persistence signal), just resets the decay timer. In Rust: last_accessed: Option<DateTime<Utc>>.

### Decision: Context injection is budget-aware — Omega renders to a max_chars limit with priority ordering

**Status:** decided
**Rationale:** renderForInjection currently always renders up to section caps regardless of available context budget. In the Omega model, GET /api/memory/context accepts max_chars and stops adding facts when the budget would be exceeded, respecting priority ordering: working_memory (pinned) > semantic_hits > recent_architecture_facts. The TS bridge tells Omega 'give me N chars' and receives a pre-measured block. This prevents memory injection from consuming context needed for the task itself.

### Decision: Global DB uses advisory file lock for concurrent migration safety

**Status:** decided
**Rationale:** Multiple Omega instances (one per project) share ~/.pi/memory/global.db. If two instances start simultaneously and both find schema version < expected, both attempt migrations — second one fails or corrupts. Fix: acquire advisory fcntl file lock on ~/.pi/memory/global.lock before checking PRAGMA user_version. Released after migration completes. Hold time is milliseconds. Rust: fs2::FileExt::lock_exclusive(). Standard POSIX semantics, works across processes.

### Decision: Hybrid FTS5+embedding retrieval via Reciprocal Rank Fusion

**Status:** decided
**Rationale:** Neither FTS5 nor embeddings alone is sufficient. FTS5 has 100% coverage, zero latency, and catches identifiers — but returns 0 results for paraphrased queries (AND mode) or low-precision noise (OR mode). Embeddings catch semantic similarity — but have 73% coverage (broken indexer left 562 facts invisible for 5 days), ~80ms latency, and mediocre quality on short factual text (278 chars avg) with a 0.6B model. RRF merge (k=60) boosts facts appearing in both lists and degrades to FTS5-only when embeddings are unavailable — no codepath split needed.

### Decision: Unified 6-tier priority pipeline replaces dual bulk/semantic injection

**Status:** decided
**Rationale:** The old semantic mode injected ALL Decisions (315 facts, 143K chars, ~36K tokens) because the 'load all Decisions' design was written when there were ~20. The length<30 cap only gated semantic hits, not core facts. Bulk mode capped at 50 facts but had no query relevance. The unified pipeline fixes both: 6 tiers (WM → Decisions top-8 → Arch top-8 → hybrid search → structural fill → recency fill) with a character budget gate (15% of context, floor 4K, ceiling 16K). Priority ordering means highest-value facts always survive budget pressure. When embeddings are down, T4 degrades to FTS5-only — no separate codepath.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/core.ts` (new) — Pure computation: computeConfidence, cosineSimilarity, vectorToBlob/blobToVector, contentHash, normalizeForHash — no DB/pi/Node imports
- `extensions/project-memory/api-types.ts` (new) — HTTP request/response envelope types for /api/memory/* — shared contract between TS bridge and Rust Omega
- `extensions/project-memory/factstore.ts` (modified) — Add decay_profile column support, embedding_metadata table, typed row interfaces, import core.ts for pure fns
- `extensions/project-memory/migration.ts` (modified) — Add migration for decay_profile column and embedding_metadata table
- `src/memory/mod.rs` (new) — Omega memory backend: FactStore, DecayProfile enum, schema migrations via refinery
- `src/memory/decay.rs` (new) — computeConfidence, DecayProfile enum — direct port of core.ts with exhaustive match
- `src/memory/vectors.rs` (new) — cosine_similarity (SIMD via auto-vectorization), vector BLOB serde, embedding metadata
- `src/memory/search.rs` (new) — FTS5 full-text search, semantic search with typed params, dimension mismatch as typed error
- `src/memory/api.rs` (new) — Axum routes: /api/memory/* — fact CRUD, context rendering, recall, export/import, episodes
- `src/memory/jsonl.rs` (new) — JSONL import/export for git sync — serde_json, identical field names to current facts.jsonl
- `extensions/project-memory/api-types.ts` (new) — Canonical Omega /api/memory/* wire protocol types — source of truth for TS bridge and Rust Axum handlers
- `extensions/project-memory/factstore.ts` (modified) — Schema v3 migration (decay_profile, version, last_accessed, embedding_metadata); storeFact/reinforceFact/archiveFact write Lamport version; semanticSearch uses per-fact decay_profile and touchFact for access reinforcement; storeFactVector registers model in embedding_metadata
- `extensions/project-memory/core.ts` (modified) — Pure computation module — zero deps, direct Rust port target (computeConfidence, cosineSimilarity, vectorToBlob/blobToVector, contentHash, DecayProfileName, resolveDecayProfile)

### Constraints

- JSONL field names must match current facts.jsonl exactly for zero-migration compatibility (decide before implementation)
- decay_profile column added via migration — default 'standard' for all existing facts
- embedding_metadata table: model_name TEXT, dims INTEGER, inserted_at TEXT — one row per model ever used
- facts_vec gains model_name TEXT FK to embedding_metadata — allows multi-model coexistence
- cosine_similarity in Rust must produce bit-identical results to TS implementation for same inputs (verified by cross-impl test)
- Dimension mismatch must return EmbeddingDimensionMismatch error, not silently skip
- JSONL format must remain backward-compatible — new fields are additive with defaults on import
- Lamport version is MAX(version)+1 per mutation — never use wall-clock as version
- Dimension mismatch must produce a logged warning, not a silent skip
- Per-fact decay_profile must be used at read-time, not store-wide default
- embedding_metadata table must be populated on first storeFactVector call

## Acceptance Criteria

### Falsifiability

- This decision is wrong if: If any open question remains unresolved, the node cannot be decided
- This decision is wrong if: If api-types.ts is missing or incomplete (fewer than 10 endpoint types), the wire protocol is not defined
- This decision is wrong if: If schema v3 migration fails to add decay_profile, version, or last_accessed columns, the correctness fixes are incomplete
- This decision is wrong if: If semanticSearch still uses the store-wide decay profile instead of per-fact decay_profile, the wrong-profile bug persists
- This decision is wrong if: If storeFact/reinforceFact/archiveFact do not increment the Lamport clock, git-sync conflict resolution remains broken
