+++
id = "29cd2725-d9b6-4361-9467-d55a68af73b9"
kind = "document"
title = "TUI → Auspex attach handoff"
status = "exploring"
tags = []
aliases = ["auspex-attach-handoff"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["Should the initial attach-to-running-instance path use a custom URL scheme, an Auspex single-instance control socket, or both?", "How should Windows be handled before native IPC parity exists there?"]
parent = "auspex-ipc-contract"
related = []
+++

# TUI → Auspex attach handoff

## Overview

Define how a running Omegon TUI instance launches or focuses Auspex and hands it enough metadata to attach to the exact live session over the canonical backend contract, without making the embedded `/dash` browser surface the long-term transport.

The operator intent is simple:

> "I am in Omegon right now. Open the richer Auspex UI on *this* live session."

The handoff must preserve that exact session identity, must not destabilize the TUI if Auspex fails, and must keep the canonical desktop integration on the native IPC contract rather than the local browser compatibility protocol.

## Decisions

### Desktop Auspex attach uses IPC as the canonical transport

**Status:** decided

**Rationale:** Native Auspex attach from a running Omegon TUI should use the existing typed IPC contract rather than promoting the embedded /dash HTTP/WebSocket compatibility surface into the long-term first-party desktop backend.

### TUI-originated handoff is session-scoped first, workspace-scoped second

**Status:** decided

**Rationale:** When the operator launches Auspex from a live TUI session, the expected behavior is to focus the exact current session in Auspex, with workspace context as supporting context rather than the primary target.

### Omegon may launch/focus Auspex but does not deeply supervise its lifecycle

**Status:** decided

**Rationale:** Omegon should be able to request an attached Auspex UI without taking ownership of Auspex window/app lifecycle, persistence, update, or restart behavior. A failed Auspex launch must not destabilize the running TUI session.

### `/auspex` is the primary local desktop command; `/dash` remains compatibility/debug wording

**Status:** decided

**Rationale:** Operator-facing copy should present Auspex as the first-class desktop handoff path now. Keeping `/dash` as the compatibility/debug browser path preserves the embedded surface for diagnostics and transition work without presenting it as the product-default local UI.

### Native attach metadata is the primary local handoff contract

**Status:** decided

**Rationale:** Omegon should launch Auspex with a structured attach envelope for the exact live session (`AUSPEX_OMEGON_ATTACH_JSON`, `transport=omegon-ipc`) and treat the embedded browser surface only as compatibility/bootstrap support when startup metadata is not yet available. This keeps local desktop control anchored on IPC semantics even while `/dash` still exists for diagnostics and transition work.

## Open Questions

- Should the initial attach-to-running-instance path use a custom URL scheme, an Auspex single-instance control socket, or both?
- How should Windows be handled before native IPC parity exists there?
