# provider-route — Delta Spec

## ADDED Requirements

### Requirement: ProviderRoute is the single source of truth for the serving model

The runtime exposes exactly one authoritative `ProviderRoute` value owned by the
`RouteController`, which also owns the bridge handle. Every consumer (TUI footer,
loop stream options, TurnEnd events, session log, `/auth status`) reads a route
snapshot; none re-derives serving-model identity from settings, loop config, or
local copies.

#### Scenario: Footer and loop agree by construction
Given the controller route is Fallback { selected: "openai-codex:gpt-5.5", serving: "anthropic:claude-fable-5" }
When the footer renders and the loop builds StreamOptions for the next turn
Then the footer model label is "claude-fable-5"
And StreamOptions.model is "anthropic:claude-fable-5"
And both values came from the same route snapshot

#### Scenario: Route and bridge cannot diverge
Given a successful login hot-swap is in progress
When the controller installs the new bridge
Then the route transition to Serving and the bridge swap happen in one controller call
And no event observes the new bridge with the old route or vice versa

### Requirement: Startup route decision is total

Startup maps credential state to exactly one of four states — Serving, Fallback,
LoginPending, Disconnected — for every combination of selected-provider credential
state and fallback configuration. No combination panics, silently substitutes a
provider, or leaves the route unset.

#### Scenario: Selected provider has valid credentials
Given the profile selects "openai-codex:gpt-5.5"
And openai-codex credentials are valid
When startup resolves the route
Then the route is Serving { model: "openai-codex:gpt-5.5" }

#### Scenario: No credentials and empty fallback list fails explicitly
Given the profile selects "openai-codex:gpt-5.5"
And openai-codex credentials are missing
And fallback_providers is empty
When startup resolves the route
Then the route is Disconnected
And the operator message names openai-codex, lists the credential sources probed
And the message includes the exact remediation command ("/login openai-codex" or env var name)
And no provider is substituted

#### Scenario: Configured fallback engages loudly
Given the profile selects "openai-codex:gpt-5.5"
And openai-codex credentials are missing
And fallback_providers is ["anthropic"]
And anthropic credentials are valid
When startup resolves the route
Then the route is Fallback { selected: "openai-codex:gpt-5.5", serving: anthropic default model, reason: MissingCredentials }
And a RouteChanged event is emitted
And the footer renders the serving model with a persistent fallback warning

#### Scenario: Fallback list exhausted
Given fallback_providers is ["anthropic", "ollama"]
And no listed provider has usable credentials
When startup resolves the route
Then the route is Disconnected
And the message lists every provider tried and why each failed

#### Scenario: Property — startup matrix is total
Given every combination of selected credential state {valid, expired, missing} and fallback config {empty, with-credentials, without-credentials}
When startup resolves the route
Then exactly one of the four route states results
And no combination produces silent provider substitution

### Requirement: Model-changing surfaces are controller transitions

`/model`, `set_model_tier`, `switch_to_offline_driver`, login, and logout request
transitions from the controller. Command handlers do not mutate `settings.model`
directly.

#### Scenario: Model switch without credentials is refused, not absorbed
Given the route is Serving { model: "anthropic:claude-fable-5" }
And google credentials are missing
When the operator runs /model google:gemini-2.5-pro
Then the route remains Serving { model: "anthropic:claude-fable-5" }
And the operator is told the switch was refused and why
And settings.model still reads "anthropic:claude-fable-5"

#### Scenario: Model switch with credentials re-routes atomically
Given the route is Serving { model: "anthropic:claude-fable-5" }
And openai-codex credentials are valid
When the operator runs /model openai-codex:gpt-5.5
Then the controller detects a bridge for openai-codex, swaps it, and transitions to Serving { model: "openai-codex:gpt-5.5" }
And a RouteChanged event is emitted


### Requirement: Model capability and endpoint selection are separate axes

Model routing SHALL represent requested model capability as a provider-neutral grade and SHALL represent local/upstream/provider choice as endpoint selection. `local` SHALL NOT be accepted as a model grade.

#### Scenario: Local is rejected as a grade
Given the operator invokes `/model grade local`
When the command is parsed
Then the command is rejected with guidance to use `/model provider local` for local development endpoints
And no model intent or active route is changed

#### Scenario: Grade intent can fail over across providers
Given the operator intent is grade `S`, provider selection `auto`, and failover policy `strict-grade`
And the active S-grade endpoint becomes rate-limited
And another healthy authenticated S-grade endpoint exists
When the resolver selects the next route
Then the active route changes to the healthy S-grade endpoint
And the requested grade remains `S`
And the route event records the failed endpoint and failover reason

#### Scenario: Exact model override pins routing
Given the operator invokes `/model openai-codex:gpt-5.4`
When the switch succeeds
Then the model intent records an exact pinned override
And grade/provider auto-resolution does not replace that route until the operator clears or changes the override

### Requirement: Legacy tier surface is removed in 0.27.0

The legacy model-tier commands and tool semantics SHALL NOT remain as compatibility aliases.

#### Scenario: Legacy tier slash commands are unknown
Given any command in `/gloriana`, `/victory`, `/retribution`, `/opus`, `/sonnet`, or `/haiku`
When the operator invokes the command
Then the command is unknown
And the response points operators at `/model grade <F|D|C|B|A|S>` only through normal unknown-command guidance, not automatic translation

#### Scenario: Legacy tier tool is not advertised
Given the tool registry is assembled
When model-control tools are listed
Then `set_model_tier` is absent
And model-control tooling uses the model-intent vocabulary instead

### Requirement: Upstream provider matrix is protocol-profile driven

Most upstream providers SHALL use an OpenAI-compatible protocol adapter with endpoint-specific profiles. Custom adapters SHALL be reserved for materially different APIs such as Anthropic Messages.

#### Scenario: OpenAI-compatible endpoint uses shared adapter
Given an upstream endpoint whose protocol is `OpenAiCompatible`
And its endpoint profile declares unsupported request fields and optional headers
When the resolver selects a model on that endpoint
Then the shared OpenAI-compatible adapter builds the request
And the request sanitizer applies the endpoint profile before dispatch

#### Scenario: Anthropic remains custom below the common route interface
Given the resolver selects an Anthropic model
When the request is dispatched
Then the route layer treats it as an endpoint/model capability row
And the Anthropic adapter handles the native Messages API details below the common route interface
