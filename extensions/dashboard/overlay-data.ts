/**
 * Pure data builders for the dashboard overlay.
 *
 * Separated from overlay.ts so they can be tested without pi-tui dependency.
 * All rendering/theme concerns are abstracted via the ThemeFn callback.
 */

import { sharedState } from "../lib/shared-state.ts";
import type { CleaveState, DesignAssessmentResult, DesignSpecBindingState, DesignTreeDashboardState, OpenSpecDashboardState } from "./types.ts";
import type { ProviderRoutingPolicy } from "../lib/model-routing.ts";
import {
  getDashboardFileUri,
  getOpenSpecArtifactUri,
  linkDashboardFile,
  linkOpenSpecArtifact,
  linkOpenSpecChange,
} from "./uri-helper.ts";

// ── Tab definitions ─────────────────────────────────────────────

export type TabId = "design" | "openspec" | "cleave" | "system";

export interface Tab {
  id: TabId;
  label: string;
  shortcut: string;
}

export const TABS: Tab[] = [
  { id: "design",   label: "Design Tree", shortcut: "1" },
  { id: "openspec", label: "Implementation", shortcut: "2" },
  { id: "cleave",   label: "Cleave",      shortcut: "3" },
  { id: "system",   label: "System",      shortcut: "4" },
];

// ── Item model for navigable lists ──────────────────────────────

/** Theme coloring function — (colorName, text) → styled string */
export type ThemeFn = (color: string, text: string) => string;

export interface ListItem {
  key: string;
  depth: number;
  expandable: boolean;
  openUri?: string;
  lines: (th: ThemeFn, width: number) => string[];
}

/** Maximum content lines before the footer hint row. */
export const MAX_CONTENT_LINES = 30;

// ── Status icon helper ──────────────────────────────────────────

const STATUS_ICONS: Record<string, { color: string; icon: string }> = {
  decided:      { color: "success",  icon: "●" },
  exploring:    { color: "accent",   icon: "◐" },
  seed:         { color: "muted",    icon: "◌" },
  blocked:      { color: "error",    icon: "✕" },
  deferred:     { color: "warning",  icon: "◑" },
  implementing: { color: "warning",  icon: "⟳" },
  implemented:  { color: "success",  icon: "✓" },
};

export function statusIcon(status: string, th: ThemeFn): string {
  const entry = STATUS_ICONS[status];
  if (entry) return th(entry.color, entry.icon);
  return th("dim", "○");
}

// ── Design spec badge helper ────────────────────────────────────

/**
 * Returns a colored badge character based on spec binding state and last
 * assessment outcome.
 *
 * ✓  active spec + passed assessment (success)
 * ✦  active spec + no assessment yet, or needs spec (warning)
 * ◐  active spec + ambiguous/reopen assessment (accent)
 * ●  archived spec (muted)
 * '' no binding (seed/unlinked node)
 */
export function designSpecBadge(
  binding: DesignSpecBindingState | undefined,
  assessmentResult: DesignAssessmentResult | null | undefined,
  th: ThemeFn,
): string {
  if (!binding || binding.missing) return "";
  if (binding.archived) return th("muted", "●");
  if (!binding.active) return "";
  // active binding
  if (!assessmentResult) return th("warning", "✦");
  switch (assessmentResult.outcome) {
    case "pass":     return th("success", "✓");
    case "reopen":   return th("accent", "◐");
    case "ambiguous": return th("accent", "◐");
    default:         return th("warning", "✦");
  }
}

// ── Data Builders ───────────────────────────────────────────────

export function buildDesignItems(
  dt: DesignTreeDashboardState | undefined,
  expandedKeys: Set<string>,
): ListItem[] {
  if (!dt) return [];

  const items: ListItem[] = [];

  // Pipeline funnel row (collapsible — expand to see per-bucket detail lines)
  if (dt.designPipeline) {
    const p = dt.designPipeline;
    const pKey = "dt-pipeline";
    const isExpanded = expandedKeys.has(pKey);
    items.push({
      key: pKey,
      depth: 0,
      expandable: true,
      lines: (th) => {
        const parts: string[] = [];
        if (p.designing > 0)    parts.push(th("accent", `${p.designing} designing`));
        if (p.decided > 0)      parts.push(th("success", `${p.decided} decided`));
        if (p.implementing > 0) parts.push(th("warning", `${p.implementing} implementing`));
        if (p.done > 0)         parts.push(th("success", `${p.done} done`));
        const expandHint = isExpanded ? th("dim", " ▾") : th("dim", " ▸");
        return [th("dim", "→ ") + (parts.join(th("dim", " · ")) || th("dim", "empty pipeline")) + expandHint];
      },
    });

    if (isExpanded) {
      // Detail sub-rows for each non-zero bucket
      const buckets: Array<{ label: string; count: number; color: string }> = [
        { label: "designing",    count: p.designing,    color: "accent"  },
        { label: "decided",      count: p.decided,      color: "success" },
        { label: "implementing", count: p.implementing, color: "warning" },
        { label: "done",         count: p.done,         color: "success" },
      ];
      for (const b of buckets) {
        if (b.count === 0) continue;
        items.push({
          key: `dt-pipeline-${b.label}`,
          depth: 1,
          expandable: false,
          lines: (th) => [th(b.color, `${b.count} ${b.label}`)],
        });
      }
      if (p.needsSpec > 0) {
        items.push({
          key: "dt-pipeline-needs-spec",
          depth: 1,
          expandable: false,
          lines: (th) => [th("warning", `✦ ${p.needsSpec} need spec`)],
        });
      }
    }
  }

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
      openUri: getDashboardFileUri(focused.filePath),
      lines: (th) => {
        const icon = statusIcon(focused.status, th);
        const linkedTitle = linkDashboardFile(focused.title, focused.filePath);
        const label = th("accent", " (focused)");
        // TODO(types-and-emission): DesignTreeFocusedNode lacks designSpec/assessmentResult fields.
        // Once the sibling task adds those fields, call designSpecBadge here for the focused node.
        return [`${icon} ${linkedTitle}${label}`];
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

  // All nodes list
  if (dt.nodes && dt.nodes.length > 0) {
    for (const node of dt.nodes) {
      // Skip focused node — already shown above
      if (focused && node.id === focused.id) continue;

      items.push({
        key: `dt-node-${node.id}`,
        depth: 0,
        expandable: false,
        openUri: getDashboardFileUri(node.filePath),
        lines: (th) => {
          const icon = statusIcon(node.status, th);
          const badge = designSpecBadge(node.designSpec, node.assessmentResult, th);
          // spacer: "icon badge title" when badge present, "icon title" when absent
          const spacer = badge ? ` ${badge} ` : " ";
          const linkedTitle = linkDashboardFile(node.title, node.filePath);
          const qLabel = node.questionCount > 0
            ? th("warning", ` (${node.questionCount}?)`)
            : "";
          const linkSuffix = node.openspecChange
            ? th("dim", " &")
            : "";
          return [`${icon}${spacer}${linkedTitle}${qLabel}${linkSuffix}`];
        },
      });
    }
  } else if (!focused) {
    // Seed hint when no nodes at all
    items.push({
      key: "dt-empty",
      depth: 0,
      expandable: false,
      lines: (th) => [th("muted", "No design nodes — use /design new <id> <title>")],
    });
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

    // Check if any design node links to this change (for & suffix)
    const linkedToDesign = sharedState.designTree?.nodes?.some(
      (n) => n.openspecChange === change.name,
    ) ?? false;

    items.push({
      key,
      depth: 0,
      expandable: hasDetails,
      openUri: getOpenSpecArtifactUri(change.path, "proposal"),
      lines: (th) => {
        const icon = done ? th("success", "✓") : th("dim", "◦");
        const linkedName = linkOpenSpecChange(change.name, change.path);
        const progress = change.tasksTotal > 0
          ? th(done ? "success" : "dim", ` ${change.tasksDone}/${change.tasksTotal}`)
          : "";
        const designLink = linkedToDesign ? th("dim", " &") : "";
        return [`${icon} ${linkedName}${progress}${designLink}`];
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

      const artifactOrder: Array<"proposal" | "design" | "tasks"> = ["proposal", "design", "tasks"];
      for (const artifact of artifactOrder) {
        if (!change.artifacts?.includes(artifact)) continue;
        items.push({
          key: `os-artifact-${change.name}-${artifact}`,
          depth: 1,
          expandable: false,
          openUri: getOpenSpecArtifactUri(change.path, artifact),
          lines: (th) => {
            const linked = linkOpenSpecArtifact(artifact, change.path, artifact);
            return [th("dim", "file: ") + linked];
          },
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
      const isRunning = child.status === "running";
      const hasDoneElapsed = child.elapsed !== undefined && !isRunning;
      // Running children: compute live elapsed from startedAt; done: use stored elapsed
      const liveElapsedSec = isRunning && child.startedAt
        ? Math.round((Date.now() - child.startedAt) / 1000)
        : (child.elapsed ?? 0);
      const fmtSecs = (secs: number) => {
        const m = Math.floor(secs / 60);
        const s = secs % 60;
        return m > 0 ? `${m}m ${s}s` : `${s}s`;
      };

      items.push({
        key,
        depth: 0,
        expandable: hasDoneElapsed,
        lines: (th) => {
          const icon = child.status === "done" ? th("success", "✓")
            : child.status === "failed" ? th("error", "✕")
            : child.status === "running" ? th("warning", "⟳")
            : th("dim", "○");
          const elapsedBadge = isRunning
            ? th("dim", ` ${fmtSecs(liveElapsedSec)}`)
            : "";
          return [`${icon} ${child.label}${elapsedBadge}`];
        },
      });

      // Running: show last 3 ring-buffer lines (falls back to lastLine)
      const activityLines = child.recentLines?.slice(-3) ?? (child.lastLine ? [child.lastLine] : []);
      if (isRunning && activityLines.length > 0) {
        items.push({
          key: `cl-activity-${child.label}`,
          depth: 1,
          expandable: false,
          lines: (th) => activityLines.map(l => th("dim", l.slice(0, 72))),
        });
      }

      // Done: show elapsed when expanded
      if (hasDoneElapsed && expandedKeys.has(key)) {
        items.push({
          key: `cl-elapsed-${child.label}`,
          depth: 1,
          expandable: false,
          lines: (th) => [th("dim", `elapsed: ${fmtSecs(child.elapsed!)}`)],
        });
      }
    }
  }

  return items;
}

// ── System / Config tab ─────────────────────────────────────────

/** Omegon env vars surfaced in the System tab */
const PI_ENV_VARS: Array<{ key: string; description: string }> = [
  { key: "PI_CHILD",               description: "Non-empty when running as cleave child" },
  { key: "PI_OFFLINE",             description: "Force offline/local-only mode" },
  { key: "PI_SKIP_VERSION_CHECK",  description: "Disable GitHub release polling" },
  { key: "PI_DEBUG",               description: "Enable verbose debug logging" },
  { key: "PI_LOG_LEVEL",           description: "Log verbosity (debug|info|warn|error)" },
  { key: "PI_PROVIDER",            description: "Override active provider (anthropic|openai|local)" },
  { key: "PI_MODEL",               description: "Override active model ID" },
  { key: "ANTHROPIC_API_KEY",      description: "Anthropic API key" },
  { key: "OPENAI_API_KEY",         description: "OpenAI API key" },
  { key: "OLLAMA_HOST",            description: "Ollama server URL (default: localhost:11434)" },
];

function fmtBool(val: boolean, th: ThemeFn): string {
  return val ? th("success", "yes") : th("dim", "no");
}

function fmtValue(val: string | undefined, th: ThemeFn): string {
  if (!val) return th("dim", "(unset)");
  if (val.length > 40) return th("muted", val.slice(0, 37) + "…");
  return th("accent", val);
}

function routingPolicyItems(policy: ProviderRoutingPolicy, expandedKeys: Set<string>): ListItem[] {
  const key = "sys-routing";
  const items: ListItem[] = [];

  items.push({
    key,
    depth: 0,
    expandable: true,
    lines: (th) => [th("accent", "Routing Policy")],
  });

  if (!expandedKeys.has(key)) return items;

  items.push({
    key: "sys-routing-order",
    depth: 1,
    expandable: false,
    lines: (th) => [th("dim", "provider order: ") + th("muted", policy.providerOrder.join(" → "))],
  });

  if (policy.avoidProviders.length > 0) {
    items.push({
      key: "sys-routing-avoid",
      depth: 1,
      expandable: false,
      lines: (th) => [th("dim", "avoid: ") + th("warning", policy.avoidProviders.join(", "))],
    });
  }

  items.push({
    key: "sys-routing-cheap",
    depth: 1,
    expandable: false,
    lines: (th) => [th("dim", "cheap cloud over local: ") + fmtBool(policy.cheapCloudPreferredOverLocal, th)],
  });

  items.push({
    key: "sys-routing-preflight",
    depth: 1,
    expandable: false,
    lines: (th) => [th("dim", "preflight for large runs: ") + fmtBool(policy.requirePreflightForLargeRuns, th)],
  });

  if (policy.notes) {
    items.push({
      key: "sys-routing-notes",
      depth: 1,
      expandable: false,
      lines: (th) => [th("dim", "notes: ") + th("muted", policy.notes!)],
    });
  }

  return items;
}

function effortItems(effort: any | undefined, expandedKeys: Set<string>): ListItem[] {
  if (!effort) return [];
  const key = "sys-effort";
  const items: ListItem[] = [];

  items.push({
    key,
    depth: 0,
    expandable: true,
    lines: (th) => {
      const name = effort.name ?? `Level ${effort.level ?? "?"}`;
      const glyphs: Record<number, string> = { 1:"α", 2:"β", 3:"γ", 4:"δ", 5:"ε", 6:"ζ", 7:"ω" };
      const glyph = glyphs[effort.level as number] ?? "·";
      return [th("accent", "Effort Tier") + th("dim", ": ") + th("accent", glyph) + th("dim", " ") + th("muted", name)];
    },
  });

  if (!expandedKeys.has(key)) return items;

  const fields: Array<[string, string]> = [
    ["level",    String(effort.level ?? "?")],
    ["driver",   effort.driverModel ?? "?"],
    ["extract",  effort.extractionModel ?? "?"],
    ["thinking", effort.thinkingLevel ?? "?"],
  ];
  for (const [label, val] of fields) {
    items.push({
      key: `sys-effort-${label}`,
      depth: 1,
      expandable: false,
      lines: (th) => [th("dim", `${label}: `) + th("muted", val)],
    });
  }

  return items;
}

function recoveryItems(recovery: any | undefined, expandedKeys: Set<string>): ListItem[] {
  if (!recovery) return [];
  const key = "sys-recovery";
  const items: ListItem[] = [];

  const color = recovery.escalated ? "error" : recovery.action === "retry" ? "warning" : "dim";

  items.push({
    key,
    depth: 0,
    expandable: true,
    lines: (th) => [th("accent", "Last Recovery Event") + th("dim", ": ") + th(color, recovery.action ?? "?")],
  });

  if (!expandedKeys.has(key)) return items;

  items.push({
    key: "sys-recovery-provider",
    depth: 1,
    expandable: false,
    lines: (th) => [th("dim", "provider: ") + th("muted", `${recovery.provider}/${recovery.modelId}`)],
  });
  items.push({
    key: "sys-recovery-class",
    depth: 1,
    expandable: false,
    lines: (th) => [th("dim", "class: ") + th("muted", recovery.classification ?? "?")],
  });
  if (recovery.summary) {
    items.push({
      key: "sys-recovery-summary",
      depth: 1,
      expandable: false,
      lines: (th) => [th("dim", "summary: ") + th("muted", recovery.summary)],
    });
  }

  return items;
}

function memoryItems(expandedKeys: Set<string>): ListItem[] {
  const key = "sys-memory";
  const items: ListItem[] = [];
  const metrics = sharedState.lastMemoryInjection;

  items.push({
    key,
    depth: 0,
    expandable: !!metrics,
    lines: (th) => {
      const est = sharedState.memoryTokenEstimate;
      const label = est > 0 ? th("muted", `~${est.toLocaleString()} tokens`) : th("dim", "no injection yet");
      return [th("accent", "Memory Injection") + th("dim", ": ") + label];
    },
  });

  if (!metrics || !expandedKeys.has(key)) return items;

  const mFields: Array<[string, string]> = [
    ["project facts",  String(metrics.projectFactCount ?? "?")],
    ["global facts",   String(metrics.globalFactCount ?? "?")],
    ["working memory", String(metrics.workingMemoryFactCount ?? "?")],
    ["episodes",       String(metrics.episodeCount ?? "?")],
    ["tokens ~",       String(metrics.estimatedTokens ?? "?")],
  ];
  for (const [label, val] of mFields) {
    items.push({
      key: `sys-mem-${label}`,
      depth: 1,
      expandable: false,
      lines: (th) => [th("dim", `${label}: `) + th("muted", val)],
    });
  }

  return items;
}

export function buildSystemItems(expandedKeys: Set<string>): ListItem[] {
  const items: ListItem[] = [];

  // ── Section: Environment Variables ──────────────────────────
  const envKey = "sys-env";
  items.push({
    key: envKey,
    depth: 0,
    expandable: true,
    lines: (th) => [th("accent", "Environment Variables")],
  });

  if (expandedKeys.has(envKey)) {
    for (const { key, description } of PI_ENV_VARS) {
      const raw = process.env[key];
      // Mask key values to avoid leaking secrets
      const masked = key.endsWith("_KEY") && raw
        ? raw.slice(0, 4) + "…" + raw.slice(-4)
        : raw;
      items.push({
        key: `sys-env-${key}`,
        depth: 1,
        expandable: false,
        lines: (th) => {
          const valStr = fmtValue(masked, th);
          return [`${th("dim", key + ": ")}${valStr}  ${th("muted", description)}`];
        },
      });
    }
  }

  // ── Section: Routing Policy ──────────────────────────────────
  if (sharedState.routingPolicy) {
    items.push(...routingPolicyItems(sharedState.routingPolicy, expandedKeys));
  }

  // ── Section: Effort Tier ─────────────────────────────────────
  items.push(...effortItems(sharedState.effort, expandedKeys));

  // ── Section: Memory Injection ────────────────────────────────
  items.push(...memoryItems(expandedKeys));

  // ── Section: Last Recovery Event ────────────────────────────
  items.push(...recoveryItems(sharedState.recovery, expandedKeys));

  // ── Section: Dashboard Mode ──────────────────────────────────
  items.push({
    key: "sys-mode",
    depth: 0,
    expandable: false,
    lines: (th) => [
      th("dim", "dashboard mode: ") + th("muted", sharedState.dashboardMode ?? "compact") +
      th("dim", "   turns: ") + th("muted", String(sharedState.dashboardTurns ?? 0))
    ],
  });

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
    case "system":
      return buildSystemItems(expandedKeys);
  }
}

export function clampIndex(selectedIndex: number, itemCount: number): number {
  if (itemCount === 0) return 0;
  if (selectedIndex >= itemCount) return itemCount - 1;
  return selectedIndex;
}
