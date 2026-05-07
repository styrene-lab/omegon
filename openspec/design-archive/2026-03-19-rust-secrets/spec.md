+++
id = "5fd42df1-6431-4277-988c-de0a8d4e10c3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Port secrets system to Rust (redaction, recipes, tool guards) — Design Spec (extracted)

> Auto-extracted from docs/rust-secrets.md at decide-time.

## Decisions

### omegon-secrets crate with 4 layers, wired into dispatch_tools (decided)

Separate crate for testability. Redaction at the dispatch level catches all tool output. Guards fire before dispatch. Recipes stored in ~/.omegon/secrets.json. 18 tests. Matches TS 00-secrets layers 1-4,6. Layer 5 (local model scrub) deferred — same mechanism applies.
