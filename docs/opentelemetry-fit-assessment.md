+++
id = "8a8acb70-e5bf-44d5-9602-a091b866af52"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenTelemetry fit assessment

## Question

Should Omegon adopt OpenTelemetry for harness telemetry now?

## Verdict

**Recommendation: not yet for the core harness runtime.**

OpenTelemetry is a credible future export/interoperability layer, but it is **not the right primary telemetry model for the harness today**.

The harness already has a strong internal telemetry/event vocabulary optimized for operator-facing local UX:

- typed turn/tool/message/session events
- provider quota/headroom snapshots
- live TUI/dashboard state projection
- IPC/websocket event projection for external consumers
- append-only session summaries

OpenTelemetry would add the most value when Omegon needs one or more of these capabilities:

1. export to external observability backends (OTLP collector, Tempo, Jaeger, Honeycomb, SigNoz, Grafana, etc.)
2. cross-process trace correlation across parent/child cleave runs and future remote executors
3. fleet/service operations across many concurrent harness instances
4. standardized metrics/log/trace ingestion by outside systems

That is **not the dominant need today**. The current telemetry is primarily:

- local and operator-facing
- lifecycle-oriented rather than request/service-oriented
- consumed directly by the TUI, web dashboard, IPC, and session log
- relatively low-cardinality and domain-specific

So the cost/benefit is currently unfavorable for making OTel the harness-native substrate.

## What the harness already captures

### 1. First-class typed runtime events

The harness already defines a typed internal event model (`AgentEvent`) that includes:

- turn start/end
- streaming message and thinking chunks
- tool start/update/end
- phase changes
- decomposition lifecycle
- system notifications
- harness status changes
- context updates
- session reset

This is already close to an application-specific observability schema.

### 2. Provider telemetry and quota/headroom data

`ProviderTelemetrySnapshot` already captures provider-specific quota surfaces such as:

- Anthropic 5h / 7d utilization
- generic requests/tokens remaining
- retry-after
- request ID
- Codex rate-limit window metadata

This is valuable operator telemetry, but it is not a natural fit for stock OTel semantic conventions. It would need custom attributes anyway.

### 3. Multi-surface projection already exists

The same harness telemetry is projected into multiple surfaces today:

- TUI updates footer/instrument state directly
- web dashboard broadcasts `AgentEvent` over WebSocket
- IPC projects selected events into `IpcEventPayload`
- session log stores per-turn provider/model/telemetry summaries

That means the project already solved the hard part that matters most right now: **a coherent internal event vocabulary**.

### 4. Cleave observability is domain-specific

The project has explicit design work around child progress, structured progress sinks, live child inspection, and dashboard observability. Those are valuable, but they are harness-native concepts rather than standard service telemetry. OTel can carry them, but it does not define them.

## Where OpenTelemetry would help

### A. External backend export

If Omegon wants telemetry outside the local process, OTel becomes attractive because it provides:

- OTLP exporters
- collector pipeline compatibility
- standard trace/metric/log ingestion into existing observability stacks
- correlation via trace/span IDs

### B. Cross-process trace propagation

The cleave architecture already has parent/child orchestration and a growing need for structured event transport. If the project later wants end-to-end traces across:

- parent harness
- child harnesses
- remote executors
- web compatibility surface
- future external coordinators

then OTel trace context propagation becomes valuable.

### C. Standardized metrics for operations

If the project later cares about dashboards like:

- turns per minute
- tool latency histograms
- provider error rates
- compaction frequency
- auth failures by route
- child timeout rates

then OTel metrics would help export these to Prometheus/OTLP-compatible systems.

## Why full adoption is not worthwhile yet

### 1. The harness is not primarily a distributed service

Omegon today behaves more like a local interactive control system than a fleet-operated service mesh. Its important telemetry is:

- highly interactive
- session-scoped
- UX-facing
- rich in domain semantics

OTel is strongest when many processes/services need a common transport into an external observability backend. That is only partially true here.

### 2. Existing telemetry is event-rich but not span-modeled

The current model is based on domain events and snapshots, not nested spans with strict parent/child timing relationships. Forcing everything into spans now would create mapping work without much immediate operator value.

Examples of awkward mappings:

- streaming thinking chunks
- context-window snapshots
- memory fact mutations
- cleave child lifecycle cards
- provider quota snapshots at turn end

These can be represented in OTel, but mostly as custom events/attributes rather than benefiting from standard semantic conventions.

### 3. OTel would introduce operational and implementation overhead

Adopting OTel in the Rust runtime would require decisions around:

- tracer/provider initialization in interactive vs headless mode
- exporter lifecycle and shutdown
- batching behavior and performance overhead
- local/dev defaults vs collector configuration
- log/trace/metric separation
- sampling strategy
- attribute cardinality control
- privacy/sensitivity boundaries for prompts, tool args, file paths, and model output

That is real complexity. For a local harness, the default result is often a large amount of plumbing to export data nobody is yet consuming.

### 4. The project already has multiple bespoke consumers

The TUI, web dashboard, IPC, and session log all consume the existing typed event stream directly. Introducing OTel underneath them would not replace those consumers. It would add another parallel telemetry path.

That means a likely near-term architecture is:

- existing internal events remain the source of truth
- OTel becomes an export adapter layered on top

If that is the right architecture anyway, there is no reason to make OTel foundational now.

## Risks if adopted too early

1. **Telemetry model split** — harness-native events on one side, OTel spans/metrics on the other
2. **Semantic drift** — operators care about concepts OTel does not standardize
3. **Attribute leakage** — prompts, tool args, paths, auth/provider details are easy to over-export
4. **Noise/cardinality** — tool names, file paths, model names, branch names, child labels, and route IDs can explode cardinality
5. **Maintenance burden** — OTel Rust crates and bridge layers add integration churn without immediate payoff

## A better path

### Short-term recommendation

Keep the current harness-native telemetry model.

Strengthen it by continuing to:

- make `AgentEvent` / IPC / WebSocket projections canonical and complete
- preserve stable event names and payload contracts
- improve per-turn/session summaries
- improve cleave observability through structured progress events
- document the event schema and intended consumers

### Medium-term recommendation

Add an **optional export adapter**, not a foundation rewrite.

The right shape is:

1. retain `AgentEvent` + provider/session snapshots as the source model
2. map selected lifecycle events to OTel spans/events only when export is enabled
3. emit a small set of low-cardinality metrics
4. keep sensitive payloads out by default

Good first exports would be:

- trace/span for a harness turn
- child spans for tool executions
- metrics for tool duration, turn duration, provider errors, child failures
- events for provider headroom snapshots

### Trigger for reevaluation

Revisit OTel when at least one becomes true:

- Omegon needs remote/fleet observability
- cleave children run on remote workers or multiple machines
- an external collector/backend becomes part of normal operations
- operators need correlation across harness, web, IPC, and remote execution boundaries

## Conclusion

OpenTelemetry is **worth keeping in reserve**, but **not worth adopting as a primary telemetry substrate today**.

The project already has a meaningful telemetry system. The missing work is less "add OTel" and more:

- stabilize the harness-native schema
- close projection gaps across TUI/web/IPC/session log
- add optional export once there is a real external consumer

So the correct call today is:

**Do not adopt OpenTelemetry broadly yet. Design for future export compatibility instead.**
