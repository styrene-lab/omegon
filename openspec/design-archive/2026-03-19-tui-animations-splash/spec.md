+++
id = "251a6383-d2f2-44ae-8c24-1a057e94f640"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# TUI animations and splash screen — tachyonfx + Omegon branding — Design Spec (extracted)

> Auto-extracted from docs/tui-animations-splash.md at decide-time.

## Decisions

### Port splash animation directly, tachyonfx for interaction effects deferred (decided)

The splash is the highest-impact branding moment — it's what the operator sees first. Port the glitch-convergence animation logic directly in Rust (logo art, noise RNG, unlock frames, frame rendering) without tachyonfx — it's simpler as a standalone full-screen render loop. tachyonfx for subtle interaction animations (fade-in on messages, ping on footer changes) is a follow-up — it requires threading effects through the existing draw() pipeline which is a deeper refactor. The splash is self-contained and can ship independently.

### Splash animation shipped as native ratatui — no tachyonfx dependency (decided)

The glitch-convergence animation is self-contained (seeded RNG, per-character unlock frames, CRT noise glyphs). Implementing it directly in ratatui with Span-batched rendering was cleaner than pulling in tachyonfx for a single effect. tachyonfx remains the plan for interaction animations (fade-in, ping, hsl_shift) which operate on the post-render buffer.

### tachyonfx 0.25 integrated for interaction effects (decided)

tachyonfx 0.25 (ratatui 0.30 compatible) integrated with sendable feature for tokio::spawn. EffectManager with named EffectSlot keys. Effects: startup sweep+fade (footer/conversation), spinner hsl_shift ping-pong during agent work, footer ping flash on memory changes. Post-render processing via effects.process(buf, area). All effects auto-expire — no manual cleanup.

## Research Summary

### tachyonfx — ratatui's official effects library

**tachyonfx** (crates.io/crates/tachyonfx) is the standard animation library for ratatui applications. It provides shader-like composable effects:

**Available effects:**
- `fade_in` / `fade_out` — smooth opacity transitions
- `fade_to_fg` / `fade_to_bg` — color transitions
- `slide_in` / `slide_out` — directional slides (left/right/up/down)
- `dissolve` / `dissolve_to` — text dissolution/materialization
- `sweep_in` / `sweep_out` — directional sweeps
- `glitch` — random character glitch effect
…

### Existing TS splash — glitch-convergence animation with 3-tier logo

The TS implementation at `extensions/00-splash/` is a complete system:

**Logo art:** Three tiers based on terminal size:
- Full: 31-row sigil + 7-row wordmark (needs 84+ cols, 46+ rows)
- Compact: 23-row smaller sigil + 4-row wordmark (needs 58+ cols, 34+ rows)
- Wordmark only: 7 rows (needs 84+ cols, 14+ rows)

**Animation:** CRT phosphor glitch-convergence:
- Each character has a randomized unlock frame weighted center-outward
- Before unlock: shows CRT noise glyphs (▓▒░█▄▀▌▐etc.)
- After unl…
