+++
title = "Lifecycle Implement Service Pass"
tags = ["lifecycle","design-tree","openspec","mutation-service","planning"]
+++

+++
id = "00061e7c-098c-4ea9-9775-02c2eaf9afd3"
kind = "design_node"

[data]
title = "Lifecycle Implement Service Pass"
status = "decided"
issue_type = "feature"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

# Lifecycle Implement Service Pass

---
title: Lifecycle Implement Service Pass
status: decided
tags: [lifecycle, design-tree, openspec, mutation-service, planning]
---

# Lifecycle Implement Service Pass

## Overview

This node plans the next lifecycle extraction pass: moving `design_tree_update(implement)` out of the tool adapter and into a lifecycle-domain service boundary.

The current lifecycle surface-map pass extracted archive safety, OpenSpec sync, query policy, and most design-tree mutations into domain modules. `implement` is intentionally left out because it is not a simple frontmatter mutation. It crosses design-tree state, implementation notes, OpenSpec scaffolding, task generation, and lifecycle status transitions.

## Current state

Implemented and committed in the preceding pass:

- `lifecycle/archive.rs` owns OpenSpec archive transaction markers and recovery.
- `lifecycle/sync.rs` owns OpenSpec markdown-to-`omegon-opsx` synchronization.
- `lifecycle/query.rs` owns ready/blocked/frontier/children/dependency projection policy.
- `lifecycle/mutation.rs` owns most design-tree store mutations:
  - create
  - set status / archive metadata
  - add/remove question
  - add research
  - add decision
  - add dependency / related
  - add implementation notes
  - branch
  - set priority / issue type

Remaining adapter-side design-tree operations:

- `implement`
- `focus` / `unfocus`

`focus` and `unfocus` are session/provider state and can remain adapter-side. `implement` should move, but only as a dedicated pass.

## Why `implement` needs its own pass

`design_tree_update(implement)` is a workflow transition, not a narrow mutation. It owns or coordinates:

1. reading the design node and structured sections
2. validating readiness for implementation
3. using implementation notes and decisions to scaffold OpenSpec artifacts
4. creating or updating OpenSpec proposal/design/tasks/spec files
5. binding the design node to an OpenSpec change
6. setting the design node status to `implementing`
7. potentially creating branch metadata
8. refreshing provider state and synchronizing FSM state

Moving this wholesale without tests would hide behavior changes inside a refactor. The pass should first characterize current behavior, then move it behind a named service method.

## Proposed boundary

Add a method to `LifecycleMutationService`:

```rust
pub struct ImplementDesignNodeRequest {
    pub node_id: String,
    pub change_name: Option<String>,
    pub branch_name: Option<String>,
}

pub struct ImplementDesignNodeResult {
    pub node_id: String,
    pub openspec_change: String,
    pub changed_files: Vec<PathBuf>,
    pub task_groups: usize,
}

impl LifecycleMutationService {
    pub fn implement_design_node(
        &self,
        req: ImplementDesignNodeRequest,
    ) -> anyhow::Result<ImplementDesignNodeResult>;
}
```

Adapter responsibilities should remain:

- JSON argument parsing
- `ToolResult` formatting
- bus/memory side effects, if any
- operator-facing error wording if tool-specific

Service responsibilities should become:

- node lookup and readiness checks
- OpenSpec artifact creation/update
- design-node metadata mutation (`openspec_change`, status, branches)
- provider refresh
- sync with `omegon-opsx` through `lifecycle::sync`

## Implementation plan

### 1. Characterize existing behavior

Before moving code, add or identify tests that prove current `implement` behavior:

- creates the expected `openspec/changes/<change>/` directory
- writes proposal/design/tasks/spec scaffolding as currently defined
- updates design-node frontmatter with `status: implementing`
- writes `openspec_change`
- preserves implementation notes in generated tasks or design text
- rejects nodes that are not ready, if current behavior does so
- handles existing OpenSpec change directories according to current semantics

### 2. Extract pure scaffolding helpers if needed

If `implement` contains inline string generation, split pure helpers first:

- `build_implement_proposal(...)`
- `build_implement_design(...)`
- `build_implement_tasks(...)`
- `derive_change_name(...)`

These helpers should be easy to unit test without filesystem mutation.

### 3. Add `implement_design_node` service method

Move filesystem/store coordination into `LifecycleMutationService`:

- read node + sections
- create OpenSpec files
- update node frontmatter
- refresh provider
- call `sync::sync_change_by_name` or `sync::sync_change_from_info` as appropriate

### 4. Keep adapter thin

After extraction, `features/lifecycle.rs` should do only:

```rust
let req = ImplementDesignNodeRequest { ... };
let result = self.mutation_service.implement_design_node(req)?;
Ok(text_result(&format!(...)))
```

### 5. Validate broadly

Run at minimum:

```text
cargo test -p omegon --bin omegon lifecycle::mutation
cargo test -p omegon --bin omegon design_tree_implement
cargo test -p omegon --bin omegon openspec
cargo check -p omegon
```

If test names differ, use broader filters:

```text
cargo test -p omegon --bin omegon lifecycle
cargo test -p omegon --bin omegon design_tree
```

## Correctness constraints

- Do not rewrite generated OpenSpec semantics while extracting.
- Do not silently drop implementation notes, decisions, research, or constraints.
- Do not move `focus`/`unfocus` into the mutation service unless a session-state service is introduced.
- Preserve existing tool output unless there is an explicit behavior-change decision.
- Keep OpenSpec sync through `lifecycle::sync`; do not recreate ad hoc FSM transitions in the adapter.

## Open questions

None for the extraction shape. The future session should verify the exact current `implement` behavior from code before editing.

## Decisions

### Decision: Move `implement` as a dedicated service pass

**Status:** decided
**Rationale:** `implement` crosses design-tree, OpenSpec scaffolding, and FSM sync. It is too broad to fold into the mechanical mutation-service migration and needs characterization tests first.

### Decision: Keep focus/unfocus adapter-side

**Status:** decided
**Rationale:** Focus is session/provider state, not durable lifecycle store mutation. Moving it would mix session state into `LifecycleMutationService`.

## Non-goals

- No OpenSpec scaffold redesign in the extraction pass.
- No lifecycle FSM redesign.
- No new CLI/tool behavior unless current behavior is impossible to preserve.
- No broad reformat of `features/lifecycle.rs` beyond the implement extraction.

## Open Questions
