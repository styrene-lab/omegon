+++
id = "1cc9af13-4106-45c8-bd4d-93e28a8b3336"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Codex Tier Routing — Provider-aware model selection for Cleave and tooling

## Intent

Make model-tier routing provider-aware so Anthropic and OpenAI/Codex can be used interchangeably across pi-kit tooling based on current operator budget posture.

## Problem

Current tier switching and Cleave dispatch are Claude-centric at the execution layer:
- `model-budget` resolves `haiku|sonnet|opus` through Anthropic-specific prefixes
- `effort` switches cloud drivers through Anthropic-only lookup
- Cleave relies on fuzzy aliases like `--model opus` rather than explicit model IDs

This makes it hard to use Codex as a first-class cloud provider, adapt to day-to-day subscription limits, or prefer cheap cloud over local for small background tasks.

## Proposed change

1. Keep planning-time tiers abstract (`local|haiku|sonnet|opus`) for compatibility
2. Add a shared provider-aware resolver that maps those tiers to concrete model IDs using session policy and registry inspection
3. Store lightweight operator-driven session routing policy in shared state
4. Ask the operator for provider posture before large Cleave burns when configured
5. Update operator-facing UX to use Servitor/Adept/Magos/Archmagos labels while preserving internal compatibility in phase 1

## Success criteria

- Cleave dispatch and review use explicit model IDs rather than provider-specific fuzzy aliases
- `set_model_tier` and effort routing can choose Anthropic or OpenAI based on session policy
- Large Cleave runs can ask the operator whether to favor Claude or GPT for the current session/run
- Background work prefers inexpensive cloud models over local when policy allows and matching cloud models are available
- Operator-facing UX uses provider-neutral thematic labels rather than Anthropic product names
