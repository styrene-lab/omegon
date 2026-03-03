/**
 * status-bar — Session usage tracking (complements Pi's built-in status)
 *
 * Renders: $0.43 (sub) 0.2%/200k (auto)
 *
 * Only shows session cost + context % — Pi's built-in row already renders
 * the model name, full context gauge, and token counters.
 *
 * Also provides /usage command that opens the Claude usage dashboard.
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";

interface SessionUsage {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  cost: number;
}

export default function (pi: ExtensionAPI) {
  const session: SessionUsage = {
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    cost: 0,
  };

  function formatTokens(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toString();
  }

  function formatCost(n: number): string {
    if (n >= 100) return `$${Math.round(n)}`;
    if (n >= 10) return `$${n.toFixed(1)}`;
    return `$${n.toFixed(3)}`;
  }

  function render(ctx: ExtensionContext) {
    if (!ctx.hasUI) return;

    try {
      const theme = ctx.ui.theme;
      const dim = (s: string) => theme.fg("dim", s);
      const parts: string[] = [];

      // Session cost
      parts.push(theme.fg("accent", formatCost(session.cost)) + " " + dim("(sub)"));

      // Context % (compact — Pi's built-in has the full gauge)
      const usage = ctx.getContextUsage();
      const pct = usage?.percent ?? 0;
      const win = usage?.contextWindow || ctx.model?.contextWindow || 0;
      parts.push(`${Math.round(pct)}%/${formatTokens(win)}`);

      // Compaction mode hint
      const autoCompact = true; // pi default
      parts.push(dim(autoCompact ? "(auto)" : "(manual)"));

      ctx.ui.setStatus("status-bar", parts.join(" "));
    } catch (err) {
      console.error("[status-bar] render error:", err);
    }
  }

  // — Events —

  pi.on("session_start", async (_event, ctx) => {
    session.input = 0;
    session.output = 0;
    session.cacheRead = 0;
    session.cacheWrite = 0;
    session.cost = 0;
    render(ctx);
  });

  // Accumulate usage from assistant messages
  pi.on("message_end", async (event: any, ctx) => {
    const msg = event?.message;
    if (msg?.role === "assistant" && msg?.usage) {
      const u = msg.usage;
      session.input += u.input || 0;
      session.output += u.output || 0;
      session.cacheRead += u.cacheRead || 0;
      session.cacheWrite += u.cacheWrite || 0;
      if (u.cost?.total) {
        session.cost += u.cost.total;
      }
    }
    render(ctx);
  });

  pi.on("tool_execution_end", async (_event, ctx) => {
    render(ctx);
  });

  // — /usage command — opens Claude usage dashboard
  pi.registerCommand("usage", {
    description: "Open Claude usage dashboard in browser",
    handler: async (_args, ctx) => {
      await pi.exec("open", ["https://claude.ai/settings/usage"]);
      ctx.ui.notify("Opened claude.ai/settings/usage", "info");
    },
  });
}
