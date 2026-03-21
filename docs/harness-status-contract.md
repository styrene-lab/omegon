---
id: harness-status-contract
title: HarnessStatus contract — unified status surface for TUI, web dashboard, and bootstrap
status: decided
tags: [architecture, ui, tui, dashboard, bootstrap, contract, persona, mcp, secrets, inference]
open_questions: []
issue_type: feature
priority: 1
---

# HarnessStatus contract — unified status surface for TUI, web dashboard, and bootstrap

## Overview

A single Rust struct (HarnessStatus) that captures the complete observable state of the harness — active persona/tone, MCP servers, secret backend, inference backends, container runtime, context class, memory stats. The TUI renders it in the footer and settings overlay. The web dashboard reads it via WebSocket. The bootstrap prints it once at startup. One source of truth, multiple consumers. This is the UI surface contract for everything built in the persona/MCP/secrets/inference design work.

## Research

### Seven UI domains from this session's work

Each domain has distinct data, controls, and display requirements:

**1. Persona System** — Active persona badge (footer), active tone badge (footer), `/persona` and `/tone` commands, persona picker (settings overlay), plugin list (settings + dashboard). Data: PersonaSummary { id, name, badge, mind_facts_count, activated_skills, disabled_tools }.

**2. MCP Transport** — Server connection status (bootstrap + dashboard), tool list with mcp: labels, error notifications. Data: McpServerStatus { name, transport_mode, tool_count, connected, error }.

**3. Encrypted Secrets** — Backend indicator (bootstrap), lock status (footer), CLI subcommands (init/put/get/list). Data: SecretBackendStatus { backend: KeyBackend, stored_count, locked }.

**4. Native Inference** — Backend probe results (bootstrap), routing config display. Data: InferenceBackendStatus { name, kind, available, models, context_window }.

**5. Memory Layers** — Layer indicators on facts, persona mind stats, tag display. Data: MemoryLayerStats { project_facts, persona_facts, working_facts, persona_id }.

**6. Granular Permissions** — Permission prompts (TUI modal), sticky approval count, per-tool config. Data: PermissionStatus { pending_prompts, sticky_approvals, denied_count }.

**7. OCI Tools** — Container runtime detection (bootstrap), OCI badge on tool cards, build progress. Data: ContainerRuntimeStatus { runtime, version, available }.

### Three consumers, one source of truth

**Consumer 1: Bootstrap (startup)**
Prints once when omegon launches. Shows the full inventory: cloud providers, local inference, MCP servers, secrets backend, container runtime, installed plugins. This is the operator's "what do I have?" view. Already partially exists in the operator-capability-profile bootstrap flow — HarnessStatus extends it.

**Consumer 2: TUI (continuous)**
Footer bar shows: model name + context class badge + thinking level + active persona badge + active tone badge + secret lock status. Settings overlay shows: persona picker, tone picker, plugin list, MCP server list, permission config. Tool cards show: `[MCP]` or `[OCI]` badges per tool. Modals: permission prompts, persona switch confirmations.

**Consumer 3: Web Dashboard (continuous, remote)**
Receives HarnessStatus via WebSocket on the existing event bus. Renders: persona/tone in header, MCP servers in tools panel, memory layers in memory panel, inference backends in a new system panel. The dashboard is a read-only consumer — controls go through the TUI or CLI.

**Delivery mechanism:**
HarnessStatus is assembled by the agent loop from its constituent parts (PluginRegistry, McpFeature, SecretStore, etc.). On any state change (persona switch, MCP connect/disconnect, secret store lock/unlock), a BusEvent::HarnessStatusChanged is emitted. TUI and web dashboard subscribe. Bootstrap reads once at startup before the event loop begins.

```
┌──────────────────────────────────────────────┐
│              Agent Loop                       │
│  PluginRegistry ──┐                          │
│  McpFeature    ───┤── assemble() ──▶ HarnessStatus
│  SecretStore   ───┤                   │      │
│  InferenceProbe ──┘                   │      │
└───────────────────────────────────────│──────┘
                                        │
              ┌─────────────────────────┤
              ▼                         ▼
         BusEvent::                /api/status
         HarnessStatusChanged      (WebSocket)
              │                         │
         ┌────┴────┐              ┌─────┴─────┐
         │   TUI   │              │    Web     │
         │ Footer  │              │ Dashboard  │
         │ Overlay │              │  Panels    │
         └─────────┘              └───────────┘
```

## Decisions

### Decision: Bootstrap is a structured TUI panel, not plain printf

**Status:** decided
**Rationale:** The bootstrap output is the operator's first impression. A ratatui-rendered panel with colored status indicators (✓/⚠/○), aligned columns, and grouped sections looks professional and matches the TUI aesthetic. Plain printf is for CI/headless (--no-tui flag). The bootstrap panel is the same HarnessStatus struct rendered once — same data, same renderer, just displayed before the event loop starts.

### Decision: HarnessStatus is event-driven via BusEvent::HarnessStatusChanged

**Status:** decided
**Rationale:** State changes are infrequent (persona switch, MCP connect, secret unlock) but time-sensitive — the operator needs to see them immediately. Polling would either be too frequent (waste) or too slow (stale UI). The existing EventBus already delivers typed events to all subscribers. Adding HarnessStatusChanged is one enum variant. The TUI footer re-renders on this event. The web dashboard broadcasts it over WebSocket.

### Decision: Web dashboard uses existing WebSocket event bus — HarnessStatusChanged is a new message type

**Status:** decided
**Rationale:** The web dashboard already connects via WebSocket to receive agent events (tool calls, messages, cleave progress). Adding HarnessStatusChanged as a new message type on the same connection keeps the architecture simple — one WebSocket, all events. No separate /api/status endpoint needed. The dashboard renders the latest HarnessStatus snapshot from the most recent event.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/status.rs` (new) — HarnessStatus struct + sub-types (PersonaSummary, McpServerStatus, etc.) + assemble() function
- `core/crates/omegon-traits/src/lib.rs` (modified) — Add BusEvent::HarnessStatusChanged variant
- `core/crates/omegon/src/tui/footer.rs` (modified) — Render persona badge, tone badge, secret lock, MCP count in footer
- `core/crates/omegon/src/tui/bootstrap.rs` (new) — Structured bootstrap panel rendering from HarnessStatus
- `core/crates/omegon/src/web/mod.rs` (modified) — Broadcast HarnessStatusChanged over WebSocket to dashboard
- `core/crates/omegon/src/setup.rs` (modified) — Assemble HarnessStatus at startup, emit initial event

### Constraints

- HarnessStatus must be Clone + Serialize — it crosses thread boundaries and is sent over WebSocket
- All sub-types must have Display impls for the bootstrap renderer
- The footer must fit in a single terminal line — use short labels and badges, not full names
- Bootstrap panel must degrade gracefully to plain text when --no-tui is set
- The event must not contain secret values — only metadata (backend type, count, locked status)
