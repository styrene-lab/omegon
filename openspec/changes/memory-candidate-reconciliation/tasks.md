## 1. Scoped equivalence and evidence identity
<!-- specs: memory/reconciliation -->

- [ ] RED: Add exact duplicate, paraphrase, repeated-source, distinct-source, and cross-platform fixtures with a fake classifier.
- [ ] GREEN: Implement bounded candidate lookup and evidence-preserving equivalence decisions in the domain service.
- [ ] REFACTOR: Route foreground, lifecycle, and background admission through shared validation with explicit intent.

## 2. Corrections, conflicts, and retry semantics
<!-- specs: memory/reconciliation -->

- [ ] RED: Add authoritative correction, unresolved inference conflict, concurrent-version failure, retired-paraphrase, and unavailable-classifier cases.
- [ ] GREEN: Implement atomic reconciliation plans, conflict relationships, lifecycle transitions, and recoverable pending decisions.
- [ ] REFACTOR: Reuse mutation receipts and optimistic preconditions across both backends.

## 3. Verification
<!-- specs: memory/reconciliation -->

- [ ] Verify reopen/transport of candidate decisions, conflict provenance, and partial-failure rollback.
- [ ] Record scenario mappings and red/green evidence; run formation/retrieval interaction cases and applicable landing gates from ../../memory-modernization.md.
- [ ] Validate this change and record any model-quality experiment separately from deterministic contract results.
