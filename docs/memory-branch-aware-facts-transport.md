+++
id = "6a6dd391-c93c-42e0-bd56-cfc989f56b8d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory branch-aware facts transport — isolate tracked facts.jsonl intent from branch-local runtime drift

## Overview

Explore how Omegon should manage tracked .pi/memory/facts.jsonl when operators move between branches in the Omegon repo. The goal is to preserve cross-machine portability and mergeable durable knowledge without letting branch-local session activity create unrelated dirty diffs or release blockers.

## Research

### Current baseline and pressure point

facts-jsonl-stability already established that .pi/memory/facts.jsonl must remain tracked, portable, and free of volatile reinforcement metadata. That solved runtime scoring churn, but branch workflow friction remains: when working on a feature branch inside Omegon, durable memory additions from normal exploration can still dirty the tracked file and block unrelated release/publish work. The remaining design problem is therefore branch semantics and export timing, not volatile field trimming.

### Non-goals inherited from prior design

The new design must preserve the existing facts-jsonl-stability constraints: do not untrack .pi/memory/facts.jsonl, do not rely on assume-unchanged or skip-worktree hacks, and keep import/export semantics backward-compatible and merge-friendly. Any branch-aware strategy has to layer on top of that transport contract rather than replacing it with git-local tricks.

### Current implementation semantics

Project-memory currently auto-imports `.pi/memory/facts.jsonl` into the SQLite DB on startup and auto-exports `store.exportToJsonl()` back to the tracked file on session end. The export is byte-stable for identical content, but it still runs as an ambient shutdown side effect rather than an intentional workflow boundary. This means any newly stored durable fact during routine branch work can dirty the tracked file even if the operator did not intend to update repository memory as part of that branch.

### Git and merge behavior already favor append-only transport

`.gitattributes` sets `.pi/memory/facts.jsonl merge=union`, and JSONL import pre-deduplicates same-id records before merging by durable identity/content hash. That makes the tracked file suitable as a shared transport snapshot or append-friendly durable export, but it does not answer when the file should be rewritten. The branch problem is therefore not merge mechanics; it is deciding when DB state becomes intentional repo state.

### Branch-aware model in one sentence

On branch switches and new sessions, tracked `facts.jsonl` should continue to seed the local DB automatically; after that point, the DB is the mutable working memory and the tracked file changes only when memory transport is explicitly exported or reconciled. This preserves portability without making ordinary branch activity indistinguishable from intentional repository-memory updates.

## Decisions

### Decision: Treat facts.jsonl as an explicit durable transport snapshot, not an always-live mirror of the SQLite DB

**Status:** decided
**Rationale:** The DB is the correct home for ambient, branch-local, and session-local memory mutation because it is the live working store. The tracked JSONL file exists for portability, reviewable durable knowledge, and cross-branch/cross-machine reconciliation. Rewriting the tracked file automatically on every session end collapses those two roles and makes incidental exploration appear as intentional repo state. The healthier boundary is import-on-startup from tracked transport into the DB, but export back to tracked transport only at explicit sync points such as a memory export command, lifecycle reconciliation, or an operator-approved checkpoint.

### Decision: Do not introduce branch-namespaced tracked overlays for facts.jsonl

**Status:** decided
**Rationale:** Branch overlays would preserve more local detail, but they would multiply tracked artifacts, complicate merge/review semantics, and create another lifecycle surface to reconcile. Omegon already has a local DB for branch/session-specific working memory. The missing capability is intentional export timing, not another tracked representation. One canonical tracked transport file plus a branch-local live DB keeps the model simpler and better aligned with existing merge=union import behavior.

### Decision: Release and lifecycle checks should classify facts.jsonl drift as memory transport state, not as a generic release blocker by default

**Status:** decided
**Rationale:** Durable lifecycle artifacts under `docs/` and `openspec/` must stay hard-gated because they define project truth for implementation and verification. `.pi/memory/facts.jsonl` is different: it is important, tracked, and portable, but incidental drift in that file should not block unrelated publish readiness unless the current branch explicitly intends to reconcile memory transport. Release tooling should surface it clearly and offer a deliberate resolution path, but it should not be treated the same as missing lifecycle documentation.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/index.ts` (modified) — Stop unconditional shutdown export to tracked facts.jsonl; keep startup import, add explicit export/sync entrypoints and optional dirty-state reporting.
- `extensions/project-memory/jsonl-io.ts` (modified) — Add helper(s) for explicit transport export and drift detection without always writing the tracked file.
- `extensions/project-memory/index.test.ts` (modified) — Cover explicit-export behavior, no-write-on-shutdown behavior, and drift classification/reporting.
- `extensions/openspec/lifecycle-files.ts` (modified) — Keep lifecycle artifact gating focused on docs/ and openspec/ while allowing memory transport state to be reported separately.
- `extensions/project-memory/README.md` (modified) — Document the new transport model: startup import, explicit export, and branch-friendly workflow expectations.

### Constraints

- Keep `.pi/memory/facts.jsonl` tracked and merge-friendly via the existing union/dedup transport model.
- Do not require branch-specific tracked files or git metadata hacks.
- Startup must still import tracked facts into the DB automatically so fresh clones and branch switches receive durable knowledge.
- There must be an explicit agent/operator path to export or reconcile durable memory back to `.pi/memory/facts.jsonl`.
- Release/readiness output should distinguish incidental memory transport drift from hard lifecycle-documentation failures.

## Acceptance Criteria

### Scenarios

- Given a tracked `.pi/memory/facts.jsonl` and an empty or stale local DB, when Omegon starts on a branch, then the DB is populated from the tracked transport file without requiring an explicit operator step.
- Given that the session creates or reinforces durable facts during ordinary branch work, when the session ends without an explicit memory export or reconciliation action, then `.pi/memory/facts.jsonl` remains unchanged on disk.
- Given that the operator or lifecycle flow explicitly exports or reconciles memory transport, when the DB contains new durable facts, then `.pi/memory/facts.jsonl` is rewritten deterministically to include those durable additions.

### Falsifiability

- This design is wrong if ordinary session shutdown on a feature branch still rewrites tracked `.pi/memory/facts.jsonl` without an explicit export/reconcile boundary.
- This design is wrong if a fresh clone or branch switch no longer receives durable knowledge because startup import from tracked transport was removed or made optional by default.
- This design is wrong if release/readiness checks still treat incidental `.pi/memory/facts.jsonl` drift as equivalent to missing `docs/` or `openspec/` lifecycle artifacts.

### Constraints

- `.pi/memory/facts.jsonl` remains tracked in git as the canonical portable transport artifact.
- The SQLite DB remains the live mutable memory store for branch-local and session-local work.
- The system must provide an explicit export or reconciliation path so intentional durable memory updates can still be committed.
