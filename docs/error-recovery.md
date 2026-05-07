+++
id = "2edde31a-96fa-4154-b1f0-c3eb8c192d64"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/harness-upstream-error-recovery.md"]
last_updated = "2026-03-10"
openspec_baselines = ["harness/upstream-error-recovery.md"]
subsystem = "error-recovery"
+++

# Error Recovery

> Structured upstream failure classification, bounded retry, provider fallback, and recovery state signaling to agent and operator.

## What It Does

When an upstream provider (Anthropic, OpenAI, Ollama) returns an error, the error recovery system:

1. **Classifies** the failure into a specific category (context-overflow, auth, quota, rate-limit, backoff, image-too-large, invalid-request, retryable-flake, non-retryable)
2. **Decides** the recovery action: retry same model (bounded to 1 attempt), fail over to alternate provider, or surface to operator
3. **Emits** a structured recovery event that the agent and dashboard can observe
4. **Executes** the recovery action (retry, model switch, or structured error message with guidance)

Classification chain order: context-overflow → auth → quota → tool-output → rate-limit → backoff → image-too-large → invalid-request → retryable-flake → non-retryable.

## Key Files

| File | Role |
|------|------|
| `extensions/model-budget.ts` | Recovery controller, failure classification dispatch, retry ledger, recovery event emission |
| `extensions/lib/model-routing.ts` | Pattern-based failure classification (HTTP status codes, error message patterns) |
| `extensions/shared-state.ts` | `RecoveryFailureClassification` type definition |
| `extensions/lib/operator-fallback.ts` | Alternate candidate resolution for failover |

## Design Decisions

- **Structured recovery controller**: Centralized failure classification with recovery-event emission, retry/failover, and operator/harness visibility. Not scattered across individual error handlers.
- **Retry only bounded same-model transient failures**: Obvious upstream flakiness (server_error, temporarily unavailable) retries once. Rate-limit/backoff classes fail over to alternate providers instead.
- **Extension-driven retry fallback for structured-code failures**: When retryable failures are only structured strings (e.g., Codex JSON `server_error`), model-budget queues one bounded retry.
- **Image-specific pattern matches before generic `invalid_request_error`**: Ensures targeted guidance for the common >8000px case before falling through to generic 400-class handling.

## Behavioral Contracts

See `openspec/baseline/harness/upstream-error-recovery.md` for Given/When/Then scenarios covering:
- Failure classification by error type
- Retry bounds and loop prevention
- Recovery event structure
- Operator notification format

## Constraints & Known Limitations

- Pi core has its own auto-retry for context overflow — extension-level recovery must not conflict
- Same-model retries capped at 1 to prevent loops
- Authentication and quota errors are never retried (non-transient)
- Recovery events are emitted via `pi.events` — consumers must subscribe to receive them

## Related Subsystems

- [Model Routing](model-routing.md) — provides failure classification patterns and provider cooldowns
- [Dashboard](dashboard.md) — displays recovery state and cooldown timers
- [Operator Profile](operator-profile.md) — fallback policy determines which alternate providers are allowed
