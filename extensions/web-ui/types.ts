/**
 * ControlPlaneState — versioned read-only snapshot contract for the web UI.
 *
 * Served at GET /api/state and derived section routes.
 * All fields are plain JSON-serialisable values — no Dates, no symbols.
 */

// ── Schema version ────────────────────────────────────────────────────────────

/** Bumped when the shape of ControlPlaneState changes in a breaking way. */
export const SCHEMA_VERSION = 1 as const;
export type SchemaVersion = typeof SCHEMA_VERSION;

// ── Session ───────────────────────────────────────────────────────────────────

export interface SessionSnapshot {
  /** ISO 8601 timestamp when the snapshot was generated. */
  capturedAt: string;
  /** pi-kit package version (from package.json). */
  piKitVersion: string;
  /** Absolute path of the repository root (process.cwd() at extension load). */
  repoRoot: string;
  /** Current git branch, or null if not a git repo. */
  gitBranch: string | null;
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

export interface DashboardSnapshot {
  /** Current display mode. */
  mode: string;
  /** Number of conversation turns in the current session. */
  turns: number;
  /** Approximate token count of the last memory injection. */
  memoryTokenEstimate: number;
  /** Routing policy tier overrides, if any. */
  routingPolicy: Record<string, unknown> | null;
  /** Current effort level label, if effort extension is active. */
  effortLevel: string | null;
  /** Latest harness recovery event summary, if any. */
  recovery: RecoverySnapshot | null;
  /**
   * Pinned operator-context metadata mirroring the raised-mode dashboard block.
   * Exposes context/model/thinking and memory-oriented state structurally so
   * web UI consumers do not need to parse footer or display text.
   */
  operatorMetadata: OperatorMetadataSnapshot;
}

export interface RecoverySnapshot {
  provider: string;
  modelId: string;
  classification: string;
  summary: string;
  action: string;
  retryCount: number | null;
  timestamp: number;
  escalated: boolean;
  /**
   * True when the recovery event represents a condition that may require
   * operator attention or UI-level action (e.g. escalation, hard failover).
   * False for passive/observe-only recovery events.
   * Web UI consumers should use this flag instead of parsing action strings.
   */
  actionable: boolean;
}

/**
 * Pinned operator-context metadata exposed by the raised-mode dashboard.
 * Groups the context/model/thinking indicators and memory-oriented state so
 * web UI consumers do not need to reverse-engineer values from footer text.
 */
export interface OperatorMetadataSnapshot {
  /** Active effort tier name (e.g. "Ruthless"), or null if effort extension is inactive. */
  effortName: string | null;
  /** Numeric effort level 1-7, or null. */
  effortLevel: number | null;
  /** Driver model tier (e.g. "sonnet", "opus"), or null. */
  driverTier: string | null;
  /** Extended thinking level (e.g. "medium", "high"), or null. */
  thinkingLevel: string | null;
  /** Whether the effort level is ceiling-locked by an operator cap. */
  effortCapped: boolean;
  /** Approximate token count of the last memory injection. */
  memoryTokenEstimate: number;
  /** Number of facts in working memory during the last injection. */
  workingMemoryCount: number | null;
  /** Total fact count from the last injection (project + global + working). */
  totalFactCount: number | null;
}

// ── Design Tree ───────────────────────────────────────────────────────────────

export interface DesignTreeSnapshot {
  /** Total node count. */
  nodeCount: number;
  /** Count by status. */
  statusCounts: Record<string, number>;
  /** Number of open questions across all nodes. */
  openQuestionCount: number;
  /** Currently focused node, or null. */
  focusedNode: DesignNodeSummary | null;
  /** Summary of every node in the tree. */
  nodes: DesignNodeSummary[];
}

export interface DesignNodeSummary {
  id: string;
  title: string;
  status: string;
  parent: string | null;
  questionCount: number;
  questions: string[];
  tags: string[];
  openspecChange: string | null;
  branch: string | null;
}

// ── OpenSpec ──────────────────────────────────────────────────────────────────

export interface OpenSpecSnapshot {
  changes: OpenSpecChangeSummary[];
}

export interface OpenSpecChangeSummary {
  name: string;
  stage: string;
  hasProposal: boolean;
  hasDesign: boolean;
  hasSpecs: boolean;
  hasTasks: boolean;
  tasksTotal: number;
  tasksDone: number;
  specDomains: string[];
}

// ── Cleave ────────────────────────────────────────────────────────────────────

export interface CleaveSnapshot {
  status: string;
  runId: string | null;
  children: CleaveChildSummary[];
  updatedAt: number | null;
}

export interface CleaveChildSummary {
  label: string;
  status: string;
  elapsed: number | null;
}

// ── Models ────────────────────────────────────────────────────────────────────

export interface ModelsSnapshot {
  routingPolicy: Record<string, unknown> | null;
  effortLevel: string | null;
  effortCapped: boolean;
  resolvedExtractionModelId: string | null;
}

// ── Memory ────────────────────────────────────────────────────────────────────

export interface MemorySnapshot {
  tokenEstimate: number;
  lastInjection: MemoryInjectionSummary | null;
}

export interface MemoryInjectionSummary {
  factCount: number;
  episodeCount: number;
  workingMemoryCount: number;
  totalTokens: number;
}

// ── Health ────────────────────────────────────────────────────────────────────

export interface HealthSnapshot {
  status: "ok";
  uptimeMs: number;
  /** Whether the web UI server itself is considered healthy. */
  serverAlive: boolean;
}

// ── Root ──────────────────────────────────────────────────────────────────────

/**
 * Top-level versioned control-plane state snapshot.
 * Served at GET /api/state.
 */
export interface ControlPlaneState {
  schemaVersion: SchemaVersion;
  session: SessionSnapshot;
  dashboard: DashboardSnapshot;
  designTree: DesignTreeSnapshot;
  openspec: OpenSpecSnapshot;
  cleave: CleaveSnapshot;
  models: ModelsSnapshot;
  memory: MemorySnapshot;
  health: HealthSnapshot;
}
