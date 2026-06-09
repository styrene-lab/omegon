---
id: surface-replay-test-harness
title: "Surface replay and action test harness"
status: exploring
parent: ui-surface-action-protocol
tags: [testing, replay, surfaces]
open_questions:
  - "What is the first replay fixture format: pure Rust test builder, JSONL envelope fixture, or recorded session excerpt?"
dependencies: []
related: []
---

# Surface replay and action test harness

## Overview

Build replay/snapshot tests for surface events and action handling so alternate frontends can validate against semantic behavior instead of terminal-cell snapshots.

## Research

### Replay harness gap analysis

Current replay coverage is action-outcome-only: accepted/rejected/noop/deferred semantic action outcomes can be wrapped in versioned envelopes. Missing replay layers: typed action payload DTO conversion, revision allocation policy, surface snapshot fixtures, and event-stream replay from runtime events into surfaces. The next useful test harness slice should couple an action envelope/outcome with a known semantic side effect, e.g. SubmitPrompt produces a TuiCommand plus accepted outcome envelope, without asserting terminal rendering.

### Orbytal clock applicability check

Sister project Orbytal currently has design docs but no Rust source/Cargo crate on disk. Its clock concept is a Bevy ECS simulation-time model covering realtime/manual/replay/time-warp/fixed-step scheduling, state snapshots, semantic events, and render extraction. That is not the same as Omegon's UI replay revision counter. Useful lesson: keep wall time, simulation/playback time, semantic identity, and replay revision separate.

## Decisions

### Start replay harness with action outcome envelopes

**Status:** accepted

**Rationale:** Commit 33e30f7 added `ui_runtime::replay::outcome_to_envelope`, giving tests and future transports a deterministic bridge from internal `UiActionOutcome` values to versioned `UiActionOutcomeEnvelope` records before adding full surface event replay.

### Use a global monotonic UiRevisionCounter for replay revisions

**Status:** accepted

**Rationale:** Commit 7229a668 replaced the temporary replay clock concept with `UiRevision` and `UiRevisionCounter`, making revisions deterministic causal ordering rather than time. The counter uses checked increment and converts to `u64` only at the envelope boundary.

## Open Questions

- What is the first replay fixture format: pure Rust test builder, JSONL envelope fixture, or recorded session excerpt?
