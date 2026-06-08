---
id: acp-task-mutation-contract
title: "ACP task mutation contract"
status: exploring
tags: [acp, tasks, mutations, lifecycle, ecosystem]
parent: acp-task-durability-contract
related: [acp-ecosystem-capability-negotiation, acp-task-binding-store]
open_questions:
  - "[assumption] Mutation support must be per task/source, not global."
  - "Which operations can safely apply to OpenSpec-backed tasks without editing markdown directly?"
  - "Should evidence append and status completion share one revision precondition model?"
---

# ACP task mutation contract

## Overview

Define explicit mutation semantics for ACP task projections. `writable: true` is too coarse for external UIs. Each task must list supported operations, their durability, and their revision preconditions.

## Research

Flynt needs `supported_mutations` before enabling any mutation UI. Candidate mutations:

- `bind_external_ref`
- `set_status`
- `append_evidence`
- `complete`
- `reopen`
- `detach`

OpenSpec/design-backed tasks should generally expose fewer mutations than session tasks until write-through tools have stable identities and conflict handling.

## Decisions

### Advertise supported mutations per task

**Status:** proposed

**Rationale:** External clients should not infer allowed actions from plan source, status, or writable boolean alone.

### Require expected revision for mutations

**Status:** proposed

**Rationale:** External clients must not overwrite changes from another Omegon session, branch, or tool run.

## Open Questions

- [assumption] Mutation support must be per task/source, not global.
- Which operations can safely apply to OpenSpec-backed tasks without editing markdown directly?
- Should evidence append and status completion share one revision precondition model?

## Implementation Notes

Primary files:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/acp.rs`
- `core/crates/omegon/src/tools/mod.rs`

Implementation targets:

- Add `supported_mutations` to task ACP output.
- Add `expected_revision` to mutation requests.
- Return structured `not_writable`, `unsupported_source`, `stale_revision`, and `conflict` errors.
- Add tests that unsupported mutation UI inputs are rejected without source edits.
