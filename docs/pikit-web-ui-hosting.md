---
id: pikit-web-ui-hosting
title: pi-kit self-hosted web UI
status: implemented
parent: repo-consolidation-hardening
openspec_change: localhost-web-ui-mvp
open_questions: []
---

# pi-kit self-hosted web UI

## Overview

> Parent: [Repo Consolidation, Security Hardening, and Lifecycle Normalization](repo-consolidation-hardening.md)
> Spawned from: "How could pi-kit host its own web UI for status, lifecycle visibility, and operator control?"

Implement a first-party localhost-only, read-only web UI for pi-kit that exposes a versioned control-plane snapshot and a lightweight polling dashboard shell without adding mutation routes or a separate persistence layer.

## Research

### Current web-capable surface already in repo

pi-kit already hosts one browser-facing surface through `extensions/vault/index.ts`, which spawns `mdserve` on localhost and opens a browser view for markdown docs and graph navigation. That proves the package can manage a local companion web process today, but it is document-oriented rather than app-oriented: there is no generic HTTP API, no websocket/SSE event stream, and no reusable browser state model. A future web UI can either (A) extend this companion-process pattern, or (B) add a first-party local HTTP server extension inside pi-kit.

### Most natural first architecture options

Option 1: 'Read-only dashboard server' — a lightweight localhost HTTP server exposes normalized JSON for dashboard, design-tree, OpenSpec, cleave, memory, and model-routing state plus a simple HTML UI. Lowest risk and best fit for current architecture. Option 2: 'Evented operator console' — add SSE/WebSocket updates and limited write actions such as toggling dashboard mode, focusing design nodes, or kicking `/opsx:status`; more powerful but needs a command authorization model. Option 3: 'mdserve-plus' — keep markdown rendering in mdserve and add sidecar JSON endpoints for live state; attractive for docs-heavy UX but couples app UX to an external binary. Option 4: 'full SPA control plane' — browser app with richer navigation and charts; likely overkill before a stable API contract exists.

### Key design constraints

The web UI should remain localhost-only by default, reuse existing subsystem state instead of inventing parallel stores, avoid shelling out to broad process managers, and separate read-only status APIs from mutating operator actions. The cleanest backend seam is a canonical 'control-plane state' resolver that already normalizes dashboard/shared-state, design-tree, OpenSpec, lifecycle assessment, and model-routing data; both TUI and browser UI should render from that same resolver.

### Canonical state model should precede HTTP transport

The existing TUI already has a partial shared process model in `extensions/shared-state.ts` (`designTree`, `openspec`, `cleave`, `effort`, `routingPolicy`, `recovery`, memory injection metrics). That is enough for a first browser dashboard, but it is incomplete and somewhat presentation-shaped. A web UI should define a first-class `ControlPlaneState` snapshot with explicit sections such as `session`, `dashboard`, `designTree`, `openspec`, `cleave`, `models`, `memory`, and `health`. The HTTP layer should only serialize that normalized snapshot; transport should not directly expose raw mutable globals.

### Proposed ControlPlaneState MVP schema

A read-only MVP can stay narrow and still be useful. Proposed top-level shape: `session` (cwd, pid, startedAt, uiMode if known), `dashboard` (mode, updatedAt), `designTree` (focusedNode, counts, implementingNodes, active nodes summary), `openspec` (active changes with stage/tasks/artifacts/specDomains), `cleave` (status, runId, child status summary, updatedAt), `models` (effort state, routing policy, recovery summary), `memory` (token estimate and last injection metrics summary), and `health` (web UI status, vault/mdserve status, last refresh timestamp). The snapshot should be explicitly versioned, e.g. `schemaVersion: 1`, so the browser contract can evolve independently from raw internal types.

### Proposed HTTP surface for MVP

Recommended MVP routes: `GET /` serves a tiny built-in HTML dashboard; `GET /api/state` returns the full normalized `ControlPlaneState`; `GET /api/health` returns lightweight liveness and port/binding info; `GET /api/design-tree`, `/api/openspec`, `/api/cleave`, `/api/models`, and `/api/memory` return the corresponding slices for debugging and future UI decomposition. Phase 2 can add `GET /api/events` via Server-Sent Events so the browser can subscribe to dashboard updates without polling. Mutating routes should be explicitly out of scope for v1.

### Update and rendering model

The cleanest update model is snapshot-first, event-second. The server should be able to build a fresh `ControlPlaneState` on every request from normalized shared state plus filesystem-backed sources (design-tree and OpenSpec scans where needed). For the HTML page, start with light polling of `/api/state` every 1-2 seconds; add SSE only after the state contract settles. This keeps the first implementation simple and avoids over-committing to websocket/event semantics before operator actions exist.

### Security model for the browser surface

The MVP should bind only to `127.0.0.1` by default, choose a configurable high port, and expose read-only JSON plus static HTML only. No command execution endpoints, no shell passthrough, no filesystem browsing beyond normalized state, and no implicit LAN exposure. If later operator actions are added, they should require an explicit opt-in mode plus a per-session capability token or equivalent CSRF-resistant confirmation boundary, rather than trusting same-machine browser origin alone.

## Decisions

### Decision: Explore a localhost read-only web dashboard first

**Status:** decided
**Rationale:** The repo already has enough structured state to power a browser UI, but not yet a hardened command-authorization layer for remote mutation. Starting with a read-only localhost dashboard minimizes risk, reuses existing dashboard/lifecycle data, and forces normalization of a canonical state model before adding write operations.

### Decision: Use a first-party localhost HTTP extension instead of extending mdserve

**Status:** decided
**Rationale:** mdserve proves the companion-process pattern but is optimized for document rendering, not control-plane APIs or live status composition. A first-party localhost HTTP extension keeps pi-kit in control of the API contract, security defaults, and browser UI evolution while still allowing docs to link out to mdserve where helpful.

### Decision: Adopt a versioned ControlPlaneState and read-only HTTP surface for MVP

**Status:** decided
**Rationale:** A versioned normalized snapshot gives pi-kit one browser-facing contract that can also discipline the TUI data model. Starting with `GET /`, `GET /api/state`, and small read-only slice routes keeps implementation simple, avoids premature write-authority design, and still delivers immediate value for lifecycle visibility and operator awareness.

### Decision: Compute MVP web state from live sharedState plus on-demand scans, not a separate history store

**Status:** decided
**Rationale:** A localhost MVP should stay stateless and derive its snapshot from existing in-process shared state plus design-tree/OpenSpec scans when necessary. Adding a dedicated browser timeline store would duplicate authority, increase lifecycle drift risk, and complicate security and maintenance before the UI contract is proven.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/web-ui/types.ts` (new) — Define the versioned `ControlPlaneState` contract and slice types
- `extensions/web-ui/state.ts` (new) — Build normalized snapshots and slices from shared state, OpenSpec, and design-tree scans
- `extensions/web-ui/server.ts` (new) — Implement the localhost-only HTTP server, dashboard shell delivery, and read-only route handling
- `extensions/web-ui/static/index.html` (new) — Serve the minimal polling-first HTML dashboard shell
- `extensions/web-ui/http.test.ts` (new) — Cover state, slices, mutation refusal, polling semantics, and 404 behavior
- `extensions/web-ui/server.test.ts` (new) — Cover server lifecycle and localhost binding
- `extensions/web-ui/state.test.ts` (new) — Cover snapshot schema and live-state derivation
- `extensions/web-ui/index.ts` (new) — Register `/web-ui` command lifecycle actions and browser-open behavior
- `extensions/web-ui/index.test.ts` (new) — Cover command surface behavior and shutdown cleanup
- `package.json` (modified) — Register the new `web-ui` extension in `pi.extensions`
- `README.md` (modified) — Document localhost web UI commands, routes, and security defaults

### Constraints

- Bind to `127.0.0.1` only; no LAN exposure in the MVP.
- Expose read-only routes only in v1; no command execution or mutating endpoints.
- Use a versioned `ControlPlaneState` schema and normalize internal state before serialization.
- Prefer polling over SSE/WebSocket for the first implementation.
- Avoid introducing a second persistence layer for browser history or session timelines.
