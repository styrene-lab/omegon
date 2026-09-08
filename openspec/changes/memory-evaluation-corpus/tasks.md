## 1. Initial-wave offline regression fixtures
<!-- specs: memory/evaluation -->

- [x] Add synthetic input-only JSONL and evaluator-owned assertions for historical populations, scope, section filtering, packing, and an irrelevant Architecture flood.
- [x] Demonstrate behavioral red and green through both backend/provider implementations and the live managed tool boundary.
- [x] Add adversarial checks for vector/graph filtering, read-only repeated retrieval, quoted terms, and FTS failure after reopen.
- [x] Complete the scoped landing evidence in verification-wave-0.md; extraction-availability fixtures remain part of Wave 3.

## 2. Evidence isolation and offline determinism
<!-- specs: memory/evaluation -->

- [ ] RED: Add a fixture exposing future-event or answer-label leakage and an offline reproducibility case.
- [ ] GREEN: Implement evaluator-owned labels, evidence cutoffs, fake clocks, and controlled model outputs in the owning test targets.
- [ ] REFACTOR: Share fixture loaders without exposing assertions through writer or retriever interfaces.

## 3. Attribution and comparative reporting
<!-- specs: memory/evaluation -->

- [ ] RED: Add cases for packing loss, absent evidence, failed dependencies, and missing cost fields.
- [ ] GREEN: Emit per-stage outcomes and implement no-memory, file-search, current-memory, and candidate-policy adapters.
- [ ] REFACTOR: Consolidate run manifests and reporting while preserving raw per-case evidence.
- [ ] Freeze development-selected model-evaluation thresholds before held-out experiments; document the execution budget.

## 4. Verification
<!-- specs: memory/evaluation -->

- [ ] Record scenario-to-test mappings and red/green evidence; run focused domain and main-crate tests.
- [ ] Run applicable crate/shared-contract landing gates from ../../memory-modernization.md and validate this change.
