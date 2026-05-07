+++
id = "d8697e83-90e3-4d95-b77d-bfbda9da211d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Spinner Sermon — Crawler-style scrawling text during long operations — Design Spec (extracted)

> Auto-extracted from docs/spinner-sermon.md at decide-time.

## Decisions

### Sermon is a single continuous text that wraps cyclically (decided)

A single long passage (~2000+ chars) with no punctuation, inspired by the Crawler's style. When it reaches the end, it wraps back to the beginning seamlessly (the text is written to loop). This avoids visible seams during multi-hour operations. The entry point is randomized on each activation so repeated short operations don't always show the same opening.

### Variable scrawl speed with punctuation pauses (decided)

Base rate ~30ms per character (~33 cps) with brief pauses at word boundaries (~80ms) and longer dwells at phrase boundaries marked by whitespace clusters (~200ms). This gives a biological, breathing rhythm rather than a mechanical ticker. The effect should feel like watching something being written by a hand that occasionally hesitates.

### 5-second dwell threshold before sermon appears (decided)

The sermon widget only activates after 5 seconds without a setWorkingMessage change. This means fast tool sequences never see it — it only manifests during genuinely long waits. On the next event (turn_start, tool_call, turn_end), the sermon immediately disappears and resets.

### Three layered glitch effects: substitution, color shimmer, and combining diacritics (decided)

Use all three lightweight effects at low probability. Substitution (~3% per char per render) replaces a character with a block glyph for 1-2 render cycles. Color shimmer (~5%) renders a char in the Alpharius accent color (teal/cyan) instead of muted. Combining diacritics (~1.5%) append a strikethrough combining char. All effects are transient — they resolve on the next render cycle (80ms Loader interval drives re-renders). The phrase-echo effect is too complex for the return and risks visual jank. Keep it simple: the text looks mostly stable but occasionally breathes and corrupts, like writing in living tissue that's still metabolizing.

## Research Summary

### Extension API capabilities

The pi extension API provides two rendering surfaces during tool execution:\n\n1. `ctx.ui.setWorkingMessage(msg)` — sets the text beside the braille spinner (Loader component, 80ms frame interval). Single line only.\n2. `ctx.ui.setWidget(key, factory, { placement })` — registers a custom TUI Component. The factory receives `(tui: TUI, theme: Theme)` and returns a `Component & { dispose?() }`. The component's `render(width): string[]` is called on each TUI render. The component can run its own `s…

### The Crawler's sermon — source material

From VanderMeer's Annihilation, the Crawler inscribes on the tower wall:\n\n\"Where lies the strangling fruit that came from the hand of the sinner I shall bring forth the seeds of the dead to share with the worms that gather in the darkness and surround the world with the power of their lives while from the dimlit halls of other places forms that never were and never could be writhe for the impatience of the few who never saw what could have been.\"\n\nKey properties of the original:\n- No punc…

### Glitch effects — what the Shimmer does to text

The existing splash logo uses CRT noise glyphs (▓▒░█▄▀▌▐ etc.) for a convergence animation. The sermon can borrow this vocabulary but needs a different effect — not convergence (noise → clean) but *corruption* (clean → mutated → clean). This maps to the Shimmer's effect: things pass through it and come back changed.\n\nCandidate effects for the sermon scrawl:\n\n1. **Sporadic substitution glitch** — Every N characters, one letter briefly shows as a block/noise glyph before resolving. Low probabi…
