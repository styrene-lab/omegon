// @config LOCAL_INFERENCE_URL "Ollama / OpenAI-compatible inference server URL" [default: http://localhost:11434]

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";

/**
 * Offline Driver Extension
 *
 * Provides seamless failover from cloud (Anthropic) to local (Ollama) models.
 * Auto-registers available Ollama models via pi.registerProvider() on session start,
 * eliminating the need for a static models.json config.
 *
 * Registers /offline and /online commands plus a switch_to_offline_driver tool
 * the agent can self-invoke when it detects connectivity issues.
 */

const OLLAMA_URL = process.env.LOCAL_INFERENCE_URL || "http://localhost:11434";
const PROVIDER_NAME = "local";

// Known models with metadata for ranking and display.
// Preferred ordering: Qwen3 32B is the best all-round local driver for this harness
// (reliable tool-call JSON, 128K ctx, thinking-mode toggle, fits in 64GB at Q8).
// Qwen2.5-Coder 32B is preferred for code-focused leaf tasks.
// Qwen2.5 72B provides the most reasoning depth but is slower (~10-15 tok/s on M1).
const KNOWN_MODELS: Record<string, { label: string; icon: string; contextWindow: number; maxTokens: number }> = {
  "qwen3:32b":             { label: "Qwen3 32B",              icon: "🐉", contextWindow: 131072,  maxTokens: 32768 },
  "qwen2.5-coder:32b":     { label: "Qwen2.5-Coder 32B",      icon: "🔧", contextWindow: 131072,  maxTokens: 32768 },
  "qwen2.5:72b":           { label: "Qwen2.5 72B",            icon: "🧠", contextWindow: 131072,  maxTokens: 32768 },
  "nemotron-3-nano:30b":   { label: "Nemotron 3 Nano 30B",    icon: "🏔️", contextWindow: 1048576, maxTokens: 32768 },
  "devstral-small-2:24b":  { label: "Devstral Small 2 24B",   icon: "⚙️", contextWindow: 393216,  maxTokens: 32768 },
  "qwen3:30b":             { label: "Qwen3 30B",              icon: "🐲", contextWindow: 262144,  maxTokens: 32768 },
};

// Preferred offline driver in priority order (general orchestration).
// Qwen3 32B leads — best tool-call reliability for the harness on M1 Max 64GB.
export const PREFERRED_ORDER = ["qwen3:32b", "nemotron-3-nano:30b", "devstral-small-2:24b", "qwen2.5-coder:32b", "qwen3:30b"];

// Preferred models for code-focused child tasks (cleave leaf workers).
export const PREFERRED_ORDER_CODE = ["qwen2.5-coder:32b", "qwen3:32b", "devstral-small-2:24b", "qwen3:30b"];

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

async function goOffline(
  pi: ExtensionAPI,
  ctx: any,
  preferredModel?: string
): Promise<{ success: boolean; message: string }> {
  if (isOffline) {
    return { success: true, message: "Already in offline mode." };
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
    };
  }
  registeredModels = registerOllamaProvider(pi, ollamaModels);

  // Select model: preferred > priority order > first available
  const targetId = preferredModel && registeredModels.includes(preferredModel)
    ? preferredModel
    : PREFERRED_ORDER.find((id) => registeredModels.includes(id)) || registeredModels[0];

  if (!targetId) {
    return { success: false, message: "No chat models available in Ollama." };
  }

  const model = ctx.modelRegistry.find(PROVIDER_NAME, targetId);
  if (!model) {
    return { success: false, message: `Model ${targetId} not found in registry after registration.` };
  }

  const success = await pi.setModel(model);
  if (success) {
    isOffline = true;
    const known = KNOWN_MODELS[targetId];
    const icon = known?.icon || "🏠";
    const label = known?.label || targetId;
    ctx.ui.setStatus("offline-driver", `${icon} OFFLINE: ${label}`);
    return { success: true, message: `Switched to offline driver: ${label} (${targetId})` };
  }

  return { success: false, message: "Failed to set offline model." };
}

async function goOnline(
  pi: ExtensionAPI,
  ctx: any
): Promise<{ success: boolean; message: string }> {
  if (!isOffline) {
    return { success: true, message: "Already in online mode." };
  }

  const provider = savedCloudProvider || "anthropic";
  let modelId = savedCloudModel;

  // If no saved model, find the best available opus model by prefix
  if (!modelId) {
    const all = ctx.modelRegistry.getAll();
    const opus = all
      .filter((m: any) => m.provider === "anthropic" && m.id.startsWith("claude-opus"))
      .sort((a: any, b: any) => b.id.localeCompare(a.id));
    modelId = opus[0]?.id;
  }

  if (!modelId) {
    return { success: false, message: "No Anthropic opus model found in registry." };
  }

  const model = ctx.modelRegistry.find(provider, modelId);

  if (!model) {
    return {
      success: false,
      message: `Cannot restore cloud model ${provider}/${modelId} — not found in registry.`,
    };
  }

  const anthropicOk = await checkAnthropic();
  if (!anthropicOk) {
    return {
      success: false,
      message: "Anthropic API still unreachable. Staying offline. Retry with /online when connectivity is restored.",
    };
  }

  const success = await pi.setModel(model);
  if (success) {
    isOffline = false;
    ctx.ui.setStatus("offline-driver", "");
    return { success: true, message: `Restored cloud driver: ${provider}/${modelId}` };
  }

  return { success: false, message: "Failed to restore cloud model." };
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

    ctx.ui.notify(parts.join(" | "), anthropicOk ? "info" : "warning");

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
      const result = await goOffline(pi, ctx, preferredModel);
      ctx.ui.notify(result.message, result.success ? "info" : "error");
    },
  });

  // /online command
  pi.registerCommand("online", {
    description: "Restore the cloud (Anthropic) model as the driving agent",
    handler: async (_args, ctx) => {
      const result = await goOnline(pi, ctx);
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
            "Optional: specific model ID to use (qwen3:32b, qwen2.5-coder:32b, qwen2.5:72b, nemotron-3-nano:30b, devstral-small-2:24b, qwen3:30b)",
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
      const result = await goOffline(pi, ctx, params.preferred_model);
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
        details: undefined,
      };
    },
  });
}
