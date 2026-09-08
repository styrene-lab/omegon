## 1. Current and historical eligibility
<!-- specs: memory/retrieval, memory/search-stability -->

- [x] RED: Reproduce active-only archive results and ignored section filters through both standalone backends and the live managed tool adapter.
- [x] GREEN: Add current/historical SearchFilter contracts and enforce section eligibility before lexical/vector result limits and on expanded neighbors.
- [x] REFACTOR: Share eligibility/scoring policy without forcing identical backend relevance formulas; preserve compatibility defaults for existing callers.
- [x] Verify dormant/archived/superseded populations, read-only historical queries, quoted terms, reopen, and operational FTS failure propagation.

## 2. Embedding identity and explanatory ranking
<!-- specs: memory/retrieval -->

- [x] RED: Reproduce unidentified equal-dimensional vector comparison and missing score/conflict metadata; add model/revision/pipeline/dimension drift fixtures.
- [x] GREEN: Persist explicit embedding space and source hashes, expose per-fact index state, and enforce identified query comparison.
- [x] Add named channel scores, bounded top-k collection, directional graph evidence, conflict labels, and current/historical supersession behavior.
- [x] Share domain contracts and score formatting between standalone and live managed adapters; preserve legacy hybrid response envelopes unless diagnostics are requested.

## 3. Verification
<!-- specs: memory/retrieval, memory/search-stability -->

- [x] Verify schema-v9 migration to v10, reopen, unknown legacy identity, stale content fingerprints, and failed-repair rollback; regenerate the schema contract.
- [x] Exercise the real backfill CLI against a local fake server, including `--cwd`, ready-vector skipping, and unchanged fact reinforcement/version.
- [x] Verify Ollama digest-change fencing and optional local artifact identity/shape boundaries with controlled tests.
- [x] Complete scenario mappings, final gates, and adversarial verdict in verification-wave-4.md.
- [x] Complete the Wave 1 landing/adversarial gates documented in verification-wave-1.md; retain Wave 4 requirements as pending.
- [x] Run applicable landing gates from ../../memory-modernization.md and validate this change.

## 4. Adversarial episode-search parity
<!-- specs: memory/search-stability -->

- [x] RED/GREEN: Fix quoted episode queries, title matching, and empty-query behavior across both backends.
- [x] Complete the final landing recheck in ../memory-evidence-capture/adversarial-review-wave-3.md.
