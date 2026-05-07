+++
id = "71cab07c-c2f9-48b1-9338-5b16f56853b2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Codex Tier Routing — Provider-aware model selection for Cleave and tooling

## Overview

Explore how Omegon should represent abstract model tiers while allowing Codex/OpenAI-backed execution in Cleave, effort controls, and related tooling without hard-coding Claude-specific tier names throughout the codebase.

## Research

### Current architecture audit

Current model selection is split across three layers. (1) `extensions/model-budget.ts` exposes abstract tool-level switches (`set_model_tier` with `haiku|sonnet|opus`) but resolves those tiers by hard-coded Anthropic prefixes (`claude-haiku`, `claude-sonnet`, `claude-opus`). (2) `extensions/effort/*` stores global effort policy using abstract-ish tiers (`local|sonnet|opus`) but also switches driver models via Anthropic-specific prefix lookup. (3) Cleave stores `ChildPlan.executeModel` as abstract `ModelTier = local|haiku|sonnet|opus`, but `mapModelTierToFlag()` converts tiers to CLI flags assuming Anthropic fuzzy aliases (`haiku`, `opus`, default-sonnet). Result: planning is mostly provider-agnostic, but resolution/execution is Claude-centric at the last mile.

### Key seam for provider abstraction

The cleanest abstraction boundary is to keep planning-time labels capability-based and make provider/model resolution explicit in one shared resolver. Planning constructs (`EffortConfig.driver`, `ChildPlan.executeModel`, review policy) should continue to express intent like `fast`, `balanced`, `deep` or the existing tier names, while one resolver maps that intent to an actual registry model ID or CLI `--model` flag based on an active provider profile. That avoids rewriting OpenSpec tasks, Cleave contracts, and effort policy every time a provider changes.

### Compatibility constraints

There are two compatibility constraints. First, existing tools and specs already use `haiku|sonnet|opus`; changing public tool schemas would cause broad churn across prompts, docs, tests, and OpenSpec baselines. Second, Cleave child dispatch currently depends on CLI fuzzy matching (`pi --model opus`, `pi --model haiku`) and on the implicit default model for sonnet. A provider-aware design should stop relying on provider-specific fuzzy aliases and instead resolve an explicit model ID wherever possible.

### Operator-driven budget awareness

The operator wants Anthropic and Codex/OpenAI to be used largely interchangeably while they empirically learn model strengths under a new subscription. This means provider selection should not be static. Before launching expensive multi-child Cleave runs or other high-burn workflows, the system should ask the operator for current practical budget/limit posture (for example: 'low on Claude today, prefer GPT unless review quality demands otherwise'). Routing should optimize for not hitting subscription/session limits mid-run, rather than assuming one provider is always primary.

### Budget-aware routing implications

Budget awareness suggests a two-layer policy: (1) abstract capability intent (`haiku|sonnet|opus|local`) remains stable for planning, and (2) a mutable execution policy decides which provider should satisfy that intent right now based on operator-provided limit posture. For example, `sonnet`-class execution might resolve to Codex today and Anthropic tomorrow; `opus`-class review might prefer Anthropic when available but degrade to Codex when Claude budget is constrained. This argues for a session-scoped provider preference state, not hard-coded provider ownership of tiers.

### Local fallbacks should be demoted

Local inference remains useful as a resilience and zero-cost fallback, but the operator now has access to effectively free/cheap GPT-backed models for embeddings and other leaf tasks. Therefore local should no longer be the first fallback for many small cloud-eligible tasks. The design should preserve explicit local mode and offline recovery, but default background-task routing should prefer inexpensive cloud options when available and only fall back to local when cloud/provider limits or offline conditions require it.

### Naming model: canonical vs display

The cleanest naming design is dual-layered. Canonical code-level identifiers remain stable and terse for compatibility (`local|haiku|sonnet|opus` in phase 1, potentially superseded later by `servitor|adept|magos|archmagos`). Separately, the UI/help/commands present generic and thematic names: Servitor (local), Adept (fast cloud), Magos (balanced cloud), Archmagos (deep cloud). This lets existing specs/tests survive the first migration while the user experience becomes provider-neutral and more coherent.

## Decisions

### Decision: Keep public tier semantics stable; introduce provider-aware resolution layer

**Status:** exploring
**Rationale:** Do not add Codex-specific tier names to user-facing tools or Cleave plans. Preserve existing abstract tier vocabulary (`haiku|sonnet|opus` plus `local`) for compatibility, but treat those as capability labels rather than provider names. Add a shared model-resolution layer that knows the active cloud provider profile (Anthropic default, Codex/OpenAI optional) and maps each abstract tier to an explicit registry model ID / CLI flag. This contains provider specificity in one place while leaving OpenSpec, design-tree, Cleave planning, and effort tiers stable.

### Decision: Prefer explicit model IDs over fuzzy tier aliases at execution time

**Status:** exploring
**Rationale:** Cleave currently emits `haiku`/`opus` and relies on pi fuzzy model resolution, which is convenient but Claude-shaped. To support Codex cleanly, execution should resolve a concrete model ID first (for example, the best configured model for capability tier `deep`) and pass that full ID to `pi --model`. This reduces ambiguity, avoids provider coupling, and makes behavior easier to test.

### Decision: Model tiers stay abstract; provider choice becomes a session policy

**Status:** decided
**Rationale:** The operator wants Anthropic and Codex/OpenAI used interchangeably while learning their practical strengths and staying within subscription/session limits. Therefore `haiku|sonnet|opus|local` should remain planning-time capability labels, while a session-scoped routing policy chooses the provider that satisfies each tier at execution time. This avoids provider lock-in and lets Cleave adapt to real-time budget posture.

### Decision: Large burns should ask the operator about current provider limits before dispatch

**Status:** decided
**Rationale:** For expensive actions such as large Cleave runs, routing should incorporate the operator's current practical limit posture rather than assuming static availability. A lightweight preflight question can gather whether Claude or GPT should be favored today, reducing the chance of mid-run throttling or subscription exhaustion.

### Decision: Adopt generic capability names with 40K-flavored display labels

**Status:** decided
**Rationale:** Use stable generic capability semantics in architecture while presenting Warhammer-flavored names in operator UX. Proposed mapping: `local` → Servitor, `haiku`-class fast tier → Adept, `sonnet`-class balanced tier → Magos, `opus`-class deep tier → Archmagos. This preserves implementation clarity while giving the system a coherent naming scheme that no longer depends on Anthropic product names.

### Decision: Cleave planning stores canonical abstract tiers, not provider or branded names

**Status:** decided
**Rationale:** To keep task planning stable across providers, `ChildPlan.executeModel`, review settings, and effort config should continue storing canonical abstract tier identifiers (`local|haiku|sonnet|opus` initially, or a future renamed internal enum). Provider choice and operator-facing names are resolved later by the routing and presentation layers. This cleanly answers how executeModel/reviewModel should be expressed across providers.

### Decision: Phase 1 keeps internal tier keys for compatibility; UX adopts Servitor/Adept/Magos/Archmagos labels

**Status:** decided
**Rationale:** Current OpenSpec baselines, tests, and tool schemas are written against `local|haiku|sonnet|opus`. Renaming the internal enum immediately would cause unnecessary churn across effort specs, model-budget tools, and Cleave tests before provider-aware routing lands. Phase 1 should therefore keep canonical internal keys for compatibility while changing operator-facing labels, help text, and status displays to Servitor (local), Adept (haiku-class), Magos (sonnet-class), and Archmagos (opus-class). A later cleanup can supersede the internal keys once routing is centralized and migration cost is lower.

### Decision: Minimal registry is capability-based provider preference plus model matchers

**Status:** decided
**Rationale:** The minimal routing registry does not need a large catalog. It only needs: (1) canonical capability tiers, (2) provider preference order for the session, and (3) per-provider match rules for each tier, such as preferred model IDs or prefixes. The resolver can inspect the existing pi model registry and choose the best available model for a requested capability, avoiding duplicated Anthropic/Codex lookup logic across effort, model-budget, and Cleave.

### Decision: Session budget posture is captured as provider order plus lightweight routing flags

**Status:** decided
**Rationale:** The initial session-scoped budget state should stay simple and operator-driven rather than pretending to know exact quotas. V1 policy should include provider preference order, optional avoid-provider list, a flag preferring cheap cloud over local, a flag requiring preflight for large runs, and optional operator notes. This is enough to route work adaptively across Anthropic, OpenAI, and local without introducing brittle pseudo-accounting.

## Open Questions

*No open questions.*
