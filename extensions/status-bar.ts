/**
 * status-bar — Severity-colored context gauge with memory segment + turn counter
 *
 * Renders: T{turns} ▓▓████████░░░░░░ {pct}%
 *                    ^^-- memory (accent)
 *                      ^^^^^^^^-- conversation (severity-colored)
 *                              ^^^^^^^^-- free (dim)
 *
 * The memory segment shows how much of the context window is occupied
 * by injected project memory (facts, episodes, global knowledge).
 * Data comes from shared-state.ts, written by the project-memory extension.
 *
 * Bar colors:
 *   accent       — memory injection (facts/episodes/global knowledge)
 *   muted/yellow/red — conversation context (leading indicator for compaction)
 *                      <45% muted, 45-70% warning, >70% error
 *                      Leads pressure gradient (55-75%) and auto-compact (85%)
 *   dim          — free space
 *
 * Source: ctx.getContextUsage().percent, sharedState.memoryTokenEstimate
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { sharedState } from "./shared-state.js";

export default function (pi: ExtensionAPI) {
  let turns = 0;

  /**
   * Build a context gauge with a memory segment.
   *
   * Three segments:
   *   1. Memory (accent) — estimated tokens from project memory injection
   *   2. Conversation (severity-colored) — remaining used context
   *   3. Free (dim) — unused context window
   */
  function buildContextBar(ctx: ExtensionContext, barWidth: number): string {
    const theme = ctx.ui.theme;
    const usage = ctx.getContextUsage();
    const pct = usage?.percent ?? 0;
    const contextWindow = usage?.contextWindow ?? 0;

    if (barWidth <= 0) return "";

    // Calculate memory's share of the context window
    const memTokens = sharedState.memoryTokenEstimate;
    const memPct = contextWindow > 0 ? (memTokens / contextWindow) * 100 : 0;

    // Conversation = total used - memory
    const convPct = Math.max(0, pct - memPct);

    // Convert percentages to block counts
    const memBlocks = memPct > 0 ? Math.max(1, Math.round((memPct / 100) * barWidth)) : 0;
    const convBlocks = convPct > 0 ? Math.max(1, Math.round((convPct / 100) * barWidth)) : 0;
    const totalFilled = Math.min(memBlocks + convBlocks, barWidth);
    const freeBlocks = barWidth - totalFilled;

    // Severity color for conversation portion (based on TOTAL fullness)
    // Thresholds LEAD the compaction system so the user sees what's coming:
    //   <45%  muted   — well ahead of pressure onset (55%)
    //   45-70% warning — pressure gradient approaching or active (55-75%)
    //   >70%  error   — auto-compaction at 85% is on the horizon
    const convColor = pct > 70 ? "error" : pct > 45 ? "warning" : "muted";

    let bar = "";
    if (memBlocks > 0) bar += theme.fg("accent", "▓".repeat(memBlocks));
    if (convBlocks > 0) bar += theme.fg(convColor, "█".repeat(convBlocks));
    if (freeBlocks > 0) bar += theme.fg("dim", "░".repeat(freeBlocks));

    return bar;
  }

  function render(ctx: ExtensionContext) {
    if (!ctx.hasUI) return;

    try {
      const theme = ctx.ui.theme;
      const usage = ctx.getContextUsage();
      const pct = usage?.percent ?? 0;

      const parts: string[] = [];

      parts.push(theme.fg("dim", `T${turns}`));

      const bar = buildContextBar(ctx, 24);
      if (bar) parts.push(bar);

      // Context % — colored by severity (matches bar thresholds)
      const pctStr = `${Math.round(pct)}%`;
      if (pct > 70) {
        parts.push(theme.fg("error", pctStr));
      } else if (pct > 45) {
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
