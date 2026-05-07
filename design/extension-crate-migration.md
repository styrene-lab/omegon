+++
id = "b0a9d56e-0297-4954-bb6b-82f6c426a6f3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension-to-crate migration — how each Omegon extension maps to the Rust architecture

## Overview

With the Rust agent loop owning the process, pi's extension API (registerTool, registerCommand, pi.on events, ctx.ui) ceases to exist. Every Omegon extension currently written as a TypeScript pi-package guest needs to become something else. This node maps each extension to its post-inversion form and defines the Rust trait surface that replaces pi's extension API.

**The current extension inventory (44k+ LoC TypeScript across 19 extensions + 12 standalone modules):**

The extensions break into four categories based on what they actually do:

**Category A — Business logic + tools (the real features):**
These register tools the agent calls, maintain state, and inject context. They're the reason Omegon exists.
- project-memory (8,590 LoC): 12 tools, 8 event hooks, heavy context injection
- cleave (9,485 LoC): 3 tools, subprocess orchestration, worktree management
- design-tree (4,630 LoC): 2 tools, 4 events, markdown file I/O + state machine
- openspec (4,132 LoC): 1 tool, spec parsing, lifecycle gating
- render (2,909 LoC): 6 tools, calls external binaries (d2, flux, satori, excalidraw)
- web-search (306 LoC): 1 tool, HTTP calls to search providers
- view (1,007 LoC): 1 tool, file rendering (images, PDFs, code)
- local-inference (727 LoC): 3 tools, Ollama management
- mcp-bridge (1,321 LoC): dynamic tool registration from MCP servers

**Category B — Infrastructure + orchestration (the plumbing):**
These don't register agent tools but manage the runtime environment.
- bootstrap (1,574 LoC): dependency checking, first-run setup
- effort/tier routing (783 LoC): model selection logic
- tool-profile (586 LoC): tool enable/disable management
- 01-auth (765 LoC): authentication flow management
- 00-secrets (1,239 LoC): credential management

**Category C — Pure rendering (the skin):**
These produce visual output but contain zero business logic.
- dashboard (3,530 LoC): raised dashboard, lifecycle state display
- 00-splash (850 LoC): startup animation
- spinner-verbs + sermon (456 LoC): thinking indicators
- core-renderers (193 LoC): tool call rendering
- terminal-title (191 LoC): terminal title management
- style (281 LoC): Alpharius color definitions

**Category D — Side-channel services:**
- web-ui (1,121 LoC): HTTP server for web dashboard
- vault (185 LoC): Obsidian conventions
- session-log (174 LoC): session logging
- version-check (94 LoC): update notifications
- defaults (274 LoC): AGENTS.md deployment
- auto-compact (42 LoC): compaction trigger
- model-budget (752 LoC): cost tracking
- offline-driver (410 LoC): offline mode switching

## Research

### The trait surface — what replaces pi's extension API

Pi's extension API is a bag of imperative calls: `pi.registerTool()`, `pi.registerCommand()`, `pi.on("event_name")`, `ctx.ui.setFooter()`. It's a guest API — extensions ask the host for permission to exist.

In the Rust world, there is no host/guest separation. "Extensions" are library crates linked into the binary. They participate through trait implementations that the agent loop composes at startup.

### The four traits

```rust
/// Provides tools to the agent loop.
/// Each tool has a name, JSON schema, and an execute function.
trait ToolProvider: Send + Sync {
    fn tools(&self) -> Vec<ToolDefinition>;
    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> Result<ToolResult>;
}

/// Provides dynamic context for the ContextManager.
/// Called once per turn to inject relevant content into the system prompt.
trait ContextProvider: Send + Sync {
    /// Return context to inject, given the current signals.
    /// Return None to inject nothing this turn.
    fn provide_context(&self, signals: &ContextSignals) -> Option<ContextInjection>;
}

/// Reacts to agent events for side effects (logging, dashboard, etc.)
/// Must not block — events are broadcast, not request/response.
trait EventSubscriber: Send + Sync {
    fn on_event(&self, event: &AgentEvent);
}

/// Session lifecycle hooks.
trait SessionHook: Send + Sync {
    async fn on_session_start(&mut self, config: &SessionConfig) -> Result<()> { Ok(()) }
    async fn on_session_end(&mut self, stats: &SessionStats) -> Result<()> { Ok(()) }
    async fn on_before_turn(&mut self, turn: u32, intent: &IntentDocument) -> Result<()> { Ok(()) }
    async fn on_after_turn(&mut self, turn: u32, intent: &IntentDocument) -> Result<()> { Ok(()) }
    async fn on_compaction(&mut self, intent: &IntentDocument) -> Result<()> { Ok(()) }
}
```

### How they compose

A crate can implement any combination of traits. Project-memory, for example, implements all four:
- `ToolProvider` — registers memory_store, memory_recall, memory_query, etc.
- `ContextProvider` — injects relevant facts into the system prompt per-turn
- `EventSubscriber` — tracks tool calls for extraction candidates
- `SessionHook` — loads facts on session start, generates episodes on session end

The agent loop collects all trait implementations at startup:

```rust
struct AgentRuntime {
    tools: Vec<Box<dyn ToolProvider>>,
    context_providers: Vec<Box<dyn ContextProvider>>,
    event_subscribers: Vec<Box<dyn EventSubscriber>>,
    session_hooks: Vec<Box<dyn SessionHook>>,
}
```

Tool dispatch: when the LLM calls a tool, the runtime finds the `ToolProvider` that owns that tool name and calls its `execute`. No dynamic dispatch through a plugin loader — the mapping is built once at startup from the registered providers.

### What this is NOT

This is not a plugin system. Crates are compiled in. There is no dynamic loading, no runtime discovery, no extension manifest. The binary knows exactly which crates are linked and what tools they provide. This is intentional:

- Compile-time type safety (schema mismatches are build errors)
- No serialization boundary (crates share `ToolResult`, `AgentMessage`, etc. directly)
- No IPC overhead (function calls, not JSON-RPC)
- Dead code elimination (unused crates are stripped by the linker if behind cargo features)

If external extensibility is needed later (third-party tools), that's what the MCP bridge is for — it provides a well-defined protocol boundary for external tools while native crates remain zero-overhead.

### The ContextSignals struct

```rust
struct ContextSignals {
    user_prompt: &str,            // current user message text
    recent_tools: &[String],      // last N tool names called
    recent_files: &[PathBuf],     // last N files touched
    intent: &IntentDocument,      // current session intent
    context_budget: usize,        // remaining tokens available for injection
    turn_number: u32,
}
```

Each `ContextProvider` receives these signals and decides whether to inject anything. The ContextManager calls all providers, collects their injections, sorts by priority, and fits them within the budget. This replaces the current model where each extension independently appends to `promptGuidelines` with no coordination.

### Category A — business logic crates: the per-extension migration map



### project-memory (8,590 LoC) → `omegon-memory` crate

**Today:** The largest extension. 12 tools, 8 event hooks, heavy context injection pipeline. The TS side does: tool registration, context injection (the priority-ordered pipeline with tiers 1-6), startup payload, compaction hooks, memory rendering, lifecycle candidate ingestion. There's already a partial Rust port planned (omega-memory-backend design node).

**After:** This becomes one of the richest crate implementations:
- `ToolProvider`: memory_store, memory_recall, memory_query, memory_episodes, memory_focus, memory_release, memory_archive, memory_supersede, memory_compact, memory_connect, memory_search_archive, memory_ingest_lifecycle
- `ContextProvider`: The priority-ordered injection pipeline. Gets the user prompt from signals, runs hybrid search (FTS5 + embeddings), returns facts within budget. This is where the biggest token savings happen — the ContextManager coordinates memory injection with other providers instead of memory blindly dumping 8k tokens.
- `SessionHook`: on_session_start loads the factstore, initializes embeddings. on_session_end generates episodes. on_compaction ensures intent document + working memory survive.
- `EventSubscriber`: Tracks tool calls for extraction candidates (automatic fact discovery from what the agent does).

**What changes:** The context injection pipeline currently operates in isolation — it grabs as much budget as it can without knowing what other extensions need. In the ContextManager model, memory competes with design-tree context, skill injections, and other providers for a shared budget. This is strictly better — the ContextManager arbitrates, not each provider.

**Migration complexity:** HIGH. The factstore + embeddings + retrieval engine is substantial. This benefits most from the omega-memory-backend work (Rust-native sqlite + vec search). The TS injection pipeline (~600 lines of priority tiering) translates directly to Rust.

---

### cleave (9,485 LoC) → `omegon-cleave` crate

**Today:** Dispatcher (child subprocess spawning, worktree management, wave planning), assessment bridge, review loop orchestration, process tree coordination. The TS side does: tool registration, subprocess management, task file generation, result harvesting, merge orchestration.

**After:**
- `ToolProvider`: cleave_assess, cleave_run, execute_slash_command (for /cleave, /assess)
- `SessionHook`: on_session_start registers the slash command bridge
- `EventSubscriber`: tracks cleave child progress for dashboard reporting

**What changes fundamentally:** Cleave currently spawns full Omegon instances as children. With the Rust agent loop, cleave children are the Phase 0 headless binary — `omegon-agent`. This is the single biggest architectural win: instead of spawning a 44k-LoC TypeScript runtime for each child, you spawn a single Rust binary with 4 tools. The cleave crate's dispatcher becomes dramatically simpler because it doesn't need to manage the complexity of full Omegon subprocess initialization.

The worktree management (create branch, checkout, merge back) moves to `gix` calls instead of shelling out to `git`. The wave planner (dependency ordering via topological sort) moves to `petgraph`. Both are natural Rust fits.

**Migration complexity:** HIGH, but the payoff is enormous. The dispatcher is the most complex single piece of Omegon code.

---

### design-tree (4,630 LoC) → `omegon-design-tree` crate

**Today:** 2 tools (design_tree query, design_tree_update), 4 events, markdown file I/O with YAML frontmatter, state machine for node lifecycle, focus management, lifecycle binding to openspec.

**After:**
- `ToolProvider`: design_tree (query), design_tree_update (mutations)
- `ContextProvider`: When a node is focused, injects its overview + decisions + open questions. Responds to signals — if user mentions "design" or a known node ID, injects relevant context even without explicit focus.
- `SessionHook`: on_session_start scans the design/ directory for nodes

**What changes:** The markdown parsing + YAML frontmatter handling becomes a Rust crate using `serde_yaml` + a markdown parser. The state machine (seed → exploring → resolved → decided → implemented) becomes a Rust enum with exhaustive match — illegal transitions are compile errors instead of runtime checks.

**Migration complexity:** MEDIUM. File I/O + parsing + state machine. Straightforward Rust.

---

### openspec (4,132 LoC) → `omegon-openspec` crate

**Today:** 1 tool (openspec_manage with sub-actions), spec parsing (Given/When/Then), stage computation, archive gating, reconciliation, change lifecycle.

**After:**
- `ToolProvider`: openspec_manage
- `ContextProvider`: When bound to a design-tree node, injects spec scenarios and tasks for the current change
- `SessionHook`: on_session_start scans openspec/ for active changes

**What changes:** The spec parser (Given/When/Then scenarios, falsifiability criteria) becomes a Rust parser. Stage computation (proposed → specced → planned → implementing → verifying → archived) becomes a Rust enum. The archive gating logic (refuse stale state) is pattern matching.

**Migration complexity:** MEDIUM. Closely coupled with design-tree — they share lifecycle state. The two crates need to reference each other or share a common `omegon-lifecycle` types crate.

---

### render (2,909 LoC) → `omegon-render` crate (partially)

**Today:** 6 tools: generate_image_local (FLUX via MLX), render_diagram (D2), render_native_diagram (SVG), render_excalidraw, render_composition_still (Satori), render_composition_video (Satori + gifenc).

**After:** This is the hardest migration because the render tools depend on external ecosystems:
- D2 → Go binary, called via subprocess. Same in Rust — just `tokio::process::Command`.
- FLUX/MLX → Python/Swift, called via subprocess. Same approach.
- Satori → Node.js library. Either: subprocess bridge (like the LLM bridge) or eliminate in favor of a Rust SVG renderer.
- Excalidraw → Playwright + Chromium. Subprocess.

`ToolProvider`: All 6 tools. Most are thin wrappers around subprocess calls.

**What changes:** Not much architecturally. These tools are already "shell out to external binary" — the TS wrapper is just argument assembly + result parsing. The Rust version does the same with `tokio::process::Command`.

The composition rendering (Satori + React → SVG → PNG) is the one that genuinely depends on Node.js. Options: keep a Node subprocess for it, or find a Rust-native SVG rendering path.

**Migration complexity:** LOW-MEDIUM. Subprocess wrappers translate mechanically. The Satori dependency is the only wrinkle.

---

### web-search (306 LoC) → `omegon-web-search` crate

**Today:** 1 tool, HTTP calls to Brave/Tavily/Serper search APIs.

**After:**
- `ToolProvider`: web_search

**What changes:** Nothing meaningful. HTTP calls via `reqwest` instead of `fetch`. The API key resolution reads from the same config files.

**Migration complexity:** LOW. 306 LoC of TS becomes ~200 lines of Rust.

---

### view (1,007 LoC) → `omegon-view` crate

**Today:** 1 tool, renders files inline (images via terminal protocols, PDFs via conversion, code with syntax highlighting).

**After:**
- `ToolProvider`: view

**What changes:** Image rendering uses terminal protocols (iTerm2 inline images, Kitty graphics). This is byte-level protocol output — natural Rust. PDF rendering calls external tools (same approach). Syntax highlighting could use `syntect` (Rust-native, sublime-syntax based) instead of shelling out.

**Migration complexity:** LOW-MEDIUM.

---

### local-inference (727 LoC) → `omegon-local-inference` crate

**Today:** 3 tools (ask_local_model, list_local_models, manage_ollama), manages Ollama lifecycle.

**After:**
- `ToolProvider`: ask_local_model, list_local_models, manage_ollama
- `SessionHook`: on_session_start checks Ollama availability

**What changes:** Ollama has a REST API. `reqwest` calls replace `fetch`. Model management (pull, start, stop) via the same REST API.

**Migration complexity:** LOW.

---

### mcp-bridge (1,321 LoC) → `omegon-mcp` crate

**Today:** Dynamic tool registration from MCP servers. Connects to external tool servers via the MCP protocol.

**After:**
- `ToolProvider`: Dynamically registers tools discovered from MCP servers
- `SessionHook`: on_session_start connects to configured MCP servers

**What changes:** The MCP protocol has a Rust SDK (`mcp-sdk-rs`). The bridge becomes a Rust MCP client that discovers tools at startup and registers them as `ToolDefinition` entries. When the agent calls an MCP tool, the crate forwards the call to the MCP server.

This is the external extensibility boundary — third-party tools connect via MCP, not via compiled crates.

**Migration complexity:** MEDIUM. The MCP Rust SDK handles the protocol; the crate handles discovery + registration.

### Categories B, C, D — what gets absorbed, rendered, or eliminated



### Category B — Infrastructure: absorbed into the agent loop core

These extensions exist because pi's guest model requires explicit wiring for things the host should own natively. In the Rust architecture, they're not crates — they're built into the agent loop itself.

**effort/tier routing (783 LoC) → absorbed into `ContextManager` + `LlmBridge`**

Today: Separate extension that manages model tier selection (retribution/victory/gloriana), thinking levels, and local vs cloud routing. Registers commands (/tier, /think), tracks effort state.

After: The ContextManager owns model selection as part of its per-turn assembly. The LlmBridge knows which provider/model to use. Thinking level is a parameter on the stream call. The tier names become variants of a Rust enum. No separate crate needed — this is core loop configuration.

**tool-profile (586 LoC) → cargo features + runtime config**

Today: Tool enable/disable management, profiles (base, development, etc.).

After: Compile-time: cargo features control which crates are linked (`features = ["render", "mcp"]`). Runtime: a config struct that marks which tools are active for this session. The agent loop filters `ToolProvider::tools()` results against the active set. This is ~30 lines of filter logic in the core, not an extension.

**01-auth (765 LoC) → absorbed into `LlmBridge` + session startup**

Today: Authentication flow management, OAuth handling, API key resolution.

After: The LLM bridge subprocess (Node.js) handles OAuth natively because it imports pi-ai. For Rust-native providers (Phase 3), auth token resolution reads from `~/.pi/agent/settings.json` — a `serde_json::from_reader` call. The auth *UI* (login dialog) becomes part of the TUI layer in Phase 1, or a browser-redirect flow.

**00-secrets (1,239 LoC) → `omegon-secrets` crate or absorbed**

Today: Credential management, secret storage, environment variable handling.

After: Depends on complexity. If it's just "read credentials from files/env" it's absorbed into the core config layer. If there's meaningful credential rotation or vault integration, it's a small crate.

**bootstrap (1,574 LoC) → changes fundamentally**

Today: Checks for Node.js deps, runs npm install, verifies d2/nix/etc.

After: Most bootstrap concerns disappear when the binary is Rust — no npm dependencies to install. What remains: checking for external tools the render crate needs (d2, ffmpeg), Ollama availability, and first-run config. This becomes a startup check function in the core, not a separate crate. ~100 lines.

**auto-compact (42 LoC) → absorbed into context decay**

Today: Triggers compaction when context is too full.

After: Context decay is a core loop feature. The agent loop monitors context usage every turn and decays/compacts as needed. The 42-line trigger becomes a method on `ConversationState`.

**model-budget (752 LoC) → absorbed into `SessionStats` + `IntentDocument`**

Today: Tracks token costs, estimates session budget.

After: `SessionStats` in the IntentDocument tracks tokens consumed. Cost estimation is a function of (model, tokens) — a lookup table in the core. Dashboard renders cost, but the tracking is ambient.

**offline-driver (410 LoC) → absorbed into `LlmBridge` fallback chain**

Today: Switches from cloud to local model when API fails.

After: The LlmBridge trait has a fallback chain. If the primary bridge (cloud) fails, it falls back to local (Ollama). This is a core loop concern, not a separate module.

---

### Category C — Pure rendering: becomes the rendering layer

These modules contain zero business logic. They translate state into terminal output. In the Rust architecture, they're all part of the rendering layer — initially the pi-tui bridge subprocess, eventually Dioxus/ratatui components.

**dashboard (3,530 LoC) → rendering layer subscriber**

The dashboard reads lifecycle state (design-tree nodes, openspec changes, cleave progress, memory stats) and renders it. In the Rust world, it subscribes to the `AgentEvent` broadcast channel and queries the crates for their state. It never registered tools — it was always pure rendering.

The dashboard needs a query interface to each feature crate:
```rust
trait DashboardState: Send + Sync {
    fn dashboard_section(&self) -> Option<DashboardEntry>;
}
```

Feature crates implement this to expose their state for the dashboard.

**splash, spinner-verbs, sermon, terminal-title, style, core-renderers → rendering layer**

All of these are terminal output concerns. They subscribe to events and render. In Phase 0 (headless), they don't exist. In Phase 1 (TUI bridge), they live in the Node.js TUI subprocess. In Phase 2 (native TUI), they become ratatui/crossterm components.

These are the *last* things to migrate because they're the most tightly coupled to the current TUI implementation. And that's fine — they're also the least architecturally significant.

---

### Category D — Side-channel services

**web-ui (1,121 LoC) → Axum endpoint in the main binary**

Today: Separate extension that starts an HTTP server, serves static files, exposes lifecycle state via REST.

After: The Rust binary includes an Axum HTTP server (already planned for Omega). Lifecycle state is served directly from the feature crates — no extension needed to bridge between pi's event model and an HTTP response.

**vault (185 LoC) → part of the system prompt / skills system**

Today: Registers a slash command that generates Obsidian-compatible markdown.

After: This is conventions, not code. It becomes a skill file that the ContextManager injects when the agent is generating documentation. ~0 lines of Rust.

**session-log (174 LoC) → absorbed into session persistence**

Today: Logs session events to a file.

After: Session persistence is a core loop concern. The canonical conversation history is saved as part of session state. A dedicated crate isn't needed.

**version-check (94 LoC) → startup check in main()**

Today: Checks npm registry for updates.

After: Checks GitHub releases or a version manifest. ~30 lines in the startup path.

**defaults (274 LoC) → config initialization in main()**

Today: Deploys AGENTS.md, kitty theme, etc. on first run.

After: Part of first-run initialization. Not a separate module.

### The net effect — from 44k LoC of TypeScript to what?



### What becomes Rust crates (Category A business logic)

| Crate | TS LoC | Est. Rust LoC | Traits | Migration |
|-------|--------|---------------|--------|-----------|
| omegon-memory | 8,590 | ~4,000 | All four | HIGH |
| omegon-cleave | 9,485 | ~3,500 | Tool, Session, Event | HIGH |
| omegon-design-tree | 4,630 | ~2,000 | Tool, Context, Session | MEDIUM |
| omegon-openspec | 4,132 | ~1,800 | Tool, Context, Session | MEDIUM |
| omegon-render | 2,909 | ~1,200 | Tool | LOW-MED |
| omegon-view | 1,007 | ~500 | Tool | LOW-MED |
| omegon-web-search | 306 | ~200 | Tool | LOW |
| omegon-local-inference | 727 | ~400 | Tool, Session | LOW |
| omegon-mcp | 1,321 | ~800 | Tool, Session | MEDIUM |
| **Total** | **33,107** | **~14,400** | | |

### What gets absorbed into the core loop (Categories B, D)

| Module | TS LoC | Est. Rust LoC | Where it goes |
|--------|--------|---------------|---------------|
| effort/tiers | 783 | ~150 | ContextManager + LlmBridge |
| tool-profile | 586 | ~50 | Runtime config filter |
| auth | 765 | ~0* | LLM bridge subprocess |
| secrets | 1,239 | ~200 | Config layer |
| bootstrap | 1,574 | ~100 | Startup checks |
| auto-compact | 42 | ~20 | Context decay |
| model-budget | 752 | ~100 | SessionStats |
| offline-driver | 410 | ~80 | LlmBridge fallback |
| session-log | 174 | ~50 | Session persistence |
| version-check | 94 | ~30 | main() startup |
| defaults | 274 | ~40 | main() first-run |
| web-ui | 1,121 | ~300 | Axum HTTP server |
| vault | 185 | ~0 | Skill file, not code |
| **Total** | **7,999** | **~1,120** | |

*Auth is handled by the Node.js LLM bridge subprocess — it imports pi-ai which owns OAuth.

### What becomes the rendering layer (Category C)

| Module | TS LoC | Notes |
|--------|--------|-------|
| dashboard | 3,530 | Phase 1: TUI bridge. Phase 2: ratatui. |
| splash | 850 | Phase 2 concern |
| spinner-verbs + sermon | 610 | Phase 2 concern |
| core-renderers | 193 | Phase 2 concern |
| terminal-title | 191 | Phase 2 concern |
| style | 281 | Shared constants, migrates early |
| **Total** | **5,655** | Migrated last, in Phase 2 |

### The bottom line

| Layer | TS LoC | Rust LoC | Reduction |
|-------|--------|----------|-----------|
| Feature crates | 33,107 | ~14,400 | 57% fewer lines |
| Core absorption | 7,999 | ~1,120 | 86% fewer lines |
| Rendering (Phase 2) | 5,655 | TBD | Migrated last |
| **Total** | **46,761** | **~15,520 + rendering** | |

The 57% reduction in feature crate code isn't because Rust is terser — it's because the TypeScript adapter layer (pi API registration, event wiring, serialization, TUI glue) disappears entirely. That layer was ~40-60% of every extension's code. The actual business logic translates at roughly 1:1 or even expands slightly (Rust is more explicit), but the overhead evaporates.

### The agent loop core itself

| Component | Est. Rust LoC |
|-----------|---------------|
| Agent loop state machine | ~300 |
| LlmBridge + subprocess manager | ~250 |
| ContextManager | ~400 |
| ConversationState + decay | ~500 |
| IntentDocument | ~200 |
| Core tools (understand, change, execute, bash, read, write, edit) | ~1,200 |
| System prompt assembly | ~200 |
| Session persistence | ~300 |
| CLI + startup | ~200 |
| LLM bridge JS | ~100 (JS) |
| **Total core** | **~3,650** |

**Grand total: ~19,170 lines of Rust + ~100 lines of JS** replaces **~46,761 lines of TypeScript**. Plus the rendering layer (Phase 2) which is TBD but likely ~2,000-3,000 lines of ratatui.

This isn't a rewrite cost — it's a *complexity reduction*. The TypeScript version is 46k lines because the guest model, the serialization boundaries, the event wiring, and the IPC overhead all take code. Remove those layers and the actual logic is about 40% of the original size.

## Decisions

### Decision: Four traits replace pi's extension API: ToolProvider, ContextProvider, EventSubscriber, SessionHook

**Status:** decided
**Rationale:** Pi's extension API is an imperative guest model — `registerTool`, `pi.on("event")`, `ctx.ui.setFooter()`. The Rust architecture has no host/guest separation. Feature crates implement four composable traits that the agent loop collects at startup: ToolProvider (register and execute tools), ContextProvider (inject dynamic system prompt content per-turn), EventSubscriber (react to agent events for side effects), and SessionHook (lifecycle callbacks). A crate implements whichever combination it needs. The agent loop composes them without dynamic dispatch through a plugin loader. Compile-time type safety replaces runtime string matching.

### Decision: MCP is the external extensibility boundary — third-party tools connect via protocol, not compiled crates

**Status:** decided
**Rationale:** The trait-based crate model is not a plugin system — it requires compilation into the binary. External extensibility for third-party tools comes through MCP (Model Context Protocol), which provides a well-defined protocol boundary. The omegon-mcp crate is a ToolProvider that discovers tools from MCP servers at session start and proxies calls to them at runtime. This gives external tools a stable interface without requiring them to be Rust crates or linked into the binary. Native crates are zero-overhead; MCP tools pay the protocol cost but get universal compatibility.

### Decision: Infrastructure extensions (effort, auth, bootstrap, etc.) are absorbed into the core — they don't become crates

**Status:** decided
**Rationale:** Extensions like effort/tier routing, auth, bootstrap, tool-profile, auto-compact, model-budget, and offline-driver exist because pi's guest model requires explicit wiring for things the host should own natively. In the Rust architecture, the host owns these directly: tier routing is ContextManager config, auth is LLM bridge internals, bootstrap is startup checks, compaction is context decay, budget is SessionStats. Converting each to a separate crate would preserve unnecessary abstraction boundaries. They become functions in the core, not modules — ~1,120 lines total replacing ~8,000 lines of TypeScript.

### Decision: Rendering is the last migration layer — Phase 2, after the core and crates are proven

**Status:** decided
**Rationale:** Dashboard, splash, spinner/sermon, core-renderers, terminal-title, and style (~5,655 LoC) are pure rendering with zero business logic. In Phase 1, they run in the pi-tui bridge subprocess unchanged. In Phase 2, they become ratatui/crossterm components. Migrating them last is correct because: they're the most tightly coupled to the current TUI, they contain no logic that affects correctness, and they're the most visible to users (so they should only change once the underlying architecture is stable). The rendering layer subscribes to the AgentEvent broadcast channel and queries feature crates via a DashboardState trait.

### Decision: design-tree, openspec, and cleave are reclassified from feature crates to core lifecycle engine — see lifecycle-native-loop

**Status:** exploring
**Rationale:** The initial migration map placed these three systems as feature crates implementing ToolProvider. But they're not features — they're the cognitive architecture that makes Omegon more than a tool-calling loop. Design exploration, specification, and decomposition define how the agent thinks about work, not what the agent can do. They move into the core loop as the Lifecycle Engine, with ambient capture from the agent's reasoning replacing most explicit tool calls. See lifecycle-native-loop for the full exploration. This changes the crate count from 9 to 6 and shifts ~7,800 LoC from feature crates into the core loop.

### Decision: Lifecycle types live in the core — design-tree/openspec sharing is moot since both are in the Lifecycle Engine

**Status:** decided
**Rationale:** The lifecycle-native-loop reclassification moved design-tree, openspec, and cleave from feature crates into the core loop's Lifecycle Engine. The type-sharing question dissolves — both design nodes and spec changes are rows in lifecycle.db, sharing the same sqlite schema natively. No separate types crate needed.

### Decision: Feature crate migration order: web-search → local-inference → view → render → mcp → memory

**Status:** decided
**Rationale:** After the lifecycle reclassification, only 6 feature crates remain. Migration order follows complexity and dependency:
1. web-search (~200 LoC, pure HTTP, validates reqwest integration)
2. local-inference (~400 LoC, tests Ollama REST bridge)
3. view (~500 LoC, tests terminal protocol output + syntect)
4. render (~1,200 LoC, subprocess wrappers — straightforward but more surface area)
5. mcp (~800 LoC, tests MCP protocol integration with mcp-sdk-rs)
6. memory (~4,000 LoC, most complex, depends on the Omega memory backend work — migrates last)
Design-tree, openspec, and cleave are no longer in this list — they're in the core.

## Open Questions

*No open questions.*
