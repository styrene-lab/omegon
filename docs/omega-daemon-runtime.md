+++
id = "006c7e38-3ca0-439b-953c-49e886a3f5de"
kind = "document"
title = "Omega daemon runtime — persistent agent instances and event ingress"
status = "resolved"
tags = []
aliases = ["omega-daemon-runtime"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["What is the minimum daemon v1 process model: single long-lived server with one active session, or a server managing multiple named agent instances from day one?", "What is the canonical control plane for local daemon instances in v1: native IPC only, or IPC plus a minimal localhost HTTP surface for health/event ingress?", "What is the minimum typed event-ingress contract for v1: a generic event envelope plus one local/manual injection path, or immediate first-class webhook endpoints and connector polling?", "[assumption] Auspex-managed long-running Omegon instances and standalone headless agent deployments can share the same daemon runtime shape, differing only in owner/control metadata rather than process architecture."]
parent = "omega"
related = ["auspex-attach-handoff", "omega-coordinator", "harness-diagnostics"]
+++

# Omega daemon runtime — persistent agent instances and event ingress

## Overview

Define the persistent runtime mode that lets Omegon/Omega run as a long-lived local agent/daemon behind Auspex or standalone. Scope includes daemon/server startup mode, persistent instance identity and lifecycle, attachable long-running sessions, minimal managed-instance model for Auspex control, and a typed event-ingress surface for future triggers such as webhooks, connectors, and scheduled sources.

## Decisions

### Daemon v1 uses a single persistent server process with one active session and optional queued work, not multi-instance management inside one process

**Status:** decided

**Rationale:** Multi-instance orchestration inside one daemon is the wrong first step. Auspex can manage multiple Omegon/Omega processes externally, while daemon v1 proves persistence, attachability, and long-running session correctness with a single durable server/runtime. This minimizes state-machine complexity and keeps the first lifecycle model debuggable.

### Daemon v1 exposes native IPC as the canonical local control plane plus a minimal localhost HTTP surface for health and event ingress

**Status:** decided

**Rationale:** Local operator/Auspex control should remain anchored on native IPC semantics. But event ingress and health/readiness are materially simpler over localhost HTTP. Splitting these concerns keeps attach/control on IPC while allowing webhook/manual event submission without tunneling everything through the attach channel.

### Event ingress v1 is a typed generic event envelope plus one authenticated local HTTP submission path, not full connector/webhook products

**Status:** decided

**Rationale:** Connectors, polling, and external webhook products are second-order integrations. The irreducible primitive is a typed event envelope that can be submitted to a persistent daemon and routed into agent work. Implement the envelope and one ingress endpoint first; build webhooks/connectors/schedulers as producers on top of it later.

### Auspex-managed and standalone headless agent deployments share the same daemon runtime shape; they differ by ownership metadata and ingress policy

**Status:** decided

**Rationale:** There is no value in maintaining separate persistent runtime architectures for 'embedded behind Auspex' and 'standalone agent service'. The durable process, session model, event queue, and control plane should be identical. Ownership metadata and allowed ingress methods can vary without forking the runtime.

### HTTP and raw WebSocket are degraded/bootstrap transports; HTTPS and WSS are the secure network happy path until Styrene RPC is available

**Status:** decided

**Rationale:** Plain HTTP and raw WebSocket are acceptable only for tightly-scoped local/bootstrap use. They must not become the normative remote control or event-ingress posture. Secure network transports should be HTTPS/WSS, and the long-term managed-instance path should converge on Styrene identity-based RPC with mutual authentication semantics.

### Interactive session runtime uses a main-owned turn supervisor with a runtime-owned prompt queue

**Status:** decided

**Rationale:** The interactive/session runtime must stop coupling input ingestion to active turn execution. `main.rs` should own a command-driven turn supervisor with one active turn, a FIFO runtime-owned prompt queue, and explicit Running/Cancelling/Idle truth. TUI, IPC, web, and Auspex become adapters over runtime state rather than owners of prompt lifecycle. This preserves clean multi-surface semantics and enables queueing, honest cancel behavior, and future identity-aware supervision.

## Open Questions

- What is the minimum daemon v1 process model: single long-lived server with one active session, or a server managing multiple named agent instances from day one?
- What is the canonical control plane for local daemon instances in v1: native IPC only, or IPC plus a minimal localhost HTTP surface for health/event ingress?
- What is the minimum typed event-ingress contract for v1: a generic event envelope plus one local/manual injection path, or immediate first-class webhook endpoints and connector polling?
- [assumption] Auspex-managed long-running Omegon instances and standalone headless agent deployments can share the same daemon runtime shape, differing only in owner/control metadata rather than process architecture.
