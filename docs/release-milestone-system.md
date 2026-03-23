---
id: release-milestone-system
title: Release milestone and feature freeze system
status: decided
tags: [meta, process]
open_questions: []
jj_change_id: qxvkosqoopxxpptolvoxnzwmxktqypxz
---

# Release milestone and feature freeze system

## Overview

Map design nodes to release milestones (v0.15.0, v0.16.0, backlog). Support feature freezes where no new scope can be added to a milestone. Three options evaluated: (A) first-class milestone field on design nodes, (B) convention via tags, (C) separate release planning system. Recommended: start with tags (B), promote to first-class (A) if the pattern proves valuable.

## Research

### Relationship to OpenSpec lifecycle



## Decisions

### Decision: Tags + /milestone command, freeze enforced in implement action

**Status:** decided
**Rationale:** Zero schema changes. Tags already exist on design nodes. /milestone is a query+display layer. Freeze check is one conditional in design_tree_update(implement). Prove the workflow before promoting to first-class fields.

## Open Questions

*No open questions.*

## OpenSpec is per-change, milestones are per-release

OpenSpec tracks individual changes: propose → spec → tasks → implement → verify → archive. It has no concept of "this change is part of v0.15.0."

Milestones are a collection layer: they group design nodes (and their bound OpenSpec changes) into a release. The milestone answers "what ships together?" while OpenSpec answers "how is each piece built?"

## Natural integration points

1. **Design node tags → milestone membership.** A node tagged `v0.15.0` is in that milestone's scope. The milestone is the sum of its tagged nodes.

2. **OpenSpec change state → milestone readiness.** A milestone is "ready to release" when all its design nodes are `implemented` and all their OpenSpec changes are `archived` (verified + complete). If any change is still in-progress, the milestone isn't ready.

3. **Feature freeze → no new `implement` actions.** When a milestone is frozen, the system prevents `design_tree_update(implement)` on nodes tagged with that milestone. Existing in-progress changes can complete, but no new ones start.

4. **Dashboard view.** The dashboard could show a milestone progress bar: X/Y nodes implemented, Z OpenSpec changes archived. This replaces the current flat node list with a release-scoped view.

## Implementation options within existing paradigms

**Option B+ (tags + convention, OpenSpec-aware):**

Use tags for milestone membership: `tags: ["v0.15.0"]`. Add a `/milestone` command:
- `/milestone v0.15.0` — show all nodes tagged v0.15.0, their status, bound OpenSpec changes
- `/milestone v0.15.0 freeze` — add a `frozen` tag to all nodes, prevent new implement actions
- `/milestone v0.15.0 status` — readiness report (nodes decided/implemented, changes open/archived)

This requires zero schema changes. Tags are already on design nodes. The `/milestone` command is just a query + display layer. The freeze is enforced by checking tags in `design_tree_update(implement)`.

**Option A (first-class, later):**

If B+ proves useful, promote to:
- `milestone: "v0.15.0"` field on design nodes (replaces tag)
- `Milestone` struct in the design tree with status (open/frozen/released)
- Dashboard milestone progress bar
- Automated release notes from implemented nodes in a milestone

## What NOT to build

- Don't make milestones a separate system from the design tree. They're a view/query on the design tree, not a parallel hierarchy.
- Don't tie milestones to git tags or GitHub releases. That's the build/CI layer. Milestones are design-level planning.
- Don't require milestones. Most projects won't need them until they have 50+ design nodes and multiple release tracks.
