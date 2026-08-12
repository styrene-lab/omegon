---
state: proposed
---

# Full-display TUI navigation shell

## Intent

Replace constrained conversation overlays with an exclusive full-display interactive surface model. `/settings` becomes the canonical configuration hierarchy; task-oriented commands such as `/extensions`, `/skills`, `/model`, and `/auth` deep-link into settings destinations. Inspectors and blocking prompts use the same exclusive display ownership without being misclassified as settings. Semantic menu projections remain renderer-neutral; the TUI shell owns navigation history, input routing, viewport transitions, and return to conversation.

## Scope

- Introduce an exclusive full-display interactive-surface mode for the inline TUI.
- Make `/settings` the canonical configuration hierarchy and route task-oriented aliases to destination identities.
- Preserve independent renderer-neutral menu projection builders.
- Add hierarchical Back/Escape behavior and retain parent state across selectors and detail pages.
- Move process/detail and legacy extension modal rendering behind the exclusive display boundary.
- Add focused unit, render, snapshot, and viewport-restoration tests.

## Constraints

- Do not special-case Extensions in rendering.
- Do not place Ratatui geometry or key codes in semantic surface contracts.
- Do not flatten all configuration inventories into one eager settings projection.
- Preserve native terminal scrollback and do not republish completed exchanges during mode transitions.
- Resolve aliases and settings rows through canonical destination routing, not recursive slash execution.
