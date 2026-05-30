+++
id = "92a4ffc6-0132-437f-b198-f41e235ac698"
kind = "design_node"

[data]
title = "Standalone SDK UI Contribution Protocol Types"
status = "exploring"
issue_type = "design"
priority = 2
dependencies = []
open_questions = []
+++

# Standalone SDK UI Contribution Protocol Types

## Overview

Track the standalone SDK follow-up extracted from stale PR #105. The old PR added typed UI contribution structs under the former in-tree `core/crates/omegon-extension` crate, but that crate has since been removed from this repository and extracted to a standalone SDK. The useful protocol model should move to the standalone SDK repository instead of being revived here.

This node captures the protocol-shape work so the host parser and SDK exported types can remain lockstep without reopening the obsolete branch.

## Source context

Stale PR: [#105 feat(extensions): add UI contribution schema](https://github.com/styrene-lab/omegon/pull/105)

Useful SDK concepts from #105:

- `UiContributionSet`
- `UiNamespace`
- `UiContribution::{Command, StatusItem, Surface}`
- `CommandContribution`
- `StatusItemContribution`
- `SurfaceContribution`
- `SurfaceRendering::{Host, Delegated}`
- `SurfacePlacement::{SidePane, BottomPane, Modal, NewTab, External, BackgroundSession}`
- `PrimitiveView::List`
- list item templates and primitive actions

Old location was obsolete:

```text
core/crates/omegon-extension/src/ui_contributions.rs
```

The corresponding work now belongs in the standalone SDK repository/package.

## Protocol sketch

Candidate JSON shape:

```json
{
  "version": 1,
  "namespace": {
    "requested": "reader",
    "fallback": "omegon-reader"
  },
  "contributions": [
    {
      "kind": "surface",
      "id": "reader",
      "title": "Reader",
      "surface_type": "document_reader",
      "rendering": "delegated",
      "preferred_placements": ["side_pane", "new_tab", "external"],
      "open_tool": "reader_open",
      "status_tool": "reader_status"
    }
  ]
}
```

Host-rendered primitive list candidate:

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
        "args": {"id": "{id}"}
      }
    ]
  }
}
```

## Open Questions

- [assumption] The standalone SDK should export typed UI contribution structs, not leave extension authors to hand-roll JSON.
- [assumption] The host manifest parser and SDK protocol types should share the same field names even if they live in different repositories.
- Should the SDK expose both manifest `[ui]` config structs and runtime JSON contribution structs, or only one canonical model?
- Should `SurfaceRendering`, `SurfacePlacement`, and primitive view kinds be closed enums now, or should the SDK allow unknown values for forward compatibility?
- Should template strings such as `{title}` and `{id}` have SDK-side validation helpers?
- Should command/status/surface IDs be namespace-relative in the SDK, with the host applying namespace qualification?
- Should UI contribution protocol live in the same contract artifact as tools/resources/prompts/host actions?
- How should non-Rust SDKs derive the same types: generated from JSON Schema, hand-written, or contract tests?

## Candidate decisions to evaluate

### Decision candidate: SDK owns author-facing typed builders

The SDK should provide ergonomic constructors/builders for common contribution types while preserving serde-compatible structs for raw protocol access.

Tradeoff: better extension author UX; more API surface to keep stable.

### Decision candidate: host and SDK share a contract fixture

Add UI contribution examples to the SDK conformance/contract fixture so host and SDK drift is caught by tests.

Tradeoff: requires coordination across repos; prevents silent schema mismatch.

### Decision candidate: tolerate unknown placements/rendering values

SDK may deserialize unknown future placements as strings or `Unknown(String)` variants to avoid breaking older SDK consumers.

Tradeoff: more complex type model; better forward compatibility.

## Implementation scope

This node is for the standalone SDK repo, not this repository's host implementation.

Likely standalone SDK scope:

- exported UI contribution protocol module
- serde round-trip tests
- examples for delegated document-reader surface and host-rendered primitive list surface
- contract fixture update
- changelog/release note in SDK repo

Host repo coordination:

- [[Host-Side Extension UI Contribution Manifest Parsing]] should parse equivalent manifest fields.
- Host runtime registration/rendering remains a later node.

## Success criteria

- Standalone SDK exports typed UI contribution structs/builders.
- Round-trip tests cover delegated surface and host primitive list examples from stale #105.
- Host-side parser and SDK examples use matching field names and placement/rendering vocabulary.
- Future extension authors can declare UI contributions without depending on removed in-tree SDK code.

## Open Questions
