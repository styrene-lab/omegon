/**
 * dashboard — Unified live dashboard for Design Tree + OpenSpec + Cleave
 *
 * Renders a custom footer via setFooter() that supports modes:
 *   compact:  Dashboard summary + context gauge + original footer data
 *   raised:   Section details for design tree, openspec, cleave + footer data
 *   panel:    Non-capturing overlay (visible but doesn't steal input)
 *   focused:  Interactive overlay with keyboard navigation
 *
 * Toggle: ctrl+` or /dashboard command.
 * Cycle: compact → raised → panel → focused → compact
 *
 * Reads sharedState written by producer extensions (design-tree, openspec, cleave).
 * Subscribes to "dashboard:update" events for live re-rendering.
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import type { OverlayHandle } from "@mariozechner/pi-tui";
import { DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import { DashboardFooter } from "./footer.ts";
import { DashboardOverlay, showDashboardOverlay } from "./overlay.ts";
import type { DashboardState, DashboardMode } from "./types.ts";
import { debug } from "../debug.ts";

/** Mode cycle order for ctrl+` toggling */
const MODE_CYCLE: DashboardMode[] = ["compact", "raised", "panel", "focused"];

/** Valid /dashboard subcommands for tab completion */
const DASHBOARD_SUBCOMMANDS = ["compact", "raised", "panel", "focus", "open"];

export default function (pi: ExtensionAPI) {
  const state: DashboardState = {
    mode: "compact",
    turns: 0,
  };

  let footer: DashboardFooter | null = null;
  let tui: any = null; // TUI reference for requestRender
  let unsubscribeEvents: (() => void) | null = null;

  // ── Non-capturing overlay state ─────────────────────────────
  /** Overlay handle for non-capturing panel (visibility + focus control) */
  let overlayHandle: OverlayHandle | null = null;
  /** The done() callback to resolve the custom() promise on permanent close */
  let overlayDone: ((result: void) => void) | null = null;
  /** Whether the non-capturing overlay has been created this session */
  let overlayCreated = false;
  /** Whether focus should be applied once the handle arrives (handles async creation) */
  let pendingFocus = false;

  /**
   * Restore persisted dashboard mode from session entries.
   * Panel/focused modes restore to raised (overlay is session-transient).
   */
  function restoreMode(ctx: ExtensionContext): void {
    try {
      const entries = ctx.sessionManager.getEntries();
      for (let i = entries.length - 1; i >= 0; i--) {
        const entry = entries[i] as any;
        if (entry.type === "dashboard-state" && entry.data?.mode) {
          const saved = entry.data.mode as DashboardMode;
          // Overlay modes don't persist — fall back to raised
          state.mode = (saved === "panel" || saved === "focused") ? "raised" : saved;
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
      // Persist the base mode (panel/focused stored as raised)
      const persistable = (state.mode === "panel" || state.mode === "focused") ? "raised" : state.mode;
      pi.appendEntry("dashboard-state", { mode: persistable });
    } catch { /* session may not support it */ }
  }

  /**
   * Update footer context and trigger re-render.
   */
  function refresh(ctx: ExtensionContext): void {
    debug("dashboard", "refresh", {
      hasFooter: !!footer,
      hasTui: !!tui,
      footerType: footer?.constructor?.name,
    });
    if (footer) {
      footer.setContext(ctx);
    }
    tui?.requestRender();
  }

  /**
   * Show the non-capturing overlay panel.
   * Creates it on first call, then toggles visibility via setHidden.
   */
  function showPanel(ctx: ExtensionContext): void {
    if (overlayHandle && !overlayHandle.isHidden()) {
      // Already visible — nothing to do
      return;
    }

    if (overlayHandle) {
      // Was hidden — show it
      overlayHandle.setHidden(false);
      tui?.requestRender();
      return;
    }

    if (overlayCreated) {
      // Overlay was created but handle hasn't arrived yet (async), or
      // was permanently destroyed — don't recreate in same session
      return;
    }

    // Create the non-capturing overlay (fire-and-forget — don't await)
    overlayCreated = true;
    void ctx.ui.custom<void>(
      (tuiRef, theme, _kb, done) => {
        overlayDone = done;
        const overlay = new DashboardOverlay(tuiRef, theme, () => {
          // Esc from focused mode → unfocus, stay visible
          if (overlayHandle?.isFocused()) {
            overlayHandle.unfocus();
            state.mode = "panel";
            tui?.requestRender();
          } else {
            // Esc from unfocused panel → hide
            hidePanel();
          }
        });
        overlay.setEventBus(pi.events);
        return overlay;
      },
      {
        overlay: true,
        overlayOptions: {
          anchor: "right-center",
          width: "40%",
          minWidth: 40,
          maxHeight: "80%",
          margin: { top: 1, right: 1, bottom: 1 },
          visible: (termWidth: number) => termWidth >= 80,
          nonCapturing: true,
        },
        onHandle: (handle) => {
          overlayHandle = handle;
          // Apply deferred focus if cycleTo("focused") requested it before handle arrived
          if (pendingFocus) {
            pendingFocus = false;
            handle.focus();
          }
        },
      },
    );
  }

  /**
   * Hide the non-capturing overlay without destroying it.
   */
  function hidePanel(): void {
    pendingFocus = false;
    if (overlayHandle) {
      if (overlayHandle.isFocused()) {
        overlayHandle.unfocus();
      }
      overlayHandle.setHidden(true);
    }
    state.mode = "compact";
    tui?.requestRender();
  }

  /**
   * Focus the non-capturing overlay for interactive keyboard navigation.
   */
  function focusPanel(): void {
    if (overlayHandle && !overlayHandle.isHidden()) {
      overlayHandle.focus();
    } else {
      // Handle not yet available — defer until onHandle fires
      pendingFocus = true;
    }
  }

  /**
   * Cycle to a specific dashboard mode.
   */
  function cycleTo(ctx: ExtensionContext, targetMode: DashboardMode): void {
    state.mode = targetMode;

    switch (targetMode) {
      case "compact":
      case "raised":
        hidePanel();
        // hidePanel sets mode to "compact"; override for "raised"
        state.mode = targetMode;
        break;
      case "panel":
        pendingFocus = false;
        showPanel(ctx);
        break;
      case "focused":
        showPanel(ctx);
        focusPanel();
        break;
    }

    persistMode(ctx);
    tui?.requestRender();
  }

  /**
   * Advance to the next mode in the cycle.
   */
  function cycleNext(ctx: ExtensionContext): void {
    const currentIdx = MODE_CYCLE.indexOf(state.mode);
    const nextIdx = (currentIdx + 1) % MODE_CYCLE.length;
    cycleTo(ctx, MODE_CYCLE[nextIdx]!);
  }

  // ── Session start: set up the custom footer ──────────────────

  pi.on("session_start", async (_event, ctx) => {
    debug("dashboard", "session_start:enter", {
      hasUI: ctx.hasUI,
      cwd: ctx.cwd,
      hasSetFooter: typeof ctx.ui?.setFooter === "function",
    });
    if (!ctx.hasUI) {
      debug("dashboard", "session_start:bail", { reason: "no UI" });
      return;
    }

    state.turns = 0;
    overlayHandle = null;
    overlayDone = null;
    overlayCreated = false;
    pendingFocus = false;
    restoreMode(ctx);
    debug("dashboard", "session_start:mode", { mode: state.mode });

    // Set the custom footer
    try {
      ctx.ui.setFooter((tuiRef, theme, footerData) => {
        debug("dashboard", "footer:factory:enter", {
          hasTui: !!tuiRef,
          hasTheme: !!theme,
          hasFooterData: !!footerData,
          themeFgType: typeof theme?.fg,
        });
        try {
          tui = tuiRef;
          footer = new DashboardFooter(tuiRef, theme, footerData, state);
          footer.setContext(ctx);
          debug("dashboard", "footer:factory:ok", {
            footerType: footer?.constructor?.name,
            hasRender: typeof footer?.render === "function",
          });
          return footer;
        } catch (factoryErr: any) {
          debug("dashboard", "footer:factory:ERROR", {
            error: factoryErr?.message,
            stack: factoryErr?.stack?.split("\n").slice(0, 5).join(" | "),
          });
          throw factoryErr;
        }
      });
      debug("dashboard", "session_start:setFooter:ok");
    } catch (err: any) {
      debug("dashboard", "session_start:setFooter:ERROR", {
        error: err?.message,
        stack: err?.stack?.split("\n").slice(0, 5).join(" | "),
      });
    }

    // Subscribe to dashboard:update events from producer extensions.
    unsubscribeEvents = pi.events.on(DASHBOARD_UPDATE_EVENT, (_data) => {
      debug("dashboard", "update-event", _data as Record<string, unknown>);
      tui?.requestRender();
    });

    // Deferred initial render
    queueMicrotask(() => {
      debug("dashboard", "microtask:render", {
        tuiSet: !!tui,
        footerSet: !!footer,
        footerType: footer?.constructor?.name,
      });
      tui?.requestRender();
    });

    // Non-blocking guardrail health check — runs each check in a child process
    // to avoid blocking the event loop (execSync freezes the TUI).
    setTimeout(async () => {
      try {
        const { discoverGuardrails } = await import("../cleave/guardrails.ts");
        const { exec } = await import("node:child_process");
        const checks = discoverGuardrails(ctx.cwd);
        if (checks.length === 0) return;

        const failures: string[] = [];
        let pending = checks.length;

        for (const check of checks) {
          const timeoutMs = (check.timeout ?? 30) * 1000;
          exec(check.cmd, { cwd: ctx.cwd, timeout: timeoutMs, encoding: "utf-8" }, (err) => {
            if (err) {
              const exitCode = (err as any).code;
              const code = exitCode === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER" ? "output overflow" :
                (err as any).killed ? `timeout after ${check.timeout}s` :
                exitCode === 127 ? "command not found" :
                exitCode === 126 ? "not executable" :
                exitCode === 1 ? "errors found" :
                `exit ${exitCode ?? "?"}`;
              failures.push(`${check.name}: ${code}`);
            }
            pending--;
            if (pending === 0 && failures.length > 0) {
              const summary = failures.join(", ");
              ctx.ui.notify(`Guardrails: ${summary}`, "info");
              // Inject context the agent can see — notify() is TUI-only chrome
              pi.sendMessage({
                customType: "guardrail-health-check",
                content: `[pi-kit startup health check] Guardrail failures detected: ${summary}. `
                  + `These are static analysis checks auto-discovered by pi-kit's guardrail system `
                  + `(see extensions/cleave/guardrails.ts). Checks ran: ${checks.map(c => `\`${c.cmd}\``).join(", ")}. `
                  + `This is informational — the project may have pre-existing lint/type issues unrelated to current work.`,
                display: true,
              });
            }
          });
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
    // Permanently close the non-capturing overlay
    if (overlayHandle) {
      overlayHandle.hide();
      overlayHandle = null;
    }
    if (overlayDone) {
      overlayDone();
      overlayDone = null;
    }
    overlayCreated = false;
    pendingFocus = false;
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

  // ── Keyboard shortcut: ctrl+` ────────────────────────────────
  // Cycles through: compact → raised → panel → focused → compact

  pi.registerShortcut("ctrl+`", {
    description: "Cycle dashboard mode (compact → raised → panel → focused)",
    handler: (ctx) => {
      cycleNext(ctx);
    },
  });

  // ── Slash command: /dashboard [open|compact|raised|panel|focus] ─

  pi.registerCommand("dashboard", {
    description: "Toggle dashboard mode. Subcommands: compact, raised, panel, focus, open (legacy modal)",
    getArgumentCompletions: (prefix) => {
      const lower = prefix.toLowerCase();
      return DASHBOARD_SUBCOMMANDS
        .filter(s => s.startsWith(lower))
        .map(s => ({ label: s, value: s }));
    },
    handler: async (args, ctx) => {
      const arg = (args ?? "").trim().toLowerCase();

      if (arg === "open") {
        // Legacy modal overlay (capturing, blocks until Esc)
        state.mode = "raised";
        persistMode(ctx);
        tui?.requestRender();
        await showDashboardOverlay(ctx, pi);
        return;
      }

      if (arg === "compact") {
        cycleTo(ctx, "compact");
        ctx.ui.notify("Dashboard: compact", "info");
        return;
      }

      if (arg === "raised") {
        cycleTo(ctx, "raised");
        ctx.ui.notify("Dashboard: raised", "info");
        return;
      }

      if (arg === "panel") {
        cycleTo(ctx, "panel");
        ctx.ui.notify("Dashboard: panel (non-capturing)", "info");
        return;
      }

      if (arg === "focus") {
        cycleTo(ctx, "focused");
        ctx.ui.notify("Dashboard: focused (interactive)", "info");
        return;
      }

      // Default: cycle to next mode
      cycleNext(ctx);
      ctx.ui.notify(`Dashboard: ${state.mode}`, "info");
    },
  });
}
