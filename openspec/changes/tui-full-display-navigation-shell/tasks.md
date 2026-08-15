# Tasks: Full-display TUI navigation shell

> Historical branch checklist. Checked items record work on the original branch,
> not verification of this rebased tree. Current ownership uses `../tui-dual-presentation/`.

Dependencies: Group 2 depends on Group 1. Group 3 depends on Groups 1–2. Group 4 depends on Group 3. Group 5 validates all prior groups.

## 1. Navigation contracts and state machine
<!-- specs: tui-navigation -->

- [ ] 1.1 Add `surfaces::navigation` destination and breadcrumb contracts without Ratatui types or TUI-local selection/filter state.
- [ ] 1.2 Add canonical Settings section identities and one parser/router for `/settings <section>`, `/config <section>`, and every retained task-oriented alias; define invalid/unavailable destination diagnostics.
- [ ] 1.3 Add TUI `NavigationSession` stack entries that preserve destination-local `MenuState` while rebuilding live projections; restore selection by row identity and clamp safely after refresh.
- [ ] 1.4 Add the single `InteractiveSurface` enum and return-target/pending-prompt model that replaces simultaneous top-level menu, selector, process, copy, prompt, mention-picker, and extension-modal ownership.
- [ ] 1.5 Write failing tests for push/pop/root close, deterministic bare Settings, complete alias equivalence, invalid routes, deep-link replacement, input consumption, queued prompt return, and identity-based local-state restoration; register the test file before production implementation.

## 2. Exclusive rendering and two-terminal ownership
<!-- specs: tui-navigation -->

- [x] 2.1 Add a pure `TerminalPresentation` transition state machine for bounded primary-screen inline conversation and distinct alternate-screen fullscreen interaction; cover entry, exit, repeated cycles, partial acquisition rollback, and idempotent restoration before production wiring.
- [x] 2.2 Keep the inline `Terminal` alive but dormant while fullscreen owns a separately constructed `Terminal`; preserve the existing `App`, coordinator, channels, event drains, scheduler, and canonical conversation state in one `run_tui` loop.
- [x] 2.3 Acquire and release alternate-screen and fullscreen-only mouse modes symmetrically and only after successful terminal commands; never insert native transcript publications into the fullscreen terminal.
- [x] 2.4 Compose compact conversation in the bounded inline frame and short-circuit fullscreen rendering so the active exclusive surface clears, styles, and owns every cell in its `frame.area()`.
- [ ] 2.5 Refactor `render_menu_surface` with an explicit fullscreen layout mode/content area rather than relying on centered-modal geometry; derive paging from rendered row capacity with one-row overlap.
- [ ] 2.6 Queue passive notifications during exclusive ownership and route blocking prompts through `InteractiveSurface` with an explicit return target.
- [ ] 2.7 Add normal-height and constrained-height buffer/snapshot tests for fixture absence, selected-row visibility, overflow indicators, complete cell styling, physical resize, deferred exactly-once publication, and restoration after repeated cycles.

## 3. Canonical Settings hierarchy and deep links
<!-- specs: tui-navigation -->

- [ ] 3.1 Route `/settings`, `/config`, all `/settings <section>` forms, and every retained task-oriented alias through canonical destination identities; reject unknown or unavailable sections without discarding active state.
- [ ] 3.2 Replace Settings rows that recursively execute slash commands with explicit navigation actions.
- [ ] 3.3 Dispatch each Settings destination to its existing independent live projection builder and mark rows without a registered editor/destination visibly unavailable.
- [ ] 3.4 Add an explicit action capability/availability contract so Enter cannot emit unsupported mutation commands and completed editors refresh their parent projection.
- [ ] 3.5 Update tests to assert destination identity, breadcrumb, complete alias parity, unsupported action behavior, and deep links invoked while navigation is already active.

## 4. Child interactions and non-settings surfaces
<!-- specs: tui-navigation -->

- [ ] 4.1 Preserve parent navigation state and explicit return targets while opening, confirming, and cancelling selectors, prompts, or inline value editors.
- [ ] 4.2 Implement one keyboard/paste/mouse dispatcher and Escape precedence for transient input, pending confirmation, child interaction, destination pop, and root close; prove events cannot fall through to composer handling.
- [ ] 4.3 Move process/session detail and copy/document viewers to full-display inspector variants outside the Settings breadcrumb.
- [ ] 4.4 Adapt legacy extension modal and action-required payloads to bounded exclusive variants with FIFO/no-clobber admission and return-target restoration.
- [ ] 4.5 Test selector confirm/cancel, filtered-list refresh/reorder/removal, blocking prompt ownership, document/process inspector classification, extension-event concurrency, and composer-state preservation.

## 5. Regression validation and lifecycle reconciliation
<!-- specs: tui-navigation -->

- [ ] 5.1 Run focused navigation, Settings, Extensions, selector, render, input-arbitration, extension-concurrency, and terminal-geometry tests.
- [ ] 5.2 Run `just test-crate omegon` and `just clippy-changed`.
- [ ] 5.3 Reconcile implementation scope and completed tasks in this file, register test evidence and lifecycle progress, and move the bound design node to `implemented` only after all scenarios pass.
- [ ] 5.4 Produce a scenario-to-test evidence table covering every scenario in `specs/tui-navigation.md`; archive only when it has no missing or ambiguous evidence.
