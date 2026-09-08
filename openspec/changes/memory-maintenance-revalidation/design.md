# Maintenance and revalidation design

Depend on provenance and reconciliation. Define separate evidence confidence,
freshness timestamps, usage/salience signals, validity intervals, and lifecycle
status. Existing legacy confidence/decay fields require an explicit migration
interpretation: preserve their historical values as legacy metadata rather than
claiming calibrated evidence confidence. New reads do not mutate evidence strength.

Use policy by memory kind and evidence, not a universal age-to-truth rule. A durable
invariant can retain current eligibility with old observation time and unchanged
supporting revision. A transient workaround can expire or require revalidation.
Selection may deprioritize old rarely useful material without making it untrue.

Source revision/hash changes enqueue bounded revalidation against declared evidence
references. Revalidation can confirm, correct through reconciliation, mark unresolved,
or identify unavailable evidence. It must not infer deletion from missing mounts,
transient reads, symlinks outside the vault, or provider failure.

Maintenance plans identify reasons, expected versions, and intended transitions.
Application checks versions atomically and replays through existing receipts.
Archive/dormancy is retention policy; verified contradiction is a knowledge update.
Preserve these distinctions in inspection. Provide correction/retirement operations
through existing command registration rather than a new TUI-only arm.

Use injected clocks and controlled source snapshots for tests. Preserve bidirectional
vault idempotency and avoid source-content logging. Portable evidence history and
local usage statistics need explicit export semantics and round-trip fixtures.
