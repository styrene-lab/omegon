+++
id = "50d76db4-e782-466d-b281-d2852f93392b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Orchestratable provider model — treat providers as assignable resources, not user preferences — Design Spec (extracted)

> Auto-extracted from docs/orchestratable-provider-model.md at decide-time.

## Decisions

### Provider routing produces ranked candidates, not a single choice (decided)

The current auto_detect_bridge returns the first match from a hardcoded fallback list. The orchestratable model replaces this with route(request, inventory, policy) → Vec<(provider, model, score)>. The caller picks the top candidate. If it fails at runtime, the next candidate is tried. This means the fallback chain is no longer a separate concept — it's the natural consequence of walking down a scored ranking. auto_detect_bridge becomes `route(Default, inventory, default_policy)[0]` — backward compatible.

### Capability tiers, not model names, drive routing (decided)

Tasks request a CapabilityTier (Leaf/Mid/Frontier/Max), not a specific model. The router maps the tier to concrete provider+model using the inventory. This insulates orchestration from model churn — when gpt-5.5 drops or Qwen4 releases, the router adapts without touching task assignment logic. The existing effort tiers (Servitor→Omnissiah) map 1:1: Servitor→Leaf, Adept→Mid, Magos→Frontier, Archmagos/Omnissiah→Max.

### Interactive chat preserves operator provider preference (decided)

The orchestratable model doesn't override the operator's choice for interactive chat. If the operator says /model anthropic:opus, that's what drives the conversation. The routing engine only takes over for background tasks (cleave children, memory extraction, compaction) where the operator hasn't expressed a preference and cost/capability optimization matters. The primary bridge remains Arc<RwLock> with hot-swap — the BridgeFactory sits alongside it for orchestrated tasks.

### No cost tracking in V1 — route by tier and credential availability (decided)

Cost tracking requires provider-specific usage parsing from every SSE response (each provider reports tokens differently or not at all). This is scope creep for V1. The routing signal we have — which providers are authenticated, what capability tier the task needs, and operator preference — is sufficient to make good assignments. Cost tracking is a V2 concern after the routing infrastructure proves itself.

### V1 budget signal is implicit: authenticated providers = available budget (decided)

The operator already tells us their budget posture by which providers they've authenticated. Someone with only Ollama and Codex Spark (free) has a zero-cost posture. Someone with Anthropic API key + OpenAI API key has a premium posture. The routing engine respects effort tier caps (/effort command) and the existing cheapCloudPreferredOverLocal session policy bit. Explicit budget ceilings are a V2 UX surface that can layer on top.

### Scope-based heuristic for V1, per-project override deferred (decided)

A V1 heuristic (scope size + keywords in description → tier) is sufficient to prove the routing concept. Per-project configuration (e.g. omegon.toml tier overrides) can be added once we have data on how well the heuristic performs across real cleave runs. The heuristic is also overridable at the ChildPlan level if the plan JSON includes an explicit executeModel.

## Research Summary

### Current architecture — single-bridge model

**How it works today:**

```
Startup:
  auto_detect_bridge(model_spec) → pick first available → Arc<RwLock<Box<dyn LlmBridge>>>
  
Interactive chat:
  bridge.read().stream(prompt, messages, tools, options) → single provider
  
Cleave children:
  CleaveConfig { model: "anthropic:claude-sonnet-4-6", ... }
  → ALL children use the same model string
  → Each child re-runs auto_detect_bridge independently
  
Hot-swap:
  /login success OR /model provider change → bridge.write() = new_bridge
```

**Lim…

### Target architecture — ProviderPool and task-aware routing

**The shift:** provider is not a user preference — it's an orchestration resource.

```
┌──────────────────────────────────────────────────────────┐
│                   ProviderInventory                       │
│  ┌──────────┐ ┌──────────┐ ┌───────┐ ┌───────┐ ┌──────┐│
│  │Anthropic │ │ Codex    │ │ Groq  │ │  HF   │ │Ollama││
│  │ opus,son.│ │ spark(f) │ │ llama │ │ qwen3 │ │30B,8B││
│  │$15/$75   │ │ $0       │ │ $0.10 │ │ $0.50 │ │ free ││
│  │ 200K ctx │ │ 128K ctx │ │ 128K  │ │ 128K  │ │ 32…

### Level of Effort breakdown

**Total: ~5 features, estimated 3-4 focused sessions**

Each feature is independently shippable — no big-bang required.

---

**Feature 1: ProviderInventory** — S/M (1 session, possibly same session as F2)

*What*: Struct that holds which providers have credentials, what models they offer, and for Ollama, what's installed/running. Probed at startup.

*Already have*: splash screen probes already check providers. `resolve_api_key_sync()` tests credential existence. `auth::PROVIDERS` is the registr…
