/**
 * Dashboard type definitions.
 *
 * These interfaces define the shape of dashboard state
 * written by producer extensions (design-tree, openspec, cleave)
 * and read by the dashboard extension for rendering.
 *
 * Re-exported from shared-state.ts for convenience.
 */

// ── Design Tree ──────────────────────────────────────────────

/** Mirrors DesignSpecBinding from openspec/archive-gate.ts for dashboard consumers. */
export interface DesignSpecBindingState {
  active: boolean;
  archived: boolean;
  missing: boolean;
}

/** Acceptance-criteria counts derived from a design node's AC section. */
export interface AcSummary {
  scenarios: number;
  falsifiability: number;
  constraints: number;
}

/** Outcome of an /assess spec run persisted in openspec/design/<id>/assessment.json. */
export interface DesignAssessmentResult {
  outcome: "pass" | "reopen" | "ambiguous";
  timestamp: string;
  summary?: string;
}

/** Per-status counts across the design pipeline for funnel rendering. */
export interface DesignPipelineCounts {
  /** seed/exploring nodes without a spec binding */
  needsSpec: number;
  /** seed/exploring nodes with an active or archived spec binding */
  designing: number;
  /** nodes in 'decided' status */
  decided: number;
  /** nodes in 'implementing' status */
  implementing: number;
  /** nodes in 'implemented' status */
  done: number;
}

export interface DesignTreeFocusedNode {
  id: string;
  title: string;
  status: string;
  questions: string[];
  branch?: string;
  branchCount?: number;
  filePath?: string;
}

export interface DesignTreeDashboardState {
  nodeCount: number;
  decidedCount: number;
  exploringCount: number;
  implementingCount: number;
  implementedCount: number;
  blockedCount: number;
  deferredCount: number;
  openQuestionCount: number;
  focusedNode: DesignTreeFocusedNode | null;
  /** All nodes for overlay list view */
  nodes?: Array<{
    id: string;
    title: string;
    status: string;
    questionCount: number;
    filePath?: string;
    branches?: string[];
    /** OpenSpec design-phase binding state (undefined for seed nodes) */
    designSpec?: DesignSpecBindingState;
    /** Acceptance-criteria counts (undefined if no AC section) */
    acSummary?: AcSummary | null;
    /** Last /assess spec result (undefined if no assessment.json) */
    assessmentResult?: DesignAssessmentResult | null;
  }>;
  /** Implementing nodes shown in raised mode with branch associations */
  implementingNodes?: Array<{ id: string; title: string; branch?: string; filePath?: string }>;
  /** Design pipeline funnel counts */
  designPipeline?: DesignPipelineCounts;
}

// ── OpenSpec ─────────────────────────────────────────────────

export interface OpenSpecChangeEntry {
  name: string;
  stage: string;
  tasksDone: number;
  tasksTotal: number;
  /** Which lifecycle artifacts exist */
  artifacts?: ("proposal" | "design" | "specs" | "tasks")[];
  /** Spec domain names (e.g. ["auth", "api/tokens"]) */
  specDomains?: string[];
  /** Absolute path to the change directory */
  path?: string;
}

export interface OpenSpecDashboardState {
  changes: OpenSpecChangeEntry[];
}

// ── Cleave ───────────────────────────────────────────────────

export type CleaveStatus =
  | "idle"
  | "assessing"
  | "planning"
  | "dispatching"
  | "merging"
  | "done"
  | "failed";

export interface CleaveChildState {
  label: string;
  status: "pending" | "running" | "done" | "failed";
  elapsed?: number;
  /** Epoch ms when the child transitioned to "running". Used for live elapsed calculation. */
  startedAt?: number;
  /** Last meaningful stdout line from the child process. Updated ~500ms while running. */
  lastLine?: string;
}

export interface CleaveState {
  status: CleaveStatus;
  runId?: string;
  children?: CleaveChildState[];
  /** Unix epoch ms of the last cleave dashboard update */
  updatedAt?: number;
}

// ── Harness Recovery ─────────────────────────────────────────

export type RecoveryAction =
  | "retry"
  | "switch_candidate"
  | "switch_offline"
  | "cooldown"
  | "escalate"
  | "observe";

export interface RecoveryTarget {
  provider: string;
  modelId?: string;
  label?: string;
}

export interface RecoveryCooldownSummary {
  scope: "provider" | "candidate";
  key: string;
  provider?: string;
  modelId?: string;
  until: number;
  reason?: string;
}

export interface RecoveryDashboardState {
  provider: string;
  modelId: string;
  classification: string;
  summary: string;
  action: RecoveryAction;
  retryCount?: number;
  maxRetries?: number;
  attemptId?: string;
  timestamp: number;
  escalated?: boolean;
  target?: RecoveryTarget;
  cooldowns?: RecoveryCooldownSummary[];
}

// ── Dashboard UI ─────────────────────────────────────────────

export type DashboardMode = "compact" | "raised" | "panel" | "focused";

/** Mutable state held by the dashboard extension, read by the footer component. */
export interface DashboardState {
  mode: DashboardMode;
  turns: number;
}
