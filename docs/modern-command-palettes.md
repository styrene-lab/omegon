---
id: modern-command-palettes
title: "Modern Command Palettes"
status: exploring
tags: [tui, commands, slash-menu, skills, prompts, operator-surface]
open_questions:
  - "[assumption] `/skills` and `/prompt` default outputs should be improved first as text projections before building a full interactive modal palette."
  - "[assumption] The command registry has enough metadata to drive palette rows for command name, argument shape, availability, safety, and short description without duplicating a separate slash-menu allowlist."
  - "How should palette rows represent object scope for skills and prompts: bundled/user/project-local, installed/not installed, editable/read-only, and safe/destructive actions?"
dependencies: []
related: []
---

# Modern Command Palettes

## Overview

Redesign operator-facing slash/menu surfaces from report-style dumps into compact, action-oriented command palettes. The first targets are `/skills` and `/prompt`, because both expose CRUD/manageable content where the current default output should prioritize next actions, searchable/narrowable rows, concise metadata, and detail-on-demand rather than long inventory prose.

## Research

### External CLI/TUI affordance scan

Claude Code and Codex present slash command surfaces as command-first rows with argument shape and one-line effect, e.g. `/model`, `/status`, `/prompt-like command [args]`, rather than inventory dumps. Hermes documentation emphasizes slash-command autocomplete, modal overlays, and `/skills` as an interactive TUI command. Shared pattern: default slash discovery is action-oriented; details/full bodies are behind explicit detail commands or secondary surfaces.

### Current `/skills` surface

Current `/skills` TUI route passes through `canonical_slash_command("skills", args)` in `core/crates/omegon/src/tui/mod.rs`, then `control_runtime::skills_view_response()` renders a wide inventory dump. The renderer groups bundled/user/project-local skills but leads with long descriptions and places actionable commands only at the bottom. This is the first concrete target for a palette-style projection.

### Current `/prompt` and user command surface

`/prompt` is now registered through `features::prompt::PromptFeature` as a command-palette-native router with `CommandAvailability::ALL` and prompt-injection-sensitive safety metadata. Prompt IDs remain data: `/prompt <name>` is shorthand for preview, while direct slash invocation such as `/review` is provided by explicit user command aliases loaded from `.omegon/commands/*.toml` and `~/.omegon/commands/*.toml` via `features::user_commands::UserCommandFeature`. Prompt definition CRUD/read surfaces are backed by `prompts.rs`, and ACP/IPC/WebSocket expose read/preview projections with safety verdicts rather than silent queue execution.

## Decisions

### Start with palette-style text projections, not a new modal

**Status:** proposed

**Rationale:** The current pain is visible in default command output, and the same projection can feed TUI transcript, CLI remote slash execution, and ACP responses. A modal can consume the projection later without blocking immediate UX improvement.

### Use `/skills` and `/prompt` as first palette targets

**Status:** proposed

**Rationale:** Both are operator-facing CRUD/manage surfaces. `/skills` exposes bundled/user/project-local instruction artifacts; `/prompt` manages reusable prompt records. They exercise the common palette needs: action rows, object rows, scope/status indicators, detail-on-demand, and safe handling of destructive actions.

### Prompt IDs are data; user commands are invocation surfaces

**Status:** accepted

**Rationale:** Prompt libraries should not pollute the global slash namespace or collide with built-ins such as `/model`, `/help`, or `/plan`. `/prompt <name>` is the canonical quick preview path. If an operator wants direct `/review` style invocation, they create an explicit user command alias targeting `prompt:<id>` with availability and safety metadata.

## Open Questions

- [assumption] `/skills` and `/prompt` default outputs should be improved first as text projections before building a full interactive modal palette.
- [assumption] The command registry has enough metadata to drive palette rows for command name, argument shape, availability, safety, and short description without duplicating a separate slash-menu allowlist.
- How should palette rows represent object scope for skills and prompts: bundled/user/project-local, installed/not installed, editable/read-only, and safe/destructive actions?
- How should `/skills` default output be refactored into the same compact action-row projection without losing detail-on-demand access to full skill bodies?

## Implementation Notes

### File Scope

- `core/crates/omegon/src/control_runtime.rs` — existing `/skills` control response remains the main target for palette-style skill projection.
- `core/crates/omegon/src/tui/mod.rs` — static help/completion should expose prompt/user-command surfaces through registry-backed command definitions rather than bespoke allowlists.
- `core/crates/omegon/src/skills.rs` — skill inventory already provides bundled/user/project-local data needed for palette rows.
- `core/crates/omegon/src/prompts.rs` — implemented reusable prompt definitions, storage lookup, safety verdicts, and bundled/user/project-local prompt inventory.
- `core/crates/omegon/src/features/prompt.rs` — implemented `/prompt` as the registry-native prompt router with `<name>` shorthand preview.
- `core/crates/omegon/src/features/user_commands.rs` — implemented explicit prompt-backed user command aliases for direct slash invocation.
- `core/crates/omegon/src/backend.rs` — registered skill/prompt ACP/RPC surface contracts.
- `core/crates/omegon/src/acp.rs` — wired ACP skill and prompt read/preview handling.
- `core/crates/omegon/src/ipc/connection.rs` — exposed IPC prompt read/preview methods and skill get.
- `core/crates/omegon/src/web/ws.rs` — exposed WebSocket prompt read/preview methods.
- `core/crates/omegon/src/control_actions.rs` — classified skill and prompt actions for legacy IPC/Web safety gates.
- `core/crates/omegon-traits/src/lib.rs` — command availability and safety metadata already exists and is the registry contract.

## Consolidation Tracks

1. **Skill and prompt palettes** — convert `/skills` and related manageable-content commands from inventory dumps into compact action/object rows with detail-on-demand commands.
2. **Shared palette DTO** — extract renderer-neutral palette row/group projections so TUI transcript output, command menus, CLI remote slash execution, and ACP/web clients consume the same semantic surface.
3. **Settings surface completion** — make `SettingsSurfaceProjection` the settings-page source of truth and leave TUI-local code responsible only for navigation, filtering, selection, and input dispatch.
4. **Registry-backed menu discovery** — drive slash help, menus, command palette rows, CLI remote execution metadata, and ACP command discovery from command-registry availability/safety metadata instead of per-surface allowlists.

## Remaining Work

- Integrate `/context` and `/think` into the modern palette track without breaking their existing bare-command selector behavior. Their static TUI metadata now exposes action-oriented subcommands, but they still need shared state/action projections for CLI/ACP/text surfaces.
- Refactor `/skills` default output into a compact palette-style action/object projection.
- Extract a shared command-palette row DTO so `/skills`, `/prompt`, TUI palette, ACP, and CLI text output can consume one projection.
- Complete settings-page consolidation by deriving TUI settings rows/selectors from `SettingsSurfaceProjection` instead of parallel descriptors.
- Add a stronger confirmation/trust flow before any prompt/user-command surface queues or executes prompt bodies directly.
