## 1. Current and historical eligibility
<!-- specs: memory/retrieval, memory/search-stability -->

- [x] RED: Reproduce active-only archive results and ignored section filters through both standalone backends and the live managed tool adapter.
- [x] GREEN: Add current/historical SearchFilter contracts and enforce section eligibility before lexical/vector result limits and on expanded neighbors.
- [x] REFACTOR: Share eligibility/scoring policy without forcing identical backend relevance formulas; preserve compatibility defaults for existing callers.
- [x] Verify dormant/archived/superseded populations, read-only historical queries, quoted terms, reopen, and operational FTS failure propagation.

## 2. Embedding identity and explanatory ranking
<!-- specs: memory/retrieval -->

- [ ] RED: Add equal-dimension/different-model, unknown legacy space, repair replay, fused-score rendering, and graph-conflict cases.
- [ ] GREEN: Add embedding-space metadata, compatible-query enforcement, repair state, named score components, and relation-aware bounded expansion.
- [ ] REFACTOR: Share the public domain contract between MemoryProvider, managed service, and MemoryFeature adapters.

## 3. Verification
<!-- specs: memory/retrieval, memory/search-stability -->

- [ ] Verify vector migration/reopen and failed-repair atomicity; update wire/schema fixtures where changed.
- [ ] Map every scenario to tests and record red/green results, FTS-only behavior, and main-crate tool integration checks.
- [x] Complete the Wave 1 landing/adversarial gates documented in verification-wave-1.md; retain Wave 4 requirements as pending.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
