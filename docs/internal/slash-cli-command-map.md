# Internal Slash Command and CLI Map

This document maps Omegon's operator command surfaces. It is intentionally internal: the goal is to expose drift, duplicates, and dead aliases so they can be cut rather than carried forever.

## Sources of truth

| Surface | Primary implementation | Notes |
|---|---|---|
| Interactive slash commands | `core/crates/omegon/src/tui/mod.rs::handle_slash_command` | Executes TUI-local actions and routes canonical commands to control/runtime handlers. |
| Slash autocomplete | `core/crates/omegon/src/tui/mod.rs::App::COMMANDS` plus bus commands | Must advertise only supported canonical commands. Aliases should be rare and intentional. |
| Canonical slash parser | `core/crates/omegon/src/tui/mod.rs::canonical_slash_command` | Shared parser for commands that can route through `control_runtime`. |
| Control runtime mapping | `core/crates/omegon/src/control_runtime.rs::control_request_from_slash` | Converts canonical slash commands into executable control requests. |
| CLI commands | `core/crates/omegon/src/main.rs::Commands` and nested `*Action` enums | Clap surface for non-interactive and system-management workflows. |
| Operator docs | `core/docs/cli-reference.md` | Public-facing subset; not exhaustive. |

## Policy

- Canonical commands are documented and autocomplete-visible.
- Aliases are not kept for nostalgia. Keep only aliases that materially improve ergonomics or match a still-active external concept.
- Removed commands must fail explicitly. Do not silently show status for unknown subcommands.
- TUI-only layout/state commands do not need CLI equivalents.
- CLI-only automation/system commands do not need slash equivalents unless the operation is useful during an interactive session.
- If a slash command and CLI command manage the same concept, prefer shared parsing/control-runtime code over duplicate implementations.

## Interactive slash command tree

This is the built-in tree exposed by `App::COMMANDS` after UI cleanup.

```text
/help

/copy
  raw
  plain
  latest
  session

/transcript
  file
  open
  scrollback

/mouse
  on
  off

/model
  list

/think
  off
  minimal
  low
  medium
  high

/profile
  view
  capture
  apply
  mqtt
  extension
  persona
  tone

/stats
/bench

/new

/detail
  lean
  compact
  detailed
  verbose

  lean
  compact
  detailed
  verbose

/context
  status
  compact
  clear
  squad
  maniple
  clan
  legion

/plan
  status
  list
  set
  approve
  execute
  advance
  skip
  clear

/sessions
/memory

/skills
  list
  install
  create
  get
  delete

/extension
  list
  get
  install
  remove
  update
  enable
  disable
  search

  list
  get
  install
  remove
  update
  enable
  disable
  search

/plugin
  list
  install
  remove
  update

/armory
  browse
  search
  list
  install

/catalog
  list
  install
  remove

/cleave
  status

/auth
  status
  unlock
  login
  logout
  anthropic
  openai
  openai-codex
  openrouter
  ollama-cloud
  github

/chronos
  week
  month
  quarter
  relative
  iso
  epoch
  tz
  range
  all

/init
  scan
  migrate

/update
  install
  channel

/ui
  status
  lean
  full
  show
  hide
  toggle
  detail
  density


/migrate
  auto
  claude-code
  pi
  codex
  cursor
  aider

  status

/auspex
  status
  open

/secrets
  list
  set
  get
  delete

/vault
  status
  configure
  init-policy

/persona
  list
  create
  off

/tone
  off

/delegate
  status

/status
/focus

/tree
  list
  frontier
  ready
  blocked

/milestone
  freeze
  status

/notes  # transitional; planned scratchpad-extension extraction
  add
  clear
  checkin

/editor
  zed
  vscode
  status

/preferences

/permissions
  list
  add
  remove
  keys

/automation
  status
  ask
  guarded
  flow
  autonomous

  ask
  guarded
  flow
  autonomous

  add
  remove
  list

/sandbox
  on
  off
  status

/version
/exit
```

## Help-guided tutorial

The tutorial is no longer a top-level slash command. Use:

```text
/help tutorial
/help tutorial status
/help tutorial reset
/help tutorial consent
/help tutorial demo
/help next
/help prev
```

Rationale: tutorial navigation belongs under help/discovery, not the primary command palette.

## Current `/ui` canonical subtree

```text
/ui
/ui status
/ui lean
/ui full
/ui show dashboard|instruments|footer
/ui hide dashboard|instruments|footer
/ui toggle dashboard|instruments|footer
/ui detail lean|compact|detailed|verbose
/ui density lean|compact|detailed|verbose
```

Accepted ergonomic aliases:

```text
dash -> dashboard
instrument -> instruments
tools -> instruments
```

Removed aliases / dead commands:

```text
/ui standard
/ui std
/ui slim
/ui minimal
/ui show|hide|toggle tree
/ui show|hide|toggle status
```

Rationale: `standard` was only `lean + footer` and did not represent a distinct layout. `slim/minimal` duplicate `lean`. `tree` toggled the whole dashboard, not a tree-specific surface. `status` conflicted with `/ui status` and the Slim status line.


## Extension extraction: scratchpad

Concrete sister-repo evidence: `/Users/wilson/workspace/styrene-labs/omegon-extensions/issue-omegon-ui-contributions.md` proposes declarative extension UI contributions, with `scratchpad` as the dogfood target. The proposed contribution model lets extensions declare slash commands, command-palette entries, passive status items, panels, completion providers, notifications, and keybindings while Omegon/Cockpit owns validation, routing, policy, and terminal rendering.

Current core surface:

```text
/notes
/notes add <text>
/notes clear
/notes checkin
hidden /note
hidden /checkin
```

Status: transitional only. `/notes` is not the long-term canonical endpoint. It exists until the default scratchpad/tutorial extension can contribute commands through the extension UI contribution protocol.

Target extension surface:

```text
/scratchpad:add
/scratchpad:list
/scratchpad:search
/scratchpad:clear
/scratchpad:checkin
```

Optional passive status contribution:

```text
scratch:{count}
```

Extraction requirements:

- extension manifest declares a `ui` namespace such as `scratchpad`;
- runtime protocol supports `ui/list_contributions`;
- host validates runtime contributions against the install-time manifest envelope;
- command palette and slash routing accept namespaced extension commands;
- host-rendered status item can call a refresh tool such as `scratchpad_stats`;
- raw terminal drawing from extensions remains denied;
- migration/import path handles existing `.omegon/notes.md` once.

Deletion debt after extraction:

```text
/notes
/note
/checkin
```

## CLI command tree

Top-level Clap commands from `main.rs::Commands`:

```text
omegon interactive
omegon serve
omegon embedded        # hidden
omegon auth <action>
omegon migrate [source]
omegon eval
omegon plugin <action>
omegon extension <action>
omegon armory <action>
omegon secret <action>
omegon cleave
omegon switch [version]
omegon run [task-spec]
omegon ollama <action>
omegon acp
omegon embedding <action>
omegon sentry
omegon doctor          # hidden
omegon skills <action>
omegon catalog <action>
omegon persona <action>
omegon bench <action>  # hidden
omegon task <action>
omegon nex <action>
```

Known nested CLI action families:

```text
auth:      status | login | logout | unlock
plugin:    list | install | remove | update
extension: list | get | install | remove | update | enable | disable | search
armory:    browse | search | install
secret:    list | set | get | delete
ollama:    register | unregister | status
skills:    list | install | get | create | delete
catalog:   list | install | remove
persona:   list | create | delete
nex:       init | list | inspect | compose | networkpolicy | status
```

Some nested families are intentionally summarized here because their flags are operational detail rather than command-tree shape: `serve`, `eval`, `cleave`, `switch`, `run`, `acp`, `embedding`, `sentry`, `bench`, and `task`.

## Slash to CLI equivalence

| Concept | Slash | CLI | Notes |
|---|---|---|---|
| Provider auth | `/auth` | `omegon auth ...` | `/auth login <provider>` and `/auth logout <provider>` are canonical; `/login` and `/logout` are hidden deletion debt. |
| Extension management | `/extension` | `omegon extension ...` | Canonical slash command and CLI command align. |
| Plugin management | `/plugin` | `omegon plugin ...` | Same operator concept. |
| Armory inventory | `/armory` | `omegon armory ...` | Same operator concept. |
| Agent catalog | `/catalog` | `omegon catalog ...` | Same operator concept. |
| Secrets | `/secrets` | `omegon secret ...` | Slash is plural, CLI is singular. Consider normalizing eventually. |
| Skills | `/skills` | `omegon skills ...` | Same operator concept. |
| Personas | `/persona` | `omegon persona ...` | Same operator concept. |
| Migration | `/migrate` | `omegon migrate ...` | Same operator concept. |
| Sandbox/Nex | `/sandbox` | `omegon nex ...` | Slash toggles runtime isolation; CLI manages profiles. Related but not equivalent. |
| Cleave | `/cleave` | `omegon cleave ...` | Slash is interactive/status/decompose; CLI is orchestration runner. |
| Headless run | none | `omegon run ...` | CLI-only automation surface. |
| Daemon/control plane | none | `omegon serve`, `omegon acp`, `omegon sentry` | CLI-only service surfaces. |
| UI layout | `/ui`, `/focus`, `/mouse` | none | TUI-only. |
| Session-local plan | `/plan` | none | TUI/session lifecycle surface. |
| Context window | `/context` | run flags partially overlap | Interactive context management has no direct CLI equivalent. |

## Drift and cleanup backlog

- Verify every command in `App::COMMANDS` has an active `handle_slash_command` path or intentional bus-command route.
- Verify every `handle_slash_command` arm that is operator-facing is either autocomplete-visible or explicitly hidden.
- Alias commands removed from autocomplete: `/ext`, `/perf`, `/density`, `/prefs`, `/autonomy`, and `/trust`. `/bench` and `/detail` are also no longer primary palette entries; use `/stats bench` and `/ui detail ...`. Delete hidden handlers too if usage evidence stays absent.
- `/dash` removed from autocomplete; delete the handler after Auspex diagnostics no longer require the compatibility browser path.
- Normalize `secret` vs `secrets` naming across CLI/slash or document the asymmetry as intentional.
- Keep `/ui` strict: no `standard`, `std`, `slim`, `minimal`, `tree`, or `status` aliases.
- Keep autocomplete canonical-only; hidden handlers are temporary debt, not compatibility promises.
