## 1. Initial-wave offline regression fixtures
<!-- specs: memory/evaluation -->

- [x] Add synthetic input-only JSONL and evaluator-owned assertions for historical populations, scope, section filtering, packing, and an irrelevant Architecture flood.
- [x] Demonstrate behavioral red and green through both backend/provider implementations and the live managed tool boundary.
- [x] Add adversarial checks for vector/graph filtering, read-only repeated retrieval, quoted terms, and FTS failure after reopen.
- [x] Complete the scoped landing evidence in verification-wave-0.md; extraction-availability fixtures remain part of Wave 3.

## 2. Evidence isolation and offline determinism
<!-- specs: memory/evaluation -->

- [x] RED: Add a fixture exposing future-event or answer-label leakage and an offline reproducibility case.
- [x] GREEN: Implement evaluator-owned labels, evidence cutoffs, fake clocks, and controlled model outputs in the owning test targets; applicability time is injected and wall-clock decay is neutralized as documented in verification-wave-5.md.
- [x] REFACTOR: Share fixture loaders without exposing assertions through writer or retriever interfaces.

## 3. Attribution and comparative reporting
<!-- specs: memory/evaluation -->

- [x] RED: Add cases for packing loss, absent evidence, failed dependencies, and missing cost fields.
- [x] GREEN: Emit per-stage outcomes and implement no-memory, file-search, current-memory, and candidate-policy adapters.
- [x] REFACTOR: Consolidate run manifests and reporting while preserving raw per-case evidence.
- [x] Freeze development-selected model-evaluation thresholds before held-out experiments; document the execution budget. The Codex subscription continuation passed frozen smoke acceptance within the original aggregate cap; see codex-continuation.md and live-codex-subscription-wave5.json. Initial unavailable reports are preserved.

## 4. Verification
<!-- specs: memory/evaluation -->

- [x] Record scenario-to-test mappings and red/green evidence; run focused domain and main-crate tests.
- [x] Run applicable crate/shared-contract landing gates from ../../memory-modernization.md and validate this change.

Implementation evidence is in verification-wave-5.md and codex-continuation.md.
Focused evaluator, runtime selection, and non-interactive inspection tests pass.
The parent owns final shared-tree gates and joint archival.
