---
title: Headroom Benchmark Tasks
status: seed
tags: [headroom, evaluation, benchmark]
---

# Headroom Benchmark Tasks

These tasks are the first high-level benchmark layer for native headroom compression. They are deliberately separate from `headroom-eval`:

- `headroom-eval` checks low-level compressor behavior against fixtures.
- `scripts/benchmark_harness.py` checks whether an agent can still complete tasks while headroom evidence is attached to the benchmark artifact.

## Prerequisite fixture generation

Generate local ignored fixtures before running the tasks:

```bash
rm -rf .tmp/headroom/fixtures .tmp/headroom/generated-corpus.json .tmp/headroom/sources
just headroom-corpus-collect docs/evals/headroom-corpus-large.example.json
python3 scripts/headroom_dogfood.py .tmp/headroom/generated-corpus.json
just headroom-eval --text --fixtures .tmp/headroom/fixtures --max-restored-facts 0
```

The `.tmp/headroom` directory is non-authoritative and must not be committed.

## Task set

Current task specs:

- `ai/benchmarks/tasks/headroom-read-large-rust.yaml`
- `ai/benchmarks/tasks/headroom-read-large-markdown.yaml`
- `ai/benchmarks/tasks/headroom-diff-investigation.yaml`
- `ai/benchmarks/tasks/headroom-json-sparse-critical.yaml`
- `ai/benchmarks/tasks/headroom-retrieve-original.yaml`

Each task includes:

```yaml
headroom:
  run_eval: true
  eval_fixtures: .tmp/headroom/fixtures
  max_restored_facts: 0
  token_counter: bytes_div_4
```

The benchmark harness attaches a low-level `headroom-eval` JSON artifact to each result. That artifact is evidence about the fixture corpus, not proof that the specific agent task succeeded.

Pack-level evaluation now runs both `headroom-eval` and `headroom-understanding-eval`:

```bash
just headroom-pack-eval evals/headroom/packs/core/omegon
just headroom-pack-eval-strict evals/headroom/packs/core/omegon
```

As of the current native-headroom workstream, the strict core pack passes with 96% evaluated savings, zero restored facts, and 103/103 understanding questions passing. This is the default-on readiness gate for the fixture corpus; high-level agent benchmark tasks still need to prove task success under `headroom.mode=on`.

## Suggested execution

Run one task at a time while this surface is new:

```bash
python3 scripts/benchmark_harness.py ai/benchmarks/tasks/headroom-read-large-rust.yaml --harness omegon
python3 scripts/benchmark_harness.py ai/benchmarks/tasks/headroom-retrieve-original.yaml --harness omegon
```

Do not expand to a full cross-harness matrix until single-task Omegon runs are stable and the result artifacts show useful headroom telemetry.

## Next A/B task matrix

Next session should broaden live Omegon A/B evidence beyond the sparse-critical JSON sentinel. Run these shapes in order, keeping each task as an off/on pair with `--headroom-ab-real` and recording provider-token totals, task status, headroom eval status, restored fact count, and any retrieval behavior:

1. `headroom-log-failure-diagnosis` — long failing build/test log; answer must name the failing test/error line and relevant source path.
2. `headroom-diff-risk-review` — large patch review; answer must name changed files/functions and at least one seeded behavioral or security risk.
3. `headroom-retrieval-required-original` — compressed output intentionally omits an original-only detail; answer must use CCR retrieval and include that detail.
4. `headroom-markdown-decision-recall` — long design doc; answer must cite a decision, rejected alternative, and constraint.
5. `headroom-code-api-explain` — large Rust module; answer must explain a target function contract, edge case, and relevant test/call site.
6. `headroom-debug-from-log-to-code` — failure log plus source inspection; answer must connect symptom to source mechanism and, where fixture-safe, propose or apply the minimal fix.

Priority rationale: JSON already has a positive real A/B result. Log and diff are in the current default auto-kind candidate set, so they need live evidence first. Markdown, code, and broad plain text show strong corpus savings, but should remain gated until live task evidence proves no reasoning degradation.

## Acceptance posture

These benchmarks are evidence-gathering tasks. Native headroom remains default-off until benchmark results show that `headroom.mode=on` preserves task success while reducing context pressure.

Default-on consideration requires at least:

- strict core pack eval passes with `strict_max_restored_facts = 0`
- strict core pack understanding eval passes with zero failed questions
- high-level tasks pass with headroom enabled
- retrieval handles resolve correctly during agent operation
- token/context telemetry improves or remains neutral
- no increase in task failures attributable to compressed context
