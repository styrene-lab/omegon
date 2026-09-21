# Memory candidate reconciliation

## Intent

Plan evidence-aware admission after Wave 5, against source revision `7b2da402`.
Repeated paraphrases, explicit corrections, refinements, and unresolved
contradictions require different durable outcomes. This change is planned, not
implemented. See [the source assessment](assessment.md) for the starting boundary.

## Scope

Freeze candidate identity, decision ownership, admission units, and confirmation
contracts first. Then add bounded matching, atomic versioned admission, recoverable
decision work, host/read-side integration, and reconciliation evaluation.

Reuse `MemoryMutation`, operation receipts, `FactPrecondition`, both backends, and
the managed memory service. Foreground writes, lifecycle ingestion, and extracted
candidates use shared domain validation with explicit admission intent. Ordinary
explicit writes do not acquire blanket operator approval. Inferred lifecycle
admission retains its operator-confirmation policy.

Maintenance revalidation and procedural learning remain sibling work. This plan
does not authorize implementation, migrations, provider calls, or live spending.

## Success criteria

- Equivalent claims do not multiply active facts.
- Explicit corrections preserve historical evidence and supersession.
- Unresolved contradictions remain inspectable without silently choosing the newest.
- Retries and repeated evidence do not masquerade as independent confirmation.
- Durable decision recovery resumes admission without re-extracting completed batches.
- Changed decisions or target versions invalidate the corresponding operator approval.
- Development and held-out reconciliation corpora meet newly frozen measured
  thresholds under a fresh, explicitly authorized live budget.
