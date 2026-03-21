---
id: rust-phase-2
title: "Phase 2 — Native TUI: Dioxus/ratatui replaces pi-tui bridge subprocess"
status: implemented
parent: rust-agent-loop
tags: [rust, phase-2, tui, dioxus, ratatui]
open_questions: []
priority: 1
---

# Phase 2 — Native TUI: Dioxus/ratatui replaces pi-tui bridge subprocess

## Overview

The TUI bridge subprocess disappears. The Rust binary drives the terminal directly via Dioxus terminal renderer or ratatui/crossterm. Dashboard, splash, spinner, tool card rendering — all native Rust. The Node.js LLM bridge is the only remaining subprocess. ~5.7k LoC of TypeScript rendering code migrates to Rust.

## Research

### Phase 2 is now the process inversion — Rust binary + native TUI replaces pi entirely

Phase 1 investigation revealed that pi's InteractiveMode can't be separated into a rendering subprocess. This elevates Phase 2 from "nice-to-have TUI swap" to "the actual process inversion." The Rust binary needs its own terminal UI to become the user-facing process.

**What Phase 2 delivers:**
- The user runs `omegon` which is the Rust binary (not bin/omegon.mjs)
- The Rust binary drives the terminal directly (ratatui or crossterm)
- The Node.js LLM bridge subprocess is the only remaining TS dependency
- All Omegon extensions are Rust crates (no pi extension API needed)

**Technology options for the TUI:**
1. **ratatui** (https://ratatui.rs) — mature, well-documented, large ecosystem. Immediate-mode rendering. Used by gitui, bottom, lazygit.
2. **Dioxus terminal** (https://dioxuslabs.com) — React-like component model with terminal renderer. Newer but rapidly maturing. Retained-mode.
3. **crossterm raw** — lowest level, maximum control, most code to write.

**The 5.7k LoC of TS rendering that needs Rust equivalents:**
- Conversation view (streaming text, tool call cards, thinking blocks)
- Editor (text input with line editing, history, multiline)
- Dashboard (design-tree state, openspec progress, cleave status, memory stats)
- Footer (model, tokens, branch, extension statuses)
- Splash/spinner (startup animation, thinking indicators)

**The critical path:** The editor (user input) and conversation view (streaming output) are the minimum for a usable interactive session. Dashboard, footer, splash are polish.

**What's already in Rust that the TUI can consume:**
- AgentEvent broadcast channel (loop emits all lifecycle events)
- ContextManager (system prompt assembly)
- ConversationState (message history, intent)
- Session persistence (save/load)
- Lifecycle context (design-tree, openspec)
- Memory tools (facts, recall, episodes)

## Decisions

### Decision: ratatui is the right choice — mature, battle-tested, well-documented, and the immediate-mode model fits streaming agent output

**Status:** decided
**Rationale:** Dioxus terminal is interesting but too new for the critical path. ratatui has: 18k GitHub stars, extensive widget library (paragraphs, lists, tables, tabs, gauges), built-in markdown-like text styling, active maintenance, and is used by gitui/lazygit/bottom. The immediate-mode model (rebuild the frame every tick) is natural for streaming LLM output — we just re-render as new text arrives. The dashboard (design-tree state, openspec progress, cleave status) maps directly to ratatui's layout/widget system. crossterm handles the terminal backend.

### Decision: Minimum viable TUI: editor input + streaming conversation + tool call summaries. No dashboard in first cut.

**Status:** decided
**Rationale:** The first interactive Rust session needs exactly three things: (1) a text editor for user input (single-line with Enter to submit, basic line editing), (2) a scrollable conversation view that streams assistant text as it arrives and shows tool call start/end, (3) Ctrl+C to cancel. Dashboard, footer, splash, and overlay system are all polish that can be layered on after the core loop works interactively. This keeps the MVP at ~500-800 lines of ratatui code.

### Decision: Reuse the existing LLM bridge subprocess — it already handles streaming, auth, and all providers

**Status:** decided
**Rationale:** The LLM bridge (bridge.js, spawned as a Node.js subprocess, communicates via ndjson over stdio) is proven and handles all 15+ providers, OAuth, streaming. The interactive TUI uses the same bridge the headless agent already uses. No new LLM integration needed — the bridge is provider-agnostic from the Rust side.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/mod.rs` (new) — TUI module: App state, event loop, render cycle
- `core/crates/omegon/src/tui/editor.rs` (new) — Input editor: single-line text input with line editing
- `core/crates/omegon/src/tui/conversation.rs` (new) — Conversation view: scrollable message display with streaming
- `core/crates/omegon/src/main.rs` (modified) — Add interactive subcommand that launches TUI instead of headless
- `core/crates/omegon/Cargo.toml` (modified) — Add ratatui + crossterm dependencies

### Constraints

- Terminal must be restored to normal state on panic (crossterm raw mode cleanup)
- Streaming text must render incrementally, not wait for full message
- Ctrl+C during agent execution cancels the turn, Ctrl+C at the editor exits
- The TUI event loop must not block the agent loop — separate tokio tasks communicating via channels
