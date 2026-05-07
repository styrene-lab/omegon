+++
id = "91a54f45-b04e-47c4-a9e6-d2fe6cfe28c7"
kind = "document"
title = "Interactive runtime supervisor"
status = "decided"
tags = ["omegon", "runtime", "supervisor", "queueing", "auspex", "tui", "ipc"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Interactive runtime supervisor

Related:
- [[omega-daemon-runtime]]
- [[auspex-omegon-launch-contract]]
- [[omegon-daemon-child-supervisor-control-surface]]

## Purpose

Define the **runtime-owned supervision model** for interactive Omegon sessions so:

- prompt intake is no longer blocked by active turn execution
- queued prompts are owned by the runtime, not the TUI
- cancel is modeled as a request followed by authoritative completion
- TUI, IPC, web, and Auspex become adapters over runtime truth rather than competing owners of state

This document covers the **inner interactive/session runtime**, not outer process supervision. Auspex remains the outer supervisor for multi-instance orchestration.

---

## Layering

### Outer supervisor

**Auspex** or another launcher owns:
- Omegon process lifetime
- restart policy
- placement
- multi-instance orchestration
- cross-runtime operator experience

### Inner runtime supervisor

**Omegon** owns:
- active turn scheduling
- prompt queueing
- turn cancel semantics
- conversation/session mutation
- authoritative busy/idle/cancelling state
- runtime event emission

### Interface adapters

**TUI / IPC / web / Auspex attach surfaces**:
- send commands into the runtime
- render runtime state and events
- do not own canonical queue or turn state

---

## Core decisions

### Runtime owns the prompt queue

Queued prompts must live in the runtime supervisor, not the TUI. The TUI may render queue state later, but queue semantics must be shared across:
- local terminal use
- IPC/Auspex control
- web surfaces
- future daemon/event ingress

### Exactly one active turn per Omegon runtime

A single Omegon runtime executes at most one active agent turn at a time. Concurrency pressure is absorbed by the queue, not by parallel turns within one session runtime.

### Cancel is a request, not completion

A cancel action transitions the active turn from `running` to `cancelling`. The runtime becomes idle only when the active worker task actually exits.

### Supervisor task owns mutable runtime state

The runtime should be supervised by a single command-driven task that owns:
- `AgentSetup`
- prompt queue
- active turn handle
- state transitions

This avoids split-brain ownership between UI, IPC, and the active worker future.

### Every queued prompt and cancel request carries actor identity

Multi-surface control requires actor-aware transitions. The runtime must record:
- who submitted a prompt
- who requested cancel
- through which surface the request arrived

Without identity, queueing and cancel become operationally ambiguous in a TUI + Auspex world.

---

## Supervisor state model

```rust
enum ActiveTurnPhase {
    Running,
    Cancelling {
        requested_at: Instant,
        requested_by: RuntimeActor,
        via: ControlSurface,
    },
}
```

```rust
struct ActiveTurn {
    runtime_turn_id: u64,
    prompt: PromptEnvelope,
    started_at: Instant,
    phase: ActiveTurnPhase,
    cancel: CancellationToken,
    task: JoinHandle<TurnTaskResult>,
}
```

```rust
struct InteractiveRuntimeSupervisor {
    agent: AgentSetup,
    queue: VecDeque<PromptEnvelope>,
    active_turn: Option<ActiveTurn>,
    next_prompt_id: u64,
    next_runtime_turn_id: u64,
}
```

### Busy semantics

`busy = true` whenever `active_turn.is_some()`.

That includes both:
- `Running`
- `Cancelling`

Queue depth alone does not imply busy.

---

## Identity model

```rust
enum RuntimeActorKind {
    Tui,
    Auspex,
    IpcClient,
    WebClient,
    DaemonEvent,
    System,
}
```

```rust
struct RuntimeActor {
    kind: RuntimeActorKind,
    label: String,
}
```

```rust
enum ControlSurface {
    Tui,
    Ipc,
    WebSocket,
    HttpEventIngress,
    Internal,
}
```

### Why identity is required

The runtime must be able to answer:
- who queued this prompt?
- who requested this cancel?
- did the action come from the local TUI, Auspex, IPC, or daemon ingress?

This is necessary for operator trust, multi-surface debugging, and future policy enforcement.

---

## Prompt envelope

```rust
struct PromptEnvelope {
    id: u64,
    text: String,
    image_paths: Vec<PathBuf>,
    submitted_at: Instant,
    submitted_by: RuntimeActor,
    via: ControlSurface,
}
```

Initial queue policy is deliberately simple:
- FIFO
- in-memory only
- no overwrite
- no dedupe
- no priority tiers

---

## Runtime command model

All surfaces should submit the same semantic commands into the supervisor:

```rust
enum RuntimeCommand {
    EnqueuePrompt {
        text: String,
        image_paths: Vec<PathBuf>,
        actor: RuntimeActor,
        via: ControlSurface,
    },
    CancelActiveTurn {
        actor: RuntimeActor,
        via: ControlSurface,
    },
    SetModel {
        model: String,
        actor: RuntimeActor,
        via: ControlSurface,
    },
    RunSlashCommand {
        name: String,
        args: String,
        actor: RuntimeActor,
        via: ControlSurface,
        respond_to: Option<oneshot::Sender<SlashCommandResponse>>,
    },
    Compact {
        actor: RuntimeActor,
        via: ControlSurface,
    },
    ContextClear {
        actor: RuntimeActor,
        via: ControlSurface,
    },
    NewSession {
        actor: RuntimeActor,
        via: ControlSurface,
    },
    Shutdown,
}
```

---

## Supervisor loop shape

```rust
loop {
    tokio::select! {
        Some(cmd) = runtime_cmd_rx.recv() => {
            handle_command(cmd).await;
        }
        result = await_active_turn(), if active_turn.is_some() => {
            handle_turn_completion(result).await;
        }
    }

    maybe_start_next_turn().await;
}
```

This is the engine-style supervision loop:
- input ingestion remains live
- runtime mutation has one owner
- queueing and cancellation stay serialized and authoritative

---

## Command semantics

### `EnqueuePrompt`
- create `PromptEnvelope`
- append to runtime queue
- return immediately
- runtime, not TUI, owns canonical prompt acceptance

### `CancelActiveTurn`
- if idle: no-op / report nothing active
- if running: switch to `Cancelling`, fire cancel token
- if already cancelling: no-op / report already cancelling

### `SetModel`
- updates settings for the next turn
- does not mutate the active running turn

### `NewSession` / `ContextClear` / `Compact`
Initial policy:
- allowed only when idle
- rejected while running or cancelling

This keeps the first state machine simple and explicit.

---

## Turn start and completion policy

### Start
When idle and queue is non-empty:
- dequeue next prompt
- append canonical user message to runtime conversation state
- snapshot current model/settings for the turn
- spawn one worker task that runs `r#loop::run(...)`
- mark runtime busy

### Completion
When the worker exits:
- clear active turn
- mark runtime idle
- emit completion/failure/cancel result
- immediately start the next queued prompt if present

### Important
The TUI must not infer completion from keypresses. Only the supervisor may declare a turn complete.

---

## Interface implications

### TUI
The TUI should:
- submit `RuntimeCommand::EnqueuePrompt`
- submit `RuntimeCommand::CancelActiveTurn`
- render runtime events/state

The TUI should not:
- own queued prompt state
- append canonical user prompts before runtime acceptance
- clear busy on interrupt request alone

### IPC / Auspex
IPC should be a transport adapter over runtime commands and runtime snapshots.

Short-term, `submit_prompt` may still reject with `TurnInProgress` while the protocol only supports `AcceptedResponse { accepted }`.

Long-term, the IPC submit response should become queue-aware, including fields such as:
- `queued`
- `started_immediately`
- `queue_depth`
- `prompt_id`

### Web / daemon ingress
Web and daemon ingress should use the same runtime command model as TUI and IPC. They are additional surfaces, not alternate runtimes.

---

## Snapshot/export guidance

Even if not immediately rendered in the TUI, the runtime should be ready to project:
- `busy`
- `queue_depth`
- active turn phase (`running` / `cancelling`)
- active turn submitter identity
- cancel requester identity and timestamp

This is especially valuable for Auspex’s multi-runtime operator view.

---

## Migration plan

### Phase 1 — supervisor skeleton
- introduce command-driven runtime supervisor in `main.rs`
- move prompt queue ownership into runtime
- spawn turns instead of awaiting inline in the command loop

### Phase 2 — canonical prompt ownership
- runtime appends canonical user prompts
- TUI stops pre-owning prompt lifecycle

### Phase 3 — state export
- extend runtime/session snapshots with queue depth and active turn phase
- expose actor identity in active turn / cancel state

### Phase 4 — protocol evolution
- upgrade IPC submit response to queue-aware semantics

---

## Invariants

The following must remain true:

1. Interfaces do not own canonical queue semantics.
2. A cancel request does not imply turn completion.
3. A single Omegon runtime executes one active turn at a time.
4. Actor identity is attached to every queued prompt and cancel request.
5. Runtime truth is exported consistently across TUI, IPC, web, and Auspex.
