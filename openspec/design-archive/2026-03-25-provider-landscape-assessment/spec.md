+++
id = "351e74dc-153e-409e-a20d-662eb9e563aa"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider landscape assessment — gap analysis against upstream and strategic positioning — Design Spec (extracted)

> Auto-extracted from docs/provider-landscape-assessment.md at decide-time.

## Decisions

### Generic OpenAI-compatible client replaces per-provider wrappers (decided)

OpenRouterClient is already just a thin wrapper around OpenAIClient with a different base URL. HuggingFace, Groq, xAI, Cerebras, Mistral, and Ollama all speak the same protocol. Instead of creating N separate XyzClient structs, create a single `OpenAICompatClient` that takes (base_url, api_key, provider_id) at construction. Register each provider in auth::PROVIDERS with its base URL. OpenRouterClient becomes `OpenAICompatClient::new("https://openrouter.ai/api", key, "openrouter")`. This also means the `auto_detect_bridge` fallback chain becomes data-driven from PROVIDERS, not a hardcoded match cascade.

### OllamaClient as native Rust LlmBridge with model management (decided)

Ollama is not just another OpenAI-compatible endpoint. It needs: model pull/status/warm management, VRAM awareness, process lifecycle (start/stop), connectivity detection (is it running?), and automatic model selection based on hardware. The TS local-inference extension already handles all of this. The Rust port should be a proper OllamaClient that wraps OpenAI-compat for streaming but exposes model management as native methods. This is the foundation for treating local inference as a first-class orchestration resource — assigning cleave children to local models based on task complexity and available VRAM.

### HuggingFace gets a model browser redirect, not a built-in catalog (decided)

HuggingFace has 1M+ models and a sophisticated web UI for filtering, sorting, and downloading. Building a TUI model browser would be enormous scope and worse than the web experience. Instead: (1) for HF inference API, it's just OpenAI-compat with HF_TOKEN — add to PROVIDERS with base_url. (2) for local model discovery, link operators to huggingface.co/models?pipeline_tag=text-generation&sort=trending and ollama.com/library for model selection, then use `ollama pull` to get them. (3) The harness can list installed Ollama models and suggest appropriate ones for the hardware profile.

### Google Gemini deferred — OpenAI-compat providers first (decided)

Google's Generative AI SDK is a proprietary protocol requiring a full separate implementation (think: Anthropic-level effort). The payoff is access to Gemini 2.5 Flash (competitive, free tier) and Gemini 3 Pro. But: (1) Gemini models are also available via OpenRouter today, (2) Google's Gemini CLI OAuth flow is complex (GCP credentials), (3) the engineering effort is better spent on OpenAI-compat providers (5 providers for the cost of one Google impl). Revisit when demand warrants or when Google ships an OpenAI-compat gateway.

## Research Summary

### Upstream pi-ai provider registry (808 models, 23 providers)

**Upstream provider → API wire protocol mapping:**

| Provider | API Protocol | Model Count | Auth |
|---|---|---|---|
| **Anthropic** | anthropic-messages (proprietary) | 23 | API key + OAuth |
| **OpenAI** | openai-responses (Responses API) | 40 | API key |
| **OpenAI Codex** | openai-codex-responses | 8 | ChatGPT OAuth JWT |
| **Google** | google-generative-ai (proprietary SDK) | 24 | GEMINI_API_KEY |
| **Google Gemini CLI** | google-gemini-cli | 6 | OAuth |
| **Google Vertex** | google-verte…

### Omegon's current Rust provider coverage

**What we have (4 clients, 3 wire protocols):**

| Client | Protocol | Status | Models |
|---|---|---|---|
| `AnthropicClient` | anthropic-messages | ✅ Full (OAuth + API key, token refresh, thinking, signatures) | claude-sonnet-4-6, opus-4-6, etc. |
| `OpenAIClient` | openai-completions (Chat Completions) | ✅ Full (API key only, rejects JWTs) | gpt-4.1, gpt-5 family |
| `CodexClient` | openai-codex-responses | ✅ New (JWT auth, account ID, SSE, retry) | gpt-5.3-codex-spark (free!), gpt-5.4 |
| `O…

### Strategic assessment — what matters for Omegon's differentiation

**Tier 1: Must-have (differentiates Omegon)**

**Local Inference (Ollama → Rust-native LlmBridge)**
- THE differentiating factor. No other harness treats local models as first-class citizens in orchestration.
- Ollama is OpenAI-compatible at `/v1/chat/completions` — we can use a variant of OpenAIClient.
- But it also has its own `/api/chat` endpoint with features like `keep_alive`, `num_ctx`, model management.
- Critical: model browsing/pulling, VRAM-aware model selection, warm/cold state awaren…
