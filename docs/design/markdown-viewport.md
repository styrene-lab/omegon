---
id: markdown-viewport
title: "Omegon Rendering Engine — Lifecycle Visualization & Project Intelligence Layer"
status: decided
related: [omega]
tags: [rendering, rust, visualization, design-tree, openspec, lifecycle, dashboard, project-intelligence]
open_questions: []
issue_type: epic
priority: 2
---

# Omegon Rendering Engine — Lifecycle Visualization & Project Intelligence Layer

## Overview

Pi-kit has organically grown ~75% of a software engineering and project management platform: a design exploration tree, a spec-driven implementation layer (OpenSpec), a parallel execution engine (cleave), a memory system, and a dashboard. The missing piece is a **rendering and visualization layer** — a browser-based surface that turns the code-native lifecycle artifacts (markdown docs, facts.jsonl, tasks.md, specs) into a navigable, queryable, visually rich project intelligence view.

This is not a markdown previewer. It is the **project intelligence portal** for Omegon-powered workflows. The scope spans three levels:

1. **Document layer** — render design nodes, OpenSpec specs, tasks.md, memory facts as interlinked pages with wikilink navigation and live-reload
2. **Lifecycle layer** — visualize the state of the design tree and OpenSpec pipeline as interactive graphs and kanban-style boards; surface open questions, blocked nodes, spec coverage, and verification status
3. **Intelligence layer** — go beyond what the TUI dashboard can show: dependency graphs, spec-to-implementation traceability, cleave execution timelines, memory fact graphs, project health scoring

The rendering backend is a **lightweight Rust binary** (single file, <10MB, no Node.js). It serves the browser UI over localhost HTTP with WebSocket push for live updates. It is spawned by a Omegon extension (`/auspex open`) and is also usable standalone.

## Research

### Prior art — mdserve baseline

mdserve is a Rust single-binary markdown preview server built for AI coding agent workflows. It already has:
- Directory serving with sidebar nav
- WebSocket live-reload on file change
- Catppuccin themes
- ~42 tests, 5.8MB release binary

Gaps from the new scope:
- No wikilink resolution (`[[target|display]]` syntax)
- No graph view
- No lifecycle-aware data model (design nodes, OpenSpec changes, cleave tasks)
- No custom rendering for structured frontmatter (status, tags, open questions, AC)
- No cross-document traceability

The fork-mdserve path gets us the document layer cheaply. Lifecycle and intelligence layers require custom data extraction and frontend components.

### Pi-kit lifecycle artifacts available for rendering

| Artifact | Location | Structure |
|---|---|---|
| Design nodes | `docs/design/*.md`, `docs/*.md` | Frontmatter (id, status, tags, open_questions, dependencies) + structured sections |
| OpenSpec proposals | `openspec/changes/*/proposal.md` | Free-form markdown |
| OpenSpec specs | `openspec/changes/*/specs/**/*.md` | Given/When/Then scenarios, requirements |
| OpenSpec design.md | `openspec/changes/*/design.md` | File scope, decisions, constraints |
| OpenSpec tasks.md | `openspec/changes/*/tasks.md` | Task groups with spec annotations |
| Baseline specs | `openspec/baseline/*.md` | Archived spec contracts |
| Memory facts | `.pi/memory/facts.jsonl` | JSONL: id, section, content, confidence, created_at |
| Memory graph edges | `.pi/memory/facts.db` (runtime) | source_id, target_id, relation |
| Cleave assessment | `openspec/changes/*/assessment.md` | Review results, issue lists |

### What the TUI dashboard can't do

The current dashboard (compact/raised widget) is constrained to ~10 terminal lines. It can surface counts and status badges but not:
- Full dependency graph with blocking chain visualization
- Spec-to-task traceability (which spec scenario covers which task group)
- OpenSpec stage funnel (proposal → spec → design → tasks → implementing → verified → archived)
- Memory fact graph (relations between facts — depends_on, contradicts, enables)
- Cleave child timeline (which children ran, in what order, review outcomes, merge status)
- Project health scoring (open questions per node, spec coverage %, test pass rate)
- Cross-change traceability (which design decision drove which OpenSpec change drove which cleave execution)

### Fork assessment — ~/workspace/ai/mdserve (as of 2026-03-12)

The fork already exists and is substantially ahead of upstream. Last 3 commits:
- `96b8f95` fix: XSS hardening, offline graph, test coverage
- `bb5da97` feat: project-scoped settings via localStorage namespacing  
- `8bb9e8b` feat: wikilinks, graph view, recursive scanning, Styrene theme

**Already in the fork (2514 lines across 4 source files):**
- `app.rs` (2135 lines) — axum HTTP server, WebSocket live-reload, notify file watcher, minijinja template rendering, directory mode with sidebar, port auto-increment
- `graph.rs` (129 lines) — `GraphData { nodes, edges }`, `build_graph()`, `local_graph()` (1-hop neighborhood), force-graph.js already **vendored at compile time** via `include_str!`
- `wikilinks.rs` (182 lines) — slug map builder, `[[target|display]]` parser, edge extraction, resolved/unresolved link rendering
- `main.rs` (68 lines) — clap CLI, single/directory mode dispatch

**Stack**: axum 0.7 + tokio + minijinja (embed templates at compile time) + notify (file watching) + serde_json + Mermaid.js (vendored) + force-graph.js (vendored).

**Distribution**: upstream ships Homebrew formula + cargo install + curl installer script. The fork will need its own Nix flake.

**What this means for architecture**: Document layer is essentially done. The fork just needs lifecycle-aware frontmatter parsing and custom rendering for design node / OpenSpec / tasks.md structure. The remaining work is entirely in the lifecycle + intelligence layers.

### Dioxus assessment — web + desktop targets

Dioxus is a Rust UI framework with three relevant render targets:
- **Web** — compiles to WASM, runs as a SPA in the browser. Full reactive component model, signals-based state.
- **Desktop** — wraps a webview (WKWebView on macOS, WebView2 on Windows, webkit2gtk on Linux). Runs the same WASM app in a native window. No browser required for users who want a native feel.
- **TUI** — crossterm-based terminal rendering. Same component tree, different renderer.

**Why Dioxus for lifecycle/intelligence layers:**
The lifecycle views (kanban board, OpenSpec funnel, design tree graph, memory fact graph) are too complex for minijinja server-side templates. They require reactive state — filtering, expanding nodes, navigating the graph interactively. Dioxus WASM gives us a proper component model in Rust, served from the same axum binary as a compiled WASM bundle at `/app/`.

**Architecture split:**
- **axum backend** — keeps all existing routes (markdown rendering, file serving, WebSocket push, static assets). Adds a `/api/` route group serving lifecycle JSON (design nodes, OpenSpec changes, memory facts graph edges, cleave history).
- **minijinja frontend** — continues serving the document layer (individual markdown pages, sidebar, wikilinks). Fast, no WASM load time, good for navigation.
- **Dioxus WASM frontend** — serves the lifecycle dashboard at `/dashboard`, graph view at `/graph`, kanban at `/board`. Talks to `/api/` via fetch + the existing WebSocket for live updates.

**Desktop mode:**
`mdserve --app` launches a native Dioxus Desktop window wrapping the same WASM app. This coexists with the TUI dashboard — TUI remains ambient status in the terminal, the desktop app is the deep inspection surface for larger projects.

**Graph layout in WASM:**
force-graph.js is already vendored in the fork and working. For Dioxus WASM, we can call into force-graph.js via `web_sys` / `wasm-bindgen` interop, or use `fdg` (Rust-native force-directed layout) for fully Rust-native layout computation. `petgraph` handles graph algorithms (cycle detection, shortest paths, subgraph extraction) on the backend for the intelligence layer.

### Nix distribution — styrened flake pattern

styrened uses `flake-utils.lib.eachDefaultSystem` + a `nix/package.nix` that separates the build derivation from the flake, keeping flake.nix clean. For a Rust binary we'll use the same pattern:

```
flake.nix — inputs: nixpkgs + flake-utils (+ crane for incremental Rust builds)
nix/package.nix — buildRustPackage or crane.buildRustPackage
nix/shell.nix — devShell with cargo, rust-analyzer, cargo-watch, wasm-pack
```

Key details from styrened pattern:
- `version = builtins.replaceStrings ["\n"] [""] (builtins.readFile ./VERSION)` — version from VERSION file
- `commitSha = if self ? shortRev then self.shortRev else "unknown"` — injected at build time
- `pkgs.lib.cleanSource ./.` — avoids polluting the sandbox with target/ and node_modules/
- `meta` block with `platforms = platforms.linux ++ platforms.darwin` — multi-platform

For Rust with WASM (Dioxus web target), the flake will also need:
- `wasm32-unknown-unknown` target in the Rust toolchain
- `wasm-bindgen-cli` and `wasm-opt` in nativeBuildInputs
- Separate derivation for the WASM bundle, embedded in the main binary via `include_bytes!`

This means a single `cargo build --release` produces the complete binary with WASM UI embedded — no separate asset serving needed.

## Decisions

### Decision: Scope as project intelligence portal, not markdown previewer

**Status:** decided
**Rationale:** The narrow mdserve-fork framing undersells the opportunity. Pi-kit has enough structured lifecycle data to build a full project intelligence layer. The rendering engine should be designed from the start to consume all lifecycle artifacts, not retrofitted later.

### Decision: Rust binary, browser frontend, WebSocket push

**Status:** decided
**Rationale:** Rust gives us a single distributable binary with no runtime dependencies. The browser frontend handles the rich graph/board UI with existing JS graph libraries. WebSocket push enables live updates as the agent modifies lifecycle artifacts — the dashboard becomes a live view of agent work in progress.

### Decision: Three-layer architecture (document / lifecycle / intelligence)

**Status:** decided
**Rationale:** Document layer is the foundation and can ship first. Lifecycle layer (graph views, kanban, funnel) ships second. Intelligence layer (traceability, health scoring, memory graph) ships third. Each layer is independently useful.

### Decision: Standalone binary + Omegon extension bridge

**Status:** decided
**Rationale:** The binary lives in its own repo and is independently installable. Pi-kit gets a `/auspex open` extension command that spawns it and opens the browser. Clean separation — the viewer is useful beyond Omegon.

### Decision: Extend the existing mdserve fork — distribution model decided

**Status:** decided
**Rationale:** The fork at ~/workspace/ai/mdserve already has wikilinks, force-graph.js (vendored), recursive scanning, WebSocket live-reload, Styrene theme, and 2514 lines of working Rust. Document layer is ~80% done. No reason to start over — extend this fork for lifecycle and intelligence layers. The fork lives in its own repo and is independently distributable.

### Decision: Long-lived daemon with WebSocket push — already in the fork

**Status:** decided
**Rationale:** Interactivity requires a running server. The fork already implements this: tokio async runtime, notify file watcher, broadcast channel for WebSocket push to all connected clients. Static generation would break the live-update-while-agent-runs use case. Daemon mode is the only viable path for a live project intelligence view. Ephemeral by design: start during a session, kill when done.

### Decision: force-graph.js (already vendored) + fdg for Rust-native WASM layout

**Status:** decided
**Rationale:** force-graph.js is already vendored in the fork and working for the wikilink graph. For the Dioxus WASM lifecycle views, use `fdg` (Rust-native force-directed layout) so layout computation runs in WASM without JS interop. `petgraph` handles graph algorithms (cycle detection, blocking-chain traversal, subgraph extraction) on the axum backend. Two-tier: fdg/petgraph for Rust-native intelligence, force-graph.js for the legacy document graph view.

### Decision: Coexistence: TUI for ambient status, Dioxus Web + Desktop for deep inspection

**Status:** decided
**Rationale:** TUI dashboard stays — it's ambient, zero-friction, always visible in the terminal. The rendering engine is the "pull" surface: open it when you need to see the full graph, triage blocked nodes, trace spec coverage, or inspect cleave history. Dioxus targets both Web (WASM SPA at localhost) and Desktop (native webview window via `mdserve --app`) from the same codebase. Desktop target means the intelligence layer can sit alongside the terminal rather than requiring a browser tab. Both surfaces receive the same WebSocket push from the daemon — they stay in sync.

### Decision: Nix flake distribution following styrened pattern

**Status:** decided
**Rationale:** Nix flake with `flake-utils.lib.eachDefaultSystem` + `nix/package.nix` (buildRustPackage or crane) following styrened's exact structure. Version from a VERSION file, commitSha injected at build time, `cleanSource` to exclude target/. WASM bundle for Dioxus web target built as a separate derivation and embedded in the main binary via `include_bytes!` — single binary output, zero runtime deps, works on macOS + Linux. The Omegon extension (`/auspex open`) invokes the binary by name; it is the user's responsibility to have it on PATH (installed via Nix). Cargo install remains available for non-Nix users.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `src/lifecycle.rs` (new) — Lifecycle data model — parse design node frontmatter, OpenSpec change directories, tasks.md groups, facts.jsonl. Produces typed Rust structs for API serialization.
- `src/api.rs` (new) — axum /api/ route group — GET /api/design-tree, /api/openspec, /api/memory-graph, /api/cleave-history. Serves lifecycle JSON to the Dioxus frontend.
- `src/wasm/` (new) — Dioxus WASM frontend — components for kanban board, OpenSpec funnel, design tree graph (fdg layout), memory fact graph, cleave timeline. Built separately, embedded in binary via include_bytes!.
- `src/app.rs` (modified) — Add /dashboard, /graph, /board routes that serve the Dioxus WASM SPA. Add lifecycle-aware frontmatter rendering for design nodes and OpenSpec docs.
- `flake.nix` (new) — Nix flake — flake-utils.lib.eachDefaultSystem, packages.default = mdserve binary, devShell with cargo/rust-analyzer/wasm-pack/cargo-watch.
- `nix/package.nix` (new) — buildRustPackage derivation — includes WASM build step for Dioxus web target, embeds bundle into binary.

### Constraints

- Single Rust binary, no Node.js runtime required to serve
- <15MB release binary
- Works offline (no CDN dependencies in production build — vendor JS/CSS)
- Live-reload via WebSocket on any artifact change (design docs, OpenSpec, facts.jsonl)
- Serves on configurable localhost port (default: 7842)
- Single binary output — WASM bundle embedded via include_bytes!, zero runtime asset dependencies
- All existing mdserve document-layer functionality preserved — no regressions
- Dioxus WASM SPA served at /dashboard, /graph, /board — minijinja document layer unchanged at all other routes
- fdg crate for Rust-native force-directed layout in WASM; petgraph for backend graph algorithms
- Nix flake must build on macOS (aarch64-darwin, x86_64-darwin) and Linux (x86_64-linux, aarch64-linux)
- facts.db graph edges accessible via SQLite read-only (runtime); fall back to facts.jsonl for git-committed state
