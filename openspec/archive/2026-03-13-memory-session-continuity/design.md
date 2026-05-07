+++
id = "2a23dbe2-0a2e-42af-b580-a6a5000763db"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory: Session Continuity — Proactive startup injection and recency window — Design

## Architecture Decisions

### Decision: Session start loads three layers before first message: recent episodes + recency window + Architecture/Decisions core

**Status:** decided
**Rationale:** Current injection is reactive — waits for first user message then semantically queries. Fails for continuation questions that assume context. Solution: at session_start inject (1) last 3 episodes (narrative of what was worked on), (2) top 20 facts by last_reinforced DESC regardless of section (recency window), (3) Architecture + Decisions sections always. This reconstructs "where we were" before the user speaks.

### Decision: Semantic retrieval on first message augments the startup payload, not replaces it

**Status:** decided
**Rationale:** Proactive structural load (session start) and reactive semantic retrieval (first message) serve different purposes and must both run. Startup payload gives structural context. Semantic retrieval adds task-specific Constraints/Specs/Known Issues on top. Core sections move from [Constraints, Specs] to [Architecture, Decisions] — those are always loaded. Constraints and Specs are retrieved semantically when relevant.
