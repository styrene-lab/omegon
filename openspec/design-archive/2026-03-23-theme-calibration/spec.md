+++
id = "7185070d-cc87-4791-8285-17b6ed97d334"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Theme calibration — /calibrate command, gamma/sat/hue slider, tweakcn-style theme export — Design Spec (extracted)

> Auto-extracted from docs/theme-calibration.md at decide-time.

## Decisions

### Calibration as HSL transform layer over alpharius.json, not a theme replacement (decided)

Alpharius is the brand theme — we don't want operators building arbitrary themes. What we want is display adaptation: if your screen is dim, bump lightness; if colors look washed, boost saturation; if teal doesn't render well on your terminal, shift hue. This is a 3-parameter transform (gamma/lightness, saturation multiplier, hue shift) applied uniformly to all theme colors. The base theme stays alpharius — calibration adjusts the output. Persisted to settings as `{ gamma: 1.0, saturation: 1.0, hue_shift: 0 }`. Applied at theme load time in the Rust theme module.

### /calibrate as a TUI overlay with live preview and arrow-key sliders (decided)

Calibration is visual — the operator needs to SEE the result as they adjust. A command-line `/calibrate gamma 1.2` forces blind experimentation. Instead: `/calibrate` opens a TUI overlay showing color swatches + the current footer/engine/dashboard as preview. Arrow keys adjust the selected parameter. The overlay shows before/after. Enter saves, Esc cancels. Similar to the selector widget we already have. Three sliders: Lightness (gamma curve), Saturation (multiplier), Hue (degree shift).

### tweakcn integration deferred — publish alpharius as tweakcn theme later, not blocking 0.15.0 (decided)

Publishing alpharius on tweakcn is a nice distribution channel but adds web tooling complexity (CSS variable export, registry API). The core need — display calibration via /calibrate — doesn't require tweakcn at all. Ship /calibrate with HSL transforms in 0.15.0, add tweakcn export/import in a follow-up. The formats are compatible — our alpharius.json hex values can round-trip to/from HSL.

## Research Summary

### tweakcn architecture and applicability

tweakcn is a visual editor for shadcn/ui themes. Key features:
- Registry API at `https://tweakcn.com/api/registry/theme/<name>` — JSON themes fetchable by name
- Exports CSS custom properties in HSL or OKLCH format
- Supports Tailwind v3/v4
- Has HSL adjustment controls (hue shift, saturation, lightness)

**Applicability to Omegon:**
- We can create an `alpharius` theme on tweakcn as a distribution/sharing mechanism
- The HSL adjustment model maps well to our calibration needs — operator adjust…
