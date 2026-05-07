+++
id = "797f4b85-666a-4e18-b89b-9c8697685084"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Guardrails as Capability Probes — Omegon runtime dependency checks

## Overview

Guardrails exist to tell users at session start which Omegon features won't work in their current environment. They are NOT project linters. The correct scope is: "is the tooling Omegon depends on available?" — ollama for local inference, d2 for diagram rendering, pandoc for document viewing, etc. Skill-frontmatter-driven project checks (mypy, tsc against user dirs) were a wrong abstraction and should be removed.

## Decisions

### Decision: Health check drives off DEPS registry, not discoverGuardrails()

**Status:** decided
**Rationale:** bootstrap/deps.ts already has the canonical, declarative capability registry (ollama, d2, pandoc, etc.) with check() functions and tier classification. The session_start health check should import DEPS, filter to core+recommended, run check() for each, and report missing ones. discoverGuardrails() belongs in the cleave lifecycle (pre-check before child dispatch), not at session start.

## Open Questions

*No open questions.*
