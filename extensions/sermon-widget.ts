/**
 * sermon-widget.ts — TUI Component that scrawls the Crawler's sermon
 * character by character beneath the spinner verb.
 *
 * Renders a single line of dim text that slowly reveals itself, wrapping
 * cyclically through the sermon. Entry point is randomized.
 *
 * Glitch effects (transient, per-render):
 *   - Substitution (~3%): character replaced with block glyph
 *   - Color shimmer (~5%): character rendered in accent color
 *   - Combining diacritics (~1.5%): strikethrough/corruption mark
 *
 * Timing:
 *   - Base character interval: 67ms (~15 cps)
 *   - Word boundary pause: 120ms additional
 *   - The effect is biological — hesitant, breathing
 */

import type { TUI, Component } from "@styrene-lab/pi-tui";
import type { Theme } from "@styrene-lab/pi-coding-agent";
import { SERMON } from "./sermon.js";

const CHAR_INTERVAL_MS = 67;
const WORD_PAUSE_MS = 120;

/** Minimum visible characters (floor for very narrow terminals). */
const MIN_VISIBLE = 40;

// Glitch vocabulary — borrowed from the splash CRT noise aesthetic
const NOISE_CHARS = "▓▒░█▄▀▌▐▊▋▍▎▏◆■□▪◇┼╬╪╫";

// Combining diacritics that overlay without breaking monospace
const COMBINING_GLITCH = [
  "\u0336", // combining long stroke overlay  ̶
  "\u0337", // combining short solidus overlay ̷
  "\u0338", // combining long solidus overlay  ̸
  "\u0335", // combining short stroke overlay  ̵
];

// Sermon palette — much dimmer than the spinner verb.
// The sermon is background thought, not actionable signal.
// Base text is near the noise floor; glitch accents stay subdued.
// IMPORTANT: glitch colors must return to SERMON_DIM, never RESET (which
// snaps to terminal default — often bright white, causing flash).
const SERMON_DIM   = "\x1b[38;2;50;55;65m";     // #323741 — barely visible
const GLITCH_GLYPH = "\x1b[38;2;55;70;80m";     // #374650 — noise glyphs, only slightly above base
const GLITCH_COLOR = "\x1b[38;2;45;80;90m";      // #2d505a — very muted teal, close to base
const RESET_TO_DIM = SERMON_DIM;                  // return to base after glitch, never full reset
const RESET        = "\x1b[0m";                   // only for end-of-line

// Glitch probabilities per character per render — kept subtle
const P_SUBSTITUTE = 0.02;
const P_COLOR      = 0.035;
const P_COMBINING  = 0.01;

function randomFrom<T>(arr: readonly T[] | string): T | string {
  return arr[Math.floor(Math.random() * arr.length)];
}

function glitchChar(ch: string): string {
  // Don't glitch spaces
  if (ch === " ") return ch;

  const r = Math.random();

  // Substitution — replace with noise glyph, only slightly above base
  if (r < P_SUBSTITUTE) {
    return GLITCH_GLYPH + randomFrom(NOISE_CHARS) + RESET_TO_DIM;
  }

  // Color shimmer — very muted teal, not a flash
  if (r < P_SUBSTITUTE + P_COLOR) {
    return GLITCH_COLOR + ch + RESET_TO_DIM;
  }

  // Combining diacritics — corruption overlay at base dim
  if (r < P_SUBSTITUTE + P_COLOR + P_COMBINING) {
    return ch + randomFrom(COMBINING_GLITCH);
  }

  // Normal — rendered in the SERMON_DIM wrapper set by the caller
  return ch;
}

export function createSermonWidget(
  tui: TUI,
  theme: Theme,
): Component & { dispose(): void } {
  // Randomize entry point
  let cursor = Math.floor(Math.random() * SERMON.length);
  let revealed = "";
  let intervalId: ReturnType<typeof setTimeout> | null = null;

  function advance() {
    const ch = SERMON[cursor % SERMON.length];
    cursor = (cursor + 1) % SERMON.length;
    revealed += ch;

    // Sliding window — keep a generous buffer; render() trims to actual width
    if (revealed.length > 300) {
      revealed = revealed.slice(revealed.length - 300);
    }

    tui.requestRender();

    // Schedule next character with variable timing
    const nextCh = SERMON[cursor % SERMON.length];
    const delay = nextCh === " " ? CHAR_INTERVAL_MS + WORD_PAUSE_MS : CHAR_INTERVAL_MS;
    intervalId = setTimeout(advance, delay);
  }

  // Start the scrawl
  intervalId = setTimeout(advance, CHAR_INTERVAL_MS);

  return {
    render(width: number): string[] {
      // Use full terminal width minus a small indent (2 chars)
      const maxW = Math.max(MIN_VISIBLE, width - 4);
      const visible = revealed.length > maxW
        ? revealed.slice(revealed.length - maxW)
        : revealed;

      // Build the line character by character with glitch effects.
      // Base color is SERMON_DIM — near the noise floor. Glitch effects
      // momentarily escape to slightly brighter colors, then fall back.
      let line = "  " + SERMON_DIM;
      for (const ch of visible) {
        line += glitchChar(ch);
      }
      line += RESET;

      return [line];
    },
    invalidate() {
      // No cached state to invalidate
    },
    dispose() {
      if (intervalId) {
        clearTimeout(intervalId);
        intervalId = null;
      }
    },
  };
}
