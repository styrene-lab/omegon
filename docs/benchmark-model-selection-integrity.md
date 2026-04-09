---
id: benchmark-model-selection-integrity
title: "Benchmark model selection integrity — explicit model pinning vs profile override drift"
status: exploring
tags: [benchmark, routing, model-selection, blocking, p1]
open_questions:
  - "What is the authoritative model-selection precedence for headless and benchmark runs: explicit CLI `--model`, benchmark task config, repo profile, shared settings defaults, or provider auto-detect fallback?"
  - "Where exactly is the requested model being overwritten from `anthropic:claude-sonnet-4-6` to `gpt-4.1`: benchmark adapter argv construction, `settings::shared(&cli.model)`, `Profile::load/apply_to`, or a later runtime normalization step?"
  - "How should the system fail when the effective model differs from the requested benchmark model: hard error before provider call, explicit warning with both values, or benchmark artifact metadata containing requested vs effective model?"
dependencies: []
related: []
issue_type: bug
priority: 1
---

# Benchmark model selection integrity — explicit model pinning vs profile override drift

## Overview

Benchmark and headless agent runs can silently execute a different model than the operator or harness intended because repo/profile settings are applied after initial CLI/model selection. Observed blocker: a clean-room benchmark intended for Anthropic/Claude drifted to `gpt-4.1`, then routed through Anthropic and failed with `Anthropic 404 Not Found: model: gpt-4.1`. This blocks benchmark validity because results can be attributed to the wrong model or fail non-obviously.

## Research

### Observed evidence

Clean-room benchmark run wrote an Omegon artifact with zero usage because the provider call failed. The underlying log shows `Error: LLM error: Anthropic 404 Not Found: model: gpt-4.1`, even though the intended benchmark target was Anthropic/Claude. `run_agent_command()` in `core/crates/omegon/src/main.rs` constructs shared settings from `cli.model`, then loads `Profile::load(&cli.cwd)` and applies it via `profile.apply_to(&mut s)`, which is a plausible override point. This issue now blocks all benchmark comparisons because model attribution is untrustworthy until explicit selection integrity is enforced.

## Open Questions

- What is the authoritative model-selection precedence for headless and benchmark runs: explicit CLI `--model`, benchmark task config, repo profile, shared settings defaults, or provider auto-detect fallback?
- Where exactly is the requested model being overwritten from `anthropic:claude-sonnet-4-6` to `gpt-4.1`: benchmark adapter argv construction, `settings::shared(&cli.model)`, `Profile::load/apply_to`, or a later runtime normalization step?
- How should the system fail when the effective model differs from the requested benchmark model: hard error before provider call, explicit warning with both values, or benchmark artifact metadata containing requested vs effective model?
