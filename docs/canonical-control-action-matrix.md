---
id: canonical-control-action-matrix
title: Canonical control action matrix
status: exploring
parent: runtime-profile-status-contract
tags: [control-plane, commands, rbac, ipc, web, cli, slash]
open_questions:
  - Which slash actions remain TUI-only by design versus becoming remote-safe?
  - Should IPC grow explicit session.new and graph/state mutation methods, or continue to route many mutations through run_slash_command?
  - Which model-change intents split into edit versus admin (same-provider set vs provider switch) in the first enforcement pass?
dependencies: []
related: []
---

# Canonical control action matrix

## Overview

This document defines the **transport-neutral control surface** for Omegon.

The goal is to describe operator intent once, then bind multiple ingress
surfaces onto that canonical action set:

- slash commands
- CLI subcommands
- IPC methods
- web/daemon trigger kinds

This matrix is the future source of truth for:

- role mapping (`read`, `edit`, `admin`)
- transport capability exposure
- help/docs generation
- RBAC enforcement
- parity audits across slash/CLI/IPC/web surfaces

The matrix is intentionally defined **before** full enforcement so the command
surface can be normalized without encoding security policy into ad hoc strings.

---

## Live ingress inventory (evidence-backed)

This section records the **currently implemented** command-and-control ingress
surfaces, using the live code as evidence rather than intended architecture.
It exists to make drift visible.

### 1. Local TUI slash surface

Primary parse/normalization entrypoint:

- `core/crates/omegon/src/tui/mod.rs`
  - `canonical_slash_command(...)`

Current characteristics:

- acts as the de facto parser for several canonical command families
- supplies some user-facing command metadata/help text
- is still partly transport-coupled because remote slash execution reuses this
  parser directly

Examples of command families currently normalized here:

- context (`/context ...`)
- model (`/model ...`)
- thinking (`/think ...`)
- session (`/new`, `/sessions`)
- auth (`/login`, `/logout`, `/auth status`, `/auth unlock`)

### 2. TUI command bus surface

Primary dispatch path:

- `TuiCommand::BusCommand`
- `runtime_state.bus.dispatch_command(...)`
- special-case auth handling in `core/crates/omegon/src/main.rs`

Current characteristics:

- bus dispatch is not identical to slash parsing
- some auth actions bypass generic bus dispatch and are handled as special cases
- this is a drift vector because transport bindings can hit bus commands without
  going through the same validation path as slash

Examples:

- `auth_login`
- `auth_logout`
- feature-defined bus commands via `dispatch_command(...)`

### 3. Native IPC control plane

Primary transport contract:

- `core/crates/omegon-traits/src/lib.rs`
- `core/crates/omegon/src/ipc/connection.rs`

Current request methods implemented in IPC:

- `hello`
- `ping`
- `get_state`
- `submit_prompt`
- `cancel`
- `subscribe`
- `unsubscribe`
- `get_graph`
- `context_status`
- `context_compact`
- `context_clear`
- `new_session`
- `list_sessions`
- `auth_status`
- `model_view`
- `model_list`
- `set_model`
- `set_thinking`
- `skills_view`
- `skills_install`
- `plugin_view`
- `plugin_install`
- `plugin_remove`
- `plugin_update`
- `secrets_view`
- `secrets_set`
- `secrets_get`
- `secrets_delete`
- `vault_status`
- `vault_unseal`
- `vault_login`
- `vault_configure`
- `vault_init_policy`
- `cleave_status`
- `cleave_cancel_child`
- `delegate_status`
- `run_slash_command`
- `shutdown`

Current characteristics:

- IPC already has several **first-class canonical methods** (`submit_prompt`,
  `cancel`, `get_state`, `get_graph`, `shutdown`)
- IPC still tunnels a broad mutation surface through generic
  `run_slash_command`
- role checks already exist at this layer, but many are attached to the method
  or remote-slash classifier rather than a single transport-neutral command
  contract

### 4. WebSocket / daemon remote control surface

Primary transport handler:

- `core/crates/omegon/src/web/ws.rs`

Current inbound command types:

- `user_prompt`
- `model_view`
- `model_list`
- `set_model`
- `set_thinking`
- `auth_status`
- `context_status`
- `context_compact`
- `context_clear`
- `new_session`
- `skills_view`
- `skills_install`
- `plugin_view`
- `plugin_install`
- `plugin_remove`
- `plugin_update`
- `secrets_view`
- `secrets_set`
- `secrets_get`
- `secrets_delete`
- `vault_status`
- `vault_unseal`
- `vault_login`
- `vault_configure`
- `vault_init_policy`
- `cleave_status`
- `delegate_status`
- `cancel_cleave_child`
- `slash_command`
- `cancel`
- `request_snapshot`

Current characteristics:

- WebSocket mirrors IPC in spirit but not in vocabulary
- prompt submission is first-class (`user_prompt`)
- slash execution is still a generic tunnel (`slash_command`)
- snapshot/state refresh exists as a transport command rather than a canonical
  action binding

### 5. CLI operator surface

Primary entrypoint:

- `core/crates/omegon/src/main.rs`

Current characteristics:

- CLI exposes some first-class commands (`auth`, `serve`, `embedded`, startup
  flags, prompt submission)
- some CLI operations correspond cleanly to canonical actions
- others remain bootstrap/runtime wiring rather than reusable control intents

Examples already in scope for this matrix:

- `omegon auth status`
- `omegon auth login <provider>`
- `omegon auth logout <provider>`
- `--model`
- `--prompt` / `--prompt-file`

### 6. Event/read surfaces that participate in C2

These are not mutating command surfaces, but they are part of the control
contract because operators and controllers rely on them for observation:

- IPC `get_state`
- IPC `get_graph`
- IPC event `subscribe`
- WebSocket `request_snapshot`
- Web/dashboard state/graph APIs
- local slash read surfaces like `/status`, `/stats`, `/model`, `/context`

---

## Observed drift points

These are the concrete reasons the command surfaces are not yet unified.

### Minimal typed promotion now landed for cleave/delegate

`cleave` and `delegate` intentionally remain **partially** promoted.
The current typed surface is:

- `cleave status`
- `cleave cancel <label>`
- `delegate status`

These are exposed as canonical typed control requests across TUI, IPC, and
WebSocket.

What is **not** promoted on purpose:

- `cleave run ...`
- general delegate execution/invocation flows

Rationale:

- status/cancel are observation/control-plane intents and are safe to unify
- execution remains orchestration-owned and feature-local
- typed control should not absorb the cleave/delegate ownership boundary

This is the same pattern used for the earlier minimal-scope decision: promote
status and child cancellation, but leave execution on the feature-owned path.

### Generic slash tunneling still carries too much control traffic

Both IPC and WebSocket expose broad generic slash execution surfaces:

- IPC: `run_slash_command`
- WebSocket: `slash_command`

That is useful as a compatibility bridge, but it means:

- transport policy is attached to slash strings
- help/docs can drift from runtime behavior
- role classification depends on classifiers instead of canonical action ids
- parity testing becomes stringly-typed and fragile

### Parsing, help, policy, and execution are still split

Today, different layers own different pieces of the contract:

- TUI slash parser owns syntax for many commands
- command list/help text owns discoverability metadata
- remote slash executor owns remote acceptance behavior
- IPC/WebSocket handlers own transport gating and role checks
- bus/auth special cases own some execution semantics

That split is exactly how stale behavior like implicit `/logout anthropic`
slipped through.

### IPC and WebSocket do not yet share a transport-neutral method vocabulary

The two remote surfaces expose similar intents with different transport method
names:

| Intent | IPC | WebSocket |
|---|---|---|
| prompt submit | `submit_prompt` | `user_prompt` |
| slash tunnel | `run_slash_command` | `slash_command` |
| cancel | `cancel` | `cancel` |
| state refresh | `get_state` | `request_snapshot` |
| graph read | `get_graph` | no direct peer |
| shutdown | `shutdown` | no direct peer |

That is survivable, but it is not a unified C2 contract.

---

## Convergence target

The intended end state is:

1. **Canonical action ids own intent**
   - e.g. `auth.logout`, `prompt.submit`, `runtime.shutdown`
2. **Transport bindings map onto those ids**
   - slash, IPC, WebSocket, CLI
3. **Role requirements attach to canonical actions**
   - not to raw slash strings or transport-specific method names
4. **Help/discoverability derive from the same registry**
   - provider lists, aliases, usage, remote-safety
5. **Parity tests validate bindings across surfaces**
   - same operator intent, different transport wrappers

## Remaining slash inventory after dispatcher migration

The promoted control families are now largely dispatcher-backed across slash,
IPC, and Web. What remains is the residual slash surface in
`core/crates/omegon/src/tui/mod.rs::handle_slash_command(...)`.

### Dispatcher-backed slash families

These slash forms now primarily act as syntax veneers over
`ControlRequest -> execute_control(...)`:

- `/model`
- `/model list`
- `/model <spec>`
- `/think <level>`
- `/context`
- `/context status`
- `/context compact`
- `/context clear`
- `/context request <kind> <query>`
- `/context request {json}`
- `/context <class>`
- `/new`
- `/sessions`
- `/auth`
- `/auth status`
- `/auth unlock`
- `/login <provider>`
- `/logout <provider>`

### Slash commands that still appear intentionally TUI-local or UX-local

These are not yet on the canonical control rail, and many probably should stay
presentation-local unless there is a strong remote-use case:

- `/help`
- `/mouse [on|off]`
- `/persona ...`
- `/tone ...`
- `/detail ...`
- `/focus`
- `/copy [raw|plain]`
- `/tree`
- tutorial flow: `/tutorial`, `/demo`, `/next`, `/prev`
- note-taking flow: `/note`, `/notes`, `/checkin`
- quick exit aliases: `/exit`, `/quit`, `/q`

These are mostly UI affordances, editor/view state, or operator guidance.
They do not obviously belong in the remote control plane.

### Slash commands that are feature/workflow surfaces and need explicit decisions

These are not mere UI affordances. They represent real capability surfaces, but
have not yet been normalized into the canonical dispatcher:

- `/skills [list|install]`
- `/plugin ...`
- `/secrets ...`
- `/vault ...`
- `/cleave ...`
- `/milestone ...`
- `/update ...`
- `/init`
- `/migrate`
- `/chronos`
- `/delegate ...`
- `/splash`
- `/auspex ...`
- `/dash ...`

These need an explicit decision per family:

1. promote to canonical `ControlRequest`
2. keep slash-only by design
3. expose only via CLI or local TUI, not remote transports

### Auspex exposure policy

Auspex should expose a **full operator slash vocabulary** for attached Omegon
instances, including the local Auspex-managed instance. Terminal-only control
surfaces create operator friction and effectively hide capability.

That does **not** mean slash should remain the semantic owner.

Preferred execution stack:

1. slash UX in Auspex
2. parse to canonical `ControlRequest`
3. dispatch through shared control runtime
4. use a slash adapter only as a short-lived migration bridge while promotion is in flight

Decision rule for residual families:

- **Default** — promote to canonical `ControlRequest`
- **Migration only** — use a compatibility adapter briefly while the canonical action lands
- **Avoid** — require terminal-only execution for common operator workflows

Project stance at current maturity:

## WebSocket command assessment after typed control expansion

The WebSocket surface in `core/crates/omegon/src/web/ws.rs` should now be read
as a **JSON web command protocol normalized by Omegon into canonical control
requests**, not as a legacy one-off layer.

### Commands that already route through `ExecuteControl`

These WebSocket commands currently dispatch via `WebCommand::ExecuteControl`
and are therefore already on the canonical control rail:

- `model_view`
- `model_list`
- `skills_view`
- `skills_install`
- `plugin_view`
- `set_model`
- `switch_dispatcher`
- `set_thinking`
- `plugin_install`
- `plugin_remove`
- `plugin_update`
- `secrets_view`
- `secrets_set`
- `secrets_get`
- `secrets_delete`
- `vault_status`
- `vault_unseal`
- `vault_login`
- `vault_configure`
- `vault_init_policy`
- `cleave_status`
- `delegate_status`
- `auth_status`
- `context_status`
- `context_compact`
- `context_clear`

These should be treated as **canonicalized** unless and until the
transport-neutral action set changes.

### Commands that still bypass `ExecuteControl`

The following branches in `web/ws.rs` still use bespoke WebSocket-native
handling:

- `user_prompt`
- `slash_command`
- `cancel`
- `cancel_cleave_child`
- `request_snapshot`

### Classification: should stay transport-native

These commands are inherently session/transport mechanics rather than canonical
mutation/control actions:

- `user_prompt`
  - reason: operator/runtime input ingress into the active turn pipeline
- `cancel`
  - reason: interruption of the currently active turn, naturally coupled to the
    live session execution surface
- `request_snapshot`
  - reason: transport refresh / state pull, not domain control intent

These should **remain transport-native** unless Omegon explicitly chooses an
"everything is a command object" architecture.

### Classification: should migrate to `ControlRequest`

These commands represent real control intent and are not merely transport
mechanics:

- `cancel_cleave_child`
  - preferred target: `ControlRequest::CleaveCancelChild`
  - rationale: this is a canonical control mutation in the cleave/delegate
    family, not a transport concern
- `slash_command`
  - rationale: compatibility tunnel only
  - expected lifecycle: shrink over time as remaining slash families are
    promoted into typed control requests

### Current architectural conclusion

The WebSocket surface is now largely canonicalized. Remaining bespoke commands
are either:

1. **correctly transport-native** (`user_prompt`, `cancel`, `request_snapshot`)
2. **explicit migration debt** (`slash_command`, and `cancel_cleave_child` if it
   has not yet been fully normalized)

That means Auspex attach/manage readiness is no longer blocked on WebSocket
control-surface shape. Remaining work is cleanup and convergence, not missing
operator capability.

- Omegon is still early enough that command-surface churn is acceptable.
- We should prefer promotion over preserving legacy slash tunnels.
- Generic slash execution should be treated as transitional scaffolding, not a first-class long-term control surface.
- If a command represents real operator intent, the bias should be to give it a canonical action id rather than defend string-based compatibility.

### Residual family prioritization for Auspex

These families matter because operators are likely to need them from Auspex,
not just from a local terminal.

#### 1. `skills` / `plugin`

Why it matters:
- extension lifecycle and capability installation are operator-facing workflows
- forcing terminal use here would make Auspex feel incomplete

Recommended direction:
- expose full slash UX in Auspex immediately
- promote listing/status flows first only as an implementation sequencing choice
- promote install/remove/update flows next with explicit role gating
- do not preserve a permanent `skills` / `plugin` slash-only execution path

Target canonical actions (illustrative):
- `skills.view`
- `skills.install`
- `plugin.view`
- `plugin.install`
- `plugin.remove`
- `plugin.update`

#### 2. `secrets` / `vault`

Status:
- promoted to canonical `ControlRequest` routing in TUI, IPC, and WebSocket
- no longer depends on the bespoke `main.rs` slash-era `secrets` branch
- residual generic slash compatibility still exists repo-wide for other families, but not as the semantic owner for this family

Why it matters:
- operators need credential and backend visibility from the orchestration plane
- local-only terminal workflows here create high friction during setup and recovery

Current posture:
- `secrets.view`, `secrets.set`, `secrets.get`, `secrets.delete` are typed control actions
- `vault.status`, `vault.unseal`, `vault.login`, `vault.configure`, `vault.init_policy` are typed control actions
- transport role policy is currently conservative:
  - `secrets.*` are edit-scoped and local-only / non-remote-safe
  - `vault.status` is read-scoped but still local-only / non-remote-safe
  - mutating vault flows remain admin-scoped and local-only
- some vault actions are still instructional UX rather than fully interactive workflows

Recommended next work:
- add/maintain focused IPC and WebSocket tests for the typed methods
- keep sensitive value entry ergonomics in mind; some flows may need guided UI
- if remote execution is ever widened, do it intentionally with explicit policy and auditability
- do not preserve a permanent `secrets` / `vault` slash-only execution path

Canonical actions (landed):
- `secrets.view`
- `secrets.set`
- `secrets.get`
- `secrets.delete`
- `vault.status`
- `vault.unseal`
- `vault.login`
- `vault.configure`
- `vault.init_policy`

#### 3. `cleave` / `delegate`

Why it matters:
- these are orchestration-native capabilities and are especially relevant in
  Auspex, where multiple agents/instances are visible together
- requiring terminal fallback here would undermine the point of the control plane

Recommended direction:
- expose slash UX in Auspex immediately
- promote status/list/cancel/read flows before broad mutating orchestration
- preserve explicit role boundaries for spawning or dispatching work
- do not preserve a permanent `cleave` / `delegate` slash-only execution path

Target canonical actions (illustrative):
- `cleave.view`
- `cleave.run`
- `cleave.cancel_child`
- `delegate.status`
- `delegate.run`

#### 4. `auspex` / `dash`

Why it matters:
- these are self-management / browser-surface workflows
- they are awkward because Auspex is both the controller and, for the local
  instance, the attached environment

Recommended direction:
- distinguish self-management from attached-instance control explicitly
- prefer canonical actions for status/open flows that make sense remotely
- avoid transport loops where Auspex-in-Auspex style commands become ambiguous
- do not preserve a permanent `auspex` / `dash` slash-only execution path unless a command is proven to be pure local presentation state

Target canonical actions (illustrative):
- `dashboard.status`
- `dashboard.open`
- `auspex.status`

### Suggested promotion order

If we continue the dispatcher expansion, the recommended order is:

1. `skills` / `plugin`
2. `secrets` / `vault`
3. `cleave` / `delegate`
4. `auspex` / `dash`

Rationale:
- `skills` / `plugin` and `secrets` / `vault` are common operator workflows
- `cleave` / `delegate` are high-value orchestration capabilities
- `auspex` / `dash` need extra care because they blend controller and target roles

---

## Design rule

Canonical actions own intent. Ingresses are bindings.

Examples:

- `context.view` is the operator intent
  - slash: `/context`, `/context status`
  - future IPC/web binding may be added later
- `runtime.shutdown` is the operator intent
  - IPC: `shutdown`
  - web daemon trigger: `shutdown`
  - local TUI: `Quit`
- `session.new` is the operator intent
  - slash: `/new`
  - web daemon trigger: `new-session`

RBAC, transport support, and docs should attach to `context.view`,
`runtime.shutdown`, `session.new`, etc. — **not** directly to raw slash strings
or individual transport method names.

---

## Starter roles

### `read`
Read-only observation of state.

Allowed shape:
- inspect state
- inspect graph/status
- inspect model/context posture
- inspect available skills/models
- subscribe to events

Not allowed:
- mutate session
- submit work
- change runtime settings
- modify secrets
- shutdown/reset

### `edit`
Normal operator workflow mutation.

Allowed shape:
- submit prompts
- mutate session state
- compact/clear context
- tune model class / thinking level
- set or delete secret values
- run normal work-oriented slash commands

Not allowed:
- change provider
- change auth/login state
- shutdown runtime
- alter transport/runtime ownership posture

### `admin`
Runtime and control-plane authority.

Allowed shape:
- provider switching
- auth/login/logout/unlock
- runtime shutdown
- future transport/control-plane sensitive actions

Includes all `edit` and `read` capabilities.

---

### Contractual matrix snapshot (v0 live)

This table is the first-pass **contractual C2 matrix**. It records the live
canonical action ids already reflected in `core/crates/omegon/src/control_actions.rs`,
plus their current transport bindings.

Status legend:

- `canonical` — first-class transport binding to a named canonical intent
- `tunneled` — currently exposed through generic slash transport rather than a
  dedicated transport method
- `missing` — no implemented binding on that surface
- `divergent` — transport has a near-peer binding, but vocabulary/shape differs

| Canonical action | Slash | IPC | WebSocket | CLI | Role | Remote-safe | Status | Notes |
|---|---|---|---|---|---|---|---|---|
| `status.view` | `/status`, `/stats`, `/auspex status`, `/dash status` | `get_state`, `get_graph`, `subscribe`, `unsubscribe` | `request_snapshot` | — | read | yes | divergent | IPC and WebSocket expose similar observation intents with different method names and shapes |
| `prompt.submit` | normal prompt entry | `submit_prompt` | `user_prompt` | `--prompt`, `--prompt-file` | edit | yes | divergent | Same intent; remote method names differ |
| `turn.cancel` | local cancel flows | `cancel` | `cancel` | — | edit | yes | canonical | Best-aligned C2 action today |
| `runtime.shutdown` | local quit path | `shutdown` | missing | process exit / local shutdown path | admin | yes (IPC) | missing | WebSocket lacks first-class shutdown peer |
| `session.new` | `/new` | missing | missing | — | edit | yes in daemon trigger classifier | missing | Web daemon trigger exists conceptually, but not as a documented WebSocket/IPC method |
| `session.view.list` | `/sessions` | missing | missing | — | read | no | tunneled | Slash-only today; not remote-safe |
| `context.view` | `/context`, `/context status` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | read | yes | tunneled | Canonical action exists in classifier but lacks first-class remote method |
| `context.compact` | `/context compact` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | yes | tunneled | Remote-safe but still stringly |
| `context.clear` | `/context clear` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | yes | tunneled | Remote-safe but still stringly |
| `context.request` | `/context request ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | yes | tunneled | Good candidate for first-class C2 method later |
| `context.set_class` | `/context <class>` | tunneled via `run_slash_command` | tunneled via `slash_command` | `--context-class` (startup) | edit | yes | tunneled | Same intent spans slash/runtime config but no shared method binding |
| `model.view` | `/model` | tunneled via `run_slash_command` | tunneled via `slash_command` | startup logs only | read | yes | tunneled | Read-only but still slash-tunneled remotely |
| `model.list` | `/model list` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | read | yes | tunneled | Candidate for dedicated query surface |
| `model.set.same_provider` | `/model <model>` | tunneled via `run_slash_command` | tunneled via `slash_command` | partial via `--model` | edit | yes | tunneled | Current classifier distinguishes same-provider model changes |
| `provider.switch` | `/model <provider:model>` | tunneled via `run_slash_command` | tunneled via `slash_command` | `--model` | admin | no | tunneled | Same syntax as model.set, different canonical intent and policy |
| `thinking.set` | `/think <level>` | tunneled via `run_slash_command` | tunneled via `slash_command` | profile/startup settings | edit | yes | tunneled | Canonical action exists but no first-class transport binding |
| `skills.view` | `/skills`, `/skills list` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon skills list` | read | yes | tunneled | Read intent is classified and remote-safe but still tunneled |
| `skills.install` | `/skills install` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon skills install` | edit | no | tunneled | Classified local-only on remote surfaces |
| `auth.status` | `/auth`, `/auth status` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon auth status` | read | yes | tunneled | Read-only, but remote still depends on generic slash tunnel |
| `auth.login` | `/login <provider>` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon auth login <provider>` | admin | no | tunneled | Local-only by policy |
| `auth.logout` | `/logout <provider>` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon auth logout <provider>` | admin | no | tunneled | Local-only by policy; explicit provider now required |
| `auth.unlock` | `/auth unlock` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon auth unlock` | admin | no | tunneled | Sensitive backend action |
| `secrets.view` | `/secrets`, `/secrets list` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | no | tunneled | Explicitly not remote-safe today |
| `secrets.set` | `/secrets set ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | no | tunneled | Local-only by policy |
| `secrets.get` | `/secrets get ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | no | tunneled | Local-only by policy |
| `secrets.delete` | `/secrets delete ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | — | edit | no | tunneled | Local-only by policy |
| `plugin.view` | `/plugin`, `/plugin list` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon plugin list` | read | yes | tunneled | Read path exists but not first-class remotely |
| `plugin.install` | `/plugin install ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon plugin install ...` | edit | no | tunneled | Local-only by policy |
| `plugin.remove` | `/plugin remove ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon plugin remove ...` | edit | no | tunneled | Local-only by policy |
| `plugin.update` | `/plugin update ...` | tunneled via `run_slash_command` | tunneled via `slash_command` | `omegon plugin update ...` | edit | no | tunneled | Local-only by policy |

### Immediate implications

1. **`control_actions.rs` is already the embryo of the canonical registry**
   - it defines canonical actions
   - it defines starter roles
   - it defines remote-safety
   - it classifies both slash commands and IPC methods

2. **The main gap is no longer 'invent a matrix'**
   - the main gap is to make transports bind directly to canonical actions
     instead of tunneling broad classes of intent through generic slash
     execution

3. **Best current first-class C2 actions**
   - `turn.cancel`
   - `prompt.submit`
   - `status.view` (partially; still vocabulary-divergent)
   - `runtime.shutdown` (IPC only)

4. **Highest-value unification targets**
   - `context.*`
   - `model.*`
   - `auth.status`
   - `session.new`

---

## Current canonical matrix (v0 draft)

The following tables capture the **currently implemented** surfaces and the
proposed canonical actions they map to.

### Context

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `context.view` | `/context`, `/context status` | — | — | — | read | Bare `/context` now shows the rich status surface |
| `context.compact` | `/context compact`, `/context compress` | — | — | — | edit | Mutates session by compacting older turns |
| `context.clear` | `/context clear` | — | — | — | edit | Resets live conversation context |
| `context.request` | `/context request …` | — | — | — | edit | Pulls a mediated context pack for current work |
| `context.set_class` | `/context <class>` | `--context-class` at startup | — | — | edit | Command-surface intent is workflow tuning |

### Skills

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `skills.view` | `/skills`, `/skills list` | `omegon skills list` | — | — | read | Bare `/skills` is now a status surface |
| `skills.install` | `/skills install` | `omegon skills install` | — | — | edit | Installs bundled skills into `~/.omegon/skills` |

### Model / thinking / provider

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `model.view` | `/model` | startup logs only | — | — | read | Bare `/model` now shows model/provider posture |
| `model.list` | `/model list` | — | — | — | read | Lists catalogued models |
| `model.set.same_provider` | `/model <provider:model>` when provider does not change | `--model` | — | — | edit | Workflow tuning; does not change auth/control boundary |
| `provider.switch` | `/model <provider:model>` when provider changes | `--model` | — | — | admin | Same slash syntax, different canonical intent |
| `thinking.set` | `/think <level>` | startup/profile settings | — | — | edit | Workflow tuning |
| `thinking.view` | implied in `/model`, `/context`, `/stats` | — | — | — | read | Not yet a dedicated top-level action |

### Session lifecycle

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `session.view.list` | `/sessions` | — | — | — | read | Local list of resumable sessions |
| `session.new` | `/new` | — | — | `new-session` | edit | Reuses `TuiCommand::NewSession` |
| `session.reset` | same underlying local effect as `session.new` | — | — | same as above | edit | Keep one canonical action unless semantics diverge later |

### Runtime lifecycle

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `turn.cancel` | local cancel flows | — | `cancel` | `cancel` | edit | Shared cancellation token path |
| `runtime.shutdown` | local quit path | process signal / local exit | `shutdown` | `shutdown` | admin | Reuses `TuiCommand::Quit` |

### Prompt/work submission

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `prompt.submit` | normal operator input | `--prompt`, `--prompt-file` | `submit_prompt` | `prompt` | edit | One-shot CLI/headless path is still local operator-driven |
| `slash.execute` | many `/…` commands | — | `run_slash_command` | `slash-command` | depends | Needs subcommand-level classification |

### Auth

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `auth.status` | `/auth`, `/auth status` | `omegon auth status` | via slash path today | via slash path today | read | Safe observation |
| `auth.login` | `/login`, `/auth login …` | `omegon auth login …` | via slash path today | via slash path today | admin | Changes provider auth state |
| `auth.logout` | `/logout`, `/auth logout …` | `omegon auth logout …` | via slash path today | via slash path today | admin | Changes provider auth state |
| `auth.unlock` | `/auth unlock` | `omegon auth unlock` | via slash path today | via slash path today | admin | Secret/auth backend sensitive |

### Secrets

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `secrets.view` | `/secrets`, `/secrets list` | — | via slash path today | via slash path today | edit | Operational editing surface, not pure read |
| `secrets.set` | `/secrets set …` | — | via slash path today | via slash path today | edit | Explicitly requested to be edit-capable |
| `secrets.get` | `/secrets get …` | — | via slash path today | via slash path today | edit | Operational secret use |
| `secrets.delete` | `/secrets delete …` | — | via slash path today | via slash path today | edit | Operational secret mutation |

### Skills / plugins / memory / status (additional common surfaces)

| Canonical action | Current slash binding | CLI | IPC | Web/daemon | Starter role | Notes |
|---|---|---:|---:|---:|---|---|
| `status.view` | `/status`, `/stats`, `/auspex status`, `/dash status` | — | `get_state`, `get_graph`, event subscribe | web `/api/state`, `/api/graph` | read | Several current read-only surfaces should eventually normalize here |
| `memory.view` | `/memory` | — | — | — | read | Local summary today |
| `plugin.view` | `/plugin`, `/plugin list` | `omegon plugin list` | — | — | read | Common administration surface |
| `plugin.install` | `/plugin install …` | `omegon plugin install …` | — | — | edit/admin (TBD) | Needs policy decision |
| `plugin.remove` | `/plugin remove …` | `omegon plugin remove …` | — | — | edit/admin (TBD) | Needs policy decision |
| `plugin.update` | `/plugin update …` | `omegon plugin update …` | — | — | edit/admin (TBD) | Needs policy decision |

---

## High-priority ambiguities to resolve

### 1. `run_slash_command` is too broad

IPC and web currently expose generic slash execution paths.
That is useful for parity, but it is not RBAC-ready.

We need a classifier that resolves:

- raw slash command + args
- → canonical action id
- → required role
- → remote-safe or local-only

Without that classifier, any transport-level RBAC for slash execution will be
coarse and error-prone.

### 2. `/model` mixes two intents

`/model <provider:model>` currently handles both:

- same-provider model set → `edit`
- provider switch → `admin`

The canonical matrix distinguishes those intents already. Enforcement will need
intent parsing, not just command-name matching.

### 3. Some top-level slash commands still mix view and action semantics

Examples:
- `/auth`
- `/plugin`
- `/memory`

The canonical matrix should continue moving bare commands toward useful status
views with explicit action subcommands where possible.

---

## Immediate next implementation targets

This document is a **definitions-first artifact**. Before full RBAC enforcement,
we should add code support for:

1. A canonical action classifier
   - input: ingress + command/method/trigger + args
   - output: canonical action id + role + transport policy

2. A small machine-readable registry table in code
   - enough to drive help/docs and future enforcement together

3. Transport-boundary checks
   - IPC dispatch
   - web daemon event ingress
   - web/IPC slash execution wrapper paths

---

## Current command-surface normalization progress

Already normalized toward the matrix:

- `/context` → rich status surface by default; subcommands preserved
- `/skills` → rich status surface by default; install preserved
- `/model` → rich status surface by default; list and direct set preserved

These are the first examples of:

- top-level command = readable status surface
- deeper subcommands / arguments = explicit actions

That pattern should drive the rest of the common control plane.
