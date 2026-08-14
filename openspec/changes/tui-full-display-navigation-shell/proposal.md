---
state: proposed
---

# Full-display TUI navigation shell

> Historical branch proposal retained for provenance. Current terminal ownership
> follows `../tui-dual-presentation/`, rather than the earlier ownership model below.

## Intent

Replace constrained conversation overlays with an exclusive full-display interactive surface model. `/settings` becomes the canonical configuration hierarchy; task-oriented commands such as `/extensions`, `/skills`, `/model`, and `/auth` deep-link into settings destinations. Inspectors and blocking prompts use the same exclusive display ownership without being misclassified as settings. Semantic menu projections remain renderer-neutral; the TUI shell owns navigation history, input routing, viewport transitions, and return to conversation.

## Scope

- Introduce an exclusive full-display interactive-surface mode for the inline TUI.
- Make `/settings` the canonical configuration hierarchy and route task-oriented aliases to destination identities.
- Preserve independent renderer-neutral menu projection builders.
- Add hierarchical Back/Escape behavior, one keyboard/paste/mouse dispatcher, and explicit return targets across selectors, prompts, and detail pages.
- Move process/detail and legacy extension modal rendering behind the exclusive display boundary with no-clobber admission.
- Add focused unit, render, snapshot, and viewport-restoration tests.

## Constraints

- Do not special-case Extensions in rendering.
- Do not place Ratatui geometry or key codes in semantic surface contracts.
- Do not flatten all configuration inventories into one eager settings projection.
- Preserve native terminal scrollback by retaining one terminal-height inline viewport and switching composition—not terminal/viewport mode—between compact conversation and full-frame interactive surfaces.
- Resolve aliases and settings rows through canonical destination routing, not recursive slash execution.
- Do not advertise enabled Settings actions without a registered runtime editor or destination.
- Queue passive notifications and extension compatibility payloads so they cannot clobber an active exclusive surface.
