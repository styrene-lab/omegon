---
id: provider-landscape-assessment
title: Provider landscape assessment — gap analysis against upstream and strategic positioning
status: implemented
parent: bridge-provider-routing
tags: [providers, architecture, assessment, strategy, local-inference, google, huggingface]
open_questions: []
jj_change_id: syvtxqvswmlukkmvzlpmmoxxqlsluysu
---

# Provider landscape assessment — gap analysis against upstream and strategic positioning

## Overview

Full assessment of Omegon's provider client coverage against upstream pi-ai's 23-provider, 808-model registry and the strategic inference landscape. Goal: identify what's on the table, what to pick up now, and what the differentiation path looks like.

## Research

### Upstream pi-ai provider registry (808 models, 23 providers)

**Upstream provider → API wire protocol mapping:**

| Provider | API Protocol | Model Count | Auth |
|---|---|---|---|
| **Anthropic** | anthropic-messages (proprietary) | 23 | API key + OAuth |
| **OpenAI** | openai-responses (Responses API) | 40 | API key |
| **OpenAI Codex** | openai-codex-responses | 8 | ChatGPT OAuth JWT |
| **Google** | google-generative-ai (proprietary SDK) | 24 | GEMINI_API_KEY |
| **Google Gemini CLI** | google-gemini-cli | 6 | OAuth |
| **Google Vertex** | google-vertex | 12 | GCP credentials |
| **Google Antigravity** | google-gemini-cli | 9 | OAuth |
| **Mistral** | mistral-conversations (proprietary) | 25 | API key |
| **xAI (Grok)** | openai-completions (OAI-compat) | 24 | API key |
| **Groq** | openai-completions (OAI-compat) | 15 | API key |
| **HuggingFace** | openai-completions (OAI-compat) | 18 | HF_TOKEN |
| **Cerebras** | openai-completions (OAI-compat) | 4 | API key |
| **GitHub Copilot** | anthropic-messages + openai-completions + openai-responses | 24 | OAuth |
| **OpenRouter** | openai-completions (OAI-compat) | 246 | API key |
| **Azure OpenAI** | azure-openai-responses | 40 | Azure credentials |
| **Amazon Bedrock** | bedrock-converse-stream (AWS SDK) | 83 | IAM |
| **Vercel AI Gateway** | anthropic-messages | 147 | Vercel token |
| **Others** (minimax, zai, kimi-coding, opencode) | mixed | 50 | various |

**Key insight: 5 distinct wire protocols cover everything:**
1. **anthropic-messages** — Anthropic's proprietary streaming protocol
2. **openai-completions** — OpenAI Chat Completions (v1/chat/completions) — *the lingua franca*
3. **openai-responses** — OpenAI Responses API (newer, different event format)
4. **openai-codex-responses** — Responses API at chatgpt.com (same wire, different auth/URL)
5. **google-generative-ai** — Google's proprietary SDK protocol

Everything else is either (a) OpenAI-compatible with different base URL + API key, or (b) uses cloud-provider-specific SDKs (AWS Bedrock, Azure, GCP Vertex) that are enterprise concerns, not harness differentiators.

### Omegon's current Rust provider coverage

**What we have (4 clients, 3 wire protocols):**

| Client | Protocol | Status | Models |
|---|---|---|---|
| `AnthropicClient` | anthropic-messages | ✅ Full (OAuth + API key, token refresh, thinking, signatures) | claude-sonnet-4-6, opus-4-6, etc. |
| `OpenAIClient` | openai-completions (Chat Completions) | ✅ Full (API key only, rejects JWTs) | gpt-4.1, gpt-5 family |
| `CodexClient` | openai-codex-responses | ✅ New (JWT auth, account ID, SSE, retry) | gpt-5.3-codex-spark (free!), gpt-5.4 |
| `OpenRouterClient` | openai-completions (delegates to OpenAIClient) | ✅ Full (different base URL) | 246 models, free tier |

**What we're missing from the "Big Providers":**
1. **Google Gemini** — proprietary SDK, 24+ models, GEMINI_API_KEY — free tier available
2. **OpenAI Responses API** (non-Codex) — newer protocol that `openai` provider uses for gpt-5.x via API key; our OpenAIClient still uses Chat Completions

**What we get "for free" via OpenAI-compatible base URL swapping:**
- xAI/Grok (api.x.ai/v1) — 24 models
- Groq (api.groq.com/openai/v1) — 15 models, fast inference
- HuggingFace (router.huggingface.co/v1) — 18 models, OSS frontier
- Cerebras (api.cerebras.ai/v1) — 4 models, fast inference
- Mistral (api.mistral.ai) — 25 models (technically has its own protocol but also speaks OpenAI-compat)

**OpenAI-compatible providers are trivially supportable** — same wire format as our `OpenAIClient`, just different base_url + API key env var. The `OpenRouterClient` pattern already demonstrates this.

**Local inference (Ollama):**
- Currently lives in TypeScript extensions (offline-driver.ts, local-inference/)
- Ollama speaks OpenAI-compatible at localhost:11434/v1
- This is the ONLY provider that runs on the operator's hardware
- Not yet a Rust-native `LlmBridge` implementation

### Strategic assessment — what matters for Omegon's differentiation

**Tier 1: Must-have (differentiates Omegon)**

**Local Inference (Ollama → Rust-native LlmBridge)**
- THE differentiating factor. No other harness treats local models as first-class citizens in orchestration.
- Ollama is OpenAI-compatible at `/v1/chat/completions` — we can use a variant of OpenAIClient.
- But it also has its own `/api/chat` endpoint with features like `keep_alive`, `num_ctx`, model management.
- Critical: model browsing/pulling, VRAM-aware model selection, warm/cold state awareness.
- The current TS local-inference extension has: manage_ollama (start/stop/status/pull), offline-driver (auto-fallback), model tier hierarchy (70B→4B).
- This MUST become a native Rust `OllamaClient` — not just OpenAI-compat passthrough.

**OpenAI full stack (API key + ChatGPT OAuth)**
- ✅ API key → Chat Completions (done: `OpenAIClient`)
- ✅ ChatGPT OAuth → Codex Responses API (just built: `CodexClient`)
- ⚠️ Gap: `openai` provider upstream now uses Responses API (`openai-responses`), not Chat Completions. Our `OpenAIClient` uses the older protocol. Works fine for now but may diverge.

**Anthropic full stack**
- ✅ Fully covered: API key + OAuth, token refresh, thinking signatures, 1M context.

**Tier 2: High-value, low-effort (OpenAI-compatible with different URL)**

**HuggingFace Inference API**
- `router.huggingface.co/v1` — OpenAI-compatible
- 18 frontier models: DeepSeek-R1, Qwen3-235B, GLM-5, Kimi-K2.5
- HF_TOKEN auth — many users already have this
- Gateway to OSS frontier — operator pays HF, not OpenAI/Anthropic
- Critical for FOSS future: these models run locally too (same weights, different serving)
- Model browsing: HF has a web catalog. We should link to it and/or query their API.

**Groq**
- `api.groq.com/openai/v1` — blazing fast inference for supported models
- Good for leaf tasks / cleave children where latency matters more than capability
- 15 models including llama3.3-70b, deepseek-r1-distill

**xAI (Grok)**
- `api.x.ai/v1` — growing model family, competitive pricing
- 24 models, some with vision

**Cerebras**
- `api.cerebras.ai/v1` — hardware-accelerated, very fast
- Only 4 models but includes gpt-oss-120b

**Tier 3: Nice-to-have but not urgent**

**Google Gemini** — proprietary SDK, requires a separate wire protocol implementation. Gemini 2.5 Flash is competitive and has a free tier, but the engineering cost is higher than OpenAI-compat providers. Defer until demand warrants.

**Mistral** — has its own protocol but also speaks OpenAI-compat. Could be added as OpenAI-compat with `api.mistral.ai` base URL.

**Enterprise (Azure, Bedrock, Vertex, Copilot)** — cloud SDK-specific, enterprise-only. Not consumer-facing. Defer to post-1.0.

**Tier 4: OpenRouter covers it**

OpenRouter already proxies 246 models including all of the above. It's the universal fallback. But as noted — it's a startup, not infrastructure. Every provider we can talk to directly is one fewer dependency on OpenRouter's continued existence.

**The longevity risk**: OpenRouter could fold, rate-limit, or paywall at any time. Direct provider clients are insurance. HuggingFace is particularly important because it's the canonical OSS model registry — it's not going anywhere.

## Decisions

### Decision: Generic OpenAI-compatible client replaces per-provider wrappers

**Status:** decided
**Rationale:** OpenRouterClient is already just a thin wrapper around OpenAIClient with a different base URL. HuggingFace, Groq, xAI, Cerebras, Mistral, and Ollama all speak the same protocol. Instead of creating N separate XyzClient structs, create a single `OpenAICompatClient` that takes (base_url, api_key, provider_id) at construction. Register each provider in auth::PROVIDERS with its base URL. OpenRouterClient becomes `OpenAICompatClient::new("https://openrouter.ai/api", key, "openrouter")`. This also means the `auto_detect_bridge` fallback chain becomes data-driven from PROVIDERS, not a hardcoded match cascade.

### Decision: OllamaClient as native Rust LlmBridge with model management

**Status:** decided
**Rationale:** Ollama is not just another OpenAI-compatible endpoint. It needs: model pull/status/warm management, VRAM awareness, process lifecycle (start/stop), connectivity detection (is it running?), and automatic model selection based on hardware. The TS local-inference extension already handles all of this. The Rust port should be a proper OllamaClient that wraps OpenAI-compat for streaming but exposes model management as native methods. This is the foundation for treating local inference as a first-class orchestration resource — assigning cleave children to local models based on task complexity and available VRAM.

### Decision: HuggingFace gets a model browser redirect, not a built-in catalog

**Status:** decided
**Rationale:** HuggingFace has 1M+ models and a sophisticated web UI for filtering, sorting, and downloading. Building a TUI model browser would be enormous scope and worse than the web experience. Instead: (1) for HF inference API, it's just OpenAI-compat with HF_TOKEN — add to PROVIDERS with base_url. (2) for local model discovery, link operators to huggingface.co/models?pipeline_tag=text-generation&sort=trending and ollama.com/library for model selection, then use `ollama pull` to get them. (3) The harness can list installed Ollama models and suggest appropriate ones for the hardware profile.

### Decision: Google Gemini deferred — OpenAI-compat providers first

**Status:** decided
**Rationale:** Google's Generative AI SDK is a proprietary protocol requiring a full separate implementation (think: Anthropic-level effort). The payoff is access to Gemini 2.5 Flash (competitive, free tier) and Gemini 3 Pro. But: (1) Gemini models are also available via OpenRouter today, (2) Google's Gemini CLI OAuth flow is complex (GCP credentials), (3) the engineering effort is better spent on OpenAI-compat providers (5 providers for the cost of one Google impl). Revisit when demand warrants or when Google ships an OpenAI-compat gateway.

## Open Questions

*No open questions.*
