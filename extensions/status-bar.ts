/**
 * status-bar — Severity-colored context gauge + turn counter
 *
 * Renders: T{turns} ████████░░░░░░░░ {pct}%
 *
 * Bar color = context fullness severity (a direct value proposition —
 * it affects both remaining window AND inference quality):
 *   green (<70%), yellow (70-90%), red (>90%)
 *
 * The built-in footer already renders token counts, cost, model, and
 * context %/window on line 2 — this extension adds only the visual
 * gauge and turn counter.
 *
 * Source: ctx.getContextUsage().percent
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  let turns = 0;

  /**
   * Build a context gauge colored by severity.
   *
   * Color reflects a single value proposition: context fullness directly
   * affects remaining window capacity AND current inference quality.
   *   green (<70%)  — plenty of room
   *   yellow (70-90%) — getting tight, consider compacting
   *   red (>90%)    — compact now, quality is degrading
   *
   * Token types (cache/input/output) are NOT color-coded here — they have
   * no inherent good/bad mapping, and the built-in footer already shows
   * their counts as ↑input ↓output R{cache} W{write}.
   *
   * Source: ctx.getContextUsage().percent
   */
  function buildContextBar(ctx: ExtensionContext, barWidth: number): string {
    const theme = ctx.ui.theme;
    const usage = ctx.getContextUsage();
    const pct = usage?.percent ?? 0;

    if (barWidth <= 0) return "";

    const filledBlocks = pct > 0 ? Math.max(1, Math.round((pct / 100) * barWidth)) : 0;
    const emptyBlocks = barWidth - filledBlocks;

    const color = pct > 90 ? "error" : pct > 70 ? "warning" : "success";

    let bar = "";
    if (filledBlocks > 0) bar += theme.fg(color, "█".repeat(filledBlocks));
    if (emptyBlocks > 0) bar += theme.fg("dim", "░".repeat(emptyBlocks));

    return bar;
  }

  function render(ctx: ExtensionContext) {
    if (!ctx.hasUI) return;

    try {
      const theme = ctx.ui.theme;
      const usage = ctx.getContextUsage();
      const pct = usage?.percent ?? 0;

      // T{turns} [████████░░░░] {pct}%
      const parts: string[] = [];

      parts.push(theme.fg("dim", `T${turns}`));

      const bar = buildContextBar(ctx, 16);
      if (bar) parts.push(bar);

      // Context % — colored by severity
      const pctStr = `${Math.round(pct)}%`;
      if (pct > 90) {
        parts.push(theme.fg("error", pctStr));
      } else if (pct > 70) {
        parts.push(theme.fg("warning", pctStr));
      } else {
        parts.push(theme.fg("dim", pctStr));
      }

      ctx.ui.setStatus("status-bar", parts.join(" "));
    } catch (err) {
      console.error("[status-bar] render error:", err);
    }
  }

  // — Events —
  // Re-render after state changes. Source: pi extension event lifecycle.

  pi.on("session_start", async (_event, ctx) => {
    turns = 0;
    render(ctx);
  });

  pi.on("turn_end", async (_event, ctx) => {
    turns++;
    render(ctx);
  });

  pi.on("message_end", async (_event, ctx) => {
    render(ctx);
  });

  pi.on("tool_execution_end", async (_event, ctx) => {
    render(ctx);
  });
}
