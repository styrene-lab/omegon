+++
id = "e94e65e3-528f-4bc9-8d8b-0b837a078c80"
kind = "document"
title = "Self-curated memory — agent writes to its own durable knowledge layer"
status = "deferred"
tags = ["rust", "memory", "autonomy", "self-improvement", "mind"]
aliases = ["self-curated-memory"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = ["How does self-curated memory interact with the existing mind system's section taxonomy (Architecture, Decisions, Constraints, etc.)? Is it a separate section with its own decay, or a different trust tier within the same sections?", "Should the operator see self-curated facts in the dashboard and be able to promote them to operator-curated (authoritative) status?"]
parent = "omega-memory-backend"
priority = "3"
+++

# Self-curated memory — agent writes to its own durable knowledge layer

## Overview

The agent should be able to write back to its own memory — not just read facts injected by the harness. This is an additional layer on top of the existing mind system: operator-curated facts (memory_store/supersede) remain authoritative, but the agent can maintain its own working knowledge file (e.g. ~/.omegon/mind/self.md or a dedicated SQLite collection). Use cases: (1) agent discovers a project convention and persists it for future sessions, (2) agent records lessons learned from failed attempts, (3) agent saves reusable workflows or command patterns. The self-curated layer should be clearly separated from operator-curated facts — different trust level, faster decay, and the operator can review/prune it. OpenCrabs does this with MEMORY.md (agent-writable) vs SOUL.md (human-owned). The mind system already has section-based organization and decay policies — self-curated facts could be a new section with aggressive pruning.

## Open Questions

- How does self-curated memory interact with the existing mind system's section taxonomy (Architecture, Decisions, Constraints, etc.)? Is it a separate section with its own decay, or a different trust tier within the same sections?
- Should the operator see self-curated facts in the dashboard and be able to promote them to operator-curated (authoritative) status?
