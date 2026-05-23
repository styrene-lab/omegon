+++
id = "par-term-embedded-graphics-v1-v2-plan"
kind = "document"
title = "par-term Embedded Graphics V1/V2 Plan"
status = "exploring"
tags = ["tui", "par-term", "sixel", "kitty", "graphics", "reader", "substrate"]
aliases = ["par-term-graphics-v1-v2", "embedded-graphics-v1-v2"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "extension-side-process-substrate-api"
dependencies = [
  "side-process-backend-terminal-compatibility-matrix",
  "side-process-backend-macos-linux-compatibility",
  "cockpit-par-term-substrate-analysis",
  "par-term-emu-core-rust-reader-pane-analysis",
  "reader-extension-side-pane-contract"
]
open_questions = [
  "Can Bookokrat PDF/image mode emit Sixel or otherwise be coerced into a Sixel-compatible path for v1?",
  "Should v1 render Sixel graphics live in the TUI, or only export screenshot/artifact PNGs?",
  "What is the minimum usable artifact bridge from TerminalGraphic RGBA to Omegon display artifacts?",
  "Does the Sixel path validate on Linux as well as macOS?",
  "For v2, is Kitty APC routing fixable inside par-term Terminal::process, or should Omegon call KittyParser directly?"
]
related = [
  "extension-side-process-substrate-api",
  "reader-extension-side-pane-contract",
  "side-process-backend-terminal-compatibility-matrix",
  "par-term-emu-core-rust-reader-pane-analysis",
  "par-term-sixel-v1-acceptance"
]
+++

# par-term Embedded Graphics V1/V2 Plan

## Overview

Define an incremental graphics plan for the embedded par-term backend:

```text
V1: Sixel-first embedded graphics/artifact path
V2: Kitty graphics expansion after APC routing is understood
```

This avoids overfitting v1 to unresolved Kitty terminal-routing behavior while still preserving Kitty as the strategic modern graphics target.

## Evidence baseline

Current `.tmp/par-term-probe` evidence:

```text
Validated:
  PTY child capture
  Bookokrat EPUB text capture
  Sixel via Terminal::process
  Sixel RGBA TerminalGraphic payload
  Sixel screenshot PNG artifact
  Direct KittyParser RGB/RGBA decode

Unresolved:
  Kitty via Terminal::process APC routing
  iTerm2 direct/Terminal::process PNG fixture
  Bookokrat PDF/image graphics behavior
  Linux validation
```

The key implication is:

```text
Sixel is the only graphics protocol currently proven end-to-end through par-term Terminal::process.
Kitty decoding is promising, but terminal stream routing is not v1-ready.
```

## V1 — Sixel-first embedded graphics

### Goal

Ship or prototype an embedded backend path that treats Sixel as the first supported real graphics protocol.

V1 is not "all terminal graphics." It is:

```text
child PTY emits Sixel
  → par-term Terminal::process decodes Sixel
  → TerminalGraphic RGBA is available
  → Omegon can render/export an artifact or degraded preview
```

### V1 supported capabilities

The backend may advertise:

```text
host_process: true
text_tui: true
embedded_pane: true
resize_propagation: promising / under validation
graphics_decode_sixel: true
graphics_decode_kitty: false
graphics_decode_iterm: false
graphics_render_artifact: true once screenshot bridge lands
graphics_render_live: false until explicitly proven
```

### V1 non-goals

- Do not claim Kitty graphics support through `Terminal::process`.
- Do not claim iTerm2 inline image support.
- Do not claim Bookokrat PDF/image works until an image-heavy fixture proves it.
- Do not block EPUB/text mode on graphics.
- Do not require Kitty as a global Omegon prerequisite.

### V1 product behavior

For EPUB/text:

```text
Use par-term PTY text capture.
Graphics are optional.
```

For PDF/image:

```text
If Sixel graphics are emitted and captured:
  produce artifact screenshot / preview.
Else:
  report missing graphics capability and offer fallback modes.
```

Potential operator messages:

```text
Opened reader in embedded text mode.
Graphics: not required for EPUB text.
```

```text
PDF image rendering requires a graphics path.
This backend currently supports Sixel capture only.
No Sixel graphics were observed for this document.
Available fallbacks: text extraction, artifact export, Kitty backend, external viewer.
```

### V1 implementation targets

1. Keep `.tmp/par-term-probe` validation green:

```bash
cargo test
cargo run -- --validate --assert
```

2. Add a real Sixel fixture file:

```text
fixtures/tiny.sixel
```

3. Generalize screenshot/artifact output:

```bash
cargo run -- --sixel --screenshot target/sixel.png
cargo run -- --bookokrat cockpit-test-assets/pride-and-prejudice.epub --screenshot target/bookokrat.png
```

4. Add Bookokrat PDF/image fixture testing:

```text
cockpit-test-assets/image-page.pdf
```

5. Classify Bookokrat PDF/image outcome:

```text
captured_sixel
no_graphics_emitted
unsupported_document
process_failed
```

6. Validate on Linux.

### V1 acceptance criteria

V1 can be considered feasible when:

- PTY text capture passes on macOS and Linux.
- Sixel fixture capture passes on macOS and Linux.
- Screenshot/artifact output is generated from captured terminal state.
- Reader EPUB text remains usable.
- PDF/image behavior is honestly classified, even if unsupported unless Sixel is observed.

V1 should not require Bookokrat PDF/image success. It should require a correct capability result.

## V2 — Kitty graphics expansion

### Goal

Add Kitty graphics support after resolving how Kitty APC sequences should flow into par-term.

Current evidence says:

```text
Direct KittyParser works.
Terminal::process APC routing fails for tested variants.
```

So V2 starts with routing, not decoding.

### V2 supported capabilities target

```text
graphics_decode_kitty: true
graphics_decode_sixel: true
graphics_render_artifact: true
graphics_render_live: possible / separate decision
```

### V2 research branches

#### Branch A — Fix or adapt `Terminal::process` APC routing

Investigate par-term internals:

- `terminal/apc_filter.rs`
- `Terminal::filter_apc_and_advance`
- `KittyParser::parse_chunk`
- `KittyParser::build_graphic`

Acceptance:

```text
cargo run -- --kitty-matrix
```

has at least one terminal-routed variant pass:

```text
protocol=Kitty
pixels > 0
```

#### Branch B — Direct parser integration

If `Terminal::process` routing remains blocked, Omegon/par-term backend may intercept APC sequences before feeding non-APC bytes into the terminal and call `KittyParser` directly.

This is more invasive but may be appropriate if the direct parser is stable.

Acceptance:

- raw child stream can be split into:
  - terminal text/control bytes;
  - Kitty APC image payloads.
- image payloads decode to `TerminalGraphic`.
- non-image bytes still update terminal state correctly.

#### Branch C — External Kitty backend instead

If embedded Kitty decode is too invasive, keep Kitty graphics as an external terminal-native backend:

```text
Bookokrat → Kitty side pane/window
```

This keeps V1 embedded Sixel/artifact path separate from V2 terminal-native Kitty path.

### V2 non-goals

- Do not make Kitty a global Omegon requirement.
- Do not silently degrade Kitty graphics to text.
- Do not couple extension APIs to Kitty-specific commands.

### V2 acceptance criteria

Kitty expansion is ready when one of these is true:

1. par-term `Terminal::process` captures Kitty graphics from a canonical fixture; or
2. a direct-parser interception layer captures Kitty graphics from a PTY stream without corrupting terminal text; or
3. a backend-registry Kitty external backend opens graphics-capable side panes reliably.

Until then, Kitty remains diagnostic/experimental.

## Extension capability mapping

V1 backend advertises:

```text
reader_text: supported
graphics_sixel_artifact: supported
graphics_kitty: unsupported
graphics_iterm: unsupported
```

V2 backend may advertise:

```text
reader_text: supported
graphics_sixel_artifact: supported
graphics_kitty_artifact: supported
graphics_kitty_live: optional
```

The extension request should specify requirements by mode:

```text
EPUB/text:
  requires: host_process, text_tui
  prefers: adjacent_pane, resize_propagation

PDF/image v1:
  requires: artifact_output OR graphics_decode_sixel
  prefers: graphics_render_live

PDF/image v2:
  requires: graphics_decode_sixel OR graphics_decode_kitty OR external_kitty_backend OR artifact_output
```

## Documentation updates required

When V1 lands, update:

- `par-term-emu-core-rust-reader-pane-analysis`
- `side-process-backend-terminal-compatibility-matrix`
- `side-process-backend-macos-linux-compatibility`
- `reader-extension-side-pane-contract`

Key wording:

```text
Sixel is validated as the v1 embedded graphics protocol.
Kitty is validated only at the direct parser layer and remains a v2 expansion target.
```
