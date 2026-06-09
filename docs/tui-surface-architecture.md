# TUI Surface Architecture

Omegon's TUI rendering has been split into three layers so semantic state, protocol adapters, and Ratatui rendering can evolve independently.

## Layer model

```text
backing state / events
  -> shared semantic surfaces (`surfaces::*`)
  -> adapter layer (`tui::*`, `acp::surfaces`)
  -> concrete renderer / protocol output
```

## Shared semantic surfaces

Shared surface contracts live under:

```text
core/crates/omegon/src/surfaces/
```

Current modules:

- `conversation` — typed conversation segment projections and tool categories.
- `footer` — engine/context/memory/session/workspace status projection.
- `dashboard` — lifecycle/dashboard semantic projection.
- `editor` — editor/input semantic projection.
- `instruments` — instrument panel semantic projection.
- `layout` — high-level UI surface preset state.

Rules:

1. `surfaces::*` must not import Ratatui.
2. `surfaces::*` must not import ACP wire types.
3. `surfaces::*` should describe semantic state, not colors, borders, `Rect`s, or protocol field names.
4. TUI-owned backing state implements projection traits in TUI adapter modules when needed.

## ACP adapter

ACP owns wire DTOs, redaction, identity/revision, and extension notifications in:

```text
core/crates/omegon/src/acp/surfaces.rs
```

Conversation surface behavior:

- Flynt receives `_surface/conversation/update` by default.
- Zed remains on standard ACP `SessionUpdate` by default.
- `OMEGON_ACP_SURFACE_UPDATES=1` forces surface update emission for debugging/other clients.
- ACP DTOs are derived from semantic conversation projections, not from TUI render structs.

Rules:

1. `acp::surfaces` must not import `tui` semantic projection modules.
2. ACP redaction policy stays in ACP adapter code, not in `surfaces::*`.
3. ACP update identity must include stable segment id, optional turn id, sequence, and revision.

## TUI layout and sub-surfaces

Top-level Ratatui area allocation lives in:

```text
core/crates/omegon/src/tui/layout_projection.rs
```

This owns slim/full mode `Rect` allocation for:

- conversation
- editor/input
- status/footer
- dashboard
- active tool stream
- permission lane
- slim plan panel

Small TUI render islands now live in:

```text
core/crates/omegon/src/tui/active_tool_stream.rs
core/crates/omegon/src/tui/permission_lane.rs
core/crates/omegon/src/tui/slim_plan.rs
core/crates/omegon/src/tui/extension_overlays.rs
core/crates/omegon/src/tui/focus_view.rs
core/crates/omegon/src/tui/tab_bar.rs
```

Rules:

1. `tui/mod.rs` should orchestrate state and dispatch, not own visual policy.
2. Slim/full surface visibility and area allocation belongs in `layout_projection.rs`.
3. Surface-specific rendering belongs in the smallest relevant TUI module.

## Conversation segment components

Conversation segment render bodies live under:

```text
core/crates/omegon/src/tui/segment_components/
```

Current components:

- `assistant.rs`
- `image.rs`
- `lifecycle.rs`
- `separator.rs`
- `system.rs`
- `tool_card.rs`
- `user_prompt.rs`

All component render entrypoints now use `SegmentRenderContext`:

```rust
render(props, area, buf, ctx)
```

`SegmentRenderContext` currently carries:

- theme
- render mode
- tool detail density
- pinned state

Rules:

1. Component public entrypoints should accept `&SegmentRenderContext<'_>`.
2. Internal helper props may keep narrower fields such as `&dyn Theme` when that is all they need.
3. `segments.rs` should remain segment data/projection/glue plus shared utilities/tests; avoid adding new large render bodies there.
4. New segment variants should add a component module or extend an existing component, not render inline in `segments.rs`.

## TUI render adapters

Conversation-specific Ratatui chrome mapping lives in:

```text
core/crates/omegon/src/tui/conversation_render_projection.rs
```

It maps semantic conversation concepts to Ratatui chrome, including:

- segment role/category chrome
- tool category color
- tool card chrome
- segment render traits/context

Rules:

1. Semantic classification stays in `surfaces::conversation`.
2. Color, icon, border, and terminal layout mapping stays in TUI render adapters/components.

## Current readiness

The decoupling foundation is complete enough to begin Ratatui/component architecture work. Remaining worthwhile cleanup is mostly internal helper extraction from `segments.rs` into smaller utility modules if/when a render change touches those helpers.
