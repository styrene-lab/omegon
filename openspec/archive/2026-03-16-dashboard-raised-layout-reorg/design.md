+++
id = "4aee70c4-4737-407d-9a30-f8c5f06e9d9b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Raised dashboard layout reorganization — design-dominant workspace and contextual implementation rail — Design

## Architecture Decisions

### Decision: Wide raised mode should be organized as header + primary workspace + secondary rail + telemetry

**Status:** decided
**Rationale:** The current equal design/implementation split encodes a symmetry that is not present in real usage. Branch/workspace identity belongs in a full-width header; Design Tree should own the primary workspace; implementation/OpenSpec/Cleave should appear contextually in a secondary rail or docked work section when active; telemetry remains a separate lower zone.

### Decision: Implementation should dock contextually instead of permanently owning half the raised body

**Status:** decided
**Rationale:** OpenSpec/implementation is often empty for long stretches, which is healthy workflow-wise. Reserving half the body for it produces dead air and constrains Design Tree unnecessarily. The layout should expand Design Tree by default and only allocate larger implementation presentation when there is active work to show.

## File Changes

- `extensions/dashboard/footer.ts` (modified) — Refactor raised body composition for full-width header, design-dominant main column, contextual side rail, and docked implementation section.
- `extensions/dashboard/footer-raised.test.ts` (modified) — Update raised-layout expectations and add coverage for the new structural organization.

## Constraints

- Prioritize structural relocation before detailed telemetry polish.
- Keep compact mode behavior stable unless required by shared helpers.
- Responsive tiers should preserve the same semantic reading order even as columns collapse.
