// @config LOCAL_INFERENCE_URL "Ollama / OpenAI-compatible inference server URL" [default: http://localhost:11434]

/**
 * local-inference — Delegate sub-tasks to locally running LLM inference servers
 *
 * Registers an `ask_local_model` tool that the driving agent (Claude) can call to
 * delegate specific sub-tasks to local models running via Ollama or any
 * OpenAI-compatible local server. Zero API cost for delegated work.
 *
 * Use cases:
 *   - Boilerplate/template generation
 *   - File summarization
 *   - Code transforms (formatting, conversion)
 *   - Draft generation for review by the driving agent
 *   - Embeddings generation
 *
 * Architecture:
 *   This is Option C (tool-callable sub-agent): the driving agent stays Claude
 *   with reliable tool use and reasoning, but can offload cheap work to local models.
 *   The abstraction layer supports any OpenAI-compatible backend. Default: Ollama.
 */

import { execSync, spawn, type ChildProcess } from "node:child_process";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { StringEnum } from "../lib/typebox-helpers";

const DEFAULT_URL = "http://localhost:11434";

interface LocalModel {
  id: string;
  object: string;
  owned_by: string;
}

interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

interface ChatResponse {
  id: string;
  choices: Array<{
    message: {
      role: string;
      content: string;
      reasoning?: string;
    };
    finish_reason: string;
  }>;
  usage: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

function getBaseUrl(): string {
  return process.env.LOCAL_INFERENCE_URL || DEFAULT_URL;
}

async function discoverModels(baseUrl: string): Promise<LocalModel[]> {
  try {
    const resp = await fetch(`${baseUrl}/v1/models`, {
      signal: AbortSignal.timeout(3000),
    });
    if (!resp.ok) return [];
    const data = await resp.json();
    return (data.data || []).filter(
      (m: LocalModel) => !m.id.includes("embed") // exclude embedding models from chat
    );
  } catch {
    return [];
  }
}

async function listAllModels(baseUrl: string): Promise<LocalModel[]> {
  try {
    const resp = await fetch(`${baseUrl}/v1/models`, {
      signal: AbortSignal.timeout(3000),
    });
    if (!resp.ok) return [];
    const data = await resp.json();
    return data.data || [];
  } catch {
    return [];
  }
}

function stripThinkTokens(text: string): string {
  // Clean up leaked thinking tokens from various model families
  return text
    .replace(/<think>[\s\S]*?<\/think>\s*/g, "")       // <think>...</think>
    .replace(/<\|begin_of_box\|>/g, "")                  // GLM box tokens
    .replace(/<\|end_of_box\|>/g, "")
    .trim();
}

async function chatCompletionStreaming(
  baseUrl: string,
  model: string,
  messages: ChatMessage[],
  opts: {
    maxTokens?: number;
    temperature?: number;
    signal?: AbortSignal;
    onToken?: (accumulated: string) => void;
  }
): Promise<{ content: string; reasoning?: string; usage: ChatResponse["usage"] }> {
  const resp = await fetch(`${baseUrl}/v1/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      model,
      messages,
      max_tokens: opts.maxTokens || 2048,
      temperature: opts.temperature ?? 0.3,
      stream: true,
    }),
    signal: opts.signal,
  });

  if (!resp.ok) {
    const body = await resp.text().catch(() => "");
    throw new Error(`Local inference failed (${resp.status}): ${body}`);
  }

  if (!resp.body) throw new Error("No response body from local model");

  let accumulated = "";
  let reasoning = "";
  let usage: ChatResponse["usage"] = { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 };

  const reader = resp.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed || !trimmed.startsWith("data: ")) continue;
      const payload = trimmed.slice(6);
      if (payload === "[DONE]") continue;

      try {
        const chunk = JSON.parse(payload);
        const delta = chunk.choices?.[0]?.delta;
        if (delta?.content) {
          accumulated += delta.content;
          opts.onToken?.(accumulated);
        }
        if (delta?.reasoning) {
          reasoning += delta.reasoning;
        }
        // Ollama sends usage in the final chunk
        if (chunk.usage) {
          usage = chunk.usage;
        }
      } catch {
        // skip malformed chunks
      }
    }
  }

  return {
    content: stripThinkTokens(accumulated),
    reasoning: reasoning || undefined,
    usage,
  };
}

export default function (pi: ExtensionAPI) {
  // Track available models (refreshed on session start and via command)
  let cachedModels: LocalModel[] = [];
  let serverOnline = false;

  async function refreshModels() {
    const baseUrl = getBaseUrl();
    cachedModels = await discoverModels(baseUrl);
    serverOnline = cachedModels.length > 0 || (await listAllModels(baseUrl)).length > 0;
    return cachedModels;
  }

  // --- Ollama lifecycle management ---

  let ollamaChild: ChildProcess | null = null;
  let ollamaBinaryAvailable: boolean | null = null; // cached after first check

  function hasOllama(): boolean {
    if (ollamaBinaryAvailable !== null) return ollamaBinaryAvailable;
    try {
      execSync("which ollama", { stdio: "ignore" });
      ollamaBinaryAvailable = true;
    } catch {
      ollamaBinaryAvailable = false;
    }
    return ollamaBinaryAvailable;
  }

  async function isOllamaReachable(): Promise<boolean> {
    try {
      const resp = await fetch(`${getBaseUrl()}/api/tags`, {
        signal: AbortSignal.timeout(2000),
      });
      return resp.ok;
    } catch {
      return false;
    }
  }

  /** Try brew services first (persists across reboots), fall back to ollama serve */
  function startOllamaProcess(): { method: string } {
    if (process.platform === "darwin") {
      try {
        execSync("brew services start ollama", { stdio: "ignore", timeout: 10_000 });
        return { method: "brew services" };
      } catch {
        // fall through to manual serve
      }
    }

    const child = spawn("ollama", ["serve"], {
      stdio: "ignore",
      detached: true,
    });
    child.unref();
    ollamaChild = child;

    child.on("exit", () => {
      if (ollamaChild === child) ollamaChild = null;
    });

    return { method: "ollama serve (background)" };
  }

  function stopOllama(): string {
    if (process.platform === "darwin") {
      try {
        execSync("brew services stop ollama", { stdio: "ignore", timeout: 10_000 });
        serverOnline = false;
        cachedModels = [];
        return "Stopped Ollama (brew services).";
      } catch { /* fall through */ }
    }

    if (ollamaChild) {
      ollamaChild.kill("SIGTERM");
      ollamaChild = null;
      serverOnline = false;
      cachedModels = [];
      return "Stopped Ollama background process.";
    }

    try {
      execSync("pkill -f 'ollama serve'", { stdio: "ignore" });
      serverOnline = false;
      cachedModels = [];
      return "Stopped Ollama process.";
    } catch {
      return "Ollama doesn't appear to be running.";
    }
  }

  async function waitForOllama(maxSeconds: number): Promise<boolean> {
    for (let i = 0; i < maxSeconds; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      if (await isOllamaReachable()) return true;
    }
    return false;
  }

  function pullModel(modelName: string, signal?: AbortSignal): Promise<{ success: boolean; output: string }> {
    return new Promise((resolve) => {
      const child = spawn("ollama", ["pull", modelName], { stdio: "pipe" });
      let output = "";
      child.stdout?.on("data", (d: Buffer) => { output += d.toString(); });
      child.stderr?.on("data", (d: Buffer) => { output += d.toString(); });
      child.on("exit", (code) => {
        resolve({ success: code === 0, output: output.slice(-200) });
      });
      child.on("error", (err) => {
        resolve({ success: false, output: err.message });
      });
      signal?.addEventListener("abort", () => { child.kill("SIGTERM"); });
    });
  }

  // Check server + auto-start ollama on session start
  pi.on("session_start", async (_event, ctx) => {
    await refreshModels();

    if (serverOnline) return;

    // Auto-start if binary exists, server is down, and no custom URL configured
    if (!hasOllama() || process.env.LOCAL_INFERENCE_URL) return;

    if (ctx.hasUI) {
      ctx.ui.notify("Ollama installed but not running — starting...", "info");
    }

    startOllamaProcess();

    if (await waitForOllama(10)) {
      await refreshModels();
      if (ctx.hasUI) {
        ctx.ui.notify(`Ollama started — ${cachedModels.length} chat models available`, "success");
      }
    }
  });

  // Clean up spawned ollama child on session end
  pi.on("session_shutdown", () => {
    // Only kill if WE spawned it (not brew services)
    if (ollamaChild) {
      ollamaChild.kill("SIGTERM");
      ollamaChild = null;
    }
  });

  // Main delegation tool
  pi.registerTool({
    name: "ask_local_model",
    label: "Ask Local Model",
    description:
      "Delegate a sub-task to a locally running LLM (zero API cost). " +
      "The local model runs on-device via Ollama. Use for:\n" +
      "- Boilerplate/template generation\n" +
      "- File summarization or content transforms\n" +
      "- Code formatting, conversion, or simple generation\n" +
      "- Drafting text for your review\n" +
      "- Any task where perfect accuracy isn't critical\n\n" +
      "You receive the local model's response and can review, edit, or use it. " +
      "The local model has NO access to tools, files, or conversation context — " +
      "you must include all necessary context in the prompt.",
    promptSnippet: "Delegate sub-tasks to local LLM via Ollama (zero API cost, on-device)",
    promptGuidelines: [
      "Include ALL necessary context in the prompt — the local model cannot see conversation history or access tools",
      "Use for boilerplate generation, file summarization, code transforms, and drafting text for review",
    ],
    parameters: Type.Object({
      prompt: Type.String({
        description: "Complete prompt for the local model. Include ALL necessary context — the local model cannot see our conversation or access any tools.",
      }),
      system: Type.Optional(
        Type.String({
          description: "Optional system prompt to set the local model's behavior (e.g., 'You are a Python expert. Output only code, no explanations.')",
        })
      ),
      model: Type.Optional(
        Type.String({
          description: "Specific model ID to use. Omit to auto-select the best available model.",
        })
      ),
      max_tokens: Type.Optional(
        Type.Number({
          description: "Maximum response tokens (default: 2048)",
        })
      ),
      temperature: Type.Optional(
        Type.Number({
          description: "Sampling temperature 0.0-1.0 (default: 0.3, lower = more deterministic)",
        })
      ),
    }),
    execute: async (
      _toolCallId,
      params: {
        prompt: string;
        system?: string;
        model?: string;
        max_tokens?: number;
        temperature?: number;
      },
      signal,
      onUpdate,
      ctx
    ) => {
      const baseUrl = getBaseUrl();

      // Refresh models if cache is empty
      if (cachedModels.length === 0) await refreshModels();

      if (!serverOnline) {
        return {
          content: [
            {
              type: "text" as const,
              text: `Local inference server not available at ${baseUrl}. Is Ollama running? Start with: ollama serve`,
            },
          ],
        };
      }

      // Model selection: explicit > auto (prefer largest/most capable)
      let modelId = params.model;
      if (!modelId) {
        // Prefer models roughly by capability heuristic (larger/newer = higher score)
        const ranked = [...cachedModels].sort((a, b) => {
          const score = (id: string) => {
            if (id.includes("nemotron")) return 110;
            if (id.includes("qwen3")) return 100;
            if (id.includes("devstral")) return 95;
            if (id.includes("qwen2.5")) return 80;
            if (id.includes("qwen")) return 75;
            if (id.includes("llama")) return 60;
            if (id.includes("mistral")) return 50;
            if (id.includes("gemma")) return 45;
            return 30;
          };
          return score(b.id) - score(a.id);
        });
        modelId = ranked[0]?.id;
      }

      if (!modelId) {
        return {
          content: [
            {
              type: "text" as const,
              text: "No chat models available in Ollama. Pull a model with: ollama pull nemotron-3-nano:30b",
            },
          ],
        };
      }

      const messages: ChatMessage[] = [];
      if (params.system) {
        messages.push({ role: "system", content: params.system });
      }
      messages.push({ role: "user", content: params.prompt });

      try {
        const result = await chatCompletionStreaming(baseUrl, modelId, messages, {
          maxTokens: params.max_tokens,
          temperature: params.temperature,
          signal: signal,
          onToken: (accumulated) => {
            onUpdate?.({
              content: [
                {
                  type: "text" as const,
                  text: `**Local model:** ${modelId} *(streaming...)*\n\n---\n\n${stripThinkTokens(accumulated)}`,
                },
              ],
            });
          },
        });

        const parts: Array<{ type: "text"; text: string }> = [];
        parts.push({
          type: "text" as const,
          text: `**Local model:** ${modelId}\n**Tokens:** ${result.usage.prompt_tokens} in → ${result.usage.completion_tokens} out\n\n---\n\n${result.content}`,
        });

        if (result.reasoning) {
          parts.push({
            type: "text" as const,
            text: `\n\n---\n**Model reasoning:** ${result.reasoning}`,
          });
        }

        return { content: parts };
      } catch (err: any) {
        return {
          content: [
            {
              type: "text" as const,
              text: `Local inference error (${modelId}): ${err.message}`,
            },
          ],
        };
      }
    },
  });

  // List available local models
  pi.registerTool({
    name: "list_local_models",
    label: "List Local Models",
    description:
      "List all models currently available in the local inference server (Ollama). " +
      "Use to check what's loaded before delegating work.",
    promptSnippet: "List available Ollama models before delegating work",
    parameters: Type.Object({}),
    execute: async (_toolCallId, _params, _signal, _onUpdate, ctx) => {
      const baseUrl = getBaseUrl();
      const all = await listAllModels(baseUrl);

      if (all.length === 0) {
        return {
          content: [
            {
              type: "text" as const,
              text: `No models available at ${baseUrl}. Is Ollama running? Start with: ollama serve`,
            },
          ],
        };
      }

      const lines = all.map((m) => {
        const isEmbed = m.id.includes("embed");
        return `- \`${m.id}\` ${isEmbed ? "(embeddings)" : "(chat)"}`;
      });

      return {
        content: [
          {
            type: "text" as const,
            text: `**Local models at ${baseUrl}:**\n${lines.join("\n")}`,
          },
        ],
      };
    },
  });

  // --- Ollama management tool (agent-callable) ---

  function toolResult(msg: string) {
    return { content: [{ type: "text" as const, text: msg }] };
  }

  function modelCount(models: LocalModel[]): string {
    return `${models.length} chat model${models.length !== 1 ? "s" : ""}`;
  }

  async function ollamaStatus(): Promise<string> {
    const reachable = await isOllamaReachable();
    if (!reachable) return `Ollama is not running at ${getBaseUrl()}.`;

    const all = await listAllModels(getBaseUrl());
    const chat = all.filter((m) => !m.id.includes("embed"));
    const embed = all.filter((m) => m.id.includes("embed"));
    let msg = `Ollama running at ${getBaseUrl()}\n`;
    if (chat.length > 0) msg += `Chat models: ${chat.map((m) => m.id).join(", ")}\n`;
    if (embed.length > 0) msg += `Embedding models: ${embed.map((m) => m.id).join(", ")}\n`;
    if (all.length === 0) msg += "No models installed.";
    return msg;
  }

  async function ollamaStart(): Promise<string> {
    if (await isOllamaReachable()) {
      const models = await refreshModels();
      return `Ollama is already running at ${getBaseUrl()} — ${modelCount(models)} available.`;
    }

    startOllamaProcess();

    if (await waitForOllama(15)) {
      const models = await refreshModels();
      const suffix = models.length === 0
        ? " No models installed yet — pull one with `ollama pull qwen3:30b`."
        : "";
      return `Ollama started successfully — ${modelCount(models)} available.${suffix}`;
    }
    return "Ollama process started but server not responding after 15s. It may still be loading.";
  }

  pi.registerTool({
    name: "manage_ollama",
    label: "Manage Ollama",
    description:
      "Manage the Ollama local inference server: start, stop, check status, or pull models. " +
      "Use 'start' when local models are needed but Ollama isn't running. " +
      "Use 'pull' to download a model before delegating work to it. " +
      "Use 'status' to check what's available. Use 'stop' to free GPU memory.",
    promptSnippet: "Start/stop Ollama, pull models, check status",
    promptGuidelines: [
      "Call with action 'start' if ask_local_model or list_local_models reports Ollama is not running",
      "Call with action 'pull' and a model name to download models (e.g., 'qwen3:30b', 'devstral-small:24b')",
      "Call with action 'status' to check if Ollama is running and what models are available",
      "Call with action 'stop' when done with local inference to free GPU/memory",
    ],
    parameters: Type.Object({
      action: StringEnum(["start", "stop", "status", "pull"], {
        description: "Action to perform",
      }),
      model: Type.Optional(
        Type.String({
          description: "Model name for 'pull' action (e.g., 'qwen3:30b', 'devstral-small:24b', 'qwen3-embedding')",
        })
      ),
    }),
    execute: async (_toolCallId, params: { action: string; model?: string }, signal) => {
      if (!hasOllama()) {
        return toolResult("Ollama is not installed. The user should run `/bootstrap` to set up pi-kit dependencies.");
      }

      switch (params.action) {
        case "start":
          return toolResult(await ollamaStart());

        case "stop":
          return toolResult(stopOllama());

        case "status":
          return toolResult(await ollamaStatus());

        case "pull": {
          if (!params.model) {
            return toolResult("Model name required for pull. Examples: qwen3:30b, devstral-small:24b, qwen3-embedding");
          }
          if (!(await isOllamaReachable())) {
            return toolResult("Ollama is not running. Start it first (action: 'start').");
          }

          const result = await pullModel(params.model, signal);
          if (result.success) {
            await refreshModels();
            return toolResult(`Successfully pulled ${params.model}. Model is now available for use.`);
          }
          return toolResult(`Failed to pull ${params.model}. ${result.output}`);
        }

        default:
          return toolResult("Unknown action. Use: start, stop, status, pull");
      }
    },
  });

  // Manual commands
  pi.registerCommand("local-models", {
    description: "List available local inference models",
    handler: async (_args, ctx) => {
      const all = await listAllModels(getBaseUrl());
      if (all.length === 0) {
        ctx.ui.notify("No local models available — is Ollama running?", "warning");
      } else {
        const names = all.map((m) => m.id).join("\n  ");
        ctx.ui.notify(`Local models:\n  ${names}`, "info");
      }
    },
  });

  pi.registerCommand("local-status", {
    description: "Check local inference server status",
    handler: async (_args, ctx) => {
      await refreshModels();
      if (serverOnline) {
        ctx.ui.notify(
          `🏠 Local inference online — ${cachedModels.length} chat models available`,
          "info"
        );
      } else {
        ctx.ui.notify(`Local inference offline at ${getBaseUrl()}`, "warning");
      }
    },
  });

  pi.registerCommand("ollama", {
    description: "Manage Ollama — start, stop, status, pull models",
    handler: async (args, ctx) => {
      const parts = args.trim().split(/\s+/);
      const sub = parts[0]?.toLowerCase() || "";

      if (!hasOllama()) {
        ctx.ui.notify("`ollama` is not installed. Run `/bootstrap` to set up pi-kit dependencies.", "warning");
        return;
      }

      switch (sub) {
        case "start": {
          ctx.ui.notify("Starting Ollama...", "info");
          const msg = await ollamaStart();
          ctx.ui.notify(msg, "info");
          return;
        }

        case "stop": {
          ctx.ui.notify(stopOllama(), "info");
          return;
        }

        case "pull": {
          const modelName = parts[1];
          if (!modelName) {
            ctx.ui.notify(
              "Usage: /ollama pull <model>\n\nPopular models:\n" +
              "- qwen3:30b — general purpose, 256K context\n" +
              "- devstral-small:24b — code-focused, 128K context\n" +
              "- qwen3-embedding — embeddings\n" +
              "- nemotron-3-nano:30b — NVIDIA, 1M context",
              "info"
            );
            return;
          }

          if (!(await isOllamaReachable())) {
            ctx.ui.notify("Ollama is not running. Start it first with /ollama start.", "warning");
            return;
          }

          ctx.ui.notify(`Pulling ${modelName}...`, "info");
          const result = await pullModel(modelName);
          if (result.success) {
            await refreshModels();
            ctx.ui.notify(`✅ ${modelName} pulled successfully.`, "info");
          } else {
            ctx.ui.notify(`❌ Failed to pull ${modelName}. ${result.output}`, "warning");
          }
          return;
        }

        case "status":
        case "": {
          ctx.ui.notify(await ollamaStatus(), "info");
          return;
        }

        default:
          ctx.ui.notify("Usage: /ollama [start|stop|status|pull <model>]", "info");
          return;
      }
    },
  });
}
