---
id: subagent-architecture
title: Subagent architecture — map cleave onto the subagent mental model with Omegon-native advantages
status: implementing
tags: [architecture, cleave, subagents, delegation, multi-agent, ux, competitive, strategic]
open_questions: []
branches: ["feature/subagent-architecture"]
openspec_change: subagent-architecture
issue_type: epic
priority: 1
---

# Subagent architecture — map cleave onto the subagent mental model with Omegon-native advantages

## Overview

The industry has converged on \"subagents\" as the developer mental model for multi-agent work: a parent agent invokes specialist children for focused tasks. Claude Code, OpenCode, Codex CLI, Spring AI — all use this pattern. Omegon has cleave, which is more powerful (git worktrees, merge policies, adversarial review, scope isolation) but maps poorly to this mental model because it's batch-oriented (plan → split → execute all → merge) rather than on-demand (working → need help → invoke specialist → get result → continue).\n\nThe opportunity: expose cleave's infrastructure through the subagent UX pattern, giving developers the familiar interaction model with Omegon's superior execution guarantees.

## Research

### The industry pattern — what developers expect from subagents

**Claude Code's model (the de facto standard, March 2026):**

Three tiers of delegation:
1. **Inline subagent** (Task tool) — parent invokes a specialist synchronously, waits for result, continues. Each subagent has: own system prompt, own tool access, own permissions, own context window. The parent's context stays clean — only the result comes back.
2. **Background subagent** (Ctrl+B / `run_in_background: true`) — parent fires off a task asynchronously, gets a task ID, continues working. Can check status later, retrieve results when done.
3. **Agent teams** (experimental) — multiple agents coordinate, share findings, communicate. Not fire-and-forget — bidirectional.

**Built-in agents:**
- Explorer (Haiku, read-only, quick analysis)
- Plan (Sonnet, plan mode research)
- General (Sonnet, all tools)

**Custom agents:** `.claude/agents/*.md` files with frontmatter (name, description, model, tools, permissions). Operator creates these to specialize: "UI Expert", "Security Reviewer", "Test Writer".

**Key properties developers expect:**
- **Context isolation** — subagent gets a fresh context, not the parent's 200k token conversation. This is the primary value: deep work without polluting the main session.
- **Tool scoping** — the reviewer agent can't write files, the explorer can't execute bash. Principle of least privilege.
- **Model routing** — use Haiku for quick reads, Sonnet for complex work, Opus for creative/review. Cost-aware delegation.
- **Transparent lifecycle** — the developer can see what subagents are running, their progress, and their results.
- **On-demand invocation** — the parent decides when to delegate, not a planner upfront.

**OpenCode's model:**
- Agents defined in `opencode.json` with model, tools, permissions
- Primary agents cycle via Tab; subagents invoked via Task tool
- `permission.task` controls which agents can invoke which
- Hidden agents: only invocable programmatically, not via Tab
- Max depth: 2 levels recommended

**Codex CLI:**
- CSV fan-out (batch parallel, no mid-batch recovery)
- Per-task isolation in cloud sandboxes
- No interactive subagent invocation

### What Omegon has today — cleave is a superset, not a subset

**Cleave's current capabilities exceed subagents in several dimensions:**

| Capability | Claude Code subagents | Omegon cleave |
|---|---|---|
| Context isolation | ✅ Fresh context per subagent | ✅ Fresh context per child + git worktree isolation |
| Tool scoping | ✅ Per-agent tool list | ✅ Per-child scope (file paths) |
| Model routing | ✅ Per-agent model | ✅ Per-child model via prefer_local + tier |
| Parallel execution | ✅ Background agents | ✅ Up to 4 parallel children |
| **Git isolation** | ❌ Shared filesystem | ✅ Separate git worktrees — children can't conflict |
| **Merge policy** | ❌ File writes are racy | ✅ Squash-merge with conflict detection |
| **Adversarial review** | ❌ Self-review only | ✅ Separate review pass after each child |
| **Scope enforcement** | ❌ Honor system | ✅ `verify_scope_accessible()` — children can only touch their assigned files |
| **OpenSpec binding** | ❌ No spec system | ✅ Children get design.md context + task verification |
| **Lifecycle tracking** | ✅ Task IDs + status | ✅ Dashboard with per-child progress, elapsed time, status |

**But cleave maps poorly to the subagent mental model:**

| Subagent pattern | Cleave equivalent | Gap |
|---|---|---|
| "Invoke explorer to check X" (on-demand, sync) | No equivalent | **Major gap** — cleave requires upfront planning |
| "Run this in background" (async, fire-and-forget) | cleave_run with 1 child | Awkward — too much ceremony for a quick delegation |
| "Custom agent with .md spec" | Persona + plugin.toml | Different UX — personas aren't "agents you invoke" |
| Parent continues while child works | Parent blocked during cleave_run | **Major gap** — parent can't do other work during cleave |
| "Check status of my background tasks" | Dashboard shows cleave progress | OK, but only during active cleave |

**The two fundamental gaps:**
1. **No on-demand invocation** — cleave is batch ("here are 4 tasks, go"). Subagents are interactive ("help me with this one thing now").
2. **Parent blocks** — during cleave_run, the parent session is waiting. Claude Code's parent keeps working while background agents run.

**What we'd need to bridge these:**
- A `delegate` tool that spawns a single child asynchronously and returns immediately
- The parent continues its conversation while the child runs in a worktree
- Results come back as a BusEvent or message injection when the child completes
- The existing cleave infrastructure (worktree creation, merge, review) powers the child — but the UX is "invoke a specialist" not "decompose a plan"

### Proposed architecture — three delegation modes on one infrastructure

**The insight: cleave is the execution engine. Subagents are the UX layer.**

One infrastructure (git worktrees, merge policies, scope isolation, review), three invocation patterns:

### Mode 1: Decompose (existing cleave)

```
/cleave — plan → split → parallel children → merge → review
```
Batch-parallel. The operator or agent defines the full plan upfront. Best for: large implementation tasks, multi-file changes, OpenSpec-driven work. **This stays as-is.**

### Mode 2: Delegate (new — single async subagent)

```
Agent: "I need someone to review the auth module for security issues."
→ delegate tool spawns a child in a worktree
→ parent continues working
→ child completes → result injected as message
→ if child made changes, merge prompt appears
```
On-demand, async. The parent invokes a named or ad-hoc specialist for one focused task. Best for: code review, research, test writing, documentation — tasks where the parent wants a result, not file changes.

**Tool definition:**
```json
{
  "name": "delegate",
  "description": "Spawn a subagent for a focused task. Runs asynchronously — you continue working while it executes.",
  "parameters": {
    "task": "string — what the subagent should do",
    "agent": "string? — named agent (from .omegon/agents/*.md) or omit for general",
    "scope": "string[]? — file paths the subagent can access",
    "model": "string? — model override (e.g. 'haiku' for quick tasks)",
    "background": "boolean — true=async (default), false=wait for result"
  }
}
```

### Mode 3: Swarm (future — agent teams)

```
Multiple persistent agents coordinating via shared context.
Agents can communicate, share findings, hand off work.
```
This is the Omega coordinator tier from the design tree — far future.

### Named agents (`.omegon/agents/*.md`)

Borrow the `.claude/agents/*.md` convention — it's good UX. Each markdown file defines a specialist:

```yaml
---
name: Security Reviewer
description: Reviews code for security vulnerabilities, injection risks, and access control issues.
model: gloriana
tools: [read, bash, web_search]
scope: ["**/*.rs", "**/*.ts"]
---

You are a security-focused code reviewer. Examine the provided code for:
- Input validation gaps
- Path traversal vulnerabilities
- Secret exposure risks
- Injection vectors (SQL, command, template)

Report findings as a structured list with severity, location, and remediation.
```

At startup, scan `.omegon/agents/` and register them. The `delegate` tool accepts an agent name. `list_personas` becomes `list_agents` (or we keep personas separate — personas are who you ARE, agents are who you INVOKE).

### How delegate uses cleave infrastructure

Under the hood, `delegate` does exactly what a single-child cleave does:
1. Create git worktree (scope-isolated)
2. Write task file with system prompt from agent .md + user's task description
3. Spawn child process (same omegon binary, headless mode)
4. Monitor via existing dashboard progress tracking
5. When child completes: squash-merge if it made changes, inject result as message

But the UX is: `delegate(task: "review auth.rs for security issues", agent: "security-reviewer")` — one tool call, one sentence. Not a 5-step cleave plan.

### What this gives us over Claude Code

| Property | Claude Code | Omegon delegate |
|---|---|---|
| Context isolation | ✅ Fresh window | ✅ Fresh window + git worktree |
| File safety | ❌ Shared FS, write races | ✅ Worktree isolation, merge on complete |
| Scope enforcement | ❌ Tool list only | ✅ `verify_scope_accessible()` — can't touch files outside scope |
| Review gate | ❌ No review | ✅ Optional adversarial review before merge |
| Result quality | Varies | ✅ Spec-aware: delegate can receive OpenSpec scenarios to verify against |
| Persistence | Session-only | ✅ Results tracked in design tree + memory |
| Cost control | Model per agent | ✅ Model per agent + context class per agent |

**The pitch to developers: "It's subagents, but they can't break each other's files."**

### Persona vs agent — orthogonal axes, not the same thing

**A persona is who the harness IS. An agent is who the harness INVOKES.**

| | Persona | Agent/Subagent |
|---|---|---|
| Lifecycle | Session-long (activated, stays active) | Task-scoped (invoked, returns result, done) |
| Context | Injects into parent's system prompt | Gets its own fresh context |
| Memory | Has a mind store (persistent facts) | Stateless (or inherits parent's memory) |
| Tools | Modifies parent's tool profile | Has its own tool set |
| Identity | "I am a systems engineer" | "Go ask the security reviewer" |
| Multiplicity | One active at a time | Multiple concurrent |

They compose:
- The parent has persona "Systems Engineer" active
- The parent invokes agent "Security Reviewer" to check a file
- The agent runs with its own system prompt + tools + model
- The agent does NOT have the parent's persona — it has its own instructions
- The result comes back to the parent (who is still the Systems Engineer)

**Or:** the parent invokes agent "Test Writer" which happens to be a persona-based agent — it activates the "Test Expert" persona for the duration of the task, including that persona's mind store. When the agent completes, the persona deactivates.

This means agents CAN be backed by personas (for rich ones with mind stores and skills) or be simple `.md` instruction files (for lightweight specialists). The `.omegon/agents/` directory contains both:

```
.omegon/agents/
├── security-reviewer.md     ← lightweight: just instructions + tool list
├── test-writer.md           ← lightweight
└── pcb-designer/            ← rich: backed by persona plugin
    └── agent.md → persona = "dev.styrene.pcb-designer"
```

The `delegate` tool doesn't need to know the difference — it reads the agent spec, applies the config, and runs the child.

## Decisions

### Decision: Worktree only for write agents — read-only delegates run in-place

**Status:** decided
**Rationale:** A worktree for a read-only agent (explorer, reviewer returning text) is a prison bar, not a guardrail. It adds 500ms overhead for zero safety benefit. Write agents MUST get a worktree — the isolation guarantee is the entire point. The delegate tool checks whether the agent's tool list includes write/edit/bash — if yes, worktree. If read-only (read, grep, web_search), run in the main tree. Always err for safety: if in doubt, create the worktree.

### Decision: Sync delegates return as tool result (B), async delegates toast + retrieve (C)

**Status:** decided
**Rationale:** Two modes, two delivery mechanisms. Sync (background=false): parent is waiting, so the result IS the tool response — natural tool call semantics. Async (background=true): parent has moved on, so injecting a system message (A) is disruptive mid-thought. Toast notification on completion + explicit retrieval via delegate_result(task_id) lets the parent fetch when ready. Early exits (child realizes the task is wrong) work identically — child exits, toast fires, result is available. The parent isn't interrupted. delegate_result is a lightweight tool that reads the stored output — no LLM call, just retrieval.

### Decision: Field kit model — parent curates what the child needs (model, thinking, prompt, mind, facts)

**Status:** decided
**Rationale:** The parent knows the task, so it knows the field kit. The delegate tool accepts: model (tier or explicit), thinking_level, scope (file paths), facts (specific fact IDs or a query to pull relevant facts), mind (persona ID for persona-backed agents). The child doesn't get the full 2500-fact project memory — it gets what the parent thinks it needs. This is both cheaper (smaller context) and more focused (relevant facts only). Named agents in .omegon/agents/*.md can declare default field kits (model, tools, mind) that the parent overrides per invocation. The agent .md is the base kit; the delegate call is the mission briefing.

### Decision: Naming: Decompose (cleave), Delegate (single child), Hydra (swarm)

**Status:** decided
**Rationale:** Three modes, three names that convey the relationship: Decompose = split one task into parts (existing cleave). Delegate = hand one task to a specialist (new). Hydra = coordinating team that grows heads as needed (future). delegate is the tool name — clear about the parent→child relationship without implying process-level detail (spawn) or being generic (invoke/task). Hydra captures the multi-headed, regenerative nature of an agent team better than 'swarm' (which implies undirected).

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/features/delegate.rs` (new) — DelegateFeature: delegate tool (sync+async), delegate_result tool (retrieve async results), delegate_status tool (list active/completed). Agent loader from .omegon/agents/*.md. Field kit assembly (model, thinking, scope, facts, mind). Worktree decision based on tool list write-check.
- `core/crates/omegon/src/delegate/` (new) — Delegate execution engine: agent_loader.rs (parse .omegon/agents/*.md), field_kit.rs (assemble child context from parent memory + agent defaults), runner.rs (spawn child using cleave worktree infra for write agents, in-place for read-only), result_store.rs (store/retrieve async results by task ID)
- `core/crates/omegon/src/features/mod.rs` (modified) — Register delegate module
- `core/crates/omegon/src/setup.rs` (modified) — Register DelegateFeature with access to cleave infra + memory backend
- `core/crates/omegon/src/tui/mod.rs` (modified) — Toast handler for delegate completion events. /delegate status slash command.

### Constraints

- Read-only agents (tool list has no write/edit/bash) run in main tree — no worktree overhead
- Write agents ALWAYS get a worktree — if in doubt, create one (err for safety)
- Sync delegate (background=false) blocks until child completes, returns as tool result
- Async delegate (background=true) returns task_id immediately, toasts on completion
- delegate_result(task_id) retrieves stored output — no LLM call, pure retrieval
- Field kit: parent specifies model/thinking/scope/facts/mind per invocation; agent .md provides defaults
- Agent .md files scanned from .omegon/agents/ at startup, registered for delegate tool tab-completion
- Maximum concurrent async delegates: 4 (same as cleave max_parallel)
