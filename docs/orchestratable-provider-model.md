---
id: orchestratable-provider-model
title: Orchestratable provider model — treat providers as assignable resources, not user preferences
status: implemented
parent: bridge-provider-routing
tags: [architecture, providers, orchestration, cleave, local-inference, routing, strategic, epic]
open_questions: []
branches: ["feature/orchestratable-provider-model"]
openspec_change: orchestratable-provider-model
jj_change_id: syvtxqvswmlukkmvzlpmmoxxqlsluysu
issue_type: epic
priority: 1
---

# Orchestratable provider model — treat providers as assignable resources, not user preferences

## Overview

Transform provider handling from 'pick one at startup, fallback if it fails' to 'maintain an inventory of available providers, assign them to tasks based on cost/capability/latency requirements during orchestration'. The single Arc<RwLock<Box<dyn LlmBridge>>> becomes a ProviderPool. Cleave children get per-task provider assignments. Local inference becomes a schedulable resource with VRAM awareness. The harness becomes a router, not a client.

## Research

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

**Limitations:**
1. **One provider at a time** — the bridge is singular. Even though cleave children are separate processes, they all get the same `--model` flag from `CleaveConfig.model`.
2. **No task-provider matching** — a leaf task (rename a file) gets the same $15/MTok Opus that an architecture decision gets.
3. **No VRAM awareness** — if Ollama has a 32B model loaded, nothing knows. If it needs 45s to load a 70B model, nothing factors that in.
4. **No cost tracking** — no visibility into what each provider call costs, no budget enforcement.
5. **`execute_model` field exists but is never set** — `ChildState.execute_model` is `Option<String>` in state.rs, but `dispatch_child` never populates it from the plan. The TS `ChildPlan.executeModel` field is also defined but orphaned.

**What already exists as building blocks:**
- `auth::PROVIDERS` with `openai_compat_url` — knows about all providers
- `resolve_provider()` — can create a bridge for any provider ID
- `ChildState.execute_model` — the slot for per-child model assignment
- Effort tiers (Servitor→Omnissiah) — abstract capability labels
- Session budget posture — `ProviderRoutingPolicy` with `providerOrder`, `avoidProviders`, `cheapCloudPreferredOverLocal`
- `resolve_api_key_sync()` — checks if a provider has credentials without creating a bridge
- Ollama `/api/tags` — lists installed models
- Ollama `/api/ps` — lists running (VRAM-loaded) models

### Target architecture — ProviderPool and task-aware routing

**The shift:** provider is not a user preference — it's an orchestration resource.

```
┌──────────────────────────────────────────────────────────┐
│                   ProviderInventory                       │
│  ┌──────────┐ ┌──────────┐ ┌───────┐ ┌───────┐ ┌──────┐│
│  │Anthropic │ │ Codex    │ │ Groq  │ │  HF   │ │Ollama││
│  │ opus,son.│ │ spark(f) │ │ llama │ │ qwen3 │ │30B,8B││
│  │$15/$75   │ │ $0       │ │ $0.10 │ │ $0.50 │ │ free ││
│  │ 200K ctx │ │ 128K ctx │ │ 128K  │ │ 128K  │ │ 32K  ││
│  │ ✓ creds  │ │ ✓ JWT    │ │ ✓ key │ │ ✓ tok │ │ warm ││
│  └──────────┘ └──────────┘ └───────┘ └───────┘ └──────┘│
│                                                          │
│  route(task) → match capability requirement → provider   │
└──────────────────────────────────────────────────────────┘
         │                    │                    │
    Interactive           Cleave child          Background
    (operator pref)       (task-assigned)      (cheapest viable)
```

**Five components:**

**1. ProviderInventory** — runtime snapshot of what's available
- Probed at startup (piggyback on splash screen probes — already exists)
- Refreshed on /login, /model, or credential change
- For each provider: has_credentials, available_models, cost_tier, context_window, capabilities (reasoning, vision, tools)
- For Ollama: installed_models, running_models (VRAM-loaded), available_vram

**2. CapabilityRequest** — what a task needs
- Required: min_capability (leaf/mid/frontier), tool_support, min_context_window
- Preferred: cost_ceiling, latency_target, prefer_local, avoid_providers
- Cleave planner already has this concept in `ChildPlan.executeModel` — just underspecified

**3. ProviderRouter** — the matching function
- `route(request: CapabilityRequest, inventory: &ProviderInventory) -> Vec<ProviderCandidate>`
- Returns ranked candidates, not a single choice — fallback built in
- Operator preference is an input bias, not a hard override
- Budget posture (from effort tiers / session policy) constrains the candidate set

**4. BridgeFactory** — create bridges on demand
- `create_bridge(provider_id: &str, model: &str) -> Box<dyn LlmBridge>`
- Pool of warm bridges for frequently-used providers
- Bridges are cheap to create (just an HTTP client + credentials)

**5. Cleave integration** — per-child provider assignment
- Planner annotates each child with a CapabilityRequest based on task complexity
- Orchestrator calls route() for each child before dispatch
- `--model` flag per child, not global
- Dashboard shows which provider each child is using
- Progress tracking includes cost attribution per child

### Level of Effort breakdown

**Total: ~5 features, estimated 3-4 focused sessions**

Each feature is independently shippable — no big-bang required.

---

**Feature 1: ProviderInventory** — S/M (1 session, possibly same session as F2)

*What*: Struct that holds which providers have credentials, what models they offer, and for Ollama, what's installed/running. Probed at startup.

*Already have*: splash screen probes already check providers. `resolve_api_key_sync()` tests credential existence. `auth::PROVIDERS` is the registry.

*Build*:
- `ProviderInventory` struct in new `core/crates/omegon/src/routing.rs`
- `probe_inventory()` function — iterates PROVIDERS, calls `resolve_api_key_sync()`, populates
- Ollama probe: HTTP GET `/api/tags` (installed models) and `/api/ps` (running models, VRAM)
- Store in `Arc<RwLock<ProviderInventory>>` alongside the bridge
- Refresh on `/login` success, `/model` change

*Seam*: The splash screen `startup.rs` already probes providers. Unify that probe into ProviderInventory so the splash reads from it instead of doing independent checks.

*Risk*: Low. Mostly data aggregation, no control flow changes.

---

**Feature 2: CapabilityRequest + ProviderRouter** — M (1 session)

*What*: Define what a task needs (capability tier, context window, cost ceiling) and a function that matches it against the inventory.

*Already have*: effort tier labels (local/haiku/sonnet/opus → Servitor/Adept/Magos/Archmagos). Model-routing.ts `resolveTier()`. Session budget posture.

*Build*:
- `CapabilityTier` enum: `Leaf`, `Mid`, `Frontier`, `Max` (maps to effort tiers)
- `CapabilityRequest` struct: `tier`, `min_context_k`, `tool_support`, `prefer_local`, `cost_ceiling_per_mtok`, `avoid_providers`
- `route(req, inventory, policy) -> Vec<(provider_id, model_id, score)>`
- Scoring: tier match × cost penalty × latency bonus × local preference
- Unit tests with mock inventories

*Key decision*: The router produces a ranked list, not a single answer. The caller picks the top candidate and falls back down the list on failure. This is how the fallback chain naturally evolves — from a hardcoded list to a scored ranking.

*Seam*: `auto_detect_bridge` becomes `route(Default::default(), inventory, policy)[0]`. Backward compatible.

*Risk*: Medium. The scoring function needs tuning, but it only affects quality of assignment, not correctness. A bad assignment still works — it's just suboptimal cost-wise.

---

**Feature 3: BridgeFactory + warm pool** — S (partial session)

*What*: Create `Box<dyn LlmBridge>` on demand from (provider_id, model_id). Cache warm bridges.

*Already have*: `resolve_provider()` does exactly this. Just needs a wrapper that caches.

*Build*:
- `BridgeFactory` struct wrapping `HashMap<String, Box<dyn LlmBridge>>`
- `get_or_create(provider_id, model_id) -> &dyn LlmBridge`
- Eviction: LRU or time-based (bridges are cheap, just HTTP clients + credentials)
- Replace single `Arc<RwLock<Box<dyn LlmBridge>>>` with `Arc<RwLock<BridgeFactory>>`
- Primary bridge tracked separately for interactive chat

*Risk*: Low. Bridges are stateless HTTP clients. Caching is optimization, not correctness.

---

**Feature 4: Per-child provider assignment in cleave** — M (1 session)

*What*: The cleave orchestrator assigns a provider+model per child based on task complexity, instead of giving every child the same `--model` flag.

*Already have*: `CleaveConfig.model` (global). `ChildState.execute_model` (per-child slot, never populated). `ChildPlan.executeModel` in TS (also orphaned).

*Build*:
- `CleaveConfig` gains `inventory: Arc<RwLock<ProviderInventory>>`
- During plan → dispatch, for each child:
  - Infer `CapabilityRequest` from child description + scope size
  - Call `route()` to get provider+model
  - Pass `--model provider:model` per child
  - Populate `ChildState.execute_model`
- Dashboard shows provider attribution per child
- Simple heuristic for V1: scope ≤ 2 files → Leaf, scope ≤ 5 → Mid, else → Frontier

*Seam*: `dispatch_child` already takes `model: &str`. Just change where it comes from: `config.model` → per-child resolution.

*Risk*: Medium. Wrong assignment is recoverable (child fails, gets retried with a different provider). But the heuristic needs calibration against real cleave runs.

---

**Feature 5: Ollama model management in Rust** — M (1 session)

*What*: Native Rust methods for Ollama model lifecycle: list installed, list running (VRAM), pull, recommend based on hardware.

*Already have*: TS manage_ollama with start/stop/status/pull. Ollama REST API at /api/*.

*Build*:
- `OllamaManager` struct in `core/crates/omegon/src/ollama.rs`
- Methods: `list_models()`, `list_running()`, `pull_model()`, `start()`, `stop()`, `available_vram()`
- `/models` slash command: list installed, show VRAM usage, link to ollama.com/library
- Integrate with ProviderInventory — Ollama inventory includes model-level detail
- Hardware profile: total_vram, current_usage → max model size recommendation

*Seam*: `OpenAICompatClient::from_env_ollama()` currently does a bare TCP connect. Replace with `OllamaManager::is_reachable()`.

*Risk*: Low for listing/probing. Medium for pull (long-running, progress tracking). Process lifecycle (start/stop) needs platform-specific handling (already solved in TS).

---

**Sequencing:**

```
F1 (ProviderInventory)  ─────┐
                              ├──▶ F2 (Router)  ──▶ F4 (Cleave integration)
F3 (BridgeFactory)      ─────┘
F5 (Ollama management) ─────────── independent, enriches F1
```

F1+F3 are prerequisites for F2. F2 is prerequisite for F4. F5 is independent but makes F1's Ollama data richer. Could ship F1+F3+F5 as a first milestone, then F2+F4 as the orchestration layer.

**Conservative total**: 4 sessions × 3-4 hours = 12-16 hours of focused implementation.
**Aggressive (with /cleave)**: 2-3 sessions — F1+F3 are highly parallelizable with F5.

## Decisions

### Decision: Provider routing produces ranked candidates, not a single choice

**Status:** decided
**Rationale:** The current auto_detect_bridge returns the first match from a hardcoded fallback list. The orchestratable model replaces this with route(request, inventory, policy) → Vec<(provider, model, score)>. The caller picks the top candidate. If it fails at runtime, the next candidate is tried. This means the fallback chain is no longer a separate concept — it's the natural consequence of walking down a scored ranking. auto_detect_bridge becomes `route(Default, inventory, default_policy)[0]` — backward compatible.

### Decision: Capability tiers, not model names, drive routing

**Status:** decided
**Rationale:** Tasks request a CapabilityTier (Leaf/Mid/Frontier/Max), not a specific model. The router maps the tier to concrete provider+model using the inventory. This insulates orchestration from model churn — when gpt-5.5 drops or Qwen4 releases, the router adapts without touching task assignment logic. The existing effort tiers (Servitor→Omnissiah) map 1:1: Servitor→Leaf, Adept→Mid, Magos→Frontier, Archmagos/Omnissiah→Max.

### Decision: Interactive chat preserves operator provider preference

**Status:** decided
**Rationale:** The orchestratable model doesn't override the operator's choice for interactive chat. If the operator says /model anthropic:opus, that's what drives the conversation. The routing engine only takes over for background tasks (cleave children, memory extraction, compaction) where the operator hasn't expressed a preference and cost/capability optimization matters. The primary bridge remains Arc<RwLock> with hot-swap — the BridgeFactory sits alongside it for orchestrated tasks.

### Decision: No cost tracking in V1 — route by tier and credential availability

**Status:** decided
**Rationale:** Cost tracking requires provider-specific usage parsing from every SSE response (each provider reports tokens differently or not at all). This is scope creep for V1. The routing signal we have — which providers are authenticated, what capability tier the task needs, and operator preference — is sufficient to make good assignments. Cost tracking is a V2 concern after the routing infrastructure proves itself.

### Decision: V1 budget signal is implicit: authenticated providers = available budget

**Status:** decided
**Rationale:** The operator already tells us their budget posture by which providers they've authenticated. Someone with only Ollama and Codex Spark (free) has a zero-cost posture. Someone with Anthropic API key + OpenAI API key has a premium posture. The routing engine respects effort tier caps (/effort command) and the existing cheapCloudPreferredOverLocal session policy bit. Explicit budget ceilings are a V2 UX surface that can layer on top.

### Decision: Scope-based heuristic for V1, per-project override deferred

**Status:** decided
**Rationale:** A V1 heuristic (scope size + keywords in description → tier) is sufficient to prove the routing concept. Per-project configuration (e.g. omegon.toml tier overrides) can be added once we have data on how well the heuristic performs across real cleave runs. The heuristic is also overridable at the ChildPlan level if the plan JSON includes an explicit executeModel.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/routing.rs` (new) — ProviderInventory, CapabilityRequest, CapabilityTier, ProviderRouter with route() function, BridgeFactory with warm pool
- `core/crates/omegon/src/ollama.rs` (new) — OllamaManager: list_models, list_running, pull_model, start/stop, available_vram, hardware profile
- `core/crates/omegon/src/providers.rs` (modified) — auto_detect_bridge delegates to route(). resolve_provider becomes BridgeFactory::create()
- `core/crates/omegon/src/cleave/orchestrator.rs` (modified) — CleaveConfig gains inventory. dispatch_child resolves per-child model via router. Populates ChildState.execute_model
- `core/crates/omegon/src/cleave/state.rs` (modified) — ChildState.execute_model always populated. Add provider_id field
- `core/crates/omegon/src/main.rs` (modified) — Create ProviderInventory at startup. Pass to CleaveConfig. Refresh on /login and /model
- `core/crates/omegon/src/startup.rs` (modified) — Splash screen probes unified into ProviderInventory.probe()
- `core/crates/omegon/src/tui/bootstrap.rs` (modified) — Bootstrap panel reads from ProviderInventory instead of independent probes

### Constraints

- Backward compatible: auto_detect_bridge still works with model_spec string for non-orchestrated callers
- Interactive chat bridge (Arc<RwLock<Box<dyn LlmBridge>>>) preserved — operator preference honored
- Cleave children are separate processes — provider assignment is via --model flag, not shared memory
- Ollama model management must be async — pull_model can take minutes for large models
- No cost tracking in V1 — routing by tier and credential availability only
- ProviderInventory probe must complete in <500ms — no blocking on slow endpoints
