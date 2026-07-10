# Runtime inference manifest loading — Tasks

## 1. Schema and conversion
<!-- specs: inference-manifests -->

- [x] 1.1 Add serde support required by the inventory's value types without coupling runtime files to internal map representation.
- [x] 1.2 Define versioned sparse TOML manifest records for groups, conceptual models, endpoints, transports, and offerings.
- [x] 1.3 Convert manifest records into provenance-correct `InventoryLayer` patches.
- [x] 1.4 Reject unsupported versions, managed runtime transports, malformed extension namespaces, and secret-looking references without echoing values.

## 2. Loader and atomic reload
<!-- specs: inference-manifests -->

- [x] 2.1 Add explicit `ManifestSource` records and pure default-path resolution for user/project manifests.
- [x] 2.2 Ignore absent optional files and fail on unreadable required files.
- [x] 2.3 Load all candidate layers before calling `InferenceInventoryStore::refresh`.
- [x] 2.4 Return structured, redacted read/parse/schema/conversion/validation diagnostics.
- [x] 2.5 Prove failed reload retains the exact active snapshot and generation.

## 3. Conformance and release gates
<!-- specs: inference-manifests -->

- [x] 3.1 Test standalone HTTP and local-process endpoint manifests.
- [x] 3.2 Test deterministic project/session sparse overrides and provenance.
- [x] 3.3 Test optional/required missing files, unsupported versions, malformed TOML, managed transport, and secret-value rejection.
- [x] 3.4 Run an adversarial review for path handling, diagnostics leakage, partial activation, and executable configuration; address findings.
- [x] 3.5 Update `[Unreleased]`, run focused/full tests, `just lint`, and `just link`.
- [x] 3.6 Reconcile lifecycle state and commit.
