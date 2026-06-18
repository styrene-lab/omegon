+++
id = "0f78651f-7739-46ca-a8d9-9051b45ff31f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider-neutral model controls and driver persistence

## Disposition — 2026-05-23

**Status: partially implemented concept / stale file scope.** Provider-neutral model control remains relevant, but the implementation paths listed here are from the older TypeScript extension architecture. Current model controls and provider registry behavior are Rust-native (`core/crates/omegon/src/features/model_budget.rs`, `core/crates/omegon/src/model_registry.rs`, TUI model catalog/footer code, and settings/profile handling).

Use this document for constraints and behavior intent only. Do not treat `extensions/model-budget.ts`, `extensions/effort/index.ts`, or `extensions/dashboard/footer.ts` as current implementation paths.

## Overview

Track the implementation that makes model controls provider-neutral in operator-facing UX and persists the last-used concrete driver model across sessions.

## 0.27.0 redesign: provider-agnostic grades and endpoint matrix

The 0.27.0 model-control redesign intentionally removes the legacy tier surface rather than preserving compatibility aliases. The old commands `/gloriana`, `/victory`, `/retribution`, `/opus`, `/sonnet`, and `/haiku` should not remain as hidden dispatch paths. The old `set_model_tier` semantics should be replaced, not wrapped. This is a cognitive model reset: compatibility would keep the stale axis alive and make the new flow harder to learn.

### Canonical axes

Model control is a composable intent envelope, not a concrete model shortcut:

| Axis | Meaning | Example command |
|---|---|---|
| Capability grade | Provider-neutral requested model strength | `/model grade S` |
| Provider endpoint selection | Which endpoint(s) may satisfy the grade | `/model provider auto`, `/model provider anthropic`, `/model provider local` |
| Failover/degradation policy | What may happen when the requested route is unavailable | `/model policy strict-grade` |
| Exact model override | Pin one concrete route and bypass grade resolution | `/model openai-codex:gpt-5.4` |
| Thinking level | Reasoning budget, separate from model grade | `/think medium` |
| Context class | Prompt/context budget envelope | `/context standard` |

Capability grades use the F/D/C/B/A/S vocabulary. `local` is not a grade; it is an endpoint class / operational posture. Local development endpoints are loose, best-effort dependencies. Upstream endpoints include public SaaS providers, private gateways, privately hosted Ollama/vLLM, and other non-local endpoints that participate in health, auth, quota, failover, and routing policy.

### Endpoint/protocol model

Provider identity, endpoint class, protocol kind, auth scheme, and model capabilities are separate concerns:

```rust
pub enum ModelGrade { F, D, C, B, A, S }

pub enum EndpointClass {
    LocalDev,
    Upstream,
}

pub enum EndpointProtocol {
    OpenAiCompatible,
    Anthropic,
    GeminiNative,
    OllamaNative,
}

pub struct ProviderEndpoint {
    pub id: String,
    pub display_name: String,
    pub class: EndpointClass,
    pub protocol: EndpointProtocol,
    pub base_url: Option<String>,
    pub credential_ref: Option<String>,
    pub enabled: bool,
}
```

Most upstream providers should enter through `EndpointProtocol::OpenAiCompatible` with endpoint-profile metadata rather than bespoke clients. Anthropic remains a specialized adapter because its native Messages API, streaming, tool-use, thinking, and auth behavior are materially different. Gemini may be dual-path: OpenAI-compatible for baseline chat/tool use and native Gemini for feature-complete behavior.

### OpenAI-compatible profile

```rust
pub struct OpenAiCompatibleProfile {
    pub supports_chat_completions: bool,
    pub supports_responses_api: bool,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub unsupported_request_fields: BTreeSet<String>,
    pub required_headers: BTreeMap<String, String>,
    pub optional_headers: BTreeMap<String, String>,
    pub quirks: Vec<EndpointQuirk>,
}
```

The shared adapter must sanitize requests according to the profile. “OpenAI-compatible” does not mean every OpenAI field is accepted by every endpoint.

### Researched upstream matrix

| Endpoint family | Protocol abstraction | Notes / quirks | Confidence |
|---|---|---|---|
| OpenRouter | OpenAI-compatible upstream | `/api/v1/chat/completions`; optional attribution headers `HTTP-Referer`, `X-OpenRouter-Title`, `X-Title`; normalizes across providers/models. | High |
| Groq | OpenAI-compatible upstream | `https://api.groq.com/openai/v1`; supports chat completions, streaming, tools; reject/sanitize unsupported fields including `logprobs`, `logit_bias`, `top_logprobs`, `messages[].name`; `n` must be 1; `temperature=0` becomes `1e-8`. | High |
| Mistral | OpenAI-compatible-ish upstream | `POST /v1/chat/completions`; messages/tools/parallel tool calls/streaming SSE. Use shared adapter with profile unless implementation finds incompatible shapes. | High for chat-completions shape |
| xAI | OpenAI-compatible upstream | `https://api.x.ai/v1`; supports chat completions, Responses API examples, tools, streaming. | High |
| Hugging Face router | OpenAI-compatible router | `https://router.huggingface.co/v1`; model IDs encode underlying provider; supports tools, grammars/constraints, streaming. | High |
| Google Gemini | OpenAI-compatible plus native | OpenAI compatibility exists; native Gemini path may still be required for full feature coverage. | High for compatibility endpoint existing |
| Ollama local | LocalDev endpoint; Ollama-native and/or OpenAI-compatible | Local development posture, not a capability grade. Exact endpoint behavior should be verified against the installed Ollama version during implementation. | Medium-high |
| Remote/private Ollama/vLLM | Upstream endpoint | Treat as upstream even when protocol is Ollama-native or OpenAI-compatible. | High as design rule |
| Anthropic | Custom Anthropic adapter | Native `/v1/messages`, version header, custom tool/streaming/thinking semantics. | High |
| Cerebras | Likely OpenAI-compatible upstream | Evidence through router integrations is strong, but direct official docs should be confirmed before hard-coding. | Medium |

### Resolver contract

The route resolver operates over endpoint/model capability rows:

```rust
pub struct ModelCapabilityRow {
    pub endpoint_id: String,
    pub model_id: String,
    pub grade: ModelGrade,
    pub grade_source: GradeSource,
    pub context_window: Option<u32>,
    pub supports_tools: bool,
    pub supports_streaming: bool,
    pub supports_json_mode: bool,
    pub supports_vision: bool,
    pub cost_band: Option<CostBand>,
    pub latency_band: Option<LatencyBand>,
}
```

Resolution filters by requested grade, provider/endpoint selection, endpoint health, credentials/quota, required features, and failover/degradation policy. The operator intent must remain durable even when the concrete serving route changes.

### Command and tool target

Canonical command path:

```text
/model
/model list
/model grade <F|D|C|B|A|S>
/model provider <auto|local|upstream|endpoint-id>
/model policy <strict-grade|nearest-grade|cost-aware|latency-aware|local-first|upstream-first|pinned>
/model route
/model providers
/model <provider:model>
```

Agent-facing tools should align with the new ontology. `set_model_intent` is the durable target and should update the whole model intent atomically. Avoid separate agent tools for grade/provider/policy unless they are thin command-path helpers over the same atomic reducer. Do not keep `set_model_tier` as a transitional alias.

### Policy semantics to decide before implementation

The current command sketch names `strict-grade`, but implementation must avoid ambiguous semantics. The implementation plan should split three concerns:

```rust
pub enum GradePolicy {
    Exact,                         // requested grade only
    Minimum,                       // requested grade or stronger
    NearestAllowed { max_downgrade_steps: u8 },
}

pub enum FailoverPolicy {
    SameGradeOtherEndpoint,
    AnyPolicyCompliantEndpoint,
}

pub enum DegradationPolicy {
    None,
    OneStep,
    BestEffort,
    Ask,
}
```

Exact concrete model switches create a pinned override. The command surface must include an escape hatch such as `/model unpin` or `/model intent clear-exact`; otherwise `/model grade ...` can appear ignored while the exact pin still wins.

### Endpoint profile completeness

OpenAI-compatible endpoint profiles must cover request shaping, response normalization, and error normalization:

```rust
pub struct EndpointProfile {
    pub request: RequestProfile,
    pub response: ResponseProfile,
    pub error: ErrorProfile,
    pub metadata: EndpointProfileMetadata,
}
```

Profiles should include docs URL, verification date, and confidence so provider API drift is inspectable during future maintenance.

### Auth scheme model

`credential_ref` alone is insufficient. Endpoint definitions need an auth scheme so new upstream providers do not reintroduce provider-specific auth branches:

```rust
pub enum AuthScheme {
    None,
    BearerToken { secret_ref: String },
    ApiKeyHeader { header: String, secret_ref: String },
    OAuthProvider { provider: String },
    Custom { kind: String, secret_ref: Option<String> },
}
```

### Provider selector namespace

`auto`, `local`, and `upstream` are reserved provider-selector tokens. Endpoint IDs must not use those names. `local` selects `EndpointClass::LocalDev`; `upstream` selects `EndpointClass::Upstream`; `auto` selects all enabled endpoints allowed by the operator profile and policy.

## Open Questions

- What is the default grade for new sessions? Candidate: `B` as daily-driver capable.
- What is the default provider selection? Candidate: `auto`.
- What is the default grade/failover/degradation policy for interactive sessions versus daemon agents?
- Should `strict-grade` be replaced in the operator-facing vocabulary with explicit `exact`, `minimum`, and `nearest` grade policy terms?
- What command clears a pinned exact model override: `/model unpin`, `/model mode auto`, or `/model intent clear-exact`?
- Does `/model provider local` select all local-dev endpoints or only the default local-dev endpoint? Candidate: all enabled `EndpointClass::LocalDev` endpoints.
- Should local-dev endpoints participate in `/model provider auto` by default, or only when the operator explicitly chooses local/local-first?
- How are grade assignments reviewed, and when is `GradeSource::Heuristic` allowed to satisfy high-grade requests?
- Does the bundled registry hard-break from `tiers` to capability rows, or is there an internal one-release data migration with no user-facing compatibility?
- How will baseline OpenSpec requirements that still mention `/local`, `/haiku`, `/sonnet`, `/opus`, and `set_model_tier` be modified or removed for 0.27.0?

## Implementation Notes

### File Scope

- `core/crates/omegon/src/features/model_budget.rs` (modified) — replace `ModelTier` and legacy commands/tooling with model-intent vocabulary.
- `core/crates/omegon/src/model_registry.rs` (modified) — migrate from provider-tier maps to endpoint/model capability rows.
- `core/crates/omegon/src/route.rs` (modified) — preserve model intent separately from active route; expose structured resolution/failover reasons.
- `core/crates/omegon/src/routing.rs` (modified) — resolve grade/provider/policy intent over endpoint capability rows.
- `core/crates/omegon/src/command_registry.rs` (modified) — advertise canonical `/model grade|provider|policy|route|providers|unpin` subcommands.
- `core/crates/omegon/src/tui/mod.rs` (modified) — parse new `/model` grammar before exact model fallback; remove legacy tier dispatch.
- `core/crates/omegon/src/tui/model_catalog.rs` (modified) — project endpoint/model capabilities and provider-selector filters.
- `core/crates/omegon/src/tui/footer.rs` and `core/crates/omegon/src/tui/segments.rs` (modified) — display intent and active route, including pinned/degraded/failover states.
- `core/crates/omegon/src/ipc/snapshot.rs` and `core/crates/omegon/src/surfaces/` (modified) — expose renderer-neutral model intent/route projections.
- `data/model-registry.json` (modified) — endpoint definitions, profiles, and model capability rows.
- `pkl/` schemas (modified) — validate endpoint IDs, reserved provider-selector tokens, auth schemes, and capability row shape.
- `openspec/baseline/routing.md` and `openspec/baseline/effort.md` (modified by the 0.27.0 OpenSpec delta) — remove or rewrite legacy tier requirements.

### Constraints

- Persist only successful explicit model switches; failed switch attempts must not overwrite a working saved model.
- Store operator intent separately from active route so failover can preserve requested grade/provider/policy while changing the serving endpoint.
- Exact model overrides are pinned and must be visibly represented until explicitly cleared.
- Request, response, and error normalization for OpenAI-compatible endpoints must be profile-driven.
- Route explanations must include rejected candidates and structured reasons for debugging and autonomous recovery.
