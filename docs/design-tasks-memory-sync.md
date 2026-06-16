+++
id = "426fc68b-0b33-47e1-ad10-2d711a0e5d49"
kind = "document"
title = "Design Tasks + Memory Sync — Open Questions, tasks.md, and memory integration"
status = "implemented"
tags = []
aliases = ["design-tasks-memory-sync"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "dual-lifecycle-openspec"
+++

# Design Tasks + Memory Sync — Open Questions, tasks.md, and memory integration

## Overview

> Parent: [Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer](dual-lifecycle-openspec.md)
> Spawned from: "tasks.md auto-mirror from Open Questions: is this a live sync (every add_question/remove_question also writes tasks.md) or a one-time generation at exploring time that diverges? Live sync keeps them in sync but adds write overhead to every question mutation."

*To be explored.*

## Research

### Options for tasks.md sync and memory integration



## Decisions

### Decision: Memory sync workflows are context-gated, not universal startup behavior

**Status:** decided
**Rationale:** Multi-checkout project memory sync is valuable for long-running repo work, but it must not overfit Omegon's own development workflow onto one-off tasks. Lightweight memory recall/store remains generally useful, while Git reconciliation, changelog/design/handoff artifacts, and sibling-checkout federation checks activate only when project/repo signals exist or the operator explicitly asks. Non-Git one-off work should stay task-local: read/edit/report without assuming Git, OpenSpec, design docs, or handoff files.

**Operating modes:**

- **One-off / non-Git:** no Git assumptions; no changelog, design-node, handoff, or federation artifacts unless requested.
- **Ordinary Git repo:** use Git status/fetch only when relevant to the task; do not impose Omegon lifecycle conventions unless project directives require them.
- **Known lifecycle project:** reconcile memory with project artifacts such as design docs, OpenSpec, tasks, and changelog when behavior or decisions change.
- **Multi-checkout / federation:** explicitly compare sibling checkouts and memory backend state only when the operator asks for cross-checkout continuity or the project declares that topology.

### Decision: Option D — Live sync for presence, memory fact required for closure

**Status:** decided
**Rationale:** Live sync keeps tasks.md honest without manual overhead. The completion gate (decision added OR memory fact connected as "answers") prevents the current failure mode where questions are dismissed without producing any artifact. Memory integration is confirmatory, not autonomous — semantic matching is useful for surfacing candidates but not for auto-closing questions. Option C's full semantic matching is a future enhancement, not the gating mechanism.

### Decision: Open Questions emitted as memory facts for cross-session and cross-node research surfacing

**Status:** decided
**Rationale:** Emitting each open question as a project-memory fact in section "Specs" makes questions searchable via memory_recall across sessions and nodes. An agent storing a finding via memory_store can surface "this may answer an open question on node X" without full semantic matching — it's a retrieval hint, not an autonomous action. Low implementation complexity relative to the value: research is durable across compactions, questions are discoverable cross-project, the memory system becomes the research layer for design work.

## Open Questions

*No open questions.*

## The problem being solved

Design OpenSpec changes have a `tasks.md` representing "what work remains before this design is done." For implementation changes, tasks are discrete code deliverables. For design changes, tasks are **questions to be answered through research**. The source of truth for those questions already exists: `## Open Questions` in the design node document.

Two problems:
1. **Sync**: How does tasks.md stay consistent with Open Questions?
2. **Completion**: How does the system know when a question is "answered" — i.e., when a task is done?

---

## Option A — Live sync (dumb mirror)

Every `add_question` / `remove_question` call also rewrites `openspec/design/<id>/tasks.md`. Questions map directly to unchecked tasks; removing a question marks it done or removes it.

**Pros:** Always in sync. tasks.md is always a valid view of current open questions. Simple mechanical mapping.

**Cons:** "Answering" a question isn't the same as "removing" it. You remove a question when it's resolved — but resolution should produce a *decision* or *research entry*, not just a deletion. The current flow allows removing a question with no record of what was decided. Under this model, tasks.md tracks presence/absence of questions, not the quality or completeness of their answers.

---

## Option B — One-time generation, then diverges

tasks.md is generated once at `set_status(exploring)` time from the current Open Questions. After that, it's edited manually or by the agent during assessment, independent of the node document.

**Pros:** Minimal write overhead. No coupling between question mutation and tasks.md.

**Cons:** Drift is guaranteed. Within a session, the agent removes questions from the node but forgets to update tasks.md. The design OpenSpec change looks incomplete even when the node is ready. This is just a different way to recreate the current "vibes-based decided" problem.

---

## Option C — Memory-integrated question lifecycle

This is the interesting one the user intuited.

**Core idea**: Open Questions aren't just text strings — they're semantic queries waiting to be satisfied by memory facts. Each question, when added, is also stored as a project-memory fact with `section: "Specs"` and linked via `memory_connect` to the design node. When research produces a decision or finding that answers the question, the corresponding memory fact is updated, and `remove_question` is triggered automatically (or surfaced as a suggestion).

**The flow:**
```
add_question("Which storage approach scales to 10x?")
  → writes to ## Open Questions (existing behavior)
  → emits memory fact: {section: "Specs", content: "OPEN: Which storage approach scales to 10x? [design:my-node]"}
  → memory_connect(question_fact, node_fact, "open_question_of")

[agent researches, stores findings via memory_store]

memory_store("SQLite with WAL mode scales to ~100K writes/sec under test conditions")
  → lifecycle emitter checks: does this fact semantically satisfy any open question facts?
  → if yes: surfaces "This finding may answer: 'Which storage approach scales to 10x?' — close it?"
  → operator/agent confirms: remove_question() + store decision + mark question_fact answered

tasks.md: live-mirrored from open question facts, not the node document directly
  → task is "done" when the question fact is marked answered, not just removed from the list
```

**What memory adds here:**
- Semantic matching between research findings and open questions (already have embeddings)
- Cross-node question answering: a finding stored for node A might answer a question on node B
- Session continuity: questions persist as memory facts across compactions — you don't lose research context
- Question provenance: you can trace which memory facts answered which questions, building an audit trail of the design reasoning
- The memory system becomes the *research tracking layer* for design work

**Cons:** Complexity. Semantic matching of "does this fact answer this question" is probabilistic — false positives require confirmation, false negatives silently leave questions open. The lifecycle emitter grows significantly. The memory system becomes load-bearing for the design lifecycle.

---

## Option D — Hybrid: live sync for presence + memory for completion

tasks.md live-syncs with question add/remove (Option A mechanics) but question removal requires one of:
1. A corresponding decision was added in the same operation
2. A memory fact is connected to the question as "answers"
3. Explicit override flag (for questions closed by external context)

This gives live sync without the "delete with no record" problem, and uses memory for the completion signal without making the full semantic matching load-bearing.

**Pros:** Disciplined without being fully automated. The agent is guided to answer questions rather than just dismiss them. Memory integration is opt-in per question rather than required for all.

**Cons:** Still adds coupling to question removal. Some friction on legitimate "this question turned out not to matter" closes.

---

## Analysis

Option D is the right default. It closes the "removal with no record" gap (the primary failure mode) while keeping the memory integration lightweight and confirmatory rather than autonomous. The semantic matching in Option C is genuinely compelling for cross-node research surfacing — worth adding as an enhancement, but not as the gating mechanism.

The memory system as research tracking layer is a real architectural insight. Even without full semantic matching as the completion gate, emitting open questions as memory facts makes them searchable across sessions and projects via `memory_recall`. An agent working on a different node that encounters a relevant finding can surface it via memory search. That's additive value with low risk.

---

## 2026-05-21 Update — Recursive tasking and memory supersession

The memory-integrated question lifecycle is one layer of the same recursive tasking system now being unified across Slim plans, IntentDocument work plans, design nodes, OpenSpec tasks, and cleave decomposition. Open questions are tasking items whose completion evidence may be memory facts; execution slices are lower-level tasking items whose durable conclusions may become memory facts.

### Decision: Memory stores durable tasking transitions, not transient checklist state

**Status:** decided
**Rationale:** Memory should capture decisions, supersession rationale, recurring blockers, recovery paths, and resumable suspended-work pointers. It should not store every Slim checklist item or every active step.

### Decision: Superseding a task can require superseding memory facts

**Status:** decided
**Rationale:** If a new tasking slice replaces an old approach, any stored memory fact that asserts the old approach as current must be superseded. The tasking event should link old execution state, new execution state, stale memory fact IDs, and replacement rationale.

### Open Questions

- Should tasking-to-memory updates be emitted by a dedicated `TaskingMemoryBridge`, or folded into existing ambient capture/lifecycle emitters?
- What confidence/approval threshold should be required before semantic matching closes a task/question or supersedes a memory fact automatically?
- How should suspended execution slices be represented in memory: as full facts, compact pointers to lifecycle artifacts, or both?
