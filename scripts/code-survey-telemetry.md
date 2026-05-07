+++
id = "9084d005-ef0a-4ae8-845c-06f337f51a57"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Telemetry Code Survey

## Scope checked

- `core/`
- `scripts/`
- repo root `package.json` reference from task contract

## Scope note

This worktree does **not** contain a top-level `extensions/` directory or a root `package.json`. The only package manifest present is `site/package.json`, which is out of scope for this task.

## Current telemetry captured in the harness

### 1. Provider quota and billing telemetry

The harness captures per-turn provider telemetry from upstream HTTP response headers and carries it through the runtime.

- `core/crates/omegon/src/providers.rs`
  - `parse_rate_limit_snapshot()` extracts provider-specific quota fields from response headers.
  - `log_rate_limit_headers()` logs telemetry-related headers for debugging.
- `core/crates/omegon-traits/src/lib.rs`
  - `ProviderTelemetrySnapshot` defines the normalized cross-provider shape.
- `core/crates/omegon/src/bridge.rs`
  - `LlmEvent::Done` includes `input_tokens`, `output_tokens`, `cache_read_tokens`, and optional `provider_telemetry`.
- `core/crates/omegon/src/conversation.rs`
  - `AssistantMessage` persists `provider_tokens` and `provider_telemetry` per assistant turn.

What is normalized today:

- Anthropic unified 5h / 7d utilization percentages
- OpenAI-style request and token remaining counts
- Retry-after / reset timing
- request IDs
- Codex-specific active-limit and reset window fields

### 2. Session-level usage counters

The harness tracks coarse session telemetry for operator feedback and persistence.

- `core/crates/omegon/src/conversation.rs`
  - `SessionStatsAccumulator` tracks turns, tool calls, tokens consumed, and compactions.
- `core/crates/omegon/src/features/harness_settings.rs`
  - `stats` action exposes session telemetry through the `harness_settings` tool.
- `core/crates/omegon/src/tui/footer.rs`
  - `FooterData` carries context percentage/window, token totals, turn count, tool calls, compactions, and provider telemetry.
- `core/crates/omegon/src/tui/mod.rs`
  - TUI updates footer/dashboard counters every frame and rolls cleave child token deltas into session totals.

### 3. TUI instrumentation surfaces

Telemetry is heavily oriented toward local operator visibility inside the TUI.

- `core/crates/omegon/src/tui/instruments.rs`
  - instrument panel visualizes context pressure, memory activity direction, recency-sorted tool activity, and cleave progress.
- `core/crates/omegon/src/tui/mod.rs`
  - drives the instrument panel with live context usage, memory operations, active tool name, thinking level, and child token deltas.
- `core/crates/omegon/src/tui/footer.rs`
  - footer surfaces provider, context, session token totals, turn/tool-call counts, compactions, and provider headroom.
- `core/crates/omegon/src/tui/dashboard.rs`
  - dashboard sidebar exposes session counts (`turns`, `tool_calls`, `compactions`) plus lifecycle/cleave status.

### 4. Web / IPC event telemetry

The harness exposes runtime telemetry to external consumers via typed events and a dashboard API.

- `core/crates/omegon-traits/src/lib.rs`
  - `IpcEventPayload::TurnEnded` includes estimated tokens, actual input/output tokens, cache-read tokens, and provider telemetry.
- `core/crates/omegon/src/web/ws.rs`
  - serializes `AgentEvent::TurnEnd` and forwards provider telemetry over WebSocket.
- `core/crates/omegon/src/web/api.rs`
  - `/api/state` exposes session counters (`turns`, `tool_calls`, `compactions`) in the dashboard snapshot.
- `core/crates/omegon/src/ipc/snapshot.rs`
  - snapshot path reads shared session counters from dashboard handles.

### 5. Session journal telemetry

The harness writes a durable session journal with condensed telemetry.

- `core/crates/omegon/src/features/session_log.rs`
  - stores per-turn summaries including provider, model, actual input/output/cache tokens, and provider telemetry.
  - appends session-end entries with date, branch, recent commits, active OpenSpec changes, turns, tool calls, and duration.

### 6. Internal diagnostic logging

The codebase uses `tracing` broadly for structured diagnostic logs.

- `core/crates/omegon/Cargo.toml` and `core/Cargo.toml`
  - depend on `tracing`, `tracing-subscriber`, and `tracing-appender`.
- `core/crates/omegon/src/main.rs`
  - initializes tracing subscribers, with file-only logging in interactive mode and optional file+stderr logging in headless mode.
- `core/crates/omegon/src/providers.rs`, `loop.rs`, `bus.rs`, `setup.rs`, `cleave/orchestrator.rs`, and others
  - emit structured `tracing::{info,warn,error,debug,trace}!` diagnostics across provider calls, loop retries, tool registration, startup, and cleave orchestration.

## Where telemetry lives

### Operator-facing runtime state

- TUI footer: `core/crates/omegon/src/tui/footer.rs`
- TUI dashboard: `core/crates/omegon/src/tui/dashboard.rs`
- TUI instrument panel: `core/crates/omegon/src/tui/instruments.rs`
- TUI update loop: `core/crates/omegon/src/tui/mod.rs`

### Durable / replayable state

- Conversation turn state: `core/crates/omegon/src/conversation.rs`
- Agent journal: `core/crates/omegon/src/features/session_log.rs`

### Programmatic interfaces

- Bridge events: `core/crates/omegon/src/bridge.rs`
- IPC event schema: `core/crates/omegon-traits/src/lib.rs`
- WebSocket serialization: `core/crates/omegon/src/web/ws.rs`
- Web dashboard snapshot API: `core/crates/omegon/src/web/api.rs`

### Raw diagnostic logs

- subscriber setup: `core/crates/omegon/src/main.rs`
- structured event emission throughout `core/crates/omegon/src/**`

## Notable gaps and inconsistencies

### No unified telemetry pipeline

Telemetry is collected ad hoc for several consumers:

- TUI footer/dashboard
- WebSocket/dashboard API
- session journal
- tracing logs

These surfaces share concepts but not a single event model or collector. The same facts are recomputed or copied across multiple paths.

### Mostly session UI telemetry, not operational telemetry

The harness knows about:

- turns
- tool calls
- context pressure
- token counts
- provider headroom
- memory operation direction

It does **not** appear to maintain first-class counters/histograms/spans for things like:

- per-tool latency distributions
- tool success/error rates by tool name
- provider request latency by model/provider
- retry counts by upstream/provider
- queue depth / concurrency / child lifecycle timings in a normalized sink
- long-term aggregated time series for sessions

`tracing` logs contain pieces of this, but there is no metrics backend extracting it.

### Provider telemetry normalization is partial and header-driven

`ProviderTelemetrySnapshot` is useful, but it is tightly coupled to whichever headers current providers return. That means:

- fields are sparse and provider-specific
- there is no normalized semantic model for quotas/limits beyond a few shared names
- historical comparisons across providers are awkward
- non-header telemetry sources are second-class

### Token accounting is split across multiple layers

Token data appears in several places:

- provider SSE / bridge done event
- conversation assistant message state
- footer session totals
- session log turn summaries
- IPC/websocket turn-end payloads

That duplication increases drift risk. A future bug in one path could make TUI totals, session journal entries, and web snapshots disagree.

### Dashboard/session stats are coarse

Shared dashboard session stats only include:

- turns
- tool_calls
- compactions

That is enough for status display, but not enough for deeper telemetry analysis or external observability.

### No OpenTelemetry dependencies in the core runtime

The surveyed runtime currently depends on `tracing` and `tracing-subscriber`, but not on `opentelemetry`, `tracing-opentelemetry`, or a metrics exporter package.

### Scripts do not appear to add telemetry collection

The `scripts/` directory mostly contains release/demo utilities. I did not find telemetry exporters, metric scrapers, or instrumentation setup there.

## Bottom line

The harness already captures meaningful **local runtime telemetry**:

- per-turn token usage
- provider quota/headroom snapshots
- session counters
- context pressure
- memory/tool activity
- web/IPC snapshots for dashboards
- durable session journal summaries
- extensive structured diagnostic logging via `tracing`

What it lacks is a **single operational observability model**. The current system is optimized for the operator-facing TUI and local debugging, not for backend observability or cross-process telemetry analysis.

That means OpenTelemetry is only worthwhile if the project wants one or more of these outcomes:

- normalized spans/metrics across provider calls, tool execution, and cleave children
- exporter support to external backends
- long-term aggregated telemetry outside the TUI/session journal
- correlation across TUI, web, IPC, and logs through shared trace/span context

Without those goals, the current `tracing` + dashboard/session-log approach already covers the local harness use case reasonably well.
