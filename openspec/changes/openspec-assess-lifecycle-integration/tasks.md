# openspec-assess-lifecycle-integration — Tasks

Dependencies:
- Group 1 defines the persisted assessment artifact and snapshot freshness helpers used by later groups.
- Group 2 makes assessment producers emit the lifecycle metadata OpenSpec must persist.
- Group 3 wires verify/archive/reconcile flows onto the authoritative assessment record.
- Group 4 updates docs/tests and validates fail-closed lifecycle behavior.

## 1. Persisted assessment artifact + snapshot helpers
<!-- specs: openspec/assessment-lifecycle -->

- [x] 1.1 Add assessment artifact helpers in `extensions/openspec/spec.ts` for reading and writing per-change `assessment.json`
- [x] 1.2 Define the persisted assessment record shape with change name, assessment kind, outcome, timestamp, snapshot identity, and reconciliation hints
- [x] 1.3 Add freshness helpers that compare persisted assessment state against the current implementation snapshot
- [x] 1.4 Ensure persisted assessment lookup is scoped to the requested change so records cannot bleed across active changes
- [x] 1.5 Add tests for artifact read/write, per-change scoping, and stale-vs-current snapshot detection

## 2. Structured assessment producers
<!-- specs: openspec/assessment-lifecycle -->

- [x] 2.1 Tighten `extensions/cleave/assessment.ts` result contracts around lifecycle outcomes: `pass`, `reopen`, `ambiguous`
- [x] 2.2 Update `extensions/cleave/index.ts` so bridged assessment results expose change name, assessment kind, outcome, snapshot identity, and reconciliation hints in persistable form
- [x] 2.3 Preserve the structured metadata through `extensions/lib/slash-command-bridge.ts` so lifecycle consumers receive it intact
- [x] 2.4 Add tests proving structured assessment outputs carry the metadata OpenSpec needs to persist and gate on

## 3. OpenSpec lifecycle integration
<!-- specs: openspec/assessment-lifecycle -->

- [x] 3.1 Update `extensions/openspec/index.ts` so `/opsx:verify` executes or refreshes structured assessment for the current implementation snapshot
- [x] 3.2 Allow `/opsx:verify` to reuse persisted assessment state only when it is current for the present snapshot
- [x] 3.3 Persist the latest relevant structured assessment result into the change directory during verify/reconcile flows
- [x] 3.4 Enforce archive fail-closed behavior when relevant assessment state is missing, stale, ambiguous, or reopened
- [x] 3.5 Permit archive to proceed only when the latest relevant assessment for the current snapshot is an explicit pass
- [x] 3.6 Update reconciliation flows to consume structured assessment hints directly instead of reparsing human-readable assessment text
- [x] 3.7 Add tests for verify refresh, verify reuse of current state, archive refusal paths, and archive success on current explicit pass

## 4. Docs, workflow guidance, and validation
<!-- specs: openspec/assessment-lifecycle -->

- [x] 4.1 Expand `docs/openspec-assess-lifecycle-integration.md` with the OpenSpec-owned assessment artifact model and verify/archive gate behavior
- [x] 4.2 Update lifecycle guidance or representative `openspec/changes/*/tasks.md` patterns to reflect assessment/reconciliation checkpoints explicitly
- [x] 4.3 Add regression tests ensuring lifecycle commands preserve human-readable UX while using structured state internally
- [x] 4.4 Validate targeted tests and typecheck for verify/archive/reconcile behavior and persisted assessment artifacts
