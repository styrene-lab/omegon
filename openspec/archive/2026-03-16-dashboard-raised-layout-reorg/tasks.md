+++
id = "2029a565-7216-4562-ab8c-155fd4567f51"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Raised dashboard layout reorganization — design-dominant workspace and contextual implementation rail — Tasks

## 1. extensions/dashboard/footer.ts (modified)

- [x] 1.1 Refactor raised body composition for full-width header, design-dominant main column, contextual side rail, and docked implementation section.
- [x] 1.2 Collapse the contextual rail when implementation/recovery content is absent so Design Tree reclaims the body width.
- [x] 1.3 Always render a separator between the workspace header and the raised body, even when the branch tree fits on one line.

## 2. extensions/dashboard/footer-raised.test.ts (modified)

- [x] 2.1 Update raised-layout expectations and add coverage for the new structural organization.

## 3. Cross-cutting constraints

- [x] 3.1 Prioritize structural relocation before detailed telemetry polish.
- [x] 3.2 Redefine compact mode as a runtime HUD and reserve lifecycle/work sections for raised mode.
- [x] 3.3 Responsive tiers should preserve the same semantic reading order even as columns collapse.
- [x] 3.4 Wide/full-screen telemetry uses distinct context, models, memory, and system/recovery cards instead of merged appendices.
