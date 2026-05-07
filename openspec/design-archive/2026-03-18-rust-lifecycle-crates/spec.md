+++
id = "57743c42-9274-4edf-be09-2c5564f51abe"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust lifecycle crates — design-tree + openspec as native Rust modules — Design Spec (extracted)

> Auto-extracted from docs/rust-lifecycle-crates.md at decide-time.

## Decisions

### Phase 1a: read-only lifecycle parsing + context injection. Phase 1b: full mutation tools when Rust becomes the interactive parent. (decided)

The Rust binary is currently a cleave child executor. Children need lifecycle *awareness* (what node is focused, what specs apply to their scope) but not lifecycle *mutation* (create nodes, archive changes). Building read-only parsing + ContextProvider first delivers value immediately — children get design context in their system prompts — without the complexity of the full mutation surface. The mutation tools (Phase 1b) are only needed when the Rust binary replaces bin/omegon.mjs as the interactive parent. This halves the initial implementation (~800-1200 LoC vs ~3000-4000 LoC) and avoids building write paths that won't be exercised until Phase 1 TUI bridge ships.

### Keep markdown files as source of truth — no lifecycle.db for Phase 1 (decided)

The TS extensions read/write markdown files with YAML frontmatter. Introducing a sqlite schema now would mean maintaining two representations (markdown for git + sqlite for queries) and a sync mechanism. Markdown-as-source-of-truth matches current behavior, is git-friendly, and the read-only parsing needed for Phase 1a is simpler against files than a database. lifecycle.db can be introduced in Phase 2+ when the full lifecycle engine (with query optimization, cross-node relationships, and ambient phase detection) warrants it.

## Research Summary

### What the Rust agent loop actually needs vs. what exists

**The critical insight: the Rust binary today is a cleave child. Cleave children don't need full design-tree/openspec tool support.** They need:

1. **Read-only awareness** — know which design node is focused, which openspec change is active, what specs apply to their scope
2. **Context injection** — inject relevant design decisions, spec scenarios, and constraints into the system prompt
3. **Ambient capture** — parse `omg:` tags from responses (ALREADY DONE in `lifecycle/capture.rs`)

The **ful…
