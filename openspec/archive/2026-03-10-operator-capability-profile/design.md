+++
id = "86e44ac5-3e03-4808-9a6c-e1240fb3e96d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Operator capability profile — Design

## Architecture Decisions

### Decision: Profile should gate fallbacks, not just advertise capabilities

**Status:** decided  
**Rationale:** The operator's actual problem is unsafe automatic behavior, not lack of detection. The profile must be consulted before fallback routing so pi-kit can refuse or require confirmation for materially worse local paths even when they are technically available.

### Decision: Schema maps semantic roles to ordered concrete candidates

**Status:** decided  
**Rationale:** The operator configures intent at the role level (`archmagos`, `magos`, `adept`, `servitor`, `servoskull`) rather than binding every feature directly to a model string. Ordered candidates preserve provider-neutral routing while making preference order explicit.

### Decision: Provider/source and role/tier are separate axes

**Status:** decided  
**Rationale:** Role names express capability intent; candidate metadata expresses where execution happens. The schema must not overload old terms like `frontier` to mean all cloud models.

### Decision: The full public ladder is archmagos → magos → adept → servitor → servoskull

**Status:** decided  
**Rationale:** All supported capability roles remain operator-visible. Lower tiers represent reduced capability and reasoning depth, not merely a different provider location.

### Decision: Candidate objects carry explicit thinking ceilings

**Status:** decided  
**Rationale:** Thinking depth is part of tier semantics and cannot be derived safely from role alone. Per-candidate `maxThinking` preserves overlap between adjacent tiers while guaranteeing that `servoskull` remains thinking-off.

### Decision: Default behavior must be safe without setup

**Status:** decided  
**Rationale:** If setup is skipped, pi-kit still synthesizes a conservative profile: prefer upstream candidates, allow same-role cross-provider fallback, and block or require confirmation for silent upstream-to-heavy-local fallback.

### Decision: Transient upstream failures enter runtime cooldown state

**Status:** decided  
**Rationale:** Mid-run failures like Anthropic 429s and OpenAI session-limit exhaustion are temporary capability loss. They should cool down the failing provider/candidate and feed the next resolution attempt through the normal fallback policy.

### Decision: Runtime cooldowns use a fixed 5-minute window in v1

**Status:** decided  
**Rationale:** A fixed cooldown is simple, predictable, and easy to explain. Adaptive backoff can come later if operational evidence justifies more complexity.

## Research Context

### Problem being solved

Bootstrap can detect dependencies and auth state, and existing routing helpers can select concrete models, but pi-kit lacked a durable operator profile that answers:
- which upstream providers are available and preferred,
- which local inference paths are acceptable on this machine,
- and what fallback boundaries preserve UX instead of merely maximizing availability.

### Why this matters

A technically available local model is not always an acceptable fallback. Heavy local models can degrade responsiveness badly enough that an explicit denial or confirmation prompt is preferable to silent failover.

### Chosen profile shape

The implementation splits durable operator intent from volatile runtime state:
- `.pi/config.json` stores `operatorProfile.roles` and `operatorProfile.fallback`
- `.pi/runtime/operator-profile.json` stores transient cooldowns and related runtime availability state

Each role resolves through ordered candidate objects:

```json
{
  "id": "claude-sonnet-4-6",
  "provider": "anthropic",
  "source": "upstream",
  "weight": "normal",
  "maxThinking": "medium"
}
```

### Resolution model

Resolver flow:
1. map internal aliases like `planning`, `review`, `compaction`, `extraction`, and `cleave.leaf` onto public roles,
2. load the ordered candidates for that role,
3. filter out unavailable or cooled-down candidates,
4. choose the first viable candidate,
5. if the next viable candidate crosses provider or source boundaries, consult fallback policy,
6. return a structured `allow`, `ask`, or `deny` outcome instead of inventing new candidates.

### Fallback policy model

V1 uses `allow | ask | deny` for:
- same-role cross-provider fallback
- cross-source fallback
- heavy-local fallback
- unknown-local-performance fallback

The critical guardrail is that upstream-to-local transitions are not silent by default when they materially change latency or quality.

## Current implementation state

Implemented in this change:
- canonical operator profile schema and defaults in `extensions/lib/operator-profile.ts`
- bridge from persisted config/runtime state into resolver-native profile/runtime structures
- bootstrap/setup integration that synthesizes safe defaults and reorders candidates from provider readiness/preferences
- effort/model-budget integration so driver and extraction resolution use the saved profile and runtime cooldown state
- transient failure handling that records 5-minute provider/candidate cooldowns for upstream failures
- operator-facing guidance for blocked or confirmation-required fallback paths

## File Scope

- `extensions/lib/operator-profile.ts` — canonical schema, defaults, parsing, persistence, runtime-state bridging
- `extensions/lib/operator-profile.test.ts` — parsing, round-trip, and legacy-normalization coverage
- `extensions/bootstrap/index.ts` — setup capture, default synthesis, routing policy derivation
- `extensions/bootstrap/index.test.ts` — bootstrap/profile setup coverage
- `extensions/effort/index.ts` — startup driver/extraction resolution through the profile/runtime bridge
- `extensions/model-budget.ts` — tier switching through the resolver plus turn-end transient failure handling
- `extensions/lib/operator-fallback.ts` — cooldown persistence and operator-facing fallback explanations
- `extensions/lib/operator-fallback.test.ts` — retry, expiry, and blocked heavy-local fallback coverage
- `extensions/startup-order.test.ts` — startup ordering protection so local models register before effort resolves tiers

## Constraints

- Startup routing must use the saved operator profile before effort session startup applies model selection.
- Failed tier resolution should not emit misleading warnings if pi already has a usable current model.
- Persisted runtime cooldowns may contain legacy candidate keys like `provider:model`; bridge logic must normalize them.
- Guided setup remains qualitative only: it reorders candidates and configures fallback posture without benchmarking local hardware.
- Local-model failures should not enter the upstream provider cooldown path.
- Tier-switch failures must surface resolver policy explanations when fallback is blocked or confirmation-gated.
