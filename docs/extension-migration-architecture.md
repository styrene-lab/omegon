+++
id = "b3e1788a-3d84-45cd-81fd-6b9f715e8286"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension migration architecture — TS extensions → Rust integrated features

## Overview

Evaluate the Rust crate structure for readiness to absorb TS extensions as native features. Determine what architectural changes (event bus, feature crate conventions, TUI integration points) are needed before migration begins.

## Research

### Current Rust architecture — what exists

**Three crates, ~23k LoC:**

```
omegon-traits (179 LoC)
  Four traits: ToolProvider, ContextProvider, EventSubscriber, SessionHook
  Event enum: AgentEvent (14 variants)
  Types: ToolResult, ToolDefinition, ContextInjection, ContextSignals

omegon-memory (3,405 LoC)
  SQLite factstore, implements ToolProvider + ContextProvider
  Decay, hashing, vector search, JSONL import/export

omegon (19,496 LoC) — the monolith
  Everything else: agent loop, LLM providers, auth, TUI, tools,
  lifecycle, cleave, session, migration, context, prompt, settings
```

**The problem: omegon is a monolith with no internal boundaries.**

The main binary crate has 19.5k LoC across 40+ files with no crate-level separation between concerns. All of these are siblings in one `src/` tree:
- Agent loop (`loop.rs` — 1,178 LoC)
- LLM providers (`providers.rs` — 724 LoC)
- TUI (`tui/` — 3,900 LoC across 10 files)
- Tools (`tools/` — 3,493 LoC across 11 files)
- Lifecycle/design-tree (`lifecycle/` — 1,665 LoC)
- Cleave orchestrator (`cleave/` — 1,797 LoC)
- Context/prompt (`context.rs` + `prompt.rs` — 985 LoC)
- Auth (`auth.rs` — 548 LoC)
- Conversation state (`conversation.rs` — 1,424 LoC)
- Settings/session/setup (`settings.rs` + `session.rs` + `setup.rs` — 975 LoC)

This worked during rapid prototyping. It will not scale for extension migration.

### TS extension API surface — what the extensions actually use

**160k LoC of TS extensions, but the API surface they use is narrow:**

Top pi API calls across all extensions:
- `ctx.ui` (395 uses) — TUI output: notify, setHeader, custom overlays
- `ctx.cwd` (120) — working directory
- `pi.on("event")` (63) — event subscription (session_start, turn_end, etc.)
- `ctx.hasUI` (45) — headless/interactive mode detection
- `pi.sendMessage` (39) — inject system/user messages into conversation
- `pi.registerTool` (37) — register agent-callable tools
- `pi.registerCommand` (34) — register slash commands
- `pi.exec` (29) — execute shell commands
- `pi.registerToolRenderer` (17) — custom tool result rendering

**Event hooks used:**
- `session_start` (19) — initialization
- `session_shutdown` (11) — cleanup
- `before_agent_start` (6) — pre-turn injection
- `turn_end` (5) — post-turn processing
- `tool_call` / `tool_execution_end` (3 each) — tool lifecycle
- `agent_end` (3) — turn completion

**The pattern is clear:** Extensions are just bundles of {tools, commands, event hooks, context injections}. This maps directly to Omegon's existing four traits.

**What DOESN'T map cleanly:**
1. `ctx.ui` — 395 uses. Extensions write to the TUI directly. In Rust, the TUI is its own task communicating via channels. Extensions can't directly call `frame.render_widget()`.
2. `pi.sendMessage` — 39 uses. Extensions inject messages into the conversation mid-turn. In Rust, the conversation state is owned by the agent loop.
3. `pi.registerToolRenderer` — 17 uses. Custom rendering per tool. In Rust, rendering is in the TUI crate.
4. `pi.registerCommand` — 34 uses. Slash commands need TUI integration.

### Gap analysis — what omegon-traits is missing

**omegon-traits has 4 traits. The TS extension API has ~12 capabilities.**

What exists (maps cleanly):
- `ToolProvider` → `pi.registerTool` ✓
- `ContextProvider` → `before_agent_start` context injection ✓
- `EventSubscriber` → `pi.on("turn_end")` etc ✓
- `SessionHook` → `pi.on("session_start/shutdown")` ✓

What's missing:
1. **Command registration** — slash commands (`/model`, `/think`, `/splash`, `/memory`). Currently hardcoded in `tui/mod.rs`. No trait for feature crates to register commands.
2. **TUI notifications** — `ctx.ui.notify()`. No channel from features to TUI for user-visible messages.
3. **Message injection** — `pi.sendMessage()`. Features can't inject messages into the conversation without going through the agent loop.
4. **Tool rendering hints** — `pi.registerToolRenderer()`. Features can't customize how their tool results look in the conversation view.
5. **Settings mutation** — features need to read/write shared settings (model, thinking level). Currently this is `SharedSettings` (Arc<Mutex<Settings>>), not exposed via trait.
6. **Mutable event bus** — `EventSubscriber::on_event(&self)` is `&self`, so subscribers can't maintain state. Extensions like `auto-compact` and `session-log` need mutable state.

The fundamental gap: **omegon-traits defines read-only, stateless participation**. Real features are stateful actors that both produce AND consume events, register UI elements, and mutate shared state.

### Proposed architecture — feature crate convention + event bus

**The core insight: features aren't plugins, they're integrated subsystems.**

TS extensions used a plugin API because pi's codebase was closed. Omegon owns everything — there's no reason for dynamic registration. Features should be statically linked crates with well-defined interfaces.

**Proposed crate structure:**

```
omegon-traits      — trait definitions, event types, messages (keep thin)
omegon-bus         — NEW: typed event bus (broadcast + request/response)
omegon-memory      — memory factstore (unchanged)
omegon-lifecycle   — NEW: extracted from omegon/src/lifecycle/
omegon-cleave      — NEW: extracted from omegon/src/cleave/
omegon-tui         — NEW: extracted from omegon/src/tui/
omegon             — binary: setup, main, wiring
```

**The event bus (`omegon-bus`):**

Not a generic pubsub — a typed, bidirectional coordination layer:

```rust
// Events flow DOWN (loop → features → TUI):
enum BusEvent {
    Turn(TurnEvent),      // start, end, soft-limit
    Tool(ToolEvent),      // call, result, error  
    Message(MsgEvent),    // chunk, thinking, complete
    Session(SessionEvent), // start, end, compact, switch
    Lifecycle(LifeEvent),  // phase-change, decomposition
    Ui(UiEvent),          // notification, command-response
}

// Requests flow UP (features → loop or TUI):
enum BusRequest {
    InjectMessage { role: String, content: String },
    RegisterCommand { name: String, handler: CommandFn },
    Notify { message: String, level: Level },
    MutateSetting { key: String, value: Value },
    RequestCompaction,
}
```

Key properties:
- **Typed, not stringly-typed** — no `pi.on("session_start")` string matching
- **Bidirectional** — features both subscribe to events AND send requests
- **Async channels** — tokio broadcast for events, mpsc for requests
- **Feature-specific** — each feature registers what it consumes at startup

**Feature crate convention:**

```rust
// omegon-lifecycle/src/lib.rs
pub struct LifecycleFeature { /* state */ }

impl Feature for LifecycleFeature {
    fn tools(&self) -> Vec<ToolDefinition> { ... }
    fn commands(&self) -> Vec<CommandDefinition> { ... }
    fn context(&self, signals: &Signals) -> Option<Injection> { ... }
    fn on_event(&mut self, event: &BusEvent) { ... }
}
```

The `Feature` trait unifies all four current traits + commands into one interface. Features are stateful (`&mut self`). The bus processes events sequentially per-feature (no concurrent mutation).

**TUI integration:**

The TUI subscribes to `BusEvent` like any other feature. Features communicate to the TUI via `BusRequest::Notify` and `BusRequest::RegisterCommand`. The TUI doesn't know about individual features — it just renders events and handles commands.

**What this replaces:**
- `pi.on("session_start")` → `Feature::on_event(BusEvent::Session(Start))`
- `pi.registerTool` → `Feature::tools()`
- `pi.registerCommand` → `Feature::commands()`
- `ctx.ui.notify` → `BusRequest::Notify`
- `pi.sendMessage` → `BusRequest::InjectMessage`
- `ctx.ui.setHeader` → hardcoded TUI knowledge (features don't customize rendering)

### Migration ordering — what to extract first

**Phase 0: Foundation (do BEFORE any extension migration)**
1. Define the `Feature` trait in `omegon-traits` (replaces 4 separate traits)
2. Build `omegon-bus` with typed events + request channels
3. Extract `omegon-tui` into its own crate (biggest untangling)
4. Wire the bus: loop emits events → bus fans out → features + TUI

**Phase 1: Extract existing code into feature crates**
These are already in the monolith — just need crate boundaries:
- `omegon-lifecycle` ← `lifecycle/` (design-tree, openspec, capture)
- `omegon-cleave` ← `cleave/` (orchestrator, plan, waves, worktree)

**Phase 2: Port "thin" TS extensions (tools + event hooks, no TUI)**
Easy wins — mostly tool definitions + event handlers:
- `chronos` ← `extensions/chronos/` (date/time tool, no UI)
- `web-search` ← already ported as Rust tool
- `local-inference` ← already ported
- `view` ← already ported
- `render` ← already ported
- `auto-compact` ← `extensions/auto-compact.ts` (39 LoC, turn_end hook)
- `session-log` ← `extensions/session-log.ts` (174 LoC, session hooks)
- `version-check` ← `extensions/version-check.ts` (check for updates)

**Phase 3: Port "thick" TS extensions (need bus + TUI integration)**
These are the ones that justify the event bus:
- `design-tree` ← full tool suite + lifecycle awareness + dashboard integration
- `openspec` ← tool suite + lifecycle + assessment hooks
- `project-memory` ← episodic memory, semantic search, injection (ALREADY native via omegon-memory, but needs reconciliation with TS version's injection modes)
- `cleave` ← native dispatch, review, assessment (partially native already)
- `model-budget` ← 752 LoC of effort/tier/cost tracking (needs settings mutation)
- `defaults` ← AGENTS.md management, convention detection (partially in prompt.rs)
- `dashboard` ← footer + dashboard rendering (already native in TUI)
- `splash` ← already ported
- `spinner-verbs` ← already ported

**Phase 4: Port or replace TUI-heavy extensions**
- `sermon` ← subagent message rendering (sermon-widget.ts)
- `style` ← color system (already in theme.rs)
- `tool-profile` ← tool visibility management
- `igor` ← adversarial review integration
- `offline-driver` ← local model fallback (partially in providers.rs)

**Never port:**
- `00-secrets` — Rust handles auth natively via auth.rs
- `01-auth` — same, native OAuth
- `bootstrap` — Node.js bootstrapping, irrelevant
- `web-ui` — browser-based UI, separate project
- `mcp-bridge` — defer to MCP SDK integration
- `core-renderers.ts` — pi-specific rendering hooks

### Migration priority assessment — scored by value, complexity, and dependency

**Scoring: Value (1-5) × Ease (1-5). Higher = do first.**

Tier 1 — Immediate (pure Feature ports, no new deps, high value):

| Extension | TS LoC | Value | Ease | Score | Notes |
|-----------|--------|-------|------|-------|-------|
| chronos | 668 | 5 | 5 | 25 | Pure computation, no I/O, no deps. 1 tool + 1 command. Already has tests. Port the date math to chrono crate. |
| terminal-title | 191 | 4 | 5 | 20 | Sets terminal tab title via ANSI escapes. Event subscriber only — on_event responds to TurnStart/ToolStart/AgentEnd. No tools, no commands. |
| version-check | 94 | 3 | 5 | 15 | HTTP fetch on session_start, hourly timer. Uses reqwest (already a dep). on_event → SessionStart. BusRequest::Notify for update alerts. |
| session-log | 174 | 3 | 4 | 12 | 1 command (/session-log). Reads/writes .session_log file. Injects recent entries on session_start. File I/O only. |

Tier 2 — Near-term (need settings/model integration):

| Extension | TS LoC | Value | Ease | Score | Notes |
|-----------|--------|-------|------|-------|-------|
| defaults | 274 | 4 | 3 | 12 | AGENTS.md management, convention detection. Partially in prompt.rs already. Needs settings mutation for quietStartup, auto-detection. |
| model-budget | 752 | 4 | 2 | 8 | Effort tiers, cost tracking, model selection policy. Needs SharedSettings access + model registry. Significant new state. |
| tool-profile | 763 | 3 | 3 | 9 | Tool visibility management. Needs bus to filter tool_definitions(). /profile command. |
| offline-driver | 410 | 3 | 2 | 6 | Model fallback to local. Needs provider registry, model switching. |

Tier 3 — Complex (lifecycle subsystems, deep integration):

| Extension | TS LoC | Value | Ease | Score | Notes |
|-----------|--------|-------|------|-------|-------|
| effort | 1264 | 4 | 2 | 8 | Effort calibration. Needs model registry, TUI widgets, shared state. |
| design-tree | 7921 | 5 | 1 | 5 | Full lifecycle tool suite. Already has native lifecycle/ module. Need to reconcile. |
| openspec | 6838 | 5 | 1 | 5 | Spec-driven dev. Same — partial native code exists. |
| cleave | 16510 | 5 | 1 | 5 | Task decomposition. Already has native cleave/ module. |
| project-memory | 12401 | 5 | 1 | 5 | Already native via omegon-memory. TS version has episodic memory + injection modes not yet ported. |

Never port:
- 00-secrets (native auth.rs)
- 01-auth (native auth.rs)
- bootstrap (Node.js bootstrapping)
- web-ui (separate project)
- mcp-bridge (defer to MCP SDK)
- core-renderers (pi-specific)
- sermon/sermon-widget (pi TUI component — need native equivalent)
- lib/ (shared TS utilities — rewrite as needed)

**Recommendation: Start with chronos + terminal-title + version-check.**

These three are pure Feature implementations with zero new dependencies, zero new crate boundaries, and zero integration complexity. They exercise every part of the bus: tools (chronos), commands (chronos, session-log), event subscriptions (terminal-title, version-check), context injection (session-log), and notifications (version-check). If all three port cleanly, the bus architecture is validated for the harder tiers.

## Decisions

### Decision: Features are modules within the binary crate, not separate crates

**Status:** decided
**Rationale:** The trait boundary IS the decoupling — a module implementing Feature is just as isolated as a separate crate. Separate crates add Cargo.toml proliferation, cross-crate compile overhead, and version coordination pain for zero additional type safety. Extract to crates only when a feature needs to be used from multiple binaries (e.g. omegon-memory is used by the main binary AND could be used by a standalone migration tool). The binary crate stays monolithic in structure but modular in design via the Feature trait.

### Decision: Bus types live in omegon-traits, bus runtime is a module in the binary crate

**Status:** decided
**Rationale:** omegon-traits already defines the shared vocabulary (AgentEvent, ToolResult, etc). Bus event/request enums belong there because omegon-memory and any future extracted crate need them. The runtime (channel creation, dispatch loop, feature registry) stays in the binary crate because only the binary wires it.

### Decision: Sequential &mut self delivery — features processed in registration order

**Status:** decided
**Rationale:** Interior mutability (Arc<Mutex>) adds contention and API complexity for no benefit — features don't need concurrent event processing. The bus delivers events to each feature sequentially in registration order. Features that need to communicate with each other do so via BusRequest, not direct calls. This mirrors how TS extensions worked (pi event hooks were sequential).

### Decision: First migration batch: chronos, terminal-title, version-check, session-log

**Status:** decided
**Rationale:** These four extensions collectively exercise every bus capability (tools, commands, event subscriptions, context injection, notifications) while being individually trivial to port. Total TS: ~1,127 LoC → estimated Rust: ~600-800 LoC. chronos validates tool+command registration. terminal-title validates pure event subscription with ANSI escape output. version-check validates async HTTP in on_event + session lifecycle. session-log validates file I/O + context injection. If all four work, the bus architecture is proven for the harder tiers.

### Decision: Only use Feature trait for stateful subsystems — simple tools are just functions

**Status:** decided
**Rationale:** The Feature trait exists for things with lifecycle and state (timers, counters, event-driven behavior). Stateless capabilities like chronos (pure date math) or session-log (file I/O) don't need the ceremony of a Feature implementation — they're a tool function in tools/ and a command match arm in the TUI. Pi's extension API forced everything into the same plugin shape. We don't have that constraint. Use the simplest representation that works: function > Feature > crate.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon-traits/src/lib.rs` (modified) — Unified Feature trait with tools(), commands(), execute(), provide_context(), on_event(&mut self) -> Vec<BusRequest>. BusEvent (14 variants), BusRequest (Notify, InjectSystemMessage, RequestCompaction). CommandDefinition + CommandResult.
- `core/crates/omegon/src/bus.rs` (new) — EventBus runtime — sequential &mut self delivery, feature registry, tool/command definition caching, drain_requests(), emit_harness_status()
- `core/crates/omegon/src/features/` (new) — 10 Feature impls: adapter.rs (3 legacy adapters), auto_compact.rs, cleave.rs, lifecycle.rs, manage_tools.rs, model_budget.rs, session_log.rs, terminal_title.rs, version_check.rs
- `core/crates/omegon/src/plugins/armory_feature.rs` (new) — ArmoryFeature — script/OCI tool execution for armory plugins
- `core/crates/omegon/src/plugins/http_feature.rs` (new) — HttpPluginFeature — HTTP endpoint tools from legacy manifests
- `core/crates/omegon/src/plugins/mcp.rs` (new) — McpFeature — MCP protocol tool servers (4 transport modes)

### Constraints

- Features are modules within the binary crate, not separate crates
- Bus types (BusEvent, BusRequest) live in omegon-traits; bus runtime in binary crate
- Sequential &mut self delivery — no concurrent mutation, registration-order processing
- 14 Feature impls total: 10 built-in features + 3 plugin types + legacy adapters
