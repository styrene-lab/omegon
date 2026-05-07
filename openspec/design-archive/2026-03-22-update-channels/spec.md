+++
id = "72e93d8f-0c05-467e-aeb3-54322cf978bf"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Update channels and in-TUI self-update — stable/rc/nightly channels with /update command — Design Spec (extracted)

> Auto-extracted from docs/update-channels.md at decide-time.

## Decisions

### Nightly is cron-triggered daily, full release profile always (decided)

Nightly means nightly — once per day on a cron schedule, not on every push. All builds (stable, rc, nightly) use the full release profile (fat LTO, codegen-units=1). Binary size and optimization matter for distribution; CI time is the cost of quality.

### Update visual: fractal state surface transition during exec() restart (decided)

The auto-update restart is a moment of transformation — the binary is literally replacing itself. Instead of a plain text notification, use the fractal state surface (already implemented in tui/fractal.rs but never surfaced) as a brief transition animation. The fractal renders the current system state as a visual, morphs briefly during the download/verify/swap, then the exec() replaces the process and the new version's splash plays. The operator sees: fractal pulse → brief 'Updating to v0.14.2…' → new splash. Creative, branded, and communicates that something meaningful is happening.

## Research Summary

### Channel model

**Channels map to GitHub Release conventions:**

```
stable     → latest non-prerelease GitHub Release (e.g. v0.14.1)
rc         → latest prerelease GitHub Release (e.g. v0.14.1-rc.15)
nightly    → latest commit on main, built by a scheduled CI job (not yet implemented)
pinned     → specific version string from .omegon-version or config
```

**Channel persistence** lives in `~/.config/omegon/channel.json`:
```json
{"channel": "rc", "auto_update": true, "last_check": "2026-03-22T00:00:00Z"}
```

…

### /update slash command design

**Usage:**
```
/update              → check for updates on current channel, prompt to apply
/update check        → check only, don't apply
/update apply        → apply pending update (if one was found)
/update channel rc   → switch channel to rc
/update channel      → show current channel
/update auto on      → enable auto-update on startup
/update auto off     → disable auto-update
/update pin 0.14.1   → pin to a specific version
```

**In-TUI update flow:**
1. `/update` fetches releases for th…

### Fractal state surface — existing implementation

The fractal renderer exists at `tui/fractal.rs` (335 lines). It renders a Mandelbrot/Julia fractal using half-block characters with telemetry-driven parameters:

- Zoom depth → context utilization
- Color palette → cognitive mode (ocean/amber/violet)
- Animation speed → agent activity
- Fractal type → persona (Mandelbrot default, Julia per persona)
- Iteration depth → thinking level

Has `update_from_status()` which takes harness telemetry and maps it to visual parameters. Has `render()` which d…
