+++
id = "56da7422-2b2c-4b77-962f-1d1f92806775"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# harness/upstream-error-recovery — Delta Spec

## ADDED Requirements

### Requirement: Upstream driver failures are surfaced as structured recovery events
When an assistant turn fails because an upstream model provider or driver returns an error, pi-kit must emit a structured recovery event that is visible to both the operator and the harness/agent.

#### Scenario: upstream server error becomes a structured recovery notice
Given the current model returns an assistant error caused by an upstream `server_error`
When pi-kit handles the failed turn
Then it records a structured recovery event containing the provider, model, error classification, and original error summary
And the operator can see that recovery state in dashboard or status surfaces
And the harness can observe that recovery notice without scraping terminal prose

### Requirement: Obvious upstream flakiness retries at most once on the same model
pi-kit may retry same-model execution only for bounded, obviously transient upstream failures such as server errors, transient 5xx responses, overloads, or transport timeouts.

#### Scenario: same-model retry is attempted once for a transient upstream failure
Given an assistant turn fails with a retryable transient upstream failure
And the failure is not classified as rate limiting, quota exhaustion, authentication failure, malformed output, or context overflow
When pi-kit applies recovery
Then it emits a structured recovery notice before or with the retry
And it retries the failed turn at most once on the same provider/model
And repeated failures of the same turn do not loop indefinitely

### Requirement: Rate limits and explicit backoff trigger failover rather than blind retry
Failures that indicate throttling, provider backoff, or temporary usage exhaustion must cool down the failing candidate and prefer an alternate route.

#### Scenario: rate-limited provider is cooled down and an alternate candidate is selected
Given an assistant turn fails with a 429, rate-limit, session-limit, or explicit try-again-later error
When pi-kit applies recovery
Then it does not immediately retry the same provider/model
And it records provider and candidate cooldown state for routing
And it resolves an alternate candidate through the existing capability profile when one is available
And if only a local driver remains viable, pi-kit may hand off to the offline driver path

### Requirement: Non-transient failures are not misclassified as generic retry cases
Authentication, quota exhaustion, malformed tool/schema results, and context overflow must not be treated as ordinary transient upstream retries.

#### Scenario: non-retryable failures are surfaced without generic transient retry
Given an assistant turn fails because of authentication failure, hard quota exhaustion, malformed tool output, or context overflow
When pi-kit classifies the failure
Then it marks the event as non-retryable or separately handled
And it does not apply the generic same-model transient retry path
And any guidance to switch models or compact context remains explicit and classification-specific

### Requirement: Recovery state is visible to dashboard consumers
Dashboard and shared state consumers must be able to render the current recovery condition and any active cooldown guidance.

#### Scenario: dashboard sees latest recovery state and cooldowns
Given pi-kit has handled an upstream driver or provider failure
When dashboard consumers read shared state
Then they can access the latest structured recovery event
And they can render whether pi-kit retried, switched candidate, cooled down a provider, or escalated to the operator
