## 1. Defaults reconciliation and readiness matrix
<!-- specs: memory/models -->

- [x] Amend historical model-default requirements explicitly; preserve the shipped Rust default with independent profile override/disable settings and the existing child policy.
- [x] RED/GREEN: Reproduce missing extraction without embeddings, then verify independent configuration, profile override, disable, and child-session behavior.
- [x] Wire the configured host extractor through setup and persist typed extraction outcomes independently of embedding state.
- [x] Keep provider clients/routing in the host and domain candidate validation in omegon-memory.
- [x] Complete the embeddings-only/unavailable-storage readiness matrix and expose full component readiness through managed status.

## 2. Bounded degradation and recovery
<!-- specs: memory/models -->

- [x] Add read-only status, embedding timeout, cancellation, and repair-without-reinforcement scenarios; record actual RED evidence without treating compile errors as behavioral failures.
- [x] GREEN: Persist repairable index state and expose independent component failure reasons through existing semantic surfaces.
- [x] REFACTOR: Reuse managed deadlines, cancellation, and embedding-space checks.
- [x] Verify controlled extractor failure and timeout preserve evidence, and cancellation after source commit leaves durable pending extraction.

## 3. Verification
<!-- specs: memory/models -->

- [x] Verify no-agent compilation, setup behavior, non-interactive status, and replay/reopen of pending indexing state.
- [x] Record scenario mappings and red/green evidence; run applicable landing gates from ../../memory-modernization.md and validate this change.
- [x] Complete the scoped Wave 3 gate in ../memory-evidence-capture/verification-wave-3.md; the Wave 4/5 sections complete indexing repair and readiness.

## 4. Adversarial configuration isolation
<!-- specs: memory/models -->

- [x] RED/GREEN: Reject invalid model configuration before it can prevent source persistence.
- [x] Complete the compatibility and landing recheck in ../memory-evidence-capture/adversarial-review-wave-3.md.

## 5. Wave 4 explicit indexing repair
<!-- specs: memory/models -->

- [x] Route automatic indexing and recall through identified generation, with bounded waits and cooperative cancellation.
- [x] Expose missing/legacy/incompatible/stale/ready per-fact state and preserve facts when generation or version-checked writes fail.
- [x] Verify explicit CLI repair, ready-vector skipping, digest drift, and no reinforcement from repair.
- [x] Complete the Wave 4 gate in ../memory-retrieval-contract/verification-wave-4.md; Wave 5 completes durable attempt state and full component status, with bounded explicit repair as the recovery action.

## 6. Wave 5 component observations
<!-- specs: memory/models -->

- [x] Add pure readiness projections covering the independent storage/extraction/embedding matrix, unknown indexing, and extraction-outage/pending-repair coexistence.
- [x] Publish startup configuration through cached, serialized harness status; verify repeated reads and storage outages preserve independent observations without durable writes.
- [x] Share identified-generation cancellation/deadline handling across automatic generation, recall, and explicit repair; verify owned inference is dropped.
- [x] Integrate durable pending indexing and content-free failure reasons through both backends and managed status; verify migration, version/attempt/space checks, atomic rollback, cancellation, reopen, and repair without reinforcement.
- [x] Record the final Wave 5 gate and reconcile all remaining checkboxes before archival.
