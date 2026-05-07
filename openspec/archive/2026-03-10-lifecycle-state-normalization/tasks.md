+++
id = "758bdb57-e6be-4ecb-85e6-4a0469793137"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle state normalization — Tasks

## 1. Define the canonical lifecycle resolver
<!-- specs: lifecycle/resolver -->

- [x] 1.1 Define a normalized lifecycle summary shape with stage, verification substate, archive readiness, binding status, task counts, and assessment freshness
- [x] 1.2 Implement the canonical resolver in shared OpenSpec lifecycle logic
- [x] 1.3 Add regression tests for canonical lifecycle summary computation

## 2. Move OpenSpec status/archive surfaces onto the resolver
<!-- specs: lifecycle/resolver -->

- [x] 2.1 Refactor OpenSpec status and get-detail reporting to consume the canonical lifecycle resolver
- [x] 2.2 Refactor archive-readiness/gating paths to consume the same resolver outcome
- [x] 2.3 Add regression tests proving status/detail/archive surfaces agree on lifecycle truth

## 3. Align dashboard and design-tree lifecycle truth incrementally
<!-- specs: lifecycle/resolver -->

- [x] 3.1 Update dashboard-facing OpenSpec lifecycle publication to use the canonical resolver output
- [x] 3.2 Normalize design-tree bound-to-OpenSpec lifecycle metadata against the same resolver/binding rule set
- [x] 3.3 Add regression tests for dashboard/design-tree lifecycle agreement where shared fields overlap

## 4. Validate the lifecycle normalization slice
<!-- specs: lifecycle/resolver -->

- [x] 4.1 Run targeted OpenSpec and design-tree tests covering lifecycle normalization
- [x] 4.2 Run `npm run typecheck`
