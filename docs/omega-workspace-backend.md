---
id: omega-workspace-backend
title: Omega WorkspaceBackend — local worktree vs. k8s pod storage
status: exploring
parent: omega
open_questions:
  - "For k8s pod workspaces, which storage backend is the v1 default: shared PVC (simpler, local k8s friendly) or branch clone (works on any cluster, requires a git remote)? The answer determines whether Omega needs a git remote configured in its k8s backend, or just a PVC claim name."
---

# Omega WorkspaceBackend — local worktree vs. k8s pod storage

## Overview

> Parent: [Omega — Rust execution engine and project intelligence daemon](omega.md)
> Spawned from: "For k8s pod workspaces, which storage backend is the v1 default: shared PVC (simpler, local k8s friendly) or branch clone (works on any cluster, requires a git remote)? The answer determines whether Omega needs a git remote configured in its k8s backend, or just a PVC claim name."

*To be explored.*

## Decisions

### Decision: Branch clone is the default k8s workspace backend; PVC is an optional backend for shared-storage clusters

**Status:** decided
**Rationale:** ReadWriteMany PVCs require NFS/Ceph infrastructure not available on typical local k8s (Docker Desktop, k3s, minikube default storage classes are ReadWriteOnce). Branch clone works on any cluster using only a git remote. For local k8s runs without a persistent remote, Omega starts an ephemeral git daemon (RAII-managed via Drop) for the duration of the cleave run. PVC backend is worth implementing for enterprise clusters with shared storage but is not the default.

### Decision: Workspace lifecycle and agent dispatch are separate traits — WorkspaceBackend does not know how the agent runs

**Status:** decided
**Rationale:** Current TS worktree.ts conflates filesystem setup with process management. In Rust, the Workspace trait owns branch lifecycle and result harvesting; AgentBackend (LocalProcess | K8sJob) owns how pi+Omega run against that workspace. The wave planner composes both without coupling either. This separation means a local worktree workspace can be paired with a LocalProcess backend (default) or a K8sJob backend running on a local node mount (for testing k8s paths without a full cluster).

## Open Questions

- For k8s pod workspaces, which storage backend is the v1 default: shared PVC (simpler, local k8s friendly) or branch clone (works on any cluster, requires a git remote)? The answer determines whether Omega needs a git remote configured in its k8s backend, or just a PVC claim name.

## Implementation Notes

### File Scope

- `src/workspace/mod.rs` (new) — WorkspaceBackend trait, Workspace trait, AgentUnit struct
- `src/workspace/local_worktree.rs` (new) — LocalWorktreeBackend — git worktree add/remove, RAII Drop impl
- `src/workspace/k8s_branch_clone.rs` (new) — K8sBranchCloneBackend — push branch, submit Job, fetch result bundle
- `src/workspace/k8s_pvc.rs` (new) — K8sPvcBackend — shared PVC mount, worktree within PVC
- `src/workspace/git_daemon.rs` (new) — EphemeralGitDaemon — RAII-managed git daemon for local k8s runs without a persistent remote
- `src/dispatch/agent.rs` (new) — AgentBackend enum (LocalProcess | K8sJob), AgentUnit composition

### Constraints

- Workspace trait must be object-safe (no generic methods, no Self return types)
- WorktreeGuard Drop impl must call git worktree remove --force even on panic
- EphemeralGitDaemon must bind to 127.0.0.1 only when used for local k8s — never exposed externally
- K8s Job names must be deterministic from cleave run ID + child label for idempotent resubmission
