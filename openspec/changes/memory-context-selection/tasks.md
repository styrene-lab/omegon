## 1. Task-aware shared selection
<!-- specs: memory/selection -->

- [x] RED: Reproduce irrelevant Architecture flooding and duplicate pinned/recalled facts through the standalone provider and renderer.
- [x] GREEN: Share early task-matched context retrieval in omegon-memory between standalone and managed ambient paths; enforce current eligibility and mind-scoped pins.
- [x] REFACTOR: Preserve domain candidate order during presentation instead of promoting Architecture by section order.
- [ ] Add full cross-adapter selection/explanation parity and explicit superseded-pin replacement tests.
- [ ] Complete shared selection and token-packing policy across MemoryProvider, MemoryFeature, and request_context; remove remaining adapter duplication.

## 2. Token packing and observable decisions
<!-- specs: memory/injection-budget -->

- [ ] Select and document the routine cap and per-kind allocations using development fixtures.
- [x] RED/GREEN: Reproduce and repair oversized-first-item starvation while accounting for complete emitted character blocks and deduplicating pins.
- [ ] RED: Add exact-token multilingual, zero-budget, low-signal, and telemetry cases for the complete shared selector.
- [ ] GREEN: Implement token-aware packing, provenance handles, and semantic inspection reporting.
- [ ] REFACTOR: Share packing/accounting and preserve host final-budget enforcement.

## 3. Cache correctness and verification
<!-- specs: memory/selection, memory/injection-budget -->

- [ ] RED: Add task-change, archive-before-TTL, scope-change, smaller-budget, eligibility-expiry, and unchanged-turn backend-call-count tests.
- [x] RED/GREEN: Reproduce a task change with no matches retaining a live injection; emit an explicit empty replacement to retire it in the managed context path.
- [ ] GREEN: Implement semantic cache invalidation and bounded selection reuse.
- [ ] Record scenario mappings and policy ablation results; verify runtime context and non-interactive inspection surfaces.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
- [x] Complete the Wave 2 scoped landing/adversarial gates in verification-wave-2.md; retain full token accounting, caching, and provenance work for Wave 5.
