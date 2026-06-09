---
id: surface-replay-test-harness
title: "Surface replay and action test harness"
status: exploring
parent: ui-surface-action-protocol
tags: [testing, replay, surfaces]
open_questions:
  - "What is the first replay fixture format: pure Rust test builder, JSONL envelope fixture, or recorded session excerpt?"
  - "Should replay revisions be allocated globally per session or separately per surface/action stream?"
dependencies: []
related: []
---

# Surface replay and action test harness

## Overview

Build replay/snapshot tests for surface events and action handling so alternate frontends can validate against semantic behavior instead of terminal-cell snapshots.

## Research

### Replay harness gap analysis

Current replay coverage is action-outcome-only: accepted/rejected/noop/deferred semantic action outcomes can be wrapped in versioned envelopes. Missing replay layers: typed action payload DTO conversion, revision allocation policy, surface snapshot fixtures, and event-stream replay from runtime events into surfaces. The next useful test harness slice should couple an action envelope/outcome with a known semantic side effect, e.g. SubmitPrompt produces a TuiCommand plus accepted outcome envelope, without asserting terminal rendering.

## Decisions

### Start replay harness with action outcome envelopes

**Status:** accepted

**Rationale:** Commit 33e30f7 added `ui_runtime::replay::outcome_to_envelope`, giving tests and future transports a deterministic bridge from internal `UiActionOutcome` values to versioned `UiActionOutcomeEnvelope` records before adding full surface event replay.

## Open Questions

- What is the first replay fixture format: pure Rust test builder, JSONL envelope fixture, or recorded session excerpt?
- Should replay revisions be allocated globally per session or separately per surface/action stream?
