+++
id = "750a74b0-f623-4ce7-8338-03e690d0fd3a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# harness-upstream-error-recovery — Design

## Spec-Derived Architecture

### harness/upstream-error-recovery

- **Upstream driver failures are surfaced as structured recovery events** (added) — 1 scenarios
- **Obvious upstream flakiness retries at most once on the same model** (added) — 1 scenarios
- **Rate limits and explicit backoff trigger failover rather than blind retry** (added) — 1 scenarios
- **Non-transient failures are not misclassified as generic retry cases** (added) — 1 scenarios
- **Recovery state is visible to dashboard consumers** (added) — 1 scenarios

## Architecture Decisions

### Decision: Handle upstream failures through a structured recovery controller

**Status:** decided  
**Rationale:** Do not scatter retry and failover logic across individual tools or slash commands. A single recovery controller should observe assistant error turns, classify failures, emit a structured recovery event into shared/session state, and decide whether to retry, switch model/provider, or escalate to the operator.

### Decision: Retry only same-model transient upstream failures; switch on rate-limit/backoff classes

**Status:** decided  
**Rationale:** Bounded same-model retry is appropriate for obvious upstream flakiness such as `server_error`, transient 5xx responses, overloads, or transport timeouts. Repeating the same request against a provider that is explicitly rate limiting or backing off wastes time and tokens; those cases should cool down the failing candidate and resolve an alternate model or local driver through existing routing policy.

## Research Context

### Existing recovery primitives and current gap

pi core already persists assistant error turns and has one built-in automatic recovery path for context overflow in `agent-session.js`: it removes the last error message, compacts, and retries once. pi-kit also already records transient provider/model cooldowns in `extensions/model-budget.ts` via `turn_end -> getAssistantErrorMessage() -> recordTransientFailureForModel()`, but today that path only notifies the operator. The agent/harness does not receive a structured recovery event, so an upstream Codex `server_error` remains opaque to the agent even when pi-kit can classify it as transient or route around it.

### Useful existing routing hooks

`extensions/lib/model-routing.ts` already classifies transient failures with patterns such as 429, rate limit, temporarily unavailable, overloaded, and try again later, and can place both providers and individual candidates on cooldown. `extensions/lib/operator-fallback.ts` can resolve alternate candidates for the same capability role after a transient failure. `extensions/offline-driver.ts` can switch execution to local models when cloud reachability fails. The missing layer is a harness-facing recovery controller that turns these primitives into structured events, bounded retries, and explicit model/driver switches the agent can observe.

## File Changes

- `extensions/lib/model-routing.ts` (modified) — replace boolean transient detection with structured failure classification and retry/failover metadata
- `extensions/lib/model-routing.test.ts` (modified) — cover retryable vs non-retryable classes, rate-limit/backoff detection, and server-error classification
- `extensions/lib/operator-fallback.ts` (modified) — translate classified failures and cooldown state into executable recovery plans rather than notify-only guidance
- `extensions/lib/operator-fallback.test.ts` (modified) — cover alternate-candidate and provider-cooldown recovery plans
- `extensions/model-budget.ts` (modified) — observe assistant error turns, emit structured recovery notices, perform bounded retry/failover decisions, and persist latest recovery state
- `extensions/model-budget.test.ts` (modified) — verify retry-once semantics, non-retryable exclusions, and structured recovery emission
- `extensions/offline-driver.ts` (modified) — expose automatic local handoff helper for cloud-failure recovery when policy permits
- `extensions/shared-state.ts` (modified) — store the latest harness recovery event for dashboard and other extensions
- `extensions/dashboard/types.ts` (modified) — add dashboard-facing recovery state types
- `extensions/dashboard/footer.ts` (modified) — surface latest recovery status and cooldown guidance in footer/dashboard views
- `extensions/dashboard/footer-dashboard.test.ts` (modified) — verify recovery state rendering in dashboard summaries
- `docs/harness-upstream-error-recovery.md` (modified) — design-tree node bound to the implementation and updated with concrete constraints

## Constraints

- The agent must see a structured recovery notice before or with any automatic retry so upstream failures are not invisible in the harness transcript.
- Automatic retries must be bounded and idempotent-aware: at most one same-model retry for obvious upstream flakiness before escalation or failover.
- Provider/model switching should reuse existing capability-role routing and cooldown state instead of hardcoding Codex-specific fallbacks.
- Authentication, quota exhaustion, malformed tool results, and context-overflow paths must not be treated as generic transient retry cases.
- Recovery handling must preserve pi core’s existing context-overflow auto-compaction path rather than duplicating or masking it.
