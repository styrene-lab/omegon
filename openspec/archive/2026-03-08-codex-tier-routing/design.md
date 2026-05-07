+++
id = "f73dc2f4-fb0c-46f7-89e5-6bba9feb3976"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Codex Tier Routing — Provider-aware model selection for Cleave and tooling — Design

## Summary

This change keeps canonical planning-time tiers stable (`local|haiku|sonnet|opus`) while introducing a shared runtime routing layer that selects concrete models from Anthropic, OpenAI, or local inference according to a session-scoped operator policy.

Phase 1 preserves internal compatibility and test stability. The main user-visible change is provider-aware execution plus new operator-facing labels:
- Servitor = local
- Adept = haiku-class fast cloud
- Magos = sonnet-class balanced cloud
- Archmagos = opus-class deep cloud

## Architecture decisions

### Decision: Keep public tier semantics stable; introduce provider-aware resolution layer

**Status:** decided

Canonical tier identifiers remain stable for plans, specs, and tool schemas. A shared resolver chooses the actual provider and model ID at runtime.

### Decision: Prefer explicit model IDs over fuzzy tier aliases at execution time

**Status:** decided

Dispatch code resolves concrete model IDs before spawning child agents or switching the interactive driver. This removes provider-specific coupling to fuzzy aliases like `opus`.

### Decision: Model tiers stay abstract; provider choice becomes a session policy

**Status:** decided

Provider ownership of a tier is not hard-coded. A session policy determines which provider should satisfy a requested tier today.

### Decision: Large burns should ask the operator about current provider limits before dispatch

**Status:** decided

Large Cleave runs use a lightweight preflight to update provider preference before dispatch, reducing the chance of hitting subscription/session limits mid-run.

### Decision: Phase 1 keeps internal tier keys for compatibility; UX adopts Servitor/Adept/Magos/Archmagos labels

**Status:** decided

Internal enums and tool schemas stay compatible while operator-facing status text and docs become provider-neutral.

## Data model

### Session routing policy

A lightweight shared-state object captures current operator posture for the session:

```ts
interface ProviderRoutingPolicy {
  providerOrder: Array<"openai" | "anthropic" | "local">;
  avoidProviders: Array<"openai" | "anthropic" | "local">;
  cheapCloudPreferredOverLocal: boolean;
  requirePreflightForLargeRuns: boolean;
  notes?: string;
}
```

This is intentionally policy-oriented rather than quota-oriented. It reflects what the operator wants to favor or avoid right now.

### Resolver result

```ts
interface ResolvedTierModel {
  tier: "local" | "haiku" | "sonnet" | "opus";
  provider: "openai" | "anthropic" | "local";
  modelId: string;
}
```

## Resolver behavior

A shared module under `extensions/lib/` inspects the pi model registry and resolves tiers by:

1. honoring `local` explicitly when requested
2. iterating provider order from session policy
3. skipping avoided providers unless fallback is necessary
4. using per-provider matcher rules to find the best available model for the requested tier
5. returning a concrete `{provider, modelId}` result

The initial matcher set should be minimal:
- Anthropic: prefix matching for haiku/sonnet/opus classes
- OpenAI: configured preferred IDs or prefixes for haiku/sonnet/opus classes
- local: preferred local model selection reused from existing local-model helpers

## Integration points

### model-budget

`set_model_tier` and slash commands continue to accept canonical tier keys, but switching uses the shared resolver and presents thematic display labels.

### effort

Effort tier config remains canonical. Driver switching routes through the shared resolver. Background tasks should prefer inexpensive cloud over local when policy allows and matching cloud models exist.

### Cleave

Cleave preserves `ChildPlan.executeModel` as canonical tier values. At dispatch time, child execution and review both resolve to explicit model IDs and pass those IDs via `pi --model <id>`.

### Preflight

Before a large run, Cleave asks the operator for current provider posture if `requirePreflightForLargeRuns` is true. The answer updates session policy and is used for the pending run.

## File scope

- `extensions/lib/model-routing.ts` — new shared provider-aware resolver and label helpers
- `extensions/shared-state.ts` — add session routing policy state if not already present
- `extensions/model-budget.ts` — use resolver, update UX labels
- `extensions/effort/index.ts` — use resolver for driver/background model selection
- `extensions/cleave/dispatcher.ts` — explicit model-ID dispatch and large-run preflight
- `extensions/cleave/*.test.ts`, `extensions/effort/*.test.ts` — update/add tests
- `README.md` — document provider-aware routing and thematic labels

## Constraints

- Preserve canonical internal keys in phase 1 for compatibility
- Avoid hard-coding Codex as the permanent owner of any tier
- Prefer cheap cloud over local where policy and availability allow
- Keep the session policy simple and operator-driven rather than quota-driven
