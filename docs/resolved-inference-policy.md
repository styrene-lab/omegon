---
id: "resolved-inference-policy"
title: "Resolved inference route policy"
status: implementing
tags:
  - "architecture"
  - "inference"
  - "context"
dependencies:
  - "unified-model-request-contract"
related:
  - "provider-policy-and-route-preference"
  - "lifecycle-closure-reconciliation"
open_questions: []
branches:
  - "feat/resolved-inference-policy"
openspec_change: "resolved-inference-policy"
---

# Resolved inference route policy

## Overview

How should one resolved route policy keep provider facts, operator intent, request
preparation, and semantic UI consistent?

The user approved end-to-end implementation on 2026-10-08, accepting all three
policy decisions below. Implementation is in progress. The linked [proposal](../openspec/changes/resolved-inference-policy/proposal.md),
[design](../openspec/changes/resolved-inference-policy/design.md),
[delta requirements](../openspec/changes/resolved-inference-policy/specs/inference/route-policy.md),
and [implementation tasks](../openspec/changes/resolved-inference-policy/tasks.md)
define the accepted implementation direction.

## Research

### Observed mismatch and proposed invariant

At `6b276f3b8fc7e610ff61dab961d3e21778114c00`, the Codex Astra registry entry
stores 872,000 input tokens while its description distinguishes a 272,000 upstream
default. Settings use the former as the context window. The TUI can calculate a
percentage against an assembly window and later display a registry-window
denominator. Portable `Minimal` is displayed while the provider sends `low`.

The [evidence ledger](../openspec/changes/resolved-inference-policy/design.md#evidence-ledger)
records local sources and immutable peer references. Peer clients provide
comparison evidence, not policy authority or proof of account entitlement.

The accepted invariant is one immutable, route-bound policy snapshot for each
prepared request. Context assembly and semantic projections consume that policy.
Provider facts, operator intent, and effective request values retain separate
provenance.

### Architecture ownership

The [observed LikeC4 model](../site/diagrams/likec4/context.c4) and its
[working agreement](../site/diagrams/likec4/WORKING-AGREEMENT.md) locate the work
within existing owners in `hostComponents`:

- `omegon.host.inference`: resolve route facts and effective inference settings
  through the existing gateway and `PreparedModelRequest` boundary.
- `omegon.host.context`: assemble and account for provider-visible input and reserves.
- `omegon.host.orchestration`: retain request identity, captured generations, and flow policy.
- `omegon.host.interfaces`: consume semantic projections and expose inspection.

Stable relationships are
`omegon.host.orchestration → omegon.host.context: Assembles the next turn's context`,
`omegon.host.orchestration → omegon.host.inference: Requests streamed model responses`,
and `omegon.host.interfaces → omegon.host.orchestration: Submits and cancels work; observes progress`.
Bounded auxiliary work follows the existing
`omegon.host.memory → omegon.host.inference: Requests bounded memory extraction` relationship.

This is an accepted contract refinement within these owners. It introduces no
service/component box or observed topology change. The
[accepted request-preparation node](unified-model-request-contract.md) remains
the authority for the shared envelope and flow boundaries.

### Review mapping and lifecycle boundary

The `inference/route-policy` delta owns new policy behavior. Task groups 1–3 own
facts and capacity, reasoning, and request capture respectively.
The `inference/policy-projection` delta and task group 4 own semantic presentation.
Task numbers `1.1` through `4.3` are stable references within this change.

OpenSpec checkboxes record validated implementation progress. Native task/test
registration and ledger reconciliation are unavailable in this harness and have
not been performed. This node is not archived or implementation-complete.
The existing [closure concern](lifecycle-closure-reconciliation.md) remains separate.

## Accepted Decisions

- Use route-advertised default capacity. Enlargement requires an explicit numeric route-scoped target.
- Reject unsupported Off with needs-resolution feedback. Preserve saved profile intent.
- Retain existing heuristic reserves initially, with explicit accounting and named thresholds.
