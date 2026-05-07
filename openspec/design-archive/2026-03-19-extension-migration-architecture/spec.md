+++
id = "17629e1f-0e09-42b1-bc47-c31c19da14c9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension migration architecture — TS extensions → Rust integrated features — Design Spec (extracted)

> Auto-extracted from docs/extension-migration-architecture.md at decide-time.

## Decisions

### Features are modules within the binary crate, not separate crates (decided)

The trait boundary IS the decoupling — a module implementing Feature is just as isolated as a separate crate. Separate crates add Cargo.toml proliferation, cross-crate compile overhead, and version coordination pain for zero additional type safety. Extract to crates only when a feature needs to be used from multiple binaries (e.g. omegon-memory is used by the main binary AND could be used by a standalone migration tool). The binary crate stays monolithic in structure but modular in design via the Feature trait.

### Bus types live in omegon-traits, bus runtime is a module in the binary crate (decided)

omegon-traits already defines the shared vocabulary (AgentEvent, ToolResult, etc). Bus event/request enums belong there because omegon-memory and any future extracted crate need them. The runtime (channel creation, dispatch loop, feature registry) stays in the binary crate because only the binary wires it.

### Sequential &mut self delivery — features processed in registration order (decided)

Interior mutability (Arc<Mutex>) adds contention and API complexity for no benefit — features don't need concurrent event processing. The bus delivers events to each feature sequentially in registration order. Features that need to communicate with each other do so via BusRequest, not direct calls. This mirrors how TS extensions worked (pi event hooks were sequential).

## Research Summary

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
  lifecycle, clea…

### TS extension API surface — what the extensions actually use

**160k LoC of TS extensions, but the API surface they use is narrow:**

Top pi API calls across all extensions:
- `ctx.ui` (395 uses) — TUI output: notify, setHeader, custom overlays
- `ctx.cwd` (120) — working directory
- `pi.on("event")` (63) — event subscription (session_start, turn_end, etc.)
- `ctx.hasUI` (45) — headless/interactive mode detection
- `pi.sendMessage` (39) — inject system/user messages into conversation
- `pi.registerTool` (37) — register agent-callable tools
- `pi.registerCo…

### Gap analysis — what omegon-traits is missing

**omegon-traits has 4 traits. The TS extension API has ~12 capabilities.**

What exists (maps cleanly):
- `ToolProvider` → `pi.registerTool` ✓
- `ContextProvider` → `before_agent_start` context injection ✓
- `EventSubscriber` → `pi.on("turn_end")` etc ✓
- `SessionHook` → `pi.on("session_start/shutdown")` ✓

What's missing:
1. **Command registration** — slash commands (`/model`, `/think`, `/splash`, `/memory`). Currently hardcoded in `tui/mod.rs`. No trait for feature crates to register commands.…

### Proposed architecture — feature crate convention + event bus

**The core insight: features aren't plugins, they're integrated subsystems.**

TS extensions used a plugin API because pi's codebase was closed. Omegon owns everything — there's no reason for dynamic registration. Features should be statically linked crates with well-defined interfaces.

**Proposed crate structure:**

```
omegon-traits      — trait definitions, event types, messages (keep thin)
omegon-bus         — NEW: typed event bus (broadcast + request/response)
omegon-memory      — memory f…

### Migration ordering — what to extract first

**Phase 0: Foundation (do BEFORE any extension migration)**
1. Define the `Feature` trait in `omegon-traits` (replaces 4 separate traits)
2. Build `omegon-bus` with typed events + request channels
3. Extract `omegon-tui` into its own crate (biggest untangling)
4. Wire the bus: loop emits events → bus fans out → features + TUI

**Phase 1: Extract existing code into feature crates**
These are already in the monolith — just need crate boundaries:
- `omegon-lifecycle` ← `lifecycle/` (design-tree, op…
