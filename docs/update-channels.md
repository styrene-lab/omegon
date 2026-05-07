+++
id = "17e9680e-21f2-4d3c-a418-bf5025b8a81c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Update channels and in-TUI self-update — stable/rc/nightly channels with /update command

## Overview

The version switcher exists as `omegon switch` but requires exiting the TUI. The operator wants:

1. `/update` slash command — check for and apply updates without leaving the session
2. Update channels — stable, rc, nightly, pinned version
3. Auto-update — check on startup, optionally apply automatically
4. Channel-aware SBOM — each channel has its own verification chain

This builds on the existing switcher infrastructure (download from GitHub Releases, checksum verification, symlink activation) but wraps it in a TUI-native experience with channel semantics.

## Research

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

**Channel selection hierarchy** (highest priority wins):
1. `.omegon-version` file in project root (pin overrides everything)
2. `~/.config/omegon/channel.json` (operator preference)
3. Default: `stable`

**Nightly channel** requires a new CI workflow: scheduled daily build from main, tagged `nightly-YYYY-MM-DD`, uploaded as prerelease. The switcher already handles prerelease artifacts. The new piece is the cron workflow.

**SBOM per channel**: each release already gets an SBOM (cargo-cyclonedx) and Sigstore signature. Channel awareness means the update checker verifies the SBOM matches the channel's expected signing identity. Stable releases require the release workflow identity; nightly requires the nightly workflow identity.

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
1. `/update` fetches releases for the current channel (reuses switch.rs GitHub client)
2. Compares latest available version against `build_version()` (the compiled-in version)
3. If newer: shows version diff, changelog summary, asks to apply
4. Apply: downloads + verifies (reuses switch.rs install+activate), then shows "Restart omegon to use v0.14.2"
5. The binary can't replace itself while running — the symlink swap works but the running process keeps the old binary. Restart is required.

**Auto-update on startup:**
1. Before entering the TUI, check `channel.json` for `auto_update: true`
2. If last_check was >1 hour ago, query GitHub Releases
3. If newer version available: download, install, swap symlink, then exec() the new binary (replacing the current process — seamless restart)
4. If no update: proceed normally, update `last_check`

**The exec() trick**: on Unix, `std::os::unix::process::Command::exec()` replaces the current process with a new one. The updated binary at the symlink target IS the new binary. So: download → install → activate symlink → exec(current_exe) → new binary starts. The user sees a brief restart, then the new version's splash screen. No manual restart needed.

### Fractal state surface — existing implementation

The fractal renderer exists at `tui/fractal.rs` (335 lines). It renders a Mandelbrot/Julia fractal using half-block characters with telemetry-driven parameters:

- Zoom depth → context utilization
- Color palette → cognitive mode (ocean/amber/violet)
- Animation speed → agent activity
- Fractal type → persona (Mandelbrot default, Julia per persona)
- Iteration depth → thinking level

Has `update_from_status()` which takes harness telemetry and maps it to visual parameters. Has `render()` which draws to a ratatui Buffer using half-block `▀` characters for 2x vertical resolution.

**Never surfaced in the TUI** — the widget exists but no code path renders it. It was designed for the dashboard sidebar but the dashboard uses text-based status instead.

For the update transition: render the fractal at current state, then animate a zoom-in or palette shift during the download/verify/swap phases, then exec(). The fractal becomes a visual "warp drive" moment — the system is transforming.

## Decisions

### Decision: Nightly is cron-triggered daily, full release profile always

**Status:** decided
**Rationale:** Nightly means nightly — once per day on a cron schedule, not on every push. All builds (stable, rc, nightly) use the full release profile (fat LTO, codegen-units=1). Binary size and optimization matter for distribution; CI time is the cost of quality.

### Decision: Update visual: fractal state surface transition during exec() restart

**Status:** decided
**Rationale:** The auto-update restart is a moment of transformation — the binary is literally replacing itself. Instead of a plain text notification, use the fractal state surface (already implemented in tui/fractal.rs but never surfaced) as a brief transition animation. The fractal renders the current system state as a visual, morphs briefly during the download/verify/swap, then the exec() replaces the process and the new version's splash plays. The operator sees: fractal pulse → brief 'Updating to v0.14.2…' → new splash. Creative, branded, and communicates that something meaningful is happening.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/mod.rs` (modified) — Add /update slash command handler — dispatch to update module
- `core/crates/omegon/src/update.rs` (new) — Update module — channel config, version check, download+apply, auto-update startup flow, exec() restart
- `core/crates/omegon/src/tui/update_screen.rs` (new) — Update transition screen — fractal animation during download/verify/swap phases
- `core/crates/omegon/src/main.rs` (modified) — Wire auto-update check before TUI startup, add Update slash command to TuiCommand enum
- `.github/workflows/nightly.yml` (new) — Nightly build workflow — cron-triggered, full release profile, nightly-YYYY-MM-DD tag
- `core/crates/omegon/src/switch.rs` (modified) — Extract shared download/verify/install logic into reusable functions for both switch and update

### Constraints

- All builds use full release profile (fat LTO, codegen-units=1)
- Nightly workflow is cron-triggered daily, not on every push
- Channel config persisted in ~/.config/omegon/channel.json
- .omegon-version overrides channel config
- Auto-update uses exec() to replace the running process — seamless restart
- Update transition shows fractal animation (tui/fractal.rs already implemented)
- SBOM and Sigstore signatures verified per channel
- Nightly tags use nightly-YYYY-MM-DD format
- exec() only on Unix — Windows falls back to 'restart to apply' message
