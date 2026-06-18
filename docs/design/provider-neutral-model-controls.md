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

Agent-facing tools should align with the new ontology. Prefer `set_model_intent` as the durable target; if incremental tools are needed, use `set_model_grade`, `set_model_provider`, and `set_model_policy`. Do not keep `set_model_tier` as a transitional alias.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/model-budget.ts` (modified) — Provider-aware tier descriptions and concrete provider/model notifications
- `extensions/effort/index.ts` (modified) — Restore persisted driver model on startup and report resolved provider/model
- `extensions/lib/model-preferences.ts` (new) — Persist and load last-used concrete driver model from `.omegon/profile.json`
- `extensions/dashboard/footer.ts` (modified) — Compact footer cleanup to a single dashboard-first line with inline model visibility
- `extensions/model-budget.test.ts` (new) — Coverage for provider-aware model control copy
- `extensions/lib/model-preferences.test.ts` (new) — Coverage for last-used model persistence helpers
- `extensions/dashboard/footer-compact.test.ts` (new) — Coverage for compact footer single-line rendering and inline model display

### Constraints

- Persist only successful explicit model switches; failed switch attempts must not overwrite a working saved model.
- On session_start, restore the persisted concrete model before falling back to effort-tier default routing.
- Compact dashboard footer should remain single-line and dashboard-first while still exposing active model/provider at a glance on wide terminals.
