+++
id = "9be41e7a-768a-4d4f-bc61-25fc5065052b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard and lifecycle publisher consolidation — Design

## Spec-Derived Architecture

### dashboard/publishers

- **OpenSpec dashboard publication uses shared refresh helpers** (added) — 2 scenarios
- **Design-tree dashboard publication uses shared refresh helpers** (added) — 2 scenarios
- **Shared publisher helpers remain incremental and extension-local** (added) — 2 scenarios

## Scope

Consolidate repeated dashboard/lifecycle publication boilerplate by introducing shared refresh helpers for OpenSpec and design-tree publisher paths. The helpers should own dashboard-facing recomputation/event emission at mutation boundaries so callers stop repeating `emit*State(...)` orchestration inline. This slice is intentionally incremental: it does not redesign dashboard rendering or refactor unrelated domain logic.

## File Changes

- `extensions/openspec/dashboard-state.ts` and/or a new adjacent refresh helper module — define reusable OpenSpec dashboard refresh helper(s)
- `extensions/openspec/index.ts` — switch mutation paths to the shared OpenSpec refresh helper
- `extensions/design-tree/dashboard-state.ts` and/or a new adjacent refresh helper module — define reusable design-tree dashboard refresh helper(s)
- `extensions/design-tree/index.ts` — switch mutation paths to the shared design-tree refresh helper
- `extensions/openspec/*.test.ts` — regression coverage for consolidated OpenSpec publisher refresh behavior
- `extensions/design-tree/*.test.ts` — regression coverage for consolidated design-tree refresh behavior
- `docs/dashboard-lifecycle-publisher-consolidation.md` — keep the design node synchronized with final file scope and constraints
