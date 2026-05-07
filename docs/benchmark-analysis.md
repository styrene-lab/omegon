+++
id = "39ae8896-b7e3-42ac-876c-08b6393702ea"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Benchmark analysis — comparison, regression detection, emergent signal

## Overview

Given N results artifacts, produce actionable analysis. Three tiers: (1) Regression detection — did this RC break something that the previous RC passed? (2) Configuration comparison — which model/thinking/persona combo produces the best results for a given cost? (3) Emergent signal — correlations across the matrix that weren't hypothesized. Could be a CLI report, a web dashboard, or an agent-driven analysis session where omegon analyzes its own benchmark results.

A benchmark matrix must not collapse provider/model temperament into a single "LLM" bucket. The same harness can fail differently on different model families:
- Anthropic/Sonnet may be the stable parity baseline.
- Codex/GPT‑5.4 may expose higher orientation churn or delayed execution.

So analysis needs at least two orthogonal axes:
- **harness/profile axis** — `pi`, `claude-code`, `omegon`, `om`
- **provider/model axis** — e.g. `anthropic:claude-sonnet-4-6`, `openai-codex:gpt-5.4`

The practical question is no longer just "which harness wins?" but also:
- which failures are harness-level across providers?
- which fixes generalize?
- which fixes merely overfit one provider's behavior?

## Open Questions

*No open questions.*
