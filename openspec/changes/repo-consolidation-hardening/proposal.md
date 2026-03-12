# Repo Consolidation, Security Hardening, and Lifecycle Normalization

## Intent

Reduce sprawl in major pi-kit subsystems, remove risky process-management patterns, normalize lifecycle truth across design-tree/OpenSpec/dashboard, and establish a clearer internal architecture for model control and shared status publication.

## Scope

**In scope (delivered as bounded child slices):**
- Cleave checkpoint parity and volatile memory hygiene (`cleave-checkpoint-parity`)
- Dashboard and lifecycle publisher consolidation (`dashboard-lifecycle-publisher-consolidation`)
- Lifecycle state normalization (`lifecycle-state-normalization`)
- pi-kit self-hosted web UI (`pikit-web-ui-hosting`)
- Subprocess safety hardening (`subprocess-safety-hardening`)

**Out of scope (deferred to future initiatives):**
- Oversized extension entrypoint decomposition (project-memory, cleave, openspec, design-tree index files)
- Model-control unification across effort/model-budget/offline-driver/local-inference/lib/model-routing

## Success Criteria

All five child slices archived with spec-backed verification. No open child nodes under this umbrella. Design node status `implemented`.
