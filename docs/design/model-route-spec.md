# Model Route Spec Internal State

## Status

Implemented incrementally from the GitHub Copilot provider-prefix hardening work.

## Problem

Provider route state previously stored model routes as raw `String` values. That made malformed nested provider strings representable inside authoritative route state, for example:

```text
anthropic:github-copilot:gpt-5.5
```

This shape conflates producer metadata (`anthropic`) with the serving provider (`github-copilot`). Once present in route state, downstream consumers could infer the wrong provider, probe the wrong credentials, or mis-rank model capability.

## Decision

Route state stores provider-qualified model IDs as `ModelRouteSpec`, not raw `String`.

`ModelRouteSpec` canonicalizes at construction through the shared provider/model normalization path. Route state variants therefore hold canonical specs:

```rust
ProviderRoute::Serving { model: ModelRouteSpec }
ProviderRoute::Fallback { selected: ModelRouteSpec, serving: ModelRouteSpec, .. }
ProviderRoute::Disconnected { selected: ModelRouteSpec, .. }
```

External/UI/event/protocol surfaces convert back to string explicitly via `as_str()`, `Display`, or `to_string()`.

## Invariant

Nested provider-prefix strings must be unrepresentable in `ProviderRoute` state. If such a string reaches a route ingress boundary, construction canonicalizes it before storage:

```text
anthropic:github-copilot:gpt-5.5 -> github-copilot:gpt-5.5
github-copilot:anthropic:claude-sonnet-4-6 -> github-copilot:claude-sonnet-4-6
```

## Boundaries

Canonicalization is enforced at:

- startup route resolution
- login completion route installation
- intent-candidate route installation
- model switching
- logout/disconnect route state
- direct `ProviderRoute` test construction through `From<&str>` / `From<String>`

## Tradeoffs

- This is intentionally narrower than a full model registry route object. It preserves existing external string contracts while hardening internal state.
- `ModelRouteSpec` still stores a string internally, but construction centralizes normalization and makes accidental raw route storage harder.
- Future work can split the wrapper into `{ provider_id, model_id }` fields once all external string contracts have DTO boundaries.
