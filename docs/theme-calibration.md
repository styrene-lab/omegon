---
id: theme-calibration
title: "Theme calibration — /calibrate command, gamma/sat/hue slider, tweakcn-style theme export"
status: implemented
parent: alpharius-theme
tags: [theme, ux, calibration, display, tweakcn, 0.15.0]
open_questions: []
jj_change_id: zwpyutkzknmqptxxutqpkzvwuotkuywu
issue_type: feature
priority: 3
---

# Theme calibration — /calibrate command, gamma/sat/hue slider, tweakcn-style theme export

## Overview

Alpharius is a strong opinionated theme but doesn't account for display variation (dim laptop screens, ultra-wide monitors, terminal emulator differences). Add a /calibrate slash command that lets operators adjust gamma, saturation, and hue shift — persisted to settings. Look at shadcn's tweakcn (https://tweakcn.com) for the import/export model: lists of CSS color values that can be shared. Create an alpharius/omegon theme set on tweakcn as a distribution channel. The Styrene Python TUI already did this pattern. The calibration UI could be a TUI overlay with live preview — operator sees changes immediately as they adjust sliders.

## Research

### tweakcn architecture and applicability

tweakcn is a visual editor for shadcn/ui themes. Key features:
- Registry API at `https://tweakcn.com/api/registry/theme/<name>` — JSON themes fetchable by name
- Exports CSS custom properties in HSL or OKLCH format
- Supports Tailwind v3/v4
- Has HSL adjustment controls (hue shift, saturation, lightness)

**Applicability to Omegon:**
- We can create an `alpharius` theme on tweakcn as a distribution/sharing mechanism
- The HSL adjustment model maps well to our calibration needs — operator adjusts lightness (gamma), saturation, and hue
- But our theme format is `alpharius.json` with hex RGB values, not CSS variables
- We need a round-trip: alpharius.json ↔ tweakcn format ↔ calibrated alpharius.json
- The calibration itself is pure color math — apply HSL transforms to the base theme's RGB values and write a `calibrated-alpharius.json` or settings override

**What the Styrene Python TUI did:**
- Had a rich/textual based TUI with a theme system
- Exposed theme customization via a config file
- Colors were adjustable per-component

## Decisions

### Decision: Calibration as HSL transform layer over alpharius.json, not a theme replacement

**Status:** decided
**Rationale:** Alpharius is the brand theme — we don't want operators building arbitrary themes. What we want is display adaptation: if your screen is dim, bump lightness; if colors look washed, boost saturation; if teal doesn't render well on your terminal, shift hue. This is a 3-parameter transform (gamma/lightness, saturation multiplier, hue shift) applied uniformly to all theme colors. The base theme stays alpharius — calibration adjusts the output. Persisted to settings as `{ gamma: 1.0, saturation: 1.0, hue_shift: 0 }`. Applied at theme load time in the Rust theme module.

### Decision: /calibrate as a TUI overlay with live preview and arrow-key sliders

**Status:** decided
**Rationale:** Calibration is visual — the operator needs to SEE the result as they adjust. A command-line `/calibrate gamma 1.2` forces blind experimentation. Instead: `/calibrate` opens a TUI overlay showing color swatches + the current footer/engine/dashboard as preview. Arrow keys adjust the selected parameter. The overlay shows before/after. Enter saves, Esc cancels. Similar to the selector widget we already have. Three sliders: Lightness (gamma curve), Saturation (multiplier), Hue (degree shift).

### Decision: tweakcn integration deferred — publish alpharius as tweakcn theme later, not blocking 0.15.0

**Status:** decided
**Rationale:** Publishing alpharius on tweakcn is a nice distribution channel but adds web tooling complexity (CSS variable export, registry API). The core need — display calibration via /calibrate — doesn't require tweakcn at all. Ship /calibrate with HSL transforms in 0.15.0, add tweakcn export/import in a follow-up. The formats are compatible — our alpharius.json hex values can round-trip to/from HSL.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/theme.rs` (modified) — HSL ↔ RGB conversion, CalibrationParams struct, apply_calibration() that transforms all theme colors
- `core/crates/omegon/src/settings.rs` (modified) — Add calibration fields to Settings (gamma, saturation, hue_shift), load/save
- `core/crates/omegon/src/tui/mod.rs` (modified) — /calibrate slash command, calibration overlay key handling
- `core/crates/omegon/src/tui/calibration.rs` (new) — New: CalibrationOverlay widget with sliders and live preview

### Constraints

- HSL transforms must preserve the Alpharius color relationships — don't break the perceptual ramp
- Live preview must not cause flickering — rebuild theme on each slider change
- Settings must persist across sessions
- Default calibration is identity (1.0, 1.0, 0) — no change from base theme
