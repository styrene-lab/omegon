+++
id = "c2101ff9-ecba-4b86-9487-cd598b210757"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory: Task-Completion Facts — Mid-term \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"what was done\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\" layer with fast decay

## Overview

> Parent: [Memory System Overhaul — Reliable Cross-Session Context Continuity](memory-system-overhaul.md)
> Spawned from: "How should mid-term task-completion facts work — what triggers them, what's their decay profile, and how do they differ from architectural facts?"

*To be explored.*

## Decisions

### Decision: Task-completion facts triggered by write/edit tool calls, not LLM discretion

**Status:** decided
**Rationale:** Tool-call interception is the most reliable trigger — doesn't depend on the LLM deciding to store. Hook write_file and edit tool calls (intentional changes, not reads or greps). Queue a lightweight fact: "Wrote [file] — [purpose]" where purpose is inferred from surrounding context (file path + recent tool call sequence). Fire-and-forget, non-blocking. Filter: only file writes/edits, not bash commands or reads.

### Decision: Task-completion facts decay in 1–5 days (business week maximum), no reinforcement extension

**Status:** decided
**Rationale:** Task-completion facts are ephemeral session receipts — breadcrumbs, not architecture. They must be gone within a business week regardless of reinforcement. Decay profile: halfLifeDays=2, reinforcementFactor=1.0 (reinforcement does NOT extend half-life — this is the key difference from architectural facts where reinforcement extends lifetime). A fact written Monday is gone by Friday whether or not it was seen again. This prevents task-completion facts from graduating into permanent memory simply by being reinforced.

### Decision: Task-completion facts live in a new "Recent Work" section, separate from Architecture/Decisions

**Status:** decided
**Rationale:** Mixing task-completion facts into Architecture would pollute durable knowledge with ephemeral receipts. A dedicated "Recent Work" section with its own decay profile keeps the separation clean. "Recent Work" is always injected at session start (it's part of the proactive payload) and decays structurally via DB job — no extraction agent involvement needed.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/project-memory/factstore.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
