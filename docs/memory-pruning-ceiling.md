---
id: memory-pruning-ceiling
title: "Memory: Structural Pruning Ceiling — DB-level decay enforcement and section caps"
status: implemented
parent: memory-system-overhaul
open_questions: []
---

# Memory: Structural Pruning Ceiling — DB-level decay enforcement and section caps

## Overview

> Parent: [Memory System Overhaul — Reliable Cross-Session Context Continuity](memory-system-overhaul.md)
> Spawned from: "How do we enforce a structural fact ceiling — DB-level pruning by confidence/age — without losing genuinely durable long-lived facts?"

*To be explored.*

## Decisions

### Decision: Cap effective half-life at 90 days regardless of reinforcement count

**Status:** decided
**Rationale:** Current formula: halfLife = halfLifeDays * reinforcementFactor^(n-1). With reinforcement counts up to 119 and any factor > 1.0 this produces years-long half-lives, making decay functionally inert. Fix: clamp the computed halfLife to a maximum of 90 days. Facts that need to live longer must be explicitly pinned via memory_focus. This gives decay teeth without destroying genuinely durable facts — a fact reinforced every session for 3 months is still around, but it has to keep getting reinforced to stay.

### Decision: Per-section ceiling: when a section exceeds 60 facts, run a targeted LLM archival pass over that section only

**Status:** decided
**Rationale:** A holistic extraction agent can't audit 1273 facts — it only sees the current conversation. A per-section archival pass is tractable: 60 facts fits in context, the agent can reason about what to keep vs archive within that section. Trigger at session_start when section count > 60. Scoped prompt: "here are all 70 Architecture facts, archive the least useful ones to bring the section under 60." Immune: facts in working memory (pinned). This is separate from the confidence-decay mechanism — two independent ceilings.

## Implementation Notes

Test coverage lives in `extensions/project-memory/factstore.test.ts` and `extensions/project-memory/edges.test.ts`. Updated tests assert the new 90-day ceiling semantics:

- `"highly reinforced facts are capped at 90-day half-life (decay ceiling)"` — verifies `computeConfidence(90, 15) ≈ 0.5` and `computeConfidence(30, 15) > 0.75`.
- `"decays slower with more reinforcements (up to the 90-day ceiling)"` — verifies that c5 > c1 still holds, and c10 >= c5 (equal once both hit ceiling is correct).
- `"global RC=5 at 90 days is at half-life (90-day ceiling applies)"` — verifies GLOBAL_DECAY also respects the cap.

Old tests that asserted immortal-fact behavior (`> 0.85 at 180 days`, `> 0.8 at 90 days with 15 reinforcements`) were updated to assert the corrected ceiling semantics. All 1603+ tests pass after the change.

## Acceptance Criteria

### Scenarios

- **Given** a fact has been reinforced 50 times over years, **when** `computeConfidence()` is called after 90 days, **then** the returned confidence is approximately 0.5 (not > 0.9), because the 90-day ceiling caps its effective half-life regardless of reinforcement count.
- **Given** a fact with reinforcement count 1 and half-life 14 days, **when** `computeConfidence()` is called after 14 days, **then** confidence ≈ 0.5 (normal half-life decay, unaffected by ceiling).
- **Given** a memory section has 70 active facts, **when** a new session starts, **then** `runSectionPruningPass()` is invoked for that section with target=60 and the returned fact IDs are archived, bringing the section to ≤ 60 facts.
- **Given** `runSectionPruningPass()` returns an ID that does not belong to the specified section, **when** the session-start handler processes results, **then** that ID is silently ignored (safety guard prevents cross-section or phantom archival).
- **Given** the pruning model fails or returns malformed JSON, **when** the pass runs, **then** no facts are archived and the session proceeds normally (best-effort, non-blocking).
- **Given** the `Recent Work` section has > 60 facts, **when** session start runs, **then** no LLM pruning pass is triggered for that section (fast decay handles it structurally).

### Falsifiability

- If a fact reinforced 119 times still shows confidence > 0.9 after 120 days, the ceiling is not working.
- If any section permanently exceeds 60 facts across multiple sessions without the pruning pass firing, the session-start hook is not wired correctly.
- If a fact outside the target section gets archived during a pruning pass, the ID validation guard is broken.
- If the session startup is blocked or delayed by the pruning pass, the fire-and-forget pattern is not implemented correctly.

### Constraints

- [x] `computeConfidence()` must clamp `halfLife = Math.min(rawHalfLife, 90)` — the cap applies to all decay profiles including GLOBAL_DECAY and RECENT_WORK_DECAY.
- [x] Facts pinned via `memory_focus` are in working memory and remain in context regardless of decay; the ceiling does not force-archive them.
- [x] The pruning pass runs fire-and-forget (no `await` in session_start main path) so it cannot block TUI startup.
- [x] `runSectionPruningPass()` must validate returned IDs against the section's fact set before archiving.
- [x] `Recent Work` section is explicitly excluded from the LLM pruning pass (it uses `RECENT_WORK_DECAY` with 2-day half-life instead).
- [x] Tests must assert the new ceiling semantics, not the old immortal-fact behavior.

## Open Questions

*No open questions.*
