/**
 * Shared provider-aware model resolver for Omegon.
 *
 * Keeps canonical compatibility tiers (local|retribution|victory|gloriana) stable while
 * also supporting public operator capability roles
 * (archmagos|magos|adept|servitor|servoskull).
 */

import { PREFERRED_ORDER } from "./local-models.ts";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ModelTier = "local" | "retribution" | "victory" | "gloriana";
/**
 * Well-known provider names for routing policy ordering and preference.
 * Any string is accepted at runtime (unknown providers participate in
 * capability-based matching) but these have explicit routing support.
 */
export type ProviderName = "anthropic" | "openai" | "github-copilot" | "google" | "amazon-bedrock" | "azure-openai-responses" | "xai" | "groq" | "mistral" | "openrouter" | "local" | (string & {});
export type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high";
export type CapabilityRole = "archmagos" | "magos" | "adept" | "servitor" | "servoskull";
export type CandidateSource = "upstream" | "local";
export type CandidateWeight = "light" | "normal" | "heavy" | "unknown";
export type FallbackDisposition = "allow" | "ask" | "deny";
export type UpstreamFailureClass =
  | "retryable-flake"
  | "rate-limit"
  | "backoff"
  | "auth"
  | "quota"
  | "tool-output"
  | "context-overflow"
  | "invalid-request"
  | "user-abort"
  | "non-retryable";
export type UpstreamRecoveryAction = "retry-same-model" | "failover" | "surface" | "handled-elsewhere";

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

export interface CapabilityCandidate {
  id: string;
  provider: ProviderName;
  source: CandidateSource;
  weight: CandidateWeight;
  maxThinking: ThinkingLevel;
}

export interface RoleProfile {
  candidates: CapabilityCandidate[];
}

export interface CapabilityProfilePolicy {
  sameRoleCrossProvider: FallbackDisposition;
  crossSource: FallbackDisposition;
  heavyLocal: FallbackDisposition;
  unknownLocalPerformance: FallbackDisposition;
}

export interface CapabilityProfile {
  roles: Record<CapabilityRole, RoleProfile>;
  internalAliases: Record<string, CapabilityRole>;
  policy: CapabilityProfilePolicy;
}

export interface CooldownEntry {
  until: number;
  reason?: string;
}

export interface CapabilityRuntimeState {
  candidateCooldowns?: Record<string, CooldownEntry>;
  providerCooldowns?: Partial<Record<ProviderName, CooldownEntry>>;
}

export interface UpstreamFailureClassification {
  class: UpstreamFailureClass;
  recoveryAction: UpstreamRecoveryAction;
  summary: string;
  reason: string;
  retryable: boolean;
  cooldownProvider: boolean;
  cooldownCandidate: boolean;
}

/**
 * Resolved concrete model for a requested tier.
 */
export interface ResolvedTierModel {
  tier: ModelTier;
  provider: ProviderName;
  modelId: string;
  maxThinking?: ThinkingLevel;
}

export interface ResolvedCapabilityCandidate {
  role: CapabilityRole;
  candidate: CapabilityCandidate;
}

export interface RoleResolution {
  ok: boolean;
  role: CapabilityRole;
  selected?: ResolvedCapabilityCandidate;
  blockedBy?: "cross-source" | "heavy-local" | "unknown-local-performance" | "denied";
  requiresConfirmation?: boolean;
  reason?: string;
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
// Constants
// ---------------------------------------------------------------------------

export const TRANSIENT_PROVIDER_COOLDOWN_MS = 5 * 60 * 1000;

const THINKING_ORDER: Record<ThinkingLevel, number> = {
  off: 0,
  minimal: 1,
  low: 2,
  medium: 3,
  high: 4,
};

const ROLE_COMPATIBILITY_MAP: Record<ModelTier, CapabilityRole> = {
  local: "servitor",
  retribution: "adept",
  victory: "magos",
  gloriana: "archmagos",
};

const TIER_DISPLAY_LABELS: Record<ModelTier, string> = {
  local: "Servitor [Local]",
  retribution: "Adept [Retribution Class]",
  victory: "Magos [Victory Class]",
  gloriana: "Archmagos [Gloriana Class]",
};

const ROLE_DISPLAY_LABELS: Record<CapabilityRole, string> = {
  archmagos: "Archmagos",
  magos: "Magos",
  adept: "Adept",
  servitor: "Servitor",
  servoskull: "Servoskull",
};

// ---------------------------------------------------------------------------
// Universal model-to-tier classification (provider-transparent)
// ---------------------------------------------------------------------------
// These patterns match model IDs regardless of provider. A github-copilot
// model "claude-opus-4-6" matches the same rules as anthropic's.
// Order within each tier matters — earlier entries are preferred.
// ---------------------------------------------------------------------------

interface TierRule {
  exact?: string;
  prefix?: string;
  weight: CandidateWeight;
  maxThinking: ThinkingLevel;
  source: CandidateSource;
}

const TIER_RULES: Record<Exclude<ModelTier, "local">, TierRule[]> = {
  gloriana: [
    { prefix: "claude-opus", weight: "heavy", maxThinking: "high", source: "upstream" },
    { exact: "gpt-5.4", weight: "heavy", maxThinking: "high", source: "upstream" },
    { prefix: "gpt-5.4-", weight: "heavy", maxThinking: "high", source: "upstream" },
    { prefix: "gemini-3-pro", weight: "heavy", maxThinking: "high", source: "upstream" },
    { prefix: "gemini-3.1-pro", weight: "heavy", maxThinking: "high", source: "upstream" },
  ],
  victory: [
    { prefix: "claude-sonnet", weight: "normal", maxThinking: "high", source: "upstream" },
    { prefix: "gpt-5.3-codex", weight: "normal", maxThinking: "medium", source: "upstream" },
    { exact: "gpt-5.3", weight: "normal", maxThinking: "medium", source: "upstream" },
    { prefix: "gpt-5.2-codex", weight: "normal", maxThinking: "medium", source: "upstream" },
    { exact: "gpt-5.2", weight: "normal", maxThinking: "medium", source: "upstream" },
    { prefix: "gemini-3-flash", weight: "normal", maxThinking: "medium", source: "upstream" },
    { prefix: "gemini-2.5-pro", weight: "normal", maxThinking: "medium", source: "upstream" },
    { prefix: "grok-", weight: "normal", maxThinking: "medium", source: "upstream" },
  ],
  retribution: [
    { prefix: "claude-haiku", weight: "light", maxThinking: "low", source: "upstream" },
    { prefix: "gpt-5.1-codex", weight: "light", maxThinking: "low", source: "upstream" },
    { exact: "gpt-5.1", weight: "light", maxThinking: "low", source: "upstream" },
    { prefix: "gpt-5-mini", weight: "light", maxThinking: "low", source: "upstream" },
    { prefix: "gpt-5-nano", weight: "light", maxThinking: "low", source: "upstream" },
    { prefix: "gemini-2.0-flash", weight: "light", maxThinking: "low", source: "upstream" },
    { prefix: "mistral-large", weight: "light", maxThinking: "low", source: "upstream" },
    { prefix: "codestral", weight: "light", maxThinking: "low", source: "upstream" },
  ],
};

// Legacy aliases — used only by matchAnthropicTier/matchOpenAITier for
// backward compat with operator profiles that reference these constants.
const ANTHROPIC_TIER_PREFIXES: Record<Exclude<ModelTier, "local">, string[]> = {
  retribution: ["claude-haiku"],
  victory: ["claude-sonnet"],
  gloriana: ["claude-opus"],
};

// Deprecated models that should never be selected even if the provider
// still lists them. Prevents routing to dead or EOL endpoints.
// Updated 2026-03-14: GPT-4.1, GPT-4.1 mini, o4-mini retired Feb 13 2026.
const DEPRECATED_MODELS = new Set([
  "gpt-4o", "gpt-4o-mini",
  "gpt-4-turbo", "gpt-4",
  "gpt-3.5-turbo",
  "gpt-4.1", "gpt-4.1-mini", "gpt-4.1-nano",
  "o4-mini",
  "gpt-5", "gpt-5-instant", "gpt-5-thinking",  // retired Feb 13 2026
  "claude-3-haiku-20240307", "claude-3-sonnet-20240229", "claude-3-opus-20240229",
  "claude-3-5-sonnet-20240620",  // ancient snapshots
]);

// o3 is a specialized reasoning model — not suitable as a general-purpose
// archmagos candidate. It belongs in a dedicated "reasoning" role if needed.

/**
 * Filter deprecated models from a registry snapshot.
 *
 * Call this on `getAvailable()` output before passing to `resolveTier()`.
 * Centralizes deprecation logic so callsites don't need to know the list.
 */
export function filterDeprecated(models: RegistryModel[]): RegistryModel[] {
  return models.filter((m) => !DEPRECATED_MODELS.has(m.id));
}

/**
 * Get the viable model pool: auth'd + non-deprecated.
 *
 * Single entry point for all routing callsites. Uses `getAvailable()` (only
 * models with configured auth) then strips deprecated models.
 *
 * Pass the result to `resolveTier()`, `getDefaultCapabilityProfile()`, etc.
 */
export function getViableModels(registry: { getAvailable(): { id: string; provider: string }[] }): RegistryModel[] {
  return filterDeprecated(registry.getAvailable() as unknown as RegistryModel[]);
}
const OPENAI_TIER_MODELS: Record<Exclude<ModelTier, "local">, string[]> = {
  retribution: ["gpt-5.1-codex", "gpt-5.1"],
  victory: ["gpt-5.3-codex-spark", "gpt-5.3", "gpt-5.2-codex", "gpt-5.2"],
  gloriana: ["gpt-5.4"],
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function parseModelVersion(id: string): number[] {
  const parts = id.split("-");
  const versions: number[] = [];
  for (let i = parts.length - 1; i >= 0; i--) {
    const n = parseInt(parts[i], 10);
    if (!isNaN(n)) versions.unshift(n);
    else break;
  }
  return versions;
}

function compareModelVersionsDesc(a: string, b: string): number {
  const va = parseModelVersion(a);
  const vb = parseModelVersion(b);
  for (let i = 0; i < Math.max(va.length, vb.length); i++) {
    const diff = (vb[i] ?? 0) - (va[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

function matchAnthropicTier(models: RegistryModel[], tier: Exclude<ModelTier, "local">): RegistryModel | undefined {
  const prefixes = ANTHROPIC_TIER_PREFIXES[tier];
  for (const prefix of prefixes) {
    const candidates = models
      .filter((m) => m.provider === "anthropic" && m.id.startsWith(prefix))
      .sort((a, b) => compareModelVersionsDesc(a.id, b.id));
    if (candidates.length > 0) return candidates[0];
  }
  return undefined;
}

function matchOpenAITier(models: RegistryModel[], tier: Exclude<ModelTier, "local">): RegistryModel | undefined {
  const exactIds = OPENAI_TIER_MODELS[tier];
  for (const modelId of exactIds) {
    if (DEPRECATED_MODELS.has(modelId)) continue;
    const match = models.find((m) => m.provider === "openai" && m.id === modelId);
    if (match) return match;
  }
  const exactIdSet = new Set(exactIds);
  const prefixFallbacks: Record<string, string[]> = {
    retribution: ["gpt-5.1-"],
    victory: ["gpt-5.2-", "gpt-5.3-"],
    gloriana: ["gpt-5.4-"],
  };
  for (const prefix of prefixFallbacks[tier] ?? []) {
    const found = models.find(
      (m) => m.provider === "openai" && m.id.startsWith(prefix)
        && !exactIdSet.has(m.id) && !DEPRECATED_MODELS.has(m.id),
    );
    if (found) return found;
  }
  return undefined;
}

function matchLocalTier(models: RegistryModel[]): RegistryModel | undefined {
  const locals = models.filter((m) => m.provider === "local");
  if (locals.length === 0) return undefined;
  for (const preferred of PREFERRED_ORDER) {
    const match = locals.find((m) => m.id === preferred);
    if (match) return match;
  }
  return locals[0];
}

// ---------------------------------------------------------------------------
// Provider-transparent tier matching
// ---------------------------------------------------------------------------

/**
 * Classify a model into a tier using TIER_RULES.
 * Returns the matching rule or undefined if no rule matches.
 */
export function classifyModelTier(modelId: string): { tier: Exclude<ModelTier, "local">; rule: TierRule } | undefined {
  for (const tier of ["gloriana", "victory", "retribution"] as const) {
    for (const rule of TIER_RULES[tier]) {
      if (rule.exact && modelId === rule.exact) return { tier, rule };
      if (rule.prefix && modelId.startsWith(rule.prefix)) return { tier, rule };
    }
  }
  return undefined;
}

/**
 * Match ALL viable models to a tier, across every provider.
 * Returns candidates sorted by TIER_RULES preference order (earlier rules = preferred).
 * Within the same rule, models are sorted by version descending (newest first).
 */
function matchTierUniversal(
  models: RegistryModel[],
  tier: Exclude<ModelTier, "local">,
): Array<{ model: RegistryModel; rule: TierRule }> {
  const rules = TIER_RULES[tier];
  const results: Array<{ model: RegistryModel; rule: TierRule; ruleIndex: number }> = [];

  for (const model of models) {
    if (model.provider === "local") continue; // local handled separately
    if (DEPRECATED_MODELS.has(model.id)) continue;
    for (let i = 0; i < rules.length; i++) {
      const rule = rules[i];
      const matches = rule.exact
        ? model.id === rule.exact
        : rule.prefix ? model.id.startsWith(rule.prefix) : false;
      if (matches) {
        results.push({ model, rule, ruleIndex: i });
        break; // first matching rule wins for this model
      }
    }
  }

  // Sort: rule priority first, then newest version within same rule
  results.sort((a, b) => {
    if (a.ruleIndex !== b.ruleIndex) return a.ruleIndex - b.ruleIndex;
    return compareModelVersionsDesc(a.model.id, b.model.id);
  });

  return results.map(({ model, rule }) => ({ model, rule }));
}

function dedupeProviderOrder(order: ProviderName[], avoided: ProviderName[]): ProviderName[] {
  const seen = new Set<ProviderName>();
  const result: ProviderName[] = [];
  for (const p of order) {
    if (!seen.has(p)) {
      seen.add(p);
      result.push(p);
    }
  }
  for (const p of avoided) {
    if (!seen.has(p)) {
      seen.add(p);
      result.push(p);
    }
  }
  return result;
}

function getCandidateKey(candidate: CapabilityCandidate): string {
  return `${candidate.provider}/${candidate.id}`;
}

function isCandidateCooledDown(
  candidate: CapabilityCandidate,
  runtimeState: CapabilityRuntimeState | undefined,
  now: number,
): boolean {
  const candidateCooldown = runtimeState?.candidateCooldowns?.[getCandidateKey(candidate)];
  if (candidateCooldown && candidateCooldown.until > now) return true;
  const providerCooldown = runtimeState?.providerCooldowns?.[candidate.provider];
  return Boolean(providerCooldown && providerCooldown.until > now);
}

function registryHasCandidate(candidate: CapabilityCandidate, models: RegistryModel[]): boolean {
  return models.some((m) => m.provider === candidate.provider && m.id === candidate.id);
}

function applyProviderPolicyOrder(
  candidates: CapabilityCandidate[],
  policy: ProviderRoutingPolicy,
): CapabilityCandidate[] {
  const providerOrder = dedupeProviderOrder(policy.providerOrder, policy.avoidProviders);
  const providerRank = new Map(providerOrder.map((provider, index) => [provider, index]));
  return [...candidates].sort((a, b) => {
    const aRank = providerRank.get(a.provider) ?? Number.MAX_SAFE_INTEGER;
    const bRank = providerRank.get(b.provider) ?? Number.MAX_SAFE_INTEGER;
    if (aRank !== bRank) return aRank - bRank;
    return candidates.indexOf(a) - candidates.indexOf(b);
  });
}

function fallbackDispositionForCandidate(
  firstCandidate: CapabilityCandidate | undefined,
  candidate: CapabilityCandidate,
  profilePolicy: CapabilityProfilePolicy,
): { blockedBy?: RoleResolution["blockedBy"]; disposition: FallbackDisposition } {
  if (!firstCandidate) return { disposition: "allow" };
  if (candidate.source !== firstCandidate.source) {
    return { blockedBy: "cross-source", disposition: profilePolicy.crossSource };
  }
  if (candidate.provider !== firstCandidate.provider) {
    return { disposition: profilePolicy.sameRoleCrossProvider };
  }
  if (candidate.source === "local" && candidate.weight === "heavy") {
    return { blockedBy: "heavy-local", disposition: profilePolicy.heavyLocal };
  }
  if (candidate.source === "local" && candidate.weight === "unknown") {
    return { blockedBy: "unknown-local-performance", disposition: profilePolicy.unknownLocalPerformance };
  }
  return { disposition: "allow" };
}

function explainBlockedResolution(
  role: CapabilityRole,
  candidate: CapabilityCandidate,
  blockedBy: NonNullable<RoleResolution["blockedBy"]>,
  disposition: FallbackDisposition,
): string {
  const roleLabel = getRoleDisplayLabel(role);
  const target = `${candidate.provider}/${candidate.id}`;
  const reason = blockedBy === "cross-source"
    ? `cross-source fallback to ${candidate.source}`
    : blockedBy === "heavy-local"
      ? "heavy local fallback"
      : blockedBy === "unknown-local-performance"
        ? "unknown local performance"
        : "policy";
  if (disposition === "ask") {
    return `${roleLabel} resolution requires operator confirmation before ${reason} via ${target}.`;
  }
  return `${roleLabel} resolution blocked by policy: ${reason} via ${target} is not permitted.`;
}

function inferWeightFromModel(model: RegistryModel): CandidateWeight {
  const id = model.id.toLowerCase();
  if (id.includes("70b") || id.includes("72b") || id.includes("30b") || id.includes("32b") || id.includes("24b")) {
    return "heavy";
  }
  if (id.includes("14b") || id.includes("8b")) return "normal";
  return "light";
}

function classifyFailureMessage(message: string): UpstreamFailureClassification {
  const normalized = message.toLowerCase();

  const patterns: Array<{
    match: boolean;
    classification: UpstreamFailureClassification;
  }> = [
    {
      // User-initiated cancellation: Esc in pi, SIGINT, AbortController.abort(), etc.
      // These are NOT upstream API failures and must never surface as recovery events.
      match: ["operation aborted", "command aborted", "user aborted", "abortederror", "request aborted", "abort was called"].some((needle) => normalized.includes(needle))
        || normalized === "aborted",
      classification: {
        class: "user-abort",
        recoveryAction: "handled-elsewhere",
        summary: "user-initiated abort",
        reason: "The operation was cancelled by the user (Esc / SIGINT / AbortSignal). No upstream failure occurred.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["context window", "context length", "too many tokens", "maximum context", "prompt is too long"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "context-overflow",
        recoveryAction: "handled-elsewhere",
        summary: "context overflow",
        reason: "Context overflow should be handled by explicit compaction/context management logic.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["invalid api key", "authentication", "unauthorized", "forbidden", "permission denied", "auth failed", "401", "403"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "auth",
        recoveryAction: "surface",
        summary: "authentication failure",
        reason: "Authentication and authorization failures are not generic transient retries.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["quota exceeded", "insufficient_quota", "hard quota", "billing", "credits", "usage limit exceeded"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "quota",
        recoveryAction: "surface",
        summary: "quota exhaustion",
        reason: "Hard quota exhaustion requires explicit operator or provider action.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["malformed tool output", "invalid tool output", "tool result schema", "tool output parse", "tool call parse", "schema validation", "malformed json", "invalid json", "structured output"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "tool-output",
        recoveryAction: "surface",
        summary: "malformed tool output",
        reason: "Malformed tool output should be surfaced explicitly rather than retried as an upstream flake.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["429", "rate limit", "rate-limit", "too many requests", "session limit"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "rate-limit",
        recoveryAction: "failover",
        summary: "rate limited",
        reason: "Rate limits and session limits should cool down the failing route and prefer failover.",
        retryable: false,
        cooldownProvider: true,
        cooldownCandidate: true,
      },
    },
    {
      match: ["try again later", "backoff", "retry-after", "retry after", "temporarily unavailable", "temporarily blocked"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "backoff",
        recoveryAction: "failover",
        summary: "explicit backoff",
        reason: "Explicit backoff guidance should avoid an immediate retry on the same provider/model.",
        retryable: false,
        cooldownProvider: true,
        cooldownCandidate: true,
      },
    },
    {
      match: ["image dimensions exceed", "image.source.base64.data", "image too large", "image size exceeds", "max allowed size: 8000"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "invalid-request",
        recoveryAction: "surface",
        summary: "image too large for API (max 8000px per dimension)",
        reason: "An image in the conversation exceeds the API's 8000px dimension limit. Resize or remove the image before retrying.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["invalid_request_error", "invalid request", "malformed request", "bad request"].some((needle) => normalized.includes(needle)) && !["rate limit", "429", "quota", "authentication", "unauthorized"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "invalid-request",
        recoveryAction: "surface",
        summary: "invalid API request",
        reason: "The request was rejected by the API as malformed or invalid. Check the error details and fix the request payload.",
        retryable: false,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
    {
      match: ["server_error", "internal server error", "bad gateway", "gateway timeout", "timed out", "timeout", "econnreset", "socket hang up", "overloaded", "5xx", "502", "503", "504"].some((needle) => normalized.includes(needle)),
      classification: {
        class: "retryable-flake",
        recoveryAction: "retry-same-model",
        summary: "transient upstream flake",
        reason: "Obvious upstream flakiness is eligible for one bounded retry on the same model.",
        retryable: true,
        cooldownProvider: false,
        cooldownCandidate: false,
      },
    },
  ];

  for (const entry of patterns) {
    if (entry.match) return entry.classification;
  }

  return {
    class: "non-retryable",
    recoveryAction: "surface",
    summary: "non-retryable upstream failure",
    reason: "The failure does not match a known transient, failover, or separately handled recovery class.",
    retryable: false,
    cooldownProvider: false,
    cooldownCandidate: false,
  };
}

export function clampThinkingLevel(requested: ThinkingLevel, maxThinking: ThinkingLevel): ThinkingLevel {
  return THINKING_ORDER[requested] <= THINKING_ORDER[maxThinking] ? requested : maxThinking;
}

export function classifyUpstreamFailure(error: unknown): UpstreamFailureClassification {
  const message = error instanceof Error ? error.message : String(error);
  return classifyFailureMessage(message);
}

export function classifyTransientFailure(error: unknown): boolean {
  return classifyUpstreamFailure(error).retryable || classifyUpstreamFailure(error).recoveryAction === "failover";
}

export function withProviderCooldown(
  runtimeState: CapabilityRuntimeState | undefined,
  provider: ProviderName,
  reason: string,
  now: number = Date.now(),
  cooldownMs: number = TRANSIENT_PROVIDER_COOLDOWN_MS,
): CapabilityRuntimeState {
  return {
    candidateCooldowns: { ...(runtimeState?.candidateCooldowns ?? {}) },
    providerCooldowns: {
      ...(runtimeState?.providerCooldowns ?? {}),
      [provider]: { until: now + cooldownMs, reason },
    },
  };
}

export function withCandidateCooldown(
  runtimeState: CapabilityRuntimeState | undefined,
  candidate: CapabilityCandidate,
  reason: string,
  now: number = Date.now(),
  cooldownMs: number = TRANSIENT_PROVIDER_COOLDOWN_MS,
): CapabilityRuntimeState {
  return {
    candidateCooldowns: {
      ...(runtimeState?.candidateCooldowns ?? {}),
      [getCandidateKey(candidate)]: { until: now + cooldownMs, reason },
    },
    providerCooldowns: { ...(runtimeState?.providerCooldowns ?? {}) },
  };
}

export function getDefaultCapabilityProfile(models: RegistryModel[] = []): CapabilityProfile {
  const local = matchLocalTier(models);

  // Build candidate lists from ALL available models using universal tier rules.
  // Deduplicate by model ID — a model may appear from multiple providers
  // (e.g. claude-opus-4-6 via both 'anthropic' and 'github-copilot'),
  // but we want each unique provider+id pair as a separate candidate so the
  // policy engine can prefer one provider over another.
  function buildCandidates(tier: Exclude<ModelTier, "local">): CapabilityCandidate[] {
    const matches = matchTierUniversal(models, tier);
    const seen = new Set<string>();
    const candidates: CapabilityCandidate[] = [];
    for (const { model, rule } of matches) {
      const key = `${model.provider}/${model.id}`;
      if (seen.has(key)) continue;
      seen.add(key);
      candidates.push({
        id: model.id,
        provider: model.provider as ProviderName,
        source: rule.source,
        weight: rule.weight,
        maxThinking: rule.maxThinking,
      });
    }
    return candidates;
  }

  const archmagosCandidates = buildCandidates("gloriana");
  const magosCandidates = buildCandidates("victory");
  const adeptCandidates = buildCandidates("retribution");

  // Servitor: reuse adept candidates (cheapest cloud) + local
  const servitorCandidates: CapabilityCandidate[] = [
    ...adeptCandidates.map((c) => ({ ...c, maxThinking: "low" as ThinkingLevel })),
  ];
  if (local) servitorCandidates.push({ id: local.id, provider: "local", source: "local", weight: inferWeightFromModel(local), maxThinking: "medium" });

  // Servoskull: local first, then cheapest cloud
  const servoskullCandidates: CapabilityCandidate[] = [];
  if (local) servoskullCandidates.push({ id: local.id, provider: "local", source: "local", weight: inferWeightFromModel(local), maxThinking: "off" });
  servoskullCandidates.push(
    ...adeptCandidates.map((c) => ({ ...c, maxThinking: "off" as ThinkingLevel })),
  );

  return {
    roles: {
      archmagos: { candidates: archmagosCandidates },
      magos: { candidates: magosCandidates },
      adept: { candidates: adeptCandidates },
      servitor: { candidates: servitorCandidates },
      servoskull: { candidates: servoskullCandidates },
    },
    internalAliases: {
      gloriana: "archmagos",
      victory: "magos",
      retribution: "adept",
      local: "servitor",
      review: "archmagos",
      planning: "archmagos",
      compaction: "servitor",
      extraction: "servitor",
      "cleave.leaf": "adept",
      summary: "servoskull",
      background: "servoskull",
    },
    policy: {
      sameRoleCrossProvider: "allow",
      crossSource: "ask",
      heavyLocal: "ask",
      unknownLocalPerformance: "ask",
    },
  };
}

export function resolveCapabilityRole(
  requestedRole: CapabilityRole | string,
  models: RegistryModel[],
  policy: ProviderRoutingPolicy,
  profile: CapabilityProfile = getDefaultCapabilityProfile(models),
  runtimeState?: CapabilityRuntimeState,
  now: number = Date.now(),
): RoleResolution {
  const role = (profile.internalAliases[requestedRole] ?? requestedRole) as CapabilityRole;
  const roleProfile = profile.roles[role];
  if (!roleProfile) {
    return {
      ok: false,
      role: "servitor",
      blockedBy: "denied",
      reason: `Unknown capability role: ${requestedRole}`,
    };
  }

  const orderedCandidates = applyProviderPolicyOrder(roleProfile.candidates, policy);
  const firstCandidate = orderedCandidates[0];

  for (const candidate of orderedCandidates) {
    if (!registryHasCandidate(candidate, models)) continue;
    if (isCandidateCooledDown(candidate, runtimeState, now)) continue;

    const fallback = fallbackDispositionForCandidate(firstCandidate, candidate, profile.policy);
    if (fallback.disposition === "deny") {
      return {
        ok: false,
        role,
        blockedBy: fallback.blockedBy ?? "denied",
        reason: explainBlockedResolution(role, candidate, fallback.blockedBy ?? "denied", fallback.disposition),
      };
    }
    if (fallback.disposition === "ask") {
      return {
        ok: false,
        role,
        blockedBy: fallback.blockedBy ?? "denied",
        requiresConfirmation: true,
        reason: explainBlockedResolution(role, candidate, fallback.blockedBy ?? "denied", fallback.disposition),
      };
    }

    return {
      ok: true,
      role,
      selected: { role, candidate },
    };
  }

  return {
    ok: false,
    role,
    blockedBy: "denied",
    reason: `No viable candidate available for ${getRoleDisplayLabel(role)}.`,
  };
}

// ---------------------------------------------------------------------------
// Core compatibility resolver
// ---------------------------------------------------------------------------

/**
 * Resolve an abstract tier to a concrete {provider, modelId} using the
 * session routing policy and the available model registry.
 */
export function resolveTier(
  tier: ModelTier,
  models: RegistryModel[],
  policy: ProviderRoutingPolicy,
  runtimeState?: CapabilityRuntimeState,
  profile?: CapabilityProfile,
): ResolvedTierModel | undefined {
  if (tier === "local") {
    const local = matchLocalTier(models);
    if (!local) return undefined;
    return { tier, provider: "local", modelId: local.id, maxThinking: "high" };
  }

  const resolution = resolveCapabilityRole(
    ROLE_COMPATIBILITY_MAP[tier],
    models,
    policy,
    profile ?? getDefaultCapabilityProfile(models),
    runtimeState,
  );
  if (!resolution.ok || !resolution.selected) return undefined;
  return {
    tier,
    provider: resolution.selected.candidate.provider,
    modelId: resolution.selected.candidate.id,
    maxThinking: resolution.selected.candidate.maxThinking,
  };
}

// ---------------------------------------------------------------------------
// Display labels + defaults
// ---------------------------------------------------------------------------

export function getTierDisplayLabel(tier: ModelTier): string {
  return TIER_DISPLAY_LABELS[tier];
}

export function getRoleDisplayLabel(role: CapabilityRole): string {
  return ROLE_DISPLAY_LABELS[role];
}

export function getDefaultPolicy(): ProviderRoutingPolicy {
  return {
    providerOrder: ["anthropic", "openai", "github-copilot", "google", "xai", "groq", "mistral", "amazon-bedrock", "azure-openai-responses", "openrouter", "local"],
    avoidProviders: [],
    cheapCloudPreferredOverLocal: false,
    requirePreflightForLargeRuns: true,
  };
}
