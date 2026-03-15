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
import { sharedState } from "../lib/shared-state.ts";
import { listChanges, listDesignChanges } from "../openspec/spec.ts";
import { scanDesignDocs, countAcceptanceCriteria } from "../design-tree/tree.ts";
import {
  SCHEMA_VERSION,
  type ControlPlaneState,
  type SessionSnapshot,
  type DashboardSnapshot,
  type DesignTreeSnapshot,
  type DesignNodeSummary,
  type DesignSpecBinding,
  type ACSummary,
  type AssessmentResult,
  type OpenSpecSnapshot,
  type OpenSpecChangeSummary,
  type CleaveSnapshot,
  type ModelsSnapshot,
  type MemorySnapshot,
  type HealthSnapshot,
  type RecoverySnapshot,
  type OperatorMetadataSnapshot,
  type DesignPipelineSnapshot,
  type DesignChangeSummary,
  type DesignFunnelCounts,
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

function resolveDesignSpecBinding(repoRoot: string, nodeId: string): DesignSpecBinding | null {
  const activeDir   = path.join(repoRoot, "openspec", "design", nodeId);
  const archiveDir  = path.join(repoRoot, "openspec", "design-archive", nodeId);
  const [dir, isArchived] = fs.existsSync(activeDir)
    ? [activeDir, false]
    : fs.existsSync(archiveDir)
      ? [archiveDir, true]
      : [null, false];

  if (!dir) return null;

  const hasProposal   = fs.existsSync(path.join(dir, "proposal.md"));
  const hasSpec       = fs.existsSync(path.join(dir, "spec.md"));
  const hasTasks      = fs.existsSync(path.join(dir, "tasks.md"));
  const hasAssessment = fs.existsSync(path.join(dir, "assessment.json"));

  let tasksDone  = 0;
  let tasksTotal = 0;
  if (hasTasks) {
    try {
      const raw = fs.readFileSync(path.join(dir, "tasks.md"), "utf8");
      tasksTotal = (raw.match(/^\s*-\s+\[[ xX]\]/gm) ?? []).length;
      tasksDone  = (raw.match(/^\s*-\s+\[[xX]\]/gm) ?? []).length;
    } catch { /* ignore */ }
  }

  // Relative path from repoRoot for portability
  const changePath = path.relative(repoRoot, dir);

  return { changePath, hasProposal, hasSpec, hasTasks, hasAssessment, tasksDone, tasksTotal, isArchived };
}

function readAssessmentResult(repoRoot: string, nodeId: string): AssessmentResult | null {
  for (const subDir of ["design", "design-archive"]) {
    const assessPath = path.join(repoRoot, "openspec", subDir, nodeId, "assessment.json");
    if (fs.existsSync(assessPath)) {
      try {
        const raw = JSON.parse(fs.readFileSync(assessPath, "utf8")) as {
          outcome?: string;
          timestamp?: string;
        };
        if (raw.timestamp) {
          return { pass: raw.outcome === "pass", capturedAt: raw.timestamp };
        }
      } catch { /* ignore */ }
    }
  }
  return null;
}

/**
 * Scan docs/ once and build the enriched DesignNodeSummary list.
 * Called at most once per request; the result is shared between
 * buildDesignTree and buildDesignPipeline to avoid duplicate scans.
 */
function scanDesignNodes(repoRoot: string): DesignNodeSummary[] {
  const docsDir = path.join(repoRoot, "docs");
  if (!fs.existsSync(docsDir)) return [];
  try {
    const tree = scanDesignDocs(docsDir);
    const out: DesignNodeSummary[] = [];
    for (const [, node] of tree.nodes) {
      const acRaw = countAcceptanceCriteria(node);
      const acSummary: ACSummary | null = acRaw
        ? { scenarios: acRaw.scenarios, falsifiability: acRaw.falsifiability, constraints: acRaw.constraints }
        : null;
      const designSpec       = resolveDesignSpecBinding(repoRoot, node.id);
      const assessmentResult = readAssessmentResult(repoRoot, node.id);
      out.push({
        id: node.id,
        title: node.title,
        status: node.status,
        parent: node.parent ?? null,
        questionCount: node.open_questions.length,
        questions: node.open_questions,
        tags: node.tags,
        openspecChange: node.openspec_change ?? null,
        branch: node.branch ?? null,
        designSpec,
        acSummary,
        assessmentResult,
      });
    }
    return out;
  } catch {
    return [];
  }
}

function buildDesignTree(repoRoot: string, scannedNodes?: DesignNodeSummary[]): DesignTreeSnapshot {
  // Use sharedState.designTree if available (populated by design-tree extension),
  // otherwise fall back to an on-demand file scan.
  const live = sharedState.designTree;

  // Accept pre-scanned nodes (shared with buildDesignPipeline) or scan now.
  let nodes: DesignNodeSummary[] = scannedNodes ?? scanDesignNodes(repoRoot);
  let focusedNodeId: string | null = null;

  if (live?.focusedNode) {
    focusedNodeId = live.focusedNode.id;
  }

  let statusCounts: Record<string, number> = {};
  let openQuestionCount = 0;
  let focusedNode: DesignNodeSummary | null = null;

  let scanSucceeded = nodes.length > 0 || fs.existsSync(path.join(repoRoot, "docs"));
  if (nodes.length > 0 || scannedNodes !== undefined) {
    try {
      for (const node of nodes) {
        statusCounts[node.status] = (statusCounts[node.status] ?? 0) + 1;
        openQuestionCount += node.questionCount;
        if (node.id === focusedNodeId) focusedNode = node;
      }
      scanSucceeded = true;
    } catch {
      // fall through to live state fallback
    }
  } else {
    // No pre-scanned nodes and no docs dir — mark as not succeeded
    scanSucceeded = false;
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

function buildDesignPipeline(repoRoot: string, scannedNodes?: DesignNodeSummary[]): DesignPipelineSnapshot {
  const raw = listDesignChanges(repoRoot);

  const changes: DesignChangeSummary[] = raw.map((c) => ({
    nodeId:         c.nodeId,
    changePath:     path.relative(repoRoot, c.path),
    hasProposal:    c.hasProposal,
    hasSpec:        c.hasSpec,
    hasTasks:       c.hasTasks,
    hasAssessment:  c.hasAssessment,
    assessmentPass: c.assessmentPass,
    capturedAt:     c.capturedAt,
    tasksDone:      c.tasksDone,
    tasksTotal:     c.tasksTotal,
    isArchived:     c.isArchived,
    archivedPath:   c.archivedPath ? path.relative(repoRoot, c.archivedPath) : undefined,
  }));

  // Compute funnel counts from the already-scanned node list (shared with
  // buildDesignTree to avoid a second full docs/ + openspec/design/ scan per request).
  const nodes = scannedNodes ?? scanDesignNodes(repoRoot);
  let total    = nodes.length;
  let bound    = 0;
  let tasksComplete = 0;
  let assessed = 0;
  const archived = raw.filter((c) => c.isArchived).length;

  for (const node of nodes) {
    if (!node.designSpec) continue;
    bound++;
    if (node.designSpec.tasksTotal > 0 && node.designSpec.tasksDone >= node.designSpec.tasksTotal) tasksComplete++;
    if (node.assessmentResult?.pass) assessed++;
  }

  const funnelCounts: DesignFunnelCounts = { total, bound, tasksComplete, assessed, archived };

  return {
    capturedAt: new Date().toISOString(),
    changes,
    funnelCounts,
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
  // Scan design docs exactly once per request — result shared between
  // buildDesignTree and buildDesignPipeline to avoid duplicate I/O.
  const scannedNodes = scanDesignNodes(repoRoot);
  return {
    schemaVersion: SCHEMA_VERSION,
    session: buildSession(repoRoot),
    dashboard: buildDashboard(),
    designTree: buildDesignTree(repoRoot, scannedNodes),
    openspec: buildOpenSpec(repoRoot),
    cleave: buildCleave(),
    models: buildModels(),
    memory: buildMemory(),
    health: buildHealth(startedAt),
    designPipeline: buildDesignPipeline(repoRoot, scannedNodes),
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
    case "designPipeline": return buildDesignPipeline(repoRoot);
    default: {
      const _exhaustive: never = slice;
      throw new Error(`Unhandled slice: ${String(_exhaustive)}`);
    }
  }
}
