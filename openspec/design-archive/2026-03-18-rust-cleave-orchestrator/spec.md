+++
id = "2d268847-8726-42b0-a7fb-476d569f0367"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust cleave orchestrator — move child dispatch, worktree, and merge out of TypeScript — Design Spec (extracted)

> Auto-extracted from docs/rust-cleave-orchestrator.md at decide-time.

## Decisions

### Add cleave subcommand to omegon-agent binary, not a separate binary (decided)

One binary, two modes: `omegon-agent --prompt` runs a single agent task, `omegon-agent cleave --plan plan.json` orchestrates multiple children. Shares the LLM bridge, tool infrastructure, and build pipeline. The TS extension spawns `omegon-agent cleave` and reads the result from state.json.
