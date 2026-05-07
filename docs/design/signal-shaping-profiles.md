+++
id = "5924bafd-02f9-4a13-a5f8-3262dec0f46c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Signal Shaping Profiles

## Problem

`omegon --slim` proved that a thinner harness surface can materially improve token efficiency on routine coding tasks.

Measured on `example-shadow-context`:

- **Omegon default**: 2,650,542 tokens
- **Omegon slim**: 895,731 tokens
- **Claude Code**: 930,344 tokens

This establishes two facts:

1. **Default Omegon carries real overhead** in routine coding scenarios.
2. **Slim Omegon is a fair comparison profile** for mainline CLI coding agents.

However, a follow-up experiment with more aggressive slim history compression reduced the `hist` snapshot bucket while making total task tokens *worse*. That means the problem is not simply “make everything smaller”.

The real problem is:

> How do we ensure the model sees the right information, at the right fidelity, for the current task class?

This is a **signal-shaping** problem, not just a token-minimization problem.

## Evidence

### Valid baseline runs

`example-shadow-context`:

- **Default Omegon**
  - total tokens: 2,650,542
  - wall clock: 490.106s
- **Slim Omegon**
  - total tokens: 895,731
  - wall clock: 264.766s
  - `omegon_context` snapshot:
    - `sys`: 19,699
    - `tools`: 2,243
    - `conv`: 568
    - `mem`: 0
    - `hist`: 23,034
    - `think`: 318
- **Claude Code**
  - total tokens: 930,344
  - wall clock: 547.51s

### What the snapshot says

The remaining slim overhead is dominated by:

1. `hist` — preserved tool/result working history
2. `sys` — always-on system instructions
3. `tools` — tool schema overhead

### What the failed follow-up says

A more aggressive history compression pass reduced `hist` in the latest-turn snapshot but caused total tokens to rise sharply. That implies some history is **productive working memory**, not waste.

Conclusion:

- Some context is pure overhead
- Some context prevents expensive rediscovery
- The system must distinguish between the two

## Design goal

Replace the crude “full vs slim” mental model with **signal profiles**:

- baseline coding profile
- richer systems-engineering profile
- potentially additional task-shaped profiles later

The design target is:

> Minimize wasted tokens without removing high-signal working context.

## Profile model

### 1. Core signal

Always useful:

- current user prompt
- current working directory and repo
- active model and context window constraints
- core system behavior rules
- immediate recent working history
- currently available tool schema

This is the irreducible minimum.

### 2. Situational signal

Useful only when the task needs it:

- lifecycle/design tree
- openspec/spec workflow
- memory retrieval
- web search
- local inference
- codebase indexing
- delegation / subagents

This should be activated by:

- explicit operator request
- task classification
- observed failure mode
- repo structure / artifact presence

### 3. Residual signal

Potentially useful, often bloated:

- old raw tool outputs
- stale assistant reasoning
- long test logs after the key diagnosis is known
- repeated directory listings / file dumps
- repeated global directive text with no effect on the current task

This is where the main token waste lives.

## Product profiles

### Default Omegon

Purpose:

- maximum systems-engineering harness behavior
- richer lifecycle/spec/memory affordances
- best suited for design-heavy or process-heavy sessions

Tradeoff:

- more token overhead
- more prompt/tool surface

### `--slim`

Purpose:

- coding-first, fair-comparison mode
- benchmark baseline versus Claude Code / Codex / pi-class CLI agents
- fast interactive mode for routine work

Tradeoff:

- reduced systems-engineering scaffolding
- should still remain competent for normal edit/test/debug loops

### Future profiles

Possible later expansion:

- `--build` / `--debug`: preserve compile/test failure context more aggressively
- `--design`: enable lifecycle/spec/memory by default
- `--investigate`: preserve traces/history more aggressively than slim

These should only be added if the benchmark matrix shows that task-shaped profiles outperform one-size-fits-all slim/default behavior.

## Implementation direction

### A. Prompt overlays, not monoliths

Split the system prompt into independently activatable overlays:

- core
- coding
- lifecycle
- memory
- search
- delegation
- local inference

Profiles become bundles of overlays instead of one giant always-on prompt.

**Directive:** `--slim` should load only the core + coding overlays by default.

### B. Importance-weighted history retention

Do **not** compress all history uniformly.

Instead classify tool history by utility:

#### High-signal history

Preserve more aggressively:

- failed `bash`/test/compiler output
- changed file references
- file paths later referenced by assistant messages
- build or test commands that guided subsequent action
- key stderr diagnostics

#### Low-signal history

Compress aggressively:

- successful noisy stdout
- long listings
- repeated file dumps after path/shape are known
- generic success confirmations

**Directive:** move from “shorter decay skeletons” to **tool-aware summaries**.

### C. Recovery-aware summaries

Every decayed tool result should preserve enough state to avoid rediscovery.

Minimum useful retained structure for old tool results:

- tool name
- args summary / file path / target
- exit status
- one or two salient lines
- for compilation/test failures: the primary failing diagnostic

If a summary does not preserve the information needed to continue acting, it is not a summary — it is deletion.

### D. Benchmark by task family

Do not judge profiles by a single task.

Minimum benchmark families:

1. routine code edit / test loop
2. compile-fix loop
3. repo forensics / debugging
4. design/spec-heavy task

Then compare:

- default Omegon
- slim Omegon
- future smart-history slim
- Claude Code

## Success metrics

For `--slim` and successor profiles, track:

- pass/fail
- total tokens
- wall clock
- turn count
- latest-turn context composition (`sys/tools/conv/mem/hist/think`)
- repeated-read / repeated-bash indicators if added later

Primary goal:

- lower total tokens **without** lowering pass rate

Secondary goal:

- reduce `hist` and `sys` without causing compensating token growth in `conv` or total turns

## Immediate next work

### 1. Keep current `--slim` as the comparison baseline

Use the validated `895,731` token pass as the current benchmark reference.

### 2. Rework slim history compression

Replace the current overly aggressive generic shortening with:

- tool-aware summaries
- stronger preservation for high-signal failures and path references
- stronger compression only for low-signal success noise

### 3. Modularize prompt loading

Make `--slim` a first-class profile bundle instead of a couple of if-statements.

### 4. Expand benchmark suite

Add a small set of task-family exemplars before making broad product claims.

## Recommendation

Treat `omegon --slim` as the **de-facto comparison profile** for external CLI-agent benchmarking.

Treat default Omegon as the **premium harness profile** where higher token cost is sometimes justified by higher-quality systems-engineering assistance.

The road forward is not “always smaller”.
It is:

> smaller where the signal is waste, fuller where the signal prevents rediscovery.
