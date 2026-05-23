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

## Next evidence to collect

- Upstream repository URL and crate versions.
- Minimal API map.
- License text.
- Local prototype output showing how image protocol sequences are handled.
- Bookokrat PDF/image test inside Cockpit/par-term path.
