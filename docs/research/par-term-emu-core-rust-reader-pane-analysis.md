---
title: par-term-emu-core-rust Reader Pane Analysis
status: seed
tags: [research, terminal, par-term, cockpit, reader, graphics]
---

# par-term-emu-core-rust Reader Pane Analysis

## Purpose

Recover and continue the deeper research thread below Cockpit: whether `par-term-emu-core-rust` changes the feasibility of embedded reader panes and broader Omegon TUI pane substrates.

## Recovered context

Current persisted evidence proves Cockpit can host text/full-screen TUI children at smoke-test level, including Bookokrat EPUB text mode. The unresolved problem is graphical fidelity for Bookokrat image/PDF rendering and other terminal graphics workflows.

Recent design memory records the conclusion:

> `par-term-emu-core-rust` is more relevant than Cockpit for graphical embedded terminal work.

Treat that as a recovered pointer, not a fully re-verified fact. This document should be updated with direct upstream links, license, API notes, and command output.

## Why par-term matters

Cockpit answers pane management questions: PTYs, focus switching, pane widgets, child process lifecycle.

par-term may answer terminal emulation questions: how escape sequences, alternate screen state, mouse mode, colors, and graphics protocols are parsed and represented.

For Bookokrat, this distinction is critical. EPUB text rendering can succeed while PDF/image rendering still fails if the rendering path only supports text cells or half-block approximations.

## Actual imagery versus half-block fallback

```text
Actual imagery:
  pixel/raster image rendered by terminal graphics protocol or GUI renderer

Half-block fallback:
  terminal text cells approximating pixels with colored Unicode glyphs
```

Half-block fallback may be acceptable for thumbnails or degraded previews. It is not equivalent to native image rendering for a document reader if the requirement is faithful PDF/image display.

## Research questions

1. What exact repository/crate provides `par-term-emu-core-rust`?
2. What license applies?
3. Is it maintained?
4. Is it directly used by Cockpit, optionally used, or only conceptually related?
5. Does it parse or preserve:
   - Kitty graphics protocol;
   - Sixel;
   - iTerm2 inline images;
   - OSC 8 hyperlinks;
   - bracketed paste;
   - mouse reporting;
   - alternate screen;
   - truecolor?
6. Does it expose raster/image payloads to embedders, drop them, or degrade them to terminal cells?
7. Can Omegon render those payloads through Ratatui/crossterm, or would it need a different terminal graphics layer?
8. Does Ghostty/Kitty outer-terminal support survive through this embedded path?
9. Is par-term useful only for child PTY panes, or also for Omegon-owned pane surfaces?

## Decision pressure

If par-term cannot preserve or expose actual graphics payloads, embedded Bookokrat PDF/image support should not block on it. Use Zellij/external workspace for v1 and keep embedded mode text-first or experimental.

If par-term can expose actual image protocol payloads cleanly, it may justify a deeper embedded-pane branch for Bookokrat and future interactive tools.

## Local scratch probe evidence — 2026-05-23

A scratch Cargo probe was recovered from `.tmp/par-term-probe` before cleanup and its useful findings are now captured here.

Probe dependency shape:

```toml
par-term-emu-core-rust = { version = "0.42.1", default-features = false, features = ["rust-only"] }
anyhow = "1"
```

Minimal API map from the probe:

- `PtySession::new(cols, rows, scrollback)` creates a PTY-backed terminal session.
- `PtySession::set_env("TERM", "xterm-kitty")` sets the child terminal environment.
- `PtySession::spawn(&cmd, &arg_refs)` launches an argv-based child process.
- `PtySession::terminal()` returns the captured terminal model behind a lock.
- `Terminal::new(cols, rows)` can be driven directly with escape-sequence bytes via `Terminal::process(bytes)`.
- `Terminal::export_text()` exposes the current text-cell screen.
- `Terminal::graphics_count()`, `scrollback_graphics_count()`, `dropped_sixel_graphics()`, and `all_graphics()` expose captured graphics metadata and pixel payloads.

Validated command shapes from the probe:

```bash
cd .tmp/par-term-probe
cargo check
cargo run --
cargo run -- --sixel
cargo run -- --sequence
cargo run -- bookokrat --zen-mode /Users/wilson/bravo/omegon/.tmp/cockpit-test-assets/pride-and-prejudice.epub
```

Observed results:

- `par-term-emu-core-rust = 0.42.1` compiled on the macOS workspace with `default-features = false` and `features = ["rust-only"]`.
- `PtySession` spawned `/bin/sh` and captured terminal output into a `Terminal` screen model.
- `PtySession` spawned Bookokrat and captured an EPUB `--zen-mode` screen into the `Terminal` text model.
- Synthetic Sixel capture succeeded: `Terminal::graphics_count()` returned `1`, protocol `Sixel`, size `6x12`, and `288` RGBA bytes. This proves par-term can expose at least one valid Sixel sequence as actual pixel data in `TerminalGraphic`, not merely as half-block text.
- Synthetic iTerm2 inline PNG capture did **not** succeed in the current probe: `Terminal::graphics_count()` remained `0`. This does not prove iTerm2 support is absent; the sequence may have been malformed, blocked by parser expectations, or routed through a different parser path.
- The probe killed long-running TUI children after 5 seconds. That was intentional for non-interactive capture and is not evidence about interactive viability.

This changes the par-term posture from purely speculative to partially evidenced: text PTY capture and Sixel pixel-payload extraction work in a minimal local harness. Kitty graphics, real Bookokrat PDF/image output, mouse reporting, and production rendering through Omegon/Ratatui remain unproven.

## Scratch artifact disposition

The recovered `.tmp/par-term-probe` source, lockfile, README, and 929 MB `target/` directory were temporary spike artifacts. Their useful architecture evidence is preserved in this design node; the scratch directory can be removed from the repository.

## Next evidence to collect

- Upstream repository URL.
- License text.
- Maintenance status.
- Real Kitty graphics protocol fixture.
- Real Sixel fixture from a tool rather than a synthetic byte string.
- Bookokrat PDF/image test inside the par-term path.
- Whether valid graphics payloads can be rendered back through Omegon's Ratatui/crossterm frame or require a different terminal graphics layer.
