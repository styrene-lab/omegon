---
id: "unified-model-request-contract"
title: "Unified model request preparation contract"
status: implementing
tags:
  - "architecture"
  - "inference"
  - "session"
dependencies: []
related: []
open_questions:
  - "Does final review accept the implementation and scenario evidence for all three flows?"
  - "Does the parent architecture model need more detail for sessionless auxiliary inference?"
branches:
  - "feat/unified-model-request-preparation"
openspec_change: "unified-model-request-contract"
---

# Unified model request preparation contract

## Overview

Can Omegon consolidate model request preparation while preserving each flow's
authority, route binding, tool restrictions, limits, and cancellation ownership?

The user accepted a validated internal envelope over the existing route service
after PR #246 merged. The [OpenSpec change](../openspec/changes/unified-model-request-contract/proposal.md)
owns implementation scope and verification. Independent code review found no
blockers. The user approved PR creation; final lifecycle acceptance remains pending.
This copy evolved from the sibling diagram-worktree draft; that draft is unchanged.

## Research

### Observed request paths

Normal turns already capture authority-backed context and schema manifests.
Compaction has its own summary-request and authority protocol. Memory extraction
uses attributed session evidence but sends a sessionless, bounded provider request.
All three use existing provider-route infrastructure.

The [research ledger](../openspec/changes/unified-model-request-contract/design.md#evidence-ledger)
links source evidence for these boundaries. Repeated preparation warrants review;
the presence of separate collectors does not establish unwanted duplication.

### Architecture model references

The unlanded LikeC4 model at `site/diagrams/likec4/context.c4` in the sibling
diagram worktree is observed and simplified. Its working agreement is at
`site/diagrams/likec4/WORKING-AGREEMENT.md`. Stable references in view
`hostComponents` are:

- `omegon.host.orchestration → omegon.host.context`: Assembles the next turn's context.
- `omegon.host.orchestration → omegon.host.inference`: Requests streamed model responses.
- `omegon.host.context → omegon.host.memory`: Requests relevant retained knowledge.
- `omegon.host.inference → inferenceProvider`: Requests model inference, declared on the model and visible through broader views.

The model does not enumerate memory extraction's outbound inference request.
That simplification must not be read as a rule that only interactive orchestration
can request inference. Any accepted preparation boundary should be reconciled with
the model and linked specifications before implementation closure.

### Accepted preparation direction

One preparation contract makes common validity checks explicit while flow
adapters retain their own inputs and execution owners retain their own effects.
No-tools inference must remain no-tools. Source-session evidence must not grant
session authority. Existing route leases and immutable request evidence should
remain the primary contracts rather than being duplicated.

## Open Questions

- Does final review accept the implementation and scenario evidence for all three flows?
- Does the parent architecture model need more detail for sessionless auxiliary inference?
