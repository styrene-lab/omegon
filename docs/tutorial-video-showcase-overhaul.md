+++
id = "d52dc400-ea9f-497f-bdd0-fa725099aec0"
kind = "document"
title = "Tutorial video/showcase overhaul — deterministic, recordable onboarding flow"
status = "exploring"
tags = ["tutorial", "onboarding", "demo", "video", "ux", "site"]
aliases = ["tutorial-video-showcase-overhaul"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["What is the primary product for this overhaul: a deterministic showcase flow optimized for recording, a first-run onboarding flow optimized for live operators, or a single system that must serve both without branching into contradictory UX?", "How much of the current-project adaptive path should remain inside `/tutorial` once the showcase flow becomes more structured — full lifecycle mutation, read-only orientation, or an explicitly separate onboarding mode?", "[assumption] A deterministic video-grade tutorial requires stronger control over prompts, timing, and visible outputs than the current AutoPrompt/live-agent flow can guarantee."]
parent = "tutorial-system"
related = []
+++

# Tutorial video/showcase overhaul — deterministic, recordable onboarding flow

## Overview

Redesign the tutorial system so it works as both operator onboarding and a clean recorded showcase for the public site. The flow should be deterministic, visually coherent, structurally sound, and honest about what the agent is doing. This likely means treating the tutorial as a scripted product surface, not just an adaptive helper overlay with legacy compatibility baggage.

## Research

### Current tutorial architecture drift

The current implementation already proves the core problem: tutorial intent is split between an old lesson-runner model and a newer overlay/showcase model. `tutorial-system` was decided around lesson files, `/next`, and sandbox pacing, but the shipped code in `core/crates/omegon/src/tui/tutorial.rs` now uses compiled step arrays with overlay triggers (`Tab`, `Command`, `AnyInput`, `AutoPrompt`) and two operator-facing modes (`/tutorial` current-project, `/tutorial demo` showcase). At the same time, `core/crates/omegon/src/tui/mod.rs` still carries `TutorialState`, `/tutorial lessons`, `/next`, and `/prev`. For a video-grade tutorial, this split is harmful: the product story is no longer singular, and the code still exposes obsolete pacing primitives in operator-facing help.

### Video-grade tutorial requirements

A tutorial meant to double as a recorded site video needs properties the current live overlay does not guarantee strongly enough: deterministic visible sequence, bounded timing, stable outputs, no surprise branching, no dependence on whatever exists in the operator's project, and no ambiguous operator choices mid-recording. The current adaptive `/tutorial` path is good onboarding but bad cinema because tool output, memory findings, design-node creation, and model phrasing vary by repo and provider. The showcase path should therefore be treated as a scripted product surface with a fixed demo project, fixed artifact progression, fixed narration beats, and explicit cut points. The current-project path should remain valuable, but it should stop carrying the burden of being the canonical public demonstration.

### Proposed showcase structure

Proposed deterministic showcase script:

Act 1 — Framing
1. Title card / promise: what Omegon is and what the operator will see
2. Cockpit orientation: conversation, footer console, dashboard surfaces
3. Demo-project context: explain the seeded broken app / lifecycle artifacts

Act 2 — Understanding
4. Read code + memory: agent inspects project and stores durable facts
5. Design decision: agent reads a seeded design node, answers one open question, records the decision

Act 3 — Defining correctness
6. OpenSpec tour: show proposal/spec/tasks for the seeded change
7. Execution plan: explain why the change splits cleanly into parallel work

Act 4 — Execution
8. Trigger cleave: operator executes one explicit command, overlay narrates progress
9. Watch progress: branch/worktree/tool activity and lifecycle state are the visual centerpiece

Act 5 — Verification and result
10. Verify implementation: assessment or explicit validation step confirms what changed
11. Open result/dashboard: show the working output and the live dashboard view

Act 6 — Handoff
12. Summary: what the viewer just saw, plus pointer to onboarding in their own project

This flow is better recording material than the current mixed overlay because it has explicit beats, natural cut points, and a clean story arc: understand → decide → specify → execute → verify. The operator should have at most one meaningful action in the recorded flow (the cleave trigger) so the rest remains deterministic.

### Proposed onboarding structure

Proposed onboarding structure (current-project path):

1. Orientation — explain the TUI surfaces briefly
2. Read this repo — store 2-3 memory facts
3. Design-tree bootstrap — inspect one node or create an architecture-overview node
4. OpenSpec bootstrap — propose one focused improvement and generate initial scenarios
5. Dashboard reveal — auto-open dashboard to show the same state from the browser
6. Handoff — tell the operator what artifacts now exist in their repo and what to do next

Key constraint: onboarding should be useful and bounded. It should avoid expensive or risky fully automated implementation steps by default. In particular, it should not trigger cleave or mutate large parts of the operator's repo during onboarding. Success is not 'spectacle'; success is that the project now contains memory, design, and spec scaffolding the operator actually wanted.

### Implementation surfaces and constraints

Required implementation surfaces for the overhaul:
- `core/crates/omegon/src/tui/tutorial.rs` — re-author step model around first-class showcase/onboarding modes; support explicit recording-grade narration beats and clearer visible state transitions
- `core/crates/omegon/src/tui/mod.rs` — simplify command routing, remove legacy lesson-runner branches, and make tutorial command/help surfaces reflect only the real product
- `core/crates/omegon/src/tui/tests.rs` and `core/crates/omegon/src/tui/tutorial.rs` tests — replace lesson-runner tests with overlay-mode/command-surface tests and determinism constraints
- demo-project backing repo/content — ensure seeded artifacts match the showcase script and remain stable enough for site recording
- public docs/site pages — docs should describe showcase vs onboarding explicitly once command naming is decided

Determinism constraints for showcase implementation:
- no dependence on current cwd
- no reliance on emergent agent phrasing for critical comprehension
- one explicit operator action max in the recorded path
- dashboard open should be automatic and predictable
- success/failure states should have explicit tutorial-owned messaging, not raw tool noise only
- visible pauses/cut points should correspond to step boundaries, not incidental agent timing

### Phased implementation plan

Proposed implementation plan:

Phase 1 — Command/model cleanup
- Make `/tutorial` launch onboarding mode explicitly
- Add `/tutorial showcase` as the canonical deterministic flow
- Keep `/tutorial demo` as a compatibility alias only, with code/comments/help text steering toward `showcase`
- Remove `/next`, `/prev`, and `/tutorial lessons` from command routing, autocomplete/help, and public-facing messages

Phase 2 — Overlay model cleanup
- Remove legacy `TutorialState`/lesson-file runner and progress persistence model
- Rename internal booleans/constructors so modes reflect product names (`onboarding` vs `showcase`) rather than the older `demo`/implicit current-project split
- Make the overlay step model explicitly own tutorial-controlled narration, pauses, and handoff messaging

Phase 3 — Showcase strengthening
- Re-author showcase steps around the 12-beat script
- Ensure the demo project content is stable, seeded, and aligned with the script
- Add explicit tutorial-owned interstitials around long operations (read/design/cleave/verify) so recordings do not depend on incidental agent phrasing to make sense

Phase 4 — Onboarding refinement
- Bound the current-project path so it creates value without over-mutating the operator repo
- End with an explicit 'here is what Omegon created for you' summary

Phase 5 — Test and docs realignment
- Replace legacy lesson-runner tests with command-surface and mode-behavior tests
- Add tests for showcase alias behavior and for removing legacy commands
- Update docs, quickstart, and site references to describe onboarding vs showcase separately

## Decisions

### Split tutorial into two first-class products: showcase and onboarding

**Status:** decided

**Rationale:** One system cannot cleanly optimize both for recorded demonstration and for operator-specific onboarding without compromising both. The showcase flow should be deterministic, demo-project-backed, and visually staged for recording/site video. The onboarding flow should remain adaptive to the operator's real repository and be optimized for usefulness rather than cinematic predictability. Both can share overlay infrastructure, but they should be treated as separate products with different constraints, success criteria, and docs surfaces.

### The public-facing recorded tutorial should be a deterministic showcase flow

**Status:** decided

**Rationale:** The site video/tutorial must be stable enough to record once and re-use across releases within a version line. That requires a fixed demo project, fixed step order, fixed artifact set, and explicit narrative beats. The showcase should never depend on whatever repository the operator happened to open, nor on emergent agent choices for core comprehension.

### The adaptive current-project flow should remain, but as onboarding rather than the canonical showcase

**Status:** decided

**Rationale:** Current-project mode is valuable precisely because it adapts to the operator's repo and leaves behind useful artifacts. That makes it good onboarding, but bad recording material. Reframing it as onboarding removes the pressure to make it deterministic while preserving operator value.

### Legacy lesson-runner commands should be removed from the harness, not just from docs

**Status:** decided

**Rationale:** Keeping `/tutorial lessons`, `/next`, `/prev`, and `TutorialState` alive preserves an obsolete mental model and complicates command/help surfaces. The overlay engine is now the real tutorial system. Retaining legacy commands for compatibility is not worth the conceptual cost if the goal is a structurally sound tutorial product.

### Use `/tutorial` for onboarding and `/tutorial showcase` for the deterministic public flow

**Status:** decided

**Rationale:** `/tutorial` is the natural default for the operator-helpful current-project experience. The public recorded flow needs a distinct name that signals a polished, repeatable presentation rather than an adaptive helper. `showcase` is more accurate than `demo` because it describes the product role: a staged, canonical walkthrough suitable for recording and site embedding. `/tutorial demo` can remain as a temporary alias during migration, but it should stop being the preferred public name.

### Remove legacy lesson-runner lifecycle and replace its tests with mode-oriented overlay tests

**Status:** decided

**Rationale:** Keeping dead tutorial architecture alive because it has tests is backwards. The correct move is to delete the obsolete subsystem and write tests for the product we actually ship: onboarding/showcase mode selection, trigger behavior, deterministic showcase progression, alias compatibility, and absence of legacy commands in routing/help. This reduces conceptual load and keeps the test suite aligned with the real command surface.

## Open Questions

- What is the primary product for this overhaul: a deterministic showcase flow optimized for recording, a first-run onboarding flow optimized for live operators, or a single system that must serve both without branching into contradictory UX?
- How much of the current-project adaptive path should remain inside `/tutorial` once the showcase flow becomes more structured — full lifecycle mutation, read-only orientation, or an explicitly separate onboarding mode?
- [assumption] A deterministic video-grade tutorial requires stronger control over prompts, timing, and visible outputs than the current AutoPrompt/live-agent flow can guarantee.
