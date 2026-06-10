---
title: Lifecycle/OpenSpec Surface Map
status: exploring
tags: [architecture, lifecycle, openspec, design-tree, decoupling, correctness]
---

# Lifecycle/OpenSpec Surface Map

This document maps Omegon's lifecycle surface before further feature work. The goal is correctness and decoupling: preserve the current design-tree/OpenSpec behavior while making ownership, invariants, and extraction seams explicit.

## Current tool surface

Tool names are registered under `core/crates/omegon/src/tool_registry.rs` in `lifecycle::*` and implemented by `core/crates/omegon/src/features/lifecycle.rs`.

| Tool | Purpose | Current adapter owner |
|---|---|---|
| `design_tree` | query design nodes, frontier, dependencies, children, ready/blocked work | `core/crates/omegon/src/features/lifecycle.rs` |
| `design_tree_update` | mutate design nodes: create, status, archive, questions, research, decisions, dependencies, focus, implement | `core/crates/omegon/src/features/lifecycle.rs` |
| `openspec_manage` | manage OpenSpec changes: status/get/propose/add spec/register tasks/test files/set task status/archive | `core/crates/omegon/src/features/lifecycle.rs` |
| `lifecycle_doctor` | audit lifecycle drift and suspicious design-tree states | `core/crates/omegon/src/features/lifecycle.rs` |

Slash/command surface is also adapter-owned in `LifecycleFeature`:

| Command | Purpose |
|---|---|
| `/design-focus` | pin a design node for lifecycle context injection |
| `/design-unfocus` | clear focused design-node context |
| `/focus` / `/unfocus` variants | operator-facing focus shortcuts where enabled |

## Engine/library surface

Lifecycle is currently a Rust module inside the main `omegon` crate plus an FSM crate:

```text
core/crates/omegon/src/lifecycle/
core/crates/omegon-opsx/
```

Important `omegon::lifecycle` modules:

| Module | Responsibility |
|---|---|
| `design.rs` | parse/write design-node markdown, frontmatter, sections, tree scanning, node mutations |
| `spec.rs` | parse/list/read OpenSpec changes, specs, scenarios, task groups, TDD evidence annotations |
| `types.rs` | design-node, OpenSpec, section, decision, task, scenario DTOs |
| `context.rs` | `LifecycleContextProvider`, focus state, context injection from focused nodes and active changes |
| `read_model.rs` | joined lifecycle projection for UI/API consumers, OpenSpec snapshots, drift projection |
| `doctor.rs` | heuristic audits for design-tree and OpenSpec/FSM drift |
| `capture.rs` | ambient lifecycle capture support |
| `codex_export.rs` | design tree export to Codex vault notes |

Important `omegon-opsx` modules:

| Module | Responsibility |
|---|---|
| `fsm.rs` | lifecycle FSM operations and legal transitions |
| `types.rs` | node/change/milestone state types |
| `store.rs` | JSON file state store and in-memory store |
| `error.rs` | `OpsxError` |

Public-ish abstractions used across the harness:

```rust
LifecycleFeature
LifecycleContextProvider
LifecycleReadHandle
LifecycleSnapshot / OpenSpecProjection
DesignNode / DocumentSections / ChangeInfo / SpecFile / Scenario
omegon_opsx::Lifecycle<JsonFileStore>
omegon_opsx::{NodeState, ChangeState}
```

## Architectural position

Lifecycle already has three distinct layers, but `LifecycleFeature` still carries too much orchestration:

```text
Tool/command adapter (`features/lifecycle.rs`)
  → markdown parser/writer modules (`lifecycle::{design,spec}`)
  → lifecycle FSM (`omegon-opsx` JSON store)
```

A separate read projection already exists:

```text
LifecycleReadHandle → LifecycleSnapshot / OpenSpecProjection
```

That shape is healthy. The risk is that mutation orchestration, markdown writes, FSM synchronization, archive recovery, memory event queueing, context refresh, and operator-visible formatting all still meet inside one broad feature adapter.

## Adapter-owned responsibilities

These should stay in `core/crates/omegon/src/features/lifecycle.rs` or narrow tool/command adapter modules:

- tool definitions and JSON schemas
- JSON argument parsing and validation
- `ToolResult` markdown/details formatting
- command registration and command result phrasing
- provider refresh after mutation
- context-provider focus command wiring
- pending memory fact/event queueing after lifecycle actions
- operator-visible error wording
- feature trait integration and bus event handling

## Engine/service-owned responsibilities

These belong in `core/crates/omegon/src/lifecycle/` or `omegon-opsx`, not in the tool adapter:

- design-node mutation semantics
- OpenSpec change mutation semantics
- markdown ↔ FSM synchronization
- design-tree-first implementation gate
- OpenSpec archive transaction recovery
- archive rollback/recovery state machine
- ready/blocked query policy
- drift detection and reconciliation advice
- task checkbox mutation by stable task id/group
- context selection policy before final prompt rendering
- lifecycle projection for dashboard/API consumers

## Current coupling risks

### `LifecycleFeature` is a facade plus lifecycle service

`features/lifecycle.rs` currently owns tool schemas, command handling, markdown mutations, FSM synchronization, archive transaction recovery, provider refresh, pending memory requests, and result formatting. That makes lifecycle correctness harder to test without constructing the whole feature and its harness-facing state.

### Markdown state and FSM state require explicit synchronization

Design nodes and OpenSpec changes are git-native markdown artifacts, while `omegon-opsx` keeps a JSON FSM state. Current code syncs opportunistically from parsed markdown/change info into the FSM. This is a real boundary and should be named as one, because stale or missing FSM records are correctness issues rather than mere display differences.

### OpenSpec archive is transactional but adapter-local

`OpenSpecArchiveTransaction` and recovery live in `features/lifecycle.rs`, even though they are lifecycle-domain behavior. The adapter should not own the details of `intent_written`, `content_moved`, rollback rename, and FSM forced transition recovery.

### Read projection exists but mutations do not target it

`LifecycleReadHandle` provides a joined projection for UI/API consumers. Mutations still return hand-built JSON or text from the adapter. Over time, each new surface can accidentally recompute lifecycle truth differently unless the read model becomes the canonical post-mutation projection source.

### Design/OpenSpec terminology straddles user and implementation language

Operator-facing docs call OpenSpec the implementation layer, while code and tool names still use `openspec`. That is acceptable, but the boundary should remain explicit: rename UX labels where useful, but do not hide the on-disk OpenSpec contract or tool names from developers.

## Correctness invariants

### Design node identity and parsing

- Every design node has a stable `id` in frontmatter.
- TOML frontmatter under `[data]` and older top-level fields parse consistently.
- Publication metadata is ignored by lifecycle parsing.
- `docs/<node-id>.md` remains the authoritative design artifact.
- Parsed `open_questions` include `[assumption]` entries as unknowns for readiness.

### Design state transitions

- `seed` is capture-only and may be underspecified.
- `exploring` nodes should have open questions or assumptions.
- `decided` requires resolved unknowns and at least one recorded decision.
- `implementing` is entered through `design_tree_update(implement)` after implementation scaffolding.
- `implemented` should not retain open questions.
- Archived/superseded nodes should not appear in normal ready/frontier lists.

### Design-tree-first implementation

- Tracked implementation work originates from a decided design node.
- The OpenSpec change is bound back to the design node via `openspec_change`.
- Implementation scaffolding must derive from current decisions and implementation notes, not stale/rejected decisions.
- Archive of implementation work is the path back to implemented design state.

### OpenSpec change state

- Active changes live under `openspec/changes/<name>/`.
- Archived changes live under `openspec/archive/<name>/`.
- File stage derived from proposal/spec/design/tasks must not silently diverge from `omegon-opsx` `ChangeState`.
- Registered task progress mirrors `tasks.md` checkbox state.
- Registered spec domains mirror files under `specs/`.
- Archive requires complete/reconciled tasks and acceptable assessment state.

### Archive transaction safety

- A pending archive transaction records source, destination, old state, target state, and phase.
- If the change directory exists and archive directory does not, stale intent can be removed safely.
- If the archive directory exists and change directory does not, FSM state must be forced to archived before the transaction is cleared.
- If both directories exist, recovery must refuse and require human reconciliation.
- If neither directory exists, recovery must refuse and require human reconciliation.

### Drift detection

- A disk OpenSpec change without an `omegon-opsx` record is drift.
- A disk-derived OpenSpec stage that disagrees with FSM state is drift.
- An archived directory whose FSM state is not archived is drift.
- Implemented parents with active children are suspicious until reconciled.
- Open questions apparently answered by decided sections are suspicious until removed or explained.

### Context injection

- Focused design-node context is injected only when focus is set and the node parses.
- Active implementing/verifying OpenSpec changes can inject spec/task context.
- Provider refresh must clear stale section caches after mutation.
- Context injection is a selection/rendering concern; mutation correctness must not depend on prompt context being injected.

## Proposed service boundaries

Introduce narrow lifecycle services without adding user-facing behavior:

```rust
pub struct LifecycleMutationService {
    repo_path: PathBuf,
    opsx: Arc<Mutex<OpsxLifecycle<JsonFileStore>>>,
}
```

Initial methods should preserve existing behavior:

```rust
create_design_node(req) -> DesignNode
set_design_status(node_id, status) -> DesignNode
archive_design_node(req) -> DesignNode
add_design_question(node_id, question) -> DesignNode
add_design_decision(req) -> DesignNode
implement_design_node(node_id) -> ImplementationScaffold
propose_change(req) -> ChangeInfo
add_spec(req) -> ChangeInfo
register_tasks(change_name) -> ChangeInfo
set_task_status(req) -> ChangeInfo
archive_change(change_name) -> ArchiveOutcome
recover_archive_transactions() -> Vec<RecoveryEvent>
sync_opsx_from_markdown() -> SyncReport
```

Keep a separate projection boundary:

```rust
pub struct LifecycleReadHandle { ... }
```

The adapter would parse tool args, call mutation services, refresh the provider, and render `ToolResult`s. The services would encode lifecycle semantics in one reusable place.

## Low-risk extraction candidates

1. **Archive transaction recovery**
   - Move `OpenSpecArchiveTransaction`, transaction path helpers, write/remove/recover logic out of `features/lifecycle.rs`.
   - Test filesystem edge cases with temp dirs and `omegon-opsx` in-memory or JSON store.
   - No tool behavior change.

2. **OpenSpec task status mutation**
   - Move stable task id/group checkbox updates into a lifecycle service/helper.
   - Preserve `openspec_manage(set_task_status)` output.
   - Add tests for missing group, duplicate ids, and idempotent done/pending updates.

3. **Markdown-to-FSM sync**
   - Name the current bootstrap/sync behavior as `sync_opsx_from_design_tree` and `sync_opsx_from_changes`.
   - Return structured drift/sync reports instead of silently forcing or ignoring state.
   - Keep current startup behavior until stricter gates are tested.

4. **Ready/blocked query policy**
   - Move `design_tree` ready/blocked selection into `lifecycle/read_model.rs` or a sibling query module.
   - UI, tools, and future API consumers should share one policy.

5. **Tool result rendering**
   - After mutation methods return typed outcomes, keep markdown/JSON formatting adapter-side.
   - This is a cleanup after domain extraction, not the first extraction.

## Relationship to Memory Mind and Codebase Mind

Lifecycle is the project-control mind: it describes what work exists, why it exists, and what state it is in. It should reference semantic memory and codebase structure but not collapse into either.

| Concern | Lifecycle/OpenSpec | Semantic Memory Mind | Codebase Mind |
|---|---|---|---|
| Primary unit | design node, change, spec, task, scenario | fact, episode, edge | file, symbol, chunk, relation |
| Store | docs/OpenSpec markdown + opsx JSON FSM | memory DB + JSONL/vault projection | structural index/projection |
| Projection | dashboard/read model/context injection | prompt facts/vault notes | repository structure context |
| Mutation source | agent/operator lifecycle tools | agent/operator/session extraction | scanners/indexers/discovery |
| Correctness risk | stale state, skipped gates, unreconciled specs | mind leakage, stale facts, recall drift | stale structural map, incorrect symbols |

Promotion between these surfaces should be explicit. For example, archiving a change may emit a memory fact, and codebase scans may inform implementation notes, but neither should mutate lifecycle state without a lifecycle action.

## Non-goals for this branch

- No new lifecycle tools.
- No OpenSpec UX rename in code.
- No change to design-tree/OpenSpec gates.
- No migration of existing OpenSpec artifacts.
- No broad refactor of `LifecycleFeature` until invariants have targeted tests.
- No replacement of markdown as the authoritative git-native artifact format.

## Recommended next implementation slice

1. Add targeted tests around archive transaction recovery and OpenSpec/FSM drift detection.
2. Extract archive transaction recovery from `features/lifecycle.rs` into `lifecycle/`.
3. Keep `LifecycleFeature` tool schemas and result rendering unchanged.
4. Validate with `cargo test -p omegon lifecycle` or narrower filters, then `cargo check -p omegon`.
