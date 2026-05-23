+++
id = "b2d219a7-b43b-43ee-bf2e-47ad23f17b67"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Harness upstream error recovery and fallback signaling

## Disposition — 2026-05-23

**Status: current concept / stale implementation scope.** Structured provider-error classification and bounded recovery remain active concerns, and current Rust code includes upstream error classification in `core/crates/omegon/src/upstream_errors.rs` plus model switching in `core/crates/omegon/src/features/model_budget.rs`. The implementation notes below refer to absent TypeScript extension files and should be treated as historical planning, not current file scope.

Use the decisions and constraints as design intent. Reconcile all code references with Rust upstream-error, settings, model-registry, and TUI status surfaces before making implementation changes.

## Overview

Surface upstream driver/provider failures to the agent and operator, classify obvious transient failures for bounded retry, and route sustained usage/limit/backoff issues into intelligent model or driver fallback.

## Research

### Existing recovery primitives and current gap

pi core already persists assistant error turns and has one built-in automatic recovery path for context overflow in agent-session.js: it removes the last error message, compacts, and retries once. Omegon also already records transient provider/model cooldowns in extensions/model-budget.ts via turn_end -> getAssistantErrorMessage() -> recordTransientFailureForModel(), but today that path only notifies the operator. The agent/harness does not receive a structured recovery event, so an upstream Codex/server_error remains opaque to the agent even when Omegon can classify it as transient or route around it.

### Useful existing routing hooks

extensions/lib/model-routing.ts already classifies transient failures with patterns such as 429, rate limit, temporarily unavailable, overloaded, and try again later, and can place both providers and individual candidates on cooldown. extensions/lib/operator-fallback.ts can resolve alternate candidates for the same capability role after a transient failure. Offline-driver.ts can switch execution to local models when cloud reachability fails. The missing layer is a harness-facing recovery controller that turns these primitives into structured events, bounded retries, and explicit model/driver switches the agent can observe.

## Decisions

### Decision: Handle upstream failures through a structured recovery controller

**Status:** decided
**Rationale:** Do not scatter retry and failover logic across individual tools or slash commands. A single recovery controller should observe assistant error turns, classify failures, emit a structured recovery event into the session, and decide whether to retry, switch model/provider, or escalate to the operator.

### Decision: Retry only same-model transient upstream failures; switch on rate-limit/backoff classes

**Status:** decided
**Rationale:** Bounded same-model retry is appropriate for obvious upstream flakiness such as server_error, transient 5xx, timeouts, or overloaded responses. Repeating the same request against a provider that is explicitly rate-limiting or backing off wastes time and tokens. Those cases should instead cool down the failing provider/candidate and resolve an alternate model or local driver through the existing routing profile.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/model-budget.ts` (modified) — replace notify-only transient failure handling with structured recovery classification and fallback planning
- `extensions/lib/model-routing.ts` (modified) — expand failure taxonomy from boolean transient detection to structured classes and retryability metadata
- `extensions/lib/operator-fallback.ts` (modified) — promote alternate-candidate guidance into executable recovery plans
- `extensions/offline-driver.ts` (modified) — export reusable `switchToOfflineDriver()` / `restoreCloudDriver()` helpers so the recovery controller can perform automatic local handoff without duplicating command logic
- `extensions/shared-state.ts` (modified) — store last recovery event / retry budget for dashboard and session injection
- `extensions/dashboard/types.ts` (modified) — define dashboard-facing recovery event and cooldown summary interfaces for shared-state consumers
- `extensions/dashboard/footer.ts` (modified) — surface latest recovery action plus nearest cooldown guidance in compact and raised footer modes
- `extensions/dashboard/footer-dashboard.test.ts` (modified) — verify recovery state shapes and footer rendering expectations
- `extensions/model-budget.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/lib/model-routing.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/harness-upstream-error-recovery.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- The agent must see a structured recovery notice before or with any automatic retry so upstream failures are not invisible in the harness transcript.
- Automatic retries must be bounded and idempotent-aware: at most one same-model retry for obvious upstream flakiness before escalation or failover.
- Provider/model switching should reuse existing capability-role routing and cooldown state instead of hardcoding Codex-specific fallbacks.
- Dashboard-facing recovery state must tolerate partial producer rollout: footer rendering should safely read an optional shared-state payload while sibling recovery-controller work lands.
- Automatic offline handoff must return structured target metadata (provider/model/automatic flag) so the controller and dashboard can describe what changed without scraping human-facing status text.
- Authentication, quota exhaustion, malformed tool results, and context-overflow paths must not be treated as generic transient retry cases.
- The recovery controller usually piggybacks on pi core auto-retry: rate-limit/backoff recovery switches the selected model during `turn_end`, then core `agent_end` retry continues on the newly selected route.
- When a provider surfaces a retryable upstream failure only through a structured code string such as `server_error` (for example Codex JSON error envelopes that do not match pi core's textual retry regex), Omegon schedules one extension-driven same-message retry so recovery still occurs.
- Recovery state is stored in both `sharedState.latestRecoveryEvent` (raw harness event) and `sharedState.recovery` (dashboard projection) so status surfaces do not need to derive UI state from prose.
- Omegon now schedules one extension-driven same-message retry when a retryable upstream failure is encoded only as a structured code string such as Codex JSON server_error and therefore does not match pi core's built-in textual retry regex.
- Bounded same-model retries are ledgered per request fingerprint plus provider/model and the retry ledger is cleared after the next successful assistant turn to avoid indefinite loops across extension-driven retries.
