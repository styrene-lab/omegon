+++
id = "2bb6c6c4-3842-4d12-a48c-e99ffadfea5d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle gate ergonomics — guardrails not brick walls — Design Spec (extracted)

> Auto-extracted from docs/lifecycle-gate-ergonomics.md at decide-time.

## Decisions

### Gate on substance (open questions, decisions), not artifacts (openspec/design/ directory existence) (exploring)

The design spec artifact (openspec/design/{id}/) is a formalization of work that already exists in docs/{id}.md. When the doc has thorough research, decisions recorded, and zero open questions, the substance is there — the artifact is paperwork. Auto-scaffolding the artifact from the doc eliminates busywork while preserving the audit trail.

### Error messages must follow ⚠ what → how pattern with actionable commands (exploring)

Current messages like "scaffold design spec first via set_status(exploring)" are cryptic even to the agent that built the system. Every rejection should say what's blocked, why, and exactly what command to run next. The system should feel like power armor giving tactical guidance, not a bureaucrat stamping DENIED.

### Design spec artifact should be deterministically extracted from the doc, not LLM-generated or stubbed (exploring)

Option C: the doc already has structured sections (Decisions, Acceptance Criteria, Research, Open Questions) that map directly to a spec. A deterministic function extracts and formats them — zero LLM cost, real content in the audit trail, honest about being a snapshot at decide-time. Empty sections are a legitimate signal (you haven't written acceptance criteria yet), not something to paper over with a stub.

### Close the seed→exploring gap: auto-transition on first substance addition, auto-scaffold at gate time as safety net (exploring)

The edge case (substance exists but artifact doesn't) happens because nodes accumulate research/decisions in seed status without transitioning to exploring. Fix at both ends: auto-transition seed→exploring when add_research/add_decision is called (closes the gap at source), AND auto-scaffold at set_status(decided) time if the artifact is still missing (safety net). The gate stays — it just becomes self-healing rather than blocking.

### All four decisions implemented and shipped (decided)

Substance-over-ceremony gates, actionable error messages, deterministic extraction, and seed→exploring auto-transition are all implemented in commit 9196479. Tests updated to reflect new behavior (1722 pass). The gates are now guardrails that guide and auto-scaffold, not brick walls that block and demand manual ceremony.

### Gate on substance (open questions, decisions), not artifacts (openspec/design/ directory existence) (decided)

The design spec artifact (openspec/design/{id}/) is a formalization of work that already exists in docs/{id}.md. When the doc has thorough research, decisions recorded, and zero open questions, the substance is there — the artifact is paperwork. Auto-scaffolding the artifact from the doc eliminates busywork while preserving the audit trail.

### Error messages must follow ⚠ what → how pattern with actionable commands (decided)

Current messages like "scaffold design spec first via set_status(exploring)" are cryptic even to the agent that built the system. Every rejection should say what's blocked, why, and exactly what command to run next.

### Design spec artifact should be deterministically extracted from the doc, not LLM-generated or stubbed (decided)

Option C: the doc already has structured sections that map directly to a spec. A deterministic function extracts and formats them — zero LLM cost, real content in the audit trail, honest about being a snapshot at decide-time.

### Close the seed→exploring gap: auto-transition on first substance addition, auto-scaffold at gate time as safety net (decided)

The edge case happens because nodes accumulate research/decisions in seed status without transitioning to exploring. Fix at both ends: auto-transition seed→exploring when add_research/add_decision is called, AND auto-scaffold at set_status(decided) time if the artifact is still missing.

## Research Summary

### Identified friction points in the current gate system

**Gate 1: `set_status(decided)` requires archived design spec**

Current behavior (design-tree/index.ts:800-828):
- Non-lightweight nodes (feature, epic) must have `openspec/design/{id}/` scaffolded AND archived
- If missing: "Cannot mark X decided: scaffold design spec first via set_status(exploring)"
- If active but not archived: "Cannot mark X decided: run /assess design then archive"

Problems:
- A node with thorough research, decisions, and zero open questions in the doc itself STILL fails …

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
   -…

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
- The agent references t…

### Q1 assessment — auto-scaffold tradeoffs: real spec vs minimal stub

**Option A: Real spec generation from doc content (LLM pass)**

Pros:
- Audit trail has real content — someone reading openspec/design-archive/ sees actual acceptance criteria, not a stub
- Can cross-validate: doc says "5 decisions" but spec might reveal gaps
- The /assess design command has something meaningful to evaluate
- Future agents resuming work can reference the formalized spec

Cons:
- Costs an LLM turn (~10-30s, API cost)
- The generated spec may be lower quality than what the agent w…

### Q2 assessment — the edge case that shouldn't exist

You're right that this is an edge case that shouldn't be possible, and the fix isn't "bypass the gate" — it's "close the gap that allows the edge case."

**The gap**: A node can go from `create` → `exploring` (in conversation, without calling `set_status(exploring)`) → accumulate research/decisions → try `set_status(decided)` — and fail because `set_status(exploring)` was never called, so `scaffoldDesignOpenSpecChange()` never ran.

**This happens because**:
1. `design_tree_update(create)` creat…
