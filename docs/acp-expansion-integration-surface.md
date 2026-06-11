
+++
id = "d9eed35c-0139-42b3-8e80-28e80cc3e722"
kind = "design_node"

[data]
title = "ACP Expansion and Integration Surface"
status = "decided"
issue_type = "epic"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

ACP is already Omegon's canonical rich-client integration surface for Zed, Flynt, and future editor/IDE clients. The next ACP expansion should extend that existing product API with lifecycle and UI semantic read projections first, not create a separate lifecycle protocol or client-specific side channel.

Recent lifecycle extraction work created protocol-independent backends that ACP can call directly:

- `lifecycle/archive.rs` — OpenSpec archive transaction/recovery behavior
- `lifecycle/sync.rs` — markdown/OpenSpec ↔ `omegon-opsx` FSM synchronization
- `lifecycle/query.rs` — design-tree query projections such as ready/blocked/frontier
- `lifecycle/mutation.rs` — service-backed design-tree mutations, including `implement`
- `lifecycle/read_model.rs` — joined lifecycle/OpenSpec/drift projection surface

Recent UI extraction work also created semantic surfaces under `core/crates/omegon/src/surfaces/` for conversation, dashboard, editor, footer, instruments, and layout. Those are the right source for ACP UI-facing DTOs because they are independent of Ratatui rendering and ACP wire details.

## Existing ACP/Zed/Flynt surface

This is not a greenfield ACP design. Existing repository surfaces already establish the integration model:

- `docs/acp-surface.md` defines ACP as Omegon's canonical rich editor/client integration path outside the TUI. Zed is the primary compatibility target; Flynt should share the same Omegon ACP semantics.
- `docs/zed-integration.md` documents Zed spawning `omegon acp`, model/thinking/posture dropdowns, host delegation for file/terminal/approval, MCP forwarding, slash commands, and session title behavior.
- `docs/flynt-integration.md` documents Flynt spawning Omegon over ACP, `.flynt/operator-settings.json`, plan updates, session titles, and ext_method settings/control surfaces.
- `core/crates/omegon/src/acp.rs` already implements underscore-prefixed ext_method surfaces such as `_runtime/status`, `_runtime/capabilities`, and `_extensions/call`.
- `core/crates/omegon/src/surfaces/mod.rs` already declares semantic projections for `conversation`, `dashboard`, `editor`, `footer`, `instruments`, and `layout`.
- `docs/tui-acp-conversation-projection-seam.md` already decides that ACP should receive concrete serializable DTOs derived from semantic projections, not raw TUI segments or Ratatui render traits.

Therefore the correct architecture is:

```text
Zed / Flynt / future ACP clients
        ↓ ACP ext_method / ACP events
Omegon ACP bridge
        ↓
semantic projections + lifecycle read/mutation services
```

The wrong architecture would be a new lifecycle-specific protocol, client-specific Flynt/Zed branches, or ACP methods that call tool handlers such as `design_tree_update` instead of domain services.

## Architectural model

ACP should be a thin adapter over stable read models and domain services:

| ACP concern | Backend |
|---|---|
| runtime capabilities/status | existing ACP runtime surfaces |
| extension/provider/secrets/package status | existing ACP ext_method surfaces |
| lifecycle snapshot/status | `LifecycleReadHandle` |
| ready/blocked/frontier design queries | `lifecycle::query` |
| lifecycle drift/doctor findings | `lifecycle::doctor` / `LifecycleReadHandle` |
| design-tree mutations | `LifecycleMutationService` |
| OpenSpec archive recovery status | `lifecycle::archive` / doctor findings |
| conversation UI state | `surfaces::conversation` |
| dashboard state | `surfaces::dashboard` |
| editor/input state | `surfaces::editor` |
| footer/instrument state | `surfaces::{footer,instruments}` |

Adapter responsibilities:

- ACP method routing and capability advertisement
- request/response DTO versioning
- protocol error mapping
- redaction and permission gates
- client compatibility shims only where needed

Backend responsibilities:

- lifecycle semantics
- markdown/OpenSpec store mutation
- `omegon-opsx` coordination
- semantic projection construction
- provider/runtime state ownership

## First pass: headless read-only ACP lifecycle expansion

ACP clients will usually be talking to a headless `omegon acp` process, not an interactive TUI instance. Therefore the first implementation pass should expose read-only lifecycle/runtime projections that exist in headless mode. UI surface reads are deferred unless they describe durable/session semantic state that is actually present in the ACP worker.

### Capability advertisement

Extend `_runtime/capabilities` to advertise new surfaces and versions:

```json
{
  "_lifecycle/snapshot": { "version": 1 },
  "_lifecycle/design/list": { "version": 1 },
  "_lifecycle/design/get": { "version": 1 },
  "_lifecycle/design/ready": { "version": 1 },
  "_lifecycle/design/blocked": { "version": 1 },
  "_lifecycle/design/frontier": { "version": 1 }
}
```

Do not remove existing runtime/extension/config capabilities. Do not advertise UI snapshot surfaces in the first pass unless the ACP/headless runtime can actually provide them without TUI state.

### `_lifecycle/snapshot`

Backend: `LifecycleReadHandle::snapshot`.

Purpose: one reconnect-safe lifecycle summary for ACP clients.

Response shape should include:

- OpenSpec changes with lifecycle state, file stage, task counts, archived flag, and optional spec summaries
- total/done task counts
- drift findings
- tasking projection when available

Initial options:

```json
{
  "include_archived": false,
  "include_specs": false
}
```

### `_lifecycle/design/list`

Backend: provider/design nodes plus lifecycle query policy.

Purpose: list active design nodes with lightweight metadata.

Response fields:

- `id`
- `title`
- `status`
- `parent`
- `tags`
- `open_questions`
- `dependencies`
- `branches`
- `openspec_change`
- `priority`
- `issue_type`
- `children`

Exclude archived nodes by default. Add `include_archived` later if needed.

### `_lifecycle/design/get`

Backend: provider/design node plus `design::read_node_sections`.

Request:

```json
{ "node_id": "..." }
```

Response should mirror the existing `design_tree { action: node }` semantics but as a stable ACP DTO:

- node metadata
- children summary
- overview
- research
- decisions
- implementation file scope
- implementation constraints
- readiness summary

### `_lifecycle/design/ready`

Backend: `lifecycle::query::ready`.

Purpose: show implementation-ready design nodes.

Response fields:

- `id`
- `title`
- `priority`

### `_lifecycle/design/blocked`

Backend: `lifecycle::query::blocked`.

Response fields:

- `id`
- `title`
- `status`
- `blocked_by`

### `_lifecycle/design/frontier`

Backend: `lifecycle::query::frontier`.

Response fields:

- `id`
- `title`
- `status`
- `open_questions`

### Deferred: UI/dashboard surfaces

Do not include UI/dashboard reads in the first pass. In ACP headless mode there may be no TUI dashboard, selected segment, footer, instrument panel, or editor textarea state to read. Exposing those methods too early would either return empty placeholders or accidentally couple ACP to TUI internals.

A later UI-adjacent ACP pass can expose only headless-valid semantic state, for example:

- conversation/session transcript metadata already owned by the ACP worker
- active turn/tool-call status
- plan state streamed through ACP session updates
- durable lifecycle summary derived from `LifecycleReadHandle`
- runtime/config/provider/extension status already owned by ACP

If Flynt wants dashboard cards, the right first model is for Flynt to compose them client-side from `_runtime/status`, `_lifecycle/snapshot`, and existing plan/tool/session events. Omegon should not expose `_ui/dashboard/snapshot` until there is a real headless dashboard projection service.

## First-pass implementation plan

1. Add ACP DTO helpers near the existing ACP ext_method routing, or in a small `acp/lifecycle_surfaces` module if `acp.rs` grows too much.
2. Extend `_runtime/capabilities` with the read-only lifecycle/UI methods above.
3. Add ACP route arms for `_lifecycle/snapshot`, `_lifecycle/design/list`, `_lifecycle/design/get`, `_lifecycle/design/ready`, `_lifecycle/design/blocked`, `_lifecycle/design/frontier`.
4. Use `LifecycleReadHandle`, `lifecycle::query`, and `design::read_node_sections`; do not call `design_tree` tool handlers.
5. Do not add `_ui/surfaces/status` or `_ui/dashboard/snapshot` in this first pass; document them as deferred/headless-dependent.
6. Add tests in `acp.rs` for each method using temporary lifecycle fixtures.
7. Validate with:

```text
cargo test -p omegon --bin omegon acp_lifecycle
cargo test -p omegon --bin omegon runtime_capabilities
cargo test -p omegon --bin omegon lifecycle
cargo check -p omegon
```

If exact filters differ, use broader filters:

```text
cargo test -p omegon --bin omegon acp
cargo test -p omegon --bin omegon lifecycle
```

## Future write expansion

After the read-only DTOs stabilize, expose service-backed writes:

```text
_lifecycle/design/create
_lifecycle/design/set_status
_lifecycle/design/add_question
_lifecycle/design/add_decision
_lifecycle/design/implement
```

All write methods should call `LifecycleMutationService` directly. They should not call tool handlers. They need explicit permission/capability policy and stable error codes before being considered product API.

## OpenSpec expansion

OpenSpec read status can be included through lifecycle snapshot immediately. Broad OpenSpec mutation should wait for a dedicated OpenSpec service boundary comparable to `LifecycleMutationService`.

Safe early reads:

```text
_openspec/status
_openspec/get
_openspec/drift
```

Deferred writes:

```text
_openspec/propose
_openspec/add_spec
_openspec/register_tasks
_openspec/register_test_file
_openspec/set_task_status
_openspec/archive
```

## Risk assessment

Low risk:

- lifecycle/design read projections
- lifecycle snapshot
- runtime capability advertisement

Medium risk:

- lifecycle writes through `LifecycleMutationService`
- headless conversation/session metadata beyond existing ACP events

Higher risk:

- remote UI control commands
- broad OpenSpec mutations
- diagnostics/recent event exposure
- full transcript/session hydration

## Decisions

### Decision: Extend existing Zed/Flynt ACP surface, do not create a parallel protocol

**Status:** decided
**Rationale:** ACP is already the canonical rich-client path for Zed and Flynt. Lifecycle/UI expansion should be new ext_method surfaces and DTOs over existing backend seams.

### Decision: Start with headless-valid read-only lifecycle projections

**Status:** decided
**Rationale:** ACP usually runs against a headless Omegon process. Lifecycle/runtime projections are available there; TUI/dashboard/editor surface reads are not necessarily meaningful without a live UI instance.

### Decision: ACP must call read models/services, not tool handlers

**Status:** decided
**Rationale:** Tool handlers are provider-facing JSON adapters. ACP should use `LifecycleReadHandle`, `lifecycle::query`, semantic UI projections, and `LifecycleMutationService` directly.

### Decision: Defer broad OpenSpec mutation exposure

**Status:** decided
**Rationale:** OpenSpec mutation remains more adapter-heavy than design-tree mutation. Expose reads first and add writes after an OpenSpec service boundary exists.

## Non-goals for the first pass

- No new lifecycle write ACP methods.
- No OpenSpec mutation ACP methods.
- No client-specific Flynt-only or Zed-only lifecycle APIs.
- No UI/dashboard/editor ACP reads in the first pass unless backed by headless runtime state.
- No TUI/Ratatui render structs in ACP DTOs.
- No transcript/session hydration redesign.

## Open Questions

None for the first read-only expansion pass.
