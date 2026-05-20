+++
id = "5e256c7f-c69b-4bca-b5c4-29cbcb1a3651"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Kubernetes Workload Matrix

Omegon instances are workload-polymorphic: the same binary and manifest format runs across all k8s resource types. The manifest declares identity and capabilities; the k8s resource type declares the lifecycle pattern.

## Workload Matrix

### Legend

- **Impl**: Implementation status
  - `--` = not started
  - `stub` = subcommand exists but incomplete
  - `partial` = core path works, gaps remain
  - `done` = production-ready
- **Deps**: Prerequisites from other rows

### Core Commands

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| C1 | `omegon run` | Bounded headless subcommand. Process prompt → structured output → exit. | `done` | — | Subcommand implemented. Exit codes: 0=done, 1=error, 2=exhausted, 3=timeout |
| C2 | `omegon serve --manifest` | Daemon mode loading full identity from Pkl manifest | `partial` | C3 | `serve` exists; manifest loading does not |
| C3 | Manifest-as-config | `AgentManifest.pkl` is the sole config source. Subsumes profile.json, triggers, vox, secrets decl. | `partial` | — | Pkl schema exists; runtime loading partial (triggers only) |

### Configuration Surface

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| F1 | Profile from manifest | Settings (model, posture, thinking, max_turns) read from manifest instead of profile.json | `--` | C3 | `SettingsConfig` class exists in Pkl |
| F2 | Vox from manifest | Vox channel config (Discord, Slack, webhook, RNS) read from manifest | `--` | C3 | `VoxConfig` exists in Codex daemon; not in AgentManifest.pkl yet |
| F3 | Triggers from manifest | Trigger definitions read from manifest inline, not separate TOML files | `partial` | C3 | `TriggerDef` class in AgentManifest.pkl; daemon reads from .omegon/triggers/ |
| F4 | Secrets declaration | Manifest declares required/optional secrets; runtime validates before start | `partial` | C3 | `SecretsConfig` in Pkl; extension preflight exists; not wired for top-level agent |
| F5 | Persona from manifest | Persona directive and mind facts loaded from manifest bundle | `partial` | C3 | Persona system exists; manifest references directive path |

### Input/Output Contract

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| IO1 | Prompt input | `--prompt`, `--prompt-file`, stdin, or mounted file at known path | `partial` | C1 | `--prompt` and `--prompt-file` exist for headless |
| IO2 | Structured output | JSON result (files changed, commits, test results, summary) to stdout or `--output <path>` | `done` | C1 | RunResult JSON with status, turns, tokens, files_read, files_modified, duration, summary, error |
| IO3 | Exit codes | Semantic exit codes: 0=done, 1=error, 2=exhausted, 3=timeout | `done` | C1 | All four exit codes implemented in omegon run |
| IO4 | Progress signaling | Liveness/readiness probes for long-running; progress events for jobs | `--` | C1,C2 | serve has WebSocket events; run has nothing |

### Resource Bounds

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| B1 | Turn limit | `--max-turns N` hard ceiling | `done` | — | Exists and works |
| B2 | Wall-clock timeout | `--timeout <seconds>` kills the agent loop | `done` | C1 | Implemented via tokio timeout + CancellationToken, exit code 3 |
| B3 | Token budget | `--token-budget <N>` caps total input+output tokens | `partial` | C1 | Tracked and logged; enforcement (stopping the loop) not yet wired |
| B4 | Context class | `--context-class squad\|maniple\|clan\|legion` | `done` | — | Exists |

### Credential Management

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| S1 | k8s Secret → env | Standard k8s secretKeyRef in pod spec | `done` | — | Works today — ANTHROPIC_API_KEY etc. from Secrets |
| S2 | Secret preflight | Validate all declared secrets present before agent loop starts | `partial` | F4 | Extension secrets preflighted; top-level agent secrets not |
| S3 | No env leakage | Child processes (cleave/delegate) receive only declared secrets | `done` | — | `env_clear()` + safe inherit list exists |

### State & Persistence

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| P1 | Ephemeral workspace | Job runs in tmpfs or emptyDir, no persistence | `done` | — | `--no-session` exists |
| P2 | PVC workspace | StatefulSet/Deployment with persistent workspace + git repo | `partial` | — | Works if volume mounted at cwd; no explicit PVC awareness |
| P3 | Shared vault | Multiple agents mount same Codex vault PVC | `partial` | — | Vault sync exists; concurrent access not tested |
| P4 | Memory handoff | Job A exports facts.jsonl → Job B imports on startup | `partial` | — | `vault_sync::materialize_to_vault` + `import_from_vault` exist; not wired to run mode |
| P5 | Result artifact | Job writes structured result to volume for pipeline consumption | `--` | IO2 | Needs IO2 first |

### Container Infrastructure

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| K1 | OCI image | Multi-arch container image with omegon binary + bundled skills | `done` | — | Containerfile exists, deployed previously |
| K2 | Helm chart | Parameterized chart for Deployment/StatefulSet/DaemonSet/Job/CronJob | `--` | C1,C2,C3 | — |
| K3 | Health probes | `/api/healthz` and `/api/readyz` endpoints on serve mode | `done` | C2 | HTTP health probes with state machine (starting/ready/degraded/failed) |
| K4 | Graceful shutdown | SIGTERM → finish current turn → save session → exit | `done` | — | `shutdown` command via IPC/WebSocket; SIGHUP reload |
| K5 | Resource limits | Recommended CPU/memory requests and limits per workload type | `--` | K1 | Needs benchmarking |
| K6 | PTY terminal capability | Interactive `terminal` tool in containers | `partial` | K1 | Requires `/dev/pts` and writable config/transcript storage. Set profile `terminalTool: false` or `OMEGON_TERMINAL_TOOL=0` for hardened/headless pods. Bootstrap auto-hides the tool when PTY allocation or transcript storage is unavailable. |

### Pipeline & Orchestration

| # | Component | Description | Impl | Deps | Notes |
|---|-----------|-------------|------|------|-------|
| O1 | Job chaining | Job A output → Job B input via shared volume | `--` | IO2,P5 | k8s native via initContainers or Argo/Tekton |
| O2 | Cleave as k8s Job | Cleave children spawned as k8s Jobs instead of local processes | `--` | C1,K2 | Requires k8s API client in omegon |
| O3 | Delegate as k8s Job | Delegate workers spawned as k8s Jobs | `--` | C1,K2 | Same as O2 |
| O4 | Auspex fleet control | Auspex manages multiple k8s-deployed agents via WebSocket | `partial` | C2 | WebSocket control plane exists; k8s discovery does not |
| O5 | Workflow DAG | Multi-step agent workflows defined in manifest | `--` | O1,C3 | Future: Pkl-defined workflow graphs |

## k8s Resource Type Mapping

| Resource | Omegon Command | Lifecycle | Persistence | Vox | Triggers |
|----------|---------------|-----------|-------------|-----|----------|
| **Job** | `omegon run` | Bounded (turns/timeout) | Ephemeral or PVC | No | No (k8s schedules) |
| **CronJob** | `omegon run` | Recurring bounded | Ephemeral | No | No (k8s cron) |
| **Deployment** | `omegon serve` | Long-lived, restartable | emptyDir or PVC | Yes | Yes (internal) |
| **StatefulSet** | `omegon serve` | Long-lived, stable identity | PVC (required) | Yes | Yes (internal) |
| **DaemonSet** | `omegon serve` | Per-node, long-lived | hostPath or PVC | Yes | Yes (internal) |

## Implementation Priority

### Phase 1: Bounded Worker (Job/CronJob)
C1, IO1, IO2, IO3, B2, B3, K1, K6 profile default decision

### Phase 2: Manifest-Driven Daemon (Deployment/StatefulSet)
C2→C3, F1, F2, F3, F4, F5, K3, K4

### Phase 3: Fleet Orchestration (DaemonSet + pipelines)
O1, O2, O3, O4, K2, K5

### Phase 4: Workflow Engine
O5
