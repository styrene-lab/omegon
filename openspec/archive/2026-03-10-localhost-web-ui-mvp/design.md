+++
id = "a539253a-6b7e-4df4-97b1-498ed0ef5ebf"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# localhost-web-ui-mvp — Design

## Spec-Derived Architecture

### web-ui

- **Start a localhost-only web UI server** (added) — 2 scenarios
- **Serve a read-only dashboard shell** (added) — 1 scenarios
- **Expose a versioned ControlPlaneState snapshot** (added) — 2 scenarios
- **Expose read-only slice routes for debugging and composition** (added) — 1 scenarios
- **Reject unsupported mutation routes in MVP** (added) — 2 scenarios
- **Use polling-first browser updates** (added) — 1 scenarios
- **Command surface for web UI lifecycle** (modified) — 2 scenarios
- **HTML shell delivery strategy** (modified) — 1 scenarios

## Scope

Implement a first-party `web-ui` extension that serves a localhost-only, read-only HTTP dashboard over a lightweight polling model. The MVP exposes a versioned `ControlPlaneState` snapshot and slice routes derived from live shared state plus on-demand design-tree and OpenSpec scans. It does not add mutation endpoints, websockets, authentication, or a separate persistence layer.

## File Changes

- `extensions/web-ui/types.ts` — versioned `ControlPlaneState` contract and section types
- `extensions/web-ui/state.ts` — snapshot and slice builders from shared state, OpenSpec, and design docs
- `extensions/web-ui/server.ts` — localhost-only HTTP server, read-only routes, shell delivery, mutation refusal
- `extensions/web-ui/static/index.html` — transport-light polling dashboard shell
- `extensions/web-ui/http.test.ts` — route coverage for state, slices, mutation refusal, polling, and 404 behavior
- `extensions/web-ui/server.test.ts` — lifecycle and localhost binding coverage
- `extensions/web-ui/state.test.ts` — snapshot contract and live-state derivation coverage
- `extensions/web-ui/index.ts` — `/web-ui` command surface for start, stop, status, and open
- `extensions/web-ui/index.test.ts` — command lifecycle and browser-open behavior coverage
- `package.json` — registers the extension in pi-kit startup
- `README.md` — documents the localhost-only web UI MVP
