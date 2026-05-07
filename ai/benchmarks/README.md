+++
id = "b9fb7fdc-07b9-4136-af47-4d3d5ecef8f4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Benchmarks

This directory contains the **active local benchmark program** for Omegon.

## Layout

- `tasks/` — benchmark task specs
- `runs/` — current active local result artifacts
- `archive/` — archived local result artifacts from previous benchmark eras/resets
- `examples/` — reference/example payloads

## Policy

### `runs/` is the active comparison set

Treat `ai/benchmarks/runs/` as the working set for the **current** benchmark era.
It should stay intentionally small and relevant to current harness decisions.

Do **not** let it become a junk drawer of every historical experiment.

### Historical findings belong in `docs/`

Long-lived interpretation belongs in documents such as:
- `docs/benchmark-finding-rc63-shadow-context.md`
- `docs/benchmark-finding-rc64-shadow-context.md`
- `docs/benchmark-redesign-task-and-eval-spec.md`

Those documents are the historical record.
The JSON run artifacts are local evidence, not the canonical narrative.

### Archive old run artifacts before starting a fresh matrix

When the benchmark contract changes materially — task schema, artifact schema, provider/model matrix, process metrics, or harness heuristics — archive the current `runs/` set and start fresh.

Use:

```bash
python3 scripts/benchmark_reset.py
```

That will:
- move `ai/benchmarks/runs/*.json` into `ai/benchmarks/archive/<timestamp>/`
- write `manifest.json` for the archived batch
- recreate an empty active `runs/` directory

## Why archive instead of keeping everything active?

Because stale runs distort comparisons.

The benchmark redesign introduced new dimensions that older runs often do not reflect cleanly:
- richer task semantics
- structured acceptance
- process metrics
- provider/model temperament
- harness anti-churn heuristics

Old runs are still useful as local forensic evidence, but they should not remain in the active matrix once the benchmark contract changes.
