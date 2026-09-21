# Provider connections delta

## ADDED Requirements

### Requirement: Startup provider output is bounded by the current route

Interactive startup must show at most one provider route summary and one actionable
route diagnostic before terminal wrapping. Available-provider catalog size must not
increase this output. Both layouts and both detail levels follow this policy.

#### Scenario: Usable route and unrelated unconfigured providers
Given a usable selected route and any number of unrelated providers without credentials
When either TUI starts at Active or Full detail
Then startup shows the current route without enumerating those providers
And unrelated missing credentials generate no startup warnings

#### Scenario: No selected route
Given no selected provider route
When the TUI starts
Then startup gives one concise prompt to use /connect
And it does not automatically open a menu or browser

#### Scenario: Selected credentials expired
Given expired credentials for the selected route
When the TUI starts
Then startup shows one diagnostic with a scoped /connect action
And other startup owners do not repeat that credential diagnostic

#### Scenario: Fallback is serving
Given a selected route and a different serving fallback route
When the TUI starts
Then the compact route summary distinguishes selected and serving routes
And detailed fallback evidence remains available through explicit diagnostics

#### Scenario: First prompt has no usable provider
Given NullBridge is the active bridge
When the operator submits a prompt
Then the response points to /connect without listing suggested providers

#### Scenario: Explicit diagnostics retain inventory
Given a populated provider inventory
When an operator requests the existing detailed harness status
Then the response retains provider status and provenance information

### Requirement: Connection discovery uses the shared menu on demand

The /connect menu must distinguish existing connections from available providers,
reuse shared menu interaction, and preserve credential and route state semantics.

#### Scenario: Existing and expired connections
Given configured, expired, and never-configured providers
When the operator opens /connect
Then the initial view contains configured and expired connections plus Add provider
And never-configured providers appear only in the available-provider view
And configured credentials are not represented as proof of live service health

#### Scenario: Search available providers
Given the Connections view is open
When the operator selects Add provider
Then the shared searchable provider view opens
And opening and filtering it do not change credentials or routes or launch a browser

#### Scenario: Provider selection starts connection setup
Given an existing or available provider row in /connect
When the operator activates that row's primary action
Then the provider's connection setup opens rather than its model selector
And providers with multiple supported authentication methods offer an explicit method choice
And browsing or canceling that choice does not launch OAuth or submit credentials
And model selection remains available through /model or an explicit model action

#### Scenario: Anonymous hosted model choice remains explicit
Given the Connections view contains the free hosted option
When the operator activates that option
Then the reviewed free-model selector opens with its existing terms and admission checks

#### Scenario: Inline cancellation preserves work
Given an unsent draft in either TUI presentation
When the operator opens and cancels the connection menu
Then the original draft and route remain intact
And the original terminal presentation is restored

### Requirement: Route diagnostics use operator language

Route status and failed connection or model actions must describe the usable route
and recovery action without Rust debug structures or duplicated route headings.
Detailed model intent remains available in explicit status.

#### Scenario: Unselected route diagnostic
Given no selected model and no serving provider
When route status is requested
Then the route summary says no provider is connected and directs the operator to /connect
And it contains no empty provider identifier or Rust enum field dump

#### Scenario: Selected route cannot serve
Given a selected route with missing or expired credentials or unavailable service
When route status is requested
Then the summary names the selected route and describes its problem in plain language
And it gives a connection recovery action without exposing internal enum names

### Requirement: Connect uses established authentication boundaries

/connect must be discoverable through the command registry and dispatch setup through
the existing authorized authentication path. User secrets must stay out of ordinary
composer history and transcript output.

#### Scenario: Direct provider setup
Given a recognized provider with an available authentication method
When the operator invokes /connect with that provider identifier
Then the existing provider authentication flow runs under its established authorization
And any resulting route change still passes existing route admission

#### Scenario: Local or externally managed provider
Given a provider whose configuration is local or whose interactive authentication handler is unavailable
When the operator invokes /connect with its identifier
Then the TUI gives external-configuration guidance without dispatching authentication
And endpoint URLs are never treated as API-key fields

#### Scenario: Provider aliases
Given a supported provider alias or differently cased provider identifier
When the operator invokes /connect with that identifier
Then the canonical provider determines the authentication method and secret name

#### Scenario: API key entry
Given a provider that accepts an API key
When the operator starts its connection flow
Then key entry uses the hidden secret-input surface
And the provider console opens only after a separate explicit action
And submitted secret values do not enter ordinary history or transcript output

#### Scenario: Remote interaction is unavailable
Given a remote or ACP caller without the secure interaction required by a provider
When the caller requests connection setup
Then the result explains the interaction limitation without claiming success
And command authorization is not weakened by the /connect entry

#### Scenario: Compatibility entry
Given an existing /login or /auth login invocation
When that invocation is dispatched during this migration stage
Then its existing authentication behavior remains available
And new setup guidance and menu discovery prefer /connect
