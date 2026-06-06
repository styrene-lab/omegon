+++
title = "Context Compaction Control Diagnostics"
tags = ["design","context","diagnostics"]
+++

+++
id = "736cfff2-4c24-48e5-8f87-4a46707329f4"
kind = "design_node"

[data]
title = "Context Compaction Control Diagnostics"
status = "exploring"
issue_type = "design"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

# Context Compaction Control Diagnostics

---
title: Context Compaction Control Diagnostics
status: exploring
tags: [design, context, diagnostics]
issue: https://github.com/styrene-lab/omegon/issues/130
---

# Context Compaction Control Diagnostics

## Overview

Manual `/context compact` currently runs through `control_runtime::context_compact_response()` and calls `compact_via_llm()` directly. In-turn compaction uses the loop configuration `force_compact` / `pending_compact` path. These two paths can diverge in behavior and diagnostics.

## Problem

The operator-facing control surface needs one coherent model for context compaction outcomes:

- no payload / nothing to compact
- queued for next turn
- started
- succeeded
- failed
- skipped because a compaction is already active

Today those states are split across direct control runtime behavior and loop-driven behavior.

## Design questions

- [assumption] Manual compaction should remain available outside an active turn.
- Should `/context compact` be immediate, queued, or explicitly support both modes?
- What structured event should ACP/control clients receive for compaction state changes?
- Should compaction diagnostics use existing `AgentEvent::SystemNotification`, a new typed event, or both?

## Candidate design

Keep immediate manual compaction, but add typed diagnostics shared by manual and loop-driven paths:

```text
ContextCompactionRequested
ContextCompactionSkipped
ContextCompactionStarted
ContextCompactionSucceeded
ContextCompactionFailed
```

Then ACP/control surfaces can report consistent results while preserving manual behavior.

## Acceptance criteria

- Document current manual and in-turn compaction paths.
- Decide immediate vs queued semantics for manual compaction.
- Add structured diagnostics for compaction lifecycle states.
- Ensure `/context compact`, ACP/control requests, and loop-driven compaction report compatible outcomes.
- Add tests for no-op, success, failure, and queued/skipped behavior.

## Deferred from

Deferred from ACP issue #128 release work. ACP turn-control telemetry should ship first; compaction diagnostics should follow as a focused patch.

## Open Questions
