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

const TIERS = {
  opus: { id: "claude-opus-4-6", label: "opus", icon: "🧠" },
  sonnet: { id: "claude-sonnet-4-6", label: "sonnet", icon: "⚡" },
  haiku: { id: "claude-haiku-4-5", label: "haiku", icon: "💨" },
} as const;

type TierName = keyof typeof TIERS;

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

async function switchTo(tier: TierName, pi: ExtensionAPI, ctx: any): Promise<boolean> {
  const spec = TIERS[tier];
  const model = ctx.modelRegistry.find("anthropic", spec.id);
  if (!model) return false;
  const success = await pi.setModel(model);
  return success;
}

function currentTierName(ctx: ExtensionContext): TierName | null {
  const model = ctx.model;
  if (!model) return null;
  for (const [name, spec] of Object.entries(TIERS)) {
    if (model.id === spec.id) return name as TierName;
  }
  return null;
}

export default function (pi: ExtensionAPI) {
  // Default to Opus on session start
  pi.on("session_start", async (_event, ctx) => {
    await switchTo("opus", pi, ctx);
  });

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
      params: { tier: TierName; reason: string },
      _signal,
      _onUpdate,
      ctx,
    ) => {
      const success = await switchTo(params.tier, pi, ctx);
      const spec = TIERS[params.tier];
      if (success) {
        const thinking = pi.getThinkingLevel();
        ctx.ui.notify(`${spec.icon} → ${spec.label} (thinking: ${thinking}): ${params.reason}`, "info");
        return {
          content: [
            {
              type: "text" as const,
              text: `Switched to ${spec.label} (${spec.id}), thinking: ${thinking}. ${params.reason}`,
            },
          ],
        };
      }
      return {
        content: [
          {
            type: "text" as const,
            text: `Failed to switch to ${spec.label} — model not found or no API key`,
          },
        ],
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
      params: { level: ThinkingLevelName; reason: string },
      _signal,
      _onUpdate,
      ctx,
    ) => {
      const previous = pi.getThinkingLevel();
      pi.setThinkingLevel(params.level);
      const info = THINKING_LABELS[params.level];
      const tier = currentTierName(ctx) ?? "unknown";
      ctx.ui.notify(`${info.icon} thinking: ${previous} → ${params.level} (model: ${tier}): ${params.reason}`, "info");
      return {
        content: [
          {
            type: "text" as const,
            text: `Thinking: ${previous} → ${params.level} (${info.label}), model: ${tier}. ${params.reason}`,
          },
        ],
      };
    },
  });

  // --- Manual commands for direct control ---
  for (const [name, spec] of Object.entries(TIERS)) {
    pi.registerCommand(name, {
      description: `Switch to ${spec.label} (${spec.icon})`,
      handler: async (_args, ctx) => {
        const success = await switchTo(name as TierName, pi, ctx);
        if (!success) {
          ctx.ui.notify(`Failed to switch to ${spec.label}`, "error");
        }
      },
    });
  }
}
