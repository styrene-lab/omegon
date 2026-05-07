+++
id = "f2a99c9d-c027-4473-9247-11be78b9279c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Interactive runtime supervisor — implementation plan

Related:
- [[interactive-runtime-supervisor]]
- [[omega-daemon-runtime]]
- [[auspex-omegon-launch-contract]]

## Goal

Refactor interactive Omegon so `main.rs` owns a command-driven turn supervisor with:
- one active turn per runtime
- a runtime-owned FIFO prompt queue
- explicit cancel-request vs turn-complete semantics
- actor-aware prompt/cancel identity
- TUI/IPC/web acting as adapters over runtime truth

---

## Current blocking seams

### 1. Prompt execution is awaited inline in `main.rs`

Current code paths:
- `core/crates/omegon/src/main.rs` — `TuiCommand::UserPromptWithImages`
- `core/crates/omegon/src/main.rs` — `TuiCommand::UserPrompt`

These arms currently:
- mutate canonical conversation state directly
- create a cancel token
- call `r#loop::run(...).await` inline
- only then return to the command loop

This is the primary architectural blocker.

### 2. TUI still owns queue behavior

Current code paths:
- `core/crates/omegon/src/tui/mod.rs` — `queued_prompts`
- `core/crates/omegon/src/tui/mod.rs` — `queue_prompt(...)`
- `core/crates/omegon/src/tui/mod.rs` — drain block near the bottom of `run_tui`

This queue now has FIFO semantics, but it is still owned by the TUI. That is only an intermediate safety fix, not the target architecture.

### 3. IPC prompt submission still maps directly to `TuiCommand::UserPrompt`

Current code path:
- `core/crates/omegon/src/ipc/connection.rs` — `submit_prompt`

The current `TurnInProgress` gate is safer than blind acceptance, but it is still a transport-level workaround rather than a queue-aware runtime contract.

---

## Phase 1 — introduce runtime command + supervisor skeleton in `main.rs`

### New types to add in `core/crates/omegon/src/main.rs`

1. `RuntimeActorKind`
2. `RuntimeActor`
3. `ControlSurface`
4. `PromptEnvelope`
5. `ActiveTurnPhase`
6. `ActiveTurn`
7. `TurnTaskResult`
8. `TurnOutcome`
9. `RuntimeCommand`
10. `InteractiveRuntimeSupervisor`

### Minimal initial field set

`PromptEnvelope`:
- `id`
- `text`
- `image_paths`
- `submitted_at`
- `submitted_by`
- `via`

`ActiveTurn`:
- `runtime_turn_id`
- `prompt`
- `started_at`
- `phase`
- `cancel`
- `task`

`InteractiveRuntimeSupervisor`:
- `agent: AgentSetup`
- `queue: VecDeque<PromptEnvelope>`
- `active_turn: Option<ActiveTurn>`
- `next_prompt_id`
- `next_runtime_turn_id`

### Intent of phase 1

Do **not** solve every command immediately.
Start with the narrow slice:
- enqueue prompt
- cancel active turn
- poll completion
- maybe start next turn

---

## Phase 2 — route prompt submission through runtime-owned queue

### Replace direct `TuiCommand::UserPrompt*` execution

Current direct execution arms in `main.rs` should be replaced.

Instead of:
- `push_user(...)`
- build loop config
- create cancel token
- `r#loop::run(...).await`

Do:
- build a `RuntimeCommand::EnqueuePrompt { ... }`
- hand it to the runtime supervisor
- return immediately to the outer command loop

### Canonical prompt ownership change

This is the critical boundary correction:
- TUI must stop appending canonical user messages before runtime acceptance
- runtime appends canonical user messages when a queued prompt is actually accepted/started

That means `core/crates/omegon/src/tui/mod.rs` should eventually stop doing:
- `conversation.push_user(...)`
- `conversation.push_user_with_attachments(...)`

before prompt dispatch.

For phase 2, it is acceptable to keep a transitional display-only echo in the TUI if necessary, but canonical history must move into runtime ownership.

---

## Phase 3 — spawn turns instead of awaiting inline

### Add a turn worker spawn helper

New helper in `main.rs`:
- `spawn_prompt_turn(...) -> ActiveTurn`

Responsibilities:
- snapshot model/max_turns settings for this turn
- append canonical user message to conversation state
- create cancel token
- create worker task
- return `ActiveTurn`

### Worker task contract

The worker should:
- execute one `r#loop::run(...)`
- produce `TurnTaskResult`
- avoid mutating queue/supervisor state directly

### Important constraint

Only the supervisor may:
- set/clear busy
- move Running -> Cancelling -> Idle
- dequeue and start next prompt

The worker task only reports outcome.

---

## Phase 4 — move TUI queue ownership out of `tui/mod.rs`

Once runtime queueing is live:
- remove `queued_prompts` from `App`
- remove `queue_prompt(...)`
- remove the bottom-of-loop queue drain block in `run_tui`

### TUI replacement behavior

When busy and the operator submits text:
- send `RuntimeCommand::EnqueuePrompt`
- optionally render a system message such as `Queued [n]` based on runtime feedback

But TUI no longer owns queue storage.

---

## Phase 5 — wire IPC and web to `RuntimeCommand`

### IPC
Current:
- `submit_prompt` checks `busy`
- sends `TuiCommand::UserPrompt`

Target:
- `submit_prompt` forwards to runtime command ingress
- short-term may still reject with `TurnInProgress`
- long-term should return queue-aware acceptance payload

### Web / daemon ingress
Current web forwarding path in `main.rs` converts:
- `WebCommand::UserPrompt(text)` -> `TuiCommand::UserPrompt(text)`

Target:
- `WebCommand::UserPrompt(text)` -> runtime enqueue command
- `WebCommand::Cancel` -> runtime cancel command

This removes the TUI command channel as the fake universal runtime API.

---

## Phase 6 — busy state becomes supervisor-derived

Current busy truth is updated in multiple places:
- TUI `TurnStart`
- TUI `AgentEnd`
- prior optimistic interrupt handling
- transport projections

Target:
- busy = `active_turn.is_some()`
- `Cancelling` still counts as busy
- transport and TUI projections read supervisor-derived state only

`core/crates/omegon/src/tui/dashboard.rs` session stats can remain as the projection target in phase 1, but the writer should become the supervisor, not the TUI.

---

## Phase 7 — identity-aware snapshots

After the runtime supervisor exists, extend exported state with:
- queue depth
- active turn phase (`running` / `cancelling`)
- active turn submitter identity
- cancel requester identity + timestamp

Potential targets:
- `core/crates/omegon/src/ipc/snapshot.rs`
- `core/crates/omegon-traits/src/lib.rs`
- web compatibility snapshots if needed

This data may not be rendered in the TUI immediately, but the runtime should expose it for Auspex.

---

## Commands to leave out of the first slice

Do **not** try to solve all runtime commands in the first implementation.

Safe first-slice focus:
- prompt enqueue
- cancel active turn
- next-turn start
- completion handling

Leave these as follow-on work if needed:
- queue-aware slash execution policies
- model-switch while cancelling edge cases
- queue persistence across process restart
- queue-aware IPC response schema

---

## Tests required

### Runtime supervisor tests

Add focused tests for:
1. prompt enqueue while idle starts immediately
2. prompt enqueue while running increases queue depth without dropping prompt
3. cancel while running transitions to cancelling, not idle
4. idle transition occurs only after worker completion
5. queued prompts start in FIFO order after completion
6. queued prompts preserve actor identity

### TUI regression tests

Keep/update tests for:
- no optimistic completion on interrupt request
- no local queue ownership once runtime queue lands

### IPC tests

Add/extend tests for:
- busy runtime returns `TurnInProgress`
- idle runtime accepts prompt submission
- future queue-aware response shape once protocol evolves

---

## Concrete file plan

### `core/crates/omegon/src/main.rs`

Primary refactor site.

Tasks:
- add supervisor structs/enums
- add runtime command ingress
- replace inline prompt execution
- add spawn + completion helpers
- route web/IPC/TUI prompt/cancel through runtime supervisor

### `core/crates/omegon/src/tui/mod.rs`

Tasks:
- remove runtime queue ownership after supervisor is live
- stop canonical prompt append before runtime acceptance
- keep rendering purely adapter-side

### `core/crates/omegon/src/ipc/connection.rs`

Tasks:
- stop mapping prompt submission directly to TUI commands
- route to runtime ingress
- evolve response schema later

### `core/crates/omegon/src/ipc/snapshot.rs`

Tasks:
- later extend snapshot with queue depth + active turn phase/identity

### `core/crates/omegon-traits/src/lib.rs`

Tasks:
- later add runtime snapshot/submit response types if IPC queue semantics are upgraded

---

## Success criteria

The refactor is successful when:

1. `main.rs` continues servicing commands while a turn is running
2. queued prompts are runtime-owned FIFO, not TUI-owned
3. cancel does not imply completion until the worker exits
4. busy state remains true during cancelling/unwind
5. TUI and Auspex observe the same runtime truth
6. actor identity is preserved on queued prompts and cancel requests

---

## Recommended first implementation slice

The most effective next coding slice is:

1. add `RuntimeCommand` + `InteractiveRuntimeSupervisor` in `main.rs`
2. move prompt queue ownership into the supervisor
3. spawn turns instead of awaiting inline
4. route cancel through the supervisor
5. leave TUI queue display as temporary compatibility if needed, but stop treating it as canonical

That slice should be the first PR/commit boundary before adding richer snapshot/export work.
