+++
id = "b612a1ef-c435-436e-9297-8d99821966d8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Rendering Engine — Lifecycle Visualization & Project Intelligence Layer

## Intent

Extend the existing mdserve fork into a full project intelligence portal for Omegon workflows. Three-layer architecture: document (wikilinks, live-reload), lifecycle (design tree graph, OpenSpec funnel, kanban), and intelligence (traceability, memory graph, health scoring). Dioxus WASM frontend for lifecycle/intelligence views; axum backend serves both document layer (minijinja) and a JSON API. Single Rust binary with embedded WASM bundle. Nix flake distribution following styrened pattern.

## Decisions Made

All architecture decisions are decided — see design node docs/design/markdown-viewport.md.

1. Fork mdserve (~/workspace/ai/mdserve) — already has wikilinks, force-graph.js, recursive scanning, Styrene theme, 2514 lines
2. Long-lived daemon with WebSocket push (already implemented in fork)
3. force-graph.js (vendored) + fdg (Rust WASM) + petgraph (backend graph algorithms)
4. Coexistence: TUI ambient status, Dioxus Web + Desktop for deep inspection
5. Nix flake distribution following styrened pattern
