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

import type { ExtensionAPI, ExtensionContext } from "@cwilson613/pi-coding-agent";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

import type { EffortLevel, EffortState, EffortModelTier, ThinkingLevel } from "./types.ts";
import { EFFORT_NAMES } from "./types.ts";
import { tierConfig, parseTierName, DEFAULT_EFFORT_LEVEL, TIER_NAMES } from "./tiers.ts";
import { sharedState, DASHBOARD_UPDATE_EVENT } from "../shared-state.ts";
import {
  resolveTier,
  getTierDisplayLabel,
  getDefaultPolicy,
  getViableModels,
  buildProviderSummary,
  matchTierUniversal as matchTierUniversalExport,
  clampThinkingLevel,
  type ModelTier,
  type RegistryModel,
} from "../lib/model-routing.ts";
import { readLastUsedModel, writeLastUsedModel } from "../lib/model-preferences.ts";
import { readOperatorProfile, loadOperatorRuntimeState, toCapabilityProfile, toCapabilityRuntimeState } from "../lib/operator-profile.ts";
import { PROVIDER_ENV_VARS, getProviderRemediationHint } from "../lib/provider-env.ts";

// ─── Constants ───────────────────────────────────────────────

/** Tier icons indexed by level. */
const TIER_ICONS: Record<EffortLevel, string> = {
  1: "○",
  2: "●",
  3: "†",
  4: "⚔",
  5: "☠",
  6: "💀",
  7: "🤖",
};

function getResolverInputs(ctx: ExtensionContext) {
  const policy = sharedState.routingPolicy ?? getDefaultPolicy();
  const profile = toCapabilityProfile(readOperatorProfile(ctx.cwd));
  const runtimeState = toCapabilityRuntimeState(loadOperatorRuntimeState(ctx.cwd));
  return { policy, profile, runtimeState };
}

// ─── Model Switching ─────────────────────────────────────────

/**
 * Switch the driver model to match the effort tier's driver setting.
 * Uses the shared resolveTier() resolver with the current session policy.
 * Returns true if the switch succeeded.
 *
 * C3: `all` is fetched once and indexed as a Map so the post-resolution lookup
 * is O(1) with no second linear scan. Both resolveTier and the model lookup
 * operate on the same snapshot.
 */
async function switchDriverModel(
  pi: ExtensionAPI,
  ctx: ExtensionContext,
  driver: EffortModelTier,
): Promise<{ model: RegistryModel; maxThinking?: ThinkingLevel } | null> {
  // Snapshot the registry once; both resolveTier and the model lookup use it
  const all = getViableModels(ctx.modelRegistry);
  // Build O(1) index over the same snapshot — no second linear scan (C3)
  const byKey = new Map(all.map((m) => [`${m.provider}/${m.id}`, m]));
  const { policy, profile, runtimeState } = getResolverInputs(ctx);
  const resolved = resolveTier(driver, all, policy, runtimeState, profile);
  if (!resolved) return null;
  // Direct map lookup — no second linear scan of `all`
  const model = byKey.get(`${resolved.provider}/${resolved.modelId}`);
  if (!model) return null;
  const success = await pi.setModel(model as any);
  return success ? { model, maxThinking: resolved.maxThinking as ThinkingLevel | undefined } : null;
}

async function restoreLastUsedModel(
  pi: ExtensionAPI,
  ctx: ExtensionContext,
): Promise<RegistryModel | null> {
  const persisted = readLastUsedModel(ctx.cwd);
  if (!persisted) return null;
  const model = ctx.modelRegistry.find(persisted.provider, persisted.modelId) as unknown as RegistryModel | undefined;
  if (!model) return null;
  const success = await pi.setModel(model as any);
  return success ? model : null;
}

/**
 * Resolve the effective extraction tier, honoring the session routing policy.
 *
 * When cheapCloudPreferredOverLocal is true and the effort tier's extraction
 * setting is "local", we upgrade to "retribution" (cheapest cloud tier) so that
 * background extraction work uses a cost-effective cloud model when available.
 * If no cloud model satisfies retribution, falls back to "local" transparently.
 *
 * Spec: "Extraction prefers cheap cloud when configured"
 *       "Offline or unavailable cloud falls back safely"
 */
function resolveExtractionTier(
  extraction: EffortModelTier,
  ctx: ExtensionContext,
): { displayTier: string; resolvedModelId?: string } {
  const { policy, profile, runtimeState } = getResolverInputs(ctx);
  const all = getViableModels(ctx.modelRegistry);

  // Determine effective tier: upgrade local→retribution when policy prefers cheap cloud
  const effectiveTier: ModelTier =
    policy.cheapCloudPreferredOverLocal && extraction === "local" ? "retribution" : extraction;

  const resolved = resolveTier(effectiveTier, all, policy, runtimeState, profile);

  // If cloud preferred but no cloud model matched, fall back to local.
  // We call resolveTier("local") rather than matchLocalTier() directly because
  // resolveTier is the public API. The cloud-preferring policy is passed through
  // intentionally — resolveTier's "local" path ignores policy entirely and goes
  // straight to matchLocalTier(), so the policy has no effect here. This is
  // safe and avoids importing the private matchLocalTier function.
  const final =
    resolved ?? (effectiveTier !== "local" ? resolveTier("local", all, policy, runtimeState, profile) : undefined);

  return {
    displayTier: final ? getTierDisplayLabel(final.tier) : getTierDisplayLabel(effectiveTier),
    resolvedModelId: final?.modelId,
  };
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
 * resolvedExtractionModelId is always initialized to undefined here;
 * callers must invoke resolveExtractionTier() and populate it before
 * writing to sharedState.effort (W2).
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
    resolvedExtractionModelId: undefined,
  };
}

// ─── Display Helpers ─────────────────────────────────────────

function formatTierInfo(state: EffortState): string {
  const icon = TIER_ICONS[state.level];
  const capIndicator = state.capped && state.capLevel
    ? ` [CAPPED at ${EFFORT_NAMES[state.capLevel]}]`
    : "";
  const driverLabel = getTierDisplayLabel(state.driver);
  const extractionLabel = getTierDisplayLabel(state.extraction);
  const compactionLabel = getTierDisplayLabel(state.compaction);
  const reviewLabel = getTierDisplayLabel(state.reviewModel);
  const floorLabel = getTierDisplayLabel(state.cleaveFloor);
  const lines = [
    `${icon} **${state.name}** (level ${state.level}/7)${capIndicator}`,
    `  Driver: ${driverLabel} (${state.driver}) | Thinking: ${state.thinking}`,
    `  Extraction: ${extractionLabel} (${state.extraction}) | Compaction: ${compactionLabel} (${state.compaction})`,
    `  Cleave: preferLocal=${state.cleavePreferLocal}, floor=${floorLabel} (${state.cleaveFloor})`,
    `  Review: ${reviewLabel} (${state.reviewModel})`,
  ];
  return lines.join("\n");
}

// ─── Extension Entry Point ───────────────────────────────────

export default function (pi: ExtensionAPI) {
  // ── Session Start: resolve and apply effort tier ──

  pi.on("session_start", async (_event, ctx) => {
    const level = resolveInitialLevel(ctx.cwd);
    const state = buildEffortState(level);

    // Resolve extraction tier under current routing policy (C1: spec compliance).
    // When cheapCloudPreferredOverLocal is true this upgrades local→retribution and
    // falls back to local if no cloud model is available.
    const extractionResolution = resolveExtractionTier(state.extraction, ctx);
    state.resolvedExtractionModelId = extractionResolution.resolvedModelId;

    // Write to shared state
    sharedState.effort = state;
    pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "effort" });

    // Restore the operator's last explicit model choice when possible.
    // If none is persisted (or it is no longer available), fall back to the
    // current effort tier's resolved driver. As a final guard, keep pi's
    // current startup model rather than warning about an unusable session when
    // a working driver is already present.
    const restoredModel = await restoreLastUsedModel(pi, ctx);
    const switchedDriver = restoredModel ? null : await switchDriverModel(pi, ctx, state.driver);
    const retainedModel = !restoredModel && !switchedDriver && ctx.model ? ctx.model : null;

    // Set thinking level, respecting candidate ceilings when the effort-driven
    // model switch produced a structured resolver result.
    const effectiveThinking: ThinkingLevel = switchedDriver?.maxThinking
      ? clampThinkingLevel(state.thinking, switchedDriver.maxThinking)
      : restoredModel || retainedModel
        ? state.thinking
        : state.thinking;
    pi.setThinkingLevel(effectiveThinking as any);

    // Notify operator — suppress the "no model" warning during first-run
    // (bootstrap handles consolidated guidance), but always show when a model
    // is resolved so the operator knows what's driving their session.
    const icon = TIER_ICONS[state.level];
    const hasModel = !!(restoredModel || switchedDriver || retainedModel);
    const modelNote = restoredModel
      ? ` → restored ${restoredModel.provider}/${restoredModel.id}`
      : switchedDriver
        ? ` → ${switchedDriver.model.provider}/${switchedDriver.model.id}`
        : retainedModel
          ? ` → kept ${retainedModel.provider}/${retainedModel.id} (preferred ${state.driver} unavailable)`
          : " (driver model unavailable)";
    if (hasModel || !sharedState.bootstrapPending) {
      ctx.ui.notify(
        `${icon} Effort: ${state.name} (${state.driver}/${effectiveThinking})${modelNote}`,
        hasModel ? "info" : "warning",
      );
    }

    // Provider summary — show what tiers are available
    try {
      const allModels = ctx.modelRegistry.getAll() as unknown as RegistryModel[];
      const viable = getViableModels(ctx.modelRegistry);
      const policy = sharedState.routingPolicy ?? getDefaultPolicy();
      const summary = buildProviderSummary(allModels, viable, policy);

      if (summary.level === 0 && !sharedState.bootstrapPending) {
        ctx.ui.notify("⚠ No providers configured. Run /bootstrap or /providers for setup hints.", "warning");
      } else if (summary.level < 3) {
        const parts: string[] = [];
        for (const t of summary.tiers) {
          const icon = t.status === "operational" ? "●" : t.status === "degraded" ? "◐" : "○";
          const detail = t.topCandidate ? ` ${t.topCandidate.provider}/${t.topCandidate.modelId}` : "";
          parts.push(`${icon} ${getTierDisplayLabel(t.tier)}${detail}`);
        }
        ctx.ui.notify(`Routing: ${parts.join("  ")}`, "info");
      }
      // level 3 (all operational) = silent — no need to clutter startup
    } catch {
      // Non-critical — don't break startup
    }
  });

  // ── /providers command ──

  pi.registerCommand("providers", {
    description: "Show provider auth status and tier routing summary",
    handler: async (_args, ctx) => {
      const allModels = ctx.modelRegistry.getAll() as unknown as RegistryModel[];
      const viable = getViableModels(ctx.modelRegistry);
      const policy = sharedState.routingPolicy ?? getDefaultPolicy();
      const summary = buildProviderSummary(allModels, viable, policy);

      const lines: string[] = [];

      // Auth status
      if (summary.authProviders.length > 0) {
        lines.push(`**Auth configured:** ${summary.authProviders.join(", ")}`);
      }
      if (summary.unauthProviders.length > 0) {
        const top = summary.unauthProviders.slice(0, 8);
        const more = summary.unauthProviders.length > 8 ? ` (+${summary.unauthProviders.length - 8} more)` : "";
        lines.push(`**No auth:** ${top.join(", ")}${more}`);
      }
      lines.push("");

      // Tier table
      lines.push("| Tier | Status | Provider | Model |");
      lines.push("|------|--------|----------|-------|");
      for (const t of summary.tiers) {
        const icon = t.status === "operational" ? "●" : t.status === "degraded" ? "◐" : "○";
        const status = `${icon} ${t.status}`;
        const provider = t.topCandidate?.provider ?? "—";
        const model = t.topCandidate?.modelId ?? "—";
        lines.push(`| ${getTierDisplayLabel(t.tier)} | ${status} | ${provider} | ${model} |`);
      }
      lines.push("");

      // Candidate detail
      for (const t of summary.tiers) {
        if (t.candidateCount > 1) {
          const matches = matchTierUniversalExport(viable, t.tier);
          const candidateList = matches.slice(0, 5).map((m) => `${m.model.provider}/${m.model.id}`).join(", ");
          const more = matches.length > 5 ? ` (+${matches.length - 5} more)` : "";
          lines.push(`**${getTierDisplayLabel(t.tier)} candidates:** ${candidateList}${more}`);
        }
      }

      // Remediation hints for unconfigured providers that could improve coverage
      const impairedTiers = summary.tiers.filter(t => t.status === "unavailable" || t.status === "degraded");
      if (impairedTiers.length > 0 && summary.unauthProviders.length > 0) {
        const hintLines: string[] = [];
        const shown = new Set<string>();
        for (const provider of summary.unauthProviders) {
          if (shown.size >= 5) break;
          const hint = getProviderRemediationHint(provider);
          if (hint && !shown.has(provider)) {
            shown.add(provider);
            const entry = PROVIDER_ENV_VARS[provider];
            const desc = entry?.description ?? provider;
            hintLines.push(`  ${provider} (${desc}): ${hint}`);
          }
        }
        if (hintLines.length > 0) {
          lines.push("**To configure providers:**");
          lines.push(...hintLines);
          lines.push("");
        }
      }

      lines.push(`**Headline:** ${summary.headline}`);

      pi.sendMessage({
        customType: "provider-summary",
        content: lines.join("\n"),
        display: true,
      });
    },
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
        const newState = buildEffortState(state.level, true, state.level);
        // Resolve extraction tier and populate before writing to sharedState (W1)
        const extractionResolution = resolveExtractionTier(newState.extraction, ctx);
        newState.resolvedExtractionModelId = extractionResolution.resolvedModelId;
        sharedState.effort = newState;
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
        const newState = buildEffortState(state.level, false);
        // Resolve extraction tier and populate before writing to sharedState (W1)
        const extractionResolution = resolveExtractionTier(newState.extraction, ctx);
        newState.resolvedExtractionModelId = extractionResolution.resolvedModelId;
        sharedState.effort = newState;
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

      // Resolve extraction tier before writing to sharedState (C1, C2)
      const extractionResolution = resolveExtractionTier(state.extraction, ctx);
      state.resolvedExtractionModelId = extractionResolution.resolvedModelId;

      // Write to shared state only after all fields are populated
      sharedState.effort = state;
      pi.events.emit(DASHBOARD_UPDATE_EVENT, { source: "effort" });

      // Switch driver model
      const driverModel = await switchDriverModel(pi, ctx, state.driver);
      if (driverModel) {
        writeLastUsedModel(ctx.cwd, { provider: driverModel.model.provider, modelId: driverModel.model.id });
      }

      // Set thinking level
      const effectiveThinking: ThinkingLevel = driverModel?.maxThinking
        ? clampThinkingLevel(state.thinking, driverModel.maxThinking)
        : state.thinking;
      pi.setThinkingLevel(effectiveThinking as any);

      const icon = TIER_ICONS[state.level];
      const modelNote = driverModel
        ? ` → ${driverModel.model.provider}/${driverModel.model.id}`
        : " (driver model unavailable)";
      ctx.ui.notify(
        `${icon} Switched to ${state.name} (${state.driver}/${effectiveThinking})${modelNote}`,
        driverModel ? "info" : "warning",
      );
    },
  });
}
