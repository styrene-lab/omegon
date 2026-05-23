# par-term-emu-core-rust Probe

Scratch spike for testing `par-term-emu-core-rust` as an embedded PTY/terminal-emulation substrate for Omegon side-process panes.

## Validate

```bash
cd .tmp/par-term-probe
cargo test
cargo run -- --validate --assert
```

`--validate` is the hard gate. It should stay green while diagnostic targets evolve.

Currently validated:

- PTY smoke capture;
- synthetic Sixel graphics capture through `Terminal::process`;
- Sixel screenshot artifact render;
- direct `KittyParser` RGB decode;
- Bookokrat EPUB text capture when the fixture exists.

## Targeted runs

Direct parser fixtures:

```bash
cargo run -- --parser-fixtures --assert
```

This validates direct protocol decoders separately from terminal OSC/APC routing. Current status:

- Kitty direct RGB decode: validated.
- iTerm PNG decode: diagnostic/unresolved.

PTY smoke:

```bash
cargo run -- --pty --assert
```

Synthetic Sixel graphic sequence:

```bash
cargo run -- --sixel --assert
```

Expected signal:

```text
status=PASS
graphics_count=1
graphic[0]: protocol=Sixel ... pixels=288 ...
check PASS: Sixel screenshot rendered — target/par-term-probe-sixel.png
```

Sixel screenshot artifact:

```text
target/par-term-probe-sixel.png
```

This proves the par-term `Terminal` model can capture at least Sixel graphics as actual RGBA pixel data in `TerminalGraphic`, not only half-block text.

Kitty diagnostic sequence:

```bash
cargo run -- --kitty
cargo run -- --kitty-matrix
```

Current observation: direct `KittyParser` RGB decode works, but the synthetic Kitty APC path through `Terminal::process` does not populate `graphics_store()`. This remains diagnostic and is not part of `--validate` yet.

Diagnostic iTerm2 inline image sequence:

```bash
cargo run -- --iterm
```

Current observation: the terminal-routed fixture does not populate `graphics_store()`. Direct parser mode currently reports a PNG decode CRC error, so fixture hygiene is the first iTerm target.

Bookokrat EPUB text smoke:

```bash
cargo run -- --bookokrat cockpit-test-assets/pride-and-prejudice.epub --assert
```

The probe intentionally kills long-running TUI children after five seconds. That is expected for Bookokrat; the validation checks that the captured terminal model contains document text before timeout.

## Current observations

- `par-term-emu-core-rust = 0.42.1` compiles on this macOS workspace with `default-features = false, features = ["rust-only"]`.
- `PtySession` can spawn `/bin/sh` and capture terminal output into a `Terminal` screen model.
- `PtySession` can spawn Bookokrat and capture an EPUB `--zen-mode` screen into the `Terminal` text model.
- Synthetic Sixel capture succeeds: `Terminal::graphics_count()` returns `1`, protocol `Sixel`, size `6x12`, with `288` RGBA bytes.
- Sixel screenshot rendering succeeds to `target/par-term-probe-sixel.png` when `--sixel` or `--validate` runs.
- Direct `KittyParser` RGB decode succeeds: protocol `Kitty`, size `2x1`, with `8` RGBA bytes.
- Terminal-routed Kitty APC capture remains unresolved: `cargo run -- --kitty-matrix` shows raw RGB/RGBA direct parser variants pass, while all corresponding `Terminal::process` routes return `graphics_count=0`. The remaining issue is likely APC prefilter/routing or a systematic sequence-shape mismatch, not the direct parser.
- Synthetic/direct iTerm2 inline PNG capture remains unresolved; current fixture hits a PNG decode CRC error in direct parser mode and `graphics_count=0` through `Terminal::process`.

## Next prototype pass targets

The next pass is evidence gathering, not architecture commitment. Keep the hard gate green and add diagnostic targets that explain where each path fails.

### Target A — Fixture layout

Create explicit fixtures instead of hand-copied base64 strings:

```text
.tmp/par-term-probe/
  fixtures/
    tiny-rgb.raw          # 2 pixels: red, green
    tiny-rgba.png         # known-good 1x1 PNG read from disk
    tiny.sixel            # known-good Sixel sequence or generated fixture
    kitty-rgb.seq         # canonical Kitty direct RGB APC
    kitty-png.seq         # canonical Kitty PNG APC
    iterm-png.seq         # canonical OSC 1337 inline PNG
  cockpit-test-assets/
    pride-and-prejudice.epub
    dummy.pdf
    image-page.pdf        # future image-heavy fixture
```

Acceptance:

- `--parser-fixtures --assert` reads fixture files where practical.
- Diagnostic output prints fixture path and byte length.
- Inline base64 remains only for tiny raw RGB if simpler than a file.

### Target B — Kitty routing variants

Direct `KittyParser` works; `Terminal::process` routing does not. Add variants to isolate why:

- terminator: `ESC \\` versus `0x9c`;
- raw RGB: `f=24`, `s=2`, `v=1`;
- raw RGBA: `f=32`, `s=1`, `v=1`;
- PNG: `f=100` from `fixtures/tiny-rgba.png`;
- with and without `t=d`;
- with and without `q=2`.

Acceptance:

- At least one terminal-routed Kitty variant populates `graphics_store()`, or the run prints a precise negative showing direct parser pass + terminal routing fail for each variant.

### Target C — iTerm fixture hygiene

Do not draw conclusions from the current PNG CRC failure. Replace embedded PNG strings with a real fixture file:

```text
fixtures/tiny-rgba.png
```

Acceptance:

- Direct `ITermParser` decode either passes with `protocol=ITermInline` and `pixels > 0`, or fails with a fixture-backed error.
- Only after direct parser passes should `Terminal::process` iTerm routing become a hard diagnostic target.

### Target D — Bookokrat PDF/image behavior

Add an image-heavy fixture and run it through the same PTY capture path.

Candidate files:

```text
cockpit-test-assets/dummy.pdf
cockpit-test-assets/image-page.pdf
cockpit-test-assets/tiny-image.png
```

Acceptance:

- `--bookokrat` reports graphics protocol counts, not just text.
- If no graphics are captured, output distinguishes "Bookokrat emitted no graphics for this fixture" from "par-term failed to parse graphics".

### Target E — Artifact bridge

Generalize the existing Sixel screenshot path into an optional output argument:

```bash
cargo run -- --sixel --screenshot target/sixel.png
cargo run -- --bookokrat cockpit-test-assets/pride-and-prejudice.epub --screenshot target/bookokrat.png
```

Acceptance:

- PNG exists, is nonzero, and `file` reports PNG.
- This becomes the first prototype for headless/degraded graphical artifact output.

## Evidence status legend

Use this language in commits/docs:

- `validated` — part of `cargo test` or `--validate --assert`.
- `diagnostic` — runnable target, not a hard gate.
- `unresolved` — tested but not understood enough for a conclusion.
- `rejected` — tested with enough evidence to stop pursuing.

## Do not conclude yet

Current evidence supports:

```text
par-term is viable for PTY text capture, Sixel graphics decode, Sixel screenshot artifacts, and direct Kitty RGB decode.
```

Current evidence does not yet support:

```text
Terminal-routed Kitty APC works.
Terminal-routed iTerm2 OSC 1337 works.
Bookokrat PDF/image emits capturable graphics.
Decoded graphics can be live-rendered inside Omegon.
```
