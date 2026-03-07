/**
 * effort — Global inference cost control extension.
 *
 * Provides a single `/effort` command to switch between 7 named tiers
 * (Servitor → Omnissiah), each controlling the driver model, thinking level,
 * and downstream settings for cleave dispatch, extraction, and compaction.
 *
 * On session_start: resolves the active tier from PI_EFFORT env var,
 * .pi/config.json, or default (Substantial), writes to sharedState.effort,
 * and switches the driver model + thinking level accordingly.
 *
 * Commands:
 *   /effort           — Show current tier info
 *   /effort <name>    — Switch to named tier
 *   /effort cap       — Lock ceiling at current tier
 *   /effort uncap     — Remove ceiling lock
 */

import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

import type { EffortLevel, EffortState, EffortModelTier } from "./types.ts";
import { EFFORT_NAMES } from "./types.ts";
import { tierConfig, parseTierName, DEFAULT_EFFORT_LEVEL, TIER_NAMES } from "./tiers.ts";
import { sharedState, DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";

// ─── Constants ───────────────────────────────────────────────

/** Tier icons indexed by level. */
const TIER_ICONS: Record<EffortLevel, string> = {
  1: "🟢",
  2: "🔵",
  3: "🟡",
  4: "🟠",
  5: "🔴",
  6: "💀",
  7: "⚙️",
};

/** Anthropic model prefix for each cloud driver tier. */
const MODEL_PREFIX: Record<string, string> = {
  sonnet: "claude-sonnet",
  opus: "claude-opus",
};

/** Ollama inference server URL. */
const OLLAMA_URL = process.env.LOCAL_INFERENCE_URL || "http://localhost:11434";

/** Preferred local driver models in priority order.
 *  Qwen3 32B leads — best all-round tool-call reliability for orchestration tasks.
 *  Falls back to models already wired in offline-driver if 32B isn't pulled yet.
 */
const PREFERRED_LOCAL = ["qwen3:32b", "nemotron-3-nano:30b", "devstral-small-2:24b", "qwen3:30b"];

// ─── Model Switching ─────────────────────────────────────────

interface RegistryModel {
  id: string;
  provider: string;
  [key: string]: unknown;
}

/**
 * Find the best Anthropic model for a tier prefix.
 * Uses the same strategy as model-budget.ts: lexicographic descending picks
 * the short alias (e.g. claude-opus-4-6) over dated versions.
 */
function findAnthropicModel(ctx: any, prefix: string): RegistryModel | undefined {
  const all: RegistryModel[] = ctx.modelRegistry.getAll();
  const candidates = all
    .filter((m) => m.provider === "anthropic" && m.id.startsWith(prefix))
    .sort((a, b) => b.id.localeCompare(a.id));
  return candidates[0] ?? undefined;
}

/**
 * Discover available Ollama chat models.
 */
async function discoverOllamaModels(): Promise<string[]> {
  try {
    const res = await fetch(`${OLLAMA_URL}/api/tags`, {
      signal: AbortSignal.timeout(3000),
    });
    if (!res.ok) return [];
    const data = (await res.json()) as { models?: { name: string }[] };
    return (data.models || [])
      .map((m) => m.name.replace(/:latest$/, ""))
      .filter((id) => !id.includes("embed"));
  } catch {
    return [];
  }
}

/**
 * Find a local model from the registry (already registered by offline-driver).
 * Falls back to discovering Ollama and picking from PREFERRED_LOCAL.
 */
async function findLocalModel(ctx: any): Promise<RegistryModel | undefined> {
  // Check if offline-driver already registered local models
  const all: RegistryModel[] = ctx.modelRegistry.getAll();
  const localModels = all.filter((m) => m.provider === "local");

  if (localModels.length > 0) {
    // Pick preferred order first, then any available
    for (const preferred of PREFERRED_LOCAL) {
      const match = localModels.find((m) => m.id === preferred);
      if (match) return match;
    }
    return localModels[0];
  }

  // No local models registered — Ollama may be available but offline-driver
  // hasn't loaded yet. Check if models are discoverable.
  const ollamaModels = await discoverOllamaModels();
  if (ollamaModels.length === 0) return undefined;

  // Pick best available from preferred list
  const targetId =
    PREFERRED_LOCAL.find((id) => ollamaModels.includes(id)) || ollamaModels[0];

  // Check registry again (offline-driver may have loaded between checks)
  return ctx.modelRegistry.find("local", targetId) ?? undefined;
}

/**
 * Switch the driver model to match the effort tier's driver setting.
 * Returns true if the switch succeeded.
 */
async function switchDriverModel(
  pi: ExtensionAPI,
  ctx: any,
  driver: EffortModelTier,
): Promise<boolean> {
  if (driver === "local") {
    const model = await findLocalModel(ctx);
    if (!model) return false;
    return pi.setModel(model as any);
  }

  const prefix = MODEL_PREFIX[driver];
  if (!prefix) return false;

  const model = findAnthropicModel(ctx, prefix);
  if (!model) return false;
  return pi.setModel(model as any);
}

// ─── Config Resolution ───────────────────────────────────────

/**
 * Read the effort tier from .pi/config.json in the project root.
 * Returns undefined if file doesn't exist or has no effort key.
 */
function readConfigEffort(cwd: string): string | undefined {
  try {
    const configPath = join(cwd, ".pi", "config.json");
    if (!existsSync(configPath)) return undefined;
    const raw = readFileSync(configPath, "utf-8");
    const parsed = JSON.parse(raw);
    return typeof parsed.effort === "string" ? parsed.effort : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Resolve the initial effort level from (in priority order):
 * 1. PI_EFFORT environment variable
 * 2. .pi/config.json effort field
 * 3. Default (Substantial, level 3)
 */
function resolveInitialLevel(cwd: string): EffortLevel {
  // 1. Environment variable
  const envValue = process.env.PI_EFFORT;
  if (envValue) {
    const level = parseTierName(envValue);
    if (level !== undefined) return level;
  }

  // 2. Config file
  const configValue = readConfigEffort(cwd);
  if (configValue) {
    const level = parseTierName(configValue);
    if (level !== undefined) return level;
  }

  // 3. Default
  return DEFAULT_EFFORT_LEVEL;
}

/**
 * Build an EffortState from a tier level.
 * Preserves existing cap state if provided.
 */
function buildEffortState(
  level: EffortLevel,
  capped: boolean = false,
  capLevel?: EffortLevel,
): EffortState {
  const config = tierConfig(level);
  return {
    ...config,
    capped,
    capLevel,
  };
}

// ─── Display Helpers ─────────────────────────────────────────

function formatTierInfo(state: EffortState): string {
  const icon = TIER_ICONS[state.level];
  const capIndicator = state.capped && state.capLevel
    ? ` [CAPPED at ${EFFORT_NAMES[state.capLevel]}]`
    : "";
  const lines = [
    `${icon} **${state.name}** (level ${state.level}/7)${capIndicator}`,
    `  Driver: ${state.driver} | Thinking: ${state.thinking}`,
    `  Extraction: ${state.extraction} | Compaction: ${state.compaction}`,
    `  Cleave: preferLocal=${state.cleavePreferLocal}, floor=${state.cleaveFloor}`,
    `  Review: ${state.reviewModel}`,
  ];
  return lines.join("\n");
}

// ─── Extension Entry Point ───────────────────────────────────

export default function (pi: ExtensionAPI) {
  // ── Session Start: resolve and apply effort tier ──

  pi.on("session_start", async (_event, ctx) => {
    const level = resolveInitialLevel(ctx.cwd);
    const state = buildEffortState(level);

    // Write to shared state
    sharedState.effort = state;
    pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "effort" });

    // Switch driver model
    const modelSwitched = await switchDriverModel(pi, ctx, state.driver);

    // Set thinking level
    pi.setThinkingLevel(state.thinking as any);

    // Notify operator
    const icon = TIER_ICONS[state.level];
    const modelNote = modelSwitched ? "" : " (driver model unavailable)";
    ctx.ui.notify(
      `${icon} Effort: ${state.name} (${state.driver}/${state.thinking})${modelNote}`,
      modelSwitched ? "info" : "warning",
    );
  });

  // ── /effort command ──

  pi.registerCommand("effort", {
    description: "View or change effort tier. Usage: /effort [tier|cap|uncap]",
    getArgumentCompletions: (prefix: string) => {
      const options = [...TIER_NAMES, "cap", "uncap"];
      const lower = prefix.toLowerCase();
      const matches = options.filter((o) => o.toLowerCase().startsWith(lower));
      return matches.map((name) => ({
        label: name,
        value: name,
      }));
    },
    handler: async (args, ctx) => {
      const arg = args.trim();

      // No args → show current tier
      if (!arg) {
        const state = sharedState.effort;
        if (!state) {
          ctx.ui.notify("⚠️ Effort state not initialized", "warning");
          return;
        }
        ctx.ui.notify(formatTierInfo(state), "info");
        return;
      }

      // /effort cap
      if (arg.toLowerCase() === "cap") {
        const state = sharedState.effort;
        if (!state) {
          ctx.ui.notify("⚠️ Effort state not initialized", "warning");
          return;
        }
        const icon = TIER_ICONS[state.level];
        sharedState.effort = buildEffortState(state.level, true, state.level);
        pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "effort" });
        ctx.ui.notify(
          `${icon} Effort capped at ${state.name} (level ${state.level}) — agent cannot upgrade past this tier`,
          "info",
        );
        return;
      }

      // /effort uncap
      if (arg.toLowerCase() === "uncap") {
        const state = sharedState.effort;
        if (!state) {
          ctx.ui.notify("⚠️ Effort state not initialized", "warning");
          return;
        }
        const icon = TIER_ICONS[state.level];
        sharedState.effort = buildEffortState(state.level, false);
        pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "effort" });
        ctx.ui.notify(
          `${icon} Effort cap removed — agent can freely upgrade`,
          "info",
        );
        return;
      }

      // /effort <tier name>
      const level = parseTierName(arg);
      if (level === undefined) {
        const valid = TIER_NAMES.map(
          (name, i) => `${TIER_ICONS[(i + 1) as EffortLevel]} ${name}`,
        ).join(", ");
        ctx.ui.notify(
          `❌ Unknown tier "${arg}". Valid tiers: ${valid}`,
          "error",
        );
        return;
      }

      // Preserve cap state on switch
      const prev = sharedState.effort;
      const capped = prev?.capped ?? false;
      const capLevel = prev?.capLevel;
      const state = buildEffortState(level, capped, capLevel);

      // Write to shared state
      sharedState.effort = state;
      pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "effort" });

      // Switch driver model
      const modelSwitched = await switchDriverModel(pi, ctx as any, state.driver);

      // Set thinking level
      pi.setThinkingLevel(state.thinking as any);

      const icon = TIER_ICONS[state.level];
      const modelNote = modelSwitched ? "" : " (driver model unavailable)";
      ctx.ui.notify(
        `${icon} Switched to ${state.name} (${state.driver}/${state.thinking})${modelNote}`,
        modelSwitched ? "info" : "warning",
      );
    },
  });
}
