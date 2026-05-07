+++
id = "d20bbc23-6da8-4296-ae86-95f5dec3e606"
kind = "design_node"
title = "Lifecycle-native agent loop — design, spec, decomposition as cognitive modes, not external tools"
status = "resolved"
tags = ["rust", "architecture", "lifecycle", "design-tree", "openspec", "cleave", "cognitive-modes", "strategic"]
aliases = ["lifecycle-native-loop"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "epic"
open_questions = []
parent = "rust-agent-loop"
priority = "1"
+++

# Lifecycle-native agent loop — design, spec, decomposition as cognitive modes, not external tools

## Overview

The extension-to-crate migration initially treated design-tree, openspec, and cleave as feature crates that implement ToolProvider — the same relationship as web-search or local-inference, just bigger. This is wrong. These three systems are not features the agent uses — they're the structured thinking that makes Omegon a different kind of agent. Porting them as crates with tool interfaces preserves the fundamental problem: the agent loop doesn't understand what it's doing.

**The problem with the current model:**

Today the agent loop is a dumb pipe: receive prompt → call LLM → dispatch tool → repeat. It has no concept of design, specification, decomposition, or implementation. Those concepts exist only in:
1. The system prompt (directives telling the agent to follow the lifecycle)
2. The agent's own reasoning (hoping the LLM remembers to call design_tree_update)
3. External tool state (markdown files on disk that the agent reads/writes through tools)

This means:
- The agent has to be *told* to explore design questions before implementing
- The agent has to *remember* to create specs before coding
- The agent has to *choose* to assess complexity before starting work
- The agent has to *manually maintain* lifecycle state through tool calls
- When the agent forgets (which it does — constantly), the lifecycle breaks

Porting these as Rust crates with ToolProvider interfaces preserves all of this. The Rust loop is still a dumb pipe — it just dispatches to Rust tools instead of TypeScript tools. The agent still has to remember, choose, and maintain.

**What the Rust-native loop can do instead:**

The agent loop itself understands the lifecycle. Not as external tool state, but as its own cognitive architecture:

- **The loop knows what phase of work it's in.** Not because the agent declared it via a tool call, but because the loop observes: "the human asked a question that requires design exploration" or "the agent is making changes that correspond to an active spec" or "this task is complex enough to decompose."

- **The loop enforces transitions naturally.** Not through system prompt directives ("you should create a spec before implementing"), but through its own state machine: the available tools and context change based on the phase. In design mode, the agent gets research and exploration tools. In implementation mode, it gets change and validation tools with spec scenarios injected.

- **The loop maintains lifecycle state automatically.** When the agent discovers a constraint during implementation, the loop captures it in the design node — the agent doesn't need to remember to call `design_tree_update(action: "add_research")`. When the agent's reasoning reveals an open question, the loop extracts it.

- **Decomposition is a loop capability, not a tool.** The loop can assess complexity, propose decomposition, and fork itself into parallel children — because it IS the process owner. The agent doesn't call `cleave_run()` — the loop recognizes "this task exceeds my single-agent capability" and decomposes.

This is the difference between:
- "The agent uses lifecycle tools" (current: tools are external, agent must remember)
- "The agent loop IS the lifecycle" (proposed: lifecycle is how the loop thinks)

## Research

### What today's lifecycle actually is — and what it pretends to be



### The pretense

The system prompt says things like:
- "Call cleave_assess before starting any multi-system or cross-cutting task"
- "Before implementing any multi-file change, create an OpenSpec change with a proposal and specs"
- "Use design_tree to check the state of design documents before creating or modifying them"
- "When the user says 'let's explore X', use design_tree to find the relevant node"
- "Run /assess design before calling design_tree_update with set_status(decided)"

These are *instructions to an LLM hoping it will follow a process*. They work about 70% of the time. When the LLM is focused, it follows the lifecycle beautifully. When it's deep in implementation, it forgets to assess. When it's excited about a solution, it skips the spec. When it's in a long session after compaction, the directives are gone.

The lifecycle is a set of conventions enforced by system prompt text. It's not architecture — it's prose.

### What the three systems actually do today

**design-tree** is a filing cabinet for markdown files. The agent creates nodes, adds questions, records decisions, changes status — all through explicit tool calls. The design-tree extension passively reads/writes files and renders state on the dashboard. It has no opinion about when design should happen, what questions are open, or whether decisions are well-supported.

**openspec** is a lifecycle tracker for changes. The agent creates proposals, writes specs, generates plans, verifies implementations — all through explicit tool calls. The openspec extension passively manages file state and computes lifecycle stages. It has no opinion about whether specs are sufficient, whether the implementation satisfies them, or when to verify.

**cleave** is a subprocess spawner. The agent decides to decompose, crafts a plan, calls cleave_run — and cleave spawns children, harvests results, and merges. Cleave has *some* intelligence (scope-based model selection, wave planning for dependencies), but the decision to decompose, the plan itself, and the scope assignments are all the agent's responsibility.

### The gap

None of these systems have *initiative*. They wait for the agent to call them. They don't:
- Notice when the conversation has entered a design question
- Suggest that a change needs a spec before implementation
- Recognize that a task is too complex for a single agent
- Enforce that decided designs have supporting research
- Verify that implementations satisfy specs without being asked
- Track which lifecycle phase the session is in

The agent must do all of this — and it must do it through explicit tool calls while also doing the actual cognitive work of design, implementation, and debugging. Every tool call to maintain the lifecycle is a tool call *not* spent on the actual work.

### What "lifecycle-native" means

The agent loop itself understands the lifecycle phases:

```
┌─────────┐     ┌───────────┐     ┌──────────┐     ┌─────────────┐     ┌──────────┐
│ Explore │────▶│  Specify  │────▶│ Decompose│────▶│ Implement   │────▶│  Verify  │
│ (design)│     │ (spec)    │     │ (plan)   │     │ (code+test) │     │ (assess) │
└────┬────┘     └───────────┘     └──────────┘     └─────────────┘     └──────────┘
     │                                                    │
     └──── can loop back to Explore when constraints discovered ────┘
```

The loop tracks which phase the current work is in. Not through a tool call — through observation of what the agent and human are doing. The available capabilities, the injected context, and the loop's own behavior all adapt to the phase.

This is not rigid — the human can always say "just fix this, skip the ceremony" and the loop respects that. But when the work warrants structured thinking, the loop provides it natively instead of hoping the agent remembers.

### The cognitive modes — how the loop's behavior changes per phase



### Mode 1: Explore

**Triggered by:** Human asks a question with genuine unknowns. Agent's reasoning reveals competing options. The conversation contains "should we", "what if", "how would", "pros and cons."

**What the loop does differently:**
- **ContextManager** injects relevant existing design nodes, research, and related decisions. Not because the agent called `design_tree(action: "node")` — because the loop recognized the topic.
- **IntentDocument** tracks the exploration: open questions accumulate automatically from the agent's reasoning (the loop parses question marks in structured output or `<intent>` blocks). Constraints discovered during exploration are captured.
- **Understand tool** is biased toward breadth — "show me the landscape around X" rather than "show me the implementation of X."
- **The loop auto-creates design artifacts.** When the agent articulates a decision with a rationale, the loop captures it as a design decision — the agent doesn't need to separately call `design_tree_update(action: "add_decision")`. When research findings emerge, they're captured.

**What the loop does NOT do:** Force the agent into exploration mode. If the human says "just fix the bug," the loop stays in implementation mode regardless of complexity. The human's intent takes precedence over the loop's assessment.

**How the agent participates:** The agent thinks and reasons as normal. The loop observes structured markers in the response — `<design>`, `<question>`, `<decision>`, `<constraint>` — and captures them into the lifecycle state. The agent can also call explicit tools (`design_update`, `design_query`) for precise control, but the ambient capture handles the common case.

### Mode 2: Specify

**Triggered by:** Exploration converges on a decision. The human says "let's build this." The conversation shifts from "what should we do" to "what exactly needs to happen."

**What the loop does differently:**
- **ContextManager** injects the decided design (decisions, constraints, file scope) and any existing specs for related work.
- **The loop suggests specification.** Not forces — suggests. "This looks like a multi-file change. Want me to write a spec before implementing?" The suggestion is a system-level message, not a tool call.
- **When the agent writes specs** (Given/When/Then scenarios), the loop validates their structure and completeness. Does the spec cover error cases? Are the scenarios falsifiable? The loop can flag gaps.
- **The IntentDocument** shifts to track: what's being specified, which scenarios are written, what constraints have been captured.

**The key change from today:** Currently, creating a spec requires the agent to call `openspec_manage(action: "propose")`, then `openspec_manage(action: "add_spec")`, then `openspec_manage(action: "fast_forward")`. That's 3+ tool calls just to set up the lifecycle scaffolding. In the native loop, the agent writes the spec content (the actual thinking), and the loop handles the scaffolding.

### Mode 3: Decompose

**Triggered by:** A task exceeds single-agent complexity. The agent's own assessment, the loop's scope analysis (file count, system count), or the human's directive.

**What the loop does differently:**
- **The loop proposes decomposition.** It can assess complexity natively (the same logic as `cleave_assess`, but as part of the loop's turn processing). "This task touches 8 files across 3 systems. Decomposing into 3 parallel children."
- **The loop manages children natively.** It IS the process owner. It forks itself into the Phase 0 headless binaries, manages worktrees via gix, harvests results, detects conflicts, and merges — all without the agent needing to construct a plan JSON and call `cleave_run()`.
- **The agent specifies intent, the loop handles execution.** The agent says "split this into auth changes, schema changes, and test updates." The loop creates the worktrees, writes the task prompts, dispatches the children, and reports back.

**The key change from today:** Cleave currently requires the agent to: assess complexity (tool call), construct a plan JSON with labels/descriptions/scopes/dependencies, call cleave_run with the plan, then wait for results. That's a heavy cognitive burden on the agent. The loop should own the mechanical parts (worktree management, dispatch, merge) and let the agent focus on the *intent* (what should the children do, not how to manage them).

### Mode 4: Implement

**Triggered by:** Work shifts from planning to coding. The agent starts reading files and making changes.

**What the loop does differently:**
- **ContextManager** injects the active spec scenarios as acceptance criteria. The agent sees "Given X, When Y, Then Z" alongside the code it's editing — not buried in a tool result from 15 turns ago.
- **The change tool** validates against the spec after each edit set. Not just "does it compile?" but "does the test that corresponds to Scenario 2 pass?"
- **The IntentDocument** tracks implementation progress: which files changed, which scenarios are satisfied, which are untested.
- **The loop knows when implementation is complete** — not because the agent says "done," but because all spec scenarios have passing tests and the validation pipeline is clean.

### Mode 5: Verify

**Triggered by:** Implementation appears complete. All spec scenarios have tests. Validation passes.

**What the loop does differently:**
- **The loop triggers verification automatically.** The equivalent of `/assess spec` runs without the agent needing to remember. The loop compares implementation against spec scenarios, checks for coverage gaps, and reports.
- **If verification fails,** the loop transitions back to Implementation with the failures injected as context. The agent doesn't need to manually read assessment results and decide what to do — the loop puts the failures in front of it.
- **If verification passes,** the loop suggests archival: "All scenarios satisfied. Archive this change?"

### The meta-point

In every mode, the loop's job is to handle the *lifecycle mechanics* (file I/O, state tracking, artifact management, validation triggering) so the agent can focus on the *cognitive work* (design reasoning, spec writing, code implementation, debugging). The agent never needs to "maintain the lifecycle" as a separate activity — the lifecycle emerges from the agent's actual work.

### Ambient capture — the loop observes the agent's reasoning, not just its tool calls



### The key architectural difference

Today, the agent loop sees tool calls and tool results. It doesn't read the agent's text responses — those are rendered for the human and discarded. The loop treats the agent's reasoning as opaque.

The Rust agent loop can change this. It already processes assistant messages (to build the LLM-facing view, to track references for context decay). It can also parse structured markers in the agent's responses to capture lifecycle artifacts.

### How it works

The agent's response contains its reasoning as normal text, plus optional structured blocks:

```
I think the right approach is to refactor the auth flow to use token rotation.
The current direct-replacement approach won't work because the cache holds 
stale WeakRef pointers that don't get invalidated on token refresh.

<decision status="decided">
title: Use token rotation instead of direct replacement
rationale: Direct replacement fails because WeakRef cache pointers 
become stale on token refresh. Rotation creates a new token and 
invalidates the old one atomically.
</decision>

<constraint>
OAuth refresh tokens have a 30-minute TTL — the rotation must 
complete within this window.
</constraint>

Let me look at the cache implementation to understand the invalidation path.
```

The loop parses these blocks and:
- Creates a design decision in the active design node
- Adds a constraint to the IntentDocument
- Updates the design node's status if appropriate

The agent's text response is *already the reasoning*. The structured blocks tell the loop "this part of my reasoning is a lifecycle artifact — capture it." The agent doesn't need to make a separate tool call to record what it just thought.

### What this replaces

Today, the same interaction requires:
1. Agent reasons about the approach (text response)
2. Agent calls `design_tree_update(action: "add_decision", decision_title: "...", rationale: "...", decision_status: "decided")` — a separate tool call that repeats what the agent just said
3. Agent calls `design_tree_update(action: "add_research", heading: "Cache invalidation constraint", content: "...")` — another tool call
4. Agent continues with the actual work

That's 2 tool calls (2 LLM round trips) spent on lifecycle bookkeeping. With ambient capture, it's zero tool calls — the loop captures from the reasoning that was already there.

### The explicit tool calls still exist

Ambient capture handles the common case: the agent articulates something that should be captured, and the loop captures it. But explicit tool calls remain for:
- Querying the design tree ("show me all open questions across nodes")
- Complex mutations ("branch this question into a child node")
- Lifecycle transitions ("mark this node as decided" — though the loop might suggest this automatically)
- Precise control when ambient capture would be wrong

The explicit tools become the exception, not the rule. Most lifecycle maintenance happens through the agent's natural reasoning.

### The structured blocks are optional, not mandatory

If the agent doesn't include structured blocks, nothing breaks. The loop falls back to pure tool-call mode. The blocks are a *low-friction capture mechanism*, not a requirement. This is important because:
- During compacted sessions, the agent might not produce structured blocks
- During simple tasks, lifecycle capture isn't needed
- The human can disable ambient capture for "just fix this" work

### What this means for the system prompt

Instead of 20 lines of directives about when to call `design_tree_update` and `openspec_manage`, the system prompt says:

"When you articulate design decisions, constraints, or research findings, mark them with structured blocks so the loop can capture them into the design tree. This is optional — use explicit tool calls when you need precise control."

That's it. The lifecycle isn't a *process the agent must follow* — it's a *capture mechanism that amplifies the agent's natural reasoning*.

### The risk: over-capture

The loop might capture artifacts the agent didn't intend as permanent. A throwaway observation becomes a "design decision." A temporary constraint becomes a permanent limitation. The mitigation: all ambient captures are provisional — they enter the design tree in a "captured" state, and the agent or human can dismiss, edit, or promote them. Think of it as a draft layer, not a commitment layer.

### What this means architecturally — lifecycle is not a crate, it's the core



### The reclassification

The extension-to-crate migration initially placed design-tree, openspec, and cleave in Category A (feature crates implementing ToolProvider). This is wrong. They belong in the core.

**Before (migration map):**
```
Agent Loop Core
  └── Feature Crates
        ├── omegon-design-tree (ToolProvider)
        ├── omegon-openspec (ToolProvider)
        ├── omegon-cleave (ToolProvider)
        ├── omegon-memory (ToolProvider)
        └── ...
```

**After (lifecycle-native):**
```
Agent Loop Core
  ├── Lifecycle Engine
  │     ├── Design state machine (explore → specify → decompose → implement → verify)
  │     ├── Spec engine (parse, validate, compare against implementation)
  │     ├── Decomposition engine (assess, plan, fork, harvest, merge)
  │     └── Ambient capture (parse structured blocks from agent responses)
  │
  ├── ContextManager (injects lifecycle phase context)
  ├── ConversationState (decay, IntentDocument)
  ├── Core Tools (understand, change, execute, ...)
  │
  └── Feature Crates (ToolProvider)
        ├── omegon-memory
        ├── omegon-render
        ├── omegon-web-search
        └── ...
```

Design, spec, and decomposition move from "feature crates" to "core loop." They're not optional features — they're how the loop thinks.

### What stays as explicit tools

Even with lifecycle in the core, the agent needs explicit access points:

**Design tools (explicit):**
- `design_query(query)` — "show me the open questions across all nodes"
- `design_update(node, mutation)` — "branch this question into a child node"
- `design_focus(node)` — "inject this node's context"

These are for *precise control*. Most design interaction happens through ambient capture.

**Spec tools (explicit):**
- `spec_write(scenarios)` — write Given/When/Then scenarios (the agent writes the content, the loop handles filing)
- `spec_check()` — "does my implementation satisfy the active specs?"

These are simpler than today's openspec_manage with its 8 sub-actions.

**Decomposition tools (explicit):**
- `decompose(intent, children)` — "split this work along these lines"
- `decompose_status()` — "how are the children doing?"

These replace cleave_assess + cleave_run with higher-level intent-driven interfaces.

### The tool count drops dramatically

**Today:** design_tree (2 tools × 16 actions) + openspec_manage (1 tool × 8 actions) + cleave (3 tools) = 27 effective entry points.

**After:** ~6 explicit tools + ambient capture. The lifecycle surface shrinks by 75% because the loop handles the bookkeeping.

### What this means for the ContextManager

The ContextManager already injects context per-turn based on signals. With lifecycle in the core, the ContextManager has a first-class lifecycle phase signal:

```rust
enum LifecyclePhase {
    Exploring { node_id: String, open_questions: Vec<String> },
    Specifying { change: String, scenarios_written: usize },
    Decomposing { plan: Option<DecompositionPlan> },
    Implementing { spec: ActiveSpec, validation_state: ValidationState },
    Verifying { results: AssessmentResults },
    Idle,  // no active structured work
}
```

In `Exploring` phase, the ContextManager injects the focused node's research and decisions. In `Implementing`, it injects the spec scenarios. In `Verifying`, it injects the assessment results. The agent sees the right context for what it's doing without asking for it.

### What this means for the IntentDocument

The IntentDocument gains lifecycle awareness:

```rust
struct IntentDocument {
    // ... existing fields ...
    
    lifecycle_phase: LifecyclePhase,
    active_design_node: Option<String>,
    active_spec_change: Option<String>,
    spec_satisfaction: HashMap<String, ScenarioStatus>,  // scenario → pass/fail/untested
}
```

This survives compaction. The post-compaction agent knows not just "what I was doing" but "what lifecycle phase I was in, which design node I was exploring, which spec scenarios I had satisfied."

### The line between core lifecycle and feature crate

Memory is NOT lifecycle. It's a knowledge management system that supports all phases. It stays as a feature crate.

Render is NOT lifecycle. It's an output capability. It stays as a feature crate.

Web-search is NOT lifecycle. It's an information gathering capability. It stays as a feature crate.

The lifecycle is specifically: design exploration, specification, decomposition, implementation tracking, and verification. These are the phases of *structured work* — the meta-cognition about how to approach a task, not the task execution itself.

### Resolving the four open questions



### Q1: Phase detection — infer aggressively, correct cheaply

Phase detection doesn't need to be perfect because phases are suggestive, not coercive. A wrong classification means slightly wrong context injection, not blocked work. This makes aggressive inference safe.

**Three signal tiers:**

**Tier 1 — Definitive (immediate transition):**
- Tool calls are unambiguous. Calling `change()` is implementation. Calling `understand()` with broad structural queries is exploration. Calling `decompose()` is decomposition. The loop transitions immediately.
- Human commands (`/explore`, `/implement`) are explicit overrides.

**Tier 2 — Strong inference (transition after 2+ signals):**
- User prompt patterns: "what if", "should we", "pros and cons", "how would" → Explore. "Fix", "implement", "build", "add" → Implement. "Split", "parallelize", "too big" → Decompose.
- Conversation shape: multiple questions without mutations = Explore. Multiple mutations without questions = Implement.

**Tier 3 — Self-correcting (wrong inferences resolve naturally):**
- If the loop thinks we're Exploring but the agent calls `change()`, the loop transitions to Implementing. No explicit correction needed.
- If the loop thinks we're Implementing but the agent starts asking design questions, it transitions to Exploring.

**The default phase is Idle** — no structured lifecycle active. The loop only enters a lifecycle phase when signals warrant it. Simple tasks ("fix the typo on line 42") never leave Idle.

**Why this works:** The phases control context injection and ambient capture, not tool availability. If the loop is wrong about the phase, the agent still has access to all tools. The worst case is: design context injected when it wasn't needed (wasted tokens, easily evicted by the ContextManager's TTL decay) or not injected when it was (agent calls explicit tool, loop corrects).

### Q2: Structured block format — namespaced XML tags, skipping fenced code blocks

XML-style tags with an `omg:` namespace prefix. LLMs produce XML naturally — it's heavily represented in training data. The namespace prevents collision with code content.

```
<omg:decision status="decided">
Use token rotation instead of direct replacement.
Direct replacement fails because WeakRef cache pointers become stale.
</omg:decision>

<omg:constraint>
OAuth refresh tokens have a 30-minute TTL.
</omg:constraint>

<omg:question>
How does the cache handle concurrent token refreshes?
</omg:question>

<omg:approach>
Refactoring the auth flow to use atomic token rotation with cache invalidation.
</omg:approach>

<omg:failed reason="cache holds stale WeakRef pointers">
Direct token replacement without cache invalidation.
</omg:failed>
```

**Parsing rules:**
1. Skip everything inside fenced code blocks (triple backticks). This eliminates false positives from code examples.
2. Match `<omg:TAG ...>` ... `</omg:TAG>` with a simple regex or lightweight parser.
3. Tags are only recognized at the start of a line or after whitespace — not inside inline text.

**The full tag vocabulary:**
- `<omg:decision status="exploring|decided|rejected">` — design decision with rationale
- `<omg:constraint>` — discovered limitation
- `<omg:question>` — open question to track
- `<omg:approach>` — current approach (updates IntentDocument)
- `<omg:failed reason="...">` — approach that didn't work (prevents post-compaction repetition)
- `<omg:phase>explore|specify|implement|verify</omg:phase>` — explicit phase declaration

**Why not YAML or a custom format:** YAML requires indentation sensitivity that's fragile in LLM output. Custom formats require training the LLM on something novel. XML tags are the format LLMs already know how to produce reliably.

**Token cost:** A `<omg:decision>` block adds ~10 tokens of markup overhead. The equivalent explicit tool call (design_tree_update with action, decision_title, rationale, decision_status parameters) costs ~40+ tokens of JSON schema parameter formatting plus the tool call overhead. Ambient capture is strictly cheaper.

### Q3: Decomposition — autonomous with threshold, act-and-report

The loop decomposes autonomously when complexity exceeds the threshold. It doesn't ask for permission — it acts and reports what it's doing. The human can interrupt if they disagree.

**The threshold:**
- File count × system breadth, same formula as current cleave_assess (systems × (1 + 0.5 × modifiers))
- Default threshold: 2.0 (same as today)
- Below threshold: execute directly
- Above threshold: decompose, notify human, proceed

**The UX:**
```
[Omegon] This task touches 8 files across 3 systems (complexity: 3.5).
         Decomposing into 3 parallel children:
           1. auth-refactor (src/auth.ts, src/tokens.ts)
           2. schema-update (src/schema.ts, migrations/)
           3. test-coverage (src/auth.test.ts, src/tokens.test.ts)
         Working...
```

The human sees the plan as it executes. They can interrupt (`Ctrl+C` or a steering message) to stop decomposition. But the default is: the loop handles it.

**Learning over time:** The threshold adjusts based on outcomes. If decomposed tasks consistently succeed, the threshold stays. If decomposed tasks frequently produce merge conflicts or the human overrides decomposition, the threshold increases (less aggressive decomposition). This is a simple running average stored in the session config.

**Children are Phase 0 headless binaries.** Each child gets: its task description, its file scope, the relevant spec scenarios (if any), and the understanding index for its scope. The child runs the same agent loop in headless mode. The parent harvests results, detects conflicts, and merges — all within the loop's native process management.

### Q4: Artifact storage — sqlite source of truth, markdown as rendered view

The structured store is sqlite. Design nodes, spec scenarios, lifecycle state, and relationships are rows in tables. Queries are fast, no file scanning needed.

**Markdown is a rendered output, not the source of truth.**

The loop renders design nodes to markdown files in `design/` when artifacts change — for git commits, for human browsing when needed, for export. The rendering is a one-way sync: database → markdown. The markdown files are committed to git so they produce readable diffs.

On session start, if only markdown exists (fresh clone, external edit by a human), the loop imports markdown → database. This handles the bootstrap case and allows humans to create design docs by hand if they want to.

**The schema (simplified):**

```sql
CREATE TABLE design_nodes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL,  -- seed, exploring, resolved, decided, implemented
    parent_id TEXT REFERENCES design_nodes(id),
    overview TEXT,
    priority INTEGER,
    issue_type TEXT,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE design_decisions (
    id INTEGER PRIMARY KEY,
    node_id TEXT REFERENCES design_nodes(id),
    title TEXT NOT NULL,
    status TEXT NOT NULL,  -- exploring, decided, rejected
    rationale TEXT,
    created_at TEXT
);

CREATE TABLE design_questions (
    id INTEGER PRIMARY KEY,
    node_id TEXT REFERENCES design_nodes(id),
    question TEXT NOT NULL,
    resolved BOOLEAN DEFAULT FALSE
);

CREATE TABLE design_research (
    id INTEGER PRIMARY KEY,
    node_id TEXT REFERENCES design_nodes(id),
    heading TEXT NOT NULL,
    content TEXT NOT NULL
);

CREATE TABLE spec_changes (
    id TEXT PRIMARY KEY,
    design_node_id TEXT REFERENCES design_nodes(id),
    title TEXT,
    stage TEXT,  -- proposed, specced, planned, implementing, verifying, archived
    created_at TEXT
);

CREATE TABLE spec_scenarios (
    id INTEGER PRIMARY KEY,
    change_id TEXT REFERENCES spec_changes(id),
    domain TEXT,
    given_text TEXT,
    when_text TEXT,
    then_text TEXT,
    satisfaction TEXT  -- untested, pass, fail
);
```

**Why this is better than markdown-on-disk:**
- Querying "all open questions across all nodes" is a SQL query, not a file scan + YAML parse
- Phase detection can check lifecycle state in microseconds, not milliseconds
- Relationships (node → decisions, change → scenarios) are native, not embedded in markdown frontmatter
- The database is a single file (`.pi/lifecycle.db`), easy to manage

**Why markdown rendering is still valuable:**
- Git diffs are human-readable ("added decision X to node Y" shows as markdown changes)
- External tools (Obsidian, VS Code) can browse the design tree
- The rendered markdown is the "export" format for collaboration with non-Omegon users

**Sync policy:**
- Database is always the source of truth
- Markdown is regenerated on every database mutation (incremental — only changed nodes)
- On startup, if markdown exists but database doesn't (fresh clone), import markdown → database
- On startup, if both exist, database wins (human markdown edits are detected and flagged, not silently overwritten)

## Decisions

### Decision: Design, specification, and decomposition are core loop capabilities — not feature crates

**Status:** exploring
**Rationale:** These three systems define *how the agent thinks about work*, not what the agent can do. Memory, rendering, and web search are features — the agent is useful without them, just less capable. But an agent without structured design, specification, and decomposition is just a bash/read/write/edit loop with a big context window. The lifecycle is the cognitive architecture. It belongs in the core loop, not behind a ToolProvider interface. This means the loop tracks lifecycle phase, injects phase-appropriate context, maintains design/spec artifacts, and manages decomposition — as ambient loop behavior, not as tool calls the agent must remember to make.

### Decision: Ambient capture replaces most explicit lifecycle tool calls — the loop parses structured blocks from the agent's reasoning

**Status:** exploring
**Rationale:** Today the agent must make separate tool calls to record what it just articulated in its reasoning — "I decided X because Y" followed by `design_tree_update(add_decision, title: "X", rationale: "Y")`. This doubles the work. Ambient capture lets the agent mark lifecycle artifacts in its response with structured blocks (`<decision>`, `<constraint>`, `<question>`), and the loop captures them directly. The agent's reasoning IS the artifact — the tool call was just redundant transcription. Explicit tools remain for queries, precise mutations, and complex operations. But the 70% case — recording decisions, constraints, research, and questions as they emerge from reasoning — becomes zero-cost.

### Decision: The lifecycle is suggestive, not coercive — the human can always override phase transitions

**Status:** exploring
**Rationale:** The loop suggests lifecycle transitions ("this looks complex enough to decompose" or "want a spec before implementing?") but never blocks work. The human saying "just fix it" overrides any lifecycle phase. The loop is an amplifier for structured thinking, not a gatekeeper. This is a deliberate contrast with heavyweight process tools that force ceremony on simple tasks. The loop's lifecycle awareness provides value when the work warrants it and stays invisible when it doesn't.

### Decision: Phase detection is aggressive inference from tool calls + prompt keywords, self-correcting on mismatch

**Status:** decided
**Rationale:** Phases are suggestive, not coercive — wrong classification causes slightly wrong context injection, not blocked work. This makes aggressive inference safe. Tool calls are definitive signals (change() = Implement, understand() with broad queries = Explore). Prompt keywords are strong signals after 2+ matches. Misclassifications self-correct: if the loop thinks Explore but the agent calls change(), it transitions to Implement automatically. Default phase is Idle — simple tasks never enter the lifecycle. Human commands (/explore, /implement) are explicit overrides that always take precedence.

### Decision: Ambient capture uses `omg:`-namespaced XML tags, parsed outside fenced code blocks

**Status:** decided
**Rationale:** LLMs produce XML naturally — it's heavily represented in training data. The `omg:` namespace prevents collision with code content. Tags: `<omg:decision>`, `<omg:constraint>`, `<omg:question>`, `<omg:approach>`, `<omg:failed>`, `<omg:phase>`. Parsing skips fenced code blocks (triple backticks) to eliminate false positives. Token cost is ~10 tokens of markup overhead vs ~40+ tokens for the equivalent explicit tool call. YAML was rejected (indentation-sensitive, fragile in LLM output). Custom formats were rejected (LLMs need to be trained on them). The tag vocabulary is intentionally small — 6 tag types cover the lifecycle artifacts.

### Decision: Decomposition is autonomous above threshold — act and report, not propose and wait

**Status:** decided
**Rationale:** The operator wants to stabilize less over time, not more. Above the complexity threshold (default 2.0, same formula as cleave_assess), the loop decomposes autonomously — it notifies the human what it's doing and proceeds. The human can interrupt but doesn't need to approve. Below threshold, execute directly. The threshold adjusts based on outcomes: consistent success keeps it, frequent conflicts or human overrides increase it. Children are Phase 0 headless binaries, not full Omegon instances. This aligns with the operator's explicit goal: the system should require less human intervention as it matures.

### Decision: Lifecycle store is sqlite (source of truth) with markdown rendered as a git-friendly view

**Status:** decided
**Rationale:** The world is run by meatbags who sometimes need to read things. sqlite (`.pi/lifecycle.db`) is the source of truth — fast queries, structured relationships, no file scanning. Markdown files in `design/` and `openspec/` are rendered views, regenerated on database mutations, committed to git for readable diffs. On fresh clone (markdown exists, no database), the loop imports markdown → database. On startup with both, database wins. This gives: microsecond lifecycle queries for the loop, human-readable artifacts when needed, git-friendly diffs for collaboration, and the ability to produce any format from the structured data.

### Decision: Design, specification, and decomposition are core loop capabilities — not feature crates

**Status:** decided
**Rationale:** These three systems define how the agent thinks about work, not what the agent can do. An agent without structured design, specification, and decomposition is just a bash/read/write/edit loop with a big context window. The lifecycle is the cognitive architecture. It belongs in the core loop as the Lifecycle Engine — tracking phase, injecting phase-appropriate context, maintaining artifacts through ambient capture, and managing decomposition natively. This moves ~7,800 LoC of design-tree, openspec, and cleave logic from the feature crate layer into the core loop.

### Decision: Ambient capture replaces most explicit lifecycle tool calls

**Status:** decided
**Rationale:** Today the agent must make separate tool calls to record what it just articulated in reasoning — redundant transcription that costs 2+ LLM round trips per lifecycle artifact. With ambient capture via omg:-namespaced XML tags, the agent marks artifacts inline (decision, constraint, question, approach, failed, phase) and the loop captures them at zero tool-call cost. Explicit tools remain for queries, complex mutations, and precise control. The 70% case becomes zero-cost. All ambient captures enter a provisional state that can be dismissed or promoted.

### Decision: The lifecycle is suggestive, not coercive

**Status:** decided
**Rationale:** The loop suggests lifecycle transitions but never blocks work. The human saying "just fix it" overrides any lifecycle phase. Simple tasks stay in Idle and never enter the lifecycle. This is a deliberate contrast with heavyweight process tools. The loop's lifecycle awareness provides value when work warrants it and stays invisible when it doesn't. Wrong phase inferences cause slightly wrong context injection, not blocked work — and self-correct when the agent's tool calls contradict the inferred phase.

## Open Questions

*No open questions.*
