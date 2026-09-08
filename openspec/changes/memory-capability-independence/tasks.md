## 1. Defaults reconciliation and readiness matrix
<!-- specs: memory/models -->

- [ ] Decide whether to restore historical model-default requirements or add explicit modifications for current routing; record the decision before coding.
- [ ] RED: Add fake-provider setup cases for extraction-only, embeddings-only, neither optional capability, and unavailable storage.
- [ ] GREEN: Resolve extraction and embeddings independently and wire provider-neutral typed readiness through setup and managed status.
- [ ] REFACTOR: Remove availability coupling without moving provider policy into omegon-memory.

## 2. Bounded degradation and recovery
<!-- specs: memory/models -->

- [ ] RED: Add read-only status, embedding timeout, cancellation, and repair-without-reinforcement scenarios.
- [ ] GREEN: Persist repairable index state and expose independent component failure reasons through existing semantic surfaces.
- [ ] REFACTOR: Reuse managed deadlines, cancellation, and embedding-space checks.

## 3. Verification
<!-- specs: memory/models -->

- [ ] Verify no-agent compilation, setup behavior, non-interactive status, and replay/reopen of pending indexing state.
- [ ] Record scenario mappings and red/green evidence; run applicable landing gates from ../../memory-modernization.md and validate this change.
