# OpenSpec lifecycle integration with structured assessment results — Design

## Architecture Decisions

### Decision: OpenSpec is the lifecycle authority and must own persisted assessment state

**Status:** decided  
**Rationale:** OpenSpec is the underpinning workflow framework for design-tree, cleave, assess, and adjacent operations. Assessment outcomes that affect lifecycle progression therefore belong to OpenSpec, not as ephemeral command output only. OpenSpec should persist the latest structured assessment state per active change so verify, archive, reconciliation, dashboarding, and future workflow tools can all read the same authoritative lifecycle record.

### Decision: Persist the latest structured assessment result inside each OpenSpec change

**Status:** decided  
**Rationale:** Assessment state must be attributable to a specific change, review kind, and implementation snapshot. The simplest durable v1 design is for each active change to carry its own assessment artifact or metadata file inside the change directory, rather than storing it in transient process memory or a separate global cache. This keeps lifecycle state co-located with proposal/design/spec/tasks and makes archive gating inspectable and reproducible.

### Decision: `/opsx:verify` should execute or refresh structured assessment, not only render cached state

**Status:** decided  
**Rationale:** Verification is an active lifecycle checkpoint, not just a reporting view. `/opsx:verify` should invoke the relevant structured assessment path or confirm that an equivalent assessment result is current for the present implementation snapshot, then render the outcome for humans and expose it for agents. Cached assessment state is useful, but verify should not silently trust stale results.

### Decision: Archive must fail closed on missing, stale, ambiguous, or reopened assessment state

**Status:** decided  
**Rationale:** Because OpenSpec is the lifecycle authority, archive cannot rely on best-effort operator sequencing. If the latest relevant assessment is absent, predates implementation changes, reports ambiguity, or explicitly reopens work, archive should refuse to proceed and point the operator/agent to verify and reconcile first. Only explicit pass state for the current implementation snapshot should satisfy the archive gate.

### Decision: Assessment records should capture implementation snapshot and lifecycle relevance

**Status:** decided  
**Rationale:** To decide whether assessment state is current, OpenSpec needs more than pass/fail. Persisted records should include assessment kind (`spec`, `diff`, `cleave`), target change, outcome (`pass`, `reopen`, `ambiguous`), timestamp, implementation snapshot signal (such as git HEAD and/or changed-file fingerprint), and any reconciliation hints (file-scope drift, new constraints, recommended `reconcile_after_assess`). That gives archive and verify a reliable basis for gating.

## Research Context

### Why this hardening is needed

Bridging `/assess` solves command reachability, but the lifecycle remains soft if `/opsx:verify`, `/opsx:archive`, and follow-up reconciliation still depend on prose conventions or operator memory. OpenSpec should be able to consume structured assessment outcomes directly so pass/reopen/ambiguous states are explicit and machine-actionable.

### Desired workflow shape

The hardened workflow should look like: implement → structured assessment for the change → persisted OpenSpec assessment record → verify/reconcile consume that state → archive gates on explicit current pass state. Cleave review and diff review should participate in the same model when they reopen work or alter file scope or constraints.

### OpenSpec-first architecture implication

If OpenSpec is the lifecycle authority, then assessment should be treated as an OpenSpec artifact class rather than an external note. Design-tree, cleave, and command bridging may produce or consume assessment results, but the authoritative persisted state for workflow gating should live with the OpenSpec change so every workflow component reads from the same source of truth.

### Recommended v1 artifact shape

A practical v1 shape is an assessment record file under each change directory:

`openspec/changes/<name>/assessment.json`

This file stores the latest relevant structured assessment plus snapshot metadata. OpenSpec commands can update and read this file directly. Later versions could add a history log, but v1 only needs a durable latest-known-state artifact for gating and reconciliation.

### Persisted record shape

A practical persisted record shape is:

```json
{
  "changeName": "agent-assess-tooling-access",
  "assessmentKind": "spec",
  "outcome": "pass",
  "timestamp": "2026-03-09T14:32:00.000Z",
  "snapshot": {
    "gitHead": "78a4c60",
    "fingerprint": "..."
  },
  "reconciliation": {
    "reopen": false,
    "changedFiles": [],
    "constraints": [],
    "recommendedAction": null
  }
}
```

The exact snapshot signal can evolve, but it must let OpenSpec determine whether the persisted assessment is current for the implementation being verified or archived.

## File Changes

- `extensions/openspec/spec.ts` (modified) — Helpers for reading and writing per-change assessment artifacts and computing stale/current state against implementation snapshot
- `extensions/openspec/index.ts` (modified) — Make verify execute or refresh structured assessment, persist assessment records, and enforce archive gates from assessment state
- `extensions/cleave/assessment.ts` (modified) — Normalize lifecycle-oriented assessment record schema and outcome vocabulary
- `extensions/cleave/index.ts` (modified) — Ensure structured assess results include change name, outcome, snapshot, and reconciliation hints in a form OpenSpec can persist directly
- `extensions/lib/slash-command-bridge.ts` (modified) — Preserve structured command result metadata needed by lifecycle consumers
- `docs/openspec-assess-lifecycle-integration.md` (modified) — Document OpenSpec-owned assessment artifacts, verify behavior, and archive fail-closed policy
- `openspec/changes/*/tasks.md` (modified) — Reflect assessment and reconciliation checkpoints more explicitly in lifecycle guidance where appropriate
- `openspec/changes/*/assessment.json` (new) — Per-change durable latest assessment artifact used by verify/archive gating
- `extensions/openspec/spec.test.ts` (modified) — Cover assessment artifact persistence, per-change scoping, and snapshot freshness checks
- `extensions/openspec/lifecycle-integration.test.ts` (new) — Verify refresh/reuse behavior and archive refusal/success gates at the command layer
- `extensions/lib/slash-command-bridge.test.ts` (modified) — Prove bridged command envelopes preserve nested lifecycle assessment metadata

## Constraints

- OpenSpec-owned assessment state is authoritative for lifecycle gating even if produced by assess or cleave tooling.
- Persisted assessment records must include change name, assessment kind, outcome, timestamp, snapshot identity, and reconciliation hints.
- `/opsx:verify` must execute or refresh assessment for the current implementation snapshot rather than trusting stale cached output.
- Archive must fail closed unless the latest relevant assessment for the current snapshot is an explicit pass.
- Verification and archive flows must not require parsing prior human-readable terminal output.
- Reconciliation hooks must preserve the existing operator UX while enabling autonomous lifecycle progression in the harness.
