+++
id = "84bc3792-444e-4752-8320-c05f1353ea46"
kind = "document"
title = "Design Assessment Command — /assess design integration"
status = "implemented"
tags = []
aliases = ["design-assess-command"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "dual-lifecycle-openspec"
+++

# Design Assessment Command — /assess design integration

## Overview

> Parent: [Dual-Lifecycle OpenSpec — Design Layer + Implementation Layer](dual-lifecycle-openspec.md)
> Spawned from: "Is '/assess design' a new top-level command, an action on the existing /assess command (alongside cleave/diff/spec), or is the assessment done inline by set_status(decided) — blocking the transition if criteria aren't met?"

*To be explored.*

## Research

### Command integration options and analysis



## Decisions

### Decision: /assess design as explicit subcommand on the existing assess bridge

**Status:** decided
**Rationale:** Explicitness is the requirement. Inline gating on set_status hides evaluation, creates ambiguous failure states, and prevents iteration. A separate top-level command breaks conceptual unity with the existing assess family. action="design" on the /assess bridge slots into the established pattern: /assess spec before archive, /assess design before decided. Agent guidelines read uniformly, implementation reuses the bridge dispatcher, structured AssessmentResult is already the return type.

### Decision: Structural constraints checked before LLM evaluation — fast short-circuit

**Status:** decided
**Rationale:** open_questions.length > 0 or decisions.length === 0 fails immediately without an LLM call. No point spending tokens evaluating scenario prose when the document is structurally incomplete. Structural pass is prerequisite for LLM evaluation. This also gives the agent fast feedback on mechanical gaps before waiting for scenario assessment.

### Decision: Structured per-finding output: {type, index, pass, finding} per artifact

**Status:** decided
**Rationale:** LLM returns structured findings per acceptance criterion artifact, not prose. Each finding is addressable: the agent knows exactly which scenario failed, which falsifiability condition is unaddressed, which constraint is unsatisfied. This enables iteration — fix the specific gap, re-run /assess design, check that finding only. Mirrors how /assess spec returns per-scenario results rather than a pass/fail summary.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/assessment.ts` (modified) — Add 'design' to AssessmentKind union. Add runDesignAssessment() — resolves node, runs structural checks, runs LLM evaluation of Scenarios/Falsifiability/Constraints.
- `extensions/cleave/bridge.ts` (modified) — Add 'design' case to assess bridge dispatcher. Wire /assess design to runDesignAssessment().
- `extensions/cleave/index.ts` (modified) — Update /assess command description and promptGuidelines to include 'design' subcommand alongside cleave/diff/spec.

### Constraints

- Target node resolved from: explicit node_id arg → focused node → error. No implicit 'current' node without focus.
- Assessment result stored as assessment.json in openspec/design/<node-id>/ alongside other design OpenSpec artifacts
- Pass result surfaces: 'Ready to call set_status(decided) and run /opsx:archive on the design change'
- Fail result surfaces: specific findings per artifact type with actionable fix guidance, not a generic failure message

## Option A — Inline gate on set_status(decided)

The transition itself runs the assessment. Agent calls `set_status(decided)`, assessment fires, blocks if criteria aren't met.

Rejected. Hides a potentially slow LLM call inside what looks like a metadata write. The agent can't distinguish "blocked because criteria aren't met" from "blocked because of a tool error" without parsing the error message. If assessment fails mid-transition the node ends up in an ambiguous state. Most importantly: the agent can't iterate — it hits the block, doesn't know exactly what failed, can't re-run the assessment alone to check progress. Opaqueness is the enemy of explicitness.

## Option B — New top-level `/assess-design` command

Separate from the existing `/assess` command entirely.

Rejected. The `/assess` command already bridges `cleave | diff | spec` subcommands through the slash-command bridge. Adding a parallel `/assess-design` creates two separate assessment entry points with no conceptual unity. The pattern is established: all assessment lives under `/assess`.

## Option C — `action: "design"` on the existing `/assess` bridge

Extends the existing assess bridge with a new subcommand: `/assess design`. Parallel to `/assess spec`, `/assess cleave`, `/assess diff`.

This is correct. The assess bridge already:
- Dispatches to different evaluation logic per subcommand
- Returns a structured `AssessmentResult` with pass/fail/findings
- Integrates with the lifecycle reconciliation system
- Has established agent guidelines ("run /assess spec before /opsx:archive")

`/assess design` slots in identically: "run /assess design before set_status(decided)". The implementation adds a new `design` case to the bridge dispatcher that:
1. Resolves the target design node (from focus or explicit node_id arg)
2. Reads the node document + parsed acceptanceCriteria section
3. Runs structural constraint checks first (fast, no LLM)
4. Runs LLM evaluation of Scenarios and Falsifiability against the document body
5. Returns findings per artifact type, each with pass/fail + rationale
6. On pass: surfaces "ready to call set_status(decided) and scaffold design OpenSpec archive"

## Evaluation model detail

Assessment prompt provides the LLM with:
- The full node document (Research, Decisions, Implementation Notes)
- Each scenario's Given/When/Then text — asks: "Is the Then clause satisfiable from the document content? If not, what's missing?"
- Each falsifiability condition — asks: "Is this condition addressed (ruled out, accepted as known risk, or explicitly mitigated) in the document? If not, flag as gap."
- Each unchecked constraint — asks: "Is this constraint satisfied by the document content?"

The LLM returns structured findings, not prose. Each finding: `{type: "scenario"|"falsifiability"|"constraint", index: number, pass: boolean, finding: string}`. Structural constraints (open_questions.length, decisions.length) are checked before the LLM call and short-circuit if they fail — no point spending tokens if the document is structurally incomplete.
