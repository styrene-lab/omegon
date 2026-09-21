## 1. Task-aware shared selection
<!-- specs: memory/selection -->

- [x] RED: Reproduce irrelevant Architecture flooding and duplicate pinned/recalled facts through the standalone provider and renderer.
- [x] GREEN: Share early task-matched context retrieval in omegon-memory between standalone and managed ambient paths; enforce current eligibility and mind-scoped pins.
- [x] REFACTOR: Preserve domain candidate order during presentation instead of promoting Architecture by section order.
- [x] Add cross-adapter selection/explanation parity and explicit superseded-pin replacement tests.
- [x] Share selection and token-packing policy across MemoryProvider, MemoryFeature, and request_context.

## 2. Token packing and observable decisions
<!-- specs: memory/injection-budget -->

- [x] Select and document the routine cap and per-kind allocations using development fixtures.
- [x] RED/GREEN: Reproduce and repair oversized-first-item starvation while accounting for complete emitted character blocks and deduplicating pins.
- [x] Add a caller-supplied exact-counter fixture plus conservative multilingual, zero-budget, low-signal, and telemetry cases.
- [x] Implement counted token-aware packing, provenance handles, and semantic inspection reporting.
- [x] Share packing/accounting and preserve host final-budget enforcement.

## 3. Cache correctness and verification
<!-- specs: memory/selection, memory/injection-budget -->

- [x] Verify task-change, archive-before-TTL, scope-change, smaller-budget, eligibility-expiry, and unchanged-turn retrieval-call-count regressions; hosted lack-of-reuse RED evidence is recorded in verification-wave-5-cache.md.

- [x] RED/GREEN: Reproduce a task change with no matches retaining a live injection; emit an explicit empty replacement to retire it in the managed context path.
- [x] GREEN: Implement semantic cache invalidation and bounded selection reuse.
- [x] Record scenario mappings and policy ablation results; verify runtime context and non-interactive inspection surfaces. See verification-wave-5-evaluation.md and the completed Codex subscription comparison.
- [x] Run applicable selection/cache landing gates from ../../memory-modernization.md and validate this change; comparative policy evaluation is recorded in verification-wave-5-evaluation.md.
- [x] Complete the Wave 2 scoped landing/adversarial gates in verification-wave-2.md; retain full token accounting, caching, and provenance work for Wave 5.

## 4. Wave 5 counted selection slice
<!-- specs: memory/selection, memory/injection-budget -->

- [x] RED: Reproduce multilingual ambient output exceeding conservative host accounting.
- [x] Verify whole-block counted packing, zero budget, eligibility reasons, superseded pins, and low-signal episode exclusion.
- [x] Verify shared standalone/hosted/explicit-pack selection and configurable cap intersection.
- [x] Record development fixture results, compatibility, final gates, and same-executor review in verification-wave-5-selection.md.

## 5. Wave 5 semantic cache slice
<!-- specs: memory/selection, memory/injection-budget -->

- [x] RED: Reproduce lack of reuse through the hosted selection report.
- [x] Verify retrieval-call suppression, task/target/pin/budget changes, archive and external SQLite invalidation, clock reversal, and future eligibility boundaries.
- [x] Verify conservative confidence-floor expiry, backend-instance isolation, and mutation-during-computation behavior.
- [x] Record final gates and same-executor review in verification-wave-5-cache.md.

The cache slice is accepted. After Xcode license acceptance, the final
`RUST_TEST_THREADS=1 just test-commit --base f16cddfb^` gate passed on 2026-09-16.
See the cache verification record for results and the execution transcript.
