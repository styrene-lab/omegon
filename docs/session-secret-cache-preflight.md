+++
id = "511e4bfc-fa8e-48b5-814e-5be40c036ecb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Session secret cache and startup preflight

## Overview

Add a session-scoped secret cache and startup preflight so Omegon resolves required secrets once at startup, caches them in-memory for the duration of the run, and never triggers surprise Keychain/UI prompts mid-session. The cache must support headless/cleave child processes via inherited resolved secrets or fail-fast behavior.

## Decisions

### Keep per-secret durable storage; add session-scoped resolved cache

**Status:** decided

**Rationale:** Per-secret storage in Keychain/recipes preserves least-privilege, simpler rotation, and lower blast radius. The runtime problem is not storage granularity but nondeterministic access timing. The correct fix is a session cache of resolved secrets, not a single serialized secrets blob.

### Interactive runs preflight required secrets; headless runs must not prompt mid-session

**Status:** decided

**Rationale:** Operator prompts are acceptable only at a deterministic boundary. Interactive TUI sessions should warm required secrets during startup. Headless/cchild/cleave sessions must inherit resolved secrets or fail fast before work begins, never block on Keychain/UI during tool execution.

## Open Questions

- What is the minimal required secret set to preflight for an interactive TUI session (active model provider, configured web search providers, update channel artifacts, etc.)?
- What is the safest transport for resolved secrets into child Omegon processes: inherited env vars, ephemeral snapshot file with strict permissions, or another IPC mechanism?
