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

export interface DesignTreeFocusedNode {
  id: string;
  title: string;
  status: string;
  questions: string[];
}

export interface DesignTreeDashboardState {
  nodeCount: number;
  decidedCount: number;
  exploringCount: number;
  blockedCount: number;
  openQuestionCount: number;
  focusedNode: DesignTreeFocusedNode | null;
}

// ── OpenSpec ─────────────────────────────────────────────────

export interface OpenSpecChangeEntry {
  name: string;
  stage: string;
  tasksDone: number;
  tasksTotal: number;
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
}

export interface CleaveState {
  status: CleaveStatus;
  runId?: string;
  children?: CleaveChildState[];
}

// ── Dashboard UI ─────────────────────────────────────────────

export type DashboardMode = "compact" | "raised";

export interface FooterState {
  mode: DashboardMode;
}
