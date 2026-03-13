/**
 * Custom footer component for the unified dashboard.
 *
 * Implements two rendering modes:
 *   Layer 0 (compact): 1 line — dashboard summary only
 *   Layer 1 (raised):  uncapped — section details, branch tree, and footer metadata
 *
 * Reads sharedState for design-tree, openspec, and cleave data.
 * Reads footerData for git branch, extension statuses, provider count.
 * Reads ExtensionContext for token stats, model, context usage.
 */

import type { Component } from "@cwilson613/pi-tui";
import type { Theme, ThemeColor } from "@cwilson613/pi-coding-agent";
import type { ReadonlyFooterDataProvider } from "@cwilson613/pi-coding-agent";
import type { ExtensionContext } from "@cwilson613/pi-coding-agent";
import type { TUI } from "@cwilson613/pi-tui";
import { truncateToWidth, visibleWidth } from "@cwilson613/pi-tui";
import { leftRight, mergeColumns, padRight } from "./render-utils.ts";
import { buildBranchTreeLines, readLocalBranches } from "./git.ts";
import type { DashboardState, RecoveryCooldownSummary, RecoveryDashboardState } from "./types.ts";
import { sharedState } from "../shared-state.ts";
import { debug } from "../debug.ts";
import { linkDashboardFile, linkOpenSpecArtifact, linkOpenSpecChange } from "./uri-helper.ts";
import { designSpecBadge } from "./overlay-data.ts";
import { buildContextGaugeModel } from "./context-gauge.ts";

/**
 * Box-drawing character set.
 *
 * When `TERM=dumb`, `PI_ASCII=1`, or `LC_ALL`/`LANG` indicates a non-UTF-8
 * locale, fall back to plain ASCII characters that render on every terminal.
 * Otherwise use the Unicode rounded-box set that looks nice in modern emulators.
 */
const useAsciiBoxChars = (() => {
  if (process.env["PI_ASCII"] === "1") return true;
  if (process.env["TERM"] === "dumb") return true;
  // Basic locale check: if LANG/LC_ALL/LC_CTYPE doesn't mention UTF, fall back.
  const locale = (process.env["LC_ALL"] ?? process.env["LC_CTYPE"] ?? process.env["LANG"] ?? "").toUpperCase();
  if (locale && !locale.includes("UTF")) return true;
  return false;
})();

const BOX = useAsciiBoxChars
  ? { tl: "+", tr: "+", bl: "+", br: "+", h: "-", v: "|", vr: "+", vl: "+", hd: "+", hu: "+" }
  : { tl: "╭", tr: "╮", bl: "╰", br: "╯", h: "─", v: "│", vr: "├", vl: "┤", hd: "┬", hu: "┴" };

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

function getRecoveryState(): RecoveryDashboardState | undefined {
  return sharedState.recovery;
}

function formatCooldownRemaining(until: number, now: number = Date.now()): string {
  const remainingMs = Math.max(0, until - now);
  const totalSeconds = Math.ceil(remainingMs / 1000);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return seconds > 0 ? `${minutes}m${seconds}s` : `${minutes}m`;
}

function summarizeCooldown(cooldowns: RecoveryCooldownSummary[] | undefined): string | null {
  if (!cooldowns || cooldowns.length === 0) return null;
  const next = [...cooldowns].sort((a, b) => a.until - b.until)[0];
  const target = next.scope === "provider"
    ? next.provider ?? next.key
    : next.modelId ? `${next.provider ?? "candidate"}/${next.modelId}` : next.key;
  return `${target} ${formatCooldownRemaining(next.until)}`;
}

const CLEAVE_STALE_MS = 30_000;
/** Recovery notices auto-suppress in compact mode after this many ms with no new error. */
const RECOVERY_STALE_MS = 45_000;

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
            text: theme.fg("accent", `◎ Impl`) +
              theme.fg("dim", ` ${active.length} change${active.length > 1 ? "s" : ""}`) +
              progress + icon,
          });
        } else {
          dashParts.push({ text: theme.fg("accent", `◎ Impl:${active.length}`) });
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
        // Active dispatch — show child progress + lastLine activity hint
        if (wide && cl.children && cl.children.length > 0) {
          const done = cl.children.filter(c => c.status === "done").length;
          const running = cl.children.filter(c => c.status === "running").length;
          // Show the last active line from whichever running child has one
          const activeChild = cl.children.find(c => c.status === "running" && c.lastLine);
          const activityHint = activeChild?.lastLine
            ? theme.fg("dim", `  ${activeChild.lastLine.slice(0, 40)}…`)
            : "";
          dashParts.push({
            text: theme.fg("warning", `⚡ ${cl.status}`) +
              theme.fg("dim", ` ${done}✓ ${running}⟳ /${cl.children.length}`) +
              activityHint,
          });
        } else {
          dashParts.push({ text: theme.fg("warning", `⚡ ${cl.status}`) });
        }
      }
    }

    const recoveryLine = this.buildRecoveryCompactSummary(width, wide);
    if (recoveryLine) {
      dashParts.push({ text: recoveryLine });
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
    return width >= 120 ? this.renderRaisedWide(width) : this.renderRaisedStacked(width);
  }

  /**
   * Build the git branch tree lines for the raised layout.
   * Reads local branches from .git/refs/heads/ (no shell spawn).
   */
  private buildBranchTree(width: number): string[] {
    const MAX_BRANCHES = 8;
    const cwd = process.cwd();
    const repoName = cwd.split("/").pop() ?? cwd;
    const currentBranch = this.footerData.getGitBranch();
    const allBranches = readLocalBranches(cwd);
    // Cap branches fed to the tree renderer; append a hint if truncated
    const truncatedBranches = allBranches.length > MAX_BRANCHES
      ? allBranches.slice(0, MAX_BRANCHES)
      : allBranches;
    const hiddenCount = allBranches.length > MAX_BRANCHES
      ? allBranches.length - MAX_BRANCHES
      : 0;
    const designNodes = sharedState.designTree?.nodes?.map((n) => ({
      branches: n.branches ?? [],
      title: n.title,
    }));
    const lines = buildBranchTreeLines(
      { repoName, currentBranch, allBranches: truncatedBranches, designNodes },
      this.theme,
    );
    const result = lines.map((l) => truncateToWidth(l, width, "…"));
    if (hiddenCount > 0) {
      result.push(
        this.theme.fg("dim", `  … ${hiddenCount} more branches`) +
        this.theme.fg("dim", " — /dashboard to expand"),
      );
    }
    return result;
  }

  /**
   * Render content + footer lines inside a rounded box with a top border label
   * and a `/dash to compact` hint embedded in the bottom border.
   *
   * @param targetHeight  Optional fixed height — pads content with blank lines
   *                      so the total box reaches this row count.  Omit (or 0)
   *                      to render at natural content height.
   */
  private renderBoxed(
    contentLines: string[],
    footerLines: string[],
    topLineContent: string,
    width: number,
    targetHeight = 0,
  ): string[] {
    const theme = this.theme;
    const innerWidth = width - 4; // 2 for │ borders + 2 for padding spaces

    const b = (s: string) => theme.fg("border", s);

    const wrapLine = (line: string) =>
      b(BOX.v) + " " + padRight(truncateToWidth(line, innerWidth, "…"), innerWidth) + " " + b(BOX.v);

    // Top border overhead is 5 chars (╭─·space·╮), one more than content lines (│·space·│ = 4).
    // Truncate topLineContent to width-5 so the border never exceeds terminal width.
    const topMaxWidth = width - 5;
    const safeTopLine = visibleWidth(topLineContent) > topMaxWidth
      ? truncateToWidth(topLineContent, topMaxWidth, "…")
      : topLineContent;
    const topPad = Math.max(0, topMaxWidth - visibleWidth(safeTopLine));
    const topBorder = b(BOX.tl) + b(BOX.h) + " " + safeTopLine + " " + b(BOX.h.repeat(topPad)) + b(BOX.tr);

    const separator = b(BOX.vr) + b(BOX.h.repeat(width - 2)) + b(BOX.vl);

    const dashHint = " /dash to compact · /dashboard to expand ";
    const botPad = Math.max(0, width - 2 - visibleWidth(dashHint));
    const bottomBorder = b(BOX.bl) + theme.fg("dim", dashHint) + b(BOX.h.repeat(botPad)) + b(BOX.br);

    // Compute how many blank padding lines we need in the content area so the
    // total rendered box reaches targetHeight.
    //   box height = 1 (top) + content + [1 separator + footer] + 1 (bottom)
    const boxChrome = 1 + 1 + (footerLines.length > 0 ? 1 + footerLines.length : 0);
    const paddedContentLength = targetHeight > 0
      ? Math.max(contentLines.length, targetHeight - boxChrome)
      : contentLines.length;

    const lines: string[] = [topBorder];
    for (const line of contentLines) lines.push(wrapLine(line));
    // Fill blank rows up to paddedContentLength
    for (let i = contentLines.length; i < paddedContentLength; i++) {
      lines.push(wrapLine(""));
    }
    if (footerLines.length > 0) {
      lines.push(separator);
      for (const line of footerLines) lines.push(wrapLine(line));
    }
    lines.push(bottomBorder);
    return lines;
  }

  /**
   * Stacked layout for narrow terminals (<120 cols).
   * All sections rendered full-width inside a corner-bounded box.
   */
  private renderRaisedStacked(width: number): string[] {
    const innerWidth = width - 4;
    const branchLines = this.buildBranchTree(innerWidth);
    const [topLine = "", ...extraBranchLines] = branchLines;

    // The first branch line is embedded in the top border as "╭─ [topLine] ─╮",
    // which adds a 3-char prefix (╭─·).  Content lines are wrapped with "│·"
    // (2-char prefix).  Shift continuation branch lines right by 1 to keep
    // ├─/└─ connectors vertically aligned with the ┬ junction in the border.
    const alignedBranchLines = extraBranchLines.map((l) => " " + l);

    const contentLines = [
      ...alignedBranchLines,
      ...this.buildDesignTreeLines(innerWidth),
      ...this.buildOpenSpecLines(innerWidth),
      ...this.buildRecoveryLines(innerWidth),
      ...this.buildCleaveLines(innerWidth),
    ];

    // Render at natural content height — the box grows upward from the footer
    // as branches/specs/cleave tasks are added.  Full-screen expansion lives
    // in the /dashboard overlay (overlay.ts), not here.
    return this.renderBoxed(contentLines, this.buildFooterZone(innerWidth), topLine, width);
  }

  /**
   * Wide layout (≥120 cols) — two-column content inside a corner-bounded box.
   *   Left:  Design tree + Recovery + Cleave (active work context)
   *   Right: Implementation (spec/task progress)
   *   Footer zone: shared meta, memory, footer data
   */
  private renderRaisedWide(width: number): string[] {
    const innerWidth = width - 4;
    const leftColWidth = Math.floor((innerWidth - 1) / 2);
    const rightColWidth = innerWidth - leftColWidth - 1;
    const colDivider = this.theme.fg("dim", BOX.v);

    const branchLines = this.buildBranchTree(innerWidth);
    const [topLine = "", ...extraBranchLines] = branchLines;

    // Same 1-char alignment correction as renderRaisedStacked.
    const alignedBranchLines = extraBranchLines.map((l) => " " + l);

    const leftLines = [
      ...this.buildDesignTreeLines(leftColWidth),
      ...this.buildRecoveryLines(leftColWidth),
      ...this.buildCleaveLines(leftColWidth),
    ];
    const rightLines = this.buildOpenSpecLines(rightColWidth);

    const contentLines: string[] = [
      ...alignedBranchLines,
      ...(leftLines.length > 0 || rightLines.length > 0
        ? mergeColumns(leftLines, rightLines, leftColWidth, rightColWidth, colDivider)
        : []),
    ];

    // Same as stacked: natural content height, grows up from footer as needed.
    return this.renderBoxed(contentLines, this.buildFooterZone(innerWidth), topLine, width);
  }

  // ── HUD Footer Zone (raised mode) ────────────────────────────

  /**
   * Dim section divider with a lowercase label flush-left.
   * Fills the remaining inner width with ─ chars.
   *
   *   ── context ────────────────────────────────────────────
   */
  private buildHudSectionDivider(label: string, innerWidth: number): string {
    const prefix = `── ${label} `;
    const fill = Math.max(0, innerWidth - visibleWidth(prefix));
    return this.theme.fg("dim", prefix + "─".repeat(fill));
  }

  /**
   * HUD context section — two lines:
   *   ▐▓▓████░░░░░░░░░░░░░░░▌ 43% / 200k  T·8
   *   ▸ anthropic / claude-opus-4-6  ·  ◉ high
   *
   * Bar uses ▐▌ half-block delimiters (panel-slot look).
   * ▸ acts as active-pointer glyph; ◉/○ for thinking on/off.
   */
  private buildHudContextLines(width: number): string[] {
    const theme = this.theme;
    const ctx = this.ctxRef;
    if (!ctx) return [];

    const wide = width >= 100;
    const barWidth = wide ? 22 : 14;
    const lines: string[] = [];

    // ── Bar line ──────────────────────────────────────────────
    const usage = ctx.getContextUsage();
    const contextWindow = usage?.contextWindow ?? 0;
    const gaugeModel = buildContextGaugeModel({
      percent: usage?.percent,
      contextWindow,
      memoryTokenEstimate: sharedState.memoryTokenEstimate,
      turns: this.dashState.turns,
    }, barWidth);

    const bLeft  = theme.fg("dim", "▐");
    const bRight = theme.fg("dim", "▌");

    if (gaugeModel.state === "unknown") {
      const unknownBar = theme.fg("dim", "?".repeat(barWidth));
      const winStr = contextWindow > 0 ? theme.fg("dim", ` / ${formatTokens(contextWindow)}`) : "";
      lines.push(`  ${bLeft}${unknownBar}${bRight} ${theme.fg("dim", "?")}${winStr}`);
    } else {
      const percent = gaugeModel.percent ?? 0;
      const otherColor: ThemeColor = percent > 70 ? "error" : percent > 45 ? "warning" : "muted";
      let bar = "";
      if (gaugeModel.memoryBlocks > 0) bar += theme.fg("accent", "▓".repeat(gaugeModel.memoryBlocks));
      if (gaugeModel.otherBlocks > 0)  bar += theme.fg(otherColor, "█".repeat(gaugeModel.otherBlocks));
      if (gaugeModel.freeBlocks > 0)   bar += theme.fg("border", "░".repeat(gaugeModel.freeBlocks));

      const pctNum = Math.round(percent);
      const pctColor: ThemeColor = percent > 70 ? "error" : percent > 45 ? "warning" : "dim";
      const pctStr = theme.fg(pctColor, `${pctNum}%`);
      const winStr = contextWindow > 0 ? theme.fg("dim", ` / ${formatTokens(contextWindow)}`) : "";
      const turnStr = gaugeModel.turns > 0 ? `  ${theme.fg("dim", `T·${gaugeModel.turns}`)}` : "";
      lines.push(`  ${bLeft}${bar}${bRight} ${pctStr}${winStr}${turnStr}`);
    }

    // ── Provider / model / thinking line ──────────────────────
    const m = ctx.model;
    if (m) {
      const multiProvider = this.footerData.getAvailableProviderCount() > 1;
      const pointer = theme.fg("accent", "▸");
      const dot = theme.fg("dim", "  ·  ");

      const providerModel = multiProvider
        ? `${pointer} ${theme.fg("muted", m.provider)} ${theme.fg("dim", "/")} ${theme.fg("muted", m.id)}`
        : `${pointer} ${theme.fg("muted", m.id)}`;

      const parts: string[] = [providerModel];

      if (m.reasoning) {
        const thinkColor: ThemeColor = this.cachedThinkingLevel === "high"    ? "accent"
          : this.cachedThinkingLevel === "medium"   ? "muted"
          : "dim";
        const thinkIcon = this.cachedThinkingLevel === "off"
          ? theme.fg("dim", "○")
          : theme.fg(thinkColor, "◉");
        parts.push(`${thinkIcon} ${theme.fg(thinkColor, this.cachedThinkingLevel)}`);
      }

      if (this.cachedTokens.cost > 0) {
        parts.push(theme.fg("dim", `$${this.cachedTokens.cost.toFixed(3)}`));
      }

      lines.push(truncateToWidth(`  ${parts.join(dot)}`, width, "…"));
    }

    return lines;
  }

  /**
   * HUD memory section — single line:
   *   ⌗ 1167  ·  inj 23  ·  wm 5  ·  ep 3  ·  gl 2  ·  ~4.2k
   *
   * ⌗ is the established memory glyph. Sub-labels are dim, values muted.
   */
  private buildHudMemoryLine(width: number): string {
    const theme = this.theme;
    const extStatuses = this.footerData.getExtensionStatuses();
    const memStatus = extStatuses.get("memory") ?? "";
    const totalMatch = memStatus.match(/(\d+)\s+facts/);
    const totalFacts = totalMatch ? parseInt(totalMatch[1], 10) : null;
    const metrics = sharedState.lastMemoryInjection;

    if (!metrics && totalFacts === null) return "";

    const sep = theme.fg("dim", "  ·  ");
    const parts: string[] = [];

    if (totalFacts !== null) {
      parts.push(`${theme.fg("accent", "⌗")} ${theme.fg("muted", String(totalFacts))}`);
    }

    if (metrics) {
      if (metrics.projectFactCount > 0)
        parts.push(theme.fg("dim", "inj ") + theme.fg("muted", String(metrics.projectFactCount)));
      if (metrics.workingMemoryFactCount > 0)
        parts.push(theme.fg("dim", "wm ")  + theme.fg("muted", String(metrics.workingMemoryFactCount)));
      if (metrics.episodeCount > 0)
        parts.push(theme.fg("dim", "ep ")  + theme.fg("muted", String(metrics.episodeCount)));
      if (metrics.globalFactCount > 0)
        parts.push(theme.fg("dim", "gl ")  + theme.fg("muted", String(metrics.globalFactCount)));
      parts.push(theme.fg("dim", `~${metrics.estimatedTokens}`));
    } else {
      parts.push(theme.fg("dim", "pending injection"));
    }

    return truncateToWidth(`  ${parts.join(sep)}`, width, "…");
  }

  /**
   * HUD system section — one or two lines:
   *   ⌂ ~/workspace/ai/omegon                       ◦ my-session
   *   ⚡ dispatch 3/8  ·  ◎ 2 active  ·  ↑ ok
   *
   * Extension badges reuse the established content-section glyphs so the
   * footer visually echoes the dashboard sections above it.
   */
  private buildHudSystemLines(width: number): string[] {
    const theme = this.theme;
    const ctx = this.ctxRef;
    const lines: string[] = [];

    // ── pwd + session ─────────────────────────────────────────
    let pwd = process.cwd();
    const home = process.env.HOME || process.env.USERPROFILE;
    if (home && pwd.startsWith(home)) pwd = `~${pwd.slice(home.length)}`;

    const pwdStr = theme.fg("dim", "⌂ ") + theme.fg("muted", pwd);
    const sessionName = ctx?.sessionManager?.getSessionName?.();
    const sessionStr = sessionName
      ? theme.fg("dim", "◦ ") + theme.fg("muted", sessionName)
      : "";

    lines.push(sessionStr
      ? leftRight(`  ${pwdStr}`, sessionStr, width)
      : truncateToWidth(`  ${pwdStr}`, width, "…"),
    );

    // ── Extension badges ──────────────────────────────────────
    const GLYPH: Record<string, string> = {
      "cleave":        "⚡",
      "openspec":      "◎",
      "version-check": "↑",
      "version":       "↑",
      "design-tree":   "◈",
      "dashboard":     "◐",
    };

    const extStatuses = this.footerData.getExtensionStatuses();
    const badges = Array.from(extStatuses.entries())
      .filter(([name]) => name !== "memory")
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([name, text]) => {
        const glyph = GLYPH[name] ?? "▸";
        return theme.fg("accent", glyph) + " " + theme.fg("dim", sanitizeStatusText(text));
      });

    if (badges.length > 0) {
      lines.push(truncateToWidth(
        `  ${badges.join(theme.fg("dim", "  ·  "))}`,
        width, "…",
      ));
    }

    return lines;
  }

  /**
   * Assemble the full HUD footer zone from the three named sections.
   * Sections collapse when they have no data to show.
   */
  private buildFooterZone(width: number): string[] {
    // Keep token cache current (not called in compact mode — intentional).
    this._updateTokenCache();

    const zone: string[] = [];

    const contextLines = this.buildHudContextLines(width);
    if (contextLines.length > 0) {
      zone.push(this.buildHudSectionDivider("context", width));
      zone.push(...contextLines);
    }

    const memLine = this.buildHudMemoryLine(width);
    if (memLine) {
      zone.push(this.buildHudSectionDivider("memory", width));
      zone.push(memLine);
    }

    const systemLines = this.buildHudSystemLines(width);
    if (systemLines.length > 0) {
      zone.push(this.buildHudSectionDivider("system", width));
      zone.push(...systemLines);
    }

    return zone;
  }

  // ── Section builders (shared by stacked + wide layouts) ───────

  private buildRecoveryCompactSummary(width: number, wide: boolean): string {
    const theme = this.theme;
    const recovery = getRecoveryState();
    if (!recovery) return "";

    // Auto-suppress stale recovery notices in compact mode — they outlive their
    // usefulness quickly and crowd out model/driver/thinking info.
    if (Date.now() - recovery.timestamp > RECOVERY_STALE_MS) return "";

    // Past-tense labels for auto-handled actions so they read as status, not
    // directives.  'escalate' is the only case where the operator must act.
    const actionColor: ThemeColor = recovery.action === "retry" ? "warning"
      : recovery.action === "switch_candidate" || recovery.action === "switch_offline" ? "accent"
      : recovery.action === "cooldown" ? "warning"
      : recovery.action === "escalate" ? "error"
      : "dim";
    const actionLabel = recovery.action === "retry" ? "retried"
      : recovery.action === "switch_candidate" ? "switched"
      : recovery.action === "switch_offline" ? "went offline"
      : recovery.action === "cooldown" ? "cooling"
      : recovery.action === "escalate" ? "escalated"
      : "observed";

    // Compact mode: terse badge.  Wide adds provider/model context.
    // Escalate appends a dim command hint so the operator knows what to do.
    const summary = wide ? `${recovery.provider}/${recovery.modelId}` : "";
    const cooldown = summarizeCooldown(recovery.cooldowns);
    const escalateHint = recovery.action === "escalate"
      ? theme.fg("dim", "→ /set-model-tier")
      : "";
    const icon = recovery.action === "escalate" ? "⚠" : "↺";
    return composePrimaryMetaLine(width,
      theme.fg(actionColor, `${icon} ${actionLabel}`),
      [summary ? theme.fg("dim", summary) : "", cooldown ? theme.fg("dim", cooldown) : "", escalateHint].filter(Boolean),
    );
  }

  private buildRecoveryLines(width: number): string[] {
    const theme = this.theme;
    const recovery = getRecoveryState();
    if (!recovery) return [];

    // Collapse non-actionable states (observed/informational) in raised mode —
    // they add noise without operator-relevant information.
    if (recovery.action === "observe") return [];

    const actionColor: ThemeColor = recovery.action === "retry" ? "warning"
      : recovery.action === "switch_candidate" || recovery.action === "switch_offline" ? "accent"
      : recovery.action === "cooldown" ? "warning"
      : recovery.action === "escalate" ? "error"
      : "dim";
    const actionLabel = recovery.action === "retry" ? "retried"
      : recovery.action === "switch_candidate" ? "switched candidate"
      : recovery.action === "switch_offline" ? "went offline"
      : recovery.action === "cooldown" ? "cooling"
      : recovery.action === "escalate" ? "escalated — operator action required"
      : "observed";

    const recoveryIcon = recovery.action === "escalate" ? "⚠" : "↺";
    const escalateHint = recovery.action === "escalate"
      ? theme.fg("dim", "→ /set-model-tier to switch provider/driver")
      : "";

    const headerParts = [theme.fg(actionColor, actionLabel), theme.fg("dim", recovery.classification)];
    const lines = [composePrimaryMetaLine(
      width,
      theme.fg("accent", `${recoveryIcon} Recovery`),
      headerParts,
    )];
    if (escalateHint) lines.push(escalateHint);

    const target = recovery.target?.modelId
      ? `${recovery.target.provider}/${recovery.target.modelId}`
      : recovery.target?.provider;
    const retry = recovery.retryCount != null && recovery.maxRetries != null
      ? `${recovery.retryCount}/${recovery.maxRetries} retries`
      : "";
    const cooldown = summarizeCooldown(recovery.cooldowns);
    lines.push(composePrimaryMetaLine(
      width,
      `  ${sanitizeStatusText(recovery.summary)}`,
      [retry ? theme.fg("dim", retry) : "", target ? theme.fg("dim", `→ ${target}`) : "", cooldown ? theme.fg("dim", `cooldown ${cooldown}`) : ""],
    ));

    return lines;
  }

  private buildDesignTreeLines(width: number): string[] {
    const theme = this.theme;
    const lines: string[] = [];
    const dt = sharedState.designTree;
    if (!dt || dt.nodeCount === 0) return lines;

    lines.push(theme.fg("accent", "◈ Design Tree"));

    const statusParts: string[] = [];
    if (dt.decidedCount > 0) statusParts.push(theme.fg("success", `${dt.decidedCount} decided`));
    if (dt.implementingCount > 0) statusParts.push(theme.fg("accent", `${dt.implementingCount} implementing`));
    if (dt.exploringCount > 0) statusParts.push(theme.fg("muted", `${dt.exploringCount} exploring`));
    if (dt.blockedCount > 0) statusParts.push(theme.fg("error", `${dt.blockedCount} blocked`));
    if (dt.deferredCount > 0) statusParts.push(theme.fg("dim", `${dt.deferredCount} deferred`));
    if (dt.openQuestionCount > 0) statusParts.push(theme.fg("dim", `${dt.openQuestionCount}?`));

    if (statusParts.length > 0) {
      lines.push("  " + statusParts.join(" · "));
    }

    // Pipeline funnel row (after the status-counts line)
    if (dt.designPipeline) {
      const p = dt.designPipeline;
      const funnelParts: string[] = [];
      if (p.designing > 0)    funnelParts.push(theme.fg("accent", `${p.designing} designing`));
      if (p.decided > 0)      funnelParts.push(theme.fg("success", `${p.decided} decided`));
      if (p.implementing > 0) funnelParts.push(theme.fg("warning", `${p.implementing} implementing`));
      if (p.done > 0)         funnelParts.push(theme.fg("success", `${p.done} done`));
      if (funnelParts.length > 0) {
        lines.push(theme.fg("dim", "  → ") + funnelParts.join(theme.fg("dim", " · ")));
      }
      if (p.needsSpec > 0) {
        lines.push(theme.fg("dim", "  ") + theme.fg("warning", `✦ ${p.needsSpec} need spec`));
      }
    }

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
        ? theme.fg("dim", dt.focusedNode.branch) + branchExtra
        : "";
      const linkedTitle = linkDashboardFile(dt.focusedNode.title, dt.focusedNode.filePath);
      // TODO(types-and-emission): DesignTreeFocusedNode lacks designSpec/assessmentResult fields.
      // Once the sibling task adds those fields, call designSpecBadge here and append to the line.
      lines.push(composePrimaryMetaLine(
        width,
        `  ${statusIcon} ${linkedTitle}`,
        [branchInfo, qCount],
      ));
    }

    // Implementing nodes (if no focused node)
    const MAX_IMPL_NODES = 4;
    if (dt.implementingNodes && dt.implementingNodes.length > 0 && !dt.focusedNode) {
      for (const n of dt.implementingNodes.slice(0, MAX_IMPL_NODES)) {
        const branchSuffix = n.branch ? theme.fg("dim", n.branch) : "";
        const linkedTitle = linkDashboardFile(n.title, n.filePath);
        lines.push(composePrimaryMetaLine(
          width,
          `  ${theme.fg("accent", "⚙")} ${linkedTitle}`,
          [branchSuffix],
        ));
      }
      if (dt.implementingNodes.length > MAX_IMPL_NODES) {
        lines.push(
          theme.fg("dim", `  … ${dt.implementingNodes.length - MAX_IMPL_NODES} more`) +
          theme.fg("dim", " — /dashboard to expand"),
        );
      }
    }

    // If no focused node and no implementing nodes, show all nodes (up to MAX_NODES)
    if (!dt.focusedNode && (!dt.implementingNodes || dt.implementingNodes.length === 0) && dt.nodes) {
      const MAX_NODES = 6;
      for (const n of dt.nodes.slice(0, MAX_NODES)) {
        const icon = this.nodeStatusIcon(n.status);
        const badge = designSpecBadge(n.designSpec, n.assessmentResult, (c, t) => theme.fg(c as any, t));
        const badgeSep = badge ? " " : "";
        const linkedId = linkDashboardFile(theme.fg("dim", n.id), n.filePath);
        const qSuffix = n.questionCount > 0 ? theme.fg("dim", ` (${n.questionCount}?)`) : "";
        const linkSuffix = n.openspecChange ? theme.fg("dim", " &") : "";
        lines.push(composePrimaryMetaLine(
          width,
          `  ${icon}${badgeSep}${badge} ${linkedId}`,
          [qSuffix + linkSuffix],
        ));
      }
      if (dt.nodes.length > MAX_NODES) {
        lines.push(
          theme.fg("dim", `  … ${dt.nodes.length - MAX_NODES} more`) +
          theme.fg("dim", " — /dashboard to expand"),
        );
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
      case "deferred": return theme.fg("dim", "⊘");
      case "seed": return theme.fg("muted", "○");
      default: return theme.fg("muted", "○");
    }
  }

  private buildOpenSpecLines(width: number): string[] {
    const theme = this.theme;
    const lines: string[] = [];
    const os = sharedState.openspec;
    if (!os || os.changes.length === 0) return lines;

    const active = os.changes.filter(c => c.stage !== "archived");
    if (active.length === 0) return lines;

    const totalDone = active.reduce((s, c) => s + c.tasksDone, 0);
    const totalAll = active.reduce((s, c) => s + c.tasksTotal, 0);
    const allComplete = totalAll > 0 && totalDone >= totalAll;
    const aggregateProgress = totalAll > 0
      ? theme.fg(allComplete ? "success" : "dim", ` ${totalDone}/${totalAll}`)
      : "";
    lines.push(
      theme.fg("accent", "◎ Implementation") +
      theme.fg("dim", `  ${active.length} change${active.length > 1 ? "s" : ""}`) +
      aggregateProgress,
    );

    const MAX_CHANGES = 5;
    for (const c of active.slice(0, MAX_CHANGES)) {
      const done = c.tasksTotal > 0 && c.tasksDone >= c.tasksTotal;
      const icon = done ? theme.fg("success", "✓") : theme.fg("dim", "◦");

      const stageColor = c.stage === "verifying" ? "warning"
        : c.stage === "implementing" ? "accent"
        : c.stage === "ready" ? "success"
        : "dim";
      const stageLabel = c.stage === "implementing" ? "impl"
        : c.stage === "verifying" ? "verify"
        : c.stage === "specified" ? "spec"
        : c.stage === "planned" ? "plan"
        : c.stage;

      // Build a single compact metadata tag: "6/14 impl" or just "impl"
      // Avoids double-separator noise from combining pre-punctuated segments.
      const meta = [
        c.tasksTotal > 0 ? theme.fg(done ? "success" : "dim", `${c.tasksDone}/${c.tasksTotal}`) : "",
        stageLabel ? theme.fg(stageColor, stageLabel) : "",
      ].filter(Boolean).join(" ");

      const linkedName = linkOpenSpecChange(c.name, c.path);
      lines.push(composePrimaryMetaLine(
        width,
        `  ${icon} ${linkedName}`,
        meta ? [meta] : [],
      ));
    }
    if (active.length > MAX_CHANGES) {
      lines.push(
        theme.fg("dim", `  … ${active.length - MAX_CHANGES} more`) +
        theme.fg("dim", " — /dashboard to expand"),
      );
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

    // ── Header: ⚡ Cleave  dispatching  2/4 ✓ ──────────────────
    const children = cl.children ?? [];
    const doneCount = children.filter(c => c.status === "done").length;
    const failCount = children.filter(c => c.status === "failed").length;
    const countSuffix = children.length > 0
      ? [
          theme.fg("dim", `${doneCount}/${children.length}`),
          ...(failCount > 0 ? [theme.fg("error", `${failCount}✕`)] : []),
        ]
      : [];
    lines.push(composePrimaryMetaLine(
      width,
      theme.fg("accent", "⚡ Cleave"),
      [theme.fg(statusColor, cl.status), ...countSuffix],
    ));

    // ── Per-child rows + activity ────────────────────────────────
    for (const child of children) {
      const isRunning = child.status === "running";
      const icon = child.status === "done"    ? theme.fg("success", "✓")
                 : child.status === "failed"  ? theme.fg("error",   "✕")
                 : isRunning                  ? theme.fg("warning",  "⟳")
                 :                              theme.fg("dim",      "○");

      const elapsedSec = isRunning && child.startedAt
        ? Math.round((Date.now() - child.startedAt) / 1000)
        : (child.elapsed != null ? Math.round(child.elapsed / 1000) : null);
      const elapsed = elapsedSec != null ? theme.fg("dim", ` ${elapsedSec}s`) : "";

      lines.push(truncateToWidth(`  ${icon} ${theme.fg("muted", child.label)}${elapsed}`, width, "…"));

      // Show last 2 ring-buffer lines for running children
      if (isRunning && child.recentLines && child.recentLines.length > 0) {
        const tail = child.recentLines.slice(-2);
        for (const l of tail) {
          lines.push(truncateToWidth(`    ${theme.fg("dim", l)}`, width, "…"));
        }
      }
    }

    return lines;
  }

  // ── Context Gauge (compact mode only) ────────────────────────

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
      const turnLabel = model.turns > 0 ? `${theme.fg("dim", `T${model.turns}`)} ` : "";
      return `${turnLabel}${unknownBar} ${theme.fg("dim", "?")}${windowStr}`;
    }

    const percent = model.percent ?? 0;

    // Severity color for non-memory context pressure
    const otherColor: ThemeColor = percent > 70 ? "error" : percent > 45 ? "warning" : "muted";

    let bar = "";
    if (model.memoryBlocks > 0) bar += theme.fg("accent", "▓".repeat(model.memoryBlocks));
    if (model.otherBlocks > 0) bar += theme.fg(otherColor, "█".repeat(model.otherBlocks));
    if (model.freeBlocks > 0) bar += theme.fg("border", "░".repeat(model.freeBlocks));

    const pctStr = `${Math.round(percent)}%`;
    const pctColored = percent > 70 ? theme.fg("error", pctStr)
      : percent > 45 ? theme.fg("warning", pctStr)
      : theme.fg("dim", pctStr);
    const windowStr = contextWindow > 0 ? theme.fg("dim", `/${formatTokens(contextWindow)}`) : "";

    const turnLabel = model.turns > 0 ? `${theme.fg("dim", `T${model.turns}`)} ` : "";
    return `${turnLabel}${bar} ${pctColored}${windowStr}`;
  }

  // ── Token cache ───────────────────────────────────────────────

  /**
   * Incrementally scan new session entries and update the cached token/cost
   * accumulators and last-seen thinking level. Safe to call repeatedly; only
   * processes entries beyond `lastEntryCount`.
   */
  private _updateTokenCache(): void {
    const ctx = this.ctxRef;
    if (!ctx) return;
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
  }

}
