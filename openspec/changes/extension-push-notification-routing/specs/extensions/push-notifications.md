# Extensions Push Notifications — Delta Spec

## ADDED Requirements

### Requirement: Extension notifications are routable

Omegon SHALL preserve JSON-RPC notifications emitted by extension processes and route recognized notifications to registered host consumers without interfering with ordinary request/response RPC calls.

#### Scenario: Unknown notification is ignored safely
Given an extension emits a JSON-RPC notification with an unknown method
When the host receives the notification
Then the extension transport remains usable
And no daemon event is injected.

#### Scenario: Request response matching ignores notifications
Given an extension emits a notification before the response for an in-flight host request
When the host waits for the request response
Then the notification is handled separately
And the host still returns the matching response by id.

### Requirement: Voice notifications are capability gated

Omegon SHALL only route voice notifications from extensions that declare voice capability.

#### Scenario: Extension without voice capability is ignored
Given an extension does not declare `capabilities.voice = true`
When it emits `voice/transcription`
Then no daemon prompt event is injected.

#### Scenario: Extension with voice capability is routed
Given an extension declares `capabilities.voice = true`
When it emits `voice/transcription`
Then the notification is eligible for voice bridge conversion.
