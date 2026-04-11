---
id: workspace-ownership-first-pass
title: "Workspace ownership first pass — local lease registry and startup enforcement"
status: exploring
parent: workspace-ownership-model
tags: [workspace, lease, runtime, cleave, release, benchmark, startup]
open_questions:
  - "Where in the startup path should mutable-workspace admission be enforced first: TUI bootstrap/session start, daemon attach, or a shared workspace-admission layer used by both?"
  - "What is the minimum release/benchmark authority check in first pass: warn-only, hard refusal outside `release`/`benchmark` roles, or explicit override with operator confirmation?"
dependencies: []
related: [workspace-ownership-model]
---

# Workspace ownership first pass — local lease registry and startup enforcement

## Overview

This node defines the minimum viable implementation of the workspace ownership model.

The first pass is not trying to solve distributed coordination or full remote backend orchestration. It is solving the immediate and recurring problem:

> multiple mutable Omegon agents must not silently share one filesystem surface.

The first pass should introduce a machine-local runtime workspace contract that:
- records workspace identity and ownership
- prevents accidental dual mutable occupancy
- creates or suggests isolated workspaces when parallel mutable work is needed
- gives cleave the same workspace discipline as general multi-agent operation
- isolates release and benchmark authority from casual mutable development

## Decisions

### First pass uses local lease file + project registry + shared admission layer

**Status:** proposed

**Rationale:** The minimum viable system should use both a per-workspace lease file and a project-level local registry, with a shared admission layer called from TUI startup, daemon attach, cleave child creation, and release/benchmark authority checks. This gives local self-description, operator visibility, and a single policy surface without prematurely building distributed coordination.

## Scope

### In scope

- machine-local per-workspace lease metadata
- machine-local project workspace registry
- startup admission checks for mutable sessions
- conflict resolution flow for second mutable session
- workspace role metadata
- workspace kind metadata (inferred + operator-overridable)
- first-pass cleave integration via the same registry model
- first-pass release / benchmark authority checks

### Out of scope

- distributed/multi-host lease consensus
- k8s/global workspace arbitration
- full Auspex supervisor redesign
- enterprise lock managers
- forge-specific behavior differences in workspace identity

## First-pass architectural stance

### Local, not tracked

All active workspace ownership metadata is machine-local runtime state.

It must live under local runtime paths such as:
- `.omegon/runtime/workspace.json`
- `.omegon/runtime/workspaces.json`

and be excluded from git.

### Shared admission layer

The first pass should not duplicate workspace checks in unrelated call sites.

Introduce a single shared runtime admission layer that can be called from:
- TUI/session startup
- daemon attach/start flows
- cleave child workspace creation
- release/benchmark authority commands

That prevents divergent policy.

## Data model

### Per-workspace lease file

Path:
- `.omegon/runtime/workspace.json`

Proposed schema:

```json
{
  "project_id": "string",
  "workspace_id": "string",
  "label": "string",
  "path": "string",
  "backend_kind": "local-dir|git-worktree|git-clone|jj-checkout|remote-dir|pod-volume",
  "vcs_ref": {
    "vcs": "git|jj",
    "branch": "string|null",
    "revision": "string|null",
    "remote": "string|null"
  },
  "bindings": {
    "milestone_id": "string|null",
    "design_node_id": "string|null",
    "openspec_change": "string|null"
  },
  "branch": "string",
  "role": "primary|feature|cleave-child|benchmark|release|exploratory|read-only",
  "workspace_kind": "code|vault|knowledge|spec|mixed|generic",
  "mutability": "mutable|read-only",
  "owner_session_id": "string|null",
  "owner_agent_id": "string|null",
  "created_at": "RFC3339",
  "last_heartbeat": "RFC3339",
  "parent_workspace_id": "string|null",
  "source": "operator|cleave|auspex|benchmark|release"
}
```

### Project-level registry

Path:
- `.omegon/runtime/workspaces.json`

Purpose:
- list all known local workspaces for this project
- support operator visibility
- support stale lease adoption
- support future dashboard/Auspex surfacing

Proposed shape:

```json
{
  "project_id": "string",
  "repo_root": "string",
  "workspaces": [
    {
      "workspace_id": "string",
      "label": "string",
      "path": "string",
      "backend_kind": "local-dir|git-worktree|git-clone|jj-checkout|remote-dir|pod-volume",
      "vcs_ref": {
        "vcs": "git|jj",
        "branch": "string|null",
        "revision": "string|null",
        "remote": "string|null"
      },
      "bindings": {
        "milestone_id": "string|null",
        "design_node_id": "string|null",
        "openspec_change": "string|null"
      },
      "branch": "string",
      "role": "feature",
      "workspace_kind": "mixed",
      "mutability": "mutable",
      "owner_session_id": "string|null",
      "last_heartbeat": "RFC3339",
      "stale": false
    }
  ]
}
```

### Identity boundary

The first pass must keep workspace coordination identity separate from backing substrate identity.

Required distinction:
- `workspace_id` = machine/runtime coordination identity
- `label` = human-facing operator name
- `backend_kind` = how the surface is realized
- `vcs_ref` = optional descriptive VCS linkage, not identity
- `bindings.milestone_id` / `bindings.design_node_id` / `bindings.openspec_change` = optional lifecycle bindings describing what the workspace is for

This keeps workspace from collapsing into a shadow branch/worktree system.


### Inputs

Admission evaluates:
- requested mutability (`mutable` / `read-only`)
- requested role
- requested or inferred workspace kind
- current lease state
- release/benchmark intent if applicable

### Outcomes

Admission returns one of:
- `GrantedMutable`
- `GrantedReadOnly`
- `ConflictReadOnlySuggested`
- `ConflictCreateWorkspaceSuggested`
- `ConflictStaleLeaseAdoptable`
- `DeniedByAuthorityPolicy`

### First-pass default conflict policy

For a second mutable attach on the same workspace:
- do **not** silently grant mutability
- default to suggesting a sibling workspace/worktree
- allow explicit read-only attach
- allow explicit stale-lease adoption when safe

## Workspace kind flow

### Inference

First pass should support heuristic inference from:
- repo manifests (`Cargo.toml`, `package.json`, `pyproject.toml`, etc.)
- `.obsidian/`
- `openspec/`
- `docs/`
- markdown density / vault-like shape

### Declaration / override

The operator must be able to override inference.

First-pass storage:
- local workspace metadata stores the active `workspace_kind`
- later we may add durable project-level defaults

### First-pass rule

Inference is a convenience.
Declaration is authoritative.

## Startup integration

### Preferred insertion point

A shared workspace-admission layer should be called from both:
- TUI bootstrap / session startup
- daemon attach/start path

This is better than sprinkling checks independently.

### First-pass rollout order

1. TUI/session startup path
2. daemon attach/start path
3. cleave child creation path
4. release / benchmark command gates

## Cleave integration

Cleave must stop being a special-case workspace mechanism.

### First pass behavior

Cleave child creation should:
- allocate a workspace via the same local registry
- assign role `cleave-child`
- assign a mutable lease to the child session
- inherit workspace kind from the parent unless explicitly overridden

### Reconciliation

First pass does not need to redesign the whole merge engine.
It only needs cleave child creation to use the shared workspace ownership contract.

## Release and benchmark authority

### Release authority

First-pass goal:
- prevent ambiguous RC cuts from arbitrary mutable workspaces

Recommended policy:
- require role `release` or explicit operator override with confirmation

### Benchmark authority

First-pass goal:
- prevent release-candidate benchmarking from silently targeting post-tag `HEAD`

Recommended policy:
- require benchmark intent to name an explicit ref when running in release-evaluation mode
- if role is `benchmark`, warn or refuse when `HEAD` is ahead of the target RC tag

## Stale lease handling

### First-pass detection

A lease may be considered stale when:
- owner heartbeat is older than threshold
- owner process/session cannot be confirmed alive
- workspace path/branch identity still matches registry expectations

### First-pass operator options

On stale detection:
- adopt mutable lease
- create sibling workspace instead
- attach read-only

## File scope

### New modules (first pass)

- `core/crates/omegon/src/workspace/mod.rs`
  - module exports
  - core workspace metadata types
  - shared enums (`WorkspaceRole`, `WorkspaceKind`, `Mutability`, `AdmissionOutcome`)

- `core/crates/omegon/src/workspace/types.rs`
  - `WorkspaceLease`
  - `WorkspaceRegistry`
  - `WorkspaceSummary`
  - serde schema for local runtime files

- `core/crates/omegon/src/workspace/runtime.rs`
  - read/write local runtime metadata
  - heartbeat helpers
  - stale lease detection helpers
  - `.omegon/runtime/` path helpers

- `core/crates/omegon/src/workspace/infer.rs`
  - heuristic workspace-kind inference from filesystem/git/jj cues
  - no mutation side effects

- `core/crates/omegon/src/workspace/admission.rs`
  - shared mutable-workspace admission logic
  - conflict classification
  - operator-action recommendation surface

- `core/crates/omegon/src/workspace/authority.rs`
  - release / benchmark authority checks
  - explicit-ref validation helpers for release benchmarking

### Existing integration surfaces

- TUI/session startup path
- daemon attach/start path
- cleave child creation path
- release command path (`just rc` integration boundary or Rust-side preflight)
- benchmark command path

## Command and UX surface (first pass)

The first pass should expose just enough operator-facing control to make workspace hygiene real.

### Required operator-visible actions

- inspect current workspace identity
- inspect mutable owner / lease status
- inspect sibling workspaces in the local project registry
- declare or override workspace kind
- create sibling mutable workspace on conflict
- adopt stale lease explicitly

### Minimal first-pass command set

This does **not** require a large UX system yet. A minimal path can be:
- startup conflict prompts in TUI / daemon attach responses
- one slash command family for inspection and declaration
- typed control requests underneath so daemon/Auspex can share the same contract

#### Read commands

- `/workspace` → alias for `/workspace status`
- `/workspace status`
- `/workspace list`
- `/workspace kind` → show current kind and whether it is inferred or explicitly declared

#### Write commands

- `/workspace kind set <code|vault|knowledge|spec|mixed|generic>`
- `/workspace kind clear`
- `/workspace new <label>`
- `/workspace adopt`

### Deferred commands

These should **not** land in first pass:
- `/workspace attach`
- `/workspace delete`
- `/workspace reconcile`
- `/workspace promote`
- `/workspace prune`

They are real needs, but they increase workflow and policy complexity before the lease/admission model is proven.

### Command semantics

- Bare `/workspace` should be a status/inspection entrypoint.
- Mutating subcommands should route through shared control/runtime requests rather than direct TUI-only state changes.
- The same underlying request shapes should be reusable by TUI, daemon, and future Auspex UI surfaces.

## Rollout slices

The first pass should be delivered in small, attributable slices.

### Slice 0 — metadata primitives only

Deliver:
- runtime path helpers
- `WorkspaceLease`, `WorkspaceRegistry`, `WorkspaceKind`, `WorkspaceRole`
- unit tests for serialization, stale detection, and inference

No startup enforcement yet.

### Slice 1 — shared admission layer

Deliver:
- admission API for mutable/read-only attach
- conflict classification
- workspace-kind inference + explicit override plumbing

Still warn-only at call sites if necessary, but central policy exists.

### Slice 2 — startup enforcement

Deliver:
- TUI/session startup integration
- daemon attach/start integration
- operator-visible conflict handling

This is the first slice that changes day-to-day behavior.

### Slice 3 — cleave unification

Deliver:
- cleave child workspace allocation through shared registry/admission layer
- `cleave-child` role assignment
- inherited workspace kind handling

### Slice 4 — authority enforcement

Deliver:
- release-role checks for RC cuts
- benchmark-role checks for release-evaluation runs
- explicit-ref requirements for release benchmarking

## First-pass test plan

### Unit tests

- workspace kind inference is deterministic for representative repo/vault/spec fixtures
- explicit kind override beats inference
- stale lease detection is deterministic from heartbeat inputs
- mutable conflict classification returns the correct outcome for occupied/stale/free workspaces
- authority checks reject ambiguous release/benchmark contexts

### Integration tests

- second mutable attach on same workspace produces conflict outcome
- read-only attach remains allowed when mutable lease exists
- cleave child workspace registration creates distinct `cleave-child` workspaces
- release candidate benchmark path rejects post-tag `HEAD` ambiguity without explicit ref

### Non-code tests

At least one first-pass fixture should be a vault-like workspace:
- `.obsidian/`
- markdown-only docs
- no code manifests

This is required so the system does not regress into code-only assumptions.

## Constraints

- workspace ownership metadata must remain `.gitignore`d and machine-local
- workspace ontology must not encode forge/provider brand names
- workspace-kind inference must remain overridable by explicit declaration
- first pass must not require network access or hosted forge integration
- first pass must not block read-only inspection use cases

## Recommended first pass

### Policy choices

#### Source of truth
Use **both**:
- per-workspace lease file
- project-level local registry

Reason:
- local workspace self-description is useful even if registry is temporarily stale
- registry gives operator and supervisor visibility

#### Conflict default
Default second mutable attach to:
- suggest sibling worktree/workspace creation
- allow read-only attach
- require explicit action for stale-adopt or override

#### Authority gate
First pass should be:
- **hard refusal by default** for release/benchmark-sensitive actions outside proper role context
- with explicit operator override only where necessary

That is safer than warn-only and better matches the release ambiguity failures already observed.

## Rollout recommendation

### Stage 1 — runtime metadata only
- implement lease + registry data model
- no UX beyond logging and internal checks

### Stage 2 — startup enforcement
- TUI + daemon admission checks
- conflict response surfaced to operator

### Stage 3 — cleave unification
- child workspaces allocated through shared registry

### Stage 4 — authority enforcement
- RC / benchmark-sensitive operations gated by workspace role and explicit ref discipline

## Success criteria

The first pass is successful if all of the following are true:
- a second mutable Omegon session cannot silently share one workspace
- cleave children register as distinct workspaces with explicit roles
- release/benchmark flows can no longer accidentally conflate tagged RC state with later `main` commits
- non-code/plaintext workspaces remain first-class and are not forced into code-project assumptions
- the model remains forge-neutral and sovereign by default
