# Design: Full-display TUI navigation shell

## Context

Omegon runs Ratatui in an inline viewport sized for compact conversation fixtures. Structured menus are currently rendered inside that compact frame using centered modal geometry. The result is an unusably small menu viewport and fragmented parent/child interaction state.

The existing settings projection already identifies configuration areas, and existing slash commands already open most area-specific projections. This change introduces a canonical destination model and exclusive display ownership without flattening those independent projections.

## Architecture

### Exclusive interactive surface owner

`App` gains a single top-level interactive-surface owner. Its first implementation supports navigation sessions while retaining compatibility adapters for existing inspectors and extension payloads. During rendering, an active exclusive surface short-circuits normal conversation layout and receives the complete frame.

The interactive owner is distinct from transient state such as toast notifications. Blocking prompts, selectors, and inline value editors are represented as child interaction scopes so input is routed to the nearest scope first.

### Renderer-neutral navigation contract

`surfaces::navigation` defines:

- canonical destination identities;
- semantic breadcrumbs;
- projected shell title and navigation affordances;
- destination-local state needed to restore selection/filter position;
- transient interaction kind without Ratatui key or geometry types.

TUI-owned `navigation.rs` resolves destination identities to existing menu projection builders. Projection data is rebuilt from live backing state rather than retained in navigation history.

### Canonical destination routing

Slash parsing and Settings rows route through one destination resolver. `/settings` opens `SettingsRoot`; `/settings extensions`, `/extension`, and `/extensions` open the same `SettingsSection::Extensions` identity. Settings rows use navigation effects directly and do not recursively execute slash commands.

### Navigation stack

A navigation session retains a stack of destination entries. Each entry stores identity plus local menu state. Pushing a detail page preserves the parent entry. Popping restores it. A child selector or inline editor is transient state attached to the current entry and returns to that entry on completion or cancellation.

Escape precedence is:

1. cancel transient editor/search/confirmation;
2. close child selector/prompt and restore its parent;
3. pop one destination;
4. close the root and restore conversation mode.

### Rendering and viewport transition

When an exclusive surface is active, `App::draw` clears the complete frame and renders only the interactive shell. `menu_surface` accepts its content area directly and no longer unconditionally derives centered modal geometry.

The terminal loop tracks desired presentation mode. Entering navigation expands the active inline viewport to available terminal height through the existing resize-aware terminal path; leaving restores `native_transcript_viewport_height`. Transition redraws must not publish completed conversation segments to native scrollback.

### Compatibility boundary

Process/detail viewers move behind the same exclusive surface path but remain inspectors, not Settings destinations. Existing extension modal payloads may use a compatibility page during migration. New extension UI should project typed menu, inspector, picker, or prompt semantics.

## Data flow

```text
slash command / settings row / menu action
  -> canonical DestinationId
  -> NavigationSession transition
  -> destination projection rebuilt from App backing state
  -> navigation shell semantic projection
  -> full-frame Ratatui renderer
```

## Risks and mitigations

- **Scrollback corruption:** test transition behavior separately from terminal resize behavior; never call transcript publication as part of surface entry/exit.
- **Input regressions:** centralize precedence in the interactive owner and retain focused tests for Escape, Enter, search, mouse, and action keys.
- **Large migration blast radius:** retain existing `MenuProjection` builders and migrate state ownership incrementally behind a compatibility layer.
- **Stale inventories:** history stores identities and local UI state, never inventory projections.
- **Alias drift:** test all aliases against canonical destination identity rather than only projected menu IDs.

## Validation strategy

- Unit tests for destination parsing and stack transitions.
- TUI tests for slash aliases, Settings rows, selector return, and Escape precedence.
- render-buffer tests proving conversation fixtures are absent while navigation owns the frame.
- snapshots at normal and constrained dimensions.
- terminal transition tests proving compact viewport restoration and no transcript republication.
- `just test-crate omegon` and `just clippy-changed` before landing.
