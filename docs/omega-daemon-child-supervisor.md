---
id: omega-daemon-child-supervisor
title: "Omega daemon child supervisor — reattachable cleave subprocess ownership across restarts"
status: exploring
parent: omega-daemon-runtime
tags: []
open_questions:
  - "What is the durable child-registry source of truth after daemon restart: `state.json` only, a separate supervisor journal/checkpoint file, or an embedded local store that tracks child command, pid, start time, worktree, and last-seen progress?"
  - "What qualifies a live subprocess as safely reattachable to the new daemon process: matching PID only, or PID plus command fingerprint/worktree/prompt path/start timestamp validation to avoid adopting unrelated reused PIDs?"
  - "How should monitor continuity work after restart: rebuild a fresh stderr/stdout monitor from inherited file descriptors (impossible today), switch children to log-file-backed monitoring, or accept that restarted daemons can only supervise termination/status and not reconstruct full live activity streams?"
  - "What is the authoritative post-restart cancel/reap contract: PID-signal fallback only, or a true adopted child handle managed by a persistent worker/supervisor subprocess that outlives daemon replacements?"
  - "What black-box proof is required before this can be called done: must an integration test demonstrate live child survival across daemon restart, successful reattachment/adoption, continued status visibility, and successful cancel or completion from the restarted daemon?"
  - "[assumption] The current degraded recovery model (persisted PID + status reconstruction + PID-based cancel fallback) is insufficient for daemon-grade supervision because it cannot recreate monitor tasks or stream continuity after restart; this assumption should be validated against the actual operator requirements before overbuilding a persistent supervisor tier."
  - "At what point does the child-supervisor control plane require Styrene Identity mTLS: can daemon v1 use same-host process ownership and local trust only, or does any cross-process/cross-host adoption of child ownership require mutual-authenticated workload identity from the start?"
dependencies: []
related: []
---

# Omega daemon child supervisor — reattachable cleave subprocess ownership across restarts

## Overview

Define the missing supervisor model for daemon-owned cleave children so Omegon can recover authoritative control after daemon restart instead of degrading to PID-only observation/kill. Scope includes durable child registry shape, restart-time reattachment/adoption rules, monitor-task reconstruction, activity stream continuity, authoritative cancel/reap semantics after restart, and end-to-end proof strategy for live child survival across daemon process replacement.

## Research

### Current proven baseline

Omegon now has spawn-time PID persistence, resume-time PID liveness reconciliation, explicit terminate/reap semantics, progress/web visibility for active child ownership, in-process per-child cancel registry, daemon/web transport child-cancel routing, and degraded post-restart recovery via persisted state + PID fallback kill. The missing capability is authoritative reattachment and continuous supervision after daemon restart, not basic spawn/cancel transport.

### First-pass architecture split

Phase 1 (local supervisor): same-host, same-user trust boundary; durable child registry on disk; restarted daemon reconstructs supervisor state and can cancel/reap by authoritative local ownership rules; no Styrene Identity dependency. Phase 2 (identity-first extension): supervisor/daemon become authenticated peers using Styrene Identity mTLS; adoption/cancel/reap authority is identity-bound and suitable for distinct control-plane processes or remote ownership.

### First-pass implementation target

Implement a local-only supervisor model that makes post-restart child management explicit rather than degraded: define the durable child-registry schema, define safe child adoption validation (PID plus command/worktree fingerprint), define what supervision continuity is possible after restart, and add end-to-end proof for restart + child survival + cancel/completion. Do not require cross-host identity in this phase, but do not design APIs that assume local trust forever.

### Bootstrap-token foundation

Phase-1 supervisor authority should introduce a minimal local token/lease model: a per-daemon or per-run opaque token persisted in child registry metadata and checked during restart-path adoption alongside PID liveness and local execution fingerprint. The token is not a remote trust primitive; it is a same-host continuity marker that raises the bar above raw PID/path/model checks while preserving a migration path to identity-backed leases later.

## Decisions

### Supervisor v1 is local-only and same-host trusted, not identity-first

**Status:** decided

**Rationale:** The immediate requirement is robust internal cleave ownership on one machine. Same-host, same-user process ownership is sufficient for v1 supervisor semantics and keeps the implementation tractable. Pulling Styrene Identity mTLS into the first supervisor slice would slow delivery and mix process-lifecycle work with distributed trust work before the local contract is fully proven.

### Styrene Identity mTLS is the preserved phase-2 authority model for cross-process or cross-host supervisor peers

**Status:** decided

**Rationale:** Once child ownership can be adopted, cancelled, or reaped by a distinct control-plane peer beyond same-host local trust, PID/process ownership is not an adequate authority model. Mutual-authenticated workload identity is the correct boundary for distributed supervisor authority, but it should layer on top of a proven local supervisor contract rather than replacing it prematurely.

### Supervisor v1 uses a simple local bootstrap token/lease, not Styrene Identity

**Status:** decided

**Rationale:** The local supervisor phase needs stronger authority continuity than PID/path/model heuristics, but pulling in Styrene Identity now would prematurely couple restart-path child supervision to an unfinished distributed identity system. A simple local bootstrap token/lease persisted with child metadata provides a minimal lineage/ownership signal for same-host recovery, leaves transport/security semantics explicitly degraded, and creates a clean seam for later replacement by Styrene Identity-backed authority.

## Open Questions

- What is the durable child-registry source of truth after daemon restart: `state.json` only, a separate supervisor journal/checkpoint file, or an embedded local store that tracks child command, pid, start time, worktree, and last-seen progress?
- What qualifies a live subprocess as safely reattachable to the new daemon process: matching PID only, or PID plus command fingerprint/worktree/prompt path/start timestamp validation to avoid adopting unrelated reused PIDs?
- How should monitor continuity work after restart: rebuild a fresh stderr/stdout monitor from inherited file descriptors (impossible today), switch children to log-file-backed monitoring, or accept that restarted daemons can only supervise termination/status and not reconstruct full live activity streams?
- What is the authoritative post-restart cancel/reap contract: PID-signal fallback only, or a true adopted child handle managed by a persistent worker/supervisor subprocess that outlives daemon replacements?
- What black-box proof is required before this can be called done: must an integration test demonstrate live child survival across daemon restart, successful reattachment/adoption, continued status visibility, and successful cancel or completion from the restarted daemon?
- [assumption] The current degraded recovery model (persisted PID + status reconstruction + PID-based cancel fallback) is insufficient for daemon-grade supervision because it cannot recreate monitor tasks or stream continuity after restart; this assumption should be validated against the actual operator requirements before overbuilding a persistent supervisor tier.
- At what point does the child-supervisor control plane require Styrene Identity mTLS: can daemon v1 use same-host process ownership and local trust only, or does any cross-process/cross-host adoption of child ownership require mutual-authenticated workload identity from the start?
