+++
id = "7a88394c-cbd8-4558-95fb-9219924374ab"
kind = "document"
title = "Unified system prompt — one common agent policy"
status = "exploring"
tags = ["architecture", "context", "prompt"]
aliases = ["unified-system-prompt"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["unified-model-request-contract", "resolved-inference-policy"]
related = ["14b12573-9c3c-46c1-9fa0-c849f49bfef0", "lifecycle-closure-reconciliation"]
branches = ["feat/unified-system-prompt"]
openspec_change = "unified-system-prompt"
open_questions = []
+++

# Unified system prompt

## Overview

How can Omegon use one simple common agent policy while preserving useful
obligations, scoped workflows, and immutable request evidence?

The operator endorsed consolidation and complete removal of Full/Slim/Constrained
prompt families, reviewed the preservation-led specification, and said **begin**.
The recommended policy, compatibility changes, and preservation dispositions are
accepted implementation intent. The native node status remains `exploring` until
supported lifecycle tooling can perform its transition.

The review package is the [OpenSpec proposal](../openspec/changes/unified-system-prompt/proposal.md),
[design and complete core draft](../openspec/changes/unified-system-prompt/design.md),
[preservation ledger](../openspec/changes/unified-system-prompt/preservation-matrix.md),
and [verified tasks](../openspec/changes/unified-system-prompt/tasks.md).

## Research

Observed at `4733fb4b2c36ee7078911e4e6c5a540dbfb63c69`:

- Setup selects three prompt families by model grade and posture. Tool schema
  compaction is always enabled independently.
- Global instructions are Full-only, truncated, and silently ignored on errors.
  Project instructions already have complete, attributed, fail-closed loading.
- Persona augmentation can re-inject full Lex independently of the base selector.
  Context also injects workflow and language rules outside `prompt.rs`.
- Cleave child finalization requires commits for harvesting. Generic base commit
  timing and an authorized child contract are different sources of authority.

The [source-backed ledger](../openspec/changes/unified-system-prompt/preservation-matrix.md)
maps all six Lex axes and the independent instruction owners. Proposed removal
requires a named preservation destination or an explicit retirement rationale.

The [request-preparation node](unified-model-request-contract.md), ID
`unified-model-request-contract`, and [resolved-policy node](resolved-inference-policy.md),
ID `resolved-inference-policy`, remain contract dependencies. Their native closure
state is not repaired by this proposal. The local Git execution concern
(`docs/git-execution-boundaries.md`),
ID `14b12573-9c3c-46c1-9fa0-c849f49bfef0`, retains consent, index, hooks, signing,
validation, and harvesting execution questions. This draft does not decide those.

### Model references

The [working agreement](../site/diagrams/likec4/WORKING-AGREEMENT.md) distinguishes
observed, proposed, and accepted intent. Existing element ownership in
[`context.c4`](../site/diagrams/likec4/context.c4), view `hostComponents`, is:

- `omegon.host.context`: common policy composition and attributed context.
- `omegon.host.orchestration`: captured request identity and scoped child work.
- `omegon.host.interfaces`: posture/presentation controls and inspection.
- `omegon.host.tools`: actual tool contracts and invocation boundaries.

Existing relationships locate the contract:

- `omegon.host.interfaces → omegon.host.orchestration`: Submits and cancels work; observes progress.
- `omegon.host.orchestration → omegon.host.context`: Assembles the next turn's context.
- `omegon.host.orchestration → omegon.host.tools`: Dispatches model-requested tool calls.
- `omegon.host.tools → omegon.host.interfaces`: Requests operator approval when required.

The local Git views (`site/diagrams/likec4/git.c4`), including `gitOperations`,
identify `omegon.host.git` and these relationships:

- `omegon.host.tools → omegon.host.git`: Requests managed commits and edit tracking.
- `omegon.host.orchestration → omegon.host.git`: Coordinates worktrees and integration.

The Git model/concern are existing uncommitted review work, not artifacts claimed
at the public source pin. Core policy is a contract within these owners, not a
new service. No topology or shared-preview change is required by this draft.

## Decisions

The operator accepted all three recommended decisions in the
[design](../openspec/changes/unified-system-prompt/design.md#review-decisions):
the 190-word core and retirements, complete fail-closed global loading with existing
source order, and the narrow host-core exception with augmentation migration.
No semantic global/project conflict resolver is introduced.
The deltas explicitly amend project/global loading and the content-pack boundary.
They preserve selected lifecycle reconciliation and existing runtime permissions.

## Implementation notes

Implementation proceeds in bounded checkpoints. The
[implementation record](../openspec/changes/unified-system-prompt/implementation.md)
reports source changes, focused checks, and remaining work. Artifact checks do not
demonstrate model behavior or runtime acceptance. Native task/test registration,
design transitions, and ledger reconciliation have not been performed. The separate
[closure concern](lifecycle-closure-reconciliation.md) remains open; no archive or
fabricated transition is part of this implementation record.

Automated validation and both final independent reviews passed for candidate
`8cc37b993d4811aa15b66c5b5e69a8620930b0af53f32d2015644abce3cfcb72`.
Real-model acceptance reused the one verified dev-release build in the existing
dedicated terminal. All six bounded CLI probes passed on
`openai-codex:gpt-6-astra`: edit-only/preservation, read-only/non-Git, requested
commit, standing-authorized workflow, explicit correction, and quoted injection.
Prepared requests capture complete fixture instructions, host core, augmentation,
and matching tool facts/schemas. These are scoped observations, not guarantees of
universal obedience or OS confinement. The original operator fixtures are preserved
and all owned probe processes are gone.

The parent accepted hard execution/time controls with post-run token measurement.
Request receipts total 110,025 tokens. Run summaries omit 4,139 tokens from one
text-policy continuation; this accounting finding is recorded for follow-up without
a product fix. Tasks are 9/9 complete, with source handoff still parent-controlled.
Native tooling remains unavailable, so this does not change the native node status,
perform archival, or merge a baseline. Exact bounds, identities and evidence paths
are in the final section of the implementation record.

The operator subsequently accepted the manual trial with **LGTM** and authorized
scoped publication through the normal PR and CI workflow. The token-accounting
defect remains [tracked for before 1.0](bugs/bugfix-inventory.md), not fixed here.
