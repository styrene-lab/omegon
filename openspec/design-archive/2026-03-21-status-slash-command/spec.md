+++
id = "fc89537e-11be-4158-a92d-83368910e6a0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# /status slash command — re-display bootstrap panel mid-session — Design Spec (extracted)

> Auto-extracted from docs/status-slash-command.md at decide-time.

## Decisions

### Reuse render_bootstrap with color=false for /status output (decided)

The bootstrap renderer already handles all HarnessStatus sections. SlashResult::Display goes through ratatui text rendering (not raw terminal output), so ANSI codes must be off. No new renderer needed — same function, same data, different render path.
