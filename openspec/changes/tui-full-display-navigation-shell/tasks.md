# Tasks: Full-display TUI navigation shell

Dependencies: Group 2 depends on Group 1. Group 3 depends on Groups 1–2. Group 4 depends on Group 3. Group 5 validates all prior groups.

## 1. Navigation contracts and state machine
<!-- specs: tui-navigation -->

- [ ] 1.1 Add `surfaces::navigation` destination, breadcrumb, and transient-interaction contracts without Ratatui types.
- [ ] 1.2 Add canonical Settings section identities and parsing for `/settings <section>` and task-oriented aliases.
- [ ] 1.3 Add TUI `NavigationSession` stack entries that preserve destination-local `MenuState` while rebuilding live projections.
- [ ] 1.4 Test push, pop, root close, deterministic bare Settings, alias equivalence, and local-state restoration.

## 2. Exclusive rendering and viewport ownership
<!-- specs: tui-navigation -->

- [ ] 2.1 Introduce one top-level interactive-surface owner in `App` and route active navigation through it.
- [ ] 2.2 Short-circuit `App::draw` so active interactive surfaces clear and own the complete frame without conversation or permanent fixtures.
- [ ] 2.3 Refactor `render_menu_surface` to render into a supplied content area without mandatory `command_modal_area` margins.
- [ ] 2.4 Add normal-height and constrained-height buffer/snapshot tests, including selected-row visibility and overflow indicators.
- [ ] 2.5 Expand and restore the inline viewport on interactive-surface entry/exit without publishing transcript content; test restoration across terminal resize.

## 3. Canonical Settings hierarchy and deep links
<!-- specs: tui-navigation -->

- [ ] 3.1 Route `/settings`, `/config`, `/extension`, `/extensions`, `/skills`, `/model`, `/auth`, and supported settings sections through canonical destination identities.
- [ ] 3.2 Replace Settings rows that recursively execute slash commands with explicit navigation actions.
- [ ] 3.3 Dispatch each Settings destination to its existing independent live projection builder.
- [ ] 3.4 Update tests to assert destination identity and breadcrumb equivalence rather than independent popup IDs.

## 4. Child interactions and non-settings surfaces
<!-- specs: tui-navigation -->

- [ ] 4.1 Preserve parent navigation state while opening and cancelling selectors or inline value editors.
- [ ] 4.2 Implement Escape precedence for transient input, child interaction, destination pop, and root close.
- [ ] 4.3 Move process/detail and copy/document viewers to full-display inspector destinations outside the Settings breadcrumb.
- [ ] 4.4 Adapt legacy extension modal payloads to an exclusive compatibility page and prohibit concurrent conversation rendering.
- [ ] 4.5 Test selector return, filtered-list detail return, blocking input ownership, and inspector classification.

## 5. Regression validation and lifecycle reconciliation
<!-- specs: tui-navigation -->

- [ ] 5.1 Run focused navigation, Settings, Extensions, selector, render, and terminal-transition tests.
- [ ] 5.2 Run `just test-crate omegon` and `just clippy-changed`.
- [ ] 5.3 Reconcile implementation scope and completed tasks in this file and register lifecycle progress.
- [ ] 5.4 Verify every scenario in `specs/tui-navigation.md` against named test evidence before archive.
