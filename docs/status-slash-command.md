+++
id = "d0a9f1a1-76d4-4d82-a64a-f2d5adf4a6ff"
kind = "document"
title = "/status slash command — re-display bootstrap panel mid-session"
status = "implemented"
tags = ["ux", "tui", "commands", "bootstrap", "status"]
aliases = ["status-slash-command"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "task"
open_questions = []
parent = "harness-status-contract"
priority = "4"
+++

# /status slash command — re-display bootstrap panel mid-session

## Overview

Operators see the bootstrap panel once at startup and never again. A /status command that re-renders the current HarnessStatus as a conversation-inline panel would let operators check MCP health, verify persona switches, and inspect inference backend status mid-session without restarting.

## Decisions

### Decision: Reuse render_bootstrap with color=false for /status output

**Status:** decided
**Rationale:** The bootstrap renderer already handles all HarnessStatus sections. SlashResult::Display goes through ratatui text rendering (not raw terminal output), so ANSI codes must be off. No new renderer needed — same function, same data, different render path.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/mod.rs` (modified) — Added 'status' to COMMANDS table + match arm in handle_slash_command — calls render_bootstrap(color=false)
- `core/crates/omegon/src/tui/bootstrap.rs` (modified) — Added status_command_rerender_no_color test — verifies mid-session re-render with live data, no ANSI

### Constraints

- render_bootstrap called with color=false because SlashResult::Display goes through ratatui text rendering, not raw terminal
