+++
id = "ae9fe9da-b3b0-44f5-a4df-12dbd060d605"
kind = "document"
title = "RC.64 Benchmark Finding — Shadow Context Harness Comparison (Cache-Aware)"
status = "active"
tags = ["benchmark", "release-candidate", "rc64", "token-efficiency", "performance", "cache-accounting"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
date = "2026-04-10"
+++

# RC.64 Benchmark Finding — Shadow Context Harness Comparison (Cache-Aware)

## Task

Benchmark task: `example-shadow-context`

- prompt: `Finish the shadow-context assembly wiring.`
- acceptance: `cargo test -p omegon shadow_context --manifest-path core/Cargo.toml`
- clean-room benchmark harness: `scripts/benchmark_harness.py`

## Compared harnesses

- `pi` — minimal baseline
- `claude-code`
- `omegon` — default harness
- `om` — `omegon --slim`

## Matrix limitation

This `rc.64` finding is still primarily a **harness/profile comparison**, not a full provider/model matrix.

That limitation matters. Subsequent execution-pressure investigation showed that provider temperament differs materially:
- `openai-codex:gpt-5.4` exhibits more orientation/inspection churn on this task than the Anthropic/Sonnet reference line.
- A harness fix that looks good against Anthropic alone can therefore underfit or overfit when evaluated against Codex.

So `rc.64` remains the right cache-aware baseline, but it should no longer be treated as sufficient evidence for cross-provider harness quality on its own.

## Finding

`0.15.10-rc.64` is the first cache-aware benchmark set where result artifacts explicitly count both cache reads and cache writes. This makes the benchmark more honest, but it does **not** change the competitive conclusion: `pi` remains the decisive baseline, `claude-code` still beats both Omegon variants, and `om` no longer beats default `omegon` on wall clock.

### Measured results

| Harness | Status | Wall clock (s) | Input | Output | Cache | Cache write | Total tokens |
|---|---:|---:|---:|---:|---:|---:|---:|
| `pi` | pass | `430.746` | `1` | `383` | `67,495` | `1,173` | `69,052` |
| `claude-code` | pass | `726.315` | `34` | `31,621` | `1,801,843` | `86,884` | `1,920,382` |
| `omegon` | pass | `859.002` | `1,955,547` | `9,176` | `396,998` | `8,102` | `2,369,823` |
| `om` | pass | `898.707` | `2,044,983` | `10,943` | `201,056` | `10,584` | `2,267,566` |

## Time assessment

- `pi` is the fastest run at `430.746s`.
- `claude-code` trails `pi` by `295.569s`.
- default `omegon` trails `pi` by `428.256s`.
- `om` is the slowest run in this set at `898.707s`, trailing `pi` by `467.961s`.
- `om` is **slower** than default `omegon` by `39.705s`.

## Token assessment

- `pi` is again the clear efficiency baseline at `69,052` total tokens.
- `claude-code` uses `1,920,382` total tokens — about **27.8×** the token burn of `pi`.
- default `omegon` uses `2,369,823` total tokens — about **34.3×** the token burn of `pi`.
- `om` uses `2,267,566` total tokens — about **32.8×** the token burn of `pi`.
- `om` is cheaper than default `omegon` by `102,257` tokens (~4.3%), but that reduction is much smaller than the `rc.63` gap.

## Cache-accounting assessment

This is the important `rc.64` improvement:

- `pi`: `cache=67,495`, `cache_write=1,173`
- `claude-code`: `cache=1,801,843`, `cache_write=86,884`
- `omegon`: `cache=396,998`, `cache_write=8,102`
- `om`: `cache=201,056`, `cache_write=10,584`

The benchmark artifacts now explicitly capture both cache reads and cache writes. `rc.64` therefore fixes the benchmark truthfulness problem that existed before full cache-token propagation.

## Comparison against RC.63

Reference: [[benchmark-finding-rc63-shadow-context]]

### What improved

- `rc.64` artifacts are more honest: cache writes are now counted explicitly.
- `pi` improved materially from `158,293` total tokens in `rc.63` to `69,052` in `rc.64`.
- `claude-code` improved from `2,743,879` total tokens in the `rc.63` cache-blind comparison run to `1,920,382` in `rc.64`.
- default `omegon` improved from `2,953,216` to `2,369,823` total tokens.

### What did not improve

- `om` did **not** remain the fastest Omegon profile. In `rc.63`, `om` beat default `omegon` on wall clock; in `rc.64`, it is slower.
- `om` is still massively more expensive than `pi`.
- neither Omegon profile is competitive with `pi` on token efficiency.

## Interpretation

`rc.64` solved a **measurement** problem, not the underlying efficiency problem.

That matters: this benchmark set is now more trustworthy than `rc.63` because it no longer hides cache-write accounting. But the new truth is still uncomfortable:

1. **`pi` remains the real baseline.**
   It wins on both speed and token economy.

2. **`claude-code` remains stronger than Omegon.**
   It beats both `omegon` and `om` on both wall clock and total tokens.

3. **`om` is no longer clearly the preferred Omegon profile for this task.**
   It is slightly cheaper than default `omegon`, but slower.

## Decision

Treat `rc.64` as the **cache-accounting correctness release candidate**, not as the efficiency win release candidate.

- `rc.64` establishes the first trustworthy cache-aware benchmark baseline.
- follow-on work should target reducing Omegon's underlying orchestration/context overhead.
- `pi` should remain in the benchmark set as the lean baseline.
- `om` should remain in the benchmark set, but it should not be assumed to be the faster profile without measurement.

## Next target

The next release candidate after `rc.64` should target **actual efficiency improvements**, not more accounting fixes.

Priority directions:

- reduce Omegon's prompt/input inflation relative to `pi`
- investigate why `om` lost the wall-clock advantage on this task
- reduce repeated context reconstruction and oversized system/tool overhead
- expand the benchmark matrix to include provider/model temperament, starting with:
  - `anthropic:claude-sonnet-4-6`
  - `openai-codex:gpt-5.4`
- continue using the cache-aware benchmark matrix as the release gate

## Artifacts

- `ai/benchmarks/runs/2026-04-10T16-48-33Z-example-shadow-context-pi.json`
- `ai/benchmarks/runs/2026-04-10T16-53-28Z-example-shadow-context-claude-code.json`
- `ai/benchmarks/runs/2026-04-10T16-55-41Z-example-shadow-context-omegon.json`
- `ai/benchmarks/runs/2026-04-10T16-56-22Z-example-shadow-context-om.json`
