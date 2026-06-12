+++
title = "Omegon Evaluator Suite Target"
tags = ["evaluation","benchmark","headroom","dogfood"]
+++

# Omegon Evaluator Suite Target

---
title: Omegon Evaluator Suite Target
status: exploring
tags: [evaluation, benchmark, headroom, dogfood]
---

# Omegon Evaluator Suite Target

## Intent

Define a future evaluator suite that can measure Omegon deterministically where possible across the full stack:

```text
low-level component correctness
  → feature/tool behavior
  → compression/context safety
  → harness task performance
  → model/provider comparison
  → release-readiness gates
```

This document is the pie-in-the-sky target. Current implementation should continue with localized native headroom evaluations while leaving clean interfaces for this broader evaluator work.

## Current strategy decision

The broad evaluator suite is future-release work. The immediate headroom workstream should not continue expanding `scripts/benchmark_harness.py` or adding high-level benchmark tasks. The benchmark harness remains the right eventual Tier 3+ integration point, but current effort returns to localized headroom evaluation and compressor correctness.

Current focus:

- `omegon-headroom` deterministic compressor behavior
- CCR storage/retrieval correctness
- protected-anchor extraction
- dogfood corpus breadth through `headroom-dogfood`/`headroom-eval`
- provider-boundary preparation for future learned compressors

Deferred until a later evaluator release:

- benchmark task specs for headroom A/B runs
- cross-harness/provider matrices
- report dashboards
- release gates spanning non-headroom subsystems

## Principles

- Deterministic first: prefer replayable fixtures, fixed inputs, local repos, seeded generators, and explicit acceptance commands.
- Separate correctness from effectiveness: a tool can be correct but not useful; a benchmark can improve tokens but harm task success.
- Measure deltas, not vibes: every feature flag should support baseline vs variant comparison.
- Preserve operator agency: expensive/network/model-backed evals are opt-in.
- Keep raw evidence: reports should link to logs, patches, usage JSON, fixtures, and failed acceptance output.
- Never hide restoration/repair: if the evaluator patches missing context back in, that is a measured dependency, not a success by itself.

## Evaluation tiers

### Tier 0 — Unit and property tests

Scope: single functions/types/crates.

Examples:

- compression no-growth guard
- CCR eviction order
- provider identity stability
- parser/cap-policy behavior
- tool schema normalization

Entry points:

```bash
cargo test -p omegon-headroom
cargo test -p omegon <filter>
```

Target properties:

- fast enough for every commit
- deterministic
- no network
- no external model

### Tier 1 — Fixture-level feature evals

Scope: one subsystem running against structured fixtures.

Current example:

```bash
just headroom-eval --text
just headroom-eval --text --fixtures .tmp/headroom/fixtures --max-restored-facts 0
```

Future examples:

- `context-eval`: prompt assembly, compaction, retrieval windows
- `tools-eval`: tool output caps, details schemas, retrieval handles
- `memory-eval`: recall precision over known fact sets
- `codescan-eval`: symbol/chunk retrieval over known repos

Report shape:

```json
{
  "suite": "headroom",
  "provider": { "id": "native_deterministic", "kind": "deterministic" },
  "fixtures": [...],
  "classes": [...],
  "metrics": {
    "passed": true,
    "estimated_tokens_before": 100000,
    "estimated_tokens_after": 25000,
    "restored_facts": 0,
    "latency_ms": 42
  }
}
```

### Tier 2 — Replay/reconstruction evals

Scope: deterministic replay of captured tool outputs or session fragments without live LLM decision-making.

Purpose:

- evaluate context compression against real historical payloads
- compare prompt assembly policies
- test retrieval behavior without spending model tokens
- detect regressions in what would be visible to the model

Inputs:

- redacted session traces
- tool-result JSONL
- captured `read`, `codebase_search`, `web_search`, `memory_recall` outputs
- expected visible facts per turn

Entry point target:

```bash
just evaluator-replay --trace .tmp/evals/traces/session.jsonl --variant headroom-on
```

Metrics:

- visible fact retention
- prompt estimated tokens
- retrieval handle validity
- compaction/compression decisions
- deterministic diff of prompt sections

### Tier 3 — Headless task benchmarks

Scope: run Omegon as an agent on real tasks, with acceptance commands.

Existing base:

```text
scripts/benchmark_harness.py
ai/benchmarks/tasks/*.yaml
docs/eval-harness-integration.md
```

This tier answers:

- Did the task still pass?
- Did input/output tokens improve?
- Did turns change?
- Did wall-clock change?
- Did retrieval/tool-call churn increase?
- Did the final patch/output quality change?

Target command:

```bash
python3 scripts/benchmark_harness.py ai/benchmarks/tasks/headroom-read-compression.yaml \
  --variant headroom-off

python3 scripts/benchmark_harness.py ai/benchmarks/tasks/headroom-read-compression.yaml \
  --variant headroom-on
```

Variant report:

```json
{
  "task_id": "headroom-read-compression",
  "variant": "headroom-on",
  "acceptance": { "required_passed": true },
  "usage": { "input_tokens": 90000, "output_tokens": 12000, "turns": 6 },
  "headroom": {
    "compressed_outputs": 3,
    "retrieval_calls": 1,
    "estimated_tokens_saved": 45000,
    "restored_facts": 0
  }
}
```

### Tier 4 — Matrix/provider evals

Scope: compare features across models, harness modes, providers, and local/remote environments.

Axes:

- harness: `omegon`, `om`, future alternatives
- model/provider: Claude/Gemini/Ollama/local
- feature flags: headroom off/manual/on, provider native/Kompressor
- context class/posture
- repo/task domain

Target:

```bash
just evaluator-matrix ai/benchmarks/suites/headroom.toml
```

Output:

- per-cell result JSON
- comparison markdown
- regression summary
- release gate status

### Tier 5 — Release gates

Scope: curated checks that block releases or default-on feature flips.

For headroom default-on, required gates could be:

- low-level canonical/adversarial fixtures pass
- dogfood corpus passes with `--max-restored-facts 0`
- benchmark A/B acceptance does not regress
- median input-token savings exceeds threshold on selected tasks
- retrieval-call increase is below threshold
- latency overhead is below threshold
- no provider/model artifact missing in default profile

## Data and corpus strategy

### Committed fixtures

Small, redacted, deterministic examples that represent known bug classes.

Use for:

- CI
- regression tests
- fast local validation

### Generated local fixtures

Ignored `.tmp/` data from local commands, repos, docs, and operator-owned corpora.

Use for:

- dogfood
- exploratory tuning
- before/after local comparisons

### Cached public corpora

Public repos and public-domain documents cloned/downloaded under ignored cache directories.

Use for:

- broader coverage
- optional scheduled evals
- release-hardening checks

### Sensitive/private corpora

Operator-provided data never committed; fixture generator should support redaction/minimization before promotion.

## Future release path for evaluator work

### Release N: localized headroom eval hardening

Current workstream.

Deliverables:

- headroom fixture evaluator
- dogfood generator
- restoration gate
- provider identity
- bounded CCR store
- docs/design plan

Do not attempt full evaluator suite yet.

### Release N+1: evaluator substrate

Deliverables:

- common result schema for eval reports
- suite/fixture manifest format
- artifact directory convention
- baseline/compare support shared across evals
- benchmark harness variant support
- initial `headroom-read-compression` benchmark task

### Release N+2: replay and corpus expansion

Deliverables:

- captured tool-output replay format
- context/prompt replay evaluator
- public corpus downloader/generator recipes
- redaction/minimization helper
- committed small regression fixture pack

### Release N+3: provider/matrix eval

Deliverables:

- compression provider trait and provider selection
- provider comparison in `headroom-eval`
- Kompressor/Ollama optional provider gate if ready
- evaluator matrix runner
- dashboard/report generation

### Release N+4: release gates/default-on policy

Deliverables:

- curated release gate suites
- thresholds for headroom default-on expansion
- CI/nightly split between fast and expensive evals
- historical trend reports

## Current action: stay localized

For now, continue with native headroom local evals:

```bash
just headroom-dogfood
just headroom-eval --text --fixtures .tmp/headroom/fixtures --max-restored-facts 0
cargo test -p omegon-headroom
cargo test -p omegon headroom --quiet
cargo check -p omegon --quiet
```

The broader evaluator should be designed as a future release workstream. Headroom should provide the first concrete suite and report schema pressure, not become the entire evaluator architecture by itself.
