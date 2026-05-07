+++
id = "6e39b96c-aa29-42c0-a441-9f4338a0dd13"
kind = "document"
title = "Rust cleave orchestrator — move child dispatch, worktree, and merge out of TypeScript"
status = "implemented"
tags = ["rust", "cleave", "orchestration", "jiti-kill"]
aliases = ["rust-cleave-orchestrator"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
+++

# Rust cleave orchestrator — move child dispatch, worktree, and merge out of TypeScript

## Overview

Move the cleave child orchestration from extensions/cleave/dispatcher.ts (1360 lines of jiti-cached TypeScript) into a Rust binary. The TS dispatcher has been the source of every cleave reliability bug: jiti caching stale code, RPC pipe breaks from Node.js processes that refuse to die, native dispatch silently disabled by a module-level singleton cache. The Rust orchestrator spawns omegon-agent children directly, manages worktrees via git2/CLI, handles dependency wave ordering, and merges results. The TS cleave extension becomes a thin shell that calls the Rust binary and reports results.

## Decisions

### Decision: Add cleave subcommand to omegon-agent binary, not a separate binary

**Status:** decided
**Rationale:** One binary, two modes: `omegon-agent --prompt` runs a single agent task, `omegon-agent cleave --plan plan.json` orchestrates multiple children. Shares the LLM bridge, tool infrastructure, and build pipeline. The TS extension spawns `omegon-agent cleave` and reads the result from state.json.

## Open Questions

*No open questions.*
