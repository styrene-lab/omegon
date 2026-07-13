+++
title = "`/status` and `/stats` 0.28 release hardening"
tags = ["release","0.28.0","commands","status","stats","telemetry"]
+++

+++
id = "327c9cb6-4652-4fb3-bc34-4e3cb47bf60d"
kind = "design_node"

[data]
title = "`/status` and `/stats` 0.28 release hardening"
status = "decided"
issue_type = "feature"
priority = 1
parent = "harness-status-contract"
dependencies = []
open_questions = []
+++

## Overview

# `/status` and `/stats` 0.28 release hardening

# `/status` and `/stats` 0.28 release hardening

## Overview

The 0.28 release must make the two operator diagnostic commands truthful, live, and consistent at their canonical runtime boundary. `/status` owns current harness/runtime readiness and routing. `/stats` owns current-session activity and context use. Neither command may fabricate unavailable metrics, and command registry affordances must match implemented syntax.

## Research

Current evidence:

- `/status` assembles a fresh `HarnessStatus`, which can diverge from the live snapshot maintained in `DashboardHandles.harness`.
- `/stats` reads the live harness snapshot but hardcodes tool calls to zero even though `DashboardHandles.session.tool_calls` is maintained by the TUI.
- The registry advertises `/stats bench`, while canonical slash parsing only accepts argument-free `/stats`.
- TUI tests prove only that some control request was queued; they do not assert the request variant or output contract.
- ACP has separate `/status` and `/stats` implementations. Full projection convergence is larger than a release-hardening patch, but 0.28 must remove contradictory/fabricated output and establish reusable projection seams.

## Decisions

### Decision: `/status` prefers the live harness snapshot

**Status:** decided

Read `DashboardHandles.harness` and clone the current snapshot. Fall back to `HarnessStatus::assemble()` only when the shared handle is absent or unavailable. Apply live routing/settings fields to the selected snapshot before rendering.

### Decision: `/stats` reads canonical shared session counters

**Status:** decided

Turns and tool calls come from `DashboardHandles.session` when available. Conversation turn count remains a safe fallback. No metric is displayed unless backed by observed state.

### Decision: remove `/stats bench` for 0.28

**Status:** decided

There is no canonical benchmark implementation. Remove the advertised subcommand rather than inventing behavior during release hardening.

### Decision: release output remains text but projection construction becomes testable

**Status:** decided

Keep command-panel text compatibility for 0.28. Extract deterministic projection/render helpers where needed so runtime handlers and tests share the same contract. A fully versioned cross-surface DTO belongs to the post-0.28 workstream.

### Decision: diagnostics degrade instead of panicking

**Status:** decided

Poisoned settings or telemetry locks recover their inner state where safe; an unavailable telemetry snapshot falls back to truthful known values.

## Open Questions

*No open questions.*

## Acceptance Criteria

- `/status` renders values from a populated live harness snapshot.
- `/stats` reports the shared non-zero tool-call count and observed turn count.
- `/stats bench` is not advertised.
- TUI dispatch tests assert exact control request variants.
- Output tests cover populated state, zero context window, and absence of fabricated values.
- Behavior change is recorded in `[Unreleased]`.

## Implementation Notes

### File Scope

- `core/crates/omegon/src/control_runtime.rs`
- `core/crates/omegon/src/command_registry.rs`
- `core/crates/omegon/src/tui/tests.rs`
- focused semantic surface module if extraction is warranted
- `CHANGELOG.md`

### Constraints

- Never expose secret values; readiness metadata only.
- Preserve `/status` as harness/runtime health and `/stats` as session telemetry.
- Do not expand 0.28 scope into historical telemetry, cost accounting, percentile timing, or benchmark execution.

## Open Questions
