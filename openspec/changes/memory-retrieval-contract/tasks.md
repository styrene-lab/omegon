## 1. Current and historical eligibility
<!-- specs: memory/retrieval, memory/search-stability -->

- [ ] RED: Add cross-backend archive, dormancy, filter-before-limit, malformed query, and storage-failure cases.
- [ ] GREEN: Implement typed retrieval intent/filter contracts and wire memory_recall and memory_search_archive in the main crate.
- [ ] REFACTOR: Centralize eligibility without forcing identical backend relevance formulas.

## 2. Embedding identity and explanatory ranking
<!-- specs: memory/retrieval -->

- [ ] RED: Add equal-dimension/different-model, unknown legacy space, repair replay, fused-score rendering, and graph-conflict cases.
- [ ] GREEN: Add embedding-space metadata, compatible-query enforcement, repair state, named score components, and relation-aware bounded expansion.
- [ ] REFACTOR: Share the public domain contract between MemoryProvider, managed service, and MemoryFeature adapters.

## 3. Verification
<!-- specs: memory/retrieval, memory/search-stability -->

- [ ] Verify vector migration/reopen and failed-repair atomicity; update wire/schema fixtures where changed.
- [ ] Map every scenario to tests and record red/green results, FTS-only behavior, and main-crate tool integration checks.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
