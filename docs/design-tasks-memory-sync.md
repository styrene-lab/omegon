---
id: design-tasks-memory-sync
title: Design Tasks + Memory Sync — Open Questions, tasks.md, and memory integration
status: decided
parent: dual-lifecycle-openspec
open_questions:
  - "tasks.md auto-mirror from Open Questions: is this a live sync (every add_question/remove_question also writes tasks.md) or a one-time generation at exploring time that diverges? Live sync keeps them in sync but adds write overhead to every question mutation."
---

# Design Tasks + Memory Sync — Open Questions, tasks.md, and memory integration

## Overview

> Parent: [Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer](dual-lifecycle-openspec.md)
> Spawned from: "tasks.md auto-mirror from Open Questions: is this a live sync (every add_question/remove_question also writes tasks.md) or a one-time generation at exploring time that diverges? Live sync keeps them in sync but adds write overhead to every question mutation."

*To be explored.*

## Research

### Options for tasks.md sync and memory integration



## Decisions

### Decision: Option D — Live sync for presence, memory fact required for closure

**Status:** decided
**Rationale:** Live sync keeps tasks.md honest without manual overhead. The completion gate (decision added OR memory fact connected as "answers") prevents the current failure mode where questions are dismissed without producing any artifact. Memory integration is confirmatory, not autonomous — semantic matching is useful for surfacing candidates but not for auto-closing questions. Option C's full semantic matching is a future enhancement, not the gating mechanism.

### Decision: Open Questions emitted as memory facts for cross-session and cross-node research surfacing

**Status:** decided
**Rationale:** Emitting each open question as a project-memory fact in section "Specs" makes questions searchable via memory_recall across sessions and nodes. An agent storing a finding via memory_store can surface "this may answer an open question on node X" without full semantic matching — it's a retrieval hint, not an autonomous action. Low implementation complexity relative to the value: research is durable across compactions, questions are discoverable cross-project, the memory system becomes the research layer for design work.

## Open Questions

- tasks.md auto-mirror from Open Questions: is this a live sync (every add_question/remove_question also writes tasks.md) or a one-time generation at exploring time that diverges? Live sync keeps them in sync but adds write overhead to every question mutation.

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
