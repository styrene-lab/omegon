+++
id = "340427d5-b3d7-482f-bf17-9a824a3945b9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Model Routing

> Provider-aware model selection, effort tiers, cost control, and intelligent fallback — the control plane for all inference decisions across Omegon.

## What It Does

Model routing manages which models execute which tasks across the entire harness. It operates at three layers:

1. **Effort Tiers** (`/effort`): A single global knob (Servitor → Omnissiah) controlling the local-vs-cloud inference ratio. Sets caps on driver model, extraction, compaction, cleave children, and review loops.

2. **Provider Resolution**: Abstract capability tiers (local, haiku, sonnet, opus) resolve to concrete model IDs through a provider-aware registry. Supports Anthropic, OpenAI/Codex, and local Ollama with ordered preference.

3. **Fallback & Recovery**: When a provider fails, the system applies bounded retry for transient errors, rate-limit cooldowns, and operator-gated fallback to alternate providers. Request fingerprinting prevents retry loops.

The system also controls compaction model selection (local → GPT → Haiku fallback chain) and memory extraction model routing.

## Key Files

| File | Role |
|------|------|
| `extensions/model-budget.ts` | Budget tracking, recovery controller, retry ledger, `RecoveryFailureClassification` mapping |
| `extensions/lib/model-routing.ts` | Transient failure classification, provider cooldowns, candidate resolution |
| `extensions/lib/operator-fallback.ts` | Alternate candidate resolution after provider failure |
| `extensions/lib/operator-profile.ts` | Operator capability profile — provider preferences, role mappings |
| `extensions/lib/model-preferences.ts` | Model preference storage and retrieval |
| `extensions/lib/local-models.ts` | Local Ollama model discovery and management |
| `extensions/effort/` | Effort tier extension — `/effort` command, tier resolution |
| `extensions/offline-driver.ts` | Switch to local models on cloud reachability failure |
| `extensions/shared-state.ts` | `RecoveryFailureClassification` type, shared recovery state |

## Design Decisions

- **Public tier semantics stable, provider resolution underneath**: Agent-facing tiers (local/haiku/sonnet/opus) never change. Provider choice is a session-level policy resolved at runtime, not hardcoded.
- **Effort tiers coexist with model-budget**: Effort sets caps and preferences; model-budget enforces budgets and handles failures. Clean interface boundary — effort doesn't import model-budget internals.
- **`/effort cap` locks tier, agent downgrades only**: Operator sets a ceiling; the agent can voluntarily use cheaper tiers but never exceed the cap.
- **Intelligent compaction fallback chain**: Compaction tries local first (cheapest), falls back to GPT-5.3 (good quality), then Haiku (always available). Heavy local compaction disabled by default to avoid GPU contention.
- **Request fingerprint + provider/model retry ledger**: Each API call gets a fingerprint; retries tracked per-fingerprint per-provider to prevent loops. Ledger clears on next successful turn.
- **`invalid-request` is non-retryable**: 400-class errors (oversized images, malformed payloads) surface actionable guidance immediately, no blind retry.

## Behavioral Contracts

See `openspec/baseline/effort.md`, `openspec/baseline/routing.md`, `openspec/baseline/routing/spec.md`, and `openspec/baseline/models/profile.md` for Given/When/Then scenarios covering:
- Effort tier transitions and cap enforcement
- Provider fallback ordering
- Transient failure classification and retry bounds
- Operator profile role resolution

## Constraints & Known Limitations

- Pi core auto-retry exists in `agent-session.js` but extensions cannot directly tune its classification-specific retry policy
- Same-model retries bounded to one attempt — no indefinite loops
- Auth, quota, malformed output, context-overflow are not treated as generic transient retries
- Image dimensions must be under 8000px for Anthropic's API
- Provider cooldowns use a fixed 5-minute window in v1

## Related Subsystems

- [Error Recovery](error-recovery.md) — failure classification and recovery signaling
- [Dashboard](dashboard.md) — displays current model, tier, and recovery state
- [Cleave](cleave.md) — child model selection respects effort tiers
- [Project Memory](project-memory.md) — extraction/compaction model routing
- [Operator Profile](operator-profile.md) — provider preferences and fallback policy
