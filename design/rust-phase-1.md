+++
id = "467f9767-ec2e-4590-9912-677267f49708"
kind = "design_node"
title = "Phase 1 — Process inversion: Rust binary becomes the process owner, TS becomes subprocess"
status = "resolved"
tags = ["rust", "phase-1", "process-inversion", "tui-bridge", "strategic"]
aliases = ["rust-phase-1"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
priority = "1"
+++

# Phase 1 — Process inversion: Rust binary becomes the process owner, TS becomes subprocess

## Overview

Replace `bin/omegon.mjs` (Node.js) with the Rust binary as the process entry point. The Rust binary:
- Owns the process lifecycle (signal handling, exit codes, session persistence)
- Runs the agent loop for interactive sessions (not just cleave children)
- Spawns the Node.js LLM bridge as a long-lived subprocess (already exists)
- Spawns a Node.js TUI bridge subprocess (imports pi-tui, receives render events, drives terminal)
- Links all lifecycle crates (design-tree, openspec) directly
- Implements compaction (context decay + LLM-driven summarization)
- Implements session persistence (save/load conversation state)
- Implements steering messages (interactive input mid-run)

**Prerequisites from Phase 0 that are done:**
- ✅ Agent loop state machine
- ✅ LLM bridge subprocess
- ✅ 4 core tools + 8 memory tools
- ✅ Context manager (basic)
- ✅ Conversation state
- ✅ System prompt assembly

**What Phase 1 adds:**
1. **Compaction** — context decay + LLM-driven summarization via bridge
2. **Session persistence** — save/load conversation history, resume sessions
3. **TUI bridge** — Node.js subprocess that imports pi-tui, receives AgentEvent stream, drives terminal
4. **Lifecycle crates** — design-tree + openspec as Rust crates in the core
5. **Steering** — interactive input (user messages mid-turn, follow-up messages)
6. **Feature crate integration** — render, view, web-search, local-inference tools
7. **Process entry point** — the npm `bin` field points to the Rust binary wrapper

**The discontinuity:** This is the one point where the user sees a change — the binary they run is different. If Phase 0 is solid (it is), behavior should be identical.

**Version target:** 1.0.0 — this is the breaking change that warrants a major version bump.

## Decisions

### Decision: Phase 1 ships incrementally: compaction first, then lifecycle crates, then TUI bridge last

**Status:** decided
**Rationale:** The TUI bridge is the hardest and riskiest piece — it requires reimplementing the entire interactive experience boundary. Compaction and lifecycle crates can be shipped independently and immediately improve the Rust agent loop for cleave children (longer sessions, lifecycle-aware children). The incremental order: (1) compaction — enables long Rust sessions for any consumer, (2) lifecycle crates — gives Rust children design-tree/openspec awareness, (3) session persistence — enables session resume in Rust, (4) TUI bridge — the process inversion itself. Each step ships value independently. The TUI bridge is only needed for the final flip where the user's entry point changes.

### Decision: TUI bridge is a subprocess (not napi-rs) — matching the LLM bridge pattern

**Status:** decided
**Rationale:** The LLM bridge subprocess pattern is already proven — ndjson over stdio, long-lived process, clean separation. The TUI bridge should follow the same model: Rust sends AgentEvents as ndjson, receives user input back. This avoids linking against libnode, avoids ESM/CJS import fights, avoids pi-tui's internal state leaking into the Rust process. IPC latency for rendering events is negligible — the bottleneck is LLM response time, not render dispatch. The subprocess model also means the TUI bridge can be developed and tested independently.

### Decision: Feature tools (render, view, web-search, local-inference) stay TS-only through Phase 1 — accessed via tool bridge

**Status:** decided
**Rationale:** These are thin wrappers around external processes (d2, ffmpeg, satori, brave API). Porting them to Rust is low-value busywork — the Rust version would do the same subprocess calls. They can be exposed to the Rust agent loop through a tool bridge (the TUI bridge subprocess also handles tool calls for TS-only tools) or migrated lazily in Phase 2+. The priority is compaction and lifecycle — not reimplementing HTTP calls to search APIs.

### Decision: Phase 1 completes without the TUI bridge — the true process inversion is Phase 2 with native TUI

**Status:** decided
**Rationale:** Investigation of pi's InteractiveMode (3.8k lines) reveals it cannot be cleanly separated from AgentSession to serve as a rendering subprocess. The TUI bridge concept assumed pi-tui was a separable layer; it isn't. Phase 1 is complete with: (1) compaction ✅, (2) lifecycle crates ✅, (3) session persistence ✅. The Rust binary is the default headless executor. Interactive mode continues using pi's InteractiveMode (which works). The true process inversion — where the user runs a Rust binary — requires a native TUI, which is Phase 2. This is not a retreat — it's recognition that the throwaway bridge would cost more to build than the native TUI it would be replaced by.

## Open Questions

*No open questions.*
