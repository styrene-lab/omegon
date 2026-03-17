---
id: lifecycle-gate-ergonomics
title: Lifecycle gate ergonomics — guardrails not brick walls
status: exploring
parent: directive-branch-lifecycle
tags: [ux, lifecycle, design-tree, openspec, gates, ergonomics]
open_questions: []
---

# Lifecycle gate ergonomics — guardrails not brick walls

## Overview

The lifecycle gates (set_status(decided), implement) were designed to enforce design rigor but in practice create friction that causes the agent to fight the system rather than flow through it. The gates should be power armor — guiding and supporting the operator — not obstacles that require workarounds.

Observed friction from this session:
- Agent said "The gate system is fighting me" when trying to transition a thoroughly-explored node to decided
- Had to set issue_type to feature, set priority, manually scaffold openspec design spec, then delete it and retry
- Error messages like "Scaffold design spec first via set_status(exploring)" are curt and don't suggest the fastest path forward
- The design-spec-before-decided gate duplicates work when the design exploration is already thorough in the doc itself

The gates should differentiate between "you haven't done the work" and "you've done the work but haven't done the paperwork." The former deserves a hard stop. The latter deserves guidance on how to satisfy the gate with minimum ceremony.

## Research

### Identified friction points in the current gate system

**Gate 1: `set_status(decided)` requires archived design spec**

Current behavior (design-tree/index.ts:800-828):
- Non-lightweight nodes (feature, epic) must have `openspec/design/{id}/` scaffolded AND archived
- If missing: "Cannot mark X decided: scaffold design spec first via set_status(exploring)"
- If active but not archived: "Cannot mark X decided: run /assess design then archive"

Problems:
- A node with thorough research, decisions, and zero open questions in the doc itself STILL fails the gate because the design spec artifact doesn't exist
- The design spec is a separate artifact (`openspec/design/{id}/`) that duplicates what's already in `docs/{id}.md`
- The agent has to create the artifact, immediately archive it, then retry — pure busywork

**Gate 2: `implement` requires decided/resolved status AND archived design spec**

Current behavior (design-tree/index.ts:1138-1179):
- Non-lightweight nodes must pass both the status check and the design spec check
- Double-gating: you need decided (which needs design spec), AND implement rechecks design spec

Problems:
- If the node is resolved (all questions answered, decisions made) but the design spec gate wasn't passed, implement fails with a different error
- The implement gate's design spec check is redundant with the decided gate — if decided passed, the design spec is already archived

**Gate 3: Error messages lack actionable guidance**

Examples of messages that tell you what's wrong but not what to do:
- "Scaffold design spec first via set_status(exploring)" — what does that even mean? What should the agent do next?
- "archive the design change first" — which change? What command?
- "not 'decided' or 'resolved'" — but the node has zero open questions and 5 decisions

**What power-armor gates would look like:**

Instead of hard stops, gates should:
1. **Assess readiness** — check if the SUBSTANCE is there (research, decisions, no open questions) not just the ARTIFACTS
2. **Auto-scaffold when possible** — if the design spec is missing but the doc has sufficient content, create it automatically
3. **Suggest the fastest path** — "Run `design_tree_update set_status decided` — I'll scaffold the design spec for you"
4. **Differentiate ceremony from substance** — missing research = hard stop with guidance; missing artifact = auto-create

### Proposed gate behavior changes

**Principle: substance over ceremony. Automate the paperwork, gate on the thinking.**

### set_status(decided) changes:

1. **Substance check** (keep as hard gate):
   - Open questions > 0 → BLOCK: "Resolve N open questions before deciding"
   - Decisions count = 0 → BLOCK: "Record at least one decision before marking decided"

2. **Artifact check** (downgrade from hard gate to auto-scaffold):
   - Design spec missing → AUTO-CREATE from the doc's content, then proceed
   - Design spec active but not archived → AUTO-ARCHIVE, then proceed
   - Both → scaffold AND archive in one pass

3. **Message improvement**:
   - Old: "Cannot mark X decided: scaffold design spec first via set_status(exploring)"
   - New: "Auto-scaffolded design spec from docs/X.md (3 decisions, 0 open questions). Proceeding to decided."
   - If substance check fails: "Cannot mark X decided: 2 open questions remain. Resolve them first, or use `remove_question` if they're no longer relevant."

### implement changes:

1. **Remove redundant design spec check** — if the node is decided, the decided gate already handled it
2. **Allow resolved status** (already does, but the error message implies otherwise)
3. **Auto-transition resolved → decided** when implement is called on a resolved node with sufficient substance

### Error message template:

All gate rejections should follow this pattern:
```
⚠ {what's blocked}: {why}
→ {what to do next}
```

Examples:
- "⚠ Cannot decide: 2 open questions remain\n→ Resolve them with add_decision/remove_question, or branch child nodes for exploration"
- "⚠ Cannot implement: node is 'exploring', not 'decided'\n→ Resolve open questions and run set_status(decided)"

### The two-document problem — docs/ vs openspec/design/

There are TWO places where design information lives, and they serve different purposes:

**`docs/{id}.md`** — The living design document
- Created by `design_tree_update(create)`
- Contains: overview, research sections, decisions, open questions, acceptance criteria, implementation notes
- Frontmatter tracks: status, tags, branches, openspec_change binding, issue_type, priority
- This is where ALL the thinking happens
- The design-tree tools read/write this file directly
- The agent references this during implementation

**`openspec/design/{id}/`** — The design-phase formalization artifact
- Created by `scaffoldDesignOpenSpecChange()` when `set_status(exploring)` fires
- Contains: `proposal.md` (points back to docs/), `spec.md` (placeholder scenarios), `tasks.md` (placeholder)
- Meant to hold acceptance criteria for the DESIGN PHASE itself (not the implementation)
- Must be archived to `openspec/design-archive/` before `set_status(decided)` succeeds

**The friction**: The openspec/design/ artifact often stays a scaffold of placeholders. The real acceptance criteria go into the doc's `## Acceptance Criteria` section. The agent ends up either:
1. Fighting the gate because the artifact doesn't exist (skipped set_status(exploring))
2. Creating the artifact just to immediately archive it
3. Duplicating acceptance criteria between the doc and the artifact

**The root issue**: The gate checks for the ARTIFACT's existence when it should check for the DOC's substance. The artifact was designed for a workflow where design exploration has its own spec-driven lifecycle (`/assess design`), but in practice most design nodes go: create → explore (in conversation) → add decisions → resolve questions → decided. The openspec/design/ ceremony adds friction without adding value unless the operator explicitly wants `/assess design` rigor.

**What the edge case actually is**: The screenshot shows a node (`directive-branch-lifecycle`) that went through extensive exploration — 5 research sections, 5 decided decisions, 0 open questions — all in `docs/directive-branch-lifecycle.md`. But `set_status(decided)` failed because `openspec/design/directive-branch-lifecycle/` was never created (the node went from `create` directly to exploration via conversation, skipping `set_status(exploring)`).

The substance was there. The artifact wasn't. The gate checked for the wrong thing.

### Q1 assessment — auto-scaffold tradeoffs: real spec vs minimal stub

**Option A: Real spec generation from doc content (LLM pass)**

Pros:
- Audit trail has real content — someone reading openspec/design-archive/ sees actual acceptance criteria, not a stub
- Can cross-validate: doc says "5 decisions" but spec might reveal gaps
- The /assess design command has something meaningful to evaluate
- Future agents resuming work can reference the formalized spec

Cons:
- Costs an LLM turn (~10-30s, API cost)
- The generated spec may be lower quality than what the agent wrote in the doc itself
- Duplicates information — the doc already has the canonical content
- If the doc is thorough, the spec is redundant; if the doc is thin, the spec will also be thin (garbage in, garbage out)

**Option B: Minimal stub that satisfies the gate**

Pros:
- Zero latency, zero cost
- Unblocks the workflow immediately
- Honest about what it is — doesn't pretend to be a real spec

Cons:
- Audit trail is meaningless — just a pointer back to the doc
- /assess design has nothing to evaluate
- Normalizes "create empty artifact to bypass gate" pattern
- You correctly called this out: ceremony for ceremony's sake

**Option C: Auto-scaffold from doc, no LLM needed (deterministic extraction)**

The doc ALREADY HAS structured sections that map directly to a spec:
- `## Decisions` → spec decisions
- `## Acceptance Criteria` → spec scenarios (if present)
- `## Open Questions` → spec open items
- `## Research` → spec context

A deterministic function could extract these sections from the doc and write them into the openspec/design/ artifact — no LLM needed, sub-millisecond, real content.

Pros:
- Audit trail has real content (extracted from the authoritative doc)
- Zero LLM cost, near-instant
- No duplication — the artifact is a derivative of the doc, not a separate creation
- /assess design can evaluate the extracted content
- Honest — it's clearly a snapshot of the doc at decide-time

Cons:
- If the doc's acceptance criteria section is empty, the extracted spec is also empty (but this is a legitimate signal — "you haven't written acceptance criteria yet")
- Format translation may lose nuance (markdown sections → spec format)

**Recommendation: Option C.** Extract real content deterministically from the doc. The artifact becomes a snapshot at decide-time, not a separate work product. If the doc has thin content, the gate should warn about that (substance check) rather than silently creating a stub.

### Q2 assessment — the edge case that shouldn't exist

You're right that this is an edge case that shouldn't be possible, and the fix isn't "bypass the gate" — it's "close the gap that allows the edge case."

**The gap**: A node can go from `create` → `exploring` (in conversation, without calling `set_status(exploring)`) → accumulate research/decisions → try `set_status(decided)` — and fail because `set_status(exploring)` was never called, so `scaffoldDesignOpenSpecChange()` never ran.

**This happens because**:
1. `design_tree_update(create)` creates the doc with status `seed`
2. The agent starts adding research, decisions, questions — all via tool calls that modify the doc
3. The doc's status may or may not be updated to `exploring` — it's a manual step
4. When the agent tries `set_status(decided)`, the gate fails because the scaffolding that happens on `exploring` transition never fired

**The fix is not "skip the gate for features"** — it's: **ensure the scaffolding happens when it's needed, not just on a specific status transition.**

Three approaches to closing the gap:

**Approach 1: Scaffold on-demand at gate time**
When `set_status(decided)` runs and the design spec is missing, auto-scaffold it RIGHT THEN (using the doc's current content, Option C from Q1). The gate becomes self-healing — it creates what's missing rather than rejecting.

**Approach 2: Scaffold on first research/decision addition**
When `add_research` or `add_decision` is called and the design spec doesn't exist, scaffold it. This catches the "exploring without set_status(exploring)" gap at the point where substance is first added.

**Approach 3: Auto-transition seed → exploring on first mutation**
When any content-adding tool call is made on a `seed` node, auto-transition to `exploring` and trigger the scaffold. This closes the gap at the source.

**Recommendation: Approach 1 (on-demand at gate time) + Approach 3 (auto-transition)**

Approach 1 is the safety net — if something falls through, the gate fixes it. Approach 3 is the proper fix — seed nodes shouldn't accumulate substance without transitioning to exploring. Together they ensure:
- The design doc always exists when substance is present
- The design spec artifact is always available when decided is requested
- No edge case where the gate blocks legitimately explored work

## Decisions

### Decision: Gate on substance (open questions, decisions), not artifacts (openspec/design/ directory existence)

**Status:** exploring
**Rationale:** The design spec artifact (openspec/design/{id}/) is a formalization of work that already exists in docs/{id}.md. When the doc has thorough research, decisions recorded, and zero open questions, the substance is there — the artifact is paperwork. Auto-scaffolding the artifact from the doc eliminates busywork while preserving the audit trail.

### Decision: Error messages must follow ⚠ what → how pattern with actionable commands

**Status:** exploring
**Rationale:** Current messages like "scaffold design spec first via set_status(exploring)" are cryptic even to the agent that built the system. Every rejection should say what's blocked, why, and exactly what command to run next. The system should feel like power armor giving tactical guidance, not a bureaucrat stamping DENIED.

### Decision: Design spec artifact should be deterministically extracted from the doc, not LLM-generated or stubbed

**Status:** exploring
**Rationale:** Option C: the doc already has structured sections (Decisions, Acceptance Criteria, Research, Open Questions) that map directly to a spec. A deterministic function extracts and formats them — zero LLM cost, real content in the audit trail, honest about being a snapshot at decide-time. Empty sections are a legitimate signal (you haven't written acceptance criteria yet), not something to paper over with a stub.

### Decision: Close the seed→exploring gap: auto-transition on first substance addition, auto-scaffold at gate time as safety net

**Status:** exploring
**Rationale:** The edge case (substance exists but artifact doesn't) happens because nodes accumulate research/decisions in seed status without transitioning to exploring. Fix at both ends: auto-transition seed→exploring when add_research/add_decision is called (closes the gap at source), AND auto-scaffold at set_status(decided) time if the artifact is still missing (safety net). The gate stays — it just becomes self-healing rather than blocking.

### Decision: All four decisions implemented and shipped

**Status:** decided
**Rationale:** Substance-over-ceremony gates, actionable error messages, deterministic extraction, and seed→exploring auto-transition are all implemented in commit 9196479. Tests updated to reflect new behavior (1722 pass). The gates are now guardrails that guide and auto-scaffold, not brick walls that block and demand manual ceremony.

## Open Questions

*No open questions.*
