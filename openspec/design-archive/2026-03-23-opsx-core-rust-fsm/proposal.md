+++
id = "7e22425f-083c-4746-ae84-9a309eed2a08"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# opsx-core — Rust-backed lifecycle FSM for OpenSpec enforcement

## Intent

Replace markdown-as-source-of-truth with a Rust state machine that owns the lifecycle. Markdown becomes the UI/display layer, not the authority. Components: lifecycle FSM (statig), task DAG (daggy/dagcuter), spec validator (jsonschema + garde), state store (sled). Scoped to Omega (enterprise orchestrator), not Omegon (single-operator tool). The single-operator workflow stays git-native markdown; the fleet orchestration layer gets enforcement.

See [design doc](../../../docs/opsx-core-rust-fsm.md).
