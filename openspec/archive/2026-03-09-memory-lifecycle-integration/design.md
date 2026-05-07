+++
id = "f0ee8220-fea8-4ade-9b3f-253ebe9fcde2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory integration with Cleave, Design Tree, and OpenSpec — Design

## Architecture Decisions

### Decision: D1: Use hybrid lifecycle-driven memory writes

**Status:** decided
**Rationale:** Project memory should ingest stable conclusions at lifecycle checkpoints rather than free-running over all workflow artifacts. Explicit structured lifecycle data may auto-store; inferred summaries should require operator confirmation.

### Decision: D2: Define source precedence as OpenSpec baseline → Design Tree decided state → Memory → session chatter

**Status:** decided
**Rationale:** Behavioral truth belongs first to archived baseline specs, rationale and constraints belong to decided design-tree state, and memory acts as a retrieval/distillation layer that points back to those sources instead of competing with them.

### Decision: D3: Start fact-first; make graph edges optional in v1

**Status:** decided
**Rationale:** The first version should prioritize reliable fact generation, deduplication, and supersession from structured lifecycle artifacts. Explicit high-confidence relationships may create edges opportunistically, but edge creation should not block or complicate the initial rollout.

## Research Context

### Recommended lifecycle integration model

Use a hybrid memory-write model tied to lifecycle checkpoints instead of free-running extraction from all workflow artifacts. The key principle is: store stable conclusions when lifecycle state converges, not provisional work-in-progress. High-signal sources are design-tree decisions/constraints, OpenSpec baseline requirements after archive, post-assess reconciliation deltas, and verified implementation outcomes. Low-signal sources that should remain ephemeral by default are proposal-stage intent, open questions, child-task chatter, intermediate cleave plans, and failed investigative branches. The best checkpoints are: (1) design decision recorded, (2) post-assess reconciliation discovers new constraints or supersedes prior assumptions, (3) OpenSpec archive merges a change into baseline, and (4) bug fixes resolve a known issue. At each checkpoint, the system should generate candidate memory facts, dedupe against existing memory, and either auto-store high-confidence facts or present an operator-reviewable summary depending on confidence and churn risk.

## File Changes

- `extensions/project-memory/` (modified) — Add lifecycle-ingestion entry points, candidate normalization, dedup/supersede logic, and optional approval flow for explicit vs inferred facts
- `extensions/openspec/` (modified) — Emit archive and post-assess lifecycle payloads for memory candidate generation
- `extensions/design-tree/` (modified) — Emit decision and constraint payloads suitable for memory ingestion
- `extensions/cleave/` (modified) — Emit verified execution outcomes and post-review durable findings, not raw child chatter
- `extensions/shared-state.ts` (modified) — If needed, add lightweight lifecycle event summaries for cross-extension coordination without duplicating source-of-truth ownership

## Constraints

- Auto-store only explicit structured lifecycle conclusions; inferred architecture summaries should require operator confirmation.
- Do not automatically persist proposal-stage intent, open questions, child-task chatter, or failed investigative breadcrumbs.
- Prefer pointer facts that reference authoritative docs/specs over duplicating long-form artifact contents in memory.
- Use supersede/archive flows to avoid duplicating facts when lifecycle artifacts evolve or are replaced.
