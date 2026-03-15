---
id: model-degradation
title: "Graceful Model Degradation & Provider Resilience"
status: implemented
tags: [routing, resilience, architecture]
open_questions: []
issue_type: feature
priority: 1
---

# Graceful Model Degradation & Provider Resilience

## Overview

The current model routing system has hard boundaries between capability tiers. When all candidates for a role are unavailable (API key missing, model deprecated, auth failure), resolution returns `{ok: false}` with no fallback. This creates a cliff — users go from full capability to zero instead of gracefully degrading.\n\nGoal: build a degradation ladder from zero-provider (clear guidance) through single-provider (that provider serves all roles at varying quality) to full-matrix (optimal routing across Anthropic + OpenAI + local).\n\n## Current Problems\n\n1. **No cross-tier degradation**: If gloriana has no candidates, it fails — never tries victory-tier models as a fallback\n2. **Auth errors (403/401) don't failover**: They surface immediately instead of trying the next candidate\n3. **No zero-provider path**: No API keys + no Ollama = opaque failure, no guidance\n4. **Deprecated models cause retry loops**: Provider still lists them, routing selects them, they 403, classified as auth error, surfaced to user\n5. **GitHub Copilot isn't a first-class provider**: Copilot tokens hit OpenAI-compatible endpoints but with different model availability\n\n## Degradation Ladder (target)\n\n```\nLevel 0: Nothing       → Clear error: 'Configure at least one provider. Run /bootstrap'\nLevel 1: Local only    → Ollama serves ALL roles (reduced quality, not hard failure)\nLevel 2: One cloud key → That provider fills all tiers, local for background\nLevel 3: Both clouds   → Full tier separation, local augments\nLevel 4: All + local   → Complete matrix, optimal routing\n```">

## Research

### Registry & Provider Landscape Audit (March 2026)



### The Scale Problem

pi's built-in model registry has **23 providers** and **792 models**. Our routing only knows about **3 providers** (anthropic, openai, local) — leaving **731 models across 20 providers completely invisible** to tier routing.

### GitHub Copilot Is a Meta-Provider

The github-copilot provider is a single OAuth subscription that unlocks **23 models across 4 vendors**:
- **Claude**: haiku-4.5, sonnet-4/4.5/4.6, opus-4.5/4.6
- **GPT**: 4.1, 4o, 5, 5-mini, 5.1, 5.1-codex, 5.2, 5.2-codex, 5.3-codex, 5.4
- **Gemini**: 2.5-pro, 3-flash-preview, 3-pro-preview, 3.1-pro-preview
- **Grok**: code-fast-1

A Copilot subscriber has access to models spanning ALL tiers (archmagos through servoskull) through ONE auth credential. But our routing never looks at `github-copilot` provider models — it only checks `anthropic` and `openai`.

### The Driver vs Available Models Gap

- `ctx.modelRegistry.getAll()` returns ALL 792 built-in models regardless of auth
- `ctx.modelRegistry.getAvailable()` returns only models where `authStorage.hasAuth(provider)` is true
- Our `resolveTier()` takes `models: RegistryModel[]` and filters by provider name (`m.provider === 'anthropic'`, etc.)
- **We pass `getAll()` not `getAvailable()`** — meaning we try to route to models the user has no auth for, then get 403s

### Auth Methods

pi supports multiple auth sources per provider:
- **API keys**: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc. (env var or config)
- **OAuth/subscription**: GitHub Copilot, Google (via /login)
- **Cloud provider**: AWS (Bedrock), Azure, GCP Vertex — use cloud CLIs

### Deprecated Models Still Listed

The built-in registry lists **all historical models** including deprecated ones:
- openai provider: gpt-4, gpt-4-turbo, gpt-4o, gpt-4o-mini, gpt-4.1, gpt-4.1-mini, o4-mini — all retired
- anthropic provider: claude-3-haiku, claude-3-sonnet, claude-3-opus — ancient
- github-copilot: gpt-4o, gpt-4.1 — deprecated but still listed

### Key Insight: Three Separate Problems

1. **Discovery**: Which providers does the user actually have auth for? (`getAvailable()`)
2. **Filtering**: Which models within auth'd providers are actually current? (DEPRECATED_MODELS)
3. **Mapping**: Which surviving models map to which capability tiers? (tier matching)

Currently we do step 3 on unfiltered step-1 data, skipping step 2 entirely.

### Provider-Aware Tier Matching Architecture



### Current: Provider-Hardcoded Matching

```
resolveTier('gloriana', allModels, policy)
  → matchAnthropicTier(models, 'gloriana')  // only m.provider === 'anthropic'
  → matchOpenAITier(models, 'gloriana')     // only m.provider === 'openai'
  → matchLocalTier(models)                  // only m.provider === 'local'
```

This is a closed system — adding a new provider requires code changes.

### Proposed: Capability-Based Matching

Instead of hardcoding provider→tier mappings, match by model capabilities:

```typescript
interface ModelCapability {
  tier: 'archmagos' | 'magos' | 'adept' | 'servitor' | 'servoskull';
  contextWindow: number;
  reasoning: boolean;
  costTier: 'premium' | 'standard' | 'economy';
}
```

A model qualifies for a tier based on its properties, not its provider name. This would let github-copilot's claude-opus-4-6 naturally land in archmagos without special-casing.

### Hybrid Approach (Recommended)

1. **Known models**: Explicit tier mapping for models we've tested and validated (current approach, extended to more providers)
2. **Unknown models**: Capability-based inference for models we haven't mapped — context window, cost, reasoning flag → estimated tier
3. **Provider-transparent**: Match across ALL auth'd providers, not just anthropic/openai

### Impact on Auth Checking

With `getAvailable()`, a GitHub Copilot subscriber would see:
- github-copilot/claude-opus-4-6 → archmagos ✓
- github-copilot/gpt-5.4 → archmagos ✓  
- github-copilot/claude-sonnet-4-6 → magos ✓
- github-copilot/gpt-5.3-codex-spark → magos ✓

A single $10/mo subscription fills EVERY tier. But with current routing, these are invisible because `m.provider !== 'anthropic'` and `m.provider !== 'openai'`.

### API Key + Subscription Coexistence

A user might have:
- Anthropic API key (direct, pay-per-token, full model access)  
- GitHub Copilot subscription (flat rate, subset of models)
- Ollama local

The routing should prefer direct API keys (full control, latest models) but failover to Copilot when the direct provider is unavailable. Provider ordering already exists in `ProviderRoutingPolicy.providerOrder` — it just needs to include more providers.

## Decisions

### Decision: Cross-tier degradation is confirmation-gated on failure

**Status:** decided
**Rationale:** When a tier's candidates all fail (auth error, deprecation, rate limit), the system should propose degrading to the next tier down with operator confirmation: 'Archmagos unavailable (no viable gpt-5.4 or opus-4-6). Use Magos (sonnet-4-6) instead? [y/n]'. This preserves operator agency — a silent downgrade from opus to haiku mid-session would be surprising and potentially produce worse output without the operator knowing why. Automatic degradation is acceptable only for background/internal tasks (extraction, compaction, episode generation) where the operator doesn't directly see the output.

### Decision: Use getAvailable() not getAll() for tier resolution

**Status:** decided
**Rationale:** The root cause of the 403 loop: we pass `getAll()` (792 models, no auth check) to `resolveTier()`, which happily selects models the user has no credentials for. Must switch to `getAvailable()` (only models where `authStorage.hasAuth(provider)` is true). This alone would have prevented the gpt-4o 403 — if the user only has Anthropic keys, OpenAI models never enter the candidate pool. DEPRECATED_MODELS filtering then acts as a second layer on top of auth-gated discovery.

### Decision: Proactive registry-time filtering with provider-aware deprecation

**Status:** decided
**Rationale:** Deprecated models should be filtered from the available pool BEFORE tier matching, not just skipped during selection. The filter should be provider-aware: gpt-4o is deprecated on both `openai` and `github-copilot`, but github-copilot may retire models on a different schedule than direct OpenAI API. The filtered pool becomes the single source of truth for all routing decisions. This changes the pipeline to: getAvailable() → filterDeprecated() → resolveTier(). The /models visualization should show this filtered view with clear indication of what was removed and why.

### Decision: GitHub Copilot treated as first-class provider via universal tier rules

**Status:** decided
**Rationale:** Rather than special-casing github-copilot as a distinct provider type, we made tier matching provider-transparent. TIER_RULES matches model IDs by pattern (prefix/exact) regardless of which provider serves them. This means github-copilot's claude-opus-4-6, gpt-5.4, gemini-3-pro, etc. all naturally land in the correct tiers without any copilot-specific code. This also future-proofs for Google, xAI, Groq, Bedrock, Azure, and any other provider — they all participate in routing automatically if the user has auth configured. ProviderName type widened to include all known providers. Default policy providerOrder includes all providers in preference order: anthropic > openai > github-copilot > google > xai > groq > mistral > bedrock > azure > openrouter > local.

### Decision: Local models are NOT universal fallback candidates

**Status:** decided
**Rationale:** Most machines don't have useful local inference capacity. Even on Apple Silicon with MLX, local models barely reach useful capability. Local stays only in explicitly configured roles (servitor/servoskull) for embeddings and image generation — never injected as last-resort into archmagos/magos/adept. Degradation between cloud tiers is handled by cross-tier confirmation-gated fallback, not by silently dropping to a 4B local model.

### Decision: Startup provider summary shows degraded tiers, silent when all operational

**Status:** decided
**Rationale:** buildProviderSummary() compares getAll() vs getViableModels() to show auth'd/unauth'd providers and per-tier status (operational/degraded/unavailable). At startup: level 0 shows warning to run /bootstrap, levels 1-2 show tier status with ● operational / ◐ degraded / ○ unavailable, level 3 (all tiers operational) is silent. /providers command gives full detail with candidate tables. Tied into auth via getAvailable() — provider summary directly reflects which API keys and OAuth tokens are configured.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/lib/model-routing.ts` — TIER_RULES, matchTierUniversal, getViableModels, filterDeprecated, buildProviderSummary, DEPRECATED_MODELS
- `extensions/lib/provider-env.ts` — PROVIDER_ENV_VARS map (13 providers), @secret annotations, getProviderRemediationHint, isProviderEnvConfigured
- `extensions/effort/index.ts` — startup provider summary, /providers command with remediation hints, bootstrapPending suppression
- `extensions/00-secrets/index.ts` — bare-env warning, CLI_MANAGED_TOKENS exemption, env-shadow detection on /secrets configure, collapsed /secrets list
- `extensions/bootstrap/index.ts` — API key guidance on first-run, bootstrapPending flag, cloud auth check after deps
- `extensions/offline-driver.ts` — bootstrapPending-aware notification suppression
- `extensions/shared-state.ts` — bootstrapPending flag for first-run coordination
- `extensions/lib/provider-env.test.ts` — 18 tests: map validation, remediation hints, vertex ADC conjunction

### Constraints

- Cross-tier confirmation-gated degradation is decided but NOT yet implemented — future work
- Local models NOT universal fallback — stays in servitor/servoskull only
- isProviderEnvConfigured exported+tested but not yet consumed in production
