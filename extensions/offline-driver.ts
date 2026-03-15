// @config LOCAL_INFERENCE_URL "Ollama / OpenAI-compatible inference server URL" [default: http://localhost:11434]

import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import { Text } from "@cwilson613/pi-tui";
import { Type } from "@sinclair/typebox";
import {
  KNOWN_MODELS,
  PREFERRED_ORDER,
  PREFERRED_ORDER_CODE,
} from "./lib/local-models.ts";
import { filterDeprecated, type RegistryModel } from "./lib/model-routing.ts";

// Re-export so existing importers (effort, cleave) continue to work.
export { PREFERRED_ORDER, PREFERRED_ORDER_CODE };

/**
 * Offline Driver Extension
 *
 * Provides seamless failover from cloud (Anthropic) to local (Ollama) models.
 * Auto-registers available Ollama models via pi.registerProvider() on session start,
 * eliminating the need for a static models.json config.
 *
 * Registers /offline and /online commands plus a switch_to_offline_driver tool
 * the agent can self-invoke when it detects connectivity issues.
 *
 * Model registry and preference lists live in extensions/lib/local-models.ts.
 */

const OLLAMA_URL = process.env.LOCAL_INFERENCE_URL || "http://localhost:11434";
const PROVIDER_NAME = "local";

// State
let savedCloudModel: string | null = null;
let savedCloudProvider: string | null = null;
let isOffline = false;
let registeredModels: string[] = [];

interface OllamaModel {
  name: string;
  model?: string;
  size?: number;
  details?: { parameter_size?: string; family?: string };
}

export interface OfflineDriverSwitchResult {
  success: boolean;
  message: string;
  provider: "local" | "cloud";
  modelId?: string;
  label?: string;
  automatic: boolean;
}

async function discoverOllamaModels(): Promise<OllamaModel[]> {
  try {
    const res = await fetch(`${OLLAMA_URL}/api/tags`, { signal: AbortSignal.timeout(3000) });
    if (!res.ok) return [];
    const data = (await res.json()) as { models?: OllamaModel[] };
    return data.models || [];
  } catch {
    return [];
  }
}

function normalizeModelId(name: string): string {
  return name.replace(/:latest$/, "");
}

async function checkAnthropic(): Promise<boolean> {
  try {
    // Use GET on the models endpoint — lightweight, no billing, confirms API reachability.
    // Any HTTP response (even 401) means the network path works.
    const res = await fetch("https://api.anthropic.com/v1/models", {
      method: "GET",
      headers: { "anthropic-version": "2023-06-01" },
      signal: AbortSignal.timeout(5000),
    });
    return true;
  } catch {
    return false;
  }
}

/**
 * Register discovered Ollama models as a pi provider via the official API.
 * This lets pi-ai handle all streaming, token tracking, and protocol details.
 */
function registerOllamaProvider(pi: ExtensionAPI, ollamaModels: OllamaModel[]): string[] {
  const chatModels = ollamaModels
    .map((m) => normalizeModelId(m.name))
    .filter((id) => !id.includes("embed")); // exclude embedding models

  if (chatModels.length === 0) return [];

  const models = chatModels.map((id) => {
    const known = KNOWN_MODELS[id];
    return {
      id,
      name: known?.label || id,
      reasoning: false,
      input: ["text"] as ("text" | "image")[],
      contextWindow: known?.contextWindow || 131072,
      maxTokens: known?.maxTokens || 32768,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      compat: {
        supportsDeveloperRole: false,
        supportsReasoningEffort: false,
        maxTokensField: "max_tokens" as const,
        requiresThinkingAsText: true,
      },
    };
  });

  pi.registerProvider(PROVIDER_NAME, {
    baseUrl: `${OLLAMA_URL}/v1`,
    api: "openai-completions",
    apiKey: "ollama",
    models,
  });

  return chatModels;
}

export async function switchToOfflineDriver(
  pi: ExtensionAPI,
  ctx: any,
  options: { preferredModel?: string; automatic?: boolean } = {}
): Promise<OfflineDriverSwitchResult> {
  const preferredModel = options.preferredModel;
  const automatic = options.automatic ?? false;
  if (isOffline) {
    return { success: true, message: "Already in offline mode.", provider: "local", automatic };
  }

  // Save current cloud model for /online restoration
  const current = ctx.model;
  if (current && current.provider !== PROVIDER_NAME) {
    savedCloudModel = current.id;
    savedCloudProvider = current.provider;
  }

  // Re-discover and register in case models changed
  const ollamaModels = await discoverOllamaModels();
  if (ollamaModels.length === 0) {
    return {
      success: false,
      message: `Ollama not available at ${OLLAMA_URL}. Is it running? Start with: ollama serve`,
      provider: "local",
      automatic,
    };
  }
  registeredModels = registerOllamaProvider(pi, ollamaModels);

  // Select model: preferred > priority order > first available
  const targetId = preferredModel && registeredModels.includes(preferredModel)
    ? preferredModel
    : PREFERRED_ORDER.find((id) => registeredModels.includes(id)) || registeredModels[0];

  if (!targetId) {
    return { success: false, message: "No chat models available in Ollama.", provider: "local", automatic };
  }

  const model = ctx.modelRegistry.find(PROVIDER_NAME, targetId);
  if (!model) {
    return {
      success: false,
      message: `Model ${targetId} not found in registry after registration.`,
      provider: "local",
      automatic,
    };
  }

  const success = await pi.setModel(model);
  if (success) {
    isOffline = true;
    const known = KNOWN_MODELS[targetId];
    const icon = known?.icon || "🏠";
    const label = known?.label || targetId;
    ctx.ui.setStatus("offline-driver", `${icon} OFFLINE: ${label}`);
    return {
      success: true,
      message: `Switched to offline driver: ${label} (${targetId})`,
      provider: "local",
      modelId: targetId,
      label,
      automatic,
    };
  }

  return { success: false, message: "Failed to set offline model.", provider: "local", automatic };
}

export async function restoreCloudDriver(
  pi: ExtensionAPI,
  ctx: any,
  options: { automatic?: boolean } = {}
): Promise<OfflineDriverSwitchResult> {
  const automatic = options.automatic ?? false;
  if (!isOffline) {
    return { success: true, message: "Already in online mode.", provider: "cloud", automatic };
  }

  const provider = savedCloudProvider || "anthropic";
  let modelId = savedCloudModel;

  // If no saved model, find the best available gloriana-class model by prefix
  if (!modelId) {
    const all = filterDeprecated(ctx.modelRegistry.getAvailable() as unknown as RegistryModel[]);
    const topTier = all
      .filter((m: any) => m.provider === "anthropic" && m.id.startsWith("claude-opus"))
      .sort((a: any, b: any) => b.id.localeCompare(a.id));
    modelId = topTier[0]?.id;
  }

  if (!modelId) {
    return { success: false, message: "No Anthropic gloriana-class model found in registry.", provider: "cloud", automatic };
  }

  const model = ctx.modelRegistry.find(provider, modelId);

  if (!model) {
    return {
      success: false,
      message: `Cannot restore cloud model ${provider}/${modelId} — not found in registry.`,
      provider: "cloud",
      automatic,
    };
  }

  const anthropicOk = await checkAnthropic();
  if (!anthropicOk) {
    return {
      success: false,
      message: "Anthropic API still unreachable. Staying offline. Retry with /online when connectivity is restored.",
      provider: "cloud",
      automatic,
    };
  }

  const success = await pi.setModel(model);
  if (success) {
    isOffline = false;
    ctx.ui.setStatus("offline-driver", "");
    return {
      success: true,
      message: `Restored cloud driver: ${provider}/${modelId}`,
      provider: "cloud",
      modelId,
      label: `${provider}/${modelId}`,
      automatic,
    };
  }

  return { success: false, message: "Failed to restore cloud model.", provider: "cloud", automatic };
}

export default function (pi: ExtensionAPI) {
  // Auto-discover and register Ollama models on session start
  pi.on("session_start", async (_event, ctx) => {
    const [anthropicOk, ollamaModels] = await Promise.all([
      checkAnthropic(),
      discoverOllamaModels(),
    ]);

    // Register Ollama models via pi.registerProvider() — pi-ai handles streaming
    if (ollamaModels.length > 0) {
      registeredModels = registerOllamaProvider(pi, ollamaModels);
    }

    const driverModels = registeredModels.filter((id) => PREFERRED_ORDER.includes(id));
    const parts: string[] = [];

    if (anthropicOk) {
      parts.push("☁️ Anthropic: reachable");
    } else {
      parts.push("⚠️ Anthropic: UNREACHABLE");
    }

    if (registeredModels.length > 0) {
      const names = driverModels.map((id) => KNOWN_MODELS[id]?.label || id).join(", ");
      parts.push(
        `🏠 Ollama: ${driverModels.length} driver model${driverModels.length !== 1 ? "s" : ""} registered${driverModels.length > 0 ? ` (${names})` : ""}`
      );
    } else {
      parts.push("🏠 Ollama: not running");
    }

    // Suppress noisy status during first-run — bootstrap handles guidance
    // Suppress the "unreachable" warning during first-run (bootstrap handles it),
    // but always show success status so the operator sees their provider state.
    const { sharedState } = await import("./lib/shared-state.ts");
    if (anthropicOk || !sharedState.bootstrapPending) {
      ctx.ui.notify(parts.join(" | "), anthropicOk ? "info" : "warning");
    }

    // Save starting cloud model
    const current = ctx.model;
    if (current && current.provider !== PROVIDER_NAME) {
      savedCloudModel = current.id;
      savedCloudProvider = current.provider;
    }
    isOffline = false;

    if (!anthropicOk && driverModels.length > 0) {
      ctx.ui.notify("💡 Cloud unavailable. Use /offline to switch to local driver.", "warning");
    }
  });

  // /offline command
  pi.registerCommand("offline", {
    description: "Switch to best available local model as the driving agent",
    handler: async (args, ctx) => {
      const preferredModel = args?.trim() || undefined;
      const result = await switchToOfflineDriver(pi, ctx, { preferredModel });
      ctx.ui.notify(result.message, result.success ? "info" : "error");
    },
  });

  // /online command
  pi.registerCommand("online", {
    description: "Restore the cloud (Anthropic) model as the driving agent",
    handler: async (_args, ctx) => {
      const result = await restoreCloudDriver(pi, ctx);
      ctx.ui.notify(result.message, result.success ? "info" : "error");
    },
  });

  // Agent-invocable tool for self-recovery
  pi.registerTool({
    name: "switch_to_offline_driver",
    label: "Switch to Offline Driver",
    description:
      "Switch the driving model from cloud (Anthropic) to a local offline model (Ollama). " +
      "Use when you detect connectivity issues, API errors, or when the user requests offline mode. " +
      "The best available local model is auto-selected: Nemotron 3 Nano (1M context), " +
      "Devstral Small 2 (384K, code-focused), or Qwen3 30B (256K, general).",
    promptSnippet: "Switch from cloud to local Ollama model for offline operation or API failure recovery",
    promptGuidelines: [
      "Use when detecting repeated API errors, timeouts, or connectivity failures",
    ],
    parameters: Type.Object({
      reason: Type.String({
        description: "Why switching to offline mode",
      }),
      preferred_model: Type.Optional(
        Type.String({
          description:
            "Optional: specific model ID to use. Examples by size: 70B→qwen2.5:72b/llama3.3:70b, 32B→qwen3:32b/qwen2.5-coder:32b, 14B→qwen3:14b, 8B→qwen3:8b/llama3.1:8b, 4B→qwen3:4b. Omit to auto-select best available.",
        })
      ),
    }),
    execute: async (
      _toolCallId,
      params: { reason: string; preferred_model?: string },
      _signal,
      _onUpdate,
      ctx
    ) => {
      const result = await switchToOfflineDriver(pi, ctx, {
        preferredModel: params.preferred_model,
        automatic: true,
      });
      if (result.success) {
        ctx.ui.notify(`🔌 Offline: ${params.reason}`, "info");
      }
      return {
        content: [
          {
            type: "text" as const,
            text: `${result.success ? "✅" : "❌"} ${result.message}${result.success ? ` (reason: ${params.reason})` : ""}`,
          },
        ],
        details: { success: result.success, message: result.message },
      };
    },
    renderCall(args, t) {
      const reason = typeof args.reason === "string" ? args.reason : "";
      const model = typeof args.preferred_model === "string" ? args.preferred_model : "";
      const truncReason = reason.length > 72 ? `${reason.slice(0, 69)}…` : reason;
      const modelSuffix = model ? `  ${t.fg("dim", model)}` : "";
      return new Text(
        `${t.fg("warning", "⟳")} ${t.fg("toolTitle", "offline-driver")}${modelSuffix}  ${t.fg("muted", truncReason)}`,
        0, 0
      );
    },
    renderResult(result, _opts, t) {
      const details = result.details as { success?: boolean; message?: string } | undefined;
      const firstContent = result.content?.[0];
      const msg = details?.message ?? (firstContent?.type === "text" ? firstContent.text : "") ?? "";
      // Parse "Switched to offline driver: Display Name (model-id)"
      const switchMatch = msg.match(/Switched to offline driver:\s*(.+?)\s*\(([^)]+)\)/);
      if (switchMatch) {
        const displayName = switchMatch[1];
        const modelId = switchMatch[2];
        return new Text(
          `${t.fg("success", "✓")} ${t.fg("toolTitle", "offline")}  ${t.fg("accent", t.bold(displayName))}  ${t.fg("dim", modelId)}`,
          0, 0
        );
      }
      if (details?.success === false) {
        const errMsg = msg.replace(/^❌\s*/, "").replace(/Failed to switch.*?:\s*/i, "");
        return new Text(
          `${t.fg("error", "✗")} ${t.fg("toolTitle", "offline-driver")}  ${t.fg("error", errMsg.slice(0, 80))}`,
          0, 0
        );
      }
      return new Text(t.fg("toolOutput", msg.replace(/^[✅❌]\s*/, "")), 0, 0);
    },
  });
}
