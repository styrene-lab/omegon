# Retrieval design

Own intent, filtering, eligibility, score metadata, and graph policy in
`omegon-memory`. The main crate forwards explicit requests and formats results.
Evolve `MemoryBackend` and managed DTOs with compatibility adapters where needed.
Update `schema-contract.json` if the persisted/wire contract changes.

Use distinct current and historical intents. Historical requests carry explicit
statuses and optional as-of constraints once provenance supports validity ranges.
The archive tool selects archived, dormant, and superseded evidence and labels
each result. Current requests retain current-fact eligibility. Neither intent
changes confidence or reinforcement merely by reading.

Embedding space identifies model/revision, preprocessing identity, and dimensions.
Treat legacy vectors without sufficient identity as incompatible until explicitly
reindexed or mapped by verified migration metadata. Keep per-fact indexing status
and a bounded repair path. Do not silently map same-dimensional vectors.

Use typed score components: lexical relevance, cosine similarity, fused rank, and
graph contribution. These are not probabilities. Preserve existing deterministic
FTS-only fallback but expose optional-index degradation. Storage failures remain
errors. Graph traversal applies scope/status/section eligibility to neighbors and
uses relation semantics: contradiction links expose conflict rather than add
support, supersession links do not revive obsolete current facts. Define bounded
candidate counts and stable tie-breaking.

Contract tests share populations and expected eligibility across SQLite and
InMemoryBackend. They need not assert equal BM25 and lexical numeric scores.

## Wave 4 decisions

Persist optional typed embedding-space metadata and a raw-content SHA256 beside
fact vectors in schema v10. Migration leaves both fields unknown on older vectors.
Identified writes use fact-version preconditions and atomic receipts. Legacy
unidentified query APIs remain callable but refuse comparison without an identity;
the live hybrid path reports typed degradation and retains lexical results.

Ollama identity uses its documented `/api/tags` content digest, checked before and
after generation, plus the canonical model name and our raw-input preprocessing
contract. This assumes an honest stable server during the request, not cryptographic
attestation of execution. Local ONNX identity hashes model and tokenizer artifacts
and identifies the pooling/normalization pipeline. Local loading uses the exact
bounded artifact buffers that were hashed and does not implicitly resolve external
weight files. Unsupported embedding services
must explicitly implement identified generation before memory uses their vectors.

Index readiness is a per-fact derived state: missing, legacy, incompatible, stale,
or ready. The existing explicit embedding-backfill CLI remains the repair surface,
with bounded pages, generation deadlines, version checks, and content/space-bound
operation identities. It does not reinforce facts. Durable job scheduling remains
outside this wave.

Search results retain legacy numeric fields for deserialization compatibility and
add named channel scores plus directional graph evidence. Graph-derived scores are
retrieval proximity, not truth confidence. Contradiction links label both seeded
and expanded matches without reinforcing or boosting existing seeds. Unknown
relations do not imply support; supersession direction controls expansion.

The existing hybrid DTO keeps its legacy result envelope unless the caller requests
diagnostics; callers without query identity receive keyword fallback. Direct legacy
vector query APIs fail with an identity-required error. Operational vector or graph
storage errors propagate through the modern path rather than disappearing.
