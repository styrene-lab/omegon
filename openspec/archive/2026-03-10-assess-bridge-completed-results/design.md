+++
id = "8b6a4fec-ac3a-4177-81eb-2e8ccecf74b1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Assess bridge returns completed structured results — Design

## Architecture Decisions

### Decision: Bridged /assess must produce completed structured results in-band

**Status:** decided
**Rationale:** Agent and tool callers rely on the bridge result as the authoritative contract. If the real assessment happens in a later follow-up turn, callers cannot safely persist lifecycle state. The structured executor should perform the assessment synchronously for bridged usage and reserve follow-up prompting for interactive-only flows.

## Research Context

### Bridge contract mismatch

`/assess spec` currently prepares a review by emitting a kickoff banner and sending the real reviewer prompt as a follow-up turn. The slash-command bridge returns immediately with the kickoff `humanText`, so tool callers observe preparation rather than completion. This makes the returned lifecycle metadata non-authoritative for `reconcile_after_assess`.

## File Changes

- `extensions/cleave/index.ts` (modified) — Separate interactive follow-up assessment flows from bridged synchronous structured execution
- `extensions/cleave/assessment.ts` (modified) — Define completed structured assessment result shape and lifecycle-safe outcomes for bridged assess execution
- `extensions/cleave/bridge.ts` (modified) — Preserve normalized bridge envelope while carrying completed assessment data
- `extensions/lib/slash-command-bridge.ts` (modified) — Keep bridge semantics explicit for synchronous structured slash-command execution
- `extensions/cleave/*.test.ts` (modified) — Regression coverage for bridged /assess spec completion semantics

## Constraints

- Bridged `/assess spec` must not claim a completed lifecycle outcome until the review logic has actually finished.
- Interactive `/assess` can remain follow-up driven, but tool/agent invocation must return the completed structured envelope in the initial result.
- `result.args` must continue to preserve the full original tokenized invocation.
