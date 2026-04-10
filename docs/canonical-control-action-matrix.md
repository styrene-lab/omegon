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
