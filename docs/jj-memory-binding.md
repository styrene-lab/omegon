---
id: jj-memory-binding
title: Bind memory facts and episodes to jj change IDs
status: exploring
parent: git-harness-integration
tags: [architecture, memory, jj, knowledge-graph]
open_questions: []
---

# Bind memory facts and episodes to jj change IDs

## Overview

Memory facts are created during specific points in the commit graph but have no VCS anchor. Adding jj_change_id to facts and episodes enables: (1) temporal queries — what did we know when we made this decision? (2) fact provenance — which change introduced this knowledge? (3) knowledge graph edges that mirror the commit graph — facts connected by the changes that link them. (4) stale fact detection — if a fact's originating change was rebased away, the fact may be stale.

## Research

### The mapping — what jj change IDs give the memory system

**Current state:** Facts have `id`, `mind`, `section`, `content`, `date`, `confidence`. Episodes have `id`, `date`, `title`, `narrative`. Neither has any VCS context.

**What adding `jj_change_id` enables:**

### 1. Fact provenance

Every fact gets a `jj_change_id` recording which change created it. When you ask "why does this fact exist?" you can trace it back to the exact commit that introduced it — and that commit has a message explaining the context.

### 2. Temporal knowledge queries

"What did we know about vault security when we decided on PathPolicy?" — query facts created at or before the PathPolicy decision's change ID. The jj commit graph gives you the causal ordering that dates alone can't (two facts created on the same day may have a before/after relationship in the graph).

### 3. Stale fact detection

If a fact was created during change `rqwztvoy` and that change was later abandoned (jj abandon), the fact's provenance is broken — it may reference code that no longer exists. The memory system could flag facts whose originating change is no longer in the graph.

### 4. Design-to-knowledge edges

Design tree nodes already have `jj_change_id`. Facts created during a node's implementation have their own change IDs. The commit graph shows which changes are descendants of the design decision — automatically linking facts to the design context that motivated them. No manual `memory_connect` needed.

### 5. Session episodes anchored to operations

jj's operation log records every mutation. An episode narrative maps to a sequence of operations. Instead of a flat date-based episode, the episode could reference the operation range — enabling "replay" of what happened structurally, not just narratively.

### Implementation approach

**Minimal (do this now):**
- Add `jj_change_id?: string` to the fact JSONL schema
- `memory_store` captures the current jj change ID when creating facts
- `memory_query` / `memory_recall` can filter by change ancestry
- Episodes capture the jj change ID range (start_change, end_change)

**Future (design-graph unification):**
- The knowledge graph edges mirror the commit DAG — facts connected by shared change ancestry
- Design tree nodes, memory facts, and episodes all share the same identifier space (jj change IDs)
- Querying "everything related to vault-fail-closed" traverses both the commit graph and the knowledge graph simultaneously

## Decisions

### Decision: Capture jj_change_id on every memory_store and episode creation

**Status:** decided
**Rationale:** The cost is one CLI call per store operation (jj log -r @ -T change_id.short()). Facts are created infrequently — maybe 5-20 per session. The value is permanent provenance anchoring. The change ID is added as an optional field (backward compatible — old facts without it still work). Episodes already capture session date; the change ID adds graph-structural context.

## Open Questions

*No open questions.*
