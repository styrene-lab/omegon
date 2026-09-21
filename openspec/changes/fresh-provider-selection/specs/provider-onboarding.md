# Provider onboarding - Delta Spec

## ADDED Requirements

### Requirement: Embedded native providers retain executable admission

Embedded providers with native authentication and transport implementations must
use those implementations even when catalog metadata also declares an HTTP endpoint.
Custom manifests retain their configured endpoint and secret-binding admission.

#### Scenario: Codex OAuth route selected
Given usable Codex OAuth credentials and an embedded Codex model route
When the operator selects that route
Then native OAuth bridge admission succeeds without requiring a manifest API-key binding
And selecting the route does not submit an inference request

#### Scenario: Custom endpoint resembles a native provider
Given a custom provider manifest with an HTTP endpoint
When its route is admitted
Then the configured manifest endpoint and secret binding remain authoritative
And native-provider routing does not bypass manifest admission

#### Scenario: Discovery refresh preserves declared route identity
Given an embedded or operator-declared offering with an endpoint and native model identifier
When provider discovery refreshes that offering
Then its declared endpoint and native model identifier remain unchanged
And discovery can still update availability and capability evidence
And newly discovered offerings may introduce their own identity

#### Scenario: Fable 5.1 supports ordinary tool requests
Given the embedded Fable 5.1 or same-capability Mythos 5.1 offering with usable credentials
When an admitted native request includes ordinary tools
Then its declared tool capability permits request validation
And no forced-tool selection is introduced

### Requirement: Fresh startup has no implicit provider
Omegon shall represent an absent model selection without choosing Anthropic or another hosted provider.

#### Scenario: Unconfigured launch
Given no explicit CLI model and no saved model
When the operator launches om or omegon
Then the composer offers Choose a connection
And no model, thinking level, or context capacity is presented as usable
And no hosted inference is requested

#### Scenario: Expired selected connection
Given a saved model whose credentials cannot produce a working route
When startup completes
Then the interface identifies the disconnected state
And it does not advertise the selected model as ready

### Requirement: Model selection has explicit precedence
Explicit CLI selection shall override a saved selection. An absent CLI selection shall retain
the saved model. Choosing a provider without specifying a model shall use its shared registry default.

#### Scenario: Explicit override
Given a profile containing a different model
When startup receives an explicit model argument
Then the selected model equals the CLI argument
And an unavailable explicit selection is not silently replaced

#### Scenario: Saved selection
Given a saved model and no model argument
When settings initialize
Then the saved model remains selected

#### Scenario: Provider default
Given an operator chooses a provider without a model
When its default model is resolved
Then the model comes from the shared registry
And no duplicated startup model constant overrides it

#### Scenario: Unselected persistence
Given no selected model
When settings are captured into a profile
Then no empty or fabricated provider-model pair is persisted

### Requirement: Disconnected submission preserves operator work
Connection setup shall occur before dispatching a disconnected draft. Commands remain usable.

#### Scenario: Submit draft without a route
Given a disconnected composer containing a draft and attachments
When the operator submits the message
Then connection choices open
And the draft and attachments remain intact
And no conversation turn or provider request is created

#### Scenario: Cancel connection setup
Given connection setup opened from a draft
When the operator cancels
Then the unchanged draft is available for editing

### Requirement: Free hosted models are an explicit connection choice
Connection setup shall offer existing connections, local routes, free hosted models, and adding
another provider. Zen free choices shall identify the host and applicable data-use terms.

#### Scenario: Browse free hosted models
Given an unconfigured session
When the operator opens free hosted models
Then only curated anonymous-eligible zero-priced models present in the current catalog are selectable
And each choice identifies OpenCode Zen and its data-use policy
And browsing sends no inference request

#### Scenario: Catalog unavailable
Given the Zen catalog cannot be retrieved within a bounded deadline or is invalid
When the operator browses free models
Then the interface reports the failure with a retry path
And no paid or unverified model becomes selectable

### Requirement: Anonymous free routes use ordinary execution contracts
An explicitly selected free model shall use the normal provider admission, streaming, and tool-call
contracts. Free routes shall not fall back to paid providers, even if general fallbacks exist.

#### Scenario: Free model executes
Given an explicitly selected eligible free model
When a turn streams text and a tool call
Then the normal loop receives text and structured tool arguments
And no account key is required

#### Scenario: Free model withdrawn
Given a previously selected free model is absent from the live catalog
When its route is prepared
Then execution reports that the free model is unavailable
And no paid fallback request occurs

#### Scenario: Free model withdrawn during a session
Given a serving free model is withdrawn after selection
When the next provider request detects its removal or definitive access rejection
Then the authoritative route becomes disconnected
And the composer no longer presents that model as ready
And a later draft is retained for connection setup

#### Scenario: Free model throttled
Given an eligible free endpoint responds with a rate-limit error
When the request completes
Then the operator receives a bounded actionable failure
And no paid fallback request occurs
