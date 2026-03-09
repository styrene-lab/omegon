/**
 * Custom footer component for the unified dashboard.
 *
 * Implements two rendering modes:
 *   Layer 0 (compact): 1 line — dashboard summary only
 *   Layer 1 (raised):  up to 10 lines — section details + footer metadata
 *
 * Reads sharedState for design-tree, openspec, and cleave data.
 * Reads footerData for git branch, extension statuses, provider count.
 * Reads ExtensionContext for token stats, model, context usage.
 */

import type { Component } from "@mariozechner/pi-tui";
import type { Theme, ThemeColor } from "@mariozechner/pi-coding-agent";
import type { ReadonlyFooterDataProvider } from "@mariozechner/pi-coding-agent";
import type { ExtensionContext } from "@mariozechner/pi-coding-agent";
import type { TUI } from "@mariozechner/pi-tui";
import { truncateToWidth, visibleWidth } from "@mariozechner/pi-tui";
import type { DashboardState } from "./types.ts";
import { sharedState } from "../shared-state.ts";
import { debug } from "../debug.ts";
import { linkDashboardFile, linkOpenSpecArtifact, linkOpenSpecChange } from "./uri-helper.ts";
import { formatMemoryAuditSummary } from "./memory-audit.ts";
import { buildContextGaugeModel } from "./context-gauge.ts";

/**
 * Format token counts to compact display (e.g. 1.2k, 45k, 1.3M)
 */
function formatTokens(count: number): string {
  if (count < 1000) return count.toString();
  if (count < 10000) return `${(count / 1000).toFixed(1)}k`;
  if (count < 1000000) return `${Math.round(count / 1000)}k`;
  if (count < 10000000) return `${(count / 1000000).toFixed(1)}M`;
  return `${Math.round(count / 1000000)}M`;
}

/**
 * Sanitize text for display in a single-line status.
 */
function sanitizeStatusText(text: string): string {
  return text
    .replace(/[\r\n\t]/g, " ")
    .replace(/ +/g, " ")
    .trim();
}

const CLEAVE_STALE_MS = 30_000;

type PrioritySegment = {
  text: string;
  priority?: "high" | "low";
};

function joinPrioritySegments(width: number, segments: PrioritySegment[], separator = "  "): string {
  if (width <= 0) return "";

  const high = segments.filter((s) => s.text && s.priority !== "low");
  const low = segments.filter((s) => s.text && s.priority === "low");
  const ordered = [...high, ...low];
  if (ordered.length === 0) return "";

  const fitted: string[] = [];
  for (const segment of ordered) {
    const candidate = fitted.length === 0 ? segment.text : `${fitted.join(separator)}${separator}${segment.text}`;
    if (visibleWidth(candidate) <= width) {
      fitted.push(segment.text);
      continue;
    }

    if (segment.priority === "low") {
      continue;
    }

    const prefix = fitted.length === 0 ? "" : `${fitted.join(separator)}${separator}`;
    const remaining = Math.max(1, width - visibleWidth(prefix));
    if (remaining > 0) {
      fitted.push(truncateToWidth(segment.text, remaining, "…"));
    }
    break;
  }

  const joined = fitted.join(separator);
  return visibleWidth(joined) <= width ? joined : truncateToWidth(joined, width, "…");
}

function composePrimaryMetaLine(
  width: number,
  primary: string,
  metadata: string[],
  separator = " · ",
): string {
  return joinPrioritySegments(width, [
    { text: primary, priority: "high" },
    ...metadata.filter(Boolean).map((text) => ({ text, priority: "low" as const })),
  ], separator);
}

export class DashboardFooter implements Component {
  private tui: TUI;
  private theme: Theme;
  private footerData: ReadonlyFooterDataProvider;
  private dashState: DashboardState;
  private ctxRef: ExtensionContext | null = null;

  /** Cached cumulative token stats — updated incrementally. */
  private cachedTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 };
  private cachedThinkingLevel = "off";
  private lastEntryCount = 0;

  constructor(
    tui: TUI,
    theme: Theme,
    footerData: ReadonlyFooterDataProvider,
    dashState: DashboardState,
  ) {
    this.tui = tui;
    this.theme = theme;
    this.footerData = footerData;
    this.dashState = dashState;
  }

  /** Update the extension context reference (called on each event) */
  setContext(ctx: ExtensionContext): void {
    this.ctxRef = ctx;
  }

  /** No-op — theme is passed by reference */
  invalidate(): void {}

  dispose(): void {
    this.ctxRef = null;
  }

  render(width: number): string[] {
    debug("dashboard", "render", {
      mode: this.dashState.mode,
      width,
      hasDT: !!sharedState.designTree,
      hasOS: !!sharedState.openspec,
      hasCL: !!sharedState.cleave,
      hasCtx: !!this.ctxRef,
      hasTheme: !!this.theme,
      themeFgType: typeof this.theme?.fg,
    });
    try {
      if (this.dashState.mode === "raised") {
        return this.renderRaised(width);
      }
      // compact, panel, focused — all use compact footer (panel/focused show detail in overlay)
      return this.renderCompact(width);
    } catch (err: any) {
      debug("dashboard", "render:ERROR", {
        error: err?.message,
        stack: err?.stack?.split("\n").slice(0, 5).join(" | "),
      });
      return [`[dashboard render error: ${err?.message}]`];
    }
  }

  // ── Compact Mode (Layer 0) ────────────────────────────────────

  private renderCompact(width: number): string[] {
    const theme = this.theme;
    const lines: string[] = [];

    // Width breakpoints — expand details as space allows
    const wide = width >= 120;
    const ultraWide = width >= 160;

    // Line 1: Dashboard summary + context gauge
    const dashParts: PrioritySegment[] = [];

    // Design tree summary — responsive expansion
    const dt = sharedState.designTree;
    if (dt && dt.nodeCount > 0) {
      if (ultraWide && dt.focusedNode) {
        // Ultra-wide: show focused node title inline
        const statusIcon = dt.focusedNode.status === "decided" ? "●"
          : dt.focusedNode.status === "implementing" ? "⚙"
          : dt.focusedNode.status === "exploring" ? "◐"
          : "○";
        const qSuffix = dt.focusedNode.questions.length > 0
          ? theme.fg("dim", ` (${dt.focusedNode.questions.length}?)`)
          : "";
        dashParts.push({
          text: theme.fg("accent", `◈ ${dt.decidedCount}/${dt.nodeCount}`) +
            ` ${statusIcon} ${dt.focusedNode.title}${qSuffix}`,
        });
      } else if (wide) {
        // Wide: spell out counts, no node IDs (visible in raised mode)
        const parts = [`${dt.decidedCount} decided`];
        if (dt.exploringCount > 0) parts.push(`${dt.exploringCount} exploring`);
        if (dt.implementingCount > 0) parts.push(`${dt.implementingCount} impl`);
        if (dt.openQuestionCount > 0) parts.push(`${dt.openQuestionCount}?`);
        dashParts.push({ text: theme.fg("accent", `◈ Design`) + theme.fg("dim", ` ${parts.join(", ")}`) });
      } else {
        // Narrow: terse
        let dtSummary = `◈ D:${dt.decidedCount}`;
        if (dt.implementingCount > 0) dtSummary += ` I:${dt.implementingCount}`;
        if (dt.implementedCount > 0) dtSummary += ` ✓:${dt.implementedCount}`;
        dtSummary += `/${dt.nodeCount}`;
        dashParts.push({ text: theme.fg("accent", dtSummary) });
      }
    }

    // OpenSpec summary — responsive expansion
    const os = sharedState.openspec;
    if (os && os.changes.length > 0) {
      const active = os.changes.filter(c => c.stage !== "archived");
      if (active.length > 0) {
        if (wide) {
          // Wide: aggregate progress only — individual changes visible in raised mode
          const totalDone = active.reduce((s, c) => s + c.tasksDone, 0);
          const totalAll = active.reduce((s, c) => s + c.tasksTotal, 0);
          const allDone = totalAll > 0 && totalDone >= totalAll;
          const progress = totalAll > 0
            ? theme.fg(allDone ? "success" : "dim", ` ${totalDone}/${totalAll}`)
            : "";
          const icon = allDone ? theme.fg("success", " ✓") : "";
          dashParts.push({
            text: theme.fg("accent", `◎ Spec`) +
              theme.fg("dim", ` ${active.length} change${active.length > 1 ? "s" : ""}`) +
              progress + icon,
          });
        } else {
          dashParts.push({ text: theme.fg("accent", `◎ OS:${active.length}`) });
        }
      }
    }

    // Cleave summary — responsive expansion
    const cl = sharedState.cleave;
    if (cl) {
      if (cl.status === "idle") {
        dashParts.push({ text: theme.fg("dim", "⚡ idle") });
      } else if (cl.status === "done") {
        const childInfo = wide && cl.children
          ? ` ${cl.children.filter(c => c.status === "done").length}/${cl.children.length}`
          : "";
        dashParts.push({ text: theme.fg("success", `⚡ done${childInfo}`) });
      } else if (cl.status === "failed") {
        dashParts.push({ text: theme.fg("error", "⚡ fail") });
      } else {
        // Active dispatch — show child progress at wide widths
        if (wide && cl.children && cl.children.length > 0) {
          const done = cl.children.filter(c => c.status === "done").length;
          const running = cl.children.filter(c => c.status === "running").length;
          dashParts.push({
            text: theme.fg("warning", `⚡ ${cl.status}`) +
              theme.fg("dim", ` ${done}✓ ${running}⟳ /${cl.children.length}`),
          });
        } else {
          dashParts.push({ text: theme.fg("warning", `⚡ ${cl.status}`) });
        }
      }
    }

    // Context gauge — wider bar at wider terminals
    const barWidth = ultraWide ? 24 : wide ? 20 : 16;
    const gauge = this.buildContextGauge(barWidth);
    if (gauge) {
      dashParts.push({ text: gauge });
    }

    // Compact mode should stay dashboard-first, but still expose the active
    // provider/model in a terse way so multi-provider routing is visible.
    const ctx = this.ctxRef;
    const model = ctx?.model;
    if (model && wide) {
      const multiProvider = this.footerData.getAvailableProviderCount() > 1;
      const driverLabel = multiProvider ? model.provider : "default";
      const modelLabel = multiProvider ? `${driverLabel}/${model.id}` : model.id;
      dashParts.push({
        text: theme.fg("dim", "Model ") + theme.fg("muted", modelLabel),
        priority: "low",
      });
    }

    // Append /dash hint for discoverability (varies by mode)
    const dashHint = this.dashState.mode === "panel"
      ? theme.fg("dim", "/dashboard to close")
      : theme.fg("dim", "/dash to expand");

    const compactLine = joinPrioritySegments(width, [
      ...dashParts,
      { text: dashHint, priority: "low" },
    ]);
    lines.push(compactLine || truncateToWidth(dashHint, width, "…"));

    // Compact mode is intentionally dashboard-only. Detailed footer metadata
    // stays in raised mode so the compact footer does not look like the built-in
    // footer is still leaking through.
    return lines;
  }

  // ── Raised Mode (Layer 1) ─────────────────────────────────────

  private renderRaised(width: number): string[] {
    const theme = this.theme;

    const lines: string[] = [];

    // Design tree section
    lines.push(...this.buildDesignTreeLines(width));

    // OpenSpec section
    lines.push(...this.buildOpenSpecLines(width));

    // Cleave section
    lines.push(...this.buildCleaveLines(width));

    const raisedMeta = this.buildRaisedMetaLine(width);
    if (raisedMeta) {
      lines.push(raisedMeta);
    }

    const memoryAuditLine = this.buildMemoryAuditLine(width);
    if (memoryAuditLine) {
      lines.push(memoryAuditLine);
    }

    // Separator — thin rule matching section header style
    if (lines.length > 0) {
      const rule = "╶" + "─".repeat(Math.min(width - 2, 58)) + "╴";
      lines.push(theme.fg("dim", rule));
    }

    // /dash to compact hint prepended to footer data
    lines.push(truncateToWidth(theme.fg("dim", "/dash to compact"), width, "…"));

    // Original footer data
    lines.push(...this.renderFooterData(width));

    // Cap at 10 lines
    return lines.slice(0, 10);
  }

  /** Multi-column raised layout for wide terminals (≥120 cols) */
  private renderRaisedColumns(width: number): string[] {
    const theme = this.theme;
    const gutter = "  ";

    // Calculate column widths — give each half, minus a soft gutter.
    const colWidth = Math.floor((width - gutter.length) / 2);

    // Build left column (Design Tree + Cleave) and right column (OpenSpec)
    const leftLines = this.buildDesignTreeLines(colWidth);
    leftLines.push(...this.buildCleaveLines(colWidth));
    const rightLines = this.buildOpenSpecLines(colWidth);

    // Merge columns side by side without a literal divider. The spacing is
    // enough to separate sections and looks cleaner than an ASCII fence.
    const merged: string[] = [];
    const maxRows = Math.max(leftLines.length, rightLines.length);
    for (let i = 0; i < maxRows; i++) {
      const left = i < leftLines.length ? truncateToWidth(leftLines[i], colWidth, "…") : "";
      const right = i < rightLines.length ? truncateToWidth(rightLines[i], colWidth, "…") : "";

      const leftVisLen = left.replace(/\x1b\[[0-9;]*m/g, "").length;
      const leftPad = Math.max(0, colWidth - leftVisLen);
      merged.push(truncateToWidth(left + " ".repeat(leftPad) + gutter + right, width, "…"));
    }

    const raisedMeta = this.buildRaisedMetaLine(width);
    if (raisedMeta) {
      merged.push(raisedMeta);
    }

    const memoryAuditLine = this.buildMemoryAuditLine(width);
    if (memoryAuditLine) {
      merged.push(memoryAuditLine);
    }

    // Separator — thin rule matching section header style
    if (merged.length > 0) {
      const rule = "╶" + "─".repeat(Math.min(width - 2, 78)) + "╴";
      merged.push(theme.fg("dim", rule));
    }

    // /dash to compact hint prepended to footer data
    merged.push(truncateToWidth(theme.fg("dim", "/dash to compact"), width, "…"));

    // Footer data
    merged.push(...this.renderFooterData(width));

    return merged.slice(0, 10);
  }

  // ── Section builders (shared by stacked + column layouts) ─────

  private buildDesignTreeLines(width: number): string[] {
    const theme = this.theme;
    const lines: string[] = [];
    const dt = sharedState.designTree;
    if (!dt || dt.nodeCount === 0) return lines;

    const statusParts: string[] = [];
    if (dt.decidedCount > 0) statusParts.push(theme.fg("success", `${dt.decidedCount} decided`));
    if (dt.implementingCount > 0) statusParts.push(theme.fg("accent", `${dt.implementingCount} implementing`));
    if (dt.implementedCount > 0) statusParts.push(theme.fg("success", `${dt.implementedCount} implemented`));
    if (dt.exploringCount > 0) statusParts.push(theme.fg("accent", `${dt.exploringCount} exploring`));
    if (dt.blockedCount > 0) statusParts.push(theme.fg("error", `${dt.blockedCount} blocked`));
    if (dt.openQuestionCount > 0) statusParts.push(theme.fg("dim", `${dt.openQuestionCount}?`));

    lines.push(theme.fg("accent", "◈ Design Tree") + "  " + statusParts.join(" · "));

    // Focused node gets priority display
    if (dt.focusedNode) {
      const statusIcon = this.nodeStatusIcon(dt.focusedNode.status);
      const qCount = dt.focusedNode.questions.length > 0
        ? theme.fg("dim", ` — ${dt.focusedNode.questions.length} open questions`)
        : "";
      const branchExtra = (dt.focusedNode.branchCount ?? 0) > 1
        ? theme.fg("dim", ` +${dt.focusedNode.branchCount! - 1}`)
        : "";
      const branchInfo = dt.focusedNode.status === "implementing" && dt.focusedNode.branch
        ? theme.fg("dim", ` · ${dt.focusedNode.branch}`) + branchExtra
        : "";
      const linkedTitle = linkDashboardFile(dt.focusedNode.title, dt.focusedNode.filePath);
      lines.push(composePrimaryMetaLine(
        width,
        `  ${statusIcon} ${linkedTitle}`,
        [branchInfo, qCount],
      ));
    }

    // Implementing nodes (if no focused node)
    if (dt.implementingNodes && dt.implementingNodes.length > 0 && !dt.focusedNode) {
      for (const n of dt.implementingNodes.slice(0, 3)) {
        const branchSuffix = n.branch ? theme.fg("dim", ` · ${n.branch}`) : "";
        const linkedTitle = linkDashboardFile(n.title, n.filePath);
        lines.push(composePrimaryMetaLine(
          width,
          `  ${theme.fg("accent", "⚙")} ${linkedTitle}`,
          [branchSuffix],
        ));
      }
    }

    // If no focused node and no implementing nodes, show all nodes (up to 4)
    if (!dt.focusedNode && (!dt.implementingNodes || dt.implementingNodes.length === 0) && dt.nodes) {
      const maxShow = 4;
      for (const n of dt.nodes.slice(0, maxShow)) {
        const icon = this.nodeStatusIcon(n.status);
        const linkedId = linkDashboardFile(theme.fg("dim", n.id), n.filePath);
        const qSuffix = n.questionCount > 0 ? theme.fg("dim", ` (${n.questionCount}?)`) : "";
        lines.push(composePrimaryMetaLine(
          width,
          `  ${icon} ${linkedId}`,
          [qSuffix],
        ));
      }
      if (dt.nodes.length > maxShow) {
        lines.push(theme.fg("dim", `  +${dt.nodes.length - maxShow} more`));
      }
    }

    return lines;
  }

  private nodeStatusIcon(status: string): string {
    const theme = this.theme;
    switch (status) {
      case "decided": return theme.fg("success", "●");
      case "implementing": return theme.fg("accent", "⚙");
      case "implemented": return theme.fg("success", "✓");
      case "exploring": return theme.fg("accent", "◐");
      case "blocked": return theme.fg("error", "✕");
      case "seed": return theme.fg("dim", "○");
      default: return theme.fg("dim", "○");
    }
  }

  private buildOpenSpecLines(width: number): string[] {
    const theme = this.theme;
    const lines: string[] = [];
    const os = sharedState.openspec;
    if (!os || os.changes.length === 0) return lines;

    const totalDone = os.changes.reduce((s, c) => s + c.tasksDone, 0);
    const totalAll = os.changes.reduce((s, c) => s + c.tasksTotal, 0);
    const allComplete = totalAll > 0 && totalDone >= totalAll;
    const aggregateProgress = totalAll > 0
      ? theme.fg(allComplete ? "success" : "dim", ` ${totalDone}/${totalAll}`)
      : "";
    lines.push(
      theme.fg("accent", "◎ OpenSpec") + "  " +
      theme.fg("dim", `${os.changes.length} change${os.changes.length > 1 ? "s" : ""}`) +
      aggregateProgress,
    );

    for (const c of os.changes.slice(0, 3)) {
      const done = c.tasksTotal > 0 && c.tasksDone >= c.tasksTotal;
      const icon = done ? theme.fg("success", "✓") : theme.fg("dim", "◦");
      const progress = c.tasksTotal > 0
        ? theme.fg(done ? "success" : "dim", ` ${c.tasksDone}/${c.tasksTotal}`)
        : "";

      const stageColor = c.stage === "verifying" ? "warning"
        : c.stage === "implementing" ? "accent"
        : c.stage === "ready" ? "success"
        : "dim";
      const stageLabel = c.stage === "implementing" ? "impl"
        : c.stage === "verifying" ? "verify"
        : c.stage === "specified" ? "spec"
        : c.stage === "planned" ? "plan"
        : c.stage;
      const stage = stageLabel ? theme.fg(stageColor, ` · ${stageLabel}`) : "";

      const linkedName = linkOpenSpecChange(c.name, c.path);
      lines.push(composePrimaryMetaLine(
        width,
        `  ${icon} ${linkedName}`,
        [progress, stage],
      ));
    }

    return lines;
  }

  private buildCleaveLines(width: number): string[] {
    const theme = this.theme;
    const lines: string[] = [];
    const cl = sharedState.cleave;
    if (!cl || cl.status === "idle") return lines;

    const isTerminalState = cl.status === "done" || cl.status === "failed";
    if (isTerminalState && cl.updatedAt && (Date.now() - cl.updatedAt) > CLEAVE_STALE_MS) {
      return lines;
    }

    const statusColor: ThemeColor = cl.status === "done" ? "success"
      : cl.status === "failed" ? "error"
      : "warning";
    lines.push(composePrimaryMetaLine(
      width,
      theme.fg("accent", "⚡ Cleave"),
      [theme.fg(statusColor, cl.status)],
    ));

    if (cl.children && cl.children.length > 0) {
      const doneCount = cl.children.filter(c => c.status === "done").length;
      const failCount = cl.children.filter(c => c.status === "failed").length;
      const summary = `  ${doneCount}/${cl.children.length} ✓`;
      const failSuffix = failCount > 0 ? theme.fg("error", ` ${failCount} ✕`) : "";
      lines.push(theme.fg("dim", summary) + failSuffix);
    }

    return lines;
  }

  private buildRaisedMetaLine(width: number): string {
    const theme = this.theme;
    const wide = width >= 120;
    const barWidth = wide ? 20 : 16;
    const gauge = this.buildContextGauge(barWidth);
    const parts: string[] = [];

    if (gauge) {
      parts.push(theme.fg("dim", "Context ") + gauge);
    }

    const model = this.ctxRef?.model;
    if (model) {
      const multiProvider = this.footerData.getAvailableProviderCount() > 1;
      const driverLabel = multiProvider ? model.provider : "default";
      parts.push(theme.fg("dim", "Driver ") + theme.fg("muted", driverLabel));
      parts.push(theme.fg("dim", "Model ") + theme.fg("muted", model.id));

      if (model.reasoning) {
        const thinkColor: ThemeColor = this.cachedThinkingLevel === "high" ? "accent"
          : this.cachedThinkingLevel === "medium" ? "muted"
          : this.cachedThinkingLevel === "low" || this.cachedThinkingLevel === "minimal" ? "dim"
          : "dim";
        const thinkIcon = this.cachedThinkingLevel === "off" ? "○" : "◉";
        parts.push(theme.fg("dim", "Think ") + theme.fg(thinkColor, `${thinkIcon} ${this.cachedThinkingLevel}`));
      }
    }

    return parts.length > 0 ? truncateToWidth(parts.join(theme.fg("dim", "  ·  ")), width, "…") : "";
  }

  private buildMemoryAuditLine(width: number): string {
    const theme = this.theme;
    // Even on wide layouts, keep this compact so it reads as a footer audit
    // line rather than a third content column competing with the dashboard.
    const summary = formatMemoryAuditSummary(sharedState.lastMemoryInjection, { wide: width >= 180 });
    return truncateToWidth(theme.fg("dim", summary), width, "…");
  }

  // ── Context Gauge (from status-bar) ───────────────────────────

  private buildContextGauge(barWidth: number): string {
    const theme = this.theme;
    const ctx = this.ctxRef;
    if (!ctx) return "";

    const usage = ctx.getContextUsage();
    const contextWindow = usage?.contextWindow ?? 0;
    const model = buildContextGaugeModel({
      percent: usage?.percent,
      contextWindow,
      memoryTokenEstimate: sharedState.memoryTokenEstimate,
      turns: this.dashState.turns,
    }, barWidth);

    if (model.state === "unknown") {
      const unknownBar = theme.fg("dim", "?".repeat(barWidth));
      const windowStr = contextWindow > 0 ? theme.fg("dim", `/${formatTokens(contextWindow)}`) : "";
      return `${theme.fg("dim", `T${model.turns}`)} ${unknownBar} ${theme.fg("dim", "?")}${windowStr}`;
    }

    const percent = model.percent ?? 0;

    // Severity color for non-memory context pressure
    const otherColor: ThemeColor = percent > 70 ? "error" : percent > 45 ? "warning" : "muted";

    let bar = "";
    if (model.memoryBlocks > 0) bar += theme.fg("accent", "▓".repeat(model.memoryBlocks));
    if (model.otherBlocks > 0) bar += theme.fg(otherColor, "█".repeat(model.otherBlocks));
    if (model.freeBlocks > 0) bar += theme.fg("dim", "░".repeat(model.freeBlocks));

    const pctStr = `${Math.round(percent)}%`;
    const pctColored = percent > 70 ? theme.fg("error", pctStr)
      : percent > 45 ? theme.fg("warning", pctStr)
      : theme.fg("dim", pctStr);
    const windowStr = contextWindow > 0 ? theme.fg("dim", `/${formatTokens(contextWindow)}`) : "";

    return `${theme.fg("dim", `T${model.turns}`)} ${bar} ${pctColored}${windowStr}`;
  }

  // ── Original Footer Data ──────────────────────────────────────

  private renderFooterData(width: number): string[] {
    debug("dashboard", "renderFooterData:enter", {
      width,
      hasCtx: !!this.ctxRef,
      hasTheme: !!this.theme,
      hasBranch: !!this.footerData?.getGitBranch?.(),
    });
    const theme = this.theme;
    const ctx = this.ctxRef;
    const lines: string[] = [];
    const wide = width >= 120;

    // ── Line 1: pwd + git branch + session ──
    let pwd = process.cwd();
    const home = process.env.HOME || process.env.USERPROFILE;
    if (home && pwd.startsWith(home)) {
      pwd = `~${pwd.slice(home.length)}`;
    }

    let pwdLine = theme.fg("dim", "⌂ ") + theme.fg("muted", pwd);

    const branch = this.footerData.getGitBranch();
    if (branch) {
      // Color branch by convention: feature→accent, fix→warning, main/master→success
      const branchColor: ThemeColor = /^(main|master)$/.test(branch) ? "success"
        : branch.startsWith("feature/") ? "accent"
        : branch.startsWith("fix/") || branch.startsWith("hotfix/") ? "warning"
        : branch.startsWith("refactor/") ? "accent"
        : "muted";
      pwdLine += theme.fg("dim", "  ") + theme.fg(branchColor, branch);
    }

    const sessionName = ctx?.sessionManager?.getSessionName?.();
    if (sessionName) {
      pwdLine += theme.fg("dim", " • ") + theme.fg("muted", sessionName);
    }

    lines.push(truncateToWidth(pwdLine, width, "…"));

    // ── Line 2: token stats + cost │ model + thinking ──
    if (ctx) {
      // Incrementally update cached token stats (only scan new entries)
      try {
        const entries = ctx.sessionManager.getEntries();
        for (let i = this.lastEntryCount; i < entries.length; i++) {
          const entry = entries[i] as any;
          if (entry.type === "message" && entry.message?.role === "assistant") {
            const usage = entry.message.usage;
            if (usage) {
              this.cachedTokens.input += usage.input || 0;
              this.cachedTokens.output += usage.output || 0;
              this.cachedTokens.cacheRead += usage.cacheRead || 0;
              this.cachedTokens.cacheWrite += usage.cacheWrite || 0;
              this.cachedTokens.cost += usage.cost?.total || 0;
            }
          }
          if (entry.type === "thinking_level_change" && entry.thinkingLevel) {
            this.cachedThinkingLevel = entry.thinkingLevel;
          }
        }
        this.lastEntryCount = entries.length;
      } catch { /* session may not be ready */ }

      // Left side: simplified context usage as requested (X%/tokens)
      const usage = ctx.getContextUsage();
      const statsLeft = usage
        ? theme.fg("dim", `${Math.round(usage.percent ?? 0)}%/${formatTokens(usage.contextWindow ?? 0)}`)
        : theme.fg("dim", "0%");

      // Right side: provider + model + thinking level badge
      const model = ctx.model;
      const modelName = model?.id || "no-model";
      const rightParts: string[] = [];

      // Multi-provider indicator
      if (this.footerData.getAvailableProviderCount() > 1 && model) {
        rightParts.push(theme.fg("dim", `(${model.provider})`));
      }

      rightParts.push(theme.fg("muted", modelName));

      // Thinking level badge with semantic color
      if (model?.reasoning) {
        const thinkColor: ThemeColor = this.cachedThinkingLevel === "high" ? "accent"
          : this.cachedThinkingLevel === "medium" ? "muted"
          : this.cachedThinkingLevel === "low" || this.cachedThinkingLevel === "minimal" ? "dim"
          : "dim";
        const thinkIcon = this.cachedThinkingLevel === "off" ? "○" : "◉";
        rightParts.push(theme.fg("dim", "•") + " " +
          theme.fg(thinkColor, `${thinkIcon} ${this.cachedThinkingLevel}`));
      }

      const rightSide = rightParts.join(" ");

      // Layout: left-align stats, right-align model
      const statsLeftPlain = statsLeft.replace(/\x1b\[[0-9;]*m/g, "").length;
      const rightSidePlain = rightSide.replace(/\x1b\[[0-9;]*m/g, "").length;

      let statsLine: string;
      if (statsLeftPlain + 2 + rightSidePlain <= width) {
        const padding = " ".repeat(width - statsLeftPlain - rightSidePlain);
        statsLine = statsLeft + padding + rightSide;
      } else {
        statsLine = statsLeft;
      }

      lines.push(statsLine);
    }

    // ── Extension statuses — raised mode only ──
    if (this.dashState.mode === "raised") {
      const extensionStatuses = this.footerData.getExtensionStatuses();
      if (extensionStatuses.size > 0) {
        const sortedStatuses = Array.from(extensionStatuses.entries())
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([name, text]) => {
            const cleanText = sanitizeStatusText(text);
            return theme.fg("dim", "▪ ") + theme.fg("muted", cleanText);
          });
        const statusLine = sortedStatuses.join(theme.fg("dim", "  "));
        lines.push(truncateToWidth(statusLine, width, "…"));
      }
    }

    return lines;
  }
}
