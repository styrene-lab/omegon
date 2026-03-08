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
} from "./dashboard/types.ts";

import type { EffortState } from "./effort/types.ts";
import type { ProviderRoutingPolicy } from "./lib/model-routing.ts";

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
}

// Initialize once on first import, reuse thereafter via global symbol.
// New dashboard properties are intentionally omitted — they start as undefined
// and are populated when each producer extension loads.
if (!(globalThis as any)[SHARED_KEY]) {
  (globalThis as any)[SHARED_KEY] = {
    memoryTokenEstimate: 0,
  } satisfies SharedState;
}

export const sharedState: SharedState = (globalThis as any)[SHARED_KEY];
