+++
id = "eb683fa9-3f0e-40f0-bf07-862dc9bd22d6"
kind = "document"
title = "TUI bridge — Node.js subprocess receives AgentEvents and drives pi-tui terminal rendering"
status = "deferred"
tags = ["rust", "tui", "bridge", "subprocess", "rendering"]
aliases = ["rust-tui-bridge"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-phase-1"
priority = "1"
+++

# TUI bridge — Node.js subprocess receives AgentEvents and drives pi-tui terminal rendering

## Overview

A Node.js subprocess that imports pi-tui and pi-coding-agent's interactive mode, receiving AgentEvent stream from the Rust binary over a pipe (ndjson or msgpack). The TUI bridge translates Rust AgentEvents into pi-tui component updates — conversation rendering, tool call cards, dashboard, footer, editor input.

This is the transitional layer that lets Phase 1 ship with full TUI parity without rewriting the rendering layer. It's replaced by native Rust TUI in Phase 2.

**The bridge subprocess receives:**
- AgentEvent variants (turn_start, message_chunk, tool_start/end, etc.)
- Dashboard state updates (design-tree, openspec, cleave, memory)
- Prompt requests (when the agent loop needs user input)

**The bridge subprocess sends back:**
- User input (typed messages, steering)
- Slash command invocations
- Signal forwarding (Ctrl+C → cancel token)

**Key constraint:** The bridge must feel identical to current Omegon — same rendering, same keyboard shortcuts, same dashboard. Users should not notice the process inversion.

## Research

### Two architectures for the inversion — and why only one works now

**Option A: Rust owns the loop, pi-tui is a pure renderer (true inversion)**
The Rust binary runs the agent loop (as it does today for cleave children), streams AgentEvents to a Node.js subprocess that only renders. The subprocess imports pi-tui and drives the terminal. Rust sends: message chunks, tool starts/ends, turn boundaries. Node sends back: user input, Ctrl+C, slash commands.

Problem: pi-tui is NOT a standalone rendering library. It's deeply coupled to pi's `InteractiveMode` class (3.8k lines) which manages the editor, conversation view, tool call rendering, keybindings, overlay system, widget containers, footer. Extracting "just the renderer" from `InteractiveMode` would be a massive fork effort — rewriting 3.8k lines of tightly coupled JS.

**Option B: Pi's InteractiveMode is the subprocess, Rust dispatches through it (pragmatic inversion)**
The Rust binary spawns `omegon --rpc` (or similar) as a subprocess. Pi's `InteractiveMode` owns the TUI and the agent session, but the *real work* (tool execution) is delegated to the Rust binary. The Rust binary registers its tools via the existing TS tool bridge, and pi's agent loop calls them. Lifecycle context, compaction, and session persistence are handled by the Rust side.

Problem: This doesn't actually invert the process. Pi still owns the agent loop and the session. Rust is still a sidecar.

**Option C: Rust IS the process, spawns pi in headless RPC mode for the LLM + rendering (hybrid)**
The Rust binary is the user-facing process (owns stdin/stdout/terminal). It spawns pi in RPC mode as a subprocess. But instead of using pi's agent loop, it:
1. Uses the RPC `prompt` command to send messages through pi's LLM pipeline (replacing the current raw LLM bridge)
2. Captures pi's streaming events and renders them through... what? We'd need our own TUI.

This circles back to Option A's problem — we need a renderer.

**The real question: can we get interactive Omegon without rewriting pi-tui?**

**Option D: Keep pi as the process owner for interactive mode, Rust for headless**
The current architecture. Interactive → `bin/omegon.mjs` → pi's InteractiveMode + Omegon extensions. Headless → Rust `omegon-agent` for cleave children. This is what 0.11.0 shipped.

The inversion happens in Phase 2 when we build a native TUI. Until then, pi's InteractiveMode is the only viable interactive renderer — and it requires being the host, not a subprocess.

**Option E (recommended): Partial inversion — Rust owns tool execution, pi owns the UX shell**
Keep pi as the interactive process entry point BUT route all tool execution through the Rust binary. Today, pi's agent loop calls tools via its TypeScript ToolProvider implementations. We replace those with a bridge that forwards tool calls to the running Rust binary (which has native tools, lifecycle, memory, compaction). Pi becomes a thin shell: LLM calls + TUI rendering. The "brain" is Rust.

This is achievable without forking pi-tui, ships incrementally, and sets up Phase 2 cleanly.

### Reframing 1.0.0 — what does the version boundary actually mean?

**The original plan said 1.0.0 = process inversion (Rust binary is the entry point).** But examining pi's InteractiveMode reveals this was based on a false assumption: that pi-tui could be used as a standalone rendering library. It can't — it's coupled to InteractiveMode which is coupled to AgentSession.

**What 1.0.0 should mean instead:**
The Rust binary is the *default executor* for all agent work. Interactive mode still uses pi's InteractiveMode (because it's the only viable TUI), but:
- Cleave children always use the Rust binary (already done ✅)
- The Rust binary can run standalone sessions with `omegon-agent --prompt` (already done ✅)
- Session persistence, compaction, lifecycle awareness are all Rust-native (done this session ✅)

**The actual process inversion happens in Phase 2** when we build a native TUI (ratatui/Dioxus) that replaces pi's InteractiveMode entirely. At that point the Rust binary truly becomes the only process.

**What remains for 1.0.0:**
1. Core binary distribution through GitHub Releases and Homebrew
2. Version alignment between omegon npm and omegon-core Cargo
3. Integration: make the TS interactive mode use Rust memory/lifecycle when available

The TUI bridge as originally conceived (Node subprocess rendering for Rust host) is not the right next step. Building a native TUI is (Phase 2).

## Decisions

### Decision: Defer TUI bridge — pi's InteractiveMode cannot be cleanly separated from its host. Skip to native TUI in Phase 2.

**Status:** decided
**Rationale:** pi's InteractiveMode (3.8k lines) is tightly coupled to AgentSession, the extension API, the overlay system, and the widget containers. Extracting it as a subprocess renderer would require forking and rewriting most of those 3.8k lines — effort better spent building a native TUI from scratch in Phase 2. The "TUI bridge" concept assumed pi-tui was a separable rendering layer; investigation shows it isn't. The pragmatic path: keep pi as the interactive host (it works), Rust as the headless executor (proven), and invest TUI effort in Phase 2's native renderer instead of a throwaway bridge.

## Open Questions

*No open questions.*
