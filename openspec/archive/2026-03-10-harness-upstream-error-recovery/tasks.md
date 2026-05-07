+++
id = "51907651-b669-4fea-bb82-145d6a8eb977"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# harness-upstream-error-recovery — Tasks

## 1. Failure classification and recovery-plan primitives
<!-- specs: harness/upstream-error-recovery -->

- [x] 1.1 Replace boolean transient detection in `extensions/lib/model-routing.ts` with structured failure classes that distinguish retryable upstream flake, rate-limit/backoff failover, and non-retryable errors
- [x] 1.2 Update `extensions/lib/operator-fallback.ts` to convert classified failures plus cooldown state into executable recovery plans and alternate-candidate guidance
- [x] 1.3 Add or update tests in `extensions/lib/model-routing.test.ts` and `extensions/lib/operator-fallback.test.ts` covering `server_error`, 5xx/timeout overloads, 429/rate-limit/backoff, and non-retryable auth/quota/context-overflow cases

## 2. Harness recovery controller and structured event emission
<!-- specs: harness/upstream-error-recovery -->

- [x] 2.1 Extend `extensions/model-budget.ts` so assistant error turns produce a structured recovery event instead of notify-only cooldown handling
- [x] 2.2 Implement bounded same-model retry-once behavior for retryable upstream failures, with a structured notice emitted before or with the retry
- [x] 2.3 Ensure non-retryable failures bypass the generic transient retry path while preserving explicit classification-specific guidance
- [x] 2.4 Add or update tests in `extensions/model-budget.test.ts` for structured recovery emission, retry-once limits, and non-retryable exclusions

## 3. Shared state, local handoff, and dashboard visibility
<!-- specs: harness/upstream-error-recovery -->

- [x] 3.1 Add recovery-state storage to `extensions/shared-state.ts` for the latest recovery event and retry/failover outcome
- [x] 3.2 Extend `extensions/offline-driver.ts` with a reusable automatic local-handoff path for cloud recovery when routing policy permits
- [x] 3.3 Update `extensions/dashboard/types.ts` and `extensions/dashboard/footer.ts` to surface current recovery state and cooldown guidance to operators
- [x] 3.4 Add or update dashboard-focused tests, including `extensions/dashboard/footer-dashboard.test.ts`, for recovery-state rendering

## 4. Lifecycle reconciliation
<!-- specs: harness/upstream-error-recovery -->

- [x] 4.1 Update `docs/harness-upstream-error-recovery.md` so the design-tree node reflects the final implementation scope and constraints
- [x] 4.2 Reconcile OpenSpec lifecycle artifacts after implementation and before archive
