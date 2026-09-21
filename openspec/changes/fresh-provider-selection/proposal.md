# Fresh startup and free hosted connections

## Intent

A fresh install hardcodes an obsolete Anthropic model and displays model capacity even
when no bridge can serve it. Follow OpenCode's connection-first model catalog and offer
its public Zen free models through the existing provider architecture.

## Scope

Unselected startup, explicit CLI/profile precedence, registry-backed provider defaults,
draft-preserving connection setup, existing/local connection discovery, and explicit
anonymous Zen free-model selection. Preserve paid connections and named profile compatibility.
No automatic hosted inference, paid fallback from free routes, new gateway, or GUI test windows.

## Success criteria

- No configured selection means a quiet disconnected composer, without invented model telemetry.
- Submitting while disconnected preserves the draft and opens connection setup without inference.
- Explicit model selection wins over a saved profile; provider defaults come from one registry.
- Free hosted choices show current eligible Zen models and their data-use terms before selection.
- Anonymous Zen routes support the ordinary streaming/tool loop and fail closed on withdrawal.
- Tests and isolated terminal captures prove behavior without paid inference or operator steps.
