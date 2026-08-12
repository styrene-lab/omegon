---
state: implementing
---

# TUI presentation settings and capability remediation

## Intent

Expand the canonical UI settings destination into capability-aware appearance, theme, glyph/Nerd Font, startup splash, conversation, layout, and accessibility controls with live preview and profile persistence.

## Scope

- Expose existing UI presentation level, tool density, and startup splash policy in canonical Settings.
- Add persisted named theme and requested glyph preferences.
- Resolve glyph request separately from detected and effective terminal capability.
- Add a bundled semantic theme registry with safe fallback.
- Add Nerd Font detection evidence and reviewable remediation actions.
- Apply presentation choices live and persist them through profiles.

## Constraints

- Renderer-neutral settings contracts must not depend on Ratatui.
- Startup probes remain independent from splash visibility.
- Font remediation must not mutate host font or terminal configuration automatically.
- Existing profiles retain current defaults.
