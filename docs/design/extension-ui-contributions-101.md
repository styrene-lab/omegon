---
title: Extension UI Contributions (#101)
status: exploring
tags: [0.25, extensions, ui, tui, sdk]
---

# Extension UI Contributions (#101)

## Problem

Extensions need a declarative way to contribute operator-facing commands, passive status items, and host/delegated surfaces without hardcoding extension-specific behavior into Omegon or Flynt.

Reader is the dogfood case, but the design must remain generic.

## Goal

Define a stable extension UI contribution schema for 0.25:

- slash-command contributions
- status item contributions
- delegated surfaces such as a Reader/document pane
- host-rendered primitive surfaces such as list views
- capability-gated legacy-safe manifest parsing

## Current local status

Initial substrate exists on branch:

```text
feature/extension-ui-contributions-101
```

Commit:

```text
ca05ecae feat(extensions): add UI contribution schema
```

It includes:

- `Capabilities::ui_contributions`
- `omegon-extension::ui_contributions` SDK types
- `[ui]` manifest structs in SDK and host parsers
- Pkl schema support
- SDK round-trip tests for Reader delegated surface and Scratchpad host list surface

## Decisions

### Decision: Schema/protocol first

Ship #101 in slices. The first slice only defines capability, manifest/schema, SDK structs, and parser tests. Rendering/routing comes after schema stability.

Cost: user-visible UI contributions are not complete in the first PR.
Benefit: downstream SDKs and first-party extensions can start declaring intent without host UI coupling.

### Decision: Reader is a consumer, not a special case

Reader should declare a delegated `document_reader` surface through `[ui]`. Omegon should not special-case `omegon-reader` by name.

### Decision: Keep manifest structs separate from SDK payload structs for now

Manifest parser structs can remain manifest-specific while SDK structs represent protocol payloads. Add conversion tests before runtime registration.

Cost: duplicate type definitions can drift.
Mitigation: conversion/round-trip tests and eventual schema contract artifact (#102).

## Open questions

- [assumption] A first-slice manifest parser can accept `[ui]` declarations without immediately registering commands in the TUI.
- [assumption] `namespace` should be unique per extension and host-normalized on collision.
- How should slash commands resolve conflicts: extension namespace prefix only, explicit operator aliases, or first-install wins?
- Should delegated surfaces require a paired HostAction such as `terminal.create@1` or `resource.open@1`, or can `open_tool` remain an arbitrary extension tool?

## Acceptance for first slice

- Legacy manifests parse with `ui_contributions = false` by default.
- `[capabilities] ui_contributions = true` parses in SDK and host manifest loaders.
- `[ui]` commands/status/surfaces parse from TOML.
- SDK exports typed contribution structs.
- Pkl schema validates the envelope.
- Tests cover Reader delegated surface and host-rendered list primitive shape.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[terminal-background-session-visibility-104]]
- [[resource-open-host-action-83]]
