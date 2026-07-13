# Unified Work Agent — Preliminary Design

Status: preliminary
Date: 2026-07-12
Scope: Omegon agent integration over repository-local work, OpenSpec lifecycle,
agent execution tasks, and an eventual shared task server

## Summary

Omegon needs one semantic work surface without declaring one storage system to
be the universal authority. The proposed **Unified Work Agent** is an
Omegon-owned runtime service that composes multiple independently authoritative
work sources behind the `styrene-work` contracts now present in the workspace.

```text
Markplane repository work ──────┐
OpenSpec lifecycle ─────────────┤
Omegon execution tasks ─────────┼── WorkRuntime ── projections / commands / tools
Styrene relation sidecar ───────┤
Future shared task server ──────┘
```

The agent is not a new autonomous persona or a Markplane wrapper process. It is
a semantic subsystem used by the normal Omegon agent loop, command registry,
Workbench, ACP, and future Flynt clients.

## Goals

1. Let the Omegon agent answer “what work exists, what is active, and what is
   blocked?” from one normalized graph.
2. Preserve explicit ownership and lifecycle semantics for every item.
3. Keep Markplane replaceable by confining its model to `styrene-work-local`.
4. Reuse Omegon's native OpenSpec parser and evidence model rather than
   maintaining a second lifecycle parser.
5. Expose one renderer-neutral projection to CLI, TUI, ACP, tools, and Flynt.
6. Support offline repository work now and shared task-server authority later.
7. Introduce writes incrementally, routed by item authority.

## Non-goals

- Replacing OpenSpec with Markplane plans.
- Treating `.omegon/tasks` and Markplane tasks as interchangeable stores.
- Running Markplane's CLI, MCP server, or web UI inside Omegon.
- Selecting the future task-server protocol in this phase.
- Automatically copying whole task or proposal bodies into the model prompt.
- Providing enterprise identity, billing, portfolio, or tenant administration.

## Authority model

| Source | Namespace | Authority | Intended role |
|---|---|---|---|
| Markplane | `markplane:` | Repository | Project planning, initiatives, notes, local task state |
| OpenSpec | `openspec:` | OpenSpec | Requirements, scenarios, implementation lifecycle, verification |
| Omegon task tree | `omegon-task:` | Repository/execution | Agent execution requests, sentry scheduling, run constraints |
| Shared task server | `server:` | TaskServer | Collaborative cross-repository task state |
| Relation sidecar | n/a | Derived | Typed edges between records owned by different systems |

`styrene-work` owns normalized contracts, not records. A generic work command
must inspect `WorkItem.authority` before selecting a mutation path.

## Existing implementation foothold

The Omegon workspace now includes:

```text
core/crates/styrene-work
core/crates/styrene-work-local
core/crates/styrene-work-relations
```

- `styrene-work` defines normalized IDs, authority, state, priority, relations,
  queries, commands, source traits, and aggregate projections.
- `styrene-work-local` embeds pinned `markplane-core`; no Markplane CLI or MCP
  process is required.
- `styrene-work-relations` overlays validated typed cross-source relations from
  `.styrene/relations.yaml`.

The Markplane adapter was exercised in a disposable Omegon clone together with
nine active OpenSpec changes. Local source references are repository-relative,
and typed sidecar edges resolve only when both endpoints exist.

## Proposed architecture

### 1. `WorkRuntime`

Add an Omegon-owned runtime module, initially under:

```text
core/crates/omegon/src/work_runtime.rs
```

Conceptual shape:

```rust
pub struct WorkRuntime {
    repo_root: PathBuf,
    generation: u64,
    snapshot: WorkProjection,
    warnings: Vec<WorkWarning>,
}
```

The runtime discovers sources from the canonical repository root:

```text
.markplane/                         → MarkplaneLocalSource
openspec/                           → OmegonOpenSpecSource
.omegon/tasks/                      → OmegonTaskSource
.styrene/relations.yaml             → RelationOverlay
.styrene/cache/server-work.json     → TaskServer cache source, when enabled
```

The runtime owns refresh and projection generation. Command handlers and UI
components must not independently probe these files.

Initial refresh policy:

- Build on first `/work` or work-tool access.
- Cache one immutable projection with a monotonically increasing generation.
- Support explicit refresh.
- Add file watching only after the semantic command path is stable.

### 2. Native OpenSpec source

Do not import the spike's standalone OpenSpec crate. Omegon already owns a
richer parser in:

```text
core/crates/omegon/src/lifecycle/spec.rs
core/crates/omegon/src/lifecycle/types.rs
```

Implement `OmegonOpenSpecSource` over `lifecycle::spec::list_changes()` and
`ChangeInfo`.

Mapping:

| OpenSpec | Unified work |
|---|---|
| `ChangeInfo.name` | `openspec:<name>` |
| `ChangeStage::Proposed/Specified` | `Draft` |
| `ChangeStage::Planned` | `Planned` |
| `ChangeStage::Implementing/Verifying` | `Active`, exact stage retained |
| `ChangeStage::Archived` | `Archived` |
| task groups | compact task-count/group metadata |
| requirements/scenarios | scenario and evidence summaries |
| `ChangeInfo.path` | repository-relative external ref |

Exact lifecycle stage and TDD/claim evidence remain structured metadata. The
normalized `WorkState` is intentionally coarser.

### 3. Omegon execution-task source

Wrap the existing task tree in:

```text
core/crates/omegon/src/task_tree.rs
```

as `OmegonTaskSource`.

This is also an architectural clarification: `.omegon/tasks` should converge on
**execution bindings**, not compete with Markplane as a general project manager.
Its distinctive fields already point in that direction:

- model
- skill
- turn, timeout, and token budgets
- execution mode
- cron/webhook triggers
- sentry status

Status mapping:

| Task tree | Unified work |
|---|---|
| `Todo` | `Backlog` |
| `InProgress` | `Active` |
| `Done` | `Completed` |
| `Blocked` | `Blocked` |
| `Failed` | `Cancelled` plus execution-failure metadata |

Links map to typed relations:

- `depends_on` → `DependsOn`
- `openspec_change` → `Implements` or `Specifies`, according to command intent
- `design_node_id` → typed external design reference

The existing `omegon task` CLI remains compatible during migration. A later
change may rename the execution-specific surface rather than silently changing
its meaning.

### 4. Typed relation overlay

Cross-source relationships live in a Styrene-owned sidecar:

```yaml
version: 1
relations:
  - source: markplane:TASK-abc12
    kind: implements
    target: openspec:identity-secrets
```

This avoids modifying Markplane or OpenSpec schemas merely so they can point at
one another. The runtime validates source and target IDs against the aggregate
snapshot and fails closed on duplicate, missing, self-referential, unsafe, or
unknown-version records.

### 5. Future task-server source

The server source will implement the same contracts but remains disabled until
specified. Its requirements are already visible:

- scoped authentication and workspace identity
- revision-bearing records
- idempotent mutation keys
- optimistic concurrency
- event subscription or polling
- durable offline outbox
- acknowledgement, retry, and compaction
- conflict projection suitable for operator resolution

Server records use `server:` IDs and `Authority::TaskServer`. Offline mutation
must never queue commands for repository- or OpenSpec-owned records.

## Semantic projection

Add a renderer-neutral surface:

```text
core/crates/omegon/src/surfaces/work.rs
```

Proposed DTOs:

```rust
pub struct WorkProjection {
    pub version: u16,
    pub generation: u64,
    pub summary: WorkSummaryProjection,
    pub items: Vec<WorkItemProjection>,
    pub warnings: Vec<WorkWarningProjection>,
}

pub struct WorkSummaryProjection {
    pub active: usize,
    pub blocked: usize,
    pub planned: usize,
    pub completed: usize,
    pub by_authority: Vec<AuthorityCountProjection>,
}
```

List projections are compact. They include:

- ID and title
- kind, authority, state, and priority
- assignee and tags
- relation/ref summaries
- lifecycle certainty and other warnings

They do not contain complete Markdown bodies. Full details are fetched with a
specific item request.

This surface follows the existing Omegon rule: semantic projection first;
renderer adapters second. TUI, CLI, ACP, and Flynt must not maintain separate
work-discovery logic.

## Command surface

Register a canonical read-only `/work` command through `CommandDefinition`:

```text
/work
/work list
/work active
/work blocked
/work get <id>
/work graph <id>
/work warnings
/work refresh
```

Availability and safety:

```text
TUI: yes
CLI remote slash: yes
ACP: yes
Safety: read-only
```

Add corresponding canonical slash and control requests, for example:

```rust
ControlRequest::WorkList { filter }
ControlRequest::WorkGet { id }
ControlRequest::WorkGraph { id, depth }
ControlRequest::WorkWarnings
ControlRequest::WorkRefresh
```

All routes call `WorkRuntime`; none parse source files directly.

A direct CLI command may later expose the same projection as:

```text
omegon work list [--json]
omegon work get <id> [--json]
```

The slash/control registry remains the canonical command definition.

## Agent tools

After the read-only command path is stable, expose tools backed by the same
runtime:

```text
work_list
work_get
work_neighbors
work_search
work_refresh
```

The normal agent loop sees normalized records. Source-specific behavior is
selected only when authority affects an operation.

Initial safety classification:

| Tool | Safety |
|---|---|
| `work_list` | read-only |
| `work_get` | read-only |
| `work_neighbors` | read-only |
| `work_search` | read-only |
| `work_refresh` | read-only local refresh |

No generic mutation tool ships in the first slice.

## Workbench and dashboard integration

The Workbench should consume `WorkProjection`, showing compact operational
state such as:

```text
Active
  3 repository tasks
  2 OpenSpec changes implementing
  1 execution task running

Blocked
  markplane:TASK-auth-ui → openspec:identity-secrets

Warnings
  2 inferred lifecycle states
  1 unresolved relation target
```

Selection uses namespaced IDs. A focused record can then drive item detail,
graph navigation, or source-specific actions.

The dashboard may display summary counts, but Workbench remains the primary
operational surface. Neither should expose Markplane-native DTOs.

## Context integration

The model prompt receives a bounded synopsis, not the complete graph:

```text
Work snapshot generation 17:
- 3 active
- 1 blocked
- 2 planned
- 1 OpenSpec change verifying
- focused: markplane:TASK-abc12
```

The agent retrieves bodies, scenarios, and neighbors on demand. This preserves
Markplane's useful context-compilation idea while leaving prompt budgeting and
redaction under Omegon control.

## Mutation routing

Mutation is a second-phase capability. Routing is authority-based:

### Markplane repository item

Permitted operations initially:

- state transition
- assignee change

These route through `MarkplaneLocalSource::apply` and preserve Markplane's
locking and atomic-write behavior.

### OpenSpec change

Generic work mutation is rejected. The agent must use existing OpenSpec
lifecycle operations so test registration, task reconciliation, verification,
and archive guards remain intact.

### Omegon execution task

Routes through existing `task_tree` operations. Execution-specific fields stay
owned by that subsystem.

### Server item

Routes through online transport when connected, otherwise a revision-aware,
idempotent outbox. This remains disabled until the server protocol is specified.

## Failure and degradation behavior

Source failure must not silently fabricate state.

- A missing optional source yields zero items for that authority.
- A malformed present source yields a projection warning or command failure,
  depending on whether partial projection is safe.
- Invalid relation sidecars fail closed for cross-source edges.
- OpenSpec exact lifecycle stage is never inferred if native lifecycle state is
  available.
- Server cache records lacking server authority, namespace, or revision are
  rejected.
- Full local paths and secrets must not appear in operator projections.

The projection identifies inferred/degraded state explicitly so the agent can
separate known facts from best-effort interpretation.

## Security constraints

- Canonicalize the repository root using Omegon's bounded project-root logic.
- Derive source paths from that root; do not accept arbitrary adapter paths in
  agent tools.
- Reject symlink escapes and traversal components for sidecars and caches.
- Keep references repository-relative in serialized projections.
- Treat task and proposal bodies as untrusted project content when adding them
  to model context.
- Do not log server credentials, access tokens, or complete authorization
  envelopes.
- Require expected revisions for server mutations.

## Implementation sequence

### Slice 1 — native read model

- Add `WorkRuntime`.
- Add native OpenSpec adapter.
- Add `.omegon/tasks` adapter.
- Compose Markplane and typed sidecar relations.
- Add `surfaces/work.rs` with Markdown and JSON projections.
- Add focused unit tests.

### Slice 2 — canonical `/work` command

- Register command availability/safety metadata.
- Add canonical slash parsing and control requests.
- Route CLI/TUI/ACP through one runtime response.
- Add parity tests.

### Slice 3 — Workbench and context

- Add compact active/blocked/warning sections.
- Add focused item handling.
- Add bounded context summary and on-demand detail retrieval.

### Slice 4 — guarded local mutation

- Enable Markplane state/assignee updates.
- Adapt execution-task updates.
- Reject generic OpenSpec writes.
- Emit mutation provenance and refresh generation.

### Slice 5 — shared server

- Specify and implement transport and authentication.
- Add revision/conflict semantics and event refresh.
- Add durable acknowledgement/retry/compaction.
- Add promotion or binding workflows between repository and shared records.

## First acceptance target

The first end-to-end implementation is complete when:

1. `/work list` projects Markplane, native OpenSpec, and `.omegon/tasks` records
   through only `styrene-work`/surface DTOs.
2. `.styrene/relations.yaml` creates validated typed cross-source edges.
3. CLI remote slash, TUI, and ACP receive the same semantic result.
4. Full bodies are absent from list projections and retrievable by ID.
5. Malformed sources produce explicit warnings or errors, not fabricated state.
6. No mutation paths are enabled.
7. Existing `omegon task` behavior remains unchanged.

## Open questions

1. Should `WorkState` gain a dedicated `Verifying` state, or should exact
   lifecycle remain an authority-specific facet?
2. Should `.omegon/tasks` be renamed to `.omegon/executions` before or after the
   unified read surface ships?
3. Should relation sidecars remain one project file or become per-source
   fragments merged by the runtime?
4. What is the stable ACP projection/versioning contract for work graph pages?
5. Should repository-local work be promoted to server authority, mirrored, or
   linked as separate records when collaboration begins?

These questions do not block Slice 1. The simplest implementation can retain
coarse `WorkState`, preserve `.omegon/tasks`, use one sidecar, and expose a
versioned read-only projection.
