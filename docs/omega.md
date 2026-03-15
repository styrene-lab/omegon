---
id: omega
title: Omega — Rust execution engine and project intelligence daemon
status: exploring
related: [markdown-viewport, pikit-auspex-extension]
tags: [rust, architecture, cleave, lifecycle, dioxus, execution-engine, strategic]
open_questions:
  - "What is the exact HTTP API surface between Omega and the auspex bridge — REST with typed request/response envelopes, or a more structured RPC protocol like JSON-RPC or Cap'n Proto?"
  - "Does Omega subsume the mdserve fork binary directly (single Rust workspace with execution engine + HTTP server + Dioxus frontend), or are they separate crates/binaries composed at the Nix level?"
  - "How does Omega push cleave child progress back to Omegon's TUI dashboard in real time — WebSocket from Omega that the auspex bridge subscribes to and relays via pi.events, or does the bridge poll /api/cleave/status?"
  - "What is the instance lifecycle model — does Omega terminate when no pi session is connected (ephemeral), or does it run as a persistent daemon that outlives individual sessions (persistent)? Persistent enables background cleave execution and cross-session state, but requires daemon management (launchd/systemd or self-managed PID file)."
  - "For Ollama VRAM coordination across instances — cooperative catalog-based scheduling (each instance reads catalog before dispatching local children) vs. delegating all local inference to a single designated \"Ollama owner\" instance? Cooperative is simpler but relies on instances respecting the catalog; delegation is authoritative but asymmetric."
---

# Omega — Rust execution engine and project intelligence daemon

## Overview

Omega is a standalone Rust binary (and eventual library crate) that owns all deterministic process logic currently spread across TypeScript extensions in Omegon. The motivation is correctness, not performance: TypeScript's runtime type casting, absence of exhaustive pattern matching, lack of RAII cleanup guarantees, and stringly-typed state machines are the wrong tool for lifecycle management and process orchestration.

**What Omega owns:**
- Design tree file I/O and state machine (parse frontmatter, enforce valid transitions, mutate sections)
- OpenSpec lifecycle: stage computation, spec parsing, archive gating, reconciliation
- Cleave execution engine: worktree lifecycle (RAII via Drop), wave planning (petgraph toposort), child process dispatch (tokio::process), review loop orchestration
- Skills matching and guardrail execution
- Conflict detection and merge orchestration
- Lifecycle intelligence API (/api/*) consumed by Dioxus frontend and Omegon bridge
- Dioxus WASM frontend (visualization layer — design tree graph, OpenSpec funnel, cleave timeline)

**What stays in Omegon (TypeScript):**
- Tool registration with pi (registerTool, registerCommand) — pi API is TS-only
- TUI hooks (setFooter, setWidget, custom overlays)
- Event bus wiring (pi.events)
- sendMessage / sendUserMessage for LLM notifications
- The auspex bridge extension (~300 LOC) — receives tool calls, forwards to Omega via HTTP

**The boundary:**
Every tool handler in the auspex bridge is ~15 lines: deserialize params, POST to Omega, map response to pi tool result. Omega handles all business logic, state mutations, and process management. The TypeScript layer is a protocol adapter, not a logic layer.

**Relationship to markdown-viewport:**
Omega subsumes the markdown-viewport epic. The mdserve fork (~/workspace/ai/mdserve) is the starting point for Omega's HTTP server and document rendering. The lifecycle backend, Dioxus frontend, and Nix distribution work planned under markdown-viewport all become children of Omega.

## Research

### Multi-instance topology options

Three viable models for a single operator running multiple Omega instances:

**A. One Omega per project, flat catalog**
Each project root gets its own Omega process. A catalog file (~/.omega/catalog.json) registers running instances by path + port + pid. The auspex bridge checks for an existing instance on session_start, spawns one if absent. Any Omega (or a dedicated global Omega) can serve an aggregated Dioxus view by reading the catalog and federating /api/state requests across registered instances.

**B. One global Omega, multi-workspace routing**
Single Omega process manages all projects as named workspaces. Requests are routed by workspace ID. Enables native cross-project intelligence (shared memory graph, portfolio health view) but creates a single point of failure and couples unrelated project lifecycles.

**C. Omega controller + per-project workers**
A coordinator process (call it Omega Prime, or the Fabricator General) manages a pool of per-project Omega workers. The controller handles registration, routing, cross-project cleave dispatch, and the global Dioxus view. Workers handle per-project execution and lifecycle mutation. Matches the cleave pattern (coordinator spawning workers) but at the project granularity level.

**Assessment:**
Option A is the right default. It preserves process isolation (one project crashing doesn't affect others), is the simplest to implement, and the catalog gives you the discovery layer for a cross-project view without coupling instance lifecycles. Option C becomes relevant if cross-project cleave (fanning out a task across multiple repos simultaneously) is a real use case. Option B is the wrong default — global singletons are fragile and the workspace-routing layer adds complexity without proportional benefit.

### Resource coordination between instances

The real constraint isn't ports or CPU — it's Ollama VRAM. Multiple Omega instances dispatching cleave children simultaneously can all try to load different models into the same GPU. Without coordination, this causes thrashing (models evicted and reloaded) or OOM on smaller hardware.

The catalog is the natural coordination surface. Each registered Omega instance writes its current model usage to the catalog entry:
{ "path": "/work/proj-a", "port": 7842, "pid": 12345, "active_model": "qwen3:32b", "active_children": 3 }

A new Omega instance checks the catalog before dispatching local-tier children: if another instance already holds a heavy model, it either waits, uses a lighter model, or escalates to cloud. This is cooperative scheduling, not preemptive — it works for a single operator on one machine without requiring any kernel-level coordination.

The alternative is delegating all Ollama scheduling to a single Omega instance that owns the Ollama connection. Other instances forward local-tier inference requests to it. This centralizes the VRAM budget but creates a bottleneck and makes the catalog relationship asymmetric (one instance is "the Ollama owner").

### K8s execution model — implications for the worktree abstraction

The current cleave model treats a "child" as: git worktree (local filesystem) + pi subprocess (local process). Both are tightly coupled to the parent's machine.

The k8s insight reframes what a child is: an **agent unit** — a pi+Omega pair, where the pi process is the LLM agent and the Omega sub-instance is the execution engine providing that agent with tools, workspace management, and a result channel back to the parent. This pair can run locally or in a pod — the parent orchestrator doesn't care which.

**What this breaks in the worktree model:**
Git worktrees are a local optimization — they share the parent repo's object store via `.git/worktrees/`. In a k8s pod, there is no shared object store with the parent. A pod cannot `git worktree add` against a repo it doesn't have on disk. Three viable substitutes:

1. **Shared PVC**: Parent mounts the repo on a ReadWriteMany PVC. Child pods mount the same PVC and use `git worktree add` within it. Identical to local model. Works on single-node k8s (k3s, kind) or with NFS/CephFS PVCs. Fails on fully isolated multi-node k8s without network storage.

2. **Branch clone**: Parent pushes the child's branch to a git remote (origin, or a local Gitea/Soft Serve instance). Child pod clones that branch, does work, pushes back to origin. Parent fetches and merges. True isolation, any k8s cluster. Requires a git remote and credentials in pods.

3. **Git bundle transfer**: Parent creates a `git bundle` of the child's branch, uploads it to a shared location (S3, MinIO, a ConfigMap for small repos). Pod downloads, unbundles, works, creates a new bundle of its result, uploads. Parent merges. Fully self-contained, no remote needed. Higher latency.

**The right abstraction:**
A `WorkspaceBackend` trait in Omega that makes the workspace lifecycle (create, expose to child, harvest result, merge, cleanup) backend-agnostic:

```rust
trait WorkspaceBackend: Send + Sync {
    async fn create(&self, plan: &ChildPlan) -> Result<Box<dyn Workspace>>;
}

trait Workspace: Send + Sync {
    fn agent_unit_spec(&self) -> AgentUnitSpec;  // how to run pi+Omega here
    async fn harvest(&self) -> Result<ChildResult>;
    async fn merge_back(&self, target: &str) -> Result<MergeResult>;
    async fn cleanup(&self) -> Result<()>;  // called by Drop impl
}

struct LocalWorktreeBackend { repo: PathBuf }
struct K8sPodBackend { namespace: String, backend: K8sStorageBackend }

enum K8sStorageBackend { SharedPvc(String), BranchClone { remote: String }, Bundle { store: BundleStore } }
```

This is the key structural change relative to the current TS model: worktree.ts is not a module, it is the *only* backend. The abstraction makes local worktrees and k8s pods interchangeable from the orchestration layer's perspective.

**What stays the same:**
- The prompt content (child pi still gets the same task prompt)
- The wave planning algorithm (dependency ordering is independent of backend)
- The result harvesting contract (child pi writes to known files in its workspace; Omega reads them)
- The merge semantics (always a git merge of a named branch into the parent branch)

### The agent unit — pi+Omega as a paired execution primitive

The current model: parent Omegon spawns bare `pi -p --no-session` subprocesses. Each subprocess runs the full Omegon extension set, including all lifecycle tools, the dashboard, memory system, etc. — most of which are irrelevant for a child task.

The new model: each child is a **pi+Omega pair**:
- `pi` — LLM agent, receives the task prompt via stdin, uses tools via the auspex bridge
- `omega --worker` — local Omega instance, provides the child's tool surface, manages workspace, reports to parent

The child pi has a minimal extension set: just the auspex bridge pointing at localhost:PORT (the child Omega). The child Omega handles all tool calls for that child's task, manages its workspace, and streams progress/results back to the parent Omega.

**In local mode:**
```
parent omega → tokio::process → child omega (worker, port N)
                               → tokio::process → child pi (via auspex on localhost:N)
```
Child Omega manages the git worktree directly. Child pi has PI_CHILD=1 and no extensions except the auspex bridge.

**In k8s mode:**
```
parent omega → k8s Job API → pod spec:
                              container: omega --worker --parent=http://parent:7842
                              container: pi -p --no-session (auspex → localhost)
                              volume: workspace (worktree via PVC or cloned branch)
```
The pod is the unit of isolation. Parent Omega submits Jobs, watches for completion, fetches results.

**Why this matters for cleave correctness:**
Current child pi processes inherit the full Omegon extension set — including the dashboard extension, memory extraction, local-inference management, etc. These extensions run session_start handlers in each child, potentially triggering Ollama pulls, sqlite writes, and HTTP calls from within what should be a pure task-execution environment. The pi+Omega pair eliminates this: the child pi extension set is exactly one extension (auspex bridge), and the child Omega is a worker-mode instance with only the execution engine active (no Dioxus server, no dashboard, no memory system).

## Decisions

### Decision: Each cleave child is a pi+Omega agent unit, not a bare pi subprocess

**Status:** decided
**Rationale:** A bare pi subprocess inherits the full Omegon extension set — dashboard, memory, inference management — none of which are appropriate in a child task context. The pi+Omega pair gives each child a clean, minimal execution environment: pi handles LLM inference, child Omega handles tool execution and workspace management via the auspex bridge. This also enables k8s deployment as a natural pod spec (two containers, one volume).

### Decision: WorkspaceBackend trait abstracts local worktrees from k8s pod workspaces

**Status:** decided
**Rationale:** Git worktrees and k8s pod workspaces have the same logical contract (create branch, expose to child, harvest result, merge back, cleanup) but different physical implementations. A trait boundary keeps the orchestration layer backend-agnostic. Local worktree is the default backend; k8s backends (shared PVC, branch clone, git bundle) are swappable without changing the wave planner or dispatcher.

### Decision: Omega runs in two modes: server (full daemon) and worker (child execution only)

**Status:** decided
**Rationale:** The server mode is what the auspex bridge connects to — full HTTP API, Dioxus frontend, lifecycle management. The worker mode is what each cleave child runs — no HTTP server, no dashboard, no Dioxus, just the execution engine and a result channel back to the parent Omega. Same binary, different startup path: `omega serve` vs `omega worker --parent-url http://parent:PORT --workspace /path`.

## Open Questions

- What is the exact HTTP API surface between Omega and the auspex bridge — REST with typed request/response envelopes, or a more structured RPC protocol like JSON-RPC or Cap'n Proto?
- Does Omega subsume the mdserve fork binary directly (single Rust workspace with execution engine + HTTP server + Dioxus frontend), or are they separate crates/binaries composed at the Nix level?
- How does Omega push cleave child progress back to Omegon's TUI dashboard in real time — WebSocket from Omega that the auspex bridge subscribes to and relays via pi.events, or does the bridge poll /api/cleave/status?
- What is the instance lifecycle model — does Omega terminate when no pi session is connected (ephemeral), or does it run as a persistent daemon that outlives individual sessions (persistent)? Persistent enables background cleave execution and cross-session state, but requires daemon management (launchd/systemd or self-managed PID file).
- For Ollama VRAM coordination across instances — cooperative catalog-based scheduling (each instance reads catalog before dispatching local children) vs. delegating all local inference to a single designated "Ollama owner" instance? Cooperative is simpler but relies on instances respecting the catalog; delegation is authoritative but asymmetric.
