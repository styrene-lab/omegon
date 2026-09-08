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
