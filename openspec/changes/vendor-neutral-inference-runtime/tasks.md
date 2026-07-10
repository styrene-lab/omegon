# Vendor-neutral inference runtime model — Tasks

## 1. Neutral domain model
<!-- specs: inference-runtime-model -->

- [x] 1.1 Replace mandatory provider-integration identity with optional `EndpointGroupId` and `EndpointGroup` records in `core/crates/omegon/src/inference_inventory.rs`.
- [x] 1.2 Add `AdapterId`, `TransportSpec`, composable policy attributes, and namespaced extension metadata.
- [x] 1.3 Add corresponding sparse patch fields and provenance-preserving merge behavior.
- [x] 1.4 Keep offerings attached to endpoints and add offering extension metadata.

## 2. Validation and bootstrap projection
<!-- specs: inference-runtime-model -->

- [x] 2.1 Validate IDs, optional group references, adapter identity, transport requirements, extension namespaces, and secret-reference shape.
- [x] 2.2 Project `ModelRegistry` endpoints into neutral groups, adapters, transports, and offerings without changing current consumers.
- [x] 2.3 Ensure compatibility filtering uses adapter identity and remains independent of extension metadata.

## 3. Adversarial fixtures and gates
<!-- specs: inference-runtime-model -->

- [x] 3.1 Test standalone private, local-process, public API, brokered, and opaque connector metadata fixtures.
- [x] 3.2 Test malformed transport, secret-looking credential value, dangling group, and invalid metadata namespace rejection.
- [x] 3.3 Run an adversarial review focused on accidental vendor coupling, executable configuration, and policy conflation; address findings.
- [x] 3.4 Update `[Unreleased]`, run focused and full tests, `just lint`, and `just link`.
- [x] 3.5 Reconcile OpenSpec lifecycle state and commit the completed slice.
