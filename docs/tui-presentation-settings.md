---
id: tui-presentation-settings
title: "TUI presentation settings and capability remediation"
status: seed
parent: tui-full-display-navigation-shell
tags: [tui, settings, theme, glyphs, nerd-font, splash, accessibility]
open_questions: []
dependencies: []
related: []
---

# TUI presentation settings and capability remediation

## Research

### Existing implementation seams

- `SettingsSurfaceProjection` currently exposes only tool density in its UI tab, while runtime settings already persist `ui_presentation` and `startup_splash`.
- `tui::theme` already defines semantic color slots and a JSON loader, but theme discovery is hardwired to Alpharius.
- `tui::glyphs` already provides ASCII, Unicode, and Nerd Font matrices plus detection evidence and a help URL; it lacks persisted operator preference and requested/effective resolution.

## Decisions

### Presentation preferences are canonical settings, not TUI-local state

**Status:** accepted

**Rationale:** Theme, glyph intent, splash, presentation level, and density must survive sessions and project/user profile application. The TUI applies them live, but renderer-neutral settings and profile persistence own the values.

### Glyph request, detection, and effective profile remain distinct

**Status:** accepted

**Rationale:** Auto follows detection, while explicit choices express operator intent. A Nerd Font request on an unverified terminal remains requested and produces remediation rather than silently rewriting the preference.

### Font remediation is diagnostic and reviewable

**Status:** accepted

**Rationale:** Installing a font does not configure a terminal to use it. Omegon shows evidence, a specimen/help path, and external documentation but does not modify host fonts or terminal configuration automatically.

## Implementation Notes

See `openspec/changes/tui-presentation-settings/design.md` and `tasks.md` for the first delivery slice.
