+++
id = "ddb26880-5868-4b94-b3b0-4113912713e7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust-native agent loop — middle-out replacement of pi's orchestration core

## Intent

The current architecture is inverted: Omegon is a guest inside pi's TypeScript runtime. pi owns the agent loop, session lifecycle, tool dispatch, system prompt, compaction — and Omegon bolts features on top via the extension API. This means every piece of Omegon logic must route through pi's abstractions, pi's event model, pi's rendering pipeline.

See [design doc](../../../docs/rust-agent-loop.md).
