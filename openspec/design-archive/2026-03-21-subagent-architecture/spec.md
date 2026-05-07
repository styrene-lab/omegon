+++
id = "30a91427-92e7-4ba8-9137-188bb2d9d089"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Subagent architecture — map cleave onto the subagent mental model with Omegon-native advantages — Design Spec (extracted)

> Auto-extracted from docs/subagent-architecture.md at decide-time.

## Decisions

### Worktree only for write agents — read-only delegates run in-place (decided)

A worktree for a read-only agent (explorer, reviewer returning text) is a prison bar, not a guardrail. It adds 500ms overhead for zero safety benefit. Write agents MUST get a worktree — the isolation guarantee is the entire point. The delegate tool checks whether the agent's tool list includes write/edit/bash — if yes, worktree. If read-only (read, grep, web_search), run in the main tree. Always err for safety: if in doubt, create the worktree.

### Sync delegates return as tool result (B), async delegates toast + retrieve (C) (decided)

Two modes, two delivery mechanisms. Sync (background=false): parent is waiting, so the result IS the tool response — natural tool call semantics. Async (background=true): parent has moved on, so injecting a system message (A) is disruptive mid-thought. Toast notification on completion + explicit retrieval via delegate_result(task_id) lets the parent fetch when ready. Early exits (child realizes the task is wrong) work identically — child exits, toast fires, result is available. The parent isn't interrupted. delegate_result is a lightweight tool that reads the stored output — no LLM call, just retrieval.

### Field kit model — parent curates what the child needs (model, thinking, prompt, mind, facts) (decided)

The parent knows the task, so it knows the field kit. The delegate tool accepts: model (tier or explicit), thinking_level, scope (file paths), facts (specific fact IDs or a query to pull relevant facts), mind (persona ID for persona-backed agents). The child doesn't get the full 2500-fact project memory — it gets what the parent thinks it needs. This is both cheaper (smaller context) and more focused (relevant facts only). Named agents in .omegon/agents/*.md can declare default field kits (model, tools, mind) that the parent overrides per invocation. The agent .md is the base kit; the delegate call is the mission briefing.

### Naming: Decompose (cleave), Delegate (single child), Hydra (swarm) (decided)

Three modes, three names that convey the relationship: Decompose = split one task into parts (existing cleave). Delegate = hand one task to a specialist (new). Hydra = coordinating team that grows heads as needed (future). delegate is the tool name — clear about the parent→child relationship without implying process-level detail (spawn) or being generic (invoke/task). Hydra captures the multi-headed, regenerative nature of an agent team better than 'swarm' (which implies undirected).

## Research Summary

### The industry pattern — what developers expect from subagents

**Claude Code's model (the de facto standard, March 2026):**

Three tiers of delegation:
1. **Inline subagent** (Task tool) — parent invokes a specialist synchronously, waits for result, continues. Each subagent has: own system prompt, own tool access, own permissions, own context window. The parent's context stays clean — only the result comes back.
2. **Background subagent** (Ctrl+B / `run_in_background: true`) — parent fires off a task asynchronously, gets a task ID, continues working. Can ch…

### What Omegon has today — cleave is a superset, not a subset

**Cleave's current capabilities exceed subagents in several dimensions:**

| Capability | Claude Code subagents | Omegon cleave |
|---|---|---|
| Context isolation | ✅ Fresh context per subagent | ✅ Fresh context per child + git worktree isolation |
| Tool scoping | ✅ Per-agent tool list | ✅ Per-child scope (file paths) |
| Model routing | ✅ Per-agent model | ✅ Per-child model via prefer_local + tier |
| Parallel execution | ✅ Background agents | ✅ Up to 4 parallel children |
| **Git isolation**…

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
  "name": "deleg…

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
- Injection vectors (SQL,…

### How delegate uses cleave infrastructure

Under the hood, `delegate` does exactly what a single-child cleave does:
1. Create git worktree (scope-isolated)
2. Write task file with system prompt from agent .md + user's task description
3. Spawn child process (same omegon binary, headless mode)
4. Monitor via existing dashboard progress tracking
5. When child completes: squash-merge if it made changes, inject result as message

But the UX is: `delegate(task: "review auth.rs for security issues", agent: "security-reviewer")` — one tool call…

### What this gives us over Claude Code

| Property | Claude Code | Omegon delegate |
|---|---|---|
| Context isolation | ✅ Fresh window | ✅ Fresh window + git worktree |
| File safety | ❌ Shared FS, write races | ✅ Worktree isolation, merge on complete |
| Scope enforcement | ❌ Tool list only | ✅ `verify_scope_accessible()` — can't touch files outside scope |
| Review gate | ❌ No review | ✅ Optional adversarial review before merge |
| Result quality | Varies | ✅ Spec-aware: delegate can receive OpenSpec scenarios to verify against |
|…

### Persona vs agent — orthogonal axes, not the same thing

**A persona is who the harness IS. An agent is who the harness INVOKES.**

| | Persona | Agent/Subagent |
|---|---|---|
| Lifecycle | Session-long (activated, stays active) | Task-scoped (invoked, returns result, done) |
| Context | Injects into parent's system prompt | Gets its own fresh context |
| Memory | Has a mind store (persistent facts) | Stateless (or inherits parent's memory) |
| Tools | Modifies parent's tool profile | Has its own tool set |
| Identity | "I am a systems engineer" | "G…
