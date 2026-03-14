---
id: alpharius-theme
title: Alpharius Theme — Alpha Legion visual identity
status: implemented
tags: [theme, design, alpha-legion, warhammer]
open_questions: []
---

# Alpharius Theme — Alpha Legion visual identity

## Overview

Rename the internal "Verdant" design system to "Alpharius" and replace the teal-green organic palette with the Alpha Legion color scheme from Warhammer 40k: deep void-black backgrounds, iridescent blue-green ceramite armor tones, silver metallic highlights, and blood-red/brass signal colors. Affects: themes/default.json → themes/alpharius.json, skills/style/SKILL.md, extensions/style.ts, extensions/render/index.ts, docs/render.md, README.md, CHANGELOG.md.

## Research

### Terminal bleed-through analysis — why colors suppress on light-background terminals

**Root cause:** pi-tui appends `SEGMENT_RESET = "\x1b[0m"` (full attribute reset) to every rendered line. After this reset, any unpainted area — whitespace, padding, line endings, margins — shows the terminal emulator's native background and foreground. On terminals with a light background this means suppressed/desaturated colours visible in gaps between rendered spans.

**What pi themes control:** Every `colors` entry in alpharius.json resolves to a hex `#RRGGBB` value. When pi calls `theme.fg("text", str)` it emits `\x1b[38;2;R;G;Bm${str}\x1b[39m` in truecolor mode. The explicit 24-bit values are correct — the issue is the reset *between* spans.

**What we cannot change:** The `\x1b[0m` line-reset is hardcoded in pi-tui's `applyLineResets()`. It cannot be overridden from a theme file. Theme color entries cannot be `""` (bare terminal default) on any key — but alpharius.json already has zero `""` entries.

**What we can change:**
1. The Kitty `.conf` (already done) sets `background #0d1420` and `foreground #c4d8e4` — this makes the terminal's native defaults match Alpharius, so the `\x1b[0m` reset falls through to the *right* colors in Kitty.
2. **OSC 10/11 sequences** — `\x1b]10;#c4d8e4\x07` and `\x1b]11;#0d1420\x07` set the terminal's foreground and background color programmatically for the duration of the session, without requiring any `.conf` file. Supported by iTerm2, WezTerm, foot, alacritty, most VTE terminals, and modern xterm. NOT supported by Kitty (Kitty ignores OSC 10/11 in favor of its own theme system).
3. **`\x1b[?25l` / DECSCNM** and similar — not relevant here.

**The portable solution:** Emit OSC 10/11 at session_start from an extension, clamping the terminal's background and foreground to Alpharius values for terminals that aren't Kitty. On session_end/shutdown emit OSC 110/111 (reset to saved value) to restore the user's original colors. This is the same mechanism that tools like `bat`, `vivid`, and terminal color scheme managers use.

## Decisions

### Decision: Emit OSC 10/11 in defaults.ts to anchor terminal fg/bg to Alpharius values

**Status:** decided
**Rationale:** pi-tui appends \x1b[0m (full reset) after every line — hardcoded in applyLineResets(). On non-Kitty terminals with lighter backgrounds this makes resets bleed through. Fix: emit OSC 10 (fg) and OSC 11 (bg) at session_start with Alpharius palette values so the terminal's native defaults match our palette. Restore via OSC 110/111 on session_shutdown. Kitty silently ignores OSC 10/11 (covered by alpharius.conf). Lives in defaults.ts alongside existing theme enforcement logic.

## Open Questions

*No open questions.*
