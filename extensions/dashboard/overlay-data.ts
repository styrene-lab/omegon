/**
 * Pure data builders for the dashboard overlay.
 *
 * Separated from overlay.ts so they can be tested without pi-tui dependency.
 * All rendering/theme concerns are abstracted via the ThemeFn callback.
 */

import { sharedState } from "../shared-state.ts";
import type { CleaveState, DesignTreeDashboardState, OpenSpecDashboardState } from "./types.ts";

// ── Tab definitions ─────────────────────────────────────────────

export type TabId = "design" | "openspec" | "cleave";

export interface Tab {
  id: TabId;
  label: string;
  shortcut: string;
}

export const TABS: Tab[] = [
  { id: "design", label: "Design Tree", shortcut: "1" },
  { id: "openspec", label: "OpenSpec", shortcut: "2" },
  { id: "cleave", label: "Cleave", shortcut: "3" },
];

// ── Item model for navigable lists ──────────────────────────────

/** Theme coloring function — (colorName, text) → styled string */
export type ThemeFn = (color: string, text: string) => string;

export interface ListItem {
  key: string;
  depth: number;
  expandable: boolean;
  lines: (th: ThemeFn, width: number) => string[];
}

/** Maximum content lines before the footer hint row. */
export const MAX_CONTENT_LINES = 30;

// ── Status icon helper ──────────────────────────────────────────

const STATUS_ICONS: Record<string, { color: string; icon: string }> = {
  decided:   { color: "success",  icon: "●" },
  exploring: { color: "accent",   icon: "◐" },
  seed:      { color: "muted",    icon: "◌" },
  blocked:   { color: "error",    icon: "✕" },
  deferred:  { color: "warning",  icon: "◑" },
};

export function statusIcon(status: string, th: ThemeFn): string {
  const entry = STATUS_ICONS[status];
  if (entry) return th(entry.color, entry.icon);
  return th("dim", "○");
}

// ── Data Builders ───────────────────────────────────────────────

export function buildDesignItems(
  dt: DesignTreeDashboardState | undefined,
  expandedKeys: Set<string>,
): ListItem[] {
  if (!dt || dt.nodeCount === 0) return [];

  const items: ListItem[] = [];

  // Summary
  items.push({
    key: "dt-summary",
    depth: 0,
    expandable: false,
    lines: (th) => {
      const parts: string[] = [];
      if (dt.decidedCount > 0) parts.push(th("success", `${dt.decidedCount} decided`));
      if (dt.exploringCount > 0) parts.push(th("accent", `${dt.exploringCount} exploring`));
      if (dt.blockedCount > 0) parts.push(th("error", `${dt.blockedCount} blocked`));
      if (dt.openQuestionCount > 0) parts.push(th("warning", `${dt.openQuestionCount} open questions`));
      return [parts.join(" · ") || th("dim", "empty")];
    },
  });

  // Focused node
  const focused = dt.focusedNode;
  if (focused) {
    const focusedKey = `dt-focused-${focused.id}`;
    const hasQuestions = focused.questions.length > 0;
    items.push({
      key: focusedKey,
      depth: 0,
      expandable: hasQuestions,
      lines: (th) => {
        const icon = statusIcon(focused.status, th);
        const label = th("accent", " (focused)");
        return [`${icon} ${focused.title}${label}`];
      },
    });

    if (hasQuestions && expandedKeys.has(focusedKey)) {
      for (let qi = 0; qi < focused.questions.length; qi++) {
        items.push({
          key: `dt-q-${focused.id}-${qi}`,
          depth: 1,
          expandable: false,
          lines: (th) => [th("warning", `? ${focused.questions[qi]}`)],
        });
      }
    }
  }

  // Seed hint when no focused node
  if (!focused) {
    const seedCount = dt.nodeCount - dt.decidedCount - dt.exploringCount - dt.blockedCount;
    if (seedCount > 0) {
      items.push({
        key: "dt-seeds",
        depth: 0,
        expandable: false,
        lines: (th) => [th("muted", `${seedCount} seed${seedCount > 1 ? "s" : ""} — use /design focus to explore`)],
      });
    }
  }

  return items;
}

export function buildOpenSpecItems(
  os: OpenSpecDashboardState | undefined,
  expandedKeys: Set<string>,
): ListItem[] {
  if (!os || os.changes.length === 0) return [];

  const items: ListItem[] = [];

  items.push({
    key: "os-summary",
    depth: 0,
    expandable: false,
    lines: (th) => [th("dim", `${os.changes.length} active change${os.changes.length > 1 ? "s" : ""}`)],
  });

  for (const change of os.changes) {
    const done = change.tasksTotal > 0 && change.tasksDone >= change.tasksTotal;
    const hasDetails = change.stage !== undefined || change.tasksTotal > 0;
    const key = `os-change-${change.name}`;

    items.push({
      key,
      depth: 0,
      expandable: hasDetails,
      lines: (th) => {
        const icon = done ? th("success", "✓") : th("dim", "◦");
        const progress = change.tasksTotal > 0
          ? th(done ? "success" : "dim", ` ${change.tasksDone}/${change.tasksTotal}`)
          : "";
        return [`${icon} ${change.name}${progress}`];
      },
    });

    if (hasDetails && expandedKeys.has(key)) {
      if (change.stage) {
        items.push({
          key: `os-stage-${change.name}`,
          depth: 1,
          expandable: false,
          lines: (th) => [th("dim", `stage: ${change.stage}`)],
        });
      }
      if (change.tasksTotal > 0) {
        const pct = Math.round((change.tasksDone / change.tasksTotal) * 100);
        items.push({
          key: `os-progress-${change.name}`,
          depth: 1,
          expandable: false,
          lines: (th) => {
            const barW = 20;
            const filled = Math.round((change.tasksDone / change.tasksTotal) * barW);
            const bar = th("success", "█".repeat(filled)) + th("dim", "░".repeat(barW - filled));
            return [`${bar} ${pct}%`];
          },
        });
      }
    }
  }

  return items;
}

export function buildCleaveItems(
  cl: CleaveState | undefined,
  expandedKeys: Set<string>,
): ListItem[] {
  if (!cl) return [];

  const items: ListItem[] = [];

  const statusColorMap: Record<string, string> = {
    done: "success", failed: "error", idle: "dim",
  };
  const color = statusColorMap[cl.status] ?? "warning";

  items.push({
    key: "cl-status",
    depth: 0,
    expandable: false,
    lines: (th) => {
      const runLabel = cl.runId ? th("dim", ` (${cl.runId})`) : "";
      return [th(color, cl.status) + runLabel];
    },
  });

  if (cl.children && cl.children.length > 0) {
    const doneCount = cl.children.filter((c) => c.status === "done").length;
    const failCount = cl.children.filter((c) => c.status === "failed").length;
    const runCount = cl.children.filter((c) => c.status === "running").length;

    items.push({
      key: "cl-summary",
      depth: 0,
      expandable: false,
      lines: (th) => {
        const parts: string[] = [];
        parts.push(`${cl.children!.length} children`);
        if (doneCount > 0) parts.push(th("success", `${doneCount} ✓`));
        if (runCount > 0) parts.push(th("warning", `${runCount} ⟳`));
        if (failCount > 0) parts.push(th("error", `${failCount} ✕`));
        return [parts.join("  ")];
      },
    });

    for (const child of cl.children) {
      const key = `cl-child-${child.label}`;
      const hasElapsed = child.elapsed !== undefined;

      items.push({
        key,
        depth: 0,
        expandable: hasElapsed,
        lines: (th) => {
          const icon = child.status === "done" ? th("success", "✓")
            : child.status === "failed" ? th("error", "✕")
            : child.status === "running" ? th("warning", "⟳")
            : th("dim", "○");
          return [`${icon} ${child.label}`];
        },
      });

      if (hasElapsed && expandedKeys.has(key)) {
        items.push({
          key: `cl-elapsed-${child.label}`,
          depth: 1,
          expandable: false,
          lines: (th) => {
            const secs = child.elapsed ?? 0;
            const m = Math.floor(secs / 60);
            const s = Math.round(secs % 60);
            const elapsed = m > 0 ? `${m}m ${s}s` : `${s}s`;
            return [th("dim", `elapsed: ${elapsed}`)];
          },
        });
      }
    }
  }

  return items;
}

// ── State management ────────────────────────────────────────────

export function rebuildItems(
  activeTab: TabId,
  expandedKeys: Set<string>,
): ListItem[] {
  switch (activeTab) {
    case "design":
      return buildDesignItems(sharedState.designTree, expandedKeys);
    case "openspec":
      return buildOpenSpecItems(sharedState.openspec, expandedKeys);
    case "cleave":
      return buildCleaveItems(sharedState.cleave, expandedKeys);
  }
}

export function clampIndex(selectedIndex: number, itemCount: number): number {
  if (itemCount === 0) return 0;
  if (selectedIndex >= itemCount) return itemCount - 1;
  return selectedIndex;
}
