+++
title = "Universal settings and config entrypoint"
tags = ["tui","settings","ux","commands"]
+++

# Universal settings and config entrypoint

# Universal settings and config entrypoint

## Overview

`/settings` is the universal discovery and routing surface for operator-configurable capabilities. `/config` is an equivalent registry-backed alias. Domain menus remain canonical owners of their behavior; the settings hub links to them rather than duplicating their internals.

## Decisions

1. Bare `/settings` and `/config` open the same expanded settings hub.
2. The hub leads with a **Configuration** tab whose rows route to canonical domain menus, followed by the existing runtime settings tabs unchanged.
3. Direct routes are supported from either entrance: `runtime`, `model`, `auth`, `skills`, `extensions`, `ui`, `context`, `memory`, `profile`, `secrets`, `sandbox`, and `updates`.
4. `/settings` owns discovery and routing. `/auth`, `/skills`, and the other domain commands continue to own their menus and actions.
5. Skills belong in settings because installation, enablement, refresh, and persistence are configuration concerns. Mixed operational/configuration menus may be linked whole rather than partially reimplemented.
6. Initial implementation uses a typed, colocated route table and regression tests that require every route target to remain registry-backed. A broader registry metadata generalization is deferred until another frontend needs the same taxonomy.

## Constraints

- Preserve direct access to every canonical domain command.
- Preserve existing runtime settings rows and profile save/apply actions.
- Do not duplicate domain menu contents in the hub.
- Keep command dispatch and command discovery aligned across `/settings` and `/config`.
- Settings routes unavailable as an interactive menu may still route to their canonical command response.

## Acceptance criteria

- `/config` and `/settings` open the same menu.
- `/settings auth` and `/config auth` open Authentication.
- `/settings skills` and `/config skills` open Skills.
- The Settings menu visibly lists all supported configuration domains.
- Existing editable runtime rows remain reachable.
- Registry metadata advertises the alias and direct routes.
- Focused tests, `cargo test -p omegon`, lint, and self-link pass before commit.

## Open Questions

None for the first implementation slice.
