/**
 * Custom footer component for the unified dashboard.
 *
 * Implements two rendering modes:
 *   Layer 0 (compact): persistent runtime HUD with compact telemetry cards
 *   Layer 1 (raised):  uncapped workspace + lifecycle surfaces above the HUD
 *
 * Reads sharedState for design-tree, openspec, and cleave data.
 * Reads footerData for git branch, extension statuses, provider count.
 * Reads ExtensionContext for token stats, model, context usage.
 */

import type { Component } from "@styrene-lab/pi-tui";
import type { Theme, ThemeColor } from "@styrene-lab/pi-coding-agent";
import type { ReadonlyFooterDataProvider } from "@styrene-lab/pi-coding-agent";
import type { ExtensionContext } from "@styrene-lab/pi-coding-agent";
import type { TUI } from "@styrene-lab/pi-tui";
import { truncateToWidth, visibleWidth } from "@styrene-lab/pi-tui";
import { leftRight, mergeColumns, padRight } from "./render-utils.ts";
import { buildBranchTreeLines, readLocalBranches } from "./git.ts";
import type { DashboardModelRoleSummary, DashboardState, RecoveryCooldownSummary, RecoveryDashboardState } from "./types.ts";
import { sharedState } from "../lib/shared-state.ts";
import { debug } from "../lib/debug.ts";
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
const RAISED_NARROW_WIDTH = 100;
const RAISED_WIDE_WIDTH = 160;

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

function normalizeLocalModelLabel(model: string): { canonical: string; alias?: string } {
  if (model === "devstral-small-2:24b") {
    return { canonical: "Devstral 24B", alias: "devstral-small-2:24b" };
  }
  if (model === "devstral:24b") {
    return { canonical: "Devstral 24B" };
  }
  return { canonical: model };
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
    // Compact mode is the persistent runtime HUD. Raised mode should reveal the
    // work surfaces above it, not replace it with a different footer grammar.
    return this.buildFooterZone(width, width, true);
  }

  // ── Raised Mode (Layer 1) ─────────────────────────────────────

  private renderRaised(width: number): string[] {
    if (width < RAISED_NARROW_WIDTH) return this.renderRaisedNarrow(width);
    if (width < RAISED_WIDE_WIDTH) return this.renderRaisedMedium(width);
    return this.renderRaisedWide(width);
  }

  private buildRaisedHeaderSummary(width: number): string {
    const theme = this.theme;
    const summary: string[] = [];
    const dt = sharedState.designTree;
    const os = sharedState.openspec;
    const cl = sharedState.cleave;

    if (dt) {
      const parts: string[] = [];
      if (dt.decidedCount > 0) parts.push(theme.fg("success", `${dt.decidedCount} decided`));
      if (dt.implementingCount > 0) parts.push(theme.fg("accent", `${dt.implementingCount} implementing`));
      if (dt.exploringCount > 0) parts.push(theme.fg("muted", `${dt.exploringCount} exploring`));
      if (dt.openQuestionCount > 0) parts.push(theme.fg("dim", `${dt.openQuestionCount}?`));
      if (parts.length > 0) summary.push(`${theme.fg("accent", "Design")} ${parts.join(theme.fg("dim", " · "))}`);
    }

    if (os) {
      const active = os.changes.filter((c) => c.stage !== "archived");
      if (active.length > 0) {
        const totalDone = active.reduce((sum, c) => sum + c.tasksDone, 0);
        const totalAll = active.reduce((sum, c) => sum + c.tasksTotal, 0);
        const progress = totalAll > 0 ? theme.fg("dim", `${totalDone}/${totalAll}`) : theme.fg("dim", `${active.length}`);
        summary.push(`${theme.fg("accent", "Impl")} ${progress}`);
      }
    }

    if (cl && cl.status !== "idle") {
      const children = cl.children ?? [];
      const doneCount = children.filter((c) => c.status === "done").length;
      const failCount = children.filter((c) => c.status === "failed").length;
      const parts = [theme.fg(cl.status === "done" ? "success" : cl.status === "failed" ? "error" : "warning", cl.status)];
      if (children.length > 0) parts.push(theme.fg("dim", `${doneCount}/${children.length}`));
      if (failCount > 0) parts.push(theme.fg("error", `${failCount}✕`));
      summary.push(`${theme.fg("accent", "Cleave")} ${parts.join(theme.fg("dim", " · "))}`);
    }

    return joinPrioritySegments(width, summary.map((text) => ({ text, priority: "low" as const })), "   ");
  }

  private buildRaisedBodySeparator(width: number): string {
    return this.theme.fg("dim", BOX.h.repeat(Math.max(0, width)));
  }

  private buildRaisedTopLine(topLine: string, innerWidth: number): string {
    const summaryWidth = Math.max(0, innerWidth - visibleWidth(topLine) - 1);
    const headerSummary = this.buildRaisedHeaderSummary(summaryWidth);
    return headerSummary ? leftRight(topLine, headerSummary, innerWidth - 1) : topLine;
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
   * Narrow layout for terminals under 100 cols.
   * Lifecycle/work stays above; lower dashboard uses stacked summary cards.
   */
  private renderRaisedNarrow(width: number): string[] {
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
    return this.renderBoxed(contentLines, this.buildFooterZone(innerWidth, width), topLine, width);
  }

  /**
   * Medium layout (100–139 cols) — two-column work area with compact summary cards below.
   */
  private renderRaisedMedium(width: number): string[] {
    const innerWidth = width - 4;
    const preferredLeftWidth = Math.floor((innerWidth - 1) * 0.6);
    const preferredRightWidth = innerWidth - preferredLeftWidth - 1;
    const colDivider = this.theme.fg("dim", BOX.v);

    const branchLines = this.buildBranchTree(innerWidth);
    const [topLine = "", ...extraBranchLines] = branchLines;
    const alignedBranchLines = extraBranchLines.map((l) => " " + l);

    const rightLines = [
      ...this.buildOpenSpecLines(preferredRightWidth),
      ...this.buildCleaveLines(preferredRightWidth),
      ...this.buildRecoveryLines(preferredRightWidth),
    ];
    const useRail = rightLines.length > 0;
    const leftLines = this.buildDesignTreeLines(useRail ? preferredLeftWidth : innerWidth);

    const contentLines: string[] = [
      ...alignedBranchLines,
      this.buildRaisedBodySeparator(innerWidth),
      ...(useRail
        ? mergeColumns(leftLines, rightLines, preferredLeftWidth, preferredRightWidth, colDivider)
        : leftLines),
    ];

    return this.renderBoxed(
      contentLines,
      this.buildFooterZone(innerWidth, width),
      this.buildRaisedTopLine(topLine, innerWidth),
      width,
    );
  }

  /**
   * Wide layout (140+ cols) prioritizes a design-dominant main workspace with a
   * narrower contextual rail for implementation, cleave, and recovery state.
   */
  private renderRaisedWide(width: number): string[] {
    const innerWidth = width - 4;
    const preferredLeftWidth = Math.floor((innerWidth - 1) * 0.72);
    const preferredRightWidth = innerWidth - preferredLeftWidth - 1;
    const colDivider = this.theme.fg("dim", BOX.v);

    const branchLines = this.buildBranchTree(innerWidth);
    const [topLine = "", ...extraBranchLines] = branchLines;
    const alignedBranchLines = extraBranchLines.map((l) => " " + l);

    const rightLines = [
      ...this.buildOpenSpecLines(preferredRightWidth),
      ...this.buildCleaveLines(preferredRightWidth),
      ...this.buildRecoveryLines(preferredRightWidth),
    ];
    const useRail = rightLines.length > 0;
    const leftLines = this.buildDesignTreeLines(useRail ? preferredLeftWidth : innerWidth);

    const contentLines: string[] = [
      ...alignedBranchLines,
      this.buildRaisedBodySeparator(innerWidth),
      ...(useRail
        ? mergeColumns(leftLines, rightLines, preferredLeftWidth, preferredRightWidth, colDivider)
        : leftLines),
    ];

    return this.renderBoxed(
      contentLines,
      this.buildFooterZone(innerWidth),
      this.buildRaisedTopLine(topLine, innerWidth),
      width,
    );
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
   * Derive a short directive label from the active mind name.
   * "directive/my-feature" → "my-feature", null/default → null.
   */
  private getDirectiveLabel(): string | null {
    const mind = sharedState.activeMind;
    if (!mind || mind === "default") return null;
    // Strip common prefixes: "directive/", "mind/"
    const label = mind.replace(/^(?:directive|mind)\//, "");
    return label || null;
  }

  /**
   * Build directive indicator lines for the model card area.
   * Shows "▸ directive: my-feature" when a directive mind is active.
   * Returns empty array when no directive is active.
   */
  private buildDirectiveIndicatorLines(width: number): string[] {
    const label = this.getDirectiveLabel();
    if (!label) return [];
    const theme = this.theme;
    const line = `${theme.fg("warning", "▸")} ${theme.fg("dim", "directive:")} ${theme.fg("warning", label)}`;
    return [truncateToWidth(line, width, "…")];
  }

  private buildModelTopologySummaries(): DashboardModelRoleSummary[] {
    const ctx = this.ctxRef;
    const summaries: DashboardModelRoleSummary[] = [];
    const memoryStatus = this.footerData.getExtensionStatuses().get("memory") ?? "";
    const offlineStatus = this.footerData.getExtensionStatuses().get("offline-driver") ?? "";

    if (ctx?.model) {
      summaries.push({
        role: "driver",
        label: "Driver",
        model: ctx.model.id,
        source: ctx.model.provider === "local" ? "local" : "cloud",
        state: offlineStatus.includes("OFFLINE:") ? "offline" : "active",
        detail: this.footerData.getAvailableProviderCount() > 1 ? ctx.model.provider : undefined,
      });
    }

    const effort = sharedState.effort;
    if (memoryStatus || effort?.resolvedExtractionModelId) {
      const extractionModel = effort?.resolvedExtractionModelId ?? effort?.extraction ?? "?";
      const extractionLocal = extractionModel.includes(":") || effort?.extraction === "local";
      summaries.push({
        role: "extraction",
        label: "Extraction",
        model: extractionModel,
        source: extractionLocal ? "local" : "cloud",
        state: extractionLocal ? "ready" : "active",
      });
    }

    if (memoryStatus) {
      const embedMatch = memoryStatus.match(/semantic/i);
      summaries.push({
        role: "embeddings",
        label: "Embeddings",
        model: embedMatch ? "semantic retrieval" : "available",
        source: "unknown",
        state: "ready",
      });
    }

    if (offlineStatus.includes("OFFLINE:")) {
      const raw = sanitizeStatusText(offlineStatus).replace(/^.*OFFLINE:\s*/i, "");
      summaries.push({
        role: "fallback",
        label: "Fallback",
        model: raw || "local fallback",
        source: "local",
        state: "offline",
      });
    }

    return summaries;
  }

  private formatModelTopologyLine(summary: DashboardModelRoleSummary, width: number, compact = false): string {
    const theme = this.theme;
    const forceCompact = compact || width < 40;
    // In compact mode, use single-char glyphs to save space.
    const sourceBadge = forceCompact
      ? (summary.source === "local" ? theme.fg("accent", "⌂") : summary.source === "cloud" ? theme.fg("muted", "☁") : theme.fg("dim", "?"))
      : (summary.source === "local"
        ? theme.fg("accent", "local")
        : summary.source === "cloud"
          ? theme.fg("muted", "cloud")
          : theme.fg("dim", summary.source));
    const stateBadge = forceCompact
      ? ""
      : (summary.state === "active"
        ? theme.fg("success", "active")
        : summary.state === "offline"
          ? theme.fg("warning", "offline")
          : summary.state === "fallback"
            ? theme.fg("warning", "fallback")
            : theme.fg("dim", summary.state));
    const normalized = normalizeLocalModelLabel(summary.model);
    const alias = forceCompact ? "" : (normalized.alias ? theme.fg("dim", `alias ${normalized.alias}`) : "");
    const roleLabel = forceCompact ? summary.label.slice(0, 1) : summary.label;
    const primary = `${theme.fg("accent", roleLabel)} ${theme.fg("muted", normalized.canonical)}`;
    return truncateToWidth(
      composePrimaryMetaLine(
        width,
        primary,
        [sourceBadge, stateBadge, summary.detail ? theme.fg("dim", summary.detail) : "", alias].filter(Boolean),
      ),
      width,
      "…",
    );
  }

  private buildSummaryCard(title: string, lines: string[], width: number): string[] {
    if (lines.length === 0) return [];
    return [this.buildHudSectionDivider(title, width), ...lines.map((line) => truncateToWidth(`  ${line}`, width, "…"))];
  }

  private buildSummaryCardForColumn(title: string, lines: string[], columnWidth: number, contentWidth: number): string[] {
    if (lines.length === 0) return [];
    return this.buildSummaryCard(title, lines, Math.max(1, columnWidth)).map((line) => truncateToWidth(line, Math.max(1, contentWidth), "…"));
  }

  private buildFooterHintLine(width: number): string {
    const hint = this.dashState.mode === "panel"
      ? "/dashboard to close"
      : "/dash to expand  ·  /dashboard modal";
    return truncateToWidth(this.theme.fg("dim", hint), Math.max(1, width), "…");
  }

  private buildFooterZone(width: number, totalWidth = width, compactPersistent = false): string[] {
    this._updateTokenCache();

    const buildCards = (cardWidth: number) => {
      const safeWidth = Math.max(1, cardWidth);
      return {
        contextCard: this.buildSummaryCard("context", this.buildHudContextLines(Math.max(1, safeWidth - 2)).map((l) => l.trimStart()), safeWidth),
        modelCard: this.buildSummaryCard(
          "models",
          [
            ...this.buildDirectiveIndicatorLines(Math.max(1, safeWidth - 2)),
            ...this.buildModelTopologySummaries().map((s) => this.formatModelTopologyLine(s, Math.max(1, safeWidth - 2), safeWidth < 44)),
          ],
          safeWidth,
        ),
        memoryCard: this.buildSummaryCard("memory", (() => {
          const line = this.buildHudMemoryLine(Math.max(1, safeWidth - 2));
          return line ? [line.trimStart()] : [];
        })(), safeWidth),
        systemCard: this.buildSummaryCard("system", this.buildHudSystemLines(Math.max(1, safeWidth - 2)).map((l) => l.trimStart()), safeWidth),
        recoveryCard: this.buildSummaryCard(
          "recovery",
          (compactPersistent
            ? this.buildRecoveryCompactLines(Math.max(1, safeWidth - 2))
            : this.buildRecoveryLines(Math.max(1, safeWidth - 2))
          ).map((l) => l.trimStart()),
          safeWidth,
        ),
      };
    };

    const { contextCard, modelCard, memoryCard, systemCard, recoveryCard } = buildCards(width);
    const footerHintLine = compactPersistent ? this.buildFooterHintLine(width) : undefined;

    if (width < RAISED_NARROW_WIDTH) {
      return [
        ...contextCard,
        ...modelCard,
        ...memoryCard,
        ...(recoveryCard.length > 0 ? recoveryCard : []),
        ...systemCard,
        ...(footerHintLine ? [footerHintLine] : []),
      ];
    }

    if (width < RAISED_WIDE_WIDTH) {
      // Two columns: context+memory on left, models+system on right
      const left = [...contextCard, ...memoryCard];
      const right = [...modelCard, ...(recoveryCard.length > 0 ? recoveryCard : []), ...systemCard];
      const colWidth = Math.floor((width - 1) / 2);
      const rightWidth = width - colWidth - 1;
      const merged = mergeColumns(left, right, colWidth, rightWidth, this.theme.fg("dim", BOX.v));
      return footerHintLine ? [...merged, footerHintLine] : merged;
    }

    // Wide/full-screen: keep the final memory|system divider running all the way
    // to the box base, and align that divider with the raised body split above.
    // In persistent compact mode there is no upper split, so use a balanced
    // four-column HUD instead of the raised work-area proportions.
    const divider = this.theme.fg("dim", BOX.v);
    const mainSplit = compactPersistent
      ? Math.floor((totalWidth - 1) * 0.75)
      : Math.floor((totalWidth - 1) * 0.72);
    const leftTelemetryWidth = Math.max(3, mainSplit - 2);
    const rightTelemetryWidth = Math.max(1, totalWidth - mainSplit - 1);

    const col1W = Math.max(1, Math.floor(leftTelemetryWidth * 0.35));
    const col2W = Math.max(1, Math.floor(leftTelemetryWidth * 0.40));
    const col3W = Math.max(1, leftTelemetryWidth - col1W - col2W);
    const col4W = rightTelemetryWidth;
    const wideCards = {
      contextCard: this.buildSummaryCardForColumn("context", this.buildHudContextLines(Math.max(1, col1W - 2)).map((l) => l.trimStart()), col1W, col1W),
      modelCard: this.buildSummaryCardForColumn(
        "models",
        [
          ...this.buildDirectiveIndicatorLines(Math.max(1, col2W - 2)),
          ...this.buildModelTopologySummaries().map((s) => this.formatModelTopologyLine(s, Math.max(1, col2W - 2), col2W < 44)),
        ],
        col2W,
        col2W,
      ),
      memoryCard: this.buildSummaryCardForColumn("memory", (() => {
        const line = this.buildHudMemoryLine(Math.max(1, col3W - 2));
        return line ? [line.trimStart()] : [];
      })(), col3W, col3W),
      systemCard: this.buildSummaryCardForColumn("system", this.buildHudSystemLines(Math.max(1, col4W - 2)).map((l) => l.trimStart()), col4W, col4W),
      recoveryCard: this.buildSummaryCardForColumn(
        "recovery",
        (compactPersistent
          ? this.buildRecoveryCompactLines(Math.max(1, col4W - 2))
          : this.buildRecoveryLines(Math.max(1, col4W - 2))
        ).map((l) => l.trimStart()),
        col4W,
        col4W,
      ),
    };
    const col1 = wideCards.contextCard;
    const col2 = wideCards.modelCard;
    const col3 = wideCards.memoryCard;
    const col4 = [...(wideCards.recoveryCard.length > 0 ? wideCards.recoveryCard : []), ...wideCards.systemCard];

    const rows = Math.max(col1.length, col2.length, col3.length, col4.length);
    const merged: string[] = [];
    for (let i = 0; i < rows; i++) {
      const cell1 = i < col1.length
        ? padRight(truncateToWidth(col1[i], col1W, "…"), col1W)
        : " ".repeat(col1W);
      const cell2 = i < col2.length
        ? padRight(truncateToWidth(col2[i], col2W, "…"), col2W)
        : " ".repeat(col2W);
      const cell3 = i < col3.length
        ? padRight(truncateToWidth(col3[i], col3W, "…"), col3W)
        : " ".repeat(col3W);
      const cell4 = i < col4.length
        ? padRight(truncateToWidth(col4[i], col4W, "…"), col4W)
        : " ".repeat(col4W);
      merged.push(`${cell1}${divider}${cell2}${divider}${cell3}${divider}${cell4}`);
    }
    return footerHintLine ? [...merged, footerHintLine] : merged;
  }

  // ── Section builders (shared by stacked + wide layouts) ───────

  private buildRecoveryCompactLines(width: number): string[] {
    const theme = this.theme;
    const recovery = getRecoveryState();
    if (!recovery) return [];

    // Auto-suppress stale recovery notices in compact mode — they outlive their
    // usefulness quickly and crowd out model/driver/thinking info.
    if (Date.now() - recovery.timestamp > RECOVERY_STALE_MS) return [];

    // Collapse non-actionable observational notices in compact mode.
    if (recovery.action === "observe") return [];

    // Past-tense labels for auto-handled actions so they read as status, not
    // directives. 'escalate' is the only case where the operator must act.
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

    // Compact mode stays terse: badge + cooldown, with an explicit command hint
    // only when operator intervention is required.
    const cooldown = summarizeCooldown(recovery.cooldowns);
    const escalateHint = recovery.action === "escalate"
      ? theme.fg("dim", "→ /set-model-tier")
      : "";
    const icon = recovery.action === "escalate" ? "⚠" : "↺";

    const header = composePrimaryMetaLine(
      width,
      theme.fg("accent", `${icon} Recovery`) + theme.fg("dim", " · ") + theme.fg(actionColor, actionLabel),
      [],
    );
    const meta = [cooldown ? theme.fg("dim", cooldown) : "", escalateHint].filter(Boolean);
    if (meta.length === 0) return [header];

    return [
      header,
      composePrimaryMetaLine(width, "", meta),
    ];
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

    const headerParts = [theme.fg("dim", recovery.classification)];
    const lines = [composePrimaryMetaLine(
      width,
      theme.fg("accent", `${recoveryIcon} Recovery`) + theme.fg("dim", " · ") + theme.fg(actionColor, actionLabel),
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
      case "resolved": return theme.fg("success", "◉");
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
