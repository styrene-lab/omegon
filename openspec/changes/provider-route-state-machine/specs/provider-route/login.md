# provider-route/login — Delta Spec

## ADDED Requirements

### Requirement: Login attempts are stateful with terminal outcomes

A `/login` creates a tracked attempt. The route transitions to
LoginPending { provider, since, prior } and reaches exactly one terminal outcome:
Succeeded (controller hot-swaps and transitions to Serving) or
Failed { timeout | stale_state | refused } (route reverts to `prior`).

#### Scenario: Successful login transitions to Serving
Given the route is Fallback { selected: "openai-codex:gpt-5.5", serving: "anthropic:claude-fable-5" }
When the operator runs /login openai-codex and completes the browser flow with a matching state callback
Then the attempt outcome is Succeeded
And the controller swaps the bridge and transitions to Serving { model: "openai-codex:gpt-5.5" }
And the persistent fallback warning is cleared

#### Scenario: Login timeout reverts and warns persistently
Given the route is Fallback { selected: "openai-codex:gpt-5.5", serving: "anthropic:claude-fable-5" }
When a /login openai-codex callback listener times out
Then the attempt outcome is Failed { reason: timeout }
And the route reverts to the prior Fallback state
And a persistent footer warning reports the failed login until the next attempt
And the warning is not merely a scrolling notification

#### Scenario: Stale-tab callback does not satisfy the attempt
Given a /login attempt is pending with state S2
When a callback arrives carrying state S1 from an earlier login tab
Then the callback is answered with 409 and the attempt remains pending
And if no matching callback arrives before the deadline the outcome is Failed { reason: stale_state_only }
And the failure message tells the operator to close old login tabs and retry

#### Scenario: Login state is queryable
Given a /login attempt is pending
When the operator runs /auth status
Then the output shows LoginPending with the provider and elapsed time
And after a terminal outcome the output shows the outcome and reason

### Requirement: CredentialLedger reports structured per-provider state

Credential probing returns {Valid, Expired(refreshable), Missing} per provider from
the env/auth.json/external merge, re-probed on every route decision — never cached
across transitions.

#### Scenario: External credentials adopted with correct state
Given no CHATGPT_OAUTH_TOKEN env var and no openai-codex entry in auth.json
And valid Codex CLI credentials exist in the external path
When the ledger probes openai-codex
Then the state is Valid with source "external"

#### Scenario: Expired OAuth is Expired, not Missing
Given auth.json holds an expired anthropic OAuth token with a refresh token
When the ledger probes anthropic
Then the state is Expired(refreshable)
And route decisions may attempt refresh before declaring the provider unusable

#### Scenario: External mutation is observed without restart
Given the ledger probed openai-codex as Missing during startup
And Codex CLI credentials are written externally afterwards
When the operator requests a model switch to openai-codex
Then the ledger re-probes and reports Valid
And the switch succeeds
