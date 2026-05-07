+++
id = "96d129e7-0d3b-4b98-b848-bb8182d044a8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenAI provider identity and routing honesty — separate API vs ChatGPT/Codex auth, route GPT models correctly, and surface the active engine truthfully — Design Spec (extracted)

> Auto-extracted from docs/openai-provider-identity-and-routing-honesty.md at decide-time.

## Decisions

### `openai` auth means OpenAI API credentials only; ChatGPT OAuth remains `openai-codex` (decided)

The harness must stop treating `openai-codex` storage as proof that the generic `openai` provider is executable. `resolve_api_key_sync("openai")`, auth status surfaces, and selector gating should report `openai` only when an actual OpenAI API key or equivalent OpenAI API credential exists. ChatGPT Plus/Pro OAuth stays a distinct provider identity (`openai-codex`) because it uses a different endpoint, auth artifact, and client implementation.

### GPT-family routing may fall through from `openai` intent to `openai-codex` execution when that is the only viable OpenAI-family path (decided)

The operator’s model intent can stay GPT-family / OpenAI-family without lying about the executable backend. If a request targets an `openai:*` GPT-family model and no OpenAI API route is viable, the router may select `openai-codex` as the concrete provider when ChatGPT/Codex OAuth can actually execute the model. The resolved runtime surface must then report the concrete provider as `openai-codex` (or ChatGPT/Codex), not pretend the request ran on generic OpenAI API.

### All operator-facing auth and engine surfaces must show concrete provider identity and method inside the OpenAI family (decided)

The auth status table, model selector gating, startup/bootstrap capability summary, and active engine display should distinguish at least: `OpenAI API` (API key) versus `ChatGPT/Codex` (OAuth). When a conversation is running, the operator should be able to see the resolved provider, resolved model, and credential class in the active engine surface so there is no ambiguity about whether the harness is using API OpenAI or ChatGPT/Codex.

## Research Summary

### Current mismatch in Rust auth and provider resolution

`auth.rs` defines provider id `openai` with `auth_key: "openai-codex"`, so the generic OpenAI surface reads ChatGPT/Codex OAuth credentials from the same auth.json entry used by `openai-codex`. `providers::resolve_api_key_sync("openai")` therefore reports OpenAI as authenticated when only ChatGPT OAuth exists. The model picker in `tui/mod.rs` uses that probe to offer `openai:gpt-5.4`, `openai:o3`, and related GPT-family models. But `resolve_provider("openai")` still constructs `OpenAIClient`, wh…

### Existing routing and UX affordances already imply the needed split

`bridge-provider-routing` already established that provider identity must remain honest when `/model` changes, and `provider-neutral-model-controls` established that the operator must be able to see the concrete active provider/model, not just a tier label. The missing piece is provider-family honesty inside the OpenAI family: `openai` should mean API-key OpenAI unless a route explicitly targets the ChatGPT/Codex client, while GPT-family selection should still be able to land on `openai-codex` w…
