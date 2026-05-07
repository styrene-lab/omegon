+++
id = "3fa61035-feb7-4f46-a60d-0acad77e0094"
kind = "document"
title = "Raised dashboard layout reorganization — design-dominant workspace and contextual implementation rail"
status = "implemented"
tags = ["dashboard", "footer", "ux", "layout", "responsive"]
aliases = ["dashboard-raised-layout-reorg"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
issue_type = "feature"
open_questions = []
openspec_change = "dashboard-raised-layout-reorg"
parent = "unified-dashboard"
priority = "2"
+++

# Raised dashboard layout reorganization — design-dominant workspace and contextual implementation rail

## Overview

Reorganize raised dashboard structure around operator attention instead of a fixed design-vs-implementation 50/50 split. Wide/full-screen mode should use a full-width workspace header, a design-dominant primary area, a contextual secondary rail, and the existing lower telemetry zone. Implementation/OpenSpec should dock contextually when active rather than permanently owning half the body.

## Decisions

### Decision: Wide raised mode should be organized as header + primary workspace + secondary rail + telemetry

**Status:** decided
**Rationale:** The current equal design/implementation split encodes a symmetry that is not present in real usage. Branch/workspace identity belongs in a full-width header; Design Tree should own the primary workspace; implementation/OpenSpec/Cleave should appear contextually in a secondary rail or docked work section when active; telemetry remains a separate lower zone.

### Decision: Implementation should dock contextually instead of permanently owning half the raised body

**Status:** decided
**Rationale:** OpenSpec/implementation is often empty for long stretches, which is healthy workflow-wise. Reserving half the body for it produces dead air and constrains Design Tree unnecessarily. The layout should expand Design Tree by default and only allocate larger implementation presentation when there is active work to show.

### Decision: Lowered footer should become a runtime HUD while lifecycle/work surfaces move exclusively to raised mode

**Status:** decided
**Rationale:** The default persistent footer should optimize for ambient operating conditions: identity, branch, context pressure, model topology, memory, and runtime/system state. Design tree, implementation, cleave, and branch tree details are intentional workspace surfaces and should appear only when the dashboard is raised. This gives lowered and raised modes distinct jobs instead of forcing the compact footer to be a compressed dashboard.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/dashboard/footer.ts` (modified) — Refactor raised body composition for full-width header, design-dominant main column, contextual side rail, and docked implementation section.
- `extensions/dashboard/footer-raised.test.ts` (modified) — Update raised-layout expectations and add coverage for the new structural organization.
- `extensions/dashboard/footer-dashboard.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/dashboard/footer-compact.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/dashboard/git.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `package.json` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `bin/omegon.mjs` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `vendor/pi-mono` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `.pi/memory/facts.jsonl` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Prioritize structural relocation before detailed telemetry polish.
- Keep compact mode behavior stable unless required by shared helpers.
- Responsive tiers should preserve the same semantic reading order even as columns collapse.
- Compact mode is intentionally redefined as a runtime HUD rather than a compressed lifecycle summary.
- Lifecycle/work sections (design tree, implementation, cleave, additional branches) are hidden in lowered mode and reserved for raised mode.

## Acceptance Criteria

### Falsifiability

- This decision is wrong if: This design is wrong if wide raised mode still reads primarily as two equal left/right halves with implementation permanently reserved even when empty.
- This decision is wrong if: This design is wrong if branch/workspace identity still bleeds directly into Design Tree with no clear structural separation.
- This decision is wrong if: This design is wrong if responsive collapse changes semantic ordering rather than only changing grouping and density.
