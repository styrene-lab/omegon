+++
id = "85ec964e-a501-4759-b998-c2428619cf81"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Embedded web dashboard — lightweight localhost UI served from the omegon binary — Design Spec (extracted)

> Auto-extracted from docs/embedded-web-dashboard.md at decide-time.

## Decisions

### Option C: Axum + Preact/HTM standalone — no build step, no WASM, ~120KB embedded assets (decided)

Preact+HTM+Signals is 15KB for a React-class component model with reactive signals. No build step — single JS file, vendored via include_str!. D3.js or force-graph.js for graphs. Total embedded assets ~120KB. Binary size increase ~1.5MB (axum is marginal since tokio/hyper already deps). Build time unchanged — no WASM compilation. Claude-hindsight crate validates this exact pattern (ratatui + axum + embedded frontend in single binary). HTMX was considered but lacks the interactivity needed for graph exploration. Dioxus/Leptos WASM was considered but the build pipeline cost is disproportionate to the dashboard scope — save those for the mdserve intelligence portal.

### Same Arc handles for TUI and web — zero-copy state sharing (decided)

The web server runs as a tokio task inside the omegon binary. It reads the same Arc<Mutex<LifecycleContextProvider>> and Arc<Mutex<CleaveProgress>> handles the TUI dashboard reads. No filesystem polling, no IPC serialization. WebSocket push via a subscription to the existing broadcast::channel<AgentEvent>. The /api/state endpoint serializes a snapshot from the shared handles on demand.

### force-graph.js for design tree graph, vanilla SVG for funnel and timeline (decided)

force-graph.js (50KB) is already proven in the mdserve fork, handles our 139-node design tree easily, and provides interactive pan/zoom/select. The OpenSpec pipeline funnel and cleave execution timeline are linear/parallel structures that don't need graph layout — hand-rolled SVG with simple positioning is cleaner and zero-dependency.

### /dash open starts the web server, /dash raises the TUI panel — two surfaces, one data model (decided)

The TUI dashboard panel (right side, 36 cols) shows ambient status — focused node, active changes, cleave progress, session stats. /dash toggles between compact (no panel) and raised (panel visible). /dash open starts the embedded axum server on localhost and opens the default browser — this gives the full interactive view with design tree graph, OpenSpec funnel, cleave timeline, and dependency exploration. Both surfaces read the same data. The TUI panel is for glancing; the browser is for working.

## Research Summary

### Approach survey — 6 options evaluated

**Option A: Axum + Dioxus WASM SPA**
Dioxus compiles to WASM, served from axum at `/dashboard`. Full Rust component model, signals-based reactivity. Embed WASM bundle via `include_bytes!`. Requires wasm-pack + wasm-opt build step, separate `wasm32-unknown-unknown` target. WASM output ~200-500KB for a dashboard app. Binary size increase ~2-3MB (axum+dioxus deps). Build time: significant — WASM compilation is slow. Prior art: the markdown-viewport design node already decided on this for mdserve's …

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
-…

### Real-time state sharing architecture

The key advantage of embedding the server in the omegon binary: the web dashboard reads the **same Arc<Mutex<>> handles** that the TUI dashboard reads. No filesystem polling, no IPC protocol, no serialization delay.

```
omegon binary process
├── Agent loop task (tokio)
│   └── EventBus → Features → Arc<Mutex<LifecycleContextProvider>>
│                            → Arc<Mutex<CleaveProgress>>
├── TUI task (tokio)
│   └── draw() → DashboardHandles.refresh_into() → renders terminal
└── Web server …

### Graph rendering options for the browser

The design tree is a DAG (directed acyclic graph) with ~139 nodes, parent/child edges, dependency edges, and related-node edges. The OpenSpec pipeline is a linear funnel. The cleave timeline is a Gantt-like parallel execution view.

**force-graph.js** (50KB, already vendored in mdserve fork)
- 3D and 2D force-directed layout
- WebGL rendering, handles 1000+ nodes
- Excellent for the design tree dependency graph
- Already proven in the mdserve wikilink graph view

**D3.js** (~90KB minified)
- Ind…
