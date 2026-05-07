+++
id = "9b230407-a3c4-48b9-ab10-707bad744b11"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust-native agent loop — middle-out replacement of pi's orchestration core — Design Spec (extracted)

> Auto-extracted from docs/rust-agent-loop.md at decide-time.

## Decisions

### LLM provider access via reverse-sidecar: Rust hosts, Node.js subprocess bridges to pi-ai (exploring)

The 15+ AI provider HTTP clients in pi-ai (25k LoC) are commodity code that changes constantly with upstream API revisions. Porting them to Rust would be mass-producing maintenance debt for zero capability gain. Instead, the Rust agent loop spawns a long-lived Node.js subprocess that imports pi-ai and exposes `streamSimple` over stdin/stdout ndjson. This is the sidecar pattern inverted — Rust is the host, Node is the sidecar. The serialization overhead is negligible relative to LLM latency. pi-ai updates (new providers, model registry, OAuth) are picked up by updating the npm package with zero Rust changes. The bridge is ~100 lines of JS that will never need modification because the streamSimple contract is stable. Rust-native provider clients (reqwest-based Anthropic/OpenAI) can be added later as an optimization for the common case, without removing the bridge for the long tail of providers.

### The Rust agent loop emits typed events, not TUI calls — rendering is a downstream consumer (exploring)

The agent loop must never call TUI APIs directly. It emits a stream of typed events (turn_start, message_chunk, tool_started, tool_completed, etc.) via a tokio broadcast channel. Rendering backends (pi-tui bridge today, Dioxus tomorrow, headless for cleave children) subscribe to this channel and translate events into their own rendering model. This is what makes the TUI transition staged and mechanical — swapping the renderer doesn't touch the agent loop. It also means cleave children run the same agent loop with a null/logging renderer, not a stripped-down version of the loop.

### The existing Omegon extension TypeScript adapters disappear — Rust crates link directly into the agent loop binary (exploring)

Each Omegon extension today is a TypeScript adapter (~200-800 lines) that registers pi tools/commands and renders TUI components, calling into a Rust sidecar for business logic. After the inversion, the sidecar's Rust logic becomes a library crate linked directly into the omegon binary. Tool registration is a Rust trait implementation, not a pi API call. The TypeScript adapter layer, the pi extension API, and the IPC transport between them all disappear. This eliminates the entire class of bugs that comes from serializing/deserializing across the TS↔Rust boundary and removes ~5,000+ lines of adapter code across all extensions.

### Subprocess bridge for LLM providers, not napi-rs — isolation over performance at this boundary (decided)

The LLM call boundary is latency-dominated (50-500ms per streaming chunk from the provider). JSON serialization adds <1ms. napi-rs would save that microsecond at the cost of coupling the Rust binary to libnode, fighting ESM/CJS import semantics, and complicating the build pipeline. The subprocess bridge is ~100 lines of JS that imports pi-ai's streamSimple and relays events as ndjson over stdout. It's spawned once (long-lived), matches the proven sidecar pattern, and completely decouples Rust from Node.js at the process boundary. pi-ai updates (new providers, OAuth, model registry) require zero Rust changes. Rust-native reqwest clients for Anthropic/OpenAI can be added later as a Phase 3 optimization for the common case.

### Phase 0 (headless Rust agent loop) ships as cleave child executor before replacing the main runtime (decided)

The Rust agent loop's first production consumer is cleave children, not the interactive session. Today, cleave children spawn full Omegon instances with all extensions, dashboard, memory extraction — massive overhead for what is fundamentally "call LLM, run tools, write results." The headless Rust binary (omegon-agent) is the ideal child executor: single binary, no TUI, no extensions, fast startup, proper signal handling via RAII. This gives the Rust loop real production traffic from day one without touching the main Omegon runtime. Phase 1 (process inversion for interactive sessions) only happens after Phase 0 is proven in production via cleave.

### The minimum viable agent is ~800 lines of Rust + ~100 lines of JS bridge, implementing 4 core tools (decided)

The MVA needs: the agent loop state machine (~200 lines), the LlmBridge trait + subprocess implementation (~150 lines Rust + ~100 lines JS), 4 core tools — bash (~100 lines), read (~80 lines), write (~40 lines), edit (~60 lines) — a system prompt template (~50 lines), and a CLI entry point (~80 lines). This is sufficient to run useful coding sessions end-to-end. Everything else — compaction, sessions, memory, steering, TUI, extensions — is additive. The MVA is testable in isolation against real LLMs. If the loop + bridge works, every feature layer on top is mechanical: implement tool, register in list, done.

### The migration is four phases: headless proof → process inversion → TUI migration → native providers (decided)

Phase 0: Headless Rust agent loop as standalone binary. First consumer: cleave children. Developed in parallel with existing Omegon, zero disruption. Phase 1: Rust binary becomes the process owner. Node.js demoted to two subprocesses (LLM bridge + TUI bridge). All Omegon feature crates linked directly. This is the one user-visible discontinuity. Phase 2: Dioxus or ratatui replaces the pi-tui bridge subprocess. Node.js only needed for LLM bridge. Phase 3: Rust-native reqwest clients for Anthropic/OpenAI. Node.js subprocess retained for long-tail providers but not spawned in the common case. Each phase is independently valuable and shippable. Phase 0 has immediate value for cleave performance. Phase 3 is optional.

### Event-driven rendering via broadcast channel — the agent loop never touches TUI APIs (decided)

The agent loop emits AgentEvent variants through a tokio::broadcast channel. Renderers subscribe: pi-tui bridge (Phase 0-1), Dioxus terminal (Phase 2+), headless/logging (cleave children), or structured JSON (test harness). The loop's behavior is identical regardless of which renderer is attached or whether any renderer is attached at all. This makes the TUI transition staged and mechanical — swapping the renderer changes zero lines in the loop. It also means cleave children run the exact same agent loop binary with a null renderer, not a stripped-down variant.

### Omegon features become library crates linked into the agent binary — the extension adapter layer disappears (decided)

Each Omegon extension today has a TypeScript adapter (200-800 lines) that registers pi tools/commands and renders TUI components, calling a Rust sidecar for business logic. After inversion, the sidecar's Rust logic becomes a library crate linked directly into the omegon binary. Tool registration is a Rust trait implementation. The pi extension API (registerTool, registerCommand, setFooter) is not needed when Rust is the host. This eliminates ~5,000+ lines of adapter code, removes the entire class of IPC serialization bugs, and makes adding new tools a matter of implementing a Rust trait — not wiring TypeScript glue between two languages.

### The tool taxonomy is understand/change/execute/remember/speculate — not bash/read/write/edit (exploring)

The traditional 4-tool model (bash, read, write, edit) operates at the filesystem level. The agent's reasoning operates at the understanding level. The mismatch forces the agent to spend 5 tool calls (5 LLM round trips, 5x context growth) to answer a question it could state in one sentence. The new taxonomy aligns tools with how the agent actually thinks:

- **understand** — semantic code access via tree-sitter + dependency analysis. Returns relevant context, not raw files. Answers "how does X work" without manual file navigation.
- **change** — atomic multi-file mutations with automatic validation (typecheck, lint, affected tests). Eliminates the "forgot to run tests" and "partial edit broke state" failure modes.
- **execute** — bash, but with structured output parsing and automatic context management (large outputs summarized before entering context).
- **remember** — session-local scratchpad surviving compaction. Key-value store for working notes, distinct from cross-session memory facts.
- **speculate** — checkpoint/rollback via git. "Try this approach; if it fails, undo." Currently requires manual git management.

Plus **observe** — not a tool but ambient state the loop maintains: context budget, validation status, session duration.

read/write/edit/bash still exist as low-level primitives that the higher-level tools compose internally, and as escape hatches when the agent needs direct filesystem access. But 70%+ of the agent's work is better served by the higher-level taxonomy.

### The agent loop maintains structured intent that survives compaction — not just a flat summary (exploring)

The most common post-compaction failure mode is repeating an approach that already failed, because the compaction summary didn't record the failure or its cause. The agent loop should maintain a machine-readable intent document alongside the conversation: current task, current approach, constraints discovered, failed approaches with reasons, files modified/read, open questions. This document is updated automatically from tool calls (file touched → added to files list) and explicitly by the agent (constraint discovered → added to constraints). It survives compaction verbatim — it's not summarized, it's preserved. This gives the post-compaction agent full awareness of what was tried and why, without needing to fit that information into a prose summary.

### Context injection is dynamic and signal-driven, not a static system prompt dump (exploring)

The current system prompt contains instructions for 30+ tools, 12 skills, memory facts, design tree context, and documentation paths — most of which is irrelevant for any given turn. This wastes tokens and dilutes attention. The Rust agent loop's ContextManager injects context dynamically based on deterministic signals: which tools were called recently (inject relevant skill), what file types are being touched (inject language-specific conventions), what the current task involves (inject relevant memory facts), and what the human's prompt references. The base system prompt is minimal — identity, core capabilities, project constraints. Everything else is injected on demand and can be evicted when no longer relevant. This is a material improvement over the current model where Omegon front-loads everything into the system prompt and hopes the LLM pays attention to the right parts.

### Context decay is continuous, not a compaction event — tool results have lifetimes (exploring)

The current compaction model is all-or-nothing: either full conversation or a lossy summary. This wastes context budget on stale tool results (a file read 40 turns ago still consuming tokens) and loses important reasoning when compaction fires. The Rust agent loop implements continuous decay: recent tool results at full fidelity, older results decayed to metadata skeletons ("read file X, 200 lines" instead of the contents), unreferenced results evicted entirely. Decay rate is influenced by whether the LLM referenced the result (mentioned paths, function names, identifiers from the output). This is deterministic — string matching of assistant responses against recent tool results, no LLM call needed. Compaction still exists as the last resort, but it fires less often and preserves more because the context never gets as bloated.

### The consolidated architecture: Lifecycle Engine + ContextManager + tool taxonomy + 6 feature crates (decided)

Reconciling all child explorations into the final architecture:

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

## Research Summary

### Anatomy of pi's agent loop — what we're actually replacing

The agent loop lives in `vendor/pi-mono/packages/agent/src/` — 1,604 lines across 3 files. It is surprisingly clean and small:

**agent-loop.ts (682 lines)** — the core state machine:
- `runLoop()` — outer loop: prompt → LLM call → tool dispatch → repeat until stop
- `streamAssistantResponse()` — calls LLM via `streamSimple()`, emits streaming events
- `executeToolCalls()` — sequential or parallel tool dispatch with `beforeToolCall`/`afterToolCall` hooks
- Steering messages — mid-run user interr…

### The LLM streaming bridge — the one hard FFI problem

The entire pi-ai provider surface (25k LoC, 15+ providers) reduces to one function signature from the agent loop's perspective:

```typescript
streamSimple(model: Model, context: Context, options: SimpleStreamOptions): AssistantMessageEventStream
```

Where `AssistantMessageEventStream` yields events: `start`, `text_start`, `text_delta`, `text_end`, `thinking_start`, `thinking_delta`, `thinking_end`, `toolcall_start`, `toolcall_delta`, `toolcall_end`, `done`, `error`.

**Option A: Subprocess bri…

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
    → Rust-native tools (bash, read, write, ed…

### Migration path — incremental inversion, not a single cutover

The question is whether the inversion from "pi hosts Omegon" to "Omegon hosts pi" requires a big bang cutover or can be staged. The answer is staged, with one unavoidable discontinuity.

**Phase 0: Headless proof (parallel development, no disruption)**

Build the Rust agent loop as a standalone binary (`omegon-agent`) that runs headless coding sessions. No TUI, no dashboard, no extensions. Just:

```
omegon-agent --prompt "Fix the bug in foo.rs" --cwd /path/to/repo
```

Internals:
- Rust agent l…

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
    Done { message: Assis…

### What the agent actually experiences — a self-report from inside the harness

This section is written from the perspective of an agent (Opus 4.6) reflecting on what it actually experiences while operating inside Omegon, and what it would want from a successor it designs.

### The tool granularity problem

The 4 core tools (bash, read, write, edit) operate at the filesystem level. My reasoning operates at the *understanding* level. When I need to understand how a module works, I do:

1. `read` the main file (get 200 lines, 150 irrelevant)
2. `bash` grep for a symbol (get 30 matches, need 3)
3. `read` a second file referenced in the first
4. `read` a third file for the type definitions
5. `bash` to check test files exist

That's 5 tool calls — 5 round trips through the LLM — to answer a question I …

### The context window is my only memory, and it's terrible at it

Everything I learn during a session — file contents, command outputs, intermediate conclusions, failed approaches — goes into a flat list of messages. There's no structure, no indexing, no selective retrieval. When I need to remember what I learned about module X forty turns ago, I have to scan through hundreds of messages (or hope the compaction summary preserved it, which it often doesn't because summaries are lossy by nature).

Omegon's memory system (facts, episodes, working memory) is a mas…

### I can't see the consequences of my actions without asking

When I edit a file, I get back "Successfully replaced text in path." I don't know if the file still compiles. I don't know if the tests pass. I don't know if I introduced a type error. I have to explicitly run `bash npx tsc --noEmit` or `bash npm test` — and I often forget, or I defer it to save time, and then the human discovers broken code.

A well-designed agent loop would make observation automatic. Every mutation (edit, write) should trigger relevant validation and include the results in th…

### The system prompt is enormous and mostly noise per-turn

My system prompt contains instructions for 30+ tools, 12 skills, memory conventions, lifecycle processes, the entire project memory, design tree focus context, and pi documentation paths. Most of this is irrelevant for any given turn. When the human says "fix the typo in line 42," I don't need instructions for render_composition_video or the OpenSpec lifecycle.

This isn't just a token cost — it's an attention problem. LLMs have finite attention over their context window. Burying the relevant in…

### Compaction destroys my reasoning chain

When the context gets compacted, I lose the nuance of *why* I made certain decisions. The compaction summary says "edited foo.ts to fix the auth flow." It doesn't say "tried approach A first, which failed because of X, so switched to approach B which works but requires Y." The reasoning chain — the failed approaches, the constraints discovered, the alternatives considered — is gone.

This matters because the most common post-compaction failure mode is: I try the same approach that already failed…

### I have no budget awareness

I don't know how much context I've consumed, how close I am to compaction, how much a tool call will cost in tokens, or how long the session has been running. I make decisions (read a large file, include verbose output) without understanding their resource impact.

The Rust agent loop should maintain ambient state that I can access without a tool call: context usage, token budget remaining, session duration, compaction proximity. Not as tools — as state that influences my behavior automatically …

### What I'd actually design for my successor

Forget the 4-tool model. Here's what I'd want:

**1. Workspace understanding as a primitive.**

Not `read(file)` but `understand(query)`. "Show me how the auth flow works." "What files implement the memory system?" "What changed since the last commit?" The tool uses tree-sitter parsing, dependency analysis, and semantic search to return *the relevant context*, not raw file contents. It understands code structure — functions, types, imports, call graphs — and can answer structural questions direc…

### What this means for the Rust implementation

The "4 core tools" framing was wrong. It was thinking about what a coding agent traditionally has. The right framing is: **what capabilities does the agent loop need to expose to the LLM to make it maximally effective at its actual job?**

The answer is:
- **Understand** (semantic code access)
- **Change** (atomic validated mutations)
- **Execute** (bash, but with structured output and automatic context management)
- **Remember** (session scratchpad)
- **Speculate** (checkpoint/rollback)
- **Obs…

### The output truncation problem — signal vs noise is not knowable in advance

The proposal to "auto-manage bash output before it enters context" has a fundamental problem: the agent loop cannot know in advance which parts of command output are signal. This is not merely hard — it's undefined without knowing the agent's intent and the command's output format.

**Examples of where signal lives in different outputs:**

| Command | Signal location | Why |
|---------|----------------|-----|
| `npm test` | early + summary at end | failures print at the test that fails, summary …

### The real problem: context pollution from tool results

The actual pain point isn't that the output is too long. It's that **all tool results stay in the conversation history forever**, consuming context budget even after they're no longer relevant. When I read a 200-line file on turn 5, those 200 lines are still in context on turn 50, taking up space that could be used for reasoning.

The right fix targets context lifetime, not output truncation:

**1. Progressive disclosure (the honest approach):**
- Full output always stored in a retrievable locat…

### What about learned heuristics?

A long-running heuristics model that learns "for this project, `npm test` output signal is in the failure lines" is interesting but is a Phase 2+ optimization. It requires:
- Tracking what the agent *does* with command output (which lines does it reference in subsequent responses?)
- Building per-project, per-command extraction profiles over sessions
- Persisting these profiles (in memory facts or a dedicated store)

This is feasible but not necessary for the MVA. The combination of progressive …

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
    extrac…

### Context decay — the loop-level fix for tool result pollution

The truncation question exposes the real issue: it's not that individual tool results are too long — it's that **all tool results accumulate in context forever**. A file I read on turn 5 is still consuming context budget on turn 50. The output of a grep I ran twenty minutes ago is still there, taking space from my current reasoning.

The current compaction system is a blunt instrument: when context gets too full, summarize everything, throw away the originals. This is all-or-nothing — either you…

### The understand tool — tree-sitter + scope graph as a codebase index



### What "understand" replaces

Today, when I need to understand how a module works, I perform this sequence:
1. `bash find . -name '*.ts' | head` — orient in the file tree
2. `read src/module/index.ts` — read the entry point (200 lines, 150 irrelevant)
3. `bash grep -rn 'functionName' src/` — find where a symbol is used (30 matches, need 3)
4. `read src/module/types.ts` — read the type definitions
5. `read src/other/consumer.ts` — read a consumer to understand the contract

Five tool calls, five LLM round trips, ~500 lines of…

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
  MemoryInje…

### How this works under the hood

**The index:** On session start (or first `understand` call), the agent loop builds a codebase index using tree-sitter:
- Parse all source files into ASTs (tree-sitter supports 100+ languages)
- Extract symbol definitions: functions, classes, types, interfaces, constants
- Extract imports/exports: which files depend on which
- Build a scope graph: which symbols are visible where, what references what
- Store in an in-memory data structure (refreshed incrementally on file changes)

This is ~2-5 s…

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

**Dependencies:** tree-sitter (…

### The escape hatch

`read` still exists for when the agent needs a specific file at a specific line range. `understand` is for "tell me about X" — structural questions. `read` is for "show me exactly lines 42-80 of foo.ts" — precise access. They compose: `understand` gives the map, `read` gives the territory when the map isn't enough.

### The change tool — atomic edits with automatic validation



### The failure modes this eliminates

**Partial edit state:** Today I edit file A, then edit file B. If the edit to B fails (text not found, ambiguous match), file A is already modified. The codebase is in a half-changed state. The human gets a broken build until I notice and fix B.

**Forgot to validate:** I edit a TypeScript file and say "done." The human runs `tsc` and finds 3 type errors. I should have checked, but the tool didn't remind me and I had 15 other things in flight. This happens constantly.

**Validation is expensive …

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
3. If `validate` is not "none", run the vali…

### The validation pipeline — project-aware, not hardcoded

The validation pipeline is discovered at session start, not hardcoded per language:

1. **Language detection:** tree-sitter grammars loaded for the project's languages
2. **Tool discovery:** scan for `tsconfig.json` (→ tsc), `pyproject.toml` (→ mypy/ruff), `Cargo.toml` (→ cargo check), `.eslintrc` (→ eslint), etc.
3. **Test runner discovery:** scan for `vitest.config`, `jest.config`, `pytest.ini`, `Cargo.toml [test]`, etc.
4. **Affected test resolution:** for "full" validation, use the import gr…

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

The default is `standard`. The agent can override per call. The agent loop can also *su…

### Relationship to speculate

`change` with validation is synchronous — apply, validate, respond. `speculate` is asynchronous — checkpoint, make multiple changes, evaluate holistically, then commit or rollback. They compose:

```
speculate_start("refactor-auth")
  change({edits: [...], validate: "quick"})   // fast feedback during exploration
  change({edits: [...], validate: "quick"})
  change({edits: [...], validate: "quick"})
speculate_check()  // full validation of the complete refactor
speculate_commit() // or speculate…

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

That's 9,000-17,000 tokens of system prompt. On a …

### The ContextManager: deterministic, per-turn, no LLM

The Rust agent loop's ContextManager maintains a dynamic system prompt. It starts minimal and injects context based on signals:

**Base prompt (always present, ~500 tokens):**
- Agent identity and core behavior
- Currently available tool *names* (not full descriptions — descriptions injected on demand)
- Project-level constraints (AGENTS.md)
- Current working directory and date

**Signal-driven injection layers (added per-turn):**

| Signal | What gets injected | How detected |
|--------|-------…

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
    recent_files: VecDeque<PathBuf>,   // last N f…

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
    
    // Updated explicitly by t…

### How it gets populated

**Automatic (loop observes tool calls):**
- Agent calls `read(file)` → `files_read.insert(file)`
- Agent calls `change(edits)` → `files_modified.insert(files)`, update `validation_state`
- Agent calls `execute(cmd)` with non-zero exit → could indicate a problem, log it
- User sends a message → update `current_task` heuristically (first user message, or after compaction)

**Explicit (agent declares):**
The agent can update the intent document through structured content in its responses. Not a sep…

### How it survives compaction

When the agent loop compacts the conversation, the IntentDocument is **not summarized — it's preserved verbatim** as a preamble to the compacted context:

```
[Intent — preserved through compaction]
Task: Fix the auth flow token rotation
Approach: Refactoring to use token rotation with cache invalidation
Files modified: src/auth.ts, src/cache.ts, src/auth.test.ts
Files read: src/types.ts, src/oauth/tokens.ts, docs/auth-design.md
Constraints: OAuth refresh token has 30-minute TTL; cache uses Weak…

### Relationship to the session scratchpad

The IntentDocument is the *ambient* layer — mostly automatic, structured, loop-maintained. The scratchpad (`remember`) is the *deliberate* layer — agent-directed, freeform, key-value. They serve different purposes:

- IntentDocument: "what am I doing, what have I tried, what's the state" — operational context
- Scratchpad: "note to self: the auth flow requires tokens from three sources" — working notes

Both survive compaction. Neither is cross-session (that's what memory facts are for). They co…

### Context decay and provider caching — the two-view solution



### The problem

Anthropic's prompt caching works by caching prefixes: if the first N tokens of your request match a previous request, those N tokens are served from cache (90% cheaper). The cache key is the exact byte sequence of the system prompt + message prefix.

Context decay *rewrites* old messages: a tool result that was "full file contents (200 lines)" becomes "Read file X (200 lines)." This changes the byte sequence. The cache key changes. Every turn after a decay event invalidates the cache.

This is a…

### The two-view solution

The agent loop maintains two views of the conversation:

**Canonical history:** The full, unmodified conversation as it happened. Tool results at full fidelity. Never modified. This is what's persisted to disk for session save/restore.

**LLM-facing view:** The decayed version sent to the provider. Old tool results replaced with metadata skeletons. Dynamic system prompt. This is constructed fresh for each LLM call by applying decay transforms to the canonical history.

The key insight: **decay o…

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
  …

### OpenAI's model

OpenAI's response caching works differently — it caches responses by input hash, and recent models support `store: true` for persistent context. Decay doesn't interact with this the same way. The two-view approach is still correct but the cache optimization is less relevant for OpenAI.

### The bottom line

Context decay and provider caching are compatible because decay targets old messages that are already outside the cache window. The two-view architecture (canonical + LLM-facing) keeps the canonical history intact for session persistence while the LLM-facing view is optimized for each provider's caching model. The ContextManager's dynamic system prompt is the bigger cache concern — and it's solvable by keeping a stable prefix.
