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

## Decisions

### Start with palette-style text projections, not a new modal

**Status:** proposed

**Rationale:** The current pain is visible in default command output, and the same projection can feed TUI transcript, CLI remote slash execution, and ACP responses. A modal can consume the projection later without blocking immediate UX improvement.

### Use `/skills` and `/prompt` as first palette targets

**Status:** proposed

**Rationale:** Both are operator-facing CRUD/manage surfaces. `/skills` exposes bundled/user/project-local instruction artifacts; `/prompt` manages reusable prompt records. They exercise the common palette needs: action rows, object rows, scope/status indicators, detail-on-demand, and safe handling of destructive actions.

## Open Questions

- [assumption] `/skills` and `/prompt` default outputs should be improved first as text projections before building a full interactive modal palette.
- [assumption] The command registry has enough metadata to drive palette rows for command name, argument shape, availability, safety, and short description without duplicating a separate slash-menu allowlist.
- How should palette rows represent object scope for skills and prompts: bundled/user/project-local, installed/not installed, editable/read-only, and safe/destructive actions?

## Implementation Notes

### File Scope

- `core/crates/omegon/src/control_runtime.rs` — 
- `core/crates/omegon/src/tui/mod.rs` — 
- `core/crates/omegon/src/skills.rs` — 
- `core/crates/omegon/src/prompt.rs` — 
- `core/crates/omegon-traits/src/lib.rs` —
