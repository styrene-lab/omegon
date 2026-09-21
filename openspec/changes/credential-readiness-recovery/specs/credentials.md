# Credential readiness recovery - Delta Spec

## ADDED Requirements

### Requirement: Expired OAuth cannot establish provider readiness

Resolvers and provider clients must never return a known expired OAuth access token after a failed refresh.

#### Scenario: Definitively rejected refresh
Given expired OAuth credentials and a refresh endpoint returning invalid_grant
When the selected provider resolves its credentials
Then resolution reports a terminal refresh failure
And no expired access token is returned or used for inference

#### Scenario: Environment contains a hydrated expired access token
Given an expired stored OAuth credential and the same access token in an OAuth environment variable
When synchronous or asynchronous resolution checks readiness
Then the environment copy does not make that credential usable

### Requirement: Credential precedence supports independent recovery

Explicit API keys and usable stored or external credentials retain their authority. A fresh external credential can replace an expired stored credential before refresh.

#### Scenario: Fresh external credential
Given expired stored OAuth and a fresh external credential
When the selected provider resolves credentials
Then the fresh external credential is returned without refreshing the expired credential
And synchronous and asynchronous resolution agree on the selected source and credential type

### Requirement: Refresh attempts are coalesced and failure-aware

Concurrent requests for one provider and credential generation share refresh work. Terminal failures remain suppressed until the generation changes or an explicit connection retry occurs. Transient failures remain retryable after a bounded interval.

#### Scenario: Concurrent rejected refresh
Given several concurrent resolutions for one expired credential generation
When the refresh endpoint rejects that generation
Then exactly one refresh request is made
And each caller observes the same sanitized terminal classification

#### Scenario: Operator retry or replacement credential
Given a suppressed terminal failure
When the operator explicitly retries connection setup or replaces the credential
Then a subsequent resolution can attempt refresh for the authorized retry or new generation

#### Scenario: Temporary refresh outage
Given a refresh endpoint returning a rate limit or server failure
When resolution attempts refresh
Then the failure is transient
And the expired credential remains unusable
And refresh can retry after the bounded transient interval

### Requirement: Discovery does not refresh unrelated providers

Ordinary provider status and connection inventory must inspect credentials without refreshing them.

#### Scenario: Disconnected inventory
Given expired credentials for several providers and no selected route
When startup or connection inventory projects provider status
Then it makes no OAuth refresh requests
And expired credentials are not marked authenticated

#### Scenario: Unselected startup model limits
Given an empty model selection and an expired Anthropic credential
When interactive startup considers model-limit discovery
Then it does not infer Anthropic as a selected route
And it makes no OAuth refresh request

#### Scenario: Model-limit and ACP status inventory
Given expired provider credentials
When ordinary model-limit or ACP provider-status inventory runs
Then it treats those credentials as unavailable without refreshing them

#### Scenario: Selected saved-route recovery
Given a nonempty saved model selection whose provider needs credential recovery
When startup resolves that requested route
Then it may refresh the requested provider
And unselected providers are not refreshed

### Requirement: Refresh diagnostics exclude sensitive payloads

Refresh failures expose typed classifications and bounded status information without credentials or raw provider bodies.

#### Scenario: Provider echoes sensitive values in an error
Given a refresh error body containing credential-like values
When the failure is logged or displayed
Then the raw body and echoed values are absent
And the diagnostic distinguishes terminal from transient failure
