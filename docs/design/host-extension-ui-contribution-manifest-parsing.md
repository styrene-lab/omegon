+++
id = "a743f4ca-cdd7-4516-bf53-20fdd4f933ed"
kind = "design_node"

[data]
title = "Host-Side Extension UI Contribution Manifest Parsing"
status = "exploring"
issue_type = "design"
priority = 2
dependencies = []
open_questions = []
+++

# Host-Side Extension UI Contribution Manifest Parsing

## Overview

Reincarnate the still-relevant host-side portion of stale PR #105 after the in-tree `omegon-extension` SDK crate was removed from this repository. The old branch mixed standalone SDK protocol types, host manifest parsing, Pkl schema changes, and broad roadmap/design docs. Current `main` only needs a smaller host-focused slice: parse and validate declarative `[ui]` extension manifest contributions without rendering or registering them yet.

This design node preserves the useful #105 concept while avoiding the obsolete in-tree SDK edits.

## Source context

Stale PR: [#105 feat(extensions): add UI contribution schema](https://github.com/styrene-lab/omegon/pull/105)

Relevant salvage from #105:

- `capabilities.ui_contributions` boolean with legacy default `false`.
- `[ui]` manifest section for extension-contributed operator surfaces.
- Host parser structs for commands, status items, and surfaces.
- Pkl schema support in `pkl/ExtensionManifest.pkl`.
- Tests for parsing a manifest containing UI contributions.

Obsolete from #105:

- `core/crates/omegon-extension/*` changes. The internal SDK crate has been removed from this repository and extracted to the standalone SDK.
- Broad 0.25 roadmap/design docs that overlap with newer `main` SDK extraction docs.
- Any runtime registration/rendering behavior; #105 intentionally did not implement rendering.

## Proposed manifest shape

Candidate host manifest model, re-authored against current `main`:

```toml
[capabilities]
ui_contributions = true

[ui]
namespace = "reader"
description = "Reader operator surfaces"

[[ui.commands]]
id = "open"
title = "Open Reader"
slash = "/reader open"
tool = "reader_open"

[[ui.status_items]]
id = "reader-status"
title = "Reader"
refresh_tool = "reader_status"
interval_ms = 10000
template = "{state}"

[[ui.surfaces]]
id = "reader"
title = "Reader"
surface_type = "document_reader"
rendering = "delegated"
preferred_placements = ["side_pane", "new_tab", "external"]
open_tool = "reader_open"
status_tool = "reader_status"
```

Primitive host-rendered list surfaces remain a candidate but may be deferred if they expand scope too much.

## Open Questions

- [assumption] Host-side parsing belongs in `core/crates/omegon/src/extensions/manifest.rs` and Pkl schema support belongs in `pkl/ExtensionManifest.pkl`.
- [assumption] `capabilities.ui_contributions` should default to `false` so existing manifests preserve behavior.
- [assumption] The host may parse `[ui]` declarations before runtime registration/rendering exists, as long as the parsed data is inert.
- Should `[ui]` be accepted only when `capabilities.ui_contributions = true`, or parsed regardless and ignored unless capability is enabled?
- Should parser validation verify that referenced tools exist in `get_tools`, or is that a later startup/runtime validation step?
- Are command slashes allowed to claim top-level names, or must they be namespaced under extension namespace?
- Which surface placements are valid in this repo today after terminal/background-session work: `side_pane`, `bottom_pane`, `modal`, `new_tab`, `external`, `background_session`?
- Should `rendering = "host"` primitive views be included in v1 parsing, or should v1 only parse delegated surfaces?
- Should UI contribution declarations be exposed through ACP initialize/session metadata immediately, or remain internal until runtime registration is designed?

## Candidate decisions to evaluate

### Decision candidate: host parses but does not render in first PR

Add host manifest/Pkl parsing and tests only. Parsed UI contributions remain inert until a separate runtime registration/rendering design lands.

Tradeoff: low-risk schema substrate; no immediate operator-visible UI.

### Decision candidate: require explicit capability opt-in

Only treat `[ui]` declarations as active when `capabilities.ui_contributions = true`. Missing capability means legacy behavior.

Tradeoff: prevents accidental activation; may surprise extension authors if `[ui]` silently parses but is inactive.

### Decision candidate: parse strings first, enums later

Use string fields for rendering and placement in the host parser initially, with Pkl validation constraining allowed values. Rust enums can follow once runtime semantics stabilize.

Tradeoff: less type safety now; easier compatibility with evolving surface taxonomy.

## Implementation scope

Files likely in scope:

- `core/crates/omegon/src/extensions/manifest.rs`
- `pkl/ExtensionManifest.pkl`
- tests in `core/crates/omegon/src/extensions/manifest.rs` or nearby extension manifest tests
- `CHANGELOG.md`

Explicitly out of scope:

- standalone SDK exported types
- TUI rendering
- ACP UI registration
- command palette integration
- statusline/status item refresh loops
- host-rendered primitive view execution

## Success criteria

- Extension manifests with `[ui]` parse successfully on current `main`.
- Existing manifests without `[ui]` or `capabilities.ui_contributions` continue to parse with default inert behavior.
- Pkl schema accepts and validates the proposed `[ui]` shape.
- Tests cover commands, status items, delegated surfaces, and defaults.
- The old PR #105 can remain closed with this design node as the host-side replacement path.

## Open Questions
