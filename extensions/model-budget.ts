/**
 * model-budget — Model tier + thinking level control
 *
 * Provides two orthogonal levers for cost/capability tuning:
 *   1. Model tier: opus (deep) → sonnet (capable) → haiku (fast)
 *   2. Thinking level: off → minimal → low → medium → high
 *
 * The agent can adjust both independently. Combined, these give fine-grained
 * control: e.g., sonnet+high for moderate tasks that need careful reasoning,
 * or opus+low for broad context understanding with minimal deliberation.
 *
 * Tools:
 *   set_model_tier     — Switch model (opus/sonnet/haiku)
 *   set_thinking_level — Adjust extended thinking budget
 *
 * Commands:
 *   /opus, /sonnet, /haiku — Direct model switch
 */

import { createHash } from "node:crypto";
import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import type { ImageContent, Model, TextContent } from "@mariozechner/pi-ai";
import { DASHBOARD_UPDATE_EVENT, sharedState } from "./shared-state.ts";
import type { RecoveryEvent, RecoveryFailureClassification } from "./shared-state.ts";
import type { RecoveryAction, RecoveryCooldownSummary, RecoveryDashboardState, RecoveryTarget } from "./dashboard/types.ts";
import { tierConfig } from "./effort/tiers.ts";
import type { EffortLevel } from "./effort/types.ts";
import { clampThinkingLevel, classifyUpstreamFailure, getDefaultPolicy, getTierDisplayLabel, resolveTier, type CapabilityRuntimeState, type ModelTier, type RegistryModel, type UpstreamFailureClassification } from "./lib/model-routing.ts";
import { writeLastUsedModel } from "./lib/model-preferences.ts";
import { loadOperatorRuntimeState, readOperatorProfile, toCapabilityProfile, toCapabilityRuntimeState } from "./lib/operator-profile.ts";
import { buildFallbackGuidance, explainTierResolutionFailure, planRecoveryForModel, recordTransientFailureForModel, type RecoveryPlan } from "./lib/operator-fallback.ts";
import { switchToOfflineDriver } from "./offline-driver.ts";

/** Model tier ordering for effort cap comparison. */
export const TIER_ORDER: Record<string, number> = { local: 0, haiku: 1, sonnet: 2, opus: 3 };

/**
 * Check whether an effort cap blocks a model tier switch.
 *
 * Derives the ceiling from capLevel (the level at which the cap was set),
 * NOT from effort.driver (which reflects the current tier and changes
 * when the operator switches tiers mid-session).
 *
 * If sharedState.effort is capped and the requested tier is higher than the
 * cap ceiling's driver, returns { blocked: true, message: "..." }.
 * Otherwise returns { blocked: false }.
 *
 * Exported for testing (extensions/effort/model-budget-cap.test.ts).
 */
export function checkEffortCap(requestedTier: string): { blocked: boolean; message?: string } {
  const effort = (sharedState as any).effort as
    | { capped?: boolean; capLevel?: number; driver?: string; name?: string; level?: number }
    | undefined;
  if (!effort?.capped || effort.capLevel == null) return { blocked: false };

  // Derive the ceiling driver from the capLevel, not the current tier's driver.
  const capConfig = tierConfig(effort.capLevel as EffortLevel);
  const capDriver = capConfig.driver;

  const requestedOrder = TIER_ORDER[requestedTier] ?? -1;
  const capOrder = TIER_ORDER[capDriver] ?? -1;

  if (requestedOrder > capOrder) {
    return {
      blocked: true,
      message:
        `Effort cap active: ${capConfig.name} (level ${effort.capLevel}) limits driver to ${capDriver}. ` +
        `Cannot upgrade to ${requestedTier}. Use /effort uncap to remove the ceiling.`,
    };
  }
  return { blocked: false };
}

/** Tier icons for operator notifications */
const TIER_ICONS: Record<ModelTier, string> = {
  local:  "🤖",
  haiku:  "💨",
  sonnet: "⚡",
  opus:   "🧠",
};

type TierName = ModelTier;

// Thinking levels ordered by cost/depth (xhigh excluded — OpenAI-only)
const THINKING_LEVELS = ["off", "minimal", "low", "medium", "high"] as const;
type ThinkingLevelName = (typeof THINKING_LEVELS)[number];

const THINKING_LABELS: Record<ThinkingLevelName, { icon: string; label: string }> = {
  off: { icon: "⏭️", label: "no thinking" },
  minimal: { icon: "💭", label: "minimal thinking" },
  low: { icon: "💭", label: "low thinking" },
  medium: { icon: "🤔", label: "medium thinking" },
  high: { icon: "🧠", label: "deep thinking" },
};

const TIER_CAPABILITY_COPY: Record<TierName, string> = {
  local: "on-device local execution",
  haiku: "fast lightweight cloud tier",
  sonnet: "balanced capability tier",
  opus: "deep reasoning tier",
};

export function buildSetModelTierDescription(): string {
  return (
    "Switch the active capability tier based on task complexity. " +
    "pi-kit resolves the requested tier through the active provider routing policy, so the backing model may come from Anthropic, OpenAI, or local inference. " +
    "Use 'local' for on-device work, 'haiku' for simple lookups and boilerplate, 'sonnet' for routine coding and execution, and 'opus' for deep reasoning and architecture."
  );
}

export function buildTierCommandDescription(tier: TierName): string {
  return `Switch to ${getTierDisplayLabel(tier)} [${tier}] — ${TIER_CAPABILITY_COPY[tier]} via provider-aware routing`;
}

function getResolverInputs(ctx: ExtensionContext) {
  const policy = (sharedState as any).routingPolicy ?? getDefaultPolicy();
  const profile = toCapabilityProfile(readOperatorProfile(ctx.cwd));
  const runtimeState = toCapabilityRuntimeState(loadOperatorRuntimeState(ctx.cwd));
  return { policy, profile, runtimeState };
}

function getAssistantErrorMessage(message: unknown): string | undefined {
  if (!message || typeof message !== "object") return undefined;
  const record = message as { role?: string; errorMessage?: unknown };
  if (record.role !== "assistant" || typeof record.errorMessage !== "string" || !record.errorMessage.trim()) {
    return undefined;
  }
  return record.errorMessage;
}

function summarizeErrorMessage(errorMessage: string): string {
  return errorMessage.replace(/\s+/g, " ").trim().slice(0, 240);
}

function mapRecoveryFailureClassification(classification: UpstreamFailureClassification): {
  classification: RecoveryFailureClassification;
  retryable: boolean;
  guidance: string;
} {
  switch (classification.class) {
    case "retryable-flake":
      return {
        classification: "transient_server_error",
        retryable: true,
        guidance: "Obvious upstream flakiness can retry once on the same provider/model before escalation.",
      };
    case "rate-limit":
    case "backoff":
      return {
        classification: "rate_limited",
        retryable: false,
        guidance: "Rate limiting/backoff should cool down the failing route and prefer an alternate candidate instead of blind retry.",
      };
    case "auth":
      return {
        classification: "authentication_failed",
        retryable: false,
        guidance: "Authentication failed; refresh credentials or switch to a provider with a valid session.",
      };
    case "quota":
      return {
        classification: "quota_exhausted",
        retryable: false,
        guidance: "Quota exhaustion is not retryable; switch models/providers or restore quota before retrying.",
      };
    case "tool-output":
      return {
        classification: "malformed_output",
        retryable: false,
        guidance: "Malformed output should not use generic retry; adjust the prompt/schema or switch models explicitly.",
      };
    case "context-overflow":
      return {
        classification: "context_overflow",
        retryable: false,
        guidance: "Context overflow is handled separately; compact context or reduce prompt size before retrying.",
      };
    case "invalid-request":
      return {
        classification: "invalid_request",
        retryable: false,
        guidance: "The API rejected the request (e.g. image too large, malformed payload). Fix the request content before retrying.",
      };
    case "user-abort":
      return {
        classification: "unknown_upstream",
        retryable: false,
        guidance: "Operation was cancelled by the user; no recovery action needed.",
      };
    default:
      return {
        classification: "unknown_upstream",
        retryable: false,
        guidance: "Upstream failure was not safely classified for automatic retry; surface it and ask the operator to choose the next route.",
      };
  }
}

export function classifyRecoveryFailure(errorMessage: string): {
  classification: RecoveryFailureClassification;
  retryable: boolean;
  guidance: string;
} {
  return mapRecoveryFailureClassification(classifyUpstreamFailure(errorMessage));
}

function hashRecoveryRequestContent(content: string | (TextContent | ImageContent)[]): string {
  return createHash("sha1").update(JSON.stringify(content)).digest("hex").slice(0, 12);
}

function getRetryLedgerKey(provider: string, model: string, requestFingerprint: string): string {
  return `${provider}/${model}:${requestFingerprint}`;
}

function getLatestUserRecoveryRequest(ctx: ExtensionContext): {
  content: string | (TextContent | ImageContent)[];
  fingerprint: string;
} | undefined {
  const entries = ctx.sessionManager.getEntries();
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    if (entry.type !== "message" || entry.message.role !== "user") continue;
    const content = entry.message.content as string | (TextContent | ImageContent)[];
    return {
      content,
      fingerprint: hashRecoveryRequestContent(content),
    };
  }
  return undefined;
}

export function piCoreAutoRetryLikelyHandles(errorMessage: string): boolean {
  return /overloaded|rate.?limit|too many requests|429|500|502|503|504|service.?unavailable|server error|internal error|connection.?error|connection.?refused|other side closed|fetch failed|upstream.?connect|reset before headers|terminated|retry delay/i.test(errorMessage);
}

export function buildRecoveryEvent(params: {
  provider: string;
  model: string;
  turnIndex: number;
  errorMessage: string;
  retryCount: number;
  guidance: string;
  alternateCandidate?: { provider: string; id: string };
  cooldownApplied?: boolean;
}): RecoveryEvent {
  const classified = classifyRecoveryFailure(params.errorMessage);
  const retryAttempted = classified.retryable && params.retryCount < 1;
  return {
    provider: params.provider,
    model: params.model,
    turnIndex: params.turnIndex,
    classification: classified.classification,
    originalErrorSummary: summarizeErrorMessage(params.errorMessage),
    retryable: classified.retryable,
    disposition: classified.classification === "context_overflow"
      ? "handled_elsewhere"
      : retryAttempted
        ? "retry_same_model"
        : params.cooldownApplied || classified.classification === "rate_limited"
          ? "cooldown_and_failover"
          : classified.retryable
            ? "escalate"
            : "guidance_only",
    retryAttempted,
    retryCount: retryAttempted ? params.retryCount + 1 : params.retryCount,
    maxRetries: classified.retryable ? 1 : 0,
    guidance: params.guidance || classified.guidance,
    cooldownApplied: params.cooldownApplied,
    alternateCandidate: params.alternateCandidate
      ? { provider: params.alternateCandidate.provider, model: params.alternateCandidate.id }
      : undefined,
    timestamp: Date.now(),
  };
}

function buildRecoveryCooldowns(runtimeState: CapabilityRuntimeState | undefined): RecoveryCooldownSummary[] | undefined {
  if (!runtimeState) return undefined;
  const cooldowns: RecoveryCooldownSummary[] = [];

  for (const [provider, entry] of Object.entries(runtimeState.providerCooldowns ?? {})) {
    if (!entry) continue;
    cooldowns.push({
      scope: "provider",
      key: provider,
      provider,
      until: entry.until,
      reason: entry.reason,
    });
  }

  for (const [key, entry] of Object.entries(runtimeState.candidateCooldowns ?? {})) {
    const [provider, modelId] = key.split("/");
    cooldowns.push({
      scope: "candidate",
      key,
      provider,
      modelId,
      until: entry.until,
      reason: entry.reason,
    });
  }

  return cooldowns.length > 0 ? cooldowns.sort((a, b) => a.until - b.until) : undefined;
}

function mapRecoveryAction(plan: RecoveryPlan, escalated = false): RecoveryAction {
  if (escalated) return "escalate";
  switch (plan.action) {
    case "retry-same-model": return "retry";
    case "switch-model": return "switch_candidate";
    case "handoff-local": return "switch_offline";
    case "handled-elsewhere": return "observe";
    case "surface":
      return plan.classification.cooldownProvider || plan.classification.cooldownCandidate ? "cooldown" : "observe";
    default:
      return "observe";
  }
}

function buildRecoverySummary(plan: RecoveryPlan, target: RecoveryTarget | undefined, escalated = false): string {
  if (escalated) {
    return `Escalated ${plan.classification.summary} after recovery could not switch away from the failing route.`;
  }
  switch (plan.action) {
    case "retry-same-model":
      return `Retrying once on the same model after ${plan.classification.summary}.`;
    case "switch-model":
      return target?.modelId
        ? `Switching recovery to ${target.provider}/${target.modelId} after ${plan.classification.summary}.`
        : `Switching to an alternate candidate after ${plan.classification.summary}.`;
    case "handoff-local":
      return target?.label
        ? `Switching recovery to local driver ${target.label} after ${plan.classification.summary}.`
        : `Switching recovery to the local driver after ${plan.classification.summary}.`;
    case "handled-elsewhere":
      return `Observed ${plan.classification.summary}; recovery is handled by explicit compaction/context management logic.`;
    default:
      return `Observed ${plan.classification.summary}; ${plan.reason}`;
  }
}

export function buildRecoveryDashboardState(params: {
  recoveryEvent: RecoveryEvent;
  plan: RecoveryPlan;
  runtimeState?: CapabilityRuntimeState;
  target?: RecoveryTarget;
  escalated?: boolean;
}): RecoveryDashboardState {
  return {
    provider: params.recoveryEvent.provider,
    modelId: params.recoveryEvent.model,
    classification: params.recoveryEvent.classification,
    summary: buildRecoverySummary(params.plan, params.target, params.escalated ?? false),
    action: mapRecoveryAction(params.plan, params.escalated ?? false),
    retryCount: params.recoveryEvent.retryCount,
    maxRetries: params.recoveryEvent.maxRetries,
    attemptId: `${params.recoveryEvent.turnIndex}:${params.recoveryEvent.provider}/${params.recoveryEvent.model}`,
    timestamp: params.recoveryEvent.timestamp,
    escalated: params.escalated,
    target: params.target,
    cooldowns: buildRecoveryCooldowns(params.runtimeState),
  };
}

function buildRecoveryNotice(recoveryEvent: RecoveryEvent, dashboardState: RecoveryDashboardState): string {
  const target = dashboardState.target?.modelId
    ? `${dashboardState.target.provider}/${dashboardState.target.modelId}`
    : dashboardState.target?.provider;
  const retry = recoveryEvent.maxRetries > 0 ? `retry ${recoveryEvent.retryCount}/${recoveryEvent.maxRetries}` : undefined;
  return [
    `Recovery observed ${recoveryEvent.classification} for ${recoveryEvent.provider}/${recoveryEvent.model}.`,
    dashboardState.summary,
    retry,
    target ? `target ${target}` : undefined,
  ].filter(Boolean).join(" ");
}

export function shouldUseExtensionRetryFallback(errorMessage: string, retryAttempted: boolean): boolean {
  return retryAttempted && !piCoreAutoRetryLikelyHandles(errorMessage);
}

function scheduleExtensionRetry(
  pi: ExtensionAPI,
  request: { content: string | (TextContent | ImageContent)[]; fingerprint: string },
): void {
  setTimeout(() => {
    try {
      pi.sendUserMessage(request.content);
    } catch {
      // If the retry prompt itself fails to queue, the original recovery notice remains in-session.
    }
  }, 0);
}

async function applyRecoveryPlan(
  plan: RecoveryPlan,
  pi: ExtensionAPI,
  ctx: ExtensionContext,
): Promise<{ target?: RecoveryTarget; escalated?: boolean }> {
  if (plan.action === "switch-model" && plan.alternateCandidate) {
    const targetModel = ctx.modelRegistry.find(plan.alternateCandidate.provider, plan.alternateCandidate.id);
    if (!targetModel) return { escalated: true };
    const success = await pi.setModel(targetModel as Model<any>);
    return success
      ? { target: { provider: plan.alternateCandidate.provider, modelId: plan.alternateCandidate.id, label: `${plan.alternateCandidate.provider}/${plan.alternateCandidate.id}` } }
      : { escalated: true };
  }

  if (plan.action === "handoff-local") {
    const offline = await switchToOfflineDriver(pi, ctx as any, {
      preferredModel: plan.alternateCandidate?.id,
      automatic: true,
    });
    return offline.success
      ? { target: { provider: offline.provider, modelId: offline.modelId, label: offline.label } }
      : { escalated: true };
  }

  return {};
}

async function switchTo(tier: TierName, pi: ExtensionAPI, ctx: ExtensionContext): Promise<RegistryModel | null> {
  const all = ctx.modelRegistry.getAll() as unknown as RegistryModel[];
  const { policy, profile, runtimeState } = getResolverInputs(ctx);
  const resolved = resolveTier(tier, all, policy, runtimeState, profile);
  if (!resolved) return null;
  const model = all.find((m) => m.id === resolved.modelId);
  if (!model) return null;
  const success = await pi.setModel(model as unknown as Model<any>);
  if (success) {
    writeLastUsedModel(ctx.cwd, { provider: model.provider, modelId: model.id });
    const currentThinking = pi.getThinkingLevel() as ThinkingLevelName;
    const clampedThinking = clampThinkingLevel(currentThinking, resolved.maxThinking ?? "high");
    if (clampedThinking !== currentThinking) {
      pi.setThinkingLevel(clampedThinking as any);
    }
    return model;
  }
  return null;
}

function currentTierName(ctx: ExtensionContext): TierName | null {
  const model = ctx.model;
  if (!model) return null;
  // Resolve the current model against the registry using the shared resolver
  const all = ctx.modelRegistry.getAll() as unknown as RegistryModel[];
  const { policy, profile, runtimeState } = getResolverInputs(ctx);
  for (const tier of ["opus", "sonnet", "haiku", "local"] as TierName[]) {
    const resolved = resolveTier(tier, all, policy, runtimeState, profile);
    if (resolved?.modelId === model.id) return tier;
  }
  return null;
}

export default function (pi: ExtensionAPI) {
  // session_start model selection is handled by the effort extension.
  // model-budget only provides the set_model_tier / set_thinking_level tools.

  pi.on("turn_end", async (event, ctx) => {
    const errorMessage = getAssistantErrorMessage(event.message);
    if (!errorMessage) {
      if (sharedState.recoveryRetryCounts && Object.keys(sharedState.recoveryRetryCounts).length > 0) {
        sharedState.recoveryRetryCounts = {};
      }
      return;
    }

    // User-initiated aborts (Esc / SIGINT / AbortSignal) must never enter the
    // upstream recovery pipeline.  They are not API failures.
    const abortClassification = classifyUpstreamFailure(errorMessage);
    if (abortClassification.class === "user-abort") {
      // Clear any stale recovery state so the dashboard doesn't linger on the
      // previous recovery notice.
      sharedState.recovery = undefined;
      sharedState.latestRecoveryEvent = undefined;
      pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "model-budget", recovery: undefined });
      return;
    }

    if (!ctx.model) return;

    const provider = ctx.model.provider;
    const recoveryRequest = getLatestUserRecoveryRequest(ctx);
    const ledgerKey = getRetryLedgerKey(
      provider,
      ctx.model.id,
      recoveryRequest?.fingerprint ?? `turn-${event.turnIndex}`,
    );
    const retryCounts = sharedState.recoveryRetryCounts ?? {};
    const priorRetryCount = retryCounts[ledgerKey] ?? 0;

    const persistedRuntimeState = recordTransientFailureForModel(ctx.cwd, ctx.model, errorMessage);
    const { policy, profile } = getResolverInputs(ctx);
    const models = ctx.modelRegistry.getAll() as unknown as RegistryModel[];
    const runtimeState = persistedRuntimeState ?? toCapabilityRuntimeState(loadOperatorRuntimeState(ctx.cwd));
    const plan = planRecoveryForModel(ctx.model, errorMessage, models, policy, profile, runtimeState ?? {}, Date.now());
    const guidance = buildFallbackGuidance(ctx.model, models, policy, profile, runtimeState ?? {}, Date.now());
    const applied = await applyRecoveryPlan(plan, pi, ctx);
    const classified = mapRecoveryFailureClassification(classifyUpstreamFailure(errorMessage));
    const recoveryEvent = buildRecoveryEvent({
      provider,
      model: ctx.model.id,
      turnIndex: event.turnIndex,
      errorMessage,
      retryCount: priorRetryCount,
      guidance: plan.reason || guidance?.reason || classified.guidance,
      alternateCandidate: plan.alternateCandidate,
      cooldownApplied: Boolean(persistedRuntimeState),
    });
    const dashboardState = buildRecoveryDashboardState({
      recoveryEvent,
      plan,
      runtimeState,
      target: applied.target,
      escalated: applied.escalated,
    });
    const useExtensionRetryFallback = shouldUseExtensionRetryFallback(errorMessage, recoveryEvent.retryAttempted);

    sharedState.latestRecoveryEvent = recoveryEvent;
    sharedState.recovery = dashboardState;
    sharedState.recoveryRetryCounts = {
      ...retryCounts,
      [ledgerKey]: recoveryEvent.retryCount,
    };
    pi.sendMessage({
      customType: "recovery-event",
      content: buildRecoveryNotice(recoveryEvent, dashboardState),
      display: true,
      details: {
        recoveryEvent,
        recovery: dashboardState,
        plan: {
          action: plan.action,
          sameModelRetry: plan.sameModelRetry,
          reason: plan.reason,
          classification: plan.classification.class,
          role: plan.role,
          alternateCandidate: plan.alternateCandidate,
          retryStrategy: useExtensionRetryFallback ? "extension-sendUserMessage" : "core-auto-retry",
        },
      },
    }, { triggerTurn: false });
    pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "model-budget", recoveryEvent, recovery: dashboardState });

    if (useExtensionRetryFallback && recoveryRequest) {
      scheduleExtensionRetry(pi, recoveryRequest);
    }

    if (!ctx.hasUI) return;

    if (dashboardState.action === "retry") {
      ctx.ui.notify(dashboardState.summary, "warning");
      return;
    }

    if (dashboardState.action === "switch_candidate" || dashboardState.action === "switch_offline") {
      ctx.ui.notify(dashboardState.summary, applied.escalated ? "warning" : "info");
      return;
    }

    const level = recoveryEvent.disposition === "handled_elsewhere"
      ? "info"
      : guidance?.requiresConfirmation || applied.escalated
        ? "warning"
        : recoveryEvent.retryable
          ? "warning"
          : "error";
    ctx.ui.notify(dashboardState.summary, level);
  });

  const modelTierParameters = {
    type: "object",
    properties: {
      tier: {
        type: "string",
        enum: ["local", "haiku", "sonnet", "opus"],
        description: "Target model tier",
      },
      reason: {
        type: "string",
        description: "Brief explanation for the tier change",
      },
    },
    required: ["tier", "reason"],
    additionalProperties: false,
  } as const;

  const thinkingLevelParameters = {
    type: "object",
    properties: {
      level: {
        type: "string",
        enum: ["off", "minimal", "low", "medium", "high"],
        description: "Thinking level — higher = more reasoning tokens, slower, more expensive",
      },
      reason: {
        type: "string",
        description: "Brief explanation for the thinking level change",
      },
    },
    required: ["level", "reason"],
    additionalProperties: false,
  } as const;

  // --- Model Tier Tool ---
  pi.registerTool({
    name: "set_model_tier",
    label: "Set Model Tier",
    description: buildSetModelTierDescription(),
    promptSnippet: "Switch capability tier (local/haiku/sonnet/opus) through provider-aware routing",
    promptGuidelines: [
      "Downgrade to sonnet for routine file edits, command execution, and cleanup tasks",
      "Upgrade to opus when encountering architecture decisions, complex debugging, or multi-step planning",
      "Use haiku for simple lookups, formatting, and boilerplate generation",
    ],
    parameters: modelTierParameters as any,
    execute: async (
      _toolCallId,
      params: { tier: string; reason: string },
      _signal,
      _onUpdate,
      ctx,
    ) => {
      const tier = params.tier as TierName;
      const icon = TIER_ICONS[tier];
      const displayLabel = getTierDisplayLabel(tier);

      // Enforce effort cap — block upgrades past the ceiling
      const capCheck = checkEffortCap(tier);
      if (capCheck.blocked) {
        return {
          content: [{ type: "text" as const, text: capCheck.message! }],
          details: undefined,
        };
      }

      const model = await switchTo(tier, pi, ctx);
      if (model) {
        const thinking = pi.getThinkingLevel();
        const target = `${model.provider}/${model.id}`;
        ctx.ui.notify(`${icon} → ${displayLabel} [${tier}] → ${target} (thinking: ${thinking}): ${params.reason}`, "info");
        return {
          content: [
            {
              type: "text" as const,
              text: `Switched to ${displayLabel} [${tier}] via ${target}, thinking: ${thinking}. ${params.reason}`,
            },
          ],
          details: undefined,
        };
      }
      const { policy, profile, runtimeState } = getResolverInputs(ctx);
      const failure = explainTierResolutionFailure(
        tier,
        ctx.modelRegistry.getAll() as unknown as RegistryModel[],
        policy,
        profile,
        runtimeState,
      ) ?? `Failed to switch to ${displayLabel} [${tier}] — no matching model found or no API key`;
      return {
        content: [
          {
            type: "text" as const,
            text: failure,
          },
        ],
        details: undefined,
      };
    },
  });

  // --- Thinking Level Tool ---
  pi.registerTool({
    name: "set_thinking_level",
    label: "Set Thinking Level",
    description:
      "Adjust the extended thinking budget independently of model tier. " +
      "Higher levels allocate more tokens for internal reasoning before responding. " +
      "Use 'high' for complex multi-step problems, debugging, or architecture. " +
      "Use 'medium' (default) for general tasks. " +
      "Use 'low' or 'minimal' for straightforward execution where speed matters. " +
      "Use 'off' to disable extended thinking entirely (fastest, cheapest). " +
      "Thinking level and model tier are orthogonal — adjust both for fine-grained control.",
    promptSnippet: "Adjust extended thinking budget (off/minimal/low/medium/high)",
    promptGuidelines: [
      "Reduce thinking for mechanical tasks: file reads, grep, simple edits, formatting",
      "Increase thinking for: debugging, architecture decisions, complex refactors, multi-file changes",
      "Combine with model tier: sonnet+high is cheaper than opus+medium for moderate reasoning tasks",
    ],
    parameters: thinkingLevelParameters as any,
    execute: async (
      _toolCallId,
      params: { level: string; reason: string },
      _signal,
      _onUpdate,
      ctx,
    ) => {
      const previous = pi.getThinkingLevel();
      pi.setThinkingLevel(params.level as any);
      const level = params.level as ThinkingLevelName;
      const info = THINKING_LABELS[level];
      const tier = currentTierName(ctx) ?? "unknown";
      ctx.ui.notify(`${info.icon} thinking: ${previous} → ${level} (model: ${tier}): ${params.reason}`, "info");
      return {
        content: [
          {
            type: "text" as const,
            text: `Thinking: ${previous} → ${level} (${info.label}), model: ${tier}. ${params.reason}`,
          },
        ],
        details: undefined,
      };
    },
  });

  // --- Manual commands for direct control ---
  const COMMAND_TIERS: ModelTier[] = ["local", "haiku", "sonnet", "opus"];
  for (const tier of COMMAND_TIERS) {
    const icon = TIER_ICONS[tier];
    const displayLabel = getTierDisplayLabel(tier);
    pi.registerCommand(tier, {
      description: `${buildTierCommandDescription(tier)} (${icon})`,
      handler: async (_args, ctx) => {
        // Enforce effort cap — same check as the tool
        const capCheck = checkEffortCap(tier);
        if (capCheck.blocked) {
          ctx.ui.notify(`⛔ ${capCheck.message}`, "warning");
          return;
        }
        const model = await switchTo(tier, pi, ctx);
        if (!model) {
          const { policy, profile, runtimeState } = getResolverInputs(ctx);
          const failure = explainTierResolutionFailure(
            tier,
            ctx.modelRegistry.getAll() as unknown as RegistryModel[],
            policy,
            profile,
            runtimeState,
          );
          ctx.ui.notify(failure ?? `Failed to switch to ${displayLabel} [${tier}]`, "error");
        }
      },
    });
  }
}
