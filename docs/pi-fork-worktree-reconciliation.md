---
id: pi-fork-worktree-reconciliation
title: pi-mono local fork worktree reconciliation
status: resolved
parent: singular-package-update-lifecycle
open_questions: []
---

# pi-mono local fork worktree reconciliation

## Overview

> Parent: [Singular package integration and full-lifecycle update parity](singular-package-update-lifecycle.md)
> Spawned from: "Which currently vendored pi-mono modifications are intended local fork work that should survive integration, and which are stale or conflicting with Omegon mainline behavior?"

*To be explored.*

## Research

### Recovered local fork delta after unstash

The previously stashed local pi-mono work mostly reapplied cleanly onto the fork's local `main`. Surviving intended deltas appear to be: provider/auth handling changes in `packages/ai/src/providers/anthropic.ts`, session/auth updates in `packages/coding-agent/src/core/{agent-session,auth-storage}.ts`, tool rendering work in `packages/coding-agent/src/modes/interactive/components/tool-execution.ts`, clipboard package fallback in `packages/coding-agent/src/utils/clipboard-native.ts`, and a richer diff renderer in `packages/coding-agent/src/modes/interactive/components/diff.ts`. The diff renderer conflict came from upstream simplifying the file while the stashed branch added syntax-highlighted context lines and line/background-aware diff tinting.

## Decisions

### Decision: Prefer the richer local diff renderer over the simplified upstream variant during unstash reconciliation

**Status:** decided
**Rationale:** The stashed local version preserves enhanced Omegon/Alpharius diff rendering behavior: syntax-highlighted context lines, line-level added/removed background tinting, and dedicated highlight colors for intra-line changes. The conflict was isolated to the comment/import region, so reconciling in favor of the richer implementation keeps intended fork behavior without discarding the rest of the recovered local work.

## Open Questions

*No open questions.*
