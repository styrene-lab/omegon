+++
id = "ea886989-7da9-4852-bba4-c930f999bc19"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Spinner Sermon — Crawler-style scrawling text during long operations

## Overview

During long-running operations (cleave children, extended tool calls), the spinner verb sits static for minutes or hours. Add a second layer beneath the verb: a slowly scrawling sermon inspired by the Crawler's writing in Annihilation — text that crawls character-by-character, giving visual proof-of-life.\n\nThe sermon text should feel alien and procedural, like biological processes masquerading as language. It appears only after a dwell threshold (e.g. 5s without a verb change) and disappears immediately when the next event arrives.

## Research

### Extension API capabilities

The pi extension API provides two rendering surfaces during tool execution:\n\n1. `ctx.ui.setWorkingMessage(msg)` — sets the text beside the braille spinner (Loader component, 80ms frame interval). Single line only.\n2. `ctx.ui.setWidget(key, factory, { placement })` — registers a custom TUI Component. The factory receives `(tui: TUI, theme: Theme)` and returns a `Component & { dispose?() }`. The component's `render(width): string[]` is called on each TUI render. The component can run its own `setInterval` and call `tui.requestRender()` to animate. Widget is removed when `setWidget(key, undefined)` is called or `dispose()` fires.\n\nWidget placement: `aboveEditor` (default) or `belowEditor`. Max lines enforced by `MAX_WIDGET_LINES` for string-array widgets, but Component factories are unconstrained.\n\nThe sermon should use the widget factory form with a custom Component that runs a character-reveal timer. The existing spinner verb continues via `setWorkingMessage` on turn_start/tool_call events — the sermon widget is additive.

### The Crawler's sermon — source material

From VanderMeer's Annihilation, the Crawler inscribes on the tower wall:\n\n\"Where lies the strangling fruit that came from the hand of the sinner I shall bring forth the seeds of the dead to share with the worms that gather in the darkness and surround the world with the power of their lives while from the dimlit halls of other places forms that never were and never could be writhe for the impatience of the few who never saw what could have been.\"\n\nKey properties of the original:\n- No punctuation — a single run-on sentence that never terminates\n- Recursive/self-referential structure\n- Biological imagery masquerading as liturgy\n- Written in bioluminescent fungal tissue — the medium IS the message\n- The act of reading it changes the reader (infection vector)\n- It extends infinitely — the biologist never finds the end\n\nFor our purposes we want text that:\n- Feels procedural and alien but thematically resonant with computation\n- Has no clear beginning or end (can start/stop at any point)\n- Mixes biological/organic imagery with technical concepts\n- Is long enough to never visibly loop during even hour-long operations\n- Uses no punctuation except possibly ellipsis at display boundaries

### Glitch effects — what the Shimmer does to text

The existing splash logo uses CRT noise glyphs (▓▒░█▄▀▌▐ etc.) for a convergence animation. The sermon can borrow this vocabulary but needs a different effect — not convergence (noise → clean) but *corruption* (clean → mutated → clean). This maps to the Shimmer's effect: things pass through it and come back changed.\n\nCandidate effects for the sermon scrawl:\n\n1. **Sporadic substitution glitch** — Every N characters, one letter briefly shows as a block/noise glyph before resolving. Low probability (~2-5%). Feels like the text is being written in an unstable medium.\n\n2. **Color shimmer** — Individual characters occasionally render in a different color (green/cyan tint) for 1-2 frames before returning to muted. Bioluminescent flicker.\n\n3. **Combining diacritics (Zalgo-lite)** — Occasionally append a combining unicode character (̷ ̸ ̶) to a letter, giving it a struck-through / corrupted appearance without breaking monospace alignment. Very sparingly — one every ~40 chars.\n\n4. **Brief phrase echo** — When a word completes, very rarely (~1%) the previous 3-4 chars re-render shifted/doubled for one frame, as if the text stuttered. Then resolves.\n\nThe splash logo's NOISE_CHARS set can be reused. The key constraint is that the sermon is rendered via `render(width): string[]` which returns ANSI-colored strings — we have full control over per-character coloring and Unicode content.

## Decisions

### Decision: Sermon is a single continuous text that wraps cyclically

**Status:** decided
**Rationale:** A single long passage (~2000+ chars) with no punctuation, inspired by the Crawler's style. When it reaches the end, it wraps back to the beginning seamlessly (the text is written to loop). This avoids visible seams during multi-hour operations. The entry point is randomized on each activation so repeated short operations don't always show the same opening.

### Decision: Variable scrawl speed with punctuation pauses

**Status:** decided
**Rationale:** Base rate ~30ms per character (~33 cps) with brief pauses at word boundaries (~80ms) and longer dwells at phrase boundaries marked by whitespace clusters (~200ms). This gives a biological, breathing rhythm rather than a mechanical ticker. The effect should feel like watching something being written by a hand that occasionally hesitates.

### Decision: 5-second dwell threshold before sermon appears

**Status:** decided
**Rationale:** The sermon widget only activates after 5 seconds without a setWorkingMessage change. This means fast tool sequences never see it — it only manifests during genuinely long waits. On the next event (turn_start, tool_call, turn_end), the sermon immediately disappears and resets.

### Decision: Three layered glitch effects: substitution, color shimmer, and combining diacritics

**Status:** decided
**Rationale:** Use all three lightweight effects at low probability. Substitution (~3% per char per render) replaces a character with a block glyph for 1-2 render cycles. Color shimmer (~5%) renders a char in the Alpharius accent color (teal/cyan) instead of muted. Combining diacritics (~1.5%) append a strikethrough combining char. All effects are transient — they resolve on the next render cycle (80ms Loader interval drives re-renders). The phrase-echo effect is too complex for the return and risks visual jank. Keep it simple: the text looks mostly stable but occasionally breathes and corrupts, like writing in living tissue that's still metabolizing.

## Open Questions

- What is the right scrawl speed — characters per second? Should it vary (faster bursts, pauses at punctuation)?
- Should the sermon text be a single continuous passage or drawn from fragments? Does it loop or terminate?
