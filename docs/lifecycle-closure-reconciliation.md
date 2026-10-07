---
id: "lifecycle-closure-reconciliation"
title: "Lifecycle closure reconciliation"
status: exploring
tags:
  - "architecture"
  - "lifecycle"
dependencies: []
related:
  - "unified-model-request-contract"
open_questions:
  - "Which supported owner can idempotently reconcile late evidence, native lifecycle state, archive content, and baseline deltas?"
branches: []
---

# Lifecycle closure reconciliation

## Overview

How should accepted, merged work close when completed artifacts predate native
evidence registration and available archive paths update different records?
The [request-contract acceptance record](unified-model-request-contract.md) and
[quiet-inline verification](../openspec/changes/quiet-inline-replay/verification.md)
expose this gap. Both implementations are accepted and merged; combined-main
runtime evidence is tracked separately in the inline verification record.
Native administrative closure remains pending despite user authorization to
close. This exploring concern has no implementation plan or OpenSpec change.

## Research

### Observed closure boundaries — 2026-10-07

Source evidence is pinned to merged revision
`024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48`. The three cited native source files
are byte-identical at combined main `77996814af7720a45f15fd482ac8579093ebdd6f`:

- Native [`register_test_file`](https://github.com/styrene-lab/omegon/blob/024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48/core/crates/omegon/src/features/lifecycle.rs#L2108-L2151)
  accepts `planned`, `testing`, or `implementing`, but rejects `verifying`.
  Late registration therefore needs a supported reconciliation path when all
  artifact tasks are already complete.
- Native [`archive`](https://github.com/styrene-lab/omegon/blob/024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48/core/crates/omegon/src/features/lifecycle.rs#L2153-L2194)
  requires `verifying` and calls the lifecycle transaction. Its
  [`content operation`](https://github.com/styrene-lab/omegon/blob/024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48/core/crates/omegon-opsx/src/archive.rs#L162-L188)
  moves the change into an undated archive directory without merging baseline
  deltas. The [`state operation`](https://github.com/styrene-lab/omegon/blob/024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48/core/crates/omegon-opsx/src/fsm.rs#L533-L598)
  persists the archived transition and provides rollback on save failure.
- The standalone Python OpenSpec skill's `scripts/openspec.py`, inspected on
  2026-10-07, has different semantics. `cmd_archive` writes planned baseline
  merges and moves the change into a dated archive, without native ledger
  reconciliation. Its read-only `archive --check` result establishes only those
  standalone gates.

### Architecture reference and reconciliation invariant

The [LikeC4 model](../site/diagrams/likec4/context.c4) identifies
`omegon.host.lifecycle` in `hostComponents`. The relationship
`omegon.host.tools → omegon.host.lifecycle: Executes design and change operations`
locates the integration boundary. The [working agreement](../site/diagrams/likec4/WORKING-AGREEMENT.md)
defines the model's observed and accepted-intent boundaries.

Closure needs one supported, idempotent owner for evidence registration, design
and change state, archive movement, and baseline merging. Repeating closure after
interruption must converge without duplicate baseline requirements or false
completion. Existing test provenance must remain truthful; registering evidence
late must not claim that native registration preceded implementation.

Reopening completed tasks, manually changing guarded states, or invoking one
archive implementation as a substitute for the other would hide the mismatch.
The current factual-artifact fallback preserves acceptance evidence while leaving
native closure pending, as the repository OpenSpec skill permits.

## Open Questions

- Which supported owner can idempotently reconcile late evidence, native lifecycle state, archive content, and baseline deltas?
