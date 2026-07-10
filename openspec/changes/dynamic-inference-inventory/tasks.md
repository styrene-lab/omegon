# Dynamic inference inventory foundation — Tasks

## Dependencies

- Group 2 depends on Group 1's identity, evidence, and patch types.
- Group 3 depends on Group 2's validated snapshots.
- Group 4 depends on Groups 1–3 and completes lifecycle reconciliation.

## 1. Inventory domain and embedded bootstrap adapter
<!-- specs: inference-inventory -->

- [x] 1.1 Add typed provider integration, endpoint deployment, offering, optional conceptual-model, inference-interface, modality, evidence, and capability-grade structures in `core/crates/omegon/src/inference_inventory.rs`.
- [x] 1.2 Represent ungraded offerings as missing capability grades without preventing explicit use.
- [x] 1.3 Add a projection from `ModelRegistry` into an embedded bootstrap `InventoryLayer` without changing existing registry consumers.
- [x] 1.4 Test internal ungraded offerings and heterogeneous offerings under one provider integration.

## 2. Layer merge, validation, and atomic activation
<!-- specs: inference-inventory -->

- [x] 2.1 Implement sparse record patches and deterministic precedence ordering for embedded, organization, user, project, session, discovery, and probe layers.
- [x] 2.2 Merge fields independently while retaining source and verification provenance.
- [x] 2.3 Validate unique IDs and provider/deployment/conceptual-model references before activation.
- [x] 2.4 Implement generation-stamped snapshot activation with last-known-good retention on validation failure.
- [x] 2.5 Test single-field override provenance, dangling-reference rejection, generation increment, and no partial activation.

## 3. Compatibility filtering and ungraded policy
<!-- specs: inference-inventory -->

- [x] 3.1 Add hard filtering for enabled state, inference interface, input/output modalities, required capabilities, and evidence confidence.
- [x] 3.2 Add capability-specific grade floors after hard compatibility filtering; do not route on display averages.
- [x] 3.3 Exclude ungraded offerings from autonomous selection by default while allowing exact pins and explicit policy admission.
- [x] 3.4 Return structured rejection reasons suitable for later route diagnostics.
- [x] 3.5 Test image/text incompatibility, default ungraded exclusion, exact ungraded selection, policy admission, and probed capability evidence without invented grades.

## 4. Integration and verification
<!-- specs: inference-inventory -->

- [x] 4.1 Register the module in `main.rs` and document the bootstrap/runtime boundary in module-level documentation.
- [x] 4.2 Update `CHANGELOG.md` with the implemented foundation behavior.
- [ ] 4.3 Run focused tests, `cargo test -p omegon`, `just lint`, and `just link`.
- [ ] 4.4 Reconcile task status, design-tree implementation state, and OpenSpec verification evidence before archive.
