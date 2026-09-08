## 1. Task-aware shared selection
<!-- specs: memory/selection -->

- [ ] RED: Add irrelevant-flood, cross-adapter parity, duplicate-pin, and superseded-pin cases with controlled retrieval inputs.
- [ ] GREEN: Implement the shared selector in omegon-memory and wire MemoryProvider, MemoryFeature, and request_context adapters.
- [ ] REFACTOR: Remove independent adapter selection rules and leave renderers presentation-only.

## 2. Token packing and observable decisions
<!-- specs: memory/injection-budget -->

- [ ] Select and document the routine cap and per-kind allocations using development fixtures.
- [ ] RED: Add multilingual/token-boundary, zero-budget, oversized-first-item, low-signal, and telemetry cases.
- [ ] GREEN: Implement token-aware packing, provenance handles, and semantic inspection reporting.
- [ ] REFACTOR: Share packing/accounting and preserve host final-budget enforcement.

## 3. Cache correctness and verification
<!-- specs: memory/selection, memory/injection-budget -->

- [ ] RED: Add task-change, archive-before-TTL, scope-change, smaller-budget, eligibility-expiry, and unchanged-turn backend-call-count tests.
- [ ] GREEN: Implement semantic cache invalidation and bounded selection reuse.
- [ ] Record scenario mappings and policy ablation results; verify runtime context and non-interactive inspection surfaces.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
