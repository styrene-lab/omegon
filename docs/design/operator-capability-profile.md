+++
id = "57851038-af1c-4e2e-b177-c7c0600f4f29"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Operator capability profile — provider login + local hardware assessment

## Disposition — 2026-05-23

**Status: current concept / stale implementation scope.** The separation between durable operator routing intent, volatile runtime availability, and guarded fallback policy remains important. The listed implementation files are stale: the TypeScript `extensions/lib/operator-profile.ts`, `extensions/bootstrap/*`, `extensions/effort/*`, and `extensions/model-budget.ts` paths are absent in the current Rust-native repository.

Use this document as the conceptual model for future Rust implementation or reconciliation. Verify current behavior against `.omegon/profile.json`, Rust settings/auth/model registry code, `core/crates/omegon/src/features/model_budget.rs`, and `core/crates/omegon/src/upstream_errors.rs` before relying on any schema or file-scope details.

## Overview

Add a durable operator capability profile so Omegon can choose models according to operator intent instead of falling back only on raw technical availability. The profile captures preferred candidates for public capability roles and the policy boundaries that determine whether cross-provider or cross-source fallback is allowed, confirmation-gated, or denied.

## Research

### Assessment summary

This work separates two concerns that had previously been conflated:
1. environment capability discovery,
2. operator routing preferences.

Bootstrap already knows how to discover dependencies and provider readiness. What was missing was a durable answer to questions like:
- which upstream providers are preferred,
- which local models are acceptable on this machine,
- and when a fallback should be blocked instead of silently accepted.

### Why this is worth doing now

Recent behavior showed that “best available local model” is not the same as “acceptable fallback.” A heavy local model that takes minutes to respond can degrade the session more than an explicit failure or confirmation prompt. The profile layer exists to prevent that class of silent regression.

### Chosen profile shape

The implementation splits durable intent from volatile runtime state:
- `.omegon/profile.json` stores the durable `operatorProfile`
- runtime state is tracked in Omegon-owned transient status storage rather than `.pi/runtime/`

Durable profile data is structured as:
- `roles`: ordered candidates for each public capability role
- `fallback`: policy for same-role cross-provider, cross-source, heavy-local, and unknown-local-performance transitions

Each candidate has this v1 shape:

```json
{
  "id": "claude-sonnet-4-6",
  "provider": "anthropic",
  "source": "upstream",
  "weight": "normal",
  "maxThinking": "medium"
}
```

### Public role ladder

The public capability ladder is:
- `archmagos`
- `magos`
- `adept`
- `servitor`
- `servoskull`

These are capability roles, not provider names. Provider/source remains a separate axis.

### Role semantics

- `archmagos` — highest capability and deepest reasoning
- `magos` — strong workhorse tier for routine high-quality work
- `adept` — common bounded coding and simpler execution
- `servitor` — fast shallow execution and utility work
- `servoskull` — minimum acceptable tier with thinking forced off

Adjacent tiers may overlap in raw model quality. Tier membership expresses intended operating envelope and default reasoning depth, not a hard capability partition.

### Resolver algorithm sketch

Resolution works as follows:
1. map internal aliases like `planning`, `review`, `compaction`, `extraction`, and `cleave.leaf` onto public roles,
2. load ordered candidates for the target role,
3. filter out unavailable or cooled-down candidates,
4. choose the first viable candidate,
5. if the next viable move crosses provider or source boundaries, consult fallback policy,
6. return a structured allow/ask/deny outcome instead of inventing a new candidate.

### Fallback policy model

V1 uses `allow | ask | deny` for:
- same-role cross-provider fallback
- cross-source fallback
- heavy-local fallback
- unknown-local-performance fallback

The important boundary is upstream-to-local transitions that materially change UX. Those should not be silent by default.

### Default behavior without setup

If setup is skipped, Omegon synthesizes a safe default profile instead of leaving routing undefined. The default posture is:
- prefer upstream candidates,
- allow same-role cross-provider fallback,
- require confirmation or deny silent upstream-to-heavy-local fallback.

### Dynamic upstream failure handling

Transient upstream failures such as Anthropic 429s and OpenAI session-limit exhaustion are treated as temporary capability loss. Omegon records a fixed 5-minute runtime cooldown for the failed upstream provider/candidate and re-runs resolution through the same policy boundaries.

That means:
- same-role cross-provider retry can happen automatically when policy allows it,
- upstream-to-local fallback still consults operator policy,
- blocked or confirmation-required transitions produce an operator-facing explanation.

### Stale assumptions superseded

Earlier exploration used overloaded terminology like `frontier.*` and `local.*` as if provider/source and capability role were the same axis. That is no longer the model.

Current guidance is:
- roles describe capability intent,
- provider/source describes where the candidate executes,
- fallback policy governs when transitions across those boundaries are acceptable.

## Decisions

### Decision: Profile should gate fallbacks, not just advertise capabilities

**Status:** decided
**Rationale:** The profile must participate in routing decisions so Omegon can refuse or require confirmation for harmful fallbacks instead of merely listing what is technically available.

### Decision: Schema should map semantic roles to ordered concrete candidates

**Status:** decided
**Rationale:** Ordered per-role candidates keep operator intent stable while allowing provider-aware routing underneath.

### Decision: Provider/source and role/tier must be separate axes

**Status:** decided
**Rationale:** Capability role names are not provider names. This avoids conflating cloud location with model quality.

### Decision: All capability roles are public and operator-visible

**Status:** decided
**Rationale:** The full ladder remains configurable and visible to the operator; internal aliases resolve through those public roles rather than introducing hidden tiers.

### Decision: Candidate objects must encode explicit thinking ceilings

**Status:** decided
**Rationale:** Thinking depth is part of the tier contract and must be preserved per candidate.

### Decision: Default profile must be safe without setup

**Status:** decided
**Rationale:** Skipping setup must still yield conservative, predictable routing behavior.

### Decision: Transient upstream availability failures should enter the fallback policy path

**Status:** decided
**Rationale:** Runtime failures are part of real availability and should affect subsequent resolution, but only through the same guarded policy model.

### Decision: Use a fixed 5-minute cooldown for transient provider failures in v1

**Status:** decided
**Rationale:** A fixed cooldown keeps the first implementation easy to reason about and explain.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/lib/operator-profile.ts` — canonical profile schema, defaults, parsing, persistence, runtime-state bridging
- `extensions/lib/operator-profile.test.ts` — parsing, round-trip, and legacy-normalization coverage
- `extensions/bootstrap/index.ts` — setup capture, safe default synthesis, routing policy derivation
- `extensions/bootstrap/index.test.ts` — bootstrap/operator-profile setup coverage
- `extensions/effort/index.ts` — startup driver and extraction resolution through profile/runtime state
- `extensions/model-budget.ts` — tier switching and transient failure handling through the resolver
- `extensions/lib/operator-fallback.ts` — provider/candidate cooldown persistence and fallback explanations
- `extensions/lib/operator-fallback.test.ts` — retry, expiry, and blocked heavy-local fallback coverage
- `extensions/startup-order.test.ts` — protects startup ordering so local models register before effort resolves tiers
- `extensions/lib/model-routing.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/lib/model-routing.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/effort/tiers.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/effort/tiers.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/operator-capability-profile.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Startup routing must apply the saved operator profile before effort session startup resolves the driver.
- Runtime cooldown bridging must normalize legacy persisted keys like `provider:model`.
- Guided setup remains qualitative only; it does not benchmark hardware.
- Local-model failures do not enter the upstream provider cooldown path.
- Tier-switch failures should surface resolver policy explanations when fallback is blocked or requires confirmation.
- Silent upstream-to-heavy-local fallback is not allowed by default; operator policy must govern cross-source and heavy-local transitions.
