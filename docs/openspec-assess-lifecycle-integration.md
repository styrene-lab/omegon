---
id: openspec-assess-lifecycle-integration
title: OpenSpec lifecycle integration with structured assessment results
status: implementing
parent: agent-assess-tooling-access
tags: [openspec, assess, lifecycle, workflow, harness]
open_questions: []
branches: ["feature/openspec-assess-lifecycle-integration"]
openspec_change: openspec-assess-lifecycle-integration
---

# OpenSpec lifecycle integration with structured assessment results

## Overview

OpenSpec now owns the durable assessment state that gates lifecycle progression. Instead of depending on operator memory or human-readable `/assess` output, each active change persists its latest structured assessment in `openspec/changes/<change>/assessment.json`. Verify, reconciliation, and archive all consume that same record.

The result is a fail-closed lifecycle:

`implement → assess → persist assessment → reconcile → verify/archive`

## Research

### Why this hardening is needed

Bridging `/assess` solved command reachability, but it did not make lifecycle state authoritative. Before this change, `/opsx:verify` and `/opsx:archive` still relied on operator sequencing and prose interpretation. That left room for stale or ambiguous review outcomes to slip through.

### OpenSpec-first lifecycle model

OpenSpec is the workflow authority, so assessment state now lives beside the rest of the change artifacts:

- `proposal.md`
- `design.md`
- `tasks.md`
- `specs/**`
- `assessment.json`

That keeps lifecycle evidence co-located, inspectable, and scoped to one change.

## Persisted assessment artifact

### Location

Each active change stores its latest lifecycle-relevant assessment at:

`openspec/changes/<change-name>/assessment.json`

### Record shape

The persisted record captures both the outcome and the implementation snapshot it applies to.

```json
{
  "schemaVersion": 1,
  "changeName": "openspec-assess-lifecycle-integration",
  "assessmentKind": "spec",
  "outcome": "pass",
  "timestamp": "2026-03-09T16:12:00.000Z",
  "snapshot": {
    "gitHead": "78a4c60...",
    "fingerprint": "5f3d...",
    "dirty": false,
    "scopedPaths": [
      "extensions/openspec/index.ts",
      "docs/openspec-assess-lifecycle-integration.md"
    ],
    "files": []
  },
  "reconciliation": {
    "reopen": false,
    "changedFiles": [],
    "constraints": [],
    "recommendedAction": null
  }
}
```

### Snapshot semantics

The snapshot exists to answer one question: _does this assessment still describe the current implementation?_ OpenSpec computes freshness from:

- git HEAD
- working tree cleanliness
- a content fingerprint covering scoped implementation files and change artifacts

If any of those signals drift, the persisted assessment becomes stale.

## Verify behavior

### `/opsx:verify` is an active checkpoint

`/opsx:verify <change>` no longer acts like a passive reminder. It now evaluates persisted state against the current snapshot.

- If the persisted assessment is **current**, verify reuses it and shows an operator-facing summary.
- If the assessment is **missing** or **stale**, verify prompts a fresh `/assess spec <change>` run and instructs the harness to persist the resulting structured outcome.

### Refresh flow

The intended loop is:

1. Run `/opsx:verify <change>`
2. If verify reports stale or missing state, run `/assess spec <change>`
3. Persist the structured result through `openspec_manage` reconciliation
4. Re-run `/opsx:verify` or proceed to archive when the state is current and passing

## Reconciliation behavior

`openspec_manage` action `reconcile_after_assess` now serves two roles:

1. apply lifecycle reconciliation effects such as reopening work or appending constraints
2. persist the structured assessment record for the change snapshot

That means reconciliation is no longer just a prose-follow-up step. It also refreshes the authoritative lifecycle artifact used by later gates.

## Archive gate behavior

### Fail-closed policy

`/opsx:archive` and the `openspec_manage archive` action now refuse to continue when the latest relevant assessment is:

- missing
- stale for the current snapshot
- `ambiguous`
- `reopen`

Archive only proceeds when the latest persisted assessment for the current implementation snapshot explicitly reports `pass`.

### Operator-facing UX

The gate still explains itself in human terms. Refusals include a readable reason plus the persisted assessment summary when one exists. The workflow stays understandable for operators while remaining machine-actionable for the harness.

## Representative workflow

### Happy path

1. Implement the change
2. Run `/assess spec <change>`
3. Call `openspec_manage` with `action: reconcile_after_assess`, the assessment kind/outcome, and any follow-up hints
4. Run `/opsx:verify <change>` to confirm the persisted record is current
5. Run `/opsx:archive <change>`

### Reopened work

1. `/assess spec <change>` reports remaining issues
2. Reconciliation persists outcome `reopen`
3. OpenSpec reopens lifecycle state and archive refuses to proceed
4. Finish the work, reassess, and persist a fresh `pass`

## Validation coverage

Regression coverage for this integration now checks:

- per-change `assessment.json` persistence and scoping
- snapshot freshness detection for current vs stale assessment records
- bridged slash-command metadata preservation for lifecycle fields
- `/opsx:verify` reuse of current state vs refresh prompting for stale state
- `/opsx:archive` refusal on missing, stale, ambiguous, and reopened assessment state
- `/opsx:archive` success on a current explicit pass

## Decisions

### Decision: OpenSpec is the lifecycle authority and must own persisted assessment state

**Status:** decided  
**Rationale:** Lifecycle gates need one source of truth. Putting assessment state inside each change keeps verify, reconciliation, and archive aligned.

### Decision: `/opsx:verify` should execute or refresh structured assessment, not only render cached state

**Status:** decided  
**Rationale:** Verification is meaningful only if it is current for the implementation snapshot being evaluated.

### Decision: Archive must fail closed on missing, stale, ambiguous, or reopened assessment state

**Status:** decided  
**Rationale:** Archive is the lifecycle commit point. It must reject uncertain or outdated state instead of relying on best effort.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/spec.ts` — persisted assessment artifact helpers and freshness evaluation
- `extensions/openspec/index.ts` — verify/archive lifecycle gates and reconciliation persistence
- `extensions/cleave/index.ts` — structured assessment payloads for OpenSpec persistence
- `extensions/lib/slash-command-bridge.ts` — preservation of structured assessment metadata
- `extensions/openspec/spec.test.ts` — artifact persistence and freshness regression tests
- `extensions/openspec/lifecycle-integration.test.ts` — verify/archive lifecycle command regression tests
- `extensions/lib/slash-command-bridge.test.ts` — structured bridge metadata regression test

### Constraints

- Archive must fail closed unless the latest persisted assessment is a current explicit `pass`.
- Lifecycle gating must not parse prior human-readable assessment text.
- Persisted assessment state must remain scoped to one OpenSpec change.
- Operator UX should remain readable even though lifecycle decisions are now driven by structured state.
