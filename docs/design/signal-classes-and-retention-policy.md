+++
id = "35fe55ce-24d9-40a4-b9ac-302509463f63"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Signal Classes and Retention Policy

## Purpose

This document defines a more precise retention model for Omegon context shaping.
It exists because the benchmark evidence showed two things at once:

1. `omegon --slim` can dramatically reduce token usage while staying competitive or better than peer CLI agents.
2. A blunt attempt to compress history further can reduce the `hist` bucket while increasing total task tokens, which means some retained history is productive working memory.

The goal is to move from generic shortening toward **signal-aware retention**.

## Core principle

Do not ask:

> How do we make history smaller?

Ask:

> Which parts of history prevent rediscovery, and which parts are just residual noise?

Retention policy should preserve **state-bearing evidence** and compress **low-signal repetition**.

## Signal classes

Every decayed tool result should be treated as one of the following classes.

### 1. Failure signal

Definition:

- tool result indicates failure, non-zero exit, exception, or explicit error condition

Examples:

- failed `bash` compile/test command
- failed `read` / `edit` / `change`
- provider error responses
- schema/tool execution errors

Why it matters:

Failure state is often the shortest path to correct next action.
If removed, the model is forced to rediscover the same failure by re-running commands.

Retention policy:

- preserve tool name
- preserve args summary / command summary
- preserve failure kind / exit status
- preserve 1–3 salient error lines
- preserve any file path or target name mentioned in the failure

Compression policy:

- remove repetitive stack/noise lines
- truncate long stderr after salient lines are preserved

### 2. Mutation signal

Definition:

- tool result changes repository state or confirms a concrete mutation

Examples:

- `edit`
- `write`
- `change`
- file-generating shell scripts
- `commit`

Why it matters:

Mutation history is the local ledger of what the agent has already done.
If removed, the model may duplicate or undo work, or re-open already-solved subproblems.

Retention policy:

- preserve tool name
- preserve path(s)
- preserve summary of action
- preserve success/failure

Compression policy:

- do not retain raw diff body in history if the changed file is already recorded elsewhere
- keep path-bearing mutation summary over raw output

### 3. Reference signal

Definition:

- tool result later referenced by assistant reasoning or subsequent actions

Examples:

- assistant mentions a function found in `read`
- assistant acts on a test failure output
- assistant uses a file path or symbol discovered earlier

Why it matters:

This is explicit evidence that the result entered working memory.
Referenced results should decay slower and more gently.

Retention policy:

- preserve the referenced identifier / path / target
- preserve the short causal context explaining why it mattered
- preserve richer summary than non-referenced success noise

Compression policy:

- retain identifiers and targets even if raw body is discarded

### 4. Structural discovery signal

Definition:

- tool result establishes repo topology or problem structure without direct mutation

Examples:

- `read` of a key file
- `codebase_search`
- `design_tree` query
- `openspec_manage get`
- listing discovered relevant files

Why it matters:

These results reduce search cost later.
Over-compressing them can increase repeated reads and repeated searches.

Retention policy:

- preserve path(s)
- preserve query summary
- preserve key identifiers / names
- preserve a short one-line summary of what was found

Compression policy:

- discard long bodies once the path and finding summary are retained

### 5. Noise signal

Definition:

- low-signal successful output that is unlikely to be needed again verbatim

Examples:

- long successful stdout with no later references
- repeated listings
- large read bodies after structure is already known
- generic success confirmations

Why it matters:

This is the main safe compression target.

Retention policy:

- preserve only the minimal stub needed for chronology
- tool name
- maybe args summary
- maybe a one-line preview

Compression policy:

- highly aggressive
- should be the default target for token reduction

## Retention rules by class

### Failure

Decay gently.

Retain:

- error summary
- args summary
- primary failing line(s)
- path/target/command

### Mutation

Retain as structured ledger.

Retain:

- tool name
- path(s)
- action summary
- status

### Reference

Retain with elevated priority.

Retain:

- referenced identifier/path
- concise context
- enough summary to preserve the causal chain

### Structural discovery

Retain path-first summary.

Retain:

- file path(s)
- symbol/query summary
- one-line finding

### Noise

Compress hardest.

Retain:

- minimal stub only

## Mapping to current buckets

This policy is expected to affect:

- **`hist`** most directly
- second-order effects on **`conv`** and total tokens

Success condition:

- `hist` decreases
- total tokens do not rise
- pass rate remains stable

Failure condition:

- `hist` decreases but `conv` or turns rise sharply
- total tokens regress
- benchmark pass rate drops

## Implementation guidance

### First pass

Implement a classifier on decayed tool results using signals already available in history:

- `result.is_error`
- `result.tool_name`
- `result.args_summary`
- whether the turn is in `referenced_turns`

That is enough for a meaningful first version.

### Suggested classifier order

1. `is_error == true` → `Failure`
2. mutation tools (`edit`, `write`, `change`, `commit`) → `Mutation`
3. referenced turn → `Reference`
4. structural tools (`read`, `codebase_search`, lifecycle/spec queries) → `StructuralDiscovery`
5. everything else → `Noise`

### Important constraint

Do not conflate “successful” with “noise”.
A successful `bash cargo test ...` run can still be high-signal if later reasoning depends on it.

## Benchmark method for evaluating policy

Use the existing harness on the same task family and compare:

- total tokens
- wall clock
- pass/fail
- `hist`
- `conv`
- turn count if available

Interpretation rule:

- if `hist` goes down and total tokens go down or stay stable → likely good
- if `hist` goes down and total tokens go up sharply → retention got too aggressive or too lossy

## Recommendation

The next implementation pass should not make history globally shorter.
It should assign each decayed tool result a signal class and choose the summary shape accordingly.

That is the path most consistent with the benchmark evidence so far.
