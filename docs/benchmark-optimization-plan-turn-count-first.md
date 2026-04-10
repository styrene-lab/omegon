---
title: Benchmark Optimization Plan — Turn Count First, Prompt Mass Second
status: active
tags: [benchmark, optimization, token-efficiency, turn-count, prompt-mass, rc65]
date: 2026-04-10
---

# Benchmark Optimization Plan — Turn Count First, Prompt Mass Second

Reference findings:
- [[benchmark-finding-rc63-shadow-context]]
- [[benchmark-finding-rc64-shadow-context]]

## Problem statement

The `rc.64` cache-aware benchmark established that Omegon's overspend is not primarily a missing-cache problem.

On `example-shadow-context`, the telemetry-enhanced reruns showed:

| Harness | Wall clock (s) | Total tokens | Turn count | Avg input / turn |
|---|---:|---:|---:|---:|
| `omegon` | `1074.209` | `2,529,077` | `51` | `41,299` |
| `om` | `880.221` | `2,095,502` | `41` | `45,567` |

This means two things are true at once:

1. **Too many turns** — 41–51 turns is wildly too high for a task that is already effectively solved in the target tree.
2. **Too much fresh input per turn** — average fresh input remains ~41k–46k tokens even after cache support lands.

The highest-value optimization order is therefore:

1. reduce turn count
2. reduce per-turn prompt mass

## Goal for next RC

Make Omegon converge faster on already-solved or quickly-verifiable tasks without regressing correctness, and reduce total token burn primarily by avoiding unnecessary turns.

## Track 1 — Turn-count reduction (first priority)

### Hypothesis

Omegon spends too many turns re-deriving state and continuing exploratory behavior after enough evidence already exists to conclude the task is complete or near-complete.

### Why this is first

Reducing turn count multiplies every other saving:
- fewer fresh input payloads
- fewer repeated cached prefixes
- fewer tool invocations
- fewer opportunities for prompt bloat to recur

### Candidate interventions

#### 1. Early-stop on passing evidence

When all of the following are true within a turn window:
- acceptance-relevant test passes
- no code changes are needed or the target area is already implemented
- latest assistant reasoning concludes completion / no-op

then bias the loop toward final answer instead of another exploration turn.

Potential implementation surfaces:
- `core/crates/omegon/src/loop.rs`
- `core/crates/omegon/src/conversation.rs`

#### 2. Stronger completion heuristics after read-only validation

If the agent has only used read/search/test tools and the target condition already holds, do not require additional “do more work” turns.

Signal candidates:
- no write/edit/change calls in session
- acceptance passes
- assistant result contains explicit “already complete” semantics

#### 3. Reduce commit-nudge / follow-up churn for no-op tasks

If a task required no modifications, the loop should not push extra turns looking for commit behavior.

#### 4. Bias toward summarize-and-stop after decisive tests

Once the acceptance command or a narrow validation command succeeds and no contradictory evidence appears, prefer summarization over further exploration.

### Success metric

For `example-shadow-context`:
- reduce `omegon` turn count materially below `51`
- reduce `om` turn count materially below `41`

## Track 2 — Prompt-mass reduction (second priority)

### Hypothesis

Even after cache support, Omegon still pays too much fresh input because the dynamic prompt region and replayed conversation/tool context remain too large.

### Candidate interventions

#### 1. Audit dynamic prompt content after cache boundary

Measure what is still sent after `CACHE_BOUNDARY` each turn:
- intent block
- memory injections
- bus feature injections
- tool/history sections
- other dynamic system text

Add instrumentation before changing policy.

#### 2. Reduce repeated tool-history injection

Current telemetry suggests history remains a non-trivial component. Investigate whether old tool traces are being repeated longer than necessary for this task class.

#### 3. Tighten context inclusion for solved / near-solved tasks

If the task scope is narrow and recent evidence is decisive, reduce background context breadth rather than keeping the full systems-engineering frame every turn.

#### 4. Re-examine slim-mode dynamic prompt shape

`om` currently wins mostly by using fewer turns, not by having a lighter per-turn input. Its average fresh input per turn is actually higher than default Omegon in the telemetry run.

That means slim mode needs a second pass on *what* stays in the dynamic prompt, not just which static sections are suppressed.

### Success metric

For `example-shadow-context`:
- reduce average fresh input/turn materially below:
  - `41,299` for `omegon`
  - `45,567` for `om`

## Execution order

### Phase A — instrumentation complete
- [x] cache reads/writes recorded in benchmark artifacts
- [x] turn count and per-turn averages recorded in benchmark artifacts

### Phase B — turn-count attack
- [ ] identify exact loop conditions that permit already-solved tasks to continue for dozens of turns
- [ ] implement at least one early-stop or completion-bias heuristic
- [ ] rerun `pi` vs `omegon` vs `om`

### Phase C — prompt-mass attack
- [ ] instrument dynamic prompt segment sizes after `CACHE_BOUNDARY`
- [ ] identify largest uncached contributors
- [ ] trim one concrete contributor
- [ ] rerun benchmark set

## Release gating recommendation

The next RC should be considered successful only if it shows at least one of:

- a large turn-count reduction for Omegon/om on `example-shadow-context`
- a large drop in average fresh input per turn
- preferably both

## Decision

Treat turn-count reduction as the first optimization target. Prompt-mass reduction remains mandatory, but it should follow once the system stops taking 41–51 turns on a task that is already effectively complete.
