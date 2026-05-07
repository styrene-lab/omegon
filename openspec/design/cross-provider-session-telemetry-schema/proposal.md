+++
id = "6249d3c6-48ce-49a1-9f80-5e13c6fb3054"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cross-provider session telemetry schema for replay and inspection

## Intent

Define a provider-agnostic session/event log schema rich enough to support a claude-devtools-class inspector for Omegon across Anthropic, OpenAI-compatible providers, Codex, and local models. The schema should preserve replayability, token/cost/quota attribution, tool execution detail, context composition, model/provider switching, and subagent/cleave trees without binding the format to any single upstream provider's transcript structure.

See [Cross-provider session telemetry schema for replay and inspection design doc](../../../docs/cross-provider-session-telemetry-schema.md) for full context.
