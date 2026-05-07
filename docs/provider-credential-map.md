+++
id = "8e641402-3344-4a0e-8a6f-25a16f47fe1d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider Credential Map

Canonical reference for how each service's credentials are stored, resolved, and refreshed. The single source of truth is `auth::PROVIDERS` in `core/crates/omegon/src/auth.rs`. This document reflects that code.

## Auth Method Types

| Type | How it works | Storage | Refresh |
|---|---|---|---|
| **OAuth** | Browser flow → callback → token exchange | auth.json (access + refresh) | Automatic on expiry |
| **API Key** | Direct value, no expiry | auth.json + OS keyring | Never (manual rotation) |
| **Dynamic** | CLI tool executed on demand | secrets.json recipe | Every invocation |

## Wire Protocols

The Rust provider clients implement 3 wire protocols that cover the entire inference landscape:

| Protocol | Client | Providers |
|---|---|---|
| **anthropic-messages** | `AnthropicClient` | Anthropic (API key + OAuth) |
| **openai-completions** | `OpenAICompatClient` | OpenAI, OpenRouter, Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama |
| **openai-codex-responses** | `CodexClient` | OpenAI Codex (ChatGPT OAuth JWT) |

Adding a new OpenAI-compatible provider: one struct entry in `auth::PROVIDERS` with `openai_compat_url`.

## Provider Map

### LLM Providers — Proprietary Protocol

| Provider | Auth Type | Env Var | auth.json Key | Wire Protocol |
|---|---|---|---|---|
| Anthropic | API Key / OAuth | `ANTHROPIC_API_KEY`, `ANTHROPIC_OAUTH_TOKEN` | `anthropic` | anthropic-messages |
| OpenAI Codex (ChatGPT) | OAuth | `CHATGPT_OAUTH_TOKEN` | `openai-codex` | openai-codex-responses |

Anthropic OAuth is the sanctioned Claude Code / subscription-adjacent route for interactive use, but not an unrestricted automation credential. OpenAI Codex / ChatGPT OAuth is currently an experimental consumer route in Omegon, not a first-class supported backend.

### LLM Providers — OpenAI-Compatible

| Provider | Env Var | auth.json Key | Base URL | Notes |
|---|---|---|---|---|
| OpenAI | `OPENAI_API_KEY` | `openai-codex` | `api.openai.com` | API key billing (sk-...) |
| OpenRouter | `OPENROUTER_API_KEY` | `openrouter` | `openrouter.ai/api` | 200+ models, free tier |
| Groq | `GROQ_API_KEY` | `groq` | `api.groq.com/openai` | Ultra-fast inference |
| xAI (Grok) | `XAI_API_KEY` | `xai` | `api.x.ai` | Grok models |
| Mistral AI | `MISTRAL_API_KEY` | `mistral` | `api.mistral.ai` | Codestral, Mistral Large |
| Cerebras | `CEREBRAS_API_KEY` | `cerebras` | `api.cerebras.ai` | Hardware-accelerated |
| Hugging Face | `HF_TOKEN` | `huggingface` | `router.huggingface.co` | OSS frontier models |
| Ollama (Local) | `OLLAMA_HOST` | `ollama` | `localhost:11434` | No API key — your hardware |

### Search Providers (web_search tool)

| Provider | Env Var | auth.json Key |
|---|---|---|
| Brave | `BRAVE_API_KEY` | `brave` |
| Tavily | `TAVILY_API_KEY` | `tavily` |
| Serper | `SERPER_API_KEY` | `serper` |

### Git Forges

| Provider | Auth Type | Env Var | Resolution |
|---|---|---|---|
| GitHub | Dynamic | `GITHUB_TOKEN` / `GH_TOKEN` | `cmd:gh auth token` |
| GitLab | API Key | `GITLAB_TOKEN` | Direct / keyring |

## Fallback Chain

When the requested provider is unavailable, `auto_detect_bridge` tries providers in this order:

1. **Anthropic** — primary, highest quality
2. **OpenAI** (API key) — Chat Completions
3. **OpenAI Codex** (OAuth JWT) — Responses API, includes free models
4. **Groq** — fast inference
5. **xAI** — Grok models
6. **Mistral** — Codestral
7. **HuggingFace** — OSS frontier
8. **Cerebras** — hardware-accelerated
9. **OpenRouter** — universal aggregator (200+ models, free tier)
10. **Ollama** — local inference (last resort, but the most durable)

## Resolution Order

For each provider, credentials are resolved in this order:

1. Environment variables (non-OAuth)
2. OAuth token env vars (ANTHROPIC_OAUTH_TOKEN, CHATGPT_OAUTH_TOKEN)
3. `~/.pi/agent/auth.json` — with automatic token refresh for OAuth

## Adding a New Provider

1. Add a `ProviderCredential` entry to `auth::PROVIDERS` in `core/crates/omegon/src/auth.rs`
2. For OpenAI-compatible: set `openai_compat_url = Some("https://...")` — done!
3. For proprietary protocol: add a `match` arm in `resolve_provider()` in `providers.rs`
4. Update this document

## Strategic Notes

**OpenRouter** provides critical universal coverage while it exists, but as a startup it's not infrastructure. Every direct provider client is insurance against its disappearance.

**Ollama** is the differentiating factor. No other harness treats local models as first-class citizens in orchestration routing. The path forward is VRAM-aware model assignment during cleave — local models for leaf tasks, cloud for frontier reasoning.

**HuggingFace** is the canonical OSS model registry. Its inference API serves frontier open models (DeepSeek-R1, Qwen3-235B, GLM-5). For local model browsing, operators should visit huggingface.co/models or ollama.com/library.

**Google Gemini** uses a proprietary SDK protocol (not OpenAI-compatible). Deferred — accessible via OpenRouter today, native client when demand warrants.
