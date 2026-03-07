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
import { Type } from "@sinclair/typebox";
import { StringEnum } from "./lib/typebox-helpers";
import { sharedState } from "./shared-state.ts";

/** Model tier ordering for effort cap comparison. */
export const TIER_ORDER: Record<string, number> = { local: 0, haiku: 1, sonnet: 2, opus: 3 };

/**
 * Check whether an effort cap blocks a model tier switch.
 *
 * If sharedState.effort is capped and the requested tier is higher than the
 * cap's driver model, returns { blocked: true, message: "..." }.
 * Otherwise returns { blocked: false }.
 *
 * Exported for testing (extensions/effort/model-budget-cap.test.ts).
 */
export function checkEffortCap(requestedTier: string): { blocked: boolean; message?: string } {
  const effort = (sharedState as any).effort as
    | { capped?: boolean; driver?: string; name?: string; level?: number }
    | undefined;
  if (!effort?.capped) return { blocked: false };

  const requestedOrder = TIER_ORDER[requestedTier] ?? -1;
  const capOrder = TIER_ORDER[effort.driver ?? ""] ?? -1;

  if (requestedOrder > capOrder) {
    return {
      blocked: true,
      message:
        `Effort cap active: ${effort.name} (level ${effort.level}) limits driver to ${effort.driver}. ` +
        `Cannot upgrade to ${requestedTier}. Use /effort uncap to remove the ceiling.`,
    };
  }
  return { blocked: false };
}

/** Static tier metadata — model IDs resolved dynamically at runtime */
const TIER_META = {
  opus:   { prefix: "claude-opus",   label: "opus",   icon: "🧠" },
  sonnet: { prefix: "claude-sonnet", label: "sonnet", icon: "⚡" },
  haiku:  { prefix: "claude-haiku",  label: "haiku",  icon: "💨" },
} as const;

type TierName = keyof typeof TIER_META;

interface RegistryModel {
  id: string;
  provider: string;
  [key: string]: unknown;
}

/**
 * Find the best matching Anthropic model for a tier by prefix.
 * Picks the latest model ID alphabetically (higher version = later sort).
 * Pi-core prefers short aliases (claude-opus-4-6) over dated versions
 * (claude-opus-4-6-20250514), and lexicographic descending gets the alias.
 */
function findTierModel(ctx: any, tier: TierName): RegistryModel | undefined {
  const meta = TIER_META[tier];
  const all: RegistryModel[] = ctx.modelRegistry.getAll();
  const candidates = all
    .filter((m) => m.provider === "anthropic" && m.id.startsWith(meta.prefix))
    .sort((a, b) => b.id.localeCompare(a.id)); // latest version first
  return candidates[0] ?? undefined;
}

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

async function switchTo(tier: TierName, pi: ExtensionAPI, ctx: any): Promise<RegistryModel | null> {
  const model = findTierModel(ctx, tier);
  if (!model) return null;
  const success = await pi.setModel(model as any);
  return success ? model : null;
}

function currentTierName(ctx: ExtensionContext): TierName | null {
  const model = ctx.model;
  if (!model) return null;
  for (const [name, meta] of Object.entries(TIER_META)) {
    if (model.id.startsWith(meta.prefix)) return name as TierName;
  }
  return null;
}

export default function (pi: ExtensionAPI) {
  // session_start model selection is handled by the effort extension.
  // model-budget only provides the set_model_tier / set_thinking_level tools.

  // --- Model Tier Tool ---
  pi.registerTool({
    name: "set_model_tier",
    label: "Set Model Tier",
    description:
      "Switch the active model tier based on task complexity. " +
      "Use 'opus' for deep reasoning, architecture, and planning. " +
      "Use 'sonnet' for routine code edits, file operations, and execution. " +
      "Use 'haiku' for simple lookups, formatting, and boilerplate generation. " +
      "Downgrade when the current task is straightforward to conserve budget. " +
      "Upgrade when you encounter something that needs deeper reasoning.",
    promptSnippet: "Switch model tier (opus/sonnet/haiku) to match task complexity and conserve budget",
    promptGuidelines: [
      "Downgrade to sonnet for routine file edits, command execution, and cleanup tasks",
      "Upgrade to opus when encountering architecture decisions, complex debugging, or multi-step planning",
      "Use haiku for simple lookups, formatting, and boilerplate generation",
    ],
    parameters: Type.Object({
      tier: StringEnum(["opus", "sonnet", "haiku"], {
        description: "Target model tier",
      }),
      reason: Type.String({
        description: "Brief explanation for the tier change",
      }),
    }),
    execute: async (
      _toolCallId,
      params: { tier: string; reason: string },
      _signal,
      _onUpdate,
      ctx,
    ) => {
      const tier = params.tier as TierName;
      const meta = TIER_META[tier];

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
        ctx.ui.notify(`${meta.icon} → ${meta.label} (thinking: ${thinking}): ${params.reason}`, "info");
        return {
          content: [
            {
              type: "text" as const,
              text: `Switched to ${meta.label} (${model.id}), thinking: ${thinking}. ${params.reason}`,
            },
          ],
          details: undefined,
        };
      }
      return {
        content: [
          {
            type: "text" as const,
            text: `Failed to switch to ${meta.label} — no matching ${meta.prefix}-* model found or no API key`,
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
    parameters: Type.Object({
      level: StringEnum(["off", "minimal", "low", "medium", "high"], {
        description: "Thinking level — higher = more reasoning tokens, slower, more expensive",
      }),
      reason: Type.String({
        description: "Brief explanation for the thinking level change",
      }),
    }),
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
  for (const [name, meta] of Object.entries(TIER_META)) {
    pi.registerCommand(name, {
      description: `Switch to ${meta.label} (${meta.icon})`,
      handler: async (_args, ctx) => {
        const model = await switchTo(name as TierName, pi, ctx);
        if (!model) {
          ctx.ui.notify(`Failed to switch to ${meta.label}`, "error");
        }
      },
    });
  }
}
