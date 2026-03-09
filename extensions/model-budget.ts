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

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import type { Model } from "@mariozechner/pi-ai";
import { sharedState } from "./shared-state.ts";
import { tierConfig } from "./effort/tiers.ts";
import type { EffortLevel } from "./effort/types.ts";
import { resolveTier, getTierDisplayLabel, getDefaultPolicy, clampThinkingLevel } from "./lib/model-routing.ts";
import type { ModelTier, RegistryModel } from "./lib/model-routing.ts";
import { writeLastUsedModel } from "./lib/model-preferences.ts";

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

async function switchTo(tier: TierName, pi: ExtensionAPI, ctx: ExtensionContext): Promise<RegistryModel | null> {
  const all = ctx.modelRegistry.getAll() as unknown as RegistryModel[];
  const policy = (sharedState as any).routingPolicy ?? getDefaultPolicy();
  const resolved = resolveTier(tier, all, policy);
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
  const policy = (sharedState as any).routingPolicy ?? getDefaultPolicy();
  for (const tier of ["opus", "sonnet", "haiku", "local"] as TierName[]) {
    const resolved = resolveTier(tier, all, policy);
    if (resolved?.modelId === model.id) return tier;
  }
  return null;
}

export default function (pi: ExtensionAPI) {
  // session_start model selection is handled by the effort extension.
  // model-budget only provides the set_model_tier / set_thinking_level tools.

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
      return {
        content: [
          {
            type: "text" as const,
            text: `Failed to switch to ${displayLabel} [${tier}] — no matching model found or no API key`,
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
          ctx.ui.notify(`Failed to switch to ${displayLabel} [${tier}]`, "error");
        }
      },
    });
  }
}
