# HarnessStatus contract — unified status surface for TUI, web dashboard, and bootstrap

## Intent

A single Rust struct (HarnessStatus) that captures the complete observable state of the harness — active persona/tone, MCP servers, secret backend, inference backends, container runtime, context class, memory stats. The TUI renders it in the footer and settings overlay. The web dashboard reads it via WebSocket. The bootstrap prints it once at startup. One source of truth, multiple consumers. This is the UI surface contract for everything built in the persona/MCP/secrets/inference design work.

See [design doc](../../../docs/harness-status-contract.md).
