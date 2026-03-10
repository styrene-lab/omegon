/**
 * Shared state between extensions loaded in the same pi process.
 *
 * Uses globalThis to guarantee sharing regardless of module loader
 * caching behavior (jiti may create separate instances per extension).
 *
 * Keep this minimal — only data that genuinely needs cross-extension sharing.
 */

import type {
  DesignTreeDashboardState,
  OpenSpecDashboardState,
  CleaveState,
  RecoveryDashboardState,
} from "./dashboard/types.ts";

import type { EffortState } from "./effort/types.ts";
import type { ProviderRoutingPolicy } from "./lib/model-routing.ts";
import type { MemoryInjectionMetrics } from "./project-memory/injection-metrics.ts";
import type { LifecycleMemoryMessage } from "./project-memory/types.ts";
import { getDefaultPolicy } from "./lib/model-routing.ts";

export type RecoveryFailureClassification =
  | "transient_server_error"
  | "rate_limited"
  | "quota_exhausted"
  | "authentication_failed"
  | "malformed_output"
  | "context_overflow"
  | "invalid_request"
  | "unknown_upstream";

export type RecoveryDisposition =
  | "retry_same_model"
  | "cooldown_and_failover"
  | "guidance_only"
  | "handled_elsewhere"
  | "escalate";

export interface RecoveryEvent {
  provider: string;
  model: string;
  turnIndex: number;
  classification: RecoveryFailureClassification;
  originalErrorSummary: string;
  retryable: boolean;
  disposition: RecoveryDisposition;
  retryAttempted: boolean;
  retryCount: number;
  maxRetries: number;
  guidance: string;
  cooldownApplied?: boolean;
  alternateCandidate?: {
    provider: string;
    model: string;
  };
  timestamp: number;
}

// Re-export dashboard types for consumer convenience
export type {
  DesignTreeDashboardState,
  DesignTreeFocusedNode,
  OpenSpecDashboardState,
  OpenSpecChangeEntry,
  CleaveState,
  CleaveChildState,
  CleaveStatus,
  DashboardMode,
  DashboardState,
} from "./dashboard/types.ts";

// Re-export routing types for consumer convenience
export type { ProviderRoutingPolicy, ResolvedTierModel, ModelTier, ProviderName } from "./lib/model-routing.ts";

// Re-export effort types for consumer convenience
export type {
  EffortState,
  EffortConfig,
  EffortLevel,
  EffortModelTier,
  EffortName,
  ThinkingLevel,
} from "./effort/types.ts";

/** Event channel fired by producers after writing dashboard state. */
export const DASHBOARD_UPDATE_EVENT = "dashboard:update" as const;

const SHARED_KEY = Symbol.for("pi-kit-shared-state");

interface SharedState {
  /** Approximate token count of the last memory injection into context.
   *  Written by project-memory, read by dashboard for the context gauge. */
  memoryTokenEstimate: number;

  /** Structured snapshot of the last memory injection payload. */
  lastMemoryInjection?: MemoryInjectionMetrics;

  /** Design tree summary state. Written by design-tree extension. */
  designTree?: DesignTreeDashboardState;

  /** OpenSpec changes summary. Written by openspec/cleave extension. */
  openspec?: OpenSpecDashboardState;

  /** Cleave execution state. Written by cleave extension. */
  cleave?: CleaveState;

  /** Effort tier state. Written by effort extension, read by model-budget and cleave. */
  effort?: EffortState;

  /** Session routing policy. Written by operator/preflight, read by cleave and model-budget. */
  routingPolicy?: ProviderRoutingPolicy;

  /** Pending structured lifecycle candidates waiting for project-memory ingestion. */
  lifecycleCandidateQueue?: LifecycleMemoryMessage[];

  /** Latest upstream recovery event for dashboard/harness visibility. */
  latestRecoveryEvent?: RecoveryEvent;

  /** Dashboard-friendly recovery summary derived from the latest recovery event. */
  recovery?: RecoveryDashboardState;

  /** Current dashboard display mode. Written by dashboard extension. */
  dashboardMode?: string;

  /** Number of conversation turns in the current session. Written by dashboard extension. */
  dashboardTurns?: number;

  /** Per-request retry ledger for bounded recovery decisions across core and extension-driven retries. */
  recoveryRetryCounts?: Record<string, number>;
}

// Initialize once on first import, reuse thereafter via global symbol.
// New dashboard properties are intentionally omitted — they start as undefined
// and are populated when each producer extension loads.
if (!(globalThis as any)[SHARED_KEY]) {
  (globalThis as any)[SHARED_KEY] = {
    memoryTokenEstimate: 0,
    routingPolicy: getDefaultPolicy(),
  } satisfies SharedState;
}

export const sharedState: SharedState = (globalThis as any)[SHARED_KEY];
