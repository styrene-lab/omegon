+++
id = "1f4a1e4f-7c3a-4d8b-a3b5-c4c13f9e1f8a"
tags = ["commands", "tui", "cli", "control-plane"]
aliases = ["command-map", "slash-canonical-map"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Canonical command map

## Overview

Omegon currently exposes operator actions through several overlapping surfaces:

- TUI slash commands (`/auth login`, `/context clear`, `/new`)
- CLI subcommands (`omegon auth login`, `omegon extension list`)
- control-plane requests (`ControlRequest`)
- web/IPC slash forwarding
- extension-contributed commands planned for 0.25+

The command surface is now partially canonicalized through `CanonicalSlashCommand`, but canonical ownership is still implicit. This document defines the domain map we should converge toward before rewriting dispatch.

## Core rule

Canonical commands are **domain-scoped**. Top-level verbs are allowed only as fast aliases.

If a top-level command and a domain command perform the same state transition, the domain command is canonical and the top-level form aliases to it.

Examples:

- `/context reset` is canonical; `/new` is shorthand.
- `/auth login <provider>` is canonical; `/login <provider>` is shorthand.
- `/stats bench` is canonical; `/bench` is compatibility shorthand if retained.

## Command lifecycle states

Every command spelling should have an explicit lifecycle:

| State | Meaning | Help behavior |
|---|---|---|
| `canonical` | Primary documented spelling owned by a domain | Shown in default help |
| `supported_alias` | Ergonomic shorthand or long-lived compatibility alias | May be shown with canonical parent |
| `deprecated_alias` | Still routed, but should disappear from docs/default help | Only shown in `/help all` or migration docs |
| `hidden_compat` | Machine/back-compat route; not an operator-facing command | Hidden except debug/registry inspection |
| `removed` | No longer routed | Not shown |

## Canonical domains

### Context and session lifecycle

Canonical parent: `/context`

| Canonical | Purpose | Aliases | Notes |
|---|---|---|---|
| `/context status` | Inspect context usage, class, model, thinking level | `/context` when no args opens selector in TUI | Status should mention reset/compact/request actions |
| `/context compact` | Summarize older turns to reduce context pressure | `/context compress` | Pure context-budget operation |
| `/context request <kind> <query>` | Request mediated context pack | none | Agent/operator shared concept |
| `/context class <compact|standard|extended|massive>` | Set context class | current direct `/context compact` etc. | Current direct forms can remain supported aliases |
| `/context reset` | Save current session and start fresh context/session | `/context clear`, `/new` | Preferred long-term name. If not introduced immediately, `/context clear` temporarily acts as canonical. |

Decision: `/new` is not canonical. It is a high-frequency alias for `/context reset` (or `/context clear` during transition).

Rationale: the operation resets conversation state, session id, resume info, context metrics, and emits context/session reset events. That is a context/session lifecycle operation.

### Authentication

Canonical parent: `/auth`

| Canonical | Purpose | Aliases | CLI equivalent |
|---|---|---|---|
| `/auth status` | Show provider credential/session status | `/auth` | `omegon auth status` |
| `/auth login <provider>` | Start OAuth/API-key login flow | `/login <provider>` | `omegon auth login <provider>` |
| `/auth logout <provider>` | Remove stored credentials | `/logout <provider>` | `omegon auth logout <provider>` |
| `/auth unlock` | Unlock encrypted secrets/auth backend | none | `omegon auth unlock` |

Decision: recovery hints should only mention canonical `/auth login <provider>` unless space-constrained.

### UI and presentation

Canonical parent: `/ui`

| Canonical | Purpose | Aliases | Notes |
|---|---|---|---|
| `/ui status` | Show current UI preset/surface state | none | Default status command for UI |
| `/ui lean` | Apply lean/slim UI preset | `/slim` if still routed | Transitional aliases should be hidden |
| `/ui full` | Apply full UI preset | none | Canonical expanded preset |
| `/ui show <surface>` | Show surface | none | Surface names should be registry-driven later |
| `/ui hide <surface>` | Hide surface | none | — |
| `/ui toggle <surface>` | Toggle surface | none | — |
| `/ui detail <level>` | Set tool/output detail density | `/ui density <level>`, `/detail <level>` | `density` is a supported alias; top-level `/detail` should become deprecated/hidden |

Decision: UI preset names should be nouns/adjectives under `/ui`; standalone layout commands are aliases only.

### Metrics and performance

Canonical parent: `/stats`

| Canonical | Purpose | Aliases | Notes |
|---|---|---|---|
| `/stats` | Show session telemetry and usage | `/usage` if still routed | Keep separate from `/status`, which is runtime health |
| `/stats bench` | Run local benchmark/perf view | `/bench` | `/bench` should be deprecated or hidden once `/stats bench` is established |

### Runtime status

Canonical parent: `/status`

| Canonical | Purpose | Aliases | Notes |
|---|---|---|---|
| `/status` | Show runtime/harness health | none | Distinct from `/stats` metrics |

### Planning

Canonical parent: `/plan`

| Canonical | Purpose | Aliases |
|---|---|---|
| `/plan status` or `/plan` | Show active plan | none |
| `/plan list` | List active/recent plans | none |
| `/plan set <items>` | Set plan | none |
| `/plan approve` | Approve plan | none |
| `/plan execute` | Execute approved plan | none |
| `/plan advance` | Mark/advance current step | none |
| `/plan skip` | Skip current step | none |
| `/plan clear` | Clear active plan | none |

### Workspace

Canonical parent: `/workspace`

| Canonical | Purpose | Aliases |
|---|---|---|
| `/workspace status` | Show workspace lease/admission/bindings | none |
| `/workspace list` | List workspaces | none |
| `/workspace new <label>` | Create workspace | none |
| `/workspace adopt` | Adopt current workspace | none |
| `/workspace release` | Release lease | none |
| `/workspace archive` | Archive workspace | none |
| `/workspace prune` | Prune stale workspaces | none |
| `/workspace role <role>` | Set/clear role | none |
| `/workspace kind <kind>` | Set/clear kind | none |
| `/workspace bind milestone|node|clear` | Bind lifecycle target | none |

### Permissions and trust

Canonical parent: `/permissions`

| Canonical | Purpose | Aliases | Notes |
|---|---|---|---|
| `/permissions` | View permission/trust state | `/permission` | Singular is supported alias |
| `/permissions add <path>` | Trust/add path | `/trust add <path>` | `/trust` is a domain alias; may remain supported because it is semantically clear |
| `/permissions remove <path>` | Revoke/remove path | `/trust remove <path>` | — |

### Secrets and vault

Canonical parents: `/secrets`, `/vault`

| Canonical | Purpose | Aliases |
|---|---|---|
| `/secrets` | View configured secrets | none |
| `/secrets set <name> <value>` | Store secret | none |
| `/secrets get <name>` | Read/verify secret | none |
| `/secrets delete <name>` | Delete secret | none |
| `/vault status` | Vault backend status | `/vault` |
| `/vault configure` | Configure vault | none |
| `/vault init-policy` | Initialize policy | none |

Decision: `/vault unseal` and `/vault login` are CLI/control concepts if present, but should not expand TUI docs unless actively supported in interactive flow.

### Installation and ecosystem

These domains are distinct but operator-confusing. Preserve canonical parent boundaries and add glossary/help grouping.

| Domain | Canonical parent | Meaning | Aliases |
|---|---|---|---|
| Extensions | `/extension` | Installed runtime integrations | `/ext` supported alias |
| Plugins | `/plugin` | MCP/plugin manifests | none |
| Armory | `/armory` | Remote installable registry/source | none |
| Catalog | `/catalog` | Agent bundles/catalog entries | none |
| Skills | `/skills` | Prompt/runtime skills | `/skill` supported alias |
| Persona | `/persona` | Runtime behavior preset | none |

Decision: do not collapse these into one `/install` domain yet. They represent different installation targets and lifecycle semantics. Instead, improve glossary and help grouping.

### Lifecycle/work execution

| Canonical | Purpose | Aliases |
|---|---|---|
| `/tree` | Design tree operations/status | none |
| `/cleave` | Decomposition status/cancel | none |
| `/delegate` | Delegate/scout status | none |
| `/checkin` | Session/lifecycle check-in | none |

### Notes and scratchpad

Canonical parent: `/notes`

| Canonical | Purpose | Aliases | Notes |
|---|---|---|---|
| `/notes` | Show notes/scratchpad | none | Transitional until scratchpad extraction is settled |
| `/notes clear` | Clear notes | none | — |
| `/notes add <text>` | Add note | `/note <text>` | `/note` is a supported alias for now; likely extension-owned later |

Decision: note/scratchpad is a candidate for extraction into an extension-contributed command once UI contribution substrate is ready.

## First consolidation target

### `/new` and `/context clear`

Current state: both save current session and allocate a fresh conversation/session; `/context clear` additionally resets context metrics and emits `ContextUpdated`.

Target state:

- canonical operation: `/context reset`
- supported aliases: `/context clear`, `/new`
- one implementation helper performs the transition
- all routes emit the same state events:
  - save previous session unless `--no-session`
  - clear conversation
  - allocate session id
  - clear resume info
  - reset context metrics
  - emit `ContextUpdated`
  - emit `SessionReset`

If `/context reset` is too much naming churn for the immediate patch, use `/context clear` as temporary canonical and still make `/new` an alias.

## Implementation sequence

1. Define command metadata module with canonical/alias lifecycle states.
2. Canonicalize `/new` into the context reset/clear path.
3. Make `/help` derive from command metadata.
4. Add drift tests for visible help commands and aliases.
5. Gradually migrate simple dispatch paths from ad hoc `match` arms to registry-backed handlers.

## Non-goals for first implementation

- Do not generate Clap CLI from the registry.
- Do not remove compatibility aliases immediately.
- Do not collapse extension/plugin/armory/catalog/skills into one domain.
- Do not replace all `handle_slash_command` dispatch at once.

## Open questions

1. Should the long-term canonical reset spelling be `/context reset` rather than `/context clear`?
2. Should `/new` remain visible in default help as a high-frequency alias, or only in `/help all`?
3. Should aliases like `/login` and `/logout` stay visible because they are common, or should recovery/help output exclusively teach `/auth login|logout`?
4. Should `/trust` remain a supported alias under permissions, or become deprecated once `/permissions add/remove` is established?
