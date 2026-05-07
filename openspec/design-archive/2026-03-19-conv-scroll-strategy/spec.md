+++
id = "fc2c3478-4f21-40d8-82aa-80731533d499"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation scroll strategy — tui-scrollview vs custom virtual scroll — Design Spec (extracted)

> Auto-extracted from docs/conv-scroll-strategy.md at decide-time.

## Decisions

### Build our own segment-aware scroll — tui-scrollview is too generic (decided)

tui-scrollview renders into a full virtual buffer, which means rendering ALL segments even when only 20 are visible out of 500. We need a segment-aware scroll that: (1) only renders visible segments, (2) knows segment boundaries for snap-scrolling, (3) tracks per-segment state (collapsed/expanded), (4) handles streaming text in the last segment without re-rendering everything. Building our own gives us these properties and avoids the tui-scrollview dependency. The implementation is straightforward — maintain a list of segment heights, compute which segments are in the viewport, render only those at their computed y-offsets.
