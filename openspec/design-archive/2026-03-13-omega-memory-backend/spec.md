+++
id = "c63ffd5d-9061-4fae-b863-c8d4341de2ae"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omega memory backend — Design Spec

> This spec defines acceptance criteria for the design phase.

## Scenarios

### All original open questions have recorded decisions

Given the omega-memory-backend design node was created with 2 open questions
When the design analysis resolves each question with a rationale
Then each question is removed and replaced by a decided decision entry

### Correctness failures are documented with fixes

Given the TS implementation has 6+ known correctness failures (any-typed DB, dimension mismatch, wrong-profile decay, etc.)
When each failure is analyzed
Then a research entry documents the failure AND either a TS preparatory fix or a Rust-only fix is prescribed

### TS/Rust boundary is defined with typed wire protocol

Given the memory system has two separable concerns (storage vs. tool registration)
When the boundary analysis completes
Then api-types.ts exists with full request/response types for all /api/memory/* endpoints
And field names match Rust serde conventions (snake_case)

### Schema migrations are implemented for correctness fixes

Given 3 correctness fixes require schema changes (decay_profile, Lamport version, embedding_metadata)
When the migration is written
Then schema_version advances to 3
And existing facts receive safe defaults (decay_profile='standard', version=0, last_accessed=NULL)
And new mutations increment the Lamport clock correctly

### Vector store strategy is decided with quantitative analysis

Given the vector store could use SQLite BLOB, sqlite-vec, or HNSW
When the performance analysis completes with concrete numbers (scan time vs embedding latency, break-even fact count)
Then a decision records which strategy is chosen with quantitative justification
And the composite scoring function requirement (similarity × decay-adjusted confidence) is addressed
