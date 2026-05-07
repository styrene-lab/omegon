+++
id = "8471f8bd-d2f0-4545-8363-5cf9b65f6fe6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Embedded web dashboard — Omegon-local browser compatibility surface served from the binary

## Overview

The TUI dashboard panel is constrained to ~36 columns of text. For complex lifecycle operations — dependency graph traversal, spec-to-task traceability, multi-change OpenSpec funnels, cleave timeline inspection — operators need a richer browser surface. Auspex is the primary browser experience for that role. This document covers the narrower question of how Omegon still serves an in-process localhost compatibility surface from the binary without introducing a heavy build pipeline or separate process.

## Research

### Scope note — embedded compatibility surface versus Auspex browser portal

This node is the Omegon-local `/dash` surface served from the omegon binary. It is a local compatibility/debug browser surface, not the primary browser portal. Auspex owns the primary browser/project-intelligence experience tracked by `markdown-viewport`.

### Approach survey — 6 options evaluated

**Option A: Axum + Dioxus WASM SPA**
Dioxus compiles to WASM, served from axum at `/dashboard`. Full Rust component model, signals-based reactivity. Embed WASM bundle via `include_bytes!`. Requires wasm-pack + wasm-opt build step, separate `wasm32-unknown-unknown` target. WASM output ~200-500KB for a dashboard app. Binary size increase ~2-3MB (axum+dioxus deps). Build time: significant — WASM compilation is slow. Prior art: the markdown-viewport design node already decided on this for mdserve's intelligence layer. Verdict: **overkill for an embedded dashboard — save for mdserve**.

**Option B: Axum + HTMX (server-rendered HTML)**
HTMX is 14KB (min+gz), does partial page updates via HTTP. Server renders HTML via template engine (minijinja/askama). No client-side JS framework. Very lightweight binary impact. Drawback: every interaction requires a round-trip to the server. Fine for simple dashboards but awkward for interactive graph exploration (pan/zoom/expand nodes). Verdict: **good for forms and lists, wrong for graphs**.

**Option C: Axum + Preact/HTM standalone (no build step)**
preact-htm-signals-standalone: single JS file (~15KB min), provides React-like components + signals for reactive state. No build step, no CDN, no node_modules. Embed via `include_str!`. D3.js (~90KB min+gz) or force-graph.js (already vendored in mdserve fork) for graph visualization. Total embedded asset: ~120KB. Binary size increase: ~1MB (axum + tower-http, tokio already a dep). Build time: zero extra — no WASM compilation. Prior art: `claude-hindsight` crate uses exactly this pattern (ratatui TUI + axum + embedded React frontend). Verdict: **sweet spot — React-class interactivity at include_str! simplicity**.

**Option D: Axum + vanilla JS (no framework)**
Vanilla JS with DOM manipulation. No framework overhead. Full control. But: manual state management, no component model, verbose for reactive UIs. D3.js still needed for graphs. Total embedded: ~100KB (D3 + custom JS). Verdict: **viable but tedious — Preact/HTM is only 15KB more for much better DX**.

**Option E: Leptos (Rust WASM framework)**
Leptos has fine-grained reactivity and SSR support. Lighter than Dioxus. But still requires WASM build pipeline (cargo-leptos, wasm-pack). WASM output ~150-300KB. Same build complexity as Dioxus. Verdict: **same overkill problem as Option A**.

**Option F: External mdserve (existing plan)**
Spawn mdserve as a subprocess. Clean separation. But: reads filesystem not in-memory state, no real-time cleave progress without IPC, requires separate installation. Verdict: **right for document/intelligence layers, wrong for live agent dashboard**.

### Binary size and dependency analysis

Current omegon binary: 11.3MB release.
- tokio: already a dependency (full features)
- hyper: already pulled transitively by reqwest (for LLM providers)
- axum: marginal cost is axum itself + tower-http (~500KB after tree shaking)
- tower: already pulled by reqwest

Estimated binary size with axum + embedded assets:
- axum + tower-http: +500KB-1MB
- Preact+HTM+Signals standalone: ~15KB
- D3.js minified: ~90KB (or force-graph.js: ~50KB — already vendored in mdserve)
- Custom dashboard CSS: ~5KB
- Custom dashboard JS: ~10-20KB

Total increase: ~1.5MB → binary goes from 11.3MB to ~12.8MB.

For comparison:
- claude-hindsight: uses rust-embed to bake a Next.js build into the binary. Their frontend build is ~1-2MB of JS/CSS. Much heavier than our approach.
- mdserve fork: 5.8MB binary with Mermaid.js + force-graph.js vendored.

The Preact/HTM approach is dramatically lighter than any WASM or full-framework option.

### Real-time state sharing architecture

The key advantage of embedding the server in the omegon binary: the web dashboard reads the **same Arc<Mutex<>> handles** that the TUI dashboard reads. No filesystem polling, no IPC protocol, no serialization delay.

```
omegon binary process
├── Agent loop task (tokio)
│   └── EventBus → Features → Arc<Mutex<LifecycleContextProvider>>
│                            → Arc<Mutex<CleaveProgress>>
├── TUI task (tokio)
│   └── draw() → DashboardHandles.refresh_into() → renders terminal
└── Web server task (tokio, started by /dash open)
    ├── GET /api/state → reads same Arc handles → JSON
    ├── WS /ws → tokio::sync::watch channel fed by bus events
    └── GET / → include_str!("dashboard.html")
```

The WebSocket push is trivial: we already have `broadcast::channel<AgentEvent>` for the TUI. The web server subscribes to the same channel. On each event, push a JSON snapshot to all connected WebSocket clients.

State snapshot JSON shape (matches what the TUI dashboard renders):
```json
{
  "design": {
    "counts": { "total": 139, "implemented": 100, ... },
    "focused": { "id": "...", "title": "...", "status": "...", "questions": 3 },
    "implementing": [{ "id": "...", "title": "...", "branch": "..." }],
    "actionable": [{ "id": "...", "status": "exploring", "questions": 2 }]
  },
  "openspec": {
    "changes": [{ "name": "...", "stage": "implementing", "done": 5, "total": 8 }]
  },
  "cleave": {
    "active": true,
    "children": [{ "label": "...", "status": "running", "duration": 12.3 }]
  },
  "session": { "turns": 15, "tool_calls": 42, "model": "claude-sonnet-4-6" }
}
```

### Graph rendering options for the browser

The design tree is a DAG (directed acyclic graph) with ~139 nodes, parent/child edges, dependency edges, and related-node edges. The OpenSpec pipeline is a linear funnel. The cleave timeline is a Gantt-like parallel execution view.

**force-graph.js** (50KB, already vendored in mdserve fork)
- 3D and 2D force-directed layout
- WebGL rendering, handles 1000+ nodes
- Excellent for the design tree dependency graph
- Already proven in the mdserve wikilink graph view

**D3.js** (~90KB minified)
- Industry standard for data visualization
- d3-hierarchy for tree layouts (perfect for parent-child design tree)
- d3-sankey for OpenSpec pipeline funnel
- d3-timeline for cleave execution
- Heavier but more versatile

**vis.js Network** (~170KB minified)
- Interactive graph with drag, zoom, clustering
- Built-in hierarchical layout
- Heavier than force-graph.js but more features

**ELK.js** (Layered graph layout algorithm, ~300KB)
- The same layout algorithm we use for D2 diagrams
- Excellent hierarchical layouts
- Too heavy for an embedded dashboard

**Recommendation: force-graph.js for the design tree graph, vanilla SVG for the OpenSpec funnel and cleave timeline.**
force-graph.js is already proven, small, and handles our node count. The funnel and timeline are simple enough for hand-rolled SVG (no layout algorithm needed — they're linear/parallel, not graph-shaped).

### Implementation status (March 2026)

**Backend complete (884 LoC Rust, 10 tests):**
- Axum server with `/api/state`, `/api/graph`, `/ws`, `/` routes
- WebSocket bidirectional: all AgentEvents pushed, inbound commands (user_prompt, slash_command, cancel)
- Random auth token per session (required for WS connection)
- `WebState` shares `Arc<Mutex<>>` handles with TUI for zero-copy state
- `/dash open` starts server + opens browser, `/dash` toggles TUI panel
- `HarnessStatusChanged` event broadcast over WS

**Frontend (42KB embedded HTML):**
- Single HTML with Alpharius theme CSS custom properties
- Real-time WebSocket consumer
- Panels: session stats, routing, design tree counts, openspec changes, cleave progress

**Not yet implemented:**
- force-graph.js for interactive design tree DAG visualization (backend serves graph data via `/api/graph`, frontend doesn't render it yet)
- Preact/HTM should be vendored as separate files for offline use (currently CDN or inline reference)
- OpenSpec pipeline funnel SVG
- Cleave Gantt-style timeline SVG

## Decisions

### Decision: Option C: Axum + Preact/HTM standalone — no build step, no WASM, ~120KB embedded assets

**Status:** decided
**Rationale:** Preact+HTM+Signals is 15KB for a React-class component model with reactive signals. No build step — single JS file, vendored via include_str!. D3.js or force-graph.js for graphs. Total embedded assets ~120KB. Binary size increase ~1.5MB (axum is marginal since tokio/hyper already deps). Build time unchanged — no WASM compilation. Claude-hindsight crate validates this exact pattern (ratatui + axum + embedded frontend in single binary). HTMX was considered but lacks the interactivity needed for graph exploration. Dioxus/Leptos WASM was considered but the build pipeline cost is disproportionate to the dashboard scope — save those for the mdserve intelligence portal.

### Decision: Same Arc handles for TUI and web — zero-copy state sharing

**Status:** decided
**Rationale:** The web server runs as a tokio task inside the omegon binary. It reads the same Arc<Mutex<LifecycleContextProvider>> and Arc<Mutex<CleaveProgress>> handles the TUI dashboard reads. No filesystem polling, no IPC serialization. WebSocket push via a subscription to the existing broadcast::channel<AgentEvent>. The /api/state endpoint serializes a snapshot from the shared handles on demand.

### Decision: force-graph.js for design tree graph, vanilla SVG for funnel and timeline

**Status:** decided
**Rationale:** force-graph.js (50KB) is already proven in the mdserve fork, handles our 139-node design tree easily, and provides interactive pan/zoom/select. The OpenSpec pipeline funnel and cleave execution timeline are linear/parallel structures that don't need graph layout — hand-rolled SVG with simple positioning is cleaner and zero-dependency.

### Decision: Auspex is the primary browser surface; `/dash open` remains the Omegon-local compatibility path and `/dash` raises the TUI panel

**Status:** decided
**Rationale:** The TUI dashboard panel (right side, 36 cols) shows ambient status — focused node, active changes, cleave progress, session stats. `/dash` toggles between compact (no panel) and raised (panel visible). `/dash open` starts the embedded axum server on localhost and opens the default browser as a local compatibility/debug surface. The primary browser workflow should move to Auspex, but both browser surfaces can read the same underlying lifecycle data. The TUI panel is for glancing; the browser is for deeper inspection.

### Decision: WebSocket protocol is the full agent interface — bidirectional events + commands

**Status:** decided
**Rationale:** The WebSocket connection carries ALL AgentEvents (turn lifecycle, streaming text, tool calls, lifecycle phase changes, decomposition progress) plus accepts inbound commands (user prompts, slash commands, cancel, model switch). This makes the omegon binary a black-box backend — any web UI can connect to ws://localhost:PORT/ws and drive the full agent experience. The embedded Preact dashboard is just the first consumer. A future full web UI (React/Next.js/Dioxus) can replace it without touching the backend. The protocol is JSON-over-WebSocket with typed event/command envelopes.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `crates/omegon/src/web/mod.rs` (new) — Axum web server — routes, state, startup/shutdown
- `crates/omegon/src/web/api.rs` (new) — JSON API endpoints — GET /api/state, GET /api/design-tree, GET /api/openspec
- `crates/omegon/src/web/ws.rs` (new) — WebSocket handler — push state updates on AgentEvent broadcast
- `crates/omegon/src/web/assets/dashboard.html` (new) — Single HTML file with embedded Preact/HTM components — design tree graph, OpenSpec funnel, cleave timeline, session stats
- `crates/omegon/src/web/assets/vendor/preact-htm-signals.js` (new) — Vendored Preact+HTM+Signals standalone (~15KB)
- `crates/omegon/src/web/assets/vendor/force-graph.min.js` (new) — Vendored force-graph.js (~50KB)
- `crates/omegon/src/tui/mod.rs` (modified) — Add /dash command handling — toggle panel, /dash open starts web server
- `crates/omegon/src/tui/dashboard.rs` (modified) — Enrich with status counts, pipeline funnel, actionable nodes, implementing nodes
- `crates/omegon/Cargo.toml` (modified) — Add axum, tower-http (serve-static feature) dependencies
- `core/crates/omegon/src/web/assets/dashboard.html` (new) — 42KB single-page dashboard with Alpharius theme, real-time WebSocket updates, session/routing/design/openspec/cleave panels. Preact/HTM referenced, no force-graph yet.

### Constraints

- Total embedded assets must be under 200KB
- Web server starts on demand (/dash open), not at startup — zero overhead when not used
- Port auto-increment if default (7842) is busy
- WebSocket reconnects automatically on disconnect
- Dashboard HTML must work offline — no CDN dependencies
- Alpharius theme colors applied via CSS custom properties loaded from the same themes/alpharius.json
- force-graph.js for interactive design tree DAG not yet vendored — dashboard works without it, follow-on enhancement
- Preact/HTM not vendored as separate files — referenced from CDN or inlined in HTML; should be vendored for offline use per constraint
