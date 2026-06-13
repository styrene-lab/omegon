---
id: acp-send-vs-local-runtime-ownership-assessment
title: "ACP Send handlers vs local single-thread runtime ownership assessment"
status: deferred
tags: [acp, runtime-ownership, architecture, migration]
open_questions:
  - "[assumption] ACP's Send-oriented builder API is an intentional upstream architectural commitment rather than an incidental implementation detail that may gain local/non-Send support soon."
  - "[assumption] Omegon's ACP/runtime state should remain local single-threaded until the broader runtime/TUI protocol split decides ownership and synchronization boundaries."
dependencies: []
related:
  - runtime-ui-separation-via-protocol-gateway
  - runtime-facade-command-event-model
---

# ACP Send handlers vs local single-thread runtime ownership assessment

## Overview

Assess the architectural fork between ACP 0.12's Send-oriented builder/handler paradigm and Omegon's current local single-threaded ACP/runtime ownership model. The node exists to evaluate whether future work should align with upstream ACP's Send handler model, retain a local JSON-RPC adapter boundary, or introduce a bridge layer that isolates upstream Send requirements from Omegon's local runtime state.

## Research

### ACP 0.12.1 handler/transport findings

ACP 0.12.1 inspection: the old `AgentSideConnection`/`impl Agent` seam is gone. `Agent` is now a role marker with `Agent.builder()`. Builder handlers (`on_receive_request`, `on_receive_notification`, `on_receive_dispatch`, and custom `HandleDispatchFrom`) require `Send`, and `ConnectTo` also requires `Send + 'static`. This conflicts directly with Omegon's current `Rc<RefCell<...>>`/LocalSet ACP agent state. The SDK does expose `ByteStreams::into_channel_and_future()` and a public `Channel` carrying raw `jsonrpcmsg::Message` values, which can support a local JSON-RPC adapter that bypasses Send handler closures while preserving the byte-stream transport.

### Cleave and delegation implications

Implication for cleave/subagent/delegation: subagents should not borrow or mutate the ACP/TUI runtime state directly. Cleave and delegation operations should be modeled as runtime commands/jobs owned by the single runtime owner, with status streamed back as RuntimeEvents/ACP session updates. Worker execution may remain concurrent behind the runtime boundary, but UI/protocol state mutation stays serialized through the owner. This avoids deadlocks and races from ACP handler-level `Send` concurrency while preserving parallel child execution where it already belongs: worker tasks, process execution, model calls, and external runtime-to-runtime protocols.

## Decisions

### Use a local JSON-RPC adapter for ACP 0.25.6 behavior preservation

**Status:** proposed

**Rationale:** The immediate ACP 0.25.6 migration should not force `OmegonAcpAgent` into `Send`/`Sync` or convert local state to `Arc<Mutex<...>>`. A local adapter over ACP `Channel` preserves current LocalSet semantics and keeps the upstream Send-vs-local ownership question isolated for later assessment.

### Evaluate an upstream-aligned Send bridge after runtime/TUI split seams are clearer

**Status:** candidate

**Rationale:** A future bridge could keep ACP SDK builder semantics while forwarding typed requests into a local runtime owner. That should be assessed after `RuntimeHandle`/command/event ownership boundaries exist, because otherwise the bridge risks encoding temporary synchronization choices as architecture.

### Keep ACP 0.25.6 runtime ownership local and single-threaded

**Status:** accepted

**Rationale:** Omegon will not convert ACP/runtime state from `Rc<RefCell<...>>`/LocalSet ownership to `Arc<Mutex<...>>` merely to satisfy ACP 0.12's Send-oriented builder handlers. For 0.25.6, ACP remains an edge compatibility adapter around a local runtime owner. Runtime-runtime communication belongs in A2A/Styrene or a superset protocol, and future multi-client attach should route through an explicit runtime command/event boundary rather than direct shared mutable ACP handler state.

### Use one runtime owner with protocol adapters, not shared mutable runtime state

**Status:** accepted

**Rationale:** The intended architecture is one runtime owner receiving commands and emitting events/snapshots. ACP, A2A/Styrene, TUI, daemon attach, and future clients should be adapters into that boundary. This preserves 1:1 client:Omegon ACP as the normal topology while leaving explicit room for observer, reconnect, supervisor, or multi-client modes later without allowing arbitrary clients to mutate shared runtime state directly.

## Open Questions

- [assumption] ACP's Send-oriented builder API is an intentional upstream architectural commitment rather than an incidental implementation detail that may gain local/non-Send support soon.
- [assumption] Omegon's ACP/runtime state should remain local single-threaded until the broader runtime/TUI protocol split decides ownership and synchronization boundaries.
