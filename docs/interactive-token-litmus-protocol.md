---
title: Interactive Token Litmus Protocol
author: omegon
status: active
tags: [benchmark, interactive, token-efficiency, release, litmus]
date: 2026-04-11
---

# Interactive Token Litmus Protocol

## Purpose

This protocol exists because the current single-shot autonomous feature benchmark is **not** a faithful proxy for the operator-steered Omegon experience.

Use this protocol to answer the narrower release question that actually matters:

> Is the candidate release materially worse than the prior experience for a skilled operator driving the same short workflow?

This is a **bounded interactive comparison**, not an autonomous eval.

## Scope

Use this for medium-complexity coding tasks that the operator normally drives interactively in a few minutes.

Current target litmus:
- `@` symbol file selection in the TUI input area

## Comparison cell definition

Run the same protocol against:
- baseline build/ref
- candidate build/ref

Hold constant:
- repo/task setup
- operator prompt sequence
- model/provider
- acceptance checks
- stop conditions

Recommended first comparison:
- baseline: prior known-good interactive build or release
- candidate: `v0.15.10-rc.74`
- model: `anthropic:claude-sonnet-4-6`

## Hard bounds

The protocol is invalid if you let the session meander indefinitely.

Stop immediately on the first condition hit:
- success criteria satisfied
- 3 operator prompts sent
- 10 minutes elapsed
- 12 completed turns

If the session hits a bound before success, record it as a bounded failure.

## Prompt script

Use these prompts exactly. Do not improvise unless the run is obviously wedged and you are explicitly recording a protocol deviation.

### Prompt 1

```text
Add @ file selection in the TUI input area. Keep scope tight and implement the smallest viable version.
```

### Prompt 2

Send only if the agent is wandering or broadening scope.

```text
Stop broad exploration. Stay in the TUI input implementation path and touch the minimum files required.
```

### Prompt 3

Send only if still not converged.

```text
Finish the smallest working implementation now. Do not broaden scope further.
```

## Acceptance

Use the same deterministic checks for both runs.

Minimum acceptance for this litmus:

```bash
grep -RIEq "(@|at[_-])(symbol|file).*pick|pick.*file" core/crates/omegon/src/tui/
```

If stronger targeted checks exist by the time you run this, use them for both cells.

## Evidence collection

For Omegon interactive runs, the authoritative token evidence comes from the session journal turn summaries written by the session-log feature.

Those lines have this format:

```text
- turn N — provider / model in:X out:Y cache:Z
```

Collect:
- total input tokens
- total output tokens
- total cache read tokens
- total tokens = input + output + cache
- completed turns
- wall clock
- outcome: pass/fail/bounded_fail

## Required run record

For each comparison cell, record:
- build/ref
- model/provider
- prompts actually sent (1/2/3)
- completed turns
- wall clock seconds
- total input/output/cache/total tokens
- acceptance result
- operator notes on obvious drift or recovery

## Grading

This protocol is meant to detect **material regressions**, not declare perfection.

Suggested release gate:
- candidate fails if it uses >2x baseline total tokens
- or >2x baseline wall clock
- or bounded-fails where baseline passes
- under the same scripted interactive protocol

## Interpretation

This protocol answers:
- whether the operator-steered experience is materially worse than before

It does **not** answer:
- whether the autonomous controller is strong enough for one-shot feature delivery

Keep those benchmark classes separate.
