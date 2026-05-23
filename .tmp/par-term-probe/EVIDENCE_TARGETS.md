# par-term Probe Evidence Targets

This file tracks the next prototype pass. Keep it factual: commands, expected evidence, and current status.

## Hard validation

Command:

```bash
cd .tmp/par-term-probe
cargo test
cargo run -- --validate --assert
```

Current required checks:

- [x] PTY child output capture.
- [x] Sixel via `Terminal::process`.
- [x] Sixel RGBA payload present.
- [x] Sixel screenshot artifact renders.
- [x] Direct Kitty parser RGB decode.
- [x] Bookokrat EPUB text capture.

## Diagnostic targets

### Kitty terminal routing

Current status: unresolved after matrix run.

Known evidence:

- Direct `KittyParser` RGB decode passes.
- `--kitty-matrix` tested RGB/RGBA/PNG-style variants with `ESC \` and `0x9c` terminators.
- Every raw RGB/RGBA direct parser variant passed, but every `Terminal::process` route returned `graphics_count=0`.
- The PNG variant currently fails direct parser with a PNG CRC fixture problem.

Planned variants:

- [ ] `ESC \\` terminator.
- [ ] `0x9c` terminator.
- [ ] RGB direct, `f=24`.
- [ ] RGBA direct, `f=32`.
- [ ] PNG direct, `f=100`.
- [ ] with `t=d`.
- [ ] without `t=d`.
- [ ] with `q=2`.
- [ ] without `q=2`.

Acceptance:

- PASS if at least one variant produces `protocol=Kitty` and `pixels > 0` via `Terminal::process`.
- Current result: no terminal-routed variant passed. Next action is inspect APC prefilter/routing or build a minimal upstream-style unit around `filter_apc_and_advance`.

### iTerm parser and terminal routing

Current status: unresolved.

Known evidence:

- Current direct parser fixture fails with PNG CRC error.
- Current `Terminal::process` OSC 1337 fixture returns `graphics_count=0`.

Planned steps:

- [ ] Add `fixtures/tiny-rgba.png` as an actual file.
- [ ] Direct `ITermParser` reads and decodes that file.
- [ ] Generate OSC 1337 from the same file bytes.
- [ ] Feed generated OSC through `Terminal::process`.

Acceptance:

- Direct parser must pass before terminal-routed iTerm can become meaningful.

### Real Sixel fixture

Current status: synthetic fixture validated.

Planned steps:

- [ ] Add or generate `fixtures/tiny.sixel`.
- [ ] Compare fixture result against current inline synthetic Sixel.
- [ ] Keep Sixel in hard validation if stable.

Acceptance:

- `protocol=Sixel`, `pixels > 0`, screenshot artifact renders.

### Bookokrat PDF/image

Current status: untested.

Planned steps:

- [ ] Add image-heavy PDF fixture.
- [ ] Add direct image fixture if Bookokrat supports image files.
- [ ] Run `bookokrat --zen-mode` through `PtySession`.
- [ ] Report protocols captured by `Terminal::graphics_store()`.
- [ ] Generate screenshot artifact.

Acceptance:

- PASS if graphics are captured or if a clear negative identifies that Bookokrat emitted no graphics for the fixture.

### Screenshot/artifact bridge

Current status: Sixel-specific screenshot validated.

Planned steps:

- [ ] Add `--screenshot OUT.png` to applicable probe modes.
- [ ] Validate screenshot for `--sixel`.
- [ ] Validate screenshot for `--bookokrat`.

Acceptance:

- Output file exists, is nonzero, and `file OUT.png` identifies PNG.

## Evidence interpretation

- Direct parser pass + terminal route fail = routing/APC/OSC fixture issue, not decoder absence.
- Terminal route pass = protocol is viable through par-term `Terminal::process`.
- Bookokrat text pass + graphics absent = fixture or Bookokrat mode may not emit images.
- Sixel capture + screenshot pass = proven artifact bridge baseline.
