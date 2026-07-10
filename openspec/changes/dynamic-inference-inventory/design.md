# Dynamic inference inventory foundation — Design

## Scope

This change implements the domain foundation behind the decided provider-route/conceptual-model matrix without replacing the existing `ModelRegistry` consumers in one step.

The first vertical slice adds:

1. Typed identities for provider integrations, endpoint deployments, model offerings, and optional conceptual models.
2. Extensible inference interfaces, modalities, capability evidence, capability-specific quality grades, and ungraded state.
3. Deterministic field-level merge of ordered inventory layers with provenance.
4. Snapshot validation and an atomically activated, last-known-good inventory generation.
5. Compatibility filtering that evaluates hard interface/modality/capability/evidence requirements before optional grades.
6. An adapter from the embedded `ModelRegistry`, establishing it as bootstrap inventory rather than the only possible source.

## Architecture

### Module boundary

Add `core/crates/omegon/src/inference_inventory.rs` as the domain and snapshot owner. Keep endpoint request shaping and legacy registry lookup in `model_registry.rs`. This avoids destabilizing current provider construction while creating the seam through which runtime files and discovery can be wired later.

### Identity model

- `ProviderIntegrationId`: administrative/provider integration identity.
- `EndpointDeploymentId`: callable deployment identity and protocol/auth boundary.
- `OfferingId`: stable route identity within a deployment.
- `ConceptualModelId`: optional reviewed equivalence identity.

An `InferenceOffering` references exactly one existing deployment. A deployment references exactly one provider integration. Conceptual identity is optional.

### Evidence and quality

Every mergeable capability field is represented by an evidenced value containing the value, source layer, and verification kind. Capability support and quality grades are separate maps. Missing grade is represented by absence, never by a fallback average.

### Layer merge

`InventoryLayer` has an explicit source and precedence. Records are sparse patches. Layers are sorted by `(precedence, input order)` and merged field-by-field. Higher precedence replaces only supplied fields; untouched values keep their original provenance.

### Activation

`InferenceInventoryStore` owns an `Arc<RwLock<Arc<InventorySnapshot>>>`. Refresh builds and validates a complete candidate off-lock, then takes the write lock only to assign the next generation. Validation failure returns diagnostics and leaves the existing `Arc` untouched. This is atomic for readers without adding a dependency.

### Compatibility

`CompatibilityRequest` filters in this order:

1. enabled deployment/offering;
2. inference interface;
3. required input/output modalities;
4. required capabilities and minimum evidence confidence;
5. ungraded policy;
6. capability-specific minimum grades.

Explicit offering pins may admit ungraded offerings; autonomous selection excludes them unless policy opts in.

## Migration

The embedded registry is projected into a bootstrap layer. Existing callers remain on `ModelRegistry` during this change. Subsequent changes can wire configuration loading, discovery/probes, semantic route resolution, and catalog UX onto `InferenceInventoryStore` independently.

## Security and operations

- Inventory contains secret references, never secret values.
- IDs and references are validated before activation.
- Unknown interfaces are data values but are incompatible unless a request names the same understood interface.
- Snapshot refresh has no partial writes.
- Diagnostic errors identify record IDs without exposing credentials.

## Validation

Unit tests cover ungraded internal offerings, heterogeneous modalities, field provenance, deterministic precedence, dangling references, last-known-good retention, generation activation, compatibility-before-grade, explicit ungraded selection, policy-admitted ungraded selection, and evidence/grade independence.
