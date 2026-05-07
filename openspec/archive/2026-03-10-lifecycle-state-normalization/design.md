+++
id = "46dd0c52-8e2e-431a-bfcf-c7edde14a418"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle state normalization — Design

## Spec-Derived Architecture

### lifecycle/resolver

- **Canonical lifecycle resolver produces shared change summaries** (added) — 2 scenarios
- **OpenSpec status surfaces consume the canonical lifecycle resolver** (added) — 2 scenarios
- **Dashboard and design-tree bindings consume canonical lifecycle state incrementally** (added) — 2 scenarios

## Scope

Introduce a canonical lifecycle resolver module that computes normalized change summaries from OpenSpec artifacts, assessment state, and design-tree binding information. Adopt that resolver first in OpenSpec status/archive-readiness paths and then in dashboard/design-tree lifecycle surfaces so shared truth about stage, verification substate, archive readiness, and binding status comes from one code path. This slice is incremental and does not attempt to centralize every mutable state update in one pass.

## File Changes

- `extensions/openspec/spec.ts` or a new adjacent resolver module — define the canonical lifecycle summary shape and computation helpers
- `extensions/openspec/index.ts` — switch status/get/archive-readiness paths to the shared lifecycle resolver
- `extensions/openspec/dashboard-state.ts` — publish dashboard-facing OpenSpec lifecycle state from the canonical resolver
- `extensions/design-tree/index.ts` and/or `extensions/design-tree/dashboard-state.ts` — align bound-to-OpenSpec lifecycle metadata with the canonical resolver
- `extensions/openspec/*.test.ts` — regression coverage for shared lifecycle summary, verification substates, and archive readiness
- `extensions/design-tree/*.test.ts` — regression coverage for binding-truth normalization where needed
- `docs/lifecycle-state-normalization.md` — keep the design node synchronized with implementation notes and constraints discovered during the slice
