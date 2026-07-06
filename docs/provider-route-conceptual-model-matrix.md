---
id: provider-route-conceptual-model-matrix
title: "Provider Route / Conceptual Model Matrix — first-class semantic model identity across providers"
status: exploring
tags: [architecture, routing, providers, models, github-copilot]
open_questions:
  - "[assumption] Stable conceptual IDs such as claude-sonnet-4.6, gpt-5.5, and gemini-2.5-pro can be maintained in the reviewed model registry."
  - "[assumption] Copilot-hosted variants are close enough to direct-provider variants to share conceptual model identity even when route envelopes, safety filters, context limits, or tool behavior differ."
  - "[assumption] executionClass values local, remote-local-network, subscription-cloud, api-cloud, broker-cloud are sufficient for initial policy and UX grouping."
dependencies: []
related: []
---

# Provider Route / Conceptual Model Matrix — first-class semantic model identity across providers

## Overview

Split model routing into conceptual model identity and concrete provider route identity so routes like github-copilot:claude-sonnet-4-6 and anthropic:claude-sonnet-4-6 are operationally distinct routes serving the same conceptual model. Capability, model class, UX grouping, degradation, and benchmarking operate on conceptual model identity; auth, transport, cost, quota, availability, diagnostics, and execution operate on provider route identity.

## Research

### Second- and third-order effects

Provider route metadata must own context envelope, output limit, transport dialect, auth, quota, cost, deprecation, and execution failure state. Conceptual model metadata owns semantic class, aliases, capability class, and UX grouping. Benchmarks, telemetry, session replay, memory, provider policy, and diagnostics must carry both identities to avoid misattributing route failures to model classes or silently changing commercial/auth surfaces.

## Decisions

### Decision: conceptual model identity and provider route identity are distinct but linked

**Status:** decided

**Rationale:** The same model class can be served by multiple commercial/auth/transport surfaces. Collapsing routes loses auth, billing, transport, quota, and diagnostics truth; treating routes as unrelated fragments capability routing, degradation, benchmarking, and UX grouping. The route matrix therefore links concrete provider routes to stable conceptual model IDs.

### Add producer and execution class as separate axes without constraining local/offline routes

**Status:** decided

**Rationale:** OpenAI/Anthropic/Google can be both model producers and serving providers, while OpenRouter/GitHub Copilot broker producer models and Ollama/DwarfStar serve local/offline model artifacts. The matrix needs producer for attribution/policy/benchmarking and executionClass for privacy/trust posture, but producer must remain optional so dynamic local/offline routes with unknown lineage continue to work.

## Open Questions

- [assumption] Stable conceptual IDs such as claude-sonnet-4.6, gpt-5.5, and gemini-2.5-pro can be maintained in the reviewed model registry.
- [assumption] Copilot-hosted variants are close enough to direct-provider variants to share conceptual model identity even when route envelopes, safety filters, context limits, or tool behavior differ.
- [assumption] executionClass values local, remote-local-network, subscription-cloud, api-cloud, broker-cloud are sufficient for initial policy and UX grouping.
