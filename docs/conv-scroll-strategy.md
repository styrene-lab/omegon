+++
id = "8b023f00-b00e-4998-9b36-fbd62ec37a54"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Conversation scroll strategy — tui-scrollview vs custom virtual scroll

## Overview

> Parent: [Conversation widget — structured rendering for all message types](conversation-widget.md)
> Spawned from: "Should we use tui-scrollview or build our own virtual scroll? tui-scrollview is maintained by the ratatui author but adds a dependency. Our own gives full control over segment-aware snapping."

*To be explored.*

## Decisions

### Decision: Build our own segment-aware scroll — tui-scrollview is too generic

**Status:** decided
**Rationale:** tui-scrollview renders into a full virtual buffer, which means rendering ALL segments even when only 20 are visible out of 500. We need a segment-aware scroll that: (1) only renders visible segments, (2) knows segment boundaries for snap-scrolling, (3) tracks per-segment state (collapsed/expanded), (4) handles streaming text in the last segment without re-rendering everything. Building our own gives us these properties and avoids the tui-scrollview dependency. The implementation is straightforward — maintain a list of segment heights, compute which segments are in the viewport, render only those at their computed y-offsets.

## Open Questions

*No open questions.*
