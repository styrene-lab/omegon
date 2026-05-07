+++
id = "7558f2de-5115-4081-8c7a-74764dc87c6f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon: auspex extension — visualization daemon bridge

## Overview

The Omegon side of the integration. A small extension in this repo (extensions/auspex/) that provides: `/auspex open` — spawns the mdserve binary pointed at the project root, opens the browser to /dashboard; `/auspex stop` — kills the daemon; optionally a footer/widget showing when the daemon is running and the local URL. Checks for the binary on PATH, surfaces a helpful error if not found (points to Nix install instructions). This is the only piece that lives in Omegon rather than the mdserve fork repo.

## Decisions

### Decision: Auspex bridge is adjacent to both browser and terminal rendering tracks

**Status:** decided

**Rationale:** The auspex extension is a bridge, not the owner of either rendering surface. It is related to the browser/project-intelligence portal (`markdown-viewport`) because it launches and talks to that surface, and related to terminal rendering (`conversation-rendering-engine`) because it may also participate in display handoff and shared lifecycle state. Keeping it as a cross-cutting bridge avoids falsely nesting it under one rendering concern.

## Open Questions

*No open questions.*
