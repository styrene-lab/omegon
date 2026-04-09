---
title: Token efficiency comparison harness v1
status: exploring
tags: [testing, benchmarking, token-efficiency, anthropic, harness]
date: 2026-04-09
---

# Token efficiency comparison harness v1

## Overview

This document defines a **lightweight testing harness** for comparing Omegon against other coding-agent harnesses — initially Claude Code — on the question that matters:

> For the same coding task, how many tokens does each harness require to reach an equivalently verified outcome?

The point is **not** to build a generic agent-eval platform.
The point is to give Omegon a reproducible way to answer whether its mediation layer improves or hurts the **correctness:token** ratio.

This v1 should stay small, local-first, and disposable enough to evolve without becoming a second product.

## Goals

1. **Compare harnesses, not just models**
   - Hold the task and acceptance checks constant.
   - Vary the harness (`omegon`, `claude-code`) and optionally the model.

2. **Measure verified task outcomes, not vibes**
   - A run succeeds only if deterministic acceptance checks pass.
   - Prefer shell-based checks already used in the repo: tests, typecheck, lint, targeted assertions.

3. **Track total task token usage**
   - Record provider-reported or harness-reported input/output/cache tokens when available.
   - Treat missing fields honestly; do not fabricate “equivalent” provider semantics.

4. **Expose Omegon-specific overhead buckets**
   - When the harness is Omegon, capture context-composition substats already available in the session log:
     - `sys`
     - `tools`
     - `conv`
     - `mem`
     - `hist`
     - `think`
   - This is the main diagnostic advantage over opaque external harnesses.

5. **Produce results that are easy to compare and audit**
   - One task spec in, one result artifact out.
   - Use plain files (YAML/JSON) stored in-repo or under ignored local directories.

## Non-goals

1. **Not a generalized benchmark platform**
   - No web dashboard.
   - No multi-tenant trace store.
   - No hosted leaderboard.

2. **Not a replacement for SWE-bench or other public benchmarks**
   - v1 is for **Omegon development decisions**, not public claims.

3. **Not a universal agent abstraction layer**
   - Only the adapters needed for current comparisons should exist.
   - Start with `omegon` and `claude-code`.

4. **Not a full observability product**
   - Reuse existing Omegon telemetry and simple result artifacts.
   - Add tracing/export only if v1 proves useful and the missing data justifies it.

## Key question and metrics

The main comparison should be framed as:

- **pass@1** — did the harness solve the task in a single run?
- **tokens per passing run** — total tokens spent to reach a verified pass
- **correctness per token** — normalized score divided by total tokens
- **wall-clock per passing run** — useful secondary systems metric

### Correctness score

v1 should use a simple scalar score:

- `1.0` — all acceptance checks passed
- `0.5` — partial / useful but failed deterministic acceptance checks
- `0.0` — failed

The scorer should stay conservative.
If acceptance checks fail, the default should be `0.0` unless an operator explicitly marks the run partial.

### Token accounting

A run records **run-level totals** for:

- `input_tokens`
- `output_tokens`
- `cache_tokens` when available
- `total_tokens`

For Omegon runs, also record the **latest context-composition snapshot** seen during the run as `omegon_context`:

- `sys`
- `tools`
- `conv`
- `mem`
- `hist`
- `think`
- `free`

Additionally, preserve Omegon's estimator metadata when available:

- `telemetry.estimated_tokens`
- `telemetry.context_window`

Important: these context buckets are a **snapshot diagnostic**, not an aggregate over the entire run. The token totals are run-level metrics; the context buckets describe the most recent turn shape observed by Omegon. These buckets are heuristic, not tokenizer-accurate. That is acceptable as long as they are used for **relative diagnosis** inside Omegon rather than provider-billing claims.

## v1 scope

### In scope

- repo-local task specs
- local execution against a clean worktree or worktree copy
- deterministic acceptance commands
- two harness adapters:
  - `omegon`
  - `claude-code`
- result JSON artifacts
- a small comparison report (CLI text or markdown)
- directory-level aggregation by reading a folder of result JSON artifacts and grouping summaries by task ID

### Out of scope

- distributed execution
- benchmark orchestration across many hosts
- automatic statistical analysis beyond simple summaries
- automatic prompt synthesis
- full replay viewer

## Task specification

A task file defines one comparison case.

Suggested format:

```yaml
id: shadow-context-assembly
repo: .
base_ref: main
model: anthropic:claude-sonnet-4-6
prompt: |
  Finish the shadow-context assembly wiring.
harnesses:
  - omegon
  - claude-code
acceptance:
  - cargo test -p omegon shadow_context
  - cargo test -p omegon request_context
  - npm run typecheck
success_files:
  - core/crates/omegon/src/context.rs
budget:
  max_turns: 20
  max_minutes: 30
notes: |
  Compare equivalent first-shot implementation quality.
```

### Required fields

- `id`
- `repo`
- `base_ref`
- `prompt`
- `acceptance`

### Optional fields

- `harnesses`
- `model` — benchmark-default model for the task; CLI `--model` still overrides it
- `success_files`
- `budget`
- `notes`

### Model and credential integrity

- Task/model selection must be deterministic: CLI `--model` overrides `task.model`, which overrides adapter defaults.
- Benchmark runs must use the same credential surfaces the harness uses in normal local execution.
- For local Omegon Anthropic benchmark runs, an authenticated OAuth session in `~/.config/omegon/auth.json` is a valid credential source when that is the operator's available access path.
- The harness must never silently swap to a different model/provider just because a preferred API-key path is unavailable.

## Result artifact

Each run should emit one JSON file.

Suggested shape:

```json
{
  "task_id": "shadow-context-assembly",
  "harness": "omegon",
  "model": "anthropic:claude-sonnet-4-6",
  "status": "pass",
  "score": 1.0,
  "wall_clock_sec": 812,
  "attempts": 1,
  "tokens": {
    "input": 18234,
    "output": 1102,
    "cache": 0,
    "cache_write": null,
    "total": 19336
  },
  "omegon_context": {
    "sys": 6200,
    "tools": 4100,
    "conv": 2800,
    "mem": 700,
    "hist": 3100,
    "think": 1134,
    "free": 181966
  },
  "telemetry": {
    "estimated_tokens": 19380,
    "context_window": 200000
  },
  "acceptance": {
    "commands": [
      {"cmd": "cargo test -p omegon shadow_context", "exit": 0},
      {"cmd": "cargo test -p omegon request_context", "exit": 0}
    ]
  },
  "artifact_paths": {
    "patch": null,
    "log": "./ai/benchmarks/runs/2026-04-09-shadow-context-assembly-omegon.log"
  }
}
```

### Rules

- Missing token fields remain `null` or omitted — never guessed.
- `status` is a small enum: `pass`, `partial`, `fail`, `error`.
- `score` is numeric for easy aggregation.
- Raw acceptance command exit codes are preserved.

## Harness adapter contract

Each harness adapter should implement the smallest contract that can work.

### Adapter responsibilities

1. prepare an isolated run directory from `base_ref`
2. execute the task prompt through the harness
3. capture usage data the harness exposes
4. run deterministic acceptance commands against that same isolated run directory
5. stop after budget exhaustion or task completion
6. return a normalized run summary

### Minimal normalized adapter output

```json
{
  "model": "anthropic:claude-sonnet-4-6",
  "usage": {
    "input_tokens": 18234,
    "output_tokens": 1102,
    "cache_tokens": 0
  },
  "log_path": "...",
  "patch_path": null,
  "extra": {}
}
```

### Omegon adapter

The Omegon adapter should reuse existing surfaces where possible:

- session-log usage summaries
- per-turn context composition
- existing CLI flags for model/thinking configuration
- existing acceptance workflow after the run

The Omegon adapter is allowed to be richer than other adapters because it is the system under test.
That is acceptable as long as the top-line comparison still uses shared metrics.

### Claude Code adapter

The Claude Code adapter can start crude.
If it can only capture top-line token usage and logs, that is still enough for v1.
It does **not** need Omegon-style internal context buckets.

## Acceptance strategy

Acceptance should be deterministic and shell-based.

### Preferred checks

- targeted `cargo test`
- `npx tsc --noEmit`
- `npm run typecheck`
- targeted integration tests
- file-scope assertions where appropriate

### Avoid in v1

- LLM-as-judge scoring as the primary result
- broad subjective review criteria in the main score
- complex weighted multi-axis rubrics that are hard to reproduce

### Optional operator annotation

A human may add a secondary annotation after the fact:

- `notes`
- `partial_reason`
- `review_findings`

But deterministic acceptance remains the main outcome.

## Comparison report

v1 should generate a simple text or markdown summary like:

```text
Task: shadow-context-assembly

- omegon / anthropic:claude-sonnet-4-6
  status: pass
  score: 1.0
  total tokens: 19336
  wall clock: 812s
  omegon context: sys 6200, tools 4100, conv 2800, mem 700, hist 3100, think 1134

- claude-code / claude-sonnet-4-6
  status: pass
  score: 1.0
  total tokens: 7211
  wall clock: 503s

Delta
- token ratio: 2.68x more tokens for Omegon
- likely excess buckets: sys + tools + hist
```

When `--report` is pointed at a directory, the harness should read `*.json` result files in that directory, ignore non-JSON files, and emit one grouped summary per `task_id`. This is intentionally minimal: no recursive scan, no persisted aggregate artifact, just directory-backed reporting over the existing per-run JSON schema.

The report must degrade honestly on incomplete historical artifacts. If a result lacks usable `tokens.total` data, the report should print `unknown` or mark the token ratio unavailable rather than crashing or inventing a comparison.

That is enough to drive engineering decisions.
No charting is required for v1.

## Storage model

Prefer plain files.

Suggested layout:

```text
ai/benchmarks/
  tasks/
    shadow-context-assembly.yaml
  examples/
    shadow-context-assembly-omegon.result.json
  runs/
    2026-04-09-shadow-context-assembly-omegon.json
    2026-04-09-shadow-context-assembly-claude-code.json
    2026-04-09-shadow-context-assembly-omegon.log
```

Whether `runs/` is tracked or ignored should be decided conservatively.
Default recommendation:

- task specs tracked
- example artifacts tracked when they document the current schema
- live run artifacts ignored unless manually promoted into docs/examples

## Phased implementation plan

### Phase 1 — single-task local runner

Deliver:

- task spec loader
- Omegon adapter
- acceptance runner
- JSON result writer

This phase can answer:

- how many tokens Omegon spent
- whether the task passed
- which Omegon buckets dominated

### Phase 2 — second adapter and direct comparison

Add:

- Claude Code adapter
- side-by-side comparison report

This phase answers the actual product question:

- is there still a material Omegon vs Claude Code token delta for the same task?

### Phase 3 — small task suite

Add:

- 5–10 representative tasks
- aggregate summary across tasks

Only proceed if Phase 2 produces useful signals.

## Design constraints

1. **Stay local-first**
   - No remote service dependency should be required.

2. **Prefer auditable file artifacts**
   - Results must be inspectable without specialized infrastructure.

3. **Do not normalize unlike token semantics dishonestly**
   - Report what each harness exposes.
   - Keep derived comparisons narrow and explicit.

4. **Keep operator setup friction low**
   - The harness should be runnable by an engineer already working in the repo.

5. **Do not outgrow the problem**
   - If a feature does not help compare correctness-per-token, it is probably out of scope for v1.

## Open questions

1. What is the thinnest viable Claude Code adapter surface for v1?
2. Should partial scoring exist in v1, or should v1 remain pass/fail only?
3. Should the harness eventually emit both latest-snapshot and aggregated-per-run context diagnostics for Omegon, or is latest-snapshot sufficient for v1?
4. Where should benchmark task specs live if they need repo-specific setup scripts?

## Recommendation

Proceed with **Phase 1 only** until there is a real task file and one successful Omegon run artifact.
Do not implement the Claude Code adapter, aggregate suite support, or any reporting surface beyond a simple file-backed summary until the local runner proves useful.
