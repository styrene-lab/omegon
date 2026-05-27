---
title: Terminal Background Session Visibility (#104)
status: exploring
tags: [0.25, terminal, host-actions, tui, reader]
---

# Terminal Background Session Visibility (#104)

## Problem

`terminal.create@1` can create an Omegon-owned `portable_pty` background session, but standalone Omegon TUI currently gives the operator no practical way to inspect, tail, attach to, or stop that session.

For `omegon-reader`, this means the HostAction succeeds but the operator cannot read the spawned reader output.

## Goal

Make `terminal.create@1` fallback sessions visible and actionable in standalone Omegon TUI.

## Desired first slice

When a HostAction returns:

```json
{
  "type": "terminal.create@1",
  "actual_placement": "background_session",
  "terminal_id": "..."
}
```

Omegon should expose at least one usable affordance:

- visible `terminal_id`
- transcript path or transcript tail
- exact command to inspect/tail/stop the session from the TUI
- explicit degraded placement warning (`side_pane -> background_session`)

## Better slice

Add a TUI terminal sessions panel/list, including:

- active sessions
- terminal IDs/titles
- latest transcript tail
- stop action
- attach/read affordance

## Decisions

### Decision: Standalone fallback remains required even with ACP/Flynt

ACP/Flynt terminal delegation (#87) is not a replacement for standalone visibility. If Omegon creates a background session, Omegon must make it inspectable.

### Decision: Be honest about placement degradation

A degraded `side_pane` request must not pretend a visual side pane opened. The result card/panel must show the actual placement.

## Open questions

- [assumption] Existing terminal session registry already has enough transcript/tail state to expose in the TUI.
- Is the first visible surface an expanded HostAction result card, a terminal panel, or both?
- Should `terminal_id` be clickable/selectable in Slim mode?
- How do we avoid flooding the conversation with live terminal output?

## Acceptance

- Running `omegon-reader reader_open execute=true` in standalone Omegon TUI yields a visible reader/terminal affordance.
- The operator can inspect output without leaving Omegon.
- The result clearly states actual placement and degradation warning.
- No Flynt dependency is required.

## Links

- [[0.25-roadmap-extension-surfaces]]
- [[acp-terminal-delegation-87]]
- [[extension-ui-contributions-101]]
