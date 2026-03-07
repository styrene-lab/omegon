# Design Tree Lifecycle: Implementation Tracking + Branch Binding

## Intent

Extend the design tree node lifecycle beyond `decided` to track implementation state and bind nodes to git feature branches. Currently the lifecycle ends at `decided` — there's no visibility into whether a decided node has been implemented, is in-progress, or which branch carries the work. This creates a gap between design decisions and their realization.

**Current lifecycle:** `seed → exploring → decided` (terminal states: `blocked`, `deferred`)

**Proposed lifecycle:** `seed → exploring → decided → implementing → implemented`

**Branch binding:** Each node that enters `implementing` is associated with a git feature branch (derived from node ID or explicitly set). This creates a traceable link: design node → branch → OpenSpec change → cleave execution.

**Motivation:** Past features like `skill-aware-dispatch` and `scenario-first-task-generation` naturally mapped to both design nodes AND feature branches AND OpenSpec changes. The binding should be explicit and visible in the dashboard.
