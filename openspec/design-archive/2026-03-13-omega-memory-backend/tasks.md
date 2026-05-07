+++
id = "f159e898-a18a-4c9c-9007-cb48acff80dc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omega memory backend — Tasks

## Completed

- [x] Extract pure computation to core.ts (computeConfidence, cosineSimilarity, vectorToBlob, contentHash, DecayProfileName)
- [x] Define api-types.ts wire protocol (all /api/memory/* request/response types)
- [x] Schema v3 migration: decay_profile column, version (Lamport) column, last_accessed column
- [x] Schema v3 migration: embedding_metadata table, model_name FK on facts_vec/episodes_vec
- [x] storeFact writes decay_profile and increments Lamport version
- [x] reinforceFact increments Lamport version
- [x] archiveFact increments Lamport version
- [x] semanticSearch uses per-fact decay_profile via resolveDecayProfile (not store-wide default)
- [x] semanticSearch calls touchFact for access reinforcement on returned results
- [x] semanticSearch logs dimension mismatch warning with count and guidance
- [x] storeFactVector registers model in embedding_metadata
- [x] Resolve Q1: JSONL format stays identical, version field additive with default 0
- [x] Resolve Q2: SQLite BLOB + linear scan permanently, quantitative justification recorded
- [x] Document 6 additional architectural issues (7-12) with fixes

## Remaining (deferred to Rust implementation)

- [ ] Port core.ts to Rust (src/memory/decay.rs, src/memory/vectors.rs)
- [ ] Implement Axum routes satisfying api-types.ts contract
- [ ] Cross-implementation tests (TS vs Rust produce identical results for same inputs)
- [ ] Budget-aware context rendering (max_chars parameter)
- [ ] Structured episode metadata (affected_nodes, files_changed, tags)
- [ ] Global DB advisory file lock for concurrent migration safety
- [ ] JSONL import with Lamport conflict resolution (higher version wins)
