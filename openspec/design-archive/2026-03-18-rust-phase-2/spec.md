+++
id = "7f512fc4-904d-4a9c-86de-359ddafc79e8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Phase 2 — Native TUI: Dioxus/ratatui replaces pi-tui bridge subprocess — Design Spec (extracted)

> Auto-extracted from docs/rust-phase-2.md at decide-time.

## Decisions

### ratatui is the right choice — mature, battle-tested, well-documented, and the immediate-mode model fits streaming agent output (decided)

Dioxus terminal is interesting but too new for the critical path. ratatui has: 18k GitHub stars, extensive widget library (paragraphs, lists, tables, tabs, gauges), built-in markdown-like text styling, active maintenance, and is used by gitui/lazygit/bottom. The immediate-mode model (rebuild the frame every tick) is natural for streaming LLM output — we just re-render as new text arrives. The dashboard (design-tree state, openspec progress, cleave status) maps directly to ratatui's layout/widget system. crossterm handles the terminal backend.

### Minimum viable TUI: editor input + streaming conversation + tool call summaries. No dashboard in first cut. (decided)

The first interactive Rust session needs exactly three things: (1) a text editor for user input (single-line with Enter to submit, basic line editing), (2) a scrollable conversation view that streams assistant text as it arrives and shows tool call start/end, (3) Ctrl+C to cancel. Dashboard, footer, splash, and overlay system are all polish that can be layered on after the core loop works interactively. This keeps the MVP at ~500-800 lines of ratatui code.

### Reuse the existing LLM bridge subprocess — it already handles streaming, auth, and all providers (decided)

The LLM bridge (bridge.js, spawned as a Node.js subprocess, communicates via ndjson over stdio) is proven and handles all 15+ providers, OAuth, streaming. The interactive TUI uses the same bridge the headless agent already uses. No new LLM integration needed — the bridge is provider-agnostic from the Rust side.

## Research Summary

### Phase 2 is now the process inversion — Rust binary + native TUI replaces pi entirely

Phase 1 investigation revealed that pi's InteractiveMode can't be separated into a rendering subprocess. This elevates Phase 2 from "nice-to-have TUI swap" to "the actual process inversion." The Rust binary needs its own terminal UI to become the user-facing process.

**What Phase 2 delivers:**
- The user runs `omegon` which is the Rust binary (not bin/omegon.mjs)
- The Rust binary drives the terminal directly (ratatui or crossterm)
- The Node.js LLM bridge subprocess is the only remaining TS de…
