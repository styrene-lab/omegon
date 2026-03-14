---
id: omega-memory-rust-port
title: Omega memory Rust port — rusqlite + Axum handlers satisfying api-types.ts
status: deferred
parent: omega-memory-backend
dependencies: [omega-memory-backend]
tags: [rust, memory, sqlite, axum, port]
open_questions: []
issue_type: feature
priority: 3
---

# Omega memory Rust port — rusqlite + Axum handlers satisfying api-types.ts

## Overview

Port the memory storage engine from TypeScript (factstore.ts, core.ts, embeddings.ts) to Rust. All design decisions are already made in the parent node. The TS hardening sprint (schema v4, unified pipeline, hybrid search, decay sweep, embedding lifecycle) means the Rust port is a faithful reimplementation, not a redesign.

Scope:
- src/memory/decay.rs — computeConfidence, DecayProfile enum (direct port of core.ts)
- src/memory/vectors.rs — cosine_similarity, vector BLOB serde (SIMD auto-vectorized)
- src/memory/store.rs — FactStore with rusqlite, typed row structs, schema migrations via refinery
- src/memory/search.rs — FTS5 + semantic search + hybrid RRF merge, typed params
- src/memory/jsonl.rs — JSONL import/export (serde_json, field names match current facts.jsonl)
- src/memory/api.rs — Axum routes for /api/memory/* satisfying api-types.ts contract exactly
- Cross-impl tests: same inputs → same outputs for core.ts vs decay.rs/vectors.rs

The TS extension becomes an auspex bridge: registers tools with pi, forwards calls to Omega HTTP endpoints, receives pre-rendered context blocks.

Prerequisites: parent node decisions are all implemented in TS and validated. api-types.ts is the canonical contract.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `src/memory/mod.rs` (new) — Module root — re-exports, FactStore constructor
- `src/memory/decay.rs` (new) — computeConfidence, DecayProfile enum, MAX_HALF_LIFE_DAYS cap — direct port of core.ts
- `src/memory/vectors.rs` (new) — cosine_similarity (SIMD via auto-vec), vector_to_blob, blob_to_vector — direct port of core.ts
- `src/memory/store.rs` (new) — FactStore: rusqlite + typed FactRow/EpisodeRow/EdgeRow structs, store/reinforce/archive/supersede with Lamport clock
- `src/memory/search.rs` (new) — FTS5 full-text, semantic search with typed params, hybrid RRF merge (k=60), sweepDecayedFacts
- `src/memory/jsonl.rs` (new) — JSONL import/export — serde_json with #[serde(rename_all)] matching current facts.jsonl field names exactly
- `src/memory/api.rs` (new) — Axum routes: /api/memory/facts, /context, /recall, /episodes, /edges, /export, /import — satisfying api-types.ts
- `src/memory/migrations/` (new) — refinery SQL migrations — schema v1-v4 matching current TS migrations exactly
- `tests/memory_cross_impl.rs` (new) — Cross-implementation tests: same inputs to TS core.ts and Rust decay.rs/vectors.rs produce identical outputs
- `extensions/project-memory/index.ts` (modified) — Auspex bridge mode: detect running Omega, forward tool calls to HTTP endpoints, receive pre-rendered context

### Constraints

- api-types.ts field names are the source of truth — Rust serde attributes must match exactly
- cosine_similarity must produce bit-identical results to TS for same f32 inputs (cross-impl test)
- computeConfidence must match TS for same (daysSince, reinforcementCount, profile) inputs
- Schema migrations must produce identical table structure to current TS migrations (v1-v4)
- JSONL format is backward-compatible — Rust must read files produced by TS and vice versa
- Lamport version semantics identical: MAX(version)+1 per mutation, default 0 on import
- TS extension must work standalone (no Omega) AND in bridge mode (Omega running) — feature-detect at startup
- No HNSW, no sqlite-vec — linear scan only per parent decision
