+++
id = "9b031e96-9f04-4f4e-8dd7-3c3fb7490f55"
kind = "design_node"
title = "Dual-LLM model routing — prefilter classification for cost-optimized sentry execution"
status = "decided"
tags = ["sentry", "cost", "model-routing", "llm", "optimization"]
aliases = ["dual-llm-model-routing"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "omega"
priority = "2"
related = ["autonomous-tasking"]
+++

# Dual-LLM model routing — prefilter classification for cost-optimized sentry execution

## Problem

Sentry tasks use a single configured model for everything. A complex PR review and a trivial "check if CI passed" task both run on the same (typically expensive) model. Delfhos addresses this with a dual-LLM architecture: a cheap `light_llm` for prefiltering/classification and an expensive `heavy_llm` for actual work. This halves cost on high-volume autonomous workloads where many tasks are simple.

## Design

### Model routing table

Add a routing config to sentry that maps task complexity to models:

```toml
# sentry.toml
[sentry]
max_concurrent = 2

[sentry.routing]
prefilter_model = "anthropic:claude-haiku-4-5-20251001"
light_model = "anthropic:claude-sonnet-4-6"
heavy_model = "anthropic:claude-opus-4-6"

# Classification thresholds (prefilter output)
light_threshold = 0.7    # confidence above this → use light_model
```

Or per-task:
```toml
[[task]]
name = "ci-check"
prompt = "Check if CI passed and report"
model = "auto"          # triggers routing
```

Or in task tree:
```markdown
+++
id = "ci-check"
title = "Check CI"

[execution]
model = "auto"
+++
```

### Prefilter call

When `model = "auto"`, before spawning the full agent loop:

1. Send a single-turn classification prompt to `prefilter_model` (Haiku):

```
Given this task, classify its complexity:
- SIMPLE: single-step check, yes/no answer, status lookup
- MODERATE: multi-step but well-defined, standard review, documentation
- COMPLEX: open-ended analysis, architectural decisions, multi-file changes

Task: "{task_prompt}"

Respond with exactly one word: SIMPLE, MODERATE, or COMPLEX.
```

2. Map the classification to a model:
   - SIMPLE → `light_model`
   - MODERATE → `light_model`
   - COMPLEX → `heavy_model`

3. Spawn the agent loop with the selected model.

The prefilter call costs ~200 input tokens + 1 output token on Haiku (~$0.0001). Even if the routing saves only 10% of tasks from using Opus→Sonnet, it pays for itself within 2 tasks.

### Fallback and override

- If prefilter fails (timeout, error), default to `heavy_model` (safe fallback)
- Per-task `model = "anthropic:claude-sonnet-4-6"` overrides routing entirely
- `model = "auto"` is opt-in; existing configs with explicit models are unchanged
- Routing config is optional; if `[sentry.routing]` is absent, `model = "auto"` resolves to the CLI `--model` flag

### Cost tracking

The prefilter call's token usage is tracked in the budget system under the task's budget. This ensures the classification cost is visible and counts toward daily limits.

### Interactive mode

For interactive (non-sentry) sessions, routing is less valuable since the user controls the conversation. However, a similar mechanism could be used for auto-model-switching within a session:

- Start on light model
- If the agent detects it's struggling (repeated failures, user corrections), suggest or auto-escalate to heavy model
- This is a future extension, not in scope for this design

## Scope

### Phase 1: Sentry prefilter routing (~150 lines)
- `[sentry.routing]` config parsing in `sentry/mod.rs`
- `classify_task_complexity()` async function in `sentry/executor.rs`
- Integration with `spawn_task_execution()` — prefilter before agent loop
- Fallback on prefilter failure
- Cost tracking for prefilter call

### Phase 2: Task tree + flynt board support (~50 lines)
- `model = "auto"` recognized in `ExecutionSpec`
- FlyntTaskBoard passes "auto" through to executor
- TaskTreeBoard same

### Phase 3: Adaptive routing (~200 lines, future)
- Track success/failure rates per complexity class
- Adjust routing thresholds based on observed outcomes
- Metrics export for routing decisions

## Critical files

| File | Purpose |
|---|---|
| `src/sentry/mod.rs` | RoutingConfig parsing |
| `src/sentry/executor.rs` | classify_task_complexity(), model selection |
| `src/sentry/types.rs` | "auto" model sentinel |

## Dependencies

None. Uses existing provider infrastructure for the prefilter call.
