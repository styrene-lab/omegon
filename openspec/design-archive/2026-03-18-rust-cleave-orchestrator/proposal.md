+++
id = "f4c2db13-06d3-4e34-87ed-d57f0a7886fc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust cleave orchestrator — move child dispatch, worktree, and merge out of TypeScript

## Intent

Move the cleave child orchestration from extensions/cleave/dispatcher.ts (1360 lines of jiti-cached TypeScript) into a Rust binary. The TS dispatcher has been the source of every cleave reliability bug: jiti caching stale code, RPC pipe breaks from Node.js processes that refuse to die, native dispatch silently disabled by a module-level singleton cache. The Rust orchestrator spawns omegon-agent children directly, manages worktrees via git2/CLI, handles dependency wave ordering, and merges results. The TS cleave extension becomes a thin shell that calls the Rust binary and reports results.

See [design doc](../../../docs/rust-cleave-orchestrator.md).
