+++
id = "accac304-8c26-4b58-a735-07a42b21ec7b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Provider thinking capability matrix

This document is the reviewed snapshot for how Omegon maps its session-level
`ThinkingLevel` abstraction onto upstream provider request contracts.

It complements the checked-in contract file at [[.pi/provider-contracts.json]],
which now also carries a `live_upstream_matrix` section for daily endpoint drift
verification. That JSON is the machine-readable source for probe execution;
this document remains the human-readable explanation of the reasoning controls.

It exists for one reason: **the harness-level notion of `off/minimal/low/medium/high`
is not itself an upstream API**. Each provider exposes different controls, cost
semantics, and capability boundaries.

Related:
- [[cross-provider-session-telemetry-schema]]
- [[context-class-taxonomy-and-routing-policy]]
- [[provider-route-research-brief]]

## Boundary

Omegon supports thinking/reasoning **at the API boundary we actually use**.

That means:
- Anthropic Messages API
- OpenAI Responses/Codex API
- Ollama local server API
- Ollama Cloud native API

That explicitly does **not** mean chasing every bleeding-edge, model-specific,
or undocumented knob in the local Ollama model ecosystem. For Ollama, the cut
boundary is the server API surface we call, not every individual model family’s
latest experimental switch.

## Harness abstraction

Omegon session settings expose:
- `off`
- `minimal`
- `low`
- `medium`
- `high`

This abstraction serves three purposes:
1. operator control (`/thinking`, `set_thinking_level`)
2. TUI/status visibility
3. per-turn provider request shaping

It does **not** imply every provider supports all five values natively.

## Matrix

| Provider / transport | Upstream control surface | Omegon mapping | Notes |
|---|---|---|---|
| Anthropic Claude 4.6 (Sonnet / Opus) | `thinking: { type: "adaptive" }` + `effort` | `minimal/low/medium/high` → effort; no fixed budget by default | Recommended upstream path. Manual `budget_tokens` is deprecated on 4.6. |
| Anthropic older/manual thinking models | `thinking: { type: "enabled", budget_tokens: N }` + `effort` | `minimal`→1024, `low`→5000, `medium`→10000, `high`→50000 | Manual fallback for non-4.6/manual-thinking models. |
| OpenAI Responses / Codex | `reasoning: { effort: ... , summary: "auto" }` | `minimal`, `low`, `medium`, `high`, `xhigh` preserved where supported | Effort support is model-dependent. We do not collapse `minimal` to `low`. |
| Ollama local server API | top-level `think` on chat/generate | GPT-OSS: string level (`low/medium/high`); others: boolean enablement | Boundary is Ollama API, not per-model experimental internals. |
| Ollama Cloud native API | top-level `think` on `/api/chat` | same as local API | Cloud follows the native Ollama API contract, not the OpenAI-compat shim. |

## Provider-specific notes

### Anthropic

Current upstream guidance:
- Claude Sonnet 4.6 and Opus 4.6 should use **adaptive thinking**
- manual `budget_tokens` remains functional but is deprecated for 4.6
- `effort` is the preferred depth control

Operationally, Omegon defaults 4.6 models to adaptive thinking partly because
Anthropic bills thinking against output-token pricing. For operators, that means
adaptive thinking is both a capability alignment choice and a cost-control
choice.

### OpenAI

OpenAI reasoning effort is model-dependent. Supported values can include:
- `none`
- `minimal`
- `low`
- `medium`
- `high`
- `xhigh`

Omegon preserves `minimal` where the upstream supports it rather than flattening
it into `low`.

### Ollama local / Ollama Cloud

Ollama exposes native thinking controls via top-level `think`.

Important exception:
- GPT-OSS expects string levels (`low`, `medium`, `high`) and ignores booleans.

Most other Ollama thinking-capable models can be treated conservatively via
boolean enablement at the API layer.

## Non-goals

These are intentionally out of scope for Omegon’s provider thinking layer:
- maintaining a bleeding-edge per-model Ollama feature catalog
- inventing pseudo-unified token budgets across unlike providers
- assuming that harness `ThinkingLevel::budget_tokens()` is a wire contract

## Implementation pointers

Current implementation lives in:
- `core/crates/omegon/src/loop.rs` — per-turn settings → `StreamOptions.reasoning`
- `core/crates/omegon/src/providers.rs` — provider-native request shaping
- `core/crates/omegon/src/settings.rs` — `ThinkingLevel` and heuristic reserve
- `core/crates/omegon/src/features/usage.rs` — operator-facing usage notes

## Important constraint

`ThinkingLevel::budget_tokens()` in `settings.rs` is a **heuristic reserve for
context assembly**, not the source of truth for upstream request parameters.
Provider-native shaping must remain in `providers.rs`.
