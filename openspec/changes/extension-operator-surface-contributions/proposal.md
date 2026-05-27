# Proposal: Extension Operator Surface Contributions

## Intent

Add a declarative extension contribution surface for operator-facing UX:
commands, passive status items, and host-managed surfaces. Reader is the primary
MVP dogfood target and Scratchpad is the primitive host-rendered smoke target.

## Why

Extensions currently expose tools and HostActions, but not discoverable operator
UX. Reader needs host-owned command/status/surface affordances without directly
drawing into the terminal or mutating host UI registries.

## Scope

First implementation track:

- Add SDK/protocol types for UI contribution declarations.
- Add manifest envelope parsing for Reader-style command/status/surface entries.
- Add runtime `ui/list_contributions` response shape.
- Add host validation/registry design for accepted contributions.
- Keep rendering and command routing as later slices unless explicitly pulled in.

## Non-goals

- No raw extension terminal drawing.
- No arbitrary ANSI/HTML/JS rendering.
- No direct keybinding install.
- No guaranteed side pane.
- No automatic HostAction execution outside existing policy/approval paths.
