## 1. Defaults reconciliation and readiness matrix
<!-- specs: memory/models -->

- [x] Amend historical model-default requirements explicitly; preserve the shipped Rust default with independent profile override/disable settings and the existing child policy.
- [x] RED/GREEN: Reproduce missing extraction without embeddings, then verify independent configuration, profile override, disable, and child-session behavior.
- [x] Wire the configured host extractor through setup and persist typed extraction outcomes independently of embedding state.
- [x] Keep provider clients/routing in the host and domain candidate validation in omegon-memory.
- [ ] Complete the embeddings-only/unavailable-storage readiness matrix and expose full component readiness through managed status.

## 2. Bounded degradation and recovery
<!-- specs: memory/models -->

- [ ] RED: Add read-only status, embedding timeout, cancellation, and repair-without-reinforcement scenarios.
- [ ] GREEN: Persist repairable index state and expose independent component failure reasons through existing semantic surfaces.
- [ ] REFACTOR: Reuse managed deadlines, cancellation, and embedding-space checks.
- [x] Verify controlled extractor failure and timeout preserve evidence, and cancellation after source commit leaves durable pending extraction.

## 3. Verification
<!-- specs: memory/models -->

- [ ] Verify no-agent compilation, setup behavior, non-interactive status, and replay/reopen of pending indexing state.
- [ ] Record scenario mappings and red/green evidence; run applicable landing gates from ../../memory-modernization.md and validate this change.
- [x] Complete the scoped Wave 3 gate in ../memory-evidence-capture/verification-wave-3.md; indexing repair remains later-wave work.

## 4. Adversarial configuration isolation
<!-- specs: memory/models -->

- [x] RED/GREEN: Reject invalid model configuration before it can prevent source persistence.
- [x] Complete the compatibility and landing recheck in ../memory-evidence-capture/adversarial-review-wave-3.md.
