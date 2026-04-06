---
title: Telemetry survey
status: implemented
tags: [telemetry, observability, survey, opentelemetry]
date: 2026-04-06
---

# Telemetry survey

## Scope

Survey of telemetry-related design/docs artifacts in `docs/`, `openspec/`, `ai/`, and `AGENTS.md` to establish:

1. what telemetry the harness is already intended to capture
2. which constraints and semantics have already been decided
3. whether the current direction resembles an OpenTelemetry-shaped problem

## Executive summary

The documented telemetry model is **operator-facing session telemetry**, not generic infra observability.

The dominant themes are:

- **provider/session economics**: token usage, quota or rate-limit headroom, context utilization, provider/model switches, estimated cost
- **workflow telemetry**: cleave child lifecycle, tool-call summaries, progress events, phase boundaries, memory injection metrics
- **auditability and replay**: per-turn snapshots, provider-specific semantics preserved honestly, cross-provider event logs, typed child-session events
- **dashboard-first consumption**: most telemetry is designed for the footer/HUD, raised dashboard, replay/inspection tools, and lifecycle audit surfaces

There is **no evidence of an OpenTelemetry adoption plan** in the scoped docs. A repo-wide search found telemetry/observability/tracing discussions, but **no references to `OpenTelemetry`, `otel`, or `opentelemetry`** in `docs/`, `openspec/`, `ai/`, or `AGENTS.md`.

## Intended telemetry model

### 1. Cross-provider session telemetry is the long-term north star

The clearest design statement is the `cross-provider-session-telemetry-schema` node. It defines the target as a **provider-agnostic session/event log schema** that supports replay and inspection across Anthropic, OpenAI-compatible providers, Codex, and local models while preserving:

- replayability
- token / cost / quota attribution
- tool execution detail
- context composition
- model/provider switching
- subagent/cleave trees

See `docs/cross-provider-session-telemetry-schema.md:15-17`.

This document also argues that the current cleave path is weak because it relies on scraping stderr rather than emitting typed events. The explicit direction is toward:

- typed child-session lifecycle events
- semantic heartbeats
- progress snapshots
- structured completion payloads

See `docs/cross-provider-session-telemetry-schema.md:23-39`.

### 2. Provider usage and quota data should be first-class session data

`docs/perpetual-rolling-context.md` identifies a concrete bug: provider usage data is present in upstream responses but currently discarded. The doc says this is the only ground-truth source for token counting and should be extracted and stored as `Usage { input_tokens, output_tokens }` alongside the assistant message.

See `docs/perpetual-rolling-context.md:272-276`.

The same design also treats per-model billing and rate limits as important observability/session-economics data and proposes surfacing real-time and per-session metrics such as:

- buffer utilization
- projection coverage
- last-turn input/output tokens
- cumulative tokens
- estimated cost
- provider switches

See `docs/perpetual-rolling-context.md:317-319` and `docs/perpetual-rolling-context.md:521-533`.

It further ties telemetry to routing/cost posture, with behavior derived from provider auth and subscription type rather than a one-size-fits-all metric model.

See `docs/perpetual-rolling-context.md:502-519` and `docs/perpetual-rolling-context.md:537-546`.

### 3. Provider telemetry must preserve provider-specific semantics honestly

The archived OpenSpec change `provider-telemetry-footer` is explicit: Omegon should capture provider quota or rate-limit telemetry **when upstream providers expose it**, and the UI must display it **without pretending providers share identical semantics**.

Its requirements include:

- Anthropic unified utilization headers captured as raw five-hour / seven-day utilization values
- OpenAI-style `x-ratelimit` headroom and reset fields captured when present
- footer/HUD presentation that preserves semantics instead of flattening everything into fake generic percentages
- per-turn persistence of provider telemetry snapshots for later audit

See `openspec/archive/provider-telemetry-footer/specs/telemetry/provider-telemetry.md:5-34`.

The proposal frames the scope as footer/HUD display plus per-turn persistence for mixed-provider session audit, starting with Anthropic/OpenAI/OpenRouter and leaving room for Codex status telemetry.

See `openspec/archive/provider-telemetry-footer/proposal.md:1-9`.

### 4. Cleave observability is moving from log-scraping to structured progress events

The implemented `native-dispatch-observability` design records the transition from unstructured `tracing::info!` stderr lines to a clean structured progress channel. It rejects TS-side regex parsing as fragile and chooses NDJSON lifecycle events emitted from Rust.

See `docs/native-dispatch-observability.md:52-70` and `docs/native-dispatch-observability.md:108-118`.

The intended progress schema includes events such as:

- `wave_start`
- `child_spawned`
- `child_status`
- `child_activity`
- `auto_commit`
- `merge_start`
- `merge_result`
- `done`

See `docs/native-dispatch-observability.md:72-100`.

The constraints are important because they define the telemetry contract shape:

- stdout is reserved for JSON progress only
- events must be self-contained
- child activity must be throttled
- child labels must match the TS-side child identity exactly

See `docs/native-dispatch-observability.md:134-139`.

Related design docs reinforce the same model: the existing child process surface is described as opaque and lossy, and the preferred direction is richer structured observability rather than raw log retention.

See `docs/cleave-child-observability.md:14-44` and `docs/cleave-child-observability.md:69-93`.

### 5. Memory/context telemetry is approximate, calibrated, and operator-visible

`docs/design/memory-mind-audit.md` documents an implemented audit slice for memory injection telemetry. It measures and surfaces:

- injection mode
- project fact count
- edge count
- working-memory fact count
- semantic-hit count
- recent episode count
- global fact count
- payload characters
- estimated token count

See `docs/design/memory-mind-audit.md:62-88`.

The same design is careful about semantic honesty:

- the memory bar is explicitly approximate
- `char/4` is heuristic, not tokenizer-accurate
- the denominator is total context window, not pure conversation
- after compaction, unknown usage should render as unknown, not zero

See `docs/design/memory-mind-audit.md:39-47` and `docs/design/memory-mind-audit.md:123-147`.

It also adds a calibration path against observed provider input tokens instead of treating estimates as truth.

See `docs/design/memory-mind-audit.md:152-176`.

### 6. Benchmarking telemetry is phase-oriented, not infra-oriented

`docs/benchmark-telemetry-capture.md` proposes structured telemetry at phase boundaries for benchmark/demo runs. The events capture phase changes, metric snapshots, and headless instrument intensity statistics for later results artifacts.

See `docs/benchmark-telemetry-capture.md:10-14`.

This is again application/session telemetry for evaluation workflows, not distributed tracing for backend services.

## Explicit design constraints already present

### Preserve semantic honesty

The strongest recurring constraint is: **do not normalize unlike provider signals into fake generic telemetry**.

Evidence:

- provider footer spec requires preserving Anthropic utilization semantics vs OpenAI rate-limit semantics (`openspec/archive/provider-telemetry-footer/specs/telemetry/provider-telemetry.md:20-29`)
- memory audit insists approximate/unknown values remain labeled as such (`docs/design/memory-mind-audit.md:39-47`, `docs/design/memory-mind-audit.md:133-147`)

### Prefer typed events over log scraping

Telemetry contracts should be schema-first, not regex-first.

Evidence:

- `docs/cross-provider-session-telemetry-schema.md:23-39`
- `docs/native-dispatch-observability.md:54-64`, `docs/native-dispatch-observability.md:110-118`

### Optimize for dashboard/replay consumers

Telemetry is intended primarily for operator-facing surfaces and post-hoc inspection:

- footer/HUD
- raised dashboard
- session audit
- replay/inspector tooling

Evidence:

- `openspec/archive/provider-telemetry-footer/specs/telemetry/provider-telemetry.md:20-34`
- `docs/native-dispatch-observability.md:16`, `docs/native-dispatch-observability.md:40-50`
- `docs/cross-provider-session-telemetry-schema.md:15-17`

### Capture per-turn snapshots, not only aggregates

The session model wants turn-level attribution so mixed-provider sessions can be audited later.

Evidence:

- `openspec/archive/provider-telemetry-footer/specs/telemetry/provider-telemetry.md:30-34`
- `docs/perpetual-rolling-context.md:274-276`, `docs/perpetual-rolling-context.md:523-533`

### Keep telemetry payloads bounded and useful

Not everything should be emitted. The docs repeatedly constrain telemetry to high-signal summaries:

- no raw thinking stream in cleave progress (`docs/native-dispatch-observability.md:115-118`)
- throttle `child_activity` to at most 1/sec (`docs/native-dispatch-observability.md:138`)
- ring-buffer style recent lines instead of full persistent log spam for child observability (`docs/cleave-child-observability.md:69-93`)

### The harness is file-backed and audit-oriented

Within `ai/`, the only relevant scoped artifact is `ai/lifecycle/state.json`, which is a structured lifecycle state store rather than a telemetry pipeline. That fits the broader pattern: Omegon prefers transparent JSON/markdown artifacts and operator-auditable state over opaque daemonized telemetry infrastructure.

See `ai/lifecycle/state.json` and the broader lifecycle guidance referenced from docs such as `docs/opsx-core-rust-fsm.md`.

## What AGENTS.md contributes

`AGENTS.md` does not define telemetry architecture directly. The relevant constraints are process constraints:

- trunk-based workflow and conventional commits
- TypeScript changes must pass `npx tsc --noEmit`
- release preflight requires a clean tree and reconciled release-facing surfaces

See `AGENTS.md:8-19`.

That matters only indirectly: any telemetry work must remain auditable, checked, and release-disciplined.

## Assessment: does this look like an OpenTelemetry problem?

Mostly **no**.

The documented need is not primarily:

- cross-service distributed tracing
- vendor-neutral export to external observability backends
- fleet-wide metrics collection
- standard trace/span propagation across network boundaries

The documented need is primarily:

- session replay
- per-turn/provider audit
- dashboard UI state
- cleave child progress
- cost/quota/context economics
- operator-facing introspection

That is a **domain telemetry schema** problem first.

OpenTelemetry might help later for a subset of needs — especially if the project wants standard spans/metrics exported to external backends or wants internal Rust/TS runtime instrumentation normalized across components — but the current docs point to a custom session/event model as the primary contract.

## Preliminary worthiness evaluation for OpenTelemetry

### Where OpenTelemetry could help

- internal runtime spans around provider calls, tool execution, and cleave orchestration
- exporter compatibility if the project later wants OTLP/Jaeger/Grafana-style sinks
- standardized metrics plumbing for counters/histograms around request latency, retries, failures, and token usage

### Where it does not fit the documented problem well

- provider-specific quota semantics need custom modeling anyway
- replayable session history and cross-provider transcript inspection are richer than standard spans/metrics/logs alone
- UI-facing progress events (`child_activity`, provider telemetry snapshots, memory audit snapshots) need bespoke schemas regardless
- the docs repeatedly care about **honest semantic representation**, which argues against prematurely flattening everything into generic tracing conventions

### Net assessment

Adopting OpenTelemetry **as the primary telemetry model** would probably be the wrong abstraction at this stage.

If adopted at all, it should be **secondary infrastructure instrumentation** layered under a custom session telemetry schema — not a replacement for that schema.

## Recommendation to the parent assessment

1. Treat the existing design direction as a **custom session telemetry/event model**.
2. Preserve provider-specific and workflow-specific payloads as first-class schema objects.
3. Evaluate OpenTelemetry only as an optional export/runtime-instrumentation layer, not the canonical data model.
4. Do not let an OTEL migration flatten away the semantics the docs explicitly protect.

## Evidence index

- `docs/cross-provider-session-telemetry-schema.md:15-17`
- `docs/cross-provider-session-telemetry-schema.md:23-39`
- `docs/perpetual-rolling-context.md:272-276`
- `docs/perpetual-rolling-context.md:317-319`
- `docs/perpetual-rolling-context.md:502-519`
- `docs/perpetual-rolling-context.md:521-533`
- `docs/perpetual-rolling-context.md:561-593`
- `docs/native-dispatch-observability.md:52-70`
- `docs/native-dispatch-observability.md:72-118`
- `docs/native-dispatch-observability.md:134-139`
- `docs/cleave-child-observability.md:14-44`
- `docs/cleave-child-observability.md:69-93`
- `docs/design/memory-mind-audit.md:39-47`
- `docs/design/memory-mind-audit.md:62-88`
- `docs/design/memory-mind-audit.md:123-176`
- `docs/benchmark-telemetry-capture.md:10-14`
- `openspec/archive/provider-telemetry-footer/proposal.md:1-9`
- `openspec/archive/provider-telemetry-footer/specs/telemetry/provider-telemetry.md:5-34`
- `AGENTS.md:8-19`
