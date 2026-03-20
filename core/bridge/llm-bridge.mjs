#!/usr/bin/env node
/**
 * LLM Bridge — translates between Omegon's wire format and pi-ai.
 *
 * Omegon defines the message contract. This bridge adapts it for pi-ai.
 * If pi-ai is replaced with a different provider library, only this file changes.
 *
 * Protocol: ndjson over stdin/stdout.
 *   Rust → Bridge: {"id":1,"method":"stream","params":{systemPrompt,messages,tools,model,reasoning}}
 *   Bridge → Rust: {"id":1,"event":{type:"text_delta",delta:"..."}}  (streamed)
 *   Bridge → Rust: {"id":1,"event":{type:"done",message:{...}}}      (terminal)
 *   Bridge → Rust: {"id":1,"error":"message"}                         (terminal, on failure)
 */

import { createInterface } from "readline";
import { readFileSync, existsSync } from "fs";
import { resolve } from "path";
import { streamSimple } from "@styrene-lab/pi-ai";

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

// ─── Omegon → pi-ai message translation ────────────────────────────────────

/**
 * Convert Omegon's message format to pi-ai's Message[].
 * Omegon sends: { role: "user"|"assistant"|"tool_result", ... }
 * pi-ai expects: UserMessage | AssistantMessage | ToolResultMessage
 */
function toProviderMessages(omegonMessages) {
  return omegonMessages.map((msg) => {
    switch (msg.role) {
      case "user":
        return {
          role: "user",
          content: msg.content,
          timestamp: Date.now(),
        };

      case "assistant":
        // If we have the raw provider message, pass it through for
        // multi-turn continuity (thinking signatures, cache IDs, etc.)
        if (msg.raw) {
          return msg.raw;
        }
        // Otherwise reconstruct from our fields
        return {
          role: "assistant",
          content: [
            ...(msg.text || []).map((t) => ({ type: "text", text: t })),
            ...(msg.tool_calls || []).map((tc) => ({
              type: "toolCall",
              id: tc.id,
              name: tc.name,
              arguments: tc.arguments,
            })),
          ],
          api: "anthropic",
          provider: "anthropic",
          model: "unknown",
          usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
          stopReason: "stop",
          timestamp: Date.now(),
        };

      case "tool_result":
        return {
          role: "toolResult",
          toolCallId: msg.call_id,
          toolName: msg.tool_name,
          content: [{ type: "text", text: msg.content }],
          isError: msg.is_error,
          timestamp: Date.now(),
        };

      default:
        // Unknown role — pass through and hope for the best
        return msg;
    }
  });
}

/**
 * Map from Omegon's short provider name to pi-ai's registered API identifier.
 */
const PROVIDER_TO_API = {
  anthropic: "anthropic-messages",
  openai: "openai-responses",
  google: "google-generative-ai",
  mistral: "mistral-conversations",
  azure: "azure-openai-responses",
  bedrock: "bedrock-converse-stream",
  vertex: "google-vertex",
};

/**
 * Resolve model spec from Omegon's "provider:model" string.
 * Returns a pi-ai Model object.
 */
function resolveModel(modelSpec) {
  const [provider, modelId] = modelSpec.includes(":")
    ? modelSpec.split(":", 2)
    : ["anthropic", modelSpec];

  const api = PROVIDER_TO_API[provider] ?? provider;

  // Minimal model object — pi-ai fills in defaults from its registry.
  // maxTokens left high to avoid truncating responses with extended thinking.
  return {
    id: modelId,
    name: modelId,
    api,
    provider,
    baseUrl: provider === "anthropic"
      ? "https://api.anthropic.com"
      : provider === "openai"
        ? "https://api.openai.com/v1"
        : `https://api.${provider}.com`,
    reasoning: true,
    input: ["text", "image"],
    cost: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 },
    contextWindow: 200000,
    maxTokens: 128000,
  };
}

/**
 * Convert pi-ai's AssistantMessage to Omegon's wire format.
 */
function toOmegonAssistantMessage(piMsg) {
  const text = [];
  const thinking = [];
  const toolCalls = [];

  for (const block of piMsg.content || []) {
    switch (block.type) {
      case "text":
        text.push(block.text);
        break;
      case "thinking":
        thinking.push(block.thinking);
        break;
      case "toolCall":
        toolCalls.push({
          id: block.id,
          name: block.name,
          arguments: block.arguments,
        });
        break;
    }
  }

  return {
    role: "assistant",
    text,
    thinking: thinking.length > 0 ? thinking : undefined,
    tool_calls: toolCalls.length > 0 ? toolCalls : undefined,
    raw: piMsg, // preserve for multi-turn continuity
  };
}

// ─── Request handling ───────────────────────────────────────────────────────

rl.on("line", async (line) => {
  let req;
  try {
    req = JSON.parse(line);
  } catch (e) {
    send({ id: null, error: `Invalid JSON: ${e.message}` });
    return;
  }

  const { id, method, params } = req;

  if (method === "shutdown") {
    send({ id, result: "ok" });
    process.exit(0);
  }

  if (method !== "stream") {
    send({ id, error: `Unknown method: ${method}` });
    return;
  }

  const { systemPrompt, messages, tools, model: modelSpec, reasoning } = params;

  try {
    const model = resolveModel(modelSpec || "anthropic:claude-sonnet-4-20250514");
    const providerMessages = toProviderMessages(messages || []);

    const context = {
      systemPrompt,
      messages: providerMessages,
      tools: (tools || []).map((t) => ({
        name: t.name,
        description: t.description,
        parameters: t.parameters,
      })),
    };

    const options = {};
    if (reasoning) {
      options.reasoning = reasoning;
    }

    // Resolve API key — try env var first, then pi's auth.json OAuth token
    const apiKey = resolveApiKey(model.provider);
    if (apiKey) {
      options.apiKey = apiKey;
    }

    const eventStream = streamSimple(model, context, options);

    for await (const event of eventStream) {
      const slim = slimEvent(event);
      if (slim) send({ id, event: slim });
    }

    const finalMessage = await eventStream.result();
    send({
      id,
      event: {
        type: "done",
        message: toOmegonAssistantMessage(finalMessage),
      },
    });
  } catch (e) {
    send({ id, error: e.message ?? String(e) });
  }
});

rl.on("close", () => process.exit(0));

// ─── Event slimming (pi-ai → Omegon) ───────────────────────────────────────

/**
 * Slim pi-ai events to Omegon's wire format.
 * Strip the partial AssistantMessage (redundant — Rust builds its own).
 */
function slimEvent(event) {
  switch (event.type) {
    case "text_delta":
      return { type: "text_delta", delta: event.delta };
    case "thinking_delta":
      return { type: "thinking_delta", delta: event.delta };
    case "toolcall_delta":
      return { type: "toolcall_delta", delta: event.delta };
    case "toolcall_end":
      return {
        type: "toolcall_end",
        tool_call: {
          id: event.toolCall.id,
          name: event.toolCall.name,
          arguments: event.toolCall.arguments,
        },
      };
    case "text_start":
    case "text_end":
    case "thinking_start":
    case "thinking_end":
    case "toolcall_start":
      return { type: event.type };
    // done is handled separately — we convert to Omegon format above
    case "done":
    case "error":
      return null; // handled by the stream completion path
    default:
      return { type: event.type };
  }
}

// ─── API key resolution ─────────────────────────────────────────────────────

/**
 * Resolve API key for a provider.
 * Priority: env var → pi's auth.json OAuth access token → null.
 */
function resolveApiKey(provider) {
  // 1. Environment variables (standard names)
  const envMap = {
    anthropic: ["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
    openai: ["OPENAI_API_KEY"],
    google: ["GOOGLE_API_KEY"],
    mistral: ["MISTRAL_API_KEY"],
  };
  for (const envVar of envMap[provider] ?? [`${provider.toUpperCase()}_API_KEY`]) {
    if (process.env[envVar]) return process.env[envVar];
  }

  // 2. pi's auth.json — contains OAuth access tokens
  try {
    const home = process.env.HOME || process.env.USERPROFILE || "~";
    const authPath = resolve(home, ".pi", "agent", "auth.json");
    if (existsSync(authPath)) {
      const auth = JSON.parse(readFileSync(authPath, "utf8"));
      const entry = auth[provider];
      if (entry?.access) return entry.access;
    }
  } catch { /* ignore */ }

  return null;
}

process.stderr.write("llm-bridge: ready\n");
