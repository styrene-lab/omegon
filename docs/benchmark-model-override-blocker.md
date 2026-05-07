+++
id = "30311a6a-8ea0-48f8-9f16-4414146a8acb"
kind = "document"
title = "Benchmark harness model override blocks Omegon token-comparison runs"
status = "exploring"
tags = []
aliases = ["benchmark-model-override-blocker"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
issue_type = "bug"
open_questions = ["What exact configuration precedence caused the benchmark run to ignore the intended model and select `gpt-4.1` instead — CLI defaulting, benchmark adapter omission, repo profile override via `Profile::load(...).apply_to(...)`, or another settings layer?", "What is the deterministic contract for benchmark model selection across harnesses: must the adapter always pass an explicit model argument and reject ambiguous defaulting?", "What regression proof is required to prevent recurrence: an integration test showing clean-room Omegon benchmark runs cannot be hijacked by repo profile settings when a benchmark model is specified?"]
priority = "1"
related = []
+++

# Benchmark harness model override blocks Omegon token-comparison runs

## Overview

Benchmark runs against Omegon can silently ignore the intended benchmark model and fall back to repo/profile-configured defaults. In the observed failure, a clean-room Omegon benchmark intended for Anthropic/Claude instead attempted model `gpt-4.1` via Anthropic and failed with `Anthropic 404 Not Found: model: gpt-4.1`. This blocks all token-accounting and cross-harness comparison work because the benchmark is not executing the requested model deterministically.

## Open Questions

- What exact configuration precedence caused the benchmark run to ignore the intended model and select `gpt-4.1` instead — CLI defaulting, benchmark adapter omission, repo profile override via `Profile::load(...).apply_to(...)`, or another settings layer?
- What is the deterministic contract for benchmark model selection across harnesses: must the adapter always pass an explicit model argument and reject ambiguous defaulting?
- What regression proof is required to prevent recurrence: an integration test showing clean-room Omegon benchmark runs cannot be hijacked by repo profile settings when a benchmark model is specified?
