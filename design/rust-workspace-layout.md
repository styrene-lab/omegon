+++
id = "d15995a9-f45d-4a3a-95dd-db811703c16a"
kind = "design_node"
title = "Rust workspace layout — crate organization for the agent loop and feature crates"
status = "resolved"
tags = []
aliases = ["rust-workspace-layout"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
+++

# Rust workspace layout — crate organization for the agent loop and feature crates

## Overview

> Parent: [Rust-native agent loop — middle-out replacement of pi's orchestration core](rust-agent-loop.md)
> Spawned from: "Where does the Rust agent loop binary live in the repo — a new top-level Cargo workspace (e.g., `crates/omegon-agent/`), or inside the existing `omega/` planned workspace? This affects how Omegon feature crates (memory, design-tree, openspec, cleave) are organized as workspace members."

*To be explored.*

## Decisions

### Decision: Single Cargo workspace at `crates/` with core + feature crate members

**Status:** decided
**Rationale:** The Rust workspace lives at `crates/` in the Omegon repo, coexisting with the TypeScript `extensions/` and `vendor/` directories during the migration. Workspace members:

```
crates/
├── Cargo.toml              # workspace root
├── omegon/                  # binary crate — the agent loop entry point
│   └── src/
│       ├── main.rs          # CLI, startup, runtime assembly
│       ├── loop.rs          # agent loop state machine
│       ├── bridge.rs        # LlmBridge subprocess manager
│       ├── context.rs       # ContextManager
│       ├── conversation.rs  # ConversationState, decay, IntentDocument
│       ├── lifecycle/       # Lifecycle Engine (design, spec, decomposition)
│       └── tools/           # core tools (understand, change, execute, ...)
├── omegon-memory/           # feature crate
├── omegon-render/           # feature crate
├── omegon-view/             # feature crate
├── omegon-web-search/       # feature crate
├── omegon-local-inference/  # feature crate
└── omegon-mcp/              # feature crate
```

The `omegon` binary crate depends on all feature crates. Feature crates implement traits defined in a shared `omegon-traits` crate (or inline in the binary crate if the trait surface is small enough to avoid a separate crate). Lifecycle engine is inside the binary crate, not a separate crate, because it's core loop logic.

## Open Questions

*No open questions.*
