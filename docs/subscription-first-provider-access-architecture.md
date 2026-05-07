+++
id = "609a5f56-1901-4da7-b513-092872bb028f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Subscription-first provider access architecture

## Overview

Model access should be designed around the operator's existing entitlements and auth mechanisms, not just provider-native API key paths. The architecture must distinguish model families, commercial entitlements (subscriptions, seats, plans), concrete execution backends, and automation/legal boundaries so Omegon can prefer the operator's cheapest already-paid route while still reporting the exact backend honestly.

## Research

### Current architecture is provider-centric, not entitlement-centric

Current provider architecture is provider-centric. `core/crates/omegon/src/auth.rs` models providers as canonical credential descriptors with provider id, auth.json key, env vars, auth method, and display name. `core/crates/omegon/src/providers.rs` resolves concrete runtime bridges by provider id. This works well for API-key-first integrations, but it conflates brand/provider identity with execution path. The repo already contains evidence that these must be separated: `openai` vs `openai-codex` are distinct because the operator-facing model family overlaps but the backend, auth artifact, and endpoint differ. The Anthropic subscription/OAuth dispute also showed that auth mechanism and automation allowance are route properties, not just provider properties.

### Operator goal: route through already-paid entitlements first

Operator requirement articulated in-session: the happy path should be using auth mechanisms the operator already has in place, especially subscriptions that are more cost-effective than per-token billing. The important matrix is not just which provider offers which models, but how Omegon can legitimately reach those model families using existing operator entitlements. API keys are mechanically straightforward and not the interesting design dimension; the design challenge is representing and ranking subscription-backed access routes without lying about the concrete backend or policy boundary.

### Initial external evidence: Ollama Cloud is a backend; Google AI Pro is an entitlement layer

External research so far:
- Ollama Cloud exposes a real programmable hosted backend with bearer-token auth, host `https://ollama.com/v1`, and OpenAI-compatible calling semantics. This should be modeled separately from `ollama-local` because the entitlement, quota surface, and execution backend are different.
- Gemini developer documentation points to API-key-based integration (`GEMINI_API_KEY` / `GOOGLE_API_KEY`) with project billing and paid tiers. Google AI Pro / Google One pages emphasize app/product access, not a sanctioned general API entitlement. Treat `google-ai-pro` as an operator-facing entitlement concept, not a supported execution route.
- ChatGPT/Codex consumer OAuth currently looks like an unsupported or operator-owned-risk consumer route rather than a sanctioned happy path. It should not be presented as equivalent to OpenAI API billing.
- Claude Pro/Max OAuth is materially different: the Claude Code CLI/OAuth path is sanctioned enough to be usable, but fair-use throttling and interactive-use caveats mean it belongs in a supported-with-caveats bucket, not the default automation-safe tier.
These findings support a design where entitlements and execution routes are represented separately and linked explicitly.

## Decisions

### Separate entitlement identity from execution backend identity

**Status:** proposed

**Rationale:** The operator buys access through plans, seats, subscriptions, and API billing arrangements, but Omegon executes against concrete technical backends. These identities overlap but are not the same object. Treating them as the same causes honesty bugs and policy confusion.

### Model routing should optimize for operator-preferred existing entitlements before pay-as-you-go API routes

**Status:** proposed

**Rationale:** For many operators, the cost-effective and operationally preferred route is the entitlement they already pay for. API-key support remains necessary, but should not be assumed to be the primary happy path for every model family.

### Automation policy belongs to execution routes, not abstract model families

**Status:** proposed

**Rationale:** The same model family may be reachable through multiple routes with different automation allowances, telemetry, quotas, and terms. Policy and capability must therefore be attached to the concrete route that is used, not to the model family label alone.

## Open Questions

- What are the canonical entities in the routing model: model family, entitlement, auth mechanism, execution backend, or some other split?
- How should Omegon rank routes when multiple paths can reach the same model family: existing subscription entitlement, API key, local runtime, hosted third-party compatibility layer, or explicit operator override?
- How do we represent the distinction between operator-facing entitlement names (Google AI Pro, ChatGPT Plus/Pro, Claude Pro/Max) and concrete runtime backends (Gemini API, Codex backend, Anthropic API, app-backed OAuth endpoint) without lying in the UI?
- [assumption] Most operators prefer the cheapest already-paid access path for a model family over setting up new per-token billing, provided the route is officially supported enough for regular use.
- How should automation policy be attached to routes: by entitlement, by auth mechanism, by backend, or by an explicit capability profile on each execution route?
- What minimum evidence threshold is required before Omegon treats a subscription-backed route as a supported happy path rather than an experimental/operator-owned integration?
