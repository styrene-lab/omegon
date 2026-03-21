---
id: tui-animations-splash
title: TUI animations and splash screen — tachyonfx + Omegon branding
status: implemented
parent: tui-visual-system
tags: [tui, ratatui, animation, visual, branding]
open_questions: []
---

# TUI animations and splash screen — tachyonfx + Omegon branding

## Overview

Add smooth animations via tachyonfx and an Omegon splash screen to the TUI. The binary loads in ~50ms so the splash is a branding moment, not a loading screen — user should be able to disable via --no-splash or settings. Animations should enhance the sci-fi aesthetic without adding latency to the interaction loop.

## Research

### tachyonfx — ratatui's official effects library

**tachyonfx** (crates.io/crates/tachyonfx) is the standard animation library for ratatui applications. It provides shader-like composable effects:

**Available effects:**
- `fade_in` / `fade_out` — smooth opacity transitions
- `fade_to_fg` / `fade_to_bg` — color transitions
- `slide_in` / `slide_out` — directional slides (left/right/up/down)
- `dissolve` / `dissolve_to` — text dissolution/materialization
- `sweep_in` / `sweep_out` — directional sweeps
- `glitch` — random character glitch effect
- `ping` — brief highlight flash
- `coalesce` — scattered text coalesces into readable form
- `hsl_shift` — hue/saturation/lightness animation
- `translate` — position animation
- `resize` — size animation
- `parallel` / `sequence` — compose effects
- `repeat` / `with_duration` — timing control

**How it works:**
- Effects operate on ratatui `Buffer` cells (the rendered frame)
- You render your widget normally, then apply effects as a post-processing pass
- Effects have timers (duration in ms) and interpolation (linear, ease-in, bounce, etc.)
- The render loop calls `effect.process(duration_since_last_frame, buf, area)` each frame
- Zero-cost when no effects are active

**Use cases for Omegon:**
1. **Splash screen**: fade_in the Ω logo + text, then dissolve into the conversation view
2. **Tool card transitions**: slide_in from left when a tool call starts
3. **Message appearance**: fade_in new assistant text
4. **Footer card updates**: ping/flash when values change (fact count, context %)
5. **Thinking indicator**: hsl_shift cycling on the spinner verb text
6. **Interrupt feedback**: glitch effect on the cancelled message

**Performance:** Effects run in the render loop at ~60fps (16ms per frame). Each effect processes only the cells in its area. The event poll timeout is already 16ms, so the frame rate is set up for animation.

### Existing TS splash — glitch-convergence animation with 3-tier logo

The TS implementation at `extensions/00-splash/` is a complete system:

**Logo art:** Three tiers based on terminal size:
- Full: 31-row sigil + 7-row wordmark (needs 84+ cols, 46+ rows)
- Compact: 23-row smaller sigil + 4-row wordmark (needs 58+ cols, 34+ rows)
- Wordmark only: 7 rows (needs 84+ cols, 14+ rows)

**Animation:** CRT phosphor glitch-convergence:
- Each character has a randomized unlock frame weighted center-outward
- Before unlock: shows CRT noise glyphs (▓▒░█▄▀▌▐etc.)
- After unlock: final character in theme colors
- Total: ~38 frames at 45ms = ~1.7s to full resolution
- 6 hold frames after convergence
- Seeded RNG for deterministic-per-session noise

**Loading checklist:** Shows subsystem init status:
- secrets, providers, memory, mcp, tools
- States: pending (·), active (▸ scanning), done (✓), failed (✗)
- Animated scan indicator cycles through braille frames

**Post-splash:** Transitions to minimal branded header: `omegon v0.12.0  / commands  esc interrupt  ctrl+c clear/exit`

**Dismissal:** "press any key to continue" with blinking prompt. Any keypress dismisses once animation + loading complete.

**Rust port plan:**
- Port the logo art constants directly (raw string arrays)
- Port the frame rendering logic (noise → unlock → final)
- Use ratatui's immediate-mode rendering instead of ANSI escape codes
- Render splash as a full-screen overlay before entering the main TUI loop
- `--no-splash` CLI flag + settings toggle
- `/splash` command to replay

## Decisions

### Decision: Port splash animation directly, tachyonfx for interaction effects deferred

**Status:** decided
**Rationale:** The splash is the highest-impact branding moment — it's what the operator sees first. Port the glitch-convergence animation logic directly in Rust (logo art, noise RNG, unlock frames, frame rendering) without tachyonfx — it's simpler as a standalone full-screen render loop. tachyonfx for subtle interaction animations (fade-in on messages, ping on footer changes) is a follow-up — it requires threading effects through the existing draw() pipeline which is a deeper refactor. The splash is self-contained and can ship independently.

### Decision: Splash animation shipped as native ratatui — no tachyonfx dependency

**Status:** decided
**Rationale:** The glitch-convergence animation is self-contained (seeded RNG, per-character unlock frames, CRT noise glyphs). Implementing it directly in ratatui with Span-batched rendering was cleaner than pulling in tachyonfx for a single effect. tachyonfx remains the plan for interaction animations (fade-in, ping, hsl_shift) which operate on the post-render buffer.

### Decision: tachyonfx 0.25 integrated for interaction effects

**Status:** decided
**Rationale:** tachyonfx 0.25 (ratatui 0.30 compatible) integrated with sendable feature for tokio::spawn. EffectManager with named EffectSlot keys. Effects: startup sweep+fade (footer/conversation), spinner hsl_shift ping-pong during agent work, footer ping flash on memory changes. Post-render processing via effects.process(buf, area). All effects auto-expire — no manual cleanup.

## Open Questions

*No open questions.*
