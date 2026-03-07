/**
 * dashboard — Unified live dashboard for Design Tree + OpenSpec + Cleave
 *
 * Renders a custom footer via setFooter() that supports two modes:
 *   Layer 0 (compact): Dashboard summary + context gauge + original footer data
 *   Layer 1 (raised):  Section details for design tree, openspec, cleave + footer data
 *
 * Layer 2 (interactive overlay) opened via /dashboard open or Ctrl+Shift+B from raised.
 *
 * Reads sharedState written by producer extensions (design-tree, openspec, cleave).
 * Subscribes to "dashboard:update" events for live re-rendering.
 *
 * Absorbs status-bar.ts — the context gauge (turn counter + memory bar + %)
 * is rendered directly in the compact footer line.
 *
 * Toggle: Ctrl+Shift+B or /dashboard command.
 * Persistence: raised/lowered state saved via appendEntry("dashboard-state").
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import { DashboardFooter } from "./footer.ts";
import { showDashboardOverlay } from "./overlay.ts";
import type { DashboardState, DashboardMode } from "./types.ts";
import { debug } from "../debug.ts";

export default function (pi: ExtensionAPI) {
  const state: DashboardState = {
    mode: "compact",
    turns: 0,
  };

  let footer: DashboardFooter | null = null;
  let tui: any = null; // TUI reference for requestRender
  let unsubscribeEvents: (() => void) | null = null;

  /**
   * Restore persisted dashboard mode from session entries.
   */
  function restoreMode(ctx: ExtensionContext): void {
    try {
      const entries = ctx.sessionManager.getEntries();
      for (let i = entries.length - 1; i >= 0; i--) {
        const entry = entries[i] as any;
        if (entry.type === "dashboard-state" && entry.data?.mode) {
          state.mode = entry.data.mode as DashboardMode;
          return;
        }
      }
    } catch { /* first session, no entries yet */ }
  }

  /**
   * Persist the current mode to the session.
   */
  function persistMode(_ctx: ExtensionContext): void {
    try {
      pi.appendEntry("dashboard-state", { mode: state.mode });
    } catch { /* session may not support it */ }
  }

  /**
   * Toggle between compact and raised modes.
   */
  function toggle(ctx: ExtensionContext): void {
    state.mode = state.mode === "compact" ? "raised" : "compact";
    persistMode(ctx);
    tui?.requestRender();
  }

  /**
   * Update footer context and trigger re-render.
   */
  function refresh(ctx: ExtensionContext): void {
    if (footer) {
      footer.setContext(ctx);
    }
    tui?.requestRender();
  }

  /**
   * Open overlay with error handling.
   */
  function openOverlay(ctx: ExtensionContext): void {
    if (!ctx.isIdle()) {
      ctx.ui.notify("Dashboard overlay unavailable while agent is streaming", "warning");
      return;
    }
    showDashboardOverlay(ctx, pi).catch((err) => {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`[dashboard] overlay error: ${msg}`);
    });
  }

  // ── Session start: set up the custom footer ──────────────────

  pi.on("session_start", async (_event, ctx) => {
    if (!ctx.hasUI) return;

    debug("dashboard", "session_start", { hasUI: ctx.hasUI, cwd: ctx.cwd });

    state.turns = 0;
    restoreMode(ctx);

    // Set the custom footer
    ctx.ui.setFooter((tuiRef, theme, footerData) => {
      tui = tuiRef;
      footer = new DashboardFooter(tuiRef, theme, footerData, state);
      footer.setContext(ctx);
      debug("dashboard", "footer:factory", { tuiSet: !!tui });
      return footer;
    });

    // Subscribe to dashboard:update events from producer extensions.
    // Don't setContext here — ctx from session_start would overwrite
    // the fresher ctx set by turn_end/message_end. The footer reads
    // sharedState directly at render time; ctx is only needed for
    // token stats which the per-turn handlers keep current.
    unsubscribeEvents = pi.events.on(DASHBOARD_UPDATE_EVENT, (_data) => {
      debug("dashboard", "update-event", _data as Record<string, unknown>);
      tui?.requestRender();
    });

    // Deferred initial render — design-tree emits synchronously during
    // its session_start handler (which fires before ours per extension
    // load order), so sharedState.designTree is already populated.
    // We just need to trigger a render after setFooter has installed.
    // Use queueMicrotask to run after the current event loop tick
    // completes (all sync session_start work is done) but before any
    // setTimeout-based async work.
    queueMicrotask(() => {
      debug("dashboard", "microtask:render", { tuiSet: !!tui });
      tui?.requestRender();
    });

    // Non-blocking guardrail health check
    setTimeout(async () => {
      try {
        const { discoverGuardrails, runGuardrails } = await import("../cleave/guardrails.ts");
        const checks = discoverGuardrails(ctx.cwd);
        if (checks.length === 0) return;
        const suite = runGuardrails(ctx.cwd, checks);
        if (!suite.allPassed) {
          const failures = suite.results.filter((r: { passed: boolean }) => !r.passed);
          const msg = failures
            .map((f: { check: { name: string }; exitCode: number; output: string }) =>
              `${f.check.name}: ${f.exitCode !== 0 ? f.output.split("\n").length + " errors" : "failed"}`,
            )
            .join(", ");
          ctx.ui.notify(`⚠ Guardrail check failed: ${msg}`, "warning");
        }
      } catch {
        /* non-fatal */
      }
    }, 2000);
  });

  // ── Session shutdown: cleanup ─────────────────────────────────

  pi.on("session_shutdown", async () => {
    if (unsubscribeEvents) {
      unsubscribeEvents();
      unsubscribeEvents = null;
    }
    footer = null;
    tui = null;
  });

  // ── Events that trigger re-render ─────────────────────────────

  pi.on("turn_end", async (_event, ctx) => {
    state.turns++;
    refresh(ctx);
  });

  pi.on("message_end", async (_event, ctx) => {
    refresh(ctx);
  });

  pi.on("tool_execution_end", async (_event, ctx) => {
    refresh(ctx);
  });

  // ── Keyboard shortcut: Ctrl+Shift+B ──────────────────────────
  // First press: compact → raised. Second press from raised: open overlay.
  // From overlay return or compact: cycles normally.

  pi.registerShortcut("ctrl+shift+b", {
    description: "Toggle dashboard (compact → raised → overlay)",
    handler: (ctx) => {
      if (state.mode === "raised") {
        openOverlay(ctx);
      } else {
        toggle(ctx);
      }
    },
  });

  // ── Slash command: /dashboard [open|compact|raised] ─────────

  pi.registerCommand("dashboard", {
    description: "Toggle dashboard view, or /dashboard open for interactive overlay",
    handler: async (args, ctx) => {
      const arg = (args ?? "").trim().toLowerCase();

      if (arg === "open") {
        state.mode = "raised";
        persistMode(ctx);
        tui?.requestRender();
        await showDashboardOverlay(ctx, pi);
        return;
      }

      if (arg === "compact") {
        state.mode = "compact";
        persistMode(ctx);
        tui?.requestRender();
        ctx.ui.notify("Dashboard: compact", "info");
        return;
      }

      if (arg === "raised") {
        state.mode = "raised";
        persistMode(ctx);
        tui?.requestRender();
        ctx.ui.notify("Dashboard: raised", "info");
        return;
      }

      // Default: toggle
      toggle(ctx);
      const modeLabel = state.mode === "raised" ? "raised" : "compact";
      ctx.ui.notify(`Dashboard: ${modeLabel}`, "info");
    },
  });
}
