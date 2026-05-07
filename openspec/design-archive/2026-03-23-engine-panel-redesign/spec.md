+++
id = "0df422cf-64b5-4634-bea3-db2d32f51708"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Engine panel redesign — visual hierarchy and information density — Design Spec (extracted)

> Auto-extracted from docs/engine-panel-redesign.md at decide-time.

## Decisions

### Grouped layout with visual hierarchy — model headline, context gauge, tier+thinking, counters at bottom (decided)

The current 6-line text dump has no visual hierarchy — every value has the same weight. The redesign groups by operator importance: model identity (brightest, top), context gauge (visual bar, middle), tuning parameters (tier+thinking), session counters (dimmest, bottom). Context gets a ▰▱ gauge bar because it's the most dynamically changing value and deserves spatial treatment. Provider+auth merge onto one line. Context class right-aligns with model name.

## Research Summary

### Current values inventory and operator meaning

The engine panel currently shows 6 lines of text. Here's what each value IS and what it MEANS to the operator:

### Layout proposal — grouped sections with visual hierarchy


