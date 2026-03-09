/**
 * Dashboard interactive overlay (Layer 2).
 *
 * Right-anchored sidepanel with three tabs:
 *   [1] Design Tree — node list with status icons, expand to show questions
 *   [2] OpenSpec    — change list with stage/progress
 *   [3] Cleave      — dispatch children with status/elapsed
 *
 * Keyboard:
 *   Tab / 1-3    — switch tabs
 *   ↑/↓          — navigate items
 *   Enter/→      — expand/collapse item
 *   ←            — collapse expanded item
 *   Esc / ctrl+c — close overlay
 *
 * Reads sharedState for all data. Subscribes to dashboard:update for live refresh.
 */

import { spawn } from "node:child_process";
import type { ExtensionContext } from "@mariozechner/pi-coding-agent";
import type { Theme } from "@mariozechner/pi-coding-agent";
import type { TUI } from "@mariozechner/pi-tui";
import { matchesKey, truncateToWidth, visibleWidth } from "@mariozechner/pi-tui";
import { DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import {
  TABS,
  MAX_CONTENT_LINES,
  rebuildItems,
  clampIndex,
  type TabId,
  type ListItem,
} from "./overlay-data.ts";

// ── Overlay Component ───────────────────────────────────────────

export class DashboardOverlay {
  private tui: TUI;
  private theme: Theme;
  private done: (result: void) => void;

  private activeTab: TabId = "design";
  private selectedIndex = 0;
  private flatItems: ListItem[] = [];
  private expandedKeys = new Set<string>();
  private statusMessage: string | null = null;

  /** Event unsubscribe handle for live refresh. */
  private unsubscribe: (() => void) | null = null;

  constructor(tui: TUI, theme: Theme, done: (result: void) => void) {
    this.tui = tui;
    this.theme = theme;
    this.done = done;
    this.rebuild();
  }

  /** Attach to the pi event bus for live data refresh while overlay is open. */
  setEventBus(events: { on(event: string, handler: (data: unknown) => void): () => void }): void {
    this.unsubscribe = events.on(DASHBOARD_UPDATE_EVENT, () => {
      this.rebuild();
      this.tui.requestRender();
    });
  }

  private selectFirstOpenableItem(): void {
    const firstOpenable = this.flatItems.findIndex((item) => !!item.openUri);
    if (firstOpenable >= 0) {
      this.selectedIndex = firstOpenable;
    }
  }

  private openSelectedItem(): void {
    const item = this.flatItems[this.selectedIndex];
    if (!item?.openUri) {
      this.statusMessage = "Selected row has nothing to open";
      this.tui.requestRender();
      return;
    }

    this.statusMessage = "Opening selected item…";
    this.tui.requestRender();

    try {
      if (process.platform === "darwin") {
        spawn("open", [item.openUri], { stdio: "ignore", detached: true }).unref();
      } else if (process.platform === "win32") {
        spawn("cmd", ["/c", "start", "", item.openUri], { stdio: "ignore", detached: true }).unref();
      } else {
        spawn("xdg-open", [item.openUri], { stdio: "ignore", detached: true }).unref();
      }
    } catch {
      this.statusMessage = "Open failed";
      this.tui.requestRender();
      // Best effort only; clickable OSC 8 links remain the primary path.
    }
  }

  // ── Keyboard handling ───────────────────────────────────────────

  handleInput(data: string): void {
    if (matchesKey(data, "escape") || matchesKey(data, "ctrl+c")) {
      this.done();
      return;
    }

    // Tab switching
    if (matchesKey(data, "tab")) {
      const idx = TABS.findIndex((t) => t.id === this.activeTab);
      this.activeTab = TABS[(idx + 1) % TABS.length]!.id;
      this.selectedIndex = 0;
      this.statusMessage = null;
      this.rebuild();
      this.selectFirstOpenableItem();
      this.tui.requestRender();
      return;
    }

    for (const tab of TABS) {
      if (data === tab.shortcut) {
        this.activeTab = tab.id;
        this.selectedIndex = 0;
        this.statusMessage = null;
        this.rebuild();
        this.selectFirstOpenableItem();
        this.tui.requestRender();
        return;
      }
    }

    // Navigation — guard empty list
    if (this.flatItems.length === 0) return;

    if (matchesKey(data, "up")) {
      this.selectedIndex = Math.max(0, this.selectedIndex - 1);
      this.statusMessage = null;
      this.tui.requestRender();
      return;
    }
    if (matchesKey(data, "down")) {
      this.selectedIndex = Math.min(this.flatItems.length - 1, this.selectedIndex + 1);
      this.statusMessage = null;
      this.tui.requestRender();
      return;
    }

    // Expand/collapse
    if (matchesKey(data, "return") || matchesKey(data, "right")) {
      const item = this.flatItems[this.selectedIndex];
      if (item?.expandable) {
        if (this.expandedKeys.has(item.key)) {
          this.expandedKeys.delete(item.key);
        } else {
          this.expandedKeys.add(item.key);
        }
        this.statusMessage = null;
        this.rebuild();
        this.tui.requestRender();
      } else if (item?.openUri) {
        this.openSelectedItem();
      }
      return;
    }

    if (matchesKey(data, "left")) {
      const item = this.flatItems[this.selectedIndex];
      if (item && this.expandedKeys.has(item.key)) {
        this.expandedKeys.delete(item.key);
        this.statusMessage = null;
        this.rebuild();
        this.tui.requestRender();
      }
      return;
    }

    if (data === "o" || data === "O") {
      this.openSelectedItem();
      return;
    }
  }

  // ── Rendering ─────────────────────────────────────────────────

  render(width: number): string[] {
    const th = this.theme;
    const innerW = Math.max(1, width - 2);
    const border = (c: string) => th.fg("border", c);
    const pad = (s: string) => truncateToWidth(s, innerW, "…", true);
    const lines: string[] = [];

    // Top border with title
    const title = " Dashboard ";
    const titleW = visibleWidth(title);
    const topLeft = "─".repeat(Math.floor((innerW - titleW) / 2));
    const topRight = "─".repeat(Math.max(0, innerW - titleW - topLeft.length));
    lines.push(border("╭" + topLeft) + th.fg("accent", title) + border(topRight + "╮"));

    // Tab bar
    const tabParts: string[] = [];
    for (const tab of TABS) {
      if (tab.id === this.activeTab) {
        tabParts.push(th.fg("accent", `[${tab.shortcut}] ${tab.label}`));
      } else {
        tabParts.push(th.fg("dim", `[${tab.shortcut}] ${tab.label}`));
      }
    }
    lines.push(border("│") + pad(" " + tabParts.join("  ")) + border("│"));
    lines.push(border("├" + "─".repeat(innerW) + "┤"));

    // Content area (capped to prevent maxHeight from eating the footer)
    const contentLines = this.renderContent(innerW).slice(0, MAX_CONTENT_LINES);
    if (contentLines.length === 0) {
      lines.push(border("│") + pad(th.fg("dim", " (no data)")) + border("│"));
    } else {
      for (const cl of contentLines) {
        lines.push(border("│") + pad(cl) + border("│"));
      }
    }

    // Footer with key hints
    lines.push(border("├" + "─".repeat(innerW) + "┤"));
    const footerPrimary = this.statusMessage
      ? th.fg("warning", ` ${this.statusMessage}`)
      : th.fg("dim", " ↵/o open selected item  ↑↓ navigate  ←→ expand/collapse");
    lines.push(border("│") + pad(footerPrimary) + border("│"));
    lines.push(border("│") + pad(th.fg("dim", " Tab switch  Esc close  items with ↗ are openable")) + border("│"));
    lines.push(border("╰" + "─".repeat(innerW) + "╯"));

    return lines;
  }

  private renderContent(innerW: number): string[] {
    const th = this.theme;
    const thFn = (color: string, text: string) => th.fg(color as any, text);
    const lines: string[] = [];

    for (let i = 0; i < this.flatItems.length; i++) {
      const item = this.flatItems[i]!;
      const isSelected = i === this.selectedIndex;
      const indent = "  ".repeat(item.depth);
      const cursor = isSelected ? th.fg("accent", "→ ") : "  ";

      // Expand indicator
      let expandIcon = "  ";
      if (item.expandable) {
        expandIcon = this.expandedKeys.has(item.key)
          ? th.fg("dim", "▾ ")
          : th.fg("dim", "▸ ");
      }

      const itemLines = item.lines(thFn, innerW - 4 - item.depth * 2);
      const openMarker = item.openUri ? th.fg("accent", "↗ ") : "";
      if (itemLines.length > 0) {
        lines.push(`${cursor}${indent}${expandIcon}${openMarker}${itemLines[0]}`);
        for (let j = 1; j < itemLines.length; j++) {
          lines.push(`  ${indent}  ${itemLines[j]}`);
        }
      }
    }

    return lines;
  }

  // ── State ─────────────────────────────────────────────────────

  private rebuild(): void {
    this.flatItems = rebuildItems(this.activeTab, this.expandedKeys);
    this.selectedIndex = clampIndex(this.selectedIndex, this.flatItems.length);
    if (this.flatItems.length > 0 && !this.flatItems[this.selectedIndex]?.openUri && this.selectedIndex === 0) {
      this.selectFirstOpenableItem();
    }
  }

  // ── Component lifecycle ───────────────────────────────────────

  invalidate(): void {}

  dispose(): void {
    if (this.unsubscribe) {
      this.unsubscribe();
      this.unsubscribe = null;
    }
  }
}

// ── Public API ──────────────────────────────────────────────────

/**
 * Show the dashboard overlay as a right-anchored sidepanel.
 * Blocks until the user presses Esc.
 */
export async function showDashboardOverlay(ctx: ExtensionContext, pi?: { events: { on(e: string, h: (data: unknown) => void): () => void } }): Promise<void> {
  await ctx.ui.custom<void>(
    (tui, theme, _kb, done) => {
      const overlay = new DashboardOverlay(tui, theme, done);
      if (pi?.events) {
        overlay.setEventBus(pi.events);
      }
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
      },
    },
  );
}
