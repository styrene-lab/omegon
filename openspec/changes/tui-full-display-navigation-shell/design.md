# Design: Full-display TUI navigation shell

> Historical branch design. Current terminal ownership is specified by
> `../tui-dual-presentation/`; this record does not replace that contract.

## Context

Omegon runs Ratatui in an inline viewport sized for compact conversation fixtures. Structured menus are currently rendered inside that compact frame using centered modal geometry. The result is an unusably small menu viewport and fragmented parent/child interaction state.

The existing settings projection already identifies configuration areas, and existing slash commands already open most area-specific projections. This change introduces a canonical destination model and exclusive display ownership without flattening those independent projections.

## Architecture

### Exclusive interactive surface owner

`App` gains a single top-level interactive-surface owner. Its first implementation supports navigation sessions while retaining compatibility adapters for existing inspectors and extension payloads. During rendering, an active exclusive surface short-circuits normal conversation layout and receives the complete frame.

The interactive owner is distinct from transient state such as toast notifications. Blocking prompts, selectors, and inline value editors are represented as child interaction scopes so input is routed to the nearest scope first.

### Input arbitration

Rendering geometry does not establish ownership. All keyboard, paste, and mouse events pass through one dispatcher whose state is a single exclusive-surface enum. The active variant consumes or rejects the event before conversation/composer handling can run. Passive notifications have no input path.

The migration must eliminate simultaneously active top-level states rather than impose a precedence order over `active_menu`, `selector`, `process_viewer`, `command_prompt`, copy state, `at_picker`, and extension modal fields. Compatibility producers enqueue or transition to one of these semantic variants:

- `Navigation` — settings, menus, help, and inventory/detail routes;
- `Inspector` — process/session, document/copy, evidence, and tool detail;
- `Picker` — model, context, secret, mention/file, and similar bounded selection;
- `Prompt` — permission, responder-backed decision, confirmation, and extension action request;
- `LegacyExtension` — bounded compatibility adapter for arbitrary extension payloads.

Input precedence inside a variant is nearest-scope first: transient text/search editor, pending confirmation, child picker/prompt, destination, then root close. Background producers may queue a blocking prompt but may not replace an active surface or discard its navigation state. When accepted for display, the prompt retains an explicit return target and resumes the prior surface after resolution.

### Renderer-neutral navigation contract

`surfaces::navigation` defines only canonical destination identities, semantic breadcrumbs, shell titles, and navigation affordances. It does not contain selection indexes, filters, Ratatui keys, geometry, or concrete `MenuState`.

TUI-owned `navigation.rs` owns destination-local `MenuState` snapshots and resolves destination identities to existing menu projection builders. Projection data is rebuilt from live backing state rather than retained in navigation history. State restoration is identity-based: the selected row ID is restored if still visible, otherwise selection clamps to the first available row; filters are retained; unavailable destinations return to the nearest valid parent with a diagnostic. Reordering never retargets an action by stale numeric index.

### Canonical destination routing

Slash parsing and Settings rows route through one destination resolver. `/settings` opens `SettingsRoot`; `/settings extensions`, `/extension`, and `/extensions` open the same `SettingsSection::Extensions` identity. Settings rows use navigation effects directly and do not recursively execute slash commands.

### Navigation stack

A navigation session retains a stack of destination entries. Each entry stores identity plus local menu state. Pushing a detail page preserves the parent entry. Popping restores it. A child selector or inline editor is transient state attached to the current entry and returns to that entry on completion or cancellation.

Escape precedence is:

1. cancel transient editor/search/confirmation;
2. close child selector/prompt and restore its parent;
3. pop one destination;
4. close the root and restore conversation mode.

### Rendering and terminal ownership

Native-transcript conversation retains its bounded primary-screen inline `Terminal` for the entire session. The inline terminal is the only target allowed to call `insert_before`, preserving completed exchanges in native scrollback. Opening a root interactive surface enters the alternate screen and constructs a distinct fullscreen `Terminal`; the inline terminal remains alive but dormant until primary-screen ownership returns.

The fullscreen transition is presentation-only. `App`, the coordinator, command and cancellation channels, event receivers, scheduler, and canonical conversation remain in the same `run_tui` loop. Entering fullscreen must not await agent completion, suspend drains, or introduce a second transcript queue. While fullscreen is active the committed `TranscriptPublicationCursor` remains unchanged and completed records remain in canonical conversation state. On return, primary-screen ownership is restored before pending records are inserted and the cursor is committed only after insertion succeeds.

Terminal-mode acquisition and release are success-ordered and symmetric. The session guard records alternate-screen and fullscreen-only mouse capture only after the corresponding terminal command succeeds, and clears each record only after release succeeds. A failed partial transition rolls back the modes actually acquired; panic, ordinary error, repeated entry/exit, and final `Drop` restoration remain idempotent.

Every exclusive renderer must style every cell in `frame.area()` of the fullscreen terminal. Passive notifications are queued while an exclusive surface is active; they may not visually or interactively preempt it. A blocking permission or responder-backed prompt is itself an exclusive surface transition, not a late overlay.

### Settings action availability

The navigation shell does not imply universal editability. Settings rows may attach an action only when the runtime implements that editor or destination. Unsupported projected editors are visibly unavailable with a diagnostic reason and cannot advertise an enabled primary action. Enter invokes the row's declared enabled action; it does not promise that every projected row can be edited.

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
