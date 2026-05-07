+++
id = "6fdb47c6-5b0e-491a-a2f8-e33d7da44494"
kind = "document"
title = "Repo Consolidation, Security Hardening, and Lifecycle Normalization"
status = "implemented"
tags = ["architecture", "security", "lifecycle", "consolidation", "Omegon"]
aliases = ["repo-consolidation-hardening"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Repo Consolidation, Security Hardening, and Lifecycle Normalization

## Overview

Reduce internal sprawl across major extensions, tighten process safety around subprocess and shell usage, normalize lifecycle state across design-tree/OpenSpec/dashboard, and improve presentation/data-model coherence for Omegon as it matures into a platform.

## Research

### Repo-wide assessment findings

Top opportunities: (1) break up oversized extension entrypoints such as project-memory/index.ts, cleave/index.ts, openspec/index.ts, and design-tree/index.ts into thinner registration files over explicit domain/store/ui/bridge layers; (2) consolidate repeated dashboard and lifecycle-emitter plumbing into shared publishers; (3) harden subprocess management by replacing broad pkill patterns and shell-string execution; (4) normalize lifecycle state so design-tree, OpenSpec, dashboard, and memory derive from one canonical resolver; (5) unify model-control responsibilities currently split across effort, model-budget, offline-driver, local-inference, and lib/model-routing.

### Program wrap-up

The initially identified highest-leverage consolidation and hardening slices are now implemented as bounded child efforts: cleave checkpoint parity / volatile hygiene, localhost web UI hosting, subprocess safety hardening, lifecycle state normalization, and dashboard publisher consolidation. The remaining ideas from the original assessment — especially oversized entrypoint decomposition and model-control consolidation — remain valid future work, but they are no longer part of the current hardening wrap-up.

## Decisions

### Decision: Conclude repo-consolidation-hardening after the bounded child slices

**Status:** decided
**Rationale:** The concrete risk-reduction and consistency slices identified at the start of the effort have been delivered as separate archived changes. Remaining opportunities are broader architectural improvements that should be tracked as future initiatives rather than keeping this umbrella effort open indefinitely.

### Decision: Completed Design→OpenSpec→Cleave slices should not retain dedicated feature branches

**Status:** decided
**Rationale:** Once a slice has progressed from design through OpenSpec-backed implementation and its lifecycle is complete, keeping its dedicated feature branch open adds noise without preserving additional lifecycle value. Any later bug fix or enhancement should be tracked as a new change rather than by reusing the old branch.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `docs/cleave-checkpoint-parity.md` (modified) — Implemented child slice for dirty-tree/checkpoint parity and volatile memory hygiene.
- `docs/pikit-web-ui-hosting.md` (modified) — Implemented localhost-only, read-only web UI child slice.
- `docs/subprocess-safety-hardening.md` (modified) — Implemented subprocess/process safety child slice.
- `docs/lifecycle-state-normalization.md` (modified) — Implemented canonical lifecycle resolver adoption child slice.
- `docs/dashboard-lifecycle-publisher-consolidation.md` (modified) — Implemented dashboard publisher consolidation child slice.

### Constraints

- Treat future oversized entrypoint decomposition and model-control consolidation as separate follow-on initiatives rather than extending this closed umbrella slice.
- Prefer bounded, spec-backed child slices for future architecture hardening instead of one large umbrella implementation change.
