---
id: provider-route-conceptual-model-matrix
title: "Provider Route / Conceptual Model Matrix — first-class semantic model identity across providers"
status: decided
tags: [architecture, routing, providers, models, github-copilot, inference, evidence]
open_questions: []
dependencies: []
related: []
---

# Provider Route / Conceptual Model Matrix — first-class semantic model identity across providers

## Overview

Split model routing into conceptual model identity and concrete provider route identity so routes like github-copilot:claude-sonnet-4-6 and anthropic:claude-sonnet-4-6 are operationally distinct routes serving the same conceptual model. Capability, model class, UX grouping, degradation, and benchmark evidence operate on conceptual model identity when identity equivalence is established; auth, transport, cost, quota, availability, diagnostics, deployment-specific capability verification, and execution operate on provider route identity. Unknown or internal deployments remain valid route identities without requiring a conceptual-model mapping or quality grade.

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

### Runtime inference estate is data, not compiled product logic

**Status:** decided

**Rationale:** Omegon releases define supported inference interfaces, protocol adapters, authentication semantics, response normalization, and routing policy. Runtime configuration and discovery define currently deployed endpoints, offerings, model IDs, aliases, declared capabilities, and credential references. The embedded registry is a bootstrap layer, not the sole source of truth; adding or replacing a deployment that uses an understood adapter must not require recompilation.

### Inventory records endpoint deployments and offerings, not provider-wide capability

**Status:** decided

**Rationale:** A provider or endpoint fabric such as Nutanix Enterprise AI may expose multiple deployments with different models, modalities, tool behavior, context envelopes, health, and credentials. Inventory therefore separates provider/administrative integration, callable endpoint deployment, and model offering. Provider-wide grades are compatibility projections only and cannot establish offering-level fitness.

### Modality and inference-interface compatibility precede grade comparison

**Status:** decided

**Rationale:** Grades are quality summaries within capability domains, not universal types. Routing first filters by understood inference interface, input/output modalities, protocol behavior, context and payload constraints, and policy. It then applies capability-specific grade floors and confidence requirements. Image, video, embedding, reranking, and conversational offerings cannot substitute for one another merely because they share an overall display tier.

### Capability evidence and quality grade remain separate

**Status:** decided

**Rationale:** An endpoint can be verified to accept image input or emit tool calls while its quality remains unknown. Capability values carry provenance and verification state (embedded, configured, discovered, probed, benchmarked, or runtime-observed). Public benchmark observations are immutable inputs to a versioned synthesis policy; daily collection may publish a candidate signed matrix but cannot directly rewrite canonical grades or routing policy.

### Ungraded offerings are usable under explicit or policy-bounded selection

**Status:** decided

**Rationale:** Internal and R&D deployments must be callable immediately when their protocol and modality contract is understood, even without benchmark evidence. Ungraded means quality unknown, not unsupported. Explicit selection is allowed; autonomous selection requires an organization/session policy that admits ungraded offerings within a bounded authority envelope. No route may silently infer an average grade.

### Runtime inventory is layered, provenance-preserving, and atomically refreshable

**Status:** decided

**Rationale:** Resolution composes an embedded bootstrap registry, signed/cached evidence snapshots, organization and user endpoint configuration, project/session overlays, discovery, and active probes. Fields merge with provenance rather than whole records replacing one another. Refresh validates a complete candidate snapshot and atomically activates it; failures retain the last known-good generation. Long-running operations record the generation and may fail over only within their authorized routing envelope.

### Novel model-service semantics stay behind explicit integration boundaries

**Status:** decided

**Rationale:** Runtime configuration may compose adapters and inference interfaces already understood by Omegon, but cannot define arbitrary executable parsing, preprocessing, transports, or domain-specific prediction semantics. Novel protocols or general inference families require an Omegon adapter/release. Application-specific ML services belong behind OpenAPI tools, Omegon Extensions, or MCP servers.

### Overall tier is visualization, not an operational guarantee

**Status:** decided

**Rationale:** The matrix may synthesize an overall profile-specific display tier such as Agentic A from capability sub-grades, including models whose individual dimensions range above or below A. Route eligibility never uses that average to compensate for a missing hard requirement: every required modality, interface, capability floor, and confidence floor must pass independently.

## Resolved Assumptions

- Conceptual IDs are maintained only where exact or explicitly reviewed equivalence is supportable; internal, quantized, fine-tuned, or ambiguous offerings may remain route-only identities.
- Brokered routes share benchmark evidence with a conceptual model only when the mapping is explicit. Route-specific envelopes, safety behavior, tool behavior, and telemetry remain independent and can lower route fitness without mutating the conceptual prior.
- Execution class is an extensible data value grouped into policy-relevant trust/locality attributes, not a permanently closed enum limited to the initial five labels.
