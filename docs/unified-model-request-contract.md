---
id: "unified-model-request-contract"
title: "Unified model request preparation contract"
status: implementing
tags:
  - "architecture"
  - "inference"
  - "session"
dependencies: []
related:
  - "lifecycle-closure-reconciliation"
open_questions: []
branches:
  - "feat/unified-model-request-preparation"
openspec_change: "unified-model-request-contract"
---

# Unified model request preparation contract

## Overview

Omegon now shares model request preparation while preserving each flow's authority,
route binding, tool restrictions, limits, and cancellation ownership.

The user accepted a validated internal envelope over the existing route service
for normal turns, compaction, and bounded auxiliary inference. Independent code
review found no blockers, and the implementation and its tests merged in
[PR #247](https://github.com/styrene-lab/omegon/pull/247) at `13760f25`.
The [OpenSpec change](../openspec/changes/unified-model-request-contract/proposal.md)
owns implementation scope and verification.

As of 2026-10-07, subsequent user authorization covers landing and lifecycle and
architecture reconciliation. Implementation is accepted. The structured status
remains `implementing` because native administrative closure is pending, not
because implementation tasks remain. All 12 tasks are checked. The change is
not archived; the [closure concern](lifecycle-closure-reconciliation.md) records
the unsupported reconciliation boundary.

## Research

### Observed request paths

Normal turns already capture authority-backed context and schema manifests.
Compaction has its own summary-request and authority protocol. Memory extraction
uses attributed session evidence but sends a sessionless, bounded provider request.
All three use existing provider-route infrastructure.

The historical [research ledger](../openspec/changes/unified-model-request-contract/design.md#evidence-ledger)
links pre-implementation source evidence for these boundaries. The accepted
envelope shares preparation checks while retaining separate collectors.

### Architecture model references

The [LikeC4 model](../site/diagrams/likec4/context.c4) is observed and simplified.
Its [working agreement](../site/diagrams/likec4/WORKING-AGREEMENT.md) defines the
evidence and shared-review boundaries. Stable references in view `hostComponents` are:

- `omegon.host.orchestration → omegon.host.context`: Assembles the next turn's context.
- `omegon.host.orchestration → omegon.host.inference`: Requests streamed model responses.
- `omegon.host.context → omegon.host.memory`: Requests relevant retained knowledge.
- `omegon.host.memory → omegon.host.inference`: Requests bounded memory extraction.
- `omegon.host.inference → inferenceProvider`: Requests model inference, declared on the model and visible through broader views.

The model includes bounded memory extraction at `context.c4:157`.
These fully qualified references remain stable across checkout consolidation.
Immutable implementation evidence is
[`model_request.rs`](https://github.com/styrene-lab/omegon/blob/024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48/core/crates/omegon/src/model_request.rs)
and [`formation.rs`](https://github.com/styrene-lab/omegon/blob/024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48/core/crates/omegon/src/features/memory/formation.rs).
The accepted contract spans flow adapters within existing owners; it does not
require a new model component. Artifact integration preserves model source and
manual geometry; it does not establish new rendered-view verification.

### Accepted preparation direction

One preparation contract makes common validity checks explicit while flow
adapters retain their own inputs and execution owners retain their own effects.
No-tools inference must remain no-tools. Source-session evidence must not grant
session authority. Existing route leases and immutable request evidence should
remain the primary contracts rather than being duplicated.

### Acceptance evidence — 2026-10-07

The [verification record](../openspec/changes/unified-model-request-contract/verification.md)
preserves the original scenario tests and review. It also records successful
composed regression evidence at provider-repair head `a08d526`, after CI repair
PR #248 and before provider-repair PR #250 merged at `024a8616`. This later
evidence is distinct from the original request-contract test head.
