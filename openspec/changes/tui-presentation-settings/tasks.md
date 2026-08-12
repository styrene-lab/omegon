# Tasks: TUI presentation settings and capability remediation

Dependencies: Group 2 depends on Group 1. Group 3 depends on Groups 1–2. Group 4 validates all prior groups.

## 1. Preference and resolution contracts
<!-- specs: tui-presentation -->

- [x] 1.1 Add serializable named theme and glyph preference settings with backwards-compatible defaults and profile persistence.
- [x] 1.2 Resolve requested, detected, and effective glyph profiles without conflating operator intent with capability evidence.
- [x] 1.3 Add unit tests for settings parsing/defaults/profile application and glyph resolution.

## 2. Semantic settings and runtime editors
<!-- specs: tui-presentation -->

- [x] 2.1 Expand the renderer-neutral UI settings tab with presentation, density, splash, theme, glyph, effective capability, and remediation rows.
- [x] 2.2 Register selectors and application outcomes for every editable UI choice.
- [x] 2.3 Apply theme and glyph changes live while persisting settings and refreshing the parent projection.

## 3. Theme registry and Nerd Font remediation
<!-- specs: tui-presentation -->

- [x] 3.1 Replace the hardwired Alpharius loader with a stable named registry and safe fallback.
- [x] 3.2 Bundle an initial Styrene semantic theme alongside Alpharius.
- [x] 3.3 Expose bounded Nerd Font capability evidence, specimen/help information, and external help action without automatic host mutation.

## 4. Verification and lifecycle reconciliation
<!-- specs: tui-presentation -->

- [x] 4.1 Add focused TUI tests for projected rows, active choices, selector application, warning status, and remediation.
- [x] 4.2 Run focused tests, `just test-crate omegon`, and `just clippy-changed`.
- [x] 4.3 Reconcile task, design-node, and OpenSpec state and commit the completed slice.
