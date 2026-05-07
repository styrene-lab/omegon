+++
id = "ebd1f29b-b32a-4d51-b1b8-841baf5b1746"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Native dispatch observability — surface Rust child progress to dashboard

## Intent

The Rust cleave orchestrator emits structured tracing::info! lines to stderr (child spawned, child completed, wave dispatching, merge phase, etc.) but the TS native-dispatch.ts wrapper treats them as opaque text — it forwards them to onProgress but never parses them into structured state updates. The dashboard footer already renders per-child status (icon, elapsed, recent activity lines) from sharedState.cleave.children[], but native dispatch never populates these fields during execution. Children stay as grey circles until the entire run completes and state.json is read back.

See [design doc](../../../docs/native-dispatch-observability.md).
