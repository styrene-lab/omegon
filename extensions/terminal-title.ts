import type { ExtensionAPI, ExtensionContext } from "@cwilson613/pi-coding-agent";
import { basename } from "path";
import { DASHBOARD_UPDATE_EVENT, sharedState } from "./lib/shared-state.ts";
import type { CleaveState } from "./dashboard/types.ts";

/**
 * Dynamic terminal tab title — rich status for multi-tab workflows.
 *
 * Shows agent state, current activity, tool execution, cleave dispatch,
 * and dashboard mode in the terminal tab/window title bar.
 *
 * Format: π <project> [<status>] <activity> <flags>
 *
 * Examples:
 *   π omegon ✦                           — idle, awaiting input
 *   π omegon ◆ fixing auth bug           — thinking about user's request
 *   π omegon ⚙ Bash                      — executing a tool
 *   π omegon ⚙ Read → Edit              — tool chain (last 2)
 *   π omegon ◆ fixing auth bug ✦         — done, awaiting next input
 *   π omegon ⚡ cleave 3/5               — cleave dispatch in progress
 *   π omegon ⚡ cleave ✓                 — cleave complete
 *   π omegon T4 ◆ refactoring types      — turn 4, thinking
 */
export default function (pi: ExtensionAPI) {
  const project = basename(process.cwd());

  // ── State ──────────────────────────────────────────────────
  let ctx: ExtensionContext | null = null;
  let promptSnippet = "";
  let idle = true;
  let turnIndex = 0;

  // Tool chain — show last 2 tools for pipeline visibility
  let toolChain: string[] = [];
  let toolActive = false;

  // Cleave state from shared dashboard state
  let cleaveStatus: CleaveState["status"] = "idle";
  let cleaveDone = 0;
  let cleaveTotal = 0;

  // ── Helpers ────────────────────────────────────────────────

  function truncate(text: string, max: number): string {
    const clean = text.split("\n")[0]!.trim().replace(/\s+/g, " ");
    if (clean.length <= max) return clean;
    return clean.slice(0, max).trimEnd() + "…";
  }

  /** Read cleave state from shared dashboard state */
  function syncCleaveState(): void {
    const cleave = sharedState.cleave;
    if (cleave) {
      cleaveStatus = cleave.status;
      const children = cleave.children ?? [];
      cleaveTotal = children.length;
      cleaveDone = children.filter(c => c.status === "done").length;
    } else {
      cleaveStatus = "idle";
      cleaveDone = 0;
      cleaveTotal = 0;
    }
  }

  function render() {
    if (!ctx?.ui?.setTitle) return;

    const parts: string[] = [`Ω ${project}`];

    // Cleave dispatch — takes priority when active
    const cleaveActive = cleaveStatus !== "idle" && cleaveStatus !== "done" && cleaveStatus !== "failed";
    if (cleaveActive) {
      if (cleaveStatus === "dispatching" || cleaveStatus === "merging") {
        parts.push(`⚡ cleave ${cleaveDone}/${cleaveTotal}`);
      } else {
        parts.push(`⚡ ${cleaveStatus}`);
      }
    } else if (cleaveStatus === "done") {
      parts.push("⚡ cleave ✓");
    } else if (cleaveStatus === "failed") {
      parts.push("⚡ cleave ✗");
    }

    // Tool execution
    if (toolActive && toolChain.length > 0) {
      const display = toolChain.slice(-2).join(" → ");
      parts.push(`⚙ ${display}`);
    }
    // Agent thinking (no active tool)
    else if (!idle && promptSnippet && !cleaveActive) {
      parts.push(`◆ ${promptSnippet}`);
    }

    // Turn counter when actively working (T2+)
    if (!idle && turnIndex >= 2) {
      parts.push(`T${turnIndex}`);
    }

    // Idle indicator
    if (idle) {
      parts.push("✦");
    }

    ctx.ui.setTitle(parts.join(" "));
  }

  // ── Session lifecycle ──────────────────────────────────────

  function resetState(c: ExtensionContext) {
    ctx = c;
    promptSnippet = "";
    toolChain = [];
    toolActive = false;
    idle = true;
    turnIndex = 0;
    cleaveStatus = "idle";
    cleaveDone = 0;
    cleaveTotal = 0;
    setTimeout(render, 50);
  }

  pi.on("session_start", (_e, c) => resetState(c));
  pi.on("session_switch", (_e, c) => resetState(c));
  pi.on("session_fork", (_e, c) => resetState(c));

  // ── Agent lifecycle ────────────────────────────────────────

  pi.on("before_agent_start", (event) => {
    if (event.prompt) {
      promptSnippet = truncate(event.prompt, 30);
    }
  });

  pi.on("agent_start", (_e, c) => {
    ctx = c;
    idle = false;
    toolChain = [];
    toolActive = false;
    render();
  });

  pi.on("turn_start", (event) => {
    turnIndex = event.turnIndex;
    render();
  });

  pi.on("tool_execution_start", (event) => {
    // Deduplicate consecutive same-tool calls
    if (toolChain[toolChain.length - 1] !== event.toolName) {
      toolChain.push(event.toolName);
    }
    toolActive = true;
    render();
  });

  pi.on("tool_execution_end", () => {
    toolActive = false;
    render();
  });

  pi.on("agent_end", (_e, c) => {
    ctx = c;
    idle = true;
    toolChain = [];
    toolActive = false;
    render();
  });

  // ── Session compaction ─────────────────────────────────────

  pi.on("session_compact", () => {
    // Brief flash during compaction
    const prev = promptSnippet;
    promptSnippet = "compacting…";
    render();
    setTimeout(() => {
      promptSnippet = prev;
      render();
    }, 2000);
  });

  // ── Dashboard + cleave state updates ───────────────────────

  pi.events.on(DASHBOARD_UPDATE_EVENT, (data) => {
    const source = (data as Record<string, unknown>)?.source;
    if (source === "cleave") {
      syncCleaveState();
    }
    render();
  });
}
