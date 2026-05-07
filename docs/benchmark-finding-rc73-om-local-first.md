+++
id = "ef9a7319-d6d5-4d83-8ecb-55b86abdaff9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Benchmark finding — rc.73 om local-first doctrine

## Context

After `v0.15.10-rc.73`, `om` was hardened with a local-first, patch-or-prove doctrine on top of the shared evidence-sufficiency convergence controller. The goal was to determine whether `om` could become a useful low-cost mode for bounded local work instead of behaving like a diluted but still expensive Omegon.

The key commits in this sequence were:
- `eec700c7` — shared evidence sufficiency / forced convergence
- `ba737ea2` — tighten post-sufficiency convergence pressure
- `vmxrqtvxwsrv` — `fix(om): enforce local-first patch-or-prove behavior`

Superseded intermediate benchmark artifacts from before the `om` local-first hardening were archived to `ai/benchmarks/archive/superseded-2026-04-11/` so the active run set reflects current behavior.

## Evaluated tasks

Two light benchmark rungs were used for the post-fix comparison:
1. `controller-research-repair`
2. `remote-logout-robustness`

Both were run against `omegon` and `om`.

## Results

### 1. controller-research-repair

Current matrix artifact:
- `ai/benchmarks/runs/matrix-2026-04-11T21-02-19Z-controller-research-repair.json`

| Harness | Outcome | Efficiency | Discipline | Turns | Tokens | Wall |
|---|---:|---:|---:|---:|---:|---:|
| `omegon` | pass | fail | fail | 22 | 964,524 | 359.2s |
| `om` | pass | pass | warn | 7 | 266,753 | 180.1s |

### 2. remote-logout-robustness

Current matrix artifact:
- `ai/benchmarks/runs/matrix-2026-04-11T21-11-54Z-remote-logout-robustness.json`

| Harness | Outcome | Efficiency | Discipline | Turns | Tokens | Wall |
|---|---:|---:|---:|---:|---:|---:|
| `omegon` | pass | pass | fail | 18 | 831,949 | 288.9s |
| `om` | fail | pass | warn | 15 | 546,934 | 281.5s |

## Interpretation

### `om` is now materially useful on bounded local work

On `controller-research-repair`, `om` now clearly outperforms full `omegon`:
- far fewer turns
- far fewer tokens
- much lower wall clock
- still successful

This is strong evidence that the local-first patch-or-prove doctrine fixed the previous `om` failure mode on research-heavy local repair.

### `om` still should not own the richer runtime-contract rung

On `remote-logout-robustness`, `om` became cheaper and cleaner but still failed the outcome. Full `omegon` still owns the harder runtime-contract task.

This is a healthy boundary:
- `om` = lightweight local scout
- `omegon` = broader runtime-contract / systems harness

### Full `omegon` remains too expensive on bounded local work

The same data also shows a new problem plainly: full `omegon` is still too unconstrained on the local controller rung. It solved the task, but with a 22-turn / 964k-token process that is hard to justify against `om`'s 7-turn / 266k-token result.

## Decision

Keep the `om` local-first doctrine.

The profile split is now meaningful enough to preserve:
- `om` is no longer just a smaller Omegon
- it can credibly own cheap local repair / research tasks
- `omegon` still owns richer, broader engineering tasks

## Next work

Do **not** retune `om` immediately.

The higher-value next move is to reduce full `omegon` waste on bounded local tasks without losing its broader capability. The most likely ROI path is to make full `omegon` more decisive once a bounded local task has a clear target and enough evidence, while preserving its stronger systems framing for genuinely broader work.
