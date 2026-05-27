# Design: Extension Operator Surface Contributions

Canonical design node: `docs/design/extension-operator-surface-contributions.md`.

## Architecture

```text
Extension manifest envelope
  -> runtime ui/list_contributions
  -> host validation and namespace resolution
  -> accepted contribution registry
  -> host/Cockpit/TUI/Flynt render or route contributions
```

## Contribution vocabulary

MVP contribution kinds:

- `command`
- `status_item`
- `surface`

Surface contributions include:

- `rendering = delegated | host`
- `surface_type`
- `preferred_placements`

Reader uses delegated rendering with `surface_type = document_reader`.
Scratchpad uses host rendering with `surface_type = primitive_view` and
`view.primitive = list`.

## Host validation

Runtime contributions must not exceed the manifest envelope. The host validates:

- namespace
- contribution kind
- command ids/slash aliases/tools
- status refresh tool and minimum interval
- surface ids/types/rendering/preferred placements
- primitive view schema for host-rendered surfaces
- tool ownership

Invalid contributions are rejected with diagnostics; valid contributions are
stored in a registry.

## Safety

Extensions never draw directly into the terminal. Host-rendered primitive views
use blessed schemas; delegated surfaces go through host-selected placement and
HostAction policy.
