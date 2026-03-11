/**
 * ControlPlaneState snapshot builder.
 *
 * Derives a versioned, JSON-serialisable snapshot from:
 *   - sharedState (live in-process data)
 *   - on-demand scans of the design-tree and OpenSpec directories
 *
 * This module is pure logic — no HTTP, no side-effects. The HTTP layer
 * calls buildControlPlaneState() on every GET /api/state request.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { sharedState } from "../shared-state.ts";
import { listChanges } from "../openspec/spec.ts";
import { scanDesignDocs } from "../design-tree/tree.ts";
import {
  SCHEMA_VERSION,
  type ControlPlaneState,
  type SessionSnapshot,
  type DashboardSnapshot,
  type DesignTreeSnapshot,
  type DesignNodeSummary,
  type OpenSpecSnapshot,
  type OpenSpecChangeSummary,
  type CleaveSnapshot,
  type ModelsSnapshot,
  type MemorySnapshot,
  type HealthSnapshot,
  type RecoverySnapshot,
  type OperatorMetadataSnapshot,
} from "./types.ts";

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Resolve the package version from the nearest package.json */
function readPiKitVersion(repoRoot: string): string {
  try {
    const pkgPath = path.join(repoRoot, "package.json");
    const raw = fs.readFileSync(pkgPath, "utf8");
    const parsed = JSON.parse(raw) as { version?: unknown };
    return typeof parsed.version === "string" ? parsed.version : "unknown";
  } catch {
    return "unknown";
  }
}

/** Attempt to read the current git branch without shelling out if possible. */
function readGitBranch(repoRoot: string): string | null {
  try {
    const headPath = path.join(repoRoot, ".git", "HEAD");
    const head = fs.readFileSync(headPath, "utf8").trim();
    const match = /^ref: refs\/heads\/(.+)$/.exec(head);
    return match ? match[1] : head.slice(0, 12); // detached HEAD → short SHA
  } catch {
    return null;
  }
}

// ── Section builders ──────────────────────────────────────────────────────────

function buildSession(repoRoot: string): SessionSnapshot {
  return {
    capturedAt: new Date().toISOString(),
    piKitVersion: readPiKitVersion(repoRoot),
    repoRoot,
    gitBranch: readGitBranch(repoRoot),
  };
}

/**
 * Recovery actions that require operator attention — the web UI should treat
 * these as "actionable" and may surface prompts, alerts, or indicators.
 */
const ACTIONABLE_RECOVERY_ACTIONS = new Set([
  "escalate",
  "retry",
  "switch_candidate",
  "switch_offline",
  "cooldown",
]);

function buildDashboard(): DashboardSnapshot {

  let recovery: RecoverySnapshot | null = null;
  const r = sharedState.recovery;
  if (r) {
    recovery = {
      provider: r.provider,
      modelId: r.modelId,
      classification: r.classification,
      summary: r.summary,
      action: r.action,
      retryCount: r.retryCount ?? null,
      timestamp: r.timestamp,
      escalated: r.escalated ?? false,
      // Structural actionability flag — avoids web consumers parsing action strings.
      actionable:
        (r.escalated ?? false) || ACTIONABLE_RECOVERY_ACTIONS.has(r.action),
    };
  }

  const effort = sharedState.effort;
  const effortLevel = effort?.name ?? null;

  const routingPolicy = sharedState.routingPolicy
    ? (sharedState.routingPolicy as unknown as Record<string, unknown>)
    : null;

  const inj = sharedState.lastMemoryInjection;
  const operatorMetadata: OperatorMetadataSnapshot = {
    effortName: effort?.name ?? null,
    effortLevel: effort?.level ?? null,
    driverTier: effort?.driver ?? null,
    thinkingLevel: effort?.thinking ?? null,
    effortCapped: effort?.capped ?? false,
    memoryTokenEstimate: sharedState.memoryTokenEstimate,
    workingMemoryCount: inj?.workingMemoryFactCount ?? null,
    totalFactCount: inj
      ? (inj.projectFactCount ?? 0) +
        (inj.globalFactCount ?? 0) +
        (inj.workingMemoryFactCount ?? 0)
      : null,
  };

  return {
    mode: sharedState.dashboardMode ?? "compact",
    turns: sharedState.dashboardTurns ?? 0,
    memoryTokenEstimate: sharedState.memoryTokenEstimate,
    routingPolicy,
    effortLevel,
    recovery,
    operatorMetadata,
  };
}

function buildDesignTree(repoRoot: string): DesignTreeSnapshot {
  // Use sharedState.designTree if available (populated by design-tree extension),
  // otherwise fall back to an on-demand file scan.
  const live = sharedState.designTree;

  // On-demand scan for full node details (always)
  const docsDir = path.join(repoRoot, "docs");
  let nodes: DesignNodeSummary[] = [];
  let focusedNodeId: string | null = null;

  if (live?.focusedNode) {
    focusedNodeId = live.focusedNode.id;
  }

  let statusCounts: Record<string, number> = {};
  let openQuestionCount = 0;
  let focusedNode: DesignNodeSummary | null = null;

  let scanSucceeded = false;
  if (fs.existsSync(docsDir)) {
    try {
      const tree = scanDesignDocs(docsDir);
      for (const [, node] of tree.nodes) {
        const summary: DesignNodeSummary = {
          id: node.id,
          title: node.title,
          status: node.status,
          parent: node.parent ?? null,
          questionCount: node.open_questions.length,
          questions: node.open_questions,
          tags: node.tags,
          openspecChange: node.openspec_change ?? null,
          branch: node.branch ?? null,
        };
        nodes.push(summary);
        statusCounts[node.status] = (statusCounts[node.status] ?? 0) + 1;
        openQuestionCount += node.open_questions.length;

        if (node.id === focusedNodeId) {
          focusedNode = summary;
        }
      }
      scanSucceeded = true;
    } catch {
      // docs dir exists but scan failed — fall through to live state fallback
    }
  }

  if (!scanSucceeded && live) {
    // No docs dir but we have live dashboard state — synthesise minimal summary
    openQuestionCount = live.openQuestionCount;
    statusCounts = {
      decided: live.decidedCount,
      exploring: live.exploringCount,
      implementing: live.implementingCount,
      implemented: live.implementedCount,
      blocked: live.blockedCount,
    };
  }

  return {
    nodeCount: nodes.length || live?.nodeCount || 0,
    statusCounts,
    openQuestionCount,
    focusedNode,
    nodes,
  };
}

function buildOpenSpec(repoRoot: string): OpenSpecSnapshot {
  // On-demand scan
  let changes: OpenSpecChangeSummary[] = [];

  try {
    const rawChanges = listChanges(repoRoot);
    changes = rawChanges.map((c) => ({
      name: c.name,
      stage: c.stage,
      hasProposal: c.hasProposal,
      hasDesign: c.hasDesign,
      hasSpecs: c.hasSpecs,
      hasTasks: c.hasTasks,
      tasksTotal: c.totalTasks,
      tasksDone: c.doneTasks,
      specDomains: c.specs.map((s) => s.domain),
    }));
  } catch {
    // openspec dir may not exist
  }

  return { changes };
}

function buildCleave(): CleaveSnapshot {
  const c = sharedState.cleave;
  if (!c) {
    return { status: "idle", runId: null, children: [], updatedAt: null };
  }
  return {
    status: c.status,
    runId: c.runId ?? null,
    children: (c.children ?? []).map((ch) => ({
      label: ch.label,
      status: ch.status,
      elapsed: ch.elapsed ?? null,
    })),
    updatedAt: c.updatedAt ?? null,
  };
}

function buildModels(): ModelsSnapshot {
  const effort = sharedState.effort;
  return {
    routingPolicy: sharedState.routingPolicy
      ? (sharedState.routingPolicy as unknown as Record<string, unknown>)
      : null,
    effortLevel: effort?.name ?? null,
    effortCapped: effort?.capped ?? false,
    resolvedExtractionModelId: effort?.resolvedExtractionModelId ?? null,
  };
}

function buildMemory(): MemorySnapshot {
  const inj = sharedState.lastMemoryInjection;
  return {
    tokenEstimate: sharedState.memoryTokenEstimate,
    lastInjection: inj
      ? {
          factCount:
            (inj.projectFactCount ?? 0) +
            (inj.globalFactCount ?? 0) +
            (inj.workingMemoryFactCount ?? 0),
          episodeCount: inj.episodeCount,
          workingMemoryCount: inj.workingMemoryFactCount,
          totalTokens: inj.estimatedTokens,
        }
      : null,
  };
}

function buildHealth(startedAt: number): HealthSnapshot {
  return {
    status: "ok",
    uptimeMs: Date.now() - startedAt,
    serverAlive: true,
  };
}

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Build a full ControlPlaneState snapshot.
 *
 * @param repoRoot   Absolute path to the repository root.
 * @param startedAt  Unix epoch ms when the web UI server started (for uptime).
 */
export function buildControlPlaneState(
  repoRoot: string,
  startedAt: number
): ControlPlaneState {
  return {
    schemaVersion: SCHEMA_VERSION,
    session: buildSession(repoRoot),
    dashboard: buildDashboard(),
    designTree: buildDesignTree(repoRoot),
    openspec: buildOpenSpec(repoRoot),
    cleave: buildCleave(),
    models: buildModels(),
    memory: buildMemory(),
    health: buildHealth(startedAt),
  };
}

/**
 * Build only the named top-level slice.
 * Used by the slice routes (e.g. GET /api/design-tree).
 */
export function buildSlice(
  slice: keyof Omit<ControlPlaneState, "schemaVersion">,
  repoRoot: string,
  startedAt: number
): ControlPlaneState[typeof slice] {
  switch (slice) {
    case "session":    return buildSession(repoRoot);
    case "dashboard":  return buildDashboard();
    case "designTree": return buildDesignTree(repoRoot);
    case "openspec":   return buildOpenSpec(repoRoot);
    case "cleave":     return buildCleave();
    case "models":     return buildModels();
    case "memory":     return buildMemory();
    case "health":     return buildHealth(startedAt);
    default: {
      const _exhaustive: never = slice;
      throw new Error(`Unhandled slice: ${String(_exhaustive)}`);
    }
  }
}
