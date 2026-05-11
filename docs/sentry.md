+++
id = "76f04d6c-3a0f-4abf-8e35-2e3c5f8f8f4a"
kind = "document"
tags = ["sentry", "automation", "routing"]
aliases = ["sentry-automation"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
last_updated = "2026-05-11"
subsystem = "sentry"
+++

# Sentry Automation

Sentry is Omegon's long-running task executor. It watches task sources, consumes trigger events, claims actionable work, runs bounded agent tasks, records outcomes in `.omegon/sentry/state.db`, and exposes a small HTTP control plane.

## Entry Point

```sh
omegon sentry --config sentry.toml --control-port 7842
```

Health probes:

- `GET /api/healthz`
- `GET /api/readyz`

Sentry task endpoints:

- `GET /api/sentry/tasks`
- `GET /api/sentry/tasks/{id}`
- `POST /api/sentry/tasks/{id}/run`
- `POST /api/sentry/trigger/{name}`

## Task Sources

Sentry resolves tasks in this order:

1. `.omegon/tasks/` task tree
2. `sentry.toml`
3. Flynt vault boards when running inside a Flynt vault

If both `.omegon/tasks/` and `sentry.toml` exist, the task tree wins and `sentry.toml` task entries are ignored.

## File Config

```toml
[sentry]
max_concurrent = 2
log_retention_days = 30

[sentry.routing]
prefilter_model = "anthropic:claude-haiku-4-5-20251001"
light_model = "anthropic:claude-sonnet-4-6"
heavy_model = "anthropic:claude-opus-4-6"

[[task]]
name = "ci-check"
prompt = "Check CI status and summarize failures"
model = "auto"
max_turns = 10
timeout_secs = 300

[task.trigger.cron]
schedule = "*/30 * * * *"

[task.budget]
max_tokens_per_day = 500000
max_cost_per_day_usd = 5.00
```

Task fields:

| Field | Behavior |
| --- | --- |
| `name` | Stable task id. |
| `prompt` / `prompt_file` | Inline prompt or path to prompt text. One is required. |
| `model` | Explicit model route, `auto`, or omitted to use the process default. |
| `max_turns` | Per-run turn ceiling. |
| `timeout_secs` | Wall-clock task timeout. |
| `cwd` | Optional working directory override. |
| `env` | Optional per-task environment values. |
| `priority` | Optional board priority metadata. |
| `trigger` | Cron, webhook, file-watch, or git-event trigger config. |
| `budget` | Daily token and estimated cost limits. |

## Auto Routing

When a task sets `model = "auto"` and `[sentry.routing]` is configured, Sentry classifies the prompt and routes simple/moderate tasks to `light_model` and complex tasks to `heavy_model`.

The `prefilter_model` field is parsed as part of the routing contract. The current implementation uses local prompt heuristics for classification, so treat the configured prefilter as forward-compatible metadata rather than proof that a separate model call ran.

## Code-Act Status

The codebase now contains an experimental code-act executor that asks an LLM to generate a Python script, writes it under `.omegon/code-act-<id>.py`, and executes it through the existing bash tool path. It is permission-gated by `OMEGON_CODE_ACT=1` or `--dangerously-bypass-permissions`.

As of this document, code-act is an internal executor module, not a public CLI or slash-command workflow. Do not document it as a normal operator path until it is wired into a command or task mode.
