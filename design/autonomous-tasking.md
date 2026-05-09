# Autonomous Tasking — Roadmap

## Problem

`omegon run` exits when it finishes. There is no way to say "keep watching this repo, act when a PR opens" or "review staging logs every 4 hours." This blocks the entire class of long-running, event-driven agent workflows — the kind that justify running omegon as a persistent pod rather than a one-shot container.

Hermes solves this with a Python cron scheduler and a messaging gateway bolted onto the agent loop. The result is broad but brittle — 18 messaging adapters, 7 terminal backends, weekly 1000-commit releases that feel like velocity without architecture. We don't want to replicate that surface. We want to identify the missing primitive and build it correctly.

## Core Insight

The missing primitive is **a durable task that survives across agent turns and sessions.** Everything else — scheduling, triggers, watchdogs — composes on top.

Today's building blocks:
- `omegon run` — bounded headless execution with clean exit codes
- `omegon serve` — long-lived daemon with HTTP control plane
- `omegon acp --listen` — WebSocket agent server with health probes
- Checkpoints — turn-boundary crash recovery (JSONL append)
- Memory — typed persistent facts across sessions
- Skills — declarative task definitions with phase tracking
- Cleave — supervised multi-agent worktree orchestration
- **Flynt task system** — kanban boards, SQLite-backed tasks, git sync, decay scoring

What's missing between these:
1. **Executor bridge** — something that reads tasks from a board and runs agents against them
2. **Triggers** — conditions that mark tasks ready (time, webhook, file change, git event)
3. **Session continuity** — resuming a task where it left off, not starting fresh
4. **Supervision** — watchdog that restarts failed tasks, enforces budgets, reports health

## Architecture

### The TaskBoard Trait

Sentry does not own the task queue. It consumes one through an abstract interface. This decouples the executor from any particular task management system and lets the right tool own each concern.

```rust
/// A source of executable tasks for the sentry loop.
///
/// Implementations handle persistence, UI, and lifecycle management.
/// Sentry handles execution, triggers, budgets, and supervision.
pub trait TaskBoard: Send + Sync {
    /// Return tasks that are ready to execute.
    /// Implementations define what "ready" means — could be column-based
    /// (Flynt: tasks in "Scheduled" column), status-based, or tag-based.
    fn list_actionable(&self) -> Result<Vec<SentryTask>>;

    /// Atomically claim a task for execution. Returns false if already
    /// claimed (another sentry instance, or the same task still running).
    /// Implementations should use advisory locking or CAS to prevent
    /// double-execution.
    fn claim(&self, task_id: &str) -> Result<bool>;

    /// Release a claimed task without completing it (sentry shutting down,
    /// task preempted). Returns the task to actionable state.
    fn release(&self, task_id: &str) -> Result<()>;

    /// Mark a task completed with a result summary.
    fn complete(&self, task_id: &str, result: &TaskResult) -> Result<()>;

    /// Mark a task failed. The board decides whether to retry or
    /// dead-letter based on its own policy.
    fn fail(&self, task_id: &str, error: &TaskError) -> Result<()>;

    /// Get the execution spec for a task — prompt, model, bounds.
    fn task_spec(&self, task_id: &str) -> Result<TaskSpec>;
}
```

### SentryTask — The Wire Type

```rust
/// Minimal task representation that crosses the board boundary.
/// The board owns rich metadata (priority, decay, document refs).
/// Sentry only sees what it needs to execute.
pub struct SentryTask {
    pub id: String,
    pub name: String,
    pub priority: u8,           // 0=low, 1=medium, 2=high, 3=critical
    pub triggers: Vec<Trigger>,
    pub last_run: Option<DateTime<Utc>>,
    pub run_count: u32,
}

pub struct TaskSpec {
    pub prompt: String,
    pub model: Option<String>,
    pub skill: Option<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub token_budget: Option<u64>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
}

pub struct TaskResult {
    pub exit_code: i32,
    pub summary: String,
    pub tokens_used: u64,
    pub duration_secs: u64,
    pub session_id: String,     // for checkpoint resume
}

pub struct TaskError {
    pub message: String,
    pub retriable: bool,
    pub attempt: u32,
}

pub enum Trigger {
    Cron(String),               // "0 */4 * * *"
    Webhook(String),            // trigger name
    FileWatch {
        paths: Vec<PathBuf>,
        debounce_secs: u64,
    },
    GitEvent {
        events: Vec<GitEventKind>,
        poll_interval_secs: u64,
    },
    Manual,                     // only via API/CLI
}

pub enum GitEventKind {
    NewCommit,
    NewTag,
    NewBranch,
    PullRequest,
}
```

### Board Implementations

```
TaskBoard (trait)
  │
  ├── FileTaskBoard          — built-in, reads sentry.toml
  │   └── zero dependencies, works out of the box
  │   └── tasks defined inline in TOML config
  │   └── state tracked in local SQLite
  │
  ├── FlyntTaskBoard         — reads from Flynt vault via flynt-agent RPC
  │   └── the happy path when Flynt is mature
  │   └── tasks managed in Flynt UI, sentry just executes
  │   └── requires: task update ops, execution metadata fields
  │
  └── (future)
      ├── LinearTaskBoard    — pulls from Linear project via API
      ├── GitHubTaskBoard    — pulls from GitHub Issues with label filter
      └── McpTaskBoard       — generic MCP resource provider
```

### The Sentry Loop

```
omegon sentry [--config sentry.toml] [--flynt-vault ~/.flynt/vault]
  │
  ├── board: impl TaskBoard
  │     └── selected by config or CLI flags
  │
  ├── trigger evaluator (tick-based, ~10s)
  │     ├── cron: evaluates SentryTask.triggers against wall clock
  │     ├── webhook: POST /api/sentry/trigger/{name} enqueues matching tasks
  │     ├── file watch: notify crate, debounced, matches FileWatch triggers
  │     └── git poll: checks for new commits/PRs/tags on interval
  │
  ├── executor (bounded concurrency, default max_concurrent=3)
  │     ├── board.claim(task_id) — atomic claim before execution
  │     ├── board.task_spec(task_id) — get execution parameters
  │     ├── spawns run_task() with checkpoint resume
  │     ├── on success: board.complete(task_id, result)
  │     ├── on failure: board.fail(task_id, error)
  │     └── on shutdown: board.release(task_id) for all active tasks
  │
  ├── budget ledger (SQLite)
  │     ├── per-task token/cost tracking with rolling windows
  │     └── budget exhaustion → skip task until window rolls
  │
  └── control plane (reuses omegon serve infrastructure)
        ├── GET  /api/sentry/tasks         — list tasks + status from board
        ├── GET  /api/sentry/tasks/:id     — task detail + run history
        ├── POST /api/sentry/tasks/:id/run — force-run a task now
        ├── POST /api/sentry/trigger/:name — fire a named webhook trigger
        ├── GET  /api/sentry/budget        — budget utilization
        ├── GET  /api/healthz              — liveness (existing)
        └── GET  /api/readyz              — readiness (existing)
```

### Why Not Just Cron

External cron (`k8s CronJob`, `systemd timer`, `crontab`) can already invoke `omegon run`. That covers the simplest case. Sentry adds value where external schedulers can't:

- **Session continuity** — a cron job starts fresh every time. Sentry resumes from the last checkpoint, preserving conversation context and memory across invocations.
- **Event-driven triggers** — cron is time-only. Sentry reacts to webhooks, file changes, and git events without external glue.
- **Unified budget tracking** — Sentry enforces cumulative token/cost budgets across recurring runs of the same task, not just per-invocation.
- **Coordination** — multiple tasks share a concurrency limit and priority queue, preventing thundering-herd on a single pod.
- **Board integration** — tasks managed in Flynt (or Linear, or GitHub Issues) execute automatically without leaving the tool the team already uses.

### FileTaskBoard — Built-in Default

For users without Flynt, `sentry.toml` defines tasks inline:

```toml
[sentry]
max_concurrent = 3
log_retention_days = 30

[[task]]
name = "pr-review"
prompt = "Review all open PRs, leave comments on issues found"
model = "anthropic:claude-sonnet-4-6"
max_turns = 20
timeout_secs = 300

[task.trigger.cron]
schedule = "0 */4 * * *"

[task.trigger.webhook]
name = "github-pr"

[task.budget]
max_tokens_per_day = 500_000
max_cost_per_day_usd = 5.00

[[task]]
name = "staging-monitor"
prompt_file = "tasks/staging-check.md"
model = "anthropic:claude-haiku-4-5-20251001"
max_turns = 10
skill = "security"

[task.trigger.cron]
schedule = "*/30 * * * *"

[task.trigger.file_watch]
paths = ["deploy/", "k8s/"]
events = ["modify", "create"]
debounce_secs = 60
```

`FileTaskBoard` implements `TaskBoard` by:
- `list_actionable()` → all tasks defined in the TOML
- `claim()` / `release()` → advisory file lock on `.omegon/sentry/{task_name}.lock`
- `complete()` / `fail()` → append to SQLite run history
- `task_spec()` → read from the parsed TOML

State (run history, budget counters, last trigger time) lives in `.omegon/sentry/state.db`.

### FlyntTaskBoard — The Upgrade Path

When Flynt's task system matures, `FlyntTaskBoard` reads tasks from a Flynt board via the `flynt-agent` extension RPC:

- A designated "Sentry" board (or column) holds tasks the agent should execute
- Task description is the prompt; execution metadata lives in `[data.sentry]` frontmatter
- `claim()` → moves task to "Running" column via RPC
- `complete()` → moves to "Done", touches decay clock
- `fail()` → moves to "Failed" column or increments retry counter
- Flynt UI shows live task status without any sentry-specific dashboard

**Prerequisites in Flynt before this adapter is viable:**
1. Task update RPC (change status, column, priority, arbitrary fields)
2. `external_refs` persisted to SQLite (currently lost on restart)
3. Execution metadata field or frontmatter section (`[data.sentry]`)
4. Claim/release semantics (advisory lock or CAS on status field)

See: `flynt/design/sentry-integration.md` for the Flynt-side roadmap.

## Phases

### Phase 1: TaskBoard Trait + FileTaskBoard + Cron Trigger

The minimum viable sentry. Define the trait, implement `FileTaskBoard` backed by `sentry.toml`, add cron evaluation on a tick loop, execute tasks sequentially (max_concurrent=1). Results written to SQLite. Control plane endpoints for listing tasks and status.

**Primitives built**: `TaskBoard` trait, `FileTaskBoard`, cron evaluator, executor with checkpoint resume, sentry control plane routes, `omegon sentry` CLI subcommand.

**Validates**: Is the trait surface correct? Is the resume-from-checkpoint path reliable? Is the task lifecycle model right?

### Phase 2: Webhook + File Watch Triggers

Add event-driven triggers. Webhook endpoint accepts POST with optional JSON payload (injected into task prompt as context). File watch uses `notify` crate with configurable debounce. Git poll checks for new commits/PRs on interval.

**Primitives built**: trigger registry, event-to-task mapping, payload injection into prompt context.

**Validates**: Can the trigger evaluator handle bursty events without spawning duplicate tasks?

### Phase 3: Concurrent Execution + Budget Enforcement

Raise max_concurrent, add priority queue ordering, implement cumulative budget tracking (tokens/cost per day/week per task). Dead-letter handling with configurable alerting (initially: write to a file, log a tracing::error). Task-level concurrency locks prevent the same task from running twice simultaneously.

**Primitives built**: priority queue, budget ledger, concurrent executor with claim/release.

### Phase 4: FlyntTaskBoard Adapter

Implement the Flynt adapter once the prerequisites are met. This phase is gated on Flynt-side work (see `flynt/design/sentry-integration.md`).

**Primitives built**: `FlyntTaskBoard`, RPC bridge to flynt-agent extension.

### Phase 5: Sentry Dashboard

Extend the existing `omegon serve` web dashboard to show sentry state: task list from board, run history, next scheduled trigger, budget utilization, dead-letter items. Read-only view initially — mutations go through the API.

### Phase 6: Distributed Sentry (Future)

Multiple sentry instances coordinate via `claim()`/`release()` semantics. With `FileTaskBoard`, this means shared SQLite on a network volume or migrating to Postgres. With `FlyntTaskBoard`, Flynt's own conflict resolution handles coordination. Leader election for trigger evaluation.

## What We're NOT Building

- **Messaging gateway** — Omegon is not a chatbot platform. If someone wants Discord/Slack, they write a trigger webhook that bridges their platform to `/api/sentry/trigger/{name}`. One POST, not 18 adapters.
- **Remote execution backends** — We already have `acp --listen` for remote access and k8s for orchestration. "Run this on Modal" is a deployment concern, not an agent concern.
- **Self-improving skill loop** — Interesting research direction but architecturally premature. Skills should be authored and reviewed by humans until the agent loop is proven reliable enough to trust with self-modification. Revisit after sentry has production mileage.
- **Natural-language cron** — A cron expression is 5 fields. LLM parsing adds latency, ambiguity, and a failure mode for zero usability gain.
- **Our own task management UI** — Flynt already exists. `FileTaskBoard` is for headless/TOML-only users. Everyone else should use a real task board.

## k8s Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: omegon-sentry
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: omegon
        image: ghcr.io/styrene-lab/omegon:latest
        command: ["omegon", "sentry", "--config", "/etc/omegon/sentry.toml"]
        ports:
        - containerPort: 7842
        volumeMounts:
        - name: config
          mountPath: /etc/omegon
        - name: data
          mountPath: /var/lib/omegon
        livenessProbe:
          httpGet:
            path: /api/healthz
            port: 7842
        readinessProbe:
          httpGet:
            path: /api/readyz
            port: 7842
      volumes:
      - name: config
        configMap:
          name: omegon-sentry-config
      - name: data
        persistentVolumeClaim:
          claimName: omegon-sentry-data
```

## Dependencies

- `cron` crate (cron expression parsing) — mature, well-maintained
- `notify` crate (file watching) — already in ecosystem, battle-tested
- `rusqlite` — already a workspace dependency
- Existing: checkpoint system, `omegon run` executor, `omegon serve` control plane, health probes

## Open Questions

1. **Should sentry subsume `omegon serve`?** Or should it be a separate mode that optionally mounts the serve dashboard? Leaning toward sentry being a superset — `omegon sentry` implies `omegon serve` plus the task queue.

2. **Checkpoint granularity for resume** — Current checkpoints capture turn-boundary state. Is that sufficient for meaningful resume, or do we need conversation-level snapshots? Needs experimentation in Phase 1.

3. **Budget enforcement across restarts** — The budget ledger needs to survive pod restarts. SQLite on a PVC handles this, but what about multi-replica scenarios in Phase 6?

4. **Trigger ownership** — Should triggers live in the board (Flynt task metadata) or in sentry config? For `FileTaskBoard` they're inline in TOML. For `FlyntTaskBoard`, they could be in task frontmatter (`external_refs` or a dedicated field) or in a separate sentry-side mapping file. The latter is simpler but duplicates the task identity.
