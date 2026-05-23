+++
id = "b65811bc-dccd-4b6d-af98-d2e8e023c47d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Effort Tiers — Global Inference Cost Control (Servitor → Omnissiah)

## Disposition — 2026-05-23

**Status: partially superseded / stale implementation scope.** This node describes a TypeScript `extensions/effort/` control plane and seven named tiers. The current Rust-native model budget feature lives in `core/crates/omegon/src/features/model_budget.rs` and uses the current tier vocabulary `Local`, `Retribution`, `Victory`, and `Gloriana`; the referenced `extensions/effort/*`, `extensions/model-budget.ts`, and `project-memory/*` paths are absent.

Keep this document for the durable idea of a global cost/capability knob and operator-visible ceilings. Reconcile against `codex-tier-routing.md`, `provider-neutral-model-controls.md`, and the Rust model registry before using any tier names, file scope, or cloud/local percentages as current behavior.

## Overview

A single global knob controlling the ratio of local-vs-cloud inference across the entire harness. Seven named tiers inspired by Space Marine 2 difficulty levels and 40K threat designations. From fully local (Servitor) to all-opus-all-the-time (Omnissiah). Replaces the current patchwork of per-extension model selection with a unified control plane.

## Research

### Decision Points Inventory — What the Effort Knob Controls

**Seven distinct inference decision points across the harness:**

| # | Decision Point | Location | Current Control | Effort Would Set |
|---|---------------|----------|-----------------|-----------------|
| 1 | **Driver model** (the main agent) | `model-budget.ts` → `pi.setModel()` | Agent calls `set_model_tier` voluntarily | Programmatic: `pi.setModel()` + `pi.setThinkingLevel()` |
| 2 | **Extraction model** | `project-memory/types.ts` → `extractionModel` | Config default `devstral-small-2:24b` | Override at session start based on tier |
| 3 | **Compaction model** | `project-memory/index.ts` → `compactionLocalFirst` | Config boolean | Boolean flip based on tier |
| 4 | **Cleave child tier** | `cleave/dispatcher.ts` → `resolveExecuteModel()` | `preferLocal` + scope autoclassification | Override `preferLocal` + adjust thresholds |
| 5 | **Review loop model** | `cleave/review.ts` → executor.review() | Hardcoded `"opus"` in dispatcher.ts | Tier-dependent: local/sonnet/opus |
| 6 | **Episode generation** | `project-memory/extraction-v2.ts` → `generateEpisodeDirect` | Always local (qwen3:30b) | Keep local for Servitor-Ruthless, cloud for Lethal+ |
| 7 | **Offline driver** | `offline-driver.ts` → `pi.setModel()` | Manual `/offline` command | Auto-activate at Servitor/Average |

**Existing infrastructure that already supports this:**
- `sharedState` (globalThis symbol) for cross-extension reads
- `pi.setModel()` + `pi.setThinkingLevel()` for driver control
- `pi.registerProvider("local", ...)` for Ollama models
- `switch_to_offline_driver` tool for agent self-service
- `preferLocal` param already plumbed into cleave_run
- `config.extractionModel` already a mutable string

### Tier Matrix — Concrete Behavior Per Level

| Tier | Name | Driver | Thinking | Extraction | Compaction | Cleave Default | Review | Est. Cloud % |
|------|------|--------|----------|------------|------------|----------------|--------|:------------:|
| 1 | **Servitor** | local (offline) | off | local | local | all local | local | **0%** |
| 2 | **Average** | local (offline) | minimal | local | local | scope-based (local bias) | local | **0%** |
| 3 | **Substantial** | sonnet | low | local | local | scope-based (normal) | sonnet | **~30%** |
| 4 | **Ruthless** | sonnet | medium | local | local | scope-based (normal) | sonnet | **~45%** |
| 5 | **Lethal** | sonnet+opus | high | local | local | scope-based + opus big | opus | **~65%** |
| 6 | **Absolute** | opus | high | sonnet | sonnet | all sonnet, opus review | opus | **~85%** |
| 7 | **Omnissiah** | opus | high | opus | opus | all opus | opus | **100%** |

**Key design principle:** tiers 1-2 are fully local — zero API cost. Tiers 3-4 are the daily driver sweet spot. Tiers 5-7 are for when quality matters more than cost.

**Scope autoclassification interaction:** At tiers 3-4, scope-based autoclassification runs normally (≤3 files→local, 4-8→sonnet, 9+→opus). At tiers 1-2, ALL children are forced local regardless of scope. At tiers 5-7, the floor is raised (e.g., tier 6 forces minimum sonnet).

**Driver model at tier 5 (Lethal):** Starts on sonnet, agent guidelines say "upgrade to opus for architecture and complex debugging." This is the current behavior — just with explicit framing.

**No-fail-past preserved:** At tiers 1-2, children classified local stay local. The effort tier doesn't create an escalation path — it sets the ceiling, not a fallback chain.

### Architecture — Extension Design

**New extension: `extensions/effort/`**

```
extensions/effort/
  index.ts     — /effort command, shared state writer, session_start init
  tiers.ts     — tier definitions, pure functions: tierConfig(level) → EffortConfig
  types.ts     — EffortLevel enum, EffortConfig interface
```

**EffortConfig interface** (what `tierConfig()` returns):
```typescript
interface EffortConfig {
  level: EffortLevel;          // 1-7
  name: string;                // "Servitor" | "Average" | ... | "Omnissiah"
  driver: "local" | "sonnet" | "opus";
  thinking: ThinkingLevel;
  extraction: "local" | "sonnet" | "opus";
  compaction: "local" | "sonnet" | "opus";
  cleavePreferLocal: boolean;
  cleaveFloor: ModelTier;      // minimum tier for cleave children
  reviewModel: "local" | "sonnet" | "opus";
}
```

**How extensions read it:**
1. `sharedState.effort` → `EffortConfig` (always populated by effort extension on session_start)
2. Each extension reads at its decision points. No coupling — just a shared state read.
3. Extensions that load before effort fall back to a sensible default (Substantial / tier 3).

**Persistence:**
- `.omegon/profile.json` → `{"effort": "Ruthless"}` for project-level default
- `PI_EFFORT=Servitor` env var for CI / automation / override
- `/effort <name>` command for mid-session switching
- Priority: env var > command > config > default (Substantial)

**Mid-session switching:**
- `/effort Omnissiah` → writes to shared state → calls `pi.setModel(opus)` + `pi.setThinkingLevel("high")`
- `/effort Servitor` → writes to shared state → calls `pi.setModel(local_model)` + `pi.setThinkingLevel("off")`
- Extensions pick up the change on next decision point (extraction cycle, cleave dispatch, etc.)
- No retroactive effect on in-flight operations

**Integration with model-budget.ts:**
- model-budget currently defaults to opus on session_start — effort extension would supersede this
- Two options: (A) effort absorbs model-budget, or (B) effort sets tier and model-budget's session_start reads it
- Option B is cleaner: model-budget keeps its `set_model_tier` / `set_thinking_level` tools for agent self-service, effort sets the session-start default and provides the `/effort` command

**Integration with adversarial review findings (C1-C3):**
- C1 (fetch not killable) should be fixed regardless — effort doesn't change the bug
- C2 (dead COMPLEX_FILE_PATTERNS) — delete it
- C3 (fragile isLocalModel) — effort makes this less critical because the extraction model is now derived from tier config, not a free-form string. But the fallback path still needs the fix.

### Feasibility Assessment

**Verdict: Highly feasible. Medium implementation effort.**

**What makes this tractable:**
1. Every decision point already has a knob — we're just unifying them under one control
2. `sharedState` is the existing cross-extension communication channel — no new infra needed
3. `pi.setModel()` and `pi.setThinkingLevel()` are proven APIs
4. The offline-driver already handles local model registration and switching
5. Cleave already has `preferLocal`, `executeModel`, and scope-based classification
6. Extraction already has `extractionModel` config and direct Ollama path

**What needs care:**
1. **Extension load order** — effort must initialize shared state before other extensions read it. Pi loads extensions in package.json order, so effort goes first.
2. **model-budget interaction** — the `set_model_tier` / `set_thinking_level` tools let the agent override within a tier's ceiling. Effort sets the default; agent can downgrade but not upgrade past the tier ceiling. Or: the tools remain unconstrained for flexibility, and effort is a "starting position" not a hard cap. Operator can always `/effort` to change.
3. **Local model availability** — tiers 1-2 REQUIRE Ollama running. If Ollama is down, `/effort Servitor` should warn and refuse, or auto-start Ollama (the local-inference extension already has `ollamaStart()`).
4. **The adversarial review bugs (C1-C3)** should be fixed first — they're in the code paths effort will route through.

**Estimated scope:**
- New: `extensions/effort/` (3 files, ~300 lines)
- Modified: `shared-state.ts` (add `effort?: EffortConfig`)
- Modified: `model-budget.ts` (read effort on session_start instead of hardcoding opus)
- Modified: `cleave/index.ts` (read `sharedState.effort` for `preferLocal` + floor)
- Modified: `cleave/dispatcher.ts` (respect cleaveFloor from effort config)
- Modified: `project-memory/index.ts` (read effort for extraction/compaction model)
- Modified: `cleave/review.ts` or `dispatcher.ts` (review model from effort config)
- Bug fixes: C1 (abort controller), C2 (dead code), C3 (isLocalModel)

**Total: ~8 files touched, ~500 lines new, ~100 lines modified. Good /cleave candidate with 3-4 children.**

### OpenAI Codex — First-Class Provider via ChatGPT Pro OAuth

**Discovery**: pi ships `openaiCodexOAuthProvider` as a built-in OAuth provider in `pi-ai/dist/utils/oauth/index.js`. No API key needed — uses PKCE browser OAuth against `auth.openai.com`, stores `chatgpt_account_id` in AuthStorage.

**Auth flow**: `authStorage.login("openai-codex", callbacks)` → browser opens → token stored. Auto-refreshes. The `/login` slash command in pi TUI triggers this.

**Endpoint**: `https://chatgpt.com/backend-api/codex/responses` (WebSocket + REST, not the OpenAI API)

**Available models** (all `reasoning: true`, 272K context unless noted):
| Model | Cost (in/out) | Notes |
|---|---|---|
| `gpt-5.3-codex-spark` | **$0 / $0**, 128K | Free under Pro |
| `gpt-5.1-codex-mini` | $0.25 / $2 | Cheap reasoning |
| `gpt-5.1` | $1.25 / $10 | Mid-tier |
| `gpt-5.2` / `gpt-5.2-codex` | $1.75 / $14 | |
| `gpt-5.3-codex` | $1.75 / $14 | |
| `gpt-5.4` | $2.50 / $15 | Frontier |

**Key implication**: `gpt-5.3-codex-spark` ($0) is a zero-cost cloud reasoning model — it belongs at tier 3-4 (Substantial/Ruthless) alongside Sonnet but from a different provider. The current `EffortConfig.driver: "local" | "sonnet" | "opus"` cannot represent this.

**Thinking format**: Codex models use `reasoning_effort` ("low"/"medium"/"high"), not Anthropic's thinking token budget. The `thinkingFormat` field in ModelDef handles this but `setThinkingLevel()` in model-budget.ts currently maps only to Anthropic's format.

## Decisions

### Decision: /effort cap: locks current tier, agent downgrades only

**Status:** decided
**Rationale:** /effort sets a starting position. /effort cap locks it — agent can downgrade via set_model_tier but cannot upgrade past the cap. Operator is notified. /effort uncap releases the lock. This gives the operator a hard cost ceiling while letting the agent be efficient within it.

### Decision: Coexist with model-budget, clean interface boundary

**Status:** decided
**Rationale:** effort owns tier state + /effort command + session-start defaults. model-budget owns set_model_tier + set_thinking_level tools for agent self-service. Interface: model-budget reads sharedState.effort.cap to enforce ceiling on upgrades. Clean separation supports future providers — effort defines abstract tiers, model-budget maps them to concrete provider models.

### Decision: Fix C1/C2/C3 bugs before implementing effort tiers

**Status:** decided
**Rationale:** Bugs are in code paths effort routes through. Fixing first prevents compounding risk. C1 (fetch abort), C2 (dead code), C3 (isLocalModel heuristic) are all small, targeted fixes.

## Open Questions

*No open questions.*
