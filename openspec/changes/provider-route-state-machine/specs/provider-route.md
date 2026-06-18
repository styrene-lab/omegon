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


### Requirement: Model policy semantics are explicit

Grade selection, endpoint failover, and capability degradation SHALL be separate policy concepts. The route resolver SHALL NOT overload a single `strict-grade` value with both failover and degradation behavior.

#### Scenario: Minimum grade permits stronger models
Given the operator intent requests minimum grade `B`
And healthy authenticated `B`, `A`, and `S` candidates exist
When the resolver ranks candidates
Then `A` and `S` candidates remain eligible
And the selected route records that it satisfied a minimum-grade request

#### Scenario: Exact grade rejects adjacent grades
Given the operator intent requests exact grade `B`
And only healthy authenticated `A` candidates exist
When the resolver evaluates candidates
Then no candidate satisfies the grade policy
And degradation handling decides whether to ask, degrade, or fail visibly

#### Scenario: Pinned exact override can be cleared
Given the model intent contains an exact model override
When the operator invokes the chosen unpin command
Then the exact override is cleared
And subsequent resolution uses grade, provider selection, and policy intent

### Requirement: Provider selector namespace is reserved

The provider selector tokens `auto`, `local`, and `upstream` SHALL be reserved and SHALL NOT be valid endpoint IDs.

#### Scenario: Reserved endpoint id is rejected
Given endpoint configuration declares an endpoint id `local`
When profile or registry validation runs
Then validation fails
And the error explains that `local` is reserved for `EndpointClass::LocalDev` selection

#### Scenario: Local provider selector filters by endpoint class
Given the operator invokes `/model provider local`
When model intent is updated
Then provider selection targets enabled endpoints with `EndpointClass::LocalDev`
And `local` is not stored as a provider brand or capability grade

### Requirement: Model intent tool updates atomically

Agent-facing model-control tooling SHALL update model intent through one atomic reducer. Partial grade/provider/policy tool updates SHALL NOT leave committed half-valid intent.

#### Scenario: Invalid provider keeps prior intent
Given current model intent is grade `S`, provider selection `auto`
When an agent requests model intent grade `A` with provider endpoint `missing-provider`
Then the request is rejected
And the prior grade `S`, provider selection `auto` intent remains committed

### Requirement: OpenAI-compatible profiles normalize responses and errors

OpenAI-compatible endpoint profiles SHALL define request shaping, response normalization, and error normalization.

#### Scenario: Provider-specific error becomes route rejection reason
Given an OpenAI-compatible endpoint returns a provider-specific rate-limit error envelope
When the shared adapter handles the response
Then the error profile maps it to `RouteRejectionReason::RateLimited`
And the resolver can apply endpoint cooldown or failover policy

#### Scenario: Streaming tool-call delta is normalized
Given an OpenAI-compatible endpoint streams tool-call deltas with endpoint-specific quirks
When the shared adapter receives the stream
Then the response profile normalizes the stream into Omegon's common tool-call event shape
And downstream loop/tool execution code does not branch on endpoint id

### Requirement: Endpoint auth schemes are data-driven

Endpoint definitions SHALL carry auth scheme metadata rather than requiring provider-specific auth branches for every OpenAI-compatible provider.

#### Scenario: Bearer-token endpoint resolves credential reference
Given endpoint `groq` declares `BearerToken { secret_ref: "GROQ_API_KEY" }`
When the credential ledger probes `groq`
Then it reads the declared secret reference
And no Groq-specific credential branch is required in route resolution

### Requirement: Legacy baseline specs are superseded

The 0.27.0 change SHALL modify or remove baseline requirements that require `/local`, `/haiku`, `/sonnet`, `/opus`, or `set_model_tier` behavior.

#### Scenario: Baseline routing no longer requires legacy tier copy
Given baseline routing specs are assessed after this change
When requirements mention legacy model-tier commands or `set_model_tier`
Then those requirements have been removed or rewritten to the model-intent vocabulary
And implementation that rejects legacy commands is not considered a regression


### Requirement: OpenAI-compatible response and error normalization is profile-driven

OpenAI-compatible endpoint profiles SHALL cover request shaping, response normalization, stream normalization, and provider-specific error normalization. Request shaping alone SHALL NOT satisfy this requirement.

#### Scenario: Request shaping alone is insufficient
Given an OpenAI-compatible endpoint profile strips unsupported request fields
When implementation evidence is assessed
Then the requirement remains incomplete until provider-specific response and error normalization are also implemented

#### Scenario: Error envelope maps to common rejection reason
Given an OpenAI-compatible endpoint returns a provider-specific rate-limit error envelope
When the shared OpenAI-compatible adapter handles the error
Then the endpoint error profile maps it to a common rate-limit/retry category
And route/failover logic can use that category without branching on endpoint id

#### Scenario: Streaming tool-call deltas normalize before tool execution
Given an OpenAI-compatible endpoint streams tool-call deltas using endpoint-specific chunk quirks
When the shared OpenAI-compatible adapter receives the stream
Then the endpoint response profile normalizes those chunks into Omegon's common tool-call event shape
And downstream tool execution does not branch on endpoint id
