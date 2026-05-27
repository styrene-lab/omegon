# Extension Operator Surface Contributions

Status: exploring
Issue: #101
Primary dogfood target: omegon-reader

## Problem

Extensions need operator-facing affordances beyond raw tools: slash commands, command-palette entries, passive status items, and host-managed reading/panel surfaces. The existing extension boundary exposes tools and declarative HostActions, but it does not let an extension install a discoverable operator UX such as reader open/status commands, a reader footer item, or a host-selected reader surface.

Reader is the right MVP because it exercises the hard parts of the boundary: command contribution, passive status contribution, surface intent with preferred placements, HostAction approval/backend degradation, and strict prohibition on extension-owned terminal drawing.

## Core decision

Extensions declare operator-surface contributions. They do not draw UI, mutate host registries, or claim terminal layout directly.

```text
Extension manifest/runtime declaration
  -> Omegon validates namespace + permission envelope
  -> host registry stores accepted contributions
  -> Cockpit/TUI/Flynt render commands, status items, and surfaces
  -> HostActions execute only through existing approval/policy paths
```

Panels are host placements, not extension-owned drawing contexts.

```text
contribution kind: surface
placement choice: side_pane | bottom_pane | new_tab | external | background_session
```

A Reader extension can declare a document_reader surface and prefer side_pane, then new_tab, then external. The host chooses the actual placement based on active capabilities.

## Non-goals for the MVP

- No raw extension drawing in the terminal.
- No arbitrary ANSI rendering from extensions.
- No direct keybinding install without explicit future approval policy.
- No imperative mutation of slash command registries by extensions.
- No requirement that TUI implement a visual reader pane in the first slice.
- No automatic HostAction execution outside existing approval/policy seams.

## Contribution kinds

Initial protocol vocabulary:

| Kind | Meaning | MVP status |
|---|---|---|
| command | Slash command / command palette entry routed to an extension tool | MVP |
| status_item | Passive host-rendered status/footer item refreshed by a tool | MVP |
| surface | Semantic host-managed surface intent, e.g. document_reader | MVP declaration only |
| completion_provider | Argument completion source | later |
| notification | Rate-limited operator notification | later |
| keybinding | Explicit operator-approved keybinding | later |

## Reader MVP shape

### Manifest envelope

The manifest declares the install-time permission envelope. Runtime contributions must be a subset of this envelope.

```toml
[capabilities]
tools = true
host_actions = true
ui_contributions = true

[ui]
namespace = "reader"
description = "Open books and document-like files in a host-managed reader surface."

[[ui.commands]]
id = "open"
title = "Open Reader"
slash = "open"
tool = "reader_open"

[[ui.commands]]
id = "status"
title = "Reader Status"
slash = "status"
tool = "reader_status"

[[ui.status_items]]
id = "reader"
title = "Reader"
refresh_tool = "reader_status"
interval_ms = 30000
template = "reader:{state}"

[[ui.surfaces]]
id = "reader"
title = "Reader"
surface_type = "document_reader"
open_tool = "reader_open"
status_tool = "reader_status"
preferred_placements = ["side_pane", "new_tab", "external", "background_session"]
```

### Runtime contribution response

Proposed method:

```text
ui/list_contributions
```

Example:

```json
{
  "version": 1,
  "namespace": {
    "requested": "reader",
    "fallback": "omegon-reader"
  },
  "contributions": [
    {
      "kind": "command",
      "id": "open",
      "title": "Open Reader",
      "slash": "open",
      "tool": "reader_open"
    },
    {
      "kind": "command",
      "id": "status",
      "title": "Reader Status",
      "slash": "status",
      "tool": "reader_status"
    },
    {
      "kind": "status_item",
      "id": "reader",
      "title": "Reader",
      "refresh_tool": "reader_status",
      "refresh_interval_ms": 30000,
      "template": "reader:{state}"
    },
    {
      "kind": "surface",
      "id": "reader",
      "title": "Reader",
      "surface_type": "document_reader",
      "preferred_placements": ["side_pane", "new_tab", "external", "background_session"],
      "open_tool": "reader_open",
      "status_tool": "reader_status"
    }
  ]
}
```

Resolved operator UI examples:

```text
reader open command
reader status command
reader status footer item
```

## Namespace rules

- Namespace is required for any operator-visible contribution.
- Namespace must be lowercase kebab/snake/alphanumeric with separators only.
- Runtime requested namespace must match or be within manifest envelope.
- If namespace conflicts with an existing host command or extension namespace, host chooses deterministic fallback, e.g. omegon-reader.
- Conflict must be visible in diagnostics/status.
- Contribution ids are local to namespace.

## Validation rules

The host rejects or drops runtime contributions that exceed the manifest envelope:

- command id not declared in manifest
- slash alias not declared in manifest
- tool not owned by that extension
- status item refresh interval below host minimum
- status template references unsupported keys
- surface type not declared in manifest
- preferred placement not supported by host policy vocabulary
- raw drawing/ANSI/panel-content claims

## Relationship to HostActions

Reader surface opening can start with current HostAction primitives using terminal.create@1, but the protocol should evolve toward semantic surface actions such as surface.open@1.

For the MVP, surface contributions describe the operator affordance and preferred placement. Actual opening may still route through reader_open and terminal.create@1 until a surface.open@1 HostAction exists.

## MVP implementation slices

### Slice 1: protocol/schema only

- Add ui_contributions capability.
- Add SDK types for contribution sets, namespaces, command/status/surface contributions.
- Add manifest parser support for ui commands, status items, and surfaces.
- Add JSON/TOML round-trip tests using Reader examples.
- No TUI rendering yet.

### Slice 2: host validation/registry

- During extension spawn, call ui/list_contributions only when capability is enabled.
- Validate runtime contributions against manifest envelope.
- Store accepted contributions in a host registry.
- Expose diagnostics through a status/debug path.

### Slice 3: Reader command routing

- Register accepted command contributions into slash parser/command palette.
- Route reader open/status commands to declared tools.
- Preserve namespace conflict diagnostics.

### Slice 4: Reader status item

- Poll declared refresh tool with rate limits.
- Render reader state in footer/status.
- Degrade silently if extension unavailable; surface diagnostics in details.

### Slice 5: semantic surfaces

- Register surface contributions.
- Map Reader document_reader intent into available host surface backends.
- Keep terminal/bookokrat fallback as backend, not protocol.

## Open questions

- [assumption] Cockpit/TUI command palette has a stable registry entry point.
- [assumption] Slash command routing can call extension tools without reopening the entire prompt submission path.
- Should status item refresh use tool calls, extension notifications, or both?
- Should surface.open@1 be introduced in this change or deferred until after Reader command/status MVP?
- What is the exact namespace conflict display in Slim mode?

## Acceptance for first MVP

- Reader manifest can declare command/status/surface contribution envelope.
- Reader runtime can return matching ui/list_contributions payload.
- Omegon SDK/protocol tests validate the payload shape.
- Host validates runtime contributions against manifest envelope.
- Host rejects raw drawing claims.
- At least a reader status diagnostic can show accepted contributions before full visual rendering lands.

## Surface rendering modes

Surface contributions have two independent axes:

```text
rendering: host | delegated
placement: side_pane | bottom_pane | modal | new_tab | external | background_session
```

`placement` says where the operator should see the contribution. `rendering`
says who owns the visual/runtime implementation.

### Delegated rendering

Delegated surfaces are for extensions that own a domain-specific UI or runtime.
Reader is the canonical MVP example.

```json
{
  "kind": "surface",
  "id": "reader",
  "title": "Reader",
  "surface_type": "document_reader",
  "rendering": "delegated",
  "preferred_placements": ["side_pane", "new_tab", "external", "background_session"],
  "open_tool": "reader_open",
  "status_tool": "reader_status"
}
```

Meaning:

- Reader owns document navigation/rendering through its backend.
- Omegon/Cockpit owns placement, lifecycle, focus, diagnostics, and policy.
- Placement can degrade honestly, e.g. side pane to external app to background
  terminal session.
- Delegated rendering still cannot draw into host frames unless the selected
  backend is host-approved.

### Host-rendered primitive surfaces

Host-rendered surfaces are for simple extensions that want to use Omegon/Cockpit
primitives rather than ship their own UI.

The extension declares data tools, action tools, and a primitive view schema. The
host renders the view using blessed primitives.

```json
{
  "kind": "surface",
  "id": "scratchpad",
  "title": "Scratchpad",
  "surface_type": "primitive_view",
  "rendering": "host",
  "preferred_placements": ["side_pane", "modal"],
  "view": {
    "primitive": "list",
    "data_tool": "scratchpad_list",
    "item": {
      "title": "{title}",
      "subtitle": "{body_preview}",
      "badge": "{tag_count}"
    },
    "actions": [
      {
        "id": "open",
        "title": "Open",
        "tool": "scratchpad_get",
        "args": { "id": "{id}" }
      }
    ]
  }
}
```

Meaning:

- Extension does not render.
- Extension provides data through declared tools.
- Host owns layout, styling, focus, scrolling, selection, and confirmation UI.
- Host-owned actions route only to declared extension tools.
- If no requested placement is available, host-rendered primitive views can
  degrade to ordinary command/tool result output.

## Initial primitive vocabulary

The first MVP should implement only one host-rendered primitive:

```text
list
```

Reserve but do not implement the broader vocabulary:

```text
table
key_value
markdown
form
card_grid
tree
log
progress
empty_state
```

The `list` primitive should be enough for a scratchpad or simple search-results
extension and prevents the protocol from overfitting to Reader's delegated
surface model.

## Host-rendered primitive safety rules

Allowed for `rendering = host`:

- data from declared tools
- limited field interpolation such as `{title}` or `{id}`
- host-owned list/card/table styling
- host-owned actions routed to declared tools
- host-owned confirmation dialogs
- host-enforced refresh intervals

Denied:

- raw ANSI
- terminal escape sequences
- arbitrary layout code
- arbitrary HTML/JS
- keyboard capture
- background refresh below host minimum
- actions targeting tools not owned by the extension
- filesystem or process side effects except through existing tool/HostAction
  policy paths

## Updated Reader/Scratchpad MVP pairing

Use two dogfood extensions to keep the model honest:

| Dogfood target | Surface mode | Proves |
|---|---|---|
| Reader | delegated document_reader | placement/backend degradation and domain UI ownership |
| Scratchpad | host primitive_view/list | simple extensions can use host primitives without owning UI |

Reader remains the primary MVP because it exercises surface placement,
HostAction approval, backend degradation, and status. Scratchpad should be the
minimal primitive-view smoke test.

## Updated slice plan

### Slice 1: protocol/schema

- Add `ui_contributions` capability.
- Add surface `rendering = host | delegated`.
- Add surface `surface_type`.
- Add `preferred_placements`.
- Add host-rendered `view.primitive = list` schema.
- Add SDK JSON round-trip tests for:
  - Reader delegated document_reader surface.
  - Scratchpad host-rendered list surface.
- Add manifest TOML parsing tests for both examples.
- No rendering yet.

### Slice 2: validation/registry

- Runtime `ui/list_contributions` discovery gated by capability.
- Runtime contribution validation against manifest envelope.
- Host rejects raw drawing claims.
- Host records accepted delegated and host-rendered surfaces in a registry.
- Diagnostics expose accepted/rejected contribution reasons.

### Slice 3: command contribution MVP

- Reader command contribution routes a namespaced slash command to `reader_open`
  or `reader_status`.
- Scratchpad command contribution routes to a simple scratchpad tool.
- Namespace conflict behavior is deterministic and visible.

### Slice 4: primitive list MVP

- Host renders a scratchpad-style list primitive from a data tool result.
- Supports title/subtitle/badge field templates.
- Supports one host-owned action routed to an extension tool.
- No arbitrary rendering or custom styling.

### Slice 5: Reader surface placement MVP

- Host registers Reader's delegated document_reader surface.
- Host chooses an available placement/backend.
- If side pane is unavailable, result reports actual placement and warning.
- Existing terminal.create@1/Bookokrat fallback remains a backend, not the
  surface protocol itself.
