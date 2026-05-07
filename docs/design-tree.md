+++
id = "1fe4c9f9-4bd7-4bd7-8b3a-f2ae076645af"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/design-tree-lifecycle.md"]
last_updated = "2026-03-12"
openspec_baselines = []
subsystem = "design-tree"
+++

# Design Tree

> Structured design exploration — seed ideas, research options, record decisions, track implementation, and bridge to OpenSpec for execution.

## What It Does

The design tree manages the lifecycle of design explorations as structured documents with frontmatter metadata. Each node progresses through statuses: `seed` → `exploring` → `decided` → `implementing` → `implemented` (or `blocked`/`deferred`).

Two agent tools provide full read/write access:
- **`design_tree`** (query): list nodes, get node details, find open questions (frontier), check dependencies, list children, query ready work, query blocked work
- **`design_tree_update`** (mutate): create nodes, set status, set priority, set issue type, add research/decisions/questions, branch child nodes, focus a node for context injection

The `implement` action bridges a decided node to OpenSpec by scaffolding `openspec/changes/<node-id>/` with proposal, design, and tasks from the node's content. From there, `/cleave` executes the implementation.

Documents live in `docs/design/` (archived explorations) and `docs/` (active explorations). Structured sections: Overview, Research, Decisions, Open Questions, Implementation Notes.

## Node Fields

Every design node is a markdown file with YAML frontmatter. Key fields:

| Field | Type | Description |
|---|---|---|
| `id` | string | Unique slug identifier |
| `status` | `NodeStatus` | Lifecycle stage (see below) |
| `title` | string | Human-readable name |
| `parent` | string? | Parent node ID for hierarchy |
| `dependencies` | string[] | Blocking node IDs — this node cannot implement until all deps are `implemented` |
| `tags` | string[] | Free-form labels |
| `issue_type` | `IssueType`? | Classification: `epic \| feature \| task \| bug \| chore` |
| `priority` | `1–5`? | Work urgency: 1 = critical, 5 = trivial. Used by `ready` query sorting. |
| `open_questions` | string[] | Synced from `## Open Questions` body section |
| `branches` | string[] | Git branches associated with this node |
| `openspec_change` | string? | Bound OpenSpec change name |

## Node Status Lifecycle

```
seed → exploring → decided → implementing → implemented
                           ↘ blocked
                           ↘ deferred
```

| Status | Icon | Meaning |
|---|---|---|
| `seed` | ◌ | Idea captured, not yet explored |
| `exploring` | ◐ | Actively researching options |
| `decided` | ● | Decision made, ready to implement |
| `implementing` | ⚙ | OpenSpec change + cleave in progress |
| `implemented` | ✓ | Work complete |
| `blocked` | ✕ | Explicitly stalled — see `blocked` query |
| `deferred` | ◑ | Intentionally parked for later |

## Issue Types

Nodes can be classified with `issue_type` to express intent:

| Type | Meaning |
|---|---|
| `epic` | Large parent work item grouping features/tasks |
| `feature` | User-visible capability |
| `task` | Discrete implementation unit |
| `bug` | Known defect requiring a fix |
| `chore` | Non-functional: refactor, docs, dependency update |

Issue type is optional and purely informational — it does not affect lifecycle transitions. The `ready` and `blocked` queries include `issue_type` in their output so the agent can filter if needed.

## `design_tree` — Query Actions

| Action | Parameters | Description |
|---|---|---|
| `list` | — | All nodes with status, tags, lifecycle binding |
| `node` | `node_id` | Full structured content of one node (sections + lifecycle) |
| `frontier` | — | All open questions across all nodes |
| `dependencies` | `node_id` | Dependency graph for a node |
| `children` | `node_id` | Direct children of a node |
| `ready` | — | **All unblocked nodes ready to implement** (see below) |
| `blocked` | — | **All blocked nodes with blocking dependency details** (see below) |

### `ready` — Session-start triage

Returns all `decided` nodes where every dependency is `implemented`, sorted by priority ascending (p1 first; nodes without priority sort last). This is the canonical "what should I work on next?" query.

```jsonc
// design_tree(action="ready")
[
  { "id": "auth-refresh", "title": "Token refresh", "priority": 1, "issue_type": "bug",
    "tags": ["auth"], "openspec_change": "auth-refresh-tokens" },
  { "id": "dashboard-perf", "title": "Dashboard render perf", "priority": 2,
    "issue_type": "feature", "tags": ["dashboard"], "openspec_change": null }
]
```

**When to call it**: at the start of a session when no specific task has been assigned. The result gives a priority-ordered queue of work that is dependency-resolved and lifecycle-ready.

### `blocked` — Dependency audit

Returns all nodes that are either explicitly `blocked` or are `exploring`/`decided` with at least one dependency not yet `implemented`. Each entry includes `blocking_deps` listing every unresolved blocker by id, title, and status.

Nodes with status `seed` or `deferred` are excluded — parked work is not "blocked", it's waiting.

```jsonc
// design_tree(action="blocked")
[
  {
    "id": "search-indexing", "title": "Search indexing", "status": "exploring",
    "priority": 2, "issue_type": "feature",
    "blocking_deps": [
      { "id": "db-schema-v2", "title": "DB schema v2", "status": "decided" }
    ]
  }
]
```

**When to call it**: before planning a sprint, or when the `ready` list is unexpectedly short. Shows exactly what's stalled and why.

## `design_tree_update` — Mutation Actions

| Action | Key Parameters | Description |
|---|---|---|
| `create` | `id`, `title`, `status?`, `parent?`, `tags?` | New node |
| `set_status` | `node_id`, `status` | Transition lifecycle stage |
| `set_priority` | `node_id`, `priority` | Set urgency 1–5 |
| `set_issue_type` | `node_id`, `issue_type` | Classify as epic/feature/task/bug/chore |
| `add_question` | `node_id`, `question` | Record an open question |
| `remove_question` | `node_id`, `question` | Answer/close a question |
| `add_research` | `node_id`, `heading`, `content` | Record a research finding |
| `add_decision` | `node_id`, `decision_title`, `decision_status`, `rationale` | Crystallize a choice |
| `add_dependency` | `node_id`, `target_id` | Declare a blocking dependency |
| `add_related` | `node_id`, `target_id` | Non-blocking relationship |
| `add_impl_notes` | `node_id`, `file_scope`, `constraints` | Add implementation scope |
| `branch` | `node_id`, `question`, `child_id`, `child_title` | Spawn child node from open question |
| `focus` | `node_id` | Inject node content into every agent turn |
| `unfocus` | — | Clear focused node |
| `implement` | `node_id` | Bridge decided node → OpenSpec change directory |

## Priority & Issue Type in Practice

**Triage a bug found during a session:**
```
design_tree_update(action="create", id="login-crash", title="Login crash on empty password",
                   status="decided", issue_type="bug", tags=["auth"])
design_tree_update(action="set_priority", node_id="login-crash", priority=1)
```

**Next session — the bug surfaces first:**
```
design_tree(action="ready")
→ [{ id: "login-crash", priority: 1, issue_type: "bug", ... }, ...]
```

**Audit what's stalling a feature:**
```
design_tree(action="blocked")
→ [{ id: "...", blocking_deps: [{ id: "...", status: "exploring" }] }]
```

## Dual-Lifecycle Pipeline

The design-tree and OpenSpec work together as two complementary lifecycle layers:

```
seed → exploring → [design spec scaffolded] → /assess design → decided
                                                                    ↓
                                                           implement gate
                                                   (design OpenSpec archived)
                                                                    ↓
                                               openspec/changes/<id>/ scaffolded
                                                  + auto-checkout feature/<id>
                                                  + memory mind forked from default
                                                  + design focus set
                                                                    ↓
                                                                /cleave
                                                                    ↓
                                                             /assess spec
                                                                    ↓
                                                     archive (mind → default merge)
                                                                    ↓
                                                              implemented
```

**Layer 1 — Design Phase** (`openspec/design/<id>/`):
- Scaffolded automatically when a node enters `exploring` status
- Contains Acceptance Criteria: Scenarios, Falsifiability tests, Constraints
- `/assess design` validates the design spec before the node can become `decided`
- Acts as a gate: `design_tree_update(implement)` requires design OpenSpec to be archived

**Layer 2 — Implementation Phase** (`openspec/changes/<id>/`):
- Scaffolded by `design_tree_update(implement)` on a decided node
- Contains proposal.md, design.md, tasks.md and Given/When/Then specs
- `/cleave` executes tasks in parallel with spec scenario assignment per child
- `/assess spec` validates implementation against behavioral contracts
- Archive merges passing scenarios into `openspec/baseline/`

### Acceptance Criteria Section Format

Design nodes include an `## Acceptance Criteria` section with three subsections:

**Scenarios** — Observable behaviors the implementation must satisfy:
```markdown
## Acceptance Criteria

### Scenarios
- Given a decided node, when implement is called, then openspec/changes/<id>/ is scaffolded
- Given an unarchived design OpenSpec, when implement is called, then it is rejected with a clear error
```

**Falsifiability** — Conditions that would prove the design is wrong:
```markdown
### Falsifiability
- If the implement gate can be bypassed without archiving the design spec, the design is falsified
- If cleave produces output that contradicts a passing scenario, the design is falsified
```

**Constraints** — Hard requirements that bound the implementation:
```markdown
### Constraints
- Must not modify files outside the declared scope
- Command names /opsx:* and internal variable names must remain unchanged
```

### `ready` and `blocked` Queries — Design Spec Gate

The `ready` query surfaces nodes that are dependency-resolved and lifecycle-ready. For nodes in the dual-lifecycle pipeline, a node is only `ready` to implement when:
1. All declared dependencies are `implemented`
2. The node status is `decided`
3. (For new nodes) the design-phase OpenSpec has been archived

The `blocked` query will surface nodes where the design spec gate has not been cleared, indicating the design phase needs completion before implementation can proceed.

## Key Files

| File | Role |
|------|------|
| `extensions/design-tree/index.ts` | Extension entry — 2 tools, commands, lifecycle event handlers |
| `extensions/design-tree/tree.ts` | Pure domain logic — parse/generate frontmatter+sections, scan, mutations, branching |
| `extensions/design-tree/types.ts` | `NodeStatus`, `IssueType`, `Priority`, `DesignNode`, `DocumentSections`, `DesignTree` |
| `extensions/design-tree/dashboard-state.ts` | Dashboard state emission for focused node display |
| `extensions/design-tree/lifecycle-emitter.ts` | Memory lifecycle events on status transitions |

## Design Decisions

- **Frontmatter-driven metadata**: Node status, tags, dependencies, branches, OpenSpec binding, priority, and issue_type stored in YAML frontmatter. Body sections parsed structurally.
- **Open questions synced between body and frontmatter**: Adding/removing questions in the `## Open Questions` section updates the frontmatter array and vice versa.
- **`ready` excludes unresolved deps, not just `blocked` status**: A `decided` node whose dependency is still `exploring` won't appear in the ready list — the dep must be fully `implemented`.
- **`blocked` excludes `seed`/`deferred`**: Parked nodes aren't blocked — they're intentionally waiting. Surfacing them as blocked would add noise.
- **Priority sorts `ready`, doesn't gate it**: Priority is advisory. An unprioritized node still appears in the ready list, just sorted last.
- **Auto-transition seed → exploring**: `add_research` and `add_decision` on seed nodes automatically transition to exploring and scaffold the design spec — no manual ceremony required.
- **Substance-over-ceremony decided gate**: `set_status(decided)` checks for open questions (must be empty) and recorded decisions (must have at least one) rather than requiring artifact directory existence. Design specs are auto-extracted from doc content and archived.
- **`implement` bridges to OpenSpec + mind fork**: A decided node's decisions, file scope, and constraints scaffold an OpenSpec change directory. `implement` also auto-checkouts the directive branch (`feature/<node-id>`), forks a scoped memory mind from `default`, and sets design focus — all in one action.
- **Focus context injection**: When a node is focused via `design_tree_update('focus')`, its content is injected into the agent's context on every turn — ensuring design decisions stay visible during implementation.
- **Scan both `docs/` and `docs/design/`**: After the archive migration, the scanner reads from both directories to maintain visibility of all historical nodes.

## Constraints & Known Limitations

- Documents must have valid YAML frontmatter with at least `id` and `status` to be recognized
- No `archived` status exists yet — implemented nodes remain in the tree with `implemented` status
- Focus injection adds to context token usage — unfocus when not actively working on a design
- `priority` and `issue_type` are optional — existing nodes without them are fully supported and sort/filter gracefully

## Related Subsystems

- [OpenSpec](openspec.md) — receives scaffolded changes from `implement` action
- [Cleave](cleave.md) — executes OpenSpec changes generated from design nodes
- [Dashboard](dashboard.md) — displays focused node and tree statistics
- [Project Memory](project-memory.md) — lifecycle events stored as facts on status transitions
