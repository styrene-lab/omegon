# par-term-emu-core-rust Probe

Scratch spike for testing `par-term-emu-core-rust` as an embedded PTY/terminal-emulation substrate for Omegon side-process panes.

## Build

```bash
cd .tmp/par-term-probe
cargo check
```

## Runs


Synthetic Sixel graphic sequence:

```bash
cd .tmp/par-term-probe
DEBUG_LEVEL=3 cargo run -- --sixel
```

Observed result: `graphics_count=1`, protocol `Sixel`, with RGBA pixel payload present. This proves the par-term `Terminal` model can capture at least Sixel graphics as actual pixel data in `TerminalGraphic`, not only half-block text.

Synthetic iTerm2 inline image sequence:

```bash
cd .tmp/par-term-probe
cargo run -- --sequence
```

PTY smoke:

```bash
cd .tmp/par-term-probe
cargo run --
```

Bookokrat EPUB text smoke:

```bash
cd .tmp/par-term-probe
cargo run -- bookokrat --zen-mode /Users/wilson/bravo/omegon/.tmp/cockpit-test-assets/pride-and-prejudice.epub
```

## Current observations

- `par-term-emu-core-rust = 0.42.1` compiles on this macOS workspace with `default-features = false, features = ["rust-only"]`.
- `PtySession` can spawn `/bin/sh` and capture terminal output into a `Terminal` screen model.
- `PtySession` can spawn Bookokrat and capture an EPUB `--zen-mode` screen into the `Terminal` text model.
- The current probe times out and kills long-running TUI children after 5 seconds; that is intentional for non-interactive capture.
- Synthetic Sixel capture succeeded: `Terminal::graphics_count()` returned `1`, protocol `Sixel`, size `6x12`, with `288` RGBA bytes. This proves actual pixel payload capture is possible through par-term for valid Sixel sequences.
- `Terminal::graphics_count()` remained `0` for the synthetic iTerm2 inline PNG sequence in the current probe. That means this first synthetic iTerm2 sequence did not prove graphics capture. It may be malformed, blocked by parser expectations, or require a different protocol path.

## Next spike steps

- Add real Kitty graphics protocol fixture.
- Add real Sixel fixture.
- Add a PDF/image Bookokrat fixture.
- Inspect par-term parser expectations for OSC 1337 and APC Kitty image chunks.
- Determine whether graphics are stored in `Terminal::graphics_store()` for valid sequences or only in specific rendering paths.
