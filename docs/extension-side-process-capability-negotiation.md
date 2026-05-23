+++
id = "extension-side-process-capability-negotiation"
kind = "document"
title = "Extension Side-Process Capability Negotiation"
status = "seed"
tags = ["extension", "substrate", "capabilities", "workspace", "api"]
aliases = ["extension-side-process-capability-negotiation"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "extension-side-process-substrate-api"
dependencies = ["extension-side-process-substrate-api"]
open_questions = [
  "What exact capability flags should the substrate adapter expose?",
  "Should extensions query capabilities directly, or should they submit a request and receive structured unavailable/degraded responses?",
  "How should degraded mode be represented when graphics or mouse passthrough is unavailable?",
  "Should backend choice be user-configurable, automatic, or both?",
  "How should telemetry/logging record which substrate handled a request?"
]
related = ["reader-workspace-substrate-adapter", "cockpit-par-term-substrate-analysis", "extension-side-process-substrate-api", "extension-side-process-backend-registry"]
+++

# Extension Side-Process Capability Negotiation

## Overview

Define how extensions discover or receive the side-process capabilities available in the current Omegon session.

The extension should not ask "am I in Zellij?" or "is Cockpit enabled?" It should ask for a capability or submit a request with requirements.

## Candidate flow

```text
extension submits request
  ↓
core validates manifest policy
  ↓
core computes required + preferred capabilities
  ↓
substrate registry selects backend
  ↓
backend opens pane or returns structured unavailable/degraded result
```

## Required versus preferred capabilities

A request may contain:

- required capabilities: absence means fail;
- preferred capabilities: absence means continue with warning/degraded status.

Example for Bookokrat:

```text
required:
  - host_process
  - adjacent_pane
  - resize_propagation

preferred:
  - graphics_passthrough
  - mouse_passthrough
  - replace_named_pane
```

If EPUB text mode is requested, graphics may be preferred. If PDF/image mode is requested, graphics may become required.

## Response model

Possible result variants:

```rust
enum SideProcessResult {
    Opened(SideProcessHandle),
    OpenedDegraded {
        handle: SideProcessHandle,
        missing: Vec<Capability>,
        message: String,
    },
    Unavailable {
        missing: Vec<Capability>,
        setup: Vec<SetupInstruction>,
    },
    Denied {
        reason: DenialReason,
    },
    Failed {
        backend: Option<BackendId>,
        message: String,
    },
}
```

Backend IDs may be included for diagnostics, but extensions should not branch on them for normal behavior.

## Handle model

A handle should be logical and backend-neutral:

```rust
SideProcessHandle {
    id: SideProcessId,
    title: String,
    placement: PanePlacement,
    capabilities: CapabilitySet,
    lifecycle: LifecycleState,
}
```

Backend-specific pane IDs remain internal to core.

## Research tasks

1. Define the minimal capability enum.
2. Define request/response structs.
3. Map Zellij capabilities.
4. Map Cockpit capabilities.
5. Map Kitty remote-control capabilities if kept as a spike.
6. Define fallback/setup instruction schema.
7. Decide whether extensions can subscribe to lifecycle events.
