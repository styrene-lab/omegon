+++
title = "Post-0.28 diagnostic and telemetry command architecture"
tags = ["architecture","post-0.28","telemetry","status","stats","surfaces"]
+++

+++
id = "d7592673-8651-4d9d-b952-2e005753e433"
kind = "design_node"

[data]
title = "Post-0.28 diagnostic and telemetry command architecture"
status = "exploring"
issue_type = "epic"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

# Post-0.28 diagnostic and telemetry command architecture

# Post-0.28 diagnostic and telemetry command architecture

## Overview

After 0.28, replace ad hoc slash-command strings with versioned semantic projections shared by TUI, ACP, CLI, web, and IPC. The workstream turns `/status` and `/stats` into stable views over canonical runtime observations rather than surface-owned implementations.

## Design Direction

### Canonical projections

Introduce two serializable DTO families under `core/crates/omegon/src/surfaces/`:

- `HarnessStatusProjection`: identity/authority, runtime generation, route, provider readiness, MCP readiness, secret-backend readiness, capabilities, automation, and diagnostics.
- `SessionTelemetryProjection`: session identity, activity counters, token attribution, context composition, model-route slices, tool outcomes, timings, compactions, and child-work attribution.

Projection builders consume canonical runtime snapshots. Renderers consume projections; renderers never probe state.

### Observation ownership

- `HarnessStatus` remains the live capability/readiness snapshot.
- A dedicated session telemetry accumulator owns monotonic counters and per-turn/model/tool slices.
- Agent events update these stores once. TUI, ACP, web, IPC, checkpoints, and session persistence read the same state.
- Missing data is represented explicitly (`Option`, availability, or provenance), never as zero unless zero was observed.

### Command contracts

- `/status`: “Can this harness act now, through which route, under which identity and authority, and what is degraded?”
- `/stats`: “What has this session consumed and produced so far?”
- `/stats bench` may return only when backed by a separately designed benchmark projection; it is not a synonym for session stats.
- Structured control/RPC methods expose the DTO directly; slash and CLI surfaces provide human renderings of it.

### Versioning and compatibility

Each structured projection carries a schema version. Additive fields are preferred. Renames/removals require an explicit compatibility adapter for persisted sessions and remote clients.

### Privacy and authority

Status exposes secret backend names and readiness only, never values. Telemetry records tool names/outcomes and bounded summaries, not raw secret-bearing arguments by default. Remote projections pass through the canonical control-action/RBAC policy and redaction boundary.

## Phased Workstream

1. **Projection foundation** — DTOs, provenance/availability model, text renderers, parity fixtures.
2. **Canonical accumulator** — event-driven session counters, tokens, tools, latency, compactions, route changes.
3. **Surface migration** — TUI, ACP, CLI, web, IPC consume shared projections; delete duplicate handlers.
4. **Persistence and replay** — store versioned telemetry with sessions and expose timeline/replay views.
5. **Advanced observability** — cost estimates, cache behavior, latency percentiles, quota signals, child-work trees.
6. **Benchmark separation** — define benchmark-run telemetry and only then consider restoring `/stats bench`.

## Decisions

### Decision: semantic DTOs precede renderers

**Status:** decided

All surfaces share facts and provenance while retaining presentation-specific formatting.

### Decision: event-driven accumulation is authoritative

**Status:** decided

Session metrics are updated at the event source, not reconstructed by scanning conversation text or renderer state.

### Decision: unknown is distinct from zero

**Status:** decided

Every metric must distinguish unavailable/unobserved from observed zero.

### Decision: benchmark telemetry is a separate domain

**Status:** decided

Interactive session telemetry and benchmark-run telemetry have different lifecycle, reproducibility, and comparison requirements.

## Open Questions

- Which projection fields are stable enough for a public v1 schema?
- Should persisted telemetry use the existing session format, JSONL events, or a compact indexed sidecar?
- What retention and redaction defaults apply to tool arguments, outputs, and provider diagnostics?
- Which latency clock boundaries are portable across direct providers, ACP workers, and delegated child processes?
- How should cost and quota estimates communicate provider uncertainty?

## Assumptions

- [assumption] `AgentEvent` and the shared dashboard handles can be evolved into a single canonical observation pipeline without unacceptable hot-path contention.
- [assumption] ACP, web, and IPC clients can adopt versioned additive projection schemas.
- [assumption] Existing cross-provider telemetry and evidence-ledger designs remain upstream constraints rather than competing stores.

## Dependencies

- Cross-provider session telemetry schema
- Unified observation/evidence architecture
- Canonical control action matrix
- HarnessStatus contract

## Open Questions
