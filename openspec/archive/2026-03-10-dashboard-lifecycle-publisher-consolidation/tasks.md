+++
id = "c838f2df-ce9c-4ac4-8a09-90962c22f9d1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard and lifecycle publisher consolidation — Tasks

## 1. Consolidate OpenSpec dashboard refresh plumbing
<!-- specs: dashboard/publishers -->

- [x] 1.1 Introduce a shared OpenSpec dashboard refresh helper around publisher/event emission
- [x] 1.2 Replace repeated inline refresh boilerplate in `extensions/openspec/index.ts` with the shared helper
- [x] 1.3 Add regression tests for OpenSpec publisher refresh consolidation

## 2. Consolidate design-tree dashboard refresh plumbing
<!-- specs: dashboard/publishers -->

- [x] 2.1 Introduce a shared design-tree dashboard refresh helper around publisher/event emission
- [x] 2.2 Replace repeated inline refresh boilerplate in `extensions/design-tree/index.ts` with the shared helper
- [x] 2.3 Add regression tests for focus-aware design-tree refresh consolidation

## 3. Validate the consolidation slice
<!-- specs: dashboard/publishers -->

- [x] 3.1 Run targeted OpenSpec and design-tree tests covering consolidated publisher paths
- [x] 3.2 Run `npm run typecheck`
