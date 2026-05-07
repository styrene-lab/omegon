+++
id = "ba0ac8ac-9609-4c09-b036-8f9e321bf1a1"
kind = "document"
title = "Runner execution plane and update nudges"
status = "seed"
tags = []
aliases = ["runner-execution-plane-and-update-nudges"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["What is the canonical phase-1 mapping for `!` intents: non-empty `!cmd` should execute immediately via local shell runner, but should bare `!` suspend the TUI into the operator's real shell now or only scaffold the handoff contract in this pass?", "What is the minimum update-nudge policy for stale versions: startup banner only, background polling + footer/dashboard badge, and at what semantic version distance does the UI elevate from passive notice to stronger degraded-state warning?"]
related = []
+++

# Runner execution plane and update nudges

## Overview

Define the canonical `/run` execution surface and tighten operator update visibility. Scope includes near-real-time update nudges in the TUI, stale-version UI policy, direct `!cmd` execution without LLM mediation, and the first in-scope shell handoff semantics for bare `!`. This node should separate immediate implementation slices from longer-horizon runner architecture for OCI/Kubernetes backends.

## Open Questions

- What is the canonical phase-1 mapping for `!` intents: non-empty `!cmd` should execute immediately via local shell runner, but should bare `!` suspend the TUI into the operator's real shell now or only scaffold the handoff contract in this pass?
- What is the minimum update-nudge policy for stale versions: startup banner only, background polling + footer/dashboard badge, and at what semantic version distance does the UI elevate from passive notice to stronger degraded-state warning?
