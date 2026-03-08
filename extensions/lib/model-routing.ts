/**
 * Shared provider-aware model resolver for pi-kit.
 *
 * Keeps canonical tier semantics (local|haiku|sonnet|opus) stable while
 * resolving to concrete provider+model at runtime based on a session policy.
 *
 * Design decisions followed:
 * - Tiers stay abstract; provider choice is a session policy concern
 * - Explicit model IDs are preferred over fuzzy tier aliases at execution time
 * - Phase 1 keeps internal tier keys; UX adopts Servitor/Adept/Magos/Archmagos
 */

import { PREFERRED_ORDER } from "./local-models.ts";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ModelTier = "local" | "haiku" | "sonnet" | "opus";
export type ProviderName = "openai" | "anthropic" | "local";

/**
 * Operator-driven session routing policy.
 * Reflects current provider posture rather than hard quota tracking.
 */
export interface ProviderRoutingPolicy {
  /** Providers to try in preference order. */
  providerOrder: ProviderName[];
  /** Providers to skip unless no acceptable alternative exists. */
  avoidProviders: ProviderName[];
  /** When true, prefer inexpensive cloud over local for background tasks. */
  cheapCloudPreferredOverLocal: boolean;
  /** When true, ask operator for current provider posture before large Cleave runs. */
  requirePreflightForLargeRuns: boolean;
  /** Optional free-text note (e.g. "Anthropic budget is low today"). */
  notes?: string;
}

/**
 * Resolved concrete model for a requested tier.
 */
export interface ResolvedTierModel {
  tier: ModelTier;
  provider: ProviderName;
  modelId: string;
}

/**
 * Minimal model shape expected from the pi model registry.
 */
export interface RegistryModel {
  id: string;
  provider: string;
  [key: string]: unknown;
}

// ---------------------------------------------------------------------------
// Anthropic tier matchers
// ---------------------------------------------------------------------------

const ANTHROPIC_TIER_PREFIXES: Record<Exclude<ModelTier, "local">, string[]> = {
  haiku: ["claude-haiku"],
  sonnet: ["claude-sonnet"],
  opus: ["claude-opus"],
};

function matchAnthropicTier(models: RegistryModel[], tier: Exclude<ModelTier, "local">): RegistryModel | undefined {
  const prefixes = ANTHROPIC_TIER_PREFIXES[tier];
  for (const prefix of prefixes) {
    const candidates = models
      .filter((m) => m.provider === "anthropic" && m.id.startsWith(prefix))
      .sort((a, b) => b.id.localeCompare(a.id));
    if (candidates.length > 0) return candidates[0];
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// OpenAI tier matchers
// ---------------------------------------------------------------------------

/**
 * Explicit OpenAI model IDs per tier (most preferred first).
 * Mapping follows the design spec:
 *   haiku  → gpt-5.1-codex
 *   sonnet → gpt-5.3-codex-spark
 *   opus   → gpt-5.4
 */
const OPENAI_TIER_MODELS: Record<Exclude<ModelTier, "local">, string[]> = {
  haiku: ["gpt-5.1-codex", "gpt-4o-mini", "gpt-4.1-mini"],
  sonnet: ["gpt-5.3-codex-spark", "gpt-4.1", "gpt-4o"],
  opus: ["gpt-5.4", "gpt-4.5", "o3"],
};

function matchOpenAITier(models: RegistryModel[], tier: Exclude<ModelTier, "local">): RegistryModel | undefined {
  const candidates = OPENAI_TIER_MODELS[tier];
  for (const modelId of candidates) {
    const match = models.find((m) => m.provider === "openai" && m.id === modelId);
    if (match) return match;
  }
  // Fallback: prefix scan for anything that looks right
  const prefixMap: Record<string, string[]> = {
    haiku: ["gpt-4o-mini", "gpt-4.1-mini"],
    sonnet: ["gpt-4o", "gpt-4.1"],
    opus: ["gpt-4.5", "o3", "gpt-5"],
  };
  for (const prefix of prefixMap[tier] ?? []) {
    const found = models.find((m) => m.provider === "openai" && m.id.startsWith(prefix));
    if (found) return found;
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// Local tier matcher
// ---------------------------------------------------------------------------

function matchLocalTier(models: RegistryModel[]): RegistryModel | undefined {
  const locals = models.filter((m) => m.provider === "local");
  if (locals.length === 0) return undefined;
  // Respect preference order from local-models registry
  for (const preferred of PREFERRED_ORDER) {
    const match = locals.find((m) => m.id === preferred);
    if (match) return match;
  }
  return locals[0];
}

// ---------------------------------------------------------------------------
// Core resolver
// ---------------------------------------------------------------------------

/**
 * Resolve an abstract tier to a concrete {provider, modelId} using the
 * session routing policy and the available model registry.
 *
 * @param tier      The abstract tier requested (local|haiku|sonnet|opus)
 * @param models    Snapshot of the pi model registry (modelRegistry.getAll())
 * @param policy    Session routing policy (from sharedState.routingPolicy)
 * @returns         Resolved model or undefined if nothing matched
 */
export function resolveTier(
  tier: ModelTier,
  models: RegistryModel[],
  policy: ProviderRoutingPolicy,
): ResolvedTierModel | undefined {
  // "local" is always satisfied locally — policy cannot redirect it to cloud.
  if (tier === "local") {
    const local = matchLocalTier(models);
    if (!local) return undefined;
    return { tier, provider: "local", modelId: local.id };
  }

  // Build effective provider order: avoid-list providers go last (as fallback),
  // not completely excluded — we still try them if no other option exists.
  const ordered = dedupeProviderOrder(policy.providerOrder, policy.avoidProviders);

  for (const provider of ordered) {
    if (policy.avoidProviders.includes(provider)) continue; // skip in first pass
    const resolved = tryProvider(tier, provider, models);
    if (resolved) return { tier, provider, modelId: resolved.id };
  }

  // Fallback: try avoided providers before giving up
  for (const provider of policy.avoidProviders) {
    const resolved = tryProvider(tier, provider, models);
    if (resolved) return { tier, provider, modelId: resolved.id };
  }

  return undefined;
}

function tryProvider(
  tier: Exclude<ModelTier, "local">,
  provider: ProviderName,
  models: RegistryModel[],
): RegistryModel | undefined {
  if (provider === "anthropic") return matchAnthropicTier(models, tier);
  if (provider === "openai") return matchOpenAITier(models, tier);
  if (provider === "local") {
    // When "local" appears in the cloud provider order, resolve with the best
    // local model (even for non-local tiers). This lets operators that run
    // purely offline still get a result.
    return matchLocalTier(models);
  }
  return undefined;
}

/**
 * Produce a deduplicated provider order that guarantees all providers appear
 * (avoided ones at the end, in their original relative order).
 */
function dedupeProviderOrder(
  order: ProviderName[],
  avoided: ProviderName[],
): ProviderName[] {
  const seen = new Set<ProviderName>();
  const result: ProviderName[] = [];
  for (const p of order) {
    if (!seen.has(p)) {
      seen.add(p);
      result.push(p);
    }
  }
  // Append any avoided providers not already in the list
  for (const p of avoided) {
    if (!seen.has(p)) {
      seen.add(p);
      result.push(p);
    }
  }
  return result;
}

// ---------------------------------------------------------------------------
// Display labels
// ---------------------------------------------------------------------------

const TIER_DISPLAY_LABELS: Record<ModelTier, string> = {
  local: "Servitor",
  haiku: "Adept",
  sonnet: "Magos",
  opus: "Archmagos",
};

/**
 * Get the operator-facing display label for an abstract tier.
 *
 * local   → Servitor
 * haiku   → Adept
 * sonnet  → Magos
 * opus    → Archmagos
 */
export function getTierDisplayLabel(tier: ModelTier): string {
  return TIER_DISPLAY_LABELS[tier];
}

// ---------------------------------------------------------------------------
// Default policy
// ---------------------------------------------------------------------------

/**
 * Sensible defaults for a fresh session:
 * - Try Anthropic first (typical paid-subscription setup), then OpenAI, then local
 * - Avoid nothing by default
 * - Do not prefer cheap cloud over local (operator can opt-in)
 * - Require preflight for large runs
 */
export function getDefaultPolicy(): ProviderRoutingPolicy {
  return {
    providerOrder: ["anthropic", "openai", "local"],
    avoidProviders: [],
    cheapCloudPreferredOverLocal: false,
    requirePreflightForLargeRuns: true,
  };
}
