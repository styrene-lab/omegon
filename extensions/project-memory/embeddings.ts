/**
 * Project Memory — Embeddings
 *
 * Provider resolution order (automatic, no config required):
 *   1. Custom OpenAI-compatible endpoint — if MEMORY_EMBEDDING_BASE_URL is set
 *      (covers LM Studio, Mistral, Together, any /v1/embeddings-compatible server)
 *   2. Voyage AI — if VOYAGE_API_KEY is set (default model: voyage-3-lite)
 *   3. OpenAI — if OPENAI_API_KEY is set (default model: text-embedding-3-small)
 *   4. Ollama — local inference at OLLAMA_HOST or localhost:11434 (default model: qwen3-embedding:0.6b)
 *      Always returns a candidate; isEmbeddingAvailable() validates at startup.
 *   5. FTS5 keyword search — graceful degradation when no provider passes healthcheck
 *
 * Voyage AI is preferred over OpenAI when both keys are present because:
 *   - Anthropic-backed, aligned with pi's primary provider
 *   - voyage-3-lite: 512 dims, optimized for retrieval, cheaper per token
 *   - voyage-3: 1024 dims, higher quality for large corpora
 *
 * All providers speak the OpenAI /v1/embeddings wire format.
 * Vectors stored as raw Float32Array buffers in SQLite BLOB columns.
 * Dimension mismatch between providers is detected and handled by the caller
 * (factstore purges stale vectors on model change).
 */

export type EmbeddingProvider = "voyage" | "openai" | "openai-compatible" | "ollama";

// --- Endpoint and model defaults ---

const VOYAGE_BASE_URL = "https://api.voyageai.com/v1";
const OPENAI_BASE_URL = "https://api.openai.com/v1";

const DEFAULT_VOYAGE_MODEL = "voyage-3-lite";
const DEFAULT_OPENAI_MODEL = "text-embedding-3-small";

/** Known embedding dimensions by model name */
export const MODEL_DIMS: Record<string, number> = {
  // Voyage AI
  "voyage-3-lite": 512,
  "voyage-3": 1024,
  "voyage-3-large": 1024,
  "voyage-code-3": 1024,
  // OpenAI
  "text-embedding-3-small": 1536,
  "text-embedding-3-large": 3072,
  "text-embedding-ada-002": 1536,
  // Ollama local models
  "qwen3-embedding:0.6b": 1024,
  "qwen3-embedding": 1024,
  "nomic-embed-text": 768,
  "mxbai-embed-large": 1024,
  "all-minilm": 384,
  "snowflake-arctic-embed": 1024,
};

export interface EmbeddingResult {
  embedding: Float32Array;
  model: string;
  dims: number;
}

export interface EmbeddingOptions {
  provider?: EmbeddingProvider;
  model?: string;
  baseUrl?: string;
  timeout?: number;
  apiKey?: string;
}

// --- Provider resolution ---

/**
 * Auto-detect the best available provider from environment variables.
 * Called once at startup; result stored in MemoryConfig.embeddingProvider.
 */
export function resolveEmbeddingProvider(): { provider: EmbeddingProvider; model: string } | null {
  // 1. Custom OpenAI-compatible endpoint (highest priority — explicit user intent)
  if (process.env.MEMORY_EMBEDDING_BASE_URL) {
    const model = process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_OPENAI_MODEL;
    return { provider: "openai-compatible", model };
  }

  // 2. Voyage AI (preferred cloud provider)
  const voyageKey = process.env.VOYAGE_API_KEY ?? process.env.MEMORY_EMBEDDING_API_KEY;
  if (voyageKey && !process.env.MEMORY_EMBEDDING_BASE_URL) {
    // Only use Voyage if no custom base URL is set (custom URL implies a different provider)
    const model = process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_VOYAGE_MODEL;
    if (model.startsWith("voyage-")) {
      return { provider: "voyage", model };
    }
  }

  // Re-check: MEMORY_EMBEDDING_API_KEY with no base URL and a non-voyage model → OpenAI
  if (process.env.VOYAGE_API_KEY) {
    const model = process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_VOYAGE_MODEL;
    return { provider: "voyage", model };
  }

  // 3. OpenAI
  if (process.env.OPENAI_API_KEY) {
    const model = process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_OPENAI_MODEL;
    return { provider: "openai", model };
  }

  // 4. Ollama (local inference — zero config, auto-detected)
  // Ollama exposes an OpenAI-compatible /v1/embeddings endpoint.
  // We always return a candidate here; isEmbeddingAvailable() validates at startup
  // by sending a test embedding. If Ollama isn't running, it fails gracefully → FTS5.
  {
    const model = process.env.MEMORY_EMBEDDING_MODEL ?? "qwen3-embedding:0.6b";
    return { provider: "ollama", model };
  }
}

// --- Embedding implementations ---

/**
 * All three providers use the OpenAI /v1/embeddings wire format.
 * This one function handles all of them; only the base URL and API key differ.
 */
async function embedViaOpenAIFormat(
  text: string,
  baseUrl: string,
  apiKey: string,
  model: string,
  timeout: number,
): Promise<EmbeddingResult | null> {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);

    const resp = await fetch(`${baseUrl.replace(/\/$/, "")}/embeddings`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${apiKey}`,
      },
      body: JSON.stringify({ model, input: text }),
      signal: controller.signal,
    });

    clearTimeout(timer);
    if (!resp.ok) return null;

    const data = (await resp.json()) as { data?: Array<{ embedding?: number[] }> };
    const values = data.data?.[0]?.embedding;
    if (!values?.length) return null;

    const arr = new Float32Array(values);
    return { embedding: arr, model, dims: arr.length };
  } catch {
    return null;
  }
}

function resolveOpts(opts?: EmbeddingOptions): {
  provider: EmbeddingProvider;
  model: string;
  baseUrl: string;
  apiKey: string;
  timeout: number;
} | null {
  const timeout = opts?.timeout ?? 8_000;

  // Explicit provider override (e.g. from MemoryConfig)
  const provider = opts?.provider ?? ("voyage" as EmbeddingProvider);

  if (provider === "openai-compatible") {
    const baseUrl = opts?.baseUrl ?? process.env.MEMORY_EMBEDDING_BASE_URL ?? "";
    const apiKey = opts?.apiKey ?? process.env.MEMORY_EMBEDDING_API_KEY ?? "";
    const model = opts?.model ?? process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_OPENAI_MODEL;
    if (!baseUrl || !apiKey) return null;
    return { provider, model, baseUrl, apiKey, timeout };
  }

  if (provider === "voyage") {
    const baseUrl = opts?.baseUrl ?? VOYAGE_BASE_URL;
    const apiKey = opts?.apiKey ?? process.env.VOYAGE_API_KEY ?? process.env.MEMORY_EMBEDDING_API_KEY ?? "";
    const model = opts?.model ?? process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_VOYAGE_MODEL;
    if (!apiKey) return null;
    return { provider, model, baseUrl, apiKey, timeout };
  }

  if (provider === "ollama") {
    let ollamaHost = process.env.OLLAMA_HOST ?? "http://localhost:11434";
    // OLLAMA_HOST is commonly set without a scheme (e.g., "localhost:11434")
    if (!/^https?:\/\//i.test(ollamaHost)) {
      ollamaHost = `http://${ollamaHost}`;
    }
    const baseUrl = `${ollamaHost.replace(/\/$/, "")}/v1`;
    const model = opts?.model ?? process.env.MEMORY_EMBEDDING_MODEL ?? "qwen3-embedding:0.6b";
    // Ollama doesn't require an API key but the OpenAI-compatible endpoint
    // accepts any non-empty bearer token.
    return { provider, model, baseUrl, apiKey: "ollama", timeout };
  }

  // openai
  const baseUrl = opts?.baseUrl ?? process.env.MEMORY_EMBEDDING_BASE_URL ?? OPENAI_BASE_URL;
  const apiKey = opts?.apiKey ?? process.env.MEMORY_EMBEDDING_API_KEY ?? process.env.OPENAI_API_KEY ?? "";
  const model = opts?.model ?? process.env.MEMORY_EMBEDDING_MODEL ?? DEFAULT_OPENAI_MODEL;
  if (!apiKey) return null;
  return { provider, model, baseUrl, apiKey, timeout };
}

// --- Public API ---

/**
 * Embed a single text string via the configured embedding backend.
 * Returns null if the provider is unreachable or not configured.
 */
export async function embed(
  text: string,
  opts?: EmbeddingOptions,
): Promise<EmbeddingResult | null> {
  const resolved = resolveOpts(opts);
  if (!resolved) return null;
  return embedViaOpenAIFormat(text, resolved.baseUrl, resolved.apiKey, resolved.model, resolved.timeout);
}

/**
 * Check if the configured embedding backend is reachable and usable.
 */
export async function isEmbeddingAvailable(
  opts?: EmbeddingOptions,
): Promise<{ available: boolean; model?: string; dims?: number }> {
  const result = await embed("embedding healthcheck", opts);
  if (!result) return { available: false };
  return { available: true, model: result.model, dims: result.dims };
}

// Re-export pure vector math from core.ts — single source of truth and Rust port target.
export { cosineSimilarity, vectorToBlob, blobToVector } from "./core.ts";
