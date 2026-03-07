---
id: markdown-viewport
title: Markdown Viewport — Lightweight FOSS viewer for agent-generated docs
status: decided
tags: [viewport, markdown, documentation, foss]
open_questions: []
---

# Markdown Viewport — Lightweight FOSS viewer for agent-generated docs

## Overview

A lightweight, FOSS tool to render interlinked agent-generated markdown (design tree, OpenSpec, memory) as a navigable web UI with graph view. Reuses the remark-wikilinks plugin and local graph component from styrene's Astro site. Goal: zero-friction human viewport into agent state — not a data store, not bidirectional.

## Decisions

### Decision: Fork mdserve, add wikilinks + graph view

**Status:** decided
**Rationale:** mdserve is a Rust single-binary markdown preview server built for AI coding agent workflows. It already has directory serving, sidebar nav, WebSocket live-reload, and Catppuccin themes. Missing wikilink resolution and graph view — both addable with ~500 lines. Leaner than Astro (no node_modules), more capable than mdBook (no SUMMARY.md friction), purpose-built for our use case.

### Decision: Standalone tool, pi-kit extension invokes it

**Status:** decided
**Rationale:** The fork lives in its own repo (standalone Rust binary). pi-kit gets a lightweight skill teaching the agent wikilink conventions, and optionally an extension with /vault serve that spawns the binary. Clean separation — the viewer is useful beyond pi-kit.

### Decision: Configurable roots, sensible defaults

**Status:** decided
**Rationale:** mdserve already takes a directory arg. Wikilink resolution scans all .md files in the served tree. Default invocation from pi-kit would point at project root (capturing .pi/, openspec/, docs/). User can narrow to any subdir.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `src/wikilinks.rs` (new) — Wikilink resolution — slug map builder, [[target|display]] parser, edge extraction
- `src/graph.rs` (new) — Graph data structures (nodes/edges) and builder from wikilink edges
- `src/app.rs` (modified) — Recursive scanning, path-relative keys, wikilink integration, /graph routes
- `templates/main.html` (modified) — Styrene theme, wikilink CSS, graph view sidebar link

### Constraints

- All 42 tests passing
- 5.8MB release binary
- force-graph.js loaded from CDN (unpkg)
