+++
id = "c3340cf5-42ec-4b4f-b470-f0e7c62a699a"
kind = "document"
title = "RC.63 Benchmark Finding — Shadow Context Harness Comparison"
status = "active"
tags = ["benchmark", "release-candidate", "rc63", "rc64", "token-efficiency", "performance"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
date = "2026-04-10"
+++

# RC.63 Benchmark Finding — Shadow Context Harness Comparison

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

## Finding

For the `example-shadow-context` benchmark on `0.15.10-rc.63`, `om` is the fastest completed harness, but `pi` is the decisive token-efficiency baseline.

### Measured results

| Harness | Status | Wall clock (s) | Total tokens |
|---|---:|---:|---:|
| `pi` | pass | `891.493` | `158,293` |
| `om` | pass | `878.842` | `2,198,674` |
| `claude-code` | pass | `930.943` | `2,743,879` |
| `omegon` | pass | `1018.974` | `2,953,216` |

### Time assessment

- `om` is the fastest run at `878.842s`.
- `pi` is effectively tied on latency at `891.493s`, only `12.651s` slower than `om`.
- `claude-code` is slower than `om` by `52.101s`.
- default `omegon` is the slowest run at `1018.974s`, `140.132s` slower than `om`.

### Token assessment

- `pi` is the clear efficiency baseline at `158,293` total tokens.
- `om` uses `2,198,674` total tokens — **~13.9×** the token burn of `pi`.
- default `omegon` uses `2,953,216` total tokens — **~18.7×** the token burn of `pi`.
- `om` is materially better than default `omegon`, saving `754,542` tokens (~25.6% reduction), but it remains massively more expensive than `pi`.

## Interpretation

This benchmark says two important things at once:

1. **Slim mode is directionally correct.**
   `om` beats default `omegon` on both wall clock and total token burn.

2. **Omegon is still not competitive on token efficiency.**
   Even after the slim-mode reduction, `om` remains catastrophically more expensive than `pi` on the same task.

The current bottleneck is not just raw latency. The stronger signal is unnecessary token spend inside Omegon's runtime shape.

## RC.64 target

`0.15.10-rc.64` should target **Omegon efficiency improvements** using this benchmark as the gating comparison.

### Primary objective

Reduce `omegon` and especially `om` token burn against the `pi` baseline without regressing pass rate.

### Success conditions for RC.64

At minimum:

- preserve passing behavior on `example-shadow-context`
- preserve or improve `om` wall-clock performance
- reduce `om` total tokens materially from the current `2,198,674`
- reduce default `omegon` total tokens materially from the current `2,953,216`

### Priority direction

Investigate and reduce Omegon-specific overhead from:

- oversized tool-schema/context injection
- repeated history/context reconstruction
- excess internal orchestration turns
- benchmark/runtime behavior that causes large prompt re-issuance relative to `pi`

## Decision

Treat this benchmark as an RC gate finding:

- `rc.63` establishes the current baseline
- `rc.64` work should focus on efficiency improvements in Omegon
- `pi` should remain in the comparison set as the lean baseline
- `om` should remain in the comparison set as the intended fast-path Omegon profile

## Artifacts

- `ai/benchmarks/runs/2026-04-10T15-49-52Z-example-shadow-context-pi.json`
- `ai/benchmarks/runs/2026-04-10T15-36-34Z-example-shadow-context-claude-code.json`
- `ai/benchmarks/runs/2026-04-10T15-41-24Z-example-shadow-context-omegon.json`
- `ai/benchmarks/runs/2026-04-10T15-39-05Z-example-shadow-context-omegon.json` ← runtime corresponds to `om` (`omegon --slim`) even though the artifact predates the `om` filename/label fix
