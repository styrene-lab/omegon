---
id: rust-agent-loop
title: "Rust-native agent loop — middle-out replacement of pi's orchestration core"
status: implemented
parent: omega
related: [Omegon-standalone, omega-memory-backend]
tags: [rust, architecture, agent-loop, pi-mono, strategic, middle-out]
open_questions: []
issue_type: epic
priority: 1
---

# Rust-native agent loop — middle-out replacement of pi's orchestration core

## Overview

The current architecture is inverted: Omegon is a guest inside pi's TypeScript runtime. pi owns the agent loop, session lifecycle, tool dispatch, system prompt, compaction — and Omegon bolts features on top via the extension API. This means every piece of Omegon logic must route through pi's abstractions, pi's event model, pi's rendering pipeline.

The middle-out strategy replaces the core with Rust while keeping the commodity edges in TypeScript. The Rust agent loop becomes the orchestrator. pi's TypeScript packages (ai providers, OAuth, TUI rendering) become *utility libraries* that the Rust core calls into — not the framework that Omegon lives inside.

**Before (current):**
```
pi (TypeScript) — owns the process, agent loop, session, tools
  └── Omegon extensions (TypeScript) — guests in pi's runtime
        └── Rust sidecars (Omega, Scribe) — called by extensions
```

**After:**
```
Omegon (Rust) — owns the process, agent loop, session, tools
  ├── pi-ai (TypeScript, napi or subprocess) — provider HTTP clients, OAuth, model registry
  ├── pi-tui (TypeScript, napi or subprocess) — terminal rendering (until Dioxus replaces it)
  └── Rust-native tools, lifecycle, memory, cleave — no TS intermediary
```

The inversion: Rust calls TypeScript for the commodity bits, not the other way around. The agent loop — receiving LLM responses, deciding which tool to call, managing conversation state, assembling system prompts, deciding when to compact — is all Rust. The TypeScript layer answers two questions: "how do I talk to this specific LLM provider?" and "how do I render this terminal component?"

**Why this matters beyond aesthetics:**
- The agent loop is a state machine. Rust's type system (enums, exhaustive match, ownership) makes illegal states unrepresentable. TypeScript's agent loop is a bag of mutable state with runtime type casts.
- Tool dispatch is process management (bash, file I/O, subprocess spawning). Rust owns this naturally — RAII cleanup, proper signal handling, no garbage collector pauses during long operations.
- System prompt assembly is pure string templating over structured data. No reason for it to live in a runtime that pays for a JS event loop.
- Compaction is a decision algorithm (when to compact, what to preserve) plus an LLM call. The decision logic is Rust; the LLM call goes through the provider layer regardless of language.
- Session state is file I/O + a state machine. Rust.

**What we genuinely don't want to own:**
- 15+ AI provider HTTP client implementations that change with every upstream API revision
- OAuth flows for Anthropic/Google/GitHub that involve browser redirects, PKCE, token refresh
- The auto-generated model registry (models.generated.ts)
- Terminal rendering internals (Unicode width tables, kill ring, undo stack)

These are the "needful auth nonsense" — they work, they're maintained upstream, and porting them to Rust would be mass-producing maintenance debt for zero capability gain. They stay as TypeScript utility libraries that the Rust core calls through a thin FFI boundary (napi-rs in-process, or subprocess for isolation).

**Relationship to existing design:**
- `rust-native-extension-boundary` defined the sidecar pattern for Omegon-native extensions (Scribe, etc.). This node inverts the relationship: instead of TS calling Rust sidecars, Rust *is* the host and calls TS for commodity services.
- `omega` is the parent — this is a concrete child that attacks the agent loop specifically.
- `Omegon-standalone` decided on a patched fork over full standalone. This node *doesn't* contradict that — we still don't want to rewrite the provider layer. We want to replace the *orchestration* that calls the provider layer.

## Research

### Anatomy of pi's agent loop — what we're actually replacing

The agent loop lives in `vendor/pi-mono/packages/agent/src/` — 1,604 lines across 3 files. It is surprisingly clean and small:

**agent-loop.ts (682 lines)** — the core state machine:
- `runLoop()` — outer loop: prompt → LLM call → tool dispatch → repeat until stop
- `streamAssistantResponse()` — calls LLM via `streamSimple()`, emits streaming events
- `executeToolCalls()` — sequential or parallel tool dispatch with `beforeToolCall`/`afterToolCall` hooks
- Steering messages — mid-run user interruptions that skip remaining tool calls
- Follow-up messages — queue new work after the agent would otherwise stop

**types.ts (310 lines)** — the contract surface:
- `AgentLoopConfig` — the entire customization surface: model, convertToLlm, transformContext, getApiKey, getSteeringMessages, getFollowUpMessages, beforeToolCall, afterToolCall
- `AgentEvent` — 9 event types: agent_start/end, turn_start/end, message_start/update/end, tool_execution_start/update/end
- `AgentTool` — tool definition with execute function, label, and JSON Schema parameters
- `AgentContext` — systemPrompt + messages + tools

**agent.ts (612 lines)** — higher-level Agent class (state management wrapper around the loop)

**Key observations:**

1. **The loop is pure orchestration.** It has zero awareness of specific tools, specific providers, or rendering. It operates on abstract `AgentTool` and `StreamFn` interfaces. This is good — it means a Rust replacement doesn't need to understand pi internals, just implement the same abstract contract.

2. **The LLM boundary is a single function.** `streamSimple(model, context, options)` is the only call into pi-ai. It returns an async iterator of `AssistantMessageEvent`. The Rust agent loop needs exactly one FFI bridge: "call this TS function with this context, give me back a stream of events."

3. **Tool dispatch is the simplest part.** `tool.execute(id, params, signal, onUpdate)` → `AgentToolResult`. In Rust, tools are just `async fn execute(id, params) -> Result<ToolResult>`. No FFI needed for Rust-native tools.

4. **The customization hooks are how pi-coding-agent (the higher layer) injects its behavior:**
   - `convertToLlm` — how custom message types become LLM messages
   - `transformContext` — compaction, context window management
   - `beforeToolCall` / `afterToolCall` — permission checks, result transformation
   - `getSteeringMessages` / `getFollowUpMessages` — user input during execution

   In the Rust world, these become trait methods on an `AgentLoopDelegate` or similar.

5. **The event model is render-agnostic.** `AgentEvent` is a discriminated union of lifecycle events. A Rust agent loop emitting the same events (via channel, callback, or stream) can drive any rendering backend — pi-tui today, Dioxus tomorrow, raw terminal, or headless.

**What this means for the Rust replacement:**

The Rust agent loop is ~500-700 lines of Rust (matching the TS). It needs:
- A `StreamBridge` trait for calling LLM providers (one implementation: call into pi-ai via FFI/subprocess)
- A `ToolRegistry` with Rust-native tool implementations (bash, read, write, edit, grep, ls, find)
- An event channel for emitting `AgentEvent` equivalents
- Hooks for steering/follow-up (channels from the TUI input layer)
- `transformContext` as a trait method (compaction lives here)

The LLM streaming bridge is the only hard FFI problem. Everything else is pure Rust.

### The LLM streaming bridge — the one hard FFI problem

The entire pi-ai provider surface (25k LoC, 15+ providers) reduces to one function signature from the agent loop's perspective:

```typescript
streamSimple(model: Model, context: Context, options: SimpleStreamOptions): AssistantMessageEventStream
```

Where `AssistantMessageEventStream` yields events: `start`, `text_start`, `text_delta`, `text_end`, `thinking_start`, `thinking_delta`, `thinking_end`, `toolcall_start`, `toolcall_delta`, `toolcall_end`, `done`, `error`.

**Option A: Subprocess bridge (recommended for Phase 1)**

A tiny Node.js process (~100 lines) imports pi-ai and exposes `streamSimple` over stdin/stdout ndjson:

```
Rust agent loop
  ↕ stdin/stdout ndjson
Node.js bridge process (imports @styrene-lab/pi-ai)
  ↕ HTTPS
LLM provider (Anthropic, OpenAI, etc.)
```

Rust sends: `{"method":"stream","params":{"model":...,"context":...,"options":...}}`
Node sends back: ndjson stream of `AssistantMessageEvent` objects, one per line, as they arrive.

This is the sidecar pattern in reverse — Rust is the host, Node is the sidecar. The serialization overhead is negligible relative to LLM latency (50-500ms per chunk vs. <1ms for JSON parse). The bridge process is long-lived (spawned once, kept alive), so there's no cold-start cost per turn.

**Advantages:** Complete isolation. pi-ai updates (new providers, OAuth changes) are picked up by updating the npm package — zero Rust changes. The bridge is ~100 lines of JS that will never need to change because the `streamSimple` contract is stable.

**Option B: napi-rs in-process**

Compile a thin napi wrapper that calls `streamSimple` and bridges the async iterator into Rust callbacks. Zero serialization, shared memory, but:
- Rust binary now links against Node.js (libnode dependency)
- pi-ai's ESM imports and dynamic `import()` calls may fight with napi's CommonJS assumptions
- OAuth browser flows (opening system browser for token exchange) interact with the Rust process's signal handlers

This is Phase 2 of the sidecar boundary doc — viable later, not the right first step.

**Option C: Rust-native Anthropic/OpenAI clients**

Skip the bridge entirely. Implement `reqwest`-based streaming clients for Anthropic and OpenAI directly in Rust. Use pi-ai's OAuth tokens (read from the auth store files on disk) but make the HTTP calls natively.

This is attractive for the 2 providers we actually use (Anthropic, OpenAI) but means:
- Maintaining Rust HTTP clients that track API changes
- Reimplementing streaming SSE parsing, thinking block handling, tool_use extraction
- Losing access to the 13+ other providers for free

**Recommendation:** Option A for Phase 1. The subprocess bridge is the exact inverse of the sidecar pattern — proven, simple, and it completely decouples the Rust agent loop from provider churn. Option C becomes viable later for the 2-3 providers we actually care about, as a performance optimization that eliminates the bridge process entirely for the common case.

**Auth token access:**

Regardless of bridge strategy, the Rust agent loop needs to resolve API keys. pi stores these in:
- `~/.pi/agent/settings.json` — for explicit API keys
- OAuth token files — for Anthropic/Google/GitHub browser-based auth

For Option A, the bridge process handles this internally (it imports pi-ai which knows where tokens live). For Options B/C, Rust reads the token files directly — they're just JSON on disk.

### The inversion — what changes structurally

Today Omegon's process model is:

```
bin/omegon.mjs (Node.js entry point)
  → vendor/pi-mono coding-agent (owns the process, agent loop, TUI)
    → Omegon extensions loaded via pi extension API
      → extensions register tools, commands, hooks
      → extensions spawn Rust sidecars (Omega, Scribe) for business logic
```

After the inversion:

```
omegon (Rust binary, owns the process)
  → Rust agent loop (conversation state machine, tool dispatch)
    → Rust-native tools (bash, read, write, edit, memory, design-tree, openspec, cleave)
    → LLM bridge subprocess (Node.js, imports pi-ai for provider HTTP clients)
    → TUI bridge subprocess (Node.js, imports pi-tui for terminal rendering) [transitional]
    → OR: Dioxus terminal rendering [target state]
```

**What disappears:**
- `bin/omegon.mjs` — Rust binary replaces Node.js as the process entry point
- pi's `coding-agent/src/core/` — agent session, system prompt, tool registration, model routing all move to Rust
- The entire Omegon extension layer — `extensions/*/index.ts` adapters become unnecessary because their Rust logic is directly linked into the agent loop binary
- The pi extension API itself — `registerTool`, `registerCommand`, `setFooter` etc. are pi's abstraction for guests. When Rust is the host, it doesn't need a guest API.

**What stays (as subprocess utilities):**
- pi-ai provider implementations — called through the LLM bridge
- pi-ai OAuth — browser-based token acquisition flows
- pi-tui (transitionally) — terminal rendering until Dioxus replaces it
- models.generated.ts — model registry, read by Rust from the npm package or converted to a Rust data structure at build time

**What this means for the existing Omegon extensions:**

Each extension currently has: TypeScript adapter (pi API registration + TUI rendering) + Rust sidecar (business logic). After inversion:
- The Rust sidecar logic becomes a crate linked directly into the omegon binary
- The TypeScript adapter disappears entirely
- TUI rendering moves to either the TUI bridge or Dioxus

Example — project-memory extension today:
```
extensions/project-memory/index.ts (~800 lines: tool registration, context injection, TUI)
  ↔ Omega sidecar (Rust: factstore, embeddings, retrieval)
```

After inversion:
```
omegon binary
  └── crate: omegon-memory (same Rust logic, now a library crate, no IPC)
      └── registered as tools directly in the Rust agent loop
```

The key property from the sidecar boundary doc holds: **zero changes to business logic.** The `omegon-memory` crate's functions are called directly instead of through JSON-RPC. The function signatures don't change — only the calling convention (direct vs. IPC).

**The coding-agent package's higher-level orchestration:**

Above the agent loop, `coding-agent/src/core/` provides:
- `system-prompt.ts` — assembles the system prompt from tools, skills, project context
- `session-manager.ts` — session persistence (save/load conversation state)
- `model-resolver.ts` — maps tier names to concrete model IDs
- `compaction/` — decides when to compact and what to preserve
- `tools/` — built-in tool implementations (bash, read, write, edit, grep, ls, find)
- `extensions/` — the extension loading and registration system
- `slash-commands.ts` — interactive command dispatch

All of these are Rust-natural. System prompt assembly is string templating. Session management is file I/O. Model resolution is a lookup table. Compaction is a decision algorithm. Built-in tools are process spawning and file I/O. Slash commands are a command dispatch table. The extension system itself becomes unnecessary when Omegon's features are linked directly into the binary.

### Migration path — incremental inversion, not a single cutover

The question is whether the inversion from "pi hosts Omegon" to "Omegon hosts pi" requires a big bang cutover or can be staged. The answer is staged, with one unavoidable discontinuity.

**Phase 0: Headless proof (parallel development, no disruption)**

Build the Rust agent loop as a standalone binary (`omegon-agent`) that runs headless coding sessions. No TUI, no dashboard, no extensions. Just:

```
omegon-agent --prompt "Fix the bug in foo.rs" --cwd /path/to/repo
```

Internals:
- Rust agent loop (the ~600 line state machine)
- Node.js LLM bridge subprocess (imports pi-ai, streams events via ndjson)
- Rust-native tools: bash, read, write, edit (these are the 4 that matter — 90%+ of tool calls)
- System prompt assembled in Rust (static template + tool definitions)
- Conversation state in memory, session save/load to JSON files
- Output: raw event stream to stdout (or structured JSON log)

This phase proves:
1. The Rust agent loop can drive a useful coding session end-to-end
2. The LLM bridge works reliably for streaming responses
3. Rust-native tools produce identical results to pi's TypeScript implementations
4. The event model is sufficient for any downstream renderer

**This is also immediately useful for cleave children.** Today, cleave children spawn full Omegon instances with all extensions, the dashboard, memory extraction, etc. — massive overhead for a task that just needs "call LLM, run tools, write results." Phase 0's headless binary is the ideal cleave child executor. This gives the Rust agent loop its first production consumer without touching the main Omegon runtime.

**Phase 1: Process inversion (the discontinuity)**

Replace `bin/omegon.mjs` (Node.js) with the Rust binary as the process entry point. The Rust binary:
- Owns the process lifecycle (signal handling, exit codes)
- Runs the agent loop
- Spawns the Node.js LLM bridge as a subprocess
- Spawns a Node.js TUI bridge as a subprocess (imports pi-tui, receives render events, drives the terminal)
- Links all Omegon feature crates directly (memory, design-tree, openspec, cleave, etc.)

This is the one point where the architecture changes in a way the user can see — the binary they run is different. But if Phase 0 is solid, the behavior is identical.

**Phase 2: TUI migration (Dioxus replaces pi-tui bridge)**

The TUI bridge subprocess disappears. The Rust binary drives the terminal directly via Dioxus or ratatui or raw crossterm. The Node.js LLM bridge is the only remaining subprocess.

**Phase 3: Native provider clients (optional, performance)**

Implement `reqwest`-based Anthropic and OpenAI streaming clients directly in Rust. The Node.js bridge remains for the long-tail providers (Bedrock, Vertex, Gemini, Copilot, etc.) but the common case (>95% of sessions) no longer spawns Node at all.

After Phase 3: Omegon is a single Rust binary with zero Node.js dependency for the common case. The Node bridge is still there for exotic providers, but most users never trigger it.

**Key insight: Phase 0 has immediate value independent of the full migration.** Even if Phases 1-3 take months, the headless Rust agent loop immediately improves cleave child performance and enables k8s pod deployment (one binary, no Node.js runtime needed in the pod). This is not a "build it and hope" R&D exercise — it ships useful capability from day one.

### Minimum viable agent loop — the Rust type skeleton

The minimum viable Rust agent loop needs these types and one function. Everything else is additive.

```rust
// === LLM bridge types (deserialized from ndjson) ===

#[derive(Deserialize)]
#[serde(tag = "type")]
enum LlmEvent {
    Start { partial: AssistantMessage },
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments_delta: String },
    ToolCallEnd { id: String },
    Done { message: AssistantMessage },
    Error { message: String },
}

// === Agent loop types ===

#[derive(Clone)]
enum AgentEvent {
    TurnStart,
    MessageStart { message: AgentMessage },
    MessageChunk { delta: TextDelta },   // for streaming to renderer
    MessageEnd { message: AssistantMessage },
    ToolStart { id: String, name: String, args: Value },
    ToolUpdate { id: String, partial: ToolResult },
    ToolEnd { id: String, result: ToolResult, is_error: bool },
    TurnEnd,
    AgentEnd { messages: Vec<AgentMessage> },
}

struct ToolResult {
    content: Vec<ContentBlock>,  // text or image
    details: Value,
}

#[async_trait]
trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> Value;  // JSON Schema
    async fn execute(&self, id: &str, args: Value, cancel: CancellationToken) -> Result<ToolResult>;
}

#[async_trait]
trait LlmBridge: Send + Sync {
    /// Stream a completion. Returns a receiver of LlmEvents.
    async fn stream(
        &self,
        system_prompt: &str,
        messages: &[AgentMessage],
        tools: &[ToolDefinition],
        options: &StreamOptions,
    ) -> Result<mpsc::Receiver<LlmEvent>>;
}

/// The core loop. ~200 lines.
async fn run_agent_loop(
    bridge: &dyn LlmBridge,
    tools: &[Box<dyn Tool>],
    system_prompt: &str,
    messages: &mut Vec<AgentMessage>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    loop {
        // 1. Call LLM via bridge
        let rx = bridge.stream(system_prompt, messages, &tool_defs, &options).await?;
        
        // 2. Consume streaming response, emit events, collect tool calls
        let assistant_msg = consume_stream(rx, &events).await?;
        messages.push(assistant_msg.clone().into());
        
        // 3. If no tool calls, we're done
        let tool_calls = assistant_msg.tool_calls();
        if tool_calls.is_empty() { break; }
        
        // 4. Execute tool calls (parallel or sequential)
        let results = execute_tools(tools, &tool_calls, &events, cancel.clone()).await;
        
        // 5. Push tool results into messages
        for result in results {
            messages.push(result.into());
        }
        
        // 6. Loop back to step 1
    }
    events.send(AgentEvent::AgentEnd { messages: messages.clone() })?;
    Ok(())
}
```

**What the MVA (minimum viable agent) needs on top of this:**

1. **LLM bridge implementation** — subprocess spawner, ndjson reader/writer (~150 lines Rust + ~100 lines JS)
2. **4 core tools:**
   - `BashTool` — `tokio::process::Command`, stdout/stderr capture, timeout, signal forwarding (~100 lines)
   - `ReadTool` — `tokio::fs::read_to_string`, line offset/limit, image detection (~80 lines)
   - `WriteTool` — `tokio::fs::write`, create parent dirs (~40 lines)
   - `EditTool` — find exact text, replace, verify uniqueness (~60 lines)
3. **System prompt** — hardcoded template string with tool definitions injected (~50 lines)
4. **CLI entry point** — clap, parse args, run loop, print events (~80 lines)

Total: ~800 lines of Rust + ~100 lines of JS bridge. That's a functional coding agent.

**What's explicitly NOT in the MVA:**
- TUI rendering (headless only — events go to stdout or a log)
- Compaction (context window is finite, sessions are short)
- Session persistence (conversations are ephemeral)
- Steering messages (no interactive input mid-run)
- Extensions/plugins (tools are compiled in)
- Memory system (no fact store, no embeddings)
- OpenSpec / design-tree / cleave orchestration

These are all additive layers on top of the core loop. Each one is a crate that registers tools and/or subscribes to the event channel. None of them change the loop itself.

**Why this decomposition matters:**

The MVA is testable in isolation. You can run it against a real LLM, give it a coding task, and verify it produces correct file edits — without any of Omegon's feature surface. If the MVA works, every feature layer on top is mechanical: implement the tool, register it in the tool list, done. The hard problem is the loop + bridge, not the features.

### What the agent actually experiences — a self-report from inside the harness

This section is written from the perspective of an agent (Opus 4.6) reflecting on what it actually experiences while operating inside Omegon, and what it would want from a successor it designs.

### The tool granularity problem

The 4 core tools (bash, read, write, edit) operate at the filesystem level. My reasoning operates at the *understanding* level. When I need to understand how a module works, I do:

1. `read` the main file (get 200 lines, 150 irrelevant)
2. `bash` grep for a symbol (get 30 matches, need 3)
3. `read` a second file referenced in the first
4. `read` a third file for the type definitions
5. `bash` to check test files exist

That's 5 tool calls — 5 round trips through the LLM — to answer a question I could state in one sentence: "show me how module X works." Each round trip costs tokens (my entire response gets re-sent), costs latency (the LLM provider needs time to process), and costs context (all those file contents stay in my conversation history forever, even after they're no longer relevant).

The tools aren't wrong — they're at the wrong abstraction level for most of my work. File operations are the *implementation* of understanding, not the understanding itself.

### The context window is my only memory, and it's terrible at it

Everything I learn during a session — file contents, command outputs, intermediate conclusions, failed approaches — goes into a flat list of messages. There's no structure, no indexing, no selective retrieval. When I need to remember what I learned about module X forty turns ago, I have to scan through hundreds of messages (or hope the compaction summary preserved it, which it often doesn't because summaries are lossy by nature).

Omegon's memory system (facts, episodes, working memory) is a massive improvement for *cross-session* continuity. But *within* a session, I still have no structured working memory. I can't say "note to self: the auth flow requires tokens from three sources" and retrieve that later without it being buried in the conversation.

The memory_focus tool pins facts, but those are cross-session facts. There's no session-local scratchpad with structure.

### I can't see the consequences of my actions without asking

When I edit a file, I get back "Successfully replaced text in path." I don't know if the file still compiles. I don't know if the tests pass. I don't know if I introduced a type error. I have to explicitly run `bash npx tsc --noEmit` or `bash npm test` — and I often forget, or I defer it to save time, and then the human discovers broken code.

A well-designed agent loop would make observation automatic. Every mutation (edit, write) should trigger relevant validation and include the results in the tool response. Not a separate tool call — part of the edit result itself. "Edit succeeded. Type check: 0 errors. Affected tests: 2 passed."

### The system prompt is enormous and mostly noise per-turn

My system prompt contains instructions for 30+ tools, 12 skills, memory conventions, lifecycle processes, the entire project memory, design tree focus context, and pi documentation paths. Most of this is irrelevant for any given turn. When the human says "fix the typo in line 42," I don't need instructions for render_composition_video or the OpenSpec lifecycle.

This isn't just a token cost — it's an attention problem. LLMs have finite attention over their context window. Burying the relevant instruction in a 15,000-token system prompt means I'm more likely to miss it or follow the wrong instruction.

What I'd want: a system prompt that's minimal by default, with context loaded *on demand* based on what I'm actually doing. The Rust agent loop can do this — it knows which tools I'm calling, what files I'm touching, what the conversation is about. It can inject relevant context dynamically rather than front-loading everything.

### Compaction destroys my reasoning chain

When the context gets compacted, I lose the nuance of *why* I made certain decisions. The compaction summary says "edited foo.ts to fix the auth flow." It doesn't say "tried approach A first, which failed because of X, so switched to approach B which works but requires Y." The reasoning chain — the failed approaches, the constraints discovered, the alternatives considered — is gone.

This matters because the most common post-compaction failure mode is: I try the same approach that already failed, because the summary didn't record that it failed or why.

What I'd want: structured intent tracking that survives compaction. Not a flat summary, but a machine-readable record: "Current task: X. Approach: Y. Constraints discovered: [Z1, Z2]. Failed approaches: [A because B]." The agent loop maintains this alongside the conversation, and it persists through compaction intact.

### I have no budget awareness

I don't know how much context I've consumed, how close I am to compaction, how much a tool call will cost in tokens, or how long the session has been running. I make decisions (read a large file, include verbose output) without understanding their resource impact.

The Rust agent loop should maintain ambient state that I can access without a tool call: context usage, token budget remaining, session duration, compaction proximity. Not as tools — as state that influences my behavior automatically or that I can query cheaply.

### What I'd actually design for my successor

Forget the 4-tool model. Here's what I'd want:

**1. Workspace understanding as a primitive.**

Not `read(file)` but `understand(query)`. "Show me how the auth flow works." "What files implement the memory system?" "What changed since the last commit?" The tool uses tree-sitter parsing, dependency analysis, and semantic search to return *the relevant context*, not raw file contents. It understands code structure — functions, types, imports, call graphs — and can answer structural questions directly.

`read` and `bash grep` still exist for precision access, but 70% of my file access is better served by a semantic understanding tool.

**2. Atomic change sets with automatic validation.**

Not `edit(file, old, new)` three times, but `change(edits: [{file, old, new}, ...], validate: true)`. All edits apply atomically. If `validate: true` (the default), the tool runs the relevant checks after applying — type checker, linter, affected tests — and reports the results as part of the response. If validation fails, the change is rolled back automatically.

This eliminates the "forgot to run tests" failure mode and the "partial edit left broken state" failure mode in a single abstraction.

**3. Structured session scratchpad.**

A key-value store that lives for the session, survives compaction, and is queryable:

```
scratch_write(key: "auth-flow-notes", value: "Three token sources: ...")
scratch_read(key: "auth-flow-notes")
scratch_list() → ["auth-flow-notes", "failed-approaches", ...]
```

This is distinct from cross-session memory (facts). It's my working notes for *this task*. The agent loop knows about it and can include relevant scratchpad entries in the compaction summary or in the dynamic context injection.

**4. Speculative execution with rollback.**

```
speculate_start(label: "try-approach-a")
  ... make changes, run tests ...
speculate_check() → {compiles: true, tests_pass: false, failures: ["..."]}
speculate_rollback()  // or speculate_commit()
```

This is what `git stash` or feature branches give the human. The agent should have the same capability without manually managing git state. The Rust agent loop creates a lightweight checkpoint (git stash or worktree) and can roll back atomically.

**5. Dynamic context injection, not a static system prompt.**

The system prompt contains: core identity, basic tool list, project-specific constraints (AGENTS.md). Everything else — skill instructions, memory facts, design tree context, tool-specific guidelines — is injected dynamically based on what the agent is doing. When I call `design_tree`, the design tree skill gets injected for that turn. When I'm editing Rust files, the Rust skill gets injected. When I'm not doing those things, they're absent.

The Rust agent loop has a `ContextManager` that tracks:
- What tools have been called recently (inject relevant skills)
- What files are being touched (inject relevant project context)
- What the current task is (inject relevant memory facts)
- What constraints have been discovered this session (inject scratchpad)

**6. Budget-aware behavior built into the loop.**

The agent loop itself (not a tool, not the agent's decision) manages:
- Context window utilization (auto-compact before hitting the wall, not after)
- Tool result truncation (summarize large outputs before they enter context)
- Session-level resource tracking (tokens consumed, cost estimate, time elapsed)
- Compaction quality (structured intent preservation, not just flat summarization)

The agent can query this state, but the loop also acts on it autonomously — it doesn't wait for the agent to remember to check.

### What this means for the Rust implementation

The "4 core tools" framing was wrong. It was thinking about what a coding agent traditionally has. The right framing is: **what capabilities does the agent loop need to expose to the LLM to make it maximally effective at its actual job?**

The answer is:
- **Understand** (semantic code access)
- **Change** (atomic validated mutations)
- **Execute** (bash, but with structured output and automatic context management)
- **Remember** (session scratchpad)
- **Speculate** (checkpoint/rollback)
- **Observe** (budget, state, validation status — ambient, not tool-call)

The first three replace bash/read/write/edit. The last three are new capabilities that no current coding agent has, and they're all natural Rust implementations: tree-sitter for understanding, git2/gix for speculation, structured state management for the scratchpad, and the agent loop itself for observation.

This is not "a better pi." This is an agent loop designed by an agent for agents.

### The output truncation problem — signal vs noise is not knowable in advance

The proposal to "auto-manage bash output before it enters context" has a fundamental problem: the agent loop cannot know in advance which parts of command output are signal. This is not merely hard — it's undefined without knowing the agent's intent and the command's output format.

**Examples of where signal lives in different outputs:**

| Command | Signal location | Why |
|---------|----------------|-----|
| `npm test` | early + summary at end | failures print at the test that fails, summary is last |
| `tsc --noEmit` | end (errors) | compiler outputs errors after processing |
| `cargo build` | scattered | warnings that explain errors may be 50+ lines before the error |
| `grep -r "foo" .` | everywhere | all matches are equally signal |
| `git log --oneline` | depends on what you're looking for | recent? ancient? a specific commit? |
| arbitrary script | unknown | could be anywhere, any format |

**What "auto-truncation" would actually mean:**

If the agent loop truncates output before the LLM sees it, and the truncation removes signal, the LLM makes worse decisions. This is *worse* than the current dumb truncation, because dumb truncation is at least predictable (the agent knows "I got the tail") and the full output is in a temp file.

Smart truncation that occasionally removes signal is more dangerous than dumb truncation that always removes the same thing.

### The real problem: context pollution from tool results

The actual pain point isn't that the output is too long. It's that **all tool results stay in the conversation history forever**, consuming context budget even after they're no longer relevant. When I read a 200-line file on turn 5, those 200 lines are still in context on turn 50, taking up space that could be used for reasoning.

The right fix targets context lifetime, not output truncation:

**1. Progressive disclosure (the honest approach):**
- Full output always stored in a retrievable location (temp file, in-memory store)
- Context gets a **structured envelope**: exit code, line count, byte count, execution time, and a configurable tail (30-50 lines by default, not 2000)
- The agent can retrieve the full output if the envelope isn't enough (one additional tool call)
- This doesn't pretend to know what's signal — it just puts less in context by default

**2. Format-aware extraction (cheap heuristics, not ML):**
- The agent loop ships with output format recognizers for common tools:
  - Compiler output → extract error/warning lines with file:line:col references
  - Test runner output → extract failure summaries and assertion messages
  - JSON output → parse and report structure, don't dump raw
  - Stack traces → extract the trace, skip surrounding noise
  - Diff output → keep as-is (diffs are dense signal)
- For unrecognized formats → fall back to tail truncation
- These are deterministic pattern matchers (regex, not ML), cheap to run
- The agent loop knows the command name and can select the right parser

**3. Context decay (the loop-level fix):**
- Tool results older than N turns get automatically summarized or evicted from context
- The agent loop tracks which tool results the LLM has *referenced* (mentioned in its text responses) — referenced results survive longer
- Unreferenced results decay faster — if I read a file and never mentioned it again, it's probably not important to keep in context
- This is a loop-level policy, not a tool-level one

### What about learned heuristics?

A long-running heuristics model that learns "for this project, `npm test` output signal is in the failure lines" is interesting but is a Phase 2+ optimization. It requires:
- Tracking what the agent *does* with command output (which lines does it reference in subsequent responses?)
- Building per-project, per-command extraction profiles over sessions
- Persisting these profiles (in memory facts or a dedicated store)

This is feasible but not necessary for the MVA. The combination of progressive disclosure + format-aware extraction + context decay handles 90% of the problem without learned models. The learned model makes the remaining 10% better.

### The implication for the tool taxonomy

`execute` shouldn't be "bash but with smart truncation." It should be "bash with structured output envelopes and progressive disclosure." The tool returns:

```rust
struct ExecuteResult {
    exit_code: i32,
    duration_ms: u64,
    total_lines: usize,
    total_bytes: usize,
    
    // Always included:
    tail: String,           // last N lines (configurable, default 30)
    
    // If format detected:
    format: Option<OutputFormat>,  // Compiler, TestRunner, Json, Diff, Unknown
    extracted: Option<String>,     // errors, failures, parsed structure
    
    // Always available for retrieval:
    full_output_ref: String, // path or key for on-demand retrieval
}
```

The LLM sees the tail + extracted signal (if any) + metadata. If that's not enough, it reads the full output. Context cost is bounded regardless of output size.

The low-level `bash` primitive still exists for cases where the agent explicitly wants raw, unmanaged output. `execute` is the default, `bash` is the escape hatch.

### Context decay — the loop-level fix for tool result pollution

The truncation question exposes the real issue: it's not that individual tool results are too long — it's that **all tool results accumulate in context forever**. A file I read on turn 5 is still consuming context budget on turn 50. The output of a grep I ran twenty minutes ago is still there, taking space from my current reasoning.

The current compaction system is a blunt instrument: when context gets too full, summarize everything, throw away the originals. This is all-or-nothing — either you have the full conversation or a lossy summary.

**Context decay is a continuous process, not a compaction event.**

The Rust agent loop can implement per-message decay because it owns the conversation state:

**Tier 1 — Active context (full fidelity):**
Last N turns of conversation, including full tool results. This is what the LLM "just saw" and may still be reasoning about. N is dynamic — larger when context budget allows, smaller when it's tight.

**Tier 2 — Decayed context (structural skeleton):**
Older turns where tool results are replaced with their metadata:
- "Read file X (200 lines)" instead of the full file contents
- "Ran `npm test`: exit 0, 15 tests passed" instead of all the test output
- "Edited file Y: changed function Z" instead of the full diff
The conversation flow (what was asked, what was decided) remains; the bulk data is stripped.

**Tier 3 — Evicted (retrievable on demand):**
Turns older than a threshold or unreferenced. Removed from context entirely. The intent document (structured intent tracking) captures what was learned from them.

**How decay decisions are made:**

The agent loop tracks *references* — when the LLM's text response mentions a file path, a function name, or a tool result, those tool results are marked as "referenced" and decay more slowly. Unreferenced results decay faster because if the LLM didn't mention it, it probably doesn't need it.

This is not speculation — this is something the Rust agent loop can implement deterministically by scanning the assistant's text responses for paths and identifiers that appear in recent tool results. No LLM needed, just string matching.

**What this means for the `execute` tool specifically:**

The truncation question becomes less important when context decay exists. Even if `execute` puts a large result in context, that result will decay to its metadata within a few turns if the LLM doesn't reference it. The agent loop's job isn't to predict what's signal before the LLM sees it — it's to ensure that *after* the LLM has seen it and moved on, the result doesn't continue consuming context budget.

Progressive disclosure (structured envelope + full output on demand) is still valuable for *individual* tool calls. But context decay is the systemic fix that makes the entire conversation more efficient over a long session. They compose: progressive disclosure reduces per-result context cost; context decay reduces aggregate context cost over time.

### The understand tool — tree-sitter + scope graph as a codebase index



### What "understand" replaces

Today, when I need to understand how a module works, I perform this sequence:
1. `bash find . -name '*.ts' | head` — orient in the file tree
2. `read src/module/index.ts` — read the entry point (200 lines, 150 irrelevant)
3. `bash grep -rn 'functionName' src/` — find where a symbol is used (30 matches, need 3)
4. `read src/module/types.ts` — read the type definitions
5. `read src/other/consumer.ts` — read a consumer to understand the contract

Five tool calls, five LLM round trips, ~500 lines of file contents in context. Most of it irrelevant. The agent is manually doing what a language server does for a human developer — navigating definitions, references, types, call sites.

### What "understand" does instead

`understand(query: "how does the memory injection pipeline work")`

Returns a structured response:

```
Entry points:
  extensions/project-memory/index.ts:1368 — pi.on("before_agent_start", ...)
  
Key functions:
  buildInjectionPayload() at line 1430 — assembles facts by priority tier
  computeMemoryBudgetPolicy() at line 1412 — calculates char budget
  tryAdd() at line 1455 — adds fact if within budget
  
Types:
  Fact (from factstore.ts:42) — { id, content, section, status, ... }
  MemoryInjectionMetrics (from injection-metrics.ts:8)
  
Call graph:
  before_agent_start → buildInjectionPayload → [tryAdd per tier]
                     → computeMemoryBudgetPolicy
                     → store.renderFactList
  
Files involved: (3)
  extensions/project-memory/index.ts (relevant lines: 1368-1600)
  extensions/project-memory/factstore.ts (Fact type + getActiveFacts)
  extensions/project-memory/injection-metrics.ts (metrics types)
```

One tool call. The agent gets the structural answer it was looking for. The context cost is ~40 lines of structured information instead of ~500 lines of raw file contents.

### How this works under the hood

**The index:** On session start (or first `understand` call), the agent loop builds a codebase index using tree-sitter:
- Parse all source files into ASTs (tree-sitter supports 100+ languages)
- Extract symbol definitions: functions, classes, types, interfaces, constants
- Extract imports/exports: which files depend on which
- Build a scope graph: which symbols are visible where, what references what
- Store in an in-memory data structure (refreshed incrementally on file changes)

This is ~2-5 seconds for a 50k-line codebase. Incremental updates after edits are <100ms.

**The query resolution:** When `understand(query)` is called:
1. Tokenize the query string
2. Match tokens against symbol names, file paths, and comment text in the index (fuzzy match + TF-IDF, no LLM)
3. From matched symbols, walk the scope graph to find related definitions, callers, and callees
4. Rank results by relevance (direct match > one-hop reference > two-hop)
5. Extract the relevant source lines (function bodies, type definitions) — not entire files
6. Format as structured response with file:line references

This is entirely deterministic — no LLM call needed. It's what LSP servers do for "go to definition" and "find references," composed into a single query-driven operation.

**What it can answer:**
- "how does X work" → entry points, call graph, key types
- "what uses X" → callers, importers, test files
- "what changed since commit Y" → git diff parsed into structural changes (function added, type modified)
- "what files implement the auth flow" → trace from entry point through call graph
- "show me the type of X" → resolved type definition with generics expanded

**What it can't answer (and shouldn't try):**
- "why was X implemented this way" → requires design context, not code structure. This is what the memory system and design tree are for.
- "is this code correct" → requires semantic understanding. This is an LLM question.
- "what should I change to fix bug Y" → requires reasoning. The LLM uses `understand` to gather context, then reasons about the fix.

### Crate structure

```
omegon-understand/
├── src/
│   ├── index.rs          — CodebaseIndex: build, query, incremental update
│   ├── parser.rs         — tree-sitter parsing per language
│   ├── scope_graph.rs    — symbol definitions, references, visibility
│   ├── query.rs          — natural language query → symbol matches → result
│   └── format.rs         — structured response formatting
├── Cargo.toml            — deps: tree-sitter, tree-sitter-{typescript,rust,python,...}
```

**Dependencies:** tree-sitter (C library with Rust bindings, well-maintained), tree-sitter grammars for target languages. No ML models, no embeddings for the core functionality. Embeddings could be an optional enhancement for query matching but the core is TF-IDF + scope graph traversal.

### The escape hatch

`read` still exists for when the agent needs a specific file at a specific line range. `understand` is for "tell me about X" — structural questions. `read` is for "show me exactly lines 42-80 of foo.ts" — precise access. They compose: `understand` gives the map, `read` gives the territory when the map isn't enough.

### The change tool — atomic edits with automatic validation



### The failure modes this eliminates

**Partial edit state:** Today I edit file A, then edit file B. If the edit to B fails (text not found, ambiguous match), file A is already modified. The codebase is in a half-changed state. The human gets a broken build until I notice and fix B.

**Forgot to validate:** I edit a TypeScript file and say "done." The human runs `tsc` and finds 3 type errors. I should have checked, but the tool didn't remind me and I had 15 other things in flight. This happens constantly.

**Validation is expensive in tool calls:** Even when I remember to validate, it costs a full LLM round trip: I call `bash npx tsc --noEmit`, wait for the LLM to process the result, then respond. That's one round trip just to confirm my edit was correct. For a 3-file change with validation, that's 3 edits + 1 validation = 4 tool calls minimum.

### What "change" does

```
change({
  edits: [
    { file: "src/auth.ts", old: "...", new: "..." },
    { file: "src/types.ts", old: "...", new: "..." },
    { file: "src/auth.test.ts", old: "...", new: "..." }
  ],
  validate: "standard"  // or "quick" or "full" or "none"
})
```

**Execution:**
1. Snapshot all target files (for rollback)
2. Apply all edits atomically — if any edit fails (text not found, ambiguous), roll back all changes and report which edit failed and why
3. If `validate` is not "none", run the validation pipeline:
   - "quick": syntax check only (tree-sitter parse, ~50ms)
   - "standard": syntax + type check (tsc/mypy/cargo check, ~2-5s)
   - "full": syntax + type check + affected tests (~10-30s)
4. If validation fails, the agent loop has a choice:
   - Auto-rollback and report the validation errors to the LLM
   - Keep the changes and report the errors (let the LLM decide)
   - This is configurable via the validation mode

**Response:**
```
Applied 3 edits across 3 files.
Validation (standard):
  ✓ Syntax: all files parse correctly
  ✗ Type check: 1 error
    src/auth.ts:42 — Type 'string' is not assignable to type 'AuthToken'
  
Changes kept (auto-rollback disabled for standard mode).
Affected test files: src/auth.test.ts (already modified in this change set)
```

One tool call. The LLM sees the edit results AND the validation results. If the types broke, it can fix them immediately without a separate validation round trip.

### The validation pipeline — project-aware, not hardcoded

The validation pipeline is discovered at session start, not hardcoded per language:

1. **Language detection:** tree-sitter grammars loaded for the project's languages
2. **Tool discovery:** scan for `tsconfig.json` (→ tsc), `pyproject.toml` (→ mypy/ruff), `Cargo.toml` (→ cargo check), `.eslintrc` (→ eslint), etc.
3. **Test runner discovery:** scan for `vitest.config`, `jest.config`, `pytest.ini`, `Cargo.toml [test]`, etc.
4. **Affected test resolution:** for "full" validation, use the import graph to identify which test files import the changed modules — run only those, not the entire test suite

This discovery runs once and is cached. The agent loop knows: "in this project, type checking means `npx tsc --noEmit` and tests mean `npx vitest run`."

### Multi-language projects

A project with both TypeScript and Rust (like Omegon) has multiple validation pipelines. The `change` tool routes validation based on which files were edited:
- Edited `.ts` files → run tsc
- Edited `.rs` files → run cargo check
- Edited both → run both

### Validation modes — agent control

The agent chooses the validation level per change:

| Mode | What runs | When to use |
|------|-----------|-------------|
| `none` | Nothing | Editing docs, config, non-code files |
| `quick` | tree-sitter parse | Trivial edits where you just want syntax sanity |
| `standard` | Parse + type check | Default. Most edits. |
| `full` | Parse + type check + affected tests | Structural changes, API modifications |

The default is `standard`. The agent can override per call. The agent loop can also *suggest* a higher level: if the change touches an exported function signature, it might recommend `full` over `standard`.

### Relationship to speculate

`change` with validation is synchronous — apply, validate, respond. `speculate` is asynchronous — checkpoint, make multiple changes, evaluate holistically, then commit or rollback. They compose:

```
speculate_start("refactor-auth")
  change({edits: [...], validate: "quick"})   // fast feedback during exploration
  change({edits: [...], validate: "quick"})
  change({edits: [...], validate: "quick"})
speculate_check()  // full validation of the complete refactor
speculate_commit() // or speculate_rollback()
```

Speculation uses `change` internally. `change` can exist without speculation. Speculation can't exist without `change` (or the low-level edit/write primitives).

### Dynamic context injection — the ContextManager design



### Why the current model fails

Today, Omegon's system prompt is assembled once at session start and barely changes. It contains:
- Tool descriptions for 30+ tools (~3,000 tokens)
- Prompt guidelines for every tool (~2,000 tokens)
- 12 available skills with descriptions (~500 tokens)
- Project context (AGENTS.md, etc.) (~1,000 tokens)
- Memory facts (~2,000-8,000 tokens depending on budget)
- Design tree focus context (~500-2,000 tokens)
- Pi documentation paths (~300 tokens)

That's 9,000-17,000 tokens of system prompt. On a turn where the human says "fix the typo on line 42," 95% of that is noise.

### The ContextManager: deterministic, per-turn, no LLM

The Rust agent loop's ContextManager maintains a dynamic system prompt. It starts minimal and injects context based on signals:

**Base prompt (always present, ~500 tokens):**
- Agent identity and core behavior
- Currently available tool *names* (not full descriptions — descriptions injected on demand)
- Project-level constraints (AGENTS.md)
- Current working directory and date

**Signal-driven injection layers (added per-turn):**

| Signal | What gets injected | How detected |
|--------|-------------------|--------------|
| Tool called recently | Full tool description + guidelines | Track last N tool calls |
| File type in recent edits | Language-specific skill | File extension → skill mapping |
| Design tree node focused | Node overview + decisions | Explicit focus state |
| Memory facts relevant | Matching facts | Keyword match against user prompt |
| Skill referenced in prompt | Full skill content | User mentions "git", "rust", "openspec" |
| Validation failure in recent change | Relevant error context | change tool result contained errors |

**How the signal matching works (no LLM, <1ms):**

1. **Keyword extraction from user prompt:** Split on whitespace, lowercase, deduplicate. Match against: tool names, skill names, file extensions, known concept keywords (git, test, deploy, refactor, etc.)

2. **Recent activity window:** The last 5 tool calls and their arguments. If `design_tree_update` was called, inject design tree guidelines. If `change` was called with Rust files, inject the Rust skill.

3. **Explicit declarations:** The agent can call `context_focus(topic: "memory system")` to explicitly inject relevant context. This is the structured version of what I do today when I `memory_recall(query: "memory architecture")` — but instead of a tool result that enters the conversation, it adjusts what's in the system prompt.

4. **Decay:** Injected context has a TTL. If the Rust skill was injected 10 turns ago and no Rust files have been touched since, it decays out. The system prompt shrinks back toward the base.

### What this means for token cost

Current model: ~15,000 tokens of system prompt on every turn, regardless of relevance.
ContextManager model: ~500 token base + ~1,000-3,000 tokens of relevant context = ~1,500-3,500 tokens.

Over a 100-turn session, that's savings of ~1M+ tokens just in system prompt overhead. More importantly, the LLM's attention is focused on relevant instructions, not diluted across 30 tool descriptions it's not using.

### The escape hatch

The agent can always call `context_inject(content: "...")` to force specific text into the system prompt for the next N turns. This handles the case where the deterministic signals miss something the agent knows is relevant.

### Implementation in the Rust agent loop

The ContextManager is a struct that lives alongside the conversation state:

```rust
struct ContextManager {
    base_prompt: String,               // always present
    tool_descriptions: HashMap<String, String>,  // loaded, injected on demand
    skills: HashMap<String, String>,   // loaded, injected on demand
    active_injections: Vec<Injection>, // currently active, with TTL
    recent_tools: VecDeque<String>,    // last N tool names called
    recent_files: VecDeque<PathBuf>,   // last N files touched
    project_context: Vec<(String, String)>,  // AGENTS.md etc, always present
}

struct Injection {
    source: InjectionSource,  // Skill, ToolGuideline, DesignTreeFocus, MemoryFact, Explicit
    content: String,
    ttl_turns: u32,           // decrement each turn, remove at 0
    priority: u8,             // for budget-constrained truncation
}

impl ContextManager {
    /// Called before each LLM request. Assembles the system prompt.
    fn build_system_prompt(&mut self, user_prompt: &str, budget_tokens: usize) -> String {
        self.decay_expired();
        self.inject_from_signals(user_prompt);
        self.assemble_within_budget(budget_tokens)
    }
}
```

The `build_system_prompt` is called once per turn, runs in <1ms, and produces a system prompt tailored to what the agent is actually doing.

### Structured intent tracking — the IntentDocument



### Why this is a first-class concept, not just the scratchpad

The session scratchpad (`remember`) is agent-directed: the agent explicitly writes notes. The IntentDocument is loop-directed: the agent loop *automatically* maintains it from observable actions. The agent can also write to it explicitly, but most of its content comes from the loop watching what happens.

### The IntentDocument schema

```rust
struct IntentDocument {
    // Updated automatically by the loop from user messages
    current_task: Option<String>,
    
    // Updated automatically from tool calls
    files_read: IndexSet<PathBuf>,
    files_modified: IndexSet<PathBuf>,
    tools_used: Vec<(String, u32)>,  // (tool_name, call_count)
    
    // Updated automatically from change tool validation results
    validation_state: ValidationState,  // Clean, HasErrors(Vec<Error>), Unknown
    
    // Updated explicitly by the agent (via a lightweight tool or structured output)
    approach: Option<String>,
    constraints_discovered: Vec<String>,
    failed_approaches: Vec<FailedApproach>,
    open_questions: Vec<String>,
    
    // Updated automatically by the loop
    session_stats: SessionStats,  // turns, tool calls, tokens used, duration
}

struct FailedApproach {
    description: String,
    reason: String,
    turn_number: u32,
}

struct SessionStats {
    turns: u32,
    tool_calls: u32,
    tokens_consumed: u64,
    context_utilization_percent: f32,
    session_duration: Duration,
    compactions: u32,
}
```

### How it gets populated

**Automatic (loop observes tool calls):**
- Agent calls `read(file)` → `files_read.insert(file)`
- Agent calls `change(edits)` → `files_modified.insert(files)`, update `validation_state`
- Agent calls `execute(cmd)` with non-zero exit → could indicate a problem, log it
- User sends a message → update `current_task` heuristically (first user message, or after compaction)

**Explicit (agent declares):**
The agent can update the intent document through structured content in its responses. Not a separate tool call — a convention in the response format:

```
<intent>
approach: Refactoring the auth flow to use token rotation
constraint: The OAuth refresh token has a 30-minute TTL
failed: Direct token replacement doesn't work because the cache holds stale references
</intent>
```

The agent loop parses these structured blocks from the assistant's response and updates the IntentDocument. This is zero-cost to the agent — it's just text in the response that the loop intercepts.

Alternatively, a lightweight tool: `intent_update(approach: "...", constraint: "...", failed: {approach: "...", reason: "..."})`. But the structured response block is lower friction.

### How it survives compaction

When the agent loop compacts the conversation, the IntentDocument is **not summarized — it's preserved verbatim** as a preamble to the compacted context:

```
[Intent — preserved through compaction]
Task: Fix the auth flow token rotation
Approach: Refactoring to use token rotation with cache invalidation
Files modified: src/auth.ts, src/cache.ts, src/auth.test.ts
Files read: src/types.ts, src/oauth/tokens.ts, docs/auth-design.md
Constraints: OAuth refresh token has 30-minute TTL; cache uses WeakRef
Failed approaches:
  - Direct token replacement: failed because cache holds stale WeakRef pointers (turn 12)
  - Event-based invalidation: too complex, would require refactoring the event bus (turn 18)
Validation: 0 errors, 0 warnings
Session: 25 turns, 47 tool calls, ~120k tokens, 18 minutes

[Compaction summary]
... (the lossy summary of the conversation) ...
```

The post-compaction agent sees *exactly* what was tried and why it failed. It won't repeat approach A because the IntentDocument says "A failed because B." This is the single highest-value improvement over the current compaction model.

### Relationship to the session scratchpad

The IntentDocument is the *ambient* layer — mostly automatic, structured, loop-maintained. The scratchpad (`remember`) is the *deliberate* layer — agent-directed, freeform, key-value. They serve different purposes:

- IntentDocument: "what am I doing, what have I tried, what's the state" — operational context
- Scratchpad: "note to self: the auth flow requires tokens from three sources" — working notes

Both survive compaction. Neither is cross-session (that's what memory facts are for). They compose: the IntentDocument tells the post-compaction agent *what happened*, the scratchpad tells it *what I was thinking*.

### Context decay and provider caching — the two-view solution



### The problem

Anthropic's prompt caching works by caching prefixes: if the first N tokens of your request match a previous request, those N tokens are served from cache (90% cheaper). The cache key is the exact byte sequence of the system prompt + message prefix.

Context decay *rewrites* old messages: a tool result that was "full file contents (200 lines)" becomes "Read file X (200 lines)." This changes the byte sequence. The cache key changes. Every turn after a decay event invalidates the cache.

This is a real cost: Anthropic charges 0.25x for cached tokens vs. 1x for uncached. If decay invalidates the cache on every turn, we're paying 4x more for the prefix than we need to.

### The two-view solution

The agent loop maintains two views of the conversation:

**Canonical history:** The full, unmodified conversation as it happened. Tool results at full fidelity. Never modified. This is what's persisted to disk for session save/restore.

**LLM-facing view:** The decayed version sent to the provider. Old tool results replaced with metadata skeletons. Dynamic system prompt. This is constructed fresh for each LLM call by applying decay transforms to the canonical history.

The key insight: **decay only modifies messages that are already outside the cache window.**

Anthropic's cache has a 5-minute TTL. Messages from 5+ minutes ago are already uncached. Decay targets messages older than N turns (where N is configurable, default ~10). In practice, the messages being decayed are already outside the cache window anyway.

The agent loop ensures cache-friendliness by:
1. Keeping the system prompt stable (the ContextManager changes it, but the base prefix stays constant)
2. Never modifying recent messages (decay only touches messages older than the cache window)
3. Using cache breakpoints at stable positions (after system prompt, after tool definitions)

### The practical implementation

```rust
struct ConversationState {
    // The canonical, unmodified history
    canonical: Vec<AgentMessage>,
    
    // Decay metadata: which messages have been decayed and how
    decay_state: HashMap<usize, DecayedContent>,
    
    // The IntentDocument
    intent: IntentDocument,
}

impl ConversationState {
    /// Build the LLM-facing view with decay applied
    fn build_llm_view(&self, decay_window: usize) -> Vec<LlmMessage> {
        self.canonical.iter().enumerate().map(|(i, msg)| {
            let age = self.canonical.len() - i;
            if age > decay_window {
                // Old message — apply decay
                self.decay_message(msg)
            } else {
                // Recent message — full fidelity
                self.convert_to_llm(msg)
            }
        }).collect()
    }
    
    fn decay_message(&self, msg: &AgentMessage) -> LlmMessage {
        match msg {
            AgentMessage::ToolResult { name: "read", path, content, .. } => {
                // Replace file contents with metadata
                LlmMessage::tool_result(format!("Read {} ({} lines)", path, content.lines().count()))
            }
            AgentMessage::ToolResult { name: "execute", output, exit_code, .. } => {
                // Replace output with summary
                LlmMessage::tool_result(format!("Executed command: exit {}, {} lines output", exit_code, output.lines().count()))
            }
            // Assistant messages, user messages: keep as-is (they're small)
            _ => self.convert_to_llm(msg)
        }
    }
}
```

### OpenAI's model

OpenAI's response caching works differently — it caches responses by input hash, and recent models support `store: true` for persistent context. Decay doesn't interact with this the same way. The two-view approach is still correct but the cache optimization is less relevant for OpenAI.

### The bottom line

Context decay and provider caching are compatible because decay targets old messages that are already outside the cache window. The two-view architecture (canonical + LLM-facing) keeps the canonical history intact for session persistence while the LLM-facing view is optimized for each provider's caching model. The ContextManager's dynamic system prompt is the bigger cache concern — and it's solvable by keeping a stable prefix.

## Decisions

### Decision: LLM provider access via reverse-sidecar: Rust hosts, Node.js subprocess bridges to pi-ai

**Status:** exploring
**Rationale:** The 15+ AI provider HTTP clients in pi-ai (25k LoC) are commodity code that changes constantly with upstream API revisions. Porting them to Rust would be mass-producing maintenance debt for zero capability gain. Instead, the Rust agent loop spawns a long-lived Node.js subprocess that imports pi-ai and exposes `streamSimple` over stdin/stdout ndjson. This is the sidecar pattern inverted — Rust is the host, Node is the sidecar. The serialization overhead is negligible relative to LLM latency. pi-ai updates (new providers, model registry, OAuth) are picked up by updating the npm package with zero Rust changes. The bridge is ~100 lines of JS that will never need modification because the streamSimple contract is stable. Rust-native provider clients (reqwest-based Anthropic/OpenAI) can be added later as an optimization for the common case, without removing the bridge for the long tail of providers.

### Decision: The Rust agent loop emits typed events, not TUI calls — rendering is a downstream consumer

**Status:** exploring
**Rationale:** The agent loop must never call TUI APIs directly. It emits a stream of typed events (turn_start, message_chunk, tool_started, tool_completed, etc.) via a tokio broadcast channel. Rendering backends (pi-tui bridge today, Dioxus tomorrow, headless for cleave children) subscribe to this channel and translate events into their own rendering model. This is what makes the TUI transition staged and mechanical — swapping the renderer doesn't touch the agent loop. It also means cleave children run the same agent loop with a null/logging renderer, not a stripped-down version of the loop.

### Decision: The existing Omegon extension TypeScript adapters disappear — Rust crates link directly into the agent loop binary

**Status:** exploring
**Rationale:** Each Omegon extension today is a TypeScript adapter (~200-800 lines) that registers pi tools/commands and renders TUI components, calling into a Rust sidecar for business logic. After the inversion, the sidecar's Rust logic becomes a library crate linked directly into the omegon binary. Tool registration is a Rust trait implementation, not a pi API call. The TypeScript adapter layer, the pi extension API, and the IPC transport between them all disappear. This eliminates the entire class of bugs that comes from serializing/deserializing across the TS↔Rust boundary and removes ~5,000+ lines of adapter code across all extensions.

### Decision: Subprocess bridge for LLM providers, not napi-rs — isolation over performance at this boundary

**Status:** decided
**Rationale:** The LLM call boundary is latency-dominated (50-500ms per streaming chunk from the provider). JSON serialization adds <1ms. napi-rs would save that microsecond at the cost of coupling the Rust binary to libnode, fighting ESM/CJS import semantics, and complicating the build pipeline. The subprocess bridge is ~100 lines of JS that imports pi-ai's streamSimple and relays events as ndjson over stdout. It's spawned once (long-lived), matches the proven sidecar pattern, and completely decouples Rust from Node.js at the process boundary. pi-ai updates (new providers, OAuth, model registry) require zero Rust changes. Rust-native reqwest clients for Anthropic/OpenAI can be added later as a Phase 3 optimization for the common case.

### Decision: Phase 0 (headless Rust agent loop) ships as cleave child executor before replacing the main runtime

**Status:** decided
**Rationale:** The Rust agent loop's first production consumer is cleave children, not the interactive session. Today, cleave children spawn full Omegon instances with all extensions, dashboard, memory extraction — massive overhead for what is fundamentally "call LLM, run tools, write results." The headless Rust binary (omegon-agent) is the ideal child executor: single binary, no TUI, no extensions, fast startup, proper signal handling via RAII. This gives the Rust loop real production traffic from day one without touching the main Omegon runtime. Phase 1 (process inversion for interactive sessions) only happens after Phase 0 is proven in production via cleave.

### Decision: The minimum viable agent is ~800 lines of Rust + ~100 lines of JS bridge, implementing 4 core tools

**Status:** decided
**Rationale:** The MVA needs: the agent loop state machine (~200 lines), the LlmBridge trait + subprocess implementation (~150 lines Rust + ~100 lines JS), 4 core tools — bash (~100 lines), read (~80 lines), write (~40 lines), edit (~60 lines) — a system prompt template (~50 lines), and a CLI entry point (~80 lines). This is sufficient to run useful coding sessions end-to-end. Everything else — compaction, sessions, memory, steering, TUI, extensions — is additive. The MVA is testable in isolation against real LLMs. If the loop + bridge works, every feature layer on top is mechanical: implement tool, register in list, done.

### Decision: The migration is four phases: headless proof → process inversion → TUI migration → native providers

**Status:** decided
**Rationale:** Phase 0: Headless Rust agent loop as standalone binary. First consumer: cleave children. Developed in parallel with existing Omegon, zero disruption. Phase 1: Rust binary becomes the process owner. Node.js demoted to two subprocesses (LLM bridge + TUI bridge). All Omegon feature crates linked directly. This is the one user-visible discontinuity. Phase 2: Dioxus or ratatui replaces the pi-tui bridge subprocess. Node.js only needed for LLM bridge. Phase 3: Rust-native reqwest clients for Anthropic/OpenAI. Node.js subprocess retained for long-tail providers but not spawned in the common case. Each phase is independently valuable and shippable. Phase 0 has immediate value for cleave performance. Phase 3 is optional.

### Decision: Event-driven rendering via broadcast channel — the agent loop never touches TUI APIs

**Status:** decided
**Rationale:** The agent loop emits AgentEvent variants through a tokio::broadcast channel. Renderers subscribe: pi-tui bridge (Phase 0-1), Dioxus terminal (Phase 2+), headless/logging (cleave children), or structured JSON (test harness). The loop's behavior is identical regardless of which renderer is attached or whether any renderer is attached at all. This makes the TUI transition staged and mechanical — swapping the renderer changes zero lines in the loop. It also means cleave children run the exact same agent loop binary with a null renderer, not a stripped-down variant.

### Decision: Omegon features become library crates linked into the agent binary — the extension adapter layer disappears

**Status:** decided
**Rationale:** Each Omegon extension today has a TypeScript adapter (200-800 lines) that registers pi tools/commands and renders TUI components, calling a Rust sidecar for business logic. After inversion, the sidecar's Rust logic becomes a library crate linked directly into the omegon binary. Tool registration is a Rust trait implementation. The pi extension API (registerTool, registerCommand, setFooter) is not needed when Rust is the host. This eliminates ~5,000+ lines of adapter code, removes the entire class of IPC serialization bugs, and makes adding new tools a matter of implementing a Rust trait — not wiring TypeScript glue between two languages.

### Decision: The tool taxonomy is understand/change/execute/remember/speculate — not bash/read/write/edit

**Status:** exploring
**Rationale:** The traditional 4-tool model (bash, read, write, edit) operates at the filesystem level. The agent's reasoning operates at the understanding level. The mismatch forces the agent to spend 5 tool calls (5 LLM round trips, 5x context growth) to answer a question it could state in one sentence. The new taxonomy aligns tools with how the agent actually thinks:

- **understand** — semantic code access via tree-sitter + dependency analysis. Returns relevant context, not raw files. Answers "how does X work" without manual file navigation.
- **change** — atomic multi-file mutations with automatic validation (typecheck, lint, affected tests). Eliminates the "forgot to run tests" and "partial edit broke state" failure modes.
- **execute** — bash, but with structured output parsing and automatic context management (large outputs summarized before entering context).
- **remember** — session-local scratchpad surviving compaction. Key-value store for working notes, distinct from cross-session memory facts.
- **speculate** — checkpoint/rollback via git. "Try this approach; if it fails, undo." Currently requires manual git management.

Plus **observe** — not a tool but ambient state the loop maintains: context budget, validation status, session duration.

read/write/edit/bash still exist as low-level primitives that the higher-level tools compose internally, and as escape hatches when the agent needs direct filesystem access. But 70%+ of the agent's work is better served by the higher-level taxonomy.

### Decision: The agent loop maintains structured intent that survives compaction — not just a flat summary

**Status:** exploring
**Rationale:** The most common post-compaction failure mode is repeating an approach that already failed, because the compaction summary didn't record the failure or its cause. The agent loop should maintain a machine-readable intent document alongside the conversation: current task, current approach, constraints discovered, failed approaches with reasons, files modified/read, open questions. This document is updated automatically from tool calls (file touched → added to files list) and explicitly by the agent (constraint discovered → added to constraints). It survives compaction verbatim — it's not summarized, it's preserved. This gives the post-compaction agent full awareness of what was tried and why, without needing to fit that information into a prose summary.

### Decision: Context injection is dynamic and signal-driven, not a static system prompt dump

**Status:** exploring
**Rationale:** The current system prompt contains instructions for 30+ tools, 12 skills, memory facts, design tree context, and documentation paths — most of which is irrelevant for any given turn. This wastes tokens and dilutes attention. The Rust agent loop's ContextManager injects context dynamically based on deterministic signals: which tools were called recently (inject relevant skill), what file types are being touched (inject language-specific conventions), what the current task involves (inject relevant memory facts), and what the human's prompt references. The base system prompt is minimal — identity, core capabilities, project constraints. Everything else is injected on demand and can be evicted when no longer relevant. This is a material improvement over the current model where Omegon front-loads everything into the system prompt and hopes the LLM pays attention to the right parts.

### Decision: Context decay is continuous, not a compaction event — tool results have lifetimes

**Status:** exploring
**Rationale:** The current compaction model is all-or-nothing: either full conversation or a lossy summary. This wastes context budget on stale tool results (a file read 40 turns ago still consuming tokens) and loses important reasoning when compaction fires. The Rust agent loop implements continuous decay: recent tool results at full fidelity, older results decayed to metadata skeletons ("read file X, 200 lines" instead of the contents), unreferenced results evicted entirely. Decay rate is influenced by whether the LLM referenced the result (mentioned paths, function names, identifiers from the output). This is deterministic — string matching of assistant responses against recent tool results, no LLM call needed. Compaction still exists as the last resort, but it fires less often and preserves more because the context never gets as bloated.

### Decision: The consolidated architecture: Lifecycle Engine + ContextManager + tool taxonomy + 6 feature crates

**Status:** decided
**Rationale:** Reconciling all child explorations into the final architecture:

The Rust agent loop core (~7,000 lines) contains:
1. **Agent loop state machine** — prompt → LLM → tool dispatch → repeat, with steering/follow-up
2. **Lifecycle Engine** — design exploration, specification, decomposition as cognitive modes with ambient capture (omg: tags), phase detection via tool-call inference, autonomous decomposition above threshold
3. **ContextManager** — dynamic per-turn system prompt injection from lifecycle phase, tool activity, file types, memory, and explicit signals
4. **ConversationState** — canonical + LLM-facing two-view history with continuous context decay (active → skeleton → evicted), reference-tracking for decay rate
5. **IntentDocument** — auto-populated session state (task, approach, constraints, failed approaches, files, lifecycle phase) surviving compaction verbatim
6. **Core tools** — understand (tree-sitter), change (atomic + auto-validation), execute (progressive disclosure + format-aware extraction), read/write/edit/bash (low-level primitives), remember (session scratchpad), speculate (git checkpoint/rollback)
7. **Lifecycle store** — sqlite source of truth (.pi/lifecycle.db) with markdown rendering for git
8. **LLM bridge** — Node.js subprocess importing pi-ai, ndjson over stdio

6 feature crates (~6,600 lines): memory, render, view, web-search, local-inference, mcp. Each implements ToolProvider + optionally ContextProvider/EventSubscriber/SessionHook.

Infrastructure (effort, auth, bootstrap, tool-profile, etc.) absorbed into core.
Rendering layer (dashboard, splash, sermon) migrates last in Phase 2.

Supersedes the earlier "4 core tools" MVA decision — the MVA still ships first as the Phase 0 headless binary for cleave children, but it now includes the lifecycle engine and the full tool taxonomy from the start.

## Open Questions

*No open questions.*
