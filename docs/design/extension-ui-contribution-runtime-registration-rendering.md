+++
id = "c3785312-e772-4a10-8324-f8fe0c5961ae"
kind = "design_node"

[data]
title = "Extension UI Contribution Runtime Registration and Rendering"
status = "seed"
issue_type = "design"
priority = 3
dependencies = []
open_questions = []
+++

# Extension UI Contribution Runtime Registration and Rendering

## Overview

Track the later runtime work intentionally left out of stale PR #105 and the host-side manifest parsing salvage. After the host can parse `[ui]` declarations and the standalone SDK can export matching protocol types, Omegon still needs a runtime path that turns parsed contributions into operator-visible surfaces.

This node exists to keep parsing/schema work from expanding into rendering prematurely.

## Problem statement

Declarative UI contributions are only useful if the host can eventually register and render them safely. However, runtime UI integration crosses several volatile surfaces:

- command palette and slash command routing;
- statusline/status item refresh loops;
- TUI panes, modals, and tabs;
- delegated external surfaces such as reader/browser/terminal backends;
- policy boundaries for contributed actions and host-managed rendering;
- ACP/client metadata exposure.

This is too broad to bundle with manifest parsing.

## Dependency chain

Depends on:

- [[Host-Side Extension UI Contribution Manifest Parsing]]
- [[Standalone SDK UI Contribution Protocol Types]]

Related current design/issues:

- focus mode/status visibility work
- terminal background session visibility
- resource/browser/terminal HostAction domain split
- extension push notification routing
- ACP host action approval

## Candidate runtime surfaces

### Commands

Extension manifests may contribute commands such as:

```toml
[[ui.commands]]
id = "open"
title = "Open Reader"
slash = "/reader open"
tool = "reader_open"
```

Runtime questions:

- Register as slash commands, command palette entries, or both?
- Require namespace prefix?
- How are conflicts resolved?
- Does command invocation call extension tools directly or produce a host action?

### Status items

Extension manifests may contribute passive status items:

```toml
[[ui.status_items]]
id = "reader-status"
refresh_tool = "reader_status"
interval_ms = 10000
template = "{state}"
```

Runtime questions:

- Where do they render in Slim vs Full UI?
- What refresh cadence is allowed?
- How are failures/degraded states represented?
- Can status items produce notifications or only passive labels?

### Surfaces

Extension manifests may contribute delegated or host-rendered surfaces:

```toml
[[ui.surfaces]]
id = "reader"
rendering = "delegated"
preferred_placements = ["side_pane", "new_tab", "external"]
open_tool = "reader_open"
```

Runtime questions:

- Which placements are supported by Omegon TUI vs external hosts?
- How does a surface open/focus/close lifecycle work?
- How does delegated rendering differ from host-rendered primitive views?
- What capability/policy checks apply before opening a surface?

## Open Questions

- [assumption] Runtime registration should be a separate feature after schema parsing lands.
- [assumption] Slash command conflicts must be resolved by namespace qualification, not first-writer-wins.
- [assumption] Status item refresh tools need rate limits and failure backoff.
- Should contributed commands be visible to the LLM tool surface, operator command palette, or only operator UI?
- Should contributed surfaces be exposed to ACP clients in initialize/session metadata?
- How should headless/daemon mode represent UI contributions when no local TUI exists?
- Can host-rendered primitive views safely interpolate extension-provided templates, and what escaping rules are required?
- Should surface opening route through HostAction approval for delegated/external placements?
- What is the minimum runtime slice that proves the manifest schema is useful without overbuilding the UI system?

## Candidate decisions to evaluate

### Decision candidate: runtime starts with command palette only

The first runtime slice registers contributed commands as operator-visible command palette entries that call extension tools. Status items and surfaces wait.

Tradeoff: lowest visible integration; does not validate surface model.

### Decision candidate: runtime starts with delegated surfaces only

The first runtime slice supports `rendering = "delegated"` surfaces where an extension-owned tool returns or opens a host-managed resource/session.

Tradeoff: validates Reader/Browser/Terminal-like use cases; requires placement/policy decisions.

### Decision candidate: no contributed slash commands without namespace

All contributed command routes must live under the extension namespace, e.g. `/reader open`, not `/open`.

Tradeoff: prevents collisions; less convenient for extension authors.

## Implementation constraints

- Do not allow extensions to seize global keybindings or top-level slash names without explicit host policy.
- UI contribution rendering must not bypass HostAction approval or extension manifest permissions.
- Status refresh loops must not create unbounded background work.
- Host-rendered primitive views must escape templates according to the render target.
- Runtime registration should degrade cleanly in headless mode.

## Success criteria

- Parsed UI contributions can be registered into a runtime registry with conflict detection.
- At least one contribution type becomes operator-visible under a controlled namespace.
- Runtime behavior is covered by tests for conflict handling, disabled capability, and extension tool failure.
- Rendering/registration does not expand the model-facing tool surface unexpectedly.

## Open Questions
