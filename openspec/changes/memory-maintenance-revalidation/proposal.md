# Evidence-aware memory maintenance — Wave 7

## Intent

Separate evidence support from age and attention. After Wave 5, effective confidence
still multiplies persisted confidence by reinforcement-dependent age decay. The
ambient floor, dormancy planner, and selection-cache deadlines share that policy.
Vault references and duplicate stores still extend its influence through writes.

This is a planned change. [Assessment](assessment.md) distinguishes existing
behavior from remaining work. All implementation tasks remain unchecked.

## Scope

- Define memory-kind policy for evidence confidence, freshness, usage, validity,
  and retention before migrating data.
- Extend existing dormancy planning through `MemoryMutation`, `TransitionFacts`,
  `FactPrecondition`, and payload-bound receipts.
- Add bounded source-linked scheduling and managed-runtime revalidation.
- Preserve archive-search semantics and add explicit all-status retained-history
  retrieval with as-of applicability, including Active-but-expired facts.
- Coordinate selection, cache deadlines, write-side attention semantics, and
  field-by-field transport compatibility.
- Evaluate stale-guidance rejection alongside retention of old supported knowledge.

Wave 6 owns source-independence and correction admission. Wave 7 consumes an
accepted, integrated Wave 6 contract reference before enabling correction. No such
acceptance is established here. A sibling branch does not satisfy that dependency.
Pure policy, characterization, readers, and fixtures can proceed after DTO freeze.
Wave 8 consumes the evidence-versus-attention and validity slice, not the complete
Wave 7 scheduler. See [design](design.md) for ownership and handoff gates.

## Success criteria

- Old supported invariants remain eligible when applicable across cold and cached selection.
- Reads, references, and duplicate stores do not masquerade as independent verification.
- Changed sources produce bounded, durable work with explicit incomplete-read outcomes.
- Inspection and indexing freshness never imply claim revalidation.
- Atomic maintenance rejects stale inputs and changed-payload operation reuse;
  exact replay returns the original outcome before fresh source or version reads.
- Legacy metadata and imported verification attribution survive transport without
  importing another workspace's queue, receipts, or local progress.
- Development thresholds are frozen before held-out evaluation. Any live-model
  evaluation has fresh authorization; the Wave 5 budget is not standing permission.
