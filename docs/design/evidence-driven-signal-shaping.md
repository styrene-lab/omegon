+++
id = "80e173a7-edb7-47de-83db-2856066dbc9b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Evidence-Driven Signal Shaping

## Thesis

Omegon should not optimize prompt and context shape by intuition alone.
It should use a benchmark harness to produce evidence about:

1. **what the token difference is**
2. **where the token difference appears in the harness surface**
3. **whether reducing a surface lowers total tokens or merely shifts cost elsewhere**

This is the discipline that should guide future prompt, history, and tool-surface work.

## What the benchmark harness is for

The token-efficiency harness is not just a scoreboard.
It is an instrumentation loop for product direction.

The useful outputs are:

- pass/fail against deterministic acceptance
- total task token usage
- wall-clock time
- Omegon context buckets (`sys`, `tools`, `conv`, `mem`, `hist`, `think`)
- artifacts/logs that explain *why* a run behaved the way it did

The harness is valuable when it answers:

- Did a change reduce total tokens?
- Did it preserve pass rate?
- Which buckets moved?
- Did cost truly disappear, or did it reappear as extra turns / extra conversation / rediscovery?

## Findings so far

### 1. Default Omegon vs slim Omegon vs Claude Code

A valid benchmark run on `example-shadow-context` established:

- **Default Omegon**: materially higher token cost
- **Slim Omegon**: dramatically lower token cost
- **Claude Code**: a fair external baseline

The strongest validated comparison showed that `omegon --slim` could be slightly cheaper than Claude Code while remaining faster on wall clock for the same verified task.

This supports a product split:

- **Default Omegon** = premium harness mode
- **`omegon --slim`** = de-facto comparison mode for mainstream CLI coding agents

### 2. Bucket-level evidence matters

The slim snapshot showed the remaining cost concentrated in:

- `hist`
- `sys`
- `tools`

This means most remaining overhead is not generic “conversation bloat”.
It is harness structure:

- retained tool/result history
- system prompt surface
- tool schema exposure

That is actionable evidence.

### 3. Not all history is waste

An aggressive attempt to compress slim-mode tool history reduced the `hist` bucket in the latest-turn snapshot but caused total task tokens to rise sharply.

This is critical evidence.

It means:

> Some retained history is productive working memory.

The benchmark showed that over-compressing history can force the model to re-derive prior state, increasing turns and total token usage even while the history bucket itself shrinks.

This is exactly why the harness must be treated as a design instrument rather than a vanity metric.

## Methodology: how evidence should shape direction

### Step 1 — Establish a valid baseline

Before changing signal shape, ensure we have a trustworthy baseline with:

- deterministic acceptance pass
- stable model selection
- valid auth/runtime environment
- trustworthy result artifact

Do not infer from failed infrastructure runs.

### Step 2 — Change one signal dimension at a time

Examples:

- reduce system prompt overlays
- narrow tool schema
- compress tool history
- alter thinking defaults
- alter context-class defaults

Do **not** change multiple unrelated dimensions if the goal is attribution.

### Step 3 — Compare both totals and buckets

A change is not a win merely because one bucket shrank.

A real win requires checking:

- pass/fail
- total tokens
- wall clock
- context-bucket movement

If `hist` shrinks but `conv` and total tokens balloon, the optimization failed.

### Step 4 — Preserve productive signal, cut wasteful signal

Evidence should push us toward:

- **importance-weighted retention**
- not generic “shorter everywhere” policies

Examples:

Preserve more:

- failure-bearing tool results
- file-path-bearing results
- mutation summaries
- diagnostics the assistant later references

Compress harder:

- successful noisy stdout
- long repetitive listings
- large read outputs once identity/shape is known
- generic success confirmations

### Step 5 — Re-run the same task family

To avoid confounding, compare profile variants on the same task family first.
Only then expand to a broader suite.

## Direction this implies

### Short term

- keep `--slim` as the benchmark baseline
- use benchmark evidence to guide smaller signal-shaping changes
- prioritize smarter history retention over indiscriminate prompt shrinking

### Medium term

Move toward explicit signal layers:

- core signal
- situational signal
- residual signal

And explicit profiles:

- default Omegon
- slim Omegon
- possibly later: debug/build/design profiles

### Long term

The goal is not the smallest prompt.
The goal is the highest **useful signal density**.

## Should the benchmark harness move to a separate repo?

**Not yet.**

### Why not yet

Right now the harness is still tightly coupled to Omegon internals:

- `omegon --usage-json`
- `omegon_context` bucket semantics
- auth/provider behavior
- clean-room cargo target handling
- benchmark-mode CLI plumbing

These are not stable external interfaces yet.
Splitting now would add coordination overhead before the seams are mature.

### What should happen first

Keep the harness in-repo until:

1. adapter contracts stabilize
2. result schemas stop shifting every session
3. provider/auth behavior is predictable
4. at least one additional project or harness actually wants to consume it independently

### When a separate repo would make sense

A separate repo becomes justified when the harness is clearly an independent product with:

- stable adapter interfaces
- stable artifact schema
- multiple consuming projects
- low need to change Omegon core and benchmark harness in the same commit

Until then, keeping it in-repo is the better engineering trade.

## Recommendation

Use the benchmark harness as an **evidence engine** for signal shaping.

Do not treat it as merely a token scoreboard.

The right workflow is:

1. benchmark
2. inspect totals + buckets + logs
3. form a hypothesis about signal waste vs useful scaffolding
4. implement one targeted change
5. benchmark again
6. keep only changes that improve total efficiency without sacrificing verified task success

That is how Omegon should evolve from a high-capability harness into a harness with intentionally shaped signal profiles.
