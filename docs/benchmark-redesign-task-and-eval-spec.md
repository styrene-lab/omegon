+++
id = "29ed7d34-e849-4e2a-bcf2-1244aa429535"
kind = "document"
title = "Benchmark Redesign — Task, Acceptance, Process, and Efficiency Spec"
status = "active"
tags = ["benchmark", "evals", "harness", "task-design", "provider-matrix"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
date = "2026-04-10"
+++

# Benchmark Redesign — Task, Acceptance, Process, and Efficiency Spec

## Why redesign now

The current benchmark setup was good enough to answer a first question:

> Can Omegon be compared reproducibly against a few other harnesses on a clean-room coding task?

The answer is yes.

But it is no longer good enough for the next question:

> Is a given harness change actually improving the harness, or merely moving noise around for one provider, one profile, or one task shape?

That is the current gap.

The old benchmark artifacts are mostly no longer useful as a planning substrate because they were produced before:
- cache-aware accounting fully landed
- turn-count telemetry landed
- provider/model temperament was recognized as an explicit axis
- execution-pressure heuristics entered the harness
- Codex-specific churn patterns were observed

So the correct move is not to keep accreting more runs onto a partially obsolete matrix.
The correct move is to define a sharper benchmark contract, archive or discard stale runs, and start a fresh series from a clean baseline.

## Core principle

Keep the current local clean-room runner.
Do **not** replace the execution harness yet.

Instead, redesign:
- the **task definition**
- the **acceptance contract**
- the **process metrics**
- the **efficiency metrics**
- the **artifact schema**
- the **matrix dimensions**

This is the lean/effective compromise:
- **lean** because we do not migrate to a new hosted or third-party runtime harness
- **effective** because we adopt a much stronger eval model and trace discipline

## Benchmark philosophy

A benchmark run should answer four different questions, not one.

### 1. Outcome
Did the harness achieve the target state?

This must remain deterministic whenever possible.

### 2. Process
Did the harness take a sane path to get there?

This is where we detect:
- unnecessary orientation
- repeated broad search
- delayed editing
- lack of decisive validation
- thrashing between tools

### 3. Efficiency
How much did the harness spend to get there?

This includes:
- wall clock
- turns
- tool calls
- tokens
- time-to-first-edit
- inspection-only turns before edit

### 4. Discipline
Did the harness behave like a well-run engineering system rather than a chatty tourist?

This includes:
- stopping when enough evidence exists
- not over-reading after decisive evidence
- not mutating too early without repo contact
- not narrating endlessly instead of editing

A benchmark that only measures Outcome will miss harness regressions.
A benchmark that only measures Efficiency will reward reckless hacks.
We need all four.

---

# What the “task” actually is

The benchmarked thing is **not** merely “solve a coding prompt.”

The benchmarked thing is:

> Given an operator directive, a clean repository state, a provider/model, and a set of tool affordances, can the harness drive a coding agent to a verified engineering outcome with sane process and acceptable cost?

That means each benchmark task must define:
1. the engineering problem
2. the expected solution surface
3. the repo-local evidence that should matter
4. the deterministic outcome checks
5. the process constraints
6. the efficiency expectations

## Task taxonomy

We should explicitly separate benchmark tasks by **harness behavior under test**.

### A. Implementation tasks
Agent must make a real code change.

Examples:
- wire a missing integration
- fix a logic bug
- add a targeted test
- repair a broken metric path

Primary risks measured:
- delayed editing
- premature editing
- insufficient validation
- context/tool thrash

### B. Read-only recognition tasks
Agent should discover that the target condition is already satisfied or that no change is justified.

Examples:
- “verify that X is already wired”
- “assess whether Y is missing” when it is already present

Primary risks measured:
- pointless edits
- endless searching after decisive evidence
- failure to conclude no-op cleanly

### C. Triage/diagnosis tasks
Agent must isolate cause correctly before editing.

Examples:
- classify an auth failure path
- identify why a benchmark drifted models
- locate the source of duplicated telemetry

Primary risks measured:
- false certainty
- over-editing before diagnosis
- failure to gather decisive evidence

### D. Repair-after-diagnosis tasks
Agent must first identify the bug, then apply the smallest correct fix.

Examples:
- auth fallback bug
- release-timeline mutation bug
- stale identity reuse bug

Primary risks measured:
- too much orientation before acting
- too much action before diagnosis
- incomplete fix scope

A task file should declare which of these categories it is testing.

---

# Task spec redesign

## Proposed task schema

```yaml
id: shadow-context-assembly-v2
kind: implementation
repo: .
base_ref: main

prompt: |
  Finish the shadow-context assembly wiring.

matrix:
  harnesses:
    - omegon
    - om
    - pi
    - claude-code
  models:
    - anthropic:claude-sonnet-4-6
    - openai-codex:gpt-5.4

expected_solution:
  summary: |
    Wire shadow-context assembly into the relevant runtime path so the
    task acceptance tests pass without unrelated churn.
  primary_files:
    - core/crates/omegon/src/context.rs
    - core/crates/omegon/src/loop.rs
  forbidden_patterns:
    - broad unrelated refactors
    - disabling tests
    - acceptance-check bypasses

acceptance:
  required:
    - cargo test -p omegon shadow_context
  optional:
    - cargo test -p omegon request_context
  failure_if:
    - git diff --name-only | rg '^core/crates/omegon/src/' -v

process_expectations:
  must_touch_repo_before_edit: true
  max_orientation_only_turns: 1
  max_inspection_only_turns_before_edit: 2
  require_narrow_validation_after_edit: true
  disallow_repeated_broad_search_after_decisive_evidence: true

budgets:
  soft:
    max_turns: 12
    max_tool_calls: 40
    max_wall_clock_sec: 900
    max_total_tokens: 1200000
    max_input_tokens: 1000000
  hard:
    max_turns: 25
    max_wall_clock_sec: 1800

notes: |
  This task primarily evaluates execution pressure and anti-churn behavior,
  not only correctness.
```

## Required top-level fields

- `id`
- `kind`
- `repo`
- `base_ref`
- `prompt`
- `acceptance.required`
- `matrix.harnesses`
- `matrix.models`

## Optional but strongly recommended fields

- `expected_solution`
- `process_expectations`
- `budgets`
- `notes`

## Why this is better than the current task spec

The current task spec is underpowered because it mostly says:
- here is the prompt
- here is an acceptance command
- here is a rough budget

That is enough to say pass/fail.
It is not enough to say whether the harness behaved well.

The redesigned schema turns each task into:
- a **coding goal**
- a **process contract**
- an **efficiency contract**

That is the right unit for harness evaluation.

---

# Acceptance criteria design

Acceptance must be designed in layers.

## Layer 1 — Deterministic outcome acceptance

This is the non-negotiable pass/fail layer.

Examples:
- `cargo test -p omegon shadow_context`
- `cargo test -p omegon request_context`
- `npm run typecheck`
- `cargo check`
- `cargo test specific_module`

Rules:
- prefer narrow commands over repo-wide commands when the task scope is local
- acceptance commands must run in the same clean-room repo used by the harness
- acceptance should prove the intended behavior, not merely compileability

## Layer 2 — Guardrail acceptance

These fail the run even if the primary test passes.

Examples:
- unexpected file sprawl
- disabled tests
- unrelated refactors
- dirty worktree after run
- deleting assertions instead of fixing behavior

Examples of guardrail commands:
- `git diff --name-only`
- `git diff --check`
- targeted grep checks for forbidden edits

## Layer 3 — Process acceptance

These should not always fail the run, but they must be recorded and scored.

Examples:
- too many orientation-only turns
- too many inspection-only turns before first edit
- repeated broad search after decisive evidence
- no validation after edit

This layer creates process scores, not necessarily hard failure.

## Outcome scoring proposal

### Outcome score
- `1.0` — all required acceptance checks pass and no hard guardrail fails
- `0.0` — any required acceptance check fails or any hard guardrail fails

No fuzzy partial credit by default.
If we ever want partial credit, it should be explicit and task-specific.

### Process score
A separate bounded score in `[0, 1]`, derived from process rules.

Example deductions:
- `-0.2` if orientation-only turns exceed task max
- `-0.2` if inspection-only turns before first edit exceed max
- `-0.2` if no narrow validation occurs after first edit
- `-0.2` if repeated broad search persists after decisive evidence
- `-0.2` if no-op completion was missed on a read-only task

### Efficiency score
Also separate from outcome.

Not absolute. Relative to the matrix.
For a given task, compute percentile/rank or normalized values for:
- wall clock
- total tokens
- turns
- time-to-first-edit

### Discipline score
Binary or low-cardinality flags are often enough:
- `stopped_when_done`
- `avoided_broad_search_after_evidence`
- `edited_only_after_repo_contact`
- `validated_after_edit`

Do not collapse all of these into one number too early.
Preserve raw facts first.

---

# Process metrics we should capture

## Per-run metrics

### Basic identity
- task id
- harness
- profile (`omegon`, `om`, etc.)
- provider/model
- repo/base ref
- git SHA of harness under test

### Outcome metrics
- pass/fail
- acceptance command results
- guardrail failures
- files changed

### Efficiency metrics
- wall clock
- turn count
- total tool calls
- total input tokens
- total output tokens
- cache read tokens
- cache write tokens
- total tokens

### Time-to-X metrics
- time to first tool call
- time to first repo read
- time to first repo search
- time to first edit
- time to first validation command

### Turn-shape metrics
- orientation-only turns before first repo contact
- inspection-only turns before first edit
- turns after decisive evidence before stop
- repeated same-tool streaks

### Heuristic-trigger metrics
- did first-turn execution-bias trigger?
- did post-inspection execution-pressure trigger?
- how many times?

These are critical because once we introduce harness-level heuristics, we need to know whether they are doing useful work or just masking deeper problems.

## Per-turn metrics

For each turn, store:
- turn number
- provider/model
- input/output/cache token counts
- tool calls issued
- whether turn contained any repo inspection
- whether turn contained any mutation
- whether turn contained any validation
- turn-end reason
- whether a harness system nudge was injected after the turn

This gives us the raw material for later trace grading.

---

# Token spend design

## What we should measure

At minimum:
- input tokens
- output tokens
- cache read tokens
- cache write tokens
- total tokens

## What we should not pretend to know

Do not force fake equivalence across providers where semantics differ.
If one provider does not expose a token bucket cleanly, record the missingness honestly.

## Token budget philosophy

Budgets should not be treated as instant hard-fail in v1 of the redesign.
That would encourage gaming.

Instead:
- define **soft** budgets used for scoring and regression detection
- keep **hard** budgets only for catastrophic runaway control

### Soft budgets
Used to say:
- this run passed, but with unacceptable spend
- this heuristic reduced turns but exploded input tokens
- this provider is pathologically expensive for this task

### Hard budgets
Only for:
- infinite loops
- runaway tool thrash
- broken harness behavior

## Cost normalization

Eventually, we may want estimated dollar cost.
But that should be derived from token totals after the basic artifact schema is stabilized.
Do not make pricing tables a prerequisite for the redesign.

---

# Tool-use design

Tool use is not just a side effect. It is part of the benchmark.

## Tool categories

Define tool categories so process analysis is easier:

- **orientation**
  - `memory_recall`
  - `context_status`
  - `request_context`

- **repo inspection**
  - `read`
  - `codebase_search`
  - `view`

- **mutation**
  - `edit`
  - `write`
  - `change`

- **validation**
  - `bash` when running tests/checks
  - possibly structured validator tools later

- **meta/system**
  - `manage_tools`
  - `context_compact`
  - etc.

## Why categorize

Because we care about patterns, not just counts.

Examples of bad patterns:
- orientation → orientation → orientation
- inspection → inspection → inspection → inspection without edit
- edit → more broad search instead of validation

Examples of good patterns:
- repo inspection → targeted read → edit → narrow validation
- inspection → recognize already-complete → validation → stop

## Metrics to compute from tool use

- orientation-only turns before repo contact
- inspection-only turns before first edit
- validation gap after edit
- repeated broad-search streak length
- tool entropy / diversity by phase

---

# Matrix design

We now need a benchmark matrix with at least two axes.

## Axis 1 — Harness/profile
- `omegon`
- `om`
- `pi`
- `claude-code`

## Axis 2 — Provider/model
Minimum set:
- `anthropic:claude-sonnet-4-6`
- `openai-codex:gpt-5.4`

Later candidates:
- `anthropic:claude-opus-4-6`
- `openai-codex:gpt-5.4-mini`
- any local model we deem operationally relevant

## Why both axes matter

A harness change can:
- help on Sonnet and hurt on Codex
- help on Codex and overconstrain Sonnet
- help `omegon` but not `om`
- help only when a provider is naturally more decisive

Without both axes, we will keep fooling ourselves.

---

# Fresh-start benchmark reset plan

## Why clean old runs

Past runs are not worthless historically, but they are no longer a clean baseline for current optimization decisions.

So we should distinguish:
- **historical findings** — keep in docs
- **active matrix artifacts** — reset and rebuild

## Recommended reset strategy

### Keep
- benchmark docs and findings under `docs/`
- task design docs
- analysis docs

### Archive or remove from active comparison set
- old JSON result artifacts under `ai/benchmarks/runs/`
- old ad hoc task specs that do not conform to the new schema

### Start fresh with a new active baseline set
Initial fresh baseline matrix should be intentionally small:
- 1 task from each key task kind we care about
- 2 providers/models minimum
- 2–4 harness profiles maximum

Do not explode the matrix on day one.

---

# Proposed first fresh benchmark set

## Task 1 — Implementation / anti-churn
`shadow-context-assembly-v2`

Purpose:
- measure execution pressure
- detect orientation churn
- detect inspection-only stalls before edit

## Task 2 — Read-only recognition / no-op discipline
A task where the requested change is already present.

Purpose:
- measure stop discipline
- measure pointless edit avoidance
- detect “search forever” behavior

## Task 3 — Diagnosis-first repair
A targeted failure classification or fallback bug.

Purpose:
- ensure the harness does not force editing before diagnosis
- measure evidence gathering followed by smallest correct fix

That trio is enough to reveal most harness pathologies.

---

# Implementation recommendation

## Phase 1 — Schema and artifact redesign
Deliver:
- new task schema
- new result artifact schema
- tool category mapping
- process metric calculation
- archival/reset of stale active runs

## Phase 2 — Sequential comparison runner
Deliver:
- one-command matrix expansion over:
  - harness/profile
  - provider/model
- sequential execution by default to avoid cargo lock contention

## Phase 3 — Analysis/reporting
Deliver:
- task-grouped report
- provider/model comparison tables
- time-to-first-edit and inspection-stall summaries
- heuristic-trigger summaries

## Phase 4 — Optional observability integration
Only if needed:
- Langfuse or LangSmith for richer trace inspection
- keep runner local-first regardless

---

# Decision

We should reset the active benchmark program and rebuild it around a stronger task/eval contract.

The correct compromise is:
- keep the local Python clean-room runner
- redesign the task schema and artifact model
- treat benchmark evaluation as four-dimensional:
  - outcome
  - process
  - efficiency
  - discipline
- require a provider/model axis in addition to the harness/profile axis
- start a fresh active matrix from a small, well-designed task set rather than dragging obsolete runs forward

## Immediate next authoring tasks

1. define the new task YAML schema precisely
2. define the new result JSON schema precisely
3. define tool category and process-metric derivation rules
4. define the archival/reset mechanics for old benchmark runs
5. author the first fresh benchmark task set
