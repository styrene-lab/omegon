+++
id = "49535c4f-2188-4f93-bf21-9e88988f3ff8"
kind = "document"
title = "Context Class Taxonomy and Routing Policy — named context classes, route envelopes, downgrade safeguards"
status = "implemented"
tags = ["architecture", "routing", "context-window", "policy", "control-plane", "ux"]
aliases = ["context-class-taxonomy-and-routing-policy"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = ["feature/context-class-taxonomy-and-routing-policy"]
open_questions = []
openspec_change = "context-class-taxonomy-and-routing-policy"
related = ["model-degradation", "effort-tiers", "rust-provider-fallback", "codex-tier-routing", "provider-neutral-model-controls", "perpetual-rolling-context"]
+++

# Context Class Taxonomy and Routing Policy — named context classes, route envelopes, downgrade safeguards

## Overview

Concrete implementation plan for context-aware routing in Pi/Omegon: define named context classes (Compact/Standard/Extended/Massive), routing state schema with active capacity + required minimum floor, downgrade safeguards (compatible/compact/degrading/ineligible classification), model compatibility selection starting from authenticated providers, and Argo-based upstream metadata refresh workflows.

## Research

### Providers expose fixed route ceilings, not operator-selectable context sizes

Upstream providers and local runtimes generally do not let operators choose an arbitrary context size per request. Instead, each concrete route has a fixed offered envelope. Current observed examples: Anthropic API Claude Opus 4.6 = 1,000,000 and Claude Sonnet 4.6 = 1,000,000; OpenAI API GPT-5.4 = 272,000, GPT-5.4 Pro = 1,050,000, GPT-5.4 mini = 400,000; GitHub Copilot Claude 4.6 routes = 128,000, GitHub Copilot GPT-5.4 = 400,000; OpenAI Codex GPT-5.4 = 272,000; local Ollama models span 262,144 to 1,048,576. Routing must operate on a reviewed matrix of concrete provider/model envelopes rather than assume a model family with a user-tunable context slider.

### Some providers have breakpoint zones inside a larger ceiling

Even where a route supports a large ceiling, provider docs expose important internal breakpoints. Anthropic docs indicate Claude 4.6 requests over 200k input tokens now work automatically without beta headers, making 200k a meaningful operational boundary even within a 1M route. OpenAI docs indicate GPT-5.4 long-context routes have a 272k breakpoint above which full-session pricing changes materially. These are not separate selectable context modes, but they are policy-relevant stability/cost zones that the harness should track in its local route envelope snapshot.

### Control-plane requirement: track upstream drift without routing against live claims

Provider context metadata changes over time and sometimes regresses or diverges by transport. Pi/Omegon therefore should not fetch provider claims at request time and trust them blindly for routing. Instead, upstream state should be monitored on a schedule, compared against the checked-in local route matrix, and promoted only through a reviewed snapshot. This keeps operator behavior stable while still tracking a fast-moving provider ecosystem.

## Decisions

### Decision: Four stable context classes: Compact (128k), Standard (272k), Extended (400k), Massive (1m+)

**Status:** decided
**Rationale:** These classes match the real breakpoints currently observed across Anthropic, OpenAI, Copilot, Codex, and local Ollama models. They are easy for operators to reason about and stable enough to survive upstream drift. Exact token counts remain internal metadata; the classes are the operator-facing and policy-facing abstraction.

### Decision: Routing state tracks both active context capacity and required minimum context floor

**Status:** decided
**Rationale:** Safe routing requires distinguishing the capacity of the currently selected model from the minimum capacity the current session can safely tolerate. The state model tracks: activeContextWindow/activeContextClass, requiredMinContextWindow/requiredMinContextClass, optional pinned floor, observed usage/headroom, and downgrade safety arm/override status.

### Decision: Final operator taxonomy: context classes are Compact / Standard / Extended / Massive; thinking levels are Servitor / Functionary / Adept / Magos / Archmagos / Omnissiah

**Status:** decided
**Rationale:** Context names express formation scale and memory span (Iron Hands / Mechanicum blend). Thinking levels express cognitive sophistication (Mechanicum cognition ladder). This keeps context, thinking, and capability tier as three clearly distinct semantic axes.

### Decision: Downgrades classified as compatible, compatible-with-compaction, degrading, or ineligible

**Status:** decided
**Rationale:** The harness compares the current session's required minimum context floor against concrete provider/model envelopes. Each candidate route is classified into one of four categories. This route-envelope classification becomes the basis for all automatic and manual downgrade behavior.

### Decision: Downgrade policy: auto-reroute when compatible, auto-compact in safe bounds, otherwise require operator confirmation

**Status:** decided
**Rationale:** Prefer compatible reroute, then safe compaction, then operator-confirmed degradation. Unsafe context downshifts must never happen silently. Large multi-class drops (e.g. Massive to Compact) always require explicit operator confirmation.

### Decision: Routing selection starts from authenticated providers with opinionated default preference and operator override

**Status:** decided
**Rationale:** Filter to providers the operator is logged into. Within that set, prefer Anthropic by default where all constraints are satisfied. Provider preference is user-configurable routing policy layered on hard feasibility checks, not a hidden override of safety constraints.

### Decision: Dangerous switches use explicit confirmation with durable 'don't ask again' overrides

**Status:** decided
**Rationale:** Confirmation dialog shows current route, target route, context class delta, compaction consequences. Operator may approve once, cancel, or persist override. Persisted overrides remain visible and reversible, never bypass hard feasibility checks.

### Decision: Argo control plane refreshes provider metadata on schedule, emits reviewed route-matrix snapshots

**Status:** decided
**Rationale:** Scheduled automation probes upstream catalogs, normalizes into a candidate route matrix, compares against current reviewed snapshot. Drift opens issues/PRs. Runtime consumes only the last reviewed local snapshot, never raw live upstream data.

### Decision: Refresh pipeline: collect → assess drift → promote reviewed snapshot

**Status:** decided
**Rationale:** Stage 1 collects from authoritative sources. Stage 2 classifies deltas (additive, limit increase/decrease, route removal, breakpoint change, ambiguity). Stage 3 auto-promotes safe changes via PR, blocks risky changes (context decreases, removals) for human review.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `omegon-pi/extensions/lib/context-class.ts` (new) — Context class enum (Compact/Standard/Extended/Massive), token thresholds, classification function
- `omegon-pi/extensions/lib/route-matrix.ts` (new) — Route envelope type, reviewed local matrix, downgrade classification (compatible/compact/degrading/ineligible)
- `omegon-pi/extensions/lib/routing-state.ts` (new) — Routing session state: active capacity, required floor, pinned floor, headroom, override status
- `omegon-pi/extensions/lib/downgrade-policy.ts` (new) — Downgrade evaluation: auto-reroute, safe-compact, operator-confirm logic
- `omegon-pi/extensions/model-router/index.ts` (new) — Model router extension: integrates context class, route matrix, downgrade policy with set_model_tier/set_thinking_level
- `omegon-pi/extensions/lib/provider-preference.ts` (new) — Provider preference config: default order, operator overrides, authenticated filtering
- `omegon-pi/data/route-matrix.json` (new) — Checked-in reviewed route matrix snapshot
- `omegon-pi/extensions/model-router/set_model_tier.ts` (modified) — Enhanced set_model_tier integrating context-aware routing
- `omegon-pi/extensions/model-router/set_thinking_level.ts` (modified) — Enhanced set_thinking_level integrating context-aware routing

### Constraints

- Internal routing compares exact token counts; operators see named classes only
- Reviewed local route matrix stores per-route context ceiling, mapped class, provider/transport identity, and breakpoint zones
- Downgrade evaluation compares against session's required minimum context floor, not current prompt size
- Automatic compaction allowed only when policy judges semantic loss acceptable and no pinned floor is crossed
- Large multi-class drops (e.g. Massive→Compact) always require explicit operator confirmation
- Route selection begins by filtering to authenticated providers/routes
- Anthropic preferred by default when multiple routes satisfy all hard constraints
- Provider preference must be user-visible, dismissible, and reversible
- Persisted overrides never bypass hard feasibility checks
- Runtime routing consumes only last reviewed local snapshot, not live provider responses
- Context decreases and route removals must never auto-promote — require human review
